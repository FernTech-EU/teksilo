//! Event types passed to [`UsageReporter::record`](super::UsageReporter::record).
//!
//! Events are zero-copy borrowed structures so they can be assembled on
//! the stack inside the dispatch path without allocation. Adapters that
//! need to defer transmission convert to [`OwnedEvent`] before queueing.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A telemetry event in flight.
///
/// Constructed by the codegen'd `emit_*` helpers in `bastyde-telemetry`
/// (or hand-written for framework events) and handed to a reporter
/// synchronously. The borrowed `'a` lifetime keeps the call site
/// allocation-free — adapters that buffer events MUST convert to
/// [`OwnedEvent`] before queueing.
#[derive(Debug)]
pub struct Event<'a> {
    /// Stable, dev-authored event name (`"intent.dispatched"`,
    /// `"lifecycle.app_started"`). Always a `&'static str` literal.
    pub name: &'static str,
    pub category: EventCategory,
    pub timestamp: SystemTime,
    /// `Some(uuid)` in pseudonymous mode, `None` in anonymous mode.
    pub install_id: Option<&'a str>,
    /// Per-process random session id. Not persisted across restarts.
    pub session_id: &'a str,
    /// Event-schema version at emission time. Used for server-side
    /// schema validation and the consent re-prompt rule.
    pub schema_version: u32,
    pub props: &'a [Prop<'a>],
}

impl<'a> Event<'a> {
    pub fn to_owned(&self) -> OwnedEvent {
        OwnedEvent {
            name: self.name.to_owned(),
            category: self.category,
            timestamp: self.timestamp,
            install_id: self.install_id.map(str::to_owned),
            session_id: self.session_id.to_owned(),
            schema_version: self.schema_version,
            props: self.props.iter().map(Prop::to_owned).collect(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventCategory {
    Intent,
    Lifecycle,
    Navigation,
    Census,
    Custom,
}

impl EventCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Lifecycle => "lifecycle",
            Self::Navigation => "navigation",
            Self::Census => "census",
            Self::Custom => "custom",
        }
    }
}

/// One key/value pair on an event. Keys are always `&'static str`.
#[derive(Debug, Clone)]
pub struct Prop<'a> {
    pub key: &'static str,
    pub value: PropValue<'a>,
}

impl<'a> Prop<'a> {
    pub fn to_owned(&self) -> OwnedProp {
        OwnedProp {
            key: self.key.to_owned(),
            value: self.value.to_owned(),
        }
    }
}

/// Closed enum of allowlisted property values. There is **no** `String`
/// variant — anything dynamic must be length-bounded by the schema or
/// pre-bucketed. This is the type-system enforcement of the data-
/// minimisation rule: an app author physically cannot pass a runtime
/// `String` from a `TextField` because the codegen'd emit signature
/// won't accept one.
#[derive(Debug, Clone)]
pub enum PropValue<'a> {
    /// `&'static str` — for `dev_static` schema properties (intent
    /// names, source enums, etc.).
    StaticStr(&'static str),
    /// `&'a str` — for properties the schema marks as bounded-length
    /// (locale code, app version). The caller is responsible for the
    /// length bound; codegen enforces it at the emit-fn signature.
    BoundedStr(&'a str),
    U32(u32),
    I64(i64),
    /// Pre-bucketed float. Raw `f64` is intentionally absent — high-
    /// entropy floats are a fingerprinting risk and must be bucketed
    /// at the call site.
    F64Bucket(F64Bucket),
    Bool(bool),
    /// Type-erased enum variant — the variant's `&'static str` name.
    Enum {
        variant: &'static str,
    },
    /// Histogram of `(static_key, count)`. Used by `widget.census`.
    HistogramStrU32(&'a [(&'static str, u32)]),
}

impl<'a> PropValue<'a> {
    pub fn to_owned(&self) -> OwnedPropValue {
        match self {
            Self::StaticStr(s) => OwnedPropValue::Str((*s).to_owned()),
            Self::BoundedStr(s) => OwnedPropValue::Str((*s).to_owned()),
            Self::U32(v) => OwnedPropValue::U32(*v),
            Self::I64(v) => OwnedPropValue::I64(*v),
            Self::F64Bucket(b) => OwnedPropValue::F64Bucket(*b),
            Self::Bool(v) => OwnedPropValue::Bool(*v),
            Self::Enum { variant } => OwnedPropValue::Str((*variant).to_owned()),
            Self::HistogramStrU32(entries) => OwnedPropValue::HistogramStrU32(
                entries.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
            ),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct F64Bucket {
    /// Inclusive lower bound of the bucket, encoded as a
    /// platform-independent fixed-point. The pair `(min_x100, max_x100)`
    /// = `(120, 250)` represents `[1.20, 2.50)`.
    pub min_x100: i64,
    pub max_x100: i64,
}

// --- Owned variants for queueing / cross-thread handoff --------------
//
// `name` and `key` are `String` (not `&'static str`) so the type is
// serde-friendly and round-trips through redb / JSON. The `&'static`
// guarantee is load-bearing only on `Event<'_>` (the in-flight type
// where codegen ensures literal-only usage); once an event is owned
// for queueing, the static-ness has already done its job.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedEvent {
    pub name: String,
    pub category: EventCategory,
    pub timestamp: SystemTime,
    pub install_id: Option<String>,
    pub session_id: String,
    pub schema_version: u32,
    pub props: Vec<OwnedProp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedProp {
    pub key: String,
    pub value: OwnedPropValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OwnedPropValue {
    Str(String),
    U32(u32),
    I64(i64),
    F64Bucket(F64Bucket),
    Bool(bool),
    HistogramStrU32(Vec<(String, u32)>),
}

// --- IntentSource ---------------------------------------------------

/// Where an intent came from. Reserved for future use; all dispatch
/// sites currently emit `IntentSource::Unknown` because the dispatch
/// path doesn't yet propagate origin information. Plumbing through the
/// real source is a Phase 5 deliverable.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IntentSource {
    Shortcut,
    Menu,
    Handler,
    Programmatic,
    Accessibility,
    Unknown,
}

impl IntentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shortcut => "shortcut",
            Self::Menu => "menu",
            Self::Handler => "handler",
            Self::Programmatic => "programmatic",
            Self::Accessibility => "accessibility",
            Self::Unknown => "unknown",
        }
    }
}

// --- RemoteDataExport (Art. 15 + 20) -------------------------------

/// Server-side data fetched by [`UsageReporter::fetch_remote_data`].
///
/// Self-describing: the `schema_version`, `endpoint`, and `adapter`
/// fields make the exported document a complete RGPD Art. 20
/// portability artifact when serialized to JSON. The widget's
/// "Save as JSON…" button writes this struct verbatim.
///
/// `Serialize` is derived; `Deserialize` is not (the `adapter` field
/// is `&'static str`). The export is a write-only artifact.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteDataExport {
    pub install_id: String,
    pub fetched_at: SystemTime,
    pub adapter: &'static str,
    pub endpoint: String,
    pub schema_version: u32,
    pub events: Vec<RemoteEvent>,
    pub server_metadata: BTreeMap<String, RemoteValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteEvent {
    pub name: String,
    pub timestamp: SystemTime,
    pub properties: BTreeMap<String, RemoteValue>,
}

/// JSON-shaped value type for fetched server records. Kept minimal
/// (no serde dep at the `bastyde-core` level); adapter crates may map
/// to their own richer types as needed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RemoteValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_to_owned_round_trip() {
        let props = [
            Prop {
                key: "name",
                value: PropValue::StaticStr("app.save"),
            },
            Prop {
                key: "count",
                value: PropValue::U32(3),
            },
        ];
        let event = Event {
            name: "intent.dispatched",
            category: EventCategory::Intent,
            timestamp: SystemTime::UNIX_EPOCH,
            install_id: None,
            session_id: "abc",
            schema_version: 1,
            props: &props,
        };
        let owned = event.to_owned();
        assert_eq!(owned.name, "intent.dispatched");
        assert_eq!(owned.props.len(), 2);
        assert!(matches!(owned.props[0].value, OwnedPropValue::Str(ref s) if s == "app.save"));
        assert!(matches!(owned.props[1].value, OwnedPropValue::U32(3)));
    }

    #[test]
    fn intent_source_str() {
        assert_eq!(IntentSource::Shortcut.as_str(), "shortcut");
        assert_eq!(IntentSource::Unknown.as_str(), "unknown");
    }
}
