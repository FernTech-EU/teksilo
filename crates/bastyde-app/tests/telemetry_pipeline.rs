//! End-to-end integration test for the telemetry pipeline.
//!
//! Exercises the full chain from `BastydeAppBuilder::telemetry(...)` →
//! app_state registration → dispatch tap in
//! `WidgetTree::dispatch_intent` → `StubReporter` recorded events.

#![cfg(feature = "telemetry")]

use std::rc::Rc;
use std::time::Duration;

use bastyde_app::BastydeAppBuilder;
use bastyde_canvas::SizeProposal;
use bastyde_core::BuildContext;
use bastyde_core::action::Action;
use bastyde_core::event::{Key, Modifiers, WidgetEvent};
use bastyde_core::intent::Intent;
use bastyde_core::shortcut::{KeyStroke, Shortcut};
use bastyde_core::widget::{LayoutContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_settings::{AppPaths, SettingsBundle};
use bastyde_telemetry::{ConsentScope, StubReporter, TelemetryBundle, TelemetryMode, UsageReporter};
use tempfile::tempdir;

/// Probe widget that registers a global shortcut + matching action so
/// dispatching a KeyDown synthesises an `Intent` through the full
/// dispatch path (shortcut intercept → enqueue_intent → drain →
/// dispatch_intent → telemetry tap).
#[derive(Debug)]
struct ProbeWidget;

impl Widget for ProbeWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("test.fire")
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
        );
        ctx.register_action(
            Action::new("test.fire").on_invoke(|_intent: &Intent, _ctx| {
                // No-op; we only care that the dispatch path ran.
            }),
        );
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[test]
fn shortcut_keydown_emits_intent_dispatched_through_tap() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let stub = Rc::new(StubReporter::anonymous());
    let stub_handle = stub.clone();

    let mut app = BastydeAppBuilder::new()
        .app_paths(paths.clone())
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(stub as Rc<dyn UsageReporter>)
                .with_default_mode(TelemetryMode::Anonymous)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    // Reach the OpenedTelemetry to grant consent before dispatching.
    let opened = app
        .tree
        .app_context()
        .app_state::<bastyde_telemetry::OpenedTelemetry>()
        .expect("telemetry installed");
    opened
        .consent
        .grant(ConsentScope::all(), opened.reporter.endpoint())
        .unwrap();

    app.tree.add(ProbeWidget);
    app.tree.layout(SizeProposal::exact(100.0, 100.0));

    // Dispatch the shortcut keystroke. Triggers shortcut → intent
    // enqueue → drain → dispatch_intent → telemetry tap.
    app.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::B,
        modifiers: Modifiers::CTRL,
        text: None,
    });

    assert_eq!(
        stub_handle.recorded_count(),
        1,
        "exactly one intent.dispatched event should have been recorded"
    );
    assert_eq!(
        stub_handle.last_recorded_name().as_deref(),
        Some("intent.dispatched")
    );
}

#[test]
fn no_telemetry_means_no_emission() {
    // Sanity: an app without `.telemetry(...)` has no TelemetryContext,
    // so the dispatch tap is a no-op (no panic, no allocation).
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let mut app = BastydeAppBuilder::new()
        .app_paths(paths)
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .build_headless();

    assert!(
        app.tree
            .app_context()
            .app_state::<bastyde_core::telemetry::TelemetryContext>()
            .is_none()
    );

    app.tree.add(ProbeWidget);
    app.tree.layout(SizeProposal::exact(100.0, 100.0));
    // Dispatching shouldn't panic even though no reporter is wired.
    app.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::B,
        modifiers: Modifiers::CTRL,
        text: None,
    });
}

#[test]
fn consent_gate_drops_events_until_granted() {
    let dir = tempdir().unwrap();
    let paths = AppPaths::for_testing(dir.path());

    let stub = Rc::new(StubReporter::anonymous());
    let stub_handle = stub.clone();

    let mut app = BastydeAppBuilder::new()
        .app_paths(paths)
        .settings(SettingsBundle::new().with_debounce(Duration::ZERO))
        .telemetry(
            TelemetryBundle::new(1)
                .with_anonymous(stub as Rc<dyn UsageReporter>)
                .with_default_mode(TelemetryMode::Anonymous)
                .with_debounce(Duration::ZERO),
        )
        .build_headless();

    // Pre-consent dispatch — should be dropped.
    app.tree.add(ProbeWidget);
    app.tree.layout(SizeProposal::exact(100.0, 100.0));
    app.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::B,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(stub_handle.recorded_count(), 0);

    // Grant consent and dispatch again.
    let opened = app
        .tree
        .app_context()
        .app_state::<bastyde_telemetry::OpenedTelemetry>()
        .expect("telemetry installed");
    opened
        .consent
        .grant(ConsentScope::all(), opened.reporter.endpoint())
        .unwrap();

    app.tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::B,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(stub_handle.recorded_count(), 1);
}
