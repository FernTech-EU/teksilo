// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What an assistive technology gets from a lightweight [`TextItem`].
//!
//! A scene item's AccessKit node is built by the walker in
//! `a11y_impl.rs`, under a view transform the item never sees at paint
//! time, so its geometry is only right end to end — and these tests
//! exercise it that way, through a real `SceneView` in a `WidgetTree`
//! with a measuring text backend.
//!
//! The walks go through `accessibility_tree_snapshot`, which is a full
//! walk every time. `sync_accessibility` serves a cached tree whose
//! synthetic children are patched only when their *owner* widget moved,
//! and panning a scene moves the content inside a stationary viewport.

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::{Node, NodeId, Role, TreeUpdate};
use accesskit_consumer::{NodeRef, Tree};
use teksilo_canvas::{MockTextBackend, Point, Rect, SizeProposal, Vec2};
use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;

use crate::{ItemId, Scene, SceneView, TextItem};

/// The mock backend's fixed metrics, so the rectangles below are
/// checkable by hand.
const CHAR_WIDTH: f32 = 8.0;
const LINE_HEIGHT: f32 = 16.0;

/// A tree holding one `SceneView` over one text item, laid out and
/// painted — painting is what gives the item the layout its runs report.
fn painted(item: TextItem, at: Point) -> (WidgetTree, WidgetId, ItemId) {
    let mut scene = Scene::new();
    let item_id = scene.add_item(item, at);
    let backend = Rc::new(RefCell::new(MockTextBackend::new()));
    let mut tree = WidgetTree::new().with_text_backend(backend);
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let _ = tree.render();
    (tree, view_id, item_id)
}

fn view_of(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
    tree.widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("the view is a SceneView")
}

/// Re-layout, re-paint and re-walk after a camera change.
fn refresh(tree: &mut WidgetTree) -> TreeUpdate {
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let _ = tree.render();
    tree.accessibility_tree_snapshot()
}

fn node(update: &TreeUpdate, id: NodeId) -> &Node {
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n)
        .unwrap_or_else(|| panic!("node {id:?} is absent from the emitted tree"))
}

fn item_id_of(view_id: WidgetId, item_id: ItemId) -> NodeId {
    synthetic_node_id(view_id, item_id.as_u64(), SyntheticKind::SceneItem)
}

fn item_node(update: &TreeUpdate, view_id: WidgetId, item_id: ItemId) -> &Node {
    node(update, item_id_of(view_id, item_id))
}

/// The item's `Role::TextRun` children, in emission order.
fn runs(update: &TreeUpdate, view_id: WidgetId, item_id: ItemId) -> Vec<&Node> {
    item_node(update, view_id, item_id)
        .children()
        .iter()
        .map(|cid| node(update, *cid))
        .filter(|n| n.role() == Role::TextRun)
        .collect()
}

/// Locate a node in the consumer's tree by the id it reports.
fn find<'a>(state: &'a accesskit_consumer::TreeState, id: NodeId) -> NodeRef<'a> {
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.locate().0 == id {
            return node;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    panic!("node {id:?} is absent from the emitted tree");
}

#[test]
fn a_text_item_supports_text_ranges() {
    // `supports_text_ranges` is the gate every platform's text API sits
    // behind — `AXBoundsForRange`, UIA's `TextPattern` and AT-SPI's
    // `Text` interface all answer through it, and it is false for a
    // label with no runs.
    let (mut tree, view_id, item_id) = painted(
        TextItem::new(lit!("Scene node"), Rect::new(0.0, 0.0, 200.0, 30.0)),
        Point::new(40.0, 40.0),
    );
    let update = refresh(&mut tree);

    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let item = find(state, item_id_of(view_id, item_id));
    assert!(
        item.supports_text_ranges(),
        "a scene text item must be reviewable by character, word and line"
    );
    // The consumer derives the document text from the runs while the
    // platform announces the node's own value; a divergence is a place
    // where what a reader reviews and what it hears disagree.
    assert_eq!(item.document_range().text(), "Scene node");
    assert_eq!(item.value(), Some("Scene node".to_string()));
    assert!(
        !item.document_range().bounding_boxes().is_empty(),
        "one geometry-less run empties the boxes of every range around it"
    );
}

#[test]
fn the_runs_are_direct_children_of_the_text_item() {
    // `accesskit_consumer` routes a run's update to its *filtered*
    // parent, and anything in between that does not itself support text
    // ranges makes macOS, Windows and AT-SPI all drop the text-change
    // event.
    let (mut tree, view_id, item_id) = painted(
        TextItem::new(lit!("Hi"), Rect::new(0.0, 0.0, 100.0, 30.0)),
        Point::ZERO,
    );
    let update = refresh(&mut tree);
    let item = item_node(&update, view_id, item_id);
    assert_eq!(item.role(), Role::Label);
    assert_eq!(
        item.children().len(),
        1,
        "the runs hang straight off the label"
    );
    assert_eq!(node(&update, item.children()[0]).role(), Role::TextRun);
}

#[test]
fn a_zoomed_view_scales_the_reported_character_positions() {
    // Zoom never reflows scene text — the view paints one logical layout
    // larger — so the window-space extents AT reads are the layout's
    // own, multiplied by the zoom.
    let (mut tree, view_id, item_id) = painted(
        TextItem::new(lit!("abcd"), Rect::new(0.0, 0.0, 100.0, 30.0)),
        Point::new(10.0, 20.0),
    );

    let update = refresh(&mut tree);
    let unzoomed_runs = runs(&update, view_id, item_id);
    assert_eq!(unzoomed_runs.len(), 1);
    assert_eq!(
        unzoomed_runs[0].character_positions(),
        Some(&[0.0, CHAR_WIDTH, 2.0 * CHAR_WIDTH, 3.0 * CHAR_WIDTH][..])
    );
    assert_eq!(
        unzoomed_runs[0].character_widths(),
        Some(&[CHAR_WIDTH; 4][..])
    );
    let unzoomed = unzoomed_runs[0]
        .bounds()
        .expect("a measured run reports its box");
    assert_eq!(unzoomed.x0, 10.0);
    assert_eq!(unzoomed.x1, 10.0 + 4.0 * f64::from(CHAR_WIDTH));
    assert_eq!(unzoomed.y1 - unzoomed.y0, f64::from(LINE_HEIGHT));

    view_of(&tree, view_id).set_zoom(2.0);
    let update = refresh(&mut tree);
    let zoomed_runs = runs(&update, view_id, item_id);
    assert_eq!(zoomed_runs.len(), 1);
    assert_eq!(
        zoomed_runs[0].character_positions(),
        Some(&[0.0, 2.0 * CHAR_WIDTH, 4.0 * CHAR_WIDTH, 6.0 * CHAR_WIDTH][..]),
        "character positions are window-space offsets, not logical ones"
    );
    assert_eq!(
        zoomed_runs[0].character_widths(),
        Some(&[2.0 * CHAR_WIDTH; 4][..])
    );
    let zoomed = zoomed_runs[0]
        .bounds()
        .expect("a measured run reports its box");
    assert_eq!(zoomed.x0, 20.0);
    assert_eq!(zoomed.x1, 20.0 + 8.0 * f64::from(CHAR_WIDTH));
    assert_eq!(zoomed.y1 - zoomed.y0, 2.0 * f64::from(LINE_HEIGHT));
}

#[test]
fn a_rotated_item_reports_degenerate_geometry() {
    // A run describes its characters as offsets along one reading
    // direction inside one axis-aligned box, which cannot express a
    // rotated baseline. A zero-width box only stops a magnifier
    // magnifying; a wrong one sends it where the text is not.
    let (mut tree, view_id, item_id) = painted(
        TextItem::new(lit!("Axis"), Rect::new(0.0, 0.0, 100.0, 30.0))
            .rotation(std::f32::consts::FRAC_PI_2),
        Point::new(10.0, 20.0),
    );
    let update = refresh(&mut tree);
    let runs = runs(&update, view_id, item_id);
    assert_eq!(runs.len(), 1, "the text is still announced and reviewable");
    assert_eq!(runs[0].value(), Some("Axis"));
    assert_eq!(runs[0].character_positions(), Some(&[0.0; 4][..]));
    assert_eq!(runs[0].character_widths(), Some(&[0.0; 4][..]));
    let bounds = runs[0]
        .bounds()
        .expect("a degenerate run still reports a box");
    assert_eq!(bounds.x1 - bounds.x0, 0.0);
}

#[test]
fn a_scrolled_view_moves_its_items_and_their_runs() {
    // The runs carry absolute rects, so they must follow the pan the
    // item's own node follows — by exactly as much, or a magnifier and
    // an object-navigating reader disagree about where the label is.
    let (mut tree, view_id, item_id) = painted(
        TextItem::new(lit!("abcd"), Rect::new(0.0, 0.0, 100.0, 30.0)),
        Point::new(10.0, 20.0),
    );
    let before = refresh(&mut tree);
    let item_before = item_node(&before, view_id, item_id)
        .bounds()
        .expect("the item advertises a box");
    let run_before = runs(&before, view_id, item_id)[0]
        .bounds()
        .expect("a measured run reports its box");

    view_of(&tree, view_id).set_pan(Vec2::new(-30.0, -45.0));
    let pan = view_of(&tree, view_id).pan();
    assert_ne!(pan.x, 0.0, "the scene must actually have panned");
    let after = refresh(&mut tree);
    let item_after = item_node(&after, view_id, item_id)
        .bounds()
        .expect("the item advertises a box");
    let run_after = runs(&after, view_id, item_id)[0]
        .bounds()
        .expect("a measured run reports its box");

    assert_eq!(item_after.x0 - item_before.x0, f64::from(pan.x));
    assert_eq!(item_after.y0 - item_before.y0, f64::from(pan.y));
    assert_eq!(run_after.x0 - run_before.x0, f64::from(pan.x));
    assert_eq!(run_after.y0 - run_before.y0, f64::from(pan.y));
    // And the characters keep their extents: panning is a translation.
    assert_eq!(
        runs(&after, view_id, item_id)[0].character_widths(),
        Some(&[CHAR_WIDTH; 4][..])
    );
}

#[test]
fn an_access_label_override_is_announced_and_reviewed_as_one_string() {
    // The runs spell out whatever the node announces, because the
    // consumer derives the document text from them. An override
    // describes a different string from the one that was measured, so
    // its geometry is degenerate rather than the painted text's.
    let (mut tree, view_id, item_id) = painted(
        TextItem::new(lit!("12"), Rect::new(0.0, 0.0, 100.0, 30.0)).access_label(lit!("Twelve")),
        Point::ZERO,
    );
    let update = refresh(&mut tree);
    assert_eq!(item_node(&update, view_id, item_id).value(), Some("Twelve"));
    let runs = runs(&update, view_id, item_id);
    assert_eq!(runs[0].value(), Some("Twelve"));
    assert_eq!(runs[0].character_widths(), Some(&[0.0; 6][..]));
}

#[test]
fn an_unpainted_item_still_reports_a_box_for_every_range() {
    // The AT walk can run before the first paint. The label must still
    // be reviewable then — a node with no runs supports no text ranges
    // at all, and a run with no geometry empties the boxes of every
    // range that touches it.
    let mut scene = Scene::new();
    let item_id = scene.add_item(
        TextItem::new(lit!("Scene node"), Rect::new(0.0, 0.0, 200.0, 30.0)),
        Point::new(40.0, 40.0),
    );
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let update = tree.accessibility_tree_snapshot();

    let consumer = Tree::new(update, false);
    let state = consumer.state();
    let item = find(state, item_id_of(view_id, item_id));
    assert!(item.supports_text_ranges());
    assert_eq!(item.document_range().text(), "Scene node");
    assert!(!item.document_range().bounding_boxes().is_empty());
}

#[test]
fn two_text_items_with_the_same_text_get_distinct_run_ids() {
    // Two runs sharing a NodeId panic `accesskit_consumer`'s tree
    // builder, and a scene holding two identical labels is ordinary.
    let mut scene = Scene::new();
    let a = scene.add_item(
        TextItem::new(lit!("Node"), Rect::new(0.0, 0.0, 100.0, 30.0)),
        Point::new(0.0, 0.0),
    );
    let b = scene.add_item(
        TextItem::new(lit!("Node"), Rect::new(0.0, 0.0, 100.0, 30.0)),
        Point::new(0.0, 60.0),
    );
    let backend = Rc::new(RefCell::new(MockTextBackend::new()));
    let mut tree = WidgetTree::new().with_text_backend(backend);
    let view_id = tree.add(SceneView::new(scene));
    let update = refresh(&mut tree);

    let ids_a: Vec<NodeId> = item_node(&update, view_id, a).children().to_vec();
    let ids_b: Vec<NodeId> = item_node(&update, view_id, b).children().to_vec();
    assert_eq!(ids_a.len(), 1);
    assert_eq!(ids_b.len(), 1);
    assert_ne!(ids_a[0], ids_b[0]);
}
