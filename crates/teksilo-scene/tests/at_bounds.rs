// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a scene says its contents are, to an assistive technology.
//!
//! A `SceneView` is a fixed viewport over content in a coordinate system of its
//! own. A heavyweight card at scene (100, 100) has arena bounds of (100, 100)
//! at every pan, and a lightweight item's rectangle is stored the same way, so
//! neither is a window-space rectangle and neither can be published as one.
//!
//! The mechanism under test is one sentence: **every rectangle in the scene
//! subtree is published in scene coordinates, and the camera is declared once
//! as an AccessKit node transform at the top of that subtree.** That is what
//! makes the composed answer right at any pan, zoom and rotation, for both
//! tiers, and for the per-character geometry inside them.
//!
//! Every claim here is measured the way an assistive technology gets it:
//! `sync_accessibility` (the function every platform adapter is fed, cache and
//! all — not `accessibility_tree_snapshot`, which walks afresh every time and
//! so cannot see a missing invalidation) fed through `accesskit_consumer`, the
//! crate every platform adapter is built on. A raw `Node::bounds()` is the
//! wrong instrument for all of them: it is the *undeclared* half of the answer.

use accesskit::{NodeId, TreeUpdate};
use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeState};
use teksilo_canvas::{Point, Rect, Size, SizeProposal, Vec2};
use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{ItemId, RectItem, Scene, SceneView};

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(400.0),
    height: Some(300.0),
};

/// The side of both the card and the item, so "same scene position, same
/// screen rectangle" is a claim about two things of equal size.
const SIDE: f32 = 50.0;

/// A card with something worth focusing in it — the shape that makes a
/// heavyweight entry worth being one.
#[derive(Debug)]
struct Card;

impl Widget for Card {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new()
                .focusable(true)
                .on_tap(|_e, _c| {}),
        );
        vec![]
    }
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(SIDE, SIDE).into()
    }
}

fn view_handle(tree: &WidgetTree, view_id: WidgetId) -> &SceneView {
    tree.widget_as_any(view_id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("the view is a SceneView")
}

/// Locate a node in the consumer's tree by the id Teksilo emitted it under.
fn find<'a>(state: &'a TreeState, target: NodeId) -> NodeRef<'a> {
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if state.locate_node(node.id()).map(|(local, _)| local) == Some(target) {
            return node;
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    panic!("node {target:?} is absent from the published tree");
}

/// The rectangle an assistive technology resolves for a node: raw bounds
/// composed with every transform above it, which is what every platform
/// adapter calls to answer "where is this".
fn screen_rect(update: &TreeUpdate, target: NodeId) -> Rect {
    let consumer = Tree::new(update.clone(), false);
    let state = consumer.state();
    let r = find(state, target)
        .bounding_box()
        .expect("the node advertises a box");
    Rect::new(
        r.x0 as f32,
        r.y0 as f32,
        (r.x1 - r.x0) as f32,
        (r.y1 - r.y0) as f32,
    )
}

/// What an explore-by-touch probe at `at` lands on, starting from the window
/// root — the same descent `AXUIElementCopyElementAtPosition`,
/// `IRawElementProviderFragmentRoot::ElementProviderFromPoint` and AT-SPI's
/// `GetAccessibleAtPoint` all resolve through.
fn hit(update: &TreeUpdate, at: Point) -> Option<NodeId> {
    let consumer = Tree::new(update.clone(), false);
    let state = consumer.state();
    let node = state.root().node_at_point(
        accesskit::Point::new(at.x as f64, at.y as f64),
        &|n: &NodeRef| {
            if accesskit_consumer::common_filter(n) == FilterResult::Include {
                FilterResult::Include
            } else {
                FilterResult::ExcludeNode
            }
        },
    )?;
    state.locate_node(node.id()).map(|(local, _)| local)
}

/// A view holding one card and one lightweight item at the **same** scene
/// position, so the two tiers are directly comparable, plus the ids to ask
/// about. The item is added first so the card, drawn over it, is the one a hit
/// test finds.
fn scene_with_both(at: Point) -> (WidgetTree, WidgetId, CardIds, NodeId) {
    let mut scene = Scene::new();
    let item: ItemId = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, SIDE, SIDE)), at);
    let card = scene.add_widget(Card, Rect::new(at.x, at.y, SIDE, SIDE));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let card_widget = view_handle(&tree, view_id)
        .widget_id_for(card)
        .expect("the card materialised");
    let card_node = widget_id_to_node_id(card_widget);
    let item_node = synthetic_node_id(view_id, item.as_u64(), SyntheticKind::SceneItem);
    (
        tree,
        view_id,
        CardIds {
            widget: card_widget,
            node: card_node,
        },
        item_node,
    )
}

/// The card, addressed both ways: the arena id whose bounds layout writes, and
/// the AccessKit id the published tree names it by.
#[derive(Clone, Copy)]
struct CardIds {
    widget: WidgetId,
    node: NodeId,
}

/// Equal to within a thousandth of a logical pixel — the difference between
/// taking a turned rectangle's bounding box in `f32` and letting the consumer
/// take it in `f64`, and nothing else.
fn close(a: Rect, b: Rect) -> bool {
    let eps = 1e-3;
    (a.x - b.x).abs() < eps
        && (a.y - b.y).abs() < eps
        && (a.width - b.width).abs() < eps
        && (a.height - b.height).abs() < eps
}

fn published(tree: &mut WidgetTree) -> TreeUpdate {
    tree.layout(VIEWPORT);
    tree.sync_accessibility()
}

// ───────────────────────────────────── the card follows the camera

#[test]
fn a_cards_published_rectangle_tracks_a_pan() {
    // The defect this whole file exists for. A card's arena bounds are scene
    // coordinates and do not move when the camera does, so publishing them as
    // they stand told an AT client the card was where the *model* says it is
    // rather than where it is painted — at pan zero those agree, and nowhere
    // else.
    let (mut tree, view_id, card, _) = scene_with_both(Point::new(100.0, 100.0));

    let before = published(&mut tree);
    assert_eq!(
        screen_rect(&before, card.node),
        Rect::new(100.0, 100.0, SIDE, SIDE)
    );

    view_handle(&tree, view_id).set_pan(Vec2::new(-90.0, -40.0));
    let after = published(&mut tree);
    assert_eq!(
        screen_rect(&after, card.node),
        Rect::new(10.0, 60.0, SIDE, SIDE),
        "the card paints at (10, 60) after the pan, and that is where it must \
         say it is"
    );
    assert_eq!(
        tree.bounds(card.widget),
        Rect::new(100.0, 100.0, SIDE, SIDE),
        "…while its arena bounds have deliberately not moved: the placement \
         origin stays canonical so a parked card keeps a position to come back \
         to. The projection is the AT layer's job, not layout's."
    );
}

#[test]
fn a_cards_published_rectangle_tracks_a_zoom() {
    let (mut tree, view_id, card, _) = scene_with_both(Point::new(100.0, 100.0));
    let _ = published(&mut tree);

    view_handle(&tree, view_id).set_zoom(2.0);
    let after = published(&mut tree);
    assert_eq!(
        screen_rect(&after, card.node),
        Rect::new(200.0, 200.0, 2.0 * SIDE, 2.0 * SIDE),
        "a zoomed card is twice the size and twice as far out"
    );
}

#[test]
fn a_cards_published_rectangle_tracks_a_rotation() {
    // The case a pre-projected rectangle could not get right on its own: a
    // `Rect` is axis-aligned, so projecting a rotated one by hand yields its
    // bounding box and loses the shape. Declaring the transform hands AccessKit
    // the exact rectangle and the exact mapping, and lets the consumer take the
    // bounding box at the last moment — which is also what every platform does
    // with it.
    let scene_rect = Rect::new(100.0, 100.0, SIDE, SIDE);
    let (mut tree, view_id, card, item) = scene_with_both(Point::new(100.0, 100.0));
    let upright = published(&mut tree);
    assert_eq!(screen_rect(&upright, card.node), scene_rect);

    view_handle(&tree, view_id).set_rotation(0.3);
    let after = published(&mut tree);
    let rotated = screen_rect(&after, card.node);
    assert_ne!(
        rotated, scene_rect,
        "the camera turned; the answer must too"
    );
    let painted = view_handle(&tree, view_id)
        .view_transform()
        .apply_rect(scene_rect);
    assert!(
        close(rotated, painted),
        "the published rectangle is the painted geometry: {rotated:?} vs {painted:?}"
    );
    assert!(
        rotated.width > SIDE,
        "a turned square\u{2019}s bounding box is wider than the square: {rotated:?}"
    );
    assert!(
        close(rotated, screen_rect(&after, item)),
        "and the lightweight item at the same scene position is turned to the \
         same place, by the same declaration"
    );
}

// ───────────────────────────────────── the two tiers are one space

#[test]
fn the_two_tiers_advertise_the_same_screen_rectangle() {
    // The parity claim. A card and a lightweight item of the same size at the
    // same scene position must be indistinguishable to an AT client that asks
    // where they are — at rest and under any camera. They agree because they
    // are described in one coordinate system, not because two projections
    // happen to round the same way.
    let (mut tree, view_id, card, item) = scene_with_both(Point::new(120.0, 80.0));

    for camera in [
        (Vec2::ZERO, 1.0_f32),
        (Vec2::new(-90.0, 0.0), 1.0),
        (Vec2::new(0.0, -55.0), 1.0),
        (Vec2::new(-30.0, -20.0), 2.0),
        (Vec2::new(40.0, 25.0), 0.5),
    ] {
        {
            let view = view_handle(&tree, view_id);
            view.set_zoom(camera.1);
            view.set_pan(camera.0);
        }
        let update = published(&mut tree);
        assert_eq!(
            screen_rect(&update, card.node),
            screen_rect(&update, item),
            "the tiers disagree at pan {:?} zoom {}",
            camera.0,
            camera.1
        );
    }
}

// ───────────────────────────────────── a probe finds what it aims at

#[test]
fn an_at_hit_test_at_the_painted_position_finds_the_card() {
    // What the wrong rectangle actually costs. A platform's explore-by-touch
    // probe descends the published tree by inverting each node's transform;
    // with the camera undeclared the descent tested a window-space point
    // against scene-space rectangles and landed on the view, or on nothing.
    let (mut tree, view_id, card, _) = scene_with_both(Point::new(100.0, 100.0));

    let at_rest = published(&mut tree);
    assert_eq!(
        hit(&at_rest, Point::new(125.0, 125.0)),
        Some(card.node),
        "at pan zero the two spaces coincide, so this is the control"
    );

    view_handle(&tree, view_id).set_pan(Vec2::new(-90.0, -40.0));
    let panned = published(&mut tree);
    assert_eq!(
        hit(&panned, Point::new(35.0, 85.0)),
        Some(card.node),
        "a probe at the card's painted centre must find the card"
    );
    assert_ne!(
        hit(&panned, Point::new(125.0, 125.0)),
        Some(card.node),
        "…and one aimed where the model says it is must not"
    );
}

// ────────────────────────── a relocated descendant keeps its space

/// A card with one child, so a test has a *deep* descendant to relocate.
#[derive(Debug)]
struct CardWithInner;

impl Widget for CardWithInner {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let inner = ctx.add(teksilo_widgets::TextWidget::new(teksilo_i18n::lit!(
            "Inner"
        )));
        vec![inner]
    }
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(SIDE, SIDE).into()
    }
    fn children(&self) -> Vec<WidgetId> {
        vec![]
    }
}

#[test]
fn a_widget_relocated_into_the_logical_tree_is_still_in_the_scenes_space() {
    // `set_a11y_parent(A11yNode::Widget(..), ..)` lifts a widget from inside a
    // card and hangs it under a logical group — so its AccessKit parent is no
    // longer its arena parent. Its rectangle is still a scene rectangle, and
    // the group it lands under still declares the camera, so the composition
    // survives the move. That is the payoff for putting both tiers in one
    // space rather than pre-projecting one of them: a group whose children
    // came from different tiers would otherwise have to carry two conventions
    // at once, and could carry neither.
    use teksilo_scene::{A11yGroup, A11yNode};

    let mut scene = Scene::new();
    let group = scene.add_a11y_group(A11yGroup::builder().label(teksilo_i18n::lit!("Tools")));
    scene.add_widget(CardWithInner, Rect::new(100.0, 100.0, SIDE, SIDE));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let card_widget = tree.children(view_id)[0];
    let inner = tree.children(card_widget)[0];
    let inner_rect = tree.bounds(inner);
    assert!(
        inner_rect.x >= 100.0,
        "precondition: the inner widget is laid out in scene coordinates \
         inside the card: {inner_rect:?}"
    );

    tree.widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("the view is a SceneView")
        .scene_mut()
        .set_a11y_parent(A11yNode::Widget(inner), Some(A11yNode::Group(group)));

    view_handle(&tree, view_id).set_pan(Vec2::new(-90.0, -40.0));
    let update = published(&mut tree);

    let inner_node = widget_id_to_node_id(inner);
    let group_node = synthetic_node_id(view_id, group.as_u64(), SyntheticKind::SceneGroup);
    assert!(
        update
            .nodes
            .iter()
            .any(|(id, n)| *id == group_node && n.children().contains(&inner_node)),
        "precondition: the relocation happened — the group claims the widget"
    );
    assert_eq!(
        screen_rect(&update, inner_node),
        Rect::new(
            inner_rect.x - 90.0,
            inner_rect.y - 40.0,
            inner_rect.width,
            inner_rect.height
        ),
        "the relocated widget follows the camera through its new parent"
    );
}

// ───────────────────────────── the cache is not allowed to lie either

#[test]
fn a_camera_move_republishes_the_tree_rather_than_serving_the_last_one() {
    // The half of the defect that hid the other half. `sync_accessibility`
    // returns the *cached* tree unless something set `a11y_dirty`, and a
    // relayout deliberately does not. A camera change altered no structure, no
    // focus and no arena bounds — so before the view bound its transform at
    // `AccessibilityOnly`, a walk that had been correct for both tiers was
    // simply never asked for, and the published rectangles stood still.
    //
    // Measured as the difference between the two doors: a fresh walk and the
    // published tree must agree after a pan, and this is the assertion that
    // fails if the invalidation is dropped again.
    let (mut tree, view_id, card, item) = scene_with_both(Point::new(100.0, 100.0));
    let _ = published(&mut tree);

    view_handle(&tree, view_id).set_pan(Vec2::new(-90.0, 0.0));
    tree.layout(VIEWPORT);
    let walked = tree.accessibility_tree_snapshot();
    let served = tree.sync_accessibility();

    assert_eq!(
        screen_rect(&served, card.node),
        screen_rect(&walked, card.node)
    );
    assert_eq!(screen_rect(&served, item), screen_rect(&walked, item));
    assert_eq!(
        screen_rect(&served, card.node),
        Rect::new(10.0, 100.0, SIDE, SIDE)
    );
}

// ─────────────────────────────────────────── the tree stays walkable

#[test]
fn the_published_tree_survives_the_consumers_validation_under_any_camera() {
    // `Tree::new` runs the validation every platform adapter runs on
    // activation — a dangling child, an orphan, a duplicate or an unresolvable
    // focus panics here rather than inside VoiceOver. Declaring a transform
    // touches every node in the scene subtree, so it is worth asserting that
    // none of them became unreachable.
    let (mut tree, view_id, card, item) = scene_with_both(Point::new(60.0, 60.0));
    for (pan, zoom, rotation) in [
        (Vec2::ZERO, 1.0_f32, 0.0_f32),
        (Vec2::new(-1_000.0, -1_000.0), 1.0, 0.0),
        (Vec2::new(-10.0, -10.0), 4.0, 0.3),
        (Vec2::new(5.0, 5.0), 0.25, -1.2),
    ] {
        {
            let view = view_handle(&tree, view_id);
            view.set_pan(pan);
            view.set_zoom(zoom);
            view.set_rotation(rotation);
        }
        let update = published(&mut tree);
        Tree::new(update.clone(), false);
        // Whichever of the two the off-screen policy still describes, it is
        // described in a space the consumer can resolve. A camera far enough
        // out drops both — that is the retention and off-screen machinery
        // doing its job, not a rectangle going missing.
        for node in [card.node, item] {
            if update.nodes.iter().any(|(id, _)| *id == node) {
                let _ = screen_rect(&update, node);
            }
        }
    }
}

// ──────────────────────── synthetic children inside a heavyweight card

/// Where a *rich-text* card says its own sub-structure is.
///
/// Everything above is about a card's own rectangle. A card holding a real
/// editor also publishes synthetic descendants — a text run per visual line,
/// and for a table a `Role::Table` / `Role::Row` / `Role::Cell` each with a box
/// of their own. Those are emitted through `set_child_bounds_local`, which the
/// framework walker resolves as `owner_origin + local` — and inside a scene
/// `owner_origin` is a *scene* coordinate, not a window one.
///
/// So the claim under test is that a cell's box composes the same way the
/// card's does: published in scene coordinates, carried to the screen by the
/// one declared camera transform. If the walker were projecting these to
/// window space instead, they would be right at pan zero and wrong everywhere
/// else — and a magnifier following a caret into a table would jump.
mod rich_text_in_a_card {
    use super::*;
    use accesskit::Role;
    use teksilo_text::text_document::{MoveMode, TextDocument};
    use teksilo_widgets::rich_text::RichTextEditor;

    const CARD_X: f32 = 100.0;
    const CARD_Y: f32 = 100.0;
    const CARD_SIZE: f32 = 260.0;

    /// A scene holding one card whose content is an editor with a filled 2x2
    /// table, plus the view id to drive the camera with.
    fn scene_with_a_table_card() -> (WidgetTree, WidgetId, ItemId) {
        let doc = TextDocument::new();
        doc.set_plain_text("Intro").unwrap();
        {
            let c = doc.cursor_at(0);
            c.set_position(5, MoveMode::MoveAnchor);
            c.insert_table(2, 2).unwrap();
        }
        for text in ["Alpha", "Beta", "Gamma", "Delta"] {
            let Some(pos) = first_empty_cell_end(&doc) else {
                break;
            };
            let c = doc.cursor_at(0);
            c.set_position(pos, MoveMode::MoveAnchor);
            c.insert_text(text).unwrap();
        }

        let mut scene = Scene::new();
        let card = scene.add_widget(
            RichTextEditor::editor(doc),
            Rect::new(CARD_X, CARD_Y, CARD_SIZE, CARD_SIZE),
        );
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        // The editor lays its document out in a debounced frame loop, not in
        // `layout`. Without a tick it publishes zero-width runs and no table
        // geometry at all — which looks exactly like the bug this is hunting.
        tree.layout(VIEWPORT);
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(200));
        tree.layout(VIEWPORT);
        let _ = tree.render();
        (tree, view_id, card)
    }

    /// End of the first cell that is still empty, so successive calls fill the
    /// cells left to right without having to track shifting positions.
    fn first_empty_cell_end(doc: &TextDocument) -> Option<usize> {
        use teksilo_text::text_document::FlowElementSnapshot;
        for el in &doc.snapshot_flow().elements {
            if let FlowElementSnapshot::Table(t) = el {
                for cell in &t.cells {
                    let b = cell.blocks.first()?;
                    if b.text.is_empty() {
                        return Some(b.position);
                    }
                }
            }
        }
        None
    }

    /// The first `Role::Cell` in the published tree, by node id.
    fn a_cell(update: &TreeUpdate) -> NodeId {
        let consumer = Tree::new(update.clone(), false);
        let state = consumer.state();
        let mut stack = vec![state.root()];
        let mut found = Vec::new();
        while let Some(n) = stack.pop() {
            if n.role() == Role::Cell {
                found.push(n.id());
            }
            for child in n.children() {
                stack.push(child);
            }
        }
        found.sort();
        let id = *found.first().expect("the card's table publishes cells");
        let consumer = Tree::new(update.clone(), false);
        consumer
            .state()
            .locate_node(id)
            .map(|(local, _)| local)
            .expect("the cell is in this tree")
    }

    /// The card's own rectangle at rest, and the check that a descendant's box
    /// lies inside it.
    ///
    /// The camera tests compare a box against itself, so a constant offset
    /// would survive both of them. This is the absolute half of the claim: at
    /// rest the card is drawn where the model puts it, and a cell of it
    /// belongs within that rectangle.
    fn assert_inside_the_card(r: Rect) {
        let card = Rect::new(CARD_X, CARD_Y, CARD_SIZE, CARD_SIZE);
        assert!(
            r.x >= card.x
                && r.y >= card.y
                && r.x + r.width <= card.x + card.width
                && r.y + r.height <= card.y + card.height,
            "a cell sits inside the card that holds it: cell {r:?} vs card \
             {card:?}"
        );
    }

    #[test]
    fn a_table_cell_inside_a_card_tracks_a_pan() {
        let (mut tree, view_id, _card) = scene_with_a_table_card();

        let before = published(&mut tree);
        let cell = a_cell(&before);
        let at_rest = screen_rect(&before, cell);
        assert!(
            at_rest.width > 0.0 && at_rest.height > 0.0,
            "the cell must advertise a real box to begin with: {at_rest:?}"
        );
        assert_inside_the_card(at_rest);

        let pan = Vec2::new(-90.0, -40.0);
        view_handle(&tree, view_id).set_pan(pan);
        let after = published(&mut tree);
        let moved = screen_rect(&after, a_cell(&after));

        let expected = Rect::new(
            at_rest.x + pan.x,
            at_rest.y + pan.y,
            at_rest.width,
            at_rest.height,
        );
        assert!(
            close(moved, expected),
            "a cell inside a card has to move with the camera exactly as the \
             card does: expected {expected:?}, got {moved:?}"
        );
    }

    #[test]
    fn a_table_cell_inside_a_card_tracks_a_zoom() {
        let (mut tree, view_id, _card) = scene_with_a_table_card();

        let before = published(&mut tree);
        let at_rest = screen_rect(&before, a_cell(&before));
        assert_inside_the_card(at_rest);

        view_handle(&tree, view_id).set_zoom(2.0);
        let after = published(&mut tree);
        let zoomed = screen_rect(&after, a_cell(&after));

        let expected = Rect::new(
            at_rest.x * 2.0,
            at_rest.y * 2.0,
            at_rest.width * 2.0,
            at_rest.height * 2.0,
        );
        assert!(
            close(zoomed, expected),
            "a zoomed cell is twice the size and twice as far out, like every \
             other rectangle in the scene: expected {expected:?}, got {zoomed:?}"
        );
    }

    #[test]
    fn a_table_cell_inside_a_card_tracks_a_rotation() {
        // The case a pre-projected rectangle cannot get right. A `Rect` is
        // axis-aligned, so a walk that projected these boxes by hand would
        // publish the bounding box of the turned cell and lose the shape.
        // Declaring the camera once hands AccessKit the exact rectangle and
        // the exact mapping, and the consumer takes the bounding box last —
        // so a rotated cell grows in both axes rather than staying put.
        let (mut tree, view_id, _card) = scene_with_a_table_card();

        let before = published(&mut tree);
        let upright = screen_rect(&before, a_cell(&before));
        assert_inside_the_card(upright);

        view_handle(&tree, view_id).set_rotation(0.3);
        let after = published(&mut tree);
        let turned = screen_rect(&after, a_cell(&after));

        assert_ne!(
            turned, upright,
            "a rotated camera must move the cell's published box"
        );

        // The exact bounding box of the turned rectangle. Note the width
        // *shrinks*: a cell is far wider than it is tall, so `w·cos + h·sin`
        // comes out under `w`. That asymmetry is the point — it is what a
        // hand-projected axis-aligned rectangle could not reproduce, and what
        // makes this a check of the mapping rather than of "something moved".
        let (sin, cos) = (0.3_f32.sin().abs(), 0.3_f32.cos().abs());
        let expected = Rect::new(
            turned.x,
            turned.y,
            upright.width * cos + upright.height * sin,
            upright.width * sin + upright.height * cos,
        );
        assert!(
            (turned.width - expected.width).abs() < 0.01
                && (turned.height - expected.height).abs() < 0.01,
            "the turned box must be the exact bounding box of the rotated \
             cell: expected {}x{}, got {}x{}",
            expected.width,
            expected.height,
            turned.width,
            turned.height
        );
    }

    #[test]
    fn a_table_cell_follows_the_card_it_is_dragged_with() {
        // Moving a card is the one path that recomputes a descendant's box
        // *outside* a full walk: `patch_accessibility_bounds` re-places every
        // synthetic child of a widget the arena reports as moved, as
        // `owner_origin + local`. It is also what a scene does constantly, so
        // a cell that did not follow its card would be wrong most of the time.
        let (mut tree, view_id, card) = scene_with_a_table_card();

        let before = published(&mut tree);
        let at_rest = screen_rect(&before, a_cell(&before));
        assert_inside_the_card(at_rest);

        let delta = Vec2::new(40.0, 25.0);
        view_handle(&tree, view_id)
            .model()
            .set_local_pos(card, Point::new(CARD_X + delta.x, CARD_Y + delta.y));
        let after = published(&mut tree);
        let moved = screen_rect(&after, a_cell(&after));

        let expected = Rect::new(
            at_rest.x + delta.x,
            at_rest.y + delta.y,
            at_rest.width,
            at_rest.height,
        );
        assert!(
            close(moved, expected),
            "a cell moves with the card that holds it: expected {expected:?}, \
             got {moved:?}"
        );
    }

    #[test]
    fn a_text_run_inside_a_card_tracks_the_camera() {
        // The same question for the geometry every text surface publishes, not
        // just the table one. If this is wrong it predates tables.
        let (mut tree, view_id, _card) = scene_with_a_table_card();

        let run_of = |update: &TreeUpdate| -> NodeId {
            let consumer = Tree::new(update.clone(), false);
            let state = consumer.state();
            let mut stack = vec![state.root()];
            while let Some(n) = stack.pop() {
                if n.role() == Role::TextRun && n.data().value() == Some("Intro") {
                    return state
                        .locate_node(n.id())
                        .map(|(local, _)| local)
                        .expect("in this tree");
                }
                for child in n.children() {
                    stack.push(child);
                }
            }
            panic!("the editor's prose run must be published");
        };

        let before = published(&mut tree);
        let at_rest = screen_rect(&before, run_of(&before));

        view_handle(&tree, view_id).set_zoom(2.0);
        let after = published(&mut tree);
        let zoomed = screen_rect(&after, run_of(&after));

        let expected = Rect::new(
            at_rest.x * 2.0,
            at_rest.y * 2.0,
            at_rest.width * 2.0,
            at_rest.height * 2.0,
        );
        assert!(
            close(zoomed, expected),
            "a text run inside a card scales with the camera: expected \
             {expected:?}, got {zoomed:?}"
        );
    }
}
