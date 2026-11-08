//! The one place that reads the system clock.
//!
//! Domain methods take `now` as a parameter. UI code calls [`now`] and passes
//! the value in. Storage keeps millisecond precision, so `now` truncates to
//! milliseconds: a value read back from the database then equals the value
//! that was written.

use jiff::Timestamp;

#[must_use]
pub fn now() -> Timestamp {
    let ms = Timestamp::now().as_millisecond();
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Today's date in the system time zone.
#[must_use]
pub fn today() -> jiff::civil::Date {
    jiff::Zoned::now().date()
}

/// The local wall-clock time, to the minute (for calendars).
#[must_use]
pub fn now_local() -> jiff::civil::DateTime {
    let now = jiff::Zoned::now().datetime();
    now.date().to_datetime(
        jiff::civil::Time::new(now.hour(), now.minute(), 0, 0).unwrap_or(jiff::civil::Time::MIN),
    )
}
