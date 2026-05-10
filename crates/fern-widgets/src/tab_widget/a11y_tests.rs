use fern_canvas::SizeProposal;
use fern_core::accesskit;
use fern_core::signal::Signal;
use fern_core::widget::Widget;
use fern_core::widget_tree::WidgetTree;
use fern_data::ListModel;
use fern_core::Theme;

use crate::tab_widget::{TabHandle, TabId, TabInfo, TabWidget};

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
