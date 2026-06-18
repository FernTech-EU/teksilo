// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! View-side magnetism internals: the in-flight port-drag state and the
//! built-in feedback renderer.
//!
//! The public magnetism types ([`MagnetismConfig`](crate::MagnetismConfig),
//! [`MagnetFeedback`](crate::MagnetFeedback), …) and the snap mechanism
//! live in [`crate::magnet`] / [`crate::scene`]. This module holds only
//! the per-view runtime state the [`SceneView`](crate::SceneView) drag /
//! paint / keyboard code threads around, plus the default renderer used
//! when the consumer installs no custom [`MagnetismConfig::feedback`].

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use bastyde_core::binding::BindingLevel;
use bastyde_core::event::Key;
use bastyde_core::widget::{EventContext, PaintContext};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Color;

use super::SceneView;
use crate::magnet::{
    MagnetConnection, MagnetFeedback, MagnetId, MagnetMarker, MagnetVerdict, MagnetVisualState,
    MagnetismConfig,
};
use crate::scene_model::SceneModel;

/// Resolve a `(from, to)` magnet pair plus the verdict payload into a
/// [`MagnetConnection`] for the consumer's `on_connect`. `None` if
/// either magnet has gone away (e.g. its item was removed mid-drag).
pub(super) fn build_connection(
    model: &SceneModel,
    from: MagnetId,
    to: MagnetId,
    payload: Option<Rc<dyn Any>>,
) -> Option<MagnetConnection> {
    let from = model.magnet(from)?;
    let to = model.magnet(to)?;
    Some(MagnetConnection { from, to, payload })
}

/// Whether two keys denote the same logical key, case-insensitively for
/// character keys (so `m` matches whether or not Shift is held).
fn key_matches(a: &Key, b: &Key) -> bool {
    match (a.to_char(), b.to_char()) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
        _ => a == b,
    }
}

/// Every enabled magnet in the scene as `(id, scene_pos)`, in scene
/// insertion order (item order, then magnet order). The deterministic
/// traversal order for the keyboard connect cycle.
fn enabled_magnets(model: &SceneModel) -> Vec<(MagnetId, Point)> {
    let mut out = Vec::new();
    for item in model.ids() {
        for mid in model.magnet_ids_of(item) {
            if let Some(pos) = model.magnet_scene_pos(mid) {
                out.push((mid, pos));
            }
        }
    }
    out
}

/// The first enabled magnet in traversal order, if any.
fn first_enabled_magnet(model: &SceneModel) -> Option<MagnetId> {
    enabled_magnets(model).first().map(|(id, _)| *id)
}

/// Whether a candidate magnet is a legal target for the pending source
/// under the predicate. `true` when there is no pending source (free
/// navigation) or the candidate is the source itself is excluded.
fn passes_gate(
    model: &SceneModel,
    cfg: &MagnetismConfig,
    pending: Option<MagnetId>,
    cand: MagnetId,
) -> bool {
    match pending {
        None => true,
        Some(src) if src == cand => false,
        Some(src) => match (model.magnet(src), model.magnet(cand)) {
            (Some(a), Some(b)) => (cfg.predicate)(&a, &b).is_accept(),
            _ => false,
        },
    }
}

/// The next magnet to focus when an arrow is pressed: the nearest gated
/// candidate in the pressed direction from `from`; if none lies ahead,
/// the globally nearest gated candidate (so navigation never dead-ends).
fn nearest_in_direction(
    model: &SceneModel,
    cfg: &MagnetismConfig,
    from: Option<MagnetId>,
    dir: (f32, f32),
    pending: Option<MagnetId>,
) -> Option<MagnetId> {
    let all = enabled_magnets(model);
    let from_pos = from.and_then(|f| model.magnet_scene_pos(f));
    let mut ahead: Option<(MagnetId, f32)> = None;
    let mut any: Option<(MagnetId, f32)> = None;
    for (mid, pos) in &all {
        if Some(*mid) == from {
            continue;
        }
        if !passes_gate(model, cfg, pending, *mid) {
            continue;
        }
        let (dx, dy) = match from_pos {
            Some(fp) => (pos.x - fp.x, pos.y - fp.y),
            None => (pos.x, pos.y),
        };
        let dist = (dx * dx + dy * dy).sqrt();
        if any.map(|b| dist < b.1).unwrap_or(true) {
            any = Some((*mid, dist));
        }
        // "Ahead" = positive projection on the pressed direction.
        if dx * dir.0 + dy * dir.1 > 0.0 && ahead.map(|b| dist < b.1).unwrap_or(true) {
            ahead = Some((*mid, dist));
        }
    }
    ahead.or(any).map(|(id, _)| id)
}

/// Pan so the given magnet is on-screen, via the SceneView's
/// `ensure_visible`. Deferred through `with_widget_mut` because the key
/// closure has no `&self`.
fn reveal_focus(
    model: &SceneModel,
    mid: Option<MagnetId>,
    self_id: Option<WidgetId>,
    ctx: &mut EventContext,
) {
    let (Some(mid), Some(self_id)) = (mid, self_id) else {
        return;
    };
    let Some(pos) = model.magnet_scene_pos(mid) else {
        return;
    };
    // A small box around the magnet so ensure_visible leaves margin.
    let rect = Rect::new(pos.x - 8.0, pos.y - 8.0, 16.0, 16.0);
    ctx.with_widget_mut::<SceneView>(self_id, BindingLevel::Relayout, move |v| {
        v.ensure_visible(rect, 24.0);
    });
}

/// Handle a key for the magnetism keyboard connect flow. Returns `true`
/// if the key was consumed. Toggles connect mode on the config's connect
/// key; while in connect mode, arrows / Home / End move the virtual
/// focus (gated by the predicate once a source is pending), Enter / Space
/// activate the source then form the connection, and Esc cancels the
/// pending source or exits the mode.
#[allow(clippy::too_many_arguments)] // cohesive keyboard-connect state, threaded from the on_key closure
pub(super) fn handle_connect_key(
    key: &Key,
    cfg: &MagnetismConfig,
    model: &SceneModel,
    connect_mode: &Rc<Cell<bool>>,
    focus: &Rc<Cell<Option<MagnetId>>>,
    pending: &Rc<Cell<Option<MagnetId>>>,
    self_id: Option<WidgetId>,
    ctx: &mut EventContext,
) -> bool {
    // Toggle connect mode on the configured key, from any state.
    if key_matches(key, &cfg.connect_key) {
        let now = !connect_mode.get();
        connect_mode.set(now);
        if now {
            focus.set(first_enabled_magnet(model));
            reveal_focus(model, focus.get(), self_id, ctx);
        } else {
            focus.set(None);
            pending.set(None);
        }
        ctx.request_accessibility_update();
        ctx.request_frame();
        return true;
    }
    if !connect_mode.get() {
        return false;
    }
    match key {
        Key::Escape => {
            if pending.get().is_some() {
                pending.set(None);
            } else {
                connect_mode.set(false);
                focus.set(None);
            }
        }
        Key::Enter | Key::Space => {
            if let Some(cur) = focus.get() {
                match pending.get() {
                    None => pending.set(Some(cur)),
                    Some(src) => {
                        if src != cur
                            && let (Some(a), Some(b)) = (model.magnet(src), model.magnet(cur))
                                && let MagnetVerdict::Accept(payload) = (cfg.predicate)(&a, &b)
                                    && let Some(conn) = build_connection(model, src, cur, payload) {
                                        (cfg.on_connect)(&conn, ctx);
                                    }
                        pending.set(None);
                    }
                }
            }
        }
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
            let dir = match key {
                Key::ArrowLeft => (-1.0, 0.0),
                Key::ArrowRight => (1.0, 0.0),
                Key::ArrowUp => (0.0, -1.0),
                _ => (0.0, 1.0),
            };
            if let Some(next) = nearest_in_direction(model, cfg, focus.get(), dir, pending.get()) {
                focus.set(Some(next));
                reveal_focus(model, Some(next), self_id, ctx);
            }
        }
        Key::Home => {
            let next = enabled_magnets(model)
                .into_iter()
                .map(|(id, _)| id)
                .find(|id| passes_gate(model, cfg, pending.get(), *id));
            if next.is_some() {
                focus.set(next);
                reveal_focus(model, next, self_id, ctx);
            }
        }
        Key::End => {
            let next = enabled_magnets(model)
                .into_iter()
                .map(|(id, _)| id)
                .rfind(|id| passes_gate(model, cfg, pending.get(), *id));
            if next.is_some() {
                focus.set(next);
                reveal_focus(model, next, self_id, ctx);
            }
        }
        _ => return false,
    }
    ctx.request_accessibility_update();
    ctx.request_frame();
    true
}

/// An in-flight port-drag: the user grabbed a magnet handle and is
/// dragging a transient wire whose free end follows the cursor.
#[derive(Clone)]
pub(crate) struct PortDragState {
    /// The grabbed source magnet.
    pub source: MagnetId,
    /// The source magnet's scene position (fixed wire anchor).
    pub source_scene: Point,
    /// The current free end of the wire in scene coords.
    pub cursor_scene: Point,
    /// The currently-snapped target, if the cursor is within range of
    /// an accepting magnet: `(target id, target scene pos, verdict payload)`.
    pub snapped: Option<(MagnetId, Point, Option<Rc<dyn Any>>)>,
}

/// Marker base radius in screen pixels (converted to scene units via
/// `/ zoom` so it stays constant on screen at any zoom).
const MARKER_PX: f32 = 4.5;
/// Emphasis ring radius in screen pixels for focused / pending magnets.
const RING_PX: f32 = 8.0;
/// Connector stroke width in screen pixels.
const WIRE_PX: f32 = 2.0;

fn marker_color(state: MagnetVisualState) -> Color {
    match state {
        MagnetVisualState::Idle => Color::new(0.45, 0.55, 0.75, 0.65),
        MagnetVisualState::Candidate => Color::new(0.30, 0.55, 0.95, 0.95),
        MagnetVisualState::Snapped => Color::new(0.25, 0.75, 0.40, 1.0),
        MagnetVisualState::Focused => Color::new(0.95, 0.70, 0.20, 1.0),
        MagnetVisualState::PendingSource => Color::new(0.95, 0.45, 0.20, 1.0),
    }
}

/// The default magnetism feedback renderer: a marker dot per eligible
/// magnet (coloured by state, constant pixel size) plus a bezier
/// connector for any forming connection. Paints in scene coordinates.
pub(crate) fn render_default_feedback(canvas: &mut Canvas, _ctx: &PaintContext, fb: &MagnetFeedback) {
    let zoom = if fb.zoom.is_finite() && fb.zoom > 0.0 {
        fb.zoom
    } else {
        1.0
    };
    let marker_r = MARKER_PX / zoom;
    let ring_r = RING_PX / zoom;
    let wire_w = (WIRE_PX / zoom).max(0.001);

    // Connector first, so markers sit on top of the wire ends.
    if let Some((from, to, accepted)) = fb.connector {
        let color = if accepted {
            Color::new(0.25, 0.75, 0.40, 0.95)
        } else {
            Color::new(0.35, 0.60, 0.95, 0.75)
        };
        let mut path = Path::new();
        path.move_to(from);
        // Horizontal-handled cubic, the node-graph wire look. Handle
        // length is half the horizontal span (min 24 scene units) so a
        // near-vertical connector still bows sensibly.
        let dx = (to.x - from.x).abs().max(48.0) * 0.5;
        path.cubic_to(
            Point::new(from.x + dx, from.y),
            Point::new(to.x - dx, to.y),
            to,
        );
        canvas.stroke_path(&path, color, StrokeStyle::solid(wire_w));
    }

    for m in &fb.markers {
        let color = marker_color(m.state);
        // Emphasis ring behind the dot for focused / pending / snapped.
        if matches!(
            m.state,
            MagnetVisualState::Focused
                | MagnetVisualState::PendingSource
                | MagnetVisualState::Snapped
        ) {
            canvas.stroke_circle(
                m.scene_pos,
                ring_r,
                color,
                StrokeStyle::solid((1.5 / zoom).max(0.001)),
            );
        }
        canvas.fill_circle(m.scene_pos, marker_r, color);
    }
}

impl SceneView {
    /// The active magnetism config for this view: `Some` iff a config is
    /// installed and its enabled signal is currently `true`.
    pub(super) fn magnetism_active(&self) -> Option<Rc<MagnetismConfig>> {
        let cfg = self.magnetism.clone()?;
        if cfg.enabled.get() { Some(cfg) } else { None }
    }

    /// Whether any magnetism interaction is currently in flight (an
    /// item-drag snap, a port-drag, or keyboard connect mode).
    pub(super) fn magnet_interaction_active(&self) -> bool {
        self.item_snap.borrow().is_some()
            || self.port_drag.borrow().is_some()
            || self.magnet_connect_mode.get()
    }

    /// Whether the post-paint pass must run for magnetism this frame:
    /// magnetism is enabled and either markers are always shown or an
    /// interaction is forming.
    pub(super) fn magnet_wants_post_paint(&self) -> bool {
        match self.magnetism_active() {
            Some(cfg) => {
                matches!(cfg.markers, crate::magnet::MarkerVisibility::Always)
                    || self.magnet_interaction_active()
            }
            None => false,
        }
    }

    /// Paint the magnetism feedback (markers + connector) for this frame,
    /// using the config's custom renderer or the built-in one. Called
    /// from `post_paint`, in the view-transform (scene-coord) scope.
    pub(super) fn paint_magnet_feedback(
        &self,
        bounds: Rect,
        canvas: &mut Canvas,
        ctx: &PaintContext,
    ) {
        let Some(cfg) = self.magnetism_active() else {
            return;
        };
        let Some(fb) = self.build_magnet_feedback(bounds) else {
            return;
        };
        match &cfg.feedback {
            Some(custom) => custom(canvas, ctx, &fb),
            None => render_default_feedback(canvas, ctx, &fb),
        }
    }

    /// The view's current geometric zoom (scale of the composed view
    /// transform), guarded to a positive finite value.
    pub(super) fn magnet_zoom(&self) -> f32 {
        let z = self.view_transform().geometric_scale();
        if z.is_finite() && z > 0.0 { z } else { 1.0 }
    }

    /// Build the per-frame [`MagnetFeedback`] for the feedback renderer,
    /// or `None` when there is nothing to draw (magnetism off, or
    /// markers are off and no interaction is forming a connector).
    pub(super) fn build_magnet_feedback(&self, bounds: Rect) -> Option<MagnetFeedback> {
        let cfg = self.magnetism_active()?;
        let active = self.magnet_interaction_active();
        let show_markers = match cfg.markers {
            crate::magnet::MarkerVisibility::Always => true,
            crate::magnet::MarkerVisibility::DuringInteraction => active,
            crate::magnet::MarkerVisibility::Never => false,
        };
        if !show_markers && !active {
            return None;
        }

        let zoom = self.magnet_zoom();
        let connect = self.magnet_connect_mode.get();
        let focus = self.magnet_focus.get();
        let pending = self.magnet_pending.get();
        let snap_to = self.item_snap.borrow().as_ref().map(|s| s.to);
        let port = self.port_drag.borrow().clone();

        let scene = self.scene();
        let mut markers: Vec<MagnetMarker> = Vec::new();
        if show_markers {
            let region = self.visible_scene_region(bounds);
            for item in scene.items_in_rect(region) {
                for mid in scene.magnet_ids_of(item) {
                    if !scene.magnet_enabled(mid) {
                        continue;
                    }
                    let Some(m) = scene.magnet(mid) else {
                        continue;
                    };
                    let mut state = MagnetVisualState::Idle;
                    if connect {
                        if Some(mid) == focus {
                            state = MagnetVisualState::Focused;
                        } else if Some(mid) == pending {
                            state = MagnetVisualState::PendingSource;
                        } else if let Some(p) = pending
                            && let Some(src) = scene.magnet(p)
                                && (cfg.predicate)(&src, &m).is_accept() {
                                    state = MagnetVisualState::Candidate;
                                }
                    }
                    if let Some(port) = &port {
                        if mid == port.source {
                            state = MagnetVisualState::PendingSource;
                        }
                        if port.snapped.as_ref().map(|(t, _, _)| *t) == Some(mid) {
                            state = MagnetVisualState::Snapped;
                        }
                    }
                    if Some(mid) == snap_to {
                        state = MagnetVisualState::Snapped;
                    }
                    markers.push(MagnetMarker {
                        id: mid,
                        scene_pos: m.scene_pos,
                        role: m.role,
                        state,
                    });
                }
            }
        }

        // Connector: port-drag wire, or keyboard preview. Item-drag needs
        // no connector — the snapped pair coincides, shown by the Snapped
        // marker.
        let connector = if let Some(port) = &port {
            let to = port
                .snapped
                .as_ref()
                .map(|(_, pos, _)| *pos)
                .unwrap_or(port.cursor_scene);
            Some((port.source_scene, to, port.snapped.is_some()))
        } else if connect {
            match (pending, focus) {
                (Some(p), Some(f)) if p != f => {
                    match (scene.magnet(p), scene.magnet(f)) {
                        (Some(src), Some(dst)) => {
                            let accepted = (cfg.predicate)(&src, &dst).is_accept();
                            Some((src.scene_pos, dst.scene_pos, accepted))
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        Some(MagnetFeedback {
            zoom,
            markers,
            connector,
        })
    }
}
