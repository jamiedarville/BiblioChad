use std::path::{Path, PathBuf};

/// On-disk locations. On Windows this resolves to
/// `%LOCALAPPDATA%\BiblioChad\...`, on Linux to
/// `$XDG_DATA_HOME/bibliochad/...` (`~/.local/share/bibliochad`).
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    /// Platform default location.
    pub fn platform_default() -> crate::Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "BiblioChad")
            .ok_or_else(|| crate::Error::Other("no home directory".into()))?;
        Ok(Self::at(dirs.data_local_dir()))
    }

    pub fn at(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn db(&self) -> PathBuf {
        self.root.join("library.sqlite3")
    }
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn covers(&self) -> PathBuf {
        self.root.join("covers")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// Optional OAuth client file (the "Desktop app" JSON downloaded from
    /// Google Cloud console).
    pub fn google_client_file(&self) -> PathBuf {
        self.root.join("google-client.json")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [self.root.clone(), self.cache(), self.covers(), self.logs()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}
