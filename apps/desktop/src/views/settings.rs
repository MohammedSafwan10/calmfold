//! Settings screen, built on GPUI Kit's searchable `Settings` component.
//!
//! Pages: Appearance, General, Keyboard, About. Add a feature's settings as a
//! new `SettingPage` (or group) here; the search box covers every page.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::setting::{
    RenderOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, Keystroke,
    ParentElement as _, Render, SharedString, Styled as _, Window, div,
};
use keepstone_ui::appearance::{
    self, AppearanceSettings, ContentWidth, MonoFont, MotionPreference, ThemePreference, UiFont,
};
use keepstone_ui::components::segmented;

use crate::actions::{EDITING_SHORTCUTS, SHORTCUTS, ShortcutGroup};

pub struct SettingsView {
    data_dir: PathBuf,
    focus: FocusHandle,
}

impl SettingsView {
    pub fn new(data_dir: PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            data_dir,
            focus: cx.focus_handle(),
        }
    }

    fn appearance_page() -> SettingPage {
        let theme = SettingItem::new(
            "Appearance",
            SettingField::render(|_, _, cx| {
                segmented(
                    "set-theme",
                    &[
                        (ThemePreference::System, "System"),
                        (ThemePreference::Light, "Light"),
                        (ThemePreference::Dark, "Dark"),
                    ],
                    appearance::settings(cx).theme,
                    |v, _, cx| appearance::update(cx, |s| s.theme = v),
                )
            }),
        )
        .description("System follows your computer's light or dark setting.")
        .keywords(["theme", "dark mode", "light mode", "color"]);

        let width = SettingItem::new(
            "Content width",
            SettingField::render(|_, _, cx| {
                segmented(
                    "set-width",
                    &[
                        (ContentWidth::Compact, "Compact"),
                        (ContentWidth::Comfortable, "Comfortable"),
                        (ContentWidth::Wide, "Wide"),
                    ],
                    appearance::settings(cx).content_width,
                    |v, _, cx| appearance::update(cx, |s| s.content_width = v),
                )
            }),
        )
        .description("How wide notes and lists can grow on large screens.")
        .keywords(["layout", "width", "density"]);

        let motion = SettingItem::new(
            "Reduce motion",
            SettingField::render(|_, _, cx| {
                segmented(
                    "set-motion",
                    &[
                        (MotionPreference::System, "System"),
                        (MotionPreference::On, "On"),
                        (MotionPreference::Off, "Off"),
                    ],
                    appearance::settings(cx).reduce_motion,
                    |v, _, cx| appearance::update(cx, |s| s.reduce_motion = v),
                )
            }),
        )
        .description(
            "Turns off menu and dialog animations. System follows your accessibility setting.",
        )
        .keywords(["animation", "accessibility"]);

        let ui_font = SettingItem::new(
            "Interface font",
            SettingField::render(|_, _, cx| {
                h_flex()
                    .gap_2()
                    .child(segmented(
                        "set-ui-font",
                        &[(UiFont::Inter, "Inter"), (UiFont::System, "System")],
                        appearance::settings(cx).ui_font,
                        |v, _, cx| appearance::update(cx, |s| s.ui_font = v),
                    ))
                    .child(size_stepper("ui-size", appearance::settings(cx).ui_font_size, AppearanceSettings::UI_FONT_SIZES, |s, v| s.ui_font_size = v))
            }),
        )
        .description(format!(
            "Everything outside code. The size also scales spacing, so it works as interface zoom. Default {} px.",
            AppearanceSettings::DEFAULT_UI_FONT_SIZE
        ))
        .keywords(["typography", "zoom", "text size", "font size"]);

        let mono_font = SettingItem::new(
            "Code font",
            SettingField::render(|_, _, cx| {
                h_flex()
                    .gap_2()
                    .child(segmented(
                        "set-mono-font",
                        &[
                            (MonoFont::JetbrainsMono, "JetBrains Mono"),
                            (MonoFont::System, "System"),
                        ],
                        appearance::settings(cx).mono_font,
                        |v, _, cx| appearance::update(cx, |s| s.mono_font = v),
                    ))
                    .child(size_stepper(
                        "mono-size",
                        appearance::settings(cx).mono_font_size,
                        AppearanceSettings::MONO_FONT_SIZES,
                        |s, v| s.mono_font_size = v,
                    ))
            }),
        )
        .description("Code blocks and inline code in notes.")
        .keywords(["typography", "monospace"]);

        let preview = SettingItem::render(|_, _, cx| {
            let theme = cx.theme();
            v_flex()
                .w_full()
                .gap_2()
                .p_3()
                .rounded(theme.radius)
                .bg(theme.background)
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_sm()
                        .child("The quick brown fox jumps over the lazy dog."),
                )
                .child(
                    div()
                        .font_family(theme.mono_font_family.clone())
                        .text_size(theme.mono_font_size)
                        .text_color(theme.muted_foreground)
                        .child("fn main() { println!(\"0O 1lI\"); }"),
                )
                .into_any_element()
        })
        .keywords(["preview", "typography"]);

        let spelling = SettingItem::new(
            "Check spelling",
            SettingField::switch(
                |cx| appearance::settings(cx).spell_check,
                |on, cx| appearance::update(cx, |s| s.spell_check = on),
            ),
        )
        .description(
            "Underlines misspelled words in notes. Right-click one for suggestions. Code, links and tags are skipped.",
        )
        .keywords(["spelling", "spellcheck", "typo", "dictionary"]);

        let reset = SettingItem::render(|_, _, _| {
            h_flex()
                .w_full()
                .justify_start()
                .child(
                    Button::new("appearance-reset")
                        .ghost()
                        .small()
                        .label("Restore default appearance")
                        .on_click(|_, _, cx| appearance::reset(cx)),
                )
                .into_any_element()
        })
        .keywords(["reset", "default"]);

        SettingPage::new("Appearance")
            .icon(Lucide::Palette)
            .default_open(true)
            .group(
                SettingGroup::new()
                    .title("Theme")
                    .items(vec![theme, width, motion]),
            )
            .group(
                SettingGroup::new()
                    .title("Typography")
                    .items(vec![ui_font, mono_font, preview]),
            )
            .group(SettingGroup::new().title("Writing").items(vec![spelling]))
            .group(
                SettingGroup::new()
                    .variant(GroupBoxVariant::Normal)
                    .item(reset),
            )
    }

    fn general_page(data_dir: PathBuf) -> SettingPage {
        let shown = SharedString::from(data_dir.display().to_string());
        let location = SettingItem::new(
            "Data folder",
            SettingField::render(move |_, _, _| {
                let dir = data_dir.clone();
                Button::new("reveal-data")
                    .outline()
                    .small()
                    .label(if cfg!(target_os = "macos") {
                        "Show in Finder"
                    } else {
                        "Open folder"
                    })
                    .on_click(move |_, _, cx| cx.reveal_path(&dir))
            }),
        )
        .description(shown)
        .keywords(["storage", "database", "backup", "location"]);

        SettingPage::new("General").icon(Lucide::HardDrive).group(
            SettingGroup::new()
                .title("Storage")
                .item(location)
                .footer(|_, _| "Everything stays on this device. Sync is not set up yet."),
        )
    }

    fn keyboard_page() -> SettingPage {
        let key_row = |keys: &'static str, label: &'static str| {
            SettingItem::new(
                label,
                SettingField::render(move |_: &RenderOptions, _: &mut Window, _: &mut App| {
                    match Keystroke::parse(keys) {
                        // Kbd draws the platform's symbols: ⌘⇧N on macOS, Ctrl+Shift+N on Windows.
                        Ok(ks) => Kbd::new(ks).into_any_element(),
                        Err(_) => div().child(keys).into_any_element(),
                    }
                }),
            )
            .keywords(["shortcut", "hotkey", "keyboard"])
        };

        let mut groups: Vec<SettingGroup> = ShortcutGroup::ALL
            .into_iter()
            .map(|group| {
                let items = SHORTCUTS
                    .iter()
                    .filter(|s| s.group == group)
                    .map(|s| key_row(s.keys, s.label))
                    .collect::<Vec<_>>();
                SettingGroup::new().title(group.title()).items(items)
            })
            .collect();

        let link = SettingItem::new(
            "Link to another note",
            SettingField::render(|_: &RenderOptions, _: &mut Window, cx: &mut App| {
                div()
                    .px_1p5()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_sm()
                    .child("[[")
            }),
        )
        .description("Type it in a note, then pick a title.")
        .keywords(["wiki", "link", "autocomplete"]);
        let mut editing = EDITING_SHORTCUTS
            .iter()
            .map(|&(keys, label)| key_row(keys, label))
            .collect::<Vec<_>>();
        editing.push(link);
        groups.push(
            SettingGroup::new()
                .title("Writing")
                .items(editing)
                .footer(|_, _| {
                    if cfg!(target_os = "macos") {
                        "On Windows and Linux, Ctrl takes the place of ⌘."
                    } else {
                        "On macOS, ⌘ takes the place of Ctrl."
                    }
                }),
        );

        SettingPage::new("Keyboard")
            .icon(Lucide::Keyboard)
            .groups(groups)
    }

    fn about_page() -> SettingPage {
        let version = SettingItem::new(
            "Version",
            SettingField::render(|_, _, _| div().text_sm().child(env!("CARGO_PKG_VERSION"))),
        );
        let fonts = SettingItem::new(
            "Fonts",
            SettingField::render(|_, _, _| {
                div().text_sm().child("Inter, JetBrains Mono · SIL OFL 1.1")
            }),
        );
        let icons = SettingItem::new(
            "Icons",
            SettingField::render(|_, _, _| div().text_sm().child("Lucide · ISC")),
        );
        SettingPage::new("About").icon(Lucide::Info).group(
            SettingGroup::new()
                .title("Keepstone")
                .items(vec![version, fonts, icons]),
        )
    }
}

/// A compact "− 15 px +" control for font sizes.
fn size_stepper(
    id: &'static str,
    value: u8,
    range: std::ops::RangeInclusive<u8>,
    set: fn(&mut AppearanceSettings, u8),
) -> impl IntoElement {
    let (min, max) = (*range.start(), *range.end());
    h_flex()
        .gap_1()
        .child(
            Button::new(SharedString::from(format!("{id}-dec")))
                .outline()
                .xsmall()
                .icon(Lucide::Minus)
                .disabled(value <= min)
                .tooltip("Smaller")
                .on_click(move |_, _, cx| {
                    appearance::update(cx, |s| set(s, value.saturating_sub(1).max(min)))
                }),
        )
        .child(
            div()
                .w_12()
                .text_center()
                .text_sm()
                .child(format!("{value} px")),
        )
        .child(
            Button::new(SharedString::from(format!("{id}-inc")))
                .outline()
                .xsmall()
                .icon(Lucide::Plus)
                .disabled(value >= max)
                .tooltip("Larger")
                .on_click(move |_, _, cx| {
                    appearance::update(cx, |s| set(s, value.saturating_add(1).min(max)))
                }),
        )
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full().child(
            Settings::new("keepstone-settings")
                .with_group_variant(GroupBoxVariant::Outline)
                .pages(vec![
                    Self::appearance_page(),
                    Self::general_page(self.data_dir.clone()),
                    Self::keyboard_page(),
                    Self::about_page(),
                ]),
        )
    }
}
