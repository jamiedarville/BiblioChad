//! Getting book bytes onto disk and extracting metadata from them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bc_core::BookFormat;
use bc_reader::epub::EpubArchive;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{CmdError, CmdResult};
use crate::state::AppState;

#[derive(Clone, Serialize)]
struct DownloadProgress {
    book_id: i64,
    done: u64,
    total: Option<u64>,
}

fn cache_file_name(key: &str, md5: Option<&str>, format: BookFormat) -> String {
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}-{}.{}", md5.unwrap_or("nomd5"), format.as_str())
}

/// Make sure the book's file is available locally and return its path.
/// Drive books are downloaded (md5-verified) into the cache.
pub async fn ensure_local(app: &AppHandle, book_id: i64) -> CmdResult<PathBuf> {
    let state = app.state::<AppState>();
    let src = state.lib.book_source(book_id)?;
    if let Some(p) = &src.local_path {
        let p = PathBuf::from(p);
        return if p.exists() {
            Ok(p)
        } else {
            Err(CmdError::new(
                "not_found",
                format!("The imported file is gone: {}", p.display()),
            ))
        };
    }
    if let Some(hit) = state.lib.cache_lookup(&src.key, src.md5.as_deref())? {
        let p = PathBuf::from(&hit.path);
        if p.exists() {
            return Ok(p);
        }
        state.lib.cache_remove(&src.key)?;
    }
    let file_id = src
        .drive_file_id
        .clone()
        .ok_or_else(|| CmdError::internal("book has neither a Drive id nor a local path"))?;
    let drive = state.drive().await.ok_or_else(|| {
        CmdError::new(
            "offline",
            "This book is not downloaded yet. Connect Google Drive to open it.",
        )
    })?;
    let dest = state
        .paths
        .cache()
        .join(cache_file_name(&src.key, src.md5.as_deref(), src.format));
    let mut last = Instant::now() - Duration::from_secs(1);
    let app2 = app.clone();
    let bytes = drive
        .download_to(&file_id, &dest, src.md5.as_deref(), move |done, total| {
            if last.elapsed() > Duration::from_millis(150) || Some(done) == total {
                last = Instant::now();
                let _ = app2.emit(
                    "download-progress",
                    DownloadProgress {
                        book_id,
                        done,
                        total,
                    },
                );
            }
        })
        .await?;
    state.lib.cache_put(
        &src.key,
        src.md5.as_deref(),
        &dest.to_string_lossy(),
        bytes as i64,
    )?;
    enforce_cache_cap(&state, Some(&src.key))?;
    Ok(dest)
}

/// Evict least-recently-used, unpinned books over the configured cap.
pub fn enforce_cache_cap(state: &AppState, keep: Option<&str>) -> CmdResult<()> {
    let cap = state.core_prefs().cache_cap_mb as i64 * 1024 * 1024;
    let open = state.open_book_key.lock().unwrap().clone();
    let keep = keep.map(str::to_string).or(open);
    for path in state.lib.cache_evict(cap, keep.as_deref())? {
        remove_cached_file(state, Path::new(&path));
    }
    Ok(())
}

pub fn remove_cached_file(state: &AppState, path: &Path) {
    // Release open handles first: Windows cannot delete open files.
    if let Ok(pdf) = &state.pdf {
        pdf.close(path);
    }
    state.epubs.lock().unwrap().clear();
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("could not delete cached file: {e}");
        }
    }
}

/// Open (or reuse) the EPUB archive for a book.
pub fn epub_archive(
    state: &AppState,
    book_id: i64,
    path: &Path,
) -> CmdResult<Arc<Mutex<EpubArchive>>> {
    let mut cache = state.epubs.lock().unwrap();
    if let Some(a) = cache.get(&book_id) {
        return Ok(a.clone());
    }
    let a = Arc::new(Mutex::new(EpubArchive::open(path)?));
    cache.put(book_id, a.clone());
    Ok(a)
}

/// Extract title/author/cover/etc. from the file and store them.
pub fn extract_metadata(
    state: &AppState,
    book_id: i64,
    path: &Path,
    format: BookFormat,
) -> CmdResult<()> {
    let cover_dest = state.cover_file(book_id);
    let had_cover = state.lib.cover_path(book_id)?.is_some();
    match format {
        BookFormat::Epub => {
            let archive = epub_archive(state, book_id, path)?;
            let mut a = archive.lock().unwrap();
            let pkg = a.package()?;
            let mut cover = None;
            if let Some(cp) = &pkg.cover_path {
                match a.read(cp).map_err(CmdError::from).and_then(|bytes| {
                    bc_library::save_cover_thumbnail(&bytes, &cover_dest).map_err(CmdError::from)
                }) {
                    Ok(()) => cover = Some(cover_dest.to_string_lossy().into_owned()),
                    Err(e) => tracing::warn!(book_id, "cover extraction failed: {}", e.message),
                }
            }
            state
                .lib
                .apply_metadata(book_id, &pkg.metadata, cover.as_deref())?;
        }
        BookFormat::Pdf => {
            let pdf = state.pdf()?;
            let info = pdf.info(path)?;
            let mut cover = None;
            if !had_cover {
                if let Ok(png) = pdf.render_png(path, 0, 600) {
                    if bc_library::save_cover_thumbnail(&png, &cover_dest).is_ok() {
                        cover = Some(cover_dest.to_string_lossy().into_owned());
                    }
                }
            }
            state
                .lib
                .apply_metadata(book_id, &info.metadata, cover.as_deref())?;
        }
    }
    Ok(())
}

/// Hash a local file (SHA-256) and import it.
pub fn import_local_file(state: &AppState, path: &Path) -> CmdResult<i64> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let format = BookFormat::detect(None, &name)
        .ok_or_else(|| CmdError::invalid(format!("{name} is not an EPUB or PDF")))?;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0i64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as i64;
        hasher.update(&buf[..n]);
    }
    let sha = hex::encode(hasher.finalize());
    let id = state.lib.import_local(path, &sha, format, size)?;
    if !state.lib.book_source(id)?.metadata_extracted {
        if let Err(e) = extract_metadata(state, id, path, format) {
            tracing::warn!("metadata extraction failed for {name}: {}", e.message);
        }
    }
    Ok(id)
}
