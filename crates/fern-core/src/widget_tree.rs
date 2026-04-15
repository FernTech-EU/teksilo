use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, RenderFrame, SizeProposal};
use fern_tokens::Theme;

use crate::app_command::{AppCommand, ErasedCommand};
use crate::arena::WidgetArena;
use crate::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
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

struct AnimatedRegistration(crate::signal::WeakAnimatedSignal);

impl AnimatedRegistration {
    fn same_signal(&self, signal: &crate::signal::Signal<f32>) -> bool {
        self.0.same_signal(signal)
    }

    fn is_alive(&self) -> bool {
        self.0.upgrade().is_some()
    }

    fn take_pending_animation(
        &self,
    ) -> Option<(crate::signal::Signal<f32>, crate::animation::AnimationRequest)> {
        let signal = self.0.upgrade()?;
        let request = signal.take_pending_animation()?;
        Some((signal, request))
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
    pending_modal_dismissal: bool,
    shortcut_lookup: Option<ShortcutLookup>,
    shortcut_reverse_lookup: Option<ShortcutReverseLookup>,
    binding_registry: crate::binding::BindingRegistry,
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
    /// Reverse map from synthetic (widget-emitted) AccessKit NodeIds
    /// to the WidgetId that owns them. Rebuilt on every full
    /// accessibility walk. `handle_accessibility_actions` uses this
    /// to route an `ActionRequest` targeting a TextRun child back
    /// to the owning rich-text editor, since synthetic NodeIds
    /// can't be decoded back to a WidgetId by value alone.
    pub(crate) synthetic_parent_map: std::collections::HashMap<accesskit::NodeId, WidgetId>,
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
    /// Optional platform host for custom window chrome (set when the
    /// application opts in via `WindowConfig::custom_chrome(true)`). Stored
    /// here so that the root-builder closure has access during widget
    /// construction; the same `Rc` is also held by `WindowManager` so it
    /// outlives the widget tree if needed.
    title_bar_host: Option<Rc<dyn crate::PlatformTitleBarHost>>,
    /// App-level subscription state: registered event source adapter,
    /// proxy poster, UI-side subscription callbacks. Default is empty;
    /// fern-app installs a populated context when an event source is
    /// registered on the builder. See architecture §9.4.
    pub(crate) app_context: Rc<crate::event_source::TreeAppContext>,
    /// Active locale identifier. fern-i18n is still a stub, so the tree
    /// only stores the value and rebuilds composite widgets on change —
    /// translation lookup happens entirely in user code today.
    pub(crate) locale: Option<String>,
    /// Per-frame delta-seconds signal, advanced by `layout()` **only when
    /// a widget has explicitly requested a frame** via `request_frame()`.
    /// This preserves FernUI's draw-when-needed model: idle trees stay
    /// idle even if widgets have registered observers on this signal.
    pub(crate) frame_tick: crate::signal::Signal<f32>,
    /// Set by `request_frame()`; consumed by `advance_frame_tick()` on
    /// the next `layout()`. Observers that need another tick after the
    /// current one must re-request. Stored as `Rc<Cell>` so observers
    /// fired from inside the layout pass (`ctx.effect` closures on
    /// `frame_tick`) can chain-request without needing &mut access
    /// to the tree — see [`FrameRequestHandle`].
    pub(crate) frame_tick_requested: std::rc::Rc<std::cell::Cell<bool>>,
    /// Delayed frame wake-up deadline. Widgets that need to schedule
    /// a future frame without pumping at full framerate (caret blink,
    /// etc.) store the target instant here via
    /// [`wake_at_handle`](Self::wake_at_handle). `next_timer_deadline`
    /// rolls it into the event loop's WaitUntil; when reached, the
    /// next `layout()` automatically re-arms `frame_tick_requested`
    /// so the frame-tick effects run on the wake-up pass.
    pub(crate) pending_wake_at: std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>,
    /// Wall-clock time of the previous `layout()` call (for delta computation).
    pub(crate) last_frame_time: Option<std::time::Instant>,
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
    /// When set, the tooltip auto-promotes to "sticky" after this
    /// much elapsed time since it was shown. The entry stays in the
    /// table and is just flagged sticky — the difference is that
    /// pointer-leave no longer dismisses it and the overlay's
    /// dismiss behavior is swapped to `EscapeOrClickOutside`.
    sticky_after: Option<std::time::Duration>,
    /// True when the dwell timer reached `sticky_after`. Causes
    /// `tooltip_pointer_leave` to skip the dismissal and lets the
    /// overlay survive pointer-leave until the user explicitly
    /// dismisses it via Escape or a click outside.
    is_sticky: bool,
    /// When the overlay was shown (simulated). Together with
    /// `sticky_after` drives auto-promotion.
    shown_at_sim: Option<std::time::Instant>,
    /// When the overlay was shown (real).
    shown_at_real: Option<std::time::Instant>,
    /// Optional shared sink the tooltip widget can read from to
    /// compute its own dwell progress. Mirrors `shown_at_real`:
    /// set on show, cleared on dismissal. Used by `RichTooltipWidget`
    /// to drive the dwell indicator without relying on a fragile
    /// paint-gap heuristic.
    shown_at_sink: Option<std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>>,
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
            pending_modal_dismissal: false,
            shortcut_lookup: None,
            shortcut_reverse_lookup: None,
            binding_registry: crate::binding::BindingRegistry::new(),
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
            synthetic_parent_map: std::collections::HashMap::new(),
            cached_frame: None,
            pointer_captured_by: None,
            current_cursor: crate::widget::CursorIcon::Default,
            pending_delayed_overlays: Vec::new(),
            prefers_high_contrast: false,
            prefers_reduced_motion: false,
            text_scale_factor: 1.0,
            active_drag: None,
            title_bar_host: None,
            app_context: Rc::new(crate::event_source::TreeAppContext::empty()),
            locale: None,
            frame_tick: crate::signal::Signal::new(0.0_f32),
            frame_tick_requested: std::rc::Rc::new(std::cell::Cell::new(false)),
            pending_wake_at: std::rc::Rc::new(std::cell::Cell::new(None)),
            last_frame_time: None,
        }
    }

    /// Clone the shared "frame requested" flag. Widgets stash this
    /// in their state and call `.set(true)` from inside frame-tick
    /// closures to chain-request another frame without needing
    /// mutable access to the tree. See `RichTextEditor` for the
    /// canonical use (caret blink, drag-select auto-scroll).
    pub fn frame_request_handle(&self) -> std::rc::Rc<std::cell::Cell<bool>> {
        self.frame_tick_requested.clone()
    }

    /// Clone the shared wake-at deadline cell. Widgets stash this in
    /// their state and call [`request_wake_at`] from frame-tick effects
    /// to schedule a one-shot deadline without keeping the event loop
    /// in `Poll` mode. On the next `layout()` at or past the deadline,
    /// the tree auto-arms `frame_tick_requested` so the effect runs on
    /// the wake-up pass. Canonical use: the rich text editor's caret
    /// blink schedules a 500 ms wake instead of pumping every frame.
    pub fn wake_at_handle(&self) -> std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>> {
        self.pending_wake_at.clone()
    }

    /// Schedule a one-shot frame wake at `at`. Merges with any existing
    /// deadline — keeps the earlier instant so the most urgent wake
    /// wins.
    pub fn request_wake_at(&self, at: std::time::Instant) {
        let current = self.pending_wake_at.get();
        let merged = match current {
            Some(existing) if existing <= at => existing,
            _ => at,
        };
        self.pending_wake_at.set(Some(merged));
    }

    /// The per-frame delta-seconds signal. Observers fire **only on frames
    /// the tree was asked to pump** via [`request_frame`](Self::request_frame);
    /// merely observing the signal does not keep the event loop awake.
    /// See [`BuildContext::frame_tick`] for widget-side access and
    /// [`BuildContext::request_frame`] for the opt-in request side.
    pub fn frame_tick(&self) -> crate::signal::Signal<f32> {
        self.frame_tick.clone()
    }

    /// Ask the tree to pump exactly one more frame. `needs_redraw()`
    /// returns true until the request is consumed by the next
    /// `layout()` call, which fires the per-frame tick signal and
    /// clears the flag. Observers that still need more frames (drag
    /// auto-scroll, caret blink, pending document events) must call
    /// `request_frame()` again from inside their tick closure.
    ///
    /// Takes `&self` on purpose: widget handlers and per-frame effects
    /// receive a shared reference to the tree via `EventContext` /
    /// `BuildContext`, and the request flag is a `Cell` specifically so
    /// those shared paths can toggle it without ceremony.
    pub fn request_frame(&self) {
        self.frame_tick_requested.set(true);
    }

    /// Whether a frame was explicitly requested. Exposed for tests and
    /// for the event-loop driver that decides when to schedule the next
    /// wake-up.
    pub fn frame_requested(&self) -> bool {
        self.frame_tick_requested.get()
    }

    /// Advance the frame tick signal when (and only when) a frame was
    /// requested. Called by `layout()` before the scheduler tick so the
    /// per-frame observers fire on the same frame they asked for.
    pub(crate) fn advance_frame_tick(&mut self, now: std::time::Instant) {
        if !self.frame_tick_requested.get() {
            self.last_frame_time = Some(now);
            return;
        }
        self.frame_tick_requested.set(false);
        let delta = match self.last_frame_time {
            Some(prev) => {
                let d = now.saturating_duration_since(prev).as_secs_f32();
                // Clamp absurd deltas (pause/breakpoint) so observers never see a spike.
                d.clamp(0.0, 0.1)
            }
            None => 0.0,
        };
        self.last_frame_time = Some(now);
        self.frame_tick.set(delta);
    }

    /// Replace the per-tree app context. Called by `fern-app` when
    /// constructing a window so the widget tree can reach the registered
    /// event source adapter and post subscription events through the
    /// event-loop proxy. See architecture §9.4.
    pub fn set_app_context(&mut self, app_context: Rc<crate::event_source::TreeAppContext>) {
        self.app_context = app_context;
    }

    /// Get the per-tree app context. Used by `BuildContext::subscribe_event`
    /// and by the event-loop handler when dispatching incoming
    /// `AppEvent::SubscriptionEvent`.
    pub fn app_context(&self) -> &Rc<crate::event_source::TreeAppContext> {
        &self.app_context
    }

    /// Switch the tree-level locale at runtime. Rebuilds all composite
    /// widgets so that any tr! lookups picked up at build time are
    /// re-evaluated against the new locale, and marks all widgets dirty.
    ///
    /// **Rebuild policy (architecture §12.7).** A composite rebuild is
    /// only strictly required when the layout direction flips (because
    /// child order and leading/trailing resolution are decided inside
    /// `build()`). For same-direction locale switches, the reactive
    /// binding system alone is sufficient: every `LocalizedString` that
    /// went through `to_signal()` observes the i18n manager's version
    /// counter and re-resolves automatically, and the framework only
    /// needs to mark bound widgets dirty for repaint/relayout.
    ///
    /// This method currently always rebuilds for safety, matching the
    /// Phase G / Phase H exit state. A later optimization — skip the
    /// rebuild when the caller already applied a direction change (or
    /// when no direction change is needed) — is tracked as a follow-up.
    /// The optimization requires proving that every composite widget
    /// which builds translated children does so through a
    /// `LocalizedString::to_signal()` binding, not by resolving text
    /// eagerly inside `build()` into a static `String`.
    pub fn set_locale(&mut self, locale: String) {
        if self.locale.as_deref() == Some(locale.as_str()) {
            return;
        }
        self.locale = Some(locale);
        self.rebuild_built_widgets();
        self.arena.mark_all_dirty();
    }

    /// Currently active locale identifier, if any.
    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
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

    /// Attach a platform host for custom window chrome. Set by the
    /// `WindowManager` when the application opts in via
    /// `WindowConfig::custom_chrome(true)`. Widgets like `TitleBar` retrieve
    /// it from inside the root-builder closure via [`Self::title_bar_host`].
    pub fn with_title_bar_host(mut self, host: Rc<dyn crate::PlatformTitleBarHost>) -> Self {
        self.title_bar_host = Some(host);
        self
    }

    pub fn set_title_bar_host(&mut self, host: Rc<dyn crate::PlatformTitleBarHost>) {
        self.title_bar_host = Some(host);
    }

    /// Get the platform title bar host, if one was attached. Returns `None`
    /// when the application did not opt into custom chrome, or when the
    /// platform does not support it (e.g. X11).
    pub fn title_bar_host(&self) -> Option<Rc<dyn crate::PlatformTitleBarHost>> {
        self.title_bar_host.clone()
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Whether any widget needs layout or paint (i.e., a redraw would be useful).
    pub fn needs_redraw(&self) -> bool {
        self.arena.any_needs_layout()
            || self.arena.any_needs_paint()
            || self.animation_scheduler.has_active()
            || self.frame_tick_requested.get()
    }

    /// Whether a render pass is needed (any widget needs layout or paint).
    pub fn needs_render(&self) -> bool {
        self.arena.any_needs_layout() || self.arena.any_needs_paint()
    }

    /// Register a `Signal<f32>` for animation support. The framework
    /// checks registered signals each frame for pending `animate_to`
    /// requests. Called automatically by `BuildContext::animated_signal()`.
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
                .push(AnimatedRegistration(weak_signal));
        }
    }

    /// Whether any animation is currently running.
    pub fn has_active_animations(&self) -> bool {
        self.animation_scheduler.has_active()
    }

    /// Pick up pending `animate_to` requests from registered signals
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

        for (signal, req) in pending {
            self.animation_scheduler.animate_with_frame_interval(
                &signal,
                req.target,
                req.duration,
                req.easing,
                req.frame_interval,
                now,
            );
        }
    }

    /// Advance time-driven gesture recognizers (currently only
    /// [`crate::gesture::LongPressRecognizer`]) across every widget that
    /// has a gesture arena. Must be called by the event loop on each
    /// wake-up; otherwise long-press will never fire during an idle hold.
    ///
    /// When a recognizer transitions to `Recognized`, the corresponding
    /// handler on the owning widget is invoked with a fresh
    /// [`EventContext`], and any commands / overlay requests it emits are
    /// collected through the normal post-event path.
    pub fn tick_gestures(&mut self, now: std::time::Instant) {
        let ids = self.arena.active_ids();
        for id in ids {
            let gesture = match self.arena.get_mut(id) {
                Some(node) => node
                    .handlers
                    .gesture_arena
                    .as_mut()
                    .and_then(|arena| arena.tick(now)),
                None => None,
            };
            let Some(gesture) = gesture else { continue };

            let mut ctx = EventContext::new().with_app_context(self.app_context.clone());
            if let Some(node) = self.arena.get_mut(id) {
                Self::dispatch_recognized_gesture(node, gesture, &mut ctx);
            }
            self.collect_from_ctx(ctx, id);
            self.arena.mark_needs_paint(id);
        }
    }

    /// Earliest wall-clock deadline at which any active gesture arena
    /// needs [`WidgetTree::tick_gestures`] called — typically a pending
    /// long-press timeout. Returns `None` when no recognizer is waiting.
    pub fn next_gesture_deadline(&self) -> Option<std::time::Instant> {
        self.arena
            .active_ids()
            .into_iter()
            .filter_map(|id| self.arena.get(id))
            .filter_map(|node| node.handlers.gesture_arena.as_ref())
            .filter_map(|arena| arena.next_deadline())
            .min()
    }

    /// Advance animations by simulated time (for deterministic testing).
    /// Pending `animate_to` requests are started at the current sim_clock,
    /// then time advances by `duration`, and the scheduler ticks at the new time.
    pub fn tick_animations(&mut self, duration: std::time::Duration) {
        self.process_pending_animations_at(self.sim_clock);

        self.sim_clock += duration;

        if self.frame_tick_requested.get() {
            self.frame_tick_requested.set(false);
            let delta = duration.as_secs_f32().clamp(0.0, 0.1);
            self.frame_tick.set(delta);
        }

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
        // Per §9.4.5, drop the source handle first (stops further source-side
        // dispatch) and then remove the UI-side callback. Either order gives
        // the same user-visible outcome for events that get posted between
        // the two steps (they are silently dropped once the callback is
        // gone), but dropping the source handle first stops the publisher
        // thread's work sooner.
        let drained_subs = if let Some(node) = self.arena.get_mut(widget_id) {
            node.effect_handles.clear();
            node.dirty.needs_rebuild = false;
            std::mem::take(&mut node.subscription_handles)
        } else {
            Vec::new()
        };
        for (sub_id, handle) in drained_subs {
            drop(handle);
            self.app_context
                .subscription_callbacks
                .borrow_mut()
                .remove(&sub_id);
        }

        let old_children: Vec<WidgetId> = self.arena.children(widget_id).to_vec();
        for child_id in old_children {
            self.destroy_subtree(child_id);
        }

        let mut widget_box = match self.arena.take_widget(widget_id) {
            Some(widget) => widget,
            None => return,
        };

        let mut build_ctx = crate::build_context::BuildContext {
            tree: self,
            composite_id: Some(widget_id),
            effect_handles: Vec::new(),
            subscription_handles: Vec::new(),
        };
        let new_children = widget_box.build(&mut build_ctx);
        let effect_handles = std::mem::take(&mut build_ctx.effect_handles);
        let subscription_handles = std::mem::take(&mut build_ctx.subscription_handles);

        self.arena.restore_widget(widget_id, widget_box);

        for &child_id in &new_children {
            if let Some(child_node) = self.arena.get_mut(child_id) {
                child_node.parent = Some(widget_id);
            }
        }
        if let Some(node) = self.arena.get_mut(widget_id) {
            node.children = new_children;
            node.effect_handles = effect_handles;
            node.subscription_handles = subscription_handles;
        }
    }

    /// Recursively destroy a subtree, dropping per-widget subscription
    /// handles and removing their UI-side callbacks. Use this in place of
    /// `arena.destroy()` whenever a widget that may have subscribed to
    /// events is being torn down.
    pub(crate) fn destroy_subtree(&mut self, widget_id: WidgetId) {
        let children: Vec<WidgetId> = self.arena.children(widget_id).to_vec();
        for child in children {
            self.destroy_subtree(child);
        }
        let drained_subs = self
            .arena
            .get_mut(widget_id)
            .map(|node| std::mem::take(&mut node.subscription_handles))
            .unwrap_or_default();
        for (sub_id, handle) in drained_subs {
            drop(handle);
            self.app_context
                .subscription_callbacks
                .borrow_mut()
                .remove(&sub_id);
        }
        self.arena.destroy(widget_id);
    }

    /// Set the layout direction (LTR/RTL). Marks all widgets as needing layout.
    pub fn set_layout_direction(&mut self, direction: crate::environment::LayoutDirection) {
        self.layout_direction = direction;
        self.arena.mark_all_dirty();
    }

    /// The current layout direction.
    pub fn layout_direction(&self) -> crate::environment::LayoutDirection {
        self.layout_direction
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
    pub fn binding_registry(&self) -> &crate::binding::BindingRegistry {
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

    /// Drain whether the current native modal window should be dismissed.
    pub fn drain_pending_modal_dismissal(&mut self) -> bool {
        std::mem::replace(&mut self.pending_modal_dismissal, false)
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
                subscription_handles: Vec::new(),
            };
            let built_children = widget_box.build(&mut build_ctx);
            let effect_handles = std::mem::take(&mut build_ctx.effect_handles);
            let subscription_handles = std::mem::take(&mut build_ctx.subscription_handles);

            self.arena.restore_widget(id, widget_box);

            // Transfer per-widget handles to the node. Both lists are
            // stored unconditionally — a leaf widget that registers an
            // effect in its build() still needs its ObserverHandle to
            // persist (otherwise the effect unregisters the moment
            // BuildContext drops).
            if let Some(node) = self.arena.get_mut(id) {
                node.subscription_handles = subscription_handles;
                node.effect_handles = effect_handles;
            }

            if !built_children.is_empty() {
                for &child_id in &built_children {
                    if let Some(child_node) = self.arena.get_mut(child_id) {
                        child_node.parent = Some(id);
                    }
                }
                if let Some(node) = self.arena.get_mut(id) {
                    node.children = built_children;
                    node.has_built_children = true;
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
                    subscription_handles: Vec::new(),
                };
                let built_children = widget_box.build(&mut build_ctx);
                let effect_handles = std::mem::take(&mut build_ctx.effect_handles);
                let subscription_handles = std::mem::take(&mut build_ctx.subscription_handles);

                self.arena.restore_widget(id, widget_box);

                // Transfer per-widget handles to the node. See the
                // matching block in `insert_widget` — effect and
                // subscription handles must persist for leaf widgets
                // too, not only composite ones.
                if let Some(node) = self.arena.get_mut(id) {
                    node.subscription_handles = subscription_handles;
                    node.effect_handles = effect_handles;
                }

                if !built_children.is_empty() {
                    for &child_id in &built_children {
                        if let Some(child_node) = self.arena.get_mut(child_id) {
                            child_node.parent = Some(id);
                        }
                    }
                    if let Some(node) = self.arena.get_mut(id) {
                        node.children = built_children;
                        node.has_built_children = true;
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
            crate::binding::BindingLevel::Relayout,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.visible_state = Some(prop);
        }
    }

    /// Bind a widget's enabled state to a boolean prop or compatibility state binding.
    /// When false, the widget and its entire subtree ignore all events but remain
    /// visible. Focus traversal skips disabled subtrees and AccessKit marks their
    /// nodes as disabled. Accepts `Signal<bool>`, `Prop<bool>`, compatibility state
    /// bindings, or plain `bool`.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        let prop = state.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::Relayout,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.enabled_state = Some(prop);
        }
    }

    /// Whether a widget is effectively enabled. Returns `false` if the widget
    /// itself or any ancestor has `enabled_state` bound to `false`.
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

}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

