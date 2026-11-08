//! One task row, shared by the Today and Tasks screens.

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
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

    let menu = task_menu_builder(
        id,
        task.priority(),
        task.status(),
        task.due(),
        trashed,
        today,
        library,
    );

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
        .child(menu_button(&key, menu.clone()))
        .context_menu(move |m, window, cx| menu(m, window, cx))
}

/// Row menu. Organised as: open → set (due, priority as submenus showing the
/// current value) → status → trash. Only valid status changes are listed.
#[allow(clippy::too_many_arguments)]
fn task_menu_builder(
    id: TaskId,
    priority: Priority,
    status: TaskStatus,
    due: Option<Date>,
    trashed: bool,
    today: Date,
    library: &Entity<Library>,
) -> MenuBuilder {
    let library = library.clone();
    Rc::new(
        move |menu: PopupMenu, window: &mut Window, cx: &mut gpui_kit::Context<PopupMenu>| {
            // One menu item that runs a Library change and reports errors.
            let action = |lib: &Entity<Library>,
                          label: &'static str,
                          icon: Option<Lucide>,
                          f: fn(
                &mut Library,
                TaskId,
                Date,
                &mut gpui_kit::Context<Library>,
            ) -> anyhow::Result<()>| {
                let lib = lib.clone();
                PopupMenuItem::new(label)
                    .when_some(icon, |item, icon| item.icon(icon))
                    .on_click(move |_, window, cx| {
                        notify_err(lib.update(cx, |l, cx| f(l, id, today, cx)), window, cx);
                    })
            };

            if trashed {
                let lib = library.clone();
                return menu
                    .item(action(
                        &library,
                        "Restore",
                        Some(Lucide::ArchiveRestore),
                        |l, id, _, cx| l.restore_task(id, cx),
                    ))
                    .separator()
                    .item(
                        PopupMenuItem::new("Delete forever…")
                            .icon(Lucide::Trash)
                            .on_click(move |_, window, cx| {
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
                                            lib.update(cx, |l, cx| {
                                                l.delete_task_permanently(id, cx)
                                            }),
                                            window,
                                            cx,
                                        );
                                    },
                                );
                            }),
                    );
            }

            let due_label = match due {
                Some(d) => format!("Due: {}", relative_day(d, today)),
                None => "Due date".to_owned(),
            };
            let priority_label = match priority {
                Priority::None => "Priority".to_owned(),
                p => format!("Priority: {}", p.label()),
            };
            let tomorrow = today.tomorrow().ok();
            let next_week = today.checked_add(jiff::Span::new().days(7)).ok();

            let details_lib = library.clone();
            let due_lib = library.clone();
            let prio_lib = library.clone();
            let trash_lib = library.clone();

            menu.item(
                PopupMenuItem::new("Open details")
                    .icon(Lucide::PanelRightOpen)
                    .on_click(move |_, window, cx| {
                        open_task_detail(id, details_lib.clone(), window, cx)
                    }),
            )
            .separator()
            .submenu_with_icon(
                Some(Lucide::CalendarDays.into()),
                due_label,
                window,
                cx,
                move |sub, _, _| {
                    let set = |label: &'static str, value: Option<Date>| {
                        let lib = due_lib.clone();
                        PopupMenuItem::new(label)
                            .checked(due == value && (value.is_some() || label == "No date"))
                            .on_click(move |_, window, cx| {
                                notify_err(
                                    lib.update(cx, |l, cx| l.set_task_due(id, value, cx)),
                                    window,
                                    cx,
                                );
                            })
                    };
                    sub.item(set("Today", Some(today)))
                        .item(set("Tomorrow", tomorrow))
                        .item(set("Next week", next_week))
                        .separator()
                        .item(set("No date", None))
                },
            )
            .submenu_with_icon(
                Some(Lucide::Flag.into()),
                priority_label,
                window,
                cx,
                move |sub, _, _| {
                    let set = |p: Priority| {
                        let lib = prio_lib.clone();
                        PopupMenuItem::new(p.label())
                            .checked(priority == p)
                            .on_click(move |_, window, cx| {
                                notify_err(
                                    lib.update(cx, |l, cx| l.set_task_priority(id, p, cx)),
                                    window,
                                    cx,
                                );
                            })
                    };
                    sub.item(set(Priority::High))
                        .item(set(Priority::Medium))
                        .item(set(Priority::Low))
                        .separator()
                        .item(set(Priority::None))
                },
            )
            .separator()
            // Only transitions the state machine allows from the current status.
            .when(status == TaskStatus::Todo, |m| {
                m.item(action(
                    &library,
                    "Start",
                    Some(Lucide::Play),
                    |l, id, _, cx| l.set_task_status(id, TaskStatus::InProgress, cx),
                ))
            })
            .when(status == TaskStatus::InProgress, |m| {
                m.item(action(
                    &library,
                    "Pause",
                    Some(Lucide::Pause),
                    |l, id, _, cx| l.set_task_status(id, TaskStatus::Todo, cx),
                ))
            })
            .when(status.is_open(), |m| {
                m.item(action(
                    &library,
                    "Cancel task",
                    Some(Lucide::Ban),
                    |l, id, _, cx| l.set_task_status(id, TaskStatus::Cancelled, cx),
                ))
            })
            .when(!status.is_open(), |m| {
                m.item(action(
                    &library,
                    "Reopen",
                    Some(Lucide::RotateCcw),
                    |l, id, _, cx| l.set_task_status(id, TaskStatus::Todo, cx),
                ))
            })
            .separator()
            .item(
                PopupMenuItem::new("Move to trash")
                    .icon(Lucide::Trash)
                    .on_click(move |_, window, cx| {
                        if feedback::report(
                            trash_lib.update(cx, |l, cx| l.trash_task(id, cx)),
                            window,
                            cx,
                        )
                        .is_some()
                        {
                            let lib = trash_lib.clone();
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
                    }),
            )
        },
    )
}

/// The same menu backs the "…" button and right-click on the row.
pub type MenuBuilder =
    Rc<dyn Fn(PopupMenu, &mut Window, &mut gpui_kit::Context<PopupMenu>) -> PopupMenu>;

fn menu_button(key: &SharedString, build: MenuBuilder) -> impl IntoElement {
    Button::new(SharedString::from(format!("{key}-menu")))
        .ghost()
        .xsmall()
        .icon(Lucide::Ellipsis)
        .tooltip("Task actions")
        .dropdown_menu(move |menu, window, cx| build(menu, window, cx))
}

/// Shows an error as a notification instead of failing silently.
pub fn notify_err<T>(result: anyhow::Result<T>, window: &mut Window, cx: &mut App) {
    feedback::report(result, window, cx);
}
