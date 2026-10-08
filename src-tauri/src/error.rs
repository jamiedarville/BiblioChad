use serde::Serialize;

/// Error type returned to the UI. `kind` lets the frontend react (e.g. show
/// "Reconnect" for `reauth`); `message` is plain and actionable, never a joke.
#[derive(Debug, Serialize)]
pub struct CmdError {
    pub kind: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl From<bc_library::LibraryError> for CmdError {
    fn from(e: bc_library::LibraryError) -> Self {
        match e {
            bc_library::LibraryError::NotFound(m) => Self::new("not_found", m),
            bc_library::LibraryError::Invalid(m) => Self::invalid(m),
            other => Self::internal(other.to_string()),
        }
    }
}

impl From<bc_drive::DriveError> for CmdError {
    fn from(e: bc_drive::DriveError) -> Self {
        use bc_drive::DriveError as D;
        let kind = match &e {
            D::ReauthRequired => "reauth",
            D::NotConnected => "not_connected",
            D::NotConfigured(_) => "not_configured",
            D::AuthFailed(_) => "auth_failed",
            D::Http(_) => "offline",
            D::Checksum { .. } => "checksum",
            D::Api { status: 404, .. } => "not_found",
            _ => "drive",
        };
        Self::new(kind, e.to_string())
    }
}

impl From<bc_reader::ReaderError> for CmdError {
    fn from(e: bc_reader::ReaderError) -> Self {
        Self::new("book", e.to_string())
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        Self::internal(e.to_string())
    }
}

pub type CmdResult<T> = Result<T, CmdError>;
