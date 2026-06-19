// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Multi-view: two `SceneView`s over one shared [`SceneModel`].
//!
//! A shared `Rc<RefCell<Scene>>` handle plus a per-view
//! heavyweight delegate. Mutating the model once must reconcile **every**
//! attached view's arena independently; each view builds its own widget
//! instances; cameras and per-view default selection stay independent; a
//! shared `SceneSelection` is observed by all panes.

use super::*;
use crate::scene_model::SceneModel;
use crate::selection::{SceneSelection, SceneSelectionMode};
use bastyde_canvas::Vec2;
use bastyde_widgets::{Expand, HStack};
use std::cell::Cell;
use std::rc::Rc;

fn viewport() -> SizeProposal {
    SizeProposal::exact(800.0, 600.0)
}

/// A delegate that builds a `FillWidget` per `u32` payload and counts its
/// invocations (so tests can prove a rebuild re-ran it).
fn counting_delegate(
    counter: Rc<Cell<usize>>,
) -> impl Fn(&u32, ItemId) -> Box<dyn Widget> + 'static {
    move |_payload, _id| {
        counter.set(counter.get() + 1);
        Box::new(FillWidget::new())
    }
}

/// Build a tree with two `SceneView`s over `model`, wrapped in an `HStack` so
/// both are laid out. Returns `(tree, view_a, view_b, count_a, count_b)`.
fn two_views(
    model: &SceneModel,
    sel_a: SceneSelection,
    sel_b: SceneSelection,
) -> (
    WidgetTree,
    WidgetId,
    WidgetId,
    Rc<Cell<usize>>,
    Rc<Cell<usize>>,
) {
    let count_a = Rc::new(Cell::new(0));
    let count_b = Rc::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    let a = tree.add(
        SceneView::with_model(model.clone())
            .selection_model(sel_a)
            .delegate_typed::<u32>(counting_delegate(count_a.clone())),
    );
    let b = tree.add(
        SceneView::with_model(model.clone())
            .selection_model(sel_b)
            .delegate_typed::<u32>(counting_delegate(count_b.clone())),
    );
    tree.add(
        HStack::new()
            .child(Expand::new().child_id(a))
            .child(Expand::new().child_id(b)),
    );
    tree.layout(viewport());
    (tree, a, b, count_a, count_b)
}

fn view_ref<R>(tree: &WidgetTree, id: WidgetId, f: impl FnOnce(&SceneView) -> R) -> R {
    let v = tree
        .widget_as_any(id)
        .and_then(|a| a.downcast_ref::<SceneView>())
        .expect("view is a SceneView");
    f(v)
}

#[test]
fn both_views_materialise_delegated_entry_independently() {
    let model = SceneModel::new();
    model.add_widget_item(1u32, Rect::new(0.0, 0.0, 100.0, 50.0));
    let (tree, a, b, ca, cb) = two_views(
        &model,
        SceneSelection::new(SceneSelectionMode::None),
        SceneSelection::new(SceneSelectionMode::None),
    );
    assert_eq!(tree.children(a).len(), 1, "view A built the card");
    assert_eq!(tree.children(b).len(), 1, "view B built its OWN card");
    assert_eq!(ca.get(), 1);
    assert_eq!(cb.get(), 1);
    // Independent arena instances.
    assert_ne!(tree.children(a)[0], tree.children(b)[0]);
}

#[test]
fn set_local_pos_re_places_in_both_views() {
    let model = SceneModel::new();
    let id = model.add_widget_item(1u32, Rect::new(0.0, 0.0, 100.0, 50.0));
    let (mut tree, a, b, _ca, _cb) = two_views(
        &model,
        SceneSelection::new(SceneSelectionMode::None),
        SceneSelection::new(SceneSelectionMode::None),
    );
    model.set_local_pos(id, Point::new(300.0, 200.0));
    tree.layout(viewport());

    let ba = tree.bounds(tree.children(a)[0]);
    let bb = tree.bounds(tree.children(b)[0]);
    assert_eq!((ba.x, ba.y), (300.0, 200.0), "view A re-placed");
    assert_eq!((bb.x, bb.y), (300.0, 200.0), "view B re-placed");
}

#[test]
fn set_payload_rebuilds_the_card_in_both_views() {
    let model = SceneModel::new();
    let id = model.add_widget_item(1u32, Rect::new(0.0, 0.0, 100.0, 50.0));
    let (mut tree, _a, _b, ca, cb) = two_views(
        &model,
        SceneSelection::new(SceneSelectionMode::None),
        SceneSelection::new(SceneSelectionMode::None),
    );
    assert_eq!(ca.get(), 1, "delegate ran once at first build (A)");
    assert_eq!(cb.get(), 1, "delegate ran once at first build (B)");

    model.set_payload(id, 2u32);
    tree.layout(viewport());

    assert_eq!(
        ca.get(),
        2,
        "view A re-invoked its delegate for the changed card"
    );
    assert_eq!(cb.get(), 2, "view B re-invoked its delegate too");
}

#[test]
fn remove_reaps_from_both_views() {
    let model = SceneModel::new();
    let id = model.add_widget_item(1u32, Rect::new(0.0, 0.0, 100.0, 50.0));
    model.add_widget_item(2u32, Rect::new(120.0, 0.0, 100.0, 50.0));
    let (mut tree, a, b, _ca, _cb) = two_views(
        &model,
        SceneSelection::new(SceneSelectionMode::None),
        SceneSelection::new(SceneSelectionMode::None),
    );
    assert_eq!(tree.children(a).len(), 2);
    assert_eq!(tree.children(b).len(), 2);

    model.remove(id);
    tree.layout(viewport());

    assert_eq!(tree.children(a).len(), 1, "view A reaped the removed card");
    assert_eq!(
        tree.children(b).len(),
        1,
        "view B reaped it too (no orphan)"
    );
    view_ref(&tree, a, |v| assert_eq!(v.widget_id_for(id), None));
    view_ref(&tree, b, |v| assert_eq!(v.widget_id_for(id), None));
}

#[test]
fn cameras_are_independent_per_view() {
    let model = SceneModel::new();
    model.add_widget_item(1u32, Rect::new(0.0, 0.0, 100.0, 50.0));
    let (mut tree, a, b, _ca, _cb) = two_views(
        &model,
        SceneSelection::new(SceneSelectionMode::None),
        SceneSelection::new(SceneSelectionMode::None),
    );
    // Pan view A; view B's camera must not move.
    {
        let v = tree
            .widget_as_any_mut(a)
            .and_then(|x| x.downcast_mut::<SceneView>())
            .expect("view A");
        v.set_pan(Vec2::new(200.0, 0.0));
    }
    tree.layout(viewport());
    let pan_a = view_ref(&tree, a, |v| v.pan());
    let pan_b = view_ref(&tree, b, |v| v.pan());
    assert_eq!(pan_a.x, 200.0, "view A panned");
    assert_eq!(pan_b.x, 0.0, "view B's independent camera is unaffected");
}

#[test]
fn add_widget_once_is_single_view_only() {
    // `add_widget` (the `Once` path) puts a single widget instance into the
    // model; only the first view to build drains it. A second view over the
    // same model produces no child for it (and must not panic).
    let model = SceneModel::new();
    model.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 100.0, 50.0));
    let mut tree = WidgetTree::new();
    let a = tree.add(SceneView::with_model(model.clone()));
    let b = tree.add(SceneView::with_model(model.clone()));
    tree.add(
        HStack::new()
            .child(Expand::new().child_id(a))
            .child(Expand::new().child_id(b)),
    );
    tree.layout(viewport());
    assert_eq!(
        tree.children(a).len(),
        1,
        "first/owning view materialises the Once widget"
    );
    assert_eq!(
        tree.children(b).len(),
        0,
        "second view gets nothing — Once is single-view"
    );
}

#[test]
fn shared_selection_is_the_same_signal_for_both_views() {
    let model = SceneModel::new();
    let shared = SceneSelection::new(SceneSelectionMode::Multi);
    let (tree, a, b, _ca, _cb) = two_views(&model, shared.clone(), shared.clone());
    let sig_a = view_ref(&tree, a, |v| v.selection().selection_signal());
    let sig_b = view_ref(&tree, b, |v| v.selection().selection_signal());
    assert!(
        Signal::same(&sig_a, &sig_b),
        "panes given the shared SceneSelection observe one signal"
    );
}

#[test]
fn per_view_default_selection_is_independent() {
    let model = SceneModel::new();
    // Distinct (not shared) selections — the per-view default.
    let (tree, a, b, _ca, _cb) = two_views(
        &model,
        SceneSelection::new(SceneSelectionMode::Multi),
        SceneSelection::new(SceneSelectionMode::Multi),
    );
    let sig_a = view_ref(&tree, a, |v| v.selection().selection_signal());
    let sig_b = view_ref(&tree, b, |v| v.selection().selection_signal());
    assert!(
        !Signal::same(&sig_a, &sig_b),
        "independent selections are distinct signals"
    );
}

#[test]
fn drag_on_delegated_heavyweight_tappable_card_starts_marquee() {
    // Faithful to scene_corkboard: with_model + delegate_typed builds a
    // heavyweight card carrying on_tap selection. Dragging from on top of it
    // must start a marquee — the card's tap must NOT shadow the scene on_drag
    // (the cross-widget tap/drag disambiguation reaches the delegate path).
    use bastyde_canvas::Point;
    use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
    use bastyde_core::widget_builder::WidgetBuilder;

    let model = SceneModel::new();
    model.add_widget_item(1u32, Rect::new(40.0, 40.0, 120.0, 80.0));

    let mut tree = WidgetTree::new();
    let view_id = tree.add(
        SceneView::with_model(model.clone())
            .selection_mode(SceneSelectionMode::Multi)
            .delegate_typed::<u32>(|_p, _id| Box::new(FillWidget::new().on_tap(|_p, _c| {}))),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    tree.pointer_move(Point::new(70.0, 70.0));
    tree.dispatch_event(WidgetEvent::PointerDown {
        position: Point::new(70.0, 70.0),
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    });
    tree.dispatch_event(WidgetEvent::PointerMove {
        position: Point::new(120.0, 120.0),
    });

    view_ref(&tree, view_id, |v| {
        assert!(
            v.marquee.get().is_some(),
            "dragging a delegated heavyweight tappable card must start a marquee"
        );
    });
}
