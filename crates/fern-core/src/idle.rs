//! Idle callback system for incremental work between frames.
//!
//! Operations that take 5–50ms (too short for a background thread, too long
//! for a single frame) are broken into chunks via `request_idle_callback`.
//! The event loop runs idle work during gaps between frames, respecting
//! a time budget.

use std::time::{Duration, Instant};

/// Time budget for an idle callback. The callback should check
/// `time_remaining()` and yield if it runs out.
pub struct IdleDeadline {
    deadline: Instant,
}

impl IdleDeadline {
    /// Create a new deadline with the given budget from now.
    pub fn new(budget: Duration) -> Self {
        Self {
            deadline: Instant::now() + budget,
        }
    }

    /// Create a deadline from an absolute instant.
    pub fn from_instant(deadline: Instant) -> Self {
        Self { deadline }
    }

    /// How much time remains before the deadline.
    pub fn time_remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// Whether the deadline has passed.
    pub fn did_timeout(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// A stored idle callback waiting to be executed.
pub(crate) type IdleCallback = Box<dyn FnOnce(IdleDeadline)>;

/// Queue of pending idle callbacks.
#[derive(Default)]
pub(crate) struct IdleQueue {
    callbacks: Vec<IdleCallback>,
}

impl IdleQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue an idle callback.
    #[allow(dead_code)] // Public API for idle callback scheduling
    pub fn push(&mut self, callback: impl FnOnce(IdleDeadline) + 'static) {
        self.callbacks.push(Box::new(callback));
    }

    /// Enqueue an already-boxed idle callback.
    pub fn push_boxed(&mut self, callback: IdleCallback) {
        self.callbacks.push(callback);
    }

    /// Drain all pending callbacks.
    pub fn drain(&mut self) -> Vec<IdleCallback> {
        std::mem::take(&mut self.callbacks)
    }

    /// Whether there are pending callbacks.
    pub fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }

    /// Number of pending callbacks.
    #[allow(dead_code)] // Public API complementing is_empty()
    pub fn len(&self) -> usize {
        self.callbacks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_deadline_time_remaining() {
        let deadline = IdleDeadline::new(Duration::from_millis(100));
        assert!(!deadline.did_timeout());
        assert!(deadline.time_remaining() > Duration::ZERO);
    }

    #[test]
    fn idle_deadline_expired() {
        let deadline = IdleDeadline::from_instant(Instant::now() - Duration::from_millis(1));
        assert!(deadline.did_timeout());
        assert_eq!(deadline.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn idle_queue_push_and_drain() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut queue = IdleQueue::new();
        assert!(queue.is_empty());

        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        queue.push(move |_deadline| {
            c.set(true);
        });

        assert_eq!(queue.len(), 1);
        let callbacks = queue.drain();
        assert!(queue.is_empty());

        // Execute the callback
        callbacks.into_iter().for_each(|cb| {
            cb(IdleDeadline::new(Duration::from_millis(16)));
        });
        assert!(called.get());
    }
}
