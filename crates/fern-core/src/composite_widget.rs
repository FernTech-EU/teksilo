use crate::accessibility::AccessNodeBuilder;
use crate::event::{EventResponse, WidgetEvent};
use crate::focus::FocusPolicy;
use crate::state::{BindingRegistry, State};
use crate::widget::EventContext;
use crate::widget_id::WidgetId;

/// Context available during CompositeWidget::build().
pub struct BuildContext<'a> {
    pub(crate) tree: &'a mut crate::widget_tree::WidgetTree,
}

impl<'a> BuildContext<'a> {
    /// Add a widget to the tree and return its ID.
    /// Automatically registers any reactive property bindings.
    pub fn add(&mut self, widget: impl crate::widget::Widget + 'static) -> WidgetId {
        let id = self.tree.add(widget);
        // Auto-register reactive bindings now that the widget has an ID
        if let Some(node) = self.tree.arena_get(id) {
            node.widget.register_bindings(id, self.tree.binding_registry());
        }
        id
    }

    /// Add a nested composite widget and return its adapter ID.
    pub fn add_composite(
        &mut self,
        composite: impl CompositeWidget + 'static,
    ) -> WidgetId {
        self.tree.add_composite_inner(Box::new(composite))
    }

    /// Add any widget (Level 1 or Level 2) via the unified `IntoWidgetTree` trait.
    pub fn add_widget(&mut self, widget: impl crate::widget::IntoWidgetTree) -> WidgetId {
        self.tree.add_widget(widget)
    }

    /// Add a widget as a child of another widget.
    /// Automatically registers any reactive property bindings.
    pub fn add_child(
        &mut self,
        parent: WidgetId,
        widget: impl crate::widget::Widget + 'static,
    ) -> WidgetId {
        let id = self.tree.add_child(parent, widget);
        if let Some(node) = self.tree.arena_get(id) {
            node.widget.register_bindings(id, self.tree.binding_registry());
        }
        id
    }

    /// Create a new reactive state value.
    pub fn state<T: 'static>(&mut self, value: T) -> State<T> {
        State::new(value)
    }

    /// Observe a state value: the callback is called whenever the value changes.
    /// This is for notifying the application layer, not for widget bindings
    /// (use `.bind_to()` or `.visible_when()` for those).
    pub fn observe<T: 'static>(
        &self,
        state: &State<T>,
        callback: impl Fn(&T) + 'static,
    ) {
        state.observe(callback);
    }

    /// Get the binding registry for registering State→Widget bindings.
    pub fn binding_registry(&self) -> &BindingRegistry {
        self.tree.binding_registry()
    }

    /// Get the current theme (for resolving colors during build).
    pub fn theme(&self) -> &fern_tokens::Theme {
        self.tree.theme()
    }

    /// Attach a tooltip to a widget. The content widget should have been
    /// added to the tree already (it will be set dormant until shown).
    pub fn attach_tooltip(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
    ) {
        self.tree.attach_tooltip(anchor_id, content_id, delay);
    }
}

/// Trait for Level 1 (composition) widgets.
/// A composite widget describes what it is made of by implementing build().
pub trait CompositeWidget: std::fmt::Debug {
    /// Construct the initial subtree. Pure — no side effects.
    fn build(&self, ctx: &mut BuildContext) -> WidgetId;

    /// Handle events for the composite as a unit.
    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    /// Focus behavior for the composite.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Default
    }

    /// Whether the composite as a whole can receive keyboard focus.
    fn is_focusable(&self) -> bool {
        false
    }

    /// Accessibility identity for the composite root node.
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}
