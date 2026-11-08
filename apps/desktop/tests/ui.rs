#![allow(clippy::expect_used)] // Test helpers outside #[test] fns.

//! UI integration tests: the real Keepstone shell in a headless window.
//!
//! Each test starts the app exactly like `main` does (`keepstone_desktop::start`)
//! with a temporary data folder and an in-memory database, then drives it with
//! actions, typing and key presses, and checks the stored result.
//!
//! Run: `cargo test -p keepstone-desktop --test ui`

use std::time::Duration;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use keepstone_core::TaskStatus;
use keepstone_desktop::actions::{
    GoNotes, GoSettings, GoTasks, GoToday, NewNote, NewTask, OpenCommandPalette, ToggleSidebar,
    UseDarkTheme, UseLightTheme,
};
use keepstone_desktop::paths::AppPaths;
use keepstone_desktop::shell::AppShell;
use keepstone_desktop::views::Route;
use keepstone_desktop::views::tasks::TaskTab;
use keepstone_storage::{Store, TaskFilter};

struct App {
    window: AnyWindowHandle,
    shell: Entity<AppShell>,
    _dir: tempfile::TempDir,
    paths: AppPaths,
}

fn launch(cx: &mut TestAppContext) -> App {
    let dir = tempfile::tempdir().expect("tempdir");
    let paths = AppPaths::in_dir(dir.path().to_path_buf());
    let store = Store::open_in_memory().expect("store");
    let (window, shell) =
        cx.update(|cx| keepstone_desktop::start(&paths, store, cx).expect("start"));
    cx.run_until_parked();
    App {
        window,
        shell,
        _dir: dir,
        paths,
    }
}

fn dispatch(app: &App, cx: &mut TestAppContext, action: impl gpui_kit::Action) {
    cx.update_window(app.window, |_, window, cx| {
        window.dispatch_action(Box::new(action), cx)
    })
    .expect("window");
    cx.run_until_parked();
}

fn type_text(app: &App, cx: &mut TestAppContext, text: &str) {
    cx.update_window(app.window, |_, window, cx| window.input(text, cx))
        .expect("window");
    cx.run_until_parked();
}

fn press(app: &App, cx: &mut TestAppContext, key: &str) {
    cx.update_window(app.window, |_, window, cx| window.press(key, cx))
        .expect("window");
    cx.run_until_parked();
}

fn route(app: &App, cx: &mut TestAppContext) -> Route {
    cx.read(|cx| app.shell.read(cx).route())
}

#[gpui_kit::test]
fn starts_on_today_and_navigates_with_actions(cx: &mut TestAppContext) {
    let app = launch(cx);
    assert_eq!(route(&app, cx), Route::Today);
    dispatch(&app, cx, GoNotes);
    assert_eq!(route(&app, cx), Route::Notes);
    dispatch(&app, cx, GoTasks);
    assert_eq!(route(&app, cx), Route::Tasks);
    dispatch(&app, cx, GoSettings);
    assert_eq!(route(&app, cx), Route::Settings);
    dispatch(&app, cx, GoToday);
    assert_eq!(route(&app, cx), Route::Today);
}

#[gpui_kit::test]
fn new_note_typing_autosaves_with_links(cx: &mut TestAppContext) {
    let app = launch(cx);
    // A target note, so the link has somewhere to point.
    cx.update(|cx| {
        app.shell.read(cx).library().clone().update(cx, |lib, cx| {
            let id = lib.create_note("Service log", cx).expect("create");
            lib.edit_note(id, "Service log", "Oil changed", cx)
                .expect("edit");
        });
    });

    dispatch(&app, cx, NewNote);
    assert_eq!(route(&app, cx), Route::Notes);

    // The title is focused on a new note; Enter moves to the body.
    type_text(&app, cx, "Car");
    press(&app, cx, "enter");
    type_text(&app, cx, "Insurance renews in May. See [[Service log]].");

    // Nothing is written before the autosave delay...
    let titles = |cx: &mut TestAppContext| {
        cx.read(|cx| {
            app.shell
                .read(cx)
                .library()
                .read(cx)
                .notes(false)
                .expect("notes")
                .into_iter()
                .map(|n| n.title)
                .collect::<Vec<_>>()
        })
    };
    assert!(!titles(cx).contains(&"Car".to_owned()), "saved too early");

    // ...and everything is written after it.
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert!(
        titles(cx).contains(&"Car".to_owned()),
        "autosave did not run"
    );

    let backlinks = cx.read(|cx| {
        app.shell
            .read(cx)
            .library()
            .read(cx)
            .backlinks("service log")
            .expect("backlinks")
    });
    assert_eq!(backlinks.len(), 1);
    assert_eq!(backlinks[0].title, "Car");
    let body = cx.read(|cx| {
        let lib = app.shell.read(cx).library().read(cx);
        lib.note(backlinks[0].id)
            .expect("load")
            .expect("exists")
            .body()
            .to_owned()
    });
    assert_eq!(body, "Insurance renews in May. See [[Service log]].");
}

#[gpui_kit::test]
fn switching_notes_flushes_unsaved_typing(cx: &mut TestAppContext) {
    let app = launch(cx);
    dispatch(&app, cx, NewNote);
    type_text(&app, cx, "First");
    // Create another note immediately, before the autosave timer fires.
    dispatch(&app, cx, NewNote);
    let titles = cx.read(|cx| {
        app.shell
            .read(cx)
            .library()
            .read(cx)
            .notes(false)
            .expect("notes")
            .into_iter()
            .map(|n| n.title)
            .collect::<Vec<_>>()
    });
    assert!(
        titles.contains(&"First".to_owned()),
        "typing lost on switch: {titles:?}"
    );
}

#[gpui_kit::test]
fn quick_add_creates_task_due_today_and_clears_input(cx: &mut TestAppContext) {
    let app = launch(cx);
    dispatch(&app, cx, NewTask);
    assert_eq!(route(&app, cx), Route::Tasks);
    assert_eq!(
        cx.read(|cx| app.shell.read(cx).tasks().read(cx).tab()),
        TaskTab::Today
    );

    type_text(&app, cx, "Pay rent");
    press(&app, cx, "enter");
    // Blank input does nothing.
    press(&app, cx, "enter");

    let today = keepstone_core::clock::today();
    let tasks = cx.read(|cx| {
        app.shell
            .read(cx)
            .library()
            .read(cx)
            .tasks(TaskFilter::Today(today))
            .expect("tasks")
    });
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title(), "Pay rent");
    assert_eq!(tasks[0].due(), Some(today));

    // Completing it moves it out of Today and into Done.
    let id = tasks[0].id();
    cx.update(|cx| {
        app.shell
            .read(cx)
            .library()
            .clone()
            .update(cx, |lib, cx| lib.toggle_task(id, cx))
            .expect("toggle");
    });
    let lib_tasks = |filter, cx: &mut TestAppContext| {
        cx.read(|cx| {
            app.shell
                .read(cx)
                .library()
                .read(cx)
                .tasks(filter)
                .expect("tasks")
        })
    };
    assert!(lib_tasks(TaskFilter::Today(today), cx).is_empty());
    let done = lib_tasks(TaskFilter::Closed, cx);
    assert_eq!(done[0].status(), TaskStatus::Done);
}

#[gpui_kit::test]
fn sidebar_toggle_persists_layout(cx: &mut TestAppContext) {
    let app = launch(cx);
    assert!(!cx.read(|cx| app.shell.read(cx).layout().sidebar_collapsed));
    dispatch(&app, cx, ToggleSidebar);
    assert!(cx.read(|cx| app.shell.read(cx).layout().sidebar_collapsed));

    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    let saved = keepstone_desktop::ui_state::LayoutState::load(
        &keepstone_desktop::ui_state::layout_file(&app.paths.data_dir),
    );
    assert!(saved.sidebar_collapsed, "collapsed state written to disk");
}

#[gpui_kit::test]
fn theme_commands_switch_mode_and_save(cx: &mut TestAppContext) {
    let app = launch(cx);
    dispatch(&app, cx, UseDarkTheme);
    assert_eq!(cx.read(|cx| Theme::global(cx).mode), ThemeMode::Dark);
    assert_eq!(
        cx.read(|cx| Theme::global(cx).theme_name().to_string()),
        keepstone_ui::theme::DARK
    );
    dispatch(&app, cx, UseLightTheme);
    assert_eq!(cx.read(|cx| Theme::global(cx).mode), ThemeMode::Light);

    let saved = keepstone_ui::appearance::AppearanceSettings::load(&app.paths.appearance);
    assert_eq!(
        saved.theme,
        keepstone_ui::appearance::ThemePreference::Light
    );
}

#[gpui_kit::test]
fn command_palette_opens_and_escape_closes_it(cx: &mut TestAppContext) {
    let app = launch(cx);
    let dialog_open = |cx: &mut TestAppContext| {
        cx.update_window(app.window, |_, window, cx| {
            gpui_kit::component::WindowExt::has_active_dialog(window, cx)
        })
        .expect("window")
    };
    assert!(!dialog_open(cx));
    dispatch(&app, cx, OpenCommandPalette);
    assert!(dialog_open(cx));
    press(&app, cx, "escape");
    assert!(!dialog_open(cx));
}

fn click(app: &App, cx: &mut TestAppContext, id: &'static str) {
    cx.update_window(app.window, |_, window, cx| window.click(id, cx))
        .expect("window");
    cx.run_until_parked();
}

fn dialog_open(app: &App, cx: &mut TestAppContext) -> bool {
    cx.update_window(app.window, |_, window, cx| {
        gpui_kit::component::WindowExt::has_active_dialog(window, cx)
    })
    .expect("window")
}

fn create_and_open_note(app: &App, cx: &mut TestAppContext, title: &str) -> keepstone_core::NoteId {
    let id = cx.update(|cx| {
        app.shell.read(cx).library().clone().update(cx, |lib, cx| {
            let id = lib.create_note(title, cx).expect("create");
            lib.edit_note(id, title, "body", cx).expect("edit");
            id
        })
    });
    cx.update_window(app.window, |_, window, cx| {
        app.shell
            .update(cx, |shell, cx| shell.open_note(id, window, cx));
    })
    .expect("window");
    cx.run_until_parked();
    id
}

fn note_state(app: &App, cx: &mut TestAppContext, id: keepstone_core::NoteId) -> Option<bool> {
    cx.read(|cx| {
        app.shell
            .read(cx)
            .library()
            .read(cx)
            .note(id)
            .expect("load")
            .map(|n| n.is_trashed())
    })
}

#[gpui_kit::test]
fn trash_is_immediate_and_undo_restores(cx: &mut TestAppContext) {
    let app = launch(cx);
    // Toasts fade in on a wall-clock animation; reduced motion shows them at once.
    cx.update(|cx| cx.set_reduce_motion(true));
    let id = create_and_open_note(&app, cx, "Groceries");

    click(&app, cx, "note-trash");
    assert!(
        !dialog_open(&app, cx),
        "reversible actions must not ask for confirmation"
    );
    assert_eq!(note_state(&app, cx, id), Some(true));

    click(&app, cx, "undo");
    assert_eq!(
        note_state(&app, cx, id),
        Some(false),
        "Undo restores the note"
    );
}

#[gpui_kit::test]
fn delete_forever_requires_confirmation(cx: &mut TestAppContext) {
    let app = launch(cx);
    let id = create_and_open_note(&app, cx, "Old draft");
    click(&app, cx, "note-trash");
    // Open the trashed note (read-only view shows Restore / Delete forever).
    cx.update_window(app.window, |_, window, cx| {
        app.shell
            .update(cx, |shell, cx| shell.open_note(id, window, cx));
    })
    .expect("window");
    cx.run_until_parked();

    click(&app, cx, "note-delete");
    assert!(dialog_open(&app, cx), "permanent delete asks first");
    press(&app, cx, "escape");
    assert!(!dialog_open(&app, cx));
    assert_eq!(
        note_state(&app, cx, id),
        Some(true),
        "Escape keeps the note"
    );

    click(&app, cx, "note-delete");
    press(&app, cx, "enter");
    assert!(!dialog_open(&app, cx));
    assert_eq!(note_state(&app, cx, id), None, "confirming deletes it");
}
