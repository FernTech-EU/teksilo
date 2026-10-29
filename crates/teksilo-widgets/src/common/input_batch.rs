// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A text surface's batch of writes from the platform's input system: what
//! [`TextInputEditor::begin_input_batch`] and
//! [`TextInputEditor::end_input_batch`] open and close.
//!
//! Inside a batch the surface changes only its own state; what the
//! application can observe is queued here and handed back when the
//! outermost batch ends: the document's events (`TextDocument::on_change`,
//! held by `TextDocument::hold_events` from the outermost begin), the
//! insertions to report to `on_text_inserted`, and whether the caret moved
//! (its signals are written then). The bound text signal and the change
//! callbacks already wait for the next frame tick, so they need nothing here.
//!
//! The held events reach the surface's own change journal when they are
//! released, so the surface releases them as the input system's writes
//! ([`InputSystemMarker`](super::input_journal::InputSystemMarker)): inside a
//! batch only the input system writes.
//!
//! Shared by `RichTextEditor` and `TextInputField`.
//!
//! [`TextInputEditor::begin_input_batch`]: teksilo_core::text_input_document::TextInputEditor::begin_input_batch
//! [`TextInputEditor::end_input_batch`]: teksilo_core::text_input_document::TextInputEditor::end_input_batch

use std::cell::{Cell, RefCell};

use teksilo_text::text_document::{EventHold, TextDocument};

use crate::EditSource;

/// What a batch queued, handed back as it ends.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Deferred {
    /// The insertions to report, in order.
    pub(crate) arrivals: Vec<(EditSource, usize)>,
    /// The caret or the selection moved.
    pub(crate) cursor_moved: bool,
}

/// A surface's batch state. Interior mutability: it is reached through a
/// shared borrow of the surface's state, where the arrivals are reported.
#[derive(Debug, Default)]
pub(crate) struct InputBatch {
    depth: Cell<u32>,
    deferred: RefCell<Deferred>,
    /// The document's events, held from the outermost begin.
    hold: RefCell<Option<EventHold>>,
}

/// What the outermost batch hands back as it ends.
#[derive(Debug)]
pub(crate) struct Ended {
    /// The effects it queued.
    pub(crate) deferred: Deferred,
    /// The document's held events: dropping it delivers them, running the
    /// document's subscribers, so it is dropped with no borrow of the
    /// surface's state held.
    pub(crate) hold: Option<EventHold>,
}

impl InputBatch {
    /// Open a batch over `document`, or one more inside an open one.
    pub(crate) fn begin(&self, document: &TextDocument) {
        if self.depth.get() == 0 {
            *self.hold.borrow_mut() = Some(document.hold_events());
        }
        self.depth.set(self.depth.get() + 1);
    }

    /// Close a batch: what it queued, when it was the outermost.
    pub(crate) fn end(&self) -> Option<Ended> {
        let depth = self.depth.get();
        if depth == 0 {
            return None;
        }
        self.depth.set(depth - 1);
        (depth == 1).then(|| Ended {
            deferred: self.deferred.take(),
            hold: self.hold.take(),
        })
    }

    /// Whether a batch is open.
    pub(crate) fn is_open(&self) -> bool {
        self.depth.get() > 0
    }

    /// Queue an insertion's report while a batch is open; `false` (report it
    /// now) when none is.
    pub(crate) fn defer_arrival(&self, source: EditSource, chars: usize) -> bool {
        if !self.is_open() {
            return false;
        }
        self.deferred.borrow_mut().arrivals.push((source, chars));
        true
    }

    /// Note that the caret moved; `false` (write its signals now) when no
    /// batch is open.
    pub(crate) fn defer_cursor_moved(&self) -> bool {
        if !self.is_open() {
            return false;
        }
        self.deferred.borrow_mut().cursor_moved = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use teksilo_core::text_input_document::ChangeCause;
    use teksilo_text::text_document::DocumentEvent;

    use super::*;
    use crate::common::input_journal::DocumentJournal;

    #[test]
    fn effects_wait_for_the_outermost_batch() {
        let document = TextDocument::new();
        let batch = InputBatch::default();
        assert!(
            !batch.defer_arrival(EditSource::Ime, 1),
            "no batch: report now"
        );
        batch.begin(&document);
        batch.begin(&document);
        assert!(batch.defer_arrival(EditSource::Ime, 2));
        assert!(batch.defer_cursor_moved());
        assert!(batch.end().is_none(), "the inner batch hands nothing back");
        assert!(batch.defer_arrival(EditSource::Dictation, 3));
        let ended = batch.end().expect("the outermost batch ends");
        assert_eq!(
            ended.deferred,
            Deferred {
                arrivals: vec![(EditSource::Ime, 2), (EditSource::Dictation, 3)],
                cursor_moved: true,
            }
        );
        assert!(ended.hold.is_some());
        assert!(!batch.is_open());
        assert!(batch.end().is_none(), "an unbalanced end does nothing");
    }

    #[test]
    fn the_documents_events_wait_for_the_batch_and_are_the_input_systems() {
        let document = TextDocument::new();
        document.set_plain_text("abc").unwrap();
        let journal = DocumentJournal::watching(&document);
        let heard = Arc::new(Mutex::new(Vec::new()));
        let _subscription = {
            let heard = heard.clone();
            document.on_change(move |event| {
                if let DocumentEvent::ContentsChanged { position, .. } = event {
                    heard.lock().unwrap().push(position);
                }
            })
        };
        let before = journal.revision();
        let batch = InputBatch::default();
        batch.begin(&document);
        document.cursor_at(3).insert_text("d").unwrap();
        document.cursor_at(0).insert_text("x").unwrap();
        assert!(heard.lock().unwrap().is_empty(), "held");
        assert_eq!(journal.revision(), before, "the journal waits too");
        let ended = batch.end().unwrap();
        journal.marker().while_writing(|| drop(ended.hold));
        assert_eq!(*heard.lock().unwrap(), vec![3, 0], "in order");
        let changed = journal.changed_since(before).unwrap();
        assert_eq!((changed.start, changed.old_end, changed.new_end), (0, 3, 5));
        assert_eq!(changed.cause, ChangeCause::InputSystem);
    }
}
