// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Background HTTP worker.
//!
//! The adapter spawns a worker thread on construction. The thread
//! drains an [`EventQueue`] (in-memory or persistent) and POSTs to
//! Plausible. The UI thread sends events via an mpsc channel; the
//! channel hands events to the worker, the worker pushes them into
//! the queue, the worker periodically drains the queue and sends.
//!
//! Why funnel through a queue rather than a `VecDeque` directly:
//! a [`PersistentEventQueue`](teksilo_telemetry::PersistentEventQueue)
//! survives process restart, so events buffered when the machine
//! sleeps or the app crashes still flush on next launch. The trait
//! lets the same worker work against either backend.
//!
//! Commands (see [`WorkerCommand`]):
//! - `Record(OwnedEvent, u64)`: push to queue, unless a discard was
//!   asked for after it was recorded.
//! - `Flush(SyncSender<...>)`: drain immediately, reply when done.
//! - `Discard`: apply [`WorkerStats::discards`], clearing the queue without
//!   sending.
//! - `Shutdown`: final flush then exit.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use teksilo_core::telemetry::OwnedEvent;
use teksilo_telemetry::EventQueue;

use crate::config::PlausibleConfig;
use crate::transport::{SendOutcome, build_agent, next_backoff, send_event};
use crate::wire::PlausibleEvent;

#[derive(Debug)]
pub(crate) enum WorkerCommand {
    /// An event, with the count of discards asked for when it was recorded:
    /// one asked for since is not sent.
    Record(OwnedEvent, u64),
    Flush(mpsc::SyncSender<Result<(), String>>),
    Discard,
    Shutdown,
}

/// Atomic counter the worker increments after each successful send.
/// Surfaced through `PlausibleAdapter` for tests and for the
/// "Inspect data sent" panel.
#[derive(Debug, Default)]
pub(crate) struct WorkerStats {
    pub accepted: AtomicUsize,
    pub dropped: AtomicUsize,
    pub queued: AtomicUsize,
    /// Discards asked for so far. The worker compares it with the last one
    /// it applied before every event it sends, and before putting failed
    /// events back, so nothing recorded before "Reject all" goes out after
    /// it, however many events sit in the channel ahead of the wake-up.
    pub discards: AtomicU64,
}

/// Empty the queue when a discard was asked for since the last one this
/// worker applied. `true` when it did.
fn apply_discard(queue: &dyn EventQueue, stats: &WorkerStats, seen: &mut u64) -> bool {
    let asked = stats.discards.load(Ordering::Acquire);
    if asked == *seen {
        return false;
    }
    *seen = asked;
    queue.discard_all();
    stats.queued.store(0, Ordering::Relaxed);
    true
}

pub(crate) fn spawn_worker(
    config: PlausibleConfig,
    queue: Arc<dyn EventQueue>,
    stats: Arc<WorkerStats>,
) -> (mpsc::Sender<WorkerCommand>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<WorkerCommand>();
    let handle = thread::Builder::new()
        .name("teksilo-plausible-worker".to_string())
        .spawn(move || worker_loop(rx, config, queue, stats))
        .expect("spawn plausible worker");
    (tx, handle)
}

fn worker_loop(
    rx: mpsc::Receiver<WorkerCommand>,
    config: PlausibleConfig,
    queue: Arc<dyn EventQueue>,
    stats: Arc<WorkerStats>,
) {
    let agent = build_agent(config.request_timeout);
    let mut current_backoff = config.initial_backoff;
    // Set by a `Retry`: the full-batch path below sends nothing before it,
    // so an outage costs one request per backoff, not one per event.
    let mut retry_at: Option<Instant> = None;
    let mut last_flush = Instant::now();
    // The last discard applied: see `WorkerStats::discards`.
    let mut seen = 0_u64;

    loop {
        let wait = if queue.is_empty() {
            config.flush_interval
        } else {
            let until_flush = config.flush_interval.saturating_sub(last_flush.elapsed());
            until_flush.min(current_backoff)
        };

        match rx.recv_timeout(wait) {
            Ok(WorkerCommand::Record(event, discards)) => {
                apply_discard(&*queue, &stats, &mut seen);
                if discards >= seen {
                    queue.push(event);
                    stats.queued.store(queue.len(), Ordering::Relaxed);
                }
            }
            Ok(WorkerCommand::Flush(reply)) => {
                let result = drain(
                    &*queue,
                    &agent,
                    &config,
                    &stats,
                    &mut current_backoff,
                    &mut retry_at,
                    &mut seen,
                );
                let _ = reply.send(result);
                last_flush = Instant::now();
            }
            Ok(WorkerCommand::Discard) => {
                apply_discard(&*queue, &stats, &mut seen);
            }
            Ok(WorkerCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                // `drain` takes at most `max_batch_size` events, so a single
                // call is not a "final flush" for a queue bigger than one batch
                // — with the in-memory queue the surplus would die with the
                // worker. Keep draining while each pass makes progress. A pass
                // that fails to shrink the queue means either an empty queue or
                // a failing endpoint (a `Retry` re-queues what it could not
                // send), and neither is worth asking about twice, so exit stays
                // bounded by one extra request.
                loop {
                    let before = queue.len();
                    let _ = drain(
                        &*queue,
                        &agent,
                        &config,
                        &stats,
                        &mut current_backoff,
                        &mut retry_at,
                        &mut seen,
                    );
                    if queue.len() >= before {
                        break;
                    }
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !queue.is_empty() {
                    let _ = drain(
                        &*queue,
                        &agent,
                        &config,
                        &stats,
                        &mut current_backoff,
                        &mut retry_at,
                        &mut seen,
                    );
                    last_flush = Instant::now();
                }
            }
        }

        let backing_off = retry_at.is_some_and(|t| Instant::now() < t);
        if queue.len() >= config.max_batch_size && !backing_off {
            let _ = drain(
                &*queue,
                &agent,
                &config,
                &stats,
                &mut current_backoff,
                &mut retry_at,
                &mut seen,
            );
            last_flush = Instant::now();
        }
    }
}

/// Drain up to `max_batch_size` events. Events that hit a `Retry`
/// outcome are pushed back to the queue (re-enqueued at the tail —
/// FIFO order is preserved among the events that *don't* fail; the
/// failed ones get retried after newer ones, which is acceptable for
/// analytics ordering).
///
/// A discard asked for before or during the drain stops it: nothing
/// after it is sent, and nothing is put back.
fn drain(
    queue: &dyn EventQueue,
    agent: &ureq::Agent,
    config: &PlausibleConfig,
    stats: &WorkerStats,
    current_backoff: &mut Duration,
    retry_at: &mut Option<Instant>,
    seen: &mut u64,
) -> Result<(), String> {
    let mut last_error = None;
    if apply_discard(queue, stats, seen) {
        return Ok(());
    }
    let batch = queue.drain_batch(config.max_batch_size);
    let mut to_requeue: Vec<OwnedEvent> = Vec::new();
    let mut hit_retry = false;
    for owned in batch {
        if apply_discard(queue, stats, seen) {
            return Ok(());
        }
        if hit_retry {
            // After the first retry, every subsequent event is also
            // re-enqueued — don't keep hitting a failing endpoint.
            to_requeue.push(owned);
            continue;
        }
        let event =
            PlausibleEvent::from_owned(&owned, &config.domain, &config.synthetic_url_scheme);
        match send_event(agent, config, &event) {
            SendOutcome::Accepted => {
                stats.accepted.fetch_add(1, Ordering::Relaxed);
                *current_backoff = config.initial_backoff;
                *retry_at = None;
            }
            SendOutcome::Retry(why) => {
                to_requeue.push(owned);
                *current_backoff = next_backoff(*current_backoff, config.max_backoff);
                *retry_at = Some(Instant::now() + *current_backoff);
                last_error = Some(why);
                hit_retry = true;
            }
            SendOutcome::Drop(why) => {
                stats.dropped.fetch_add(1, Ordering::Relaxed);
                last_error = Some(why);
            }
        }
    }
    if apply_discard(queue, stats, seen) {
        return Ok(());
    }
    for event in to_requeue {
        queue.push(event);
    }
    stats.queued.store(queue.len(), Ordering::Relaxed);
    match last_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::telemetry::EventCategory;
    use teksilo_telemetry::InMemoryEventQueue;

    /// An in-memory queue that counts the events handed out to be sent.
    struct CountingQueue {
        inner: InMemoryEventQueue,
        handed_out: AtomicUsize,
    }

    impl EventQueue for CountingQueue {
        fn push(&self, event: OwnedEvent) {
            self.inner.push(event);
        }
        fn drain_batch(&self, n: usize) -> Vec<OwnedEvent> {
            let batch = self.inner.drain_batch(n);
            self.handed_out.fetch_add(batch.len(), Ordering::Relaxed);
            batch
        }
        fn len(&self) -> usize {
            self.inner.len()
        }
        fn discard_all(&self) {
            self.inner.discard_all();
        }
        fn peek_recent(&self, n: usize) -> Vec<OwnedEvent> {
            self.inner.peek_recent(n)
        }
    }

    fn event() -> OwnedEvent {
        OwnedEvent {
            name: "intent.dispatched".into(),
            category: EventCategory::Intent,
            timestamp: std::time::SystemTime::UNIX_EPOCH,
            install_id: None,
            session_id: "s".into(),
            schema_version: 1,
            props: Vec::new(),
        }
    }

    /// Events recorded before a discard and still in the channel when it is
    /// asked for are never sent, even though they fill a batch before the
    /// worker reads the wake-up; one recorded after it is.
    #[test]
    fn a_discard_drops_the_events_still_in_the_channel() {
        let queue = Arc::new(CountingQueue {
            inner: InMemoryEventQueue::with_capacity(1000),
            handed_out: AtomicUsize::new(0),
        });
        let config = PlausibleConfig {
            // Nothing listens there: every send fails fast and is retried.
            endpoint: "http://127.0.0.1:1/api/event".into(),
            domain: "test.app".into(),
            max_batch_size: 50,
            flush_interval: Duration::from_secs(3600),
            initial_backoff: Duration::from_secs(3600),
            request_timeout: Duration::from_millis(200),
            ..PlausibleConfig::default()
        };
        let stats = Arc::new(WorkerStats::default());
        // The adapter counts the discard before the worker reads any of the
        // events recorded ahead of it.
        stats.discards.store(1, Ordering::Release);
        let (tx, handle) = spawn_worker(config, queue.clone(), stats.clone());

        for _ in 0..60 {
            tx.send(WorkerCommand::Record(event(), 0)).unwrap();
        }
        tx.send(WorkerCommand::Discard).unwrap();
        tx.send(WorkerCommand::Record(event(), 1)).unwrap();
        tx.send(WorkerCommand::Shutdown).unwrap();
        handle.join().unwrap();

        assert_eq!(
            queue.handed_out.load(Ordering::Relaxed),
            1,
            "a refused event was handed out to be sent"
        );
    }
}
