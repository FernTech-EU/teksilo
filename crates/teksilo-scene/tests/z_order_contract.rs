// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `z_between` and `set_z` have to agree about what "the same `z`" means.
//!
//! `z_between` exists so a caller is never told "there is room between these
//! two" and then gets a silent no-op — its documentation says so, and offers
//! renumbering the band as the answer to `None`. That promise holds only while
//! the two use one metric for equality. They used not to: `z_between` bisects
//! **relatively** (`mid > lo && mid < hi`) and `set_z` guarded **absolutely**
//! (`|old - z| < f32::EPSILON`), and near zero the absolute guard swallows
//! millions of distinct floats the relative test is happy to hand out.
//!
//! Run with: cargo test -p teksilo-scene --test z_order_contract

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_scene::{ItemChange, ItemId, RectItem, SceneModel};

fn square(model: &SceneModel) -> ItemId {
    model.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO)
}

/// Every `ZChanged` the scene emits, in order.
type ZLog = Rc<RefCell<Vec<(ItemId, f32, f32)>>>;

fn record_z(model: &SceneModel) -> (ZLog, teksilo_core::signal::ObserverHandle) {
    let log: ZLog = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let handle = model.item_change_signal().observe(move |c| {
        if let ItemChange::ZChanged { id, old, new } = c.change {
            sink.borrow_mut().push((id, old, new));
        }
    });
    (log, handle)
}

/// The exact reproduction. Near zero, `z_between` answers `Some` and the old
/// `set_z` threw the answer away: no change, **zero** `ZChanged` events, and
/// the ordering the caller asked for silently not applied.
///
/// Put the absolute `f32::EPSILON` guard back in `set_z` and this reddens on
/// the first three assertions.
#[test]
fn a_z_between_answer_is_always_a_z_set_z_applies() {
    let model = SceneModel::new();
    let lower = square(&model);
    let upper = square(&model);
    let middle = square(&model);
    let (log, _h) = record_z(&model);

    // A band that is tiny but perfectly representable.
    model.set_z(upper, 1.25e-7);
    assert_eq!(model.z(lower), Some(0.0));
    assert_eq!(
        model.z(upper),
        Some(1.25e-7),
        "a distinct float near zero is a distinct z, whatever f32::EPSILON says \
         about the spacing at 1.0"
    );

    let mid = model
        .z_between(lower, upper)
        .expect("there is room: 6.25e-8 sits strictly between 0.0 and 1.25e-7");
    assert!(mid > 0.0 && mid < 1.25e-7, "{mid}");

    model.set_z(middle, mid);
    assert_eq!(
        model.z(middle),
        Some(mid),
        "z_between said there was room, so set_z must put the item there"
    );

    // And it is observable: an app mirroring z-order to a data layer learns of
    // it, rather than the scene and the mirror silently diverging.
    let events = log.borrow();
    assert_eq!(
        events.len(),
        2,
        "one for `upper`, one for `middle`: {events:?}"
    );
    assert_eq!(events[1], (middle, 0.0, mid));

    // The point of the whole exercise: the paint order is what was asked for.
    assert!(model.z(lower).unwrap() < model.z(middle).unwrap());
    assert!(model.z(middle).unwrap() < model.z(upper).unwrap());
}

/// The contract generalised: bisect a band down to exhaustion and check that
/// **every** `Some` is applied and the `None` is the real end of the road.
#[test]
fn bisecting_a_band_to_exhaustion_never_lies() {
    let model = SceneModel::new();
    let lower = square(&model);
    let upper = square(&model);
    model.set_z(lower, 0.0);
    model.set_z(upper, 1.0);

    let mut inserted = 0usize;
    let mut top = upper;
    while let Some(mid) = model.z_between(lower, top) {
        let item = square(&model);
        model.set_z(item, mid);
        assert_eq!(
            model.z(item),
            Some(mid),
            "bisection {inserted}: z_between offered {mid} and set_z refused it"
        );
        assert!(model.z(lower).unwrap() < mid && mid < model.z(top).unwrap());
        top = item;
        inserted += 1;
        assert!(inserted < 200, "an f32 band cannot bisect this many times");
    }
    // ~24 mantissa bits, so a couple of dozen bisections before the band is
    // genuinely exhausted — the number the doc quotes.
    assert!(
        inserted > 20,
        "only {inserted} bisections before z_between gave up — the band ran out \
         far earlier than f32 precision does"
    );
}

/// The no-op that *should* still be a no-op: writing the value the entry
/// already holds changes nothing and announces nothing.
#[test]
fn writing_the_same_z_is_still_silent() {
    let model = SceneModel::new();
    let id = square(&model);
    model.set_z(id, 4.0);
    let (log, _h) = record_z(&model);
    model.set_z(id, 4.0);
    model.set_z(id, 4.0);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

/// A band with no room still answers `None`, so the documented remedy
/// (renumber and bisect again) still has something to trigger on.
#[test]
fn an_exhausted_or_empty_band_still_answers_none() {
    let model = SceneModel::new();
    let a = square(&model);
    let b = square(&model);
    // Same z: no strict midpoint exists.
    assert_eq!(model.z_between(a, b), None);
    // An item against itself, likewise.
    assert_eq!(model.z_between(a, a), None);
    // Adjacent floats: nothing fits between them.
    model.set_z(b, f32::from_bits(1.0_f32.to_bits() + 1));
    model.set_z(a, 1.0);
    assert_eq!(model.z_between(a, b), None);
}
