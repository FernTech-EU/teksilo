// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The hit snapshots are cached, and the cache is not allowed to be wrong.
//!
//! Two claims, and each needs its own kind of evidence:
//!
//! * **The cost claim.** A pan runs a full layout pass and must not touch the
//!   snapshots. Measured in *branches taken*, not nanoseconds
//!   ([`HitSnapshotSync::tally`]), so the assertion is exact and survives a
//!   loaded CI runner. Every test here that says "the answer is still right"
//!   also says *which branch produced it* — otherwise it would pass just as
//!   happily with the whole mechanism deleted and the snapshots rebuilt every
//!   pass.
//! * **The correctness claim.** Everything is driven through `WidgetTree`'s
//!   real pointer path, so a stale row shows up as a tap landing on the wrong
//!   item (or on nothing), which is how a user would meet it.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal, Transform2D};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

use crate::flags::ItemFlags;
use crate::item::ItemId;
use crate::items::RectItem;
use crate::scene::Scene;
use crate::scene_model::SceneModel;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::transform_session::TransformConfig;
use crate::view::SceneView;
use crate::view::hit_snapshot::{HitSnapshotSync, PlanTally};

// ---------------------------------------------------------------------- rig

const VIEWPORT: (f32, f32) = (400.0, 300.0);

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

fn counter() -> Rc<Cell<u32>> {
    Rc::new(Cell::new(0))
}

/// A 40×40 tappable tile at `pos`.
fn tile(scene: &mut Scene, pos: Point, taps: &Rc<Cell<u32>>) -> ItemId {
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        pos,
    );
    let t = taps.clone();
    scene
        .handlers_mut(id)
        .unwrap()
        .on_tap(move |_pt, _ctx| t.set(t.get() + 1));
    id
}

/// A mounted view plus the handles a test needs to watch it: the shared sync
/// record, and a `SceneModel` to mutate through.
struct Rig {
    tree: WidgetTree,
    model: SceneModel,
    sync: Rc<std::cell::RefCell<HitSnapshotSync>>,
    pan_x: teksilo_core::signal::Signal<f32>,
    zoom: teksilo_core::signal::Signal<f32>,
    view_id: WidgetId,
}

impl Rig {
    fn mount(scene: Scene) -> Self {
        Self::mount_view(scene, |view| view)
    }

    /// The same rig with a selection and a transform controller installed —
    /// the arrangement a live move of a selected item runs through.
    fn mount_transforming(scene: Scene, selection: SceneSelection) -> Self {
        Self::mount_view(scene, |view| {
            view.selection_model(selection)
                .transform_controller(TransformConfig::new())
        })
    }

    fn mount_view(scene: Scene, configure: impl FnOnce(SceneView) -> SceneView) -> Self {
        let model = SceneModel::from_scene(scene);
        let view = configure(SceneView::with_model(model.clone()));
        let sync = view.hit_sync.clone();
        let pan_x = view.pan_x_signal();
        let zoom = view.zoom_signal();
        let mut tree = WidgetTree::new();
        let view_id = tree.add(view);
        let mut rig = Self {
            tree,
            model,
            sync,
            pan_x,
            zoom,
            view_id,
        };
        rig.layout();
        rig
    }

    /// Apply whatever a finished gesture committed — the drain `build()` runs.
    fn flush_transform(&mut self) -> bool {
        self.tree
            .widget_as_any_mut(self.view_id)
            .and_then(|a| a.downcast_mut::<SceneView>())
            .expect("downcast")
            .flush_pending_transform()
    }

    fn layout(&mut self) {
        self.tree
            .layout(SizeProposal::exact(VIEWPORT.0, VIEWPORT.1));
    }

    fn tally(&self) -> PlanTally {
        self.sync.borrow().tally
    }

    /// The branch counts a block of work took, so a test states a delta rather
    /// than an absolute that shifts whenever the fixture gains an item.
    fn tally_delta(&self, before: PlanTally) -> PlanTally {
        let now = self.tally();
        PlanTally {
            reused: now.reused - before.reused,
            patched: now.patched - before.patched,
            rebuilt: now.rebuilt - before.rebuilt,
        }
    }
}

// -------------------------------------------------------------- the pan case

/// The defect this whole mechanism exists for: a pan touches no item, so it
/// must not touch the snapshots — and the snapshots must still answer.
#[test]
fn a_pan_reuses_the_snapshots_and_they_still_answer() {
    let taps = counter();
    let mut scene = Scene::new();
    tile(&mut scene, Point::new(100.0, 100.0), &taps);
    let mut rig = Rig::mount(scene);

    let before = rig.tally();
    for i in 1..=5 {
        rig.pan_x.set(i as f32 * 10.0);
        rig.layout();
    }
    assert_eq!(
        rig.tally_delta(before),
        PlanTally {
            reused: 5,
            patched: 0,
            rebuilt: 0
        },
        "five pan samples must reuse the snapshots five times and rebuild \
         nothing — this is the measured 16 ms/sample defect, asserted as a \
         branch count so it cannot come back quietly",
    );

    // …and the reused snapshot is still correct under the new camera: the item
    // sits at scene (100,100)–(140,140) and the view is panned by +50.
    click(&mut rig.tree, Point::new(170.0, 120.0));
    assert_eq!(taps.get(), 1, "the panned item must still take its tap");
    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(
        taps.get(),
        1,
        "and the point it used to occupy must now miss — a snapshot frozen in \
         *screen* space would have taken this one",
    );
}

/// A zoom is the same argument: the narrow phase takes the view scale as a
/// call-time argument, so nothing in a snapshot depends on it.
#[test]
fn a_zoom_reuses_the_snapshots_and_they_still_answer() {
    let taps = counter();
    let mut scene = Scene::new();
    tile(&mut scene, Point::new(100.0, 100.0), &taps);
    let mut rig = Rig::mount(scene);
    let before = rig.tally();
    rig.zoom.set(2.0);
    rig.layout();
    assert_eq!(
        rig.tally_delta(before).rebuilt,
        0,
        "a zoom must not rebuild the snapshots",
    );

    // At zoom 2 the item's scene rect (100,100)–(140,140) covers screen
    // (200,200)–(280,280).
    click(&mut rig.tree, Point::new(240.0, 240.0));
    assert_eq!(taps.get(), 1, "the zoomed item must still take its tap");
}

// -------------------------------------------------------- the transform case

/// A live group move is the pan argument one step further in. A transform
/// sample previews an affine and writes no item, so — like a pan — it must
/// reuse the snapshots; and the row it is previewing must be patched exactly
/// once when the gesture finally commits.
///
/// `tests/transform_scaling_probe.rs` makes the first half of that claim in
/// nanoseconds. This is its exact half, which the pan and zoom cases above
/// each already have: without it the wall-clock gate is the only thing
/// standing between a per-sample O(N) rebuild and a release, and it is a
/// quotient of two medians taken on a shared CI runner.
#[test]
fn a_transform_sample_reuses_the_snapshots_and_the_commit_patches_once() {
    let taps = counter();
    let mut scene = Scene::new();
    let id = tile(&mut scene, Point::new(100.0, 100.0), &taps);
    scene.set_flag(id, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([id]);
    let mut rig = Rig::mount_transforming(scene, selection);

    // Latch the move: press inside the tile, then travel past the slop.
    rig.tree.pointer_move(Point::new(120.0, 120.0));
    rig.tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(120.0, 120.0),
        PointerButton::Primary,
        Modifiers::default(),
    ));
    rig.tree
        .dispatch_event(WidgetEvent::pointer_move(Point::new(150.0, 120.0)));
    rig.layout();

    let before = rig.tally();
    for i in 1..=5 {
        rig.tree
            .dispatch_event(WidgetEvent::pointer_move(Point::new(
                150.0 + i as f32 * 10.0,
                120.0,
            )));
        rig.layout();
    }
    assert_eq!(
        rig.tally_delta(before),
        PlanTally {
            reused: 5,
            patched: 0,
            rebuilt: 0
        },
        "five samples of a live move must reuse the snapshots five times and \
         write nothing — the preview is an affine, not an edit",
    );

    // The commit is the one write, and it is a patch: a move changes no
    // membership and no paint order.
    let before = rig.tally();
    rig.tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(200.0, 120.0),
        PointerButton::Primary,
        Modifiers::default(),
    ));
    assert!(rig.flush_transform(), "the release must post a commit");
    rig.layout();
    assert_eq!(
        rig.tally_delta(before),
        PlanTally {
            reused: 0,
            patched: 1,
            rebuilt: 0
        },
        "the release is the one write, and a move changes no membership and no \
         paint order, so it must patch rather than rebuild",
    );

    // …and the row followed. The press travelled +80, so the tile's scene rect
    // went from (100,100)-(140,140) to (180,100)-(220,140).
    let r = rig.model.scene_rect(id).expect("resolves");
    assert!(
        (r.x - 180.0).abs() < 1e-3,
        "precondition for the tap below: the commit must have moved the tile, \
         got {r:?}"
    );
    let base = taps.get();
    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(
        taps.get(),
        base,
        "the point the tile used to occupy must now miss",
    );
    click(&mut rig.tree, Point::new(200.0, 120.0));
    assert_eq!(taps.get(), base + 1, "and the moved tile must take its tap");
}

// ------------------------------------------------------------ the patch case

/// Moving one item rewrites one row, and the tap follows it.
#[test]
fn moving_an_item_patches_its_row() {
    let taps = counter();
    let mut scene = Scene::new();
    let id = tile(&mut scene, Point::new(100.0, 100.0), &taps);
    let mut rig = Rig::mount(scene);

    let before = rig.tally();
    rig.model.set_local_pos(id, Point::new(200.0, 100.0));
    rig.layout();
    assert_eq!(
        rig.tally_delta(before),
        PlanTally {
            reused: 0,
            patched: 1,
            rebuilt: 0
        },
        "a plain move changes no membership and no paint order, so it must \
         patch rather than rebuild",
    );

    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(taps.get(), 0, "the item is no longer where it was");
    click(&mut rig.tree, Point::new(220.0, 120.0));
    assert_eq!(taps.get(), 1, "the patched row answers at the new position");
}

/// The trap in patching by id: a change names the item that moved, but a
/// *parent* move shifts every descendant's scene transform and emits nothing
/// for them. A patch that took the change at face value would leave every child
/// hit-testable at its old place.
#[test]
fn moving_a_parent_patches_the_whole_subtree() {
    let child_taps = counter();
    let grandchild_taps = counter();
    let mut scene = Scene::new();
    let parent = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
    let child = tile(&mut scene, Point::new(50.0, 50.0), &child_taps);
    let grandchild = tile(&mut scene, Point::new(20.0, 20.0), &grandchild_taps);
    scene.set_item_parent(child, Some(parent));
    scene.set_item_parent(grandchild, Some(child));
    let mut rig = Rig::mount(scene);

    // Grandchild starts at scene (70,70)–(110,110).
    click(&mut rig.tree, Point::new(90.0, 90.0));
    assert_eq!(grandchild_taps.get(), 1, "fixture sanity");

    let before = rig.tally();
    rig.model.set_local_pos(parent, Point::new(100.0, 0.0));
    rig.layout();
    assert_eq!(
        rig.tally_delta(before).patched,
        1,
        "one move, one patch — the subtree must be reached from the patch, not \
         from a rebuild",
    );
    assert_eq!(rig.tally_delta(before).rebuilt, 0);

    click(&mut rig.tree, Point::new(90.0, 90.0));
    assert_eq!(
        grandchild_taps.get(),
        1,
        "the grandchild moved with its ancestor and must not still answer at \
         its old scene position",
    );
    click(&mut rig.tree, Point::new(190.0, 90.0));
    assert_eq!(
        grandchild_taps.get(),
        2,
        "…and must answer at the new one — two levels below the item the change \
         actually named",
    );
    // (155,55) is inside the moved child — child spans (150,50)–(190,90) — and
    // clear of the grandchild, which starts at (170,70) and would otherwise win
    // the overlap by being the later entry.
    click(&mut rig.tree, Point::new(155.0, 55.0));
    assert_eq!(child_taps.get(), 1, "the direct child moved too");
}

/// A transform is geometry as much as a position is, and it composes down the
/// same chain.
#[test]
fn transforming_a_parent_patches_the_subtree() {
    let child_taps = counter();
    let mut scene = Scene::new();
    let parent = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
    let child = tile(&mut scene, Point::new(20.0, 20.0), &child_taps);
    scene.set_item_parent(child, Some(parent));
    let mut rig = Rig::mount(scene);

    let before = rig.tally();
    rig.model
        .set_transform(parent, Transform2D::translate(60.0, 0.0));
    rig.layout();
    assert_eq!(rig.tally_delta(before).patched, 1);

    click(&mut rig.tree, Point::new(30.0, 30.0));
    assert_eq!(child_taps.get(), 0, "the child is no longer there");
    click(&mut rig.tree, Point::new(90.0, 30.0));
    assert_eq!(child_taps.get(), 1, "…it is here");
}

// ---------------------------------------------------------- the rebuild cases

/// `z` is the sort key the snapshots are ordered by, so a change to it cannot
/// be a patch: the row has to move.
#[test]
fn raising_z_rebuilds_and_re_sorts() {
    let bottom_taps = counter();
    let top_taps = counter();
    let mut scene = Scene::new();
    let bottom = tile(&mut scene, Point::new(100.0, 100.0), &bottom_taps);
    let top = tile(&mut scene, Point::new(100.0, 100.0), &top_taps);
    scene.set_z(top, 1.0);
    let mut rig = Rig::mount(scene);

    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(
        (bottom_taps.get(), top_taps.get()),
        (0, 1),
        "fixture sanity"
    );

    let before = rig.tally();
    rig.model.set_z(bottom, 2.0);
    rig.layout();
    assert_eq!(
        rig.tally_delta(before).rebuilt,
        1,
        "a z change moves the row in the sort, which a per-row patch cannot do",
    );

    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(
        (bottom_taps.get(), top_taps.get()),
        (1, 1),
        "the raised item is topmost now and takes the tap",
    );
}

/// Hiding an item removes its row. `is_hit_testable` chains visibility up the
/// parent chain, so this is a membership change for a whole subtree.
#[test]
fn hiding_an_item_rebuilds_and_drops_its_row() {
    let taps = counter();
    let mut scene = Scene::new();
    let id = tile(&mut scene, Point::new(100.0, 100.0), &taps);
    let mut rig = Rig::mount(scene);

    let before = rig.tally();
    rig.model.set_visible(id, false);
    rig.layout();
    assert!(rig.tally_delta(before).rebuilt >= 1);

    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(taps.get(), 0, "an invisible item is not hit-tested");
}

/// Hiding a *parent* must drop the child's row too — the membership rule that
/// makes visibility a rebuild rather than a per-item patch.
#[test]
fn hiding_a_parent_drops_its_children_from_the_snapshot() {
    let child_taps = counter();
    let mut scene = Scene::new();
    let parent = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
    let child = tile(&mut scene, Point::new(100.0, 100.0), &child_taps);
    scene.set_item_parent(child, Some(parent));
    let mut rig = Rig::mount(scene);

    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(child_taps.get(), 1, "fixture sanity");

    rig.model.set_visible(parent, false);
    rig.layout();
    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(
        child_taps.get(),
        1,
        "a child of a hidden parent is not hit-tested either",
    );
}

/// `handlers_mut` used to be the one door in the model that changed observable
/// state and told nobody. A cache over the change stream cannot survive that,
/// so it now fires `ItemChange::HandlersChanged` — and this is the test that
/// goes red if that emission is removed.
#[test]
fn editing_handlers_after_the_first_layout_is_picked_up() {
    let mut scene = Scene::new();
    // No handler set at all to begin with — the row exists (every hit-testable
    // entry is in the snapshot), but carries `handlers: None`.
    let id = scene.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(100.0, 100.0),
    );
    let mut rig = Rig::mount(scene);

    click(&mut rig.tree, Point::new(120.0, 120.0));

    let taps = counter();
    let t = taps.clone();
    let before = rig.tally();
    rig.model.with_handlers_mut(id, |h| {
        h.on_tap(move |_pt, _ctx| t.set(t.get() + 1));
    });
    rig.layout();
    assert_eq!(
        rig.tally_delta(before).rebuilt,
        1,
        "a handler edit must invalidate the snapshot that carries a clone of \
         the handler set",
    );

    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(
        taps.get(),
        1,
        "the handler installed after the first layout must reach the pointer",
    );
}

/// Adding and removing items are membership changes, and the snapshot must
/// follow both — including the one where a removed item's row would otherwise
/// keep answering.
#[test]
fn adding_and_removing_items_rebuilds() {
    let first_taps = counter();
    let second_taps = counter();
    let mut scene = Scene::new();
    let first = tile(&mut scene, Point::new(100.0, 100.0), &first_taps);
    let mut rig = Rig::mount(scene);

    let second = {
        let t = second_taps.clone();
        let id = rig.model.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
            Point::new(200.0, 100.0),
        );
        rig.model.with_handlers_mut(id, move |h| {
            h.on_tap(move |_pt, _ctx| t.set(t.get() + 1));
        });
        id
    };
    rig.layout();
    click(&mut rig.tree, Point::new(220.0, 120.0));
    assert_eq!(second_taps.get(), 1, "an added item enters the snapshot");

    rig.model.remove(first);
    rig.layout();
    click(&mut rig.tree, Point::new(120.0, 120.0));
    assert_eq!(first_taps.get(), 0, "a removed item leaves it");
    let _ = second;
}

// --------------------------------------------------- the "did I see it" guard

/// The initial population is emitted before this view's observer exists, so the
/// view has been delivered *none* of it. The first refresh must therefore read
/// the model rather than trust an empty dirty set — which is exactly the case a
/// counter comparison written as absolute equality would get wrong.
#[test]
fn the_first_refresh_reads_the_model_it_never_observed() {
    let taps = counter();
    let mut scene = Scene::new();
    tile(&mut scene, Point::new(100.0, 100.0), &taps);
    tile(&mut scene, Point::new(200.0, 100.0), &taps);
    let rig = Rig::mount(scene);

    assert_eq!(
        rig.tally(),
        PlanTally {
            reused: 0,
            patched: 0,
            rebuilt: 1
        },
        "the first layout must rebuild",
    );
    let mut rig = rig;
    click(&mut rig.tree, Point::new(120.0, 120.0));
    click(&mut rig.tree, Point::new(220.0, 120.0));
    assert_eq!(
        taps.get(),
        2,
        "both items added before the view existed are hit-testable",
    );
}

/// Two views over one model keep independent caches, and a mutation has to
/// reach both. A per-view record that leaked between views — or one view's
/// refresh satisfying another's — would show up here as a stale row in the
/// second view only.
#[test]
fn a_second_view_over_the_same_model_stays_current() {
    let taps = counter();
    let mut scene = Scene::new();
    let id = tile(&mut scene, Point::new(100.0, 100.0), &taps);
    let model = SceneModel::from_scene(scene);

    let mount = |model: &SceneModel| {
        let view = SceneView::with_model(model.clone());
        let sync = view.hit_sync.clone();
        let mut tree = WidgetTree::new();
        tree.add(view);
        tree.layout(SizeProposal::exact(VIEWPORT.0, VIEWPORT.1));
        (tree, sync)
    };
    let (mut tree_a, sync_a) = mount(&model);
    let (mut tree_b, sync_b) = mount(&model);

    let before_a = sync_a.borrow().tally;
    let before_b = sync_b.borrow().tally;
    model.set_local_pos(id, Point::new(200.0, 100.0));
    tree_a.layout(SizeProposal::exact(VIEWPORT.0, VIEWPORT.1));
    tree_b.layout(SizeProposal::exact(VIEWPORT.0, VIEWPORT.1));
    assert_eq!(
        sync_a.borrow().tally.patched - before_a.patched,
        1,
        "the first view patches",
    );
    assert_eq!(
        sync_b.borrow().tally.patched - before_b.patched,
        1,
        "and so does the second, from its own record",
    );

    click(&mut tree_a, Point::new(220.0, 120.0));
    click(&mut tree_b, Point::new(220.0, 120.0));
    assert_eq!(taps.get(), 2, "both views hit the moved item");
}
