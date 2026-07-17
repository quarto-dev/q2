/*
 * dates.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Shared date parsing and formatting (bd-gx9cic8z P4, bd-13f821l5).
 */

//! Date parsing and formatting for document metadata and listings.
//!
//! The Rust counterpart of Quarto 1's `src/core/date.ts`, per the
//! approved design in
//! `claude-notes/plans/2026-07-17-date-formatting-design.md`. The
//! user-facing contract mirrors Q1's
//! `quarto-web/docs/reference/dates.qmd`: a fixed list of accepted
//! input forms, the `today`/`now`/`last-modified` keywords (resolved
//! by the calling transform, not here), the named styles
//! `full | long | medium | short | iso`, and day.js-style token
//! format strings with `[...]` literal escapes.
//!
//! Documented deviations from Q1 (design doc table):
//! - No guessing-parser tail: unparseable input returns `None` and
//!   the caller reports a diagnostic naming the accepted forms.
//! - Named styles and month/day names are English-only for now;
//!   locale support joins the deferred localization design (epic
//!   decision Q3).
//! - Locale-week tokens (`w ww wo gggg`) and named-timezone tokens
//!   (`z zzz`) are deferred: they format literally and surface a
//!   warning. ISO-week tokens (`W WW GGGG`) are supported — `time`
//!   computes ISO weeks without locale data.
//! - Characters day.js does not treat as tokens (e.g. the `T` in
//!   `YYYY-MM-DDTHH:mm:ssZ`) pass through literally with no warning,
//!   matching day.js; only *known-but-deferred* tokens warn.

use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

/// A parsed date, possibly carrying a time-of-day and a UTC offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDate {
    /// The civil date-time (midnight when the input had no time).
    pub datetime: PrimitiveDateTime,
    /// The UTC offset, when the input carried one.
    pub offset: Option<UtcOffset>,
    /// Whether the input included a time-of-day component.
    pub has_time: bool,
}

impl ParsedDate {
    /// The ISO form for machine slots (`date-meta`, feeds):
    /// `YYYY-MM-DD` for plain dates, RFC-3339-style timestamp when a
    /// time component was present.
    pub fn iso_string(&self) -> String {
        if self.has_time {
            let (out, warnings) = format_tokens(self, "YYYY-MM-DD[T]HH:mm:ssZ");
            debug_assert!(warnings.is_empty());
            out
        } else {
            let (out, warnings) = format_tokens(self, "YYYY-MM-DD");
            debug_assert!(warnings.is_empty());
            out
        }
    }

    /// The datetime as an [`OffsetDateTime`], assuming UTC when the
    /// input carried no offset (used for unix-timestamp tokens).
    fn to_offset_datetime(&self) -> OffsetDateTime {
        self.datetime
            .assume_offset(self.offset.unwrap_or(UtcOffset::UTC))
    }
}

/// The accepted input forms, in Q1's order (`parsePandocDate`), for
/// diagnostics. ISO timestamps (with or without offset) are also
/// accepted.
pub const ACCEPTED_DATE_FORMS: &str = "MM/DD/YYYY, MM-DD-YYYY, MM/DD/YY, MM-DD-YY, YYYY-MM-DD, DD MM YYYY, \
     \"MM DD, YYYY\", or an ISO timestamp (YYYY-MM-DDTHH:mm:ss with optional offset)";

/// Parse a date string, trying Q1's explicit format list and ISO
/// timestamp forms. Returns `None` when nothing matches (no guessing
/// tail — the caller reports [`ACCEPTED_DATE_FORMS`]).
pub fn parse_date(input: &str) -> Option<ParsedDate> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    // ISO timestamp with offset (RFC 3339).
    if let Ok(odt) = OffsetDateTime::parse(input, &time::format_description::well_known::Rfc3339) {
        return Some(ParsedDate {
            datetime: PrimitiveDateTime::new(odt.date(), odt.time()),
            offset: Some(odt.offset()),
            has_time: true,
        });
    }

    // ISO timestamp without offset: 2026-07-01T09:30:00 or with space.
    let naive_ts = [
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        format_description!("[year]-[month]-[day]T[hour]:[minute]"),
    ];
    for fmt in naive_ts {
        if let Ok(dt) = PrimitiveDateTime::parse(input, fmt) {
            return Some(ParsedDate {
                datetime: dt,
                offset: None,
                has_time: true,
            });
        }
    }

    // Date-only forms (Q1's list). `padding:none` accepts both `3/7`
    // and `03/07` style components.
    let date_forms = [
        // MM/dd/yyyy
        format_description!("[month padding:none]/[day padding:none]/[year]"),
        // MM-dd-yyyy
        format_description!("[month padding:none]-[day padding:none]-[year]"),
        // yyyy-MM-dd
        format_description!("[year]-[month padding:none]-[day padding:none]"),
        // dd MM yyyy
        format_description!("[day padding:none] [month padding:none] [year]"),
        // MM dd, yyyy
        format_description!("[month padding:none] [day padding:none], [year]"),
    ];
    for fmt in date_forms {
        if let Ok(date) = time::Date::parse(input, fmt) {
            return Some(ParsedDate {
                datetime: PrimitiveDateTime::new(date, time::Time::MIDNIGHT),
                offset: None,
                has_time: false,
            });
        }
    }

    // Two-digit-year forms (MM/dd/yy, MM-dd-yy): `time` won't parse a
    // bare two-digit year into a full year, so pivot manually to
    // 20yy (Q1's behavior via Deno's datetime parser).
    let two_digit = [
        format_description!("[month padding:none]/[day padding:none]/[year repr:last_two]"),
        format_description!("[month padding:none]-[day padding:none]-[year repr:last_two]"),
    ];
    for fmt in two_digit {
        let mut parsed = time::parsing::Parsed::new();
        if parsed.parse_items(input.as_bytes(), fmt).is_ok()
            && let (Some(month), Some(day), Some(yy)) =
                (parsed.month(), parsed.day(), parsed.year_last_two())
            && let Ok(date) = time::Date::from_calendar_date(2000 + i32::from(yy), month, day.get())
        {
            return Some(ParsedDate {
                datetime: PrimitiveDateTime::new(date, time::Time::MIDNIGHT),
                offset: None,
                has_time: false,
            });
        }
    }

    None
}

/// A date output style: one of the named styles or a day.js-style
/// token format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateStyle {
    /// `Monday, March 7, 2005`
    Full,
    /// `March 7, 2005`
    Long,
    /// `Mar 7, 2005`
    Medium,
    /// `3/7/05`
    Short,
    /// `2005-03-07`
    Iso,
    /// A day.js-style token format string.
    Tokens(String),
}

impl DateStyle {
    /// Parse a `date-format` option value.
    pub fn parse(s: &str) -> Self {
        match s {
            "full" => Self::Full,
            "long" => Self::Long,
            "medium" => Self::Medium,
            "short" => Self::Short,
            "iso" => Self::Iso,
            other => Self::Tokens(other.to_string()),
        }
    }

    /// The equivalent token string (named styles are defined in terms
    /// of tokens; English-only per the design's localization deferral).
    fn token_string(&self) -> &str {
        match self {
            Self::Full => "dddd, MMMM D, YYYY",
            Self::Long => "MMMM D, YYYY",
            Self::Medium => "MMM D, YYYY",
            Self::Short => "M/D/YY",
            Self::Iso => "YYYY-MM-DD",
            Self::Tokens(s) => s,
        }
    }
}

/// Format a parsed date with the given style. Returns the formatted
/// string plus warnings for any known-but-deferred tokens
/// encountered (locale-week `w ww wo gggg`, named-timezone `z zzz`).
pub fn format_date(date: &ParsedDate, style: &DateStyle) -> (String, Vec<String>) {
    format_tokens(date, style.token_string())
}

const MONTHS_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS_FULL: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// English ordinal suffix (1st, 2nd, 3rd, 4th, …, 11th, 21st, …).
fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Known-but-deferred day.js tokens: matched (longest-first) so they
/// render literally with a warning instead of being split into
/// smaller supported tokens.
const DEFERRED_TOKENS: &[&str] = &["gggg", "wo", "ww", "w", "zzz", "z"];

/// Supported tokens, longest-first per starting letter (the
/// tokenizer takes the longest match at each position).
const SUPPORTED_TOKENS: &[&str] = &[
    "YYYY", "YY", "MMMM", "MMM", "MM", "M", "DD", "Do", "D", "dddd", "ddd", "dd", "d", "HH", "H",
    "hh", "h", "kk", "k", "mm", "m", "ss", "s", "SSS", "A", "a", "Q", "X", "x", "ZZ", "Z", "WW",
    "W", "GGGG",
];

/// Render one supported token.
fn render_token(token: &str, date: &ParsedDate) -> String {
    let dt = &date.datetime;
    let offset = date.offset.unwrap_or(UtcOffset::UTC);
    match token {
        "YYYY" => format!("{:04}", dt.year()),
        "YY" => format!("{:02}", dt.year().rem_euclid(100)),
        "MMMM" => MONTHS_FULL[dt.month() as usize - 1].to_string(),
        "MMM" => MONTHS_FULL[dt.month() as usize - 1][..3].to_string(),
        "MM" => format!("{:02}", dt.month() as u8),
        "M" => format!("{}", dt.month() as u8),
        "DD" => format!("{:02}", dt.day()),
        "Do" => ordinal(u32::from(dt.day())),
        "D" => format!("{}", dt.day()),
        "dddd" => WEEKDAYS_FULL[dt.weekday().number_days_from_sunday() as usize].to_string(),
        "ddd" => WEEKDAYS_FULL[dt.weekday().number_days_from_sunday() as usize][..3].to_string(),
        "dd" => WEEKDAYS_FULL[dt.weekday().number_days_from_sunday() as usize][..2].to_string(),
        "d" => format!("{}", dt.weekday().number_days_from_sunday()),
        "HH" => format!("{:02}", dt.hour()),
        "H" => format!("{}", dt.hour()),
        "hh" => format!("{:02}", twelve_hour(dt.hour())),
        "h" => format!("{}", twelve_hour(dt.hour())),
        "kk" => format!("{:02}", if dt.hour() == 0 { 24 } else { dt.hour() }),
        "k" => format!("{}", if dt.hour() == 0 { 24 } else { dt.hour() }),
        "mm" => format!("{:02}", dt.minute()),
        "m" => format!("{}", dt.minute()),
        "ss" => format!("{:02}", dt.second()),
        "s" => format!("{}", dt.second()),
        "SSS" => format!("{:03}", dt.millisecond()),
        "A" => if dt.hour() < 12 { "AM" } else { "PM" }.to_string(),
        "a" => if dt.hour() < 12 { "am" } else { "pm" }.to_string(),
        "Q" => format!("{}", (dt.month() as u8 - 1) / 3 + 1),
        "X" => format!("{}", date.to_offset_datetime().unix_timestamp()),
        "x" => format!(
            "{}",
            date.to_offset_datetime().unix_timestamp_nanos() / 1_000_000
        ),
        "ZZ" => {
            let (h, m, _) = offset.as_hms();
            format!(
                "{}{:02}{:02}",
                if h < 0 || m < 0 { "-" } else { "+" },
                h.abs(),
                m.abs()
            )
        }
        "Z" => {
            let (h, m, _) = offset.as_hms();
            format!(
                "{}{:02}:{:02}",
                if h < 0 || m < 0 { "-" } else { "+" },
                h.abs(),
                m.abs()
            )
        }
        "WW" => format!("{:02}", dt.iso_week()),
        "W" => format!("{}", dt.iso_week()),
        "GGGG" => {
            let (iso_year, _, _) = dt.to_iso_week_date();
            format!("{:04}", iso_year)
        }
        _ => unreachable!("render_token called with unsupported token {token}"),
    }
}

fn twelve_hour(h: u8) -> u8 {
    match h % 12 {
        0 => 12,
        other => other,
    }
}

/// Format with a day.js-style token string. `[...]` escapes literal
/// text; known-but-deferred tokens render literally and warn;
/// characters that are not day.js tokens pass through literally
/// (day.js semantics — e.g. the `T` in `YYYY-MM-DDTHH:mm:ssZ`).
fn format_tokens(date: &ParsedDate, format: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut warnings = Vec::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Bracket escape.
        if chars[i] == '[' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != ']' {
                out.push(chars[j]);
                j += 1;
            }
            i = if j < chars.len() { j + 1 } else { j };
            continue;
        }

        let rest: String = chars[i..].iter().collect();

        // Deferred tokens first (longest-first), so e.g. `wo` doesn't
        // fall through to two unknown characters.
        if let Some(tok) = DEFERRED_TOKENS.iter().find(|t| rest.starts_with(**t)) {
            warnings.push(format!(
                "date-format token `{tok}` is not supported yet \
                 (deferred: locale-week and named-timezone tokens); \
                 rendering it literally"
            ));
            out.push_str(tok);
            i += tok.len();
            continue;
        }

        // Supported tokens, longest match.
        if let Some(tok) = SUPPORTED_TOKENS.iter().find(|t| rest.starts_with(**t)) {
            out.push_str(&render_token(tok, date));
            i += tok.len();
            continue;
        }

        // Anything else (separators, unrecognized letters like `T`)
        // passes through literally, matching day.js.
        out.push(chars[i]);
        i += 1;
    }

    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(input: &str) -> ParsedDate {
        parse_date(input).unwrap_or_else(|| panic!("should parse: {input}"))
    }

    fn fmt(input: &str, style: &str) -> String {
        let (out, warnings) = format_date(&d(input), &DateStyle::parse(style));
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        out
    }

    // ── Parsing: Q1's accepted forms ─────────────────────────────────

    #[test]
    fn parses_q1_form_list() {
        // All spellings of March 7, 2005 from the Q1 docs page.
        for input in [
            "03/07/2005",
            "3/7/2005",
            "03-07-2005",
            "2005-03-07",
            "07 03 2005",
            "03 07, 2005",
        ] {
            let p = d(input);
            assert_eq!(
                (
                    p.datetime.year(),
                    p.datetime.month() as u8,
                    p.datetime.day()
                ),
                (2005, 3, 7),
                "{input}"
            );
            assert!(!p.has_time, "{input}");
        }
    }

    #[test]
    fn parses_two_digit_years_with_2000_pivot() {
        for input in ["03/07/05", "03-07-05"] {
            let p = d(input);
            assert_eq!(p.datetime.year(), 2005, "{input}");
        }
    }

    #[test]
    fn parses_iso_timestamps() {
        let with_offset = d("2005-03-07T00:00:00-05:00");
        assert!(with_offset.has_time);
        assert_eq!(with_offset.offset.unwrap().whole_hours(), -5);

        let naive = d("2026-07-01T09:30:00");
        assert!(naive.has_time);
        assert!(naive.offset.is_none());
        assert_eq!(naive.datetime.hour(), 9);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_date("not a date").is_none());
        assert!(parse_date("").is_none());
        // Q1's guessing tail is deliberately not ported.
        assert!(parse_date("7th of March, 2005").is_none());
    }

    // ── Named styles (Q1 docs table, en locale) ──────────────────────

    #[test]
    fn named_styles_match_q1_docs_examples() {
        assert_eq!(fmt("03/07/2005", "full"), "Monday, March 7, 2005");
        assert_eq!(fmt("03/07/2005", "long"), "March 7, 2005");
        assert_eq!(fmt("03/07/2005", "medium"), "Mar 7, 2005");
        assert_eq!(fmt("03/07/2005", "short"), "3/7/05");
        assert_eq!(fmt("03/07/2005", "iso"), "2005-03-07");
    }

    // ── Token strings (Q1 docs examples table) ───────────────────────

    #[test]
    fn token_examples_match_q1_docs() {
        assert_eq!(fmt("03/07/2005", "MMM D, YYYY"), "Mar 7, 2005");
        assert_eq!(fmt("03/07/2005", "DD/MM/YYYY"), "07/03/2005");
        assert_eq!(fmt("03/07/2005", "dddd MMM D, YYYY"), "Monday Mar 7, 2005");
        // Bracket escaping + literal-T passthrough (offset from input).
        assert_eq!(
            fmt(
                "2005-03-07T00:00:00-05:00",
                "[YYYYescape] YYYY-MM-DDTHH:mm:ssZ[Z]"
            ),
            "YYYYescape 2005-03-07T00:00:00-05:00Z"
        );
    }

    #[test]
    fn assorted_tokens() {
        assert_eq!(fmt("03/07/2005", "Do"), "7th");
        assert_eq!(fmt("03/21/2005", "Do"), "21st");
        assert_eq!(fmt("03/07/2005", "Q"), "1");
        assert_eq!(fmt("03/07/2005", "d"), "1"); // Monday, Sunday=0
        assert_eq!(fmt("03/07/2005", "dd"), "Mo");
        assert_eq!(fmt("2026-07-01T13:05:09", "h:mm A"), "1:05 PM");
        assert_eq!(fmt("2026-07-01T00:00:00", "k"), "24");
        // ISO week tokens (in scope per the approved design).
        assert_eq!(fmt("2005-03-07", "W"), "10");
        assert_eq!(fmt("2005-03-07", "WW"), "10");
        assert_eq!(fmt("2005-01-01", "GGGG"), "2004"); // ISO week-year
        // Unix timestamp of the epoch.
        assert_eq!(fmt("1970-01-01T00:00:00+00:00", "X"), "0");
    }

    #[test]
    fn deferred_tokens_warn_and_render_literally() {
        let (out, warnings) = format_date(&d("2005-03-07"), &DateStyle::parse("[Week] w, YYYY"));
        assert_eq!(out, "Week w, 2005");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains('w'));

        let (out, warnings) = format_date(&d("2005-03-07"), &DateStyle::parse("zzz"));
        assert_eq!(out, "zzz");
        assert_eq!(warnings.len(), 1);
    }

    // ── ISO machine form ─────────────────────────────────────────────

    #[test]
    fn iso_string_forms() {
        assert_eq!(d("03/07/2005").iso_string(), "2005-03-07");
        assert_eq!(
            d("2005-03-07T10:30:00-05:00").iso_string(),
            "2005-03-07T10:30:00-05:00"
        );
        assert_eq!(
            d("2026-07-01T09:30:00").iso_string(),
            "2026-07-01T09:30:00+00:00"
        );
    }
}
