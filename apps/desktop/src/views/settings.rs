//! Settings screen, built on GPUI Kit's searchable `Settings` component.
//!
//! Pages: Appearance, General, Calendar, Keyboard, About. Add a feature's settings as a
//! new `SettingPage` (or group) here; the search box covers every page.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::ThemeMode;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::setting::{
    RenderOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, FocusHandle, Focusable, InteractiveElement as _, IntoElement, Keystroke,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    TestSupportExt as _, Window, div,
};
use keepstone_ui::appearance::{
    self, AppearanceSettings, ClockFormat, ContentWidth, LineSpacing, MonoFont, MotionPreference,
    ThemePreference, UiFont, WeekStart,
};
use keepstone_ui::components::segmented;

use crate::actions::{
    CALENDAR_SHORTCUTS, EDITING_SHORTCUTS, HABIT_SHORTCUTS, SHORTCUTS, ShortcutGroup,
};

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
        let mode = SettingItem::new(
            "Mode",
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
        .keywords(["theme", "dark mode", "light mode", "appearance"]);

        let light =
            SettingItem::render(|_, _, cx| theme_cards(ThemeMode::Light, cx).into_any_element())
                .keywords([
                    "theme",
                    "light",
                    "colors",
                    "catppuccin",
                    "solarized",
                    "gruvbox",
                    "flexoki",
                    "ayu",
                    "everforest",
                ]);
        let dark =
            SettingItem::render(|_, _, cx| theme_cards(ThemeMode::Dark, cx).into_any_element())
                .keywords([
                    "theme",
                    "dark",
                    "colors",
                    "catppuccin",
                    "tokyo night",
                    "solarized",
                    "gruvbox",
                    "flexoki",
                    "ayu",
                    "everforest",
                ]);

        let ui_font = SettingItem::new(
            "Interface",
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
            "Font for everything outside code. The size also scales spacing, so it works as zoom. Default {} px.",
            AppearanceSettings::DEFAULT_UI_FONT_SIZE
        ))
        .keywords(["typography", "font", "zoom", "text size", "font size"]);

        let mono_font = SettingItem::new(
            "Code",
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
        .keywords(["typography", "monospace", "font"]);

        let spacing = SettingItem::new(
            "Line spacing",
            SettingField::render(|_, _, cx| {
                segmented(
                    "set-line-spacing",
                    &[
                        (LineSpacing::Compact, "Compact"),
                        (LineSpacing::Comfortable, "Comfortable"),
                        (LineSpacing::Relaxed, "Relaxed"),
                    ],
                    appearance::settings(cx).line_spacing,
                    |v, _, cx| appearance::update(cx, |s| s.line_spacing = v),
                )
            }),
        )
        .description("Space between lines when writing notes.")
        .keywords(["typography", "line height", "leading", "density"]);

        let preview = SettingItem::render(|_, _, cx| {
            let theme = cx.theme();
            let ratio = appearance::settings(cx).line_spacing.ratio();
            v_flex()
                .w_full()
                .gap_2()
                .p_4()
                .rounded(theme.radius)
                .bg(theme.background)
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .child("Weekend plans"),
                )
                .child(
                    div()
                        .line_height(gpui_kit::relative(ratio))
                        .child("Pack the tent and check the weather on Friday. The quick brown fox jumps over the lazy dog, and the line keeps going so you can see how it wraps."),
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

        // Calm order: how it looks (theme), how text reads, how it's laid out,
        // then the two behaviour switches.
        SettingPage::new("Appearance")
            .icon(Lucide::Palette)
            .default_open(true)
            .group(SettingGroup::new().title("Theme").items(vec![mode]))
            .group(SettingGroup::new().title("Light theme").items(vec![light]))
            .group(SettingGroup::new().title("Dark theme").items(vec![dark]))
            .group(
                SettingGroup::new()
                    .title("Text")
                    .items(vec![ui_font, mono_font, spacing, preview]),
            )
            .group(SettingGroup::new().title("Layout").items(vec![width]))
            .group(
                SettingGroup::new()
                    .title("Writing and motion")
                    .items(vec![spelling, motion]),
            )
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

    fn calendar_page() -> SettingPage {
        let week = SettingItem::new(
            "Week starts on",
            SettingField::render(|_, _, cx| {
                segmented(
                    "set-week-start",
                    &[(WeekStart::Monday, "Monday"), (WeekStart::Sunday, "Sunday")],
                    appearance::settings(cx).week_start,
                    |v, _, cx| appearance::update(cx, |s| s.week_start = v),
                )
            }),
        )
        .description("The first column of the month and week views.")
        .keywords(["calendar", "week", "monday", "sunday"]);
        let clock = SettingItem::new(
            "Time format",
            SettingField::render(|_, _, cx| {
                segmented(
                    "set-clock",
                    &[
                        (ClockFormat::TwelveHour, "12-hour"),
                        (ClockFormat::TwentyFourHour, "24-hour"),
                    ],
                    appearance::settings(cx).clock,
                    |v, _, cx| appearance::update(cx, |s| s.clock = v),
                )
            }),
        )
        .description("How times are written: 2:30 PM or 14:30.")
        .keywords(["calendar", "clock", "time", "24 hour", "am pm"]);
        SettingPage::new("Calendar")
            .icon(Lucide::CalendarDays)
            .group(SettingGroup::new().items(vec![week, clock]))
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

        groups.push(
            SettingGroup::new().title("Habits").items(
                HABIT_SHORTCUTS
                    .iter()
                    .map(|s| key_row(s.keys, s.label))
                    .collect::<Vec<_>>(),
            ),
        );
        groups.push(
            SettingGroup::new().title("Calendar").items(
                CALENDAR_SHORTCUTS
                    .iter()
                    .map(|s| key_row(s.keys, s.label))
                    .collect::<Vec<_>>(),
            ),
        );

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
/// One row of theme cards for `mode`. Each card previews the theme in its own
/// colors (page, text, accent, two code colors); the chosen one has a ring.
/// Choosing a theme of the other mode also switches to that mode, so the
/// change is visible, unless the mode follows the system.
fn theme_cards(mode: ThemeMode, cx: &App) -> impl IntoElement {
    use gpui_kit::component::try_parse_color;
    let app = cx.theme();
    let settings = appearance::settings(cx);
    let selected = if mode.is_dark() {
        settings.dark_theme.clone()
    } else {
        settings.light_theme.clone()
    };
    let cards = keepstone_ui::theme::choices(mode, cx)
        .into_iter()
        .map(|choice| {
            let c = &choice.config.colors;
            let parse = |v: &Option<SharedString>| v.as_ref().and_then(|s| try_parse_color(s).ok());
            let bg = parse(&c.background).unwrap_or(app.background);
            let fg = parse(&c.foreground).unwrap_or(app.foreground);
            let border = parse(&c.border).unwrap_or(app.border);
            let accent = parse(&c.primary).unwrap_or(fg);
            let syntax = choice.config.highlight.as_ref().map(|h| &h.syntax);
            let tone =
                |s: Option<&gpui_kit::component::highlighter::ThemeStyle>| s.and_then(|s| s.color);
            let dots = [
                accent,
                syntax
                    .and_then(|s| tone(s.keyword.as_ref()))
                    .unwrap_or(accent),
                syntax.and_then(|s| tone(s.string.as_ref())).unwrap_or(fg),
                syntax.and_then(|s| tone(s.function.as_ref())).unwrap_or(fg),
            ];
            let is_selected = choice.name.as_ref() == selected;
            let name = choice.name.clone();
            let id = SharedString::from(format!(
                "theme-{}",
                choice.name.to_lowercase().replace(' ', "-")
            ));
            v_flex()
                .id(gpui_kit::ElementId::Name(id))
                .test_support()
                .w(gpui_kit::rems(9.))
                .gap_1p5()
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let name = name.to_string();
                    appearance::update(cx, |s| {
                        if mode.is_dark() {
                            s.dark_theme = name;
                        } else {
                            s.light_theme = name;
                        }
                        if s.theme != ThemePreference::System {
                            s.theme = if mode.is_dark() {
                                ThemePreference::Dark
                            } else {
                                ThemePreference::Light
                            };
                        }
                    });
                })
                .child(
                    v_flex()
                        .h(gpui_kit::rems(5.))
                        .p_2()
                        .gap_1()
                        .justify_between()
                        .rounded(app.radius_lg)
                        .bg(bg)
                        .border_1()
                        .border_color(border)
                        .when(is_selected, |this| this.border_2().border_color(app.ring))
                        .child(
                            div()
                                .text_lg()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .text_color(fg)
                                .child("Aa"),
                        )
                        .child(
                            h_flex().gap_1().children(
                                dots.into_iter()
                                    .map(|d| div().size(gpui_kit::rems(0.6)).rounded_full().bg(d)),
                            ),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if is_selected {
                            app.foreground
                        } else {
                            app.muted_foreground
                        })
                        .when(is_selected, |this| {
                            this.font_weight(gpui_kit::FontWeight::MEDIUM)
                        })
                        .truncate()
                        .child(choice.name.clone()),
                )
        });
    h_flex().w_full().flex_wrap().gap_3().children(cards)
}

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
                    Self::calendar_page(),
                    Self::keyboard_page(),
                    Self::about_page(),
                ]),
        )
    }
}
