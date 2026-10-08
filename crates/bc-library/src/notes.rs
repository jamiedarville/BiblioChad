use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};

use bc_core::Locator;

use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    pub id: String,
    pub book_id: i64,
    pub locator: Locator,
    pub label: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub id: String,
    pub book_id: i64,
    pub kind: String,
    pub locator: Locator,
    pub text: Option<String>,
    pub color: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewAnnotation {
    pub locator: Locator,
    pub text: Option<String>,
    pub color: Option<String>,
    pub note: Option<String>,
}

fn bookmark_from_row(r: &Row) -> rusqlite::Result<Option<Bookmark>> {
    let loc: String = r.get("locator_json")?;
    Ok(Locator::from_json(&loc).ok().map(|locator| Bookmark {
        id: r.get("id").unwrap_or_default(),
        book_id: r.get("book_id").unwrap_or_default(),
        locator,
        label: r.get("label").unwrap_or_default(),
        created_at: r.get("created_at").unwrap_or_default(),
        updated_at: r.get("updated_at").unwrap_or_default(),
    }))
}

fn annotation_from_row(r: &Row) -> rusqlite::Result<Option<Annotation>> {
    let loc: String = r.get("locator_json")?;
    Ok(Locator::from_json(&loc).ok().map(|locator| Annotation {
        id: r.get("id").unwrap_or_default(),
        book_id: r.get("book_id").unwrap_or_default(),
        kind: r.get("kind").unwrap_or_default(),
        locator,
        text: r.get("text").unwrap_or_default(),
        color: r.get("color").unwrap_or_default(),
        note: r.get("note").unwrap_or_default(),
        created_at: r.get("created_at").unwrap_or_default(),
        updated_at: r.get("updated_at").unwrap_or_default(),
    }))
}

impl Library {
    pub fn add_bookmark(
        &self,
        book_id: i64,
        locator: &Locator,
        label: Option<&str>,
    ) -> Result<Bookmark> {
        let now = bc_core::now_ms();
        let id = bc_core::new_id();
        self.conn().execute(
            "INSERT INTO bookmarks(id, book_id, locator_json, label, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, book_id, locator.to_json(), label, now],
        )?;
        Ok(Bookmark {
            id,
            book_id,
            locator: locator.clone(),
            label: label.map(str::to_string),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn bookmarks(&self, book_id: i64) -> Result<Vec<Bookmark>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT * FROM bookmarks WHERE book_id = ?1 AND deleted_at IS NULL ORDER BY created_at",
        )?;
        let rows = st.query_map([book_id], bookmark_from_row)?;
        Ok(rows.filter_map(|r| r.ok().flatten()).collect())
    }

    /// Tombstone a bookmark so the deletion syncs.
    pub fn delete_bookmark(&self, id: &str) -> Result<()> {
        let now = bc_core::now_ms();
        self.conn().execute(
            "UPDATE bookmarks SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn add_annotation(&self, book_id: i64, a: &NewAnnotation) -> Result<Annotation> {
        let now = bc_core::now_ms();
        let id = bc_core::new_id();
        let kind = if a.note.as_deref().is_some_and(|n| !n.trim().is_empty()) {
            "note"
        } else {
            "highlight"
        };
        self.conn().execute(
            "INSERT INTO annotations(id, book_id, kind, locator_json, text, color, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![id, book_id, kind, a.locator.to_json(), a.text, a.color, a.note, now],
        )?;
        Ok(Annotation {
            id,
            book_id,
            kind: kind.into(),
            locator: a.locator.clone(),
            text: a.text.clone(),
            color: a.color.clone(),
            note: a.note.clone(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_annotation(
        &self,
        id: &str,
        color: Option<&str>,
        note: Option<&str>,
    ) -> Result<()> {
        let kind = if note.is_some_and(|n| !n.trim().is_empty()) {
            "note"
        } else {
            "highlight"
        };
        let n = self.conn().execute(
            "UPDATE annotations SET color = COALESCE(?2, color), note = ?3, kind = ?4, updated_at = ?5
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, color, note, kind, bc_core::now_ms()],
        )?;
        if n == 0 {
            return Err(LibraryError::NotFound(format!("annotation {id}")));
        }
        Ok(())
    }

    pub fn delete_annotation(&self, id: &str) -> Result<()> {
        let now = bc_core::now_ms();
        self.conn().execute(
            "UPDATE annotations SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn annotations(&self, book_id: i64) -> Result<Vec<Annotation>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT * FROM annotations WHERE book_id = ?1 AND deleted_at IS NULL ORDER BY created_at",
        )?;
        let rows = st.query_map([book_id], annotation_from_row)?;
        let mut v: Vec<Annotation> = rows.filter_map(|r| r.ok().flatten()).collect();
        v.sort_by(|a, b| position_key(&a.locator).total_cmp(&position_key(&b.locator)));
        Ok(v)
    }

    /// Export a book's highlights and notes as Markdown.
    pub fn export_annotations_markdown(&self, book_id: i64) -> Result<String> {
        let s = self.book_summary(book_id)?;
        let notes = self.annotations(book_id)?;
        let marks = self.bookmarks(book_id)?;
        let mut out = format!("# {}\n\n", s.title);
        if let Some(a) = &s.author {
            out.push_str(&format!("*{a}*\n\n"));
        }
        if notes.is_empty() && marks.is_empty() {
            out.push_str("_No highlights yet. Chad is disappointed but not surprised._\n");
            return Ok(out);
        }
        if !notes.is_empty() {
            out.push_str("## Highlights and notes\n\n");
            for n in &notes {
                let where_ = n.locator.label(position_key(&n.locator));
                if let Some(t) = n.text.as_deref().filter(|t| !t.trim().is_empty()) {
                    for line in t.trim().lines() {
                        out.push_str(&format!("> {line}\n"));
                    }
                    out.push_str(&format!(">\n> — {where_}\n\n"));
                }
                if let Some(note) = n.note.as_deref().filter(|t| !t.trim().is_empty()) {
                    out.push_str(&format!("**Note:** {}\n\n", note.trim()));
                }
            }
        }
        if !marks.is_empty() {
            out.push_str("## Bookmarks\n\n");
            for m in &marks {
                let label = m.label.clone().unwrap_or_else(|| "Bookmark".into());
                out.push_str(&format!(
                    "- {label} ({})\n",
                    m.locator.label(position_key(&m.locator))
                ));
            }
        }
        Ok(out)
    }
}

fn position_key(l: &Locator) -> f64 {
    match l {
        Locator::Epub { percent, .. } => *percent,
        Locator::Pdf { page, offset, .. } => *page as f64 + offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::lib_with_tree;

    #[test]
    fn notes_and_export() {
        let (lib, _) = lib_with_tree();
        let id = lib.book_id_by_key("drive:dune").unwrap().unwrap();
        let loc = |p: f64| Locator::Epub {
            cfi: format!("epubcfi(/6/{})", (p * 100.0) as i32),
            href: None,
            percent: p,
        };
        let a = lib
            .add_annotation(
                id,
                &NewAnnotation {
                    locator: loc(0.5),
                    text: Some("Fear is the mind-killer.".into()),
                    color: Some("yellow".into()),
                    note: None,
                },
            )
            .unwrap();
        lib.add_annotation(
            id,
            &NewAnnotation {
                locator: loc(0.1),
                text: Some("A beginning".into()),
                color: None,
                note: Some("delicate".into()),
            },
        )
        .unwrap();
        let list = lib.annotations(id).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0].text.as_deref(),
            Some("A beginning"),
            "sorted by position"
        );
        assert_eq!(list[0].kind, "note");

        lib.update_annotation(&a.id, Some("green"), Some("Litany"))
            .unwrap();
        let b = lib.add_bookmark(id, &loc(0.3), Some("Arrakis")).unwrap();
        let md = lib.export_annotations_markdown(id).unwrap();
        assert!(md.starts_with("# Dune"));
        assert!(md.contains("> Fear is the mind-killer."));
        assert!(md.contains("**Note:** Litany"));
        assert!(md.contains("- Arrakis (30%)"));

        lib.delete_bookmark(&b.id).unwrap();
        lib.delete_annotation(&a.id).unwrap();
        assert!(lib.bookmarks(id).unwrap().is_empty());
        assert_eq!(lib.annotations(id).unwrap().len(), 1);
        assert!(lib.update_annotation(&a.id, None, None).is_err());
    }
}
