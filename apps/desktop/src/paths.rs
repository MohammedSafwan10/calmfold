//! Where Keepstone keeps its files.
//!
//! * `KEEPSTONE_DATA_DIR` overrides everything (tests, portable installs).
//! * Debug builds use a separate "Keepstone Dev" folder so development never
//!   touches real data.
//! * Release builds use the platform data folder:
//!   macOS `~/Library/Application Support/app.Keepstone.Keepstone`,
//!   Windows `%APPDATA%\Keepstone\Keepstone\data`.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use directories::ProjectDirs;

pub const DATA_DIR_ENV: &str = "KEEPSTONE_DATA_DIR";

#[derive(Debug, Clone)]
pub struct AppPaths {
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
                    "Keepstone Dev"
                } else {
                    "Keepstone"
                };
                ProjectDirs::from("app", "Keepstone", app)
                    .context("could not find a home folder for app data")?
                    .data_dir()
                    .to_path_buf()
            }
        };
        Ok(Self::in_dir(data_dir))
    }

    #[must_use]
    pub fn in_dir(data_dir: PathBuf) -> Self {
        Self {
            database: data_dir.join("keepstone.sqlite3"),
            appearance: data_dir.join("appearance.json"),
            data_dir,
        }
    }
}
