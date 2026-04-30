//! `SettingsKey<bool>` constants for the per-scope toggles.
//!
//! These keys live in the **app's** `SettingsStore` (i.e. `general.toml`),
//! mirrored from the `ConsentStore`'s on every mutation. The mirror
//! exists so the toggles survive even if the consent file is deleted
//! by hand — and so power users editing `general.toml` directly can
//! flip the toggles without going through the widget.
//!
//! The widget reads from the `ConsentStore`'s `Signal<ConsentState>`,
//! not from these keys; the keys are the persistence projection, not
//! the source of truth. `ConsentStore::grant` / `set_scope` updates
//! both.
//!
//! Endpoint and region overrides also live here. They are read once,
//! at `TelemetryBundle::open` time, and never observed mid-session
//! (changing the recipient mid-session would violate the Art. 13
//! notice the user consented to — see plan §11.4).

use fern_settings::SettingsKey;

// ----- consent scopes (mirror of ConsentStore.scope) -----

pub const TELEMETRY_ANONYMOUS_METRICS: SettingsKey<bool> =
    SettingsKey::new("telemetry.consent.anonymous_metrics", || false);

pub const TELEMETRY_CRASH_REPORTS: SettingsKey<bool> =
    SettingsKey::new("telemetry.consent.crash_reports", || false);

pub const TELEMETRY_FEATURE_FLAGS: SettingsKey<bool> =
    SettingsKey::new("telemetry.consent.feature_flags", || false);

// ----- runtime overrides -----

/// Optional endpoint override. When `Some(url)`, the adapter uses this
/// URL instead of its compiled-in default. Read once at startup; the
/// adapter holds it by value for the rest of the session.
///
/// Empty string is treated as "no override" (so power users editing
/// the TOML can clear the field without removing the line).
pub const TELEMETRY_ENDPOINT_OVERRIDE: SettingsKey<String> =
    SettingsKey::new("telemetry.endpoint_override", String::new);

/// Optional human-readable label for the data-residency region —
/// surfaced verbatim by the consent widget. When the user points at
/// a self-hosted endpoint they may want to label it ("EU self-hosted",
/// "Frankfurt", etc.).
pub const TELEMETRY_REGION_OVERRIDE: SettingsKey<String> =
    SettingsKey::new("telemetry.region_override", String::new);
