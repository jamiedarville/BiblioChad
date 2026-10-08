use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use bc_core::BookFormat;

use crate::query::BookSummary;
use crate::{Library, LibraryError, Result};

/// Folder id used for the virtual "Imported" shelf of local files.
pub const LOCAL_FOLDER_ID: &str = "local:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Drive,
    Local,
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub id: i64,
    pub kind: SourceKind,
    pub email: Option<String>,
    pub drive_root_id: Option<String>,
    pub name: String,
    pub changes_page_token: Option<String>,
    pub crawl_state: Option<String>,
    pub last_synced_at: Option<i64>,
}

/// A Drive item as reported by the API (or a local import).
#[derive(Debug, Clone, Default)]
pub struct NodeRecord {
    pub drive_id: String,
    pub parent_drive_id: Option<String>,
    pub name: String,
    /// For shortcuts, the *target's* mime type.
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub md5: Option<String>,
    pub modified_time: Option<String>,
    pub is_folder: bool,
    pub is_trashed: bool,
    pub shortcut_target_id: Option<String>,
    pub thumbnail_link: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderSummary {
    /// The id to navigate to (a folder shortcut resolves to its target).
    pub id: String,
    pub name: String,
    pub book_count: i64,
    pub folder_count: i64,
    pub is_shortcut: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Breadcrumb {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderListing {
    pub id: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub folders: Vec<FolderSummary>,
    pub books: Vec<BookSummary>,
}

fn source_from_row(r: &Row) -> rusqlite::Result<Source> {
    let kind: String = r.get("kind")?;
    Ok(Source {
        id: r.get("id")?,
        kind: if kind == "local" { SourceKind::Local } else { SourceKind::Drive },
        email: r.get("email")?,
        drive_root_id: r.get("drive_root_id")?,
        name: r.get("name")?,
        changes_page_token: r.get("changes_page_token")?,
        crawl_state: r.get("crawl_state")?,
        last_synced_at: r.get("last_synced_at")?,
    })
}

/// Name without its book extension, used as the title before metadata exists.
pub fn title_from_file_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for ext in [".epub", ".pdf"] {
        if lower.ends_with(ext) {
            return name[..name.len() - ext.len()].trim().to_string();
        }
    }
    name.to_string()
}

impl Library {
    /// Register (or replace) the Drive library root. Only one Drive source is
    /// supported in v1; choosing a new root clears the old mirror but keeps
    /// books, progress and notes (they are keyed by file id).
    pub fn add_drive_source(&self, email: Option<&str>, root_id: &str, name: &str) -> Result<i64> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM sources WHERE kind = 'drive'", [])?;
        tx.execute("DELETE FROM accounts", [])?;
        tx.execute(
            "INSERT INTO accounts(email, created_at) VALUES (?1, ?2)",
            params![email, bc_core::now_ms()],
        )?;
        let account = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO sources(kind, account_id, drive_root_id, name) VALUES ('drive', ?1, ?2, ?3)",
            params![account, root_id, name],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn drive_source(&self) -> Result<Option<Source>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT s.*, a.email FROM sources s LEFT JOIN accounts a ON a.id = s.account_id
                 WHERE s.kind = 'drive' LIMIT 1",
                [],
                source_from_row,
            )
            .optional()?)
    }

    pub fn remove_drive_source(&self) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM sources WHERE kind = 'drive'", [])?;
        c.execute("DELETE FROM accounts", [])?;
        Ok(())
    }

    fn local_source_id(&self) -> Result<i64> {
        let c = self.conn();
        if let Some(id) = c
            .query_row("SELECT id FROM sources WHERE kind = 'local'", [], |r| r.get(0))
            .optional()?
        {
            return Ok(id);
        }
        c.execute("INSERT INTO sources(kind, name) VALUES ('local', 'Imported')", [])?;
        Ok(c.last_insert_rowid())
    }

    pub fn set_source_sync_state(
        &self,
        source_id: i64,
        changes_page_token: Option<&str>,
        crawl_state: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE sources SET changes_page_token = COALESCE(?2, changes_page_token),
                                crawl_state = ?3, last_synced_at = ?4 WHERE id = ?1",
            params![source_id, changes_page_token, crawl_state, bc_core::now_ms()],
        )?;
        Ok(())
    }

    /// Insert or update Drive nodes. Book files get a `books` row on first
    /// sight. Returns the number of book files touched.
    pub fn upsert_nodes(&self, source_id: i64, nodes: &[NodeRecord]) -> Result<usize> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let mut books = 0;
        {
            let mut up = tx.prepare_cached(
                "INSERT INTO nodes(source_id, drive_id, parent_drive_id, name, mime_type, size, md5,
                                   modified_time, is_folder, is_trashed, shortcut_target_id,
                                   thumbnail_link, file_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(drive_id) DO UPDATE SET
                   source_id = excluded.source_id, parent_drive_id = excluded.parent_drive_id,
                   name = excluded.name, mime_type = excluded.mime_type, size = excluded.size,
                   md5 = excluded.md5, modified_time = excluded.modified_time,
                   is_folder = excluded.is_folder, is_trashed = excluded.is_trashed,
                   shortcut_target_id = excluded.shortcut_target_id,
                   thumbnail_link = COALESCE(excluded.thumbnail_link, nodes.thumbnail_link),
                   file_key = excluded.file_key",
            )?;
            let mut book = tx.prepare_cached(
                "INSERT INTO books(book_key, format, file_name, content_md5, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(book_key) DO UPDATE SET
                   -- A shortcut's name must not rename the target book.
                   file_name = CASE WHEN ?6 THEN books.file_name ELSE excluded.file_name END,
                   content_md5 = COALESCE(excluded.content_md5, books.content_md5),
                   -- Content changed: re-extract metadata on next open.
                   metadata_extracted_at = CASE
                     WHEN excluded.content_md5 IS NOT NULL
                      AND books.content_md5 IS NOT NULL
                      AND excluded.content_md5 <> books.content_md5
                     THEN NULL ELSE books.metadata_extracted_at END",
            )?;
            for n in nodes {
                let format = if n.is_folder {
                    None
                } else {
                    BookFormat::detect(n.mime_type.as_deref(), &n.name)
                };
                let file_key = format.map(|_| {
                    format!("drive:{}", n.shortcut_target_id.as_deref().unwrap_or(&n.drive_id))
                });
                up.execute(params![
                    source_id,
                    n.drive_id,
                    n.parent_drive_id,
                    n.name,
                    n.mime_type,
                    n.size,
                    n.md5,
                    n.modified_time,
                    n.is_folder,
                    n.is_trashed,
                    n.shortcut_target_id,
                    n.thumbnail_link,
                    file_key,
                ])?;
                if let (Some(fmt), Some(key)) = (format, &file_key) {
                    if !n.is_trashed {
                        book.execute(params![
                            key,
                            fmt.as_str(),
                            n.name,
                            n.md5,
                            bc_core::now_ms(),
                            n.shortcut_target_id.is_some()
                        ])?;
                        books += 1;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(books)
    }

    /// Mark a node and everything below it as gone (trashed, deleted, or
    /// moved out of the library). Books, progress and notes are kept.
    pub fn remove_subtree(&self, drive_id: &str) -> Result<usize> {
        let c = self.conn();
        let n = c.execute(
            "WITH RECURSIVE sub(id) AS (
               SELECT ?1
               UNION
               SELECT COALESCE(n.shortcut_target_id, n.drive_id) FROM nodes n JOIN sub ON n.parent_drive_id = sub.id
               UNION
               SELECT n.drive_id FROM nodes n JOIN sub ON n.parent_drive_id = sub.id
             )
             UPDATE nodes SET is_trashed = 1 WHERE drive_id IN (SELECT id FROM sub)",
            [drive_id],
        )?;
        Ok(n)
    }

    /// Whether `folder_id` is the library root or a live folder inside it.
    pub fn is_folder_in_library(&self, folder_id: &str) -> Result<bool> {
        let c = self.conn();
        let root: Option<String> = c
            .query_row("SELECT drive_root_id FROM sources WHERE kind = 'drive'", [], |r| r.get(0))
            .optional()?
            .flatten();
        if root.as_deref() == Some(folder_id) {
            return Ok(true);
        }
        let live: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE is_folder = 1 AND is_trashed = 0
                AND (drive_id = ?1 OR shortcut_target_id = ?1))",
            [folder_id],
            |r| r.get(0),
        )?;
        Ok(live)
    }

    pub fn node_parent(&self, drive_id: &str) -> Result<Option<Option<String>>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT parent_drive_id FROM nodes WHERE drive_id = ?1",
                [drive_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Books that have a Drive thumbnail but no cover yet.
    pub fn books_needing_thumbnails(&self, limit: usize) -> Result<Vec<(i64, String)>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT b.id, n.thumbnail_link FROM books b JOIN nodes n ON n.file_key = b.book_key
             WHERE b.cover_path IS NULL AND n.thumbnail_link IS NOT NULL AND n.is_trashed = 0
             GROUP BY b.id LIMIT ?1",
        )?;
        let rows = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn clear_thumbnail_link(&self, book_id: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE nodes SET thumbnail_link = NULL
             WHERE file_key = (SELECT book_key FROM books WHERE id = ?1)",
            [book_id],
        )?;
        Ok(())
    }

    /// Import a local file. `sha256` is the content hash (hex) and becomes
    /// the book identity. Returns the book id.
    pub fn import_local(
        &self,
        path: &std::path::Path,
        sha256: &str,
        format: BookFormat,
        size: i64,
    ) -> Result<i64> {
        let source = self.local_source_id()?;
        let key = format!("local:{sha256}");
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| LibraryError::Invalid("path has no file name".into()))?;
        let c = self.conn();
        c.execute(
            "INSERT INTO nodes(source_id, drive_id, parent_drive_id, name, mime_type, size, md5,
                               is_folder, is_trashed, local_path, file_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0, 0, ?7, ?2)
             ON CONFLICT(drive_id) DO UPDATE SET is_trashed = 0, local_path = excluded.local_path,
                                                 name = excluded.name",
            params![
                source,
                key,
                LOCAL_FOLDER_ID,
                name,
                format.mime(),
                size,
                path.to_string_lossy()
            ],
        )?;
        c.execute(
            "INSERT INTO books(book_key, format, file_name, content_md5, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(book_key) DO NOTHING",
            params![key, format.as_str(), name, sha256, bc_core::now_ms()],
        )?;
        Ok(c.query_row("SELECT id FROM books WHERE book_key = ?1", [&key], |r| r.get(0))?)
    }

    pub fn has_local_books(&self) -> Result<bool> {
        Ok(self.conn().query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE parent_drive_id = ?1 AND is_trashed = 0)",
            [LOCAL_FOLDER_ID],
            |r| r.get(0),
        )?)
    }

    /// Remove a local import from the library (the file is not touched).
    pub fn remove_local(&self, book_id: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE nodes SET is_trashed = 1
             WHERE file_key = (SELECT book_key FROM books WHERE id = ?1) AND local_path IS NOT NULL",
            [book_id],
        )?;
        Ok(())
    }

    fn breadcrumbs(&self, folder_id: &str) -> Result<Vec<Breadcrumb>> {
        let source = self.drive_source()?;
        let root = source.as_ref().and_then(|s| s.drive_root_id.clone());
        let root_name = source.map(|s| s.name).unwrap_or_else(|| "Library".into());
        let mut crumbs = Vec::new();
        if folder_id == LOCAL_FOLDER_ID {
            crumbs.push(Breadcrumb { id: LOCAL_FOLDER_ID.into(), name: "Imported".into() });
            return Ok(crumbs);
        }
        let c = self.conn();
        let mut cur = folder_id.to_string();
        let mut guard = 0;
        while Some(cur.as_str()) != root.as_deref() && guard < 64 {
            guard += 1;
            // A folder, or a shortcut pointing at it.
            let hit: Option<(String, Option<String>)> = c
                .query_row(
                    "SELECT name, parent_drive_id FROM nodes
                     WHERE is_folder = 1 AND (drive_id = ?1 OR shortcut_target_id = ?1)
                     ORDER BY shortcut_target_id IS NOT NULL LIMIT 1",
                    [&cur],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let Some((name, parent)) = hit else { break };
            crumbs.push(Breadcrumb { id: cur.clone(), name });
            match parent {
                Some(p) => cur = p,
                None => break,
            }
        }
        if let Some(r) = root {
            crumbs.push(Breadcrumb { id: r, name: root_name });
        }
        crumbs.reverse();
        Ok(crumbs)
    }

    /// Contents of one folder: sub-folders (shelves) then books.
    pub fn list_folder(&self, folder_id: Option<&str>) -> Result<FolderListing> {
        let root = self.drive_source()?.and_then(|s| s.drive_root_id);
        let id = match folder_id.or(root.as_deref()) {
            Some(id) => id.to_string(),
            None => LOCAL_FOLDER_ID.to_string(),
        };
        let mut folders = {
            let c = self.conn();
            let mut st = c.prepare_cached(
                "SELECT COALESCE(n.shortcut_target_id, n.drive_id) AS fid, n.name,
                        n.shortcut_target_id IS NOT NULL,
                        (SELECT COUNT(*) FROM nodes c WHERE c.parent_drive_id = COALESCE(n.shortcut_target_id, n.drive_id)
                            AND c.is_trashed = 0 AND c.file_key IS NOT NULL),
                        (SELECT COUNT(*) FROM nodes c WHERE c.parent_drive_id = COALESCE(n.shortcut_target_id, n.drive_id)
                            AND c.is_trashed = 0 AND c.is_folder = 1)
                 FROM nodes n
                 WHERE n.parent_drive_id = ?1 AND n.is_folder = 1 AND n.is_trashed = 0
                 ORDER BY n.name COLLATE NOCASE",
            )?;
            let rows = st.query_map([&id], |r| {
                Ok(FolderSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    is_shortcut: r.get(2)?,
                    book_count: r.get(3)?,
                    folder_count: r.get(4)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if Some(id.as_str()) == root.as_deref() && self.has_local_books()? {
            let count: i64 = self.conn().query_row(
                "SELECT COUNT(*) FROM nodes WHERE parent_drive_id = ?1 AND is_trashed = 0",
                [LOCAL_FOLDER_ID],
                |r| r.get(0),
            )?;
            folders.push(FolderSummary {
                id: LOCAL_FOLDER_ID.into(),
                name: "Imported".into(),
                book_count: count,
                folder_count: 0,
                is_shortcut: false,
            });
        }
        let books = self.books_in_folder(&id)?;
        Ok(FolderListing { breadcrumbs: self.breadcrumbs(&id)?, id, folders, books })
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::lib_with_tree;
    use crate::NodeRecord;

    #[test]
    fn mirrors_tree() {
        let (lib, _) = lib_with_tree();
        let root = lib.list_folder(None).unwrap();
        assert_eq!(root.id, "root");
        let names: Vec<_> = root.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["Classics", "Sci-Fi"]);
        assert_eq!(root.folders[1].book_count, 2);
        // Google Docs are ignored; only the top-level PDF is a book here.
        assert_eq!(root.books.len(), 1);
        assert_eq!(root.books[0].title, "Top Level");

        let sf = lib.list_folder(Some("sf")).unwrap();
        assert_eq!(sf.books.len(), 2);
        let crumbs: Vec<_> = sf.breadcrumbs.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(crumbs, vec!["Books", "Sci-Fi"]);

        // Extension fallback for octet-stream.
        let cl = lib.list_folder(Some("cl")).unwrap();
        assert_eq!(cl.books[0].format, bc_core::BookFormat::Epub);
    }

    #[test]
    fn shortcuts_share_a_book_and_resolve_folders() {
        let (lib, src) = lib_with_tree();
        lib.upsert_nodes(
            src,
            &[
                NodeRecord {
                    drive_id: "sc1".into(),
                    parent_drive_id: Some("cl".into()),
                    name: "Dune shortcut".into(),
                    mime_type: Some("application/epub+zip".into()),
                    shortcut_target_id: Some("dune".into()),
                    ..Default::default()
                },
                NodeRecord {
                    drive_id: "scf".into(),
                    parent_drive_id: Some("root".into()),
                    name: "Elsewhere".into(),
                    is_folder: true,
                    shortcut_target_id: Some("ext".into()),
                    ..Default::default()
                },
                NodeRecord {
                    drive_id: "extbook".into(),
                    parent_drive_id: Some("ext".into()),
                    name: "External.pdf".into(),
                    mime_type: Some("application/pdf".into()),
                    ..Default::default()
                },
            ],
        )
        .unwrap();
        let cl = lib.list_folder(Some("cl")).unwrap();
        assert_eq!(cl.books.len(), 2);
        let sf = lib.list_folder(Some("sf")).unwrap();
        let dune_id = sf.books.iter().find(|b| b.title == "Dune").unwrap().id;
        assert!(cl.books.iter().any(|b| b.id == dune_id), "shortcut shares the book row");

        let root = lib.list_folder(None).unwrap();
        let ext = root.folders.iter().find(|f| f.name == "Elsewhere").unwrap();
        assert_eq!(ext.id, "ext");
        assert!(ext.is_shortcut);
        let inside = lib.list_folder(Some("ext")).unwrap();
        assert_eq!(inside.books.len(), 1);
        assert_eq!(inside.breadcrumbs.last().unwrap().name, "Elsewhere");
        assert!(lib.is_folder_in_library("ext").unwrap());
    }

    #[test]
    fn removing_a_folder_hides_its_books() {
        let (lib, _) = lib_with_tree();
        assert_eq!(lib.query_books(&Default::default()).unwrap().len(), 4);
        lib.remove_subtree("sf").unwrap();
        assert_eq!(lib.query_books(&Default::default()).unwrap().len(), 2);
        assert!(!lib.is_folder_in_library("sf").unwrap());
        assert!(lib.is_folder_in_library("root").unwrap());
    }

    #[test]
    fn local_imports_get_a_shelf() {
        let (lib, _) = lib_with_tree();
        let id = lib
            .import_local(std::path::Path::new("/books/Local.epub"), "abc", bc_core::BookFormat::Epub, 10)
            .unwrap();
        let again = lib
            .import_local(std::path::Path::new("/books/Local.epub"), "abc", bc_core::BookFormat::Epub, 10)
            .unwrap();
        assert_eq!(id, again);
        let root = lib.list_folder(None).unwrap();
        assert!(root.folders.iter().any(|f| f.id == "local:" && f.book_count == 1));
        let local = lib.list_folder(Some("local:")).unwrap();
        assert_eq!(local.books.len(), 1);
        let src = lib.book_source(id).unwrap();
        assert_eq!(src.local_path.as_deref(), Some("/books/Local.epub"));
        lib.remove_local(id).unwrap();
        assert!(lib.list_folder(Some("local:")).unwrap().books.is_empty());
    }
}
