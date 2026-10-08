use std::sync::Mutex;

use crate::{DriveError, Result};

/// Where the long-lived refresh token is kept.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<String>>;
    fn save(&self, refresh_token: &str) -> Result<()>;
    fn clear(&self) -> Result<()>;
}

/// OS credential store: Windows Credential Manager on Windows, Keychain on
/// macOS, the kernel keyring on Linux.
pub struct KeyringStore {
    service: String,
    user: String,
}

impl KeyringStore {
    pub fn new(service: &str, user: &str) -> Self {
        Self {
            service: service.into(),
            user: user.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, &self.user)
            .map_err(|e| DriveError::Keyring(e.to_string()))
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new("BiblioChad", "google-drive-refresh-token")
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<String>> {
        match self.entry()?.get_password() {
            Ok(t) => Ok(Some(t)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(DriveError::Keyring(e.to_string())),
        }
    }

    fn save(&self, refresh_token: &str) -> Result<()> {
        self.entry()?
            .set_password(refresh_token)
            .map_err(|e| DriveError::Keyring(e.to_string()))
    }

    fn clear(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(DriveError::Keyring(e.to_string())),
        }
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryStore(Mutex<Option<String>>);

impl MemoryStore {
    pub fn with(token: &str) -> Self {
        Self(Mutex::new(Some(token.into())))
    }
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, refresh_token: &str) -> Result<()> {
        *self.0.lock().unwrap() = Some(refresh_token.into());
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
