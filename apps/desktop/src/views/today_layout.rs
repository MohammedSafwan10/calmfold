//! The Today dashboard's sections, as data.
//!
//! To add a section: add a variant, a row in [`TodaySection::ALL`] (its
//! stable key, title and width), and render it in `today.rs`. The user's
//! order and visibility (Settings → Today) are stored by key, so sections can
//! be added or removed without breaking saved layouts.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TodaySection {
    /// Three numbers: next event, tasks, habits.
    Glance,
    /// Today's events as a timeline with a now marker.
    Agenda,
    /// Today's habits with one-tap checks.
    Habits,
    /// Tasks due today and overdue.
    Tasks,
    /// Tasks done and habit completion over the last 7 days.
    Week,
    /// Events and tasks in the next 7 days.
    Upcoming,
    /// Recently edited notes.
    Notes,
}

/// How much of the row a section takes on wide windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Full,
    /// Pairs with the next half-width section; stacks when narrow.
    Half,
}

impl TodaySection {
    /// Default order.
    pub const ALL: [Self; 7] = [
        Self::Glance,
        Self::Agenda,
        Self::Habits,
        Self::Tasks,
        Self::Week,
        Self::Upcoming,
        Self::Notes,
    ];

    /// Stable storage key. Never change one.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Glance => "glance",
            Self::Agenda => "agenda",
            Self::Habits => "habits",
            Self::Tasks => "tasks",
            Self::Week => "week",
            Self::Upcoming => "upcoming",
            Self::Notes => "notes",
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Glance => "At a glance",
            Self::Agenda => "Today's schedule",
            Self::Habits => "Habits",
            Self::Tasks => "Due today",
            Self::Week => "This week",
            Self::Upcoming => "Next 7 days",
            Self::Notes => "Recent notes",
        }
    }

    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Glance => "Next event, tasks left and habits left, side by side.",
            Self::Agenda => "Today's events in time order, with what's on now.",
            Self::Habits => "Check off today's habits without leaving Today.",
            Self::Tasks => "Tasks due today and anything overdue.",
            Self::Week => "Tasks finished and habits kept over the last 7 days.",
            Self::Upcoming => "Events and tasks coming up this week.",
            Self::Notes => "The notes you edited last.",
        }
    }

    #[must_use]
    pub fn width(self) -> Width {
        match self {
            Self::Glance | Self::Tasks => Width::Full,
            Self::Agenda | Self::Habits | Self::Week | Self::Upcoming | Self::Notes => Width::Half,
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// Every section in the saved order (unknown keys dropped, new sections
/// appended in default order), each with whether it is shown.
#[must_use]
pub fn resolve(order: &[String], hidden: &[String]) -> Vec<(TodaySection, bool)> {
    let mut out: Vec<TodaySection> = Vec::new();
    for key in order {
        if let Some(s) = TodaySection::from_key(key)
            && !out.contains(&s)
        {
            out.push(s);
        }
    }
    for s in TodaySection::ALL {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out.into_iter()
        .map(|s| (s, !hidden.iter().any(|h| h == s.key())))
        .collect()
}

/// The saved order after moving `section` one place up (`-1`) or down (`1`).
#[must_use]
pub fn moved(order: &[String], section: TodaySection, dir: i32) -> Vec<String> {
    let mut keys: Vec<String> = resolve(order, &[])
        .into_iter()
        .map(|(s, _)| s.key().to_owned())
        .collect();
    if let Some(i) = keys.iter().position(|k| k == section.key()) {
        let j = i as i64 + i64::from(dir);
        if (0..keys.len() as i64).contains(&j) {
            keys.swap(i, j as usize);
        }
    }
    keys
}

/// Visible sections grouped into rows: a full-width section alone, or up to
/// two consecutive half-width ones side by side.
#[must_use]
pub fn rows(sections: &[TodaySection]) -> Vec<Vec<TodaySection>> {
    let mut rows: Vec<Vec<TodaySection>> = Vec::new();
    for &s in sections {
        match (s.width(), rows.last_mut()) {
            (Width::Half, Some(last)) if last.len() == 1 && last[0].width() == Width::Half => {
                last.push(s);
            }
            _ => rows.push(vec![s]),
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn keys_are_unique_and_round_trip() {
        for s in TodaySection::ALL {
            assert_eq!(TodaySection::from_key(s.key()), Some(s));
        }
        let mut k: Vec<_> = TodaySection::ALL.iter().map(|s| s.key()).collect();
        k.sort_unstable();
        k.dedup();
        assert_eq!(k.len(), TodaySection::ALL.len());
    }

    #[test]
    fn saved_order_wins_unknowns_drop_new_ones_append() {
        let r = resolve(
            &keys(&["notes", "bogus", "glance", "notes"]),
            &keys(&["week"]),
        );
        let order: Vec<_> = r.iter().map(|(s, _)| *s).collect();
        assert_eq!(order[0], TodaySection::Notes);
        assert_eq!(order[1], TodaySection::Glance);
        assert_eq!(order.len(), TodaySection::ALL.len());
        assert!(
            r.iter()
                .any(|(s, shown)| *s == TodaySection::Week && !shown)
        );
    }

    #[test]
    fn moving_swaps_neighbours_and_stops_at_the_ends() {
        let m = moved(&[], TodaySection::Agenda, -1);
        assert_eq!(m[0], "agenda");
        let m = moved(&m, TodaySection::Agenda, -1);
        assert_eq!(m[0], "agenda", "already first");
        let m = moved(&[], TodaySection::Notes, 1);
        assert_eq!(m.last().map(String::as_str), Some("notes"));
    }

    #[test]
    fn halves_pair_up_fulls_stand_alone() {
        use TodaySection::*;
        let r = rows(&[Glance, Agenda, Habits, Tasks, Week, Upcoming, Notes]);
        assert_eq!(
            r,
            vec![
                vec![Glance],
                vec![Agenda, Habits],
                vec![Tasks],
                vec![Week, Upcoming],
                vec![Notes]
            ]
        );
    }
}
