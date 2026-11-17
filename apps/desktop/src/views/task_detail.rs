//! Task detail panel: a right-hand sheet to edit one task.
//!
//! Title and notes autosave (debounced, plus on blur and when the panel
//! closes). Status, due date and priority save immediately. Status buttons
//! offer only the transitions the task state machine allows.

use std::time::Duration;

use chrono::Datelike as _;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::calendar::Date as PickerDate;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Placement, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Task as GpuiTask, Window, div, px,
};
use ottero_core::{Date, Priority, Task, TaskId, TaskStatus, clock};
use ottero_ui::components::{section_label, segmented};
use ottero_ui::feedback;

use crate::library::{Library, LibraryEvent};
use crate::views::task_row::notify_err;

const AUTOSAVE_DELAY: Duration = Duration::from_millis(500);

/// Opens the detail panel for `id`.
pub fn open_task_detail(id: TaskId, library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let detail = cx.new(|cx| TaskDetail::new(id, library, window, cx));
    window.open_sheet_at(Placement::Right, cx, move |sheet, _, _| {
        sheet.title("Task").size(px(440.)).child(detail.clone())
    });
}

pub struct TaskDetail {
    id: TaskId,
    library: Entity<Library>,
    task: Option<Task>,
    title: Entity<InputState>,
    notes: Entity<TextareaState>,
    due: Entity<DatePickerState>,
    /// Quick-add field for checklist steps.
    new_subtask: Entity<InputState>,
    subtasks: Vec<Task>,
    dirty: bool,
    pending_save: Option<GpuiTask<()>>,
    _subscriptions: Vec<Subscription>,
}

impl TaskDetail {
    pub fn new(
        id: TaskId,
        library: Entity<Library>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let task = library.read(cx).tasks_by_id(id);
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Task title")
                .validate(|text, _| text.chars().count() <= ottero_core::MAX_TITLE_CHARS)
        });
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 14)
                .placeholder("Notes")
        });
        let due = cx.new(|cx| DatePickerState::new(window, cx));
        let new_subtask = cx.new(|cx| InputState::new(window, cx).placeholder("Add a step"));
        if let Some(task) = &task {
            title.update(cx, |t, cx| t.set_value(task.title().to_owned(), window, cx));
            notes.update(cx, |n, cx| n.set_value(task.notes().to_owned(), window, cx));
            if let Some(d) = task.due().and_then(to_chrono) {
                due.update(cx, |p, cx| p.set_date(d, window, cx));
            }
        }

        let subscriptions = vec![
            cx.subscribe_in(&title, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.schedule_save(window, cx),
                InputEvent::PressEnter { .. } => {
                    this.flush(window, cx);
                    this.notes.update(cx, |n, cx| n.focus(window, cx));
                }
                InputEvent::Blur => this.flush(window, cx),
                _ => {}
            }),
            cx.subscribe_in(&notes, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.schedule_save(window, cx),
                InputEvent::Blur => this.flush(window, cx),
                _ => {}
            }),
            cx.subscribe_in(
                &due,
                window,
                |this, _, event: &DatePickerEvent, window, cx| {
                    let DatePickerEvent::Change(value) = event;
                    let date = value.start().and_then(|dt| from_chrono(dt.date()));
                    let id = this.id;
                    notify_err(
                        this.library
                            .update(cx, |lib, cx| lib.set_task_due(id, date, cx)),
                        window,
                        cx,
                    );
                },
            ),
            cx.subscribe_in(&new_subtask, window, |this, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let title = input.read(cx).value().to_string();
                    if title.trim().is_empty() {
                        return;
                    }
                    let id = this.id;
                    let result = this
                        .library
                        .update(cx, |lib, cx| lib.add_subtask(id, &title, cx));
                    if result.is_ok() {
                        input.update(cx, |i, cx| i.set_value("", window, cx));
                    }
                    notify_err(result, window, cx);
                }
            }),
            // Keep status/priority/due in sync with changes made elsewhere
            // (e.g. the checkbox in the list). Text fields are left alone.
            cx.subscribe(&library, |this, library, event, cx| match event {
                LibraryEvent::TasksChanged => {
                    this.task = library.read(cx).tasks_by_id(this.id);
                    this.subtasks = library.read(cx).subtasks(this.id);
                    cx.notify();
                }
                LibraryEvent::FocusChanged => {
                    cx.notify();
                }
                _ => {}
            }),
            // Closing the panel drops this view: save what is still pending.
            cx.on_release(|this, cx| {
                if this.dirty {
                    _ = this.save(cx);
                }
            }),
        ];

        let subtasks = library.read(cx).subtasks(id);
        Self {
            id,
            library,
            task,
            title,
            notes,
            due,
            dirty: false,
            pending_save: None,
            subtasks,
            new_subtask,
            _subscriptions: subscriptions,
        }
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dirty = true;
        self.pending_save = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            _ = this.update_in(cx, |this, window, cx| this.flush(window, cx));
        }));
    }

    fn flush(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_save = None;
        if !self.dirty {
            return;
        }
        if let Err(err) = self.save(cx) {
            feedback::error_toast(format!("{err:#}"), window, cx);
        }
    }

    /// Writes title and notes. A blank title is not saved (the field keeps it
    /// so the user can fix it); notes are saved regardless.
    fn save(&mut self, cx: &mut App) -> anyhow::Result<()> {
        let id = self.id;
        let title = self.title.read(cx).value().to_string();
        let notes = self.notes.read(cx).value().to_string();
        self.library.update(cx, |lib, cx| {
            lib.set_task_notes(id, &notes, cx)?;
            if !title.trim().is_empty() {
                lib.rename_task(id, &title, cx)?;
            }
            anyhow::Ok(())
        })?;
        self.dirty = false;
        Ok(())
    }

    fn set_status(&mut self, status: TaskStatus, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        notify_err(
            self.library
                .update(cx, |lib, cx| lib.set_task_status(id, status, cx)),
            window,
            cx,
        );
    }

    fn set_due_quick(&mut self, due: Option<Date>, window: &mut Window, cx: &mut Context<Self>) {
        self.due.update(cx, |p, cx| match due.and_then(to_chrono) {
            Some(d) => p.set_date(d, window, cx),
            None => p.set_date(PickerDate::Single(None), window, cx),
        });
        let id = self.id;
        notify_err(
            self.library
                .update(cx, |lib, cx| lib.set_task_due(id, due, cx)),
            window,
            cx,
        );
    }

    fn status_buttons(&self, status: TaskStatus, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str,
                      label: &'static str,
                      to: TaskStatus,
                      primary: bool,
                      cx: &mut Context<Self>| {
            Button::new(id)
                .small()
                .label(label)
                .map(|b| if primary { b.primary() } else { b.outline() })
                .on_click(cx.listener(move |this, _, window, cx| this.set_status(to, window, cx)))
        };
        // Exactly the transitions allowed by `TaskStatus::can_transition_to`.
        h_flex().gap_2().flex_wrap().map(|row| match status {
            TaskStatus::Todo => row
                .child(button(
                    "detail-complete",
                    "Complete",
                    TaskStatus::Done,
                    true,
                    cx,
                ))
                .child(button(
                    "detail-start",
                    "Start",
                    TaskStatus::InProgress,
                    false,
                    cx,
                ))
                .child(button(
                    "detail-cancel",
                    "Cancel task",
                    TaskStatus::Cancelled,
                    false,
                    cx,
                )),
            TaskStatus::InProgress => row
                .child(button(
                    "detail-complete",
                    "Complete",
                    TaskStatus::Done,
                    true,
                    cx,
                ))
                .child(button("detail-pause", "Pause", TaskStatus::Todo, false, cx))
                .child(button(
                    "detail-cancel",
                    "Cancel task",
                    TaskStatus::Cancelled,
                    false,
                    cx,
                )),
            TaskStatus::Done | TaskStatus::Cancelled => row.child(button(
                "detail-reopen",
                "Reopen",
                TaskStatus::Todo,
                false,
                cx,
            )),
        })
    }
}

impl TaskDetail {
    /// Steps: a checklist inside the task. Done steps stay, struck through.
    fn render_subtasks(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let done = self
            .subtasks
            .iter()
            .filter(|t| !t.status().is_open())
            .count();
        let label = if self.subtasks.is_empty() {
            "Steps".to_owned()
        } else {
            format!("Steps · {done}/{}", self.subtasks.len())
        };
        v_flex()
            .gap_1()
            .child(section_label(label, cx))
            .children(self.subtasks.iter().map(|sub| {
                let id = sub.id();
                let checked = !sub.status().is_open();
                let library = self.library.clone();
                let remove = self.library.clone();
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Checkbox::new(SharedString::from(format!("step-{id}")))
                            .checked(checked)
                            .on_change(move |_, window, cx| {
                                notify_err(
                                    library.update(cx, |l, cx| l.toggle_task(id, cx)),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .when(checked, |this| {
                                this.line_through().text_color(theme.muted_foreground)
                            })
                            .child(sub.title().to_owned()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("step-{id}-remove")))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Remove step")
                            .on_click(move |_, window, cx| {
                                notify_err(
                                    remove.update(cx, |l, cx| l.delete_subtask(id, cx)),
                                    window,
                                    cx,
                                );
                            }),
                    )
            }))
            .child(
                Input::new(&self.new_subtask).id("new-step").small().prefix(
                    Icon::new(Lucide::Plus)
                        .xsmall()
                        .text_color(theme.muted_foreground),
                ),
            )
    }
}

impl Render for TaskDetail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(task) = self.task.clone() else {
            return v_flex()
                .gap_2()
                .child(div().text_sm().child("This task no longer exists."));
        };
        if task.is_trashed() {
            return v_flex().gap_2().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("This task is in the trash. Restore it to edit."),
            );
        }
        let focus_stats = self
            .library
            .read(cx)
            .focus_stats_for_task(self.id)
            .unwrap_or_default();
        let focus_title = task.title().to_owned();
        let focus_row = h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if focus_stats.sessions == 0 {
                        "No focus sessions yet".to_owned()
                    } else {
                        let word = if focus_stats.sessions == 1 {
                            "session"
                        } else {
                            "sessions"
                        };
                        format!(
                            "{} focused ({} {word})",
                            ottero_core::format_duration(focus_stats.total_secs),
                            focus_stats.sessions
                        )
                    }),
            )
            .when(task.status().is_open(), |row| {
                let id = self.id;
                row.child(
                    Button::new("detail-start-focus")
                        .outline()
                        .xsmall()
                        .icon(Lucide::Timer)
                        .label("Start focus")
                        .on_click(move |_, window, cx| {
                            crate::focus::start_task_focus(id, &focus_title, window, cx);
                        }),
                )
            });

        let today = clock::today();
        let view = cx.entity().downgrade();
        let priority = segmented(
            "detail-priority",
            &[
                (Priority::None, "None"),
                (Priority::Low, "Low"),
                (Priority::Medium, "Medium"),
                (Priority::High, "High"),
            ],
            task.priority(),
            move |p, window, cx| {
                _ = view.update(cx, |this, cx| {
                    let id = this.id;
                    notify_err(
                        this.library
                            .update(cx, |lib, cx| lib.set_task_priority(id, p, cx)),
                        window,
                        cx,
                    );
                });
            },
        );
        let quick =
            |id: &'static str, label: &'static str, due: Option<Date>, cx: &mut Context<Self>| {
                Button::new(id).ghost().xsmall().label(label).on_click(
                    cx.listener(move |this, _, window, cx| this.set_due_quick(due, window, cx)),
                )
            };
        let tomorrow = today.tomorrow().ok();
        let next_week = today.checked_add(jiff::Span::new().days(7)).ok();
        let status_text: SharedString = match task.completed_at() {
            Some(_) => "Done".into(),
            None => task.status().label().into(),
        };

        v_flex()
            .gap_5()
            .child(
                Input::new(&self.title)
                    .id("detail-title")
                    .large()
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label(format!("Status · {status_text}"), cx))
                    .child(self.status_buttons(task.status(), cx)),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label("Due", cx))
                    .child(
                        DatePicker::new(&self.due)
                            .cleanable(true)
                            .placeholder("No due date"),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(quick("due-today", "Today", Some(today), cx))
                            .child(quick("due-tomorrow", "Tomorrow", tomorrow, cx))
                            .child(quick("due-week", "Next week", next_week, cx))
                            .when(task.due().is_some(), |this| {
                                this.child(quick("due-none", "Clear", None, cx))
                            }),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label("Priority", cx))
                    .child(priority),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label("Focus", cx))
                    .child(focus_row),
            )
            .child(self.render_subtasks(cx))
            .child(
                v_flex()
                    .gap_2()
                    .child(section_label("Notes", cx))
                    .child(Textarea::new(&self.notes)),
            )
            .child(
                h_flex().pt_2().child(
                    Button::new("detail-trash")
                        .ghost()
                        .small()
                        .icon(Lucide::Trash)
                        .label("Move to trash")
                        .on_click(cx.listener(|this, _, window, cx| {
                            let id = this.id;
                            let library = this.library.clone();
                            if feedback::report(
                                library.update(cx, |l, cx| l.trash_task(id, cx)),
                                window,
                                cx,
                            )
                            .is_some()
                            {
                                window.close_sheet(cx);
                                feedback::undo_toast(
                                    "Task moved to trash.",
                                    window,
                                    cx,
                                    move |window, cx| {
                                        notify_err(
                                            library.update(cx, |l, cx| l.restore_task(id, cx)),
                                            window,
                                            cx,
                                        );
                                    },
                                );
                            }
                        })),
                ),
            )
    }
}

fn to_chrono(date: Date) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(
        i32::from(date.year()),
        u32::try_from(date.month()).ok()?,
        u32::try_from(date.day()).ok()?,
    )
}

fn from_chrono(date: chrono::NaiveDate) -> Option<Date> {
    Date::new(
        i16::try_from(date.year()).ok()?,
        i8::try_from(date.month()).ok()?,
        i8::try_from(date.day()).ok()?,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_conversion_round_trips() {
        for s in ["2026-11-08", "2024-02-29", "1999-12-31"] {
            let d: Date = s.parse().expect("date");
            assert_eq!(to_chrono(d).and_then(from_chrono), Some(d));
        }
    }
}
