//! Extension traits exposing telemetry services on `BuildContext` /
//! `EventContext`.
//!
//! Same convention as
//! [`bastyde_settings::SettingsExt`](bastyde_settings::SettingsExt). Apps
//! `use bastyde_telemetry::TelemetryExt;` to reach `ctx.consent()`,
//! `ctx.usage_reporter()`, etc.

use bastyde_core::BuildContext;
use bastyde_core::widget::EventContext;

use crate::bundle::OpenedTelemetry;
use crate::consent::ConsentStore;
use crate::dynamic_reporter::DynamicReporter;

/// Convenience accessors for telemetry services attached to the app's
/// `app_state` registry.
pub trait TelemetryExt {
    /// The full bundle. Panics if `BastydeAppBuilder::telemetry(...)`
    /// was not called.
    fn telemetry(&self) -> &OpenedTelemetry {
        self.try_telemetry().unwrap_or_else(|| {
            panic!(
                "TelemetryExt::telemetry(): no OpenedTelemetry registered. \
                 Call BastydeAppBuilder::telemetry(TelemetryBundle::new(...)) at startup."
            )
        })
    }

    /// The consent store. Panics if no telemetry bundle was registered.
    fn consent(&self) -> &ConsentStore {
        &self.telemetry().consent
    }

    /// The active dynamic reporter (forwards to the adapter
    /// matching the current mode). Panics if no telemetry bundle was
    /// registered.
    fn usage_reporter(&self) -> &DynamicReporter {
        &self.telemetry().reporter
    }

    fn try_telemetry(&self) -> Option<&OpenedTelemetry>;
}

impl<'a> TelemetryExt for BuildContext<'a> {
    fn try_telemetry(&self) -> Option<&OpenedTelemetry> {
        self.app_state::<OpenedTelemetry>()
    }
}

impl<'a> TelemetryExt for EventContext<'a> {
    fn try_telemetry(&self) -> Option<&OpenedTelemetry> {
        self.app_state::<OpenedTelemetry>()
    }
}
