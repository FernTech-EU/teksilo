// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::cell::RefCell;
use std::rc::Rc;

use crate::styles::Theme;
use teksilo_canvas::{Canvas, Point, Rect, RenderFrame, SizeProposal};

use crate::arena::WidgetArena;
use crate::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use crate::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

mod accessibility_description_impl;
#[cfg(test)]
mod accessibility_description_tests;
mod accessibility_emit_impl;
mod accessibility_impl;
#[cfg(test)]
mod accessibility_proxy_tests;
#[cfg(test)]
mod adapter_ids_tests;
#[cfg(test)]
mod announcement_ring_tests;
#[cfg(test)]
mod announcer_tests;
mod drag_drop_impl;
mod focus_impl;
mod gesture_dispatch_impl;
#[cfg(test)]
mod hit_targeting_tests;
mod layout_impl;
mod overlay_impl;
pub mod pan_arbiter;
mod pointer_cancel;
mod pointer_router;
mod pointer_state;
mod query_impl;
mod rendering_impl;
mod test_api;
pub mod touch_route;
#[cfg(test)]
mod window_name_tests;

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

    /// Non-consuming counterpart to [`take_pending_animation`](Self::take_pending_animation),
    /// for [`WidgetTree::needs_reconcile`] — which must be able to ask
    /// "is there work here?" without doing any.
    fn has_pending_animation(&self) -> bool {
        self.weak
            .upgrade()
            .is_some_and(|signal| signal.has_pending_animation())
    }
}

// (line 33, above `struct AnimatedRegistration`) — remove entirely
// (line 71, above `#[allow(clippy::type_complexity)]` / `pub struct WidgetTree`)
/// The main widget tree orchestrating arena, layout, events, accessibility, and paint.
/// Provides both the runtime API and the headless test API.
#[allow(clippy::type_complexity)]
pub struct WidgetTree {
    pub(crate) arena: WidgetArena,
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
    /// How the active [`TargetDensity`](teksilo_tokens::TargetDensity) is chosen. `Fixed(Compact)` by default,
    /// so nothing switches density unless the app asks.
    ///
    /// `FollowLastPointer` is **stored and read by nobody**. The pointer
    /// ingress it was written for exists — the app event loop routes contacts
    /// into `dispatch_pointer_with_ops` — but honouring the policy means
    /// deciding what a stray tap costs (a density switch discards every widget
    /// id in the tree), whether a pen counts as coarse, and how the hysteresis
    /// commits when the user simply stops touching. None of that is settled, so
    /// the policy is a declaration the framework does not yet act on.
    density_policy: teksilo_tokens::DensityPolicy,
    /// User-controlled global text-scale factor (`1.0` = 100 %). Layered on top
    /// of the OS `text_scale_factor`: the two multiply. Set via
    /// `set_user_text_scale`; persisted by the application through
    /// `teksilo_settings::TEXT_SCALE_KEY`.
    user_text_scale: f32,
    /// Cached projection of `theme` whose `typography` is scaled by
    /// `user_text_scale * text_scale_factor`. Recomputed by
    /// `recompute_effective_theme` whenever the theme or either scale factor
    /// changes; the layout and paint walkers read this instead of `theme` so
    /// all text grows uniformly. Equal to `theme` when the combined factor is 1.
    effective_theme: Theme,
    /// The combined `user_text_scale * text_scale_factor`, cached so the
    /// layout/paint context construction sites don't recompute it. The single
    /// scalar published to widgets that size from a source *other* than
    /// `Theme.typography` (icons, the rich-text engine, calendar constants,
    /// scene text). Written alongside `effective_theme` in
    /// `recompute_effective_theme`.
    effective_text_scale: f32,
    /// Reactive mirror of `effective_text_scale`, for build-time binders that
    /// must react to a scale change without a rebuild path of their own (e.g.
    /// `Calendar` binds this at `Rebuild` level). Fired by
    /// `recompute_effective_theme`.
    text_scale_signal: crate::signal::Signal<f32>,
    /// Reactive window-active state (`focused AND not occluded`), the
    /// occlusion-aware companion to `WindowState::focused` (which is raw OS
    /// focus only). The single source of truth for "is this window active",
    /// read by `is_window_active()` and published to widgets via
    /// `window_active_signal()` / `BuildContext::window_active*` /
    /// `PaintContext::window_active`. Drives caret hiding, selection
    /// desaturation and `DimWhenInactive`. Starts `true` — winit may not send
    /// `Focused(true)` for the first window, so a window must not be born
    /// inactive. Mutated only by `set_window_active`.
    window_active_signal: crate::signal::Signal<bool>,
    text_backend: Option<Rc<RefCell<dyn teksilo_canvas::TextBackend>>>,
    focused: Option<WidgetId>,
    /// Reactive mirror of `focused`. Same pattern as `hovered_signal`
    /// — kept in sync via `set_focused`. Drives the inspector's Focus
    /// tab without polling.
    focused_signal: crate::signal::Signal<Option<WidgetId>>,
    /// Every pointer the tree currently knows about, plus the primary and
    /// hover-owner elections.
    ///
    /// Replaces the singular `hovered` / `last_pointer_position` /
    /// `pointer_captured_by` this tree used to carry. Hover lives on the
    /// **hover owner**'s entry (a contact never hovers), position and the
    /// legacy singular accessors read the **primary**, and capture is per
    /// pointer — two contacts hold independent captures, each released only by
    /// its own Up or Cancel. See [`crate::pointer::table`].
    pointers: crate::pointer::table::PointerTable,
    /// Reactive mirror of the hover owner's hovered widget. Set whenever it
    /// changes during dispatch / hit-test recovery so external observers
    /// (notably the debug inspector's hover tooltip) can react without
    /// polling. Held by handle so the field is a cheap clone.
    hovered_signal: crate::signal::Signal<Option<WidgetId>>,
    /// Reactive mirror of the kind of the pointer that most recently produced
    /// a sample. Lets a widget switch an affordance between the mouse and the
    /// touch form without a rebuild, and without every widget having to
    /// remember an `on_pointer_event` of its own just to learn the modality.
    last_pointer_kind_signal: crate::signal::Signal<teksilo_tokens::PointerKind>,
    /// The pointer position *before* the move currently being
    /// dispatched — i.e. the last sample that was still over the
    /// previously-hovered widget. Read when arming an overlay's safe
    /// triangle: the apex belongs at the point the pointer left the
    /// anchor, not at the first sample past it (they coincide for a
    /// real mouse, but the distinction is what keeps the cone from
    /// degenerating to "the apex is wherever I am, so I am always
    /// inside it").
    previous_pointer_position: Option<teksilo_canvas::Point>,
    /// A rebuild destroyed the focused widget: the ancestors it had inside the
    /// subtree that owned focus, innermost first, remembered so the end of the
    /// layout pass can land focus back inside the innermost one still alive.
    ///
    /// A rebuild allocates fresh `WidgetId`s for its children, so the focused
    /// node dies and `revalidate_interaction_state` drops focus to `None`.
    /// Leaving it there kicks the user out of the widget they were in — most
    /// visibly, a popover that refreshes its content when it opens would throw
    /// away the very row the popover had just focused, so the menu comes up with
    /// no keyboard focus at all. Focus is re-entered *after* the layout walk
    /// (see the tail of `layout_with_ops`), once the fresh children have real
    /// bounds for the focus-driven scroll-into-view — the same shape as the
    /// post-layout hover refresh next to it.
    ///
    /// The innermost survivor, not the rebuilt root: a reconciling rebuild
    /// keeps the children it re-attaches, so the group that held focus is often
    /// still there, and the root's first focusable descendant is the *first*
    /// group, whichever one the user was in.
    pending_focus_restore: Vec<WidgetId>,
    /// The interaction anchors as of the last layout walk — the focused node,
    /// each live pointer's captor, and an in-flight drag's source.
    ///
    /// A `culls_children` parent is told to keep what the user is in the
    /// middle of, and it is only asked during layout. So the anchor set is an
    /// *input* to that decision, and when it changes the decision is stale —
    /// even though nothing moved and nothing resized, which is what the
    /// idle-pass early-return keys off. Kept here so the pass can notice the
    /// change and re-ask the culling parents it affects; see
    /// `invalidate_culls_for_moved_interaction`.
    last_interaction_anchors: Vec<WidgetId>,
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
    /// Window-global actions registered via
    /// [`BuildContext::register_action_global`](crate::BuildContext::register_action_global).
    /// Consulted as a fallback at the end of every intent dispatch — *after* the
    /// source→root walk finds no consuming node action — so an app-global command
    /// is reachable no matter where the intent originated (a menu-bar dropdown
    /// overlay, deep content, a global shortcut anchored at the root). Each entry
    /// is owned by the registering widget and torn down on its rebuild/destroy,
    /// mirroring `register_shortcut_global`.
    global_actions: Vec<(WidgetId, crate::action::Action)>,
    /// The widgets that edit text, registered via
    /// [`BuildContext::register_text_surface`](crate::BuildContext::register_text_surface).
    ///
    /// Owned by the registering widget and torn down on its rebuild/destroy,
    /// exactly like `global_actions` above. Read through
    /// [`WidgetTree::focused_text_surface`] by a host that has taken a text
    /// chord — `Ctrl+Z`, `Ctrl+C` — for itself and owes every text widget in the
    /// tree an answer about what happens to it. See
    /// [`crate::text_surface`] for why the framework is the only place that
    /// question can be answered completely.
    text_surfaces: crate::text_surface::TextSurfaces,
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
    ///
    /// Its initial value is the tree's **epoch**, and [`Self::input_clock`] is
    /// seeded from the very same `Instant` — so `EventTime::ZERO` and this
    /// field's starting value name one moment, and the input timeline and the
    /// animation timeline are one axis. See
    /// [`crate::pointer::clock`] for why that matters.
    sim_clock: std::time::Instant,
    /// The one source of [`EventTime`](crate::pointer::EventTime)s for this
    /// tree. A [`MonotonicClock`](crate::pointer::clock::MonotonicClock)
    /// anchored at the tree epoch by default; a test swaps in a
    /// [`ManualClock`](crate::pointer::clock::ManualClock) via
    /// [`set_input_clock`](Self::set_input_clock).
    input_clock: std::rc::Rc<dyn crate::pointer::clock::InputClock>,
    /// What is known about the sample currently being dispatched, snapshotted
    /// onto every [`EventContext`] built while it
    /// runs. Holds its default — a mouse at the epoch — outside a pointer or
    /// scroll dispatch.
    current_input: crate::pointer::InputSnapshot,
    /// Whether simulated time is *currently* what this tree measures against —
    /// i.e. whether [`Self::sim_clock`], rather than the wall clock, is what
    /// [`Self::animation_clock`] answers with and what freezes the input
    /// timeline.
    ///
    /// Set by `enter_simulated_mode`, which
    /// [`advance_time`](Self::advance_time) calls before it moves anything, and
    /// cleared by [`resume_real_time`](Self::resume_real_time). Both axes turn
    /// on this one flag, so the tree has one notion of "is time simulated right
    /// now" rather than two that can disagree.
    ///
    /// It is not a latch. A headless test never hands the clock back, so for a
    /// test it behaves like one: an animation may then only progress by
    /// advancing the clock, never by the test taking a long time. A host
    /// sharing a live tree with a real event loop — the debug automation bridge
    /// — hands it back after every operation, and real time drives the tree
    /// again until the next advance.
    ///
    /// What each axis does across the two transitions differs, because the two
    /// carry different state. The animation scheduler holds absolute instants,
    /// so switching axes **rebases** them (`AnimationScheduler::rebase`) and the
    /// reading itself is simply whichever clock is in charge. The input axis
    /// holds none, so it is the *reading* that is carried: see
    /// [`Self::sim_input_origin`] and [`Self::sim_input_offset`].
    sim_time_frozen: bool,
    /// Where the input timeline stood when this tree entered simulated mode,
    /// as `(the reading then, the `sim_clock` then)`.
    ///
    /// The input axis cannot simply become `sim_clock - epoch`: samples
    /// dispatched *before* the switch were stamped from the wall clock, which
    /// by then is ahead of `sim_clock`, so every one of them would sit in the
    /// virtual future and no interval measured from them would ever elapse.
    /// Continuing the axis from where it stood instead makes every stamp taken
    /// before the switch lie in the past and every interval after it exactly
    /// the duration advanced.
    ///
    /// `None` while the tree runs on real time, and also under a clock that has
    /// no wall-clock anchor — a
    /// [`ManualClock`](crate::pointer::clock::ManualClock) *is* the virtual
    /// axis already, and is moved directly by `advance_time`.
    ///
    /// Cleared by [`resume_real_time`](Self::resume_real_time), which hands the
    /// axis back to the wall clock with what was advanced carried forward in
    /// [`Self::sim_input_offset`].
    sim_input_origin: Option<(crate::pointer::EventTime, std::time::Instant)>,
    /// How far ahead of the raw input clock this tree's input timeline runs,
    /// having been advanced and then handed back to real time.
    ///
    /// Added to every reading of the clock, and **re-measured** (not
    /// accumulated) at each hand-back as `frozen reading − raw reading`, floored
    /// at zero. That is what makes the axis monotone across the hand-back: the
    /// reading at the instant of
    /// [`resume_real_time`](Self::resume_real_time) is the later of the frozen
    /// reading it had and the raw one, and it moves with the wall clock from
    /// there. Without it
    /// the axis would jump *backwards* by everything that was advanced, and a
    /// monotone [`EventTime`](crate::pointer::EventTime) is a platform
    /// conformance invariant.
    ///
    /// So it is not monotone in itself: a tree that spent longer on the wall
    /// clock than it was ever advanced is already ahead of its frozen reading
    /// and the right offset is then zero, which is what the floor is for. It is
    /// also dropped outright by
    /// [`set_input_clock`](Self::set_input_clock) — it is a distance measured
    /// against one clock's readings and means nothing against another's.
    ///
    /// It is also what keeps a deadline schedulable: `instant_for` subtracts it
    /// again, so a long press armed after an advance is reported to the event
    /// loop at a *future* `Instant` rather than one in the past that can never
    /// ripen.
    sim_input_offset: std::time::Duration,
    /// Overlay manager for tooltips, menus, popovers.
    pub(crate) overlay_manager: crate::overlay::OverlayManager,
    /// Re-entrancy guard for the dismissal-callback drain. A callback may
    /// dismiss another overlay, whose teardown drains again; the inner drain
    /// returns at once and the outer loop picks up whatever it parked, so the
    /// recursion is one level deep by construction rather than by luck.
    pub(crate) draining_dismiss: std::cell::Cell<bool>,
    /// Tooltip attachments, one per anchor. See [`TooltipEntry`].
    tooltips: Vec<TooltipEntry>,
    /// Simulated-clock end of the tooltip "reshow session". While any tip is
    /// visible, or until this instant after the last tip dismissed, subsequent
    /// anchors use `MotionTokens::tooltip_reshow_delay` instead of the full
    /// initial delay (Windows `TTDT_RESHOW` behaviour).
    tooltip_session_until_sim: Option<std::time::Instant>,
    /// Real-clock counterpart of [`Self::tooltip_session_until_sim`].
    tooltip_session_until_real: Option<std::time::Instant>,
    /// The tooltip currently surfaced by keyboard menu navigation
    /// (`show_highlight_tooltip`): `(overlay_id, content_id)`. At most one
    /// is shown at a time; moving the highlight or closing the menu clears
    /// it. Distinct from the hover/focus tooltip paths — this one is a
    /// `Manual`-dismiss child of the menu overlay so a single Escape closes
    /// the menu (and cascades the tooltip) rather than only the tooltip.
    highlight_tooltip: Option<(crate::overlay::OverlayId, WidgetId)>,
    /// How the currently focused widget gained focus.
    focus_origin: Option<crate::focus::FocusOrigin>,
    /// Every live pointer press, keyed by the node whose gesture arena took it.
    /// The framework's press visual — see [`crate::press`] for the four things
    /// a widget's own `PointerDown`/`PointerUp` bookkeeping cannot see.
    presses: crate::press::PressTable,
    /// Input-modality "focus-visible" state: `true` after keyboard input,
    /// `false` after pointer input. Focus rings (e.g. `StandardItem`'s current
    /// row) show only while this is `true`, the standard `:focus-visible`
    /// behaviour — so a mouse click selects without a ring, and keyboard
    /// navigation reveals it.
    focus_visible: crate::signal::Signal<bool>,
    /// Active focus-scope stack during build. A data view pushes its scope
    /// (`begin_view_focus`) around its row loop so each row reads *its view's*
    /// focus deterministically — independent of arena parenting, which may not
    /// be wired yet while rows build (docked / virtualized content). Drives
    /// focus-aware selection + focus rings in `StandardItem`.
    view_focus_stack: Vec<crate::signal::Signal<bool>>,
    /// Layout direction for RTL/LTR support.
    pub(crate) layout_direction: crate::environment::LayoutDirection,
    /// Animation scheduler for smooth animated state and signal transitions.
    animation_scheduler: crate::animation::AnimationScheduler,
    /// Weakly tracked animated values from both state and signal APIs.
    animated_values: Vec<AnimatedRegistration>,
    /// Registry of shader-driven animated quads (opt-in alternative to
    /// `Signal<f32>::animate_looping` for decorative motion — progress
    /// sweeps, sprite-atlas frame cycling, future pulse/shimmer). The
    /// scheduler-style signal path stays for everything else. Per-slot
    /// `AnimParams` are ticked and attached to every `RenderFrame`
    /// produced by `render()` — the renderer reads them from there.
    animated_quads: crate::animated_quad::AnimatedQuadRegistry,
    /// Per-frame-effect scheduler. Owns the registry of widgets that
    /// asked for a frame-tick subscription (Pulse, Cycle, …). Sits
    /// alongside `animation_scheduler` and `animated_quads` as the
    /// third visibility-aware motion source — they all consult the
    /// same [`motion_visibility`](crate::motion_visibility) helpers.
    /// After every `render()` the tree calls
    /// `FrameTickScheduler::should_arm_frame_tick` and re-arms
    /// `frame_tick_armed_by_subscribers` if any subscriber's owner was
    /// painted this frame.
    pub(crate) frame_tick_scheduler: crate::frame_tick_scheduler::FrameTickScheduler,
    /// Live pans, live coasts, the window's pinch, and the palm watches — the
    /// whole touch-motion layer, in one field. See
    /// [`pan_arbiter`].
    touch_motion: pan_arbiter::TouchMotion,
    /// The claimant chain the next synthesised scroll is to walk.
    ///
    /// Armed immediately before that scroll is pushed through
    /// [`dispatch_scroll`](Self::dispatch_scroll) and taken by the router arm
    /// that routes it, because the two producers know different things: a pan
    /// has a live session to read, a coast has only the chain it was launched
    /// with. `None` for every scroll from a backend, which derives its own
    /// chain from the sample's position.
    armed_chain: Option<Vec<(WidgetId, crate::pointer::touch_action::PanClaim)>>,
    /// Monotonic counter bumped at the start of each `render()` call.
    /// Each widget's `last_painted_epoch` is set to this value whenever
    /// the paint pass (or the cache-hit early-out) confirms the widget
    /// intersects the window viewport. The animation scheduler uses it
    /// to detect and pause animations for widgets that have scrolled
    /// off-screen. Starts at `0`, which serves as the "never painted"
    /// sentinel; tests that only call `layout()` see the gate bypass.
    paint_epoch: u64,
    /// Cached accessibility tree update, rebuilt only when something that
    /// changes the AT tree has happened, not on every layout.
    cached_a11y: Option<accesskit::TreeUpdate>,
    /// Whether the accessibility tree needs rebuilding. Set by focus moves,
    /// overlay changes, widget rebuilds, active↔dormant transitions,
    /// `AccessibilityOnly` binding flips and `request_accessibility_update()`;
    /// a plain relayout does not set it.
    a11y_dirty: bool,
    /// Snapshot of `shortcut_registry.version()` at the last
    /// `sync_accessibility` call. When the live version differs the
    /// AT cache is dirtied, so widgets that bound their announced
    /// shortcut via `access_shortcut_id(id)` track user rebinds
    /// without any explicit signaling from the settings UI.
    last_synced_shortcut_version: u64,
    /// Snapshot of `locale_signal` at the last `sync_accessibility`
    /// call. When the locale differs the AT cache is dirtied, so
    /// `access_label(tr!(...))` (stored as a locale-bound
    /// `Prop<String>`) re-resolves into the announced node — even on a
    /// same-direction switch that doesn't rebuild the composite.
    last_synced_locale: Option<String>,
    /// The window title the last accessibility walk named the root after.
    /// A new title re-walks the tree, as a locale switch does, so the
    /// window's name follows it. See `accessibility_emit_impl`.
    last_synced_window_title: Option<String>,
    /// Reverse map from synthetic (widget-emitted) AccessKit NodeIds
    /// to the WidgetId that owns them. Rebuilt on every full
    /// accessibility walk. `handle_accessibility_actions` uses this
    /// to route an `ActionRequest` targeting a TextRun child back
    /// to the owning rich-text editor, since synthetic NodeIds
    /// can't be decoded back to a WidgetId by value alone.
    pub(crate) synthetic_parent_map: std::collections::HashMap<accesskit::NodeId, WidgetId>,
    /// Where each synthetic child sits inside its owner, for the ones that
    /// declared their bounds in the owner's own space (a label's line
    /// boxes, an editor's rows). A pure translation re-places them from
    /// here; a synthetic node absent from this map — a scene item under a
    /// view transform, which reports absolute rects — is moved by the
    /// owner's delta instead.
    pub(crate) synthetic_local_bounds:
        std::collections::HashMap<accesskit::NodeId, teksilo_canvas::Rect>,
    /// Monotonic count of accessibility walks. A consumer that has not
    /// seen the latest one is holding a tree whose *shape* may be stale,
    /// not merely its geometry.
    pub(crate) a11y_walk_generation: u64,
    /// Cached full render frame — reused when no widget needs painting.
    /// `Rc<RenderFrame>` rather than `RenderFrame` so cache-hit frames
    /// cost an atomic refcount bump instead of a deep clone of every
    /// draw-command Vec. `render()` uses `Rc::make_mut` to update
    /// `anim_params` in place when the tree is the sole owner (the
    /// common case — the caller usually drops the previous frame
    /// before calling render() again).
    cached_frame: Option<std::rc::Rc<RenderFrame>>,
    /// Dispatches that arrived while another dispatch was in flight, replayed
    /// once the outer one completes.
    ///
    /// A handler that dispatches (a synthetic click, an AT action re-entering
    /// the door) must not observe half-updated pointer state, and must not be
    /// able to unwind the sample the outer dispatch is still standing on. So a
    /// nested dispatch is queued here rather than run inline, and drained —
    /// faithfully, event *and* input snapshot — after the outer sample
    /// completes. From the caller's side nothing changes: the queue is empty
    /// again before the top-level `dispatch_*` call returns.
    pending_dispatch: std::collections::VecDeque<pointer_router::QueuedDispatch>,
    /// How many dispatches are on the stack. Non-zero means "queue, do not
    /// re-enter"; see [`Self::pending_dispatch`].
    dispatch_depth: u32,
    /// Pointers whose current press was cancelled and which have not pressed
    /// again since.
    ///
    /// `PointerCancel` is terminal: the interaction was taken away, so an `Up`
    /// arriving for the same press afterwards must not complete it. A platform
    /// can genuinely send both (Windows delivers a `WM_POINTERUP` after a
    /// capture loss), so the `Up` is swallowed rather than asserted against.
    /// An entry is dropped by that swallowed `Up`, or by the pointer's next
    /// press — a fresh press is a fresh interaction. Bounded by the contact
    /// cap plus the mouse.
    cancelled_pointers: Vec<crate::pointer::PointerId>,
    /// Current cursor selected by hover/interaction routing.
    current_cursor: crate::widget::CursorIcon,
    /// What the **node-declared** cursor mechanism last resolved for the
    /// hovered chain — the value `PointerEnter` wrote, or `Default` after the
    /// `PointerLeave` that undid it.
    ///
    /// Kept so a handler can hand the cursor back
    /// ([`EventContext::release_cursor`]) after having overridden it, without
    /// having to re-derive the chain's declaration and risk disagreeing with
    /// the walk that produced it. It is exactly as fresh as `current_cursor`:
    /// both move only when the enter/leave pair fires, so a node whose
    /// `.cursor(..)` changes under a stationary pointer is stale in the same
    /// way, and by the same mechanism.
    ///
    /// [`EventContext::release_cursor`]: crate::widget::EventContext::release_cursor
    node_declared_cursor: crate::widget::CursorIcon,
    /// Delayed overlay requests (e.g., submenu hover-open delay).
    pending_delayed_overlays: Vec<PendingDelayedOverlay>,
    /// Reusable scratch buffer for active-id snapshots taken on hot
    /// paths that mutate per-widget state inside the loop
    /// (`tick_gestures_with_ops`, post-render dirty-bit clear,
    /// post-layout `needs_layout` clear). Cleared and refilled on
    /// every use via `WidgetArena::fill_active_ids`. Previously these
    /// sites called the allocating `arena.active_ids()` per frame,
    /// which `perf record` ranked at ~13 % of CPU on the
    /// `widget_catalog --tab animations` scene.
    active_ids_scratch: Vec<WidgetId>,
    /// Widgets currently carrying a non-`None` `EventHandlers::gesture_arena`.
    /// Updated on attach (`ensure_gesture_arena` install) and on
    /// teardown (rebuild / destroy / handler-clear). Every per-frame
    /// gesture pass (`tick_gestures_with_ops`, `next_gesture_deadline`)
    /// iterates this set instead of every active widget — most active
    /// widgets have `gesture_arena = None`, so the savings come from
    /// not even visiting them. Filtered by `arena.is_active(id)` at
    /// iteration time so dormant entries don't fire (a widget can be
    /// dormant while still holding its handlers).
    gesture_owners: std::collections::HashSet<WidgetId>,
    /// OS-level accessibility preferences (high contrast, reduced motion, text scale).
    prefers_high_contrast: bool,
    prefers_reduced_motion: bool,
    text_scale_factor: f64,
    /// Whether an assistive technology is reading the tree, as reported by the
    /// *operating system* — not by AccessKit. See
    /// [`Self::set_screen_reader_state`] for why the distinction matters.
    screen_reader: crate::environment::ScreenReaderState,
    /// The app's explore-by-touch policy. See [`Self::set_explore_by_touch`].
    explore_by_touch: crate::environment::ExploreByTouch,
    /// Whether an AccessKit client is currently attached to this window's
    /// adapter. Written by `teksilo-app` from the platform adapter's
    /// activation / deactivation handlers.
    at_client_attached: bool,
    /// How a density switch should be worded to a screen reader, if the
    /// application wants one announced. See
    /// [`Self::set_density_announcement`].
    density_announcement: Option<std::rc::Rc<dyn Fn(teksilo_tokens::TargetDensity) -> String>>,
    /// How "a context menu opened" should be worded to a screen reader when a
    /// **hold** opened it, if the application wants one announced. See
    /// [`Self::set_context_menu_announcement`].
    context_menu_announcement: Option<std::rc::Rc<dyn Fn() -> String>>,
    /// The tree-owned long press the router is waiting out, if any. One at a
    /// time: two fingers holding two different controls is not a gesture any
    /// desktop idiom assigns a meaning to, and the second contact's arrival
    /// replaces the first's route rather than racing it. See
    /// [`touch_route`].
    pending_touch_route: Option<touch_route::PendingTouchRoute>,
    /// Host window HiDPI device scale (physical px per logical px), fed by
    /// `teksilo-app` before each layout. Surfaced to widgets via
    /// `LayoutContext::scale_factor`. The widget tree is otherwise fully
    /// logical (the renderer applies this scale at the vertex stage); this is
    /// the escape hatch for widgets that must size a device-pixel OS resource
    /// (e.g. a `WebView`'s native subview). 1.0 in headless / test contexts.
    device_scale_factor: f32,
    /// Platform safe-area insets for the host window — a notch, a rounded
    /// corner, a home indicator — in logical pixels, fed by `teksilo-app`
    /// after every window resize. Reaches overlay placement through
    /// [`OverlayViewport::safe_area`](crate::overlay::OverlayViewport::safe_area).
    /// `ZERO` on every platform that reports nothing, which is every desktop
    /// platform but macOS.
    safe_area: teksilo_canvas::EdgeInsets,
    /// A rectangle of the window currently covered by something outside the
    /// tree — a soft keyboard, a platform IME candidate window — in
    /// window-logical pixels. Reaches overlay placement through
    /// [`OverlayViewport::occluded`](crate::overlay::OverlayViewport::occluded).
    /// `None` on every frame where nothing is covering the window.
    occluded_inset: Option<Rect>,
    /// A pending `EventContext::request_soft_keyboard` call, taken by the app
    /// layer once per dispatch. `Some(true)` asks for the keyboard,
    /// `Some(false)` asks it to go away.
    soft_keyboard_request: Option<bool>,
    /// Active drag-and-drop session, if any.
    pub(crate) active_drag: Option<crate::drag_state::DragSession>,
    /// Source widget of an in-flight OS (outbound) drag that escalated past
    /// the window boundary. Set only on the window that *started* the drag.
    /// The in-app `active_drag` session is torn down at escalation (the OS owns
    /// the pointer); this remembers who started it so the eventual `DragEnded`
    /// can fire the source's `on_drag_ended`.
    pub(crate) outbound_drag_source: Option<WidgetId>,
    /// True while *this* window currently holds the re-entered internal session
    /// for an in-flight app-originated OS drag (the OS drag wandered back over
    /// this window — possibly a different window than the source — and we
    /// restored the original typed payload). Distinguishes that session from a
    /// plain internal drag so leaving again re-stashes instead of starting a
    /// second OS drag, and dropping doesn't double-fire `on_drag_ended`.
    pub(crate) os_drag_reentered: bool,
    /// The accept state last pushed to the platform for an inbound OS drag, so
    /// the same answer is not re-sent on every motion sample.
    ///
    /// `None` outside an OS drag, and reset when one ends — a fresh drag must
    /// push its first verdict even if it happens to match the last drag's.
    pub(crate) os_drop_accepted: Option<bool>,
    /// Optional platform host for custom window chrome (set when the
    /// application opts in via `WindowConfig::custom_chrome(true)`). Stored
    /// here so that the root-builder closure has access during widget
    /// construction; the same `Rc` is also held by `WindowManager` so it
    /// outlives the widget tree if needed.
    title_bar_host: Option<Rc<dyn crate::PlatformTitleBarHost>>,
    /// App-level subscription state: registered event source adapter,
    /// proxy poster, UI-side subscription callbacks. Default is empty;
    /// teksilo-app installs a populated context when an event source is
    /// registered on the builder.
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
    /// a frame was asked for**: through `request_frame()`, or by the
    /// render-end re-arm for a painted frame-tick subscriber. This
    /// preserves Teksilo's draw-when-needed model: idle trees stay idle
    /// even if widgets have registered observers on this signal.
    pub(crate) frame_tick: crate::signal::Signal<f32>,
    /// Set by `request_frame()`; consumed by `advance_frame_tick()` on
    /// the next `layout()`. Observers that need another tick after the
    /// current one must re-request. Stored as `Rc<Cell>` so observers
    /// fired from inside the layout pass (`ctx.effect` closures on
    /// `frame_tick`) can chain-request without needing &mut access
    /// to the tree — see [`frame_request_handle`](Self::frame_request_handle).
    pub(crate) frame_tick_requested: std::rc::Rc<std::cell::Cell<bool>>,
    /// Set after a render in which a frame-tick subscriber's owner was
    /// painted; consumed alongside `frame_tick_requested` by
    /// `advance_frame_tick()`.
    ///
    /// A flag of its own so that [`frame_tick_deadline`](Self::frame_tick_deadline)
    /// knows *why* a frame is wanted. A throttled subscriber may stretch the
    /// wait for its own tick to its interval; a frame asked for through
    /// `frame_tick_requested` (the announcer's follow-up syncs, a caret, a
    /// drag auto-scroll) is due at 60 Hz whatever is on screen. When both
    /// reasons shared one flag they could not be told apart, and a
    /// once-a-minute clock anywhere in the window made every requested frame
    /// wait a minute.
    pub(crate) frame_tick_armed_by_subscribers: bool,
    /// Debug-only re-entrancy flag: `true` while a focus-change dispatch
    /// (`FocusGained` / `FocusLost` handlers) is running. Threaded into each
    /// `EventContext` so `open_window` / `focus_window` can warn if a handler
    /// changes context merely because a control gained focus (WCAG 3.2.1). A
    /// shared `Rc<Cell<bool>>` (like `frame_tick_requested`) so the flag is
    /// readable from an `EventContext` that holds no `&mut` to the tree.
    pub(crate) in_focus_dispatch: std::rc::Rc<std::cell::Cell<bool>>,
    /// Shared "accessibility re-walk requested" flag. Set via
    /// [`request_accessibility_update`](Self::request_accessibility_update)
    /// (or its `BuildContext` / `EventContext` wrappers) and drained at the
    /// top of [`sync_accessibility`](Self::sync_accessibility) into
    /// `a11y_dirty`. A relayout no longer re-walks the AT tree on its own, so
    /// widgets that restructure their subtree in an AT-affecting way (e.g.
    /// `SceneView` materialising / destroying scene widgets) need this lever.
    /// `Rc<Cell>` so the shared `&self` paths can toggle it like
    /// `frame_tick_requested`.
    pub(crate) a11y_update_requested: std::rc::Rc<std::cell::Cell<bool>>,
    /// Delayed frame wake-up deadline. Widgets that need to schedule
    /// a future frame without pumping at full framerate (caret blink,
    /// etc.) store the target instant here via
    /// [`wake_at_handle`](Self::wake_at_handle). `next_timer_deadline`
    /// rolls it into the event loop's WaitUntil; when reached, the
    /// next `layout()` automatically re-arms `frame_tick_requested`
    /// so the frame-tick effects run on the wake-up pass.
    pub(crate) pending_wake_at: std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>>,
    /// One-shot post-mount actions enqueued during `build()` via
    /// [`BuildContext::run_after_mount`](crate::BuildContext::run_after_mount), drained by the app loop (and by
    /// tests) through [`WidgetTree::run_mount_actions`] with a real
    /// [`EventContext`] — the only place a widget
    /// can read the OS parent handle / app-state / poster together after it is
    /// mounted. Used by widgets that own a native resource needing a window
    /// handle to initialise (a `WebView`'s engine subview).
    pub(crate) pending_mount_actions: Vec<Box<dyn FnOnce(&mut crate::widget::EventContext)>>,
    /// Wall-clock time of the previous `layout()` call (for delta computation).
    pub(crate) last_frame_time: Option<std::time::Instant>,
    /// Set by [`EventContext::close_window`] during dispatch; drained
    /// by the application event loop after each event via
    /// [`WidgetTree::take_close_window_request`]. A *guarded* close
    /// request — the app routes it through the window's close guard.
    pub(crate) close_window_requested: bool,
    /// Set by [`EventContext::close_window_forced`] during dispatch;
    /// drained via [`WidgetTree::take_force_close_request`]. An
    /// *unconditional* close request that bypasses the window's close
    /// guard.
    pub(crate) force_close_requested: bool,
    /// Raised by [`EventContext::set_locale`] during dispatch; drained by
    /// the application event loop (see
    /// `WindowManager::drain_pending_locale_requests`) so the switch can be
    /// routed through the `I18nManager` (active locale + version signal +
    /// RTL direction). `WidgetTree::set_locale` alone would only update the
    /// tree's local locale signal — the i18n thread-local would stay put
    /// and `tr!` lookups would not re-resolve.
    pub(crate) pending_locale_request: Option<String>,
    /// Raised by [`EventContext::set_theme`] during dispatch; drained by
    /// the application event loop (see
    /// `WindowManager::drain_pending_theme_requests`) so the switch is
    /// routed through `WindowManager::set_theme`, which fans the new theme
    /// out to *every* window. Applying via `WidgetTree::set_theme` inline
    /// would only re-theme the originating window — the rest of the app
    /// would stay on the old theme. Mirrors `pending_locale_request`.
    pub(crate) pending_theme_request: Option<crate::styles::Theme>,
    /// Raised by [`EventContext::follow_system_theme`] during dispatch;
    /// drained by the application event loop (see
    /// `WindowManager::drain_pending_follow_system_requests`), which switches
    /// the app to `ThemeMode::Native` and recomputes the theme from current
    /// OS colours. Mirrors `pending_theme_request`.
    pub(crate) pending_follow_system_request: bool,
    /// Raised by [`EventContext::set_text_scale`] during dispatch; drained by
    /// the application event loop (see
    /// `WindowManager::drain_pending_text_scale_requests`) so the change is
    /// routed through `WindowManager::set_text_scale`, fanning the new factor
    /// out to *every* window. Mirrors `pending_theme_request`.
    pub(crate) pending_text_scale_request: Option<f32>,
    /// Monotonic version counter bumped after every *real* accessibility
    /// rebuild in [`Self::sync_accessibility`] (cache hits don't bump).
    /// Mirror of [`crate::shortcut::ShortcutRegistry::version`]: an
    /// automation / test harness can poll it to know whether the AT tree
    /// changed without diffing the whole `TreeUpdate`.
    at_version: crate::signal::Signal<u64>,
    /// The framework's own live regions, one per politeness level. See
    /// [`crate::announcer`]: each speaks every message from a node the tree
    /// has never had, which is the only mechanism all three platform adapters
    /// agree announces, and never in an update that moves focus.
    announcer_polite: crate::announcer::Announcer,
    announcer_assertive: crate::announcer::Announcer,
    /// Where both announcers draw the id of each message's node from, so no
    /// message is spoken from an id the tree has used before.
    announcer_ids: crate::announcer::AnnouncerIds,
    /// The announcements the platform adapters would have made from the
    /// updates `sync_accessibility` returned (see
    /// [`crate::accessibility::Announcement`]), drained by
    /// [`Self::announcements_since`], while the tree records them
    /// ([`Self::set_records_announcements`]).
    announcement_ring: crate::accessibility::announcements::AnnouncementRing,
    /// The ids the window's platform adapter holds, which differ from the
    /// tree's own for a node that left the tree a reader sees and came back.
    /// See [`crate::accessibility::adapter_ids`] and
    /// [`Self::deliver_accessibility`].
    adapter_ids: crate::accessibility::adapter_ids::AdapterIds,
    /// What the last delivered update said about descriptions and live
    /// regions, which is what the next one's descriptions are measured
    /// against. See [`accessibility_description_impl`].
    description_memory: accessibility_description_impl::DescriptionMemory,
    /// Whether the `WidgetEvent::AccessAction` currently being dispatched was
    /// consumed by a handler. Written by the dispatcher's `AccessAction` arm,
    /// read (and reset) by [`Self::dispatch_access_action`], which is the only
    /// caller that can answer "did anything happen?" to its own caller.
    ///
    /// A side channel because `dispatch_event_with_ops` returns `()` for every
    /// event kind, and routing AT actions through the *same* path as everything
    /// else is load-bearing (it is what lets an action open a window). The
    /// alternative — reporting success whenever a live widget merely *existed*
    /// at the target — is how an unhandled action came to look like a
    /// successful one to every automation client.
    access_action_handled: bool,
    /// The `WindowState` for this tree's hosting window. Populated
    /// by the app-level window manager when the tree is registered;
    /// `None` for standalone trees. Cloned into every `EventContext`
    /// and `BuildContext` so widgets can bind to the current window's
    /// signals via `ctx.window()`.
    pub(crate) window_state: Option<crate::window::WindowState>,
}

/// How long the shortened reshow delay stays active after the last tooltip
/// dismisses. Long enough to cover moving between adjacent toolbar icons;
/// short enough that a later, deliberate hover still pays the full initial
/// delay. Not a theme token — it is session bookkeeping, not a visual feel.
const TOOLTIP_SESSION_GRACE: std::time::Duration = std::time::Duration::from_millis(1000);

/// Number of visible steps a sticky-on-dwell tooltip's promotion window is
/// divided into.
///
/// The tree uses this only to decide *how often to wake* while a dwell is
/// running — one redraw per step boundary rather than a free-run — but it must
/// match the step count the content widget actually renders, or the indicator
/// would advance on a different beat from the wake-ups driving it.
/// `teksilo-widgets`' `DWELL_STEPS` is defined *as* this constant, so the two
/// cannot drift; the per-step *duration* is derived from each entry's own
/// `sticky_after`, so a caller that picks a non-default promotion window still
/// gets correctly-spaced wake-ups.
pub const TOOLTIP_DWELL_STEPS: u32 = 4;

/// Max pointer travel (logical px) from the hover-origin before a pending
/// tooltip timer restarts. Mirrors Windows hover-tracking slop
/// (`SPI_GETMOUSEHOVERWIDTH` / height, typically ~4 px): the tip waits for a
/// *paused* pointer, not merely "entered the bounds."
const TOOLTIP_STATIONARY_SLOP: f32 = 4.0;

/// A tooltip attachment managed by the WidgetTree.
struct TooltipEntry {
    anchor_id: WidgetId,
    content_id: WidgetId,
    /// The widget whose accessibility node should carry this tooltip's
    /// description, which is not always the node the overlay hangs off.
    ///
    /// A composing control anchors the *overlay* on an inner chrome node it
    /// built -- the thing with the right bounds to open a tooltip against --
    /// while its role, its name and its focusability live on its own outer
    /// node. A description on the inner one is a description an assistive
    /// technology never reads, because it never lands there.
    ///
    /// Recorded by `BuildContext`'s `attach_tooltip*` wrappers as the widget
    /// that was building at the time. Defaults to `anchor_id`, which is both
    /// the historic behaviour and the right answer for a widget that anchors
    /// its tooltip on itself.
    ///
    /// Naming an owner is a *claim*, not a guarantee: the accessibility walk
    /// honours it only where exactly one tooltip claims that node. See
    /// `WidgetTree::build_accessibility_recursive`.
    description_owner_id: WidgetId,
    delay: std::time::Duration,
    /// Simulated hover start (for deterministic tests via advance_time).
    hover_start: Option<std::time::Instant>,
    /// Real hover start (for windowed apps via layout).
    real_hover_start: Option<std::time::Instant>,
    /// Pointer position when the current pending hover started. Used to
    /// restart the delay if the pointer keeps moving inside the anchor
    /// (stationary-pointer intent filter).
    hover_origin: Option<teksilo_canvas::Point>,
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
    /// True when the tooltip was shown by the keyboard-focus path
    /// rather than the pointer-hover path. Focus-promoted tooltips
    /// dismiss when focus moves outside both the anchor and the
    /// tooltip content subtree (preventing accumulation as the user
    /// Tabs through a form); pointer-dwelled stickies survive
    /// focus changes and only dismiss via Escape or click-outside.
    promoted_by_focus: bool,
    /// Set while this entry's pending delay was started by keyboard focus
    /// rather than by the pointer. Decides, at show time, that the surface
    /// dismisses on `Escape`/click-outside rather than on pointer-leave (there
    /// is no pointer in the story), and that it counts as focus-shown.
    armed_by_focus: bool,
    /// Set while this entry's pending delay was started by a **hold** — the
    /// tree-owned long-press route in [`touch_route`].
    ///
    /// Decides two things at show time that neither the hover nor the focus arm
    /// wants: the surface dismisses on `Escape` / a press outside (a finger has
    /// already lifted, so there is no pointer left to leave), and it carries an
    /// auto-dismiss of [`touch_route::TOUCH_TOOLTIP_DISMISS`] — because with no
    /// pointer to leave and no focus to move, nothing else would ever retire it.
    armed_by_hold: bool,
    /// Set when the tip was dismissed while the focus that summoned it is
    /// still inside its anchor — i.e. Escape on a focus-promoted tooltip.
    ///
    /// Escape restores focus to the anchor, and that restore runs the ordinary
    /// focus path, which ends in `tooltip_focus_enter`. Without this flag the
    /// tip the user just dismissed re-opens on the same keystroke, because
    /// `dormant_dismissed_content` has already cleared `overlay_id` by then and
    /// the entry looks eligible again. Cleared when focus genuinely leaves the
    /// anchor (`tooltip_focus_leave_outside`), so Tabbing away and back
    /// re-summons it normally. The hover path needs no equivalent: a stationary
    /// pointer never re-fires `tooltip_pointer_enter`.
    suppressed_until_focus_leaves: bool,
    /// Where the tooltip opens relative to its anchor. `Below` (default)
    /// for the common case; `Side` for anchors stacked vertically (menu
    /// items, a vertical tab strip, list/tree rows) so the tooltip does
    /// not cover the next sibling. Consulted at show time in both the
    /// hover (`process_tooltips_impl`) and focus (`tooltip_focus_enter`)
    /// paths.
    placement: crate::overlay::TooltipPlacement,
}

/// A delayed overlay request (e.g., submenu hover-open delay).
struct PendingDelayedOverlay {
    request: crate::overlay::OverlayRequest,
    delay: std::time::Duration,
    focus_target: Option<WidgetId>,
    /// Dismiss the anchor's sibling overlays when this one finally
    /// shows, instead of when it was requested. See
    /// [`EventContext::show_overlay_after_replacing_siblings`](crate::widget::EventContext::show_overlay_after_replacing_siblings).
    replace_siblings: bool,
    /// When the request was made (real time, for windowed apps).
    real_requested_at: std::time::Instant,
    /// When the request was made (simulated time, for tests).
    sim_requested_at: std::time::Instant,
}

impl WidgetTree {
    pub fn new() -> Self {
        let initial_theme = crate::presets::intui::light();
        // One signal, shared: the field the tree writes and the registry the
        // application reads must be the same one, or a host mirroring "is the
        // caret in a text widget" would never see focus move.
        let focused_signal = crate::signal::Signal::new(None);
        // ONE epoch. `sim_clock` starts here and the input clock is anchored
        // here, so a single `advance_time` moves gesture deadlines, tooltips,
        // overlays and animations against the same origin.
        let epoch = std::time::Instant::now();
        // ONE scheduler. The fling pump shares the tree's per-frame table
        // rather than making a second one, so a coast wakes the loop through
        // the same path a `Pulse` does.
        let scheduler = crate::frame_tick_scheduler::FrameTickScheduler::new();
        Self {
            arena: WidgetArena::new(),
            theme: initial_theme.clone(),
            theme_signal: crate::signal::Signal::new(initial_theme.clone()),
            density_policy: teksilo_tokens::DensityPolicy::default(),
            user_text_scale: 1.0,
            effective_theme: initial_theme,
            effective_text_scale: 1.0,
            text_scale_signal: crate::signal::Signal::new(1.0),
            // Starts active: winit may not send `Focused(true)` for the first
            // window, so a window must not be born inactive (caret hidden,
            // selection muted) before the first focus event arrives.
            window_active_signal: crate::signal::Signal::new(true),
            text_backend: None,
            focused: None,
            focused_signal: focused_signal.clone(),
            pointers: crate::pointer::table::PointerTable::new(),
            hovered_signal: crate::signal::Signal::new(None),
            last_pointer_kind_signal: crate::signal::Signal::new(
                teksilo_tokens::PointerKind::Mouse,
            ),
            focus_visible: crate::signal::Signal::new(false),
            presses: crate::press::PressTable::default(),
            view_focus_stack: Vec::new(),
            previous_pointer_position: None,
            pending_focus_restore: Vec::new(),
            last_interaction_anchors: Vec::new(),
            last_proposal: SizeProposal::exact(800.0, 600.0),
            pending_modal_requests: Vec::new(),
            pending_modal_dismissal: false,
            shortcut_registry: crate::shortcut::ShortcutRegistry::new(),
            pending_intents: Vec::new(),
            global_actions: Vec::new(),
            text_surfaces: crate::text_surface::TextSurfaces::new(focused_signal.clone()),
            key_capture: None,
            binding_registry: crate::binding::BindingRegistry::new(),
            idle_queue: crate::idle::IdleQueue::new(),
            sim_clock: epoch,
            input_clock: std::rc::Rc::new(crate::pointer::clock::MonotonicClock::new(epoch)),
            current_input: crate::pointer::InputSnapshot::default(),
            sim_time_frozen: false,
            sim_input_origin: None,
            sim_input_offset: std::time::Duration::ZERO,
            focus_origin: None,
            overlay_manager: crate::overlay::OverlayManager::new(),
            draining_dismiss: std::cell::Cell::new(false),
            tooltips: Vec::new(),
            tooltip_session_until_sim: None,
            tooltip_session_until_real: None,
            highlight_tooltip: None,
            layout_direction: crate::environment::LayoutDirection::default(),
            animation_scheduler: crate::animation::AnimationScheduler::new(),
            animated_values: Vec::new(),
            animated_quads: crate::animated_quad::AnimatedQuadRegistry::new(),
            frame_tick_scheduler: scheduler.clone(),
            touch_motion: pan_arbiter::TouchMotion::new(scheduler),
            armed_chain: None,
            paint_epoch: 0,
            cached_a11y: None,
            a11y_dirty: true,
            last_synced_shortcut_version: 0,
            last_synced_locale: None,
            last_synced_window_title: None,
            synthetic_parent_map: std::collections::HashMap::new(),
            synthetic_local_bounds: std::collections::HashMap::new(),
            a11y_walk_generation: 0,
            cached_frame: None,
            pending_dispatch: std::collections::VecDeque::new(),
            cancelled_pointers: Vec::new(),
            dispatch_depth: 0,
            current_cursor: crate::widget::CursorIcon::Default,
            node_declared_cursor: crate::widget::CursorIcon::Default,
            pending_delayed_overlays: Vec::new(),
            active_ids_scratch: Vec::new(),
            gesture_owners: std::collections::HashSet::new(),
            prefers_high_contrast: false,
            prefers_reduced_motion: false,
            text_scale_factor: 1.0,
            screen_reader: crate::environment::ScreenReaderState::default(),
            explore_by_touch: crate::environment::ExploreByTouch::default(),
            at_client_attached: false,
            density_announcement: None,
            context_menu_announcement: None,
            pending_touch_route: None,
            device_scale_factor: 1.0,
            safe_area: teksilo_canvas::EdgeInsets::ZERO,
            occluded_inset: None,
            soft_keyboard_request: None,
            active_drag: None,
            outbound_drag_source: None,
            os_drag_reentered: false,
            os_drop_accepted: None,
            title_bar_host: None,
            app_context: Rc::new(crate::event_source::TreeAppContext::empty()),
            locale: None,
            locale_signal: crate::signal::Signal::new(None),
            frame_tick: crate::signal::Signal::new(0.0_f32),
            frame_tick_requested: std::rc::Rc::new(std::cell::Cell::new(false)),
            frame_tick_armed_by_subscribers: false,
            in_focus_dispatch: std::rc::Rc::new(std::cell::Cell::new(false)),
            a11y_update_requested: std::rc::Rc::new(std::cell::Cell::new(false)),
            pending_wake_at: std::rc::Rc::new(std::cell::Cell::new(None)),
            pending_mount_actions: Vec::new(),
            last_frame_time: None,
            close_window_requested: false,
            force_close_requested: false,
            pending_locale_request: None,
            pending_theme_request: None,
            pending_follow_system_request: false,
            pending_text_scale_request: None,
            at_version: crate::signal::Signal::new(0),
            announcer_polite: crate::announcer::Announcer::new(
                crate::announcer::Politeness::Polite,
            ),
            announcer_assertive: crate::announcer::Announcer::new(
                crate::announcer::Politeness::Assertive,
            ),
            announcer_ids: crate::announcer::AnnouncerIds::new(),
            announcement_ring: crate::accessibility::announcements::AnnouncementRing::new(),
            adapter_ids: crate::accessibility::adapter_ids::AdapterIds::new(),
            description_memory: Default::default(),
            access_action_handled: false,
            window_state: None,
        }
    }

    /// Construct an [`EventContext`]
    /// pre-populated with the tree's app-state registry, hosting
    /// `WindowState`, and a `&mut dyn WindowOps` handle so handlers
    /// can synchronously reach the multi-window API. Used by every
    /// dispatch site.
    pub(crate) fn make_event_context<'ops>(
        &self,
        ops: &'ops mut dyn crate::window::WindowOps,
    ) -> crate::widget::EventContext<'ops> {
        let drag_is_external = self.active_drag.as_ref().is_some_and(|d| d.is_external);
        // Read-only snapshot of tree query state that handlers may
        // need synchronously: the last pointer position and a
        // (content_id, bounds) slice of open overlays, both read by the
        // safe-triangle submenu hover gate, and the focused widget, read
        // by a container deciding whether a shortcut of its own should
        // yield to the widget the user is standing on.
        let overlay_snapshot: Vec<(
            crate::widget_id::WidgetId,
            teksilo_canvas::Rect,
            Option<teksilo_canvas::Point>,
        )> = self
            .overlay_manager
            .active_content_ids()
            .into_iter()
            .filter_map(|cid| {
                self.overlay_manager
                    .bounds_for_content(cid)
                    .map(|r| (cid, r, self.unexpired_safe_apex_for_content(cid)))
            })
            .collect();
        crate::widget::EventContext::new()
            .with_app_context(self.app_context.clone())
            .with_window_context(ops, self.window_state.clone())
            .with_drag_external(drag_is_external)
            .with_query_snapshot(
                self.last_pointer_position(),
                overlay_snapshot,
                self.focused(),
            )
            .with_pointer_captor(self.current_pointer_capture())
            .with_layout_direction(self.layout_direction)
            .with_window_active(self.is_window_active())
            .with_input_snapshot(self.current_input.clone())
            .with_touch_action(self.current_frozen_touch_action())
            .with_press(self.current_press_snapshot())
            .with_focus_dispatch_flag(self.in_focus_dispatch.clone())
    }

    /// Run a closure with a fresh [`EventContext`] anchored at this
    /// tree, then collect any pending operations queued through the
    /// context (intents, modal requests, frame requests, idle
    /// callbacks…) so they take effect on the next event-loop tick.
    ///
    /// Used by the `teksilo-app` event-loop dispatcher to deliver
    /// async-result callbacks (file dialogs, future background
    /// tasks) on the main thread with full handler-equivalent
    /// semantics. There is no source widget for app-level events,
    /// so intents are anchored at the tree's first root id (or
    /// silently dropped when the tree is empty).
    pub fn run_with_event_context<F>(&mut self, ops: &mut dyn crate::window::WindowOps, f: F)
    where
        F: FnOnce(&mut crate::widget::EventContext),
    {
        let mut ctx = self.make_event_context(ops);
        f(&mut ctx);
        let anchor = self.arena.roots().first().copied();
        if let Some(anchor_id) = anchor {
            self.collect_from_ctx(ctx, anchor_id);
        } else {
            // Empty tree — nothing to anchor intents on. Drop ctx;
            // its only side effects (frame requests, cursor) are
            // not meaningful for an empty tree.
            drop(ctx);
        }
    }

    /// Enqueue a one-shot action to run with a real
    /// [`EventContext`] after the current build,
    /// once the tree is mounted under its window. Used via
    /// [`BuildContext::run_after_mount`](crate::BuildContext::run_after_mount). Drained by
    /// [`Self::run_mount_actions`].
    pub(crate) fn queue_mount_action(
        &mut self,
        action: Box<dyn FnOnce(&mut crate::widget::EventContext)>,
    ) {
        self.pending_mount_actions.push(action);
    }

    /// Whether any post-mount actions are waiting to run.
    pub fn has_pending_mount_actions(&self) -> bool {
        !self.pending_mount_actions.is_empty()
    }

    /// Drain and run every queued post-mount action with a fresh
    /// [`EventContext`] built over `ops`. The app
    /// loop calls this each iteration with a real `WindowOps` sink (so
    /// `ctx.parent_window_handle()` resolves); headless tests call it with a
    /// `NoopWindowOps`. Actions enqueued *by* an action (rare) are left for the
    /// next drain rather than run re-entrantly.
    pub fn run_mount_actions(&mut self, ops: &mut dyn crate::window::WindowOps) {
        if self.pending_mount_actions.is_empty() {
            return;
        }
        let actions = std::mem::take(&mut self.pending_mount_actions);
        self.run_with_event_context(ops, move |ctx| {
            for action in actions {
                action(ctx);
            }
        });
    }

    /// Attach the [`WindowState`](crate::window::WindowState) for this
    /// tree's hosting window. Called by `WindowManager::create_window`.
    pub fn set_window_state(&mut self, state: crate::window::WindowState) {
        self.window_state = Some(state);
    }

    pub fn window_state(&self) -> Option<&crate::window::WindowState> {
        self.window_state.as_ref()
    }

    /// The window's title as its accessible name: `None` without a window
    /// state or with a blank title, since a name that is empty is still a
    /// name to every adapter.
    pub(crate) fn window_title_for_accessibility(&self) -> Option<String> {
        let title = self.window_state.as_ref()?.title().get();
        (!title.trim().is_empty()).then_some(title)
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
    /// their state and call `request_wake_at` from frame-tick effects
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
    /// See `BuildContext::frame_tick` for widget-side access and
    /// `BuildContext::request_frame` for the opt-in request side.
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

    /// Request that the AccessKit tree be re-walked on the next
    /// [`sync_accessibility`](Self::sync_accessibility). Takes `&self` (the
    /// flag is a `Cell`) so handlers and `build()` closures reaching the tree
    /// through a shared reference can request a re-walk without `&mut` access.
    /// The drain at the top of `sync_accessibility` flips `a11y_dirty`.
    pub fn request_accessibility_update(&self) {
        self.a11y_update_requested.set(true);
    }

    /// Speak `message` to the screen reader, politely.
    ///
    /// For anything the user needs told that is not the name of a widget: a
    /// completed action, a changed count, the result of an undo. The message is
    /// delivered on the next two accessibility syncs, which this schedules.
    ///
    /// Prefer `EventContext::announce` inside a handler and
    /// `BuildContext::announce` inside a build; this is the tree-level entry
    /// point both of those reach.
    ///
    /// Takes `impl Into<String>`, so `tr!(…)` works directly. See
    /// [`crate::announcer`] for why it is a `String` and not a
    /// `LocalizedString`, and for why an announcement beside a `Toast` says
    /// everything twice.
    pub fn announce(&mut self, message: impl Into<String>) {
        self.announce_with(message, crate::announcer::Politeness::Polite);
    }

    /// Speak `message` to the screen reader at the given urgency.
    ///
    /// [`Politeness::Assertive`](crate::announcer::Politeness::Assertive)
    /// interrupts whatever is being spoken; reserve it for something the user
    /// must not miss and cannot recover by re-reading the screen.
    pub fn announce_with(
        &mut self,
        message: impl Into<String>,
        politeness: crate::announcer::Politeness,
    ) {
        match politeness {
            crate::announcer::Politeness::Polite => self.announcer_polite.push(message.into()),
            crate::announcer::Politeness::Assertive => {
                self.announcer_assertive.push(message.into())
            }
        }
        // A message is put in the tree by a sync, and a sync only happens on
        // a frame. Without both of these a message queued from a handler that
        // changed nothing visible would sit unspoken until something else
        // happened to redraw.
        self.request_accessibility_update();
        self.request_frame();
    }

    /// Clone the shared "accessibility re-walk requested" flag, for the same
    /// stash-and-toggle pattern as [`frame_request_handle`](Self::frame_request_handle).
    pub fn a11y_request_handle(&self) -> std::rc::Rc<std::cell::Cell<bool>> {
        self.a11y_update_requested.clone()
    }

    /// Whether the next `layout()` will fire the per-frame tick: a frame
    /// was requested, or a render painted a frame-tick subscriber's owner
    /// and re-armed the chain. Exposed for tests and for the event-loop
    /// driver that decides when to schedule the next wake-up; *when* that
    /// frame is due is [`frame_tick_deadline`](Self::frame_tick_deadline).
    pub fn frame_requested(&self) -> bool {
        self.frame_tick_requested.get() || self.frame_tick_armed_by_subscribers
    }

    /// Consume both reasons for a frame tick at once, returning whether
    /// there was one. Every path that fires `frame_tick` goes through here,
    /// so neither flag can outlive the tick it asked for.
    pub(crate) fn take_frame_tick_request(&mut self) -> bool {
        let requested = self.frame_tick_requested.replace(false);
        let armed = std::mem::take(&mut self.frame_tick_armed_by_subscribers);
        requested || armed
    }

    /// The next wake-up deadline for the per-frame-effect path, or
    /// `None` when no per-frame effect is armed.
    ///
    /// This is the **60 Hz cap** for continuous per-frame animations
    /// (`Pulse`, caret blink, drag auto-scroll, `--cycle` drivers). The
    /// per-frame-effect path used to force `ControlFlow::Poll`, which
    /// free-runs at the display's refresh rate — so on a 300 Hz panel a
    /// single `Pulse`/`Cycle` rendered at 300 fps (measured ~45 % CPU) for
    /// motion that looks identical at 60 fps. Routing it through a fixed
    /// 16.667 ms deadline (folded into
    /// [`next_timer_deadline`](Self::next_timer_deadline)) makes it pace at
    /// 60 Hz regardless of refresh rate, matching the signal-tween
    /// [`AnimationScheduler`](crate::animation::AnimationScheduler) and
    /// shader-quad [`AnimatedQuadRegistry`](crate::animated_quad::AnimatedQuadRegistry),
    /// which already share the same interval.
    ///
    /// A **throttled** subscriber (registered via
    /// [`FrameTickScheduler::subscribe_throttled`](crate::frame_tick_scheduler::FrameTickScheduler::subscribe_throttled),
    /// e.g. `Cycle`, whose visible child only changes once per period)
    /// stretches the wait for **its own** tick to its interval: the loop
    /// then sleeps to the period instead of rendering identical 60 fps
    /// frames in between. The interval for the subscribers' re-arm is the
    /// **minimum across all currently-visible subscribers**, so a `Cycle`
    /// next to a `Pulse` still ticks at 60 Hz while a lone `Cycle` sleeps to
    /// its period.
    ///
    /// A frame asked for through [`request_frame`](Self::request_frame) or
    /// [`frame_request_handle`](Self::frame_request_handle) is due at 60 Hz
    /// whatever subscribers are visible, and the earlier of the two
    /// deadlines wins. The announcer, a caret, a drag auto-scroll and the
    /// bootstrap frame of a new subscription all ask that way, and none of
    /// them can be made to wait on somebody else's clock: an announcement
    /// queued behind a once-a-minute clock was spoken a minute late, or at
    /// the user's next key press.
    ///
    /// Paces from `last_frame_time` so the cadence is drift-free; before
    /// the first render it fires on the next loop turn.
    pub fn frame_tick_deadline(&self) -> Option<std::time::Instant> {
        const DEFAULT_INTERVAL: std::time::Duration = std::time::Duration::from_micros(16_667);
        // Each reason for a frame brings its own pace. A subscriber re-arm
        // with no visible subscriber left keeps the 60 Hz fallback, so the
        // one frame that finds nothing to re-arm still runs and the flag
        // does not linger.
        let requested = self.frame_tick_requested.get().then_some(DEFAULT_INTERVAL);
        let subscribed = self.frame_tick_armed_by_subscribers.then(|| {
            self.frame_tick_scheduler
                .min_visible_interval(&self.arena, self.paint_epoch)
                .unwrap_or(DEFAULT_INTERVAL)
        });
        let interval = requested.into_iter().chain(subscribed).min()?;
        Some(match self.last_frame_time {
            Some(prev) => prev + interval,
            None => std::time::Instant::now(),
        })
    }

    /// Subscribe `owner` to the per-frame-effect scheduler. The
    /// returned [`FrameTickSubscription`](crate::frame_tick_scheduler::FrameTickSubscription)
    /// is an RAII guard — drop it (typically by replacing the field on
    /// the owning widget on rebuild, or letting the widget's `Drop`
    /// run) to remove the subscription. While the guard is alive, the
    /// tree will keep re-arming the frame tick after every render
    /// in which `owner` was painted, and stop on frames where it
    /// wasn't — so a subscribed widget hidden inside a non-selected
    /// `Switcher` branch contributes zero idle frames.
    ///
    /// Apps should not call this directly — use
    /// [`BuildContext::subscribe_frame_tick`](crate::build_context::BuildContext::subscribe_frame_tick)
    /// from inside `Widget::build`.
    pub fn subscribe_frame_tick(
        &self,
        owner: WidgetId,
    ) -> crate::frame_tick_scheduler::FrameTickSubscription {
        self.frame_tick_scheduler.subscribe(owner)
    }

    /// Like [`subscribe_frame_tick`](Self::subscribe_frame_tick), but the
    /// owner only needs to wake **at most once per `interval`** while
    /// visible. Same visibility gate; between wakes the event loop sleeps
    /// to the interval deadline instead of rendering identical 60 fps
    /// frames. Use for effects whose visible output changes far less often
    /// than 60 Hz — e.g. `Cycle`'s once-per-period index advance.
    ///
    /// Apps should not call this directly — use
    /// [`BuildContext::subscribe_frame_tick_throttled`](crate::build_context::BuildContext::subscribe_frame_tick_throttled)
    /// from inside `Widget::build`.
    pub fn subscribe_frame_tick_throttled(
        &self,
        owner: WidgetId,
        interval: std::time::Duration,
    ) -> crate::frame_tick_scheduler::FrameTickSubscription {
        self.frame_tick_scheduler
            .subscribe_throttled(owner, interval)
    }

    /// Advance the frame tick signal when (and only when) a frame was
    /// requested. Called by `layout()` before the scheduler tick so the
    /// per-frame observers fire on the same frame they asked for.
    pub(crate) fn advance_frame_tick(&mut self, now: std::time::Instant) {
        if !self.take_frame_tick_request() {
            self.last_frame_time = Some(now);
            return;
        }
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

    /// Replace the per-tree app context. Called by `teksilo-app` when
    /// constructing a window so the widget tree can reach the registered
    /// event source adapter and post subscription events through the
    /// event-loop proxy.
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
    /// observes the teksilo-i18n manager, and anything else that depends on the
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

    /// Whether the overlay's safe region is armed and still within
    /// [`SAFE_REGION_BUDGET`](crate::overlay::SAFE_REGION_BUDGET).
    ///
    /// Both clocks are consulted and either expiring is enough: the real
    /// one drives a running app, the simulated one drives headless tests
    /// (`advance_time`), and an overlay armed in one and aged in the
    /// other must still expire.
    fn safe_region_unexpired(
        &self,
        overlay_id: crate::overlay::OverlayId,
        real_now: std::time::Instant,
        sim_now: std::time::Instant,
    ) -> bool {
        let Some(overlay) = self
            .overlay_manager
            .stack
            .iter()
            .find(|overlay| overlay.id == overlay_id)
        else {
            return false;
        };
        if overlay.safe_apex.is_none() {
            return false;
        }
        let budget = crate::overlay::SAFE_REGION_BUDGET;
        let real_ok = overlay
            .safe_apex_started_real
            .is_none_or(|started| real_now.saturating_duration_since(started) < budget);
        let sim_ok = overlay
            .safe_apex_started_sim
            .is_none_or(|started| sim_now.saturating_duration_since(started) < budget);
        real_ok && sim_ok
    }

    /// The armed safe-triangle apex of the overlay rooted at
    /// `content_id`, or `None` when no region is armed or its budget is
    /// spent.
    ///
    /// This is the form the per-dispatch `EventContext` snapshot carries,
    /// because a widget asking "is a traversal toward that submenu still
    /// live?" must get the same answer the dismissal path acts on — an
    /// unfiltered apex outlives the region by up to a frame and makes a
    /// sibling row stand aside for a submenu the framework has already
    /// stopped protecting.
    fn unexpired_safe_apex_for_content(
        &self,
        content_id: crate::widget_id::WidgetId,
    ) -> Option<teksilo_canvas::Point> {
        // Cheap short-circuit first: nothing is armed in the common case,
        // and this runs once per node per pointer dispatch.
        let apex = self.overlay_manager.safe_apex_for_content(content_id)?;
        let id = self.overlay_manager.find_by_content(content_id)?;
        self.safe_region_unexpired(id, std::time::Instant::now(), self.sim_clock)
            .then_some(apex)
    }

    fn update_pointer_leave_overlays(
        &mut self,
        position: Point,
        ops: &mut dyn crate::window::WindowOps,
    ) {
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
            // A pointer travelling the safe triangle toward this overlay
            // is still "inside" as far as dismissal is concerned — that
            // is the whole point of the triangle. Without this the
            // grace period runs for the entire diagonal and closes the
            // submenu mid-flight, no matter what the hover handlers do.
            // See `overlay::safe_triangle`.
            let armed = self.safe_region_unexpired(overlay_id, real_now, sim_now);
            let travelling = !inside
                && armed
                && self
                    .overlay_manager
                    .point_in_safe_region(overlay_id, position);
            // Arrived, or returned to the trigger row: the traversal is
            // over and the ordinary rules apply again.
            //
            // Straying out of the cone deliberately does NOT disarm. The
            // cone is a needle at its apex — the first sample after the
            // pointer leaves the trigger row is a pixel or two away, so
            // whether it lands inside is one quantized step's direction
            // rather than the user's intent. Disarming there made that
            // sample final, because a region is only ever armed as the
            // pointer leaves the row and the pointer has already left:
            // nothing could re-arm it. Keeping it armed makes the cone a
            // per-sample hold that the grace below re-evaluates instead,
            // so a wobble mid-diagonal costs nothing while a real change
            // of mind still closes the submenu one close-delay later. The
            // budget is what retires a region for good; the frame pass
            // spends it even for a pointer that stopped moving entirely.
            if armed && inside {
                self.overlay_manager.clear_safe_region(overlay_id);
            }
            if let Some(overlay) = self
                .overlay_manager
                .stack
                .iter_mut()
                .find(|overlay| overlay.id == overlay_id)
            {
                if inside || travelling {
                    overlay.pointer_leave_started_real = None;
                    overlay.pointer_leave_started_sim = None;
                } else if overlay.pointer_leave_started_real.is_none() {
                    overlay.pointer_leave_started_real = Some(real_now);
                    overlay.pointer_leave_started_sim = Some(sim_now);
                    self.arena.mark_needs_paint(overlay.anchor);
                }
            }
        }

        self.process_pointer_leave_overlays_real(&mut *ops);
    }

    fn process_pointer_leave_overlays(&mut self) {
        let sim_now = self.sim_clock;
        let mut noop = crate::window::NoopWindowOps;
        self.process_pointer_leave_overlays_impl(
            |overlay| {
                overlay
                    .pointer_leave_started_sim
                    .map(|started| sim_now.saturating_duration_since(started))
            },
            &mut noop,
        );
    }

    fn process_pointer_leave_overlays_real(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let real_now = std::time::Instant::now();
        self.process_pointer_leave_overlays_impl(
            |overlay| {
                overlay
                    .pointer_leave_started_real
                    .map(|started| real_now.saturating_duration_since(started))
            },
            &mut *ops,
        );
    }

    fn process_auto_dismiss_overlays(&mut self) {
        let sim_now = self.sim_clock;
        let mut noop = crate::window::NoopWindowOps;
        self.process_auto_dismiss_overlays_impl(
            |overlay| {
                overlay
                    .auto_dismiss_after
                    .map(|_| sim_now.saturating_duration_since(overlay.shown_at_sim))
            },
            &mut noop,
        );
    }

    fn process_auto_dismiss_overlays_real(&mut self, ops: &mut dyn crate::window::WindowOps) {
        let real_now = std::time::Instant::now();
        self.process_auto_dismiss_overlays_impl(
            |overlay| {
                overlay
                    .auto_dismiss_after
                    .map(|_| real_now.saturating_duration_since(overlay.shown_at_real))
            },
            &mut *ops,
        );
    }

    /// Drain overlays whose fade-out tween has completed (set up by
    /// `OverlayRequest::with_fade`). Parks the content as the normal
    /// dismiss path does, and hands focus back only when that leaves it
    /// nowhere ([`restore_focus_left_behind`](Self::restore_focus_left_behind)):
    /// the dismissal that started the fade may have meant to leave focus
    /// alone, and the user may have moved it on since; called once per layout pass
    /// after `process_auto_dismiss_overlays_real` so an overlay that
    /// hits its auto-dismiss deadline kicks off its fade-out tween
    /// in the same pass.
    pub(crate) fn process_overlay_fade_dismissals_real(
        &mut self,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        let now = std::time::Instant::now();
        let pending = self.overlay_manager.process_pending_fade_dismissals(now);
        for (_id, dismissed, focus_restore) in pending {
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            self.restore_focus_left_behind(focus_restore, &mut *ops);
        }
    }

    /// Sim-clock variant for headless tests. Same shape as
    /// [`process_overlay_fade_dismissals_real`](Self::process_overlay_fade_dismissals_real)
    /// but reads `dismissing_started_sim`.
    pub(crate) fn process_overlay_fade_dismissals_sim(&mut self) {
        let mut noop = crate::window::NoopWindowOps;
        let pending = self
            .overlay_manager
            .process_pending_fade_dismissals_sim(self.sim_clock);
        for (_id, dismissed, focus_restore) in pending {
            self.dormant_dismissed_content(&dismissed, &mut noop);
            self.restore_focus_left_behind(focus_restore, &mut noop);
        }
    }

    /// Hand focus to `restore` after an overlay has gone of its own accord
    /// (its auto-dismiss timer ran out, or its fade-out ended), but only when
    /// focus is left nowhere: the overlay held it and
    /// [`dormant_dismissed_content`](Self::dormant_dismissed_content) has just
    /// taken it away with the content, or nothing held it. Focus the user has
    /// moved elsewhere since the overlay opened stays where they put it. A tip
    /// that focus summoned holds its anchor as `restore`, and restoring that
    /// whatever held focus sent a reader who had Tabbed on back to the control
    /// they had just left.
    fn restore_focus_left_behind(
        &mut self,
        restore: Option<WidgetId>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        if let Some(restore) = restore
            && self.arena.is_active(restore)
            && self.focused.is_none_or(|id| !self.arena.is_active(id))
        {
            self.focus_ops(restore, ops);
        }
    }

    fn process_auto_dismiss_overlays_impl(
        &mut self,
        elapsed_fn: impl Fn(&crate::overlay::ActiveOverlay) -> Option<std::time::Duration>,
        ops: &mut dyn crate::window::WindowOps,
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
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            self.restore_focus_left_behind(focus_restore, &mut *ops);
        }
    }

    fn process_pointer_leave_overlays_impl(
        &mut self,
        elapsed_fn: impl Fn(&crate::overlay::ActiveOverlay) -> Option<std::time::Duration>,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Retire safe regions whose budget is spent. `update_pointer_leave_overlays`
        // does this too, but only when the pointer moves — and a pointer parked
        // inside the cone generates no moves at all, so without this pass the
        // submenu would stay open indefinitely. Expiring here also starts the
        // ordinary grace, so the overlay closes `delay` later exactly as if the
        // triangle had never been armed.
        let real_now = std::time::Instant::now();
        let sim_now = self.sim_clock;
        let expired: Vec<crate::overlay::OverlayId> = self
            .overlay_manager
            .stack
            .iter()
            .filter(|overlay| overlay.safe_apex.is_some())
            .map(|overlay| overlay.id)
            .filter(|id| !self.safe_region_unexpired(*id, real_now, sim_now))
            .collect();
        let budget = crate::overlay::SAFE_REGION_BUDGET;
        for id in expired {
            if let Some(overlay) = self
                .overlay_manager
                .stack
                .iter_mut()
                .find(|overlay| overlay.id == id)
                && overlay.pointer_leave_started_real.is_none()
            {
                // Backdate to the instant the budget ran out: the
                // pointer stopped counting as "inside" then, not when a
                // frame got around to noticing. Otherwise the grace
                // silently restarts from zero and the total wait
                // depends on the frame rate.
                let backdate = |started: Option<std::time::Instant>, now: std::time::Instant| {
                    started
                        .and_then(|s| s.checked_add(budget))
                        .map(|deadline| deadline.min(now))
                        .unwrap_or(now)
                };
                overlay.pointer_leave_started_real =
                    Some(backdate(overlay.safe_apex_started_real, real_now));
                overlay.pointer_leave_started_sim =
                    Some(backdate(overlay.safe_apex_started_sim, sim_now));
            }
            self.overlay_manager.clear_safe_region(id);
        }

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
            self.dormant_dismissed_content(&dismissed, &mut *ops);
            if let Some(restore_id) = focus_restore
                && self.arena.is_active(restore_id)
            {
                self.focus_ops(restore_id, &mut *ops);
            }
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        // Update both the cached `Theme` AND the reactive
        // `theme_signal` — widgets that observe the signal (e.g.
        // `TextInputField` resetting the rich-text engine's default
        // text colour) would otherwise see the constructor's
        // `presets::intui::light()` initial value forever, even when
        // `TeksiloAppBuilder.theme(crate::presets::intui::dark())` was used.
        // `set_theme` already does this; `with_theme` was the
        // builder-time analogue that forgot to keep them aligned.
        self.theme = theme.clone();
        self.theme_signal.set(theme);
        self.recompute_effective_theme();
        self
    }

    pub fn with_text_backend(
        mut self,
        backend: Rc<RefCell<dyn teksilo_canvas::TextBackend>>,
    ) -> Self {
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
    /// platform does not support it (X11 without an EWMH-capable window
    /// manager, or a headless build).
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
    /// pause would save nothing. Both the signal scheduler AND the
    /// shader-driven animated-quad registry are consulted — a
    /// ProgressBar::indeterminate whose widget has no pending paint
    /// dirt still needs the loop to keep waking at the animation
    /// frame interval so its phase advances.
    pub fn needs_redraw(&self) -> bool {
        self.arena.any_needs_layout()
            || self.arena.any_needs_paint()
            || self.animation_scheduler.has_running()
            || self.animated_quads.has_running()
            || self.frame_requested()
    }

    /// Whether a render pass is needed (any widget needs layout or paint).
    pub fn needs_render(&self) -> bool {
        self.arena.any_needs_layout() || self.arena.any_needs_paint()
    }

    /// Whether this tree has reactive work that only a `layout()` pass can
    /// turn into arena dirt — i.e. whether reconciling it right now could
    /// change what [`needs_render`](Self::needs_render) reports.
    ///
    /// Read-only and cheap: `O(unique bound sources)` `u64` comparisons
    /// plus one peek per registered animated signal. No arena walk, no
    /// rebuilds, no geometry. Asking does not consume the answer, so it
    /// can be asked every dispatch.
    ///
    /// # Why this is exactly the right question, and no broader
    ///
    /// `teksilo_app::WindowManager::request_redraw_needing_render` exists
    /// for ONE case: a handler in window A wrote a `Signal` that window
    /// B's widgets also bind, and B — which never saw the event — must be
    /// reconciled before anyone can tell it needs repainting. Every OTHER
    /// thing `layout_with_ops` drives already has its own scheduling
    /// path and does not need this sweep:
    ///
    /// - tooltip dwell + sticky steps, delayed overlays, auto-dismiss,
    ///   overlay fades, the animation scheduler, animated quads,
    ///   gestures, `wake_at` and the 60 Hz frame tick are all timing
    ///   driven, and every one of them contributes to
    ///   [`next_timer_deadline`](Self::next_timer_deadline) — which
    ///   `request_redraw_due` polls to wake precisely the due windows;
    /// - drag ticks follow that window's own pointer stream;
    /// - a handler that called `request_rebuild` marked the arena
    ///   directly, so `needs_render()` is already true without any
    ///   reconcile.
    ///
    /// So the two terms below are what the sweep uniquely covers:
    /// binding-registry staleness (the whole point), and a *pending*
    /// `animate_to` — which the scheduler has not started yet, so it
    /// contributes no deadline, and which only `process_pending_animations`
    /// (inside `layout`) can promote into one. The second term is
    /// belt-and-braces: arming an animation also advances the signal's
    /// generation, so a bound animated signal is already covered by the
    /// first — but an animated signal registered without being bound
    /// would not be, and this makes that impossible to get wrong.
    pub fn needs_reconcile(&self) -> bool {
        self.binding_registry.any_dirty()
            || self
                .animated_values
                .iter()
                .any(AnimatedRegistration::has_pending_animation)
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
        let now = self.animation_clock();
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
    /// inactive. Propagates to the animation scheduler AND the
    /// animated-quad registry so both pause-resume in lockstep — no
    /// ticks, no frame wakes, no GPU submits.
    ///
    /// On an actual state change it also fires `window_active_signal`
    /// (so build-time binders and `DimWhenInactive` react) and issues a
    /// global paint-only dirty mark, so every widget that reads
    /// `PaintContext::window_active` (caret gates, selection bands) repaints
    /// once. This is a repaint, not a relayout — geometry never changes when
    /// the window's active state flips (the caret keeps its space). Window
    /// focus changes are rare and user-driven, so the O(n) mark is cheap and
    /// Mark every node paint-dirty (no relayout, no rebuild) so the next
    /// render re-runs their `paint()`. This is the paint-cache invalidation an
    /// off-thread source needs after posting a [`RepaintWindowRequest`](crate::RepaintWindowRequest):
    /// a bare redraw request re-presents the cached frame, so a widget whose
    /// content changed off the UI thread (a terminal's PTY output) must be
    /// marked dirty for its `paint()` to run again.
    pub fn mark_all_needs_paint_only(&mut self) {
        self.arena.mark_all_needs_paint_only();
    }

    /// strictly lighter than `set_theme`'s `mark_all_dirty` (layout + paint).
    pub fn set_window_active(&mut self, active: bool) {
        let mut noop = crate::window::NoopWindowOps;
        self.set_window_active_with_ops(active, &mut noop);
    }

    /// [`set_window_active`](Self::set_window_active) with the caller's
    /// app-level [`WindowOps`](crate::window::WindowOps) sink, so the
    /// `on_pointer_cancel` handlers a deactivation fires can reach the
    /// multi-window API like any other handler.
    pub fn set_window_active_with_ops(
        &mut self,
        active: bool,
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // The scheduler is measured on the tree's animation axis, so its pause
        // mark has to be taken there too — a pause stamped on the wall clock
        // while time is simulated would rebase every animation by the gap
        // between the axes on resume. The shader-driven quad registry has no
        // simulated door at all (it is ticked from `render()`), so it keeps the
        // wall clock.
        self.animation_scheduler
            .set_window_active(active, self.animation_clock());
        self.animated_quads
            .set_window_active(active, std::time::Instant::now());
        if self.window_active_signal.get() != active {
            self.window_active_signal.set(active);
            self.arena.mark_all_needs_paint_only();
            if !active {
                // A held pointer first, before any other state is cleared: a
                // widget that captured the pointer for a drag (a column-resize
                // grip, a splitter divider, a scrollbar thumb, a slider) will
                // never see the matching `PointerUp` — the user releases the
                // button over the window that took focus, and this window is
                // told nothing. Releasing the capture silently, which is what
                // this used to do, strands the widget instead: it keeps the
                // half of the interaction it owns, with no event left that
                // could clear it. The cancel funnel releases the capture *and*
                // tells it, so it can let go.
                self.cancel_all_pointers(
                    crate::pointer::CancelReason::WindowDeactivated,
                    &mut *ops,
                );
                // The pointer has left for another window; the OS sends no
                // leave event we can rely on, so a tooltip shown at the moment
                // of the switch would float over the newly-focused window's
                // chrome with nothing left to dismiss it. Retire tips and
                // cancel pending dwells — but leave *sticky* ones, which the
                // user pinned deliberately and expects to find on return.
                self.tooltip_window_deactivated();
            }
        }
    }

    /// Whether the owning window is currently active (`focused AND not
    /// occluded`). The reactive companion is [`Self::window_active_signal`].
    pub fn is_window_active(&self) -> bool {
        self.window_active_signal.get()
    }

    /// Reactive handle on window-active state. Fires when the window gains or
    /// loses active status. Bind at [`BindingLevel::RepaintOnly`] — an
    /// active-state flip never affects geometry. Starts `true`.
    ///
    /// [`BindingLevel::RepaintOnly`]: crate::binding::BindingLevel::RepaintOnly
    pub fn window_active_signal(&self) -> crate::signal::Signal<bool> {
        self.window_active_signal.clone()
    }

    /// Register a new animated quad for the currently-building widget.
    /// Called by [`crate::build_context::BuildContext::animated_quad`];
    /// returns an opaque handle the widget stashes for its `paint()`
    /// call.
    pub fn register_animated_quad(
        &mut self,
        owner: WidgetId,
        kind: crate::animated_quad::AnimatedQuadKind,
    ) -> crate::animated_quad::AnimatedQuadHandle {
        self.animated_quads
            .register(owner, kind, std::time::Instant::now())
    }

    /// Active animated-quad slot count. Test / debug helper.
    pub fn animated_quad_count(&self) -> usize {
        self.animated_quads.active_count()
    }

    /// Advance animations by simulated time (for deterministic testing).
    ///
    /// An **alias** of [`advance_time`](Self::advance_time), not a second door.
    /// It was one once, and the two moved disjoint halves of the tree from
    /// clocks they each advanced independently: a caller that wanted both had
    /// to call both, which advanced simulated time twice, and a caller that
    /// wanted one silently froze the other — an animation and the fling it was
    /// racing could not be moved to the same instant by any sequence of calls.
    /// Kept as a name rather than folded away because it reads correctly at
    /// its ~120 call sites, all of which mean "advance the clock".
    pub fn tick_animations(&mut self, duration: std::time::Duration) {
        self.advance_time(duration);
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
        self.recompute_effective_theme();
        self.arena.mark_all_dirty();
        // A theme change re-resolves typography, so every label re-shapes —
        // inside bounds the layout pass may leave untouched, which means no
        // resize is recorded and nothing else would dirty the tree. The
        // label's lines, and therefore its text runs, can be a different
        // set at the same size.
        self.a11y_dirty = true;
    }

    /// The [`TargetDensity`] the active theme was projected onto.
    ///
    /// [`TargetDensity`]: teksilo_tokens::TargetDensity
    pub fn input_density(&self) -> teksilo_tokens::TargetDensity {
        self.theme.input.density
    }

    /// Project the active theme onto another density and **rebuild** the tree.
    ///
    /// A rebuild, not [`Self::set_theme`]'s `mark_all_dirty()`: a target size is
    /// baked in `build()` (a `MinSize` wrapper, a recipe's `Rc<dyn FooStyle>`,
    /// the number of `Toolbar` items that fit), and marking layout + paint
    /// cannot re-bake it. This reuses the exact path a
    /// `BindingLevel::Rebuild` binding takes — `mark_needs_rebuild` on each
    /// root plus `mark_ancestors_need_layout` — so the next layout pass drains
    /// it through `process_rebuilds`, which already handles focus restoration,
    /// the a11y re-walk and interaction-state revalidation.
    ///
    /// A no-op when the density is already the requested one: a density switch
    /// throws away every widget id in the tree, so it must not fire on a
    /// repeated set.
    ///
    /// [`TargetDensity`]: teksilo_tokens::TargetDensity
    pub fn set_input_density(&mut self, density: teksilo_tokens::TargetDensity) {
        if self.theme.input.density == density {
            return;
        }
        // Exactly one utterance per switch, guaranteed by the guard above:
        // every widget id in the tree is about to be thrown away, so a screen
        // reader that was reading one is about to lose its place and needs to
        // be told what happened.
        if let Some(wording) = self.density_announcement.clone() {
            self.announce(wording(density));
        }
        self.set_theme(self.theme.with_density(density));
        // The `BindingLevel::Rebuild` arm of `process_state_changes`
        // (`widget_tree/layout_impl.rs`), applied at every root.
        for root in self.arena.roots() {
            self.arena.mark_needs_rebuild(root);
            self.arena.mark_ancestors_need_layout(root);
        }
    }

    /// Announce density switches to a screen reader, in the application's own
    /// words.
    ///
    /// A density switch rebuilds the entire tree, so a screen reader loses its
    /// place and the user hears no explanation for it. Registering a wording
    /// makes [`Self::set_input_density`] speak once — and only once — per real
    /// switch, through the same [`Self::announce`] path everything else uses.
    ///
    /// The wording is the application's because it cannot be the framework's:
    /// `teksilo-i18n` depends on this crate, so nothing here can name
    /// `LocalizedString` or reach a translation bundle, and a hardcoded English
    /// sentence spoken into a French screen reader is worse than silence. Pass
    /// a closure that resolves `tr!(…)`:
    ///
    /// ```ignore
    /// tree.set_density_announcement(Some(std::rc::Rc::new(|d| match d {
    ///     TargetDensity::Compact => tr!(layout_compact()).into(),
    ///     TargetDensity::Comfortable => tr!(layout_comfortable()).into(),
    ///     TargetDensity::Touch => tr!(layout_touch()).into(),
    /// })));
    /// ```
    ///
    /// `None` — the default — announces nothing.
    pub fn set_density_announcement(
        &mut self,
        wording: Option<std::rc::Rc<dyn Fn(teksilo_tokens::TargetDensity) -> String>>,
    ) {
        self.density_announcement = wording;
    }

    /// How to word "a context menu opened" for a screen reader, when the menu
    /// was opened by a **hold**.
    ///
    /// The other three routes need nothing: a secondary press, `Shift+F10` and
    /// the AccessKit `ShowContextMenu` action are all deliberate, and the menu
    /// takes focus, which is announcement enough. A hold is the one route whose
    /// user cannot see the menu appear — a finger is on top of where it opens —
    /// and which they may not have meant.
    ///
    /// The wording is the application's for the same reason
    /// [`Self::set_density_announcement`]'s is: `teksilo-i18n` depends on this
    /// crate, so nothing here can name a `LocalizedString`, and a hardcoded
    /// English sentence spoken into a French screen reader is worse than
    /// silence.
    ///
    /// ```ignore
    /// tree.set_context_menu_announcement(Some(std::rc::Rc::new(|| {
    ///     tr!(context_menu_opened()).into()
    /// })));
    /// ```
    ///
    /// `None` — the default — announces nothing. Where it is set, the
    /// announcement is still suppressed if the **pressed node's** own subtree
    /// already carries a live region — the widget speaking for itself, so the
    /// framework does not speak over it — through
    /// [`Self::announce_unless_widget_speaks`]. The menu's own subtree is not
    /// consulted: it is raised by this very call and has not been walked yet.
    pub fn set_context_menu_announcement(
        &mut self,
        wording: Option<std::rc::Rc<dyn Fn() -> String>>,
    ) {
        self.context_menu_announcement = wording;
    }

    /// Speak `message`, unless `widget` already speaks for itself.
    ///
    /// A framework announcement that lands beside a widget's own live region
    /// says everything twice — the failure mode [`crate::announcer`] warns
    /// about for `Toast`. This is the check that avoids it: if the last
    /// accessibility tree carried a live region *inside* `widget`'s subtree
    /// that the platform adapters would speak for (in the filtered tree, with
    /// a name), the widget is already talking and this stays quiet. A live
    /// region that is hidden, or that carries its text where its role takes
    /// no name from, is silent on every platform and does not count.
    /// Returns whether the message was queued.
    ///
    /// It necessarily reads **one tree behind**. An announcement is queued
    /// during event dispatch; the live-region text it would duplicate is
    /// whatever the *last* built update carried, because the next one has not
    /// been built yet. A widget that speaks for the first time in the same
    /// dispatch is therefore not yet visible here — which is the right bias:
    /// it errs toward saying something rather than toward silence.
    pub fn announce_unless_widget_speaks(
        &mut self,
        widget: WidgetId,
        message: impl Into<String>,
    ) -> bool {
        if self.widget_subtree_speaks(widget) {
            return false;
        }
        self.announce(message);
        true
    }

    /// Whether the last delivered accessibility tree had a live node in
    /// `widget`'s subtree that the platform adapters would speak for.
    ///
    /// Read through `accesskit_consumer`, so by the adapters' rules, whether
    /// or not this tree records announcements: the node is in the filtered
    /// tree (not hidden, not inside a hidden subtree), it is live, itself or
    /// through an ancestor, and it has a name, which for a `Status` is its
    /// label and not its value.
    /// A live region no platform can hear does not speak, and must not keep
    /// the framework quiet. The politeness must be set inside the subtree,
    /// too: a widget that is live only because it sits in somebody else's
    /// live region is not the one speaking.
    ///
    /// Synthetic children count too, resolved to their owning widget through
    /// the same parent map the AccessKit action router uses. `teksilo-scene`
    /// can mark a scene item as a live region, and a live region is a live
    /// region wherever it was emitted from.
    fn widget_subtree_speaks(&self, widget: WidgetId) -> bool {
        let mut wanted: std::collections::HashSet<accesskit::NodeId> =
            std::collections::HashSet::new();
        let mut stack = vec![widget];
        while let Some(id) = stack.pop() {
            if self.arena.get(id).is_none() {
                continue;
            }
            wanted.insert(crate::accessibility::widget_id_to_node_id(id));
            stack.extend_from_slice(self.arena.children(id));
        }
        let Some(update) = &self.cached_a11y else {
            return false;
        };
        crate::accessibility::announcements::speaks_within(update, |node_id| {
            wanted.contains(&node_id)
                || self
                    .synthetic_parent_map
                    .get(&node_id)
                    .is_some_and(|owner| {
                        wanted.contains(&crate::accessibility::widget_id_to_node_id(*owner))
                    })
        })
    }

    /// How the active density is chosen. See [`DensityPolicy`].
    ///
    /// [`DensityPolicy`]: teksilo_tokens::DensityPolicy
    pub fn density_policy(&self) -> teksilo_tokens::DensityPolicy {
        self.density_policy
    }

    /// Set the density-selection policy.
    ///
    /// Storing a `Fixed(d)` policy does **not** by itself switch the density —
    /// call [`Self::set_input_density`] for that. `FollowLastPointer` is not
    /// acted on either: it is state and an accessor, and the field's own
    /// documentation says what is still undecided about honouring it.
    pub fn set_density_policy(&mut self, policy: teksilo_tokens::DensityPolicy) {
        self.density_policy = policy;
    }

    /// Whether touch input is accepted. See
    /// [`InputTokens::touch_enabled`](teksilo_tokens::InputTokens::touch_enabled).
    pub fn touch_enabled(&self) -> bool {
        self.theme.input.touch_enabled
    }

    /// The runtime touch kill switch. With `false`, the platform translator
    /// drops touch input and the router installs no touch-only recognizers,
    /// so an app can fall back to mouse-only behaviour at runtime.
    ///
    /// Repaint-level only: turning touch off changes which events are accepted,
    /// never a dimension, so nothing is rebuilt or relaid out here. (The
    /// translator and router honour it from P08 / P15; this is the state.)
    pub fn set_touch_enabled(&mut self, enabled: bool) {
        if self.theme.input.touch_enabled == enabled {
            return;
        }
        let mut theme = self.theme.clone();
        theme.input.touch_enabled = enabled;
        self.theme = theme.clone();
        self.theme_signal.set(theme);
        self.recompute_effective_theme();
    }

    /// Recompute [`Self::effective_theme`] from the current `theme` and the
    /// combined text scale (`user_text_scale * text_scale_factor`). Callers
    /// that change either input are responsible for `mark_all_dirty()`.
    fn recompute_effective_theme(&mut self) {
        let combined = (self.user_text_scale as f64 * self.text_scale_factor) as f32;
        self.effective_theme = if (combined - 1.0).abs() < f32::EPSILON {
            self.theme.clone()
        } else {
            let mut t = self.theme.clone();
            t.typography = t.typography.scaled(combined);
            t
        };
        // Single source: every downstream consumer (the layout/paint context
        // `text_scale` field, the reactive `text_scale_signal`) reads from here.
        self.effective_text_scale = combined;
        self.text_scale_signal.set(combined);
    }

    /// The combined effective text scale (`user_text_scale * OS text_scale_factor`).
    /// Read by the layout/paint walkers to populate `ctx.text_scale` for widgets
    /// that size from a source other than `Theme.typography`.
    pub fn effective_text_scale(&self) -> f32 {
        self.effective_text_scale
    }

    /// Reactive handle on [`Self::effective_text_scale`]. Build-time binders that
    /// must react to a scale change without their own rebuild path bind this
    /// (e.g. `Calendar` binds it at `Rebuild` level so its fixed cell constants
    /// recompute). Fires on `set_user_text_scale` / theme / OS-pref change.
    pub fn text_scale_signal(&self) -> crate::signal::Signal<f32> {
        self.text_scale_signal.clone()
    }

    /// Set the user-controlled global text-scale factor (`1.0` = 100 %).
    ///
    /// The factor multiplies with the OS accessibility text-scale preference to
    /// produce the rendered scale. Recomputes the effective theme and marks all
    /// widgets dirty so every text widget grows on the next pass; no rebuild,
    /// so focus/scroll/interaction state survive. Values outside `[0.25, 8.0]`
    /// are clamped. Persisted by the application via
    /// `teksilo_settings::TEXT_SCALE_KEY`.
    pub fn set_user_text_scale(&mut self, factor: f32) {
        let clamped = factor.clamp(0.25, 8.0);
        if (self.user_text_scale - clamped).abs() < f32::EPSILON {
            return;
        }
        self.user_text_scale = clamped;
        self.recompute_effective_theme();
        self.arena.mark_all_dirty();
        // Same reasoning as `set_theme`: bigger text re-wraps inside a box
        // whose size the parent may hold fixed.
        self.a11y_dirty = true;
    }

    /// The current user-controlled text-scale factor (`1.0` = 100 %).
    pub fn user_text_scale(&self) -> f32 {
        self.user_text_scale
    }

    /// After a rebuild that destroyed subtrees — or after a `visible_when` /
    /// `Switcher` pass parks the focused widget dormant — drop any interaction
    /// state (focus, hover) whose target `WidgetId` is no longer active.
    ///
    /// **FocusLost is load-bearing.** Clearing `self.focused` alone leaves the
    /// widget's own `on_focus` / `has_focus` / caret-blink state thinking it is
    /// still focused. A rich-text editor in that state keeps scheduling
    /// `wake_at` caret toggles and re-arming `frame_request` from its tick
    /// effect — and because `frame_tick` observers are **not** gated on
    /// dormancy, every open tab's editor (TabWidget mounts them all) still runs
    /// on those wakes. Rapid tab switches that park a focused editor without a
    /// real focus move (programmatic selection, race with pointer focus) used
    /// to accumulate stuck "focused" editors and unbounded frame work. Dispatch
    /// `FocusLost` first so widgets clear that state, then drop the tree's
    /// focus pointer.
    ///
    /// Preserves state when the target still exists *and* is active. Called from
    /// data-driven rebuild paths (`process_state_changes`) and after every
    /// rebuild drain; theme and locale switches no longer rebuild.
    pub(crate) fn revalidate_interaction_state(&mut self, ops: &mut dyn crate::window::WindowOps) {
        if let Some(id) = self.focused
            && !self.arena.is_active(id)
        {
            let old = self.focused;
            // Deliver FocusLost while the node still exists (dormant or about to
            // be torn down). Skip if the node is already gone — destroy paths
            // take care of bookkeeping without a deliverable target.
            if self.arena.get(id).is_some() {
                // Direct: no bubble through dormant ancestors, no overlay
                // dismiss side-effects — this is a teardown signal, not a
                // user-driven focus move.
                self.dispatch_to_widget_direct(
                    id,
                    &crate::event::WidgetEvent::FocusLost,
                    &mut *ops,
                );
            }
            self.set_focused(None);
            self.focus_origin = None;
            self.update_focus_within_signals(old, None);
            self.update_view_focus_signals(old, None);
            self.a11y_dirty = true;
        }
        if self.focused.is_none() {
            self.focus_origin = None;
        }
        if let Some(id) = self.hovered_id()
            && !self.arena.is_active(id)
        {
            let old = self.hovered_id();
            self.set_hovered(None);
            self.update_hover_within_signals(old, None);
        }
        // Pointer capture anchored at a destroyed widget would otherwise
        // swallow every subsequent Move/Up — dispatch_to_widget rejects
        // inactive targets. Drop the capture so events resume normal
        // hit-test dispatch. Same for any in-flight drag session whose
        // source was torn down: the user sees the drag "stick".
        self.pointers.retain_active(&self.arena);
        // External (OS) drags have no in-app source widget, so they are never
        // torn down by source destruction — only internal drags are salvaged.
        let source_gone = self
            .active_drag
            .as_ref()
            .and_then(|s| s.source_widget)
            .is_some_and(|sw| !self.arena.is_active(sw));
        if source_gone {
            // `cancel_active_drag` fires on_drag_leave on the current
            // target before cleanup — the same contract as Escape.
            self.cancel_active_drag(&mut *ops);
        } else {
            // The drag *source* survived but its current hover **target** was
            // torn down by the rebuild (e.g. a side disabled / collapsed
            // mid-drag destroyed the panel under the pointer). Clear the stale
            // target id so a subsequent drop doesn't resolve to a destroyed
            // widget (and silently vanish); the next move — or the drop's own
            // re-hit-test — re-engages a live target.
            let stale_target = self
                .active_drag
                .as_ref()
                .and_then(|d| d.current_target)
                .is_some_and(|t| !self.arena.is_active(t));
            if stale_target && let Some(drag) = self.active_drag.as_mut() {
                drag.current_target = None;
            }
        }
    }

    /// Rebuild a single composite widget: destroy old children, re-run `build()`,
    /// and wire up new children. Called from `process_state_changes()` when a
    /// binding at `BindingLevel::Rebuild` fires (data-driven rebuild). Theme
    /// and locale changes do **not** rebuild — they update reactive signals
    /// that widgets bind to via `theme_signal()` / `locale_signal()`.
    /// Test-only: force-mark a widget for rebuild on the next layout
    /// pass. Lets regression tests exercise the rebuild path without
    /// needing to trip a Signal binding. Exposed cross-crate (not
    /// `#[cfg(test)]`-gated) so widget-crate tests in `teksilo-widgets`
    /// and elsewhere can also drive rebuilds; the `_for_testing`
    /// suffix marks it as not intended for application code.
    pub fn arena_mark_needs_rebuild_for_testing(&mut self, id: WidgetId) {
        self.arena.mark_needs_rebuild(id);
    }

    /// Force a [`DeferredSubtree`](crate::deferred_subtree::DeferredSubtree) at
    /// `id` to build its content now. A no-op for any other widget.
    ///
    /// The framework's own door into deferred content, for the case where the
    /// decision to show is the tree's rather than a widget's: a tooltip whose
    /// dwell has just matured has no open signal anyone could have handed over.
    pub(crate) fn materialize_deferred(&mut self, id: WidgetId) {
        let forced = self
            .arena
            .get_mut(id)
            .and_then(|n| n.widget.as_any_mut())
            .and_then(|any| any.downcast_mut::<crate::deferred_subtree::DeferredSubtree>())
            .map(|deferred| {
                let needed = !deferred.is_materialized();
                deferred.force();
                needed
            })
            .unwrap_or(false);
        if forced {
            self.rebuild_single_widget(id);
        }
    }

    pub(crate) fn rebuild_single_widget(&mut self, widget_id: WidgetId) {
        // Per §9.4.5, drop the source handle first (stops further source-side
        // dispatch) and then remove the UI-side callback. Either order gives
        // the same user-visible outcome for events that get posted between
        // the two steps, but dropping the source handle first stops the
        // publisher thread's work sooner.
        //
        // ⚠ That reasoning is about the two steps below, and it used to be
        // read as covering the whole problem. It does not. The dangerous gap
        // is not the microseconds between these two lines, it is the whole
        // span from *publish* to *dispatch*: a backend event is posted with
        // the id its publisher captured and is handled by the UI thread
        // frames later, so any rebuild in between used to strand it. The ids
        // are therefore carried across into the new build (see
        // `BuildContext::reusable_sub_ids`) rather than being retired here.
        // Skribisto's Analysis pane hit this every time it was the restored
        // view at project open: it starts a long operation in `build()` and
        // sets its own `Rebuild`-bound state signal, the operation finished
        // inside its own rebuild, and the pane sat on "Reading the
        // manuscript…" for the rest of the session with nothing logged.
        // Cancel any looping/one-shot animations owned by this widget
        // before build() runs. Without this, a widget that creates a
        // fresh `animated_signal` in build() would leak the previous
        // instance's scheduler entry: the old Signal<f32> clone lives
        // in `animations` forever, ticking against an orphaned signal
        // (silent CPU waste) and, for looping animations, doubling up
        // when the new one registers.
        self.animation_scheduler.cancel_by_widget(widget_id);
        // Same pattern for shader-driven animated quads: free the
        // widget's slot(s) so `build()` can allocate fresh handles.
        // The old cached_paint (if any) carries stale slot indices —
        // clear it so paint() re-runs and re-emits DrawCommands with
        // the newly-allocated slot.
        self.animated_quads.cancel_by_widget(widget_id);

        let drained_subs = if let Some(node) = self.arena.get_mut(widget_id) {
            node.effect_handles.clear();
            node.actions.clear();
            node.dirty.needs_rebuild = false;
            node.cached_paint = None;
            node.dirty.needs_paint = true;
            // Reset only the OWN handler bucket so `apply_self_handlers`
            // during this build's fresh build() starts from empty and
            // doesn't stack N-fold handler chains across rebuilds.
            // `external_handlers` — set by the `WidgetBuilder` chain at
            // creation time or by a composing parent's
            // `apply_handlers(child_id, ...)` — persists: those handlers
            // come from outside the widget and aren't re-emitted by its
            // own `build()`.
            //
            // `node_focusable` / `node_tab_index` / `node_cursor` /
            // `clips_children` / `context_menu_factory` are simple
            // values, not accumulating closures. Leave them alone —
            // apply_self_handlers rewrites them if the new build
            // specifies non-None values; otherwise values from the
            // creation site survive the rebuild.
            node.handlers = crate::event_handlers::EventHandlers::new();
            std::mem::take(&mut node.subscription_handles)
        } else {
            Vec::new()
        };
        // Rebuild wiped the OWN handler bucket above (the gesture arena
        // lived there), so the widget no longer owns any recognizers.
        // The next pointer hit re-runs `ensure_gesture_arena` and
        // re-inserts if the new build still wires gesture handlers.
        // External handlers (set via the builder chain at creation time)
        // persist, but `external_handlers` never carries a gesture arena
        // directly — it's always built by `ensure_gesture_arena` into
        // the OWN bucket.
        self.gesture_owners.remove(&widget_id);
        // Shortcuts the widget declared are torn down too — they will
        // be re-registered during the upcoming `build()` call. User
        // overrides live in a separate map keyed by id, so user
        // rebindings survive this round-trip (see ShortcutRegistry
        // graveyard semantics).
        self.shortcut_registry.unregister_all_for_owner(widget_id);
        self.global_actions.retain(|(owner, _)| *owner != widget_id);
        self.text_surfaces.remove(widget_id);
        // Re-apply `Widget::declare_shortcuts` so the static metadata
        // survives the rebuild (the unregister above wiped both
        // declared and build-registered entries; build() will refill
        // the handler-bearing ones, but it can't be relied on to
        // refill the metadata-only declarations).
        self.apply_declared_shortcuts(widget_id);
        // Drop any signal→widget bindings from the previous build
        // cycle so `build()` can re-register a fresh set without
        // accumulating duplicates across rebuilds.
        self.binding_registry.unregister_for_widget(widget_id);
        // Kept, in order, and handed to the upcoming `build()` so it re-subscribes under
        // the same ids. A subscription's id is what a publisher captured and posted with;
        // minting new ones here would leave every event already queued for this widget
        // naming an id nothing answers to. See `BuildContext::reusable_sub_ids`.
        let mut reusable_sub_ids = Vec::with_capacity(drained_subs.len());
        for (sub_id, handle) in drained_subs {
            drop(handle);
            self.app_context
                .subscription_callbacks
                .borrow_mut()
                .remove(&sub_id);
            self.app_context
                .subscription_ctx_callbacks
                .borrow_mut()
                .remove(&sub_id);
            reusable_sub_ids.push(sub_id);
        }

        // Decide how to treat the existing children. Two modes:
        //
        // * Default (`preserves_children_on_rebuild() == false`): the widget
        //   re-derives its whole subtree, so tear down every old child up
        //   front and let `build()` produce a fresh set.
        //
        // * Reconcile (`preserves_children_on_rebuild() == true`): the widget
        //   re-attaches the children it keeps (by id) and drops the rest. We
        //   snapshot the old children, run `build()`, then destroy only the
        //   old children the new build did NOT re-attach and did NOT re-parent
        //   elsewhere. Re-attached children keep their state (focus, scroll,
        //   text, subscriptions); dropped children are reaped rather than left
        //   as stranded, still-active orphans.
        let preserve_children = self
            .arena
            .get(widget_id)
            .map(|n| n.widget.preserves_children_on_rebuild())
            .unwrap_or(false);
        let old_children: Vec<WidgetId> = self.arena.children(widget_id).to_vec();
        if !preserve_children {
            for child_id in &old_children {
                self.destroy_subtree(*child_id);
            }
        }

        // The parentless nodes the *previous* build owned. Taken now so
        // `build()` records its new set into an empty list, and destroyed after
        // it returns — by then the widget's own fields point at the new nodes,
        // so tearing the old ones down cannot strand a live id in the widget.
        // Both the `preserve_children` reconcile and the plain path want this:
        // detached content is rebuilt wholesale either way (it is not addressed
        // by id from the outside, so there is nothing to preserve).
        let old_detached: Vec<WidgetId> = self
            .arena
            .get_mut(widget_id)
            .map(|node| std::mem::take(&mut node.detached))
            .unwrap_or_default();

        let mut widget_box = match self.arena.take_widget(widget_id) {
            Some(widget) => widget,
            None => return,
        };

        let mut build_ctx = crate::build_context::BuildContext {
            tree: self,
            composite_id: Some(widget_id),
            effect_handles: Vec::new(),
            subscription_handles: Vec::new(),
            reusable_sub_ids,
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

        // Reconcile the preserve path: reap any old child the new build
        // dropped (not in `new_children`) and did not re-parent elsewhere
        // (its `parent` still points here). Authoritative parent pointers mean
        // a kept subtree re-parented out of a dropped sibling survives. Runs
        // before `node.children` is overwritten so the destroy walk can't see
        // the new list. Re-parented survivors already have their new parent by
        // now (`ctx.add` builds nested widgets synchronously and re-homes their
        // children), so the `parent == widget_id` test correctly excludes them.
        if preserve_children {
            let new_set: std::collections::HashSet<WidgetId> =
                new_children.iter().copied().collect();
            for &old_c in &old_children {
                if !new_set.contains(&old_c) && self.arena.parent(old_c) == Some(widget_id) {
                    self.destroy_subtree_inner(old_c, true);
                }
            }
        }

        if let Some(node) = self.arena.get_mut(widget_id) {
            node.children = new_children;
            node.effect_handles = effect_handles;
            node.subscription_handles = subscription_handles;
        }

        // Reap the previous build's parentless content, now that the fresh set
        // is recorded and the widget points at it.
        for id in old_detached {
            self.destroy_subtree_inner(id, false);
        }
    }

    /// Record that `owner` created and owns the parentless node `detached` —
    /// pre-built overlay content that is deliberately not a child. See
    /// [`BuildContext::add_detached`](crate::build_context::BuildContext::add_detached)
    /// and [`WidgetNode::detached`](crate::arena::WidgetNode).
    pub(crate) fn record_detached(&mut self, owner: WidgetId, detached: WidgetId) {
        if let Some(node) = self.arena.get_mut(owner) {
            node.detached.push(detached);
        }
    }

    /// Destroy every parentless node `owner` owns, and forget them.
    ///
    /// Taken out of the node first: the destroy walk below can re-enter this
    /// function (a detached node may own detached nodes of its own — a rich
    /// tooltip's cascade children each pre-build their own), and it must not
    /// see a list it is halfway through consuming.
    fn destroy_detached_of(&mut self, owner: WidgetId) {
        let detached = self
            .arena
            .get_mut(owner)
            .map(|node| std::mem::take(&mut node.detached))
            .unwrap_or_default();
        for id in detached {
            self.destroy_subtree_inner(id, false);
        }
    }

    /// Recursively destroy a subtree, dropping per-widget subscription
    /// handles and removing their UI-side callbacks. Use this in place of
    /// `arena.destroy()` whenever a widget that may have subscribed to
    /// events is being torn down.
    pub(crate) fn destroy_subtree(&mut self, widget_id: WidgetId) {
        self.destroy_subtree_inner(widget_id, false);
    }

    /// Shared teardown for [`destroy_subtree`](Self::destroy_subtree) and the
    /// reconciling rebuild path. When `reparent_aware` is `true`, recursion
    /// descends into a child only if that child's `parent` still points at
    /// `widget_id`.
    ///
    /// A reconciling rebuild (a [`preserves_children_on_rebuild`] widget) may
    /// re-parent a kept subtree *out* of a dropped sibling and *into* the new
    /// tree. The dropped sibling's `children` list still lists that subtree
    /// (stale), so following it would tear down a node that is actually alive
    /// elsewhere. Following the authoritative `parent` pointer instead stops
    /// at the boundary of what genuinely still belongs to the node being
    /// destroyed. The per-node teardown ends with `arena.remove_node` (a
    /// single-node removal), NOT `arena.destroy` (which would re-recurse the
    /// stale `children` list and undo the skip).
    ///
    /// [`preserves_children_on_rebuild`]: crate::widget::Widget::preserves_children_on_rebuild
    fn destroy_subtree_inner(&mut self, widget_id: WidgetId, reparent_aware: bool) {
        // See the matching cancel in `rebuild_single_widget` — the
        // scheduler holds strong Signal<f32> clones, so the animation
        // would outlive its widget without this explicit cancellation.
        self.animation_scheduler.cancel_by_widget(widget_id);
        // Release the animated-quad slot(s) too.
        self.animated_quads.cancel_by_widget(widget_id);
        // A tooltip's content widget is parentless (`ctx.add`), so the child
        // walk below never reaches it — reap it explicitly or the entry and
        // its node outlive the anchor for the lifetime of the tree.
        self.retire_tooltips_of_destroyed_anchor(widget_id);
        // Same reasoning, one level up: every *other* parentless node this
        // widget built (a dropdown menu, a calendar, a tooltip's nested
        // cascade children) is unreachable from the child walk and dies here
        // or never.
        self.destroy_detached_of(widget_id);

        let children: Vec<WidgetId> = self.arena.children(widget_id).to_vec();
        for child in children {
            if reparent_aware && self.arena.parent(child) != Some(widget_id) {
                // Re-parented into the surviving tree by this rebuild — leave it.
                continue;
            }
            self.destroy_subtree_inner(child, reparent_aware);
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
            self.app_context
                .subscription_ctx_callbacks
                .borrow_mut()
                .remove(&sub_id);
        }
        // Drop any shortcuts the destroyed widget owned. Unlike
        // `rebuild_single_widget`, destruction is permanent; if the
        // user had overrides, they stay in the graveyard.
        self.shortcut_registry.unregister_all_for_owner(widget_id);
        self.global_actions.retain(|(owner, _)| *owner != widget_id);
        self.text_surfaces.remove(widget_id);
        // Bindings from this widget stop being relevant; clean them
        // up so the registry doesn't leak dead entries for the
        // lifetime of the app.
        self.binding_registry.unregister_for_widget(widget_id);
        // Keep `gesture_owners` honest — destroying the widget tears
        // down its handlers, so the per-frame gesture pass must stop
        // visiting it.
        self.gesture_owners.remove(&widget_id);
        // If focus pointed at the widget about to disappear, drop it
        // so later dispatch doesn't anchor intent walks at a dead id
        // (which would silently swallow the intent).
        if self.focused == Some(widget_id) {
            let old = self.focused;
            self.set_focused(None);
            self.focus_origin = None;
            self.update_focus_within_signals(old, None);
            self.update_view_focus_signals(old, None);
        }
        if self.hovered_id() == Some(widget_id) {
            let old = self.hovered_id();
            self.set_hovered(None);
            self.update_hover_within_signals(old, None);
        }
        // Symmetric with focus/hover above: a pointer capture anchored at the
        // widget about to disappear would otherwise swallow every subsequent
        // Move/Up (dispatch rejects inactive targets) until the next layout
        // pass runs `revalidate_interaction_state`. Drop it eagerly so capture
        // never outlives its owner, even when a destroy happens mid-gesture.
        self.pointers.release_captures_of(widget_id);
        // Single-node removal: this function already recursed into the
        // children above (honouring re-parenting when `reparent_aware`).
        // `arena.destroy` would re-recurse the now-stale `children` list and
        // tear down a survivor re-homed out of this subtree.
        self.arena.remove_node(widget_id);
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
    /// Called by `teksilo-app` after querying the platform layer. Updates the
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
            // The OS factor feeds the effective text scale (multiplied with the
            // user factor), so refresh the cached scaled typography.
            self.recompute_effective_theme();
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

    /// Report whether the *operating system* says an assistive technology is
    /// reading the screen.
    ///
    /// Fed by `teksilo-app` from `teksilo_platform::AccessibilityPreferences`,
    /// which asks Windows for `SPI_GETSCREENREADER`, AT-SPI for
    /// `org.a11y.Status.ScreenReaderEnabled`, and macOS for
    /// `NSWorkspace::isVoiceOverEnabled`. A platform that cannot answer leaves
    /// it [`ScreenReaderState::Unknown`], which behaves as "no".
    ///
    /// [`ScreenReaderState::Unknown`]: crate::environment::ScreenReaderState::Unknown
    ///
    /// **Not** AccessKit activation. An AccessKit adapter activates for
    /// anything that walks the tree — a screen magnifier, a voice-control front
    /// end, a UI-automation inspector, a tree browser like Accerciser — none of
    /// which want a touch to become a probe. Treating activation as evidence of
    /// a screen reader is how explore-by-touch gets switched on under an
    /// inspector and makes the app untouchable.
    ///
    /// [`ScreenReaderState`]: crate::environment::ScreenReaderState
    pub fn set_screen_reader_state(&mut self, state: crate::environment::ScreenReaderState) {
        self.screen_reader = state;
    }

    /// The screen-reader state most recently reported by the platform.
    ///
    /// [`ScreenReaderState::Unknown`] until something reports one.
    ///
    /// [`ScreenReaderState::Unknown`]: crate::environment::ScreenReaderState::Unknown
    pub fn screen_reader_state(&self) -> crate::environment::ScreenReaderState {
        self.screen_reader
    }

    /// Choose how explore-by-touch is decided for this window.
    ///
    /// [`ExploreByTouch::Off`] is the default and today's behaviour;
    /// [`ExploreByTouch::Auto`] follows [`Self::screen_reader_state`];
    /// [`ExploreByTouch::On`] forces it regardless. Read the resolved answer
    /// with [`Self::explore_by_touch_active`].
    ///
    /// [`ExploreByTouch::Off`]: crate::environment::ExploreByTouch::Off
    /// [`ExploreByTouch::Auto`]: crate::environment::ExploreByTouch::Auto
    /// [`ExploreByTouch::On`]: crate::environment::ExploreByTouch::On
    pub fn set_explore_by_touch(&mut self, mode: crate::environment::ExploreByTouch) {
        self.explore_by_touch = mode;
    }

    /// The explore-by-touch policy most recently set.
    pub fn explore_by_touch(&self) -> crate::environment::ExploreByTouch {
        self.explore_by_touch
    }

    /// Whether explore-by-touch is in force right now.
    ///
    /// `On` is unconditional; `Auto` requires the platform to have reported an
    /// active screen reader; `Off` is never in force. Nothing in the framework
    /// consumes this yet — the touch-as-probe interaction it gates has no
    /// owner — so it is a supply, a policy and a query, and no pointer path
    /// branches on it.
    pub fn explore_by_touch_active(&self) -> bool {
        use crate::environment::{ExploreByTouch, ScreenReaderState};
        match self.explore_by_touch {
            ExploreByTouch::Off => false,
            ExploreByTouch::On => true,
            ExploreByTouch::Auto => self.screen_reader == ScreenReaderState::Active,
        }
    }

    /// Report whether an AccessKit client is attached to this window.
    ///
    /// Written by `teksilo-app` from the platform adapter's activation and
    /// deactivation handlers, and used **asymmetrically** on purpose:
    ///
    /// * Attaching proves nothing. Magnifier, Voice Access and a UI-automation
    ///   inspector all activate the adapter, so this never sets
    ///   [`ScreenReaderState::Active`].
    /// * Detaching proves something. When the last client goes away there is
    ///   no screen reader either, so a `true` → `false` transition forces
    ///   [`ScreenReaderState::Inactive`] and an
    ///   [`ExploreByTouch::Auto`](crate::environment::ExploreByTouch::Auto)
    ///   window stops exploring immediately, without waiting for the next OS
    ///   query. A later OS query is free to say `Active` again.
    ///
    /// [`ScreenReaderState::Active`]: crate::environment::ScreenReaderState::Active
    /// [`ScreenReaderState::Inactive`]: crate::environment::ScreenReaderState::Inactive
    pub fn set_at_client_attached(&mut self, attached: bool) {
        if self.at_client_attached && !attached {
            self.screen_reader = crate::environment::ScreenReaderState::Inactive;
        }
        self.at_client_attached = attached;
    }

    /// Whether an AccessKit client is currently attached to this window.
    pub fn at_client_attached(&self) -> bool {
        self.at_client_attached
    }

    /// Set the host window HiDPI device scale (physical px per logical px).
    /// Written by `teksilo-app` when the window is created and again on
    /// `WindowEvent::ScaleFactorChanged`. Surfaced to widgets via
    /// `LayoutContext::scale_factor`.
    ///
    /// Layout needs no dirty-marking here: it rides the layout pass that
    /// follows, and a scale change already triggers a relayout. The
    /// accessibility tree does, because the scale is the root node's
    /// transform (AccessKit wants physical coordinates, the tree emits
    /// logical ones) and a plain relayout does not invalidate the AT cache —
    /// so dragging a window between a 1x and a 2x monitor would otherwise
    /// leave every reported rectangle at the old display's scale.
    pub fn set_device_scale_factor(&mut self, scale_factor: f32) {
        if self.device_scale_factor != scale_factor {
            self.device_scale_factor = scale_factor;
            self.a11y_dirty = true;
        }
    }

    /// The host window HiDPI device scale most recently set (1.0 by default).
    pub fn device_scale_factor(&self) -> f32 {
        self.device_scale_factor
    }

    /// Report the host window's platform safe-area insets — the region the
    /// window owns but a person cannot fully see or touch (a display cutout, a
    /// rounded corner, a home indicator).
    ///
    /// Fed by `teksilo-app` from
    /// `teksilo_platform::safe_area`, which reads the window on macOS and
    /// answers `ZERO` everywhere else because no other desktop platform
    /// reports one. Overlays clamp into what is left; the root layout
    /// proposal is deliberately **not** shrunk — a safe area moves what floats
    /// over the content, not the content.
    pub fn set_safe_area(&mut self, insets: teksilo_canvas::EdgeInsets) {
        if self.safe_area != insets {
            self.safe_area = insets;
            // Overlay placement is recomputed from scratch by every layout
            // pass, so the change reaches the screen as soon as one runs; the
            // frame request is what guarantees one does.
            self.request_frame();
        }
    }

    /// The safe-area insets most recently set (`ZERO` by default).
    pub fn safe_area(&self) -> teksilo_canvas::EdgeInsets {
        self.safe_area
    }

    /// Report a rectangle of the window currently covered from outside the
    /// tree — a soft keyboard, a platform IME candidate window — in
    /// window-logical pixels, or `None` when nothing covers it.
    ///
    /// A **rectangle**, not a named edge, because that is what a platform
    /// reports and because the placement code resolves it by keeping the
    /// largest free slab rather than by insetting an edge: a keyboard at the
    /// bottom gives the band above it, a candidate window at a side gives the
    /// band beside it, and neither needs the platform to say which edge it
    /// came from.
    ///
    /// Scope: like the safe area, this reaches **overlay placement only**. The
    /// root layout proposal keeps the whole window, so a scroll container
    /// still extends behind the keyboard and nothing reflows when one rises —
    /// which is what the desktop convention wants, and what keeps a keyboard
    /// appearing from being a full relayout of the document. Bringing a
    /// focused field out from behind the band is a scroll, against
    /// [`usable_viewport`](Self::usable_viewport), not a resize.
    pub fn set_occluded_inset(&mut self, occluded: Option<Rect>) {
        if self.occluded_inset != occluded {
            self.occluded_inset = occluded;
            self.request_frame();
        }
    }

    /// The occluding rectangle most recently set (`None` by default).
    pub fn occluded_inset(&self) -> Option<Rect> {
        self.occluded_inset
    }

    /// The viewport overlays are placed into: the last laid-out window size,
    /// less the safe area, less anything covering it.
    ///
    /// The same rectangle `position_overlays` clamps into, exposed so a
    /// consumer that must put something *in front of* a keyboard — a
    /// scroll-into-view for the focused field — can ask for it rather than
    /// re-deriving it.
    pub fn usable_viewport(&self) -> Rect {
        let proposal = self.last_proposal();
        let size = teksilo_canvas::Size::new(
            proposal.width.unwrap_or(0.0),
            proposal.height.unwrap_or(0.0),
        );
        self.overlay_viewport_for(size)
            .usable(self.layout_direction)
    }

    /// Build the overlay viewport for a window of `size`, folding in the
    /// safe area and the occluding rectangle.
    pub(crate) fn overlay_viewport_for(
        &self,
        size: teksilo_canvas::Size,
    ) -> crate::overlay::OverlayViewport {
        crate::overlay::OverlayViewport::new(size)
            .with_safe_area(self.safe_area)
            .with_occluded(self.occluded_inset)
    }

    /// Ask the platform to show (`true`) or hide (`false`) its on-screen
    /// keyboard.
    ///
    /// Recorded here rather than pushed straight at the window because the one
    /// thing the request must not do is re-assert IME allowance while a
    /// composition is live, and the IME-allowance state lives in the app
    /// layer's per-window reconcile. `teksilo-app` takes the request once per
    /// dispatch, after that reconcile, and applies it against the platform's
    /// [`SoftKeyboardSupport`](crate::window::SoftKeyboardSupport) answer.
    pub fn request_soft_keyboard(&mut self, visible: bool) {
        self.soft_keyboard_request = Some(visible);
    }

    /// Take the pending soft-keyboard request, if any. Called by the app layer
    /// once per dispatch.
    pub fn take_soft_keyboard_request(&mut self) -> Option<bool> {
        self.soft_keyboard_request.take()
    }

    /// Mark a widget as clipping its children to its bounds (scroll areas).
    pub fn set_clips_children(&mut self, id: WidgetId, clips: bool) {
        self.arena.set_clips_children(id, clips);
    }

    /// Apply a `HandlerSet` to an existing node in the arena, routed
    /// into the rebuild-cleared `handlers` slot (the widget's own
    /// self-applied handlers).
    /// Register any *bound* builder-level accessibility Props
    /// (`access_hidden` / `access_label` / `access_description` /
    /// `access_value`) at `BindingLevel::AccessibilityOnly`, so that a change
    /// to the underlying signal flips `a11y_dirty` and the AccessKit tree
    /// re-walks — re-resolving the announced hidden-state / name / description
    /// / value — without a visual relayout. Static Props are ignored by
    /// `register_if_bound`. Takes the registry explicitly (rather than `&self`)
    /// so insertion-path callers can keep a disjoint `&mut self.arena` borrow
    /// on the node alive.
    fn register_access_prop_bindings(
        access: &crate::widget_builder::AccessibilityOverrides,
        id: WidgetId,
        registry: &crate::binding::BindingRegistry,
    ) {
        use crate::binding::BindingLevel::AccessibilityOnly;
        if let Some(p) = access.hidden.as_ref() {
            p.register_if_bound(id, registry, AccessibilityOnly);
        }
        if let Some(p) = access.label.as_ref() {
            p.register_if_bound(id, registry, AccessibilityOnly);
        }
        if let Some(p) = access.description.as_ref() {
            p.register_if_bound(id, registry, AccessibilityOnly);
        }
        if let Some(p) = access.value.as_ref() {
            p.register_if_bound(id, registry, AccessibilityOnly);
        }
    }

    pub(crate) fn apply_self_handler_set(
        &mut self,
        id: WidgetId,
        mut handler_set: crate::widget_builder::HandlerSet,
    ) {
        // `visible_when` needs the binding registry (which the arena lacks), so
        // pull it out here and apply it via `self.visible_when` after.
        let visible_when = handler_set.visible_when.take();
        // Same reason for the reactive access Props: the arena can't reach the
        // registry, so register them here before handing the set to the arena.
        if let Some(access) = handler_set.access.as_ref() {
            Self::register_access_prop_bindings(access, id, &self.binding_registry);
        }
        self.arena
            .apply_handler_set(id, handler_set, crate::arena::HandlerScope::Own);
        if let Some(prop) = visible_when {
            self.visible_when(id, prop);
        }
    }

    /// Apply a `HandlerSet` to an existing node as *external* handlers —
    /// the kind attached by a composing parent via
    /// `BuildContext::apply_handlers(child_id, ...)` or by the
    /// `WidgetBuilder` chain at insertion time. These persist across
    /// the target widget's own rebuilds.
    pub(crate) fn apply_external_handler_set(
        &mut self,
        id: WidgetId,
        mut handler_set: crate::widget_builder::HandlerSet,
    ) {
        // See `apply_self_handler_set`: route `visible_when` and the reactive
        // access Props through the registry (the arena can't reach it).
        let visible_when = handler_set.visible_when.take();
        if let Some(access) = handler_set.access.as_ref() {
            Self::register_access_prop_bindings(access, id, &self.binding_registry);
        }
        self.arena
            .apply_handler_set(id, handler_set, crate::arena::HandlerScope::External);
        if let Some(prop) = visible_when {
            self.visible_when(id, prop);
        }
    }

    /// Append an accessibility `labelled_by` relation onto an already-mounted
    /// node, *preserving* any overrides the widget already carries — unlike
    /// `apply_external_handler_set`, which replaces the whole override struct.
    /// Used by container widgets (e.g. `FormLayout`) to name a field after its
    /// label once both ids are known. Idempotent: re-adding the same target is
    /// a no-op, so a composite may re-register the relation on every rebuild.
    pub(crate) fn push_access_labelled_by(&mut self, id: WidgetId, label_id: WidgetId) {
        if let Some(node) = self.arena.get_mut(id) {
            let overrides = node.access_overrides.get_or_insert_with(|| {
                Box::new(crate::widget_builder::AccessibilityOverrides::default())
            });
            // A composite that names itself from its content re-registers the
            // relation on every rebuild. Appending blindly would concatenate the
            // title into the name once per rebuild.
            if overrides.labelled_by.contains(&label_id) {
                return;
            }
            overrides.labelled_by.push(label_id);
            self.a11y_dirty = true;
        }
    }

    /// Append an accessibility `described_by` relation onto an already-mounted
    /// node, preserving existing overrides (the `described_by` counterpart of
    /// [`push_access_labelled_by`](Self::push_access_labelled_by)).
    pub(crate) fn push_access_described_by(&mut self, id: WidgetId, target_id: WidgetId) {
        if let Some(node) = self.arena.get_mut(id) {
            let overrides = node.access_overrides.get_or_insert_with(|| {
                Box::new(crate::widget_builder::AccessibilityOverrides::default())
            });
            if overrides.described_by.contains(&target_id) {
                return;
            }
            overrides.described_by.push(target_id);
            self.a11y_dirty = true;
        }
    }

    /// Set a per-child alignment override on a widget.
    pub fn set_alignment(&mut self, id: WidgetId, alignment: teksilo_tokens::Alignment) {
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
    /// node. Invoked by `BuildContext::register_action`; not meant
    /// to be called directly.
    /// Record that `widget_id` edits text. Replaces any previous registration
    /// from the same widget, so a rebuild re-points rather than accumulating.
    pub(crate) fn push_text_surface(
        &mut self,
        widget_id: WidgetId,
        surface: std::rc::Rc<dyn crate::text_surface::TextSurface>,
    ) {
        self.text_surfaces.insert(widget_id, surface);
    }

    /// A cloneable view of this tree's text surfaces, for a caller that must ask
    /// the question later, without a `&WidgetTree` in hand.
    pub fn text_surfaces(&self) -> crate::text_surface::TextSurfaces {
        self.text_surfaces.clone()
    }

    /// The text-editing widget that currently holds the keyboard focus.
    ///
    /// `None` when focus is elsewhere — or nowhere — which is exactly what a
    /// host needs in order to know that a text chord is safe to route itself.
    pub fn focused_text_surface(
        &self,
    ) -> Option<std::rc::Rc<dyn crate::text_surface::TextSurface>> {
        self.text_surfaces.focused()
    }

    /// Is the keyboard focus inside a widget that edits text?
    ///
    /// The cheap half of [`focused_text_surface`](Self::focused_text_surface),
    /// for a host that only needs to decide whether to step aside.
    pub fn focused_is_text_surface(&self) -> bool {
        self.text_surfaces.focused_is_text_surface()
    }

    pub(crate) fn push_action(&mut self, widget_id: WidgetId, action: crate::action::Action) {
        if let Some(node) = self.arena.get_mut(widget_id) {
            node.actions.push(action);
        }
    }

    /// Telemetry dispatch tap. Looks up a registered
    /// [`crate::telemetry::TelemetryContext`] in `app_state` and emits
    /// an `intent.dispatched` event with the intent's name. No-op when
    /// no telemetry is configured. Errors and consent gating are
    /// handled inside the reporter — this site only needs to call
    /// `record`.
    fn tap_intent_dispatched(&self, intent: &crate::intent::Intent) {
        let Some(tcx) = self
            .app_context()
            .app_state::<crate::telemetry::TelemetryContext>()
        else {
            return;
        };
        let install_id = tcx.reporter.install_id();
        let props = [
            crate::telemetry::Prop {
                key: "name",
                value: crate::telemetry::PropValue::StaticStr(intent.name),
            },
            crate::telemetry::Prop {
                key: "source",
                value: crate::telemetry::PropValue::Enum {
                    variant: intent.source.as_str(),
                },
            },
        ];
        let event = crate::telemetry::Event {
            name: "intent.dispatched",
            category: crate::telemetry::EventCategory::Intent,
            timestamp: std::time::SystemTime::now(),
            install_id,
            session_id: &tcx.session_id,
            schema_version: tcx.schema_version,
            props: &props,
        };
        tcx.reporter.record(&event);
    }

    /// Histogram of widget concrete-type names across the active
    /// arena. Used by the `widget.census` telemetry emitter to surface
    /// "which widgets does this app actually use" data back to the
    /// framework. Keyed by
    /// `std::any::type_name::<T>()` of the concrete widget — a
    /// dotted, fully-qualified path like
    /// `teksilo_widgets::button::Button`.
    ///
    /// `&'static str` keys: `type_name_of_val` returns a
    /// compile-time string, so the histogram preserves the static
    /// lifetime all the way to the wire-format prop. This avoids
    /// any allocation for the type-name strings themselves.
    ///
    /// Cost: one `Box<dyn Widget>` indirection per active node plus
    /// a `HashMap` insert. Sub-millisecond on arenas with thousands
    /// of widgets. Safe to call every frame in tests; in production
    /// gate behind a periodic ticker (hourly or on-idle).
    pub fn widget_type_histogram(&self) -> std::collections::HashMap<&'static str, u32> {
        let mut out = std::collections::HashMap::<&'static str, u32>::new();
        for id in self.arena.active_ids_iter() {
            if let Some(node) = self.arena.get(id) {
                // `Widget::type_name` is monomorphized per impl, so
                // calling through the vtable correctly resolves to
                // the concrete type — `type_name_of_val(&*widget)`
                // alone would collapse to `"dyn teksilo_core::widget::Widget"`.
                let name: &'static str = node.widget.type_name();
                *out.entry(name).or_insert(0) += 1;
            }
        }
        out
    }

    /// Number of active widgets in the arena. Cheap; matches the
    /// totals returned by `widget_type_histogram` when summed.
    pub fn active_widget_count(&self) -> usize {
        self.arena.active_ids_iter().count()
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
    pub(crate) fn drain_pending_intents(&mut self, ops: &mut dyn crate::window::WindowOps) {
        while !self.pending_intents.is_empty() {
            let (source, intent, propagate) = self.pending_intents.remove(0);
            self.dispatch_intent(source, intent, propagate, &mut *ops);
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
        ops: &mut dyn crate::window::WindowOps,
    ) {
        // Telemetry tap. Single insertion point catches every intent
        // — shortcut-driven, programmatic via `send_intent`, etc. —
        // because every dispatch funnels through here. A no-op when
        // no `TelemetryContext` is registered.
        self.tap_intent_dispatched(&intent);

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
            let Some((mut action, idx, enabled)) = self.arena.get_mut(id).and_then(|node| {
                let idx = node.actions.iter().position(|a| a.intent == intent.name)?;
                let enabled = node.actions[idx].is_enabled();
                Some((node.actions.remove(idx), idx, enabled))
            }) else {
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

            let mut ctx = self.make_event_context(&mut *ops);
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

        // Fallback: window-global actions (registered via
        // `register_action_global`). The source→root walk found no consuming
        // node action, so consult app-global commands — reachable regardless of
        // where the intent originated (menu-bar overlay, content, shortcut).
        let mut i = 0;
        while i < self.global_actions.len() {
            let matches = {
                let (_, action) = &self.global_actions[i];
                action.intent == intent.name && action.is_enabled()
            };
            if !matches {
                i += 1;
                continue;
            }
            // Take the action out so the FnMut handler can run without holding a
            // borrow on `self`; reinsert at its slot afterwards.
            let (owner, mut action) = self.global_actions.remove(i);
            let mut ctx = self.make_event_context(&mut *ops);
            let response = (action.handler)(&intent, &mut ctx);
            self.global_actions.insert(i, (owner, action));
            self.collect_from_ctx(ctx, owner);
            match response {
                crate::intent::IntentResponse::Handled => return,
                crate::intent::IntentResponse::Propagated => {
                    i += 1;
                    continue;
                }
            }
        }
    }

    /// Register a window-global [`Action`](crate::action::Action) owned by
    /// `owner`. Consulted as a dispatch fallback (see [`Self::dispatch_intent`]);
    /// torn down when `owner` rebuilds or is destroyed. Backs
    /// [`BuildContext::register_action_global`](crate::BuildContext::register_action_global).
    pub(crate) fn push_global_action(&mut self, owner: WidgetId, action: crate::action::Action) {
        self.global_actions.push((owner, action));
    }

    // --- Window-close request (drained by the app loop) ---

    /// Drain the "close this window" flag set by
    /// [`EventContext::close_window`] during dispatch. A *guarded* close
    /// — the app routes it through the window's close guard.
    pub fn take_close_window_request(&mut self) -> bool {
        std::mem::replace(&mut self.close_window_requested, false)
    }

    /// Drain the "close this window, no questions asked" flag set by
    /// [`EventContext::close_window_forced`] during dispatch. An
    /// *unconditional* close that bypasses the window's close guard.
    pub fn take_force_close_request(&mut self) -> bool {
        std::mem::replace(&mut self.force_close_requested, false)
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

    /// Drain the pending theme switch raised by
    /// [`EventContext::set_theme`] during dispatch. The app layer
    /// (`WindowManager::drain_pending_theme_requests`) routes it through
    /// `WindowManager::set_theme` so the new theme is applied to every
    /// window, not just the one whose handler requested it.
    pub fn take_pending_theme_request(&mut self) -> Option<crate::styles::Theme> {
        self.pending_theme_request.take()
    }

    /// Drain the pending "follow OS theme" request raised by
    /// [`EventContext::follow_system_theme`] during dispatch. The app layer
    /// (`WindowManager::drain_pending_follow_system_requests`) switches to
    /// `ThemeMode::Native` and recomputes the theme from the OS for every
    /// window. Returns `true` if a request was pending.
    pub fn take_pending_follow_system_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_follow_system_request)
    }

    /// Drain the pending text-scale change raised by
    /// [`EventContext::set_text_scale`] during dispatch. The app layer
    /// (`WindowManager::drain_pending_text_scale_requests`) routes it through
    /// `WindowManager::set_text_scale` so the new factor is applied to every
    /// window, not just the one whose handler requested it.
    pub fn take_pending_text_scale_request(&mut self) -> Option<f32> {
        self.pending_text_scale_request.take()
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

    /// Walk `Widget::declare_shortcuts` for an already-inserted widget
    /// and register every returned shortcut with the registry, owned
    /// by `id`. Called at insertion AND at rebuild so the declared
    /// metadata survives across rebuilds (which `unregister_all_for_owner`
    /// would otherwise wipe). Build-time `ctx.register_shortcut` calls
    /// upsert handlers on top; the registry is idempotent on id.
    pub(crate) fn apply_declared_shortcuts(&mut self, id: WidgetId) {
        let declared = self
            .arena
            .get(id)
            .map(|n| n.widget.declare_shortcuts())
            .unwrap_or_default();
        for shortcut in declared {
            self.shortcut_registry.register_owned(shortcut, id);
        }
    }

    /// Internal: insert a widget, call build(), wire children, register clips.
    fn insert_widget(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        let id = self.arena.insert(widget);

        {
            if let Some(mut widget_box) = self.arena.take_widget(id) {
                if let Some(handler_set) = widget_box.take_handler_set() {
                    self.arena.restore_widget(id, widget_box);
                    if let Some(node) = self.arena.get_mut(id) {
                        // Handlers attached at the widget's creation site
                        // are external from its own perspective — keep
                        // them out of the rebuild-cleared `handlers`
                        // slot so they survive data-driven rebuilds.
                        node.external_handlers = handler_set.handlers;
                        node.node_focusable = handler_set.focusable;
                        node.node_tab_index = handler_set.tab_index;
                        node.node_cursor = handler_set.cursor;
                        // `clips_children` and `event_pass_through` are
                        // node-level flags on `WidgetNode` — they must
                        // be mirrored here too. Without this an
                        // `Inner::new().event_pass_through(true)` chain
                        // silently no-ops (the flag stays at default
                        // `false`), and any widget wrapped with it
                        // catches every pointer event in its bounds.
                        if let Some(clips) = handler_set.clips_children {
                            node.clips_children = clips;
                        }
                        if let Some(pass_through) = handler_set.event_pass_through {
                            node.event_pass_through = pass_through;
                        }
                        if let Some(dead_zone) = handler_set.gesture_dead_zone {
                            node.gesture_dead_zone = dead_zone;
                        }
                        if let Some(role) = handler_set.long_press_role {
                            node.long_press_role = role;
                        }
                        if let Some(action) = handler_set.touch_action {
                            node.touch_action = action;
                        }
                        if let Some(claim) = handler_set.pan_claim {
                            node.pan_claim = Some(claim);
                        }
                        if let Some(behavior) = handler_set.overscroll_behavior {
                            node.overscroll_behavior = behavior;
                        }
                        if let Some(activation) = handler_set.drag_activation {
                            node.drag_activation = activation;
                        }
                        if let Some(policy) = handler_set.multi_contact {
                            node.multi_contact = policy;
                        }
                        if let Some(keyboard_capture) = handler_set.keyboard_capture {
                            node.keyboard_capture = keyboard_capture;
                        }
                        if let Some(hit_transparent) = handler_set.hit_transparent {
                            node.hit_transparent = hit_transparent;
                        }
                        if let Some(slop) = handler_set.hit_slop {
                            node.hit_slop = Some(slop);
                        }
                        if let Some(no_slop) = handler_set.no_hit_slop {
                            node.no_hit_slop = no_slop;
                        }
                        if handler_set.context_menu_factory.is_some() {
                            node.context_menu_factory = handler_set.context_menu_factory;
                        }
                        if let Some(sig) = handler_set.focus_within {
                            node.focus_within_signal = Some(sig);
                        }
                        if let Some(sig) = handler_set.hover_within {
                            node.hover_within_signal = Some(sig);
                        }
                        // Builder-chained `visible_when: prop`. Mirror of
                        // `WidgetTree::visible_when`: register a bound prop at
                        // Relayout, then store it on the node. (Disjoint field
                        // borrow, like `access_hidden` below.)
                        if let Some(prop) = handler_set.visible_when {
                            prop.register_if_bound(
                                id,
                                &self.binding_registry,
                                crate::binding::BindingLevel::Relayout,
                            );
                            node.visible_state = Some(prop);
                        }
                        // Builder-level accessibility overrides + subtree
                        // mode. Mirrored here because this insertion path
                        // bypasses `apply_handler_set`.
                        if handler_set.access.is_some() {
                            // Register any bound access_hidden/label/description/
                            // value Props at AccessibilityOnly so the AT tree
                            // re-walks when they flip. (Disjoint field borrow:
                            // `node` borrows `self.arena`, this reads
                            // `self.binding_registry`.)
                            if let Some(access) = handler_set.access.as_ref() {
                                Self::register_access_prop_bindings(
                                    access,
                                    id,
                                    &self.binding_registry,
                                );
                            }
                            // Merged, not assigned: a node can already carry a
                            // block from its builder chain, and replacing it
                            // drops everything in it (see
                            // `AccessibilityOverrides::merge_from`).
                            match (&mut node.access_overrides, handler_set.access) {
                                (Some(existing), Some(incoming)) => existing.merge_from(*incoming),
                                (slot, incoming) => *slot = incoming,
                            }
                        }
                        if let Some(mode) = handler_set.access_subtree {
                            node.access_subtree = mode;
                        }
                    }
                } else {
                    self.arena.restore_widget(id, widget_box);
                }
            }
        }

        // Walk Widget::declare_shortcuts before build() so the
        // declared metadata lands in the registry first; if build()
        // also registers the same id with a real on_activate, the
        // registry upserts (preserving any user override).
        self.apply_declared_shortcuts(id);

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
                // A first mount has no previous build to inherit ids from.
                reusable_sub_ids: Vec::new(),
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

    /// The widget that paints `id`'s title, when it has one.
    ///
    /// See [`Widget::accessible_title_node`].
    pub fn widget_accessible_title_node(&self, id: WidgetId) -> Option<WidgetId> {
        self.arena.get(id)?.widget.accessible_title_node()
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
                        // Creation-site handlers are external (persist
                        // across the widget's own rebuilds) — see the
                        // matching block in `insert_widget`.
                        node.external_handlers = handler_set.handlers;
                        node.node_focusable = handler_set.focusable;
                        node.node_tab_index = handler_set.tab_index;
                        node.node_cursor = handler_set.cursor;
                        if let Some(clips) = handler_set.clips_children {
                            node.clips_children = clips;
                        }
                        if let Some(pass_through) = handler_set.event_pass_through {
                            node.event_pass_through = pass_through;
                        }
                        if let Some(dead_zone) = handler_set.gesture_dead_zone {
                            node.gesture_dead_zone = dead_zone;
                        }
                        if let Some(role) = handler_set.long_press_role {
                            node.long_press_role = role;
                        }
                        if let Some(action) = handler_set.touch_action {
                            node.touch_action = action;
                        }
                        if let Some(claim) = handler_set.pan_claim {
                            node.pan_claim = Some(claim);
                        }
                        if let Some(behavior) = handler_set.overscroll_behavior {
                            node.overscroll_behavior = behavior;
                        }
                        if let Some(activation) = handler_set.drag_activation {
                            node.drag_activation = activation;
                        }
                        if let Some(policy) = handler_set.multi_contact {
                            node.multi_contact = policy;
                        }
                        if let Some(keyboard_capture) = handler_set.keyboard_capture {
                            node.keyboard_capture = keyboard_capture;
                        }
                        if let Some(hit_transparent) = handler_set.hit_transparent {
                            node.hit_transparent = hit_transparent;
                        }
                        if let Some(slop) = handler_set.hit_slop {
                            node.hit_slop = Some(slop);
                        }
                        if let Some(no_slop) = handler_set.no_hit_slop {
                            node.no_hit_slop = no_slop;
                        }
                        if handler_set.context_menu_factory.is_some() {
                            node.context_menu_factory = handler_set.context_menu_factory;
                        }
                        if let Some(sig) = handler_set.focus_within {
                            node.focus_within_signal = Some(sig);
                        }
                        if let Some(sig) = handler_set.hover_within {
                            node.hover_within_signal = Some(sig);
                        }
                        // Builder-chained `visible_when: prop`. Same as in
                        // `insert_widget`.
                        if let Some(prop) = handler_set.visible_when {
                            prop.register_if_bound(
                                id,
                                &self.binding_registry,
                                crate::binding::BindingLevel::Relayout,
                            );
                            node.visible_state = Some(prop);
                        }
                        // Builder-level accessibility overrides + subtree
                        // mode. Same rationale as in `insert_widget`.
                        if handler_set.access.is_some() {
                            // Register any bound access_hidden/label/description/
                            // value Props at AccessibilityOnly so the AT tree
                            // re-walks when they flip. (Disjoint field borrow:
                            // `node` borrows `self.arena`, this reads
                            // `self.binding_registry`.)
                            if let Some(access) = handler_set.access.as_ref() {
                                Self::register_access_prop_bindings(
                                    access,
                                    id,
                                    &self.binding_registry,
                                );
                            }
                            // Merged, not assigned: a node can already carry a
                            // block from its builder chain, and replacing it
                            // drops everything in it (see
                            // `AccessibilityOverrides::merge_from`).
                            match (&mut node.access_overrides, handler_set.access) {
                                (Some(existing), Some(incoming)) => existing.merge_from(*incoming),
                                (slot, incoming) => *slot = incoming,
                            }
                        }
                        if let Some(mode) = handler_set.access_subtree {
                            node.access_subtree = mode;
                        }
                    }
                } else {
                    self.arena.restore_widget(id, widget_box);
                }
            }
        }

        // Same shortcut-declaration walk as `insert_widget` — keeps
        // metadata visible from the moment the child mounts, before
        // build() runs.
        self.apply_declared_shortcuts(id);

        {
            if let Some(mut widget_box) = self.arena.take_widget(id) {
                let mut build_ctx = crate::build_context::BuildContext {
                    tree: self,
                    composite_id: Some(id),
                    effect_handles: Vec::new(),
                    subscription_handles: Vec::new(),
                    // A first mount has no previous build to inherit ids from.
                    reusable_sub_ids: Vec::new(),
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

    /// Bind a widget's visibility to a boolean prop.
    /// When false, the widget is set dormant; when true, it is activated.
    /// Accepts `Signal<bool>`, `Prop<bool>`, or plain `bool`.
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

    /// Install (or reuse) the activation signal on a node and return a
    /// handle to it. The framework sets it to `false` when the node is
    /// parked dormant (`Switcher` / `visible_when`) and `true` when it is
    /// re-activated — see [`crate::arena::WidgetArena::set_dormant`] /
    /// [`activate`](crate::arena::WidgetArena::activate). The returned
    /// signal is initialised to the node's current active state. Used by
    /// widgets owning a resource outside the paint pass (a native subview)
    /// that must hide/show it in lockstep with framework activation.
    pub fn activation_signal(&mut self, id: WidgetId) -> crate::signal::Signal<bool> {
        if let Some(node) = self.arena.get_mut(id) {
            if let Some(existing) = node.activation_signal.clone() {
                return existing;
            }
            let active = node.activation == crate::arena::ActivationState::Active;
            let sig = crate::signal::Signal::new(active);
            node.activation_signal = Some(sig.clone());
            sig
        } else {
            // Node missing (shouldn't happen in build) — hand back a
            // detached signal so the caller still gets a valid handle.
            crate::signal::Signal::new(true)
        }
    }

    // -----------------------------------------------------------------
    // In-node target geometry
    // -----------------------------------------------------------------

    /// The interactive sub-regions the widget at `id` paints inside its own
    /// single node, in absolute arena coordinates.
    ///
    /// The read side of [`Widget::target_regions`]:
    /// a scroll bar's thumb, a slider's knob, a header cell's filter
    /// affordance. Empty for the overwhelming majority of widgets, whose node
    /// *is* their target and which therefore have nothing to add.
    ///
    /// Reporting only — reading this changes nothing. It exists so a
    /// conformance audit, and a test of one, can see geometry that no layout
    /// ever produced.
    pub fn widget_target_regions(&self, id: WidgetId) -> Vec<crate::partition::TargetRegion> {
        let Some(node) = self.arena.get(id) else {
            return Vec::new();
        };
        node.widget.target_regions(self.arena.bounds(id))
    }

    /// The hit outset the **mounted** widget at `id` declares for `kind`,
    /// against the tree's live input tokens.
    ///
    /// The read side of [`Widget::hit_outset`], and the companion of
    /// [`widget_target_regions`](Self::widget_target_regions): both let a
    /// conformance audit — and a test of one — see target geometry that no
    /// layout ever produced.
    ///
    /// It has to read the mounted node rather than a freshly-built widget,
    /// because an outset is usually derived from what the widget *painted*,
    /// and an unmounted one has painted nothing. That is also what makes the
    /// **gates** assertable: a decorative avatar, an inert twist arrow, a
    /// disabled swatch and a breadcrumb's current crumb all take no press, so
    /// each must declare `EdgeInsets::ZERO` — a widened node that then refuses
    /// the press is a hole punched in whatever is behind it.
    ///
    /// The tokens are the **effective** theme's, not `theme`'s, because that
    /// is what the hit path itself reads:
    /// `hit_test_for_excluding` builds its `HitContext` from
    /// `effective_theme.input`, as do the pointer profile and the touch-enabled
    /// gate in `pointer_state.rs`. The two themes agree only for as long as
    /// nothing between them touches `input` — `recompute_effective_theme`
    /// currently projects typography alone — and an accessor that describes a
    /// path has to read that path's source rather than one that happens to
    /// match it.
    ///
    /// Reporting only — reading this changes nothing.
    pub fn widget_hit_outset(
        &self,
        id: WidgetId,
        kind: teksilo_tokens::PointerKind,
    ) -> teksilo_canvas::EdgeInsets {
        let Some(node) = self.arena.get(id) else {
            return teksilo_canvas::EdgeInsets::ZERO;
        };
        node.widget.hit_outset(kind, &self.effective_theme.input)
    }

    // -----------------------------------------------------------------
    // Press state
    // -----------------------------------------------------------------

    /// Install (or reuse) the framework press signal on a node and return a
    /// handle to it.
    ///
    /// `true` while the node holds a pointer press whose visual is showing:
    /// between press and release, `false` once the pointer leaves the press's
    /// tap boundary and `true` again on re-entry, cleared on a cancel or when
    /// a peer wins the arbitration. See `docs/touch-and-pen.md` §7.
    pub fn pressed_signal(&mut self, id: WidgetId) -> crate::signal::Signal<bool> {
        let showing = self.is_pressed(id);
        let Some(node) = self.arena.get_mut(id) else {
            // Node missing (should not happen during build) — hand back a
            // detached signal so the caller still gets a valid handle.
            return crate::signal::Signal::new(false);
        };
        if let Some(existing) = node.pressed_signal.clone() {
            return existing;
        }
        let sig = crate::signal::Signal::new(showing);
        node.pressed_signal = Some(sig.clone());
        sig
    }

    /// The press held by the pointer being dispatched, as `(inside, pending)`,
    /// for [`EventContext`]'s per-dispatch
    /// snapshot. `None` when that pointer holds no press.
    pub(crate) fn current_press_snapshot(&self) -> Option<(bool, bool)> {
        self.presses
            .get(self.current_pointer_id())
            .map(|p| (p.inside, p.pending()))
    }

    /// The contact holding `id`'s press, whether or not its visual is showing.
    ///
    /// `None` for a node nothing is pressing. A node held by a finger whose
    /// press-feedback delay has not elapsed still answers with that finger:
    /// the press is real, only its visual is waiting.
    pub fn pressed_by(&self, id: WidgetId) -> Option<crate::pointer::PointerId> {
        self.presses.owner_of(id)
    }

    /// Whether `id`'s press visual is showing — held, inside its tap boundary,
    /// and past any press-feedback delay. What the node's
    /// [`pressed_signal`](Self::pressed_signal) mirrors.
    pub fn is_pressed(&self, id: WidgetId) -> bool {
        self.presses
            .for_node(id)
            .is_some_and(crate::press::Press::showing)
    }

    /// Whether `id` is held and the pointer has not left the press's tap
    /// boundary. True during a press-feedback delay, unlike
    /// [`is_pressed`](Self::is_pressed).
    pub fn press_is_inside(&self, id: WidgetId) -> bool {
        self.presses.for_node(id).is_some_and(|p| p.inside)
    }

    /// Whether `id` is held but its press-feedback delay has not elapsed, so
    /// the visual is deliberately withheld.
    pub fn press_pending(&self, id: WidgetId) -> bool {
        self.presses
            .for_node(id)
            .is_some_and(crate::press::Press::pending)
    }

    /// Publish `id`'s press signal from the table. The one place a press
    /// signal is written, so "the table changed" and "the recipe was told"
    /// cannot drift apart.
    pub(crate) fn publish_pressed(&mut self, id: WidgetId) {
        let showing = self.is_pressed(id);
        if let Some(node) = self.arena.get(id)
            && let Some(sig) = node.pressed_signal.clone()
            && sig.get() != showing
        {
            sig.set(showing);
        }
    }

    /// Fire the `activation_signal` of every node that transitioned
    /// Active↔Dormant since the last flush. Called at a well-defined tree-level
    /// point (the end of `process_state_changes`), never from inside the arena
    /// recursion — so an observer (e.g. a `WebView` calling the engine's
    /// `set_visible`) runs after the visibility pass has fully committed,
    /// matching the `focus_within` / `hover_within` update discipline.
    pub(crate) fn flush_activation_signals(&mut self) {
        let changes = self.arena.take_activation_changes();
        for (id, _recorded) in changes {
            // Re-read the node at flush time; it still exists (the transition
            // was recorded in the same synchronous operation).
            let Some(node) = self.arena.get(id) else {
                continue;
            };
            let Some(sig) = node.activation_signal.clone() else {
                continue;
            };
            // Fire the node's **current** state, not the value recorded at the
            // transition, and only when it differs from what observers last
            // saw. `pending_activation_changes` is an append-only queue: a
            // node parked and re-activated inside one batch records both
            // edges, and replaying them in order hands observers a `false`
            // that was never observable — the node is already Active by the
            // time anyone is told anything.
            //
            // The in-tree modal path does exactly that on every open: build
            // the content, `set_dormant` it, mount the scrim, `activate` it,
            // then move focus in. Both edges land in one batch and flush
            // *after* the focus dispatch, so the stale `false` arrives last
            // and observers act on a state that has already been superseded.
            // For a text editor that meant its dormancy handler wiped the
            // `has_focus` it had just been granted, and the dialog opened with
            // no caret; a `WebView` would have taken a real `set_visible(false)`
            // OS call for a subview that never left the screen.
            //
            // Collapsing to the final state also makes the flush idempotent
            // over duplicate ids: the first iteration syncs the signal, the
            // rest find it already equal and skip. `Signal::set` notifies
            // unconditionally, so the equality guard is what stops the
            // redundant fanout.
            let active = node.activation == crate::arena::ActivationState::Active;
            if sig.get() != active {
                sig.set(active);
            }
        }
    }

    /// Bind an opacity multiplier (0..1) to a widget. The render walker
    /// emits `SetOpacity(value)` before painting the widget's subtree
    /// and `RestoreOpacity` afterwards, so the multiplier composes
    /// correctly with ancestor opacity scopes via the canvas's stacked
    /// opacity model. Bound at `Repaint` level: opacity changes never
    /// trigger relayout. Pass any `Prop<f32>` or `Signal<f32>` source
    /// (typically an animated signal driven by a `Fade` wrapper).
    pub fn set_opacity(&mut self, id: WidgetId, opacity: impl Into<crate::signal::Prop<f32>>) {
        let prop = opacity.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::RepaintOnly,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.opacity_prop = Some(prop);
        }
    }

    /// Bind a 2D affine transform to a widget. The render walker emits
    /// `PushTransform(value)` before painting the widget's subtree and
    /// `PopTransform` afterwards; the renderer composes the transform
    /// onto its stack so nested wrappers and widget-internal canvas
    /// transforms compose correctly. Bound at `Repaint` level: visual-
    /// only transforms never trigger relayout. Wrappers that want the
    /// transform's *value change* to also drive layout (e.g.
    /// `Scale::reflow(true)`) must additionally bind the *driver*
    /// signal to themselves at `Relayout` level — the transform prop
    /// itself stays at Repaint. Pass a `Transform2D`, `Signal<Transform2D>`,
    /// or `Prop<Transform2D>`.
    pub fn set_transform(
        &mut self,
        id: WidgetId,
        transform: impl Into<crate::signal::Prop<teksilo_canvas::Transform2D>>,
    ) {
        let prop = transform.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::RepaintOnly,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.transform_prop = Some(prop);
            // A plain transform is a *self* transform — clear any prior
            // content-transform marker so the flag can never go stale if a
            // node switches from `set_content_transform` to `set_transform`.
            node.content_transform = false;
        }
    }

    /// Like [`set_transform`](Self::set_transform), but marks the transform as
    /// a **content** transform: it positions the node's content within a fixed
    /// parent-space viewport (the node's bounds) rather than transforming the
    /// node itself. Hit-testing then keeps the whole viewport interactive at
    /// any pan / zoom. Used by `SceneView`; see
    /// `WidgetNode::content_transform`.
    pub fn set_content_transform(
        &mut self,
        id: WidgetId,
        transform: impl Into<crate::signal::Prop<teksilo_canvas::Transform2D>>,
    ) {
        let prop = transform.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::RepaintOnly,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.transform_prop = Some(prop);
            node.content_transform = true;
        }
    }

    /// Bind a Gaussian-equivalent blur radius to a widget. The render
    /// walker emits `BeginBlurredSubtree { bounds, radius }` before
    /// painting the widget's subtree and `EndBlurredSubtree` afterwards;
    /// the renderer redirects drawing into an intermediate texture, runs
    /// a dual-Kawase blur chain at the requested radius, and composites
    /// the blurred result back into the parent pass. Bound at `Repaint`
    /// level: blur radius changes never trigger relayout. Sub-perceptual
    /// radii (< 0.5) skip the Begin/End pair entirely so animated
    /// enable/disable patterns have zero per-frame cost when fully off.
    /// Pass any `Prop<f32>` or `Signal<f32>` source.
    pub fn set_blur(&mut self, id: WidgetId, radius: impl Into<crate::signal::Prop<f32>>) {
        let prop = radius.into();
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::RepaintOnly,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.blur_prop = Some(prop);
        }
    }

    /// Bind a widget's enabled state to a boolean prop or compatibility state binding.
    /// When false, the widget and its entire subtree ignore all events but remain
    /// visible. Focus traversal skips disabled subtrees and AccessKit marks their
    /// nodes as disabled. Accepts `Signal<bool>`, `Prop<bool>`, compatibility state
    /// bindings, or plain `bool`.
    ///
    /// The bound signal registers at `BindingLevel::SubtreeRepaint`: when
    /// it flips, the entire subtree rooted at `id` is marked for repaint
    /// (not relayout — geometry doesn't change). Leaves like
    /// `IconWidget` then re-resolve their role color
    /// using the new `PaintContext::effective_enabled` value, so a
    /// disabled subtree's icons and text dim automatically.
    pub fn enabled_when(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        let prop = state.into();
        // SubtreeRepaint propagates the visual dirty mark through the
        // disabled subtree so leaves re-resolve their role colors.
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::SubtreeRepaint,
        );
        // AccessibilityOnly is orthogonal — it flips `a11y_dirty` so
        // AccessKit's `disabled` flag refreshes on the next a11y sync
        // (the accessibility walker reads `arena.is_enabled(id)`,
        // which is already correct via the prop, but the tree needs
        // to be told to rebuild).
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::AccessibilityOnly,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.enabled_state = Some(prop);
        }
    }

    /// Reactive view of "is this widget effectively enabled?" — the AND
    /// of the widget's own `enabled_state` and every ancestor's.
    /// [`Self::is_enabled`] is the non-reactive equivalent; this method
    /// gives composite widgets a `Signal<bool>` for derived state.
    ///
    /// Leaves (`IconWidget`, `TextWidget`, `RectWidget`) do NOT need this
    /// — they receive the resolved bool via
    /// [`crate::widget::PaintContext::effective_enabled`] at paint time.
    /// This method is for composites that want to derive cursor / custom
    /// paint roles / etc. reactively.
    ///
    /// Install-or-reuse, exactly like [`Self::activation_signal`]: the signal
    /// lives on the node and the framework refreshes it from the live arena
    /// once per state-change pass (`flush_effective_enabled_signals`).
    ///
    /// It is deliberately NOT a signal derived by walking the ancestor chain
    /// here. A widget's `parent` is still `None` while its own `build()` runs
    /// — `insert_widget` inserts the node parentless and wires the parent link
    /// only after `build()` returns — so an ancestor walk performed from
    /// inside `build()` (which is how every caller uses this) sees an empty
    /// chain and would capture the widget's OWN `enabled` prop as the whole
    /// answer, permanently. That was a real bug: a Button inside a disabled
    /// form stayed painted as if enabled.
    ///
    /// The value is seeded from the live arena and corrected on the next
    /// flush, so a first-`build()` caller (parent not yet wired) and a
    /// rebuild caller (parent wired) both converge before anything paints.
    pub fn effective_enabled_signal(&mut self, id: WidgetId) -> crate::signal::Signal<bool> {
        if let Some(existing) = self
            .arena
            .get(id)
            .and_then(|n| n.effective_enabled_signal.clone())
        {
            return existing;
        }
        // Seed from the live tree. Mid-`build()` the parent is not wired yet,
        // so this is the widget's own state only; `flush_effective_enabled_signals`
        // corrects it against the fully-wired tree before the first paint.
        let seed = self.arena.is_enabled(id);
        let sig = crate::signal::Signal::new(seed);
        let Some(node) = self.arena.get_mut(id) else {
            // Node missing (shouldn't happen in build) — hand back a detached
            // handle so the caller still gets a valid signal.
            return crate::signal::Signal::new(true);
        };
        node.effective_enabled_signal = Some(sig.clone());
        self.arena.watch_effective_enabled(id);
        sig
    }

    /// Refresh every node-resident `effective_enabled_signal` against the live
    /// arena, firing observers only where the value actually changed.
    ///
    /// Unlike [`Self::flush_activation_signals`] this cannot be driven off a
    /// change queue: a node's `enabled_state` is a `Prop<bool>` that may be
    /// bound to an app `Signal` which flips without the arena being notified,
    /// so there is no mutation site at which to record a transition. Instead
    /// this recomputes the (cheap, `O(depth)`) ancestor AND for each opted-in
    /// node and diffs. Only nodes that called
    /// [`Self::effective_enabled_signal`] are visited, so a tree with no
    /// interactive widgets pays nothing.
    ///
    /// Values are collected first and set afterwards: a `Signal::set` observer
    /// may mutate the tree, and must not run while the arena is being walked —
    /// the same discipline as `flush_activation_signals` and the
    /// `focus_within` / `hover_within` updates.
    pub(crate) fn flush_effective_enabled_signals(&mut self) {
        self.arena.prune_effective_enabled_watchers();
        let mut updates: Vec<(crate::signal::Signal<bool>, bool)> = Vec::new();
        for id in self.arena.effective_enabled_watchers() {
            let Some(sig) = self
                .arena
                .get(id)
                .and_then(|n| n.effective_enabled_signal.clone())
            else {
                continue;
            };
            let now = self.arena.is_enabled(id);
            if sig.get() != now {
                updates.push((sig, now));
            }
        }
        for (sig, value) in updates {
            sig.set(value);
        }
    }

    /// Whether a widget is effectively enabled. Returns `false` if the widget
    /// itself or any ancestor has `enabled_state` bound to `false`.
    pub fn is_enabled(&self, id: WidgetId) -> bool {
        self.arena.is_enabled(id)
    }

    /// Bind a widget's Tab-key participation to a boolean prop or
    /// compatibility state binding. When false, the widget is removed
    /// from Tab / Shift+Tab traversal (`cycle_focus`) but remains
    /// reachable via `request_focus` and arrow-key navigation that
    /// calls `request_focus`. Implements the ARIA roving-tabindex
    /// pattern (HTML `tabindex="-1"` semantics). Accepts
    /// `Signal<bool>`, `Prop<bool>`, or plain `bool`.
    /// Publish what a data view's `Space` should do when the row containing
    /// `id` holds the keyboard cursor. See
    /// [`WidgetNode::keyboard_toggle`](crate::arena::WidgetNode).
    pub fn set_keyboard_toggle(
        &mut self,
        id: WidgetId,
        f: std::rc::Rc<dyn Fn(&mut crate::widget::EventContext)>,
    ) {
        if let Some(node) = self.arena.get_mut(id) {
            node.keyboard_toggle = Some(f);
        }
    }

    /// The first keyboard-toggle action published in `root`'s subtree, in
    /// traversal order.
    ///
    /// Searched per keypress rather than cached: a data view rebuilds its rows
    /// as they realize, so an id recorded at build time would outlive the
    /// widget it named.
    pub fn keyboard_toggle_in(
        &self,
        root: WidgetId,
    ) -> Option<std::rc::Rc<dyn Fn(&mut crate::widget::EventContext)>> {
        if let Some(f) = self.arena.get(root).and_then(|n| n.keyboard_toggle.clone()) {
            return Some(f);
        }
        for &child in self.arena.children(root) {
            if let Some(found) = self.keyboard_toggle_in(child) {
                return Some(found);
            }
        }
        None
    }

    pub fn set_tab_stop(&mut self, id: WidgetId, state: impl Into<crate::signal::Prop<bool>>) {
        let prop = state.into();
        // Bind at the lightest level — tab-stop changes never affect
        // layout or paint; cycle_focus reads the current value on
        // each Tab keypress.
        prop.register_if_bound(
            id,
            &self.binding_registry,
            crate::binding::BindingLevel::RepaintOnly,
        );
        if let Some(node) = self.arena.get_mut(id) {
            node.tab_stop = Some(prop);
        }
    }

    /// Current Tab-key participation for a widget. Returns the value
    /// of the `tab_stop` prop if bound, or `true` (the default) when
    /// no binding is present. Mirrors the filter used by
    /// `cycle_focus` — primarily for tests asserting the
    /// roving-tabindex contract.
    pub fn tab_stop(&self, id: WidgetId) -> bool {
        self.arena
            .get(id)
            .and_then(|node| node.tab_stop.as_ref())
            .map(|prop| prop.get())
            .unwrap_or(true)
    }

    /// Declare `id` as a **traversal-scope boundary** with the given policy.
    /// `cycle_focus` then treats the node's subtree as an independent Tab
    /// group: `tab_index` values inside it are scoped (they never collide
    /// with sibling scopes) and `policy` governs Tab at the scope's ends.
    ///
    /// The scope node is forced non-focusable — it is a transparent boundary,
    /// never itself a Tab stop. Called from `BuildContext::set_traversal_scope`
    /// (which the `FocusScope` wrapper widget invokes during `build`), and
    /// directly usable from tests with no dependency on the widgets crate.
    pub fn set_traversal_scope(
        &mut self,
        id: WidgetId,
        policy: crate::focus::TraversalScopePolicy,
    ) {
        if let Some(node) = self.arena.get_mut(id) {
            node.node_traversal_scope = Some(policy);
            node.node_focusable = Some(false);
        }
    }

    /// Remove a previously set traversal-scope marker from `id` (rebuild
    /// paths where a `FocusScope` is replaced by a non-scope widget). Leaves
    /// `node_focusable` untouched — a later handler-set application resets it.
    pub fn clear_traversal_scope(&mut self, id: WidgetId) {
        if let Some(node) = self.arena.get_mut(id) {
            node.node_traversal_scope = None;
        }
    }

    /// Current traversal-scope policy on `id`, if any. For tests asserting
    /// the scope marker contract.
    pub fn traversal_scope(&self, id: WidgetId) -> Option<crate::focus::TraversalScopePolicy> {
        self.arena
            .get(id)
            .and_then(|node| node.node_traversal_scope)
    }

    // --- Theme override ---

    /// Set a theme override on a widget. All descendants of this widget
    /// will see the modified theme during layout and paint.
    /// The override function receives a mutable `Theme` to modify.
    ///
    /// ```
    /// # use teksilo_core::{Widget, LayoutResponse, LayoutContext, widget_tree::WidgetTree};
    /// # use teksilo_canvas::{Size, SizeProposal};
    /// # use teksilo_tokens::ColorTokens;
    /// # #[derive(Debug)] struct MinWidget;
    /// # impl Widget for MinWidget {
    /// #     fn layout_response(&self, _: SizeProposal, _: &LayoutContext) -> LayoutResponse {
    /// #         Size::new(0.0, 0.0).into()
    /// #     }
    /// # }
    /// # let mut tree = WidgetTree::new();
    /// # let panel_id = tree.add(MinWidget);
    /// tree.set_theme_override(panel_id, |theme| {
    ///     theme.colors = ColorTokens::dark_default();
    /// });
    /// ```
    pub fn set_theme_override(
        &mut self,
        id: WidgetId,
        f: impl Fn(&mut crate::styles::Theme) + 'static,
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
    pub fn resolved_theme(&self, id: WidgetId) -> crate::styles::Theme {
        self.arena.resolve_theme(id, &self.theme).into_owned()
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod activation_signal_tests {
    use super::*;
    use crate::build_context::BuildContext;
    use crate::signal::Signal;
    use crate::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_canvas::SizeProposal;

    /// A leaf that, on build, opts into its activation signal and mirrors it
    /// into an out-of-band signal the test can read.
    #[derive(Debug)]
    struct ActivationProbe {
        log: Signal<Vec<bool>>,
    }

    impl Widget for ActivationProbe {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let id = ctx.self_id();
            let vis = ctx.activation_signal(id);
            let log = self.log.clone();
            ctx.effect(&vis, move |active| {
                let mut v = log.get();
                v.push(*active);
                log.set(v);
            });
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }

    /// A widget that enqueues a post-mount action via `run_after_mount` has it
    /// run exactly once, with a real `EventContext`, when the tree drains
    /// (`run_mount_actions`) — and `has_pending_mount_actions` reflects the
    /// queue state.
    #[derive(Debug)]
    struct MountActionProbe {
        ran: Signal<u32>,
    }

    impl Widget for MountActionProbe {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let ran = self.ran.clone();
            ctx.run_after_mount(move |_ectx| ran.set(ran.get() + 1));
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }

    #[test]
    fn run_after_mount_runs_once_on_drain() {
        let mut tree = WidgetTree::new();
        let ran = Signal::new(0_u32);
        tree.add(MountActionProbe { ran: ran.clone() });
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Queued during build, not yet run.
        assert!(tree.has_pending_mount_actions());
        assert_eq!(ran.get(), 0);

        // Drain with a Noop sink (as headless callers do).
        tree.run_mount_actions(&mut crate::window::NoopWindowOps);
        assert_eq!(ran.get(), 1);
        assert!(!tree.has_pending_mount_actions());

        // Draining again is a no-op (the queue is empty).
        tree.run_mount_actions(&mut crate::window::NoopWindowOps);
        assert_eq!(ran.get(), 1);
    }

    #[test]
    fn activation_signal_fires_on_dormant_and_reactivate() {
        let mut tree = WidgetTree::new();
        let log = Signal::new(Vec::<bool>::new());
        let probe = tree.add(ActivationProbe { log: log.clone() });

        // Gate the probe's visibility on a signal.
        let visible = Signal::new(true);
        tree.visible_when(probe, visible.clone());
        tree.layout(SizeProposal::exact(100.0, 100.0));

        // Initially active: effect registration alone fires nothing.
        assert_eq!(log.get(), Vec::<bool>::new());

        // Hide → dormant → activation signal false.
        visible.set(false);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(log.get(), vec![false]);

        // Show → active → activation signal true.
        visible.set(true);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(log.get(), vec![false, true]);

        // Redundant relayout while active fires nothing new.
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(log.get(), vec![false, true]);
    }

    /// **A node parked and re-woken before the flush never looks dormant.**
    ///
    /// `pending_activation_changes` is an append-only queue, so this records
    /// two edges; replaying them in order would hand observers a `false` that
    /// was already superseded — for a state signal that is a lie, not a
    /// history. `present_in_tree_modal_request` takes exactly this route on
    /// every dialog (build the content, park it, mount the scrim, wake it,
    /// *then* move focus in), and the stale `false` landed after the focus
    /// dispatch: a text editor's dormancy handler wiped the focus it had just
    /// been granted and the dialog opened with no caret.
    #[test]
    fn a_park_and_wake_inside_one_batch_fires_nothing() {
        let mut tree = WidgetTree::new();
        let log = Signal::new(Vec::<bool>::new());
        let probe = tree.add(ActivationProbe { log: log.clone() });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(log.get(), Vec::<bool>::new());

        tree.set_dormant(probe);
        tree.activate(probe);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(
            log.get(),
            Vec::<bool>::new(),
            "a node that ends the batch where it started never observably \
             changed — firing the intermediate `false` makes observers act on \
             a state that was never visible",
        );

        // The converse still reports: a batch with a *net* transition fires
        // once, with the state the node actually ended in.
        tree.activate(probe);
        tree.set_dormant(probe);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(
            log.get(),
            vec![false],
            "a net Active→Dormant batch must still report, exactly once",
        );

        tree.activate(probe);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert_eq!(log.get(), vec![false, true]);
    }
}

#[cfg(test)]
mod visible_when_builder_tests {
    use super::*;
    use crate::signal::{Prop, Signal};
    use crate::widget_builder::WidgetBuilder;

    /// `.visible_when(signal)` on any widget builder threads a *bound*
    /// visibility prop onto the inserted node — so `teksu!`'s property form
    /// (`Widget { visible_when: sig }`) reaches the same `node.visible_state`
    /// slot as the imperative `ctx.visible_when(id, sig)`.
    #[test]
    fn visible_when_builder_binds_node_visibility() {
        let mut tree = WidgetTree::new();
        let shown = Signal::new(false);
        let id = tree.add(crate::test_widgets::FillWidget::new().visible_when(shown.clone()));

        let node = tree.arena.get(id).expect("node exists");
        assert!(
            matches!(node.visible_state, Some(Prop::Bound(_))),
            "`.visible_when(Signal)` must store a bound visibility prop"
        );
    }

    /// A static `bool` is accepted too (`Prop::Static`), matching
    /// `ctx.visible_when` / `access_hidden` semantics.
    #[test]
    fn visible_when_builder_accepts_static_bool() {
        let mut tree = WidgetTree::new();
        let id = tree.add(crate::test_widgets::FillWidget::new().visible_when(false));

        let node = tree.arena.get(id).expect("node exists");
        assert!(matches!(node.visible_state, Some(Prop::Static(false))));
    }
}

#[cfg(test)]
mod text_scale_tests {
    use super::*;

    #[test]
    fn effective_theme_starts_equal_to_theme() {
        let tree = WidgetTree::new();
        assert_eq!(
            tree.effective_theme.typography.body.size,
            tree.theme.typography.body.size
        );
        assert_eq!(tree.user_text_scale(), 1.0);
    }

    #[test]
    fn effective_text_scale_and_signal_track_the_combined_factor() {
        let mut tree = WidgetTree::new();
        assert_eq!(tree.effective_text_scale(), 1.0);
        assert_eq!(tree.text_scale_signal().get(), 1.0);

        tree.set_user_text_scale(1.5);
        assert!((tree.effective_text_scale() - 1.5).abs() < 0.001);
        assert!((tree.text_scale_signal().get() - 1.5).abs() < 0.001);

        // OS preference multiplies in.
        tree.set_accessibility_preferences(false, false, 2.0);
        assert!((tree.effective_text_scale() - 3.0).abs() < 0.01);
        assert!((tree.text_scale_signal().get() - 3.0).abs() < 0.01);
    }

    #[test]
    fn set_user_text_scale_scales_effective_typography() {
        let mut tree = WidgetTree::new();
        let base = tree.theme.typography.body.size;
        tree.set_user_text_scale(1.5);
        assert!((tree.effective_theme.typography.body.size - base * 1.5).abs() < 0.001);
        // The unscaled base theme is untouched.
        assert_eq!(tree.theme.typography.body.size, base);
    }

    #[test]
    fn set_theme_preserves_existing_user_scale() {
        let mut tree = WidgetTree::new();
        tree.set_user_text_scale(2.0);
        let dark = crate::presets::intui::dark();
        let dark_base = dark.typography.body.size;
        tree.set_theme(dark);
        assert!((tree.effective_theme.typography.body.size - dark_base * 2.0).abs() < 0.001);
    }

    #[test]
    fn os_text_scale_multiplies_with_user_scale() {
        let mut tree = WidgetTree::new();
        let base = tree.theme.typography.body.size;
        tree.set_user_text_scale(1.5);
        // OS preference reports 1.2 → combined 1.8.
        tree.set_accessibility_preferences(false, false, 1.2);
        assert!((tree.effective_theme.typography.body.size - base * 1.8).abs() < 0.01);
    }

    #[test]
    fn same_scale_is_a_noop_and_factor_is_clamped() {
        let mut tree = WidgetTree::new();
        tree.set_user_text_scale(1.5);
        // Re-setting the same value should not panic / change anything.
        tree.set_user_text_scale(1.5);
        assert_eq!(tree.user_text_scale(), 1.5);
        // Out-of-range clamps into [0.25, 8.0].
        tree.set_user_text_scale(100.0);
        assert_eq!(tree.user_text_scale(), 8.0);
    }
}

/// Covers the `WidgetTree`-level facts
/// `teksilo_app::WindowManager::request_redraw_needing_render` (the
/// targeted cross-window redraw added for shared-`Signal` dispatch
/// fan-out) relies on. Neither test touches windows at all — they exist
/// to pin down `needs_render()`'s contract in isolation, since
/// teksilo-app cannot stand up a real `PlatformWindow` in a unit test.
#[cfg(test)]
mod cross_window_redraw_signal_tests {
    use super::*;
    use crate::signal::Signal;
    use crate::test_widgets::{FillWidget, StackWidget};
    use teksilo_canvas::SizeProposal;

    /// The premise the fix acts on, AND the trap a naive fix would fall
    /// into. A `Signal` shared by two independent trees (standing in for
    /// two windows) is supposed to dirty both when mutated, even though
    /// only one of them is the tree whose dispatch made the mutation —
    /// but `Signal::set` only flips a dirty flag on the signal itself and
    /// in the `BindingRegistry`; nothing walks that into a tree's
    /// `needs_layout` / `needs_paint` bits (what `needs_render()` reads)
    /// except that tree's OWN `process_state_changes`, run at the top of
    /// its OWN `layout()`. So immediately after the mutation, with
    /// neither tree having re-run `layout()`, BOTH read clean — a naive
    /// "just check `needs_render()`" cross-window redraw would see
    /// nothing to do and stay a permanent no-op. Once tree B's `layout()`
    /// runs (what `request_redraw_needing_render` does for every window
    /// before checking it), the same mutation is finally visible there.
    ///
    /// Each tree wraps its gated leaf in a `StackWidget` parent (rather
    /// than gating a bare root leaf) so the fact under test — an ACTIVE
    /// widget ending up dirty — is unambiguous: `any_needs_layout()` /
    /// `any_needs_paint()` only ever look at `Active` nodes, and a leaf
    /// that itself goes dormant is deliberately excluded from both (a
    /// hidden widget has nothing to paint). What must go dirty here is
    /// the STILL-ACTIVE stack, via `mark_ancestors_need_layout` — the
    /// same mechanism that makes a real window's content re-flow around
    /// a child that just appeared or disappeared.
    #[test]
    fn a_shared_signal_mutation_only_shows_up_after_that_trees_own_layout_reconciles_it() {
        let shared = Signal::new(true);

        let mut tree_a = WidgetTree::new();
        let stack_a = tree_a.add(StackWidget::new());
        let id_a = tree_a.add_child(stack_a, FillWidget::new());
        tree_a.visible_when(id_a, shared.clone());

        let mut tree_b = WidgetTree::new();
        let stack_b = tree_b.add(StackWidget::new());
        let id_b = tree_b.add_child(stack_b, FillWidget::new());
        tree_b.visible_when(id_b, shared.clone());

        let proposal = SizeProposal::exact(100.0, 100.0);

        // Bring both to the same clean baseline a real event loop reaches
        // after its initial layout + paint.
        tree_a.layout(proposal);
        tree_a.render();
        tree_b.layout(proposal);
        tree_b.render();
        assert!(!tree_a.needs_render(), "precondition: tree A starts clean");
        assert!(!tree_b.needs_render(), "precondition: tree B starts clean");

        // Simulate a handler mutating the shared Signal during tree A's
        // dispatch. Neither tree re-runs layout() here yet.
        shared.set(false);

        assert!(
            !tree_a.needs_render(),
            "the mutation alone does not retroactively dirty tree A either — \
             a Signal write cannot poke an arena directly, only the next \
             process_state_changes (inside layout()) can"
        );
        assert!(
            !tree_b.needs_render(),
            "and tree B reads exactly as clean as tree A does at this point — \
             checking needs_render() without reconciling first cannot tell them apart"
        );

        // ...but they are NOT indistinguishable to `needs_reconcile()`,
        // which is what `request_redraw_needing_render` actually gates
        // its reconcile on. Both trees observe the shared Signal, so
        // both report pending reactive work here — and asking is
        // read-only, so asking tree A first does not answer for tree B.
        assert!(
            tree_a.needs_reconcile() && tree_b.needs_reconcile(),
            "both trees must report pending reactive work from the shared write"
        );

        // This is what `request_redraw_needing_render` does for a window
        // whose gate is open — reconcile at the window's OWN current
        // size, which is what walks a pending Signal-driven change into
        // the arena.
        tree_b.layout(proposal);

        assert!(
            tree_b.needs_render(),
            "tree B, which merely OBSERVES the shared Signal, is now dirty — \
             this is the cross-tree fan-out request_redraw_needing_render \
             exists to notice (via its own reconcile-then-check) and repaint"
        );
        assert!(
            !tree_b.needs_reconcile(),
            "and having reconciled, tree B's gate closes again"
        );
        assert!(
            tree_a.needs_reconcile(),
            "while tree A — which has NOT reconciled — is still waiting; one \
             window's reconcile must never close another's gate"
        );
    }

    /// The gate `request_redraw_needing_render` gained must stay SHUT for
    /// a window with nothing reactive pending, or it saves nothing: that
    /// method runs after every dispatched event, and an open gate costs
    /// a full `layout_with_ops` — a dozen per-frame passes (pending
    /// animations, frame tick, scheduler tick, drag tick,
    /// `process_state_changes`, tooltips, four overlay passes) before it
    /// reaches the geometry short-circuit, for every open window, on
    /// every event of a fast mouse-move stream.
    #[test]
    fn needs_reconcile_is_false_for_an_idle_tree() {
        let shared = Signal::new(true);
        let mut idle = WidgetTree::new();
        let stack = idle.add(StackWidget::new());
        let leaf = idle.add_child(stack, FillWidget::new());
        idle.visible_when(leaf, shared.clone());
        idle.layout(SizeProposal::exact(100.0, 100.0));
        idle.render();

        assert!(!idle.needs_reconcile(), "nothing written — gate shut");
        assert!(!idle.needs_reconcile(), "and asking does not open it");

        shared.set(false);
        assert!(idle.needs_reconcile(), "a write opens it");
        idle.layout(SizeProposal::exact(100.0, 100.0));
        assert!(!idle.needs_reconcile(), "reconciling closes it again");
    }

    /// A *pending* `animate_to` contributes no scheduler deadline until
    /// `process_pending_animations` promotes it, so `next_timer_deadline`
    /// (and therefore `request_redraw_due`) cannot see it. The gate must,
    /// or arming an animation from another window's handler would leave
    /// it parked until something unrelated woke the window.
    #[test]
    fn needs_reconcile_sees_an_animation_armed_but_not_yet_started() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.render();

        // Registered but deliberately NOT bound to any widget, so the
        // binding-registry term cannot be what notices it.
        let anim = Signal::new_animated(0.0_f32);
        tree.register_animated_signal(&anim, id);
        assert!(!tree.needs_reconcile(), "precondition: nothing armed yet");

        anim.animate_to(
            1.0,
            std::time::Duration::from_millis(200),
            teksilo_tokens::Easing::Linear,
        );
        assert!(
            tree.needs_reconcile(),
            "an armed-but-unstarted animation is reactive work only layout() can pick up"
        );
        assert!(
            anim.has_pending_animation(),
            "and asking must not have consumed the request"
        );

        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(
            !anim.has_pending_animation(),
            "the reconcile promoted it into the scheduler"
        );
    }

    /// An animation armed *after* the wall clock has overtaken the simulated one
    /// must still run.
    ///
    /// `layout` promotes a pending `animate_to` into the scheduler; once
    /// `tick_animations` is driving the tree, the scheduler is only ever ticked at
    /// `sim_clock`. Stamping the promotion with `Instant::now()` therefore put the
    /// start in the scheduler's *future* the moment a test's real time outran the
    /// simulated time it had asked for — and a start in the future does not run
    /// slow, it does not run at all. That made animated headless tests a function
    /// of machine load: green run alone, frozen once a full suite filled the cores
    /// and stretched each test's wall-clock time past its simulated budget.
    ///
    /// The two clocks below are the shape of that: 90 ms simulated against at
    /// least 100 ms real, so simulated time never catches up. Under the old
    /// behaviour the animation stays pinned at its start value forever.
    #[test]
    fn an_animation_armed_after_real_time_outran_the_sim_clock_still_runs() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(50.0, 50.0));

        // Put the tree in simulated time, then let the wall clock get ahead of it.
        tree.tick_animations(std::time::Duration::from_millis(10));
        std::thread::sleep(std::time::Duration::from_millis(100));

        let anim = Signal::new_animated(0.0_f32);
        tree.register_animated_signal(&anim, id);
        anim.animate_to(
            1.0,
            std::time::Duration::from_millis(50),
            teksilo_tokens::Easing::Linear,
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));

        // 80 ms of simulated time against a 50 ms animation: comfortably finished
        // on the only clock the scheduler is ever ticked with, and still short of
        // the ~100 ms of real time that has passed.
        tree.tick_animations(std::time::Duration::from_millis(80));

        assert_eq!(
            anim.get(),
            1.0,
            "the animation must be measured against the clock it is ticked with, \
             not the wall clock that has already run past it"
        );
        assert!(
            !tree.has_active_animations(),
            "and having reached its target it must be off the scheduler"
        );
    }

    /// A layout pass on a simulated tree advances no animation.
    ///
    /// The mirror image of the test above, and the other half of the same
    /// contract: `layout` both **promotes** a pending `animate_to` and
    /// **ticks** the scheduler, and it has to do both against the same clock.
    /// Promoting at `sim_clock` while ticking at `Instant::now()` hands the
    /// freshly promoted animation an elapsed time equal to the tree's entire
    /// wall-clock age — so it finishes inside the very layout pass that started
    /// it, and how much of it a test ever observes depends on how long that
    /// test took to get there.
    ///
    /// The 120 ms slept below is what the wall clock would contribute; the
    /// tween is 100 ms, so under the old behaviour it is already over before
    /// the caller advances anything.
    #[test]
    fn a_layout_pass_on_a_simulated_tree_advances_no_animation() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(50.0, 50.0));

        // Put the tree on the simulated clock, then let real time run past the
        // whole duration of the animation that is about to be armed.
        tree.advance_time(std::time::Duration::from_millis(1));
        std::thread::sleep(std::time::Duration::from_millis(120));

        let anim = Signal::new_animated(0.0_f32);
        tree.register_animated_signal(&anim, id);
        anim.animate_to(
            100.0,
            std::time::Duration::from_millis(100),
            teksilo_tokens::Easing::Linear,
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));

        assert_eq!(
            anim.get(),
            0.0,
            "a layout pass promotes the animation; it does not also age it by \
             however long the tree has been alive"
        );

        // It moves when, and only when, the clock is advanced.
        tree.advance_time(std::time::Duration::from_millis(50));
        assert!(
            (anim.get() - 50.0).abs() < 2.0,
            "half of a 100 ms linear tween: {}",
            anim.get()
        );
    }

    /// `needs_render()` (paint/layout dirt only) must stay `false` while a
    /// per-frame `Signal<f32>` animation is merely *running*, with nothing
    /// new to paint. `request_redraw_needing_render` filters on
    /// `needs_render()`, not the broader `needs_redraw()`, precisely so a
    /// window with a live animation isn't forced into an extra immediate
    /// redraw on every sibling-window event — that would defeat the 60 Hz
    /// `WaitUntil` pacing those animations already get elsewhere and
    /// reintroduce the uncapped free-running redraw bug that pacing was
    /// written to remove.
    #[test]
    fn needs_render_excludes_a_running_animation_with_no_dirty_paint() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.render();
        assert!(!tree.needs_render(), "precondition: tree starts clean");
        assert!(!tree.needs_redraw(), "precondition: nothing running yet");

        let anim = Signal::new_animated(0.0_f32);
        tree.register_animated_signal(&anim, id);
        anim.animate_to(
            1.0,
            std::time::Duration::from_millis(200),
            teksilo_tokens::Easing::Linear,
        );
        // `process_pending_animations` (which picks up the pending
        // `animate_to` and starts it on the scheduler) runs inside `layout`.
        tree.layout(SizeProposal::exact(50.0, 50.0));

        assert!(
            tree.needs_redraw(),
            "an animation just started, so needs_redraw() (has_running()) must be true"
        );
        assert!(
            !tree.needs_render(),
            "but nothing is actually dirty for paint/layout — needs_render() must stay false, \
             which is the whole point of using it (not needs_redraw()) as the cross-window filter"
        );
    }
}

#[cfg(test)]
mod effective_enabled_signal_tests {
    use super::*;
    use crate::build_context::BuildContext;
    use crate::signal::Signal;
    use crate::widget::{LayoutContext, LayoutResponse, Widget};
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::SizeProposal;

    type SignalSlot = Rc<RefCell<Option<Signal<bool>>>>;

    /// A leaf that opts into `effective_enabled_signal` from inside its own
    /// `build()` — the only way real widgets use it, and the case that was
    /// broken. It publishes the handle so the test can read the live value.
    #[derive(Debug)]
    struct EnabledProbe {
        out: SignalSlot,
    }

    impl Widget for EnabledProbe {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let id = ctx.self_id();
            let sig = ctx.effective_enabled_signal(id);
            // Also pins that the signal is *mutable*: the previous derived
            // implementation panicked here with "observe() is only supported
            // on mutable signals", which is why widgets could not use
            // `ctx.effect` to react to being disabled.
            ctx.effect(&sig, |_| {});
            *self.out.borrow_mut() = Some(sig);
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }

    /// A composite that adds the probe through the ordinary `ctx.add` idiom, so
    /// the child is inserted PARENTLESS and builds before its parent link is
    /// wired — the exact situation that defeated the old
    /// walk-the-ancestors-at-call-time implementation.
    #[derive(Debug)]
    struct Form {
        enabled: Signal<bool>,
        out: SignalSlot,
    }

    impl Widget for Form {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let id = ctx.self_id();
            ctx.enabled_when(id, self.enabled.clone());
            vec![ctx.add(EnabledProbe {
                out: self.out.clone(),
            })]
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }

    fn mount_form(enabled: Signal<bool>) -> (WidgetTree, WidgetId, Signal<bool>) {
        let out: SignalSlot = Rc::new(RefCell::new(None));
        let mut tree = WidgetTree::new();
        let form = tree.add(Form {
            enabled,
            out: out.clone(),
        });
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let sig = out.borrow().clone().expect("probe published its signal");
        (tree, form, sig)
    }

    /// THE REGRESSION. A widget whose own `enabled` is untouched must still
    /// report disabled when an ANCESTOR is disabled. This failed before the
    /// signal became node-resident: `insert_widget` inserts the node with
    /// `parent: None` and wires the parent only after `build()` returns, so an
    /// ancestor walk done during `build()` saw an empty chain and captured
    /// "enabled" for the widget's whole life.
    #[test]
    fn tracks_an_ancestor_disabled_before_mount() {
        let (_tree, _form, sig) = mount_form(Signal::new(false));
        assert!(
            !sig.get(),
            "a child of a disabled ancestor must report effectively-disabled"
        );
    }

    /// The live case: the ancestor's bound signal flips after mount. The child
    /// must follow, in both directions, with no rebuild.
    #[test]
    fn follows_an_ancestor_flipping_after_mount() {
        let enabled = Signal::new(true);
        let (mut tree, _form, sig) = mount_form(enabled.clone());
        assert!(sig.get(), "starts enabled");

        enabled.set(false);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(!sig.get(), "child follows the ancestor going disabled");

        enabled.set(true);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        assert!(sig.get(), "and follows it coming back");
    }

    /// A widget's own `enabled_state` still works on its own.
    #[test]
    fn honours_the_widgets_own_state() {
        let out: SignalSlot = Rc::new(RefCell::new(None));
        let mut tree = WidgetTree::new();
        let probe = tree.add(EnabledProbe { out: out.clone() });
        tree.enabled_when(probe, false);
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let sig = out.borrow().clone().unwrap();
        assert!(!sig.get(), "own enabled_state alone disables");
    }

    /// The signal must agree with the paint-time bool the render walker
    /// computes. If they disagreed, role-driven chrome (which dims from
    /// `PaintContext::effective_enabled`) and signal-driven chrome (which dims
    /// from this signal) would grey out at different moments.
    #[test]
    fn agrees_with_the_paint_time_effective_enabled() {
        let enabled = Signal::new(true);
        let (mut tree, form, sig) = mount_form(enabled.clone());
        let probe = tree.children(form)[0];

        for value in [false, true, false] {
            enabled.set(value);
            tree.layout(SizeProposal::exact(100.0, 100.0));
            assert_eq!(
                sig.get(),
                tree.is_enabled(probe),
                "signal and the arena's live is_enabled must agree (enabled={value})"
            );
        }
    }

    /// Install-or-reuse: asking twice hands back the same signal.
    #[test]
    fn is_install_or_reuse() {
        let out: SignalSlot = Rc::new(RefCell::new(None));
        let mut tree = WidgetTree::new();
        let id = tree.add(EnabledProbe { out });
        let a = tree.effective_enabled_signal(id);
        let b = tree.effective_enabled_signal(id);
        assert!(
            Signal::same(&a, &b),
            "must hand back the same signal handle"
        );
    }
}

#[cfg(test)]
mod density_tests {
    use super::*;
    use teksilo_tokens::{DensityPolicy, TargetDensity};

    /// A fresh tree is Compact — today's behaviour — and reports it.
    #[test]
    fn a_fresh_tree_is_compact() {
        let tree = WidgetTree::new();
        assert_eq!(tree.input_density(), TargetDensity::Compact);
        assert_eq!(tree.theme().input, teksilo_tokens::InputTokens::default());
        assert!(tree.touch_enabled());
        assert_eq!(
            tree.density_policy(),
            DensityPolicy::Fixed(TargetDensity::Compact)
        );
    }

    /// A density switch changes what the tree reports **and** dirties it at the
    /// rebuild level — a target size is baked in `build()`, so layout+paint
    /// alone cannot re-bake it.
    #[test]
    fn switching_density_reports_and_dirties_at_rebuild_level() {
        let mut tree = WidgetTree::new();
        let root = tree.add(crate::test_widgets::FillWidget::new());
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert!(
            tree.arena.collect_needs_rebuild().is_empty(),
            "a laid-out tree starts clean"
        );

        tree.set_input_density(TargetDensity::Touch);

        assert_eq!(tree.input_density(), TargetDensity::Touch);
        assert_eq!(tree.theme().input.target_size, 44.0);
        assert_eq!(tree.theme().input.grab_size, 16.0);
        assert!(
            tree.arena.collect_needs_rebuild().contains(&root),
            "the root must be marked for rebuild, not merely relayout"
        );
    }

    /// Re-setting the same density must not throw away every widget id in the
    /// tree for nothing.
    #[test]
    fn re_setting_the_same_density_is_a_no_op() {
        let mut tree = WidgetTree::new();
        let _root = tree.add(crate::test_widgets::FillWidget::new());
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.set_input_density(TargetDensity::Compact);

        assert!(tree.arena.collect_needs_rebuild().is_empty());
    }

    /// The theme signal follows a density switch, so a `theme_signal`-bound
    /// widget re-resolves without its own wiring.
    #[test]
    fn the_theme_signal_follows_a_density_switch() {
        let mut tree = WidgetTree::new();
        tree.set_input_density(TargetDensity::Comfortable);
        assert_eq!(
            tree.theme_signal().get().input.density,
            TargetDensity::Comfortable
        );
        assert_eq!(tree.theme_signal().get().input.target_size, 32.0);
    }

    /// The kill switch is state on the theme, reachable both ways, and does
    /// not rebuild — it changes which events are accepted, not a dimension.
    #[test]
    fn the_touch_kill_switch_round_trips_without_a_rebuild() {
        let mut tree = WidgetTree::new();
        let _root = tree.add(crate::test_widgets::FillWidget::new());
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.set_touch_enabled(false);

        assert!(!tree.touch_enabled());
        assert!(!tree.theme().input.touch_enabled);
        assert!(!tree.theme_signal().get().input.touch_enabled);
        assert!(tree.arena.collect_needs_rebuild().is_empty());

        tree.set_touch_enabled(true);
        assert!(tree.touch_enabled());
    }

    /// The policy is stored verbatim and does not itself move the density.
    #[test]
    fn setting_a_policy_does_not_switch_the_density() {
        let mut tree = WidgetTree::new();
        let policy = DensityPolicy::FollowLastPointer {
            coarse: TargetDensity::Touch,
            fine: TargetDensity::Compact,
            hysteresis: std::time::Duration::from_secs(1),
        };
        tree.set_density_policy(policy);

        assert_eq!(tree.density_policy(), policy);
        assert_eq!(tree.input_density(), TargetDensity::Compact);
    }
}
