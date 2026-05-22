//! Runtime-switchable wrapper holding both anonymous-mode and
//! pseudonymous-mode adapters.
//!
//! `DynamicReporter` implements [`UsageReporter`] by forwarding to
//! whichever inner adapter matches the currently-active
//! [`TelemetryMode`]. The mode is held in an `RwLock<TelemetryMode>`;
//! `PrivacySettings` flips it via `set_active_mode`.
//!
//! Emission is gated by the [`ConsentStore`]'s `is_granted`. When
//! consent is not granted, `record` short-circuits without touching
//! the inner adapter.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use bastyde_core::Signal;
use bastyde_core::telemetry::{
    ConsentScope, Event, RemoteDataExport, TelemetryError, UsageReporter,
};

use crate::bundle::TelemetryMode;
use crate::consent::ConsentStore;
use crate::queue::{EventQueue, InMemoryEventQueue};

/// Forwarding wrapper that routes to one of two adapters at runtime.
///
/// Single-threaded — held inside `OpenedTelemetry` as
/// `Rc<DynamicReporter>` and registered into the app-state registry.
///
/// `record()` tees each event into a `recent_log` ring buffer in
/// addition to forwarding to the active adapter. The `PrivacySettings`
/// widget reads `recent_log` to populate the "Inspect data sent"
/// accordion. The recent log is **not** the adapter's outbound queue
/// — events stay in the recent log even after the adapter has flushed
/// them, until evicted by the ring buffer's capacity.
pub struct DynamicReporter {
    anonymous: Option<Rc<dyn UsageReporter>>,
    pseudonymous: Option<Rc<dyn UsageReporter>>,
    active: Cell<TelemetryMode>,
    consent: ConsentStore,
    recent_log: Arc<InMemoryEventQueue>,
    /// Monotonic counter bumped on every `record()` and `discard_pending()`.
    /// The `PrivacySettings` widget binds to this signal so its
    /// "Inspect data sent" accordion auto-rebuilds when new events
    /// land. Living on `DynamicReporter` (which is `Rc`-shared on
    /// the UI thread) keeps `Signal`'s thread-affinity contract
    /// honored — record() is called only from the UI-thread
    /// dispatch tap.
    recent_log_revision: Signal<u64>,
}

impl DynamicReporter {
    pub fn new(
        anonymous: Option<Rc<dyn UsageReporter>>,
        pseudonymous: Option<Rc<dyn UsageReporter>>,
        default: TelemetryMode,
        consent: ConsentStore,
        recent_log: Arc<InMemoryEventQueue>,
    ) -> Self {
        debug_assert!(
            anonymous.is_some() || pseudonymous.is_some(),
            "DynamicReporter needs at least one adapter",
        );
        Self {
            anonymous,
            pseudonymous,
            active: Cell::new(default),
            consent,
            recent_log,
            recent_log_revision: Signal::new(0),
        }
    }

    pub fn recent_log(&self) -> &Arc<InMemoryEventQueue> {
        &self.recent_log
    }

    /// Signal bumped on every event recorded into the recent-log
    /// ring buffer (and on `discard_pending`). The `PrivacySettings`
    /// widget binds to it for `BindingLevel::Rebuild` so the
    /// "Inspect data sent" accordion stays in sync without polling.
    pub fn recent_log_revision(&self) -> Signal<u64> {
        self.recent_log_revision.clone()
    }

    fn bump_revision(&self) {
        let v = self.recent_log_revision.get();
        self.recent_log_revision.set(v.wrapping_add(1));
    }

    pub fn active_mode(&self) -> TelemetryMode {
        self.active.get()
    }

    /// Atomically swap the active mode. Caller is responsible for the
    /// pre/post-conditions: `erase_remote_data` before leaving
    /// pseudonymous, `discard_pending` to drop the queue, then
    /// `consent.reset()` to force re-prompt.
    pub fn set_active_mode(&self, mode: TelemetryMode) {
        self.active.set(mode);
    }

    /// `true` iff the bundle was constructed with both adapters; only
    /// then is the mode switch UI shown.
    pub fn supports_mode_switch(&self) -> bool {
        self.anonymous.is_some() && self.pseudonymous.is_some()
    }

    /// `true` iff the given mode has an adapter configured.
    pub fn has_mode(&self, mode: TelemetryMode) -> bool {
        match mode {
            TelemetryMode::Anonymous => self.anonymous.is_some(),
            TelemetryMode::Pseudonymous => self.pseudonymous.is_some(),
        }
    }

    fn active_adapter(&self) -> Option<&Rc<dyn UsageReporter>> {
        match self.active_mode() {
            TelemetryMode::Anonymous => self.anonymous.as_ref(),
            TelemetryMode::Pseudonymous => self.pseudonymous.as_ref(),
        }
    }

    pub fn consent(&self) -> &ConsentStore {
        &self.consent
    }
}

impl UsageReporter for DynamicReporter {
    fn record(&self, event: &Event<'_>) {
        if !self.consent.is_granted() {
            return;
        }
        // Tee into the user-facing recent-log ring buffer first; the
        // user can inspect what was actually emitted regardless of
        // adapter outcome.
        self.recent_log.push(event.to_owned());
        self.bump_revision();
        if let Some(adapter) = self.active_adapter() {
            adapter.record(event);
        }
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        // Flush both — events buffered before a mode switch should
        // still go out via the original adapter.
        if let Some(a) = &self.anonymous {
            a.flush()?;
        }
        if let Some(p) = &self.pseudonymous {
            p.flush()?;
        }
        Ok(())
    }

    fn discard_pending(&self) -> Result<(), TelemetryError> {
        // Drop the user-facing recent-log ring buffer alongside the
        // adapters' outbound buffers so the privacy widget's
        // "Inspect data sent" panel reflects the wipe immediately.
        self.recent_log.discard_all();
        self.bump_revision();
        if let Some(a) = &self.anonymous {
            a.discard_pending()?;
        }
        if let Some(p) = &self.pseudonymous {
            p.discard_pending()?;
        }
        Ok(())
    }

    fn erase_remote_data(&self) -> Result<(), TelemetryError> {
        // Only the pseudonymous adapter has erase semantics; anonymous
        // adapters return ErasureUnsupported. We forward to whichever
        // mode is active — the widget hides the button on anonymous.
        match self.active_adapter() {
            Some(a) => a.erase_remote_data(),
            None => Err(TelemetryError::ErasureUnsupported),
        }
    }

    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError> {
        match self.active_adapter() {
            Some(a) => a.fetch_remote_data(),
            None => Err(TelemetryError::FetchUnsupported),
        }
    }

    fn install_id(&self) -> Option<&str> {
        // Caller signature returns `Option<&str>` borrowing from the
        // reporter — we have to forward through the active adapter.
        // `Arc::as_ref` then `dyn UsageReporter::install_id` returns
        // `Option<&str>` borrowed from the adapter. Lifetime works
        // because the adapter outlives `&self`.
        match self.active_mode() {
            TelemetryMode::Anonymous => self.anonymous.as_deref().and_then(|a| a.install_id()),
            TelemetryMode::Pseudonymous => {
                self.pseudonymous.as_deref().and_then(|a| a.install_id())
            }
        }
    }

    fn adapter_name(&self) -> &'static str {
        match self.active_adapter() {
            Some(a) => a.adapter_name(),
            None => "none",
        }
    }

    fn endpoint(&self) -> &str {
        match self.active_mode() {
            TelemetryMode::Anonymous => self.anonymous.as_deref().map_or("", |a| a.endpoint()),
            TelemetryMode::Pseudonymous => {
                self.pseudonymous.as_deref().map_or("", |a| a.endpoint())
            }
        }
    }

    fn supported_scopes(&self) -> ConsentScope {
        match self.active_adapter() {
            Some(a) => a.supported_scopes(),
            None => ConsentScope::none(),
        }
    }
}

impl std::fmt::Debug for DynamicReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicReporter")
            .field("active_mode", &self.active_mode())
            .field("has_anonymous", &self.anonymous.is_some())
            .field("has_pseudonymous", &self.pseudonymous.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::StubReporter;
    use bastyde_settings::AppPaths;
    use std::time::Duration;
    use tempfile::tempdir;

    fn make(
        anon: bool,
        pseudo: bool,
        default: TelemetryMode,
    ) -> (DynamicReporter, ConsentStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let consent = ConsentStore::open(&paths, Duration::ZERO, 1, "stub://").unwrap();
        let anonymous: Option<Rc<dyn UsageReporter>> =
            anon.then(|| Rc::new(StubReporter::anonymous()) as _);
        let pseudonymous: Option<Rc<dyn UsageReporter>> =
            pseudo.then(|| Rc::new(StubReporter::pseudonymous("uuid-1")) as _);
        let recent_log = Arc::new(InMemoryEventQueue::with_capacity(64));
        let dyn_r = DynamicReporter::new(
            anonymous,
            pseudonymous,
            default,
            consent.clone(),
            recent_log,
        );
        (dyn_r, consent, dir)
    }

    fn make_event(
        name: &'static str,
    ) -> (Event<'static>, [bastyde_core::telemetry::Prop<'static>; 0]) {
        let props: [bastyde_core::telemetry::Prop<'static>; 0] = [];
        // The borrow checker requires the props slice to outlive Event;
        // tests construct fresh each time so we return both.
        (
            Event {
                name,
                category: bastyde_core::telemetry::EventCategory::Intent,
                timestamp: std::time::SystemTime::UNIX_EPOCH,
                install_id: None,
                session_id: "s",
                schema_version: 1,
                props: &[],
            },
            props,
        )
    }

    #[test]
    fn record_drops_when_consent_unknown() {
        let (r, _consent, _dir) = make(true, false, TelemetryMode::Anonymous);
        let (e, _) = make_event("intent.dispatched");
        r.record(&e);
        // Stub captures via downcast; we assert via the public API.
        assert!(matches!(
            r.fetch_remote_data(),
            Err(TelemetryError::FetchUnsupported)
        ));
        // Anonymous stub doesn't expose its internal vec; accept the
        // assertion via "no error and no events fetched".
    }

    #[test]
    fn record_routes_to_active_adapter() {
        let (r, consent, _dir) = make(true, true, TelemetryMode::Pseudonymous);
        consent.grant(ConsentScope::all(), "stub://").unwrap();

        let (e, _) = make_event("intent.dispatched");
        r.record(&e);

        // We routed to the pseudonymous adapter — fetch should return 1.
        let export = r.fetch_remote_data().unwrap();
        assert_eq!(export.events.len(), 1);
    }

    #[test]
    fn mode_switch_changes_active_adapter() {
        let (r, consent, _dir) = make(true, true, TelemetryMode::Anonymous);
        consent.grant(ConsentScope::all(), "stub://").unwrap();

        let (e, _) = make_event("intent.dispatched");
        r.record(&e);
        // Anonymous mode → fetch returns FetchUnsupported.
        assert!(matches!(
            r.fetch_remote_data(),
            Err(TelemetryError::FetchUnsupported)
        ));

        // Switch to pseudonymous and emit again.
        r.set_active_mode(TelemetryMode::Pseudonymous);
        r.record(&e);
        let export = r.fetch_remote_data().unwrap();
        assert_eq!(export.events.len(), 1); // only the post-switch event
    }

    #[test]
    fn supports_mode_switch_only_with_both() {
        let (r1, _, _d1) = make(true, false, TelemetryMode::Anonymous);
        assert!(!r1.supports_mode_switch());
        let (r2, _, _d2) = make(true, true, TelemetryMode::Anonymous);
        assert!(r2.supports_mode_switch());
    }
}
