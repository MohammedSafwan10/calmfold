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
//!   off everywhere (Raksha Bandhan, Mother's Day, Halloween).
//!
//! Lunar holidays that are announced later carry `estimated`; their date can
//! move by a day.
//!
//! Regions (states, provinces) add their own days on top of the country's:
//! codes like `IN-TN` (Tamil Nadu). A region only lists what the country
//! doesn't already have, or days that are a day off only there.

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

/// A state or province (`IN-TN`), with its own extra holidays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub code: &'static str,
    pub name: &'static str,
}

impl Region {
    /// The country part (`IN` of `IN-TN`).
    #[must_use]
    pub fn country(&self) -> &'static str {
        self.code.split('-').next().unwrap_or(self.code)
    }
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
    /// Set when it comes from a region only (`IN-TN`), not the whole country.
    pub region: Option<&'static str>,
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
    regions: Vec<Region>,
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
    let mut regions = Vec::new();
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
            Some("S") => {
                if let (Some(code), Some(name)) = (f.next(), f.next()) {
                    regions.push(Region { code, name });
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
    regions.sort_by(|a, b| (a.country(), a.name).cmp(&(b.country(), b.name)));
    Table {
        countries,
        regions,
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

/// A name short enough for a calendar cell: drops a trailing note in
/// brackets and keeps the first of alternatives. "Pratihar Shashthi or Surya
/// Shashthi (Chhath Puja)" → "Pratihar Shashthi"; "Ganesh Chaturthi /
/// Vinayak Chaturthi" → "Ganesh Chaturthi". The full name stays in tooltips
/// and the day panel.
#[must_use]
pub fn short_name(name: &str) -> &str {
    let base = match name.find(" (") {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    [" / ", " or "]
        .iter()
        .filter_map(|sep| base.find(sep))
        .min()
        .map_or(base, |i| &base[..i])
        .trim()
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

/// The regions (states, provinces) of `country`, by name.
#[must_use]
pub fn regions(country: &str) -> Vec<Region> {
    table()
        .regions
        .iter()
        .filter(|r| r.country().eq_ignore_ascii_case(country))
        .copied()
        .collect()
}

/// A region's name ("Tamil Nadu"), if covered.
#[must_use]
pub fn region_name(code: &str) -> Option<&'static str> {
    table()
        .regions
        .iter()
        .find(|r| r.code.eq_ignore_ascii_case(code))
        .map(|r| r.name)
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

/// Holidays of `places` (countries like `IN`, regions like `IN-TN`) on
/// days in `days` (end exclusive), by date, public holidays first. The same
/// name on the same date in several places is one entry listing their
/// countries; a day off in a region upgrades the country's observance.
/// Observances only when asked.
#[must_use]
pub fn between(places: &[&str], days: Range<Date>, observances: bool) -> Vec<Holiday> {
    let t = table();
    let mut out: Vec<Holiday> = Vec::new();
    for place in places {
        let Some((&key, list)) = t.entries.get_key_value(*place) else {
            continue;
        };
        let (code, region) = match key.split_once('-') {
            Some((country, _)) => (
                t.countries
                    .iter()
                    .find(|c| c.code == country)
                    .map_or(key, |c| c.code),
                Some(key),
            ),
            None => (key, None),
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
                    if e.kind < h.kind {
                        h.region = region;
                    } else if region.is_none() {
                        h.region = None;
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
                    region,
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
        let march = between(&["IN"], d("2026-03-01")..d("2026-04-01"), false);
        assert!(
            march
                .iter()
                .any(|h| h.name == "Holi" && h.date == d("2026-03-04"))
        );
        // Raksha Bandhan is a restricted holiday in India: an observance.
        let aug = d("2026-08-01")..d("2026-09-01");
        let rakhi = between(&["IN"], aug.clone(), true);
        let rakhi = rakhi
            .iter()
            .find(|h| h.name == "Raksha Bandhan")
            .expect("rakhi");
        assert_eq!(rakhi.kind, HolidayKind::Observance);
        assert!(
            !between(&["IN"], aug, false)
                .iter()
                .any(|h| h.name == "Raksha Bandhan"),
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
    fn short_names_fit_a_cell() {
        assert_eq!(
            short_name("Pratihar Shashthi or Surya Shashthi (Chhath Puja)"),
            "Pratihar Shashthi"
        );
        assert_eq!(
            short_name("Ganesh Chaturthi / Vinayak Chaturthi"),
            "Ganesh Chaturthi"
        );
        assert_eq!(short_name("Meshadi (Tamil New Year's Day)"), "Meshadi");
        assert_eq!(short_name("Diwali"), "Diwali");
        assert_eq!(short_name("(odd)"), "(odd)");
    }

    #[test]
    fn regions_add_their_own_days() {
        let tn = regions("IN")
            .into_iter()
            .find(|r| r.name == "Tamil Nadu")
            .expect("tamil nadu");
        assert_eq!(tn.code, "IN-TN");
        assert_eq!(region_name("IN-TS"), Some("Telangana"));
        let jan = d("2026-01-14")..d("2026-01-17");
        let national = between(&["IN"], jan.clone(), false);
        assert!(!national.iter().any(|h| h.name == "Pongal"), "{national:?}");
        let with_state = between(&["IN", "IN-TN"], jan, false);
        let pongal = with_state
            .iter()
            .find(|h| h.name == "Pongal")
            .expect("pongal");
        assert_eq!(pongal.kind, HolidayKind::Holiday, "a day off in Tamil Nadu");
        assert_eq!(pongal.region, Some("IN-TN"));
        assert_eq!(pongal.countries, ["IN"]);
        assert!(regions("US").iter().any(|r| r.name == "California"));
    }

    #[test]
    fn the_table_reaches_far_enough_ahead() {
        // A reminder that fails `scripts/check.sh` three years before the
        // data runs out, so the table is regenerated in time.
        let (_, last) = years();
        let this_year = crate::clock::today().year();
        assert!(
            last >= this_year + 3,
            "Holiday data ends in {last}. Regenerate it: uv run scripts/gen-holidays.py"
        );
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
