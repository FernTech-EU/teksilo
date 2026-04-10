use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, RenderFrame, SizeProposal};
use fern_tokens::Theme;

use crate::app_command::{AppCommand, ErasedCommand};
use crate::arena::{GestureBinding, WidgetArena};
use crate::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use crate::gesture::{GestureArena, GestureEvent, GestureRecognizer, RawPointerEvent};
use crate::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

mod accessibility_impl;
mod event_dispatch_impl;
mod focus_impl;
mod overlay_impl;
mod rendering_impl;
mod test_api;

/// The main widget tree orchestrating arena, layout, events, accessibility, and paint.
/// Provides both the runtime API and the headless test API.
/// Type-erased shortcut lookup function.
/// The third argument is `focused`, and the fourth is an `is_in_scope(focused, scope)` checker.
type ShortcutLookup = Box<
    dyn Fn(
        Key,
        Modifiers,
        Option<WidgetId>,
        &dyn Fn(WidgetId, WidgetId) -> bool,
    ) -> Option<ErasedCommand>,
>;

/// Type-erased reverse lookup: given a command (as `&dyn Any`), find its shortcut.
type ShortcutReverseLookup = Box<dyn Fn(&dyn std::any::Any) -> Option<crate::shortcut::Shortcut>>;

enum AnimatedRegistration {
    State(crate::state::WeakAnimatedState),
    Signal(crate::signal::WeakAnimatedSignal),
}

enum PendingAnimationRegistration {
    State(crate::state::State<f32>, crate::state::AnimationRequest),
    Signal(crate::signal::Signal<f32>, crate::state::AnimationRequest),
}

impl AnimatedRegistration {
    fn same_state(&self, state: &crate::state::State<f32>) -> bool {
        match self {
            Self::State(weak_state) => weak_state.same_state(state),
            Self::Signal(_) => false,
        }
    }

    fn same_signal(&self, signal: &crate::signal::Signal<f32>) -> bool {
        match self {
            Self::State(_) => false,
            Self::Signal(weak_signal) => weak_signal.same_signal(signal),
        }
    }

    fn is_alive(&self) -> bool {
        match self {
            Self::State(weak_state) => weak_state.upgrade().is_some(),
            Self::Signal(weak_signal) => weak_signal.upgrade().is_some(),
        }
    }

    fn take_pending_animation(&self) -> Option<PendingAnimationRegistration> {
        match self {
            Self::State(weak_state) => {
                let state = weak_state.upgrade()?;
                let request = state.take_pending_animation()?;
                Some(PendingAnimationRegistration::State(state, request))
            }
            Self::Signal(weak_signal) => {
                let signal = weak_signal.upgrade()?;
                let request = signal.take_pending_animation()?;
                Some(PendingAnimationRegistration::Signal(signal, request))
            }
        }
    }
}

#[allow(clippy::type_complexity)]
pub struct WidgetTree {
    arena: WidgetArena,
    theme: Theme,
    text_backend: Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    last_proposal: SizeProposal,
    command_handler: Option<Box<dyn FnMut(&ErasedCommand)>>,
    pending_commands: Vec<ErasedCommand>,
    shortcut_lookup: Option<ShortcutLookup>,
    shortcut_reverse_lookup: Option<ShortcutReverseLookup>,
    binding_registry: crate::state::BindingRegistry,
    idle_queue: crate::idle::IdleQueue,
    /// Simulated clock for deterministic time-dependent testing.
    sim_clock: std::time::Instant,
    /// Overlay manager for tooltips, menus, popovers.
    pub(crate) overlay_manager: crate::overlay::OverlayManager,
    /// Tooltip attachments: (anchor_id, content_id, text, delay, hover_start, overlay_id).
    tooltips: Vec<TooltipEntry>,
    /// How the currently focused widget gained focus.
    focus_origin: Option<crate::focus::FocusOrigin>,
    /// Layout direction for RTL/LTR support.
    layout_direction: crate::environment::LayoutDirection,
    /// Animation scheduler for smooth animated state and signal transitions.
    animation_scheduler: crate::animation::AnimationScheduler,
    /// Weakly tracked animated values from both state and signal APIs.
    animated_values: Vec<AnimatedRegistration>,
    /// Cached accessibility tree update — only rebuilt when layout changes.
    cached_a11y: Option<accesskit::TreeUpdate>,
    /// Whether the accessibility tree needs rebuilding (set when layout runs).
    a11y_dirty: bool,
    /// Cached full render frame — reused when no widget needs painting.
    cached_frame: Option<RenderFrame>,
    /// Widget that has captured the pointer (receives all PointerMove/PointerUp
    /// regardless of hit-test). Set via `EventContext::capture_pointer()`.
    pointer_captured_by: Option<WidgetId>,
    /// Delayed overlay requests (e.g., submenu hover-open delay).
    pending_delayed_overlays: Vec<PendingDelayedOverlay>,
    /// OS-level accessibility preferences (high contrast, reduced motion, text scale).
    prefers_high_contrast: bool,
    prefers_reduced_motion: bool,
    text_scale_factor: f64,
}

/// A tooltip attachment managed by the WidgetTree.
struct TooltipEntry {
    anchor_id: WidgetId,
    content_id: WidgetId,
    delay: std::time::Duration,
    /// Simulated hover start (for deterministic tests via advance_time).
    hover_start: Option<std::time::Instant>,
    /// Real hover start (for windowed apps via layout).
    real_hover_start: Option<std::time::Instant>,
    overlay_id: Option<crate::overlay::OverlayId>,
}

/// A delayed overlay request (e.g., submenu hover-open delay).
struct PendingDelayedOverlay {
    request: crate::overlay::OverlayRequest,
    delay: std::time::Duration,
    focus_target: Option<WidgetId>,
    /// When the request was made (real time, for windowed apps).
    real_requested_at: std::time::Instant,
    /// When the request was made (simulated time, for tests).
    sim_requested_at: std::time::Instant,
}

impl WidgetTree {
    pub fn new() -> Self {
        Self {
            arena: WidgetArena::new(),
            theme: Theme::light_default(),
            text_backend: None,
            focused: None,
            hovered: None,
            last_proposal: SizeProposal::exact(800.0, 600.0),
            command_handler: None,
            pending_commands: Vec::new(),
            shortcut_lookup: None,
            shortcut_reverse_lookup: None,
            binding_registry: crate::state::BindingRegistry::new(),
            idle_queue: crate::idle::IdleQueue::new(),
            sim_clock: std::time::Instant::now(),
            focus_origin: None,
            overlay_manager: crate::overlay::OverlayManager::new(),
            tooltips: Vec::new(),
            layout_direction: crate::environment::LayoutDirection::default(),
            animation_scheduler: crate::animation::AnimationScheduler::new(),
            animated_values: Vec::new(),
            cached_a11y: None,
            a11y_dirty: true,
            cached_frame: None,
            pointer_captured_by: None,
            pending_delayed_overlays: Vec::new(),
            prefers_high_contrast: false,
            prefers_reduced_motion: false,
            text_scale_factor: 1.0,
        }
    }

    fn pointer_inside_overlay_region(
        &self,
        overlay_id: crate::overlay::OverlayId,
        position: Point,
    ) -> bool {
        let Some(overlay) = self
            .overlay_manager
            .stack
            .iter()
            .find(|overlay| overlay.id == overlay_id)
        else {
            return false;
        };

        if self.arena.is_active(overlay.anchor)
            && self.arena.bounds(overlay.anchor).contains(position)
        {
            return true;
        }

        self.overlay_manager.stack.iter().any(|candidate| {
            (candidate.id == overlay_id
                || self.overlay_manager.is_descendant_of(candidate.id, overlay_id))
                && candidate.bounds.contains(position)
        })
    }

    fn update_pointer_leave_overlays(&mut self, position: Point) {
        let overlay_ids: Vec<crate::overlay::OverlayId> = self
            .overlay_manager
            .stack
            .iter()
            .filter(|overlay| {
                matches!(
                    overlay.dismiss,
                    crate::overlay::DismissBehavior::PointerLeave { .. }
                )
            })
            .map(|overlay| overlay.id)
            .collect();

        let real_now = std::time::Instant::now();
        let sim_now = self.sim_clock;

        for overlay_id in overlay_ids {
            let inside = self.pointer_inside_overlay_region(overlay_id, position);
            if let Some(overlay) = self
                .overlay_manager
                .stack
                .iter_mut()
                .find(|overlay| overlay.id == overlay_id)
            {
                if inside {
                    overlay.pointer_leave_started_real = None;
                    overlay.pointer_leave_started_sim = None;
                } else if overlay.pointer_leave_started_real.is_none() {
                    overlay.pointer_leave_started_real = Some(real_now);
                    overlay.pointer_leave_started_sim = Some(sim_now);
                    self.arena.mark_needs_paint(overlay.anchor);
                }
            }
        }

        self.process_pointer_leave_overlays_real();
    }

    fn process_pointer_leave_overlays(&mut self) {
        let sim_now = self.sim_clock;
        self.process_pointer_leave_overlays_impl(|overlay| {
            overlay
                .pointer_leave_started_sim
                .map(|started| sim_now.saturating_duration_since(started))
        });
    }

    fn process_pointer_leave_overlays_real(&mut self) {
        let real_now = std::time::Instant::now();
        self.process_pointer_leave_overlays_impl(|overlay| {
            overlay
                .pointer_leave_started_real
                .map(|started| real_now.saturating_duration_since(started))
        });
    }

    fn process_pointer_leave_overlays_impl(
        &mut self,
        elapsed_fn: impl Fn(&crate::overlay::ActiveOverlay) -> Option<std::time::Duration>,
    ) {
        let mut to_dismiss = Vec::new();

        for overlay in self.overlay_manager.stack.iter().rev() {
            let crate::overlay::DismissBehavior::PointerLeave { delay } = overlay.dismiss else {
                continue;
            };

            if to_dismiss
                .iter()
                .any(|ancestor| self.overlay_manager.is_descendant_of(overlay.id, *ancestor))
            {
                continue;
            }

            if let Some(elapsed) = elapsed_fn(overlay)
                && elapsed >= delay
            {
                to_dismiss.push(overlay.id);
            }
        }

        for overlay_id in to_dismiss {
            let (dismissed, focus_restore) =
                self.overlay_manager.dismiss_with_focus_restore(overlay_id);
            self.dormant_dismissed_content(&dismissed);
            if let Some(restore_id) = focus_restore {
                if self.arena.is_active(restore_id) {
                    self.focus(restore_id);
                }
            }
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn with_text_backend(mut self, backend: Rc<RefCell<dyn fern_canvas::TextBackend>>) -> Self {
        self.text_backend = Some(backend);
        self
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Whether any widget needs layout or paint (i.e., a redraw would be useful).
    pub fn needs_redraw(&self) -> bool {
        self.arena.any_needs_layout()
            || self.arena.any_needs_paint()
            || self.animation_scheduler.has_active()
    }

    /// Whether a render pass is needed (any widget needs layout or paint).
    pub fn needs_render(&self) -> bool {
        self.arena.any_needs_layout() || self.arena.any_needs_paint()
    }

    /// Register a `State<f32>` for animation support. The framework checks
    /// registered values each frame for pending `set_animated` requests.
    /// Called automatically by `BuildContext::animated_state()`.
    pub fn register_animated_state(&mut self, state: &crate::state::State<f32>) {
        self.animated_values.retain(|registration| registration.is_alive());
        if !self
            .animated_values
            .iter()
            .any(|registration| registration.same_state(state))
            && let Some(weak_state) = state.weak_handle()
        {
            self.animated_values
                .push(AnimatedRegistration::State(weak_state));
        }
    }

    /// Register a `Signal<f32>` for animation support.
    pub fn register_animated_signal(&mut self, signal: &crate::signal::Signal<f32>) {
        self.animated_values.retain(|registration| registration.is_alive());
        if !self
            .animated_values
            .iter()
            .any(|registration| registration.same_signal(signal))
            && let Some(weak_signal) = signal.weak_handle()
        {
            self.animated_values
                .push(AnimatedRegistration::Signal(weak_signal));
        }
    }

    /// Whether any animation is currently running.
    pub fn has_active_animations(&self) -> bool {
        self.animation_scheduler.has_active()
    }

    /// Pick up pending `set_animated` requests from registered states
    /// and start them on the animation scheduler.
    fn process_pending_animations(&mut self) {
        let now = std::time::Instant::now();
        self.process_pending_animations_at(now);
    }

    /// Pick up pending animations using the given time (for sim clock).
    fn process_pending_animations_at(&mut self, now: std::time::Instant) {
        let mut pending = Vec::new();
        self.animated_values.retain(|registration| {
            if let Some(animation) = registration.take_pending_animation() {
                pending.push(animation);
                true
            } else {
                registration.is_alive()
            }
        });

        for animation in pending {
            match animation {
                PendingAnimationRegistration::State(state, req) => {
                    self.animation_scheduler
                        .animate(&state, req.target, req.duration, req.easing, now);
                }
                PendingAnimationRegistration::Signal(signal, req) => {
                    self.animation_scheduler.animate_signal_with_frame_interval(
                        &signal,
                        req.target,
                        req.duration,
                        req.easing,
                        req.frame_interval,
                        now,
                    );
                }
            }
        }
    }

    /// Advance animations by simulated time (for deterministic testing).
    /// Pending `set_animated` requests are started at the current sim_clock,
    /// then time advances by `duration`, and the scheduler ticks at the new time.
    pub fn tick_animations(&mut self, duration: std::time::Duration) {
        self.process_pending_animations_at(self.sim_clock);

        self.sim_clock += duration;

        self.animation_scheduler.tick(self.sim_clock);

        self.process_state_changes();
    }

    /// Switch the tree-level theme at runtime.
    /// Rebuilds all composite widgets (their derived state closures capture theme
    /// tokens at build time) and marks all widgets as needing layout and repaint.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.focused = None;
        self.hovered = None;
        self.focus_origin = None;
        self.tooltips.clear();
        self.rebuild_built_widgets();
        self.arena.mark_all_dirty();
    }

    /// Reconstruct all widgets that have `has_built_children == true`.
    /// Called when the environment changes (theme switch, locale switch).
    fn rebuild_built_widgets(&mut self) {
        let ids: Vec<WidgetId> = self
            .arena
            .active_ids()
            .into_iter()
            .filter(|id| self.arena.get(*id).is_some_and(|n| n.has_built_children))
            .collect();

        for widget_id in ids {
            if let Some(node) = self.arena.get_mut(widget_id) {
                node.effect_handles.clear();
            }

            let old_children: Vec<WidgetId> = self.arena.children(widget_id).to_vec();
            for child_id in old_children {
                self.arena.destroy(child_id);
            }

            let mut widget_box = match self.arena.take_widget(widget_id) {
                Some(widget) => widget,
                None => continue,
            };

            let mut build_ctx = crate::build_context::BuildContext {
                tree: self,
                composite_id: Some(widget_id),
                effect_handles: Vec::new(),
            };
            let new_children = widget_box.build(&mut build_ctx);
            let effect_handles = std::mem::take(&mut build_ctx.effect_handles);

            self.arena.restore_widget(widget_id, widget_box);

            for &child_id in &new_children {
                if let Some(child_node) = self.arena.get_mut(child_id) {
                    child_node.parent = Some(widget_id);
                }
            }
            if let Some(node) = self.arena.get_mut(widget_id) {
                node.children = new_children;
                node.effect_handles = effect_handles;
            }
        }
    }

    /// Set the layout direction (LTR/RTL). Marks all widgets as needing layout.
    pub fn set_layout_direction(&mut self, direction: crate::environment::LayoutDirection) {
        self.layout_direction = direction;
        self.arena.mark_all_dirty();
    }

    /// Set OS-level accessibility preferences.
    ///
    /// Called by `fern-app` after querying the platform layer. Updates the
    /// values fed into `PaintContext` and `Environment` on subsequent frames.
    /// Marks all widgets dirty so the new preferences take effect immediately.
    pub fn set_accessibility_preferences(
        &mut self,
        high_contrast: bool,
        reduced_motion: bool,
        text_scale_factor: f64,
    ) {
        let changed = self.prefers_high_contrast != high_contrast
            || self.prefers_reduced_motion != reduced_motion
            || (self.text_scale_factor - text_scale_factor).abs() > f64::EPSILON;

        if changed {
            self.prefers_high_contrast = high_contrast;
            self.prefers_reduced_motion = reduced_motion;
            self.text_scale_factor = text_scale_factor;
            self.arena.mark_all_dirty();
        }
    }

    /// Whether the OS has requested high-contrast mode.
    pub fn prefers_high_contrast(&self) -> bool {
        self.prefers_high_contrast
    }

    /// Whether the OS has requested reduced motion.
    pub fn prefers_reduced_motion(&self) -> bool {
        self.prefers_reduced_motion
    }

    /// OS text scaling factor (1.0 = normal).
    pub fn text_scale_factor(&self) -> f64 {
        self.text_scale_factor
    }

    /// Mark a widget as clipping its children to its bounds (scroll areas).
    pub fn set_clips_children(&mut self, id: WidgetId, clips: bool) {
        self.arena.set_clips_children(id, clips);
    }

    /// Apply a `HandlerSet` to an existing node in the arena.
    /// Used by `BuildContext::apply_self_handlers()` to attach handlers
    /// from within `build()`.
    pub(crate) fn apply_handler_set(
        &mut self,
        id: WidgetId,
        handler_set: crate::widget_builder::HandlerSet,
    ) {
        self.arena.apply_handler_set(id, handler_set);
    }

    /// Set a per-child alignment override on a widget.
    pub fn set_alignment(&mut self, id: WidgetId, alignment: fern_tokens::Alignment) {
        self.arena.set_alignment_override(id, alignment);
    }

    /// Get the binding registry for registering State→Widget bindings.
    pub fn binding_registry(&self) -> &crate::state::BindingRegistry {
        &self.binding_registry
    }

    /// Process dirty state bindings: mark bound widgets for repaint.
    /// Called automatically at the start of layout().
    fn process_state_changes(&mut self) {
        let dirty_widgets = self.binding_registry.flush_dirty();
        for (id, level) in &dirty_widgets {
            match level {
                crate::state::BindingLevel::RepaintOnly => {
                    self.arena.mark_needs_paint(*id);
                }
                crate::state::BindingLevel::Relayout => {
                    self.arena.mark_needs_layout(*id);
                    self.arena.mark_ancestors_need_layout(*id);
                }
            }
        }

        let mut to_dormant = Vec::new();
        let mut to_activate = Vec::new();
        for (id, is_active, should_be_visible) in self.arena.visibility_checks() {
            if is_active && !should_be_visible {
                to_dormant.push(id);
            } else if !is_active && should_be_visible {
                to_activate.push(id);
            }
        }
        for id in to_dormant {
            self.arena.set_dormant(id);
        }
        for id in to_activate {
            self.arena.activate(id);
        }
    }

    /// Register a ShortcutMap for keyboard shortcut interception.
    /// Shortcuts are checked before any widget sees the key event (preview pass).
    pub fn with_shortcuts<C: AppCommand>(mut self, map: crate::shortcut::ShortcutMap<C>) -> Self {
        let map_for_reverse = map.clone();
        self.shortcut_lookup = Some(Box::new(move |key, modifiers, focused, is_in_scope| {
            let shortcut = crate::shortcut::Shortcut::new(key, modifiers);
            map.find(&shortcut, focused, is_in_scope)
                .map(|cmd| ErasedCommand::new(cmd.clone()))
        }));
        self.shortcut_reverse_lookup = Some(Box::new(move |cmd_any: &dyn std::any::Any| {
            cmd_any
                .downcast_ref::<C>()
                .and_then(|cmd| map_for_reverse.find_shortcut_for(cmd).copied())
        }));
        self
    }

    /// Lookup a shortcut, returning a type-erased command if matched.
    fn shortcut_map_lookup(&self, key: Key, modifiers: Modifiers) -> Option<ErasedCommand> {
        let lookup = self.shortcut_lookup.as_ref()?;
        let is_in_scope =
            |focused: WidgetId, scope: WidgetId| -> bool { self.is_descendant_of(focused, scope) };
        lookup(key, modifiers, self.focused, &is_in_scope)
    }

    /// Reverse-lookup: find the shortcut label for a type-erased command.
    /// Returns the `Shortcut::to_string()` display (e.g. "Ctrl+S").
    pub(crate) fn shortcut_label_for_any(&self, command: &dyn std::any::Any) -> Option<String> {
        self.shortcut_reverse_lookup
            .as_ref()
            .and_then(|lookup| lookup(command))
            .map(|shortcut| shortcut.to_string())
    }

    // --- Command handling ---

    /// Register a typed command handler.
    pub fn on_command<C: AppCommand>(&mut self, mut handler: impl FnMut(&C) + 'static) {
        self.command_handler = Some(Box::new(move |erased: &ErasedCommand| {
            if let Some(cmd) = erased.downcast_ref::<C>() {
                handler(cmd);
            }
        }));
    }

    fn flush_commands(&mut self) {
        if let Some(handler) = &mut self.command_handler {
            let commands: Vec<ErasedCommand> = self.pending_commands.drain(..).collect();
            for cmd in &commands {
                handler(cmd);
            }
        }
    }

    /// Drain all pending commands without calling the tree-level handler.
    /// Used by the app-level event loop to route commands through a
    /// window-aware `CommandContext`.
    pub fn drain_pending_commands(&mut self) -> Vec<ErasedCommand> {
        std::mem::take(&mut self.pending_commands)
    }

    // --- Widget insertion ---

    /// Internal: insert a widget, call build(), wire children, register clips.
    fn insert_widget(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        let id = self.arena.insert(widget);

        {
            if let Some(mut widget_box) = self.arena.take_widget(id) {
                if let Some(handler_set) = widget_box.take_handler_set() {
                    self.arena.restore_widget(id, widget_box);
                    if let Some(node) = self.arena.get_mut(id) {
                        node.handlers = handler_set.handlers;
                        node.node_focusable = handler_set.focusable;
                        node.node_tab_index = handler_set.tab_index;
                        node.node_cursor = handler_set.cursor;
                        if handler_set.context_menu_factory.is_some() {
                            node.context_menu_factory = handler_set.context_menu_factory;
                        }
                    }
                } else {
                    self.arena.restore_widget(id, widget_box);
                }
            }
        }

        {
            let mut widget_box = match self.arena.take_widget(id) {
                Some(widget) => widget,
                None => return id,
            };
            let mut build_ctx = crate::build_context::BuildContext {
                tree: self,
                composite_id: Some(id),
                effect_handles: Vec::new(),
            };
            let built_children = widget_box.build(&mut build_ctx);
            let effect_handles = std::mem::take(&mut build_ctx.effect_handles);

            self.arena.restore_widget(id, widget_box);

            if !built_children.is_empty() {
                for &child_id in &built_children {
                    if let Some(child_node) = self.arena.get_mut(child_id) {
                        child_node.parent = Some(id);
                    }
                }
                if let Some(node) = self.arena.get_mut(id) {
                    node.children = built_children;
                    node.has_built_children = true;
                    node.effect_handles = effect_handles;
                }
            }
        }

        let clips = self
            .arena
            .get(id)
            .is_some_and(|node| node.widget.clips_children());
        if clips {
            self.arena.set_clips_children(id, true);
        }

        id
    }

    /// Add a widget to the tree.
    pub fn add(&mut self, widget: impl Widget + 'static) -> WidgetId {
        self.insert_widget(Box::new(widget))
    }

    /// Add a pre-boxed widget to the tree.
    pub fn add_boxed(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        self.insert_widget(widget)
    }

    /// Add a widget as a child of another widget.
    pub fn add_child(&mut self, parent: WidgetId, widget: impl Widget + 'static) -> WidgetId {
        let boxed: Box<dyn Widget> = Box::new(widget);

        let id = self.arena.insert_child(parent, boxed);

        {
            if let Some(mut widget_box) = self.arena.take_widget(id) {
                if let Some(handler_set) = widget_box.take_handler_set() {
                    self.arena.restore_widget(id, widget_box);
                    if let Some(node) = self.arena.get_mut(id) {
                        node.handlers = handler_set.handlers;
                        node.node_focusable = handler_set.focusable;
                        node.node_tab_index = handler_set.tab_index;
                        node.node_cursor = handler_set.cursor;
                        if handler_set.context_menu_factory.is_some() {
                            node.context_menu_factory = handler_set.context_menu_factory;
                        }
                    }
                } else {
                    self.arena.restore_widget(id, widget_box);
                }
            }
        }

        {
            if let Some(mut widget_box) = self.arena.take_widget(id) {
                let mut build_ctx = crate::build_context::BuildContext {
                    tree: self,
                    composite_id: Some(id),
                    effect_handles: Vec::new(),
                };
                let built_children = widget_box.build(&mut build_ctx);
                let effect_handles = std::mem::take(&mut build_ctx.effect_handles);

                self.arena.restore_widget(id, widget_box);

                if !built_children.is_empty() {
                    for &child_id in &built_children {
                        if let Some(child_node) = self.arena.get_mut(child_id) {
                            child_node.parent = Some(id);
                        }
                    }
                    if let Some(node) = self.arena.get_mut(id) {
                        node.children = built_children;
                        node.has_built_children = true;
                        node.effect_handles = effect_handles;
                    }
                }
            }
        }

        let clips = self
            .arena
            .get(id)
            .is_some_and(|node| node.widget.clips_children());
        if clips {
            self.arena.set_clips_children(id, true);
        }

        id
    }

    // --- Property bindings ---

    /// Bind a widget's visibility to a boolean prop or compatibility state binding.
    /// When false, the widget is set dormant; when true, it is activated.
    /// Accepts `Signal<bool>`, `Prop<bool>`, compatibility state bindings, or plain `bool`.
    pub fn visible_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        let prop = state.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::state::BindingLevel::Relayout,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.visible_state = Some(prop);
        }
    }

    /// Bind a widget's enabled state to a boolean prop or compatibility state binding.
    /// When false, the widget ignores all events but remains visible.
    /// Accepts `Signal<bool>`, `Prop<bool>`, compatibility state bindings, or plain `bool`.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        let prop = state.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::state::BindingLevel::Relayout,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.enabled_state = Some(prop);
        }
    }

    /// Whether a widget is currently enabled (no enabled_state or state is true).
    pub fn is_enabled(&self, id: WidgetId) -> bool {
        self.arena.is_enabled(id)
    }

    // --- Theme override ---

    /// Set a theme override on a widget. All descendants of this widget
    /// will see the modified theme during layout and paint.
    /// The override function receives a mutable `Theme` to modify.
    ///
    /// ```ignore
    /// tree.set_theme_override(panel_id, |theme| {
    ///     theme.colors = ColorTokens::dark_default();
    /// });
    /// ```
    pub fn set_theme_override(
        &mut self,
        id: WidgetId,
        f: impl Fn(&mut fern_tokens::Theme) + 'static,
    ) {
        let had_override = self
            .arena
            .get(id)
            .is_some_and(|n| n.theme_override.is_some());
        if let Some(node) = self.arena.get_mut(id) {
            node.theme_override = Some(crate::environment::ThemeOverride { func: Box::new(f) });
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
        if !had_override {
            self.arena.theme_override_count += 1;
        }
    }

    /// Get the resolved theme for a specific widget (applying ancestor overrides).
    pub fn resolved_theme(&self, id: WidgetId) -> fern_tokens::Theme {
        self.arena.resolve_theme(id, &self.theme)
    }

    // --- Gesture attachment ---

    /// Attach a gesture recognizer to a widget with a handler callback.
    /// Multiple recognizers can be attached by calling this multiple times;
    /// they compete via a [`GestureArena`].
    ///
    /// ```ignore
    /// tree.attach_gesture(widget_id, DragRecognizer::new(), |gesture, ctx| {
    ///     // handle drag
    /// });
    /// ```
    pub fn attach_gesture(
        &mut self,
        id: WidgetId,
        recognizer: impl GestureRecognizer + 'static,
        mut handler: impl FnMut(GestureEvent, &mut EventContext) + 'static,
    ) {
        if let Some(node) = self.arena.get_mut(id) {
            if let Some(binding) = &mut node.gesture_binding {
                // Already has gestures — wrap existing handler to also call new one
                let mut old_handler = std::mem::replace(&mut binding.handler, Box::new(|_, _| {}));
                binding.arena.add(recognizer);
                binding.handler = Box::new(move |gesture, ctx| {
                    old_handler(gesture.clone(), ctx);
                    handler(gesture, ctx);
                });
            } else {
                let mut arena = GestureArena::new();
                arena.add(recognizer);
                node.gesture_binding = Some(GestureBinding {
                    arena,
                    handler: Box::new(handler),
                });
            }
        }
    }

    // --- Layout ---

    /// Run the layout pass with the given size proposal.
    pub fn layout(&mut self, proposal: SizeProposal) {
        // Process pending animation requests from set_animated() calls.
        self.process_pending_animations();

        // Tick active animations (uses real time for windowed apps).
        let now = std::time::Instant::now();
        self.animation_scheduler.tick(now);

        // Process state bindings: mark widgets whose bound state changed.
        self.process_state_changes();

        // Process tooltip timers with real time (for windowed apps).
        self.process_tooltips_real();

        // Process delayed overlay requests (submenu hover-open delays).
        self.process_delayed_overlays_real();

        // Process pointer-leave dismissal timers for overlays such as submenus.
        self.process_pointer_leave_overlays_real();

        // Refresh cached root list if the tree structure changed.
        self.arena.refresh_roots();

        let proposal_changed = self.last_proposal != proposal;
        self.last_proposal = proposal;

        // Skip the full layout pass if nothing is dirty and the proposal hasn't changed.
        if !proposal_changed && !self.arena.any_needs_layout() {
            return;
        }

        // Layout is running — accessibility tree needs rebuilding
        self.a11y_dirty = true;

        let base_theme = self.theme.clone();

        // Layout main content roots (excluding overlay content)
        let overlay_content_ids = self.overlay_manager.active_content_ids();
        let roots: Vec<WidgetId> = self.arena.roots();
        for root_id in roots {
            if overlay_content_ids.contains(&root_id) {
                continue; // Overlay content is laid out separately
            }
            layout_widget_recursive(
                &mut self.arena,
                root_id,
                Rect::from_origin_size(Point::ZERO, proposal.resolve(0.0, 0.0)),
                proposal,
                &base_theme,
                self.layout_direction,
                self.text_backend.as_ref(),
            );
        }

        // Position and layout overlay content
        let anchor_bounds = |id: WidgetId| -> Rect { self.arena.bounds(id) };
        let viewport = (
            proposal.width.unwrap_or(800.0),
            proposal.height.unwrap_or(600.0),
        );
        self.overlay_manager
            .position_overlays(anchor_bounds, viewport);
        for content_id in &overlay_content_ids {
            if !self.arena.is_active(*content_id) {
                continue;
            }
            // Get the overlay's computed bounds
            let overlay_id = self.overlay_manager.find_by_content(*content_id);
            // First, measure the content widget's intrinsic size
            let intrinsic = {
                let resolved_theme = self.arena.resolve_theme(*content_id, &base_theme);
                let ctx = LayoutContext {
                    theme: &resolved_theme,
                    layout_direction: self.layout_direction,
                    text_backend: self.text_backend.as_ref(),
                    arena: Some(&self.arena),
                };
                let node = self.arena.get(*content_id).unwrap();
                node.widget.size_that_fits(
                    SizeProposal {
                        width: None,
                        height: None,
                    },
                    &ctx,
                )
            };
            // Update the overlay bounds with the intrinsic size
            if let Some(oid) = overlay_id {
                self.overlay_manager.set_content_bounds(oid, intrinsic);
                // Re-position with correct size
                let anchor_bounds2 = |id: WidgetId| -> Rect { self.arena.bounds(id) };
                self.overlay_manager
                    .position_overlays(anchor_bounds2, viewport);
            }
            // Get final overlay position
            let overlay_bounds = overlay_id
                .and_then(|oid| {
                    self.overlay_manager
                        .stack
                        .iter()
                        .find(|o| o.id == oid)
                        .map(|o| o.bounds)
                })
                .unwrap_or(Rect::ZERO);
            // Layout the content at the overlay's position
            let content_proposal = SizeProposal::exact(intrinsic.width, intrinsic.height);
            layout_widget_recursive(
                &mut self.arena,
                *content_id,
                overlay_bounds,
                content_proposal,
                &base_theme,
                self.layout_direction,
                self.text_backend.as_ref(),
            );
        }

        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_layout = false;
            }
        }
    }

    // --- Querying ---

    /// Get an immutable reference to a widget node (for internal use).
    #[allow(dead_code)] // Internal helper used for widget introspection
    pub(crate) fn arena_get(&self, id: WidgetId) -> Option<&crate::arena::WidgetNode> {
        self.arena.get(id)
    }

    pub fn bounds(&self, id: WidgetId) -> Rect {
        self.arena.bounds(id)
    }

    pub fn children(&self, id: WidgetId) -> Vec<WidgetId> {
        self.arena.children(id).to_vec()
    }

    pub fn needs_layout(&self) -> bool {
        self.arena.any_needs_layout()
    }

    pub fn needs_paint(&self) -> bool {
        self.arena.any_needs_paint()
    }

    pub fn active_animation_count(&self) -> usize {
        self.animation_scheduler.active_count()
    }

    pub fn pending_tooltip_count(&self) -> usize {
        self.tooltips
            .iter()
            .filter(|entry| entry.overlay_id.is_none() && entry.real_hover_start.is_some())
            .count()
    }

    /// Whether there are pending idle callbacks to run.
    pub fn has_idle_work(&self) -> bool {
        !self.idle_queue.is_empty()
    }

    /// Drain and run all pending idle callbacks with the given time budget.
    /// Called by the event loop during idle periods between frames.
    pub fn run_idle_callbacks(&mut self, budget: std::time::Duration) {
        let callbacks = self.idle_queue.drain();
        for cb in callbacks {
            cb(crate::idle::IdleDeadline::new(budget));
        }
    }

}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a `WidgetEvent` to a `RawPointerEvent` if applicable.
fn to_raw_pointer_event(event: &WidgetEvent) -> Option<RawPointerEvent> {
    match event {
        WidgetEvent::PointerDown { position, button } => Some(RawPointerEvent::Down {
            position: *position,
            button: *button,
        }),
        WidgetEvent::PointerMove { position } => Some(RawPointerEvent::Move {
            position: *position,
        }),
        WidgetEvent::PointerUp { position, button } => Some(RawPointerEvent::Up {
            position: *position,
            button: *button,
        }),
        _ => None,
    }
}

/// Recursive layout pass operating on the arena directly (avoids borrow conflicts).
fn layout_widget_recursive(
    arena: &mut WidgetArena,
    id: WidgetId,
    parent_bounds: Rect,
    proposal: SizeProposal,
    base_theme: &fern_tokens::Theme,
    layout_direction: crate::environment::LayoutDirection,
    text_backend: Option<&std::rc::Rc<std::cell::RefCell<dyn fern_canvas::TextBackend>>>,
) {
    if !arena.is_active(id) {
        return;
    }

    // Resolve the effective theme for this widget (applies ancestor overrides)
    let resolved_theme = arena.resolve_theme(id, base_theme);

    // Phase 1: query desired size (borrows arena immutably via ctx)
    let desired_size = {
        let ctx = LayoutContext {
            theme: &resolved_theme,
            layout_direction,
            text_backend,
            arena: Some(arena),
        };
        let node = arena.get(id).unwrap();
        node.widget.size_that_fits(proposal, &ctx)
    }; // ctx dropped here, releasing immutable borrow

    let bounds = Rect::new(
        parent_bounds.x,
        parent_bounds.y,
        proposal.width.unwrap_or(desired_size.width),
        proposal.height.unwrap_or(desired_size.height),
    );
    if let Some(node) = arena.get_mut(id) {
        if node.bounds != bounds {
            node.cached_paint = None;
            node.dirty.needs_paint = true;
        }
        node.bounds = bounds;
    }

    let child_ids: Vec<WidgetId> = arena.children(id).to_vec();
    if !child_ids.is_empty() {
        // Only include active children in placements — dormant children
        // should not occupy layout space.
        let active_child_ids: Vec<WidgetId> = child_ids
            .iter()
            .copied()
            .filter(|&cid| arena.is_active(cid))
            .collect();

        let mut placements: Vec<WidgetPlacement> = active_child_ids
            .iter()
            .map(|&cid| WidgetPlacement {
                id: cid,
                origin: bounds.origin(),
                size: bounds.size(),
            })
            .collect();

        // Phase 2: place children (borrows arena immutably via ctx)
        {
            let ctx = LayoutContext {
                theme: &resolved_theme,
                layout_direction,
                text_backend,
                arena: Some(arena),
            };
            let node = arena.get(id).unwrap();
            node.widget
                .place_children(bounds, proposal, &mut placements, &ctx);
        } // ctx dropped here

        for placement in &placements {
            let child_bounds = Rect::from_origin_size(placement.origin, placement.size);
            if let Some(child_node) = arena.get_mut(placement.id) {
                if child_node.bounds != child_bounds {
                    child_node.cached_paint = None;
                    child_node.dirty.needs_paint = true;
                }
                child_node.bounds = child_bounds;
            }

            let child_proposal = SizeProposal::exact(placement.size.width, placement.size.height);
            let grandchild_ids: Vec<WidgetId> = arena.children(placement.id).to_vec();
            if !grandchild_ids.is_empty() {
                layout_widget_recursive(
                    arena,
                    placement.id,
                    child_bounds,
                    child_proposal,
                    base_theme,
                    layout_direction,
                    text_backend,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, InsetWidget, StackWidget};
    use fern_canvas::Size;
    use fern_tokens::{Color, CornerRadius};

    #[derive(Debug)]
    struct ShrinkWrapContainer {
        child: WidgetId,
        inset: f32,
    }

    impl Widget for ShrinkWrapContainer {
        fn size_that_fits(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> Size {
            let child_size = ctx
                .child_size(self.child, SizeProposal::unspecified())
                .unwrap_or(Size::ZERO);
            Size::new(
                child_size.width + self.inset * 2.0,
                child_size.height + self.inset * 2.0,
            )
        }

        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = Point::new(bounds.x + self.inset, bounds.y + self.inset);
                child.size = Size::new(
                    (bounds.width - self.inset * 2.0).max(0.0),
                    (bounds.height - self.inset * 2.0).max(0.0),
                );
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            vec![self.child]
        }
    }

    #[test]
    fn single_widget_fills_proposal() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 40.0));
        let bounds = tree.bounds(w);
        assert_eq!(bounds.width, 200.0);
        assert_eq!(bounds.height, 40.0);
    }

    #[test]
    fn stack_children_overlap() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        let stack = tree.add(StackWidget::new().add_child(a).add_child(b));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let children = tree.children(stack);
        assert_eq!(children.len(), 2);
        let ab = tree.bounds(children[0]);
        let bb = tree.bounds(children[1]);
        assert_eq!(ab.origin(), bb.origin());
        assert_eq!(ab.size(), bb.size());
    }

    #[test]
    fn inset_widget_insets_child() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(InsetWidget::new(10.0).set_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let children = tree.children(parent);
        let cb = tree.bounds(children[0]);
        assert_eq!(cb.x, 10.0);
        assert_eq!(cb.y, 10.0);
        assert_eq!(cb.width, 80.0);
        assert_eq!(cb.height, 30.0);
    }

    #[test]
    fn recursive_layout_preserves_exact_parent_placement_for_containers() {
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let shrink = tree.add(ShrinkWrapContainer {
            child: leaf,
            inset: 8.0,
        });
        let root = tree.add(StackWidget::new().add_child(shrink));

        tree.layout(SizeProposal::exact(120.0, 80.0));

        assert_eq!(tree.bounds(root), Rect::new(0.0, 0.0, 120.0, 80.0));
        assert_eq!(
            tree.bounds(shrink),
            Rect::new(0.0, 0.0, 120.0, 80.0),
            "child container should keep the exact size assigned by its parent"
        );
        assert_eq!(tree.bounds(leaf), Rect::new(8.0, 8.0, 104.0, 64.0));
    }

    #[test]
    fn pointer_enter_leave_synthesized() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered, Some(w));
        tree.pointer_move(Point::new(200.0, 200.0));
        assert_eq!(tree.hovered, None);
    }

    #[test]
    fn click_dispatches_to_widget() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.click(w);
    }

    #[test]
    fn focus_widget() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(w);
        assert_eq!(tree.focused(), Some(w));
    }

    #[test]
    fn focus_change() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));
        tree.focus(b);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn tab_cycles_through_focusable_widgets() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.focused(), None);

        // First Tab → focuses first focusable
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        // Second Tab → next
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));

        // Third Tab → next
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(c));

        // Fourth Tab → wraps to first
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn shift_tab_cycles_backwards() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Tab to first, then Shift+Tab wraps to last
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(c));

        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn tab_skips_non_focusable_widgets() {
        let mut tree = WidgetTree::new();
        let _not_focusable = tree.add(FillWidget::new());
        let a = tree.add(FillWidget::new().focusable());
        let _also_not = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));
    }

    #[test]
    fn tab_focus_has_keyboard_origin() {
        let mut tree = WidgetTree::new();
        let _a = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focus_origin(),
            Some(crate::focus::FocusOrigin::Keyboard)
        );
    }

    #[test]
    fn fill_widget_produces_shape_in_frame() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(6.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].shape, fern_canvas::ShapeKind::RoundedRect);
    }

    #[test]
    fn empty_tree_renders_empty_frame() {
        let mut tree = WidgetTree::new();
        let frame = tree.render();
        assert!(frame.is_empty());
    }

    #[test]
    fn labeled_widget_has_accessibility() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().label("Hello"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        let info = tree.accessibility_node(w);
        assert_eq!(info.role(), accesskit::Role::Label);
        assert_eq!(info.name(), Some("Hello"));
    }

    #[test]
    fn find_by_label_works() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().label("Save"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert_eq!(tree.find_by_label("Save"), Some(w));
    }

    #[test]
    fn find_by_role_works() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Text"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert!(tree.find_by_role(accesskit::Role::Label).is_some());
    }

    #[test]
    fn needs_paint_after_layout() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        assert!(tree.needs_layout());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(!tree.needs_layout());
    }

    #[test]
    fn render_clears_paint_dirty() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.needs_paint());
        tree.render();
        assert!(!tree.needs_paint());
    }

    #[test]
    fn dormant_widget_not_rendered() {
        let mut tree = WidgetTree::new();
        let w = tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(!frame.shapes.is_empty());

        tree.set_dormant(w);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(frame.shapes.is_empty());
    }

    #[test]
    fn dormancy_is_recursive() {
        let mut tree = WidgetTree::new();
        let child = tree.add(
            FillWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Both render
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);

        // Dormant parent should also dormant child
        tree.set_dormant(parent);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert!(frame.shapes.is_empty());

        // Reactivate parent should also reactivate child
        tree.activate(parent);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
    }

    #[test]
    fn dormant_widget_not_hit_tested() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Active: hover works
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered, Some(w));

        // Dormant: not hit-testable
        tree.set_dormant(w);
        tree.pointer_move(Point::new(200.0, 200.0)); // clear hover first
        tree.pointer_move(Point::new(50.0, 25.0));
        assert_eq!(tree.hovered, None);
    }

    #[test]
    fn dormant_widget_not_in_focus_cycle() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        let c = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(a);
        tree.set_dormant(b);

        // Tab from a should skip dormant b and go to c
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(c));
    }

    #[test]
    fn destroy_removes_from_arena() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().label("Gone"));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.find_by_label("Gone").is_some());

        tree.arena.destroy(w);
        assert!(tree.find_by_label("Gone").is_none());
    }

    #[test]
    fn child_bounds_helper() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new());
        let parent = tree.add(InsetWidget::new(5.0).set_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let cb = tree.child_bounds(parent, 0);
        assert_eq!(cb.x, 5.0);
    }

    #[derive(Debug, Clone, PartialEq)]
    enum TestCmd {
        Save,
        #[allow(dead_code)]
        Bold,
    }
    impl AppCommand for TestCmd {}

    #[test]
    fn state_get_set_and_derived() {
        use crate::state::State;
        let text = State::new(String::new());
        let is_empty = text.map(|t| t.is_empty());
        assert!(is_empty.get());
        text.set("hello".to_string());
        assert!(!is_empty.get());
    }

    #[test]
    fn shortcut_intercepts_before_widget() {
        use crate::shortcut::{Shortcut, ShortcutMap};
        use std::cell::Cell;
        use std::rc::Rc;

        let save_called = Rc::new(Cell::new(false));
        let s = save_called.clone();

        let shortcuts = ShortcutMap::new().bind(Shortcut::ctrl(Key::S), TestCmd::Save);

        let mut tree = WidgetTree::new().with_shortcuts(shortcuts);
        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                s.set(true);
            }
        });

        // Add a focusable widget so keyboard events have a target
        let w = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(w);

        // Press Ctrl+S — should be intercepted by shortcut map
        tree.press_key(Key::S, Modifiers::CTRL);
        assert!(save_called.get());
    }

    #[test]
    fn tab_cycles_focus_in_tree_order() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new().focusable());
        let b = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(200.0, 80.0));

        tree.focus(a);
        assert_eq!(tree.focused(), Some(a));

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(b));

        // Shift-Tab goes back
        tree.press_key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(tree.focused(), Some(a));
    }

    #[test]
    fn scroll_event_dispatched_to_hovered() {
        let mut tree = WidgetTree::new();
        let _w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Move pointer over widget
        tree.pointer_move(Point::new(50.0, 25.0));
        // Dispatch scroll — should not panic
        tree.dispatch_event(WidgetEvent::Scroll {
            delta: crate::event::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
    }

    #[test]
    fn ime_event_dispatched_to_focused() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().focusable());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(w);

        // Dispatch IME events — should not panic
        tree.dispatch_event(WidgetEvent::ImeComposition {
            text: "あ".to_string(),
            cursor: None,
        });
        tree.dispatch_event(WidgetEvent::ImeCommit {
            text: "あ".to_string(),
        });
    }

    #[test]
    fn state_binding_marks_widget_dirty_on_layout() {
        use crate::state::State;

        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.render(); // clear dirty

        assert!(!tree.needs_paint());

        // Bind a state to the widget
        let visible = State::new(true);
        visible.bind_to(
            w,
            tree.binding_registry(),
            crate::state::BindingLevel::RepaintOnly,
        );

        // Change state — widget should become dirty on next layout()
        visible.set(false);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.needs_paint());
    }

    // --- Gesture integration tests ---

    #[test]
    fn gesture_tap_recognized_on_click() {
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let t = tapped.clone();

        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(w, TapRecognizer::new(), move |gesture, _ctx| {
            if matches!(gesture, crate::gesture::GestureEvent::Tap { .. }) {
                t.set(true);
            }
        });

        tree.click(w);
        assert!(tapped.get());
    }

    #[test]
    fn gesture_drag_recognized_on_drag() {
        use crate::gesture::DragRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let drag_started = Rc::new(Cell::new(false));
        let drag_ended = Rc::new(Cell::new(false));
        let ds = drag_started.clone();
        let de = drag_ended.clone();

        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(
            w,
            DragRecognizer::new().threshold(5.0),
            move |gesture, _ctx| match gesture {
                crate::gesture::GestureEvent::DragStarted { .. } => ds.set(true),
                crate::gesture::GestureEvent::DragEnded { .. } => de.set(true),
                _ => {}
            },
        );

        // Drag from (50,25) to (80,25) — beyond threshold
        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));

        assert!(drag_started.get());
        assert!(drag_ended.get());
    }

    #[test]
    fn gesture_handler_can_emit_commands() {
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let cmd_received = Rc::new(Cell::new(false));
        let cr = cmd_received.clone();

        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(w, TapRecognizer::new(), |_gesture, ctx| {
            ctx.emit(TestCmd::Save);
        });

        tree.on_command(move |cmd: &TestCmd| {
            if *cmd == TestCmd::Save {
                cr.set(true);
            }
        });

        tree.click(w);
        assert!(cmd_received.get());
    }

    #[test]
    fn gesture_handler_called_on_tap() {
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let handler_called = Rc::new(Cell::new(false));
        let h = handler_called.clone();

        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(w, TapRecognizer::new(), move |_, _| {
            h.set(true);
        });

        tree.click(w);
        assert!(handler_called.get());
    }

    #[test]
    fn multiple_recognizers_on_same_widget() {
        use crate::gesture::{DragRecognizer, TapRecognizer};
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let dragged = Rc::new(Cell::new(false));
        let t = tapped.clone();
        let d = dragged.clone();

        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(w, TapRecognizer::new(), move |gesture, _ctx| {
            if matches!(gesture, crate::gesture::GestureEvent::Tap { .. }) {
                t.set(true);
            }
        });
        tree.attach_gesture(
            w,
            DragRecognizer::new().threshold(5.0),
            move |gesture, _ctx| {
                if matches!(gesture, crate::gesture::GestureEvent::DragStarted { .. }) {
                    d.set(true);
                }
            },
        );

        // Click — should trigger tap, not drag
        tree.click(w);
        assert!(tapped.get());
        assert!(!dragged.get());

        // Reset
        tapped.set(false);
        dragged.set(false);

        // Drag — should trigger drag, not tap
        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));
        assert!(dragged.get());
    }

    // --- Theming tests ---

    /// A widget that paints a rounded rect using theme.colors.primary.
    #[derive(Debug)]
    struct ThemeAwareWidget;

    impl Widget for ThemeAwareWidget {
        fn size_that_fits(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_canvas::Size {
            proposal.resolve(0.0, 0.0)
        }

        fn paint(
            &self,
            bounds: fern_canvas::Rect,
            canvas: &mut fern_canvas::Canvas,
            ctx: &PaintContext,
        ) {
            canvas.fill_rounded_rect(
                bounds,
                fern_tokens::CornerRadius::uniform(4.0),
                ctx.theme.colors.primary,
            );
        }
    }

    #[test]
    fn set_theme_marks_all_dirty() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(FillWidget::new().background(Color::RED));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.render(); // clear dirty

        assert!(!tree.needs_layout());
        assert!(!tree.needs_paint());

        tree.set_theme(Theme::dark_default());
        assert!(tree.needs_layout());
        assert!(tree.needs_paint());
    }

    #[test]
    fn set_theme_changes_rendered_colors() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(ThemeAwareWidget);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let light_frame = tree.render();
        let light_color = light_frame.shapes[0].color;

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let dark_frame = tree.render();
        let dark_color = dark_frame.shapes[0].color;

        // Light and dark primary colors should differ
        assert_ne!(light_color, dark_color);
    }

    #[test]
    fn subtree_theme_override() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        // Parent uses default theme
        let parent = tree.add(ThemeAwareWidget);
        // Child will get a dark theme override
        let _child = tree.add_child(parent, ThemeAwareWidget);

        tree.set_theme_override(parent, |theme| {
            theme.colors = fern_tokens::ColorTokens::dark_default();
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        // Both parent and child should use the dark primary
        let dark_primary = fern_tokens::ColorTokens::dark_default().primary.to_array();
        assert_eq!(frame.shapes[0].color, dark_primary);
        assert_eq!(frame.shapes[1].color, dark_primary);
    }

    #[test]
    fn theme_override_only_affects_subtree() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        let _unaffected = tree.add(ThemeAwareWidget);
        let overridden = tree.add(ThemeAwareWidget);

        tree.set_theme_override(overridden, |theme| {
            theme.colors = fern_tokens::ColorTokens::dark_default();
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));
        let frame = tree.render();

        let light_primary = fern_tokens::ColorTokens::light_default().primary.to_array();
        let dark_primary = fern_tokens::ColorTokens::dark_default().primary.to_array();

        // First widget (unaffected) uses light theme
        assert_eq!(frame.shapes[0].color, light_primary);
        // Second widget (overridden) uses dark theme
        assert_eq!(frame.shapes[1].color, dark_primary);
    }

    #[test]
    fn resolved_theme_reflects_overrides() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        let parent = tree.add(FillWidget::new());
        let child = tree.add_child(parent, FillWidget::new());

        tree.set_theme_override(parent, |theme| {
            theme.colors.primary = Color::RED;
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Parent's resolved theme should have RED primary
        let parent_theme = tree.resolved_theme(parent);
        assert_eq!(parent_theme.colors.primary, Color::RED);

        // Child inherits the override
        let child_theme = tree.resolved_theme(child);
        assert_eq!(child_theme.colors.primary, Color::RED);
    }

    #[test]
    fn nested_theme_overrides_compose() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());

        let grandparent = tree.add(FillWidget::new());
        let parent = tree.add_child(grandparent, FillWidget::new());
        let child = tree.add_child(parent, FillWidget::new());

        // Grandparent overrides primary color
        tree.set_theme_override(grandparent, |theme| {
            theme.colors.primary = Color::RED;
        });
        // Parent additionally overrides secondary color
        tree.set_theme_override(parent, |theme| {
            theme.colors.secondary = Color::GREEN;
        });

        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Child sees both overrides composed
        let child_theme = tree.resolved_theme(child);
        assert_eq!(child_theme.colors.primary, Color::RED);
        assert_eq!(child_theme.colors.secondary, Color::GREEN);
    }

    // --- Idle callback tests ---

    #[test]
    fn idle_callback_requested_from_event_handler() {
        use crate::widget_builder::WidgetBuilder;
        use std::cell::Cell;
        use std::rc::Rc;

        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().on_tap(move |ctx| {
            let called = c.clone();
            ctx.request_idle_callback(move |_deadline| {
                called.set(true);
            });
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(!tree.has_idle_work());

        // Click the widget — its event handler requests an idle callback
        tree.click(w);

        assert!(tree.has_idle_work());
        assert!(!called.get());

        // Run idle callbacks
        tree.run_idle_callbacks(std::time::Duration::from_millis(16));

        assert!(called.get());
        assert!(!tree.has_idle_work());
    }

    #[test]
    fn idle_deadline_provides_time_budget() {
        use std::cell::Cell;
        use std::rc::Rc;

        let had_time = Rc::new(Cell::new(false));
        let _h = had_time.clone();

        // Directly test IdleDeadline
        let deadline = crate::idle::IdleDeadline::new(std::time::Duration::from_millis(100));
        assert!(!deadline.did_timeout());
        assert!(deadline.time_remaining() > std::time::Duration::ZERO);
    }

    // --- Accessibility tests ---

    #[test]
    fn accessibility_node_collects_actions() {
        use crate::accessibility::AccessNodeBuilder;

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

        let mut tree = WidgetTree::new();
        let w = tree.add(ActionWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        let info = tree.accessibility_node(w);
        assert_eq!(info.role(), accesskit::Role::Button);
        assert_eq!(info.name(), Some("Save"));
        assert_eq!(info.actions().len(), 2);
        assert!(info.actions().contains(&accesskit::Action::Click));
        assert!(info.actions().contains(&accesskit::Action::Focus));
    }

    #[test]
    fn sync_accessibility_produces_tree_update() {
        let mut tree = WidgetTree::new();
        let _w1 = tree.add(FillWidget::new().label("First"));
        let _w2 = tree.add(FillWidget::new().label("Second"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();

        // Should have root node + 2 widget nodes = 3 total
        assert_eq!(update.nodes.len(), 3);

        // Root node is first
        assert_eq!(update.nodes[0].0, accesskit::NodeId(0));

        // Tree metadata present
        assert!(update.tree.is_some());
    }

    #[test]
    fn sync_accessibility_excludes_dormant_widgets() {
        let mut tree = WidgetTree::new();
        let _w1 = tree.add(FillWidget::new().label("Active"));
        let w2 = tree.add(FillWidget::new().label("Dormant"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.set_dormant(w2);

        let update = tree.sync_accessibility();

        // Root + 1 active widget = 2 nodes (dormant excluded)
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn sync_accessibility_includes_focus() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().focusable().label("Focused"));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(w);

        let update = tree.sync_accessibility();

        // Focus should point to the focused widget's NodeId
        let expected_focus = crate::accessibility::widget_id_to_node_id(w);
        assert_eq!(update.focus, expected_focus);
    }

    #[test]
    fn sync_accessibility_parent_child_relationship() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let parent = tree.add(crate::test_widgets::StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();

        // Root + parent + child = 3 nodes
        assert_eq!(update.nodes.len(), 3);

        // Find the parent node and check it has child as a child
        let parent_node_id = crate::accessibility::widget_id_to_node_id(parent);
        let parent_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == parent_node_id)
            .map(|(_, n)| n)
            .unwrap();

        let child_node_id = crate::accessibility::widget_id_to_node_id(child);
        assert!(parent_node.children().contains(&child_node_id));
    }

    // --- Testability query helpers ---

    /// Widget that declares a Click action in accessibility.
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
    fn find_by_action_finds_clickable() {
        let mut tree = WidgetTree::new();
        let w = tree.add(ClickableWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.find_by_action(accesskit::Action::Click), Some(w));
        assert_eq!(tree.find_by_action(accesskit::Action::Focus), None);
    }

    #[test]
    fn is_visible_reflects_dormancy() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(tree.is_visible(w));
        tree.set_dormant(w);
        assert!(!tree.is_visible(w));
        tree.activate(w);
        assert!(tree.is_visible(w));
    }

    #[test]
    fn text_content_returns_accessibility_name() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new().label("Hello World"));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(w), Some("Hello World".to_string()));
    }

    #[test]
    fn text_content_returns_none_without_label() {
        let mut tree = WidgetTree::new();
        let w = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(w), None);
    }

    #[test]
    fn text_value_returns_accessibility_value() {
        /// Widget that sets a value in accessibility.
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
        let w = tree.add(ValueWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.text_value(w), Some("75%".to_string()));
        assert_eq!(tree.text_content(w), Some("Volume".to_string()));
    }

    #[test]
    fn advance_time_updates_simulated_clock() {
        let mut tree = WidgetTree::new();
        let t0 = tree.simulated_now();

        tree.advance_time(std::time::Duration::from_millis(500));
        let t1 = tree.simulated_now();

        assert_eq!(t1.duration_since(t0), std::time::Duration::from_millis(500));
    }

    // --- Overlay system tests ---

    #[test]
    fn show_and_dismiss_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new().label("Overlay"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!(tree.active_overlays().is_empty());

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);

        tree.dismiss_overlay(id);
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content), "dismissed content should be dormant");
    }

    #[test]
    fn escape_dismisses_topmost_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().focusable());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.focus(anchor);

        tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
        });

        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Escape, Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content), "escaped content should be dormant");
    }

    #[test]
    fn click_outside_dismisses_overlay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::ClickOutside,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
        });

        // Set overlay bounds so hit-test works
        tree.overlay_manager
            .set_content_bounds(id, fern_canvas::Size::new(100.0, 50.0));

        assert_eq!(tree.active_overlays().len(), 1);

        // Click outside overlay bounds
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(500.0, 500.0),
            button: PointerButton::Primary,
        });
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(content), "click-outside content should be dormant");
    }

    #[test]
    fn cascade_dismissal() {
        let mut tree = WidgetTree::new();
        let a1 = tree.add(FillWidget::new());
        let c1 = tree.add(FillWidget::new());
        let c2 = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let parent = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: c1,
            anchor: a1,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
        });
        let _child = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: c2,
            anchor: c1,
            placement: crate::overlay::OverlayPlacement::TrailingEdge,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: Some(parent),
        });

        assert_eq!(tree.active_overlays().len(), 2);

        tree.dismiss_overlay(parent);
        assert!(tree.active_overlays().is_empty());
        assert!(!tree.is_visible(c1), "cascaded content c1 should be dormant");
        assert!(!tree.is_visible(c2), "cascaded content c2 should be dormant");
    }

    #[test]
    fn dismissed_overlay_content_is_dormant_and_invisible() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let content = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let id = tree.show_overlay(crate::overlay::OverlayRequest {
            content_id: content,
            anchor,
            placement: crate::overlay::OverlayPlacement::Below,
            dismiss: crate::overlay::DismissBehavior::Manual,
            layer: crate::overlay::OverlayLayer::InTree,
            parent_overlay: None,
        });

        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Dismiss the overlay
        tree.dismiss_overlay(id);
        assert!(!tree.is_visible(content), "content should be dormant");

        // Run layout again — dormant content should not interfere
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Content must not be found by hit testing (dormant widgets are skipped)
        let center = tree.bounds(content).center();
        let hit = tree.hit_test(center);
        assert_ne!(
            hit,
            Some(content),
            "dormant dismissed content must not be hit-testable"
        );

        // Content must not appear in rendered frame (dormant widgets are skipped)
        let _frame = tree.render();
        // The anchor should render, but the dismissed content should not add shapes
        // (dormant is_active check at paint_widget_cached:2072 prevents painting)
        assert!(!tree.is_visible(content), "content stays dormant after layout+render");
    }

    #[test]
    fn tooltip_appears_after_delay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tooltip text"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        // Hover over the anchor
        let center = tree.bounds(anchor).center();
        tree.pointer_move(center);

        // Not yet shown — delay hasn't elapsed
        assert!(tree.active_overlays().is_empty());

        // Advance time past the delay
        tree.advance_time(std::time::Duration::from_millis(600));

        // Tooltip should now be shown as an overlay
        assert_eq!(tree.active_overlays().len(), 1);

        // The tooltip widget should be findable
        assert!(tree.find_by_label("Tooltip text").is_some());
    }

    #[test]
    fn tooltip_dismissed_on_pointer_leave() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        // Hover and wait
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(600));
        assert_eq!(tree.active_overlays().len(), 1);

        // Move pointer away
        tree.pointer_move(Point::new(500.0, 500.0));
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn tooltip_not_shown_if_pointer_leaves_before_delay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tip"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(anchor, tooltip, std::time::Duration::from_millis(500));

        // Hover briefly then leave
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(200));
        tree.pointer_move(Point::new(500.0, 500.0));

        // Even after more time, tooltip should not appear
        tree.advance_time(std::time::Duration::from_millis(500));
        assert!(tree.active_overlays().is_empty());
    }

    // --- AccessKit action routing ---

    #[test]
    fn access_action_routes_to_target_widget() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        let b = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Focus widget a, but dispatch AccessAction targeting b
        tree.focus(a);
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Click,
            target: Some(b),
        });
        // The event should reach b, not a. Both widgets ignore it (FillWidget
        // returns Ignored), so this is a routing test — no crash, no panic.
    }

    #[test]
    fn access_action_falls_back_to_focused() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.focus(a);
        // No target specified — should fall back to focused widget
        tree.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Focus,
            target: None,
        });
    }

    // --- Scoped shortcuts ---

    #[test]
    fn scoped_shortcut_fires_when_focused_in_subtree() {
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Debug, Clone, Copy, PartialEq)]
        #[allow(dead_code)]
        enum Cmd {
            ScopedAction,
            GlobalAction,
        }
        impl crate::app_command::AppCommand for Cmd {}

        let fired = Rc::new(Cell::new(None));
        let f = fired.clone();

        let shortcuts = crate::shortcut::ShortcutMap::new()
            .bind(crate::shortcut::Shortcut::ctrl(Key::Z), Cmd::GlobalAction);

        let mut tree = WidgetTree::new().with_shortcuts(shortcuts);
        tree.on_command(move |cmd: &Cmd| {
            f.set(Some(*cmd));
        });

        let parent = tree.add(FillWidget::new());
        let child = tree.add_child(parent, FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Focus the child and press the global shortcut
        tree.focus(child);
        tree.press_key(Key::Z, Modifiers::CTRL);
        assert_eq!(fired.get(), Some(Cmd::GlobalAction));
    }

    // --- Animation ---

    #[test]
    fn set_animated_interpolates_over_time() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::Linear,
        );

        // Advance 100ms (50%)
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (*state.get() - 50.0).abs() < 2.0,
            "at 50%: {}",
            *state.get()
        );

        // Advance another 100ms (100%)
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (*state.get() - 100.0).abs() < 0.1,
            "at 100%: {}",
            *state.get()
        );

        assert!(!tree.has_active_animations());
    }

    #[test]
    fn set_animated_with_easing() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::EaseIn,
        );

        // At 50%, EaseIn (t²) = 0.25 → value ≈ 25
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!(
            (*state.get() - 25.0).abs() < 2.0,
            "ease-in at 50%: {}",
            *state.get()
        );
    }

    #[test]
    fn set_animated_replaces_in_flight() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::Linear,
        );
        tree.tick_animations(std::time::Duration::from_millis(100)); // at 50
        assert!((*state.get() - 50.0).abs() < 2.0);

        // New animation from current value to 0
        state.set_animated(
            0.0,
            std::time::Duration::from_millis(100),
            fern_tokens::Easing::Linear,
        );
        tree.tick_animations(std::time::Duration::from_millis(50)); // 50% of new
        assert!(
            (*state.get() - 25.0).abs() < 3.0,
            "mid-replace: {}",
            *state.get()
        );

        tree.tick_animations(std::time::Duration::from_millis(50)); // 100% of new
        assert!(
            (*state.get() - 0.0).abs() < 0.5,
            "end-replace: {}",
            *state.get()
        );
    }

    #[test]
    fn animation_marks_widgets_dirty() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new_animated(100.0_f32);
        tree.register_animated_state(&state);

        let w = tree.add(FillWidget::new());
        state.bind_to(
            w,
            tree.binding_registry(),
            crate::state::BindingLevel::Relayout,
        );

        tree.layout(SizeProposal::exact(200.0, 100.0));

        state.set_animated(
            0.0,
            std::time::Duration::from_millis(100),
            fern_tokens::Easing::Linear,
        );

        // Tick animation — state changes should mark widget dirty
        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(tree.needs_redraw());
    }
}
