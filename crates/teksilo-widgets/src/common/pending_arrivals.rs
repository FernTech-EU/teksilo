// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The characters a text surface has queued but not inserted yet, counted per
//! [`EditSource`].
//!
//! Typing and the input system's commits push into one pending string that the
//! frame tick inserts as a unit, so the string alone cannot say which route
//! each character came down. A surface notes the count beside each push and
//! flushes the tally where it inserts the batch: what is reported is what was
//! written, and each route is reported apart.
//!
//! Shared by `RichTextEditor` and `TextInputField`, which batch the same way.

use crate::EditSource;

/// Per-source count of the characters waiting in a surface's pending batch.
///
/// Reset by [`flush`](Self::flush), which the surface calls where it inserts
/// the batch, and only there.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct PendingArrivals {
    /// One entry per source in the batch, in the order each first arrived, so
    /// a batch is reported in the order it was written.
    counts: Vec<(EditSource, usize)>,
}

impl PendingArrivals {
    /// `chars` characters just joined the pending batch through `source`.
    ///
    /// Zero notes nothing: there is no such thing as zero characters arriving.
    pub(crate) fn note(&mut self, source: EditSource, chars: usize) {
        if chars == 0 {
            return;
        }
        match self.counts.iter_mut().find(|(seen, _)| *seen == source) {
            Some((_, count)) => *count += chars,
            None => self.counts.push((source, chars)),
        }
    }

    /// The batch was inserted: hand `report` each source's count, once, in the
    /// order the sources arrived, and start the next batch empty.
    pub(crate) fn flush(&mut self, mut report: impl FnMut(EditSource, usize)) {
        for (source, chars) in self.counts.drain(..) {
            report(source, chars);
        }
    }

    /// Whether nothing is waiting.
    pub(crate) fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flushed(tally: &mut PendingArrivals) -> Vec<(EditSource, usize)> {
        let mut seen = Vec::new();
        tally.flush(|source, chars| seen.push((source, chars)));
        seen
    }

    #[test]
    fn an_empty_tally_reports_nothing() {
        let mut tally = PendingArrivals::default();
        assert!(tally.is_empty());
        assert!(flushed(&mut tally).is_empty());
    }

    #[test]
    fn counts_add_up_per_source() {
        let mut tally = PendingArrivals::default();
        tally.note(EditSource::Keyboard, 2);
        tally.note(EditSource::Keyboard, 3);
        assert_eq!(flushed(&mut tally), vec![(EditSource::Keyboard, 5)]);
    }

    #[test]
    fn each_source_is_reported_apart_in_the_order_it_arrived() {
        let mut tally = PendingArrivals::default();
        tally.note(EditSource::Ime, 1);
        tally.note(EditSource::Keyboard, 2);
        tally.note(EditSource::Ime, 4);
        tally.note(EditSource::Dictation, 6);
        assert_eq!(
            flushed(&mut tally),
            vec![
                (EditSource::Ime, 5),
                (EditSource::Keyboard, 2),
                (EditSource::Dictation, 6),
            ]
        );
    }

    #[test]
    fn zero_characters_are_not_an_arrival() {
        let mut tally = PendingArrivals::default();
        tally.note(EditSource::Keyboard, 0);
        assert!(tally.is_empty());
        assert!(flushed(&mut tally).is_empty());
    }

    #[test]
    fn a_flush_starts_the_next_batch_empty() {
        let mut tally = PendingArrivals::default();
        tally.note(EditSource::Keyboard, 3);
        let _ = flushed(&mut tally);
        assert!(tally.is_empty());
        assert!(
            flushed(&mut tally).is_empty(),
            "a batch is reported once, not again at the next flush"
        );
        tally.note(EditSource::WritingTools, 1);
        assert_eq!(flushed(&mut tally), vec![(EditSource::WritingTools, 1)]);
    }
}
