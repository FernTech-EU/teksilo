//! Pure trait + type surface for usage telemetry.
//!
//! `bastyde-core` only owns the abstract surface: the [`UsageReporter`] trait,
//! the [`Event`] / [`Prop`] / [`PropValue`] types, and the consent enums.
//! All persistence, codegen, the SQLite queue, and the consent widget live
//! in `bastyde-telemetry` (which depends on this module + `bastyde-settings`).
//!
//! Why the split: `bastyde-settings` already depends on `bastyde-core` for
//! `Signal<T>` etc. Putting `ConsentStore` here would create a cycle. The
//! trait/types stay foundational; everything that touches disk lives one
//! crate higher.
//!
//! The framework's only insertion point is the dispatch tap in
//! [`crate::widget_tree::WidgetTree::dispatch_intent`], which calls
//! `app_state::<dyn UsageReporter>()` if registered and emits an
//! `intent.dispatched` event with the intent's `name`. Apps that don't
//! install a reporter pay nothing.

pub mod event;
pub mod reporter;

pub use event::{
    Event, EventCategory, F64Bucket, IntentSource, OwnedEvent, OwnedProp, OwnedPropValue, Prop,
    PropValue, RemoteDataExport, RemoteEvent, RemoteValue,
};
pub use reporter::{ConsentScope, ConsentState, TelemetryContext, TelemetryError, UsageReporter};
