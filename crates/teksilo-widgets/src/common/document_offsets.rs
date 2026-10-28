// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A [`TextDocument`]'s offsets as the platform's text input system needs
//! them: where the document ends, and its [`Utf16Index`], kept until its text
//! changes.
//!
//! # The end
//!
//! The last offset a caret can take, [`document_end`], counts every paragraph
//! break and every table's anchor. `TextDocument::character_count` does not:
//! it counts the characters of the paragraphs, and is short by one per
//! paragraph break and two per table. A range the input system read from the
//! text and clamped to `character_count` lost its tail in any paragraph after
//! the first.
//!
//! # The UTF-16 index
//!
//! The platform's text input system converts offsets on every question it
//! asks, and asks often: macOS dictation polls the selection two or three
//! times a second for as long as it listens. Indexing the document on each
//! question would read the whole text every time. [`DocumentUtf16`] reads it
//! once per change instead: it counts the document's text changes as they
//! happen, and rebuilds the index the first time it is asked after one.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use teksilo_core::text_input_document::Utf16Index;
use teksilo_text::text_document::{
    DocumentEvent, MoveMode, MoveOperation, Subscription, TextDocument,
};

/// The last offset a caret can take in `document`: its length in characters,
/// paragraph breaks and table anchors included. See the
/// [module documentation](self#the-end).
///
/// Asked of a scratch cursor, which the document answers from the length of
/// its text without reading it, so it costs the same on any document.
pub(crate) fn document_end(document: &TextDocument) -> usize {
    let probe = document.cursor();
    probe.move_position(MoveOperation::End, MoveMode::MoveAnchor, 1);
    probe.position()
}

/// The UTF-16 index of one document, rebuilt lazily after its text changes.
///
/// Holds a subscription to the document's change events for as long as it
/// lives, so it belongs beside the document in the widget's state.
pub(crate) struct DocumentUtf16 {
    /// Bumped by the subscription on every event that may change the text.
    changes: Arc<AtomicU64>,
    _subscription: Subscription,
    /// The last index built, and the value of `changes` it was built at.
    built: RefCell<Option<(u64, Rc<Utf16Index>)>>,
}

impl DocumentUtf16 {
    /// Start watching `document`. Builds nothing until asked.
    pub(crate) fn watching(document: &TextDocument) -> Self {
        let changes = Arc::new(AtomicU64::new(0));
        let subscription = {
            let changes = changes.clone();
            document.on_change(move |event| {
                if may_change_text(&event) {
                    changes.fetch_add(1, Ordering::Relaxed);
                }
            })
        };
        Self {
            changes,
            _subscription: subscription,
            built: RefCell::new(None),
        }
    }

    /// The index of `document` as it stands: the one built before, while the
    /// text has not changed since, or a new one.
    ///
    /// `document` must be the one this watches. The index is over the
    /// document's addressable text, the string its character offsets index,
    /// table anchors included, so it agrees with the document's caret and
    /// selection offsets.
    pub(crate) fn index(&self, document: &TextDocument) -> Rc<Utf16Index> {
        let changes = self.changes.load(Ordering::Relaxed);
        if let Ok(built) = self.built.try_borrow()
            && let Some((at, index)) = built.as_ref()
            && *at == changes
        {
            return index.clone();
        }
        let index = Rc::new(Utf16Index::new(
            &document.to_addressable_text().unwrap_or_default(),
        ));
        if let Ok(mut built) = self.built.try_borrow_mut() {
            *built = Some((changes, index.clone()));
        }
        index
    }
}

/// Whether `event` may have changed the document's text.
///
/// Errs towards yes: a needless rebuild costs one read of the text, a missed
/// one converts offsets against text that is gone. A whole-document load
/// (`set_plain_text`, an import) sends `DocumentReset` and no
/// `ContentsChanged`, and leaves the document's `content_revision` where it
/// was, which is why this counts events itself rather than reading that.
fn may_change_text(event: &DocumentEvent) -> bool {
    !matches!(
        event,
        DocumentEvent::FormatChanged { .. }
            | DocumentEvent::HighlightPaintChanged { .. }
            | DocumentEvent::UndoRedoChanged { .. }
            | DocumentEvent::ModificationChanged(_)
            | DocumentEvent::LongOperationProgress { .. }
            // Always sent with the `ContentsChanged` that counts the edit.
            | DocumentEvent::TextInserted { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_counts_paragraph_breaks_and_table_anchors() {
        let document = TextDocument::new();
        document.set_plain_text("one\ntwo").unwrap();
        assert_eq!(document.character_count(), 6, "premise: no break counted");
        assert_eq!(document_end(&document), 7);

        let cursor = document.cursor_at(3);
        cursor.insert_table(1, 1).unwrap();
        let addressable = document.to_addressable_text().unwrap();
        assert!(addressable.contains('\u{FFFC}'));
        assert_eq!(document_end(&document), addressable.chars().count());
    }

    #[test]
    fn the_index_is_shared_until_the_text_changes() {
        let document = TextDocument::new();
        document.set_plain_text("ab").unwrap();
        let watch = DocumentUtf16::watching(&document);

        let first = watch.index(&document);
        assert_eq!(first.utf16_len(), 2);
        assert!(
            Rc::ptr_eq(&first, &watch.index(&document)),
            "no change, no rebuild"
        );

        let cursor = document.cursor_at(2);
        cursor.insert_text("\u{1F600}").unwrap();
        let after_edit = watch.index(&document);
        assert!(!Rc::ptr_eq(&first, &after_edit), "an edit rebuilds");
        assert_eq!(after_edit.char_count(), 3);
        assert_eq!(after_edit.utf16_len(), 4);
    }

    #[test]
    fn a_whole_document_load_rebuilds_although_the_revision_stays() {
        let document = TextDocument::new();
        document.set_plain_text("x").unwrap();
        let watch = DocumentUtf16::watching(&document);
        let before = watch.index(&document);
        let revision = document.content_revision();

        // The same length, so only the text itself tells the two apart.
        document.set_plain_text("\u{1F600}").unwrap();
        assert_eq!(
            document.content_revision(),
            revision,
            "the premise: a load does not move the content revision"
        );
        let after = watch.index(&document);
        assert!(!Rc::ptr_eq(&before, &after));
        assert_eq!(after.utf16_len(), 2);
    }

    #[test]
    fn a_format_change_keeps_the_index() {
        let document = TextDocument::new();
        document.set_plain_text("bold").unwrap();
        let watch = DocumentUtf16::watching(&document);
        let before = watch.index(&document);

        let cursor = document.cursor_at(0);
        cursor.set_position(4, teksilo_text::text_document::MoveMode::KeepAnchor);
        let format = teksilo_text::text_document::TextFormat {
            font_bold: Some(true),
            ..Default::default()
        };
        cursor.merge_char_format(&format).unwrap();
        assert!(Rc::ptr_eq(&before, &watch.index(&document)));
    }
}
