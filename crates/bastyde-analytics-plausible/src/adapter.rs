// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `PlausibleAdapter` — the public type that implements
//! [`bastyde_core::telemetry::UsageReporter`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use bastyde_core::telemetry::{
    ConsentScope, Event, RemoteDataExport, TelemetryError, UsageReporter,
};
use bastyde_telemetry::{EventQueue, InMemoryEventQueue, PersistentEventQueue};

use crate::config::PlausibleConfig;
use crate::worker::{WorkerCommand, WorkerStats, spawn_worker};

/// Plausible-backed [`UsageReporter`].
///
/// Anonymous-by-design: `install_id()` is always `None`,
/// `fetch_remote_data` and `erase_remote_data` always return their
/// `*Unsupported` errors. Construct via [`builder`](Self::builder).
pub struct PlausibleAdapter {
    config: PlausibleConfig,
    worker_tx: mpsc::Sender<WorkerCommand>,
    worker_handle: Option<JoinHandle<()>>,
    stats: Arc<WorkerStats>,
}

impl PlausibleAdapter {
    pub fn builder() -> PlausibleAdapterBuilder {
        PlausibleAdapterBuilder::default()
    }

    pub fn config(&self) -> &PlausibleConfig {
        &self.config
    }

    /// Number of events successfully accepted by the server.
    /// Useful for tests and for the privacy widget's
    /// "events sent: N" counter.
    pub fn events_accepted(&self) -> usize {
        self.stats.accepted.load(Ordering::Relaxed)
    }

    /// Number of events permanently dropped (4xx, malformed, etc.).
    pub fn events_dropped(&self) -> usize {
        self.stats.dropped.load(Ordering::Relaxed)
    }

    /// Number of events currently buffered in the worker, awaiting
    /// flush. Includes events that are waiting on a backoff window.
    pub fn events_queued(&self) -> usize {
        self.stats.queued.load(Ordering::Relaxed)
    }
}

impl Drop for PlausibleAdapter {
    fn drop(&mut self) {
        let _ = self.worker_tx.send(WorkerCommand::Shutdown);
        if let Some(handle) = self.worker_handle.take() {
            // Best-effort join — if the worker is wedged we don't
            // want shutdown to hang. A short timeout would be nicer
            // but std doesn't offer it; rely on the request_timeout
            // bounding individual sends.
            let _ = handle.join();
        }
    }
}

impl UsageReporter for PlausibleAdapter {
    fn record(&self, event: &Event<'_>) {
        let owned = event.to_owned();
        // Best-effort send; channel only fails if the worker is
        // gone, which means the adapter is being dropped — drop the
        // event silently.
        let _ = self.worker_tx.send(WorkerCommand::Record(owned));
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        let (tx, rx) = mpsc::sync_channel::<Result<(), String>>(0);
        if self.worker_tx.send(WorkerCommand::Flush(tx)).is_err() {
            return Err(TelemetryError::Other(
                "plausible worker has exited".to_string(),
            ));
        }
        match rx.recv_timeout(self.config.request_timeout * 4) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(TelemetryError::Other(e)),
            Err(_) => Err(TelemetryError::Other("flush timed out".into())),
        }
    }

    fn discard_pending(&self) -> Result<(), TelemetryError> {
        if self.worker_tx.send(WorkerCommand::Discard).is_err() {
            return Err(TelemetryError::Other(
                "plausible worker has exited".to_string(),
            ));
        }
        Ok(())
    }

    fn erase_remote_data(&self) -> Result<(), TelemetryError> {
        // Anonymous-by-design: nothing linkable on the server.
        Err(TelemetryError::ErasureUnsupported)
    }

    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError> {
        Err(TelemetryError::FetchUnsupported)
    }

    fn install_id(&self) -> Option<&str> {
        None
    }

    fn adapter_name(&self) -> &'static str {
        "plausible"
    }

    fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    fn supported_scopes(&self) -> ConsentScope {
        ConsentScope::anonymous_metrics_only()
    }
}

impl std::fmt::Debug for PlausibleAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlausibleAdapter")
            .field("endpoint", &self.config.endpoint)
            .field("domain", &self.config.domain)
            .field("queued", &self.events_queued())
            .field("accepted", &self.events_accepted())
            .finish()
    }
}

// ---------------- Builder ----------------

#[derive(Default)]
pub struct PlausibleAdapterBuilder {
    config: PlausibleConfig,
    queue_path: Option<PathBuf>,
    explicit_queue: Option<Arc<dyn EventQueue>>,
}

impl std::fmt::Debug for PlausibleAdapterBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlausibleAdapterBuilder")
            .field("config", &self.config)
            .field("queue_path", &self.queue_path)
            .field("explicit_queue", &self.explicit_queue.is_some())
            .finish()
    }
}

impl PlausibleAdapterBuilder {
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.config.endpoint = url.into();
        self
    }

    /// If the given override URL is non-empty, replace this builder's
    /// endpoint. No-op when the override is empty. Designed to be
    /// fed the `bastyde_telemetry::scopes::TELEMETRY_ENDPOINT_OVERRIDE`
    /// settings value so an operator-set runtime override applies
    /// without the app having to special-case `String::is_empty()`.
    pub fn endpoint_override(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        if !url.is_empty() {
            self.config.endpoint = url;
        }
        self
    }

    pub fn domain(mut self, name: impl Into<String>) -> Self {
        self.config.domain = name.into();
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

    pub fn initial_backoff(mut self, d: Duration) -> Self {
        self.config.initial_backoff = d;
        self
    }

    pub fn max_backoff(mut self, d: Duration) -> Self {
        self.config.max_backoff = d;
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

    pub fn synthetic_url_scheme(mut self, scheme: impl Into<String>) -> Self {
        self.config.synthetic_url_scheme = scheme.into();
        self
    }

    /// Use a redb-backed persistent queue at the given path. Events
    /// recorded but not yet sent will survive process restart and be
    /// flushed on next launch. Without this, the worker uses an
    /// in-memory queue and loses pending events on hard exit.
    ///
    /// Typical path:
    /// `paths.data_dir().join("bastyde-telemetry/plausible-queue.redb")`.
    /// The parent directory is created if missing.
    ///
    /// Mutually exclusive with [`Self::queue`]; the last call wins.
    pub fn persistent_queue_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.queue_path = Some(path.into());
        self.explicit_queue = None;
        self
    }

    /// Use an explicit queue implementation. Overrides
    /// [`Self::persistent_queue_path`]. Useful for tests that want
    /// to inspect queue state, or for sharing one queue across
    /// multiple adapters.
    pub fn queue(mut self, queue: Arc<dyn EventQueue>) -> Self {
        self.explicit_queue = Some(queue);
        self.queue_path = None;
        self
    }

    /// Spawns the worker thread. Panics if the OS refuses to spawn
    /// (out of resources) or if a `persistent_queue_path` was set
    /// but the file can't be opened.
    pub fn build(self) -> PlausibleAdapter {
        assert!(
            !self.config.domain.is_empty(),
            "PlausibleAdapter::builder().domain(...) is required",
        );
        assert!(
            !self.config.endpoint.is_empty(),
            "PlausibleAdapter::builder().endpoint(...) cannot be empty",
        );
        let queue: Arc<dyn EventQueue> = match (self.explicit_queue, self.queue_path) {
            (Some(q), _) => q,
            (None, Some(path)) => {
                let q = PersistentEventQueue::open_with(
                    path,
                    self.config.max_queue_size,
                    Duration::from_secs(60 * 60 * 24 * 7),
                )
                .expect(
                    "PlausibleAdapter::builder().persistent_queue_path(...): \
                     could not open redb file",
                );
                Arc::new(q)
            }
            (None, None) => Arc::new(InMemoryEventQueue::with_capacity(
                self.config.max_queue_size,
            )),
        };
        let stats = Arc::new(WorkerStats::default());
        let (tx, handle) = spawn_worker(self.config.clone(), queue, stats.clone());
        PlausibleAdapter {
            config: self.config,
            worker_tx: tx,
            worker_handle: Some(handle),
            stats,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "domain(...)")]
    fn build_panics_without_domain() {
        let _ = PlausibleAdapter::builder().endpoint("http://x").build();
    }

    #[test]
    fn defaults_are_anonymous() {
        let a = PlausibleAdapter::builder()
            .endpoint("http://x")
            .domain("test.app")
            .build();
        assert!(a.install_id().is_none());
        assert_eq!(a.adapter_name(), "plausible");
        assert!(a.supported_scopes().anonymous_metrics);
        assert!(matches!(
            a.erase_remote_data(),
            Err(TelemetryError::ErasureUnsupported)
        ));
        assert!(matches!(
            a.fetch_remote_data(),
            Err(TelemetryError::FetchUnsupported)
        ));
    }
}
