//! BiblioChad app shell: wires the Rust core to the WebView2 UI.

mod books;
mod commands;
mod drive_sync;
mod error;
mod protocol;
mod state;

use std::path::PathBuf;
use std::time::Duration;

use bc_core::paths::AppPaths;
use tauri::{Emitter, Manager, WindowEvent};

use state::AppState;

fn init_logging(paths: &AppPaths) {
    use tracing_subscriber::{fmt, EnvFilter};
    let _ = std::fs::create_dir_all(paths.logs());
    let log_path = paths.logs().join("bibliochad.log");
    // Keep the log small: start over once it passes 5 MB.
    if std::fs::metadata(&log_path)
        .map(|m| m.len() > 5 * 1024 * 1024)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&log_path, paths.logs().join("bibliochad.old.log"));
    }
    let filter =
        EnvFilter::try_from_env("BIBLIOCHAD_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            let _ = fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .try_init();
        }
        Err(_) => {
            let _ = fmt().with_env_filter(filter).try_init();
        }
    }
}

/// Where to look for pdfium: `PDFIUM_DIR`, the bundled resources folder,
/// then next to the executable.
fn pdfium_dir(app: &tauri::App) -> Option<PathBuf> {
    let lib_name = if cfg!(windows) {
        "pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "libpdfium.dylib"
    } else {
        "libpdfium.so"
    };
    let mut candidates = Vec::new();
    if let Ok(d) = std::env::var("PDFIUM_DIR") {
        candidates.push(PathBuf::from(d));
    }
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("resources").join("pdfium"));
        candidates.push(res.join("pdfium"));
        candidates.push(res);
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        candidates.push(exe_dir);
    }
    candidates.into_iter().find(|d| d.join(lib_name).exists())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("bibliochad", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(protocol::handle(&app, &request));
            });
        })
        .setup(|app| {
            let paths = match std::env::var("BIBLIOCHAD_DATA_DIR") {
                Ok(d) => AppPaths::at(d),
                Err(_) => AppPaths::platform_default()?,
            };
            init_logging(&paths);
            tracing::info!(version = env!("CARGO_PKG_VERSION"), data = %paths.root.display(), "starting BiblioChad");
            let state = AppState::init(paths, pdfium_dir(app).as_deref())?;
            // Files passed on the command line ("Open with BiblioChad", or
            // the file association) are imported, and the first is opened.
            let mut first = None;
            for arg in std::env::args_os().skip(1) {
                let path = PathBuf::from(arg);
                if path.is_file() {
                    match books::import_local_file(&state, &path) {
                        Ok(id) => {
                            first.get_or_insert(id);
                        }
                        Err(e) => tracing::warn!("could not open {}: {}", path.display(), e.message),
                    }
                }
            }
            *state.pending_open.lock().unwrap() = first;
            app.manage(state);

            // Restore the Drive session and keep the library fresh.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle.state::<AppState>().restore_drive().await;
                let _ = handle.emit("library-changed", ());
                loop {
                    if let Err(e) = drive_sync::sync_all(handle.clone()).await {
                        if e.kind == "reauth" {
                            *handle.state::<AppState>().drive.write().await = None;
                            let _ = handle.emit("drive-reauth", e.message.clone());
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(10 * 60)).await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Give the reader a moment to flush its position, then close.
                api.prevent_close();
                let _ = window.emit("app-closing", ());
                let w = window.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    let _ = w.destroy();
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::list_folder,
            commands::query_books,
            commands::recently_read,
            commands::book_details,
            commands::set_book_overrides,
            commands::set_favorite,
            commands::set_status,
            commands::set_pinned,
            commands::collections,
            commands::create_collection,
            commands::rename_collection,
            commands::delete_collection,
            commands::set_in_collection,
            commands::import_local_files,
            commands::remove_local_book,
            commands::export_annotations,
            commands::open_book,
            commands::save_progress,
            commands::close_book,
            commands::add_bookmark,
            commands::delete_bookmark,
            commands::add_annotation,
            commands::update_annotation,
            commands::delete_annotation,
            commands::pdf_text,
            commands::pdf_search,
            commands::drive_status,
            commands::drive_import_client_file,
            commands::drive_connect,
            commands::drive_list_folders,
            commands::drive_choose_root,
            commands::drive_sync_now,
            commands::drive_disconnect,
            commands::get_prefs,
            commands::set_prefs,
            commands::cache_stats,
            commands::clear_cache,
            commands::app_info,
            commands::take_pending_open,
        ])
        .run(tauri::generate_context!())
        .expect("error while running BiblioChad");
}
