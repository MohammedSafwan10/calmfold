//! Feature screens. Each screen is an `Entity` that owns its own state and
//! reads data through [`crate::library::Library`].

pub mod calendar;
pub mod calendar_grid;
pub mod calendar_layout;
pub mod code_highlight;
pub mod diagram;
pub mod event_detail;
pub mod habit_detail;
pub mod habit_insights_view;
pub mod habit_widgets;
pub mod habits;
pub mod note_editor;
pub mod notes;
pub mod settings;
pub mod task_detail;
pub mod task_row;
pub mod tasks;
pub mod today;

use keepstone_core::NoteId;

/// Top-level destinations shown in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Today,
    Notes,
    Tasks,
    Calendar,
    Habits,
    Settings,
}

impl Route {
    pub const ALL: [Self; 6] = [
        Self::Today,
        Self::Notes,
        Self::Tasks,
        Self::Calendar,
        Self::Habits,
        Self::Settings,
    ];

    /// Parses a route name ("today", "notes", "tasks", "calendar", "settings").
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|r| r.label().eq_ignore_ascii_case(name.trim()))
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Notes => "Notes",
            Self::Tasks => "Tasks",
            Self::Calendar => "Calendar",
            Self::Habits => "Habits",
            Self::Settings => "Settings",
        }
    }
}

/// Emitted by screens that want the shell to change what is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigate {
    To(Route),
    OpenNote(NoteId),
}

/// Formats a due date relative to `today`: "Today", "Tomorrow", "Yesterday",
/// a weekday within the next week, otherwise "Nov 20" (or "Nov 20, 2027").
#[must_use]
pub fn relative_day(day: keepstone_core::Date, today: keepstone_core::Date) -> String {
    let diff = (day - today).get_days();
    match diff {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        2..=6 => day.strftime("%A").to_string(),
        _ if day.year() == today.year() => day.strftime("%b %-d").to_string(),
        _ => day.strftime("%b %-d, %Y").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> keepstone_core::Date {
        s.parse().expect("date")
    }

    #[test]
    fn relative_day_labels() {
        let today = d("2026-11-08"); // a Sunday
        assert_eq!(relative_day(today, today), "Today");
        assert_eq!(relative_day(d("2026-11-09"), today), "Tomorrow");
        assert_eq!(relative_day(d("2026-11-07"), today), "Yesterday");
        assert_eq!(relative_day(d("2026-11-11"), today), "Wednesday");
        assert_eq!(relative_day(d("2026-11-20"), today), "Nov 20");
        assert_eq!(relative_day(d("2027-01-02"), today), "Jan 2, 2027");
        assert_eq!(relative_day(d("2026-10-01"), today), "Oct 1");
    }
}
