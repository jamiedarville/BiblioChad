use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bc_core::paths::AppPaths;
use bc_drive::{Auth, DriveClient, KeyringStore, OAuthClient, TokenStore};
use bc_library::Library;
use bc_reader::epub::EpubArchive;
use bc_reader::pdf::PdfEngine;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Preferences the Rust side cares about. The UI stores its own (theme,
/// typography, Chad Mode, ...) in the same JSON blob; unknown keys are kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorePrefs {
    #[serde(default = "default_cache_cap")]
    pub cache_cap_mb: u64,
}

fn default_cache_cap() -> u64 {
    2048
}

pub const PREFS_KEY: &str = "prefs";

pub struct AppState {
    pub paths: AppPaths,
    pub lib: Arc<Library>,
    pub device_id: String,
    pub pdf: Result<PdfEngine, String>,
    pub http: reqwest::Client,
    pub store: Arc<dyn TokenStore>,
    pub drive: RwLock<Option<DriveClient>>,
    pub epubs: Mutex<LruCache<i64, Arc<Mutex<EpubArchive>>>>,
    /// Book currently open in the reader, so eviction never removes it.
    pub open_book_key: Mutex<Option<String>>,
    pub sync_lock: tokio::sync::Mutex<()>,
    pub syncing: AtomicBool,
    /// Book to open once the UI is ready (from "Open with BiblioChad").
    pub pending_open: Mutex<Option<i64>>,
}

impl AppState {
    pub fn init(paths: AppPaths, pdfium_dir: Option<&Path>) -> anyhow_like::Result<Self> {
        paths.ensure()?;
        let lib = Arc::new(Library::open(&paths.db())?);
        let device_id = lib.device_id()?;
        let pdf = PdfEngine::start(pdfium_dir).map_err(|e| e.to_string());
        let http = reqwest::Client::builder()
            .user_agent(concat!("BiblioChad/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| anyhow_like::Error(e.to_string()))?;
        let store: Arc<dyn TokenStore> = Arc::new(KeyringStore::default());
        let state = Self {
            paths,
            lib,
            device_id,
            pdf,
            http,
            store,
            drive: RwLock::new(None),
            epubs: Mutex::new(LruCache::new(NonZeroUsize::new(3).unwrap())),
            open_book_key: Mutex::new(None),
            sync_lock: tokio::sync::Mutex::new(()),
            syncing: AtomicBool::new(false),
            pending_open: Mutex::new(None),
        };
        Ok(state)
    }

    /// The OAuth client: `google-client.json` in the data folder wins, then
    /// values baked in at build time.
    pub fn oauth_client(&self) -> Option<OAuthClient> {
        if let Ok(bytes) = std::fs::read(self.paths.google_client_file()) {
            match OAuthClient::from_installed_json(&bytes) {
                Ok(c) => return Some(c),
                Err(e) => tracing::warn!("ignoring google-client.json: {e}"),
            }
        }
        let id = option_env!("BIBLIOCHAD_GOOGLE_CLIENT_ID")?;
        Some(OAuthClient::new(
            id,
            option_env!("BIBLIOCHAD_GOOGLE_CLIENT_SECRET").map(str::to_string),
        ))
    }

    pub fn make_drive_client(&self, auth: Auth) -> DriveClient {
        let shared = self.lib.setting("drive_shared_drive_id").ok().flatten();
        DriveClient::new(self.http.clone(), auth, self.store.clone()).with_shared_drive(shared)
    }

    /// Rebuild the Drive client from the stored refresh token, if any.
    pub async fn restore_drive(&self) {
        let Some(client) = self.oauth_client() else {
            return;
        };
        match self.store.load() {
            Ok(Some(_)) => {
                let auth = Auth::new(client, self.http.clone());
                *self.drive.write().await = Some(self.make_drive_client(auth));
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("credential store unavailable: {e}"),
        }
    }

    pub async fn drive(&self) -> Option<DriveClient> {
        self.drive.read().await.clone()
    }

    pub fn core_prefs(&self) -> CorePrefs {
        self.lib
            .setting_json::<CorePrefs>(PREFS_KEY)
            .ok()
            .flatten()
            .unwrap_or(CorePrefs {
                cache_cap_mb: default_cache_cap(),
            })
    }

    pub fn cover_file(&self, book_id: i64) -> PathBuf {
        self.paths.covers().join(format!("{book_id}.jpg"))
    }

    pub fn pdf(&self) -> Result<&PdfEngine, crate::error::CmdError> {
        self.pdf.as_ref().map_err(|e| {
            crate::error::CmdError::new(
                "pdfium_missing",
                format!("PDF support is unavailable because pdfium could not be loaded ({e}). Reinstall BiblioChad or place the pdfium library next to the app."),
            )
        })
    }
}

/// Minimal error plumbing for startup, without pulling in anyhow.
pub mod anyhow_like {
    #[derive(Debug)]
    pub struct Error(pub String);
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl std::error::Error for Error {}
    impl From<std::io::Error> for Error {
        fn from(e: std::io::Error) -> Self {
            Self(e.to_string())
        }
    }
    impl From<bc_library::LibraryError> for Error {
        fn from(e: bc_library::LibraryError) -> Self {
            Self(e.to_string())
        }
    }
    pub type Result<T> = std::result::Result<T, Error>;
}
