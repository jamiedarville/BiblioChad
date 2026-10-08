//! Book engines: EPUB container/OPF parsing with untrusted-input guards, and a
//! PDFium wrapper that renders page bitmaps.

pub mod epub;
pub mod pdf;
pub mod sanitize;

use serde::Serialize;

/// Metadata extracted from a book file. Every field is optional because real
/// books are messy.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct BookMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub description: Option<String>,
    pub page_count: Option<u32>,
}

impl BookMetadata {
    pub fn author_line(&self) -> Option<String> {
        if self.authors.is_empty() {
            None
        } else {
            Some(self.authors.join(", "))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a valid EPUB: {0}")]
    BadEpub(String),
    #[error("unsafe archive entry: {0}")]
    UnsafeEntry(String),
    #[error("archive entry too large: {0}")]
    TooLarge(String),
    #[error("entry not found: {0}")]
    NotFound(String),
    #[error("pdf: {0}")]
    Pdf(String),
    #[error("image: {0}")]
    Image(String),
}

pub type Result<T> = std::result::Result<T, ReaderError>;
