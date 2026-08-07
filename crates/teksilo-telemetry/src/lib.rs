// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `teksilo-telemetry` — privacy-respecting product analytics for Teksilo.
//!
//! This crate ships the foundational pieces:
//!
//! - [`ConsentStore`] — persisted consent state atop
//!   [`teksilo_settings::SettingsFile`].
//! - [`InstallId`] — pseudonymous-mode UUID with 13-month rotation.
//! - [`TelemetryBundle`] / [`OpenedTelemetry`] — declarative
//!   configuration following the same pattern as
//!   [`teksilo_settings::SettingsBundle`].
//! - [`DynamicReporter`] — runtime mode-switch wrapper holding both
//!   anonymous-mode and pseudonymous-mode adapters and forwarding to
//!   the active one.
//! - [`TelemetryExt`] — convenience accessors for `BuildContext` /
//!   `EventContext` (`use teksilo_telemetry::TelemetryExt;`).
//! - In-memory event queue.
//! - [`StubReporter`] — testing-only adapter that collects events into
//!   a `Vec`.
//! - Hand-written framework events in [`generated`].
//!
//! Re-exports the pure trait/type surface from `teksilo_core::telemetry`
//! so apps need only `use teksilo_telemetry::*` to access the full API.

pub mod bundle;
pub mod consent;
pub mod dynamic_reporter;
pub mod ext;
pub mod generated;
pub mod install_id;
pub mod queue;
pub mod scopes;
pub mod stub;

pub use bundle::{
    DataResidencyRegion, OpenedTelemetry, PrivacyPolicy, TelemetryBundle, TelemetryBundleError,
    TelemetryMode,
};
pub use consent::{ConsentFile, ConsentStore, PersistedConsentState};
pub use dynamic_reporter::DynamicReporter;
pub use ext::TelemetryExt;
pub use install_id::{InstallId, InstallIdFile};
pub use queue::{EventQueue, InMemoryEventQueue, PersistentEventQueue, PersistentQueueError};
pub use stub::StubReporter;

// Re-exports from teksilo-core so apps need only one `use`.
pub use teksilo_core::telemetry::{
    ConsentScope, ConsentState, Event, EventCategory, F64Bucket, IntentSource, OwnedEvent,
    OwnedProp, OwnedPropValue, Prop, PropValue, RemoteDataExport, RemoteEvent, RemoteValue,
    TelemetryContext, TelemetryError, UsageReporter,
};
