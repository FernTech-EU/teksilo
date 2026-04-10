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