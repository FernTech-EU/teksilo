// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The mechanics of the deferred change fan-out.
//!
//! `observer_reentrancy_probe.rs` pins the *promise* — an observer may read the
//! scene and write it back. This file pins the machinery that promise rests on,
//! and in particular the ways it could fail **silently**, which is the one
//! failure mode worse than the panic it replaced:
//!
//! - an observer that panics mid-drain must not leave the drain flag set, or
//!   the scene stops notifying anything, for ever, with no diagnostic;
//! - nor may it cost the rest of the batch: every notification it did not reach
//!   has to still be deliverable, or the scene advances with no notification and
//!   the views disagree with the model permanently;
//! - a panic inside a write scope must release the `RefCell` borrow, and the
//!   batch it abandoned must still be deliverable;
//! - a runaway observer feedback loop must announce itself in **every** build
//!   profile rather than freezing the thread or quietly dropping the
//!   notifications that would have settled it;
//! - `flush_changes` is the recovery door all of the above point at, so it must
//!   never be the thing that panics.
//!
//! Plus the semantics the design had to choose and therefore has to state:
//! ordering (including across the seam between the deferred and the synchronous
//! door), re-entrant queueing, coalescing, what a "round" is, and what happens
//! to a queue whose scene is dropped.
//!
//! Run with: cargo test -p teksilo-scene --test deferred_change_fanout

use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;

use accesskit::{NodeId, Role, TreeUpdate};
use teksilo_canvas::{MockTextBackend, Point, Rect, SizeProposal};
use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
use teksilo_core::signal::ObserverHandle;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_scene::{
    A11yNode, CascadeBudget, ItemChange, ItemId, Magnet, MagnetId, RectItem, Scene, SceneModel,
    SceneView, TextItem,
};

fn unit_rect() -> Rect {
    Rect::new(0.0, 0.0, 10.0, 10.0)
}

/// One entry in a recorded notification log. Both channels land in one log so
/// their relative order is observable.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Note {
    Moved(ItemId),
    MovedTo(ItemId, i32, i32),
    Removed(ItemId),
    /// `Scene::set_visible` emits this **and then** `FlagsChanged`, so one
    /// mutator call is two notifications — a free second data point for FIFO
    /// wherever a test uses it.
    VisibilityChanged(ItemId),
    FlagsChanged(ItemId),
    Other,
    A11y,
}

impl From<ItemChange> for Note {
    fn from(c: ItemChange) -> Self {
        match c {
            ItemChange::LocalPosChanged { id, .. } => Note::Moved(id),
            ItemChange::Removed { id } => Note::Removed(id),
            ItemChange::VisibilityChanged { id, .. } => Note::VisibilityChanged(id),
            ItemChange::FlagsChanged { id, .. } => Note::FlagsChanged(id),
            _ => Note::Other,
        }
    }
}

/// The same, but keeping the destination, for the tests that care *where* an
/// item was reported to have gone and not just that it moved.
fn note_with_target(c: ItemChange) -> Note {
    match c {
        ItemChange::LocalPosChanged { id, new, .. } => {
            Note::MovedTo(id, new.x as i32, new.y as i32)
        }
        other => Note::from(other),
    }
}

type Log = Rc<RefCell<Vec<Note>>>;

/// Record every notification from **both** channels into one ordered log.
/// Returns the log and the observer handles, which must be kept alive.
fn record(model: &SceneModel) -> (Log, [ObserverHandle; 2]) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let items = log.clone();
    let a11y = log.clone();
    let h_item = model
        .item_change_signal()
        .observe(move |c| items.borrow_mut().push(Note::from(*c)));
    let h_a11y = model
        .a11y_change_signal()
        .observe(move |_| a11y.borrow_mut().push(Note::A11y));
    (log, [h_item, h_a11y])
}

/// Run `f` with the panic hook silenced, so a deliberately-panicking test does
/// not spray a backtrace over the suite's output.
fn hushed<R>(f: impl FnOnce() -> R) -> std::thread::Result<R> {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let outcome = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(previous);
    outcome
}

fn panic_message(err: &(dyn std::any::Any + Send)) -> String {
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Silent-failure modes
// ---------------------------------------------------------------------------

/// THE regression that makes this design safe to ship. An observer that panics
/// runs *inside* the drain loop, which claims a drain flag on entry so a
/// re-entrant flush collapses into the outer one. Clear that flag at the tail
/// of the loop and a panicking observer skips the clear — after which every
/// later flush returns immediately and the scene stops notifying anything,
/// permanently and silently. That is strictly worse than the `RefCell` panic
/// this whole mechanism replaced, so the flag is cleared by an RAII guard whose
/// `Drop` survives the unwind.
#[test]
fn a_panicking_observer_does_not_stop_later_notification() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    // Observer A explodes exactly once, then behaves.
    let armed = Rc::new(Cell::new(true));
    let fuse = armed.clone();
    let _boom = model.item_change_signal().observe(move |_| {
        if fuse.replace(false) {
            panic!("observer exploded");
        }
    });
    // Observer B is the witness: it must still be reachable afterwards.
    let seen = Rc::new(Cell::new(0_u32));
    let counter = seen.clone();
    let _witness = model
        .item_change_signal()
        .observe(move |_| counter.set(counter.get() + 1));

    let outcome = hushed(|| model.set_local_pos(id, Point::new(1.0, 1.0)));
    assert!(outcome.is_err(), "the observer's panic must propagate");
    assert_eq!(
        seen.get(),
        0,
        "B sits downstream of the exploding observer on the signal's own \
         callback list, so it misses *that one* notification — the panic costs \
         one delivery, which is the whole of what it is allowed to cost \
         (`a_panicking_observer_costs_only_its_own_delivery` pins the rest)"
    );

    // The scene must still be usable, and still notifying.
    model.set_local_pos(id, Point::new(2.0, 2.0));
    assert_eq!(
        seen.get(),
        1,
        "a panicking observer left the drain flag set: the scene has gone \
         permanently silent"
    );
    assert_eq!(model.local_pos(id), Some(Point::new(2.0, 2.0)));
}

/// The batch **tail** survives a panicking observer.
///
/// A drain that lifted its batch out of the queue into a local `Vec` and
/// iterated `batch.drain(..)` would have the `Drain` destructor discard the
/// untouched tail during the unwind — and those notifications are no longer in
/// the scene's queue either, so nothing can redeliver them. The scene would
/// have moved three items and told its observers about one, permanently: the
/// exact corruption this mechanism's termination policy exists to avoid, and
/// the one the suite could not see while every batch it tested had a single
/// element. So the drain pops one notification at a time, immediately before
/// delivering it, and never parks the rest anywhere an unwind can reach.
#[test]
fn a_panicking_observer_costs_only_its_own_delivery() {
    let model = SceneModel::new();
    let a = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let b = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let c = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    let armed = Rc::new(Cell::new(true));
    let fuse = armed.clone();
    let _boom = model.item_change_signal().observe(move |_| {
        if fuse.replace(false) {
            panic!("observer exploded on the first of three");
        }
    });
    let (log, _handles) = record(&model);

    // A three-change batch, so there is a tail to lose.
    let editing = model.clone();
    let outcome = hushed(move || {
        let mut scene = editing.write_guard();
        scene.set_local_pos(a, Point::new(1.0, 1.0));
        scene.set_local_pos(b, Point::new(2.0, 2.0));
        scene.set_local_pos(c, Point::new(3.0, 3.0));
    });
    assert!(outcome.is_err(), "the observer's panic must propagate");
    assert!(
        log.borrow().is_empty(),
        "the witness is downstream of the exploding observer, so it sees \
         nothing of the notification that panicked: {:?}",
        log.borrow()
    );

    // All three moves are in the scene…
    assert_eq!(model.local_pos(a), Some(Point::new(1.0, 1.0)));
    assert_eq!(model.local_pos(b), Some(Point::new(2.0, 2.0)));
    assert_eq!(model.local_pos(c), Some(Point::new(3.0, 3.0)));

    // …so all three must be *reportable*. One delivery was spent on the panic;
    // the other two are still queued, in order.
    model.flush_changes();
    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(b), Note::Moved(c)],
        "the batch tail was discarded by the unwind: the scene advanced with \
         no notification and nothing can redeliver it"
    );
}

/// A panic inside an open write scope must release the `RefCell` borrow (or
/// every later mutation panics with "already mutably borrowed"), and must not
/// run observers from `Drop` while unwinding (a second panic there aborts the
/// process). The batch it abandoned survives in the queue and is delivered by
/// the next flush — which is what `SceneModel::flush_changes` is public for.
#[test]
fn a_panic_inside_a_write_scope_releases_the_borrow_and_keeps_the_batch() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let other = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    let editing = model.clone();
    let outcome = hushed(move || {
        let mut scene = editing.write_guard();
        scene.set_local_pos(id, Point::new(5.0, 5.0));
        scene.set_visible(other, false);
        panic!("edit exploded");
    });
    assert!(outcome.is_err());
    assert!(
        log.borrow().is_empty(),
        "an unwinding guard must not run observers from its Drop"
    );

    // The borrow is free — a read proves it without mutating.
    assert_eq!(model.local_pos(id), Some(Point::new(5.0, 5.0)));

    model.flush_changes();
    assert_eq!(
        *log.borrow(),
        vec![
            Note::Moved(id),
            Note::VisibilityChanged(other),
            Note::FlagsChanged(other)
        ],
        "the whole abandoned batch must be deliverable by the next flush, in \
         emission order"
    );

    // …and the scene keeps working, still batching: the write scope was closed
    // by the unwind, not stranded open.
    log.borrow_mut().clear();
    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(6.0, 6.0));
        assert!(
            log.borrow().is_empty(),
            "the write scope depth leaked: nothing is deferring any more"
        );
    }
    assert_eq!(*log.borrow(), vec![Note::Moved(id)]);
}

/// A panic out of a caller's closure inside a mutator is the same hazard by a
/// different door: `with_handlers_mut` runs app code while the scene is
/// exclusively borrowed.
#[test]
fn a_panic_in_with_handlers_mut_releases_the_borrow() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    let editing = model.clone();
    let outcome =
        hushed(move || editing.with_handlers_mut(id, |_| panic!("handler edit exploded")));
    assert!(outcome.is_err());

    model.set_local_pos(id, Point::new(3.0, 3.0));
    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(id)],
        "a panicking handler closure stranded the scene"
    );

    // The borrow came back *and* so did the write-scope depth: a leaked depth
    // would leave every later change queued for a drain that never comes, which
    // a single delivered notification above cannot distinguish from health.
    log.borrow_mut().clear();
    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(4.0, 4.0));
        assert!(log.borrow().is_empty(), "the write scope is not deferring");
    }
    assert_eq!(*log.borrow(), vec![Note::Moved(id)]);
}

/// Pull an unsigned number out of the runaway diagnostic, immediately after
/// `marker`.
///
/// The diagnostic's numbers are the whole point of it — they are what separates
/// a write-back piling onto one subject from a spawner that repeats none — so
/// the tests read them back rather than matching prose around them.
fn number_after(message: &str, marker: &str) -> u64 {
    let (_, rest) = message
        .split_once(marker)
        .unwrap_or_else(|| panic!("`{marker}` missing from the diagnostic: {message}"));
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("no number after `{marker}` in the diagnostic: {message}"))
}

/// An observer that writes on **every** change with no equality guard never
/// settles. The drain deliberately keeps going rather than dropping the
/// remainder — losing notifications would leave the scene and its views
/// permanently disagreeing about geometry — so the deliveries are capped and the
/// diagnostic names the cause.
///
/// The cap is **not** `#[cfg(debug_assertions)]`, and this test is not either.
/// The tempting analogy to `Signal::try_set`'s debug-only depth guard is wrong
/// in the one way that matters: `try_set` *recurses*, so an unchecked loop
/// there exhausts the stack and aborts loudly on its own. This drain is an
/// iterative loop, so an unchecked loop here returns never and dies never — a
/// frozen UI thread with no diagnostic and no core dump.
///
/// The diagnostic has to name the **item**, because that is the thing an app
/// author can then go and look at. The bound that trips is a flat total, which
/// knows nothing about subjects; the per-subject counts kept alongside it are
/// what turn "something did not settle" into "*this* did not settle", and this
/// test pins that they survive being demoted to diagnostics.
#[test]
fn a_runaway_observer_trips_the_cap_and_names_the_item() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    let writer = model.clone();
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            // No equality guard, and a value that never repeats: each round
            // produces exactly one more, always about the same item.
            writer.set_local_pos(id, Point::new(new.x + 1.0, new.y));
        }
    });

    let outcome = hushed(|| model.set_local_pos(id, Point::new(1.0, 0.0)));
    let err = outcome.expect_err("an unbounded feedback loop must not spin");
    let message = panic_message(&*err);
    assert!(
        message.contains("did not settle"),
        "the diagnostic must say what went wrong, got: {message}"
    );
    assert!(
        message.contains(&format!("{id:?}")),
        "the diagnostic must name the offending item ({id:?}) - pointing at the \
         observer's subject is most of what it is worth, and is exactly what a \
         round counter could not do. Got: {message}"
    );
    assert!(
        message.contains("LocalPosChanged"),
        "...and the kind of change it kept producing, got: {message}"
    );

    let budget = CascadeBudget::default().total;
    assert_eq!(
        number_after(&message, "budget of "),
        budget,
        "the diagnostic must quote the bound this scene enforced, got: {message}"
    );
    let delivered = number_after(&message, "have generated ");
    assert_eq!(
        delivered,
        budget + 1,
        "...and the count that broke it, which is the bound plus the one \
         delivery that passed it - not the bound itself. Got: {message}"
    );
    assert_eq!(
        number_after(&message, ", with "),
        delivered,
        "every delivery here was about the same item, so the most-charged \
         subject must account for all of them. A count near 1 is the *other* \
         runaway shape and must read differently. Got: {message}"
    );
}

/// THE regression this budget exists for, and the case the round cap got wrong.
///
/// A chain of 5000 items with a "when a node moves, drag the next one along"
/// observer - guarded exactly as this crate's own diagnostic and docs instruct.
/// Every link fires **once**, so the cascade settles; it just takes one round
/// per link, because a round is what was queued when it began. A cap on rounds
/// therefore caps how long a legitimate chain may be, and aborted the process
/// (in release too) at link 256 - an ordinary length for the node graphs,
/// timelines and CAD canvases this crate advertises.
///
/// Depth is exempt by construction under the flat total as well: a settling
/// chain costs one delivery per link, so a 5000-link chain spends 5000 of the
/// default 100 000 and the other 95 000 are still there for the AT work the
/// next test does on top.
#[test]
fn a_deep_guarded_cascade_settles_at_any_depth() {
    const LINKS: usize = 5000;

    let model = SceneModel::new();
    let ids: Vec<ItemId> = (0..LINKS)
        .map(|_| model.add_item(RectItem::new(unit_rect()), Point::ZERO))
        .collect();
    // A map, not `position()`: at this length a linear scan per link, not the
    // drain, would be what the test measures.
    let index: Rc<std::collections::HashMap<ItemId, usize>> = Rc::new(
        ids.iter()
            .copied()
            .enumerate()
            .map(|(n, id)| (id, n))
            .collect(),
    );

    let delivered = Rc::new(Cell::new(0_usize));
    let counted = delivered.clone();
    let writer = model.clone();
    let links = Rc::new(ids.clone());
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            counted.set(counted.get() + 1);
            let Some(&at) = index.get(&id) else {
                return;
            };
            let Some(next) = links.get(at + 1).copied() else {
                return; // end of the chain: the cascade stops here
            };
            let want = Point::new(new.x, new.y + 1.0);
            // The guard the crate's own diagnostic prescribes. It is what makes
            // this settle - and it is present, which is why tripping here was a
            // false positive and not a caught bug.
            if writer.local_pos(next) != Some(want) {
                writer.set_local_pos(next, want);
            }
        }
    });

    model.set_local_pos(ids[0], Point::new(50.0, 0.0));

    assert_eq!(
        model.local_pos(ids[LINKS - 1]),
        Some(Point::new(50.0, (LINKS - 1) as f32)),
        "the cascade must reach the end of the chain"
    );
    assert_eq!(
        model.local_pos(ids[LINKS / 2]),
        Some(Point::new(50.0, (LINKS / 2) as f32)),
        "...and every link on the way must have been dragged exactly once"
    );
    assert_eq!(
        delivered.get(),
        LINKS,
        "one delivery per link and no more - if this grows, the chain is not \
         settling and the test is measuring something else"
    );
    assert!(
        (delivered.get() as u64) < CascadeBudget::default().total,
        "a chain this long must sit well inside the default budget ({}), or \
         the bound is a cap on depth after all",
        CascadeBudget::default().total
    );
}

/// The same settling chain with the AT work a real node-graph does on a drag:
/// every node that moves re-places its port magnets and refreshes its logical-AT
/// structure. Each of those calls is an `a11y_change_signal` bump.
///
/// While the budget was a per-subject cap keyed on the whole logical-AT channel,
/// "keep AT in step per node" was the observer shape guaranteed to abort — the
/// identical chain that touched no AT at all passed. On a crate whose
/// differentiator is per-item accessibility, that was exactly the wrong channel
/// to bottleneck. Keying each bump to the node it is about fixed the
/// diagnostic's aim, and moving the trip condition onto a flat total removed the
/// bottleneck itself: the AT volume here is eleven times the item volume and
/// still spends under a quarter of the default budget.
#[test]
fn a_guarded_cascade_that_maintains_at_structure_per_node_settles() {
    const LINKS: usize = 2000;
    /// Port magnets per node, re-placed on every move.
    const PORTS: usize = 8;
    /// Structure calls per node, on top of the ports: parent, live, landmark.
    const STRUCTURE_CALLS: usize = 3;

    let model = SceneModel::new();
    let ids: Vec<ItemId> = (0..LINKS)
        .map(|_| model.add_item(RectItem::new(unit_rect()), Point::ZERO))
        .collect();
    let ports: Rc<std::collections::HashMap<ItemId, Vec<MagnetId>>> = Rc::new(
        ids.iter()
            .map(|id| {
                let handles = (0..PORTS)
                    .map(|p| model.add_magnet(*id, Magnet::new(Point::new(p as f32, 0.0))))
                    .collect();
                (*id, handles)
            })
            .collect(),
    );
    // A map, not `position()`: at this length a linear scan per link, not the
    // drain, would be what the test measures.
    let index: Rc<std::collections::HashMap<ItemId, usize>> = Rc::new(
        ids.iter()
            .copied()
            .enumerate()
            .map(|(n, id)| (id, n))
            .collect(),
    );

    let at_bumps = Rc::new(Cell::new(0_usize));
    let writer = model.clone();
    let links = Rc::new(ids.clone());
    let counted = at_bumps.clone();
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            let Some(&at) = index.get(&id) else {
                return;
            };
            // Re-place this node's ports and refresh its AT structure. All
            // idempotent, so none of it is what would fail to settle — and all
            // of it is on the *other* channel, so it never re-enters here.
            for (p, magnet) in ports[&id].iter().enumerate() {
                writer.set_magnet_local_pos(*magnet, Point::new(p as f32, new.y));
            }
            let node = A11yNode::Item(id);
            writer.set_a11y_parent(node, None);
            writer.set_a11y_live(node, accesskit::Live::Polite);
            writer.set_a11y_landmark(node, Role::Region);
            counted.set(counted.get() + PORTS + STRUCTURE_CALLS);

            let Some(next) = links.get(at + 1).copied() else {
                return; // end of the chain
            };
            let want = Point::new(new.x, new.y + 1.0);
            if writer.local_pos(next) != Some(want) {
                writer.set_local_pos(next, want);
            }
        }
    });

    model.set_local_pos(ids[0], Point::new(50.0, 0.0));

    assert_eq!(
        model.local_pos(ids[LINKS - 1]),
        Some(Point::new(50.0, (LINKS - 1) as f32)),
        "the cascade must reach the end of the chain"
    );
    assert_eq!(
        at_bumps.get(),
        LINKS * (PORTS + STRUCTURE_CALLS),
        "...and every node must have had its AT structure maintained on the way"
    );
    assert_eq!(
        model.a11y_parent_of(A11yNode::Item(ids[LINKS - 1])),
        None,
        "the AT structure the observer wrote must have landed"
    );

    // Both channels together, which is what the flat bound counts.
    let deliveries = (LINKS * (1 + PORTS + STRUCTURE_CALLS)) as u64;
    assert!(
        deliveries > 20_000,
        "this shape has to be big enough to be worth pinning; it is {deliveries}"
    );
    assert!(
        deliveries < CascadeBudget::default().total,
        "...and still inside the default budget ({}), or per-item \
         accessibility is back to being the bottleneck. It is {deliveries}",
        CascadeBudget::default().total
    );
}

/// Fan-in is a property of the *scene*, not a constant — which is why no
/// per-subject cap survives.
///
/// A hub that tracks the rightmost spoke: every spoke that moves recomputes the
/// hub's target and writes it **only if it differs**, which is the guard the
/// crate's own diagnostic prescribes. Half the spokes move somewhere that leaves
/// the target where it already is, so the guard genuinely suppresses — asserted
/// below, because a guard that cannot suppress is decoration and an earlier
/// version of this test had exactly that (`at != at + STEP`, always true).
///
/// The other half re-notify one subject 4200 times. Against a flat per-subject
/// 4096 that aborted the process; scaling the cap with the entry count rescued
/// it and made the abort cost scale with the scene instead. Under a flat total
/// the whole thing costs 4200 of 100 000 and the question does not arise.
#[test]
fn a_guarded_relaxation_with_a_wide_fan_in_settles() {
    /// Half push the hub, half are absorbed by the guard.
    const SPOKES: usize = 8400;
    /// Where the spokes that must *not* move the hub go. Below every pushing
    /// spoke's position, and not their starting point, so they still emit.
    const QUIET_X: f32 = 0.25;
    /// The flat per-subject constant this design replaced.
    const OLD_FLAT_CAP: usize = 4096;

    let model = SceneModel::new();
    let hub = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let spokes: Vec<ItemId> = (0..SPOKES)
        .map(|_| model.add_item(RectItem::new(unit_rect()), Point::ZERO))
        .collect();

    let writes = Rc::new(Cell::new(0_usize));
    let suppressed = Rc::new(Cell::new(0_usize));
    let wrote = writes.clone();
    let skipped = suppressed.clone();
    let writer = model.clone();
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            if id == hub {
                return; // the hub's own move is not an incident edge
            }
            let Some(at) = writer.local_pos(hub) else {
                return;
            };
            // The hub sits at the rightmost spoke seen so far.
            let want = Point::new(at.x.max(new.x), at.y);
            if writer.local_pos(hub) != Some(want) {
                writer.set_local_pos(hub, want);
                wrote.set(wrote.get() + 1);
            } else {
                skipped.set(skipped.get() + 1);
            }
        }
    });

    {
        let mut scene = model.write_guard();
        for (n, spoke) in spokes.iter().enumerate() {
            // Even spokes advance the rightmost position; odd ones land behind
            // it, so the guard absorbs them.
            let x = if n.is_multiple_of(2) {
                (n / 2 + 1) as f32
            } else {
                QUIET_X
            };
            scene.set_local_pos(*spoke, Point::new(x, 0.0));
        }
    }

    let pushing = SPOKES / 2;
    assert_eq!(
        model.local_pos(hub),
        Some(Point::new(pushing as f32, 0.0)),
        "the hub must end at the rightmost spoke"
    );
    assert_eq!(
        writes.get(),
        pushing,
        "every advancing spoke must have moved the hub"
    );
    assert_eq!(
        suppressed.get(),
        SPOKES - pushing,
        "...and every other spoke must have been absorbed by the guard. If this \
         is 0 the guard is decoration and the test proves nothing about guarded \
         cascades"
    );
    assert!(
        writes.get() > OLD_FLAT_CAP,
        "the hub has to be re-notified past the flat per-subject constant this \
         design replaced ({OLD_FLAT_CAP}), or the test would pass against it"
    );
}

/// An unguarded write-back on the logical-AT channel is still caught, and the
/// diagnostic still names the node — including one that is not a scene item at
/// all.
///
/// `A11yGroup` is the interesting key here: a virtual group is not a scene
/// entry, so it exercises the half of the subject key that has no `ItemId`
/// behind it. Without per-subject keying the message could only say "the AT
/// channel", which points at nothing an app author owns.
#[test]
fn an_unguarded_a11y_write_back_trips_and_names_its_node() {
    let model = SceneModel::new();
    let group = model.add_a11y_group(teksilo_scene::A11yGroup::builder().label(lit!("Act one")));

    let writer = model.clone();
    let _h = model.a11y_change_signal().observe(move |_| {
        // No equality guard: `set_a11y_landmark` bumps whether or not the role
        // changed, so this re-notifies the same node for ever.
        writer.set_a11y_landmark(A11yNode::Group(group), Role::Region);
    });

    let outcome = hushed(|| model.set_a11y_live(A11yNode::Group(group), accesskit::Live::Polite));
    let err = outcome.expect_err("an unbounded AT feedback loop must not spin");
    let message = panic_message(&*err);
    assert!(
        message.contains("did not settle"),
        "the diagnostic must say what went wrong, got: {message}"
    );
    assert!(
        message.contains(&format!("{group:?}")),
        "the diagnostic must name the offending node ({group:?}), not just \
         'the AT channel'. Got: {message}"
    );
    assert_eq!(
        number_after(&message, ", with "),
        number_after(&message, "have generated "),
        "one node took every delivery, so the most-charged count must say so \
         - that number is what tells this shape from a spawner. Got: {message}"
    );
}

/// The budget is a knob with a documented default, not a constant — because it
/// cannot decide the one question that would justify being one.
///
/// A guarded cascade that is genuinely enormous and an unguarded write-back are
/// the same picture from the queue's side. So the drain does not claim to tell
/// them apart: it states the bound it enforced, and an app whose graph genuinely
/// lives past it changes the bound instead of crashing. This pins both
/// directions — lowering it makes a settling cascade trip, and raising it lets
/// the same cascade through — and pins that the numbers in the message describe
/// the cascade that actually happened.
#[test]
fn the_cascade_budget_is_a_knob_in_both_directions() {
    const LINKS: usize = 300;
    /// Deliveries the chain below generates *after* the caller's own one.
    const CASCADE: u64 = LINKS as u64 - 1;
    const TIGHT: u64 = 100;

    let build = |budget: CascadeBudget| {
        let model = SceneModel::new();
        model.set_cascade_budget(budget);
        let ids: Vec<ItemId> = (0..LINKS)
            .map(|_| model.add_item(RectItem::new(unit_rect()), Point::ZERO))
            .collect();
        let writer = model.clone();
        let links = Rc::new(ids.clone());
        let handle = model.item_change_signal().observe(move |change| {
            if let ItemChange::LocalPosChanged { id, new, .. } = *change {
                let Some(at) = links.iter().position(|c| *c == id) else {
                    return;
                };
                let Some(next) = links.get(at + 1).copied() else {
                    return;
                };
                let want = Point::new(new.x, new.y + 1.0);
                if writer.local_pos(next) != Some(want) {
                    writer.set_local_pos(next, want);
                }
            }
        });
        (model, ids, handle)
    };

    // Squeezed under the cascade this scene runs: the cascade is unchanged and
    // still settles, but this scene has declared it will not pay for it.
    const { assert!(TIGHT < CASCADE) };
    let (model, ids, _h) = build(CascadeBudget::new(TIGHT));
    let outcome = hushed(|| model.set_local_pos(ids[0], Point::new(50.0, 0.0)));
    let err = outcome.expect_err("a cascade past the declared budget must trip");
    let message = panic_message(&*err);
    assert_eq!(
        number_after(&message, "budget of "),
        TIGHT,
        "the diagnostic must quote the bound it actually enforced, not a \
         constant. Got: {message}"
    );
    assert_eq!(
        number_after(&message, "have generated "),
        TIGHT + 1,
        "...and the delivery count that broke it. Quoting the bound in both \
         places is the off-by-one this assertion exists to catch. Got: {message}"
    );
    assert_eq!(
        number_after(&message, ", with "),
        1,
        "a settling chain charges each link once, so the most-charged subject \
         must read 1 - the number that says 'this is not a write-back'. Got: \
         {message}"
    );
    assert!(
        message.contains("likeliest cause"),
        "...must offer the equality guard as a likely cause rather than assert \
         it, since the queue cannot tell the two shapes apart. Got: {message}"
    );
    assert!(
        message.contains("set_cascade_budget"),
        "...and must say how to raise the bound, or an app with a genuinely \
         large cascade has no answer but a crash. Got: {message}"
    );

    // The same cascade, with room declared for it.
    let (model, ids, _h) = build(CascadeBudget::new(CASCADE + 1));
    model.set_local_pos(ids[0], Point::new(50.0, 0.0));
    assert_eq!(
        model.local_pos(ids[LINKS - 1]),
        Some(Point::new(50.0, (LINKS - 1) as f32)),
        "with the bound raised, the identical cascade settles"
    );
}

/// A budget of zero is rejected where it is written, not where it trips.
///
/// Stored as-is it would forbid the reactive write-back this channel is
/// advertised for, and its panic would have no cascade to describe — the
/// measured symptom was a message quoting a bound of 0 against a drain that had
/// delivered 1. Clamping at the setter means every panic's numbers describe
/// something that happened.
#[test]
fn a_nonsensical_budget_is_clamped_where_it_is_written() {
    assert_eq!(CascadeBudget::default().total, 100_000);
    assert_eq!(
        CascadeBudget::new(0).total,
        1,
        "zero is not a usable budget"
    );

    let model = SceneModel::new();
    model.set_cascade_budget(CascadeBudget { total: 0 });
    assert_eq!(
        model.cascade_budget().total,
        1,
        "the setter must clamp too - the public field lets a caller bypass \
         `CascadeBudget::new`"
    );

    // Three links, so the cascade generates two observer deliveries and passes
    // the clamped bound of one. The message must describe *that*.
    let ids: Vec<ItemId> = (0..3)
        .map(|_| model.add_item(RectItem::new(unit_rect()), Point::ZERO))
        .collect();
    let writer = model.clone();
    let links = Rc::new(ids.clone());
    let _h = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change {
            let Some(at) = links.iter().position(|c| *c == id) else {
                return;
            };
            let Some(next) = links.get(at + 1).copied() else {
                return;
            };
            let want = Point::new(new.x, new.y + 1.0);
            if writer.local_pos(next) != Some(want) {
                writer.set_local_pos(next, want);
            }
        }
    });

    let outcome = hushed(|| model.set_local_pos(ids[0], Point::new(50.0, 0.0)));
    let err = outcome.expect_err("two deliveries must not fit in a budget of one");
    let message = panic_message(&*err);
    assert_eq!(number_after(&message, "budget of "), 1, "got: {message}");
    assert_eq!(
        number_after(&message, "have generated "),
        2,
        "the count has to be what was delivered, got: {message}"
    );
    assert_eq!(
        number_after(&message, ", with "),
        1,
        "...and the most-charged subject has to be a subject that exists, got: \
         {message}"
    );
}

/// The other runaway shape, and the reason a per-subject reading is a
/// diagnostic rather than a trip condition: an observer that *spawns* repeats no
/// subject at all, so per-subject counts stay near 1 for ever. The flat total is
/// what stops it, and the diagnostic says which shape it was by reporting a
/// most-charged count near 1 against a hundred thousand deliveries.
#[test]
fn a_runaway_that_never_repeats_an_item_still_terminates() {
    let model = SceneModel::new();

    let writer = model.clone();
    let _h = model
        .item_change_signal()
        .observe(move |change| match *change {
            // Each reaction is about a brand-new item, so every key is fresh.
            ItemChange::Added { id } => {
                writer.remove(id);
            }
            ItemChange::Removed { .. } => {
                writer.add_item(RectItem::new(unit_rect()), Point::ZERO);
            }
            _ => {}
        });

    let outcome = hushed(|| model.add_item(RectItem::new(unit_rect()), Point::ZERO));
    let err = outcome.expect_err("a spawning feedback loop must not spin for ever");
    let message = panic_message(&*err);
    assert!(
        message.contains("did not settle"),
        "the diagnostic must say what went wrong, got: {message}"
    );
    assert!(
        message.contains("*new* subjects"),
        "...and must offer the shape it actually is - an observer producing \
         new subjects - beside the write-back it cannot rule out. Got: {message}"
    );
    let delivered = number_after(&message, "have generated ");
    assert_eq!(delivered, CascadeBudget::default().total + 1);
    let most_charged = number_after(&message, ", with ");
    assert!(
        most_charged <= 2,
        "no subject repeats in a spawner, so the most-charged count is what \
         tells a reader which shape this was; it must not be large. Got \
         {most_charged} of {delivered}: {message}"
    );
}

/// The caller's own batch is exempt from the budget, at any size. It is
/// finite by construction, so charging it would turn the runaway detector into
/// a cap on how much one `write_guard` may do - the mistake a plain delivery
/// counter makes, whose "fix" is to raise it until it catches nothing.
#[test]
fn the_callers_own_batch_is_exempt_from_the_runaway_budget() {
    let model = SceneModel::new();
    let ids: Vec<ItemId> = (0..400)
        .map(|_| model.add_item(RectItem::new(unit_rect()), Point::ZERO))
        .collect();
    let (log, _handles) = record(&model);

    {
        let mut scene = model.write_guard();
        for (n, id) in ids.iter().enumerate() {
            scene.set_local_pos(*id, Point::new(n as f32 + 1.0, 0.0));
        }
        assert!(
            log.borrow().is_empty(),
            "400 changes are queued, not 400 fan-outs — if they delivered as \
             they were made this is not measuring rounds at all"
        );
    }

    assert_eq!(
        log.borrow().len(),
        400,
        "400 changes in one scope are one round of 400 deliveries, not 400 rounds"
    );
}

/// Dropping the scene with a batch still queued is legal and silent. It can
/// only happen after an unwind abandoned one, and by then there is no scene
/// left to observe.
#[test]
fn a_queue_abandoned_by_an_unwind_dies_with_the_scene() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    let editing = model.clone();
    let outcome = hushed(move || {
        let mut scene = editing.write_guard();
        scene.set_local_pos(id, Point::new(5.0, 5.0));
        panic!("edit exploded");
    });
    assert!(outcome.is_err());
    assert!(log.borrow().is_empty());

    drop(model); // last handle: the scene and its queue go together
    assert!(
        log.borrow().is_empty(),
        "a dropped scene must not fan out from its destructor"
    );
}

// ---------------------------------------------------------------------------
// `flush_changes` is the recovery door, so it never panics
// ---------------------------------------------------------------------------

/// Every unwind-recovery path in this file is told to call `flush_changes`, and
/// an app's `catch_unwind` boundary has no way to know what the code it caught
/// was holding when it blew up. A `flush_changes` that panicked from some call
/// sites would be a recovery door that fails exactly when it is needed.
///
/// Inside an open write scope the right answer is "nothing": that scope owns its
/// batch and fans it out when it closes, and delivering a half-finished mutation
/// would be worse than delivering it late. Saying so quietly is the contract —
/// the earlier implementation took a plain `borrow()` here and panicked with
/// `RefCell already mutably borrowed`, against a doc sentence promising it was
/// safe to call at any time.
#[test]
fn flush_changes_inside_a_write_scope_is_a_no_op_not_a_panic() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(1.0, 1.0));
        model.flush_changes(); // must not panic, must not deliver
        assert!(
            log.borrow().is_empty(),
            "a flush from inside the scope delivered a half-finished mutation"
        );
        scene.set_local_pos(id, Point::new(2.0, 2.0));
    }

    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(id), Note::Moved(id)],
        "the scope's own close still owes the whole batch"
    );
}

/// The same question with a *shared* borrow outstanding, which is the shape of
/// every read-only policy closure in the crate (`SceneView::focus_order`,
/// `MagnetismConfig`'s predicate) — and of `SceneView::scene()`, used here
/// because it is the public way to hold one.
///
/// A shared borrow disqualifies a flush just as firmly as an exclusive one, and
/// for a reason `try_borrow` cannot see: an observer is *invited* to write, and
/// a write needs the cell exclusively. Draining under a live shared borrow would
/// hand that invitation to an observer and then panic it.
#[test]
fn flush_changes_under_a_shared_borrow_is_a_no_op_not_a_panic() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    // An observer that writes back — legal, and the reason a shared borrow is
    // not good enough to drain under.
    let writer = model.clone();
    let _reactor = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, new, .. } = *change
            && new.x < 9.0
        {
            writer.set_local_pos(id, Point::new(9.0, 9.0));
        }
    });

    // Abandon a batch so there is something for the flush to be tempted by.
    let editing = model.clone();
    let outcome = hushed(move || {
        let mut scene = editing.write_guard();
        scene.set_local_pos(id, Point::new(1.0, 1.0));
        panic!("edit exploded");
    });
    assert!(outcome.is_err());

    let view = SceneView::with_model(model.clone());
    {
        let _reading = view.scene(); // a live shared borrow
        model.flush_changes(); // must not panic, must not deliver
        assert!(log.borrow().is_empty());
    }

    model.flush_changes();
    assert_eq!(model.local_pos(id), Some(Point::new(9.0, 9.0)));
    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(id), Note::Moved(id)],
        "once the borrow is gone the queued change and the observer's answer to \
         it both land"
    );
}

/// A flush re-entered from inside an observer collapses into the outer drain
/// rather than recursing. Without that, delivery would go depth-first and an
/// observer's own write would jump ahead of changes emitted before it.
#[test]
fn flush_changes_from_inside_an_observer_is_a_no_op() {
    let model = SceneModel::new();
    let a = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let b = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let c = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    let (log, _handles) = record(&model);
    let writer = model.clone();
    let _reactor = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, .. } = *change
            && id == a
        {
            writer.set_local_pos(c, Point::new(99.0, 99.0));
            writer.flush_changes(); // must not deliver c's move here
        }
    });

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(a, Point::new(1.0, 1.0));
        scene.set_local_pos(b, Point::new(2.0, 2.0));
    }

    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(a), Note::Moved(b), Note::Moved(c)],
        "a nested flush delivered depth-first"
    );
}

// ---------------------------------------------------------------------------
// Ordering, batching, re-entrancy, coalescing
// ---------------------------------------------------------------------------

/// A block of edits under one guard is one notification round, delivered after
/// the borrow is released — not interleaved with the edits.
#[test]
fn a_write_guard_block_fans_out_once_the_borrow_is_released() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(10.0, 10.0));
        assert!(
            log.borrow().is_empty(),
            "nothing may fan out while the scene is exclusively borrowed"
        );
        scene.set_visible(id, false);
        assert!(log.borrow().is_empty());
    }

    assert_eq!(
        *log.borrow(),
        vec![
            Note::Moved(id),
            Note::VisibilityChanged(id),
            Note::FlagsChanged(id)
        ],
        "the whole batch fans out on drop, in emission order"
    );
}

/// FIFO has to hold across the seam between the two doors, not just inside
/// each. A queue left part-delivered — by an observer that panicked, or by an
/// unwind out of a write scope — is older than anything emitted afterwards, so a
/// later synchronous fan-out must queue behind it rather than overtake it.
/// Firing past it makes the item appear to move backwards when the stale change
/// finally lands, which is a worse answer than late delivery and breaks the one
/// ordering guarantee this channel makes.
///
/// The synchronous door here is `SceneView::scene_mut()`, a `&mut Scene`
/// reborrowed out of the shared model — the only way to reach the `defer_depth
/// == 0` branch on a scene that can have a queue at all.
#[test]
fn a_synchronous_emit_never_overtakes_an_abandoned_batch() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let _h = model
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(note_with_target(*c)));

    // Abandon a batch: (1,1) is emitted but never delivered.
    let editing = model.clone();
    let outcome = hushed(move || {
        let mut scene = editing.write_guard();
        scene.set_local_pos(id, Point::new(1.0, 1.0));
        panic!("edit exploded");
    });
    assert!(outcome.is_err());
    assert!(log.borrow().is_empty());

    // A later edit through the synchronous door.
    let mut view = SceneView::with_model(model.clone());
    view.scene_mut().set_local_pos(id, Point::new(2.0, 2.0));

    assert_eq!(
        *log.borrow(),
        vec![Note::MovedTo(id, 1, 1), Note::MovedTo(id, 2, 2)],
        "the synchronous emit jumped the queue: the item is reported moving \
         backwards once the older change lands"
    );
}

/// `Scene::remove` documents a leaves-then-root order for the `Removed` events
/// of a subtree. A FIFO queue preserves it; a stack or a per-id map would not.
///
/// And the events arrive *after* the whole removal, not interleaved with it: an
/// observer told about the first leaf already sees a scene with the entire
/// subtree gone. Under a synchronous fan-out it would be told about a leaf while
/// the parent was still half-present — and reading the scene to find that out
/// would panic on the borrow, which is the trap this mechanism removed.
#[test]
fn a_recursive_remove_keeps_its_leaves_then_root_order() {
    let model = SceneModel::new();
    let parent = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let child_a = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let child_b = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    model.set_item_parent(child_a, Some(parent));
    model.set_item_parent(child_b, Some(parent));

    let (log, _handles) = record(&model);

    // Every `Removed` observer reads the scene back. `None` at every one of them
    // means the mutation had fully landed before any notification went out.
    let reader = model.clone();
    let sizes: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let seen_sizes = sizes.clone();
    let _watcher = model.item_change_signal().observe(move |c| {
        if matches!(*c, ItemChange::Removed { .. }) {
            seen_sizes.borrow_mut().push(reader.len());
        }
    });

    model.remove(parent);

    let removals: Vec<ItemId> = log
        .borrow()
        .iter()
        .filter_map(|n| match n {
            Note::Removed(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(removals.len(), 3, "parent and both children: {removals:?}");
    assert_eq!(
        removals.last(),
        Some(&parent),
        "the root must be removed last: {removals:?}"
    );
    assert!(removals[..2].contains(&child_a) && removals[..2].contains(&child_b));
    assert_eq!(
        *sizes.borrow(),
        vec![0, 0, 0],
        "every observer call must see the completed mutation, not a subtree \
         half-way through being torn down"
    );
}

/// The re-entrancy policy, stated as a test: a write made from inside an
/// observer queues behind whatever was already queued, rather than being
/// delivered depth-first at the point of the write. So the third item's move
/// arrives *after* the second item's, even though it was caused by the first's.
#[test]
fn an_observer_write_lands_after_the_changes_already_queued() {
    let model = SceneModel::new();
    let a = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let b = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let c = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    let (log, _handles) = record(&model);
    let writer = model.clone();
    let _reactor = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalPosChanged { id, .. } = *change
            && id == a
        {
            writer.set_local_pos(c, Point::new(99.0, 99.0));
        }
    });

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(a, Point::new(1.0, 1.0));
        scene.set_local_pos(b, Point::new(2.0, 2.0));
    }

    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(a), Note::Moved(b), Note::Moved(c)],
        "an observer's own write must not jump the queue"
    );
    assert_eq!(model.local_pos(c), Some(Point::new(99.0, 99.0)));
}

/// The two channels share one queue precisely so that a mutation touching item
/// geometry *and* logical AT structure still reads in the order it happened —
/// two independently-ordered streams would let an AT observer see a landmark
/// declared before the item it describes has moved.
#[test]
fn both_channels_interleave_in_emission_order() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(1.0, 1.0));
        scene.set_a11y_landmark(A11yNode::Item(id), Role::Region);
        assert!(
            log.borrow().is_empty(),
            "both channels defer, or only one of them is being tested here"
        );
        scene.set_local_pos(id, Point::new(2.0, 2.0));
    }

    assert_eq!(
        *log.borrow(),
        vec![Note::Moved(id), Note::A11y, Note::Moved(id)]
    );
}

/// No coalescing, on purpose. The advertised uses of this channel — validation,
/// persistence, audit, mirroring into a data layer — are exactly the ones that
/// need every event, and collapsing two changes would mean merging one's `old`
/// with the other's `new`, a transaction semantic this crate does not define.
#[test]
fn repeated_moves_of_one_item_are_not_coalesced() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let _h = model
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(note_with_target(*c)));

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(1.0, 0.0));
        scene.set_local_pos(id, Point::new(2.0, 0.0));
        assert!(
            log.borrow().is_empty(),
            "the batch must still be queued, or this is not testing the queue"
        );
        scene.set_local_pos(id, Point::new(3.0, 0.0));
    }

    assert_eq!(
        *log.borrow(),
        vec![
            Note::MovedTo(id, 1, 0),
            Note::MovedTo(id, 2, 0),
            Note::MovedTo(id, 3, 0)
        ],
        "every intermediate position must survive the queue, distinctly"
    );
}

/// The documented timing split, and the control case for everything above: a
/// `Scene` owned outright has nothing holding a `RefCell` open for an observer
/// to escape from, so it keeps fanning out synchronously inside the mutator.
/// This one is meant to pass whether or not deferral exists — that is what makes
/// it the control.
#[test]
fn a_bare_scene_still_notifies_synchronously() {
    let mut scene = Scene::new();
    let id = scene.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let _h = scene
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(Note::from(*c)));

    scene.set_local_pos(id, Point::new(4.0, 4.0));
    assert_eq!(*log.borrow(), vec![Note::Moved(id)]);

    scene.set_visible(id, false);
    assert_eq!(
        *log.borrow(),
        vec![
            Note::Moved(id),
            Note::VisibilityChanged(id),
            Note::FlagsChanged(id)
        ],
        "nothing accumulates unflushed on a scene nobody opened a scope on"
    );
}

/// `mutation_version` is the scene's own state, not a notification, so it keeps
/// advancing at mutation time while the notification queues. The two are
/// transiently out of step only for as long as the scene is exclusively
/// borrowed, which is what makes the divergence unobservable: nothing else can
/// read the scene through that window.
#[test]
fn mutation_version_advances_at_mutation_time_not_at_flush_time() {
    let model = SceneModel::new();
    let id = model.add_item(RectItem::new(unit_rect()), Point::ZERO);
    let (log, _handles) = record(&model);

    let before = model.mutation_version();
    let inside = {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(1.0, 0.0));
        scene.set_local_pos(id, Point::new(2.0, 0.0));
        assert!(log.borrow().is_empty());
        scene.mutation_version()
    };
    assert_ne!(inside, before, "the version must track the mutation");
    assert_eq!(
        model.mutation_version(),
        inside,
        "the flush must not advance it a second time"
    );
    assert_eq!(log.borrow().len(), 2);
}

/// `structural_version` is `mutation_version` with per-frame dynamic-bounds
/// churn subtracted out, and it is what an expensive rebuild — the `SceneView`'s
/// AccessKit re-walk — gates on.
///
/// The exclusion has to be named rather than reconstructed by bracketing
/// `refresh_dynamic_bounds` with two `mutation_version` snapshots, because the
/// refresh now fans its changes out and an observer may legally mutate the model
/// from that fan-out. Inside the bracket, that mutation is indistinguishable
/// from churn; folded into the baseline, an AT-structural change made there
/// would never un-gate a re-walk again. So: the refresh's own emissions must not
/// move `structural_version`, and an observer's write during it must.
#[test]
fn structural_version_excludes_dynamic_churn_but_not_an_observers_write() {
    /// A lightweight item whose `local_bounds` is read back out of a cell, the
    /// shape `add_item_dynamic` exists for (an animating or signal-driven size).
    #[derive(Debug)]
    struct DynRect(Rc<Cell<Rect>>);
    impl teksilo_scene::SceneItem for DynRect {
        fn local_bounds(&self) -> Rect {
            self.0.get()
        }
        fn set_local_bounds(&mut self, b: Rect) {
            self.0.set(b);
        }
        fn paint(
            &self,
            _: &mut teksilo_canvas::Canvas,
            _: &teksilo_scene::SceneItemPaintContext<'_>,
        ) {
        }
    }

    let model = SceneModel::new();
    let bounds = Rc::new(Cell::new(unit_rect()));
    let dynamic = model.add_item_dynamic(DynRect(bounds.clone()), Point::ZERO);
    let other = model.add_item(RectItem::new(unit_rect()), Point::ZERO);

    // Churn alone: `mutation_version` moves, `structural_version` does not.
    let structural_before = model.structural_version();
    let mutation_before = model.mutation_version();
    bounds.set(Rect::new(0.0, 0.0, 40.0, 40.0));
    assert!(model.refresh_dynamic_bounds(), "the item's bounds changed");
    assert_ne!(
        model.mutation_version(),
        mutation_before,
        "the refresh is still a mutation"
    );
    assert_eq!(
        model.structural_version(),
        structural_before,
        "per-frame bounds drift is not a structural change"
    );

    // Now an observer that makes a real AT-structural change from inside the
    // refresh's fan-out. It must move `structural_version`.
    let writer = model.clone();
    let _reactor = model.item_change_signal().observe(move |change| {
        if let ItemChange::LocalBoundsChanged { id, .. } = *change
            && id == dynamic
        {
            writer.set_a11y_landmark(A11yNode::Item(other), Role::Region);
        }
    });

    let structural_before = model.structural_version();
    bounds.set(Rect::new(0.0, 0.0, 60.0, 60.0));
    assert!(model.refresh_dynamic_bounds());
    assert_ne!(
        model.structural_version(),
        structural_before,
        "an AT-structural change made from inside the refresh's fan-out was \
         swallowed by the churn exclusion: the AccessKit re-walk it should have \
         un-gated will never happen"
    );
}

// ---------------------------------------------------------------------------
// Accessibility: the AT tree still follows a batched mutation end to end
// ---------------------------------------------------------------------------

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

fn node_role_and_bounds(update: &TreeUpdate, id: NodeId) -> (Role, accesskit::Rect) {
    let node = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n)
        .unwrap_or_else(|| panic!("node {id:?} is absent from the emitted tree"));
    (node.role(), node.bounds().expect("a scene item is placed"))
}

fn refresh(tree: &mut WidgetTree) -> TreeUpdate {
    tree.layout(VIEWPORT);
    let _ = tree.render();
    tree.accessibility_tree_snapshot()
}

/// Deferral changes *when* the two channels fire, and the `SceneView` is their
/// main consumer: its `item_change_signal` observer drives the reconcile pass
/// that re-walks AccessKit, and its `a11y_change_signal` observer does the same
/// for pure-AT structure. Both must still land — a batched move has to reach
/// assistive tech with the right bounds, and a landmark declared in the same
/// batch has to reach it with the right role.
#[test]
fn the_at_tree_follows_a_batched_model_mutation() {
    let model = SceneModel::new();
    let id = model.add_item(
        TextItem::new(lit!("Scene node"), Rect::new(0.0, 0.0, 200.0, 30.0)),
        Point::new(40.0, 40.0),
    );

    let backend = Rc::new(RefCell::new(MockTextBackend::new()));
    let mut tree = WidgetTree::new().with_text_backend(backend);
    let view_id: WidgetId = tree.add(SceneView::with_model(model.clone()));
    let before = refresh(&mut tree);

    let at_id = synthetic_node_id(view_id, id.as_u64(), SyntheticKind::SceneItem);
    let (role_before, bounds_before) = node_role_and_bounds(&before, at_id);
    assert_ne!(role_before, Role::Region);

    // One batch: a geometry change on the item channel and a landmark on the
    // AT-structure channel, both deferred to the guard's drop.
    let (log, _handles) = record(&model);
    {
        let mut scene = model.write_guard();
        scene.set_local_pos(id, Point::new(140.0, 240.0));
        scene.set_a11y_landmark(A11yNode::Item(id), Role::Region);
        assert!(
            log.borrow().is_empty(),
            "the batch must still be queued here, or the view is being driven \
             by a synchronous fan-out and this proves nothing about deferral"
        );
    }

    let after = refresh(&mut tree);
    let (role_after, bounds_after) = node_role_and_bounds(&after, at_id);
    assert_eq!(
        role_after,
        Role::Region,
        "the logical-AT channel must still reach the walker"
    );
    assert!(
        (bounds_after.x0 - bounds_before.x0 - 100.0).abs() < 0.01
            && (bounds_after.y0 - bounds_before.y0 - 200.0).abs() < 0.01,
        "the item channel must still move the AT node: {bounds_before:?} -> {bounds_after:?}"
    );
}
