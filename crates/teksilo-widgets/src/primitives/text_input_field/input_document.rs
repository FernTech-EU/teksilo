// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The field's document, as the platform's text input system reads it: the
//! [`TextInputDocument`] a [`TextFieldHandle`] answers with, for
//! `TextInputField` and every field built on it (`TextInput`, `PasswordField`,
//! `SearchField`, `SpinBox`).
//!
//! One line, no wrapping: the first line of any range is the whole range.
//!
//! # A secure field
//!
//! Gives out no text ([`text_in`](TextInputDocument::text_in) is `None`), and
//! no Writing Tools. It still says where its caret and selection are, which
//! an input method needs in order to insert anything at all. One that echoes
//! nothing (`EchoMode::NoEcho`) lays out an empty line, so its geometry would
//! tell the text's length; it answers every range with the same rectangle at
//! the start of that line, and no offset for a point, following
//! `TextInputState::caret_geometry_is_meaningful` as the touch controller
//! does.
//!
//! # Typing not yet inserted
//!
//! Typed characters wait in `pending_chars` until the frame tick inserts them
//! with `apply_pending_chars`. The answers about the text, the selection, the
//! composition or where they are drawn insert them first, as
//! `TextFieldHandle::text_as_typed` does and for the reasons the rich text
//! editor's `input_document` module gives: the input system's next commit
//! names a range in the text as typed and inserts the batch before reading
//! it, and inserting runs outside dispatch already (it is the tick's first
//! step). It writes the document and reports the batch to the application's
//! arrivals callback; the bound `Signal<String>` follows at the next tick,
//! for which a frame is requested.
//!
//! # Busy
//!
//! `None` (or the cautious answer) while the field's state is borrowed, and
//! before the field is built.

use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::text_input_document::{FirstLineRect, TextInputDocument, Utf16Index};
use teksilo_text::CursorAffinity;

use super::TextFieldHandle;
use super::state::{SharedState, TextInputState};

impl TextFieldHandle {
    /// The field's shared state, while it is built and the handle's slot is
    /// free.
    fn shared_state(&self) -> Option<SharedState> {
        let slot = self.slot.try_borrow().ok()?;
        slot.as_ref().cloned()
    }

    /// Run `f` on the field's state with every typed character inserted.
    /// `None` while the state is borrowed, or before the field is built.
    fn with_typing_inserted<R>(&self, f: impl FnOnce(&TextInputState) -> R) -> Option<R> {
        let state = self.shared_state()?;
        {
            let st = state.try_borrow().ok()?;
            if st.pending_chars.is_empty() {
                return Some(f(&st));
            }
        }
        let mut st = state.try_borrow_mut().ok()?;
        st.apply_pending_chars();
        if let Some(request) = &st.frame_request {
            request.set(true);
        }
        Some(f(&st))
    }

    /// Run `f` on the field's state as it is, typing or not, for a question
    /// the text does not change the answer to.
    fn with_state<R>(&self, f: impl FnOnce(&TextInputState) -> R) -> Option<R> {
        let state = self.shared_state()?;
        let st = state.try_borrow().ok()?;
        Some(f(&st))
    }
}

impl TextInputDocument for TextFieldHandle {
    fn char_count(&self) -> Option<usize> {
        self.with_typing_inserted(document_end)
    }

    fn text_in(&self, range: Range<usize>) -> Option<String> {
        self.with_typing_inserted(|st| {
            if st.secure {
                return None;
            }
            let range = clamp(range, document_end(st));
            if range.is_empty() {
                return Some(String::new());
            }
            st.document.text_at(range.start, range.len()).ok()
        })
        .flatten()
    }

    fn selection(&self) -> Option<Range<usize>> {
        self.with_typing_inserted(|st| {
            let (anchor, caret) = (st.cursor.anchor(), st.cursor.position());
            anchor.min(caret)..anchor.max(caret)
        })
    }

    fn caret(&self) -> Option<usize> {
        self.with_typing_inserted(|st| st.cursor.position())
    }

    fn composition_range(&self) -> Option<Range<usize>> {
        // The document says where the composition is: a recorded write since
        // it was shown reverted it, and an ended one waits only for its commit.
        self.with_typing_inserted(|st| st.composition.range(&st.document))
            .flatten()
    }

    fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect> {
        self.with_typing_inserted(|st| first_line_rect(st, range))
            .flatten()
    }

    fn offset_at_point(&self, window_point: Point) -> Option<usize> {
        self.with_typing_inserted(|st| offset_at_point(st, window_point))
            .flatten()
    }

    fn visible_text_rect(&self) -> Option<Rect> {
        self.with_state(visible_text_rect).flatten()
    }

    fn is_editable(&self) -> bool {
        self.with_state(|st| !st.read_only).unwrap_or(false)
    }

    fn is_secure(&self) -> bool {
        self.with_state(|st| st.secure).unwrap_or(true)
    }

    fn writing_tools_allowed(&self) -> bool {
        self.with_state(TextInputState::writing_tools_allowed)
            .unwrap_or(false)
    }

    fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
        self.with_typing_inserted(|st| st.utf16.index(&st.document))
    }
}

/// The field's last offset. For its one paragraph the same number as
/// `TextDocument::character_count`; asked the way the rich text editor asks,
/// so the two widgets count the same way.
fn document_end(st: &TextInputState) -> usize {
    crate::common::document_offsets::document_end(&st.document)
}

/// `range` within `0..=len`, its end not before its start.
fn clamp(range: Range<usize>, len: usize) -> Range<usize> {
    let start = range.start.min(len);
    start..range.end.min(len).max(start)
}

/// The caret rectangle at `offset`, in window space: the composition
/// `keyboard::report_ime_cursor_area` uses, at least 1 wide.
fn caret_in_window(st: &TextInputState, offset: usize) -> Rect {
    let caret = st.engine.caret_rect(offset, CursorAffinity::Downstream);
    Rect::new(
        st.viewport_origin.x + caret[0] - st.scroll_x,
        st.viewport_origin.y + st.text_offset_y + caret[1],
        caret[2].max(1.0),
        caret[3].max(1.0),
    )
}

/// The rectangle of `range` on the field's one line, in window space.
fn first_line_rect(st: &TextInputState, range: Range<usize>) -> Option<FirstLineRect> {
    if !st.engine.has_full_layout() {
        return None;
    }
    let range = clamp(range, document_end(st));
    if !st.caret_geometry_is_meaningful() {
        // The engine holds an empty line: offset 0 is the only place there
        // is, and it is the same place whatever was asked.
        return Some(FirstLineRect {
            rect: caret_in_window(st, 0),
            covered: range,
        });
    }
    let first = caret_in_window(st, range.start);
    if range.is_empty() {
        return Some(FirstLineRect {
            rect: first,
            covered: range,
        });
    }
    let last = caret_in_window(st, range.end);
    let left = first.x.min(last.x);
    let right = first.x.max(last.x);
    Some(FirstLineRect {
        rect: Rect::new(left, first.y, (right - left).max(1.0), first.height),
        covered: range,
    })
}

/// The document offset nearest `window_point`, `None` outside the text area
/// (the suffix strip included) and on a field whose geometry means nothing.
fn offset_at_point(st: &TextInputState, window_point: Point) -> Option<usize> {
    if !st.engine.has_full_layout() || !st.caret_geometry_is_meaningful() {
        return None;
    }
    let area = visible_text_rect(st)?;
    if !area.contains(window_point) {
        return None;
    }
    let local = Point::new(
        window_point.x - st.viewport_origin.x,
        window_point.y - st.viewport_origin.y,
    );
    super::mouse::hit_test_in(st, local)
}

/// The field's box less its suffix strip, in the window.
fn visible_text_rect(st: &TextInputState) -> Option<Rect> {
    let width = (st.viewport_width - st.suffix_width).max(0.0);
    (width > 0.0 && st.viewport_height > 0.0).then(|| {
        Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            width,
            st.viewport_height,
        )
    })
}
