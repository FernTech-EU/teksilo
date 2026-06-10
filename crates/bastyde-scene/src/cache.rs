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
    /// `id` → cached RenderFrame in **local item coordinates**, plus
    /// the text raster scale the frame was recorded at. Looked up by
    /// the paint walk; dropped on geometry change. The scale rides
    /// along because glyph quads in the frame sample bitmaps of that
    /// density: when the item's effective raster scale moves (the
    /// view's zoom crossed a raster bucket, or the item's own
    /// transform scale changed), [`get`](Self::get) misses and the
    /// item re-records against fresh bitmaps. The arena-level
    /// `paint_raster_scale` stamp can't reach frames cached here, so
    /// the scale must be part of this cache's own validity.
    entries: HashMap<ItemId, (RenderFrame, f32)>,
    /// [`TextBackend::glyph_epoch`](bastyde_canvas::TextBackend::glyph_epoch)
    /// as of the last paint pass that consulted this cache. Cached
    /// frames bake glyph atlas UVs; when the backend evicts or resets
    /// glyphs it bumps the epoch, and every entry here must be dropped
    /// before being replayed — the baked UVs may now point at pixels
    /// owned by unrelated glyphs. This cache lives outside the widget
    /// arena, so the framework-level `invalidate_all_paints` recovery
    /// cannot reach it; the epoch gate in `paint_band` is what keeps
    /// it honest.
    glyph_epoch: u64,
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

    /// Borrow the cached frame for `id`, if any — provided it was
    /// recorded at `raster_scale`. A scale mismatch reads as a miss:
    /// the caller re-records and [`insert`](Self::insert) replaces the
    /// stale entry.
    pub fn get(&self, id: ItemId, raster_scale: f32) -> Option<&RenderFrame> {
        self.entries
            .get(&id)
            .filter(|(_, baked)| *baked == raster_scale)
            .map(|(frame, _)| frame)
    }

    /// Insert (or replace) a cached frame for `id`, recorded at
    /// `raster_scale`.
    pub fn insert(&mut self, id: ItemId, frame: RenderFrame, raster_scale: f32) {
        self.entries.insert(id, (frame, raster_scale));
    }

    /// Evict `id`'s entry. Called on `ItemChange::LocalBoundsChanged`
    /// or any other invalidation.
    pub fn evict(&mut self, id: ItemId) {
        self.entries.remove(&id);
    }

    /// Drop every entry. Called when the glyph epoch moves (see
    /// [`sync_glyph_epoch`](Self::sync_glyph_epoch)).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Compare the text backend's current glyph epoch against the one
    /// recorded on the last paint pass; on a change, drop every cached
    /// frame (their baked atlas UVs may reference recycled slots) and
    /// record the new epoch. Returns `true` when the cache was cleared.
    pub fn sync_glyph_epoch(&mut self, current_epoch: u64) -> bool {
        if self.glyph_epoch == current_epoch {
            return false;
        }
        self.glyph_epoch = current_epoch;
        let had_entries = !self.entries.is_empty();
        self.clear();
        had_entries
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
        c.insert(id, RenderFrame::default(), 1.0);
        assert!(c.contains(id));
        assert_eq!(c.len(), 1);
        c.evict(id);
        assert!(!c.contains(id));
        assert!(c.is_empty());
    }

    #[test]
    fn sync_glyph_epoch_clears_on_change_only() {
        let mut c = ItemCoordinateCache::new();
        let id = fresh_id();
        c.insert(id, RenderFrame::default(), 1.0);

        // Same epoch as the initial one (0): entries survive.
        assert!(!c.sync_glyph_epoch(0));
        assert!(c.contains(id));

        // Epoch moved (glyph eviction / scale reset): everything drops —
        // the cached frames' baked atlas UVs may reference recycled slots.
        assert!(c.sync_glyph_epoch(1));
        assert!(!c.contains(id));
        assert!(c.is_empty());

        // Same epoch again: a refilled cache survives.
        c.insert(id, RenderFrame::default(), 1.0);
        assert!(!c.sync_glyph_epoch(1));
        assert!(c.contains(id));
    }

    #[test]
    fn get_misses_on_raster_scale_mismatch() {
        let mut c = ItemCoordinateCache::new();
        let id = fresh_id();
        c.insert(id, RenderFrame::default(), 1.0);

        // Hit at the recorded scale.
        assert!(c.get(id, 1.0).is_some());
        // The zoom crossed a raster bucket: the entry's glyph quads
        // sample bitmaps of the old density — read as a miss so the
        // item re-records.
        assert!(c.get(id, 1.953_125).is_none());
        // Replacing re-records at the new scale.
        c.insert(id, RenderFrame::default(), 1.953_125);
        assert!(c.get(id, 1.953_125).is_some());
        assert!(c.get(id, 1.0).is_none());
        assert_eq!(c.len(), 1);
    }
}
