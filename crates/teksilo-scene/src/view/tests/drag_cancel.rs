// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! AUDIT PROBE — not a shipped test. Proves whether a revoked drag leaves the
//! view's visual drag state behind.

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

    let id = finger();
    let from = Point::new(100.0, 90.0);
    tree.dispatch_pointer(contact(id, PointerPhase::Down, from, 0));
    // Several move samples: the first crosses the slop and *starts* the drag
    // (anchor == that position), the rest carry the item away from it.
    for (n, y) in [(1u64, 140.0f32), (2, 180.0), (3, 240.0)] {
        tree.dispatch_pointer(contact(id, PointerPhase::Move, Point::new(100.0, y), n * 16));
    }

    {
        let view = view_handle(&tree, view_id);
        assert!(
            view.drag_target.get().is_some(),
            "precondition: the finger grabbed the item",
        );
    }

    // The system revokes the pointer mid-drag.
    tree.dispatch_pointer(contact(id, PointerPhase::Cancel, Point::new(100.0, 240.0), 64));
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

    let id = finger();
    tree.dispatch_pointer(contact(id, PointerPhase::Down, Point::new(100.0, 90.0), 0));
    for (n, y) in [(1u64, 140.0f32), (2, 180.0), (3, 240.0)] {
        tree.dispatch_pointer(contact(id, PointerPhase::Move, Point::new(100.0, y), n * 16));
    }
    tree.dispatch_pointer(contact(id, PointerPhase::Cancel, Point::new(100.0, 240.0), 64));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let after = transform_commands(&tree.render());
    println!("after  transforms = {after:?}");
    assert_eq!(
        after, before,
        "a revoked drag must draw the item back where the model says it is",
    );
}
