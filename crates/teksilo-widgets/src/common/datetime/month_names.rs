// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Localized month names — `tr_widget!` keys grouped by width.

/// Long month name key (e.g. "January"). Month is `1..=12`; out-of-range
/// inputs return January's key (caller bug — months never come from user
/// input in the calendar code path).
pub fn month_long_key(month: i8) -> &'static str {
    match month {
        1 => "calendar-month-long-january",
        2 => "calendar-month-long-february",
        3 => "calendar-month-long-march",
        4 => "calendar-month-long-april",
        5 => "calendar-month-long-may",
        6 => "calendar-month-long-june",
        7 => "calendar-month-long-july",
        8 => "calendar-month-long-august",
        9 => "calendar-month-long-september",
        10 => "calendar-month-long-october",
        11 => "calendar-month-long-november",
        12 => "calendar-month-long-december",
        _ => "calendar-month-long-january",
    }
}

/// Short month name key (e.g. "Jan").
pub fn month_short_key(month: i8) -> &'static str {
    match month {
        1 => "calendar-month-short-january",
        2 => "calendar-month-short-february",
        3 => "calendar-month-short-march",
        4 => "calendar-month-short-april",
        5 => "calendar-month-short-may",
        6 => "calendar-month-short-june",
        7 => "calendar-month-short-july",
        8 => "calendar-month-short-august",
        9 => "calendar-month-short-september",
        10 => "calendar-month-short-october",
        11 => "calendar-month-short-november",
        12 => "calendar-month-short-december",
        _ => "calendar-month-short-january",
    }
}
