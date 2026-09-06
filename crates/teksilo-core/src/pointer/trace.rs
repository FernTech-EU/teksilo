// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Opt-in input tracing.
//!
//! An input bug is a *sequence* bug: the sample that mattered is three packets
//! back, and by the time a widget misbehaves the evidence is gone. Rather than
//! reach for a logging framework (`teksilo-core` has seven dependencies and
//! intends to keep them), the input layer carries one environment-variable
//! switch and one macro.
//!
//! ```text
//! TEKSILO_TRACE_INPUT=samples   # every pointer and scroll sample
//! TEKSILO_TRACE_INPUT=gestures  # recognizer transitions and arbitration
//! TEKSILO_TRACE_INPUT=all       # both
//! ```
//!
//! A line looks like:
//!
//! ```text
//! [teksilo input] down PointerId(2) at Point { x: 120.0, y: 44.0 }
//! ```
//!
//! # Cost when off
//!
//! The variable is read **once**, through a [`OnceLock`], so a
//! [`trace_enabled`] call after the first is one relaxed load and a comparison.
//! More importantly [`trace_input!`](crate::trace_input) guards its arguments: a trace call in a
//! hot path formats nothing, allocates nothing and evaluates none of its
//! argument expressions unless the level is on. That is what makes it
//! acceptable to leave a trace call on the per-sample path.

use std::sync::OnceLock;

/// How much input tracing is on.
///
/// Ordered by inclusion: [`All`](Self::All) implies both of the others, so a
/// call site asks "is *my* level on?" rather than matching every combination.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TraceLevel {
    /// Nothing is traced. The default, and what an unset or unrecognised
    /// `TEKSILO_TRACE_INPUT` means.
    #[default]
    Off,
    /// Raw samples entering the tree.
    Samples,
    /// Recognizer transitions, arbitration decisions and cancellations.
    Gestures,
    /// Both.
    All,
}

/// The level parsed from the environment, resolved once per process.
static LEVEL: OnceLock<TraceLevel> = OnceLock::new();

/// Parse a `TEKSILO_TRACE_INPUT` value. Unrecognised values are
/// [`TraceLevel::Off`] — a typo must not silently enable a different level, and
/// must not be an error either.
fn parse_level(raw: &str) -> TraceLevel {
    match raw.trim().to_ascii_lowercase().as_str() {
        "samples" | "sample" => TraceLevel::Samples,
        "gestures" | "gesture" => TraceLevel::Gestures,
        "all" | "1" | "true" => TraceLevel::All,
        _ => TraceLevel::Off,
    }
}

/// The active trace level.
pub fn trace_level() -> TraceLevel {
    *LEVEL.get_or_init(|| {
        std::env::var("TEKSILO_TRACE_INPUT")
            .ok()
            .map(|raw| parse_level(&raw))
            .unwrap_or_default()
    })
}

/// Whether `level` is currently being traced.
///
/// [`TraceLevel::All`] enables both categories; asking for
/// [`TraceLevel::Off`] is always `false` (there is nothing to trace at that
/// level), so a caller cannot accidentally turn a guard into a no-op by
/// passing it.
pub fn trace_enabled(level: TraceLevel) -> bool {
    match (trace_level(), level) {
        (TraceLevel::Off, _) | (_, TraceLevel::Off) => false,
        (TraceLevel::All, _) => true,
        (active, wanted) => active == wanted,
    }
}

/// Emit one trace line if the given level is on.
///
/// ```ignore
/// trace_input!(Samples, "down {:?} at {:?}", id, position);
/// ```
///
/// The first argument names a [`TraceLevel`] variant without its path. The rest
/// is an ordinary `format!` argument list — and it is **not evaluated** unless
/// the level is on, so an expensive `{:?}` on a large structure costs nothing
/// in a normal run.
#[macro_export]
macro_rules! trace_input {
    ($level:ident, $($arg:tt)*) => {
        if $crate::pointer::trace::trace_enabled($crate::pointer::trace::TraceLevel::$level) {
            eprintln!("[teksilo input] {}", format_args!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn levels_parse_leniently_and_fail_closed() {
        assert_eq!(parse_level("samples"), TraceLevel::Samples);
        assert_eq!(parse_level("  SAMPLES "), TraceLevel::Samples);
        assert_eq!(parse_level("gesture"), TraceLevel::Gestures);
        assert_eq!(parse_level("all"), TraceLevel::All);
        assert_eq!(parse_level("verbose"), TraceLevel::Off);
        assert_eq!(parse_level(""), TraceLevel::Off);
    }

    #[test]
    fn off_is_the_default() {
        assert_eq!(TraceLevel::default(), TraceLevel::Off);
    }

    /// The whole point of the macro's guard: with tracing off, the argument
    /// expressions must never run. A trace call on the per-sample path that
    /// formatted its arguments regardless would be a per-sample allocation.
    ///
    /// The test suite runs without `TEKSILO_TRACE_INPUT` set, so this also
    /// pins the "unset means off" default.
    #[test]
    fn tracing_off_does_not_evaluate_its_arguments() {
        assert_eq!(
            trace_level(),
            TraceLevel::Off,
            "the suite must run with TEKSILO_TRACE_INPUT unset"
        );

        let evaluated = Cell::new(0u32);
        let bump = || {
            evaluated.set(evaluated.get() + 1);
            "argument"
        };

        trace_input!(Samples, "{}", bump());
        trace_input!(Gestures, "{} {}", bump(), bump());

        assert_eq!(
            evaluated.get(),
            0,
            "arguments must be guarded, not formatted"
        );
    }

    #[test]
    fn asking_for_off_is_never_enabled() {
        assert!(!trace_enabled(TraceLevel::Off));
    }
}
