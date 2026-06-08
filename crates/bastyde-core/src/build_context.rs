//! BuildContext — context available during Widget::build().
//!
//! Provides Signal-based APIs for creating reactive state, registering
//! effects, and adding child widgets during the build lifecycle.

use crate::binding::BindingRegistry;
use crate::event_source::{SubscriptionHandle, SubscriptionId};
use crate::signal::{ObserverHandle, Signal};
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
    /// Registered with the animation scheduler automatically. The owning
    /// widget (`self_id()`) is recorded so that the scheduler can pause
    /// the animation when the widget is offscreen, dormant, or rebuilt.
    pub fn animated_signal(&mut self, value: f32) -> Signal<f32> {
        let signal = Signal::new_animated(value);
        let owner = self.self_id();
        self.tree.register_animated_signal(&signal, owner);
        signal
    }

    /// Register a pre-existing `Signal<f32>` for animation support.
    /// Use this when the signal was created outside of `build()` (e.g. in the
    /// widget constructor) and needs to be registered with the animation scheduler.
    pub fn register_animated_signal(&mut self, signal: &Signal<f32>) {
        let owner = self.self_id();
        self.tree.register_animated_signal(signal, owner);
    }

    /// Read the OS-level `prefers-reduced-motion` preference. Widgets
    /// that use looping or decorative animations (spinners, sprite
    /// icons, marquee text, etc.) should skip starting them when this
    /// returns `true` so the UI respects accessibility settings and —
    /// as a bonus — draws no CPU/GPU.
    pub fn prefers_reduced_motion(&self) -> bool {
        self.tree.prefers_reduced_motion()
    }

    /// Build an [`AnimationSpec`](crate::animation_builder::AnimationSpec)
    /// — the fluent ergonomic façade over `Signal<f32>::animate_to`.
    /// Captures the theme's `MotionTokens` and the platform
    /// reduced-motion preference at build time, returns a clonable
    /// spec that event-handler closures can drive without
    /// re-threading durations and easing.
    ///
    /// ```ignore
    /// let knob_anim = ctx.animate().fast().standard();
    /// handlers = handlers.on_tap(move |_, _| {
    ///     knob_anim.to_or_snap(&knob_position, target);
    /// });
    /// ```
    pub fn animate(&self) -> crate::animation_builder::AnimationSpec {
        crate::animation_builder::AnimationSpec::from_motion(
            self.theme().motion.clone(),
            self.prefers_reduced_motion(),
        )
    }

    /// Opt into the shader-driven animated-quad pipeline. The widget
    /// paint() emits ONE `canvas.draw_animated_quad(bounds, handle.slot(),
    /// class)` call; the renderer samples per-slot state from its
    /// uniform buffer each frame and the widget's paint() does not
    /// re-run for animation ticks — only on layout changes. The
    /// returned handle is stable for the widget-mount lifetime and
    /// should be stashed on `self` to thread to `paint()`.
    ///
    /// For decorative motion that isn't a quad (scroll-offset tweens,
    /// sidebar slide, toggle knob), keep using `ctx.animated_signal` +
    /// `signal.animate_looping` — both paths coexist.
    pub fn animated_quad(
        &mut self,
        kind: crate::animated_quad::AnimatedQuadKind,
    ) -> crate::animated_quad::AnimatedQuadHandle {
        let owner = self.self_id();
        self.tree.register_animated_quad(owner, kind)
    }

    /// The per-frame delta-seconds signal. Observe it via
    /// `ctx.effect(&ctx.frame_tick(), |delta| ...)` to run code once per
    /// frame **the tree was explicitly asked to pump**. Merely observing
    /// this signal does not keep the event loop awake — widgets must
    /// call [`request_frame`](Self::request_frame) (typically from an
    /// event handler or from inside the tick closure itself) to schedule
    /// the next wake-up. This preserves Bastyde's draw-when-needed model.
    pub fn frame_tick(&self) -> Signal<f32> {
        self.tree.frame_tick()
    }

    /// Ask the tree to pump exactly one more frame. See
    /// [`frame_tick`](Self::frame_tick) for the observer side.
    pub fn request_frame(&self) {
        self.tree.request_frame();
    }

    /// Request that the AccessKit tree be re-walked after this build pass.
    /// Use when `build()` restructured its subtree in a way that changes the
    /// accessibility tree (relayout alone no longer re-walks AT). `SceneView`
    /// calls this each build, since it may have materialised or destroyed
    /// scene widgets or applied a11y-only scene mutations.
    pub fn request_accessibility_update(&self) {
        self.tree.request_accessibility_update();
    }

    /// Clone the shared "frame requested" flag. Stash it on widget
    /// state and call `.set(true)` from inside a frame-tick effect
    /// closure to chain-request another frame without needing
    /// mutable access to the tree. Used by widgets with continuous
    /// frame needs (caret blink, drag auto-scroll, smooth
    /// animations driven from a tick closure).
    ///
    /// **Prefer [`subscribe_frame_tick`](Self::subscribe_frame_tick)**
    /// for visual-only continuous animations (Pulse, Cycle, …): the
    /// scheduler-backed path automatically pauses the chain when the
    /// owner widget is hidden, while this raw handle keeps the event
    /// loop pumping at full frame rate regardless of visibility.
    pub fn frame_request_handle(&self) -> std::rc::Rc<std::cell::Cell<bool>> {
        self.tree.frame_request_handle()
    }

    /// Subscribe the widget being built to the per-frame-effect
    /// scheduler. The returned RAII guard removes the subscription on
    /// drop — store it on `self` so its lifetime tracks the widget's.
    ///
    /// While at least one subscriber's owner is visible, the framework
    /// auto-arms `frame_tick_requested` after every render. When all
    /// subscribers are hidden (e.g. parked inside a non-selected
    /// `Switcher` branch), no re-arm happens and the chain dies, so
    /// the event loop sleeps. On a hidden→visible transition the
    /// `visible_when` binding's relayout dirty triggers a repaint that
    /// paints the subscriber, which the post-render arm then detects
    /// and resumes the chain.
    ///
    /// Replaces the widget-managed `frame_request.set(true)` re-arm
    /// pattern for visual-only continuous animations. The widget's
    /// `frame_tick` effect closure no longer needs to call
    /// `frame_request.set(true)` itself — the scheduler handles it.
    pub fn subscribe_frame_tick(&self) -> crate::frame_tick_scheduler::FrameTickSubscription {
        let sub = self.tree.subscribe_frame_tick(self.self_id());
        // Bootstrap: ensure at least one frame runs after registration
        // so the first paint happens. The post-render re-arm takes over
        // from there. This is also the resume nudge for the case where
        // a widget rebuilds (e.g. due to a state change) while still
        // hidden — the parent's relayout dirty will trigger paint, and
        // post-render arm will pick up the chain.
        self.tree.request_frame();
        sub
    }

    /// Clone the shared wake-at deadline cell. Stash it on widget
    /// state and set `Some(instant)` from a frame-tick effect to
    /// schedule a one-shot deadline wake-up without keeping the event
    /// loop in `Poll` mode. See `WidgetTree::wake_at_handle` for
    /// the underlying mechanism.
    pub fn wake_at_handle(&self) -> std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>> {
        self.tree.wake_at_handle()
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

    /// Get the binding registry.
    pub fn binding_registry(&self) -> &BindingRegistry {
        self.tree.binding_registry()
    }

    /// Get the current theme.
    pub fn theme(&self) -> &crate::styles::Theme {
        self.tree.theme()
    }

    /// Reactive handle on the current theme. Fires observers when
    /// `tree.set_theme(...)` is called. Build implementations that want
    /// theme-driven values to update without a rebuild should use this
    /// instead of cloning tokens from `self.theme()` — for example,
    /// `ctx.theme_signal().map(|t| t.colors.primary)` or combining with
    /// interaction state via `zip(...)`.
    pub fn theme_signal(&self) -> crate::signal::Signal<crate::styles::Theme> {
        self.tree.theme_signal().clone()
    }

    /// Reactive handle on the current locale. Fires observers when
    /// `tree.set_locale(...)` is called.
    pub fn locale_signal(&self) -> crate::signal::Signal<Option<String>> {
        self.tree.locale_signal().clone()
    }

    /// The [`WindowState`](crate::window::WindowState) for the window
    /// hosting this tree. `None` only for trees built outside of an
    /// app (tests, headless scenarios). Use this to bind widgets to
    /// window-level signals like `placement`, `size`, `focused`.
    pub fn window(&self) -> Option<&crate::window::WindowState> {
        self.tree.window_state()
    }

    /// Retrieve an application-scoped value of type `T` registered via
    /// `BastydeAppBuilder::app_state`. Returns `None` if no value of
    /// that type was registered. The returned reference borrows from
    /// the framework for the duration of the build pass.
    pub fn app_state<T: 'static>(&self) -> Option<&T> {
        self.tree.app_context().app_state::<T>()
    }

    /// Borrow the [`AppEventPoster`](crate::AppEventPoster) installed by the
    /// framework, if any. Mirrors [`EventContext::poster`](crate::widget::EventContext::poster).
    /// Used by integrations that wire a platform callback (e.g. a native menu
    /// item) to post a typed payload back to the UI loop. Returns `None` for
    /// trees built outside an app (tests / headless).
    pub fn poster(&self) -> Option<&std::sync::Arc<dyn crate::AppEventPoster>> {
        self.tree.app_context().poster()
    }

    /// Bind a widget's visibility to a boolean prop or compatibility state binding.
    pub fn visible_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        self.tree.visible_when(id, state);
    }

    /// Observe a node's framework activation as a `Signal<bool>` — `true`
    /// while active, `false` while parked dormant by a `Switcher` /
    /// `visible_when` gate. Initialised to the node's current state and
    /// updated only on an actual Active↔Dormant transition.
    ///
    /// Ordinary widgets never need this: dormant subtrees are simply not
    /// painted, so they vanish for free. It exists for the one case where
    /// "not painted" ≠ "hidden" — a widget owning a native OS resource
    /// that renders *outside* the wgpu pass (a `WebView`'s engine subview).
    /// Such a widget does `ctx.effect(&ctx.activation_signal(id), move |a|
    /// handle.set_visible(*a))` to hide/show the native surface in lockstep.
    pub fn activation_signal(&mut self, id: WidgetId) -> Signal<bool> {
        self.tree.activation_signal(id)
    }

    /// Bind an opacity multiplier (0..1) to a widget. The render walker
    /// emits `SetOpacity(value)` before painting the widget's subtree
    /// and `RestoreOpacity` afterwards, so the multiplier composes
    /// correctly with ancestor opacity scopes. Bound at `RepaintOnly`:
    /// opacity changes never trigger relayout. Used by the `Fade`
    /// wrapper to animate a child between hidden and fully visible.
    pub fn set_opacity(&mut self, id: WidgetId, opacity: impl Into<crate::signal::Prop<f32>>) {
        self.tree.set_opacity(id, opacity);
    }

    /// Bind a 2D affine transform to a widget. The render walker emits
    /// `PushTransform(value)` before painting the widget's subtree and
    /// `PopTransform` afterwards, so the transform composes onto the
    /// renderer's stack with any ancestor transform scopes and with
    /// the widget's own canvas-level transforms. Bound at `RepaintOnly`:
    /// visual-only transforms never trigger relayout. Used by `Scale`
    /// and `Rotate`; reflow-driving wrappers (e.g. `Scale::reflow(true)`)
    /// must additionally bind their driver signal to themselves at
    /// `Relayout` to make layout track the value.
    pub fn set_transform(
        &mut self,
        id: WidgetId,
        transform: impl Into<crate::signal::Prop<bastyde_canvas::Transform2D>>,
    ) {
        self.tree.set_transform(id, transform);
    }

    /// Bind a 2D affine **content** transform to a widget — the transform
    /// positions the widget's content within its fixed parent-space viewport
    /// (its bounds) rather than transforming the widget itself. Renders the
    /// same `PushTransform` / `PopTransform` scope as
    /// [`set_transform`](Self::set_transform), but hit-testing treats the
    /// bounds as a fixed viewport so the whole visible area stays interactive
    /// at any pan / zoom. Used by `SceneView` for its pan/zoom view transform.
    pub fn set_content_transform(
        &mut self,
        id: WidgetId,
        transform: impl Into<crate::signal::Prop<bastyde_canvas::Transform2D>>,
    ) {
        self.tree.set_content_transform(id, transform);
    }

    /// Bind a Gaussian-equivalent blur radius to a widget. The render
    /// walker emits `BeginBlurredSubtree { bounds, radius }` before
    /// painting the widget's subtree and `EndBlurredSubtree` afterwards;
    /// the renderer redirects drawing into an intermediate texture, runs
    /// a dual-Kawase blur chain at the requested radius, and composites
    /// the blurred result back into the parent pass at the widget's
    /// bounds. Bound at `RepaintOnly`: blur radius changes never trigger
    /// relayout. Sub-perceptual radii (< 0.5) skip the Begin/End pair
    /// entirely so animated enable/disable patterns have zero per-frame
    /// cost when fully off. Used by the `Blur` wrapper.
    pub fn set_blur(&mut self, id: WidgetId, radius: impl Into<crate::signal::Prop<f32>>) {
        self.tree.set_blur(id, radius);
    }

    /// Bind a widget's enabled state to a boolean prop or compatibility state binding.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        self.tree.enabled_when(id, state);
    }

    /// Reactive view of "is this widget effectively enabled?" — the AND
    /// of the widget's own `enabled_state` and every ancestor's. The
    /// arena's [`crate::arena::WidgetArena::is_enabled`] is the
    /// non-reactive equivalent; this method gives composite widgets a
    /// `Signal<bool>` they can `.map(...)` / `.zip(...)` against to
    /// derive other reactive UI state (cursor, custom paint, helper
    /// signals).
    ///
    /// Leaves like `IconWidget` / `TextWidget` / `RectWidget` do NOT
    /// need this — they get the bool directly via
    /// [`crate::widget::PaintContext::effective_enabled`] at paint time.
    /// This method is for composites that need the value at build time
    /// or want to chain signals.
    ///
    /// Returns `Signal::new(true)` for any node whose entire ancestor
    /// chain (including itself) has no `enabled_state` bound. Captures
    /// the ancestor chain at call time — re-parenting a widget after
    /// this returns will not retroactively update the derived signal.
    /// In Bastyde's rebuild-on-change tree this is the expected
    /// semantics; widgets that re-anchor across builds should call
    /// `effective_enabled_signal` from inside their `build()`.
    pub fn effective_enabled_signal(&self, id: WidgetId) -> Signal<bool> {
        self.tree.effective_enabled_signal(id)
    }

    /// Bind a widget's Tab-key participation to a boolean prop or
    /// compatibility state binding. When false, the widget is removed
    /// from Tab / Shift+Tab traversal but remains reachable via
    /// `request_focus` and arrow-key navigation. Implements the ARIA
    /// roving-tabindex pattern (HTML `tabindex="-1"` semantics).
    pub fn set_tab_stop(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        self.tree.set_tab_stop(id, state);
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

    /// Attach a tooltip that auto-promotes to sticky after a dwell
    /// timer. Non-None `sticky_after` enables the sticky-on-dwell UX:
    /// once the tooltip has been shown for `sticky_after`, the tree
    /// flags the entry sticky and swaps the overlay's dismiss
    /// behavior to `EscapeOrClickOutside`.
    pub fn attach_tooltip_with_sticky(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
    ) {
        self.tree
            .attach_tooltip_with_sticky(anchor_id, content_id, delay, sticky_after);
    }

    /// Variant of [`attach_tooltip_with_sticky`](Self::attach_tooltip_with_sticky)
    /// that takes a shared `Rc<Cell<Option<Instant>>>` "sink" the
    /// tree updates whenever the tooltip is shown / dismissed. The
    /// tooltip widget reads from this sink to compute its own dwell
    /// progress reliably, without needing a paint-gap heuristic.
    pub fn attach_tooltip_with_sticky_sink(
        &mut self,
        anchor_id: WidgetId,
        content_id: WidgetId,
        delay: std::time::Duration,
        sticky_after: Option<std::time::Duration>,
        shown_at_sink: std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>,
    ) {
        self.tree.attach_tooltip_with_sticky_sink(
            anchor_id,
            content_id,
            delay,
            sticky_after,
            shown_at_sink,
        );
    }

    /// Promote a shown tooltip to "sticky": removes its auto-dismiss
    /// on pointer-leave and swaps the overlay's dismiss behavior to
    /// `EscapeOrClickOutside`. Used by rich tooltips that implement a
    /// dwell timer.
    pub fn promote_tooltip_to_sticky(&mut self, content_id: WidgetId) {
        self.tree.promote_tooltip_to_sticky(content_id);
    }

    /// Set a widget as dormant (inactive). Used to pre-create overlay content
    /// that will be activated later via `EventContext::activate()`.
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.tree.set_dormant(id);
    }

    /// Destroy a widget and its entire subtree, removing them from the
    /// arena and dropping any per-widget subscription / effect handles.
    ///
    /// Use this to clean up dormant subtrees that the current widget
    /// created during a prior build and that live outside its regular
    /// arena children — e.g., a pre-built popup panel inserted via
    /// `ctx.add(..)` + `ctx.set_dormant(..)` that becomes stale after a
    /// rebuild. Regular arena children of the composite (i.e. widgets
    /// whose ids are returned from `build`) are destroyed automatically
    /// by the framework's rebuild path and do not need this call.
    ///
    /// If an overlay currently references `id` as its content, the
    /// overlay is dismissed first so the manager does not retain a
    /// stale content reference.
    pub fn destroy_subtree(&mut self, id: WidgetId) {
        let overlay_id = self.tree.overlay_manager().find_by_content(id);
        if let Some(overlay_id) = overlay_id {
            self.tree.dismiss_overlay(overlay_id);
        }
        self.tree.destroy_subtree(id);
    }

    /// Apply a `HandlerSet` to the composite widget being built (self).
    /// This transfers attached event handlers, focusable flag, cursor, etc.
    /// to the widget's arena node, replacing `event()` and `is_focusable()` overrides.
    pub fn apply_self_handlers(&mut self, handler_set: crate::widget_builder::HandlerSet) {
        let id = self.self_id();
        self.tree.apply_self_handler_set(id, handler_set);
    }

    /// Move keyboard focus to `id`. Mirrors
    /// `EventContext::request_focus` for use during `build()` — e.g.
    /// when a composing widget pre-builds an editor and needs focus to
    /// land on it as soon as the subtree is wired in.
    pub fn focus(&mut self, id: WidgetId) {
        self.tree.focus(id);
    }

    /// Find the first focusable widget within the subtree rooted at
    /// `root` in depth-first order. Returns `None` when the subtree has
    /// no focusable descendant or `root` is not in the arena.
    pub fn first_focusable_descendant(&self, root: WidgetId) -> Option<WidgetId> {
        self.tree.first_focusable_descendant(root)
    }

    // --- Actions & shortcuts ---

    /// Attach an [`Action`](crate::action::Action) to the widget being
    /// built. Actions are consulted during intent dispatch as the
    /// framework walks source-widget → root; the first matching,
    /// enabled action wins (subject to the `IntentResponse` returned
    /// by its handler).
    ///
    /// Actions are cleared on rebuild, mirroring event handlers.
    pub fn register_action(&mut self, action: crate::action::Action) {
        let id = self.self_id();
        self.tree.push_action(id, action);
    }

    /// Register a [`Shortcut`](crate::shortcut::Shortcut) in the
    /// tree's registry, owned by the widget being built.
    ///
    /// If the shortcut builder left `scope` at the default
    /// ([`ShortcutScope::Global`](crate::shortcut::ShortcutScope::Global)),
    /// this method rewrites it to `Scoped(self_id)` so the shortcut
    /// only fires when focus is inside the registering widget's
    /// subtree — the ergonomic default for widget-declared shortcuts.
    /// Callers that want an explicit global shortcut should use
    /// [`BuildContext::register_shortcut_global`] instead; callers
    /// that want to scope to a specific child should set
    /// `.scope_to(child_id)` on the builder themselves.
    ///
    /// Ownership: the shortcut is removed from the registry when the
    /// widget is destroyed or rebuilt. User overrides survive across
    /// rebuilds (graveyard semantics).
    pub fn register_shortcut(&mut self, mut shortcut: crate::shortcut::Shortcut) {
        let id = self.self_id();
        if shortcut.scope == crate::shortcut::ShortcutScope::Global {
            shortcut.scope = crate::shortcut::ShortcutScope::Scoped(id);
        }
        self.tree
            .shortcut_registry_mut()
            .register_owned(shortcut, id);
    }

    /// Register a [`Shortcut`](crate::shortcut::Shortcut) with
    /// explicit global scope, owned by the widget being built. Unlike
    /// [`BuildContext::register_shortcut`], this does not rewrite the
    /// scope — the shortcut fires regardless of focus position.
    ///
    /// Ownership still applies: the shortcut is torn down when this
    /// widget goes away.
    pub fn register_shortcut_global(&mut self, mut shortcut: crate::shortcut::Shortcut) {
        let id = self.self_id();
        shortcut.scope = crate::shortcut::ShortcutScope::Global;
        self.tree
            .shortcut_registry_mut()
            .register_owned(shortcut, id);
    }

    /// Pre-declare shortcuts on behalf of a not-yet-mounted child
    /// (e.g. a `Switcher` walking its `Pending` slots' static
    /// declarations before they're inserted). Each shortcut is owned
    /// by the *calling* widget and its declared scope is preserved
    /// as-is — unlike [`register_shortcut`](Self::register_shortcut),
    /// no rewrite from `Global` to `Scoped(self)` happens, because
    /// the child intended its own scope.
    ///
    /// When the child is eventually mounted, the framework's
    /// insert-time walk of `Widget::declare_shortcuts` re-registers
    /// the same ids owned by the *child*; the registry's idempotent
    /// upsert moves ownership cleanly. If the child never mounts, the
    /// pre-declared entries stay alive (owned by the parent) so
    /// settings UIs still see them, and they get torn down when the
    /// parent goes away.
    pub fn register_pending_shortcuts(
        &mut self,
        shortcuts: impl IntoIterator<Item = crate::shortcut::Shortcut>,
    ) {
        let id = self.self_id();
        let registry = self.tree.shortcut_registry_mut();
        for shortcut in shortcuts {
            registry.register_owned(shortcut, id);
        }
    }

    /// Read-through access to the tree's shortcut registry. Consumers
    /// (menus, tooltips) look up the effective keystroke for a given
    /// id here, and observe
    /// [`ShortcutRegistry::version`](crate::shortcut::ShortcutRegistry::version)
    /// to refresh when the user rebinds.
    pub fn shortcut_registry(&self) -> &crate::shortcut::ShortcutRegistry {
        self.tree.shortcut_registry()
    }

    /// Effective view of a shortcut by id, merged with any user
    /// override. Returns `None` when no default has been registered
    /// for `id`. Typical caller pattern: call from `paint()` so
    /// late-registered shortcuts are still picked up without a
    /// dedicated build-phase query.
    pub fn effective_shortcut<'b>(
        &'b self,
        id: &str,
    ) -> Option<crate::shortcut::EffectiveShortcut<'b>> {
        self.shortcut_registry().effective(id)
    }

    /// Convenience accessor for the reactive version signal. Widgets
    /// that render shortcut-derived state (menu labels, tooltips)
    /// observe this so the UI refreshes when the user rebinds or a
    /// new shortcut is registered.
    pub fn shortcut_version(&self) -> &Signal<u64> {
        self.shortcut_registry().version()
    }

    /// Apply a `HandlerSet` to a child widget created during this build.
    /// Use this to attach event handlers to children without wrapping them
    /// in `WidgetWithHandlers`.
    pub fn apply_handlers(
        &mut self,
        id: crate::widget_id::WidgetId,
        handler_set: crate::widget_builder::HandlerSet,
    ) {
        // A composing parent attaches handlers to a child — from the
        // child's perspective these are external and must survive the
        // child's own rebuilds.
        self.tree.apply_external_handler_set(id, handler_set);
    }

    /// Subscribe to events from the registered application event source.
    /// The callback runs on the UI thread when the source publishes an
    /// event with a matching origin.
    ///
    /// The subscription is scoped to the current widget's lifetime: when
    /// the widget is rebuilt or destroyed, the framework drops the source
    /// handle (unregistering from the source) and removes the UI-side
    /// callback.
    ///
    /// # Panics
    ///
    /// Panics if no event source has been registered on the
    /// `BastydeAppBuilder`. In debug builds, also asserts that the `Origin`
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
             on BastydeAppBuilder. Call .event_source(source) on the builder first.",
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
                 is installed on the tree. bastyde-app installs one when an \
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

#[cfg(test)]
mod effect_tests {
    use super::*;
    use crate::widget::{LayoutContext, Widget};
    use crate::widget_id::WidgetId;
    use crate::widget_tree::WidgetTree;
    use bastyde_canvas::SizeProposal;

    /// A leaf widget that registers an effect on one signal to mirror its
    /// value into another. Produces no children.
    #[derive(Debug)]
    struct LeafWithEffect {
        source: Signal<i32>,
        mirror: Signal<i32>,
    }

    impl Widget for LeafWithEffect {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let mirror = self.mirror.clone();
            ctx.effect(&self.source, move |v| mirror.set(*v));
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    /// A widget that observes the per-frame tick signal and accumulates the
    /// deltas it receives into a shared counter, so a test can verify both
    /// that the tick fires at all and that the delta value is non-zero.
    #[derive(Debug)]
    struct FrameTickListener {
        ticks: Signal<u32>,
        last_delta: Signal<f32>,
    }

    impl Widget for FrameTickListener {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let ticks = self.ticks.clone();
            let last_delta = self.last_delta.clone();
            let tick = ctx.frame_tick();
            ctx.effect(&tick, move |delta| {
                ticks.set(ticks.get() + 1);
                last_delta.set(*delta);
            });
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    #[test]
    fn frame_tick_stays_silent_until_explicit_request() {
        // The draw-when-needed contract: a widget that merely observes
        // frame_tick must NOT keep the tree awake. Only an explicit
        // `request_frame()` call pumps a tick.
        let mut tree = WidgetTree::new();
        let ticks = Signal::new(0_u32);
        let last_delta = Signal::new(-1.0_f32);
        tree.add(FrameTickListener {
            ticks: ticks.clone(),
            last_delta: last_delta.clone(),
        });

        // Flush the initial layout-dirty flag from widget insertion.
        tree.layout(bastyde_canvas::SizeProposal::exact(400.0, 300.0));
        assert!(
            !tree.frame_requested(),
            "observing frame_tick does not set the request flag"
        );

        tree.tick_animations(std::time::Duration::from_millis(16));
        assert_eq!(
            ticks.get(),
            0,
            "an un-requested tick_animations must not fire frame_tick observers"
        );
        assert_eq!(last_delta.get(), -1.0);
    }

    #[test]
    fn frame_tick_fires_once_per_request() {
        let mut tree = WidgetTree::new();
        let ticks = Signal::new(0_u32);
        let last_delta = Signal::new(-1.0_f32);
        let id = tree.add(FrameTickListener {
            ticks: ticks.clone(),
            last_delta: last_delta.clone(),
        });

        // Flush initial layout-dirty flag so assertions reflect only
        // the frame-tick contract.
        tree.layout(bastyde_canvas::SizeProposal::exact(400.0, 300.0));

        tree.request_frame();
        assert!(tree.needs_redraw(), "explicit request marks the tree dirty");
        assert!(tree.frame_requested());

        tree.tick_animations(std::time::Duration::from_millis(16));
        assert_eq!(ticks.get(), 1);
        assert!((last_delta.get() - 0.016).abs() < 0.001);
        assert!(
            !tree.frame_requested(),
            "request flag must be cleared after the tick fired"
        );

        // Second request fires exactly one more tick.
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        assert_eq!(ticks.get(), 2);

        // Without a request, further ticks silently advance time.
        tree.tick_animations(std::time::Duration::from_millis(16));
        assert_eq!(ticks.get(), 2);

        tree.destroy_subtree(id);
        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_millis(16));
        assert_eq!(
            ticks.get(),
            2,
            "destroyed widget's observer must not resurrect"
        );
    }

    #[test]
    fn frame_tick_delta_clamped_against_huge_pauses() {
        let mut tree = WidgetTree::new();
        let ticks = Signal::new(0_u32);
        let last_delta = Signal::new(-1.0_f32);
        tree.add(FrameTickListener {
            ticks: ticks.clone(),
            last_delta: last_delta.clone(),
        });

        tree.request_frame();
        tree.tick_animations(std::time::Duration::from_secs(5));
        assert_eq!(ticks.get(), 1);
        assert!(
            (last_delta.get() - 0.1).abs() < 1e-4,
            "frame delta must clamp at 0.1s even after a multi-second pause"
        );
    }

    #[test]
    fn leaf_widget_effect_fires_and_is_cleaned_up_on_destroy() {
        // Regression guard: before the insert_widget / add_child fix,
        // effect_handles for a leaf widget (Vec::new() from build()) were
        // dropped the moment BuildContext went out of scope, silently
        // unregistering the observer. After the fix, the handle is
        // transferred to the arena node and the effect fires on signal
        // changes until the widget is destroyed.
        let mut tree = WidgetTree::new();
        let source = Signal::new(0_i32);
        let mirror = Signal::new(0_i32);

        let id = tree.add(LeafWithEffect {
            source: source.clone(),
            mirror: mirror.clone(),
        });

        // The effect should be live after insertion.
        source.set(42);
        assert_eq!(
            mirror.get(),
            42,
            "leaf widget effect must survive build() and fire on signal change"
        );

        source.set(7);
        assert_eq!(mirror.get(), 7);

        // Destroying the widget drops its effect_handles, which in turn
        // drops each ObserverHandle and unregisters the observer.
        tree.destroy_subtree(id);
        source.set(100);
        assert_eq!(
            mirror.get(),
            7,
            "effect must be unregistered after widget destruction"
        );
    }
}
