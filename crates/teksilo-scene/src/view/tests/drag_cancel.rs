// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a revoked drag leaves behind.
//!
//! A `PointerCancel` is terminal: no `PointerUp` follows and nothing may
//! activate, so every widget has to unwind the state it opened for that
//! pointer. These two measure the scene's half of that, at the model and at the
//! renderer.
//!
//! **The gesture shape is load-bearing.** A `SceneView` with selection on
//! declares a `PanClaim` *and* carries `on_drag` on the same node, so its own
//! drag is deferred behind a hold (`PointerSequence::defer_own_drag`) and a
//! finger that simply presses and moves pans the camera instead of grabbing
//! anything — which is the behaviour `view::tests::touch_camera` pins. Both
//! tests therefore press, hold past the profile's `long_press`, and only then
//! travel, exactly as `touch_grabs::finger_drag` does. Measured with the
//! press-and-move shape these tests used to have, neither one can reach the
//! drag path at all: the first fails on its own precondition and the second
//! fails on a camera pan that a cancel has no business reverting.

use super::*;
use crate::items::RectItem;
use teksilo_canvas::Point;
use teksilo_core::event::Modifiers;
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample,
};

fn finger() -> PointerId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    PointerIdAllocator::global().begin(
        BackendDeviceKey::new(0x5CE3),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )
}

fn contact(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
    PointerSample {
        pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
        phase,
        position: at,
        button: None,
        modifiers: Modifiers::default(),
        coalesced: Vec::new(),
    }
}

/// The hold a scene grab is armed by, in milliseconds, plus a margin. See the
/// module doc, and `touch_grabs::hold_ms`.
fn hold_ms() -> u64 {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch)
        .long_press
        .as_millis() as u64
        + 50
}

/// Press at `from`, hold, arm the drag at `via`, then carry the item to `to` —
/// leaving the contact *live*, so the caller can revoke it.
///
/// Three samples after the press, and each one earns its place:
///
/// * one **at the press point** when the hold deadline arrives. The deferral is
///   a hold, so a press that has already travelled past `long_press_slop` by
///   then is withdrawn rather than armed.
/// * one at `via`, which is the sample the drag *starts* on — so `via` becomes
///   the drag's anchor.
/// * one at `to`, the first sample that actually carries the item, by
///   `to - via`. Without it the anchor and the current position coincide and
///   the in-flight offset is zero, which is a grab but not yet a move.
fn finger_grab_and_carry(
    tree: &mut WidgetTree,
    from: Point,
    via: Point,
    to: Point,
) -> (PointerId, u64) {
    let id = finger();
    let held = hold_ms();
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from, 0));
    tree.dispatch_pointer(contact(id, PointerPhase::Move, from, held));
    tree.dispatch_pointer(contact(id, PointerPhase::Move, via, held + 16));
    tree.dispatch_pointer(contact(id, PointerPhase::Move, to, held + 32));
    (id, held + 32)
}

fn draggable_scene() -> (Scene, crate::item::ItemId) {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 120.0, 90.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    (scene, id)
}

/// A revoked drag must leave nothing behind: the model position is unchanged
/// (that half already holds) AND the view must stop painting the item at the
/// dragged position.
#[test]
fn a_revoked_item_drag_leaves_the_item_where_it_was() {
    let (scene, item) = draggable_scene();
    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let (id, t) = finger_grab_and_carry(
        &mut tree,
        Point::new(100.0, 90.0),
        Point::new(100.0, 165.0),
        Point::new(100.0, 240.0),
    );

    {
        let view = view_handle(&tree, view_id);
        assert!(
            view.drag_target.get().is_some(),
            "precondition: the finger grabbed the item",
        );
    }

    // The system revokes the pointer mid-drag.
    tree.dispatch_pointer(contact(
        id,
        PointerPhase::Cancel,
        Point::new(100.0, 240.0),
        t + 16,
    ));
    // Let every deferred rebuild / relayout the cancel scheduled run.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let view = view_handle(&tree, view_id);
    // The model half — this is the part that already holds.
    assert_eq!(
        view.model().local_pos(item),
        Some(Point::new(50.0, 50.0)),
        "a revoked drag must commit nothing to the model",
    );
    // The visual half — paint reads `drag_target` and translates by
    // (current_scene - anchor_scene).
    let leftover = view.drag_target.get();
    assert!(
        leftover.is_none(),
        "a revoked drag must also drop the visual drag offset; paint still \
         translates this item by {:?}",
        leftover.map(|t| (
            t.current_scene.x - t.anchor_scene.x,
            t.current_scene.y - t.anchor_scene.y
        )),
    );
}

/// Every transform the frame pushes, in order. A `DecorationRect`'s `rect` is
/// in the *local* frame the enclosing transform stack maps; the scene's
/// drag offset lives in that stack, not in the rect.
fn transform_commands(frame: &teksilo_canvas::RenderFrame) -> Vec<[f32; 6]> {
    frame
        .draw_order
        .iter()
        .filter_map(|c| match c {
            teksilo_canvas::DrawCommand::PushTransform(t)
            | teksilo_canvas::DrawCommand::SetTransform(t) => Some(t.m),
            _ => None,
        })
        .collect()
}

/// The same thing measured at the renderer: after the cancel, the item must be
/// drawn where the model says it is.
#[test]
fn a_revoked_item_drag_paints_the_item_back_at_its_model_position() {
    let (scene, _item) = draggable_scene();
    let mut tree = WidgetTree::new();
    let _view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let before = transform_commands(&tree.render());
    println!("before transforms = {before:?}");

    let (id, t) = finger_grab_and_carry(
        &mut tree,
        Point::new(100.0, 90.0),
        Point::new(100.0, 165.0),
        Point::new(100.0, 240.0),
    );
    // The offset is on screen right now — this is what the cancel has to undo.
    let during = transform_commands(&tree.render());
    println!("during transforms = {during:?}");
    assert_ne!(
        during, before,
        "precondition: the in-flight drag is actually painting an offset",
    );
    tree.dispatch_pointer(contact(
        id,
        PointerPhase::Cancel,
        Point::new(100.0, 240.0),
        t + 16,
    ));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let after = transform_commands(&tree.render());
    println!("after  transforms = {after:?}");
    assert_eq!(
        after, before,
        "a revoked drag must draw the item back where the model says it is",
    );
}
