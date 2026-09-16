// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Does installing a geometry constraint keep the scene alive for ever?
//!
//! The scene **owns** the constraint closure, so a closure that owns a
//! `SceneModel` back closes the ring
//! `SceneModel → RefCell<Scene> → geometry_constraint → SceneModel` and nothing
//! in it is ever dropped — every item, every heavyweight payload, the journal
//! and the spatial index, for the life of the process. The scene keeps working
//! perfectly, which is what makes it worth a test rather than a comment.
//!
//! Every test here hangs a `Drop` sentinel off a heavyweight payload, which is
//! the one thing in the scene that is *observably* destroyed when the scene is.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ChangeVerdict, RectItem, SceneModel};

/// Sets its flag when it is dropped. Parked in the scene as a heavyweight
/// payload, so "the sentinel dropped" means "the scene dropped".
struct DropSentinel(Rc<Cell<bool>>);

impl Drop for DropSentinel {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

/// A model holding one lightweight square and one heavyweight sentinel payload.
fn scene_with_sentinel() -> (SceneModel, Rc<Cell<bool>>) {
    let model = SceneModel::new();
    model.add_item(RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)), Point::ZERO);
    let flag = Rc::new(Cell::new(false));
    model.add_widget_item(DropSentinel(flag.clone()), Rect::new(0.0, 0.0, 10.0, 10.0));
    (model, flag)
}

/// The form the documentation teaches: read through the `&Scene` the hook is
/// handed. Nothing is captured, so nothing is kept alive.
///
/// Make the closure capture `model.clone()` instead and this reddens.
#[test]
fn a_constraint_reading_the_handed_scene_lets_the_scene_drop() {
    let (model, dropped) = scene_with_sentinel();
    model.set_geometry_constraint(|c| {
        // The whole read surface, and no handle held.
        if c.scene.is_empty() {
            return ChangeVerdict::Reject;
        }
        ChangeVerdict::Accept
    });
    assert!(model.has_geometry_constraint());
    assert!(!dropped.get(), "still alive while the model is held");

    drop(model);
    assert!(
        dropped.get(),
        "the scene was not dropped when its last handle went away — something \
         inside it is holding a SceneModel. The geometry constraint is the one \
         closure the scene owns, so that is where to look: read through \
         ProposedChange::scene, or capture a WeakSceneModel."
    );
}

/// The escape hatch for a policy object that genuinely holds a model for its
/// other work: capture the weak handle and upgrade inside the call.
///
/// Turn `model.downgrade()` into `model.clone()` and this reddens.
#[test]
fn a_constraint_capturing_a_weak_handle_lets_the_scene_drop() {
    let (model, dropped) = scene_with_sentinel();
    let weak = model.downgrade();
    let saw_scene = Rc::new(Cell::new(false));
    let probe = saw_scene.clone();
    model.set_geometry_constraint(move |_c| match weak.upgrade() {
        Some(m) => {
            probe.set(m.len() == 2);
            ChangeVerdict::Accept
        }
        // The scene outlived by its own constraint is a contradiction: with a
        // weak handle it simply cannot happen, which is the point.
        None => ChangeVerdict::Reject,
    });

    // It really does read the scene — a weak handle that never upgrades would
    // pass the drop assertion below for the wrong reason.
    let a = model.ids()[0];
    let start = model.transform_frame(&[a]).expect("resolves");
    let _ = model.constrain_frame(
        &[a],
        teksilo_scene::TransformOp::Move,
        start,
        start,
        teksilo_scene::TransformSource::Pointer,
    );
    assert!(
        saw_scene.get(),
        "the constraint upgraded and read the scene"
    );

    drop(model);
    assert!(
        dropped.get(),
        "a WeakSceneModel captured by a constraint must not keep the scene alive"
    );
}

/// The hazard itself, pinned so it is a measured fact rather than an opinion:
/// a constraint that captures a **strong** handle leaks the whole scene.
///
/// This is the one shape the crate's documentation forbids
/// (`crate::constrain`'s module header, `ProposedChange::scene`,
/// `SceneModel::downgrade`). The framework cannot refuse it — a `'static`
/// closure may capture any `Rc`, and the scene has to own the closure because
/// the closure's lifetime is the document's — so the answer is a rule plus this
/// test, which says exactly what the rule costs when it is broken.
///
/// **If a future design removes the hazard, delete this test with it.** An
/// assertion that a leak still happens is worth keeping only while the leak is
/// the documented consequence of a documented mistake.
#[test]
fn a_constraint_capturing_a_strong_handle_leaks_the_scene() {
    let (model, dropped) = scene_with_sentinel();
    let captured = model.clone();
    model.set_geometry_constraint(move |c| {
        let _ = captured.local_pos(c.items[0]);
        ChangeVerdict::Accept
    });

    drop(model);
    assert!(
        !dropped.get(),
        "a strong capture no longer leaks — good. Delete this test and the \
         warnings it backs (constrain.rs's module header, ProposedChange::scene, \
         SceneModel::downgrade)."
    );
}

/// Clearing the constraint releases the closure, and with it whatever the
/// closure captured — so an app that got it wrong has a way out that does not
/// involve restarting the process.
#[test]
fn clearing_the_constraint_releases_what_it_captured() {
    let (model, dropped) = scene_with_sentinel();
    let captured = model.clone();
    model.set_geometry_constraint(move |c| {
        let _ = captured.local_pos(c.items[0]);
        ChangeVerdict::Accept
    });
    // One handle in the closure, one here.
    assert_eq!(model.handle_count(), 2);

    model.clear_geometry_constraint();
    assert_eq!(model.handle_count(), 1);
    assert!(!model.has_geometry_constraint());

    drop(model);
    assert!(dropped.get(), "the cycle was broken, so the scene dropped");
}
