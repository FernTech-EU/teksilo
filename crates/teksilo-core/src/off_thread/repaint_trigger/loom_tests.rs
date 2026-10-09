// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! loom models of the trigger's register-then-read attach and of a waker
//! installed after a request, on the real trigger and node state.

use std::sync::Arc as StdArc;

use teksilo_canvas::sync::{Arc, AtomicU64, AtomicUsize, Ordering, model, thread};
use teksilo_canvas::wake::{RedrawWaker, WakeKind};

use super::{NodeWakeState, RepaintTrigger};

/// Counts wakes with a loom atomic, so loom sees them.
struct LoomWaker(AtomicUsize);

impl RedrawWaker for LoomWaker {
    fn wake(&self, _kind: WakeKind) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// A producer that stores its state and requests, racing a widget attaching
/// and then reading the state in its first paint: either the read sees the
/// state, or the request woke the window. It rests on the list lock: the
/// attach publishes under it, and a request copies the list under it. Red
/// with the read moved before the attach.
#[test]
fn loom_trigger_attach_register_then_read() {
    model(|| {
        let trigger = RepaintTrigger::new();
        let waker = StdArc::new(LoomWaker(AtomicUsize::new(0)));
        let content = Arc::new(AtomicU64::new(0));
        let producer = {
            let (trigger, content) = (trigger.clone(), content.clone());
            thread::spawn(move || {
                content.store(1, Ordering::Relaxed);
                trigger.request_repaint();
            })
        };
        let state = StdArc::new(NodeWakeState::new(Some(waker.clone())));
        trigger.attach_state(&state);
        // The first paint after attaching: take the request, read the state.
        state.consume_repaint();
        let seen = content.load(Ordering::Relaxed);
        producer.join().unwrap();
        assert!(
            seen == 1 || waker.0.load(Ordering::Relaxed) == 1,
            "the change was neither read nor woken for"
        );
    });
}

/// A request racing the window's waker being installed: whenever the request
/// is still pending at the end, a wake was issued. It rests on the waker
/// lock: the raise is sequenced before the producer locks it, and the
/// installer reads the flag after unlocking it. Red without the re-wake in
/// `set_waker`.
#[test]
fn loom_trigger_waker_install_after_raise() {
    model(|| {
        let trigger = RepaintTrigger::new();
        let state = StdArc::new(NodeWakeState::new(None));
        trigger.attach_state(&state);
        let waker = StdArc::new(LoomWaker(AtomicUsize::new(0)));
        let producer = {
            let trigger = trigger.clone();
            thread::spawn(move || trigger.request_repaint())
        };
        state.set_waker(Some(waker.clone()));
        producer.join().unwrap();
        if state.repaint_pending() {
            assert!(
                waker.0.load(Ordering::Relaxed) >= 1,
                "a pending request whose window was never woken"
            );
        }
    });
}
