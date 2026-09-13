// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A dimension this preset writes must be the dimension the widget renders.
//!
//! The unit tests beside each style check that the *recipe* carries Fluent's
//! number. That is only half the claim: a recipe field nothing reads is a
//! number written and never rendered, and several of them were exactly that.
//! These tests mount the real widget under the real preset and measure the
//! laid-out geometry, so they fail when the widget stops asking the style —
//! which no assertion about a recipe can see.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{MockTextBackend, SizeProposal};
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::ListModel;
use teksilo_i18n::lit;
use teksilo_widgets::TableView;
use teksilo_widgets::table_view::{Column, ColumnWidth};

/// Mount `w` under `theme` in a tree with a real text backend and lay it out.
fn mounted(theme: Theme, w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let id = tree.add(w);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, id)
}

/// Every node under `root` whose `Widget::type_name` ends in `name`, in tree
/// order. Named by type rather than by index so an extra wrapper somewhere in
/// the chrome does not silently move the measurement onto another node.
fn nodes_named(tree: &WidgetTree, root: WidgetId, name: &str) -> Vec<WidgetId> {
    let mut out = Vec::new();
    fn walk(tree: &WidgetTree, id: WidgetId, name: &str, out: &mut Vec<WidgetId>) {
        if tree
            .widget_type_name(id)
            .is_some_and(|t| t.rsplit("::").next() == Some(name))
        {
            out.push(id);
        }
        for c in tree.children(id) {
            walk(tree, c, name, out);
        }
    }
    walk(tree, root, name, &mut out);
    out
}

/// A one-column table's header gutter: the leading inset between the header
/// cell and the row of content inside it.
fn header_gutter(theme: Theme) -> f32 {
    let model = ListModel::from_vec(vec!["alpha".to_string()]);
    let view = TableView::new(model).add_column(
        Column::new("name", lit!("Name"), |s: &String, _| {
            Box::new(teksilo_widgets::primitives::TextWidget::new(lit!(
                s.clone()
            )))
        })
        .width(ColumnWidth::Fixed(200.0)),
    );
    let (tree, root) = mounted(theme, view);

    let cells = nodes_named(&tree, root, "HeaderCell");
    assert_eq!(cells.len(), 1, "one column, one header cell");
    let paddings = nodes_named(&tree, cells[0], "Padding");
    assert_eq!(
        paddings.len(),
        1,
        "the header cell wraps its content row in exactly one Padding",
    );
    let pad = paddings[0];
    let inner = tree.children(pad);
    assert_eq!(inner.len(), 1, "a Padding has one child");
    tree.bounds(inner[0]).x - tree.bounds(pad).x
}

/// Fluent's `ListViewItem` rhythm puts a 12 dp gutter inside every cell. The
/// header used to read the shipped 8 dp constant instead, so the preset's
/// number was written on the recipe and never rendered.
///
/// The IntUI arm is the control: it must still be 8 dp, which is the
/// programme-wide "Compact IntUI does not move" invariant for this dimension.
#[test]
fn a_table_header_cell_pads_by_the_fluent_gutter() {
    assert_eq!(
        header_gutter(teksilo_core::presets::intui::light()),
        8.0,
        "IntUI at Compact must be unchanged",
    );
    assert_eq!(
        header_gutter(teksilo_theme_fluent::light()),
        12.0,
        "the Fluent recipe's `cell_padding_horizontal` must reach the cell",
    );
    assert_eq!(
        header_gutter(teksilo_theme_fluent::dark()),
        12.0,
        "both appearances install the same metrics",
    );
}

/// The accessor half of the same mechanism, checked directly on the installed
/// slot: what a widget asking the theme's `TableStyle` is told.
#[test]
fn the_installed_table_slot_reports_the_fluent_gutter() {
    let theme = teksilo_theme_fluent::light();
    let style = theme
        .style_slots
        .table
        .clone()
        .expect("the preset installs a table slot");
    assert_eq!(style.cell_padding_horizontal(&theme.input), 12.0);
    assert_eq!(
        style.cell_padding_vertical(&theme.input),
        4.0,
        "Fluent takes the shipped vertical gutter",
    );
}
