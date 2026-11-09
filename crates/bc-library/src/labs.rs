//! "Build a lab" guides: one saved lab series per book, stored locally
//! with the book (like zone maps, they are not part of the Drive sync
//! file). The guide itself is kept as the JSON the tutor produced
//! (`bc_tutor::LabGuide`); `fingerprint` identifies the book file the
//! guide was built from so the UI can flag it as stale.

use rusqlite::{params, Row};
use serde::Serialize;

use crate::{Library, Result};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LabGuideRecord {
    pub id: String,
    pub book_id: i64,
    /// The guide as the tutor returned it (see `bc_tutor::LabGuide`).
    pub guide: serde_json::Value,
    pub model: String,
    /// Book-file identity at generation time (`book_key` + content md5).
    pub fingerprint: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn record_from_row(r: &Row) -> rusqlite::Result<Option<LabGuideRecord>> {
    let guide: String = r.get("guide_json")?;
    let Ok(guide) = serde_json::from_str(&guide) else {
        return Ok(None);
    };
    Ok(Some(LabGuideRecord {
        id: r.get("id")?,
        book_id: r.get("book_id")?,
        guide,
        model: r.get("model")?,
        fingerprint: r.get("fingerprint")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    }))
}

/// Identity of the exact book file a guide was built from.
pub fn lab_fingerprint(book_key: &str, md5: Option<&str>) -> String {
    format!("{book_key}:{}", md5.unwrap_or(""))
}

impl Library {
    /// The saved lab guide for a book, if one has been built.
    pub fn lab_guide(&self, book_id: i64) -> Result<Option<LabGuideRecord>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT * FROM lab_guides WHERE book_id = ?1")?;
        let rows = st.query_map([book_id], record_from_row)?;
        let found: Vec<LabGuideRecord> = rows.filter_map(|r| r.ok().flatten()).collect();
        Ok(found.into_iter().next())
    }

    /// Save (or replace) the lab guide for a book. One guide per book:
    /// rebuilding replaces the previous artefact.
    pub fn save_lab_guide(
        &self,
        book_id: i64,
        guide: &serde_json::Value,
        model: &str,
        fingerprint: &str,
    ) -> Result<LabGuideRecord> {
        // Fail loudly for a book that doesn't exist (FK would too, but
        // this gives the caller a NotFound instead of a raw constraint).
        self.book_summary(book_id)?;
        let now = bc_core::now_ms();
        let existing = self.lab_guide(book_id)?;
        let (id, created_at) = match &existing {
            Some(e) => (e.id.clone(), e.created_at),
            None => (bc_core::new_id(), now),
        };
        self.conn().execute(
            "INSERT INTO lab_guides(id, book_id, guide_json, model, fingerprint, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(book_id) DO UPDATE SET guide_json = excluded.guide_json,
               model = excluded.model, fingerprint = excluded.fingerprint,
               updated_at = excluded.updated_at",
            params![id, book_id, guide.to_string(), model, fingerprint, created_at, now],
        )?;
        Ok(LabGuideRecord {
            id,
            book_id,
            guide: guide.clone(),
            model: model.to_string(),
            fingerprint: fingerprint.to_string(),
            created_at,
            updated_at: now,
        })
    }

    pub fn delete_lab_guide(&self, book_id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM lab_guides WHERE book_id = ?1", [book_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::lib_with_tree;
    use crate::BookQuery;

    #[test]
    fn save_replace_read_delete() {
        let (lib, _) = lib_with_tree();
        let book = lib.query_books(&BookQuery::default()).unwrap()[0].id;
        assert!(lib.lab_guide(book).unwrap().is_none());

        let g1 = serde_json::json!({ "labs": [{ "title": "One" }] });
        let a = lib.save_lab_guide(book, &g1, "m1", "fp1").unwrap();
        assert_eq!(lib.lab_guide(book).unwrap().unwrap().guide, g1);

        let g2 = serde_json::json!({ "labs": [{ "title": "Two" }] });
        let b = lib.save_lab_guide(book, &g2, "m2", "fp2").unwrap();
        assert_eq!(b.id, a.id, "rebuilding keeps the artefact id");
        assert_eq!(b.created_at, a.created_at);
        assert_eq!(lib.lab_guide(book).unwrap().unwrap().model, "m2");

        assert_eq!(lab_fingerprint("drive:x", Some("abc")), "drive:x:abc");
        assert_eq!(lab_fingerprint("local:y", None), "local:y:");

        lib.delete_lab_guide(book).unwrap();
        assert!(lib.lab_guide(book).unwrap().is_none());
        assert!(lib.save_lab_guide(999_999, &g1, "m", "fp").is_err());
    }
}
