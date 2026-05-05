//! Background HTTP worker.
//!
//! The adapter spawns a worker thread on construction. The thread
//! drains an [`EventQueue`] (in-memory or persistent) and POSTs to
//! Plausible. The UI thread sends events via an mpsc channel; the
//! channel hands events to the worker, the worker pushes them into
//! the queue, the worker periodically drains the queue and sends.
//!
//! Why funnel through a queue rather than a `VecDeque` directly:
//! a [`PersistentEventQueue`](fern_telemetry::PersistentEventQueue)
//! survives process restart, so events buffered when the machine
//! sleeps or the app crashes still flush on next launch. The trait
//! lets the same worker work against either backend.
//!
//! Commands (see [`WorkerCommand`]):
//! - `Record(OwnedEvent)` — push to queue.
//! - `Flush(SyncSender<...>)` — drain immediately, reply when done.
//! - `Discard` — clear the queue without sending.
//! - `Shutdown` — final flush then exit.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fern_core::telemetry::OwnedEvent;
use fern_telemetry::EventQueue;

use crate::config::PlausibleConfig;
use crate::transport::{SendOutcome, build_agent, next_backoff, send_event};
use crate::wire::PlausibleEvent;

#[derive(Debug)]
pub(crate) enum WorkerCommand {
    Record(OwnedEvent),
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
}

pub(crate) fn spawn_worker(
    config: PlausibleConfig,
    queue: Arc<dyn EventQueue>,
    stats: Arc<WorkerStats>,
) -> (mpsc::Sender<WorkerCommand>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<WorkerCommand>();
    let handle = thread::Builder::new()
        .name("fern-plausible-worker".to_string())
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
    let mut last_flush = Instant::now();

    loop {
        let wait = if queue.is_empty() {
            config.flush_interval
        } else {
            let until_flush = config.flush_interval.saturating_sub(last_flush.elapsed());
            until_flush.min(current_backoff)
        };

        match rx.recv_timeout(wait) {
            Ok(WorkerCommand::Record(event)) => {
                queue.push(event);
                stats.queued.store(queue.len(), Ordering::Relaxed);
            }
            Ok(WorkerCommand::Flush(reply)) => {
                let result = drain(&*queue, &agent, &config, &stats, &mut current_backoff);
                let _ = reply.send(result);
                last_flush = Instant::now();
            }
            Ok(WorkerCommand::Discard) => {
                queue.discard_all();
                stats.queued.store(0, Ordering::Relaxed);
            }
            Ok(WorkerCommand::Shutdown) => {
                let _ = drain(&*queue, &agent, &config, &stats, &mut current_backoff);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !queue.is_empty() {
                    let _ = drain(&*queue, &agent, &config, &stats, &mut current_backoff);
                    last_flush = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = drain(&*queue, &agent, &config, &stats, &mut current_backoff);
                break;
            }
        }

        if queue.len() >= config.max_batch_size {
            let _ = drain(&*queue, &agent, &config, &stats, &mut current_backoff);
            last_flush = Instant::now();
        }
    }
}

/// Drain up to `max_batch_size` events. Events that hit a `Retry`
/// outcome are pushed back to the queue (re-enqueued at the tail —
/// FIFO order is preserved among the events that *don't* fail; the
/// failed ones get retried after newer ones, which is acceptable for
/// analytics ordering).
fn drain(
    queue: &dyn EventQueue,
    agent: &ureq::Agent,
    config: &PlausibleConfig,
    stats: &WorkerStats,
    current_backoff: &mut Duration,
) -> Result<(), String> {
    let mut last_error = None;
    let batch = queue.drain_batch(config.max_batch_size);
    let mut to_requeue: Vec<OwnedEvent> = Vec::new();
    let mut hit_retry = false;
    for owned in batch {
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
            }
            SendOutcome::Retry(why) => {
                to_requeue.push(owned);
                *current_backoff = next_backoff(*current_backoff, config.max_backoff);
                last_error = Some(why);
                hit_retry = true;
            }
            SendOutcome::Drop(why) => {
                stats.dropped.fetch_add(1, Ordering::Relaxed);
                last_error = Some(why);
            }
        }
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
