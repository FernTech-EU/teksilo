// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Focus requests aimed at a dormant node, and `clear_focus`.
//!
//! A handler that shows a hidden branch and asks for focus inside it in the
//! same breath used to see focus land on the dormant node, then lose it on the
//! next layout: `revalidate_interaction_state` ran before the pass woke the
//! branch, found the target inactive, and dropped focus. These tests pin the
//! deferred request that replaced that, and the `clear_focus` door that
//! cancels it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::SizeProposal;

use crate::signal::Signal;
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// Children side by side in equal columns, so a synthesised tap at a child's
/// centre lands on that child and no other.
#[derive(Debug)]
struct Row(Vec<WidgetId>);

impl crate::widget::Widget for Row {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &crate::widget::LayoutContext,
    ) -> crate::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        _ctx: &crate::widget::LayoutContext,
    ) {
        let width = bounds.width / children.len().max(1) as f32;
        for (i, child) in children.iter_mut().enumerate() {
            child.origin = teksilo_canvas::Point::new(bounds.x + width * i as f32, bounds.y);
            child.size = teksilo_canvas::Size::new(width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.0.clone()
    }
}

fn size() -> SizeProposal {
    SizeProposal::exact(400.0, 300.0)
}

/// A tree with an inert `go` node and a `visible_when(shown)` branch holding a
/// focusable `leaf`. `go`'s tap shows the branch and asks for focus on `leaf`
/// — or on whatever `target` holds when it is set.
struct Gate {
    tree: WidgetTree,
    go: WidgetId,
    branch: WidgetId,
    leaf: WidgetId,
    other: WidgetId,
    shown: Signal<bool>,
}

fn gate(show_on_tap: bool) -> Gate {
    let shown = Signal::new(false);
    let target: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
    let mut tree = WidgetTree::new();
    let leaf = tree.add(FillWidget::new().focusable());
    let branch = tree.add(StackWidget::new().child(leaf));
    tree.visible_when(branch, shown.clone());
    let other = tree.add(FillWidget::new().focusable());
    let go = {
        let (shown, target) = (shown.clone(), target.clone());
        tree.add(FillWidget::new().on_tap(move |_, ctx| {
            if show_on_tap {
                shown.set(true);
            }
            if let Some(id) = target.get() {
                ctx.request_focus(id);
            }
        }))
    };
    tree.add(StackWidget::new().child(branch).child(other).child(go));
    tree.layout(size());
    assert!(
        !tree.is_active(leaf),
        "the fixture starts with the branch hidden"
    );
    target.set(Some(leaf));
    Gate {
        tree,
        go,
        branch,
        leaf,
        other,
        shown,
    }
}

/// The request waits for the branch the same handler showed, then lands on
/// exactly the node it named.
#[test]
fn a_request_into_a_branch_the_handler_shows_lands_after_the_layout() {
    let mut g = gate(true);
    g.tree.focus(g.other);
    g.tree.click(g.go);
    assert_eq!(
        g.tree.focused(),
        Some(g.other),
        "a dormant node is never focused outright"
    );
    g.tree.layout(size());
    assert!(g.tree.is_active(g.leaf));
    assert_eq!(g.tree.focused(), Some(g.leaf));
}

/// A branch that stays hidden drops the request at the end of the pass rather
/// than keeping it forever: showing the branch later moves nothing.
#[test]
fn a_request_whose_target_stays_dormant_is_dropped() {
    let mut g = gate(false);
    g.tree.focus(g.other);
    g.tree.click(g.go);
    assert!(g.tree.has_pending_focus_request());
    g.tree.layout(size());
    assert!(!g.tree.has_pending_focus_request(), "dropped by the pass");
    assert_eq!(g.tree.focused(), Some(g.other));
    g.shown.set(true);
    g.tree.layout(size());
    assert!(g.tree.is_active(g.leaf));
    assert_eq!(
        g.tree.focused(),
        Some(g.other),
        "a stale request does not come back when its target finally wakes"
    );
}

/// A focus move made after the request supersedes it.
#[test]
fn a_later_focus_move_cancels_the_waiting_request() {
    let mut g = gate(true);
    g.tree.click(g.go);
    assert!(g.tree.has_pending_focus_request());
    g.tree.focus(g.other);
    g.tree.layout(size());
    assert!(g.tree.is_active(g.leaf));
    assert_eq!(g.tree.focused(), Some(g.other));
}

/// A focus move that is not a request (a pointer or a Tab, through the one
/// funnel every move takes) supersedes it as well.
#[test]
fn a_pointer_or_keyboard_focus_cancels_the_waiting_request() {
    for origin in [
        crate::focus::FocusOrigin::POINTER,
        crate::focus::FocusOrigin::Keyboard,
    ] {
        let mut g = gate(true);
        g.tree.click(g.go);
        assert!(g.tree.has_pending_focus_request());
        g.tree.focus_with_origin(g.other, origin);
        g.tree.layout(size());
        assert!(g.tree.is_active(g.leaf));
        assert_eq!(g.tree.focused(), Some(g.other), "{origin:?}");
    }
}

/// A request naming the node that already has focus is a request too.
#[test]
fn a_later_request_for_the_focused_node_still_cancels() {
    let mut g = gate(true);
    g.tree.focus(g.other);
    g.tree.click(g.go);
    g.tree.focus(g.other);
    g.tree.layout(size());
    assert_eq!(g.tree.focused(), Some(g.other));
}

/// `WidgetTree::focus` defers the same way the handler request does.
#[test]
fn tree_focus_on_a_dormant_node_waits_for_it() {
    let mut g = gate(false);
    g.tree.focus(g.other);
    g.tree.focus(g.leaf);
    assert_eq!(g.tree.focused(), Some(g.other));
    g.shown.set(true);
    g.tree.layout(size());
    assert_eq!(g.tree.focused(), Some(g.leaf));
}

/// A target destroyed while its request waits takes the request with it.
#[test]
fn a_destroyed_target_drops_the_waiting_request() {
    let mut g = gate(true);
    g.tree.focus(g.other);
    g.tree.click(g.go);
    g.tree.destroy_subtree(g.branch);
    g.tree.layout(size());
    assert_eq!(g.tree.focused(), Some(g.other));
    assert!(!g.tree.has_pending_focus_request());
}

/// `clear_focus` cancels a request still waiting for its target.
#[test]
fn clear_focus_cancels_the_waiting_request() {
    let mut g = gate(true);
    g.tree.focus(g.other);
    g.tree.click(g.go);
    g.tree.clear_focus();
    g.tree.layout(size());
    assert!(g.tree.is_active(g.leaf));
    assert_eq!(g.tree.focused(), None);
}

/// `clear_focus` is a focus move to nothing: `FocusLost`, `focused() == None`,
/// the ancestors' `focus_within` back to `false`, the ring hidden.
#[test]
fn clear_focus_is_a_move_to_nothing() {
    let within = Signal::new(false);
    let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new();
    let leaf = {
        let seen = seen.clone();
        tree.add(
            FillWidget::new()
                .focusable()
                .on_focus(move |gained, _| seen.borrow_mut().push(gained)),
        )
    };
    tree.add(StackWidget::new().child(leaf).focus_within(within.clone()));
    tree.layout(size());

    tree.focus_with_origin(leaf, crate::focus::FocusOrigin::Keyboard);
    assert!(within.get());
    assert!(
        tree.focus_visible_signal().get(),
        "a keyboard focus shows the ring"
    );

    tree.a11y_dirty = false;
    tree.clear_focus();
    assert_eq!(tree.focused(), None);
    assert_eq!(*seen.borrow(), vec![true, false], "FocusLost was delivered");
    assert!(!within.get(), "focus_within follows focus out");
    assert!(!tree.focus_visible_signal().get(), "the ring goes with it");
    assert!(tree.a11y_dirty, "the accessibility tree is told");

    // Nothing focused: a second clear is a no-op and delivers nothing.
    tree.clear_focus();
    assert_eq!(seen.borrow().len(), 2);
}

/// From a handler, `ctx.clear_focus()` — the opt-in for "a click on empty
/// space drops focus".
#[test]
fn ctx_clear_focus_from_a_background_tap() {
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    let background = tree.add(FillWidget::new().on_tap(|_, ctx| ctx.clear_focus()));
    tree.add(Row(vec![background, field]));
    tree.layout(size());
    tree.focus(field);
    tree.click(background);
    assert_eq!(tree.focused(), None);
}

/// The calls apply in the order they were made: a request queued after
/// `clear_focus` in the same handler still lands, one queued before it does not.
#[test]
fn ctx_clear_focus_orders_with_the_requests_around_it() {
    let mut tree = WidgetTree::new();
    let a = tree.add(FillWidget::new().focusable());
    let b = tree.add(FillWidget::new().focusable());
    let clear_then_request = tree.add(FillWidget::new().on_tap(move |_, ctx| {
        ctx.clear_focus();
        ctx.request_focus(b);
    }));
    let request_then_clear = tree.add(FillWidget::new().on_tap(move |_, ctx| {
        ctx.request_focus(b);
        ctx.clear_focus();
    }));
    tree.add(Row(vec![a, b, clear_then_request, request_then_clear]));
    tree.layout(size());

    tree.focus(a);
    tree.click(clear_then_request);
    assert_eq!(tree.focused(), Some(b));

    tree.focus(a);
    tree.click(request_then_clear);
    assert_eq!(tree.focused(), None);
}

/// A `FocusLost` handler that moves focus elsewhere has the last word.
#[test]
fn a_focus_lost_handler_can_redirect_a_clear() {
    let mut tree = WidgetTree::new();
    let fallback = tree.add(FillWidget::new().focusable());
    let leaf = tree.add(FillWidget::new().focusable().on_focus(move |gained, ctx| {
        if !gained {
            ctx.request_focus(fallback);
        }
    }));
    tree.add(StackWidget::new().child(leaf).child(fallback));
    tree.layout(size());
    tree.focus(leaf);
    tree.clear_focus();
    assert_eq!(tree.focused(), Some(fallback));
}
