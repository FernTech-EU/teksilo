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
mod layout_impl;
mod overlay_impl;
mod query_impl;
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
    pending_modal_requests: Vec<crate::modal::QueuedModalRequest>,
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
    /// Current cursor selected by hover/interaction routing.
    current_cursor: crate::widget::CursorIcon,
    /// Delayed overlay requests (e.g., submenu hover-open delay).
    pending_delayed_overlays: Vec<PendingDelayedOverlay>,
    /// OS-level accessibility preferences (high contrast, reduced motion, text scale).
    prefers_high_contrast: bool,
    prefers_reduced_motion: bool,
    text_scale_factor: f64,
    /// Active drag-and-drop session, if any.
    pub(crate) active_drag: Option<crate::drag_state::DragSession>,
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
            pending_modal_requests: Vec::new(),
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
            current_cursor: crate::widget::CursorIcon::Default,
            pending_delayed_overlays: Vec::new(),
            prefers_high_contrast: false,
            prefers_reduced_motion: false,
            text_scale_factor: 1.0,
            active_drag: None,
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
                || self
                    .overlay_manager
                    .is_descendant_of(candidate.id, overlay_id))
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

    fn process_auto_dismiss_overlays(&mut self) {
        let sim_now = self.sim_clock;
        self.process_auto_dismiss_overlays_impl(|overlay| {
            overlay
                .auto_dismiss_after
                .map(|_| sim_now.saturating_duration_since(overlay.shown_at_sim))
        });
    }

    fn process_auto_dismiss_overlays_real(&mut self) {
        let real_now = std::time::Instant::now();
        self.process_auto_dismiss_overlays_impl(|overlay| {
            overlay
                .auto_dismiss_after
                .map(|_| real_now.saturating_duration_since(overlay.shown_at_real))
        });
    }

    fn process_auto_dismiss_overlays_impl(
        &mut self,
        elapsed_fn: impl Fn(&crate::overlay::ActiveOverlay) -> Option<std::time::Duration>,
    ) {
        let mut to_dismiss = Vec::new();

        for overlay in self.overlay_manager.stack.iter().rev() {
            let Some(delay) = overlay.auto_dismiss_after else {
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
        self.animated_values
            .retain(|registration| registration.is_alive());
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
        self.animated_values
            .retain(|registration| registration.is_alive());
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
                    self.animation_scheduler.animate(
                        &state,
                        req.target,
                        req.duration,
                        req.easing,
                        now,
                    );
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
            self.rebuild_single_widget(widget_id);
        }
    }

    /// Rebuild a single composite widget: destroy old children, re-run `build()`,
    /// and wire up new children. Used by both `rebuild_built_widgets()` (environment
    /// changes) and `process_state_changes()` (data-driven rebuild).
    pub(crate) fn rebuild_single_widget(&mut self, widget_id: WidgetId) {
        if let Some(node) = self.arena.get_mut(widget_id) {
            node.effect_handles.clear();
            node.dirty.needs_rebuild = false;
        }

        let old_children: Vec<WidgetId> = self.arena.children(widget_id).to_vec();
        for child_id in old_children {
            self.arena.destroy(child_id);
        }

        let mut widget_box = match self.arena.take_widget(widget_id) {
            Some(widget) => widget,
            None => return,
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

    /// Drain all pending modal requests recorded during event handling.
    ///
    /// Each request includes the originating widget so higher layers can
    /// resolve routing and focus behavior relative to the source tree.
    pub fn drain_pending_modal_requests(&mut self) -> Vec<crate::modal::QueuedModalRequest> {
        std::mem::take(&mut self.pending_modal_requests)
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
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a `WidgetEvent` to a `RawPointerEvent` if applicable.
fn to_raw_pointer_event(event: &WidgetEvent) -> Option<RawPointerEvent> {
    match event {
        WidgetEvent::PointerDown {
            position, button, ..
        } => Some(RawPointerEvent::Down {
            position: *position,
            button: *button,
        }),
        WidgetEvent::PointerMove { position } => Some(RawPointerEvent::Move {
            position: *position,
        }),
        WidgetEvent::PointerUp {
            position, button, ..
        } => Some(RawPointerEvent::Up {
            position: *position,
            button: *button,
        }),
        _ => None,
    }
}
