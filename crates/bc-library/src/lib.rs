//! The local library database.
//!
//! The library mirrors a Drive folder tree in `nodes`, keeps one row per
//! distinct book file in `books`, and stores everything the user does
//! (progress, bookmarks, highlights, collections) locally. Drive is never
//! written to; overrides live only here.

mod cache;
mod collections;
mod covers;
mod notes;
mod nodes;
mod progress;
mod query;
mod schema;
mod settings;
mod sync;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

pub use cache::{CacheEntry, CacheStats};
pub use collections::Collection;
pub use covers::save_cover_thumbnail;
pub use nodes::{Breadcrumb, FolderListing, FolderSummary, NodeRecord, Source, SourceKind};
pub use notes::{Annotation, Bookmark, NewAnnotation};
pub use progress::{ProgressRecord, Stats};
pub use query::{BookDetails, BookQuery, BookSource, BookSummary, SortKey};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("book file: {0}")]
    Reader(#[from] bc_reader::ReaderError),
    #[error("image: {0}")]
    Image(String),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

pub struct Library {
    conn: Mutex<Connection>,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        schema::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The stable id of this installation, created on first use.
    pub fn device_id(&self) -> Result<String> {
        if let Some(id) = self.setting("device_id")? {
            return Ok(id);
        }
        let id = bc_core::new_id();
        self.set_setting("device_id", &id)?;
        Ok(id)
    }

    /// Delete every row (used by "Disconnect and wipe").
    pub fn wipe(&self) -> Result<()> {
        let c = self.conn();
        c.execute_batch(
            "DELETE FROM collection_books; DELETE FROM collections; DELETE FROM annotations;
             DELETE FROM bookmarks; DELETE FROM progress; DELETE FROM cache_entries;
             DELETE FROM books; DELETE FROM nodes; DELETE FROM sources; DELETE FROM accounts;
             DELETE FROM reading_days; DELETE FROM settings WHERE key <> 'device_id';",
        )?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn lib_with_tree() -> (Library, i64) {
        let lib = Library::open_in_memory().unwrap();
        let src = lib.add_drive_source(Some("me@example.com"), "root", "Books").unwrap();
        let n = |id: &str, parent: &str, name: &str, folder: bool, mime: &str| NodeRecord {
            drive_id: id.into(),
            parent_drive_id: Some(parent.into()),
            name: name.into(),
            mime_type: Some(mime.into()),
            size: Some(1000),
            md5: if folder { None } else { Some(format!("md5-{id}")) },
            modified_time: None,
            is_folder: folder,
            is_trashed: false,
            shortcut_target_id: None,
            thumbnail_link: None,
        };
        const FOLDER: &str = "application/vnd.google-apps.folder";
        lib.upsert_nodes(
            src,
            &[
                n("sf", "root", "Sci-Fi", true, FOLDER),
                n("cl", "root", "Classics", true, FOLDER),
                n("dune", "sf", "Dune.epub", false, "application/epub+zip"),
                n("found", "sf", "Foundation.pdf", false, "application/pdf"),
                n("moby", "cl", "Moby Dick.epub", false, "application/octet-stream"),
                n("doc", "root", "Notes", false, "application/vnd.google-apps.document"),
                n("top", "root", "Top Level.pdf", false, "application/pdf"),
            ],
        )
        .unwrap();
        (lib, src)
    }
}
