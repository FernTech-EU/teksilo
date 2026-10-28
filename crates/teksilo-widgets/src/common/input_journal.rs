// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A text surface's change journal, for the platform's input system: the
//! revisions and the merged range [`TextInputDocument::revisions`] and
//! [`TextInputDocument::text_changed_since`] answer.
//!
//! [`DocumentJournal`] hears the document's own change events
//! (`DocumentEvent::ContentsChanged`, its position and the characters it
//! removed and added) and logs each as a [`TextChanged`]; an event that
//! changes the text with no range (a flow element inserted or removed, a
//! load) leaves a gap, and a question reaching back across it is answered
//! `None`, which tells the bridge to compare the whole text. The cause is
//! [`ChangeCause::InputSystem`] for the events heard while the surface marks
//! a write of the input system's ([`DocumentJournal::input_system_write`]),
//! [`ChangeCause::Other`] for every other.
//!
//! Shared by `RichTextEditor` and `TextInputField`.
//!
//! [`TextInputDocument::revisions`]: teksilo_core::text_input_document::TextInputDocument::revisions
//! [`TextInputDocument::text_changed_since`]: teksilo_core::text_input_document::TextInputDocument::text_changed_since

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use teksilo_core::text_input_document::{ChangeCause, TextChanged};
use teksilo_text::text_document::{DocumentEvent, Subscription, TextDocument};

/// How many changes the log keeps; a question older than that is answered
/// `None`.
const KEPT: usize = 256;

/// The log of changes, by text revision.
#[derive(Debug, Default)]
pub(crate) struct JournalLog {
    /// The current text revision: the number of changes heard.
    revision: u64,
    /// The changes kept, each with the revision it made.
    changes: VecDeque<(u64, TextChanged)>,
    /// Up to this revision nothing can be answered (a change with no range,
    /// or one dropped from the log).
    unknown_until: u64,
}

impl JournalLog {
    /// The current text revision.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Log a change with its range.
    pub(crate) fn record(&mut self, change: TextChanged) {
        self.revision += 1;
        self.changes.push_back((self.revision, change));
        if self.changes.len() > KEPT
            && let Some((dropped, _)) = self.changes.pop_front()
        {
            self.unknown_until = dropped;
        }
    }

    /// Log a change whose range is not known.
    pub(crate) fn record_unknown(&mut self) {
        self.revision += 1;
        self.changes.clear();
        self.unknown_until = self.revision;
    }

    /// Every change since `revision`, merged; an empty change at 0 when
    /// there is none, `None` when the log cannot say.
    pub(crate) fn changed_since(&self, revision: u64) -> Option<TextChanged> {
        if revision < self.unknown_until || revision > self.revision {
            return None;
        }
        self.changes
            .iter()
            .filter(|(at, _)| *at > revision)
            .map(|(_, change)| *change)
            .reduce(merge)
            .or(Some(TextChanged {
                start: 0,
                old_end: 0,
                new_end: 0,
                cause: ChangeCause::InputSystem,
            }))
    }
}

/// `first` followed by `then` (in the text as `first` left it), as one
/// change of the text before `first`.
pub(crate) fn merge(first: TextChanged, then: TextChanged) -> TextChanged {
    let start = first.start.min(then.start);
    // Where `then`'s old end was in the text before `first`.
    let then_old_end = if then.old_end >= first.new_end {
        then.old_end - first.new_end + first.old_end
    } else {
        first.old_end
    };
    // Where `first`'s new end is after `then`.
    let first_new_end = if first.new_end >= then.old_end {
        first.new_end + then.new_end - then.old_end
    } else {
        then.new_end
    };
    TextChanged {
        start,
        old_end: first.old_end.max(then_old_end),
        new_end: first_new_end.max(then.new_end),
        cause: if first.cause == ChangeCause::InputSystem && then.cause == ChangeCause::InputSystem
        {
            ChangeCause::InputSystem
        } else {
            ChangeCause::Other
        },
    }
}

/// The journal of a surface's document, held beside it in the surface's
/// state for as long as it lives.
pub(crate) struct DocumentJournal {
    log: Arc<Mutex<JournalLog>>,
    input_system: Arc<AtomicBool>,
    _subscription: Subscription,
}

impl std::fmt::Debug for DocumentJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocumentJournal")
            .field("revision", &self.revision())
            .finish_non_exhaustive()
    }
}

impl DocumentJournal {
    /// Start hearing `document`.
    pub(crate) fn watching(document: &TextDocument) -> Self {
        let log = Arc::new(Mutex::new(JournalLog::default()));
        let input_system = Arc::new(AtomicBool::new(false));
        let subscription = {
            let log = log.clone();
            let input_system = input_system.clone();
            document.on_change(move |event| {
                let Ok(mut log) = log.lock() else {
                    return;
                };
                let cause = if input_system.load(Ordering::Relaxed) {
                    ChangeCause::InputSystem
                } else {
                    ChangeCause::Other
                };
                match event {
                    DocumentEvent::ContentsChanged {
                        position,
                        chars_removed,
                        chars_added,
                        ..
                    } => log.record(TextChanged {
                        start: position,
                        old_end: position + chars_removed,
                        new_end: position + chars_added,
                        cause,
                    }),
                    DocumentEvent::FlowElementsInserted { .. }
                    | DocumentEvent::FlowElementsRemoved { .. }
                    | DocumentEvent::DocumentReset => log.record_unknown(),
                    _ => {}
                }
            })
        };
        Self {
            log,
            input_system,
            _subscription: subscription,
        }
    }

    /// The current text revision.
    pub(crate) fn revision(&self) -> u64 {
        self.log.lock().map_or(0, |log| log.revision())
    }

    /// See [`JournalLog::changed_since`].
    pub(crate) fn changed_since(&self, revision: u64) -> Option<TextChanged> {
        self.log.lock().ok()?.changed_since(revision)
    }

    /// Run `write`, the changes it makes logged as the input system's.
    pub(crate) fn input_system_write<R>(&self, write: impl FnOnce() -> R) -> R {
        let outer = self.input_system.swap(true, Ordering::Relaxed);
        let result = write();
        self.input_system.store(outer, Ordering::Relaxed);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(start: usize, old_end: usize, new_end: usize) -> TextChanged {
        TextChanged {
            start,
            old_end,
            new_end,
            cause: ChangeCause::Other,
        }
    }

    /// Apply a change to `text` (by characters), taking the new text from
    /// `source` at the same positions.
    fn apply(text: &str, change: TextChanged, inserted: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out: String = chars[..change.start].iter().collect();
        out.push_str(inserted);
        out.extend(&chars[change.old_end..]);
        out
    }

    /// Merging every pair of edits of a short text agrees with applying them
    /// one after the other: the merged range, applied to the text before
    /// both, with the text after both in it, gives the text after both.
    #[test]
    fn a_merged_change_turns_the_first_text_into_the_last() {
        let base = "abcdefgh";
        let len = base.chars().count();
        let mut checked = 0;
        for s1 in 0..=len {
            for e1 in s1..=len {
                for n1 in 0..3 {
                    let first = change(s1, e1, s1 + n1);
                    let after_first = apply(base, first, &"X".repeat(n1));
                    let len1 = after_first.chars().count();
                    for s2 in (0..=len1).step_by(2) {
                        for e2 in s2..=len1.min(s2 + 3) {
                            for n2 in 0..3 {
                                let then = change(s2, e2, s2 + n2);
                                let after_both = apply(&after_first, then, &"Y".repeat(n2));
                                let merged = merge(first, then);
                                let middle: String = after_both
                                    .chars()
                                    .skip(merged.start)
                                    .take(merged.new_end - merged.start)
                                    .collect();
                                assert_eq!(
                                    apply(base, merged, &middle),
                                    after_both,
                                    "{first:?} then {then:?} merged as {merged:?}"
                                );
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1000);
    }

    #[test]
    fn the_log_answers_since_a_revision_and_forgets_what_it_cannot_say() {
        let mut log = JournalLog::default();
        assert_eq!(
            log.changed_since(0),
            Some(change(0, 0, 0)).map(|c| TextChanged {
                cause: ChangeCause::InputSystem,
                ..c
            })
        );
        log.record(change(3, 3, 5));
        log.record(TextChanged {
            cause: ChangeCause::InputSystem,
            ..change(0, 1, 1)
        });
        assert_eq!(log.revision(), 2);
        assert_eq!(
            log.changed_since(1).map(|c| c.cause),
            Some(ChangeCause::InputSystem)
        );
        let both = log.changed_since(0).unwrap();
        assert_eq!((both.start, both.old_end, both.new_end), (0, 3, 5));
        assert_eq!(
            both.cause,
            ChangeCause::Other,
            "one of them was not the input system's"
        );
        assert_eq!(log.changed_since(9), None, "a revision from the future");

        log.record_unknown();
        assert_eq!(log.changed_since(2), None, "across a change with no range");
        log.record(change(1, 1, 2));
        assert!(log.changed_since(3).is_some());

        for _ in 0..KEPT + 1 {
            log.record(change(0, 0, 1));
        }
        assert_eq!(log.changed_since(4), None, "dropped from the log");
    }

    #[test]
    fn a_documents_edits_are_logged_with_their_cause() {
        let document = TextDocument::new();
        document.set_plain_text("hello").unwrap();
        let journal = DocumentJournal::watching(&document);
        let start = journal.revision();
        let cursor = document.cursor_at(5);
        cursor.insert_text(" world").unwrap();
        assert!(journal.revision() > start);
        let typed = journal.changed_since(start).unwrap();
        assert_eq!((typed.start, typed.new_end), (5, 11));
        assert_eq!(typed.cause, ChangeCause::Other);

        let before = journal.revision();
        journal.input_system_write(|| cursor.insert_text("!").unwrap());
        let written = journal.changed_since(before).unwrap();
        assert_eq!(
            (written.start, written.old_end, written.new_end),
            (11, 11, 12)
        );
        assert_eq!(written.cause, ChangeCause::InputSystem);
    }
}
