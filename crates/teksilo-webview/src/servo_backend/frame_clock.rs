// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`FrameClock`]: when Servo starts an animation frame, and when the backend
//! looks at its engine again.
//!
//! Servo's own refresh driver ticks every 8 ms, so an animated page renders 120
//! frames a second whatever the display shows: measured, 594 frames rendered
//! and read back for 300 a 60 Hz window presented. This clock starts a frame at
//! most every [`FRAME_INTERVAL`], and only while Servo has asked for one, so a
//! page that animates nothing costs nothing. It does not follow the display's
//! vertical sync: a widget is not painted when a picture's pixels change, so
//! no per-frame call from the window reaches the backend to follow.
//!
//! The same thread wakes the engine at a deadline the backend sets
//! ([`FrameClock::wake_at`]): to hand on a readback still on the GPU, or to
//! give up waiting for the frame that follows a resize.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use servo::RefreshDriver;

/// The shortest time between two animation frames: 60 a second.
pub(super) const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

type FrameCallback = Box<dyn Fn() + Send + 'static>;

#[derive(Default)]
struct ClockState {
    /// Servo's callbacks for the next frame start.
    waiting: Vec<FrameCallback>,
    /// When the last frame started.
    last_frame: Option<Instant>,
    /// When to wake the engine next.
    wake_at: Option<Instant>,
    stop: bool,
}

struct Shared {
    state: Mutex<ClockState>,
    changed: Condvar,
}

/// A thread that starts Servo's animation frames and wakes the engine on
/// time. See the [module docs](self). Dropping the last handle stops it.
pub(super) struct FrameClock {
    shared: Arc<Shared>,
}

impl FrameClock {
    /// Start the clock. `wake` asks the UI thread to run the engine; it is
    /// called from the clock's thread.
    pub(super) fn start(wake: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(ClockState::default()),
            changed: Condvar::new(),
        });
        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("teksilo-servo-clock".into())
            .spawn(move || run(&thread_shared, &wake))
            .expect("spawn the Servo frame clock");
        Self { shared }
    }

    /// Wake the engine at `at`, or sooner if an earlier wake is set already.
    pub(super) fn wake_at(&self, at: Instant) {
        let mut state = lock(&self.shared);
        if state.wake_at.is_none_or(|current| at < current) {
            state.wake_at = Some(at);
            self.shared.changed.notify_one();
        }
    }

    /// The refresh driver Servo's rendering contexts hand out.
    pub(super) fn driver(&self) -> ClockDriver {
        ClockDriver {
            shared: self.shared.clone(),
        }
    }
}

impl Drop for FrameClock {
    fn drop(&mut self) {
        lock(&self.shared).stop = true;
        self.shared.changed.notify_one();
    }
}

/// [`RefreshDriver`] over a [`FrameClock`].
pub(super) struct ClockDriver {
    shared: Arc<Shared>,
}

impl RefreshDriver for ClockDriver {
    fn observe_next_frame(&self, start_frame_callback: Box<dyn Fn() + Send + 'static>) {
        lock(&self.shared).waiting.push(start_frame_callback);
        self.shared.changed.notify_one();
    }
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, ClockState> {
    shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run(shared: &Shared, wake: &dyn Fn()) {
    let mut state = lock(shared);
    loop {
        if state.stop {
            return;
        }
        let now = Instant::now();
        let next_frame = (!state.waiting.is_empty()).then(|| {
            state
                .last_frame
                .map_or(now, |last| (last + FRAME_INTERVAL).max(now))
        });
        let next = match (next_frame, state.wake_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(next) = next else {
            state = shared
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            continue;
        };
        if next > now {
            state = shared
                .changed
                .wait_timeout(state, next - now)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            continue;
        }
        let frame = next_frame
            .filter(|at| *at <= now)
            .map(|_| std::mem::take(&mut state.waiting));
        let woken = state.wake_at.is_some_and(|at| at <= now);
        if frame.is_some() {
            state.last_frame = Some(now);
        }
        if woken {
            state.wake_at = None;
        }
        // Callbacks and the wake run unlocked: either may register again.
        drop(state);
        for callback in frame.into_iter().flatten() {
            callback();
        }
        if woken {
            wake();
        }
        state = lock(shared);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn frames_start_no_faster_than_the_interval() {
        let clock = FrameClock::start(|| {});
        let driver = clock.driver();
        let starts = Arc::new(Mutex::new(Vec::new()));
        let begin = Instant::now();
        for _ in 0..4 {
            let (sent, seen) = std::sync::mpsc::channel();
            let starts = starts.clone();
            driver.observe_next_frame(Box::new(move || {
                starts.lock().unwrap().push(Instant::now());
                let _ = sent.send(());
            }));
            seen.recv_timeout(Duration::from_secs(1))
                .expect("a frame starts");
        }
        let starts = starts.lock().unwrap();
        for pair in starts.windows(2) {
            assert!(
                pair[1] - pair[0] >= FRAME_INTERVAL - Duration::from_micros(500),
                "two frames {:?} apart",
                pair[1] - pair[0]
            );
        }
        assert!(begin.elapsed() >= FRAME_INTERVAL * 3 - Duration::from_millis(2));
    }

    #[test]
    fn a_deadline_wakes_the_engine_once() {
        let woken = Arc::new(AtomicU32::new(0));
        let counter = woken.clone();
        let clock = FrameClock::start(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        clock.wake_at(Instant::now() + Duration::from_millis(5));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(woken.load(Ordering::SeqCst), 1);
    }
}
