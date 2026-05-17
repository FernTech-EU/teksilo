//! End-to-end tests for the Plausible adapter.
//!
//! Exercises the full flow against a tiny mock HTTP server bound to
//! a system-allocated port: record → worker → POST → parse received
//! body → assert.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use bastyde_analytics_plausible::PlausibleAdapter;
use bastyde_core::telemetry::{Event, EventCategory, IntentSource, Prop, PropValue, UsageReporter};

/// Recorded request metadata for assertions.
#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

#[derive(Default)]
struct MockServerState {
    captured: Mutex<Vec<CapturedRequest>>,
    /// When > 0, the next N requests fail with this status. Decrements
    /// after each failure-mode response.
    fail_next: Mutex<usize>,
    fail_status: Mutex<u16>,
}

impl MockServerState {
    fn capture(&self, req: CapturedRequest) {
        self.captured.lock().unwrap().push(req);
    }
    fn captured_count(&self) -> usize {
        self.captured.lock().unwrap().len()
    }
    fn captured(&self) -> Vec<CapturedRequest> {
        self.captured.lock().unwrap().clone()
    }
    fn arm_failures(&self, count: usize, status: u16) {
        *self.fail_next.lock().unwrap() = count;
        *self.fail_status.lock().unwrap() = status;
    }
    fn take_failure_status(&self) -> Option<u16> {
        let mut count = self.fail_next.lock().unwrap();
        if *count == 0 {
            return None;
        }
        *count -= 1;
        Some(*self.fail_status.lock().unwrap())
    }
}

struct MockServer {
    port: u16,
    state: Arc<MockServerState>,
    _handle: thread::JoinHandle<()>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl MockServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(MockServerState::default());
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let state_thread = state.clone();
        let shutdown_thread = shutdown.clone();
        let handle = thread::spawn(move || {
            run_server(listener, state_thread, shutdown_thread);
        });
        Self {
            port,
            state,
            _handle: handle,
            shutdown,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/api/event", self.port)
    }

    fn wait_for_captured(&self, n: usize, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.state.captured_count() >= n {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn run_server(
    listener: TcpListener,
    state: Arc<MockServerState>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) {
    while !shutdown.load(std::sync::atomic::Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let state = state.clone();
                thread::spawn(move || handle_request(stream, state));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_request(mut stream: TcpStream, state: Arc<MockServerState>) {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut buf = [0u8; 8192];
    let mut accumulated = Vec::new();
    // Read until we have headers + (Content-Length) bytes.
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        accumulated.extend_from_slice(&buf[..n]);
        if let Some(headers_end) = find_headers_end(&accumulated) {
            let headers = std::str::from_utf8(&accumulated[..headers_end]).unwrap_or("");
            let content_length = parse_content_length(headers).unwrap_or(0);
            let body_start = headers_end + 4;
            if accumulated.len() >= body_start + content_length {
                let request_line = headers.lines().next().unwrap_or("");
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let body =
                    String::from_utf8_lossy(&accumulated[body_start..body_start + content_length])
                        .to_string();
                state.capture(CapturedRequest { method, path, body });
                let response = match state.take_failure_status() {
                    Some(s) => format!(
                        "HTTP/1.1 {s} Failed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                    None => {
                        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    }
                };
                let _ = stream.write_all(response.as_bytes());
                return;
            }
        }
        if accumulated.len() > 65_536 {
            break;
        }
    }
}

fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

fn make_event<'a>(name: &'static str, props: &'a [Prop<'a>]) -> Event<'a> {
    Event {
        name,
        category: EventCategory::Intent,
        timestamp: SystemTime::UNIX_EPOCH,
        install_id: None,
        session_id: "test-session",
        schema_version: 1,
        props,
    }
}

#[test]
fn record_then_flush_posts_to_endpoint() {
    let server = MockServer::start();
    let adapter = PlausibleAdapter::builder()
        .endpoint(server.endpoint())
        .domain("test.app")
        .max_batch_size(1)
        .flush_interval(Duration::from_millis(50))
        .request_timeout(Duration::from_secs(2))
        .build();

    let props = [
        Prop {
            key: "name",
            value: PropValue::StaticStr("app.save"),
        },
        Prop {
            key: "source",
            value: PropValue::Enum {
                variant: IntentSource::Shortcut.as_str(),
            },
        },
    ];
    adapter.record(&make_event("intent.dispatched", &props));

    adapter.flush().unwrap();

    assert!(server.wait_for_captured(1, Duration::from_secs(2)));
    let captured = server.state.captured();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].method, "POST");
    assert_eq!(captured[0].path, "/api/event");
    let parsed: serde_json::Value = serde_json::from_str(&captured[0].body).unwrap();
    assert_eq!(parsed["name"], "intent.dispatched");
    assert_eq!(parsed["domain"], "test.app");
    assert_eq!(parsed["url"], "app://test.app/intent.dispatched");
    assert_eq!(parsed["props"]["name"], "app.save");
    assert_eq!(parsed["props"]["source"], "shortcut");

    assert_eq!(adapter.events_accepted(), 1);
    assert_eq!(adapter.events_dropped(), 0);
}

#[test]
fn discard_pending_drops_buffer_without_sending() {
    let server = MockServer::start();
    // Long flush interval so the buffer doesn't auto-drain.
    let adapter = PlausibleAdapter::builder()
        .endpoint(server.endpoint())
        .domain("test.app")
        .max_batch_size(1000)
        .flush_interval(Duration::from_secs(60))
        .build();

    for _ in 0..5 {
        adapter.record(&make_event("intent.dispatched", &[]));
    }
    // Give the worker time to receive the records.
    thread::sleep(Duration::from_millis(100));
    adapter.discard_pending().unwrap();

    // Now flush — there should be nothing to send.
    adapter.flush().unwrap();
    thread::sleep(Duration::from_millis(100));

    assert_eq!(server.state.captured_count(), 0);
    assert_eq!(adapter.events_accepted(), 0);
}

#[test]
fn drops_4xx_without_retry() {
    let server = MockServer::start();
    server.state.arm_failures(10, 400); // every request fails with 400
    let adapter = PlausibleAdapter::builder()
        .endpoint(server.endpoint())
        .domain("test.app")
        .max_batch_size(1)
        .flush_interval(Duration::from_millis(50))
        .request_timeout(Duration::from_secs(2))
        .build();

    adapter.record(&make_event("intent.dispatched", &[]));
    adapter.flush().ok();

    assert!(server.wait_for_captured(1, Duration::from_secs(2)));
    // Only one POST attempt (no retry on 4xx).
    thread::sleep(Duration::from_millis(200));
    assert_eq!(server.state.captured_count(), 1);
    assert_eq!(adapter.events_dropped(), 1);
    assert_eq!(adapter.events_accepted(), 0);
}

#[test]
fn retries_5xx_then_succeeds() {
    let server = MockServer::start();
    server.state.arm_failures(2, 503); // first two requests fail, then 200
    let adapter = PlausibleAdapter::builder()
        .endpoint(server.endpoint())
        .domain("test.app")
        .max_batch_size(1)
        .flush_interval(Duration::from_millis(50))
        // Tiny backoff so the test runs quickly.
        .initial_backoff(Duration::from_millis(20))
        .max_backoff(Duration::from_millis(100))
        .request_timeout(Duration::from_secs(2))
        .build();

    adapter.record(&make_event("intent.dispatched", &[]));

    // Wait for the eventual success.
    let success = (0..50).any(|_| {
        let _ = adapter.flush();
        thread::sleep(Duration::from_millis(50));
        adapter.events_accepted() == 1
    });
    assert!(
        success,
        "event should eventually be accepted after 5xx retries"
    );
    assert!(server.state.captured_count() >= 3);
}

#[test]
fn shutdown_drains_pending_events() {
    let server = MockServer::start();
    {
        let adapter = PlausibleAdapter::builder()
            .endpoint(server.endpoint())
            .domain("test.app")
            .max_batch_size(1000)
            .flush_interval(Duration::from_secs(60))
            .build();
        for _ in 0..3 {
            adapter.record(&make_event("intent.dispatched", &[]));
        }
        // Don't call flush — let Drop handle the final drain.
        thread::sleep(Duration::from_millis(50));
    }
    // Adapter dropped — worker should have drained.
    assert!(server.wait_for_captured(3, Duration::from_secs(3)));
}

#[test]
fn events_persist_across_process_restart() {
    // Simulates a hard exit: phase 1 records events to a persistent
    // queue against an unreachable endpoint (so the worker's drop-
    // time flush fails and events stay queued). Phase 2 spins up a
    // mock server, opens a new adapter at the same queue path, and
    // verifies the queued events flush.
    let dir = tempfile::tempdir().unwrap();
    let queue_path = dir.path().join("plausible-queue.redb");

    // Phase 1 — record while no server is listening.
    {
        let adapter = PlausibleAdapter::builder()
            // Port 1 is the standard "unreachable" port (TCPMUX, almost
            // always closed). Tight timeout so the test isn't slow.
            .endpoint("http://127.0.0.1:1/api/event")
            .domain("test.app")
            .max_batch_size(1)
            .flush_interval(Duration::from_secs(60))
            .request_timeout(Duration::from_millis(100))
            .initial_backoff(Duration::from_millis(50))
            .max_backoff(Duration::from_millis(200))
            .persistent_queue_path(&queue_path)
            .build();

        for i in 0..3 {
            adapter.record(&make_event(
                if i == 0 {
                    "intent.dispatched"
                } else if i == 1 {
                    "lifecycle.app_started"
                } else {
                    "lifecycle.app_exited"
                },
                &[],
            ));
        }
        // Wait long enough for the worker to receive all three records.
        thread::sleep(Duration::from_millis(200));
        // Adapter drops — flush attempts fail (server unreachable),
        // events stay in the queue file.
    }

    // Verify the queue file actually has the events on disk.
    assert!(queue_path.exists(), "queue file should persist");

    // Phase 2 — server is up, new adapter opens the same queue, drains.
    let server = MockServer::start();
    let adapter = PlausibleAdapter::builder()
        .endpoint(server.endpoint())
        .domain("test.app")
        .max_batch_size(10)
        .flush_interval(Duration::from_millis(50))
        .request_timeout(Duration::from_secs(2))
        .persistent_queue_path(&queue_path)
        .build();

    // Trigger flush; the queue is non-empty from phase 1.
    adapter.flush().unwrap();

    assert!(
        server.wait_for_captured(3, Duration::from_secs(3)),
        "all 3 events from phase 1 should be flushed by phase 2 \
         (received {})",
        server.state.captured_count()
    );
    let names: Vec<String> = server
        .state
        .captured()
        .iter()
        .map(|r| {
            let parsed: serde_json::Value = serde_json::from_str(&r.body).unwrap();
            parsed["name"].as_str().unwrap_or("").to_string()
        })
        .collect();
    assert!(names.contains(&"intent.dispatched".to_string()));
    assert!(names.contains(&"lifecycle.app_started".to_string()));
    assert!(names.contains(&"lifecycle.app_exited".to_string()));
}
