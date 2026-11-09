//! Which holidays the calendar shows, and where the user is.
//!
//! The data is `calmfold_core::holidays` (offline). The settings live in
//! the appearance file: on/off, observances on/off, and the user's
//! countries. On first launch the country is guessed from the computer's
//! time zone (no location permission, nothing sent anywhere). jiff reads it
//! on every platform: `/etc/localtime` on macOS and Linux, the registry's
//! Windows zone (mapped to its IANA name) on Windows.
//!
//! "Where you are": when the time zone points to a country that isn't one
//! of the user's (travelling), its holidays show too, labelled with the
//! country, so a closed-shops day abroad isn't a surprise. macOS updates
//! the time zone itself when "Set time zone automatically" is on.

use std::collections::BTreeMap;
use std::ops::Range;

use calmfold_core::Date;
use calmfold_core::holidays::{self, Holiday, HolidayKind};
use calmfold_ui::appearance;
use gpui_kit::{App, Global};

/// Test override for the detected country (the test machine's zone varies).
#[derive(Default)]
struct DetectedOverride(Option<Option<&'static str>>);

impl Global for DetectedOverride {}

#[doc(hidden)]
pub fn set_detected_country_for_tests(code: Option<&'static str>, cx: &mut App) {
    cx.set_global(DetectedOverride(Some(code)));
}

/// The country of the computer's current time zone, if the table covers it.
#[must_use]
pub fn detected_country(cx: &App) -> Option<&'static str> {
    if let Some(DetectedOverride(Some(code))) = cx.try_global::<DetectedOverride>() {
        return *code;
    }
    let zone = jiff::tz::TimeZone::try_system().ok()?;
    holidays::country_for_zone(zone.iana_name()?)
}

/// First launch: pick the country of the time zone as "yours".
pub fn ensure_countries(cx: &mut App) {
    if appearance::settings(cx).holiday_countries.is_some() {
        return;
    }
    let home: Vec<String> = detected_country(cx)
        .map(|c| vec![c.to_owned()])
        .unwrap_or_default();
    appearance::update(cx, |s| s.holiday_countries = Some(home));
}

/// The user's countries (codes), as saved.
#[must_use]
pub fn chosen(cx: &App) -> Vec<&'static str> {
    appearance::settings(cx)
        .holiday_countries
        .iter()
        .flatten()
        .filter_map(|c| {
            holidays::countries()
                .iter()
                .find(|k| k.code.eq_ignore_ascii_case(c))
                .map(|k| k.code)
        })
        .collect()
}

/// The chosen regions (`IN-TN`) of chosen countries.
#[must_use]
pub fn chosen_regions(cx: &App) -> Vec<&'static str> {
    let countries = chosen(cx);
    countries
        .iter()
        .flat_map(|c| holidays::regions(c))
        .filter(|r| {
            appearance::settings(cx)
                .holiday_regions
                .iter()
                .any(|code| code.eq_ignore_ascii_case(r.code))
        })
        .map(|r| r.code)
        .collect()
}

/// The country you're in when it isn't one of yours (and that's wanted).
#[must_use]
pub fn away_country(cx: &App) -> Option<&'static str> {
    if !appearance::settings(cx).holidays_where_you_are {
        return None;
    }
    let here = detected_country(cx)?;
    (!chosen(cx).contains(&here)).then_some(here)
}

/// Holidays for a visible range, by day.
#[derive(Debug, Clone, Default)]
pub struct DayHolidays {
    by_day: BTreeMap<Date, Vec<Holiday>>,
    /// More than one country is shown: say which one each is from.
    labelled: bool,
}

impl DayHolidays {
    /// What to show for `days` under the current settings.
    #[must_use]
    pub fn load(days: Range<Date>, cx: &App) -> Self {
        let prefs = appearance::settings(cx);
        if !prefs.holidays {
            return Self::default();
        }
        let mut countries = chosen(cx);
        countries.extend(away_country(cx));
        let mut places = countries.clone();
        places.extend(chosen_regions(cx));
        let mut by_day: BTreeMap<Date, Vec<Holiday>> = BTreeMap::new();
        for h in holidays::between(&places, days, prefs.holiday_observances) {
            by_day.entry(h.date).or_default().push(h);
        }
        Self {
            by_day,
            labelled: countries.len() > 1,
        }
    }

    #[must_use]
    pub fn on(&self, day: Date) -> &[Holiday] {
        self.by_day.get(&day).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_day.is_empty()
    }

    /// Days with something, in order.
    pub fn days(&self) -> impl Iterator<Item = Date> + '_ {
        self.by_day.keys().copied()
    }

    /// "Diwali", with several countries shown "Thanksgiving Day · US", and
    /// a regional day names its region: "Pongal · Tamil Nadu".
    #[must_use]
    pub fn label(&self, h: &Holiday) -> String {
        if let Some(region) = h.region.and_then(holidays::region_name) {
            format!("{} · {region}", h.name)
        } else if self.labelled {
            format!("{} · {}", h.name, h.countries.join(", "))
        } else {
            h.name.to_owned()
        }
    }

    /// One short line for a day: the first holiday's short name, "+N" for
    /// the rest (full names are in the tooltip and the day panel).
    #[must_use]
    pub fn summary(&self, day: Date) -> Option<String> {
        let list = self.on(day);
        let first = holidays::short_name(list.first()?.name);
        Some(match list.len() {
            1 => first.to_owned(),
            n => format!("{first} +{}", n - 1),
        })
    }

    /// Full text for a tooltip or panel: one line per holiday.
    #[must_use]
    pub fn details(&self, day: Date) -> Vec<String> {
        self.on(day).iter().map(describe).collect()
    }
}

/// "Diwali · Public holiday in India", "Holi · Festival in India · Date may shift".
#[must_use]
pub fn describe(h: &Holiday) -> String {
    let kind = match h.kind {
        HolidayKind::Holiday => "Public holiday",
        HolidayKind::Observance => "Observance",
    };
    let names: Vec<&str> = h
        .countries
        .iter()
        .map(|c| holidays::country_name(c).unwrap_or(c))
        .collect();
    let place = match h.region.and_then(holidays::region_name) {
        Some(region) => format!("{region}, {}", names.join(", ")),
        None => names.join(", "),
    };
    let mut text = format!("{} · {kind} in {place}", h.name);
    if h.estimated {
        text.push_str(" · Date may shift by a day");
    }
    text
}
