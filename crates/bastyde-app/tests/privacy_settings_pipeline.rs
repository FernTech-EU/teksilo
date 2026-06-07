//! Integration tests for the `PrivacySettings` widget.
//!
//! Verifies that programmatic interactions with `ConsentStore` (matching
//! what the widget's buttons would do) update the state correctly.

#![cfg(feature = "telemetry")]

use std::rc::Rc;
use std::time::Duration;

use bastyde_app::BastydeAppBuilder;
use bastyde_settings::{AppPaths, SettingsBundle};
use bastyde_telemetry::{
    ConsentState, StubReporter, TelemetryBundle, TelemetryMode, UsageReporter,
};
use tempfile::tempdir;

#[test]
fn accept_all_grants_supported_scopes() {
    // The widget's "Accept all" button calls `consent.grant(supported, endpoint)`.
    // We exercise the same path directly since simulating button clicks
    // through the tree's event dispatch would be deep plumbing for no
    // additional coverage.
    let dir = tempdir().unwrap();
    let app = BastydeAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
                .with_default_mode(TelemetryMode::Anonymous)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    let opened = app
        .tree
        .app_context()
        .app_state::<bastyde_telemetry::OpenedTelemetry>()
        .expect("telemetry installed");

    assert!(matches!(
        opened.consent.state_signal().get(),
        ConsentState::Unknown
    ));

    let supported = opened.reporter.supported_scopes();
    let endpoint = opened.reporter.endpoint().to_string();
    opened.consent.grant(supported, &endpoint).unwrap();

    let scope = match opened.consent.state_signal().get() {
        ConsentState::Granted(s) => s,
        other => panic!("expected Granted, got {other:?}"),
    };
    // The granted scope must match what the adapter reported as
    // supported (CNIL parity rule: don't grant scopes the adapter
    // can't honor). The stub reporter reports all-supported except
    // session_recording.
    assert_eq!(scope, supported);
}

#[test]
fn reject_all_denies() {
    let dir = tempdir().unwrap();
    let app = BastydeAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    let opened = app
        .tree
        .app_context()
        .app_state::<bastyde_telemetry::OpenedTelemetry>()
        .unwrap();

    opened.consent.deny().unwrap();
    assert!(matches!(
        opened.consent.state_signal().get(),
        ConsentState::Denied
    ));
}

#[test]
fn set_or_grant_scope_transitions_unknown_to_granted() {
    // The single-toggle interaction: user flips one scope on while
    // state is Unknown. The widget calls `set_or_grant_scope`, which
    // transitions Unknown → Granted with that one scope on.
    let dir = tempdir().unwrap();
    let app = BastydeAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    let opened = app
        .tree
        .app_context()
        .app_state::<bastyde_telemetry::OpenedTelemetry>()
        .unwrap();
    let endpoint = opened.reporter.endpoint().to_string();

    assert!(matches!(
        opened.consent.state_signal().get(),
        ConsentState::Unknown
    ));

    opened
        .consent
        .set_or_grant_scope(&endpoint, |s| s.anonymous_metrics = true)
        .unwrap();

    if let ConsentState::Granted(scope) = opened.consent.state_signal().get() {
        assert!(scope.anonymous_metrics);
        assert!(!scope.crash_reports);
    } else {
        panic!("expected Granted");
    }
}

#[test]
fn set_or_grant_scope_blocked_by_denied() {
    let dir = tempdir().unwrap();
    let app = BastydeAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    let opened = app
        .tree
        .app_context()
        .app_state::<bastyde_telemetry::OpenedTelemetry>()
        .unwrap();

    opened.consent.deny().unwrap();
    let applied = opened
        .consent
        .set_or_grant_scope(opened.reporter.endpoint(), |s| s.anonymous_metrics = true)
        .unwrap();
    assert!(!applied);
    assert!(matches!(
        opened.consent.state_signal().get(),
        ConsentState::Denied
    ));
}

