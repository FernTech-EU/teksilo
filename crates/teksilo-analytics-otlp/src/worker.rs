// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Background worker — drains the queue, batches into one OTLP
//! `ExportLogsServiceRequest` per flush, retries on transient
//! failure with exponential backoff.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use teksilo_core::telemetry::OwnedEvent;

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
        .name("teksilo-otlp-worker".into())
        .spawn(move || worker_loop(rx, config, stats))
        .expect("spawn teksilo-otlp-worker");
    (tx, handle)
}

fn worker_loop(rx: mpsc::Receiver<WorkerCommand>, config: OtlpConfig, stats: Arc<WorkerStats>) {
    let mut buffer: VecDeque<OwnedEvent> = VecDeque::new();
    let agent = build_agent(config.request_timeout);
    let builder = WireBuilder {
        service_name: config.service_name.clone(),
        service_version: config.service_version.clone(),
    };
    let mut current_backoff = config.initial_backoff;
    // Set by a `Retry`: the full-batch path below sends nothing before it,
    // so an outage costs one request per backoff, not one per event.
    let mut retry_at: Option<Instant> = None;
    let mut last_flush = Instant::now();

    loop {
        let wait = if buffer.is_empty() {
            config.flush_interval
        } else {
            let until_flush = config.flush_interval.saturating_sub(last_flush.elapsed());
            until_flush.min(current_backoff)
        };

        let mut drain_now = |buffer: &mut VecDeque<OwnedEvent>| {
            drain(
                buffer,
                &agent,
                &builder,
                &config,
                &stats,
                &mut current_backoff,
                &mut retry_at,
            )
        };

        match rx.recv_timeout(wait) {
            Ok(WorkerCommand::Record(event)) => {
                push_with_cap(&mut buffer, event, config.max_queue_size);
                stats.queued.store(buffer.len(), Ordering::Relaxed);
            }
            Ok(WorkerCommand::Flush(reply)) => {
                let result = drain_now(&mut buffer);
                let _ = reply.send(result);
                last_flush = Instant::now();
            }
            Ok(WorkerCommand::Discard) => {
                buffer.clear();
                stats.queued.store(0, Ordering::Relaxed);
            }
            Ok(WorkerCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                // `drain` sends at most `max_batch_size` events, so one call
                // is not a final flush: the surplus would die with the
                // worker. Keep draining while each pass shrinks the buffer;
                // a pass that does not (empty buffer, or a `Retry` that put
                // its batch back) ends it, so exit costs at most one failed
                // request.
                loop {
                    let before = buffer.len();
                    let _ = drain_now(&mut buffer);
                    if buffer.len() >= before {
                        break;
                    }
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !buffer.is_empty() {
                    let _ = drain_now(&mut buffer);
                    last_flush = Instant::now();
                }
            }
        }

        let backing_off = retry_at.is_some_and(|t| Instant::now() < t);
        if buffer.len() >= config.max_batch_size && !backing_off {
            let _ = drain(
                &mut buffer,
                &agent,
                &builder,
                &config,
                &stats,
                &mut current_backoff,
                &mut retry_at,
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
/// - `Retry` → leave events in the buffer, double the backoff and set
///   `retry_at` to the end of it
/// - `Drop` → empty the buffer (events are unrecoverable), reset
fn drain(
    buffer: &mut VecDeque<OwnedEvent>,
    agent: &ureq::Agent,
    builder: &WireBuilder,
    config: &OtlpConfig,
    stats: &WorkerStats,
    current_backoff: &mut Duration,
    retry_at: &mut Option<Instant>,
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
            stats.accepted.fetch_add(events.len(), Ordering::Relaxed);
            *current_backoff = config.initial_backoff;
            *retry_at = None;
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
            *retry_at = Some(Instant::now() + *current_backoff);
            Err(why)
        }
        SendOutcome::Drop(why) => {
            stats.dropped.fetch_add(events.len(), Ordering::Relaxed);
            stats.queued.store(buffer.len(), Ordering::Relaxed);
            Err(why)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicU16;
    use teksilo_core::telemetry::EventCategory;

    /// A collector on a local port: answers every POST with `status`, and
    /// counts the requests.
    struct Collector {
        endpoint: String,
        status: Arc<AtomicU16>,
        requests: Arc<AtomicUsize>,
    }

    impl Collector {
        fn start(status: u16) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/v1/logs", listener.local_addr().unwrap());
            let status = Arc::new(AtomicU16::new(status));
            let requests = Arc::new(AtomicUsize::new(0));
            let (status_t, requests_t) = (status.clone(), requests.clone());
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    // Read headers, then the body its Content-Length names.
                    let complete = loop {
                        let Ok(n) = stream.read(&mut chunk) else {
                            break false;
                        };
                        if n == 0 {
                            break false;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                            continue;
                        };
                        let headers = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                        let length: usize = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + length {
                            break true;
                        }
                    };
                    if complete {
                        requests_t.fetch_add(1, Ordering::SeqCst);
                        let code = status_t.load(Ordering::SeqCst);
                        let _ = stream.write_all(
                            format!(
                                "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        );
                    }
                }
            });
            Self {
                endpoint,
                status,
                requests,
            }
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    fn config(endpoint: &str) -> OtlpConfig {
        OtlpConfig {
            endpoint: endpoint.to_string(),
            service_name: "test".into(),
            max_batch_size: 50,
            // Nothing is sent on a timer during a test, and a failed send
            // backs off for the rest of it.
            flush_interval: Duration::from_secs(3600),
            initial_backoff: Duration::from_secs(3600),
            request_timeout: Duration::from_secs(5),
            ..OtlpConfig::default()
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

    fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// A worker backing off after a failed send does not send again when
    /// the next event arrives, even with a full batch queued.
    #[test]
    fn a_new_event_during_backoff_sends_nothing() {
        let collector = Collector::start(503);
        let stats = Arc::new(WorkerStats::default());
        let (tx, handle) = spawn_worker(config(&collector.endpoint), stats.clone());

        for _ in 0..50 {
            tx.send(WorkerCommand::Record(event())).unwrap();
        }
        wait_until("the full batch's failed send", || collector.requests() == 1);
        tx.send(WorkerCommand::Record(event())).unwrap();
        wait_until("the 51st event", || {
            stats.queued.load(Ordering::Relaxed) == 51
        });
        thread::sleep(Duration::from_millis(200));
        assert_eq!(
            collector.requests(),
            1,
            "an event during backoff sent a request"
        );

        tx.send(WorkerCommand::Shutdown).unwrap();
        handle.join().unwrap();
    }

    /// Shutdown sends everything queued, not just one batch.
    #[test]
    fn shutdown_sends_every_queued_event() {
        // The collector refuses at first, so events pile up past one batch
        // while the worker backs off; it is back up by the time of exit.
        let collector = Collector::start(503);
        let stats = Arc::new(WorkerStats::default());
        let (tx, handle) = spawn_worker(config(&collector.endpoint), stats.clone());

        for _ in 0..200 {
            tx.send(WorkerCommand::Record(event())).unwrap();
        }
        wait_until("all 200 events queued", || {
            stats.queued.load(Ordering::Relaxed) == 200
        });
        collector.status.store(200, Ordering::SeqCst);
        tx.send(WorkerCommand::Shutdown).unwrap();
        handle.join().unwrap();

        assert_eq!(stats.accepted.load(Ordering::Relaxed), 200);
        assert_eq!(stats.queued.load(Ordering::Relaxed), 0);
    }
}
