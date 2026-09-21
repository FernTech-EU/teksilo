// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What happens when a geometry constraint hands back a frame that cannot be
//! applied?
//!
//! `ChangeVerdict::Adjust` carries a consumer-authored `TransformFrame`, and
//! the arithmetic that produces one is easy to get wrong in a way no type
//! catches. The documented snap closure is
//! `f.rect.x = (f.rect.x / grid).round() * grid` — with `grid` left at `0.0`
//! that is `NaN`, and a `NaN` applied verbatim gives the item a `NaN`
//! `local_pos`, a `(inf, inf, -inf, -inf)` scene rect, and a permanent absence
//! from hit-testing, from the marquee and from every spatial query. No panic,
//! no event, no way back: the item is in the document and reachable by nothing.
//!
//! So an inapplicable `Adjust` is **refused** — the sample behaves as
//! `ChangeVerdict::Reject` — and a debug build panics naming the frame. Both
//! halves are checked here, each in the profile it applies to, so
//! `cargo test` and `cargo test --release` each prove their own.

use teksilo_canvas::{Point, Rect, Vec2};
use teksilo_scene::{ChangeVerdict, ItemId, RectItem, SceneModel, TransformOp, TransformSource};

fn one_square() -> (SceneModel, ItemId) {
    let model = SceneModel::new();
    let id = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)),
        Point::new(50.0, 50.0),
    );
    (model, id)
}

/// Run `f`, returning the panic message when it panicked, with the default
/// hook muted so a deliberate panic does not litter the test output.
fn caught(f: impl FnOnce()) -> Option<String> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(hook);
    match out {
        Ok(()) => None,
        Err(payload) => Some(
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default(),
        ),
    }
}

/// Everything the scene can be asked about the item, in one answer: is it still
/// where a user could find it?
fn reachable(model: &SceneModel, id: ItemId) -> bool {
    let whole_world = Rect::new(-1.0e6, -1.0e6, 2.0e6, 2.0e6);
    model.items_in_rect(whole_world).contains(&id)
        && model.scene_rect(id).is_some_and(|r| r.width >= 0.0)
        && model.local_pos(id).is_some_and(|p| p.x.is_finite())
}

/// The one-character app bug, end to end through the public app-driven door.
///
/// Delete the `frame_is_applicable` check in `constrain.rs` and this reddens in
/// both profiles: in debug there is no longer a panic to catch, and in release
/// the item vanishes from every query.
#[test]
fn a_nan_frame_from_a_zero_grid_does_not_lose_the_item() {
    let (model, id) = one_square();
    let grid = 0.0_f32; // the bug
    model.set_geometry_constraint(move |c| {
        let mut f = c.proposed;
        f.rect.x = (f.rect.x / grid).round() * grid;
        f.rect.y = (f.rect.y / grid).round() * grid;
        ChangeVerdict::Adjust(f)
    });

    let start = model.transform_frame(&[id]).expect("resolves");
    assert!(reachable(&model, id));

    if cfg!(debug_assertions) {
        let message = caught(|| {
            let _ = model.constrain_move(
                &[id],
                start,
                Vec2::new(31.0, 12.0),
                TransformSource::Pointer,
            );
        })
        .expect("a debug build must name the broken policy rather than apply it");
        assert!(
            message.contains("cannot be applied"),
            "the panic must name the hook and the fix, got: {message}"
        );
        assert!(
            message.contains("grid"),
            "the panic must name the usual cause, got: {message}"
        );
    } else {
        let applied = model.constrain_move(
            &[id],
            start,
            Vec2::new(31.0, 12.0),
            TransformSource::Pointer,
        );
        assert_eq!(
            applied,
            Vec2::ZERO,
            "an inapplicable frame is refused, which is a zero translation"
        );
        model.set_local_pos(id, Point::new(50.0 + applied.x, 50.0 + applied.y));
    }

    assert!(
        reachable(&model, id),
        "the item must still be in the spatial index, still have a finite \
         position and still have a non-inverted scene rect"
    );
}

/// A mirrored resize — extents driven negative — is inapplicable for the same
/// reason: it inverts the item's AABB.
#[test]
fn a_negative_extent_frame_is_refused() {
    let (model, id) = one_square();
    model.set_geometry_constraint(|c| {
        let mut f = c.proposed;
        f.rect.width = -f.rect.width;
        ChangeVerdict::Adjust(f)
    });
    let start = model.transform_frame(&[id]).expect("resolves");
    // Field assignment rather than a struct literal, on purpose: `TransformFrame`
    // crosses the constraint hook's boundary, so it should be able to grow a
    // field without breaking every consumer that derives a proposal from the
    // start frame. A literal here would be the thing that stops it.
    let mut proposed = start;
    proposed.rect = Rect::new(60.0, 60.0, 40.0, 40.0);

    if cfg!(debug_assertions) {
        let message = caught(|| {
            let _ = model.constrain_frame(
                &[id],
                TransformOp::Resize,
                start,
                proposed,
                TransformSource::Pointer,
            );
        })
        .expect("a negative extent must be named, not stored");
        assert!(
            message.contains("neither extent may be negative"),
            "{message}"
        );
    } else {
        let out = model.constrain_frame(
            &[id],
            TransformOp::Resize,
            start,
            proposed,
            TransformSource::Pointer,
        );
        assert_eq!(out, start, "refused, so the gesture keeps its start frame");
    }
}

/// A **zero** extent is not an error. The resize path clamps at `min_size`
/// rather than mirroring, and that floor lives in the controller rather than
/// in this validation, so a frame collapsed to a point is a legitimate end of
/// a drag and must go through.
#[test]
fn a_zero_extent_frame_is_still_applied() {
    let (model, id) = one_square();
    model.set_geometry_constraint(|c| {
        let mut f = c.proposed;
        f.rect.width = 0.0;
        f.rect.height = 0.0;
        ChangeVerdict::Adjust(f)
    });
    let start = model.transform_frame(&[id]).expect("resolves");
    let out = model.constrain_frame(
        &[id],
        TransformOp::Resize,
        start,
        start,
        TransformSource::Pointer,
    );
    assert_eq!(out.rect.width, 0.0);
    assert_eq!(out.rect.height, 0.0);
}

/// A non-finite **rotation** is inapplicable too — it poisons the whole
/// composed basis, not just the origin.
#[test]
fn a_non_finite_rotation_is_refused() {
    let (model, id) = one_square();
    model.set_geometry_constraint(|c| {
        let mut f = c.proposed;
        f.rotation = f32::INFINITY;
        ChangeVerdict::Adjust(f)
    });
    let start = model.transform_frame(&[id]).expect("resolves");
    let call = || {
        model.constrain_frame(
            &[id],
            TransformOp::Rotate,
            start,
            start,
            TransformSource::Keyboard,
        )
    };
    if cfg!(debug_assertions) {
        assert!(
            caught(|| {
                let _ = call();
            })
            .is_some()
        );
    } else {
        assert_eq!(call(), start);
    }
}

/// `ChangeVerdict::Accept` and `ChangeVerdict::Reject` are not validated —
/// both hand back a frame the framework built, so there is nothing
/// consumer-authored in them to check, and adding a check would charge every
/// unadjusted sample for a mistake it cannot make.
#[test]
fn accept_and_reject_are_not_second_guessed() {
    let (model, id) = one_square();
    model.set_geometry_constraint(|_| ChangeVerdict::Accept);
    let start = model.transform_frame(&[id]).expect("resolves");
    let applied = model.constrain_move(&[id], start, Vec2::new(7.0, 9.0), TransformSource::Pointer);
    assert_eq!(applied, Vec2::new(7.0, 9.0));

    model.set_geometry_constraint(|_| ChangeVerdict::Reject);
    let applied = model.constrain_move(&[id], start, Vec2::new(7.0, 9.0), TransformSource::Pointer);
    assert_eq!(applied, Vec2::ZERO);
}
