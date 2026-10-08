use rusqlite::params;

use bc_core::Locator;
use bc_sync::{AnnotationEntry, BookEntry, BookmarkEntry, SyncDoc};

use crate::{Library, Result};

impl Library {
    /// Snapshot local state as a sync document: the newest position per
    /// book plus every bookmark and annotation (tombstones included).
    pub fn export_sync_doc(&self) -> Result<SyncDoc> {
        let c = self.conn();
        let mut doc = SyncDoc::default();
        {
            let mut st = c.prepare(
                "SELECT b.book_key, p.locator_json, p.percent, p.status, p.updated_at, p.device_id
                 FROM progress p JOIN books b ON b.id = p.book_id
                 WHERE p.updated_at = (SELECT MAX(updated_at) FROM progress p2 WHERE p2.book_id = p.book_id)",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?;
            for row in rows {
                let (key, loc, percent, status, updated_at, device_id) = row?;
                let Ok(locator) = Locator::from_json(&loc) else { continue };
                let entry = BookEntry {
                    locator,
                    percent,
                    status: bc_core::ReadStatus::parse(&status).unwrap_or_default(),
                    updated_at,
                    device_id,
                };
                // Deterministic choice when two devices share a timestamp.
                let replace = doc
                    .books
                    .get(&key)
                    .is_none_or(|e| (e.updated_at, &e.device_id) < (entry.updated_at, &entry.device_id));
                if replace {
                    doc.books.insert(key, entry);
                }
            }
        }
        {
            let mut st = c.prepare(
                "SELECT m.id, b.book_key, m.locator_json, m.label, m.created_at, m.updated_at, m.deleted_at
                 FROM bookmarks m JOIN books b ON b.id = m.book_id",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            })?;
            for row in rows {
                let (id, book_key, loc, label, created_at, updated_at, deleted_at) = row?;
                let Ok(locator) = Locator::from_json(&loc) else { continue };
                doc.bookmarks.insert(
                    id,
                    BookmarkEntry { book_key, locator, label, created_at, updated_at, deleted_at },
                );
            }
        }
        {
            let mut st = c.prepare(
                "SELECT a.id, b.book_key, a.kind, a.locator_json, a.text, a.color, a.note,
                        a.created_at, a.updated_at, a.deleted_at
                 FROM annotations a JOIN books b ON b.id = a.book_id",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                ))
            })?;
            for row in rows {
                let (id, book_key, kind, loc, text, color, note, created_at, updated_at, deleted_at) = row?;
                let Ok(locator) = Locator::from_json(&loc) else { continue };
                doc.annotations.insert(
                    id,
                    AnnotationEntry {
                        book_key,
                        kind,
                        locator,
                        text,
                        color,
                        note,
                        created_at,
                        updated_at,
                        deleted_at,
                    },
                );
            }
        }
        Ok(doc)
    }

    /// Apply a merged sync document locally. Records for books this library
    /// does not know are skipped here (they stay in the document).
    pub fn import_sync_doc(&self, doc: &SyncDoc) -> Result<usize> {
        let mut applied = 0;
        for (key, entry) in &doc.books {
            if let Some(id) = self.book_id_by_key(key)? {
                self.upsert_remote_progress(id, entry)?;
                applied += 1;
            }
        }
        let c = self.conn();
        let book_id = |key: &str| -> rusqlite::Result<Option<i64>> {
            use rusqlite::OptionalExtension;
            c.query_row("SELECT id FROM books WHERE book_key = ?1", [key], |r| r.get(0)).optional()
        };
        for (id, m) in &doc.bookmarks {
            let Some(bid) = book_id(&m.book_key)? else { continue };
            applied += c.execute(
                "INSERT INTO bookmarks(id, book_id, locator_json, label, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET locator_json = excluded.locator_json,
                   label = excluded.label, updated_at = excluded.updated_at, deleted_at = excluded.deleted_at
                 WHERE MAX(excluded.updated_at, COALESCE(excluded.deleted_at, 0))
                     > MAX(bookmarks.updated_at, COALESCE(bookmarks.deleted_at, 0))",
                params![id, bid, m.locator.to_json(), m.label, m.created_at, m.updated_at, m.deleted_at],
            )?;
        }
        for (id, a) in &doc.annotations {
            let Some(bid) = book_id(&a.book_key)? else { continue };
            applied += c.execute(
                "INSERT INTO annotations(id, book_id, kind, locator_json, text, color, note, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, locator_json = excluded.locator_json,
                   text = excluded.text, color = excluded.color, note = excluded.note,
                   updated_at = excluded.updated_at, deleted_at = excluded.deleted_at
                 WHERE MAX(excluded.updated_at, COALESCE(excluded.deleted_at, 0))
                     > MAX(annotations.updated_at, COALESCE(annotations.deleted_at, 0))",
                params![
                    id,
                    bid,
                    a.kind,
                    a.locator.to_json(),
                    a.text,
                    a.color,
                    a.note,
                    a.created_at,
                    a.updated_at,
                    a.deleted_at
                ],
            )?;
        }
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::lib_with_tree;
    use crate::NewAnnotation;
    use bc_core::{FitMode, Locator};

    #[test]
    fn two_devices_converge() {
        let (laptop, _) = lib_with_tree();
        let (tablet, _) = lib_with_tree();
        let lid = laptop.book_id_by_key("drive:found").unwrap().unwrap();
        let tid = tablet.book_id_by_key("drive:found").unwrap().unwrap();
        let page = |p| Locator::Pdf { page: p, offset: 0.0, fit: FitMode::Width, zoom: 1.0 };

        laptop.save_progress(lid, "laptop", &page(10), 0.02, None).unwrap();
        let a = laptop
            .add_annotation(lid, &NewAnnotation { locator: page(3), text: Some("psychohistory".into()), color: None, note: None })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        tablet.save_progress(tid, "tablet", &page(211), 0.42, None).unwrap();

        // Laptop syncs first (remote empty), then tablet, then laptop again.
        let remote = bc_sync::merge(&Default::default(), &laptop.export_sync_doc().unwrap());
        let remote = bc_sync::merge(&remote, &tablet.export_sync_doc().unwrap());
        tablet.import_sync_doc(&remote).unwrap();
        laptop.import_sync_doc(&remote).unwrap();

        assert_eq!(tablet.annotations(tid).unwrap().len(), 1);
        let (_, prompt) = laptop.resume_point(lid, "laptop").unwrap();
        assert_eq!(prompt.unwrap().label, "page 212");

        // Deleting on the tablet propagates as a tombstone.
        std::thread::sleep(std::time::Duration::from_millis(3));
        tablet.delete_annotation(&a.id).unwrap();
        let remote = bc_sync::merge(&remote, &tablet.export_sync_doc().unwrap());
        laptop.import_sync_doc(&remote).unwrap();
        assert!(laptop.annotations(lid).unwrap().is_empty());
        // An old copy cannot resurrect it.
        let stale = bc_sync::merge(&remote, &laptop.export_sync_doc().unwrap());
        assert!(stale.annotations[&a.id].deleted_at.is_some());
    }
}
