// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch selection for the single-line stack: the [`TextHitSource`] this
//! surface answers with, and the controller mount that owns its affordances.
//!
//! One adoption here serves every widget built on
//! [`TextInputField`](super::TextInputField) — `TextInput`, `PasswordField`,
//! `SearchField`, `HexColorInput`, `FilePickerField`, `SpinBox`, `DateEdit`,
//! `TimeEdit`, `DateTimeEdit`, `DateRangeEdit` and everything composed from
//! those.
//!
//! # Coordinates
//!
//! [`teksilo_core::text_touch`] is stated entirely in **window** coordinates,
//! and the router hands a handler **widget-local** ones: `localize_event`
//! rewrites every pointer position and every `TapEvent` through
//! `WidgetArena::local_pointer_position` before the target sees it. So a host
//! that forwards what it was given puts each affordance one viewport origin
//! away from the text it marks.
//!
//! Two conversions, and they are not interchangeable:
//!
//! * **A pointer sample** uses [`EventContext::pointer_position`], which is the
//!   window position of *the sample being dispatched*. This is the only source
//!   that stays correct for the affordance nodes: those are placed on the
//!   handle geometry, so they **move while they are being dragged**, and a
//!   local-plus-origin conversion would have to guess which placement the
//!   router localized against — the published geometry can already be a sample
//!   ahead of the arena bounds when two moves land in one frame.
//!   `tree_pointer_position` is *not* a substitute: it reports the table's
//!   elected primary, which prefers the mouse, so on a machine with both it
//!   answers for the wrong device.
//! * **A long press** has no sample — it is recognised by a timer, and
//!   `pointer_position` is `None` there — so it converts `local + viewport_origin`
//!   instead. That is exact, because the *field* does not move mid-press, and it
//!   is the arithmetic
//!   [`reposition_caret_for_context_menu`](super::mouse::reposition_caret_for_context_menu)
//!   already uses for the window-space right-click position.
//!
//! Neither conversion carries a transform term, so a field under a scene or
//! zoom transform reports affordance geometry in the untransformed space —
//! exactly as this stack's two existing window-space paths (the OS IME
//! candidate area and the context-menu caret) already do.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::overlay::{
    DismissBehavior, OverlayBand, OverlayLayer, OverlayPlacement, OverlayRequest,
    SelectionHandleKind,
};
use teksilo_core::signal::Signal;
use teksilo_core::text_touch::{
    HandleDragPhase, SelectionHandleGeometry, TextAffordanceDelegate, TextHitSource, TouchSelection,
};
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;
use teksilo_text::CursorAffinity;
use teksilo_text::text_document::{MoveMode, SelectionType};

use super::state::{SharedState, TextInputState, sync_cursor_signals_in};

/// Which of the two overlays a dismissal callback belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DismissedOverlay {
    Layer,
    Toolbar,
}

/// What a raise or a refresh should do about the selection toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolbarIntent {
    /// Put it up — a deliberate selection was just made.
    Show,
    /// Take it down — the caret merely moved, or the text changed under it.
    Hide,
    /// Leave it as it is — the geometry moved and nothing else did.
    Keep,
}

/// One text field, as the touch controller needs to see it.
///
/// Borrows the state rather than living on it: every controller entry point
/// wants `&mut TouchSelection` and `&mut dyn TextHitSource` at once, so a
/// controller stored *inside* `TextInputState` would need two overlapping
/// borrows of one `RefCell` on every call.
pub(crate) struct FieldHitSource<'a> {
    st: &'a mut TextInputState,
}

impl<'a> FieldHitSource<'a> {
    pub(crate) fn new(st: &'a mut TextInputState) -> Self {
        Self { st }
    }

    fn doc_len(&self) -> usize {
        self.st
            .document
            .to_plain_text()
            .unwrap_or_default()
            .chars()
            .count()
    }
}

/// The vertical middle of the single line the text sits on.
///
/// Affordances hang deliberately *off* that line — a handle's disc sits a dozen
/// dp below the descender — so hit-testing a point at its own `y` would miss the
/// glyph row for every `x` and fall back to the document end for all of them. A
/// one-line surface has no vertical information to lose.
pub(crate) fn line_middle_y(st: &TextInputState) -> f32 {
    let caret = st.engine.caret_rect(0, CursorAffinity::Downstream);
    caret[1] + caret[3] / 2.0
}

/// The window rectangle of the caret at `offset`.
///
/// The engine answers in its own space — `x` in content space with no
/// horizontal scroll applied, `y` already screen-relative — so both the OS IME
/// candidate area and the touch affordances need this exact composition. It
/// lives here, once, so the two cannot drift.
pub(crate) fn caret_rect_in_window(st: &TextInputState, offset: usize) -> Rect {
    let caret = st.engine.caret_rect(offset, CursorAffinity::Downstream);
    Rect::new(
        st.viewport_origin.x + caret[0] - st.scroll_x,
        st.viewport_origin.y + caret[1],
        caret[2].max(1.0),
        caret[3],
    )
}

/// A window point in the field's own space.
pub(crate) fn window_to_local(st: &TextInputState, point: Point) -> Point {
    Point::new(
        point.x - st.viewport_origin.x,
        point.y - st.viewport_origin.y,
    )
}

impl TextHitSource for FieldHitSource<'_> {
    fn offset_at(&self, point: Point) -> usize {
        let mut local = window_to_local(self.st, point);
        local.y = line_middle_y(self.st);
        match super::mouse::hit_test_in(self.st, local) {
            Some(offset) => offset,
            // `hit_test_in` answers `None` only for a point outside the
            // editable strip. The controller's contract has no "nowhere", and
            // an affordance drag routinely leaves the field, so clamp to the
            // end the point is nearest.
            None if local.x < 0.0 => 0,
            None => self.doc_len(),
        }
    }

    fn caret_rect(&self, offset: usize) -> Rect {
        caret_rect_in_window(self.st, offset)
    }

    fn word_range_at(&self, offset: usize) -> std::ops::Range<usize> {
        // A scratch cursor, not the field's own: `word_range_at` answers a
        // question and must not move the caret to do it. The document registers
        // cursors weakly and prunes dead ones, so it costs nothing to drop.
        //
        // Deliberately the same `SelectionType::WordUnderCursor` the
        // double-click path uses, so a hold and a double click cannot disagree
        // about where a word ends — and deliberately *not* the AccessKit
        // `compute_word_starts` table, which is an alphanumeric-plus-underscore
        // heuristic rather than UAX #29.
        let probe = self.st.document.cursor_at(offset.min(self.doc_len()));
        probe.select(SelectionType::WordUnderCursor);
        let (a, p) = (probe.anchor(), probe.position());
        a.min(p)..a.max(p)
    }

    fn line_range_at(&self, _offset: usize) -> std::ops::Range<usize> {
        0..self.doc_len()
    }

    fn selection(&self) -> std::ops::Range<usize> {
        let a = self.st.cursor.anchor();
        let p = self.st.cursor.position();
        a.min(p)..a.max(p)
    }

    fn set_selection(&mut self, range: std::ops::Range<usize>) {
        let len = self.doc_len();
        let start = range.start.min(len);
        let end = range.end.min(len);
        self.st.cursor.set_position(start, MoveMode::MoveAnchor);
        if end != start {
            self.st.cursor.set_position(end, MoveMode::KeepAnchor);
        }
        sync_cursor_signals_in(self.st);
    }

    fn selection_bounds(&self) -> Option<Rect> {
        let selection = self.selection();
        if selection.is_empty() {
            return None;
        }
        let start = caret_rect_in_window(self.st, selection.start);
        let end = caret_rect_in_window(self.st, selection.end);
        let x = start.x.min(end.x);
        let y = start.y.min(end.y);
        Some(Rect::new(
            x,
            y,
            (start.right().max(end.right()) - x).max(0.0),
            (start.bottom().max(end.bottom()) - y).max(0.0),
        ))
    }

    fn viewport(&self) -> Rect {
        // The *text* strip, not the whole box: a field with a non-editable
        // suffix ("%", "€") scrolls its text behind that strip and the caret
        // can never enter it, so a handle clamped into the full width could sit
        // over a character that does not exist.
        Rect::new(
            self.st.viewport_origin.x,
            self.st.viewport_origin.y,
            (self.st.viewport_width - self.st.suffix_width).max(0.0),
            self.st.viewport_height,
        )
    }

    fn document_len(&self) -> usize {
        self.doc_len()
    }

    fn is_editable(&self) -> bool {
        !self.st.read_only
    }

    /// The predicate the field's own `Ctrl+C` and context menu use, so the
    /// touch toolbar cannot offer a Copy the keyboard would refuse — or refuse
    /// one it allows.
    ///
    /// Note this is `copy_allowed()` and **not** the raw `allow_copy` opt-in
    /// that [`TextSurface::allows_copy`](teksilo_core::text_surface::TextSurface::allows_copy)
    /// reports for the same handle: that one answers `false` for a *revealed*
    /// password field whose keyboard copy works, a pre-existing divergence this
    /// deliberately does not inherit.
    fn allows_copy(&self) -> bool {
        self.st.copy_allowed()
    }
}

/// The touch-selection mount for one field: the controller, the ids of the two
/// overlays it raises, and the state they act on.
///
/// Minted with the widget rather than in `build()` so a composite can hold it
/// (and a test can read it) before the field is moved into the tree, and so the
/// handle survives a rebuild even though the controller inside it is replaced.
pub(crate) struct FieldTouch {
    pub(crate) controller: RefCell<TouchSelection>,
    /// The affordance layer's content root, in the text-affordance band.
    pub(crate) layer: Cell<Option<WidgetId>>,
    /// The selection toolbar's content root, in the standard band.
    pub(crate) toolbar: Cell<Option<WidgetId>>,
    /// The field itself — the overlays' anchor.
    pub(crate) field: Cell<Option<WidgetId>>,
    /// Whether the host wants the selection toolbar up.
    ///
    /// The host's intent, not the controller's: the controller offers a toolbar
    /// for any state with a command worth offering — including a bare caret,
    /// where it is Paste and Select All — and a menu popping open on every tap
    /// in a text field is not what any platform does. The toolbar belongs to a
    /// **deliberate** selection: a hold, a multi-tap, or the end of a handle
    /// drag. This one signal is both the overlay's `visible_when` gate and the
    /// condition for showing it, so the two cannot disagree.
    toolbar_wanted: Signal<bool>,
    /// The contact whose press a hold already spent.
    ///
    /// A hold fires from the gesture timer *before* the finger lifts, so the
    /// release that follows is the release of a press that has already been
    /// answered. Without this the release would place a caret and collapse the
    /// word the hold had just selected — every hold would end as a tap.
    hold_consumed: Cell<Option<teksilo_core::pointer::PointerId>>,
    slot: Rc<RefCell<Option<SharedState>>>,
}

impl std::fmt::Debug for FieldTouch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldTouch")
            .field("controller", &self.controller.borrow())
            .field("layer", &self.layer.get())
            .finish()
    }
}

impl FieldTouch {
    pub(crate) fn new(slot: Rc<RefCell<Option<SharedState>>>) -> Rc<Self> {
        Rc::new(Self {
            controller: RefCell::new(TouchSelection::new()),
            layer: Cell::new(None),
            toolbar: Cell::new(None),
            field: Cell::new(None),
            hold_consumed: Cell::new(None),
            toolbar_wanted: Signal::new(false),
            slot,
        })
    }

    /// Record that `pointer`'s press was spent by a hold.
    pub(crate) fn mark_hold_consumed(&self, pointer: teksilo_core::pointer::PointerId) {
        self.hold_consumed.set(Some(pointer));
    }

    /// Whether `pointer`'s press was spent by a hold, clearing the record.
    pub(crate) fn take_hold_consumed(&self, pointer: teksilo_core::pointer::PointerId) -> bool {
        if self.hold_consumed.get() == Some(pointer) {
            self.hold_consumed.set(None);
            return true;
        }
        false
    }

    fn state(&self) -> Option<SharedState> {
        self.slot.borrow().clone()
    }

    /// Run `f` with the controller and this field's hit source. `None` before
    /// the field is built.
    fn with_source<R>(
        &self,
        f: impl FnOnce(&mut TouchSelection, &mut FieldHitSource<'_>) -> R,
    ) -> Option<R> {
        let state = self.state()?;
        let mut st = state.borrow_mut();
        let mut source = FieldHitSource::new(&mut st);
        let mut controller = self.controller.borrow_mut();
        Some(f(&mut controller, &mut source))
    }

    /// Whether this surface has caret geometry worth hanging an affordance off.
    fn geometry_is_meaningful(&self) -> bool {
        self.state()
            .is_some_and(|state| state.borrow().caret_geometry_is_meaningful())
    }

    pub(crate) fn handles(&self) -> Vec<SelectionHandleGeometry> {
        self.controller.borrow().handles()
    }

    /// Raise the affordances for the current selection and mount their
    /// overlays.
    ///
    /// `toolbar` says what to do about the selection toolbar; see
    /// [`ToolbarIntent`].
    pub(crate) fn raise(self: &Rc<Self>, ctx: &mut EventContext<'_>, toolbar: ToolbarIntent) {
        if !self.geometry_is_meaningful() {
            self.dismiss();
            return;
        }
        let direction = ctx.layout_direction();
        self.with_source(|controller, source| controller.raise(direction, source));
        self.mount(ctx, toolbar);
    }

    /// Select the word at a window point and raise the affordances — the
    /// hold. Returns `false` when the controller declined, either because
    /// `pointer` is not a direct one or because this surface has no geometry to
    /// select against, so the caller can leave the gesture unhandled.
    ///
    /// The word selection itself is the controller's
    /// ([`TouchSelection::on_long_press`]), not a second copy of it here: the
    /// host contributes the two things core cannot know — the window point (the
    /// gesture's own position is widget-local) and the overlay mount.
    pub(crate) fn select_word_at(
        self: &Rc<Self>,
        pointer: teksilo_core::pointer::PointerInfo,
        point: Point,
        ctx: &mut EventContext<'_>,
    ) -> bool {
        if !self.geometry_is_meaningful() {
            self.dismiss();
            return false;
        }
        let handled = self.with_source(|controller, source| {
            controller.on_long_press(pointer, point, ctx, source)
        });
        if handled != Some(teksilo_core::event::EventResponse::Handled) {
            return false;
        }
        self.mount(ctx, ToolbarIntent::Show);
        true
    }

    /// Recompute the affordances after a change the controller did not make —
    /// a keystroke, an undo, an assistive-technology selection. A no-op until
    /// something has been raised, so an untouched field pays nothing.
    pub(crate) fn refresh(self: &Rc<Self>, ctx: &mut EventContext<'_>, toolbar: ToolbarIntent) {
        if !self.geometry_is_meaningful() {
            self.dismiss();
            return;
        }
        let direction = ctx.layout_direction();
        self.with_source(|controller, source| controller.refresh(direction, source));
        self.mount(ctx, toolbar);
    }

    /// Retract every affordance.
    ///
    /// The one mechanism: the controller publishes empty geometry, and every
    /// affordance node — the handles and the lens — is gated on that published
    /// state by `visible_when`, while the toolbar is gated on
    /// [`toolbar_wanted`](Self::toolbar_wanted). Nothing here dismisses an
    /// overlay, which is what lets the ctx-less paths (the window-active effect,
    /// the external text sync) retire the chrome at all.
    pub(crate) fn dismiss(&self) {
        // Guarded, like `hide_toolbar`: this is now reached from every mouse
        // press in the field, and a `Signal::set` notifies whether or not the
        // value moved. `TextAffordances::publish` already early-returns on an
        // unchanged state, so with this the whole call is free for a field that
        // has nothing raised.
        if self.toolbar_wanted.get() {
            self.toolbar_wanted.set(false);
        }
        self.controller.borrow_mut().dismiss();
    }

    /// Place the caret at a window point, as a press on the text does.
    ///
    /// [`AffordanceHost`]'s alone: it is what answers a **cursor's** click on a
    /// selection handle, which the field itself never sees (the handle is the
    /// hit target, and the field is not on its bubble path).
    pub(crate) fn place_caret_at(&self, window: Point, ctx: &mut EventContext<'_>) -> bool {
        let Some(state) = self.state() else {
            return false;
        };
        let local = {
            let st = state.borrow();
            let mut local = window_to_local(&st, window);
            // One line: pin the vertical coordinate to it, so a press on the
            // strip a handle occupies below the text still names a character.
            local.y = line_middle_y(&st);
            local
        };
        let Some(offset) = super::mouse::hit_test(&state, &local) else {
            return false;
        };
        {
            let st = state.borrow();
            st.cursor
                .set_position(offset, teksilo_text::text_document::MoveMode::MoveAnchor);
        }
        super::state::sync_cursor_signals(&state);
        super::keyboard::report_ime_cursor_area(&state, ctx);
        true
    }

    /// Tell the platform where the caret is, so an on-screen keyboard's
    /// candidate window follows it.
    ///
    /// The field's own reporter, not the controller's — the same one
    /// [`Self::place_caret_at`] ends with: it holds the focus / layout guard
    /// and the ibus-feedback-loop dedup, and a second route into the same
    /// report would leave that cache stale. Reports the area only; re-asserting
    /// IME allowance cancels a live composition.
    pub(crate) fn report_ime_area(&self, ctx: &mut EventContext<'_>) {
        let Some(state) = self.state() else {
            return;
        };
        super::keyboard::report_ime_cursor_area(&state, ctx);
    }

    /// Take the toolbar down without touching the handles.
    ///
    /// The press that is about to place a caret, and the first sample of a
    /// handle drag: in both the commands on offer are about to be aimed at a
    /// selection that no longer exists.
    pub(crate) fn hide_toolbar(&self) {
        if self.toolbar_wanted.get() {
            self.toolbar_wanted.set(false);
        }
    }

    /// A callback that keeps the host's published state honest when the
    /// *framework* takes an overlay down — a right-click's `dismiss_except`, a
    /// modal's `dismiss_all`, Escape. Without it the content's `visible_when`
    /// gate would re-activate a node no overlay hosts any more, and a menu panel
    /// would appear as a stray root in the corner of the window.
    ///
    /// Weak, so the callback the overlay manager holds cannot keep this mount
    /// (and through it the whole field state) alive.
    fn on_overlay_dismissed(
        self: &Rc<Self>,
        what: DismissedOverlay,
    ) -> teksilo_core::overlay::OverlayDismissCallback {
        let weak = Rc::downgrade(self);
        Rc::new(move |_, _| {
            if let Some(this) = weak.upgrade() {
                match what {
                    DismissedOverlay::Toolbar => this.toolbar_wanted.set(false),
                    DismissedOverlay::Layer => this.dismiss(),
                }
            }
        })
    }

    /// Show, or re-place, the overlays.
    fn mount(self: &Rc<Self>, ctx: &mut EventContext<'_>, toolbar: ToolbarIntent) {
        let Some(anchor) = self.field.get() else {
            return;
        };
        if let Some(layer) = self.layer.get() {
            // `FullViewport`, which is the placement the affordance band was
            // written for: the layer positions each handle in window
            // coordinates, so the overlay it lives in only has to *contain*
            // them for the hit-test walk to descend, and the viewport contains
            // every position the text can be at.
            //
            // This shape needed a fix in `WidgetTree::hit_test_with` before it
            // was usable, and it is worth knowing which: an overlay is chosen by
            // its **bounds**, and the router used to return whatever that
            // overlay's content answered — `None` included. A viewport-sized
            // overlay therefore made the field beneath stop taking presses
            // altogether. The router now falls through to the tree when the
            // chosen overlay's content root declares `event_pass_through` and
            // its subtree claimed nothing, which both this layer and
            // [`AffordanceHost`] do. Hence no `update_overlay_placement_by_content`
            // per publish either: the viewport does not move when a handle does.
            ctx.show_overlay_in_band(
                OverlayRequest {
                    content_id: layer,
                    anchor,
                    placement: OverlayPlacement::FullViewport,
                    // The band is exempt from outside-press dismissal — every
                    // caret-moving tap is outside a handle — so the lifetime is
                    // the controller's published state, above.
                    dismiss: DismissBehavior::Manual,
                    layer: OverlayLayer::InTree,
                    parent_overlay: None,
                    on_dismiss: Some(self.on_overlay_dismissed(DismissedOverlay::Layer)),
                    fade_duration: None,
                },
                OverlayBand::TextAffordance,
            );
        }
        let wanted = match toolbar {
            ToolbarIntent::Show => true,
            ToolbarIntent::Hide => false,
            ToolbarIntent::Keep => self.toolbar_wanted.get(),
        };
        if self.toolbar_wanted.get() != wanted {
            self.toolbar_wanted.set(wanted);
        }
        let Some(toolbar_id) = self.toolbar.get() else {
            return;
        };
        if !wanted {
            return;
        }
        let Some(request) = self.controller.borrow().toolbar() else {
            return;
        };
        ctx.show_overlay_in_band(
            OverlayRequest {
                content_id: toolbar_id,
                anchor,
                placement: request.placement(),
                // **Not** `ClickOutside`. A direct pointer's outside press is
                // *armed* rather than dismissed (`arm_outside_press_dismissal`):
                // both the down and the up are withheld from the tree, on the
                // grounds that a finger covers what it is about to actuate. With
                // a click-outside toolbar up, the next touch anywhere — the text,
                // a selection handle — would be spent closing the menu, so every
                // gesture would need doing twice. `EscapeKey` is skipped by that
                // rule entirely, and the host owns every other way down:
                // the press that places a caret, a keystroke, a handle drag,
                // focus loss, window deactivation, a content change.
                dismiss: DismissBehavior::EscapeKey,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: Some(self.on_overlay_dismissed(DismissedOverlay::Toolbar)),
                fade_duration: None,
            },
            OverlayBand::Standard,
        );
        // Showing content that is already up is a no-op, so the selection
        // moving under an open toolbar needs this second door.
        ctx.update_overlay_placement_by_content(toolbar_id, request.placement());
    }

    /// The delegate the affordance layer services its widgets through.
    pub(crate) fn delegate(self: &Rc<Self>) -> Rc<dyn TextAffordanceDelegate> {
        Rc::new(FieldDelegate(Rc::clone(self)))
    }
}

struct FieldDelegate(Rc<FieldTouch>);

impl TextAffordanceDelegate for FieldDelegate {
    fn handle_drag(
        &self,
        kind: SelectionHandleKind,
        phase: HandleDragPhase,
        point: Point,
        ctx: &mut EventContext<'_>,
    ) {
        // The handle nodes move as they are dragged, so the position the router
        // localized against is not one this closure can reconstruct — see the
        // module docs. `pointer_position` is the sample's own window position.
        let window = ctx.pointer_position().unwrap_or(point);
        if matches!(phase, HandleDragPhase::Begin | HandleDragPhase::Move) {
            // The controller stops publishing a toolbar request the moment a
            // drag starts, so leaving the menu up would leave an empty panel.
            self.0.hide_toolbar();
        }
        // No pointer-kind guard here: `TouchSelection::drag_handle` refuses an
        // indirect pointer as its first statement, and the handle's own node
        // refuses one before that.
        self.0.with_source(|controller, source| {
            controller.drag_handle(kind, phase, window, ctx, source)
        });
        // The controller moved the caret by writing a selection, which nothing
        // downstream sees, so the candidate window would stay where the caret
        // last was *typed* to. See `drag_moves_the_caret` for which samples
        // count as a caret move.
        if teksilo_core::text_touch::drag_moves_the_caret(kind, phase) {
            self.0.report_ime_area(ctx);
        }
        // Every phase, not only the last: the overlay's rectangle is the
        // affordances' own, so it has to follow them sample by sample.
        self.0.mount(
            ctx,
            match phase {
                HandleDragPhase::Begin | HandleDragPhase::Move => ToolbarIntent::Hide,
                // The finger let go of a range it chose: that is when the
                // commands for it belong on screen.
                HandleDragPhase::End | HandleDragPhase::Cancel => ToolbarIntent::Show,
            },
        );
        ctx.request_frame();
    }

    fn set_handle_offset(
        &self,
        kind: SelectionHandleKind,
        offset: usize,
        ctx: &mut EventContext<'_>,
    ) {
        // The assistive-technology route: `SetValue` on a handle's slider node.
        // Not a pointer at all, so it is deliberately ungated by pointer kind.
        let direction = ctx.layout_direction();
        self.0.with_source(|controller, source| {
            let selection = source.selection();
            let clamped = offset.min(source.document_len());
            let next = match kind {
                SelectionHandleKind::Caret => clamped..clamped,
                SelectionHandleKind::Start => clamped.min(selection.end)..selection.end,
                SelectionHandleKind::End => selection.start..clamped.max(selection.start),
            };
            source.set_selection(next);
            controller.refresh(direction, source);
        });
        // The same caret move as a drag's, reached from assistive technology
        // instead of a finger, so it owes the same report.
        if kind == SelectionHandleKind::Caret {
            self.0.report_ime_area(ctx);
        }
        self.0.mount(ctx, ToolbarIntent::Keep);
        ctx.request_frame();
    }
}

// ---------------------------------------------------------------------------
// Mounting
// ---------------------------------------------------------------------------

impl super::TextInputField {
    /// Configure the touch controller and build the two overlay contents it
    /// raises. Called once per `build`, after the shared state exists.
    pub(crate) fn mount_touch_selection(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) {
        use teksilo_core::styles::TextSelectionStyle;
        use teksilo_core::text_touch::HandleMetrics;

        let theme = ctx.theme_signal().get();
        let style: Rc<dyn TextSelectionStyle> = match theme.style_slots.text_selection.clone() {
            Some(installed) => installed,
            None => Rc::new(crate::styles::RecipeTextSelectionStyle::for_tokens(
                &theme.input,
            )),
        };
        let handle_recipe = style.handle(&theme);
        let lens_recipe = style.magnifier(&theme);

        // A secure field gets **no lens**, revealed or not. The magnifier is a
        // replay of the host's own text layer raised 40 dp clear of the
        // fingertip and magnified: while the field is revealed that layer is the
        // password, in plain sight and larger than life, which is the exact
        // shoulder-surfing exposure the masking exists to prevent. Masked, the
        // lens would show nothing but bigger bullets, so there is nothing on the
        // other side of the trade.
        //
        // Both switches are set here — the controller stops computing a request
        // and the layer builds no node — because either alone would leave the
        // other able to reintroduce it.
        let secure = self.secure;

        let controller = TouchSelection::new()
            // The rectangle the controller hit-tests is the one the layer
            // paints, at whatever density the theme resolved to.
            .metrics(HandleMetrics::from(&handle_recipe))
            .magnifier_metrics(
                lens_recipe.radius,
                lens_recipe.half_height,
                lens_recipe.rise,
                lens_recipe.scale,
            )
            .magnifier(!secure)
            // `prefers_reduced_motion` has no accessor on `EventContext`, so it
            // has to be read while a `BuildContext` is in hand.
            .reduced_motion(ctx.prefers_reduced_motion());
        let affordances = controller.affordances();
        *self.touch.controller.borrow_mut() = controller;

        // Nothing to do about the *previous* build's overlay contents. The
        // rebuild path destroys them (they are `add_detached`, so they belong to
        // the build that made them) and `WidgetTree::gc_orphaned_overlays`
        // dismisses, at the next layout, every overlay whose content is no longer
        // active — running the `on_dismiss` callbacks below on the way out. An
        // explicit reclamation here was written first and then measured to be
        // dead code, which is why this paragraph exists instead of it.

        let painter = (!secure).then(|| self.magnifier_painter());
        let host = AffordanceHost::new(
            affordances.clone(),
            handle_recipe,
            lens_recipe,
            self.touch.delegate(),
            painter,
            Rc::downgrade(&self.touch),
        );
        // `add_detached`, never a bare `add`: the content is owned by this build
        // and dies with it. A plain `add` would strand another copy in the arena
        // on every rebuild.
        let layer_id = ctx.add_detached(host);
        ctx.set_dormant(layer_id);
        self.touch.layer.set(Some(layer_id));

        let toolbar_id = ctx.add_detached_boxed(build_selection_toolbar_widget(
            self.state().clone(),
            affordances.clone(),
        ));
        ctx.set_dormant(toolbar_id);
        // The toolbar is a real menu, so an empty one would paint an empty
        // panel — and a `visible_when(true)` node outside an overlay is an
        // ordinary root that paints wherever layout puts it. Gate it on the same
        // signal `mount` consults before showing it, so "shown" and "visible"
        // are one fact; this is also what retires the toolbar on the paths that
        // have no `EventContext` to dismiss an overlay from.
        ctx.visible_when(toolbar_id, self.touch.toolbar_wanted.clone());
        self.touch.toolbar.set(Some(toolbar_id));

        self.touch.field.set(Some(ctx.self_id()));
    }

    /// The closure that fills the magnifier: **only** this field's text layer,
    /// re-emitted in window coordinates.
    ///
    /// It is re-entered during the same frame, inside a transform-and-clip
    /// scope, so it deliberately does none of what
    /// [`TextInputField::paint`](teksilo_core::widget::Widget::paint) does around
    /// the same call: no `sync_viewport` (it would adopt the *lens*'s bounds as
    /// the field's), no `ensure_caret_visible_h` (it writes `scroll_x`), no
    /// `set_clip` (`replay` installs the lens clip and a `clear_clip` in here
    /// would destroy it), no border, focus ring or suffix strip. What is left —
    /// re-rendering an already-laid-out flow — produces the same display list
    /// twice, which is what the contract asks for.
    fn magnifier_painter(
        &self,
    ) -> Rc<dyn Fn(&mut teksilo_canvas::Canvas, &teksilo_core::widget::PaintContext<'_>)> {
        use crate::rich_text::paint::{PaintParams, paint_frame};
        let state = self.state().clone();
        Rc::new(move |canvas, _ctx| {
            let mut st = state.borrow_mut();
            if !st.engine.has_full_layout() {
                return;
            }
            let origin = Point::new(st.viewport_origin.x - st.scroll_x, st.viewport_origin.y);
            let target: &mut TextInputState = &mut st;
            let TextInputState {
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
                        image_resolver: None,
                        selection: None,
                        selection_color: [0.0; 4],
                        selected_image_out: None,
                        resize_preview: None,
                        // A lens that blinked would be a second caret with a
                        // phase of its own; the selection band the frame already
                        // carries is what a handle drag is aiming at.
                        draw_caret: false,
                    },
                );
            });
        })
    }
}

/// The selection toolbar: the same four commands the right-click menu offers,
/// each row shown only while the controller says the surface will honour it.
///
/// Built once per `build` rather than per raise, because it is overlay content
/// and an `EventContext` cannot add widgets. Reactive visibility does the work a
/// fresh build would: the rows are gated on the controller's published
/// [`ClipboardActions`](teksilo_core::text_touch::ClipboardActions), which is
/// derived from the surface's own `is_editable` / `allows_copy` / selection, so
/// a row can never offer a command the field would refuse.
fn build_selection_toolbar_widget(
    state: SharedState,
    affordances: teksilo_core::text_touch::TextAffordances,
) -> Box<dyn teksilo_core::widget::Widget> {
    use teksilo_core::text_touch::TextAction;
    let offers = |action: TextAction| {
        let aff = affordances.clone();
        affordances
            .version_signal()
            .map(move |_| aff.toolbar().is_some_and(|t| t.actions.contains(&action)))
    };
    Box::new(
        crate::menu_list::MenuList::new()
            .item_when(super::menu_row_cut(&state), offers(TextAction::Cut))
            .item_when(super::menu_row_copy(&state), offers(TextAction::Copy))
            .item_when(super::menu_row_paste(&state), offers(TextAction::Paste))
            .item_when(
                super::menu_row_select_all(&state),
                offers(TextAction::SelectAll),
            ),
    )
}

// ---------------------------------------------------------------------------
// The overlay content root
// ---------------------------------------------------------------------------

/// The bounding rectangle of everything `affordances` currently wants shown.
///
/// Nothing in the mount uses it any more — the overlay is the whole viewport and
/// the layer places each handle in window coordinates inside it. It survives as
/// the tests' way of naming a point that is clear of every affordance, which is
/// what the fall-through assertions are about.
#[cfg(test)]
pub(crate) fn affordance_bounds(affordances: &teksilo_core::text_touch::TextAffordances) -> Rect {
    let mut union: Option<Rect> = None;
    let mut add = |rect: Rect| {
        union = Some(match union {
            None => rect,
            Some(current) => {
                let x = current.x.min(rect.x);
                let y = current.y.min(rect.y);
                Rect::new(
                    x,
                    y,
                    current.right().max(rect.right()) - x,
                    current.bottom().max(rect.bottom()) - y,
                )
            }
        });
    };
    for handle in affordances.handles() {
        add(handle.hit);
    }
    if let Some(lens) = affordances.magnifier() {
        add(lens.lens);
    }
    union.unwrap_or(Rect::ZERO)
}

/// The affordance layer, wrapped in a viewport-sized, pass-through overlay
/// content root that answers one thing: a **cursor's** click on a handle.
///
/// # Why the wrapper exists
///
/// It used to exist to be *small*. An overlay is chosen by its bounds, and the
/// router returned whatever the chosen overlay's content answered — `None`
/// included — so a viewport-sized affordance overlay made the field beneath stop
/// taking presses altogether. Sizing the overlay to the affordances was the
/// workaround. `WidgetTree::hit_test_with` now falls through to the tree when
/// the chosen overlay's content root declares `event_pass_through` and its
/// subtree claimed nothing, so the sizing is gone and the layer is mounted at
/// the placement it was designed for.
///
/// What is left is the one press neither the layer nor the field can answer.
/// `event_pass_through` removes a node from **hit-testing**, not from the
/// **bubble path** of a descendant that was hit — so a press on a handle still
/// arrives here on its way up, while a press that lands on no handle never
/// reaches this node at all and goes to the field instead.
///
/// A handle refuses an indirect pointer (its own guard, in `teksilo-core`), and
/// the field never sees the press because a handle is a different root: without
/// this node a cursor's click on touch chrome would be swallowed, leaving the
/// chrome standing with nothing able to remove it. So here it places the caret
/// it was asking for and retires the affordances. A cursor's click anywhere
/// *else* in the field retires them too — that is the field's own mouse arm, in
/// [`super::mouse`].
///
/// A finger's press is refused outright: on a handle it belongs to the handle,
/// and anywhere else it never arrives.
pub(crate) struct AffordanceHost {
    affordances: teksilo_core::text_touch::TextAffordances,
    handle_recipe: teksilo_core::styles::TextSelectionHandleRecipe,
    magnifier_recipe: teksilo_core::styles::TextMagnifierRecipe,
    delegate: Rc<dyn TextAffordanceDelegate>,
    painter:
        Option<Rc<dyn Fn(&mut teksilo_canvas::Canvas, &teksilo_core::widget::PaintContext<'_>)>>,
    touch: std::rc::Weak<FieldTouch>,
    layer: Option<WidgetId>,
}

impl std::fmt::Debug for AffordanceHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AffordanceHost")
            .field("layer", &self.layer)
            .finish()
    }
}

impl AffordanceHost {
    pub(crate) fn new(
        affordances: teksilo_core::text_touch::TextAffordances,
        handle_recipe: teksilo_core::styles::TextSelectionHandleRecipe,
        magnifier_recipe: teksilo_core::styles::TextMagnifierRecipe,
        delegate: Rc<dyn TextAffordanceDelegate>,
        painter: Option<
            Rc<dyn Fn(&mut teksilo_canvas::Canvas, &teksilo_core::widget::PaintContext<'_>)>,
        >,
        touch: std::rc::Weak<FieldTouch>,
    ) -> Self {
        Self {
            affordances,
            handle_recipe,
            magnifier_recipe,
            delegate,
            painter,
            touch,
            layer: None,
        }
    }
}

impl teksilo_core::widget::Widget for AffordanceHost {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        use teksilo_core::text_touch::TextAffordanceLayer;
        let mut layer = TextAffordanceLayer::new(
            self.affordances.clone(),
            self.handle_recipe,
            self.magnifier_recipe,
            Rc::clone(&self.delegate),
        );
        if let Some(painter) = self.painter.clone() {
            layer = layer.magnifier_painter(painter);
        }
        let id = ctx.add(layer);
        self.layer = Some(id);

        let touch = self.touch.clone();
        let handlers = teksilo_core::widget_builder::HandlerSet::new()
            // Not a hit-test candidate: a press that lands on no handle must go
            // to the field beneath, which is what the router's fall-through
            // does with an overlay content root carrying this flag. Bubbled
            // presses from the handles still arrive — the flag governs
            // targeting, not dispatch — which is the one thing this node is for.
            .event_pass_through(true)
            .on_pointer_event(move |event, ctx| {
                if ctx.pointer_kind().is_direct() {
                    // On a handle it is the handle's; anywhere else it never got
                    // here.
                    return teksilo_core::event::EventResponse::Ignored;
                }
                if !matches!(event, teksilo_core::event::WidgetEvent::PointerDown { .. }) {
                    return teksilo_core::event::EventResponse::Ignored;
                }
                let Some(touch) = touch.upgrade() else {
                    return teksilo_core::event::EventResponse::Ignored;
                };
                let Some(window) = ctx.pointer_position() else {
                    return teksilo_core::event::EventResponse::Ignored;
                };
                // A cursor has clicked touch chrome. Place the caret it asked
                // for and take the chrome down.
                touch.place_caret_at(window, ctx);
                touch.dismiss();
                ctx.request_frame();
                teksilo_core::event::EventResponse::Handled
            });
        ctx.apply_self_handlers(handlers);
        self.children()
    }

    fn layout_response(
        &self,
        proposal: teksilo_canvas::SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Whatever the overlay gives it, which is the viewport.
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: teksilo_canvas::SizeProposal,
        children: &mut [teksilo_core::widget::WidgetPlacement],
        _ctx: &teksilo_core::widget::LayoutContext,
    ) {
        // The layer positions its own children in window coordinates, so its own
        // rectangle only has to *contain* them for the hit-test walk to descend
        // — and the viewport contains every position the text can be at.
        for placement in children.iter_mut() {
            placement.origin = bounds.origin();
            placement.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.layer.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        // A bare container, like the layer inside it: the walker prunes it, so it
        // adds no traversal stop between the editor and its handles.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}
