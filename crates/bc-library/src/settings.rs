use rusqlite::{params, OptionalExtension};

use crate::{Library, Result};

impl Library {
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn().execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Settings as JSON values (UI preferences are stored as JSON strings).
    pub fn setting_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.setting(key)? {
            Some(s) => Ok(serde_json::from_str(&s).ok()),
            None => Ok(None),
        }
    }

    pub fn set_setting_json<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.set_setting(key, &serde_json::to_string(value)?)
    }
}

#[cfg(test)]
mod tests {
    use crate::Library;

    #[test]
    fn settings_roundtrip() {
        let lib = Library::open_in_memory().unwrap();
        assert_eq!(lib.setting("x").unwrap(), None);
        lib.set_setting("x", "1").unwrap();
        lib.set_setting("x", "2").unwrap();
        assert_eq!(lib.setting("x").unwrap().as_deref(), Some("2"));
        let id = lib.device_id().unwrap();
        assert_eq!(lib.device_id().unwrap(), id);
        lib.set_setting_json("prefs", &vec![1, 2]).unwrap();
        assert_eq!(lib.setting_json::<Vec<i32>>("prefs").unwrap(), Some(vec![1, 2]));
    }
}
