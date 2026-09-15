// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! AUDIT PROBES — not shipped tests. Each asserts the documented contract; a
//! failure is the finding.

use super::*;
use crate::flags::ItemFlags;
use crate::items::RectItem;
use std::cell::Cell;
use std::rc::Rc;
use teksilo_canvas::Point;
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

fn tapped_scene() -> (Scene, crate::item::ItemId, Rc<Cell<u32>>) {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 120.0, 90.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    let taps = Rc::new(Cell::new(0u32));
    let t = taps.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_tap(move |_pt, _ctx| t.set(t.get() + 1));
    (scene, id, taps)
}

fn click(tree: &mut WidgetTree, at: Point) {
    tree.pointer_move(at);
    tree.dispatch_event(WidgetEvent::pointer_down(
        at,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        at,
        PointerButton::Primary,
        Modifiers::default(),
    ));
}

/// `ItemFlags::IS_VISIBLE`: "Clearing this is the equivalent of Qt's
/// `setVisible(false)` — the item is neither painted nor hit-tested."
#[test]
fn an_invisible_item_is_not_hit_tested() {
    let (mut scene, id, taps) = tapped_scene();
    scene.set_visible(id, false);
    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Painted: the doc's first half.
    assert!(
        tree.render().decorations.is_empty(),
        "an invisible item must not paint",
    );
    // Hit-tested: the doc's second half.
    click(&mut tree, Point::new(100.0, 90.0));
    assert_eq!(
        taps.get(),
        0,
        "an invisible item must not be hit-tested either",
    );
}

/// `ItemFlags::IS_ENABLED`: "Disabled items are still painted but pass clicks
/// through to items beneath."
#[test]
fn a_disabled_item_passes_clicks_through() {
    let (mut scene, id, taps) = tapped_scene();
    scene.set_flag(id, ItemFlags::IS_ENABLED, false);
    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    assert_eq!(
        tree.render().decorations.len(),
        1,
        "a disabled item is still painted",
    );
    click(&mut tree, Point::new(100.0, 90.0));
    assert_eq!(taps.get(), 0, "a disabled item must pass clicks through");
}

/// An invisible item must not be grabbable by the built-in drag handler either
/// — the draggable snapshot is the other consumer of `IS_VISIBLE`.
#[test]
fn an_invisible_item_is_not_draggable() {
    let mut scene = Scene::new();
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 120.0, 90.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    scene.set_visible(id, false);

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let from = Point::new(100.0, 90.0);
    tree.pointer_move(from);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(100.0, 200.0)));

    let view = view_handle(&tree, view_id);
    assert!(
        view.drag_target.get().is_none(),
        "an invisible item must not be grabbed by the drag handler",
    );
}

// ---------------------------------------------------------------- cull cost

#[derive(Debug)]
struct FocusableLeaf;

impl Widget for FocusableLeaf {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(teksilo_core::widget_builder::HandlerSet::new().focusable(true));
        vec![]
    }
    fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        Size::new(50.0, 50.0).into()
    }
}

/// A heavyweight widget culled to zero size: what does it still cost?
#[test]
fn a_culled_heavyweight_widget_costs_nothing() {
    let mut scene = Scene::new();
    // One inside the viewport, one far outside it.
    scene.add_widget(FocusableLeaf, Rect::new(10.0, 10.0, 50.0, 50.0));
    scene.add_widget(FocusableLeaf, Rect::new(90_000.0, 90_000.0, 50.0, 50.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let kids = tree.children(view_id);
    assert_eq!(kids.len(), 2, "both are materialised");
    assert_eq!(
        tree.bounds(kids[1]).size(),
        Size::ZERO,
        "the off-screen one is collapsed to zero size",
    );

    let stops = tree.tab_stops_within(view_id);
    println!("tab stops = {stops:?}, kids = {kids:?}, view = {view_id:?}");
    assert!(
        !stops.contains(&kids[1]),
        "a culled widget must not remain a tab stop; stops = {stops:?}, \
         culled child = {:?}",
        kids[1],
    );
}

/// …and it must not remain in the AccessKit tree either. Measured as a delta:
/// adding a card that is entirely off-screen must add nothing to the AT tree.
#[test]
fn a_culled_heavyweight_widget_is_not_in_the_at_tree() {
    let at_node_count = |off_screen: bool| {
        let mut scene = Scene::new();
        scene.add_widget(FocusableLeaf, Rect::new(10.0, 10.0, 50.0, 50.0));
        if off_screen {
            scene.add_widget(FocusableLeaf, Rect::new(90_000.0, 90_000.0, 50.0, 50.0));
        }
        let mut tree = WidgetTree::new();
        let _view_id = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.accessibility_tree_snapshot().nodes.len()
    };
    let without = at_node_count(false);
    let with = at_node_count(true);
    println!("AT nodes without = {without}, with a culled card = {with}");
    assert_eq!(
        with, without,
        "a card culled to zero size must contribute no AccessKit node",
    );
}

// ---------------------------------------------------- group drag / tier hit

/// A pointer drag of one item in a multi-selection must carry the whole
/// selection — the contract `Alt+Arrow` keeps, and the one the drag handler's
/// own comment claims ("matching pointer group-drag").
#[test]
fn a_pointer_drag_carries_the_whole_selection() {
    let mut scene = Scene::new();
    let a = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(50.0, 50.0),
    );
    let b = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0))
            .fill(teksilo_tokens::Color::BLUE)
            .draggable(true),
        Point::new(200.0, 50.0),
    );

    let mut tree = WidgetTree::new();
    let view_id =
        tree.add(SceneView::new(scene).selection_mode(crate::selection::SceneSelectionMode::Multi));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    view_handle(&tree, view_id).selection().replace([a, b]);

    // Grab `a` and drag it 100 px down.
    let from = Point::new(80.0, 80.0);
    tree.pointer_move(from);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(80.0, 180.0)));
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(80.0, 180.0),
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let model = view_handle(&tree, view_id).model().clone();
    assert_eq!(
        model.local_pos(a),
        Some(Point::new(50.0, 150.0)),
        "the grabbed item moved",
    );
    assert_eq!(
        model.local_pos(b),
        Some(Point::new(200.0, 150.0)),
        "the other selected item must move with it",
    );
}

#[derive(Debug)]
struct InertCard;

impl Widget for InertCard {
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(100.0, 100.0).into()
    }
}

/// A lightweight `Over` item painted above a heavyweight card: does a tap on
/// the overlap reach the item?
#[test]
fn an_over_item_above_a_card_receives_the_tap() {
    let mut scene = Scene::new();
    scene.add_widget(InertCard, Rect::new(50.0, 50.0, 100.0, 100.0));
    let over = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(over, crate::scene::SceneLayer::Over);
    scene.set_z(over, 100.0);
    let taps = Rc::new(Cell::new(0u32));
    let t = taps.clone();
    scene
        .handlers_mut(over)
        .unwrap()
        .on_tap(move |_pt, _ctx| t.set(t.get() + 1));

    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    click(&mut tree, Point::new(100.0, 100.0));
    assert_eq!(
        taps.get(),
        1,
        "a tap on a lightweight Over item painted above a card must reach it",
    );
}

#[derive(Debug)]
struct TappableCard(Rc<Cell<u32>>);

impl Widget for TappableCard {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        let c = self.0.clone();
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new()
                .on_tap(move |_e, _ctx| c.set(c.get() + 1)),
        );
        vec![]
    }
    fn layout_response(&self, p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
        p.resolve(100.0, 100.0).into()
    }
}

/// The converse: an *interactive* card under a lightweight `Over` item that
/// paints on top of it. Which one gets the tap?
#[test]
fn an_over_item_above_an_interactive_card_receives_the_tap() {
    let card_taps = Rc::new(Cell::new(0u32));
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    let over = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(over, crate::scene::SceneLayer::Over);
    scene.set_z(over, 100.0);
    let item_taps = Rc::new(Cell::new(0u32));
    let t = item_taps.clone();
    scene
        .handlers_mut(over)
        .unwrap()
        .on_tap(move |_pt, _ctx| t.set(t.get() + 1));

    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    click(&mut tree, Point::new(100.0, 100.0));
    println!(
        "card taps = {}, over-item taps = {}",
        card_taps.get(),
        item_taps.get()
    );
    assert_eq!(
        item_taps.get(),
        1,
        "the item painted on top must get the tap (card got {})",
        card_taps.get(),
    );
}

/// Within the lightweight tier, does the `Over` band win the hit test against
/// a higher-`z` `Under` item that it paints on top of?
#[test]
fn an_over_item_beats_a_higher_z_under_item_in_hit_test() {
    let mut scene = Scene::new();
    let under = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(50.0, 50.0),
    );
    scene.set_z(under, 100.0); // painted last within Under…
    let over = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    scene.set_layer(over, crate::scene::SceneLayer::Over); // …but Over paints later still
    scene.set_z(over, 0.0);

    let under_taps = Rc::new(Cell::new(0u32));
    let over_taps = Rc::new(Cell::new(0u32));
    let u = under_taps.clone();
    scene
        .handlers_mut(under)
        .unwrap()
        .on_tap(move |_pt, _ctx| u.set(u.get() + 1));
    let o = over_taps.clone();
    scene
        .handlers_mut(over)
        .unwrap()
        .on_tap(move |_pt, _ctx| o.set(o.get() + 1));

    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    click(&mut tree, Point::new(100.0, 100.0));
    println!(
        "under taps = {}, over taps = {}",
        under_taps.get(),
        over_taps.get()
    );
    assert_eq!(
        over_taps.get(),
        1,
        "the Over item is the one the user sees; it must take the tap \
         (Under got {})",
        under_taps.get(),
    );
}

/// Control for the two probes above: with no lightweight item in the way, the
/// card's own `on_tap` fires. If this passes and
/// `an_over_item_above_an_interactive_card_receives_the_tap` also passes, the
/// lightweight tier really is stealing the tap from the card.
#[test]
fn control_a_bare_card_receives_its_own_tap() {
    let card_taps = Rc::new(Cell::new(0u32));
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );

    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    click(&mut tree, Point::new(100.0, 100.0));
    assert_eq!(card_taps.get(), 1, "a bare card gets its own tap");
}

/// And the same card with a lightweight item that has NO handlers sitting over
/// it: the item must not absorb the card's tap.
#[test]
fn control_a_handlerless_item_does_not_absorb_a_cards_tap() {
    let card_taps = Rc::new(Cell::new(0u32));
    let mut scene = Scene::new();
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );
    let over = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(80.0, 80.0),
    );
    scene.set_layer(over, crate::scene::SceneLayer::Over);

    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    click(&mut tree, Point::new(100.0, 100.0));
    assert_eq!(
        card_taps.get(),
        1,
        "a decorative lightweight item must not swallow the card's tap",
    );
}

/// The strongest form: a lightweight item in the **Under** band — i.e. one the
/// user sees painted *behind* the card — with a tap handler. The card is
/// visibly on top. Who gets the tap?
#[test]
fn a_card_beats_a_handler_item_painted_behind_it() {
    let card_taps = Rc::new(Cell::new(0u32));
    let mut scene = Scene::new();
    // Added first, Under band (the default): painted behind the card.
    let under = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(20.0, 20.0),
    );
    let item_taps = Rc::new(Cell::new(0u32));
    let t = item_taps.clone();
    scene
        .handlers_mut(under)
        .unwrap()
        .on_tap(move |_pt, _ctx| t.set(t.get() + 1));
    scene.add_widget(
        TappableCard(card_taps.clone()),
        Rect::new(50.0, 50.0, 100.0, 100.0),
    );

    let mut tree = WidgetTree::new();
    let _view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    click(&mut tree, Point::new(100.0, 100.0));
    println!(
        "card taps = {}, behind-item taps = {}",
        card_taps.get(),
        item_taps.get()
    );
    assert_eq!(
        card_taps.get(),
        1,
        "the card is painted on top and must get the tap; the item behind it \
         got {}",
        item_taps.get(),
    );
}
