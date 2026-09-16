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

use teksilo_core::event::ScrollMotion;
use teksilo_core::pointer::ScrollPhase;

use super::magnetism::{PortDragState, build_connection, handle_connect_key};
use super::*;

/// Everything the pointer handlers need to drive the view's item tooltip.
///
/// Grouped rather than passed as five parallel parameters: they are one
/// concern, and threading them individually put `register_pointer_handlers`
/// over the argument-count limit the moment the deferred-build gate
/// (`shown`) joined them.
pub(super) struct TooltipWiring {
    /// The (deferred) tooltip body.
    pub content_id: WidgetId,
    /// The text to show, resolved at show time so it stays locale-correct.
    pub text: Signal<String>,
    /// Flipped true the first time a tooltip is shown, which is what builds
    /// `content_id`'s subtree.
    pub shown: Signal<bool>,
    pub fade: Option<Duration>,
    pub delay: Duration,
}

impl SceneView {
    pub(super) fn register_pointer_handlers(
        &self,
        mut handlers: HandlerSet,
        self_id: WidgetId,
        tooltip: TooltipWiring,
        input_tokens: teksilo_tokens::InputTokens,
    ) -> HandlerSet {
        let TooltipWiring {
            content_id: tooltip_content_id,
            text: tooltip_text,
            shown: tooltip_shown,
            fade: tooltip_fade,
            delay: tooltip_delay,
        } = tooltip;
        // Where the contact that is currently holding went down, in view pixels.
        // The hold route below arms a delayed tooltip at the press and disarms it
        // the moment the contact travels far enough to be a pan or a drag; the
        // press point is what "far enough" is measured from.
        let hold_origin: Rc<Cell<Option<Point>>> = Rc::new(Cell::new(None));
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
            let tooltip_shown = tooltip_shown.clone();
            let tooltip_anchor_id = self_id;
            let hold_origin = hold_origin.clone();
            let press_floor = self.press_floor.clone();
            let press_floor_claimants = self.over_claimants.clone();
            let press_floor_memo = self.veto_memo.clone();
            let press_floor_scans = self.veto_scans.clone();
            let press_floor_generation = self.snapshot_generation.clone();
            let transform_for_cursor = self.transform_driver();
            handlers = handlers.on_pointer_event(move |ev, ctx| {
                use teksilo_core::event::PointerButton;
                use teksilo_core::event::WidgetEvent as Ev;
                use teksilo_core::widget::CursorIcon;

                // The kind half of every grab tolerance below, resolved per
                // event: the same view serves a mouse and a finger, so the
                // tolerance cannot be decided at build time. The token half is
                // the build-time snapshot, which a density change invalidates by
                // marking the tree for rebuild.
                let slop = super::GrabSlop::new(input_tokens, ctx.pointer_kind());

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

                // The paint-order floor for THIS dispatch. `on_pointer_event`
                // fires on every strict ancestor during the preview pass and on
                // the target itself during the bubble, so this handler running
                // at all means one of exactly two things: a heavyweight child
                // won the arena's walk (we are previewing it), or nothing did
                // and we are the target. Reading the router's verdict is what
                // lets the lightweight tier yield to a card **without** guessing
                // the card's own hit-test from a rectangle — a guess that would
                // be wrong for every node that overrides `Widget::hit_shape`,
                // which `docs/teksilo-scene.md` tells app authors to do.
                let floor = SceneView::dispatch_floor(ctx, self_id);

                // Hit-test the handler snapshot for the topmost item under
                // the pointer. Shared with the long-press handler below, so
                // both routes agree on what "the item under the finger" means.
                let hit_handler_item =
                    |screen_pt: Point, scene_pt: Point, slop: super::GrabSlop| {
                        super::hit_handler_item(
                            &handler_snapshot.borrow(),
                            screen_pt,
                            scene_pt,
                            view_xform_signal.get(),
                            slop,
                            floor,
                        )
                    };

                match ev {
                    Ev::PointerMove { position, .. } => {
                        cursor_pos.set(Some(*position));
                        // Disarm the hold as soon as the contact has travelled far
                        // enough to be something else. Runs before the hover gate
                        // below, because a contact never reaches that.
                        if let Some(origin) = hold_origin.get() {
                            let dx = position.x - origin.x;
                            let dy = position.y - origin.y;
                            if (dx * dx + dy * dy).sqrt() > slop.tap_tolerance_px() {
                                hold_origin.set(None);
                                ctx.cancel_delayed_overlay(tooltip_content_id);
                            }
                        }
                        // Hover, item tooltips and the cursor shape are all one
                        // seam and it belongs to a pointer that hovers. A contact
                        // is dispatched moves like any other pointer, but it has
                        // no hover to give — the tree refuses it the hover-owner
                        // role — and running the seam for one has two visible
                        // costs: a finger dragging across items schedules item
                        // tooltips it never asked for, and since a lift produces
                        // no `PointerLeave` the only retract path never runs, so
                        // the tip outlives the gesture. A finger reaches an
                        // item's tip by holding still on it instead; see the
                        // long-press handler.
                        if !ctx.pointer_kind().hovers() {
                            return EventResponse::Ignored;
                        }
                        let scene_pt = to_scene(*position);

                        // Hover transitions: compare current hit
                        // with previously-hovered item; fire
                        // on_hover(false) on the old, on_hover(true)
                        // on the new.
                        let new_hit = hit_handler_item(*position, scene_pt, slop);
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
                                // Build the tooltip body if this is the first
                                // time this view shows one.
                                tooltip_shown.set(true);
                                ctx.materialize_now(tooltip_content_id);
                                ctx.show_overlay_after(
                                    teksilo_core::overlay::OverlayRequest {
                                        content_id: tooltip_content_id,
                                        anchor: tooltip_anchor_id,
                                        // Drop the tooltip just below-right
                                        // of the cursor so it doesn't sit
                                        // under the pointer.
                                        placement:
                                            teksilo_core::overlay::OverlayPlacement::AtPointer(
                                                Point::new(position.x + 12.0, position.y + 16.0),
                                            ),
                                        // Manual: every dismiss path (item
                                        // change, pointer-down, pointer-leave)
                                        // is driven explicitly below.
                                        dismiss: teksilo_core::overlay::DismissBehavior::Manual,
                                        layer: teksilo_core::overlay::OverlayLayer::InTree,
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
                                slop,
                                floor,
                            )
                            .is_some()
                        };
                        // The cursor rule. When this view is yielding — a card
                        // took the pointer and nothing of ours has a cursor to
                        // offer on top of it — the card's own node cursor is
                        // the right answer, so a yield must not be a reset to
                        // `Default`, which the preview pass would write *over*
                        // the card's declaration.
                        //
                        // Nor can it be silence. The cursor moves only when
                        // something writes to it, and the card's declaration is
                        // written on `PointerEnter`/`PointerLeave` alone — so
                        // going quiet after this view has already spoken for
                        // the same hover episode leaves *our* last word
                        // standing. Slide off an `Over` hint onto the note
                        // under it and the note keeps the hint's cursor for as
                        // long as the pointer stays on the note, because no
                        // hover transition ever fires in between. A yield is
                        // therefore a **hand-back**: `release_cursor` withdraws
                        // whatever we said and lets the card's declaration
                        // apply again. It is a no-op when we never spoke.
                        //
                        // The test is `item_cursor.is_none()`, NOT
                        // `new_hit.is_none()`. The snapshot deliberately holds
                        // every hit-testable entry, so a purely **decorative**
                        // `Over` rect — the exact case a yield exists to
                        // protect — is still a `new_hit`, and keying off that
                        // stops the yield and stomps the card's cursor with
                        // `Default`. What decides is whether this view actually
                        // has an answer: a cursor the item declared, or the
                        // `Grab` it owes a draggable one.
                        //
                        // This is also where an `Over` item's `cursor` is
                        // arbitrated rather than vetoed: it wins here, over the
                        // card, without taking the card's press away (see
                        // `crate::pick::claims_press`).
                        //
                        // A drag in flight is never a yield: the pointer is
                        // captured by this view, so the `Grabbing` cursor is
                        // ours to set wherever the hand has got to. (The captor
                        // path already reports this view as the target, so the
                        // `floor` test alone would do it — this says so rather
                        // than relying on it.)
                        // The transform chrome is painted on top of everything
                        // and grabbed before everything, so its cursor outranks
                        // everything too — including a card's, which is the one
                        // case where saying nothing would be wrong. It also
                        // records which handle is hovered, for the chrome.
                        let transform_cursor = transform_for_cursor
                            .as_ref()
                            .and_then(|d| d.hover(scene_pt, slop));
                        let yielding = transform_cursor.is_none()
                            && drag_target_for_cursor.get().is_none()
                            && floor > crate::PaintKey::bottom()
                            && item_cursor.is_none()
                            && !over_draggable;
                        if yielding {
                            ctx.release_cursor();
                        } else {
                            let cursor = if let Some(c) = transform_cursor {
                                c
                            } else if drag_target_for_cursor.get().is_some() {
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
                    }
                    Ev::PointerDown {
                        position,
                        button,
                        modifiers,
                        ..
                    } => {
                        cursor_pos.set(Some(*position));
                        let scene_pt = to_scene(*position);
                        // The floor for the whole press, recorded here because
                        // this is the one dispatch that carries the router's
                        // verdict. `floor` already says "a card won the arena's
                        // walk"; the `Over`-claimant test adds the other way a
                        // press can reach this view over a card -- the veto. The
                        // drag reads this rather than re-deriving it, so a grab
                        // and a tap on the same press cannot disagree about
                        // what was on top.
                        // "Is anything painted above the cards claiming this
                        // press?" — asked at the widget rank rather than at
                        // `Over`, so an `Interleaved` claimant is seen too. The
                        // floor recorded is the claimant's own **rank**, not its
                        // whole key: a rank is what the drag hit test below
                        // takes, and both forms resolve to the same winner
                        // because that hit returns the topmost match anyway.
                        let veto = super::press_claimant_above(
                            super::VetoState {
                                over_claimants: &press_floor_claimants,
                                veto_memo: &press_floor_memo,
                                veto_scans: &press_floor_scans,
                                snapshot_generation: &press_floor_generation,
                                handler_snapshot: &handler_snapshot,
                                view_transform: &view_xform_signal,
                            },
                            crate::PaintKey::rank_floor(crate::RANK_WIDGET),
                            scene_pt,
                        );
                        press_floor.set(match veto {
                            Some(key) => floor.max(crate::PaintKey::rank_floor(key.rank())),
                            None => floor,
                        });
                        let hit = hit_handler_item(*position, scene_pt, slop);
                        // Any press retracts a hover tooltip — the user has
                        // committed to an action. The shown one always goes; the
                        // *pending* one is cancelled only when this press is not
                        // itself arming a hold, because the router applies the
                        // whole handler's delayed-show requests before its cancels
                        // and a cancel issued here would take the hold's own show
                        // down with it. Nothing is lost by skipping it: a
                        // `show_overlay_after` already replaces a pending show for
                        // the same content.
                        ctx.dismiss_overlay_by_content(tooltip_content_id);
                        let arming_hold = !ctx.pointer_kind().hovers()
                            && hit
                                .as_ref()
                                .and_then(|e| e.handlers.as_deref())
                                .is_some_and(|h| h.tooltip.is_some());
                        if !arming_hold {
                            ctx.cancel_delayed_overlay(tooltip_content_id);
                        }
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
                                        .unwrap_or(teksilo_core::event::ButtonMask::PRIMARY);
                                    if accept.contains(*button) {
                                        // The press point is recorded in VIEW
                                        // pixels, not scene units: the release
                                        // compares the two on screen, and a scene
                                        // comparison would both scale the
                                        // tolerance with the zoom and measure the
                                        // press through a transform the release no
                                        // longer projects through when the zoom
                                        // changed mid-gesture.
                                        pending_tap.set(Some((*position, entry.id, *button)));
                                    } else {
                                        pending_tap.set(None);
                                    }
                                } else {
                                    pending_tap.set(None);
                                }
                            }
                        }

                        // --- The hold: a contact's route to an item's tip
                        //
                        // A scene item is not a widget, so the framework's own
                        // hold route cannot reach it: the item has no `WidgetId`
                        // to carry an attached tooltip for the tree to find. The
                        // view holds the one tooltip surface every item shares, so
                        // the view is where a hold on an item has to be answered.
                        //
                        // Armed as a *delayed overlay* rather than through an
                        // `on_long_press` handler, because installing one would
                        // put a `LongPressRecognizer` in this node's arena, and a
                        // recognizer that wins resets its peers — a mouse press
                        // held past the deadline would lose the marquee its drag
                        // recognizer was waiting to start. See
                        // `a_mouse_press_held_past_the_hold_deadline_still_marquees`.
                        hold_origin.set(None);
                        if arming_hold
                            && let Some(ls) = hit
                                .as_ref()
                                .and_then(|e| e.handlers.as_deref())
                                .and_then(|h| h.tooltip.as_ref())
                        {
                            hold_origin.set(Some(*position));
                            tooltip_text.set(ls.resolve_now());
                            tooltip_shown.set(true);
                            ctx.materialize_now(tooltip_content_id);
                            ctx.show_overlay_after(
                                teksilo_core::overlay::OverlayRequest {
                                    content_id: tooltip_content_id,
                                    anchor: tooltip_anchor_id,
                                    placement: teksilo_core::overlay::OverlayPlacement::AtPointer(
                                        Point::new(position.x + 12.0, position.y + 16.0),
                                    ),
                                    dismiss: teksilo_core::overlay::DismissBehavior::Manual,
                                    layer: teksilo_core::overlay::OverlayLayer::InTree,
                                    parent_overlay: None,
                                    on_dismiss: None,
                                    fade_duration: tooltip_fade,
                                },
                                // The wait is the pointer's own long-press
                                // duration, read from its gesture profile rather
                                // than being a second number the scene invents.
                                input_tokens.profile(ctx.pointer_kind()).long_press,
                            );
                        }
                    }
                    Ev::PointerUp {
                        position,
                        button,
                        modifiers,
                        ..
                    } => {
                        // A lift before the deadline is a tap, not a hold, so the
                        // pending tip is dropped. A lift *after* it finds nothing
                        // pending — the tip is already up — and is left alone, so a
                        // finger can read what it uncovered.
                        if hold_origin.take().is_some() {
                            ctx.cancel_delayed_overlay(tooltip_content_id);
                        }
                        // The press is over; the next one records its own.
                        press_floor.set(crate::PaintKey::bottom());
                        // Tap dispatch only fires when the button that
                        // came back up matches the one we recorded on
                        // the press. Mixed-button down/up sequences
                        // discard the pending tap.
                        if let Some((press_screen, item_id, press_button)) = pending_tap.take()
                            && press_button == *button
                        {
                            let scene_pt = to_scene(*position);
                            let dx = position.x - press_screen.x;
                            let dy = position.y - press_screen.y;
                            if (dx * dx + dy * dy).sqrt() <= slop.tap_tolerance_px() {
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
                    _ => {}
                }
                EventResponse::Ignored
            });
        }

        // --- The departure ---------------------------------------------
        //
        // The hover episode's other end, and it has to be registered *here*
        // rather than as a `PointerLeave` arm of the `on_pointer_event` closure
        // above — which is where it used to live, unreachable on both passes.
        // `PointerEnter` / `PointerLeave` are hover transitions the router
        // synthesizes rather than raw pointer samples, and it routes them
        // accordingly: `try_handler_preview` returns `None` for both outright
        // (so an ancestor's drag guard can never swallow a descendant's hover),
        // and the bubble matches its own dedicated `on_hover` arms well before
        // it reaches the `on_pointer_event` catch-all. Nothing in the router
        // delivers Enter or Leave to `on_pointer_event` at all, so the arm ran
        // exactly never — and the cursor the view had raised over a draggable
        // item stayed raised over whatever the pointer went on to, forever,
        // while the item it had left went on believing it was hovered and its
        // tooltip went on counting down.
        //
        // `on_hover` is the route the router actually delivers on, and it is
        // the same one every other widget in the workspace receives a leave
        // through. It fires on the view's own node whenever the pointer crosses
        // into or out of it *or any of its descendants*, so a heavyweight card
        // inside the scene is covered by the same registration as the window
        // boundary.
        //
        // The `true` half is deliberately empty: the `PointerMove` that caused
        // the transition is dispatched right after the enter and re-decides the
        // whole seam from the pointer's actual position, which is a better
        // answer than anything an enter carrying no position could give.
        {
            let unwind = self.hover_unwind(tooltip_content_id);
            handlers = handlers.on_hover(move |entered, ctx| {
                if entered {
                    return;
                }
                // Only the hover episode. A press is **not** a hover, so
                // `pending_tap`, the hold and the press floor are left exactly
                // where they are — the desktop convention is that a click whose
                // pointer wanders off the control and comes back still
                // activates, and a press that is genuinely taken away arrives
                // as the `PointerCancel` below, which owns that half.
                unwind.clear(ctx);
            });
        }

        // --- The terminal cancel -------------------------------------
        //
        // `PointerCancel` is terminal: no `PointerUp` follows, nothing may
        // activate, and every widget is expected to unwind the state it opened
        // for that pointer itself. The scene had no arm for it at all, so a
        // revoked contact left behind whatever the interaction had reached —
        // a translated item, a lasso, a half-drawn wire, a pending tap that the
        // *next* release would then fire, a tooltip nobody could dismiss.
        //
        // Registered here rather than in `register_drag_handlers` because the
        // hover / tooltip / pending-tap half of the state lives here and is
        // installed unconditionally, while the drag handlers are only installed
        // when selection or magnetism is on. `HandlerSet::on_pointer_cancel` is
        // a single slot, so there is one arm and it owns the whole unwind.
        //
        // A revocation is a departure **plus** the press. The hover half is the
        // same `HoverUnwind` the leave arm above runs, shared rather than
        // restated so the two cannot drift; what a cancel adds is everything a
        // press had in flight — the drag visual, the pending tap, the hold, the
        // press floor — which a mere departure must *not* touch, because a
        // pointer that wanders out of a control and back is still entitled to
        // its click.
        {
            let pending_tap = self.pending_tap.clone();
            let hold_origin = hold_origin.clone();
            let unwind = self.drag_unwind();
            let hover_unwind = self.hover_unwind(tooltip_content_id);
            let press_floor_for_cancel = self.press_floor.clone();
            let reconcile_dirty = self.reconcile_dirty.clone();
            handlers = handlers.on_pointer_cancel(move |_pointer, _reason, ctx| {
                // Everything the contact had in flight — see `DragUnwind`,
                // which the drag's own `Cancelled` arm hands to as well. The
                // keyboard connect flow's half-made connection is deliberately
                // NOT in it: that is armed by a key, not by this pointer, and a
                // revoked contact is no reason to drop it.
                let had_visual = unwind.clear(ctx);

                // A press that is taken away can never become a tap, and the
                // hold it may have armed can never become a tooltip.
                pending_tap.set(None);
                press_floor_for_cancel.set(crate::PaintKey::bottom());
                hold_origin.set(None);

                // The hover half: the item's owed `on_hover(false)`, both
                // tooltip retraction paths, and the cursor. Hover is owned by a
                // pointer that hovers, so a cancelled contact usually has none
                // — but a pen or a mouse can be revoked too, and the tooltip
                // calls are needed either way (a *held* tip was armed by a
                // contact that never hovered at all).
                hover_unwind.clear(ctx);

                if had_visual {
                    // Drive the rebuild that re-places the item at its model
                    // position; without it the frame that painted the drag
                    // offset is the last one drawn until something else
                    // dirties the view.
                    reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
                }
            });
        }

        handlers
    }

    /// Wheel / trackpad pan, Ctrl+wheel zoom, pinch, and the keyboard camera —
    /// plus the one arm that is **not** camera input: a descendant's request to
    /// be revealed.
    ///
    /// `interactive` gates the input arms, not the reveal. A non-interactive
    /// view is one the *user* may not drive; a focused caret inside a card it
    /// contains is still entitled to be on screen, and a read-only page whose
    /// editor scrolls away from its own caret is a bug in every host that has
    /// shipped it. So `on_scroll` is registered either way and its wheel body
    /// returns early when the view is not interactive — which is what a view
    /// with no handler at all did, since `try_handler_bubble`'s `None` and
    /// `Some(Ignored)` meet at the same `unwrap_or` one line later.
    pub(super) fn register_camera_handlers(
        &self,
        mut handlers: HandlerSet,
        line_height: f32,
        pan_dur: Duration,
        overscroll: OverscrollBehavior,
        prefers_reduced: bool,
        interactive: bool,
    ) -> HandlerSet {
        {
            // The camera, as something a closure can own. The reveal arm below
            // has to move it from inside a `HandlerSet` closure, which outlives
            // the `&self` that built it.
            let camera = self.camera();
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
            handlers = handlers.on_scroll(move |event, ctx| {
                use crate::scene::PanAxes;
                // **A descendant asked to be seen.** The framework's reveal
                // walk (`WidgetTree::scroll_rect_into_view`) visits every
                // `clips_children` ancestor of the widget that called
                // `EventContext::ensure_visible`, and a `SceneView` clips — so
                // a caret moving inside an embedded `RichTextEditor` arrives
                // here, and arrives *first*, before the wheel arm that shares
                // this slot.
                //
                // `target_bounds` is already in this view's **content** space,
                // which for a scene is scene coordinates: the walk projects the
                // rectangle into each ancestor's own space as it climbs, and a
                // card's arena bounds inside a `SceneView` are its scene rect
                // (the camera is a content transform, so a card at scene
                // (100, 100) keeps bounds of (100, 100) however far the camera
                // has panned). No inverse belongs here; applying one would
                // un-camera a rectangle that was never cameraed.
                //
                // Reduced motion is honoured on this route and not on the
                // public `ensure_visible`, because only this one has an
                // `EventContext` to ask. A reveal is a jump the user did not
                // request, which is precisely the class the preference is about.
                if let WidgetEvent::ScrollIntoView {
                    target_bounds,
                    margin,
                    align,
                    motion,
                    applied_scroll,
                } = event
                {
                    let motion = if prefers_reduced {
                        ScrollMotion::Instant
                    } else {
                        *motion
                    };
                    let applied = camera.reveal(*target_bounds, *margin, *align, motion);
                    // The back-channel, in the space the event was stated in.
                    // An outer scroll container is re-targeted by subtracting
                    // this from `target_bounds` before the walk projects it
                    // outward; leaving it zero costs that container nothing but
                    // accuracy, and filling it with a *screen*-space pan delta
                    // would cost it a factor of the zoom.
                    if let Some(cell) = applied_scroll
                        && let Ok(mut slot) = cell.lock()
                    {
                        *slot = Point::new(applied.x, applied.y);
                    }
                    return if applied == Vec2::ZERO {
                        // Nothing moved: the target already fits, or a policy
                        // or a clamp refused. Saying `Handled` here would buy a
                        // layout pass for a decision not to move.
                        EventResponse::Ignored
                    } else {
                        EventResponse::Handled
                    };
                }
                // Everything below is camera *input*, which a non-interactive
                // view does not take.
                if !interactive {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::Scroll {
                    delta,
                    modifiers,
                    phase,
                    ..
                } = event
                else {
                    return EventResponse::Ignored;
                };
                let touch_pan =
                    ctx.scroll_source() == teksilo_core::pointer::ScrollSource::TouchPan;
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
                // A finger's pan is a pan, whatever a keyboard is doing at
                // the same time: the zoom branch below is a *wheel* gesture
                // (Ctrl held, one notch at a time), and a pinch — the touch
                // gesture that zooms — arrives as a `PinchChanged`, not here.
                // Testing the source before the modifier is what keeps a
                // stray Ctrl from turning a drag into a zoom.
                if touch_pan {
                    return pan_by_touch(
                        &PanByTouch {
                            pan_x: &pan_x,
                            pan_y: &pan_y,
                            zoom: &zoom,
                            scene_pan_bounds: &scene_pan_bounds_sig,
                            view_pan_bounds: &view_pan_bounds_sig,
                            viewport: &last_viewport_for_scroll,
                        },
                        *phase,
                        dx,
                        dy,
                        overscroll,
                    );
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
                        None => teksilo_canvas::Point::new(
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
                // Sign convention — must match the rest of the framework.
                //
                // A `ScrollDelta` is expressed in *scroll-offset* terms: a
                // POSITIVE y is "scroll down", and every scrollable moves its
                // content UP in response (see `ScrollArea`, whose own test pins
                // `Pixels { y: +100 }` → content lands at a negative y).
                //
                // A scene's `pan` is NOT a scroll offset: it is *added* to the
                // content's position by the view transform
                // (`compose_view` → `translate(+pan)`), so a larger `pan.y`
                // pushes content DOWN — the exact opposite of a scroll offset,
                // which is subtracted. The delta must therefore be **negated**
                // when it drives a pan, or the scene scrolls backwards
                // (content chasing the wheel, the macOS "natural" feel) while
                // every list/panel in the same app scrolls the normal way.
                //
                // So: positive delta (scroll down / right) → pan decreases →
                // content moves up / left → the viewport advances down / right
                // through the scene. Same as `ScrollArea`.
                let base_x = pan_x.animation_target().unwrap_or_else(|| pan_x.get());
                let base_y = pan_y.animation_target().unwrap_or_else(|| pan_y.get());
                // Clamp the projected pan against effective bounds.
                // Axes already applied by zeroing dx/dy above.
                let clamped = clamp_pan(
                    Vec2::new(base_x - dx, base_y - dy),
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
                    (clamped.x - base_x).abs() > teksilo_core::overscroll::SCROLL_MOVE_EPSILON;
                let moved_y =
                    (clamped.y - base_y).abs() > teksilo_core::overscroll::SCROLL_MOVE_EPSILON;
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

        // The camera *input* arms. Gated as they always were: these are
        // the user driving the view, which is exactly what
        // `interactive(false)` switches off. Only the reveal arm above
        // outlives the flag.
        if !interactive {
            return handlers;
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
                    ..
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
            use teksilo_core::event::{EventResponse, Key, WidgetEvent};
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
            // Keyboard item-nudge (WCAG 2.5.7): move the selected item(s).
            let selection_for_keys = self.selection.clone();
            let connect_mode_keys = self.magnet_connect_mode.clone();
            let magnet_focus_keys = self.magnet_focus.clone();
            let magnet_pending_keys = self.magnet_pending.clone();
            let self_id_for_keys = self.self_widget_id.get();
            let transform_for_keys = self.transform_driver();
            handlers = handlers.on_key(move |event, ctx| {
                use crate::scene::PanAxes;
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
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
                // The selection transform controller's keyboard route: the
                // mode key enters it, then Tab roves the handles, arrows drive
                // the roved one, Enter commits and Esc cancels. It runs before
                // the pan/zoom keys because an active mode owns the arrows —
                // and it deliberately declines an Alt+Arrow, which stays the
                // immediate nudge below.
                if let Some(d) = transform_for_keys.as_ref()
                    && d.handle_key(key, *modifiers, ctx)
                {
                    return EventResponse::Handled;
                }
                // Alt+Arrow nudges the selected scene item(s) — the keyboard
                // alternative to pointer drag-to-move (WCAG 2.5.7 Dragging
                // Movements). Moves every selected **root** so a multi-selection
                // stays together, which is what the pointer's group drag does:
                // a selected item whose ancestor is also selected is dropped,
                // because its `local_pos` is parent-relative and moving the
                // ancestor already moved it. (Moving both translated it twice,
                // which is what this used to do.) Directly mutates the model
                // (SceneModel mutators are `&self`); the resulting ItemChange
                // reconciles the view + AT bounds. Scene-coord step; Shift = x10.
                // Ignores view rotation (v1).
                if modifiers.alt() {
                    let step = if modifiers.shift() { 10.0 } else { 1.0 };
                    let delta = match key {
                        Key::ArrowLeft => Some((-step, 0.0)),
                        Key::ArrowRight => Some((step, 0.0)),
                        Key::ArrowUp => Some((0.0, -step)),
                        Key::ArrowDown => Some((0.0, step)),
                        _ => None,
                    };
                    if let Some((dx, dy)) = delta {
                        let selected = selection_for_keys.selected();
                        if !selected.is_empty() {
                            let roots = model_for_keys.selection_roots(&selected);
                            // The same document rule the pointer obeys, over
                            // the same quantity — the roots' scene box, not a
                            // per-item position. A nudge that ignored it would
                            // give the keyboard a different geometry from the
                            // mouse, which is the WCAG 2.5.7 equivalence this
                            // handler exists to keep.
                            let (dx, dy) = match model_for_keys.transform_frame(&roots) {
                                Some(start) => {
                                    let t = model_for_keys.constrain_move(
                                        &roots,
                                        start,
                                        Vec2::new(dx, dy),
                                        crate::transform_session::TransformSource::Keyboard,
                                    );
                                    (t.x, t.y)
                                }
                                None => (dx, dy),
                            };
                            // A refused nudge is (0, 0) and is applied anyway:
                            // `Scene::set_local_pos` early-returns on an
                            // unchanged value, so nothing is written and no
                            // `ItemChange` is emitted. Guarding it here would be
                            // a second, untestable statement of the same rule.
                            // One `User` transaction for the whole selection:
                            // a 12-item nudge is one edit the user made, and
                            // without the guard it arrives as 12 unrelated
                            // moves with nothing tying them together.
                            let _txn = model_for_keys.user_edit();
                            for id in roots {
                                if let Some(p) = model_for_keys.local_pos(id) {
                                    model_for_keys.set_local_pos(
                                        id,
                                        teksilo_canvas::Point::new(p.x + dx, p.y + dy),
                                    );
                                }
                            }
                            return EventResponse::Handled;
                        }
                    }
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
                    let anchor_screen = teksilo_canvas::Point::new(
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

    pub(super) fn register_drag_handlers(
        &self,
        mut handlers: HandlerSet,
        input_tokens: teksilo_tokens::InputTokens,
    ) -> HandlerSet {
        let marquee = self.marquee.clone();
        let pending_marquee_commit = self.pending_marquee_commit.clone();
        let marquee_mode = self.marquee_mode;
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
        // The drag group (and hence the frame a geometry constraint measures
        // against) is derived from the selection at the press. Captured as a
        // handle, not snapshotted: `drag_group_of` is the one definition of
        // "what is moving", shared with the commit and the paint feedback.
        let selection_for_drag = self.selection.clone();
        let magnetism_for_drag = self.magnetism.clone();
        let port_drag = self.port_drag.clone();
        let item_snap = self.item_snap.clone();
        let drag_unwind = self.drag_unwind();
        let press_floor_for_drag = self.press_floor.clone();
        // The selection transform controller, as a bundle of shared handles —
        // a `HandlerSet` closure outlives the `&self` that built it, so the
        // controller's behaviour cannot live on a `&self` method. `None` when
        // no controller is installed, and then every branch below is exactly
        // what it was.
        let transform_for_drag = self.transform_driver();
        handlers = handlers.on_drag(move |phase, ctx| {
            // Per-event grab tolerance: see the twin in
            // `register_pointer_handlers`. Zero for a mouse by arithmetic, so
            // the two grab hit tests below are byte-identical for one.
            let slop = super::GrabSlop::new(input_tokens, ctx.pointer_kind());
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
                use teksilo_core::gesture::DragPhase;
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
            use teksilo_core::gesture::DragPhase;
            match phase {
                DragPhase::Started {
                    position, button, ..
                } => {
                    if !matches!(button, teksilo_core::event::PointerButton::Primary) {
                        return;
                    }
                    // Project screen press to scene coords for
                    // hit-test against the snapshot.
                    let xform = view_xform_signal.get();
                    let scene_press = match xform.inverse() {
                        Some(inv) => inv.apply_point(position),
                        None => Point::ZERO,
                    };
                    // The transform controller's chrome outranks everything
                    // else: it is painted on top, and — because the frame is
                    // drawn `padding` **outside** the selection — it is the one
                    // affordance that is reachable over a heavyweight card,
                    // whose own pixels stop at the content edge.
                    if let Some(d) = transform_for_drag.as_ref()
                        && let Some(handle) = d.hit_handle(scene_press, slop)
                        && d.begin(
                            handle,
                            scene_press,
                            Some(position),
                            crate::transform_session::TransformSource::Pointer,
                            ctx,
                        )
                    {
                        return;
                    }
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
                        let grab = slop.magnet_grab_scene_radius(cfg.capture_px, zoom);
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
                    // Group move: a press on the **body** of a selected,
                    // movable item drags the whole selection. This is the only
                    // route by which a heavyweight card moves at all — the
                    // draggable snapshot below is lightweight-only, by
                    // construction — and it is best-effort over one, because a
                    // card that claims its own press (`capture_pointer`, or its
                    // own `on_drag`) wins it and this handler never runs. The
                    // frame band above is the route that always works.
                    //
                    // `IS_DRAGGABLE` is honoured here, not bypassed: routing by
                    // selection membership alone would make every default item
                    // pointer-movable, since `ItemFlags::default()` carries
                    // `IS_SELECTABLE` and not `IS_DRAGGABLE`.
                    if let Some(d) = transform_for_drag.as_ref()
                        && d.hit_body(scene_press)
                        && d.begin(
                            crate::transform_session::TransformHandle::Move,
                            scene_press,
                            Some(position),
                            crate::transform_session::TransformSource::Pointer,
                            ctx,
                        )
                    {
                        return;
                    }
                    // Narrow-phase hit-test: target the topmost draggable
                    // item whose actual SHAPE (not just its AABB) contains the
                    // press, so a thin draggable item (e.g. a connector path)
                    // is grabbed only on its stroke. The snapshot is z-sorted
                    // and refreshed each layout pass — see `place_children`.
                    // The floor the press recorded -- see
                    // `SceneView::press_floor`. A `SceneView` sees the press
                    // even when a card is the arena's target (press-release is
                    // a descendant tap, press-then-move is an ancestor drag:
                    // click a card to select it, pull away from it to marquee),
                    // so "the drag is running, therefore no card was hit" is
                    // simply false. Without this floor a pull starting on a
                    // card would grab whatever lightweight item the card was
                    // covering.
                    let floor = press_floor_for_drag.get();
                    let hit = {
                        let snap = bounds_snapshot.borrow();
                        super::hit_draggable_item(&snap, position, scene_press, xform, slop, floor)
                    };
                    if let Some(item_id) = hit {
                        // Drag-to-move: enter that mode,
                        // not marquee.
                        //
                        // The group's scene box is captured **once**, here, and
                        // is what a geometry constraint is measured against for
                        // the rest of the gesture. It is deliberately not the
                        // press point: a constraint stated over the pointer
                        // would snap the cursor and leave the item off-grid by
                        // the grab offset on every drag.
                        //
                        // Captured only when there is a rule to measure it
                        // against. Both halves cost a full prune of the
                        // selection, and `constrain_drag` is the only reader —
                        // so an unconstrained scene, which is most scenes, would
                        // otherwise pay them on the press of every drag for a
                        // value nothing goes on to look at.
                        let start_frame = model_for_drag
                            .has_geometry_constraint()
                            .then(|| {
                                let group = super::drag_group_of(
                                    &model_for_drag,
                                    &selection_for_drag,
                                    item_id,
                                );
                                model_for_drag.transform_frame(&group)
                            })
                            .flatten();
                        drag_target.set(Some(DragTarget {
                            item_id,
                            anchor_scene: scene_press,
                            current_scene: scene_press,
                            start_frame,
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
                    // A live transform owns the gesture: it was claimed at the
                    // press, and nothing below can be running at the same time.
                    if let Some(d) = transform_for_drag.as_ref()
                        && d.is_active()
                    {
                        d.update(position, ctx);
                        return;
                    }
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
                            // The document's standing geometry rule gets the
                            // last word, over the magnetised proposal — and is
                            // told that it is magnetised, so a policy that
                            // wants an explicit magnet to outrank it says so in
                            // one line. Constraining `current_scene` rather
                            // than the committed delta is what makes the
                            // dragged ghost show the constrained position:
                            // the pair is only ever read as a difference, and
                            // this keeps that difference equal to the applied
                            // translation on every sample.
                            let magnetised = target.current_scene;
                            let was_snapped = item_snap.borrow().is_some();
                            constrain_drag(
                                &model_for_drag,
                                &selection_for_drag,
                                &mut target,
                                was_snapped,
                            );
                            // A magnet the rule overruled is not snapped, and
                            // must not go on drawing its marker as if it were.
                            // The same verdict decides the geometry, the
                            // feedback and (at the release) the connection.
                            if was_snapped && !constraint_kept(magnetised, target.current_scene) {
                                item_snap.replace(None);
                            }
                            drag_target.set(Some(target));
                        }
                    } else if let Some(mut state) = marquee.get() {
                        state.current = position;
                        marquee.set(Some(state));
                    }
                }
                DragPhase::Ended { position, .. } => {
                    // The transform's one and only model write. Everything up
                    // to here was a preview; this posts a single
                    // `Scene::apply_transform_delta` for `build()` to apply.
                    if let Some(d) = transform_for_drag.as_ref()
                        && d.is_active()
                    {
                        d.update(position, ctx);
                        d.commit(ctx);
                        return;
                    }
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
                        let mut magnet_connection = None;
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
                                magnet_connection = build_connection(
                                    &model_for_drag,
                                    snap.from,
                                    snap.to,
                                    snap.payload,
                                );
                            }
                        }
                        // The geometry constraint sees the release sample with
                        // exactly the inputs the last preview gave it, so the
                        // committed position is the previewed one and the item
                        // does not jump at the release.
                        let magnetised = target.current_scene;
                        constrain_drag(
                            &model_for_drag,
                            &selection_for_drag,
                            &mut target,
                            magnet_connection.is_some(),
                        );
                        // A magnet whose alignment the document's rule overruled
                        // did not connect anything. Two snapping systems that
                        // disagree is worse than one, so the constraint's
                        // verdict decides both the geometry and the connection.
                        if let Some(conn) = magnet_connection
                            && constraint_kept(magnetised, target.current_scene)
                            && let Some(cfg) =
                                magnetism_for_drag.as_ref().filter(|c| c.enabled.get())
                        {
                            (cfg.on_connect)(&conn, ctx);
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
                    // Under a rotated view the band's scene-space form is a
                    // quadrilateral, not a box. Taking its AABB — which is
                    // what `inv.apply_rect` gives — over-selects everything in
                    // the enlarged hull, so `SceneRegion` keeps the exact
                    // corners whenever the transform does not preserve axes.
                    let region = match xform.inverse() {
                        Some(inv) => SceneRegion::from_screen_rect(screen_rect, &inv),
                        None => SceneRegion::empty(),
                    };
                    pending_marquee_commit.replace(Some((region, marquee_mode, state.additive)));
                    reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
                }
                // A revoked drag commits nothing AND leaves nothing behind.
                //
                // This used to drop only the lasso and the port-drag wire, and
                // leave `drag_target` / `item_snap` set — so a revoked drag
                // painted the item at the dragged position for ever while the
                // model went on saying it had never moved. The whole list now
                // lives in `DragUnwind`, which the node's `on_pointer_cancel`
                // arm hands to as well; clearing twice is a no-op, and having
                // one list is what stops the two arms drifting apart.
                //
                // Both arms exist on purpose. This one is the drag recognizer's
                // own contract ("a handler that committed as it went unwinds
                // here"); the `on_pointer_cancel` arm is installed
                // unconditionally, whereas these drag handlers are only
                // registered when selection or magnetism is on, and it also
                // unwinds the hover / pending-tap / hold state that lives over
                // there.
                DragPhase::Cancelled { .. } => {
                    drag_unwind.clear(ctx);
                    reconcile_dirty.set(reconcile_dirty.get().wrapping_add(1));
                }
                _ => {}
            }
        });
        handlers
    }
}

/// The signals [`pan_by_touch`] moves, borrowed rather than cloned: it is
/// called from inside the scroll handler that already owns them.
pub(super) struct PanByTouch<'a> {
    pub pan_x: &'a Signal<f32>,
    pub pan_y: &'a Signal<f32>,
    pub zoom: &'a Signal<f32>,
    pub scene_pan_bounds: &'a Signal<Option<Rect>>,
    pub view_pan_bounds: &'a Signal<Option<Rect>>,
    pub viewport: &'a Signal<Size>,
}

/// Move the view by one sample of a finger's pan, or of the coast that
/// follows it.
///
/// The scene deliberately does **not** go through
/// `teksilo_widgets::common::scrollable`, and the reason is the pan itself
/// rather than a preference. That helper models a surface as an offset in
/// `[0, max]` per axis; a scene's `pan` is neither. It is *added* by the view
/// transform rather than subtracted (so the delta is negated here, exactly as
/// on the wheel path), its legal region is an arbitrary rectangle that moves
/// with the zoom, and where that rectangle is smaller than the viewport on an
/// axis the rule is not a clamp at all but a centring pin — which no
/// `[min, max]` range can express. An unbounded scene has no range whatever.
/// So the geometry stays in [`clamp_pan`], and only the *policy* — no tween
/// under a finger, hard clamp on a coast, decline at the boundary and at the
/// end of the stream — is written out here.
///
/// Three differences from the wheel path above, each one the touch contract:
///
/// * **No tween.** A finger is already the animation, and a coast is already
///   integrated by the tree's fling driver; either one aimed at a 150 ms
///   ease-out would lag behind the hand.
/// * **The end of the stream is declined.** `Ended` / `MomentumEnded` /
///   `Cancelled` are bookkeeping every claimant outward must see, so this
///   surface never claims one — not even under
///   [`OverscrollBehavior::Contain`], which has no movement to contain.
/// * **A coast is hard-clamped.** The driver integrates an unbounded
///   simulation, so stopping it at the scene's bound is this surface's job.
///   That falls out of [`clamp_pan`] and the boundary answer below.
pub(super) fn pan_by_touch(
    signals: &PanByTouch<'_>,
    phase: ScrollPhase,
    dx: f32,
    dy: f32,
    overscroll: OverscrollBehavior,
) -> EventResponse {
    if matches!(
        phase,
        ScrollPhase::Ended | ScrollPhase::MomentumEnded | ScrollPhase::Cancelled
    ) {
        return EventResponse::Ignored;
    }
    if dx == 0.0 && dy == 0.0 {
        return EventResponse::Ignored;
    }
    // The live pan, not an animation target: a finger takes over from whatever
    // tween was in flight rather than accumulating onto its destination.
    let base_x = signals.pan_x.get();
    let base_y = signals.pan_y.get();
    let clamped = clamp_pan(
        Vec2::new(base_x - dx, base_y - dy),
        signals.scene_pan_bounds.get(),
        signals.view_pan_bounds.get(),
        signals.viewport.get(),
        signals.zoom.get(),
    );
    let moved_x = (clamped.x - base_x).abs() > teksilo_core::overscroll::SCROLL_MOVE_EPSILON;
    let moved_y = (clamped.y - base_y).abs() > teksilo_core::overscroll::SCROLL_MOVE_EPSILON;
    if !moved_x && !moved_y {
        return match overscroll {
            OverscrollBehavior::Contain => EventResponse::Handled,
            OverscrollBehavior::Chain => EventResponse::Ignored,
        };
    }
    signals.pan_x.set(clamped.x);
    signals.pan_y.set(clamped.y);
    EventResponse::Handled
}

/// Apply the scene's geometry constraint to a live lightweight drag, in place.
///
/// Rewrites `target.current_scene` so that `current_scene − anchor_scene` is
/// the **constrained** translation. That one field is what paint reads for the
/// dragged ghost *and* what `Ended` turns into the committed delta, so
/// constraining it here is what makes the preview, the commit and the
/// notification agree — there is no second place for them to drift apart.
///
/// A no-op when the scene carries no constraint (one `Option` test, no closure
/// built) or when the group had no resolvable geometry at the press.
pub(super) fn constrain_drag(
    model: &SceneModel,
    selection: &crate::selection::SceneSelection,
    target: &mut DragTarget,
    magnet_snapped: bool,
) {
    let Some(start) = target.start_frame else {
        return;
    };
    let raw = Vec2::new(
        target.current_scene.x - target.anchor_scene.x,
        target.current_scene.y - target.anchor_scene.y,
    );
    let scene = model.0.borrow();
    let Some(call) = crate::constrain::ConstraintCall::new(&scene) else {
        return;
    };
    // The drag group is resolved **here**, past the `Option` test, and not by
    // the caller. It prunes the whole selection — `Scene::selection_roots` — so
    // computing it before the test made a scene with no constraint at all pay
    // that prune on every pointer sample of every drag. The published claim
    // that an unconstrained scene "pays one `Option` test per gesture sample"
    // was only ever true of a single-item selection.
    //
    // A second shared borrow of the scene is legal and is what
    // `drag_group_of` needs; the exclusive one a mutator would take cannot
    // exist while this one is open.
    let group = super::drag_group_of(model, selection, target.item_id);
    let applied = crate::constrain::constrained_translation(
        &call,
        &group,
        &start,
        raw,
        crate::transform_session::TransformSource::Pointer,
        magnet_snapped,
    );
    target.current_scene = Point::new(
        target.anchor_scene.x + applied.x,
        target.anchor_scene.y + applied.y,
    );
}

/// Whether the geometry constraint left a magnetised proposal where it was.
///
/// Deliberately **not** `==`. The constraint path round-trips the translation
/// through `Transform2D::rotate(-theta)` and back, and for any non-zero
/// rotation that is not bit-exact — so an exact test reported "the rule
/// overruled the magnet" for a rule that had said
/// [`ChangeVerdict::Accept`](crate::ChangeVerdict::Accept), cleared the snap
/// and never fired `on_connect`. It failed only on rotated items, which is why
/// every in-tree test passed: at zero degrees the round-trip *is* exact.
///
/// The tolerance is stated in units in the last place rather than as a scene
/// distance, because the error it must absorb is a relative one: 64 ulps of the
/// larger coordinate. That is roughly two thousandths of a scene unit around
/// the origin and well under one unit out at 1e5 — far below anything a user
/// can see, and orders of magnitude below the smallest displacement a real
/// rule (a grid snap, a page clamp) applies.
pub(super) fn constraint_kept(before: Point, after: Point) -> bool {
    let scale = before
        .x
        .abs()
        .max(before.y.abs())
        .max(after.x.abs())
        .max(after.y.abs())
        .max(1.0);
    let tolerance = f32::EPSILON * 64.0 * scale;
    (before.x - after.x).abs() <= tolerance && (before.y - after.y).abs() <= tolerance
}
