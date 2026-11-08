//! Natural-language quick add: `Pay rent tomorrow !high #Home`.
//!
//! Rules, chosen to avoid surprises:
//! * `!high` / `!medium` / `!low` (or `!h` `!m` `!l`) set priority, anywhere.
//! * `#Name` picks a project, anywhere (the caller decides whether it exists).
//! * Dates and repeats are read **only from the end** of the text, so a title
//!   like "Plan Monday standup" keeps its words. Recognized endings:
//!   `today` `tomorrow` `tonight`, a weekday (`friday`, `fri`), `next week`,
//!   `next month`, `in 3 days`/`weeks`/`months`, `nov 20`, `20 nov`, `2026-11-20`,
//!   and repeats like `every day`, `daily`, `every weekday`, `every monday`,
//!   `every 2 weeks`, `weekly`, `monthly`, `yearly`. An optional `on`/`due`/`by`
//!   before a date is removed too.
//! * If nothing would be left for the title, the text is kept as typed.

use jiff::Span;
use jiff::civil::{Date, Weekday};

use crate::recurrence::{Recurrence, Weekdays, parse_weekday};
use crate::task::Priority;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickAdd {
    pub title: String,
    pub due: Option<Date>,
    pub priority: Option<Priority>,
    pub recurrence: Option<Recurrence>,
    pub project: Option<String>,
}

impl QuickAdd {
    /// True when anything beyond the title was recognized (for showing hints).
    #[must_use]
    pub fn has_extras(&self) -> bool {
        self.due.is_some()
            || self.priority.is_some()
            || self.recurrence.is_some()
            || self.project.is_some()
    }
}

#[must_use]
pub fn parse_quick_add(input: &str, today: Date) -> QuickAdd {
    let mut words: Vec<&str> = input.split_whitespace().collect();
    let mut priority = None;
    let mut project = None;

    words.retain(|w| {
        if let Some(p) = parse_priority(w) {
            priority = Some(p);
            return false;
        }
        if let Some(name) = w
            .strip_prefix('#')
            .filter(|n| !n.is_empty() && n.chars().next().is_some_and(char::is_alphabetic))
        {
            project = Some(name.replace('_', " "));
            return false;
        }
        true
    });

    let mut due = None;
    let mut recurrence = None;
    // Strip trailing date/repeat phrases (each at most once).
    loop {
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        if recurrence.is_none()
            && let Some((len, rule)) = trailing_recurrence(&lower)
        {
            recurrence = Some(rule);
            words.truncate(words.len() - len);
            continue;
        }
        if due.is_none()
            && let Some((len, date)) = trailing_date(&lower, today)
        {
            due = Some(date);
            words.truncate(words.len() - len);
            if words
                .last()
                .is_some_and(|w| matches!(w.to_lowercase().as_str(), "on" | "due" | "by"))
            {
                words.pop();
            }
            continue;
        }
        break;
    }
    if due.is_none() {
        due = recurrence.map(|r| r.first_on_or_after(today));
    }

    let title = words.join(" ");
    let title = if title.trim().is_empty() {
        input.trim().to_owned()
    } else {
        title
    };
    QuickAdd {
        title,
        due,
        priority,
        recurrence,
        project,
    }
}

fn parse_priority(word: &str) -> Option<Priority> {
    match word.to_lowercase().as_str() {
        "!high" | "!h" | "!!!" => Some(Priority::High),
        "!medium" | "!med" | "!m" | "!!" => Some(Priority::Medium),
        "!low" | "!l" => Some(Priority::Low),
        _ => None,
    }
}

fn add(today: Date, span: Span) -> Option<Date> {
    today.checked_add(span).ok()
}

/// (words consumed, date) for a date phrase at the end of `w`.
fn trailing_date(w: &[String], today: Date) -> Option<(usize, Date)> {
    let n = w.len();
    let last = w.last()?.as_str();
    let prev = n.checked_sub(2).map(|i| w[i].as_str());
    let prev2 = n.checked_sub(3).map(|i| w[i].as_str());

    // "in 3 days" / "in 2 weeks" / "in 1 month"
    if let (Some(count), Some("in")) = (prev, prev2)
        && let Ok(count) = count.parse::<i64>()
        && (1..=999).contains(&count)
    {
        let span = match last.trim_end_matches('s') {
            "day" => Some(Span::new().days(count)),
            "week" => Some(Span::new().weeks(count)),
            "month" => Some(Span::new().months(count)),
            _ => None,
        };
        if let Some(date) = span.and_then(|s| add(today, s)) {
            return Some((3, date));
        }
    }
    // "next week" / "next month" / "next friday"
    if prev == Some("next") {
        let date = match last {
            "week" => add(today, Span::new().weeks(1)),
            "month" => add(today, Span::new().months(1)),
            day => parse_weekday(day).map(|d| next_weekday(today, d)),
        };
        if let Some(date) = date {
            return Some((2, date));
        }
    }
    // "nov 20" / "20 nov"
    if let Some(prev) = prev
        && let Some(date) = month_day(prev, last, today).or_else(|| month_day(last, prev, today))
    {
        return Some((2, date));
    }
    let single = match last {
        "today" | "tonight" | "tod" => Some(today),
        "tomorrow" | "tmr" | "tmrw" => add(today, Span::new().days(1)),
        word => word
            .parse::<Date>()
            .ok()
            .or_else(|| parse_weekday(word).map(|d| next_weekday(today, d))),
    };
    single.map(|date| (1, date))
}

/// (words consumed, rule) for a repeat phrase at the end of `w`.
fn trailing_recurrence(w: &[String]) -> Option<(usize, Recurrence)> {
    let n = w.len();
    let last = w.last()?.as_str();
    let prev = n.checked_sub(2).map(|i| w[i].as_str());
    let prev2 = n.checked_sub(3).map(|i| w[i].as_str());

    match last {
        "daily" => return Some((1, Recurrence::DAILY)),
        "weekly" => return Some((1, Recurrence::WEEKLY)),
        "monthly" => return Some((1, Recurrence::MONTHLY)),
        "yearly" | "annually" => return Some((1, Recurrence::YEARLY)),
        _ => {}
    }
    // "every 2 weeks"
    if prev2 == Some("every")
        && let Some(count) = prev
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|c| (1..=365).contains(c))
    {
        let rule = match last.trim_end_matches('s') {
            "day" => Some(Recurrence::Days(count)),
            "week" => Some(Recurrence::Weeks {
                every: count,
                days: Weekdays::NONE,
            }),
            "month" => Some(Recurrence::Months(count)),
            "year" => Some(Recurrence::Years(count)),
            _ => None,
        };
        if let Some(rule) = rule {
            return Some((3, rule));
        }
    }
    if prev == Some("every") {
        let rule = match last {
            "day" => Some(Recurrence::DAILY),
            "weekday" | "workday" => Some(Recurrence::WORKDAYS),
            "week" => Some(Recurrence::WEEKLY),
            "month" => Some(Recurrence::MONTHLY),
            "year" => Some(Recurrence::YEARLY),
            day => parse_weekday(day).map(|d| Recurrence::Weeks {
                every: 1,
                days: Weekdays::single(d),
            }),
        };
        if let Some(rule) = rule {
            return Some((2, rule));
        }
    }
    None
}

/// The next `day` strictly after `today`.
fn next_weekday(today: Date, day: Weekday) -> Date {
    let ahead = (i64::from(day.to_monday_zero_offset())
        - i64::from(today.weekday().to_monday_zero_offset())
        + 7)
        % 7;
    let ahead = if ahead == 0 { 7 } else { ahead };
    today.checked_add(Span::new().days(ahead)).unwrap_or(today)
}

/// "nov" + "20" → the next Nov 20 on or after today.
fn month_day(month: &str, day: &str, today: Date) -> Option<Date> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let m = MONTHS
        .iter()
        .position(|m| month.len() >= 3 && month.starts_with(m))?;
    let day: i8 = day
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .ok()?; // "20th"
    let month = i8::try_from(m + 1).ok()?;
    let this_year = Date::new(today.year(), month, day).ok()?;
    if this_year >= today {
        Some(this_year)
    } else {
        Date::new(today.year() + 1, month, day).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-08"; // Sunday

    fn parse(s: &str) -> QuickAdd {
        parse_quick_add(s, TODAY.parse().expect("date"))
    }

    fn d(s: &str) -> Option<Date> {
        Some(s.parse().expect("date"))
    }

    #[test]
    fn plain_titles_are_untouched() {
        let q = parse("Plan Monday standup with team");
        assert_eq!(q.title, "Plan Monday standup with team");
        assert!(!q.has_extras());
    }

    #[test]
    fn trailing_dates() {
        assert_eq!(parse("Pay rent tomorrow").due, d("2026-11-09"));
        assert_eq!(parse("Pay rent tomorrow").title, "Pay rent");
        assert_eq!(parse("Call mom today").due, d("2026-11-08"));
        assert_eq!(parse("Gym friday").due, d("2026-11-13"));
        assert_eq!(
            parse("Gym on sun").due,
            d("2026-11-15"),
            "same weekday means next week"
        );
        assert_eq!(parse("Review next week").due, d("2026-11-15"));
        assert_eq!(parse("Taxes next month").due, d("2026-12-08"));
        assert_eq!(parse("Renew in 3 days").due, d("2026-11-11"));
        assert_eq!(parse("Renew in 2 weeks").due, d("2026-11-22"));
        assert_eq!(parse("Party nov 20").due, d("2026-11-20"));
        assert_eq!(parse("Party 20th Nov").due, d("2026-11-20"));
        assert_eq!(
            parse("Party jan 2").due,
            d("2027-01-02"),
            "past dates roll to next year"
        );
        assert_eq!(parse("Dentist due 2026-12-01").due, d("2026-12-01"));
        assert_eq!(parse("Dentist due 2026-12-01").title, "Dentist");
    }

    #[test]
    fn priority_and_project_anywhere() {
        let q = parse("!high Pay rent #Home tomorrow");
        assert_eq!(q.title, "Pay rent");
        assert_eq!(q.priority, Some(Priority::High));
        assert_eq!(q.project.as_deref(), Some("Home"));
        assert_eq!(q.due, d("2026-11-09"));
        assert_eq!(parse("x !m").priority, Some(Priority::Medium));
        assert_eq!(
            parse("Fix #42 bug").project,
            None,
            "issue numbers are not projects"
        );
        assert_eq!(parse("Fix #42 bug").title, "Fix #42 bug");
        assert_eq!(
            parse("Work on #Side_Project").project.as_deref(),
            Some("Side Project")
        );
    }

    #[test]
    fn repeats() {
        let q = parse("Water plants every day");
        assert_eq!(q.title, "Water plants");
        assert_eq!(q.recurrence, Some(Recurrence::DAILY));
        assert_eq!(q.due, d("2026-11-08"), "repeating tasks start today");

        let q = parse("Standup every weekday");
        assert_eq!(q.recurrence, Some(Recurrence::WORKDAYS));
        assert_eq!(q.due, d("2026-11-09"), "first workday after Sunday");

        let q = parse("Trash out every monday");
        assert_eq!(q.due, d("2026-11-09"));
        assert_eq!(parse("Rent monthly").recurrence, Some(Recurrence::MONTHLY));
        assert_eq!(
            parse("Backup every 2 weeks").recurrence,
            Some(Recurrence::Weeks {
                every: 2,
                days: Weekdays::NONE
            })
        );
        let q = parse("Report every month next friday");
        assert_eq!(
            (q.recurrence, q.due),
            (Some(Recurrence::MONTHLY), d("2026-11-13"))
        );
    }

    #[test]
    fn never_empties_the_title() {
        assert_eq!(parse("tomorrow").title, "tomorrow");
        assert_eq!(parse("  !high  ").title, "!high");
    }
}
