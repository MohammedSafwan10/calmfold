//! Repeat rules for tasks.
//!
//! Completing a repeating task creates the next one (see [`crate::Task::next_occurrence`]).
//! The next date is counted from the task's due date (or today if it had none)
//! and moved forward until it is today or later, so a long-overdue daily task
//! doesn't leave a pile of instances behind.

use std::fmt;

use jiff::Span;
use jiff::civil::{Date, Weekday};
use serde::{Deserialize, Serialize};

/// Days of the week as a bit set, Monday = bit 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Weekdays(u8);

impl Weekdays {
    pub const NONE: Self = Self(0);
    pub const WORKDAYS: Self = Self(0b001_1111);
    const ORDER: [Weekday; 7] = [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ];

    #[must_use]
    pub fn single(day: Weekday) -> Self {
        Self(1 << day.to_monday_zero_offset())
    }

    #[must_use]
    pub fn with(self, day: Weekday) -> Self {
        Self(self.0 | Self::single(day).0)
    }

    #[must_use]
    pub fn contains(self, day: Weekday) -> bool {
        self.0 & Self::single(day).0 != 0
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 & 0b111_1111 == 0
    }

    fn iter(self) -> impl Iterator<Item = Weekday> {
        Self::ORDER.into_iter().filter(move |d| self.contains(*d))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Recurrence {
    Days(u16),
    /// Every `every` weeks; on `days` (empty means "same weekday as the due date").
    Weeks {
        every: u16,
        days: Weekdays,
    },
    Months(u16),
    Years(u16),
}

impl Recurrence {
    pub const DAILY: Self = Self::Days(1);
    pub const WEEKLY: Self = Self::Weeks {
        every: 1,
        days: Weekdays::NONE,
    };
    pub const WORKDAYS: Self = Self::Weeks {
        every: 1,
        days: Weekdays::WORKDAYS,
    };
    pub const MONTHLY: Self = Self::Months(1);
    pub const YEARLY: Self = Self::Years(1);

    /// Presets offered in menus, in display order.
    pub const PRESETS: [Self; 5] = [
        Self::DAILY,
        Self::WORKDAYS,
        Self::WEEKLY,
        Self::MONTHLY,
        Self::YEARLY,
    ];

    fn every(self) -> i64 {
        let n = match self {
            Self::Days(n) | Self::Months(n) | Self::Years(n) | Self::Weeks { every: n, .. } => n,
        };
        i64::from(n.max(1))
    }

    /// The first date strictly after `date` that matches the rule.
    #[must_use]
    pub fn next_after(self, date: Date) -> Date {
        let add = |span: Span| date.checked_add(span).unwrap_or(date);
        match self {
            Self::Days(_) => add(Span::new().days(self.every())),
            Self::Months(_) => add(Span::new().months(self.every())),
            Self::Years(_) => add(Span::new().years(self.every())),
            Self::Weeks { days, .. } if days.is_empty() => add(Span::new().weeks(self.every())),
            Self::Weeks { days, .. } => {
                // Later this week (Mon..Sun)?
                let offset = i64::from(date.weekday().to_monday_zero_offset());
                for ahead in 1..(7 - offset) {
                    let candidate = add(Span::new().days(ahead));
                    if days.contains(candidate.weekday()) {
                        return candidate;
                    }
                }
                // Otherwise the first chosen day of the week `every` weeks on.
                let week_start = add(Span::new().days(-offset + 7 * self.every()));
                let first = days
                    .iter()
                    .next()
                    .map_or(0, |d| i64::from(d.to_monday_zero_offset()));
                week_start
                    .checked_add(Span::new().days(first))
                    .unwrap_or(week_start)
            }
        }
    }

    /// The first due date for a new repeating task created `today`.
    #[must_use]
    pub fn first_on_or_after(self, today: Date) -> Date {
        match self {
            Self::Weeks { days, .. } if !days.is_empty() && !days.contains(today.weekday()) => {
                Self::Weeks { every: 1, days }.next_after(today)
            }
            _ => today,
        }
    }

    /// Next due date after `due`, moved forward to today or later.
    #[must_use]
    pub fn next_due(self, due: Date, today: Date) -> Date {
        let mut next = self.next_after(due);
        for _ in 0..10_000 {
            if next >= today {
                break;
            }
            next = self.next_after(next);
        }
        next
    }

    /// Stable storage code: `d1`, `w1`, `w1:mon,wed`, `m1`, `y1`. Never change.
    #[must_use]
    pub fn to_code(self) -> String {
        match self {
            Self::Days(n) => format!("d{n}"),
            Self::Months(n) => format!("m{n}"),
            Self::Years(n) => format!("y{n}"),
            Self::Weeks { every, days } if days.is_empty() => format!("w{every}"),
            Self::Weeks { every, days } => {
                let names: Vec<_> = days.iter().map(short_name).collect();
                format!("w{every}:{}", names.join(","))
            }
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        let (head, days) = code
            .split_once(':')
            .map_or((code, None), |(h, d)| (h, Some(d)));
        let kind = head.chars().next()?;
        let every: u16 = head.get(1..)?.parse().ok().filter(|n| *n >= 1)?;
        Some(match (kind, days) {
            ('d', None) => Self::Days(every),
            ('m', None) => Self::Months(every),
            ('y', None) => Self::Years(every),
            ('w', None) => Self::Weeks {
                every,
                days: Weekdays::NONE,
            },
            ('w', Some(list)) => {
                let mut set = Weekdays::NONE;
                for name in list.split(',') {
                    set = set.with(parse_weekday(name)?);
                }
                Self::Weeks { every, days: set }
            }
            _ => return None,
        })
    }
}

impl fmt::Display for Recurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.every();
        let unit = |one: &str, many: &str| {
            if n == 1 {
                format!("Every {one}")
            } else {
                format!("Every {n} {many}")
            }
        };
        match *self {
            Self::Days(_) => f.write_str(&unit("day", "days")),
            Self::Months(_) => f.write_str(&unit("month", "months")),
            Self::Years(_) => f.write_str(&unit("year", "years")),
            Self::Weeks { days, .. } if days == Weekdays::WORKDAYS && n == 1 => {
                f.write_str("Every weekday")
            }
            Self::Weeks { days, .. } if days.is_empty() => f.write_str(&unit("week", "weeks")),
            Self::Weeks { days, .. } => {
                let names: Vec<_> = days.iter().map(|d| capitalize(short_name(d))).collect();
                write!(f, "{} on {}", unit("week", "weeks"), names.join(", "))
            }
        }
    }
}

fn short_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "mon",
        Weekday::Tuesday => "tue",
        Weekday::Wednesday => "wed",
        Weekday::Thursday => "thu",
        Weekday::Friday => "fri",
        Weekday::Saturday => "sat",
        Weekday::Sunday => "sun",
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Parses "mon", "monday", "tues", ... (case-insensitive).
#[must_use]
pub fn parse_weekday(word: &str) -> Option<Weekday> {
    let w = word.trim().to_lowercase();
    let w = w.trim_end_matches('s'); // "mondays"
    Weekdays::ORDER.into_iter().find(|day| {
        let full = match day {
            Weekday::Monday => "monday",
            Weekday::Tuesday => "tuesday",
            Weekday::Wednesday => "wednesday",
            Weekday::Thursday => "thursday",
            Weekday::Friday => "friday",
            Weekday::Saturday => "saturday",
            Weekday::Sunday => "sunday",
        };
        w.len() >= 3 && full.starts_with(w) || w == full.trim_end_matches('s')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }

    #[test]
    fn simple_intervals() {
        assert_eq!(
            Recurrence::DAILY.next_after(d("2026-11-08")),
            d("2026-11-09")
        );
        assert_eq!(
            Recurrence::Days(3).next_after(d("2026-11-08")),
            d("2026-11-11")
        );
        assert_eq!(
            Recurrence::WEEKLY.next_after(d("2026-11-08")),
            d("2026-11-15")
        );
        assert_eq!(
            Recurrence::MONTHLY.next_after(d("2026-01-31")),
            d("2026-02-28"),
            "clamps to month end"
        );
        assert_eq!(
            Recurrence::YEARLY.next_after(d("2024-02-29")),
            d("2025-02-28")
        );
    }

    #[test]
    fn weekday_sets() {
        // 2026-11-06 is a Friday.
        assert_eq!(
            Recurrence::WORKDAYS.next_after(d("2026-11-06")),
            d("2026-11-09"),
            "Fri -> Mon"
        );
        assert_eq!(
            Recurrence::WORKDAYS.next_after(d("2026-11-09")),
            d("2026-11-10")
        );
        let mon_wed = Recurrence::Weeks {
            every: 1,
            days: Weekdays::single(Weekday::Monday).with(Weekday::Wednesday),
        };
        assert_eq!(mon_wed.next_after(d("2026-11-09")), d("2026-11-11"));
        assert_eq!(mon_wed.next_after(d("2026-11-11")), d("2026-11-16"));
        let fortnightly_mon = Recurrence::Weeks {
            every: 2,
            days: Weekdays::single(Weekday::Monday),
        };
        assert_eq!(fortnightly_mon.next_after(d("2026-11-09")), d("2026-11-23"));
    }

    #[test]
    fn first_due_and_catch_up() {
        let sunday = d("2026-11-08");
        let mondays = Recurrence::Weeks {
            every: 1,
            days: Weekdays::single(Weekday::Monday),
        };
        assert_eq!(mondays.first_on_or_after(sunday), d("2026-11-09"));
        assert_eq!(Recurrence::DAILY.first_on_or_after(sunday), sunday);
        // Overdue by a week: the next one is today, not a week ago.
        assert_eq!(Recurrence::DAILY.next_due(d("2026-11-01"), sunday), sunday);
        assert_eq!(Recurrence::DAILY.next_due(sunday, sunday), d("2026-11-09"));
    }

    #[test]
    fn codes_round_trip_and_reject_junk() {
        let all = [
            Recurrence::DAILY,
            Recurrence::Days(3),
            Recurrence::WEEKLY,
            Recurrence::WORKDAYS,
            Recurrence::Weeks {
                every: 2,
                days: Weekdays::single(Weekday::Sunday),
            },
            Recurrence::MONTHLY,
            Recurrence::YEARLY,
        ];
        for r in all {
            assert_eq!(Recurrence::from_code(&r.to_code()), Some(r), "{r:?}");
        }
        for junk in ["", "x1", "d0", "d", "w1:xyz", "m1:mon", "d-1"] {
            assert_eq!(Recurrence::from_code(junk), None, "{junk}");
        }
    }

    #[test]
    fn labels() {
        assert_eq!(Recurrence::DAILY.to_string(), "Every day");
        assert_eq!(Recurrence::Days(2).to_string(), "Every 2 days");
        assert_eq!(Recurrence::WORKDAYS.to_string(), "Every weekday");
        let r = Recurrence::Weeks {
            every: 1,
            days: Weekdays::single(Weekday::Monday).with(Weekday::Friday),
        };
        assert_eq!(r.to_string(), "Every week on Mon, Fri");
    }

    #[test]
    fn weekday_names() {
        assert_eq!(parse_weekday("Mon"), Some(Weekday::Monday));
        assert_eq!(parse_weekday("tuesday"), Some(Weekday::Tuesday));
        assert_eq!(parse_weekday("thurs"), Some(Weekday::Thursday));
        assert_eq!(parse_weekday("fridays"), Some(Weekday::Friday));
        assert_eq!(parse_weekday("mo"), None);
        assert_eq!(parse_weekday("money"), None);
    }
}
