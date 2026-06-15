// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! OTLP/HTTP logs wire format (JSON encoding of the OpenTelemetry
//! `ExportLogsServiceRequest` proto).
//!
//! Spec:
//! <https://opentelemetry.io/docs/specs/otlp/#otlphttp>
//! <https://opentelemetry.io/docs/specs/otel/protocol/exporter/>
//!
//! We hand-roll the JSON shape rather than pulling
//! `opentelemetry-proto` because:
//!
//! 1. The shape is small and stable (LogRecord + AnyValue + Resource
//!    + Scope is ~6 nested types).
//! 2. The proto crate brings prost + 250 KB of generated code we
//!    don't otherwise need.
//! 3. JSON is the OTLP/HTTP path. The protobuf path is an
//!    optimization for high-volume servers that doesn't apply at
//!    desktop-app scale.

use std::borrow::Cow;
use std::time::SystemTime;

use bastyde_core::telemetry::{OwnedEvent, OwnedProp, OwnedPropValue};
use serde::Serialize;

/// Top-level `ExportLogsServiceRequest`.
#[derive(Debug, Serialize)]
pub(crate) struct OtlpLogsRequest<'a> {
    #[serde(rename = "resourceLogs")]
    pub resource_logs: Vec<ResourceLogs<'a>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResourceLogs<'a> {
    pub resource: Resource<'a>,
    #[serde(rename = "scopeLogs")]
    pub scope_logs: Vec<ScopeLogs<'a>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Resource<'a> {
    pub attributes: Vec<KeyValue<'a>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScopeLogs<'a> {
    pub scope: Scope,
    #[serde(rename = "logRecords")]
    pub log_records: Vec<LogRecord<'a>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Scope {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct LogRecord<'a> {
    /// Nanoseconds since Unix epoch, as a string (OTLP/JSON convention
    /// for fields the proto declares as `fixed64` — JSON's number
    /// can't safely represent values > 2^53).
    #[serde(rename = "timeUnixNano")]
    pub time_unix_nano: String,
    #[serde(rename = "observedTimeUnixNano")]
    pub observed_time_unix_nano: String,
    /// Severity number per OpenTelemetry §"Severity number" — INFO=9
    /// is the right default for analytics events.
    #[serde(rename = "severityNumber")]
    pub severity_number: u32,
    /// Severity text. "INFO" for analytics events.
    #[serde(rename = "severityText")]
    pub severity_text: &'static str,
    pub body: AnyValue<'a>,
    pub attributes: Vec<KeyValue<'a>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct KeyValue<'a> {
    pub key: String,
    pub value: AnyValue<'a>,
}

#[derive(Debug, Serialize)]
pub(crate) enum AnyValue<'a> {
    #[serde(rename = "stringValue")]
    String(Cow<'a, str>),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(String), // OTLP/JSON: int64 as string
    #[serde(rename = "doubleValue")]
    Double(f64),
}

// -------------------- builder --------------------

pub(crate) struct WireBuilder {
    pub service_name: String,
    pub service_version: String,
}

impl WireBuilder {
    /// Build the full OTLP request body for one batch of events.
    /// `install_id` becomes `service.instance.id` per the spec —
    /// when present, it identifies the per-install pseudonymous
    /// session; absent in anonymous mode.
    pub(crate) fn build_body(&self, events: &[OwnedEvent]) -> serde_json::Value {
        let mut resource_attrs = vec![
            KeyValue {
                key: "service.name".into(),
                value: AnyValue::String(Cow::Borrowed(self.service_name.as_str())),
            },
            KeyValue {
                key: "service.version".into(),
                value: AnyValue::String(Cow::Borrowed(self.service_version.as_str())),
            },
            KeyValue {
                key: "telemetry.sdk.name".into(),
                value: AnyValue::String(Cow::Borrowed("bastyde-analytics-otlp")),
            },
            KeyValue {
                key: "telemetry.sdk.version".into(),
                value: AnyValue::String(Cow::Borrowed(env!("CARGO_PKG_VERSION"))),
            },
        ];

        // `service.instance.id` from the first event's install_id,
        // when any event in the batch has one. The OTLP convention
        // is one resource block per service-instance, so a
        // pseudonymous batch picks the install_id; an anonymous
        // batch leaves the field unset.
        if let Some(install_id) = events.iter().find_map(|e| e.install_id.as_deref()) {
            resource_attrs.push(KeyValue {
                key: "service.instance.id".into(),
                value: AnyValue::String(Cow::Borrowed(install_id)),
            });
        }

        let log_records: Vec<LogRecord> = events.iter().map(record_for).collect();

        let req = OtlpLogsRequest {
            resource_logs: vec![ResourceLogs {
                resource: Resource {
                    attributes: resource_attrs,
                },
                scope_logs: vec![ScopeLogs {
                    scope: Scope {
                        name: "bastyde",
                        version: env!("CARGO_PKG_VERSION"),
                    },
                    log_records,
                }],
            }],
        };

        serde_json::to_value(&req).unwrap_or_else(|_| serde_json::json!({}))
    }
}

fn record_for(ev: &OwnedEvent) -> LogRecord<'_> {
    let nanos = ev
        .timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let time_str = nanos.to_string();

    // Attributes: every prop becomes `bastyde.<key>`, plus session_id
    // and the event category.
    let mut attributes = vec![
        KeyValue {
            key: "bastyde.category".into(),
            value: AnyValue::String(Cow::Borrowed(category_str(ev.category))),
        },
        KeyValue {
            key: "bastyde.session_id".into(),
            value: AnyValue::String(Cow::Borrowed(ev.session_id.as_str())),
        },
        KeyValue {
            key: "bastyde.schema_version".into(),
            value: AnyValue::Int(ev.schema_version.to_string()),
        },
    ];
    for prop in &ev.props {
        attributes.push(prop_to_kv(prop));
    }

    LogRecord {
        time_unix_nano: time_str.clone(),
        observed_time_unix_nano: time_str,
        severity_number: 9, // INFO
        severity_text: "INFO",
        body: AnyValue::String(Cow::Borrowed(ev.name.as_str())),
        attributes,
    }
}

fn category_str(c: bastyde_core::telemetry::EventCategory) -> &'static str {
    use bastyde_core::telemetry::EventCategory;
    match c {
        EventCategory::Intent => "intent",
        EventCategory::Lifecycle => "lifecycle",
        EventCategory::Navigation => "navigation",
        EventCategory::Census => "census",
        EventCategory::Custom => "custom",
    }
}

fn prop_to_kv(p: &OwnedProp) -> KeyValue<'_> {
    let key = format!("bastyde.{}", p.key);
    let value = match &p.value {
        OwnedPropValue::Str(s) => AnyValue::String(Cow::Borrowed(s.as_str())),
        OwnedPropValue::U32(n) => AnyValue::Int((*n as i64).to_string()),
        OwnedPropValue::I64(n) => AnyValue::Int(n.to_string()),
        OwnedPropValue::Bool(b) => AnyValue::Bool(*b),
        OwnedPropValue::F64Bucket(b) => {
            // Emit the midpoint as a double; lossy, but more useful
            // to OTel-side aggregators than a struct.
            let midpoint = (b.min_x100 as f64 + b.max_x100 as f64) / 200.0;
            AnyValue::Double(midpoint)
        }
        OwnedPropValue::HistogramStrU32(entries) => {
            // Histograms collapse to a JSON-encoded string; OTel
            // collectors can drop them straight into structured-log
            // sinks, and dashboards can re-parse client-side.
            let json = serde_json::to_string(entries).unwrap_or_else(|_| "[]".into());
            AnyValue::String(Cow::Owned(json))
        }
    };
    KeyValue { key, value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::telemetry::EventCategory;

    fn ev(name: &str, install: Option<&str>) -> OwnedEvent {
        OwnedEvent {
            name: name.into(),
            category: EventCategory::Intent,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: install.map(str::to_string),
            session_id: "s".into(),
            schema_version: 1,
            props: vec![
                OwnedProp {
                    key: "name".into(),
                    value: OwnedPropValue::Str("app.save".into()),
                },
                OwnedProp {
                    key: "n".into(),
                    value: OwnedPropValue::U32(42),
                },
            ],
        }
    }

    #[test]
    fn anonymous_batch_omits_service_instance_id() {
        let b = WireBuilder {
            service_name: "test.app".into(),
            service_version: "1.0".into(),
        };
        let body = b.build_body(&[ev("intent.save", None)]);
        let attrs = body["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        assert!(
            !attrs.iter().any(|a| a["key"] == "service.instance.id"),
            "anonymous batch must NOT carry service.instance.id"
        );
    }

    #[test]
    fn pseudonymous_batch_carries_service_instance_id() {
        let b = WireBuilder {
            service_name: "test.app".into(),
            service_version: "1.0".into(),
        };
        let body = b.build_body(&[ev("intent.save", Some("uuid-42"))]);
        let attrs = body["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        let instance = attrs
            .iter()
            .find(|a| a["key"] == "service.instance.id")
            .expect("service.instance.id present");
        assert_eq!(instance["value"]["stringValue"], "uuid-42");
    }

    #[test]
    fn props_become_bastyde_namespaced_attributes() {
        let b = WireBuilder {
            service_name: "test.app".into(),
            service_version: "1.0".into(),
        };
        let body = b.build_body(&[ev("intent.save", None)]);
        let attrs = body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
            .as_array()
            .unwrap();
        let prop_name = attrs
            .iter()
            .find(|a| a["key"] == "bastyde.name")
            .expect("bastyde.name attribute");
        assert_eq!(prop_name["value"]["stringValue"], "app.save");
        let prop_n = attrs
            .iter()
            .find(|a| a["key"] == "bastyde.n")
            .expect("bastyde.n attribute");
        // OTLP/JSON encodes int64 as a string.
        assert_eq!(prop_n["value"]["intValue"], "42");
    }

    #[test]
    fn body_carries_event_name() {
        let b = WireBuilder {
            service_name: "test.app".into(),
            service_version: "1.0".into(),
        };
        let body = b.build_body(&[ev("intent.dispatched", None)]);
        let body_field = &body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["body"];
        assert_eq!(body_field["stringValue"], "intent.dispatched");
    }

    #[test]
    fn severity_is_info() {
        let b = WireBuilder {
            service_name: "test.app".into(),
            service_version: "1.0".into(),
        };
        let body = b.build_body(&[ev("e", None)]);
        let rec = &body["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(rec["severityNumber"], 9);
        assert_eq!(rec["severityText"], "INFO");
    }
}
