//! [`SceneView`] — the viewport widget that hosts a [`Scene`] and
//! places its items at scene coordinates.
//!
//! Phase 1 surface: identity view transform, no pan/zoom, no spatial
//! index, no lightweight items, no a11y customisation. The view is a
//! plain composing container whose children are the scene's
//! materialised heavyweight widgets, placed at their `scene_rect`s in
//! parent-local coordinates.
//!
//! Phase 2 layers four animated `Signal<f32>`s (`pan_x`, `pan_y`,
//! `zoom`, `rotation`) and applies a `set_transform` scope on top of
//! the same content, so the placement code below stays unchanged
//! (it always plants children at scene coords; the transform happens
//! at the renderer / hit-test / a11y level).

use std::collections::HashMap;

use fern_canvas::{Point, Rect, Size, SizeProposal};
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;

use crate::item::ItemId;
use crate::scene::Scene;

/// A pannable/zoomable viewport hosting a [`Scene`]'s items at scene
/// coordinates. In Phase 1 the view transform is fixed at identity; in
/// Phase 2 the configuration knobs (`min_zoom`, `pan_modes`,
/// `a11y_off_screen_mode`, …) become live and pan/zoom signals drive a
/// `set_transform` scope on the content.
#[derive(Debug)]
pub struct SceneView {
    scene: Scene,
    /// Materialisation map populated during `build`: every entry whose
    /// `pending_widget` was consumed appears here as
    /// `(item_id → widget_id)`. Stable across rebuilds — subsequent
    /// `build` calls just return the cached widget ids unchanged.
    materialized: HashMap<ItemId, WidgetId>,
    /// Fallback size when the parent's `SizeProposal` is unspecified
    /// on either axis. The viewport is conceptually a "fill available
    /// space" widget; without a proposal we land on a sensible
    /// rectangle for a top-level scene-style window pane.
    default_size: Size,
}

impl SceneView {
    /// Wrap a [`Scene`] in a viewport. The scene is moved into the
    /// view; query / mutate it later via [`SceneView::scene_mut`].
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            materialized: HashMap::new(),
            default_size: Size::new(800.0, 600.0),
        }
    }

    /// Override the size used when the parent doesn't propose one on
    /// an axis. Defaults to 800×600 logical pixels.
    pub fn default_size(mut self, w: f32, h: f32) -> Self {
        self.default_size = Size::new(w, h);
        self
    }

    /// Read access to the underlying scene model.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Mutable access to the underlying scene model. Intended for
    /// pre-build configuration (Phase 1) or future runtime mutation
    /// (Phase 6); after `SceneView` has been added to the tree, fresh
    /// `add_widget` calls only take effect on the next rebuild.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// The `WidgetId` an item was materialised as, if known. Available
    /// after `build` has run at least once and the entry's
    /// `pending_widget` has been consumed.
    pub fn widget_id_for(&self, id: ItemId) -> Option<WidgetId> {
        self.materialized.get(&id).copied()
    }
}

impl Widget for SceneView {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Drain any newly-added entries. Already-materialised entries
        // (pending_widget = None, present in `self.materialized`) are
        // returned unchanged so subsequent rebuilds don't re-insert
        // their widgets.
        let mut child_ids = Vec::with_capacity(self.scene.entries.len());
        for entry in self.scene.entries.iter_mut() {
            if let Some(widget) = entry.pending_widget.take() {
                let wid = ctx.add_boxed(widget);
                self.materialized.insert(entry.id, wid);
                child_ids.push(wid);
            } else if let Some(wid) = self.materialized.get(&entry.id).copied() {
                child_ids.push(wid);
            }
        }
        child_ids
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Greedy: take the proposed size on each axis, fall back to
        // `default_size` when unspecified. The scene viewport is
        // intended to fill its parent's slot — Phase 2 doesn't change
        // this; the view transform is purely visual.
        let size = proposal.resolve(self.default_size.width, self.default_size.height);
        LayoutResponse::rigid(size)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Phase 1: identity view transform, so a child's scene-coord
        // rectangle becomes its parent-local placement directly. The
        // viewport's `bounds.origin()` is the scene-coord origin in
        // parent-local space.
        //
        // Phase 2 keeps this exact logic; the view transform is
        // applied separately as a `set_transform` scope at build time.
        for placement in children.iter_mut() {
            if let Some(rect) = self.scene_rect_for(placement.id) {
                placement.origin =
                    Point::new(bounds.x + rect.x, bounds.y + rect.y);
                placement.size = Size::new(rect.width, rect.height);
            }
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

impl SceneView {
    /// O(N) scan over `scene.entries` matching against
    /// `self.materialized`. Acceptable in Phase 1 (handful of items);
    /// Phase 3's spatial index removes this from the hot path
    /// entirely (only viewport-intersecting items reach
    /// `place_children`).
    fn scene_rect_for(&self, widget_id: WidgetId) -> Option<Rect> {
        for entry in &self.scene.entries {
            if self.materialized.get(&entry.id) == Some(&widget_id) {
                return Some(entry.scene_rect);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    /// Minimal leaf widget for view-level placement tests. See
    /// `scene.rs` for the same shim; the integration tests use real
    /// fern-widgets components.
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
    fn scene_view_places_widgets_at_scene_coords() {
        // The Phase 1 acceptance test: a SceneView wrapping a Scene
        // with widgets at fixed scene rects must lay each widget out
        // at exactly that rect (in screen coordinates, anchored at
        // the SceneView's bounds origin — identity view transform).
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::new(10.0, 20.0, 100.0, 50.0));
        let b = scene.add_widget(FillWidget::new(), Rect::new(200.0, 100.0, 80.0, 80.0));

        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let kids = tree.children(view_id);
        assert_eq!(kids.len(), 2, "both widgets must materialise");

        // Placement order = scene insertion order (Phase 1).
        assert_eq!(tree.bounds(kids[0]), Rect::new(10.0, 20.0, 100.0, 50.0));
        assert_eq!(tree.bounds(kids[1]), Rect::new(200.0, 100.0, 80.0, 80.0));

        // `widget_id_for` round-trips for both items.
        let view = tree
            .widget_as_any(view_id)
            .and_then(|a| a.downcast_ref::<SceneView>())
            .expect("view is a SceneView");
        assert_eq!(view.widget_id_for(a), Some(kids[0]));
        assert_eq!(view.widget_id_for(b), Some(kids[1]));
    }

    #[test]
    fn scene_view_layout_takes_proposal() {
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let bounds = tree.bounds(view_id);
        assert_eq!(bounds.width, 400.0);
        assert_eq!(bounds.height, 300.0);
    }

    #[test]
    fn empty_scene_has_no_children() {
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert!(tree.children(view_id).is_empty());
    }

    #[test]
    fn scene_view_default_size_when_proposal_unspecified() {
        // When the parent proposes nothing on either axis, the
        // viewport falls back to its `default_size`. Used by
        // top-level scenes whose parent is unconstrained.
        let scene = Scene::new();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene).default_size(640.0, 480.0));
        tree.layout(SizeProposal::unspecified());
        let bounds = tree.bounds(view_id);
        assert_eq!(bounds.width, 640.0);
        assert_eq!(bounds.height, 480.0);
    }
}
