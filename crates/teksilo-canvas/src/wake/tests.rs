// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{CountingWaker, RedrawWaker, WakeFlag, WakeGate, WakeKind};

#[test]
fn flag_raise_reports_only_the_clear_to_set_edge() {
    let flag = WakeFlag::new();
    assert!(flag.raise(), "the first raise of a clear flag wakes");
    for _ in 0..99 {
        assert!(!flag.raise(), "later raises merge into it");
    }
}

#[test]
fn flag_take_returns_the_previous_value_and_rearms() {
    let flag = WakeFlag::new();
    let _ = flag.raise();
    assert!(flag.take());
    assert!(!flag.take(), "taken once");
    assert!(flag.raise(), "a taken flag wakes again");
}

#[test]
fn flag_park_coalesces_until_the_next_take() {
    let flag = WakeFlag::new();
    flag.park();
    assert!(!flag.raise(), "a parked flag does not wake");
    assert!(flag.take());
    assert!(flag.raise(), "after the take, it wakes again");
}

#[test]
fn flag_is_raised_consumes_nothing() {
    let flag = WakeFlag::new();
    assert!(!flag.is_raised());
    let _ = flag.raise();
    assert!(flag.is_raised());
    assert!(flag.is_raised());
    assert!(flag.take(), "still there to take");
}

#[test]
fn flag_peek_consumes_nothing() {
    let flag = WakeFlag::new();
    assert!(!flag.peek());
    assert!(flag.raise(), "a peek of a clear flag leaves it clear");
    assert!(flag.peek());
    assert!(flag.peek());
    assert!(flag.take(), "still there to take");
}

#[test]
fn gate_drops_while_hidden_and_owes_exactly_one() {
    let gate = WakeGate::new();
    gate.hide();
    for _ in 0..10 {
        assert!(!gate.pass(), "a hidden window drops the wake");
    }
    assert!(gate.show(), "and owes one when shown");
    assert!(!gate.show(), "only one");
    assert!(gate.pass(), "a shown window routes");
}

#[test]
fn gate_shown_again_without_a_drop_owes_nothing() {
    let gate = WakeGate::new();
    gate.hide();
    assert!(!gate.show());
}

#[test]
fn gate_hide_is_idempotent_and_is_hidden_follows() {
    let gate = WakeGate::new();
    assert!(!gate.is_hidden());
    gate.hide();
    gate.hide();
    assert!(gate.is_hidden());
    assert!(!gate.pass());
    assert!(gate.show());
    assert!(!gate.is_hidden());
}

#[test]
fn kind_order() {
    assert!(WakeKind::Draw < WakeKind::Layout);
    assert_eq!(WakeKind::Draw.max(WakeKind::Layout), WakeKind::Layout);
}

#[test]
fn counting_waker_counts_per_kind() {
    let waker = CountingWaker::new();
    waker.wake(WakeKind::Draw);
    waker.wake(WakeKind::Draw);
    waker.wake(WakeKind::Layout);
    assert_eq!(waker.count(), 3);
    assert_eq!(waker.count_of(WakeKind::Draw), 2);
    assert_eq!(waker.count_of(WakeKind::Layout), 1);
}

#[test]
fn counting_waker_wait_for_wakes_on_another_thread() {
    let waker = Arc::new(CountingWaker::new());
    let remote = waker.clone();
    let sleeper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        remote.wake(WakeKind::Layout);
    });
    assert!(waker.wait_for(1, Duration::from_secs(5)));
    sleeper.join().unwrap();
}

#[test]
fn counting_waker_wait_for_times_out() {
    let waker = CountingWaker::new();
    assert!(!waker.wait_for(1, Duration::from_millis(10)));
    assert!(
        waker.wait_for(0, Duration::from_secs(5)),
        "nothing to wait for"
    );
}

#[test]
fn object_safety_and_auto_traits() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<WakeFlag>();
    send_sync::<WakeGate>();
    send_sync::<CountingWaker>();
    let waker: Arc<dyn RedrawWaker> = Arc::new(CountingWaker::new());
    assert!(
        !waker.is_hidden(),
        "a waker that knows nothing is not hidden"
    );
}

/// The protocol under real threads and as many interleavings as the hardware
/// gives: four producers each publish 10,000 values, and a consumer that
/// takes the flag before reading must see every producer's last value. Most
/// telling on a weakly ordered machine (CI's macOS runners are Apple Silicon).
#[test]
fn flag_stress_no_lost_wake() {
    const PRODUCERS: usize = 4;
    const VALUES: u64 = 10_000;
    let flag = Arc::new(WakeFlag::new());
    let waker = Arc::new(CountingWaker::new());
    let slots: Arc<Vec<AtomicU64>> = Arc::new((0..PRODUCERS).map(|_| AtomicU64::new(0)).collect());

    let producers: Vec<_> = (0..PRODUCERS)
        .map(|p| {
            let (flag, waker, slots) = (flag.clone(), waker.clone(), slots.clone());
            std::thread::spawn(move || {
                for value in 1..=VALUES {
                    // Relaxed on purpose: the flag carries every ordering.
                    slots[p].store(value, Ordering::Relaxed);
                    if flag.raise() {
                        waker.wake(WakeKind::Draw);
                    }
                }
            })
        })
        .collect();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut wakes_seen = 0;
    loop {
        let all_done = producers.iter().all(|p| p.is_finished());
        if flag.take() {
            let seen: Vec<u64> = slots.iter().map(|s| s.load(Ordering::Relaxed)).collect();
            if all_done && seen.iter().all(|&v| v == VALUES) {
                break;
            }
        } else if all_done {
            // Nothing raised since the last take, every producer finished: the
            // last take must have seen every final value.
            let seen: Vec<u64> = slots.iter().map(|s| s.load(Ordering::Relaxed)).collect();
            assert!(
                seen.iter().all(|&v| v == VALUES),
                "a change was published with no wake left to report it: {seen:?}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the consumer never caught up"
        );
        wakes_seen += 1;
        let _ = waker.wait_for(wakes_seen, Duration::from_millis(50));
    }
    for p in producers {
        p.join().unwrap();
    }
}
