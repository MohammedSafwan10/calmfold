//! Public holidays and notable days for ~245 countries, offline.
//!
//! The table (`data/holidays.txt`) is generated from the `holidays` Python
//! package by `scripts/gen-holidays.py`; nothing is hard-coded here and
//! nothing is fetched at runtime. It covers about a decade; regenerate it
//! yearly to roll the window forward.
//!
//! Two kinds, so the calendar can stay calm:
//! * [`HolidayKind::Holiday`]: a public holiday, a day off (Diwali in India,
//!   Thanksgiving in the US).
//! * [`HolidayKind::Observance`]: a festival or notable day that isn't a day
//!   off everywhere (Holi, Raksha Bandhan, Mother's Day, Halloween).
//!
//! Lunar holidays that are announced later carry `estimated`; their date can
//! move by a day.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

use jiff::civil::Date;

const DATA: &str = include_str!("../data/holidays.txt");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HolidayKind {
    Holiday,
    Observance,
}

/// A country (ISO 3166 alpha-2 code) the table covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Country {
    pub code: &'static str,
    pub name: &'static str,
}

/// One holiday on one date, merged across the countries asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    pub date: Date,
    pub name: &'static str,
    pub kind: HolidayKind,
    /// Its date may still move by a day (lunar, announced later).
    pub estimated: bool,
    /// Codes of the countries that observe it, in the order asked for.
    pub countries: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    date: Date,
    name: &'static str,
    kind: HolidayKind,
    estimated: bool,
}

struct Table {
    countries: Vec<Country>,
    zones: HashMap<&'static str, &'static str>,
    /// Per country, sorted by date.
    entries: HashMap<&'static str, Vec<Entry>>,
    years: (i16, i16),
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| parse(DATA))
}

fn parse(data: &'static str) -> Table {
    let mut countries = Vec::new();
    let mut zones = HashMap::new();
    let mut entries: HashMap<&'static str, Vec<Entry>> = HashMap::new();
    let (mut first, mut last) = (i16::MAX, i16::MIN);
    for line in data.lines() {
        let mut f = line.split('\t');
        match f.next() {
            Some("C") => {
                if let (Some(code), Some(name)) = (f.next(), f.next()) {
                    countries.push(Country { code, name });
                }
            }
            Some("Z") => {
                if let (Some(zone), Some(code)) = (f.next(), f.next()) {
                    zones.insert(zone, code);
                }
            }
            Some("H") => {
                let (Some(code), Some(kind), Some(name), Some(dates)) =
                    (f.next(), f.next(), f.next(), f.next())
                else {
                    continue;
                };
                let kind = if kind == "P" {
                    HolidayKind::Holiday
                } else {
                    HolidayKind::Observance
                };
                let list = entries.entry(code).or_default();
                for d in dates.split(',') {
                    let (d, estimated) = match d.strip_suffix('~') {
                        Some(d) => (d, true),
                        None => (d, false),
                    };
                    let Some(date) = parse_date(d) else { continue };
                    first = first.min(date.year());
                    last = last.max(date.year());
                    list.push(Entry {
                        date,
                        name,
                        kind,
                        estimated,
                    });
                }
            }
            _ => {}
        }
    }
    for list in entries.values_mut() {
        list.sort_by_key(|e| (e.date, e.kind));
    }
    countries.sort_by(|a, b| a.name.cmp(b.name));
    Table {
        countries,
        zones,
        entries,
        years: (first, last),
    }
}

fn parse_date(s: &str) -> Option<Date> {
    if s.len() != 8 {
        return None;
    }
    Date::new(
        s[0..4].parse().ok()?,
        s[4..6].parse().ok()?,
        s[6..8].parse().ok()?,
    )
    .ok()
}

/// Every covered country, by name.
#[must_use]
pub fn countries() -> &'static [Country] {
    &table().countries
}

/// The country's name ("India"), if covered.
#[must_use]
pub fn country_name(code: &str) -> Option<&'static str> {
    table()
        .countries
        .iter()
        .find(|c| c.code.eq_ignore_ascii_case(code))
        .map(|c| c.name)
}

/// The covered country of an IANA time zone ("Asia/Kolkata" → "IN"). Used
/// to guess where the user is without asking for their location.
#[must_use]
pub fn country_for_zone(zone: &str) -> Option<&'static str> {
    table().zones.get(zone).copied()
}

/// First and last year the table covers.
#[must_use]
pub fn years() -> (i16, i16) {
    table().years
}

/// Holidays of `countries` on days in `days` (end exclusive), by date,
/// public holidays first. The same name on the same date in several
/// countries is one entry listing them all. Observances only when asked.
#[must_use]
pub fn between(countries: &[&str], days: Range<Date>, observances: bool) -> Vec<Holiday> {
    let t = table();
    let mut out: Vec<Holiday> = Vec::new();
    for code in countries {
        let Some((&code, list)) = t.entries.get_key_value(*code) else {
            continue;
        };
        let start = list.partition_point(|e| e.date < days.start);
        for e in list[start..].iter().take_while(|e| e.date < days.end) {
            if e.kind == HolidayKind::Observance && !observances {
                continue;
            }
            match out
                .iter_mut()
                .find(|h| h.date == e.date && h.name == e.name)
            {
                Some(h) => {
                    if !h.countries.contains(&code) {
                        h.countries.push(code);
                    }
                    h.kind = h.kind.min(e.kind);
                    h.estimated &= e.estimated;
                }
                None => out.push(Holiday {
                    date: e.date,
                    name: e.name,
                    kind: e.kind,
                    estimated: e.estimated,
                    countries: vec![code],
                }),
            }
        }
    }
    out.sort_by_key(|a| (a.date, a.kind));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        s.parse().expect("date")
    }

    #[test]
    fn covers_many_countries_and_a_decade() {
        assert!(countries().len() > 150);
        assert_eq!(country_name("IN"), Some("India"));
        assert_eq!(country_name("us"), Some("United States"));
        let (first, last) = years();
        assert!(last - first >= 9, "{first}-{last}");
    }

    #[test]
    fn finds_india_holidays_including_lunar_ones() {
        let nov = between(&["IN"], d("2026-11-01")..d("2026-12-01"), false);
        let names: Vec<_> = nov.iter().map(|h| (h.date, h.name)).collect();
        assert!(names.contains(&(d("2026-11-08"), "Diwali")), "{names:?}");
        assert!(nov.iter().all(|h| h.kind == HolidayKind::Holiday));
        // Holi is a restricted holiday in India: an observance.
        let march = between(&["IN"], d("2026-03-01")..d("2026-04-01"), true);
        let holi = march.iter().find(|h| h.name == "Holi").expect("holi");
        assert_eq!(holi.kind, HolidayKind::Observance);
        assert!(
            !between(&["IN"], d("2026-03-01")..d("2026-04-01"), false)
                .iter()
                .any(|h| h.name == "Holi"),
            "observances only when asked"
        );
    }

    #[test]
    fn merges_shared_days_and_keeps_order() {
        let dec = between(&["IN", "US"], d("2026-12-24")..d("2026-12-27"), false);
        let christmas: Vec<_> = dec
            .iter()
            .filter(|h| h.date == d("2026-12-25"))
            .map(|h| (h.name, h.countries.clone()))
            .collect();
        // India says "Christmas", the US "Christmas Day": both stay.
        assert!(christmas.iter().any(|(_, c)| c == &["IN"]));
        assert!(christmas.iter().any(|(_, c)| c == &["US"]));
        let all = between(&["US", "IN"], d("2026-01-01")..d("2027-01-01"), true);
        assert!(all.windows(2).all(|w| w[0].date <= w[1].date));
        assert!(between(&["XX"], d("2026-01-01")..d("2027-01-01"), true).is_empty());
    }

    #[test]
    fn lunar_dates_ahead_are_marked_estimated() {
        let eid = between(&["IN"], d("2027-01-01")..d("2028-01-01"), false);
        assert!(
            eid.iter()
                .any(|h| h.name.starts_with("Eid al-Fitr") && h.estimated)
        );
        assert!(eid.iter().any(|h| h.name == "Republic Day" && !h.estimated));
    }

    #[test]
    fn time_zones_map_to_countries() {
        assert_eq!(country_for_zone("Asia/Kolkata"), Some("IN"));
        assert_eq!(country_for_zone("Asia/Calcutta"), Some("IN"));
        assert_eq!(country_for_zone("America/New_York"), Some("US"));
        assert_eq!(country_for_zone("Europe/Berlin"), Some("DE"));
        assert_eq!(country_for_zone("UTC"), None);
    }
}
