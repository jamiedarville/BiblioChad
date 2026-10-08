use rusqlite::Connection;

use crate::Result;

/// Ordered migrations. Each runs once; `PRAGMA user_version` tracks progress.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE accounts(
  id INTEGER PRIMARY KEY,
  email TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE sources(
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('drive', 'local')),
  account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
  drive_root_id TEXT,
  name TEXT NOT NULL,
  changes_page_token TEXT,
  crawl_state TEXT,
  last_synced_at INTEGER
);

-- Mirror of Drive items under the library root (plus local imports).
CREATE TABLE nodes(
  id INTEGER PRIMARY KEY,
  source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  drive_id TEXT NOT NULL UNIQUE,
  parent_drive_id TEXT,
  name TEXT NOT NULL,
  mime_type TEXT,
  size INTEGER,
  md5 TEXT,
  modified_time TEXT,
  is_folder INTEGER NOT NULL DEFAULT 0,
  is_trashed INTEGER NOT NULL DEFAULT 0,
  shortcut_target_id TEXT,
  thumbnail_link TEXT,
  local_path TEXT,
  -- 'drive:<fileId>' / 'local:<sha256>' for book files, NULL otherwise.
  file_key TEXT
);
CREATE INDEX nodes_parent ON nodes(parent_drive_id);
CREATE INDEX nodes_file_key ON nodes(file_key);
CREATE INDEX nodes_shortcut ON nodes(shortcut_target_id);

-- One row per distinct book file (a shortcut and its target share a row).
CREATE TABLE books(
  id INTEGER PRIMARY KEY,
  book_key TEXT NOT NULL UNIQUE,
  format TEXT NOT NULL CHECK (format IN ('epub', 'pdf')),
  file_name TEXT NOT NULL,
  title TEXT, author TEXT, series TEXT, series_index REAL,
  publisher TEXT, language TEXT, isbn TEXT, description TEXT,
  page_count INTEGER,
  cover_path TEXT,
  content_md5 TEXT,
  added_at INTEGER NOT NULL,
  metadata_extracted_at INTEGER,
  title_override TEXT, author_override TEXT, series_override TEXT,
  status_override TEXT,
  favorite INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE progress(
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  device_id TEXT NOT NULL,
  locator_json TEXT NOT NULL,
  percent REAL NOT NULL,
  status TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (book_id, device_id)
);
CREATE INDEX progress_updated ON progress(updated_at);

CREATE TABLE bookmarks(
  id TEXT PRIMARY KEY,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  locator_json TEXT NOT NULL,
  label TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE TABLE annotations(
  id TEXT PRIMARY KEY,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  locator_json TEXT NOT NULL,
  text TEXT,
  color TEXT,
  note TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX annotations_book ON annotations(book_id);

CREATE TABLE collections(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
CREATE TABLE collection_books(
  collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  PRIMARY KEY (collection_id, book_id)
);

-- Downloaded book files, keyed by book_key + content md5.
CREATE TABLE cache_entries(
  book_key TEXT PRIMARY KEY,
  md5 TEXT,
  path TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  last_access_at INTEGER NOT NULL
);

CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);

-- Days (YYYY-MM-DD, local time) on which any reading happened; for streaks.
CREATE TABLE reading_days(day TEXT PRIMARY KEY);
"#];

pub fn migrate(conn: &Connection) -> Result<()> {
    let version: usize = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("db.sqlite");
        crate::Library::open(&p).unwrap();
        let lib = crate::Library::open(&p).unwrap();
        let v: i64 = lib.conn().pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v as usize, super::MIGRATIONS.len());
    }
}
