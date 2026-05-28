//! Overlay system for tooltips, dropdown menus, context menus, and popovers.
//!
//! Overlays render outside the normal layout hierarchy. They float above the
//! main content, positioned relative to an anchor widget or the pointer.
//! The `OverlayManager` coordinates creation, positioning, stacking, dismissal,
//! event routing, and accessibility.

use std::rc::Rc;
use std::time::{Duration, Instant};

use bastyde_canvas::{Point, Rect, Size, Vec2};
use bastyde_tokens::Corner;

use crate::environment::LayoutDirection;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Callback invoked by the framework when an overlay is dismissed —
/// regardless of the dismiss path (Escape, click outside, pointer
/// leave, explicit API call, cascade). The anchor widget uses this
/// hook to reset its own interaction state so that SR-facing
/// properties like `set_expanded` on a `ComboBox` or a submenu
/// trigger stay consistent with the actual overlay-visible state.
///
/// Fired exactly once per overlay lifetime, at the point the
/// overlay is removed from the stack. `Fn` rather than `FnOnce`
/// simply because it's easier to pass around by `Rc`; the
/// framework only invokes it once.
pub type OverlayDismissCallback = Rc<dyn Fn()>;

/// Unique identifier for an active overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(u64);

impl OverlayId {
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }
}

/// How an overlay is positioned relative to its anchor.
#[derive(Debug, Clone)]
pub enum OverlayPlacement {
    /// Below the anchor, leading-edge aligned (dropdown).
    Below,
    /// Above the anchor (fallback when no space below).
    Above,
    /// To the trailing side of the anchor (submenu).
    TrailingEdge,
    /// At the pointer position (context menu).
    AtPointer(Point),
    /// Near the anchor with a preferred alignment and offset (tooltip).
    NearAnchor { offset: Vec2 },
    /// Centered within the viewport (dialog).
    Centered,
    /// Bottom-centered within the viewport (snackbar/toast).
    BottomCenter,
    /// Below the anchor if space allows, otherwise above (combo box dropdown).
    /// The viewport height is supplied by `position_overlays()` at layout time.
    BelowPreferred,
    /// Snaps content to a viewport corner with a per-axis margin
    /// (used by `ToastHost` for stacked toast notifications, also
    /// suitable for picture-in-picture, floating action overlays).
    /// Anchor bounds are ignored. The leading/trailing axis honours
    /// `LayoutDirection`: `TopTrailing` is top-right under LTR and
    /// top-left under RTL.
    ViewportCorner { corner: Corner, margin: Vec2 },
    /// Fills the entire viewport, anchor-independent. Used by the
    /// modal-presentation pipeline to mount a dialog scrim behind a
    /// centered modal panel — the scrim covers the full window so the
    /// content behind dims uniformly. Anchor bounds are ignored.
    FullViewport,
}

/// When an overlay is dismissed.
#[derive(Debug, Clone)]
pub enum DismissBehavior {
    /// Dismiss when the user clicks outside the overlay.
    ClickOutside,
    /// Dismiss when the user presses Escape.
    EscapeKey,
    /// Dismiss on either Escape or an outside click.
    EscapeOrClickOutside,
    /// Dismiss when the pointer leaves both anchor and overlay.
    PointerLeave { delay: Duration },
    /// Dismiss only via explicit API call.
    Manual,
}

/// Where the overlay renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayLayer {
    /// Rendered within the application window's wgpu surface.
    InTree,
    /// Rendered in a separate native OS window.
    NativePopup,
    /// Framework decides based on content size.
    Auto,
}

/// A request to show an overlay.
pub struct OverlayRequest {
    /// The root widget of the overlay content.
    pub content_id: WidgetId,
    /// The widget this overlay is anchored to.
    pub anchor: WidgetId,
    /// Positioning relative to the anchor.
    pub placement: OverlayPlacement,
    /// How the overlay is dismissed.
    pub dismiss: DismissBehavior,
    /// Rendering layer.
    pub layer: OverlayLayer,
    /// Parent overlay (for submenu cascading).
    pub parent_overlay: Option<OverlayId>,
    /// Invoked when the overlay is dismissed by any path. Use this
    /// to reset anchor-side state (e.g. `ComboBox.interaction`)
    /// when the framework tears down the overlay without going
    /// through the anchor's own key/tap handlers.
    pub on_dismiss: Option<OverlayDismissCallback>,
    /// Optional fade-in / fade-out duration. When `Some`, the
    /// framework attaches an animated opacity scope to `content_id`
    /// at show time (using the existing `set_opacity` rendering
    /// pipeline — no `Fade` widget required from the caller), tweens
    /// the opacity from 0 → 1 over `duration`, and on dismiss
    /// reverses the tween and defers the actual stack removal by
    /// `duration`. Construct with [`OverlayRequest::with_fade`] when
    /// the struct-literal idiom isn't ergonomic.
    pub fade_duration: Option<Duration>,
}

impl OverlayRequest {
    /// Attach a fade-in / fade-out animation to this request.
    /// `duration` controls both directions. The framework wires
    /// everything internally — caller does not create a `Fade`
    /// widget or manage a signal:
    ///
    /// ```ignore
    /// let req = OverlayRequest { content_id, anchor, ... }
    ///     .with_fade(theme.motion.duration_fast);
    /// ```
    pub fn with_fade(mut self, duration: Duration) -> Self {
        self.fade_duration = Some(duration);
        self
    }
}

/// Fade-on-show / fade-on-dismiss state for an overlay. Populated by
/// the framework when an [`OverlayRequest`] carries `fade_duration`.
/// The framework owns the `Signal<f32>` (an animated 0..1 opacity)
/// and applies it to the overlay's content via `set_opacity`, so the
/// caller doesn't need to wrap the content in a `Fade` widget — the
/// rendering walker's opacity scope (Item 1) does the work.
///
/// Mirrors the `pointer_leave_started_real/_sim` and
/// `shown_at_real/_sim` dual-clock pattern used elsewhere in
/// `ActiveOverlay`: the real-clock field drives the live event loop;
/// the sim-clock field drives the headless `tick_animations` /
/// `advance_time` test path so deterministic tests can advance the
/// fade-out window without `std::thread::sleep`.
#[derive(Clone)]
pub(crate) struct OverlayFadeState {
    /// Animated opacity (0..1) bound to the overlay's content via
    /// `WidgetTree::set_opacity`. The framework starts the tween at
    /// 0 and animates to 1 on show, then animates back to 0 on
    /// dismiss before the deferred removal fires.
    pub opacity: Signal<f32>,
    /// Tween duration on both directions. Picked from
    /// `theme.motion.duration_fast` for tooltip / popover and
    /// `duration_normal` for snackbar / dialog.
    pub duration: Duration,
    /// `Some(start_real)` when a dismiss has been requested and the
    /// fade-out tween has started. The real-clock processor
    /// considers the overlay ready for removal once
    /// `Instant::now() - start_real >= duration`.
    pub dismissing_started_real: Option<Instant>,
    /// `Some(start_sim)` set in lockstep with `dismissing_started_real`
    /// using the tree's `sim_clock`. The sim-clock processor uses
    /// it for deterministic headless tests.
    pub dismissing_started_sim: Option<Instant>,
}

/// An active overlay in the stack.
pub(crate) struct ActiveOverlay {
    pub id: OverlayId,
    pub content_id: WidgetId,
    pub anchor: WidgetId,
    pub placement: OverlayPlacement,
    pub dismiss: DismissBehavior,
    pub layer: OverlayLayer,
    pub parent_overlay: Option<OverlayId>,
    /// Computed bounds after positioning.
    pub bounds: Rect,
    /// Widget that had focus before this overlay was shown.
    /// Used to restore focus when the overlay is dismissed.
    pub focus_restore: Option<WidgetId>,
    /// When pointer-leave dismissal started (real time).
    pub pointer_leave_started_real: Option<std::time::Instant>,
    /// When pointer-leave dismissal started (simulated time).
    pub pointer_leave_started_sim: Option<std::time::Instant>,
    /// Dismiss automatically after this duration, if set.
    pub auto_dismiss_after: Option<Duration>,
    /// While the auto-dismiss timer is paused (via
    /// [`OverlayManager::pause_auto_dismiss`]), `auto_dismiss_after`
    /// is cleared and the time that *would have remained* is stashed
    /// here. [`OverlayManager::resume_auto_dismiss`] restores
    /// `auto_dismiss_after = Some(this)` and stamps a fresh
    /// `shown_at_*`. `None` whenever the overlay is not paused.
    pub paused_remaining: Option<Duration>,
    /// When the overlay was shown (real time).
    pub shown_at_real: std::time::Instant,
    /// When the overlay was shown (simulated time).
    pub shown_at_sim: std::time::Instant,
    /// Dismiss callback supplied by the show request. Invoked
    /// exactly once when the overlay is removed from the stack,
    /// regardless of dismiss path.
    pub on_dismiss: Option<OverlayDismissCallback>,
    /// Optional fade-in / fade-out state. Configured post-show via
    /// `OverlayManager::set_fade`. When `Some`, all dismiss paths
    /// (auto, escape, click-outside, pointer-leave, manual) defer
    /// the actual removal until the fade-out tween completes.
    pub fade: Option<OverlayFadeState>,
}

// Manual Debug impl: `Rc<dyn Fn()>` doesn't derive Debug, but the
// surrounding systems (tests, logging) want ActiveOverlay to be
// printable. Skip the callback field and tag it with a placeholder.
impl std::fmt::Debug for ActiveOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveOverlay")
            .field("id", &self.id)
            .field("content_id", &self.content_id)
            .field("anchor", &self.anchor)
            .field("placement", &self.placement)
            .field("dismiss", &self.dismiss)
            .field("layer", &self.layer)
            .field("parent_overlay", &self.parent_overlay)
            .field("bounds", &self.bounds)
            .field("focus_restore", &self.focus_restore)
            .field(
                "pointer_leave_started_real",
                &self.pointer_leave_started_real,
            )
            .field("pointer_leave_started_sim", &self.pointer_leave_started_sim)
            .field("auto_dismiss_after", &self.auto_dismiss_after)
            .field("shown_at_real", &self.shown_at_real)
            .field("shown_at_sim", &self.shown_at_sim)
            .field(
                "on_dismiss",
                &self.on_dismiss.as_ref().map(|_| "<callback>"),
            )
            .field("fading", &self.fade.is_some())
            .finish()
    }
}

/// Maximum overlay nesting depth. Bounds runaway cascades: a rich-tooltip
/// `[label](:key)` link loop (A→B→A) keeps minting fresh nested overlays
/// (and dormant widgets) on each hop with no natural ceiling. A real
/// menu-submenu or tooltip cascade never gets close to this — once a new
/// overlay would exceed it, `OverlayManager::show*` drops the request
/// instead of growing the stack without bound.
pub(crate) const MAX_OVERLAY_NESTING_DEPTH: usize = 12;

/// Manages the overlay stack — creation, positioning, dismissal, cascading.
pub struct OverlayManager {
    pub(crate) stack: Vec<ActiveOverlay>,
    next_id: u64,
    /// Latest known sim-clock value, mirrored from
    /// `WidgetTree::sim_clock` via [`Self::set_sim_clock`]. Read by
    /// `dismiss` to stamp `dismissing_started_sim` in lockstep with
    /// `dismissing_started_real`. Defaults to `Instant::now()` so
    /// constructions outside a tree (tests of OverlayManager in
    /// isolation) still produce sensible values.
    sim_clock: Instant,
    /// Monotonic counter bumped on every stack mutation (show /
    /// dismiss). External observers — notably the inspector's Overlays
    /// tab — bind to this signal to know when the visible overlay set
    /// has changed without polling. Mirrors the
    /// `ShortcutRegistry::version` pattern.
    version: Signal<u64>,
}

impl OverlayManager {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            next_id: 1,
            sim_clock: Instant::now(),
            version: Signal::new(0),
        }
    }

    /// Reactive handle bumped on every overlay mutation (show /
    /// dismiss / cascade). Cheap clone. Same shape as
    /// [`crate::shortcut::ShortcutRegistry::version`].
    pub fn version(&self) -> &Signal<u64> {
        &self.version
    }

    /// Bump the version signal. Called from every stack-mutating path.
    fn bump_version(&self) {
        self.version.set(self.version.get().wrapping_add(1));
    }

    /// Mirror the tree's sim_clock onto the manager so the fade
    /// dismiss path can stamp the sim-time start in lockstep with
    /// real time. Called by `WidgetTree` whenever `sim_clock` is
    /// advanced (e.g. from `tick_animations` and `advance_time`).
    pub(crate) fn set_sim_clock(&mut self, now_sim: Instant) {
        self.sim_clock = now_sim;
    }

    /// Show a new overlay. Returns the OverlayId.
    pub fn show(&mut self, request: OverlayRequest) -> OverlayId {
        self.show_with_auto_dismiss(request, None)
    }

    /// Show a new overlay that dismisses automatically after `duration`.
    pub fn show_for(&mut self, request: OverlayRequest, duration: Duration) -> OverlayId {
        self.show_with_auto_dismiss(request, Some(duration))
    }

    fn show_with_auto_dismiss(
        &mut self,
        request: OverlayRequest,
        auto_dismiss_after: Option<Duration>,
    ) -> OverlayId {
        let id = OverlayId::new(self.next_id);
        self.next_id += 1;

        // Bound cascade depth — see `MAX_OVERLAY_NESTING_DEPTH`. If this
        // overlay would nest deeper than the cap, drop it silently: don't
        // push, and return the (now unused) id so callers' follow-ups
        // (`set_shown_at_sim`, `set_top_focus_restore`) safely no-op on
        // the absent overlay. This is reachable by degenerate-but-real
        // user action (a cyclic tooltip `:key` cascade), so it must not
        // panic — graceful drop is the whole point.
        if self.ancestor_depth(request.parent_overlay) >= MAX_OVERLAY_NESTING_DEPTH {
            return id;
        }

        let now = std::time::Instant::now();

        let overlay = ActiveOverlay {
            id,
            content_id: request.content_id,
            anchor: request.anchor,
            placement: request.placement,
            dismiss: request.dismiss,
            layer: request.layer,
            parent_overlay: request.parent_overlay,
            bounds: Rect::ZERO,
            focus_restore: None,
            pointer_leave_started_real: None,
            pointer_leave_started_sim: None,
            auto_dismiss_after,
            paused_remaining: None,
            shown_at_real: now,
            shown_at_sim: now,
            on_dismiss: request.on_dismiss,
            fade: None,
        };
        self.stack.push(overlay);
        self.bump_version();
        id
    }

    /// Internal: install a framework-managed opacity signal as the
    /// overlay's fade state. Called by `WidgetTree::show_overlay`
    /// when [`OverlayRequest::fade_duration`] is `Some`. The
    /// framework also applies the same signal to `content_id` via
    /// `set_opacity` (so the rendering walker emits the per-frame
    /// opacity scope) and kicks off the 0→1 fade-in tween.
    pub(crate) fn attach_fade(&mut self, id: OverlayId, opacity: Signal<f32>, duration: Duration) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.fade = Some(OverlayFadeState {
                opacity,
                duration,
                dismissing_started_real: None,
                dismissing_started_sim: None,
            });
        }
    }

    /// Public read-only accessor for the fade state. Returns the
    /// duration if fade is configured, `None` otherwise. Used by
    /// `WidgetTree::dismiss_overlay` to know whether to leave the
    /// content active for the fade-out window.
    pub fn fade_duration(&self, id: OverlayId) -> Option<Duration> {
        self.stack
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.fade.as_ref().map(|f| f.duration))
    }

    pub fn next_auto_dismiss_deadline(&self) -> Option<std::time::Instant> {
        self.stack
            .iter()
            .filter_map(|overlay| {
                overlay
                    .auto_dismiss_after
                    .map(|delay| overlay.shown_at_real + delay)
            })
            .min()
    }

    /// Pause the auto-dismiss timer for an overlay shown with
    /// [`show_for`](Self::show_for). The remaining time
    /// (`auto_dismiss_after - elapsed`) is stashed; subsequent calls
    /// to [`next_auto_dismiss_deadline`](Self::next_auto_dismiss_deadline)
    /// ignore this overlay until [`resume_auto_dismiss`](Self::resume_auto_dismiss)
    /// is called. Idempotent — pausing an already-paused overlay is
    /// a no-op (the originally-stashed remaining time is preserved).
    ///
    /// Used by `ToastHost` to implement hover-pause: when the user
    /// is hovering over any live toast, all live toasts pause their
    /// timers so the user can read each one without losing the
    /// notification they're about to act on.
    ///
    /// No-op on overlays without `auto_dismiss_after` (persistent
    /// overlays don't have a timer to pause) and on unknown ids.
    pub fn pause_auto_dismiss(&mut self, id: OverlayId) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id)
            && overlay.paused_remaining.is_none()
            && let Some(delay) = overlay.auto_dismiss_after.take()
        {
            let elapsed = overlay.shown_at_real.elapsed();
            overlay.paused_remaining = Some(delay.saturating_sub(elapsed));
        }
    }

    /// Resume an auto-dismiss timer paused via
    /// [`pause_auto_dismiss`](Self::pause_auto_dismiss). The stashed
    /// remaining time becomes the new `auto_dismiss_after`, and
    /// `shown_at_real` / `shown_at_sim` are reset to now so the
    /// deadline computation works correctly. Idempotent — resuming
    /// an un-paused overlay is a no-op.
    pub fn resume_auto_dismiss(&mut self, id: OverlayId) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id)
            && let Some(remaining) = overlay.paused_remaining.take()
        {
            overlay.auto_dismiss_after = Some(remaining);
            let now = std::time::Instant::now();
            overlay.shown_at_real = now;
            overlay.shown_at_sim = self.sim_clock;
        }
    }

    /// Whether the auto-dismiss timer for an overlay is currently paused.
    /// `false` for overlays without `auto_dismiss_after`, unknown ids,
    /// and overlays whose timer is running.
    pub fn is_auto_dismiss_paused(&self, id: OverlayId) -> bool {
        self.stack
            .iter()
            .find(|o| o.id == id)
            .is_some_and(|o| o.paused_remaining.is_some())
    }

    pub(crate) fn set_shown_at_sim(&mut self, id: OverlayId, shown_at_sim: std::time::Instant) {
        if let Some(overlay) = self.stack.iter_mut().find(|overlay| overlay.id == id) {
            overlay.shown_at_sim = shown_at_sim;
        }
    }

    /// Count the ancestor chain length for an overlay whose parent is
    /// `parent` — i.e. the nesting depth the *new* overlay would have.
    /// A root (`parent == None`) is depth 0; a child of a root is depth
    /// 1; and so on. The walk is bounded by the stack length so a
    /// malformed parent cycle can't loop forever.
    fn ancestor_depth(&self, parent: Option<OverlayId>) -> usize {
        let mut depth = 0;
        let mut current = parent;
        while let Some(p) = current {
            depth += 1;
            if depth > self.stack.len() {
                // Defensive: malformed parent cycle. Report a depth that
                // trips the guard rather than spinning.
                break;
            }
            current = self
                .stack
                .iter()
                .find(|overlay| overlay.id == p)
                .and_then(|overlay| overlay.parent_overlay);
        }
        depth
    }

    pub(crate) fn is_descendant_of(&self, child: OverlayId, ancestor: OverlayId) -> bool {
        let mut current = self
            .stack
            .iter()
            .find(|overlay| overlay.id == child)
            .and_then(|overlay| overlay.parent_overlay);

        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            current = self
                .stack
                .iter()
                .find(|overlay| overlay.id == parent)
                .and_then(|overlay| overlay.parent_overlay);
        }

        false
    }

    pub(crate) fn overlay(&self, id: OverlayId) -> Option<&ActiveOverlay> {
        self.stack.iter().find(|overlay| overlay.id == id)
    }

    pub(crate) fn topmost_centered(&self) -> Option<&ActiveOverlay> {
        self.stack
            .iter()
            .rev()
            .find(|overlay| matches!(overlay.placement, OverlayPlacement::Centered))
    }

    /// Dismiss an overlay and all its children (cascade), returning the
    /// dismissed content widget IDs and the overlay's focus_restore target.
    pub fn dismiss_with_focus_restore(
        &mut self,
        id: OverlayId,
    ) -> (Vec<WidgetId>, Option<WidgetId>) {
        let focus_restore = self
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .and_then(|overlay| overlay.focus_restore);
        let dismissed = self.dismiss(id);
        (dismissed, focus_restore)
    }

    /// Dismiss all descendant overlays of `parent`, optionally preserving the
    /// subtree rooted at `preserve`.
    pub fn dismiss_descendants_of(
        &mut self,
        parent: OverlayId,
        preserve: Option<OverlayId>,
    ) -> (Vec<WidgetId>, Option<WidgetId>) {
        let mut to_dismiss = Vec::new();

        for overlay in &self.stack {
            if !self.is_descendant_of(overlay.id, parent) {
                continue;
            }
            if preserve
                .is_some_and(|keep| overlay.id == keep || self.is_descendant_of(overlay.id, keep))
            {
                continue;
            }
            to_dismiss.push(overlay.id);
        }

        if to_dismiss.is_empty() {
            return (Vec::new(), None);
        }

        let focus_restore = self
            .stack
            .iter()
            .rev()
            .find(|overlay| to_dismiss.contains(&overlay.id))
            .and_then(|overlay| overlay.focus_restore);

        let dismissed_content: Vec<WidgetId> = self
            .stack
            .iter()
            .filter(|overlay| to_dismiss.contains(&overlay.id))
            .map(|overlay| overlay.content_id)
            .collect();
        let callbacks: Vec<OverlayDismissCallback> = self
            .stack
            .iter()
            .filter(|overlay| to_dismiss.contains(&overlay.id))
            .filter_map(|overlay| overlay.on_dismiss.clone())
            .collect();
        self.stack
            .retain(|overlay| !to_dismiss.contains(&overlay.id));
        for cb in callbacks {
            cb();
        }

        (dismissed_content, focus_restore)
    }

    /// Update the placement of an existing overlay.
    pub fn update_placement(&mut self, id: OverlayId, placement: OverlayPlacement) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.placement = placement;
        }
    }

    /// Update the parent-overlay link of an existing overlay. Used by the
    /// modal-presentation pipeline to retroactively attach the dialog
    /// scrim (pushed first, below the modal in the stack) to the modal
    /// (pushed second) so that dismissing the modal cascades through
    /// `dismiss_immediate` and also dismisses the scrim.
    pub fn set_parent_overlay(&mut self, id: OverlayId, parent: Option<OverlayId>) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.parent_overlay = parent;
        }
    }

    /// Dismiss an overlay and all its children (cascade).
    /// Returns the content widget IDs of all dismissed overlays.
    ///
    /// **Fade-aware**: when an overlay was shown with
    /// [`OverlayRequest::with_fade`] and is not yet fading out, this
    /// method instead kicks off the fade-out tween on the framework-
    /// owned opacity signal and marks `dismiss_at`, returning an
    /// empty vec — the actual stack removal and content dormancy
    /// happen later via
    /// [`process_pending_fade_dismissals`](Self::process_pending_fade_dismissals).
    /// Cascaded descendants vanish with the leaf's fade-out (they're
    /// typically submenus the user dismissed *via* the leaf, and a
    /// per-descendant tween would compete with the leaf's).
    pub fn dismiss(&mut self, id: OverlayId) -> Vec<WidgetId> {
        // Fade gate: if the target overlay has fade and isn't
        // already fading out, kick off the fade-out and defer the
        // entire cascade. Stamps both real and sim start times in
        // lockstep — the sim time uses the manager's mirrored
        // `sim_clock`, kept in sync by `WidgetTree::set_sim_clock`.
        let sim_now = self.sim_clock;
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id)
            && let Some(fade) = &mut overlay.fade
            && fade.dismissing_started_real.is_none()
        {
            // Animate opacity 1 → 0 over `duration`. Uses the same
            // try_animate_with_options path the rest of the
            // animation system uses; the scheduler picks it up next
            // frame and ticks the signal, dirty-marking the
            // content's opacity binding for repaint.
            let _ = fade
                .opacity
                .try_animate_with_options(crate::animation::AnimationRequest {
                    target: 0.0,
                    duration: fade.duration,
                    easing: bastyde_tokens::Easing::EaseOut,
                    frame_interval: None,
                    looping: false,
                    epsilon: 0.0,
                    max_duration: None,
                });
            let now_real = Instant::now();
            fade.dismissing_started_real = Some(now_real);
            fade.dismissing_started_sim = Some(sim_now);
            return Vec::new();
        }
        self.dismiss_immediate(id)
    }

    /// Internal: same shape as the original `dismiss`, but bypasses
    /// the fade gate. Used both by `dismiss` (no fade configured /
    /// already fading out) and by `process_pending_fade_dismissals`
    /// when a fade-out tween has completed.
    fn dismiss_immediate(&mut self, id: OverlayId) -> Vec<WidgetId> {
        // Collect IDs to dismiss: the target + all descendants
        let mut to_dismiss = vec![id];
        let mut i = 0;
        while i < to_dismiss.len() {
            let parent = to_dismiss[i];
            for overlay in &self.stack {
                if overlay.parent_overlay == Some(parent) && !to_dismiss.contains(&overlay.id) {
                    to_dismiss.push(overlay.id);
                }
            }
            i += 1;
        }
        let dismissed_content: Vec<WidgetId> = self
            .stack
            .iter()
            .filter(|o| to_dismiss.contains(&o.id))
            .map(|o| o.content_id)
            .collect();
        // Collect dismiss callbacks (via Rc::clone) before retain
        // so we can invoke them AFTER the borrow is released.
        // Callbacks may do anything, including touching the arena,
        // so running them mid-retain would risk re-entrancy.
        let callbacks: Vec<OverlayDismissCallback> = self
            .stack
            .iter()
            .filter(|o| to_dismiss.contains(&o.id))
            .filter_map(|o| o.on_dismiss.clone())
            .collect();
        self.stack.retain(|o| !to_dismiss.contains(&o.id));
        if !to_dismiss.is_empty() {
            self.bump_version();
        }
        for cb in callbacks {
            cb();
        }
        dismissed_content
    }

    /// Drain overlays whose real-clock fade-out tween has completed.
    /// Call from the live layout pass; the framework dormants the
    /// returned content widget IDs and restores focus where
    /// appropriate. Each entry is
    /// `(overlay_id, dismissed_content_ids, focus_restore)` so the
    /// layout pass can run the same dormant-and-restore-focus flow
    /// it uses for
    /// [`dismiss_with_focus_restore`](Self::dismiss_with_focus_restore).
    pub fn process_pending_fade_dismissals(
        &mut self,
        now: Instant,
    ) -> Vec<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        self.process_pending_fade_dismissals_with(|fade| {
            let started = fade.dismissing_started_real?;
            Some(now.saturating_duration_since(started) >= fade.duration)
        })
    }

    /// Sim-clock variant for deterministic headless tests. Same
    /// shape as [`process_pending_fade_dismissals`](Self::process_pending_fade_dismissals)
    /// but reads `dismissing_started_sim`.
    pub fn process_pending_fade_dismissals_sim(
        &mut self,
        now_sim: Instant,
    ) -> Vec<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        self.process_pending_fade_dismissals_with(|fade| {
            let started = fade.dismissing_started_sim?;
            Some(now_sim.saturating_duration_since(started) >= fade.duration)
        })
    }

    fn process_pending_fade_dismissals_with(
        &mut self,
        mut elapsed_done: impl FnMut(&OverlayFadeState) -> Option<bool>,
    ) -> Vec<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        let ready: Vec<(OverlayId, Option<WidgetId>)> = self
            .stack
            .iter()
            .filter_map(|o| {
                let fade = o.fade.as_ref()?;
                if elapsed_done(fade)? {
                    Some((o.id, o.focus_restore))
                } else {
                    None
                }
            })
            .collect();
        ready
            .into_iter()
            .map(|(id, focus_restore)| {
                let dismissed = self.dismiss_immediate(id);
                (id, dismissed, focus_restore)
            })
            .collect()
    }

    /// Earliest real-clock deadline at which a fading-out overlay
    /// wants to finish its dismissal. Used by the event-loop wakeup
    /// logic to schedule the next frame.
    pub fn next_fade_dismiss_deadline(&self) -> Option<Instant> {
        self.stack
            .iter()
            .filter_map(|o| {
                let fade = o.fade.as_ref()?;
                let started = fade.dismissing_started_real?;
                Some(started + fade.duration)
            })
            .min()
    }

    /// Dismiss the topmost overlay unconditionally (e.g., ArrowLeft for submenu cascading).
    /// Returns the overlay ID, content widget IDs, and focus_restore target.
    pub fn dismiss_top(&mut self) -> Option<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        if let Some(overlay) = self.stack.last() {
            let id = overlay.id;
            let focus_restore = overlay.focus_restore;
            let content_ids = self.dismiss(id);
            Some((id, content_ids, focus_restore))
        } else {
            None
        }
    }

    /// Try to dismiss the topmost overlay on Escape, respecting `DismissBehavior`.
    /// Only dismisses if the overlay's behavior includes Escape dismissal.
    /// Returns the overlay ID, content widget IDs, and focus_restore target, or `None`
    /// if the topmost overlay does not allow Escape dismissal.
    pub fn try_dismiss_top_on_escape(
        &mut self,
    ) -> Option<(OverlayId, Vec<WidgetId>, Option<WidgetId>)> {
        let dominated_by_escape = self.stack.last().is_some_and(|o| {
            matches!(
                o.dismiss,
                DismissBehavior::EscapeKey | DismissBehavior::EscapeOrClickOutside
            )
        });
        if dominated_by_escape {
            self.dismiss_top()
        } else {
            None
        }
    }

    /// Set the focus_restore target for the topmost overlay.
    pub fn set_top_focus_restore(&mut self, focus_restore: WidgetId) {
        if let Some(overlay) = self.stack.last_mut() {
            overlay.focus_restore = Some(focus_restore);
        }
    }

    /// Dismiss all overlays.
    /// Returns the content widget IDs of all dismissed overlays.
    /// Fires every dismissed overlay's `on_dismiss` callback after the
    /// stack is cleared — same contract as [`dismiss`](Self::dismiss),
    /// so wrappers like [`PopoverButton`](crate::widget::EventContext)'s
    /// `popover_open` signal flip back to `false` when a `MenuItem`
    /// fires `ctx.dismiss_all_overlays()`. Without this, the trigger's
    /// next click would observe stale-true and silently retoggle
    /// instead of reopening the menu.
    pub fn dismiss_all(&mut self) -> Vec<WidgetId> {
        let content_ids: Vec<WidgetId> = self.stack.iter().map(|o| o.content_id).collect();
        if content_ids.is_empty() {
            return content_ids;
        }
        // Collect dismiss callbacks (via Rc::clone) before clear so we
        // can invoke them AFTER the borrow is released. Callbacks may
        // do anything, including touching the arena, so running them
        // mid-clear would risk re-entrancy. Mirrors the pattern in
        // [`dismiss_immediate`](Self::dismiss_immediate).
        let callbacks: Vec<OverlayDismissCallback> = self
            .stack
            .iter()
            .filter_map(|o| o.on_dismiss.clone())
            .collect();
        self.stack.clear();
        self.bump_version();
        for cb in callbacks {
            cb();
        }
        content_ids
    }

    /// Whether there are any active overlays.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Number of active overlays.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Get all active overlay content widget IDs (for rendering).
    pub fn active_content_ids(&self) -> Vec<WidgetId> {
        self.stack.iter().map(|o| o.content_id).collect()
    }

    /// Get all active overlay IDs (for testing/querying). Excludes
    /// overlays currently fading out — once a dismiss has been
    /// requested the overlay is conceptually gone (the visible
    /// opacity tween is on the way to 0 and the deferred removal
    /// will fire on the next layout pass after the fade-out
    /// completes), so user code asking "is this overlay still up?"
    /// gets the expected answer.
    pub fn active_ids(&self) -> Vec<OverlayId> {
        self.stack
            .iter()
            .filter(|o| {
                o.fade
                    .as_ref()
                    .is_none_or(|f| f.dismissing_started_real.is_none())
            })
            .map(|o| o.id)
            .collect()
    }

    /// Get the anchor widget for an overlay.
    pub fn anchor_for(&self, id: OverlayId) -> Option<WidgetId> {
        self.stack.iter().find(|o| o.id == id).map(|o| o.anchor)
    }

    /// Get the topmost overlay.
    #[allow(dead_code)] // V2 API: used for overlay z-ordering and focus management
    pub(crate) fn topmost(&self) -> Option<&ActiveOverlay> {
        self.stack.last()
    }

    /// Check if a point hits any overlay (topmost first).
    /// Returns the overlay ID if hit, None if the point is outside all overlays.
    pub fn hit_test(&self, point: Point) -> Option<OverlayId> {
        for overlay in self.stack.iter().rev() {
            if overlay.bounds.contains(point) {
                return Some(overlay.id);
            }
        }
        None
    }

    /// Handle a click-outside event: if the click is outside all overlays
    /// with ClickOutside dismiss behavior, dismiss them.
    /// Returns the content widget IDs of dismissed overlays (empty if none)
    /// and the focus-restore target — the widget that was focused before
    /// the *bottommost* dismissed overlay opened. Topmost overlays'
    /// `focus_restore` would point inside an overlay that's also being
    /// dismissed in the same pass, which would leave focus on a
    /// dormant widget; the bottommost target represents focus before
    /// any of the dismissed overlays opened. Aligns the click-outside
    /// path with the Esc / ArrowLeft-cascade paths, both of which
    /// already restore focus from the dismissed overlay.
    pub fn handle_click_outside(&mut self, point: Point) -> (Vec<WidgetId>, Option<WidgetId>) {
        if self.stack.is_empty() {
            return (Vec::new(), None);
        }

        // Check if the point is inside any overlay
        if self.hit_test(point).is_some() {
            return (Vec::new(), None);
        }

        // Dismiss all overlays that should close on an outside click.
        let to_dismiss: Vec<OverlayId> = self
            .stack
            .iter()
            .filter(|o| {
                matches!(
                    o.dismiss,
                    DismissBehavior::ClickOutside
                        | DismissBehavior::EscapeOrClickOutside
                        | DismissBehavior::PointerLeave { .. }
                )
            })
            .map(|o| o.id)
            .collect();

        if to_dismiss.is_empty() {
            return (Vec::new(), None);
        }

        let focus_restore = self
            .stack
            .iter()
            .find(|o| to_dismiss.contains(&o.id))
            .and_then(|o| o.focus_restore);

        let mut all_dismissed = Vec::new();
        for id in to_dismiss {
            all_dismissed.extend(self.dismiss(id));
        }
        (all_dismissed, focus_restore)
    }

    /// Compute overlay positions based on anchor bounds.
    /// Called after layout to position overlays correctly.
    /// `viewport` is (width, height) used for clamping overlays to the visible area.
    ///
    /// `anchor_bounds_fn` returns `None` when the anchor widget is no
    /// longer in the arena (destroyed by a host's rebuild while the
    /// overlay is still up). In that case the overlay's bounds are
    /// left untouched — keeping it at its last valid position rather
    /// than collapsing to the (0,0) origin from a `Rect::ZERO`
    /// fallback.
    pub fn position_overlays(
        &mut self,
        anchor_bounds_fn: impl Fn(WidgetId) -> Option<Rect>,
        viewport: (f32, f32),
        layout_direction: LayoutDirection,
    ) {
        let (vw, vh) = viewport;
        let rtl = matches!(layout_direction, LayoutDirection::RightToLeft);
        for overlay in &mut self.stack {
            let Some(anchor) = anchor_bounds_fn(overlay.anchor) else {
                // Anchor destroyed; preserve the previous bounds.
                continue;
            };
            let content_size = overlay.bounds.size(); // Will be set from content layout

            overlay.bounds = match &overlay.placement {
                OverlayPlacement::Below => Rect::new(
                    anchor.x,
                    anchor.y + anchor.height + 4.0,
                    content_size.width.max(anchor.width),
                    content_size.height,
                ),
                OverlayPlacement::Above => Rect::new(
                    anchor.x,
                    anchor.y - content_size.height - 4.0,
                    content_size.width.max(anchor.width),
                    content_size.height,
                ),
                OverlayPlacement::TrailingEdge => {
                    let x_right = anchor.x + anchor.width + 2.0;
                    let fits_right = x_right + content_size.width <= vw;
                    let x = if fits_right {
                        x_right
                    } else {
                        // Fallback: open to the leading edge
                        anchor.x - content_size.width - 2.0
                    };
                    let y = anchor.y.min(vh - content_size.height).max(0.0);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::AtPointer(point) => {
                    // Clamp to viewport so menus don't overflow off-screen
                    let x = point.x.min(vw - content_size.width).max(0.0);
                    let y = if point.y + content_size.height <= vh {
                        point.y
                    } else {
                        // Not enough space below pointer — open above
                        (point.y - content_size.height).max(0.0)
                    };
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::NearAnchor { offset } => {
                    // Prefer below the anchor at `offset` + 4 px.
                    // Flip above when the content would otherwise spill
                    // past the viewport bottom — same pattern as
                    // `BelowPreferred`. Without this, a tooltip whose
                    // anchor sits near the window edge gets clipped by
                    // the surface bounds (overlays paint unclipped, but
                    // the window itself still bounds the framebuffer).
                    let below_y = anchor.y + anchor.height + offset.y + 4.0;
                    let fits_below = below_y + content_size.height <= vh;
                    let y = if fits_below {
                        below_y
                    } else {
                        // Symmetric offset above: same gap as below.
                        let above_y = anchor.y - content_size.height - offset.y - 4.0;
                        above_y.max(0.0)
                    };
                    // Horizontal anchoring is direction-aware: LTR aligns
                    // the content's leading (left) edge to the anchor's
                    // left edge + offset; RTL mirrors it, aligning the
                    // content's trailing (right) edge to the anchor's
                    // right edge - offset. The clamp then keeps it in view
                    // when the anchor is near a viewport edge.
                    let unclamped_x = if rtl {
                        anchor.x + anchor.width - content_size.width - offset.x
                    } else {
                        anchor.x + offset.x
                    };
                    let x = unclamped_x.min(vw - content_size.width).max(0.0);
                    Rect::new(x, y, content_size.width, content_size.height)
                }
                OverlayPlacement::Centered => Rect::new(
                    ((vw - content_size.width) / 2.0).max(0.0),
                    ((vh - content_size.height) / 2.0).max(0.0),
                    content_size.width.min(vw),
                    content_size.height.min(vh),
                ),
                OverlayPlacement::BottomCenter => Rect::new(
                    ((vw - content_size.width) / 2.0).max(0.0),
                    (vh - content_size.height - 24.0).max(0.0),
                    content_size.width.min(vw),
                    content_size.height.min(vh),
                ),
                OverlayPlacement::BelowPreferred => {
                    let below_y = anchor.y + anchor.height + 4.0;
                    let fits_below = below_y + content_size.height <= vh;
                    let y = if fits_below {
                        below_y
                    } else {
                        anchor.y - content_size.height - 4.0
                    };
                    // Clamp horizontally
                    let x = anchor.x.min(vw - content_size.width).max(0.0);
                    Rect::new(
                        x,
                        y,
                        content_size.width.max(anchor.width),
                        content_size.height,
                    )
                }
                OverlayPlacement::ViewportCorner { corner, margin } => {
                    let (x, y) = corner.resolve(
                        (content_size.width, content_size.height),
                        (vw, vh),
                        (margin.x, margin.y),
                        rtl,
                    );
                    Rect::new(
                        x,
                        y,
                        content_size.width.min(vw),
                        content_size.height.min(vh),
                    )
                }
                OverlayPlacement::FullViewport => Rect::new(0.0, 0.0, vw, vh),
            };
        }
    }

    /// Set the content bounds for an overlay (after its content has been laid out).
    pub fn set_content_bounds(&mut self, id: OverlayId, size: Size) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.bounds = Rect::new(overlay.bounds.x, overlay.bounds.y, size.width, size.height);
        }
    }

    /// Get overlay by content widget ID (for routing events to the correct overlay).
    pub fn find_by_content(&self, content_id: WidgetId) -> Option<OverlayId> {
        self.stack
            .iter()
            .find(|o| o.content_id == content_id)
            .map(|o| o.id)
    }

    /// Change the dismiss behavior of an active overlay in place.
    ///
    /// Used by rich tooltips that promote from "ephemeral hover" to
    /// "sticky panel" after a dwell timer: at t=2s the tooltip calls
    /// this to swap `PointerLeave` for `EscapeOrClickOutside`, so the
    /// overlay stops vanishing the moment the pointer leaves the
    /// anchor. Also cancels any in-flight pointer-leave countdown.
    pub fn set_dismiss(&mut self, id: OverlayId, behavior: DismissBehavior) {
        if let Some(overlay) = self.stack.iter_mut().find(|o| o.id == id) {
            overlay.dismiss = behavior;
            overlay.pointer_leave_started_real = None;
            overlay.pointer_leave_started_sim = None;
        }
    }
}

impl Default for OverlayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OverlayManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverlayManager")
            .field("active_count", &self.stack.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::KeyData;

    fn fake_id(n: u64) -> WidgetId {
        KeyData::from_ffi(n).into()
    }

    #[test]
    fn dismiss_all_fires_on_dismiss_callbacks() {
        // Regression: a `MenuItem`'s tap handler calls
        // `ctx.dismiss_all_overlays()` to close the menu after firing
        // its action. The dismiss callback set on the parent
        // `PopoverButton`/`PopoverIconButton`'s `OverlayRequest`
        // (which flips `popover_open` back to `false`) must fire so
        // the next trigger click reopens the menu instead of
        // observing stale-true and silently retoggling.
        use std::cell::Cell;
        use std::rc::Rc;
        let mut mgr = OverlayManager::new();
        let fired_a = Rc::new(Cell::new(0_u32));
        let fired_b = Rc::new(Cell::new(0_u32));
        let cb_a: OverlayDismissCallback = {
            let f = fired_a.clone();
            Rc::new(move || f.set(f.get() + 1))
        };
        let cb_b: OverlayDismissCallback = {
            let f = fired_b.clone();
            Rc::new(move || f.set(f.get() + 1))
        };
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: Some(cb_a),
            fade_duration: None,
        });
        mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: Some(cb_b),
            fade_duration: None,
        });
        let dismissed = mgr.dismiss_all();
        assert_eq!(dismissed.len(), 2);
        assert!(mgr.is_empty());
        assert_eq!(
            fired_a.get(),
            1,
            "first overlay's on_dismiss must fire exactly once",
        );
        assert_eq!(
            fired_b.get(),
            1,
            "second overlay's on_dismiss must fire exactly once",
        );
    }

    #[test]
    fn show_and_dismiss() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(mgr.len(), 1);

        mgr.dismiss(id);
        assert!(mgr.is_empty());
    }

    #[test]
    fn cascade_dismissal() {
        let mut mgr = OverlayManager::new();
        let parent = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        let _child = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(10),
            placement: OverlayPlacement::TrailingEdge,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: Some(parent),
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(mgr.len(), 2);

        // Dismissing parent cascades to child
        mgr.dismiss(parent);
        assert!(mgr.is_empty());
    }

    #[test]
    fn cascade_depth_is_bounded() {
        // A cyclic tooltip `:key` cascade (A→B→A) keeps minting nested
        // overlays with no natural ceiling. `MAX_OVERLAY_NESTING_DEPTH`
        // bounds it: once a new overlay would nest at the cap, `show`
        // drops it rather than growing the stack forever — and must not
        // panic, since this is reachable by real user clicking.
        let mut mgr = OverlayManager::new();
        let mut parent = mgr.show(OverlayRequest {
            content_id: fake_id(100),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        // Root is depth 0; fill the chain so MAX overlays exist, the
        // deepest at depth MAX-1.
        for i in 1..MAX_OVERLAY_NESTING_DEPTH {
            parent = mgr.show(OverlayRequest {
                content_id: fake_id(100 + i as u64),
                anchor: fake_id(1),
                placement: OverlayPlacement::Below,
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: Some(parent),
                on_dismiss: None,
                fade_duration: None,
            });
        }
        assert_eq!(
            mgr.len(),
            MAX_OVERLAY_NESTING_DEPTH,
            "chain should fill exactly to the cap"
        );

        // The next child would nest at depth == MAX → dropped.
        let dropped = mgr.show(OverlayRequest {
            content_id: fake_id(999),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: Some(parent),
            on_dismiss: None,
            fade_duration: None,
        });
        assert_eq!(
            mgr.len(),
            MAX_OVERLAY_NESTING_DEPTH,
            "over-cap overlay must not be pushed"
        );
        assert!(
            mgr.stack.iter().all(|o| o.id != dropped),
            "the dropped overlay id must not appear in the stack"
        );
    }

    #[test]
    fn dismiss_top() {
        let mut mgr = OverlayManager::new();
        let _a = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        let b = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        let dismissed = mgr.dismiss_top();
        assert_eq!(dismissed.map(|(id, _, _)| id), Some(b));
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn click_outside_dismisses() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        // Set overlay bounds
        let id = mgr.active_ids()[0];
        mgr.set_content_bounds(id, Size::new(100.0, 50.0));

        // Click inside — no dismiss
        let (dismissed, _) = mgr.handle_click_outside(Point::new(50.0, 25.0));
        assert!(dismissed.is_empty());
        assert_eq!(mgr.len(), 1);

        // Click outside — dismissed
        let (dismissed, _) = mgr.handle_click_outside(Point::new(500.0, 500.0));
        assert!(!dismissed.is_empty());
        assert!(mgr.is_empty());
    }

    #[test]
    fn click_outside_returns_focus_restore() {
        let mut mgr = OverlayManager::new();
        let trigger = fake_id(99);
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        let id = mgr.active_ids()[0];
        mgr.set_content_bounds(id, Size::new(100.0, 50.0));
        mgr.set_top_focus_restore(trigger);

        let (dismissed, focus_restore) = mgr.handle_click_outside(Point::new(500.0, 500.0));
        assert_eq!(dismissed.len(), 1);
        assert_eq!(focus_restore, Some(trigger));
    }

    #[test]
    fn click_outside_focus_restore_picks_bottommost() {
        // When click-outside dismisses several stacked top-level
        // overlays in one pass, focus should land on the *oldest*
        // overlay's restore target — the focus state from before any
        // overlay opened. The topmost overlay's restore target points
        // inside the (now-dismissed) overlay below it.
        let mut mgr = OverlayManager::new();
        let pre_overlay_focus = fake_id(99);
        let inside_a = fake_id(50);
        let a = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        mgr.set_content_bounds(a, Size::new(100.0, 50.0));
        mgr.set_top_focus_restore(pre_overlay_focus);
        let b = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        mgr.set_content_bounds(b, Size::new(100.0, 50.0));
        mgr.set_top_focus_restore(inside_a);

        let (_, focus_restore) = mgr.handle_click_outside(Point::new(500.0, 500.0));
        assert_eq!(focus_restore, Some(pre_overlay_focus));
    }

    #[test]
    fn manual_dismiss_ignores_click_outside() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        let (dismissed, _) = mgr.handle_click_outside(Point::new(500.0, 500.0));
        assert!(dismissed.is_empty());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn escape_dismisses_escape_or_click_outside() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        let dismissed = mgr.try_dismiss_top_on_escape();
        assert_eq!(dismissed.map(|(oid, _, _)| oid), Some(id));
        assert!(mgr.is_empty());
    }

    #[test]
    fn escape_dismisses_escape_key_only() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeKey,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        // Escape should dismiss
        let dismissed = mgr.try_dismiss_top_on_escape();
        assert_eq!(dismissed.map(|(oid, _, _)| oid), Some(id));
        assert!(mgr.is_empty());
    }

    #[test]
    fn escape_does_not_dismiss_click_outside_only() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::ClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert!(mgr.try_dismiss_top_on_escape().is_none());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn escape_does_not_dismiss_manual() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        assert!(mgr.try_dismiss_top_on_escape().is_none());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn click_outside_dismisses_escape_or_click_outside() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeOrClickOutside,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        let id = mgr.active_ids()[0];
        mgr.set_content_bounds(id, Size::new(100.0, 50.0));

        let (dismissed, _) = mgr.handle_click_outside(Point::new(500.0, 500.0));
        assert!(!dismissed.is_empty());
        assert!(mgr.is_empty());
    }

    #[test]
    fn click_outside_does_not_dismiss_escape_key_only() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::EscapeKey,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        let (dismissed, _) = mgr.handle_click_outside(Point::new(500.0, 500.0));
        assert!(dismissed.is_empty());
        assert_eq!(mgr.len(), 1);
    }

    #[test]
    fn active_content_ids() {
        let mut mgr = OverlayManager::new();
        mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        mgr.show(OverlayRequest {
            content_id: fake_id(20),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        let ids = mgr.active_content_ids();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], fake_id(10));
        assert_eq!(ids[1], fake_id(20));
    }

    #[test]
    fn hit_test_topmost_first() {
        let mut mgr = OverlayManager::new();
        let a = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        let b = mgr.show(OverlayRequest {
            content_id: fake_id(11),
            anchor: fake_id(2),
            placement: OverlayPlacement::Below,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        // Both overlays at origin with same bounds
        mgr.set_content_bounds(a, Size::new(100.0, 50.0));
        mgr.set_content_bounds(b, Size::new(100.0, 50.0));

        // Hit test should find topmost (b)
        assert_eq!(mgr.hit_test(Point::new(50.0, 25.0)), Some(b));
    }

    #[test]
    fn centered_placement_uses_viewport_center() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        mgr.set_content_bounds(id, Size::new(240.0, 120.0));
        mgr.position_overlays(
            |_| Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );

        let bounds = mgr
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .unwrap()
            .bounds;
        assert!((bounds.x - 280.0).abs() < 0.01);
        assert!((bounds.y - 240.0).abs() < 0.01);
    }

    #[test]
    fn bottom_center_placement_uses_viewport_bottom_margin() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::BottomCenter,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });

        mgr.set_content_bounds(id, Size::new(240.0, 64.0));
        mgr.position_overlays(
            |_| Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );

        let bounds = mgr
            .stack
            .iter()
            .find(|overlay| overlay.id == id)
            .unwrap()
            .bounds;
        assert!((bounds.x - 280.0).abs() < 0.01);
        assert!((bounds.y - 512.0).abs() < 0.01);
    }

    // --- ViewportCorner placement ---

    fn show_corner_overlay(
        mgr: &mut OverlayManager,
        corner: Corner,
        margin: Vec2,
        size: Size,
    ) -> OverlayId {
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(10),
            anchor: fake_id(1),
            placement: OverlayPlacement::ViewportCorner { corner, margin },
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        mgr.set_content_bounds(id, size);
        id
    }

    fn overlay_bounds(mgr: &OverlayManager, id: OverlayId) -> Rect {
        mgr.stack.iter().find(|o| o.id == id).unwrap().bounds
    }

    #[test]
    fn viewport_corner_top_leading_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopLeading,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert!((b.x - 24.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 24.0).abs() < 0.01, "y = {}", b.y);
    }

    #[test]
    fn viewport_corner_top_trailing_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopTrailing,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        // 800 - 380 - 24 = 396
        assert!((b.x - 396.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 24.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_bottom_leading_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomLeading,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        // 600 - 100 - 24 = 476
        assert!((b.x - 24.0).abs() < 0.01);
        assert!((b.y - 476.0).abs() < 0.01, "y = {}", b.y);
    }

    #[test]
    fn viewport_corner_bottom_trailing_ltr() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomTrailing,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert!((b.x - 396.0).abs() < 0.01);
        assert!((b.y - 476.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_top_trailing_rtl_flips_to_left() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopTrailing,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::RightToLeft,
        );
        let b = overlay_bounds(&mgr, id);
        // RTL flips Trailing to physical left
        assert!((b.x - 24.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 24.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_bottom_leading_rtl_flips_to_right() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomLeading,
            Vec2::new(24.0, 24.0),
            Size::new(380.0, 100.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::RightToLeft,
        );
        let b = overlay_bounds(&mgr, id);
        assert!((b.x - 396.0).abs() < 0.01, "x = {}", b.x);
        assert!((b.y - 476.0).abs() < 0.01);
    }

    #[test]
    fn viewport_corner_ignores_anchor_bounds() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::BottomTrailing,
            Vec2::new(0.0, 0.0),
            Size::new(100.0, 100.0),
        );
        // Even with an absurd anchor location, ViewportCorner only uses viewport.
        mgr.position_overlays(
            |_| Some(Rect::new(123.0, 456.0, 7.0, 8.0)),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert_eq!((b.x, b.y), (700.0, 500.0));
    }

    #[test]
    fn near_anchor_horizontal_is_direction_aware() {
        // NearAnchor (used by tooltips): LTR aligns the content's leading
        // (left) edge to the anchor's left edge; RTL mirrors it, aligning
        // the content's trailing (right) edge to the anchor's right edge.
        // Anchor x=600, w=100 (right edge 700); content w=200; offset 0.
        // Viewport 800×600 — wide enough that the clamp doesn't bite.
        let anchor = Rect::new(600.0, 100.0, 100.0, 20.0);
        let resolved_x = |dir: LayoutDirection| {
            let mut mgr = OverlayManager::new();
            let id = mgr.show(OverlayRequest {
                content_id: fake_id(10),
                anchor: fake_id(1),
                placement: OverlayPlacement::NearAnchor {
                    offset: Vec2::new(0.0, 8.0),
                },
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            });
            mgr.set_content_bounds(id, Size::new(200.0, 50.0));
            mgr.position_overlays(|_| Some(anchor), (800.0, 600.0), dir);
            overlay_bounds(&mgr, id).x
        };
        // LTR: anchor.x + offset.x = 600.
        assert!(
            (resolved_x(LayoutDirection::LeftToRight) - 600.0).abs() < 0.01,
            "LTR x = {}",
            resolved_x(LayoutDirection::LeftToRight)
        );
        // RTL: anchor.x + anchor.width - content.w - offset.x = 500.
        assert!(
            (resolved_x(LayoutDirection::RightToLeft) - 500.0).abs() < 0.01,
            "RTL x = {}",
            resolved_x(LayoutDirection::RightToLeft)
        );
    }

    // --- Auto-dismiss pause / resume ---

    #[test]
    fn pause_auto_dismiss_removes_overlay_from_deadline_set() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show_for(
            OverlayRequest {
                content_id: fake_id(10),
                anchor: fake_id(1),
                placement: OverlayPlacement::Centered,
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            },
            Duration::from_secs(10),
        );
        assert!(mgr.next_auto_dismiss_deadline().is_some());
        assert!(!mgr.is_auto_dismiss_paused(id));

        mgr.pause_auto_dismiss(id);
        assert!(mgr.is_auto_dismiss_paused(id));
        assert!(
            mgr.next_auto_dismiss_deadline().is_none(),
            "paused overlay must drop out of the deadline-min query"
        );

        mgr.resume_auto_dismiss(id);
        assert!(!mgr.is_auto_dismiss_paused(id));
        assert!(mgr.next_auto_dismiss_deadline().is_some());
    }

    #[test]
    fn pause_then_resume_restores_remaining_time() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show_for(
            OverlayRequest {
                content_id: fake_id(11),
                anchor: fake_id(1),
                placement: OverlayPlacement::Centered,
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            },
            Duration::from_secs(10),
        );

        mgr.pause_auto_dismiss(id);
        // Sleep equivalent: rely on the fact pausing right after show
        // captures ~10s remaining (elapsed is ~0).
        let overlay = mgr.stack.iter().find(|o| o.id == id).unwrap();
        let remaining = overlay.paused_remaining.unwrap();
        assert!(
            remaining >= Duration::from_secs(9),
            "remaining should be near the original 10s, got {remaining:?}"
        );
        assert!(remaining <= Duration::from_secs(10));

        mgr.resume_auto_dismiss(id);
        let overlay = mgr.stack.iter().find(|o| o.id == id).unwrap();
        // After resume, auto_dismiss_after equals the previously-stashed
        // remaining, and shown_at_real has been refreshed so the new
        // deadline starts from "now + remaining".
        assert_eq!(overlay.auto_dismiss_after, Some(remaining));
        assert!(overlay.paused_remaining.is_none());
    }

    #[test]
    fn pause_is_idempotent() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show_for(
            OverlayRequest {
                content_id: fake_id(12),
                anchor: fake_id(1),
                placement: OverlayPlacement::Centered,
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            },
            Duration::from_secs(10),
        );
        mgr.pause_auto_dismiss(id);
        let first_remaining = mgr.stack[0].paused_remaining;
        mgr.pause_auto_dismiss(id); // second pause must not overwrite
        let second_remaining = mgr.stack[0].paused_remaining;
        assert_eq!(
            first_remaining, second_remaining,
            "double-pause must preserve the original stashed remaining"
        );
    }

    #[test]
    fn resume_on_unpaused_is_noop() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show_for(
            OverlayRequest {
                content_id: fake_id(13),
                anchor: fake_id(1),
                placement: OverlayPlacement::Centered,
                dismiss: DismissBehavior::Manual,
                layer: OverlayLayer::InTree,
                parent_overlay: None,
                on_dismiss: None,
                fade_duration: None,
            },
            Duration::from_secs(10),
        );
        let before = mgr.stack[0].auto_dismiss_after;
        mgr.resume_auto_dismiss(id); // never paused
        let after = mgr.stack[0].auto_dismiss_after;
        assert_eq!(before, after);
    }

    #[test]
    fn pause_on_persistent_overlay_is_noop() {
        let mut mgr = OverlayManager::new();
        let id = mgr.show(OverlayRequest {
            content_id: fake_id(14),
            anchor: fake_id(1),
            placement: OverlayPlacement::Centered,
            dismiss: DismissBehavior::Manual,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss: None,
            fade_duration: None,
        });
        // No auto_dismiss_after — pause should be a no-op.
        mgr.pause_auto_dismiss(id);
        assert!(!mgr.is_auto_dismiss_paused(id));
        assert!(mgr.stack[0].paused_remaining.is_none());
    }

    #[test]
    fn pause_on_unknown_id_is_noop() {
        let mut mgr = OverlayManager::new();
        mgr.pause_auto_dismiss(OverlayId::new(9999)); // must not panic
        mgr.resume_auto_dismiss(OverlayId::new(9999));
    }

    #[test]
    fn viewport_corner_zero_margin_snaps_to_edge() {
        let mut mgr = OverlayManager::new();
        let id = show_corner_overlay(
            &mut mgr,
            Corner::TopLeading,
            Vec2::ZERO,
            Size::new(50.0, 50.0),
        );
        mgr.position_overlays(
            |_| Some(Rect::ZERO),
            (800.0, 600.0),
            LayoutDirection::LeftToRight,
        );
        let b = overlay_bounds(&mgr, id);
        assert_eq!((b.x, b.y), (0.0, 0.0));
    }
}
