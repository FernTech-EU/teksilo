// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Gesture and input registration for [`SceneView`].
//!
//! This module implements the three `pub(super)` methods that attach all
//! interactive `HandlerSet` callbacks to the view: pointer events (hover
//! transitions, tooltip scheduling, cursor shape, tap dispatch),
//! scroll / pinch / keyboard navigation (pan, zoom-about-pointer, Ctrl+wheel,
//! arrow keys, `+`/`-`/`0` zoom shortcuts), and drag handling (item
//! drag-to-move, rubber-band marquee selection, scroll-hand-drag panning, and
//! port-drag wire initiation for the magnetism subsystem).  All handlers read
//! scene and view state through `Signal` captures; none hold `&mut` to
//! `SceneView` at call time.

use super::magnetism::{PortDragState, build_connection, handle_connect_key};
use super::*;

impl SceneView {
    pub(super) fn register_pointer_handlers(
        &self,
        mut handlers: HandlerSet,
        self_id: WidgetId,
        tooltip_content_id: WidgetId,
        tooltip_text: Signal<String>,
        tooltip_fade: Option<Duration>,
        tooltip_delay: Duration,
    ) -> HandlerSet {
        // Track the latest pointer position so Ctrl+wheel can
        // zoom-about-pointer (the scene point under the cursor
        // stays put). Updated even when not interactive — the
        // outer SceneView in a nested chart still benefits from
        // knowing where the mouse is.
        //
        // Also flips the system cursor to `Move` whenever the
        // pointer is over a draggable lightweight item, and back
        // to `Default` otherwise. The visual hint matches the
        // user's affordance check ("can I grab this?") without
        // forcing app-side wiring.
        // Track the latest pointer position so Ctrl+wheel can
        // zoom-about-pointer (the scene point under the cursor
        // stays put). Updated even when not interactive — the
        // outer SceneView in a nested chart still benefits from
        // knowing where the mouse is.
        //
        // Also flips the system cursor based on the standard
        // grab/grabbing convention:
        //   - Pointer over a draggable item, no active drag → `Grab`
        //     (open hand, "you can pick this up")
        //   - Active drag in progress                       → `Grabbing`
        //     (closed fist, "you are holding it")
        //   - Anywhere else                                 → `Default`
        // Hover detection uses the same draggable-bounds snapshot
        // the on_drag::Started path consults, so the cursor and
        // the hit-test agree on what's draggable.
        {
            let cursor_pos = self.cursor_pos.clone();
            let bounds_snapshot = self.lightweight_bounds_snapshot.clone();
            let handler_snapshot = self.handler_snapshot.clone();
            let view_xform_signal = self.view_transform_signal.clone();
            let drag_target_for_cursor = self.drag_target.clone();
            let hovered_item = self.hovered_item.clone();
            let pending_tap = self.pending_tap.clone();
            let tooltip_text = tooltip_text.clone();
            let tooltip_anchor_id = self_id;
            handlers = handlers.on_pointer_event(move |ev, ctx| {
                use bastyde_core::event::PointerButton;
                use bastyde_core::event::WidgetEvent as Ev;
                use bastyde_core::widget::CursorIcon;

                // Project a screen point to scene coords for
                // hit-testing. Returns `Point::ZERO` when the view
                // transform is degenerate.
                let to_scene = |p: Point| {
                    let xform = view_xform_signal.get();
                    xform
                        .inverse()
                        .map(|inv| inv.apply_point(p))
                        .unwrap_or(Point::ZERO)
                };

                // Hit-test the handler-snapshot for the topmost
                // item under the pointer. Snapshot is z-sorted desc.
                //
                // Normal items: broad-phase tests `scene_pt` against
                // `scene_rect`, narrow-phase inverse-projects to
                // local and calls `shape_contains`.
                //
                // IGNORES_TRANSFORMATIONS items: pin at a fixed
                // screen position with their natural local-pixel
                // size, so we project `scene_anchor` through the
                // CURRENT view transform (snapshot stores the
                // pan/zoom-invariant scene_anchor; the snapshot
                // doesn't rebuild on pan/zoom). Broad-phase tests
                // `screen_pt` against the projected screen rect;
                // narrow-phase passes `(screen_pt - screen_anchor)`
                // as the item-local point.
                let hit_handler_item =
                    |screen_pt: Point, scene_pt: Point| -> Option<HandlerSnapshotEntry> {
                        let snap = handler_snapshot.borrow();
                        let view_xform = view_xform_signal.get();
                        // Logical view zoom (uniform scale of the linear part) —
                        // passed to each item's shape-test so a cosmetic
                        // (device-pixel) stroke's clickable band is converted to
                        // scene coordinates at the current zoom.
                        let view_scale = view_xform.m[0].hypot(view_xform.m[1]);
                        for entry in snap.iter() {
                            if entry.ignores_xform {
                                let screen_anchor = view_xform.apply_point(entry.scene_anchor);
                                let screen_rect = Rect::new(
                                    screen_anchor.x + entry.local_bounds.x,
                                    screen_anchor.y + entry.local_bounds.y,
                                    entry.local_bounds.width,
                                    entry.local_bounds.height,
                                );
                                if !screen_rect.contains(screen_pt) {
                                    continue;
                                }
                                let local_pt = Point::new(
                                    screen_pt.x - screen_anchor.x,
                                    screen_pt.y - screen_anchor.y,
                                );
                                // Screen-anchored items ignore the view transform,
                                // so their hit-test runs at unit scale.
                                if (entry.shape_contains)(local_pt, 1.0) {
                                    return Some(entry.clone());
                                }
                                continue;
                            }
                            if !entry.scene_rect.contains(scene_pt) {
                                continue;
                            }
                            // Inverse-project to local for narrow-phase.
                            let local_pt = entry
                                .scene_transform
                                .inverse()
                                .map(|inv| inv.apply_point(scene_pt))
                                .unwrap_or(Point::ZERO);
                            if (entry.shape_contains)(local_pt, view_scale) {
                                return Some(entry.clone());
                            }
                        }
                        None
                    };

                match ev {
                    Ev::PointerMove { position, .. } => {
                        cursor_pos.set(Some(*position));
                        let scene_pt = to_scene(*position);

                        // Hover transitions: compare current hit
                        // with previously-hovered item; fire
                        // on_hover(false) on the old, on_hover(true)
                        // on the new.
                        let new_hit = hit_handler_item(*position, scene_pt);
                        let new_id = new_hit.as_ref().map(|e| e.id);
                        let prev_id = hovered_item.get();
                        if prev_id != new_id {
                            if let Some(prev) = prev_id
                                && let Some(prev_entry) =
                                    handler_snapshot.borrow().iter().find(|e| e.id == prev)
                                && let Some(h) = prev_entry.handlers.as_deref()
                                && let Some(cb) = h.on_hover.as_ref()
                            {
                                cb(false, ctx);
                            }
                            if let Some(new_entry) = new_hit.as_ref()
                                && let Some(h) = new_entry.handlers.as_deref()
                                && let Some(cb) = h.on_hover.as_ref()
                            {
                                cb(true, ctx);
                            }
                            hovered_item.set(new_id);

                            // Tooltip: retract the previous item's tooltip,
                            // then (re)schedule for the new item. We only
                            // dismiss the *shown* overlay here (active
                            // stack); `show_overlay_after` already replaces
                            // any stale *pending* show for the same content,
                            // so calling `cancel_delayed_overlay` as well
                            // would cancel the new show (drain order:
                            // delayed-requests apply before cancels).
                            ctx.dismiss_overlay_by_content(tooltip_content_id);
                            if let Some(ls) = new_hit
                                .as_ref()
                                .and_then(|e| e.handlers.as_deref())
                                .and_then(|h| h.tooltip.as_ref())
                            {
                                tooltip_text.set(ls.resolve_now());
                                ctx.show_overlay_after(
                                    bastyde_core::overlay::OverlayRequest {
                                        content_id: tooltip_content_id,
                                        anchor: tooltip_anchor_id,
                                        // Drop the tooltip just below-right
                                        // of the cursor so it doesn't sit
                                        // under the pointer.
                                        placement:
                                            bastyde_core::overlay::OverlayPlacement::AtPointer(
                                                Point::new(position.x + 12.0, position.y + 16.0),
                                            ),
                                        // Manual: every dismiss path (item
                                        // change, pointer-down, pointer-leave)
                                        // is driven explicitly below.
                                        dismiss: bastyde_core::overlay::DismissBehavior::Manual,
                                        layer: bastyde_core::overlay::OverlayLayer::InTree,
                                        parent_overlay: None,
                                        on_dismiss: None,
                                        fade_duration: tooltip_fade,
                                    },
                                    tooltip_delay,
                                );
                            } else {
                                // Moved onto an item with no tooltip (or onto
                                // empty space): cancel any pending show.
                                ctx.cancel_delayed_overlay(tooltip_content_id);
                            }
                        }

                        // Cursor: per-item override → grab/grabbing
                        // for draggable items → default.
                        let item_cursor = new_hit
                            .as_ref()
                            .and_then(|e| e.handlers.as_deref())
                            .and_then(|h| h.cursor);
                        // Narrow-phase: the grab cursor only shows when the
                        // pointer is over the item's actual shape, agreeing
                        // with the on_drag drag-start hit-test below.
                        let over_draggable = {
                            let snap = bounds_snapshot.borrow();
                            super::hit_draggable_item(
                                &snap,
                                *position,
                                scene_pt,
                                view_xform_signal.get(),
                            )
                            .is_some()
                        };
                        let cursor = if drag_target_for_cursor.get().is_some() {
                            CursorIcon::Grabbing
                        } else if let Some(c) = item_cursor {
                            c
                        } else if over_draggable {
                            CursorIcon::Grab
                        } else {
                            CursorIcon::Default
                        };
                        ctx.set_cursor(cursor);
                    }
                    Ev::PointerDown {
                        position,
                        button,
                        modifiers,
                    } => {
                        cursor_pos.set(Some(*position));
                        // Any press retracts a hover tooltip (shown or
                        // pending) — the user has committed to an action.
                        ctx.cancel_delayed_overlay(tooltip_content_id);
                        ctx.dismiss_overlay_by_content(tooltip_content_id);
                        let scene_pt = to_scene(*position);
                        let hit = hit_handler_item(*position, scene_pt);
                        match button {
                            PointerButton::Secondary => {
                                if let Some(entry) = hit.as_ref()
                                    && let Some(h) = entry.handlers.as_deref()
                                    && let Some(cb) = h.on_context_menu.as_ref()
                                {
                                    let ev = crate::item_handlers::SceneTapEvent::new(
                                        scene_pt, *button, *modifiers,
                                    );
                                    cb(&ev, ctx);
                                    return EventResponse::Handled;
                                }
                            }
                            _ => {
                                // Gate on the item's accept_tap_buttons
                                // mask. PRIMARY is the default; items
                                // wanting middle-click-as-tap opt in
                                // via `accept_tap_buttons(...)`.
                                if let Some(entry) = hit.as_ref() {
                                    let accept = entry
                                        .handlers
                                        .as_deref()
                                        .map(|h| h.accept_tap_buttons)
                                        .unwrap_or(bastyde_core::event::ButtonMask::PRIMARY);
                                    if accept.contains(*button) {
                                        pending_tap.set(Some((scene_pt, entry.id, *button)));
                                    } else {
                                        pending_tap.set(None);
                                    }
                                } else {
                                    pending_tap.set(None);
                                }
                            }
                        }
                    }
                    Ev::PointerUp {
                        position,
                        button,
                        modifiers,
                    } => {
                        // Tap dispatch only fires when the button that
                        // came back up matches the one we recorded on
                        // the press. Mixed-button down/up sequences
                        // discard the pending tap.
                        if let Some((press_scene, item_id, press_button)) = pending_tap.take()
                            && press_button == *button
                        {
                            let scene_pt = to_scene(*position);
                            let dx = scene_pt.x - press_scene.x;
                            let dy = scene_pt.y - press_scene.y;
                            if (dx * dx + dy * dy).sqrt() <= TAP_MOVEMENT_THRESHOLD {
                                // Genuine tap — dispatch if the
                                // pressed item still has a tap
                                // handler installed.
                                if let Some(entry) =
                                    handler_snapshot.borrow().iter().find(|e| e.id == item_id)
                                    && let Some(h) = entry.handlers.as_deref()
                                    && let Some(cb) = h.on_tap.as_ref()
                                {
                                    let ev = crate::item_handlers::SceneTapEvent::new(
                                        scene_pt, *button, *modifiers,
                                    );
                                    cb(&ev, ctx);
                                    return EventResponse::Handled;
                                }
                            }
                        }
                    }
                    Ev::PointerLeave => {
                        cursor_pos.set(None);
                        // Pointer left the view entirely — retract the
                        // tooltip (shown or pending).
                        ctx.cancel_delayed_overlay(tooltip_content_id);
                        ctx.dismiss_overlay_by_content(tooltip_content_id);
                        // Clear any pending hover.
                        if let Some(prev) = hovered_item.take()
                            && let Some(prev_entry) =
                                handler_snapshot.borrow().iter().find(|e| e.id == prev)
                            && let Some(h) = prev_entry.handlers.as_deref()
                            && let Some(cb) = h.on_hover.as_ref()
                        {
                            cb(false, ctx);
                        }
                        pending_tap.set(None);
                        ctx.set_cursor(CursorIcon::Default);
                    }
                    _ => {}
                }
                EventResponse::Ignored
            });
        }
        handlers
    }

    pub(super) fn register_scroll_pinch_key_handlers(
        &self,
        mut handlers: HandlerSet,
        line_height: f32,
        pan_dur: Duration,
        overscroll: OverscrollBehavior,
        prefers_reduced: bool,
    ) -> HandlerSet {
        {
            let pan_x = self.pan_x.clone();
            let pan_y = self.pan_y.clone();
            let zoom = self.zoom.clone();
            let rotation = self.rotation.clone();
            let bounds_origin_for_scroll = self.bounds_origin_signal.clone();
            let last_viewport_for_scroll = self.last_viewport.clone();
            let cursor_pos_for_scroll = self.cursor_pos.clone();
            let pan_axes_sig = self.scene().pan_axes_signal();
            let zoomable_sig = self.scene().zoomable_signal();
            let scene_zoom_range_sig = self.scene().zoom_range_signal();
            let view_zoom_range_sig = self.zoom_range_override.clone();
            let scene_pan_bounds_sig = self.scene().pan_bounds_signal();
            let view_pan_bounds_sig = self.pan_bounds_override.clone();
            let adopt_scene_size = self.adopt_scene_size;
            handlers = handlers.on_scroll(move |event, _ctx| {
                use crate::scene::PanAxes;
                let WidgetEvent::Scroll { delta, modifiers } = event else {
                    return EventResponse::Ignored;
                };
                let (mut dx, mut dy) = match delta {
                    ScrollDelta::Pixels { x, y } => (*x, *y),
                    ScrollDelta::Lines { x, y } => (*x * line_height, *y * line_height),
                };
                // Apply the scene's pan-axes policy live: zero out
                // the restricted axis so it passes through to ancestor
                // scrollables instead of being absorbed.
                match pan_axes_sig.get() {
                    PanAxes::Both => {}
                    PanAxes::None => {
                        dx = 0.0;
                        dy = 0.0;
                    }
                    PanAxes::Horizontal => {
                        dy = 0.0;
                    }
                    PanAxes::Vertical => {
                        dx = 0.0;
                    }
                }
                let zoomable = zoomable_sig.get() && !adopt_scene_size;
                // Ctrl+wheel = zoom about the viewport center.
                // Unmodified wheel / trackpad pan = pan the view.
                if modifiers.ctrl() {
                    if !zoomable {
                        return EventResponse::Ignored;
                    }
                    // Zoom magnitude scales with vertical scroll
                    // distance. Sign convention: scroll up (negative
                    // ScrollDelta after platform negation) → zoom in.
                    // Pixels deltas are large; rescale so the
                    // step size matches one wheel notch.
                    let step_px = match delta {
                        ScrollDelta::Pixels { y, .. } => *y / 60.0,
                        ScrollDelta::Lines { y, .. } => *y,
                    };
                    if step_px == 0.0 {
                        return EventResponse::Handled;
                    }
                    // Compute multiplicative factor: each notch = 1.1×
                    // (or 1/1.1 for zoom-out). Using exp-form keeps
                    // repeated notches consistent.
                    let factor = (-step_px * 0.1).exp();
                    let z_old = zoom.get();
                    let r_now = rotation.get();
                    let scene_range = scene_zoom_range_sig.get();
                    let view_range = view_zoom_range_sig.get();
                    let effective_zoom =
                        intersect_zoom_range(scene_range.as_ref(), view_range.as_ref());
                    let z_new = clamp_zoom(z_old * factor, effective_zoom.as_ref());
                    if (z_new - z_old).abs() < 1e-6 {
                        return EventResponse::Handled;
                    }
                    let viewport_size = last_viewport_for_scroll.get();
                    let bo = bounds_origin_for_scroll.get();
                    // Anchor the zoom at the cursor when known
                    // (zoom-about-pointer — the scene point under
                    // the mouse stays put). Fall back to viewport
                    // center if no cursor position has been seen.
                    let anchor_screen = match cursor_pos_for_scroll.get() {
                        Some(p) => p,
                        None => bastyde_canvas::Point::new(
                            bo.x + viewport_size.width * 0.5,
                            bo.y + viewport_size.height * 0.5,
                        ),
                    };
                    let pan_old = Vec2::new(pan_x.get(), pan_y.get());
                    let new_pan = anchor_pan_for_pinch(
                        anchor_screen,
                        pan_old,
                        z_old,
                        r_now,
                        z_new,
                        r_now,
                        bo,
                    )
                    .unwrap_or(pan_old);
                    // Clamp the zoom-induced pan adjustment against
                    // the effective pan_bounds so wheel-zoom over
                    // a doc-style bounded scene doesn't push the
                    // viewport off the document. Clamped against the
                    // *new* zoom (not yet committed).
                    let new_pan = clamp_pan(
                        new_pan,
                        scene_pan_bounds_sig.get(),
                        view_pan_bounds_sig.get(),
                        viewport_size,
                        z_new,
                    );
                    // Snap zoom + pan together. Animating the two
                    // signals independently with EaseOut would drift
                    // mid-tween (the anchor math is exact only at
                    // start and end states). Snap is also the
                    // standard wheel-zoom feel — each notch produces
                    // an immediate, predictable step. The pinch
                    // path uses the same snap rule.
                    zoom.set(z_new);
                    pan_x.set(new_pan.x);
                    pan_y.set(new_pan.y);
                    return EventResponse::Handled;
                }
                // No-op / pass-through when both axes are zeroed by
                // the policy.
                if dx == 0.0 && dy == 0.0 {
                    return EventResponse::Ignored;
                }
                // Convention: positive scroll delta on the y-axis
                // means content scrolls "up" in the viewport, which
                // is equivalent to panning the *view* down — i.e. the
                // pan offset increases. This matches `ScrollArea` and
                // the natural-scroll feel of trackpads.
                let base_x = pan_x.animation_target().unwrap_or_else(|| pan_x.get());
                let base_y = pan_y.animation_target().unwrap_or_else(|| pan_y.get());
                // Clamp the projected pan against effective bounds.
                // Axes already applied by zeroing dx/dy above.
                let clamped = clamp_pan(
                    Vec2::new(base_x + dx, base_y + dy),
                    scene_pan_bounds_sig.get(),
                    view_pan_bounds_sig.get(),
                    last_viewport_for_scroll.get(),
                    zoom.get(),
                );
                // Boundary-based scroll chaining: if the pan can't move on
                // either axis (already clamped at a bound), decline so the
                // event bubbles to an ancestor scrollable. Mirrors the
                // ScrollArea / ListView / TreeView / TableView behavior.
                // `OverscrollBehavior::Contain` opts out — the scene keeps
                // the wheel even at its bound (no chaining).
                let moved_x =
                    (clamped.x - base_x).abs() > bastyde_core::overscroll::SCROLL_MOVE_EPSILON;
                let moved_y =
                    (clamped.y - base_y).abs() > bastyde_core::overscroll::SCROLL_MOVE_EPSILON;
                // Only the *total* boundary (neither axis can move) is a
                // chain/contain decision point. If one axis still moves the
                // event is consumed below (Handled) regardless of
                // `overscroll` — the partial-absorb / drop-the-pinned-axis
                // tradeoff, matching the widget scrollables and the browser.
                if !moved_x && !moved_y {
                    return match overscroll {
                        OverscrollBehavior::Contain => EventResponse::Handled,
                        OverscrollBehavior::Chain => EventResponse::Ignored,
                    };
                }
                if prefers_reduced {
                    pan_x.set(clamped.x);
                    pan_y.set(clamped.y);
                } else {
                    pan_x.animate_to(clamped.x, pan_dur, Easing::EaseOut);
                    pan_y.animate_to(clamped.y, pan_dur, Easing::EaseOut);
                }
                EventResponse::Handled
            });
        }

        {
            let pan_x = self.pan_x.clone();
            let pan_y = self.pan_y.clone();
            let zoom = self.zoom.clone();
            let rotation = self.rotation.clone();
            let bounds_origin_for_pinch = self.bounds_origin_signal.clone();
            let last_viewport_for_pinch = self.last_viewport.clone();
            let zoomable_sig_pinch = self.scene().zoomable_signal();
            let pan_axes_sig_pinch = self.scene().pan_axes_signal();
            let scene_zoom_range_sig_pinch = self.scene().zoom_range_signal();
            let view_zoom_range_sig_pinch = self.zoom_range_override.clone();
            let scene_pan_bounds_sig_pinch = self.scene().pan_bounds_signal();
            let view_pan_bounds_sig_pinch = self.pan_bounds_override.clone();
            let adopt_scene_size_pinch = self.adopt_scene_size;
            handlers = handlers.on_pinch(move |phase, _ctx| {
                if !zoomable_sig_pinch.get() || adopt_scene_size_pinch {
                    return;
                }
                let PinchPhase::Changed {
                    center,
                    scale,
                    rotation: rotation_delta,
                } = phase
                else {
                    return;
                };
                if !scale.is_finite() || scale <= 0.0 {
                    return;
                }
                let z_old = zoom.get();
                let r_old = rotation.get();
                let scene_range = scene_zoom_range_sig_pinch.get();
                let view_range = view_zoom_range_sig_pinch.get();
                let effective_zoom =
                    intersect_zoom_range(scene_range.as_ref(), view_range.as_ref());
                let z_new = clamp_zoom(z_old * scale, effective_zoom.as_ref());
                let r_new = r_old + rotation_delta;
                let pan_old = Vec2::new(pan_x.get(), pan_y.get());
                let bo = bounds_origin_for_pinch.get();
                let new_pan = anchor_pan_for_pinch(center, pan_old, z_old, r_old, z_new, r_new, bo)
                    .unwrap_or(pan_old);
                // Pinch is a continuous, user-driven gesture — set
                // directly so each frame's update lands without
                // queuing a tween. Idle gates still apply: at rest
                // (pinch released, no further events), no frames are
                // requested.
                zoom.set(z_new);
                rotation.set(r_new);
                // Apply pan-axes policy live (orthogonal axis held at
                // the pre-pinch pan), then clamp to effective pan_bounds
                // against the new zoom.
                let new_pan = apply_pan_axes(new_pan, pan_old, pan_axes_sig_pinch.get());
                let new_pan = clamp_pan(
                    new_pan,
                    scene_pan_bounds_sig_pinch.get(),
                    view_pan_bounds_sig_pinch.get(),
                    last_viewport_for_pinch.get(),
                    z_new,
                );
                pan_x.set(new_pan.x);
                pan_y.set(new_pan.y);
            });
        }

        // --- Keyboard navigation -------------------------------
        //
        // Default scheme:
        // - Arrow keys: pan by ~one viewport-quarter per press. Released
        //   here for now; held-key repeat naturally chains tweens via
        //   `animate_to`. Apps that wire `focus_order(...)`
        //   can override the arrow path by handling them upstream.
        // - `+` / `=`: zoom in by 1.25× about the viewport center.
        // - `-`: zoom out by 0.8× about the viewport center.
        // - `0`: reset zoom to 1.0 about the viewport center.
        //
        // Handler is `on_key` (focused-widget surface) — it only
        // fires when the SceneView itself is the focus target, NOT
        // when a heavyweight child (like a TextInput) has focus and
        // the user is typing. This is the right default: typing
        // letters into a card shouldn't pan the scene. Apps that
        // want global pan/zoom shortcuts should register them
        // through the `Shortcut`/`Action` pipeline so they work
        // regardless of focus.
        {
            use bastyde_core::event::{EventResponse, Key, WidgetEvent};
            let pan_x = self.pan_x.clone();
            let pan_y = self.pan_y.clone();
            let zoom = self.zoom.clone();
            let pan_dur = self.pan_anim_duration;
            let zoom_dur = self.zoom_anim_duration;
            let viewport_size = self.last_viewport.clone();
            let pan_x_for_xform = self.pan_x.clone();
            let pan_y_for_xform = self.pan_y.clone();
            let zoom_for_xform = self.zoom.clone();
            let rotation_for_xform = self.rotation.clone();
            let bounds_origin_for_xform = self.bounds_origin_signal.clone();
            let pan_axes_sig_keys = self.scene().pan_axes_signal();
            let zoomable_sig_keys = self.scene().zoomable_signal();
            let scene_zoom_range_sig_keys = self.scene().zoom_range_signal();
            let view_zoom_range_sig_keys = self.zoom_range_override.clone();
            let scene_pan_bounds_sig_keys = self.scene().pan_bounds_signal();
            let view_pan_bounds_sig_keys = self.pan_bounds_override.clone();
            let adopt_scene_size_keys = self.adopt_scene_size;
            // Magnetism keyboard-connect captures.
            let magnetism_for_keys = self.magnetism.clone();
            let model_for_keys = self.model.clone();
            let connect_mode_keys = self.magnet_connect_mode.clone();
            let magnet_focus_keys = self.magnet_focus.clone();
            let magnet_pending_keys = self.magnet_pending.clone();
            let self_id_for_keys = self.self_widget_id.get();
            handlers = handlers.on_key(move |event, ctx| {
                use crate::scene::PanAxes;
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                // Magnetism keyboard connect flow takes priority over
                // pan/zoom: it claims the connect key from any state, and
                // arrows / Enter / Esc while connect mode is active.
                if let Some(cfg) = magnetism_for_keys.as_ref().filter(|c| c.enabled.get())
                    && handle_connect_key(
                        key,
                        cfg,
                        &model_for_keys,
                        &connect_mode_keys,
                        &magnet_focus_keys,
                        &magnet_pending_keys,
                        self_id_for_keys,
                        ctx,
                    )
                {
                    return EventResponse::Handled;
                }
                let pan_axes_keys = pan_axes_sig_keys.get();
                let zoomable_keys = zoomable_sig_keys.get() && !adopt_scene_size_keys;
                let allow_pan_x = matches!(pan_axes_keys, PanAxes::Both | PanAxes::Horizontal);
                let allow_pan_y = matches!(pan_axes_keys, PanAxes::Both | PanAxes::Vertical);
                let clamp_to_zoom = |z: f32| -> f32 {
                    let scene_range = scene_zoom_range_sig_keys.get();
                    let view_range = view_zoom_range_sig_keys.get();
                    let effective = intersect_zoom_range(scene_range.as_ref(), view_range.as_ref());
                    clamp_zoom(z, effective.as_ref())
                };
                let clamp_to_pan = |p: Vec2, z: f32| -> Vec2 {
                    clamp_pan(
                        p,
                        scene_pan_bounds_sig_keys.get(),
                        view_pan_bounds_sig_keys.get(),
                        viewport_size.get(),
                        z,
                    )
                };
                // Pan step = quarter of the smaller viewport axis,
                // capped to a sensible minimum so unusually small
                // viewports still feel responsive.
                let vp = viewport_size.get();
                let pan_step = (vp.width.min(vp.height) * 0.25).max(64.0);
                let mut handled = true;
                let recenter_zoom = |z_new: f32| {
                    // Adjust pan so the viewport center stays fixed
                    // when zoom changes. Same anchor logic as pinch
                    // about viewport center, but always centered.
                    let bo = bounds_origin_for_xform.get();
                    let viewport = vp;
                    let anchor_screen = bastyde_canvas::Point::new(
                        bo.x + viewport.width * 0.5,
                        bo.y + viewport.height * 0.5,
                    );
                    let z_old = zoom_for_xform.get();
                    let r = rotation_for_xform.get();
                    let pan_old = Vec2::new(pan_x_for_xform.get(), pan_y_for_xform.get());
                    if let Some(new_pan) =
                        anchor_pan_for_pinch(anchor_screen, pan_old, z_old, r, z_new, r, bo)
                    {
                        pan_x_for_xform.animate_to(new_pan.x, pan_dur, Easing::EaseOut);
                        pan_y_for_xform.animate_to(new_pan.y, pan_dur, Easing::EaseOut);
                    }
                };
                // Arrow-key pan helper: take the current animation
                // target (or live value if no tween in flight),
                // shift by step on the requested axis, clamp the
                // resulting pan vector against the effective
                // pan_bounds, then animate to the clamped target.
                let pan_axis = |dx: f32, dy: f32| {
                    let base_x = pan_x.animation_target().unwrap_or_else(|| pan_x.get());
                    let base_y = pan_y.animation_target().unwrap_or_else(|| pan_y.get());
                    let target = clamp_to_pan(Vec2::new(base_x + dx, base_y + dy), zoom.get());
                    if dx != 0.0 {
                        pan_x.animate_to(target.x, pan_dur, Easing::EaseOut);
                    }
                    if dy != 0.0 {
                        pan_y.animate_to(target.y, pan_dur, Easing::EaseOut);
                    }
                };
                match key {
                    Key::ArrowLeft if allow_pan_x => pan_axis(pan_step, 0.0),
                    Key::ArrowRight if allow_pan_x => pan_axis(-pan_step, 0.0),
                    Key::ArrowUp if allow_pan_y => pan_axis(0.0, pan_step),
                    Key::ArrowDown if allow_pan_y => pan_axis(0.0, -pan_step),
                    other
                        if zoomable_keys
                            && (other.to_char() == Some('+') || other.to_char() == Some('=')) =>
                    {
                        let z_new = clamp_to_zoom(zoom.get() * 1.25);
                        zoom.animate_to(z_new, zoom_dur, Easing::EaseOut);
                        recenter_zoom(z_new);
                    }
                    other if zoomable_keys && other.to_char() == Some('-') => {
                        let z_new = clamp_to_zoom(zoom.get() * 0.8);
                        zoom.animate_to(z_new, zoom_dur, Easing::EaseOut);
                        recenter_zoom(z_new);
                    }
                    other if other.to_char() == Some('0') => {
                        zoom.animate_to(1.0, zoom_dur, Easing::EaseOut);
                        recenter_zoom(1.0);
                    }
                    _ => handled = false,
                }
                if handled {
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
            // SceneView itself is focusable so it can receive these
            // key events. Heavyweight children grab focus first when
            // they're the click target — typing in a card stays in
            // the card.
            handlers = handlers.focusable(true);
        }
        handlers
    }

    pub(super) fn register_drag_handlers(&self, mut handlers: HandlerSet) -> HandlerSet {
        let marquee = self.marquee.clone();
        let pending_marquee_commit = self.pending_marquee_commit.clone();
        let drag_target = self.drag_target.clone();
        let pending_item_move = self.pending_item_move.clone();
        let reconcile_dirty = self.reconcile_dirty.clone();
        let view_xform_signal = self.view_transform_signal.clone();
        let bounds_snapshot = self.lightweight_bounds_snapshot.clone();
        let pan_x_for_drag = self.pan_x.clone();
        let pan_y_for_drag = self.pan_y.clone();
        let drag_mode_sig = self.drag_mode.clone();
        // Live signal captures — runtime mutations to pan_axes /
        // pan_bounds take effect on the next drag event.
        let pan_axes_sig_drag = self.scene().pan_axes_signal();
        let scene_pan_bounds_sig_drag = self.scene().pan_bounds_signal();
        let view_pan_bounds_sig_drag = self.pan_bounds_override.clone();
        let zoom_for_drag = self.zoom.clone();
        let last_viewport_for_drag = self.last_viewport.clone();
        // Magnetism captures. The model handle lets the closure run the
        // snap helpers; `port_drag` / `item_snap` carry the in-flight
        // interaction state; `magnetism_for_drag` is the (optional)
        // config read live so a toolbar toggle takes effect next event.
        let model_for_drag = self.model.clone();
        let magnetism_for_drag = self.magnetism.clone();
        let port_drag = self.port_drag.clone();
        let item_snap = self.item_snap.clone();
        handlers = handlers.on_drag(move |phase, ctx| {
            // Read drag mode live so a toolbar can flip
            // between Select / Hand / NoDrag at runtime.
            let drag_mode_inner = drag_mode_sig.get();
            if drag_mode_inner == crate::item_handlers::DragMode::NoDrag {
                return;
            }
            // ScrollHandDrag mode bypasses item / marquee logic
            // entirely — drag pans the view by the cursor
            // delta in scene coords. Marquee and drag-to-move
            // are inactive in this mode.
            if drag_mode_inner == crate::item_handlers::DragMode::ScrollHandDrag {
                use bastyde_core::gesture::DragPhase;
                if let DragPhase::Moved { delta, .. } = phase {
                    // `delta` is in screen coords. Apply the scene's
                    // pan-axes policy live (orthogonal axis held at the
                    // current pan) so an axis-locked scene can't be
                    // hand-dragged off-axis. Sign convention matches
                    // scroll (drag right → pan right).
                    let pan_old = Vec2::new(pan_x_for_drag.get(), pan_y_for_drag.get());
                    let candidate = apply_pan_axes(
                        Vec2::new(pan_old.x + delta.x, pan_old.y + delta.y),
                        pan_old,
                        pan_axes_sig_drag.get(),
                    );
                    // Nothing moved on a permitted axis — let the event
                    // bubble to ancestor scrollables.
                    if candidate == pan_old {
                        return;
                    }
                    // Clamp to effective pan_bounds (intersection of
                    // Scene + view-override) so the user can't drag
                    // the document off the viewport.
                    let target = clamp_pan(
                        candidate,
                        scene_pan_bounds_sig_drag.get(),
                        view_pan_bounds_sig_drag.get(),
                        last_viewport_for_drag.get(),
                        zoom_for_drag.get(),
                    );
                    pan_x_for_drag.set(target.x);
                    pan_y_for_drag.set(target.y);
                }
                return;
            }
            use bastyde_core::gesture::DragPhase;
            match phase {
                DragPhase::Started { position, button } => {
                    if !matches!(button, bastyde_core::event::PointerButton::Primary) {
                        return;
                    }
                    // Project screen press to scene coords for
                    // hit-test against the snapshot.
                    let xform = view_xform_signal.get();
                    let scene_press = match xform.inverse() {
                        Some(inv) => inv.apply_point(position),
                        None => Point::ZERO,
                    };
                    // Magnetism: a press on a magnet handle starts a
                    // port-drag (a transient wire), taking priority over
                    // item-drag and marquee. The grab disc is a fixed
                    // screen-pixel radius, converted to scene units by the
                    // live zoom so handles stay grabbable at any zoom. Only
                    // fires for presses the SceneView itself receives
                    // (lightweight / empty regions); a handle drawn over a
                    // heavyweight widget is consumed by that widget.
                    if let Some(cfg) = magnetism_for_drag.as_ref().filter(|c| c.enabled.get()) {
                        let zoom = xform.geometric_scale().max(1e-3);
                        let grab = cfg.capture_px / zoom;
                        if let Some(mid) = model_for_drag.nearest_magnet(scene_press, grab)
                            && let Some(src) = model_for_drag.magnet_scene_pos(mid)
                        {
                            port_drag.replace(Some(PortDragState {
                                source: mid,
                                source_scene: src,
                                cursor_scene: scene_press,
                                snapped: None,
                            }));
                            return;
                        }
                    }
                    // Narrow-phase hit-test: target the topmost draggable
                    // item whose actual SHAPE (not just its AABB) contains the
                    // press, so a thin draggable item (e.g. a connector path)
                    // is grabbed only on its stroke. The snapshot is z-sorted
                    // and refreshed each layout pass — see `place_children`.
                    let hit = {
                        let snap = bounds_snapshot.borrow();
                        super::hit_draggable_item(&snap, position, scene_press, xform)
                    };
                    if let Some(item_id) = hit {
                        // Drag-to-move: enter that mode,
                        // not marquee.
                        drag_target.set(Some(DragTarget {
                            item_id,
                            anchor_scene: scene_press,
                            current_scene: scene_press,
                        }));
                    } else {
                        // Empty area — start a marquee.
                        marquee.set(Some(MarqueeState {
                            origin: position,
                            current: position,
                            additive: false,
                        }));
                    }
                }
                DragPhase::Moved { position, .. } => {
                    // Port-drag takes priority: update the wire's free end
                    // and re-evaluate the snapped target.
                    if port_drag.borrow().is_some() {
                        let xform = view_xform_signal.get();
                        if let Some(inv) = xform.inverse() {
                            let cursor = inv.apply_point(position);
                            let mut pd = port_drag.borrow().clone().unwrap();
                            pd.cursor_scene = cursor;
                            pd.snapped = None;
                            if let Some(cfg) =
                                magnetism_for_drag.as_ref().filter(|c| c.enabled.get())
                            {
                                let zoom = xform.geometric_scale().max(1e-3);
                                let radius = cfg.capture_px / zoom;
                                if let Some((target, payload)) = model_for_drag.compute_port_snap(
                                    pd.source,
                                    cursor,
                                    radius,
                                    &*cfg.predicate,
                                ) {
                                    pd.snapped = Some((target.id, target.scene_pos, payload));
                                }
                            }
                            port_drag.replace(Some(pd));
                        }
                        return;
                    }
                    if let Some(mut target) = drag_target.get() {
                        // Update current scene-coord position
                        // for live paint feedback (the paint
                        // method will pick this up).
                        let xform = view_xform_signal.get();
                        if let Some(inv) = xform.inverse() {
                            target.current_scene = inv.apply_point(position);
                            // Magnetism: snap the dragged item so its
                            // closest accepting magnet aligns onto a
                            // target. The snap vector adjusts the visual
                            // position (and hence the committed delta).
                            if let Some(cfg) =
                                magnetism_for_drag.as_ref().filter(|c| c.enabled.get())
                            {
                                let delta = Vec2::new(
                                    target.current_scene.x - target.anchor_scene.x,
                                    target.current_scene.y - target.anchor_scene.y,
                                );
                                let zoom = xform.geometric_scale().max(1e-3);
                                let radius = cfg.capture_px / zoom;
                                match model_for_drag.compute_item_snap(
                                    target.item_id,
                                    delta,
                                    radius,
                                    &*cfg.predicate,
                                ) {
                                    Some(snap) => {
                                        target.current_scene = Point::new(
                                            target.current_scene.x + snap.snap_vector.x,
                                            target.current_scene.y + snap.snap_vector.y,
                                        );
                                        item_snap.replace(Some(snap));
                                    }
                                    None => {
                                        item_snap.replace(None);
                                    }
                                }
                            }
                            drag_target.set(Some(target));
                        }
                    } else if let Some(mut state) = marquee.get() {
                        state.current = position;
                        marquee.set(Some(state));
                    }
                }
                DragPhase::Ended { position } => {
                    // Port-drag release: fire the connection if the wire
                    // snapped onto an accepting target. No item moves.
                    if port_drag.borrow().is_some() {
                        let pd = port_drag.replace(None).unwrap();
                        let xform = view_xform_signal.get();
                        let cursor = xform
                            .inverse()
                            .map(|inv| inv.apply_point(position))
                            .unwrap_or(pd.cursor_scene);
                        if let Some(cfg) = magnetism_for_drag.as_ref().filter(|c| c.enabled.get()) {
                            let zoom = xform.geometric_scale().max(1e-3);
                            let radius = cfg.capture_px / zoom;
                            if let Some((target, payload)) = model_for_drag.compute_port_snap(
                                pd.source,
                                cursor,
                                radius,
                                &*cfg.predicate,
                            ) && let Some(conn) =
                                build_connection(&model_for_drag, pd.source, target.id, payload)
                            {
                                (cfg.on_connect)(&conn, ctx);
                            }
                        }
                        return;
                    }
                    if let Some(mut target) = drag_target.get() {
                        // Drag-to-move commit: compute the
                        // delta (current − anchor) in scene
                        // coords and post (id, delta) so the
                        // drain code can apply the same delta
                        // to every descendant.
                        let xform = view_xform_signal.get();
                        if let Some(inv) = xform.inverse() {
                            target.current_scene = inv.apply_point(position);
                        }
                        // Magnetism: re-evaluate the snap at the release
                        // position (the last Moved's snapped value was
                        // overwritten by the raw projection above), apply
                        // it, and fire the connection. The snapped
                        // current_scene yields a snapped commit delta.
                        if let Some(cfg) = magnetism_for_drag.as_ref().filter(|c| c.enabled.get()) {
                            let delta = Vec2::new(
                                target.current_scene.x - target.anchor_scene.x,
                                target.current_scene.y - target.anchor_scene.y,
                            );
                            let zoom = view_xform_signal.get().geometric_scale().max(1e-3);
                            let radius = cfg.capture_px / zoom;
                            if let Some(snap) = model_for_drag.compute_item_snap(
                                target.item_id,
                                delta,
                                radius,
                                &*cfg.predicate,
                            ) {
                                target.current_scene = Point::new(
                                    target.current_scene.x + snap.snap_vector.x,
                                    target.current_scene.y + snap.snap_vector.y,
                                );
                                if let Some(conn) = build_connection(
                                    &model_for_drag,
                                    snap.from,
                                    snap.to,
                                    snap.payload,
                                ) {
                                    (cfg.on_connect)(&conn, ctx);
                                }
                            }
                        }
                        item_snap.replace(None);
                        let delta = Vec2::new(
                            target.current_scene.x - target.anchor_scene.x,
                            target.current_scene.y - target.anchor_scene.y,
                        );
                        // Keep `drag_target` set with the final
                        // current_scene so `paint` continues to
                        // translate the item to the dragged
                        // position. The rebuild that drains
                        // `pending_item_move` will clear
                        // `drag_target` once the move has
                        // actually been applied to the scene —
                        // until then, clearing here would let
                        // one or more frames paint at the
                        // ORIGINAL (pre-drag) bounds and the
                        // item appears to "snap back" before
                        // the rebuild lands. Update the saved
                        // current_scene so the visual delta
                        // stays right.
                        drag_target.set(Some(target));
                        pending_item_move.set(Some((target.item_id, delta)));
                        // Bump the rebuild signal so SceneView's
                        // `build()` runs and drains the pending
                        // move (where `&mut self.scene` is
                        // available and `Scene::set_local_pos` can
                        // commit + re-bucket the spatial index).
                        reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
                        return;
                    }
                    // Marquee commit path. Same drain-via-rebuild
                    // pattern as drag-to-move: post the pending
                    // commit, bump `reconcile_dirty` so `build()` runs
                    // and drains it (which also clears the
                    // marquee Cell so the visual lasso disappears
                    // after release).
                    let Some(mut state) = marquee.get() else {
                        return;
                    };
                    state.current = position;
                    let screen_rect = state.rect();
                    let xform = view_xform_signal.get();
                    let scene_rect = match xform.inverse() {
                        Some(inv) => inv.apply_rect(screen_rect),
                        None => Rect::ZERO,
                    };
                    pending_marquee_commit.set(Some((scene_rect, state.additive)));
                    reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
                }
            }
        });
        handlers
    }
}
