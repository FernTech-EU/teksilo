//! Scene item identity and (in later phases) the lightweight
//! `SceneItem` trait. Phase 1 only exposes [`ItemId`]; the trait is
//! Phase 4.

use std::sync::atomic::{AtomicU64, Ordering};

/// Opaque identifier for an item in a [`Scene`](crate::Scene).
/// Generated monotonically by `Scene::add_widget` / `add_item`.
/// Stable across the lifetime of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub(crate) u64);

impl ItemId {
    /// Mint a fresh, globally unique id. Used internally by `Scene`;
    /// apps obtain ids from `add_*` methods and shouldn't construct
    /// them directly.
    pub(crate) fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        ItemId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Raw numeric value of this id. Stable for the process lifetime.
    /// Used by the AT walker to derive a synthetic NodeId via
    /// `synthetic_node_id(scene_view_id, id.as_u64(), …)` in Phase 5.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_monotonic() {
        let a = ItemId::next();
        let b = ItemId::next();
        let c = ItemId::next();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert!(a.0 < b.0 && b.0 < c.0);
    }

    #[test]
    fn ids_round_trip_through_as_u64() {
        let id = ItemId::next();
        assert_eq!(id.as_u64(), id.0);
    }
}
