//! The [`Scene`] data model — items + scene-rect placement, queryable
//! by rectangle. Phase 3 routes queries through a pluggable
//! [`SpatialIndex`] (grid-hash by default — see [`GridHashIndex`]),
//! and mirrors all mutations into it so `items_in_rect` and the
//! viewport-cull path in [`SceneView`](crate::SceneView) are both
//! `O(visible)` instead of `O(N)`.

use std::collections::HashMap;

use crate::index::{GridHashIndex, SpatialIndex};
use crate::item::{ItemId, SceneItem};
use fern_canvas::Rect;
use fern_core::widget::Widget;

/// A single entry in a [`Scene`]. The two variants reflect the two
/// content tiers: heavyweight `Widget`s consumed into the arena at
/// build time, and lightweight `SceneItem`s that live in the scene
/// permanently and are painted directly from `SceneView::paint`.
pub(crate) struct SceneEntry {
    pub(crate) id: ItemId,
    pub(crate) scene_rect: Rect,
    pub(crate) kind: SceneEntryKind,
}

pub(crate) enum SceneEntryKind {
    /// A heavyweight `Widget` to materialise into the arena. `Some`
    /// until [`SceneView::build`](crate::view::SceneView) consumes
    /// it via `BuildContext::add_boxed`; `None` afterwards.
    Widget {
        pending: Option<Box<dyn Widget>>,
    },
    /// A lightweight `SceneItem`. Stays in the scene permanently —
    /// `SceneView::paint` walks visible items each frame and calls
    /// `item.paint` with the canvas.
    Item(Box<dyn SceneItem>),
}

/// The data model behind a `SceneView`: a collection of items at
/// scene coordinates plus a [`SpatialIndex`] for fast rectangular
/// queries. The Scene itself does no rendering or layout — it's a
/// passive container the view reads from at build / place / paint
/// time.
///
/// All mutators (`add_widget`, `move_item`, `remove`) update the
/// index in lockstep, so `items_in_rect` and SceneView's viewport-
/// cull path are both `O(visible)` instead of `O(N)`. Insertion
/// order is preserved by `entries` and exposed via [`Scene::ids`].
///
/// Full runtime mutation (mutate-after-`build`) is deferred to
/// Phase 6 — this Phase 3 surface assumes scenes are built up
/// before being handed to a `SceneView`.
pub struct Scene {
    pub(crate) entries: Vec<SceneEntry>,
    /// `ItemId` → index into `entries` for O(1) `scene_rect` lookup.
    /// Maintained in lockstep with `entries`; rebuilt on `remove` to
    /// keep indices accurate after the `retain`-driven shift.
    entry_index: HashMap<ItemId, usize>,
    index: Box<dyn SpatialIndex>,
}

impl Scene {
    /// An empty scene with the default [`GridHashIndex`].
    pub fn new() -> Self {
        Self::with_index(Box::new(GridHashIndex::default()))
    }

    /// An empty scene with a custom [`SpatialIndex`]. Use this to
    /// pre-tune `cell_size` for scenes with unusual item density, or
    /// to swap in an alternative index implementation (Phase 7 will
    /// ship an R-tree under the same trait).
    ///
    /// ```ignore
    /// let scene = Scene::with_index(Box::new(GridHashIndex::new(128.0)));
    /// ```
    pub fn with_index(index: Box<dyn SpatialIndex>) -> Self {
        Self {
            entries: Vec::new(),
            entry_index: HashMap::new(),
            index,
        }
    }

    /// Place a heavyweight `Widget` at a scene-coord rectangle.
    /// Returns the [`ItemId`] used to address the item later (move,
    /// remove, query). The widget is consumed at SceneView build time
    /// and added to the arena as a real interactive child — focus,
    /// keyboard, gestures, animations, accessibility all work
    /// unchanged.
    pub fn add_widget<W: Widget + 'static>(&mut self, widget: W, scene_rect: Rect) -> ItemId {
        let id = ItemId::next();
        let pos = self.entries.len();
        self.entries.push(SceneEntry {
            id,
            scene_rect,
            kind: SceneEntryKind::Widget {
                pending: Some(Box::new(widget)),
            },
        });
        self.entry_index.insert(id, pos);
        self.index.insert(id, scene_rect);
        id
    }

    /// Place a lightweight [`SceneItem`] in the scene. Returns the
    /// [`ItemId`] used to address it later (move, remove, query). The
    /// item is **not** added to the arena — it has no widget id, no
    /// focus, no per-item event handling. `SceneView::paint` walks
    /// visible items each frame and calls `item.paint` with the
    /// canvas. Use this tier for the "background furniture" of a
    /// scene — connector lines, tile patterns, decorations — where
    /// thousands of items need to render cheaply.
    ///
    /// The item's `bounds_in_scene()` is captured at insertion time
    /// for the spatial index. If the item changes its bounds later,
    /// call [`Scene::move_item`] with the new rect to re-bucket it.
    pub fn add_item<I: SceneItem + 'static>(&mut self, item: I) -> ItemId {
        let scene_rect = item.bounds_in_scene();
        let id = ItemId::next();
        let pos = self.entries.len();
        self.entries.push(SceneEntry {
            id,
            scene_rect,
            kind: SceneEntryKind::Item(Box::new(item)),
        });
        self.entry_index.insert(id, pos);
        self.index.insert(id, scene_rect);
        id
    }

    /// Borrow a lightweight [`SceneItem`] by id. Returns `None` if the
    /// id is unknown or refers to a heavyweight widget entry.
    /// `SceneView::paint` uses this to walk the visible-item set.
    pub fn item(&self, id: ItemId) -> Option<&dyn SceneItem> {
        let pos = *self.entry_index.get(&id)?;
        match &self.entries.get(pos)?.kind {
            SceneEntryKind::Item(item) => Some(item.as_ref()),
            SceneEntryKind::Widget { .. } => None,
        }
    }

    /// Update an item's scene rectangle. No-op if the id isn't in the
    /// scene. The spatial index is re-bucketed in lockstep so future
    /// `items_in_rect` queries see the new bounds.
    pub fn move_item(&mut self, id: ItemId, new_bounds: Rect) {
        if let Some(&pos) = self.entry_index.get(&id) {
            self.entries[pos].scene_rect = new_bounds;
            self.index.insert(id, new_bounds);
        }
    }

    /// Remove an item by id. No-op if the id isn't in the scene.
    pub fn remove(&mut self, id: ItemId) {
        let prev_len = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() != prev_len {
            // Rebuild the index since `retain` shifted positions.
            self.entry_index.clear();
            for (pos, entry) in self.entries.iter().enumerate() {
                self.entry_index.insert(entry.id, pos);
            }
        }
        self.index.remove(id);
    }

    /// All items whose `scene_rect` intersects the query rectangle.
    /// Backed by the spatial index — `O(visible)` after a Phase 3
    /// rebuild of the bucketing on insert/move/remove.
    ///
    /// The result is narrowed to exact-AABB intersections (the index
    /// may return cell-fan-out false-positives that don't actually
    /// intersect; Scene filters those out so callers get a clean
    /// hit list).
    pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId> {
        let candidates = self.index.query(scene_rect);
        // Narrow phase — the spatial-index doc explicitly allows cell
        // fan-out false-positives. We resolve to exact AABB here so
        // the public API gives the strict mathematical answer.
        candidates
            .into_iter()
            .filter(|id| {
                self.entries
                    .iter()
                    .find(|e| e.id == *id)
                    .map(|e| rects_intersect(e.scene_rect, scene_rect))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Read an item's current scene rectangle. Returns `None` if the
    /// id isn't in the scene. O(1) via the entry index.
    pub fn scene_rect(&self, id: ItemId) -> Option<Rect> {
        self.entry_index
            .get(&id)
            .and_then(|&pos| self.entries.get(pos))
            .map(|e| e.scene_rect)
    }

    /// Number of items currently in the scene.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the scene contains any items.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All ids currently in the scene, in insertion order.
    pub fn ids(&self) -> Vec<ItemId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// Borrow the spatial index. Useful for diagnostics and tests
    /// that want to verify an item was bucketed.
    pub fn index(&self) -> &dyn SpatialIndex {
        &*self.index
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scene")
            .field("len", &self.entries.len())
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

/// Standard half-open AABB intersection: two rects intersect iff their
/// projections overlap on both axes. Used by `Scene::items_in_rect`'s
/// narrow phase and by `SceneView`'s viewport cull.
pub(crate) fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::{Size, SizeProposal};
    use fern_core::widget::{LayoutContext, LayoutResponse, Widget};

    /// Minimal leaf widget used purely to populate `Scene` entries in
    /// these unit tests. A no-op `layout_response` keeps the test
    /// surface tiny; integration tests against real widget
    /// interactions live in `crates/fern-scene/tests/integration.rs`.
    #[derive(Debug)]
    struct FillWidget;

    impl FillWidget {
        fn new() -> Self {
            Self
        }
    }

    impl Widget for FillWidget {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            Size::new(0.0, 0.0).into()
        }
    }

    #[test]
    fn add_widget_round_trip() {
        let mut scene = Scene::new();
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let id = scene.add_widget(FillWidget::new(), r);
        assert_eq!(scene.len(), 1);
        assert_eq!(scene.scene_rect(id), Some(r));
        assert_eq!(scene.ids(), vec![id]);
    }

    #[test]
    fn add_widget_assigns_unique_ids() {
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::ZERO);
        let b = scene.add_widget(FillWidget::new(), Rect::ZERO);
        assert_ne!(a, b);
    }

    #[test]
    fn move_item_updates_scene_rect() {
        let mut scene = Scene::new();
        let id = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 50.0, 50.0));
        let new = Rect::new(100.0, 100.0, 30.0, 30.0);
        scene.move_item(id, new);
        assert_eq!(scene.scene_rect(id), Some(new));
    }

    #[test]
    fn move_item_unknown_id_is_noop() {
        let mut scene = Scene::new();
        let id = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 50.0, 50.0));
        // synthesise an id that's definitely not in the scene
        let bogus = ItemId::next();
        scene.move_item(bogus, Rect::new(99.0, 99.0, 1.0, 1.0));
        assert_eq!(
            scene.scene_rect(id),
            Some(Rect::new(0.0, 0.0, 50.0, 50.0)),
            "moving an unknown id must not affect existing items"
        );
    }

    #[test]
    fn remove_drops_the_entry() {
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::ZERO);
        let b = scene.add_widget(FillWidget::new(), Rect::ZERO);
        scene.remove(a);
        assert_eq!(scene.len(), 1);
        assert_eq!(scene.scene_rect(a), None);
        assert!(scene.scene_rect(b).is_some());
    }

    #[test]
    fn items_in_rect_brute_force() {
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let b = scene.add_widget(FillWidget::new(), Rect::new(100.0, 100.0, 10.0, 10.0));
        let c = scene.add_widget(FillWidget::new(), Rect::new(5.0, 5.0, 10.0, 10.0));

        let near_origin = scene.items_in_rect(Rect::new(0.0, 0.0, 20.0, 20.0));
        assert!(near_origin.contains(&a));
        assert!(near_origin.contains(&c));
        assert!(!near_origin.contains(&b));

        let far = scene.items_in_rect(Rect::new(95.0, 95.0, 20.0, 20.0));
        assert_eq!(far, vec![b]);

        let empty = scene.items_in_rect(Rect::new(500.0, 500.0, 1.0, 1.0));
        assert!(empty.is_empty());
    }

    #[test]
    fn add_item_round_trip_and_index_bucketed() {
        use crate::items::RectItem;
        use fern_tokens::Color;

        let mut scene = Scene::new();
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        let id = scene.add_item(RectItem::new(r).fill(Color::RED));
        assert_eq!(scene.scene_rect(id), Some(r));
        // Lightweight item participates in spatial-index queries the
        // same way heavyweight widgets do.
        let hits = scene.items_in_rect(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(hits.contains(&id));
    }

    #[test]
    fn item_accessor_returns_lightweight_only() {
        use crate::items::RectItem;

        let mut scene = Scene::new();
        let widget_id = scene.add_widget(FillWidget::new(), Rect::ZERO);
        let item_id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)));
        // Lightweight id resolves; heavyweight id returns None — the
        // `item()` accessor is the lightweight-tier-only door.
        assert!(scene.item(item_id).is_some());
        assert!(scene.item(widget_id).is_none());
    }

    #[test]
    fn move_item_updates_lightweight_bucket() {
        use crate::items::RectItem;

        let mut scene = Scene::new();
        let id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)));
        scene.move_item(id, Rect::new(500.0, 500.0, 10.0, 10.0));
        let near_origin = scene.items_in_rect(Rect::new(0.0, 0.0, 50.0, 50.0));
        assert!(!near_origin.contains(&id));
        let near_far = scene.items_in_rect(Rect::new(490.0, 490.0, 30.0, 30.0));
        assert!(near_far.contains(&id));
    }

    #[test]
    fn rects_intersect_edge_touching_excluded() {
        // Two AABBs that share only an edge are NOT considered
        // intersecting (half-open convention). Pins this so future
        // refactors of the predicate don't silently flip semantics
        // and break marquee selection / spatial-index queries.
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);
        assert!(!rects_intersect(a, b));
        assert!(!rects_intersect(b, a));
    }
}
