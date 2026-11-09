//! Analytics across habits: completion rates by day, period and weekday.
//!
//! Rates count what was *expected*: a fixed-day habit is expected on its
//! scheduled days, an "N times a week" habit `n/7` per day (a done day
//! counts as one until the week's goal is met). Days before a habit's first day don't
//! count. Today counts only once it's done, so an unfinished morning never
//! drags the numbers down.

use std::collections::HashMap;
use std::ops::Range;

use jiff::Span;
use jiff::civil::Date;

use crate::habit::{Habit, Schedule};

/// One habit's history, as the insights need it.
#[derive(Debug, Clone, Copy)]
pub struct HabitHistory<'a> {
    pub habit: &'a Habit,
    pub counts: &'a HashMap<Date, u16>,
    pub first: Date,
}

impl HabitHistory<'_> {
    fn done(&self, day: Date) -> bool {
        self.counts
            .get(&day)
            .is_some_and(|c| *c >= self.habit.target())
    }

    /// (expected, done) for one day.
    fn on(&self, day: Date, today: Date) -> (f64, f64) {
        if day < self.first || day > today {
            return (0., 0.);
        }
        let done = self.done(day);
        if day == today && !done {
            return (0., 0.);
        }
        match self.habit.schedule() {
            Schedule::PerWeek(n) => {
                // Credit a done day only while the week's goal isn't met yet.
                let offset = i64::from(day.weekday().to_monday_zero_offset());
                let before = (1..=offset)
                    .filter(|k| {
                        let d = add_days(day, -k);
                        d >= self.first && self.done(d)
                    })
                    .count();
                let credit = done && before < usize::from(n);
                (f64::from(n) / 7., if credit { 1. } else { 0. })
            }
            schedule if schedule.is_due(day) => (1., if done { 1. } else { 0. }),
            _ => (0., 0.),
        }
    }
}

fn add_days(day: Date, n: i64) -> Date {
    day.checked_add(Span::new().days(n)).unwrap_or(day)
}

fn ratio(expected: f64, done: f64) -> Option<f64> {
    (expected > 1e-9).then(|| (done / expected).clamp(0., 1.))
}

/// Share of expected check-ins done on `day`, or `None` if nothing was expected.
#[must_use]
pub fn day_rate(habits: &[HabitHistory<'_>], day: Date, today: Date) -> Option<f64> {
    let (e, d) = habits.iter().fold((0., 0.), |(e, d), h| {
        let (he, hd) = h.on(day, today);
        (e + he, d + hd)
    });
    ratio(e, d)
}

/// Share of expected check-ins done over `days` (end exclusive).
#[must_use]
pub fn period_rate(habits: &[HabitHistory<'_>], days: Range<Date>, today: Date) -> Option<f64> {
    let (mut e, mut d) = (0., 0.);
    let mut day = days.start;
    while day < days.end {
        for h in habits {
            let (he, hd) = h.on(day, today);
            e += he;
            d += hd;
        }
        day = add_days(day, 1);
    }
    ratio(e, d)
}

/// Rates per weekday (Monday first) over the last `weeks` weeks.
#[must_use]
pub fn weekday_rates(habits: &[HabitHistory<'_>], today: Date, weeks: i64) -> [Option<f64>; 7] {
    let mut sums = [(0.0_f64, 0.0_f64); 7];
    for k in 0..weeks * 7 {
        let day = add_days(today, -k);
        let i = usize::from(day.weekday().to_monday_zero_offset().unsigned_abs());
        for h in habits {
            let (e, d) = h.on(day, today);
            sums[i].0 += e;
            sums[i].1 += d;
        }
    }
    sums.map(|(e, d)| ratio(e, d))
}

/// A one-line takeaway for the weekday chart, or `None` without enough data.
#[must_use]
pub fn weekday_takeaway(rates: &[Option<f64>; 7]) -> Option<String> {
    let names = [
        "Mondays",
        "Tuesdays",
        "Wednesdays",
        "Thursdays",
        "Fridays",
        "Saturdays",
        "Sundays",
    ];
    let known: Vec<(usize, f64)> = rates
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.map(|r| (i, r)))
        .collect();
    if known.len() < 3 {
        return None;
    }
    let best = known.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let worst = known.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1))?;
    let pct = |r: f64| (r * 100.).round() as i64;
    if pct(best.1) - pct(worst.1) < 10 {
        return Some("You're about as consistent every day of the week.".into());
    }
    Some(format!(
        "Strongest on {} ({}%), weakest on {} ({}%).",
        names[best.0],
        pct(best.1),
        names[worst.0],
        pct(worst.1)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurrence::Weekdays;
    use jiff::Timestamp;
    use jiff::civil::Weekday;

    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }
    fn now() -> Timestamp {
        Timestamp::from_second(1).expect("ts")
    }

    #[test]
    fn rates_count_only_what_was_expected() {
        let daily = Habit::new("Read", Schedule::Daily, now()).expect("habit");
        let mon = Habit::new(
            "Gym",
            Schedule::Days(Weekdays::single(Weekday::Monday)),
            now(),
        )
        .expect("habit");
        let read: HashMap<Date, u16> = [(d("2026-11-02"), 1), (d("2026-11-03"), 1)].into();
        let gym: HashMap<Date, u16> = HashMap::new();
        let hs = [
            HabitHistory {
                habit: &daily,
                counts: &read,
                first: d("2026-11-02"),
            },
            HabitHistory {
                habit: &mon,
                counts: &gym,
                first: d("2026-11-02"),
            },
        ];
        let today = d("2026-11-09"); // Monday
        // Mon Nov 2: both expected, one done.
        assert_eq!(day_rate(&hs, d("2026-11-02"), today), Some(0.5));
        // Tue Nov 3: only Read expected, done.
        assert_eq!(day_rate(&hs, d("2026-11-03"), today), Some(1.0));
        // Today, nothing done yet: not counted.
        assert_eq!(day_rate(&hs, today, today), None);
        // Before the first day: nothing expected.
        assert_eq!(day_rate(&hs, d("2026-11-01"), today), None);
        // Week Nov 2–8: 7 + 1 expected, 2 done.
        let week = period_rate(&hs, d("2026-11-02")..d("2026-11-09"), today).expect("rate");
        assert!((week - 2. / 8.).abs() < 1e-9);
    }

    #[test]
    fn weekly_goals_spread_over_the_week() {
        let run = Habit::new("Run", Schedule::PerWeek(2), now()).expect("habit");
        let c: HashMap<Date, u16> = [(d("2026-11-03"), 1), (d("2026-11-05"), 1)].into();
        let hs = [HabitHistory {
            habit: &run,
            counts: &c,
            first: d("2026-11-02"),
        }];
        let r = period_rate(&hs, d("2026-11-02")..d("2026-11-09"), d("2026-11-09")).expect("rate");
        assert!((r - 1.0).abs() < 1e-9, "2 of 2 a week is 100%: {r}");
    }

    #[test]
    fn weekday_takeaways() {
        let mut rates = [
            Some(0.9),
            Some(0.5),
            Some(0.8),
            None,
            Some(0.85),
            None,
            None,
        ];
        assert_eq!(
            weekday_takeaway(&rates).as_deref(),
            Some("Strongest on Mondays (90%), weakest on Tuesdays (50%).")
        );
        rates[1] = Some(0.88);
        rates[2] = Some(0.86);
        assert_eq!(
            weekday_takeaway(&rates).as_deref(),
            Some("You're about as consistent every day of the week.")
        );
        assert_eq!(weekday_takeaway(&[None; 7]), None);
        let daily = Habit::new("Read", Schedule::Daily, now()).expect("habit");
        let all: HashMap<Date, u16> = (0..14)
            .map(|k| (add_days(d("2026-11-08"), -k), 1))
            .collect();
        let hs = [HabitHistory {
            habit: &daily,
            counts: &all,
            first: d("2026-10-26"),
        }];
        let w = weekday_rates(&hs, d("2026-11-08"), 2);
        assert!(w.iter().all(|r| *r == Some(1.0)));
    }
}
