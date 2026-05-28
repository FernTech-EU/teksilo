//! Item-coordinate paint caching.
//!
//! When a [`SceneItem`](crate::SceneItem) returns
//! [`CacheMode::ItemCoordinate`] from `SceneItem::cache_mode`,
//! the [`SceneView`](crate::SceneView) caches the item's paint
//! output as a [`RenderFrame`] in **local item coordinates**. On
//! subsequent paint passes the cached frame is replayed via
//! `Canvas::draw_render_frame` instead of re-running
//! `item.paint`. Cache validity is keyed by
//! [`Scene::item_change_signal`](crate::Scene::item_change_signal):
//! a `LocalBoundsChanged` event for an id evicts that id's entry.
//!
//! Items whose visual depends on signal state outside of their
//! `local_bounds` (e.g. `TextItem` with `with_signal_text`) should
//! NOT use `ItemCoordinate` — the cache won't see signal-driven
//! repaint dirties. The default for every `SceneItem` is
//! [`CacheMode::None`].

use std::collections::HashMap;

use bastyde_canvas::RenderFrame;

use crate::item::ItemId;

/// Per-item paint caching strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    /// Re-run `item.paint` every frame. Default for every item.
    #[default]
    None,
    /// Cache the paint output as a [`RenderFrame`] keyed by the
    /// item's `local_bounds`. Cheap when the item's geometry is
    /// stable and its content doesn't depend on external signal
    /// state. The cache is dropped on `LocalBoundsChanged` for the
    /// id.
    ItemCoordinate,
}

/// SceneView's per-item paint cache. Owned by the SceneView, shared
/// via `Rc<RefCell<>>` so the paint walk and the item-change
/// observer can both touch it.
#[derive(Debug, Default)]
pub struct ItemCoordinateCache {
    /// `id` → cached RenderFrame in **local item coordinates**.
    /// Looked up by the paint walk; dropped on geometry change.
    entries: HashMap<ItemId, RenderFrame>,
}

impl ItemCoordinateCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `id`'s entry is still valid. Test-only diagnostic;
    /// apps observe cache effectiveness via the parent
    /// [`SceneView::item_cache_len`](crate::SceneView::item_cache_len)
    /// or by counting paint calls in their own SceneItem impl.
    #[cfg(test)]
    pub(crate) fn contains(&self, id: ItemId) -> bool {
        self.entries.contains_key(&id)
    }

    /// Borrow the cached frame for `id`, if any.
    pub fn get(&self, id: ItemId) -> Option<&RenderFrame> {
        self.entries.get(&id)
    }

    /// Insert (or replace) a cached frame for `id`.
    pub fn insert(&mut self, id: ItemId, frame: RenderFrame) {
        self.entries.insert(id, frame);
    }

    /// Evict `id`'s entry. Called on `ItemChange::LocalBoundsChanged`
    /// or any other invalidation.
    pub fn evict(&mut self, id: ItemId) {
        self.entries.remove(&id);
    }

    /// Number of cached entries (diagnostics / tests).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty. Test-only diagnostic.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_id() -> ItemId {
        crate::item::ItemId::next()
    }

    #[test]
    fn cache_default_mode_is_none() {
        assert_eq!(CacheMode::default(), CacheMode::None);
    }

    #[test]
    fn cache_round_trip() {
        let mut c = ItemCoordinateCache::new();
        let id = fresh_id();
        assert!(!c.contains(id));
        c.insert(id, RenderFrame::default());
        assert!(c.contains(id));
        assert_eq!(c.len(), 1);
        c.evict(id);
        assert!(!c.contains(id));
        assert!(c.is_empty());
    }
}
