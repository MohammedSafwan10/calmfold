//! Today: what needs attention now, how the week is going, and what's next.
//!
//! Layout (one column, max ~52 rem):
//!   header (date, greeting, "Today's note")
//!   four stat tiles
//!   Due today and overdue (full task rows)
//!   [ This week chart | Next 7 days ]   (side by side, stacks when narrow)
//!   Recent notes

use std::collections::HashMap;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Corners, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt as _, Window, div,
    px, rems,
};
use keepstone_core::{Date, Task, TaskId, clock};
use keepstone_storage::{NoteSummary, Progress, TaskCounts, TaskFilter};
use keepstone_ui::components::{empty_state, section_label};

use crate::library::Library;
use crate::views::task_detail::open_task_detail;
use crate::views::task_row::task_row;
use crate::views::{Navigate, Route, relative_day};

const RECENT_NOTES: usize = 5;
const UPCOMING_DAYS: i64 = 7;
const UPCOMING_MAX: usize = 6;

/// Asks the shell to open today's daily note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenDailyNoteRequest;

pub struct TodayView {
    library: Entity<Library>,
    today: Date,
    counts: TaskCounts,
    due: Vec<Task>,
    upcoming: Vec<Task>,
    week: Vec<(Date, usize)>,
    progress: HashMap<TaskId, Progress>,
    recent: Vec<NoteSummary>,
    has_daily_note: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for TodayView {}
impl EventEmitter<OpenDailyNoteRequest> for TodayView {}

impl TodayView {
    pub fn new(library: Entity<Library>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&library, |this, _, _, cx| this.reload(cx))];
        let mut this = Self {
            library,
            today: clock::today(),
            counts: TaskCounts::default(),
            due: Vec::new(),
            upcoming: Vec::new(),
            week: Vec::new(),
            progress: HashMap::new(),
            recent: Vec::new(),
            has_daily_note: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    /// Refreshes everything, including the date (the app may stay open past midnight).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.today = clock::today();
        let horizon = self
            .today
            .checked_add(jiff::Span::new().days(UPCOMING_DAYS))
            .unwrap_or(self.today);
        let lib = self.library.read(cx);
        self.counts = lib.task_counts().unwrap_or_default();
        self.due = lib.tasks(TaskFilter::Today(self.today)).unwrap_or_default();
        self.upcoming = lib
            .tasks(TaskFilter::Upcoming(self.today))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.due().is_some_and(|d| d <= horizon))
            .take(UPCOMING_MAX)
            .collect();
        self.week = lib.completions_by_day(7);
        self.progress = lib.subtask_progress();
        self.has_daily_note = lib.daily_note_exists(self.today);
        self.recent = lib
            .notes(false)
            .map(|mut notes| {
                notes.truncate(RECENT_NOTES);
                notes
            })
            .unwrap_or_default();
        cx.notify();
    }

    fn stat(label: &'static str, value: usize, icon: Lucide, alert: bool, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_w(rems(8.))
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
                    // Color only when it means something: overdue work exists.
                    .when(alert && value > 0, |this| this.text_color(theme.danger))
                    .child(value.to_string()),
            )
    }

    fn greeting() -> &'static str {
        match jiff::Zoned::now().hour() {
            5..=11 => "Good morning",
            12..=16 => "Good afternoon",
            _ => "Good evening",
        }
    }

    fn card(cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_w(rems(16.))
            .gap_3()
            .p_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
    }

    fn week_card(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let total: usize = self.week.iter().map(|(_, n)| n).sum();
        let bar = theme.foreground;
        let faint = theme.muted_foreground.opacity(0.35);
        let data: Vec<(SharedString, f64)> = self
            .week
            .iter()
            .map(|(day, n)| {
                (
                    SharedString::from(day.strftime("%a").to_string()),
                    *n as f64,
                )
            })
            .collect();
        Self::card(cx)
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("This week", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{total} done")),
                    ),
            )
            .child(
                div().id("week-chart").h(rems(8.)).test_support().child(
                    BarChart::new(data)
                        .band(|d: &(SharedString, f64)| d.0.clone())
                        .value(|d: &(SharedString, f64)| d.1)
                        // Today's bar is solid; earlier days are quieter.
                        .fill(
                            move |d: &(SharedString, f64), _, _, _| {
                                if d.1 > 0. { bar } else { faint }
                            },
                        )
                        .corner_radii(Corners {
                            top_left: px(3.),
                            top_right: px(3.),
                            ..Default::default()
                        })
                        .grid(false),
                ),
            )
    }

    fn upcoming_card(&self, cx: &App) -> Div {
        let theme = cx.theme();
        Self::card(cx)
            .child(section_label("Next 7 days", cx))
            .map(|this| {
                if self.upcoming.is_empty() {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Nothing scheduled."),
                    )
                } else {
                    this.children(self.upcoming.iter().map(|t| {
                        let id = t.id();
                        let library = self.library.clone();
                        let hover = theme.list_hover;
                        h_flex()
                            .id(ElementId::Name(SharedString::from(format!(
                                "upcoming-{id}"
                            ))))
                            .gap_3()
                            // Hover fill bleeds into the card padding; text stays aligned.
                            .mx(px(-8.))
                            .px(px(8.))
                            .py_1()
                            .rounded(theme.radius)
                            .text_sm()
                            .cursor_pointer()
                            .hover(move |this| this.bg(hover))
                            .on_click(move |_, window, cx| {
                                open_task_detail(id, library.clone(), window, cx);
                            })
                            .test_support()
                            .child(
                                div()
                                    .w(rems(5.5))
                                    .flex_shrink_0()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        t.due()
                                            .map(|d| relative_day(d, self.today))
                                            .unwrap_or_default(),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(t.title().to_owned()),
                            )
                    }))
                }
            })
    }
}

impl Focusable for TodayView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TodayView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let date_label = self.today.strftime("%A, %B %-d").to_string();

        let header = h_flex()
            .justify_between()
            .items_end()
            .gap_4()
            .child(
                v_flex()
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
                    ),
            )
            .child(
                Button::new("today-daily-note")
                    .outline()
                    .small()
                    .icon(Lucide::NotebookPen)
                    .label(if self.has_daily_note {
                        "Open today's note"
                    } else {
                        "Start today's note"
                    })
                    .tooltip_with_action("Today's note", &crate::actions::OpenDailyNote, None)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenDailyNoteRequest))),
            );

        let stats = h_flex()
            .flex_wrap()
            .gap_3()
            .child(Self::stat(
                "Due today",
                self.counts.due_today,
                Lucide::CalendarCheck,
                false,
                cx,
            ))
            .child(Self::stat(
                "Overdue",
                self.counts.overdue,
                Lucide::CalendarClock,
                true,
                cx,
            ))
            .child(Self::stat(
                "Done today",
                self.counts.completed_today,
                Lucide::CircleCheckBig,
                false,
                cx,
            ))
            .child(Self::stat(
                "Open tasks",
                self.counts.open,
                Lucide::ListTodo,
                false,
                cx,
            ));

        let link = |id: &'static str, label: &'static str, route: Route, cx: &mut Context<Self>| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Navigate::To(route))))
        };

        let tasks = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("Due today and overdue", cx))
                    .child(link("today-all-tasks", "All tasks", Route::Tasks, cx)),
            )
            .child(v_flex().gap_0p5().map(|this| {
                if self.due.is_empty() {
                    this.child(div().h(rems(9.)).child(empty_state(
                        Lucide::Sunrise,
                        "You're all caught up",
                        "Nothing is due today.",
                        cx,
                    )))
                } else {
                    this.children(self.due.iter().map(|t| {
                        task_row(
                            t,
                            self.today,
                            self.progress.get(&t.id()).copied(),
                            &self.library,
                            cx,
                        )
                    }))
                }
            }));

        let notes = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label("Recent notes", cx))
                    .child(link("today-all-notes", "All notes", Route::Notes, cx)),
            )
            .child(v_flex().gap_0p5().map(|this| {
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
                            .gap_3()
                            .px_3()
                            .py_2()
                            .rounded(theme.radius)
                            .hover(|this| this.bg(theme.list_hover))
                            .on_click(
                                cx.listener(move |_, _, _, cx| cx.emit(Navigate::OpenNote(id))),
                            )
                            .child(
                                Icon::new(Lucide::FileText)
                                    .small()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .w(rems(10.))
                                    .flex_shrink_0()
                                    .truncate()
                                    .text_sm()
                                    .child(n.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(n.preview.clone()),
                            )
                    }))
                }
            }));

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
                    .child(tasks)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_start()
                            .gap_3()
                            .child(self.week_card(cx))
                            .child(self.upcoming_card(cx)),
                    )
                    .child(notes),
            )
    }
}
