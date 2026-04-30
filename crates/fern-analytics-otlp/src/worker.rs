//! Background worker — drains the queue, batches into one OTLP
//! `ExportLogsServiceRequest` per flush, retries on transient
//! failure with exponential backoff.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fern_core::telemetry::OwnedEvent;

use crate::config::OtlpConfig;
use crate::transport::{SendOutcome, build_agent, next_backoff, send_batch};
use crate::wire::WireBuilder;

#[derive(Debug)]
pub(crate) enum WorkerCommand {
    Record(OwnedEvent),
    Flush(mpsc::SyncSender<Result<(), String>>),
    Discard,
    Shutdown,
}

#[derive(Debug, Default)]
pub(crate) struct WorkerStats {
    pub accepted: AtomicUsize,
    pub dropped: AtomicUsize,
    pub queued: AtomicUsize,
}

pub(crate) fn spawn_worker(
    config: OtlpConfig,
    stats: Arc<WorkerStats>,
) -> (mpsc::Sender<WorkerCommand>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<WorkerCommand>();
    let handle = thread::Builder::new()
        .name("fern-otlp-worker".into())
        .spawn(move || worker_loop(rx, config, stats))
        .expect("spawn fern-otlp-worker");
    (tx, handle)
}

fn worker_loop(
    rx: mpsc::Receiver<WorkerCommand>,
    config: OtlpConfig,
    stats: Arc<WorkerStats>,
) {
    let mut buffer: VecDeque<OwnedEvent> = VecDeque::new();
    let agent = build_agent(config.request_timeout);
    let builder = WireBuilder {
        service_name: config.service_name.clone(),
        service_version: config.service_version.clone(),
    };
    let mut current_backoff = config.initial_backoff;
    let mut last_flush = Instant::now();

    loop {
        let wait = if buffer.is_empty() {
            config.flush_interval
        } else {
            let until_flush = config.flush_interval.saturating_sub(last_flush.elapsed());
            until_flush.min(current_backoff)
        };

        match rx.recv_timeout(wait) {
            Ok(WorkerCommand::Record(event)) => {
                push_with_cap(&mut buffer, event, config.max_queue_size);
                stats.queued.store(buffer.len(), Ordering::Relaxed);
            }
            Ok(WorkerCommand::Flush(reply)) => {
                let result = drain(
                    &mut buffer,
                    &agent,
                    &builder,
                    &config,
                    &stats,
                    &mut current_backoff,
                );
                let _ = reply.send(result);
                last_flush = Instant::now();
            }
            Ok(WorkerCommand::Discard) => {
                buffer.clear();
                stats.queued.store(0, Ordering::Relaxed);
            }
            Ok(WorkerCommand::Shutdown) => {
                let _ = drain(
                    &mut buffer,
                    &agent,
                    &builder,
                    &config,
                    &stats,
                    &mut current_backoff,
                );
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !buffer.is_empty() {
                    let _ = drain(
                        &mut buffer,
                        &agent,
                        &builder,
                        &config,
                        &stats,
                        &mut current_backoff,
                    );
                    last_flush = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = drain(
                    &mut buffer,
                    &agent,
                    &builder,
                    &config,
                    &stats,
                    &mut current_backoff,
                );
                break;
            }
        }

        if buffer.len() >= config.max_batch_size {
            let _ = drain(
                &mut buffer,
                &agent,
                &builder,
                &config,
                &stats,
                &mut current_backoff,
            );
            last_flush = Instant::now();
        }
    }
}

fn push_with_cap(buffer: &mut VecDeque<OwnedEvent>, event: OwnedEvent, cap: usize) {
    if buffer.len() >= cap {
        buffer.pop_front();
    }
    buffer.push_back(event);
}

/// One OTLP request per drain — bundle every queued event into a
/// single `ExportLogsServiceRequest` and POST it. Failure cases:
/// - `Accepted` → empty the buffer, reset backoff
/// - `Retry` → leave events in the buffer, double the backoff
/// - `Drop` → empty the buffer (events are unrecoverable), reset
fn drain(
    buffer: &mut VecDeque<OwnedEvent>,
    agent: &ureq::Agent,
    builder: &WireBuilder,
    config: &OtlpConfig,
    stats: &WorkerStats,
    current_backoff: &mut Duration,
) -> Result<(), String> {
    if buffer.is_empty() {
        return Ok(());
    }
    let take = config.max_batch_size.min(buffer.len());
    let events: Vec<OwnedEvent> = buffer.drain(..take).collect();
    let body = builder.build_body(&events);
    let outcome = send_batch(agent, config, &body);

    match outcome {
        SendOutcome::Accepted => {
            stats
                .accepted
                .fetch_add(events.len(), Ordering::Relaxed);
            *current_backoff = config.initial_backoff;
            stats.queued.store(buffer.len(), Ordering::Relaxed);
            Ok(())
        }
        SendOutcome::Retry(why) => {
            // Push back to the *front* so order is preserved on
            // the next attempt.
            for event in events.into_iter().rev() {
                buffer.push_front(event);
            }
            stats.queued.store(buffer.len(), Ordering::Relaxed);
            *current_backoff = next_backoff(*current_backoff, config.max_backoff);
            Err(why)
        }
        SendOutcome::Drop(why) => {
            stats
                .dropped
                .fetch_add(events.len(), Ordering::Relaxed);
            stats.queued.store(buffer.len(), Ordering::Relaxed);
            Err(why)
        }
    }
}
