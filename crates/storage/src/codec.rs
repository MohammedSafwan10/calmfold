//! Conversions between domain values and SQLite columns.

use calmfold_core::{Date, Timestamp};

use crate::{StorageError, StorageResult};

pub(crate) fn ts_to_sql(ts: Timestamp) -> i64 {
    ts.as_millisecond()
}

pub(crate) fn ts_from_sql(ms: i64, column: &'static str) -> StorageResult<Timestamp> {
    Timestamp::from_millisecond(ms).map_err(|_| StorageError::Corrupt(column))
}

pub(crate) fn opt_ts_from_sql(
    ms: Option<i64>,
    column: &'static str,
) -> StorageResult<Option<Timestamp>> {
    ms.map(|ms| ts_from_sql(ms, column)).transpose()
}

pub(crate) fn date_to_sql(date: Date) -> String {
    date.to_string()
}

pub(crate) fn date_from_sql(s: Option<String>) -> StorageResult<Option<Date>> {
    s.map(|s| s.parse().map_err(|_| StorageError::Corrupt("tasks.due")))
        .transpose()
}

pub(crate) fn uuid_from_sql(s: &str, column: &'static str) -> StorageResult<uuid::Uuid> {
    uuid::Uuid::parse_str(s).map_err(|_| StorageError::Corrupt(column))
}

/// Turns free-form user input into a safe FTS5 prefix query.
///
/// Each whitespace-separated word becomes a quoted phrase with a prefix
/// wildcard, so FTS5 operators typed by the user (`AND`, `NEAR`, `*`, `"`,
/// `:` ...) are treated as text instead of query syntax.
pub(crate) fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_quotes_every_term() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(
            fts_query("car notes").as_deref(),
            Some("\"car\"* \"notes\"*")
        );
        assert_eq!(
            fts_query("a\"b OR NEAR(").as_deref(),
            Some("\"a\"\"b\"* \"OR\"* \"NEAR(\"*")
        );
    }

    #[test]
    fn timestamps_round_trip_at_millisecond_precision() {
        let ts = Timestamp::from_millisecond(1_762_600_000_123).expect("ts");
        assert_eq!(ts_from_sql(ts_to_sql(ts), "x").expect("decode"), ts);
    }
}
