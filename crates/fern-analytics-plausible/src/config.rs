//! Adapter configuration.

use std::time::Duration;

/// Static configuration for the Plausible adapter.
///
/// Held immutably by the adapter and shared with the worker thread.
/// Set via [`PlausibleAdapterBuilder`](crate::PlausibleAdapterBuilder).
#[derive(Clone, Debug)]
pub struct PlausibleConfig {
    /// Plausible event-ingest endpoint. Default:
    /// `https://plausible.io/api/event` (EU-hosted by vendor).
    pub endpoint: String,
    /// Plausible site identifier (the domain configured in the
    /// Plausible dashboard).
    pub domain: String,
    /// `User-Agent` sent with every request. Plausible uses this
    /// (plus the source IP and a daily server-held salt) to derive
    /// the per-day session hash. Stable across requests within one
    /// process lifetime; varies by app and platform.
    pub user_agent: String,
    /// Maximum events per HTTP batch. Plausible has no batch
    /// endpoint per se, but the worker fires up to this many
    /// concurrent send-and-await calls before yielding.
    pub max_batch_size: usize,
    /// Worker flushes the buffer at least this often, even if the
    /// batch threshold isn't reached.
    pub flush_interval: Duration,
    /// Initial retry delay after a transport failure. Doubles each
    /// failed attempt up to `max_backoff`.
    pub initial_backoff: Duration,
    /// Cap on the exponential backoff.
    pub max_backoff: Duration,
    /// Per-request HTTP timeout (connect + read).
    pub request_timeout: Duration,
    /// Cap on the in-memory queue size. Oldest events are dropped
    /// past this point. Defaults to 10_000.
    pub max_queue_size: usize,
    /// Synthetic URL scheme used in the `url` field of the wire
    /// payload. Plausible expects a URL even for non-web events;
    /// desktop apps use a synthetic `app://` URL by default.
    pub synthetic_url_scheme: String,
}

impl Default for PlausibleConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://plausible.io/api/event".to_string(),
            domain: String::new(),
            user_agent: format!(
                "fern-ui/{} ({} {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
            ),
            max_batch_size: 50,
            flush_interval: Duration::from_secs(60),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60 * 60),
            request_timeout: Duration::from_secs(10),
            max_queue_size: 10_000,
            synthetic_url_scheme: "app".to_string(),
        }
    }
}
