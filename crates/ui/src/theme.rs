//! Registers the Keepstone themes with GPUI Kit's theme registry.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, SharedString};

pub const LIGHT: &str = "Keepstone Light";
pub const DARK: &str = "Keepstone Dark";

const THEMES_JSON: &str = include_str!("../themes/keepstone.json");

/// Loads the Keepstone themes and installs them as the light and dark themes.
/// Does not choose the mode; [`crate::appearance`] does that.
pub fn register(cx: &mut App) {
    if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(THEMES_JSON) {
        eprintln!("keepstone: failed to parse bundled themes: {err}");
        return;
    }
    let themes = ThemeRegistry::global(cx).themes();
    let light = themes.get(&SharedString::from(LIGHT)).cloned();
    let dark = themes.get(&SharedString::from(DARK)).cloned();
    let (Some(light), Some(dark)) = (light, dark) else {
        eprintln!("keepstone: bundled themes are missing");
        return;
    };
    install(light, dark, cx);
}

fn install(
    light: Rc<gpui_kit::component::ThemeConfig>,
    dark: Rc<gpui_kit::component::ThemeConfig>,
    cx: &mut App,
) {
    let mode = Theme::global(cx).mode;
    Theme::update(cx, |theme| {
        theme.apply_config(&light);
        theme.apply_config(&dark);
    });
    // apply_config switched the mode to the last config applied; restore it.
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

    #[test]
    fn bundled_theme_file_parses_with_both_modes() {
        let set: gpui_kit::component::ThemeSet =
            serde_json::from_str(THEMES_JSON).expect("theme json parses");
        let names: Vec<_> = set.themes.iter().map(|t| t.name.to_string()).collect();
        assert!(names.contains(&LIGHT.to_owned()));
        assert!(names.contains(&DARK.to_owned()));
        assert!(set.themes.iter().any(|t| t.mode.is_dark()));
        assert!(set.themes.iter().any(|t| !t.mode.is_dark()));
    }
}
