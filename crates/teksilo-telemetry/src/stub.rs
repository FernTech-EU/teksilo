// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Stub reporter for tests.
//!
//! Collects events into a `Mutex<Vec<OwnedEvent>>` so tests can assert
//! on what was emitted. Always reports the `"stub"` adapter name and
//! `"stub://"` endpoint.
//!
//! Two flavors:
//!
//! - `StubReporter::anonymous()` — `install_id() == None`,
//!   `erase_remote_data` / `fetch_remote_data` return the
//!   `*Unsupported` errors (matches anonymous-mode adapters).
//! - `StubReporter::pseudonymous(uuid)` — `install_id() == Some(uuid)`,
//!   `erase_remote_data` clears the recorded vec, `fetch_remote_data`
//!   returns a `RemoteDataExport` mirroring what was recorded.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::SystemTime;

use teksilo_core::telemetry::{
    ConsentScope, Event, OwnedEvent, RemoteDataExport, RemoteEvent, TelemetryError, UsageReporter,
};

pub struct StubReporter {
    pub recorded: Mutex<Vec<OwnedEvent>>,
    install_id: Option<String>,
    /// Mirrors recorded events into the fetch result. Set to `false`
    /// for adapters that should reject fetch requests entirely.
    fetch_supported: bool,
    erase_supported: bool,
}

impl StubReporter {
    /// Anonymous-mode stub: no install id, fetch + erase return
    /// `Err(*Unsupported)`.
    pub fn anonymous() -> Self {
        Self {
            recorded: Mutex::new(Vec::new()),
            install_id: None,
            fetch_supported: false,
            erase_supported: false,
        }
    }

    /// Pseudonymous-mode stub with the given install id. Fetch + erase
    /// operate on the local recorded vec.
    pub fn pseudonymous(install_id: impl Into<String>) -> Self {
        Self {
            recorded: Mutex::new(Vec::new()),
            install_id: Some(install_id.into()),
            fetch_supported: true,
            erase_supported: true,
        }
    }

    pub fn recorded_count(&self) -> usize {
        self.recorded
            .lock()
            .expect("StubReporter mutex poisoned")
            .len()
    }

    pub fn last_recorded_name(&self) -> Option<String> {
        self.recorded
            .lock()
            .expect("StubReporter mutex poisoned")
            .last()
            .map(|e| e.name.clone())
    }

    pub fn clear_recorded(&self) {
        self.recorded
            .lock()
            .expect("StubReporter mutex poisoned")
            .clear();
    }
}

impl UsageReporter for StubReporter {
    fn record(&self, event: &Event<'_>) {
        self.recorded
            .lock()
            .expect("StubReporter mutex poisoned")
            .push(event.to_owned());
    }

    fn discard_pending(&self) -> Result<(), TelemetryError> {
        self.clear_recorded();
        Ok(())
    }

    fn erase_remote_data(&self) -> Result<(), TelemetryError> {
        if !self.erase_supported {
            return Err(TelemetryError::ErasureUnsupported);
        }
        self.clear_recorded();
        Ok(())
    }

    fn fetch_remote_data(&self) -> Result<RemoteDataExport, TelemetryError> {
        if !self.fetch_supported {
            return Err(TelemetryError::FetchUnsupported);
        }
        let events = self
            .recorded
            .lock()
            .expect("StubReporter mutex poisoned")
            .iter()
            .map(|e| RemoteEvent {
                name: e.name.to_string(),
                timestamp: e.timestamp,
                properties: BTreeMap::new(), // simplified — tests assert on count, not props
            })
            .collect();
        Ok(RemoteDataExport {
            install_id: self.install_id.clone().unwrap_or_default(),
            fetched_at: SystemTime::now(),
            adapter: "stub",
            endpoint: "stub://".to_string(),
            schema_version: 1,
            events,
            server_metadata: BTreeMap::new(),
        })
    }

    fn install_id(&self) -> Option<&str> {
        self.install_id.as_deref()
    }

    fn adapter_name(&self) -> &'static str {
        "stub"
    }

    fn endpoint(&self) -> &str {
        "stub://"
    }

    fn supported_scopes(&self) -> ConsentScope {
        ConsentScope::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::telemetry::EventCategory;

    #[test]
    fn anonymous_stub_rejects_fetch_and_erase() {
        let r = StubReporter::anonymous();
        assert!(matches!(
            r.erase_remote_data(),
            Err(TelemetryError::ErasureUnsupported)
        ));
        assert!(matches!(
            r.fetch_remote_data(),
            Err(TelemetryError::FetchUnsupported)
        ));
        assert!(r.install_id().is_none());
    }

    #[test]
    fn pseudonymous_stub_round_trips() {
        let r = StubReporter::pseudonymous("test-uuid");
        let props = [];
        let e = Event {
            name: "intent.dispatched",
            category: EventCategory::Intent,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: Some("test-uuid"),
            session_id: "s",
            schema_version: 1,
            props: &props,
        };
        r.record(&e);
        assert_eq!(r.recorded_count(), 1);
        let export = r.fetch_remote_data().unwrap();
        assert_eq!(export.install_id, "test-uuid");
        assert_eq!(export.events.len(), 1);
        r.erase_remote_data().unwrap();
        assert_eq!(r.recorded_count(), 0);
    }
}
