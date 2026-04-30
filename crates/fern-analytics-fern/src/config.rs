use std::time::Duration;

/// TLS client config. Carries the parts that change per deployment;
/// bytes (PEM) rather than paths so the adapter can be configured
/// from any source (file, env var, vault).
#[derive(Debug, Clone, Default)]
pub struct TlsClientConfig {
    /// Optional CA certificate(s) to trust for the server. When
    /// `None`, the system root store is used (the typical case for
    /// publicly-rooted certs from Let's Encrypt). Provide a custom
    /// CA when self-signed or a private CA is in play.
    pub ca_pem: Option<Vec<u8>>,
    /// Optional client certificate + private key for mTLS. Both
    /// must be present together.
    pub client_cert_pem: Option<Vec<u8>>,
    pub client_key_pem: Option<Vec<u8>>,
    /// Optional override for the server's domain name as it
    /// appears in the cert. Defaults to the host parsed from the
    /// endpoint URL.
    pub domain_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FernConfig {
    /// gRPC endpoint URL (e.g. "http://127.0.0.1:50051" or
    /// "https://collector.example.com:50051").
    pub endpoint: String,

    /// Operator-defined product identifier sent on every batch.
    /// Validated server-side against the API token's allowed scope
    /// (sub-phase B). Defaults to the empty string — `build()` will
    /// panic if not set, since there's no sensible default.
    pub product_id: String,

    /// Application-level event-schema version. Independent of the
    /// proto wire version. Server may reject unknown values.
    pub schema_version: u32,

    /// Maximum number of events shipped in a single `EventBatch`.
    pub max_batch_size: usize,

    /// How long to wait between flushes when the queue is non-empty
    /// but below `max_batch_size`.
    pub flush_interval: Duration,

    /// Per-RPC connect / read timeout.
    pub request_timeout: Duration,

    /// In-memory queue cap when no persistent queue is configured.
    /// Past this, oldest events are dropped.
    pub max_queue_size: usize,

    /// Bearer token to send in the gRPC `Authorization` metadata
    /// of every request. When `None`, no auth header is sent
    /// (server must be running in unauth mode). Format:
    /// `fct_<id>_<secret>` as minted by `fern-collector token mint`.
    pub bearer_token: Option<String>,

    /// Optional TLS configuration. When `Some`, the endpoint must
    /// use the `https://` scheme. When `None`, the adapter speaks
    /// plain HTTP/2 (h2c) and the endpoint must use `http://`.
    pub tls: Option<TlsClientConfig>,

    /// Per-install pseudonymous identifier (a UUID). When `Some`,
    /// every event carries this id and the wire-format batch is
    /// tagged `pseudonymous`. When `None` the adapter runs in
    /// anonymous mode — no install_id, batches tagged `anonymous`,
    /// fetch/erase return `FetchUnsupported`/`ErasureUnsupported`.
    pub install_id: Option<String>,
}

impl Default for FernConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            product_id: String::new(),
            schema_version: 1,
            max_batch_size: 50,
            flush_interval: Duration::from_secs(60),
            request_timeout: Duration::from_secs(10),
            max_queue_size: 10_000,
            bearer_token: None,
            tls: None,
            install_id: None,
        }
    }
}
