//! Shared types for BiblioChad: formats, locators, progress, paths and errors.

pub mod error;
pub mod locator;
pub mod paths;
pub mod rank;

pub use error::{Error, Result};
pub use locator::{FitMode, Locator};

use serde::{Deserialize, Serialize};

/// Supported book formats (v1: EPUB and PDF only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    Epub,
    Pdf,
}

impl BookFormat {
    pub const EPUB_MIME: &'static str = "application/epub+zip";
    pub const PDF_MIME: &'static str = "application/pdf";

    pub fn as_str(self) -> &'static str {
        match self {
            BookFormat::Epub => "epub",
            BookFormat::Pdf => "pdf",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "epub" => Some(BookFormat::Epub),
            "pdf" => Some(BookFormat::Pdf),
            _ => None,
        }
    }

    /// Detect a format from a Drive mime type, falling back to the file
    /// extension when Drive reports something generic like
    /// `application/octet-stream`.
    pub fn detect(mime: Option<&str>, name: &str) -> Option<Self> {
        match mime {
            Some(Self::EPUB_MIME) => return Some(BookFormat::Epub),
            Some(Self::PDF_MIME) => return Some(BookFormat::Pdf),
            Some(m) if m.starts_with("application/vnd.google-apps.") => return None,
            _ => {}
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".epub") {
            Some(BookFormat::Epub)
        } else if lower.ends_with(".pdf") {
            Some(BookFormat::Pdf)
        } else {
            None
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            BookFormat::Epub => Self::EPUB_MIME,
            BookFormat::Pdf => Self::PDF_MIME,
        }
    }
}

/// Reading status shown on covers and used for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReadStatus {
    #[default]
    Unread,
    Reading,
    Finished,
}

impl ReadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadStatus::Unread => "unread",
            ReadStatus::Reading => "reading",
            ReadStatus::Finished => "finished",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unread" => Some(ReadStatus::Unread),
            "reading" => Some(ReadStatus::Reading),
            "finished" => Some(ReadStatus::Finished),
            _ => None,
        }
    }

    /// Status implied by a progress fraction (0.0..=1.0) when the user has
    /// not set one explicitly.
    pub fn from_percent(percent: f64) -> Self {
        if percent >= 0.995 {
            ReadStatus::Finished
        } else if percent > 0.0 {
            ReadStatus::Reading
        } else {
            ReadStatus::Unread
        }
    }
}

/// Milliseconds since the Unix epoch, UTC. Used for all stored timestamps.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A fresh random identifier (used for device ids, bookmarks, annotations).
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(BookFormat::detect(Some("application/pdf"), "x"), Some(BookFormat::Pdf));
        assert_eq!(
            BookFormat::detect(Some("application/octet-stream"), "Dune.EPUB"),
            Some(BookFormat::Epub)
        );
        assert_eq!(BookFormat::detect(None, "notes.txt"), None);
        assert_eq!(
            BookFormat::detect(Some("application/vnd.google-apps.document"), "a.pdf"),
            None
        );
    }

    #[test]
    fn status_from_percent() {
        assert_eq!(ReadStatus::from_percent(0.0), ReadStatus::Unread);
        assert_eq!(ReadStatus::from_percent(0.3), ReadStatus::Reading);
        assert_eq!(ReadStatus::from_percent(1.0), ReadStatus::Finished);
    }
}
