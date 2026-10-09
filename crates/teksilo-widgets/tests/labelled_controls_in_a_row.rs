// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A `Checkbox` or `RadioButton` is as wide in an `HStack` as in a `VStack`.
//!
//! Both wrap their row in a 24 dp `MinSize` hit area, and an `HStack`
//! measures its children unbounded on its main axis. `MinSize` used to turn
//! that unbounded width into its 24 dp floor, so the row was measured 24 wide
//! and the single-line label, being shrinkable, collapsed to "…": the
//! LiveImage demo's toolbar showed four checkboxes labelled "…" in a 900 dp
//! window.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, SizeProposal, TextBackend};
use teksilo_core::WidgetId;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::checkbox::Checkbox;
use teksilo_widgets::primitives::{HStack, VStack};
use teksilo_widgets::radio_button::RadioButton;

fn tree() -> WidgetTree {
    let backend: Rc<RefCell<dyn TextBackend>> = Rc::new(RefCell::new(MockTextBackend::new()));
    WidgetTree::new()
        .with_theme(teksilo_core::presets::intui::light())
        .with_text_backend(backend)
}

/// The width of `in_row` (inside an `HStack`) and `in_column` (inside a
/// `VStack`), laid out in a window wide enough for both.
fn widths(tree: &mut WidgetTree, in_row: WidgetId, in_column: WidgetId) -> (f32, f32) {
    let row = tree.add(HStack::new().child(in_row));
    tree.add(VStack::new().child(row).child(in_column));
    tree.layout(SizeProposal::exact(900.0, 600.0));
    (tree.bounds(in_row).width, tree.bounds(in_column).width)
}

#[test]
fn a_checkbox_keeps_its_label_in_a_row() {
    let mut tree = tree();
    let label = || lit!("Pause producer");
    let in_row = tree.add(Checkbox::new(Signal::new(false)).label(label()));
    let in_column = tree.add(Checkbox::new(Signal::new(false)).label(label()));
    let (row, column) = widths(&mut tree, in_row, in_column);
    assert!(
        (row - column).abs() < 0.5,
        "{row:.1} dp in an HStack, {column:.1} dp in a VStack"
    );
}

#[test]
fn a_radio_button_keeps_its_label_in_a_row() {
    let mut tree = tree();
    let selected = Signal::new(0_usize);
    let in_row = tree.add(RadioButton::new(1, selected.clone()).label(lit!("Nearest")));
    let in_column = tree.add(RadioButton::new(2, selected).label(lit!("Nearest")));
    let (row, column) = widths(&mut tree, in_row, in_column);
    assert!(
        (row - column).abs() < 0.5,
        "{row:.1} dp in an HStack, {column:.1} dp in a VStack"
    );
}
