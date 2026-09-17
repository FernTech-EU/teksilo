// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end magnetism coverage for `SceneView`: item-drag snap,
//! port-drag wires, the keyboard connect flow, synthetic magnet AT
//! nodes + `active_descendant`, multi-view, and the heavyweight
//! consumer-helper path.

use super::*;
use crate::items::RectItem;
use crate::magnet::{Magnet, MagnetRef, MagnetRole, MagnetVerdict, MagnetismConfig};
use crate::scene_model::SceneModel;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use teksilo_canvas::Point;
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};

/// Source <-> Target on different items accept; everything else rejects.
fn source_to_target(a: &MagnetRef, b: &MagnetRef) -> MagnetVerdict {
    if a.item != b.item
        && matches!(
            (a.role, b.role),
            (MagnetRole::Source, MagnetRole::Target) | (MagnetRole::Target, MagnetRole::Source)
        )
    {
        MagnetVerdict::accept()
    } else {
        MagnetVerdict::Reject
    }
}

/// Recorder for `on_connect`: a count and the last connected pair.
type Recorder = (Rc<Cell<u32>>, Rc<RefCell<Option<(MagnetId, MagnetId)>>>);

fn recording_config(
    predicate: impl Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict + 'static,
) -> (MagnetismConfig, Recorder) {
    let count = Rc::new(Cell::new(0u32));
    let pair: Rc<RefCell<Option<(MagnetId, MagnetId)>>> = Rc::new(RefCell::new(None));
    let c = count.clone();
    let p = pair.clone();
    let cfg = MagnetismConfig::new(predicate).on_connect(move |conn, _ctx| {
        c.set(c.get() + 1);
        *p.borrow_mut() = Some((conn.from.id, conn.to.id));
    });
    (cfg, (count, pair))
}

fn down(tree: &mut WidgetTree, p: Point) {
    tree.pointer_move(p);
    tree.dispatch_event(WidgetEvent::pointer_down(
        p,
        PointerButton::Primary,
        Modifiers::default(),
    ));
}
fn moved(tree: &mut WidgetTree, p: Point) {
    tree.dispatch_event(WidgetEvent::pointer_move(p));
}
fn up(tree: &mut WidgetTree, p: Point) {
    tree.dispatch_event(WidgetEvent::pointer_up(
        p,
        PointerButton::Primary,
        Modifiers::default(),
    ));
}
fn key(tree: &mut WidgetTree, k: Key) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
        text: None,
    });
}

/// Build a scene: item A (draggable) with a Source magnet at its right
/// edge, item B with a Target magnet at its left edge, 200 units apart.
/// Returns `(scene, a, source_magnet, b, target_magnet)`.
fn two_node_scene() -> (Scene, ItemId, MagnetId, ItemId, MagnetId) {
    let mut scene = Scene::new();
    let a = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0))
            .fill(teksilo_tokens::Color::RED)
            .draggable(true),
        Point::new(0.0, 0.0),
    );
    let am = scene.add_magnet(
        a,
        Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
    );
    let b = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::BLUE),
        Point::new(200.0, 0.0),
    );
    let bm = scene.add_magnet(
        b,
        Magnet::new(Point::new(0.0, 20.0)).role(MagnetRole::Target),
    );
    (scene, a, am, b, bm)
}

#[test]
fn item_drag_snaps_and_fires_connection() {
    let (scene, a, am, _b, bm) = two_node_scene();
    let (cfg, (count, pair)) = recording_config(source_to_target);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Press at A's centre (20,20) — clear of A's magnet at (40,20) so a
    // port-drag isn't grabbed — then drag right so A's source magnet
    // (40,20)+delta lands 2px short of B's target magnet (200,20).
    down(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(178.0, 20.0));
    up(&mut tree, Point::new(178.0, 20.0));

    // The connection fires on release, before the move is drained.
    assert_eq!(count.get(), 1, "exactly one connection");
    assert_eq!(*pair.borrow(), Some((am, bm)));

    // Drain the move and confirm A snapped to x=160 (158 raw + 2 snap).
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|w| w.downcast_mut::<SceneView>())
        .expect("downcast");
    view.flush_pending_item_move();
    let pos = view.scene().local_pos(a).expect("a alive");
    assert!(
        (pos.x - 160.0).abs() < 1e-3,
        "A should snap to x=160, got {}",
        pos.x
    );
}

#[test]
fn item_drag_with_rejecting_predicate_does_not_snap_or_connect() {
    let (scene, a, _am, _b, _bm) = two_node_scene();
    let (cfg, (count, _pair)) = recording_config(|_, _| MagnetVerdict::Reject);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    down(&mut tree, Point::new(20.0, 20.0));
    moved(&mut tree, Point::new(178.0, 20.0));
    up(&mut tree, Point::new(178.0, 20.0));

    assert_eq!(count.get(), 0, "rejecting predicate forms no connection");
    let view = tree
        .widget_as_any_mut(view_id)
        .and_then(|w| w.downcast_mut::<SceneView>())
        .expect("downcast");
    view.flush_pending_item_move();
    let pos = view.scene().local_pos(a).expect("a alive");
    assert!(
        (pos.x - 158.0).abs() < 1e-3,
        "no snap: A stays at the raw drag x=158, got {}",
        pos.x
    );
}

#[test]
fn port_drag_fires_connection_without_moving_the_item() {
    let (scene, a, am, _b, bm) = two_node_scene();
    let (cfg, (count, pair)) = recording_config(source_to_target);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));

    // Press directly on A's magnet handle (40,20) -> port-drag, drag the
    // wire to B's magnet (200,20).
    down(&mut tree, Point::new(40.0, 20.0));
    moved(&mut tree, Point::new(198.0, 20.0));
    up(&mut tree, Point::new(198.0, 20.0));

    assert_eq!(count.get(), 1, "port-drag forms one connection");
    assert_eq!(*pair.borrow(), Some((am, bm)));

    // The item must NOT have moved (port-drag drags a wire, not the item).
    let view = view_handle(&tree, view_id);
    let pos = view.scene().local_pos(a).expect("a alive");
    assert!(
        pos.x.abs() < 1e-3 && pos.y.abs() < 1e-3,
        "port-drag must not move the item, got {:?}",
        pos
    );
}

#[test]
fn keyboard_connect_flow_forms_connection() {
    let (scene, _a, am, _b, bm) = two_node_scene();
    let (cfg, (count, pair)) = recording_config(source_to_target);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.focus(view_id);

    // 'm' enters connect mode, focusing the first magnet (A's source).
    key(&mut tree, Key::Character('m'));
    // Enter activates it as the source.
    key(&mut tree, Key::Enter);
    // ArrowRight moves focus to the only accepting target (B's).
    key(&mut tree, Key::ArrowRight);
    // Enter confirms the connection.
    key(&mut tree, Key::Enter);

    assert_eq!(count.get(), 1, "keyboard connect forms one connection");
    assert_eq!(*pair.borrow(), Some((am, bm)));
}

#[test]
fn keyboard_escape_cancels_pending_then_exits() {
    let (scene, _a, _am, _b, _bm) = two_node_scene();
    let (cfg, (count, _pair)) = recording_config(source_to_target);

    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.focus(view_id);

    key(&mut tree, Key::Character('m')); // enter mode, focus source
    key(&mut tree, Key::Enter); // pending = source
    key(&mut tree, Key::Escape); // cancel pending
    {
        let view = view_handle(&tree, view_id);
        assert!(view.magnet_connect_mode.get(), "still in connect mode");
        assert!(view.magnet_pending.get().is_none(), "pending cleared");
    }
    key(&mut tree, Key::Escape); // exit mode
    {
        let view = view_handle(&tree, view_id);
        assert!(!view.magnet_connect_mode.get(), "connect mode exited");
    }
    assert_eq!(count.get(), 0, "no connection formed during cancel");
}

#[test]
fn emits_synthetic_magnet_at_node_only_with_magnetism() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};

    // With magnetism: the magnet appears as a synthetic AT node.
    let (scene, _a, am, _b, _bm) = two_node_scene();
    let (cfg, _rec) = recording_config(source_to_target);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let update = tree.sync_accessibility();
    let magnet_node = synthetic_node_id(view_id, am.as_u64(), SyntheticKind::SceneMagnet);
    assert!(
        update.nodes.iter().any(|(id, _)| *id == magnet_node),
        "magnet must appear as a synthetic AT node when magnetism is on"
    );

    // Without magnetism: no magnet AT node.
    let (scene2, _a2, am2, _b2, _bm2) = two_node_scene();
    let mut tree2 = WidgetTree::new();
    let view2 = tree2.add(SceneView::new(scene2));
    tree2.layout(SizeProposal::exact(800.0, 600.0));
    let update2 = tree2.sync_accessibility();
    let magnet_node2 = synthetic_node_id(view2, am2.as_u64(), SyntheticKind::SceneMagnet);
    assert!(
        !update2.nodes.iter().any(|(id, _)| *id == magnet_node2),
        "no magnet AT node when magnetism is off"
    );
}

#[test]
fn connect_mode_points_active_descendant_at_focused_magnet() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id, widget_id_to_node_id};

    let (scene, _a, am, _b, _bm) = two_node_scene();
    let (cfg, _rec) = recording_config(source_to_target);
    let mut tree = WidgetTree::new();
    let view_id = tree.add(SceneView::new(scene).magnetism(cfg));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.focus(view_id);
    key(&mut tree, Key::Character('m')); // enter connect mode, focus A's magnet

    let update = tree.sync_accessibility();
    let view_node_id = widget_id_to_node_id(view_id);
    let view_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == view_node_id)
        .map(|(_, n)| n)
        .expect("SceneView AT node present");
    let expected = synthetic_node_id(view_id, am.as_u64(), SyntheticKind::SceneMagnet);
    assert_eq!(
        view_node.active_descendant(),
        Some(expected),
        "active_descendant should point at the focused magnet"
    );
}

#[test]
fn multi_view_magnet_at_nodes_are_per_view() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
    use teksilo_widgets::{Expand, HStack};

    // One shared model, two views: only the magnetism-enabled view emits
    // magnet AT nodes.
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(0.0, 0.0),
    );
    let am = model.add_magnet(
        a,
        Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
    );

    let (cfg, _rec) = recording_config(source_to_target);
    let mut tree = WidgetTree::new();
    let view_a_id = tree.add(SceneView::with_model(model.clone()).magnetism(cfg));
    let view_b_id = tree.add(SceneView::with_model(model.clone()));
    tree.add(
        HStack::new()
            .child(Expand::new().child(view_a_id))
            .child(Expand::new().child(view_b_id)),
    );
    tree.layout(SizeProposal::exact(800.0, 600.0));

    let update = tree.sync_accessibility();
    let in_a = synthetic_node_id(view_a_id, am.as_u64(), SyntheticKind::SceneMagnet);
    let in_b = synthetic_node_id(view_b_id, am.as_u64(), SyntheticKind::SceneMagnet);
    assert!(
        update.nodes.iter().any(|(id, _)| *id == in_a),
        "magnetism view emits the magnet AT node"
    );
    assert!(
        !update.nodes.iter().any(|(id, _)| *id == in_b),
        "non-magnetism view emits no magnet AT node"
    );
}

#[test]
fn compute_item_snap_serves_heavyweight_consumer_path() {
    // Magnets on heavyweight (`add_widget`) items: the built-in mouse
    // drag path can't move them, but the reusable snap helper still
    // resolves the connecting pair — the path a heavyweight brick's own
    // drag wiring would call.
    let mut scene = Scene::new();
    let a = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 40.0, 40.0));
    let am = scene.add_magnet(
        a,
        Magnet::new(Point::new(40.0, 20.0)).role(MagnetRole::Source),
    );
    let b = scene.add_widget(FillWidget::new(), Rect::new(200.0, 0.0, 40.0, 40.0));
    let bm = scene.add_magnet(
        b,
        Magnet::new(Point::new(0.0, 20.0)).role(MagnetRole::Target),
    );

    // Heavyweight items are not snap-candidates of the SceneView drag
    // path, but compute_item_snap works on them directly.
    let snap = scene
        .compute_item_snap(a, Vec2::new(158.0, 0.0), 14.0, &source_to_target)
        .expect("snap resolved for heavyweight items");
    assert_eq!(snap.from, am);
    assert_eq!(snap.to, bm);
    assert!((snap.snap_vector.x - 2.0).abs() < 1e-3);
}

// ---------------------------------------------------------------------------
// The geometry constraint and the magnet, on a rotated item
// ---------------------------------------------------------------------------

/// The hazard, stated without a gesture: a constraint that accepts everything
/// still hands back a *different* translation for a rotated item.
///
/// `constrained_translation` states the proposal in the frame's own basis —
/// `rotate(-theta)`, add to the frame's origin, ask, subtract the origin,
/// `rotate(theta)` — and none of those four steps is bit-exact in `f32`. The
/// error is small (a few times 1e-5 here) and utterly invisible, which is
/// exactly why deciding "did the rule overrule the magnet?" with `==` was a bug
/// that no amount of looking at the screen would find.
///
/// Reverting `constraint_kept` to `==` reddens the last assertion.
#[test]
fn a_rotated_accept_round_trips_inexactly_and_the_tolerance_absorbs_it() {
    use crate::ChangeVerdict;
    use crate::transform_session::TransformSource;
    use crate::view::gestures_impl::constraint_kept;
    use teksilo_canvas::{Transform2D, Vec2};

    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(-20.0, -20.0, 40.0, 40.0)),
        Point::new(137.4, 91.7),
    );
    model.set_transform(a, Transform2D::rotate(0.6));
    model.set_geometry_constraint(|_| ChangeVerdict::Accept);

    let start = model.transform_frame(&[a]).expect("resolves");
    assert!(
        start.rotation.abs() > 1e-6,
        "precondition: the frame carries the item\'s rotation"
    );
    let raw = Vec2::new(151.37, -37.91);
    let applied = model.constrain_move(&[a], start, raw, TransformSource::Pointer);

    assert_ne!(
        applied, raw,
        "precondition: the round trip really is lossy — if this ever becomes \
         exact the `==` test below stops being a hazard and this test stops \
         meaning anything"
    );
    let anchor = Point::new(137.4, 91.7);
    let magnetised = Point::new(anchor.x + raw.x, anchor.y + raw.y);
    let constrained = Point::new(anchor.x + applied.x, anchor.y + applied.y);
    assert!(
        constraint_kept(magnetised, constrained),
        "a rule that accepted the proposal did not overrule the magnet: \
         {magnetised:?} vs {constrained:?}"
    );
}

/// The same thing end to end: a constraint that changes nothing must change
/// nothing a user can observe.
///
/// It failed **only on rotated items**, which is why every other magnetism test
/// in this file passed — at zero degrees the round trip is exact. The numbers
/// here are deliberately not round: on a tidy grid the lossy round trip can
/// land back on the same `f32` by luck, and a test that relies on that luck
/// pins nothing.
#[test]
fn a_do_nothing_constraint_still_lets_a_rotated_item_connect() {
    use crate::ChangeVerdict;

    const THETA: f32 = 0.6;
    const CARRY: f32 = 147.37;

    /// Build the scene, drag A's body `CARRY` to the right, and report how many
    /// connections fired and where A ended up.
    fn run(with_constraint: bool) -> (u32, Point) {
        let model = SceneModel::new();
        // A's rect is centred on its own origin, so rotating it leaves its
        // centre — the press point — exactly at its `local_pos`.
        let a = model.add_item(
            RectItem::new(Rect::new(-20.0, -20.0, 40.0, 40.0))
                .fill(teksilo_tokens::Color::RED)
                .draggable(true),
            Point::new(137.4, 91.7),
        );
        model.set_transform(a, teksilo_canvas::Transform2D::rotate(THETA));
        let am = model.add_magnet(
            a,
            Magnet::new(Point::new(20.0, 0.0)).role(MagnetRole::Source),
        );
        let source = model.magnet_scene_pos(am).expect("a's magnet resolves");
        // Place B so its target magnet sits 2 units beyond where A's source
        // magnet lands — inside the 14 px capture radius, so the snap closes it.
        let b = model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::BLUE),
            Point::new(source.x + CARRY + 2.0, source.y - 20.0),
        );
        model.add_magnet(
            b,
            Magnet::new(Point::new(0.0, 20.0)).role(MagnetRole::Target),
        );
        if with_constraint {
            model.set_geometry_constraint(|_| ChangeVerdict::Accept);
        }

        let (cfg, (count, _pair)) = recording_config(source_to_target);
        let mut tree = WidgetTree::new();
        let view_id = tree.add(SceneView::with_model(model).magnetism(cfg));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        down(&mut tree, Point::new(137.4, 91.7));
        moved(&mut tree, Point::new(137.4 + CARRY, 91.7));
        up(&mut tree, Point::new(137.4 + CARRY, 91.7));

        let view = tree
            .widget_as_any_mut(view_id)
            .and_then(|w| w.downcast_mut::<SceneView>())
            .expect("downcast");
        view.flush_pending_item_move();
        let pos = view.scene().local_pos(a).expect("a alive");
        (count.get(), pos)
    }

    let (bare_count, bare_pos) = run(false);
    assert_eq!(
        bare_count, 1,
        "precondition: without a constraint the magnet connects"
    );

    let (guarded_count, guarded_pos) = run(true);
    assert_eq!(
        guarded_count, 1,
        "a constraint that accepts everything must not silently break magnetism: \
         bare ended at {bare_pos:?}, constrained at {guarded_pos:?}"
    );
    assert!(
        (guarded_pos.x - bare_pos.x).abs() < 1e-2 && (guarded_pos.y - bare_pos.y).abs() < 1e-2,
        "and must not move the item either: {bare_pos:?} vs {guarded_pos:?}"
    );
}

/// `MagnetismConfig::enabled` is offered as a signal, so a toolbar toggle is
/// the expected way to drive it — and an unbound signal makes that toggle a
/// lie in the two places it is read.
///
/// This is the transform controller's `enabled` defect one door over, and the
/// tell is the same: the *fresh* walk was right all along, so the gate works
/// and only the invalidation was missing. Reading the **published** tree
/// (`sync_accessibility`, what a platform adapter is handed) rather than a
/// fresh `accessibility_tree_snapshot` is the whole point — a snapshot cannot
/// see this class of bug, and has now hidden it three times in this crate.
#[test]
fn turning_magnetism_off_takes_its_markers_and_its_at_nodes_with_it() {
    use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};

    // Keyed on the scene's own `MagnetId`s, not a guessed range: the id is a
    // process-global counter, so a fixed range only finds them when this test
    // happens to run early.
    fn magnet_nodes(tree: &mut WidgetTree, view_id: WidgetId, magnets: &[MagnetId]) -> usize {
        let ours: Vec<accesskit::NodeId> = magnets
            .iter()
            .map(|m| synthetic_node_id(view_id, m.as_u64(), SyntheticKind::SceneMagnet))
            .collect();
        tree.sync_accessibility()
            .nodes
            .iter()
            .filter(|(id, _)| ours.contains(id))
            .count()
    }
    fn draws(tree: &mut WidgetTree) -> usize {
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.render().draw_order.len()
    }

    // Calibrate against a view whose magnetism was off from the very start, so
    // the assertions below cannot pass by the markers having been invisible.
    let off_from_the_start = {
        let (scene, _a, am, _b, bm) = two_node_scene();
        let (cfg, _) = recording_config(source_to_target);
        cfg.enabled_signal().set(false);
        let mut tree = WidgetTree::new();
        let view_id = tree.add(
            SceneView::new(scene).magnetism(cfg.markers(crate::magnet::MarkerVisibility::Always)),
        );
        let d = draws(&mut tree);
        assert_eq!(magnet_nodes(&mut tree, view_id, &[am, bm]), 0);
        d
    };

    let (scene, _a, am, _b, bm) = two_node_scene();
    let (cfg, _) = recording_config(source_to_target);
    let enabled = cfg.enabled_signal();
    let mut tree = WidgetTree::new();
    let view_id = tree
        .add(SceneView::new(scene).magnetism(cfg.markers(crate::magnet::MarkerVisibility::Always)));

    let on = draws(&mut tree);
    assert!(
        on > off_from_the_start,
        "the markers must draw something to begin with ({on} vs {off_from_the_start})"
    );
    assert!(
        magnet_nodes(&mut tree, view_id, &[am, bm]) > 0,
        "…and the magnets must be published"
    );

    enabled.set(false);
    assert_eq!(
        draws(&mut tree),
        off_from_the_start,
        "a live `enabled` flip must leave exactly the frame a view with magnetism \
         off from the start would have drawn"
    );
    assert_eq!(
        magnet_nodes(&mut tree, view_id, &[am, bm]),
        0,
        "…and must stop offering a screen reader magnets it will no longer act on"
    );

    enabled.set(true);
    assert_eq!(
        draws(&mut tree),
        on,
        "the flag is a toggle, not a one-way door"
    );
    assert!(magnet_nodes(&mut tree, view_id, &[am, bm]) > 0);
}
