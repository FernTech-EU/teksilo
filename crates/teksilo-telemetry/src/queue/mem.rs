// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! In-memory `EventQueue` implementation.

use std::collections::VecDeque;
use std::sync::Mutex;

use teksilo_core::telemetry::OwnedEvent;

use super::EventQueue;

/// Bounded FIFO event buffer with `Send + Sync` access.
///
/// Capacity defaults to 10_000; oldest events are dropped past that.
/// Use [`PersistentEventQueue`](super::PersistentEventQueue) when
/// surviving process restart matters.
pub struct InMemoryEventQueue {
    inner: Mutex<VecDeque<OwnedEvent>>,
    capacity: usize,
}

impl InMemoryEventQueue {
    pub fn new() -> Self {
        Self::with_capacity(10_000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(VecDeque::with_capacity(capacity.min(256))),
            capacity,
        }
    }
}

impl Default for InMemoryEventQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl EventQueue for InMemoryEventQueue {
    fn push(&self, event: OwnedEvent) {
        let mut q = self.inner.lock().expect("queue mutex poisoned");
        if q.len() >= self.capacity {
            q.pop_front();
        }
        q.push_back(event);
    }

    fn len(&self) -> usize {
        self.inner.lock().expect("queue mutex poisoned").len()
    }

    fn drain_batch(&self, n: usize) -> Vec<OwnedEvent> {
        let mut q = self.inner.lock().expect("queue mutex poisoned");
        let take = n.min(q.len());
        q.drain(..take).collect()
    }

    fn discard_all(&self) {
        let mut q = self.inner.lock().expect("queue mutex poisoned");
        q.clear();
    }

    fn peek_recent(&self, n: usize) -> Vec<OwnedEvent> {
        let q = self.inner.lock().expect("queue mutex poisoned");
        q.iter().rev().take(n).cloned().collect::<Vec<_>>()
    }
}

impl std::fmt::Debug for InMemoryEventQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryEventQueue")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use teksilo_core::telemetry::EventCategory;

    fn ev(name: &str) -> OwnedEvent {
        OwnedEvent {
            name: name.to_string(),
            category: EventCategory::Intent,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: None,
            session_id: "test".into(),
            schema_version: 1,
            props: vec![],
        }
    }

    #[test]
    fn push_pop_round_trip() {
        let q = InMemoryEventQueue::new();
        q.push(ev("a"));
        q.push(ev("b"));
        assert_eq!(q.len(), 2);
        let batch = q.drain_batch(10);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].name, "a");
        assert_eq!(batch[1].name, "b");
        assert!(q.is_empty());
    }

    #[test]
    fn capacity_drops_oldest() {
        let q = InMemoryEventQueue::with_capacity(3);
        for name in ["a", "b", "c", "d", "e"] {
            q.push(ev(name));
        }
        assert_eq!(q.len(), 3);
        let batch = q.drain_batch(10);
        let names: Vec<String> = batch.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["c".to_string(), "d".into(), "e".into()]);
    }

    #[test]
    fn discard_all_empties() {
        let q = InMemoryEventQueue::new();
        q.push(ev("a"));
        q.push(ev("b"));
        q.discard_all();
        assert!(q.is_empty());
    }

    #[test]
    fn peek_recent_returns_newest_first() {
        let q = InMemoryEventQueue::new();
        q.push(ev("a"));
        q.push(ev("b"));
        q.push(ev("c"));
        let recent = q.peek_recent(2);
        assert_eq!(recent[0].name, "c");
        assert_eq!(recent[1].name, "b");
    }
}
