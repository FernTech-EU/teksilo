//! `TelemetryBundle` — declarative configuration for the
//! `FernAppBuilder` integration.
//!
//! Mirrors [`fern_settings::SettingsBundle`] / `OpenedSettings`:
//! construct a bundle with `with_*` methods, `bundle.open(paths,
//! settings)` returns ready-to-register handles, the app registers
//! them in `app_state` and accesses via the [`TelemetryExt`] trait.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use fern_core::telemetry::UsageReporter;
use fern_settings::{AppPaths, SettingsFileError, SettingsStore};

use crate::consent::ConsentStore;
use crate::dynamic_reporter::DynamicReporter;
use crate::install_id::InstallId;
use crate::queue::{EventQueue, InMemoryEventQueue};
use crate::scopes::{TELEMETRY_ENDPOINT_OVERRIDE, TELEMETRY_REGION_OVERRIDE};

/// Which privacy posture the reporter is operating in.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TelemetryMode {
    /// No client identifier transmitted. CNIL consent-exempt under
    /// the audience-measurement self-assessment, GDPR Art. 6(1)(f)
    /// basis. Adapter example: `fern-analytics-plausible`.
    Anonymous,
    /// Stable per-install UUID transmitted with every event. Requires
    /// explicit consent under GDPR Art. 6(1)(a) + ePrivacy 5(3).
    /// Adapter example: `fern-analytics-posthog`.
    Pseudonymous,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DataResidencyRegion {
    EU,
    US,
    Other,
}

impl DataResidencyRegion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EU => "EU",
            Self::US => "US",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrivacyPolicy {
    pub data_processor_name: String,
    pub privacy_policy_url: Option<String>,
    pub data_residency_region: DataResidencyRegion,
    /// Surfaced in the consent widget Art. 13 notice.
    pub retention_days: u32,
}

impl Default for PrivacyPolicy {
    fn default() -> Self {
        Self {
            data_processor_name: String::new(),
            privacy_policy_url: None,
            data_residency_region: DataResidencyRegion::EU,
            retention_days: 395, // 13 months
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryBundleError {
    #[error(
        "TelemetryBundle: no adapter configured (call .with_anonymous(...) or .with_pseudonymous(...))"
    )]
    NoAdapter,
    #[error("TelemetryBundle: {0}")]
    Settings(#[from] SettingsFileError),
}

/// Declarative configuration for the telemetry stack. Consume with
/// [`open`](Self::open).
#[derive(Clone)]
pub struct TelemetryBundle {
    anonymous: Option<Rc<dyn UsageReporter>>,
    pseudonymous: Option<Rc<dyn UsageReporter>>,
    default_mode: TelemetryMode,
    event_schema_version: u32,
    debounce: Duration,
    policy: PrivacyPolicy,
    recent_log_capacity: usize,
}

impl TelemetryBundle {
    /// Empty bundle — at least one of `with_anonymous` /
    /// `with_pseudonymous` must be called before `open`.
    pub fn new(event_schema_version: u32) -> Self {
        Self {
            anonymous: None,
            pseudonymous: None,
            default_mode: TelemetryMode::Anonymous,
            event_schema_version,
            debounce: Duration::from_millis(500),
            policy: PrivacyPolicy::default(),
            recent_log_capacity: 200,
        }
    }

    /// Capacity of the user-facing "recently emitted" ring buffer
    /// surfaced by the `PrivacySettings` widget's "Inspect data sent"
    /// accordion. Default: 200 events. Independent of any adapter's
    /// own outbound queue.
    pub fn with_recent_log_capacity(mut self, n: usize) -> Self {
        self.recent_log_capacity = n.max(1);
        self
    }

    /// Install the anonymous-mode adapter (e.g.
    /// `fern-analytics-plausible`). The adapter's `install_id()`
    /// must return `None`.
    pub fn with_anonymous(mut self, reporter: Rc<dyn UsageReporter>) -> Self {
        self.anonymous = Some(reporter);
        self
    }

    /// Install the pseudonymous-mode adapter (e.g.
    /// `fern-analytics-posthog`). The adapter's `install_id()` must
    /// return `Some(uuid)` once configured.
    pub fn with_pseudonymous(mut self, reporter: Rc<dyn UsageReporter>) -> Self {
        self.pseudonymous = Some(reporter);
        self
    }

    /// Which mode is active on first run before the user picks.
    pub fn with_default_mode(mut self, mode: TelemetryMode) -> Self {
        self.default_mode = mode;
        self
    }

    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    pub fn with_data_processor_name(mut self, name: impl Into<String>) -> Self {
        self.policy.data_processor_name = name.into();
        self
    }

    pub fn with_privacy_policy_url(mut self, url: impl Into<String>) -> Self {
        self.policy.privacy_policy_url = Some(url.into());
        self
    }

    pub fn with_data_residency_region(mut self, region: DataResidencyRegion) -> Self {
        self.policy.data_residency_region = region;
        self
    }

    pub fn with_retention_days(mut self, days: u32) -> Self {
        self.policy.retention_days = days;
        self
    }

    /// Open every requested service against `paths`. Reads the
    /// runtime endpoint override from `settings` exactly once; the
    /// override is propagated to adapters via their constructors
    /// (each adapter is responsible for honoring it).
    pub fn open(
        self,
        paths: &AppPaths,
        settings: &SettingsStore,
    ) -> Result<OpenedTelemetry, TelemetryBundleError> {
        if self.anonymous.is_none() && self.pseudonymous.is_none() {
            return Err(TelemetryBundleError::NoAdapter);
        }

        // Validate the requested default mode has an adapter.
        let default_mode = match self.default_mode {
            TelemetryMode::Anonymous if self.anonymous.is_some() => TelemetryMode::Anonymous,
            TelemetryMode::Pseudonymous if self.pseudonymous.is_some() => {
                TelemetryMode::Pseudonymous
            }
            // Fall back to whichever was actually configured.
            _ => {
                if self.anonymous.is_some() {
                    TelemetryMode::Anonymous
                } else {
                    TelemetryMode::Pseudonymous
                }
            }
        };

        // Read the endpoint override (if any) for the consent re-prompt
        // recipient-change check. Adapters are constructed before this
        // function runs, so they already hold their endpoint by value;
        // the override path is for adapters that subscribe to the
        // signal directly. Phase 1 stores only the recipient marker.
        let endpoint_override = settings.signal_for(&TELEMETRY_ENDPOINT_OVERRIDE).get();
        let _region_override = settings.signal_for(&TELEMETRY_REGION_OVERRIDE).get();

        let endpoint_for_consent = match (&self.anonymous, &self.pseudonymous) {
            // Endpoint string used for the re-prompt recipient check.
            // Prefer the active adapter's endpoint.
            _ if !endpoint_override.is_empty() => endpoint_override.clone(),
            (_, Some(p)) if matches!(default_mode, TelemetryMode::Pseudonymous) => {
                p.endpoint().to_string()
            }
            (Some(a), _) => a.endpoint().to_string(),
            (None, Some(p)) => p.endpoint().to_string(),
            _ => unreachable!("validated above"),
        };

        let consent = ConsentStore::open(
            paths,
            self.debounce,
            self.event_schema_version,
            &endpoint_for_consent,
        )?
        .with_settings_mirror(settings.clone());

        let install_id = if self.pseudonymous.is_some() {
            Some(InstallId::open_or_create(paths, self.debounce)?)
        } else {
            None
        };

        let recent_log = Arc::new(InMemoryEventQueue::with_capacity(self.recent_log_capacity));
        let reporter = Rc::new(DynamicReporter::new(
            self.anonymous,
            self.pseudonymous,
            default_mode,
            consent.clone(),
            recent_log.clone(),
        ));

        Ok(OpenedTelemetry {
            consent,
            install_id,
            reporter,
            recent_log,
            policy: self.policy,
            event_schema_version: self.event_schema_version,
        })
    }
}

/// Outcome of [`TelemetryBundle::open`]. Cheap to clone (every contained
/// service is `Rc`/`Arc`-shaped). Registered into `app_state` by
/// `FernAppBuilder::install_telemetry`.
#[derive(Clone)]
pub struct OpenedTelemetry {
    pub consent: ConsentStore,
    pub install_id: Option<InstallId>,
    pub reporter: Rc<DynamicReporter>,
    /// Ring buffer of recently-emitted events. `DynamicReporter::record`
    /// tees every consent-gated event into this log in addition to
    /// forwarding to the active adapter. Bounded by
    /// `TelemetryBundle::with_recent_log_capacity` (default 200).
    /// Read by the `PrivacySettings` "Inspect data sent" accordion;
    /// **independent** of the adapter's outbound queue (events stay
    /// in the recent log even after the adapter has flushed them).
    pub recent_log: Arc<InMemoryEventQueue>,
    pub policy: PrivacyPolicy,
    pub event_schema_version: u32,
}

impl OpenedTelemetry {
    /// Synchronous flush of consent + install_id files (the queue
    /// flush is the adapter's responsibility — see
    /// [`UsageReporter::flush`]).
    pub fn flush_all(&self) -> Result<(), TelemetryBundleError> {
        self.consent.flush_now()?;
        if let Some(id) = &self.install_id {
            id.flush_now()?;
        }
        Ok(())
    }

    /// Wipe the recent-log ring buffer and ask each adapter to drop
    /// its outbound buffer. Called on consent revocation and on the
    /// "Erase my data" flow.
    ///
    /// The recent-log discard happens inside `DynamicReporter::
    /// discard_pending()` (so the revision signal is bumped from
    /// the same place it's bumped on `record()`), not here.
    pub fn discard_pending(&self) -> Result<(), fern_core::telemetry::TelemetryError> {
        self.reporter.discard_pending()
    }
}

impl std::fmt::Debug for OpenedTelemetry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedTelemetry")
            .field("consent", &self.consent)
            .field("install_id", &self.install_id)
            .field("policy", &self.policy)
            .field("recent_log_len", &self.recent_log.len())
            .field("event_schema_version", &self.event_schema_version)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::StubReporter;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn empty_bundle_errors() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let store =
            SettingsStore::open_with_delay(paths.config_file("general"), Duration::ZERO).unwrap();
        let bundle = TelemetryBundle::new(1);
        let err = bundle.open(&paths, &store).unwrap_err();
        assert!(matches!(err, TelemetryBundleError::NoAdapter));
    }

    #[test]
    fn anonymous_only_bundle_opens() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let store =
            SettingsStore::open_with_delay(paths.config_file("general"), Duration::ZERO).unwrap();
        let opened = TelemetryBundle::new(1)
            .with_anonymous(Rc::new(StubReporter::anonymous()))
            .with_default_mode(TelemetryMode::Anonymous)
            .with_debounce(Duration::ZERO)
            .open(&paths, &store)
            .unwrap();
        assert!(opened.install_id.is_none());
        assert_eq!(opened.reporter.active_mode(), TelemetryMode::Anonymous);
    }

    #[test]
    fn pseudonymous_bundle_creates_install_id() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let store =
            SettingsStore::open_with_delay(paths.config_file("general"), Duration::ZERO).unwrap();
        let opened = TelemetryBundle::new(1)
            .with_pseudonymous(Rc::new(StubReporter::pseudonymous("u")))
            .with_default_mode(TelemetryMode::Pseudonymous)
            .with_debounce(Duration::ZERO)
            .open(&paths, &store)
            .unwrap();
        assert!(opened.install_id.is_some());
        let id = opened.install_id.as_ref().unwrap().get();
        assert!(!id.is_empty());
    }

    #[test]
    fn default_mode_falls_back_when_unsupported() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let store =
            SettingsStore::open_with_delay(paths.config_file("general"), Duration::ZERO).unwrap();
        // Request pseudonymous default but only ship anonymous adapter.
        let opened = TelemetryBundle::new(1)
            .with_anonymous(Rc::new(StubReporter::anonymous()))
            .with_default_mode(TelemetryMode::Pseudonymous)
            .with_debounce(Duration::ZERO)
            .open(&paths, &store)
            .unwrap();
        assert_eq!(opened.reporter.active_mode(), TelemetryMode::Anonymous);
    }
}
