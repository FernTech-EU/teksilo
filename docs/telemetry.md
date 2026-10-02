<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Telemetry

The optional telemetry subsystem routes application events through a consent
store and a reporting adapter. Enable the `telemetry` feature on `teksilo` and
add `teksilo-telemetry` at the matching framework version.

## Minimal example

This example uses `StubReporter`, an in-memory reporter that sends no network
requests. It demonstrates installation and the consent UI without requiring a
collector or credentials.

```rust
use std::rc::Rc;
use teksilo::prelude::*;
use teksilo::settings::SettingsBundle;
use teksilo::widgets::PrivacySettings;
use teksilo_telemetry::{StubReporter, TelemetryBundle};

fn main() {
    let reporter = Rc::new(StubReporter::anonymous());
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .application("eu", "Example", "TelemetryDemo")
        .settings(SettingsBundle::new())
        .telemetry(TelemetryBundle::new(1).with_anonymous(reporter))
        .initial_window(
            WindowConfig::new()
                .title("Telemetry settings")
                .size(640, 600)
                .root(|tree, _| tree.add(PrivacySettings::new())),
        )
        .run();
}
```

## Common operations

- Use `TelemetryBundle::with_anonymous` or `with_pseudonymous` to install an
  adapter. At least one is required.
- Install `PrivacySettings` for consent, scope controls, and supported data
  export or erasure actions.
- Use `TelemetryExt::try_telemetry` from a build context to access the opened
  consent store, reporter, and recent-event log.
- Define application events in the telemetry schema and use the generated
  event API. See the [schema example](../examples/telemetry_codegen/src/main.rs).
- Inspect the reporter's recent log during development. It records events that
  passed the consent gate, not proof that a remote server received them.

## Choose an adapter

| Adapter | Purpose | Setup |
| --- | --- | --- |
| `StubReporter` | Local tests | Included in `teksilo-telemetry` |
| `PlausibleAdapter` | Anonymous reporting to Plausible | Add `teksilo-analytics-plausible` and configure the domain and endpoint |
| `OtlpAdapter` | OTLP/HTTP logs | Configure the OTLP adapter and collector |
| `TeksiloAdapter` | Teksilo's separate gRPC collector | Requires the excluded native-adapter project and its collector dependencies |

For a network example, run or inspect
[`telemetry_plausible`](../examples/telemetry_plausible/src/main.rs).
The native adapter is excluded from the main workspace and currently declares
older framework dependency requirements. It is not the starting point for a
current application; reconcile its dependencies and collector setup first.

## Consent and limits

The framework's `DynamicReporter` checks consent before forwarding an event.
Application code that calls an adapter directly bypasses that gate. Keep
application reporting on the framework path.

Anonymous mode does not attach an installation identifier. Pseudonymous mode
can attach one. Inspect event payloads and server behavior when defining the
application's privacy policy; the mode name alone does not characterize all
collected data.

Data export, erasure, retry, and queue persistence depend on the chosen adapter.
A recent-event entry does not guarantee remote delivery. Withdrawing consent
and deleting data already stored on a server are separate operations.

The supplied controls support application privacy workflows. Their presence
does not establish legal compliance for the application or its backend.

## Reference

- [Telemetry source](../crates/teksilo-telemetry/src/lib.rs)
- [Privacy settings widget](widgets/privacy_settings.md)
- [Plausible adapter](../crates/teksilo-analytics-plausible/src/lib.rs)


## Engineering reference

[Implementation details and review history](https://github.com/ferntech-eu/teksilo/blob/main/engineering/docs/telemetry.md)
are retained in the repository.
