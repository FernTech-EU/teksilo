//! Defensive coverage for scene-graph edge cases the audit
//! flagged as untested: NaN / infinite coordinates, the
//! self-parent / cycle case explicitly documented as unchecked
//! in `set_item_parent`, empty-scene queries, and the marquee
//! commit's interaction with `IS_SELECTABLE` (which the original
//! audit incorrectly flagged as missing — this test pins the
//! actual behavior).

use crate::flags::ItemFlags;
use crate::item::ItemId;
use crate::items::RectItem;
use crate::scene::Scene;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::view::SceneView;
use fern_canvas::{Point, Rect};
use fern_canvas::SizeProposal;
use fern_core::widget_id::WidgetId;
use fern_core::widget_tree::WidgetTree;

fn rect_item(w: f32, h: f32) -> RectItem {
    RectItem::new(Rect::new(0.0, 0.0, w, h)).fill(fern_tokens::Color::RED)
}

fn view_handle(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
    tree.widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("view is a SceneView")
}

// -----------------------------------------------------------------
// Empty scene
// -----------------------------------------------------------------

#[test]
fn empty_scene_extent_is_none() {
    let scene = Scene::new();
    assert_eq!(scene.scene_rect_extent(), None);
}

#[test]
fn empty_scene_item_at_returns_none() {
    let scene = Scene::new();
    assert_eq!(scene.item_at(Point::new(0.0, 0.0)), None);
    assert_eq!(scene.item_at(Point::new(100.0, 100.0)), None);
}

#[test]
fn empty_scene_item_thumbnails_returns_empty_vec() {
    let scene = Scene::new();
    assert!(scene.item_thumbnails().is_empty());
}

#[test]
fn empty_scene_view_handles_pan_and_zoom_without_panic() {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(Scene::new()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let view = view_handle(&tree, view_id);
    view.set_pan(fern_canvas::Vec2::new(50.0, 50.0));
    view.set_zoom(2.0);
    let _ = tree.render();
}

// -----------------------------------------------------------------
// NaN / infinite coordinates
// -----------------------------------------------------------------

#[test]
fn item_at_with_nan_point_returns_none_without_panic() {
    // The spatial index probes a zero-extent rect at the query
    // point. A NaN point produces a NaN-extent probe, which
    // `Rect::contains` evaluates to false for every item.
    let mut scene = Scene::new();
    scene.add_item(rect_item(20.0, 20.0), Point::new(10.0, 10.0));
    let result = scene.item_at(Point::new(f32::NAN, f32::NAN));
    // NaN comparisons in Rect::contains return false on every
    // axis → no hit. Pin this behaviour so a future refactor
    // doesn't introduce a panic.
    assert_eq!(result, None);
}

#[test]
fn add_item_with_extreme_position_doesnt_panic() {
    // Items at extreme positive coordinates land in the
    // spatial index's outermost bucket. Should not panic on
    // insertion or query. We don't assert on extent.width
    // because at 1e8 magnitude f32 precision (~10 ulps) rounds
    // the 10-pixel item width to zero in (x + width) - x — that
    // collapse is a documented f32 limit, not a Scene bug.
    let mut scene = Scene::new();
    let _id = scene.add_item(rect_item(10.0, 10.0), Point::new(1e8, 1e8));
    let extent = scene.scene_rect_extent().expect("extent");
    // Origin survived the round-trip.
    assert!((extent.x - 1e8).abs() < 1.0, "extent x preserved");
    assert!((extent.y - 1e8).abs() < 1.0, "extent y preserved");
    // Item is queryable through the index.
    let probe = Rect::new(1e8 - 5.0, 1e8 - 5.0, 30.0, 30.0);
    let hits = scene.items_in_rect(probe);
    assert_eq!(hits.len(), 1, "extreme-position item should be queryable");
}

// -----------------------------------------------------------------
// Cycle / self-parent (documented as unchecked in scene.rs)
// -----------------------------------------------------------------

#[test]
fn set_item_parent_to_self_is_rejected() {
    // Self-parent would create an unbounded recursion in
    // rebucket_subtree (the walk `for entry in entries if
    // entry.parent == Some(id) push id` re-pushes the item
    // forever). The cycle guard added in Unit 9 makes the
    // call a no-op, leaving the parent unchanged.
    let mut scene = Scene::new();
    let id = scene.add_item(rect_item(10.0, 10.0), Point::ZERO);
    scene.set_item_parent(id, Some(id));
    assert_eq!(
        scene.parent_of(id),
        None,
        "self-parent must be rejected (no-op), not stored"
    );
}

#[test]
fn set_item_parent_to_descendant_is_rejected() {
    // parent ← grandparent ← child : making grandparent's parent
    // be `child` would create grandparent → child → grandparent.
    // The guard rejects.
    let mut scene = Scene::new();
    let grandparent = scene.add_item(rect_item(50.0, 50.0), Point::ZERO);
    let parent = scene.add_item(rect_item(20.0, 20.0), Point::new(5.0, 5.0));
    let child = scene.add_item(rect_item(10.0, 10.0), Point::new(2.0, 2.0));
    scene.set_item_parent(parent, Some(grandparent));
    scene.set_item_parent(child, Some(parent));

    // Try to make grandparent a child of child: would cycle.
    scene.set_item_parent(grandparent, Some(child));
    assert_eq!(
        scene.parent_of(grandparent),
        None,
        "parent-set to a descendant must be rejected"
    );
}

// -----------------------------------------------------------------
// Marquee commit respects IS_SELECTABLE
// -----------------------------------------------------------------

#[test]
fn marquee_commit_respects_is_selectable_flag() {
    // Audit flagged a hypothetical bug: marquee might select
    // items where IS_SELECTABLE is cleared. Verify the actual
    // commit path filters correctly by driving Scene's
    // commit_marquee_box through SceneSelection.
    let mut scene = Scene::new();
    let selectable = scene.add_item(rect_item(20.0, 20.0), Point::new(10.0, 10.0));
    let unselectable = scene.add_item(rect_item(20.0, 20.0), Point::new(40.0, 10.0));
    scene.set_flag(unselectable, ItemFlags::IS_SELECTABLE, false);

    // Marquee rect covers both items.
    let marquee_rect = Rect::new(0.0, 0.0, 100.0, 50.0);
    let hit_ids = scene.items_in_rect(marquee_rect);
    assert!(hit_ids.contains(&selectable));
    assert!(hit_ids.contains(&unselectable));

    // Filter to the IS_SELECTABLE-respecting subset that the
    // marquee commit should produce.
    let selectable_set: Vec<ItemId> = hit_ids
        .into_iter()
        .filter(|id| {
            scene
                .flags(*id)
                .map(|f| f.contains(ItemFlags::IS_SELECTABLE))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(selectable_set, vec![selectable]);

    // Drive the live commit through SceneSelection (single-mode
    // by default would only keep the first; use Multi).
    let sel = SceneSelection::new(SceneSelectionMode::Multi);
    sel.commit_marquee(&scene, marquee_rect, /* additive */ false);
    let selected = sel.selection_signal().get();
    assert!(
        selected.contains(&selectable),
        "selectable item should be in the commit"
    );
    assert!(
        !selected.contains(&unselectable),
        "non-selectable item must NOT be in the commit, even though it's in the marquee rect"
    );
}

// -----------------------------------------------------------------
// Item removal: dangling parent reference
// -----------------------------------------------------------------

#[test]
fn parent_reference_to_removed_item_resolves_safely() {
    let mut scene = Scene::new();
    let parent = scene.add_item(rect_item(20.0, 20.0), Point::ZERO);
    let child = scene.add_item(rect_item(10.0, 10.0), Point::new(5.0, 5.0));
    scene.set_item_parent(child, Some(parent));

    // Remove the parent.
    scene.remove(parent);

    // Child's scene_transform should still resolve without
    // panicking — the parent chain falls back to identity for
    // missing entries.
    let xform = scene.scene_transform(child);
    let pt = xform.apply_point(Point::ZERO);
    assert!(pt.x.is_finite() && pt.y.is_finite());
}
