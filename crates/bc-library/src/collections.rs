use rusqlite::params;
use serde::Serialize;

use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub book_count: i64,
}

impl Library {
    pub fn collections(&self) -> Result<Vec<Collection>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT c.id, c.name, COUNT(cb.book_id) FROM collections c
             LEFT JOIN collection_books cb ON cb.collection_id = c.id
             GROUP BY c.id ORDER BY c.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                book_count: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_collection(&self, name: &str) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::Invalid("collection name is empty".into()));
        }
        let c = self.conn();
        c.execute(
            "INSERT OR IGNORE INTO collections(name) VALUES (?1)",
            [name],
        )?;
        Ok(
            c.query_row("SELECT id FROM collections WHERE name = ?1", [name], |r| {
                r.get(0)
            })?,
        )
    }

    pub fn rename_collection(&self, id: i64, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::Invalid("collection name is empty".into()));
        }
        self.conn().execute(
            "UPDATE collections SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn delete_collection(&self, id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM collections WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_in_collection(&self, collection_id: i64, book_id: i64, member: bool) -> Result<()> {
        let c = self.conn();
        if member {
            c.execute(
                "INSERT OR IGNORE INTO collection_books(collection_id, book_id) VALUES (?1, ?2)",
                params![collection_id, book_id],
            )?;
        } else {
            c.execute(
                "DELETE FROM collection_books WHERE collection_id = ?1 AND book_id = ?2",
                params![collection_id, book_id],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::lib_with_tree;
    use crate::BookQuery;

    #[test]
    fn collections_are_independent_of_folders() {
        let (lib, _) = lib_with_tree();
        let dune = lib.book_id_by_key("drive:dune").unwrap().unwrap();
        let moby = lib.book_id_by_key("drive:moby").unwrap().unwrap();
        let c = lib.create_collection(" Favourites ").unwrap();
        assert_eq!(
            lib.create_collection("favourites").unwrap(),
            c,
            "case-insensitive"
        );
        lib.set_in_collection(c, dune, true).unwrap();
        lib.set_in_collection(c, moby, true).unwrap();
        lib.set_in_collection(c, moby, true).unwrap();
        let cols = lib.collections().unwrap();
        assert_eq!(cols[0].book_count, 2);
        let q = BookQuery {
            collection_id: Some(c),
            ..Default::default()
        };
        assert_eq!(lib.query_books(&q).unwrap().len(), 2);
        lib.set_in_collection(c, moby, false).unwrap();
        assert_eq!(lib.query_books(&q).unwrap().len(), 1);
        assert_eq!(lib.book_details(dune).unwrap().collections, vec![c]);
        lib.rename_collection(c, "Spice").unwrap();
        lib.delete_collection(c).unwrap();
        assert!(lib.collections().unwrap().is_empty());
        assert!(lib.create_collection("  ").is_err());
    }
}
