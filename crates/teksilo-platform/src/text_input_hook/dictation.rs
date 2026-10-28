// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A window's dictation session: the half that needs no platform.
//!
//! Text dictation commits looks like the end of any other composition (on
//! macOS 27: marked text, then a plain string), so whether it is dictation is
//! read from a session the platform opens and closes around it. Two orders
//! were measured on macOS 27 for the end of a dictation:
//!
//! - it ends on silence: the last sentence is committed, then the end is
//!   reported (`_dictationDidEnd:`) with nothing marked;
//! - the user stops it (the dictation key): the end is reported about 50 ms
//!   **before** the last sentence is committed, while that sentence is still
//!   marked.
//!
//! Closing the session at the reported end would tag that last sentence as a
//! plain composition. So an end that comes while the window still holds
//! marked text does not close the session: it leaves it
//! [`Ending`](DictationSession::Ending) until that composition is over (it
//! is committed, unmarked, emptied, or a key is pressed), and the commit is
//! still dictation.

#![cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS hook tracks dictation, for now")
)]

/// Where a window's dictation stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DictationSession {
    /// No dictation.
    #[default]
    Closed,
    /// Dictation is running: what it commits is dictation.
    Open,
    /// Dictation has ended while its last words were still marked: the
    /// commit that ends that composition is dictation too, and closes it.
    Ending,
}

/// Something that may open or close a window's dictation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DictationEvent {
    /// The platform reports that dictation has started on the window (macOS:
    /// the private `_dictationDidStart:`).
    Started,
    /// The application asked dictation to start (macOS: `NSApplication`'s
    /// `startDictation:`, the Edit menu's Start Dictation). Dictation may
    /// never begin: on macOS 27 it fired, then gave up, while the view had no
    /// document to offer. So it opens the session only where starts are not
    /// reported (`starts_reported` is false: on macOS, where the
    /// `_dictationDidStart:` hook could not be installed); where they are,
    /// the session waits for the dictation that actually begins.
    StartRequested { starts_reported: bool },
    /// The platform reports that dictation has ended (macOS: the private
    /// `_dictationDidEnd:`); `marked` is whether the window still holds
    /// marked text then.
    Ended { marked: bool },
    /// The application asked dictation to stop (macOS: `stopDictation:`);
    /// `marked` as for [`Ended`](Self::Ended).
    StopRequested { marked: bool },
    /// The window's composition is over: its text was committed outside a
    /// key press, the input method unmarked it or marked an empty string, or
    /// a key was pressed. Ends an [`Ending`](DictationSession::Ending)
    /// session; a running one goes on (the user may type while dictating).
    CompositionOver,
}

impl DictationSession {
    /// Whether text committed now is dictation: while the session is open,
    /// and while it is ending.
    pub(crate) fn is_dictating(self) -> bool {
        !matches!(self, Self::Closed)
    }

    /// The session after `event`.
    #[must_use]
    pub(crate) fn after(self, event: DictationEvent) -> Self {
        match event {
            DictationEvent::Started => Self::Open,
            DictationEvent::StartRequested { starts_reported } => {
                if starts_reported {
                    self
                } else {
                    Self::Open
                }
            }
            DictationEvent::Ended { marked } | DictationEvent::StopRequested { marked } => {
                if marked && self.is_dictating() {
                    Self::Ending
                } else {
                    Self::Closed
                }
            }
            DictationEvent::CompositionOver => match self {
                Self::Ending => Self::Closed,
                other => other,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DictationEvent::{CompositionOver, Ended, StartRequested, Started, StopRequested};
    use super::DictationSession::{Closed, Ending, Open};
    use super::*;
    use crate::text_input_hook::{CommitSignals, classify};
    use teksilo_core::ImeOrigin;

    /// The session after `events`, from closed.
    fn run(events: &[DictationEvent]) -> DictationSession {
        events
            .iter()
            .fold(DictationSession::Closed, |session, &event| {
                session.after(event)
            })
    }

    /// The origin of a composition committed out of band with `session`.
    fn commit(session: DictationSession) -> ImeOrigin {
        classify(CommitSignals {
            had_marked_text: true,
            dictation_session: session.is_dictating(),
            ..CommitSignals::default()
        })
    }

    #[test]
    fn every_state_after_every_event() {
        let events = [
            Started,
            StartRequested {
                starts_reported: true,
            },
            StartRequested {
                starts_reported: false,
            },
            Ended { marked: false },
            Ended { marked: true },
            StopRequested { marked: false },
            StopRequested { marked: true },
            CompositionOver,
        ];
        // One row per state, one column per event above.
        let table = [
            (
                Closed,
                [Open, Closed, Open, Closed, Closed, Closed, Closed, Closed],
            ),
            (
                Open,
                [Open, Open, Open, Closed, Ending, Closed, Ending, Open],
            ),
            (
                Ending,
                [Open, Ending, Open, Closed, Ending, Closed, Ending, Closed],
            ),
        ];
        for (state, expected) in table {
            for (event, expected) in events.iter().zip(expected) {
                assert_eq!(state.after(*event), expected, "{state:?} after {event:?}");
            }
        }
        assert!(!Closed.is_dictating());
        assert!(Open.is_dictating());
        assert!(Ending.is_dictating());
    }

    #[test]
    fn dictation_that_ends_on_silence_commits_then_closes() {
        // macOS 27: _dictationDidStart:, the marked words, the commit, then
        // _dictationDidEnd: with nothing marked.
        let session = run(&[Started]);
        assert_eq!(commit(session), ImeOrigin::Dictation);
        let session = session.after(CompositionOver);
        assert_eq!(session, Open, "the commit alone does not end dictation");
        let session = session.after(Ended { marked: false });
        assert_eq!(session, Closed);
        assert_eq!(
            commit(session),
            ImeOrigin::Composition,
            "a later IME commit"
        );
    }

    #[test]
    fn dictation_the_user_stops_still_commits_its_last_sentence_as_dictation() {
        // macOS 27, the dictation key: _dictationDidStart:, the marked words,
        // _dictationDidEnd: while the last sentence is still marked, then
        // about 50 ms later its commit.
        let session = run(&[Started, Ended { marked: true }]);
        assert_eq!(session, Ending);
        assert_eq!(commit(session), ImeOrigin::Dictation, "the last sentence");
        let session = session.after(CompositionOver);
        assert_eq!(session, Closed, "closed once committed");
        assert_eq!(
            commit(session),
            ImeOrigin::Composition,
            "a later IME commit"
        );
    }

    #[test]
    fn a_stop_request_with_the_last_words_marked_waits_for_them_too() {
        let session = run(&[Started, StopRequested { marked: true }]);
        assert_eq!(commit(session), ImeOrigin::Dictation);
        assert_eq!(session.after(CompositionOver), Closed);
    }

    #[test]
    fn an_ending_session_closes_when_its_composition_goes_some_other_way() {
        // Unmarked, emptied, or a key pressed: each is CompositionOver.
        let session = run(&[Started, Ended { marked: true }, CompositionOver]);
        assert_eq!(session, Closed);
        assert_eq!(
            commit(session),
            ImeOrigin::Composition,
            "nothing left of the dictation to commit"
        );
    }

    #[test]
    fn an_end_with_no_session_opens_nothing() {
        assert_eq!(run(&[Ended { marked: true }]), Closed);
        assert_eq!(run(&[StopRequested { marked: true }]), Closed);
    }

    #[test]
    fn a_start_request_that_comes_to_nothing_leaves_no_session() {
        // Round 1 on macOS 27: Start Dictation, then nothing, then an emoji.
        let session = run(&[StartRequested {
            starts_reported: true,
        }]);
        let origin = classify(CommitSignals {
            dictation_session: session.is_dictating(),
            ..CommitSignals::default()
        });
        assert_eq!(origin, ImeOrigin::Insertion, "the emoji is not dictation");
    }

    #[test]
    fn where_starts_are_not_reported_the_request_opens_the_session() {
        let session = run(&[StartRequested {
            starts_reported: false,
        }]);
        assert_eq!(commit(session), ImeOrigin::Dictation);
    }
}
