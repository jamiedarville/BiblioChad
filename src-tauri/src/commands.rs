//! Tauri commands. The UI is thin: everything it shows comes from here.

use std::path::PathBuf;
use std::time::Duration;

use bc_core::{BookFormat, Locator, ReadStatus};
use bc_drive::{Auth, PendingAuth};
use bc_library::{
    Annotation, BookDetails, BookQuery, BookSummary, Bookmark, CacheStats, Collection,
    FolderListing, NewAnnotation, Stats,
};
use bc_reader::pdf::{OutlineItem, SearchHit, TextRun};
use bc_sync::ResumePrompt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::books;
use crate::drive_sync;
use crate::error::{CmdError, CmdResult};
use crate::state::{AppState, PREFS_KEY};

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CmdError::internal(e.to_string()))?
}

// ---------------------------------------------------------------- library

#[derive(Serialize)]
pub struct Overview {
    pub drive: DriveStatus,
    pub stats: Stats,
    pub continue_reading: Vec<BookSummary>,
    pub has_books: bool,
    pub pdf_available: bool,
}

#[tauri::command]
pub async fn get_overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    let drive = drive_status_inner(&state).await?;
    let stats = state.lib.stats(&today())?;
    Ok(Overview {
        has_books: stats.total_books > 0,
        stats,
        continue_reading: state.lib.continue_reading(12)?,
        drive,
        pdf_available: state.pdf.is_ok(),
    })
}

#[tauri::command]
pub fn list_folder(
    state: State<'_, AppState>,
    folder_id: Option<String>,
) -> CmdResult<FolderListing> {
    Ok(state.lib.list_folder(folder_id.as_deref())?)
}

#[tauri::command]
pub fn query_books(state: State<'_, AppState>, query: BookQuery) -> CmdResult<Vec<BookSummary>> {
    Ok(state.lib.query_books(&query)?)
}

#[tauri::command]
pub fn recently_read(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CmdResult<Vec<BookSummary>> {
    Ok(state.lib.recently_read(limit.unwrap_or(200))?)
}

#[tauri::command]
pub fn book_details(state: State<'_, AppState>, id: i64) -> CmdResult<BookDetails> {
    Ok(state.lib.book_details(id)?)
}

#[tauri::command]
pub fn set_book_overrides(
    state: State<'_, AppState>,
    id: i64,
    title: Option<String>,
    author: Option<String>,
    series: Option<String>,
) -> CmdResult<()> {
    Ok(state
        .lib
        .set_overrides(id, title.as_deref(), author.as_deref(), series.as_deref())?)
}

#[tauri::command]
pub fn set_favorite(state: State<'_, AppState>, id: i64, favorite: bool) -> CmdResult<()> {
    Ok(state.lib.set_favorite(id, favorite)?)
}

#[tauri::command]
pub fn set_status(
    state: State<'_, AppState>,
    id: i64,
    status: Option<ReadStatus>,
) -> CmdResult<()> {
    if status == Some(ReadStatus::Unread) {
        state.lib.reset_progress(id)?;
    }
    Ok(state.lib.set_status_override(id, status)?)
}

#[tauri::command]
pub async fn set_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> CmdResult<()> {
    state.lib.set_pinned(id, pinned)?;
    if pinned {
        // Download in the background so the book is ready offline.
        tauri::async_runtime::spawn(async move {
            match books::ensure_local(&app, id).await {
                Ok(_) => {
                    let _ = app.emit("library-changed", ());
                }
                Err(e) => tracing::warn!(book_id = id, "pin download failed: {}", e.message),
            }
        });
    } else {
        books::enforce_cache_cap(&state, None)?;
    }
    Ok(())
}

#[tauri::command]
pub fn collections(state: State<'_, AppState>) -> CmdResult<Vec<Collection>> {
    Ok(state.lib.collections()?)
}

#[tauri::command]
pub fn create_collection(state: State<'_, AppState>, name: String) -> CmdResult<i64> {
    Ok(state.lib.create_collection(&name)?)
}

#[tauri::command]
pub fn rename_collection(state: State<'_, AppState>, id: i64, name: String) -> CmdResult<()> {
    Ok(state.lib.rename_collection(id, &name)?)
}

#[tauri::command]
pub fn delete_collection(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(state.lib.delete_collection(id)?)
}

#[tauri::command]
pub fn set_in_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    book_id: i64,
    member: bool,
) -> CmdResult<()> {
    Ok(state
        .lib
        .set_in_collection(collection_id, book_id, member)?)
}

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: Vec<i64>,
    pub failed: Vec<String>,
}

#[tauri::command]
pub async fn import_local_files(app: AppHandle, paths: Vec<String>) -> CmdResult<ImportResult> {
    let app2 = app.clone();
    let result = blocking(move || {
        let state = app2.state::<AppState>();
        let mut imported = Vec::new();
        let mut failed = Vec::new();
        for p in paths {
            match books::import_local_file(&state, &PathBuf::from(&p)) {
                Ok(id) => imported.push(id),
                Err(e) => failed.push(format!("{p}: {}", e.message)),
            }
        }
        Ok(ImportResult { imported, failed })
    })
    .await?;
    let _ = app.emit("library-changed", ());
    Ok(result)
}

#[tauri::command]
pub fn remove_local_book(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(state.lib.remove_local(id)?)
}

#[tauri::command]
pub fn export_annotations(state: State<'_, AppState>, id: i64, dest: String) -> CmdResult<()> {
    let md = state.lib.export_annotations_markdown(id)?;
    std::fs::write(dest, md)?;
    Ok(())
}

// ----------------------------------------------------------------- reader

#[derive(Serialize)]
pub struct OpenBook {
    pub id: i64,
    pub format: BookFormat,
    pub title: String,
    pub author: Option<String>,
    pub locator: Option<Locator>,
    pub percent: f64,
    pub prompt: Option<ResumePrompt>,
    pub page_count: Option<u32>,
    pub page_sizes: Vec<(f32, f32)>,
    pub outline: Vec<OutlineItem>,
    pub bookmarks: Vec<Bookmark>,
    pub annotations: Vec<Annotation>,
}

/// Download if needed, extract metadata on first open, and return
/// everything the reader needs to start at the right place.
#[tauri::command]
pub async fn open_book(app: AppHandle, id: i64) -> CmdResult<OpenBook> {
    let path = books::ensure_local(&app, id).await?;
    let app2 = app.clone();
    blocking(move || {
        let state = app2.state::<AppState>();
        let src = state.lib.book_source(id)?;
        *state.open_book_key.lock().unwrap() = Some(src.key.clone());
        if !src.metadata_extracted {
            if let Err(e) = books::extract_metadata(&state, id, &path, src.format) {
                tracing::warn!(book_id = id, "metadata extraction failed: {}", e.message);
            }
        }
        let (page_count, page_sizes, outline) = match src.format {
            BookFormat::Pdf => {
                let pdf = state.pdf()?;
                let info = pdf.info(&path)?;
                if state.lib.book_summary(id)?.page_count.is_none() {
                    state.lib.set_page_count(id, info.page_count)?;
                }
                (
                    Some(info.page_count),
                    info.page_sizes,
                    pdf.outline(&path).unwrap_or_default(),
                )
            }
            BookFormat::Epub => (None, Vec::new(), Vec::new()),
        };
        let summary = state.lib.book_summary(id)?;
        let (pos, prompt) = state.lib.resume_point(id, &state.device_id)?;
        Ok(OpenBook {
            id,
            format: src.format,
            title: summary.title,
            author: summary.author,
            percent: pos.as_ref().map(|p| p.percent).unwrap_or(0.0),
            locator: pos.map(|p| p.locator),
            prompt,
            page_count,
            page_sizes,
            outline,
            bookmarks: state.lib.bookmarks(id)?,
            annotations: state.lib.annotations(id)?,
        })
    })
    .await
}

#[tauri::command]
pub fn save_progress(
    state: State<'_, AppState>,
    id: i64,
    locator: Locator,
    percent: f64,
) -> CmdResult<()> {
    state
        .lib
        .save_progress(id, &state.device_id, &locator, percent, Some(&today()))?;
    Ok(())
}

/// Called when the reader closes: forget the open book and push progress.
#[tauri::command]
pub async fn close_book(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    *state.open_book_key.lock().unwrap() = None;
    books::enforce_cache_cap(&state, None)?;
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Some(drive) = state.drive().await {
            let _guard = state.sync_lock.lock().await;
            if let Err(e) = drive_sync::sync_progress(&state, &drive).await {
                tracing::info!("progress sync deferred: {}", e.message);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn add_bookmark(
    state: State<'_, AppState>,
    id: i64,
    locator: Locator,
    label: Option<String>,
) -> CmdResult<Bookmark> {
    Ok(state.lib.add_bookmark(id, &locator, label.as_deref())?)
}

#[tauri::command]
pub fn delete_bookmark(state: State<'_, AppState>, bookmark_id: String) -> CmdResult<()> {
    Ok(state.lib.delete_bookmark(&bookmark_id)?)
}

#[tauri::command]
pub fn add_annotation(
    state: State<'_, AppState>,
    id: i64,
    annotation: NewAnnotation,
) -> CmdResult<Annotation> {
    Ok(state.lib.add_annotation(id, &annotation)?)
}

#[tauri::command]
pub fn update_annotation(
    state: State<'_, AppState>,
    annotation_id: String,
    color: Option<String>,
    note: Option<String>,
) -> CmdResult<()> {
    Ok(state
        .lib
        .update_annotation(&annotation_id, color.as_deref(), note.as_deref())?)
}

#[tauri::command]
pub fn delete_annotation(state: State<'_, AppState>, annotation_id: String) -> CmdResult<()> {
    Ok(state.lib.delete_annotation(&annotation_id)?)
}

fn opened_path(state: &AppState, id: i64) -> CmdResult<PathBuf> {
    let src = state.lib.book_source(id)?;
    if let Some(p) = src.local_path {
        return Ok(PathBuf::from(p));
    }
    state
        .lib
        .cache_lookup(&src.key, src.md5.as_deref())?
        .map(|e| PathBuf::from(e.path))
        .ok_or_else(|| CmdError::new("not_found", "Open the book first."))
}

#[tauri::command]
pub async fn pdf_text(app: AppHandle, id: i64, page: u32) -> CmdResult<Vec<TextRun>> {
    blocking(move || {
        let state = app.state::<AppState>();
        let path = opened_path(&state, id)?;
        Ok(state.pdf()?.text_runs(&path, page)?)
    })
    .await
}

#[tauri::command]
pub async fn pdf_search(app: AppHandle, id: i64, query: String) -> CmdResult<Vec<SearchHit>> {
    blocking(move || {
        let state = app.state::<AppState>();
        let path = opened_path(&state, id)?;
        Ok(state.pdf()?.search(&path, &query, 500)?)
    })
    .await
}

// ------------------------------------------------------------------ drive

#[derive(Serialize)]
pub struct DriveStatus {
    pub configured: bool,
    pub connected: bool,
    pub email: Option<String>,
    pub root_id: Option<String>,
    pub root_name: Option<String>,
    pub last_synced_at: Option<i64>,
    pub syncing: bool,
    pub crawl_in_progress: bool,
}

async fn drive_status_inner(state: &AppState) -> CmdResult<DriveStatus> {
    let source = state.lib.drive_source()?;
    Ok(DriveStatus {
        configured: state.oauth_client().is_some(),
        connected: state.drive().await.is_some(),
        email: state.lib.setting("drive_email")?,
        root_id: source.as_ref().and_then(|s| s.drive_root_id.clone()),
        root_name: source.as_ref().map(|s| s.name.clone()),
        last_synced_at: source.as_ref().and_then(|s| s.last_synced_at),
        syncing: state.syncing.load(std::sync::atomic::Ordering::SeqCst),
        crawl_in_progress: source.as_ref().is_some_and(|s| s.crawl_state.is_some()),
    })
}

#[tauri::command]
pub async fn drive_status(state: State<'_, AppState>) -> CmdResult<DriveStatus> {
    drive_status_inner(&state).await
}

/// Import the Desktop-app OAuth client JSON downloaded from Google Cloud.
#[tauri::command]
pub fn drive_import_client_file(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let meta = std::fs::metadata(&path)?;
    if meta.len() > 64 * 1024 {
        return Err(CmdError::invalid(
            "That file is too large to be an OAuth client JSON.",
        ));
    }
    let contents = std::fs::read(&path)?;
    bc_drive::OAuthClient::from_installed_json(&contents)?;
    std::fs::write(state.paths.google_client_file(), contents)?;
    Ok(())
}

#[tauri::command]
pub async fn drive_connect(app: AppHandle, state: State<'_, AppState>) -> CmdResult<DriveStatus> {
    let client = state.oauth_client().ok_or_else(|| {
        CmdError::new(
            "not_configured",
            "No Google OAuth client is configured. Import the Desktop-app client JSON from Google Cloud console first.",
        )
    })?;
    let pending = PendingAuth::begin(&client).await?;
    app.opener()
        .open_url(pending.auth_url.clone(), None::<&str>)
        .map_err(|e| CmdError::internal(format!("could not open the browser: {e}")))?;
    let auth: Auth = pending
        .finish(
            &client,
            &state.http,
            state.store.as_ref(),
            Duration::from_secs(300),
        )
        .await?;
    let drive = state.make_drive_client(auth);
    if let Ok(Some(email)) = drive.account_email().await {
        state.lib.set_setting("drive_email", &email)?;
    }
    *state.drive.write().await = Some(drive);
    drive_status_inner(&state).await
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FolderEntry {
    pub id: String,
    pub name: String,
    /// Set for shared-drive roots and items inside shared drives.
    pub shared_drive_id: Option<String>,
}

/// Folder browser. `parent = None` lists My Drive plus shared drives.
#[tauri::command]
pub async fn drive_list_folders(
    state: State<'_, AppState>,
    parent: Option<String>,
) -> CmdResult<Vec<FolderEntry>> {
    let drive = state
        .drive()
        .await
        .ok_or(bc_drive::DriveError::NotConnected)?;
    match parent {
        None => {
            let mut out = vec![FolderEntry {
                id: "root".into(),
                name: "My Drive".into(),
                shared_drive_id: None,
            }];
            for d in drive.shared_drives().await.unwrap_or_default() {
                out.push(FolderEntry {
                    id: d.id.clone(),
                    name: d.name,
                    shared_drive_id: Some(d.id),
                });
            }
            Ok(out)
        }
        Some(p) => Ok(drive
            .with_shared_drive(None)
            .list_children(&p, true)
            .await?
            .into_iter()
            .map(|f| FolderEntry {
                id: f
                    .shortcut_details
                    .as_ref()
                    .map(|s| s.target_id.clone())
                    .unwrap_or(f.id.clone()),
                name: f.name,
                shared_drive_id: f.drive_id,
            })
            .collect()),
    }
}

#[tauri::command]
pub async fn drive_choose_root(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: FolderEntry,
) -> CmdResult<()> {
    let email = state.lib.setting("drive_email")?;
    match &folder.shared_drive_id {
        Some(d) => state.lib.set_setting("drive_shared_drive_id", d)?,
        None => state.lib.delete_setting("drive_shared_drive_id")?,
    }
    state
        .lib
        .add_drive_source(email.as_deref(), &folder.id, &folder.name)?;
    // Rebuild the client so it picks up the shared-drive scope.
    let current = state.drive().await;
    if let Some(d) = current {
        *state.drive.write().await = Some(state.make_drive_client(d.auth().clone()));
    }
    let _ = app.emit("library-changed", ());
    tauri::async_runtime::spawn(drive_sync::sync_all(app));
    Ok(())
}

#[tauri::command]
pub async fn drive_sync_now(app: AppHandle) -> CmdResult<()> {
    drive_sync::sync_all(app).await
}

/// Sign out. With `wipe`, also delete the cache, covers and local database
/// contents (reading progress that was synced stays in Drive appData).
#[tauri::command]
pub async fn drive_disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
    wipe: bool,
) -> CmdResult<()> {
    let _guard = state.sync_lock.lock().await;
    if let Some(d) = state.drive.write().await.take() {
        d.auth().revoke(state.store.as_ref()).await?;
    } else {
        state.store.clear()?;
    }
    state.lib.delete_setting("drive_email")?;
    state.lib.delete_setting("sync_file_id")?;
    if wipe {
        state.epubs.lock().unwrap().clear();
        for path in state.lib.cache_clear_unpinned(None)? {
            books::remove_cached_file(&state, std::path::Path::new(&path));
        }
        state.lib.wipe()?;
        for dir in [state.paths.cache(), state.paths.covers()] {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir)?;
        }
    } else {
        state.lib.remove_drive_source()?;
    }
    let _ = app.emit("library-changed", ());
    Ok(())
}

// --------------------------------------------------------------- settings

#[tauri::command]
pub fn get_prefs(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    Ok(state
        .lib
        .setting_json::<serde_json::Value>(PREFS_KEY)?
        .unwrap_or_else(|| serde_json::json!({})))
}

#[tauri::command]
pub fn set_prefs(state: State<'_, AppState>, prefs: serde_json::Value) -> CmdResult<()> {
    if !prefs.is_object() {
        return Err(CmdError::invalid("prefs must be an object"));
    }
    state.lib.set_setting_json(PREFS_KEY, &prefs)?;
    books::enforce_cache_cap(&state, None)
}

#[tauri::command]
pub fn cache_stats(state: State<'_, AppState>) -> CmdResult<CacheStats> {
    Ok(state.lib.cache_stats()?)
}

#[tauri::command]
pub fn clear_cache(state: State<'_, AppState>) -> CmdResult<()> {
    let keep = state.open_book_key.lock().unwrap().clone();
    for path in state.lib.cache_clear_unpinned(keep.as_deref())? {
        books::remove_cached_file(&state, std::path::Path::new(&path));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub device_id: String,
    pub pdf_error: Option<String>,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.paths.root.to_string_lossy().into_owned(),
        device_id: state.device_id.clone(),
        pdf_error: state.pdf.as_ref().err().cloned(),
    }
}

/// A book passed on the command line, to open on startup (once).
#[tauri::command]
pub fn take_pending_open(state: State<'_, AppState>) -> Option<i64> {
    state.pending_open.lock().unwrap().take()
}
