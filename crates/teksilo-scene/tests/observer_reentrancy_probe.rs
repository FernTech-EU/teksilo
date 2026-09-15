// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! PROBE (not a regression test): characterises what an `item_change_signal`
//! observer may and may not do, because `docs/teksilo-scene.md` advertises the
//! signal as the place to "wire snap-to-grid, validation, persistence" and the
//! only `&self` door back into the scene is `SceneModel`, which holds
//! `borrow_mut()` across the mutation — including the synchronous observer
//! fan-out in `Scene::emit_item_change`.
//!
//! Run with: cargo test -p teksilo-scene --test observer_reentrancy_probe

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ItemChange, RectItem, SceneModel};

/// WRITE from an observer — the documented snap-to-grid shape.
#[test]
fn observer_may_write_the_scene_back() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);

    let writer = model.clone();
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            let snapped = Point::new((new.x / 25.0).round() * 25.0, (new.y / 25.0).round() * 25.0);
            if snapped != new {
                writer.set_local_pos(id, snapped);
            }
        }
    });

    model.set_local_pos(id, Point::new(23.0, 27.0));

    assert_eq!(
        model.local_pos(id),
        Some(Point::new(25.0, 25.0)),
        "snap observer should have rounded the position onto the grid"
    );
}

/// READ from an observer — "validation" needs at least this much.
#[test]
fn observer_may_read_the_scene() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);

    let reader = model.clone();
    let seen: Rc<RefCell<Option<Rect>>> = Rc::new(RefCell::new(None));
    let sink = seen.clone();
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, .. } = *change {
            // A validator asking "where did it land in scene space?"
            *sink.borrow_mut() = reader.scene_rect(id);
        }
    });

    model.set_local_pos(id, Point::new(40.0, 40.0));

    assert!(
        seen.borrow().is_some(),
        "an observer should be able to read back the scene it was notified about"
    );
}

/// The workaround: record in the observer, apply after the mutation returns.
/// If this passes while the two above fail, the extension point is
/// "observe-then-defer" only, and the docs' wording is what needs fixing.
#[test]
fn observer_may_defer_the_write_to_after_the_mutation() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);

    let pending: Rc<RefCell<Vec<(teksilo_scene::ItemId, Point)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let queue = pending.clone();
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            let snapped = Point::new((new.x / 25.0).round() * 25.0, (new.y / 25.0).round() * 25.0);
            if snapped != new {
                queue.borrow_mut().push((id, snapped));
            }
        }
    });

    model.set_local_pos(id, Point::new(23.0, 27.0));

    // Drain outside the mutation.
    for (id, p) in pending.borrow_mut().drain(..) {
        model.set_local_pos(id, p);
    }

    assert_eq!(model.local_pos(id), Some(Point::new(25.0, 25.0)));
}
