//! Bundled fonts. Licenses: `assets/fonts/*/OFL.txt` (SIL Open Font License 1.1).

use std::borrow::Cow;

use gpui_kit::App;

/// Family name stored inside the Inter files.
pub const INTER: &str = "Inter";
/// Family name stored inside the JetBrains Mono files.
pub const JETBRAINS_MONO: &str = "JetBrains Mono";
/// GPUI's alias for the platform UI font (SF Pro on macOS, Segoe UI on Windows).
pub const SYSTEM_UI: &str = ".SystemUIFont";

static FONT_FILES: &[&[u8]] = &[
    include_bytes!("../assets/fonts/inter/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-Bold.ttf"),
    // Without real italics, *emphasis* falls back to upright Regular.
    include_bytes!("../assets/fonts/inter/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-MediumItalic.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-SemiBoldItalic.ttf"),
    include_bytes!("../assets/fonts/inter/Inter-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
];

/// Registers the bundled fonts. Must run before the first window opens.
/// A failure is logged, and the theme falls back to system fonts.
pub fn register(cx: &mut App) {
    let fonts = FONT_FILES
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("calmfold: failed to load bundled fonts: {err}");
    }
}

/// Whether GPUI can resolve `family` (bundled or installed).
#[must_use]
pub fn is_available(family: &str, cx: &App) -> bool {
    family == SYSTEM_UI
        || cx
            .text_system()
            .all_font_names()
            .iter()
            .any(|f| f == family)
}

/// Platform monospace fallback used when the user picks "System".
#[must_use]
pub fn system_mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}
