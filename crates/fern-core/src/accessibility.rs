use accesskit::{Action, Live, Node, NodeId, Role};

use crate::widget_id::WidgetId;

/// Builder wrapper around accesskit::Node for widget accessibility declarations.
pub struct AccessNodeBuilder {
    inner: Node,
    name: Option<String>,
    value: Option<String>,
    role: Role,
    actions: Vec<Action>,
}

impl AccessNodeBuilder {
    pub fn new() -> Self {
        Self {
            inner: Node::new(Role::Unknown),
            name: None,
            value: None,
            role: Role::Unknown,
            actions: Vec::new(),
        }
    }

    pub fn set_role(&mut self, role: Role) {
        self.role = role;
        self.inner.set_role(role);
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        let name: String = name.into();
        self.inner.set_label(name.clone());
        self.name = Some(name);
    }

    pub fn set_disabled(&mut self) {
        self.inner.set_disabled();
    }

    pub fn add_action(&mut self, action: Action) {
        self.inner.add_action(action);
        self.actions.push(action);
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        let v: String = value.into();
        self.inner.set_value(v.clone());
        self.value = Some(v);
    }

    pub fn set_description(&mut self, description: impl Into<String>) {
        self.inner.set_description(description.into());
    }

    pub fn set_live(&mut self, live: Live) {
        self.inner.set_live(live);
    }

    pub fn set_described_by(&mut self, ids: impl Into<Vec<NodeId>>) {
        self.inner.set_described_by(ids);
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Build the AccessKit Node with the given ID.
    pub fn build(self, id: WidgetId) -> (NodeId, Node) {
        let node_id = widget_id_to_node_id(id);
        (node_id, self.inner)
    }

    /// Get a reference to the inner node for advanced use.
    pub fn inner_mut(&mut self) -> &mut Node {
        &mut self.inner
    }
}

impl Default for AccessNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a WidgetId to an AccessKit NodeId.
pub fn widget_id_to_node_id(id: WidgetId) -> NodeId {
    use slotmap::Key;
    let key_data = id.data();
    let raw = key_data.as_ffi();
    NodeId(raw)
}

/// Convert an AccessKit NodeId back to a WidgetId.
pub fn node_id_to_widget_id(node_id: NodeId) -> WidgetId {
    use slotmap::KeyData;
    let key_data = KeyData::from_ffi(node_id.0);
    key_data.into()
}

/// The special root node ID for the accessibility tree.
pub fn root_node_id() -> NodeId {
    NodeId(0)
}

/// Query result for accessibility information about a widget.
#[derive(Debug)]
pub struct AccessibilityInfo {
    role: Role,
    name: Option<String>,
    actions: Vec<Action>,
}

impl AccessibilityInfo {
    pub fn new(role: Role, name: Option<String>, actions: Vec<Action>) -> Self {
        Self {
            role,
            name,
            actions,
        }
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn actions(&self) -> &[Action] {
        &self.actions
    }
}
