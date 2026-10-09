// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! loom model of the wake target's disconnect, on the real target.

use std::sync::Arc as StdArc;

use teksilo_canvas::sync::{Arc, AtomicBool, Ordering, model, thread};

use super::{RequestRedraw, WindowWakeTarget};

/// A window that fails the model if it is asked for a redraw after the main
/// thread's disconnect returned, and records when it is dropped.
struct Watched {
    disconnected: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

impl RequestRedraw for Watched {
    fn request_redraw(&self) {
        assert!(
            !self.disconnected.load(Ordering::Acquire),
            "a redraw was asked of a window after disconnect returned"
        );
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

/// A wake from another thread racing the main thread's disconnect never
/// reaches the window after the disconnect returned, and the window is gone
/// by then, so it was let go on the main thread. Red with a `disconnect` that
/// only clears `connected`, or with a request made on an `Arc` cloned out of
/// the slot.
#[test]
fn loom_disconnect_vs_wake() {
    model(|| {
        let disconnected = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let target = WindowWakeTarget::new(StdArc::new(Watched {
            disconnected: disconnected.clone(),
            dropped: dropped.clone(),
        }));
        let producer = {
            let target = target.clone();
            thread::spawn(move || target.wake_draw())
        };
        target.disconnect();
        assert!(
            dropped.load(Ordering::Acquire),
            "disconnect returned with the window still referenced"
        );
        disconnected.store(true, Ordering::Release);
        producer.join().unwrap();
    });
}
