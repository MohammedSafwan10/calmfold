//! Global actions, their default shortcuts, and the macOS menu bar.
//!
//! Every user-facing command is an action so it is reachable by keyboard,
//! menu, and command palette alike.

use gpui_kit::{Action, App, KeyBinding, Menu, MenuItem, actions};

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
        ToggleMarkdownSource,
        OpenDailyNote,
        NewProject,
        ImportNotes,
        ExportNotes,
        UseLightTheme,
        UseDarkTheme,
        UseSystemTheme,
        Quit,
    ]
);

// Formatting in the note editor (bound only inside it, context "NoteEditor").
actions!(
    keepstone_format,
    [
        FormatBold,
        FormatItalic,
        FormatStrikethrough,
        FormatCode,
        FormatHighlight,
        FormatLink,
        FormatHeading1,
        FormatHeading2,
        FormatHeading3,
        FormatParagraph,
        FormatBullets,
        FormatNumbers,
        FormatChecklist,
        FormatQuote,
        ToggleCheckbox,
    ]
);

/// Key context set on the note editor; format shortcuts only work there.
pub const NOTE_EDITOR_CONTEXT: &str = "NoteEditor";

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

/// Groups on the Keyboard settings page, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutGroup {
    General,
    Create,
    Navigate,
    View,
}

impl ShortcutGroup {
    pub const ALL: [Self; 4] = [Self::General, Self::Create, Self::Navigate, Self::View];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Create => "Create",
            Self::Navigate => "Go to",
            Self::View => "View",
        }
    }
}

/// One app-wide shortcut. This table is the single source of truth: key
/// bindings, the Keyboard settings page and command-palette hints all read it.
pub struct Shortcut {
    /// Platform keystroke (`cmd-…` on macOS, `ctrl-…` elsewhere).
    pub keys: &'static str,
    pub label: &'static str,
    pub group: ShortcutGroup,
    action: fn() -> Box<dyn Action>,
}

impl Shortcut {
    #[must_use]
    pub fn action(&self) -> Box<dyn Action> {
        (self.action)()
    }
}

macro_rules! shortcuts {
    ($( $group:ident, $keys:literal, $label:literal, $action:expr; )*) => {
        &[ $( Shortcut {
            keys: shortcut!($keys),
            label: $label,
            group: ShortcutGroup::$group,
            action: || Box::new($action),
        }, )* ]
    };
}

pub const SHORTCUTS: &[Shortcut] = shortcuts![
    General, "k", "Search and commands", OpenCommandPalette;
    General, ",", "Settings", GoSettings;
    Create, "n", "New note", NewNote;
    Create, "shift-n", "New task", NewTask;
    Create, "shift-d", "Today's note", OpenDailyNote;
    Navigate, "1", "Today", GoToday;
    Navigate, "2", "Notes", GoNotes;
    Navigate, "3", "Tasks", GoTasks;
    View, "\\", "Toggle sidebar", ToggleSidebar;
    View, "shift-l", "Toggle note list", ToggleListPanel;
];

/// Formatting shortcuts, active only while typing in a note (Bear/Notion keys).
pub const FORMAT_SHORTCUTS: &[Shortcut] = shortcuts![
    General, "b", "Bold", FormatBold;
    General, "i", "Italic", FormatItalic;
    General, "shift-x", "Strikethrough", FormatStrikethrough;
    General, "e", "Inline code", FormatCode;
    General, "shift-h", "Highlight", FormatHighlight;
    General, "shift-k", "Link", FormatLink;
    General, "alt-1", "Heading 1", FormatHeading1;
    General, "alt-2", "Heading 2", FormatHeading2;
    General, "alt-3", "Heading 3", FormatHeading3;
    General, "alt-0", "Body text", FormatParagraph;
    General, "shift-8", "Bulleted list", FormatBullets;
    General, "shift-7", "Numbered list", FormatNumbers;
    General, "shift-9", "Checklist", FormatChecklist;
    General, "enter", "Check / uncheck item", ToggleCheckbox;
];

/// Built-in editing keys of the note editor (zorite-editor), shown for reference only.
pub const EDITING_SHORTCUTS: &[(&str, &str)] = &[
    (shortcut!("z"), "Undo"),
    (shortcut!("shift-z"), "Redo"),
    (shortcut!("a"), "Select all"),
];

/// The shortcut bound to `action`, for showing a hint next to it.
#[must_use]
pub fn shortcut_for(action: &dyn Action) -> Option<&'static str> {
    SHORTCUTS
        .iter()
        .find(|s| s.action().name() == action.name())
        .map(|s| s.keys)
}

pub fn modifier_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    }
}

pub fn bind(cx: &mut App) {
    let mut bindings: Vec<KeyBinding> = SHORTCUTS
        .iter()
        .map(|s| KeyBinding::load(s.keys, s.action(), None, false, None, cx))
        .filter_map(Result::ok)
        .collect();
    let editor = gpui_kit::KeyBindingContextPredicate::parse(NOTE_EDITOR_CONTEXT)
        .ok()
        .map(std::rc::Rc::new);
    bindings.extend(FORMAT_SHORTCUTS.iter().filter_map(|s| {
        KeyBinding::load(s.keys, s.action(), editor.clone(), false, None, cx).ok()
    }));
    // Quitting by shortcut is a macOS convention; Windows and Linux close the window.
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-q", Quit, None));
    }
    cx.bind_keys(bindings);
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
            MenuItem::action("New Project…", NewProject),
            MenuItem::action("Today's Note", OpenDailyNote),
            MenuItem::separator(),
            MenuItem::action("Import Markdown Folder…", ImportNotes),
            MenuItem::action("Export Notes as Markdown…", ExportNotes),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_parses_and_is_unique() {
        let mut seen = std::collections::HashSet::new();
        for s in SHORTCUTS {
            assert!(
                gpui_kit::Keystroke::parse(s.keys).is_ok(),
                "{} does not parse",
                s.keys
            );
            assert!(seen.insert(s.keys), "{} is bound twice", s.keys);
            assert_eq!(shortcut_for(&*s.action()), Some(s.keys));
        }
        let mut format_keys = std::collections::HashSet::new();
        for s in FORMAT_SHORTCUTS {
            assert!(
                gpui_kit::Keystroke::parse(s.keys).is_ok(),
                "{} does not parse",
                s.keys
            );
            assert!(format_keys.insert(s.keys), "{} is bound twice", s.keys);
            assert!(
                !seen.contains(s.keys),
                "{} clashes with an app shortcut",
                s.keys
            );
        }
        for (keys, _) in EDITING_SHORTCUTS {
            assert!(gpui_kit::Keystroke::parse(keys).is_ok());
            assert!(!seen.contains(keys), "{keys} clashes with an editor key");
        }
    }

    #[test]
    fn uses_the_platform_modifier() {
        let expected = if cfg!(target_os = "macos") {
            "cmd-"
        } else {
            "ctrl-"
        };
        assert!(SHORTCUTS.iter().all(|s| s.keys.starts_with(expected)));
    }
}
