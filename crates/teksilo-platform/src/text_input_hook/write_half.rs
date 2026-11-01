// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Text the input system inserts outside a key press, written into the
//! focused field at once through its write half
//! ([`TextInputEditor`](teksilo_core::text_input_document::TextInputEditor)),
//! inside a batch: the half that needs no platform. Design section 3,
//! decision 7 of `engineering/docs/tsf-text-store.md`.
//!
//! Posted as an event, the text reached the field at the application's next
//! turn, so the input system's next question (AppKit asks `selectedRange`
//! right after an `insertText:`) was answered from the text before it.
//! Written at once, the answer is the text after it.
//!
//! The origin [`classify`](super::classify) gave goes with the write
//! ([`WriteOptions::origin`]), so the field reports what lands under it
//! (dictation as dictated, Writing Tools as a rewrite, Emoji & Symbols, the
//! accent popup and AutoFill as an input method's) and puts it through its
//! rules like any other write: one undo step, a line break a paragraph in a
//! rich text editor.
//!
//! What the field cannot take now (busy, read-only, no write half) is
//! posted as before ([`Written::No`]), and the application commits it.

#![cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS hook writes out of band, for now")
)]

use std::ops::Range;

use teksilo_core::ImeOrigin;
use teksilo_core::text_input_document::WriteOptions;
use teksilo_core::text_surface::TextSurfaces;

/// What became of a text [`write()`] was handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Written {
    /// In the field, over these characters.
    Yes(Range<usize>),
    /// Not written: the caller posts it.
    No(&'static str),
}

/// Write `text` over `replacement` (the field's characters), or over the
/// selection for none, into the focused field of `surfaces`, as `origin`.
/// Typing the field holds for its frame goes in first, so the range is read
/// against the text the input system saw.
pub(crate) fn write(
    surfaces: &TextSurfaces,
    text: &str,
    replacement: Option<Range<usize>>,
    origin: ImeOrigin,
) -> Written {
    let range = surfaces.with_focused_input_document(|document| {
        let _ = document.flush_pending_input();
        if !document.is_available() || !document.is_editable() {
            return None;
        }
        replacement.clone().or_else(|| document.selection())
    });
    let Some(Some(range)) = range else {
        return Written::No("no focused document can take it now");
    };
    let written = surfaces.with_focused_input_editor(|editor| {
        editor.begin_input_batch();
        let options = WriteOptions {
            origin: Some(origin),
            ..WriteOptions::default()
        };
        let result = editor.replace_text(range.clone(), text, options);
        editor.end_input_batch();
        result
    });
    match written {
        Some(Ok(landed)) => Written::Yes(landed.replaced.start..landed.inserted.end),
        Some(Err(_)) => Written::No("the field refused it"),
        None => Written::No("the focused field takes no writes"),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use teksilo_canvas::{Point, Rect, SizeProposal};
    use teksilo_core::text_input_document::{
        CompositionSegment, FirstLineRect, Landed, TextInputDocument, TextInputEditor, Utf16Index,
        WriteRefused,
    };
    use teksilo_core::text_surface::TextSurface;
    use teksilo_core::{
        BuildContext, EventContext, HandlerSet, LayoutContext, LayoutResponse, Widget, WidgetId,
        WidgetTree,
    };

    use super::*;

    /// A field that logs what its write half is asked.
    #[derive(Debug)]
    struct Field {
        text: RefCell<String>,
        selection: Cell<(usize, usize)>,
        pending: RefCell<String>,
        busy: Cell<bool>,
        log: RefCell<Vec<String>>,
    }

    impl Field {
        fn new(text: &str, caret: usize) -> Rc<Self> {
            Rc::new(Self {
                text: RefCell::new(text.to_owned()),
                selection: Cell::new((caret, caret)),
                pending: RefCell::new(String::new()),
                busy: Cell::new(false),
                log: RefCell::new(Vec::new()),
            })
        }
        fn len(&self) -> usize {
            self.text.borrow().chars().count()
        }
        fn splice(&self, range: Range<usize>, text: &str) {
            let mut chars: Vec<char> = self.text.borrow().chars().collect();
            chars.splice(range.clone(), text.chars());
            *self.text.borrow_mut() = chars.into_iter().collect();
            let end = range.start + text.chars().count();
            self.selection.set((end, end));
        }
    }

    impl TextInputDocument for Field {
        fn is_available(&self) -> bool {
            !self.busy.get() && self.pending.borrow().is_empty()
        }
        fn flush_pending_input(&self) -> bool {
            let typed = std::mem::take(&mut *self.pending.borrow_mut());
            let at = self.selection.get().1;
            self.splice(at..at, &typed);
            !typed.is_empty()
        }
        fn char_count(&self) -> Option<usize> {
            Some(self.len())
        }
        fn text_in(&self, range: Range<usize>) -> Option<String> {
            Some(
                self.text
                    .borrow()
                    .chars()
                    .skip(range.start)
                    .take(range.len())
                    .collect(),
            )
        }
        fn selection(&self) -> Option<Range<usize>> {
            let (a, b) = self.selection.get();
            Some(a..b)
        }
        fn caret(&self) -> Option<usize> {
            Some(self.selection.get().1)
        }
        fn composition_range(&self) -> Option<Range<usize>> {
            None
        }
        fn first_line_rect(&self, _: Range<usize>) -> Option<FirstLineRect> {
            None
        }
        fn offset_at_point(&self, _: Point) -> Option<usize> {
            None
        }
        fn visible_text_rect(&self) -> Option<Rect> {
            None
        }
        fn is_editable(&self) -> bool {
            true
        }
        fn is_secure(&self) -> bool {
            false
        }
        fn writing_tools_allowed(&self) -> bool {
            true
        }
        fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
            Some(Rc::new(Utf16Index::new(&self.text.borrow())))
        }
    }

    impl TextInputEditor for Field {
        fn replace_text(
            &self,
            range: Range<usize>,
            text: &str,
            options: WriteOptions,
        ) -> Result<Landed, WriteRefused> {
            if self.busy.get() {
                return Err(WriteRefused::Busy);
            }
            let origin = options.origin;
            self.log
                .borrow_mut()
                .push(format!("replace {range:?} {text:?} {origin:?}"));
            self.splice(range.clone(), text);
            Ok(Landed {
                inserted: range.start..range.start + text.chars().count(),
                replaced: range,
            })
        }
        fn mark_composition(
            &self,
            range: Range<usize>,
            _: &[CompositionSegment],
        ) -> Result<(), WriteRefused> {
            self.log.borrow_mut().push(format!("mark {range:?}"));
            Ok(())
        }
        fn end_composition(&self, origin: ImeOrigin) -> Result<(), WriteRefused> {
            self.log.borrow_mut().push(format!("end {origin:?}"));
            Ok(())
        }
        fn begin_input_batch(&self) {
            self.log.borrow_mut().push("begin".to_owned());
        }
        fn end_input_batch(&self) {
            self.log.borrow_mut().push("end batch".to_owned());
        }
    }

    impl TextSurface for Field {
        fn can_undo(&self) -> bool {
            false
        }
        fn can_redo(&self) -> bool {
            false
        }
        fn undo(&self) {}
        fn redo(&self) {}
        fn has_selection(&self) -> bool {
            false
        }
        fn is_read_only(&self) -> bool {
            false
        }
        fn allows_copy(&self) -> bool {
            true
        }
        fn cut(&self, _: &EventContext<'_>) {}
        fn copy(&self, _: &EventContext<'_>) {}
        fn paste(&self, _: &EventContext<'_>) {}
        fn paste_plain(&self, _: &EventContext<'_>) {}
        fn select_all(&self) {}
        fn input_document(&self) -> Option<&dyn TextInputDocument> {
            Some(self)
        }
        fn input_editor(&self) -> Option<&dyn TextInputEditor> {
            Some(self)
        }
    }

    #[derive(Debug)]
    struct Host {
        field: Rc<Field>,
    }

    impl Widget for Host {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.apply_self_handlers(HandlerSet::new().focusable(true));
            ctx.register_text_surface(self.field.clone());
            vec![]
        }
        fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            proposal.resolve(100.0, 20.0).into()
        }
    }

    fn focused(field: &Rc<Field>) -> WidgetTree {
        let mut tree = WidgetTree::new();
        let id = tree.add(Host {
            field: field.clone(),
        });
        tree.layout(SizeProposal::exact(100.0, 20.0));
        tree.focus(id);
        tree
    }

    #[test]
    fn an_insertion_is_written_over_the_selection_or_the_range_named() {
        let field = Field::new("cafe", 4);
        let tree = focused(&field);
        let surfaces = tree.text_surfaces();
        assert_eq!(
            write(&surfaces, "\u{1F600}", None, ImeOrigin::Insertion),
            Written::Yes(4..5)
        );
        assert_eq!(
            write(&surfaces, "\u{E9}", Some(3..4), ImeOrigin::Insertion),
            Written::Yes(3..4)
        );
        assert_eq!(*field.text.borrow(), "caf\u{E9}\u{1F600}");
        assert_eq!(
            *field.log.borrow(),
            vec![
                "begin",
                "replace 4..4 \"\u{1F600}\" Some(Insertion)",
                "end batch",
                "begin",
                "replace 3..4 \"\u{E9}\" Some(Insertion)",
                "end batch"
            ]
        );
        assert_eq!(
            field.selection(),
            Some(4..4),
            "answered after the write at once"
        );
    }

    #[test]
    fn dictation_and_writing_tools_are_written_with_their_origin_line_breaks_included() {
        let field = Field::new("ab", 2);
        let tree = focused(&field);
        let surfaces = tree.text_surfaces();
        assert_eq!(
            write(&surfaces, " hello", None, ImeOrigin::Dictation),
            Written::Yes(2..8)
        );
        assert_eq!(
            write(&surfaces, "one\ntwo", Some(0..2), ImeOrigin::WritingTools),
            Written::Yes(0..7)
        );
        assert_eq!(*field.text.borrow(), "one\ntwo hello");
        assert_eq!(
            *field.log.borrow(),
            vec![
                "begin",
                "replace 2..2 \" hello\" Some(Dictation)",
                "end batch",
                "begin",
                "replace 0..2 \"one\\ntwo\" Some(WritingTools)",
                "end batch"
            ]
        );
    }

    #[test]
    fn typing_waiting_goes_first_and_a_busy_field_is_left_to_the_event() {
        let field = Field::new("ab", 2);
        let tree = focused(&field);
        let surfaces = tree.text_surfaces();
        field.pending.borrow_mut().push('c');
        assert_eq!(
            write(&surfaces, "\u{1F600}", None, ImeOrigin::Insertion),
            Written::Yes(3..4)
        );
        assert_eq!(*field.text.borrow(), "abc\u{1F600}");
        field.busy.set(true);
        assert!(matches!(
            write(&surfaces, "x", None, ImeOrigin::Insertion),
            Written::No(_)
        ));
    }
}
