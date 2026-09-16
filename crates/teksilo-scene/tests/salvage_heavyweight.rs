// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Salvaging and restoring **heavyweight** entries — the tier where the answer
//! is not obvious and where refusing the wrong restores would be easy.
//!
//! Two shapes, and they behave differently on purpose:
//!
//! * a multi-view entry ([`SceneModel::add_widget_item`]) stores a type-erased
//!   payload every view can rebuild from, so a restore is total;
//! * a single-view one (`Scene::add_widget`) stores one `Box<dyn Widget>` that
//!   the first view to build takes. Once taken, the box is gone from the model
//!   and the instance lives in the arena.
//!
//! The tempting design is a `RemovedItem::is_restorable()` that refuses the
//! second case. It would be wrong, and these tests are why: `Once`
//! restorability is *per view and time-dependent*. A take and a restore inside
//! one build cycle never reach the view's orphan reap — which keys on the
//! scene's live heavyweight set, and the restore has already put the id back
//! into it — so the arena instance survives and the card is still on screen.
//! Refusing that restore would refuse one that works, and would refuse the
//! whole entry (geometry, magnets, accessibility) over a widget instance.
//!
//! Run with: cargo test -p teksilo-scene --test salvage_heavyweight

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_scene::{A11yNode, ItemId, SceneModel, SceneView};
use teksilo_widgets::TextInput;

const VIEWPORT: SizeProposal = SizeProposal {
    width: Some(800.0),
    height: Some(600.0),
};

fn tree_with(model: SceneModel) -> (WidgetTree, teksilo_core::widget_id::WidgetId) {
    let mut tree = WidgetTree::new();
    let view = SceneView::with_model(model)
        .delegate_typed::<Signal<String>>(|sig, _id| Box::new(TextInput::new(sig.clone())));
    let view_id = tree.add(view);
    tree.layout(VIEWPORT);
    let _ = tree.render();
    (tree, view_id)
}

/// How many keyboard-reachable stops the view has.
///
/// `tab_stops_within` rather than a node count, because it is the only thing
/// that answers the question a card's presence actually raises — whether a user
/// can still reach it. `WidgetTree::focus` does not check focusability, so
/// nothing else here would be an honest probe. The view itself is one stop; a
/// materialised `TextInput` adds another.
fn stops(tree: &WidgetTree, view_id: teksilo_core::widget_id::WidgetId) -> usize {
    tree.tab_stops_within(view_id).len()
}

#[test]
fn a_multi_view_card_survives_a_take_and_restore_whole() {
    let model = SceneModel::new();
    let text = Signal::new("hello".to_string());
    let id = model.add_widget_item(text.clone(), Rect::new(10.0, 20.0, 200.0, 40.0));
    model.set_a11y_landmark(A11yNode::Item(id), accesskit::Role::Region);
    let (mut tree, view_id) = tree_with(model.clone());
    let with_card = stops(&tree, view_id);

    let salvage = model.take(id);
    assert_eq!(salvage.len(), 1);
    assert!(
        salvage[0].payload().is_some(),
        "a Delegated entry carries the payload every view rebuilds from"
    );
    assert!(salvage[0].widget_instance_present());

    tree.layout(VIEWPORT);
    let _ = tree.render();
    assert!(
        stops(&tree, view_id) < with_card,
        "the view reaped the orphaned card"
    );

    model.restore_all(salvage).unwrap();
    tree.layout(VIEWPORT);
    let _ = tree.render();

    assert_eq!(
        stops(&tree, view_id),
        with_card,
        "the delegate rebuilt the card from the salvaged payload"
    );
    assert_eq!(model.local_pos(id), Some(Point::new(10.0, 20.0)));
    assert_eq!(
        model.a11y_landmark_of(A11yNode::Item(id)),
        Some(accesskit::Role::Region)
    );
}

#[test]
fn a_single_view_card_taken_and_restored_in_one_cycle_keeps_its_instance() {
    // B3, stated as a test: this restore *works*, so `restore` must not refuse
    // it on the grounds that the `Once` box has already been drained.
    let model = SceneModel::new();
    let text = Signal::new("hello".to_string());
    let id = model.add_widget(
        TextInput::new(text.clone()),
        Rect::new(0.0, 0.0, 200.0, 40.0),
    );
    let (mut tree, view_id) = tree_with(model.clone());
    let with_card = stops(&tree, view_id);
    assert!(with_card > 1, "the Once widget materialised as a tab stop");

    // Take and restore without letting the view build in between — the shape a
    // delete/undo pair inside one event handler takes.
    let salvage = model.take(id);
    assert!(
        !salvage[0].widget_instance_present(),
        "the view drained the one-shot box at its first build"
    );
    let restored = model
        .restore_all(salvage)
        .expect("a restore must not be refused over a widget instance");
    assert_eq!(restored, vec![id]);

    tree.layout(VIEWPORT);
    let _ = tree.render();
    assert_eq!(
        stops(&tree, view_id),
        with_card,
        "the orphan reap never ran, so the arena instance is still the card's"
    );
}

#[test]
fn a_single_view_card_reaped_before_the_restore_comes_back_without_its_widget() {
    // The honest other half of B3: once the view has reaped the instance there
    // is nothing to re-materialise, and the entry comes back as a heavyweight
    // slot no view can fill. The restore still succeeds — geometry and
    // accessibility are worth restoring on their own — and
    // `widget_instance_present` is how an app sees it coming.
    let model = SceneModel::new();
    let text = Signal::new("hello".to_string());
    let id = model.add_widget(TextInput::new(text), Rect::new(0.0, 0.0, 200.0, 40.0));
    let (mut tree, view_id) = tree_with(model.clone());
    let with_card = stops(&tree, view_id);

    let salvage = model.take(id);
    assert!(!salvage[0].widget_instance_present());
    // Let the view build: the id is gone from the live heavyweight set, so the
    // reap destroys the subtree.
    tree.layout(VIEWPORT);
    let _ = tree.render();
    let reaped = stops(&tree, view_id);
    assert!(reaped < with_card);

    model.restore_all(salvage).unwrap();
    tree.layout(VIEWPORT);
    let _ = tree.render();

    assert_eq!(model.len(), 1, "the entry is back");
    assert_eq!(
        stops(&tree, view_id),
        reaped,
        "…but there is no widget left to materialise: single-view content that \
         must survive an undo belongs in add_widget_item"
    );
}

#[test]
fn a_taken_id_is_never_handed_to_a_new_item() {
    // `restore` puts a retired id back, which is the one exception to "ids are
    // never reused". It must stay an exception: a *fresh* insert while the
    // salvage is held must not collide with it.
    let model = SceneModel::new();
    let id = model.add_widget_item(Signal::new(String::new()), Rect::new(0.0, 0.0, 10.0, 10.0));
    let salvage = model.take(id);

    let fresh: Vec<ItemId> = (0..8)
        .map(|_| model.add_widget_item(Signal::new(String::new()), Rect::new(0.0, 0.0, 10.0, 10.0)))
        .collect();
    assert!(!fresh.contains(&id));

    model.restore_all(salvage).unwrap();
    assert_eq!(model.len(), 9);
}
