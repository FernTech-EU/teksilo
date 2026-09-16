// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Every accumulator plural is a fold over its singular, so N singular calls
//! and one plural call over the same N values must produce the same thing.
//!
//! "The same thing" is checked structurally, not by counting: each case builds
//! the widget twice in two fresh trees, lays both out at the same proposal, and
//! compares a recursive dump of `(widget type name, bounds)` over the whole
//! subtree. A plural that dropped an element, reordered the run, or wrapped it
//! one node deeper reddens the assertion. The dump is deliberately positional,
//! so an off-by-one in ordering shows up as a diff rather than as an equal
//! count.
//!
//! Builders whose accumulated state never reaches the arena (a `Toast`'s
//! actions, a `DockRail`'s command cluster, a `FilePickerField`'s extension
//! filters) are pinned by in-crate unit tests beside their own source instead,
//! where the private field is reachable.

use teksilo_canvas::SizeProposal;
use teksilo_core::presets::intui;
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_widgets::*;

fn proposal() -> SizeProposal {
    SizeProposal::exact(600.0, 400.0)
}

fn fresh_tree() -> WidgetTree {
    WidgetTree::new().with_theme(intui::light())
}

fn walk(tree: &WidgetTree, id: WidgetId, depth: usize, out: &mut Vec<String>) {
    let b = tree.bounds(id);
    out.push(format!(
        "{:indent$}{} @ {:.2},{:.2} {:.2}x{:.2}",
        "",
        tree.widget_type_name(id).unwrap_or("<unnamed>"),
        b.x,
        b.y,
        b.width,
        b.height,
        indent = depth * 2
    ));
    for child in tree.children(id) {
        walk(tree, child, depth + 1, out);
    }
}

/// Build one variant in its own tree and return its laid-out shape.
fn shape_of(make: impl FnOnce(&mut WidgetTree) -> WidgetId) -> Vec<String> {
    let mut tree = fresh_tree();
    let root = make(&mut tree);
    tree.layout(proposal());
    let mut out = Vec::new();
    walk(&tree, root, 0, &mut out);
    out
}

/// Assert that the singular chain and the plural call build the same tree.
#[track_caller]
fn same_tree(
    singular: impl FnOnce(&mut WidgetTree) -> WidgetId,
    plural: impl FnOnce(&mut WidgetTree) -> WidgetId,
) {
    let a = shape_of(singular);
    let b = shape_of(plural);
    assert!(
        !a.is_empty(),
        "the singular build produced no nodes, so this would compare nothing"
    );
    assert_eq!(a, b, "plural tree differs from the singular chain");
}

/// A distinguishable leaf: different widths make an ordering mistake visible.
fn leaf(width: f32) -> impl Widget + 'static {
    FixedSize::new()
        .width(width)
        .height(10.0)
        .child(RectWidget::new())
}

// ── Layout primitives: `add_children` ───────────────────────────────────────

macro_rules! stack_add_children {
    ($name:ident, $ty:ident) => {
        #[test]
        fn $name() {
            same_tree(
                |t| {
                    let a = t.add(leaf(10.0));
                    let b = t.add(leaf(20.0));
                    let c = t.add(leaf(30.0));
                    t.add($ty::new().child(a).child(b).child(c))
                },
                |t| {
                    let a = t.add(leaf(10.0));
                    let b = t.add(leaf(20.0));
                    let c = t.add(leaf(30.0));
                    t.add($ty::new().children([a, b, c]))
                },
            );
        }
    };
}

stack_add_children!(vstack_add_children, VStack);
stack_add_children!(hstack_add_children, HStack);
stack_add_children!(zstack_add_children, ZStack);
stack_add_children!(wrap_add_children, Wrap);
stack_add_children!(grid_add_children, Grid);
#[test]
fn masonry_add_children() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            let c = t.add(leaf(30.0));
            t.add(MasonryLayout::new(2).child(a).child(b).child(c))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            let c = t.add(leaf(30.0));
            t.add(MasonryLayout::new(2).children([a, b, c]))
        },
    );
}
stack_add_children!(column_flow_add_children, ColumnFlow);

// ── Switcher / Cycle ────────────────────────────────────────────────────────

#[test]
fn switcher_children_boxed() {
    same_tree(
        |t| {
            t.add(
                Switcher::new(Signal::new(0usize))
                    .child(Box::new(leaf(10.0)))
                    .child(Box::new(leaf(20.0))),
            )
        },
        |t| {
            t.add(Switcher::new(Signal::new(0usize)).children([
                Box::new(leaf(10.0)) as Box<dyn Widget>,
                Box::new(leaf(20.0)),
            ]))
        },
    );
}

#[test]
fn switcher_child_ids() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(Switcher::new(Signal::new(0usize)).child(a).child(b))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(Switcher::new(Signal::new(0usize)).children([a, b]))
        },
    );
}

#[test]
fn cycle_children_boxed() {
    same_tree(
        |t| {
            t.add(
                Cycle::new()
                    .child(Box::new(leaf(10.0)))
                    .child(Box::new(leaf(20.0))),
            )
        },
        |t| {
            t.add(Cycle::new().children([
                Box::new(leaf(10.0)) as Box<dyn Widget>,
                Box::new(leaf(20.0)),
            ]))
        },
    );
}

// ── StatusBar ───────────────────────────────────────────────────────────────

#[test]
fn status_bar_children() {
    same_tree(
        |t| t.add(StatusBar::new().child(leaf(10.0)).child(leaf(20.0))),
        |t| t.add(StatusBar::new().children([leaf(10.0), leaf(20.0)])),
    );
}

#[test]
fn status_bar_add_children() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(StatusBar::new().child(a).child(b))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(StatusBar::new().children([a, b]))
        },
    );
}

// ── Toolbar ─────────────────────────────────────────────────────────────────

fn toolbar_action(label: &'static str) -> ToolbarAction {
    ToolbarAction::new(lit!(label), || IconWidget::checkmark(16.0))
}

#[test]
fn toolbar_items() {
    same_tree(
        |t| {
            t.add(
                Toolbar::new()
                    .item(ToolbarItem::action(toolbar_action("One")))
                    .item(ToolbarItem::separator())
                    .item(ToolbarItem::action(toolbar_action("Two"))),
            )
        },
        |t| {
            t.add(Toolbar::new().items([
                ToolbarItem::action(toolbar_action("One")),
                ToolbarItem::separator(),
                ToolbarItem::action(toolbar_action("Two")),
            ]))
        },
    );
}

#[test]
fn toolbar_actions() {
    same_tree(
        |t| {
            t.add(
                Toolbar::new()
                    .action(toolbar_action("One"))
                    .action(toolbar_action("Two")),
            )
        },
        |t| t.add(Toolbar::new().actions([toolbar_action("One"), toolbar_action("Two")])),
    );
}

#[test]
fn toolbar_children() {
    same_tree(
        |t| t.add(Toolbar::new().child(leaf(10.0)).child(leaf(20.0))),
        |t| t.add(Toolbar::new().children([leaf(10.0), leaf(20.0)])),
    );
}

#[test]
fn toolbar_add_children() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(Toolbar::new().child(a).child(b))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(Toolbar::new().children([a, b]))
        },
    );
}

// ── Splitter ────────────────────────────────────────────────────────────────

fn three_pane_model() -> SplitterModel {
    SplitterModel::from_panes(
        vec![
            PaneDescriptor::new().size(100.0),
            PaneDescriptor::new().stretch(1.0),
            PaneDescriptor::new().size(120.0),
        ],
        Orientation::Horizontal,
    )
}

#[test]
fn splitter_panes() {
    same_tree(
        |t| {
            t.add(
                Splitter::new(three_pane_model())
                    .pane(leaf(10.0))
                    .pane(leaf(20.0))
                    .pane(leaf(30.0)),
            )
        },
        |t| t.add(Splitter::new(three_pane_model()).panes([leaf(10.0), leaf(20.0), leaf(30.0)])),
    );
}

#[test]
fn splitter_pane_ids() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            let c = t.add(leaf(30.0));
            t.add(Splitter::new(three_pane_model()).pane(a).pane(b).pane(c))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            let c = t.add(leaf(30.0));
            t.add(Splitter::new(three_pane_model()).panes([a, b, c]))
        },
    );
}

#[test]
fn splitter_children() {
    same_tree(
        |t| {
            t.add(
                Splitter::new(three_pane_model())
                    .child(leaf(10.0))
                    .child(leaf(20.0))
                    .child(leaf(30.0)),
            )
        },
        |t| t.add(Splitter::new(three_pane_model()).children([leaf(10.0), leaf(20.0), leaf(30.0)])),
    );
}

// ── MenuList ────────────────────────────────────────────────────────────────

#[test]
fn menu_list_items() {
    same_tree(
        |t| {
            t.add(
                MenuList::new()
                    .item(MenuItem::new(lit!("One")))
                    .item(MenuItem::new(lit!("Two"))),
            )
        },
        |t| t.add(MenuList::new().items([MenuItem::new(lit!("One")), MenuItem::new(lit!("Two"))])),
    );
}

#[test]
fn menu_list_items_when() {
    let gate_a = Signal::new(true);
    let gate_b = Signal::new(false);
    let (g1, g2) = (gate_a.clone(), gate_b.clone());
    same_tree(
        move |t| {
            t.add(
                MenuList::new()
                    .item_when(MenuItem::new(lit!("One")), g1.clone())
                    .item_when(MenuItem::new(lit!("Two")), g2.clone()),
            )
        },
        move |t| {
            t.add(MenuList::new().items_when([
                (MenuItem::new(lit!("One")), gate_a.clone()),
                (MenuItem::new(lit!("Two")), gate_b.clone()),
            ]))
        },
    );
}

#[test]
fn menu_list_items_boxed_when() {
    let gate_a = Signal::new(true);
    let gate_b = Signal::new(false);
    let (g1, g2) = (gate_a.clone(), gate_b.clone());
    same_tree(
        move |t| {
            t.add(
                MenuList::new()
                    .item_boxed_when(Box::new(MenuItem::new(lit!("One"))), g1.clone())
                    .item_boxed_when(Box::new(MenuItem::new(lit!("Two"))), g2.clone()),
            )
        },
        move |t| {
            t.add(MenuList::new().items_boxed_when([
                (
                    Box::new(MenuItem::new(lit!("One"))) as Box<dyn Widget>,
                    gate_a.clone(),
                ),
                (Box::new(MenuItem::new(lit!("Two"))), gate_b.clone()),
            ]))
        },
    );
}

// ── Breadcrumb ──────────────────────────────────────────────────────────────

#[test]
fn breadcrumb_items() {
    same_tree(
        |t| {
            t.add(
                Breadcrumb::new()
                    .item(BreadcrumbItem::new(lit!("Home")))
                    .item(BreadcrumbItem::new(lit!("Docs")))
                    .item(BreadcrumbItem::new(lit!("Now"))),
            )
        },
        |t| {
            t.add(Breadcrumb::new().items([
                BreadcrumbItem::new(lit!("Home")),
                BreadcrumbItem::new(lit!("Docs")),
                BreadcrumbItem::new(lit!("Now")),
            ]))
        },
    );
}

#[test]
fn breadcrumb_item_ids() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(Breadcrumb::new().item_id(a).item_id(b))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(Breadcrumb::new().item_ids([a, b]))
        },
    );
}

// ── RadioGroup ──────────────────────────────────────────────────────────────

#[test]
fn radio_group_radios() {
    let selected = Signal::new(0usize);
    let s = selected.clone();
    same_tree(
        move |t| {
            t.add(
                RadioGroup::new()
                    .radio(RadioButton::new(0, s.clone()).label(lit!("A")))
                    .radio(RadioButton::new(1, s.clone()).label(lit!("B"))),
            )
        },
        move |t| {
            t.add(RadioGroup::new().radios([
                RadioButton::new(0, selected.clone()).label(lit!("A")),
                RadioButton::new(1, selected.clone()).label(lit!("B")),
            ]))
        },
    );
}

#[test]
fn radio_group_children() {
    same_tree(
        |t| t.add(RadioGroup::new().child(leaf(10.0)).child(leaf(20.0))),
        |t| t.add(RadioGroup::new().children([leaf(10.0), leaf(20.0)])),
    );
}

// ── MenuBar slots ───────────────────────────────────────────────────────────

#[test]
fn menu_bar_leading_slots() {
    same_tree(
        |t| {
            t.add(
                MenuBar::new()
                    .no_dispatcher_install()
                    .leading_slot(leaf(10.0))
                    .leading_slot(leaf(20.0)),
            )
        },
        |t| {
            t.add(
                MenuBar::new()
                    .no_dispatcher_install()
                    .leading_slots([leaf(10.0), leaf(20.0)]),
            )
        },
    );
}

#[test]
fn menu_bar_trailing_slots() {
    same_tree(
        |t| {
            t.add(
                MenuBar::new()
                    .no_dispatcher_install()
                    .trailing_slot(leaf(10.0))
                    .trailing_slot(leaf(20.0)),
            )
        },
        |t| {
            t.add(
                MenuBar::new()
                    .no_dispatcher_install()
                    .trailing_slots([leaf(10.0), leaf(20.0)]),
            )
        },
    );
}

// ── TabWidget ───────────────────────────────────────────────────────────────

#[test]
fn tab_widget_tabs() {
    same_tree(
        |t| {
            t.add(
                TabWidget::new(Signal::new(None))
                    .tab(lit!("One"), leaf(10.0))
                    .tab(lit!("Two"), leaf(20.0)),
            )
        },
        |t| {
            t.add(
                TabWidget::new(Signal::new(None))
                    .tabs([(lit!("One"), leaf(10.0)), (lit!("Two"), leaf(20.0))]),
            )
        },
    );
}

#[test]
fn tab_widget_static_tabs() {
    same_tree(
        |t| {
            t.add(
                TabWidget::new(Signal::new(None))
                    .static_tab(TabInfo::new().title(lit!("One")), leaf(10.0))
                    .static_tab(TabInfo::new().title(lit!("Two")), leaf(20.0)),
            )
        },
        |t| {
            t.add(TabWidget::new(Signal::new(None)).static_tabs([
                (TabInfo::new().title(lit!("One")), leaf(10.0)),
                (TabInfo::new().title(lit!("Two")), leaf(20.0)),
            ]))
        },
    );
}

#[test]
fn tab_widget_tab_ids() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(
                TabWidget::new(Signal::new(None))
                    .tab(lit!("One"), a)
                    .tab(lit!("Two"), b),
            )
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(TabWidget::new(Signal::new(None)).tab_ids([(lit!("One"), a), (lit!("Two"), b)]))
        },
    );
}

#[test]
fn tab_widget_static_tab_ids() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(
                TabWidget::new(Signal::new(None))
                    .static_tab(TabInfo::new().title(lit!("One")), a)
                    .static_tab(TabInfo::new().title(lit!("Two")), b),
            )
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(TabWidget::new(Signal::new(None)).static_tab_ids([
                (TabInfo::new().title(lit!("One")), a),
                (TabInfo::new().title(lit!("Two")), b),
            ]))
        },
    );
}

// ── FormLayout ──────────────────────────────────────────────────────────────

#[test]
fn form_layout_lines() {
    same_tree(
        |t| {
            t.add(
                FormLayout::new()
                    .line(leaf(10.0), leaf(40.0))
                    .line(leaf(20.0), leaf(50.0)),
            )
        },
        |t| t.add(FormLayout::new().lines([(leaf(10.0), leaf(40.0)), (leaf(20.0), leaf(50.0))])),
    );
}

/// The field column of `lines` takes `impl IntoTeksiChild`, not `impl Widget`,
/// so a loop holding ids can use the plural without falling back to `line_ids`.
#[test]
fn form_layout_lines_accepts_ids_in_both_columns() {
    same_tree(
        |t| {
            let (l1, f1) = (t.add(leaf(10.0)), t.add(leaf(40.0)));
            let (l2, f2) = (t.add(leaf(20.0)), t.add(leaf(50.0)));
            t.add(FormLayout::new().line(l1, f1).line(l2, f2))
        },
        |t| {
            let (l1, f1) = (t.add(leaf(10.0)), t.add(leaf(40.0)));
            let (l2, f2) = (t.add(leaf(20.0)), t.add(leaf(50.0)));
            t.add(FormLayout::new().lines([(l1, f1), (l2, f2)]))
        },
    );
}

#[test]
fn form_layout_line_ids() {
    same_tree(
        |t| {
            let (l1, f1) = (t.add(leaf(10.0)), t.add(leaf(40.0)));
            let (l2, f2) = (t.add(leaf(20.0)), t.add(leaf(50.0)));
            t.add(FormLayout::new().line(l1, f1).line(l2, f2))
        },
        |t| {
            let (l1, f1) = (t.add(leaf(10.0)), t.add(leaf(40.0)));
            let (l2, f2) = (t.add(leaf(20.0)), t.add(leaf(50.0)));
            t.add(FormLayout::new().line_ids([(l1, f1), (l2, f2)]))
        },
    );
}

#[test]
fn form_layout_full_width_rows() {
    same_tree(
        |t| {
            t.add(
                FormLayout::new()
                    .full_width(leaf(10.0))
                    .full_width(leaf(20.0)),
            )
        },
        |t| t.add(FormLayout::new().full_width_rows([leaf(10.0), leaf(20.0)])),
    );
}

#[test]
fn form_layout_full_width_row_ids() {
    same_tree(
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(FormLayout::new().full_width(a).full_width(b))
        },
        |t| {
            let a = t.add(leaf(10.0));
            let b = t.add(leaf(20.0));
            t.add(FormLayout::new().full_width_rows([a, b]))
        },
    );
}

// ── MessageBox ──────────────────────────────────────────────────────────────

#[test]
fn message_box_add_buttons() {
    same_tree(
        |t| {
            t.add(
                MessageBox::information(lit!("Title"))
                    .add_button(StandardButton::Ok)
                    .add_button(StandardButton::Cancel),
            )
        },
        |t| {
            t.add(
                MessageBox::information(lit!("Title"))
                    .add_buttons([StandardButton::Ok, StandardButton::Cancel]),
            )
        },
    );
}

// ── Menu model (not widgets: compared through the public node tree) ─────────

fn describe(nodes: &[MenuNode]) -> Vec<String> {
    nodes
        .iter()
        .map(|n| match n {
            MenuNode::Item(entry) => format!("item {:?}", entry.id()),
            MenuNode::Separator => "separator".to_string(),
            MenuNode::Submenu { id, children, .. } => {
                format!("submenu {:?} [{}]", id, describe(children).join(", "))
            }
            MenuNode::Standard(menu) => format!("standard {:?}", menu.role()),
        })
        .collect()
}

#[test]
fn menu_items_items() {
    // Cloning the entries keeps their process-unique ids identical across the
    // two builds, so the comparison is about order and nesting, not identity.
    let one = MenuEntry::new(lit!("One"));
    let two = MenuEntry::new(lit!("Two"));
    let three = MenuEntry::new(lit!("Three"));

    let (a, b, c) = (one.clone(), two.clone(), three.clone());
    let singular =
        MenuModel::new().menu(lit!("File"), move |m| m.item(a).item(b).separator().item(c));
    let (a, b, c) = (one, two, three);
    let plural = MenuModel::new().menu(lit!("File"), move |m| m.items([a, b]).separator().item(c));

    // The two top-level submenus get their own fresh ids, so compare contents.
    let s = describe(&singular.nodes());
    let p = describe(&plural.nodes());
    let inner = |v: &[String]| {
        v[0].split_once('[')
            .unwrap()
            .1
            .trim_end_matches(']')
            .to_string()
    };
    assert_eq!(inner(&s), inner(&p));
    assert!(
        inner(&s).contains("separator"),
        "the fixture lost its separator"
    );
}

#[test]
fn menu_model_standard_menus() {
    let singular = MenuModel::new()
        .standard_menu(StandardMenu::app())
        .standard_menu(StandardMenu::window())
        .standard_menu(StandardMenu::help());
    let plural = MenuModel::new().standard_menus([
        StandardMenu::app(),
        StandardMenu::window(),
        StandardMenu::help(),
    ]);
    assert_eq!(describe(&singular.nodes()), describe(&plural.nodes()));
    assert_eq!(describe(&plural.nodes()).len(), 3);
}
