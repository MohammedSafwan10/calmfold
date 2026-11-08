//! Today: a calm overview of what needs attention, plus recent notes.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, rems,
};
use keepstone_core::{Date, Task, clock};
use keepstone_storage::{NoteSummary, TaskCounts, TaskFilter};
use keepstone_ui::components::{empty_state, section_label};

use crate::library::Library;
use crate::views::task_row::task_row;
use crate::views::{Navigate, Route};

const RECENT_NOTES: usize = 6;

pub struct TodayView {
    library: Entity<Library>,
    today: Date,
    counts: TaskCounts,
    due: Vec<Task>,
    recent: Vec<NoteSummary>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for TodayView {}

impl TodayView {
    pub fn new(library: Entity<Library>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&library, |this, _, _, cx| this.reload(cx))];
        let mut this = Self {
            library,
            today: clock::today(),
            counts: TaskCounts::default(),
            due: Vec::new(),
            recent: Vec::new(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    /// Refreshes everything, including the date (the app may stay open past midnight).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.today = clock::today();
        let lib = self.library.read(cx);
        self.counts = lib.task_counts().unwrap_or_default();
        self.due = lib.tasks(TaskFilter::Today(self.today)).unwrap_or_default();
        self.recent = lib
            .notes(false)
            .map(|mut notes| {
                notes.truncate(RECENT_NOTES);
                notes
            })
            .unwrap_or_default();
        cx.notify();
    }

    fn stat(label: &'static str, value: usize, icon: Lucide, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_w(rems(9.))
            .gap_2()
            .p_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(icon).xsmall())
                    .child(label),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value.to_string()),
            )
    }

    fn greeting() -> &'static str {
        let hour = jiff::Zoned::now().hour();
        match hour {
            5..=11 => "Good morning",
            12..=16 => "Good afternoon",
            _ => "Good evening",
        }
    }
}

impl Focusable for TodayView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TodayView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let date_label = self.today.strftime("%A, %B %-d").to_string();

        let header = v_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(date_label),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Self::greeting()),
            );

        let stats = h_flex()
            .flex_wrap()
            .gap_3()
            .child(Self::stat(
                "Due today",
                self.counts.due_today,
                Lucide::CalendarCheck,
                cx,
            ))
            .child(Self::stat(
                "Overdue",
                self.counts.overdue,
                Lucide::CalendarClock,
                cx,
            ))
            .child(Self::stat(
                "Done today",
                self.counts.completed_today,
                Lucide::CircleCheckBig,
                cx,
            ))
            .child(Self::stat(
                "Open tasks",
                self.counts.open,
                Lucide::ListTodo,
                cx,
            ));

        let tasks_header = h_flex()
            .justify_between()
            .child(section_label("Due today and overdue", cx))
            .child(
                Button::new("today-all-tasks")
                    .ghost()
                    .xsmall()
                    .label("All tasks")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Navigate::To(Route::Tasks)))),
            );

        let tasks = v_flex().gap_0p5().map(|this| {
            if self.due.is_empty() {
                this.child(div().h(rems(10.)).child(empty_state(
                    Lucide::Sunrise,
                    "You're all caught up",
                    "Nothing is due today.",
                    cx,
                )))
            } else {
                this.children(
                    self.due
                        .iter()
                        .map(|t| task_row(t, self.today, &self.library, cx)),
                )
            }
        });

        let notes_header = h_flex()
            .justify_between()
            .child(section_label("Recent notes", cx))
            .child(
                Button::new("today-all-notes")
                    .ghost()
                    .xsmall()
                    .label("All notes")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Navigate::To(Route::Notes)))),
            );

        let notes = v_flex().gap_0p5().map(|this| {
            if self.recent.is_empty() {
                this.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No notes yet."),
                )
            } else {
                this.children(self.recent.iter().map(|n| {
                    let id = n.id;
                    h_flex()
                        .id(ElementId::Name(SharedString::from(format!("recent-{id}"))))
                        .gap_2()
                        .px_3()
                        .py_2()
                        .rounded(theme.radius)
                        .hover(|this| this.bg(theme.list_hover))
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(Navigate::OpenNote(id))))
                        .child(
                            Icon::new(Lucide::FileText)
                                .small()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .child(n.title.clone()),
                        )
                }))
            }
        });

        div()
            .id("today-scroll")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(rems(52.))
                    .mx_auto()
                    .px_6()
                    .py_8()
                    .gap_6()
                    .child(header)
                    .child(stats)
                    .child(v_flex().gap_2().child(tasks_header).child(tasks))
                    .child(v_flex().gap_2().child(notes_header).child(notes)),
            )
    }
}
