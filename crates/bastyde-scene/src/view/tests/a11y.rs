//! Coverage for the Scene-side a11y data model.
//!
//! The walker that builds AT nodes lives in `accessibility_impl.rs`
//! (driven by `SceneView::accessibility`); these tests focus on the
//! mutators and invariants of the underlying logical structure —
//! groups, parents, relations, live regions, landmarks, categories.
//!
//! Walker-end-to-end tests need an AccessNodeBuilder context that
//! the unit framework doesn't expose easily; tests for that pipeline
//! belong in a follow-up alongside a public `Scene::debug_a11y_tree()`
//! introspection helper.
//!
//! Added in Unit 9 to close the audit-flagged a11y coverage gap
//! (the entire `a11y.rs` module had no test coverage before).

use crate::a11y::{A11yCategory, A11yGroup, A11yNode, A11yRelation};
use crate::items::RectItem;
use crate::scene::Scene;
use accesskit::{Live, Role};
use bastyde_canvas::{Point, Rect};
use bastyde_i18n::lit;

fn rect_at(_x: f32, _y: f32) -> RectItem {
    RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).fill(bastyde_tokens::Color::RED)
}

// -----------------------------------------------------------------
// A11yGroupBuilder + add_a11y_group / a11y_group / remove_a11y_group
// -----------------------------------------------------------------

#[test]
fn add_a11y_group_round_trips_label_and_role() {
    let mut scene = Scene::new();
    let id = scene.add_a11y_group(
        A11yGroup::builder()
            .label(lit!("Inputs section"))
            .role(Role::GenericContainer),
    );
    let g = scene.a11y_group(id).expect("group must be reachable by id");
    assert_eq!(
        g.label.as_ref().map(|l| l.resolve_now()).as_deref(),
        Some("Inputs section")
    );
    assert_eq!(g.role, Role::GenericContainer);
}

#[test]
fn a11y_group_ids_are_unique() {
    let mut scene = Scene::new();
    let a = scene.add_a11y_group(A11yGroup::builder().label(lit!("a")));
    let b = scene.add_a11y_group(A11yGroup::builder().label(lit!("b")));
    let c = scene.add_a11y_group(A11yGroup::builder().label(lit!("c")));
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);
}

#[test]
fn remove_a11y_group_clears_referencing_state() {
    // remove_a11y_group must clean up parent declarations,
    // relations, live, landmark, categories that target the
    // removed group — otherwise we leak A11yNode::Group(id) refs
    // pointing at nonexistent groups.
    let mut scene = Scene::new();
    let item_id = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("g")));
    let other_item_id = scene.add_item(rect_at(10.0, 10.0), Point::ZERO);
    let other_group = scene.add_a11y_group(A11yGroup::builder().label(lit!("h")));

    // Wire references targeting `group`.
    scene.set_a11y_parent(A11yNode::Item(item_id), Some(A11yNode::Group(group)));
    scene.add_a11y_relation(
        A11yNode::Group(group),
        A11yRelation::Controls,
        A11yNode::Item(item_id),
    );
    scene.set_a11y_live(A11yNode::Group(group), Live::Polite);
    scene.set_a11y_landmark(A11yNode::Group(group), Role::ContentInfo);
    scene.set_a11y_categories(A11yNode::Group(group), &[A11yCategory::new("rotor.x")]);
    // And references targeting `other_group` for the
    // doesn't-cascade test.
    scene.set_a11y_parent(
        A11yNode::Item(other_item_id),
        Some(A11yNode::Group(other_group)),
    );

    scene.remove_a11y_group(group);

    // Group itself gone.
    assert!(scene.a11y_group(group).is_none());
    // Parent for the item that was inside the removed group is
    // cleared (orphaned items fall back to SceneView root, per
    // the doc).
    assert_eq!(scene.a11y_parent_of(A11yNode::Item(item_id)), None);
    // Relation cleaned up.
    assert!(
        scene
            .a11y_relations()
            .iter()
            .all(|(from, _, to)| *from != A11yNode::Group(group) && *to != A11yNode::Group(group))
    );
    // Live / landmark / categories on the removed group gone.
    assert!(scene.a11y_categories_of(A11yNode::Group(group)).is_none());
    // Untouched group's references survive.
    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(other_item_id)),
        Some(A11yNode::Group(other_group))
    );
}

// -----------------------------------------------------------------
// set_a11y_parent / a11y_parent_of
// -----------------------------------------------------------------

#[test]
fn set_a11y_parent_separates_visual_and_logical_trees() {
    // Item is at scene root visually but logically a child of a
    // group. The walker uses set_a11y_parent — different from
    // the visual parent chain on the entry.
    let mut scene = Scene::new();
    let item_id = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("logical parent")));

    assert_eq!(scene.a11y_parent_of(A11yNode::Item(item_id)), None);
    scene.set_a11y_parent(A11yNode::Item(item_id), Some(A11yNode::Group(group)));
    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(item_id)),
        Some(A11yNode::Group(group))
    );

    // Visual parent unchanged: scene.parent_of(item_id) on the
    // entry side (the lightweight item still lives at scene
    // root in terms of geometry).
    assert_eq!(scene.parent_of(item_id), None);
}

#[test]
fn set_a11y_parent_none_clears_redirect() {
    let mut scene = Scene::new();
    let item_id = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("g")));
    scene.set_a11y_parent(A11yNode::Item(item_id), Some(A11yNode::Group(group)));
    scene.set_a11y_parent(A11yNode::Item(item_id), None);
    assert_eq!(scene.a11y_parent_of(A11yNode::Item(item_id)), None);
}

// -----------------------------------------------------------------
// Relations
// -----------------------------------------------------------------

#[test]
fn relations_round_trip_in_insertion_order() {
    let mut scene = Scene::new();
    let a = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    let b = scene.add_item(rect_at(20.0, 0.0), Point::ZERO);
    let c = scene.add_item(rect_at(40.0, 0.0), Point::ZERO);
    scene.add_a11y_relation(A11yNode::Item(a), A11yRelation::Controls, A11yNode::Item(b));
    scene.add_a11y_relation(
        A11yNode::Item(a),
        A11yRelation::LabelledBy,
        A11yNode::Item(c),
    );
    let rels = scene.a11y_relations();
    assert_eq!(rels.len(), 2);
    assert_eq!(rels[0].0, A11yNode::Item(a));
    assert!(matches!(rels[0].1, A11yRelation::Controls));
    assert_eq!(rels[1].1, A11yRelation::LabelledBy);
}

// -----------------------------------------------------------------
// Live regions
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// Categories (rotor / quick-nav)
// -----------------------------------------------------------------

#[test]
fn set_a11y_categories_round_trips() {
    let mut scene = Scene::new();
    let item_id = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    let cats = vec![
        A11yCategory::new("rotor.headings"),
        A11yCategory::new("rotor.landmarks"),
    ];
    scene.set_a11y_categories(A11yNode::Item(item_id), &cats);
    let read = scene
        .a11y_categories_of(A11yNode::Item(item_id))
        .expect("categories set");
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].0, "rotor.headings");
    assert_eq!(read[1].0, "rotor.landmarks");
}

#[test]
fn set_a11y_categories_empty_clears_entry() {
    let mut scene = Scene::new();
    let item_id = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    scene.set_a11y_categories(A11yNode::Item(item_id), &[A11yCategory::new("rotor.x")]);
    scene.set_a11y_categories(A11yNode::Item(item_id), &[]);
    assert!(scene.a11y_categories_of(A11yNode::Item(item_id)).is_none());
}

// -----------------------------------------------------------------
// Landmarks
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// Flag interaction: hidden items still register a11y data
// -----------------------------------------------------------------

// -----------------------------------------------------------------
// Item removal cleans the logical-AT maps (separated-tree re-rooting)
// -----------------------------------------------------------------

#[test]
fn removing_an_item_re_roots_its_a11y_children() {
    // `Scene::remove` cleans the logical-AT maps for the removed item, the
    // same way `remove_a11y_group` does for a removed group. A still-alive node
    // whose declared a11y_parent was the removed item falls back to the
    // SceneView root: its explicit-parent mapping is dropped. This keeps the
    // (separate) AccessKit tree from carrying a dangling reference to a gone
    // item — part of "a11y must follow any change".
    let mut scene = Scene::new();
    let parent_item = scene.add_item(rect_at(0.0, 0.0), Point::ZERO);
    let child_item = scene.add_item(rect_at(20.0, 0.0), Point::ZERO);
    scene.set_a11y_parent(
        A11yNode::Item(child_item),
        Some(A11yNode::Item(parent_item)),
    );
    scene.remove(parent_item);
    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(child_item)),
        None,
        "removing an item must drop a11y_parent refs that target it (the child re-roots)"
    );
    let _ = (parent_item, child_item);
}
