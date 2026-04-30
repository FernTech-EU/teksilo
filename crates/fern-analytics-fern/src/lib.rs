//! FernUI analytics adapter for the home-grown
//! [`fern-collector`](../../../fern-collector) gRPC backend.
//!
//! # Sub-phase A
//!
//! Localhost ingest only — no auth, no TLS. Wire format defined in
//! `fern-collector/proto/telemetry/v1.proto` and consumed via the
//! [`fern_collector_proto`] crate.
//!
//! # Architecture
//!
//! Same layered shape as [`fern-analytics-plausible`]: a sync
//! [`UsageReporter`] surface (called from the FernUI UI thread)
//! pushes events into a [`tokio::sync::mpsc`] channel; a
//! `tokio`-runtime-owned worker task drains the channel, batches
//! events, and forwards them through a tonic bidi-stream to the
//! collector. Acks come back on the response stream and update
//! `accepted` / `dropped` counters.
//!
//! The adapter owns the tokio runtime — apps that already run a
//! tokio runtime (e.g. another adapter) can pass theirs in via
//! [`FernAdapterBuilder::runtime`] to avoid double allocation.

mod config;
mod worker;

pub use config::{FernConfig, TlsClientConfig};

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

use fern_core::telemetry::{
    ConsentScope, Event, RemoteDataExport, TelemetryError, UsageReporter,
};
use fern_telemetry::{EventQueue, InMemoryEventQueue, PersistentEventQueue};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::worker::{WorkerCommand, WorkerStats, spawn_worker};

/// Adapter for the FernUI-operated `fern-collector` gRPC service.
///
/// Construct with [`FernAdapter::builder`]. `Drop` joins the worker
/// task with a best-effort final flush.
pub struct FernAdapter {
    config: FernConfig,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_handle: Option<JoinHandle<()>>,
    runtime: Arc<Runtime>,
    stats: Arc<WorkerStats>,
}

impl FernAdapter {
    pub fn builder() -> FernAdapterBuilder {
        FernAdapterBuilder::default()
    }

    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    pub fn product_id(&self) -> &str {
        &self.config.product_id
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
        // `blocking_send` is the right call from a non-async caller
        // (the FernUI UI thread). Returns Err only if the worker
        // has shut down — at which point dropping the command is
        // the correct behavior.
        let _ = self.worker_tx.blocking_send(cmd);
    }
}

impl std::fmt::Debug for FernAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FernAdapter")
            .field("endpoint", &self.config.endpoint)
            .field("product_id", &self.config.product_id)
            .field("accepted", &self.events_accepted())
            .field("dropped", &self.events_dropped())
            .field("queued", &self.events_queued())
            .finish()
    }
}

impl UsageReporter for FernAdapter {
    fn record(&self, event: &Event<'_>) {
        self.send(WorkerCommand::Record(event.to_owned()));
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        let (tx, rx) = oneshot::channel();
        self.send(WorkerCommand::Flush(tx));
        // Bound the wait so a stuck worker never blocks the UI
        // thread indefinitely. 4× request_timeout matches the
        // Plausible adapter; aligns with the worst-case retry
        // tail of one in-flight RPC.
        let bounded = async {
            tokio::time::timeout(self.config.request_timeout * 4, rx).await
        };
        match self.runtime.block_on(bounded) {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(TelemetryError::Other(e)),
            Ok(Err(_)) => Err(TelemetryError::Other("worker dropped".into())),
            Err(_) => Err(TelemetryError::Other("flush timed out".into())),
        }
    }

    fn discard_pending(&self) -> Result<(), TelemetryError> {
        self.send(WorkerCommand::Discard);
        Ok(())
    }

    fn supported_scopes(&self) -> ConsentScope {
        if self.config.install_id.is_some() {
            // Pseudonymous: full scope is available, including
            // crash reports and feature flags (the latter only
            // really useful with an install_id anyway).
            ConsentScope::all()
        } else {
            ConsentScope::anonymous_metrics_only()
        }
    }

    fn install_id(&self) -> Option<&str> {
        self.config.install_id.as_deref()
    }

    fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    fn adapter_name(&self) -> &'static str {
        "fern-collector"
    }

    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError> {
        let Some(install_id) = self.config.install_id.clone() else {
            return Err(TelemetryError::FetchUnsupported);
        };
        let endpoint = self.config.endpoint.clone();
        let product_id = self.config.product_id.clone();
        let bearer = self.config.bearer_token.clone();
        let tls = self.config.tls.clone();
        let timeout = self.config.request_timeout;
        let schema_version = self.config.schema_version;
        let install_id_for_export = install_id.clone();
        let endpoint_for_export = endpoint.clone();

        let result: Result<RemoteDataExport, TelemetryError> = self
            .runtime
            .block_on(async move {
                worker::fetch_via_grpc(
                    &endpoint,
                    &product_id,
                    &install_id,
                    bearer.as_deref(),
                    tls.as_ref(),
                    timeout,
                )
                .await
                .map_err(TelemetryError::Other)
                .map(|events| RemoteDataExport {
                    install_id: install_id_for_export,
                    fetched_at: std::time::SystemTime::now(),
                    adapter: "fern-collector",
                    endpoint: endpoint_for_export,
                    schema_version,
                    events,
                    server_metadata: Default::default(),
                })
            });
        result
    }

    fn erase_remote_data(&self) -> Result<(), TelemetryError> {
        let Some(install_id) = self.config.install_id.clone() else {
            return Err(TelemetryError::ErasureUnsupported);
        };
        let endpoint = self.config.endpoint.clone();
        let product_id = self.config.product_id.clone();
        let bearer = self.config.bearer_token.clone();
        let tls = self.config.tls.clone();
        let timeout = self.config.request_timeout;

        self.runtime
            .block_on(async move {
                worker::erase_via_grpc(
                    &endpoint,
                    &product_id,
                    &install_id,
                    bearer.as_deref(),
                    tls.as_ref(),
                    timeout,
                )
                .await
            })
            .map_err(TelemetryError::Other)?;
        Ok(())
    }
}

impl Drop for FernAdapter {
    fn drop(&mut self) {
        // Best-effort final flush + shutdown signal. The worker
        // drains the queue once more on receiving Shutdown.
        let _ = self.worker_tx.blocking_send(WorkerCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            // Worker is a std::thread joining a tokio task — short
            // timeout via a thread-local block. Worst-case we leak
            // the worker for a few seconds; events are persisted in
            // the queue so nothing is lost.
            let _ = handle.join();
        }
    }
}

// ---------------- Builder ----------------

#[derive(Default)]
pub struct FernAdapterBuilder {
    config: FernConfig,
    queue_path: Option<std::path::PathBuf>,
    explicit_queue: Option<Arc<dyn EventQueue>>,
    runtime: Option<Arc<Runtime>>,
}

impl std::fmt::Debug for FernAdapterBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FernAdapterBuilder")
            .field("config", &self.config)
            .field("queue_path", &self.queue_path)
            .field("explicit_queue", &self.explicit_queue.is_some())
            .field("runtime", &self.runtime.is_some())
            .finish()
    }
}

impl FernAdapterBuilder {
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.endpoint = endpoint.into();
        self
    }

    /// If the given override URL is non-empty, replace this builder's
    /// endpoint. No-op when the override is empty.
    ///
    /// Designed to receive the value of the
    /// `fern_telemetry::scopes::TELEMETRY_ENDPOINT_OVERRIDE` settings
    /// key — but typed as a plain string so apps can also source it
    /// from env vars, CLI flags, or per-deployment config.
    ///
    /// Typical wiring:
    ///
    /// ```ignore
    /// let override_url = settings
    ///     .signal_for(&fern_telemetry::scopes::TELEMETRY_ENDPOINT_OVERRIDE)
    ///     .get();
    /// let adapter = FernAdapter::builder()
    ///     .endpoint("https://collector.default.example/")
    ///     .endpoint_override(override_url)  // applies iff non-empty
    ///     .product_id("my.app")
    ///     .build();
    /// ```
    ///
    /// Applies at builder time only — the worker spawns with the
    /// resolved endpoint baked in. Live runtime endpoint changes are
    /// out of scope (an app that needs them should rebuild the
    /// adapter and replace it in `app_state`).
    pub fn endpoint_override(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        if !url.is_empty() {
            self.config.endpoint = url;
        }
        self
    }

    pub fn product_id(mut self, product_id: impl Into<String>) -> Self {
        self.config.product_id = product_id.into();
        self
    }

    pub fn schema_version(mut self, v: u32) -> Self {
        self.config.schema_version = v;
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

    /// Bearer token sent in the gRPC `Authorization` metadata of
    /// every request. Format: `fct_<id>_<secret>` as minted by
    /// `fern-collector token mint`. Server rejects unauthenticated
    /// requests when `--tokens-db` is configured.
    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.config.bearer_token = Some(token.into());
        self
    }

    /// TLS client config. When set, the endpoint must use
    /// `https://`. The default empty config trusts the system
    /// root store; pass [`TlsClientConfig::ca_pem`] for a private
    /// or self-signed CA, and `client_cert_pem` + `client_key_pem`
    /// for mTLS.
    pub fn tls(mut self, tls: TlsClientConfig) -> Self {
        self.config.tls = Some(tls);
        self
    }

    /// Set the per-install pseudonymous identifier. Switches the
    /// adapter into pseudonymous mode: every batch is tagged
    /// `pseudonymous`, every event carries this install_id, and
    /// `fetch_remote_data` / `erase_remote_data` start working.
    /// When unset, the adapter runs anonymous-only.
    pub fn install_id(mut self, install_id: impl Into<String>) -> Self {
        self.config.install_id = Some(install_id.into());
        self
    }

    /// Use a redb-backed persistent queue at the given path. Events
    /// recorded but not yet sent will survive process restart.
    pub fn persistent_queue_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.queue_path = Some(path.into());
        self.explicit_queue = None;
        self
    }

    /// Explicit queue. Mutually exclusive with [`Self::persistent_queue_path`].
    pub fn queue(mut self, queue: Arc<dyn EventQueue>) -> Self {
        self.explicit_queue = Some(queue);
        self.queue_path = None;
        self
    }

    /// Reuse an existing tokio runtime. If unset, the adapter
    /// creates its own multi-threaded runtime (1 worker thread).
    pub fn runtime(mut self, rt: Arc<Runtime>) -> Self {
        self.runtime = Some(rt);
        self
    }

    pub fn build(self) -> FernAdapter {
        assert!(
            !self.config.endpoint.is_empty(),
            "FernAdapter::builder().endpoint(...) is required",
        );
        assert!(
            !self.config.product_id.is_empty(),
            "FernAdapter::builder().product_id(...) is required",
        );

        let queue: Arc<dyn EventQueue> = match (self.explicit_queue, self.queue_path) {
            (Some(q), _) => q,
            (None, Some(path)) => Arc::new(
                PersistentEventQueue::open_with(
                    path,
                    self.config.max_queue_size,
                    Duration::from_secs(60 * 60 * 24 * 7),
                )
                .expect("FernAdapterBuilder::persistent_queue_path: open redb"),
            ),
            (None, None) => {
                Arc::new(InMemoryEventQueue::with_capacity(self.config.max_queue_size))
            }
        };

        let runtime = self.runtime.unwrap_or_else(|| {
            Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .thread_name("fern-collector-worker")
                    .enable_all()
                    .build()
                    .expect("build tokio runtime for FernAdapter"),
            )
        });

        let stats = Arc::new(WorkerStats::default());
        let (tx, handle) =
            spawn_worker(self.config.clone(), queue, runtime.clone(), stats.clone());

        FernAdapter {
            config: self.config,
            worker_tx: tx,
            worker_handle: Some(handle),
            runtime,
            stats,
        }
    }
}
