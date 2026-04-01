use crate::accessibility::AccessNodeBuilder;
use crate::event::{EventResponse, WidgetEvent};
use crate::focus::FocusPolicy;
use crate::state::{BindingRegistry, State};
use crate::widget::EventContext;
use crate::widget_id::WidgetId;

/// Context available during CompositeWidget::build().
pub struct BuildContext<'a> {
    pub(crate) tree: &'a mut crate::widget_tree::WidgetTree,
    pub(crate) composite_id: Option<WidgetId>,
}

impl<'a> BuildContext<'a> {
    /// The WidgetId of the composite adapter being built.
    /// Available during `build()` so the composite can reference itself
    /// (e.g. for tooltip anchoring, clips_children, or self-referencing bindings).
    pub fn self_id(&self) -> Option<WidgetId> {
        self.composite_id
    }

    /// Add any widget (Level 1 composite or Level 2 direct) to the tree.
    /// Binding registration is handled automatically.
    pub fn add(&mut self, widget: impl crate::widget::IntoWidgetTree) -> WidgetId {
        self.tree.add_widget(widget)
    }

    /// Add a Level 2 widget as a child of another widget.
    pub fn add_child(
        &mut self,
        parent: WidgetId,
        widget: impl crate::widget::Widget + 'static,
    ) -> WidgetId {
        self.tree.add_child(parent, widget)
    }

    /// Create a new reactive state value.
    pub fn state<T: 'static>(&mut self, value: T) -> State<T> {
        State::new(value)
    }

    /// Observe a state value: the callback is called whenever the value changes.
    /// This is for notifying the application layer, not for widget bindings
    /// (use `.bind_to()` or `.visible_when()` for those).
    pub fn observe<T: 'static>(
        &mut self,
        state: &State<T>,
        callback: impl Fn(&T) + 'static,
    ) {
        let observer_id = state.observe(callback);
        // Register cleanup so the observer is removed when the composite rebuilds
        if let Some(composite_id) = self.composite_id {
            let state_clone = state.clone();
            self.tree.register_observer_cleanup(
                composite_id,
                Box::new(move || state_clone.remove_observer(observer_id)),
            );
        }
    }

    /// Get the binding registry for registering State→Widget bindings.
    pub fn binding_registry(&self) -> &BindingRegistry {
        self.tree.binding_registry()
    }

    /// Get the current theme (for resolving colors during build).
    pub fn theme(&self) -> &fern_tokens::Theme {
        self.tree.theme()
    }

    /// Bind a widget's visibility to a boolean state.
    pub fn visible_when(&mut self, id: WidgetId, state: &State<bool>) {
        self.tree.visible_when(id, state);
    }

    /// Bind a widget's enabled state to a boolean state.
    pub fn enabled_when(&mut self, id: WidgetId, state: &State<bool>) {
        self.tree.enabled_when(id, state);
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
