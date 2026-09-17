// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The controller's whole surface is a pure function of a text source and a
//! pointer stream, so all of it is exercised here against a fake source. Where
//! a test needs a real arena — the handles' accessibility node, the layer's
//! placement, the magnifier's display list — it builds one headlessly.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_tokens::{InputTokens, TargetDensity};

use crate::environment::LayoutDirection;
use crate::event::{EventResponse, Modifiers, PointerButton, WidgetEvent};
use crate::overlay::direction::HorizontalSide;
use crate::overlay::{OverlayPlacement, SelectionHandleKind};
use crate::pointer::{EventTime, InputSnapshot, PointerInfo};
use crate::widget::{EventContext, PaintContext};
use crate::widget_id::WidgetId;

use super::*;

const LTR: LayoutDirection = LayoutDirection::LeftToRight;
const RTL: LayoutDirection = LayoutDirection::RightToLeft;

// ---------------------------------------------------------------------------
// A fake text surface: one line of `CHAR_WIDTH`-wide characters per row.
// ---------------------------------------------------------------------------

const CHAR_WIDTH: f32 = 10.0;
const LINE_HEIGHT: f32 = 20.0;

#[derive(Debug)]
struct FakeText {
    /// Characters per line; `lines * per_line` is the document length.
    per_line: usize,
    lines: usize,
    selection: Range<usize>,
    viewport: Rect,
    editable: bool,
    copyable: bool,
}

impl FakeText {
    fn new() -> Self {
        Self {
            per_line: 20,
            lines: 3,
            selection: 0..0,
            viewport: Rect::new(0.0, 0.0, 200.0, 60.0),
            editable: true,
            copyable: true,
        }
    }

    fn read_only(mut self) -> Self {
        self.editable = false;
        self
    }

    fn secret(mut self) -> Self {
        self.copyable = false;
        self
    }

    fn with_viewport(mut self, viewport: Rect) -> Self {
        self.viewport = viewport;
        self
    }

    fn selected(mut self, range: Range<usize>) -> Self {
        self.selection = range;
        self
    }

    /// Where offset `n` sits: column `n % per_line` of row `n / per_line`.
    ///
    /// The text is laid out from the window origin and the viewport is a
    /// window onto it, so a viewport that does not start at the origin has
    /// text scrolled out of view — which is the state the affordances have to
    /// survive.
    fn point_of(&self, offset: usize) -> Point {
        let row = offset / self.per_line;
        let col = offset % self.per_line;
        Point::new(
            col as f32 * CHAR_WIDTH + CHAR_WIDTH / 2.0,
            row as f32 * LINE_HEIGHT + LINE_HEIGHT / 2.0,
        )
    }
}

impl TextHitSource for FakeText {
    fn offset_at(&self, point: Point) -> usize {
        let col = (point.x / CHAR_WIDTH).floor().max(0.0) as usize;
        let row = (point.y / LINE_HEIGHT).floor().max(0.0) as usize;
        (row.min(self.lines - 1) * self.per_line + col.min(self.per_line)).min(self.document_len())
    }

    fn caret_rect(&self, offset: usize) -> Rect {
        let row = offset / self.per_line;
        let col = offset % self.per_line;
        Rect::new(
            col as f32 * CHAR_WIDTH,
            row as f32 * LINE_HEIGHT,
            1.0,
            LINE_HEIGHT,
        )
    }

    /// Five-character "words" on a fixed grid — enough to tell "the word under
    /// the finger" from "the character under the finger".
    fn word_range_at(&self, offset: usize) -> Range<usize> {
        let start = offset - offset % 5;
        start..(start + 5).min(self.document_len())
    }

    fn line_range_at(&self, offset: usize) -> Range<usize> {
        let row = offset / self.per_line;
        (row * self.per_line)..((row + 1) * self.per_line).min(self.document_len())
    }

    fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    fn set_selection(&mut self, range: Range<usize>) {
        self.selection = range;
    }

    fn selection_bounds(&self) -> Option<Rect> {
        if self.selection.is_empty() {
            return None;
        }
        let start = self.caret_rect(self.selection.start);
        let end = self.caret_rect(self.selection.end);
        let x = start.x.min(end.x);
        let y = start.y.min(end.y);
        Some(Rect::new(
            x,
            y,
            start.right().max(end.right()) - x,
            start.bottom().max(end.bottom()) - y,
        ))
    }

    fn viewport(&self) -> Rect {
        self.viewport
    }

    fn document_len(&self) -> usize {
        self.per_line * self.lines
    }

    fn is_editable(&self) -> bool {
        self.editable
    }

    fn allows_copy(&self) -> bool {
        self.copyable
    }
}

// ---------------------------------------------------------------------------
// Contexts
// ---------------------------------------------------------------------------

/// A context whose pointer is of `kind`. The id is irrelevant here — nothing
/// in the controller reads it — so this borrows the mouse's and overrides only
/// the kind, which is the one thing every entry point branches on.
fn ctx_for<'a>(kind: teksilo_tokens::PointerKind, direction: LayoutDirection) -> EventContext<'a> {
    let mut pointer = PointerInfo::mouse(EventTime::ZERO);
    pointer.kind = kind;
    let snapshot = InputSnapshot {
        pointer,
        ..InputSnapshot::default()
    };
    EventContext::new()
        .with_input_snapshot(snapshot)
        .with_layout_direction(direction)
}

/// The device a gesture names — what `TapEvent::pointer` carries, and what the
/// long-press entry point now takes rather than asking a context that, on the
/// timer path, cannot know.
fn pointer_of(kind: teksilo_tokens::PointerKind) -> PointerInfo {
    let mut pointer = PointerInfo::mouse(EventTime::ZERO);
    pointer.kind = kind;
    pointer
}

fn finger() -> PointerInfo {
    pointer_of(teksilo_tokens::PointerKind::Touch)
}

fn mouse() -> PointerInfo {
    PointerInfo::mouse(EventTime::ZERO)
}

/// A context whose pointer is a finger. Touch tests drive this.
fn touch_ctx<'a>(direction: LayoutDirection) -> EventContext<'a> {
    ctx_for(teksilo_tokens::PointerKind::Touch, direction)
}

/// A context whose pointer is the mouse — the default, and what every legacy
/// `WidgetEvent` dispatch reports.
fn mouse_ctx<'a>() -> EventContext<'a> {
    EventContext::new().with_layout_direction(LTR)
}

fn down(position: Point) -> WidgetEvent {
    WidgetEvent::pointer_down(position, PointerButton::Primary, Modifiers::NONE)
}

fn up(position: Point) -> WidgetEvent {
    WidgetEvent::pointer_up(position, PointerButton::Primary, Modifiers::NONE)
}

// ---------------------------------------------------------------------------
// The mouse invariant
// ---------------------------------------------------------------------------

/// The acceptance criterion of the whole touch programme, stated for this
/// package: a mouse reaches none of this code. The long press matters most —
/// the gesture arena installs a long-press recognizer on the presence of the
/// handler alone, with no pointer-kind condition, so an unguarded controller
/// would make a half-second mouse hold select a word.
#[test]
fn a_mouse_long_press_selects_nothing_and_raises_nothing() {
    let mut source = FakeText::new();
    let mut controller = TouchSelection::new();
    let mut ctx = mouse_ctx();

    let response = controller.on_long_press(mouse(), source.point_of(7), &mut ctx, &mut source);

    assert_eq!(response, EventResponse::Ignored);
    assert_eq!(source.selection(), 0..0, "a mouse hold moved the selection");
    assert!(
        controller.handles().is_empty(),
        "a mouse hold raised handles"
    );
    assert!(
        controller.toolbar().is_none(),
        "a mouse hold raised a toolbar"
    );
}

/// The gesture names the device, and the context does not get a vote.
///
/// This is the shape the long-press entry point failed at for its whole first
/// life. A hold is recognised by the gesture timer, not by a sample, and the
/// tree's in-flight snapshot is saved-and-restored around every dispatch — so
/// the context a hold handler is handed reported **the mouse** whatever the
/// finger was, and a guard reading it refused every caller the entry point
/// exists for. The tree now installs the holding contact (pinned separately, in
/// `pointer_state`), but the guard reads the gesture's own pointer, so the two
/// halves below hold even where no tree installed anything at all: a context
/// built by hand, an assistive-technology action, a host driving its own timer.
#[test]
fn the_long_press_guard_reads_the_gesture_and_not_the_context() {
    // A finger, under a context that says "mouse" — which is exactly what the
    // timer path used to hand every hold.
    let mut source = FakeText::new();
    let mut controller = TouchSelection::new();
    let mut ctx = mouse_ctx();

    let response = controller.on_long_press(finger(), source.point_of(7), &mut ctx, &mut source);

    assert_eq!(
        response,
        EventResponse::Handled,
        "a finger's hold was refused because the context reported a mouse"
    );
    assert_eq!(source.selection(), 5..10, "the word under offset 7");
    assert_eq!(controller.handles().len(), 2);

    // …and the converse, so the guard cannot be passing by ignoring its
    // argument: a mouse, under a context that says "touch".
    let mut source = FakeText::new();
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);

    let response = controller.on_long_press(mouse(), source.point_of(7), &mut ctx, &mut source);

    assert_eq!(response, EventResponse::Ignored);
    assert_eq!(source.selection(), 0..0, "a mouse hold moved the selection");
    assert!(controller.handles().is_empty());
}

/// The same guard on the pointer route: a mouse press, move and release leave
/// the controller with nothing published, whatever the geometry says.
#[test]
fn a_mouse_press_raises_no_affordances() {
    let mut source = FakeText::new().selected(2..8);
    let mut controller = TouchSelection::new();
    let mut ctx = mouse_ctx();

    let point = source.point_of(2);
    assert_eq!(
        controller.handle_pointer(&down(point), &mut ctx, &mut source),
        EventResponse::Ignored
    );
    assert_eq!(
        controller.handle_pointer(&up(point), &mut ctx, &mut source),
        EventResponse::Ignored
    );
    assert!(
        controller.handles().is_empty() && controller.toolbar().is_none(),
        "a mouse press published affordances: {controller:?}"
    );
}

/// `drag_handle` refuses an indirect pointer on its own account.
///
/// It is `pub`, and the mouse refusal on the way in to it is *not* the same
/// check: the layer's own `is_direct` guard covers presses arriving at the
/// layer's nodes, and `handle_pointer`'s covers a host that hit-tests for
/// itself. A caller reaching `drag_handle` directly — which the public API
/// permits — passes through neither, and without this guard would drag the
/// selection with the cursor.
///
/// Both `Begin` and the `Move` that would follow are refused, so the guard
/// cannot be walked around by feeding the phases past it one at a time — a
/// `Begin` that armed nothing but a `Move` that still moved the selection would
/// be the same bug with an extra step.
#[test]
fn a_mouse_on_a_handle_is_refused_by_the_controller_itself() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    let before = source.selection();
    let mut ctx = mouse_ctx();

    assert_eq!(
        controller.drag_handle(
            SelectionHandleKind::End,
            HandleDragPhase::Begin,
            source.point_of(14),
            &mut ctx,
            &mut source,
        ),
        EventResponse::Ignored,
        "a mouse must fall through to the text beneath the handle"
    );
    assert_eq!(source.selection(), before, "a mouse moved the selection");
    assert!(!controller.is_dragging(), "a mouse armed a handle drag");

    assert_eq!(
        controller.drag_handle(
            SelectionHandleKind::End,
            HandleDragPhase::Move,
            source.point_of(2),
            &mut ctx,
            &mut source,
        ),
        EventResponse::Ignored
    );
    assert_eq!(source.selection(), before);
    assert!(!controller.is_dragging());
}

/// A stylus is a *direct* pointer, so it gets the full affordance set. The
/// distinction between `is_direct` and `is_coarse` is invisible to a test that
/// only ever drives a finger, so it is pinned here.
#[test]
fn a_pen_gets_the_same_affordances_as_a_finger() {
    let mut source = FakeText::new();
    let mut controller = TouchSelection::new();
    let mut ctx = ctx_for(
        teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::default()),
        LTR,
    );

    let response = controller.on_long_press(
        pointer_of(teksilo_tokens::PointerKind::Pen(
            teksilo_tokens::PenKind::default(),
        )),
        source.point_of(7),
        &mut ctx,
        &mut source,
    );

    assert_eq!(response, EventResponse::Handled);
    assert_eq!(source.selection(), 5..10);
    assert_eq!(controller.handles().len(), 2);
}

// ---------------------------------------------------------------------------
// Long press
// ---------------------------------------------------------------------------

#[test]
fn a_long_press_selects_the_word_under_the_finger_and_raises_two_handles() {
    let mut source = FakeText::new();
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);

    let response = controller.on_long_press(finger(), source.point_of(7), &mut ctx, &mut source);

    assert_eq!(response, EventResponse::Handled);
    assert_eq!(source.selection(), 5..10, "the word under offset 7");
    let kinds: Vec<_> = controller.handles().iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Start, SelectionHandleKind::End]
    );
}

/// A caret with nothing selected gets one handle under it, and only when the
/// surface can be typed into.
#[test]
fn an_editable_caret_raises_one_caret_handle() {
    let source = FakeText::new().selected(4..4);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let handles = controller.handles();
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].kind, SelectionHandleKind::Caret);
    assert_eq!(handles[0].offset, 4);
    assert_eq!(handles[0].document_len, source.document_len());
}

// ---------------------------------------------------------------------------
// Handle drag
// ---------------------------------------------------------------------------

#[test]
fn dragging_the_end_handle_moves_only_that_end() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    let grab = source.point_of(10);
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        grab,
        &mut ctx,
        &mut source,
    );
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Move,
        source.point_of(14),
        &mut ctx,
        &mut source,
    );
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::End,
        source.point_of(14),
        &mut ctx,
        &mut source,
    );

    assert_eq!(source.selection(), 5..14, "the start must not have moved");
    assert!(!controller.is_dragging());
}

/// Dragging one end past the other grows the range on the far side rather than
/// collapsing it, and which handle sits at the finger is read back out of the
/// resulting range: dragging the `End` leftwards past the fixed start leaves the
/// finger on the `Start`, and dragging it back rightwards restores the `End`.
/// Both directions are asserted here, which is what makes the rule symmetric
/// rather than a one-way special case.
#[test]
fn dragging_a_handle_past_the_other_end_grows_the_range_on_the_far_side() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Move,
        source.point_of(2),
        &mut ctx,
        &mut source,
    );

    assert_eq!(source.selection(), 2..5);
    let at_finger = controller
        .handles()
        .into_iter()
        .find(|h| h.offset == 2)
        .expect("a handle at the finger");
    assert_eq!(
        at_finger.kind,
        SelectionHandleKind::Start,
        "the end that crossed is now the start"
    );

    // And back again, to show the range is not stuck on the far side and that
    // the kind under the finger follows the crossing in both directions.
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Move,
        source.point_of(8),
        &mut ctx,
        &mut source,
    );
    assert_eq!(source.selection(), 5..8);
    let back_at_finger = controller
        .handles()
        .into_iter()
        .find(|h| h.offset == 8)
        .expect("a handle at the finger");
    assert_eq!(
        back_at_finger.kind,
        SelectionHandleKind::End,
        "crossing back must return the finger to the end"
    );
}

/// Dragging one end exactly onto the other empties the selection. No caret
/// handle may appear while that happens: a third target arriving under the
/// finger mid-gesture is how a drag loses its grip.
#[test]
fn collapsing_a_selection_mid_drag_raises_no_caret_handle() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Move,
        source.point_of(5),
        &mut ctx,
        &mut source,
    );

    assert!(source.selection().is_empty(), "the drag met the fixed end");
    assert!(
        !controller
            .handles()
            .iter()
            .any(|h| h.kind == SelectionHandleKind::Caret),
        "a caret handle appeared under the finger: {:?}",
        controller.handles()
    );
}

/// Cancelling a drag leaves whatever the last sample selected — the selection
/// is not a transaction — but takes the magnifier down and puts the toolbar
/// back, which is what tells the user the gesture is over.
#[test]
fn cancelling_a_drag_retires_the_magnifier_and_restores_the_toolbar() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );
    assert!(controller.magnifier_request().is_some());
    assert!(controller.toolbar().is_none());

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Cancel,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );

    assert!(controller.magnifier_request().is_none());
    assert!(controller.toolbar().is_some());
    assert!(!controller.is_dragging());
}

/// A press on a handle is claimed by the controller; a press on bare text is
/// not, so the host's own caret placement runs unchanged.
#[test]
fn a_press_on_a_handle_is_claimed_and_a_press_on_text_is_not() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    let handle = controller
        .handles()
        .into_iter()
        .find(|h| h.kind == SelectionHandleKind::End)
        .expect("end handle");

    assert_eq!(
        controller.handle_pointer(&down(handle.anchor), &mut ctx, &mut source),
        EventResponse::Handled
    );
    assert!(controller.is_dragging());

    let mut fresh = TouchSelection::new();
    fresh.raise(LTR, &source);
    assert_eq!(
        fresh.handle_pointer(&down(source.point_of(1)), &mut ctx, &mut source),
        EventResponse::Ignored
    );
    assert!(!fresh.is_dragging());
}

// ---------------------------------------------------------------------------
// RTL
// ---------------------------------------------------------------------------

/// The physical side of a handle is resolved once, through
/// `SelectionHandleKind::side`, so the mapping cannot be applied twice or drift
/// from the hit test's idea of it.
#[test]
fn handles_map_to_physical_sides_by_layout_direction() {
    let source = FakeText::new().selected(5..10);
    for (direction, start_side, end_side) in [
        (LTR, HorizontalSide::Left, HorizontalSide::Right),
        (RTL, HorizontalSide::Right, HorizontalSide::Left),
    ] {
        let mut controller = TouchSelection::new();
        controller.raise(direction, &source);
        let handles = controller.handles();
        let start = handles
            .iter()
            .find(|h| h.kind == SelectionHandleKind::Start)
            .expect("start handle");
        let end = handles
            .iter()
            .find(|h| h.kind == SelectionHandleKind::End)
            .expect("end handle");
        assert_eq!(start.side, Some(start_side), "start under {direction:?}");
        assert_eq!(end.side, Some(end_side), "end under {direction:?}");
    }
}

// ---------------------------------------------------------------------------
// The toolbar
// ---------------------------------------------------------------------------

#[test]
fn the_toolbar_hangs_above_the_selection_it_acts_on() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let toolbar = controller.toolbar().expect("a selection has a toolbar");
    let bounds = source.selection_bounds().expect("selection bounds");
    assert_eq!(toolbar.anchor, bounds);
    match toolbar.placement() {
        OverlayPlacement::AboveSelection { selection } => assert_eq!(selection, bounds),
        other => panic!("expected AboveSelection, got {other:?}"),
    }
}

#[test]
fn dismissing_the_controller_retires_every_affordance() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    assert!(controller.toolbar().is_some() && !controller.handles().is_empty());

    controller.dismiss();

    assert!(controller.toolbar().is_none());
    assert!(controller.handles().is_empty());
    assert!(controller.affordances().is_empty());
}

/// After a dismissal the controller stays quiet until it is raised again — a
/// refresh driven by an unrelated selection change must not bring the handles
/// back on its own.
#[test]
fn a_refresh_after_dismissal_raises_nothing() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    controller.dismiss();

    controller.refresh(LTR, &source);

    assert!(controller.handles().is_empty());
    assert!(controller.toolbar().is_none());
}

/// A surface that refuses edits can only be copied from, and offers no caret
/// handle because the user cannot move its caret — so no Cut, no Paste, no
/// Select All, and nothing to drag where the caret is.
///
/// Selecting is not editing, though: a read-only surface with a range still
/// gets both selection handles, which is what lets a finger widen a selection
/// in a terminal or a log view.
#[test]
fn a_read_only_surface_offers_a_copy_only_toolbar_and_no_caret_handle() {
    let source = FakeText::new().read_only().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let toolbar = controller.toolbar().expect("a copy toolbar");
    assert_eq!(toolbar.actions, vec![TextAction::Copy]);
    let kinds: Vec<_> = controller.handles().into_iter().map(|h| h.kind).collect();
    assert_eq!(
        kinds,
        vec![SelectionHandleKind::Start, SelectionHandleKind::End],
        "a read-only surface must still offer the handles that adjust a \
         selection"
    );

    let empty = FakeText::new().read_only().selected(4..4);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &empty);
    assert!(
        controller.handles().is_empty(),
        "a read-only surface must not offer a caret handle"
    );
}

/// A password field's contents may not leave the process, so neither Cut nor
/// Copy is offered even though there is a selection.
#[test]
fn a_surface_that_forbids_copying_offers_neither_cut_nor_copy() {
    let source = FakeText::new().secret().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let toolbar = controller.toolbar().expect("a paste toolbar");
    assert_eq!(toolbar.actions, vec![TextAction::Paste]);
}

#[test]
fn an_editable_caret_offers_paste_and_select_all() {
    let source = FakeText::new().selected(3..3);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let toolbar = controller.toolbar().expect("a caret toolbar");
    assert_eq!(
        toolbar.actions,
        vec![TextAction::Paste, TextAction::SelectAll]
    );
}

// ---------------------------------------------------------------------------
// Reachability
// ---------------------------------------------------------------------------

/// A handle hangs below the line it marks — except on the last line of a
/// surface whose bottom is the window's, where hanging below would put it off
/// screen. Then it flips above the line instead.
#[test]
fn a_handle_below_the_last_line_flips_above_it() {
    // Viewport exactly three lines tall, caret on the last one.
    let source = FakeText::new()
        .with_viewport(Rect::new(0.0, 0.0, 200.0, 3.0 * LINE_HEIGHT))
        .selected(45..45);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let handle = controller
        .handles()
        .into_iter()
        .next()
        .expect("caret handle");
    let caret = source.caret_rect(45);
    assert!(
        handle.anchor.y < caret.y,
        "handle at {:?} should have flipped above the caret {caret:?}",
        handle.anchor
    );
    assert!(
        handle.visual.bottom() <= source.viewport().bottom() + 1e-3,
        "the disc must stay on screen, got {:?}",
        handle.visual
    );
}

/// At the leading edge the hit square hangs half off screen. It is nudged back
/// — but only as far as it can go without losing the disc it belongs to — and
/// what remains on screen still clears the WCAG 2.2 minimum target size.
#[test]
fn a_handle_at_the_viewport_edge_keeps_a_conformant_target() {
    let source = FakeText::new().selected(0..0);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let handle = controller
        .handles()
        .into_iter()
        .next()
        .expect("caret handle");
    let viewport = source.viewport();
    let visible_width = handle.hit.right().min(viewport.right()) - handle.hit.x.max(viewport.x);
    let floor = InputTokens::default().min_target_conformance;
    assert!(
        visible_width >= floor,
        "only {visible_width} dp of the handle's target is on screen, below the {floor} dp floor"
    );
    assert!(
        handle.hit.contains(handle.anchor),
        "the nudge moved the target off the disc it belongs to"
    );
}

/// A caret scrolled out of the surface's viewport has no handle at all —
/// leaving one behind would put a live target over unrelated content.
#[test]
fn a_caret_scrolled_out_of_view_has_no_handle() {
    let source = FakeText::new()
        .with_viewport(Rect::new(0.0, 100.0, 200.0, 40.0))
        .selected(0..0);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    assert!(controller.handles().is_empty());
}

/// Neither of a handle's two dimensions moves with density, and they hold
/// still for different reasons.
///
/// The disc is a `Decoration`, which `dp` passes through untouched. The target
/// is a `Target`, which `dp` raises to the density's target size — but the
/// handle is already specified at the size the *tallest* rung asks for, so the
/// floor never bites and the shipped value stands at every rung. That is the
/// intent: a handle is dragged by a fingertip whatever the rest of the UI is
/// sized for, and a Compact build must not offer a smaller one.
///
/// Both are pinned as equalities rather than as "may only grow": an inequality
/// against a constant is satisfied by the constant, so it would assert nothing
/// the ladder could break. A theme that supplies its own
/// `TextSelectionHandleRecipe` is a different door and is not covered here.
#[test]
fn density_moves_neither_a_handles_target_nor_its_disc() {
    for density in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let metrics = HandleMetrics::for_tokens(&InputTokens::for_density(density));
        assert_eq!(
            metrics.diameter, HANDLE_DIAMETER,
            "the disc is a decoration, and {density:?} moved it"
        );
        assert_eq!(
            metrics.hit, HANDLE_HIT_SIZE,
            "the target already clears every rung's floor, and {density:?} \
             moved it"
        );
    }

    // …and the reason the floor never bites: the shipped target is not merely
    // above the *default* density's floor, it is at or above the tallest one's.
    // Were it below, the assertions above would be asserting the ladder rather
    // than the constant.
    let tallest = InputTokens::for_density(TargetDensity::Touch).target_size;
    assert!(
        HANDLE_HIT_SIZE >= tallest,
        "a handle specified below the tallest density's target ({tallest}) \
         would be widened by it"
    );
}

/// Two handles overlap when the selection is a character wide. The nearer one
/// wins, so the finger adjusts the end it is actually on.
#[test]
fn overlapping_handles_resolve_to_the_nearer_one() {
    let source = FakeText::new().selected(5..6);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let start = controller
        .handles()
        .into_iter()
        .find(|h| h.kind == SelectionHandleKind::Start)
        .expect("start handle");
    let end = controller
        .handles()
        .into_iter()
        .find(|h| h.kind == SelectionHandleKind::End)
        .expect("end handle");
    assert!(
        start.hit.contains(end.anchor),
        "this test only means something while the two targets overlap"
    );
    assert_eq!(
        controller.handle_at(end.anchor),
        Some(SelectionHandleKind::End)
    );
    assert_eq!(
        controller.handle_at(start.anchor),
        Some(SelectionHandleKind::Start)
    );
}

// ---------------------------------------------------------------------------
// The magnifier
// ---------------------------------------------------------------------------

#[test]
fn the_magnifier_follows_the_finger_and_goes_away_on_release() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );
    let first = controller
        .magnifier_request()
        .expect("magnifier during drag");

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Move,
        source.point_of(14),
        &mut ctx,
        &mut source,
    );
    let second = controller.magnifier_request().expect("magnifier still up");
    assert!(
        second.focus.x > first.focus.x,
        "the lens must follow the finger: {:?} then {:?}",
        first.focus,
        second.focus
    );

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::End,
        source.point_of(14),
        &mut ctx,
        &mut source,
    );
    assert!(controller.magnifier_request().is_none());
}

/// A lens that appears unbidden and then chases the finger is exactly the
/// movement `prefers-reduced-motion` is about, and the selection works without
/// it.
#[test]
fn reduced_motion_raises_no_magnifier() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new().reduced_motion(true);
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );

    assert!(controller.magnifier_request().is_none());
    assert!(
        !controller.handles().is_empty(),
        "the handles themselves must survive reduced motion"
    );
}

/// The per-surface opt-out, for a host whose text layer cannot be re-entered.
#[test]
fn the_magnifier_can_be_turned_off_per_surface() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new().magnifier(false);
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);

    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );

    assert!(controller.magnifier_request().is_none());
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

/// The magnifier's whole mechanism, as the display list sees it: the host's
/// painter is emitted a second time between a clip and its release, under a
/// transform that magnifies. A test cannot see the pixels — `MockTextBackend`
/// has fixed metrics and the glyph quads are only resolved in the renderer — so
/// what is asserted is the command stream.
#[test]
fn replay_emits_the_painter_between_a_clip_and_a_magnifying_transform() {
    use teksilo_canvas::{DrawCommand, Transform2D};

    let theme = crate::presets::intui::light();
    let ctx = PaintContext {
        theme: &theme,
        scale_factor: 1.0,
        text_scale: 1.0,
        layout_direction: LTR,
        effective_enabled: true,
        prefers_high_contrast: false,
        prefers_reduced_motion: false,
        prefers_large_text: false,
        window_active: true,
        clip_bounds: None,
    };
    let mut canvas = Canvas::new();
    let clip = Rect::new(10.0, 20.0, 96.0, 44.0);
    let transform = Transform2D::scale(1.25, 1.25);
    ctx.replay(
        &mut canvas,
        &|canvas, _ctx| canvas.fill_rect(Rect::new(0.0, 0.0, 4.0, 4.0), teksilo_tokens::Color::RED),
        transform,
        clip,
    );
    let frame = canvas.into_render_frame();
    frame.debug_validate_stacks();

    let clip_at = frame
        .draw_order
        .iter()
        .position(|c| matches!(c, DrawCommand::SetClip(r) if *r == clip))
        .expect("the lens clips to its own rectangle");
    let scale_at = frame
        .draw_order
        .iter()
        .position(|c| matches!(c, DrawCommand::SetTransform(t) if (t.geometric_scale() - 1.25).abs() < 1e-3))
        .expect("the replayed content is magnified");
    let paint_at = frame
        .draw_order
        .iter()
        .position(|c| matches!(c, DrawCommand::Decoration(_)))
        .expect("the painter ran");
    let release_at = frame
        .draw_order
        .iter()
        .position(|c| matches!(c, DrawCommand::ClearClip))
        .expect("the clip is released");

    assert!(
        clip_at < scale_at && scale_at < paint_at && paint_at < release_at,
        "clip {clip_at}, scale {scale_at}, paint {paint_at}, release {release_at}: {:?}",
        frame.draw_order
    );
}

/// The transform the lens widget builds puts the contact point at the middle of
/// the lens, whatever clamping did to the lens's own position.
#[test]
fn the_lens_transform_centres_the_contact_point() {
    let focus = Point::new(30.0, 120.0);
    let request = MagnifierRequest::new(
        focus,
        Rect::new(0.0, 0.0, 400.0, 300.0),
        magnifier::MAGNIFIER_RADIUS,
        magnifier::MAGNIFIER_HALF_HEIGHT,
        magnifier::MAGNIFIER_RISE,
        magnifier::MAGNIFIER_SCALE,
    );
    let mapped = request.transform().apply_point(focus);
    let centre = request.lens.center();
    assert!((mapped.x - centre.x).abs() < 1e-3 && (mapped.y - centre.y).abs() < 1e-3);
}

/// The transform the lens **paints** with is the one its request composes. There
/// is one copy of the rule — `TextMagnifier::paint` hands `request.transform()`
/// straight to `replay` — so the two tests above, which assert on the request,
/// are also assertions about the display list. Recomposing the same three steps
/// in the paint path (which is what shipped first) made them assertions about a
/// second copy nobody looked at.
///
/// Asserted on the command stream for the same reason as the replay test:
/// `MockTextBackend` has fixed metrics and glyph quads only resolve in the
/// renderer, so the transform is the only observable.
#[test]
fn the_lens_paints_with_the_transform_its_request_composes() {
    use crate::widget::Widget;
    use teksilo_canvas::DrawCommand;

    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx_events = touch_ctx(LTR);
    controller.raise(LTR, &source);
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx_events,
        &mut source,
    );
    let request = controller
        .magnifier_request()
        .expect("a magnifier while a handle is being dragged");

    let lens = TextMagnifier::new(
        controller.affordances(),
        test_magnifier_recipe(),
        Rc::new(|canvas: &mut Canvas, _ctx: &PaintContext<'_>| {
            canvas.fill_rect(Rect::new(0.0, 0.0, 4.0, 4.0), teksilo_tokens::Color::RED)
        }),
    );

    let theme = crate::presets::intui::light();
    let paint_ctx = PaintContext {
        theme: &theme,
        scale_factor: 1.0,
        text_scale: 1.0,
        layout_direction: LTR,
        effective_enabled: true,
        prefers_high_contrast: false,
        prefers_reduced_motion: false,
        prefers_large_text: false,
        window_active: true,
        clip_bounds: None,
    };
    // Painting at `request.lens` is only legitimate because that is where the
    // layer puts the node — the premise the paint path relies on when it takes
    // the centre from the request rather than from its own bounds. Checked in a
    // real arena so the assumption is not merely restated here.
    let mut layer_tree =
        crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let layer_delegate: Rc<dyn TextAffordanceDelegate> = Rc::new(RecordingDelegate::default());
    let layer = layer_tree.add(
        TextAffordanceLayer::new(
            controller.affordances(),
            test_handle_recipe(),
            test_magnifier_recipe(),
            layer_delegate,
        )
        .magnifier_painter(Rc::new(|_: &mut Canvas, _: &PaintContext<'_>| {})),
    );
    layer_tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(
        layer_tree
            .children(layer)
            .iter()
            .filter(|id| layer_tree.is_active(**id))
            .any(|id| {
                let b = layer_tree.bounds(*id);
                (b.x - request.lens.x).abs() < 1e-3
                    && (b.y - request.lens.y).abs() < 1e-3
                    && (b.width - request.lens.width).abs() < 1e-3
                    && (b.height - request.lens.height).abs() < 1e-3
            }),
        "no node was placed on the published lens {:?}",
        request.lens
    );

    let mut canvas = Canvas::new();
    lens.paint(request.lens, &mut canvas, &paint_ctx);
    let frame = canvas.into_render_frame();

    let painted = frame
        .draw_order
        .iter()
        .find_map(|c| match c {
            DrawCommand::SetTransform(t) => Some(*t),
            _ => None,
        })
        .expect("the lens replays under a transform");

    let mapped = painted.apply_point(request.focus);
    let centre = request.lens.center();
    assert!(
        (mapped.x - centre.x).abs() < 1e-3 && (mapped.y - centre.y).abs() < 1e-3,
        "the painted transform put the contact point at {mapped:?}, not at the \
         lens centre {centre:?}"
    );
    let expected = request.transform();
    let a = painted.apply_point(Point::new(0.0, 0.0));
    let b = expected.apply_point(Point::new(0.0, 0.0));
    assert!(
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3,
        "the painted transform is not the request's: {a:?} vs {b:?}"
    );
    assert!(
        (painted.geometric_scale() - request.scale).abs() < 1e-3,
        "the lens painted at scale {}, not {}",
        painted.geometric_scale(),
        request.scale
    );
}

// ---------------------------------------------------------------------------
// The affordance layer, in a real arena
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RecordingDelegate {
    drags: RefCell<Vec<(SelectionHandleKind, HandleDragPhase, Point)>>,
    offsets: RefCell<Vec<(SelectionHandleKind, usize)>>,
}

impl TextAffordanceDelegate for RecordingDelegate {
    fn handle_drag(
        &self,
        kind: SelectionHandleKind,
        phase: HandleDragPhase,
        point: Point,
        _ctx: &mut EventContext<'_>,
    ) {
        self.drags.borrow_mut().push((kind, phase, point));
    }

    fn set_handle_offset(
        &self,
        kind: SelectionHandleKind,
        offset: usize,
        _ctx: &mut EventContext<'_>,
    ) {
        self.offsets.borrow_mut().push((kind, offset));
    }
}

fn test_handle_recipe() -> crate::styles::TextSelectionHandleRecipe {
    crate::styles::TextSelectionHandleRecipe {
        diameter: HANDLE_DIAMETER,
        hit_size: HANDLE_HIT_SIZE,
        stem_width: HANDLE_STEM_WIDTH,
        fill: crate::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
        outline: crate::styles::RecipeColor::Static(teksilo_tokens::Color::WHITE),
        outline_width: 1.0,
    }
}

fn test_magnifier_recipe() -> crate::styles::TextMagnifierRecipe {
    crate::styles::TextMagnifierRecipe {
        radius: magnifier::MAGNIFIER_RADIUS,
        half_height: magnifier::MAGNIFIER_HALF_HEIGHT,
        scale: magnifier::MAGNIFIER_SCALE,
        rise: magnifier::MAGNIFIER_RISE,
        corner_radius: 12.0,
        background: crate::styles::RecipeColor::Static(teksilo_tokens::Color::WHITE),
        border: crate::styles::RecipeColor::Static(teksilo_tokens::Color::BLACK),
        border_width: 1.0,
    }
}

/// A handle is a slider to assistive technology, and the value it carries is
/// the text offset it marks — with a maximum, so the announcement is "character
/// N of M" rather than a bare number. `SetValue` is the whole AT contract; the
/// handle is deliberately not focusable, so no `Focus` action is advertised.
#[test]
fn a_handle_is_a_slider_that_accepts_set_value() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    let _ = &mut source;

    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let delegate: Rc<dyn TextAffordanceDelegate> = Rc::new(RecordingDelegate::default());
    tree.add(TextAffordanceLayer::new(
        controller.affordances(),
        test_handle_recipe(),
        test_magnifier_recipe(),
        delegate,
    ));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let update = tree.accessibility_tree_snapshot();
    let sliders: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == accesskit::Role::Slider)
        .collect();
    assert_eq!(sliders.len(), 2, "one slider per raised handle");
    for (_, node) in &sliders {
        assert!(
            node.supports_action(accesskit::Action::SetValue),
            "a handle must accept SetValue"
        );
        assert!(
            !node.supports_action(accesskit::Action::Focus),
            "a handle is outside the Tab ring, so it must not advertise Focus"
        );
        assert_eq!(node.max_numeric_value(), Some(source.document_len() as f64));
    }
    let values: Vec<_> = sliders
        .iter()
        .filter_map(|(_, node)| node.numeric_value())
        .collect();
    assert!(
        values.contains(&5.0) && values.contains(&10.0),
        "the sliders carry the selection's own offsets, got {values:?}"
    );

    // The layer is a bare container, which the walker prunes: it must not
    // become a traversal stop between the editor and its handles. Asserting the
    // absence of a *node* rather than of a role is what makes this fail if the
    // layer starts declaring something — any role at all would keep it.
    assert!(
        extra_stops(&update).is_empty(),
        "the affordance layer survived into the tree as a stop: {:?}",
        extra_stops(&update)
    );
}

/// The node id of the slider whose numeric value is `offset`, i.e. the handle
/// marking that text offset.
fn slider_node_at_offset(update: &accesskit::TreeUpdate, offset: f64) -> accesskit::NodeId {
    update
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == accesskit::Role::Slider && node.numeric_value() == Some(offset)
        })
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("no handle slider at offset {offset}"))
}

/// Advertising `SetValue` and honouring it are two different things, and the
/// advertisement alone is what the AT contract *looks* like from a snapshot. A
/// handle is deliberately not focusable, so `SetValue` is the only route a
/// screen reader has to one: this drives a real action request through the tree,
/// at the id the published snapshot names, and asserts the delegate was asked to
/// move that end.
///
/// Both `ActionData` shapes an adapter may send are covered — AccessKit carries
/// a value as either a number or a string — and so is malformed data, which must
/// be refused rather than coerced into offset zero: silently jumping the
/// selection to the top of the document is worse than doing nothing.
#[test]
fn a_set_value_action_on_a_handle_moves_that_end() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let recorder = Rc::new(RecordingDelegate::default());
    let delegate: Rc<dyn TextAffordanceDelegate> = recorder.clone();
    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    tree.add(TextAffordanceLayer::new(
        controller.affordances(),
        test_handle_recipe(),
        test_magnifier_recipe(),
        delegate,
    ));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let end_node = slider_node_at_offset(&tree.accessibility_tree_snapshot(), 10.0);
    let mut ops = crate::window::NoopWindowOps;

    assert!(
        tree.dispatch_access_action(
            end_node,
            accesskit::Action::SetValue,
            Some(accesskit::ActionData::NumericValue(7.0)),
            &mut ops,
        ),
        "the handle refused a SetValue it advertises"
    );
    assert_eq!(
        recorder.offsets.borrow().as_slice(),
        [(SelectionHandleKind::End, 7)],
        "the numeric form did not reach the delegate as offset 7"
    );

    recorder.offsets.borrow_mut().clear();
    assert!(tree.dispatch_access_action(
        end_node,
        accesskit::Action::SetValue,
        Some(accesskit::ActionData::Value("3".into())),
        &mut ops,
    ));
    assert_eq!(
        recorder.offsets.borrow().as_slice(),
        [(SelectionHandleKind::End, 3)],
        "the string form did not reach the delegate as offset 3"
    );

    // A negative offset cannot exist in a document, so it clamps to the start
    // rather than wrapping through `as usize`.
    recorder.offsets.borrow_mut().clear();
    assert!(tree.dispatch_access_action(
        end_node,
        accesskit::Action::SetValue,
        Some(accesskit::ActionData::NumericValue(-4.0)),
        &mut ops,
    ));
    assert_eq!(
        recorder.offsets.borrow().as_slice(),
        [(SelectionHandleKind::End, 0)]
    );

    recorder.offsets.borrow_mut().clear();
    for data in [
        Some(accesskit::ActionData::Value("not an offset".into())),
        Some(accesskit::ActionData::ScrollUnit(
            accesskit::ScrollUnit::Item,
        )),
        None,
    ] {
        assert!(
            !tree.dispatch_access_action(
                end_node,
                accesskit::Action::SetValue,
                data.clone(),
                &mut ops
            ),
            "SetValue with {data:?} was accepted"
        );
    }
    assert!(
        recorder.offsets.borrow().is_empty(),
        "malformed data reached the delegate: {:?}",
        recorder.offsets.borrow()
    );

    // An action the handle does not advertise is not silently absorbed either:
    // the node is a slider, and a caller that mistook it for one it can step
    // must learn that it cannot.
    assert!(
        !tree.dispatch_access_action(end_node, accesskit::Action::Increment, None, &mut ops),
        "the handle claimed an action it does not advertise"
    );
}

/// Roles in `update` that are neither a handle nor part of the tree's own
/// furniture, and are not hidden — i.e. everything the affordance layer adds
/// that a screen reader would stop on.
fn extra_stops(update: &accesskit::TreeUpdate) -> Vec<accesskit::Role> {
    update
        .nodes
        .iter()
        .filter(|(_, node)| {
            !matches!(
                node.role(),
                accesskit::Role::Window
                    | accesskit::Role::Slider
                    | accesskit::Role::Status
                    | accesskit::Role::Alert
            )
        })
        .filter(|(_, node)| !node.is_hidden())
        .map(|(_, node)| node.role())
        .collect()
}

/// The lens shows what is already in the tree; announcing it would read the
/// same sentence twice. Its node is emitted — the walker emits one per active
/// widget — but marked hidden, which is what keeps it out of navigation and
/// hit-testing.
#[test]
fn a_raised_magnifier_is_hidden_from_assistive_technology() {
    let mut source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    let mut ctx = touch_ctx(LTR);
    controller.raise(LTR, &source);
    controller.drag_handle(
        SelectionHandleKind::End,
        HandleDragPhase::Begin,
        source.point_of(10),
        &mut ctx,
        &mut source,
    );
    assert!(
        controller.magnifier_request().is_some(),
        "this test only means something with a lens up"
    );

    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let delegate: Rc<dyn TextAffordanceDelegate> = Rc::new(RecordingDelegate::default());
    tree.add(
        TextAffordanceLayer::new(
            controller.affordances(),
            test_handle_recipe(),
            test_magnifier_recipe(),
            delegate,
        )
        .magnifier_painter(Rc::new(|_canvas, _ctx| {})),
    );
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let update = tree.accessibility_tree_snapshot();
    // Every node that is neither a handle nor one of the tree's own live
    // regions must be hidden — which is exactly the lens, and is the assertion
    // that fails if `set_hidden` goes.
    let visible_extras = extra_stops(&update);
    assert!(
        visible_extras.is_empty(),
        "the lens reached the tree unhidden: {visible_extras:?}"
    );
    assert_eq!(
        update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == accesskit::Role::Slider)
            .count(),
        2,
        "both handles are still announced"
    );
}

/// The layer places each handle exactly on the target the controller published,
/// so what the pointer meets and what the audit measures are the same rectangle.
#[test]
fn the_layer_places_each_handle_on_its_published_target() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let delegate: Rc<dyn TextAffordanceDelegate> = Rc::new(RecordingDelegate::default());
    let layer = tree.add(TextAffordanceLayer::new(
        controller.affordances(),
        test_handle_recipe(),
        test_magnifier_recipe(),
        delegate,
    ));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let published = controller.handles();
    let placed: Vec<Rect> = tree
        .children(layer)
        .iter()
        .filter(|id| tree.is_active(**id))
        .map(|id| tree.bounds(*id))
        .collect();
    assert_eq!(placed.len(), published.len());
    for handle in &published {
        assert!(
            placed
                .iter()
                .any(|r| (r.x - handle.hit.x).abs() < 1e-3 && (r.y - handle.hit.y).abs() < 1e-3),
            "no node placed at {:?}; placed {placed:?}",
            handle.hit
        );
    }
}

/// Only the handles the controller wants are mounted: a caret handle parked
/// while a range is selected must be dormant, or it would take presses over
/// text it does not mark.
#[test]
fn a_handle_the_controller_does_not_want_is_dormant() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);

    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let delegate: Rc<dyn TextAffordanceDelegate> = Rc::new(RecordingDelegate::default());
    let layer = tree.add(TextAffordanceLayer::new(
        controller.affordances(),
        test_handle_recipe(),
        test_magnifier_recipe(),
        delegate,
    ));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    let active = tree
        .children(layer)
        .iter()
        .filter(|id| tree.is_active(**id))
        .count();
    assert_eq!(
        active, 2,
        "the caret handle must be parked while a range is up"
    );
}

// ---------------------------------------------------------------------------
// The two overlay doors this package needed
// ---------------------------------------------------------------------------

/// Raising an affordance layer means naming a z-band, and there was no route to
/// one from an `EventContext`: `show_overlay` names no band, and the only
/// band-aware entry point on the tree had no production caller.
#[test]
fn a_handler_can_raise_an_overlay_in_the_text_affordance_band() {
    let selection = Rect::new(10.0, 10.0, 30.0, 20.0);
    let (tree, content) = raise_in_band(move |ctx, content| {
        ctx.show_overlay_in_band(
            band_request(content, OverlayPlacement::AboveSelection { selection }),
            crate::overlay::OverlayBand::TextAffordance,
        );
    });

    let overlay = shown(&tree, content).expect("the handler's overlay is up");
    assert_eq!(overlay.0, crate::overlay::OverlayBand::TextAffordance);
    match overlay.1 {
        OverlayPlacement::AboveSelection { selection: s } => assert_eq!(s, selection),
        other => panic!("expected AboveSelection, got {other:?}"),
    }
}

/// A caret-anchored overlay has to be re-placed as the caret moves, and
/// `position_overlays` re-reads the placement the overlay was shown with — so
/// without a re-place door an affordance follows nothing. The door is
/// content-keyed because `show_overlay` returns no id for a handler to keep.
///
/// Doing both in one dispatch also pins the drain order: a handler may raise an
/// overlay and place it in the same breath.
#[test]
fn a_handler_can_re_place_an_overlay_it_only_knows_by_content() {
    let first = Rect::new(10.0, 10.0, 30.0, 20.0);
    let second = Rect::new(60.0, 40.0, 30.0, 20.0);
    let (tree, content) = raise_in_band(move |ctx, content| {
        ctx.show_overlay_in_band(
            band_request(
                content,
                OverlayPlacement::AboveSelection { selection: first },
            ),
            crate::overlay::OverlayBand::TextAffordance,
        );
        ctx.update_overlay_placement_by_content(
            content,
            OverlayPlacement::AboveSelection { selection: second },
        );
    });

    let overlay = shown(&tree, content).expect("the handler's overlay is up");
    match overlay.1 {
        OverlayPlacement::AboveSelection { selection: s } => assert_eq!(s, second),
        other => panic!("expected AboveSelection, got {other:?}"),
    }
}

fn band_request(content: WidgetId, placement: OverlayPlacement) -> crate::overlay::OverlayRequest {
    crate::overlay::OverlayRequest {
        content_id: content,
        anchor: content,
        placement,
        dismiss: crate::overlay::DismissBehavior::Manual,
        layer: crate::overlay::OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
        fade_duration: None,
    }
}

/// Build a tree whose only interactive widget runs `act` on a tap, tap it once,
/// and hand back the tree plus the widget id the overlay content lives at.
fn raise_in_band(
    act: impl Fn(&mut EventContext<'_>, WidgetId) + 'static,
) -> (crate::widget_tree::WidgetTree, WidgetId) {
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let content = tree.add(FillWidget::new());
    tree.add(FillWidget::new().on_tap(move |_event, ctx| act(ctx, content)));
    tree.layout(SizeProposal::exact(200.0, 100.0));

    tree.dispatch_event(down(Point::new(50.0, 25.0)));
    tree.dispatch_event(up(Point::new(50.0, 25.0)));
    (tree, content)
}

/// The band and placement of the overlay showing `content`, if one is up.
fn shown(
    tree: &crate::widget_tree::WidgetTree,
    content: WidgetId,
) -> Option<(crate::overlay::OverlayBand, OverlayPlacement)> {
    let id = tree.overlay_manager.find_by_content(content)?;
    let overlay = tree.overlay_manager.overlay(id)?;
    Some((overlay.band, overlay.placement.clone()))
}

/// A finger on a handle node reaches the delegate, and a **mouse** on the same
/// node does not: handles are only ever raised by a direct pointer, but on a
/// hybrid machine a mouse can arrive afterwards, and swallowing that press
/// would cost the caret placement it was asking for.
#[test]
fn a_finger_on_a_handle_node_drags_it_and_a_mouse_falls_through() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    let handle = controller
        .handles()
        .into_iter()
        .find(|h| h.kind == SelectionHandleKind::End)
        .expect("end handle");

    let recorder = Rc::new(RecordingDelegate::default());
    let delegate: Rc<dyn TextAffordanceDelegate> = recorder.clone();
    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    tree.add(TextAffordanceLayer::new(
        controller.affordances(),
        test_handle_recipe(),
        test_magnifier_recipe(),
        delegate,
    ));
    tree.layout(SizeProposal::exact(200.0, 60.0));

    // The mouse first, so a later touch cannot be credited to it.
    tree.dispatch_event(down(handle.anchor));
    tree.dispatch_event(up(handle.anchor));
    assert!(
        recorder.drags.borrow().is_empty(),
        "a mouse press reached the delegate: {:?}",
        recorder.drags.borrow()
    );

    let contact = tree.new_contact();
    tree.touch_down(contact, handle.anchor);
    tree.touch_move(contact, Point::new(handle.anchor.x + 40.0, handle.anchor.y));
    tree.touch_up(contact, Point::new(handle.anchor.x + 40.0, handle.anchor.y));

    let phases: Vec<_> = recorder
        .drags
        .borrow()
        .iter()
        .map(|(kind, phase, _)| (*kind, *phase))
        .collect();
    assert!(
        phases.contains(&(SelectionHandleKind::End, HandleDragPhase::Begin))
            && phases.contains(&(SelectionHandleKind::End, HandleDragPhase::End)),
        "a finger must drive the handle it landed on, got {phases:?}"
    );
}

/// A handle's target is wider than the disc, so it shadows the text under it —
/// and only a finger's press there is claimed.
///
/// This characterises the widening the affordance layer costs rather than
/// asserting it is small: the target is measured in characters of the fixture's
/// text, and a press aimed two characters off the anchor — a point the host's
/// own hit source reads as text other than the edge the handle belongs to — is
/// followed for each device.
///
/// * A **finger** is claimed: the handle takes the pointer's capture and drives
///   its drag. That is the point of a target larger than the disc — a fingertip
///   does not land on the pixel it aimed at, and a handle answering only its own
///   centre would be undraggable.
/// * A **mouse** is declined: nothing in the layer takes the capture, so the
///   press is still there to be answered.
///
/// What "declined" is *not* visible in is the delivery. The content root above
/// the handles — the node the layer's own docs tell a host to put its
/// cursor-click arm on, and what the `StackWidget` here stands in for — is
/// offered the press whichever device made it. So a host cannot read "the
/// handles passed on this one" off the fact that it arrived; the capture is what
/// says so.
#[test]
fn a_handles_target_shadows_text_and_only_a_finger_claims_it() {
    let source = FakeText::new().selected(5..10);
    let mut controller = TouchSelection::new();
    controller.raise(LTR, &source);
    let handle = controller
        .handles()
        .into_iter()
        .find(|h| h.kind == SelectionHandleKind::End)
        .expect("end handle");

    // The measurement: whole characters either side of the anchor lie inside the
    // target, so this is a run of text and not a rounding error.
    for dx in [-2.0, -1.0, 1.0, 2.0] {
        let at = Point::new(handle.anchor.x + dx * CHAR_WIDTH, handle.anchor.y);
        assert!(
            handle.hit.contains(at),
            "{dx} characters off the anchor is outside the target, so the \
             target shadows less text than this test characterises: {:?}",
            handle.hit
        );
    }

    // Two characters off, and what the host's own hit source reads there is not
    // the edge the handle belongs to. That is the widening.
    let shadowed = Point::new(handle.anchor.x - 2.0 * CHAR_WIDTH, handle.anchor.y);
    assert_ne!(
        source.offset_at(shadowed),
        handle.offset,
        "the target covers only its own edge, so there is no shadowed text \
         to characterise"
    );

    let recorder = Rc::new(RecordingDelegate::default());
    let delegate: Rc<dyn TextAffordanceDelegate> = recorder.clone();
    let mut tree = crate::widget_tree::WidgetTree::new().with_theme(crate::presets::intui::light());
    let layer = tree.add(TextAffordanceLayer::new(
        controller.affordances(),
        test_handle_recipe(),
        test_magnifier_recipe(),
        delegate,
    ));
    let offered_to_root: Rc<std::cell::Cell<usize>> = Default::default();
    {
        use crate::widget_builder::WidgetBuilder;
        let offered_to_root = offered_to_root.clone();
        tree.add(
            crate::test_widgets::StackWidget::new()
                .child(layer)
                .on_pointer_event(move |event, _ctx| {
                    if matches!(event, WidgetEvent::PointerDown { .. }) {
                        offered_to_root.set(offered_to_root.get() + 1);
                    }
                    EventResponse::Ignored
                }),
        );
    }
    tree.layout(SizeProposal::exact(200.0, 60.0));

    // The mouse first, so a later touch cannot be credited to it.
    tree.dispatch_event(down(shadowed));
    assert_eq!(
        tree.pointer_captured_by(),
        None,
        "a mouse press inside the target was claimed by the layer"
    );
    tree.dispatch_event(up(shadowed));
    assert!(
        recorder.drags.borrow().is_empty(),
        "a mouse press inside the target reached the handle: {:?}",
        recorder.drags.borrow()
    );
    assert_eq!(
        offered_to_root.get(),
        1,
        "the declined press did not reach the content root, so there is \
         nothing for a host's arm to answer"
    );

    let contact = tree.new_contact();
    tree.touch_down(contact, shadowed);
    let captor = tree
        .captured_by(contact)
        .expect("a finger inside the target must be claimed");
    assert_eq!(
        tree.bounds(captor),
        handle.hit,
        "the finger was claimed by something other than the handle whose \
         target it landed in"
    );
    tree.touch_up(contact, shadowed);
    let phases: Vec<_> = recorder
        .drags
        .borrow()
        .iter()
        .map(|(kind, phase, _)| (*kind, *phase))
        .collect();
    assert!(
        phases.contains(&(SelectionHandleKind::End, HandleDragPhase::Begin)),
        "a finger two characters off the anchor must still drive the handle, \
         got {phases:?}"
    );
    assert_eq!(
        offered_to_root.get(),
        2,
        "the content root is offered the press whichever device made it — a \
         host's arm there cannot read a decline off its arrival"
    );
}

// ---------------------------------------------------------------------------
// Which drag samples moved a caret
// ---------------------------------------------------------------------------

/// The predicate every host asks before reporting the IME cursor area, checked
/// over its whole domain rather than on the two cases the wiring happens to
/// exercise.
///
/// Three claims, and each is the reason a shipped host does what it does:
/// a caret drag reports on every phase that writes a selection; a range drag
/// never reports, because it chooses a range rather than an insertion point and
/// the editors' own reporters read a cursor that `set_selection` leaves on the
/// range's upper end; and `Cancel` never reports, because `cancel_drag` drops
/// the drag and recomputes geometry without moving anything.
#[test]
fn only_a_caret_drag_that_wrote_a_selection_moved_the_caret() {
    use crate::text_touch::drag_moves_the_caret;

    let writes = [
        HandleDragPhase::Begin,
        HandleDragPhase::Move,
        HandleDragPhase::End,
    ];
    for phase in writes {
        assert!(
            drag_moves_the_caret(SelectionHandleKind::Caret, phase),
            "a caret drag's {phase:?} writes a selection and so moves the caret"
        );
    }
    assert!(
        !drag_moves_the_caret(SelectionHandleKind::Caret, HandleDragPhase::Cancel),
        "Cancel drops the drag without writing a selection"
    );

    for kind in [SelectionHandleKind::Start, SelectionHandleKind::End] {
        for phase in [
            HandleDragPhase::Begin,
            HandleDragPhase::Move,
            HandleDragPhase::End,
            HandleDragPhase::Cancel,
        ] {
            assert!(
                !drag_moves_the_caret(kind, phase),
                "{kind:?} chooses a range, not an insertion point ({phase:?})"
            );
        }
    }
}

/// The three phases the predicate accepts are exactly the three that reach
/// `update_drag`, so the predicate cannot drift from the controller it
/// describes: a phase added to `HandleDragPhase` fails to compile here until it
/// is classified.
#[test]
fn every_drag_phase_is_classified() {
    for phase in [
        HandleDragPhase::Begin,
        HandleDragPhase::Move,
        HandleDragPhase::End,
        HandleDragPhase::Cancel,
    ] {
        let writes_a_selection = match phase {
            // `begin_drag` ends with an `update_drag`; `end_drag` begins with
            // one; `update_drag` is `Move` itself.
            HandleDragPhase::Begin | HandleDragPhase::Move | HandleDragPhase::End => true,
            // `cancel_drag` clears the drag and refreshes geometry.
            HandleDragPhase::Cancel => false,
        };
        assert_eq!(
            crate::text_touch::drag_moves_the_caret(SelectionHandleKind::Caret, phase),
            writes_a_selection,
            "{phase:?}",
        );
    }
}
