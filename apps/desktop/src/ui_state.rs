//! Window layout state that survives restarts (sidebar width, collapsed).
//! Separate from appearance: this is "where things are", not "how they look".

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutState {
    /// Expanded sidebar width in logical pixels.
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
}

impl LayoutState {
    pub const MIN_SIDEBAR: f32 = 180.;
    pub const MAX_SIDEBAR: f32 = 360.;
    pub const DEFAULT_SIDEBAR: f32 = 220.;

    #[must_use]
    pub fn clamp_width(width: f32) -> f32 {
        if width.is_finite() {
            width.clamp(Self::MIN_SIDEBAR, Self::MAX_SIDEBAR)
        } else {
            Self::DEFAULT_SIDEBAR
        }
    }

    #[must_use]
    pub fn load(path: &Path) -> Self {
        let mut state: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        state.sidebar_width = Self::clamp_width(state.sidebar_width);
        state
    }

    /// Best effort, atomic (temp file + rename). Layout is not worth an error dialog.
    pub fn save(&self, path: &Path) {
        let tmp = path.with_extension("json.tmp");
        let ok = serde_json::to_vec_pretty(self)
            .ok()
            .and_then(|json| fs::write(&tmp, json).ok())
            .and_then(|()| fs::rename(&tmp, path).ok());
        if ok.is_none() {
            eprintln!("keepstone: could not save layout to {}", path.display());
        }
    }
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            sidebar_width: Self::DEFAULT_SIDEBAR,
            sidebar_collapsed: false,
        }
    }
}

/// Where layout is stored, next to the database.
#[must_use]
pub fn layout_file(data_dir: &Path) -> PathBuf {
    data_dir.join("layout.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = layout_file(dir.path());
        LayoutState {
            sidebar_width: 260.,
            sidebar_collapsed: true,
        }
        .save(&path);
        assert_eq!(
            LayoutState::load(&path),
            LayoutState {
                sidebar_width: 260.,
                sidebar_collapsed: true
            }
        );

        fs::write(&path, r#"{"sidebar_width": 9999}"#).expect("write");
        assert_eq!(
            LayoutState::load(&path).sidebar_width,
            LayoutState::MAX_SIDEBAR
        );
        assert_eq!(
            LayoutState::clamp_width(f32::NAN),
            LayoutState::DEFAULT_SIDEBAR
        );
    }

    #[test]
    fn missing_or_corrupt_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = layout_file(dir.path());
        assert_eq!(LayoutState::load(&path), LayoutState::default());
        fs::write(&path, "nope").expect("write");
        assert_eq!(LayoutState::load(&path), LayoutState::default());
    }
}
