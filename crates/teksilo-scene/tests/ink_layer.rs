// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The four things an app writing an ink tool cannot work around.
//!
//! 1. **Every sample reaches the tool.** A pen batch the platform folded into
//!    one `PointerSample` must still deliver every position, with the pressure
//!    each was sampled at, to a handler on the view — and the handler an ink
//!    tool would naively reach for, `on_drag`, is not the one to use.
//! 2. **Ink between two notes.** A dried stroke must be able to sit above one
//!    heavyweight card and below another — the OneNote case, which the old
//!    two-band `SceneLayer` could not express at all.
//! 3. **A wet surface that repaints on its own**, without dragging the scene's
//!    item bands through the pass with it.
//! 4. **Accessibility does not move.** The crate's differentiator is a per-item
//!    AT node; a paint band is not allowed to change what a screen reader is
//!    told, and the assertions go through `sync_accessibility` — the cached
//!    door a platform adapter is handed — rather than the uncached snapshot.
//!
//! Run with: cargo test -p teksilo-scene --test ink_layer

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{DrawCommand, Point, Rect, Size, SizeProposal};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::{
    CoalescedSample, EventResponse, EventTime, LayoutContext, LayoutResponse, PaintContext,
    PointerAxes, PointerInfo, PointerPhase, PointerSample, Widget, WidgetTree,
};
use teksilo_scene::{RectItem, Scene, SceneItemHandlerSet, SceneLayer, SceneView, WetLayer};
use teksilo_tokens::Color;

const CARD_LOW: Color = Color::new(0.90, 0.10, 0.10, 1.0);
const CARD_HIGH: Color = Color::new(0.10, 0.90, 0.10, 1.0);
const INK: Color = Color::new(0.10, 0.10, 0.90, 1.0);
const WET: Color = Color::new(0.95, 0.95, 0.10, 1.0);

/// A heavyweight card: a widget that paints one solid colour.
#[derive(Debug)]
struct Card(Color);

impl Widget for Card {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(60.0, 60.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        canvas.fill_rect(bounds, self.0);
    }
}

/// Every solid fill colour in the frame, in the order the renderer draws them.
fn painted_colours(frame: &teksilo_canvas::RenderFrame) -> Vec<[f32; 4]> {
    frame
        .draw_order
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCommand::Decoration(i) => Some(frame.decorations[*i].color),
            _ => None,
        })
        .collect()
}

fn index_of(colours: &[[f32; 4]], c: Color) -> Option<usize> {
    colours.iter().position(|x| *x == c.to_array())
}

fn pressure(p: f32) -> PointerAxes {
    let mut a = PointerAxes::default();
    a.pressure = Some(p);
    a
}

// ---------------------------------------------------------------------------
// 2. Ink between two notes
// ---------------------------------------------------------------------------

/// The case the two-band model could not express: a lightweight stroke over one
/// card and under another.
///
/// Set the stroke's layer back to `Under` and the first assertion reddens; set
/// it to `Over` and the second does. There is no third answer in the old model,
/// which is the finding this test exists to pin.
#[test]
fn an_interleaved_item_paints_between_two_cards() {
    let mut scene = Scene::new();
    let low = scene.add_widget(Card(CARD_LOW), Rect::new(0.0, 0.0, 60.0, 60.0));
    let high = scene.add_widget(Card(CARD_HIGH), Rect::new(80.0, 0.0, 60.0, 60.0));
    scene.set_z(low, 0.0);
    scene.set_z(high, 20.0);

    let ink = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 40.0)).fill(INK),
        Point::new(0.0, 10.0),
    );
    scene.set_layer(ink, SceneLayer::Interleaved);
    scene.set_z(ink, 10.0);

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    let colours = painted_colours(&frame);

    let low_at = index_of(&colours, CARD_LOW).expect("the z=0 card painted");
    let ink_at = index_of(&colours, INK).expect("the interleaved stroke painted");
    let high_at = index_of(&colours, CARD_HIGH).expect("the z=20 card painted");

    assert!(
        low_at < ink_at,
        "the stroke (z=10) must paint over the z=0 card: order was {colours:?}"
    );
    assert!(
        ink_at < high_at,
        "the stroke (z=10) must paint under the z=20 card: order was {colours:?}"
    );
}

/// Paint order and hit order are one rule. An interleaved press claimant vetoes
/// the card it is painted **over** and leaves alone the one painted over *it*.
///
/// Go back to a rank-only veto ("any `Over` claimant vetoes every card") and
/// the second half reddens: the stroke is not in the `Over` band, so it would
/// veto nothing and the card beneath it would keep taking the press it is
/// visibly covered by.
#[test]
fn an_interleaved_claimant_vetoes_only_the_cards_below_it() {
    let mut scene = Scene::new();
    let low = scene.add_widget(Card(CARD_LOW), Rect::new(0.0, 0.0, 60.0, 60.0));
    let high = scene.add_widget(Card(CARD_HIGH), Rect::new(0.0, 0.0, 60.0, 60.0));
    scene.set_z(low, 0.0);
    scene.set_z(high, 20.0);

    let ink = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0)).fill(INK),
        Point::ZERO,
    );
    scene.set_layer(ink, SceneLayer::Interleaved);
    scene.set_z(ink, 10.0);
    // A press claimant: the occlusion rule is `claims_press`, not "is painted".
    let mut handlers = SceneItemHandlerSet::new();
    handlers.on_tap(|_, _| {});
    scene.set_item_handlers(ink, Some(handlers));

    let mut tree = WidgetTree::new();
    let view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let model = tree
        .widget_as_any(view)
        .and_then(|w| w.downcast_ref::<SceneView>())
        .expect("the view is a SceneView")
        .model();

    let inside = Point::new(30.0, 30.0);
    assert_ne!(
        tree.hit_test(inside),
        Some(view),
        "the z=20 card is painted over the claimant, so it must still win the \
         arena's walk"
    );

    // Lower that card under the stroke: now the veto must bite.
    model.set_z(high, 5.0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        tree.hit_test(inside),
        Some(view),
        "with both cards under the claimant, the press belongs to the view's own \
         lightweight dispatch — which is the whole point of the veto"
    );
}

/// The accessibility consequence of the band, stated as a test: **none**.
///
/// The item keeps the synthetic node it had in `Under`, the paint node is not
/// published, and the number of Tab stops does not move. Asserted through
/// `sync_accessibility` — the cached door a platform adapter is handed.
#[test]
fn the_interleaved_band_does_not_move_the_accessibility_tree() {
    fn build(layer: SceneLayer) -> (WidgetTree, teksilo_core::WidgetId) {
        let mut scene = Scene::new();
        let card = scene.add_widget(Card(CARD_LOW), Rect::new(0.0, 0.0, 60.0, 60.0));
        scene.set_z(card, 0.0);
        let ink = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 200.0, 40.0)).fill(INK),
            Point::new(0.0, 10.0),
        );
        scene.set_layer(ink, layer);
        scene.set_z(ink, 10.0);
        let mut tree = WidgetTree::new();
        let root = tree.add(SceneView::new(scene));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.render();
        (tree, root)
    }

    let (mut under_tree, under_root) = build(SceneLayer::Under);
    let (mut inter_tree, inter_root) = build(SceneLayer::Interleaved);

    let under = under_tree.sync_accessibility();
    let inter = inter_tree.sync_accessibility();

    assert_eq!(
        under.nodes.len(),
        inter.nodes.len(),
        "the paint node must not become an AT node: {} nodes in Under against {} \
         in Interleaved",
        under.nodes.len(),
        inter.nodes.len()
    );

    let roles = |u: &accesskit::TreeUpdate| {
        let mut r: Vec<String> = u
            .nodes
            .iter()
            .map(|(_, n)| format!("{:?}", n.role()))
            .collect();
        r.sort();
        r
    };
    assert_eq!(
        roles(&under),
        roles(&inter),
        "the same roles, in the same multiplicities"
    );

    assert_eq!(
        under_tree.tab_stops_within(under_root).len(),
        inter_tree.tab_stops_within(inter_root).len(),
        "a paint node is not focusable, so keyboard reachability is unchanged"
    );
}

// ---------------------------------------------------------------------------
// 3. A wet surface that repaints on its own
// ---------------------------------------------------------------------------

struct WetProbe {
    layer: WetLayer,
    paints: Rc<Cell<usize>>,
    points: Rc<RefCell<Vec<Point>>>,
}

fn wet_probe() -> WetProbe {
    let paints = Rc::new(Cell::new(0usize));
    let points: Rc<RefCell<Vec<Point>>> = Rc::default();
    let layer = {
        let paints = paints.clone();
        let points = points.clone();
        WetLayer::new(move |canvas, _ctx| {
            paints.set(paints.get() + 1);
            for p in points.borrow().iter() {
                canvas.fill_rect(Rect::new(p.x, p.y, 2.0, 2.0), WET);
            }
        })
    };
    WetProbe {
        layer,
        paints,
        points,
    }
}

/// The one node a layer mounted in exactly one view has.
fn one_node(layer: &WetLayer) -> teksilo_core::WidgetId {
    let nodes = layer.nodes();
    assert_eq!(nodes.len(), 1, "the layer mounted in exactly one view");
    nodes[0].id()
}

/// The reason a wet surface needs a node of its own: the render walker computes
/// **one** `needs_paint` per node and gates that node's `paint` and its
/// `post_paint` with it, so there is no way to invalidate a `SceneView`'s
/// foreground alone.
///
/// Repainting the wet node must not repaint the view. Point the repaint at the
/// view's own id instead and the second assertion reddens.
#[test]
fn repainting_the_wet_layer_does_not_repaint_the_item_bands() {
    let band_paints = Rc::new(Cell::new(0usize));
    let probe = wet_probe();

    let mut scene = Scene::new();
    scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(CARD_LOW),
        Point::ZERO,
    );

    let counter = band_paints.clone();
    let mut tree = WidgetTree::new();
    let view = tree.add(
        SceneView::new(scene)
            .wet_layer(probe.layer.clone())
            .background(move |_c, _ctx, _r| {
                counter.set(counter.get() + 1);
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render();

    let bands_before = band_paints.get();
    let wet_before = probe.paints.get();
    assert!(
        bands_before > 0 && wet_before > 0,
        "both must have painted once before the interesting part"
    );

    probe.points.borrow_mut().push(Point::new(10.0, 10.0));
    let node = one_node(&probe.layer);
    assert_ne!(node, view, "the wet surface is a node of its own");
    tree.mark_needs_paint(node);
    tree.render();

    assert_eq!(
        probe.paints.get(),
        wet_before + 1,
        "the wet surface repainted"
    );
    assert_eq!(
        band_paints.get(),
        bands_before,
        "…and the view did not: its `paint` runs the whole Under band, and \
         re-running that per pointer sample is exactly what a node of its own \
         avoids"
    );
}

/// Where the wet surface lands, stated as a test rather than left implicit in
/// three files: over every card, under the `Over` band.
#[test]
fn the_wet_layer_paints_over_the_cards_and_under_the_over_band() {
    let probe = wet_probe();
    probe.points.borrow_mut().push(Point::new(10.0, 10.0));

    let mut scene = Scene::new();
    let card = scene.add_widget(Card(CARD_HIGH), Rect::new(0.0, 0.0, 60.0, 60.0));
    scene.set_z(card, 50.0);
    let over = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 200.0, 40.0)).fill(INK),
        Point::ZERO,
    );
    scene.set_layer(over, SceneLayer::Over);

    let mut tree = WidgetTree::new();
    tree.add(SceneView::new(scene).wet_layer(probe.layer.clone()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    let colours = painted_colours(&frame);

    let card_at = index_of(&colours, CARD_HIGH).expect("the card painted");
    let wet_at = index_of(&colours, WET).expect("the wet stroke painted");
    let over_at = index_of(&colours, INK).expect("the Over item painted");

    assert!(
        card_at < wet_at,
        "wet content is drawn over what it is being drawn on: {colours:?}"
    );
    assert!(
        wet_at < over_at,
        "…and under the Over band, so the view's own chrome stays visible \
         through it: {colours:?}"
    );
}

/// One layer, two views — the `SceneModel` comparison its own doc makes.
///
/// A second view mounting the layer must not displace the first: both paint it,
/// and both are repainted. Go back to a single `Cell<Option<WidgetId>>` for the
/// mount and the last view to build wins — the painter still runs twice (each
/// node holds its own clone of the handle), but only one node is ever marked,
/// so the other pane shows stale ink until something else dirties it. That is
/// the second assertion below.
#[test]
fn one_layer_in_two_views_paints_and_repaints_both() {
    use teksilo_widgets::{Expand, HStack};

    let probe = wet_probe();
    probe.points.borrow_mut().push(Point::new(10.0, 10.0));
    let model = teksilo_scene::SceneModel::new();

    let mut tree = WidgetTree::new();
    let a = tree.add(
        SceneView::with_model(model.clone())
            .wet_layer(probe.layer.clone())
            .adopt_scene_size(false),
    );
    let b = tree.add(
        SceneView::with_model(model.clone())
            .wet_layer(probe.layer.clone())
            .adopt_scene_size(false),
    );
    tree.add(
        HStack::new()
            .child(Expand::new().child_id(a))
            .child(Expand::new().child_id(b)),
    );
    tree.layout(SizeProposal::exact(800.0, 300.0));
    tree.render();

    let nodes = probe.layer.nodes();
    assert_eq!(
        nodes.len(),
        2,
        "both views mounted the layer; a second mount must not evict the first"
    );
    assert_ne!(
        nodes[0].id(),
        nodes[1].id(),
        "each view paints into its own node"
    );
    assert_eq!(
        probe.paints.get(),
        2,
        "the painter ran once per view: {} time(s)",
        probe.paints.get()
    );

    // …and a repaint reaches both panes, not whichever one happened to build
    // last. `mark_needs_paint` is the mark `WetLayer::request_repaint` ends at,
    // minus the EventContext a test has no window for — and minus the typed
    // resolution that context buys, which is why the ids come from `nodes()`
    // and the tree is named at the call.
    let before = probe.paints.get();
    probe.points.borrow_mut().push(Point::new(20.0, 20.0));
    for node in &nodes {
        tree.mark_needs_paint(node.id());
    }
    tree.render();
    assert_eq!(
        probe.paints.get(),
        before + 2,
        "both panes repainted; a handle that tracks one node can only ever \
         refresh one of them"
    );
}

/// The positive half of the pair below: `request_repaint` called from a real
/// handler does repaint the wet surface.
///
/// Worth its own test because the mark now travels through
/// `EventContext::with_widget_mut`, which resolves the id in the calling tree
/// and reaches nothing unless the node overrides `Widget::as_any_mut`. Drop
/// that override on `WetLayerNode` and a wet stroke silently stops refreshing —
/// a failure no amount of "did the wrong thing get marked?" testing would see.
#[test]
fn a_handler_repaints_the_wet_surface_through_request_repaint() {
    let probe = wet_probe();
    let layer = probe.layer.clone();

    let mut tree = WidgetTree::new();
    tree.add(
        SceneView::new(Scene::new())
            .wet_layer(probe.layer.clone())
            .on_pointer_event(move |_event, ctx| {
                layer.request_repaint(ctx);
                EventResponse::Ignored
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render();

    let before = probe.paints.get();
    assert!(
        before > 0,
        "the surface painted once before the frame of interest"
    );

    probe.points.borrow_mut().push(Point::new(10.0, 10.0));
    tree.dispatch_pointer(
        PointerSample::mouse(
            PointerPhase::Down,
            Point::new(50.0, 50.0),
            EventTime::from_millis(0),
        )
        .with_button(teksilo_core::PointerButton::Primary),
    );
    tree.render();

    assert_eq!(
        probe.paints.get(),
        before + 1,
        "the handler asked for a repaint and the surface took it"
    );
}

/// A leaf that counts its own paints, for asking "did *this* node repaint?".
#[derive(Debug)]
struct CountingLeaf(Rc<Cell<usize>>);

impl Widget for CountingLeaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(10.0, 10.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        self.0.set(self.0.get() + 1);
        canvas.fill_rect(bounds, CARD_LOW);
    }

    /// So the test below can confirm it really staged a slot collision.
    /// Deliberately **not** paired with `as_any_mut`: this leaf is standing in
    /// for an ordinary widget, and an ordinary widget does not opt into `&mut`
    /// introspection.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// **A repaint asked for in one tree must not mark a slot in another.**
///
/// A `WidgetId` is a per-arena slot key and every tree mints the same ones, so
/// two window-less trees wear the same `None` window and the mount list cannot
/// tell them apart. Pushing the ids it holds straight at
/// `EventContext::request_repaint` therefore marks whatever unrelated widget
/// happens to hold that slot in the calling tree — which is what this stages:
/// the layer is mounted only in `b`, the repaint is asked for from `a`, and in
/// `a` that id is an ordinary leaf.
///
/// `WetLayer::request_repaint` resolves each id through
/// `EventContext::with_widget_mut` instead, which looks the id up in the
/// **calling** tree and hands the node over only if it is a `WetLayerNode` —
/// so a foreign id reaches nothing. Go back to
/// `ctx.request_repaint(mount.node)` and the leaf repaints.
#[test]
fn a_repaint_asked_for_in_one_tree_does_not_mark_another_trees_slot() {
    use teksilo_widgets::VStack;

    let probe = wet_probe();

    // `b` owns the only mount. Kept small, so its wet node takes a low slot —
    // the slots `a` fills with leaves.
    let mut b = WidgetTree::new();
    b.add(SceneView::new(Scene::new()).wet_layer(probe.layer.clone()));
    b.layout(SizeProposal::exact(400.0, 300.0));
    b.render();
    let foreign = one_node(&probe.layer);

    // `a` mounts nothing, and asks for the repaint from a handler of its own.
    let paints = Rc::new(Cell::new(0usize));
    let layer = probe.layer.clone();
    let mut a = WidgetTree::new();
    a.add(
        VStack::new()
            .children((0..8).map(|_| CountingLeaf(paints.clone())))
            .child(
                SceneView::new(Scene::new()).on_pointer_event(move |_event, ctx| {
                    layer.request_repaint(ctx);
                    EventResponse::Ignored
                }),
            ),
    );
    a.layout(SizeProposal::exact(400.0, 300.0));
    a.render();

    assert!(
        a.widget_as_any(foreign)
            .is_some_and(|w| w.downcast_ref::<CountingLeaf>().is_some()),
        "this test stages a slot collision, and there is none: {foreign:?} is \
         not one of `a`'s leaves. Re-tune the leaf count until it is, or the \
         assertion below proves nothing"
    );

    let before = paints.get();
    a.dispatch_pointer(
        PointerSample::mouse(
            PointerPhase::Down,
            Point::new(200.0, 200.0),
            EventTime::from_millis(0),
        )
        .with_button(teksilo_core::PointerButton::Primary),
    );
    a.render();

    assert_eq!(
        paints.get(),
        before,
        "the repaint was asked for in `a`, the only mount lives in `b`, and \
         {foreign:?} is a leaf in `a`. Nothing in `a` should have been marked"
    );
}

/// The wet surface must be transparent to the pointer and absent from the
/// keyboard ring, or an ink tool would swallow every click on the scene beneath
/// it and add a dead Tab stop.
#[test]
fn the_wet_layer_is_not_a_hit_target_and_not_a_tab_stop() {
    let probe = wet_probe();
    let mut scene = Scene::new();
    let card = scene.add_widget(Card(CARD_LOW), Rect::new(0.0, 0.0, 60.0, 60.0));
    scene.set_z(card, 0.0);

    let mut tree = WidgetTree::new();
    let view = tree.add(SceneView::new(scene).wet_layer(probe.layer.clone()));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render();

    let node = one_node(&probe.layer);
    assert_ne!(
        tree.hit_test(Point::new(30.0, 30.0)),
        Some(node),
        "the wet surface covers the viewport; if it took hits, nothing under it \
         would ever be clickable again"
    );
    assert!(
        !tree.tab_stops_within(view).contains(&node),
        "a surface with no content of its own is not a keyboard stop"
    );
}

// ---------------------------------------------------------------------------
// 1. Every sample reaches the tool
// ---------------------------------------------------------------------------

/// Record every position a handler is offered, oldest first: the batched ones
/// then the packet's own.
type Trail = Rc<RefCell<Vec<(Point, Option<f32>)>>>;

fn record_pointer_trail(tree: &mut WidgetTree) -> (teksilo_core::WidgetId, Trail) {
    let trail: Trail = Rc::default();
    let sink = trail.clone();
    let view = tree.add(
        SceneView::new(Scene::new()).on_pointer_event(move |event, ctx| {
            if matches!(event, teksilo_core::event::WidgetEvent::PointerMove { .. }) {
                let mut out = sink.borrow_mut();
                for c in ctx.coalesced() {
                    out.push((c.window_position, c.axes.pressure));
                }
                if let Some(p) = ctx.pointer_position() {
                    out.push((p, ctx.pointer().axes.pressure));
                }
            }
            EventResponse::Ignored
        }),
    );
    (view, trail)
}

fn batched_move() -> PointerSample {
    let mut pointer = PointerInfo::mouse(EventTime::from_millis(12));
    pointer.axes = pressure(0.6);
    PointerSample {
        pointer,
        phase: PointerPhase::Move,
        position: Point::new(30.0, 30.0),
        button: None,
        modifiers: teksilo_core::Modifiers::NONE,
        coalesced: vec![
            CoalescedSample::new(EventTime::from_millis(4), Point::new(10.0, 10.0))
                .with_axes(pressure(0.2)),
            CoalescedSample::new(EventTime::from_millis(8), Point::new(20.0, 20.0))
                .with_axes(pressure(0.4)),
        ],
    }
}

/// A tool written against `EventContext::coalesced` sees every position the
/// platform delivered, whether or not the backend batched them.
///
/// The positions arrive window-logical and oldest-first, each with the axes it
/// was sampled at, and the packet's own position is the newest — so a tool fans
/// out over `coalesced()` and then handles `pointer_position()`.
///
/// Drop the axes on the way through `InputSnapshot::from_pointer_sample` (the
/// state it was in before this landed) and the pressures come back `None`;
/// leave `InputSnapshot::coalesced` unpopulated and only one point arrives.
#[test]
fn a_tool_sees_every_position_a_batched_sample_carried() {
    let mut tree = WidgetTree::new();
    let (_view, trail) = record_pointer_trail(&mut tree);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.dispatch_pointer(batched_move());

    assert_eq!(
        *trail.borrow(),
        vec![
            (Point::new(10.0, 10.0), Some(0.2)),
            (Point::new(20.0, 20.0), Some(0.4)),
            (Point::new(30.0, 30.0), Some(0.6)),
        ],
        "two batched positions and the packet's own, oldest first, each with the \
         pressure it was sampled at — the variation inside a batch is what a \
         stroke's width is made of"
    );
}

/// The handler an ink tool would naively reach for is the wrong one, and this
/// is why `docs/ink.md` says to use `on_pointer_event`.
///
/// `DragRecognizer` returns `Pending` for every move inside the drag slop and
/// then reports `DragStarted` at the **press** position, so the samples that
/// crossed the slop — the first millimetres of a stroke, where its taper and
/// pressure ramp live — are never delivered to `on_drag` at all.
#[test]
fn the_drag_recognizer_swallows_the_start_of_a_stroke() {
    #[derive(Debug, Default)]
    struct Leaf;
    impl Widget for Leaf {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(400.0, 300.0).into()
        }
    }

    let raw: Rc<RefCell<Vec<Point>>> = Rc::default();
    let dragged: Rc<RefCell<Vec<Point>>> = Rc::default();
    let raw_sink = raw.clone();
    let drag_sink = dragged.clone();

    let mut tree = WidgetTree::new();
    tree.add(
        Leaf.on_pointer_event(move |event, _ctx| {
            if let teksilo_core::event::WidgetEvent::PointerMove { position, .. } = event {
                raw_sink.borrow_mut().push(*position);
            }
            EventResponse::Ignored
        })
        .on_drag(move |phase, _ctx| {
            if let teksilo_core::gesture::DragPhase::Moved { position, .. } = phase {
                drag_sink.borrow_mut().push(position);
            }
        }),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // A stroke drawn one device pixel at a time, well inside the 5 dp mouse slop
    // for its first few samples.
    let at = |x: f32| Point::new(x, 10.0);
    tree.dispatch_pointer(
        PointerSample::mouse(PointerPhase::Down, at(0.0), EventTime::from_millis(0))
            .with_button(teksilo_core::PointerButton::Primary),
    );
    for i in 1..=10 {
        tree.dispatch_pointer(PointerSample::mouse(
            PointerPhase::Move,
            at(i as f32),
            EventTime::from_millis(i * 4),
        ));
    }

    let raw = raw.borrow();
    let dragged = dragged.borrow();
    assert_eq!(raw.len(), 10, "every sample reached `on_pointer_event`");
    assert!(
        dragged.len() < raw.len(),
        "the drag recognizer was expected to swallow the pre-slop samples; it \
         delivered {} of {}. If this has changed, `docs/ink.md` needs \
         rewriting rather than this test deleting.",
        dragged.len(),
        raw.len()
    );
    assert!(
        !dragged.contains(&at(1.0)),
        "the first sample of the stroke is the one that goes missing"
    );
}

/// A dispatch that is **not** a sample reports no batched positions, rather than
/// whichever batch happened to arrive last.
///
/// `InputSnapshot::for_recognized_gesture` and `for_drag_session` write the
/// empty list out rather than inheriting it, so this is a decision and not an
/// accident of `..Default::default()`.
#[test]
fn a_dispatch_with_no_sample_behind_it_reports_no_batched_positions() {
    let seen: Rc<Cell<Option<usize>>> = Rc::default();
    let sink = seen.clone();

    #[derive(Debug, Default)]
    struct Leaf;
    impl Widget for Leaf {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(100.0, 100.0).into()
        }
    }

    let mut tree = WidgetTree::new();
    let id = tree.add(Leaf.focusable(true).on_access_action(move |_action, ctx| {
        sink.set(Some(ctx.coalesced().len()));
        EventResponse::Handled
    }));
    tree.layout(SizeProposal::exact(100.0, 100.0));

    // Prime the tree with a batched sample, so "empty" cannot be mistaken for
    // "nothing ever carried a batch".
    tree.dispatch_pointer(batched_move());
    let mut ops = teksilo_core::window::NoopWindowOps;
    tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(id),
        accesskit::Action::Click,
        None,
        &mut ops,
    );

    assert_eq!(
        seen.get(),
        Some(0),
        "an assistive-technology action batched nothing, and reporting the last \
         sample's positions would attribute them to a gesture that did not \
         produce them"
    );
}

/// Changing an item's band at **runtime** has to move it on both paths, not
/// one. Paint follows because the view materialises (or reaps) a paint node on
/// the next build; hit follows because `LayerChanged` invalidates the hit
/// snapshot as `Structure`. Drop either half and the two pickers disagree about
/// a stroke the user just raised.
#[test]
fn a_runtime_band_change_moves_both_paint_and_hit() {
    let mut scene = Scene::new();
    let card = scene.add_widget(Card(CARD_HIGH), Rect::new(0.0, 0.0, 60.0, 60.0));
    scene.set_z(card, 5.0);
    let ink = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 60.0, 60.0)).fill(INK),
        Point::ZERO,
    );
    scene.set_z(ink, 10.0);
    let mut handlers = SceneItemHandlerSet::new();
    handlers.on_tap(|_, _| {});
    scene.set_item_handlers(ink, Some(handlers));

    let mut tree = WidgetTree::new();
    let view = tree.add(SceneView::new(scene));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let model = tree
        .widget_as_any(view)
        .and_then(|w| w.downcast_ref::<SceneView>())
        .expect("the view is a SceneView")
        .model();

    let inside = Point::new(30.0, 30.0);
    // Three paths, not two: what a screen reader is told must not move either,
    // and a runtime change is a different code path from building in the band.
    let at_before = tree.sync_accessibility().nodes.len();
    // `Under` (the default): the item is below the card on both paths, even
    // though its `z` is higher — rank beats z, which is the band doing its job.
    let colours = painted_colours(&tree.render());
    assert!(
        index_of(&colours, INK) < index_of(&colours, CARD_HIGH),
        "an Under item paints below the card: {colours:?}"
    );
    assert_ne!(
        tree.hit_test(inside),
        Some(view),
        "…and the card takes the press"
    );

    model.set_layer(ink, SceneLayer::Interleaved);
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let colours = painted_colours(&tree.render());
    assert!(
        index_of(&colours, CARD_HIGH) < index_of(&colours, INK),
        "after the band change the item paints above the card: {colours:?}"
    );
    assert_eq!(
        tree.hit_test(inside),
        Some(view),
        "…and the press follows it — one order, not two"
    );
    assert_eq!(
        tree.sync_accessibility().nodes.len(),
        at_before,
        "…and the accessibility tree does not: the paint node the band \
         materialised is left out of `accessibility_children`, so the item keeps \
         the one synthetic node it already had"
    );

    // …and back again, because a one-way invalidation is the other half of the
    // same defect.
    model.set_layer(ink, SceneLayer::Under);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let colours = painted_colours(&tree.render());
    assert!(
        index_of(&colours, INK) < index_of(&colours, CARD_HIGH),
        "the item goes back under: {colours:?}"
    );
    assert_ne!(
        tree.hit_test(inside),
        Some(view),
        "…and so does the press. The paint node was reaped, not merely hidden."
    );
}

/// `Size` is used by `Card`'s layout in the obvious form; this keeps the import
/// honest if that ever changes.
#[allow(dead_code)]
fn _size_is_used(s: Size) -> f32 {
    s.width
}
