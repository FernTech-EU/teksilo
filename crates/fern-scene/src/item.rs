//! Scene item identity and the lightweight [`SceneItem`] trait.
//!
//! ## Two tiers
//!
//! - **Heavyweight tier** — any `Widget` (Button, TextInput, Panel,
//!   …) added via [`Scene::add_widget`](crate::Scene::add_widget).
//!   Lives in the arena, fully interactive, full a11y.
//! - **Lightweight tier (Phase 4+)** — `SceneItem`s added via
//!   [`Scene::add_item`](crate::Scene::add_item). No arena overhead;
//!   drawn from `SceneView::paint` in scene-coord space (the view
//!   transform is already on the renderer's stack at that point).
//!   Use for the "background furniture" of a scene — connector
//!   lines, tiled grids, decorations — where thousands of items
//!   need to render cheaply.
//!
//! Built-in lightweight items: [`RectItem`](crate::RectItem),
//! [`PathItem`](crate::PathItem), [`ImageItem`](crate::ImageItem),
//! [`TextItem`](crate::TextItem), [`GroupItem`](crate::GroupItem).

use std::sync::atomic::{AtomicU64, Ordering};

use fern_canvas::{Canvas, Point, Rect, Transform2D};

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

/// Context handed to [`SceneItem::paint`]. The canvas already has
/// the SceneView's view transform pushed onto its stack, so painting
/// at scene coordinates "just works": `canvas.fill_rect(scene_rect,
/// color)` lands at the correct screen position under the current
/// pan / zoom / rotation.
///
/// The `view_transform` field is the same transform; exposed so
/// items that need to draw in a non-transformed frame (e.g. a
/// pixel-aligned border that shouldn't shrink under zoom) can apply
/// the inverse manually. Phase 4 doesn't ship any such item; it's
/// here for custom implementations.
#[derive(Debug, Clone, Copy)]
pub struct SceneItemPaintContext {
    pub view_transform: Transform2D,
    /// The dirty region the renderer is currently painting, expressed
    /// in scene coordinates. Items whose `bounds_in_scene` doesn't
    /// intersect this can skip drawing entirely. `None` means
    /// "redraw everything" — the renderer hasn't computed a partial
    /// dirty rect for this frame.
    pub dirty_scene_rect: Option<Rect>,
}

/// A lightweight, paint-only scene item. Implementors define their
/// scene-coord bounds, how to paint themselves, and (optionally) a
/// custom hit-test. Items live alongside heavyweight widgets in the
/// same [`Scene`](crate::Scene) and share the same spatial index for
/// queries / culling.
///
/// Phase 4 is paint-and-hit-test only; the `accessibility` hook
/// lands in Phase 5 once the synthetic-NodeId surface is in place.
pub trait SceneItem: std::fmt::Debug + Send + 'static {
    /// Axis-aligned bounding box in scene coordinates. Used by the
    /// spatial index for bucketing / queries / culling. The trait
    /// allows the bounds to change per-frame (it's a method, not a
    /// stored value) but stable bounds are strongly preferred —
    /// `Scene` re-buckets via `Scene::move_item` only for widgets
    /// in Phase 4. If a custom item changes bounds dynamically,
    /// it must trigger a `Scene` mutation that re-bucket itself
    /// (Phase 6 territory).
    fn bounds_in_scene(&self) -> Rect;

    /// Paint the item. The canvas's transform stack has the
    /// SceneView's view transform pushed, so painting at
    /// scene-coord positions lands correctly under pan/zoom.
    fn paint(&self, canvas: &mut Canvas, ctx: &SceneItemPaintContext);

    /// Whether `scene_point` (in scene coordinates) hits this item.
    /// Default: AABB containment via `bounds_in_scene`. Path-based
    /// items (e.g. a thin polyline connector) override this to do
    /// per-segment distance checks so users can click along the
    /// stroke even when the AABB is huge.
    fn hit_test(&self, scene_point: Point) -> bool {
        self.bounds_in_scene().contains(scene_point)
    }

    /// Optional human-readable label, shown by debug introspection
    /// and used by the Phase 5 a11y walker as the default
    /// `accessibility` name when an item author hasn't overridden
    /// it.
    fn label(&self) -> Option<&str> {
        None
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
