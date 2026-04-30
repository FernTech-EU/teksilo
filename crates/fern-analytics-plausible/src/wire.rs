//! Plausible JSON wire format.
//!
//! Plausible's `/api/event` endpoint expects a JSON body with the
//! shape `{name, url, domain, props?}`. See
//! <https://plausible.io/docs/events-api>.
//!
//! The desktop integration faces a small impedance mismatch:
//! Plausible is web-centric and `url` is mandatory. We mint a
//! synthetic `app://<domain>/<event-name>` URL so each event
//! aggregates as a "page" in Plausible. Custom events with prop
//! filters work the same way they would on the web.

use std::collections::BTreeMap;

use fern_core::telemetry::{OwnedEvent, OwnedPropValue};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PlausibleEvent<'a> {
    pub name: &'a str,
    pub url: String,
    pub domain: &'a str,
    /// Properties as a flat string-keyed map. Plausible accepts
    /// strings, numbers, and booleans; we coerce all our owned
    /// values into Plausible-compatible primitives.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, serde_json::Value>,
}

impl<'a> PlausibleEvent<'a> {
    /// Build a wire-ready event from an `OwnedEvent`. The synthetic
    /// `url` carries the event name in the path so each
    /// `intent.dispatched` etc. shows up as a distinct page in the
    /// Plausible dashboard.
    pub fn from_owned(
        owned: &'a OwnedEvent,
        domain: &'a str,
        synthetic_scheme: &str,
    ) -> Self {
        let url = format!("{synthetic_scheme}://{domain}/{}", owned.name);
        let mut props: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for prop in &owned.props {
            props.insert(prop.key.clone(), owned_prop_to_json(&prop.value));
        }
        Self {
            name: &owned.name,
            url,
            domain,
            props,
        }
    }
}

fn owned_prop_to_json(value: &OwnedPropValue) -> serde_json::Value {
    match value {
        OwnedPropValue::Str(s) => serde_json::Value::String(s.clone()),
        OwnedPropValue::U32(n) => serde_json::Value::from(*n),
        OwnedPropValue::I64(n) => serde_json::Value::from(*n),
        OwnedPropValue::F64Bucket(b) => {
            serde_json::Value::String(format!("{}-{}", b.min_x100, b.max_x100))
        }
        OwnedPropValue::Bool(b) => serde_json::Value::Bool(*b),
        OwnedPropValue::HistogramStrU32(entries) => {
            // Encode the histogram as a JSON object — Plausible
            // ignores nested objects in props but keeping the data
            // structured here means the same wire payload is usable
            // by an OTLP/PostHog adapter without lossy conversion.
            let mut obj = serde_json::Map::new();
            for (k, v) in entries {
                obj.insert(k.clone(), serde_json::Value::from(*v));
            }
            serde_json::Value::Object(obj)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::telemetry::{EventCategory, OwnedProp, OwnedPropValue};
    use std::time::SystemTime;

    fn sample_event() -> OwnedEvent {
        OwnedEvent {
            name: "intent.dispatched".into(),
            category: EventCategory::Intent,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: None,
            session_id: "test".into(),
            schema_version: 1,
            props: vec![
                OwnedProp {
                    key: "name".into(),
                    value: OwnedPropValue::Str("app.save".into()),
                },
                OwnedProp {
                    key: "source".into(),
                    value: OwnedPropValue::Str("shortcut".into()),
                },
            ],
        }
    }

    #[test]
    fn synthetic_url_carries_event_name() {
        let owned = sample_event();
        let event = PlausibleEvent::from_owned(&owned, "skribisto.app", "app");
        assert_eq!(event.url, "app://skribisto.app/intent.dispatched");
        assert_eq!(event.domain, "skribisto.app");
        assert_eq!(event.name, "intent.dispatched");
    }

    #[test]
    fn props_flatten_to_string_or_primitive() {
        let owned = sample_event();
        let event = PlausibleEvent::from_owned(&owned, "skribisto.app", "app");
        assert_eq!(event.props.len(), 2);
        assert_eq!(
            event.props.get("name"),
            Some(&serde_json::Value::String("app.save".into()))
        );
        assert_eq!(
            event.props.get("source"),
            Some(&serde_json::Value::String("shortcut".into()))
        );
    }

    #[test]
    fn empty_props_serialize_without_field() {
        let owned = OwnedEvent {
            name: "lifecycle.app_started".into(),
            category: EventCategory::Lifecycle,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: None,
            session_id: "s".into(),
            schema_version: 1,
            props: vec![],
        };
        let event = PlausibleEvent::from_owned(&owned, "x.app", "app");
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("props"));
    }

    #[test]
    fn json_round_trip_matches_plausible_shape() {
        let owned = sample_event();
        let event = PlausibleEvent::from_owned(&owned, "skribisto.app", "app");
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            json["name"],
            serde_json::Value::String("intent.dispatched".into())
        );
        assert_eq!(
            json["url"],
            serde_json::Value::String("app://skribisto.app/intent.dispatched".into())
        );
        assert_eq!(
            json["domain"],
            serde_json::Value::String("skribisto.app".into())
        );
        assert_eq!(json["props"]["name"], "app.save");
    }
}
