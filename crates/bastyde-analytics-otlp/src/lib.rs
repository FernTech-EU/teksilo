//! `bastyde-analytics-otlp` — OpenTelemetry-compatible analytics
//! adapter for Bastyde.
//!
//! Speaks **OTLP/HTTP logs** over **JSON**. Works with any
//! OTel-compatible collector — `otelcol-contrib`, Honeycomb,
//! self-hosted Tempo+Loki via the OTel collector's HTTP receiver.
//!
//! # Mapping
//!
//! ```text
//! Bastyde Event             OTLP LogRecord
//! ──────────────────────── ────────────────────────────────────
//! event.name               body.stringValue
//! event.category           attributes["bastyde.category"]
//! event.timestamp          timeUnixNano (string, OTLP/JSON)
//! event.install_id         resource.service.instance.id (when set)
//! event.session_id         attributes["bastyde.session_id"]
//! event.props.<key>        attributes["bastyde.<key>"]
//! event.schema_version     attributes["bastyde.schema_version"]
//! ```
//!
//! Anonymous-mode batches (no `install_id`) omit
//! `service.instance.id`; the OTel collector treats them as
//! aggregate logs.
//!
//! # Quick start
//!
//! ```ignore
//! use bastyde_analytics_otlp::OtlpAdapter;
//! use std::rc::Rc;
//!
//! let adapter = Rc::new(
//!     OtlpAdapter::builder()
//!         .endpoint("http://127.0.0.1:4318/v1/logs")
//!         .service_name("my.app")
//!         .service_version(env!("CARGO_PKG_VERSION"))
//!         .header("x-honeycomb-team", std::env::var("HONEYCOMB_API_KEY").unwrap())
//!         .build(),
//! ) as Rc<dyn bastyde_telemetry::UsageReporter>;
//! ```
//!
//! # Fetch + erase
//!
//! Backend-dependent — the OTLP spec has no fetch or delete RPC.
//! The adapter returns
//! `TelemetryError::FetchUnsupportedByBackend` /
//! `ErasureUnsupportedByBackend` by default. Operators with a
//! self-hosted Tempo+Loki + an HTTP-side delete API can subclass
//! the adapter (or wait for a future user-provided
//! `EraseEndpoint`/`QueryEndpoint` injection point — tracked under
//! future work).
//!
//! # Queue durability
//!
//! Unlike `bastyde-analytics-plausible` and `bastyde-analytics-bastyde`,
//! this adapter has **no built-in persistent queue**. Pending
//! events live in an in-memory `VecDeque` and are lost on hard
//! exit.
//!
//! This is an intentional design decision, not an oversight:
//!
//! 1. **The OTel deployment model owns durability.** OTLP is
//!    designed to talk to a *collector* — typically `otelcol` or
//!    `otelcol-contrib` running as a sidecar, system service, or
//!    on `localhost:4318`. The collector itself buffers, retries,
//!    and persists (via the `file_storage` extension or its built-
//!    in queue). Layering a second durability tier inside the
//!    desktop adapter duplicates work the collector already does.
//! 2. **Remote-collector scenarios are atypical for desktop apps.**
//!    When an app talks straight to a remote OTLP endpoint (e.g.
//!    Honeycomb, Grafana Cloud), short network blips are absorbed
//!    by the in-memory queue's retry/backoff loop. Multi-hour
//!    offline buffering is not the OTLP HTTP-logs use case.
//!
//! Operators who *need* client-side durability against hard
//! exits can fall back to a local collector with a `file_storage`
//! exporter — the same persistence guarantee the redb-backed
//! adapters provide, but at the right layer of the stack.
//!
//! If a future use case demands client-side durability (offline
//! laptops with no local collector), exposing
//! `OtlpAdapterBuilder::queue(Arc<dyn EventQueue>)` matching the
//! Plausible/Bastyde shape is straightforward — the worker already
//! owns the only `VecDeque` site to swap.

mod config;
mod transport;
mod wire;
mod worker;

pub use config::OtlpConfig;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use bastyde_core::telemetry::{ConsentScope, Event, RemoteDataExport, TelemetryError, UsageReporter};

use crate::worker::{WorkerCommand, WorkerStats, spawn_worker};

pub struct OtlpAdapter {
    config: OtlpConfig,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_handle: Option<JoinHandle<()>>,
    stats: Arc<WorkerStats>,
}

impl OtlpAdapter {
    pub fn builder() -> OtlpAdapterBuilder {
        OtlpAdapterBuilder::default()
    }

    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    pub fn events_accepted(&self) -> usize {
        self.stats.accepted.load(Ordering::Relaxed)
    }

    pub fn events_dropped(&self) -> usize {
        self.stats.dropped.load(Ordering::Relaxed)
    }

    pub fn events_queued(&self) -> usize {
        self.stats.queued.load(Ordering::Relaxed)
    }

    fn send(&self, cmd: WorkerCommand) {
        let _ = self.worker_tx.send(cmd);
    }
}

impl std::fmt::Debug for OtlpAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtlpAdapter")
            .field("endpoint", &self.config.endpoint)
            .field("service_name", &self.config.service_name)
            .field("accepted", &self.events_accepted())
            .field("dropped", &self.events_dropped())
            .field("queued", &self.events_queued())
            .finish()
    }
}

impl UsageReporter for OtlpAdapter {
    fn record(&self, event: &Event<'_>) {
        self.send(WorkerCommand::Record(event.to_owned()));
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.send(WorkerCommand::Flush(tx));
        match rx.recv_timeout(self.config.request_timeout * 3) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(TelemetryError::Other(e)),
            Err(_) => Err(TelemetryError::Other("flush timed out".into())),
        }
    }

    fn discard_pending(&self) -> Result<(), TelemetryError> {
        self.send(WorkerCommand::Discard);
        Ok(())
    }

    fn supported_scopes(&self) -> ConsentScope {
        // OTel collectors are general-purpose — they accept whatever
        // we send. Returning `all()` lets the privacy widget show
        // every per-scope toggle. Operators can constrain at the
        // collector side via processors / attributes filters.
        ConsentScope::all()
    }

    fn install_id(&self) -> Option<&str> {
        // The adapter is stateless w.r.t. install_id — it picks the
        // value off whatever event flows through. App-side
        // pseudonymous-mode sets install_id on the event before
        // record(); anonymous-mode events have None.
        None
    }

    fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    fn adapter_name(&self) -> &'static str {
        "otlp"
    }

    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError> {
        // OTLP has no fetch path. Self-hosted Tempo+Loki can be
        // queried via LogQL but the URL + auth shape is operator-
        // specific. Future work: accept
        // a `QueryEndpoint` config that operators wire to their
        // backend's read API.
        Err(TelemetryError::FetchUnsupportedByBackend)
    }

    fn erase_remote_data(&self) -> Result<(), TelemetryError> {
        Err(TelemetryError::ErasureUnsupportedByBackend)
    }
}

impl Drop for OtlpAdapter {
    fn drop(&mut self) {
        let _ = self.worker_tx.send(WorkerCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Default)]
pub struct OtlpAdapterBuilder {
    config: OtlpConfig,
}

impl std::fmt::Debug for OtlpAdapterBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtlpAdapterBuilder")
            .field("endpoint", &self.config.endpoint)
            .field("service_name", &self.config.service_name)
            .finish()
    }
}

impl OtlpAdapterBuilder {
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.endpoint = url.into();
        self
    }

    /// Apply an operator-set runtime override iff non-empty —
    /// matches the same shape as the Plausible/Bastyde adapters.
    pub fn endpoint_override(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        if !url.is_empty() {
            self.config.endpoint = url;
        }
        self
    }

    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.config.service_name = name.into();
        self
    }

    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.config.service_version = version.into();
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.headers.push((name.into(), value.into()));
        self
    }

    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.config.user_agent = ua.into();
        self
    }

    pub fn max_batch_size(mut self, n: usize) -> Self {
        self.config.max_batch_size = n.max(1);
        self
    }

    pub fn flush_interval(mut self, d: Duration) -> Self {
        self.config.flush_interval = d;
        self
    }

    pub fn request_timeout(mut self, d: Duration) -> Self {
        self.config.request_timeout = d;
        self
    }

    pub fn max_queue_size(mut self, n: usize) -> Self {
        self.config.max_queue_size = n.max(1);
        self
    }

    pub fn build(self) -> OtlpAdapter {
        assert!(
            !self.config.endpoint.is_empty(),
            "OtlpAdapter::builder().endpoint(...) is required",
        );
        assert!(
            !self.config.service_name.is_empty(),
            "OtlpAdapter::builder().service_name(...) is required",
        );
        let stats = Arc::new(WorkerStats::default());
        let (tx, handle) = spawn_worker(self.config.clone(), stats.clone());
        OtlpAdapter {
            config: self.config,
            worker_tx: tx,
            worker_handle: Some(handle),
            stats,
        }
    }
}
