//! iCalendar (`.ics`, RFC 5545) import: Google Calendar, Apple Calendar and
//! Outlook exports.
//!
//! Pure: the caller passes the local time zone and a lookup for `TZID`
//! names, so this module does no IO. Times become local wall-clock times,
//! the calendar's model (see [`crate::event`]).
//!
//! Kept: title, notes, place, all-day, times, repeat rules Keepstone can
//! express (daily/weekly on days/monthly/yearly with an interval, ending on
//! a date or after a count), `EXDATE` (skipped repeats) and changed single
//! repeats (`RECURRENCE-ID`). A rule Keepstone can't express (for example
//! "second Tuesday of the month") imports as its first date only, and is
//! counted so the user is told.

use std::collections::HashMap;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};

use crate::event::{Event, MAX_EVENT_NOTES_BYTES, MAX_LOCATION_CHARS};
use crate::recurrence::{Recurrence, Weekdays};
use crate::text::MAX_TITLE_CHARS;

/// Most events one file may hold (keeps a hostile file bounded).
pub const MAX_IMPORT_EVENTS: usize = 20_000;
/// Largest file accepted, in bytes.
pub const MAX_IMPORT_BYTES: usize = 20 * 1024 * 1024;

/// What a file turned into.
#[derive(Debug, Default)]
pub struct IcsImport {
    pub events: Vec<Event>,
    /// Events that couldn't be read (no start, bad dates, too long).
    pub skipped: usize,
    /// Repeating events whose rule Keepstone can't express yet; only their
    /// first date was imported.
    pub simplified: usize,
}

/// A time as written in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stamp {
    Date(Date),
    Time(DateTime),
}

/// One `VEVENT`'s properties, unescaped.
#[derive(Debug, Default)]
struct Raw {
    uid: Option<String>,
    summary: String,
    description: String,
    location: String,
    start: Option<Stamp>,
    end: Option<Stamp>,
    duration: Option<Span>,
    rrule: Option<String>,
    /// The rule's `UNTIL`, as a local date.
    until: Option<Date>,
    exdates: Vec<Stamp>,
    recurrence_id: Option<Stamp>,
    cancelled: bool,
}

/// Parses `text` into events. `local` is the zone times are shown in;
/// `zone` resolves `TZID` names (`None` falls back to `local`).
pub fn parse_ics(
    text: &str,
    local: &TimeZone,
    zone: &dyn Fn(&str) -> Option<TimeZone>,
    now: Timestamp,
) -> Result<IcsImport, &'static str> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err("That calendar file is too large (over 20 MB).");
    }
    if !text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("BEGIN:VCALENDAR")
    {
        return Err("That isn't an iCalendar (.ics) file.");
    }
    let raws = read_events(text, local, zone)?;
    let mut out = IcsImport::default();
    let mut by_uid: HashMap<String, usize> = HashMap::new();
    let mut overrides = Vec::new();
    for raw in raws {
        if raw.recurrence_id.is_some() {
            overrides.push(raw);
            continue;
        }
        if raw.cancelled {
            continue;
        }
        match build(&raw, now) {
            Some((event, simplified)) => {
                out.simplified += usize::from(simplified);
                if let Some(uid) = raw.uid.clone() {
                    by_uid.insert(uid, out.events.len());
                }
                out.events.push(event);
            }
            None => out.skipped += 1,
        }
        if out.events.len() >= MAX_IMPORT_EVENTS {
            break;
        }
    }
    // Changed or cancelled single repeats of a series in the same file.
    for raw in overrides {
        let Some(&i) = raw.uid.as_ref().and_then(|u| by_uid.get(u)) else {
            continue;
        };
        let Some(series_date) = raw.recurrence_id.map(stamp_date) else {
            continue;
        };
        let event = &mut out.events[i];
        if raw.cancelled {
            _ = event.skip_occurrence(series_date, now);
            continue;
        }
        let Some((start, end, _)) = span_of(&raw) else {
            continue;
        };
        _ = event.move_occurrence(series_date, start, end, now);
    }
    Ok(out)
}

/// Builds the event; `true` when its repeat rule had to be dropped.
fn build(raw: &Raw, now: Timestamp) -> Option<(Event, bool)> {
    let (start, end, all_day) = span_of(raw)?;
    let title = clip_chars(
        raw.summary.split_whitespace().collect::<Vec<_>>().join(" "),
        MAX_TITLE_CHARS,
    );
    let title = if title.is_empty() {
        "(No title)".to_owned()
    } else {
        title
    };
    let mut event = if all_day {
        let last = end.date().yesterday().ok()?.max(start.date());
        Event::new_all_day(&title, start.date(), last, now).ok()?
    } else {
        Event::new_timed(&title, start, end, now).ok()?
    };
    let notes = clip_bytes(raw.description.trim(), MAX_EVENT_NOTES_BYTES);
    event.set_notes(notes, now).ok()?;
    let location = clip_chars(raw.location.trim().to_owned(), MAX_LOCATION_CHARS);
    event.set_location(&location, now).ok()?;
    let mut simplified = false;
    if let Some(rule) = raw.rrule.as_deref() {
        match parse_rrule(rule, start.date()) {
            Some(r) => {
                event.set_recurrence(Some(r.rule), now).ok()?;
                let until = match (raw.until.filter(|_| r.until.is_some()), r.count) {
                    (Some(u), _) => Some(u),
                    (None, Some(n)) => nth_start(&event, n),
                    (None, None) => None,
                };
                if let Some(u) = until {
                    // An end before the start would hide everything: keep one.
                    _ = event.set_until(Some(u.max(start.date())), now);
                }
                for ex in &raw.exdates {
                    _ = event.skip_occurrence(stamp_date(*ex), now);
                }
            }
            None => simplified = true,
        }
    }
    Some((event, simplified))
}

/// Start, end and all-day for an event, applying `DURATION` or the RFC's
/// defaults (one day for a date, one hour for a time with no end).
fn span_of(raw: &Raw) -> Option<(DateTime, DateTime, bool)> {
    match raw.start? {
        Stamp::Date(d) => {
            let start = d.to_datetime(Time::midnight());
            let end = match (raw.end, raw.duration) {
                (Some(Stamp::Date(e)), _) => e.to_datetime(Time::midnight()),
                (Some(Stamp::Time(e)), _) => e.date().to_datetime(Time::midnight()),
                (None, Some(span)) => start.checked_add(span).ok()?,
                (None, None) => d.tomorrow().ok()?.to_datetime(Time::midnight()),
            };
            let end = if end <= start {
                d.tomorrow().ok()?.to_datetime(Time::midnight())
            } else {
                end
            };
            Some((start, end, true))
        }
        Stamp::Time(start) => {
            let end = match (raw.end, raw.duration) {
                (Some(Stamp::Time(e)), _) => e,
                (Some(Stamp::Date(e)), _) => e.to_datetime(Time::midnight()),
                (None, Some(span)) => start.checked_add(span).ok()?,
                (None, None) => start.checked_add(Span::new().hours(1)).ok()?,
            };
            let end = if end <= start {
                start.checked_add(Span::new().hours(1)).ok()?
            } else {
                end
            };
            Some((start, end, false))
        }
    }
}

/// The date of the `n`-th occurrence (for `COUNT`).
fn nth_start(event: &Event, n: u32) -> Option<Date> {
    let n = usize::try_from(n).ok()?.clamp(1, 5000);
    let from = event.start().date();
    let to = from.checked_add(Span::new().years(50)).ok()?;
    event
        .occurrences(from..to, n)
        .last()
        .map(|o| o.start.date())
}

fn stamp_date(s: Stamp) -> Date {
    match s {
        Stamp::Date(d) => d,
        Stamp::Time(t) => t.date(),
    }
}

struct Rule {
    rule: Recurrence,
    until: Option<Date>,
    count: Option<u32>,
}

/// `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;UNTIL=20261231T235959Z`, or `None`
/// for rules Keepstone can't express.
fn parse_rrule(text: &str, first: Date) -> Option<Rule> {
    let mut freq = None;
    let mut interval = 1_u16;
    let mut days = Weekdays::NONE;
    let mut until = None;
    let mut count = None;
    for part in text.split(';') {
        let (key, value) = part.split_once('=')?;
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => freq = Some(value.to_ascii_uppercase()),
            "INTERVAL" => interval = value.parse().ok().filter(|n| *n >= 1)?,
            "UNTIL" => until = Some(stamp_date(parse_stamp(value, None)?.0)),
            "COUNT" => count = Some(value.parse().ok().filter(|n| *n >= 1)?),
            "WKST" => {}
            "BYDAY" => {
                for d in value.split(',') {
                    // "2TU" (second Tuesday) can't be expressed.
                    days = days.with(weekday(d)?);
                }
            }
            // Same as the start (Google writes these): harmless.
            "BYMONTHDAY" if value.parse::<i8>().ok() == Some(first.day()) => {}
            "BYMONTH" if value.parse::<i8>().ok() == Some(first.month()) => {}
            _ => return None,
        }
    }
    let rule = match freq?.as_str() {
        "DAILY" if days.is_empty() => Recurrence::Days(interval),
        "WEEKLY" => Recurrence::Weeks {
            every: interval,
            days: if days == Weekdays::single(first.weekday()) {
                Weekdays::NONE
            } else {
                days
            },
        },
        "MONTHLY" if days.is_empty() => Recurrence::Months(interval),
        "YEARLY" if days.is_empty() => Recurrence::Years(interval),
        _ => return None,
    };
    Some(Rule { rule, until, count })
}

fn weekday(code: &str) -> Option<jiff::civil::Weekday> {
    use jiff::civil::Weekday::{Friday, Monday, Saturday, Sunday, Thursday, Tuesday, Wednesday};
    Some(match code.trim().to_ascii_uppercase().as_str() {
        "MO" => Monday,
        "TU" => Tuesday,
        "WE" => Wednesday,
        "TH" => Thursday,
        "FR" => Friday,
        "SA" => Saturday,
        "SU" => Sunday,
        _ => return None,
    })
}

/// Reads every `VEVENT`, converting times to `local`.
fn read_events(
    text: &str,
    local: &TimeZone,
    zone: &dyn Fn(&str) -> Option<TimeZone>,
) -> Result<Vec<Raw>, &'static str> {
    let mut out = Vec::new();
    let mut current: Option<Raw> = None;
    // Components nested inside an event (VALARM) are skipped.
    let mut nested = 0_usize;
    for line in unfold(text) {
        let Some((name, params, value)) = split_line(&line) else {
            continue;
        };
        match (name.as_str(), current.is_some()) {
            ("BEGIN", false) if value.eq_ignore_ascii_case("VEVENT") => {
                current = Some(Raw::default());
                nested = 0;
            }
            ("BEGIN", true) => nested += 1,
            ("END", true) if nested > 0 => nested -= 1,
            ("END", true) if value.eq_ignore_ascii_case("VEVENT") => {
                if let Some(raw) = current.take() {
                    out.push(raw);
                }
                if out.len() > MAX_IMPORT_EVENTS * 2 {
                    return Err("That calendar file has too many events.");
                }
            }
            (_, true) if nested == 0 => {
                if let Some(raw) = current.as_mut() {
                    apply(raw, &name, &params, &value, local, zone);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn apply(
    raw: &mut Raw,
    name: &str,
    params: &[(String, String)],
    value: &str,
    local: &TimeZone,
    zone: &dyn Fn(&str) -> Option<TimeZone>,
) {
    let tzid = params
        .iter()
        .find(|(k, _)| k == "TZID")
        .map(|(_, v)| v.as_str());
    let time = |v: &str| -> Option<Stamp> {
        let (stamp, utc) = parse_stamp(v, params_value(params, "VALUE"))?;
        Some(match stamp {
            Stamp::Date(d) => Stamp::Date(d),
            Stamp::Time(t) => Stamp::Time(to_local(t, utc, tzid, local, zone)?),
        })
    };
    match name {
        "UID" => raw.uid = Some(value.to_owned()),
        "SUMMARY" => raw.summary = unescape(value),
        "DESCRIPTION" => raw.description = unescape(value),
        "LOCATION" => raw.location = unescape(value),
        "DTSTART" => raw.start = time(value),
        "DTEND" => raw.end = time(value),
        "DURATION" => raw.duration = parse_duration(value),
        "RRULE" => {
            raw.rrule = Some(value.to_owned());
            raw.until = value
                .split(';')
                .find_map(|p| p.strip_prefix("UNTIL="))
                .and_then(|u| {
                    let (stamp, utc) = parse_stamp(u, None)?;
                    Some(match stamp {
                        Stamp::Date(d) => d,
                        Stamp::Time(t) => to_local(t, utc, tzid, local, zone)?.date(),
                    })
                });
        }
        "EXDATE" => raw.exdates.extend(value.split(',').filter_map(time)),
        "RECURRENCE-ID" => raw.recurrence_id = time(value),
        "STATUS" => raw.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
        _ => {}
    }
}

fn params_value<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// `20261110`, `20261110T090000` or `20261110T090000Z`; `true` = UTC.
fn parse_stamp(v: &str, value_type: Option<&str>) -> Option<(Stamp, bool)> {
    let v = v.trim();
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    let date = |s: &str| -> Option<Date> {
        if s.len() != 8 || !digits(s) {
            return None;
        }
        Date::new(
            s[0..4].parse().ok()?,
            s[4..6].parse().ok()?,
            s[6..8].parse().ok()?,
        )
        .ok()
    };
    if value_type.is_some_and(|t| t.eq_ignore_ascii_case("DATE")) || v.len() == 8 {
        return Some((Stamp::Date(date(v)?), false));
    }
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    let (d, t) = body.split_once('T')?;
    if t.len() != 6 || !digits(t) {
        return None;
    }
    let time = Time::new(
        t[0..2].parse().ok()?,
        t[2..4].parse().ok()?,
        t[4..6].parse::<i8>().ok()?.min(59),
        0,
    )
    .ok()?;
    Some((Stamp::Time(date(d)?.to_datetime(time)), utc))
}

/// A file time as local wall-clock time: UTC and `TZID` times are converted;
/// floating times are already local.
fn to_local(
    t: DateTime,
    utc: bool,
    tzid: Option<&str>,
    local: &TimeZone,
    zone: &dyn Fn(&str) -> Option<TimeZone>,
) -> Option<DateTime> {
    let source = if utc {
        TimeZone::UTC
    } else if let Some(name) = tzid {
        // Outlook writes Windows names ("Pacific Standard Time"); unknown
        // zones are taken as local rather than dropping the event.
        zone(name.trim_matches('"')).unwrap_or_else(|| local.clone())
    } else {
        return Some(t);
    };
    let zoned = t.to_zoned(source).ok()?;
    Some(zoned.with_time_zone(local.clone()).datetime())
}

/// `P1D`, `PT1H30M`, `P2W`.
fn parse_duration(v: &str) -> Option<Span> {
    let v = v.trim().trim_start_matches('+');
    let rest = v.strip_prefix('P')?;
    let mut span = Span::new();
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                span = match (unit, in_time) {
                    ('W', false) => span.try_weeks(n).ok()?,
                    ('D', false) => span.try_days(n).ok()?,
                    ('H', true) => span.try_hours(n).ok()?,
                    ('M', true) => span.try_minutes(n).ok()?,
                    ('S', true) => span.try_seconds(n).ok()?,
                    _ => return None,
                };
            }
        }
    }
    (number.is_empty() && !span.is_zero()).then_some(span)
}

/// Joins folded lines (a line starting with a space or tab continues the
/// previous one).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        lines.push(line.to_owned());
    }
    lines
}

/// Property parameters (`TZID=…`, `VALUE=DATE`), names upper-cased.
type Params = Vec<(String, String)>;

/// `NAME;PARAM=a;P2="b:c":value` → (`NAME`, params, `value`).
fn split_line(line: &str) -> Option<(String, Params, String)> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| match c {
        '"' => {
            quoted = !quoted;
            None
        }
        ':' if !quoted => Some(i),
        _ => None,
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((
                k.trim().to_ascii_uppercase(),
                v.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect();
    Some((name, params, value.to_owned()))
}

/// RFC 5545 text: `\n`, `\,`, `\;`, `\\`.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn clip_chars(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        s.chars().take(max).collect()
    }
}

fn clip_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::from_second(1_700_000_000).expect("ts")
    }
    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }
    fn dt(s: &str) -> DateTime {
        s.parse().expect("datetime")
    }
    fn kolkata() -> TimeZone {
        TimeZone::fixed(jiff::tz::Offset::from_seconds(5 * 3600 + 1800).expect("offset"))
    }
    fn import(text: &str) -> IcsImport {
        let local = TimeZone::fixed(jiff::tz::offset(1));
        let zones = |name: &str| (name == "Asia/Kolkata").then(kolkata);
        parse_ics(text, &local, &zones, now()).expect("parse")
    }
    fn cal(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n")
    }

    #[test]
    fn rejects_non_calendars() {
        let local = TimeZone::UTC;
        assert!(parse_ics("hello", &local, &|_| None, now()).is_err());
    }

    #[test]
    fn reads_timed_all_day_and_text() {
        let text = cal(concat!(
            "BEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Dentist\\, Dr. Rao\r\n",
            "DESCRIPTION:Bring the X-ray\\nand forms\r\nLOCATION:Main St\r\n",
            "DTSTART:20261110T090000\r\nDTEND:20261110T093000\r\n",
            "BEGIN:VALARM\r\nSUMMARY:not the title\r\nEND:VALARM\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:2\r\nSUMMARY:Trip to\r\n  Coorg\r\n",
            "DTSTART;VALUE=DATE:20261120\r\nDTEND;VALUE=DATE:20261123\r\nEND:VEVENT\r\n",
        ));
        let out = import(&text);
        assert_eq!(out.events.len(), 2);
        let dentist = &out.events[0];
        assert_eq!(dentist.title(), "Dentist, Dr. Rao");
        assert_eq!(dentist.notes(), "Bring the X-ray\nand forms");
        assert_eq!(dentist.location(), "Main St");
        assert_eq!(
            (dentist.start(), dentist.end()),
            (dt("2026-11-10T09:00"), dt("2026-11-10T09:30"))
        );
        let trip = &out.events[1];
        assert_eq!(trip.title(), "Trip to Coorg", "folded line");
        assert!(trip.is_all_day());
        assert_eq!(trip.last_day(), d("2026-11-22"));
    }

    #[test]
    fn converts_utc_and_named_zones_to_local() {
        let text = cal(concat!(
            "BEGIN:VEVENT\r\nSUMMARY:Call\r\nDTSTART:20261110T080000Z\r\nDURATION:PT45M\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nSUMMARY:Standup\r\nDTSTART;TZID=Asia/Kolkata:20261110T143000\r\n",
            "DTEND;TZID=Asia/Kolkata:20261110T150000\r\nEND:VEVENT\r\n",
        ));
        let out = import(&text);
        // Local is UTC+1.
        assert_eq!(out.events[0].start(), dt("2026-11-10T09:00"));
        assert_eq!(out.events[0].end(), dt("2026-11-10T09:45"));
        assert_eq!(out.events[1].start(), dt("2026-11-10T10:00"));
    }

    #[test]
    fn keeps_supported_repeats_with_ends_and_exceptions() {
        let text = cal(concat!(
            "BEGIN:VEVENT\r\nUID:s\r\nSUMMARY:Standup\r\nDTSTART:20261102T093000\r\nDTEND:20261102T094500\r\n",
            "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;UNTIL=20261113T235959Z\r\n",
            "EXDATE:20261104T093000\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID:20261106T093000\r\nSUMMARY:Standup\r\n",
            "DTSTART:20261106T110000\r\nDTEND:20261106T111500\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:r\r\nSUMMARY:Rent\r\nDTSTART;VALUE=DATE:20261101\r\n",
            "RRULE:FREQ=MONTHLY;COUNT=3\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Book club\r\nDTSTART:20261110T190000\r\n",
            "RRULE:FREQ=MONTHLY;BYDAY=2TU\r\nEND:VEVENT\r\n",
        ));
        let out = import(&text);
        assert_eq!(out.events.len(), 3);
        assert_eq!(out.simplified, 1, "second Tuesday can't be expressed");
        let standup = &out.events[0];
        assert_eq!(standup.recurrence(), Some(Recurrence::WORKDAYS));
        assert_eq!(
            standup.until(),
            Some(d("2026-11-14")),
            "UTC end, local date"
        );
        let all = standup.occurrences(d("2026-11-01")..d("2026-12-01"), 100);
        assert_eq!(all.len(), 9, "10 weekdays minus the skipped Wednesday");
        assert!(
            all.iter()
                .any(|o| o.start == dt("2026-11-06T11:00") && o.moved)
        );
        let rent = &out.events[1];
        assert_eq!(rent.until(), Some(d("2027-01-01")), "COUNT=3");
        assert!(out.events[2].recurrence().is_none());
    }

    #[test]
    fn cancelled_and_broken_events_are_left_out() {
        let text = cal(concat!(
            "BEGIN:VEVENT\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261110T090000\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nSUMMARY:No start\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nDTSTART:20261110T090000\r\nEND:VEVENT\r\n",
        ));
        let out = import(&text);
        assert_eq!(out.skipped, 1);
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0].title(), "(No title)");
        assert_eq!(
            out.events[0].end(),
            dt("2026-11-10T10:00"),
            "an hour by default"
        );
    }

    #[test]
    fn durations_parse() {
        assert_eq!(
            parse_duration("PT1H30M").map(|s| s.total(jiff::Unit::Minute).ok()),
            Some(Some(90.0))
        );
        assert!(parse_duration("P1D").is_some());
        assert!(parse_duration("bad").is_none());
        assert!(parse_duration("PT").is_none());
    }
}
