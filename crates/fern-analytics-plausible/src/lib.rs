//! `fern-analytics-plausible` — Plausible adapter for fern-telemetry.
//!
//! Privacy posture: **anonymous-by-design** (Path A in the telemetry
//! plan). The adapter transmits no client-side identifier, no UUID,
//! no fingerprint. Plausible derives a per-day session hash
//! server-side from `X-Forwarded-For` + `User-Agent` + a daily-
//! rotating server-held salt; this scheme is on the CNIL
//! consent-exempt audience-measurement list.
//!
//! ```ignore
//! use fern_analytics_plausible::PlausibleAdapter;
//! use fern_telemetry::{TelemetryBundle, TelemetryMode, UsageReporter};
//! use std::rc::Rc;
//!
//! let plausible = PlausibleAdapter::builder()
//!     .endpoint("https://plausible.io/api/event")
//!     .domain("skribisto.app")
//!     .build();
//!
//! let telemetry = TelemetryBundle::new(EVENT_SCHEMA_VERSION)
//!     .with_anonymous(Rc::new(plausible) as Rc<dyn UsageReporter>)
//!     .with_default_mode(TelemetryMode::Anonymous);
//! ```
//!
//! Adapter lifecycle:
//!
//! 1. `PlausibleAdapter::builder().build()` spawns a worker thread
//!    that owns the in-flight buffer and the HTTP client.
//! 2. `record(event)` converts to [`OwnedEvent`] and sends through
//!    a channel to the worker. Non-blocking — the UI thread never
//!    waits on HTTP.
//! 3. The worker batches up to `max_batch_size` events or flushes
//!    every `flush_interval` and POSTs to the configured endpoint.
//! 4. Failed requests are retried with exponential backoff (capped
//!    at `max_backoff`).
//! 5. `Drop` signals the worker, blocks for graceful shutdown
//!    flush. Pending events are best-effort flushed on exit.

mod adapter;
mod config;
mod transport;
mod wire;
mod worker;

pub use adapter::{PlausibleAdapter, PlausibleAdapterBuilder};
pub use config::PlausibleConfig;
pub use wire::PlausibleEvent;
