//! Tonic-generated client + server stubs for the fern-collector wire
//! format. Re-exports the generated `fern.telemetry.v1` module under
//! [`v1`] for callers.
//!
//! See [`proto/telemetry/v1.proto`](../proto/telemetry/v1.proto) inside
//! this crate for the schema. Versioning rules: tag numbers never
//! reused, types never changed in place, additive evolution only. The
//! crate version mirrors the proto major version.

#![allow(clippy::all)]
#![allow(missing_docs)]

pub mod v1 {
    tonic::include_proto!("fern.telemetry.v1");
}
