//! Workspace navigation controls, all drawn from the feature catalog.
use crate::{nav, ui_state::LayoutStore};
use calmfold_ui::feedback;
use gpui_kit::InteractiveElement as _;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::{
    App, Entity, IntoElement, ParentElement as _, SharedString, Styled as _, TestSupportExt as _,
    div,
};

pub fn general_page(layout: Entity<LayoutStore>) -> SettingPage {
    let get = layout.clone();
    SettingPage::new("General").icon(Lucide::Settings).group(
        SettingGroup::new().title("Startup").item(
            SettingItem::new(
                "Open on startup",
                SettingField::scrollable_dropdown(
                    nav::entries()
                        .map(|e| (e.id.into(), e.label.into()))
                        .collect(),
                    move |cx| {
                        let prefs = get.read(cx).navigation();
                        nav::entries()
                            .find(|e| e.id == prefs.start_page)
                            .map_or("today", |e| e.id)
                            .into()
                    },
                    move |id, cx| {
                        layout.update(cx, |l, cx| {
                            l.edit_navigation(|p| p.start_page = id.to_string(), cx)
                        })
                    },
                ),
            )
            .description("Choose where Calmfold opens. Hidden features can still be opened.")
            .keywords(["startup", "start page", "home", "launch", "open"]),
        ),
    )
}

pub fn workspace_page(
    layout: Entity<LayoutStore>,
    focuses: &std::collections::BTreeMap<
        &'static str,
        (gpui_kit::FocusHandle, gpui_kit::FocusHandle),
    >,
    cx: &App,
) -> SettingPage {
    let rows = layout
        .read(cx)
        .navigation()
        .ordered()
        .into_iter()
        .filter_map(|entry| {
            // Own focus outside Settings' index-keyed item wrappers so a keyboard
            // reorder follows the feature instead of jumping to another row.
            let (up_focus, down_focus) = focuses.get(entry.id)?.clone();
            let (id, label, icon) = (entry.id, entry.label, entry.icon);
            let layout = layout.clone();
            // Native Settings search can find each feature, not just one giant list.
            Some(
                SettingItem::render(move |_, _, cx| {
                    let prefs = layout.read(cx).navigation();
                    let ordered = prefs.ordered();
                    let index = ordered.iter().position(|e| e.id == id).unwrap_or(0);
                    let count = ordered.len();
                    let (up_layout, down_layout, show_layout) =
                        (layout.clone(), layout.clone(), layout.clone());
                    h_flex()
                        .id(SharedString::from(format!("sidebar-option-{id}")))
                        .test_support()
                        .w_full()
                        .gap_3()
                        .items_center()
                        .py_1()
                        .child(
                            Icon::new(icon)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(div().flex_1().text_sm().child(label))
                        .child(
                            Button::new(SharedString::from(format!("sidebar-up-{id}")))
                                .track_focus(&up_focus)
                                .ghost()
                                .xsmall()
                                .icon(Lucide::ChevronUp)
                                .disabled(index == 0)
                                .accessibility_label(format!("Move {label} up"))
                                .tooltip(format!("Move {label} up"))
                                .on_click(move |_, _, cx| {
                                    up_layout.update(cx, |l, cx| {
                                        l.edit_navigation(|p| p.move_by(id, -1), cx)
                                    })
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("sidebar-down-{id}")))
                                .track_focus(&down_focus)
                                .ghost()
                                .xsmall()
                                .icon(Lucide::ChevronDown)
                                .disabled(index + 1 == count)
                                .accessibility_label(format!("Move {label} down"))
                                .tooltip(format!("Move {label} down"))
                                .on_click(move |_, _, cx| {
                                    down_layout.update(cx, |l, cx| {
                                        l.edit_navigation(|p| p.move_by(id, 1), cx)
                                    })
                                }),
                        )
                        .child(
                            Switch::new(SharedString::from(format!("sidebar-show-{id}")))
                                .checked(prefs.is_visible(id))
                                .accessibility_label(format!("Show {label} in sidebar"))
                                .on_click(move |show, _, cx| {
                                    show_layout.update(cx, |l, cx| {
                                        l.edit_navigation(|p| p.set_visible(id, *show), cx)
                                    })
                                }),
                        )
                        .into_any_element()
                })
                .keywords([
                    label,
                    "sidebar",
                    "workspace",
                    "navigation",
                    "hide",
                    "show",
                    "reorder",
                    "customize",
                ]),
            )
        })
        .collect::<Vec<_>>();
    let reset = SettingItem::render(move |_, _, _| {
        let layout = layout.clone();
        h_flex()
            .w_full()
            .child(
                Button::new("sidebar-reset")
                    .ghost()
                    .small()
                    .label("Restore default sidebar")
                    .on_click(move |_, w, cx| {
                        let previous = layout.read(cx).navigation();
                        layout.update(cx, |l, cx| l.edit_navigation(|p| p.reset_sidebar(), cx));
                        let layout = layout.clone();
                        feedback::undo_toast("Default sidebar restored.", w, cx, move |_, cx| {
                            layout.update(cx, |l, cx| {
                                l.edit_navigation(
                                    |p| {
                                        p.order = previous.order.clone();
                                        p.visible = previous.visible.clone();
                                    },
                                    cx,
                                )
                            })
                        });
                    }),
            )
            .into_any_element()
    })
    .keywords(["sidebar", "reset", "default", "restore"]);
    SettingPage::new("Workspace").icon(Lucide::PanelLeft).group(
        SettingGroup::new()
            .title("Sidebar")
            .max_w(gpui_kit::rems(44.))
            .description("Use the arrows to reorder. Hidden features remain in More.")
            .items(rows)
            .item(reset),
    )
}
