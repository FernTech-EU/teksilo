use fern_canvas::SizeProposal;
use fern_core::accessibility::node_id_to_widget_id;
use fern_core::accesskit;
use fern_core::signal::Signal;
use fern_core::widget::Widget;
use fern_core::widget_tree::WidgetTree;
use fern_data::ListModel;

use crate::tab_widget::header::{first_enabled_index, last_enabled_index};
use crate::tab_widget::{TabBarOrientation, TabHandle, TabId, TabInfo, TabWidget};

#[derive(Debug)]
struct FixedLeaf;

impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &fern_core::LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        fern_canvas::Size::new(120.0, 48.0).into()
    }
}

fn make_tree(n: usize) -> (WidgetTree, Signal<Option<TabId>>) {
    let selected: Signal<Option<TabId>> = Signal::new(None);
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let mut widget = TabWidget::new(selected.clone())
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false);
    for i in 0..n {
        widget = widget.static_tab(
            TabInfo::new().title(fern_i18n::LocalizedString::literal(format!("Tab {i}"))),
            FixedLeaf,
        );
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let mut widget = TabWidget::new(selected.clone())
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false);
    if matches!(orientation, TabBarOrientation::Vertical) {
        widget = widget.vertical();
    }
    for i in 0..n {
        widget = widget.static_tab(
            TabInfo::new().title(fern_i18n::LocalizedString::literal(format!("Tab {i}"))),
            FixedLeaf,
        );
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
    for (i, &tab_id) in tab_ids.iter().enumerate() {
        let node = find_node(&update, tab_id).unwrap();
        assert_eq!(
            node.size_of_set(),
            Some(n),
            "tab at index {i} should report size_of_set == {n}"
        );
        assert_eq!(
            node.position_in_set(),
            Some(i + 1),
            "tab at index {i} should report 1-based position_in_set"
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
    assert!(
        tree.tab_stop(header_widget_ids[0]),
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
    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
    let widget = TabWidget::new(selected.clone())
        .show_scroll_arrows(false)
        .show_overflow_dropdown(false)
        .static_tab(
            TabInfo::new()
                .title(fern_i18n::LocalizedString::literal("About"))
                .focusable_panel(true),
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
        TabHandle::dynamic(
            TabId::fresh(),
            "doc",
            TabInfo::new().title(fern_i18n::LocalizedString::literal("A")),
            (),
        ),
        TabHandle::dynamic(
            TabId::fresh(),
            "doc",
            TabInfo::new().title(fern_i18n::LocalizedString::literal("B")),
            (),
        ),
    ]);

    let mut tree = WidgetTree::new().with_theme(fern_core::presets::intui::light());
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
        TabInfo::new().title(fern_i18n::LocalizedString::literal("C")),
        (),
    ));
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();
    assert_eq!(nodes_with_role(&update, accesskit::Role::Tab).len(), 3);
    assert_no_dangling_relationships(&update);
    assert_a11y_tree_valid(&update);
}
