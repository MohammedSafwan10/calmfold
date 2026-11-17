//! Deleted events: a right-hand sheet (Calendar → View options → Deleted
//! events…, or ⌘K). Deleting an event moves it here; Restore puts it back,
//! "Delete forever" and "Empty trash" are permanent and confirmed first.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Placement, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};
use ottero_core::{Event, EventId};
use ottero_ui::appearance;
use ottero_ui::feedback::{self, Confirm};

use crate::library::{Library, LibraryEvent};
use crate::views::event_detail::summary;
use crate::views::task_row::notify_err;

/// Opens the deleted-events sheet.
pub fn open_event_trash(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| EventTrash::new(library, cx));
    window.open_sheet_at(Placement::Right, cx, move |sheet, _, _| {
        sheet
            .title("Deleted events")
            .size(px(440.))
            .child(view.clone())
    });
}

pub struct EventTrash {
    library: Entity<Library>,
    events: Vec<Event>,
    error: bool,
    _subscription: Subscription,
}

impl EventTrash {
    fn new(library: Entity<Library>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&library, |this, _, event, cx| {
            if *event == LibraryEvent::EventsChanged {
                this.reload(cx);
            }
        });
        let mut this = Self {
            library,
            events: Vec::new(),
            error: false,
            _subscription: subscription,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        match self.library.read(cx).deleted_events() {
            Ok(events) => {
                self.events = events;
                self.error = false;
            }
            Err(err) => {
                eprintln!("ottero: couldn't load deleted events: {err:#}");
                self.error = true;
            }
        }
        cx.notify();
    }

    #[doc(hidden)]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    fn restore(&self, id: EventId, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.library.update(cx, |l, cx| l.restore_event(id, cx));
        if feedback::report(result, window, cx).is_some() {
            window.push_notification("Event restored.", cx);
        }
    }

    fn delete_forever(&self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        let id = event.id();
        let library = self.library.clone();
        feedback::confirm_destructive(
            Confirm::new(
                format!("Delete \u{201c}{}\u{201d} forever?", event.title()),
                "This can't be undone.",
                "Delete forever",
            ),
            window,
            cx,
            move |window, cx| {
                notify_err(
                    library.update(cx, |l, cx| l.delete_event_forever(id, cx)),
                    window,
                    cx,
                );
            },
        );
    }

    fn empty(&self, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.events.len();
        let library = self.library.clone();
        feedback::confirm_destructive(
            Confirm::new(
                "Empty deleted events?",
                format!(
                    "{n} event{} will be deleted forever. This can't be undone.",
                    if n == 1 { "" } else { "s" }
                ),
                "Empty trash",
            ),
            window,
            cx,
            move |window, cx| {
                notify_err(
                    library.update(cx, |l, cx| l.empty_event_trash(cx).map(|_| ())),
                    window,
                    cx,
                );
            },
        );
    }
}

impl Render for EventTrash {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.error {
            return v_flex().child(
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child("Couldn't load deleted events."),
            );
        }
        if self.events.is_empty() {
            return v_flex().gap_1().pt_2().child(
                div().text_sm().text_color(theme.muted_foreground).child(
                    "No deleted events. Events you delete stay here until you empty the trash.",
                ),
            );
        }
        let clock = appearance::settings(cx).clock;
        let rows = self.events.iter().enumerate().map(|(i, event)| {
            let id = event.id();
            let for_delete = event.clone();
            h_flex()
                .gap_2()
                .py_2()
                .items_center()
                .when(i > 0, |s| s.border_t_1().border_color(theme.border))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(event.title().to_owned()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(summary(event, None, clock)),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("restore-event-{id}")))
                        .small()
                        .outline()
                        .label("Restore")
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.restore(id, window, cx)),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("forget-event-{id}")))
                        .small()
                        .ghost()
                        .icon(Lucide::Trash)
                        .tooltip("Delete forever")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.delete_forever(&for_delete, window, cx)
                        })),
                )
        });
        v_flex().gap_3().child(v_flex().children(rows)).child(
            h_flex().justify_end().child(
                Button::new("empty-event-trash")
                    .small()
                    .ghost()
                    .label("Empty trash…")
                    .on_click(cx.listener(|this, _, window, cx| this.empty(window, cx))),
            ),
        )
    }
}
