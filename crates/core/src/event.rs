//! Calendar events.
//!
//! Times are local wall-clock times (`civil::DateTime`, no time zone): a 9:00
//! standup stays at 9:00 when you travel, which is what a personal calendar
//! wants. Time zones arrive with sync/ICS import (ADR to follow then).
//!
//! An event either has times (`start < end`) or is all-day (`start`/`end` are
//! midnights; `end` is exclusive, so a one-day event ends the next midnight,
//! like ICS). A repeating event stores its first occurrence and a
//! [`Recurrence`]; [`Event::occurrences`] expands it for a date range.
//!
//! One occurrence of a repeating event can be changed on its own ("this
//! event only"): an [`Exception`] keyed by the date it *would* have had
//! (its series date) either skips it or gives it other times. Changing the
//! series' schedule (times, dates, all-day, repeat rule) clears exceptions,
//! because their series dates no longer line up; title, notes and place
//! changes keep them.
//!
//! State machine: `Active ──trash──▶ Trashed ──restore──▶ Active`; a trashed
//! event can't be edited.

use std::collections::BTreeMap;
use std::ops::Range;

use jiff::civil::{Date, DateTime, Time};
use jiff::{Span, Timestamp};

use crate::error::{DomainError, DomainResult};
use crate::id::EventId;
use crate::recurrence::Recurrence;
use crate::text::normalize_title;

/// Longest event the model accepts (keeps expansion and layout bounded).
pub const MAX_EVENT_DAYS: i64 = 366;
/// Notes and location limits.
pub const MAX_EVENT_NOTES_BYTES: usize = 64 * 1024;
pub const MAX_LOCATION_CHARS: usize = 200;
/// Most per-occurrence changes one series keeps (bounds storage and expansion).
pub const MAX_EXCEPTIONS: usize = 1000;

/// How one occurrence of a repeating event differs from the series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    /// Deleted on its own ("Delete this event only").
    Skipped,
    /// Happens at other times (same length rules as the event).
    Moved { start: DateTime, end: DateTime },
}

/// Fixed-width text for the storage code (sortable, round-trips exactly).
fn dt_code(dt: DateTime) -> String {
    dt.strftime("%Y-%m-%dT%H:%M:%S").to_string()
}

/// `2026-11-10:x;2026-11-12:2026-11-13T10:00:00/2026-11-13T11:00:00`.
/// Stored by `keepstone-storage`; never change the format.
#[must_use]
pub fn exceptions_to_code(exceptions: &BTreeMap<Date, Exception>) -> Option<String> {
    if exceptions.is_empty() {
        return None;
    }
    let parts: Vec<String> = exceptions
        .iter()
        .map(|(date, e)| match e {
            Exception::Skipped => format!("{date}:x"),
            Exception::Moved { start, end } => {
                format!("{date}:{}/{}", dt_code(*start), dt_code(*end))
            }
        })
        .collect();
    Some(parts.join(";"))
}

/// Parses [`exceptions_to_code`]. Entries that don't parse are dropped
/// (a newer version's data shouldn't make the event unreadable).
#[must_use]
pub fn exceptions_from_code(code: &str) -> BTreeMap<Date, Exception> {
    code.split(';')
        .filter_map(|part| {
            let (date, rest) = part.split_once(':')?;
            let date: Date = date.parse().ok()?;
            let e = if rest == "x" {
                Exception::Skipped
            } else {
                let (start, end) = rest.split_once('/')?;
                let (start, end): (DateTime, DateTime) = (start.parse().ok()?, end.parse().ok()?);
                if end <= start {
                    return None;
                }
                Exception::Moved { start, end }
            };
            Some((date, e))
        })
        .take(MAX_EXCEPTIONS)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    id: EventId,
    title: String,
    notes: String,
    location: String,
    all_day: bool,
    start: DateTime,
    end: DateTime,
    recurrence: Option<Recurrence>,
    exceptions: BTreeMap<Date, Exception>,
    created_at: Timestamp,
    updated_at: Timestamp,
    deleted_at: Option<Timestamp>,
}

/// Every field, for storage round-trips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub id: EventId,
    pub title: String,
    pub notes: String,
    pub location: String,
    pub all_day: bool,
    pub start: DateTime,
    pub end: DateTime,
    pub recurrence: Option<Recurrence>,
    pub exceptions: BTreeMap<Date, Exception>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub deleted_at: Option<Timestamp>,
}

/// One time an event happens, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub event: EventId,
    pub start: DateTime,
    pub end: DateTime,
    pub all_day: bool,
    /// The date the series puts it on (differs from `start` once moved).
    /// Identifies the occurrence for "this event only" changes.
    pub series_date: Date,
    /// Changed on its own (an [`Exception::Moved`]).
    pub moved: bool,
}

impl Occurrence {
    /// The days this occurrence touches (end exclusive, at least one day).
    #[must_use]
    pub fn days(&self) -> Range<Date> {
        let first = self.start.date();
        let mut last = self.end.date();
        // The end is exclusive: an end at midnight (every all-day event, and a
        // timed event ending at 00:00) doesn't touch that day.
        if self.end.time() == Time::midnight() && last > first {
            last = last.yesterday().unwrap_or(last);
        }
        first..last.tomorrow().unwrap_or(last)
    }
}

impl Event {
    /// A timed event.
    pub fn new_timed(
        title: &str,
        start: DateTime,
        end: DateTime,
        now: Timestamp,
    ) -> DomainResult<Self> {
        let mut e = Self::blank(title, start, now)?;
        e.set_times(start, end, now)?;
        Ok(e)
    }

    /// An all-day event on `first..=last` (inclusive dates).
    pub fn new_all_day(title: &str, first: Date, last: Date, now: Timestamp) -> DomainResult<Self> {
        let mut e = Self::blank(title, first.to_datetime(Time::midnight()), now)?;
        e.set_all_day_dates(first, last, now)?;
        Ok(e)
    }

    fn blank(title: &str, start: DateTime, now: Timestamp) -> DomainResult<Self> {
        Ok(Self {
            id: EventId::new(),
            title: normalize_title(title)?,
            notes: String::new(),
            location: String::new(),
            all_day: false,
            start,
            end: start,
            recurrence: None,
            exceptions: BTreeMap::new(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    #[must_use]
    pub fn from_record(r: EventRecord) -> Self {
        Self {
            id: r.id,
            title: r.title,
            notes: r.notes,
            location: r.location,
            all_day: r.all_day,
            start: r.start,
            end: r.end,
            recurrence: r.recurrence,
            // Exceptions only mean something on a repeating event.
            exceptions: if r.recurrence.is_some() {
                r.exceptions
            } else {
                BTreeMap::new()
            },
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }

    #[must_use]
    pub fn to_record(&self) -> EventRecord {
        EventRecord {
            id: self.id,
            title: self.title.clone(),
            notes: self.notes.clone(),
            location: self.location.clone(),
            all_day: self.all_day,
            start: self.start,
            end: self.end,
            recurrence: self.recurrence,
            exceptions: self.exceptions.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
        }
    }

    // ---- Accessors ----

    #[must_use]
    pub fn id(&self) -> EventId {
        self.id
    }
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    #[must_use]
    pub fn notes(&self) -> &str {
        &self.notes
    }
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }
    #[must_use]
    pub fn is_all_day(&self) -> bool {
        self.all_day
    }
    #[must_use]
    pub fn start(&self) -> DateTime {
        self.start
    }
    #[must_use]
    pub fn end(&self) -> DateTime {
        self.end
    }
    /// Inclusive last day of an all-day event.
    #[must_use]
    pub fn last_day(&self) -> Date {
        if self.all_day {
            self.end.date().yesterday().unwrap_or(self.end.date())
        } else {
            self.end.date()
        }
    }
    /// A routine: repeats at least three times a week (daily, weekdays,
    /// Mon/Wed/Fri…). Calendars show routines quietly so one-off plans stand out.
    #[must_use]
    pub fn is_routine(&self) -> bool {
        match self.recurrence {
            Some(Recurrence::Days(n)) => n <= 2,
            Some(Recurrence::Weeks { every: 1, days }) => days.len() >= 3,
            _ => false,
        }
    }

    #[must_use]
    pub fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
    }
    /// Per-occurrence changes, by series date.
    #[must_use]
    pub fn exceptions(&self) -> &BTreeMap<Date, Exception> {
        &self.exceptions
    }
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    #[must_use]
    pub fn is_trashed(&self) -> bool {
        self.deleted_at.is_some()
    }

    // ---- Changes ----

    fn editable(&self) -> DomainResult<()> {
        if self.is_trashed() {
            return Err(DomainError::Deleted);
        }
        Ok(())
    }

    fn touch(&mut self, now: Timestamp) {
        self.updated_at = now;
    }

    pub fn rename(&mut self, title: &str, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.title = normalize_title(title)?;
        self.touch(now);
        Ok(())
    }

    pub fn set_notes(&mut self, notes: &str, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        if notes.len() > MAX_EVENT_NOTES_BYTES {
            return Err(DomainError::BodyTooLarge {
                max_bytes: MAX_EVENT_NOTES_BYTES,
            });
        }
        self.notes = notes.to_owned();
        self.touch(now);
        Ok(())
    }

    pub fn set_location(&mut self, location: &str, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        let location = location.trim();
        if location.chars().count() > MAX_LOCATION_CHARS {
            return Err(DomainError::TitleTooLong {
                max: MAX_LOCATION_CHARS,
            });
        }
        self.location = location.to_owned();
        self.touch(now);
        Ok(())
    }

    /// Makes the event timed, from `start` to `end` (`start < end`, bounded length).
    pub fn set_times(
        &mut self,
        start: DateTime,
        end: DateTime,
        now: Timestamp,
    ) -> DomainResult<()> {
        self.editable()?;
        if end <= start {
            return Err(DomainError::InvalidTimeRange);
        }
        if start
            .date()
            .checked_add(Span::new().days(MAX_EVENT_DAYS))
            .ok()
            .is_some_and(|limit| end.date() > limit)
        {
            return Err(DomainError::InvalidTimeRange);
        }
        self.all_day = false;
        self.start = start;
        self.end = end;
        self.exceptions.clear();
        self.touch(now);
        Ok(())
    }

    /// Makes the event all-day on `first..=last`.
    pub fn set_all_day_dates(
        &mut self,
        first: Date,
        last: Date,
        now: Timestamp,
    ) -> DomainResult<()> {
        self.editable()?;
        if last < first {
            return Err(DomainError::InvalidTimeRange);
        }
        let end = last.tomorrow().map_err(|_| DomainError::InvalidTimeRange)?;
        if (end - first).get_days() as i64 > MAX_EVENT_DAYS {
            return Err(DomainError::InvalidTimeRange);
        }
        self.all_day = true;
        self.start = first.to_datetime(Time::midnight());
        self.end = end.to_datetime(Time::midnight());
        self.exceptions.clear();
        self.touch(now);
        Ok(())
    }

    /// Moves the whole event so it starts on `date`, keeping times and length.
    pub fn move_to_date(&mut self, date: Date, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        let shift = date - self.start.date();
        let start = self
            .start
            .checked_add(shift)
            .map_err(|_| DomainError::InvalidTimeRange)?;
        let end = self
            .end
            .checked_add(shift)
            .map_err(|_| DomainError::InvalidTimeRange)?;
        self.start = start;
        self.end = end;
        self.exceptions.clear();
        self.touch(now);
        Ok(())
    }

    pub fn set_recurrence(&mut self, rule: Option<Recurrence>, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        if rule != self.recurrence {
            self.exceptions.clear();
        }
        self.recurrence = rule;
        self.touch(now);
        Ok(())
    }

    // ---- One occurrence ----

    /// The occurrence the series puts on `series_date`, with any exception
    /// applied (`None` if the series has no occurrence that day, or it was
    /// skipped).
    #[must_use]
    pub fn occurrence_on(&self, series_date: Date) -> Option<Occurrence> {
        let regular = self.regular_on(series_date)?;
        match self.exceptions.get(&series_date) {
            None => Some(regular),
            Some(Exception::Skipped) => None,
            Some(Exception::Moved { start, end }) => Some(Occurrence {
                start: *start,
                end: *end,
                moved: true,
                ..regular
            }),
        }
    }

    /// The unchanged occurrence starting on `series_date`, if the series has one.
    fn regular_on(&self, series_date: Date) -> Option<Occurrence> {
        let next = series_date.tomorrow().ok()?;
        // Room for every occurrence that can touch one day (long repeats overlap).
        self.series(series_date..next, MAX_EVENT_DAYS as usize + 1, false)
            .into_iter()
            .find(|o| o.series_date == series_date)
    }

    fn exception_target(&self, series_date: Date) -> DomainResult<Occurrence> {
        self.editable()?;
        if self.recurrence.is_none() {
            return Err(DomainError::NoSuchOccurrence);
        }
        self.regular_on(series_date)
            .ok_or(DomainError::NoSuchOccurrence)
    }

    fn insert_exception(&mut self, series_date: Date, e: Exception) -> DomainResult<()> {
        if !self.exceptions.contains_key(&series_date) && self.exceptions.len() >= MAX_EXCEPTIONS {
            return Err(DomainError::NoSuchOccurrence);
        }
        self.exceptions.insert(series_date, e);
        Ok(())
    }

    /// "Delete this event only". Returns the exception it replaced, for Undo.
    pub fn skip_occurrence(
        &mut self,
        series_date: Date,
        now: Timestamp,
    ) -> DomainResult<Option<Exception>> {
        self.exception_target(series_date)?;
        let previous = self.exceptions.get(&series_date).copied();
        self.insert_exception(series_date, Exception::Skipped)?;
        self.touch(now);
        Ok(previous)
    }

    /// "Change this event only": the occurrence on `series_date` happens
    /// `start..end` instead (same rules as [`Event::set_times`]; all-day
    /// events pass midnights). Moving it back to its series slot removes
    /// the exception. Returns the exception it replaced, for Undo.
    pub fn move_occurrence(
        &mut self,
        series_date: Date,
        start: DateTime,
        end: DateTime,
        now: Timestamp,
    ) -> DomainResult<Option<Exception>> {
        let regular = self.exception_target(series_date)?;
        if end <= start {
            return Err(DomainError::InvalidTimeRange);
        }
        if start
            .date()
            .checked_add(Span::new().days(MAX_EVENT_DAYS))
            .ok()
            .is_some_and(|limit| end.date() > limit)
        {
            return Err(DomainError::InvalidTimeRange);
        }
        if self.all_day && (start.time() != Time::midnight() || end.time() != Time::midnight()) {
            return Err(DomainError::InvalidTimeRange);
        }
        let previous = self.exceptions.get(&series_date).copied();
        if (start, end) == (regular.start, regular.end) {
            self.exceptions.remove(&series_date);
        } else {
            self.insert_exception(series_date, Exception::Moved { start, end })?;
        }
        self.touch(now);
        Ok(previous)
    }

    /// Puts back an exception returned by [`Event::skip_occurrence`] or
    /// [`Event::move_occurrence`] (`None` = back to the series).
    pub fn restore_exception(
        &mut self,
        series_date: Date,
        previous: Option<Exception>,
        now: Timestamp,
    ) -> DomainResult<()> {
        self.exception_target(series_date)?;
        match previous {
            Some(e) => self.insert_exception(series_date, e)?,
            None => {
                self.exceptions.remove(&series_date);
            }
        }
        self.touch(now);
        Ok(())
    }

    pub fn trash(&mut self, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.deleted_at = Some(now);
        self.touch(now);
        Ok(())
    }

    pub fn restore(&mut self, now: Timestamp) {
        if self.deleted_at.take().is_some() {
            self.touch(now);
        }
    }

    // ---- Occurrences ----

    /// Every occurrence that touches a day in `days` (end exclusive), in
    /// start order, with exceptions applied. Bounded: at most `limit`.
    #[must_use]
    pub fn occurrences(&self, days: Range<Date>, limit: usize) -> Vec<Occurrence> {
        if self.is_trashed() || days.start >= days.end {
            return Vec::new();
        }
        if self.exceptions.is_empty() {
            return self.series(days, limit, false);
        }
        let mut out: Vec<Occurrence> = self
            .series(days.clone(), limit, true)
            .into_iter()
            .filter(|o| !self.exceptions.contains_key(&o.series_date))
            .collect();
        // Moved occurrences can land in the range from a series date outside it.
        out.extend(self.exceptions.iter().filter_map(|(date, e)| {
            let Exception::Moved { start, end } = *e else {
                return None;
            };
            // Validated when it was made; schedule changes clear exceptions.
            let occ = Occurrence {
                event: self.id,
                start,
                end,
                all_day: self.all_day,
                series_date: *date,
                moved: true,
            };
            let span = occ.days();
            (span.start < days.end && span.end > days.start).then_some(occ)
        }));
        out.sort_by_key(|o| (o.start, o.series_date));
        out.truncate(limit);
        out
    }

    /// The series without exceptions. With `extra`, the limit also leaves
    /// room for the exceptions that will be filtered out.
    fn series(&self, days: Range<Date>, limit: usize, extra: bool) -> Vec<Occurrence> {
        let mut out = Vec::new();
        let limit = if extra {
            limit.saturating_add(self.exceptions.len())
        } else {
            limit
        };
        let length = self.end - self.start;
        let mut start = self.start;
        let make = |start: DateTime| -> Option<Occurrence> {
            Some(Occurrence {
                event: self.id,
                start,
                end: start.checked_add(length).ok()?,
                all_day: self.all_day,
                series_date: start.date(),
                moved: false,
            })
        };
        let mut k = 0_i64;
        for _ in 0..100_000 {
            let Some(occ) = make(start) else { break };
            let span = occ.days();
            if span.start >= days.end {
                break;
            }
            if span.end > days.start {
                out.push(occ);
                if out.len() >= limit {
                    break;
                }
            }
            let Some(rule) = self.recurrence else { break };
            k += 1;
            let Some(next_date) = nth_date(rule, self.start.date(), start.date(), k) else {
                break;
            };
            if next_date <= start.date() {
                break;
            }
            start = next_date.to_datetime(start.time());
        }
        out
    }
}

/// The `k`-th repeat after `first`. Months and years count from the first
/// date so a series on the 31st stays on month ends instead of drifting to
/// the 28th after February. Weekly rules step from the previous date.
fn nth_date(rule: Recurrence, first: Date, previous: Date, k: i64) -> Option<Date> {
    let span = match rule {
        Recurrence::Days(n) => Span::new().try_days(i64::from(n.max(1)) * k).ok()?,
        Recurrence::Months(n) => Span::new().try_months(i64::from(n.max(1)) * k).ok()?,
        Recurrence::Years(n) => Span::new().try_years(i64::from(n.max(1)) * k).ok()?,
        Recurrence::Weeks { .. } => return Some(rule.next_after(previous)),
    };
    first.checked_add(span).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn now() -> Timestamp {
        Timestamp::from_second(1_700_000_000).expect("ts")
    }
    fn dt(s: &str) -> DateTime {
        s.parse().expect("datetime")
    }
    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }

    #[test]
    fn timed_events_need_a_positive_bounded_length() {
        assert!(
            Event::new_timed(
                "Lunch",
                dt("2026-11-10T12:00"),
                dt("2026-11-10T13:00"),
                now()
            )
            .is_ok()
        );
        assert_eq!(
            Event::new_timed("Bad", dt("2026-11-10T12:00"), dt("2026-11-10T12:00"), now()).err(),
            Some(DomainError::InvalidTimeRange)
        );
        assert!(
            Event::new_timed(
                "Long",
                dt("2026-01-01T00:00"),
                dt("2027-06-01T00:00"),
                now()
            )
            .is_err()
        );
        assert_eq!(
            Event::new_timed("  ", dt("2026-11-10T12:00"), dt("2026-11-10T13:00"), now()).err(),
            Some(DomainError::EmptyTitle)
        );
    }

    #[test]
    fn all_day_events_end_at_the_next_midnight() {
        let e = Event::new_all_day("Trip", d("2026-11-20"), d("2026-11-22"), now()).expect("event");
        assert!(e.is_all_day());
        assert_eq!(e.start(), dt("2026-11-20T00:00"));
        assert_eq!(e.end(), dt("2026-11-23T00:00"));
        assert_eq!(e.last_day(), d("2026-11-22"));
        let occ = e.occurrences(d("2026-11-01")..d("2026-12-01"), 10);
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].days(), d("2026-11-20")..d("2026-11-23"));
    }

    #[test]
    fn timed_events_ending_at_midnight_stay_on_their_day() {
        let e = Event::new_timed(
            "Late",
            dt("2026-11-10T22:00"),
            dt("2026-11-11T00:00"),
            now(),
        )
        .expect("event");
        let occ = e.occurrences(d("2026-11-11")..d("2026-11-12"), 10);
        assert!(occ.is_empty(), "doesn't show on the 11th");
        let overnight = Event::new_timed(
            "Flight",
            dt("2026-11-10T22:00"),
            dt("2026-11-11T06:00"),
            now(),
        )
        .expect("event");
        assert_eq!(
            overnight
                .occurrences(d("2026-11-11")..d("2026-11-12"), 10)
                .len(),
            1
        );
    }

    #[test]
    fn repeating_events_expand_within_the_range() {
        let mut e = Event::new_timed(
            "Standup",
            dt("2026-11-02T09:30"),
            dt("2026-11-02T09:45"),
            now(),
        )
        .expect("event");
        e.set_recurrence(Some(Recurrence::WORKDAYS), now())
            .expect("repeat");
        let week = e.occurrences(d("2026-11-09")..d("2026-11-16"), 100);
        assert_eq!(week.len(), 5, "Mon–Fri");
        assert_eq!(week[0].start, dt("2026-11-09T09:30"));
        assert_eq!(week[4].end, dt("2026-11-13T09:45"));
        assert_eq!(
            e.occurrences(d("2026-11-09")..d("2026-11-16"), 2).len(),
            2,
            "limit"
        );
        assert!(
            e.occurrences(d("2026-10-01")..d("2026-11-01"), 10)
                .is_empty(),
            "nothing before the first"
        );
    }

    #[test]
    fn monthly_repeats_on_the_31st_stay_at_month_end() {
        let mut e =
            Event::new_all_day("Rent", d("2026-01-31"), d("2026-01-31"), now()).expect("event");
        e.set_recurrence(Some(Recurrence::MONTHLY), now())
            .expect("repeat");
        let starts: Vec<Date> = e
            .occurrences(d("2026-01-01")..d("2026-05-01"), 10)
            .iter()
            .map(|o| o.start.date())
            .collect();
        assert_eq!(
            starts,
            [
                d("2026-01-31"),
                d("2026-02-28"),
                d("2026-03-31"),
                d("2026-04-30")
            ]
        );
    }

    #[test]
    fn routines_repeat_three_or_more_times_a_week() {
        let mut e = Event::new_timed("Gym", dt("2026-11-10T07:00"), dt("2026-11-10T08:00"), now())
            .expect("event");
        assert!(!e.is_routine());
        for (rule, routine) in [
            (Recurrence::DAILY, true),
            (Recurrence::WORKDAYS, true),
            (Recurrence::WEEKLY, false),
            (Recurrence::MONTHLY, false),
            (Recurrence::Days(3), false),
        ] {
            e.set_recurrence(Some(rule), now()).expect("repeat");
            assert_eq!(e.is_routine(), routine, "{rule}");
        }
    }

    #[test]
    fn moving_keeps_time_and_length() {
        let mut e = Event::new_timed(
            "Dentist",
            dt("2026-11-10T15:00"),
            dt("2026-11-10T16:30"),
            now(),
        )
        .expect("event");
        e.move_to_date(d("2026-11-12"), now()).expect("move");
        assert_eq!(
            (e.start(), e.end()),
            (dt("2026-11-12T15:00"), dt("2026-11-12T16:30"))
        );
    }

    #[test]
    fn trashed_events_are_frozen_and_hidden() {
        let mut e = Event::new_timed("X", dt("2026-11-10T15:00"), dt("2026-11-10T16:00"), now())
            .expect("event");
        e.trash(now()).expect("trash");
        assert_eq!(e.rename("Y", now()).err(), Some(DomainError::Deleted));
        assert!(
            e.occurrences(d("2026-11-01")..d("2026-12-01"), 10)
                .is_empty()
        );
        e.restore(now());
        assert!(e.rename("Y", now()).is_ok());
        let _ = date(2026, 1, 1);
    }

    #[test]
    fn records_round_trip() {
        let mut e =
            Event::new_all_day("Trip", d("2026-11-20"), d("2026-11-22"), now()).expect("event");
        e.set_location("Coorg", now()).expect("loc");
        e.set_notes("Pack boots", now()).expect("notes");
        assert_eq!(Event::from_record(e.to_record()), e);
    }

    fn standup() -> Event {
        let mut e = Event::new_timed(
            "Standup",
            dt("2026-11-02T09:30"),
            dt("2026-11-02T09:45"),
            now(),
        )
        .expect("event");
        e.set_recurrence(Some(Recurrence::WORKDAYS), now())
            .expect("repeat");
        e
    }

    fn starts(e: &Event, from: &str, to: &str) -> Vec<DateTime> {
        e.occurrences(d(from)..d(to), 100)
            .iter()
            .map(|o| o.start)
            .collect()
    }

    #[test]
    fn one_occurrence_can_be_skipped_and_restored() {
        let mut e = standup();
        let previous = e.skip_occurrence(d("2026-11-11"), now()).expect("skip");
        assert_eq!(previous, None);
        let week = starts(&e, "2026-11-09", "2026-11-16");
        assert_eq!(week.len(), 4);
        assert!(!week.contains(&dt("2026-11-11T09:30")));
        assert_eq!(e.occurrence_on(d("2026-11-11")), None);
        e.restore_exception(d("2026-11-11"), previous, now())
            .expect("undo");
        assert_eq!(starts(&e, "2026-11-09", "2026-11-16").len(), 5);
        // Saturday isn't in the series; one-off events have no occurrences to change.
        assert_eq!(
            e.skip_occurrence(d("2026-11-14"), now()),
            Err(DomainError::NoSuchOccurrence)
        );
        let mut once = Event::new_timed("X", dt("2026-11-10T15:00"), dt("2026-11-10T16:00"), now())
            .expect("event");
        assert_eq!(
            once.skip_occurrence(d("2026-11-10"), now()),
            Err(DomainError::NoSuchOccurrence)
        );
    }

    #[test]
    fn one_occurrence_can_move_even_outside_the_range() {
        let mut e = standup();
        // Friday's standup moves to the next Monday afternoon.
        e.move_occurrence(
            d("2026-11-13"),
            dt("2026-11-16T14:00"),
            dt("2026-11-16T14:30"),
            now(),
        )
        .expect("move");
        let this_week = starts(&e, "2026-11-09", "2026-11-14");
        assert_eq!(this_week.len(), 4, "Friday is gone from its week");
        let next_monday = e.occurrences(d("2026-11-16")..d("2026-11-17"), 10);
        assert_eq!(
            next_monday.iter().map(|o| o.start).collect::<Vec<_>>(),
            [dt("2026-11-16T09:30"), dt("2026-11-16T14:00")],
            "in start order, both on Monday"
        );
        assert!(next_monday[1].moved);
        assert_eq!(next_monday[1].series_date, d("2026-11-13"));
        assert_eq!(
            e.occurrence_on(d("2026-11-13")).map(|o| o.start),
            Some(dt("2026-11-16T14:00"))
        );
        // Moving it back to its slot drops the exception.
        e.move_occurrence(
            d("2026-11-13"),
            dt("2026-11-13T09:30"),
            dt("2026-11-13T09:45"),
            now(),
        )
        .expect("back");
        assert!(e.exceptions().is_empty());
        assert_eq!(
            e.move_occurrence(
                d("2026-11-13"),
                dt("2026-11-13T10:00"),
                dt("2026-11-13T10:00"),
                now()
            ),
            Err(DomainError::InvalidTimeRange)
        );
    }

    #[test]
    fn schedule_changes_clear_exceptions_but_text_changes_keep_them() {
        let mut e = standup();
        e.skip_occurrence(d("2026-11-11"), now()).expect("skip");
        e.rename("Daily sync", now()).expect("rename");
        e.set_location("Room 4", now()).expect("place");
        assert_eq!(e.exceptions().len(), 1);
        assert_eq!(Event::from_record(e.to_record()), e);
        e.set_times(dt("2026-11-02T10:00"), dt("2026-11-02T10:15"), now())
            .expect("times");
        assert!(e.exceptions().is_empty());
        e.skip_occurrence(d("2026-11-11"), now()).expect("skip");
        e.set_recurrence(Some(Recurrence::DAILY), now())
            .expect("rule");
        assert!(e.exceptions().is_empty());
    }

    #[test]
    fn all_day_occurrences_move_by_whole_days() {
        let mut e =
            Event::new_all_day("Bins", d("2026-11-03"), d("2026-11-03"), now()).expect("event");
        e.set_recurrence(Some(Recurrence::WEEKLY), now())
            .expect("repeat");
        assert_eq!(
            e.move_occurrence(
                d("2026-11-10"),
                dt("2026-11-11T09:00"),
                dt("2026-11-11T10:00"),
                now()
            ),
            Err(DomainError::InvalidTimeRange),
            "an all-day occurrence stays all-day"
        );
        e.move_occurrence(
            d("2026-11-10"),
            dt("2026-11-11T00:00"),
            dt("2026-11-12T00:00"),
            now(),
        )
        .expect("move");
        let occ = e.occurrences(d("2026-11-09")..d("2026-11-16"), 10);
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].days(), d("2026-11-11")..d("2026-11-12"));
    }

    #[test]
    fn exception_codes_round_trip_and_ignore_junk() {
        let mut e = standup();
        e.skip_occurrence(d("2026-11-11"), now()).expect("skip");
        e.move_occurrence(
            d("2026-11-12"),
            dt("2026-11-12T11:00"),
            dt("2026-11-12T11:20"),
            now(),
        )
        .expect("move");
        let code = exceptions_to_code(e.exceptions()).expect("code");
        assert_eq!(
            code,
            "2026-11-11:x;2026-11-12:2026-11-12T11:00:00/2026-11-12T11:20:00"
        );
        assert_eq!(&exceptions_from_code(&code), e.exceptions());
        assert_eq!(exceptions_to_code(&BTreeMap::new()), None);
        let junk = exceptions_from_code(
            "nope;2026-11-11:x;2026-11-12:bad/also;2026-11-13:2026-11-13T10:00:00/2026-11-13T09:00:00",
        );
        assert_eq!(junk.len(), 1);
    }
}
