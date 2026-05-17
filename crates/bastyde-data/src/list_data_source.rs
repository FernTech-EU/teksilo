//! Trait for large or external datasets that cannot be held in memory.
//!
//! `ListDataSource` is an escape hatch for cases where the data is too large
//! for `ListModel<T>` (paged database cursor, filesystem directory listing,
//! memory-mapped log file). The implementor owns the data and is responsible
//! for emitting correct `DataChange` notifications when it changes.
//!
//! `ListDataSource` is not related to `ListModel<T>` by inheritance — they
//! are two separate input paths consumed by `ListView`.

use bastyde_core::ObserverHandle;

use crate::data_change::DataChange;

/// A data source for large or external flat collections.
///
/// The associated `Item` type allows type-safe access. The trait uses
/// `impl FnOnce` for `with_item` rather than returning a reference,
/// allowing implementors to hold internal locks or temporary buffers
/// for the duration of the callback.
///
/// This trait is not object-safe (due to the associated type and generic
/// methods). `ListView` consumes it generically via `ListView::from_source`.
pub trait ListDataSource: 'static {
    /// The item type exposed by this data source.
    type Item: 'static;

    /// Number of items currently in the data source.
    fn len(&self) -> usize;

    /// Whether the data source is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Access an item by index via a callback. Returns `None` if out of bounds.
    fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R>;

    /// Register an observer for data change notifications.
    /// The implementor is responsible for emitting correct `DataChange`
    /// notifications when the underlying data changes.
    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle;
}
