<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# PrivacySettings

> Available under: `#[cfg(feature = "telemetry")]`

PrivacySettings — a user-facing panel for telemetry consent management.

Embeddable in any container — typically a `Dialog` for first-run consent
or a dedicated tab in the app's settings UI.  Reads from
`OpenedTelemetry` and writes to `ConsentStore`; the UI rebuilds
whenever the consent state signal changes.  When no telemetry is registered
in `app_state` the widget renders a graceful placeholder so apps without
analytics pay nothing.

# Sections (top-to-bottom)

1. **Plain-language Art. 13 notice** — controller, processor name,
   purposes, lawful basis, retention, recipients, withdrawal right.
   All strings flow through `tr_widget!` against keys defined in
   `crates/teksilo-widgets/locales/en-US.ftl`
   and `fr-FR.ftl` under the
   `privacy-*` namespace. Apps install the framework bundle via
   `I18nConfig::framework_locales(teksilo_widgets::framework_locales())`.
2. **Per-scope toggles** — one per
   `ConsentScope` field, intersected
   with `reporter.supported_scopes()` so toggles for
   unsupported scopes are hidden, not just disabled. Toggles work
   from `Unknown` (auto-transition to `Granted` with the toggled
   scope) and `Granted` states; they're disabled when state is
   `Denied` until the user clicks Withdraw → Accept.
3. **Accept all / Reject all** — equal-prominence buttons (CNIL
   parity rule, GDPR Art. 7).
4. **Identity row** (pseudonymous mode only) — install_id display,
   Get-my-data button (Art. 15 + 20), Erase-my-data button (Art. 17).
5. **Inspect data sent** — accordion listing the most-recent events
   from the bundle's recent-log ring buffer.
6. **Mode switch** (when both adapters configured) — confirm-button
   pair to flip anonymous ↔ pseudonymous.
7. **Footer** — Withdraw consent (equal prominence to Accept,
   GDPR Art. 7(3)).

When no `OpenedTelemetry` is registered in `app_state`, the
widget renders a "Telemetry not configured" placeholder. Apps that
ship without analytics pay nothing.

```ignore
// Embed in a Dialog for first-run consent (compact mode).
let panel = PrivacySettings::new()
    .compact(true)
    .data_processor_name("Acme Corp")
    .privacy_policy_url("https://example.com/privacy");
```

## Builder methods at a glance

`compact`, `show_identity_row`, `show_mode_switch`, `show_inspect`, `inspect_event_count`, `privacy_policy_url`, `data_processor_name`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/index.html)

## `pub struct PrivacySettings`

Settings widget for telemetry consent. Construct with
`PrivacySettings::new` and embed in any container.

```rust
pub struct PrivacySettings { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create a `PrivacySettings` widget with full layout and all sections shown.

#### `pub fn compact(mut self, compact: bool) -> Self`

Use a compact layout suited for first-run modals: hides the mode-switch
section and tightens spacing. Defaults to `false` (full settings panel).

#### `pub fn show_identity_row(mut self, show: bool) -> Self`

Show or hide the install-id / GDPR Art. 15 + 17 identity row in
pseudonymous mode. Set to `false` when the host app supplies its own
equivalent UI. Defaults to `true`.

#### `pub fn show_mode_switch(mut self, show: bool) -> Self`

Show or hide the anonymous ↔ pseudonymous mode-switch section when both
adapters are configured. Has no effect if only one mode is available.
Defaults to `true`.

#### `pub fn show_inspect(mut self, show: bool) -> Self`

Show or hide the "Inspect data sent" accordion that lists recent events
from the telemetry ring buffer. Defaults to `true`.

#### `pub fn inspect_event_count(mut self, n: usize) -> Self`

Maximum number of recent events shown in the inspect accordion.
Clamped to at least 1. Defaults to 50.

#### `pub fn privacy_policy_url(mut self, url: impl Into<String>) -> Self`

Surface a "Read full privacy policy" link in the Art. 13 notice.
When not set the link is hidden — the controller is responsible for
hosting their own policy page.

#### `pub fn data_processor_name(mut self, name: impl Into<String>) -> Self`

Plain-text controller name used in the Art. 13 notice ("Data is
processed by `<name>`"). Defaults to "the application".
