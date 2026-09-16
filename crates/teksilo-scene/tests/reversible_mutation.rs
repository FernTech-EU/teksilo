// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The reversible-mutation seam: the envelope on the notification channel, the
//! owning salvage door, and the transaction record delivered to the edit sink.
//!
//! Every test here is written so that removing the mechanism it names makes it
//! fail. Where that is not obvious from the assertion, the test says which line
//! of the implementation it is pinning.
//!
//! Nothing in this file is an undo stack. The framework's obligation is that
//! the record is complete, grouped, tagged and invertible; *applying* the
//! inverse is the consumer's, and the two tests that do it
//! (`a_consumer_can_reverse_*`) are standing in for that consumer.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, StrokeStyle, Transform2D};
use teksilo_core::presets::intui;
use teksilo_scene::{
    A11yCategory, A11yNode, A11yRelation, AppearanceChange, ChangeSource, GroupItem, HistoryMode,
    ItemChange, Magnet, PathItem, Placement, RectItem, RemovedItem, ReplaceItemError, RestoreError,
    Salvage, Scene, SceneChange, SceneEdit, SceneModel, SceneTransactionRecord, TextItem,
    TxnOutcome,
};
use teksilo_tokens::Color;

// -----------------------------------------------------------------
// Fixtures
// -----------------------------------------------------------------

/// A theme to resolve a `ColorProp` against. The colours under test are plain
/// `Color`s, so which theme it is does not matter — only that `resolve` has one.
fn theme() -> teksilo_core::styles::Theme {
    intui::light()
}

fn boxed_rect(x: f32, w: f32) -> RectItem {
    RectItem::new(Rect::new(x, 0.0, w, 10.0))
}

/// A sink that keeps every record it is handed.
fn recording_sink(model: &SceneModel) -> Rc<RefCell<Vec<SceneTransactionRecord>>> {
    let records: Rc<RefCell<Vec<SceneTransactionRecord>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = records.clone();
    model.set_edit_sink(move |record| sink.borrow_mut().push(record));
    records
}

/// Every `SceneChange` the model announces, in order.
fn recording_observer(
    model: &SceneModel,
) -> (
    Rc<RefCell<Vec<SceneChange>>>,
    teksilo_core::signal::ObserverHandle,
) {
    let log: Rc<RefCell<Vec<SceneChange>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = log.clone();
    let handle = model
        .item_change_signal()
        .observe(move |c| sink.borrow_mut().push(c.clone()));
    (log, handle)
}

// -----------------------------------------------------------------
// The envelope on the cheap channel
// -----------------------------------------------------------------

#[test]
fn every_change_in_one_write_scope_shares_one_transaction_id() {
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let b = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(a, Point::new(5.0, 0.0));
        scene.set_local_pos(b, Point::new(6.0, 0.0));
        scene.set_z(a, 3.0);
    }

    let seen = log.borrow();
    assert_eq!(seen.len(), 3);
    let txn = seen[0].txn;
    assert!(!txn.is_none(), "a real transaction is never the sentinel");
    assert!(
        seen.iter().all(|c| c.txn == txn),
        "one write scope is one transaction: {:?}",
        seen.iter().map(|c| c.txn).collect::<Vec<_>>()
    );
}

#[test]
fn separate_mutator_calls_are_separate_transactions() {
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    model.set_local_pos(a, Point::new(5.0, 0.0));
    model.set_local_pos(a, Point::new(6.0, 0.0));

    let seen = log.borrow();
    assert_eq!(seen.len(), 2);
    assert_ne!(
        seen[0].txn, seen[1].txn,
        "each mutator call is its own write scope, so its own transaction"
    );
}

#[test]
fn a_subtree_removal_is_one_transaction() {
    let model = SceneModel::new();
    let parent = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let child = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let grandchild = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_item_parent(child, Some(parent));
    model.set_item_parent(grandchild, Some(child));
    let (log, _h) = recording_observer(&model);

    model.remove(parent);

    let seen = log.borrow();
    assert_eq!(seen.len(), 3, "one Removed per id");
    let txn = seen[0].txn;
    assert!(
        seen.iter().all(|c| c.txn == txn),
        "undoing a three-item subtree deletion must be one step, not three"
    );
}

#[test]
fn an_explicit_transaction_groups_several_mutator_calls() {
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let b = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    {
        let _txn = model.transaction(ChangeSource::User, HistoryMode::RecordPreserveRedo);
        model.set_local_pos(a, Point::new(5.0, 0.0));
        model.set_local_pos(b, Point::new(6.0, 0.0));
    }

    let seen = log.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].txn, seen[1].txn);
    assert!(seen.iter().all(|c| c.source == ChangeSource::User));
    assert!(
        seen.iter()
            .all(|c| c.history == HistoryMode::RecordPreserveRedo),
        "the stamp the caller chose rides every change in the group"
    );
}

#[test]
fn nesting_a_transaction_joins_the_open_one_and_the_outer_stamp_wins() {
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    {
        let _outer = model.transaction(ChangeSource::User, HistoryMode::Record);
        model.set_local_pos(a, Point::new(1.0, 0.0));
        {
            // An observer opening its own transaction inside a framework
            // gesture must not split that gesture into two undo steps.
            let _inner = model.transaction(ChangeSource::Remote, HistoryMode::Ignore);
            model.set_local_pos(a, Point::new(2.0, 0.0));
        }
        model.set_local_pos(a, Point::new(3.0, 0.0));
    }

    let seen = log.borrow();
    assert_eq!(seen.len(), 3);
    let txn = seen[0].txn;
    assert!(seen.iter().all(|c| c.txn == txn));
    assert!(
        seen.iter()
            .all(|c| c.source == ChangeSource::User && c.history == HistoryMode::Record),
        "the inner stamp must be ignored, not merged"
    );
}

#[test]
fn the_dynamic_bounds_refresh_is_ephemeral_and_an_ordinary_write_is_not() {
    use teksilo_core::signal::Signal;

    // A dynamic item whose bounds follow a signal: the scene re-reads it every
    // build, so it emits `LocalBoundsChanged` per frame. That churn is
    // rendered, never recorded.
    #[derive(Debug)]
    struct Breathing(Signal<f32>);
    impl teksilo_scene::SceneItem for Breathing {
        fn local_bounds(&self) -> Rect {
            Rect::new(0.0, 0.0, self.0.get(), 10.0)
        }
        fn set_local_bounds(&mut self, _b: Rect) {}
        fn paint(
            &self,
            _canvas: &mut teksilo_canvas::Canvas,
            _ctx: &teksilo_scene::SceneItemPaintContext<'_>,
        ) {
        }
    }

    let width = Signal::new(10.0_f32);
    let model = SceneModel::new();
    let id = model.add_item_dynamic(Breathing(width.clone()), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    width.set(20.0);
    model.refresh_dynamic_bounds();
    model.set_local_pos(id, Point::new(4.0, 0.0));

    let seen = log.borrow();
    let bounds = seen
        .iter()
        .find(|c| matches!(c.change, ItemChange::LocalBoundsChanged { .. }))
        .expect("the refresh wrote the new bounds");
    assert!(
        bounds.ephemeral,
        "per-frame dynamic-bounds churn must be tagged ephemeral"
    );
    let moved = seen
        .iter()
        .find(|c| matches!(c.change, ItemChange::LocalPosChanged { .. }))
        .expect("the ordinary write");
    assert!(
        !moved.ephemeral,
        "an ordinary write must not be tagged ephemeral"
    );
}

#[test]
fn an_app_can_mark_its_own_per_frame_stream_ephemeral() {
    // The symmetric half of the knob: an app driving a rotation from a signal
    // produces the same shape of churn as the dynamic-bounds refresh, and needs
    // the same way to say so.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    {
        let _txn = model.user_edit().ephemeral();
        model.set_transform(id, Transform2D::rotate(0.1));
    }

    let seen = log.borrow();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].ephemeral);
}

// -----------------------------------------------------------------
// The three `old` values the seam owes
// -----------------------------------------------------------------

#[test]
fn a_fill_change_reports_the_colour_it_replaced() {
    // B1: the old fill lives *inside* the item, and there is no getter. It
    // rides out of `SceneItem::set_fill`, which is why a third-party item
    // cannot have a silently-wrong `old`.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0).fill(Color::RED), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    model.set_item_fill(id, Color::BLUE);

    let seen = log.borrow();
    let ItemChange::AppearanceChanged {
        change: AppearanceChange::Fill { old, new },
        ..
    } = &seen[0].change
    else {
        panic!("expected a fill change, got {:?}", seen[0].change)
    };
    assert_eq!(
        old.as_ref().map(|p| p.resolve(&theme(), true)),
        Some(Color::RED),
        "the old fill must be the colour that was actually there — a defaulted \
         getter would report None and an undo would CLEAR the fill"
    );
    assert_eq!(
        new.as_ref().map(|p| p.resolve(&theme(), true)),
        Some(Color::BLUE)
    );
}

#[test]
fn clearing_a_fill_reports_the_colour_it_cleared() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0).fill(Color::RED), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    model.clear_item_fill(id);

    let seen = log.borrow();
    let ItemChange::AppearanceChanged {
        change: AppearanceChange::Fill { old, new },
        ..
    } = &seen[0].change
    else {
        panic!("expected a fill change")
    };
    assert_eq!(
        old.as_ref().map(|p| p.resolve(&theme(), true)),
        Some(Color::RED)
    );
    assert!(new.is_none());
}

#[test]
fn a_stroke_change_reports_the_stroke_it_replaced() {
    let model = SceneModel::new();
    let id = model.add_item(
        boxed_rect(0.0, 10.0).stroke_styled(Color::RED, StrokeStyle::solid(2.0)),
        Point::ZERO,
    );
    let (log, _h) = recording_observer(&model);

    model.set_item_stroke(id, Color::BLUE, StrokeStyle::solid(4.0));

    let seen = log.borrow();
    let ItemChange::AppearanceChanged {
        change: AppearanceChange::Stroke { old, new },
        ..
    } = &seen[0].change
    else {
        panic!("expected a stroke change")
    };
    let (old_color, old_style) = old.as_ref().expect("there was a stroke");
    assert_eq!(old_color.resolve(&theme(), true), Color::RED);
    assert_eq!(old_style.width, 2.0);
    assert_eq!(new.as_ref().unwrap().1.width, 4.0);
}

#[test]
fn a_refused_appearance_write_emits_no_change_at_all() {
    // `AppearanceWrite::Refused` is the default, and a refusal must not fake an
    // `old` — it must not produce a change.
    let model = SceneModel::new();
    let id = model.add_item(
        TextItem::new(teksilo_i18n::lit!("hi"), Rect::new(0.0, 0.0, 40.0, 12.0)),
        Point::ZERO,
    );
    let (log, _h) = recording_observer(&model);

    // A text item always has a foreground colour, so a *clear* is refused.
    model.clear_item_fill(id);
    // …and it has no stroke slot at all.
    model.clear_item_stroke(id);

    assert!(
        log.borrow().is_empty(),
        "a refused appearance write must emit no change: {:?}",
        log.borrow()
    );
}

#[test]
fn a_transform_change_reports_the_transform_it_replaced() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_transform(id, Transform2D::rotate(0.5));
    let (log, _h) = recording_observer(&model);

    model.set_transform(id, Transform2D::rotate(1.0));

    let seen = log.borrow();
    let ItemChange::TransformChanged { old, new, .. } = &seen[0].change else {
        panic!("expected a transform change")
    };
    assert_eq!(*old, Transform2D::rotate(0.5));
    assert_eq!(*new, Transform2D::rotate(1.0));
}

#[test]
fn an_unchanged_transform_write_emits_nothing() {
    // Without the equality guard an app animating a rotation from a settled
    // signal produces a change — and now a transaction — every frame.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_transform(id, Transform2D::rotate(0.5));
    let (log, _h) = recording_observer(&model);

    model.set_transform(id, Transform2D::rotate(0.5));

    assert!(log.borrow().is_empty());
}

#[test]
fn a_payload_change_reports_the_payload_it_replaced() {
    let model = SceneModel::new();
    let id = model.add_widget_item(7u32, Rect::new(0.0, 0.0, 20.0, 20.0));
    let (log, _h) = recording_observer(&model);

    model.set_payload(id, 9u32);

    let seen = log.borrow();
    let ItemChange::PayloadChanged { old, new, .. } = &seen[0].change else {
        panic!("expected a payload change")
    };
    assert_eq!(old.downcast::<u32>().as_deref().copied(), Some(7));
    assert_eq!(new.downcast::<u32>().as_deref().copied(), Some(9));
}

// -----------------------------------------------------------------
// The owning salvage
// -----------------------------------------------------------------

#[test]
fn take_and_restore_return_the_item_at_the_same_id() {
    let mut scene = Scene::new();
    let id = scene.add_item(boxed_rect(4.0, 30.0), Point::new(11.0, 22.0));
    scene.set_z(id, 5.0);
    scene.set_opacity(id, 0.5);

    let salvage = scene.take(id);
    assert_eq!(salvage.len(), 1);
    assert_eq!(salvage[0].id(), id);
    assert_eq!(scene.len(), 0);

    assert_eq!(scene.restore_all(salvage).unwrap(), vec![id]);
    assert_eq!(scene.len(), 1);
    assert_eq!(scene.local_pos(id), Some(Point::new(11.0, 22.0)));
    assert_eq!(scene.z(id), Some(5.0));
    assert_eq!(scene.opacity(id), Some(0.5));
    assert_eq!(
        scene.local_bounds(id),
        Some(Rect::new(4.0, 0.0, 30.0, 10.0))
    );
}

#[test]
fn restore_puts_a_subtree_back_with_its_parent_links_and_child_order() {
    let mut scene = Scene::new();
    let root = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let a = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let b = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let deep = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    scene.set_item_parent(a, Some(root));
    scene.set_item_parent(b, Some(root));
    scene.set_item_parent(deep, Some(a));

    let salvage = scene.take(root);
    assert_eq!(salvage.len(), 4);
    // Deepest first, named root last — the order `remove` announces in.
    assert_eq!(salvage[0].id(), deep);
    assert_eq!(salvage[3].id(), root);

    scene.restore_all(salvage).unwrap();
    assert_eq!(scene.parent_of(a), Some(root));
    assert_eq!(scene.parent_of(b), Some(root));
    assert_eq!(scene.parent_of(deep), Some(a));
    // Declaration order — the order the AT walk publishes siblings in — comes
    // back exactly as it was, not with the subtree appended last.
    assert_eq!(scene.ids(), vec![root, a, b, deep]);
    // And the `children` adjacency is rebuilt in its original order rather than
    // in restore order: a breadth-first walk from the root reads a, b, deep.
    let mut descendants = Vec::new();
    scene.collect_descendants(root, &mut descendants);
    assert_eq!(descendants, vec![a, b, deep]);
}

#[test]
fn restoring_a_child_before_its_parent_is_refused_by_name() {
    let mut scene = Scene::new();
    let root = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let child = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    scene.set_item_parent(child, Some(root));

    let mut salvage = scene.take(root);
    // take order is [child, root]; taking the child first is the trap.
    let child_salvage = salvage.remove(0);
    assert_eq!(
        scene.restore(child_salvage).unwrap_err(),
        RestoreError::MissingParent {
            id: child,
            parent: root
        }
    );
}

#[test]
fn a_take_restore_cycle_never_duplicates_the_entry() {
    // The invariant that makes `RestoreError::IdAlreadyLive` unreachable
    // through the public API, stated as the thing that is actually testable:
    // `take` retires the id, so a second salvage for it cannot exist while the
    // first is held, and the restore that succeeds consumes the one that does.
    // The guard stays because it protects `entry_index` from aliasing if that
    // ever stops being true — see `RestoreError::IdAlreadyLive`.
    let mut scene = Scene::new();
    let id = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);

    for _ in 0..5 {
        let salvage = scene.take(id);
        assert_eq!(salvage.len(), 1);
        assert_eq!(scene.len(), 0);
        assert!(
            scene.take(id).is_empty(),
            "the id is retired while the salvage is held, so there is never a \
             second salvage for it"
        );
        assert_eq!(scene.restore_all(salvage).unwrap(), vec![id]);
        assert_eq!(scene.len(), 1, "one entry, not two");
    }
    assert_eq!(scene.ids(), vec![id]);
}

#[test]
fn a_restore_puts_the_item_back_at_its_place_in_declaration_order() {
    // Declaration order is the order the accessibility walk publishes siblings
    // in — the order a screen reader reads the scene in. An undone deletion
    // that appended the item last would silently move it to the end of that
    // reading order.
    let mut scene = Scene::new();
    let first = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let middle = scene.add_item(boxed_rect(1.0, 10.0), Point::ZERO);
    let last = scene.add_item(boxed_rect(2.0, 10.0), Point::ZERO);

    let salvage = scene.take(middle);
    scene.restore_all(salvage).unwrap();

    assert_eq!(scene.ids(), vec![first, middle, last]);
}

#[test]
fn a_restore_brings_back_the_magnets_with_their_ids() {
    let mut scene = Scene::new();
    let id = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let m1 = scene.add_magnet(id, Magnet::new(Point::new(0.0, 5.0)));
    let m2 = scene.add_magnet(id, Magnet::new(Point::new(10.0, 5.0)));

    let salvage = scene.take(id);
    assert_eq!(salvage[0].magnet_count(), 2);
    assert_eq!(scene.magnet_owner(m1), None, "the removal retired them");

    scene.restore_all(salvage).unwrap();
    assert_eq!(scene.magnet_ids_of(id), vec![m1, m2]);
    assert_eq!(scene.magnet_owner(m1), Some(id));
    assert_eq!(scene.magnet_owner(m2), Some(id));
}

#[test]
fn a_restore_brings_back_every_a11y_decoration() {
    let mut scene = Scene::new();
    let id = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let other = scene.add_item(boxed_rect(20.0, 10.0), Point::ZERO);
    let node = A11yNode::Item(id);

    scene.set_a11y_parent(node, Some(A11yNode::Item(other)));
    scene.add_a11y_relation(node, A11yRelation::Controls, A11yNode::Item(other));
    scene.set_a11y_live(node, accesskit::Live::Polite);
    scene.set_a11y_landmark(node, accesskit::Role::Region);
    scene.set_a11y_categories(node, &[A11yCategory::new("landmark")]);

    let salvage = scene.take(id);
    assert_eq!(
        scene.a11y_parent_of(node),
        None,
        "the removal dropped it — that is what makes this salvage load-bearing"
    );
    assert!(scene.a11y_relations().is_empty());
    assert_eq!(scene.a11y_live_of(node), None);

    scene.restore_all(salvage).unwrap();

    assert_eq!(scene.a11y_parent_of(node), Some(A11yNode::Item(other)));
    assert_eq!(
        scene.a11y_relations(),
        &[(node, A11yRelation::Controls, A11yNode::Item(other))]
    );
    assert_eq!(scene.a11y_live_of(node), Some(accesskit::Live::Polite));
    assert_eq!(scene.a11y_landmark_of(node), Some(accesskit::Role::Region));
    assert_eq!(
        scene.a11y_categories_of(node),
        Some(&[A11yCategory::new("landmark")][..])
    );
}

#[test]
fn a_restore_re_adopts_survivors_that_were_at_parented_under_the_removed_item() {
    // B2. `Scene::remove` retains `a11y_parents` on **both** endpoints, so it
    // also re-roots surviving nodes that were AT-parented under the removed
    // item. Recording only the removed item's own parent would make this
    // silently fail — at exactly the edge case the salvage is sold on.
    let mut scene = Scene::new();
    let container = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let survivor_a = scene.add_item(boxed_rect(20.0, 10.0), Point::ZERO);
    let survivor_b = scene.add_item(boxed_rect(40.0, 10.0), Point::ZERO);
    scene.set_a11y_parent(A11yNode::Item(survivor_a), Some(A11yNode::Item(container)));
    scene.set_a11y_parent(A11yNode::Item(survivor_b), Some(A11yNode::Item(container)));

    let salvage = scene.take(container);
    assert_eq!(salvage[0].a11y().adopted.len(), 2);
    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(survivor_a)),
        None,
        "while the container is gone the survivors are correctly re-rooted"
    );

    scene.restore_all(salvage).unwrap();

    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(survivor_a)),
        Some(A11yNode::Item(container))
    );
    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(survivor_b)),
        Some(A11yNode::Item(container))
    );
}

#[test]
fn a_restore_re_attaches_a_relation_pointing_at_the_removed_item() {
    // The inbound half of the relation edge: `remove` retains on both
    // endpoints, so a relation *into* the removed item is dropped too.
    let mut scene = Scene::new();
    let target = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let source = scene.add_item(boxed_rect(20.0, 10.0), Point::ZERO);
    scene.add_a11y_relation(
        A11yNode::Item(source),
        A11yRelation::Controls,
        A11yNode::Item(target),
    );

    let salvage = scene.take(target);
    assert!(scene.a11y_relations().is_empty());
    scene.restore_all(salvage).unwrap();

    assert_eq!(
        scene.a11y_relations(),
        &[(
            A11yNode::Item(source),
            A11yRelation::Controls,
            A11yNode::Item(target)
        )]
    );
}

#[test]
fn a_restore_skips_an_edge_whose_other_endpoint_is_gone() {
    let mut scene = Scene::new();
    let id = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let other = scene.add_item(boxed_rect(20.0, 10.0), Point::ZERO);
    scene.set_a11y_parent(A11yNode::Item(id), Some(A11yNode::Item(other)));

    let salvage = scene.take(id);
    scene.remove(other);
    scene.restore_all(salvage).unwrap();

    assert_eq!(
        scene.a11y_parent_of(A11yNode::Item(id)),
        None,
        "re-inserting an edge to a dead node would name something the AT walker \
         cannot resolve"
    );
}

#[test]
fn a_relation_between_two_removed_items_is_restored_once() {
    let mut scene = Scene::new();
    let root = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let child = scene.add_item(boxed_rect(20.0, 10.0), Point::ZERO);
    scene.set_item_parent(child, Some(root));
    scene.add_a11y_relation(
        A11yNode::Item(root),
        A11yRelation::Controls,
        A11yNode::Item(child),
    );

    let salvage = scene.take(root);
    scene.restore_all(salvage).unwrap();

    assert_eq!(
        scene.a11y_relations().len(),
        1,
        "recorded on one endpoint's salvage, so it comes back once"
    );
}

#[test]
fn an_undecorated_item_costs_the_a11y_channel_nothing() {
    // Round-trip on a scene with no AT structure at all: the harvest must not
    // invent decorations, and the restore must not bump the AT channel for an
    // item that never had any.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let a11y_bumps = Rc::new(std::cell::Cell::new(0u32));
    let counter = a11y_bumps.clone();
    let _h = model
        .a11y_change_signal()
        .observe(move |_| counter.set(counter.get() + 1));

    let salvage = model.take(id);
    assert!(salvage[0].a11y().is_empty());
    model.restore_all(salvage).unwrap();

    assert_eq!(
        a11y_bumps.get(),
        0,
        "an undecorated item costs the AT channel nothing"
    );
}

#[test]
fn remove_is_take_with_the_salvage_routed_to_the_sink() {
    // C9: the two doors are one implementation, so `remove` cannot forget a
    // side map that `take` remembers.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.add_magnet(id, Magnet::new(Point::ZERO));
    model.set_a11y_live(A11yNode::Item(id), accesskit::Live::Polite);
    let records = recording_sink(&model);

    model.remove(id);

    let seen = records.borrow();
    assert_eq!(seen.len(), 1);
    let SceneEdit::Removed {
        id: removed,
        salvage: Salvage::Owned(item),
    } = &seen[0].edits[0]
    else {
        panic!("expected an owned salvage, got {:?}", seen[0].edits)
    };
    assert_eq!(*removed, id);
    assert_eq!(item.magnet_count(), 1);
    assert_eq!(item.a11y().live, Some(accesskit::Live::Polite));
}

#[test]
fn take_tells_the_sink_the_caller_kept_the_salvage() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);

    let salvage = model.take(id);
    assert_eq!(salvage.len(), 1);

    let seen = records.borrow();
    assert!(matches!(
        &seen[0].edits[0],
        SceneEdit::Removed {
            salvage: Salvage::TakenByCaller,
            ..
        }
    ));
}

#[test]
fn a_consumer_can_reverse_a_deletion_from_the_record_alone() {
    // The whole point, exercised end to end: the app holds nothing but what the
    // sink handed it, and puts the scene back.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(3.0, 12.0), Point::new(7.0, 8.0));
    model.add_magnet(id, Magnet::new(Point::ZERO));
    model.set_a11y_landmark(A11yNode::Item(id), accesskit::Role::Region);

    let stash: Rc<RefCell<Vec<RemovedItem>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = stash.clone();
    model.set_edit_sink(move |record| {
        for edit in record.edits {
            if let SceneEdit::Removed {
                salvage: Salvage::Owned(item),
                ..
            } = edit
            {
                sink.borrow_mut().push(*item);
            }
        }
    });

    model.remove(id);
    assert_eq!(model.len(), 0);

    let salvage: Vec<RemovedItem> = stash.borrow_mut().drain(..).collect();
    model.restore_all(salvage).unwrap();

    assert_eq!(model.len(), 1);
    assert_eq!(model.local_pos(id), Some(Point::new(7.0, 8.0)));
    assert_eq!(model.magnet_ids_of(id).len(), 1);
    assert_eq!(
        model.a11y_landmark_of(A11yNode::Item(id)),
        Some(accesskit::Role::Region)
    );
}

#[test]
fn with_no_sink_installed_nothing_is_recorded() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    // No sink: `remove` drops the salvage, exactly as it always has.
    model.remove(id);
    assert_eq!(model.len(), 0);
    // Installing one afterwards does not resurrect anything.
    let records = recording_sink(&model);
    assert!(records.borrow().is_empty());
}

// -----------------------------------------------------------------
// replace_item
// -----------------------------------------------------------------

#[test]
fn replace_item_keeps_the_entry_and_returns_the_old_box() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_z(id, 4.0);
    model.set_opacity(id, 0.25);
    let magnet = model.add_magnet(id, Magnet::new(Point::ZERO));
    model.set_a11y_live(A11yNode::Item(id), accesskit::Live::Polite);

    let previous = model
        .replace_item(id, Box::new(boxed_rect(100.0, 60.0)))
        .unwrap();
    assert_eq!(previous.local_bounds(), Rect::new(0.0, 0.0, 10.0, 10.0));

    assert_eq!(model.z(id), Some(4.0));
    assert_eq!(model.opacity(id), Some(0.25));
    assert_eq!(model.magnet_ids_of(id), vec![magnet]);
    assert_eq!(
        model.a11y_live_of(A11yNode::Item(id)),
        Some(accesskit::Live::Polite)
    );
    assert_eq!(
        model.local_bounds(id),
        Some(Rect::new(100.0, 0.0, 60.0, 10.0))
    );
}

#[test]
fn replace_item_rebuckets_the_spatial_index() {
    // B4. The AABB the index buckets on is derived from the item, so a
    // replacement with different bounds must re-bucket or every rect query,
    // hit test and cull pass answers from the old rectangle.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    assert_eq!(
        model.items_in_rect(Rect::new(0.0, 0.0, 20.0, 20.0)),
        vec![id]
    );

    model
        .replace_item(id, Box::new(boxed_rect(500.0, 10.0)))
        .unwrap();

    assert!(
        model
            .items_in_rect(Rect::new(0.0, 0.0, 20.0, 20.0))
            .is_empty(),
        "the old AABB must not survive the swap"
    );
    assert_eq!(
        model.items_in_rect(Rect::new(490.0, 0.0, 40.0, 20.0)),
        vec![id]
    );
    assert_eq!(model.item_at(Point::new(505.0, 5.0)), Some(id));
}

#[test]
fn replace_item_emits_one_change_not_a_removal_and_an_add() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    model
        .replace_item(id, Box::new(boxed_rect(0.0, 40.0)))
        .unwrap();

    let seen = log.borrow();
    assert_eq!(seen.len(), 1);
    let ItemChange::ItemReplaced {
        old_bounds,
        new_bounds,
        ..
    } = seen[0].change
    else {
        panic!("expected ItemReplaced, got {:?}", seen[0].change)
    };
    assert_eq!(old_bounds.width, 10.0);
    assert_eq!(new_bounds.width, 40.0);
}

#[test]
fn replace_item_keeps_the_entry_flags_not_the_new_items_initial_flags() {
    // A content refresh must not silently undo an app's `set_visible(false)`.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_visible(id, false);

    model
        .replace_item(id, Box::new(boxed_rect(0.0, 10.0)))
        .unwrap();

    assert!(
        !model
            .flags(id)
            .unwrap()
            .contains(teksilo_scene::ItemFlags::IS_VISIBLE)
    );
}

#[test]
fn replace_item_refuses_a_widget_entry_and_an_unknown_id() {
    let model = SceneModel::new();
    let widget = model.add_widget_item(1u32, Rect::new(0.0, 0.0, 10.0, 10.0));
    let rejected = model
        .replace_item(widget, Box::new(boxed_rect(7.0, 10.0)))
        .unwrap_err();
    assert_eq!(rejected.error, ReplaceItemError::NotLightweight(widget));
    assert_eq!(
        rejected.item.local_bounds(),
        Rect::new(7.0, 0.0, 10.0, 10.0),
        "a refused write hands the item back rather than eating it"
    );

    let ghost = {
        let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
        model.remove(id);
        id
    };
    let rejected = model
        .replace_item(ghost, Box::new(boxed_rect(8.0, 10.0)))
        .unwrap_err();
    assert_eq!(rejected.error, ReplaceItemError::UnknownItem(ghost));
    assert_eq!(
        rejected.item.local_bounds(),
        Rect::new(8.0, 0.0, 10.0, 10.0)
    );
}

// -----------------------------------------------------------------
// The edit sink
// -----------------------------------------------------------------

#[test]
fn the_sink_runs_with_the_scene_free_and_may_write_it() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let writer = model.clone();
    model.set_edit_sink(move |record| {
        // Reading and writing from inside the sink is the advertised
        // capability; without the unborrowed delivery both panic.
        if record
            .edits
            .iter()
            .any(|e| matches!(e, SceneEdit::Change(ItemChange::LocalPosChanged { .. })))
            && writer.local_pos(id).map(|p| p.y) == Some(0.0)
        {
            writer.set_opacity(id, 0.5);
        }
    });

    model.set_local_pos(id, Point::new(3.0, 0.0));
    assert_eq!(model.opacity(id), Some(0.5));
}

#[test]
fn the_sinks_own_write_is_journaled_and_agrees_with_the_scene() {
    // B5. A sink that corrects a value must not be able to leave the record the
    // app holds disagreeing with the scene. If the sink's write were made with
    // the sink out of its slot, the app would end up believing the item is at
    // 3.0 while it sits at 4.0 — and a redo would replay 3.0.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let positions: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(Vec::new()));

    let seen = positions.clone();
    let writer = model.clone();
    model.set_edit_sink(move |record| {
        for edit in &record.edits {
            if let SceneEdit::Change(ItemChange::LocalPosChanged { new, .. }) = edit {
                seen.borrow_mut().push(new.x);
                // Snap to a multiple of 4 — converging, so it settles.
                let snapped = (new.x / 4.0).round() * 4.0;
                if (snapped - new.x).abs() > f32::EPSILON {
                    writer.set_local_pos(id, Point::new(snapped, new.y));
                }
            }
        }
    });

    model.set_local_pos(id, Point::new(3.0, 0.0));

    assert_eq!(model.local_pos(id), Some(Point::new(4.0, 0.0)));
    assert_eq!(
        *positions.borrow(),
        vec![3.0, 4.0],
        "the sink's own correction must arrive as a record too — otherwise the \
         history says 3.0 while the scene is at 4.0"
    );
}

#[test]
fn the_sink_survives_its_own_write() {
    // A take-call-put-back sink would be missing during its own write and
    // (worse) lost entirely if it panicked. Borrowing keeps it installed.
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let count = Rc::new(std::cell::Cell::new(0u32));
    let counter = count.clone();
    let writer = model.clone();
    model.set_edit_sink(move |_record| {
        counter.set(counter.get() + 1);
        if counter.get() == 1 {
            writer.set_z(a, 9.0);
        }
    });

    model.set_local_pos(a, Point::new(1.0, 0.0));
    assert_eq!(count.get(), 2, "the sink saw its own write");
    model.set_local_pos(a, Point::new(2.0, 0.0));
    assert_eq!(count.get(), 3, "and is still installed afterwards");
}

#[test]
fn an_unconverging_sink_panics_instead_of_freezing_the_thread() {
    // The delivery loop is iterative, so a sink that writes a different value
    // on every record would otherwise be a frozen UI thread with no
    // diagnostic. Same reasoning, and the same budget, as the change fan-out.
    let model = SceneModel::new();
    model.set_cascade_budget(teksilo_scene::CascadeBudget::new(64));
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let writer = model.clone();
    let step = Rc::new(std::cell::Cell::new(0.0_f32));
    model.set_edit_sink(move |_record| {
        step.set(step.get() + 1.0);
        writer.set_local_pos(id, Point::new(step.get(), 0.0));
    });

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        model.set_local_pos(id, Point::new(0.5, 0.0));
    }));
    std::panic::set_hook(hook);

    let err = outcome.expect_err("an unconverging sink must be stopped");
    let message = err
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        message.contains("edit sink did not settle"),
        "the diagnostic must name the sink: {message}"
    );
}

#[test]
fn a_transaction_that_changed_nothing_is_never_delivered() {
    // Without this, a converging sink loops forever against its own no-op.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_local_pos(id, Point::new(5.0, 0.0));
    let records = recording_sink(&model);

    model.set_local_pos(id, Point::new(5.0, 0.0));
    {
        let _txn = model.user_edit();
    }

    assert!(records.borrow().is_empty());
}

#[test]
fn the_transaction_signal_fires_once_per_committed_transaction() {
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let b = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    // A sink has to be installed for a record to exist at all.
    let _records = recording_sink(&model);
    let fires = Rc::new(std::cell::Cell::new(0u32));
    let counter = fires.clone();
    let _h = model
        .transaction_signal()
        .observe(move |_| counter.set(counter.get() + 1));

    {
        let _txn = model.user_edit();
        model.set_local_pos(a, Point::new(1.0, 0.0));
        model.set_local_pos(b, Point::new(2.0, 0.0));
    }

    assert_eq!(
        fires.get(),
        1,
        "once per gesture, not once per intermediate write"
    );
}

#[test]
fn the_sink_sees_a_scene_every_view_has_already_reconciled() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let obs = order.clone();
    let _h = model
        .item_change_signal()
        .observe(move |_| obs.borrow_mut().push("change"));
    let snk = order.clone();
    model.set_edit_sink(move |_| snk.borrow_mut().push("record"));

    model.set_local_pos(id, Point::new(1.0, 0.0));

    assert_eq!(*order.borrow(), vec!["change", "record"]);
}

#[test]
fn a_writing_observer_does_not_pull_a_record_ahead_of_the_change_fan_out() {
    // The subtle half of the ordering promise. A change observer that writes
    // the scene commits its own transaction *from inside* the drain; delivering
    // records from there would hand the sink a transaction whose changes some
    // observers had not seen yet. The records wait for the drain to settle.
    let model = SceneModel::new();
    let a = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let b = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let order: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // Observer 1 writes another item back when it sees `a` move.
    let writer = model.clone();
    let seen_first = order.clone();
    let _h1 = model.item_change_signal().observe(move |c| {
        seen_first.borrow_mut().push(format!("obs1:{:?}", c.id()));
        if c.id() == a && matches!(c.change, ItemChange::LocalPosChanged { .. }) {
            writer.set_opacity(b, 0.5);
        }
    });
    // Observer 2 must still see every change before any record is delivered.
    let seen_second = order.clone();
    let _h2 = model
        .item_change_signal()
        .observe(move |c| seen_second.borrow_mut().push(format!("obs2:{:?}", c.id())));
    let snk = order.clone();
    model.set_edit_sink(move |r| snk.borrow_mut().push(format!("record:{:?}", r.txn)));

    model.set_local_pos(a, Point::new(1.0, 0.0));

    let log = order.borrow();
    let first_record = log
        .iter()
        .position(|e| e.starts_with("record:"))
        .expect("at least one record");
    let last_change = log
        .iter()
        .rposition(|e| !e.starts_with("record:"))
        .expect("at least one change");
    assert!(
        first_record > last_change,
        "no record may be delivered while changes are still fanning out: {log:?}"
    );
}

#[test]
fn abandon_tags_the_record_and_does_not_roll_the_scene_back() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);

    {
        let txn = model.user_edit();
        model.set_local_pos(id, Point::new(9.0, 0.0));
        txn.abandon();
    }

    assert_eq!(
        model.local_pos(id),
        Some(Point::new(9.0, 0.0)),
        "the framework owns no inverse-application routine, by doctrine"
    );
    let seen = records.borrow();
    assert_eq!(seen[0].outcome, TxnOutcome::Abandoned);
    // And the record carries what is needed to revert it.
    let SceneEdit::Change(ItemChange::LocalPosChanged { old, .. }) = &seen[0].edits[0] else {
        panic!("expected a move")
    };
    assert_eq!(*old, Point::ZERO);
}

#[test]
fn squash_is_off_by_default_and_keeps_the_whole_path() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);

    {
        let _txn = model.user_edit();
        model.set_local_pos(id, Point::new(1.0, 0.0));
        model.set_local_pos(id, Point::new(2.0, 0.0));
        model.set_local_pos(id, Point::new(3.0, 0.0));
    }

    assert_eq!(
        records.borrow()[0].edits.len(),
        3,
        "losing the intermediate path by default would be silent data loss for a \
         replay or presence consumer"
    );
}

#[test]
fn squash_folds_the_path_to_its_endpoints_when_asked() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);

    {
        let _txn = model.user_edit().squash();
        model.set_local_pos(id, Point::new(1.0, 0.0));
        model.set_local_pos(id, Point::new(2.0, 0.0));
        model.set_local_pos(id, Point::new(3.0, 0.0));
    }

    let seen = records.borrow();
    assert_eq!(seen[0].edits.len(), 1);
    let SceneEdit::Change(ItemChange::LocalPosChanged { old, new, .. }) = &seen[0].edits[0] else {
        panic!("expected one move")
    };
    assert_eq!(*old, Point::ZERO);
    assert_eq!(*new, Point::new(3.0, 0.0));
}

// -----------------------------------------------------------------
// Atomic placement and fractional z
// -----------------------------------------------------------------

#[test]
fn set_placement_writes_four_fields_as_one_change() {
    let model = SceneModel::new();
    let parent = model.add_item(boxed_rect(0.0, 10.0), Point::new(100.0, 100.0));
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    let target = Placement::new(
        Some(parent),
        3.0,
        Point::new(5.0, 6.0),
        Transform2D::rotate(0.25),
    );
    model.set_placement(id, target);

    let seen = log.borrow();
    assert_eq!(
        seen.len(),
        1,
        "four separate mutators would be four events and three intermediate states"
    );
    let ItemChange::PlacementChanged { old, new, .. } = &seen[0].change else {
        panic!("expected PlacementChanged, got {:?}", seen[0].change)
    };
    assert_eq!(old.parent, None);
    assert_eq!(*new, target);
    assert_eq!(model.placement(id), Some(target));
    assert_eq!(model.parent_of(id), Some(parent));
}

#[test]
fn set_placement_refuses_a_cycle_whole_rather_than_half_applying_it() {
    let model = SceneModel::new();
    let root = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let child = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    model.set_item_parent(child, Some(root));
    let before = model.placement(root).unwrap();

    model.set_placement(
        root,
        Placement::new(
            Some(child),
            99.0,
            Point::new(50.0, 50.0),
            Transform2D::identity(),
        ),
    );

    assert_eq!(
        model.placement(root),
        Some(before),
        "a partly-applied atomic write is the thing this door exists to prevent"
    );
}

#[test]
fn reparent_keeping_scene_pos_holds_the_item_visually_still() {
    let model = SceneModel::new();
    let group = model.add_item(
        GroupItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)),
        Point::new(80.0, 40.0),
    );
    model.set_transform(group, Transform2D::scale(2.0, 2.0));
    let card = model.add_item(boxed_rect(0.0, 10.0), Point::new(30.0, 60.0));
    let before = model.scene_pos(card).expect("rooted");

    model.reparent_keeping_scene_pos(card, Some(group));

    assert_eq!(model.parent_of(card), Some(group));
    let after = model.scene_pos(card).expect("parented");
    assert!(
        (after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3,
        "drag-into-group must not move the card: {before:?} -> {after:?}"
    );
}

#[test]
fn reparent_keeping_scene_pos_is_one_change() {
    let model = SceneModel::new();
    let group = model.add_item(
        GroupItem::new(Rect::new(0.0, 0.0, 200.0, 200.0)),
        Point::new(80.0, 40.0),
    );
    let card = model.add_item(boxed_rect(0.0, 10.0), Point::new(30.0, 60.0));
    let (log, _h) = recording_observer(&model);

    model.reparent_keeping_scene_pos(card, Some(group));

    let seen = log.borrow();
    assert_eq!(seen.len(), 1);
    assert!(matches!(
        seen[0].change,
        ItemChange::PlacementChanged { .. }
    ));
}

#[test]
fn z_between_finds_room_and_says_when_there_is_none() {
    let mut scene = Scene::new();
    let lower = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let upper = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    scene.set_z(lower, 1.0);
    scene.set_z(upper, 2.0);
    let z = scene.z_between(lower, upper).unwrap();
    assert!(z > 1.0 && z < 2.0);

    // Same z: no room, and saying so is the point — `set_z` ignores a write
    // within `f32::EPSILON` of the current value, so a caller that computed an
    // exhausted midpoint itself would get a silent no-op.
    scene.set_z(upper, 1.0);
    assert_eq!(scene.z_between(lower, upper), None);
}

#[test]
fn z_between_reports_exhaustion_rather_than_bisecting_into_a_silent_no_op() {
    let mut scene = Scene::new();
    let lower = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let upper = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let middle = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    scene.set_z(lower, 1.0);
    scene.set_z(upper, 2.0);

    // Bisect repeatedly at the same locus. An `f32` carries ~24 mantissa bits,
    // so this runs out; the loop must end by being *told*, not by silently
    // writing the same value forever.
    let mut steps = 0;
    while let Some(z) = scene.z_between(lower, upper) {
        scene.set_z(middle, z);
        scene.set_z(upper, z);
        steps += 1;
        assert!(steps < 200, "bisection should exhaust long before this");
    }
    assert!(steps > 10, "there really was room to bisect at first");
    assert_eq!(scene.z_between(lower, upper), None);
}

#[test]
fn z_between_is_order_insensitive_and_rejects_unknown_ids() {
    let mut scene = Scene::new();
    let a = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let b = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    scene.set_z(a, 1.0);
    scene.set_z(b, 5.0);
    assert_eq!(scene.z_between(a, b), scene.z_between(b, a));

    let ghost = {
        let id = scene.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
        scene.remove(id);
        id
    };
    assert_eq!(scene.z_between(a, ghost), None);
}

// -----------------------------------------------------------------
// The framework's own edits are stamped
// -----------------------------------------------------------------

#[test]
fn a_programmatic_move_is_not_reported_as_a_user_edit() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    model.set_local_pos(id, Point::new(1.0, 0.0));

    assert_eq!(log.borrow()[0].source, ChangeSource::Programmatic);
}

/// A real pointer drag of a lightweight item, end to end: press, move, release,
/// then the layout pass in which `SceneView::build` drains the commit.
fn drag_item_through_a_view(
    model: &SceneModel,
    from: Point,
    to: Point,
) -> Rc<RefCell<Vec<SceneChange>>> {
    use teksilo_canvas::SizeProposal;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_scene::SceneView;

    let mut tree = WidgetTree::new();
    // The built-in item drag is registered only when the view has a selection
    // mode or magnetism (one `on_drag` handler serves both), so a bare view
    // would never arm it.
    tree.add(
        SceneView::with_model(model.clone())
            .selection_mode(teksilo_scene::SceneSelectionMode::Multi),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // Observed only from here, so the scene's own set-up does not appear.
    let (log, handle) = recording_observer(model);

    tree.pointer_move(from);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    // Real apps drain the commit from the layout pass the drag's
    // `reconcile_dirty` bump scheduled.
    tree.layout(SizeProposal::exact(400.0, 300.0));

    drop(handle);
    log
}

#[test]
fn a_real_drag_commit_is_stamped_user_by_the_framework() {
    // The framework's own gesture code has to open the transaction, because an
    // app cannot reach it. Driving `SceneModel::user_edit` by hand would test
    // the stamping mechanism and not the wiring — this drives the pointer.
    let model = SceneModel::new();
    let id = model.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0))
            .fill(Color::RED)
            .draggable(true),
        Point::ZERO,
    );

    let log = drag_item_through_a_view(&model, Point::new(60.0, 60.0), Point::new(100.0, 100.0));

    let seen = log.borrow();
    let moved: Vec<&SceneChange> = seen
        .iter()
        .filter(|c| matches!(c.change, ItemChange::LocalPosChanged { .. }))
        .collect();
    assert_eq!(
        moved.len(),
        1,
        "the built-in drag commits once; saw {seen:?}"
    );
    assert_eq!(moved[0].id(), id);
    assert_eq!(
        moved[0].source,
        ChangeSource::User,
        "a finished drag must be distinguishable from a programmatic move"
    );
    assert_eq!(moved[0].history, HistoryMode::Record);
    assert!(!moved[0].ephemeral);
}

#[test]
fn a_real_drag_of_a_multi_selection_is_one_transaction() {
    use teksilo_scene::{SceneSelection, SceneSelectionMode};

    let model = SceneModel::new();
    let a = model.add_item(
        RectItem::new(Rect::new(50.0, 50.0, 30.0, 30.0))
            .fill(Color::RED)
            .draggable(true),
        Point::ZERO,
    );
    let b = model.add_item(
        RectItem::new(Rect::new(150.0, 50.0, 30.0, 30.0))
            .fill(Color::RED)
            .draggable(true),
        Point::ZERO,
    );
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.replace([a, b]);

    use teksilo_canvas::SizeProposal;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_scene::SceneView;

    let mut tree = WidgetTree::new();
    tree.add(SceneView::with_model(model.clone()).selection_model(selection));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let (log, _h) = recording_observer(&model);

    let from = Point::new(60.0, 60.0);
    let to = Point::new(100.0, 100.0);
    tree.pointer_move(from);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::default(),
    ));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    let seen = log.borrow();
    let moved: Vec<&SceneChange> = seen
        .iter()
        .filter(|c| matches!(c.change, ItemChange::LocalPosChanged { .. }))
        .collect();
    assert_eq!(moved.len(), 2, "both selected items moved");
    assert_eq!(
        moved[0].txn, moved[1].txn,
        "dragging a two-item selection must be one undo step, not two"
    );
    assert!(moved.iter().all(|c| c.source == ChangeSource::User));
}

#[test]
fn a_transform_delta_inside_a_user_transaction_stamps_every_change_it_makes() {
    use teksilo_scene::TransformDelta;

    // `apply_transform_delta` is the single place the selection transform
    // controller writes the model, and `SceneView::build` wraps exactly this
    // call in a `User` transaction. This drives the *stamping* — that one call
    // can emit several changes and they must all carry the stamp. That
    // `build` is what opens the transaction is a separate claim, and it is the
    // real-pointer drag tests above that make it.
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let (log, _h) = recording_observer(&model);

    {
        let _txn = model.user_edit();
        model.apply_transform_delta(
            &[id],
            &TransformDelta::new(
                Point::ZERO,
                0.0,
                teksilo_canvas::Vec2::new(1.0, 1.0),
                0.0,
                teksilo_canvas::Vec2::new(5.0, 0.0),
            ),
        );
    }

    let seen = log.borrow();
    assert!(!seen.is_empty());
    assert!(
        seen.iter().all(|c| c.source == ChangeSource::User),
        "without the stamp an app cannot tell a finished gesture from a \
         programmatic move"
    );
}

// -----------------------------------------------------------------
// Sanity: the seam does not change what a path item reports
// -----------------------------------------------------------------

#[test]
fn a_path_items_stroke_swap_still_refits_its_bounds() {
    // `PathItem::set_stroke` recomputes its own bounds, and the appearance
    // write must not have lost that side effect while learning to report `old`.
    let mut path = teksilo_canvas::Path::new();
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(100.0, 0.0));
    let mut scene = Scene::new();
    let id = scene.add_item(PathItem::new(path), Point::ZERO);
    let thin = scene.item(id).unwrap().local_bounds().height;

    scene.set_item_stroke(id, Color::RED, StrokeStyle::solid(20.0));

    let thick = scene.item(id).unwrap().local_bounds().height;
    assert!(
        thick > thin,
        "a wider stroke widens the band: {thin} -> {thick}"
    );
}

// -----------------------------------------------------------------
// What identity does and does not buy
// -----------------------------------------------------------------

#[test]
fn a_removal_does_not_clear_a_views_selection_and_a_restore_does_not_re_add() {
    // Worth pinning because it is easy to read "restore keeps the id" as
    // "restore keeps the selection". It does not: `SceneSelection` is
    // **per view** and lives outside the `Scene`, so a removal cannot reach it
    // and a restore does not either. The id still being selected afterwards is
    // a consequence of nothing having cleared it, not a property of this
    // design — and an app that prunes its own selection on `Removed` (which it
    // should) has to re-add on the restore.
    use teksilo_scene::{SceneSelection, SceneSelectionMode};

    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let selection = SceneSelection::new(SceneSelectionMode::Multi);
    selection.select_one(id);
    assert!(selection.is_selected(id));

    let salvage = model.take(id);
    assert!(
        selection.is_selected(id),
        "the scene cannot reach a per-view selection"
    );
    model.restore_all(salvage).unwrap();
    assert!(selection.is_selected(id));
}

// -----------------------------------------------------------------
// One mutation, one record shape — whichever door made it
// -----------------------------------------------------------------

/// `set_flag` used to emit `VisibilityChanged` **and** `FlagsChanged` while
/// `set_flags` flipping the same bit emitted only the second, so an app showing
/// "N changes in this edit" reported 2 for hiding a card and 1 for the same
/// hide through the other door.
///
/// Both doors now announce the same pair, and the derived
/// `VisibilityChanged` stays out of the record — so the *edit* count is 1 both
/// ways. Delete the `is_edit()` test in `Scene::emit_item_change`, or the
/// visibility branch in `announce_flags`, and this reddens.
#[test]
fn hiding_a_card_is_one_edit_and_two_notifications_through_either_door() {
    for door in ["set_visible", "set_flags"] {
        let model = SceneModel::new();
        let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
        let records = recording_sink(&model);
        let (log, _h) = recording_observer(&model);

        match door {
            "set_visible" => model.set_visible(id, false),
            _ => {
                let mut hidden = model.flags(id).expect("live");
                hidden.set(teksilo_scene::ItemFlags::IS_VISIBLE, false);
                model.set_flags(id, hidden);
            }
        }

        let seen = log.borrow();
        assert_eq!(
            seen.len(),
            2,
            "{door}: the convenience notification and the change itself: {seen:?}"
        );
        assert!(
            matches!(
                seen[0].change,
                ItemChange::VisibilityChanged { visible: false, .. }
            ),
            "{door}: the derived notification comes first: {:?}",
            seen[0].change
        );
        assert!(!seen[0].change.is_edit(), "{door}: and is not an edit");
        assert!(
            matches!(seen[1].change, ItemChange::FlagsChanged { .. }),
            "{door}: {:?}",
            seen[1].change
        );
        assert!(seen[1].change.is_edit(), "{door}");

        let kept = records.borrow();
        assert_eq!(kept.len(), 1, "{door}: one transaction");
        assert_eq!(
            kept[0].edits.len(),
            1,
            "{door}: hiding a card is ONE edit: {:?}",
            kept[0].edits
        );
        assert!(
            matches!(
                kept[0].edits[0],
                SceneEdit::Change(ItemChange::FlagsChanged { .. })
            ),
            "{door}: and it is the one that carries both bitsets: {:?}",
            kept[0].edits
        );
    }
}

/// `set_item_handlers` is the handler door that knows both sides, so it
/// produces a reversible edit like every other mutator. `HandlersChanged` used
/// to carry only the id — an entry in a record whose whole claim is that every
/// entry carries both sides of what it replaced.
///
/// Make `set_item_handlers` emit `replaced: None` and this reddens.
#[test]
fn set_item_handlers_records_both_sides() {
    use teksilo_scene::SceneItemHandlerSet;

    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);

    let mut first = SceneItemHandlerSet::new();
    first.on_tap(|_, _| {});
    model.set_item_handlers(id, Some(first));

    let mut second = SceneItemHandlerSet::new();
    second.on_double_tap(|_, _| {});
    model.set_item_handlers(id, Some(second));

    model.set_item_handlers(id, None);

    let kept = records.borrow();
    assert_eq!(kept.len(), 3, "three mutations, three transactions");

    // Install: nothing there before, something there now.
    let SceneEdit::Change(ItemChange::HandlersChanged { replaced, .. }) = &kept[0].edits[0] else {
        panic!("expected a HandlersChanged: {:?}", kept[0].edits);
    };
    let replaced = replaced
        .as_ref()
        .expect("set_item_handlers knows both sides");
    assert!(replaced.old.is_none(), "the item had no handlers");
    assert!(replaced.new.as_ref().is_some_and(|h| h.on_tap.is_some()));

    // Replace: the old set is recoverable, which is what an inverse needs.
    let SceneEdit::Change(ItemChange::HandlersChanged { replaced, .. }) = &kept[1].edits[0] else {
        panic!("expected a HandlersChanged");
    };
    let replaced = replaced.as_ref().expect("both sides");
    assert!(
        replaced.old.as_ref().is_some_and(|h| h.on_tap.is_some()),
        "the set that was there is in the record, not just its absence"
    );
    assert!(
        replaced
            .new
            .as_ref()
            .is_some_and(|h| h.on_double_tap.is_some())
    );

    // Clear: the old set is still recoverable.
    let SceneEdit::Change(ItemChange::HandlersChanged { replaced, .. }) = &kept[2].edits[0] else {
        panic!("expected a HandlersChanged");
    };
    let replaced = replaced.as_ref().expect("both sides");
    assert!(replaced.old.is_some(), "a clear is a replacement too");
    assert!(replaced.new.is_none());
}

/// A consumer can actually put them back — the point of carrying both sides.
#[test]
fn a_consumer_can_reverse_a_handler_replacement_from_the_record() {
    use teksilo_scene::SceneItemHandlerSet;

    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let mut original = SceneItemHandlerSet::new();
    original.on_tap(|_, _| {});
    model.set_item_handlers(id, Some(original));

    let records = recording_sink(&model);
    model.set_item_handlers(id, None);
    assert_eq!(
        model.with_handlers_mut(id, |h| h.on_tap.is_some()),
        Some(false)
    );

    // Walk the record in reverse and apply each `old` — the documented inverse.
    let restored = {
        let kept = records.borrow();
        let SceneEdit::Change(ItemChange::HandlersChanged { replaced, .. }) = &kept[0].edits[0]
        else {
            panic!("expected a HandlersChanged");
        };
        replaced.as_ref().expect("both sides").old.clone()
    };
    model.set_item_handlers(id, restored);
    assert_eq!(
        model.with_handlers_mut(id, |h| h.on_tap.is_some()),
        Some(true),
        "the record was complete enough to invert"
    );
}

/// `handlers_mut` fires on the way *in* and cannot say what the handlers became,
/// so it announces (a cache has to know) and records nothing (a record that
/// claims both sides must not carry an entry with neither).
///
/// Make it record and `every_recorded_edit_is_an_edit` reddens too.
#[test]
fn handlers_mut_announces_but_does_not_record() {
    let model = SceneModel::new();
    let id = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);
    let (log, _h) = recording_observer(&model);

    model.with_handlers_mut(id, |h| {
        h.on_tap(|_, _| {});
    });

    let seen = log.borrow();
    assert_eq!(seen.len(), 1, "the invalidation is announced");
    assert!(matches!(
        seen[0].change,
        ItemChange::HandlersChanged { replaced: None, .. }
    ));
    assert!(
        !seen[0].change.is_edit(),
        "it describes nothing, so it is not an edit"
    );
    assert!(
        records.borrow().is_empty(),
        "an empty transaction is never delivered: {:?}",
        records.borrow()
    );
}

/// The journal's own claim, checked across every mutator in one transaction:
/// **every** entry in `edits` is either a `Removed` carrying its salvage or a
/// change that carries both sides of what it replaced.
///
/// The one that used to fail it was `HandlersChanged`, which carried neither.
#[test]
fn every_recorded_edit_is_an_edit() {
    let model = SceneModel::new();
    let parent = model.add_item(boxed_rect(0.0, 40.0), Point::ZERO);
    let child = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let doomed = model.add_item(boxed_rect(0.0, 10.0), Point::ZERO);
    let records = recording_sink(&model);

    {
        let mut scene = model.write_guard();
        scene.set_local_pos(child, Point::new(3.0, 4.0));
        scene.set_local_bounds(child, Rect::new(0.0, 0.0, 12.0, 12.0));
        scene.set_transform(child, Transform2D::rotate(0.3));
        scene.set_visible(child, false);
        scene.set_opacity(child, 0.5);
        scene.set_z(child, 2.5);
        scene.set_layer(child, teksilo_scene::SceneLayer::Over);
        scene.set_item_parent(child, Some(parent));
        scene.set_item_fill(child, Color::RED);
        scene.set_item_handlers(child, Some(teksilo_scene::SceneItemHandlerSet::new()));
        scene
            .handlers_mut(child)
            .expect("live")
            .cursor(teksilo_core::widget::CursorIcon::Pointer);
        scene.remove(doomed);
    }

    let kept = records.borrow();
    assert_eq!(kept.len(), 1, "one write scope, one transaction");
    assert!(
        kept[0].edits.len() >= 11,
        "every mutator above contributed: {:?}",
        kept[0].edits
    );
    for edit in &kept[0].edits {
        match edit {
            SceneEdit::Removed { salvage, .. } => {
                assert!(
                    matches!(salvage, Salvage::Owned(_)),
                    "a removal hands its contents over"
                );
            }
            SceneEdit::Change(change) => assert!(
                change.is_edit(),
                "a derived notification reached the record: {change:?}"
            ),
            // `SceneEdit` is `#[non_exhaustive]`; a variant added later is a
            // new claim about what a record carries and wants its own arm here.
            other => panic!("unhandled SceneEdit variant: {other:?}"),
        }
    }
    assert!(
        !kept[0]
            .edits
            .iter()
            .any(|e| matches!(e, SceneEdit::Change(ItemChange::VisibilityChanged { .. }))),
        "the derived visibility notification is not an edit: {:?}",
        kept[0].edits
    );
}

// -----------------------------------------------------------------
// A salvage crossing a scene boundary
// -----------------------------------------------------------------

/// Cut a card out of one board and paste it into another. Supported, and
/// documented on `Scene::restore` as supported — `ItemId` is process-global, so
/// the id cannot collide, and the salvage carries its whole entry rather than
/// an index into the scene it came from.
#[test]
fn a_root_salvage_moves_between_two_scenes() {
    let source = SceneModel::new();
    let target = SceneModel::new();
    let id = source.add_item(boxed_rect(0.0, 10.0), Point::new(7.0, 9.0));
    source.set_z(id, 3.0);
    source.set_visible(id, false);

    let salvage = source.take(id);
    assert_eq!(source.len(), 0);

    assert_eq!(target.restore_all(salvage).unwrap(), vec![id]);
    assert_eq!(target.len(), 1);
    assert_eq!(target.local_pos(id), Some(Point::new(7.0, 9.0)));
    assert_eq!(target.z(id), Some(3.0));
    assert!(
        !target.is_effectively_visible(id),
        "flags cross the boundary with the entry"
    );
    // And it is a first-class member of the target: indexed, hit-testable once
    // shown, and reachable by the same id.
    target.set_visible(id, true);
    assert!(
        target
            .items_in_rect(Rect::new(0.0, 0.0, 100.0, 100.0))
            .contains(&id)
    );
}

/// A *parented* salvage names a parent that is still in the other scene, so the
/// same roots-before-children rule refuses it — and `RemovedItem::detach` is
/// the documented way to bring it over as a root.
#[test]
fn a_parented_salvage_is_refused_by_a_foreign_scene_until_detached() {
    let source = SceneModel::new();
    let target = SceneModel::new();
    let parent = source.add_item(boxed_rect(0.0, 40.0), Point::ZERO);
    let child = source.add_item(boxed_rect(0.0, 10.0), Point::new(2.0, 2.0));
    source.set_item_parent(child, Some(parent));

    let mut salvage: Vec<RemovedItem> = source.take(child);
    assert_eq!(salvage.len(), 1);
    assert_eq!(salvage[0].parent(), Some(parent));

    // As taken: refused, because the parent is not here.
    let refused = target.restore_all(salvage).unwrap_err();
    assert_eq!(refused, RestoreError::MissingParent { id: child, parent });
    assert_eq!(target.len(), 0);

    // Detached: accepted, at root level, keeping its numbers.
    let child2 = source.add_item(boxed_rect(0.0, 10.0), Point::new(2.0, 2.0));
    source.set_item_parent(child2, Some(parent));
    salvage = source.take(child2);
    salvage[0].detach();
    assert_eq!(target.restore_all(salvage).unwrap(), vec![child2]);
    assert_eq!(target.parent_of(child2), None);
    assert_eq!(target.local_pos(child2), Some(Point::new(2.0, 2.0)));
}
