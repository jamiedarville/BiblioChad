//! PDFium wrapper.
//!
//! PDFium is not thread-safe and its documents borrow the library handle, so
//! a single worker thread owns the library and a small LRU of open
//! documents. Callers talk to it through [`PdfEngine`], which is cheap to
//! clone and `Send + Sync`.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use image::ImageFormat;
use lru::LruCache;
use pdfium_render::prelude::*;
use serde::Serialize;

use crate::{BookMetadata, ReaderError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct PdfInfo {
    pub page_count: u32,
    /// Page sizes in PDF points (width, height).
    pub page_sizes: Vec<(f32, f32)>,
    pub metadata: BookMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<u32>,
    pub children: Vec<OutlineItem>,
}

/// One run of text with its box, in fractions of the page size (origin top
/// left), for building a selectable text layer.
#[derive(Debug, Clone, Serialize)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub page: u32,
    pub excerpt: String,
}

type Reply<T> = mpsc::Sender<Result<T>>;

enum Job {
    Info(PathBuf, Reply<PdfInfo>),
    Render {
        path: PathBuf,
        page: u32,
        width: u32,
        reply: Reply<Vec<u8>>,
    },
    Text(PathBuf, u32, Reply<Vec<TextRun>>),
    Outline(PathBuf, Reply<Vec<OutlineItem>>),
    Search(PathBuf, String, usize, Reply<Vec<SearchHit>>),
    Close(PathBuf, Reply<()>),
}

/// Handle to the PDFium worker thread.
#[derive(Clone)]
pub struct PdfEngine {
    tx: mpsc::Sender<Job>,
}

fn pdf_err(e: impl std::fmt::Display) -> ReaderError {
    ReaderError::Pdf(e.to_string())
}

impl PdfEngine {
    /// Load PDFium from `lib_dir` (the folder containing `pdfium.dll` /
    /// `libpdfium.so`), falling back to the system library search path.
    pub fn start(lib_dir: Option<&Path>) -> Result<Self> {
        let lib_dir = lib_dir.map(Path::to_path_buf);
        let (tx, rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        thread::Builder::new()
            .name("pdfium".into())
            .spawn(move || {
                // Keep the error from the explicit path: falling back to the
                // system search would otherwise hide why it failed.
                let bindings = match &lib_dir {
                    Some(dir) => {
                        let path = Pdfium::pdfium_platform_library_name_at_path(dir);
                        Pdfium::bind_to_library(&path)
                            .map_err(|e| format!("{}: {e}", path.display()))
                    }
                    None => Pdfium::bind_to_system_library().map_err(|e| {
                        format!("not found next to the app or on the system path ({e})")
                    }),
                };
                match bindings {
                    Ok(b) => {
                        // The library lives for the rest of the process.
                        let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(b)));
                        let _ = ready_tx.send(Ok(()));
                        worker(pdfium, rx)
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(pdf_err(format!("could not load PDFium: {e}"))));
                    }
                }
            })
            .map_err(ReaderError::Io)?;
        ready_rx
            .recv()
            .map_err(|_| pdf_err("PDF worker failed to start"))??;
        Ok(Self { tx })
    }

    fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Job) -> Result<T> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .send(make(rtx))
            .map_err(|_| pdf_err("PDF worker stopped"))?;
        rrx.recv().map_err(|_| pdf_err("PDF worker stopped"))?
    }

    pub fn info(&self, path: &Path) -> Result<PdfInfo> {
        self.call(|r| Job::Info(path.to_path_buf(), r))
    }

    /// Render page `page` (zero-based) at `width` pixels wide, as PNG bytes.
    pub fn render_png(&self, path: &Path, page: u32, width: u32) -> Result<Vec<u8>> {
        self.call(|reply| Job::Render {
            path: path.to_path_buf(),
            page,
            width,
            reply,
        })
    }

    pub fn text_runs(&self, path: &Path, page: u32) -> Result<Vec<TextRun>> {
        self.call(|r| Job::Text(path.to_path_buf(), page, r))
    }

    pub fn outline(&self, path: &Path) -> Result<Vec<OutlineItem>> {
        self.call(|r| Job::Outline(path.to_path_buf(), r))
    }

    pub fn search(&self, path: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.call(|r| Job::Search(path.to_path_buf(), query.to_string(), limit, r))
    }

    /// Drop a cached open document (e.g. before its file is deleted).
    /// Waits until the worker has released the file handle.
    pub fn close(&self, path: &Path) {
        let _ = self.call(|r| Job::Close(path.to_path_buf(), r));
    }
}

fn worker(pdfium: &'static Pdfium, rx: mpsc::Receiver<Job>) {
    let mut docs: LruCache<PathBuf, PdfDocument<'static>> =
        LruCache::new(NonZeroUsize::new(4).unwrap());

    fn doc<'c>(
        pdfium: &'static Pdfium,
        docs: &'c mut LruCache<PathBuf, PdfDocument<'static>>,
        path: &Path,
    ) -> Result<&'c PdfDocument<'static>> {
        if !docs.contains(path) {
            let d = pdfium.load_pdf_from_file(path, None).map_err(pdf_err)?;
            docs.put(path.to_path_buf(), d);
        }
        Ok(docs.get(path).expect("just inserted"))
    }

    while let Ok(job) = rx.recv() {
        match job {
            Job::Info(path, reply) => {
                let _ = reply.send(doc(pdfium, &mut docs, &path).and_then(info_of));
            }
            Job::Render {
                path,
                page,
                width,
                reply,
            } => {
                let r = doc(pdfium, &mut docs, &path).and_then(|d| render(d, page, width));
                let _ = reply.send(r);
            }
            Job::Text(path, page, reply) => {
                let _ = reply.send(doc(pdfium, &mut docs, &path).and_then(|d| text_runs(d, page)));
            }
            Job::Outline(path, reply) => {
                let _ = reply.send(doc(pdfium, &mut docs, &path).map(outline_of));
            }
            Job::Search(path, q, limit, reply) => {
                let _ = reply.send(doc(pdfium, &mut docs, &path).map(|d| search(d, &q, limit)));
            }
            Job::Close(path, reply) => {
                docs.pop(&path);
                let _ = reply.send(Ok(()));
            }
        }
    }
}

fn non_empty(s: Option<PdfDocumentMetadataTag>) -> Option<String> {
    s.map(|t| t.value().trim().to_string())
        .filter(|v| !v.is_empty())
}

fn info_of(d: &PdfDocument) -> Result<PdfInfo> {
    let pages = d.pages();
    let page_count = pages.len() as u32;
    let mut page_sizes = Vec::with_capacity(page_count as usize);
    for i in 0..pages.len() {
        let size = pages.page_size(i).map_err(pdf_err)?;
        page_sizes.push((size.width().value, size.height().value));
    }
    let meta = d.metadata();
    let authors = non_empty(meta.get(PdfDocumentMetadataTagType::Author))
        .map(|a| {
            a.split([';', '&'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok(PdfInfo {
        page_count,
        page_sizes,
        metadata: BookMetadata {
            title: non_empty(meta.get(PdfDocumentMetadataTagType::Title)),
            authors,
            description: non_empty(meta.get(PdfDocumentMetadataTagType::Subject)),
            page_count: Some(page_count),
            ..Default::default()
        },
    })
}

fn page_of<'a>(d: &'a PdfDocument, page: u32) -> Result<PdfPage<'a>> {
    let idx = PdfPageIndex::try_from(page).map_err(|_| pdf_err("page out of range"))?;
    d.pages().get(idx).map_err(pdf_err)
}

fn render(d: &PdfDocument, page: u32, width: u32) -> Result<Vec<u8>> {
    let width = width.clamp(64, 4096) as i32;
    let p = page_of(d, page)?;
    let cfg = PdfRenderConfig::new()
        .set_target_width(width)
        .set_maximum_height(width * 4)
        .render_form_data(true)
        .render_annotations(true);
    let img = p.render_with_config(&cfg).map_err(pdf_err)?.as_image();
    let mut out = std::io::Cursor::new(Vec::new());
    img.into_rgb8()
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| ReaderError::Image(e.to_string()))?;
    Ok(out.into_inner())
}

fn text_runs(d: &PdfDocument, page: u32) -> Result<Vec<TextRun>> {
    let p = page_of(d, page)?;
    let (pw, ph) = (p.width().value, p.height().value);
    if pw <= 0.0 || ph <= 0.0 {
        return Ok(Vec::new());
    }
    let text = p.text().map_err(pdf_err)?;
    let mut runs = Vec::new();
    for seg in text.segments().iter() {
        let t = seg.text();
        if t.trim().is_empty() {
            continue;
        }
        let b = seg.bounds();
        runs.push(TextRun {
            text: t,
            x: b.left().value / pw,
            y: 1.0 - b.top().value / ph,
            w: (b.right().value - b.left().value) / pw,
            h: (b.top().value - b.bottom().value) / ph,
        });
    }
    Ok(runs)
}

fn outline_of(d: &PdfDocument) -> Vec<OutlineItem> {
    fn walk(first: Option<PdfBookmark>, depth: usize) -> Vec<OutlineItem> {
        let mut out = Vec::new();
        let mut cur = first;
        let mut guard = 0;
        while let Some(b) = cur {
            guard += 1;
            if guard > 10_000 || depth > 32 {
                break;
            }
            let page = b
                .destination()
                .and_then(|dest| dest.page_index().ok())
                .map(|i| i as u32);
            let children = if depth < 32 {
                walk(b.first_child(), depth + 1)
            } else {
                Vec::new()
            };
            out.push(OutlineItem {
                title: b.title().unwrap_or_default().trim().to_string(),
                page,
                children,
            });
            cur = b.next_sibling();
        }
        out
    }
    walk(d.bookmarks().root(), 0)
}

fn search(d: &PdfDocument, query: &str, limit: usize) -> Vec<SearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (i, page) in d.pages().iter().enumerate() {
        let Ok(text) = page.text() else { continue };
        let all = text.all();
        let lower = all.to_lowercase();
        // Lowercasing can change byte offsets for some scripts; work on chars.
        let hay: Vec<char> = lower.chars().collect();
        let orig: Vec<char> = all.chars().collect();
        let n: Vec<char> = needle.chars().collect();
        if hay.len() != orig.len() || n.is_empty() {
            if lower.contains(&needle) {
                hits.push(SearchHit {
                    page: i as u32,
                    excerpt: needle.clone(),
                });
            }
            continue;
        }
        let mut start = 0;
        while start + n.len() <= hay.len() {
            if hay[start..start + n.len()] == n[..] {
                let a = start.saturating_sub(40);
                let b = (start + n.len() + 40).min(orig.len());
                let excerpt: String = orig[a..b].iter().collect();
                hits.push(SearchHit {
                    page: i as u32,
                    excerpt: excerpt.split_whitespace().collect::<Vec<_>>().join(" "),
                });
                if hits.len() >= limit {
                    return hits;
                }
                start += n.len();
            } else {
                start += 1;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs only when PDFium is available (set `PDFIUM_DIR` or install it on
    /// the library path) and `BC_TEST_PDF` points at a sample PDF.
    #[test]
    fn renders_when_pdfium_present() {
        let Ok(sample) = std::env::var("BC_TEST_PDF") else {
            return;
        };
        let dir = std::env::var("PDFIUM_DIR").ok();
        let engine = match PdfEngine::start(dir.as_deref().map(Path::new)) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skipping: {e}");
                return;
            }
        };
        let p = Path::new(&sample);
        let info = engine.info(p).unwrap();
        assert!(info.page_count > 0);
        let png = engine.render_png(p, 0, 400).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let _ = engine.text_runs(p, 0).unwrap();
        let _ = engine.outline(p).unwrap();
    }
}
