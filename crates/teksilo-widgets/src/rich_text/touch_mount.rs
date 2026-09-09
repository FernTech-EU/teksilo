// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The touch-selection mount both multi-line editor stacks share: the
//! controller, the two overlays it raises, and the pass-through content root
//! that answers a cursor's click on a handle.
//!
//! # Why this lives under `rich_text`
//!
//! It serves [`RichTextEditor`](super::RichTextEditor), `CodeEditor`,
//! `PlainTextEditor` and `LogView` — three widgets over two state types — so
//! its natural home is `crate::common`. It sits here instead because
//! `code_editor` already reaches into `rich_text` for its hit test
//! ([`hit_test`](super::hit_test)) and its painter
//! ([`paint`](super::paint)), and `rich_text` is the lower of the two in the
//! crate's own dependency order: nothing in `rich_text` names `code_editor`.
//! One more shared thing crossing the same seam is cheaper than a fourth copy
//! of the mount.
//!
//! # What a host still owns
//!
//! Everything in [`teksilo_core::text_touch`] is stated in **window**
//! coordinates and the router hands a handler **widget-local** ones, so each
//! host converts on the way in and on the way out. That conversion is the one
//! thing this module cannot do for them: it depends on where the editor's body
//! sits inside its wrapper, which is a per-stack fact. See
//! [`TouchTextSurface`].
//!
//! The shape of the mount — one `FullViewport` pass-through overlay for the
//! affordances, a second overlay in the standard band for the selection
//! toolbar, retirement by publishing empty geometry rather than by tearing an
//! overlay down — is the one
//! [`docs/text-touch-editing.md`](https://github.com/ferntech-eu/teksilo/blob/main/docs/text-touch-editing.md)
//! prescribes and the single-line family adopted first.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::EventResponse;
use teksilo_core::overlay::{
    DismissBehavior, OverlayBand, OverlayLayer, OverlayPlacement, OverlayRequest,
    SelectionHandleKind,
};
use teksilo_core::pointer::{PointerId, PointerInfo};
use teksilo_core::signal::Signal;
use teksilo_core::text_touch::{
    HandleDragPhase, SelectionHandleGeometry, TextAffordanceDelegate, TextAffordances,
    TextHitSource, TouchSelection,
};
use teksilo_core::widget::{EventContext, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;

/// A magnifier painting closure: the host's **text layer**, re-emitted.
pub(crate) type LensPainter = Rc<dyn Fn(&mut teksilo_canvas::Canvas, &PaintContext<'_>)>;

/// One editor, as the touch mount needs to see it.
///
/// Implemented once per state type — once over `EditorState` for the rich-text
/// editor, once over `CodeEditorState` for the code editor and the log view —
/// and held by the mount as a `dyn` so the mount itself is not generic.
pub(crate) trait TouchTextSurface {
    /// Run `f` with this surface as a [`TextHitSource`], answering whether it
    /// ran. `false` before the widget has state to answer with.
    ///
    /// `&mut dyn FnMut` rather than a generic `FnOnce`, because this trait is
    /// used as a `dyn` object: every controller entry point wants
    /// `&mut TouchSelection` *and* `&mut dyn TextHitSource` at once, and the
    /// controller lives on the mount while the source is a short-lived wrapper
    /// around a borrow of the host's state.
    fn with_hit_source(&self, f: &mut dyn FnMut(&mut dyn TextHitSource)) -> bool;

    /// Whether this surface has caret geometry worth hanging an affordance off
    /// — a laid-out engine with a real rectangle at the caret.
    fn geometry_is_meaningful(&self) -> bool;

    /// Place the caret at a **window** point, as a press on the text does, and
    /// report the IME area for it. Answers `false` when the point names no
    /// character.
    ///
    /// The mount's own use of it is narrow and is the reason the content root
    /// exists at all: a **cursor's** click on a selection handle, which the
    /// handle refuses and the editor never sees.
    fn place_caret_at(&self, window: Point, ctx: &mut EventContext<'_>) -> bool;

    /// The closure that fills the magnifier, or `None` for a surface that
    /// mounts no lens.
    fn lens_painter(&self) -> Option<LensPainter>;

    /// The selection toolbar's rows, gated on `affordances`' published
    /// [`ClipboardActions`](teksilo_core::text_touch::ClipboardActions).
    fn toolbar_rows(&self, affordances: TextAffordances) -> Box<dyn Widget>;

    /// Scroll the surface vertically by `dy` logical pixels, clamped to its own
    /// range, and answer whether anything moved.
    ///
    /// The edge auto-scroll a handle drag needs: a selection that has to grow
    /// past the bottom of the viewport cannot, because the offset the controller
    /// asks for does not exist on screen. Vertical only — the two multi-line
    /// surfaces scroll horizontally as well, but a handle dragged sideways
    /// reaches the line's end and stops there, which is where the selection ends
    /// too.
    fn nudge_scroll(&self, dy: f32) -> bool;
}

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

/// The touch-selection mount for one editor.
///
/// Minted with the widget rather than in `build()`, so the ids and the
/// host-side intent survive a rebuild even though the controller inside is
/// replaced by each one.
pub(crate) struct EditorTouch {
    controller: RefCell<TouchSelection>,
    surface: Rc<dyn TouchTextSurface>,
    /// The affordance layer's content root, in the text-affordance band.
    layer: Cell<Option<WidgetId>>,
    /// The selection toolbar's content root, in the standard band.
    toolbar: Cell<Option<WidgetId>>,
    /// The editor itself — the overlays' anchor.
    anchor: Cell<Option<WidgetId>>,
    /// Whether the host wants the selection toolbar up.
    ///
    /// The host's intent, not the controller's: the controller offers a toolbar
    /// for any state with a command worth offering — including a bare caret,
    /// where it is Paste and Select All — and a menu opening on every tap in a
    /// text surface is not what any platform does. The toolbar belongs to a
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
    hold_consumed: Cell<Option<PointerId>>,
    /// Where the live direct-pointer press landed, in window coordinates.
    ///
    /// A text surface cannot use the framework's coarse-pointer tap boundary on
    /// its own: [`TapBoundary::Bounds`](teksilo_core::gesture::TapBoundary) is
    /// the pressed node's *rectangle*, which is the right rule for a control —
    /// the whole node is one target and a finger covers it — and the wrong one
    /// here, where the target is a **character**. A finger can pan a 400 dp-tall
    /// editor a long way and never leave it, so `press_is_inside` stays true
    /// through a pan of the surface's own scroll and a release would place a
    /// caret where the finger happened to stop. So the surface measures travel
    /// against the pointer's own `tap_slop` as well.
    press_origin: Cell<Option<(PointerId, Point)>>,
    /// What to add to a handle-drag sample to make it name the character the
    /// handle marks: `caret centre − grab point`, captured when the drag begins.
    ///
    /// A handle's disc deliberately hangs **off** the line — above it for a
    /// `Start`, below it for an `End` or a caret — so the fingertip does not
    /// cover the character the handle is pointing at. The controller hit-tests
    /// the sample it is given, so handing it the raw finger position asks the
    /// surface for the offset at a point that is a dozen dp off the glyph row:
    /// on a multi-line document that resolves onto the *next* line, and on a
    /// one-line one it falls past the text entirely and clamps to the document's
    /// end. The single-line stack answers this by pinning `offset_at`'s vertical
    /// coordinate to its only line; a multi-line surface has no such line to pin
    /// to, so the correction belongs to the drag instead — which is also the
    /// standard shape, since it is just the grab offset every drag carries.
    drag_offset: Cell<Point>,
}

impl std::fmt::Debug for EditorTouch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorTouch")
            .field("layer", &self.layer.get())
            .field("toolbar_wanted", &self.toolbar_wanted.get())
            .finish()
    }
}

impl EditorTouch {
    pub(crate) fn new(surface: Rc<dyn TouchTextSurface>) -> Rc<Self> {
        Rc::new(Self {
            controller: RefCell::new(TouchSelection::new()),
            surface,
            layer: Cell::new(None),
            toolbar: Cell::new(None),
            anchor: Cell::new(None),
            toolbar_wanted: Signal::new(false),
            hold_consumed: Cell::new(None),
            press_origin: Cell::new(None),
            drag_offset: Cell::new(Point::ZERO),
        })
    }

    /// Record that `pointer`'s press was spent by a hold.
    pub(crate) fn mark_hold_consumed(&self, pointer: PointerId) {
        self.hold_consumed.set(Some(pointer));
    }

    /// Record where `pointer`'s press landed, in window coordinates.
    pub(crate) fn mark_press(&self, pointer: PointerId, window: Option<Point>) {
        self.press_origin.set(window.map(|w| (pointer, w)));
    }

    /// Whether `pointer`'s release still names a character: it has to travel no
    /// further from where it landed than a tap of its own kind may.
    ///
    /// `None` when nothing was recorded for this contact — a release with no
    /// press of ours behind it, which is not a caret placement either.
    pub(crate) fn press_is_still_a_tap(
        &self,
        pointer: PointerId,
        window: Point,
        tap_slop: f32,
    ) -> bool {
        let Some((id, origin)) = self.press_origin.take() else {
            return false;
        };
        if id != pointer {
            return false;
        }
        let (dx, dy) = (window.x - origin.x, window.y - origin.y);
        dx * dx + dy * dy <= tap_slop * tap_slop
    }

    /// Whether `pointer`'s press was spent by a hold, clearing the record.
    pub(crate) fn take_hold_consumed(&self, pointer: PointerId) -> bool {
        if self.hold_consumed.get() == Some(pointer) {
            self.hold_consumed.set(None);
            return true;
        }
        false
    }

    /// The affordances currently published — the tests' window onto the mount.
    pub(crate) fn handles(&self) -> Vec<SelectionHandleGeometry> {
        self.controller.borrow().handles()
    }

    /// Whether the host wants the selection toolbar up.
    pub(crate) fn toolbar_is_wanted(&self) -> bool {
        self.toolbar_wanted.get()
    }

    /// The affordance layer's content root — the tests' way of reaching the
    /// handles as widgets.
    #[cfg(test)]
    pub(crate) fn layer_content(&self) -> Option<WidgetId> {
        self.layer.get()
    }

    /// The selection toolbar's content root.
    #[cfg(test)]
    pub(crate) fn toolbar_content(&self) -> Option<WidgetId> {
        self.toolbar.get()
    }

    /// Run `f` with the toolbar request the controller currently publishes, if
    /// any — the tests' window onto what commands a selection is offering.
    #[cfg(test)]
    pub(crate) fn with_toolbar_for_test(
        &self,
        f: impl FnOnce(&teksilo_core::text_touch::SelectionToolbarRequest),
    ) {
        if let Some(request) = self.controller.borrow().toolbar() {
            f(&request);
        }
    }

    /// Run `f` with the controller and this editor's hit source. `None` before
    /// the surface can answer.
    fn with_source<R>(
        &self,
        f: impl FnOnce(&mut TouchSelection, &mut dyn TextHitSource) -> R,
    ) -> Option<R> {
        let mut controller = self.controller.borrow_mut();
        let mut f = Some(f);
        let mut out = None;
        self.surface.with_hit_source(&mut |source| {
            if let Some(f) = f.take() {
                out = Some(f(&mut controller, source));
            }
        });
        out
    }

    /// Raise the affordances for the current selection and mount their
    /// overlays.
    pub(crate) fn raise(self: &Rc<Self>, ctx: &mut EventContext<'_>, toolbar: ToolbarIntent) {
        if !self.surface.geometry_is_meaningful() {
            self.dismiss();
            return;
        }
        let direction = ctx.layout_direction();
        self.with_source(|controller, source| controller.raise(direction, source));
        self.mount_overlays(ctx, toolbar);
    }

    /// Recompute the affordances after a change the controller did not make —
    /// a keystroke, an undo, an assistive client's `SetTextSelection`, the
    /// editor's own multi-tap. A no-op until something has been raised, so an
    /// untouched editor pays nothing.
    pub(crate) fn refresh(self: &Rc<Self>, ctx: &mut EventContext<'_>, toolbar: ToolbarIntent) {
        if !self.surface.geometry_is_meaningful() {
            self.dismiss();
            return;
        }
        let direction = ctx.layout_direction();
        self.with_source(|controller, source| controller.refresh(direction, source));
        self.mount_overlays(ctx, toolbar);
    }

    /// Select the word at a **window** point and raise the affordances — the
    /// hold.
    ///
    /// The word selection is the controller's
    /// ([`TouchSelection::on_long_press`]), not a second copy here, and so is
    /// the pointer-kind refusal: the gesture's own `pointer` is handed to it
    /// and it guards on that. Returns `false` when the controller declined —
    /// an indirect pointer, or a surface with no geometry to select against —
    /// so the caller can leave the gesture unhandled.
    pub(crate) fn select_word_at(
        self: &Rc<Self>,
        pointer: PointerInfo,
        point: Point,
        ctx: &mut EventContext<'_>,
    ) -> bool {
        if !self.surface.geometry_is_meaningful() {
            self.dismiss();
            return false;
        }
        let handled = self.with_source(|controller, source| {
            controller.on_long_press(pointer, point, ctx, source)
        });
        if handled != Some(EventResponse::Handled) {
            return false;
        }
        self.mount_overlays(ctx, ToolbarIntent::Show);
        true
    }

    /// Retract every affordance.
    ///
    /// The one mechanism: the controller publishes empty geometry, and every
    /// affordance node is gated on that published state by `visible_when`,
    /// while the toolbar is gated on `toolbar_wanted`. Nothing here dismisses
    /// an overlay, which is what lets the ctx-less paths (a window-active
    /// effect, an external text sync) retire the chrome at all.
    pub(crate) fn dismiss(&self) {
        // Guarded: this is reached from every mouse press in the editor, and a
        // `Signal::set` notifies whether or not the value moved.
        // `TextAffordances::publish` already early-returns on an unchanged
        // state, so with this the whole call is free for an editor that has
        // nothing raised.
        if self.toolbar_wanted.get() {
            self.toolbar_wanted.set(false);
        }
        self.controller.borrow_mut().dismiss();
    }

    /// Take the toolbar down without touching the handles — the press that is
    /// about to place a caret, and the first sample of a handle drag. In both,
    /// the commands on offer are aimed at a selection that is about to stop
    /// existing.
    pub(crate) fn hide_toolbar(&self) {
        if self.toolbar_wanted.get() {
            self.toolbar_wanted.set(false);
        }
    }

    /// A callback that keeps the host's published state honest when the
    /// *framework* takes an overlay down — a right-click's `dismiss_except`, a
    /// modal's `dismiss_all`, Escape. Without it the content's `visible_when`
    /// gate would re-activate a node no overlay hosts any more, and the menu
    /// panel would appear as a stray root in the corner of the window.
    ///
    /// Weak, so the callback the overlay manager holds cannot keep this mount
    /// — and through it the whole editor state — alive.
    fn on_overlay_dismissed(
        self: &Rc<Self>,
        what: DismissedOverlay,
    ) -> teksilo_core::overlay::OverlayDismissCallback {
        let weak = Rc::downgrade(self);
        Rc::new(move || {
            if let Some(this) = weak.upgrade() {
                match what {
                    DismissedOverlay::Toolbar => this.toolbar_wanted.set(false),
                    DismissedOverlay::Layer => this.dismiss(),
                }
            }
        })
    }

    /// Show, or re-place, the overlays.
    fn mount_overlays(self: &Rc<Self>, ctx: &mut EventContext<'_>, toolbar: ToolbarIntent) {
        let Some(anchor) = self.anchor.get() else {
            return;
        };
        if let Some(layer) = self.layer.get() {
            // `FullViewport`, the placement the affordance band was written
            // for: the layer positions each handle in window coordinates, so
            // the overlay it lives in only has to *contain* them for the
            // hit-test walk to descend, and the viewport contains every
            // position the text can be at. The router falls through to the tree
            // when this content root's subtree claims nothing, which is what
            // keeps the editor underneath taking presses — hence no
            // re-placement per publish either.
            ctx.show_overlay_in_band(
                OverlayRequest {
                    content_id: layer,
                    anchor,
                    placement: OverlayPlacement::FullViewport,
                    // The band is exempt from outside-press dismissal — every
                    // caret-moving tap is outside a handle — so the lifetime is
                    // the controller's published state.
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
                // *armed* rather than dismissed: both the down and the up are
                // withheld from the tree, on the grounds that a finger covers
                // what it is about to actuate. With a click-outside toolbar up,
                // the next touch anywhere — the text, a selection handle —
                // would be spent closing the menu, so every gesture would need
                // doing twice. `EscapeKey` is skipped by that rule entirely,
                // and the host owns every other way down.
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
    fn delegate(self: &Rc<Self>) -> Rc<dyn TextAffordanceDelegate> {
        Rc::new(EditorTouchDelegate(Rc::clone(self)))
    }

    /// Configure the controller and build the two overlay contents, once per
    /// `build()`, with `anchor` as the editor's own node.
    ///
    /// Nothing needs reclaiming from the *previous* build: the contents are
    /// `add_detached`, so they belong to the build that made them, and
    /// `WidgetTree::gc_orphaned_overlays` dismisses at the next layout every
    /// overlay whose content is no longer active — running the `on_dismiss`
    /// callbacks above on the way out.
    pub(crate) fn build(self: &Rc<Self>, ctx: &mut BuildContext, anchor: WidgetId) {
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
        let painter = self.surface.lens_painter();

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
            .magnifier(painter.is_some())
            // `prefers_reduced_motion` has no accessor on `EventContext`, so it
            // has to be read while a `BuildContext` is in hand.
            .reduced_motion(ctx.prefers_reduced_motion());
        let affordances = controller.affordances();
        *self.controller.borrow_mut() = controller;

        let host = EditorAffordanceHost {
            affordances: affordances.clone(),
            handle_recipe,
            magnifier_recipe: lens_recipe,
            delegate: self.delegate(),
            painter,
            touch: Rc::downgrade(self),
            layer: None,
        };
        // `add_detached`, never a bare `add`: the content is owned by this
        // build and dies with it. A plain `add` would strand another copy in
        // the arena on every rebuild.
        let layer_id = ctx.add_detached(host);
        ctx.set_dormant(layer_id);
        self.layer.set(Some(layer_id));

        let toolbar_id = ctx.add_detached_boxed(self.surface.toolbar_rows(affordances));
        ctx.set_dormant(toolbar_id);
        // The toolbar is a real menu, so an empty one would paint an empty
        // panel — and a `visible_when(true)` node outside an overlay is an
        // ordinary root that paints wherever layout puts it. Gate it on the
        // same signal `mount_overlays` consults before showing it, so "shown"
        // and "visible" are one fact; this is also what retires the toolbar on
        // the paths that have no `EventContext` to dismiss an overlay from.
        ctx.visible_when(toolbar_id, self.toolbar_wanted.clone());
        self.toolbar.set(Some(toolbar_id));

        self.anchor.set(Some(anchor));
    }
}

struct EditorTouchDelegate(Rc<EditorTouch>);

impl TextAffordanceDelegate for EditorTouchDelegate {
    fn handle_drag(
        &self,
        kind: SelectionHandleKind,
        phase: HandleDragPhase,
        point: Point,
        ctx: &mut EventContext<'_>,
    ) {
        // The handle nodes move as they are dragged, so the position the router
        // localized against is not one this closure can reconstruct.
        // `pointer_position` is the sample's own window position.
        let window = ctx.pointer_position().unwrap_or(point);
        if matches!(phase, HandleDragPhase::Begin | HandleDragPhase::Move) {
            // The controller stops publishing a toolbar request the moment a
            // drag starts, so leaving the menu up would leave an empty panel.
            self.0.hide_toolbar();
        }
        if phase == HandleDragPhase::Begin {
            // Capture the grab offset while the pre-drag geometry still stands:
            // from here on the sample is translated onto the caret the handle
            // marks. See `EditorTouch::drag_offset`.
            let caret = self
                .0
                .handles()
                .into_iter()
                .find(|h| h.kind == kind)
                .map(|h| {
                    Point::new(
                        h.caret.x + h.caret.width / 2.0,
                        h.caret.y + h.caret.height / 2.0,
                    )
                });
            self.0.drag_offset.set(match caret {
                Some(caret) => Point::new(caret.x - window.x, caret.y - window.y),
                None => Point::ZERO,
            });
        }
        let offset = self.0.drag_offset.get();
        let window = Point::new(window.x + offset.x, window.y + offset.y);
        if phase == HandleDragPhase::Move {
            // Edge auto-scroll, **before** the controller reads the point: a
            // selection growing past the bottom of the viewport is asking for an
            // offset that is not on screen, so the surface has to move first and
            // the same sample then names the text that arrived. The band is the
            // shared one every dragging view uses, widened for a coarse pointer
            // because a contact patch's reported centre cannot be parked as
            // finely as a cursor — and `ctx` answers for the right device here,
            // because a handle drag is a pointer **sample** rather than a tick.
            let viewport = self
                .0
                .with_source(|_, source| source.viewport())
                .unwrap_or(Rect::ZERO);
            if viewport.height > 0.0 {
                let band = crate::common::drag_autoscroll::band_for(ctx.pointer_kind());
                let step = crate::common::drag_autoscroll::step(
                    window.y - viewport.y,
                    viewport.height,
                    band,
                );
                if step != 0.0 {
                    self.0.surface.nudge_scroll(step);
                    // Keep the frame coming while the finger sits in the band, so
                    // a drag held at the edge is not stalled by the absence of a
                    // further sample.
                    ctx.request_frame();
                }
            }
        }
        // No pointer-kind guard here: `TouchSelection::drag_handle` refuses an
        // indirect pointer as its first statement, and the handle's own node
        // refuses one before that.
        self.0.with_source(|controller, source| {
            controller.drag_handle(kind, phase, window, ctx, source)
        });
        self.0.mount_overlays(
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
        self.0.mount_overlays(ctx, ToolbarIntent::Keep);
        ctx.request_frame();
    }
}

/// The affordance layer, wrapped in a viewport-sized, pass-through overlay
/// content root that answers one thing: a **cursor's** click on a handle.
///
/// `event_pass_through` removes a node from **hit-testing**, not from the
/// **bubble path** of a descendant that was hit — so a press on a handle still
/// arrives here on its way up, while a press that lands on no handle never
/// reaches this node at all and goes to the editor instead.
///
/// A handle refuses an indirect pointer (its own guard, in `teksilo-core`), and
/// the editor never sees that press because a handle is a different arena root:
/// without this node a cursor's click on touch chrome would be swallowed,
/// leaving the chrome standing with nothing able to remove it. So here it
/// places the caret it was asking for and retires the affordances. A cursor's
/// click anywhere *else* in the editor retires them too — that is the editor's
/// own mouse arm.
///
/// A finger's press is refused outright: on a handle it belongs to the handle,
/// and anywhere else it never arrives.
struct EditorAffordanceHost {
    affordances: TextAffordances,
    handle_recipe: teksilo_core::styles::TextSelectionHandleRecipe,
    magnifier_recipe: teksilo_core::styles::TextMagnifierRecipe,
    delegate: Rc<dyn TextAffordanceDelegate>,
    painter: Option<LensPainter>,
    touch: std::rc::Weak<EditorTouch>,
    layer: Option<WidgetId>,
}

impl std::fmt::Debug for EditorAffordanceHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorAffordanceHost")
            .field("layer", &self.layer)
            .finish()
    }
}

impl Widget for EditorAffordanceHost {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
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
            // to the editor beneath, which is what the router's fall-through
            // does with an overlay content root carrying this flag. Bubbled
            // presses from the handles still arrive — the flag governs
            // targeting, not dispatch — which is the one thing this node is
            // for.
            .event_pass_through(true)
            .on_pointer_event(move |event, ctx| {
                if ctx.pointer_kind().is_direct() {
                    // On a handle it is the handle's; anywhere else it never
                    // got here.
                    return EventResponse::Ignored;
                }
                if !matches!(event, teksilo_core::event::WidgetEvent::PointerDown { .. }) {
                    return EventResponse::Ignored;
                }
                let Some(touch) = touch.upgrade() else {
                    return EventResponse::Ignored;
                };
                let Some(window) = ctx.pointer_position() else {
                    return EventResponse::Ignored;
                };
                // A cursor has clicked touch chrome. Place the caret it asked
                // for and take the chrome down.
                touch.surface.place_caret_at(window, ctx);
                touch.dismiss();
                ctx.request_frame();
                EventResponse::Handled
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
        // The layer positions its own children in window coordinates, so its
        // own rectangle only has to *contain* them for the hit-test walk to
        // descend — and the viewport contains every position the text can be
        // at.
        for placement in children.iter_mut() {
            placement.origin = bounds.origin();
            placement.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.layer.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        // A bare container, like the layer inside it: the walker prunes it, so
        // it adds no traversal stop between the editor and its handles.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}
