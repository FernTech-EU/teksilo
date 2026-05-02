use super::*;

use crate::accessibility::{AccessNodeBuilder, AccessibilityInfo};

impl WidgetTree {
    /// Build an AccessKit `TreeUpdate` from the current state of all active
    /// widgets. Call this once per frame, between layout and paint, and push
    /// the result to the `accesskit_winit::Adapter`.
    /// Caches the result and only rebuilds when layout has changed.
    pub fn sync_accessibility(&mut self) -> accesskit::TreeUpdate {
        if !self.a11y_dirty
            && let Some(cached) = &self.cached_a11y
        {
            return cached.clone();
        }

        let (update, parents) = self.build_accessibility_tree();
        self.cached_a11y = Some(update.clone());
        self.synthetic_parent_map = parents;
        self.a11y_dirty = false;
        update
    }

    fn build_accessibility_tree(
        &self,
    ) -> (
        accesskit::TreeUpdate,
        std::collections::HashMap<accesskit::NodeId, WidgetId>,
    ) {
        use crate::accessibility::{root_node_id, widget_id_to_node_id};

        let roots = self.arena.roots();
        let mut nodes: Vec<(accesskit::NodeId, accesskit::Node)> = Vec::new();
        let mut synthetic_parents: std::collections::HashMap<accesskit::NodeId, WidgetId> =
            std::collections::HashMap::new();
        // Global deduplication: AccessKit's consumer panics if the same child
        // NodeId appears in more than one node's children list across a TreeUpdate.
        // Track which widget first claimed each child so we can skip duplicates
        // and emit a diagnostic pointing at the two conflicting parents.
        let mut seen_children: std::collections::HashMap<accesskit::NodeId, WidgetId> =
            std::collections::HashMap::new();

        let mut root = accesskit::Node::new(accesskit::Role::Window);
        for &root_id in &roots {
            if self.arena.is_active(root_id) {
                let child_nid = widget_id_to_node_id(root_id);
                if seen_children.insert(child_nid, root_id).is_none() {
                    root.push_child(child_nid);
                } else {
                    eprintln!(
                        "FernUI bug: duplicate accessibility child {:?} in Window root — \
                         already claimed by another parent. Please file a bug report.",
                        root_id
                    );
                }
            }
        }
        nodes.push((root_node_id(), root));

        for &root_id in &roots {
            self.build_accessibility_recursive(
                root_id,
                &mut nodes,
                &mut synthetic_parents,
                &mut seen_children,
            );
        }

        let focus = self
            .focused
            .filter(|id| self.arena.is_active(*id))
            .map(widget_id_to_node_id)
            .unwrap_or_else(root_node_id);

        // Strip relationship targets (controls, described_by) that reference
        // NodeIds absent from the emitted tree. Dormant widgets (e.g. inactive
        // tab panels) are excluded from the TreeUpdate; if a node still holds a
        // `push_controlled` or `push_described_by` reference to one of them,
        // accesskit_macos will unwrap() it and panic when VoiceOver follows the
        // linked_ui_elements attribute.
        let emitted: std::collections::HashSet<accesskit::NodeId> =
            nodes.iter().map(|(id, _)| *id).collect();
        for (_, node) in &mut nodes {
            let controlled: Vec<_> = node
                .controls()
                .iter()
                .filter(|id| emitted.contains(*id))
                .copied()
                .collect();
            if controlled.len() != node.controls().len() {
                node.set_controls(controlled);
            }
            let described: Vec<_> = node
                .described_by()
                .iter()
                .filter(|id| emitted.contains(*id))
                .copied()
                .collect();
            if described.len() != node.described_by().len() {
                node.set_described_by(described);
            }
        }

        (
            accesskit::TreeUpdate {
                nodes,
                tree: Some(accesskit::Tree::new(root_node_id())),
                tree_id: accesskit::TreeId::ROOT,
                focus,
            },
            synthetic_parents,
        )
    }

    /// Look up the owning widget for a synthetic AccessKit `NodeId`
    /// emitted by `push_text_run_child` / `push_paragraph_child`.
    /// Used by `handle_accessibility_actions` to route an
    /// `ActionRequest` targeting a TextRun child back to the
    /// editor that owns it.
    pub fn widget_for_synthetic(&self, node_id: accesskit::NodeId) -> Option<WidgetId> {
        self.synthetic_parent_map.get(&node_id).copied()
    }

    fn build_accessibility_recursive(
        &self,
        id: WidgetId,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        synthetic_parents: &mut std::collections::HashMap<accesskit::NodeId, WidgetId>,
        seen_children: &mut std::collections::HashMap<accesskit::NodeId, WidgetId>,
    ) {
        use crate::accessibility::widget_id_to_node_id;

        if !self.arena.is_active(id) {
            return;
        }

        let node = self.arena.get(id).unwrap();
        let mut builder = AccessNodeBuilder::for_widget(id);
        node.widget.accessibility(&mut builder);

        let children = self.arena.children(id);
        for &child_id in children {
            if self.arena.is_active(child_id) {
                let child_nid = widget_id_to_node_id(child_id);
                if let Some(&prior_parent) = seen_children.get(&child_nid) {
                    eprintln!(
                        "FernUI bug: duplicate accessibility child {:?}: \
                         first claimed by parent {:?}, now also claimed by {:?}. \
                         Please file a bug report.",
                        child_id, prior_parent, id
                    );
                    continue;
                }
                seen_children.insert(child_nid, id);
                builder.inner_mut().push_child(child_nid);
            }
        }

        let bounds = self.arena.bounds(id);
        builder.inner_mut().set_bounds(accesskit::Rect {
            x0: bounds.x as f64,
            y0: bounds.y as f64,
            x1: (bounds.x + bounds.width) as f64,
            y1: (bounds.y + bounds.height) as f64,
        });

        if !self.arena.is_enabled(id) {
            builder.set_disabled();
        }

        if let Some(tooltip) = self
            .tooltips
            .iter()
            .find(|t| t.anchor_id == id && t.overlay_id.is_some())
        {
            builder
                .inner_mut()
                .push_described_by(widget_id_to_node_id(tooltip.content_id));
        }

        let (node_id, ak_node, synthetic_children) = builder.build(id);
        nodes.push((node_id, ak_node));
        // Merge the widget's emitted synthetic children into the
        // tree update and record their parent-widget mapping so
        // `handle_accessibility_actions` can route incoming
        // `ActionRequest`s targeting these child NodeIds back to
        // the owning widget.
        for (syn_id, syn_node) in synthetic_children {
            nodes.push((syn_id, syn_node));
            synthetic_parents.insert(syn_id, id);
        }

        for &child_id in children {
            self.build_accessibility_recursive(
                child_id,
                nodes,
                synthetic_parents,
                seen_children,
            );
        }
    }

    pub fn accessibility_node(&self, id: WidgetId) -> AccessibilityInfo {
        let node = self.arena.get(id).unwrap();
        let mut builder = AccessNodeBuilder::for_widget(id);
        node.widget.accessibility(&mut builder);
        let role = builder.role();
        let name = builder.name().map(|s| s.to_string());
        let actions = builder.actions().to_vec();
        let mut info = AccessibilityInfo::new(role, name, actions);
        if let Some(toggled) = builder.toggled() {
            info = info.with_toggled(toggled);
        }
        if let Some(expanded) = builder.expanded() {
            info = info.with_expanded(expanded);
        }
        if let Some(selected) = builder.selected() {
            info = info.with_selected(selected);
        }
        if !self.arena.is_enabled(id) {
            info = info.with_disabled(true);
        }
        if builder.is_hidden() {
            info = info.with_hidden(true);
        }
        info
    }

    pub fn find_by_role(&self, role: accesskit::Role) -> Option<WidgetId> {
        for id in self.arena.active_ids() {
            let node = self.arena.get(id).unwrap();
            let mut builder = AccessNodeBuilder::new();
            node.widget.accessibility(&mut builder);
            if builder.role() == role {
                return Some(id);
            }
        }
        None
    }

    pub fn find_by_label(&self, label: &str) -> Option<WidgetId> {
        for id in self.arena.active_ids() {
            let node = self.arena.get(id).unwrap();
            let mut builder = AccessNodeBuilder::new();
            node.widget.accessibility(&mut builder);
            if builder.name() == Some(label) {
                return Some(id);
            }
        }
        None
    }

    pub fn find_by_action(&self, action: accesskit::Action) -> Option<WidgetId> {
        for id in self.arena.active_ids() {
            let node = self.arena.get(id).unwrap();
            let mut builder = AccessNodeBuilder::new();
            node.widget.accessibility(&mut builder);
            if builder.actions().contains(&action) {
                return Some(id);
            }
        }
        None
    }

    /// Get the text content of a widget from its accessibility name.
    /// Equivalent to the label set via `AccessNodeBuilder::set_name`.
    pub fn text_content(&self, id: WidgetId) -> Option<String> {
        let node = self.arena.get(id)?;
        let mut builder = AccessNodeBuilder::for_widget(id);
        node.widget.accessibility(&mut builder);
        builder.name().map(|s| s.to_string())
    }

    /// Get the text value of a widget from its accessibility value.
    /// Equivalent to the value set via `AccessNodeBuilder::set_value`.
    pub fn text_value(&self, id: WidgetId) -> Option<String> {
        let node = self.arena.get(id)?;
        let mut builder = AccessNodeBuilder::for_widget(id);
        node.widget.accessibility(&mut builder);
        builder.value().map(|s| s.to_string())
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    /// Feed a `TreeUpdate` into `accesskit_consumer::Tree`, which runs the
    /// same validation that every platform AT (VoiceOver, NVDA, …) runs on
    /// activation. Panics on duplicate children, dangling relationship
    /// targets, orphaned nodes, and invalid focus — turning those runtime
    /// crashes into CI failures.
    pub(crate) fn assert_a11y_tree_valid(update: &accesskit::TreeUpdate) {
        accesskit_consumer::Tree::new(update.clone(), false);
    }

    /// Assert that every NodeId referenced in `controls()` or
    /// `described_by()` of any node is present in the tree. This is the
    /// invariant our post-processing pass enforces; having a test here means
    /// a future refactor can't silently drop the pass and regress it.
    pub(crate) fn assert_no_dangling_relationships(update: &accesskit::TreeUpdate) {
        let emitted: std::collections::HashSet<accesskit::NodeId> =
            update.nodes.iter().map(|(id, _)| *id).collect();
        for (parent_id, node) in &update.nodes {
            for &target in node.controls() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has controls() → {target:?} which is absent from the tree"
                );
            }
            for &target in node.described_by() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has described_by() → {target:?} which is absent from the tree"
                );
            }
        }
    }

    /// Return all NodeIds whose role matches `role`.
    pub(crate) fn nodes_with_role(
        update: &accesskit::TreeUpdate,
        role: accesskit::Role,
    ) -> Vec<accesskit::NodeId> {
        update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == role)
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_helpers::*;
    use crate::test_widgets::{FillWidget, StackWidget};

    #[derive(Debug)]
    struct ActionWidget;

    impl Widget for ActionWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name("Save");
            builder.add_action(accesskit::Action::Click);
            builder.add_action(accesskit::Action::Focus);
        }
    }

    #[derive(Debug)]
    struct ClickableWidget;

    impl Widget for ClickableWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name("Click Me");
            builder.add_action(accesskit::Action::Click);
        }
    }

    #[test]
    fn labeled_widget_has_accessibility() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Hello"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        let info = tree.accessibility_node(widget);
        assert_eq!(info.role(), accesskit::Role::Label);
        assert_eq!(info.name(), Some("Hello"));
    }

    #[test]
    fn find_by_label_works() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Save"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert_eq!(tree.find_by_label("Save"), Some(widget));
    }

    #[test]
    fn find_by_role_works() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Text"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert!(tree.find_by_role(accesskit::Role::Label).is_some());
    }

    #[test]
    fn accessibility_node_collects_actions() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(ActionWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        let info = tree.accessibility_node(widget);
        assert_eq!(info.role(), accesskit::Role::Button);
        assert_eq!(info.name(), Some("Save"));
        assert_eq!(info.actions().len(), 2);
        assert!(info.actions().contains(&accesskit::Action::Click));
        assert!(info.actions().contains(&accesskit::Action::Focus));
    }

    #[test]
    fn sync_accessibility_produces_tree_update() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("First"));
        tree.add(FillWidget::new().label("Second"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 3);
        assert_eq!(update.nodes[0].0, accesskit::NodeId(0));
        assert!(update.tree.is_some());
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn sync_accessibility_excludes_dormant_widgets() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Active"));
        let dormant = tree.add(FillWidget::new().label("Dormant"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.set_dormant(dormant);

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 2);
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn sync_accessibility_includes_focus() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().label("Focused"));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        let update = tree.sync_accessibility();
        let expected_focus = crate::accessibility::widget_id_to_node_id(widget);
        assert_eq!(update.focus, expected_focus);
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn sync_accessibility_parent_child_relationship() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 3);

        let parent_node_id = crate::accessibility::widget_id_to_node_id(parent);
        let parent_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == parent_node_id)
            .map(|(_, node)| node)
            .unwrap();

        let child_node_id = crate::accessibility::widget_id_to_node_id(child);
        assert!(parent_node.children().contains(&child_node_id));
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn find_by_action_finds_clickable() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(ClickableWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.find_by_action(accesskit::Action::Click), Some(widget));
        assert_eq!(tree.find_by_action(accesskit::Action::Focus), None);
    }

    #[test]
    fn text_content_returns_accessibility_name() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Hello World"));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(widget), Some("Hello World".to_string()));
    }

    #[test]
    fn text_content_returns_none_without_label() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(widget), None);
    }

    #[test]
    fn descendant_of_disabled_ancestor_reports_disabled() {
        use crate::signal::Signal;

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.enabled_when(parent, Signal::new(false));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(
            tree.accessibility_node(child).is_disabled(),
            "descendant should report disabled when ancestor is disabled"
        );
    }

    #[test]
    fn text_value_returns_accessibility_value() {
        #[derive(Debug)]
        struct ValueWidget;

        impl Widget for ValueWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }

            fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
                builder.set_role(accesskit::Role::Slider);
                builder.set_name("Volume");
                builder.set_value("75%");
            }
        }

        let mut tree = WidgetTree::new();
        let widget = tree.add(ValueWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.text_value(widget), Some("75%".to_string()));
        assert_eq!(tree.text_content(widget), Some("Volume".to_string()));
    }

    #[test]
    fn sync_accessibility_has_no_duplicate_children() {
        // Regression test for the AccessKit "duplicate child" crash (VoiceOver/NVDA).
        // assert_a11y_tree_valid already catches this via the consumer, but the
        // manual check here provides a more actionable failure message.
        let mut tree = WidgetTree::new();
        let grandchild = tree.add(FillWidget::new().label("Grandchild"));
        let child_a = tree.add(StackWidget::new().add_child(grandchild));
        let child_b = tree.add(FillWidget::new().label("Sibling"));
        let _root = tree.add(StackWidget::new().add_child(child_a).add_child(child_b));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();

        let mut all_children: std::collections::HashMap<accesskit::NodeId, accesskit::NodeId> =
            std::collections::HashMap::new();
        for (parent_id, node) in &update.nodes {
            for &child_id in node.children() {
                let prev = all_children.insert(child_id, *parent_id);
                assert!(
                    prev.is_none(),
                    "duplicate child NodeId {child_id:?}: claimed by both {prev:?} and {parent_id:?}"
                );
            }
        }
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn no_dangling_relationships_in_basic_tree() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let _parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();
        assert_no_dangling_relationships(&update);
        assert_a11y_tree_valid(&update);
    }
}
