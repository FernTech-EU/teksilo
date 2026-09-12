// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent row's selection pill paints over the row and must not take its
//! presses.
//!
//! Windows 11 marks a selected row with an accent bar on its leading edge, and
//! this preset paints it by stacking a pill **over** the delegated row — which
//! is right about painting and is exactly what makes it the hit owner, because
//! the hit walk is the reverse of paint order. A pill spanning the whole row
//! therefore owns every point in it unless it declares itself transparent to
//! the walk.
//!
//! The two tests below are a pair on purpose. One says the controls inside a
//! row are reachable; the other says the pill is still painted. Either alone
//! can be satisfied by breaking the other.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, SizeProposal};
use teksilo_core::accessibility::target_audit::measure_targets;
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_tokens::TargetDensity;
use teksilo_widgets::StandardListItem;

/// A Fluent list row carrying a checkbox, laid out at `density`.
fn row(theme: Theme, density: TargetDensity, checked: &Signal<bool>, selected: bool) -> WidgetTree {
    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    tree.add(
        StandardListItem::new(lit!("Row"))
            .checkbox(checked.clone())
            .selected(Signal::new(selected)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree
}

/// A mouse can tick a checkbox inside a Fluent list row.
///
/// Asserts the **checkbox's own** signal, never the row's tap count: the row
/// answers a press that never reached the checkbox, so a row-level assertion is
/// satisfied by the defect.
#[test]
fn a_mouse_can_tick_a_checkbox_in_a_fluent_list_row() {
    // Selected as well as not: the pill is *painted* only on a selected row,
    // and selected is therefore the state the accent bar exists for — a
    // refactor that mounts the pill only when selected (or adds a second
    // selection-only overlay without `hit_transparent`) regresses exactly the
    // rows an unselected-only test never presses.
    for selected in [false, true] {
        for density in [TargetDensity::Compact, TargetDensity::Touch] {
            let checked = Signal::new(false);
            let mut tree = row(teksilo_theme_fluent::light(), density, &checked, selected);

            let box_node = measure_targets(&tree, density)
                .into_iter()
                .find(|m| m.widget.ends_with("Checkbox") && m.part.is_none())
                .unwrap_or_else(|| panic!("the row builds a checkbox at {density:?}"));
            let at = tree.bounds(box_node.node).center();

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

            assert!(
                checked.get(),
                "a mouse press at the centre of the checkbox in a Fluent row \
                 (selected: {selected}) at {density:?} did not tick it: the \
                 press was taken by something painted over the row",
            );
        }
    }
}

/// And the pill is still painted, so "transparent to the hit walk" has not been
/// implemented by deleting the accent bar Fluent is recognisable by.
///
/// The assertion is on the bar's own geometry rather than on a draw count: a
/// selected row also paints a neutral wash, so a count moves for either reason.
#[test]
fn a_selected_fluent_row_still_paints_its_pill() {
    let width = 3.0;
    let height = 16.0;
    let bars = |selected: bool| {
        let checked = Signal::new(false);
        let mut tree = row(
            teksilo_theme_fluent::light(),
            TargetDensity::Compact,
            &checked,
            selected,
        );
        let frame = tree.render();
        frame
            .shapes
            .iter()
            .filter(|s| (s.screen[2] - width).abs() < 0.01 && (s.screen[3] - height).abs() < 0.01)
            .count()
    };
    assert_eq!(
        bars(true),
        1,
        "a selected Fluent row paints exactly one accent bar",
    );
    assert_eq!(bars(false), 0, "and an unselected one paints none");
}
