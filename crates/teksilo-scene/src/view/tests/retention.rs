// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Heavyweight retention: which cards a view keeps live, and what it costs
//! when it does not.
//!
//! The mechanism under test is one sentence: a materialised card outside the
//! *retention region* is parked dormant, so it leaves paint, the layout
//! recursion, the AccessKit tree and the Tab ring while keeping its state.
//! Everything here pins one of the three ways that can go wrong —
//!
//! * it parks something it must not (focus, a live pointer, a card the
//!   accessibility mode promised to enumerate);
//! * it publishes an AccessKit child that is not in the tree, which breaks the
//!   walk rather than merely losing a link;
//! * it fails to bring a card back, or brings it back a frame late.
//!
//! Every assertion here is checked against the *consumer* where the claim is
//! about assistive technology: `accesskit_consumer::Tree` runs the same
//! validation each platform adapter runs on activation, so a tree it rejects
//! is one VoiceOver / NVDA / Orca would reject.

use super::*;
use crate::a11y::{A11yGroup, A11yNode, A11yOffScreenMode, A11yRelation};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

/// A card with something in it worth focusing — the shape that makes the
/// difference between "a widget" and "state a user would lose".
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
        Size::new(50.0, 50.0).into()
    }
}

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(400.0),
    height: Some(300.0),
};

/// Far enough out that no region any mode produces can reach it.
const FAR: f32 = 90_000.0;

/// The tree as an assistive technology actually receives it.
///
/// Deliberately `sync_accessibility` and not `accessibility_tree_snapshot`:
/// the snapshot calls the walker directly and so answers "what would a walk
/// produce", while every platform adapter is fed `sync_accessibility`, which
/// returns the *cached* tree unless something set `a11y_dirty` and only
/// re-places its bounds. A claim about what a screen reader sees that is
/// measured through the snapshot cannot see a missing invalidation — which is
/// exactly the defect a wake-only layout pass used to have.
fn published(tree: &mut WidgetTree) -> accesskit::TreeUpdate {
    tree.sync_accessibility()
}

fn at_node_count(tree: &mut WidgetTree) -> usize {
    published(tree).nodes.len()
}

/// Whether the published tree contains a node for this widget.
fn is_published(tree: &mut WidgetTree, id: WidgetId) -> bool {
    let node = teksilo_core::accessibility::widget_id_to_node_id(id);
    published(tree).nodes.iter().any(|(nid, _)| *nid == node)
}

/// Feed the update through the consumer every platform adapter uses. Panics on
/// a dangling child, an orphaned node, a duplicate child or an invalid focus —
/// i.e. turns a crash inside VoiceOver into a red test here.
fn assert_tree_valid(tree: &mut WidgetTree) {
    let update = published(tree);
    accesskit_consumer::Tree::new(update, false);
}

// ─────────────────────────────────────────────────── the basic contract

#[test]
fn an_off_screen_card_leaves_the_tab_ring_and_the_at_tree() {
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(Card, Rect::new(FAR, FAR, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(
        !tree.tab_stops_within(view_id).contains(&far_widget),
        "an off-screen card must leave the Tab ring"
    );
    assert!(
        !tree.is_active(far_widget),
        "…by being parked, not by some other route"
    );
    assert_tree_valid(&mut tree);
}

#[test]
fn a_card_inside_the_viewport_is_untouched() {
    // The control for the test above: retention must not be a blanket "park
    // heavyweight children", or it would pass for the wrong reason.
    let mut scene = Scene::new();
    let near = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let near_widget = view_handle(&tree, view_id).widget_id_for(near).unwrap();
    assert!(tree.is_active(near_widget));
    assert!(tree.tab_stops_within(view_id).contains(&near_widget));
    assert_eq!(
        tree.bounds(near_widget).size(),
        Size::new(50.0, 50.0),
        "and it is laid out at full size"
    );
}

#[test]
fn panning_a_card_back_wakes_it_in_the_same_pass() {
    // The user-visible half: a camera that jumps must not show a hole for a
    // frame. The gate is written during layout, so settling it needs the
    // post-layout sweep in `WidgetTree::layout_with_ops` — delete that sweep
    // and this reddens while every "it went away" test still passes.
    let mut scene = Scene::new();
    let far = scene.add_widget(Card, Rect::new(5_000.0, 0.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(!tree.is_active(far_widget), "parked while off screen");

    view_handle(&tree, view_id).set_pan(Vec2::new(-5_000.0, 0.0));
    tree.layout(VIEWPORT);

    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(tree.is_active(far_widget), "woken by the pan");
    assert_eq!(
        tree.bounds(far_widget).size(),
        Size::new(50.0, 50.0),
        "and laid out this pass, not the next — a woken card with stale \
         bounds paints at the wrong rectangle for a frame"
    );
    assert!(tree.tab_stops_within(view_id).contains(&far_widget));
}

#[test]
fn the_retention_margin_wakes_a_card_before_it_is_visible() {
    // What the margin is for: the card is still outside the viewport (so the
    // tight cull keeps it at zero size) but inside the retention region, so it
    // is already live and ready by the time the next pan sample shows it.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(0.0, 0.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    // ViewportOnly so the accessibility region is the bare viewport and the
    // margin is the only thing keeping anything alive outside it.
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .retention_margin(96.0),
    );
    tree.layout(VIEWPORT);

    // Push the card 50 px past the left edge: outside the viewport, inside the
    // 96 px margin.
    view_handle(&tree, view_id).set_pan(Vec2::new(-100.0, 0.0));
    tree.layout(VIEWPORT);

    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    assert!(
        tree.is_active(widget),
        "a card within the margin stays live"
    );
    assert_eq!(
        tree.bounds(widget).size(),
        Size::ZERO,
        "…while the tight cull still collapses it — two regions, layered"
    );

    // And past the margin it goes.
    view_handle(&tree, view_id).set_pan(Vec2::new(-400.0, 0.0));
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    assert!(!tree.is_active(widget), "past the margin it parks");
}

#[test]
fn a_zero_margin_parks_at_the_viewport_edge() {
    // Pins that the margin is the knob and not a constant hiding in the code:
    // the same geometry as the test above, decided the other way.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(0.0, 0.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .retention_margin(0.0),
    );
    tree.layout(VIEWPORT);
    view_handle(&tree, view_id).set_pan(Vec2::new(-100.0, 0.0));
    tree.layout(VIEWPORT);

    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    assert!(!tree.is_active(widget));
}

#[test]
fn the_margin_is_a_screen_distance_not_a_scene_distance() {
    // Zoomed out 10×, 96 screen px is 960 scene units. A scene-unit margin
    // would stop covering the screen distance a pan crosses in one frame
    // exactly when the view is showing the most content.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(0.0, 0.0, 10.0, 10.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .min_zoom(0.05)
            .retention_margin(96.0),
    );
    tree.layout(VIEWPORT);
    let view = view_handle(&tree, view_id);
    view.set_zoom(0.1);
    // 500 scene units left of the viewport: 50 screen px at this zoom, so
    // inside a screen-pixel margin and far outside a scene-unit one.
    view.set_pan(Vec2::new(-50.0, 0.0));
    tree.layout(VIEWPORT);

    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    assert!(
        tree.is_active(widget),
        "96 screen px is 960 scene units at zoom 0.1"
    );
}

// ────────────────────────────────────────────────────── B2: the pins

#[test]
fn panning_away_from_a_focused_card_does_not_destroy_focus() {
    // The defect this exists to prevent, in the user's words: type in a card,
    // pan slightly, and the caret is gone with no way back but Tab from the
    // top. Parking the card would clear focus (the dispatcher rejects
    // inactive targets) with no restore path, because `pending_focus_restore`
    // is scoped to rebuilds.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly));
    tree.layout(VIEWPORT);

    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    tree.focus(widget);
    assert_eq!(tree.focused(), Some(widget));

    // Pan the card far out of every region.
    view_handle(&tree, view_id).set_pan(Vec2::new(-FAR, -FAR));
    tree.layout(VIEWPORT);

    assert_eq!(
        tree.focused(),
        Some(widget),
        "the focused card is pinned wherever the camera goes"
    );
    assert!(tree.is_active(widget), "…which means it stays live");
    // And it must still be the AccessKit focus target, which requires the node
    // to be in the tree at all.
    assert_tree_valid(&mut tree);
}

#[test]
fn a_focused_card_is_still_culled_to_zero_size() {
    // The pin is about existence, not geometry: a pinned card off screen must
    // not start costing a full layout of its subtree. Two regions, layered.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    tree.focus(widget);

    view_handle(&tree, view_id).set_pan(Vec2::new(-FAR, -FAR));
    tree.layout(VIEWPORT);

    assert!(tree.is_active(widget));
    assert_eq!(tree.bounds(widget).size(), Size::ZERO);
}

#[test]
fn panning_during_a_press_inside_a_card_does_not_cancel_it() {
    // `park_subtree_with_ops` cancels every pointer in the subtree with
    // `CancelReason::SubtreeParked`. A user drag-selecting inside an embedded
    // text surface, on a view someone else (or a second finger) is panning,
    // would have that interaction taken away because the *view* moved.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly));
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();

    // Press inside the card so it takes the press and captures the pointer.
    let at = Point::new(30.0, 30.0);
    tree.pointer_move(at);
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    assert!(
        tree.is_pressed(widget),
        "precondition: the card owns the press"
    );

    view_handle(&tree, view_id).set_pan(Vec2::new(-FAR, -FAR));
    tree.layout(VIEWPORT);

    assert!(
        tree.is_active(widget),
        "a card holding a live pointer is pinned; parking it would cancel the \
         user's interaction because the camera moved"
    );
    assert!(
        tree.is_pressed(widget),
        "…and the press survives the pan rather than being cancelled"
    );
}

// ───────────────────────────────────────── B1: the dangling AccessKit child

#[test]
fn a_logically_reparented_card_that_parks_leaves_no_dangling_child() {
    // The blocker, exactly. A heavyweight card with a declared logical a11y
    // parent is published as `attach_scene_child_under(parent, widget_node)`,
    // and that call takes the caller's word for it. Park the card and the
    // group would name a child that is not in the update — which is not a lost
    // link but a broken walk, since everything below an unresolvable child is
    // unreachable.
    let mut scene = Scene::new();
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("Board")));
    let near = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(Card, Rect::new(FAR, FAR, 50.0, 50.0));
    scene.set_a11y_parent(A11yNode::Item(near), Some(A11yNode::Group(group)));
    scene.set_a11y_parent(A11yNode::Item(far), Some(A11yNode::Group(group)));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(
        !tree.is_active(far_widget),
        "precondition: the far card parked"
    );

    // The consumer rejects a dangling child. Without the fix this panics.
    assert_tree_valid(&mut tree);

    let update = published(&mut tree);
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    let far_node = teksilo_core::accessibility::widget_id_to_node_id(far_widget);
    assert!(
        !emitted.contains(&far_node),
        "the parked card emits nothing"
    );
    for (parent, node) in &update.nodes {
        for child in node.children() {
            assert!(
                emitted.contains(child),
                "node {parent:?} names child {child:?}, absent from the tree"
            );
        }
    }
}

#[test]
fn a_relation_pointing_at_a_parked_card_does_not_dangle() {
    // The third of the four sites: the `resolve()` closure feeding the
    // relation / live-region / landmark passes maps `A11yNode::Widget(id)`
    // straight to a NodeId with no idea whether that widget is running.
    // `labelled_by` is the one that costs a name rather than a link — the
    // consumer concatenates its targets' values and unwraps the lookup.
    let mut scene = Scene::new();
    let near = scene.add_item(
        crate::items::RectItem::new(Rect::new(0.0, 0.0, 30.0, 30.0))
            .fill(teksilo_tokens::Color::RED),
        Point::new(10.0, 10.0),
    );
    let far = scene.add_widget(Card, Rect::new(FAR, FAR, 50.0, 50.0));
    scene.add_a11y_relation(
        A11yNode::Item(near),
        A11yRelation::LabelledBy,
        A11yNode::Item(far),
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);
    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(!tree.is_active(far_widget));

    assert_tree_valid(&mut tree);
    let update = published(&mut tree);
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    for (from, node) in &update.nodes {
        for target in node.labelled_by() {
            assert!(
                emitted.contains(target),
                "node {from:?} is labelled_by {target:?}, absent from the tree"
            );
        }
    }
}

#[test]
fn a_live_region_on_a_parked_card_announces_nothing_and_breaks_nothing() {
    // The fourth site. A live region and a landmark declared on a card that is
    // now parked must not leave a decoration pointing at a node nobody
    // emitted.
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(Card, Rect::new(FAR, FAR, 50.0, 50.0));
    scene.set_a11y_live(A11yNode::Item(far), accesskit::Live::Polite);
    scene.set_a11y_landmark(A11yNode::Item(far), accesskit::Role::Complementary);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    assert_tree_valid(&mut tree);
    let _ = view_id;
}

// ─────────────────────────────────── B3: retention ⊇ the accessibility region

#[test]
fn all_items_mode_parks_nothing() {
    // `A11yOffScreenMode::AllItems` documents itself as "a complete table of
    // contents". A retention region that parked a card would silently break
    // that promise for the heavyweight tier — which is the half of the promise
    // that was already broken in the other direction (§1.7).
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(Card, Rect::new(FAR, FAR, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).a11y_off_screen_mode(A11yOffScreenMode::AllItems));
    tree.layout(VIEWPORT);

    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(tree.is_active(far_widget), "AllItems keeps every card live");
    assert!(tree.tab_stops_within(view_id).contains(&far_widget));
}

#[test]
fn the_retention_region_contains_the_accessibility_region() {
    // The load-bearing containment, asserted as geometry rather than inferred
    // from a behaviour: if it ever fails, the accessibility walk describes a
    // card the arena has parked, and the published tree names a node that is
    // not in it.
    let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
    let tight = SceneView::new(Scene::new()).visible_scene_region(bounds);
    for mode in [
        A11yOffScreenMode::ViewportOnly,
        A11yOffScreenMode::ViewportPlusN { n: 0 },
        A11yOffScreenMode::ViewportPlusN { n: 1 },
        A11yOffScreenMode::ViewportPlusN { n: 3 },
    ] {
        // Margin zero, so the containment is proved by the union and not
        // hidden by a generous margin.
        let view = SceneView::new(Scene::new())
            .a11y_off_screen_mode(mode)
            .retention_margin(0.0);
        let at = mode.at_visible_region(tight).expect("bounded mode");
        let retention = view
            .retention_scene_region(bounds)
            .expect("bounded mode retains a region");
        assert!(
            contains_rect(retention, at),
            "{mode:?}: retention {retention:?} must contain the a11y region {at:?}"
        );
        assert!(
            contains_rect(retention, tight),
            "{mode:?}: retention {retention:?} must contain the viewport {tight:?}"
        );
    }
}

fn contains_rect(outer: Rect, inner: Rect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

// ─────────────────────────────────────────────────────────── the cost

#[test]
fn the_at_tree_and_the_tab_ring_do_not_grow_with_the_off_screen_tail() {
    // §1.7's claim, measured as a count rather than a duration: a count is
    // exact, and the defect it pins ("ViewportOnly on a 10 000-card scene
    // still publishes 10 000 AT nodes and 10 000 tab stops") is about
    // membership, not speed.
    let build = |tail: usize| {
        let mut scene = Scene::new();
        for i in 0..4 {
            scene.add_widget(Card, Rect::new(10.0 + i as f32 * 60.0, 10.0, 50.0, 50.0));
        }
        for i in 0..tail {
            scene.add_widget(Card, Rect::new(FAR + i as f32 * 60.0, FAR, 50.0, 50.0));
        }
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::new(scene));
        tree.layout(VIEWPORT);
        (
            at_node_count(&mut tree),
            tree.tab_stops_within(view_id).len(),
        )
    };
    let base = build(0);
    for tail in [1, 50, 500] {
        assert_eq!(
            build(tail),
            base,
            "a tail of {tail} off-screen cards must add no AT node and no tab stop"
        );
    }
}

#[test]
fn a_parked_card_keeps_its_state_and_gets_it_back() {
    // Dormant, not destroyed: the whole reason to park rather than reap. The
    // card's arena node, and therefore everything it owns, survives the round
    // trip.
    let mut scene = Scene::new();
    let card = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly));
    tree.layout(VIEWPORT);
    let before = view_handle(&tree, view_id).widget_id_for(card).unwrap();

    view_handle(&tree, view_id).set_pan(Vec2::new(-FAR, 0.0));
    tree.layout(VIEWPORT);
    assert!(!tree.is_active(before));

    view_handle(&tree, view_id).set_pan(Vec2::new(0.0, 0.0));
    tree.layout(VIEWPORT);
    let after = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    assert_eq!(
        before, after,
        "the same arena node comes back — a fresh id would mean the card was \
         destroyed and rebuilt, losing its focus, text and animation state"
    );
    assert!(tree.is_active(after));
}

#[test]
fn a_pinned_card_outside_the_accessibility_region_is_still_grafted() {
    // The other direction of the same coupling, and the reason the walk reads
    // the *retained* set rather than re-deriving the accessibility region.
    //
    // A focused card is pinned wherever the camera goes, so it stays active
    // and the framework walker emits its node. Its logical parent is declared,
    // so `a11y_redirect_descendant` claims it and the walker does NOT push it
    // as a natural child. If the scene then skipped the graft — because the
    // card is outside the accessibility region — the node would be in the
    // update with nothing pointing at it: an orphan, which the consumer
    // rejects just as hard as a dangling child.
    let mut scene = Scene::new();
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("Board")));
    let card = scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    scene.set_a11y_parent(A11yNode::Item(card), Some(A11yNode::Group(group)));

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly));
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(card).unwrap();
    tree.focus(widget);

    view_handle(&tree, view_id).set_pan(Vec2::new(-FAR, -FAR));
    tree.layout(VIEWPORT);
    assert!(tree.is_active(widget), "precondition: focus pins it");

    let update = published(&mut tree);
    let node = teksilo_core::accessibility::widget_id_to_node_id(widget);
    assert!(
        update.nodes.iter().any(|(id, _)| *id == node),
        "the pinned card emits a node"
    );
    assert!(
        update
            .nodes
            .iter()
            .any(|(_, n)| n.children().contains(&node)),
        "…and something must name it, or it is unreachable"
    );
    assert_tree_valid(&mut tree);
}

/// A card that hands its inner widget's arena id back to the test, so the test
/// can address a widget *inside* a card — which is the shape
/// `set_a11y_parent(A11yNode::Widget(..), ..)` exists for.
#[derive(Debug)]
struct CardWithInner(std::rc::Rc<std::cell::Cell<Option<WidgetId>>>);

impl Widget for CardWithInner {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let inner = ctx.add(Card);
        self.0.set(Some(inner));
        vec![inner]
    }
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(50.0, 50.0).into()
    }
    fn children(&self) -> Vec<WidgetId> {
        self.0.get().into_iter().collect()
    }
}

#[test]
fn a_widget_inside_a_parked_card_leaves_no_dangling_child() {
    // The site the scene cannot guard on its own, and therefore the one the
    // framework-level strip is load-bearing for.
    //
    // `set_a11y_parent(A11yNode::Widget(w), …)` relocates a widget addressed
    // by its *arena* id — typically a descendant of a card, not the card root.
    // `SceneView::accessibility` has no arena, and `widget_to_item` knows only
    // card roots, so it cannot tell that `w`'s activation follows an ancestor
    // it has just parked. It grafts the node either way; nothing emits it; and
    // `WidgetTree::sync_accessibility`'s dangling-child strip is what keeps the
    // published tree walkable.
    //
    // Delete that strip and this reddens while every scene-side guard — which
    // covers the card-root address — still passes.
    let inner_slot: std::rc::Rc<std::cell::Cell<Option<WidgetId>>> = Default::default();
    let mut scene = Scene::new();
    let group = scene.add_a11y_group(A11yGroup::builder().label(lit!("Board")));
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(
        CardWithInner(inner_slot.clone()),
        Rect::new(FAR, FAR, 50.0, 50.0),
    );

    let model = SceneModel::from_scene(scene);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::with_model(model.clone()));
    tree.layout(VIEWPORT);

    // The card is parked, and its inner widget went with it.
    let far_widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(!tree.is_active(far_widget), "precondition: the card parked");
    let inner = inner_slot.get().expect("the card built its inner widget");
    assert!(
        !tree.is_active(inner),
        "precondition: the descendant went dormant with it"
    );

    // Now declare the relocation, addressed by the descendant's arena id.
    model.set_a11y_parent(A11yNode::Widget(inner), Some(A11yNode::Group(group)));
    tree.layout(VIEWPORT);

    let update = published(&mut tree);
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    let inner_node = teksilo_core::accessibility::widget_id_to_node_id(inner);
    assert!(
        !emitted.contains(&inner_node),
        "a widget inside a parked card emits nothing"
    );
    for (parent, node) in &update.nodes {
        assert!(
            !node.children().contains(&inner_node),
            "node {parent:?} still names the parked descendant {inner_node:?}"
        );
        for child in node.children() {
            assert!(
                emitted.contains(child),
                "node {parent:?} names child {child:?}, absent from the tree"
            );
        }
    }
    assert_tree_valid(&mut tree);
}

// ──────────────────────────────── waking is half of the transition

/// A card that records every activation transition the framework hands it —
/// the hook a `WebView` hangs its native subview's visibility on.
#[derive(Debug)]
struct WatchedCard(std::rc::Rc<std::cell::RefCell<Vec<bool>>>);

impl Widget for WatchedCard {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let log = self.0.clone();
        let active = ctx.activation_signal(self_id);
        ctx.effect(&active, move |v| log.borrow_mut().push(*v));
        ctx.apply_self_handlers(teksilo_core::widget_builder::HandlerSet::new().focusable(true));
        vec![]
    }
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(50.0, 50.0).into()
    }
}

#[test]
fn a_wake_that_resizes_nothing_still_republishes_the_tree() {
    // `sync_accessibility` returns the cached tree unless something set
    // `a11y_dirty`, and that is the function every platform adapter is fed. A
    // pass that only *wakes* used to set nothing: the settle step ran inside
    // `if !to_park.is_empty()`, so waking was half a transition.
    //
    // It hid because the obvious probe pans a card into view, where it goes
    // `Size::ZERO` → 50×50 and the resize dirties the cache on its own. Here
    // the card wakes into the margin band and stays zero-sized, so nothing
    // else can stand in.
    //
    // Measured through `sync_accessibility` on purpose:
    // `accessibility_tree_snapshot` bypasses the cache entirely and reports
    // the card either way.
    let mut scene = Scene::new();
    // The control card sits where BOTH viewports reach it (x ∈ [0, 400] before
    // the pan, [250, 650] after), so it neither collapses nor grows across the
    // pass under test. Put it at the origin instead and the pan collapses it to
    // `Size::ZERO`, and *that* resize dirties the cache — the test would then
    // pass for a reason that has nothing to do with the wake.
    scene.add_widget(Card, Rect::new(300.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(Card, Rect::new(1_000.0, 10.0, 50.0, 50.0));
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);

    let widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(!tree.is_active(widget), "precondition: parked at pan 0");
    // Publish once, so the cache is populated and clean.
    assert!(!is_published(&mut tree, widget));

    // Inside `ViewportPlusN { n: 1 }`'s region (x ≤ 1050) and outside the
    // tight viewport (x ≥ 650) — woken, still collapsed.
    view_handle(&tree, view_id).set_pan(Vec2::new(-250.0, 0.0));
    tree.layout(VIEWPORT);
    assert!(tree.is_active(widget), "precondition: the pan woke it");
    assert_eq!(
        tree.bounds(widget).size(),
        Size::ZERO,
        "precondition: and did not resize it, so only the wake can dirty the cache"
    );
    assert!(
        tree.accessibility_tree_snapshot()
            .nodes
            .iter()
            .any(|(nid, _)| *nid == teksilo_core::accessibility::widget_id_to_node_id(widget)),
        "precondition: a fresh walk describes the woken card"
    );

    assert!(
        is_published(&mut tree, widget),
        "…and so must the tree an assistive technology is handed"
    );
    assert_tree_valid(&mut tree);
}

#[test]
fn a_wake_fires_the_activation_signal_it_queued() {
    // `arena.activate` only *queues* the `(id, true)` transition;
    // `flush_activation_signals` drains it. With the drain inside the parking
    // branch, a pass that woke a card and parked none left the `true` in the
    // queue until some later pass happened to park something.
    //
    // The named consumer is `teksilo-webview`'s dormancy → `set_visible`
    // bridge: its subview is outside the wgpu surface, so this signal is the
    // only thing that can hide and re-show it. A `true` that never arrives is
    // an embedded page that goes away when the camera leaves and does not come
    // back.
    let log: std::rc::Rc<std::cell::RefCell<Vec<bool>>> = Default::default();
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let far = scene.add_widget(
        WatchedCard(log.clone()),
        Rect::new(1_000.0, 10.0, 50.0, 50.0),
    );

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(far).unwrap();
    assert!(!tree.is_active(widget), "precondition: parked");
    assert_eq!(
        log.borrow().as_slice(),
        &[false],
        "the park half already worked"
    );

    // Wake it without resizing it, so no other pass can be doing the work.
    view_handle(&tree, view_id).set_pan(Vec2::new(-250.0, 0.0));
    tree.layout(VIEWPORT);
    assert!(tree.is_active(widget), "precondition: woken");
    assert_eq!(
        log.borrow().as_slice(),
        &[false, true],
        "the wake must reach the observer in the pass that woke it"
    );
}

// ──────────────────────── the margin is a lifecycle knob, not an a11y one

#[test]
fn the_retention_margin_does_not_widen_what_assistive_tech_is_offered() {
    // Two knobs, two questions. `retention_margin` keeps a card warm so the
    // camera never reaches a hole; `a11y_off_screen_mode` states how much of
    // an off-screen scene a screen reader should be offered. Coupling them
    // made the first silently override the second — `ViewportOnly` documents
    // itself as strictly the current viewport, and a generous margin published
    // a band beyond it anyway.
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    // 40 px past the right edge of a 400 px viewport: well inside a 400 px
    // margin, well outside the viewport.
    let outside = scene.add_widget(Card, Rect::new(440.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .retention_margin(400.0),
    );
    tree.layout(VIEWPORT);

    let widget = view_handle(&tree, view_id).widget_id_for(outside).unwrap();
    assert!(
        tree.is_active(widget),
        "the margin still keeps it alive — that is its whole job"
    );
    assert!(
        tree.tab_stops_within(view_id).contains(&widget),
        "…and alive means reachable; this is the honest cost of the split"
    );
    assert!(
        !is_published(&mut tree, widget),
        "…but `ViewportOnly` said the viewport, so it is not offered to AT"
    );
    assert_tree_valid(&mut tree);
}

#[test]
fn a_card_the_mode_does_not_reach_is_neither_named_nor_emitted() {
    // *How* the suppression is done matters as much as that it is done. The
    // walker uses `accessibility_children` for both the child push and the
    // recursion, so an omitted card leaves no node and no reference to one.
    // Suppressing through the redirect hook instead would emit the node and
    // leave nothing pointing at it — an orphan, which the consumer rejects
    // just as hard as a dangling child.
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let outside = scene.add_widget(Card, Rect::new(440.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .retention_margin(400.0),
    );
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(outside).unwrap();
    let node = teksilo_core::accessibility::widget_id_to_node_id(widget);

    let update = published(&mut tree);
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    assert!(!emitted.contains(&node), "no node for the unlisted card");
    for (parent, n) in &update.nodes {
        assert!(
            !n.children().contains(&node),
            "node {parent:?} still names the unlisted card"
        );
        for child in n.children() {
            assert!(
                emitted.contains(child),
                "node {parent:?} names child {child:?}, absent from the tree"
            );
        }
    }
    assert_tree_valid(&mut tree);
}

#[test]
fn focus_on_a_card_the_mode_does_not_reach_publishes_it() {
    // The exception that keeps the split safe. A published tree names its
    // focused node, and a focus the walk did not emit is a broken update, not
    // a missing one. So a card the user is in the middle of is pinned into the
    // published set wherever the camera is — the same pin that keeps it alive.
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let outside = scene.add_widget(Card, Rect::new(440.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .retention_margin(400.0),
    );
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(outside).unwrap();
    assert!(
        !is_published(&mut tree, widget),
        "precondition: unlisted while nobody is using it"
    );

    tree.focus(widget);
    tree.layout(VIEWPORT);
    assert!(
        is_published(&mut tree, widget),
        "focus pins it into the published tree"
    );
    let update = published(&mut tree);
    assert_eq!(
        update.focus,
        teksilo_core::accessibility::widget_id_to_node_id(widget),
        "…as itself, not as an ancestor stood in for it"
    );
    assert_tree_valid(&mut tree);
}

#[test]
fn a_sync_between_the_focus_and_the_pass_does_not_lose_the_card() {
    // The same claim as the test above, with the one interleaving that breaks
    // a naive invalidation. `WidgetTree::focus` dirties the accessibility cache
    // for its own reasons, and a `sync_accessibility` before the layout pass
    // *consumes* that flag while `at_children` is still the old, narrower set.
    // The pass that then pins the card parks and wakes nothing, so a park/wake
    // invalidation sees no reason to dirty anything, and the cached tree served
    // afterwards omits the focused card and names an ancestor as its focus —
    // a broken update rather than a missing node.
    //
    // The ordering is not exotic: any app that renders a frame between the
    // click that moves focus and the next layout produces it.
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let outside = scene.add_widget(Card, Rect::new(440.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::new(scene)
            .a11y_off_screen_mode(A11yOffScreenMode::ViewportOnly)
            .retention_margin(400.0),
    );
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(outside).unwrap();
    assert!(
        !is_published(&mut tree, widget),
        "precondition: unlisted while nobody is using it"
    );

    tree.focus(widget);
    // The interleaving. Drains the flag `focus` set, before the set widens.
    let _ = published(&mut tree);
    tree.layout(VIEWPORT);

    assert!(
        is_published(&mut tree, widget),
        "a sync between the focus and the pass must not cost the card its node"
    );
    let update = published(&mut tree);
    assert_eq!(
        update.focus,
        teksilo_core::accessibility::widget_id_to_node_id(widget),
        "…and the update must still name the card, not an ancestor"
    );
    assert_tree_valid(&mut tree);
}

#[test]
fn a_mode_that_reaches_further_than_the_margin_publishes_the_whole_band() {
    // The control for the three tests above, and the reason the coupling is
    // one-directional: where the mode's region is the wider of the two, every
    // card it reaches is published. The default mode is this case — a
    // one-screen margin dwarfs the 96 px default — so the split changes
    // nothing for an app that never touches either knob.
    let mut scene = Scene::new();
    scene.add_widget(Card, Rect::new(10.0, 10.0, 50.0, 50.0));
    let outside = scene.add_widget(Card, Rect::new(440.0, 10.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(VIEWPORT);
    let widget = view_handle(&tree, view_id).widget_id_for(outside).unwrap();
    assert!(tree.is_active(widget));
    assert!(
        is_published(&mut tree, widget),
        "`ViewportPlusN {{ n: 1 }}` reaches 400 px past the edge and says so"
    );
}
