// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::SizeProposal;
use teksilo_core::accessibility::node_id_to_widget_id;
use teksilo_core::accesskit;
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::ListModel;
use teksilo_i18n::lit;

use crate::tab_widget::header::{first_enabled_index, last_enabled_index};
use crate::tab_widget::{TabBarOrientation, TabHandle, TabId, TabInfo, TabWidget};

#[derive(Debug)]
struct FixedLeaf;

impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &teksilo_core::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        teksilo_canvas::Size::new(120.0, 48.0).into()
    }
}

fn make_tree(n: usize) -> (WidgetTree, Signal<Option<TabId>>) {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let mut widget = TabWidget::new(selected.clone())
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false);
    for i in 0..n {
        widget = widget.static_tab(TabInfo::new().title(lit!(format!("Tab {i}"))), FixedLeaf);
    }
    let _id = tree.add(widget);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    (tree, selected)
}

fn assert_a11y_tree_valid(update: &accesskit::TreeUpdate) {
    accesskit_consumer::Tree::new(update.clone(), false);
}

fn assert_no_dangling_relationships(update: &accesskit::TreeUpdate) {
    let emitted: std::collections::HashSet<accesskit::NodeId> =
        update.nodes.iter().map(|(id, _)| *id).collect();
    for (parent_id, node) in &update.nodes {
        for &target in node.controls() {
            assert!(
                emitted.contains(&target),
                "node {parent_id:?} has controls() → {target:?} absent from tree"
            );
        }
        for &target in node.described_by() {
            assert!(
                emitted.contains(&target),
                "node {parent_id:?} has described_by() → {target:?} absent from tree"
            );
        }
    }
}

fn nodes_with_role(
    update: &accesskit::TreeUpdate,
    role: accesskit::Role,
) -> Vec<accesskit::NodeId> {
    update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == role)
        .map(|(id, _)| *id)
        .collect()
}

#[test]
fn has_exactly_one_tab_list() {
    let (mut tree, _) = make_tree(3);
    let update = tree.sync_accessibility();
    let tab_lists = nodes_with_role(&update, accesskit::Role::TabList);
    assert_eq!(tab_lists.len(), 1);
    assert_a11y_tree_valid(&update);
}

#[test]
fn tab_count_matches_widget_count() {
    for n in [2, 3, 4] {
        let (mut tree, _) = make_tree(n);
        let update = tree.sync_accessibility();
        let tabs = nodes_with_role(&update, accesskit::Role::Tab);
        assert_eq!(tabs.len(), n);
        assert_a11y_tree_valid(&update);
    }
}

#[test]
fn exactly_one_tab_panel_initially() {
    let (mut tree, _) = make_tree(3);
    let update = tree.sync_accessibility();
    let panels = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(panels.len(), 1);
    assert_a11y_tree_valid(&update);
}

#[test]
fn active_tab_has_controls_pointing_into_tree() {
    let (mut tree, _) = make_tree(3);
    let update = tree.sync_accessibility();
    let tab_ids = nodes_with_role(&update, accesskit::Role::Tab);
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();
    let first_tab_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == tab_ids[0])
        .map(|(_, n)| n)
        .unwrap();
    assert!(!first_tab_node.controls().is_empty());
    for &target in first_tab_node.controls() {
        assert!(emitted.contains(&target));
    }
    assert_no_dangling_relationships(&update);
    assert_a11y_tree_valid(&update);
}

// ─── Helpers shared by the new test cases ─────────────────────────────

fn make_tree_with_orientation(
    n: usize,
    orientation: TabBarOrientation,
) -> (WidgetTree, Signal<Option<TabId>>) {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let mut widget = TabWidget::new(selected.clone())
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false);
    if matches!(orientation, TabBarOrientation::Vertical) {
        widget = widget.vertical();
    }
    for i in 0..n {
        widget = widget.static_tab(TabInfo::new().title(lit!(format!("Tab {i}"))), FixedLeaf);
    }
    let _id = tree.add(widget);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    (tree, selected)
}

fn find_node(update: &accesskit::TreeUpdate, id: accesskit::NodeId) -> Option<&accesskit::Node> {
    update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == id)
        .map(|(_, n)| n)
}

// ─── New a11y test cases (TabWidget accessibility fixes) ─────────────

#[test]
fn tab_list_has_orientation_horizontal() {
    let (mut tree, _) = make_tree_with_orientation(3, TabBarOrientation::Horizontal);
    let update = tree.sync_accessibility();
    let tab_lists = nodes_with_role(&update, accesskit::Role::TabList);
    assert_eq!(tab_lists.len(), 1);
    let node = find_node(&update, tab_lists[0]).unwrap();
    assert_eq!(node.orientation(), Some(accesskit::Orientation::Horizontal));
    assert_a11y_tree_valid(&update);
}

#[test]
fn tab_list_has_orientation_vertical() {
    let (mut tree, _) = make_tree_with_orientation(3, TabBarOrientation::Vertical);
    let update = tree.sync_accessibility();
    let tab_lists = nodes_with_role(&update, accesskit::Role::TabList);
    assert_eq!(tab_lists.len(), 1);
    let node = find_node(&update, tab_lists[0]).unwrap();
    assert_eq!(node.orientation(), Some(accesskit::Orientation::Vertical));
    assert_a11y_tree_valid(&update);
}

#[test]
fn tabs_have_position_and_size_of_set() {
    let n = 5;
    let (mut tree, _) = make_tree(n);
    let update = tree.sync_accessibility();
    let tab_ids = nodes_with_role(&update, accesskit::Role::Tab);
    assert_eq!(tab_ids.len(), n);
    // Asked the way an adapter asks it: the position off the tab, the size by
    // walking up to the `Role::TabList` bar. Pinned and regular tabs share one
    // TabList, so they share one set.
    for (i, &tab_id) in tab_ids.iter().enumerate() {
        crate::a11y_set_semantics::assert_announces(
            &update,
            tab_id,
            i + 1,
            n,
            &format!("tab at index {i}"),
        );
    }
    assert_a11y_tree_valid(&update);
}

#[test]
fn tab_panel_is_labelled_by_active_tab() {
    let (mut tree, _) = make_tree(3);
    let update = tree.sync_accessibility();
    let tab_ids = nodes_with_role(&update, accesskit::Role::Tab);
    let panel_ids = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(panel_ids.len(), 1, "exactly one panel mounted at a time");
    let panel = find_node(&update, panel_ids[0]).unwrap();
    let labelled_by: Vec<accesskit::NodeId> = panel.labelled_by().to_vec();
    assert_eq!(
        labelled_by.len(),
        1,
        "panel should be labelled by exactly one tab"
    );
    assert_eq!(
        labelled_by[0], tab_ids[0],
        "panel should be labelled by the active (first) tab"
    );
    assert_no_dangling_relationships(&update);
    assert_a11y_tree_valid(&update);
}

#[test]
fn roving_tab_stop_only_on_selected() {
    let n = 4;
    let (mut tree, _) = make_tree(n);
    // sync to flush ids; we'll then use accesskit node ids to find header WidgetIds
    let update = tree.sync_accessibility();
    let tab_node_ids = nodes_with_role(&update, accesskit::Role::Tab);
    assert_eq!(tab_node_ids.len(), n);
    let header_widget_ids: Vec<_> = tab_node_ids
        .iter()
        .map(|nid| node_id_to_widget_id(*nid))
        .collect();

    // Only the selected (index 0 by default) tab should be in the
    // Tab-key traversal order; the others must report `tab_stop == false`.
    //
    // The positive half reads the traversal graph, not the `tab_stop` flag
    // alone: a stop is focusable *and* unsuppressed, so the flag by itself is
    // satisfied by a header no keyboard user can reach. Deleting the header's
    // `focusable(true)` left this assertion green.
    assert!(
        tree.tab_stops_within(tree.roots()[0])
            .contains(&header_widget_ids[0]),
        "selected tab must be a Tab stop"
    );
    for (i, id) in header_widget_ids.iter().enumerate().skip(1) {
        assert!(
            !tree.tab_stop(*id),
            "unselected tab at index {i} must NOT be a Tab stop (roving tabindex)"
        );
    }
}

#[test]
fn tab_panel_is_focusable_when_opted_in() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let widget = TabWidget::new(selected.clone())
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false)
        .static_tab(
            TabInfo::new().title(lit!("About")).focusable_panel(true),
            FixedLeaf,
        );
    let _ = tree.add(widget);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();
    let panel_ids = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(panel_ids.len(), 1);
    let panel = find_node(&update, panel_ids[0]).unwrap();
    assert!(
        panel.supports_action(accesskit::Action::Focus),
        "panel opted in via focusable_panel(true) must advertise Action::Focus"
    );
    let panel_widget = node_id_to_widget_id(panel_ids[0]);
    // The framework should also treat the panel as focusable (Tab-key
    // discoverable) — implements the ARIA tabindex="0" contract for
    // empty tabpanels. `first_focusable_descendant` walks the subtree
    // root and returns the first focusable WidgetId; for a
    // self-focusable pane with no focusable child, that's the pane
    // itself.
    assert_eq!(
        tree.first_focusable_descendant(panel_widget),
        Some(panel_widget),
        "focusable_panel(true) should make the pane itself focusable"
    );
}

#[test]
fn tab_panel_is_not_self_focusable_by_default() {
    let (mut tree, _) = make_tree(2);
    let update = tree.sync_accessibility();
    let panel_ids = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(panel_ids.len(), 1);
    let panel = find_node(&update, panel_ids[0]).unwrap();
    assert!(
        !panel.supports_action(accesskit::Action::Focus),
        "panel without focusable_panel opt-in must NOT advertise Action::Focus"
    );
}

#[test]
fn first_last_enabled_helpers() {
    // No tabs disabled.
    assert_eq!(first_enabled_index(&[true, true, true]), Some(0));
    assert_eq!(last_enabled_index(&[true, true, true]), Some(2));
    // Leading / trailing disabled.
    assert_eq!(first_enabled_index(&[false, false, true, true]), Some(2));
    assert_eq!(last_enabled_index(&[true, true, false, false]), Some(1));
    // All disabled.
    assert_eq!(first_enabled_index(&[false, false]), None);
    assert_eq!(last_enabled_index(&[false, false]), None);
    // Empty.
    assert_eq!(first_enabled_index(&[]), None);
    assert_eq!(last_enabled_index(&[]), None);
}

#[test]
fn dynamic_model_tab_count_is_reflected() {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let model: ListModel<TabHandle> = ListModel::from_vec(vec![
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(lit!("A")), ()),
        TabHandle::dynamic(TabId::fresh(), "doc", TabInfo::new().title(lit!("B")), ()),
    ]);

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected)
            .dynamic_tab::<()>("doc", |_h, _s| Box::new(FixedLeaf) as Box<dyn Widget>)
            .dynamic_model(model.clone())
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();
    assert_eq!(nodes_with_role(&update, accesskit::Role::Tab).len(), 2);

    // Push another tab — bar rebuilds.
    model.push(TabHandle::dynamic(
        TabId::fresh(),
        "doc",
        TabInfo::new().title(lit!("C")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();
    assert_eq!(nodes_with_role(&update, accesskit::Role::Tab).len(), 3);
    assert_no_dangling_relationships(&update);
    assert_a11y_tree_valid(&update);
}

#[test]
fn tab_label_does_not_survive_as_a_duplicate_at_node() {
    // The audit's detection rule (an ancestor whose own name equals a
    // strict-descendant Label's name) does not, strictly, cover this site:
    // `TabHeader::accessibility` names the tab from `at_name`, falling back to
    // the tooltip then the visible label, so a tab whose accessible name has
    // been steered away from its painted title (e.g. a divergent tooltip
    // fallback) would not trip a plain string-equality check — yet the label
    // is still a redundant extra stop for AT users navigating the strip. So
    // assert directly against the painted title rather than against equality
    // with the tab's current name.
    let n = 3;
    let (mut tree, _) = make_tree(n);
    let update = tree.sync_accessibility();

    let titles: Vec<String> = (0..n).map(|i| format!("Tab {i}")).collect();

    let tab_node_ids = nodes_with_role(&update, accesskit::Role::Tab);
    assert_eq!(tab_node_ids.len(), n);
    for (i, &tab_node_id) in tab_node_ids.iter().enumerate() {
        let tab_node = find_node(&update, tab_node_id).unwrap();
        assert_eq!(
            tab_node.label(),
            Some(titles[i].as_str()),
            "hiding the embedded label must not take the tab's own name away with it",
        );
    }

    assert!(
        !update.nodes.iter().any(|(_, node)| {
            node.role() == accesskit::Role::Label
                && titles.iter().any(|t| node.label() == Some(t.as_str()))
        }),
        "a tab title must not survive as a separate, duplicate-named Role::Label node",
    );

    assert_a11y_tree_valid(&update);
}

/// The widget of the node named `name` in the tree a reader walks.
fn widget_named(tree: &WidgetTree, name: &str) -> teksilo_core::WidgetId {
    let update = tree.accessibility_tree_snapshot();
    let (id, _) = update
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(name))
        .unwrap_or_else(|| panic!("no node named {name:?}"));
    node_id_to_widget_id(*id)
}

#[test]
fn a_panel_shown_again_is_alive_to_a_screen_reader() {
    // `tabs-panel-revisit` (tools/reader): Enter into Settings' panel, go to
    // Doc 1, come back, Enter into the panel again. A static pane is kept and
    // parked dormant while another tab shows, so it used to come back under
    // the ids the AT-SPI adapter had announced defunct as it left, and Orca
    // 46.1 dropped the focus on its button ("Ignoring defunct object: [push
    // button: 'Toggle orientation']", 3 of 3 runs): silent, and Space on it
    // silent too.
    use crate::common::heard_test::{Heard, Listener};
    let settings = TabId::fresh();
    let doc = TabId::fresh();
    let selected = Signal::new(Some(settings));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TabWidget::new(selected.clone())
            .static_tab_with_id(
                settings,
                TabInfo::new().title(lit!("Settings")),
                crate::Button::new(lit!("Toggle orientation")),
            )
            .static_tab_with_id(
                doc,
                TabInfo::new().title(lit!("Doc 1")),
                crate::Button::new(lit!("Make an edit")),
            )
            .show_scroll_arrows(false)
            .show_overflow_dropdown(false),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let toggle = widget_named(&tree, "Toggle orientation");
    tree.focus(toggle);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let mut listener = Listener::attach(&mut tree);

    for visit in 2..=3 {
        selected.set(Some(doc));
        tree.layout(SizeProposal::exact(640.0, 320.0));
        let _ = listener.heard(&mut tree);
        selected.set(Some(settings));
        tree.layout(SizeProposal::exact(640.0, 320.0));
        let _ = listener.heard(&mut tree);
        tree.focus(toggle);
        tree.layout(SizeProposal::exact(640.0, 320.0));
        assert_eq!(
            listener.heard(&mut tree),
            vec![Heard::Focus("Toggle orientation".to_string())],
            "visit {visit}: focus on the returned panel's button is heard"
        );
        assert_eq!(listener.dead(), Vec::<String>::new(), "visit {visit}");
    }
}
