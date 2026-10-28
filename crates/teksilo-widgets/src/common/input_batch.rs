// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A text surface's batch of writes from the platform's input system: what
//! [`TextInputEditor::begin_input_batch`] and
//! [`TextInputEditor::end_input_batch`] open and close.
//!
//! Inside a batch the surface changes only its own state; what the
//! application can observe is queued here and handed back when the
//! outermost batch ends: the insertions to report to `on_text_inserted`, and
//! whether the caret moved (its signals are written then). The bound text
//! signal and the change callbacks already wait for the next frame tick, so
//! they need nothing here.
//!
//! The document's own subscribers (`TextDocument::on_change`) still hear each
//! edit as it is made: text-document has no way to hold its events.
//!
//! Shared by `RichTextEditor` and `TextInputField`.
//!
//! [`TextInputEditor::begin_input_batch`]: teksilo_core::text_input_document::TextInputEditor::begin_input_batch
//! [`TextInputEditor::end_input_batch`]: teksilo_core::text_input_document::TextInputEditor::end_input_batch

use std::cell::{Cell, RefCell};

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
}

impl InputBatch {
    /// Open a batch, or one more inside an open one.
    pub(crate) fn begin(&self) {
        self.depth.set(self.depth.get() + 1);
    }

    /// Close a batch: what it queued, when it was the outermost.
    pub(crate) fn end(&self) -> Option<Deferred> {
        let depth = self.depth.get();
        if depth == 0 {
            return None;
        }
        self.depth.set(depth - 1);
        (depth == 1).then(|| self.deferred.take())
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
    use super::*;

    #[test]
    fn effects_wait_for_the_outermost_batch() {
        let batch = InputBatch::default();
        assert!(
            !batch.defer_arrival(EditSource::Ime, 1),
            "no batch: report now"
        );
        batch.begin();
        batch.begin();
        assert!(batch.defer_arrival(EditSource::Ime, 2));
        assert!(batch.defer_cursor_moved());
        assert_eq!(batch.end(), None, "the inner batch hands nothing back");
        assert!(batch.defer_arrival(EditSource::Dictation, 3));
        assert_eq!(
            batch.end(),
            Some(Deferred {
                arrivals: vec![(EditSource::Ime, 2), (EditSource::Dictation, 3)],
                cursor_moved: true,
            })
        );
        assert!(!batch.is_open());
        assert_eq!(batch.end(), None, "an unbalanced end does nothing");
    }
}
