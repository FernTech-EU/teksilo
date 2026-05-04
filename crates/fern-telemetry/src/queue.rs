//! Event queue — `EventQueue` trait + two impls.
//!
//! Adapters that buffer events for later transmission depend on the
//! [`EventQueue`] trait so the choice of in-memory vs. persistent
//! backing is a deployment detail. Both impls have the same FIFO
//! semantics (oldest event drained first); only [`PersistentEventQueue`]
//! survives process restart.
//!
//! - [`InMemoryEventQueue`] — `Mutex<VecDeque<OwnedEvent>>`. Default
//!   for tests and for adapters that consider events ephemeral.
//! - [`PersistentEventQueue`] — backed by [redb] at a path under
//!   [`AppPaths::data_dir()`](fern_settings::AppPaths::data_dir).
//!   Pure-Rust, no C deps, ~250–400 KB binary footprint vs. SQLite's
//!   ~1 MB. Atomic writes, MVCC reads, single-writer.
//!
//! [redb]: https://crates.io/crates/redb

mod mem;
mod persistent;

pub use mem::InMemoryEventQueue;
pub use persistent::{PersistentEventQueue, PersistentQueueError};

use fern_core::telemetry::OwnedEvent;

/// FIFO event buffer with capped size and oldest-eviction.
///
/// `Send + Sync` is required because adapters typically own a worker
/// thread that drains the queue while the UI thread pushes; the
/// trait's contract is "thread-safe enough for a producer/consumer
/// pair."
pub trait EventQueue: Send + Sync + 'static {
    /// Append an event to the tail. If the queue is at capacity,
    /// the oldest entry is dropped (FIFO eviction).
    fn push(&self, event: OwnedEvent);

    /// Take up to `n` events from the head, removing them. Used by
    /// adapter workers to assemble batches for HTTP transmission.
    fn drain_batch(&self, n: usize) -> Vec<OwnedEvent>;

    /// Number of events currently buffered. Includes events queued
    /// for retry.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop everything without sending. Called on consent revocation
    /// and on `UsageReporter::erase_remote_data`. Implementations
    /// MUST guarantee no buffered event escapes after this returns.
    fn discard_all(&self);

    /// Snapshot the head of the queue (clone). Used by the
    /// `PrivacySettings` "Inspect data sent" view. Best-effort — the
    /// returned events may have been drained by the time the caller
    /// reads them.
    fn peek_recent(&self, n: usize) -> Vec<OwnedEvent>;
}
