//! Date formats: one catalogue, shared by detection, parsing and the UI.
//!
//! The import screen used to ask for a chrono strftime string, which is a
//! reasonable thing to ask a programmer and an unreasonable thing to ask
//! anybody else. The list below is the single source of truth: detection walks
//! it in order, the dropdown is built from it, and the parser accepts any entry
//! in it.
//!
//! Order matters. ISO comes first because it is unambiguous; day-first comes
//! before month-first because the app's centre of gravity is Europe, and where
//! a file cannot settle the question the detector says so rather than guessing
//! quietly.

use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DateFormat {
    /// chrono strftime pattern.
    pub pattern: &'static str,
    /// What it looks like, for the dropdown.
    pub example: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub has_time: bool,
}

const D: fn(&'static str, &'static str, &'static str, &'static str, bool) -> DateFormat =
    |pattern, example, label, group, has_time| DateFormat {
        pattern,
        example,
        label,
        group,
        has_time,
    };

/// Every format the importer understands.
pub fn catalogue() -> Vec<DateFormat> {
    vec![
        // --- unambiguous -----------------------------------------------------
        D("%Y-%m-%d", "2026-03-04", "Year-month-day", "ISO", false),
        D("%Y/%m/%d", "2026/03/04", "Year/month/day", "ISO", false),
        D(
            "%Y%m%d",
            "20260304",
            "Year month day, no separators",
            "ISO",
            false,
        ),
        // --- day first -------------------------------------------------------
        D("%d/%m/%Y", "04/03/2026", "Day/month/year", "Europe", false),
        D("%d-%m-%Y", "04-03-2026", "Day-month-year", "Europe", false),
        D("%d.%m.%Y", "04.03.2026", "Day.month.year", "Europe", false),
        D(
            "%d/%m/%y",
            "04/03/26",
            "Day/month/2-digit year",
            "Europe",
            false,
        ),
        D(
            "%d %b %Y",
            "04 Mar 2026",
            "Day month-name year",
            "Europe",
            false,
        ),
        D(
            "%d-%b-%Y",
            "04-Mar-2026",
            "Day-month-name-year",
            "Europe",
            false,
        ),
        // --- month first -----------------------------------------------------
        D(
            "%m/%d/%Y",
            "03/04/2026",
            "Month/day/year",
            "Americas",
            false,
        ),
        D(
            "%m-%d-%Y",
            "03-04-2026",
            "Month-day-year",
            "Americas",
            false,
        ),
        D(
            "%m/%d/%y",
            "03/04/26",
            "Month/day/2-digit year",
            "Americas",
            false,
        ),
        D(
            "%b %d, %Y",
            "Mar 04, 2026",
            "Month-name day, year",
            "Americas",
            false,
        ),
        // --- with a time ------------------------------------------------------
        // The time is read and discarded: a ledger is kept by booking date, and
        // keeping the clock would make two coffees on one day sort by seconds
        // while still being the same day's spending.
        D(
            "%Y-%m-%dT%H:%M:%S",
            "2026-03-04T14:30:00",
            "ISO with time",
            "With a time",
            true,
        ),
        D(
            "%Y-%m-%d %H:%M:%S",
            "2026-03-04 14:30:00",
            "Year-month-day + time",
            "With a time",
            true,
        ),
        D(
            "%Y-%m-%d %H:%M",
            "2026-03-04 14:30",
            "Year-month-day + hour:minute",
            "With a time",
            true,
        ),
        D(
            "%d/%m/%Y %H:%M:%S",
            "04/03/2026 14:30:00",
            "Day/month/year + time",
            "With a time",
            true,
        ),
        D(
            "%d/%m/%Y %H:%M",
            "04/03/2026 14:30",
            "Day/month/year + hour:minute",
            "With a time",
            true,
        ),
        D(
            "%m/%d/%Y %H:%M:%S",
            "03/04/2026 14:30:00",
            "Month/day/year + time",
            "With a time",
            true,
        ),
        D(
            "%m/%d/%Y %I:%M %p",
            "03/04/2026 02:30 PM",
            "Month/day/year + 12-hour",
            "With a time",
            true,
        ),
    ]
}

/// Just the patterns, in catalogue order — what detection walks.
pub fn patterns() -> Vec<&'static str> {
    catalogue().into_iter().map(|f| f.pattern).collect()
}

pub fn is_known(pattern: &str) -> bool {
    catalogue().iter().any(|f| f.pattern == pattern)
}

/// Parse exactly: the value must match the pattern, date or date-and-time.
///
/// Used by detection, which is trying to work out which format a file is in and
/// must not be lenient — a sloppy match would report the wrong format and then
/// mis-read the whole file.
pub fn parse_strict(value: &str, pattern: &str) -> Option<NaiveDate> {
    let v = value.trim();
    if let Ok(d) = NaiveDate::parse_from_str(v, pattern) {
        return Some(d);
    }
    NaiveDateTime::parse_from_str(v, pattern)
        .ok()
        .map(|dt| dt.date())
}

/// Parse forgivingly, for a format the user chose by hand.
///
/// Falls back to reading the date part of a value that carries a time the
/// chosen pattern does not mention — the commonest mismatch, and one where
/// refusing the row helps nobody.
pub fn parse_lenient(value: &str, pattern: &str) -> Option<NaiveDate> {
    if let Some(d) = parse_strict(value, pattern) {
        return Some(d);
    }
    let v = value.trim();
    // "2026-03-04T14:30:00" or "04/03/2026 14:30" against a date-only pattern.
    let head = v.split(['T', ' ']).next()?.trim();
    if head.len() < v.len() {
        return parse_strict(head, pattern);
    }
    None
}

/// The first catalogue format that parses this value exactly.
pub fn detect(value: &str) -> Option<&'static str> {
    let v = value.trim();
    // Six characters is the shortest real date ("040326" is not in the
    // catalogue, so anything shorter is noise).
    if v.len() < 6 {
        return None;
    }
    catalogue()
        .into_iter()
        .find(|f| parse_strict(v, f.pattern).is_some())
        .map(|f| f.pattern)
}

#[cfg(test)]
mod tests;
