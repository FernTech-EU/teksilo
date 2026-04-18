use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Point, Rect, RenderFrame, SizeProposal};
use fern_tokens::Theme;

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
struct AnimatedRegistration {
    weak: crate::signal::WeakAnimatedSignal,
    owner: WidgetId,
}

impl AnimatedRegistration {
    fn same_signal(&self, signal: &crate::signal::Signal<f32>) -> bool {
        self.weak.same_signal(signal)
    }

    fn is_alive(&self) -> bool {
        self.weak.upgrade().is_some()
    }

    fn take_pending_animation(
        &self,
    ) -> Option<(
        crate::signal::Signal<f32>,
        crate::animation::AnimationRequest,
        WidgetId,
    )> {
        let signal = self.weak.upgrade()?;
        let request = signal.take_pending_animation()?;
        Some((signal, request, self.owner))
    }
}

#[allow(clippy::type_complexity)]
pub struct WidgetTree {
    arena: WidgetArena,
    /// Current theme value cached for `&Theme` accessors used by layout/paint
    /// contexts and by widgets that need an immediate read. The reactive source
    /// of truth is `theme_signal`; both are updated in lockstep by `set_theme`.
    theme: Theme,
    /// Reactive theme signal. Widgets that want their visual or derived state
    /// to track theme changes bind to this signal or build derived signals via
    /// `zip`/`map`. `set_theme` updates the signal (firing observers) without
    /// rebuilding the widget tree, so interaction state (focus, scroll, expanded
    /// panels, …) survives theme switches.
    theme_signal: crate::signal::Signal<Theme>,
    text_backend: Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    last_proposal: SizeProposal,
    pending_modal_requests: Vec<crate::modal::QueuedModalRequest>,
    pending_modal_dismissal: bool,
    shortcut_registry: crate::shortcut::ShortcutRegistry,
    /// Queue of intents awaiting dispatch. Populated either by the
    /// keystroke interception path (`dispatch_event` for KeyDown) or
    /// by handlers calling `ctx.send_intent(...)`. Drained between
    /// event-handler calls by [`WidgetTree::drain_pending_intents`].
    /// The tuple carries the source widget (dispatch anchor), the
    /// intent itself, and the firing shortcut's
    /// `propagate_when_disabled` policy.
    pending_intents: Vec<(WidgetId, crate::intent::Intent, bool)>,
    /// Currently-armed key-capture slot. `Some` when
    /// [`WidgetTree::begin_key_capture`] has been called and the
    /// returned [`CaptureHandle`](crate::shortcut::CaptureHandle)
    /// is still alive. The slot is shared (via `Rc`) with the handle
    /// so dropping the handle cancels the capture, and calling
    /// `begin_key_capture` again creates a fresh slot without
    /// touching the previous one (whose handle, if dropped later,
    /// only clears its own orphaned slot).
    key_capture: Option<crate::shortcut::KeyCaptureSlot>,
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
    /// Monotonic counter bumped at the start of each `render()` call.
    /// Each widget's `last_painted_epoch` is set to this value whenever
    /// the paint pass (or the cache-hit early-out) confirms the widget
    /// intersects the window viewport. The animation scheduler uses it
    /// to detect and pause animations for widgets that have scrolled
    /// off-screen. Starts at `0`, which serves as the "never painted"
    /// sentinel; tests that only call `layout()` see the gate bypass.
    paint_epoch: u64,
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
    /// Active locale identifier. Cached for `Option<&str>` accessors; the
    /// reactive source of truth is `locale_signal`. Both are updated in
    /// lockstep by `set_locale`.
    pub(crate) locale: Option<String>,
    /// Reactive locale signal. Widgets and `LocalizedString` adapters bind to
    /// this signal to react to locale changes; `set_locale` updates the signal
    /// without rebuilding the widget tree.
    pub(crate) locale_signal: crate::signal::Signal<Option<String>>,
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
    /// Set by [`EventContext::close_window`] during dispatch; drained
    /// by the application event loop after each event via
    /// [`WidgetTree::take_close_window_request`].
    pub(crate) close_window_requested: bool,
    /// Raised by [`EventContext::set_locale`] during dispatch; drained by
    /// the application event loop (see
    /// `WindowManager::drain_pending_locale_requests`) so the switch can be
    /// routed through the `I18nManager` (active locale + version signal +
    /// RTL direction). `WidgetTree::set_locale` alone would only update the
    /// tree's local locale signal — the i18n thread-local would stay put
    /// and `tr!` lookups would not re-resolve.
    pub(crate) pending_locale_request: Option<String>,
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
        let initial_theme = Theme::light_default();
        Self {
            arena: WidgetArena::new(),
            theme: initial_theme.clone(),
            theme_signal: crate::signal::Signal::new(initial_theme),
            text_backend: None,
            focused: None,
            hovered: None,
            last_proposal: SizeProposal::exact(800.0, 600.0),
            pending_modal_requests: Vec::new(),
            pending_modal_dismissal: false,
            shortcut_registry: crate::shortcut::ShortcutRegistry::new(),
            pending_intents: Vec::new(),
            key_capture: None,
            binding_registry: crate::binding::BindingRegistry::new(),
            idle_queue: crate::idle::IdleQueue::new(),
            sim_clock: std::time::Instant::now(),
            focus_origin: None,
            overlay_manager: crate::overlay::OverlayManager::new(),
            tooltips: Vec::new(),
            layout_direction: crate::environment::LayoutDirection::default(),
            animation_scheduler: crate::animation::AnimationScheduler::new(),
            animated_values: Vec::new(),
            paint_epoch: 0,
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
            locale_signal: crate::signal::Signal::new(None),
            frame_tick: crate::signal::Signal::new(0.0_f32),
            frame_tick_requested: std::rc::Rc::new(std::cell::Cell::new(false)),
            pending_wake_at: std::rc::Rc::new(std::cell::Cell::new(None)),
            last_frame_time: None,
            close_window_requested: false,
            pending_locale_request: None,
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

    /// Switch the tree-level locale at runtime.
    ///
    /// Updates `locale_signal` (a reactive `Signal<Option<String>>`) and marks
    /// all widgets dirty for relayout and repaint. Widgets are **not** rebuilt:
    /// per-string reactivity flows through `LocalizedString::to_signal()` which
    /// observes the fern-i18n manager, and anything else that depends on the
    /// tree-level locale can bind to `locale_signal()`.
    pub fn set_locale(&mut self, locale: String) {
        if self.locale.as_deref() == Some(locale.as_str()) {
            return;
        }
        let new = Some(locale);
        self.locale = new.clone();
        self.locale_signal.set(new);
        self.arena.mark_all_dirty();
    }

    /// Currently active locale identifier, if any.
    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    /// Reactive handle on the current locale. Mirrors `locale()` but updates
    /// observers when `set_locale` is called.
    pub fn locale_signal(&self) -> &crate::signal::Signal<Option<String>> {
        &self.locale_signal
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

    /// Reactive handle on the current theme. Updates fire when `set_theme`
    /// is called; widgets that want theme-derived values to stay live should
    /// build derived signals via `theme_signal.map(...)` or combine with
    /// other inputs using `.zip(...)`.
    pub fn theme_signal(&self) -> &crate::signal::Signal<Theme> {
        &self.theme_signal
    }

    /// Whether any widget needs layout or paint (i.e., a redraw would be useful).
    ///
    /// Uses `has_running` rather than `has_active` so that animations
    /// parked by the window-inactive gate stop forcing the event loop
    /// into `ControlFlow::WaitUntil`. Without this, an unfocused window
    /// would still wake at the animation frame interval and the
    /// pause would save nothing.
    pub fn needs_redraw(&self) -> bool {
        self.arena.any_needs_layout()
            || self.arena.any_needs_paint()
            || self.animation_scheduler.has_running()
            || self.frame_tick_requested.get()
    }

    /// Whether a render pass is needed (any widget needs layout or paint).
    pub fn needs_render(&self) -> bool {
        self.arena.any_needs_layout() || self.arena.any_needs_paint()
    }

    /// Register a `Signal<f32>` for animation support. The framework
    /// checks registered signals each frame for pending `animate_to`
    /// requests. Called automatically by `BuildContext::animated_signal()`
    /// — `owner` is `ctx.self_id()` of the widget whose `build()` created
    /// the signal. Used by the scheduler to pause/cancel animations when
    /// the owning widget is offscreen, dormant, or destroyed.
    pub fn register_animated_signal(
        &mut self,
        signal: &crate::signal::Signal<f32>,
        owner: WidgetId,
    ) {
        self.animated_values
            .retain(|registration| registration.is_alive());
        if let Some(existing) = self
            .animated_values
            .iter_mut()
            .find(|registration| registration.same_signal(signal))
        {
            // Signal may have been registered earlier with a placeholder
            // owner (e.g. a widget field constructed pre-build and
            // re-registered during build()) — prefer the latest owner.
            existing.owner = owner;
            return;
        }
        if let Some(weak_signal) = signal.weak_handle() {
            self.animated_values.push(AnimatedRegistration {
                weak: weak_signal,
                owner,
            });
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

        for (signal, req, owner) in pending {
            if req.looping {
                let start = signal.get();
                self.animation_scheduler.animate_looping(
                    &signal,
                    owner,
                    start,
                    req.target,
                    req.duration,
                    req.easing,
                    req.frame_interval,
                    req.epsilon,
                    req.max_duration,
                    now,
                );
            } else {
                self.animation_scheduler.animate_with_options(
                    &signal,
                    owner,
                    req.target,
                    req.duration,
                    req.easing,
                    req.frame_interval,
                    req.epsilon,
                    req.max_duration,
                    now,
                );
            }
        }
    }

    /// Mark the owning window as active (focused AND not occluded) or
    /// inactive. Propagates to the animation scheduler so looping
    /// animations pause while the window is hidden — no ticks, no frame
    /// wakes, no GPU submits. See [`crate::animation::AnimationScheduler::set_window_active`].
    pub fn set_window_active(&mut self, active: bool) {
        self.animation_scheduler
            .set_window_active(active, std::time::Instant::now());
    }

    pub fn is_window_active(&self) -> bool {
        self.animation_scheduler.is_window_active()
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

        self.animation_scheduler
            .tick(self.sim_clock, &self.arena, self.paint_epoch);

        self.process_state_changes();
    }

    /// Switch the tree-level theme at runtime.
    ///
    /// Updates `theme_signal` (a reactive `Signal<Theme>`) and marks all widgets
    /// dirty for relayout and repaint. Widgets are **not** rebuilt: the
    /// `LayoutContext` and `PaintContext` already resolve the current theme on
    /// every pass, and any widget that derives state from theme tokens should
    /// do so through a `theme_signal()` subscription rather than a build-time
    /// capture. Preserves focus, scroll offsets, and other interaction state.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme.clone();
        self.theme_signal.set(theme);
        self.tooltips.clear();
        self.arena.mark_all_dirty();
    }

    /// After a rebuild that destroyed subtrees, drop any interaction state
    /// (focus, hover) whose target `WidgetId` is no longer valid. Preserves
    /// state when the target still exists in the arena. Called from
    /// data-driven rebuild paths (`process_state_changes`); theme and locale
    /// switches no longer rebuild.
    pub(crate) fn revalidate_interaction_state(&mut self) {
        if let Some(id) = self.focused
            && !self.arena.is_active(id)
        {
            self.focused = None;
            self.focus_origin = None;
        }
        if self.focused.is_none() {
            self.focus_origin = None;
        }
        if let Some(id) = self.hovered
            && !self.arena.is_active(id)
        {
            self.hovered = None;
        }
    }

    /// Rebuild a single composite widget: destroy old children, re-run `build()`,
    /// and wire up new children. Called from `process_state_changes()` when a
    /// binding at `BindingLevel::Rebuild` fires (data-driven rebuild). Theme
    /// and locale changes do **not** rebuild — they update reactive signals
    /// that widgets bind to via `theme_signal()` / `locale_signal()`.
    pub(crate) fn rebuild_single_widget(&mut self, widget_id: WidgetId) {
        // Per §9.4.5, drop the source handle first (stops further source-side
        // dispatch) and then remove the UI-side callback. Either order gives
        // the same user-visible outcome for events that get posted between
        // the two steps (they are silently dropped once the callback is
        // gone), but dropping the source handle first stops the publisher
        // thread's work sooner.
        // Cancel any looping/one-shot animations owned by this widget
        // before build() runs. Without this, a widget that creates a
        // fresh `animated_signal` in build() would leak the previous
        // instance's scheduler entry: the old Signal<f32> clone lives
        // in `animations` forever, ticking against an orphaned signal
        // (silent CPU waste) and, for looping animations, doubling up
        // when the new one registers.
        self.animation_scheduler.cancel_by_widget(widget_id);

        let drained_subs = if let Some(node) = self.arena.get_mut(widget_id) {
            node.effect_handles.clear();
            node.actions.clear();
            node.dirty.needs_rebuild = false;
            std::mem::take(&mut node.subscription_handles)
        } else {
            Vec::new()
        };
        // Shortcuts the widget declared are torn down too — they will
        // be re-registered during the upcoming `build()` call. User
        // overrides live in a separate map keyed by id, so user
        // rebindings survive this round-trip (see ShortcutRegistry
        // graveyard semantics).
        self.shortcut_registry.unregister_all_for_owner(widget_id);
        // Drop any signal→widget bindings from the previous build
        // cycle so `build()` can re-register a fresh set without
        // accumulating duplicates across rebuilds.
        self.binding_registry.unregister_for_widget(widget_id);
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
        // See the matching cancel in `rebuild_single_widget` — the
        // scheduler holds strong Signal<f32> clones, so the animation
        // would outlive its widget without this explicit cancellation.
        self.animation_scheduler.cancel_by_widget(widget_id);

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
        // Drop any shortcuts the destroyed widget owned. Unlike
        // `rebuild_single_widget`, destruction is permanent; if the
        // user had overrides, they stay in the graveyard.
        self.shortcut_registry.unregister_all_for_owner(widget_id);
        // Bindings from this widget stop being relevant; clean them
        // up so the registry doesn't leak dead entries for the
        // lifetime of the app.
        self.binding_registry.unregister_for_widget(widget_id);
        // If focus pointed at the widget about to disappear, drop it
        // so later dispatch doesn't anchor intent walks at a dead id
        // (which would silently swallow the intent).
        if self.focused == Some(widget_id) {
            self.focused = None;
            self.focus_origin = None;
        }
        if self.hovered == Some(widget_id) {
            self.hovered = None;
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

    /// Shared access to the shortcut registry. Widgets register their
    /// default shortcuts through here during `build()` (via
    /// `BuildContext::register_shortcut`); settings UIs and
    /// persistence layers read and mutate overrides directly.
    pub fn shortcut_registry(&self) -> &crate::shortcut::ShortcutRegistry {
        &self.shortcut_registry
    }

    pub fn shortcut_registry_mut(&mut self) -> &mut crate::shortcut::ShortcutRegistry {
        &mut self.shortcut_registry
    }

    /// Install a one-shot key-capture callback, returning a
    /// [`CaptureHandle`](crate::shortcut::CaptureHandle) whose `Drop`
    /// cancels the capture if it hasn't already fired. The next
    /// `KeyDown` the tree receives bypasses shortcut-registry lookup
    /// and invokes the callback with:
    /// - the captured [`KeyStroke`](crate::shortcut::KeyStroke)
    /// - mutable access to the registry (rebind in-place)
    /// - a mutable [`EventContext`] (so the handler can also emit
    ///   commands, send intents, dismiss overlays, …)
    ///
    /// Calling this while a previous capture is armed creates a
    /// **separate** slot; the prior handle, when eventually dropped,
    /// cancels only its own (now-orphaned) slot. The new capture
    /// wins.
    pub fn begin_key_capture(
        &mut self,
        callback: impl FnOnce(
                crate::shortcut::KeyStroke,
                &mut crate::shortcut::ShortcutRegistry,
                &mut EventContext,
            ) + 'static,
    ) -> crate::shortcut::CaptureHandle {
        let slot: crate::shortcut::KeyCaptureSlot =
            std::rc::Rc::new(std::cell::RefCell::new(Some(Box::new(callback))));
        self.key_capture = Some(slot.clone());
        crate::shortcut::CaptureHandle::new(slot)
    }

    /// Cancel any currently-armed key capture without invoking it.
    /// Equivalent to dropping the [`CaptureHandle`](crate::shortcut::CaptureHandle),
    /// but exposed here so callers that lost the handle (or never
    /// kept one) can still bail out.
    pub fn cancel_key_capture(&mut self) {
        if let Some(slot) = self.key_capture.take() {
            slot.borrow_mut().take();
        }
    }

    /// Whether a key-capture callback is currently armed.
    pub fn is_capturing_keys(&self) -> bool {
        self.key_capture
            .as_ref()
            .map(|slot| slot.borrow().is_some())
            .unwrap_or(false)
    }

    /// Consume any pending key-capture callback. Used internally by
    /// the dispatch path — returns the boxed closure so the caller
    /// can invoke it once the KeyStroke has been constructed. Also
    /// drops the outer `Option<Rc<...>>` so `is_capturing_keys` goes
    /// back to `false`.
    pub(crate) fn take_key_capture(&mut self) -> Option<crate::shortcut::KeyCaptureCallback> {
        let slot = self.key_capture.take()?;
        slot.borrow_mut().take()
    }

    /// Append an [`Action`](crate::action::Action) to a widget's arena
    /// node. Invoked by [`BuildContext::register_action`]; not meant
    /// to be called directly.
    pub(crate) fn push_action(&mut self, widget_id: WidgetId, action: crate::action::Action) {
        if let Some(node) = self.arena.get_mut(widget_id) {
            node.actions.push(action);
        }
    }

    /// Enqueue an intent for dispatch from `source`. Called from
    /// `collect_from_ctx` after a handler runs `ctx.send_intent(...)`
    /// and from the KeyDown shortcut-interception path.
    pub(crate) fn enqueue_intent(
        &mut self,
        source: WidgetId,
        intent: crate::intent::Intent,
        propagate_when_disabled: bool,
    ) {
        self.pending_intents
            .push((source, intent, propagate_when_disabled));
    }

    /// Dispatch every queued intent. Handlers may call
    /// `ctx.send_intent(...)` to enqueue more; the loop consumes
    /// those too until the queue drains. No ordering guarantee
    /// beyond "first-enqueued is first-dispatched"; the `pop` path
    /// uses `remove(0)` to keep that FIFO behavior.
    pub(crate) fn drain_pending_intents(&mut self) {
        while !self.pending_intents.is_empty() {
            let (source, intent, propagate) = self.pending_intents.remove(0);
            self.dispatch_intent(source, intent, propagate);
        }
    }

    /// Walk `source → root` invoking any [`Action`](crate::action::Action)
    /// whose `intent` name matches. The first enabled, `Handled`
    /// response stops the walk. A `Propagated` or disabled action
    /// (when the shortcut's `propagate_when_disabled` is true) lets
    /// the walk continue. A disabled action with
    /// `propagate_when_disabled == false` consumes the intent at that
    /// level without invoking a handler.
    pub(crate) fn dispatch_intent(
        &mut self,
        source: WidgetId,
        intent: crate::intent::Intent,
        propagate_when_disabled: bool,
    ) {
        // Pre-compute the source → root chain so the walk doesn't
        // need to hold any arena borrow while invoking handlers.
        let chain: Vec<WidgetId> = {
            let mut v = vec![source];
            let mut current = self.arena.parent(source);
            while let Some(id) = current {
                v.push(id);
                current = self.arena.parent(id);
            }
            v
        };

        for id in chain {
            if !self.arena.is_active(id) || !self.arena.is_enabled(id) {
                continue;
            }

            // Take out the first matching action by intent name so
            // we can invoke its FnMut handler without holding an
            // arena-wide borrow. The action is reinserted at its
            // original position so declaration order is preserved
            // for any follow-on dispatch.
            let Some((mut action, idx, enabled)) =
                self.arena.get_mut(id).and_then(|node| {
                    let idx = node
                        .actions
                        .iter()
                        .position(|a| a.intent == intent.name)?;
                    let enabled = node.actions[idx].is_enabled();
                    Some((node.actions.remove(idx), idx, enabled))
                })
            else {
                continue;
            };

            if !enabled {
                // Return the action untouched.
                if let Some(node) = self.arena.get_mut(id) {
                    node.actions.insert(idx, action);
                }
                if propagate_when_disabled {
                    continue;
                }
                return;
            }

            let mut ctx = EventContext::new().with_app_context(self.app_context.clone());
            let response = (action.handler)(&intent, &mut ctx);
            if let Some(node) = self.arena.get_mut(id) {
                node.actions.insert(idx, action);
            }
            self.collect_from_ctx(ctx, id);

            match response {
                crate::intent::IntentResponse::Handled => return,
                crate::intent::IntentResponse::Propagated => continue,
            }
        }
    }

    // --- Window-close request (drained by the app loop) ---

    /// Drain the "close this window" flag set by
    /// [`EventContext::close_window`] during dispatch.
    pub fn take_close_window_request(&mut self) -> bool {
        std::mem::replace(&mut self.close_window_requested, false)
    }

    /// Drain the pending locale switch raised by
    /// [`EventContext::set_locale`] during dispatch. The app layer
    /// (`WindowManager::drain_pending_locale_requests`) parses the
    /// result and routes it through `WindowManager::set_locale` so the
    /// `I18nManager`'s active locale, version signal, and layout
    /// direction all stay in sync with the tree.
    pub fn take_pending_locale_request(&mut self) -> Option<String> {
        self.pending_locale_request.take()
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

