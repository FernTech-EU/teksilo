// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch selection for the rich text editor: the [`TextHitSource`] this surface
//! answers with, and the surface adapter the shared mount drives it through.
//!
//! One adoption here serves `RichTextEditor` in both faces — the editor and the
//! read-only viewer — because everything it needs is on the shared
//! [`EditorState`](super::state::EditorState).
//!
//! # Coordinates
//!
//! [`teksilo_core::text_touch`] is stated entirely in **window** coordinates,
//! and the router hands a handler **widget-local** ones: `localize_event`
//! rewrites every pointer position and `localize_gesture` every `TapEvent`
//! before the target sees it. A host that forwards what it was given puts each
//! affordance one viewport origin away from the text it marks.
//!
//! Two conversions, and they are not interchangeable:
//!
//! * **A pointer sample** uses [`EventContext::pointer_position`], the window
//!   position of *the sample being dispatched*. It is the only source that
//!   stays correct for the affordance nodes: they are placed on the handle
//!   geometry, so they move while they are being dragged, and a
//!   local-plus-origin conversion would have to guess which placement the
//!   router localized against.
//! * **A long press** has no sample — it is recognised by a timer, and
//!   `pointer_position` is `None` there — so it converts
//!   `local + node_origin`, which is exact because the *editor* does not move
//!   mid-press. That is the same arithmetic
//!   [`to_engine_local`](super::mouse) already does in the other direction.
//!
//! Neither conversion carries a transform term, so an editor under a scene or
//! zoom transform reports affordance geometry in the untransformed space —
//! exactly as this stack's existing window-space paths (the OS IME candidate
//! area, the context-menu caret) already do.

use std::ops::Range;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::text_touch::{TextAffordances, TextHitSource};
use teksilo_core::widget::{EventContext, Widget};
use teksilo_text::text_document::{MoveMode, SelectionType};

use super::policy::EditCommandKind;
use super::state::SharedState;
use super::touch_mount::{EditorTouch, LensPainter, TouchTextSurface};

/// The rich text editor, as the touch controller needs to see it.
///
/// Holds the shared handle rather than a borrow of the state: every accessor
/// below needs only an immutable borrow (a `TextCursor` mutates through `&self`,
/// because the document owns it), and `set_selection` has to be able to *drop*
/// its borrow before publishing the cursor signals.
pub(crate) struct RichTextHitSource {
    state: SharedState,
}

impl RichTextHitSource {
    pub(crate) fn new(state: SharedState) -> Self {
        Self { state }
    }

    fn doc_len(&self) -> usize {
        self.state.borrow().document.character_count()
    }
}

/// A **window** point in the body's own (engine) space.
///
/// The body's top-left in the window is `viewport_origin`, so this is the
/// inverse of [`caret_rect_in_window`]'s `x` term — and deliberately *not*
/// `to_engine_local`, whose input is wrapper-local rather than window.
pub(crate) fn window_to_engine_local(st: &super::state::EditorState, window: Point) -> Point {
    Point::new(
        window.x - st.viewport_origin.x,
        window.y - st.viewport_origin.y,
    )
}

/// The window rectangle of the caret at `offset`.
///
/// [`range_window_rect`](super::keyboard) with both ends at the same offset:
/// one composition, shared with the OS IME candidate area and the caret chase,
/// so the three cannot drift.
fn caret_rect_in_window(st: &super::state::EditorState, offset: usize) -> Rect {
    super::keyboard::range_window_rect(st, offset, offset).unwrap_or(Rect::ZERO)
}

impl TextHitSource for RichTextHitSource {
    fn offset_at(&self, point: Point) -> usize {
        let st = self.state.borrow();
        let local = window_to_engine_local(&st, point);
        if let Some(hit) = super::hit_test::hit_test_at(&st.engine, local, 0.0, 0.0) {
            return hit.position;
        }
        // The engine answers `None` for a point outside the laid-out text, and
        // a handle drag routinely leaves the body — up past the first line,
        // down past the last, out either side. The controller's contract has no
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
        caret_rect_in_window(&st, offset)
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        // A scratch cursor, not the editor's own: this answers a question and
        // must not move the caret to do it. The document registers cursors
        // weakly and prunes dead ones, so it costs nothing to drop.
        //
        // Deliberately the same `SelectionType::WordUnderCursor` the
        // double-click path uses, so a hold and a double click cannot disagree
        // about where a word ends.
        let st = self.state.borrow();
        let probe = st
            .document
            .cursor_at(offset.min(st.document.character_count()));
        probe.select(SelectionType::WordUnderCursor);
        let (a, p) = (probe.anchor(), probe.position());
        a.min(p)..a.max(p)
    }

    fn line_range_at(&self, offset: usize) -> Range<usize> {
        // The *block*, which is what this editor's triple click selects — a
        // paragraph, a list item, a table cell — rather than a visual line. A
        // soft-wrapped paragraph has several visual lines and one block, and the
        // controller uses this only for a whole-unit selection, where the block
        // is the unit a writer means.
        let st = self.state.borrow();
        let probe = st
            .document
            .cursor_at(offset.min(st.document.character_count()));
        probe.select(SelectionType::BlockUnderCursor);
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
            let st = self.state.borrow();
            let len = st.document.character_count();
            let start = range.start.min(len);
            let end = range.end.min(len);
            st.cursor.set_position(start, MoveMode::MoveAnchor);
            if end != start {
                st.cursor.set_position(end, MoveMode::KeepAnchor);
            }
        }
        // Outside the borrow: `sync_cursor_signals` takes the handle and
        // borrows mutably to restart the blink.
        super::sync_cursor_signals(&self.state);
    }

    fn selection_bounds(&self) -> Option<Rect> {
        let selection = self.selection();
        if selection.is_empty() {
            return None;
        }
        let st = self.state.borrow();
        super::keyboard::range_window_rect(&st, selection.start, selection.end)
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
        self.doc_len()
    }

    fn is_editable(&self) -> bool {
        !self.state.borrow().policy.is_read_only()
    }

    /// The predicate this editor's own `Ctrl+C` consults, so the touch toolbar
    /// cannot offer a Copy the keyboard would refuse — or refuse one it allows.
    ///
    /// Both halves: the clipboard policy answers "does this surface have a
    /// clipboard at all", and the command filter answers "may this surface
    /// yield its text". A viewer says yes to both; a surface whose host vetoed
    /// `Copy` says no.
    fn allows_copy(&self) -> bool {
        let policy = self.state.borrow().policy;
        policy.clipboard_policy.allows_copy()
            && policy.command_filter.accepts(EditCommandKind::Copy)
    }
}

/// The surface adapter the shared mount drives this editor through.
pub(crate) struct RichTextTouchSurface {
    state: SharedState,
}

impl RichTextTouchSurface {
    pub(crate) fn new(state: SharedState) -> Self {
        Self { state }
    }
}

impl TouchTextSurface for RichTextTouchSurface {
    fn with_hit_source(&self, f: &mut dyn FnMut(&mut dyn TextHitSource)) -> bool {
        let mut source = RichTextHitSource::new(self.state.clone());
        f(&mut source);
        true
    }

    fn geometry_is_meaningful(&self) -> bool {
        // A laid-out engine is the whole condition: an *empty* document still
        // has a caret line, and a caret handle over it is exactly the "paste
        // here" affordance every platform shows.
        self.state.borrow().engine.has_full_layout()
    }

    fn place_caret_at(&self, window: Point, ctx: &mut EventContext<'_>) -> bool {
        let hit = {
            let st = self.state.borrow();
            let local = window_to_engine_local(&st, window);
            super::hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
        };
        let Some(hit) = hit else { return false };
        {
            let mut st = self.state.borrow_mut();
            st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
            st.cursor_affinity = hit.affinity;
            st.preferred_x = None;
            st.mouse_anchored = true;
        }
        super::sync_cursor_signals(&self.state);
        super::keyboard::report_ime_cursor_area(&self.state, ctx);
        true
    }

    fn report_ime_area(&self, ctx: &mut EventContext<'_>) {
        super::keyboard::report_ime_cursor_area(&self.state, ctx);
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

/// The closure that fills the magnifier: **only** this editor's text layer,
/// re-emitted in window coordinates.
///
/// It is re-entered during the same frame, inside a transform-and-clip scope, so
/// it deliberately does none of what
/// [`RichTextEditorBody::paint`](super::body) does around the same call: no
/// `sync_viewport` (it would adopt the *lens*'s bounds as the body's), no
/// caret chase (it writes the scroll offsets), no `set_clip` (`replay` installs
/// the lens clip and a `clear_clip` in here would destroy it), no border, focus
/// ring or scroll bars. What is left — re-rendering an already-laid-out flow —
/// produces the same display list twice, which is what the contract asks for.
fn magnifier_painter(state: SharedState) -> LensPainter {
    use super::paint::{PaintParams, paint_frame};
    Rc::new(move |canvas, _ctx| {
        let mut st = state.borrow_mut();
        if !st.engine.has_full_layout() {
            return;
        }
        // The body's own origin, verbatim: this engine emits glyph coordinates
        // with **both** scroll offsets already applied, so the replay adds the
        // body's window top-left and nothing else. Subtracting `scroll_x` here
        // — which the single-line stack has to do, because its engine has no
        // horizontal scroll of its own — would slide the lens's content sideways
        // by the scroll amount.
        let origin = Point::new(st.viewport_origin.x, st.viewport_origin.y);
        let target: &mut super::state::EditorState = &mut st;
        let super::state::EditorState {
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
                    // Deliberately no resolver. The lens magnifies a region of
                    // the viewport, so every picture it can show was already
                    // painted — and resolved — by the host's own paint earlier
                    // in this same frame, which means a resolver call from here
                    // could only ever fire for content the lens cannot reach.
                    image_resolver: None,
                    selection: None,
                    selection_color: [0.0; 4],
                    // `None`: the paint pass is what tells the pointer handler
                    // where an image's resize grips are, and a second pass
                    // through the *lens*'s transform would overwrite that with
                    // magnified coordinates.
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

/// Mint the mount for one editor — called from `RichTextEditor::construct`, so the
/// handle (and the tests' view of it) exists before the widget enters the tree.
pub(crate) fn mount_for(state: SharedState) -> Rc<EditorTouch> {
    EditorTouch::new(Rc::new(RichTextTouchSurface::new(state)))
}
