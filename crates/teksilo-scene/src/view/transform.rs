// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! View-side transform-controller internals: which handles a selection offers,
//! where they are, what the pointer grabs, how the keyboard drives them, and
//! the built-in chrome painter.
//!
//! The public vocabulary ([`TransformConfig`], [`crate::TransformDelta`],
//! [`TransformFrame`], …) and all the pure math live in
//! [`crate::transform_session`]. This module holds only what needs a live view:
//! the projection between screen and scene, the theme and the event context.
//!
//! Everything hangs off [`TransformDriver`], a cheap bundle of shared handles
//! rather than a borrow of the view — the drag and key closures outlive the
//! `&self` that built them, the same reason `magnetism_for_drag` is a clone.
//!
//! # The chrome is a paint pass, and that is load-bearing
//!
//! The frame and its handles are painted in `post_paint`, beside the marquee
//! overlay — they are **not** lightweight items in the `Over` band. That is the
//! difference between chrome and content, and four things depend on it: the
//! handles never appear in `items_in_rect`, a marquee never selects them, the
//! per-item accessibility walk never sees them, and — because selection is
//! per-view while the model is shared — a two-pane scene does not show both
//! panes' handles in both panes. A later refactor that "tidies" them into items
//! would undo all four at once.

use std::rc::Rc;

use teksilo_canvas::{Canvas, Point, Rect, Size, StrokeStyle, Transform2D, Vec2};
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::signal::Signal;
use teksilo_core::widget::{EventContext, PaintContext};
use teksilo_core::widget_id::WidgetId;

use super::{GrabSlop, SceneView};
use crate::item::ItemId;
use crate::scene_model::SceneModel;
use crate::selection::SceneSelection;
use crate::transform_session::{
    ChromeKey, HANDLE_ORDER, LivePreview, LiveSession, ROTATE_OFFSET_PX, ResolvedSession,
    SelectionChrome, TransformChrome, TransformConfig, TransformFrame, TransformHandle,
    TransformOp, TransformOutcome, TransformRuntime, TransformSession, TransformSource,
    TransformStep,
};

/// One keyboard step, in scene units. Shift multiplies it.
const KEY_STEP: f32 = 1.0;
/// Shift multiplier for a keyboard step.
const KEY_STEP_SHIFT: f32 = 10.0;
/// One keyboard rotation step, in radians (1°). Shift multiplies it.
const KEY_ROTATE: f32 = std::f32::consts::PI / 180.0;

/// Everything the transform controller needs from its view, as shared handles.
///
/// Cheap to clone (a dozen `Rc`s) and captured by every closure that drives the
/// controller, which is why the controller's whole behaviour lives here rather
/// than on `SceneView`: a `HandlerSet` closure outlives the `&self` that built
/// it, so a method taking `&self` could never have been called from one.
#[derive(Clone)]
pub(crate) struct TransformDriver {
    pub model: SceneModel,
    pub selection: SceneSelection,
    pub cfg: Rc<TransformConfig>,
    pub rt: Rc<TransformRuntime>,
    pub view_transform: Signal<Transform2D>,
    pub reconcile_dirty: Signal<u64>,
    pub self_id: Option<WidgetId>,
    pub pan_x: Signal<f32>,
    pub pan_y: Signal<f32>,
    pub zoom: Signal<f32>,
    pub pan_bounds_override: Signal<Option<Rect>>,
    pub bounds_origin: Signal<Vec2>,
    pub viewport: Signal<Size>,
}

impl std::fmt::Debug for TransformDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformDriver")
            .field("enabled", &self.cfg.enabled.get())
            .field("runtime", &self.rt)
            .finish()
    }
}

impl SceneView {
    /// The controller's driver, if one is installed. Present whether or not the
    /// controller is currently *enabled* — the enabled signal is live, so it is
    /// read at use time rather than snapshotted here.
    pub(super) fn transform_driver(&self) -> Option<TransformDriver> {
        let cfg = self.transform.clone()?;
        Some(TransformDriver {
            model: self.model.clone(),
            selection: self.selection.clone(),
            cfg,
            rt: self.transform_rt.clone(),
            view_transform: self.view_transform_signal.clone(),
            reconcile_dirty: self.reconcile_dirty.clone(),
            self_id: self.self_widget_id.get(),
            pan_x: self.pan_x.clone(),
            pan_y: self.pan_y.clone(),
            zoom: self.zoom.clone(),
            pan_bounds_override: self.pan_bounds_override.clone(),
            bounds_origin: self.bounds_origin_signal.clone(),
            viewport: self.last_viewport.clone(),
        })
    }

    /// The driver, only while the controller is enabled.
    pub(super) fn transform_enabled(&self) -> Option<TransformDriver> {
        self.transform_driver().filter(|d| d.is_enabled())
    }

    /// The scene-space affine the preview applies, and the roots it applies to.
    ///
    /// **The** lockstep point of the whole controller: the lightweight tier
    /// composes this into each item's `local → scene` in `paint_band`, and the
    /// heavyweight tier applies it to `placement.origin` / `placement.size` in
    /// `place_children`. One affine, so the two tiers show the same **box**
    /// throughout a gesture and neither can half-move.
    /// [`LivePreview::Ghost`] returns `None` for both, which is what makes
    /// "only the frame moves" one decision rather than two.
    ///
    /// The box, not the treatment. A resize's *scale* is honoured differently
    /// on each side — the card is laid out at the new size and reflows, the
    /// lightweight item is drawn through the scale and stretches — and the two
    /// only converge when the commit writes `local_bounds`. See the
    /// [`transform_session`](crate::transform_session) module header for what
    /// that costs and what would close it; the box-level agreement this
    /// function guarantees is pinned by
    /// `a_lightweight_resize_previews_the_box_it_commits`.
    pub(super) fn transform_preview(&self) -> Option<(Rc<[ItemId]>, Transform2D)> {
        self.transform_enabled()?.preview()
    }

    /// Whether the chrome has anything to paint.
    pub(super) fn transform_wants_post_paint(&self) -> bool {
        self.transform_enabled()
            .is_some_and(|d| d.selection_frame().is_some())
    }

    /// Paint the frame and its handles, in scene coordinates.
    pub(super) fn paint_transform_chrome(&self, canvas: &mut Canvas, ctx: &PaintContext) {
        if let Some(d) = self.transform_enabled() {
            d.paint_chrome(canvas, ctx);
        }
    }
}

impl SceneView {
    /// Install the controller's accessibility-action route on the view's own
    /// node.
    ///
    /// A synthetic node has no handlers of its own: the framework resolves an
    /// action aimed at one to its **owner** widget and hands the target node id
    /// along, so this is where a handle's `Increment` becomes a keyboard step.
    /// Without it the actions the frame advertises would be advertised and dead,
    /// which is worse than not advertising them at all.
    pub(super) fn register_transform_handlers(
        &self,
        handlers: teksilo_core::widget_builder::HandlerSet,
        self_id: WidgetId,
    ) -> teksilo_core::widget_builder::HandlerSet {
        let Some(driver) = self.transform_driver() else {
            return handlers;
        };
        handlers.on_access_action_request(move |action, target, data, ctx| {
            use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
            use teksilo_core::event::EventResponse;
            for handle in HANDLE_ORDER {
                let node = synthetic_node_id(
                    self_id,
                    handle.index() as u64 + 1,
                    SyntheticKind::SceneHandle,
                );
                if node == target && driver.access_action(action, data.as_ref(), handle, ctx) {
                    return EventResponse::Handled;
                }
            }
            EventResponse::Ignored
        })
    }
}

impl TransformDriver {
    /// Whether the controller is switched on right now.
    pub fn is_enabled(&self) -> bool {
        self.cfg.enabled.get()
    }

    fn view_scale(&self) -> f32 {
        self.view_transform.get().geometric_scale().max(1e-3)
    }

    /// The frame, its roots and the handles it offers — resolved **once**, and
    /// reused until the model, the selection or the enabled flag moves.
    ///
    /// Every caller that draws, grabs, announces or keyboard-drives the frame
    /// comes through here. Before it, each of them asked for the pieces one at
    /// a time and each question re-pruned the whole selection: a single
    /// `hit_handle` — run on **every pointer move**, whether or not anything is
    /// being dragged — cost thirteen passes of [`Scene::selection_roots`]. It
    /// is now one pass plus one per operation, and a pointer that is merely
    /// moving over an unchanged selection pays none at all.
    ///
    /// [`Scene::selection_roots`]: crate::Scene::selection_roots
    pub(super) fn chrome(&self) -> Option<SelectionChrome> {
        let key = ChromeKey {
            model: self.model.0.borrow().item_change_version(),
            selection: self.selection.selection_signal().generation(),
            enabled: self.cfg.enabled.generation(),
        };
        {
            let memo = self.rt.chrome_memo.borrow();
            if let Some((seen, value)) = memo.as_ref()
                && *seen == key
            {
                return value.clone();
            }
        }
        // Computed with the memo *not* borrowed: the answer reads the scene,
        // and a future reader that reached back in here through it would
        // otherwise panic rather than merely recompute.
        let computed = self.compute_chrome();
        *self.rt.chrome_memo.borrow_mut() = Some((key, computed.clone()));
        computed
    }

    /// The uncached answer. One shared borrow of the scene serves the whole
    /// derivation, so the root set is pruned once rather than once per question.
    fn compute_chrome(&self) -> Option<SelectionChrome> {
        #[cfg(test)]
        self.rt
            .chrome_computes
            .set(self.rt.chrome_computes.get() + 1);
        if !self.is_enabled() {
            return None;
        }
        let selected = self.selection.selected();
        if selected.is_empty() {
            return None;
        }
        let scene = self.model.0.borrow();
        let roots = scene.selection_roots(&selected);
        if roots.is_empty() {
            return None;
        }
        let frame = scene.transform_frame(&roots)?;
        // **Move** applies to whichever roots carry `IS_DRAGGABLE` and is
        // offered when at least one does — a locked item in a mixed selection
        // simply stays where it is, which is what every editor does.
        //
        // **Resize** and **rotate** require *every* root to support them. A
        // partial resize would tear the selection apart: the ineligible members
        // would stand still while the others scaled, and the frame would stop
        // describing the thing it is drawn around.
        let supports = |op: TransformOp| {
            let eligible = scene.transformable_roots(&selected, op);
            match op {
                TransformOp::Move => !eligible.is_empty(),
                TransformOp::Resize | TransformOp::Rotate => eligible.len() == roots.len(),
            }
        };
        let move_ok = supports(TransformOp::Move);
        let resize_ok = supports(TransformOp::Resize);
        let rotate_ok = supports(TransformOp::Rotate);
        let handles: Vec<TransformHandle> = HANDLE_ORDER
            .iter()
            .copied()
            .filter(|h| self.cfg.offers(*h))
            .filter(|h| match h.op() {
                TransformOp::Move => move_ok,
                TransformOp::Resize => resize_ok,
                TransformOp::Rotate => rotate_ok,
            })
            .collect();
        Some(SelectionChrome {
            frame,
            handles: Rc::from(handles),
            supports_move: move_ok,
            supports_resize: resize_ok,
            supports_rotate: rotate_ok,
        })
    }

    /// The roots one operation would actually change.
    ///
    /// Not served from [`chrome`](Self::chrome): this is the *flag-filtered*
    /// set, it is asked once per gesture rather than once per pointer sample,
    /// and caching three of them would grow the memo for nothing.
    pub fn roots_for(&self, op: TransformOp) -> Vec<ItemId> {
        let selected = self.selection.selected();
        if selected.is_empty() {
            return Vec::new();
        }
        self.model.transformable_roots(&selected, op)
    }

    /// The frame the chrome draws, and the one a new gesture starts from.
    pub fn selection_frame(&self) -> Option<TransformFrame> {
        self.chrome().map(|c| c.frame)
    }

    /// Whether the selection can honour `op` at all. See
    /// [`compute_chrome`](Self::compute_chrome) for the rule.
    pub fn supports(&self, op: TransformOp) -> bool {
        self.chrome().is_some_and(|c| match op {
            TransformOp::Move => c.supports_move,
            TransformOp::Resize => c.supports_resize,
            TransformOp::Rotate => c.supports_rotate,
        })
    }

    /// Every handle the frame is currently offering — configured **and**
    /// honourable. What is drawn is exactly what will happen; a handle whose
    /// operation the selection cannot take is not drawn at all.
    pub fn visible_handles(&self) -> Vec<TransformHandle> {
        match self.chrome() {
            Some(c) => c.handles.to_vec(),
            None => Vec::new(),
        }
    }

    /// Scene positions of the visible handles on `frame`.
    ///
    /// [`TransformHandle::Move`] is excluded — it is the frame band, not a
    /// disc, and a dot in the middle of the selection would sit on the content.
    pub fn handle_points(
        &self,
        frame: &TransformFrame,
        view_scale: f32,
    ) -> Vec<(TransformHandle, Point)> {
        self.points_for(&self.visible_handles(), frame, view_scale)
    }

    /// [`handle_points`](Self::handle_points) for a handle list the caller
    /// already has — the form every hot caller uses, so that resolving the
    /// frame and resolving its handles is one question asked once.
    fn points_for(
        &self,
        handles: &[TransformHandle],
        frame: &TransformFrame,
        view_scale: f32,
    ) -> Vec<(TransformHandle, Point)> {
        let scale = view_scale.max(1e-3);
        let pad = self.cfg.padding_px / scale;
        let rot_off = ROTATE_OFFSET_PX / scale;
        handles
            .iter()
            .copied()
            .filter(|h| *h != TransformHandle::Move)
            .map(|h| {
                let p = frame.handle_point(h, pad, rot_off);
                (h, frame.to_scene(p))
            })
            .collect()
    }

    /// Which handle a press at `scene_pt` grabs, if any.
    ///
    /// Handles first (they are drawn on top of the band), then the band itself.
    pub fn hit_handle(&self, scene_pt: Point, slop: GrabSlop) -> Option<TransformHandle> {
        let chrome = self.chrome()?;
        let frame = chrome.frame;
        let scale = self.view_scale();
        let grab = slop.magnet_grab_scene_radius(self.cfg.handle_px * 0.5 + 2.0, scale);
        for (handle, pos) in self.points_for(&chrome.handles, &frame, scale) {
            let dx = scene_pt.x - pos.x;
            let dy = scene_pt.y - pos.y;
            if dx * dx + dy * dy <= grab * grab {
                return Some(handle);
            }
        }
        // The frame band: a ring around the padded outline, entirely outside
        // the selection's own bounds — which is what makes it grabbable over a
        // heavyweight card, whose pixels stop at the content edge.
        if chrome.handles.contains(&TransformHandle::Move) {
            let pad = self.cfg.padding_px / scale;
            let band = (self.cfg.handle_px * 0.5).max(2.0) / scale;
            let outline = frame.outline(pad);
            let p = frame.from_scene(scene_pt);
            let outer = Rect::new(
                outline.x - band,
                outline.y - band,
                outline.width + band * 2.0,
                outline.height + band * 2.0,
            );
            let inner = Rect::new(
                outline.x + band,
                outline.y + band,
                (outline.width - band * 2.0).max(0.0),
                (outline.height - band * 2.0).max(0.0),
            );
            if outer.contains(p) && !inner.contains(p) {
                return Some(TransformHandle::Move);
            }
        }
        None
    }

    /// Whether `scene_pt` lands on the body of a selected, movable item.
    ///
    /// The lightweight tier answers by its real shape; a heavyweight entry has
    /// no `SceneItem` to ask, so its scene rectangle is the answer — which is
    /// also exactly the rectangle the arena laid its card out at.
    pub fn hit_body(&self, scene_pt: Point) -> bool {
        if !self.is_enabled() || !self.cfg.body_drag {
            return false;
        }
        let roots = self.roots_for(TransformOp::Move);
        if roots.is_empty() {
            return false;
        }
        let view_scale = self.view_scale();
        let scene = self.model.0.borrow();
        // What is on **top** here, not merely what is here: asking each movable
        // root whether it contains the point would let a selected item buried
        // under an unselected one answer for it, and the answer has to agree
        // with what the user can see.
        let Some(top) = scene.entry_at(scene_pt, view_scale) else {
            return false;
        };
        roots
            .into_iter()
            .any(|root| root == top || scene.is_descendant_of(top, root))
    }

    /// Start a gesture on `handle`, grabbed at `anchor_scene`.
    ///
    /// Returns `false` (and starts nothing) when the selection cannot honour
    /// the handle's operation.
    pub fn begin(
        &self,
        handle: TransformHandle,
        anchor_scene: Point,
        current_screen: Option<Point>,
        source: TransformSource,
        ctx: &mut EventContext,
    ) -> bool {
        if !self.is_enabled() || !self.supports(handle.op()) {
            return false;
        }
        let roots = self.roots_for(handle.op());
        if roots.is_empty() {
            return false;
        }
        let Some(start_frame) = self.selection_frame() else {
            return false;
        };
        let aspect_locked = {
            let scene = self.model.0.borrow();
            roots.iter().any(|id| {
                scene
                    .flags(*id)
                    .is_some_and(|f| f.contains(crate::flags::ItemFlags::ASPECT_LOCKED))
            })
        };
        let session = LiveSession {
            roots: Rc::from(roots),
            handle,
            start_frame,
            anchor_scene,
            current_screen,
            keyboard_step: Vec2::ZERO,
            keyboard_rotation: 0.0,
            source,
            aspect_locked,
        };
        let resolved = self.resolve(&session);
        let snapshot = session.snapshot(resolved);
        *self.rt.session.borrow_mut() = Some(session);
        self.rt.published.set(Some(snapshot.clone()));
        self.rt.bump_all();
        // Esc has to reach *this* view for the rest of the gesture, and the key
        // handler is the focused-widget surface. The view is focusable, so this
        // is one call.
        //
        // It is a **loan**, not a transfer. A caret in a card's `TextInput` is
        // the user's place in their own work and a drag is not a request to
        // leave it, so a pointer gesture writes down what it displaced and
        // `commit` / `cancel` put it back. The keyboard and AT routes record
        // nothing: they arrived through a focus this view already had, or
        // through an action that does not depend on focus at all.
        if let Some(self_id) = self.self_id {
            if source == TransformSource::Pointer && ctx.focused() != Some(self_id) {
                self.rt.focus_before.set(ctx.focused());
            }
            ctx.request_focus(self_id);
        }
        if let Some(f) = &self.cfg.on_start {
            f(&snapshot, ctx);
        }
        true
    }

    /// Resolve a session, projecting through the live view transform and
    /// applying the scene's geometry constraint.
    ///
    /// The scene is borrowed **shared** for the call and released before it
    /// returns, so a constraint may read the model freely and every caller here
    /// is free to write afterwards. An unconstrained scene pays the borrow and
    /// one `Option` test; the closure is never built.
    pub fn resolve(&self, session: &LiveSession) -> ResolvedSession {
        let xform = self.view_transform.get();
        let inverse = xform.inverse();
        let to_scene = |p: Point| match inverse {
            Some(inv) => inv.apply_point(p),
            None => p,
        };
        let scale = xform.geometric_scale().max(1e-3);
        let scene = self.model.0.borrow();
        let call = crate::constrain::ConstraintCall::new(&scene);
        session.resolve(&self.cfg, scale, to_scene, call.as_ref())
    }

    /// The live gesture resolved, if one is running.
    pub fn resolved(&self) -> Option<(LiveSession, ResolvedSession)> {
        if !self.is_enabled() {
            return None;
        }
        let session = self.rt.session.borrow().clone()?;
        let resolved = self.resolve(&session);
        Some((session, resolved))
    }

    /// See [`SceneView::transform_preview`].
    pub fn preview(&self) -> Option<(Rc<[ItemId]>, Transform2D)> {
        if self.cfg.live_preview == LivePreview::Ghost {
            return None;
        }
        let (session, resolved) = self.resolved()?;
        if resolved.delta.is_identity() {
            return None;
        }
        Some((session.roots.clone(), resolved.delta.to_scene_transform()))
    }

    /// Whether a live gesture is running.
    pub fn is_active(&self) -> bool {
        self.rt.session.borrow().is_some()
    }

    /// Update the pointer position of the live gesture and fire `on_change`.
    pub fn update(&self, screen: Point, ctx: &mut EventContext) {
        let snapshot = {
            let mut slot = self.rt.session.borrow_mut();
            let Some(session) = slot.as_mut() else {
                return;
            };
            session.current_screen = Some(screen);
            let session = session.clone();
            drop(slot);
            let resolved = self.resolve(&session);
            session.snapshot(resolved)
        };
        self.rt.published.set(Some(snapshot.clone()));
        self.rt.bump();
        self.drive_edge_pan(screen);
        if let Some(f) = &self.cfg.on_change {
            f(&snapshot, ctx);
        }
    }

    /// Finish the live gesture, posting its delta for `build()` to apply.
    ///
    /// The **only** place a transform reaches the model, and it goes through one
    /// [`Scene::apply_transform_delta`](crate::Scene::apply_transform_delta) —
    /// so a gesture is one call is one reversible step, without this crate
    /// owning a history.
    pub fn commit(&self, ctx: &mut EventContext) {
        let Some(session) = self.rt.session.borrow_mut().take() else {
            return;
        };
        let resolved = self.resolve(&session);
        let snapshot = session.snapshot(resolved);
        let committed = !resolved.delta.is_identity();
        if committed {
            *self.rt.pending_commit.borrow_mut() = Some((session.roots.to_vec(), resolved.delta));
            self.reconcile_dirty
                .set(self.reconcile_dirty.get().wrapping_add(1));
            announce_transform(&snapshot, ctx);
        }
        self.rt.published.set(None);
        self.rt.bump_all();
        self.stop_edge_pan();
        self.restore_focus(ctx);
        if let Some(f) = &self.cfg.on_end {
            f(
                &snapshot,
                if committed {
                    TransformOutcome::Committed
                } else {
                    TransformOutcome::Cancelled
                },
                ctx,
            );
        }
    }

    /// Drop the live gesture without writing anything. Reports whether there
    /// was one.
    ///
    /// **The** cancel path, and the only one. A revoked contact reaches it
    /// through [`DragUnwind`](super::DragUnwind) rather than through
    /// [`TransformRuntime::abort`], because `abort` drops the session and
    /// nothing else: routing a cancel there left the edge auto-pan tweening for
    /// up to thirty seconds with no input, and never delivered the
    /// [`TransformOutcome::Cancelled`] an app tears its overlay down on.
    pub fn cancel(&self, ctx: &mut EventContext) -> bool {
        let Some(session) = self.rt.abort() else {
            // Focus is still given back: `begin` may have taken it for a
            // gesture that another path has already ended.
            self.restore_focus(ctx);
            return false;
        };
        self.stop_edge_pan();
        self.restore_focus(ctx);
        if let Some(f) = &self.cfg.on_end {
            let resolved = self.resolve(&session);
            f(
                &session.snapshot(resolved),
                TransformOutcome::Cancelled,
                ctx,
            );
        }
        true
    }

    /// Give back the focus a pointer gesture borrowed. See [`begin`](Self::begin).
    fn restore_focus(&self, ctx: &mut EventContext) {
        if let Some(previous) = self.rt.focus_before.take() {
            ctx.request_focus(previous);
        }
    }

    /// Recompute which handle the pointer is over, for the chrome and the
    /// cursor. Returns the cursor this controller wants, if any.
    pub fn hover(
        &self,
        scene_pt: Point,
        slop: GrabSlop,
    ) -> Option<teksilo_core::widget::CursorIcon> {
        if !self.is_enabled() {
            return None;
        }
        if let Some((session, _)) = self.resolved() {
            return Some(session.handle.cursor());
        }
        let hit = self.hit_handle(scene_pt, slop);
        if self.rt.hovered.get() != hit {
            self.rt.hovered.set(hit);
            self.rt.bump();
        }
        hit.map(|h| h.cursor())
    }

    /// Paint the chrome.
    pub fn paint_chrome(&self, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(state) = self.chrome() else {
            return;
        };
        let live = self.resolved();
        let frame = match &live {
            Some((_, r)) => r.frame,
            None => state.frame,
        };
        let view_scale = self.view_scale();
        let pad = self.cfg.padding_px / view_scale;
        let chrome = TransformChrome {
            outline: frame.outline(pad),
            handles: self.points_for(&state.handles, &frame, view_scale),
            handle_half: (self.cfg.handle_px * 0.5) / view_scale,
            view_scale,
            hovered: self.rt.hovered.get(),
            active: live.as_ref().map(|(s, _)| s.handle),
            keyboard_focus: if self.rt.keyboard_mode.get() {
                self.rt.keyboard_focus.get()
            } else {
                None
            },
            frame,
        };
        match &self.cfg.chrome {
            Some(painter) => painter(canvas, ctx, &chrome),
            None => paint_default_chrome(canvas, ctx, &chrome),
        }
    }

    /// Handle one key for the transform controller. Returns `true` when the key
    /// was consumed.
    ///
    /// The route is the pointer's twin by construction: it drives the same
    /// [`LiveSession`] and commits through the same [`commit`](Self::commit), so
    /// `docs/a11y/non-drag-alternatives.md`'s "the alternative must make the
    /// same model change the drag makes" is structural rather than asserted.
    pub fn handle_key(&self, key: &Key, modifiers: Modifiers, ctx: &mut EventContext) -> bool {
        if !self.is_enabled() {
            return false;
        }
        // The mode key toggles, from any state.
        if key_matches(key, &self.cfg.transform_key)
            && !modifiers.alt()
            && !modifiers.command()
            && !modifiers.ctrl()
        {
            if self.rt.keyboard_mode.get() {
                self.cancel(ctx);
                self.rt.keyboard_mode.set(false);
                self.rt.keyboard_focus.set(None);
            } else {
                let handles = self.visible_handles();
                let Some(first) = handles.first().copied() else {
                    return false;
                };
                self.rt.keyboard_mode.set(true);
                self.rt.keyboard_focus.set(Some(first));
            }
            self.rt.bump_all();
            ctx.request_accessibility_update();
            ctx.request_frame();
            return true;
        }
        // Esc ends a gesture the **pointer** started. It used to live only
        // inside the keyboard-mode branch below, and a pointer session never
        // sets that mode — so the escape hatch this design leads with ("cancel
        // is `session = None`, there is no rollback to get wrong") could not be
        // reached from a mouse or a finger at all, and the focus `begin` takes
        // so that this key can arrive bought nothing.
        //
        // In keyboard mode the arm below handles it instead, and deliberately
        // differently: there, the first Esc abandons the step and the second
        // leaves the mode.
        if matches!(key, Key::Escape) && !self.rt.keyboard_mode.get() {
            if !self.cancel(ctx) {
                return false;
            }
            self.rt.bump_all();
            ctx.request_accessibility_update();
            ctx.request_frame();
            return true;
        }
        if !self.rt.keyboard_mode.get() {
            return false;
        }
        let handles = self.visible_handles();
        if handles.is_empty() {
            self.rt.keyboard_mode.set(false);
            self.rt.keyboard_focus.set(None);
            return false;
        }
        match key {
            Key::Escape => {
                if self.is_active() {
                    self.cancel(ctx);
                } else {
                    self.rt.keyboard_mode.set(false);
                    self.rt.keyboard_focus.set(None);
                }
            }
            Key::Enter | Key::Space => self.commit(ctx),
            Key::Tab => {
                let cur = self.rt.keyboard_focus.get();
                let idx = cur
                    .and_then(|c| handles.iter().position(|h| *h == c))
                    .unwrap_or(0);
                let next = if modifiers.shift() {
                    (idx + handles.len() - 1) % handles.len()
                } else {
                    (idx + 1) % handles.len()
                };
                // Roving to another handle abandons what the current one had
                // accumulated — two handles cannot be half-dragged at once.
                self.cancel(ctx);
                self.rt.keyboard_focus.set(Some(handles[next]));
            }
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                // `Alt+Arrow` is the immediate nudge and stays that, even in
                // transform mode: two keyboard move routes that both answered
                // the same chord would make the mode a trap.
                if modifiers.alt() {
                    return false;
                }
                let Some(handle) = self.rt.keyboard_focus.get() else {
                    return false;
                };
                if !self.step(handle, key, modifiers, ctx) {
                    return false;
                }
            }
            _ => return false,
        }
        self.rt.bump_all();
        ctx.request_accessibility_update();
        ctx.request_frame();
        true
    }

    /// One keyboard step on `handle`, starting a session if none is live.
    fn step(
        &self,
        handle: TransformHandle,
        key: &Key,
        modifiers: Modifiers,
        ctx: &mut EventContext,
    ) -> bool {
        // A session for a different handle is abandoned, not extended.
        let wrong_handle = self
            .rt
            .session
            .borrow()
            .as_ref()
            .is_some_and(|s| s.handle != handle);
        if wrong_handle {
            self.cancel(ctx);
        }
        if !self.is_active() {
            let anchor = self
                .selection_frame()
                .map(|f| f.centre_scene())
                .unwrap_or(Point::ZERO);
            if !self.begin(handle, anchor, None, TransformSource::Keyboard, ctx) {
                return false;
            }
        }
        let snapshot = {
            let mut slot = self.rt.session.borrow_mut();
            let Some(session) = slot.as_mut() else {
                return false;
            };
            let mult = if modifiers.shift() {
                KEY_STEP_SHIFT
            } else {
                1.0
            };
            match handle {
                TransformHandle::Rotate => {
                    let step = KEY_ROTATE * mult;
                    match key {
                        Key::ArrowLeft | Key::ArrowUp => session.keyboard_rotation -= step,
                        _ => session.keyboard_rotation += step,
                    }
                }
                _ => {
                    let step = KEY_STEP * mult;
                    match key {
                        Key::ArrowLeft => session.keyboard_step.x -= step,
                        Key::ArrowRight => session.keyboard_step.x += step,
                        Key::ArrowUp => session.keyboard_step.y -= step,
                        _ => session.keyboard_step.y += step,
                    }
                }
            }
            let session = session.clone();
            drop(slot);
            let resolved = self.resolve(&session);
            session.snapshot(resolved)
        };
        self.rt.published.set(Some(snapshot.clone()));
        if let Some(f) = &self.cfg.on_change {
            f(&snapshot, ctx);
        }
        true
    }

    /// Route one AccessKit action aimed at a handle's synthetic node.
    ///
    /// `Increment` / `Decrement` on a handle are the screen-reader form of the
    /// keyboard step, and `Click` / `Focus` on one puts the roving keyboard
    /// mode on it — the non-drag alternative the drag owes, through the same
    /// session and the same commit.
    ///
    /// **Which way a verb pushes is the handle's own business**, not the
    /// route's: [`TransformHandle::at_steps`] turns one verb into the one or
    /// two directions that handle drives. It used to be a fixed
    /// `Increment → ArrowRight`, which meant every verb was a horizontal
    /// nudge: the "Resize top" and "Resize bottom" sliders reported the action
    /// *handled*, announced an unchanged value, and moved nothing — so an
    /// item's height could not be changed from assistive technology at all.
    ///
    /// A non-scalar handle — a corner, or the frame band — also publishes four
    /// named custom actions, one per direction, and they arrive here as
    /// [`accesskit::Action::CustomAction`] carrying the
    /// [`TransformStep::index`] as their id.
    pub fn access_action(
        &self,
        action: accesskit::Action,
        data: Option<&accesskit::ActionData>,
        handle: TransformHandle,
        ctx: &mut EventContext,
    ) -> bool {
        if !self.is_enabled() || !self.visible_handles().contains(&handle) {
            return false;
        }
        let steps: &'static [TransformStep] = match action {
            accesskit::Action::Increment => handle.at_steps(true),
            accesskit::Action::Decrement => handle.at_steps(false),
            accesskit::Action::CustomAction => {
                // A scalar handle publishes none, so one arriving for it is a
                // stale tree rather than a direction this node ever offered.
                if handle.is_scalar() {
                    return false;
                }
                let Some(accesskit::ActionData::CustomAction(idx)) = data else {
                    return false;
                };
                let Ok(idx) = usize::try_from(*idx) else {
                    return false;
                };
                let Some(step) = TransformStep::from_index(idx) else {
                    return false;
                };
                step.as_slice()
            }
            accesskit::Action::Click | accesskit::Action::Focus => {
                self.rt.keyboard_mode.set(true);
                self.rt.keyboard_focus.set(Some(handle));
                self.rt.bump_all();
                ctx.request_accessibility_update();
                return true;
            }
            _ => return false,
        };
        self.rt.keyboard_mode.set(true);
        self.rt.keyboard_focus.set(Some(handle));
        // Two steps for a diagonal, and deliberately through the same `step`
        // the arrow keys use: the second call sees the same handle and extends
        // the session the first started, so one verb is still one session and
        // one commit.
        let mut stepped = false;
        for step in steps {
            stepped |= self.step(handle, &step.key(), Modifiers::default(), ctx);
        }
        if !stepped {
            return false;
        }
        // An assistive-technology step is a whole edit, not half of one: no
        // Enter is coming, so it commits on the spot.
        self.commit(ctx);
        self.rt.bump_all();
        ctx.request_accessibility_update();
        true
    }

    /// Aim (or stop) the edge auto-pan for a pointer sample at `screen`.
    ///
    /// No frame tick of its own: the pan signals are already animated and
    /// already bound at `Relayout`, so the tween drives the relayout that
    /// re-derives the preview, and the session — which stores a **screen** point
    /// and re-projects it every time it is read — keeps the selection under the
    /// pointer as the scene slides beneath it.
    pub fn drive_edge_pan(&self, screen: Point) {
        if self.cfg.edge_pan_px <= 0.0 || !self.is_active() {
            return;
        }
        let origin = self.bounds_origin.get();
        let size = self.viewport.get();
        if size.width <= 0.0 || size.height <= 0.0 {
            return;
        }
        let band = self.cfg.edge_pan_px;
        // How far into each edge band the pointer has pushed. Panning right
        // moves the content right, which reveals what is to the *left*, so a
        // pointer at the leading edge asks for a positive pan.
        let left = band - (screen.x - origin.x);
        let right = band - (origin.x + size.width - screen.x);
        let top = band - (screen.y - origin.y);
        let bottom = band - (origin.y + size.height - screen.y);
        let dx = if left > 0.0 {
            left
        } else if right > 0.0 {
            -right
        } else {
            0.0
        };
        let dy = if top > 0.0 {
            top
        } else if bottom > 0.0 {
            -bottom
        } else {
            0.0
        };
        if dx == 0.0 && dy == 0.0 {
            self.stop_edge_pan();
            return;
        }
        // Aim far enough that a stationary pointer keeps panning, at a constant
        // speed. `clamp_pan` caps it where the scene declares bounds.
        let reach = size.width.max(size.height) * 4.0;
        let len = (dx * dx + dy * dy).sqrt().max(1e-3);
        let target = super::clamp_pan(
            Vec2::new(
                self.pan_x.get() + dx / len * reach,
                self.pan_y.get() + dy / len * reach,
            ),
            self.model.pan_bounds_signal().get(),
            self.pan_bounds_override.get(),
            size,
            self.zoom.get(),
        );
        let travel =
            ((target.x - self.pan_x.get()).powi(2) + (target.y - self.pan_y.get()).powi(2)).sqrt();
        if travel < 0.5 {
            return;
        }
        let secs = travel / self.cfg.edge_pan_speed;
        let dur = std::time::Duration::from_secs_f32(secs.clamp(0.016, 30.0));
        self.rt.edge_panning.set(true);
        self.pan_x
            .animate_to(target.x, dur, teksilo_tokens::Easing::Linear);
        self.pan_y
            .animate_to(target.y, dur, teksilo_tokens::Easing::Linear);
    }

    /// Stop an auto-pan tween wherever it has got to.
    ///
    /// Two things this is not. It is not a `set`: the scheduler owns an
    /// animated signal while its tween runs, so writing the current value would
    /// be overwritten on the next frame rather than stopping anything — arming
    /// a new one-millisecond request to where the pan has *got to* replaces the
    /// old one, which is what actually stops it. And it is not unconditional:
    /// `pan_x` / `pan_y` are bound at `Relayout`, so touching them on every
    /// sample would schedule a relayout per sample for a controller whose
    /// auto-pan never fired.
    fn stop_edge_pan(&self) {
        if !self.rt.edge_panning.replace(false) {
            return;
        }
        let tick = std::time::Duration::from_millis(1);
        self.pan_x
            .animate_to(self.pan_x.get(), tick, teksilo_tokens::Easing::Linear);
        self.pan_y
            .animate_to(self.pan_y.get(), tick, teksilo_tokens::Easing::Linear);
    }
}

/// Whether two keys denote the same logical key, case-insensitively for
/// character keys.
fn key_matches(a: &Key, b: &Key) -> bool {
    match (a.to_char(), b.to_char()) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
        _ => a == b,
    }
}

/// Say what just happened, once, and only when something did.
fn announce_transform(session: &TransformSession, ctx: &mut EventContext) {
    let f = session.frame;
    let message = match session.op() {
        TransformOp::Move => {
            if session.items.len() == 1 {
                "Moved 1 item".to_string()
            } else {
                format!("Moved {} items", session.items.len())
            }
        }
        TransformOp::Resize => format!(
            "Resized to {} by {}",
            f.rect.width.round() as i64,
            f.rect.height.round() as i64
        ),
        TransformOp::Rotate => {
            let deg = f.rotation.to_degrees().round() as i64;
            format!("Rotated to {deg} degrees")
        }
    };
    ctx.announce(message);
}

/// The built-in chrome: a hairline frame plus a handle per anchor, all at a
/// constant on-screen size.
fn paint_default_chrome(canvas: &mut Canvas, ctx: &PaintContext, chrome: &TransformChrome) {
    let accent = ctx.theme.colors.accent;
    let ring = ctx.theme.colors.border_focused;
    let fill = ctx.theme.colors.surface_raised;
    let hair = (1.0 / chrome.view_scale).max(0.001);
    canvas.save();
    // The frame is stated in its own basis; rotate into it once and every
    // rectangle below is axis-aligned again.
    if chrome.frame.rotation.abs() > 1e-6 {
        canvas.rotate(chrome.frame.rotation);
    }
    canvas.stroke_rect(chrome.outline, accent, StrokeStyle::solid(hair));
    if chrome
        .handles
        .iter()
        .any(|(h, _)| *h == TransformHandle::Rotate)
    {
        // A stem from the frame's top edge to the puck, so the puck reads as
        // attached rather than floating. Drawn inside the frame's basis,
        // alongside the outline it grows out of.
        let cx = chrome.outline.x + chrome.outline.width * 0.5;
        let puck = chrome.frame.handle_point(TransformHandle::Rotate, 0.0, 0.0);
        let stem_top = Point::new(cx, puck.y + (chrome.outline.y - chrome.frame.rect.y));
        canvas.draw_line(
            Point::new(cx, chrome.outline.y),
            stem_top,
            accent,
            StrokeStyle::solid(hair),
        );
    }
    canvas.restore();

    let half = chrome.handle_half;
    for (handle, pos) in &chrome.handles {
        let emphasised = chrome.active == Some(*handle) || chrome.keyboard_focus == Some(*handle);
        let hot = emphasised || chrome.hovered == Some(*handle);
        let r = if hot { half * 1.35 } else { half };
        let body = if hot { accent } else { fill };
        let edge = if emphasised { ring } else { accent };
        if *handle == TransformHandle::Rotate {
            canvas.fill_circle(*pos, r, body);
            canvas.stroke_circle(*pos, r, edge, StrokeStyle::solid(hair));
            continue;
        }
        let box_rect = Rect::new(pos.x - r, pos.y - r, r * 2.0, r * 2.0);
        canvas.fill_rect(box_rect, body);
        canvas.stroke_rect(box_rect, edge, StrokeStyle::solid(hair));
    }
}
