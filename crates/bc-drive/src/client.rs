//! Drive v3 client.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::io::AsyncWriteExt;

use crate::oauth::Auth;
use crate::token_store::TokenStore;
use crate::{DriveError, Result, FOLDER_MIME, SHORTCUT_MIME};

const API_BASE: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3";
const FILE_FIELDS: &str =
    "id,name,mimeType,parents,size,md5Checksum,modifiedTime,trashed,shortcutDetails,thumbnailLink,driveId";
const MAX_ATTEMPTS: u32 = 6;

fn de_size<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<i64>, D::Error> {
    // Drive returns int64 values as JSON strings.
    let v: Option<serde_json::Value> = Option::deserialize(d)?;
    Ok(match v {
        Some(serde_json::Value::String(s)) => s.parse().ok(),
        Some(serde_json::Value::Number(n)) => n.as_i64(),
        _ => None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutDetails {
    pub target_id: String,
    #[serde(default)]
    pub target_mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default, deserialize_with = "de_size")]
    pub size: Option<i64>,
    #[serde(default)]
    pub md5_checksum: Option<String>,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub shortcut_details: Option<ShortcutDetails>,
    #[serde(default)]
    pub thumbnail_link: Option<String>,
    #[serde(default)]
    pub drive_id: Option<String>,
}

impl DriveFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == FOLDER_MIME
    }
    pub fn is_shortcut(&self) -> bool {
        self.mime_type == SHORTCUT_MIME
    }
    /// A folder, or a shortcut to one.
    pub fn is_folderish(&self) -> bool {
        self.is_folder()
            || self
                .shortcut_details
                .as_ref()
                .and_then(|s| s.target_mime_type.as_deref())
                .is_some_and(|m| m == FOLDER_MIME)
    }
    /// For a shortcut, the target's mime type; otherwise our own.
    pub fn effective_mime(&self) -> &str {
        self.shortcut_details
            .as_ref()
            .and_then(|s| s.target_mime_type.as_deref())
            .unwrap_or(&self.mime_type)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileList {
    #[serde(default)]
    files: Vec<DriveFile>,
    next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub file_id: String,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub file: Option<DriveFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeList {
    #[serde(default)]
    changes: Vec<Change>,
    next_page_token: Option<String>,
    new_start_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedDrive {
    pub id: String,
    pub name: String,
}

/// Resumable breadth-first crawl state. Persist it between batches so an
/// interrupted crawl of a large library picks up where it stopped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrawlState {
    pub queue: VecDeque<String>,
    pub visited: HashSet<String>,
    pub folders_done: u64,
    pub files_seen: u64,
}

impl CrawlState {
    pub fn new(root: &str) -> Self {
        Self { queue: VecDeque::from([root.to_string()]), ..Default::default() }
    }
    pub fn is_done(&self) -> bool {
        self.queue.is_empty()
    }
}

#[derive(Clone)]
pub struct DriveClient {
    http: reqwest::Client,
    auth: Auth,
    store: Arc<dyn TokenStore>,
    api_base: String,
    upload_base: String,
    shared_drive_id: Option<String>,
    backoff: Duration,
}

fn q_escape(id: &str) -> String {
    id.replace('\\', "\\\\").replace('\'', "\\'")
}

impl DriveClient {
    pub fn new(http: reqwest::Client, auth: Auth, store: Arc<dyn TokenStore>) -> Self {
        Self {
            http,
            auth,
            store,
            api_base: API_BASE.into(),
            upload_base: UPLOAD_BASE.into(),
            shared_drive_id: None,
            backoff: Duration::from_millis(500),
        }
    }

    /// Point at a mock server (tests).
    pub fn with_base_urls(mut self, api: &str, upload: &str) -> Self {
        self.api_base = api.trim_end_matches('/').into();
        self.upload_base = upload.trim_end_matches('/').into();
        self
    }

    pub fn with_backoff(mut self, base: Duration) -> Self {
        self.backoff = base;
        self
    }

    /// Scope listing and change queries to one shared drive.
    pub fn with_shared_drive(mut self, drive_id: Option<String>) -> Self {
        self.shared_drive_id = drive_id;
        self
    }

    pub fn auth(&self) -> &Auth {
        &self.auth
    }

    fn corpora(&self) -> Vec<(&'static str, String)> {
        let mut p = vec![
            ("supportsAllDrives", "true".to_string()),
            ("includeItemsFromAllDrives", "true".to_string()),
        ];
        match &self.shared_drive_id {
            Some(d) => {
                p.push(("corpora", "drive".into()));
                p.push(("driveId", d.clone()));
            }
            None => p.push(("corpora", "user".into())),
        }
        p
    }

    /// Send an authorized request with retries: refresh once on 401, and
    /// back off exponentially on 429, 5xx and 403 rate-limit errors.
    async fn send(&self, make: impl Fn(&reqwest::Client) -> RequestBuilder) -> Result<Response> {
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let token = self.auth.access_token(self.store.as_ref()).await?;
            let resp = match make(&self.http).bearer_auth(&token).send().await {
                Ok(r) => r,
                Err(e) if attempt < MAX_ATTEMPTS && (e.is_timeout() || e.is_connect()) => {
                    self.sleep(attempt).await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let status = resp.status();
            if status.is_success() || status == StatusCode::PARTIAL_CONTENT {
                return Ok(resp);
            }
            if status == StatusCode::UNAUTHORIZED && !refreshed {
                refreshed = true;
                self.auth.invalidate().await;
                continue;
            }
            let body = resp.text().await.unwrap_or_default();
            let rate_limited = status == StatusCode::FORBIDDEN
                && (body.contains("rateLimitExceeded") || body.contains("userRateLimitExceeded"));
            let retryable =
                status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() || rate_limited;
            if retryable && attempt < MAX_ATTEMPTS {
                tracing::warn!(%status, attempt, "Drive request throttled; backing off");
                self.sleep(attempt).await;
                continue;
            }
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| body.chars().take(300).collect());
            return Err(DriveError::Api { status: status.as_u16(), message });
        }
    }

    async fn sleep(&self, attempt: u32) {
        let base = self.backoff.as_millis() as u64;
        let jitter = rand::random::<u64>() % (base / 2 + 1);
        let ms = base.saturating_mul(1 << (attempt - 1).min(6)) + jitter;
        tokio::time::sleep(Duration::from_millis(ms.min(32_000))).await;
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, query: &[(&str, String)]) -> Result<T> {
        let resp = self.send(|h| h.get(url).query(query)).await?;
        let bytes = resp.bytes().await?;
        serde_json::from_slice(&bytes).map_err(|e| DriveError::Invalid(e.to_string()))
    }

    /// The signed-in account's email address.
    pub async fn account_email(&self) -> Result<Option<String>> {
        let v: serde_json::Value = self
            .get_json(&format!("{}/about", self.api_base), &[("fields", "user(emailAddress)".into())])
            .await?;
        Ok(v["user"]["emailAddress"].as_str().map(str::to_string))
    }

    pub async fn shared_drives(&self) -> Result<Vec<SharedDrive>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct DriveList {
            #[serde(default)]
            drives: Vec<SharedDrive>,
            next_page_token: Option<String>,
        }
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut q = vec![("pageSize", "100".to_string())];
            if let Some(t) = &token {
                q.push(("pageToken", t.clone()));
            }
            let page: DriveList = self.get_json(&format!("{}/drives", self.api_base), &q).await?;
            out.extend(page.drives);
            match page.next_page_token {
                Some(t) => token = Some(t),
                None => return Ok(out),
            }
        }
    }

    pub async fn get_file(&self, id: &str) -> Result<DriveFile> {
        self.get_json(
            &format!("{}/files/{}", self.api_base, id),
            &[("fields", FILE_FIELDS.into()), ("supportsAllDrives", "true".into())],
        )
        .await
    }

    /// All non-trashed children of a folder, following pagination. With
    /// `folders_only`, only folders and shortcuts to folders.
    pub async fn list_children(&self, folder_id: &str, folders_only: bool) -> Result<Vec<DriveFile>> {
        let mut q = format!("'{}' in parents and trashed = false", q_escape(folder_id));
        if folders_only {
            q.push_str(&format!(
                " and (mimeType = '{FOLDER_MIME}' or mimeType = '{SHORTCUT_MIME}')"
            ));
        }
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut params = self.corpora();
            // Folder ids in "My Drive" listings must not use corpora=drive.
            if folder_id == "root" || self.shared_drive_id.is_none() {
                params.retain(|(k, _)| *k != "corpora" && *k != "driveId");
                params.push(("corpora", "allDrives".into()));
            }
            params.extend([
                ("q", q.clone()),
                ("fields", format!("nextPageToken,files({FILE_FIELDS})")),
                ("pageSize", "1000".into()),
                ("orderBy", "folder,name_natural".into()),
            ]);
            if let Some(t) = &token {
                params.push(("pageToken", t.clone()));
            }
            let page: FileList = self.get_json(&format!("{}/files", self.api_base), &params).await?;
            out.extend(page.files);
            match page.next_page_token {
                Some(t) => token = Some(t),
                None => break,
            }
        }
        if folders_only {
            out.retain(DriveFile::is_folderish);
        }
        Ok(out)
    }

    /// Crawl one folder from `state.queue` and return its children (with
    /// file shortcuts resolved to their targets' size/md5). Call repeatedly
    /// until [`CrawlState::is_done`]; persist `state` after each call.
    pub async fn crawl_step(&self, state: &mut CrawlState) -> Result<Option<(String, Vec<DriveFile>)>> {
        let Some(folder) = state.queue.pop_front() else { return Ok(None) };
        if !state.visited.insert(folder.clone()) {
            return Ok(Some((folder, Vec::new())));
        }
        let mut children = self.list_children(&folder, false).await?;
        for child in children.iter_mut() {
            if child.is_folder() {
                if !state.visited.contains(&child.id) {
                    state.queue.push_back(child.id.clone());
                }
            } else if let Some(sc) = child.shortcut_details.clone() {
                if child.is_folderish() {
                    if !state.visited.contains(&sc.target_id) {
                        state.queue.push_back(sc.target_id.clone());
                    }
                } else if is_book_like(child.effective_mime(), &child.name) {
                    // Pull size/md5 from the target so downloads can be verified.
                    match self.get_file(&sc.target_id).await {
                        Ok(t) => {
                            child.size = t.size;
                            child.md5_checksum = t.md5_checksum;
                            child.thumbnail_link = t.thumbnail_link;
                        }
                        Err(e) => tracing::warn!(error = %e, "could not resolve shortcut target"),
                    }
                }
            }
        }
        state.folders_done += 1;
        state.files_seen += children.len() as u64;
        Ok(Some((folder, children)))
    }

    pub async fn start_page_token(&self) -> Result<String> {
        let mut q = vec![("supportsAllDrives", "true".to_string())];
        if let Some(d) = &self.shared_drive_id {
            q.push(("driveId", d.clone()));
        }
        let v: serde_json::Value =
            self.get_json(&format!("{}/changes/startPageToken", self.api_base), &q).await?;
        v["startPageToken"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| DriveError::Invalid("no startPageToken".into()))
    }

    /// All changes since `page_token`. Returns the changes and the token to
    /// store for next time.
    pub async fn changes_since(&self, page_token: &str) -> Result<(Vec<Change>, String)> {
        let mut token = page_token.to_string();
        let mut out = Vec::new();
        loop {
            let mut params = vec![
                ("pageToken", token.clone()),
                ("pageSize", "1000".to_string()),
                ("includeRemoved", "true".into()),
                ("supportsAllDrives", "true".into()),
                ("includeItemsFromAllDrives", "true".into()),
                ("spaces", "drive".into()),
                (
                    "fields",
                    format!("nextPageToken,newStartPageToken,changes(fileId,removed,file({FILE_FIELDS}))"),
                ),
            ];
            if let Some(d) = &self.shared_drive_id {
                params.push(("driveId", d.clone()));
            }
            let page: ChangeList = self.get_json(&format!("{}/changes", self.api_base), &params).await?;
            out.extend(page.changes);
            if let Some(next) = page.next_page_token {
                token = next;
                continue;
            }
            let new = page
                .new_start_page_token
                .ok_or_else(|| DriveError::Invalid("changes page without a token".into()))?;
            return Ok((out, new));
        }
    }

    /// Stream a file to `dest` (via a temp file), verifying `expected_md5`.
    /// `progress(bytes_so_far, total)` is called as data arrives.
    pub async fn download_to(
        &self,
        file_id: &str,
        dest: &Path,
        expected_md5: Option<&str>,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<u64> {
        let url = format!("{}/files/{}", self.api_base, file_id);
        let mut resp = self
            .send(|h| h.request(Method::GET, &url).query(&[("alt", "media"), ("supportsAllDrives", "true")]))
            .await?;
        let total = resp.content_length();
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = dest.with_extension("part");
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut hasher = Md5::new();
        let mut n: u64 = 0;
        let result: Result<()> = async {
            while let Some(chunk) = resp.chunk().await? {
                hasher.update(&chunk);
                file.write_all(&chunk).await?;
                n += chunk.len() as u64;
                progress(n, total);
            }
            file.flush().await?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(e) = result {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(e);
        }
        let actual = hex::encode(hasher.finalize());
        if let Some(expected) = expected_md5 {
            if !expected.eq_ignore_ascii_case(&actual) {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(DriveError::Checksum { expected: expected.into(), actual });
            }
        }
        tokio::fs::rename(&tmp, dest).await?;
        Ok(n)
    }

    /// Fetch a byte range (inclusive start, exclusive end).
    pub async fn download_range(&self, file_id: &str, start: u64, end: u64) -> Result<Vec<u8>> {
        let url = format!("{}/files/{}", self.api_base, file_id);
        let range = format!("bytes={}-{}", start, end.saturating_sub(1));
        let resp = self
            .send(|h| {
                h.get(&url)
                    .query(&[("alt", "media"), ("supportsAllDrives", "true")])
                    .header(reqwest::header::RANGE, range.clone())
            })
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Fetch a Drive thumbnail (the links require authorization).
    pub async fn fetch_thumbnail(&self, link: &str) -> Result<Vec<u8>> {
        // Ask for a larger rendition than the default 220px.
        let link = match link.rfind("=s") {
            Some(i) if link[i + 2..].chars().all(|c| c.is_ascii_digit()) => format!("{}=s600", &link[..i]),
            _ => link.to_string(),
        };
        let resp = self.send(|h| h.get(&link)).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Id of a file in `appDataFolder` by name.
    pub async fn appdata_find(&self, name: &str) -> Result<Option<String>> {
        let q = format!("name = '{}'", q_escape(name));
        let page: FileList = self
            .get_json(
                &format!("{}/files", self.api_base),
                &[
                    ("spaces", "appDataFolder".into()),
                    ("q", q),
                    ("fields", "files(id,name,modifiedTime)".into()),
                    ("pageSize", "10".into()),
                ],
            )
            .await?;
        Ok(page.files.into_iter().next().map(|f| f.id))
    }

    pub async fn appdata_read(&self, id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/files/{}", self.api_base, id);
        let resp = self.send(|h| h.get(&url).query(&[("alt", "media")])).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Create or overwrite a JSON file in `appDataFolder`. This is the only
    /// write BiblioChad ever makes to Drive.
    pub async fn appdata_write(&self, name: &str, existing_id: Option<&str>, body: Vec<u8>) -> Result<String> {
        #[derive(Deserialize)]
        struct Created {
            id: String,
        }
        let resp = match existing_id {
            Some(id) => {
                let url = format!("{}/files/{}", self.upload_base, id);
                self.send(|h| {
                    h.patch(&url)
                        .query(&[("uploadType", "media"), ("fields", "id")])
                        .header(reqwest::header::CONTENT_TYPE, "application/json")
                        .body(body.clone())
                })
                .await?
            }
            None => {
                let boundary = format!("bibliochad-{}", rand::random::<u64>());
                let meta = serde_json::json!({ "name": name, "parents": ["appDataFolder"], "mimeType": "application/json" });
                let mut multipart = Vec::with_capacity(body.len() + 512);
                multipart.extend_from_slice(
                    format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n").as_bytes(),
                );
                multipart.extend_from_slice(&body);
                multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
                let url = format!("{}/files", self.upload_base);
                self.send(|h| {
                    h.post(&url)
                        .query(&[("uploadType", "multipart"), ("fields", "id")])
                        .header(
                            reqwest::header::CONTENT_TYPE,
                            format!("multipart/related; boundary={boundary}"),
                        )
                        .body(multipart.clone())
                })
                .await?
            }
        };
        let c: Created = serde_json::from_slice(&resp.bytes().await?).map_err(|e| DriveError::Invalid(e.to_string()))?;
        Ok(c.id)
    }
}

fn is_book_like(mime: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    mime == "application/epub+zip" || mime == "application/pdf" || n.ends_with(".epub") || n.ends_with(".pdf")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_store::MemoryStore;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    async fn client(server: &MockServer) -> DriveClient {
        DriveClient::new(reqwest::Client::new(), Auth::fixed("tok"), Arc::new(MemoryStore::default()))
            .with_base_urls(&format!("{}/drive/v3", server.uri()), &format!("{}/upload/drive/v3", server.uri()))
            .with_backoff(Duration::from_millis(1))
    }

    fn file(id: &str, name: &str, mime: &str) -> serde_json::Value {
        serde_json::json!({ "id": id, "name": name, "mimeType": mime, "size": "1234", "md5Checksum": format!("md5{id}") })
    }

    /// Serves folder listings based on the `q` parameter.
    struct Tree;
    impl Respond for Tree {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let q: std::collections::HashMap<_, _> = req.url.query_pairs().into_owned().collect();
            let qs = q.get("q").cloned().unwrap_or_default();
            let page = q.get("pageToken").cloned();
            let body = if qs.starts_with("'root' in parents") {
                match page.as_deref() {
                    None => serde_json::json!({
                        "files": [file("A", "Sci-Fi", FOLDER_MIME), file("b1", "Dune.epub", "application/epub+zip")],
                        "nextPageToken": "p2"
                    }),
                    _ => serde_json::json!({ "files": [
                        {"id": "sc", "name": "Elsewhere", "mimeType": SHORTCUT_MIME,
                         "shortcutDetails": {"targetId": "X", "targetMimeType": FOLDER_MIME}},
                        {"id": "scb", "name": "Linked.pdf", "mimeType": SHORTCUT_MIME,
                         "shortcutDetails": {"targetId": "T", "targetMimeType": "application/pdf"}}
                    ]}),
                }
            } else if qs.starts_with("'A' in parents") {
                serde_json::json!({ "files": [
                    file("b2", "Foundation.pdf", "application/pdf"),
                    // A shortcut back up the tree must not loop forever.
                    {"id": "loop", "name": "Up", "mimeType": SHORTCUT_MIME,
                     "shortcutDetails": {"targetId": "root", "targetMimeType": FOLDER_MIME}}
                ]})
            } else if qs.starts_with("'X' in parents") {
                serde_json::json!({ "files": [file("b3", "External.epub", "application/epub+zip")] })
            } else {
                serde_json::json!({ "files": [] })
            };
            ResponseTemplate::new(200).set_body_json(body)
        }
    }

    #[tokio::test]
    async fn crawl_handles_pagination_shortcuts_and_cycles() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/drive/v3/files")).respond_with(Tree).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files/T"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file("T", "Linked.pdf", "application/pdf")))
            .mount(&server)
            .await;
        let c = client(&server).await;
        let mut state = CrawlState::new("root");
        let mut seen = Vec::new();
        let mut steps = 0;
        while let Some((_folder, files)) = c.crawl_step(&mut state).await.unwrap() {
            steps += 1;
            assert!(steps < 20, "crawl must terminate");
            // Round-trip the state as an app would between batches.
            state = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
            seen.extend(files);
        }
        let ids: HashSet<_> = seen.iter().map(|f| f.id.as_str()).collect();
        for id in ["A", "b1", "sc", "scb", "b2", "loop", "b3"] {
            assert!(ids.contains(id), "missing {id}");
        }
        let linked = seen.iter().find(|f| f.id == "scb").unwrap();
        assert_eq!(linked.md5_checksum.as_deref(), Some("md5T"), "shortcut target resolved");
        assert_eq!(seen.iter().find(|f| f.id == "b1").unwrap().size, Some(1234));
        assert_eq!(state.folders_done, 3);
    }

    struct FlakyThenOk(std::sync::atomic::AtomicU32, u16);
    impl Respond for FlakyThenOk {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n < 2 {
                ResponseTemplate::new(self.1).set_body_json(serde_json::json!({
                    "error": {"code": self.1, "message": "slow down", "errors": [{"reason": "rateLimitExceeded"}]}
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"startPageToken": "42"}))
            }
        }
    }

    #[tokio::test]
    async fn retries_rate_limits() {
        for status in [429u16, 403, 503] {
            let server = MockServer::start().await;
            Mock::given(path("/drive/v3/changes/startPageToken"))
                .respond_with(FlakyThenOk(Default::default(), status))
                .expect(3)
                .mount(&server)
                .await;
            assert_eq!(client(&server).await.start_page_token().await.unwrap(), "42");
        }
    }

    #[tokio::test]
    async fn surfaces_hard_errors() {
        let server = MockServer::start().await;
        Mock::given(path("/drive/v3/files/nope"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": 404, "message": "File not found: nope."}
            })))
            .expect(1)
            .mount(&server)
            .await;
        match client(&server).await.get_file("nope").await {
            Err(DriveError::Api { status: 404, message }) => assert!(message.contains("File not found")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn changes_follow_pages() {
        let server = MockServer::start().await;
        Mock::given(path("/drive/v3/changes"))
            .and(query_param("pageToken", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "changes": [{"fileId": "b1", "removed": true}], "nextPageToken": "2"
            })))
            .mount(&server)
            .await;
        Mock::given(path("/drive/v3/changes"))
            .and(query_param("pageToken", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "changes": [{"fileId": "b9", "file": {"id": "b9", "name": "New.pdf", "mimeType": "application/pdf", "parents": ["A"]}}],
                "newStartPageToken": "3"
            })))
            .mount(&server)
            .await;
        let (changes, token) = client(&server).await.changes_since("1").await.unwrap();
        assert_eq!(token, "3");
        assert_eq!(changes.len(), 2);
        assert!(changes[0].removed);
        assert_eq!(changes[1].file.as_ref().unwrap().parents, vec!["A"]);
    }

    #[tokio::test]
    async fn download_verifies_md5() {
        let server = MockServer::start().await;
        let body = b"%PDF-1.4 hello".to_vec();
        Mock::given(path("/drive/v3/files/f1"))
            .and(query_param("alt", "media"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        let c = client(&server).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("cache/f1.pdf");
        let good = hex::encode(Md5::digest(&body));
        let mut last = 0;
        let n = c.download_to("f1", &dest, Some(&good), |done, _| last = done).await.unwrap();
        assert_eq!(n as usize, body.len());
        assert_eq!(last as usize, body.len());
        assert_eq!(std::fs::read(&dest).unwrap(), body);

        let bad = dir.path().join("cache/bad.pdf");
        let r = c.download_to("f1", &bad, Some("00000000000000000000000000000000"), |_, _| {}).await;
        assert!(matches!(r, Err(DriveError::Checksum { .. })));
        assert!(!bad.exists());
        assert!(!bad.with_extension("part").exists());
    }

    #[tokio::test]
    async fn appdata_create_then_update() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/drive/v3/files"))
            .and(query_param("spaces", "appDataFolder"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"files": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/upload/drive/v3/files"))
            .and(query_param("uploadType", "multipart"))
            .respond_with(|req: &Request| {
                let body = String::from_utf8_lossy(&req.body);
                assert!(body.contains("\"appDataFolder\""));
                assert!(body.contains("{\"schema\":1}"));
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "new-id"}))
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/upload/drive/v3/files/new-id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "new-id"})))
            .expect(1)
            .mount(&server)
            .await;
        let c = client(&server).await;
        assert_eq!(c.appdata_find("sync.json").await.unwrap(), None);
        let id = c.appdata_write("sync.json", None, br#"{"schema":1}"#.to_vec()).await.unwrap();
        assert_eq!(id, "new-id");
        c.appdata_write("sync.json", Some(&id), b"{}".to_vec()).await.unwrap();
    }

    #[test]
    fn escapes_query_ids() {
        assert_eq!(q_escape("a'b"), "a\\'b");
    }
}
