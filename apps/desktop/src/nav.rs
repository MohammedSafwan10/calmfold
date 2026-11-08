//! Sidebar navigation, as data.
//!
//! To add a feature to the sidebar, add a [`NavEntry`] to [`SECTIONS`] (and a
//! [`Route`] for it). Children are optional deep links into the screen. Only
//! list features that exist: no placeholders for unbuilt screens.

use gpui_kit::assets::IconName as Lucide;

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
    Project(keepstone_core::ProjectId),
}

/// Which count badge an entry shows, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    None,
    /// Open tasks due today or overdue.
    TasksDueNow,
}

pub struct NavChild {
    pub label: &'static str,
    pub target: NavTarget,
    pub badge: Badge,
}

pub struct NavEntry {
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
}

pub struct NavSection {
    pub label: &'static str,
    pub entries: &'static [NavEntry],
}

pub const SECTIONS: &[NavSection] = &[NavSection {
    label: "Workspace",
    entries: &[
        NavEntry {
            label: "Today",
            icon: Lucide::House,
            route: Route::Today,
            badge: Badge::TasksDueNow,
            children: &[],
            dynamic: None,
        },
        NavEntry {
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
    ],
}];

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
}
