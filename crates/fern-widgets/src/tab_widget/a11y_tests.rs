use fern_canvas::SizeProposal;
use fern_core::event::{Key, Modifiers, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::Widget;
use fern_core::widget_id::WidgetId;
use fern_core::widget_tree::WidgetTree;
use fern_tokens::Theme;

use crate::tab_widget::{TabItem, TabWidget};

// Re-export accesskit through fern_core so fern-widgets doesn't need a
// direct dep on the accesskit crate.
use fern_core::accesskit;

// ── test infrastructure ───────────────────────────────────────────────────────

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

/// Build a TabWidget with `n` uniform tabs and lay it out.
/// Returns `(tree, selected_signal, tabs_widget_id)`.
fn make_tree(n: usize) -> (WidgetTree, Signal<usize>, WidgetId) {
    let selected = Signal::new(0_usize);
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    let mut widget = TabWidget::new(selected.clone());
    for i in 0..n {
        widget = widget.tab_literal(format!("Tab {i}"), FixedLeaf);
    }
    let tabs_id = tree.add(widget);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    (tree, selected, tabs_id)
}

/// Walk the widget tree to the Nth tab header (0-based index).
/// Mirrors the `header_id` helper in the sibling `tests` module.
fn header_at(tree: &WidgetTree, tabs_id: WidgetId, index: usize) -> WidgetId {
    let root = tree.child_widget(tabs_id, 0);
    let tab_bar = tree.child_widget(root, 0);
    let row = tree.child_widget(tab_bar, 0);
    let expand = tree.child_widget(row, 0);
    let scroll = tree.child_widget(expand, 0);
    let headers = tree.child_widget(scroll, 0);
    tree.child_widget(headers, index)
}

/// Feed a `TreeUpdate` into `accesskit_consumer::Tree`, running the same
/// structural validation that VoiceOver triggers on activation. Panics on
/// duplicate children, dangling relationship targets, orphaned nodes, and
/// an invalid focus NodeId — turning those runtime AT crashes into CI failures.
fn assert_a11y_tree_valid(update: &accesskit::TreeUpdate) {
    accesskit_consumer::Tree::new(update.clone(), false);
}

/// Assert that every `controls()` / `described_by()` target is present in
/// the emitted tree. This is the invariant our post-processing pass enforces;
/// a future refactor that silently removes the pass would be caught here.
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

/// All NodeIds in the update whose role equals `role`.
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

// ── structure ─────────────────────────────────────────────────────────────────

#[test]
fn has_exactly_one_tab_list() {
    let (mut tree, _, _) = make_tree(3);
    let update = tree.sync_accessibility();
    let tab_lists = nodes_with_role(&update, accesskit::Role::TabList);
    assert_eq!(
        tab_lists.len(),
        1,
        "expected 1 TabList, found {}",
        tab_lists.len()
    );
    assert_a11y_tree_valid(&update);
}

#[test]
fn tab_count_matches_widget_count() {
    for n in [2, 3, 4] {
        let (mut tree, _, _) = make_tree(n);
        let update = tree.sync_accessibility();
        let tabs = nodes_with_role(&update, accesskit::Role::Tab);
        assert_eq!(
            tabs.len(),
            n,
            "with {n} tabs: expected {n} Tab nodes, found {}",
            tabs.len()
        );
        assert_a11y_tree_valid(&update);
    }
}

#[test]
fn exactly_one_tab_panel_initially() {
    let (mut tree, _, _) = make_tree(3);
    let update = tree.sync_accessibility();
    let panels = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(
        panels.len(),
        1,
        "expected 1 TabPanel (active pane only), found {}",
        panels.len()
    );
    assert_a11y_tree_valid(&update);
}

#[test]
fn tabs_are_descendants_of_tab_list() {
    // FernUI wraps tab headers in a ScrollArea inside the TabList node, so
    // Tabs are not *direct* children of TabList but must be reachable from it.
    // We verify this by walking the TreeUpdate from the TabList root.
    let (mut tree, _, _) = make_tree(3);
    let update = tree.sync_accessibility();

    let tab_list_id = nodes_with_role(&update, accesskit::Role::TabList)[0];
    let tab_ids: std::collections::HashSet<_> =
        nodes_with_role(&update, accesskit::Role::Tab).into_iter().collect();

    // Build a parent → children map so we can walk descendants.
    let children_of: std::collections::HashMap<accesskit::NodeId, Vec<accesskit::NodeId>> = update
        .nodes
        .iter()
        .map(|(id, n)| (*id, n.children().to_vec()))
        .collect();

    // BFS from TabList; collect all descendant NodeIds.
    let mut descendants = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(tab_list_id);
    while let Some(cur) = queue.pop_front() {
        if let Some(kids) = children_of.get(&cur) {
            for &kid in kids {
                descendants.insert(kid);
                queue.push_back(kid);
            }
        }
    }

    for tab_id in &tab_ids {
        assert!(
            descendants.contains(tab_id),
            "Tab {tab_id:?} is not a descendant of the TabList"
        );
    }
    assert_a11y_tree_valid(&update);
}

// ── relationships ─────────────────────────────────────────────────────────────

#[test]
fn active_tab_has_controls_pointing_into_tree() {
    let (mut tree, _, _) = make_tree(3);
    let update = tree.sync_accessibility();

    let tab_ids = nodes_with_role(&update, accesskit::Role::Tab);
    let emitted: std::collections::HashSet<_> = update.nodes.iter().map(|(id, _)| *id).collect();

    // The first Tab (selected by default) must have a controls() target.
    let first_tab_node = update
        .nodes
        .iter()
        .find(|(id, _)| *id == tab_ids[0])
        .map(|(_, n)| n)
        .unwrap();

    assert!(
        !first_tab_node.controls().is_empty(),
        "active Tab must have at least one controls() target"
    );
    for &target in first_tab_node.controls() {
        assert!(
            emitted.contains(&target),
            "Tab controls() → {target:?} is absent from the tree"
        );
    }
    assert_no_dangling_relationships(&update);
    assert_a11y_tree_valid(&update);
}

#[test]
fn switching_tab_keeps_controls_valid() {
    let (mut tree, selected, _) = make_tree(3);

    for i in 0..3 {
        selected.set(i);
        tree.layout(SizeProposal::exact(640.0, 320.0));
        let update = tree.sync_accessibility();

        let panels = nodes_with_role(&update, accesskit::Role::TabPanel);
        assert_eq!(
            panels.len(),
            1,
            "after switching to tab {i}: expected 1 TabPanel, found {}",
            panels.len()
        );
        assert_no_dangling_relationships(&update);
        assert_a11y_tree_valid(&update);
    }
}

#[test]
fn inactive_panels_are_absent_from_tree() {
    let (mut tree, selected, _) = make_tree(3);

    selected.set(1);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();

    let panels = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(
        panels.len(),
        1,
        "only the active panel should be in the tree; found {}",
        panels.len()
    );
    assert_a11y_tree_valid(&update);
}

// ── state ─────────────────────────────────────────────────────────────────────

#[test]
fn access_click_on_tab_updates_selected_and_tree_is_valid() {
    // AccessAction::Click changes selection (like a pointer click) but does
    // not necessarily move keyboard focus. What matters is that the tree
    // remains structurally valid after the selection change.
    let (mut tree, _, tabs_id) = make_tree(3);

    let second_header = header_at(&tree, tabs_id, 1);
    tree.dispatch_event(WidgetEvent::AccessAction {
        action: accesskit::Action::Click,
        target: Some(second_header),
        target_node: fern_core::accessibility::root_node_id(),
        data: None,
    });
    tree.layout(SizeProposal::exact(640.0, 320.0));

    let update = tree.sync_accessibility();
    // The active panel must have switched (exactly 1 TabPanel, valid tree).
    let panels = nodes_with_role(&update, accesskit::Role::TabPanel);
    assert_eq!(panels.len(), 1);
    assert_no_dangling_relationships(&update);
    assert_a11y_tree_valid(&update);
}

// ── disabled tabs ─────────────────────────────────────────────────────────────

#[test]
fn disabled_tab_has_no_click_action() {
    let selected = Signal::new(0_usize);
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    tree.add(
        TabWidget::new(selected)
            .tab_literal("Enabled", FixedLeaf)
            .tab_item(TabItem::new_literal("Locked", FixedLeaf).enabled(false))
            .tab_literal("Also enabled", FixedLeaf),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();

    let tab_ids = nodes_with_role(&update, accesskit::Role::Tab);
    assert_eq!(tab_ids.len(), 3);

    let node_for = |idx: usize| {
        update
            .nodes
            .iter()
            .find(|(id, _)| *id == tab_ids[idx])
            .map(|(_, n)| n)
            .unwrap()
    };

    assert!(
        !node_for(1).supports_action(accesskit::Action::Click),
        "disabled Tab must not support Action::Click"
    );
    assert!(
        node_for(0).supports_action(accesskit::Action::Click),
        "enabled Tab[0] must support Action::Click"
    );
    assert!(
        node_for(2).supports_action(accesskit::Action::Click),
        "enabled Tab[2] must support Action::Click"
    );
    assert_a11y_tree_valid(&update);
}

#[test]
fn disabled_tab_still_appears_in_tree() {
    let selected = Signal::new(0_usize);
    let mut tree = WidgetTree::new().with_theme(Theme::light_default());
    tree.add(
        TabWidget::new(selected)
            .tab_literal("A", FixedLeaf)
            .tab_item(TabItem::new_literal("Locked", FixedLeaf).enabled(false))
            .tab_literal("C", FixedLeaf),
    );
    tree.layout(SizeProposal::exact(640.0, 320.0));
    let update = tree.sync_accessibility();

    let tabs = nodes_with_role(&update, accesskit::Role::Tab);
    assert_eq!(
        tabs.len(),
        3,
        "disabled tabs must still appear in the tree; found {}",
        tabs.len()
    );
    assert_a11y_tree_valid(&update);
}

// ── focus traversal ───────────────────────────────────────────────────────────

#[test]
fn keyboard_navigation_visits_each_tab_in_order() {
    let (mut tree, selected, tabs_id) = make_tree(3);

    // Press Tab to focus the tab strip.
    tree.press_key(Key::Tab, Modifiers::NONE);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(
        tree.focused(),
        Some(header_at(&tree, tabs_id, 0)),
        "first Tab key should focus Tab[0]"
    );

    // Arrow right moves to Tab[1] and selects it.
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(selected.get(), 1);
    assert_eq!(tree.focused(), Some(header_at(&tree, tabs_id, 1)));
    assert_a11y_tree_valid(&tree.sync_accessibility());

    // Arrow right again moves to Tab[2].
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    tree.layout(SizeProposal::exact(640.0, 320.0));
    assert_eq!(selected.get(), 2);
    assert_eq!(tree.focused(), Some(header_at(&tree, tabs_id, 2)));
    assert_a11y_tree_valid(&tree.sync_accessibility());
}

#[test]
fn no_extra_focusable_nodes_beyond_tab_headers() {
    // Regression guard for "more focusable sub-widgets than expected".
    // Implementation-internal wrapper nodes must not expose Action::Focus.
    let (mut tree, _, _) = make_tree(3);
    let update = tree.sync_accessibility();

    let focusable_count = update
        .nodes
        .iter()
        .filter(|(_, n)| n.supports_action(accesskit::Action::Focus))
        .count();

    let tab_count = nodes_with_role(&update, accesskit::Role::Tab).len();

    // Must be at least N (one per tab header).
    assert!(
        focusable_count >= tab_count,
        "fewer focusable nodes ({focusable_count}) than Tab headers ({tab_count})"
    );

    // Must not exceed 2× N — if it does, internal wrappers have leaked
    // Action::Focus and will show up as extra Tab stops in screen readers.
    assert!(
        focusable_count <= tab_count * 2,
        "too many focusable nodes ({focusable_count}) for {tab_count} tabs — \
         check that layout wrappers are not inadvertently focusable"
    );

    assert_a11y_tree_valid(&update);
}
