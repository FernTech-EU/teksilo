//! Demonstrates `bastyde_telemetry_codegen::include_telemetry_schema!`:
//! the macro reads `telemetry/events.yaml` at compile time and expands
//! to typed `emit_*` functions and enum types.

// Expand the schema — this generates:
//   pub const SCHEMA_VERSION: u32;
//   pub enum IntentDispatchedSource { Shortcut, Menu, Handler, Programmatic, Accessibility }
//   pub enum LifecycleAppStartedThemeKind { Light, Dark, Custom }
//   pub fn emit_intent_dispatched(…)
//   pub fn emit_lifecycle_app_started(…)
//   pub fn emit_lifecycle_app_exited(…)
//   pub fn emit_widget_census(…)
bastyde_telemetry_codegen::include_telemetry_schema!("telemetry/events.yaml");

fn main() {
    let stub = bastyde_telemetry::StubReporter::anonymous();
    let session = "demo-session";

    emit_lifecycle_app_started(
        &stub,
        None,
        session,
        "1.0.0",
        env!("CARGO_PKG_VERSION"),
        LifecycleAppStartedThemeKind::Light,
    );

    emit_intent_dispatched(
        &stub,
        None,
        session,
        "app.save",
        IntentDispatchedSource::Shortcut,
    );

    emit_widget_census(&stub, None, session, 47);
    emit_lifecycle_app_exited(&stub, None, session, 120);

    let recorded = stub.recorded_count();
    println!("Captured {recorded} events via codegen'd emit_* functions.");
    println!("Schema version: {SCHEMA_VERSION}");

    assert_eq!(IntentDispatchedSource::Shortcut.as_str(), "shortcut");
    assert_eq!(LifecycleAppStartedThemeKind::Dark.as_str(), "dark");
    assert_eq!(recorded, 4);
    println!("All assertions passed.");
}
