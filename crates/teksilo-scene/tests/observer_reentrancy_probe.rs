// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! REGRESSION (was a probe): what an `item_change_signal` observer may do.
//!
//! `docs/teksilo-scene.md` advertises the signal as the place to wire
//! "validation, persistence" and the only `&self` door back into the scene is
//! [`SceneModel`], which used to hold `borrow_mut()` across the mutation —
//! including the synchronous observer fan-out in `Scene::emit_item_change`. So
//! an observer that read the scene panicked at `scene_model.rs` with "RefCell
//! already mutably borrowed", and one that wrote it back panicked with "RefCell
//! already borrowed". Two of the three tests below were written as failing
//! probes to characterise exactly that.
//!
//! They pass now. `SceneModel` mutators run inside a **write scope**: the
//! notification queues in the `Scene` and fans out once the model has released
//! its borrow. An observer may read the scene, and write it back. The
//! mechanics — ordering, re-entrancy, termination, unwind safety — are pinned
//! in `deferred_change_fanout.rs`; this file pins the promise the docs make.
//!
//! Run with: cargo test -p teksilo-scene --test observer_reentrancy_probe

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_scene::{A11yNode, ItemChange, RectItem, SceneModel};

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

/// Record in the observer, apply after the mutation returns. This was the only
/// shape that worked while the two above panicked, so it is the one apps in the
/// wild will have written; it must keep working now that they no longer have to.
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

/// The **second** channel. `a11y_change_signal` has the same shape as
/// `item_change_signal` and is fired from the same kind of `&mut self` method,
/// so an observer on it trapped identically — a fact the original probe (and
/// the spike that specified the fix) both missed. A validator that re-checks
/// the AT structure after a landmark is declared needs at least a read.
#[test]
fn a11y_observer_may_read_and_write_the_scene() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);

    let reader = model.clone();
    let seen: Rc<RefCell<Option<Rect>>> = Rc::new(RefCell::new(None));
    let sink = seen.clone();
    let _h = model.a11y_change_signal().observe(move |_| {
        // Read...
        *sink.borrow_mut() = reader.scene_rect(id);
        // ...and write back, which is what a "keep the geometry in step with
        // the AT structure" observer would do.
        reader.set_local_pos(id, Point::new(7.0, 7.0));
    });

    model.set_a11y_landmark(A11yNode::Item(id), accesskit::Role::Region);

    assert!(
        seen.borrow().is_some(),
        "an a11y-channel observer should be able to read the scene it was notified about"
    );
    assert_eq!(
        model.local_pos(id),
        Some(Point::new(7.0, 7.0)),
        "an a11y-channel observer should be able to write the scene back"
    );
}
