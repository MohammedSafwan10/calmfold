//! Habits: something you want to do on a schedule, checked in per day.
//!
//! A habit has a [`Schedule`] (every day, on chosen weekdays, or N times a
//! week) and a daily goal (`target`: 1 for "did it", 8 for "8 glasses").
//! Check-ins are counts per local date; a day is *done* when its count
//! reaches the target.
//!
//! Progress is shown two ways (see [`Stats`]):
//! * **streak**: consecutive scheduled days (or weeks, for "N times a week")
//!   that were done. Today (this week) not done *yet* doesn't break it.
//! * **strength**: an exponential moving average over scheduled days
//!   (α = 0.05, half-life ≈ 2 weeks), so one missed day after a long run
//!   barely dents it. The idea comes from Loop Habit Tracker; the formula is
//!   the textbook EMA, re-derived here.
//!
//! State machine: `Active ──archive──▶ Archived ──unarchive──▶ Active`.
//! Archived habits keep their history but leave the daily list.

use std::collections::HashMap;
use std::fmt;

use jiff::Span;
use jiff::Timestamp;
use jiff::civil::{Date, Weekday};

use crate::error::{DomainError, DomainResult};
use crate::id::HabitId;
use crate::recurrence::Weekdays;
use crate::text::normalize_title;

/// Largest daily goal ("drink 8 glasses").
pub const MAX_TARGET: u16 = 99;
/// Smoothing for [`Stats::strength`].
const ALPHA: f64 = 0.05;
/// How far back statistics look (bounds work for very old habits).
pub const HISTORY_DAYS: i64 = 366 * 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Schedule {
    Daily,
    /// On these weekdays (never empty).
    Days(Weekdays),
    /// Any days, this many times per week (1..=6; 7 is `Daily`).
    PerWeek(u8),
}

impl Schedule {
    /// Whether a fixed-day schedule expects the habit on `day`. `PerWeek`
    /// habits can be done on any day, so this is always true for them.
    #[must_use]
    pub fn is_due(self, day: Date) -> bool {
        match self {
            Self::Daily | Self::PerWeek(_) => true,
            Self::Days(days) => days.contains(day.weekday()),
        }
    }

    /// Stable storage code: `d`, `w:31` (weekday bits), `n3`. Never change.
    #[must_use]
    pub fn to_code(self) -> String {
        match self {
            Self::Daily => "d".into(),
            Self::Days(days) => format!("w:{}", days.bits()),
            Self::PerWeek(n) => format!("n{n}"),
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        if code == "d" {
            return Some(Self::Daily);
        }
        if let Some(bits) = code.strip_prefix("w:") {
            let days = Weekdays::from_bits(bits.parse().ok()?);
            return (!days.is_empty()).then_some(Self::Days(days));
        }
        let n: u8 = code.strip_prefix('n')?.parse().ok()?;
        (1..=6).contains(&n).then_some(Self::PerWeek(n))
    }

    /// Normalizes equivalent forms: all seven days or 7×/week → `Daily`.
    fn normalized(self) -> DomainResult<Self> {
        match self {
            Self::Days(days) if days.is_empty() => Err(DomainError::InvalidSchedule),
            Self::Days(days) if days == Weekdays::ALL => Ok(Self::Daily),
            Self::PerWeek(0) => Err(DomainError::InvalidSchedule),
            Self::PerWeek(n) if n >= 7 => Ok(Self::Daily),
            other => Ok(other),
        }
    }
}

impl fmt::Display for Schedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Daily => f.write_str("Every day"),
            Self::Days(days) if days == Weekdays::WORKDAYS => f.write_str("Weekdays"),
            Self::Days(days) if days == Weekdays::from_bits(0b110_0000) => f.write_str("Weekends"),
            Self::Days(days) => {
                let names: Vec<_> = days.iter().map(short_day).collect();
                f.write_str(&names.join(", "))
            }
            Self::PerWeek(1) => f.write_str("Once a week"),
            Self::PerWeek(n) => write!(f, "{n} times a week"),
        }
    }
}

/// "Mon", "Tue", …
#[must_use]
pub fn short_day(day: Weekday) -> &'static str {
    match day {
        Weekday::Monday => "Mon",
        Weekday::Tuesday => "Tue",
        Weekday::Wednesday => "Wed",
        Weekday::Thursday => "Thu",
        Weekday::Friday => "Fri",
        Weekday::Saturday => "Sat",
        Weekday::Sunday => "Sun",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Habit {
    id: HabitId,
    name: String,
    schedule: Schedule,
    target: u16,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

/// Every field, for storage round-trips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HabitRecord {
    pub id: HabitId,
    pub name: String,
    pub schedule: Schedule,
    pub target: u16,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub archived_at: Option<Timestamp>,
}

impl Habit {
    pub fn new(name: &str, schedule: Schedule, now: Timestamp) -> DomainResult<Self> {
        Ok(Self {
            id: HabitId::new(),
            name: normalize_title(name)?,
            schedule: schedule.normalized()?,
            target: 1,
            created_at: now,
            updated_at: now,
            archived_at: None,
        })
    }

    #[must_use]
    pub fn from_record(r: HabitRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            schedule: r.schedule,
            target: r.target.clamp(1, MAX_TARGET),
            created_at: r.created_at,
            updated_at: r.updated_at,
            archived_at: r.archived_at,
        }
    }

    #[must_use]
    pub fn to_record(&self) -> HabitRecord {
        HabitRecord {
            id: self.id,
            name: self.name.clone(),
            schedule: self.schedule,
            target: self.target,
            created_at: self.created_at,
            updated_at: self.updated_at,
            archived_at: self.archived_at,
        }
    }

    #[must_use]
    pub fn id(&self) -> HabitId {
        self.id
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn schedule(&self) -> Schedule {
        self.schedule
    }
    #[must_use]
    pub fn target(&self) -> u16 {
        self.target
    }
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    fn editable(&self) -> DomainResult<()> {
        if self.is_archived() {
            return Err(DomainError::Archived);
        }
        Ok(())
    }

    pub fn rename(&mut self, name: &str, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.name = normalize_title(name)?;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_schedule(&mut self, schedule: Schedule, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.schedule = schedule.normalized()?;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_target(&mut self, target: u16, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        if !(1..=MAX_TARGET).contains(&target) {
            return Err(DomainError::InvalidTarget { max: MAX_TARGET });
        }
        self.target = target;
        self.updated_at = now;
        Ok(())
    }

    pub fn archive(&mut self, now: Timestamp) -> DomainResult<()> {
        self.editable()?;
        self.archived_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    pub fn unarchive(&mut self, now: Timestamp) {
        if self.archived_at.take().is_some() {
            self.updated_at = now;
        }
    }

    /// The count after one tap on `day`: +1 up to the target, then back to 0
    /// (a one-step habit toggles). Check-ins can't be in the future.
    pub fn tap(&self, current: u16, day: Date, today: Date) -> DomainResult<u16> {
        self.editable()?;
        if day > today {
            return Err(DomainError::FutureCheckIn);
        }
        Ok(if current >= self.target {
            0
        } else {
            current + 1
        })
    }

    /// Streaks, strength and rates as of `today` (see the module docs).
    /// `counts` maps dates to check-in counts; `first` is the first day that
    /// counts (the creation date, or an earlier check-in when backfilled).
    #[must_use]
    pub fn stats(&self, counts: &HashMap<Date, u16>, first: Date, today: Date) -> Stats {
        let done = |d: Date| counts.get(&d).is_some_and(|c| *c >= self.target);
        let limit = add_days(today, -HISTORY_DAYS);
        let first = first.max(limit).min(today);
        let total = counts
            .iter()
            .filter(|(d, c)| **d <= today && **c >= self.target)
            .count();
        match self.schedule {
            Schedule::PerWeek(n) => self.week_stats(n, &done, first, today, total),
            schedule => {
                // Walk scheduled days oldest → newest.
                let mut strength = 0.0;
                let (mut run, mut best) = (0usize, 0usize);
                let mut day = first;
                while day <= today {
                    if schedule.is_due(day) {
                        if done(day) {
                            run += 1;
                            best = best.max(run);
                            strength = (1. - ALPHA) * strength + ALPHA;
                        } else if day < today {
                            run = 0;
                            strength *= 1. - ALPHA;
                        }
                    }
                    day = add_days(day, 1);
                }
                let (due, hit) = rate_window(today, first, |d| schedule.is_due(d), &done);
                Stats {
                    current_streak: run,
                    best_streak: best,
                    streak_unit: StreakUnit::Days,
                    strength,
                    rate_30: (due > 0).then(|| hit as f64 / due as f64),
                    total,
                }
            }
        }
    }

    fn week_stats(
        &self,
        n: u8,
        done: &dyn Fn(Date) -> bool,
        first: Date,
        today: Date,
        total: usize,
    ) -> Stats {
        let n = usize::from(n);
        // Weeks start on Monday for counting; the display can differ.
        let week_of = |d: Date| add_days(d, -i64::from(d.weekday().to_monday_zero_offset()));
        let this_week = week_of(today);
        let alpha_week = 1. - (1. - ALPHA).powi(7);
        let mut strength = 0.0;
        let (mut run, mut best) = (0usize, 0usize);
        let mut week = week_of(first);
        while week <= this_week {
            let hits = (0..7)
                .map(|i| add_days(week, i))
                .filter(|d| *d >= first && *d <= today && done(*d))
                .count();
            let met = hits >= n;
            if week < this_week {
                strength =
                    (1. - alpha_week) * strength + alpha_week * (hits as f64 / n as f64).min(1.);
                if met {
                    run += 1;
                } else {
                    run = 0;
                }
            } else if met {
                // This week counts once the goal is met; until then it
                // neither adds nor breaks.
                strength = (1. - alpha_week) * strength + alpha_week;
                run += 1;
            }
            best = best.max(run);
            week = add_days(week, 7);
        }
        // Rate: share of the weekly goal met over the last 4 full weeks + this one.
        let mut due = 0;
        let mut hit = 0;
        for k in 0..5 {
            let week = add_days(this_week, -7 * k);
            if add_days(week, 6) < first {
                break;
            }
            let hits = (0..7)
                .map(|i| add_days(week, i))
                .filter(|d| *d >= first && *d <= today && done(*d))
                .count();
            due += n;
            hit += hits.min(n);
        }
        Stats {
            current_streak: run,
            best_streak: best,
            streak_unit: StreakUnit::Weeks,
            strength,
            rate_30: (due > 0).then(|| hit as f64 / due as f64),
            total,
        }
    }
}

/// Done share of scheduled days in the last 30 days (today counts only if done).
fn rate_window(
    today: Date,
    first: Date,
    is_due: impl Fn(Date) -> bool,
    done: &dyn Fn(Date) -> bool,
) -> (usize, usize) {
    let (mut due, mut hit) = (0, 0);
    for k in 0..30 {
        let d = add_days(today, -k);
        if d < first {
            break;
        }
        if !is_due(d) || (d == today && !done(d)) {
            continue;
        }
        due += 1;
        hit += usize::from(done(d));
    }
    (due, hit)
}

fn add_days(day: Date, n: i64) -> Date {
    day.checked_add(Span::new().days(n)).unwrap_or(day)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreakUnit {
    Days,
    Weeks,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub current_streak: usize,
    pub best_streak: usize,
    pub streak_unit: StreakUnit,
    /// 0.0..=1.0.
    pub strength: f64,
    /// Done share of the last 30 days' schedule; `None` before anything was due.
    pub rate_30: Option<f64>,
    /// Days ever done.
    pub total: usize,
}

impl Stats {
    /// "12 days", "1 week".
    #[must_use]
    pub fn streak_label(&self, n: usize) -> String {
        let unit = match (self.streak_unit, n) {
            (StreakUnit::Days, 1) => "day",
            (StreakUnit::Days, _) => "days",
            (StreakUnit::Weeks, 1) => "week",
            (StreakUnit::Weeks, _) => "weeks",
        };
        format!("{n} {unit}")
    }
}

/// Quick add: `Read 20 pages daily`, `Gym mon wed fri`, `Run 3x a week`,
/// `Water 8 times a day`, `Stretch weekdays`. Only the end of the text is
/// read; no schedule means every day.
#[must_use]
pub fn parse_habit(input: &str) -> HabitDraft {
    let mut words: Vec<&str> = input.split_whitespace().collect();
    let mut schedule = None;
    let mut target = None;
    loop {
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let n = lower.len();
        if n < 2 {
            break;
        }
        if schedule.is_none()
            && let Some((used, s)) = trailing_schedule(&lower)
        {
            schedule = Some(s);
            words.truncate(n - used);
            continue;
        }
        if target.is_none()
            && let Some((used, t)) = trailing_target(&lower)
        {
            target = Some(t);
            words.truncate(n - used);
            continue;
        }
        break;
    }
    let name = words.join(" ");
    HabitDraft {
        name: if name.trim().is_empty() {
            input.trim().to_owned()
        } else {
            name
        },
        schedule: schedule.unwrap_or(Schedule::Daily),
        target: target.unwrap_or(1),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HabitDraft {
    pub name: String,
    pub schedule: Schedule,
    pub target: u16,
}

fn times(word: &str) -> Option<u8> {
    let n = word
        .strip_suffix('x')
        .or_else(|| word.strip_suffix("×"))
        .unwrap_or(word);
    n.parse::<u8>().ok().filter(|n| *n >= 1)
}

fn trailing_schedule(w: &[String]) -> Option<(usize, Schedule)> {
    let n = w.len();
    let last = w[n - 1].as_str();
    match last {
        "daily" => return Some((1, Schedule::Daily)),
        "weekdays" => {
            let used = if n >= 3 && w[n - 2] == "on" { 2 } else { 1 };
            return Some((used, Schedule::Days(Weekdays::WORKDAYS)));
        }
        "weekends" => {
            let used = if n >= 3 && w[n - 2] == "on" { 2 } else { 1 };
            return Some((used, Schedule::Days(Weekdays::from_bits(0b110_0000))));
        }
        _ => {}
    }
    // "every day", "every weekday"
    if n >= 3 && w[n - 2] == "every" {
        match last {
            "day" => return Some((2, Schedule::Daily)),
            "weekday" => return Some((2, Schedule::Days(Weekdays::WORKDAYS))),
            _ => {}
        }
    }
    // "3x a week", "3 times a week", "3 times per week", "3x/week", "3/week", "once a week"
    if let Some(k) = last
        .strip_suffix("/week")
        .or_else(|| last.strip_suffix("/wk"))
    {
        let k = times(k).filter(|_| n >= 2)?;
        return Some((1, Schedule::PerWeek(k).normalized().ok()?));
    }
    if last == "week" && n >= 3 && matches!(w[n - 2].as_str(), "a" | "per") {
        let base = n - 2;
        if base >= 2 && matches!(w[base - 1].as_str(), "once") {
            return Some((3, Schedule::PerWeek(1)));
        }
        if base >= 2 && w[base - 1] == "twice" {
            return Some((3, Schedule::PerWeek(2)));
        }
        if base >= 3 && matches!(w[base - 1].as_str(), "times" | "days") {
            let k = times(&w[base - 2])?;
            return (base >= 3).then_some((4, Schedule::PerWeek(k).normalized().ok()?));
        }
        if base >= 2 {
            let k = times(&w[base - 1])?;
            return (base >= 2).then_some((3, Schedule::PerWeek(k).normalized().ok()?));
        }
    }
    // A run of weekday names at the end: "mon wed fri", "on tue, thu".
    let mut days = Weekdays::NONE;
    let mut used = 0;
    for word in w.iter().rev() {
        let word = word.trim_end_matches(',');
        let Some(day) = crate::recurrence::parse_weekday(word) else {
            break;
        };
        days = days.with(day);
        used += 1;
    }
    if used > 0 && used < n {
        if n - used >= 2 && matches!(w[n - used - 1].as_str(), "on" | "every") {
            used += 1;
        }
        return Some((used, Schedule::Days(days).normalized().ok()?));
    }
    None
}

/// "8 times a day", "8x a day", "8x daily" (the daily part read separately), "x8".
fn trailing_target(w: &[String]) -> Option<(usize, u16)> {
    let n = w.len();
    let target = |s: &str| {
        times(s)
            .map(u16::from)
            .filter(|t| (2..=MAX_TARGET).contains(t))
    };
    if n >= 4 && w[n - 1] == "day" && matches!(w[n - 2].as_str(), "a" | "per") {
        if w[n - 3] == "times" && n >= 5 {
            return Some((4, target(&w[n - 4])?));
        }
        if w[n - 3].ends_with('x') {
            return Some((3, target(&w[n - 3])?));
        }
    }
    if n >= 3 && w[n - 1] == "times" {
        return Some((2, target(&w[n - 2])?));
    }
    if w[n - 1].ends_with('x') {
        return Some((1, target(&w[n - 1])?));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }
    fn now() -> Timestamp {
        Timestamp::from_second(1_700_000_000).expect("ts")
    }
    fn counts(days: &[&str]) -> HashMap<Date, u16> {
        days.iter().map(|s| (d(s), 1)).collect()
    }
    fn range(first: &str, last: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut day = d(first);
        while day <= d(last) {
            out.push(day.to_string());
            day = add_days(day, 1);
        }
        out
    }
    fn counts_of(days: &[String]) -> HashMap<Date, u16> {
        days.iter().map(|s| (d(s), 1)).collect()
    }

    #[test]
    fn schedules_round_trip_and_normalize() {
        for s in [
            Schedule::Daily,
            Schedule::Days(Weekdays::WORKDAYS),
            Schedule::PerWeek(3),
        ] {
            assert_eq!(Schedule::from_code(&s.to_code()), Some(s));
        }
        assert_eq!(Schedule::from_code("w:0"), None);
        assert_eq!(Schedule::from_code("n9"), None);
        let h = Habit::new("X", Schedule::Days(Weekdays::ALL), now()).expect("habit");
        assert_eq!(h.schedule(), Schedule::Daily);
        assert_eq!(
            Habit::new("X", Schedule::Days(Weekdays::NONE), now()).err(),
            Some(DomainError::InvalidSchedule)
        );
        assert_eq!(Schedule::PerWeek(3).to_string(), "3 times a week");
        assert_eq!(Schedule::Days(Weekdays::WORKDAYS).to_string(), "Weekdays");
        let mwf = Weekdays::single(Weekday::Monday)
            .with(Weekday::Wednesday)
            .with(Weekday::Friday);
        assert_eq!(Schedule::Days(mwf).to_string(), "Mon, Wed, Fri");
    }

    #[test]
    fn taps_count_up_then_reset_and_never_in_the_future() {
        let mut h = Habit::new("Water", Schedule::Daily, now()).expect("habit");
        let today = d("2026-11-09");
        assert_eq!(h.tap(0, today, today), Ok(1));
        assert_eq!(h.tap(1, today, today), Ok(0), "one-step toggles");
        h.set_target(3, now()).expect("target");
        assert_eq!(h.tap(2, today, today), Ok(3));
        assert_eq!(h.tap(3, today, today), Ok(0));
        assert_eq!(
            h.tap(0, d("2026-11-10"), today),
            Err(DomainError::FutureCheckIn)
        );
        assert!(h.set_target(0, now()).is_err());
        h.archive(now()).expect("archive");
        assert_eq!(h.tap(0, today, today), Err(DomainError::Archived));
    }

    #[test]
    fn daily_streaks_survive_an_unfinished_today() {
        let h = Habit::new("Read", Schedule::Daily, now()).expect("habit");
        let c = counts(&["2026-11-05", "2026-11-06", "2026-11-07", "2026-11-08"]);
        let s = h.stats(&c, d("2026-11-01"), d("2026-11-09"));
        assert_eq!(s.current_streak, 4, "today not done yet keeps the streak");
        assert_eq!(s.best_streak, 4);
        let s = h.stats(&c, d("2026-11-01"), d("2026-11-10"));
        assert_eq!(s.current_streak, 0, "a missed yesterday breaks it");
        assert_eq!(s.total, 4);
    }

    #[test]
    fn fixed_days_only_count_scheduled_days() {
        let mwf = Weekdays::single(Weekday::Monday)
            .with(Weekday::Wednesday)
            .with(Weekday::Friday);
        let h = Habit::new("Gym", Schedule::Days(mwf), now()).expect("habit");
        // Mon 2 / Wed 4 / Fri 6 / Mon 9 Nov 2026; Tue, Thu, weekend ignored.
        let c = counts(&["2026-11-02", "2026-11-04", "2026-11-06", "2026-11-09"]);
        let s = h.stats(&c, d("2026-11-02"), d("2026-11-10"));
        assert_eq!(s.current_streak, 4);
        assert_eq!(s.rate_30, Some(1.0));
    }

    #[test]
    fn weekly_goals_count_weeks() {
        let h = Habit::new("Run", Schedule::PerWeek(3), now()).expect("habit");
        // Week of Oct 26: 3 runs; week of Nov 2: 3 runs; this week (Nov 9): 1 so far.
        let c = counts(&[
            "2026-10-26",
            "2026-10-28",
            "2026-10-30",
            "2026-11-02",
            "2026-11-03",
            "2026-11-05",
            "2026-11-09",
        ]);
        let s = h.stats(&c, d("2026-10-26"), d("2026-11-10"));
        assert_eq!(s.streak_unit, StreakUnit::Weeks);
        assert_eq!(s.current_streak, 2, "this week isn't lost yet");
        assert_eq!(s.streak_label(s.current_streak), "2 weeks");
    }

    #[test]
    fn strength_grows_with_consistency_and_shrugs_off_one_miss() {
        let h = Habit::new("Meditate", Schedule::Daily, now()).expect("habit");
        let today = d("2027-02-16");
        let month = counts_of(&range("2026-11-09", "2026-12-08"));
        let s30 = h.stats(&month, d("2026-11-09"), d("2026-12-08"));
        assert!(s30.strength > 0.75, "30 perfect days: {}", s30.strength);

        let hundred = counts_of(&range("2026-11-08", "2027-02-15"));
        let s100 = h.stats(&hundred, d("2026-11-08"), today);
        assert!(s100.strength > 0.99, "100 days: {}", s100.strength);

        let mut one_miss = hundred.clone();
        one_miss.remove(&d("2027-02-10"));
        let dented = h.stats(&one_miss, d("2026-11-08"), today);
        assert!(
            s100.strength - dented.strength < 0.06,
            "one miss barely dents it"
        );

        let mut ten_missed = hundred;
        for day in range("2027-02-06", "2027-02-15") {
            ten_missed.remove(&d(&day));
        }
        let weak = h.stats(&ten_missed, d("2026-11-08"), today);
        assert!(weak.strength < 0.65, "ten misses hurt: {}", weak.strength);
        assert!((0.0..=1.0).contains(&weak.strength));
        let empty = h.stats(&HashMap::new(), d("2026-11-08"), today);
        assert!(empty.strength.abs() < f64::EPSILON);
    }

    #[test]
    fn quick_add_reads_schedules_and_goals() {
        let p = parse_habit("Read 20 pages daily");
        assert_eq!(
            (p.name.as_str(), p.schedule, p.target),
            ("Read 20 pages", Schedule::Daily, 1)
        );
        let p = parse_habit("Gym mon wed fri");
        assert_eq!(p.name, "Gym");
        assert_eq!(p.schedule.to_string(), "Mon, Wed, Fri");
        let p = parse_habit("Gym on tue, thu");
        assert_eq!(
            (p.name.as_str(), p.schedule.to_string().as_str()),
            ("Gym", "Tue, Thu")
        );
        assert_eq!(parse_habit("Run 3x a week").schedule, Schedule::PerWeek(3));
        assert_eq!(parse_habit("Run 3 times a week").name, "Run");
        assert_eq!(
            parse_habit("Swim twice a week").schedule,
            Schedule::PerWeek(2)
        );
        assert_eq!(
            parse_habit("Call home once a week").schedule,
            Schedule::PerWeek(1)
        );
        assert_eq!(parse_habit("Yoga 4/week").schedule, Schedule::PerWeek(4));
        let p = parse_habit("Drink water 8 times a day");
        assert_eq!(
            (p.name.as_str(), p.target, p.schedule),
            ("Drink water", 8, Schedule::Daily)
        );
        let p = parse_habit("Water 8x daily");
        assert_eq!((p.name.as_str(), p.target), ("Water", 8));
        assert_eq!(
            parse_habit("Stretch weekdays").schedule,
            Schedule::Days(Weekdays::WORKDAYS)
        );
        assert_eq!(parse_habit("Floss").schedule, Schedule::Daily);
        assert_eq!(
            parse_habit("daily").name,
            "daily",
            "a schedule alone is a name"
        );
        assert_eq!(parse_habit("Plan the week").name, "Plan the week");
    }
}
