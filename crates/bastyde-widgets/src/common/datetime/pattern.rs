// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Strftime-subset pattern parser shared by `DateEdit` and `TimeEdit`.
//!
//! Supported tokens:
//!
//! | Token | Meaning |
//! | --- | --- |
//! | `%Y` | 4-digit year (zero-padded; sign printed for negatives) |
//! | `%m` / `%-m` | 2-digit / 1-or-2-digit month |
//! | `%d` / `%-d` | 2-digit / 1-or-2-digit day |
//! | `%H` / `%-H` | 24-hour hour |
//! | `%I` / `%-I` | 12-hour hour |
//! | `%M` / `%-M` | minute |
//! | `%S` / `%-S` | second |
//! | `%p` | AM/PM (12-hour mode) |
//! | `%%` | literal `%` |
//!
//! Anything else between tokens is preserved verbatim as a literal
//! separator. Locale-localized literal text (`%B` / `%A`) is deliberately
//! NOT supported here — month and weekday *names* come from `tr!` keys,
//! not from the pattern.

use jiff::civil::{Date, DateTime, Time};

/// One token in a parsed pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternToken {
    /// Editable segment.
    Segment(SegmentKind),
    /// Verbatim text between segments.
    Literal(String),
}

/// What a segment edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentKind {
    /// Year, 4 digits.
    Year,
    /// Month, 2 digits, zero-padded.
    Month,
    /// Month, 1-or-2 digits, no padding.
    MonthShort,
    /// Day of month, 2 digits, zero-padded.
    Day,
    /// Day of month, 1-or-2 digits, no padding.
    DayShort,
    /// Hour, 24-hour clock, 2 digits.
    Hour24,
    /// Hour, 24-hour clock, 1-or-2 digits.
    Hour24Short,
    /// Hour, 12-hour clock, 2 digits.
    Hour12,
    /// Hour, 12-hour clock, 1-or-2 digits.
    Hour12Short,
    /// Minute, 2 digits.
    Minute,
    /// Minute, 1-or-2 digits.
    MinuteShort,
    /// Second, 2 digits.
    Second,
    /// Second, 1-or-2 digits.
    SecondShort,
    /// AM/PM marker.
    Period,
}

impl SegmentKind {
    /// Number of digit characters this segment renders. Period is non-numeric.
    pub fn max_digits(self) -> usize {
        match self {
            Self::Year => 4,
            Self::Month | Self::Day | Self::Hour24 | Self::Hour12 | Self::Minute | Self::Second => {
                2
            }
            Self::MonthShort
            | Self::DayShort
            | Self::Hour24Short
            | Self::Hour12Short
            | Self::MinuteShort
            | Self::SecondShort => 2,
            Self::Period => 0,
        }
    }

    /// Inclusive numeric range valid for this segment, ignoring
    /// month-length variation (`Day` is `1..=31`).
    pub fn value_range(self) -> Option<(i32, i32)> {
        match self {
            Self::Year => Some((-9999, 9999)),
            Self::Month | Self::MonthShort => Some((1, 12)),
            Self::Day | Self::DayShort => Some((1, 31)),
            Self::Hour24 | Self::Hour24Short => Some((0, 23)),
            Self::Hour12 | Self::Hour12Short => Some((1, 12)),
            Self::Minute | Self::MinuteShort | Self::Second | Self::SecondShort => Some((0, 59)),
            Self::Period => None,
        }
    }
}

/// A pattern parsed into segments + literals, ready for formatting and
/// reverse parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPattern {
    pub tokens: Vec<PatternToken>,
}

impl ParsedPattern {
    /// Parse a strftime-subset pattern.
    pub fn parse(pattern: &str) -> Result<Self, PatternError> {
        let mut tokens = Vec::new();
        let mut literal = String::new();
        let mut chars = pattern.chars().peekable();

        let flush_literal = |lit: &mut String, tokens: &mut Vec<PatternToken>| {
            if !lit.is_empty() {
                tokens.push(PatternToken::Literal(std::mem::take(lit)));
            }
        };

        while let Some(c) = chars.next() {
            if c != '%' {
                literal.push(c);
                continue;
            }
            // Peek for `-` modifier (no padding) or directive char.
            let no_pad = matches!(chars.peek(), Some('-'));
            if no_pad {
                chars.next();
            }
            let Some(d) = chars.next() else {
                return Err(PatternError::TrailingPercent);
            };
            let segment = match (no_pad, d) {
                (false, 'Y') => SegmentKind::Year,
                (true, 'Y') => SegmentKind::Year,
                (false, 'm') => SegmentKind::Month,
                (true, 'm') => SegmentKind::MonthShort,
                (false, 'd') => SegmentKind::Day,
                (true, 'd') => SegmentKind::DayShort,
                (false, 'H') => SegmentKind::Hour24,
                (true, 'H') => SegmentKind::Hour24Short,
                (false, 'I') => SegmentKind::Hour12,
                (true, 'I') => SegmentKind::Hour12Short,
                (false, 'M') => SegmentKind::Minute,
                (true, 'M') => SegmentKind::MinuteShort,
                (false, 'S') => SegmentKind::Second,
                (true, 'S') => SegmentKind::SecondShort,
                (false, 'p') => SegmentKind::Period,
                (false, '%') => {
                    literal.push('%');
                    continue;
                }
                (no_pad, ch) => {
                    return Err(PatternError::UnsupportedDirective {
                        directive: ch,
                        with_dash_modifier: no_pad,
                    });
                }
            };
            flush_literal(&mut literal, &mut tokens);
            tokens.push(PatternToken::Segment(segment));
        }
        flush_literal(&mut literal, &mut tokens);
        Ok(Self { tokens })
    }

    /// Iterator over only the segment tokens, in document order.
    pub fn segments(&self) -> impl Iterator<Item = SegmentKind> + '_ {
        self.tokens.iter().filter_map(|t| match t {
            PatternToken::Segment(k) => Some(*k),
            _ => None,
        })
    }
}

/// Pattern parse errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// `%` at end of string.
    #[error("trailing `%` at end of pattern")]
    TrailingPercent,
    /// `%X` where `X` isn't a supported directive.
    #[error(
        "{}",
        if *with_dash_modifier {
            format!("unsupported directive `%-{directive}` in pattern")
        } else {
            format!("unsupported directive `%{directive}` in pattern")
        }
    )]
    UnsupportedDirective {
        directive: char,
        with_dash_modifier: bool,
    },
}

/// What a segment evaluates to. `Year` can be negative; the rest
/// are non-negative integers. `Period` is `0` for AM, `1` for PM.
pub fn segment_value_for_date(d: Date, kind: SegmentKind) -> Option<i32> {
    Some(match kind {
        SegmentKind::Year => d.year() as i32,
        SegmentKind::Month | SegmentKind::MonthShort => d.month() as i32,
        SegmentKind::Day | SegmentKind::DayShort => d.day() as i32,
        _ => return None,
    })
}

/// Time-half evaluator.
pub fn segment_value_for_time(t: Time, kind: SegmentKind) -> Option<i32> {
    Some(match kind {
        SegmentKind::Hour24 | SegmentKind::Hour24Short => t.hour() as i32,
        SegmentKind::Hour12 | SegmentKind::Hour12Short => {
            let h = t.hour() as i32;
            let h12 = h % 12;
            if h12 == 0 { 12 } else { h12 }
        }
        SegmentKind::Minute | SegmentKind::MinuteShort => t.minute() as i32,
        SegmentKind::Second | SegmentKind::SecondShort => t.second() as i32,
        SegmentKind::Period => {
            if t.hour() < 12 {
                0
            } else {
                1
            }
        }
        _ => return None,
    })
}

/// Format a value into its segment string.
pub fn render_segment(kind: SegmentKind, value: i32) -> String {
    match kind {
        SegmentKind::Year => {
            if value < 0 {
                format!("-{:04}", value.unsigned_abs())
            } else {
                format!("{:04}", value)
            }
        }
        SegmentKind::Month
        | SegmentKind::Day
        | SegmentKind::Hour24
        | SegmentKind::Hour12
        | SegmentKind::Minute
        | SegmentKind::Second => format!("{:02}", value),
        SegmentKind::MonthShort
        | SegmentKind::DayShort
        | SegmentKind::Hour24Short
        | SegmentKind::Hour12Short
        | SegmentKind::MinuteShort
        | SegmentKind::SecondShort => format!("{}", value),
        SegmentKind::Period => {
            if value == 0 {
                "AM".to_string()
            } else {
                "PM".to_string()
            }
        }
    }
}

/// Format a `Date` against a parsed pattern. Time segments emit "00".
pub fn format_value(pattern: &ParsedPattern, date: Option<Date>, time: Option<Time>) -> String {
    let mut out = String::new();
    for token in &pattern.tokens {
        match token {
            PatternToken::Literal(s) => out.push_str(s),
            PatternToken::Segment(kind) => {
                let value = match (date, time) {
                    (Some(d), _) if segment_value_for_date(d, *kind).is_some() => {
                        segment_value_for_date(d, *kind).expect("guarded by is_some() above")
                    }
                    (_, Some(t)) => segment_value_for_time(t, *kind).unwrap_or(0),
                    (Some(d), None) => segment_value_for_date(d, *kind).unwrap_or(0),
                    (None, None) => 0,
                };
                out.push_str(&render_segment(*kind, value));
            }
        }
    }
    out
}

/// What kind of value is being parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseTarget {
    DateOnly,
    TimeOnly,
    DateTime,
}

/// Parsed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedValue {
    Date(Date),
    Time(Time),
    DateTime(DateTime),
}

/// Reverse-parse a string against a pattern.
///
/// Strict about literal separators but **lenient about trailing
/// segments**: input that runs out partway through the pattern fills
/// the remaining segments with sensible defaults (month → `1`, day →
/// `1`, hour/minute/second → `0`, AM/PM → AM). The first segment of
/// each target *is* required:
///
/// - `DateOnly` / `DateTime`: the year must be present.
/// - `TimeOnly`: the hour must be present.
///
/// Examples for pattern `%Y-%m-%d`:
///
/// | Input | Result |
/// | --- | --- |
/// | `"2026-05-02"` | `Date(2026, 5, 2)` |
/// | `"2026-5-2"` | `Date(2026, 5, 2)` (1-digit segments) |
/// | `"2026-5"` | `Date(2026, 5, 1)` (day defaulted) |
/// | `"2026-"` | `Date(2026, 1, 1)` (month + day defaulted) |
/// | `"2026"` | `Date(2026, 1, 1)` |
/// | `""` | `None` (year required) |
/// | `"2026/05/02"` | `None` (literal separator mismatch) |
/// | `"2026-13-02"` | `None` (out of range) |
///
/// Out-of-range values and literal-separator mismatches always
/// reject — leniency only applies to *missing* trailing input.
pub fn parse_value(
    pattern: &ParsedPattern,
    input: &str,
    target: ParseTarget,
) -> Option<ParsedValue> {
    let mut cursor = input;
    let mut year: Option<i16> = None;
    let mut month: Option<i8> = None;
    let mut day: Option<i8> = None;
    let mut hour24: Option<i8> = None;
    let mut hour12: Option<i8> = None;
    let mut minute: Option<i8> = None;
    let mut second: Option<i8> = None;
    let mut period: Option<i8> = None; // 0 = AM, 1 = PM

    for token in &pattern.tokens {
        // End-of-input mid-pattern: stop consuming, let the
        // defaults fill in the rest. Empty cursor that lands on a
        // literal token is fine — we just skip the literal. Empty
        // cursor on a segment token is fine — that segment stays
        // None and the per-target resolver below applies its
        // default (month=1, day=1, ...).
        if cursor.is_empty() {
            break;
        }
        match token {
            PatternToken::Literal(lit) => {
                // Lenient: accept a partial literal at end-of-input
                // (e.g. user typed "2026-" with pattern "%Y-%m-%d").
                // If the literal is the start of cursor we consume
                // it; if cursor is shorter than literal AND is a
                // prefix of literal, we accept and stop.
                if let Some(trimmed) = cursor.strip_prefix(lit.as_str()) {
                    cursor = trimmed;
                } else if lit.starts_with(cursor) {
                    // Cursor is a strict prefix of the expected
                    // literal — treat as "user stopped typing
                    // mid-separator", consume what's there.
                    cursor = "";
                } else {
                    // Literal mismatch (e.g. "/" where "-" was
                    // expected). Always reject.
                    return None;
                }
            }
            PatternToken::Segment(kind) => {
                if matches!(kind, SegmentKind::Period) {
                    // Period needs at least 1 char to pick AM vs
                    // PM. Be tolerant of a single 'A'/'P' (user
                    // mid-typing). Lower-case accepted.
                    let first_char = cursor.chars().next()?;
                    let upper = first_char.to_ascii_uppercase();
                    match upper {
                        'A' => {
                            period = Some(0);
                            cursor = consume_period_letters(cursor);
                        }
                        'P' => {
                            period = Some(1);
                            cursor = consume_period_letters(cursor);
                        }
                        _ => return None,
                    }
                    continue;
                }
                let max = kind.max_digits();
                let Some((digits, rest)) = take_digits(cursor, max) else {
                    // No digits at cursor — treat as missing
                    // segment, stop here. Subsequent tokens get
                    // defaults via the resolver below.
                    break;
                };
                cursor = rest;
                let v: i32 = digits.parse().ok()?;
                let (lo, hi) = kind.value_range().unwrap_or((i32::MIN, i32::MAX));
                if v < lo || v > hi {
                    return None;
                }
                match kind {
                    SegmentKind::Year => year = Some(v as i16),
                    SegmentKind::Month | SegmentKind::MonthShort => month = Some(v as i8),
                    SegmentKind::Day | SegmentKind::DayShort => day = Some(v as i8),
                    SegmentKind::Hour24 | SegmentKind::Hour24Short => hour24 = Some(v as i8),
                    SegmentKind::Hour12 | SegmentKind::Hour12Short => hour12 = Some(v as i8),
                    SegmentKind::Minute | SegmentKind::MinuteShort => minute = Some(v as i8),
                    SegmentKind::Second | SegmentKind::SecondShort => second = Some(v as i8),
                    SegmentKind::Period => unreachable!(),
                }
            }
        }
    }
    // Trailing whitespace after consumption is OK; non-whitespace
    // junk is a parse error.
    if !cursor.trim().is_empty() {
        return None;
    }

    // Resolve hour from 12h + AM/PM if 24h not present.
    let hour = match (hour24, hour12, period) {
        (Some(h), _, _) => h,
        (None, Some(h12), Some(p)) => {
            let base = h12 % 12;
            base + if p == 1 { 12 } else { 0 }
        }
        (None, Some(h12), None) => h12 % 12, // assume AM
        (None, None, _) => 0,
    };

    match target {
        ParseTarget::DateOnly => {
            // Year is required; month and day default to 1 (start of
            // year / start of month).
            let y = year?;
            let m = month.unwrap_or(1);
            let d = day.unwrap_or(1);
            Date::new(y, m, d).ok().map(ParsedValue::Date)
        }
        ParseTarget::TimeOnly => {
            // Hour is required (either 24h or 12h); m/s default to 0.
            if hour24.is_none() && hour12.is_none() {
                return None;
            }
            Time::new(hour, minute.unwrap_or(0), second.unwrap_or(0), 0)
                .ok()
                .map(ParsedValue::Time)
        }
        ParseTarget::DateTime => {
            let y = year?;
            let m = month.unwrap_or(1);
            let d = day.unwrap_or(1);
            let date = Date::new(y, m, d).ok()?;
            let time = Time::new(hour, minute.unwrap_or(0), second.unwrap_or(0), 0).ok()?;
            Some(ParsedValue::DateTime(DateTime::from_parts(date, time)))
        }
    }
}

/// Build an InputMask grammar string from a parsed pattern. Each
/// digit segment becomes the corresponding number of `9` chars; the
/// AM/PM period segment becomes `>AA` (two uppercase-locked letters);
/// literals stay as fixed separators.
///
/// Pattern → mask:
/// - `%Y-%m-%d` → `9999-99-99`
/// - `%m/%d/%Y` → `99/99/9999`
/// - `%H:%M:%S` → `99:99:99`
/// - `%I:%M %p` → `99:99 >AA`
///
/// Backslashes in the pattern's literal positions are escaped via
/// `\\` so the InputMask parser treats them as literals (otherwise
/// `\X` in a literal would consume the following char).
pub fn mask_for_pattern(pattern: &ParsedPattern) -> String {
    let mut out = String::new();
    for token in &pattern.tokens {
        match token {
            PatternToken::Literal(s) => {
                for c in s.chars() {
                    // Mask grammar metacharacters need escaping in
                    // literal positions; everything else passes
                    // through unchanged.
                    if matches!(
                        c,
                        '9' | '0'
                            | 'A'
                            | 'a'
                            | 'N'
                            | 'n'
                            | 'X'
                            | 'x'
                            | 'H'
                            | 'h'
                            | '>'
                            | '<'
                            | '!'
                            | '\\'
                    ) {
                        out.push('\\');
                    }
                    out.push(c);
                }
            }
            PatternToken::Segment(kind) => {
                let digits = match kind {
                    SegmentKind::Year => 4,
                    SegmentKind::Month
                    | SegmentKind::MonthShort
                    | SegmentKind::Day
                    | SegmentKind::DayShort
                    | SegmentKind::Hour24
                    | SegmentKind::Hour24Short
                    | SegmentKind::Hour12
                    | SegmentKind::Hour12Short
                    | SegmentKind::Minute
                    | SegmentKind::MinuteShort
                    | SegmentKind::Second
                    | SegmentKind::SecondShort => 2,
                    SegmentKind::Period => {
                        out.push_str(">AA");
                        continue;
                    }
                };
                for _ in 0..digits {
                    out.push('9');
                }
            }
        }
    }
    out
}

/// One editable segment's span in formatted-text display coordinates.
/// `(start_char_offset, end_char_offset_exclusive, kind)`. Computed by
/// walking [`ParsedPattern::tokens`] and accumulating the rendered
/// width of each segment (4 for `Year`, 2 for `Period` (`AM`/`PM`),
/// 2 for every digit segment) and each literal. Drives caret-in-
/// segment lookups for segment-stepping and per-segment selection.
pub fn segments_layout(pattern: &ParsedPattern) -> Vec<(usize, usize, SegmentKind)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for token in &pattern.tokens {
        match token {
            PatternToken::Literal(s) => pos += s.chars().count(),
            PatternToken::Segment(kind) => {
                let width = match kind {
                    SegmentKind::Year => 4,
                    // "AM" / "PM" — see `render_segment` for the
                    // canonical width of every segment kind.
                    SegmentKind::Period => 2,
                    _ => 2,
                };
                out.push((pos, pos + width, *kind));
                pos += width;
            }
        }
    }
    out
}

/// Find the segment under `caret_pos` (in formatted-text display
/// coordinates). A caret resting on a segment boundary belongs to the
/// segment to its right (typing-into semantics). A caret on a separator
/// snaps to the *preceding* segment so Up/Down keep working when the
/// caret is between two segments. Returns `None` only when the pattern
/// has no editable segments at all.
pub fn segment_at_position(
    pattern: &ParsedPattern,
    caret_pos: usize,
) -> Option<(usize, usize, SegmentKind)> {
    let layout = segments_layout(pattern);
    // First pass: caret strictly inside a segment.
    for &(start, end, kind) in &layout {
        if caret_pos >= start && caret_pos < end {
            return Some((start, end, kind));
        }
    }
    // Caret on a separator or past the end: snap to the nearest
    // segment that ENDS at-or-before the caret (preceding segment).
    layout
        .iter()
        .rev()
        .find(|(_, end, _)| *end <= caret_pos)
        .copied()
        .or_else(|| layout.first().copied())
}

/// Step a single field of a `Date` by `delta`. Year saturates at the
/// jiff range and clamps the day if the new year+month no longer holds
/// the current day (e.g. Feb 29 → Feb 28 in non-leap years). Month
/// wraps within `[1, 12]` and clamps the day to the new month's last
/// day. Day wraps within the current month — does not advance to the
/// next month, matching Qt `QDateEdit` and macOS Calendar behaviour.
/// Returns the input `date` unchanged when `kind` is not a date field.
pub fn step_date_field(date: Date, kind: SegmentKind, delta: i32) -> Date {
    match kind {
        SegmentKind::Year => {
            let new_year = (date.year() as i32 + delta).clamp(-9999, 9999) as i16;
            let last_day = Date::new(new_year, date.month(), 1)
                .map(|d| d.last_of_month().day())
                .unwrap_or(date.day());
            let day = date.day().min(last_day);
            Date::new(new_year, date.month(), day).unwrap_or(date)
        }
        SegmentKind::Month | SegmentKind::MonthShort => {
            let new_month = ((date.month() as i32 - 1).rem_euclid(12) + delta).rem_euclid(12) + 1;
            let new_month = new_month as i8;
            let last_day = Date::new(date.year(), new_month, 1)
                .map(|d| d.last_of_month().day())
                .unwrap_or(date.day());
            let day = date.day().min(last_day);
            Date::new(date.year(), new_month, day).unwrap_or(date)
        }
        SegmentKind::Day | SegmentKind::DayShort => {
            let last_day = Date::new(date.year(), date.month(), 1)
                .map(|d| d.last_of_month().day())
                .unwrap_or(28) as i32;
            let new_day = (date.day() as i32 - 1 + delta).rem_euclid(last_day) + 1;
            Date::new(date.year(), date.month(), new_day as i8).unwrap_or(date)
        }
        _ => date,
    }
}

/// Step a single field of a `Time` by `delta`. Hour wraps in `[0, 24)`
/// (whether 12h or 24h segment kind — internal storage is 24h).
/// Minute and second wrap in `[0, 60)`. AM/PM toggles on any non-zero
/// `delta` (sign-agnostic). Returns the input `time` unchanged when
/// `kind` is not a time field.
pub fn step_time_field(time: Time, kind: SegmentKind, delta: i32) -> Time {
    match kind {
        SegmentKind::Hour24
        | SegmentKind::Hour24Short
        | SegmentKind::Hour12
        | SegmentKind::Hour12Short => {
            let h = (time.hour() as i32 + delta).rem_euclid(24) as i8;
            Time::new(h, time.minute(), time.second(), 0).unwrap_or(time)
        }
        SegmentKind::Minute | SegmentKind::MinuteShort => {
            let m = (time.minute() as i32 + delta).rem_euclid(60) as i8;
            Time::new(time.hour(), m, time.second(), 0).unwrap_or(time)
        }
        SegmentKind::Second | SegmentKind::SecondShort => {
            let s = (time.second() as i32 + delta).rem_euclid(60) as i8;
            Time::new(time.hour(), time.minute(), s, 0).unwrap_or(time)
        }
        SegmentKind::Period => {
            if delta == 0 {
                return time;
            }
            let h = (time.hour() as i32 + 12).rem_euclid(24) as i8;
            Time::new(h, time.minute(), time.second(), 0).unwrap_or(time)
        }
        _ => time,
    }
}

/// Consume up to 2 ASCII letters at the front of `cursor` (so "AM",
/// "Am", "PM", or a bare "A"/"P" mid-typing all advance the cursor
/// correctly). Returns the trimmed slice.
fn consume_period_letters(cursor: &str) -> &str {
    let mut end = 0;
    for (i, ch) in cursor.char_indices() {
        if ch.is_ascii_alphabetic() && end < 2 {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    &cursor[end..]
}

/// Take up to `max` ASCII-digit characters from the front of the
/// string. Returns `None` if there are zero digits at the front.
fn take_digits(s: &str, max: usize) -> Option<(&str, &str)> {
    let mut end = 0;
    for (i, ch) in s.char_indices() {
        if ch.is_ascii_digit() && end < max {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        None
    } else {
        Some((&s[..end], &s[end..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_pattern() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        assert_eq!(
            pat.tokens,
            vec![
                PatternToken::Segment(SegmentKind::Year),
                PatternToken::Literal("-".to_string()),
                PatternToken::Segment(SegmentKind::Month),
                PatternToken::Literal("-".to_string()),
                PatternToken::Segment(SegmentKind::Day),
            ]
        );
    }

    #[test]
    fn parses_us_pattern() {
        let pat = ParsedPattern::parse("%m/%d/%Y").unwrap();
        let segs: Vec<_> = pat.segments().collect();
        assert_eq!(
            segs,
            vec![SegmentKind::Month, SegmentKind::Day, SegmentKind::Year]
        );
    }

    #[test]
    fn parses_dotted_european_pattern() {
        let pat = ParsedPattern::parse("%d.%m.%Y").unwrap();
        assert_eq!(pat.segments().count(), 3);
    }

    #[test]
    fn rejects_unsupported_directive() {
        assert!(matches!(
            ParsedPattern::parse("%Y-%B-%d"),
            Err(PatternError::UnsupportedDirective { directive: 'B', .. })
        ));
    }

    #[test]
    fn rejects_trailing_percent() {
        assert_eq!(
            ParsedPattern::parse("%Y-%"),
            Err(PatternError::TrailingPercent)
        );
    }

    #[test]
    fn round_trip_iso() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        let date = Date::constant(2026, 5, 2);
        let s = format_value(&pat, Some(date), None);
        assert_eq!(s, "2026-05-02");
        match parse_value(&pat, &s, ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, date),
            other => panic!("expected Date, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_us() {
        let pat = ParsedPattern::parse("%m/%d/%Y").unwrap();
        let date = Date::constant(2026, 5, 2);
        let s = format_value(&pat, Some(date), None);
        assert_eq!(s, "05/02/2026");
        match parse_value(&pat, &s, ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, date),
            other => panic!("expected Date, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_out_of_range_month() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        assert!(parse_value(&pat, "2026-13-02", ParseTarget::DateOnly).is_none());
    }

    #[test]
    fn parse_rejects_invalid_separator() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        assert!(parse_value(&pat, "2026/05/02", ParseTarget::DateOnly).is_none());
    }

    #[test]
    fn time_round_trip_24h() {
        let pat = ParsedPattern::parse("%H:%M:%S").unwrap();
        let time = Time::new(14, 35, 7, 0).unwrap();
        let s = format_value(&pat, None, Some(time));
        assert_eq!(s, "14:35:07");
        match parse_value(&pat, &s, ParseTarget::TimeOnly) {
            Some(ParsedValue::Time(t)) => assert_eq!(t, time),
            other => panic!("expected Time, got {other:?}"),
        }
    }

    #[test]
    fn time_round_trip_12h_with_period() {
        let pat = ParsedPattern::parse("%I:%M %p").unwrap();
        let time = Time::new(14, 35, 0, 0).unwrap();
        let s = format_value(&pat, None, Some(time));
        assert_eq!(s, "02:35 PM");
        match parse_value(&pat, &s, ParseTarget::TimeOnly) {
            Some(ParsedValue::Time(t)) => assert_eq!(t.hour(), 14),
            other => panic!("expected Time, got {other:?}"),
        }
    }

    #[test]
    fn lenient_year_only_defaults_month_and_day() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        match parse_value(&pat, "2026", ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, Date::constant(2026, 1, 1)),
            other => panic!("expected Date(2026,1,1), got {other:?}"),
        }
    }

    #[test]
    fn lenient_year_month_only_defaults_day() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        match parse_value(&pat, "2026-5", ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, Date::constant(2026, 5, 1)),
            other => panic!("expected Date(2026,5,1), got {other:?}"),
        }
    }

    #[test]
    fn lenient_trailing_separator_accepted() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        match parse_value(&pat, "2026-", ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, Date::constant(2026, 1, 1)),
            other => panic!("expected Date(2026,1,1), got {other:?}"),
        }
        match parse_value(&pat, "2026-5-", ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, Date::constant(2026, 5, 1)),
            other => panic!("expected Date(2026,5,1), got {other:?}"),
        }
    }

    #[test]
    fn lenient_two_digit_month_one_digit_day() {
        // Tests that `take_digits(2)` accepts a 1-digit run when the
        // pattern asked for 2-digit month/day. Already worked before
        // the lenient change but worth a regression guard.
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        match parse_value(&pat, "2026-5-2", ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, Date::constant(2026, 5, 2)),
            other => panic!("expected Date(2026,5,2), got {other:?}"),
        }
    }

    #[test]
    fn lenient_empty_input_rejects() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        assert!(parse_value(&pat, "", ParseTarget::DateOnly).is_none());
        assert!(parse_value(&pat, "   ", ParseTarget::DateOnly).is_none());
    }

    #[test]
    fn lenient_us_pattern_partial_input() {
        // `%m/%d/%Y` — month required first; trailing year/day fill
        // with defaults.
        let pat = ParsedPattern::parse("%m/%d/%Y").unwrap();
        match parse_value(&pat, "5", ParseTarget::DateOnly) {
            // No year → reject. Year is the *third* segment in MDY
            // pattern, so a valid lenient parse needs it present.
            None => {}
            other => panic!("expected None (no year), got {other:?}"),
        }
        match parse_value(&pat, "5/2/2026", ParseTarget::DateOnly) {
            Some(ParsedValue::Date(d)) => assert_eq!(d, Date::constant(2026, 5, 2)),
            other => panic!("expected Date, got {other:?}"),
        }
    }

    #[test]
    fn lenient_time_partial() {
        let pat = ParsedPattern::parse("%H:%M:%S").unwrap();
        // Hour-only → minute and second default to 0.
        match parse_value(&pat, "14", ParseTarget::TimeOnly) {
            Some(ParsedValue::Time(t)) => assert_eq!(t, Time::new(14, 0, 0, 0).unwrap()),
            other => panic!("expected Time(14:00:00), got {other:?}"),
        }
        // Hour:minute → second defaults to 0.
        match parse_value(&pat, "14:35", ParseTarget::TimeOnly) {
            Some(ParsedValue::Time(t)) => assert_eq!(t, Time::new(14, 35, 0, 0).unwrap()),
            other => panic!("expected Time(14:35:00), got {other:?}"),
        }
    }

    #[test]
    fn mask_for_iso_date() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        assert_eq!(mask_for_pattern(&pat), "9999-99-99");
    }

    #[test]
    fn mask_for_us_date() {
        let pat = ParsedPattern::parse("%m/%d/%Y").unwrap();
        assert_eq!(mask_for_pattern(&pat), "99/99/9999");
    }

    #[test]
    fn mask_for_european_date() {
        let pat = ParsedPattern::parse("%d.%m.%Y").unwrap();
        assert_eq!(mask_for_pattern(&pat), "99.99.9999");
    }

    #[test]
    fn mask_for_24h_time() {
        let pat = ParsedPattern::parse("%H:%M").unwrap();
        assert_eq!(mask_for_pattern(&pat), "99:99");
        let pat = ParsedPattern::parse("%H:%M:%S").unwrap();
        assert_eq!(mask_for_pattern(&pat), "99:99:99");
    }

    #[test]
    fn mask_for_12h_time() {
        let pat = ParsedPattern::parse("%I:%M %p").unwrap();
        assert_eq!(mask_for_pattern(&pat), "99:99 >AA");
    }

    #[test]
    fn segment_kind_max_digits() {
        assert_eq!(SegmentKind::Year.max_digits(), 4);
        assert_eq!(SegmentKind::Month.max_digits(), 2);
        assert_eq!(SegmentKind::Day.max_digits(), 2);
        assert_eq!(SegmentKind::Period.max_digits(), 0);
    }

    // ── Segment layout / position lookup ────────────────────────────

    #[test]
    fn segments_layout_iso_pattern() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        assert_eq!(
            segments_layout(&pat),
            vec![
                (0, 4, SegmentKind::Year),
                (5, 7, SegmentKind::Month),
                (8, 10, SegmentKind::Day),
            ]
        );
    }

    #[test]
    fn segments_layout_with_period() {
        let pat = ParsedPattern::parse("%I:%M %p").unwrap();
        assert_eq!(
            segments_layout(&pat),
            vec![
                (0, 2, SegmentKind::Hour12),
                (3, 5, SegmentKind::Minute),
                (6, 8, SegmentKind::Period),
            ]
        );
    }

    #[test]
    fn segment_at_position_inside_segments() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        // caret 0..4 → Year
        assert_eq!(
            segment_at_position(&pat, 0).map(|s| s.2),
            Some(SegmentKind::Year)
        );
        assert_eq!(
            segment_at_position(&pat, 3).map(|s| s.2),
            Some(SegmentKind::Year)
        );
        // caret 5..7 → Month
        assert_eq!(
            segment_at_position(&pat, 5).map(|s| s.2),
            Some(SegmentKind::Month)
        );
        assert_eq!(
            segment_at_position(&pat, 6).map(|s| s.2),
            Some(SegmentKind::Month)
        );
        // caret 8..10 → Day
        assert_eq!(
            segment_at_position(&pat, 9).map(|s| s.2),
            Some(SegmentKind::Day)
        );
    }

    #[test]
    fn segment_at_position_on_separator_snaps_left() {
        let pat = ParsedPattern::parse("%Y-%m-%d").unwrap();
        // caret 4 sits on the boundary at end of Year / start of `-`.
        // Snaps to Year (preceding segment).
        assert_eq!(
            segment_at_position(&pat, 4).map(|s| s.2),
            Some(SegmentKind::Year)
        );
        // caret 7 = end of Month, sits on boundary with `-`.
        assert_eq!(
            segment_at_position(&pat, 7).map(|s| s.2),
            Some(SegmentKind::Month)
        );
        // caret 10 = end of Day (text end). Snaps to Day.
        assert_eq!(
            segment_at_position(&pat, 10).map(|s| s.2),
            Some(SegmentKind::Day)
        );
    }

    // ── step_date_field ─────────────────────────────────────────────

    #[test]
    fn step_year_basic_increment() {
        let d = Date::new(2026, 5, 15).unwrap();
        assert_eq!(
            step_date_field(d, SegmentKind::Year, 1),
            Date::new(2027, 5, 15).unwrap()
        );
        assert_eq!(
            step_date_field(d, SegmentKind::Year, -10),
            Date::new(2016, 5, 15).unwrap()
        );
    }

    #[test]
    fn step_year_clamps_feb_29() {
        let leap = Date::new(2024, 2, 29).unwrap();
        // Stepping to 2025 (non-leap) clamps day to 28.
        assert_eq!(
            step_date_field(leap, SegmentKind::Year, 1),
            Date::new(2025, 2, 28).unwrap()
        );
    }

    #[test]
    fn step_month_wraps_within_year() {
        // Dec → Jan stays in same year (display-segment wrap).
        let d = Date::new(2026, 12, 5).unwrap();
        assert_eq!(
            step_date_field(d, SegmentKind::Month, 1),
            Date::new(2026, 1, 5).unwrap()
        );
        // Jan → Dec
        let d = Date::new(2026, 1, 5).unwrap();
        assert_eq!(
            step_date_field(d, SegmentKind::Month, -1),
            Date::new(2026, 12, 5).unwrap()
        );
    }

    #[test]
    fn step_month_clamps_day() {
        // Mar 31 → Feb (28 in 2026)
        let d = Date::new(2026, 3, 31).unwrap();
        assert_eq!(
            step_date_field(d, SegmentKind::Month, -1),
            Date::new(2026, 2, 28).unwrap()
        );
    }

    #[test]
    fn step_day_wraps_within_month() {
        // 31 + 1 in March → 1 (same month, not April).
        let d = Date::new(2026, 3, 31).unwrap();
        assert_eq!(
            step_date_field(d, SegmentKind::Day, 1),
            Date::new(2026, 3, 1).unwrap()
        );
        // 1 - 1 in March → 31
        let d = Date::new(2026, 3, 1).unwrap();
        assert_eq!(
            step_date_field(d, SegmentKind::Day, -1),
            Date::new(2026, 3, 31).unwrap()
        );
    }

    // ── step_time_field ─────────────────────────────────────────────

    #[test]
    fn step_hour_wraps_24h() {
        let t = Time::new(23, 30, 0, 0).unwrap();
        assert_eq!(
            step_time_field(t, SegmentKind::Hour24, 1),
            Time::new(0, 30, 0, 0).unwrap()
        );
        let t = Time::new(0, 30, 0, 0).unwrap();
        assert_eq!(
            step_time_field(t, SegmentKind::Hour24, -1),
            Time::new(23, 30, 0, 0).unwrap()
        );
    }

    #[test]
    fn step_minute_wraps_60() {
        let t = Time::new(10, 59, 0, 0).unwrap();
        assert_eq!(
            step_time_field(t, SegmentKind::Minute, 1),
            Time::new(10, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn step_period_toggles_am_pm() {
        let am = Time::new(9, 0, 0, 0).unwrap();
        assert_eq!(
            step_time_field(am, SegmentKind::Period, 1),
            Time::new(21, 0, 0, 0).unwrap()
        );
        let pm = Time::new(15, 30, 0, 0).unwrap();
        assert_eq!(
            step_time_field(pm, SegmentKind::Period, -1),
            Time::new(3, 30, 0, 0).unwrap()
        );
        // Zero delta is a no-op
        assert_eq!(step_time_field(am, SegmentKind::Period, 0), am);
    }
}
