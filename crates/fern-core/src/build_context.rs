//! BuildContext — context available during Widget::build().
//!
//! Provides Signal-based APIs for creating reactive state, registering
//! effects, and adding child widgets during the build lifecycle.

use crate::event_source::{SubscriptionHandle, SubscriptionId};
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
    /// Backend-event subscription handles registered during this build
    /// cycle via `subscribe_event`. Transferred to the arena node's
    /// `subscription_handles` after build returns.
    pub(crate) subscription_handles: Vec<(SubscriptionId, SubscriptionHandle)>,
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

    // --- Signal APIs ---

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

    /// Register a pre-existing observer handle for lifecycle management.
    /// The handle will be dropped (and the observer removed) on rebuild
    /// or widget destruction.
    pub fn own_handle(&mut self, handle: ObserverHandle) {
        self.effect_handles.push(handle);
    }

    // --- Compatibility State APIs ---

    /// Create a new reactive state value. Prefer `signal()` for new code.
    pub fn state<T: 'static>(&mut self, value: T) -> State<T> {
        State::new(value)
    }

    /// Create a new `State<f32>` that supports `set_animated()`.
    /// Prefer `animated_signal()` for new code.
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

    /// Retrieve an application-scoped value of type `T` registered via
    /// `FernAppBuilder::app_state` (architecture §9.5). Returns `None` if
    /// no value of that type was registered. The returned reference
    /// borrows from the framework for the duration of the build pass.
    pub fn app_state<T: 'static>(&self) -> Option<&T> {
        self.tree.app_context().app_state::<T>()
    }

    /// Bind a widget's visibility to a boolean prop or compatibility state binding.
    pub fn visible_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        self.tree.visible_when(id, state);
    }

    /// Bind a widget's enabled state to a boolean prop or compatibility state binding.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
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

    /// Apply a `HandlerSet` to a child widget created during this build.
    /// Use this to attach event handlers to children without wrapping them
    /// in `WidgetWithHandlers`.
    pub fn apply_handlers(
        &mut self,
        id: crate::widget_id::WidgetId,
        handler_set: crate::widget_builder::HandlerSet,
    ) {
        self.tree.apply_handler_set(id, handler_set);
    }

    /// Subscribe to events from the registered application event source
    /// (architecture §9.4). The callback runs on the UI thread when the
    /// source publishes an event with a matching origin.
    ///
    /// The subscription is scoped to the current widget's lifetime: when
    /// the widget is rebuilt or destroyed, the framework drops the source
    /// handle (unregistering from the source) and removes the UI-side
    /// callback.
    ///
    /// # Panics
    ///
    /// Panics if no event source has been registered on the
    /// `FernAppBuilder`. In debug builds, also asserts that the `Origin`
    /// and `Event` types match the registered source.
    pub fn subscribe_event<O, E, F>(&mut self, origin: O, callback: F)
    where
        O: 'static,
        E: 'static,
        F: Fn(&E) + 'static,
    {
        use std::any::{Any, TypeId};
        use std::sync::Arc;

        let app_context = self.tree.app_context.clone();

        let adapter = app_context.event_source.as_ref().expect(
            "BuildContext::subscribe_event called but no event source was registered \
             on FernAppBuilder. Call .event_source(source) on the builder first.",
        );

        debug_assert_eq!(
            adapter.origin_type,
            TypeId::of::<O>(),
            "subscribe_event origin type mismatch: source uses {}, subscribe call used {}",
            adapter.origin_type_name,
            std::any::type_name::<O>(),
        );
        debug_assert_eq!(
            adapter.event_type,
            TypeId::of::<E>(),
            "subscribe_event event type mismatch: source uses {}, subscribe call used {}",
            adapter.event_type_name,
            std::any::type_name::<E>(),
        );

        let sub_id = app_context.allocate_subscription_id();

        // The UI-side callback that runs after an event posted from the
        // source thread is delivered back to the UI thread. It downcasts
        // the type-erased payload back to `&E` and invokes the user's `F`.
        let stored_callback: Box<dyn Fn(&dyn Any)> = Box::new(move |event_any| {
            let event = event_any
                .downcast_ref::<E>()
                .expect("subscription event downcast failed — framework bug");
            callback(event);
        });
        app_context
            .subscription_callbacks
            .borrow_mut()
            .insert(sub_id, stored_callback);

        // Build the wrapper that the source will invoke from its
        // publisher thread. It carries only the sub_id (Copy) and an
        // Arc-clone of the poster (Send + Sync), boxes the typed event
        // as Any+Send, and posts an AppEvent::SubscriptionEvent through
        // the proxy. Tests that run without a registered poster post
        // events into a test queue and dispatch them back into the tree
        // via `tree.app_context().dispatch_subscription_event`.
        let poster = app_context
            .poster
            .as_ref()
            .expect(
                "BuildContext::subscribe_event called but no AppEventPoster \
                 is installed on the tree. fern-app installs one when an \
                 event source is registered on the builder; tests must \
                 supply a TestPoster via TreeAppContext::with_source_and_poster.",
            )
            .clone();
        let wrapper: Arc<dyn Fn(Box<dyn Any + Send>) + Send + Sync> =
            Arc::new(move |erased_event| {
                poster.post_subscription_event(sub_id, erased_event);
            });

        let handle = (adapter.subscribe_fn)(Box::new(origin), wrapper);
        self.subscription_handles.push((sub_id, handle));
    }
}
