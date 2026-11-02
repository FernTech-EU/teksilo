// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where focus goes when the widget that had it, or the one it was meant to
//! return to, is gone or disabled: the chain in `focus_fallback_impl`.

use teksilo_canvas::SizeProposal;

use super::WidgetTree;
use crate::event::{Key, Modifiers};
use crate::overlay::{DismissBehavior, OverlayId, OverlayPlacement, OverlayRequest};
use crate::signal::Signal;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// A focusable leaf as a child of `parent`.
fn field(tree: &mut WidgetTree, parent: WidgetId) -> WidgetId {
    tree.add_child(parent, FillWidget::new().focusable())
}

/// Show `content` as an overlay of `placement` attached to `anchor`, closed by
/// Escape, with `restore` recorded as where focus goes back to.
fn show(
    tree: &mut WidgetTree,
    content: WidgetId,
    anchor: WidgetId,
    placement: OverlayPlacement,
    restore: Option<WidgetId>,
) -> OverlayId {
    let id = tree.show_overlay(OverlayRequest::new(
        content,
        anchor,
        placement,
        DismissBehavior::EscapeKey,
    ));
    if let Some(restore) = restore {
        tree.record_top_focus_restore(restore);
    }
    lay_out(tree);
    id
}

/// A modal holding one focusable field, opened over `restore`, with focus on
/// its field. Returns the modal's content and field.
fn modal_over(
    tree: &mut WidgetTree,
    anchor: WidgetId,
    restore: Option<WidgetId>,
) -> (WidgetId, WidgetId) {
    let content = tree.add(StackWidget::new());
    let inside = field(tree, content);
    show(tree, content, anchor, OverlayPlacement::Centered, restore);
    tree.focus(inside);
    (content, inside)
}

/// The step-1 case, as before: the target is alive, focus goes back to it.
#[test]
fn escape_from_a_modal_returns_focus_to_its_live_target() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let editor = field(&mut tree, root);
    lay_out(&mut tree);
    tree.focus(editor);
    modal_over(&mut tree, root, Some(editor));

    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(editor));
}

/// A dialog opened from a menu row records the row as its restore target. The
/// menu closes while the dialog is up; Escape then sends focus where it was
/// before the menu opened (forwarding, recorded when the target was), not to
/// nothing.
#[test]
fn a_dialog_opened_from_a_menu_row_returns_focus_to_where_the_menu_was_opened_from() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    // The menu button first: the window's first Tab stop is not the editor,
    // so only the forwarding can land there.
    let menu_button = field(&mut tree, root);
    let editor = field(&mut tree, root);
    lay_out(&mut tree);
    tree.focus(editor);

    let menu = tree.add(StackWidget::new());
    let row = field(&mut tree, menu);
    let menu_overlay = show(
        &mut tree,
        menu,
        menu_button,
        OverlayPlacement::Below,
        Some(editor),
    );
    tree.focus(row);
    let (_, inside) = modal_over(&mut tree, row, Some(row));
    assert_eq!(tree.focused(), Some(inside));

    let mut noop = crate::window::NoopWindowOps;
    tree.dismiss_overlay_with_ops(menu_overlay, &mut noop);
    assert_eq!(tree.focused(), Some(inside), "the menu held no focus");

    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(
        tree.focused(),
        Some(editor),
        "the row went with its menu: focus goes where the menu was opened from"
    );
}

/// The target was destroyed while the modal was up: focus lands inside the
/// innermost ancestor of the target that survived (step 4), the anchor not
/// being focusable.
#[test]
fn a_destroyed_target_hands_focus_to_its_surviving_ancestor() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let before = field(&mut tree, root);
    let anchor = tree.add_child(root, FillWidget::new());
    let group = tree.add_child(root, StackWidget::new());
    let target = field(&mut tree, group);
    let sibling = field(&mut tree, group);
    lay_out(&mut tree);
    tree.focus(target);
    modal_over(&mut tree, anchor, Some(target));

    tree.destroy_subtree_for_testing(target);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(sibling), "not {before:?}, not nothing");
}

/// No surviving ancestor, a focusable anchor: the anchor (step 3).
#[test]
fn with_its_ancestors_gone_focus_goes_to_the_anchor() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let _first = field(&mut tree, root);
    let anchor = field(&mut tree, root);
    let group = tree.add_child(root, StackWidget::new());
    let target = field(&mut tree, group);
    lay_out(&mut tree);
    tree.focus(target);
    modal_over(&mut tree, anchor, Some(target));

    tree.destroy_subtree_for_testing(group);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(anchor));
}

/// The target and its ancestors gone, nothing focusable at or above the
/// anchor: the first Tab stop of the window (step 5).
#[test]
fn with_nothing_nearer_focus_goes_to_the_first_tab_stop() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let first = field(&mut tree, root);
    let _second = field(&mut tree, root);
    let anchor = tree.add_child(root, FillWidget::new());
    let group = tree.add_child(root, StackWidget::new());
    let target = field(&mut tree, group);
    lay_out(&mut tree);
    tree.focus(target);
    modal_over(&mut tree, anchor, Some(target));

    tree.destroy_subtree_for_testing(group);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(first));
}

/// Nothing was focused when the modal opened, so none was recorded: focus
/// goes back to nothing, where it was.
#[test]
fn with_nothing_focused_before_focus_goes_back_to_nothing() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let _first = field(&mut tree, root);
    let anchor = field(&mut tree, root);
    lay_out(&mut tree);
    modal_over(&mut tree, anchor, None);

    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(tree.focused(), None);
}

/// A path that used to restore nothing (`dismiss_overlay`, the scrim's click):
/// focus held inside goes back to the overlay's target.
#[test]
fn dismiss_overlay_returns_focus_to_the_target() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let trigger = field(&mut tree, root);
    lay_out(&mut tree);
    tree.focus(trigger);
    let content = tree.add(StackWidget::new());
    let inside = field(&mut tree, content);
    let overlay = show(
        &mut tree,
        content,
        trigger,
        OverlayPlacement::Centered,
        Some(trigger),
    );
    tree.focus(inside);

    let mut noop = crate::window::NoopWindowOps;
    tree.dismiss_overlay_with_ops(overlay, &mut noop);
    assert_eq!(tree.focused(), Some(trigger));
}

/// The fallback never takes focus from where the user put it: an overlay
/// dismissed while focus is on a live control outside it leaves focus there.
#[test]
fn the_fallback_leaves_focus_the_user_placed() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let elsewhere = field(&mut tree, root);
    let anchor = field(&mut tree, root);
    let group = tree.add_child(root, StackWidget::new());
    let target = field(&mut tree, group);
    lay_out(&mut tree);
    let content = tree.add(StackWidget::new());
    let _inside = field(&mut tree, content);
    let overlay = show(
        &mut tree,
        content,
        anchor,
        OverlayPlacement::Below,
        Some(target),
    );
    tree.destroy_subtree_for_testing(group);
    tree.focus(elsewhere);

    let mut noop = crate::window::NoopWindowOps;
    tree.dismiss_overlay_with_ops(overlay, &mut noop);
    assert_eq!(tree.focused(), Some(elsewhere));
}

/// Nested modals: the inner one's target, in the outer one, is destroyed; focus
/// falls back inside the outer modal, never behind it.
#[test]
fn a_nested_modal_falls_back_inside_the_outer_modal() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let behind = field(&mut tree, root);
    lay_out(&mut tree);
    tree.focus(behind);
    let outer = tree.add(StackWidget::new());
    let outer_first = field(&mut tree, outer);
    let outer_target = field(&mut tree, outer);
    show(
        &mut tree,
        outer,
        behind,
        OverlayPlacement::Centered,
        Some(behind),
    );
    tree.focus(outer_target);
    modal_over(&mut tree, outer_target, Some(outer_target));

    tree.destroy_subtree_for_testing(outer_target);
    tree.press_key(Key::Escape, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(outer_first));
}

// ── N18: the rebuild restore ────────────────────────────────────────────

/// A focusable composite whose rebuild replaces its one, non-focusable child.
#[derive(Debug)]
struct Rebuilding {
    epoch: Signal<u64>,
    child: Option<WidgetId>,
}

impl crate::widget::Widget for Rebuilding {
    fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
        let sid = ctx.self_id();
        let reg = ctx.binding_registry();
        self.epoch
            .bind_to(sid, reg, crate::binding::BindingLevel::Rebuild);
        let child = ctx.add(FillWidget::new());
        self.child = Some(child);
        vec![child]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &crate::widget::LayoutContext,
    ) -> crate::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

/// Focus was on a non-focusable node (a tree row a screen reader acted on)
/// that a rebuild replaced, inside a rebuilt owner with nothing focusable
/// (a tree's body pane): it lands on the nearest focusable above the owner
/// (the tree), not on nothing.
#[test]
fn a_rebuild_that_replaces_a_non_focusable_focused_node_keeps_focus_nearby() {
    let mut tree = WidgetTree::new();
    let epoch = Signal::new(0u64);
    let root = tree.add(StackWidget::new());
    let _first = field(&mut tree, root);
    let view = tree.add_child(root, StackWidget::new().focusable(true));
    let pane = tree.add_child(
        view,
        Rebuilding {
            epoch: epoch.clone(),
            child: None,
        },
    );
    lay_out(&mut tree);
    let row = tree.children(pane)[0];
    tree.focus(row);

    epoch.set(1);
    lay_out(&mut tree);
    assert_ne!(tree.children(pane)[0], row, "the row was replaced");
    assert_eq!(tree.focused(), Some(view));
}

/// A rebuild whose subtree also went dormant in the same pass (a popover
/// closing) drags no focus anywhere: the fallback needs a surviving root.
#[test]
fn a_rebuild_that_went_dormant_leaves_focus_alone() {
    let mut tree = WidgetTree::new();
    let epoch = Signal::new(0u64);
    let shown = Signal::new(true);
    let root = tree.add(StackWidget::new());
    let _first = field(&mut tree, root);
    let view = tree.add_child(
        root,
        StackWidget::new()
            .focusable(true)
            .visible_when(shown.clone()),
    );
    let pane = tree.add_child(
        view,
        Rebuilding {
            epoch: epoch.clone(),
            child: None,
        },
    );
    lay_out(&mut tree);
    let row = tree.children(pane)[0];
    tree.focus(row);

    epoch.set(1);
    shown.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), None);
}

// ── N25: a focused node that becomes disabled ───────────────────────────

/// Three fields under a root, each with its own `enabled` signal.
struct Row {
    tree: WidgetTree,
    ids: [WidgetId; 3],
    enabled: [Signal<bool>; 3],
}

fn row_of_three() -> Row {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let ids = [0, 1, 2].map(|_| field(&mut tree, root));
    let enabled = [0, 1, 2].map(|_| Signal::new(true));
    for (id, signal) in ids.iter().zip(&enabled) {
        tree.enabled_when(*id, signal.clone());
    }
    lay_out(&mut tree);
    Row { tree, ids, enabled }
}

#[test]
fn a_disabled_focused_control_hands_focus_to_the_next() {
    let Row {
        mut tree,
        ids: [a, b, c],
        enabled,
    } = row_of_three();
    tree.focus(b);
    let visible = tree.focus_visible_signal().get();
    enabled[1].set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(c));
    // Another widget, not the same one restored: a text field landed on
    // keeps its caret.
    assert_eq!(
        tree.focus_origin(),
        Some(crate::focus::FocusOrigin::Programmatic)
    );
    assert_eq!(tree.focus_visible_signal().get(), visible);
    let root = tree.arena.parent(c).expect("the row's root");
    assert_eq!(tree.tab_stops_within(root), [a, c], "B is no Tab stop now");
    tree.press_key(Key::Tab, Modifiers::SHIFT);
    assert_eq!(
        tree.focused(),
        Some(a),
        "Shift+Tab steps over the disabled one"
    );
}

/// A button whose own handler disables it, during dispatch: it is told it
/// lost focus although disabled, and focus moves on at the next pass.
#[test]
fn a_button_that_disables_itself_hears_focus_lost_and_hands_focus_on() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let enabled = Signal::new(true);
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (on_key_enabled, log) = (enabled.clone(), heard.clone());
    let save = tree.add_child(
        root,
        FillWidget::new()
            .focusable()
            .on_key(move |event, _ctx| match event {
                crate::event::WidgetEvent::KeyDown {
                    key: Key::Enter, ..
                } => {
                    on_key_enabled.set(false);
                    crate::event::EventResponse::Handled
                }
                _ => crate::event::EventResponse::Ignored,
            })
            .on_focus(move |gained, _ctx| log.borrow_mut().push(gained)),
    );
    let cancel = field(&mut tree, root);
    tree.enabled_when(save, enabled.clone());
    lay_out(&mut tree);
    tree.focus(save);
    tree.press_key(Key::Enter, Modifiers::NONE);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(cancel));
    assert_eq!(*heard.borrow(), [true, false], "Save heard it lost focus");
}

/// The hand-off also happens on a full layout pass (here the window was
/// resized in the same pass), not only on an idle one.
#[test]
fn a_disable_in_a_full_layout_pass_hands_focus_on() {
    let Row {
        mut tree,
        ids: [_a, b, c],
        enabled,
    } = row_of_three();
    tree.focus(b);
    enabled[1].set(false);
    tree.layout(SizeProposal::exact(500.0, 320.0));
    assert_eq!(tree.focused(), Some(c));
}

/// A `Continue` traversal scope does not confine the hand-off: from its last
/// control, focus goes on to the stop after it.
#[test]
fn a_continue_scope_lets_the_hand_off_out() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let group = tree.add_child(root, StackWidget::new());
    let _first = field(&mut tree, group);
    let last = field(&mut tree, group);
    let after = field(&mut tree, root);
    tree.set_traversal_scope(group, crate::focus::TraversalScopePolicy::Continue);
    let enabled = Signal::new(true);
    tree.enabled_when(last, enabled.clone());
    lay_out(&mut tree);
    tree.focus(last);
    enabled.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(after));
}

#[test]
fn the_last_control_disabled_hands_focus_to_the_previous() {
    let Row {
        mut tree,
        ids: [_a, b, c],
        enabled,
    } = row_of_three();
    tree.focus(c);
    enabled[2].set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(b));
}

#[test]
fn a_disabled_container_hands_focus_past_itself() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let _before = field(&mut tree, root);
    let group = tree.add_child(root, StackWidget::new());
    let inside = field(&mut tree, group);
    let _also_inside = field(&mut tree, group);
    let after = field(&mut tree, root);
    let enabled = Signal::new(true);
    tree.enabled_when(group, enabled.clone());
    lay_out(&mut tree);
    tree.focus(inside);
    enabled.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(after));
}

#[test]
fn inside_a_modal_a_disabled_control_hands_focus_on_within_it() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let behind = field(&mut tree, root);
    lay_out(&mut tree);
    let content = tree.add(StackWidget::new());
    let first = field(&mut tree, content);
    let last = field(&mut tree, content);
    let enabled = Signal::new(true);
    tree.enabled_when(last, enabled.clone());
    show(
        &mut tree,
        content,
        behind,
        OverlayPlacement::Centered,
        Some(behind),
    );
    tree.focus(last);
    enabled.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(first));
}

/// A popover is not a modal, but focus inside it stays inside it.
#[test]
fn inside_a_popover_a_disabled_control_hands_focus_on_within_it() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let anchor = field(&mut tree, root);
    let _after_anchor = field(&mut tree, root);
    lay_out(&mut tree);
    let content = tree.add(StackWidget::new());
    let first = field(&mut tree, content);
    let last = field(&mut tree, content);
    let enabled = Signal::new(true);
    tree.enabled_when(last, enabled.clone());
    show(
        &mut tree,
        content,
        anchor,
        OverlayPlacement::Below,
        Some(anchor),
    );
    tree.focus(last);
    enabled.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(first));
}

/// A `Cycle` traversal scope is a dialog-like region: focus leaving a
/// disabled control does not leave it.
#[test]
fn a_cycle_scope_keeps_the_handed_on_focus() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let region = tree.add_child(root, StackWidget::new());
    let first = field(&mut tree, region);
    let last = field(&mut tree, region);
    let _outside = field(&mut tree, root);
    tree.set_traversal_scope(region, crate::focus::TraversalScopePolicy::Cycle);
    let enabled = Signal::new(true);
    tree.enabled_when(last, enabled.clone());
    lay_out(&mut tree);
    tree.focus(last);
    enabled.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(first));
}

#[test]
fn a_lone_disabled_control_keeps_focus() {
    let mut tree = WidgetTree::new();
    let root = tree.add(StackWidget::new());
    let only = field(&mut tree, root);
    let enabled = Signal::new(true);
    tree.enabled_when(only, enabled.clone());
    lay_out(&mut tree);
    tree.focus(only);
    enabled.set(false);
    lay_out(&mut tree);
    assert_eq!(tree.focused(), Some(only));
}
