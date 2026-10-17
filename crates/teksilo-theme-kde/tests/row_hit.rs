// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A check box inside a list row is ticked by a press where its box is
//! painted.
//!
//! The Breeze check box paints its 20 px indicator unchanged inside a node
//! the widget raises to the density's target size, and lays its body out
//! 18 px wide so its label lands where Breeze puts it. Either could leave
//! the painted box outside the node that takes the press, or under another
//! part of the row. The press here aims at the centre of the box as it is
//! painted, read from the render frame, not at the centre of a node, in a
//! selected row (whose selection is painted over the row) and an unselected
//! one, at the Compact density and at Touch.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, Point, SizeProposal};
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::TargetDensity;
use teksilo_widgets::StandardListItem;

/// A Breeze list row carrying a checkbox, laid out at `density`.
fn row(density: TargetDensity, checked: &Signal<bool>, selected: bool) -> WidgetTree {
    let mut tree = WidgetTree::new()
        .with_theme(teksilo_theme_kde::light().with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(
        StandardListItem::new(lit!("Row"))
            .checkbox(checked.clone())
            .selected(Signal::new(selected)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree
}

/// The centre of the 15 px box the check box painted.
fn painted_box_centre(tree: &mut WidgetTree) -> Point {
    let frame = tree.render();
    let boxes: Vec<[f32; 4]> = frame
        .shapes
        .iter()
        .map(|s| s.screen)
        .filter(|s| (s[2] - 15.0).abs() < 0.01 && (s[3] - 15.0).abs() < 0.01)
        .collect();
    assert!(!boxes.is_empty(), "the row paints a 15 px box");
    let b = boxes[0];
    Point::new(b[0] + b[2] * 0.5, b[1] + b[3] * 0.5)
}

/// A mouse press at the centre of the painted box ticks it, and a second
/// one clears it.
///
/// Asserts the check box's own signal, never the row's: the row answers a
/// press that never reached the check box, so a row-level assertion is
/// satisfied by the defect.
#[test]
fn a_press_on_the_painted_box_ticks_it_in_a_breeze_row() {
    for selected in [false, true] {
        for density in [TargetDensity::Compact, TargetDensity::Touch] {
            let checked = Signal::new(false);
            let mut tree = row(density, &checked, selected);
            for expected in [true, false] {
                let at = painted_box_centre(&mut tree);
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
                assert_eq!(
                    checked.get(),
                    expected,
                    "a press at the painted box (selected: {selected}) at \
                     {density:?} did not reach the check box",
                );
                tree.layout(SizeProposal::exact(400.0, 300.0));
            }
        }
    }
}

/// The painted box lies inside the check box's own node at both densities,
/// so the press above lands on the check box rather than near it.
#[test]
fn the_painted_box_lies_inside_the_check_box_node() {
    use teksilo_core::accessibility::target_audit::measure_targets;
    for selected in [false, true] {
        for density in [TargetDensity::Compact, TargetDensity::Touch] {
            let checked = Signal::new(false);
            let mut tree = row(density, &checked, selected);
            let node = measure_targets(&tree, density)
                .into_iter()
                .find(|m| m.widget.ends_with("Checkbox") && m.part.is_none())
                .unwrap_or_else(|| panic!("the row builds a check box at {density:?}"));
            let b = tree.bounds(node.node);
            let at = painted_box_centre(&mut tree);
            let inside = |p: Point, half: f32| {
                p.x - half >= b.x
                    && p.x + half <= b.x + b.width
                    && p.y - half >= b.y
                    && p.y + half <= b.y + b.height
            };
            assert!(
                inside(at, 7.5),
                "the box around {at:?} leaves the check box node {b:?} at \
                 {density:?} (selected: {selected})"
            );
        }
    }
}
