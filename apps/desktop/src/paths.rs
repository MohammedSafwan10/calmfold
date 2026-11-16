//! Where Calmfold keeps its files.
//!
//! * `CALMFOLD_DATA_DIR` overrides everything (tests, portable installs).
//! * Debug builds use a separate "Calmfold Dev" folder so development never
//!   touches real data.
//! * Release builds use the platform data folder:
//!   macOS `~/Library/Application Support/com.Nexdark.Calmfold`,
//!   Windows `%APPDATA%\Nexdark\Calmfold\data`.
//!
//! The app was called Keepstone before. On first launch an old Keepstone
//! folder is moved to the new place and `keepstone.sqlite3` (with its WAL
//! files) is renamed, so nothing is lost. If the move fails, the old folder
//! keeps being used.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use directories::ProjectDirs;

pub const DATA_DIR_ENV: &str = "CALMFOLD_DATA_DIR";

/// Database file name (and its legacy name).
const DATABASE: &str = "calmfold.sqlite3";
const LEGACY_DATABASE: &str = "keepstone.sqlite3";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub existing_database: bool,
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub appearance: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let data_dir = match std::env::var_os(DATA_DIR_ENV) {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                let app = if cfg!(debug_assertions) {
                    "Calmfold Dev"
                } else {
                    "Calmfold"
                };
                let dir = ProjectDirs::from("com", "Nexdark", app)
                    .context("could not find a home folder for app data")?
                    .data_dir()
                    .to_path_buf();
                let legacy_app = if cfg!(debug_assertions) {
                    "Keepstone Dev"
                } else {
                    "Keepstone"
                };
                let legacy = ProjectDirs::from("app", "Keepstone", legacy_app)
                    .map(|p| p.data_dir().to_path_buf());
                adopt_legacy_dir(&dir, legacy.as_deref())
            }
        };
        rename_legacy_database(&data_dir);
        Ok(Self::in_dir(data_dir))
    }

    #[must_use]
    pub fn in_dir(data_dir: PathBuf) -> Self {
        Self {
            existing_database: data_dir.join(DATABASE).exists(),
            database: data_dir.join(DATABASE),
            appearance: data_dir.join("appearance.json"),
            data_dir,
        }
    }
}

/// Moves the old Keepstone folder to `dir` when `dir` doesn't exist yet.
/// Returns the folder to use.
fn adopt_legacy_dir(dir: &Path, legacy: Option<&Path>) -> PathBuf {
    let Some(legacy) = legacy.filter(|l| l.is_dir()) else {
        return dir.to_path_buf();
    };
    if dir.exists() {
        return dir.to_path_buf();
    }
    if let Some(parent) = dir.parent() {
        _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(legacy, dir) {
        Ok(()) => dir.to_path_buf(),
        Err(err) => {
            eprintln!(
                "calmfold: couldn't move the old Keepstone folder ({err}); using it in place"
            );
            legacy.to_path_buf()
        }
    }
}

/// `keepstone.sqlite3` (+ `-wal`, `-shm`) → `calmfold.sqlite3`, once.
fn rename_legacy_database(dir: &Path) {
    let new = dir.join(DATABASE);
    let old = dir.join(LEGACY_DATABASE);
    if new.exists() || !old.exists() {
        return;
    }
    for suffix in ["-wal", "-shm", ""] {
        let from = dir.join(format!("{LEGACY_DATABASE}{suffix}"));
        if from.exists()
            && let Err(err) = std::fs::rename(&from, dir.join(format!("{DATABASE}{suffix}")))
        {
            eprintln!("calmfold: couldn't rename the old database file: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keepstone_data_moves_over_once() {
        let root = tempfile::tempdir().expect("dir");
        let legacy = root.path().join("app.Keepstone.Keepstone");
        std::fs::create_dir_all(&legacy).expect("mkdir");
        for f in [
            "keepstone.sqlite3",
            "keepstone.sqlite3-wal",
            "appearance.json",
        ] {
            std::fs::write(legacy.join(f), f).expect("write");
        }
        let new = root.path().join("com.Nexdark.Calmfold");
        let dir = adopt_legacy_dir(&new, Some(&legacy));
        assert_eq!(dir, new);
        assert!(!legacy.exists());
        rename_legacy_database(&dir);
        assert_eq!(
            std::fs::read_to_string(dir.join("calmfold.sqlite3")).expect("db"),
            "keepstone.sqlite3"
        );
        assert!(dir.join("calmfold.sqlite3-wal").exists());
        assert!(dir.join("appearance.json").exists());
        // Already moved: nothing happens, an existing new folder wins.
        assert_eq!(adopt_legacy_dir(&new, Some(&legacy)), new);
        rename_legacy_database(&dir);
        assert!(dir.join("calmfold.sqlite3").exists());
    }
}
