//! Validation pipeline shared by `TextInputField` and the composites
//! that render its feedback (`TextInput`, `DateEdit`, `TimeEdit`,
//! `DateTimeEdit`).
//!
//! # Pipeline shape
//!
//! 1. The user commits the field (Enter, Tab-out, focus loss).
//! 2. The field's [`ValidatorFn`] runs on the current text.
//! 3. The validator returns one of three outcomes:
//!    - [`ValidationOutcome::Valid`] — keep the text as-is.
//!    - [`ValidationOutcome::Corrected`] — replace the text with a
//!      normalized form, surface a polite announcement.
//!    - [`ValidationOutcome::Invalid`] — revert to the pre-edit value,
//!      surface an assertive error.
//! 4. The field writes the resolved [`ValidationFeedback`] to its
//!    published signal so composites can render the inline strip.
//!
//! # Why `ValidationOutcome` and `ValidationFeedback` are separate
//!
//! The outcome is what the validator *returns* (input → categorized
//! result). The feedback is what observers *see* (categorized result
//! plus a `since` timestamp for time-based UI decay). Splitting the
//! types keeps validators stateless and lets the field own the
//! lifecycle (decay, reset on re-edit) without leaking timing concerns
//! into validator code.

use std::rc::Rc;
use std::time::Instant;

/// What a validator returns for a given commit attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// Input is valid as typed. The field commits unchanged and the
    /// feedback signal flips to [`ValidationFeedback::Valid`].
    Valid,
    /// Input was accepted after normalization. The field replaces its
    /// text with `corrected`, the bound `Signal<String>` observes the
    /// new value, and the feedback signal carries `message` for
    /// composites to surface as a polite announcement.
    ///
    /// Use for clamping, completion, and reformat. Examples:
    /// `"12/50/2026"` → `Corrected { corrected: "12/31/2026", … }`
    /// for "day clamped to month length"; `"2026"` →
    /// `Corrected { corrected: "2026-01-01", … }` for "year-only
    /// completed to start of year".
    Corrected { corrected: String, message: String },
    /// Input is rejected. The field reverts its text to the pre-edit
    /// value and the feedback signal carries `message` for composites
    /// to surface as an assertive error.
    Invalid { message: String },
}

/// What composites render. Distinct from [`ValidationOutcome`]: the
/// outcome is the validator's return value (no time concept); the
/// feedback adds a `since` instant so the visual layer can decay an
/// auto-correction announcement after a window without re-running the
/// validator.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ValidationFeedback {
    /// No commit has happened yet, or the user is editing again after
    /// a previous outcome (typing always clears prior feedback).
    #[default]
    Pristine,
    /// Last commit returned [`ValidationOutcome::Valid`]. Composites
    /// typically render this identically to `Pristine` — the
    /// distinction matters for tests and for callers that want to
    /// signal "yes, it's confirmed valid" with a checkmark.
    Valid,
    /// Last commit returned [`ValidationOutcome::Corrected`]. `since`
    /// is the wall-clock instant the correction was applied; composites
    /// use it to decay the visual after `corrected_pulse_duration_ms`
    /// from the theme.
    Corrected { message: String, since: Instant },
    /// Last commit returned [`ValidationOutcome::Invalid`]. Persists
    /// until the user edits again or an external `Pristine` reset.
    Invalid { message: String },
}

impl ValidationFeedback {
    /// Convenience: is this state currently signalling an error?
    pub fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    /// Convenience: was the last commit auto-corrected?
    pub fn is_corrected(&self) -> bool {
        matches!(self, Self::Corrected { .. })
    }

    /// Human-readable message, if any.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Corrected { message, .. } | Self::Invalid { message } => Some(message),
            _ => None,
        }
    }
}

/// The closure signature the field calls on every commit. Stateless —
/// the field owns the pre-edit text and reverts on `Invalid` itself.
pub type ValidatorFn = Rc<dyn Fn(&str) -> ValidationOutcome>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_is_invalid_helper() {
        assert!(!ValidationFeedback::Pristine.is_invalid());
        assert!(!ValidationFeedback::Valid.is_invalid());
        assert!(
            !ValidationFeedback::Corrected {
                message: "x".into(),
                since: Instant::now(),
            }
            .is_invalid()
        );
        assert!(
            ValidationFeedback::Invalid {
                message: "x".into()
            }
            .is_invalid()
        );
    }

    #[test]
    fn feedback_message_accessor() {
        assert_eq!(ValidationFeedback::Pristine.message(), None);
        assert_eq!(ValidationFeedback::Valid.message(), None);
        assert_eq!(
            ValidationFeedback::Invalid {
                message: "bad".into()
            }
            .message(),
            Some("bad")
        );
        assert_eq!(
            ValidationFeedback::Corrected {
                message: "fixed".into(),
                since: Instant::now(),
            }
            .message(),
            Some("fixed")
        );
    }
}
