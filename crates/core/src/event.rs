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
//! State machine: `Active ──trash──▶ Trashed ──restore──▶ Active`; a trashed
//! event can't be edited.

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
    #[must_use]
    pub fn recurrence(&self) -> Option<Recurrence> {
        self.recurrence
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
        self.touch(now);
        Ok(())
    }

    pub fn set_recurrence(&mut self, rule: Option<Recurrence>, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.recurrence = rule;
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

    /// Every occurrence that touches a day in `days` (end exclusive), in order.
    /// Bounded: at most `limit` occurrences.
    #[must_use]
    pub fn occurrences(&self, days: Range<Date>, limit: usize) -> Vec<Occurrence> {
        let mut out = Vec::new();
        if self.is_trashed() || days.start >= days.end {
            return out;
        }
        let length = self.end - self.start;
        let mut start = self.start;
        // Skip quickly to near the range for long-running repeats.
        let make = |start: DateTime| -> Option<Occurrence> {
            Some(Occurrence {
                event: self.id,
                start,
                end: start.checked_add(length).ok()?,
                all_day: self.all_day,
            })
        };
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
            let next_date = rule.next_after(start.date());
            if next_date <= start.date() {
                break;
            }
            start = next_date.to_datetime(start.time());
        }
        out
    }
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
}
