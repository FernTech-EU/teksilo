// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A rich tooltip's dwell promotion: a pointer rest or a hold pins it, a
//! focus rest does not.
//!
//! The sweep reads the real clock, so these tests register a dwell window of
//! a millisecond and wait a few for it to pass.

use std::time::Duration;

use teksilo_canvas::SizeProposal;

use super::WidgetTree;
use crate::event::{Key, Modifiers};
use crate::test_widgets::{FillWidget, StackWidget};
use crate::widget_id::WidgetId;

const DELAY: Duration = Duration::from_millis(100);

/// Its children side by side, each an equal share of the width.
#[derive(Debug)]
struct Row;

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
            child.size = teksilo_canvas::Size::new(width, bounds.height / 2.0);
        }
    }
}

fn lay_out(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(400.0, 300.0));
}

/// Two focusable fields side by side, the first carrying a rich tip with a
/// one-millisecond dwell window, its content holding a focusable link.
fn fields_with_a_rich_tip() -> (WidgetTree, WidgetId, WidgetId, WidgetId) {
    fields_with_a_rich_tip_of(Duration::from_millis(1), None)
}

/// [`fields_with_a_rich_tip`] with a dwell window of `dwell`, and the tip's
/// "shown at" sink when one is given.
fn fields_with_a_rich_tip_of(
    dwell: Duration,
    sink: Option<std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>>,
) -> (WidgetTree, WidgetId, WidgetId, WidgetId) {
    let mut tree = WidgetTree::new();
    let root = tree.add(Row);
    let first = tree.add_child(root, FillWidget::new().focusable());
    let second = tree.add_child(root, FillWidget::new().focusable());
    let tip = tree.add(StackWidget::new());
    tree.add_child(tip, FillWidget::new().focusable());
    tree.set_dormant(tip);
    match sink {
        Some(sink) => tree.attach_tooltip_with_sticky_sink(first, tip, DELAY, Some(dwell), sink),
        None => tree.attach_tooltip_with_sticky(first, tip, DELAY, Some(dwell)),
    }
    lay_out(&mut tree);
    (tree, first, second, tip)
}

/// Show the tip by the delay on the simulated clock, then let the dwell
/// window pass on the real one.
fn rest(tree: &mut WidgetTree) {
    tree.advance_time(DELAY * 2);
    lay_out(tree);
    std::thread::sleep(Duration::from_millis(5));
    lay_out(tree);
    lay_out(tree);
}

fn hover(tree: &mut WidgetTree, id: WidgetId) {
    let at = tree.bounds(id).center();
    tree.pointer_move(at);
    lay_out(tree);
}

#[test]
fn a_focus_rest_shows_the_tip_without_pinning_it() {
    let (mut tree, first, second, tip) = fields_with_a_rich_tip();
    tree.focus(first);
    rest(&mut tree);
    assert!(tree.is_active(tip), "the focus rest showed the tip");
    assert!(!tree.tooltip_is_sticky_within(first), "and did not pin it");
    let window = tree.arena.parent(first).expect("the fields' root");
    assert!(
        !tree
            .tab_stops_within(window)
            .iter()
            .any(|&id| tree.is_descendant_of(id, tip)),
        "its link is no Tab stop"
    );
    tree.press_key(Key::Tab, Modifiers::NONE);
    assert_eq!(tree.focused(), Some(second), "Tab goes past the tip");
}

#[test]
fn a_pointer_rest_pins_the_tip() {
    let (mut tree, first, _second, _tip) = fields_with_a_rich_tip();
    hover(&mut tree, first);
    rest(&mut tree);
    assert!(tree.tooltip_is_sticky_within(first));
}

#[test]
fn a_pointer_rest_after_a_focus_rest_pins_the_tip() {
    let (mut tree, first, _second, _tip) = fields_with_a_rich_tip();
    tree.focus(first);
    rest(&mut tree);
    assert!(!tree.tooltip_is_sticky_within(first));
    hover(&mut tree, first);
    std::thread::sleep(Duration::from_millis(5));
    lay_out(&mut tree);
    assert!(tree.tooltip_is_sticky_within(first));
}

#[test]
fn a_pointer_passing_over_a_focus_shown_tip_does_not_pin_it() {
    // A window long enough that the pass over the anchor never outlasts it,
    // however loaded the machine.
    let dwell = Duration::from_millis(200);
    let (mut tree, first, second, _tip) = fields_with_a_rich_tip_of(dwell, None);
    tree.focus(first);
    rest(&mut tree);
    hover(&mut tree, first);
    hover(&mut tree, second);
    std::thread::sleep(dwell + Duration::from_millis(50));
    lay_out(&mut tree);
    assert!(!tree.tooltip_is_sticky_within(first));
}

/// A tip a focus rest showed shows no dwell countdown (its sink is empty)
/// and wakes the loop for none.
#[test]
fn a_focus_shown_tip_shows_no_countdown_and_wakes_nothing() {
    let sink = std::rc::Rc::new(std::cell::Cell::new(None));
    let (mut tree, first, _second, tip) =
        fields_with_a_rich_tip_of(Duration::from_secs(60), Some(sink.clone()));
    tree.focus(first);
    tree.advance_time(DELAY * 2);
    lay_out(&mut tree);
    assert!(tree.is_active(tip), "shown");
    assert_eq!(sink.get(), None, "no countdown");
    assert_ne!(
        tree.next_timer_deadline_source().map(|(_, source)| source),
        Some("tooltip_dwell")
    );
}

/// A hold still pins the tip it showed, as before: for a touch user it is
/// the way to the tip's links.
#[test]
fn a_hold_pins_the_tip_it_showed() {
    let (mut tree, first, _second, tip) = fields_with_a_rich_tip();
    let at = tree.bounds(first).center();
    tree.long_press_at(teksilo_tokens::PointerKind::Touch, at);
    lay_out(&mut tree);
    rest(&mut tree);
    assert!(tree.is_active(tip), "the hold showed the tip");
    assert!(tree.tooltip_is_sticky_within(first));
}
