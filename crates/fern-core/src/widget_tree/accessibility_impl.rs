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

        let update = self.build_accessibility_tree();
        self.cached_a11y = Some(update.clone());
        self.a11y_dirty = false;
        update
    }

    fn build_accessibility_tree(&self) -> accesskit::TreeUpdate {
        use crate::accessibility::{root_node_id, widget_id_to_node_id};

        let roots = self.arena.roots();
        let mut nodes: Vec<(accesskit::NodeId, accesskit::Node)> = Vec::new();

        let mut root = accesskit::Node::new(accesskit::Role::Window);
        for &root_id in &roots {
            if self.arena.is_active(root_id) {
                root.push_child(widget_id_to_node_id(root_id));
            }
        }
        nodes.push((root_node_id(), root));

        for &root_id in &roots {
            self.build_accessibility_recursive(root_id, &mut nodes);
        }

        let focus = self
            .focused
            .filter(|id| self.arena.is_active(*id))
            .map(widget_id_to_node_id)
            .unwrap_or_else(root_node_id);

        accesskit::TreeUpdate {
            nodes,
            tree: Some(accesskit::Tree::new(root_node_id())),
            tree_id: accesskit::TreeId::ROOT,
            focus,
        }
    }

    fn build_accessibility_recursive(
        &self,
        id: WidgetId,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
    ) {
        use crate::accessibility::widget_id_to_node_id;

        if !self.arena.is_active(id) {
            return;
        }

        let node = self.arena.get(id).unwrap();
        let mut builder = AccessNodeBuilder::new();
        node.widget.accessibility(&mut builder);

        let children = self.arena.children(id);
        for &child_id in children {
            if self.arena.is_active(child_id) {
                builder
                    .inner_mut()
                    .push_child(widget_id_to_node_id(child_id));
            }
        }

        let bounds = self.arena.bounds(id);
        builder.inner_mut().set_bounds(accesskit::Rect {
            x0: bounds.x as f64,
            y0: bounds.y as f64,
            x1: (bounds.x + bounds.width) as f64,
            y1: (bounds.y + bounds.height) as f64,
        });

        if let Some(tooltip) = self
            .tooltips
            .iter()
            .find(|t| t.anchor_id == id && t.overlay_id.is_some())
        {
            builder
                .inner_mut()
                .push_described_by(widget_id_to_node_id(tooltip.content_id));
        }

        let (node_id, ak_node) = builder.build(id);
        nodes.push((node_id, ak_node));

        for &child_id in children {
            self.build_accessibility_recursive(child_id, nodes);
        }
    }

    pub fn accessibility_node(&self, id: WidgetId) -> AccessibilityInfo {
        let node = self.arena.get(id).unwrap();
        let mut builder = AccessNodeBuilder::new();
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
        let mut builder = AccessNodeBuilder::new();
        node.widget.accessibility(&mut builder);
        builder.name().map(|s| s.to_string())
    }

    /// Get the text value of a widget from its accessibility value.
    /// Equivalent to the value set via `AccessNodeBuilder::set_value`.
    pub fn text_value(&self, id: WidgetId) -> Option<String> {
        let node = self.arena.get(id)?;
        let mut builder = AccessNodeBuilder::new();
        node.widget.accessibility(&mut builder);
        builder.value().map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, StackWidget};

    #[derive(Debug)]
    struct ActionWidget;

    impl Widget for ActionWidget {
        fn size_that_fits(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_canvas::Size {
            proposal.resolve(0.0, 0.0)
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
        fn size_that_fits(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_canvas::Size {
            proposal.resolve(0.0, 0.0)
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
    fn text_value_returns_accessibility_value() {
        #[derive(Debug)]
        struct ValueWidget;

        impl Widget for ValueWidget {
            fn size_that_fits(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> fern_canvas::Size {
                proposal.resolve(0.0, 0.0)
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
}