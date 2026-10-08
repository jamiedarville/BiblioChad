//! Cross-device sync model.
//!
//! A single JSON document lives in the Drive `appDataFolder`. Each device
//! downloads it, merges it with its local state, and uploads the result.
//! The merge is a pure, commutative function so the order in which devices
//! sync does not matter:
//!
//! * reading positions: last writer wins per book, by `updated_at`
//!   (ties broken deterministically by device id);
//! * bookmarks and annotations: merged by id, newest `updated_at` wins, and
//!   deletions are tombstones (`deleted_at`) that are never dropped.

use std::collections::BTreeMap;

use bc_core::{Locator, ReadStatus};
use serde::{Deserialize, Serialize};

pub const SYNC_FILE_NAME: &str = "bibliochad-sync.json";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookEntry {
    pub locator: Locator,
    pub percent: f64,
    #[serde(default)]
    pub status: ReadStatus,
    pub updated_at: i64,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub book_key: String,
    pub locator: Locator,
    #[serde(default)]
    pub label: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationEntry {
    pub book_key: String,
    /// "highlight" or "note".
    pub kind: String,
    pub locator: Locator,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncDoc {
    pub schema: u32,
    #[serde(default)]
    pub books: BTreeMap<String, BookEntry>,
    #[serde(default)]
    pub bookmarks: BTreeMap<String, BookmarkEntry>,
    #[serde(default)]
    pub annotations: BTreeMap<String, AnnotationEntry>,
}

impl Default for SyncDoc {
    fn default() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            books: BTreeMap::new(),
            bookmarks: BTreeMap::new(),
            annotations: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum SyncError {
    Json(serde_json::Error),
    NewerSchema(u32),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Json(e) => write!(f, "sync file is not valid JSON: {e}"),
            SyncError::NewerSchema(v) => {
                write!(
                    f,
                    "sync file uses schema {v}; update BiblioChad on this device"
                )
            }
        }
    }
}

impl std::error::Error for SyncError {}

impl SyncDoc {
    pub fn from_json(bytes: &[u8]) -> Result<Self, SyncError> {
        let doc: SyncDoc = serde_json::from_slice(bytes).map_err(SyncError::Json)?;
        if doc.schema > SCHEMA_VERSION {
            return Err(SyncError::NewerSchema(doc.schema));
        }
        Ok(doc)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("sync doc serializes")
    }
}

/// Total order used to pick a winner between two versions of one record.
trait Versioned {
    fn version(&self) -> (i64, String);
}

impl Versioned for BookEntry {
    fn version(&self) -> (i64, String) {
        (
            self.updated_at,
            format!("{}|{}", self.device_id, self.locator.to_json()),
        )
    }
}

impl Versioned for BookmarkEntry {
    fn version(&self) -> (i64, String) {
        // A tombstone beats a live record written at the same instant.
        let ts = self.updated_at.max(self.deleted_at.unwrap_or(i64::MIN));
        (
            ts,
            format!(
                "{}|{}",
                self.deleted_at.is_some() as u8,
                serde_json::to_string(self).unwrap()
            ),
        )
    }
}

impl Versioned for AnnotationEntry {
    fn version(&self) -> (i64, String) {
        let ts = self.updated_at.max(self.deleted_at.unwrap_or(i64::MIN));
        (
            ts,
            format!(
                "{}|{}",
                self.deleted_at.is_some() as u8,
                serde_json::to_string(self).unwrap()
            ),
        )
    }
}

fn merge_map<T: Versioned + Clone>(
    a: &BTreeMap<String, T>,
    b: &BTreeMap<String, T>,
) -> BTreeMap<String, T> {
    let mut out = a.clone();
    for (k, vb) in b {
        match out.get(k) {
            Some(va) if va.version() >= vb.version() => {}
            _ => {
                out.insert(k.clone(), vb.clone());
            }
        }
    }
    out
}

/// Merge two sync documents. Commutative, associative and idempotent.
pub fn merge(a: &SyncDoc, b: &SyncDoc) -> SyncDoc {
    SyncDoc {
        schema: SCHEMA_VERSION,
        books: merge_map(&a.books, &b.books),
        bookmarks: merge_map(&a.bookmarks, &b.bookmarks),
        annotations: merge_map(&a.annotations, &b.annotations),
    }
}

/// A suggestion to jump to a position another device reached.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResumePrompt {
    pub device_id: String,
    pub locator: Locator,
    pub percent: f64,
    pub updated_at: i64,
    /// Human label such as "page 212" or "41%".
    pub label: String,
}

/// Decide whether opening a book should offer to jump to another device's
/// position: it must be newer than ours and meaningfully different.
pub fn resume_prompt(
    local: Option<&BookEntry>,
    other: &BookEntry,
    my_device: &str,
) -> Option<ResumePrompt> {
    if other.device_id == my_device {
        return None;
    }
    let ahead = match local {
        None => other.percent > 0.0,
        Some(l) => {
            other.updated_at > l.updated_at
                && l.locator
                    .meaningfully_different(&other.locator, l.percent, other.percent)
        }
    };
    ahead.then(|| ResumePrompt {
        device_id: other.device_id.clone(),
        locator: other.locator.clone(),
        percent: other.percent,
        updated_at: other.updated_at,
        label: other.locator.label(other.percent),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bc_core::FitMode;

    fn pdf(page: u32, ts: i64, dev: &str) -> BookEntry {
        BookEntry {
            locator: Locator::Pdf {
                page,
                offset: 0.0,
                fit: FitMode::Width,
                zoom: 1.0,
            },
            percent: page as f64 / 500.0,
            status: ReadStatus::Reading,
            updated_at: ts,
            device_id: dev.into(),
        }
    }

    fn ann(ts: i64, deleted: Option<i64>, note: &str) -> AnnotationEntry {
        AnnotationEntry {
            book_key: "drive:1".into(),
            kind: "highlight".into(),
            locator: Locator::Epub {
                cfi: "epubcfi(/6/2!/4/2)".into(),
                href: None,
                percent: 0.1,
            },
            text: Some("spice".into()),
            color: Some("yellow".into()),
            note: Some(note.into()),
            created_at: 1,
            updated_at: ts,
            deleted_at: deleted,
        }
    }

    #[test]
    fn last_writer_wins_per_book() {
        let mut a = SyncDoc::default();
        let mut b = SyncDoc::default();
        a.books.insert("drive:1".into(), pdf(10, 100, "laptop"));
        b.books.insert("drive:1".into(), pdf(211, 200, "tablet"));
        b.books.insert("drive:2".into(), pdf(3, 50, "tablet"));
        let m = merge(&a, &b);
        assert_eq!(m.books["drive:1"].device_id, "tablet");
        assert_eq!(m.books.len(), 2);
        assert_eq!(merge(&a, &b), merge(&b, &a));
    }

    #[test]
    fn ties_are_deterministic() {
        let mut a = SyncDoc::default();
        let mut b = SyncDoc::default();
        a.books.insert("k".into(), pdf(1, 100, "a"));
        b.books.insert("k".into(), pdf(9, 100, "b"));
        assert_eq!(merge(&a, &b), merge(&b, &a));
    }

    #[test]
    fn annotations_merge_by_id_with_tombstones() {
        let mut a = SyncDoc::default();
        let mut b = SyncDoc::default();
        a.annotations.insert("x".into(), ann(100, None, "old"));
        b.annotations.insert("x".into(), ann(150, Some(150), "old"));
        a.annotations
            .insert("y".into(), ann(300, None, "edited later"));
        b.annotations
            .insert("y".into(), ann(200, None, "edited first"));
        let m = merge(&a, &b);
        assert!(m.annotations["x"].deleted_at.is_some(), "deletion survives");
        assert_eq!(m.annotations["y"].note.as_deref(), Some("edited later"));
        // Idempotent and commutative.
        assert_eq!(merge(&m, &m), m);
        assert_eq!(merge(&b, &a), m);
        // Associative.
        let mut c = SyncDoc::default();
        c.annotations
            .insert("x".into(), ann(400, None, "resurrected"));
        assert_eq!(merge(&merge(&a, &b), &c), merge(&a, &merge(&b, &c)));
    }

    #[test]
    fn prompts_only_when_other_device_is_meaningfully_ahead() {
        let mine = pdf(10, 100, "laptop");
        assert!(resume_prompt(Some(&mine), &pdf(11, 200, "tablet"), "laptop").is_none());
        let p = resume_prompt(Some(&mine), &pdf(211, 200, "tablet"), "laptop").unwrap();
        assert_eq!(p.label, "page 212");
        assert!(resume_prompt(Some(&mine), &pdf(211, 50, "tablet"), "laptop").is_none());
        assert!(resume_prompt(Some(&mine), &pdf(211, 500, "laptop"), "laptop").is_none());
        assert!(resume_prompt(None, &pdf(5, 1, "tablet"), "laptop").is_some());
    }

    #[test]
    fn json_roundtrip_and_schema_guard() {
        let mut a = SyncDoc::default();
        a.books.insert("k".into(), pdf(1, 1, "d"));
        let back = SyncDoc::from_json(&a.to_json()).unwrap();
        assert_eq!(back, a);
        assert!(matches!(
            SyncDoc::from_json(br#"{"schema":99}"#),
            Err(SyncError::NewerSchema(99))
        ));
        assert!(SyncDoc::from_json(b"nope").is_err());
    }
}
