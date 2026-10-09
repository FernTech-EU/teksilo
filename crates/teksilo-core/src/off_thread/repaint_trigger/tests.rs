// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The trigger and a node's wake state, without a tree.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use teksilo_canvas::wake::{CountingWaker, RedrawWaker, WakeKind};

use super::{NodeWakeState, RepaintTrigger};

fn attached(trigger: &RepaintTrigger, waker: &Arc<CountingWaker>) -> Arc<NodeWakeState> {
    let state = Arc::new(NodeWakeState::new(Some(
        waker.clone() as Arc<dyn RedrawWaker>
    )));
    trigger.attach_state(&state);
    state
}

#[test]
fn a_trigger_is_send_sync_and_clone() {
    fn send_sync_clone<T: Send + Sync + Clone>() {}
    send_sync_clone::<RepaintTrigger>();
}

#[test]
fn a_request_with_nothing_attached_wakes_nobody_and_coalesces_nothing() {
    let trigger = RepaintTrigger::new();
    trigger.request_repaint();
    let stats = trigger.stats();
    assert_eq!(
        (stats.requests, stats.wakes, stats.wakes_coalesced),
        (1, 0, 0)
    );
    assert!(!trigger.is_attached());
}

#[test]
fn a_burst_of_repaints_wakes_once_until_the_paint_takes_it() {
    let trigger = RepaintTrigger::new();
    let waker = Arc::new(CountingWaker::new());
    let state = attached(&trigger, &waker);
    for _ in 0..100 {
        trigger.request_repaint();
    }
    assert_eq!(waker.count_of(WakeKind::Draw), 1);
    let stats = trigger.stats();
    assert_eq!(
        (stats.requests, stats.wakes, stats.wakes_coalesced),
        (100, 1, 99)
    );
    state.consume_repaint();
    assert_eq!(trigger.stats().repaints, 1);
    trigger.request_repaint();
    assert_eq!(waker.count(), 2, "taken, the next request wakes again");
}

#[test]
fn a_relayout_wakes_with_layout() {
    let trigger = RepaintTrigger::new();
    let waker = Arc::new(CountingWaker::new());
    let state = attached(&trigger, &waker);
    trigger.request_relayout();
    trigger.request_relayout();
    assert_eq!(waker.count_of(WakeKind::Layout), 1);
    assert!(state.take_relayout());
    assert_eq!(trigger.stats().relayouts, 1);
    assert!(!state.take_relayout());
}

/// A waker whose window can be made to say it is hidden.
#[derive(Default)]
struct HidingWaker {
    hidden: AtomicBool,
    draws: AtomicUsize,
    layouts: AtomicUsize,
}

impl RedrawWaker for HidingWaker {
    fn wake(&self, kind: WakeKind) {
        match kind {
            WakeKind::Draw => self.draws.fetch_add(1, Ordering::SeqCst),
            _ => self.layouts.fetch_add(1, Ordering::SeqCst),
        };
    }
    fn is_hidden(&self) -> bool {
        self.hidden.load(Ordering::SeqCst)
    }
}

/// A pull and a relayout wake with a state wake whatever the window says
/// of itself: the event loop, which knows what the window shows and whether
/// it draws, decides whether a frame is drawn for them. A repaint wakes with
/// a draw wake.
#[test]
fn a_state_request_always_wakes_with_a_state_wake() {
    let trigger = RepaintTrigger::new();
    let waker = Arc::new(HidingWaker::default());
    let state = Arc::new(NodeWakeState::new(Some(
        waker.clone() as Arc<dyn RedrawWaker>
    )));
    trigger.attach_state(&state);

    for hidden in [false, true] {
        waker.hidden.store(hidden, Ordering::SeqCst);
        trigger.request_pull();
        assert!(state.take_pull());
        trigger.request_relayout();
        assert!(state.take_relayout());
    }
    assert_eq!(waker.layouts.load(Ordering::SeqCst), 4);
    assert_eq!(waker.draws.load(Ordering::SeqCst), 0);
    assert_eq!(trigger.stats().pulls, 2);

    trigger.request_repaint();
    assert_eq!(
        waker.draws.load(Ordering::SeqCst),
        1,
        "a repaint: a draw wake"
    );
}

/// Detaching one trigger from a widget's state leaves its other triggers and
/// what is pending on it.
#[test]
fn detaching_one_trigger_keeps_the_others_and_the_pending_request() {
    let (first, second) = (RepaintTrigger::new(), RepaintTrigger::new());
    let waker = Arc::new(CountingWaker::new());
    let state = Arc::new(NodeWakeState::new(Some(waker.clone())));
    first.attach_state(&state);
    second.attach_state(&state);
    first.request_pull();
    first.detach_state(&state);
    first.detach_state(&state);
    assert_eq!(first.stats().attachments, 0, "detached once");
    assert_eq!(first.node_list_len(), 0);
    assert_eq!(second.stats().attachments, 1);
    assert!(state.state_pending(), "the pending pull stays");
    first.request_pull();
    assert_eq!(waker.count(), 1, "a detached trigger wakes nothing");
    assert!(state.take_pull());
    assert_eq!(first.stats().pulls, 0, "nor counts what it no longer feeds");
    assert_eq!(second.stats().pulls, 1);
}

#[test]
fn a_waker_installed_after_a_request_is_woken_for_it() {
    let trigger = RepaintTrigger::new();
    let state = Arc::new(NodeWakeState::new(None));
    trigger.attach_state(&state);
    trigger.request_repaint();
    assert_eq!(trigger.stats().wakes, 0, "no waker yet");
    let waker = Arc::new(CountingWaker::new());
    state.set_waker(Some(waker.clone()));
    assert_eq!(waker.count_of(WakeKind::Draw), 1);

    let late = Arc::new(NodeWakeState::new(None));
    trigger.attach_state(&late);
    let _ = late.take_relayout();
    trigger.request_relayout();
    let other = Arc::new(CountingWaker::new());
    late.set_waker(Some(other.clone()));
    assert_eq!(other.count_of(WakeKind::Layout), 1, "the larger kind owed");
}

#[test]
fn two_widgets_in_one_window_wake_it_once_per_request() {
    let trigger = RepaintTrigger::new();
    let waker = Arc::new(CountingWaker::new());
    let _a = attached(&trigger, &waker);
    let _b = attached(&trigger, &waker);
    trigger.request_repaint();
    assert_eq!(waker.count(), 1);
    assert_eq!(trigger.stats().attachments, 2);
}

#[test]
fn a_detached_widget_wakes_nothing_and_leaves_the_list() {
    let trigger = RepaintTrigger::new();
    let waker = Arc::new(CountingWaker::new());
    let state = attached(&trigger, &waker);
    state.detach();
    state.detach();
    assert_eq!(trigger.stats().attachments, 0, "detach is idempotent");
    trigger.request_repaint();
    assert_eq!(waker.count(), 0);
    assert_eq!(trigger.node_list_len(), 0, "pruned by the request");
}

#[test]
fn attaching_prunes_what_was_released() {
    let trigger = RepaintTrigger::new();
    let waker = Arc::new(CountingWaker::new());
    for _ in 0..1000 {
        let state = attached(&trigger, &waker);
        state.detach();
    }
    let _live = attached(&trigger, &waker);
    assert!(trigger.node_list_len() <= 1);
}

#[test]
fn debug_never_takes_the_list_lock() {
    let trigger = RepaintTrigger::new();
    let held = trigger
        .shared
        .nodes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let remote = trigger.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tx.send(format!("{remote:?}")).unwrap();
    });
    let printed = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Debug waited for the list lock");
    drop(held);
    assert!(printed.starts_with("RepaintTrigger("), "{printed}");
}
