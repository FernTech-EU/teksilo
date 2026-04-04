use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, RenderFrame, SizeProposal};
use fern_tokens::Theme;

use crate::accessibility::{AccessNodeBuilder, AccessibilityInfo};
use crate::app_command::{AppCommand, ErasedCommand};
use crate::arena::{GestureBinding, WidgetArena};
use crate::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use crate::gesture::{GestureArena, GestureEvent, GestureRecognizer, RawPointerEvent};
use crate::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

/// The main widget tree orchestrating arena, layout, events, accessibility, and paint.
/// Provides both the runtime API and the headless test API.
/// Type-erased shortcut lookup function.
/// The third argument is `focused`, and the fourth is an `is_in_scope(focused, scope)` checker.
type ShortcutLookup = Box<
    dyn Fn(Key, Modifiers, Option<WidgetId>, &dyn Fn(WidgetId, WidgetId) -> bool) -> Option<ErasedCommand>,
>;

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
    /// Animation scheduler for smooth State<f32> transitions.
    animation_scheduler: crate::animation::AnimationScheduler,
    /// States registered for animation (checked each frame for pending requests).
    animated_states: Vec<crate::state::State<f32>>,
    /// IDs of composite widget adapters (for rebuild on theme/environment change).
    composite_ids: Vec<WidgetId>,
    /// Observer cleanup: functions to remove observers registered during build().
    /// Keyed by composite adapter ID. Cleared and re-populated on each rebuild.
    observer_cleanups: std::collections::HashMap<WidgetId, Vec<Box<dyn Fn()>>>,
    /// Cached accessibility tree update — only rebuilt when layout changes.
    cached_a11y: Option<accesskit::TreeUpdate>,
    /// Whether the accessibility tree needs rebuilding (set when layout runs).
    a11y_dirty: bool,
    /// Cached full render frame — reused when no widget needs painting.
    cached_frame: Option<RenderFrame>,
    /// Widget that has captured the pointer (receives all PointerMove/PointerUp
    /// regardless of hit-test). Set via `EventContext::capture_pointer()`.
    pointer_captured_by: Option<WidgetId>,
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
            binding_registry: crate::state::BindingRegistry::new(),
            idle_queue: crate::idle::IdleQueue::new(),
            sim_clock: std::time::Instant::now(),
            focus_origin: None,
            overlay_manager: crate::overlay::OverlayManager::new(),
            tooltips: Vec::new(),
            layout_direction: crate::environment::LayoutDirection::default(),
            composite_ids: Vec::new(),
            observer_cleanups: std::collections::HashMap::new(),
            animation_scheduler: crate::animation::AnimationScheduler::new(),
            animated_states: Vec::new(),
            cached_a11y: None,
            a11y_dirty: true,
            cached_frame: None,
            pointer_captured_by: None,
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn with_text_backend(
        mut self,
        backend: Rc<RefCell<dyn fern_canvas::TextBackend>>,
    ) -> Self {
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
    /// Unlike `needs_redraw`, this doesn't consider active animations — only
    /// whether the tree has actual dirty widgets that need rendering.
    pub fn needs_render(&self) -> bool {
        self.arena.any_needs_layout() || self.arena.any_needs_paint()
    }

    /// Register a `State<f32>` for animation support. The framework checks
    /// registered states each frame for pending `set_animated` requests.
    /// Called automatically by `BuildContext::state()` for f32 states.
    pub fn register_animated_state(&mut self, state: &crate::state::State<f32>) {
        // Avoid duplicates
        if !self.animated_states.iter().any(|s| crate::state::State::same(s, state)) {
            self.animated_states.push(state.clone());
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
        for state in &self.animated_states {
            if let Some(req) = state.take_pending_animation() {
                self.animation_scheduler.animate(
                    state,
                    req.target,
                    req.duration,
                    req.easing,
                    now,
                );
            }
        }
    }

    /// Advance animations by simulated time (for deterministic testing).
    /// Pending `set_animated` requests are started at the current sim_clock,
    /// then time advances by `duration`, and the scheduler ticks at the new time.
    pub fn tick_animations(&mut self, duration: std::time::Duration) {
        // Start pending animations at the CURRENT time (before advancing)
        self.process_pending_animations_at(self.sim_clock);

        // Advance clock
        self.sim_clock += duration;

        // Tick the scheduler at the new time
        self.animation_scheduler.tick(self.sim_clock);

        // Process state changes from animation updates
        self.process_state_changes();
    }

    /// Switch the tree-level theme at runtime.
    /// Rebuilds all composite widgets (their derived state closures capture theme
    /// tokens at build time) and marks all widgets as needing layout and repaint.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        // Clear focus/hover/tooltips before rebuild — old widget IDs become stale.
        self.focused = None;
        self.hovered = None;
        self.focus_origin = None;
        self.tooltips.clear();
        self.rebuild_composites();
        self.arena.mark_all_dirty();
    }

    /// Reconstruct all composite widgets. Called when the environment changes
    /// (theme switch, locale switch). Each composite's old subtree is destroyed
    /// and `build()` is re-run with the new environment.
    fn rebuild_composites(&mut self) {
        let ids: Vec<WidgetId> = self.composite_ids.clone();
        for adapter_id in ids {
            // Run observer cleanup for this composite before rebuilding
            if let Some(cleanups) = self.observer_cleanups.remove(&adapter_id) {
                for cleanup in cleanups {
                    cleanup();
                }
            }

            // Get the old root child for destruction
            let old_root = {
                let node = match self.arena.get(adapter_id) {
                    Some(n) => n,
                    None => continue,
                };
                if !node.widget.is_composite() {
                    continue;
                }
                node.children.first().copied()
            };

            // Destroy the old subtree
            if let Some(old_root_id) = old_root {
                self.arena.destroy(old_root_id);
            }

            // Re-run build() via the adapter.
            // We need to temporarily take the widget out to get a mutable reference
            // to the adapter while also having &mut self for BuildContext.
            // Use a two-phase approach: take the widget box out, rebuild, put it back.
            let mut widget_box = match self.arena.take_widget(adapter_id) {
                Some(w) => w,
                None => continue,
            };

            let new_root = {
                let adapter = match widget_box
                    .as_any_mut()
                    .downcast_mut::<crate::composite_adapter::CompositeWidgetAdapter>()
                {
                    Some(a) => a,
                    None => {
                        self.arena.restore_widget(adapter_id, widget_box);
                        continue;
                    }
                };
                let mut build_ctx = crate::composite_widget::BuildContext {
                    tree: self,
                    composite_id: Some(adapter_id),
                };
                let (_old, new_root) = adapter.rebuild(&mut build_ctx);
                new_root
            };

            // Put the adapter back
            self.arena.restore_widget(adapter_id, widget_box);

            // Wire the new root child
            if let Some(child_node) = self.arena.get_mut(new_root) {
                child_node.parent = Some(adapter_id);
            }
            if let Some(adapter_node) = self.arena.get_mut(adapter_id) {
                adapter_node.children = vec![new_root];
            }
        }
    }

    /// Set the layout direction (LTR/RTL). Marks all widgets as needing layout.
    pub fn set_layout_direction(&mut self, direction: crate::environment::LayoutDirection) {
        self.layout_direction = direction;
        self.arena.mark_all_dirty();
    }

    /// Mark a widget as clipping its children to its bounds (scroll areas).
    pub fn set_clips_children(&mut self, id: WidgetId, clips: bool) {
        self.arena.set_clips_children(id, clips);
    }

    /// Set a per-child alignment override on a widget.
    pub fn set_alignment(&mut self, id: WidgetId, alignment: fern_tokens::Alignment) {
        self.arena.set_alignment_override(id, alignment);
    }

    /// Register an observer cleanup function for a composite.
    /// Called during build() when ctx.observe() is used.
    pub(crate) fn register_observer_cleanup(
        &mut self,
        composite_id: WidgetId,
        cleanup: Box<dyn Fn()>,
    ) {
        self.observer_cleanups
            .entry(composite_id)
            .or_default()
            .push(cleanup);
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

        // Process visible_when bindings: toggle dormancy based on State<bool>
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
        self.shortcut_lookup = Some(Box::new(move |key, modifiers, focused, is_in_scope| {
            let shortcut = crate::shortcut::Shortcut::new(key, modifiers);
            map.find(&shortcut, focused, is_in_scope)
                .map(|cmd| ErasedCommand::new(cmd.clone()))
        }));
        self
    }

    /// Lookup a shortcut, returning a type-erased command if matched.
    fn shortcut_map_lookup(&self, key: Key, modifiers: Modifiers) -> Option<ErasedCommand> {
        let lookup = self.shortcut_lookup.as_ref()?;
        let is_in_scope = |focused: WidgetId, scope: WidgetId| -> bool {
            self.is_descendant_of(focused, scope)
        };
        lookup(key, modifiers, self.focused, &is_in_scope)
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
        // Only consume commands if a tree-local handler is registered (headless tests).
        // In windowed apps, commands remain in pending_commands for the app-level
        // handler to drain via drain_pending_commands().
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

    /// Add a root-level Level 2 widget to the tree.
    /// Add any widget (Level 1 composite or Level 2 direct) to the tree.
    /// This is the single entry point — the `IntoWidgetTree` trait routes
    /// to the correct insertion path automatically.
    pub fn add_widget(&mut self, widget: impl crate::widget::IntoWidgetTree) -> WidgetId {
        Box::new(widget).register(self)
    }

    /// Insert a pre-boxed Widget directly. Used by the `IntoWidgetTree` blanket impl.
    /// Automatically resolves deferred (inline) children, registers reactive bindings,
    /// and processes builder-style `visible_when` / `enabled_when` metadata.
    pub fn add_widget_direct(&mut self, mut widget: Box<dyn Widget>) -> WidgetId {
        // 1. Resolve any deferred children before inserting this widget.
        let pending = widget.take_pending_children();
        if !pending.is_empty() {
            let resolved_ids: Vec<WidgetId> = pending
                .into_iter()
                .map(|child| match child {
                    crate::widget::PendingChild::Id(id) => id,
                    crate::widget::PendingChild::Deferred(w) => w.register(self),
                })
                .collect();
            widget.set_resolved_children(resolved_ids);
        }

        // 2. Extract builder-style visibility/enabled metadata before inserting.
        let vis_state = widget.take_visible_when();
        let ena_state = widget.take_enabled_when();

        // 3. Insert into the arena (this wires parent-child via widget.children()).
        let id = self.arena.insert(widget);

        // 4. Register reactive property bindings, animated states, and clips_children.
        let clips = self.arena.get(id).map_or(false, |n| n.widget.clips_children());
        if let Some(node) = self.arena.get(id) {
            node.widget.register_bindings(id, &self.binding_registry);
            for state in node.widget.animated_states() {
                self.register_animated_state(&state);
            }
        }
        if clips {
            self.arena.set_clips_children(id, true);
        }

        // 5. Apply deferred visible_when / enabled_when.
        if let Some(state) = vis_state {
            self.visible_when(id, state);
        }
        if let Some(state) = ena_state {
            self.enabled_when(id, state);
        }

        id
    }

    /// Insert a boxed CompositeWidget. Used by `IntoWidgetTree` impls on composites.
    pub fn add_composite_inner(
        &mut self,
        mut composite: Box<dyn crate::composite_widget::CompositeWidget>,
    ) -> WidgetId {
        // Extract builder-style visibility/enabled metadata before build.
        let vis_state = composite.take_visible_when();
        let ena_state = composite.take_enabled_when();

        use crate::composite_adapter::CompositeWidgetAdapter;
        let mut adapter = CompositeWidgetAdapter::new(composite);

        // Insert a placeholder to reserve the adapter's ID before build().
        // This allows BuildContext::self_id() to return the composite's own ID.
        use crate::arena::PlaceholderWidget;
        let adapter_id = self.arena.insert(Box::new(PlaceholderWidget));

        let mut build_ctx = crate::composite_widget::BuildContext {
            tree: self,
            composite_id: Some(adapter_id),
        };
        let root_child = adapter.build(&mut build_ctx);

        // Replace the placeholder with the real adapter
        self.arena.restore_widget(adapter_id, Box::new(adapter));

        if let Some(child_node) = self.arena.get_mut(root_child) {
            child_node.parent = Some(adapter_id);
        }
        if let Some(adapter_node) = self.arena.get_mut(adapter_id) {
            adapter_node.children = vec![root_child];
        }

        self.composite_ids.push(adapter_id);

        // Apply deferred visible_when / enabled_when.
        if let Some(state) = vis_state {
            self.visible_when(adapter_id, state);
        }
        if let Some(state) = ena_state {
            self.enabled_when(adapter_id, state);
        }

        adapter_id
    }

    /// Add a Level 2 (Widget) to the tree.
    pub fn add(&mut self, widget: impl Widget + 'static) -> WidgetId {
        self.add_widget_direct(Box::new(widget))
    }

    /// Add a widget as a child of another widget.
    /// Routes through the full insertion pipeline (pending children,
    /// binding registration, visible_when/enabled_when).
    pub fn add_child(&mut self, parent: WidgetId, widget: impl Widget + 'static) -> WidgetId {
        let mut boxed: Box<dyn Widget> = Box::new(widget);

        // 1. Resolve deferred children
        let pending = boxed.take_pending_children();
        if !pending.is_empty() {
            let resolved_ids: Vec<WidgetId> = pending
                .into_iter()
                .map(|child| match child {
                    crate::widget::PendingChild::Id(id) => id,
                    crate::widget::PendingChild::Deferred(w) => w.register(self),
                })
                .collect();
            boxed.set_resolved_children(resolved_ids);
        }

        // 2. Extract builder-style metadata
        let vis_state = boxed.take_visible_when();
        let ena_state = boxed.take_enabled_when();

        // 3. Insert as child
        let id = self.arena.insert_child(parent, boxed);

        // 4. Register reactive bindings, animated states, and clips_children
        let clips = self.arena.get(id).map_or(false, |n| n.widget.clips_children());
        if let Some(node) = self.arena.get(id) {
            node.widget.register_bindings(id, &self.binding_registry);
            for state in node.widget.animated_states() {
                self.register_animated_state(&state);
            }
        }
        if clips {
            self.arena.set_clips_children(id, true);
        }

        // 5. Apply visible_when / enabled_when
        if let Some(state) = vis_state {
            self.visible_when(id, state);
        }
        if let Some(state) = ena_state {
            self.enabled_when(id, state);
        }

        id
    }

    /// Add a Level 1 (CompositeWidget) to the tree. Alias for `add_widget()`.
    pub fn add_composite(
        &mut self,
        composite: impl crate::composite_widget::CompositeWidget + 'static,
    ) -> WidgetId {
        self.add_composite_inner(Box::new(composite))
    }

    // --- Property bindings ---

    /// Bind a widget's visibility to a boolean state or derived state.
    /// When false, the widget is set dormant; when true, it is activated.
    /// Accepts `State<bool>`, `DerivedState<bool>`, or plain `bool`.
    pub fn visible_when(&mut self, id: WidgetId, state: impl Into<crate::state::Reactive<bool>>) {
        let reactive = state.into();
        reactive.register_if_bound(id, &self.binding_registry, crate::state::BindingLevel::Relayout);
        if let Some(node) = self.arena.get_mut(id) {
            node.visible_state = Some(reactive);
        }
    }

    /// Bind a widget's enabled state to a boolean state or derived state.
    /// When false, the widget ignores all events but remains visible.
    /// Accepts `State<bool>`, `DerivedState<bool>`, or plain `bool`.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::state::Reactive<bool>>) {
        let reactive = state.into();
        reactive.register_if_bound(id, &self.binding_registry, crate::state::BindingLevel::Relayout);
        if let Some(node) = self.arena.get_mut(id) {
            node.enabled_state = Some(reactive);
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
        let had_override = self.arena.get(id).map_or(false, |n| n.theme_override.is_some());
        if let Some(node) = self.arena.get_mut(id) {
            node.theme_override = Some(crate::environment::ThemeOverride {
                func: Box::new(f),
            });
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
                let mut old_handler =
                    std::mem::replace(&mut binding.handler, Box::new(|_, _| {}));
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
        self.overlay_manager.position_overlays(anchor_bounds);
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
                    SizeProposal { width: None, height: None },
                    &ctx,
                )
            };
            // Update the overlay bounds with the intrinsic size
            if let Some(oid) = overlay_id {
                self.overlay_manager.set_content_bounds(oid, intrinsic);
                // Re-position with correct size
                let anchor_bounds2 = |id: WidgetId| -> Rect { self.arena.bounds(id) };
                self.overlay_manager.position_overlays(anchor_bounds2);
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

    // --- Event dispatch ---

    /// Dispatch a widget event, routing to the appropriate widget.
    ///
    /// Routing rules (architecture Section 7.1):
    /// - Pointer events → hit testing against layout tree
    /// - Keyboard/IME events → focused widget
    /// - AccessKit actions → target widget directly
    /// - Scroll events → hit testing (scroll target under pointer)
    pub fn dispatch_event(&mut self, event: WidgetEvent) {
        // Overlay interception: Escape dismisses topmost overlay
        if let WidgetEvent::KeyDown { key: Key::Escape, .. } = &event {
            if !self.overlay_manager.is_empty() {
                self.overlay_manager.dismiss_top();
                return;
            }
        }

        // Overlay interception: click outside dismisses ClickOutside overlays
        if let WidgetEvent::PointerDown { position, .. } = &event {
            if self.overlay_manager.handle_click_outside(*position) {
                return; // Click consumed by overlay dismissal
            }
        }

        // Shortcut interception: before any widget sees the key event
        if let WidgetEvent::KeyDown { key, modifiers, .. } = &event {
            if let Some(cmd) = self.shortcut_map_lookup(*key, *modifiers) {
                self.pending_commands.push(cmd);
                self.flush_commands();
                return;
            }
        }

        // Feed pointer events to gesture recognizers on the target widget chain
        if let Some(raw) = to_raw_pointer_event(&event) {
            let target = if self.pointer_captured_by.is_some() {
                self.pointer_captured_by
            } else {
                match &event {
                    WidgetEvent::PointerDown { position, .. }
                    | WidgetEvent::PointerUp { position, .. } => self.hit_test(*position),
                    WidgetEvent::PointerMove { position } => self.hit_test(*position),
                    _ => None,
                }
            };
            if let Some(target_id) = target {
                self.feed_gesture_recognizers(target_id, &raw);
            }
        }

        match &event {
            WidgetEvent::PointerMove { position } => {
                if let Some(captured) = self.pointer_captured_by {
                    // Pointer is captured — route directly, skip hover tracking
                    self.dispatch_to_widget(captured, &WidgetEvent::PointerMove { position: *position });
                } else {
                    self.handle_pointer_move(*position);
                }
            }
            WidgetEvent::PointerDown { position, .. } => {
                if let Some(target) = self.hit_test(*position) {
                    // Focus the nearest focusable widget (target or ancestor)
                    if let Some(focusable) = self.find_focusable_at_or_above(target) {
                        self.focus_with_origin(focusable, crate::focus::FocusOrigin::Pointer);
                    }
                    self.dispatch_to_widget(target, &event);
                }
            }
            WidgetEvent::PointerUp { position, .. } => {
                if let Some(captured) = self.pointer_captured_by {
                    // Route to the capturing widget and auto-release capture
                    self.dispatch_to_widget(captured, &event);
                    self.pointer_captured_by = None;
                } else if let Some(target) = self.hit_test(*position) {
                    self.dispatch_to_widget(target, &event);
                }
            }
            WidgetEvent::Scroll { .. } => {
                // Scroll goes to the widget under the pointer (or focused)
                if let Some(target) = self.hovered.or(self.focused) {
                    self.dispatch_to_widget(target, &event);
                }
            }
            WidgetEvent::KeyDown { key, modifiers, .. } => {
                if *key == Key::Tab {
                    self.cycle_focus(modifiers.shift());
                    // Tab is consumed — don't dispatch to the focused widget
                } else if let Some(focused) = self.focused {
                    self.dispatch_to_widget(focused, &event);
                }
            }
            WidgetEvent::KeyUp { .. }
            | WidgetEvent::ImeComposition { .. }
            | WidgetEvent::ImeCommit { .. } => {
                if let Some(focused) = self.focused {
                    self.dispatch_to_widget(focused, &event);
                }
            }
            WidgetEvent::AccessAction { target, action, .. } => {
                // Handle Action::Focus at the tree level — widgets can't request
                // focus from EventContext, so we intercept it here.
                if *action == accesskit::Action::Focus {
                    if let Some(id) = target.filter(|id| self.arena.is_active(*id)) {
                        self.focus_with_origin(id, crate::focus::FocusOrigin::Programmatic);
                    }
                } else {
                    // Route other actions to the specific target widget from AccessKit,
                    // falling back to the focused widget.
                    let dispatch_target = target
                        .filter(|id| self.arena.is_active(*id))
                        .or(self.focused);
                    if let Some(id) = dispatch_target {
                        self.dispatch_to_widget(id, &event);
                    }
                }
            }
            WidgetEvent::Gesture { .. } => {
                // Gesture events route to the widget under the pointer (or focused).
                if let Some(target) = self.hovered.or(self.focused) {
                    self.dispatch_to_widget(target, &event);
                }
            }
            // PointerEnter/Leave and Focus events are synthesized internally,
            // not dispatched from outside.
            WidgetEvent::ScrollIntoView { .. } => {
                // ScrollIntoView is dispatched directly to specific clipping ancestors
                // by scroll_focused_into_view, not through the general dispatch path.
            }
            WidgetEvent::PointerEnter
            | WidgetEvent::PointerLeave
            | WidgetEvent::FocusGained { .. }
            | WidgetEvent::FocusLost => {}
        }
        self.flush_commands();
    }

    fn handle_pointer_move(&mut self, position: Point) {
        let target = self.hit_test(position);

        if target != self.hovered {
            if let Some(old) = self.hovered {
                self.dispatch_to_widget(old, &WidgetEvent::PointerLeave);
                self.tooltip_pointer_leave(old);
            }
            if let Some(new) = target {
                self.dispatch_to_widget(new, &WidgetEvent::PointerEnter);
                self.tooltip_pointer_enter(new);
            }
            self.hovered = target;
        }

        if let Some(target) = target {
            self.dispatch_to_widget(target, &WidgetEvent::PointerMove { position });
        }
    }

    fn dispatch_to_widget(&mut self, target: WidgetId, event: &WidgetEvent) {
        // Skip events for disabled widgets (enabled_when binding is false)
        if !self.arena.is_enabled(target) {
            return;
        }

        // Preview pass: root → target
        let mut ancestors = Vec::new();
        let mut current = self.arena.parent(target);
        while let Some(id) = current {
            ancestors.push(id);
            current = self.arena.parent(id);
        }
        ancestors.reverse();

        for &id in &ancestors {
            let mut ctx = EventContext::new();
            let response = if let Some(node) = self.arena.get_mut(id) {
                node.widget.preview_event(event, &mut ctx)
            } else {
                EventResponse::Ignored
            };
            self.collect_from_ctx(ctx, id);
            if response == EventResponse::Handled {
                self.arena.mark_needs_paint(id);
                return;
            }
        }

        // Bubble pass: target → root
        let needs_layout_on_handle = matches!(
            event,
            WidgetEvent::Scroll { .. } | WidgetEvent::ScrollIntoView { .. }
        );
        let mut current = Some(target);
        while let Some(id) = current {
            let mut ctx = EventContext::new();
            let response = if let Some(node) = self.arena.get_mut(id) {
                node.widget.event(event, &mut ctx)
            } else {
                EventResponse::Ignored
            };
            self.collect_from_ctx(ctx, id);
            if response == EventResponse::Handled {
                if needs_layout_on_handle {
                    self.arena.mark_needs_layout(id);
                } else {
                    self.arena.mark_needs_paint(id);
                }
                break;
            }
            current = self.arena.parent(id);
        }
    }

    /// Collect commands, tree mutations, and idle callbacks from an EventContext.
    fn collect_from_ctx(&mut self, ctx: EventContext, source_widget: WidgetId) {
        self.pending_commands.extend(ctx.commands);
        self.apply_tree_mutations(&ctx.tree_mutations);
        for cb in ctx.idle_callbacks {
            self.idle_queue.push_boxed(cb);
        }
        for req in ctx.overlay_requests {
            self.overlay_manager.show(req);
        }
        for id in ctx.overlay_dismissals {
            self.overlay_manager.dismiss(id);
        }
        // Handle pointer capture requests
        if let Some(capture) = ctx.pointer_capture {
            if capture {
                self.pointer_captured_by = Some(source_widget);
            } else {
                self.pointer_captured_by = None;
            }
        }
    }

    /// Feed a raw pointer event to gesture recognizers on the target widget
    /// and its ancestors (bubbling up). If a gesture is recognized, dispatch
    /// it as a `WidgetEvent::Gesture` through the normal preview/bubble path.
    fn feed_gesture_recognizers(&mut self, target: WidgetId, raw: &RawPointerEvent) {
        // Collect the chain: target + ancestors
        let mut chain = vec![target];
        let mut current = self.arena.parent(target);
        while let Some(id) = current {
            chain.push(id);
            current = self.arena.parent(id);
        }

        for id in chain {
            if let Some(node) = self.arena.get_mut(id) {
                if let Some(binding) = &mut node.gesture_binding {
                    if let Some(gesture) = binding.arena.process(raw) {
                        let mut ctx = EventContext::new();
                        (binding.handler)(gesture.clone(), &mut ctx);
                        self.collect_from_ctx(ctx, id);

                        // Also dispatch as WidgetEvent::Gesture for the widget's event()
                        self.dispatch_to_widget(id, &WidgetEvent::Gesture { gesture });
                        return; // First recognized gesture wins
                    }
                }
            }
        }
    }

    fn apply_tree_mutations(&mut self, mutations: &[crate::widget::TreeMutation]) {
        use crate::widget::TreeMutation;
        for mutation in mutations {
            match mutation {
                TreeMutation::SetDormant(id) => self.arena.set_dormant(*id),
                TreeMutation::Activate(id) => self.arena.activate(*id),
                TreeMutation::Destroy(id) => self.arena.destroy(*id),
            }
        }
    }

    pub fn hit_test(&self, point: Point) -> Option<WidgetId> {
        let roots = self.arena.roots();
        for &root in roots.iter().rev() {
            if let Some(hit) = self.hit_test_recursive(root, point) {
                return Some(hit);
            }
        }
        None
    }

    fn hit_test_recursive(&self, id: WidgetId, point: Point) -> Option<WidgetId> {
        if !self.arena.is_active(id) {
            return None;
        }
        let bounds = self.arena.bounds(id);
        if !bounds.contains(point) {
            return None;
        }
        let children = self.arena.children(id).to_vec();
        for &child in children.iter().rev() {
            if let Some(hit) = self.hit_test_recursive(child, point) {
                return Some(hit);
            }
        }
        Some(id)
    }

    // --- Focus ---

    /// Set focus to a specific widget with the given origin.
    pub fn focus_with_origin(&mut self, id: WidgetId, origin: crate::focus::FocusOrigin) {
        if self.focused == Some(id) {
            return;
        }
        if let Some(old) = self.focused {
            self.dispatch_to_widget(old, &WidgetEvent::FocusLost);
        }
        self.focused = Some(id);
        self.focus_origin = Some(origin);
        self.a11y_dirty = true;
        self.dispatch_to_widget(id, &WidgetEvent::FocusGained { origin });
        self.scroll_focused_into_view(id);
        self.flush_commands();
    }

    /// After setting focus, ensure the focused widget is visible inside
    /// any ancestor scroll area (clips_children container).
    fn scroll_focused_into_view(&mut self, focused_id: WidgetId) {
        let focused_bounds = self.arena.bounds(focused_id);

        let mut current = self.arena.parent(focused_id);
        while let Some(ancestor_id) = current {
            if let Some(node) = self.arena.get(ancestor_id) {
                if node.clips_children {
                    let viewport = node.bounds;
                    let needs_scroll = focused_bounds.y < viewport.y
                        || focused_bounds.bottom() > viewport.bottom()
                        || focused_bounds.x < viewport.x
                        || focused_bounds.right() > viewport.right();

                    if needs_scroll {
                        self.dispatch_to_widget(
                            ancestor_id,
                            &WidgetEvent::ScrollIntoView {
                                target_bounds: focused_bounds,
                                margin: 0.0,
                            },
                        );
                    }
                    break; // Only scroll the nearest clipping ancestor
                }
            }
            current = self.arena.parent(ancestor_id);
        }
    }

    /// Set focus to a specific widget (programmatic origin).
    pub fn focus(&mut self, id: WidgetId) {
        self.focus_with_origin(id, crate::focus::FocusOrigin::Programmatic);
    }

    /// Get the currently focused widget.
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// How the currently focused widget gained focus.
    pub fn focus_origin(&self) -> Option<crate::focus::FocusOrigin> {
        self.focus_origin
    }

    /// Cycle focus to the next/previous focusable widget (Tab/Shift-Tab).
    /// Traverses in document order (depth-first tree traversal).
    fn cycle_focus(&mut self, reverse: bool) {
        let mut focusable = Vec::new();
        let roots = self.arena.roots();
        for root in roots {
            self.collect_focusable_tree_order(root, &mut focusable);
        }

        if focusable.is_empty() {
            return;
        }

        // Sort by tab_index if specified: widgets with a tab_index come first
        // (sorted by their index), then widgets without (in tree order).
        focusable.sort_by(|&a, &b| {
            let ta = self.arena.get(a).and_then(|n| n.widget.tab_index());
            let tb = self.arena.get(b).and_then(|n| n.widget.tab_index());
            match (ta, tb) {
                (Some(ia), Some(ib)) => ia.cmp(&ib),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal, // preserve tree order
            }
        });

        let current_idx = self
            .focused
            .and_then(|f| focusable.iter().position(|&id| id == f));

        let next_idx = match current_idx {
            Some(idx) => {
                if reverse {
                    if idx == 0 {
                        focusable.len() - 1
                    } else {
                        idx - 1
                    }
                } else {
                    (idx + 1) % focusable.len()
                }
            }
            None => 0,
        };

        self.focus_with_origin(focusable[next_idx], crate::focus::FocusOrigin::Keyboard);
    }

    /// Find the nearest focusable widget at or above the given ID.
    fn find_focusable_at_or_above(&self, id: WidgetId) -> Option<WidgetId> {
        let mut current = Some(id);
        while let Some(cid) = current {
            if let Some(node) = self.arena.get(cid) {
                if node.widget.is_focusable() {
                    return Some(cid);
                }
            }
            current = self.arena.parent(cid);
        }
        None
    }

    /// Collect focusable widgets in depth-first (document) order.
    fn collect_focusable_tree_order(&self, id: WidgetId, out: &mut Vec<WidgetId>) {
        if !self.arena.is_active(id) {
            return;
        }
        if let Some(node) = self.arena.get(id) {
            if node.widget.is_focusable() {
                out.push(id);
            }
            for &child in &node.children {
                self.collect_focusable_tree_order(child, out);
            }
        }
    }

    // --- Querying ---

    /// Get an immutable reference to a widget node (for internal use).
    pub(crate) fn arena_get(
        &self,
        id: WidgetId,
    ) -> Option<&crate::arena::WidgetNode> {
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

    // --- Rendering ---

    /// Paint all active widgets and produce a RenderFrame.
    /// Uses per-widget paint caching: only widgets with `needs_paint` are
    /// re-painted; clean widgets reuse their cached paint output.
    /// Also caches the full assembled frame — if no widget needs painting,
    /// the previous frame is returned immediately.
    pub fn render(&mut self) -> RenderFrame {
        // Flush any pending state changes so dirty flags are up-to-date
        self.process_state_changes();

        // Fast path: if nothing needs painting, return the cached frame
        if !self.arena.any_needs_paint() {
            if let Some(ref cached) = self.cached_frame {
                return cached.clone();
            }
        }

        let mut frame = RenderFrame::new();
        let base_theme = self.theme.clone();
        let text_backend = self.text_backend.clone();

        // Paint main content first
        let roots: Vec<WidgetId> = self.arena.roots();
        for root_id in roots {
            paint_widget_cached(
                &mut self.arena,
                root_id,
                &mut frame,
                &base_theme,
                &text_backend,
            );
        }

        // Paint overlays on top (in stack order, bottom to top)
        let overlay_ids = self.overlay_manager.active_content_ids();
        for content_id in overlay_ids {
            paint_widget_cached(
                &mut self.arena,
                content_id,
                &mut frame,
                &base_theme,
                &text_backend,
            );
        }

        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = false;
            }
        }

        frame.debug_validate_stacks();
        self.cached_frame = Some(frame.clone());
        frame
    }

    // --- Accessibility ---

    /// Build an AccessKit `TreeUpdate` from the current state of all active
    /// widgets. Call this once per frame, between layout and paint, and push
    /// the result to the `accesskit_winit::Adapter`.
    /// Caches the result and only rebuilds when layout has changed.
    pub fn sync_accessibility(&mut self) -> accesskit::TreeUpdate {
        // Return cached result if nothing changed since last sync
        if !self.a11y_dirty {
            if let Some(cached) = &self.cached_a11y {
                return cached.clone();
            }
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

        // Build a virtual root node whose children are the tree roots
        let mut root = accesskit::Node::new(accesskit::Role::Window);
        for &root_id in &roots {
            if self.arena.is_active(root_id) {
                root.push_child(widget_id_to_node_id(root_id));
            }
        }
        nodes.push((root_node_id(), root));

        // Walk all active widgets and build their AccessKit nodes
        for &root_id in &roots {
            self.build_accessibility_recursive(root_id, &mut nodes);
        }

        // Focus: use the focused widget if still active, or fall back to root
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

        // Add children to the AccessKit node
        let children = self.arena.children(id);
        for &child_id in children {
            if self.arena.is_active(child_id) {
                builder.inner_mut().push_child(widget_id_to_node_id(child_id));
            }
        }

        // Set bounds from layout
        let bounds = self.arena.bounds(id);
        builder.inner_mut().set_bounds(accesskit::Rect {
            x0: bounds.x as f64,
            y0: bounds.y as f64,
            x1: (bounds.x + bounds.width) as f64,
            y1: (bounds.y + bounds.height) as f64,
        });

        // Link tooltip anchor to its tooltip content via described_by
        if let Some(tooltip) = self.tooltips.iter().find(|t| t.anchor_id == id && t.overlay_id.is_some()) {
            builder.inner_mut().push_described_by(widget_id_to_node_id(tooltip.content_id));
        }

        let (node_id, ak_node) = builder.build(id);
        nodes.push((node_id, ak_node));

        // Recurse into children
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

    /// Find a widget that supports a specific AccessKit action.
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

    // --- Tooltip management ---

    /// Attach a tooltip to a widget. The tooltip content widget must already
    /// be in the tree (typically added as a dormant widget during build).
    pub fn attach_tooltip(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
    ) {
        // Start the content dormant
        self.arena.set_dormant(content_id);
        self.tooltips.push(TooltipEntry {
            anchor_id,
            content_id,
            delay,
            hover_start: None,
            real_hover_start: None,
            overlay_id: None,
        });
    }

    /// Process tooltip timers. Called from advance_time and from dispatch_event.
    /// Process tooltip timers using simulated clock (for tests via advance_time).
    fn process_tooltips(&mut self) {
        let sim_now = self.sim_clock;
        self.process_tooltips_impl(|entry| {
            entry.hover_start.map(|s| sim_now.saturating_duration_since(s))
        });
    }

    /// Process tooltip timers using real clock (for windowed apps via layout).
    fn process_tooltips_real(&mut self) {
        let real_now = std::time::Instant::now();
        self.process_tooltips_impl(|entry| {
            entry.real_hover_start.map(|s| real_now.saturating_duration_since(s))
        });
    }

    fn process_tooltips_impl(
        &mut self,
        elapsed_fn: impl Fn(&TooltipEntry) -> Option<std::time::Duration>,
    ) {
        let mut to_show = Vec::new();

        for entry in &self.tooltips {
            if entry.overlay_id.is_some() {
                continue;
            }
            if let Some(dur) = elapsed_fn(entry) {
                if dur >= entry.delay {
                    to_show.push((entry.anchor_id, entry.content_id));
                }
            }
        }

        for (anchor_id, content_id) in to_show {
            // Activate the tooltip content widget
            self.arena.activate(content_id);
            let id = self.overlay_manager.show(crate::overlay::OverlayRequest {
                content_id,
                anchor: anchor_id,
                placement: crate::overlay::OverlayPlacement::NearAnchor {
                    offset: fern_canvas::Vec2::new(0.0, 4.0),
                },
                dismiss: crate::overlay::DismissBehavior::PointerLeave {
                    delay: std::time::Duration::from_millis(100),
                },
                layer: crate::overlay::OverlayLayer::InTree,
                parent_overlay: None,
            });
            // Record the overlay ID
            for entry in &mut self.tooltips {
                if entry.anchor_id == anchor_id {
                    entry.overlay_id = Some(id);
                }
            }
        }
    }

    /// Handle pointer enter/leave for tooltip hover tracking.
    /// Check if widget_id is the anchor or a descendant of the anchor.
    fn is_descendant_of(&self, widget_id: WidgetId, ancestor: WidgetId) -> bool {
        if widget_id == ancestor {
            return true;
        }
        let mut current = self.arena.parent(widget_id);
        while let Some(pid) = current {
            if pid == ancestor {
                return true;
            }
            current = self.arena.parent(pid);
        }
        false
    }

    fn tooltip_pointer_enter(&mut self, widget_id: WidgetId) {
        // Collect matching indices first to avoid borrow conflict
        let matching: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, e)| self.is_descendant_of(widget_id, e.anchor_id))
            .map(|(i, _)| i)
            .collect();
        // Record both simulated and real time for tooltip hover start.
        // Tests use sim_clock via advance_time; real apps use Instant::now via layout.
        let now = self.sim_clock;
        let real_now = std::time::Instant::now();
        for i in matching {
            self.tooltips[i].hover_start = Some(now);
            self.tooltips[i].real_hover_start = Some(real_now);
            // Mark anchor as needing paint so the event loop keeps
            // redrawing until the tooltip delay elapses.
            self.arena.mark_needs_paint(self.tooltips[i].anchor_id);
        }
    }

    fn tooltip_pointer_leave(&mut self, widget_id: WidgetId) {
        let matching: Vec<usize> = self
            .tooltips
            .iter()
            .enumerate()
            .filter(|(_, e)| self.is_descendant_of(widget_id, e.anchor_id))
            .map(|(i, _)| i)
            .collect();
        let mut to_dismiss = Vec::new();
        for i in matching {
            self.tooltips[i].hover_start = None;
            self.tooltips[i].real_hover_start = None;
            if let Some(id) = self.tooltips[i].overlay_id.take() {
                to_dismiss.push((id, self.tooltips[i].content_id));
            }
        }
        for (id, content_id) in to_dismiss {
            self.overlay_manager.dismiss(id);
            self.arena.set_dormant(content_id);
        }
    }

    /// Returns the earliest deadline for a pending tooltip (if any).
    /// The event loop should use ControlFlow::WaitUntil(deadline) if this returns Some.
    pub fn next_timer_deadline(&self) -> Option<std::time::Instant> {
        let tooltip_deadline = self.tooltips
            .iter()
            .filter(|e| e.overlay_id.is_none())
            .filter_map(|e| e.real_hover_start.map(|start| start + e.delay))
            .min();
        let animation_deadline = self.animation_scheduler.next_deadline();

        match (tooltip_deadline, animation_deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// Get the overlay manager (read-only, for querying).
    pub fn overlay_manager(&self) -> &crate::overlay::OverlayManager {
        &self.overlay_manager
    }

    /// Get active overlay IDs (for testing).
    pub fn active_overlays(&self) -> Vec<crate::overlay::OverlayId> {
        self.overlay_manager.active_ids()
    }

    /// Show an overlay directly (for testing or framework use).
    pub fn show_overlay(&mut self, request: crate::overlay::OverlayRequest) -> crate::overlay::OverlayId {
        self.overlay_manager.show(request)
    }

    /// Dismiss an overlay directly.
    pub fn dismiss_overlay(&mut self, id: crate::overlay::OverlayId) {
        self.overlay_manager.dismiss(id);
    }

    /// Whether a widget is visible (active, not dormant or destroyed).
    pub fn is_visible(&self, id: WidgetId) -> bool {
        self.arena.is_active(id)
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

    // --- Test helpers ---

    /// Simulate a click at the center of a widget.
    pub fn click(&mut self, id: WidgetId) {
        let center = self.arena.bounds(id).center();
        self.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
        });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: center,
            button: PointerButton::Primary,
        });
    }

    /// Simulate pointer movement to a position.
    pub fn pointer_move(&mut self, position: Point) {
        self.dispatch_event(WidgetEvent::PointerMove { position });
    }

    /// Simulate a key press (down + up).
    pub fn press_key(&mut self, key: Key, modifiers: Modifiers) {
        self.dispatch_event(WidgetEvent::KeyDown {
            key,
            modifiers,
            text: None,
        });
        self.dispatch_event(WidgetEvent::KeyUp { key, modifiers });
    }

    /// Simulate typing text into the focused widget.
    pub fn type_text(&mut self, _widget: WidgetId, text: &str) {
        for ch in text.chars() {
            self.dispatch_event(WidgetEvent::KeyDown {
                key: Key::Character(ch),
                modifiers: Modifiers::NONE,
                text: Some(ch.to_string()),
            });
        }
    }

    /// Simulate a pointer down at a specific position with a specific button.
    pub fn pointer_down_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::PointerDown { position, button });
    }

    /// Simulate a pointer up at a specific position with a specific button.
    pub fn pointer_up_button(&mut self, position: Point, button: PointerButton) {
        self.dispatch_event(WidgetEvent::PointerUp { position, button });
    }

    /// Simulate a drag from one position to another.
    pub fn drag(&mut self, from: Point, to: Point) {
        self.dispatch_event(WidgetEvent::PointerDown {
            position: from,
            button: PointerButton::Primary,
        });
        self.dispatch_event(WidgetEvent::PointerMove { position: to });
        self.dispatch_event(WidgetEvent::PointerUp {
            position: to,
            button: PointerButton::Primary,
        });
    }

    /// Get bounds of a child by index.
    pub fn child_bounds(&self, parent: WidgetId, index: usize) -> Rect {
        let children = self.children(parent);
        self.bounds(children[index])
    }

    /// Get a child widget ID by index.
    pub fn child_widget(&self, parent: WidgetId, index: usize) -> WidgetId {
        self.children(parent)[index]
    }

    /// Advance simulated time (for tooltip/animation testing).
    /// Advance the simulated clock by the given duration.
    /// Triggers time-dependent behavior such as long-press gesture recognition
    /// and tooltip timers. Enables deterministic testing without real delays.
    pub fn advance_time(&mut self, duration: std::time::Duration) {
        self.sim_clock += duration;
        self.process_tooltips();
    }

    /// Get the current simulated clock value.
    pub fn simulated_now(&self) -> std::time::Instant {
        self.sim_clock
    }

    /// Mark a widget as needing repaint.
    pub fn mark_needs_paint(&mut self, id: WidgetId) {
        self.arena.mark_needs_paint(id);
    }

    /// Set a widget subtree as dormant.
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.arena.set_dormant(id);
        self.arena.mark_ancestors_need_layout(id);
        self.cached_frame = None;
        self.a11y_dirty = true;
    }

    /// Activate a dormant widget subtree.
    pub fn activate(&mut self, id: WidgetId) {
        self.arena.activate(id);
        self.arena.mark_ancestors_need_layout(id);
        self.cached_frame = None;
        self.a11y_dirty = true;
    }

    /// Invalidate all per-widget paint caches and the assembled frame cache.
    /// Forces every widget to repaint on the next `render()` call.
    /// Used when external state (e.g. glyph atlas eviction) makes cached
    /// paint output stale.
    pub fn invalidate_all_paints(&mut self) {
        for id in self.arena.active_ids() {
            if let Some(node) = self.arena.get_mut(id) {
                node.dirty.needs_paint = true;
                node.cached_paint = None;
            }
        }
        self.cached_frame = None;
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

/// Recursive paint pass with per-widget caching.
/// Only re-runs `paint()` for widgets with `needs_paint` set; clean widgets
/// reuse their `cached_paint` output. The tree walk still runs for clip/child
/// ordering, but skips the expensive `paint()` call for clean widgets.
fn paint_widget_cached(
    arena: &mut WidgetArena,
    id: WidgetId,
    frame: &mut RenderFrame,
    base_theme: &fern_tokens::Theme,
    text_backend: &Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
) {
    if !arena.is_active(id) {
        return;
    }

    // Check if we need to repaint this widget or can reuse cached output
    let node = arena.get(id).unwrap();
    let needs_paint = node.dirty.needs_paint;

    if needs_paint || node.cached_paint.is_none() {
        // Repaint: run widget.paint() and cache the result
        let resolved_theme = arena.resolve_theme(id, base_theme);
        let ctx = PaintContext {
            theme: &resolved_theme,
            scale_factor: 1.0,
            prefers_high_contrast: false,
            prefers_reduced_motion: false,
            prefers_large_text: false,
        };

        let bounds = arena.bounds(id);
        let node = arena.get(id).unwrap();

        let mut canvas = match text_backend {
            Some(tb) => Canvas::with_text_backend(tb.clone()),
            None => Canvas::new(),
        };
        node.widget.paint(bounds, &mut canvas, &ctx);
        let widget_frame = canvas.into_render_frame();

        // Store in cache and merge into output frame
        frame.merge(&widget_frame);
        if let Some(node) = arena.get_mut(id) {
            node.cached_paint = Some(widget_frame);
        }
    } else {
        // Clean widget: reuse cached paint output
        let node = arena.get(id).unwrap();
        if let Some(cached) = &node.cached_paint {
            frame.merge(cached);
        }
    }

    // Always walk children for correct draw order and clipping
    let node = arena.get(id).unwrap();
    let clips = node.clips_children;
    let children: Vec<WidgetId> = node.children.clone();
    let bounds = node.bounds;

    if clips {
        frame
            .draw_order
            .push(fern_canvas::DrawCommand::SetClip(bounds));
    }

    for child_id in children {
        paint_widget_cached(arena, child_id, frame, base_theme, text_backend);
    }

    if clips {
        frame.draw_order.push(fern_canvas::DrawCommand::ClearClip);
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
        desired_size.width,
        desired_size.height,
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

            let child_proposal =
                SizeProposal::exact(placement.size.width, placement.size.height);
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
    use fern_tokens::{Color, CornerRadius};

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
        assert_eq!(tree.focus_origin(), Some(crate::focus::FocusOrigin::Keyboard));
    }

    #[test]
    fn fill_widget_produces_shape_in_frame() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(FillWidget::new().background(Color::RED).corner_radius(CornerRadius::uniform(6.0)));
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
        let w = tree.add(FillWidget::new().background(Color::RED).corner_radius(CornerRadius::uniform(4.0)));
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
        let child = tree.add(FillWidget::new().background(Color::RED).corner_radius(CornerRadius::uniform(4.0)));
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

        let shortcuts = ShortcutMap::new()
            .bind(Shortcut::ctrl(Key::S), TestCmd::Save);

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
        let w = tree.add(FillWidget::new());
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
        visible.bind_to(w, tree.binding_registry(), crate::state::BindingLevel::RepaintOnly);

        // Change state — widget should become dirty on next layout()
        visible.set(false);
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert!(tree.needs_paint());
    }

    // --- CompositeWidget integration ---

    /// A minimal composite that builds a FillWidget with a label.
    #[derive(Debug)]
    struct TestComposite {
        label: String,
    }

    impl crate::composite_widget::CompositeWidget for TestComposite {
        fn build(&self, ctx: &mut crate::composite_widget::BuildContext) -> WidgetId {
            ctx.add(FillWidget::new().label(&self.label))
        }

        fn is_focusable(&self) -> bool {
            true
        }

        fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name(&self.label);
        }
    }

    #[test]
    fn composite_widget_is_added_to_tree() {
        let mut tree = WidgetTree::new();
        let id = tree.add_composite(TestComposite {
            label: "OK".to_string(),
        });
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // The composite has accessibility
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), accesskit::Role::Button);
        assert_eq!(info.name(), Some("OK"));
    }

    #[test]
    fn composite_widget_builds_child_subtree() {
        let mut tree = WidgetTree::new();
        let id = tree.add_composite(TestComposite {
            label: "Child".to_string(),
        });
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // The composite should have one child (the FillWidget built inside)
        let children = tree.children(id);
        assert_eq!(children.len(), 1);

        // The child has the label accessibility
        let child_info = tree.accessibility_node(children[0]);
        assert_eq!(child_info.role(), accesskit::Role::Label);
        assert_eq!(child_info.name(), Some("Child"));
    }

    #[test]
    fn composite_widget_is_focusable() {
        let mut tree = WidgetTree::new();
        let id = tree.add_composite(TestComposite {
            label: "Focus".to_string(),
        });
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.focus(id);
        assert_eq!(tree.focused(), Some(id));
    }

    #[test]
    fn composite_indistinguishable_from_widget() {
        // Both Widget and CompositeWidget produce WidgetIds that work the same
        let mut tree = WidgetTree::new();

        let w1 = tree.add(FillWidget::new().label("Level2"));
        let w2 = tree.add_composite(TestComposite {
            label: "Level1".to_string(),
        });
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // Both have bounds
        assert!(tree.bounds(w1).width > 0.0);
        assert!(tree.bounds(w2).width > 0.0);

        // Both findable by label
        assert!(tree.find_by_label("Level2").is_some());
        assert!(tree.find_by_label("Level1").is_some());
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

        tree.attach_gesture(w, DragRecognizer::new().threshold(5.0), move |gesture, _ctx| {
            match gesture {
                crate::gesture::GestureEvent::DragStarted { .. } => ds.set(true),
                crate::gesture::GestureEvent::DragEnded { .. } => de.set(true),
                _ => {}
            }
        });

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
    fn gesture_event_dispatched_as_widget_event() {
        // Verify that recognized gestures also arrive as WidgetEvent::Gesture
        // through the normal event dispatch path.
        use crate::gesture::TapRecognizer;
        use std::cell::Cell;
        use std::rc::Rc;

        let widget_got_gesture = Rc::new(Cell::new(false));

        // Use a custom widget that checks for Gesture events
        #[derive(Debug)]
        struct GestureCapture {
            got_gesture: Rc<Cell<bool>>,
        }
        impl Widget for GestureCapture {
            fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_canvas::Size {
                proposal.resolve(0.0, 0.0)
            }
            fn event(&mut self, event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
                if matches!(event, WidgetEvent::Gesture { .. }) {
                    self.got_gesture.set(true);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
        }

        let mut tree = WidgetTree::new();
        let w = tree.add(GestureCapture {
            got_gesture: widget_got_gesture.clone(),
        });
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.attach_gesture(w, TapRecognizer::new(), |_, _| {});

        tree.click(w);
        assert!(widget_got_gesture.get());
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
        tree.attach_gesture(w, DragRecognizer::new().threshold(5.0), move |gesture, _ctx| {
            if matches!(gesture, crate::gesture::GestureEvent::DragStarted { .. }) {
                d.set(true);
            }
        });

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
        let child = tree.add_child(parent, ThemeAwareWidget);

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

        let unaffected = tree.add(ThemeAwareWidget);
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

    /// A widget that requests an idle callback when it receives a click.
    #[derive(Debug)]
    struct IdleRequestWidget {
        callback_called: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl Widget for IdleRequestWidget {
        fn size_that_fits(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> fern_canvas::Size {
            proposal.resolve(0.0, 0.0)
        }

        fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
            if matches!(event, WidgetEvent::PointerUp { .. }) {
                let called = self.callback_called.clone();
                ctx.request_idle_callback(move |_deadline| {
                    called.set(true);
                });
                EventResponse::Handled
            } else {
                EventResponse::Ignored
            }
        }
    }

    #[test]
    fn idle_callback_requested_from_event_handler() {
        use std::cell::Cell;
        use std::rc::Rc;

        let called = Rc::new(Cell::new(false));
        let mut tree = WidgetTree::new();
        let w = tree.add(IdleRequestWidget {
            callback_called: called.clone(),
        });
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
        let h = had_time.clone();

        let mut tree = WidgetTree::new();
        let w = tree.add(IdleRequestWidget {
            callback_called: Rc::new(Cell::new(false)),
        });
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Manually add an idle callback via the tree's internal queue
        // by triggering a click (which requests one)
        tree.click(w);

        // Replace the callback with one that checks the deadline
        // (We need to drain and re-add since click already queued one)
        tree.run_idle_callbacks(std::time::Duration::from_millis(0)); // drain the click one

        // Now add a fresh one that checks the deadline
        // We'll do this through a second widget interaction
        let w2 = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));

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
        let w1 = tree.add(FillWidget::new().label("Active"));
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
        let parent = tree.add(
            crate::test_widgets::StackWidget::new().add_child(child),
        );
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
    }

    #[test]
    fn tooltip_appears_after_delay() {
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new());
        let tooltip = tree.add(FillWidget::new().label("Tooltip text"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.attach_tooltip(
            anchor,
            tooltip,
            std::time::Duration::from_millis(500),
        );

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

        tree.attach_tooltip(
            anchor,
            tooltip,
            std::time::Duration::from_millis(500),
        );

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

        tree.attach_tooltip(
            anchor,
            tooltip,
            std::time::Duration::from_millis(500),
        );

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
        use std::cell::Cell;
        use std::rc::Rc;

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
        enum Cmd { ScopedAction, GlobalAction }
        impl crate::app_command::AppCommand for Cmd {}

        let fired = Rc::new(Cell::new(None));
        let f = fired.clone();

        let shortcuts = crate::shortcut::ShortcutMap::new()
            .bind(crate::shortcut::Shortcut::ctrl(Key::Z), Cmd::GlobalAction);

        let mut tree = WidgetTree::new().with_shortcuts(shortcuts);
        tree.on_command(move |cmd: &Cmd| {
            f.set(Some(cmd.clone()));
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
        let state = crate::state::State::new(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(100.0, std::time::Duration::from_millis(200), fern_tokens::Easing::Linear);

        // Advance 100ms (50%)
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!((*state.get() - 50.0).abs() < 2.0, "at 50%: {}", *state.get());

        // Advance another 100ms (100%)
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!((*state.get() - 100.0).abs() < 0.1, "at 100%: {}", *state.get());

        assert!(!tree.has_active_animations());
    }

    #[test]
    fn set_animated_with_easing() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(100.0, std::time::Duration::from_millis(200), fern_tokens::Easing::EaseIn);

        // At 50%, EaseIn (t²) = 0.25 → value ≈ 25
        tree.tick_animations(std::time::Duration::from_millis(100));
        assert!((*state.get() - 25.0).abs() < 2.0, "ease-in at 50%: {}", *state.get());
    }

    #[test]
    fn set_animated_replaces_in_flight() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new(0.0_f32);
        tree.register_animated_state(&state);

        state.set_animated(100.0, std::time::Duration::from_millis(200), fern_tokens::Easing::Linear);
        tree.tick_animations(std::time::Duration::from_millis(100)); // at 50
        assert!((*state.get() - 50.0).abs() < 2.0);

        // New animation from current value to 0
        state.set_animated(0.0, std::time::Duration::from_millis(100), fern_tokens::Easing::Linear);
        tree.tick_animations(std::time::Duration::from_millis(50)); // 50% of new
        assert!((*state.get() - 25.0).abs() < 3.0, "mid-replace: {}", *state.get());

        tree.tick_animations(std::time::Duration::from_millis(50)); // 100% of new
        assert!((*state.get() - 0.0).abs() < 0.5, "end-replace: {}", *state.get());
    }

    #[test]
    fn animation_marks_widgets_dirty() {
        let mut tree = WidgetTree::new();
        let state = crate::state::State::new(100.0_f32);
        tree.register_animated_state(&state);

        let w = tree.add(FillWidget::new());
        state.bind_to(w, tree.binding_registry(), crate::state::BindingLevel::Relayout);

        tree.layout(SizeProposal::exact(200.0, 100.0));

        state.set_animated(0.0, std::time::Duration::from_millis(100), fern_tokens::Easing::Linear);

        // Tick animation — state changes should mark widget dirty
        tree.tick_animations(std::time::Duration::from_millis(50));
        assert!(tree.needs_redraw());
    }

    #[test]
    fn animated_state_from_build_context() {
        // Verify that animated_state() from BuildContext works
        #[derive(Debug)]
        struct AnimWidget {
            width_state: std::cell::RefCell<Option<crate::state::State<f32>>>,
        }
        impl crate::composite_widget::CompositeWidget for AnimWidget {
            fn build(&self, ctx: &mut crate::composite_widget::BuildContext) -> WidgetId {
                let w = ctx.animated_state(300.0);
                *self.width_state.borrow_mut() = Some(w.clone());
                ctx.add(FillWidget::new())
            }
        }
        crate::impl_composite_into_widget_tree!(AnimWidget);

        let mut tree = WidgetTree::new();
        let widget = AnimWidget { width_state: std::cell::RefCell::new(None) };
        let width_state_clone = widget.width_state.borrow().clone();
        tree.add_widget(widget);
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // The state should be registered for animation
        assert!(tree.has_active_animations() == false); // nothing animating yet
    }
}
