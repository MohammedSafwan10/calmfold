//! Natural-language event entry: `Lunch with Sam tomorrow 1pm`.
//!
//! Same spirit as task quick add: only the **end** of the text is read, so a
//! title like "Plan 2pm slot review" keeps its words. Recognized endings, in
//! any order, each at most once:
//!
//! * a time `3pm`, `3:30pm`, `15:00`, or a range `9-10am`, `9:30-9:45`,
//!   `2pm-3:30pm` (an optional `at` before it is removed);
//! * a length `for 30m`, `for 1h`, `for 90 min`, `for 2 hours`;
//! * a date (everything task quick add knows: `tomorrow`, `fri`, `nov 20`, …)
//!   or a range of days `nov 20-22`, `20-22 nov`;
//! * a repeat `every weekday`, `weekly`, … .
//!
//! No time means an all-day event. A start without an end lasts one hour
//! (or the given length). No date means today.

use jiff::Span;
use jiff::civil::{Date, DateTime, Time};

use crate::quick_add::{month_day, trailing_date, trailing_recurrence};
use crate::recurrence::Recurrence;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub title: String,
    pub all_day: bool,
    pub start: DateTime,
    pub end: DateTime,
    pub recurrence: Option<Recurrence>,
}

impl EventDraft {
    /// Inclusive last day of an all-day draft.
    #[must_use]
    pub fn last_day(&self) -> Date {
        self.end.date().yesterday().unwrap_or(self.end.date())
    }
}

#[must_use]
pub fn parse_event(input: &str, today: Date) -> EventDraft {
    let mut words: Vec<&str> = input.split_whitespace().collect();
    let mut times: Option<(Time, Option<Time>)> = None;
    let mut length: Option<Span> = None;
    let mut day_range: Option<(Date, Date)> = None;
    let mut recurrence = None;

    loop {
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let n = lower.len();
        if n == 0 {
            break;
        }
        if length.is_none()
            && let Some((used, span)) = trailing_length(&lower)
        {
            length = Some(span);
            words.truncate(n - used);
            continue;
        }
        // A time needs a title or date before it ("2pm" alone is a title).
        if times.is_none()
            && n > 1
            && let Some(t) = parse_time_range(&lower[n - 1])
        {
            times = Some(t);
            words.truncate(n - 1);
            if words.last().is_some_and(|w| w.eq_ignore_ascii_case("at")) {
                words.pop();
            }
            continue;
        }
        if recurrence.is_none()
            && let Some((used, rule)) = trailing_recurrence(&lower)
        {
            recurrence = Some(rule);
            words.truncate(n - used);
            continue;
        }
        if day_range.is_none() {
            if let Some((used, range)) = trailing_day_range(&lower, today) {
                day_range = Some(range);
                words.truncate(n - used);
                strip_connector(&mut words);
                continue;
            }
            if let Some((used, date)) = trailing_date(&lower, today) {
                day_range = Some((date, date));
                words.truncate(n - used);
                strip_connector(&mut words);
                continue;
            }
        }
        break;
    }

    let (first, last) = day_range.unwrap_or_else(|| {
        let d = recurrence.map_or(today, |r| r.first_on_or_after(today));
        (d, d)
    });
    let title = words.join(" ");
    let title = if title.trim().is_empty() {
        input.trim().to_owned()
    } else {
        title
    };

    match times {
        Some((start_t, end_t)) => {
            let start = first.to_datetime(start_t);
            let end = match (end_t, length) {
                (Some(e), _) if e > start_t => first.to_datetime(e),
                // "11pm-1am" ends the next day.
                (Some(e), _) => first.tomorrow().map_or(start, |d| d.to_datetime(e)),
                (None, Some(span)) => start.checked_add(span).unwrap_or(start),
                (None, None) => start.checked_add(Span::new().hours(1)).unwrap_or(start),
            };
            EventDraft {
                title,
                all_day: false,
                start,
                end,
                recurrence,
            }
        }
        None => EventDraft {
            title,
            all_day: true,
            start: first.to_datetime(Time::midnight()),
            end: last
                .tomorrow()
                .unwrap_or(last)
                .to_datetime(Time::midnight()),
            recurrence,
        },
    }
}

fn strip_connector(words: &mut Vec<&str>) {
    if words
        .last()
        .is_some_and(|w| matches!(w.to_lowercase().as_str(), "on" | "from"))
    {
        words.pop();
    }
}

/// `for 30m`, `for 1h`, `for 1.5h`, `for 90 min`, `for 2 hours`.
fn trailing_length(w: &[String]) -> Option<(usize, Span)> {
    let n = w.len();
    let minutes = |num: &str, unit: &str| -> Option<i64> {
        let value: f64 = num.parse().ok()?;
        let per = match unit.trim_end_matches('s') {
            "m" | "min" | "minute" | "mins" => 1.,
            "h" | "hr" | "hour" => 60.,
            _ => return None,
        };
        let m = (value * per).round();
        (1. ..=60. * 24. * 7.).contains(&m).then_some(m as i64)
    };
    // "for 2 hours"
    if n >= 3
        && w[n - 3] == "for"
        && let Some(m) = minutes(&w[n - 2], &w[n - 1])
    {
        return Some((3, Span::new().minutes(m)));
    }
    // "for 30m"
    if n >= 2 && w[n - 2] == "for" {
        let last = &w[n - 1];
        let split = last.find(|c: char| c.is_ascii_alphabetic())?;
        let (num, unit) = last.split_at(split);
        if let Some(m) = minutes(num, unit) {
            return Some((2, Span::new().minutes(m)));
        }
    }
    None
}

/// `3pm`, `3:30pm`, `15:00`, `9-10am`, `9:30-9:45`, `2pm-3:30pm`.
fn parse_time_range(word: &str) -> Option<(Time, Option<Time>)> {
    if let Some((a, b)) = word.split_once(['-', '–']) {
        let end_meridiem = meridiem(b);
        let end = parse_time(b, None)?;
        // "9-10am": the start borrows the end's am/pm when it has none.
        let start = parse_time(a, end_meridiem)?;
        return Some((start, Some(end)));
    }
    Some((parse_time(word, None)?, None))
}

fn meridiem(s: &str) -> Option<bool> {
    if s.ends_with("pm") {
        Some(true)
    } else if s.ends_with("am") {
        Some(false)
    } else {
        None
    }
}

fn parse_time(s: &str, default_pm: Option<bool>) -> Option<Time> {
    let pm = meridiem(s).or(default_pm);
    let digits = s.trim_end_matches("am").trim_end_matches("pm");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return None;
    }
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h.parse::<i8>().ok()?, m.parse::<i8>().ok()?),
        Some(_) => return None,
        // A bare number is only a time with am/pm ("3pm"), never "3" or "2026".
        None if meridiem(s).is_some() || default_pm.is_some() => (digits.parse::<i8>().ok()?, 0),
        None => return None,
    };
    let h = match pm {
        Some(_) if !(1..=12).contains(&h) => return None,
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        _ => h,
    };
    Time::new(h, m, 0, 0).ok()
}

/// `nov 20-22` / `20-22 nov` → inclusive day range.
fn trailing_day_range(w: &[String], today: Date) -> Option<(usize, (Date, Date))> {
    let n = w.len();
    if n < 2 {
        return None;
    }
    let (a, b) = (&w[n - 2], &w[n - 1]);
    let (month, days) = if b.contains('-') { (a, b) } else { (b, a) };
    let (d1, d2) = days.split_once('-')?;
    let first = month_day(month, d1, today)?;
    let last = Date::new(first.year(), first.month(), d2.parse().ok()?).ok()?;
    (last >= first).then_some((2, (first, last)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-08"; // Sunday

    fn parse(s: &str) -> EventDraft {
        parse_event(s, TODAY.parse().expect("date"))
    }
    fn dt(s: &str) -> DateTime {
        s.parse().expect("datetime")
    }

    #[test]
    fn times_dates_and_lengths() {
        let e = parse("Lunch with Sam tomorrow 1pm");
        assert_eq!(e.title, "Lunch with Sam");
        assert!(!e.all_day);
        assert_eq!(
            (e.start, e.end),
            (dt("2026-11-09T13:00"), dt("2026-11-09T14:00"))
        );

        let e = parse("Dentist fri at 3:30pm for 45m");
        assert_eq!(e.title, "Dentist");
        assert_eq!(
            (e.start, e.end),
            (dt("2026-11-13T15:30"), dt("2026-11-13T16:15"))
        );

        let e = parse("Call 9-10am");
        assert_eq!(
            (e.start, e.end),
            (dt("2026-11-08T09:00"), dt("2026-11-08T10:00"))
        );

        let e = parse("Review 2pm-3:30pm nov 20");
        assert_eq!(
            (e.start, e.end),
            (dt("2026-11-20T14:00"), dt("2026-11-20T15:30"))
        );

        let e = parse("Gig 11pm-1am sat");
        assert_eq!(
            (e.start, e.end),
            (dt("2026-11-14T23:00"), dt("2026-11-15T01:00"))
        );

        let e = parse("Workshop 15:00 for 2 hours");
        assert_eq!(
            (e.start, e.end),
            (dt("2026-11-08T15:00"), dt("2026-11-08T17:00"))
        );
    }

    #[test]
    fn no_time_means_all_day() {
        let e = parse("Trip to Coorg nov 20-22");
        assert_eq!(e.title, "Trip to Coorg");
        assert!(e.all_day);
        assert_eq!(e.start, dt("2026-11-20T00:00"));
        assert_eq!(e.end, dt("2026-11-23T00:00"));
        assert_eq!(e.last_day(), "2026-11-22".parse().expect("date"));

        let e = parse("Mom's birthday dec 3 yearly");
        assert!(e.all_day);
        assert_eq!(e.recurrence, Some(Recurrence::YEARLY));
        assert_eq!(e.start, dt("2026-12-03T00:00"));
    }

    #[test]
    fn repeats_and_plain_titles() {
        let e = parse("Standup 9:30-9:45 every weekday");
        assert_eq!(e.title, "Standup");
        assert_eq!(e.recurrence, Some(Recurrence::WORKDAYS));
        assert_eq!(e.start, dt("2026-11-09T09:30"), "first weekday");

        let e = parse("Plan 2pm slot review");
        assert_eq!(e.title, "Plan 2pm slot review", "only the end is read");
        assert!(e.all_day);

        let e = parse("Budget 2026");
        assert_eq!(e.title, "Budget 2026", "a year is not a time");
        assert_eq!(parse("tomorrow").title, "tomorrow");
        assert!(
            parse("Meet 13pm").title.contains("13pm"),
            "invalid times stay text"
        );
    }
}
