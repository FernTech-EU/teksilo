//! `SpinValue` trait — abstracts numeric primitives for `SpinBox`.
//!
//! Implemented for `i32`, `i64`, `u32`, `u64`, `usize`, `f32`, `f64`.
//! The trait is **sealed** via the private [`sealed::Sealed`]
//! supertrait: only the primitive numeric types above implement it,
//! and downstream crates cannot add their own implementations.
//! Callers that need a custom value type (fixed-point decimals,
//! durations, currencies, …) should wrap `SpinBox<i64>` /
//! `SpinBox<f64>` and use
//! [`text_from_value`](super::SpinBox::text_from_value) /
//! [`value_from_text`](super::SpinBox::value_from_text) for the
//! presentation layer.

use std::fmt::Debug;

mod sealed {
    /// Private super-trait used to seal [`SpinValue`] against
    /// downstream implementations. Only the primitive types
    /// enumerated in the parent module implement it.
    pub trait Sealed {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
    impl Sealed for usize {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

/// Numeric primitive that a [`SpinBox`](super::SpinBox) can hold.
///
/// Sealed: only the primitive integer and floating-point types
/// implement this. See the [module docs](self) for the rationale.
///
/// Implementations must provide lossless parsing and round-trip
/// formatting (`parse(format(v, d)) == Some(v)` for any finite value
/// `v` and decimals `d`). Arithmetic is saturating so clamping into
/// `[min, max]` after a step cannot overflow.
pub trait SpinValue: sealed::Sealed + Copy + PartialOrd + Debug + 'static {
    /// Lossless widening to `f64`. Used for AccessKit's numeric
    /// value / min / max / step properties and for
    /// [`StepType::Adaptive`](super::StepType::Adaptive) decimal
    /// analysis.
    fn to_f64(self) -> f64;

    /// Narrowing from `f64` with saturation at the type's full
    /// range. For integers the conversion truncates toward zero,
    /// matching Rust's `as` conversion semantics.
    fn from_f64_saturating(v: f64) -> Self;

    /// Parse a user-entered string. Leading/trailing whitespace is
    /// ignored. Returns `None` for syntactically invalid input
    /// (but NOT for out-of-range values — the SpinBox clamps
    /// separately so users can type past the bound and see the
    /// reformatted clamped result after blur).
    fn parse(s: &str) -> Option<Self>;

    /// Format for display.
    ///
    /// For integer types, `decimals` is ignored. For floats, the
    /// value is rendered with exactly `decimals` digits after the
    /// decimal point — no scientific notation, no thousands
    /// separator. Formatter closures on `SpinBox`
    /// ([`text_from_value`](super::SpinBox::text_from_value))
    /// override this.
    fn format(self, decimals: u8) -> String;

    /// Saturating addition. Out-of-type-range results clamp at
    /// `MAX` (or `MIN` for negative overflow on signed types).
    fn saturating_add(self, rhs: Self) -> Self;

    /// Saturating subtraction. See [`saturating_add`](Self::saturating_add).
    fn saturating_sub(self, rhs: Self) -> Self;

    /// Saturating multiplication by a positive integer. Used for
    /// `page_step = multiplier × single_step` when the caller
    /// omits a page step.
    fn saturating_mul_u32(self, rhs: u32) -> Self;

    /// Whether this type has integer semantics (no fractional
    /// component, no decimal separator in the default
    /// [`format`](Self::format) path). Controls the default
    /// character filter and whether `decimals` has any effect.
    fn is_integer() -> bool;

    /// Default per-character input filter for the editable field.
    /// Admits digits and, for signed types, `-`; float types also
    /// admit `.`, `+`, `e`, `E`. Callers can override the whole
    /// filter on the `SpinBox` builder.
    fn is_valid_input_char(c: char) -> bool;

    /// Clamp into an inclusive range. Falls through to
    /// `PartialOrd`.
    fn clamp_value(self, min: Self, max: Self) -> Self {
        if self < min {
            min
        } else if self > max {
            max
        } else {
            self
        }
    }
}

// ── Integer implementations ─────────────────────────────────────────

macro_rules! impl_spin_value_int {
    ($t:ty, signed = $signed:expr) => {
        impl SpinValue for $t {
            fn to_f64(self) -> f64 { self as f64 }
            fn from_f64_saturating(v: f64) -> Self {
                if v.is_nan() {
                    return 0;
                }
                if v <= <$t>::MIN as f64 { return <$t>::MIN; }
                if v >= <$t>::MAX as f64 { return <$t>::MAX; }
                v as Self
            }
            fn parse(s: &str) -> Option<Self> {
                s.trim().parse::<Self>().ok()
            }
            fn format(self, _decimals: u8) -> String {
                self.to_string()
            }
            fn saturating_add(self, rhs: Self) -> Self {
                <$t>::saturating_add(self, rhs)
            }
            fn saturating_sub(self, rhs: Self) -> Self {
                <$t>::saturating_sub(self, rhs)
            }
            fn saturating_mul_u32(self, rhs: u32) -> Self {
                // Widen to i128 so the multiply is always exact,
                // then saturate into the target type's range.
                let wide = (self as i128) * (rhs as i128);
                if wide <= <$t>::MIN as i128 { <$t>::MIN }
                else if wide >= <$t>::MAX as i128 { <$t>::MAX }
                else { wide as Self }
            }
            fn is_integer() -> bool { true }
            fn is_valid_input_char(c: char) -> bool {
                if $signed {
                    c.is_ascii_digit() || c == '-'
                } else {
                    c.is_ascii_digit()
                }
            }
        }
    };
}

impl_spin_value_int!(i32, signed = true);
impl_spin_value_int!(i64, signed = true);
impl_spin_value_int!(u32, signed = false);
impl_spin_value_int!(u64, signed = false);
impl_spin_value_int!(usize, signed = false);

// ── Float implementations ───────────────────────────────────────────

macro_rules! impl_spin_value_float {
    ($t:ty) => {
        impl SpinValue for $t {
            fn to_f64(self) -> f64 { self as f64 }
            fn from_f64_saturating(v: f64) -> Self {
                if v.is_nan() {
                    return 0.0;
                }
                // `as` casts between floats already saturate to
                // ±INFINITY on overflow and produce the nearest
                // representable value otherwise, so no manual
                // clamping is needed.
                v as Self
            }
            fn parse(s: &str) -> Option<Self> {
                s.trim().parse::<Self>().ok().filter(|f: &Self| f.is_finite())
            }
            fn format(self, decimals: u8) -> String {
                // `{:.N$}` formats with exactly N decimal places
                // and no separator — matches Qt's default
                // `QDoubleSpinBox` output for non-grouping locales.
                format!("{:.*}", decimals as usize, self)
            }
            fn saturating_add(self, rhs: Self) -> Self {
                let r = self + rhs;
                if r.is_finite() {
                    r
                } else if r.is_sign_positive() {
                    <$t>::MAX
                } else {
                    <$t>::MIN
                }
            }
            fn saturating_sub(self, rhs: Self) -> Self {
                let r = self - rhs;
                if r.is_finite() {
                    r
                } else if r.is_sign_positive() {
                    <$t>::MAX
                } else {
                    <$t>::MIN
                }
            }
            fn saturating_mul_u32(self, rhs: u32) -> Self {
                let r = self * (rhs as Self);
                if r.is_finite() {
                    r
                } else if r.is_sign_positive() {
                    <$t>::MAX
                } else {
                    <$t>::MIN
                }
            }
            fn is_integer() -> bool { false }
            fn is_valid_input_char(c: char) -> bool {
                c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E'
            }
        }
    };
}

impl_spin_value_float!(f32);
impl_spin_value_float!(f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i32_round_trip() {
        assert_eq!(<i32 as SpinValue>::parse("42"), Some(42));
        assert_eq!(<i32 as SpinValue>::parse("-1"), Some(-1));
        assert_eq!(<i32 as SpinValue>::parse(" 7 "), Some(7));
        assert_eq!(<i32 as SpinValue>::parse("abc"), None);
        assert_eq!((42_i32).format(0), "42");
    }

    #[test]
    fn u32_rejects_negative() {
        assert_eq!(<u32 as SpinValue>::parse("-5"), None);
        assert!(!<u32 as SpinValue>::is_valid_input_char('-'));
    }

    #[test]
    fn f64_formats_with_decimals() {
        assert_eq!((std::f64::consts::PI).format(2), "3.14");
        assert_eq!((std::f64::consts::PI).format(4), "3.1416");
        assert_eq!((0.1_f64 + 0.2).format(1), "0.3");
    }

    #[test]
    fn f64_parse_rejects_nan_and_infinity() {
        assert_eq!(<f64 as SpinValue>::parse("nan"), None);
        assert_eq!(<f64 as SpinValue>::parse("inf"), None);
        assert_eq!(<f64 as SpinValue>::parse("3.14"), Some(3.14));
    }

    #[test]
    fn integer_saturating_add_clamps() {
        let v = i32::MAX - 1;
        assert_eq!(SpinValue::saturating_add(v, 10_i32), i32::MAX);
        assert_eq!(SpinValue::saturating_sub(i32::MIN, 1_i32), i32::MIN);
    }

    #[test]
    fn page_step_multiply_saturates() {
        assert_eq!(SpinValue::saturating_mul_u32(1_000_000_i32, 1_000_000), i32::MAX);
    }

    #[test]
    fn is_integer_flag() {
        assert!(<i32 as SpinValue>::is_integer());
        assert!(<u64 as SpinValue>::is_integer());
        assert!(!<f32 as SpinValue>::is_integer());
        assert!(!<f64 as SpinValue>::is_integer());
    }

    #[test]
    fn clamp_value() {
        assert_eq!((42_i32).clamp_value(0, 100), 42);
        assert_eq!((150_i32).clamp_value(0, 100), 100);
        assert_eq!((-5_i32).clamp_value(0, 100), 0);
    }
}
