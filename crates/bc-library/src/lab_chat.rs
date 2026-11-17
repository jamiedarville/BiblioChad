//! Lab co-pilot chat history (BC-5): one lab-scoped conversation per
//! book, stored only in this local database and deliberately separate
//! from `book_chat_messages` — clearing one never touches the other.
//! Absent from `sync.rs`, so it is never written to Drive.

use rusqlite::{params, Row};
use serde::Serialize;

use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LabChatMessage {
    pub id: String,
    pub book_id: i64,
    /// `"user"` or `"assistant"`.
    pub role: String,
    pub content: String,
    /// Lab (0-based) attached to this turn.
    pub lab_index: i64,
    /// Step (0-based) attached to this turn, when a per-step Ask made it.
    pub step_index: Option<i64>,
    /// Human attachment label, e.g. "Lab 1 · Step 2".
    pub label: String,
    /// Provider model for assistant rows; empty for user rows.
    pub model: String,
    pub created_at: i64,
}

fn message_from_row(r: &Row) -> rusqlite::Result<LabChatMessage> {
    Ok(LabChatMessage {
        id: r.get("id")?,
        book_id: r.get("book_id")?,
        role: r.get("role")?,
        content: r.get("content")?,
        lab_index: r.get("lab_index")?,
        step_index: r.get("step_index")?,
        label: r.get("label")?,
        model: r.get("model")?,
        created_at: r.get("created_at")?,
    })
}

impl Library {
    /// Append one message to a book's lab chat. Rejects unknown roles,
    /// negative attachments and empty content.
    #[allow(clippy::too_many_arguments)]
    pub fn add_lab_chat_message(
        &self,
        book_id: i64,
        role: &str,
        content: &str,
        lab_index: i64,
        step_index: Option<i64>,
        label: &str,
        model: &str,
    ) -> Result<LabChatMessage> {
        if role != "user" && role != "assistant" {
            return Err(LibraryError::Invalid(format!("lab chat role {role:?}")));
        }
        if lab_index < 0 || step_index.is_some_and(|s| s < 0) {
            return Err(LibraryError::Invalid("negative lab attachment".into()));
        }
        if content.trim().is_empty() {
            return Err(LibraryError::Invalid("empty lab chat message".into()));
        }
        let id = bc_core::new_id();
        let now = bc_core::now_ms();
        self.conn().execute(
            "INSERT INTO lab_chat_messages(id, book_id, role, content, lab_index, step_index, label, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, book_id, role, content, lab_index, step_index, label, model, now],
        )?;
        Ok(LabChatMessage {
            id,
            book_id,
            role: role.to_string(),
            content: content.to_string(),
            lab_index,
            step_index,
            label: label.to_string(),
            model: model.to_string(),
            created_at: now,
        })
    }

    /// A book's lab chat in conversation order, capped at the most
    /// recent `limit` messages.
    pub fn lab_chat_messages(&self, book_id: i64, limit: u32) -> Result<Vec<LabChatMessage>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT * FROM (
               SELECT *, rowid AS seq FROM lab_chat_messages WHERE book_id = ?1
               ORDER BY created_at DESC, seq DESC LIMIT ?2
             ) ORDER BY created_at ASC, seq ASC",
        )?;
        let rows = st.query_map(params![book_id, limit], message_from_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Delete a book's lab chat only; its book chat is untouched.
    pub fn clear_lab_chat(&self, book_id: i64) -> Result<usize> {
        Ok(self.conn().execute(
            "DELETE FROM lab_chat_messages WHERE book_id = ?1",
            [book_id],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::lib_with_tree;
    use crate::BookQuery;

    #[test]
    fn lab_chat_is_per_book_and_separate_from_book_chat() {
        let (lib, _) = lib_with_tree();
        let books = lib.query_books(&BookQuery::default()).unwrap();
        let (a, b) = (books[0].id, books[1].id);
        lib.add_lab_chat_message(a, "user", "why step 2?", 0, Some(1), "Lab 1 · Step 2", "")
            .unwrap();
        lib.add_lab_chat_message(a, "assistant", "because…", 0, Some(1), "", "m")
            .unwrap();
        lib.add_book_chat_message(a, "user", "book question", None, "", "", "passage")
            .unwrap();
        lib.add_lab_chat_message(b, "user", "other book", 2, None, "Lab 3", "")
            .unwrap();

        let msgs = lib.lab_chat_messages(a, 50).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].step_index, Some(1));
        assert_eq!(msgs[0].label, "Lab 1 · Step 2");

        // Clearing lab chat keeps book chat, and vice versa.
        assert_eq!(lib.clear_lab_chat(a).unwrap(), 2);
        assert!(lib.lab_chat_messages(a, 50).unwrap().is_empty());
        assert_eq!(lib.book_chat_messages(a, 50).unwrap().len(), 1);
        assert_eq!(lib.lab_chat_messages(b, 50).unwrap().len(), 1);
        assert!(lib
            .add_lab_chat_message(a, "system", "x", 0, None, "", "")
            .is_err());
        assert!(lib
            .add_lab_chat_message(a, "user", " ", 0, None, "", "")
            .is_err());
    }
}
