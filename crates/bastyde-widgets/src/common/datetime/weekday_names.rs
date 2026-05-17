//! Localized weekday names — `tr_widget!` keys grouped by width.

use jiff::civil::Weekday;

/// Long weekday name key (e.g. "Monday"). Used by AccessKit labels on
/// calendar column headers and by long-form display strings.
pub fn weekday_long_key(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "calendar-weekday-long-monday",
        Weekday::Tuesday => "calendar-weekday-long-tuesday",
        Weekday::Wednesday => "calendar-weekday-long-wednesday",
        Weekday::Thursday => "calendar-weekday-long-thursday",
        Weekday::Friday => "calendar-weekday-long-friday",
        Weekday::Saturday => "calendar-weekday-long-saturday",
        Weekday::Sunday => "calendar-weekday-long-sunday",
    }
}

/// Short (3-letter convention) weekday name key (e.g. "Mon"). Used for
/// the weekday header row inside Calendar.
pub fn weekday_short_key(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "calendar-weekday-short-monday",
        Weekday::Tuesday => "calendar-weekday-short-tuesday",
        Weekday::Wednesday => "calendar-weekday-short-wednesday",
        Weekday::Thursday => "calendar-weekday-short-thursday",
        Weekday::Friday => "calendar-weekday-short-friday",
        Weekday::Saturday => "calendar-weekday-short-saturday",
        Weekday::Sunday => "calendar-weekday-short-sunday",
    }
}

/// Narrow (1-letter) weekday name key — used in tight layouts.
pub fn weekday_narrow_key(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "calendar-weekday-narrow-monday",
        Weekday::Tuesday => "calendar-weekday-narrow-tuesday",
        Weekday::Wednesday => "calendar-weekday-narrow-wednesday",
        Weekday::Thursday => "calendar-weekday-narrow-thursday",
        Weekday::Friday => "calendar-weekday-narrow-friday",
        Weekday::Saturday => "calendar-weekday-narrow-saturday",
        Weekday::Sunday => "calendar-weekday-narrow-sunday",
    }
}
