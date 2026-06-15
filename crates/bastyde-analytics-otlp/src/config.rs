// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Adapter configuration.

use std::time::Duration;

/// Static configuration for the OTLP adapter.
#[derive(Clone, Debug)]
pub struct OtlpConfig {
    /// OTLP/HTTP logs endpoint. The OpenTelemetry spec mandates the
    /// path suffix `/v1/logs`; pass the full URL here, e.g.
    /// `https://api.honeycomb.io/v1/logs` or
    /// `http://127.0.0.1:4318/v1/logs` for a local otelcol.
    pub endpoint: String,

    /// Logical service name. Becomes the
    /// `resource.service.name` attribute on every emitted log
    /// record. Required by the OTLP spec.
    pub service_name: String,

    /// App version. Becomes `resource.service.version`.
    pub service_version: String,

    /// Optional auth header. Honeycomb takes `x-honeycomb-team:
    /// <api-key>`; Loki via the otelcol HTTP receiver typically
    /// runs unauth on a private network. Pass any extra headers
    /// here as `(name, value)` pairs.
    pub headers: Vec<(String, String)>,

    /// `User-Agent`. Some collectors log it.
    pub user_agent: String,

    /// Maximum log records per HTTP batch.
    pub max_batch_size: usize,

    /// Worker flushes the buffer at least this often.
    pub flush_interval: Duration,

    /// Initial retry delay after a transport failure. Doubles each
    /// failed attempt up to `max_backoff`.
    pub initial_backoff: Duration,

    /// Cap on the exponential backoff.
    pub max_backoff: Duration,

    /// Per-request HTTP timeout.
    pub request_timeout: Duration,

    /// Cap on the in-memory queue size.
    pub max_queue_size: usize,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:4318/v1/logs".to_string(),
            service_name: String::new(),
            service_version: String::new(),
            headers: Vec::new(),
            user_agent: format!(
                "bastyde/{} ({} {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
            ),
            max_batch_size: 50,
            flush_interval: Duration::from_secs(30),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60 * 60),
            request_timeout: Duration::from_secs(10),
            max_queue_size: 10_000,
        }
    }
}
