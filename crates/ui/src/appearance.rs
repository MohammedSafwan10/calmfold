//! User appearance preferences: persisted as JSON, applied to the GPUI Kit theme.
//!
//! Flow: [`init`] loads the file (or defaults) at startup → [`update`] edits,
//! sanitizes, saves, then re-applies → [`system_appearance_changed`] re-applies
//! when the OS switches light/dark and the user chose "System".

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, Window, WindowAppearance, px};
use serde::{Deserialize, Serialize};

use crate::fonts;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiFont {
    #[default]
    Inter,
    System,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonoFont {
    #[default]
    JetbrainsMono,
    System,
}

/// Space between lines in notes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineSpacing {
    Compact,
    #[default]
    Comfortable,
    Relaxed,
}

impl LineSpacing {
    /// Line height as a multiple of the font size.
    #[must_use]
    pub fn ratio(self) -> f32 {
        match self {
            Self::Compact => 1.35,
            Self::Comfortable => 1.5,
            Self::Relaxed => 1.7,
        }
    }
}

/// How wide documents (notes, task details) may grow on large windows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentWidth {
    Compact,
    #[default]
    Comfortable,
    Wide,
}

impl ContentWidth {
    /// Maximum width in rems, so it scales with interface size.
    #[must_use]
    pub fn max_width_rems(self) -> f32 {
        match self {
            Self::Compact => 38.,
            Self::Comfortable => 48.,
            Self::Wide => 72.,
        }
    }
}

/// First column of calendar weeks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}

/// How times of day are written ("2:30 PM" or "14:30").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockFormat {
    #[default]
    TwelveHour,
    TwentyFourHour,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionPreference {
    #[default]
    System,
    On,
    Off,
}

/// Saved appearance. Unknown or missing fields fall back to defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: ThemePreference,
    /// Theme shown in light mode (a bundled theme name).
    pub light_theme: String,
    /// Theme shown in dark mode.
    pub dark_theme: String,
    pub ui_font: UiFont,
    pub ui_font_size: u8,
    pub mono_font: MonoFont,
    pub mono_font_size: u8,
    pub content_width: ContentWidth,
    /// `On` means "reduce motion is on".
    pub reduce_motion: MotionPreference,
    /// Underline misspelled words in notes (the OS spell checker).
    pub spell_check: bool,
    pub line_spacing: LineSpacing,
    pub week_start: WeekStart,
    pub clock: ClockFormat,
    /// Show routines (events repeating 3+ times a week) in the calendar.
    pub calendar_routines: bool,
    /// Show public holidays in the calendar and on Today.
    pub holidays: bool,
    /// Also show festivals and observances (days that aren't days off).
    pub holiday_observances: bool,
    /// Countries whose holidays are shown (ISO codes). `None` until first
    /// launch fills in the country of the Mac's time zone.
    pub holiday_countries: Option<Vec<String>>,
    /// Also show the holidays of the country you're in (from the time
    /// zone) when it isn't one of `holiday_countries`.
    pub holidays_where_you_are: bool,
    /// Today dashboard: section keys in display order (unknown keys are
    /// ignored, missing ones appended) and the keys the user hid.
    pub today_order: Vec<String>,
    pub today_hidden: Vec<String>,
}

impl AppearanceSettings {
    pub const UI_FONT_SIZES: std::ops::RangeInclusive<u8> = 12..=20;
    pub const MONO_FONT_SIZES: std::ops::RangeInclusive<u8> = 10..=20;
    pub const DEFAULT_UI_FONT_SIZE: u8 = 15;
    pub const DEFAULT_MONO_FONT_SIZE: u8 = 13;

    /// Clamps values a hand-edited file could put out of range.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.ui_font_size = self
            .ui_font_size
            .clamp(*Self::UI_FONT_SIZES.start(), *Self::UI_FONT_SIZES.end());
        self.mono_font_size = self
            .mono_font_size
            .clamp(*Self::MONO_FONT_SIZES.start(), *Self::MONO_FONT_SIZES.end());
        self
    }

    /// Reads settings. A missing or unreadable file yields defaults; this never fails.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<Self>(&text)
                .map(Self::sanitized)
                .unwrap_or_else(|err| {
                    eprintln!("keepstone: ignoring invalid {}: {err}", path.display());
                    Self::default()
                }),
            Err(_) => Self::default(),
        }
    }

    /// Writes atomically: a crash mid-write leaves the previous file intact.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
            file.write_all(&json)?;
            file.sync_all()?;
        }
        fs::rename(tmp, path)
    }

    #[must_use]
    pub fn ui_font_family(&self) -> &'static str {
        match self.ui_font {
            UiFont::Inter => fonts::INTER,
            UiFont::System => fonts::SYSTEM_UI,
        }
    }

    #[must_use]
    pub fn mono_font_family(&self) -> &'static str {
        match self.mono_font {
            MonoFont::JetbrainsMono => fonts::JETBRAINS_MONO,
            MonoFont::System => fonts::system_mono(),
        }
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            light_theme: crate::theme::LIGHT.to_owned(),
            dark_theme: crate::theme::DARK.to_owned(),
            ui_font: UiFont::default(),
            ui_font_size: Self::DEFAULT_UI_FONT_SIZE,
            mono_font: MonoFont::default(),
            mono_font_size: Self::DEFAULT_MONO_FONT_SIZE,
            content_width: ContentWidth::default(),
            reduce_motion: MotionPreference::default(),
            spell_check: true,
            line_spacing: LineSpacing::default(),
            week_start: WeekStart::default(),
            clock: ClockFormat::default(),
            calendar_routines: true,
            holidays: true,
            holiday_observances: true,
            holiday_countries: None,
            holidays_where_you_are: true,
            today_order: Vec::new(),
            today_hidden: Vec::new(),
        }
    }
}

struct AppearanceState {
    settings: AppearanceSettings,
    file: PathBuf,
    /// The OS reduce-motion preference as read at startup, before any override.
    system_reduce_motion: bool,
}

impl Global for AppearanceState {}

pub(crate) fn init(file: PathBuf, cx: &mut App) {
    let settings = AppearanceSettings::load(&file);
    let system_reduce_motion = cx.reduce_motion();
    cx.set_global(AppearanceState {
        settings,
        file,
        system_reduce_motion,
    });
    apply(None, cx);
}

/// Current settings.
#[must_use]
pub fn settings(cx: &App) -> &AppearanceSettings {
    &cx.global::<AppearanceState>().settings
}

/// Edits settings, saves them, and applies them to every window.
pub fn update(cx: &mut App, edit: impl FnOnce(&mut AppearanceSettings)) {
    let state = cx.global_mut::<AppearanceState>();
    let mut next = state.settings.clone();
    edit(&mut next);
    let next = next.sanitized();
    if next == state.settings {
        return;
    }
    state.settings = next;
    if let Err(err) = state.settings.save(&state.file) {
        eprintln!("keepstone: failed to save appearance: {err}");
    }
    apply(None, cx);
}

/// Restores every appearance setting to its default.
pub fn reset(cx: &mut App) {
    update(cx, |s| *s = AppearanceSettings::default());
}

/// Call from a window's appearance observer so "System" follows the OS live.
pub fn system_appearance_changed(window: &mut Window, cx: &mut App) {
    if settings(cx).theme == ThemePreference::System {
        apply(Some(window.appearance()), cx);
    }
}

fn apply(appearance: Option<WindowAppearance>, cx: &mut App) {
    let state = cx.global::<AppearanceState>();
    let settings = state.settings.clone();
    let system_reduce_motion = state.system_reduce_motion;

    let system_dark = matches!(
        appearance.unwrap_or_else(|| cx.window_appearance()),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    let mode: ThemeMode = crate::theme::resolve_mode(settings.theme, system_dark);
    crate::theme::install(&settings.light_theme, &settings.dark_theme, mode, cx);

    let ui_family = settings.ui_font_family();
    let ui_family = if fonts::is_available(ui_family, cx) {
        ui_family
    } else {
        fonts::SYSTEM_UI
    };
    let mono_family = settings.mono_font_family();
    Theme::update(cx, |theme| {
        theme.font_family = ui_family.into();
        theme.font_size = px(f32::from(settings.ui_font_size));
        theme.mono_font_family = mono_family.into();
        theme.mono_font_size = px(f32::from(settings.mono_font_size));
    });

    let reduce = match settings.reduce_motion {
        MotionPreference::System => system_reduce_motion,
        MotionPreference::On => true,
        MotionPreference::Off => false,
    };
    cx.set_reduce_motion(reduce);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            AppearanceSettings::load(&dir.path().join("nope.json")),
            AppearanceSettings::default()
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sub/appearance.json");
        let settings = AppearanceSettings {
            theme: ThemePreference::Dark,
            ui_font: UiFont::System,
            ui_font_size: 17,
            mono_font: MonoFont::System,
            mono_font_size: 14,
            content_width: ContentWidth::Wide,
            reduce_motion: MotionPreference::On,
            spell_check: false,
            light_theme: "Solarized Light".into(),
            dark_theme: "Catppuccin Mocha".into(),
            line_spacing: LineSpacing::Relaxed,
            week_start: WeekStart::Sunday,
            clock: ClockFormat::TwentyFourHour,
            calendar_routines: false,
            holidays: false,
            holiday_observances: false,
            holiday_countries: Some(vec!["IN".into(), "US".into()]),
            holidays_where_you_are: false,
            today_order: vec!["habits".into(), "agenda".into()],
            today_hidden: vec!["notes".into()],
        };
        settings.save(&path).expect("save");
        assert_eq!(AppearanceSettings::load(&path), settings);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file cleaned up"
        );
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("appearance.json");
        fs::write(&path, "{ not json").expect("write");
        assert_eq!(
            AppearanceSettings::load(&path),
            AppearanceSettings::default()
        );
    }

    #[test]
    fn partial_file_keeps_known_fields_and_clamps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("appearance.json");
        fs::write(
            &path,
            r#"{"theme":"light","ui_font_size":99,"future_field":true}"#,
        )
        .expect("write");
        let loaded = AppearanceSettings::load(&path);
        assert_eq!(loaded.theme, ThemePreference::Light);
        assert_eq!(
            loaded.ui_font_size,
            *AppearanceSettings::UI_FONT_SIZES.end()
        );
        assert_eq!(loaded.mono_font, MonoFont::JetbrainsMono);
    }
}
