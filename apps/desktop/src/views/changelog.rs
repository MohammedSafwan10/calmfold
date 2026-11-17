//! Offline release history, bundled from the same source as signed release notes.
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::label::Label;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Div, FocusHandle, FontWeight, InteractiveElement as _, KeyDownEvent,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window,
    div, rems,
};
use serde::Deserialize;
use std::rc::Rc;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Release {
    version: String,
    date: String,
    title: String,
    summary: String,
    sections: Vec<Section>,
}
#[derive(Deserialize)]
struct Section {
    title: String,
    emoji: String,
    items: Vec<String>,
}
static RELEASES: LazyLock<Result<Vec<Release>, serde_json::Error>> =
    LazyLock::new(|| serde_json::from_str(include_str!("../../changelog.json")));

pub fn render(
    focus: &FocusHandle,
    selected: usize,
    narrow: bool,
    select: impl Fn(usize, &mut Window, &mut App) + 'static,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let select = Rc::new(select);
    let theme = cx.theme();
    let mut content = v_flex().w_full().max_w(rems(44.)).gap_10().py_8();
    content = content.child(
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Label::new("What’s new")),
            )
            .child(
                div().text_color(theme.muted_foreground).child(
                    Label::new("A little better with every release.")
                        .text_color(theme.muted_foreground),
                ),
            ),
    );
    match RELEASES.as_ref() {
        Ok(releases) if !releases.is_empty() => {
            for release in releases
                .iter()
                .skip(selected.min(releases.len() - 1))
                .take(1)
            {
                let mut article = v_flex()
                    .gap_6()
                    .pb_8()
                    .border_b_1()
                    .border_color(theme.border);
                let mut metadata = h_flex()
                    .gap_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(
                        Label::new(format!("Version {}", release.version))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        Label::new(release.date.parse::<jiff::civil::Date>().map_or_else(
                            |_| release.date.clone(),
                            |d| d.strftime("%B %-d, %Y").to_string(),
                        ))
                        .text_color(theme.muted_foreground),
                    );
                if release.version == env!("CARGO_PKG_VERSION") {
                    metadata =
                        metadata.child(Label::new("Installed").text_color(theme.muted_foreground));
                }
                article = article.child(
                    v_flex()
                        .gap_2()
                        .child(metadata)
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(Label::new(release.title.clone())),
                        )
                        .child(div().text_color(theme.muted_foreground).child(
                            Label::new(release.summary.clone()).text_color(theme.muted_foreground),
                        )),
                );
                for section in &release.sections {
                    let rows = section.items.iter().map(|text| {
                        h_flex()
                            .items_start()
                            .gap_3()
                            .child(div().text_color(theme.muted_foreground).child("•"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Label::new(text.clone()).line_height(rems(1.5))),
                            )
                    });
                    article = article.child(
                        v_flex()
                            .gap_3()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(section.emoji.clone())
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(Label::new(section.title.clone())),
                                    ),
                            )
                            .children(rows),
                    );
                }
                content = content.child(article);
            }
        }
        Ok(_) => {
            content = content.child(Label::new(
                "Release notes will appear here with the next update.",
            ))
        }
        Err(_) => {
            content = content.child(Label::new(
                "Release notes couldn’t be loaded. You can keep using Calmfold.",
            ))
        }
    }
    let mut rail = v_flex()
        .id("release-versions")
        .w(rems(10.))
        .flex_shrink_0()
        .gap_1()
        .p_3()
        .border_r_1()
        .border_color(theme.border)
        .overflow_y_scroll();
    if let Ok(releases) = RELEASES.as_ref() {
        rail = rail.child(
            div()
                .px_2()
                .py_2()
                .text_sm()
                .child(Label::new("Releases").text_color(theme.muted_foreground)),
        );
        for (index, release) in releases.iter().enumerate() {
            let select = select.clone();
            rail = rail.child(
                Button::new(gpui_kit::SharedString::from(format!(
                    "release-{}",
                    release.version
                )))
                .ghost()
                .w_full()
                .justify_start()
                .accessibility_label(format!("Version {}", release.version))
                .child(
                    div()
                        .w_full()
                        .text_left()
                        .child(format!("Version {}", release.version)),
                )
                .selected(index == selected)
                .on_click(move |_, w, cx| select(index, w, cx)),
            );
        }
    }
    let picker_select = select.clone();
    let picker = Button::new("release-version-picker")
        .ghost()
        .small()
        .icon(Lucide::ChevronDown)
        .label(
            RELEASES
                .as_ref()
                .ok()
                .and_then(|r| r.get(selected))
                .map_or_else(
                    || "Releases".to_owned(),
                    |r| format!("Version {}", r.version),
                ),
        )
        .dropdown_menu(move |mut menu, _, _| {
            if let Ok(releases) = RELEASES.as_ref() {
                for (index, release) in releases.iter().enumerate() {
                    let select = picker_select.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("Version {}", release.version))
                            .checked(index == selected)
                            .on_click(move |_, w, cx| select(index, w, cx)),
                    );
                }
            }
            menu
        });
    div().size_full().child(
        v_flex()
            .id("whats-new")
            .test_support()
            .track_focus(focus)
            .on_key_down(key)
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                h_flex()
                    .px_6()
                    .py_3()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("whats-new-back")
                            .ghost()
                            .small()
                            .icon(Lucide::ArrowLeft)
                            .label("About")
                            .on_click(back),
                    )
                    .when(narrow, |this| this.child(picker)),
            )
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .when(!narrow, |this| this.child(rail))
                    .child(
                        div()
                            .id(gpui_kit::ElementId::NamedInteger(
                                "whats-new-scroll".into(),
                                selected as u64,
                            ))
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(v_flex().items_center().px_6().child(content)),
                    ),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_complete_unique_and_newest_first() {
        let releases = RELEASES.as_ref().expect("valid bundled notes");
        assert_eq!(releases[0].version, env!("CARGO_PKG_VERSION"));
        for pair in releases.windows(2) {
            assert!(calmfold_core::update::is_newer(
                &pair[0].version,
                &pair[1].version
            ));
        }
        for release in releases {
            assert!(release.date.parse::<jiff::civil::Date>().is_ok());
            assert!(!release.title.is_empty() && !release.summary.is_empty());
            assert!(!release.sections.is_empty());
            for section in &release.sections {
                assert!(!section.title.is_empty() && !section.items.is_empty());
                assert!(section.items.iter().all(|item| !item.trim().is_empty()));
            }
        }
    }
}
