//! Sidebar navigation, as data.
//!
//! To add a feature to the sidebar, add a [`NavEntry`] to [`SECTIONS`] (and a
//! [`Route`] for it). Children are optional deep links into the screen. Only
//! list features that exist: no placeholders for unbuilt screens.

use gpui_kit::assets::IconName as Lucide;

use calmfold_core::vault::ItemKind;

use crate::vault::VaultFilter;
use crate::views::Route;
use crate::views::tasks::TaskTab;

/// A deep link inside a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavTarget {
    Route(Route),
    Tasks(TaskTab),
    NotesAll,
    NotesTrash,
    DailyNote,
    Project(calmfold_core::ProjectId),
    Vault(crate::vault::VaultFilter),
}

/// Which count badge an entry shows, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    None,
    /// Open tasks due today or overdue.
    TasksDueNow,
    /// Habits scheduled today and not done yet.
    HabitsLeft,
}

pub struct NavChild {
    pub label: &'static str,
    pub target: NavTarget,
    pub badge: Badge,
}

pub struct NavEntry {
    pub id: &'static str,
    pub default_visible: bool,
    pub action: fn() -> Box<dyn gpui_kit::Action>,
    pub label: &'static str,
    pub icon: Lucide,
    pub route: Route,
    pub badge: Badge,
    pub children: &'static [NavChild],
    /// Children generated from data, shown after the static ones.
    pub dynamic: Option<DynamicChildren>,
}

/// Data-driven sidebar children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicChildren {
    /// One child per project, with its open-task count.
    Projects,
    /// The most used vault tags (only while unlocked: tags are encrypted).
    VaultTags,
}

pub struct NavSection {
    pub label: &'static str,
    pub entries: &'static [NavEntry],
}

pub const SECTIONS: &[NavSection] = &[NavSection {
    label: "Workspace",
    entries: &[
        NavEntry {
            id: "today",
            default_visible: true,
            action: || Box::new(crate::actions::GoToday),
            label: "Today",
            icon: Lucide::House,
            route: Route::Today,
            badge: Badge::TasksDueNow,
            children: &[],
            dynamic: None,
        },
        NavEntry {
            id: "notes",
            default_visible: true,
            action: || Box::new(crate::actions::GoNotes),
            label: "Notes",
            icon: Lucide::NotebookPen,
            route: Route::Notes,
            badge: Badge::None,
            children: &[
                NavChild {
                    label: "All notes",
                    target: NavTarget::NotesAll,
                    badge: Badge::None,
                },
                NavChild {
                    label: "Today's note",
                    target: NavTarget::DailyNote,
                    badge: Badge::None,
                },
                NavChild {
                    label: "Trash",
                    target: NavTarget::NotesTrash,
                    badge: Badge::None,
                },
            ],
            dynamic: None,
        },
        NavEntry {
            id: "tasks",
            default_visible: true,
            action: || Box::new(crate::actions::GoTasks),
            label: "Tasks",
            icon: Lucide::ListTodo,
            route: Route::Tasks,
            badge: Badge::None,
            children: &[
                NavChild {
                    label: "Today",
                    target: NavTarget::Tasks(TaskTab::Today),
                    badge: Badge::TasksDueNow,
                },
                NavChild {
                    label: "Upcoming",
                    target: NavTarget::Tasks(TaskTab::Upcoming),
                    badge: Badge::None,
                },
                NavChild {
                    label: "All open",
                    target: NavTarget::Tasks(TaskTab::All),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Done",
                    target: NavTarget::Tasks(TaskTab::Done),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Trash",
                    target: NavTarget::Tasks(TaskTab::Trash),
                    badge: Badge::None,
                },
            ],
            dynamic: Some(DynamicChildren::Projects),
        },
        NavEntry {
            id: "calendar",
            default_visible: true,
            action: || Box::new(crate::actions::GoCalendar),
            label: "Calendar",
            icon: Lucide::CalendarDays,
            route: Route::Calendar,
            badge: Badge::None,
            children: &[],
            dynamic: None,
        },
        NavEntry {
            id: "habits",
            default_visible: false,
            action: || Box::new(crate::actions::GoHabits),
            label: "Habits",
            icon: Lucide::Sprout,
            route: Route::Habits,
            badge: Badge::HabitsLeft,
            children: &[],
            dynamic: None,
        },
        NavEntry {
            id: "focus",
            default_visible: false,
            action: || Box::new(crate::actions::GoFocus),
            label: "Focus",
            icon: Lucide::Timer,
            route: Route::Focus,
            badge: Badge::None,
            children: &[],
            dynamic: None,
        },
        NavEntry {
            id: "developer",
            default_visible: false,
            action: || Box::new(crate::actions::GoDeveloper),
            label: "Developer",
            icon: Lucide::Code,
            route: Route::Developer,
            badge: Badge::None,
            children: &[],
            dynamic: None,
        },
        NavEntry {
            id: "passwords",
            default_visible: false,
            action: || Box::new(crate::actions::GoPasswords),
            label: "Passwords",
            icon: Lucide::KeyRound,
            route: Route::Passwords,
            badge: Badge::None,
            children: &[
                NavChild {
                    label: "All items",
                    target: NavTarget::Vault(VaultFilter::All),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Favorites",
                    target: NavTarget::Vault(VaultFilter::Favorites),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Logins",
                    target: NavTarget::Vault(VaultFilter::Kind(ItemKind::Login)),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Cards",
                    target: NavTarget::Vault(VaultFilter::Kind(ItemKind::Card)),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Secure notes",
                    target: NavTarget::Vault(VaultFilter::Kind(ItemKind::Note)),
                    badge: Badge::None,
                },
                NavChild {
                    label: "One-time codes",
                    target: NavTarget::Vault(VaultFilter::Codes),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Password health",
                    target: NavTarget::Vault(VaultFilter::Health),
                    badge: Badge::None,
                },
                NavChild {
                    label: "Trash",
                    target: NavTarget::Vault(VaultFilter::Trash),
                    badge: Badge::None,
                },
            ],
            dynamic: Some(DynamicChildren::VaultTags),
        },
    ],
}];

pub fn entries() -> impl Iterator<Item = &'static NavEntry> {
    SECTIONS.iter().flat_map(|section| section.entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_except_settings_is_in_the_sidebar_once() {
        for route in Route::ALL {
            let count = SECTIONS
                .iter()
                .flat_map(|s| s.entries)
                .filter(|e| e.route == route)
                .count();
            let expected = usize::from(route != Route::Settings);
            assert_eq!(count, expected, "{route:?}");
        }
    }

    #[test]
    fn child_labels_are_unique_within_an_entry() {
        for entry in SECTIONS.iter().flat_map(|s| s.entries) {
            let mut labels: Vec<_> = entry.children.iter().map(|c| c.label).collect();
            labels.sort_unstable();
            labels.dedup();
            assert_eq!(labels.len(), entry.children.len(), "{}", entry.label);
        }
    }
    #[test]
    fn feature_keys_are_unique_stable_and_match_routes() {
        let entries: Vec<_> = entries().collect();
        let mut ids: Vec<_> = entries.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), entries.len());
        for entry in entries {
            assert_eq!(Route::parse(entry.id), Some(entry.route));
        }
    }
}
