// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What ends a transform gesture, and what each ending owes.
//!
//! The controller has three endings — a release, an `Esc`, and a contact the
//! system takes away — and until these tests only the first was exercised. Each
//! of the others had shipped with a hole that no functional test could see,
//! because a test that drags and releases never reaches them:
//!
//! * `Esc` was wired only into the keyboard route, and a pointer gesture never
//!   enters that route, so the design's own headline escape hatch did nothing
//!   from a mouse or a finger — while `begin` went on taking the focus whose
//!   only justification was delivering that key.
//! * A revoked contact went through `TransformRuntime::abort`, which drops the
//!   session and nothing else: the edge auto-pan kept tweening for up to thirty
//!   seconds, and `on_end` — documented as firing "committed **or cancelled**"
//!   — never fired at all, so an app that opens an overlay on `on_start` leaked
//!   it on every window deactivation.
//!
//! The rest measure two things a resize used to get wrong quietly: the minimum
//! size inflating an axis the handle does not drive, and whether the box the
//! preview draws is the box the commit writes.

use super::*;
use crate::flags::ItemFlags;
use crate::items::RectItem;
use crate::scene_model::SceneModel;
use crate::selection::{SceneSelection, SceneSelectionMode};
use crate::transform_session::{TransformConfig, TransformOutcome};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use teksilo_canvas::Point;
use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::pointer::{CancelReason, PointerId};
use teksilo_core::widget_builder::WidgetBuilder;

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
fn escape(tree: &mut WidgetTree) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
        text: None,
    });
}

/// Revoke the mouse the way the platform does — window deactivation, a modal
/// opening, a system gesture. Terminal: no `PointerUp` follows.
fn revoke_mouse(tree: &mut WidgetTree) {
    tree.cancel_pointer(
        PointerId::MOUSE,
        CancelReason::WindowDeactivated,
        &mut teksilo_core::window::NoopWindowOps,
    );
}

/// Apply whatever the gesture committed — the drain `build()` runs.
fn flush(tree: &mut WidgetTree, view_id: WidgetId) -> bool {
    tree.widget_as_any_mut(view_id)
        .and_then(|a| a.downcast_mut::<SceneView>())
        .expect("downcast")
        .flush_pending_transform()
}

/// One draggable, resizable square at (50, 50), 40 x 40, plus a selection model.
fn one_square() -> (SceneModel, ItemId, SceneSelection) {
    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)).fill(teksilo_tokens::Color::RED),
        Point::new(50.0, 50.0),
    );
    model.set_flag(a, ItemFlags::IS_DRAGGABLE, true);
    model.set_flag(a, ItemFlags::IS_RESIZABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a]);
    (model, a, selection)
}

fn mount(
    model: SceneModel,
    selection: SceneSelection,
    cfg: TransformConfig,
) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::with_model(model)
            .selection_model(selection)
            .transform_controller(cfg),
    );
    tree.layout(SizeProposal::exact(600.0, 400.0));
    (tree, view_id)
}

// ---------------------------------------------------------------------------
// Esc, from a pointer gesture
// ---------------------------------------------------------------------------

/// Reverting the early `Key::Escape` arm in `TransformDriver::handle_key`
/// reddens this: the session survives the key, the release commits it, and the
/// item ends up at (140, 110).
#[test]
fn escape_ends_a_pointer_transform_and_writes_nothing() {
    let (model, a, selection) = one_square();
    let (mut tree, view_id) = mount(model.clone(), selection, TransformConfig::new());

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(160.0, 130.0));
    {
        let view = view_handle(&tree, view_id);
        let d = view.transform_enabled().expect("controller installed");
        assert!(d.is_active(), "precondition: the press started a transform");
    }

    escape(&mut tree);
    {
        let view = view_handle(&tree, view_id);
        let d = view.transform_enabled().expect("controller installed");
        assert!(!d.is_active(), "Esc must end a pointer gesture");
    }

    // The rest of the sequence is inert: the release has nothing left to commit.
    up(&mut tree, Point::new(160.0, 130.0));
    assert!(
        !flush(&mut tree, view_id),
        "a cancelled gesture must post no commit"
    );
    let r = model.scene_rect(a).expect("resolves");
    assert!(
        (r.x - 50.0).abs() < 1e-3 && (r.y - 50.0).abs() < 1e-3,
        "the item must be where the gesture found it: {r:?}"
    );
}

/// `begin` takes the focus so that `Esc` can reach the view. That is a loan: a
/// caret in a card is the user's place in their own work, and a drag is not a
/// request to leave it.
#[test]
fn a_pointer_transform_gives_back_the_focus_it_borrowed() {
    // A focusable card standing in for the text field the steal used to kill.
    // It is the *selected, draggable* item, so the press lands on it — which is
    // the only shape in which `begin`'s steal changes anything at all: a press
    // on empty scene is focused to the view by the router before any handler
    // runs, and there is nothing to take.
    let model = SceneModel::new();
    let card = model.add_widget(
        FillWidget::new().focusable(true),
        Rect::new(40.0, 40.0, 120.0, 80.0),
    );
    model.set_flag(card, ItemFlags::IS_DRAGGABLE, true);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([card]);
    let (mut tree, view_id) = mount(model, selection, TransformConfig::new());

    let stops = tree.tab_stops_within(view_id);
    let card_id = *stops
        .iter()
        .find(|id| **id != view_id)
        .expect("the card is a tab stop");

    down(&mut tree, Point::new(100.0, 80.0));
    assert_eq!(
        tree.focused(),
        Some(card_id),
        "precondition: the press put focus on the card, as it would on a field"
    );
    moved(&mut tree, Point::new(130.0, 100.0));
    moved(&mut tree, Point::new(160.0, 130.0));
    assert_eq!(
        tree.focused(),
        Some(view_id),
        "the gesture does take focus — Esc has to be able to arrive"
    );
    up(&mut tree, Point::new(160.0, 130.0));

    assert_eq!(
        tree.focused(),
        Some(card_id),
        "and it has to give it back at the end of the gesture"
    );
}

// ---------------------------------------------------------------------------
// A revoked contact
// ---------------------------------------------------------------------------

/// Routing the unwind through `TransformRuntime::abort` again reddens this: the
/// pan tween runs on to its far target with no input at all.
#[test]
fn a_revoked_contact_stops_the_edge_auto_pan() {
    let (model, _a, selection) = one_square();
    let (mut tree, view_id) = mount(model, selection, TransformConfig::new());

    down(&mut tree, Point::new(70.0, 70.0));
    // The first move only *starts* the gesture — it carries the press point, so
    // the anchor and the current position coincide. The second is the one the
    // controller updates on, and it lands in the trailing edge band (the
    // viewport is 600 wide, the band 28), arming a tween that keeps panning
    // under a pointer that has stopped moving.
    moved(&mut tree, Point::new(200.0, 150.0));
    moved(&mut tree, Point::new(585.0, 200.0));
    let armed = {
        let view = view_handle(&tree, view_id);
        assert!(
            view.transform_rt.edge_panning.get(),
            "precondition: the edge band armed an auto-pan"
        );
        view.pan()
    };

    revoke_mouse(&mut tree);
    let at_cancel = view_handle(&tree, view_id).pan();
    tree.advance_time(Duration::from_millis(1500));
    let later = view_handle(&tree, view_id).pan();

    assert!(
        !view_handle(&tree, view_id).transform_rt.edge_panning.get(),
        "a revoked contact leaves no auto-pan armed"
    );
    assert!(
        (later.x - at_cancel.x).abs() < 1.0 && (later.y - at_cancel.y).abs() < 1.0,
        "the view must stop where the contact was taken away: armed {armed:?}, \
         at cancel {at_cancel:?}, 1.5 s later {later:?}"
    );
}

/// `TransformConfig::on_end` is documented to fire "committed **or
/// cancelled**". The one ending that is not a user release used to deliver
/// nothing, so an app that pairs `on_start` with `on_end` leaked whatever it
/// opened.
#[test]
fn a_revoked_contact_delivers_on_end_cancelled() {
    let (model, _a, selection) = one_square();
    let log: Rc<RefCell<Vec<TransformOutcome>>> = Rc::new(RefCell::new(Vec::new()));
    let starts = Rc::new(std::cell::Cell::new(0u32));
    let sink = log.clone();
    let counter = starts.clone();
    let cfg = TransformConfig::new()
        .on_start(move |_s, _c| counter.set(counter.get() + 1))
        .on_end(move |_s, outcome, _c| sink.borrow_mut().push(outcome));
    let (mut tree, _view_id) = mount(model, selection, cfg);

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(100.0, 90.0));
    moved(&mut tree, Point::new(120.0, 100.0));
    assert_eq!(starts.get(), 1, "precondition: on_start fired");
    assert!(
        log.borrow().is_empty(),
        "precondition: nothing has ended yet"
    );

    revoke_mouse(&mut tree);

    assert_eq!(
        *log.borrow(),
        vec![TransformOutcome::Cancelled],
        "a revoked contact ends the gesture, and the app has to hear about it"
    );
}

/// The two cancel arms must not double-deliver: a revocation reaches a
/// `SceneView` twice — as the drag recognizer's own `Cancelled` phase and as
/// the node's `on_pointer_cancel` — and both hand over to `DragUnwind`.
#[test]
fn a_revoked_contact_ends_the_gesture_exactly_once() {
    let (model, _a, selection) = one_square();
    let log: Rc<RefCell<Vec<TransformOutcome>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let cfg = TransformConfig::new().on_end(move |_s, outcome, _c| sink.borrow_mut().push(outcome));
    let (mut tree, _view_id) = mount(model, selection, cfg);

    down(&mut tree, Point::new(70.0, 70.0));
    moved(&mut tree, Point::new(100.0, 90.0));
    moved(&mut tree, Point::new(120.0, 100.0));
    revoke_mouse(&mut tree);

    assert_eq!(log.borrow().len(), 1, "{:?}", log.borrow());
}

// ---------------------------------------------------------------------------
// The preview and the box it commits
// ---------------------------------------------------------------------------

/// A resize preview is a visual scale on the lightweight tier and a relayout on
/// the heavyweight one, but the **box** must be the same on both and must be
/// the box the release writes — otherwise the selection jumps at the instant
/// the handle is let go.
#[test]
fn a_lightweight_resize_previews_the_box_it_commits() {
    let (model, a, selection) = one_square();
    let cfg = TransformConfig::new().padding(6.0).handle_px(9.0);
    let (mut tree, view_id) = mount(model.clone(), selection, cfg);

    // The item box is 50..90 square, so the padded outline's bottom-trailing
    // corner — where that handle sits — is at (96, 96).
    down(&mut tree, Point::new(96.0, 96.0));
    // Two moves: the first starts the gesture at the press point, the second
    // is the first sample that actually carries the handle.
    moved(&mut tree, Point::new(116.0, 106.0));
    moved(&mut tree, Point::new(136.0, 116.0));

    let previewed = {
        let view = view_handle(&tree, view_id);
        let (roots, xform) = view
            .transform_preview()
            .expect("a live resize previews something");
        assert!(roots.contains(&a), "the grabbed item is in the preview set");
        xform.apply_rect(model.scene_rect(a).expect("resolves"))
    };

    up(&mut tree, Point::new(136.0, 116.0));
    assert!(flush(&mut tree, view_id), "the resize must commit");
    let committed = model.scene_rect(a).expect("resolves");

    assert!(
        (previewed.x - committed.x).abs() < 1e-2
            && (previewed.y - committed.y).abs() < 1e-2
            && (previewed.width - committed.width).abs() < 1e-2
            && (previewed.height - committed.height).abs() < 1e-2,
        "the preview must draw the box the commit writes: previewed {previewed:?}, \
         committed {committed:?}"
    );
    assert!(
        committed.width > 41.0,
        "precondition: the drag actually resized something ({committed:?})"
    );
}

// ---------------------------------------------------------------------------
// The chrome memo
// ---------------------------------------------------------------------------

fn chrome_computes(tree: &WidgetTree, view_id: WidgetId) -> u64 {
    view_handle(tree, view_id)
        .transform_rt
        .chrome_computes
        .get()
}

/// The free hover — every `PointerMove` an application makes over the view —
/// must not re-derive the frame. Deleting the memo in `TransformDriver::chrome`
/// reddens the first assertion; keying it on something that never changes
/// reddens the last two.
#[test]
fn a_hover_over_an_unchanged_selection_resolves_the_frame_once() {
    let (model, a, selection) = one_square();
    let (mut tree, view_id) = mount(model.clone(), selection.clone(), TransformConfig::new());

    // One pass to settle everything the mount left lazy.
    tree.pointer_move(Point::new(300.0, 300.0));
    let settled = chrome_computes(&tree, view_id);

    for i in 0..20 {
        tree.pointer_move(Point::new(300.0 + i as f32, 300.0));
    }
    assert_eq!(
        chrome_computes(&tree, view_id),
        settled,
        "a pointer moving over an unchanged selection re-derives nothing"
    );

    // …and the memo is not simply frozen. A model change retires it,
    model.set_local_pos(a, Point::new(120.0, 120.0));
    tree.pointer_move(Point::new(301.0, 300.0));
    let after_model = chrome_computes(&tree, view_id);
    assert!(
        after_model > settled,
        "a moved item must re-derive the frame"
    );

    // …and so does a change of selection.
    selection.clear();
    tree.pointer_move(Point::new(302.0, 300.0));
    assert!(
        chrome_computes(&tree, view_id) > after_model,
        "a changed selection must re-derive the frame"
    );
}
