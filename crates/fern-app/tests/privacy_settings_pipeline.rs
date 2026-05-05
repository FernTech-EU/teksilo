//! Integration tests for the `PrivacySettings` widget.
//!
//! Verifies the widget builds in each telemetry configuration and
//! that programmatic interactions with `ConsentStore` (matching what
//! the widget's buttons would do) update the state correctly.

use std::rc::Rc;
use std::time::Duration;

use fern_app::FernAppBuilder;
use fern_canvas::SizeProposal;
use fern_settings::{AppPaths, SettingsBundle};
use fern_telemetry::{
    ConsentScope, ConsentState, StubReporter, TelemetryBundle, TelemetryMode, UsageReporter,
};
use fern_widgets::PrivacySettings;
use tempfile::tempdir;

#[test]
fn widget_builds_with_no_telemetry_shows_placeholder() {
    let dir = tempdir().unwrap();
    let mut app = FernAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .build_headless();

    app.tree.add(PrivacySettings::new());
    app.tree.layout(SizeProposal::exact(600.0, 400.0));
    // No panic = success. The placeholder renders a single TextWidget.
}

#[test]
fn widget_builds_with_anonymous_only() {
    let dir = tempdir().unwrap();
    let mut app = FernAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
                .with_default_mode(TelemetryMode::Anonymous)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    app.tree.add(
        PrivacySettings::new()
            .data_processor_name("Test Processor")
            .privacy_policy_url("https://example.test/privacy"),
    );
    app.tree.layout(SizeProposal::exact(600.0, 600.0));
}

#[test]
fn widget_builds_with_pseudonymous_only() {
    let dir = tempdir().unwrap();
    let mut app = FernAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_pseudonymous(
                    Rc::new(StubReporter::pseudonymous("uuid-test")) as Rc<dyn UsageReporter>
                )
                .with_default_mode(TelemetryMode::Pseudonymous)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    app.tree.add(PrivacySettings::new());
    app.tree.layout(SizeProposal::exact(600.0, 700.0));
}

#[test]
fn widget_builds_with_both_modes_and_compact() {
    let dir = tempdir().unwrap();
    let mut app = FernAppBuilder::new()
        .app_paths(AppPaths::for_testing(dir.path()))
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(Rc::new(StubReporter::anonymous()) as Rc<dyn UsageReporter>)
                .with_pseudonymous(
                    Rc::new(StubReporter::pseudonymous("uuid-mix")) as Rc<dyn UsageReporter>
                )
                .with_default_mode(TelemetryMode::Anonymous)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    // Compact mode hides the mode-switch section even when both
    // adapters are configured (reserved for first-run modal).
    app.tree.add(PrivacySettings::new().compact(true));
    app.tree.layout(SizeProposal::exact(500.0, 500.0));
}

#[test]
fn accept_all_grants_supported_scopes() {
    // The widget's "Accept all" button calls `consent.grant(supported, endpoint)`.
    // We exercise the same path directly since simulating button clicks
    // through the tree's event dispatch would be deep plumbing for no
    // additional coverage.
    let dir = tempdir().unwrap();
    let app = FernAppBuilder::new()
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
        .app_state::<fern_telemetry::OpenedTelemetry>()
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
    let app = FernAppBuilder::new()
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
        .app_state::<fern_telemetry::OpenedTelemetry>()
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
    let app = FernAppBuilder::new()
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
        .app_state::<fern_telemetry::OpenedTelemetry>()
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
    let app = FernAppBuilder::new()
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
        .app_state::<fern_telemetry::OpenedTelemetry>()
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

#[test]
fn rebuild_tracks_state_signal_changes() {
    // Confirms PrivacySettings re-builds when consent state changes
    // (i.e., the binding to `consent.state_signal()` works). We can't
    // observe rebuilds directly from outside, but mutating the
    // signal and then re-laying out without panicking is the smoke
    // test. A panic on a stale signal would surface here.
    let dir = tempdir().unwrap();
    let mut app = FernAppBuilder::new()
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
        .app_state::<fern_telemetry::OpenedTelemetry>()
        .unwrap()
        .clone();

    app.tree.add(PrivacySettings::new());
    app.tree.layout(SizeProposal::exact(600.0, 600.0));

    opened
        .consent
        .grant(ConsentScope::all(), opened.reporter.endpoint())
        .unwrap();
    app.tree.layout(SizeProposal::exact(600.0, 600.0));

    opened.consent.deny().unwrap();
    app.tree.layout(SizeProposal::exact(600.0, 600.0));
}
