//! Date/time infrastructure shared by `Calendar`, `DateEdit`, `TimeEdit`,
//! and `DateTimeEdit`.
//!
//! This module is the single point of contact with the underlying date/time
//! crate (`jiff` today). The widgets above never name `jiff` — they go
//! through the type aliases re-exported here. If a future swap is ever
//! needed, the widget code does not change.

pub mod locale;
pub mod month_names;
pub mod pattern;
pub mod types;
pub mod weekday_names;

pub use self::locale::{
    first_day_of_week_for_locale, format_pattern_for_locale, prefers_12_hour_clock,
};
pub use self::month_names::{month_long_key, month_short_key};
pub use self::pattern::{
    format_value, parse_value, segment_at_position, segments_layout, step_date_field,
    step_time_field, ParsedPattern, PatternError, PatternToken, SegmentKind,
};
pub use self::types::{Date, DateTime, Time, Weekday, YearMonth};
pub use self::weekday_names::{weekday_long_key, weekday_narrow_key, weekday_short_key};
