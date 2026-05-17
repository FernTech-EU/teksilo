//! Persisted consent state.
//!
//! [`ConsentStore`] wraps a [`SettingsFile<ConsentFile>`] from
//! `bastyde-settings` and exposes a `Signal<ConsentState>` for widget
//! binding. Atomic writes, debounced flush, migration, and OS-correct
//! paths are all inherited from `SettingsFile`.
//!
//! Two version fields are tracked:
//!
//! - `ConsentFile.version` — the on-disk schema version of *this
//!   file* (driven by `Versioned::CURRENT_VERSION`). Used by future
//!   `ConsentFile` shape changes via `Migrator`.
//! - `ConsentFile.consented_to_event_schema` — which version of the
//!   *event* schema the user consented to. When the framework's
//!   event schema bumps past this, the store resets to `Unknown`
//!   and the widget re-prompts.

use std::time::{Duration, SystemTime};

use bastyde_core::Signal;
use bastyde_core::telemetry::{ConsentScope, ConsentState};
use bastyde_settings::{
    AppPaths, Migrator, SettingsFile, SettingsFileError, SettingsStore, Versioned,
};
use serde::{Deserialize, Serialize};

use crate::scopes::{
    TELEMETRY_ANONYMOUS_METRICS, TELEMETRY_CRASH_REPORTS, TELEMETRY_FEATURE_FLAGS,
};

/// On-disk consent record. Persisted via `SettingsFile<ConsentFile>`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConsentFile {
    /// `ConsentFile` schema version (driven by `Versioned`).
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub state: PersistedConsentState,
    #[serde(default)]
    pub decided_at: Option<SystemTime>,
    /// EVENT schema version at the time consent was given. When the
    /// framework's event schema increments past this number, the
    /// store resets to `Unknown` so the widget re-prompts.
    #[serde(default)]
    pub consented_to_event_schema: u32,
    /// The endpoint the consent was given against. If the user
    /// changes the endpoint override later, this differs from the
    /// reporter's current endpoint and the store re-prompts (the
    /// "recipient changed" rule from §11.4 of the plan).
    #[serde(default)]
    pub endpoint_at_consent_time: String,
}

impl Versioned for ConsentFile {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

/// Serializable mirror of [`ConsentState`]. We don't use
/// `serde(remote = "ConsentState")` because `ConsentState` lives in
/// `bastyde-core` (which has no serde dep), so this enum is the
/// persistence companion.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum PersistedConsentState {
    #[default]
    Unknown,
    Granted {
        scope: PersistedConsentScope,
    },
    Denied,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedConsentScope {
    #[serde(default)]
    pub anonymous_metrics: bool,
    #[serde(default)]
    pub crash_reports: bool,
    #[serde(default)]
    pub feature_flags: bool,
    #[serde(default)]
    pub session_recording: bool,
}

impl From<ConsentScope> for PersistedConsentScope {
    fn from(s: ConsentScope) -> Self {
        Self {
            anonymous_metrics: s.anonymous_metrics,
            crash_reports: s.crash_reports,
            feature_flags: s.feature_flags,
            session_recording: s.session_recording,
        }
    }
}

impl From<PersistedConsentScope> for ConsentScope {
    fn from(s: PersistedConsentScope) -> Self {
        Self {
            anonymous_metrics: s.anonymous_metrics,
            crash_reports: s.crash_reports,
            feature_flags: s.feature_flags,
            session_recording: s.session_recording,
        }
    }
}

impl From<PersistedConsentState> for ConsentState {
    fn from(p: PersistedConsentState) -> Self {
        match p {
            PersistedConsentState::Unknown => ConsentState::Unknown,
            PersistedConsentState::Granted { scope } => ConsentState::Granted(scope.into()),
            PersistedConsentState::Denied => ConsentState::Denied,
        }
    }
}

impl From<ConsentState> for PersistedConsentState {
    fn from(c: ConsentState) -> Self {
        match c {
            ConsentState::Unknown => PersistedConsentState::Unknown,
            ConsentState::Granted(scope) => PersistedConsentState::Granted {
                scope: scope.into(),
            },
            ConsentState::Denied => PersistedConsentState::Denied,
        }
    }
}

/// In-memory façade over [`ConsentFile`] with a `Signal<ConsentState>`
/// for widget binding.
///
/// `Clone` is cheap: both the `SettingsFile` handle and the `Signal`
/// are `Rc`-shared internally.
///
/// When constructed with an attached [`SettingsStore`] (via
/// [`ConsentStore::with_settings_mirror`]), every write also updates
/// the per-scope `SettingsKey<bool>` constants in
/// [`crate::scopes`] so power users editing `general.toml` directly
/// see the same state. The mirror is one-way (consent → settings); we
/// don't watch the settings keys for changes because the consent file
/// is always the source of truth.
#[derive(Clone)]
pub struct ConsentStore {
    file: SettingsFile<ConsentFile>,
    state: Signal<ConsentState>,
    current_event_schema: u32,
    settings_mirror: Option<SettingsStore>,
}

impl ConsentStore {
    /// Open the consent file at `paths.config_file("telemetry-consent")`.
    ///
    /// Applies the event-schema re-prompt rule: if the user previously
    /// consented to an older event-schema version (or to a different
    /// endpoint), the persisted state is reset to `Unknown` and the
    /// widget will re-prompt on first display.
    pub fn open(
        paths: &AppPaths,
        delay: Duration,
        current_event_schema: u32,
        current_endpoint: &str,
    ) -> Result<Self, SettingsFileError> {
        let migrator = Migrator::<ConsentFile>::new();
        let file = SettingsFile::load(paths.config_file("telemetry-consent"), delay, &migrator)?;

        // Re-prompt rule. Schema bump or endpoint change resets state.
        let snap = file.snapshot();
        let needs_reprompt = snap.consented_to_event_schema < current_event_schema
            || (!snap.endpoint_at_consent_time.is_empty()
                && snap.endpoint_at_consent_time != current_endpoint);
        if needs_reprompt {
            file.mutate(|f| {
                f.state = PersistedConsentState::Unknown;
                f.decided_at = None;
                f.consented_to_event_schema = current_event_schema;
                f.endpoint_at_consent_time = current_endpoint.to_string();
            })?;
        }

        let state = Signal::new(ConsentState::from(file.snapshot().state));
        Ok(Self {
            file,
            state,
            current_event_schema,
            settings_mirror: None,
        })
    }

    /// Attach a [`SettingsStore`] for one-way mirror of the per-scope
    /// toggles into the app's `general.toml`. Called by
    /// `TelemetryBundle::open` once the store is available. Idempotent
    /// — passing `None` is a no-op; calling twice replaces the
    /// previously-attached store.
    pub fn with_settings_mirror(mut self, settings: SettingsStore) -> Self {
        self.settings_mirror = Some(settings);
        // Seed the mirror with the current state so the first read
        // from the settings store reflects the consent file.
        self.write_mirror(self.state.get().scope().copied());
        self
    }

    fn write_mirror(&self, scope: Option<ConsentScope>) {
        let Some(store) = &self.settings_mirror else {
            return;
        };
        let resolved = scope.unwrap_or_default();
        store
            .signal_for(&TELEMETRY_ANONYMOUS_METRICS)
            .set(resolved.anonymous_metrics);
        store
            .signal_for(&TELEMETRY_CRASH_REPORTS)
            .set(resolved.crash_reports);
        store
            .signal_for(&TELEMETRY_FEATURE_FLAGS)
            .set(resolved.feature_flags);
    }

    /// The reactive state. Bind directly to the consent widget.
    pub fn state_signal(&self) -> Signal<ConsentState> {
        self.state.clone()
    }

    /// `true` iff [`ConsentState::is_granted`] holds. Convenience for
    /// the dispatch tap which gates emission.
    pub fn is_granted(&self) -> bool {
        self.state.get().is_granted()
    }

    /// User accepted, with the given scope. Persists `decided_at` and
    /// the current event-schema version so future schema bumps will
    /// re-prompt correctly.
    pub fn grant(&self, scope: ConsentScope, endpoint: &str) -> Result<(), SettingsFileError> {
        self.file.mutate(|f| {
            f.state = PersistedConsentState::Granted {
                scope: scope.into(),
            };
            f.decided_at = Some(SystemTime::now());
            f.consented_to_event_schema = self.current_event_schema;
            f.endpoint_at_consent_time = endpoint.to_string();
        })?;
        self.state.set(ConsentState::Granted(scope));
        self.write_mirror(Some(scope));
        Ok(())
    }

    /// User explicitly declined. No events emitted; queue is left
    /// alone (caller's responsibility to discard if appropriate).
    pub fn deny(&self) -> Result<(), SettingsFileError> {
        self.file.mutate(|f| {
            f.state = PersistedConsentState::Denied;
            f.decided_at = Some(SystemTime::now());
        })?;
        self.state.set(ConsentState::Denied);
        self.write_mirror(None);
        Ok(())
    }

    /// User withdrew previously-given consent. Same persistence as
    /// `deny`; semantically distinct in the widget UI.
    pub fn withdraw(&self) -> Result<(), SettingsFileError> {
        self.deny()
    }

    /// Reset to `Unknown` — used by the mode-switch flow before
    /// re-prompting.
    pub fn reset(&self) -> Result<(), SettingsFileError> {
        self.file.mutate(|f| {
            f.state = PersistedConsentState::Unknown;
            f.decided_at = None;
        })?;
        self.state.set(ConsentState::Unknown);
        self.write_mirror(None);
        Ok(())
    }

    /// Mutate the granted scope in place. If the current state is
    /// `Unknown`, transitions to `Granted` with the mutated default
    /// scope (so flipping a single toggle on first run grants
    /// consent for that one scope only).
    ///
    /// `Denied` is preserved — the user must explicitly withdraw the
    /// "no" before scope toggles take effect. Returns `Ok(true)` if
    /// the mutation was applied, `Ok(false)` if `Denied` blocked it.
    ///
    /// Used by the consent widget for individual scope toggles.
    pub fn set_or_grant_scope(
        &self,
        endpoint: &str,
        f: impl FnOnce(&mut ConsentScope),
    ) -> Result<bool, SettingsFileError> {
        let current = self.state.get();
        match current {
            ConsentState::Denied => Ok(false),
            ConsentState::Granted(_) => self.set_scope(f),
            ConsentState::Unknown => {
                let mut scope = ConsentScope::none();
                f(&mut scope);
                self.grant(scope, endpoint)?;
                Ok(true)
            }
        }
    }

    /// Mutate the granted scope in place (e.g. user toggles
    /// `crash_reports` from the settings widget).
    ///
    /// **Returns `Ok(false)` and is a no-op when the state is
    /// `Denied` or `Unknown`** — the widget must call `grant()`
    /// first to install a base scope. Returns `Ok(true)` when the
    /// closure was invoked and the new scope persisted.
    pub fn set_scope(&self, f: impl FnOnce(&mut ConsentScope)) -> Result<bool, SettingsFileError> {
        let mut current = self.state.get();
        let ConsentState::Granted(ref mut scope) = current else {
            return Ok(false);
        };
        f(scope);
        let scope = *scope;
        self.file.mutate(|file| {
            file.state = PersistedConsentState::Granted {
                scope: scope.into(),
            };
        })?;
        self.state.set(ConsentState::Granted(scope));
        self.write_mirror(Some(scope));
        Ok(true)
    }

    /// Force a synchronous flush to disk.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.file.flush_now()
    }

    /// The on-disk path. Surfaced verbatim by the consent widget for
    /// transparency (the user can inspect or delete the file by hand).
    pub fn path(&self) -> &std::path::Path {
        self.file.path()
    }
}

impl std::fmt::Debug for ConsentStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsentStore")
            .field("path", &self.file.path())
            .field("state", &self.state.get())
            .field("schema", &self.current_event_schema)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open(dir: &std::path::Path, schema: u32, endpoint: &str) -> ConsentStore {
        let paths = AppPaths::for_testing(dir);
        ConsentStore::open(&paths, Duration::ZERO, schema, endpoint).unwrap()
    }

    #[test]
    fn fresh_store_starts_unknown() {
        let dir = tempdir().unwrap();
        let store = open(dir.path(), 1, "stub://");
        assert!(matches!(store.state_signal().get(), ConsentState::Unknown));
        assert!(!store.is_granted());
    }

    #[test]
    fn grant_persists_and_round_trips() {
        let dir = tempdir().unwrap();
        {
            let store = open(dir.path(), 1, "stub://");
            store
                .grant(ConsentScope::anonymous_metrics_only(), "stub://")
                .unwrap();
            store.flush_now().unwrap();
        }
        let store = open(dir.path(), 1, "stub://");
        let s = store.state_signal().get();
        assert!(matches!(s, ConsentState::Granted(_)));
        if let ConsentState::Granted(scope) = s {
            assert!(scope.anonymous_metrics);
            assert!(!scope.crash_reports);
        }
    }

    #[test]
    fn schema_bump_resets_to_unknown() {
        let dir = tempdir().unwrap();
        {
            let store = open(dir.path(), 1, "stub://");
            store.grant(ConsentScope::all(), "stub://").unwrap();
            store.flush_now().unwrap();
        }
        // Reopen with a higher event-schema version.
        let store = open(dir.path(), 2, "stub://");
        assert!(matches!(store.state_signal().get(), ConsentState::Unknown));
    }

    #[test]
    fn endpoint_change_resets_to_unknown() {
        let dir = tempdir().unwrap();
        {
            let store = open(dir.path(), 1, "https://eu.example.com/");
            store
                .grant(ConsentScope::all(), "https://eu.example.com/")
                .unwrap();
            store.flush_now().unwrap();
        }
        // Reopen with a different endpoint — recipient changed.
        let store = open(dir.path(), 1, "https://us.example.com/");
        assert!(matches!(store.state_signal().get(), ConsentState::Unknown));
    }

    #[test]
    fn deny_then_grant_works() {
        let dir = tempdir().unwrap();
        let store = open(dir.path(), 1, "stub://");
        store.deny().unwrap();
        assert!(matches!(store.state_signal().get(), ConsentState::Denied));
        store
            .grant(ConsentScope::anonymous_metrics_only(), "stub://")
            .unwrap();
        assert!(store.is_granted());
    }

    #[test]
    fn set_scope_no_op_when_not_granted() {
        let dir = tempdir().unwrap();
        let store = open(dir.path(), 1, "stub://");
        // Unknown — should not panic, should not change state.
        let applied = store.set_scope(|s| s.crash_reports = true).unwrap();
        assert!(!applied, "set_scope must report skipped");
        assert!(matches!(store.state_signal().get(), ConsentState::Unknown));
    }

    #[test]
    fn settings_mirror_reflects_scope_changes() {
        use bastyde_settings::SettingsStore;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let store =
            SettingsStore::open_with_delay(paths.config_file("general"), Duration::ZERO).unwrap();
        let consent = ConsentStore::open(&paths, Duration::ZERO, 1, "stub://")
            .unwrap()
            .with_settings_mirror(store.clone());

        // Initial mirror is all-off (matches Unknown state).
        assert!(!store.signal_for(&TELEMETRY_ANONYMOUS_METRICS).get());

        consent.grant(ConsentScope::all(), "stub://").unwrap();

        assert!(store.signal_for(&TELEMETRY_ANONYMOUS_METRICS).get());
        assert!(store.signal_for(&TELEMETRY_CRASH_REPORTS).get());
        assert!(store.signal_for(&TELEMETRY_FEATURE_FLAGS).get());

        consent.set_scope(|s| s.crash_reports = false).unwrap();
        assert!(!store.signal_for(&TELEMETRY_CRASH_REPORTS).get());

        consent.deny().unwrap();
        // Denied wipes the mirror back to all-off.
        assert!(!store.signal_for(&TELEMETRY_ANONYMOUS_METRICS).get());
        assert!(!store.signal_for(&TELEMETRY_CRASH_REPORTS).get());
    }

    #[test]
    fn set_scope_updates_when_granted() {
        let dir = tempdir().unwrap();
        let store = open(dir.path(), 1, "stub://");
        store
            .grant(ConsentScope::anonymous_metrics_only(), "stub://")
            .unwrap();
        let applied = store.set_scope(|s| s.crash_reports = true).unwrap();
        assert!(applied, "set_scope must report applied");
        if let ConsentState::Granted(scope) = store.state_signal().get() {
            assert!(scope.anonymous_metrics);
            assert!(scope.crash_reports);
        } else {
            panic!("expected Granted");
        }
    }
}
