# 0007: Bundled holiday data

## Status

Accepted (2026-11-09)

## Context

The calendar should show public holidays and festivals (India first, other
countries on request), and when you travel, the holidays where you are.
Many holidays (Diwali, Holi, Eid) follow lunar calendars, so they can't be
calculated from simple rules. Calmfold is local-first and doesn't make
network requests.

## Decision

- Generate a table from the `holidays` Python package (MIT, maintained per
  country) with `scripts/gen-holidays.py`. The table covers about 245
  countries over 12 years and is stored at `crates/core/data/holidays.txt`
  (~530 KB, embedded with `include_str!`). The package licence is kept next
  to it (`holidays-LICENSE.txt`).
- Two kinds: public holidays (`P`) and observances (`O`: optional,
  unofficial and religious categories). Bank, school, government and
  half-day categories are left out as noise. Lunar dates announced later
  are marked estimated.
- "Where you are" maps the Mac's IANA time zone to a country through
  `zone.tab` (also in the table). It needs no location permission and sends
  nothing.
- Holidays are a read-only layer computed at render time. They are never
  stored as events, so they can't be edited by accident and they switch
  off cleanly.

## Consequences

- No network and no runtime dependency on Python.
- The table has to be regenerated about once a year to roll the window
  forward. `core::holidays::years()` reports the range, and Settings shows
  it.
- State and regional holidays are included as `S`/`H <code>-<sub>` rows
  (added 2026-11-09, python-holidays 0.108).
- Upkeep is automatic: a monthly GitHub Action regenerates the table from
  the latest release and opens a pull request; a unit test fails three
  years before the table runs out.
- Rejected:
  - Fetching a holiday feed at runtime, because it would add the app's first
    network dependency and a privacy question.
  - Hand-written rules, because they can't cover lunar holidays.
  - Importing holidays as ordinary events, because they would clutter the
    calendar and be hard to remove.
