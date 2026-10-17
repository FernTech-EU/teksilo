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
use teksilo_tokens::TargetDensity;
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

// ── StandardListItem row height ────────────────────────────────────────

/// The laid-out `(single-line, two-line)` heights of a `StandardListItem` row
/// in a plain column, built at `density` under `theme`.
///
/// Built *at* the density, never switched into it: a row resolves its style
/// in `build()`, so switching after the fact would measure a mixture.
fn standard_row_heights(theme: &Theme, density: TargetDensity) -> (f32, f32) {
    use teksilo_widgets::StandardListItem;
    use teksilo_widgets::primitives::VStack;

    let (tree, root) = mounted(
        theme.with_density(density),
        VStack::new()
            .child(StandardListItem::new(lit!("Documents")))
            .child(StandardListItem::new(lit!("Downloads")).subtitle(lit!("12 items"))),
    );
    let rows = nodes_named(&tree, root, "StandardListItem");
    assert_eq!(rows.len(), 2, "one single-line row, one two-line row");
    (tree.bounds(rows[0]).height, tree.bounds(rows[1]).height)
}

/// `ListViewItemMinHeight` is 40 dp, which is what the Fluent recipe writes,
/// and a laid-out row must measure it. The row used to floor itself on the
/// shipped 28 dp constant whatever the style declared.
///
/// The Fluent heights already clear the Comfortable target, so only Touch
/// raises the single-line row (to 44); the 60 dp two-line row clears every
/// rung. The IntUI arm is the control and must be unchanged; its two-line row
/// at Touch is content-driven (44.4 under the mock text metrics), because a
/// floor only ever raises a row.
#[test]
fn a_standard_list_row_measures_the_fluent_row_height_at_every_density() {
    let expected = [
        (TargetDensity::Compact, (40.0, 60.0), (28.0, 44.0)),
        (TargetDensity::Comfortable, (40.0, 60.0), (32.0, 44.0)),
        (TargetDensity::Touch, (44.0, 60.0), (44.0, 44.4)),
    ];
    for (density, fluent, intui) in expected {
        assert_eq!(
            standard_row_heights(&teksilo_core::presets::intui::light(), density),
            intui,
            "{density:?}: IntUI must be unchanged",
        );
        for theme in [teksilo_theme_fluent::light(), teksilo_theme_fluent::dark()] {
            assert_eq!(
                standard_row_heights(&theme, density),
                fluent,
                "{density:?}: the Fluent recipe's row heights must reach the row",
            );
        }
        // The accessor half: what the installed slot tells a row asking it.
        let theme = teksilo_theme_fluent::light().with_density(density);
        let slot = theme
            .style_slots
            .standard_item
            .clone()
            .expect("the preset installs a standard-item slot");
        assert_eq!(
            (
                slot.min_height_single_line(&theme.input),
                slot.min_height_two_line(&theme.input),
            ),
            fluent,
            "{density:?}: the installed slot reports the Fluent row heights",
        );
    }
}

// ── TableView / TreeTableView row and header heights ───────────────────

/// The laid-out `(header height, row height)` of a `TableView` and of a
/// `TreeTableView` over the same three rows, built at `density` under `theme`
/// with no row or header height of their own. Each pair is measured on the
/// hosted `TableHeader` and on the first realized `BodyRow`, and the first
/// row must start where the header ends, so the band the virtualization
/// arithmetic steps by is the one painted.
///
/// Built *at* the density, never switched into it: a view reads its style's
/// heights in `build()`.
fn table_bands(theme: &Theme, density: TargetDensity) -> [(f32, f32); 2] {
    use teksilo_data::TreeModel;
    use teksilo_widgets::TreeTableView;

    fn bands(tree: &WidgetTree, root: WidgetId) -> (f32, f32) {
        let header = nodes_named(tree, root, "TableHeader");
        assert_eq!(header.len(), 1, "one hosted header");
        let mut rows = nodes_named(tree, root, "BodyRow");
        rows.sort_by(|a, b| tree.bounds(*a).y.total_cmp(&tree.bounds(*b).y));
        assert!(rows.len() >= 2, "at least two realized rows");
        let h = tree.bounds(header[0]);
        let first = tree.bounds(rows[0]);
        assert_eq!(
            first.y,
            h.y + h.height,
            "the first row starts under the header"
        );
        assert_eq!(
            tree.bounds(rows[1]).y - first.y,
            first.height,
            "the row pitch is the row height",
        );
        (h.height, first.height)
    }
    fn column<T: 'static>(label: fn(&T) -> String) -> Column<T> {
        Column::new("name", lit!("Name"), move |row: &T, _| {
            Box::new(teksilo_widgets::primitives::TextWidget::new(lit!(label(
                row
            ))))
        })
        .width(ColumnWidth::Fixed(200.0))
    }
    let theme = theme.with_density(density);

    let model = ListModel::from_vec(vec!["alpha".to_string(), "beta".into(), "gamma".into()]);
    let (tree, root) = mounted(
        theme.clone(),
        TableView::new(model).add_column(column(|s: &String| s.clone())),
    );
    let flat = bands(&tree, root);

    let model: TreeModel<String> = TreeModel::new();
    for (i, name) in ["alpha", "beta", "gamma"].into_iter().enumerate() {
        model.insert_root(i, name.to_string());
    }
    let (tree, root) = mounted(
        theme,
        TreeTableView::new(model).add_column(column(|s: &String| s.clone())),
    );
    [flat, bands(&tree, root)]
}

/// Fluent's `ListViewItem` rhythm is 40 dp, which is what the Fluent recipe
/// writes for a table row and its header, and a laid-out `TableView` and
/// `TreeTableView` must measure it. Both views used to read the shipped 28 dp
/// row and 32 dp header constants whatever the style declared.
///
/// Above Compact the 40 rises to the density's target size (44 at Touch), as
/// the list row does. The IntUI arm is the control: 32 / 28 at Compact, which
/// is what the views laid out before the style was consulted, then the
/// recipe's own ladder.
#[test]
fn a_table_measures_the_fluent_row_and_header_heights_at_every_density() {
    let expected = [
        (TargetDensity::Compact, (40.0, 40.0), (32.0, 28.0)),
        (TargetDensity::Comfortable, (40.0, 40.0), (32.0, 32.0)),
        (TargetDensity::Touch, (44.0, 44.0), (44.0, 44.0)),
    ];
    for (density, fluent, intui) in expected {
        assert_eq!(
            table_bands(&teksilo_core::presets::intui::light(), density),
            [intui; 2],
            "{density:?}: IntUI follows its recipe",
        );
        for theme in [teksilo_theme_fluent::light(), teksilo_theme_fluent::dark()] {
            assert_eq!(
                table_bands(&theme, density),
                [fluent; 2],
                "{density:?}: the Fluent recipe's heights must reach both views",
            );
        }
        // The accessor half: what the installed slot tells a view asking it.
        let theme = teksilo_theme_fluent::light().with_density(density);
        let slot = theme
            .style_slots
            .table
            .clone()
            .expect("the preset installs a table slot");
        assert_eq!(
            (
                slot.header_height(&theme.input),
                slot.row_height(&theme.input)
            ),
            fluent,
            "{density:?}: the installed slot reports the Fluent heights",
        );
    }
}

// ── TabBar tab height and header padding ───────────────────────────────

/// The laid-out `(strip height, tab height, header padding)` of a horizontal
/// two-tab `TabBar` built at `density` under `theme`, with no tab height of
/// its own. The padding is the inset of each header's content row inside the
/// `Padding` that wraps it, and it must be the same on both edges and on both
/// headers, so the number measured is the one the header's natural width was
/// sized from.
///
/// Built *at* the density, never switched into it: a bar reads its style's
/// metrics when it builds.
fn tab_metrics(theme: &Theme, density: TargetDensity) -> (f32, f32, f32) {
    use teksilo_core::signal::Signal;
    use teksilo_data::ListModel;
    use teksilo_i18n::lit;
    use teksilo_widgets::tab_widget::{TabBar, TabDelegate, TabId, TabSizing};

    let model = ListModel::from_vec(vec![
        (TabId::fresh(), "Documents"),
        (TabId::fresh(), "Downloads"),
    ]);
    let delegate = TabDelegate::new(|_, t: &(TabId, &'static str)| lit!(t.1));
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let bar = tree.add(
        TabBar::horizontal(model, delegate, selected, |_, t| t.0)
            .tab_sizing(TabSizing::Independent)
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal {
        width: Some(800.0),
        height: None,
    });

    let headers = nodes_named(&tree, bar, "TabHeader");
    assert_eq!(headers.len(), 2, "two tab headers");
    let tab_h = tree.bounds(headers[0]).height;
    let mut pads = Vec::new();
    for header in headers {
        assert_eq!(tree.bounds(header).height, tab_h, "one tab height");
        let padding = nodes_named(&tree, header, "Padding");
        assert_eq!(padding.len(), 1, "one padded content row per header");
        let outer = tree.bounds(padding[0]);
        let row = tree.bounds(tree.children(padding[0])[0]);
        let leading = row.x - outer.x;
        let trailing = (outer.x + outer.width) - (row.x + row.width);
        assert!(
            (leading - trailing).abs() < 1e-3,
            "{density:?}: the padding is symmetric ({leading} / {trailing})",
        );
        pads.push(leading);
    }
    assert!((pads[0] - pads[1]).abs() < 1e-3, "one padding: {pads:?}");
    (tree.bounds(bar).height, tab_h, pads[0])
}

/// Assert `(strip, tab, padding)` measures `want` within float tolerance.
fn assert_tab_metrics(got: (f32, f32, f32), want: (f32, f32, f32), what: &str) {
    assert!(
        (got.0 - want.0).abs() < 1e-3 && (got.1 - want.1).abs() < 1e-3,
        "{what}: strip and tab heights {got:?}, want {want:?}",
    );
    assert!(
        (got.2 - want.2).abs() < 1e-3,
        "{what}: header padding {got:?}, want {want:?}",
    );
}

/// `TabViewItemMinHeight` is 32 dp and `TabViewItemHeaderPadding` 8 dp in the
/// Fluent recipe, and a laid-out bar must measure both. The bar used to
/// reserve the shipped 50 dp strip and pad by the shipped 12 dp whatever the
/// style declared, so every Fluent tab strip rendered 18 dp taller and each
/// tab 8 dp wider than its recipe.
///
/// Above Compact the 32 rises to the density's target size (44 at Touch); the
/// padding is WinUI's fixed 8 dp at every density. The IntUI arm is the
/// control: its 50 dp tab clears every target, and its padding walks the
/// spacing ladder.
#[test]
fn a_tab_bar_measures_the_fluent_tab_height_and_padding_at_every_density() {
    let expected = [
        (
            TargetDensity::Compact,
            (32.0, 32.0, 8.0),
            (50.0, 50.0, 12.0),
        ),
        (
            TargetDensity::Comfortable,
            (32.0, 32.0, 8.0),
            (50.0, 50.0, 13.8),
        ),
        (TargetDensity::Touch, (44.0, 44.0, 8.0), (50.0, 50.0, 15.6)),
    ];
    for (density, fluent, intui) in expected {
        assert_tab_metrics(
            tab_metrics(&teksilo_core::presets::intui::light(), density),
            intui,
            &format!("{density:?}: IntUI"),
        );
        for theme in [teksilo_theme_fluent::light(), teksilo_theme_fluent::dark()] {
            assert_tab_metrics(
                tab_metrics(&theme, density),
                fluent,
                &format!("{density:?}: the Fluent recipe's tab metrics must reach the bar"),
            );
        }
        // The accessor half: what the installed slot tells a bar asking it.
        let theme = teksilo_theme_fluent::light().with_density(density);
        let slot = theme
            .style_slots
            .tab
            .clone()
            .expect("the preset installs a tab slot");
        assert!((slot.editor_height(&theme.input) - fluent.1).abs() < 1e-3);
        assert!((slot.padding_horizontal(&theme.input) - fluent.2).abs() < 1e-3);
    }
}

// ── ScrollBar minimum thumb length ─────────────────────────────────────

/// The laid-out thumb length of a vertical `ScrollBar` over a 400 dp track
/// built at `density` under `theme`, with no floor of its own. The content is
/// fifty viewports long (viewport ratio 0.02), so the unfloored thumb would be
/// 8 dp and the length measured is the floor alone. Read from the mounted bar's
/// reported thumb, which is the rectangle its hit test and its drag use.
///
/// Built *at* the density, never switched into it: a bar reads its style's
/// floor when it builds.
fn scroll_thumb_length(theme: &Theme, density: TargetDensity) -> f32 {
    use teksilo_core::signal::Signal;
    use teksilo_widgets::scroll_bar::{SCROLLBAR_PART_THUMB, ScrollBar, ScrollBarOrientation};

    let mut tree = WidgetTree::new()
        .with_theme(theme.with_density(density))
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let bar = tree.add(
        ScrollBar::new(
            ScrollBarOrientation::Vertical,
            Signal::new(0.0),
            Signal::new(500.0),
            Signal::new(0.02),
        )
        .thickness(12.0),
    );
    tree.layout(SizeProposal::exact(12.0, 400.0));
    tree.widget_target_regions(bar)
        .into_iter()
        .find(|r| r.part == SCROLLBAR_PART_THUMB)
        .expect("the bar reports its thumb")
        .rect
        .height
}

/// WinUI's `ScrollBarHorizontalThumbMinWidth` is 30 dp in the Fluent recipe,
/// and a laid-out bar must measure it. The bar used to recompute the shipped
/// 24 dp floor whatever the style declared, so every Fluent thumb on a long
/// document rendered 6 dp shorter than its recipe.
///
/// Above Compact the 30 rises to the density's target size (32 at
/// Comfortable, 44 at Touch). The IntUI arm is the control: its 24 dp floor
/// walks the same ladder.
#[test]
fn a_scroll_bar_floors_its_thumb_at_the_fluent_minimum_at_every_density() {
    let expected = [
        (TargetDensity::Compact, 30.0, 24.0),
        (TargetDensity::Comfortable, 32.0, 32.0),
        (TargetDensity::Touch, 44.0, 44.0),
    ];
    for (density, preset, intui) in expected {
        let got = scroll_thumb_length(&teksilo_core::presets::intui::light(), density);
        assert!(
            (got - intui).abs() < 1e-3,
            "{density:?}: IntUI thumb {got}, want {intui}",
        );
        for theme in [teksilo_theme_fluent::light(), teksilo_theme_fluent::dark()] {
            let got = scroll_thumb_length(&theme, density);
            assert!(
                (got - preset).abs() < 1e-3,
                "{density:?}: the Fluent recipe's thumb floor must reach the bar: \
                 {got}, want {preset}",
            );
        }
        // The accessor half: what the installed slot tells a bar asking it.
        let theme = teksilo_theme_fluent::light().with_density(density);
        let slot = theme
            .style_slots
            .scroll_bar
            .clone()
            .expect("the preset installs a scroll bar slot");
        assert!((slot.min_thumb_length(&theme.input) - preset).abs() < 1e-3);
    }
}

// ── Menu rows ──────────────────────────────────────────────────────────

// The tappable row around the menu, which denies the miss-only slop pass so
// the conformance verdict below is the row's own, is
// `teksilo_target_conformance::in_tappable_row`, imported rather than copied.
use teksilo_target_conformance::in_tappable_row;

/// A `MenuList` at `density` under `theme`, in a tappable row: the laid-out
/// height of each of its four `MenuItem` rows, of its separator, and the
/// `(width, right edge minus the row's right edge)` of the submenu row's
/// chevron column, the trailing column `MenuItem` builds from the style's
/// metrics. Also the number of conformance failures the gate reports for
/// the row.
///
/// Built *at* the density, never switched into it: a row reads its style in
/// `build()`.
fn menu_geometry(theme: &Theme, density: TargetDensity) -> (Vec<f32>, f32, (f32, f32), usize) {
    use teksilo_core::accessibility::target_audit::target_audit;
    use teksilo_core::signal::Signal;
    use teksilo_widgets::primitives::FixedSize;
    use teksilo_widgets::{MenuItem, MenuList};

    let theme = theme.with_density(density);
    let icon_column = theme
        .style_slots
        .menu_item
        .as_ref()
        .expect("the preset installs a menu-item slot")
        .metrics()
        .icon_column_width;
    let mut tree = WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
    let menu = MenuList::new()
        .item(MenuItem::new(lit!("Open")))
        .item(MenuItem::new(lit!("Save")).shortcut_label("Ctrl+S"))
        .separator()
        .item(MenuItem::new(lit!("Checkable")).checked(Signal::new(true)))
        .item(MenuItem::submenu(lit!("Submenu"), || {
            Box::new(MenuList::new().item(MenuItem::new(lit!("Inner"))))
        }));
    let root = in_tappable_row(&mut tree, FixedSize::new().width(240.0).child(menu));
    tree.layout(SizeProposal::exact(600.0, 400.0));

    let rows = nodes_named(&tree, root, "MenuItem");
    assert_eq!(rows.len(), 4, "four rows");
    let heights = rows.iter().map(|r| tree.bounds(*r).height).collect();
    let separators = nodes_named(&tree, root, "MenuSeparator");
    assert_eq!(separators.len(), 1, "one separator");
    let separator = tree.bounds(separators[0]).height;

    let submenu = tree.bounds(rows[3]);
    let chevron = nodes_named(&tree, rows[3], "FixedSize")
        .into_iter()
        .map(|id| tree.bounds(id))
        .filter(|b| (b.height - icon_column).abs() < 1e-4)
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .expect("the submenu row has a chevron column");
    let failures = target_audit(&tree, density)
        .into_iter()
        .filter(|v| v.rule.is_conformance_failure())
        .count();
    (
        heights,
        separator,
        (chevron.width, chevron.right() - submenu.right()),
        failures,
    )
}

/// The line `MockTextBackend` measures for any text, whatever the theme's
/// line height asks.
const MOCK_LINE: f32 = 16.0;

/// A Fluent menu row is WinUI's 37 dp at Compact and Comfortable, whose
/// 32 dp target it already clears, and the 44 dp Touch target at Touch; the
/// separator, 3 dp, and the trailing column, 11 dp, are scaled by the
/// density's spacing factor (1.15 at Comfortable, 1.3 at Touch). No row
/// fails the conformance gate. The menu style used to report its Compact
/// metrics at every density, so a Touch separator and chevron column kept
/// their Compact sizes inside rows laid out for Touch, and `MenuList` capped
/// its height by the 37 dp row.
///
/// The laid-out row is the item height less the difference between the
/// Body line box the shipped row arithmetic pads around (20 dp) and the
/// line the mock backend measures ([`MOCK_LINE`]): the recipe derives its
/// vertical padding from the nominal line box, a finding the conformance
/// gate records against the menu row, not something this preset decides.
#[test]
fn a_menu_measures_the_fluent_rows_at_every_density() {
    let expected = [
        (TargetDensity::Compact, 37.0, 3.0, 11.0),
        (TargetDensity::Comfortable, 37.0, 3.45, 12.65),
        (TargetDensity::Touch, 44.0, 3.9, 14.3),
    ];
    for (density, item, separator, trailing) in expected {
        for theme in [teksilo_theme_fluent::light(), teksilo_theme_fluent::dark()] {
            let (rows, sep, (chevron, chevron_offset), failures) = menu_geometry(&theme, density);
            let body = &theme.typography.body;
            let short = body.size * body.line_height - MOCK_LINE;
            assert_eq!(rows, vec![item - short; 4], "{density:?}: the row height");
            assert!(
                (sep - separator).abs() < 1e-3,
                "{density:?}: the separator is {sep}, want {separator}"
            );
            assert!(
                (chevron - trailing).abs() < 1e-3,
                "{density:?}: the chevron column is {chevron}, want {trailing}"
            );
            assert!(
                chevron_offset.abs() < 1e-3,
                "{density:?}: the chevron column ends {chevron_offset} from the row's edge"
            );
            assert_eq!(failures, 0, "{density:?}: a conformance failure");
        }
        // The accessor half: what the installed slot tells a row asking it.
        let theme = teksilo_theme_fluent::light().with_density(density);
        let m = theme
            .style_slots
            .menu_item
            .clone()
            .expect("the preset installs a menu-item slot")
            .metrics();
        assert_eq!(m.item_height, item, "{density:?}");
        assert!((m.separator_height - separator).abs() < 1e-3, "{density:?}");
        assert!(
            (m.trailing_column_width - trailing).abs() < 1e-3,
            "{density:?}"
        );
    }
}
