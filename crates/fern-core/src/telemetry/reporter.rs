//! `UsageReporter` trait and consent enums.
//!
//! The trait is **synchronous** in Phase 1. Adapters that perform
//! HTTP transport spawn their own worker threads or use blocking
//! clients; the dispatch tap MUST NOT block the UI thread, so
//! [`UsageReporter::record`] queues and returns. Async upgrade is
//! possible later without changing the surface (return
//! `Pin<Box<dyn Future>>` for the methods that need it).

use std::io;
use std::rc::Rc;

use super::event::{Event, RemoteDataExport};

/// The observability sink. Implemented by adapter crates
/// (`fern-analytics-plausible`, `fern-analytics-posthog`, etc.) and
/// by `fern-telemetry::DynamicReporter` (which forwards to whichever
/// concrete adapter is currently active).
///
/// Object-safe — registered into the app-state registry as
/// `Rc<dyn UsageReporter>` and looked up by trait-object pointer.
///
/// **Single-threaded.** FernUI is single-threaded by design (the
/// arena, `Signal<T>`, `ListModel<T>` are all `Rc<RefCell<>>`-shaped).
/// Adapters that need a worker thread for HTTP transport bridge
/// internally with channels and own a separate `Send`-able state;
/// the trait surface itself stays on the UI thread.
///
/// Implementations MUST gate emission on consent state internally.
/// The dispatch tap calls `record` unconditionally; the reporter
/// drops the event when consent is not `Granted`.
pub trait UsageReporter: 'static {
    /// Invoked synchronously from any thread. MUST NOT block the
    /// caller (queue and return). Drops events when consent is not
    /// `Granted`. Errors are buffered internally — there is no
    /// return value because the caller cannot meaningfully react.
    fn record(&self, event: &Event<'_>);

    /// Best-effort drain of the on-disk queue. Called on graceful
    /// exit. **Not** called on consent revocation — see
    /// [`Self::discard_pending`].
    fn flush(&self) -> Result<(), TelemetryError> {
        Ok(())
    }

    /// Drop the queue without sending. Called when consent is
    /// revoked, when the mode is switched, or when the user clicks
    /// "Erase my data". Once consent is `Denied` or `Unknown`, the
    /// buffered events are no longer permitted to leave the device.
    fn discard_pending(&self) -> Result<(), TelemetryError> {
        Ok(())
    }

    /// GDPR Art. 17. Pseudonymous mode: send DELETE keyed by
    /// install_id; clear the local queue. Anonymous mode: returns
    /// [`TelemetryError::ErasureUnsupported`] so the widget can hide
    /// the button.
    fn erase_remote_data(&self) -> Result<(), TelemetryError> {
        Err(TelemetryError::ErasureUnsupported)
    }

    /// GDPR Art. 15 + 20. Pseudonymous mode: fetch all server-side
    /// events for this install_id as a [`RemoteDataExport`].
    /// Anonymous mode: returns [`TelemetryError::FetchUnsupported`].
    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError> {
        Err(TelemetryError::FetchUnsupported)
    }

    /// `Some(uuid)` in pseudonymous mode, `None` in anonymous mode.
    /// Surfaced verbatim by the consent widget for user inspection.
    fn install_id(&self) -> Option<&str> {
        None
    }

    /// `"plausible"`, `"posthog"`, `"otlp"`, `"stub"`. Shown in the
    /// widget's "what gets sent" tab.
    fn adapter_name(&self) -> &'static str;

    /// Endpoint URL displayed verbatim in the consent widget.
    fn endpoint(&self) -> &str;

    /// Drives the consent widget toggle group: which scopes does
    /// this adapter actually use? Toggles for unsupported scopes
    /// are hidden, not just disabled.
    fn supported_scopes(&self) -> ConsentScope {
        ConsentScope::all()
    }
}

/// Registration type for the dispatch tap.
///
/// `fern-telemetry::TelemetryBundle::open` constructs one of these
/// and registers it into `app_state`. The dispatch tap in
/// [`crate::widget_tree::WidgetTree::dispatch_intent`] looks it up
/// by `TypeId`, calls `record` if found.
///
/// Carries the metadata the tap needs to assemble a complete
/// `Event` — `session_id` (per-process random) and `schema_version`
/// (codegen'd constant from the YAML manifest).
pub struct TelemetryContext {
    pub reporter: Rc<dyn UsageReporter>,
    /// Per-process random session id. Not persisted across restarts.
    pub session_id: String,
    /// Event-schema version at this build. Bumped whenever the
    /// framework's events.yaml gains, drops, or reshapes an event.
    pub schema_version: u32,
}

impl std::fmt::Debug for TelemetryContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelemetryContext")
            .field("session_id", &self.session_id)
            .field("schema_version", &self.schema_version)
            .field("adapter", &self.reporter.adapter_name())
            .finish()
    }
}

#[derive(Debug)]
pub enum TelemetryError {
    /// Anonymous mode: no linkable data to erase.
    ErasureUnsupported,
    /// Anonymous mode: no per-user query surface.
    FetchUnsupported,
    /// Backend has no DELETE endpoint configured (OTLP variants).
    ErasureUnsupportedByBackend,
    /// Backend has no query endpoint configured.
    FetchUnsupportedByBackend,
    Network(io::Error),
    Server {
        status: u16,
        body: String,
    },
    /// Adapter rate-limited the export (Art. 12(3) one-month SLA
    /// applies — the controller must honor the request out-of-band).
    QuotaExceeded,
    /// Consent state forbids the operation right now.
    NotConsented,
    Other(String),
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ErasureUnsupported => f.write_str("erasure unsupported (anonymous mode)"),
            Self::FetchUnsupported => f.write_str("fetch unsupported (anonymous mode)"),
            Self::ErasureUnsupportedByBackend => {
                f.write_str("erasure unsupported by configured backend")
            }
            Self::FetchUnsupportedByBackend => {
                f.write_str("fetch unsupported by configured backend")
            }
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::Server { status, body } => {
                write!(f, "server returned {status}: {body}")
            }
            Self::QuotaExceeded => f.write_str("rate-limited; retry later"),
            Self::NotConsented => f.write_str("consent not granted"),
            Self::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for TelemetryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TelemetryError {
    fn from(e: io::Error) -> Self {
        Self::Network(e)
    }
}

// --- Consent state --------------------------------------------------

/// Top-level consent state, persisted via `fern-telemetry::ConsentStore`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ConsentState {
    /// Pre-decision. The widget must prompt; no events emitted.
    #[default]
    Unknown,
    /// User granted consent for the listed scopes.
    Granted(ConsentScope),
    /// User explicitly declined. No events emitted.
    Denied,
}

impl ConsentState {
    pub fn is_granted(&self) -> bool {
        matches!(self, ConsentState::Granted(_))
    }

    pub fn scope(&self) -> Option<&ConsentScope> {
        match self {
            ConsentState::Granted(s) => Some(s),
            _ => None,
        }
    }
}

/// Per-purpose consent toggles. Each is independent.
///
/// Defaults to **all-false**: an `Unknown` state translates to "no
/// scopes granted" until the user explicitly opts in. The widget shows
/// a toggle row per scope and an Accept-all / Reject-all pair (CNIL
/// equal-prominence rule).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ConsentScope {
    /// Anonymous usage metrics (intent dispatches, lifecycle, census).
    /// Anonymous mode always uses this scope — no other scope makes
    /// sense without a stable id.
    pub anonymous_metrics: bool,
    /// Crash reports. Reserved — no transport in Phase 1.
    pub crash_reports: bool,
    /// Feature-flag fetches. Reserved — no transport in Phase 1.
    pub feature_flags: bool,
    /// Session recording. Reserved; not implemented (PII risk).
    pub session_recording: bool,
}

impl ConsentScope {
    /// All scopes off — the default, and the value used for `Denied`.
    pub fn none() -> Self {
        Self::default()
    }

    /// All scopes on — convenience for tests and "Accept all".
    pub fn all() -> Self {
        Self {
            anonymous_metrics: true,
            crash_reports: true,
            feature_flags: true,
            session_recording: false, // reserved, never auto-on
        }
    }

    pub fn anonymous_metrics_only() -> Self {
        Self {
            anonymous_metrics: true,
            ..Self::default()
        }
    }

    /// True if any toggle is on.
    pub fn any(&self) -> bool {
        self.anonymous_metrics || self.crash_reports || self.feature_flags || self.session_recording
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_state_defaults_to_unknown() {
        let s = ConsentState::default();
        assert!(matches!(s, ConsentState::Unknown));
        assert!(!s.is_granted());
    }

    #[test]
    fn scope_all_excludes_session_recording() {
        let s = ConsentScope::all();
        assert!(s.anonymous_metrics);
        assert!(s.crash_reports);
        assert!(s.feature_flags);
        assert!(!s.session_recording);
    }

    #[test]
    fn telemetry_error_display() {
        let e = TelemetryError::ErasureUnsupported;
        assert!(e.to_string().contains("anonymous"));
    }
}
