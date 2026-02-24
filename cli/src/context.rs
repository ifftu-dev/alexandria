use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

const APP_IDENTIFIER: &str = "org.alexandria.node";

/// Project context — resolved paths for the Alexandria project
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Root of the project (contains package.json + src-tauri/)
    pub root: PathBuf,
    /// src-tauri directory
    pub tauri_dir: PathBuf,
    /// App data directory (~/Library/Application Support/org.alexandria.node/)
    pub app_data_dir: PathBuf,
}

impl ProjectContext {
    /// Detect the project root by walking up from CWD looking for src-tauri/tauri.conf.json
    pub fn detect() -> Result<Self> {
        let cwd = env::current_dir().context("Failed to get current directory")?;
        let root = find_project_root(&cwd)?;
        let tauri_dir = root.join("src-tauri");
        let app_data_dir = Self::resolve_app_data_dir();

        Ok(Self {
            root,
            tauri_dir,
            app_data_dir,
        })
    }

    /// Get the SQLite database path
    pub fn db_path(&self) -> PathBuf {
        self.app_data_dir.join("alexandria.db")
    }

    /// Get the Stronghold vault path
    pub fn vault_path(&self) -> PathBuf {
        self.app_data_dir.join("vault.stronghold")
    }

    /// Get the iroh data directory
    pub fn iroh_dir(&self) -> PathBuf {
        self.app_data_dir.join("iroh")
    }

    /// Check if app data directory exists
    pub fn has_app_data(&self) -> bool {
        self.app_data_dir.exists()
    }

    /// Check if the database exists
    pub fn has_db(&self) -> bool {
        self.db_path().exists()
    }

    /// Check if the vault exists
    pub fn has_vault(&self) -> bool {
        self.vault_path().exists()
    }

    /// Resolve the app data directory based on platform
    fn resolve_app_data_dir() -> PathBuf {
        if cfg!(target_os = "macos") {
            dirs::home_dir()
                .unwrap_or_default()
                .join("Library/Application Support")
                .join(APP_IDENTIFIER)
        } else if cfg!(target_os = "linux") {
            dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share"))
                .join(APP_IDENTIFIER)
        } else if cfg!(target_os = "windows") {
            dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("AppData/Roaming"))
                .join(APP_IDENTIFIER)
        } else {
            dirs::home_dir()
                .unwrap_or_default()
                .join(format!(".{}", APP_IDENTIFIER))
        }
    }
}

/// Walk up from the given path looking for a directory containing src-tauri/tauri.conf.json
fn find_project_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("src-tauri/tauri.conf.json").exists() {
            return Ok(current);
        }
        if !current.pop() {
            bail!(
                "Could not find Alexandria project root.\n\
                 Run this command from within the project directory.\n\
                 (Looking for src-tauri/tauri.conf.json)"
            );
        }
    }
}
