//! Password generator dialog.
//!
//! Random characters or EFF words, with the options that matter and nothing
//! else. The result regenerates on every change, shows its strength in
//! plain words, and can be copied or (from an edit form) used directly.
//! Options are remembered in `vault.json`; generated values never are.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, div, px, rems,
};
use ottero_core::vault::generator::{
    MAX_LENGTH, MAX_WORDS, MIN_LENGTH, MIN_WORDS, Options, Style, generate,
};
use ottero_core::vault::health::Strength;
use ottero_crypto::Zeroizing;
use ottero_ui::components::segmented;

use crate::library::Library;
use crate::vault_prefs;
use crate::views::passwords::strength_meter;
use crate::views::vault_item::{copy_field, random_below};

/// Opens the generator. With `target`, "Use password" fills that input.
pub fn open_generator(
    target: Option<Entity<InputState>>,
    library: &Entity<Library>,
    window: &mut Window,
    cx: &mut App,
) {
    library.update(cx, |lib, _| lib.touch_vault());
    let panel = cx.new(|cx| GeneratorPanel::new(target, library.clone(), window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title("Generate password")
            .w(px(460.))
            .child(panel.clone())
    });
}

pub struct GeneratorPanel {
    options: Options,
    /// `None` when the system random source failed.
    value: Option<Zeroizing<String>>,
    target: Option<Entity<InputState>>,
    /// Opened over an unlocked vault (so a later lock closes it).
    was_unlocked: bool,
    length: Entity<SliderState>,
    words: Entity<SliderState>,
    _subscriptions: Vec<Subscription>,
}

impl GeneratorPanel {
    fn new(
        target: Option<Entity<InputState>>,
        library: Entity<Library>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let options = vault_prefs::get(cx).generator;
        let length = cx.new(|_| {
            SliderState::new()
                .min(f32::from(MIN_LENGTH))
                .max(f32::from(MAX_LENGTH))
                .step(1.)
                .default_value(f32::from(options.length))
        });
        let words = cx.new(|_| {
            SliderState::new()
                .min(f32::from(MIN_WORDS))
                .max(f32::from(MAX_WORDS))
                .step(1.)
                .default_value(f32::from(options.words))
        });
        let subscriptions = vec![
            // Locking (manual, idle, background) closes the dialog and drops
            // the generated value: nothing secret stays on screen.
            cx.subscribe_in(&library, window, |this, library, event, window, cx| {
                if *event == crate::library::LibraryEvent::VaultChanged
                    && this.was_unlocked
                    && library.read(cx).vault_state()
                        != ottero_core::vault::lock::LockState::Unlocked
                {
                    this.value = None;
                    this.target = None;
                    window.close_dialog(cx);
                }
            }),
            cx.subscribe_in(&length, window, |this, _, e: &SliderEvent, _, cx| {
                if let SliderEvent::Change(v) = e {
                    let n = v.start().round() as u8;
                    if n != this.options.length {
                        this.set(|o| o.length = n, cx);
                    }
                }
            }),
            cx.subscribe_in(&words, window, |this, _, e: &SliderEvent, _, cx| {
                if let SliderEvent::Change(v) = e {
                    let n = v.start().round() as u8;
                    if n != this.options.words {
                        this.set(|o| o.words = n, cx);
                    }
                }
            }),
        ];
        let value = generate(&options, &mut random_below);
        Self {
            options,
            value,
            target,
            was_unlocked: library.read(cx).vault_state()
                == ottero_core::vault::lock::LockState::Unlocked,
            length,
            words,
            _subscriptions: subscriptions,
        }
    }

    fn set(&mut self, edit: impl FnOnce(&mut Options), cx: &mut Context<Self>) {
        edit(&mut self.options);
        self.options = self.options.clamped();
        let options = self.options;
        vault_prefs::update(cx, |p| p.generator = options);
        self.regenerate(cx);
    }

    fn regenerate(&mut self, cx: &mut Context<Self>) {
        self.value = generate(&self.options, &mut random_below);
        cx.notify();
    }

    fn strength(&self) -> Strength {
        // Entropy is exact for generated values; map it like 1Password's meter.
        match self.options.entropy_bits() {
            b if b >= 100. => Strength::VeryStrong,
            b if b >= 70. => Strength::Strong,
            b if b >= 50. => Strength::Fair,
            b if b >= 35. => Strength::Weak,
            _ => Strength::VeryWeak,
        }
    }

    fn switch(
        id: &'static str,
        label: &'static str,
        on: bool,
        edit: fn(&mut Options, bool),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .justify_between()
            .items_center()
            .child(div().text_sm().child(label))
            .child(
                Switch::new(id)
                    .checked(on)
                    .accessibility_label(label)
                    .on_click(cx.listener(move |this, value: &bool, _, cx| {
                        let value = *value;
                        this.set(|o| edit(o, value), cx);
                    })),
            )
    }
}

impl Render for GeneratorPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let o = self.options;
        let view = cx.entity().downgrade();
        let style = segmented(
            "gen-style",
            &[(Style::Random, "Characters"), (Style::Words, "Words")],
            o.style,
            move |s, _, cx| {
                _ = view.update(cx, |this, cx| this.set(|o| o.style = s, cx));
            },
        );
        let view = cx.entity().downgrade();
        let separator = segmented(
            "gen-separator",
            &[
                ('-', "Dash"),
                ('.', "Period"),
                ('_', "Underscore"),
                (' ', "Space"),
            ],
            o.separator,
            move |c, _, cx| {
                _ = view.update(cx, |this, cx| this.set(|o| o.separator = c, cx));
            },
        );
        let bits = o.entropy_bits().round() as i64;
        let options = match o.style {
            Style::Random => v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex().justify_between().text_sm().child("Length").child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(o.length.to_string()),
                            ),
                        )
                        .child(Slider::new(&self.length)),
                )
                .child(Self::switch(
                    "gen-digits",
                    "Numbers",
                    o.digits,
                    |o, v| o.digits = v,
                    cx,
                ))
                .child(Self::switch(
                    "gen-symbols",
                    "Symbols",
                    o.symbols,
                    |o, v| o.symbols = v,
                    cx,
                ))
                .child(Self::switch(
                    "gen-ambiguous",
                    "Avoid look-alikes (l, 1, O, 0)",
                    o.avoid_ambiguous,
                    |o, v| o.avoid_ambiguous = v,
                    cx,
                )),
            Style::Words => v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex().justify_between().text_sm().child("Words").child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(o.words.to_string()),
                            ),
                        )
                        .child(Slider::new(&self.words)),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_sm().child("Separator"))
                        .child(separator),
                )
                .child(Self::switch(
                    "gen-capitalize",
                    "Capitalize",
                    o.capitalize,
                    |o, v| o.capitalize = v,
                    cx,
                ))
                .child(Self::switch(
                    "gen-number",
                    "Add a number",
                    o.add_number,
                    |o, v| o.add_number = v,
                    cx,
                )),
        };
        let copy_value = self.value.as_ref().map(|v| v.to_string());
        let use_value = copy_value.clone();
        let failed = self.value.is_none();
        let target = self.target.clone();
        v_flex()
            .gap_5()
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .px_3()
                            .py_3()
                            .rounded(theme.radius_lg)
                            .bg(theme.muted)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_base()
                                    .font_family(theme.mono_font_family.clone())
                                    .whitespace_normal()
                                    .map(|s| match &self.value {
                                        Some(v) => s.child(v.to_string()),
                                        None => s
                                            .text_sm()
                                            .text_color(theme.danger)
                                            .child(crate::views::vault_item::RANDOM_FAILED),
                                    }),
                            )
                            .child(
                                Button::new("gen-again")
                                    .ghost()
                                    .small()
                                    .icon(Lucide::RefreshCw)
                                    .tooltip("Generate another")
                                    .on_click(cx.listener(|this, _, _, cx| this.regenerate(cx))),
                            ),
                    )
                    .child(strength_meter(self.strength(), None, cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{bits} bits of randomness. Made on this device and never saved here.")),
                    ),
            )
            .child(style)
            .child(options)
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("gen-copy")
                            .icon(Lucide::Copy)
                            .label("Copy")
                            .disabled(failed)
                            .on_click(move |_, window, cx| {
                                if let Some(v) = &copy_value {
                                    copy_field("Password", v.clone(), true, window, cx);
                                }
                            }),
                    )
                    .when_some(target, |row, target| {
                        row.child(
                            Button::new("gen-use")
                                .primary()
                                .label("Use password")
                                .disabled(failed)
                                .on_click(move |_, window, cx| {
                                    if let Some(v) = &use_value {
                                        target.update(cx, |i, cx| i.set_value(v.clone(), window, cx));
                                        window.close_dialog(cx);
                                    }
                                }),
                        )
                    }),
            )
            .when(o.style == Style::Random && o.length < 12, |s| {
                s.child(
                    div()
                        .text_xs()
                        .text_color(theme.warning)
                        .child("Short passwords are easier to crack. 16 or more is best."),
                )
            })
            .map(|s| s.child(div().h(rems(0.))))
    }
}

use gpui_kit::prelude::FluentBuilder as _;
