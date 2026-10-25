// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `focus_on_click(false)`: a control a click activates without moving the
//! keyboard focus to it, which stays reachable from the keyboard.
//!
//! A mouse focuses on the press and a finger on the release; both are
//! covered, since the flag has to hold on each of the two paths. The flag is
//! read on the focusable widget a click would focus, and on no other.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_tokens::PointerKind;

use crate::event::{Key, Modifiers};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_builder::WidgetBuilder;
use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

/// Children side by side in equal columns, so a tap at a child's centre lands
/// on that child and no other.
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

/// A focusable `field`, a `button` that counts its taps and takes focus on a
/// click or not as `focus_on_click` says, and a plain focusable `other`.
struct Fixture {
    tree: WidgetTree,
    row: WidgetId,
    field: WidgetId,
    button: WidgetId,
    other: WidgetId,
    taps: Rc<Cell<u32>>,
}

fn fixture(focus_on_click: bool) -> Fixture {
    let taps = Rc::new(Cell::new(0));
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    let button = {
        let taps = taps.clone();
        tree.add(
            FillWidget::new()
                .focusable()
                .on_tap(move |_, _| taps.set(taps.get() + 1))
                .focus_on_click(focus_on_click),
        )
    };
    let other = tree.add(FillWidget::new().focusable());
    let row = tree.add(Row(vec![field, button, other]));
    tree.layout(SizeProposal::exact(300.0, 100.0));
    Fixture {
        tree,
        row,
        field,
        button,
        other,
        taps,
    }
}

impl Fixture {
    fn centre(&self, id: WidgetId) -> teksilo_canvas::Point {
        let b = self.tree.bounds(id);
        teksilo_canvas::Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0)
    }
}

#[test]
fn a_mouse_click_activates_without_taking_focus() {
    let mut f = fixture(false);
    f.tree.focus(f.field);
    let at = f.centre(f.button);
    f.tree.tap_with(PointerKind::Mouse, at);
    assert_eq!(f.taps.get(), 1, "the click activates the button");
    assert_eq!(f.tree.focused(), Some(f.field), "focus stays in the field");
}

#[test]
fn a_tap_activates_without_taking_focus() {
    let mut f = fixture(false);
    f.tree.focus(f.field);
    let at = f.centre(f.button);
    f.tree.tap_with(PointerKind::Touch, at);
    assert_eq!(f.taps.get(), 1, "the tap activates the button");
    assert_eq!(f.tree.focused(), Some(f.field), "focus stays in the field");
    f.tree.assert_no_leaked_pointer_state();
}

/// The control, so the two tests above cannot pass because a click on the
/// fixture's button never focuses anything.
#[test]
fn by_default_a_click_and_a_tap_focus_the_button() {
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        let mut f = fixture(true);
        f.tree.focus(f.field);
        let at = f.centre(f.button);
        f.tree.tap_with(kind, at);
        assert_eq!(f.tree.focused(), Some(f.button), "{kind:?}");
    }
}

/// The flag is about the pointer only: the button is still a Tab stop, Tab
/// reaches it, and programmatic focus lands on it.
#[test]
fn it_stays_reachable_from_the_keyboard() {
    let mut f = fixture(false);
    assert_eq!(
        f.tree.tab_stops_within(f.row),
        vec![f.field, f.button, f.other]
    );
    f.tree.focus(f.field);
    f.tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(f.tree.focused(), Some(f.button), "Tab lands on it");
    f.tree.focus(f.other);
    f.tree.focus(f.button);
    assert_eq!(f.tree.focused(), Some(f.button), "and so does a request");
}

/// A click on a widget inside the control leaves focus alone too, and the
/// walk does not go on to focus a focusable ancestor instead.
#[test]
fn a_click_inside_it_does_not_focus_an_ancestor_instead() {
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    let label = tree.add(FillWidget::new());
    let button = tree.add(
        StackWidget::new()
            .child(label)
            .focus_on_click(false)
            .focusable(true),
    );
    let outer = tree.add(StackWidget::new().child(button).focusable(true));
    tree.add(Row(vec![field, outer]));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    tree.focus(field);
    let b = tree.bounds(label);
    let at = teksilo_canvas::Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        tree.tap_with(kind, at);
        assert_eq!(tree.focused(), Some(field), "{kind:?}");
    }
}

/// The flag belongs to the node it is set on, as GTK's `focus-on-click`
/// does: a focusable widget inside a container built with it (a text field
/// in a toolbar) still takes focus from a click. A composite whose focusable
/// node is a child of its own forwards the flag to that child itself.
#[test]
fn a_field_inside_a_container_built_with_it_still_takes_focus() {
    let mut tree = WidgetTree::new();
    let field = tree.add(FillWidget::new().focusable());
    let inner = tree.add(FillWidget::new().focusable());
    let container = tree.add(StackWidget::new().child(inner).focus_on_click(false));
    tree.add(Row(vec![field, container]));
    tree.layout(SizeProposal::exact(200.0, 100.0));
    let b = tree.bounds(inner);
    let at = teksilo_canvas::Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        tree.focus(field);
        tree.tap_with(kind, at);
        assert_eq!(tree.focused(), Some(inner), "{kind:?}");
    }
}

/// A click on a control that takes no focus blurs nothing, so before the
/// control hears it the focused field commits what it holds
/// (`on_commit_edit`), as a spin box commits on blur: the command the click
/// runs sees the value typed. A control that takes focus blurs the field
/// instead, and the hook is not called.
#[test]
fn a_click_that_takes_no_focus_has_the_focused_field_commit_first() {
    use crate::widget_builder::HandlerSet;
    for (focus_on_click, kind, expected) in [
        (false, PointerKind::Mouse, &["commit", "tap"][..]),
        (false, PointerKind::Touch, &["commit", "tap"][..]),
        (true, PointerKind::Mouse, &["tap"][..]),
    ] {
        let log = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut tree = WidgetTree::new();
        let field = tree.add(FillWidget::new().focusable());
        {
            let log = log.clone();
            tree.apply_external_handler_set(
                field,
                HandlerSet::new().on_commit_edit(move |_| log.borrow_mut().push("commit")),
            );
        }
        let button = {
            let log = log.clone();
            tree.add(
                FillWidget::new()
                    .focusable()
                    .on_tap(move |_, _| log.borrow_mut().push("tap"))
                    .focus_on_click(focus_on_click),
            )
        };
        tree.add(Row(vec![field, button]));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(field);
        let b = tree.bounds(button);
        let at = teksilo_canvas::Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
        tree.tap_with(kind, at);
        assert_eq!(*log.borrow(), expected, "{focus_on_click} {kind:?}");
    }
}
