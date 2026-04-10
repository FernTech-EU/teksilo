//! BuildContext — context available during Widget::build().
//!
//! Provides Signal-based APIs for creating reactive state, registering
//! effects, and adding child widgets during the build lifecycle.

use crate::signal::{ObserverHandle, Signal};
use crate::state::{BindingRegistry, State};
use crate::widget_id::WidgetId;

/// Context available during Widget::build().
pub struct BuildContext<'a> {
    pub(crate) tree: &'a mut crate::widget_tree::WidgetTree,
    pub(crate) composite_id: Option<WidgetId>,
    /// RAII handles for effects registered during this build cycle.
    /// Transferred to the arena node's `effect_handles` after build returns.
    pub(crate) effect_handles: Vec<ObserverHandle>,
}

impl<'a> BuildContext<'a> {
    /// The WidgetId of the widget being built.
    pub fn self_id(&self) -> WidgetId {
        self.composite_id
            .expect("self_id() called outside of build()")
    }

    /// Add a widget to the tree.
    pub fn add(&mut self, widget: impl crate::widget::Widget + 'static) -> WidgetId {
        self.tree.add(widget)
    }

    /// Add a pre-boxed widget to the tree.
    pub fn add_boxed(&mut self, widget: Box<dyn crate::widget::Widget>) -> WidgetId {
        self.tree.add_boxed(widget)
    }

    /// Add a Level 2 widget as a child of another widget.
    pub fn add_child(
        &mut self,
        parent: WidgetId,
        widget: impl crate::widget::Widget + 'static,
    ) -> WidgetId {
        self.tree.add_child(parent, widget)
    }

    // --- V2: Signal-based APIs ---

    /// Create a new mutable signal.
    pub fn signal<T: 'static>(&mut self, value: T) -> Signal<T> {
        Signal::new(value)
    }

    /// Create a new `Signal<f32>` that supports `animate_to()`.
    /// Registered with the animation scheduler automatically.
    pub fn animated_signal(&mut self, value: f32) -> Signal<f32> {
        let signal = Signal::new_animated(value);
        self.tree.register_animated_signal(&signal);
        signal
    }

    /// Register a pre-existing `Signal<f32>` for animation support.
    /// Use this when the signal was created outside of `build()` (e.g. in the
    /// widget constructor) and needs to be registered with the animation scheduler.
    pub fn register_animated_signal(&mut self, signal: &Signal<f32>) {
        self.tree.register_animated_signal(signal);
    }

    /// Register a scoped effect tied to this build cycle.
    /// The effect fires whenever the signal changes. It is automatically
    /// cleaned up on rebuild or widget destruction.
    pub fn effect<T: Clone + 'static>(&mut self, signal: &Signal<T>, f: impl Fn(&T) + 'static) {
        let handle = signal.observe(f);
        self.effect_handles.push(handle);
    }

    // --- V1: Legacy State-based APIs ---

    /// Create a new reactive state value. (V1 API — prefer `signal()`)
    pub fn state<T: 'static>(&mut self, value: T) -> State<T> {
        State::new(value)
    }

    /// Create a new `State<f32>` that supports `set_animated()`. (V1 API — prefer `animated_signal()`)
    pub fn animated_state(&mut self, value: f32) -> State<f32> {
        let state = State::new_animated(value);
        self.tree.register_animated_state(&state);
        state
    }

    /// Get the binding registry.
    pub fn binding_registry(&self) -> &BindingRegistry {
        self.tree.binding_registry()
    }

    /// Get the current theme.
    pub fn theme(&self) -> &fern_tokens::Theme {
        self.tree.theme()
    }

    /// Bind a widget's visibility to a boolean state or derived state.
    pub fn visible_when(&mut self, id: WidgetId, state: impl Into<crate::state::Reactive<bool>>) {
        self.tree.visible_when(id, state);
    }

    /// Bind a widget's enabled state to a boolean state or derived state.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::state::Reactive<bool>>) {
        self.tree.enabled_when(id, state);
    }

    /// Attach a tooltip to a widget.
    pub fn attach_tooltip(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
    ) {
        self.tree.attach_tooltip(anchor_id, content_id, delay);
    }

    /// Set a widget as dormant (inactive). Used to pre-create overlay content
    /// that will be activated later via `EventContext::activate()`.
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.tree.set_dormant(id);
    }

    /// Look up the shortcut label for a command (type-erased).
    /// Returns the display string (e.g. "Ctrl+S") if a shortcut is bound to this command
    /// in the tree's `ShortcutMap`. Used by `MenuItem` for automatic shortcut labels.
    pub fn shortcut_label_for_any(&self, command: &dyn std::any::Any) -> Option<String> {
        self.tree.shortcut_label_for_any(command)
    }

    /// Apply a `HandlerSet` to the composite widget being built (self).
    /// This transfers attached event handlers, focusable flag, cursor, etc.
    /// to the widget's arena node, replacing `event()` and `is_focusable()` overrides.
    pub fn apply_self_handlers(&mut self, handler_set: crate::widget_builder::HandlerSet) {
        let id = self.self_id();
        self.tree.apply_handler_set(id, handler_set);
    }
}
