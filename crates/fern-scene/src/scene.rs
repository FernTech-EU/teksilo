//! The [`Scene`] data model — items + scene-rect placement, queryable
//! by rectangle. Phase 1 stores items in a flat `Vec` and resolves
//! `items_in_rect` via a brute-force scan; Phase 3 adds a pluggable
//! `SpatialIndex` (grid-hash MVP, R-tree later) without API churn.

use crate::item::ItemId;
use fern_canvas::Rect;
use fern_core::widget::Widget;

/// A single entry in a [`Scene`]. Phase 1 carries only the heavyweight
/// (widget) variant. Phase 4 adds the lightweight `SceneItem` variant.
pub(crate) struct SceneEntry {
    pub(crate) id: ItemId,
    pub(crate) scene_rect: Rect,
    /// The widget to materialize into the arena. `Some` until
    /// [`SceneView::build`](crate::view::SceneView) consumes it via
    /// `BuildContext::add_boxed`; `None` afterwards.
    pub(crate) pending_widget: Option<Box<dyn Widget>>,
}

/// The data model behind a `SceneView`: a flat collection of items at
/// scene coordinates. The Scene itself does no rendering or layout —
/// it's a passive container the view reads from at build / place /
/// paint time.
///
/// In Phase 1 only [`Scene::add_widget`] is exercised. The other
/// mutators (`move_item`, `remove`) and the query method
/// (`items_in_rect`) are wired up so later phases can lean on them
/// without API churn — but full runtime mutation (mutate-after-build)
/// is deferred to Phase 6.
pub struct Scene {
    pub(crate) entries: Vec<SceneEntry>,
}

impl Scene {
    /// An empty scene.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
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
        self.entries.push(SceneEntry {
            id,
            scene_rect,
            pending_widget: Some(Box::new(widget)),
        });
        id
    }

    /// Update an item's scene rectangle. No-op if the id isn't in the
    /// scene. Phase 1 mutation: takes effect on the next layout pass
    /// (after SceneView has been built once); Phase 6 wires this to
    /// drag-to-move and the spatial index.
    pub fn move_item(&mut self, id: ItemId, new_bounds: Rect) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.scene_rect = new_bounds;
        }
    }

    /// Remove an item by id. No-op if the id isn't in the scene.
    /// Phase 1 only really makes sense before SceneView has been built;
    /// post-build removal is Phase 6.
    pub fn remove(&mut self, id: ItemId) {
        self.entries.retain(|e| e.id != id);
    }

    /// All items whose `scene_rect` intersects the query rectangle, in
    /// insertion order. Brute-force `O(N)` scan in Phase 1; replaced
    /// by a `SpatialIndex::query` lookup in Phase 3.
    pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId> {
        self.entries
            .iter()
            .filter(|e| rects_intersect(e.scene_rect, scene_rect))
            .map(|e| e.id)
            .collect()
    }

    /// Read an item's current scene rectangle. Returns `None` if the
    /// id isn't in the scene.
    pub fn scene_rect(&self, id: ItemId) -> Option<Rect> {
        self.entries
            .iter()
            .find(|e| e.id == id)
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
            .finish_non_exhaustive()
    }
}

/// Standard half-open AABB intersection: two rects intersect iff their
/// projections overlap on both axes. Inlined here in Phase 1; will
/// likely move to `fern-canvas` once the spatial index in Phase 3
/// needs the same predicate.
fn rects_intersect(a: Rect, b: Rect) -> bool {
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
