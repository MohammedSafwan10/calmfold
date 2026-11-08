//! Global actions, their default shortcuts, and the macOS menu bar.
//!
//! Every user-facing command is an action so it is reachable by keyboard,
//! menu, and command palette alike.

use gpui_kit::{App, KeyBinding, Menu, MenuItem, actions};

actions!(
    keepstone,
    [
        OpenCommandPalette,
        NewNote,
        NewTask,
        GoToday,
        GoNotes,
        GoTasks,
        GoSettings,
        ToggleSidebar,
        ToggleListPanel,
        UseLightTheme,
        UseDarkTheme,
        UseSystemTheme,
        Quit,
    ]
);

/// `cmd-<keys>` on macOS, `ctrl-<keys>` elsewhere, as a `&'static str`.
macro_rules! shortcut {
    ($keys:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $keys)
        } else {
            concat!("ctrl-", $keys)
        }
    };
}

/// (keystroke, description) pairs shown on the Keyboard settings page.
/// Keep in sync with [`bind`].
pub const SHORTCUTS: &[(&str, &str)] = &[
    (shortcut!("k"), "Open command palette"),
    (shortcut!("n"), "New note"),
    (shortcut!("shift-n"), "New task"),
    (shortcut!("1"), "Go to Today"),
    (shortcut!("2"), "Go to Notes"),
    (shortcut!("3"), "Go to Tasks"),
    (shortcut!(","), "Open settings"),
    (shortcut!("\\"), "Toggle sidebar"),
    (shortcut!("shift-l"), "Toggle note list"),
];

pub fn modifier_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    }
}

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(shortcut!("k"), OpenCommandPalette, None),
        KeyBinding::new(shortcut!("n"), NewNote, None),
        KeyBinding::new(shortcut!("shift-n"), NewTask, None),
        KeyBinding::new(shortcut!("1"), GoToday, None),
        KeyBinding::new(shortcut!("2"), GoNotes, None),
        KeyBinding::new(shortcut!("3"), GoTasks, None),
        KeyBinding::new(shortcut!(","), GoSettings, None),
        KeyBinding::new(shortcut!("\\"), ToggleSidebar, None),
        KeyBinding::new(shortcut!("shift-l"), ToggleListPanel, None),
        KeyBinding::new(shortcut!("q"), Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

pub fn set_menus(cx: &mut App) {
    cx.set_menus([
        Menu::new("Keepstone").items([
            MenuItem::action("Settings…", GoSettings),
            MenuItem::separator(),
            MenuItem::action("Quit Keepstone", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Note", NewNote),
            MenuItem::action("New Task", NewTask),
        ]),
        Menu::new("View").items([
            MenuItem::action("Command Palette", OpenCommandPalette),
            MenuItem::separator(),
            MenuItem::action("Today", GoToday),
            MenuItem::action("Notes", GoNotes),
            MenuItem::action("Tasks", GoTasks),
            MenuItem::separator(),
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Note List", ToggleListPanel),
        ]),
    ]);
}
