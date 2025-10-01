//! Validation utilities for user-provided arguments (CLI-friendly).
//!
//! The parsers here return `Result<_, String>` so they plug directly into
//! `clap`'s `value_parser` attribute. The error strings are short, user-facing
//! messages suitable for terminal output.

use anyhow::Result;
use time::{Date, format_description::FormatItem, macros::format_description};

// A shared, zero-allocation format description for strict `YYYY-MM-DD`.
const DATE_FMT: &[FormatItem<'_>] = format_description!("[year]-[month]-[day]");

/// Strictly parse a calendar date in `YYYY-MM-DD` format.
///
/// ## Behavior
/// - Trims surrounding whitespace.
/// - Requires **zero-padded** year-month-day (e.g., `2025-09-02`).
/// - Rejects datetime strings (e.g., `2025-09-02 12:34:56`) and other formats.
/// - Validates real calendar dates (e.g., leap years).
///
/// ## Arguments
/// - `s`: The input string (typically from CLI).
///
/// ## Returns
/// - `Ok(Date)` on success.
/// - `Err(String)` with a short, user-friendly message otherwise (good for CLI).
///
/// ## Examples
/// ```
/// use linkleaf_core::validation::parse_date;
/// let d = parse_date("2025-01-03").unwrap();
/// assert_eq!(d.to_string(), "2025-01-03");
/// ```
///
/// ```
/// use linkleaf_core::validation::parse_date;
/// assert!(parse_date("2025/01/03").is_err());
/// assert!(parse_date("2025-1-3").is_err()); // not zero-padded
/// ```
pub fn parse_date(s: &str) -> Result<Date, String> {
    // Accept strictly "YYYY-MM-DD"
    Date::parse(s.trim(), DATE_FMT).map_err(|e| e.to_string())
}

/// Parse a comma-separated tag list into a vector of tags.
///
/// ## Behavior
/// - Splits on commas (`,`).
/// - Trims whitespace around each tag.
/// - Drops empty entries (e.g., consecutive commas or trailing commas).
/// - **Preserves** original case and **preserves order**; no de-duplication.
///   (Use a normalization step elsewhere if you need lowercase/unique tags.)
///
/// ## Arguments
/// - `raw`: A string like `"rust, async , tokio"`.
///
/// ## Returns
/// - `Ok(Vec<String>)` with the parsed tags (possibly empty).
/// - `Err(String)` is not used currently; the function is effectively infallible,
///   but the `Result` type makes it convenient to use with `clap`.
///
/// ## Examples
/// ```
/// use linkleaf_core::validation::parse_tags;
/// assert_eq!(parse_tags(" a, b ,  ,c ").unwrap(), vec!["a","b","c"]);
/// assert!(parse_tags(" , , ").unwrap().is_empty());
/// ```
pub fn parse_tags(raw: &str) -> Result<Vec<String>, String> {
    let tags = raw
        .split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Date;

    // ---------- parse_date ----------

    #[test]
    fn parse_date_accepts_strict_iso() {
        let d = parse_date("2025-09-02").expect("valid date");
        assert_eq!(
            d,
            Date::from_calendar_date(2025, time::Month::September, 2).unwrap()
        );
    }

    #[test]
    fn parse_date_trims_whitespace() {
        let d = parse_date("  2024-02-29 \t").expect("valid leap day with whitespace");
        assert_eq!(
            d,
            Date::from_calendar_date(2024, time::Month::February, 29).unwrap()
        );
    }

    #[test]
    fn parse_date_rejects_datetime() {
        // Must be exactly YYYY-MM-DD; datetime strings should fail.
        assert!(parse_date("2025-09-02 12:34:56").is_err());
    }

    #[test]
    fn parse_date_rejects_wrong_separator_or_format() {
        assert!(parse_date("2025/09/02").is_err());
        assert!(parse_date("02-09-2025").is_err());
        assert!(parse_date("2025-9-2").is_err()); // no zero-padding → should fail
    }

    #[test]
    fn parse_date_rejects_invalid_calendar_dates() {
        assert!(parse_date("2025-02-30").is_err());
        assert!(parse_date("2023-02-29").is_err()); // not a leap year
        assert!(parse_date("2025-13-01").is_err());
        assert!(parse_date("2025-00-10").is_err());
        assert!(parse_date("2025-01-00").is_err());
    }

    // ---------- parse_tags ----------

    #[test]
    fn parse_tags_empty_string_yields_empty_vec() {
        let tags = parse_tags("").expect("ok");
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_tags_trims_and_skips_empties() {
        let tags = parse_tags(" a, b ,  ,c , , ").expect("ok");
        assert_eq!(tags, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_tags_single_value() {
        let tags = parse_tags("rust").expect("ok");
        assert_eq!(tags, vec!["rust"]);
    }

    #[test]
    fn parse_tags_handles_tabs_and_newlines() {
        let tags = parse_tags("\trust,\n async ,tokio\t").expect("ok");
        assert_eq!(tags, vec!["rust", "async", "tokio"]);
    }

    #[test]
    fn parse_tags_keeps_case_and_order() {
        let tags = parse_tags("Rust,Async,Tokio").expect("ok");
        assert_eq!(tags, vec!["Rust", "Async", "Tokio"]);
    }

    #[test]
    fn parse_tags_all_commas_or_spaces_is_empty() {
        let tags = parse_tags(" , ,  , ").expect("ok");
        assert!(tags.is_empty());
    }
}
