//! Tasks screen: quick add plus Today / Upcoming / All / Done / Trash lists.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, rems,
};
use keepstone_core::{Date, Task, clock};
use keepstone_storage::TaskFilter;
use keepstone_ui::components::{empty_state, page_header, segmented};
use keepstone_ui::feedback::{self, Confirm};

use crate::library::{Library, LibraryEvent};
use crate::views::task_row::{notify_err, task_row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTab {
    Today,
    Upcoming,
    All,
    Done,
    Trash,
}

impl TaskTab {
    const OPTIONS: [(Self, &'static str); 5] = [
        (Self::Today, "Today"),
        (Self::Upcoming, "Upcoming"),
        (Self::All, "All"),
        (Self::Done, "Done"),
        (Self::Trash, "Trash"),
    ];

    fn filter(self, today: Date) -> TaskFilter {
        match self {
            Self::Today => TaskFilter::Today(today),
            Self::Upcoming => TaskFilter::Upcoming(today),
            Self::All => TaskFilter::Open,
            Self::Done => TaskFilter::Closed,
            Self::Trash => TaskFilter::Trash,
        }
    }
}

pub struct TasksView {
    library: Entity<Library>,
    input: Entity<InputState>,
    tab: TaskTab,
    tasks: Vec<Task>,
    load_error: bool,
    today: Date,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl TasksView {
    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Add a task and press Enter"));
        let subscriptions = vec![
            cx.subscribe(&library, |this, _, event, cx| {
                if *event == LibraryEvent::TasksChanged {
                    this.reload(cx);
                }
            }),
            cx.subscribe_in(&input, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.add_from_input(window, cx);
                }
            }),
        ];
        let mut this = Self {
            library,
            input,
            tab: TaskTab::Today,
            tasks: Vec::new(),
            load_error: false,
            today: clock::today(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    /// Focuses the quick-add field (used by the "New task" action).
    pub fn focus_new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == TaskTab::Done || self.tab == TaskTab::Trash {
            self.set_tab(TaskTab::Today, cx);
        }
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn tab(&self) -> TaskTab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: TaskTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.reload(cx);
        }
    }

    /// Reloads when the date changed since the last load (app left open overnight).
    pub fn refresh_if_new_day(&mut self, cx: &mut Context<Self>) {
        if clock::today() != self.today {
            self.reload(cx);
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.today = clock::today();
        match self.library.read(cx).tasks(self.tab.filter(self.today)) {
            Ok(tasks) => {
                self.tasks = tasks;
                self.load_error = false;
            }
            Err(err) => {
                eprintln!("keepstone: failed to load tasks: {err:#}");
                self.tasks.clear();
                self.load_error = true;
            }
        }
        cx.notify();
    }

    fn add_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.input.read(cx).value().to_string();
        if title.trim().is_empty() {
            return;
        }
        // New tasks land where the user is looking: Today → due today,
        // Upcoming → due tomorrow, otherwise no date.
        let due = match self.tab {
            TaskTab::Today => Some(self.today),
            TaskTab::Upcoming => self.today.tomorrow().ok(),
            _ => None,
        };
        let result = self
            .library
            .update(cx, |lib, cx| lib.create_task(&title, due, cx));
        if result.is_ok() {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        notify_err(result, window, cx);
    }
}

impl TasksView {
    fn empty_trash(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let library = self.library.clone();
        feedback::confirm_destructive(
            Confirm::new(
                "Empty task trash?",
                format!(
                    "{} task(s) will be deleted forever. This can't be undone.",
                    self.tasks.len()
                ),
                "Empty trash",
            ),
            window,
            cx,
            move |window, cx| {
                notify_err(
                    library.update(cx, |lib, cx| lib.empty_task_trash(cx)),
                    window,
                    cx,
                );
            },
        );
    }
}

impl Focusable for TasksView {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TasksView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        let tab_bar = segmented(
            "task-tabs",
            &TaskTab::OPTIONS,
            self.tab,
            move |tab, _, cx| {
                _ = view.update(cx, |this, cx| this.set_tab(tab, cx));
            },
        );

        let (empty_title, empty_desc) = match self.tab {
            _ if self.load_error => (
                "Couldn't load tasks",
                "Your tasks are still on disk. Restart Keepstone; if this keeps happening, check the data folder in Settings.",
            ),
            TaskTab::Today => (
                "Nothing due today",
                "Add a task above. Tasks added here are due today.",
            ),
            TaskTab::Upcoming => ("Nothing upcoming", "Give a task a due date to see it here."),
            TaskTab::All => ("No open tasks", "Add a task above to get started."),
            TaskTab::Done => ("Nothing finished yet", "Completed tasks show up here."),
            TaskTab::Trash => (
                "Trash is empty",
                "Tasks you delete stay here until you empty the trash.",
            ),
        };
        let show_input = !matches!(self.tab, TaskTab::Done | TaskTab::Trash);

        v_flex()
            .track_focus(&self.focus)
            .size_full()
            .child(
                v_flex()
                    .w_full()
                    .max_w(rems(48.))
                    .mx_auto()
                    .px_6()
                    .pt_6()
                    .gap_4()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_end()
                            .gap_4()
                            .child(page_header("Tasks", None, cx))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .when(
                                        self.tab == TaskTab::Trash && !self.tasks.is_empty(),
                                        |this| {
                                            this.child(
                                                Button::new("tasks-empty-trash")
                                                    .ghost()
                                                    .small()
                                                    .label("Empty trash")
                                                    .on_click(cx.listener(Self::empty_trash)),
                                            )
                                        },
                                    )
                                    .child(tab_bar),
                            ),
                    )
                    .children(show_input.then(|| {
                        Input::new(&self.input).prefix(
                            Icon::new(Lucide::Plus)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })),
            )
            .child(
                div()
                    .id("task-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(rems(48.))
                            .mx_auto()
                            .px_6()
                            .py_3()
                            .gap_0p5()
                            .map(|list| {
                                if self.tasks.is_empty() {
                                    list.child(empty_state(
                                        Lucide::ListTodo,
                                        empty_title,
                                        empty_desc,
                                        cx,
                                    ))
                                } else {
                                    list.children(
                                        self.tasks.iter().map(|task| {
                                            task_row(task, self.today, &self.library, cx)
                                        }),
                                    )
                                }
                            }),
                    ),
            )
    }
}
