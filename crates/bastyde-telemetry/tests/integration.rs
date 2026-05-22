//! Integration tests for the full `TelemetryBundle` lifecycle.
//!
//! Covers the end-to-end flow with a `StubReporter` standing in for
//! the eventual Plausible / PostHog adapters:
//!
//! - bundle open → consent grant → record → fetch → erase → discard
//! - re-open after schema bump → re-prompt fires
//! - mode switch wipes install_id and re-prompts
//! - consent gate drops events when not granted

use std::rc::Rc;
use std::time::{Duration, SystemTime};

use bastyde_core::telemetry::{ConsentScope, ConsentState, Event, EventCategory, TelemetryError};
use bastyde_settings::{AppPaths, SettingsStore};
use bastyde_telemetry::{
    StubReporter, TelemetryBundle, TelemetryExt, TelemetryMode, UsageReporter,
};
use tempfile::tempdir;

fn make_settings(paths: &AppPaths) -> SettingsStore {
    SettingsStore::open_with_delay(paths.config_file("general"), Duration::ZERO).unwrap()
}

fn make_event<'a>(name: &'static str, install_id: Option<&'a str>) -> Event<'a> {
    Event {
        name,
        category: EventCategory::Intent,
        timestamp: SystemTime::UNIX_EPOCH,
        install_id,
        session_id: "test-session",
        schema_version: 1,
        props: &[],
    }
}

#[test]
fn end_to_end_pseudonymous_lifecycle() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let settings = make_settings(&paths);

    // Keep a separate Rc<StubReporter> handle so the test can assert
    // on what was recorded — clones share the underlying Mutex<Vec>.
    let stub = Rc::new(StubReporter::pseudonymous("test-uuid"));
    let stub_handle = stub.clone();

    let opened = TelemetryBundle::new(1)
        .with_pseudonymous(stub as Rc<dyn UsageReporter>)
        .with_default_mode(TelemetryMode::Pseudonymous)
        .with_debounce(Duration::ZERO)
        .with_data_processor_name("Test Co.")
        .with_privacy_policy_url("https://example.com/privacy")
        .open(&paths, &settings)
        .unwrap();

    // 1. Initial state: Unknown, install_id present, no events.
    assert!(matches!(
        opened.consent.state_signal().get(),
        ConsentState::Unknown
    ));
    assert!(opened.install_id.is_some());
    let install_id = opened.install_id.as_ref().unwrap().get();
    assert!(!install_id.is_empty());
    assert_eq!(stub_handle.recorded_count(), 0);

    // 2. Record before consent: dropped by DynamicReporter.
    let event = make_event("intent.dispatched", Some(&install_id));
    opened.reporter.record(&event);
    assert_eq!(stub_handle.recorded_count(), 0, "consent gate must drop");

    // 3. Grant consent. Now records flow through.
    opened
        .consent
        .grant(ConsentScope::all(), opened.reporter.endpoint())
        .unwrap();
    opened.reporter.record(&event);
    opened.reporter.record(&event);
    assert_eq!(stub_handle.recorded_count(), 2);

    // 4. Fetch (Art. 15 / 20).
    let export = opened.reporter.fetch_remote_data().unwrap();
    assert_eq!(export.install_id, "test-uuid");
    assert_eq!(export.events.len(), 2);

    // 5. Erase (Art. 17).
    opened.reporter.erase_remote_data().unwrap();
    assert_eq!(stub_handle.recorded_count(), 0);

    // 6. Withdraw consent + discard pending. New events dropped again.
    opened.consent.withdraw().unwrap();
    opened.reporter.record(&event);
    assert_eq!(stub_handle.recorded_count(), 0);
}

#[test]
fn end_to_end_anonymous_no_install_id_no_fetch_no_erase() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let settings = make_settings(&paths);

    let stub = Rc::new(StubReporter::anonymous());
    let stub_handle = stub.clone();

    let opened = TelemetryBundle::new(1)
        .with_anonymous(stub as Rc<dyn UsageReporter>)
        .with_default_mode(TelemetryMode::Anonymous)
        .with_debounce(Duration::ZERO)
        .open(&paths, &settings)
        .unwrap();

    // No install_id, no fetch, no erase.
    assert!(opened.install_id.is_none());
    opened
        .consent
        .grant(ConsentScope::anonymous_metrics_only(), "stub://")
        .unwrap();

    let event = make_event("intent.dispatched", None);
    opened.reporter.record(&event);
    assert_eq!(stub_handle.recorded_count(), 1);

    assert!(matches!(
        opened.reporter.fetch_remote_data(),
        Err(TelemetryError::FetchUnsupported)
    ));
    assert!(matches!(
        opened.reporter.erase_remote_data(),
        Err(TelemetryError::ErasureUnsupported)
    ));
}

#[test]
fn schema_version_bump_triggers_reprompt() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let settings = make_settings(&paths);

    // Round 1: open with schema version 1 and grant.
    {
        let stub = Rc::new(StubReporter::anonymous());
        let opened = TelemetryBundle::new(1)
            .with_anonymous(stub as Rc<dyn UsageReporter>)
            .with_debounce(Duration::ZERO)
            .open(&paths, &settings)
            .unwrap();
        opened
            .consent
            .grant(
                ConsentScope::anonymous_metrics_only(),
                opened.reporter.endpoint(),
            )
            .unwrap();
        opened.consent.flush_now().unwrap();
    }

    // Round 2: reopen with schema version 2 — re-prompt should fire.
    let stub = Rc::new(StubReporter::anonymous());
    let opened = TelemetryBundle::new(2)
        .with_anonymous(stub as Rc<dyn UsageReporter>)
        .with_debounce(Duration::ZERO)
        .open(&paths, &settings)
        .unwrap();
    assert!(matches!(
        opened.consent.state_signal().get(),
        ConsentState::Unknown
    ));
}

#[test]
fn mode_switch_changes_active_adapter() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let settings = make_settings(&paths);

    let anon = Rc::new(StubReporter::anonymous());
    let pseudo = Rc::new(StubReporter::pseudonymous("uuid-2"));
    let anon_handle = anon.clone();
    let pseudo_handle = pseudo.clone();

    let opened = TelemetryBundle::new(1)
        .with_anonymous(anon as Rc<dyn UsageReporter>)
        .with_pseudonymous(pseudo as Rc<dyn UsageReporter>)
        .with_default_mode(TelemetryMode::Anonymous)
        .with_debounce(Duration::ZERO)
        .open(&paths, &settings)
        .unwrap();

    opened
        .consent
        .grant(ConsentScope::all(), opened.reporter.endpoint())
        .unwrap();

    // Anonymous mode active.
    let id = opened.install_id.as_ref().unwrap().get();
    let event = make_event("intent.dispatched", Some(&id));
    opened.reporter.record(&event);
    assert_eq!(anon_handle.recorded_count(), 1);
    assert_eq!(pseudo_handle.recorded_count(), 0);

    // Switch to pseudonymous.
    opened.reporter.set_active_mode(TelemetryMode::Pseudonymous);
    opened.reporter.record(&event);
    assert_eq!(anon_handle.recorded_count(), 1, "no new anon events");
    assert_eq!(pseudo_handle.recorded_count(), 1);

    assert!(opened.reporter.supports_mode_switch());
}

#[test]
fn opened_telemetry_is_clone() {
    // Sanity: the bundle handle is cheap to clone (Rc-shared).
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());
    let settings = make_settings(&paths);

    let opened = TelemetryBundle::new(1)
        .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
        .with_debounce(Duration::ZERO)
        .open(&paths, &settings)
        .unwrap();
    let clone1 = opened.clone();
    let clone2 = opened.clone();
    assert_eq!(clone1.event_schema_version, clone2.event_schema_version);
}

#[test]
fn telemetry_ext_trait_compiles() {
    // Compile-time check that TelemetryExt is import-public; the actual
    // accessors are exercised in bastyde-app's integration tests.
    fn _accepts<T: TelemetryExt>(_t: T) {}
}
