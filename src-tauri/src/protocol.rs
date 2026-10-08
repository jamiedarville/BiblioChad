//! The `bibliochad://` scheme. Book content reaches the webview only through
//! here, so every response carries a strict CSP and EPUB markup is scrubbed
//! of scripts before it is served.
//!
//! Routes:
//! * `/cover/{book_id}`
//! * `/epub/{book_id}/manifest` (JSON: entry names and sizes)
//! * `/epub/{book_id}/file/{archive path}`
//! * `/pdf/{book_id}/page/{index}?w={pixels}`

use std::path::PathBuf;

use percent_encoding::percent_decode_str;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

use crate::books;
use crate::state::AppState;

const CSP: &str = "default-src 'none'; img-src 'self' data: blob: bibliochad: http://bibliochad.localhost; \
                   style-src 'self' 'unsafe-inline' data: blob: bibliochad: http://bibliochad.localhost; \
                   font-src 'self' data: blob: bibliochad: http://bibliochad.localhost; media-src blob: data:; script-src 'none'";

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_SECURITY_POLICY, CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .expect("valid response")
}

fn not_found(msg: &str) -> Response<Vec<u8>> {
    respond(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        msg.as_bytes().to_vec(),
    )
}

fn error(msg: &str) -> Response<Vec<u8>> {
    respond(
        StatusCode::INTERNAL_SERVER_ERROR,
        "text/plain; charset=utf-8",
        msg.as_bytes().to_vec(),
    )
}

/// Path of a book that must already be on disk (opened through
/// `open_book`). The protocol never triggers downloads.
fn local_path(state: &AppState, book_id: i64) -> Option<PathBuf> {
    let src = state.lib.book_source(book_id).ok()?;
    if let Some(p) = src.local_path {
        return Some(PathBuf::from(p));
    }
    state
        .lib
        .cache_lookup(&src.key, src.md5.as_deref())
        .ok()
        .flatten()
        .map(|e| PathBuf::from(e.path))
}

pub fn handle(app: &AppHandle, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();
    let uri = req.uri();
    let path = uri.path().trim_start_matches('/');
    let mut parts = path.splitn(4, '/');
    let kind = parts.next().unwrap_or("");
    let Some(book_id) = parts.next().and_then(|s| s.parse::<i64>().ok()) else {
        return not_found("bad book id");
    };
    match kind {
        "cover" => {
            let Ok(Some(p)) = state.lib.cover_path(book_id) else {
                return not_found("no cover");
            };
            match std::fs::read(&p) {
                Ok(bytes) => respond(StatusCode::OK, "image/jpeg", bytes),
                Err(_) => not_found("cover missing"),
            }
        }
        "epub" => {
            let Some(file) = local_path(&state, book_id) else {
                return not_found("book not downloaded");
            };
            let archive = match books::epub_archive(&state, book_id, &file) {
                Ok(a) => a,
                Err(e) => return error(&e.message),
            };
            match parts.next() {
                Some("manifest") => {
                    let a = archive.lock().unwrap();
                    let body = serde_json::to_vec(a.entries()).unwrap_or_default();
                    respond(StatusCode::OK, "application/json", body)
                }
                Some("file") => {
                    let raw = parts.next().unwrap_or("");
                    let name = percent_decode_str(raw).decode_utf8_lossy().into_owned();
                    let mut a = archive.lock().unwrap();
                    match a.read(&name) {
                        Ok(bytes) => {
                            let mime = bc_reader::epub::mime_for(&name);
                            let body = if matches!(
                                mime,
                                "application/xhtml+xml" | "text/html" | "image/svg+xml"
                            ) {
                                bc_reader::sanitize::sanitize_markup(&bytes)
                            } else {
                                bytes
                            };
                            respond(StatusCode::OK, mime, body)
                        }
                        Err(bc_reader::ReaderError::NotFound(_)) => not_found("no such entry"),
                        Err(e) => error(&e.to_string()),
                    }
                }
                _ => not_found("unknown epub route"),
            }
        }
        "pdf" => {
            let Some(file) = local_path(&state, book_id) else {
                return not_found("book not downloaded");
            };
            let (Some("page"), Some(index)) = (
                parts.next(),
                parts.next().and_then(|s| s.parse::<u32>().ok()),
            ) else {
                return not_found("unknown pdf route");
            };
            let width = uri
                .query()
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("w=")))
                .and_then(|w| w.parse::<u32>().ok())
                .unwrap_or(1200);
            let pdf = match state.pdf() {
                Ok(p) => p,
                Err(e) => return error(&e.message),
            };
            match pdf.render_png(&file, index, width) {
                Ok(png) => respond(StatusCode::OK, "image/png", png),
                Err(e) => error(&e.to_string()),
            }
        }
        _ => not_found("unknown route"),
    }
}
