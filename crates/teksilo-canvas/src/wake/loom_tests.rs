// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! loom models of the wake protocol. The change a flag guards is a loom
//! atomic written and read `Relaxed`, so the flag has to carry every ordering
//! the property rests on. Each model names the mutation that turns it red.

use super::{WakeFlag, WakeGate};
use crate::sync::{Arc, AtomicBool, AtomicU64, Ordering, model, thread};

/// A change is either seen by the consumer's read after its take, or its
/// producer wakes. Red with `raise` as `swap(true, Relaxed)`, with `take` as
/// `swap(false, Release)`, or with `raise` as a load then a store.
#[test]
fn loom_flag_change_is_seen_or_wakes() {
    model(|| {
        let flag = Arc::new(WakeFlag::new());
        let _ = flag.raise();
        let change = Arc::new(AtomicU64::new(0));
        let producer = {
            let (flag, change) = (flag.clone(), change.clone());
            thread::spawn(move || {
                change.store(1, Ordering::Relaxed);
                flag.raise()
            })
        };
        let _ = flag.take();
        let seen = change.load(Ordering::Relaxed);
        let woke = producer.join().unwrap();
        assert!(seen == 1 || woke, "a change neither seen nor woken for");
    });
}

/// A consumer that parks and later takes still sees the change, or the
/// producer wakes. Red with `park` as `store(true, Release)`.
#[test]
fn loom_flag_park_keeps_the_change() {
    model(|| {
        let flag = Arc::new(WakeFlag::new());
        flag.park();
        let change = Arc::new(AtomicU64::new(0));
        let producer = {
            let (flag, change) = (flag.clone(), change.clone());
            thread::spawn(move || {
                change.store(1, Ordering::Relaxed);
                flag.raise()
            })
        };
        flag.park();
        let _ = flag.take();
        let seen = change.load(Ordering::Relaxed);
        let woke = producer.join().unwrap();
        assert!(seen == 1 || woke, "a parked change was lost");
    });
}

/// Two producers raising one clear flag wake exactly once. Red with `raise`
/// as a load then a store.
#[test]
fn loom_flag_one_wake_per_raise() {
    model(|| {
        let flag = Arc::new(WakeFlag::new());
        let other = {
            let flag = flag.clone();
            thread::spawn(move || flag.raise())
        };
        let mine = flag.raise();
        let theirs = other.join().unwrap();
        assert!(mine ^ theirs, "two producers must wake exactly once");
    });
}

/// A droppable wake racing the window being shown again: exactly one of the
/// producer routing it and the reveal re-issuing one happens. Red with
/// `show` as a load then a store, or `pass` as a check then a `fetch_or`.
#[test]
fn loom_gate_reveal_race() {
    model(|| {
        let gate = Arc::new(WakeGate::new());
        gate.hide();
        let producer = {
            let gate = gate.clone();
            thread::spawn(move || gate.pass())
        };
        let owed = gate.show();
        let routed = producer.join().unwrap();
        assert!(routed != owed, "routed {routed}, owed {owed}");
    });
}

/// A droppable wake racing the window being hidden: either the producer
/// routed it, or the next reveal owes one. Red with a `pass` that drops the
/// wake without recording it.
#[test]
fn loom_gate_hide_race() {
    model(|| {
        let gate = Arc::new(WakeGate::new());
        let producer = {
            let gate = gate.clone();
            thread::spawn(move || gate.pass())
        };
        gate.hide();
        let routed = producer.join().unwrap();
        let owed = gate.show();
        assert!(routed != owed, "routed {routed}, owed {owed}");
    });
}

/// The draw path whole: a producer publishes, raises its flag and passes the
/// gate of a hidden window while the UI thread shows the window, takes the
/// flag and reads. Either the producer routed a wake, or the reveal drew the
/// change. Red with `show` as a load then a store, with `pass` as a check
/// then a `fetch_or`, or with a `pass` that forgets the drop. Making the flag
/// alone `Relaxed` leaves it green: on this path the gate's exchange carries
/// the ordering too.
#[test]
fn loom_draw_path_composes() {
    model(|| {
        let flag = Arc::new(WakeFlag::new());
        let gate = Arc::new(WakeGate::new());
        gate.hide();
        let change = Arc::new(AtomicU64::new(0));
        let producer = {
            let (flag, gate, change) = (flag.clone(), gate.clone(), change.clone());
            thread::spawn(move || {
                change.store(1, Ordering::Relaxed);
                flag.raise() && gate.pass()
            })
        };
        let drew = if gate.show() {
            let _ = flag.take();
            Some(change.load(Ordering::Relaxed))
        } else {
            None
        };
        let routed = producer.join().unwrap();
        assert!(
            routed || drew == Some(1),
            "neither routed nor drawn: drew {drew:?}"
        );
    });
}

/// A consumer that peeks to decide whether to serve the change in the frame
/// it is delivering, racing a producer whose wake is a request on the
/// platform's own `Relaxed` redraw flag (Wayland's): either the frame saw the
/// request, or the wake queued another frame. Red with `peek` as an
/// `Acquire` load.
#[test]
fn loom_flag_peek_orders_the_decision_before_a_raise() {
    model(|| {
        let flag = Arc::new(WakeFlag::new());
        // The platform's flag: a redraw is queued, and this frame delivers it.
        let queued = Arc::new(AtomicBool::new(true));
        let producer = {
            let (flag, queued) = (flag.clone(), queued.clone());
            thread::spawn(move || {
                flag.raise()
                    && queued
                        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
            })
        };
        queued.swap(false, Ordering::Relaxed);
        let seen = flag.peek();
        let requeued = producer.join().unwrap();
        assert!(
            seen || requeued,
            "the frame saw no request and no other frame was queued"
        );
    });
}
