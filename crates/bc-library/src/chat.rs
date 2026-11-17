//! Book chat history (BC-4): one conversation per book, stored only in
//! this local database. It is deliberately absent from `sync.rs`, so it
//! is never written to the Drive sync file (same rule as zone maps and
//! lab guides).

use rusqlite::{params, Row};
use serde::Serialize;

use bc_core::Locator;

use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BookChatMessage {
    pub id: String,
    pub book_id: i64,
    /// `"user"` or `"assistant"`.
    pub role: String,
    pub content: String,
    /// Where the reader was when the message was sent, when known.
    pub locator: Option<Locator>,
    pub label: String,
    /// Provider model for assistant rows; empty for user rows.
    pub model: String,
    /// `"passage"` (current page/section) or `"book"` (whole book).
    pub context_mode: String,
    pub created_at: i64,
}

fn message_from_row(r: &Row) -> rusqlite::Result<BookChatMessage> {
    let loc: Option<String> = r.get("locator_json")?;
    Ok(BookChatMessage {
        id: r.get("id")?,
        book_id: r.get("book_id")?,
        role: r.get("role")?,
        content: r.get("content")?,
        // A corrupt locator must not hide the message itself.
        locator: loc.as_deref().and_then(|s| Locator::from_json(s).ok()),
        label: r.get("label")?,
        model: r.get("model")?,
        context_mode: r.get("context_mode")?,
        created_at: r.get("created_at")?,
    })
}

impl Library {
    /// Append one message to a book's chat. Rejects unknown roles,
    /// unknown context modes and empty content.
    #[allow(clippy::too_many_arguments)]
    pub fn add_book_chat_message(
        &self,
        book_id: i64,
        role: &str,
        content: &str,
        locator: Option<&Locator>,
        label: &str,
        model: &str,
        context_mode: &str,
    ) -> Result<BookChatMessage> {
        if role != "user" && role != "assistant" {
            return Err(LibraryError::Invalid(format!("chat role {role:?}")));
        }
        if context_mode != "passage" && context_mode != "book" {
            return Err(LibraryError::Invalid(format!(
                "chat context mode {context_mode:?}"
            )));
        }
        if content.trim().is_empty() {
            return Err(LibraryError::Invalid("empty chat message".into()));
        }
        let id = bc_core::new_id();
        let now = bc_core::now_ms();
        self.conn().execute(
            "INSERT INTO book_chat_messages(id, book_id, role, content, locator_json, label, model, context_mode, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                book_id,
                role,
                content,
                locator.map(|l| l.to_json()),
                label,
                model,
                context_mode,
                now
            ],
        )?;
        Ok(BookChatMessage {
            id,
            book_id,
            role: role.to_string(),
            content: content.to_string(),
            locator: locator.cloned(),
            label: label.to_string(),
            model: model.to_string(),
            context_mode: context_mode.to_string(),
            created_at: now,
        })
    }

    /// A book's chat in conversation order (oldest first), capped at the
    /// most recent `limit` messages.
    pub fn book_chat_messages(&self, book_id: i64, limit: u32) -> Result<Vec<BookChatMessage>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT * FROM (
               SELECT *, rowid AS seq FROM book_chat_messages WHERE book_id = ?1
               ORDER BY created_at DESC, seq DESC LIMIT ?2
             ) ORDER BY created_at ASC, seq ASC",
        )?;
        let rows = st.query_map(params![book_id, limit], message_from_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Delete a book's whole chat; returns how many messages went.
    /// Other books are untouched.
    pub fn clear_book_chat(&self, book_id: i64) -> Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM book_chat_messages WHERE book_id = ?1",
            [book_id],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::lib_with_tree;
    use crate::BookQuery;

    fn two_books() -> (Library, i64, i64) {
        let (lib, _) = lib_with_tree();
        let books = lib.query_books(&BookQuery::default()).unwrap();
        (lib, books[0].id, books[1].id)
    }

    fn loc() -> Locator {
        Locator::Pdf {
            page: 4,
            offset: 0.0,
            fit: Default::default(),
            zoom: 1.0,
        }
    }

    #[test]
    fn append_list_oldest_first_and_per_book_isolation() {
        let (lib, a, b) = two_books();
        lib.add_book_chat_message(a, "user", "q1", Some(&loc()), "page 5", "", "passage")
            .unwrap();
        lib.add_book_chat_message(a, "assistant", "a1", None, "", "model-x", "passage")
            .unwrap();
        lib.add_book_chat_message(b, "user", "other book", None, "", "", "book")
            .unwrap();

        let msgs = lib.book_chat_messages(a, 50).unwrap();
        assert_eq!(
            msgs.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            vec!["q1", "a1"]
        );
        assert_eq!(msgs[0].locator, Some(loc()));
        assert_eq!(msgs[0].label, "page 5");
        assert_eq!(msgs[1].model, "model-x");
        assert!(msgs.iter().all(|m| m.book_id == a), "no cross-book leakage");

        let other = lib.book_chat_messages(b, 50).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].context_mode, "book");
    }

    #[test]
    fn clear_only_that_book_and_limit_keeps_newest() {
        let (lib, a, b) = two_books();
        for i in 0..5 {
            lib.add_book_chat_message(a, "user", &format!("m{i}"), None, "", "", "passage")
                .unwrap();
        }
        lib.add_book_chat_message(b, "user", "keep", None, "", "", "passage")
            .unwrap();

        let tail = lib.book_chat_messages(a, 2).unwrap();
        assert_eq!(
            tail.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            vec!["m3", "m4"],
            "limit keeps the newest, still oldest-first"
        );

        assert_eq!(lib.clear_book_chat(a).unwrap(), 5);
        assert!(lib.book_chat_messages(a, 50).unwrap().is_empty());
        assert_eq!(lib.book_chat_messages(b, 50).unwrap().len(), 1);
        assert_eq!(lib.clear_book_chat(a).unwrap(), 0, "clear is idempotent");
    }

    #[test]
    fn rejects_bad_role_mode_and_empty_content() {
        let (lib, a, _) = two_books();
        assert!(lib
            .add_book_chat_message(a, "system", "x", None, "", "", "passage")
            .is_err());
        assert!(lib
            .add_book_chat_message(a, "user", "x", None, "", "", "chapter")
            .is_err());
        assert!(lib
            .add_book_chat_message(a, "user", "   ", None, "", "", "passage")
            .is_err());
        assert!(lib.book_chat_messages(a, 50).unwrap().is_empty());
    }
}
