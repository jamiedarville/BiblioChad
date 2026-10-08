//! Keeping the local mirror in step with Drive, and syncing reading state
//! through the hidden `appDataFolder` file.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use bc_drive::{CrawlState, DriveClient, DriveFile};
use bc_library::NodeRecord;
use bc_sync::{SyncDoc, SYNC_FILE_NAME};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

const PENDING_TOKEN_KEY: &str = "pending_changes_token";
const SYNC_FILE_ID_KEY: &str = "sync_file_id";
pub const LAST_PROGRESS_SYNC_KEY: &str = "last_progress_sync";

#[derive(Clone, Serialize)]
pub struct SyncStatus {
    pub phase: &'static str,
    pub message: String,
    pub folders_done: u64,
    pub files_seen: u64,
}

fn emit_status(
    app: &AppHandle,
    phase: &'static str,
    message: impl Into<String>,
    crawl: Option<&CrawlState>,
) {
    let _ = app.emit(
        "sync-status",
        SyncStatus {
            phase,
            message: message.into(),
            folders_done: crawl.map(|c| c.folders_done).unwrap_or(0),
            files_seen: crawl.map(|c| c.files_seen).unwrap_or(0),
        },
    );
}

pub fn to_node(f: &DriveFile, parent: Option<&str>) -> NodeRecord {
    let parent = parent
        .map(str::to_string)
        .or_else(|| f.parents.first().cloned());
    NodeRecord {
        drive_id: f.id.clone(),
        parent_drive_id: parent,
        name: f.name.clone(),
        mime_type: Some(f.effective_mime().to_string()),
        size: f.size,
        md5: f.md5_checksum.clone(),
        modified_time: f.modified_time.clone(),
        is_folder: f.is_folderish(),
        is_trashed: f.trashed,
        shortcut_target_id: f.shortcut_details.as_ref().map(|s| s.target_id.clone()),
        thumbnail_link: f.thumbnail_link.clone(),
    }
}

/// Run a full sync: library mirror, covers, reading state, pinned books.
/// Only one sync runs at a time; a second call waits for the first.
pub async fn sync_all(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let _guard = state.sync_lock.lock().await;
    let Some(drive) = state.drive().await else {
        return Ok(());
    };
    state.syncing.store(true, Ordering::SeqCst);
    let result = async {
        sync_library(&app, &drive).await?;
        let _ = app.emit("library-changed", ());
        sync_progress(&state, &drive).await?;
        let _ = app.emit("library-changed", ());
        fetch_thumbnails(&app, &drive).await;
        download_pinned(&app).await;
        Ok::<_, CmdError>(())
    }
    .await;
    state.syncing.store(false, Ordering::SeqCst);
    match &result {
        Ok(()) => emit_status(&app, "idle", "Library is up to date.", None),
        Err(e) => {
            tracing::warn!("sync failed: {e}");
            emit_status(&app, "error", e.message.clone(), None);
        }
    }
    let _ = app.emit("library-changed", ());
    result
}

async fn sync_library(app: &AppHandle, drive: &DriveClient) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let Some(source) = state.lib.drive_source()? else {
        return Ok(());
    };
    let root = source.drive_root_id.clone().unwrap_or_default();
    let crawl: Option<CrawlState> = source
        .crawl_state
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    match (crawl, source.changes_page_token.as_deref()) {
        // Incremental: nothing in flight and we have a token.
        (None, Some(token)) => apply_changes(app, drive, source.id, token).await,
        // Fresh or interrupted crawl.
        (crawl, _) => {
            let pending = state.lib.setting(PENDING_TOKEN_KEY)?;
            let token = match pending {
                Some(t) if crawl.is_some() => t,
                _ => {
                    // Take the token *before* crawling so nothing is missed.
                    let t = drive.start_page_token().await?;
                    state.lib.set_setting(PENDING_TOKEN_KEY, &t)?;
                    t
                }
            };
            let crawl = crawl.unwrap_or_else(|| CrawlState::new(&root));
            run_crawl(app, drive, source.id, crawl).await?;
            state
                .lib
                .set_source_sync_state(source.id, Some(&token), None)?;
            state.lib.delete_setting(PENDING_TOKEN_KEY)?;
            // Catch up on anything that changed while we crawled.
            let fresh = state
                .lib
                .drive_source()?
                .and_then(|s| s.changes_page_token)
                .unwrap_or(token);
            apply_changes(app, drive, source.id, &fresh).await
        }
    }
}

async fn run_crawl(
    app: &AppHandle,
    drive: &DriveClient,
    source_id: i64,
    mut crawl: CrawlState,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut last_emit = Instant::now();
    emit_status(app, "crawl", "Scanning your Drive folder…", Some(&crawl));
    while let Some((folder, files)) = drive.crawl_step(&mut crawl).await? {
        let nodes: Vec<NodeRecord> = files.iter().map(|f| to_node(f, Some(&folder))).collect();
        state.lib.upsert_nodes(source_id, &nodes)?;
        let snapshot =
            serde_json::to_string(&crawl).map_err(|e| CmdError::internal(e.to_string()))?;
        state
            .lib
            .set_source_sync_state(source_id, None, Some(&snapshot))?;
        if last_emit.elapsed() > Duration::from_millis(700) {
            last_emit = Instant::now();
            emit_status(
                app,
                "crawl",
                format!(
                    "Scanned {} folders, {} items",
                    crawl.folders_done, crawl.files_seen
                ),
                Some(&crawl),
            );
            let _ = app.emit("library-changed", ());
        }
    }
    Ok(())
}

async fn apply_changes(
    app: &AppHandle,
    drive: &DriveClient,
    source_id: i64,
    token: &str,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    emit_status(app, "changes", "Checking Drive for changes…", None);
    let (changes, new_token) = drive.changes_since(token).await?;
    if !changes.is_empty() {
        tracing::info!(count = changes.len(), "applying Drive changes");
    }
    let mut new_folders: Vec<String> = Vec::new();
    // Folders first (repeat until no progress) so children find their parents.
    let mut pending: Vec<&DriveFile> = Vec::new();
    for ch in &changes {
        match &ch.file {
            Some(f) if !ch.removed && !f.trashed => pending.push(f),
            _ => {
                if state.lib.node_parent(&ch.file_id)?.is_some() {
                    state.lib.remove_subtree(&ch.file_id)?;
                }
            }
        }
    }
    let mut progressed = true;
    while progressed {
        progressed = false;
        let mut rest = Vec::new();
        for f in pending {
            let parent = f.parents.first().map(String::as_str).unwrap_or("");
            let known = state.lib.node_parent(&f.id)?.is_some();
            if state.lib.is_folder_in_library(parent)? {
                let mut f = f.clone();
                if f.is_shortcut() && !f.is_folderish() {
                    if let Some(sc) = &f.shortcut_details {
                        if let Ok(t) = drive.get_file(&sc.target_id).await {
                            f.size = t.size;
                            f.md5_checksum = t.md5_checksum;
                            f.thumbnail_link = t.thumbnail_link;
                        }
                    }
                }
                state
                    .lib
                    .upsert_nodes(source_id, &[to_node(&f, Some(parent))])?;
                if f.is_folderish() && !known {
                    let target = f
                        .shortcut_details
                        .as_ref()
                        .map(|s| s.target_id.clone())
                        .unwrap_or(f.id.clone());
                    new_folders.push(target);
                }
                progressed = true;
            } else {
                // Its parent may be a folder that appears later in this batch.
                rest.push(f);
            }
        }
        pending = rest;
    }
    // Whatever is left lives outside the library now.
    for f in pending {
        if state.lib.node_parent(&f.id)?.is_some() {
            state.lib.remove_subtree(&f.id)?;
        }
    }
    // Folders moved into the library arrive without their children.
    let mut seen = HashSet::new();
    for folder in new_folders {
        if seen.insert(folder.clone()) {
            let crawl = CrawlState::new(&folder);
            run_crawl(app, drive, source_id, crawl).await?;
        }
    }
    state
        .lib
        .set_source_sync_state(source_id, Some(&new_token), None)?;
    Ok(())
}

/// Merge reading positions, bookmarks and annotations with the copy in
/// `appDataFolder`, then write back the merged result if it changed.
pub async fn sync_progress(state: &AppState, drive: &DriveClient) -> CmdResult<()> {
    let mut file_id = state.lib.setting(SYNC_FILE_ID_KEY)?;
    if file_id.is_none() {
        file_id = drive.appdata_find(SYNC_FILE_NAME).await?;
    }
    let remote = match &file_id {
        Some(id) => match drive.appdata_read(id).await {
            Ok(bytes) => {
                SyncDoc::from_json(&bytes).map_err(|e| CmdError::new("sync", e.to_string()))?
            }
            Err(bc_drive::DriveError::Api { status: 404, .. }) => {
                file_id = None;
                SyncDoc::default()
            }
            Err(e) => return Err(e.into()),
        },
        None => SyncDoc::default(),
    };
    let local = state.lib.export_sync_doc()?;
    let merged = bc_sync::merge(&remote, &local);
    state.lib.import_sync_doc(&merged)?;
    if merged != remote || file_id.is_none() {
        let id = drive
            .appdata_write(SYNC_FILE_NAME, file_id.as_deref(), merged.to_json())
            .await?;
        state.lib.set_setting(SYNC_FILE_ID_KEY, &id)?;
    }
    state
        .lib
        .set_setting(LAST_PROGRESS_SYNC_KEY, &bc_core::now_ms().to_string())?;
    Ok(())
}

/// Use Drive's own thumbnails as covers until a book has been opened.
async fn fetch_thumbnails(app: &AppHandle, drive: &DriveClient) {
    let state = app.state::<AppState>();
    let Ok(todo) = state.lib.books_needing_thumbnails(200) else {
        return;
    };
    let mut changed = 0;
    for (book_id, link) in todo {
        let dest = state.cover_file(book_id);
        match drive.fetch_thumbnail(&link).await {
            Ok(bytes) if bc_library::save_cover_thumbnail(&bytes, &dest).is_ok() => {
                let _ = state.lib.set_cover_path(book_id, &dest.to_string_lossy());
                changed += 1;
            }
            Ok(_) => {
                let _ = state.lib.clear_thumbnail_link(book_id);
            }
            // Thumbnail links expire; the next crawl/changes pass refreshes them.
            Err(_) => {
                let _ = state.lib.clear_thumbnail_link(book_id);
            }
        }
        if changed > 0 && changed % 20 == 0 {
            let _ = app.emit("library-changed", ());
        }
    }
}

async fn download_pinned(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(ids) = state.lib.pinned_uncached() else {
        return;
    };
    for id in ids {
        if let Err(e) = crate::books::ensure_local(app, id).await {
            tracing::warn!(book_id = id, "pinned download failed: {}", e.message);
        }
    }
}
