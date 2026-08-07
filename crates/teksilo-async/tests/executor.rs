// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Headless tests for the main-thread executor: drive `tick()` manually (no
//! winit loop) and assert futures advance, offload, and cancel correctly.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use teksilo_async::{AsyncRuntimeHandle, spawn_blocking};
use teksilo_core::{AppEventPoster, Signal, SubscriptionId};

/// Pump the executor until `cond` holds or we time out. Sleeps briefly between
/// ticks so a `spawn_blocking` worker thread can make progress.
fn pump_until(rt: &AsyncRuntimeHandle, label: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        rt.tick();
        if start.elapsed() > Duration::from_secs(5) {
            panic!("timed out waiting for: {label}");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn spawn_local_awaiting_spawn_blocking_sets_signal() {
    let rt = AsyncRuntimeHandle::new();
    let result = Signal::new(0_i32);

    let sink = result.clone();
    rt.spawn_local(async move {
        let value = spawn_blocking(|| 21 * 2).await.expect("worker ran ok");
        sink.set(value); // back on the UI thread on resume
    })
    .detach();

    pump_until(&rt, "spawn_blocking result", || result.get() == 42);
    assert_eq!(result.get(), 42);
}

#[test]
fn dropping_task_handle_cancels_the_continuation() {
    let rt = AsyncRuntimeHandle::new();
    let result = Signal::new(0_i32);

    let sink = result.clone();
    let handle = rt.spawn_local(async move {
        let value = spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(40));
            99
        })
        .await
        .unwrap_or(0);
        sink.set(value);
    });

    // One poll so the blocking worker is launched and the task parks on `recv`.
    rt.tick();
    // Drop the handle → cancel. When the worker finishes and wakes the task,
    // the next tick must discard it without running the `.set(99)` continuation.
    drop(handle);

    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(150) {
        rt.tick();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        result.get(),
        0,
        "a cancelled task must not run its continuation"
    );
}

#[test]
fn detached_task_runs_to_completion() {
    let rt = AsyncRuntimeHandle::new();
    let done = Signal::new(false);

    let sink = done.clone();
    // No handle kept — `.detach()` means it must still run.
    rt.spawn_local(async move {
        let _ = spawn_blocking(|| ()).await;
        sink.set(true);
    })
    .detach();

    pump_until(&rt, "detached task completion", || done.get());
    assert!(done.get());
}

#[test]
fn idle_tick_reports_no_work() {
    let rt = AsyncRuntimeHandle::new();
    // Nothing spawned and nothing woke us → tick is a no-op and clears the
    // poll source so the loop can sleep.
    assert!(!rt.tick());
    assert!(!rt.poll_source().get());
}

/// A fake `AppEventPoster` that just counts `post_external` calls — the wake
/// nudge the executor uses to rouse a sleeping (`ControlFlow::Wait`) event
/// loop when an off-thread future becomes runnable.
#[derive(Default)]
struct CountingPoster {
    nudges: AtomicUsize,
}

impl AppEventPoster for CountingPoster {
    fn post_subscription_event(&self, _sub_id: SubscriptionId, _event: Box<dyn Any + Send>) {}

    fn post_external(&self, _payload: Box<dyn Any + Send>) {
        self.nudges.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn off_thread_completion_nudges_the_event_loop_poster() {
    // The production wake path: when a `spawn_blocking` worker finishes on
    // another thread it wakes the executor's shared `Waker`, which must nudge
    // the installed `AppEventPoster` so a parked event loop runs one more turn
    // and re-ticks. Every other test busy-polls `tick()` in a loop, so none of
    // them exercise this nudge — a regression here would silently hang async
    // work whenever the UI is idle. `spawn()` sets the `woken` flag directly
    // without touching the poster, so the *only* path that can call
    // `post_external` is the off-thread `Waker::wake` — a non-zero count proves
    // exactly that path fired.
    let rt = AsyncRuntimeHandle::new();
    let poster = Arc::new(CountingPoster::default());
    rt.set_poster(poster.clone());

    let done = Signal::new(false);
    let sink = done.clone();
    rt.spawn_local(async move {
        // Sleep on the worker so the future is genuinely parked on `recv` and
        // the wake arrives strictly after the first poll, off-thread.
        let _ = spawn_blocking(|| std::thread::sleep(Duration::from_millis(20))).await;
        sink.set(true);
    })
    .detach();

    pump_until(&rt, "off-thread completion", || done.get());

    assert!(done.get());
    assert!(
        poster.nudges.load(Ordering::SeqCst) >= 1,
        "off-thread worker completion must nudge the event-loop poster at least \
         once (got {})",
        poster.nudges.load(Ordering::SeqCst),
    );
}

#[test]
fn spawn_blocking_panic_surfaces_as_error() {
    let rt = AsyncRuntimeHandle::new();
    // 0 = pending, 1 = resolved to Err, 2 = resolved to Ok (unexpected).
    let outcome = Signal::new(0_i32);

    let sink = outcome.clone();
    rt.spawn_local(async move {
        let result: Result<i32, _> = spawn_blocking(|| panic!("boom")).await;
        sink.set(if result.is_err() { 1 } else { 2 });
    })
    .detach();

    pump_until(&rt, "spawn_blocking panic outcome", || outcome.get() != 0);
    assert_eq!(
        outcome.get(),
        1,
        "a panicking closure must resolve to Err, not crash the executor"
    );
}
