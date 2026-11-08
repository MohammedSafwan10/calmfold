//! Pure calendar geometry: which days a view shows, how to step through
//! periods, how overlapping events share a day column, and how multi-day
//! bars stack. No GPUI here, so it's all unit-tested.

use std::ops::Range;

use jiff::Span;
use jiff::civil::{Date, DateTime, Time, Weekday};
use keepstone_ui::appearance::{ClockFormat, WeekStart};

/// Days shown by the agenda.
pub const AGENDA_DAYS: i64 = 30;
pub const MINUTES_PER_DAY: i32 = 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarMode {
    Month,
    Week,
    Day,
    Agenda,
}

impl CalendarMode {
    pub const OPTIONS: [(Self, &'static str); 4] = [
        (Self::Month, "Month"),
        (Self::Week, "Week"),
        (Self::Day, "Day"),
        (Self::Agenda, "Agenda"),
    ];
}

#[must_use]
pub fn first_weekday(start: WeekStart) -> Weekday {
    match start {
        WeekStart::Monday => Weekday::Monday,
        WeekStart::Sunday => Weekday::Sunday,
    }
}

fn add_days(day: Date, n: i64) -> Date {
    day.checked_add(Span::new().days(n)).unwrap_or(day)
}

/// The first day of the week containing `day`.
#[must_use]
pub fn week_start_of(day: Date, first: Weekday) -> Date {
    let back = i64::from(day.weekday().since(first));
    add_days(day, -back)
}

/// The days a view shows around `anchor` (end exclusive). A month shows
/// whole weeks: 4 to 6 rows.
#[must_use]
pub fn visible_days(mode: CalendarMode, anchor: Date, first: Weekday) -> Range<Date> {
    match mode {
        CalendarMode::Month => {
            let start = week_start_of(anchor.first_of_month(), first);
            let end = week_start_of(add_days(anchor.last_of_month(), 7), first);
            start..end
        }
        CalendarMode::Week => {
            let start = week_start_of(anchor, first);
            start..add_days(start, 7)
        }
        CalendarMode::Day => anchor..add_days(anchor, 1),
        CalendarMode::Agenda => anchor..add_days(anchor, AGENDA_DAYS),
    }
}

/// `anchor` moved one period forward (`dir > 0`) or back.
#[must_use]
pub fn step(mode: CalendarMode, anchor: Date, dir: i64) -> Date {
    let span = match mode {
        CalendarMode::Month => Span::new().months(dir),
        CalendarMode::Week | CalendarMode::Agenda => Span::new().weeks(dir),
        CalendarMode::Day => Span::new().days(dir),
    };
    anchor.checked_add(span).unwrap_or(anchor)
}

/// The header title: "November 2026", "Nov 9 – 15, 2026", "Monday, Nov 9".
#[must_use]
pub fn title(mode: CalendarMode, anchor: Date, first: Weekday, today: Date) -> String {
    match mode {
        CalendarMode::Month => anchor.strftime("%B %Y").to_string(),
        CalendarMode::Week | CalendarMode::Agenda => {
            let days = visible_days(mode, anchor, first);
            let last = add_days(days.end, -1);
            range_label(days.start, last, today)
        }
        CalendarMode::Day if anchor.year() == today.year() => {
            anchor.strftime("%A, %B %-d").to_string()
        }
        CalendarMode::Day => anchor.strftime("%A, %B %-d, %Y").to_string(),
    }
}

/// "Nov 9 – 15", "Nov 30 – Dec 6", "Dec 28, 2026 – Jan 3, 2027"; the year
/// only when it isn't this year or the range crosses one.
#[must_use]
pub fn range_label(first: Date, last: Date, today: Date) -> String {
    let same_year = first.year() == last.year();
    let year = if same_year && first.year() == today.year() {
        String::new()
    } else {
        format!(", {}", last.year())
    };
    if !same_year {
        return format!(
            "{} – {}",
            first.strftime("%b %-d, %Y"),
            last.strftime("%b %-d, %Y")
        );
    }
    if first.month() == last.month() {
        format!(
            "{} – {}{year}",
            first.strftime("%b %-d"),
            last.strftime("%-d")
        )
    } else {
        format!(
            "{} – {}{year}",
            first.strftime("%b %-d"),
            last.strftime("%b %-d")
        )
    }
}

/// "9:30 AM" / "09:30". Whole hours drop the minutes in 12-hour form ("9 AM").
#[must_use]
pub fn format_time(t: Time, clock: ClockFormat) -> String {
    match clock {
        ClockFormat::TwentyFourHour => t.strftime("%H:%M").to_string(),
        ClockFormat::TwelveHour if t.minute() == 0 => t.strftime("%-I %p").to_string(),
        ClockFormat::TwelveHour => t.strftime("%-I:%M %p").to_string(),
    }
}

/// "9 – 10:30 AM", "11 AM – 1 PM", "09:00 – 10:30". Multi-day spans name the days.
#[must_use]
pub fn format_span(start: DateTime, end: DateTime, clock: ClockFormat) -> String {
    let (a, b) = (start.time(), end.time());
    if start.date() != end.date()
        && !(b == Time::midnight() && end.date() == add_days(start.date(), 1))
    {
        return format!(
            "{} {} – {} {}",
            start.date().strftime("%b %-d"),
            format_time(a, clock),
            end.date().strftime("%b %-d"),
            format_time(b, clock)
        );
    }
    if clock == ClockFormat::TwelveHour
        && (a.hour() < 12) == (b.hour() < 12)
        && b != Time::midnight()
    {
        // Same half of the day: say AM/PM once.
        let short = |t: Time| {
            if t.minute() == 0 {
                t.strftime("%-I").to_string()
            } else {
                t.strftime("%-I:%M").to_string()
            }
        };
        return format!("{} – {}", short(a), format_time(b, clock));
    }
    format!("{} – {}", format_time(a, clock), format_time(b, clock))
}

/// Minutes `start..end` of an occurrence clipped to `day` (an event that
/// started yesterday begins at 0; one ending tomorrow ends at 1440).
#[must_use]
pub fn minutes_on(day: Date, start: DateTime, end: DateTime) -> (i32, i32) {
    let minute = |t: Time| i32::from(t.hour()) * 60 + i32::from(t.minute());
    let from = if start.date() < day {
        0
    } else {
        minute(start.time())
    };
    let to = if end.date() > day {
        MINUTES_PER_DAY
    } else {
        minute(end.time())
    };
    (from, to.max(from))
}

/// Where a timed block sits in its day column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub lane: usize,
    /// Lanes in its overlap group: the block is `1 / lanes` wide.
    pub lanes: usize,
}

/// Side-by-side layout for overlapping blocks (Apple/Google style). `items`
/// are `(start, end)` minutes sorted by start; returns one `Placed` each.
/// Blocks shorter than `min_len` are treated as that long, since that is
/// how tall they're drawn.
#[must_use]
pub fn lay_out_timed(items: &[(i32, i32)], min_len: i32) -> Vec<Placed> {
    let mut out = vec![Placed { lane: 0, lanes: 1 }; items.len()];
    let mut lane_ends: Vec<i32> = Vec::new();
    let mut group: Vec<usize> = Vec::new();
    let mut group_end = i32::MIN;
    let close = |group: &mut Vec<usize>, lane_ends: &mut Vec<i32>, out: &mut Vec<Placed>| {
        let lanes = lane_ends.len().max(1);
        for &i in group.iter() {
            out[i].lanes = lanes;
        }
        group.clear();
        lane_ends.clear();
    };
    for (i, &(start, end)) in items.iter().enumerate() {
        let end = end.max(start + min_len);
        if start >= group_end && !group.is_empty() {
            close(&mut group, &mut lane_ends, &mut out);
        }
        let lane = match lane_ends.iter().position(|&e| e <= start) {
            Some(free) => {
                lane_ends[free] = end;
                free
            }
            None => {
                lane_ends.push(end);
                lane_ends.len() - 1
            }
        };
        out[i].lane = lane;
        group.push(i);
        group_end = if group.len() == 1 {
            end
        } else {
            group_end.max(end)
        };
    }
    close(&mut group, &mut lane_ends, &mut out);
    out
}

/// Stacks horizontal bars (inclusive column ranges, sorted by start) into
/// rows so none overlap; returns the row of each bar.
#[must_use]
pub fn pack_spans(spans: &[(usize, usize)]) -> Vec<usize> {
    let mut row_ends: Vec<usize> = Vec::new();
    spans
        .iter()
        .map(
            |&(first, last)| match row_ends.iter().position(|&e| e < first) {
                Some(row) => {
                    row_ends[row] = last;
                    row
                }
                None => {
                    row_ends.push(last);
                    row_ends.len() - 1
                }
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }
    fn dt(s: &str) -> DateTime {
        s.parse().expect("datetime")
    }
    const MON: Weekday = Weekday::Monday;

    #[test]
    fn months_show_whole_weeks() {
        // November 2026 starts on a Sunday.
        let days = visible_days(CalendarMode::Month, d("2026-11-18"), MON);
        assert_eq!(days, d("2026-10-26")..d("2026-12-07"), "6 rows");
        let sunday_first = visible_days(CalendarMode::Month, d("2026-11-18"), Weekday::Sunday);
        assert_eq!(sunday_first, d("2026-11-01")..d("2026-12-06"), "5 rows");
        // February 2027 fits in exactly 4 weeks with Monday first.
        let feb = visible_days(CalendarMode::Month, d("2027-02-10"), MON);
        assert_eq!(feb, d("2027-02-01")..d("2027-03-01"));
    }

    #[test]
    fn weeks_days_and_steps() {
        assert_eq!(
            visible_days(CalendarMode::Week, d("2026-11-08"), MON),
            d("2026-11-02")..d("2026-11-09"),
            "Sunday belongs to the week before"
        );
        assert_eq!(
            visible_days(CalendarMode::Day, d("2026-11-08"), MON),
            d("2026-11-08")..d("2026-11-09")
        );
        assert_eq!(
            step(CalendarMode::Month, d("2026-01-31"), 1),
            d("2026-02-28")
        );
        assert_eq!(
            step(CalendarMode::Week, d("2026-11-08"), -1),
            d("2026-11-01")
        );
        assert_eq!(step(CalendarMode::Day, d("2026-12-31"), 1), d("2027-01-01"));
    }

    #[test]
    fn titles() {
        let today = d("2026-11-08");
        assert_eq!(
            title(CalendarMode::Month, today, MON, today),
            "November 2026"
        );
        assert_eq!(
            title(CalendarMode::Week, d("2026-11-10"), MON, today),
            "Nov 9 – 15"
        );
        assert_eq!(
            title(CalendarMode::Week, d("2026-12-01"), MON, today),
            "Nov 30 – Dec 6"
        );
        assert_eq!(
            title(CalendarMode::Week, d("2026-12-30"), MON, today),
            "Dec 28, 2026 – Jan 3, 2027"
        );
        assert_eq!(
            title(CalendarMode::Day, d("2026-11-09"), MON, today),
            "Monday, November 9"
        );
        assert_eq!(
            title(CalendarMode::Day, d("2027-01-04"), MON, today),
            "Monday, January 4, 2027"
        );
    }

    #[test]
    fn time_formats() {
        let t = |s: &str| s.parse::<Time>().expect("time");
        assert_eq!(format_time(t("09:00"), ClockFormat::TwelveHour), "9 AM");
        assert_eq!(format_time(t("14:30"), ClockFormat::TwelveHour), "2:30 PM");
        assert_eq!(format_time(t("00:15"), ClockFormat::TwelveHour), "12:15 AM");
        assert_eq!(
            format_time(t("14:30"), ClockFormat::TwentyFourHour),
            "14:30"
        );
        let twelve = ClockFormat::TwelveHour;
        assert_eq!(
            format_span(dt("2026-11-09T09:00"), dt("2026-11-09T10:30"), twelve),
            "9 – 10:30 AM"
        );
        assert_eq!(
            format_span(dt("2026-11-09T11:00"), dt("2026-11-09T13:00"), twelve),
            "11 AM – 1 PM"
        );
        assert_eq!(
            format_span(dt("2026-11-09T22:00"), dt("2026-11-10T00:00"), twelve),
            "10 PM – 12 AM"
        );
        assert_eq!(
            format_span(dt("2026-11-09T22:00"), dt("2026-11-10T06:00"), twelve),
            "Nov 9 10 PM – Nov 10 6 AM"
        );
    }

    #[test]
    fn minutes_clip_to_the_day() {
        let day = d("2026-11-10");
        assert_eq!(
            minutes_on(day, dt("2026-11-10T09:30"), dt("2026-11-10T10:00")),
            (570, 600)
        );
        assert_eq!(
            minutes_on(day, dt("2026-11-09T22:00"), dt("2026-11-10T02:00")),
            (0, 120)
        );
        assert_eq!(
            minutes_on(day, dt("2026-11-10T22:00"), dt("2026-11-11T00:00")),
            (1320, 1440)
        );
    }

    #[test]
    fn overlapping_blocks_share_the_column() {
        // 9–10 and 9:30–11 overlap; 10–10:30 fits beside 9:30–11 in lane 0;
        // 12–13 is alone.
        let placed = lay_out_timed(&[(540, 600), (570, 660), (600, 630), (720, 780)], 20);
        assert_eq!(
            placed,
            [
                Placed { lane: 0, lanes: 2 },
                Placed { lane: 1, lanes: 2 },
                Placed { lane: 0, lanes: 2 },
                Placed { lane: 0, lanes: 1 },
            ]
        );
        // Short blocks reserve their drawn height.
        let placed = lay_out_timed(&[(540, 545), (550, 560)], 20);
        assert_eq!(placed[1], Placed { lane: 1, lanes: 2 });
    }

    #[test]
    fn bars_stack_without_overlap() {
        assert_eq!(pack_spans(&[(0, 2), (1, 1), (3, 6), (2, 4)]), [0, 1, 0, 1]);
        assert!(pack_spans(&[]).is_empty());
    }
}
