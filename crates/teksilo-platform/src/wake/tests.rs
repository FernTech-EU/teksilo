// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use teksilo_canvas::wake::{RedrawWaker, WakeKind};

use super::{RequestRedraw, WindowWakeTarget};

/// A window that counts the redraws asked of it and records the thread that
/// dropped it.
#[derive(Default)]
struct FakeWindow {
    calls: AtomicUsize,
    dropped_on: Arc<Mutex<Option<std::thread::ThreadId>>>,
}

impl RequestRedraw for FakeWindow {
    fn request_redraw(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

impl Drop for FakeWindow {
    fn drop(&mut self) {
        // Never panic here: a failing assertion may be unwinding through it.
        *self
            .dropped_on
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::thread::current().id());
    }
}

fn target() -> (Arc<WindowWakeTarget>, Arc<FakeWindow>) {
    let window = Arc::new(FakeWindow::default());
    (WindowWakeTarget::new(window.clone()), window)
}

fn counting_route() -> (Arc<dyn Fn() + Send + Sync>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let route = {
        let count = count.clone();
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }) as Arc<dyn Fn() + Send + Sync>
    };
    (route, count)
}

fn on_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f).join().unwrap();
}

/// E.1: once `disconnect` has returned, no wake reaches the window, and the
/// window reference is dropped on the disconnecting thread.
#[test]
fn a_wake_after_disconnect_reaches_nothing() {
    let dropped_on = Arc::new(Mutex::new(None));
    let window = Arc::new(FakeWindow {
        calls: AtomicUsize::new(0),
        dropped_on: dropped_on.clone(),
    });
    let calls_probe = Arc::downgrade(&window);
    let target = WindowWakeTarget::new(window);
    target.disconnect();
    assert_eq!(
        *dropped_on.lock().unwrap(),
        Some(std::thread::current().id()),
        "the window is let go on the thread that disconnects"
    );
    assert!(calls_probe.upgrade().is_none());
    let remote = target.clone();
    on_thread(move || {
        remote.wake_draw();
        remote.wake_state();
        remote.request_on_slot();
    });
    assert_eq!(target.stats().wakes, 0, "nothing counted after disconnect");
}

/// A window whose redraw request blocks until released, to hold one in
/// progress.
struct BlockingWindow {
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
    finished: Arc<AtomicBool>,
}

impl RequestRedraw for BlockingWindow {
    fn request_redraw(&self) {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            self.finished.store(true, Ordering::SeqCst);
        }
    }
}

/// E.2: `disconnect` waits for a request already in progress, so nothing
/// reaches the window after it returns.
#[test]
fn disconnect_waits_for_an_inflight_wake() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let target = WindowWakeTarget::new(Arc::new(BlockingWindow {
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(release_rx),
        finished: finished.clone(),
    }));
    let waker = {
        let target = target.clone();
        std::thread::spawn(move || target.wake_draw())
    };
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        release_tx.send(()).unwrap();
    });
    target.disconnect();
    assert!(
        finished.load(Ordering::SeqCst),
        "disconnect returned while a request was still in progress"
    );
    waker.join().unwrap();
    releaser.join().unwrap();
}

/// E.3: with an off-main route, a draw wake made off the main thread posts
/// and never takes the slot; a burst posts once until taken; a wake on the
/// main thread asks the window directly.
#[test]
fn an_off_main_route_never_takes_the_slot() {
    let (target, window) = target();
    let (route, posted) = counting_route();
    target.set_off_main_route(route);
    {
        let _held = target.hold_slot();
        let remote = target.clone();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            remote.wake_draw();
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("an off-main draw wake waited for the slot");
    }
    assert_eq!(posted.load(Ordering::SeqCst), 1);
    let remote = target.clone();
    on_thread(move || (0..10).for_each(|_| remote.wake_draw()));
    assert_eq!(posted.load(Ordering::SeqCst), 1, "a burst posts once");
    assert!(target.take_posted(WakeKind::Draw));
    let remote = target.clone();
    on_thread(move || remote.wake_draw());
    assert_eq!(posted.load(Ordering::SeqCst), 2, "taken, it posts again");
    target.wake_draw();
    assert_eq!(
        window.calls.load(Ordering::SeqCst),
        1,
        "the main thread asks directly"
    );
    assert_eq!(posted.load(Ordering::SeqCst), 2);
}

/// E.4: a hidden window drops draw wakes and asks for exactly one redraw when
/// shown; state wakes still reach it while hidden.
#[test]
fn a_hidden_target_drops_draw_wakes_and_reissues_one() {
    let (target, window) = target();
    target.set_hidden(true);
    let remote = target.clone();
    on_thread(move || (0..10).for_each(|_| remote.wake_draw()));
    assert_eq!(window.calls.load(Ordering::SeqCst), 0);
    assert_eq!(target.stats().dropped_hidden, 10);
    target.wake_state();
    assert_eq!(
        window.calls.load(Ordering::SeqCst),
        1,
        "a state wake is never dropped (no route: a redraw request)"
    );
    target.set_hidden(false);
    assert_eq!(window.calls.load(Ordering::SeqCst), 2, "one redraw owed");
    target.set_hidden(false);
    assert_eq!(window.calls.load(Ordering::SeqCst), 2, "only one");

    let (route, posted) = counting_route();
    target.set_state_route(route);
    target.set_hidden(true);
    target.wake_state();
    assert_eq!(
        posted.load(Ordering::SeqCst),
        1,
        "hidden, a state wake posts"
    );
}

/// E.6: a hidden window never calls the off-main route for a draw wake; its
/// reveal asks the window directly.
#[test]
fn a_hidden_target_never_calls_the_off_main_route() {
    let (target, window) = target();
    let (route, posted) = counting_route();
    target.set_off_main_route(route);
    target.set_hidden(true);
    let remote = target.clone();
    on_thread(move || remote.wake_draw());
    assert_eq!(posted.load(Ordering::SeqCst), 0);
    assert!(target.is_hidden());
    assert!(target.stats().hidden);
    target.set_hidden(false);
    assert_eq!(window.calls.load(Ordering::SeqCst), 1);
    assert_eq!(posted.load(Ordering::SeqCst), 0);
}

/// A frame about to run shows the window without the redraw a reveal owes:
/// the frame serves it.
#[test]
fn showing_for_drawing_owes_no_redraw() {
    let (target, window) = target();
    target.set_hidden(true);
    let remote = target.clone();
    on_thread(move || remote.wake_draw());
    assert_eq!(target.stats().dropped_hidden, 1);
    target.show_for_drawing();
    assert!(!target.is_hidden());
    assert_eq!(
        window.calls.load(Ordering::SeqCst),
        0,
        "no redraw asked for"
    );
    target.set_hidden(false);
    assert_eq!(
        window.calls.load(Ordering::SeqCst),
        0,
        "nothing left owed for a later reveal"
    );
    target.wake_draw();
    assert_eq!(window.calls.load(Ordering::SeqCst), 1, "shown: wakes pass");
}

/// macOS, with no off-main route: a draw wake made off the main thread never
/// waits, not even for the slot a request in progress holds. It hands the
/// request to the main queue, once per burst.
#[cfg(target_os = "macos")]
#[test]
fn a_macos_draw_wake_off_the_main_thread_never_waits() {
    let (target, window) = target();
    let _held = target.hold_slot();
    let remote = target.clone();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        remote.wake_draw();
        remote.wake_draw();
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("an off-main draw wake waited");
    assert_eq!(
        window.calls.load(Ordering::SeqCst),
        0,
        "not asked from there"
    );
    assert!(target.take_posted(WakeKind::Draw), "handed over once");
}

#[test]
fn state_wakes_coalesce_until_taken() {
    let (target, window) = target();
    let (route, posted) = counting_route();
    target.set_state_route(route);
    let remote = target.clone();
    on_thread(move || (0..100).for_each(|_| remote.wake_state()));
    assert_eq!(posted.load(Ordering::SeqCst), 1);
    assert!(target.take_posted(WakeKind::Layout));
    target.wake_state();
    assert_eq!(posted.load(Ordering::SeqCst), 2);
    assert_eq!(
        window.calls.load(Ordering::SeqCst),
        0,
        "posted, not requested"
    );
}

#[test]
fn defer_until_shown_is_remembered_for_the_reveal() {
    let (target, window) = target();
    assert!(!target.defer_until_shown(), "a shown window defers nothing");
    assert_eq!(target.stats().dropped_hidden, 0);
    target.set_hidden(true);
    assert!(target.defer_until_shown());
    assert_eq!(target.stats().dropped_hidden, 1);
    target.set_hidden(false);
    assert_eq!(window.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn the_waker_dispatches_by_kind() {
    let (target, window) = target();
    target.set_hidden(true);
    let waker: Arc<dyn RedrawWaker> = target.clone();
    assert!(waker.is_hidden());
    waker.wake(WakeKind::Draw);
    assert_eq!(window.calls.load(Ordering::SeqCst), 0, "draw: dropped");
    waker.wake(WakeKind::Layout);
    assert_eq!(window.calls.load(Ordering::SeqCst), 1, "layout: delivered");
}

#[test]
fn the_target_is_send_and_sync() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<WindowWakeTarget>();
}
