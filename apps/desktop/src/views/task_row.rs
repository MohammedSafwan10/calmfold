//! One task row, shared by the Today and Tasks screens.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, Window, div,
};
use keepstone_core::{Date, Priority, Task, TaskId, TaskStatus};

use keepstone_ui::feedback::{self, Confirm};

use crate::library::Library;
use crate::views::relative_day;
use crate::views::task_detail::open_task_detail;

/// Renders `task`. All mutations go through `library`; errors become notifications.
pub fn task_row(task: &Task, today: Date, library: &Entity<Library>, cx: &App) -> impl IntoElement {
    let id = task.id();
    let done = !task.status().is_open();
    let trashed = task.is_trashed();
    let theme = cx.theme();
    let key = SharedString::from(format!("task-{id}"));

    let due_label = task.due().map(|due| {
        let overdue = task.is_overdue(today);
        let text = relative_day(due, today);
        h_flex()
            .gap_1()
            .text_xs()
            .text_color(if overdue {
                theme.danger
            } else {
                theme.muted_foreground
            })
            .child(Icon::new(Lucide::CalendarDays).xsmall())
            .child(if overdue {
                format!("Overdue · {text}")
            } else {
                text
            })
    });

    let priority_label = (task.priority() != Priority::None).then(|| {
        h_flex()
            .gap_1()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(Lucide::Flag).xsmall())
            .child(task.priority().label())
    });

    let status_label = (task.status() == TaskStatus::InProgress).then(|| {
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child("In progress")
    });

    h_flex()
        .id(key.clone())
        .w_full()
        .items_start()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(theme.radius)
        .hover(|this| this.bg(theme.list_hover))
        .child(
            div().pt_0p5().child(
                Checkbox::new(SharedString::from(format!("{key}-check")))
                    .checked(done)
                    .disabled(trashed)
                    .on_change({
                        let library = library.clone();
                        move |_, window, cx| {
                            notify_err(
                                library.update(cx, |lib, cx| lib.toggle_task(id, cx)),
                                window,
                                cx,
                            );
                        }
                    }),
            ),
        )
        .child(
            v_flex()
                .id(SharedString::from(format!("{key}-open")))
                // An accessible button: screen readers (and UI tests) see "Open <title>".
                .role(gpui_kit::Role::Button)
                .aria_label(SharedString::from(format!("Open {}", task.title())))
                .on_click({
                    let library = library.clone();
                    move |_, window, cx| open_task_detail(id, library.clone(), window, cx)
                })
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .when(done, |this| {
                            this.line_through().text_color(theme.muted_foreground)
                        })
                        .child(task.title().to_owned()),
                )
                .when(
                    due_label.is_some() || priority_label.is_some() || status_label.is_some(),
                    |this| {
                        this.child(
                            h_flex()
                                .gap_3()
                                .children(status_label)
                                .children(due_label)
                                .children(priority_label),
                        )
                    },
                )
                .test_support(),
        )
        .child(task_menu(
            &key,
            id,
            task.priority(),
            task.status(),
            trashed,
            today,
            library,
        ))
}

fn task_menu(
    key: &SharedString,
    id: TaskId,
    priority: Priority,
    status: TaskStatus,
    trashed: bool,
    today: Date,
    library: &Entity<Library>,
) -> impl IntoElement {
    let library = library.clone();
    Button::new(SharedString::from(format!("{key}-menu")))
        .ghost()
        .xsmall()
        .icon(Lucide::Ellipsis)
        .tooltip("Task actions")
        .dropdown_menu(move |menu, _, _| {
            let lib = library.clone();
            let item = move |label: &'static str,
                             f: fn(
                &mut Library,
                TaskId,
                Date,
                &mut gpui_kit::Context<Library>,
            ) -> anyhow::Result<()>| {
                let lib = lib.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    notify_err(lib.update(cx, |l, cx| f(l, id, today, cx)), window, cx);
                })
            };
            if trashed {
                let lib = library.clone();
                return menu
                    .item(item("Restore", |l, id, _, cx| l.restore_task(id, cx)))
                    .separator()
                    .item(
                        PopupMenuItem::new("Delete forever…").on_click(move |_, window, cx| {
                            let lib = lib.clone();
                            feedback::confirm_destructive(
                                Confirm::new(
                                    "Delete this task forever?",
                                    "This can't be undone.",
                                    "Delete",
                                ),
                                window,
                                cx,
                                move |window, cx| {
                                    notify_err(
                                        lib.update(cx, |l, cx| l.delete_task_permanently(id, cx)),
                                        window,
                                        cx,
                                    );
                                },
                            );
                        }),
                    );
            }
            let check = |p: Priority| priority == p;
            let lib = library.clone();
            menu.item(
                PopupMenuItem::new("Details…").on_click(move |_, window, cx| {
                    open_task_detail(id, lib.clone(), window, cx);
                }),
            )
            .separator()
            .label("Due")
            .item(item("Today", |l, id, today, cx| {
                l.set_task_due(id, Some(today), cx)
            }))
            .item(item("Tomorrow", |l, id, today, cx| {
                l.set_task_due(id, Some(today.tomorrow()?), cx)
            }))
            .item(item("Next week", |l, id, today, cx| {
                l.set_task_due(id, Some(today.checked_add(jiff::Span::new().days(7))?), cx)
            }))
            .item(item("No date", |l, id, _, cx| l.set_task_due(id, None, cx)))
            .separator()
            .label("Priority")
            .item(
                item("High", |l, id, _, cx| {
                    l.set_task_priority(id, Priority::High, cx)
                })
                .checked(check(Priority::High)),
            )
            .item(
                item("Medium", |l, id, _, cx| {
                    l.set_task_priority(id, Priority::Medium, cx)
                })
                .checked(check(Priority::Medium)),
            )
            .item(
                item("Low", |l, id, _, cx| {
                    l.set_task_priority(id, Priority::Low, cx)
                })
                .checked(check(Priority::Low)),
            )
            .item(
                item("None", |l, id, _, cx| {
                    l.set_task_priority(id, Priority::None, cx)
                })
                .checked(check(Priority::None)),
            )
            .separator()
            // Only offer transitions the state machine allows from the current status.
            .when(status == TaskStatus::Todo, |m| {
                m.item(item("Start", |l, id, _, cx| {
                    l.set_task_status(id, TaskStatus::InProgress, cx)
                }))
            })
            .when(status == TaskStatus::InProgress, |m| {
                m.item(item("Pause", |l, id, _, cx| {
                    l.set_task_status(id, TaskStatus::Todo, cx)
                }))
            })
            .when(status.is_open(), |m| {
                m.item(item("Cancel task", |l, id, _, cx| {
                    l.set_task_status(id, TaskStatus::Cancelled, cx)
                }))
            })
            .when(!status.is_open(), |m| {
                m.item(item("Reopen", |l, id, _, cx| {
                    l.set_task_status(id, TaskStatus::Todo, cx)
                }))
            })
            .separator()
            .item({
                let lib = library.clone();
                PopupMenuItem::new("Move to trash").on_click(move |_, window, cx| {
                    if feedback::report(lib.update(cx, |l, cx| l.trash_task(id, cx)), window, cx)
                        .is_some()
                    {
                        let lib = lib.clone();
                        feedback::undo_toast(
                            "Task moved to trash.",
                            window,
                            cx,
                            move |window, cx| {
                                notify_err(
                                    lib.update(cx, |l, cx| l.restore_task(id, cx)),
                                    window,
                                    cx,
                                );
                            },
                        );
                    }
                })
            })
        })
}

/// Shows an error as a notification instead of failing silently.
pub fn notify_err<T>(result: anyhow::Result<T>, window: &mut Window, cx: &mut App) {
    feedback::report(result, window, cx);
}
