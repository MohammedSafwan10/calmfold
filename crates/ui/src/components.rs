//! Reusable building blocks shared by feature screens.
//!
//! Keep these thin compositions of GPUI Kit components. If a piece needs its
//! own state, make it an `Entity` in the feature that owns the state instead.

use std::rc::Rc;

use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Div, ElementId, FontWeight, ParentElement as _, SharedString, Styled as _, Window, div,
};

/// A segmented control for choosing one value from a short, fixed set.
///
/// `options` pairs each value with its visible label. Selection is controlled:
/// the caller supplies `selected` and updates its own state in `on_change`.
pub fn segmented<T>(
    id: impl Into<ElementId>,
    options: &[(T, &'static str)],
    selected: T,
    on_change: impl Fn(T, &mut Window, &mut App) + 'static,
) -> TabBar
where
    T: Copy + PartialEq + 'static,
{
    let values: Rc<Vec<T>> = Rc::new(options.iter().map(|(v, _)| *v).collect());
    let selected_index = values.iter().position(|v| *v == selected).unwrap_or(0);
    TabBar::new(id)
        .segmented()
        .small()
        .selected_index(selected_index)
        .children(options.iter().map(|(_, label)| Tab::new().label(*label)))
        .on_click(move |ix, window, cx| {
            if let Some(value) = values.get(*ix) {
                on_change(*value, window, cx);
            }
        })
}

/// Title row at the top of a screen, with optional supporting text.
pub fn page_header(
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    cx: &App,
) -> Div {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .when_some(subtitle, |this, subtitle| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle),
            )
        })
}

/// Centered placeholder for an empty list or screen.
pub fn empty_state(
    icon: impl Into<Icon>,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    cx: &App,
) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_8()
        .child(
            Icon::new(icon)
                .large()
                .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(title.into()),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .text_center()
                .max_w_80()
                .child(description.into()),
        )
}

/// Small uppercase-free section label used above groups of rows.
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
