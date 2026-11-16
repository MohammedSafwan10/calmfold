//! Window layout that survives restarts: the main sidebar and every
//! `SidePanel` (width + collapsed). One `Entity<LayoutStore>` is shared by all
//! views so only one writer touches `layout.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Context, Task};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    pub width: f32,
    pub collapsed: bool,
}

/// Size limits and default for one panel.
#[derive(Debug, Clone, Copy)]
pub struct PanelSpec {
    pub id: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl PanelSpec {
    #[must_use]
    pub fn clamp(&self, width: f32) -> f32 {
        if width.is_finite() {
            width.clamp(self.min, self.max)
        } else {
            self.default
        }
    }
}

/// The notes list next to the editor.
pub const NOTES_LIST: PanelSpec = PanelSpec {
    id: "notes-list",
    min: 220.,
    max: 460.,
    default: 288.,
};

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            width: f32::NAN,
            collapsed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutState {
    pub navigation: Option<crate::navigation::SidebarPreferences>,
    /// Expanded sidebar width in logical pixels.
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    /// Side panels by id (see [`PanelSpec`]).
    pub panels: BTreeMap<String, PanelLayout>,
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

    /// A panel's layout with its width clamped to the spec (defaults if unknown).
    #[must_use]
    pub fn panel(&self, spec: PanelSpec) -> PanelLayout {
        let stored = self.panels.get(spec.id).copied().unwrap_or_default();
        PanelLayout {
            width: spec.clamp(stored.width),
            collapsed: stored.collapsed,
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
            eprintln!("calmfold: could not save layout to {}", path.display());
        }
    }
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            navigation: None,
            sidebar_width: Self::DEFAULT_SIDEBAR,
            sidebar_collapsed: false,
            panels: BTreeMap::new(),
        }
    }
}

/// Where layout is stored, next to the database.
#[must_use]
pub fn layout_file(data_dir: &Path) -> PathBuf {
    data_dir.join("layout.json")
}

/// Run before first-launch appearance defaults are persisted. Legacy users keep
/// their sidebar; only genuinely fresh preference folders get the quiet preset.
pub fn initialize_navigation(data_dir: &Path, developer: bool, existing_database: bool) {
    let file = layout_file(data_dir);
    let existing = existing_database || file.exists() || data_dir.join("appearance.json").exists();
    let mut state = LayoutState::load(&file);
    if state.navigation.is_none() {
        state.navigation = Some(if existing {
            crate::navigation::SidebarPreferences::legacy(developer)
        } else {
            crate::navigation::SidebarPreferences::default()
        });
        state.save(&file);
    }
}

/// Shared layout owner. Views read it in `render` and call its setters; it
/// notifies observers and saves 300 ms after the last change (one write per drag).
pub struct LayoutStore {
    state: LayoutState,
    file: PathBuf,
    /// Collapse/expand count per panel id this session; drives one animation per toggle.
    toggles: BTreeMap<String, u32>,
    pending_save: Option<Task<()>>,
}

impl LayoutStore {
    #[must_use]
    pub fn load(data_dir: &Path) -> Self {
        let file = layout_file(data_dir);
        Self {
            state: LayoutState::load(&file),
            file,
            toggles: BTreeMap::new(),
            pending_save: None,
        }
    }

    #[must_use]
    pub fn state(&self) -> &LayoutState {
        &self.state
    }

    pub fn navigation(&self) -> crate::navigation::SidebarPreferences {
        self.state.navigation.clone().unwrap_or_default()
    }
    pub fn edit_navigation(
        &mut self,
        edit: impl FnOnce(&mut crate::navigation::SidebarPreferences),
        cx: &mut Context<Self>,
    ) {
        let mut next = self.navigation();
        edit(&mut next);
        if self.state.navigation.as_ref() != Some(&next) {
            self.state.navigation = Some(next);
            self.changed(cx);
        }
    }

    #[must_use]
    pub fn panel(&self, spec: PanelSpec) -> PanelLayout {
        self.state.panel(spec)
    }

    #[must_use]
    pub fn panel_toggles(&self, spec: PanelSpec) -> u32 {
        self.toggles.get(spec.id).copied().unwrap_or(0)
    }

    pub fn set_panel_width(&mut self, spec: PanelSpec, width: f32, cx: &mut Context<Self>) {
        let width = spec.clamp(width);
        let mut panel = self.panel(spec);
        if (panel.width - width).abs() >= 0.5 {
            panel.width = width;
            self.state.panels.insert(spec.id.to_owned(), panel);
            self.changed(cx);
        }
    }

    pub fn toggle_panel(&mut self, spec: PanelSpec, cx: &mut Context<Self>) {
        let mut panel = self.panel(spec);
        panel.collapsed = !panel.collapsed;
        self.state.panels.insert(spec.id.to_owned(), panel);
        *self.toggles.entry(spec.id.to_owned()).or_default() += 1;
        self.changed(cx);
    }

    pub fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = LayoutState::clamp_width(width);
        if (width - self.state.sidebar_width).abs() >= 0.5 {
            self.state.sidebar_width = width;
            self.changed(cx);
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.state.sidebar_collapsed = !self.state.sidebar_collapsed;
        self.changed(cx);
    }

    /// Writes now (used on quit and window close).
    pub fn save_now(&mut self) {
        self.pending_save = None;
        self.state.save(&self.file);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        self.pending_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            _ = this.update(cx, |this, _| this.save_now());
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_preferences_migrate_once_preserve_layout_and_persist() {
        let fresh = tempfile::tempdir().expect("dir");
        initialize_navigation(fresh.path(), true, false);
        assert_eq!(
            LayoutState::load(&layout_file(fresh.path()))
                .navigation
                .expect("prefs")
                .visible,
            ["today", "notes", "tasks", "calendar"]
        );
        let old = tempfile::tempdir().expect("dir");
        let path = layout_file(old.path());
        fs::write(&path, r#"{"sidebar_width":280,"sidebar_collapsed":true}"#).expect("old layout");
        initialize_navigation(old.path(), false, true);
        let mut saved = LayoutState::load(&path);
        assert_eq!(saved.sidebar_width, 280.);
        assert!(saved.sidebar_collapsed);
        let prefs = saved.navigation.as_mut().expect("prefs");
        assert!(!prefs.is_visible("developer"));
        assert!(prefs.is_visible("focus"));
        prefs.set_visible("notes", false);
        prefs.move_to("passwords", 0);
        prefs.start_page = "notes".into();
        saved.save(&path);
        initialize_navigation(old.path(), true, true);
        assert_eq!(
            LayoutState::load(&path),
            saved,
            "migration never overwrites an existing customization"
        );
        let only_db = tempfile::tempdir().expect("dir");
        initialize_navigation(only_db.path(), true, true);
        assert!(
            LayoutState::load(&layout_file(only_db.path()))
                .navigation
                .expect("prefs")
                .is_visible("developer"),
            "existing databases without preferences keep prior entries"
        );
    }

    #[test]
    fn round_trips_and_clamps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = layout_file(dir.path());
        let mut state = LayoutState {
            sidebar_width: 260.,
            sidebar_collapsed: true,
            ..Default::default()
        };
        state.panels.insert(
            NOTES_LIST.id.into(),
            PanelLayout {
                width: 300.,
                collapsed: true,
            },
        );
        state.save(&path);
        assert_eq!(LayoutState::load(&path), state);

        fs::write(
            &path,
            r#"{"sidebar_width": 9999, "panels": {"notes-list": {"width": 5}}}"#,
        )
        .expect("write");
        let loaded = LayoutState::load(&path);
        assert_eq!(loaded.sidebar_width, LayoutState::MAX_SIDEBAR);
        assert_eq!(loaded.panel(NOTES_LIST).width, NOTES_LIST.min);
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
        assert_eq!(
            LayoutState::default().panel(NOTES_LIST).width,
            NOTES_LIST.default
        );
        fs::write(&path, "nope").expect("write");
        assert_eq!(LayoutState::load(&path), LayoutState::default());
    }
}
