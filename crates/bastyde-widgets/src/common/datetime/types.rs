//! Stable type aliases for the date/time types used across the four
//! datetime widgets. Routes through `jiff` today; swap-friendly because
//! the widgets never name `jiff` directly.

pub use jiff::civil::{Date, DateTime, Time, Weekday};

/// Year + month pair, with no day component. Used by `Calendar` to track
/// the visible month independently of the current selection.
///
/// Stored as a `Date` anchored on the first of the month; the widget
/// never reads the day field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct YearMonth {
    year: i16,
    month: i8,
}

impl YearMonth {
    /// Construct a `YearMonth`. Year clamps to jiff's `-9999..=9999`
    /// range; month clamps to `1..=12`.
    pub fn new(year: i16, month: i8) -> Self {
        let year = year.clamp(-9999, 9999);
        let month = month.clamp(1, 12);
        Self { year, month }
    }

    /// Year as i16. Range matches jiff: `-9999..=9999`.
    pub fn year(self) -> i16 {
        self.year
    }

    /// Month as i8 in `1..=12`.
    pub fn month(self) -> i8 {
        self.month
    }

    /// First day of this month as a `Date`.
    pub fn first_day(self) -> Date {
        Date::constant(self.year, self.month, 1)
    }

    /// Last day of this month as a `Date`.
    pub fn last_day(self) -> Date {
        self.first_day().last_of_month()
    }

    /// `YearMonth` containing the given date.
    pub fn from_date(d: Date) -> Self {
        Self {
            year: d.year(),
            month: d.month(),
        }
    }

    /// The next calendar month (wraps year on December → January).
    pub fn next_month(self) -> Self {
        if self.month == 12 {
            Self::new(self.year.saturating_add(1), 1)
        } else {
            Self::new(self.year, self.month + 1)
        }
    }

    /// The previous calendar month (wraps year on January → December).
    pub fn prev_month(self) -> Self {
        if self.month == 1 {
            Self::new(self.year.saturating_sub(1), 12)
        } else {
            Self::new(self.year, self.month - 1)
        }
    }

    /// `n` months later (negative for earlier).
    pub fn offset_months(self, n: i32) -> Self {
        let total = self.year as i32 * 12 + (self.month as i32 - 1) + n;
        let year = (total.div_euclid(12)).clamp(-9999, 9999) as i16;
        let month = (total.rem_euclid(12) + 1) as i8;
        Self::new(year, month)
    }
}

/// `Weekday::from_monday_zero_offset` clamped to `0..=6` and
/// infallible — the rest of the date code only ever traffics in valid
/// offsets, and a panic here would point at a bug, not a user error.
pub fn weekday_from_monday_zero(offset: i8) -> Weekday {
    Weekday::from_monday_zero_offset(offset.rem_euclid(7))
        .expect("monday-zero offset already in 0..=6")
}

/// Today's date in the system's local time zone.
///
/// Single source of truth for "today" across the calendar / date
/// widgets — used by today-ring rendering, the today-button, the `T`
/// keyboard shortcut, and any default-when-no-value selection. Falls
/// back to a fixed sentinel only if the platform refuses to give us a
/// local zone (jiff returns `Zoned::now` infallibly on Linux/macOS/
/// Windows; other platforms with broken tzdb fall back to UTC then to
/// the sentinel).
pub fn today_local() -> Date {
    jiff::Zoned::now().date()
}
