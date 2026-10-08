//! Google Drive access for BiblioChad.
//!
//! * [`oauth`]: installed-app OAuth 2.0 with a loopback redirect and PKCE.
//!   The refresh token lives in the OS credential store (Windows Credential
//!   Manager); access tokens stay in memory.
//! * [`client`]: a small Drive v3 client with retry/backoff, the folder
//!   crawl, the changes feed, verified downloads and the `appDataFolder`
//!   sync file. The library scope is read-only; the only write is to
//!   `appDataFolder`.

pub mod client;
pub mod oauth;
pub mod token_store;

pub use client::{Change, CrawlState, DriveClient, DriveFile, SharedDrive};
pub use oauth::{Auth, OAuthClient, PendingAuth};
pub use token_store::{KeyringStore, MemoryStore, TokenStore};

pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub const SHORTCUT_MIME: &str = "application/vnd.google-apps.shortcut";

pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/drive.appdata",
];

#[derive(Debug, thiserror::Error)]
pub enum DriveError {
    /// The refresh token is gone or revoked; the user must sign in again.
    #[error("Google sign-in expired or was revoked. Reconnect Google Drive.")]
    ReauthRequired,
    #[error("Google Drive is not connected.")]
    NotConnected,
    #[error("OAuth client is not configured: {0}")]
    NotConfigured(String),
    #[error("sign-in was cancelled or failed: {0}")]
    AuthFailed(String),
    #[error("Drive API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("downloaded file failed its checksum (expected {expected}, got {actual})")]
    Checksum { expected: String, actual: String },
    #[error("credential store: {0}")]
    Keyring(String),
    #[error("invalid response: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, DriveError>;
