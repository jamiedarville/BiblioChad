use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use bc_core::{BookFormat, ReadStatus};
use bc_reader::BookMetadata;

use crate::nodes::title_from_file_name;
use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct BookSummary {
    pub id: i64,
    pub key: String,
    pub format: BookFormat,
    pub title: String,
    pub author: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub has_cover: bool,
    pub percent: f64,
    pub status: ReadStatus,
    pub last_read_at: Option<i64>,
    pub added_at: i64,
    pub size: Option<i64>,
    pub favorite: bool,
    pub pinned: bool,
    pub cached: bool,
    pub page_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BookDetails {
    #[serde(flatten)]
    pub summary: BookSummary,
    pub file_name: String,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub description: Option<String>,
    pub title_override: Option<String>,
    pub author_override: Option<String>,
    pub series_override: Option<String>,
    pub collections: Vec<i64>,
    pub folder_path: Vec<String>,
}

/// Where to get the bytes for a book.
#[derive(Debug, Clone, Serialize)]
pub struct BookSource {
    pub id: i64,
    pub key: String,
    pub format: BookFormat,
    pub file_name: String,
    pub drive_file_id: Option<String>,
    pub local_path: Option<String>,
    pub md5: Option<String>,
    pub size: Option<i64>,
    pub metadata_extracted: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Title,
    Author,
    RecentlyRead,
    Added,
    Progress,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BookQuery {
    pub search: Option<String>,
    pub sort: SortKey,
    pub descending: bool,
    pub format: Option<BookFormat>,
    pub status: Option<ReadStatus>,
    pub collection_id: Option<i64>,
    pub favorites_only: bool,
    pub cached_only: bool,
    /// Restrict to books anywhere below this folder.
    pub folder_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Columns shared by every summary query. Expects `b` = books and `p` = the
/// latest progress row for the book.
const SUMMARY_COLUMNS: &str = "
    b.id, b.book_key, b.format, b.file_name,
    COALESCE(b.title_override, b.title) AS title,
    COALESCE(b.author_override, b.author) AS author,
    COALESCE(b.series_override, b.series) AS series, b.series_index,
    b.cover_path IS NOT NULL AS has_cover,
    COALESCE(p.percent, 0) AS percent,
    COALESCE(b.status_override, p.status, 'unread') AS status,
    p.updated_at AS last_read_at, b.added_at,
    (SELECT size FROM nodes n2 WHERE n2.file_key = b.book_key ORDER BY n2.shortcut_target_id IS NOT NULL LIMIT 1) AS size,
    b.favorite, b.pinned,
    EXISTS(SELECT 1 FROM cache_entries ce WHERE ce.book_key = b.book_key) AS cached,
    b.page_count";

const LATEST_PROGRESS: &str = "
    LEFT JOIN (
      SELECT book_id, percent, status, updated_at,
             ROW_NUMBER() OVER (PARTITION BY book_id ORDER BY updated_at DESC) AS rn
      FROM progress
    ) p ON p.book_id = b.id AND p.rn = 1";

pub(crate) fn summary_from_row(r: &Row) -> rusqlite::Result<BookSummary> {
    let format: String = r.get("format")?;
    let status: String = r.get("status")?;
    let title: Option<String> = r.get("title")?;
    let file_name: String = r.get("file_name")?;
    Ok(BookSummary {
        id: r.get("id")?,
        key: r.get("book_key")?,
        format: BookFormat::parse(&format).unwrap_or(BookFormat::Epub),
        title: title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| title_from_file_name(&file_name)),
        author: r.get("author")?,
        series: r.get("series")?,
        series_index: r.get("series_index")?,
        has_cover: r.get("has_cover")?,
        percent: r.get("percent")?,
        status: ReadStatus::parse(&status).unwrap_or_default(),
        last_read_at: r.get("last_read_at")?,
        added_at: r.get("added_at")?,
        size: r.get("size")?,
        favorite: r.get("favorite")?,
        pinned: r.get("pinned")?,
        cached: r.get("cached")?,
        page_count: r.get("page_count")?,
    })
}

/// SQL expression for the effective, sortable title.
const TITLE_EXPR: &str = "COALESCE(b.title_override, b.title, b.file_name)";

impl Library {
    pub(crate) fn books_in_folder(&self, folder_id: &str) -> Result<Vec<BookSummary>> {
        let c = self.conn();
        let sql = format!(
            "SELECT DISTINCT {SUMMARY_COLUMNS} FROM nodes n
             JOIN books b ON b.book_key = n.file_key {LATEST_PROGRESS}
             WHERE n.parent_drive_id = ?1 AND n.is_trashed = 0
             ORDER BY {TITLE_EXPR} COLLATE NOCASE"
        );
        let mut st = c.prepare_cached(&sql)?;
        let rows = st.query_map([folder_id], summary_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Flat search/sort/filter over all live books.
    pub fn query_books(&self, q: &BookQuery) -> Result<Vec<BookSummary>> {
        let mut args: Vec<Value> = Vec::new();
        let mut with = String::new();
        let mut wheres = vec![
            "EXISTS(SELECT 1 FROM nodes n WHERE n.file_key = b.book_key AND n.is_trashed = 0)"
                .to_string(),
        ];
        if let Some(folder) = &q.folder_id {
            args.push(Value::Text(folder.clone()));
            with = format!(
                "WITH RECURSIVE tree(id) AS (
                   SELECT ?{}
                   UNION
                   SELECT COALESCE(n.shortcut_target_id, n.drive_id) FROM nodes n JOIN tree t
                     ON n.parent_drive_id = t.id WHERE n.is_folder = 1 AND n.is_trashed = 0
                 ) ",
                args.len()
            );
            wheres.push(
                "EXISTS(SELECT 1 FROM nodes n WHERE n.file_key = b.book_key AND n.is_trashed = 0
                        AND n.parent_drive_id IN (SELECT id FROM tree))"
                    .into(),
            );
        }
        if let Some(s) = q
            .search
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            for term in s.split_whitespace().take(8) {
                let escaped = term
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                args.push(Value::Text(format!("%{escaped}%")));
                let i = args.len();
                wheres.push(format!(
                    "({TITLE_EXPR} LIKE ?{i} ESCAPE '\\' OR COALESCE(b.author_override, b.author, '') LIKE ?{i} ESCAPE '\\'
                      OR COALESCE(b.series_override, b.series, '') LIKE ?{i} ESCAPE '\\' OR b.file_name LIKE ?{i} ESCAPE '\\'
                      OR COALESCE(b.isbn, '') LIKE ?{i} ESCAPE '\\')"
                ));
            }
        }
        if let Some(f) = q.format {
            args.push(Value::Text(f.as_str().into()));
            wheres.push(format!("b.format = ?{}", args.len()));
        }
        if let Some(s) = q.status {
            args.push(Value::Text(s.as_str().into()));
            wheres.push(format!(
                "COALESCE(b.status_override, p.status, 'unread') = ?{}",
                args.len()
            ));
        }
        if let Some(cid) = q.collection_id {
            args.push(Value::Integer(cid));
            wheres.push(format!(
                "b.id IN (SELECT book_id FROM collection_books WHERE collection_id = ?{})",
                args.len()
            ));
        }
        if q.favorites_only {
            wheres.push("b.favorite = 1".into());
        }
        if q.cached_only {
            wheres.push(
                "EXISTS(SELECT 1 FROM cache_entries ce WHERE ce.book_key = b.book_key)".into(),
            );
        }
        let dir = if q.descending { "DESC" } else { "ASC" };
        let order = match q.sort {
            SortKey::Title => format!("{TITLE_EXPR} COLLATE NOCASE {dir}"),
            SortKey::Author => format!(
                "COALESCE(b.author_override, b.author) IS NULL, COALESCE(b.author_override, b.author) COLLATE NOCASE {dir}, {TITLE_EXPR} COLLATE NOCASE"
            ),
            // "Recently read" defaults to newest first; descending flips it.
            SortKey::RecentlyRead => format!(
                "p.updated_at IS NULL, p.updated_at {}",
                if q.descending { "ASC" } else { "DESC" }
            ),
            SortKey::Added => format!("b.added_at {}", if q.descending { "ASC" } else { "DESC" }),
            SortKey::Progress => format!("COALESCE(p.percent, 0) {}", if q.descending { "ASC" } else { "DESC" }),
        };
        let limit = q.limit.unwrap_or(100_000);
        let offset = q.offset.unwrap_or(0);
        let sql = format!(
            "{with}SELECT {SUMMARY_COLUMNS} FROM books b {LATEST_PROGRESS}
             WHERE {} ORDER BY {order}, b.id LIMIT {limit} OFFSET {offset}",
            wheres.join(" AND ")
        );
        let c = self.conn();
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args.iter()), summary_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Books in progress, most recently read first.
    pub fn continue_reading(&self, limit: u32) -> Result<Vec<BookSummary>> {
        self.query_books(&BookQuery {
            status: Some(ReadStatus::Reading),
            sort: SortKey::RecentlyRead,
            limit: Some(limit),
            ..Default::default()
        })
    }

    pub fn recently_read(&self, limit: u32) -> Result<Vec<BookSummary>> {
        let mut books = self.query_books(&BookQuery {
            sort: SortKey::RecentlyRead,
            limit: Some(limit),
            ..Default::default()
        })?;
        books.retain(|b| b.last_read_at.is_some());
        Ok(books)
    }

    pub fn book_summary(&self, id: i64) -> Result<BookSummary> {
        let c = self.conn();
        let sql =
            format!("SELECT {SUMMARY_COLUMNS} FROM books b {LATEST_PROGRESS} WHERE b.id = ?1");
        c.query_row(&sql, [id], summary_from_row)
            .optional()?
            .ok_or_else(|| LibraryError::NotFound(format!("book {id}")))
    }

    pub fn book_id_by_key(&self, key: &str) -> Result<Option<i64>> {
        Ok(self
            .conn()
            .query_row("SELECT id FROM books WHERE book_key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn book_details(&self, id: i64) -> Result<BookDetails> {
        let summary = self.book_summary(id)?;
        let c = self.conn();
        let (file_name, publisher, language, isbn, description, to, ao, so) = c.query_row(
            "SELECT file_name, publisher, language, isbn, description,
                    title_override, author_override, series_override FROM books WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )?;
        let mut st = c.prepare("SELECT collection_id FROM collection_books WHERE book_id = ?1")?;
        let collections = st
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        // Folder path of the first live node holding this book.
        let mut folder_path = Vec::new();
        let mut parent: Option<String> = c
            .query_row(
                "SELECT parent_drive_id FROM nodes WHERE file_key = ?1 AND is_trashed = 0 LIMIT 1",
                [&summary.key],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let mut guard = 0;
        while let Some(p) = parent.take() {
            guard += 1;
            if guard > 64 {
                break;
            }
            let hit: Option<(String, Option<String>)> = c
                .query_row(
                    "SELECT name, parent_drive_id FROM nodes WHERE is_folder = 1
                     AND (drive_id = ?1 OR shortcut_target_id = ?1) LIMIT 1",
                    [&p],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((name, next)) = hit {
                folder_path.push(name);
                parent = next;
            }
        }
        folder_path.reverse();
        Ok(BookDetails {
            summary,
            file_name,
            publisher,
            language,
            isbn,
            description,
            title_override: to,
            author_override: ao,
            series_override: so,
            collections,
            folder_path,
        })
    }

    pub fn book_source(&self, id: i64) -> Result<BookSource> {
        let c = self.conn();
        let (key, format, file_name, extracted): (String, String, String, Option<i64>) = c
            .query_row(
                "SELECT book_key, format, file_name, metadata_extracted_at FROM books WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| LibraryError::NotFound(format!("book {id}")))?;
        let node: Option<(Option<String>, Option<String>, Option<i64>)> = c
            .query_row(
                "SELECT local_path, md5, size FROM nodes WHERE file_key = ?1
                 ORDER BY is_trashed, shortcut_target_id IS NOT NULL LIMIT 1",
                [&key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (local_path, md5, size) = node.unwrap_or((None, None, None));
        Ok(BookSource {
            id,
            drive_file_id: key.strip_prefix("drive:").map(str::to_string),
            key,
            format: BookFormat::parse(&format).unwrap_or(BookFormat::Epub),
            file_name,
            local_path,
            md5,
            size,
            metadata_extracted: extracted.is_some(),
        })
    }

    /// Store metadata extracted from the book file.
    pub fn apply_metadata(
        &self,
        id: i64,
        md: &BookMetadata,
        cover_path: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE books SET title = ?2, author = ?3, series = ?4, series_index = ?5,
               publisher = ?6, language = ?7, isbn = ?8, description = ?9,
               page_count = COALESCE(?10, page_count),
               cover_path = COALESCE(?11, cover_path), metadata_extracted_at = ?12
             WHERE id = ?1",
            params![
                id,
                md.title,
                md.author_line(),
                md.series,
                md.series_index,
                md.publisher,
                md.language,
                md.isbn,
                md.description,
                md.page_count,
                cover_path,
                bc_core::now_ms()
            ],
        )?;
        Ok(())
    }

    pub fn set_cover_path(&self, id: i64, cover_path: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE books SET cover_path = ?2 WHERE id = ?1",
            params![id, cover_path],
        )?;
        Ok(())
    }

    pub fn cover_path(&self, id: i64) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT cover_path FROM books WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten())
    }

    pub fn set_page_count(&self, id: i64, pages: u32) -> Result<()> {
        self.conn().execute(
            "UPDATE books SET page_count = ?2 WHERE id = ?1",
            params![id, pages],
        )?;
        Ok(())
    }

    /// Local-only metadata edits. Empty strings clear an override.
    pub fn set_overrides(
        &self,
        id: i64,
        title: Option<&str>,
        author: Option<&str>,
        series: Option<&str>,
    ) -> Result<()> {
        let norm = |s: Option<&str>| {
            s.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        self.conn().execute(
            "UPDATE books SET title_override = ?2, author_override = ?3, series_override = ?4 WHERE id = ?1",
            params![id, norm(title), norm(author), norm(series)],
        )?;
        Ok(())
    }

    pub fn set_favorite(&self, id: i64, favorite: bool) -> Result<()> {
        self.conn().execute(
            "UPDATE books SET favorite = ?2 WHERE id = ?1",
            params![id, favorite],
        )?;
        Ok(())
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        self.conn().execute(
            "UPDATE books SET pinned = ?2 WHERE id = ?1",
            params![id, pinned],
        )?;
        Ok(())
    }

    /// Manually mark a book. `None` returns to automatic status.
    pub fn set_status_override(&self, id: i64, status: Option<ReadStatus>) -> Result<()> {
        self.conn().execute(
            "UPDATE books SET status_override = ?2 WHERE id = ?1",
            params![id, status.map(|s| s.as_str())],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::lib_with_tree;

    #[test]
    fn search_sort_filter() {
        let (lib, _) = lib_with_tree();
        let all = lib.query_books(&BookQuery::default()).unwrap();
        let titles: Vec<_> = all.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, vec!["Dune", "Foundation", "Moby Dick", "Top Level"]);

        let q = BookQuery {
            search: Some("moby".into()),
            ..Default::default()
        };
        assert_eq!(lib.query_books(&q).unwrap().len(), 1);

        let q = BookQuery {
            search: Some("100%_".into()),
            ..Default::default()
        };
        assert!(
            lib.query_books(&q).unwrap().is_empty(),
            "wildcards are escaped"
        );

        let q = BookQuery {
            format: Some(BookFormat::Pdf),
            ..Default::default()
        };
        assert_eq!(lib.query_books(&q).unwrap().len(), 2);

        let q = BookQuery {
            folder_id: Some("sf".into()),
            ..Default::default()
        };
        assert_eq!(lib.query_books(&q).unwrap().len(), 2);

        let q = BookQuery {
            sort: SortKey::Title,
            descending: true,
            ..Default::default()
        };
        assert_eq!(lib.query_books(&q).unwrap()[0].title, "Top Level");

        let q = BookQuery {
            limit: Some(2),
            offset: Some(1),
            ..Default::default()
        };
        let page = lib.query_books(&q).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].title, "Foundation");
    }

    #[test]
    fn metadata_and_overrides() {
        let (lib, _) = lib_with_tree();
        let dune = lib
            .query_books(&BookQuery {
                search: Some("dune".into()),
                ..Default::default()
            })
            .unwrap()[0]
            .id;
        let md = BookMetadata {
            title: Some("Dune (40th Anniversary)".into()),
            authors: vec!["Frank Herbert".into()],
            series: Some("Dune".into()),
            series_index: Some(1.0),
            ..Default::default()
        };
        lib.apply_metadata(dune, &md, Some("/covers/1.jpg"))
            .unwrap();
        let s = lib.book_summary(dune).unwrap();
        assert_eq!(s.title, "Dune (40th Anniversary)");
        assert_eq!(s.author.as_deref(), Some("Frank Herbert"));
        assert!(s.has_cover);

        lib.set_overrides(dune, Some("Dune"), None, Some(""))
            .unwrap();
        let d = lib.book_details(dune).unwrap();
        assert_eq!(d.summary.title, "Dune");
        assert_eq!(d.summary.series.as_deref(), Some("Dune"));
        assert_eq!(d.folder_path, vec!["Sci-Fi"]);

        let q = BookQuery {
            sort: SortKey::Author,
            ..Default::default()
        };
        assert_eq!(
            lib.query_books(&q).unwrap()[0].id,
            dune,
            "books with authors sort first"
        );

        lib.set_favorite(dune, true).unwrap();
        let q = BookQuery {
            favorites_only: true,
            ..Default::default()
        };
        assert_eq!(lib.query_books(&q).unwrap().len(), 1);

        let src = lib.book_source(dune).unwrap();
        assert_eq!(src.drive_file_id.as_deref(), Some("dune"));
        assert_eq!(src.md5.as_deref(), Some("md5-dune"));
        assert!(src.metadata_extracted);
    }

    #[test]
    fn content_change_resets_metadata() {
        let (lib, src) = lib_with_tree();
        let id = lib.book_id_by_key("drive:dune").unwrap().unwrap();
        lib.apply_metadata(id, &BookMetadata::default(), None)
            .unwrap();
        lib.upsert_nodes(
            src,
            &[crate::NodeRecord {
                drive_id: "dune".into(),
                parent_drive_id: Some("sf".into()),
                name: "Dune.epub".into(),
                mime_type: Some("application/epub+zip".into()),
                md5: Some("new-md5".into()),
                ..Default::default()
            }],
        )
        .unwrap();
        assert!(!lib.book_source(id).unwrap().metadata_extracted);
    }
}
