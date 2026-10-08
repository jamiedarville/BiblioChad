use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::{Library, Result};

#[derive(Debug, Clone, Serialize)]
pub struct CacheEntry {
    pub book_key: String,
    pub md5: Option<String>,
    pub path: String,
    pub bytes: i64,
    pub last_access_at: i64,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    pub entries: i64,
    pub bytes: i64,
    pub pinned_bytes: i64,
}

impl Library {
    /// A cached file for `book_key`, if present and matching `md5` (when
    /// given). Touches the entry for LRU purposes.
    pub fn cache_lookup(&self, book_key: &str, md5: Option<&str>) -> Result<Option<CacheEntry>> {
        let c = self.conn();
        let hit: Option<CacheEntry> = c
            .query_row(
                "SELECT ce.book_key, ce.md5, ce.path, ce.bytes, ce.last_access_at,
                        COALESCE((SELECT pinned FROM books WHERE book_key = ce.book_key), 0)
                 FROM cache_entries ce WHERE ce.book_key = ?1",
                [book_key],
                |r| {
                    Ok(CacheEntry {
                        book_key: r.get(0)?,
                        md5: r.get(1)?,
                        path: r.get(2)?,
                        bytes: r.get(3)?,
                        last_access_at: r.get(4)?,
                        pinned: r.get(5)?,
                    })
                },
            )
            .optional()?;
        let Some(entry) = hit else { return Ok(None) };
        if let (Some(want), Some(have)) = (md5, entry.md5.as_deref()) {
            if !want.eq_ignore_ascii_case(have) {
                return Ok(None);
            }
        }
        c.execute(
            "UPDATE cache_entries SET last_access_at = ?2 WHERE book_key = ?1",
            params![book_key, bc_core::now_ms()],
        )?;
        Ok(Some(entry))
    }

    pub fn cache_put(
        &self,
        book_key: &str,
        md5: Option<&str>,
        path: &str,
        bytes: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO cache_entries(book_key, md5, path, bytes, last_access_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(book_key) DO UPDATE SET md5 = excluded.md5, path = excluded.path,
               bytes = excluded.bytes, last_access_at = excluded.last_access_at",
            params![book_key, md5, path, bytes, bc_core::now_ms()],
        )?;
        Ok(())
    }

    pub fn cache_remove(&self, book_key: &str) -> Result<Option<String>> {
        let c = self.conn();
        let path: Option<String> = c
            .query_row(
                "SELECT path FROM cache_entries WHERE book_key = ?1",
                [book_key],
                |r| r.get(0),
            )
            .optional()?;
        c.execute("DELETE FROM cache_entries WHERE book_key = ?1", [book_key])?;
        Ok(path)
    }

    pub fn cache_stats(&self) -> Result<CacheStats> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*), COALESCE(SUM(ce.bytes), 0),
                    COALESCE(SUM(CASE WHEN b.pinned = 1 THEN ce.bytes ELSE 0 END), 0)
             FROM cache_entries ce LEFT JOIN books b ON b.book_key = ce.book_key",
            [],
            |r| {
                Ok(CacheStats {
                    entries: r.get(0)?,
                    bytes: r.get(1)?,
                    pinned_bytes: r.get(2)?,
                })
            },
        )?)
    }

    /// Remove LRU, unpinned entries until the cache fits in `cap_bytes`,
    /// never touching `keep` (the book currently open). Returns the file
    /// paths the caller should delete.
    pub fn cache_evict(&self, cap_bytes: i64, keep: Option<&str>) -> Result<Vec<String>> {
        let total = self.cache_stats()?.bytes;
        if total <= cap_bytes {
            return Ok(Vec::new());
        }
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT ce.book_key, ce.path, ce.bytes FROM cache_entries ce
             LEFT JOIN books b ON b.book_key = ce.book_key
             WHERE COALESCE(b.pinned, 0) = 0 ORDER BY ce.last_access_at ASC",
        )?;
        let rows: Vec<(String, String, i64)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        let mut remaining = total;
        let mut paths = Vec::new();
        for (key, path, bytes) in rows {
            if remaining <= cap_bytes {
                break;
            }
            if Some(key.as_str()) == keep {
                continue;
            }
            c.execute("DELETE FROM cache_entries WHERE book_key = ?1", [&key])?;
            remaining -= bytes;
            paths.push(path);
        }
        Ok(paths)
    }

    /// Drop all unpinned cache entries except `keep` (for "clear cache").
    /// Returns the file paths the caller should delete.
    pub fn cache_clear_unpinned(&self, keep: Option<&str>) -> Result<Vec<String>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT ce.book_key, ce.path FROM cache_entries ce LEFT JOIN books b ON b.book_key = ce.book_key
             WHERE COALESCE(b.pinned, 0) = 0 AND ce.book_key IS NOT ?1",
        )?;
        let rows: Vec<(String, String)> = st
            .query_map([keep], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        for (k, _) in &rows {
            c.execute("DELETE FROM cache_entries WHERE book_key = ?1", [k])?;
        }
        Ok(rows.into_iter().map(|(_, p)| p).collect())
    }

    /// Pinned books that are not cached yet (to download for offline).
    pub fn pinned_uncached(&self) -> Result<Vec<i64>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT id FROM books b WHERE pinned = 1
             AND NOT EXISTS(SELECT 1 FROM cache_entries ce WHERE ce.book_key = b.book_key)",
        )?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::lib_with_tree;

    #[test]
    fn lru_eviction_respects_pins_and_open_book() {
        let (lib, _) = lib_with_tree();
        lib.cache_put("drive:dune", Some("m1"), "/c/dune", 100)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        lib.cache_put("drive:found", Some("m2"), "/c/found", 100)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        lib.cache_put("drive:moby", Some("m3"), "/c/moby", 100)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        lib.cache_put("drive:top", None, "/c/top", 100).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(
            lib.cache_lookup("drive:dune", Some("other"))
                .unwrap()
                .is_none(),
            "md5 mismatch"
        );
        let dune = lib.cache_lookup("drive:dune", Some("M1")).unwrap().unwrap();
        assert_eq!(dune.path, "/c/dune");

        let found = lib.book_id_by_key("drive:found").unwrap().unwrap();
        lib.set_pinned(found, true).unwrap();
        // Over cap by 200: found is pinned, moby is "open", so top and dune go.
        let evicted = lib.cache_evict(200, Some("drive:moby")).unwrap();
        assert_eq!(evicted, vec!["/c/top", "/c/dune"]);
        let s = lib.cache_stats().unwrap();
        assert_eq!((s.entries, s.bytes, s.pinned_bytes), (2, 200, 100));
        assert!(lib.cache_evict(1000, None).unwrap().is_empty());
        assert_eq!(lib.cache_clear_unpinned(None).unwrap(), vec!["/c/moby"]);
        assert!(lib.pinned_uncached().unwrap().is_empty());
        lib.cache_remove("drive:found").unwrap();
        assert_eq!(lib.pinned_uncached().unwrap(), vec![found]);
    }
}
