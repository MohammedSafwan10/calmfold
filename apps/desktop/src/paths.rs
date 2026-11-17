//! Where Ottero keeps its files.
//!
//! * `OTTERO_DATA_DIR` overrides everything (tests, portable installs). The
//!   pre-rename `CALMFOLD_DATA_DIR` is still read when it isn't set.
//! * Debug builds use a separate "Ottero Dev" folder so development never
//!   touches real data.
//! * Release builds use the platform data folder:
//!   macOS `~/Library/Application Support/com.Nexdark.Ottero`,
//!   Windows `%APPDATA%\Nexdark\Ottero\data`.
//!
//! The app was called Calmfold before, and Keepstone before that. On first
//! launch the newest old folder that exists (Calmfold, then Keepstone) is
//! moved to the new place, and an old database file (`calmfold.sqlite3` or
//! `keepstone.sqlite3`, with its WAL files) is renamed to `ottero.sqlite3`.
//! Both steps run once and never overwrite anything: an existing new folder
//! or database wins. If the move fails, the old folder keeps being used.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use directories::ProjectDirs;

pub const DATA_DIR_ENV: &str = "OTTERO_DATA_DIR";
/// Read when [`DATA_DIR_ENV`] isn't set, so scripts and portable setups
/// from before the rename keep working.
pub const LEGACY_DATA_DIR_ENV: &str = "CALMFOLD_DATA_DIR";

/// Database file name.
const DATABASE: &str = "ottero.sqlite3";
/// Older database names, newest first.
const LEGACY_DATABASES: [&str; 2] = ["calmfold.sqlite3", "keepstone.sqlite3"];

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub existing_database: bool,
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub appearance: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let override_dir = [DATA_DIR_ENV, LEGACY_DATA_DIR_ENV]
            .into_iter()
            .filter_map(std::env::var_os)
            .find(|dir| !dir.is_empty());
        let data_dir = match override_dir {
            Some(dir) => PathBuf::from(dir),
            None => {
                let dev = cfg!(debug_assertions);
                let suffix = if dev { " Dev" } else { "" };
                let dir = ProjectDirs::from("com", "Nexdark", &format!("Ottero{suffix}"))
                    .context("could not find a home folder for app data")?
                    .data_dir()
                    .to_path_buf();
                // Newest first: a Calmfold folder already holds any migrated
                // Keepstone data.
                let legacy: Vec<PathBuf> = [
                    ProjectDirs::from("com", "Nexdark", &format!("Calmfold{suffix}")),
                    ProjectDirs::from("app", "Keepstone", &format!("Keepstone{suffix}")),
                ]
                .into_iter()
                .flatten()
                .map(|p| p.data_dir().to_path_buf())
                .collect();
                adopt_legacy_dir(&dir, &legacy)
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

/// Moves the first existing old folder in `legacy` to `dir` when `dir`
/// doesn't exist yet. Returns the folder to use.
fn adopt_legacy_dir(dir: &Path, legacy: &[PathBuf]) -> PathBuf {
    if dir.exists() {
        return dir.to_path_buf();
    }
    let Some(legacy) = legacy.iter().find(|l| l.is_dir()) else {
        return dir.to_path_buf();
    };
    if let Some(parent) = dir.parent() {
        _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(legacy, dir) {
        Ok(()) => dir.to_path_buf(),
        Err(err) => {
            eprintln!("ottero: couldn't move the old data folder ({err}); using it in place");
            legacy.clone()
        }
    }
}

/// The newest old database (+ `-wal`, `-shm`) → `ottero.sqlite3`, once.
/// Nothing happens when `ottero.sqlite3` exists, and no file is overwritten.
fn rename_legacy_database(dir: &Path) {
    if dir.join(DATABASE).exists() {
        return;
    }
    let Some(old) = LEGACY_DATABASES
        .into_iter()
        .find(|name| dir.join(name).exists())
    else {
        return;
    };
    // The WAL files first: if this is interrupted, the main file still has
    // its old name and the next launch finishes the job.
    for suffix in ["-wal", "-shm", ""] {
        let from = dir.join(format!("{old}{suffix}"));
        let to = dir.join(format!("{DATABASE}{suffix}"));
        if from.exists()
            && !to.exists()
            && let Err(err) = std::fs::rename(&from, &to)
        {
            eprintln!("ottero: couldn't rename the old database file: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_with(path: &Path, files: &[&str]) {
        std::fs::create_dir_all(path).expect("mkdir");
        for f in files {
            std::fs::write(path.join(f), f).expect("write");
        }
    }

    fn read(path: PathBuf) -> String {
        std::fs::read_to_string(path).expect("read")
    }

    #[test]
    fn calmfold_data_moves_over_once() {
        let root = tempfile::tempdir().expect("dir");
        let calmfold = root.path().join("com.Nexdark.Calmfold");
        folder_with(
            &calmfold,
            &[
                "calmfold.sqlite3",
                "calmfold.sqlite3-wal",
                "calmfold.sqlite3-shm",
                "appearance.json",
            ],
        );
        let new = root.path().join("com.Nexdark.Ottero");
        let dir = adopt_legacy_dir(&new, std::slice::from_ref(&calmfold));
        assert_eq!(dir, new);
        assert!(!calmfold.exists());
        rename_legacy_database(&dir);
        assert_eq!(read(dir.join("ottero.sqlite3")), "calmfold.sqlite3");
        assert_eq!(read(dir.join("ottero.sqlite3-wal")), "calmfold.sqlite3-wal");
        assert_eq!(read(dir.join("ottero.sqlite3-shm")), "calmfold.sqlite3-shm");
        assert!(!dir.join("calmfold.sqlite3").exists());
        assert!(dir.join("appearance.json").exists());
        // Already moved: nothing happens again.
        assert_eq!(adopt_legacy_dir(&new, &[calmfold]), new);
        rename_legacy_database(&dir);
        assert_eq!(read(dir.join("ottero.sqlite3")), "calmfold.sqlite3");
    }

    #[test]
    fn keepstone_data_moves_over_once() {
        let root = tempfile::tempdir().expect("dir");
        let calmfold = root.path().join("com.Nexdark.Calmfold");
        let keepstone = root.path().join("app.Keepstone.Keepstone");
        folder_with(
            &keepstone,
            &[
                "keepstone.sqlite3",
                "keepstone.sqlite3-wal",
                "appearance.json",
            ],
        );
        let new = root.path().join("com.Nexdark.Ottero");
        let dir = adopt_legacy_dir(&new, &[calmfold, keepstone.clone()]);
        assert_eq!(dir, new);
        assert!(!keepstone.exists());
        rename_legacy_database(&dir);
        assert_eq!(read(dir.join("ottero.sqlite3")), "keepstone.sqlite3");
        assert!(dir.join("ottero.sqlite3-wal").exists());
        assert!(dir.join("appearance.json").exists());
        rename_legacy_database(&dir);
        assert!(dir.join("ottero.sqlite3").exists());
    }

    #[test]
    fn newest_old_folder_wins_and_existing_data_is_never_overwritten() {
        let root = tempfile::tempdir().expect("dir");
        let calmfold = root.path().join("com.Nexdark.Calmfold");
        let keepstone = root.path().join("app.Keepstone.Keepstone");
        folder_with(&calmfold, &["calmfold.sqlite3"]);
        folder_with(&keepstone, &["keepstone.sqlite3"]);
        let new = root.path().join("com.Nexdark.Ottero");
        let legacy = [calmfold.clone(), keepstone.clone()];
        assert_eq!(adopt_legacy_dir(&new, &legacy), new);
        assert!(!calmfold.exists());
        assert!(keepstone.exists(), "only the newest old folder moves");

        // An existing Ottero folder is used as is; old folders stay put.
        folder_with(&calmfold, &["calmfold.sqlite3"]);
        assert_eq!(adopt_legacy_dir(&new, &legacy), new);
        assert!(calmfold.exists());

        // An existing Ottero database wins over old ones in the same folder.
        let dir = root.path().join("mixed");
        folder_with(&dir, &["ottero.sqlite3", "calmfold.sqlite3"]);
        rename_legacy_database(&dir);
        assert_eq!(read(dir.join("ottero.sqlite3")), "ottero.sqlite3");
        assert!(dir.join("calmfold.sqlite3").exists());

        // A stray new WAL file is never replaced by an old one.
        let dir = root.path().join("stray");
        folder_with(
            &dir,
            &[
                "ottero.sqlite3-wal",
                "calmfold.sqlite3",
                "calmfold.sqlite3-wal",
            ],
        );
        rename_legacy_database(&dir);
        assert_eq!(read(dir.join("ottero.sqlite3")), "calmfold.sqlite3");
        assert_eq!(read(dir.join("ottero.sqlite3-wal")), "ottero.sqlite3-wal");
    }

    #[test]
    fn no_old_folder_uses_the_new_one() {
        let root = tempfile::tempdir().expect("dir");
        let new = root.path().join("com.Nexdark.Ottero");
        assert_eq!(adopt_legacy_dir(&new, &[root.path().join("missing")]), new);
    }
}
