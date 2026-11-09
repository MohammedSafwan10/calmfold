//! The themes Calmfold ships and how a light/dark pair is installed.
//!
//! `calmfold.json` holds the house themes (Calmfold Light/Dark). The
//! community sets under `themes/community/` are upstream GPUI Kit themes,
//! copied unchanged. The user picks one light and one dark theme; the mode
//! (System/Light/Dark) chooses which of the two is shown.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, SharedString};

pub const LIGHT: &str = "Calmfold Light";
pub const DARK: &str = "Calmfold Dark";

/// Every bundled theme file, house themes first.
const THEME_FILES: &[&str] = &[
    include_str!("../themes/calmfold.json"),
    include_str!("../themes/community/catppuccin.json"),
    include_str!("../themes/community/tokyonight.json"),
    include_str!("../themes/community/gruvbox.json"),
    include_str!("../themes/community/solarized.json"),
    include_str!("../themes/community/everforest.json"),
    include_str!("../themes/community/flexoki.json"),
    include_str!("../themes/community/ayu.json"),
];

/// A theme the user can pick, for the settings cards.
#[derive(Debug, Clone)]
pub struct ThemeChoice {
    pub name: SharedString,
    pub mode: ThemeMode,
    pub config: Rc<ThemeConfig>,
}

/// Loads every bundled theme into GPUI Kit's registry. Call once at startup.
pub fn register(cx: &mut App) {
    for file in THEME_FILES {
        if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(file) {
            eprintln!("calmfold: failed to parse a bundled theme file: {err}");
        }
    }
}

/// The bundled themes for one mode: house theme first, then by name.
#[must_use]
pub fn choices(mode: ThemeMode, cx: &App) -> Vec<ThemeChoice> {
    let mut out: Vec<ThemeChoice> = ThemeRegistry::global(cx)
        .themes()
        .values()
        .filter(|c| c.mode == mode)
        // GPUI Kit's own fallback themes aren't offered: they duplicate ours.
        .filter(|c| !c.name.starts_with("Default "))
        .map(|c| ThemeChoice {
            name: c.name.clone(),
            mode: c.mode,
            config: c.clone(),
        })
        .collect();
    out.sort_by_key(|c| (!c.name.starts_with("Calmfold"), c.name.to_string()));
    out
}

fn find(name: &str, fallback: &str, cx: &App) -> Option<Rc<ThemeConfig>> {
    let themes = ThemeRegistry::global(cx).themes();
    themes
        .get(&SharedString::from(name.to_owned()))
        .or_else(|| themes.get(&SharedString::from(fallback.to_owned())))
        .cloned()
}

/// Installs the chosen light and dark themes (unknown names fall back to the
/// Calmfold pair) and shows `mode`.
pub fn install(light: &str, dark: &str, mode: ThemeMode, cx: &mut App) {
    let (Some(light), Some(dark)) = (find(light, LIGHT, cx), find(dark, DARK, cx)) else {
        eprintln!("calmfold: bundled themes are missing");
        return;
    };
    let current = Theme::global(cx);
    let unchanged = current.mode == mode
        && current.light_theme.name == light.name
        && current.dark_theme.name == dark.name;
    if unchanged {
        return;
    }
    Theme::update(cx, |theme| {
        theme.apply_config(&light);
        theme.apply_config(&dark);
    });
    // apply_config leaves the mode at the last config applied; show the right one.
    Theme::change(mode, None, cx);
}

/// Resolves the effective light/dark mode for a preference.
#[must_use]
pub fn resolve_mode(
    preference: crate::appearance::ThemePreference,
    system_dark: bool,
) -> ThemeMode {
    use crate::appearance::ThemePreference;
    match preference {
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
        ThemePreference::System if system_dark => ThemeMode::Dark,
        ThemePreference::System => ThemeMode::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::ThemeSet;

    fn all() -> Vec<ThemeConfig> {
        THEME_FILES
            .iter()
            .flat_map(|f| {
                serde_json::from_str::<ThemeSet>(f)
                    .expect("theme json parses")
                    .themes
            })
            .collect()
    }

    #[test]
    fn every_bundled_theme_parses_and_both_modes_have_choices() {
        let themes = all();
        let names: Vec<_> = themes.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&LIGHT.to_owned()));
        assert!(names.contains(&DARK.to_owned()));
        assert!(
            themes.iter().filter(|t| t.mode.is_dark()).count() >= 6,
            "{names:?}"
        );
        assert!(
            themes.iter().filter(|t| !t.mode.is_dark()).count() >= 6,
            "{names:?}"
        );
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "theme names are unique");
    }

    #[test]
    fn every_theme_colors_code_blocks() {
        for t in all() {
            assert!(t.highlight.is_some(), "{} defines syntax colors", t.name);
        }
    }
}
