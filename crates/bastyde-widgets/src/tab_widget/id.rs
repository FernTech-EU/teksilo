//! Stable per-tab identifiers.
//!
//! [`TabId`] is the identity of a tab as it lives, dies, reorders,
//! and survives data-source mutations. Selection signals, close /
//! reorder / pin callbacks, and accessibility relations are all
//! keyed by `TabId` rather than by index — so reordering a tab via
//! drag-drop never sends the active selection to a different tab.
//!
//! Apps either let the framework allocate fresh ids
//! ([`TabId::fresh`]) or wrap their own external keys (file path
//! hashes, document UUIDs, …) via [`TabId::from_raw`].

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

/// Stable identity of a tab. Cheap to copy; persists across model
/// reorders, rebuilds, and reorders triggered by drag-and-drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(NonZeroU64);

impl TabId {
    /// Allocate a new, never-before-seen id. Backed by a monotonic
    /// global counter — overflow is theoretically possible after
    /// 2^64 calls, at which point the universe has had bigger
    /// problems.
    pub fn fresh() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let raw = COUNTER.fetch_add(1, Ordering::Relaxed);
        // The counter starts at 1 and only ever increments, so the
        // value is non-zero in any practical run.
        Self(NonZeroU64::new(raw).expect("TabId counter wrapped to zero"))
    }

    /// Wrap an externally-allocated key. Use this when the tab's
    /// identity comes from an existing app-side store (document
    /// UUID, file path hash, etc.) — calling [`TabId::fresh`] would
    /// allocate a *new* id every restart, breaking session restore.
    pub fn from_raw(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The underlying non-zero `u64`. Useful when persisting tabs
    /// across sessions: serialize this, restore via `from_raw`.
    pub fn raw(self) -> NonZeroU64 {
        self.0
    }
}

impl std::fmt::Display for TabId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TabId({})", self.0.get())
    }
}
