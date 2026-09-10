// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch selection for the code editor's core: the [`TextHitSource`] this
//! surface answers with, and the surface adapter the shared mount drives it
//! through.
//!
//! One adoption here serves all three faces —
//! [`CodeEditor`](super::CodeEditor), [`PlainTextEditor`](super::PlainTextEditor)
//! and [`LogView`](super::LogView) — because all three are one
//! [`CodeEditorState`] with different policy. The log view is the interesting
//! one: it is read-only, so the only commands it can offer at all are Copy and
//! Select All — and since Select All is offered only while nothing is selected, a
//! hold's toolbar there is Copy alone. Until now `Ctrl+C` was the *only* way to
//! get a line out of it.
//!
//! # Coordinates
//!
//! [`teksilo_core::text_touch`] is stated entirely in **window** coordinates, and
//! the router hands a handler **widget-local** ones. Two conversions, and they
//! are not interchangeable — a pointer sample takes
//! [`EventContext::pointer_position`], a timer-recognised long press has no
//! sample and converts `local + node_origin`. The rich-text module's
//! [`touch`](crate::rich_text::touch) docs carry the full argument; the same two
//! rules apply verbatim here, because both stacks put their handlers on a
//! wrapper with the body inset inside it.

use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::text_touch::{TextAffordances, TextHitSource};
use teksilo_core::widget::{EventContext, Widget};
use teksilo_text::text_document::{MoveMode, SelectionType};

use crate::rich_text::hit_test;
use crate::rich_text::touch_mount::{EditorTouch, LensPainter, TouchTextSurface};

use super::state::{CodeEditorState, SharedState};

/// The code editor, as the touch controller needs to see it.
///
/// Holds the shared handle rather than a borrow: every accessor needs only an
/// immutable borrow (a `TextCursor` mutates through `&self`), and
/// `set_selection` has to *drop* its borrow before publishing the cursor
/// signals.
pub(crate) struct CodeHitSource {
    state: SharedState,
}

impl CodeHitSource {
    fn new(state: SharedState) -> Self {
        Self { state }
    }
}

/// A **window** point in the body's own (engine) space.
///
/// The body's top-left in the window is `viewport_origin` — deliberately *not*
/// `node_origin`, which is the wrapper's and is what a widget-local pointer
/// position is measured from.
pub(crate) fn window_to_engine_local(st: &CodeEditorState, window: Point) -> Point {
    Point::new(
        window.x - st.viewport_origin.x,
        window.y - st.viewport_origin.y,
    )
}

impl TextHitSource for CodeHitSource {
    fn offset_at(&self, point: Point) -> usize {
        let st = self.state.borrow();
        let local = window_to_engine_local(&st, point);
        if let Some(hit) = hit_test::hit_test_at(&st.engine, local, 0.0, 0.0) {
            return hit.position;
        }
        // The engine answers `None` outside the laid-out text, and a handle drag
        // routinely leaves the body. The controller's contract has no
        // "nowhere", so clamp to the end the point is nearest, reading the
        // **vertical** axis first: in a multi-line surface that is the axis that
        // says which end of the document a point is past.
        let doc_len = st.document.character_count();
        if local.y < 0.0 {
            return 0;
        }
        if local.y > st.viewport_height {
            return doc_len;
        }
        if local.x < 0.0 { 0 } else { doc_len }
    }

    fn caret_rect(&self, offset: usize) -> Rect {
        let st = self.state.borrow();
        super::keyboard::window_rect_at(&st, offset).unwrap_or(Rect::ZERO)
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        // A scratch cursor, not the editor's own: this answers a question and
        // must not move a caret to do it. Deliberately the same
        // `SelectionType::WordUnderCursor` the double-click path uses, so a hold
        // and a double click cannot disagree about where a word ends.
        let st = self.state.borrow();
        let probe = st
            .document
            .cursor_at(offset.min(st.document.character_count()));
        probe.select(SelectionType::WordUnderCursor);
        let (a, p) = (probe.anchor(), probe.position());
        a.min(p)..a.max(p)
    }

    fn line_range_at(&self, offset: usize) -> Range<usize> {
        // The **line**, which is what this editor's triple click selects. In a
        // source document one line is one block, but naming the line keeps this
        // right for the wrapped plain-text face, where they diverge.
        let st = self.state.borrow();
        let probe = st
            .document
            .cursor_at(offset.min(st.document.character_count()));
        probe.select(SelectionType::LineUnderCursor);
        let (a, p) = (probe.anchor(), probe.position());
        a.min(p)..a.max(p)
    }

    fn selection(&self) -> Range<usize> {
        let st = self.state.borrow();
        let a = st.cursor.anchor();
        let p = st.cursor.position();
        a.min(p)..a.max(p)
    }

    fn set_selection(&mut self, range: Range<usize>) {
        {
            let mut st = self.state.borrow_mut();
            let len = st.document.character_count();
            let start = range.start.min(len);
            let end = range.end.min(len);
            // A touch selection is one range, so it collapses the caret set —
            // the same rule the assistive-technology `SetTextSelection` path
            // holds, and the one the rest of the editor assumes when it reports
            // a selection to AT.
            st.clear_extra_carets();
            st.cursor.set_position(start, MoveMode::MoveAnchor);
            if end != start {
                st.cursor.set_position(end, MoveMode::KeepAnchor);
            }
        }
        super::sync_cursor_signals(&self.state);
    }

    fn selection_bounds(&self) -> Option<Rect> {
        let selection = self.selection();
        if selection.is_empty() {
            return None;
        }
        let st = self.state.borrow();
        let a = super::keyboard::window_rect_at(&st, selection.start)?;
        let b = super::keyboard::window_rect_at(&st, selection.end)?;
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Some(Rect::new(
            x,
            y,
            (a.right().max(b.right()) - x).max(1.0),
            (a.bottom().max(b.bottom()) - y).max(a.height.max(b.height)),
        ))
    }

    fn viewport(&self) -> Rect {
        let st = self.state.borrow();
        Rect::new(
            st.viewport_origin.x,
            st.viewport_origin.y,
            st.viewport_width,
            st.viewport_height,
        )
    }

    fn document_len(&self) -> usize {
        self.state.borrow().document.character_count()
    }

    fn is_editable(&self) -> bool {
        !self.state.borrow().policy.is_read_only()
    }

    /// The predicate this surface's own `Ctrl+C` consults, so the touch toolbar
    /// cannot offer a Copy the keyboard would refuse — or refuse one it allows.
    /// A `LogView` answers **yes**: read-only, and copying is the whole point.
    fn allows_copy(&self) -> bool {
        super::context_menu::copy_allowed(&self.state.borrow().policy)
    }
}

/// The surface adapter the shared mount drives this editor through.
pub(crate) struct CodeTouchSurface {
    state: SharedState,
}

impl TouchTextSurface for CodeTouchSurface {
    fn with_hit_source(&self, f: &mut dyn FnMut(&mut dyn TextHitSource)) -> bool {
        let mut source = CodeHitSource::new(self.state.clone());
        f(&mut source);
        true
    }

    fn geometry_is_meaningful(&self) -> bool {
        // A laid-out engine is the whole condition. A `LogView` with no lines
        // yet has one, and a caret handle over an empty document is exactly the
        // "paste here" affordance every platform shows.
        self.state.borrow().engine.has_full_layout()
    }

    fn place_caret_at(&self, window: Point, ctx: &mut EventContext<'_>) -> bool {
        let hit = {
            let st = self.state.borrow();
            let local = window_to_engine_local(&st, window);
            hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
        };
        let Some(hit) = hit else { return false };
        {
            let mut st = self.state.borrow_mut();
            st.clear_extra_carets();
            st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
            st.cursor_affinity = hit.affinity;
            st.preferred_x = None;
        }
        super::sync_cursor_signals(&self.state);
        super::keyboard::report_ime_cursor_area(&self.state, ctx);
        true
    }

    fn lens_painter(&self) -> Option<LensPainter> {
        Some(magnifier_painter(self.state.clone()))
    }

    fn toolbar_rows(&self, affordances: TextAffordances) -> Box<dyn Widget> {
        super::context_menu::selection_toolbar(self.state.clone(), affordances)
    }

    fn nudge_scroll(&self, dy: f32) -> bool {
        let st = self.state.borrow();
        let max = st.max_scroll_y.get().max(0.0);
        let next = (st.scroll_y.get() + dy).clamp(0.0, max);
        st.scroll_y.set_if_changed(next)
    }
}

/// The closure that fills the magnifier: **only** this surface's text layer,
/// re-emitted in window coordinates.
///
/// Re-entered during the same frame inside a transform-and-clip scope, so it
/// deliberately does none of what the body's own `paint` does around the same
/// call: no `sync_viewport` (it would adopt the *lens*'s bounds), no caret
/// reveal (it writes the scroll offsets), no `set_clip` (`replay` installs the
/// lens clip and a `clear_clip` here would destroy it), no gutter, current-line
/// band, bracket wash, focus ring or scroll bars — chrome magnified over the
/// document is exactly what the contract forbids.
fn magnifier_painter(state: SharedState) -> LensPainter {
    use crate::rich_text::paint::{PaintParams, paint_frame};
    Rc::new(move |canvas, _ctx| {
        let mut st = state.borrow_mut();
        if !st.engine.has_full_layout() {
            return;
        }
        // Both scroll offsets are already applied by the engine, so the replay
        // adds the body's window top-left and nothing else — the body's own
        // origin, verbatim.
        let origin = Point::new(st.viewport_origin.x, st.viewport_origin.y);
        let target: &mut CodeEditorState = &mut st;
        let CodeEditorState {
            ref mut engine,
            ref document,
            ref mut image_cache,
            ..
        } = *target;
        engine.with_render_frame(|frame| {
            paint_frame(
                canvas,
                PaintParams {
                    frame,
                    origin,
                    document,
                    image_cache,
                    // A source document and a log have no inline pictures, so
                    // there is nothing for a resolver to resolve.
                    image_resolver: None,
                    selection: None,
                    selection_color: [0.0; 4],
                    selected_image_out: None,
                    resize_preview: None,
                    // A lens that blinked would be a second caret with a phase
                    // of its own; the selection band the frame already carries
                    // is what a handle drag is aiming at.
                    draw_caret: false,
                },
            );
        });
    })
}

/// Mint the mount for one surface — called from the widget's constructor, so the
/// handle (and the tests' view of it) exists before it enters the tree.
pub(crate) fn mount_for(state: SharedState) -> Rc<EditorTouch> {
    EditorTouch::new(Rc::new(CodeTouchSurface { state }))
}
