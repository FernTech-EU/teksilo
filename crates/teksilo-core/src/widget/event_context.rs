// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use crate::pointer::touch_action::TouchAction;
use crate::widget_id::WidgetId;

use super::CursorIcon;

/// Selects which overlay-dismissal pathway runs after a handler
/// returns. Last-write-wins: each `dismiss_*_overlays()` setter
/// overwrites the previous choice. `None` (the default) falls
/// through to draining individual ids from `overlay_dismissals`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DismissScope {
    /// Dismiss every overlay in the stack, including hosts.
    All,
    /// Dismiss every overlay whose content is *not* a host surface
    /// (`Tooltip`, `Dialog`, `AlertDialog`). Used by popover triggers
    /// and pre-show cleanup.
    AllExceptHosts,
    /// Walk up from the source widget's containing overlay,
    /// dismissing menu-like overlays and stopping at the first host
    /// surface. Used by menu / dropdown item activation.
    SelfChain,
    /// Dismiss the topmost overlay only.
    Top,
}

/// One queued "reveal this rectangle" request, drained after the handler
/// returns and turned into a [`WidgetEvent::ScrollIntoView`] per clipping
/// ancestor.
///
/// A struct rather than a tuple because the three modifiers (margin, alignment,
/// motion) are independent and positional tuples of that width stop being
/// readable at the call site.
///
/// [`WidgetEvent::ScrollIntoView`]: crate::event::WidgetEvent::ScrollIntoView
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollRevealRequest {
    /// The target, in absolute tree (window) coordinates.
    pub(crate) rect: teksilo_canvas::Rect,
    /// Breathing room to keep around the target, in logical pixels.
    pub(crate) margin: f32,
    /// Where the target should come to rest vertically.
    pub(crate) align: crate::event::ScrollAlign,
    /// Whether to jump or glide.
    pub(crate) motion: crate::event::ScrollMotion,
    /// **Whose** ancestors to walk, when that is not the widget whose handler
    /// queued this.
    ///
    /// `None` means the source widget, which is right whenever a widget reveals
    /// something inside itself. It is wrong — silently — whenever the rect belongs
    /// to a *different* widget: a find banner's Next button asking for a match in
    /// the prose, a toolbar revealing a row in the list below it. Those walk the
    /// button's ancestors, which do not include the scroll container the rect lives
    /// in, so nothing scrolls and the reveal is a no-op no error reports.
    pub(crate) from: Option<crate::widget_id::WidgetId>,
}

/// Context available during event handling.
pub struct EventContext<'ops> {
    pub(crate) cursor_request: Option<CursorIcon>,
    pub(crate) tree_mutations: Vec<TreeMutation>,
    pub(crate) idle_callbacks: Vec<crate::idle::IdleCallback>,
    pub(crate) modal_requests: Vec<crate::modal::ModalRequest>,
    pub(crate) dismiss_modal: bool,
    pub(crate) overlay_requests: Vec<crate::overlay::OverlayRequest>,
    pub(crate) overlay_dismissals: Vec<crate::overlay::OverlayId>,
    /// Content widget ids whose currently-shown overlay (if any) should
    /// be dismissed. Resolved to an `OverlayId` via
    /// `OverlayManager::find_by_content` at drain time. Lets a handler
    /// dismiss an overlay it can only identify by content (e.g. a single
    /// reusable tooltip surface) — the symmetric companion to
    /// [`cancel_delayed_overlay`](EventContext::cancel_delayed_overlay).
    pub(crate) overlay_content_dismissals: Vec<crate::widget_id::WidgetId>,
    /// Overlay requests that name a z-band other than the default. Kept apart
    /// from [`overlay_requests`](Self::overlay_requests) rather than carried on
    /// `OverlayRequest` itself: the band is a property of the *show*, not of
    /// the request, and every existing construction site of the struct would
    /// otherwise have to name it.
    pub(crate) overlay_band_requests:
        Vec<(crate::overlay::OverlayRequest, crate::overlay::OverlayBand)>,
    /// New placements for overlays named by their content root. A
    /// caret-anchored overlay has to be re-placed as the caret moves, and
    /// `position_overlays` re-reads the placement it was shown with — so
    /// without this an affordance follows nothing.
    pub(crate) overlay_placement_updates:
        Vec<(crate::widget_id::WidgetId, crate::overlay::OverlayPlacement)>,
    /// Overlay ids whose `auto_dismiss_after` timer should be paused
    /// or resumed after the handler returns (`true` = pause, `false`
    /// = resume). Drained by `WidgetTree::collect_from_ctx` against
    /// `OverlayManager::pause_auto_dismiss` / `resume_auto_dismiss`,
    /// after the dismissals in the same drain: a pause aimed at an
    /// overlay the same handler dismissed is silently dropped. Used
    /// by `ToastHost` for hover-pause.
    pub(crate) overlay_pause_requests: Vec<(crate::overlay::OverlayId, bool)>,
    /// The dismissal scope chosen by the handler, if any. Set by
    /// `dismiss_all_overlays()` / `dismiss_all_except_hosts()` /
    /// `dismiss_self_overlay_chain()` / `dismiss_top_overlay()` —
    /// last setter wins. `None` falls through to draining the
    /// per-id `overlay_dismissals` vec instead.
    pub(crate) dismiss_scope: Option<DismissScope>,
    /// Request to capture (`true`) or release (`false`) a pointer, and which
    /// one. `None` for the pointer means the one whose sample this handler is
    /// serving — the default, and what every pre-multi-touch call site means.
    pub(crate) pointer_capture: Option<(Option<crate::pointer::PointerId>, bool)>,
    /// The widget currently holding the capture of the pointer being
    /// dispatched, as the tree knew it when this context was made. Read by
    /// [`owns_pointer`](EventContext::owns_pointer).
    pub(crate) pointer_captor: Option<WidgetId>,
    /// Whether the capture request above came from a **widget handler** rather
    /// than from framework plumbing.
    ///
    /// The distinction is the whole of A4's "explicit capture is an
    /// arbitration act": the gesture arena and the drag pipeline both capture
    /// the pointer for their own bookkeeping, and neither is a widget staking
    /// a claim. Only a `capture_pointer()` written in a handler enrols its
    /// caller as a [`MemberRole::RawDrag`](crate::gesture::MemberRole::RawDrag)
    /// competitor.
    pub(crate) explicit_capture: bool,
    /// A recognizer on this node produced a gesture that **owns the rest of
    /// the press** — a drag or a swipe, as opposed to a tap, which completes
    /// the press rather than claiming it. Set by `dispatch_recognized_gesture`
    /// and read by `collect_from_ctx`, which decides the pointer's sequence in
    /// the recognizer's favour.
    pub(crate) recognized_owning_gesture: bool,
    /// A press-time [`DragActivation`](teksilo_tokens::DragActivation) chosen
    /// for this node by its own press handler, overriding its build-time
    /// declaration for this press alone. Applied by `collect_from_ctx` onto the
    /// pointer's sequence, which the enrolment walk reads immediately
    /// afterwards. See [`EventContext::set_drag_activation`].
    pub(crate) drag_activation_override: Option<teksilo_tokens::DragActivation>,
    /// Arbitration acts the handler performed on the sequence owning the
    /// pointer it is serving, in the order it performed them. Applied by
    /// `WidgetTree::collect_from_ctx` against that sequence.
    pub(crate) gesture_acts: Vec<GestureAct>,
    /// The handler asked for its pointer's whole interaction to be revoked.
    /// Queued by `WidgetTree::collect_from_ctx` onto the cancel funnel, so it
    /// runs after this dispatch rather than under it. Last reason wins.
    pub(crate) cancel_pointer_request: Option<crate::pointer::CancelReason>,
    /// The node whose handler is running, when the dispatcher knows it.
    /// `None` for a context made outside per-node dispatch (a gesture timer, a
    /// key-capture callback, an async completion).
    pub(crate) dispatch_node: Option<WidgetId>,
    /// Delayed overlay requests (request, delay, optional focus target,
    /// whether to dismiss sibling overlays when it finally shows).
    pub(crate) delayed_overlay_requests: Vec<(
        crate::overlay::OverlayRequest,
        std::time::Duration,
        Option<crate::widget_id::WidgetId>,
        bool,
    )>,
    /// Timed overlay requests (request, auto-dismiss delay).
    pub(crate) timed_overlay_requests: Vec<(crate::overlay::OverlayRequest, std::time::Duration)>,
    /// Reveal overlay requests (request, caller-owned animated progress
    /// signal, tween duration). The framework shows the overlay, then
    /// drives `progress` 0 → 1 on show and 1 → 0 on dismiss, deferring
    /// the actual stack removal until the roll-back tween completes —
    /// the same deferral the fade path uses, minus the opacity scope.
    /// The caller applies `progress` however it wants (e.g. an `Unroll`
    /// width). See [`show_overlay_with_reveal`](EventContext::show_overlay_with_reveal).
    pub(crate) reveal_overlay_requests: Vec<(
        crate::overlay::OverlayRequest,
        crate::signal::Signal<f32>,
        std::time::Duration,
    )>,
    /// Dismiss descendant overlays of the source widget's containing overlay.
    /// Optionally preserve the subtree rooted at a specific content widget ID.
    pub(crate) dismiss_descendant_overlays: Vec<Option<crate::widget_id::WidgetId>>,
    /// Cancel pending delayed overlays by content widget ID.
    pub(crate) cancel_delayed_overlays: Vec<crate::widget_id::WidgetId>,
    /// Overlays whose safe triangle should be armed at the current
    /// pointer position once this handler returns. See
    /// [`EventContext::arm_overlay_safe_region`].
    pub(crate) safe_region_arm_requests: Vec<crate::widget_id::WidgetId>,
    /// Widget IDs that need repainting (cross-widget signal propagation).
    pub(crate) repaint_requests: Vec<crate::widget_id::WidgetId>,
    /// Synthetic clicks to dispatch on target widgets after event processing.
    pub(crate) synthetic_clicks: Vec<crate::widget_id::WidgetId>,
    /// Focus requests — transfer focus to a specific widget (e.g., overlay content on open).
    pub(crate) focus_requests: Vec<crate::widget_id::WidgetId>,
    /// Focus-into requests — move focus to the *first focusable descendant* of
    /// the given widget, with no fallback to the widget itself when the subtree
    /// has none. The "dive into this region's content" intent (Enter on a tab
    /// header → into the tab panel), distinct from `focus_requests` which
    /// focuses the container itself as a last resort.
    pub(crate) focus_into_requests: Vec<crate::widget_id::WidgetId>,
    /// Rect-based "scroll this into view" requests, in **absolute tree
    /// (window) coordinates**. Queued by [`ensure_visible`](EventContext::ensure_visible)
    /// / [`ensure_visible_with_margin`](EventContext::ensure_visible_with_margin).
    /// Drained in `collect_from_ctx`, which walks the ancestors of the widget
    /// whose handler queued the request and dispatches
    /// [`WidgetEvent::ScrollIntoView`](crate::event::WidgetEvent::ScrollIntoView)
    /// to every `clips_children` scroll container that doesn't already fully
    /// contain the rect — the same ancestor-walk the focus path uses, but
    /// with a caller-supplied rectangle instead of a widget's own bounds
    /// (so a caret, a virtualized row, or a scrolled-off tab header can be
    /// revealed even though it is not itself a distinct focused node).
    pub(crate) scroll_into_view_requests: Vec<ScrollRevealRequest>,
    /// Widget-id-based "scroll this into view" requests, queued by
    /// [`ensure_widget_visible`](EventContext::ensure_widget_visible) /
    /// [`ensure_widget_visible_with_margin`](EventContext::ensure_widget_visible_with_margin).
    /// Drained in `collect_from_ctx`, which resolves each id to its current
    /// absolute arena bounds and walks *that widget's* ancestors (the target
    /// widget itself excluded) dispatching
    /// [`WidgetEvent::ScrollIntoView`](crate::event::WidgetEvent::ScrollIntoView).
    /// The convenience form of the rect API for a target that is a real
    /// mounted, non-virtualized child (a radio tile, a tab header) whose bounds
    /// the framework already knows — the caller need not compute the rect.
    pub(crate) scroll_widget_into_view_requests: Vec<(crate::widget_id::WidgetId, f32)>,
    /// Keyboard-highlight tooltip requests: surface the tooltip of the given
    /// (menu) item immediately and dismiss the previously-highlighted one.
    /// Drained after the handler (see `event_dispatch_impl`). Only the last
    /// entry per handler is honoured — a handler sets one highlight per key.
    pub(crate) highlight_tooltip_requests: Vec<crate::widget_id::WidgetId>,
    /// Drag start request: (source_widget_id, payload, optional_preview_widget).
    pub(crate) drag_start_request: Option<(
        crate::widget_id::WidgetId,
        crate::drag_payload::DragPayload,
        Option<Box<dyn crate::widget::Widget>>,
    )>,
    /// Cancel any active drag session.
    pub(crate) cancel_drag: bool,
    /// Whether the drag session active while this context is live was
    /// started by an external (OS) drag. Read via `drag_is_external()`.
    /// `false` for hand-constructed contexts and when no drag is active.
    pub(crate) drag_is_external: bool,
    /// Replace the tree-level theme. Drained after dispatch; triggers a
    /// composite-widget rebuild and full repaint.
    pub(crate) theme_request: Option<crate::styles::Theme>,
    /// Request that the app follow the OS theme (native / system mode).
    /// Drained after dispatch; the app switches to `ThemeMode::Native` and
    /// recomputes the theme from the current OS colours. Parameterless so
    /// `teksilo-widgets` never needs the app-layer `ThemeMode` enum.
    pub(crate) follow_system_request: bool,
    /// Replace the tree-level locale identifier. Drained after dispatch;
    /// triggers a composite-widget rebuild and full repaint.
    pub(crate) locale_request: Option<String>,
    /// Set the user-controlled text-scale factor. Drained after dispatch and
    /// fanned out to every window; grows all text without a rebuild.
    pub(crate) text_scale_request: Option<f32>,
    /// Set by `request_frame()`; consumed by the event dispatcher which
    /// forwards it to `WidgetTree::request_frame()` so the next layout
    /// pass advances the per-frame tick signal.
    pub(crate) frame_requested: bool,
    /// Optional reference to the tree's app-state registry, so handlers
    /// can look up application-scoped values via `app_state::<T>()`.
    /// Populated by the dispatcher before running each handler; `None`
    /// for hand-constructed contexts in tests.
    pub(crate) app_context: Option<std::rc::Rc<crate::event_source::TreeAppContext>>,
    /// App-level window-ops sink. Injected by the dispatcher so
    /// handlers can reach the multi-window API (`open_window`,
    /// `focus_window`, …) synchronously. For `EventContext`
    /// instances constructed outside a dispatch (standalone trees,
    /// tests) this is `None` and the multi-window methods no-op /
    /// return `None`.
    pub(crate) window_ops: Option<&'ops mut dyn crate::window::WindowOps>,
    /// [`WindowState`](crate::window::WindowState) for the window
    /// this tree belongs to. Cloned from the tree at construction.
    /// `None` for standalone trees.
    pub(crate) current_window: Option<crate::window::WindowState>,
    /// Snapshot of the tree's occlusion-aware window-active state
    /// (`focused AND not occluded`) at construction time. Distinct from
    /// `current_window.focused()` (raw OS focus, no occlusion). Read by
    /// [`window_active`](Self::window_active). Defaults `true` (matches the
    /// tree's initial value) for standalone / test contexts.
    pub(crate) tree_window_active: bool,
    /// Last `PointerMove` position observed by the tree. Snapshotted
    /// at handler-invocation time so widgets that don't see the live
    /// pointer event (e.g. an `on_hover` callback that fires on the
    /// boundary edge) can still query "where is the cursor right
    /// now". Read by the safe-triangle submenu hover gate.
    pub(crate) tree_pointer_position: Option<teksilo_canvas::Point>,
    /// True when the in-flight pointer press's hit target is a strict
    /// descendant of the widget whose handler is currently running and that
    /// descendant carries its own tap gesture (a chevron, checkbox, inline
    /// button). Set per-node by the dispatcher for `PointerDown`/`PointerUp`.
    /// Read via [`press_claimed_by_interactive_child`](Self::press_claimed_by_interactive_child).
    pub(crate) press_claimed_by_interactive_child: bool,
    /// Per-content-widget overlay bounds — and armed safe-triangle apex,
    /// when the overlay has one — snapshotted at handler invocation. A
    /// flat vec is fine: open overlays are typically 0–3 per tree. Read
    /// by [`EventContext::overlay_bounds_for_content`] and
    /// [`EventContext::overlay_safe_region_armed`].
    pub(crate) overlay_bounds_snapshot: Vec<(
        WidgetId,
        teksilo_canvas::Rect,
        Option<teksilo_canvas::Point>,
    )>,
    /// The widget holding focus when this batch began. Part of the same
    /// per-dispatch snapshot as the two above, and read by
    /// [`focused`](EventContext::focused).
    pub(crate) focused_widget: Option<WidgetId>,
    /// Intents queued by handlers via `send_intent`. Drained by the
    /// tree after event dispatch and routed source-widget → root.
    pub(crate) pending_intents: Vec<crate::intent::Intent>,
    /// The dispatcher sets this to the appropriate
    /// [`IntentSource`](crate::telemetry::IntentSource) before
    /// invoking a typed handler (menu select → `Menu`, AccessKit
    /// action → `Accessibility`, on_tap / button activation →
    /// `Handler`, …). `send_intent` reads it and stamps the intent
    /// before queuing. `None` outside a managed handler — bare
    /// programmatic sends keep their `Intent::source` value
    /// (default `Programmatic`).
    pub(crate) current_source: Option<crate::telemetry::IntentSource>,
    /// Key-capture callback armed via `ctx.begin_key_capture(...)`.
    /// The callback + its shared slot are installed on the tree by
    /// `collect_from_ctx`. Only one per ctx; the last caller wins.
    pub(crate) pending_key_capture: Option<crate::shortcut::KeyCaptureSlot>,
    /// Set to request cancellation of any armed key capture.
    pub(crate) cancel_key_capture: bool,
    /// Deferred mutations to the tree's [`ShortcutRegistry`](crate::shortcut::ShortcutRegistry),
    /// typically issued by settings-UI buttons to rebind or clear
    /// overrides. Applied in `collect_from_ctx` after the handler
    /// returns.
    pub(crate) pending_shortcut_mutations: Vec<ShortcutMutation>,
    /// Requests that the app-level event loop close the window this
    /// tree belongs to. Drained after dispatch via
    /// `WidgetTree::take_close_window_request`. Routed through the
    /// window's close guard (if any) — see
    /// [`WindowConfig::on_close_requested`](crate::window::WindowConfig::on_close_requested).
    pub(crate) close_window_requested: bool,
    /// Like [`close_window_requested`](Self::close_window_requested), but
    /// **bypasses** the window's close guard. Set by
    /// [`close_window_forced`](Self::close_window_forced). Drained after
    /// dispatch via `WidgetTree::take_force_close_request`. The escape
    /// hatch a confirmation dialog uses once the user confirms.
    pub(crate) force_close_requested: bool,
    /// Set by [`request_accessibility_update`](EventContext::request_accessibility_update);
    /// drained in `collect_from_ctx` to set `WidgetTree::a11y_dirty`, forcing the
    /// next `sync_accessibility` to re-walk the AccessKit tree. The general lever for a
    /// composing widget that restructured its subtree in a way that changes the AT tree
    /// (relayout alone no longer re-walks AT).
    pub(crate) request_a11y_update: bool,
    /// Set by [`request_soft_keyboard`](EventContext::request_soft_keyboard);
    /// drained in `collect_from_ctx` onto the tree, from where the app layer
    /// takes it once per dispatch — after the IME-allowance reconcile, which
    /// is the only place that knows whether re-asserting would cancel a live
    /// composition.
    pub(crate) soft_keyboard_request: Option<bool>,
    /// Messages queued by [`announce`](EventContext::announce) /
    /// [`announce_with`](EventContext::announce_with), drained into the tree's
    /// own live regions by `collect_from_ctx`. See [`crate::announcer`].
    pub(crate) announcements: Vec<(String, crate::announcer::Politeness)>,
    /// Layout direction (LTR/RTL) of the hosting tree, snapshotted at
    /// handler-invocation time by `make_event_context`. Read via
    /// [`is_rtl`](EventContext::is_rtl) so pointer / keyboard / drag
    /// handlers can mirror their x-axis logic live — a runtime locale
    /// switch dirties the tree but does **not** rebuild, so direction
    /// must be read here rather than captured at `build()` time.
    /// Defaults to `LeftToRight` for hand-constructed (test) contexts.
    pub(crate) layout_direction: crate::environment::LayoutDirection,
    /// What the tree knows about the sample being dispatched: which pointer
    /// produced it, where it was, and — for a scroll — its phase and source.
    /// Snapshotted by `make_event_context` from the tree's in-flight sample.
    /// Holds its default (a mouse at the epoch) for hand-constructed contexts
    /// and for handlers run outside a pointer dispatch (a timer, an
    /// accessibility action).
    pub(crate) input: crate::pointer::InputSnapshot,
    /// The frozen [`TouchAction`] for the gesture being handled. Defaults to
    /// [`TouchAction::AUTO`] for a hand-constructed context and for every
    /// handler today, since no dispatch path populates this yet — see
    /// [`touch_action`](EventContext::touch_action) and
    /// `crate::pointer::touch_action`.
    pub(crate) touch_action: TouchAction,
    /// The framework press held by the pointer being dispatched, as the router
    /// tracks it: `(inside, pending)`. `None` when that pointer holds no press
    /// — every handler outside a press, and every hand-constructed context.
    /// Read by [`is_pressed`](EventContext::is_pressed) and its two siblings.
    pub(crate) press: Option<(bool, bool)>,
    /// Debug-only WCAG 3.2.1 guard: `Some(flag)` where `flag` is set while a
    /// focus-change dispatch is running. `open_window` / `focus_window` warn if
    /// invoked while it reads `true` (a focus handler changing context). `None`
    /// for hand-constructed (test) contexts.
    pub(crate) in_focus_dispatch: Option<std::rc::Rc<std::cell::Cell<bool>>>,
}

/// One arbitration act a handler performed on its pointer's sequence.
///
/// Queued on the context and applied in order by
/// `WidgetTree::collect_from_ctx`, so a handler that claims and then rejects
/// leaves the sequence in the state its last word describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GestureAct {
    /// [`EventContext::claim_gesture`].
    Claim,
    /// [`EventContext::reject_gesture`].
    Reject,
    /// [`EventContext::hold_gesture`].
    Hold,
    /// [`EventContext::release_gesture`].
    Release,
}

/// Deferred edit to the tree's shortcut registry, queued on an
/// `EventContext` and applied in `collect_from_ctx`.
#[derive(Debug, Clone)]
pub(crate) enum ShortcutMutation {
    RebindPrimary {
        id: String,
        keystroke: Option<crate::shortcut::KeyStroke>,
    },
    RebindSecondary {
        id: String,
        keystroke: Option<crate::shortcut::KeyStroke>,
    },
    ClearOverride {
        id: String,
    },
}

/// A structural change to the widget tree, deferred until after event dispatch.
pub(crate) enum TreeMutation {
    SetDormant(WidgetId),
    Activate(WidgetId),
    Destroy(WidgetId),
    /// Typed mutable access to a mounted widget, applied in
    /// `apply_tree_mutations` where `&mut arena` is live. The boxed closure
    /// downcasts the node's `as_any_mut()` to the requested concrete type;
    /// `dirty` selects the post-mutation re-render level.
    WithWidgetMut {
        id: WidgetId,
        dirty: crate::binding::BindingLevel,
        apply: Box<dyn FnOnce(&mut dyn std::any::Any)>,
    },
    /// Re-run one widget's `build()` **now**, inside `apply_tree_mutations`,
    /// rather than marking it for the next layout pass. See
    /// [`EventContext::materialize_now`].
    MaterializeNow(WidgetId),
    /// `Space` on a data view's focused row: run the keyboard-toggle action
    /// published inside `row`, or `fallback` when the row publishes none.
    ///
    /// Deferred rather than resolved in the handler because finding the action
    /// means walking the row's subtree, and `EventContext` is a command buffer
    /// with no view of the arena. Carrying the fallback keeps the decision in
    /// one place: whether a row has a checkbox is a fact about the tree, not
    /// something the key handler can know.
    RowSpaceActivate {
        row: WidgetId,
        fallback: std::rc::Rc<dyn Fn()>,
    },
}

// Manual `Debug`: the `WithWidgetMut` closure is not `Debug`.
impl std::fmt::Debug for TreeMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetDormant(id) => f.debug_tuple("SetDormant").field(id).finish(),
            Self::Activate(id) => f.debug_tuple("Activate").field(id).finish(),
            Self::MaterializeNow(id) => f.debug_tuple("MaterializeNow").field(id).finish(),
            Self::Destroy(id) => f.debug_tuple("Destroy").field(id).finish(),
            Self::WithWidgetMut { id, dirty, .. } => f
                .debug_struct("WithWidgetMut")
                .field("id", id)
                .field("dirty", dirty)
                .finish_non_exhaustive(),
            Self::RowSpaceActivate { row, .. } => f
                .debug_struct("RowSpaceActivate")
                .field("row", row)
                .finish_non_exhaustive(),
        }
    }
}

impl<'ops> EventContext<'ops> {
    pub(crate) fn new() -> Self {
        Self {
            cursor_request: None,
            tree_mutations: Vec::new(),
            idle_callbacks: Vec::new(),
            modal_requests: Vec::new(),
            dismiss_modal: false,
            overlay_requests: Vec::new(),
            overlay_dismissals: Vec::new(),
            overlay_content_dismissals: Vec::new(),
            overlay_band_requests: Vec::new(),
            overlay_placement_updates: Vec::new(),
            overlay_pause_requests: Vec::new(),
            dismiss_scope: None,
            pointer_capture: None,
            pointer_captor: None,
            dispatch_node: None,
            delayed_overlay_requests: Vec::new(),
            timed_overlay_requests: Vec::new(),
            reveal_overlay_requests: Vec::new(),
            dismiss_descendant_overlays: Vec::new(),
            cancel_delayed_overlays: Vec::new(),
            safe_region_arm_requests: Vec::new(),
            repaint_requests: Vec::new(),
            synthetic_clicks: Vec::new(),
            focus_requests: Vec::new(),
            focus_into_requests: Vec::new(),
            scroll_into_view_requests: Vec::new(),
            scroll_widget_into_view_requests: Vec::new(),
            highlight_tooltip_requests: Vec::new(),
            drag_start_request: None,
            cancel_drag: false,
            drag_is_external: false,
            theme_request: None,
            follow_system_request: false,
            locale_request: None,
            text_scale_request: None,
            frame_requested: false,
            app_context: None,
            pending_intents: Vec::new(),
            current_source: None,
            pending_key_capture: None,
            cancel_key_capture: false,
            pending_shortcut_mutations: Vec::new(),
            close_window_requested: false,
            force_close_requested: false,
            request_a11y_update: false,
            soft_keyboard_request: None,
            announcements: Vec::new(),
            window_ops: None,
            current_window: None,
            tree_window_active: true,
            tree_pointer_position: None,
            press_claimed_by_interactive_child: false,
            overlay_bounds_snapshot: Vec::new(),
            focused_widget: None,
            layout_direction: crate::environment::LayoutDirection::LeftToRight,
            input: crate::pointer::InputSnapshot::default(),
            touch_action: TouchAction::AUTO,
            press: None,
            in_focus_dispatch: None,
            explicit_capture: false,
            recognized_owning_gesture: false,
            drag_activation_override: None,
            gesture_acts: Vec::new(),
            cancel_pointer_request: None,
        }
    }

    /// Record the [`TouchAction`] frozen at press for the sequence owning the
    /// pointer being dispatched. Called by `make_event_context`.
    pub(crate) fn with_touch_action(mut self, action: TouchAction) -> Self {
        self.touch_action = action;
        self
    }

    /// Record the framework press held by the pointer being dispatched, as
    /// `(inside, pending)`. Called by `make_event_context`.
    pub(crate) fn with_press(mut self, press: Option<(bool, bool)>) -> Self {
        self.press = press;
        self
    }

    /// Whether the pointer being dispatched holds a press whose visual is
    /// showing — inside its tap boundary and past any press-feedback delay.
    ///
    /// The framework already drives the pressed node's own
    /// [`pressed_signal`](crate::BuildContext::pressed_signal) from the same
    /// state; this is for a handler that has to *branch* on the press rather
    /// than paint it. `false` outside a press.
    pub fn is_pressed(&self) -> bool {
        matches!(self.press, Some((true, false)))
    }

    /// Whether the pointer being dispatched holds a press that has not left
    /// its tap boundary. Unlike [`is_pressed`](Self::is_pressed) this is still
    /// true during the press-feedback delay: the press is real, only its
    /// visual is being withheld.
    pub fn press_is_inside(&self) -> bool {
        matches!(self.press, Some((true, _)))
    }

    /// Whether the pointer being dispatched holds a press whose feedback delay
    /// has not elapsed — the press is inside a pan claimant and the framework
    /// is waiting to see whether it becomes a scroll.
    pub fn press_pending(&self) -> bool {
        matches!(self.press, Some((_, true)))
    }

    /// Record what the tree knows about the sample being dispatched. Called by
    /// `make_event_context` once per event batch.
    pub(crate) fn with_input_snapshot(mut self, input: crate::pointer::InputSnapshot) -> Self {
        self.input = input;
        self
    }

    /// Record who holds the capture of the pointer being dispatched, so
    /// [`owns_pointer`](Self::owns_pointer) can answer without a tree lookup.
    pub(crate) fn with_pointer_captor(mut self, captor: Option<WidgetId>) -> Self {
        self.pointer_captor = captor;
        self
    }

    /// Record which node's handler is about to run.
    pub(crate) fn with_dispatch_node(mut self, node: WidgetId) -> Self {
        self.dispatch_node = Some(node);
        self
    }

    /// The pointer that produced the event being handled.
    ///
    /// Two dispatches have a pointer without having a sample, and both report
    /// it: a gesture the timer recognised — a hold — reports the **contact that
    /// held**, and a drag-and-drop handler (`on_drag_hover` / `on_drag_tick` /
    /// `on_drag_leave` / `on_drop`) reports the pointer **that started the
    /// drag**, which is what makes it right inside a tick fired from a layout
    /// pass or an OS drag phase delivered from a platform thread. Outside any
    /// pointer, scroll, gesture or drag dispatch — an assistive-technology
    /// action, a hand-constructed test context — this is the mouse at the tree
    /// epoch, which is the same answer every such handler got before pointers
    /// were distinguishable.
    pub fn pointer(&self) -> crate::pointer::PointerInfo {
        self.input.pointer
    }

    /// What kind of device is pointing: mouse, finger, stylus.
    ///
    /// The one question most handlers actually need — it is what decides
    /// whether a hover affordance is reachable, whether a target needs slop,
    /// and which gesture profile governs.
    pub fn pointer_kind(&self) -> teksilo_tokens::PointerKind {
        self.input.pointer.kind
    }

    /// Where the pointer was, in window-logical coordinates, when the event
    /// being handled was produced.
    ///
    /// `None` for an event that carries no position — a keyboard-driven
    /// scroll, a wheel notch (which routes by hover rather than by position),
    /// anything dispatched outside a pointer sample. Distinct from
    /// [`tree_pointer_position`](Self::tree_pointer_position), which reports
    /// where the pointer is *at this instant* regardless of what is being
    /// dispatched.
    pub fn pointer_position(&self) -> Option<teksilo_canvas::Point> {
        self.input.position
    }

    /// Where in a continuous scroll gesture the event being handled sits.
    ///
    /// [`ScrollPhase::Discrete`](crate::pointer::ScrollPhase::Discrete) — a
    /// self-contained wheel notch — for everything that is not a phased
    /// gesture, which is every scroll Teksilo produced before the touch
    /// programme.
    pub fn scroll_phase(&self) -> crate::pointer::ScrollPhase {
        self.input.scroll_phase
    }

    /// What produced the scroll being handled: a notched wheel, a precision
    /// trackpad, a synthesised touch pan, or the app itself.
    pub fn scroll_source(&self) -> crate::pointer::ScrollSource {
        self.input.scroll_source
    }

    /// The [`TouchAction`] governing the gesture being handled.
    ///
    /// The value is **frozen at press** for the whole gesture's lifetime: the
    /// router computes it once, from `WidgetTree::effective_touch_action` of
    /// the pressed target, and stores it on that pointer's
    /// [`PointerSequence`](crate::gesture::PointerSequence), so a handler never
    /// re-reads a subtree that may have rebuilt mid-gesture.
    ///
    /// [`TouchAction::AUTO`] — the neutral value — outside a press, and for a
    /// hand-constructed context. A mouse never consults this at all. See
    /// `crate::pointer::touch_action`.
    pub fn touch_action(&self) -> TouchAction {
        self.touch_action
    }

    /// Snapshot the hosting tree's layout direction. Called by
    /// `make_event_context` once per event batch so x-axis handlers
    /// (resize, drag-reorder, arrow-key navigation) can mirror under
    /// RTL without a rebuild.
    pub(crate) fn with_layout_direction(
        mut self,
        direction: crate::environment::LayoutDirection,
    ) -> Self {
        self.layout_direction = direction;
        self
    }

    /// Layout direction of the hosting tree at dispatch time.
    pub fn layout_direction(&self) -> crate::environment::LayoutDirection {
        self.layout_direction
    }

    /// Whether the hosting tree is laid out right-to-left. Mirrors
    /// [`LayoutContext::is_rtl`](crate::widget::LayoutContext::is_rtl)
    /// for the event-dispatch side.
    pub fn is_rtl(&self) -> bool {
        self.layout_direction == crate::environment::LayoutDirection::RightToLeft
    }

    /// The widget that held focus when this event batch began.
    ///
    /// A snapshot, not a live read: it answers what the tree's focus was at
    /// dispatch time, so a handler that has already called
    /// [`request_focus`](EventContext::request_focus) still sees the old
    /// value. That is the useful reading for a handler deciding *whether* to
    /// act on the focused widget.
    ///
    /// `None` when nothing is focused, and also for an `EventContext` built
    /// outside `WidgetTree::make_event_context`, which is what a hand-made
    /// test context is. Treat it as `None`-safe, like the other snapshots.
    ///
    /// The reason this exists: a widget-scoped shortcut fires before the
    /// focused widget sees the key, so a container that binds a key which its
    /// own children also handle has no other way to yield to them.
    /// `MessageBox` is the case that asked for it, where Enter is bound to the
    /// default button and must not answer for the button the user has actually
    /// tabbed to.
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused_widget
    }

    /// Attach a per-dispatch snapshot of read-only tree query state
    /// (current pointer position, overlay bounds, focus). Called by
    /// `WidgetTree::make_event_context` once per event batch. Test
    /// `EventContext`s that don't go through that path stay with
    /// empty snapshots — handlers must treat every read as `None`-
    /// safe.
    pub(crate) fn with_query_snapshot(
        mut self,
        pointer: Option<teksilo_canvas::Point>,
        overlays: Vec<(
            WidgetId,
            teksilo_canvas::Rect,
            Option<teksilo_canvas::Point>,
        )>,
        focused: Option<WidgetId>,
    ) -> Self {
        self.tree_pointer_position = pointer;
        self.overlay_bounds_snapshot = overlays;
        self.focused_widget = focused;
        self
    }

    /// Attach the app-level window-ops sink and the hosting tree's
    /// [`WindowState`](crate::window::WindowState). Called by the
    /// dispatcher once per event batch so handlers can reach the
    /// multi-window API synchronously.
    pub(crate) fn with_window_context(
        mut self,
        ops: &'ops mut dyn crate::window::WindowOps,
        current_window: Option<crate::window::WindowState>,
    ) -> Self {
        self.window_ops = Some(ops);
        self.current_window = current_window;
        self
    }

    /// Snapshot the tree's occlusion-aware window-active state. Called by
    /// `make_event_context` once per event batch so handlers can read
    /// [`window_active`](Self::window_active).
    pub(crate) fn with_window_active(mut self, active: bool) -> Self {
        self.tree_window_active = active;
        self
    }

    /// Attach the tree's shared "inside focus dispatch" flag (WCAG 3.2.1
    /// debug guard). See [`EventContext::open_window`].
    pub(crate) fn with_focus_dispatch_flag(
        mut self,
        flag: std::rc::Rc<std::cell::Cell<bool>>,
    ) -> Self {
        self.in_focus_dispatch = Some(flag);
        self
    }

    /// Debug-only: warn (once per call) if a context change is being made from
    /// inside a focus-change dispatch — a WCAG 3.2.1 (On Focus) anti-pattern.
    /// Compiled out entirely in release builds.
    #[inline]
    fn warn_if_context_change_in_focus_dispatch(&self, what: &str) {
        #[cfg(debug_assertions)]
        if self.in_focus_dispatch.as_ref().is_some_and(|f| f.get()) {
            eprintln!(
                "[teksilo a11y] WCAG 3.2.1 (On Focus): `{what}` was called from \
                 inside an on_focus handler. Changing context (opening/focusing a \
                 window, navigating) merely because a control received focus \
                 surprises keyboard users tabbing through the UI. Move this to an \
                 explicit activation handler (on_tap / on_activate / a shortcut)."
            );
        }
        let _ = what;
    }

    /// Attach the tree's app-state registry so handlers can look up
    /// application-scoped values (`ClipboardHandle`, `SharedTypesetter`,
    /// …). Called by the dispatcher once per event batch.
    pub(crate) fn with_app_context(
        mut self,
        ctx: std::rc::Rc<crate::event_source::TreeAppContext>,
    ) -> Self {
        self.app_context = Some(ctx);
        self
    }

    /// Record whether the drag session active while this context is live
    /// originated from an external (OS) drag. Set by `make_event_context`.
    pub(crate) fn with_drag_external(mut self, is_external: bool) -> Self {
        self.drag_is_external = is_external;
        self
    }

    /// Whether a drag is currently in flight that was started by an external
    /// (OS) drag-and-drop (files / text / URLs from another application),
    /// rather than by an in-app `start_drag`. Useful in `on_drag_leave` /
    /// `on_drag_tick` handlers, which don't receive the payload directly;
    /// in `on_drag_hover` / `on_drop` prefer `payload.is_external()`.
    pub fn drag_is_external(&self) -> bool {
        self.drag_is_external
    }

    /// Look up an application-scoped value by type. Mirrors
    /// `BuildContext::app_state`. Returns `None` when the handler was
    /// invoked without a registry (hand-constructed `EventContext` in
    /// tests, or when no value of that type was registered).
    pub fn app_state<T: 'static>(&self) -> Option<&T> {
        self.app_context
            .as_ref()
            .and_then(|ctx| ctx.app_state::<T>())
    }

    /// Borrow the [`AppEventPoster`](crate::AppEventPoster) installed
    /// by the framework. Used by integrations that need to post
    /// typed payloads back to the UI loop from a worker thread
    /// (`teksilo_platform::file_dialog`'s `RfdAsyncBackend`, future
    /// async-result features). Returns `None` for hand-constructed
    /// `EventContext`s in tests.
    pub fn poster(&self) -> Option<&std::sync::Arc<dyn crate::AppEventPoster>> {
        self.app_context.as_ref().and_then(|ctx| ctx.poster())
    }

    /// Ask the tree to pump one more frame after this handler returns.
    /// Use from event handlers that kick off per-frame work (pending
    /// document events to drain, drag-select auto-scroll, caret blink
    /// restart on focus). See `WidgetTree::request_frame` for the
    /// draw-when-needed contract.
    pub fn request_frame(&mut self) {
        self.frame_requested = true;
    }

    /// Dispatch an [`Intent`](crate::intent::Intent) as if the source
    /// widget pressed its keyboard shortcut. The framework walks
    /// source-widget → root after the current handler returns,
    /// invoking any matching [`Action`](crate::action::Action) it
    /// finds. Unmatched intents are silently dropped.
    ///
    /// The intent's `source` is overridden by the dispatcher's
    /// current handler-source label (`current_source`) when one is
    /// active. This is how the framework distinguishes
    /// `IntentSource::Handler` (button taps, generic on_tap) from
    /// `IntentSource::Menu`, `IntentSource::Accessibility`, etc.
    /// Programmatic callers outside any handler pass through with
    /// `IntentSource::Programmatic` (the default).
    pub fn send_intent(&mut self, intent: impl Into<crate::intent::Intent>) {
        let mut intent: crate::intent::Intent = intent.into();
        if let Some(src) = self.current_source {
            intent.source = src;
        }
        self.pending_intents.push(intent);
    }

    /// Run a closure with the given `IntentSource` active. Any
    /// `ctx.send_intent(...)` issued from within the closure will
    /// be tagged with this source instead of the dispatcher's
    /// default (`Handler` / `Shortcut` / `Accessibility`).
    ///
    /// The previous source is restored after the closure returns.
    /// Panic during the closure unwinds the dispatcher's whole
    /// frame, so the EventContext is destroyed before the next
    /// dispatch — no need for a panic-safe drop guard.
    ///
    /// Used by framework widgets that want a more specific source
    /// label than the default — `MenuItem` wraps its activation
    /// handler to emit `IntentSource::Menu`, etc.
    pub fn with_intent_source<R>(
        &mut self,
        source: crate::telemetry::IntentSource,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = self.current_source.replace(source);
        let r = f(self);
        self.current_source = prev;
        r
    }

    /// Arm a one-shot key-capture callback, returning a
    /// [`CaptureHandle`](crate::shortcut::CaptureHandle) whose `Drop`
    /// cancels the capture if it hasn't fired yet. The next `KeyDown`
    /// bypasses shortcut resolution and invokes the callback with:
    /// - the captured [`KeyStroke`](crate::shortcut::KeyStroke)
    /// - mutable access to the registry (rebinds in-place)
    /// - a mutable [`EventContext`] (emit commands, send intents,
    ///   dismiss overlays, …)
    ///
    /// The handle must be stored somewhere with an appropriate
    /// lifetime (typically in the calling widget's state) or the
    /// capture will be cancelled immediately when the returned
    /// handle drops at end of scope.
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
        self.pending_key_capture = Some(slot.clone());
        self.cancel_key_capture = false;
        crate::shortcut::CaptureHandle::new(slot)
    }

    /// Cancel any key capture armed earlier in this handler or via
    /// `WidgetTree::begin_key_capture` before the handler ran.
    pub fn cancel_key_capture(&mut self) {
        self.pending_key_capture = None;
        self.cancel_key_capture = true;
    }

    /// Queue a deferred rebind of the primary keystroke for the
    /// registered shortcut with the given id. Applied by the tree
    /// after the current handler returns. Use `None` to explicitly
    /// unbind the slot.
    pub fn rebind_shortcut_primary(
        &mut self,
        id: impl Into<String>,
        keystroke: Option<crate::shortcut::KeyStroke>,
    ) {
        self.pending_shortcut_mutations
            .push(ShortcutMutation::RebindPrimary {
                id: id.into(),
                keystroke,
            });
    }

    /// Queue a deferred rebind of the secondary keystroke for the
    /// registered shortcut with the given id.
    pub fn rebind_shortcut_secondary(
        &mut self,
        id: impl Into<String>,
        keystroke: Option<crate::shortcut::KeyStroke>,
    ) {
        self.pending_shortcut_mutations
            .push(ShortcutMutation::RebindSecondary {
                id: id.into(),
                keystroke,
            });
    }

    /// Queue a deferred clear of any user override for the given
    /// shortcut id, restoring its declared defaults.
    pub fn clear_shortcut_override(&mut self, id: impl Into<String>) {
        self.pending_shortcut_mutations
            .push(ShortcutMutation::ClearOverride { id: id.into() });
    }

    /// Request that the application close the window this tree
    /// belongs to. Drained by the app event loop after the handler
    /// returns. Typical use: title-bar close button handlers.
    ///
    /// This is a *guarded* close: if the window declared a close guard
    /// via
    /// [`WindowConfig::on_close_requested`](crate::window::WindowConfig::on_close_requested)
    /// or [`can_close`](crate::window::WindowConfig::can_close), that
    /// guard runs first and may veto the close. To skip the guard (e.g.
    /// from the confirmation dialog the guard itself opened), use
    /// [`close_window_forced`](Self::close_window_forced).
    pub fn close_window(&mut self) {
        self.close_window_requested = true;
    }

    /// Request that the application close this tree's window
    /// **unconditionally**, bypassing any close guard declared via
    /// [`WindowConfig::on_close_requested`](crate::window::WindowConfig::on_close_requested)
    /// / [`can_close`](crate::window::WindowConfig::can_close).
    ///
    /// This is the second half of the veto-then-reissue pattern: the
    /// guard returns [`CloseResponse::Veto`](crate::window::CloseResponse::Veto)
    /// and opens a confirmation dialog; the dialog's "close anyway"
    /// button calls `close_window_forced` so the window actually closes
    /// without re-triggering the guard.
    pub fn close_window_forced(&mut self) {
        self.force_close_requested = true;
    }

    // -------------------- Multi-window API --------------------

    /// The [`WindowState`](crate::window::WindowState) for the window
    /// hosting this handler. `None` only for handlers run outside
    /// of an app (hand-constructed `EventContext` in tests).
    /// Cursor position at the moment this handler was invoked. `None`
    /// when no `PointerMove` has reached the tree yet, or when the
    /// context was constructed without a tree-side snapshot (e.g.
    /// hand-built `EventContext`s in tests). Used by the safe-triangle
    /// submenu hover gate.
    pub fn tree_pointer_position(&self) -> Option<teksilo_canvas::Point> {
        self.tree_pointer_position
    }

    /// True when the in-flight pointer press's hit target is a strict
    /// descendant of THIS handler's widget that carries its own tap gesture
    /// (chevron, checkbox, inline button). A row/container that selects on
    /// press should early-return `EventResponse::Ignored` when this is set, so
    /// the press belongs to the inner control, not the row. Only meaningful
    /// inside `on_pointer_event` handlers for `PointerDown`/`PointerUp`.
    pub fn press_claimed_by_interactive_child(&self) -> bool {
        self.press_claimed_by_interactive_child
    }

    /// Look up the bounds rect of an open overlay by its root content
    /// widget id. Returns `None` when no such overlay is currently
    /// active. The snapshot is taken once per dispatch; mid-handler
    /// `show_overlay` calls will not appear here. Used by the
    /// safe-triangle submenu hover gate.
    pub fn overlay_bounds_for_content(&self, content_id: WidgetId) -> Option<teksilo_canvas::Rect> {
        self.overlay_bounds_snapshot
            .iter()
            .find(|(cid, _, _)| *cid == content_id)
            .map(|(_, r, _)| *r)
    }

    /// Whether a safe-triangle traversal toward the overlay rooted at
    /// `content_id` is still live — i.e. whether the user may still be on
    /// their way to that submenu.
    ///
    /// A widget whose hover would otherwise tear the overlay down (a
    /// sibling menu row switching the selection) asks this first and
    /// stands aside while it is `true`, leaving the dismissal to the
    /// overlay's own pointer-leave grace — which tests the cone on every
    /// sample and closes the overlay one `delay` after the pointer stops
    /// heading there.
    ///
    /// **This is deliberately the armed window, not a point-in-cone
    /// test.** A sibling row's hover fires exactly once, at the instant
    /// the pointer crosses onto it — a pixel or two from the apex, where
    /// the cone is a needle — so answering "is this one sample inside the
    /// cone" made a single quantized step final, and any departure
    /// steeper than the cone (which is most of them, for a wide menu with
    /// a short submenu) killed the submenu the moment the pointer left
    /// the trigger row. Whether *this* sample is inside the cone is the
    /// framework's question, asked continuously; the widget's question is
    /// only whether to get out of the way.
    ///
    /// `false` when no region is armed and when its budget is spent.
    ///
    /// Arm the region with
    /// [`arm_overlay_safe_region`](Self::arm_overlay_safe_region).
    pub fn overlay_safe_region_armed(&self, content_id: WidgetId) -> bool {
        self.overlay_bounds_snapshot
            .iter()
            .find(|(cid, _, _)| *cid == content_id)
            .is_some_and(|(_, _, apex)| apex.is_some())
    }

    pub fn window(&self) -> Option<&crate::window::WindowState> {
        self.current_window.as_ref()
    }

    /// Whether the host window is currently active (`focused AND not
    /// occluded`) — the occlusion-aware companion to
    /// `self.window().map(|w| w.focused())` (raw OS focus). Snapshotted at
    /// context construction. Matches [`BuildContext::window_active`].
    ///
    /// [`BuildContext::window_active`]: crate::build_context::BuildContext::window_active
    pub fn window_active(&self) -> bool {
        self.tree_window_active
    }

    /// Open a new window, creating the winit-level surface
    /// synchronously. The returned id is immediately valid for
    /// [`focus_window`](Self::focus_window),
    /// [`window_state`](Self::window_state), and
    /// [`find_window`](Self::find_window).
    ///
    /// Panics when called from a handler on a standalone `WidgetTree`
    /// (no app context) — tests should not invoke this method.
    pub fn open_window(
        &mut self,
        config: crate::window::WindowConfig,
    ) -> crate::window::TeksiloWindowId {
        self.warn_if_context_change_in_focus_dispatch("open_window");
        self.window_ops
            .as_deref_mut()
            .expect("open_window called outside of a dispatch")
            .open_window(config)
    }

    /// Find a window by the string id assigned via
    /// [`WindowConfig::id`](crate::window::WindowConfig::id). Returns
    /// `None` if no open window carries that id.
    pub fn find_window(&self, string_id: &str) -> Option<crate::window::TeksiloWindowId> {
        self.window_ops.as_deref()?.find_window(string_id)
    }

    /// Read the [`WindowState`](crate::window::WindowState) for a
    /// specific window.
    pub fn window_state(
        &self,
        id: crate::window::TeksiloWindowId,
    ) -> Option<crate::window::WindowState> {
        self.window_ops.as_deref()?.window_state(id)
    }

    /// Snapshot of every live window's state.
    pub fn windows(&self) -> Vec<crate::window::WindowState> {
        self.window_ops
            .as_deref()
            .map(|o| o.windows())
            .unwrap_or_default()
    }

    /// Raise a window to the front and give it keyboard focus.
    pub fn focus_window(&mut self, id: crate::window::TeksiloWindowId) {
        self.warn_if_context_change_in_focus_dispatch("focus_window");
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.focus_window(id);
        }
    }

    /// Request an xdg-activation token for `id` (see
    /// [`WindowOps::request_activation_token`](crate::window::WindowOps::request_activation_token)).
    /// `cb` fires once with the token string, or `None` where the platform can't
    /// provide one — used to hand a token to a child process ("open in new
    /// window") or an IPC peer that will raise itself on Wayland.
    pub fn request_activation_token(
        &mut self,
        id: crate::window::TeksiloWindowId,
        cb: Box<dyn FnOnce(Option<String>)>,
    ) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.request_activation_token(id, cb);
        } else {
            cb(None);
        }
    }

    /// Request an activation token for the **current** window (see
    /// [`WindowOps::request_activation_token_self`](crate::window::WindowOps::request_activation_token_self)).
    /// Use this from a widget handler to mint a token from *this* focused window
    /// to hand to another window or process — it works mid-dispatch, unlike the
    /// id-based variant.
    pub fn request_activation_token_self(&mut self, cb: Box<dyn FnOnce(Option<String>)>) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.request_activation_token_self(cb);
        } else {
            cb(None);
        }
    }

    /// Close a specific window by id. Equivalent to
    /// [`close_window`](Self::close_window) when `id` is the current
    /// window's id.
    pub fn close_window_by_id(&mut self, id: crate::window::TeksiloWindowId) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.close_window_by_id(id);
        }
    }

    /// Report the focused text widget's caret rectangle (window-logical
    /// pixels) so the platform can position the OS IME candidate window at
    /// the insertion point. Text-editing widgets call this whenever the
    /// caret moves. No-op outside a dispatch / on a standalone tree.
    pub fn set_ime_cursor_area(&mut self, area: teksilo_canvas::Rect) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.set_ime_cursor_area(area);
        }
    }

    /// Resolve the platform parent handle of the window currently
    /// dispatching the event. Used by native-dialog integrations
    /// (`teksilo_platform::file_dialog`) to parent OS dialogs to the
    /// originating Teksilo window.
    ///
    /// Returns `None` when called from a standalone `WidgetTree` (no
    /// app-level `WindowOps` sink), or when the platform refuses to
    /// surface a handle (rare; mostly during teardown).
    pub fn parent_window_handle(&self) -> Option<crate::raw_handle::ParentHandle> {
        self.window_ops.as_deref()?.current_parent_handle()
    }

    /// Request a cursor icon change.
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor_request = Some(cursor);
    }

    /// Set a widget subtree as dormant (preserves state, releases rendering).
    pub fn set_dormant(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::SetDormant(id));
    }

    /// Activate a dormant widget subtree.
    pub fn activate(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::Activate(id));
    }

    /// Destroy a widget subtree (removes from arena entirely, state is gone).
    pub fn destroy(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::Destroy(id));
    }

    /// Imperatively mutate a mounted widget by id, downcasting to the
    /// concrete type `W`.
    ///
    /// The mutation is **deferred**: the closure runs after the handler
    /// returns, inside `apply_tree_mutations`, where the framework holds
    /// `&mut` arena access (a handler cannot re-borrow the arena to reach
    /// another node, so this is the only safe channel — the same model as
    /// [`destroy`](Self::destroy)). After the closure runs, the target is
    /// dirty-marked at `dirty` so the mutation takes visual effect.
    ///
    /// The target widget must override `Widget::as_any_mut` to return
    /// `Some(self)`. If the id is gone or is not a `W`, the closure is a
    /// no-op in release and a `debug_assert` failure in debug — it never
    /// silently mutates the wrong widget.
    ///
    /// Use it for per-view state a handler can't otherwise reach — e.g.
    /// `SceneView::ensure_visible(...)` (camera) after the view is mounted:
    /// ```ignore
    /// ctx.with_widget_mut::<SceneView>(view_id, BindingLevel::Relayout, |v| {
    ///     v.ensure_visible(card_rect, 40.0);
    /// });
    /// ```
    /// For scene *content*, prefer the shared `SceneModel` handle (`view.model()`)
    /// — its mutators are `&self`, so a handler holding a clone can drive the
    /// scene directly and every attached view reconciles, no `with_widget_mut`
    /// needed.
    pub fn with_widget_mut<W: 'static>(
        &mut self,
        id: WidgetId,
        dirty: crate::binding::BindingLevel,
        f: impl FnOnce(&mut W) + 'static,
    ) {
        self.tree_mutations.push(TreeMutation::WithWidgetMut {
            id,
            dirty,
            apply: Box::new(move |any| match any.downcast_mut::<W>() {
                Some(w) => f(w),
                None => debug_assert!(
                    false,
                    "with_widget_mut: widget {id:?} is not the requested type (or does not \
                     override Widget::as_any_mut)"
                ),
            }),
        });
    }

    /// Re-run one widget's `build()` **during this handler's drain**, before
    /// overlays are shown and before focus requests are applied — rather than
    /// dirty-marking it for the next layout pass, which is what every other
    /// rebuild trigger does.
    ///
    /// It exists for one shape:
    /// [`DeferredSubtree`](crate::deferred_subtree::DeferredSubtree) content
    /// that a handler is *about to depend on*. Opening a popover activates its
    /// content, shows an overlay anchored to it, and moves focus into it — all
    /// three inside the same drain (see `collect_from_ctx`). A deferred panel
    /// marked for rebuild would not exist yet at any of those points: the
    /// overlay would be measured against an empty node and
    /// `first_focusable_descendant` would find nothing to focus, so the popover
    /// would open in the wrong place and swallow the keyboard. Materializing
    /// here closes that window, and makes deferred content behave exactly like
    /// the eagerly-built content it replaces.
    ///
    /// Cheap to call redundantly: a `DeferredSubtree` that is already
    /// materialized returns its existing child, so a second open costs one
    /// `build()` of the host and nothing below it.
    ///
    /// Not a general "rebuild this widget now" door — reach for
    /// [`with_widget_mut`](Self::with_widget_mut) or a `Rebuild` binding for
    /// ordinary reactive updates, which are correctly served by the next
    /// layout pass.
    /// `Space` on a data view's focused row: activate the row's published
    /// keyboard toggle — the checkbox `StandardListItem` embeds, most often —
    /// or run `fallback` when the row publishes none.
    ///
    /// A row's controls are out of the Tab order, so this is the only keyboard
    /// route to them; `fallback` is what `Space` means on a row without one,
    /// which for the data views is "toggle the selection".
    pub fn row_space_activate(&mut self, row: WidgetId, fallback: std::rc::Rc<dyn Fn()>) {
        self.tree_mutations
            .push(TreeMutation::RowSpaceActivate { row, fallback });
    }

    pub fn materialize_now(&mut self, id: WidgetId) {
        self.tree_mutations.push(TreeMutation::MaterializeNow(id));
    }

    /// Request that the AccessKit tree be re-walked after this handler
    /// returns. Use after a mutation that changes the accessibility tree
    /// **shape** in a way the framework doesn't already detect (relayout
    /// alone no longer re-walks AT; only events that change the AT tree
    /// — focus, overlays, locale/shortcut rebinds — set the dirty flag).
    /// The companion `BuildContext::request_accessibility_update` covers
    /// the build-time path.
    pub fn request_accessibility_update(&mut self) {
        self.request_a11y_update = true;
    }

    /// Ask the platform to raise its on-screen keyboard.
    ///
    /// For the case the desktop convention has no answer for: a *finger*
    /// landing in a text field, where there is no physical keyboard and no
    /// focus change the accessibility layer would notice on its own.
    ///
    /// The request is honoured **only where it can do no harm**. Where the
    /// platform's keyboard follows the framework's IME-allowance reconcile
    /// ([`SoftKeyboardSupport::ViaAccessibility`](crate::window::SoftKeyboardSupport::ViaAccessibility)),
    /// the request resolves to nothing — always, not merely while a composition
    /// happens to be live. That reconcile *is* the request, and the only thing
    /// an explicit ask could add is a re-assertion of IME allowance, which is
    /// what cancels a composition mid-word. Nothing on this path calls
    /// `set_ime_allowed`, and that is what makes placing a caret with a finger
    /// mid-composition safe. Where the framework has no keyboard request to
    /// send at all the request is dropped; ask
    /// [`soft_keyboard_support`](Self::soft_keyboard_support) first if the
    /// widget needs to offer a fallback.
    pub fn request_soft_keyboard(&mut self) {
        self.soft_keyboard_request = Some(true);
    }

    /// Ask the platform to dismiss its on-screen keyboard.
    ///
    /// Only a platform reporting
    /// [`SoftKeyboardSupport::Explicit`](crate::window::SoftKeyboardSupport::Explicit)
    /// can honour this; elsewhere there is no dismiss request to send, and a
    /// keyboard that rose on the IME enable goes away on the matching disable
    /// when focus leaves the text surface.
    pub fn dismiss_soft_keyboard(&mut self) {
        self.soft_keyboard_request = Some(false);
    }

    /// What the host platform can do about an on-screen keyboard.
    ///
    /// [`SoftKeyboardSupport::None`](crate::window::SoftKeyboardSupport::None)
    /// on a standalone tree and on every platform the framework has no keyboard
    /// request to send on — which, on the desktop, is most of them.
    pub fn soft_keyboard_support(&self) -> crate::window::SoftKeyboardSupport {
        self.window_ops
            .as_deref()
            .map(|ops| ops.soft_keyboard_support())
            .unwrap_or_default()
    }

    /// Speak `message` to the screen reader, politely.
    ///
    /// For anything the user needs told that is not the name of a widget: a
    /// completed action, a new count, the result of an undo, a row that moved.
    /// Sighted users read those off the screen; a screen-reader user is told
    /// only what the framework says out loud.
    ///
    /// ```ignore
    /// ctx.announce(tr!(event_added(title = title.clone())));
    /// ```
    ///
    /// Takes `impl Into<String>`, so `tr!(…)` works directly.
    /// `LocalizedString` is deliberately not the parameter type: an
    /// announcement is an event, not a label, and
    /// re-resolving it on a later language switch would re-speak it. See
    /// [`crate::announcer`].
    ///
    /// **Do not pair this with a toast on the same path.** `Toast` is already a
    /// correct live region, so doing both says everything twice.
    pub fn announce(&mut self, message: impl Into<String>) {
        self.announce_with(message, crate::announcer::Politeness::Polite);
    }

    /// Speak `message` to the screen reader at the given urgency.
    ///
    /// [`Politeness::Assertive`](crate::announcer::Politeness::Assertive)
    /// interrupts whatever is being spoken. Reserve it for something the user
    /// must not miss and cannot recover by re-reading the screen — a failure, a
    /// refusal, a destructive result. Everything else is
    /// [`Polite`](crate::announcer::Politeness::Polite), which is what
    /// [`announce`](Self::announce) uses.
    pub fn announce_with(
        &mut self,
        message: impl Into<String>,
        politeness: crate::announcer::Politeness,
    ) {
        self.announcements.push((message.into(), politeness));
    }

    /// Show an overlay (tooltip, menu, popover).
    pub fn show_overlay(&mut self, request: crate::overlay::OverlayRequest) {
        self.overlay_requests.push(request);
    }

    /// Show an overlay in an explicit z-band.
    ///
    /// [`show_overlay`](Self::show_overlay) is this with
    /// [`Standard`](crate::overlay::OverlayBand::Standard). The other band is
    /// for the touch text affordances, which must render above the editor's
    /// `clips_children` ancestor, below every menu, and outside the
    /// outside-press dismissal that every caret-moving tap would otherwise
    /// trigger. Their lifetime is the controller's — see
    /// [`TouchSelection::dismiss`](crate::text_touch::TouchSelection::dismiss).
    ///
    /// Showing content that is already up is a no-op, so a host may call this
    /// on every raise without tracking whether it has.
    pub fn show_overlay_in_band(
        &mut self,
        request: crate::overlay::OverlayRequest,
        band: crate::overlay::OverlayBand,
    ) {
        self.overlay_band_requests.push((request, band));
    }

    /// Re-place the currently-shown overlay whose content root is `content_id`.
    ///
    /// Content-keyed for the same reason
    /// [`dismiss_overlay_by_content`](Self::dismiss_overlay_by_content) is:
    /// [`show_overlay`](Self::show_overlay) returns nothing, so a handler
    /// cannot learn the [`OverlayId`](crate::overlay::OverlayId) it created. A
    /// no-op when no overlay is showing that content.
    pub fn update_overlay_placement_by_content(
        &mut self,
        content_id: crate::widget_id::WidgetId,
        placement: crate::overlay::OverlayPlacement,
    ) {
        self.overlay_placement_updates.push((content_id, placement));
    }

    /// Show an overlay whose reveal/dismiss is animated by a
    /// caller-owned progress signal.
    ///
    /// `progress` must be an animated `Signal<f32>` (created with
    /// [`Signal::new_animated`](crate::signal::Signal::new_animated) or
    /// [`BuildContext::animated_signal`](crate::build_context::BuildContext::animated_signal)).
    /// The framework shows the overlay, tweens `progress` 0 → 1 over
    /// `duration`, and on any dismiss path tweens it 1 → 0 while
    /// **deferring** the overlay's removal (and its content's dormancy)
    /// until the roll-back completes — the same window the fade path
    /// uses, but with no opacity applied. The caller binds `progress`
    /// to whatever paints the reveal (e.g. an
    /// [`Unroll`](https://docs.rs/teksilo) width), and is responsible for
    /// resetting it to `0.0` before the show if a prior reveal left it
    /// at `1.0`.
    ///
    /// Under `prefers-reduced-motion`, skip this and use
    /// [`show_overlay`](Self::show_overlay) with the progress pinned at
    /// `1.0` so there is no tween and dismissal is immediate.
    pub fn show_overlay_with_reveal(
        &mut self,
        request: crate::overlay::OverlayRequest,
        progress: crate::signal::Signal<f32>,
        duration: std::time::Duration,
    ) {
        self.reveal_overlay_requests
            .push((request, progress, duration));
    }

    /// Show an overlay that dismisses automatically after `duration`.
    pub fn show_overlay_for(
        &mut self,
        request: crate::overlay::OverlayRequest,
        duration: std::time::Duration,
    ) {
        self.timed_overlay_requests.push((request, duration));
    }

    /// Dismiss an overlay by ID.
    pub fn dismiss_overlay(&mut self, id: crate::overlay::OverlayId) {
        self.overlay_dismissals.push(id);
    }

    /// Dismiss the currently-shown overlay whose content root is
    /// `content_id`, if one is active. No-op when no overlay is showing
    /// that content. Use this to dismiss an overlay you can only name by
    /// its content widget — the symmetric companion to
    /// [`cancel_delayed_overlay`](Self::cancel_delayed_overlay), which
    /// cancels a *pending* delayed show for the same content. Together
    /// they let a caller fully retract a reusable tooltip surface
    /// (shown or pending) without tracking the `OverlayId`.
    pub fn dismiss_overlay_by_content(&mut self, content_id: crate::widget_id::WidgetId) {
        self.overlay_content_dismissals.push(content_id);
    }

    /// Queue a request to pause an overlay's `auto_dismiss_after`
    /// timer. Drained by the framework after this handler returns —
    /// equivalent to calling
    /// [`OverlayManager::pause_auto_dismiss`](crate::overlay::OverlayManager::pause_auto_dismiss)
    /// at the next safe point. Idempotent.
    ///
    /// Used by `ToastHost` for hover-pause: on pointer-enter the
    /// host queues `pause_overlay_auto_dismiss(id)` for every live
    /// toast; on pointer-leave it queues `resume_overlay_auto_dismiss`.
    pub fn pause_overlay_auto_dismiss(&mut self, id: crate::overlay::OverlayId) {
        self.overlay_pause_requests.push((id, true));
    }

    /// Queue a request to resume an overlay's `auto_dismiss_after`
    /// timer paused via
    /// [`pause_overlay_auto_dismiss`](Self::pause_overlay_auto_dismiss).
    /// Idempotent on un-paused overlays.
    pub fn resume_overlay_auto_dismiss(&mut self, id: crate::overlay::OverlayId) {
        self.overlay_pause_requests.push((id, false));
    }

    /// Dismiss all active overlays (e.g., after a menu item is activated).
    pub fn dismiss_all_overlays(&mut self) {
        self.dismiss_scope = Some(DismissScope::All);
    }

    /// Dismiss the source widget's containing overlay and any ancestor
    /// overlays in the chain that are menu-like (anything that isn't a
    /// `Role::Tooltip`, `Role::Dialog`, or `Role::AlertDialog`),
    /// preserving an outer composite tooltip or modal hosting the
    /// popover. Use for menu / dropdown item activation that wants to
    /// close the menu cascade without disturbing the host surface.
    pub fn dismiss_self_overlay_chain(&mut self) {
        self.dismiss_scope = Some(DismissScope::SelfChain);
    }

    /// Dismiss every overlay whose content is *not* a host surface
    /// (`Role::Tooltip`, `Role::Dialog`, `Role::AlertDialog`),
    /// preserving an outer composite tooltip or modal hosting the
    /// trigger. Use for popover triggers and pre-show cleanup that
    /// want to close stale popovers / menus without taking a hosting
    /// surface with them.
    pub fn dismiss_all_except_hosts(&mut self) {
        self.dismiss_scope = Some(DismissScope::AllExceptHosts);
    }

    /// Dismiss the topmost overlay only (e.g., closing a submenu while
    /// keeping the parent menu open).
    pub fn dismiss_top_overlay(&mut self) {
        self.dismiss_scope = Some(DismissScope::Top);
    }

    /// Dismiss descendant overlays of the source widget's containing overlay.
    /// Useful for closing sibling submenu branches while keeping the current
    /// parent menu open.
    pub fn dismiss_child_overlays(&mut self) {
        self.dismiss_descendant_overlays.push(None);
    }

    /// Dismiss descendant overlays of the source widget's containing overlay,
    /// preserving the subtree rooted at `content_id` if it is already open.
    pub fn dismiss_child_overlays_except(&mut self, content_id: crate::widget_id::WidgetId) {
        self.dismiss_descendant_overlays.push(Some(content_id));
    }

    /// Request an idle callback to be run during the next idle period.
    /// Use this for incremental work that takes 5-50ms — too short for a
    /// background thread, too long for a single frame.
    pub fn request_idle_callback(
        &mut self,
        callback: impl FnOnce(crate::idle::IdleDeadline) + 'static,
    ) {
        self.idle_callbacks.push(Box::new(callback));
    }

    /// Request framework-owned modal presentation.
    ///
    /// The widget tree records the request together with the originating
    /// widget, and the application layer can later resolve `Auto` into a
    /// concrete presentation backend.
    pub fn present_modal(&mut self, request: crate::modal::ModalRequest) {
        self.modal_requests.push(request);
    }

    /// Synchronously open a modal as a native window — the single
    /// unified path for native-window modals. Callers that don't
    /// care whether the modal lands in-tree or in a native window
    /// use [`present_modal`](Self::present_modal), which routes
    /// `ModalPresentation::Auto` through the framework's picker.
    ///
    /// Returns the new window's id, or `None` when called outside a
    /// dispatch context (standalone trees). The window's parent is
    /// the current window; focus target and title / size from the
    /// request are honored.
    ///
    /// Only `ModalContent::Deferred` is supported here — an
    /// `ExistingWidget` id wouldn't make sense in a fresh tree.
    pub fn open_modal(
        &mut self,
        request: crate::modal::ModalRequest,
    ) -> Option<crate::window::TeksiloWindowId> {
        let parent = self.current_window.as_ref()?.id();
        let crate::modal::ModalContent::Deferred(builder) = request.content else {
            return None;
        };
        let mut config = crate::window::WindowConfig::new().modal(crate::window::ModalConfig {
            parent,
            focus_target: request.focus_target,
        });
        if let Some(title) = request.title {
            config = config.title(title);
        }
        if let Some((w, h)) = request.size {
            config = config.size(w, h);
        }
        let config = config.root(move |tree, _state| builder(tree));
        Some(self.open_window(config))
    }

    /// Dismiss the current framework-owned modal presentation.
    pub fn dismiss_modal(&mut self) {
        self.dismiss_modal = true;
    }

    /// Show an overlay after a delay. The widget tree checks pending delayed
    /// overlays during `layout()` and shows them once the delay elapses.
    /// Use this for submenu hover-open delays.
    ///
    /// The content widget should already be added to the tree (typically
    /// dormant). It will be activated automatically when the delay elapses.
    pub fn show_overlay_after(
        &mut self,
        request: crate::overlay::OverlayRequest,
        delay: std::time::Duration,
    ) {
        self.delayed_overlay_requests
            .push((request, delay, None, false));
    }

    /// Show an overlay after a delay and move focus when it opens.
    pub fn show_overlay_after_with_focus(
        &mut self,
        request: crate::overlay::OverlayRequest,
        delay: std::time::Duration,
        focus_target: crate::widget_id::WidgetId,
    ) {
        self.delayed_overlay_requests
            .push((request, delay, Some(focus_target), false));
    }

    /// Show an overlay after a delay, move focus when it opens, and
    /// dismiss the anchor's sibling overlays **at that moment** rather
    /// than when the request was made.
    ///
    /// This is the hover-switch between two submenu triggers in the same
    /// menu. Dismissing eagerly at hover-enter closes the submenu the
    /// user is still walking toward as soon as the pointer crosses a
    /// neighbouring trigger; deferring the dismissal to the moment the
    /// new submenu actually opens means a pointer merely passing through
    /// costs nothing, and one that settles gets the swap on the same
    /// frame — no window with two submenus on screen.
    pub fn show_overlay_after_replacing_siblings(
        &mut self,
        request: crate::overlay::OverlayRequest,
        delay: std::time::Duration,
        focus_target: crate::widget_id::WidgetId,
    ) {
        self.delayed_overlay_requests
            .push((request, delay, Some(focus_target), true));
    }

    /// Request a repaint on a specific widget. Use this when an event handler
    /// on one widget changes state that affects a different widget's appearance
    /// (e.g., keyboard navigation highlighting items in an overlay).
    pub fn request_repaint(&mut self, id: crate::widget_id::WidgetId) {
        self.repaint_requests.push(id);
    }

    /// Programmatically click a widget (synthetic PointerDown + PointerUp at
    /// its center). Use this for keyboard activation of a child widget, e.g.,
    /// Enter on a keyboard-focused menu item.
    pub fn synthetic_click(&mut self, id: crate::widget_id::WidgetId) {
        self.synthetic_clicks.push(id);
    }

    /// Transfer focus to a specific widget. Use this when opening overlay
    /// content (menus, dialogs) that should receive keyboard events.
    pub fn request_focus(&mut self, id: crate::widget_id::WidgetId) {
        self.focus_requests.push(id);
    }

    /// Move focus **into** the content of `id`: focus its first focusable
    /// descendant in tab order. Unlike [`request_focus`](Self::request_focus),
    /// this does **not** fall back to focusing `id` itself when the subtree has
    /// no focusable descendant — it is a no-op in that case, so an empty region
    /// never traps focus on a non-interactive container.
    ///
    /// Use this for "dive into this region" gestures, e.g. pressing Enter on a
    /// focused tab header to move focus into the tab's content panel. A panel
    /// with focusable content lands on its first control; a panel that opted
    /// into focusability itself (no inner controls) lands on the panel; a bare
    /// panel with neither leaves focus where it was.
    pub fn request_focus_into(&mut self, id: crate::widget_id::WidgetId) {
        self.focus_into_requests.push(id);
    }

    /// Scroll the given rectangle into view inside every enclosing scroll
    /// container, walking outward from the widget whose handler is running.
    ///
    /// `rect` is in **absolute tree (window) coordinates** — the same space
    /// the arena stores widget bounds in. After the handler returns, the
    /// framework walks the current widget's ancestors and, for each
    /// `clips_children` scroll container whose viewport does not already
    /// fully contain `rect`, dispatches
    /// [`WidgetEvent::ScrollIntoView`](crate::event::WidgetEvent::ScrollIntoView)
    /// so the container adjusts its offset. Nested scroll areas each get a
    /// turn (outermost included), exactly like the focus-driven path.
    ///
    /// Unlike the automatic focus follow — which can only reveal a *focused
    /// widget's own bounds* — this lets a widget reveal an arbitrary interior
    /// rectangle it computed itself: a text caret, a virtualized list/table
    /// row (which is not a distinct focusable node), or a scrolled-off tab
    /// header. The widget remains responsible for scrolling its *own* interior
    /// viewport; `ensure_visible` handles the enclosing containers. It is a
    /// no-op when there is no scroll container above the widget, or when every
    /// container already shows the rect.
    ///
    /// See [`ensure_visible_with_margin`](Self::ensure_visible_with_margin) to
    /// keep breathing room around the target.
    pub fn ensure_visible(&mut self, rect: teksilo_canvas::Rect) {
        self.scroll_into_view_requests.push(ScrollRevealRequest {
            rect,
            margin: 0.0,
            align: crate::event::ScrollAlign::Minimal,
            motion: crate::event::ScrollMotion::Instant,
            from: None,
        });
    }

    /// [`ensure_visible`](Self::ensure_visible), for a rect that belongs to
    /// **another** widget.
    ///
    /// The framework walks `owner`'s ancestors rather than the handling widget's.
    /// That distinction is the whole of it, and getting it wrong fails silently:
    /// a find banner's Next button sits *beside* the scrolling page, not inside
    /// it, so a reveal walked from the button climbs out through the banner and
    /// never meets the scroll container the match is in. The match is selected,
    /// the counter moves, and the viewport does not follow.
    ///
    /// The same reasoning [`ensure_widget_visible`](Self::ensure_widget_visible)
    /// already records for the id-based form; this is its rect-based twin, for a
    /// target that is an interior span rather than a mounted child.
    ///
    /// `rect` is in absolute tree (window) coordinates.
    pub fn ensure_visible_from(
        &mut self,
        owner: crate::widget_id::WidgetId,
        rect: teksilo_canvas::Rect,
    ) {
        self.scroll_into_view_requests.push(ScrollRevealRequest {
            rect,
            margin: 0.0,
            align: crate::event::ScrollAlign::Minimal,
            motion: crate::event::ScrollMotion::Instant,
            from: Some(owner),
        });
    }

    /// Like [`ensure_visible`](Self::ensure_visible), but keeps `margin`
    /// logical pixels of breathing room around `rect` on every edge, so the
    /// target does not sit flush against the viewport boundary (the caret at
    /// the bottom line, the selected row at the fold). `rect` is in absolute
    /// tree (window) coordinates.
    pub fn ensure_visible_with_margin(&mut self, rect: teksilo_canvas::Rect, margin: f32) {
        self.scroll_into_view_requests.push(ScrollRevealRequest {
            rect,
            margin: margin.max(0.0),
            align: crate::event::ScrollAlign::Minimal,
            motion: crate::event::ScrollMotion::Instant,
            from: None,
        });
    }

    /// **Pin** `rect` at `fraction` of the way down the innermost enclosing
    /// scroll container — `0.0` flush with the top, `0.5` centred, `1.0` flush
    /// with the bottom — instead of merely revealing it.
    ///
    /// The difference from [`ensure_visible`](Self::ensure_visible) is that this
    /// scrolls **even when the target is already visible**. That is what makes
    /// it usable for typewriter scrolling: a caret that only moved the view once
    /// it fell off the edge would not be pinned to anything.
    ///
    /// Only the **innermost** clipping ancestor aligns; any further ancestors
    /// out fall back to a minimal reveal, since an outer container's job is to
    /// bring the inner viewport on screen, not to align a rectangle it does not
    /// own.
    ///
    /// `fraction` is clamped to `0.0..=1.0`. The container additionally clamps
    /// to its own scroll range, so a target near the start or end of the content
    /// lands as close to `fraction` as the range permits — see the scroll
    /// container's `scroll_past_end` for buying range past the content's end so
    /// the last line can still reach the pin.
    ///
    /// `rect` is in absolute tree (window) coordinates.
    pub fn ensure_visible_aligned(
        &mut self,
        rect: teksilo_canvas::Rect,
        fraction: f32,
        motion: crate::event::ScrollMotion,
    ) {
        self.scroll_into_view_requests.push(ScrollRevealRequest {
            rect,
            margin: 0.0,
            align: crate::event::ScrollAlign::Fraction(fraction.clamp(0.0, 1.0)),
            motion,
            from: None,
        });
    }

    /// [`ensure_visible_aligned`](Self::ensure_visible_aligned), for a rect that
    /// belongs to **another** widget — see [`ensure_visible_from`](Self::ensure_visible_from)
    /// for why the distinction exists and how it fails when it is missed.
    pub fn ensure_visible_aligned_from(
        &mut self,
        owner: crate::widget_id::WidgetId,
        rect: teksilo_canvas::Rect,
        fraction: f32,
        motion: crate::event::ScrollMotion,
    ) {
        self.scroll_into_view_requests.push(ScrollRevealRequest {
            rect,
            margin: 0.0,
            align: crate::event::ScrollAlign::Fraction(fraction.clamp(0.0, 1.0)),
            motion,
            from: Some(owner),
        });
    }

    /// Scroll a specific mounted widget into view inside every enclosing
    /// scroll container — the id-based companion to
    /// [`ensure_visible`](Self::ensure_visible).
    ///
    /// The framework resolves `id` to its current absolute bounds after the
    /// handler returns and walks *that widget's* ancestors (never `id`
    /// itself), dispatching
    /// [`WidgetEvent::ScrollIntoView`](crate::event::WidgetEvent::ScrollIntoView)
    /// to each `clips_children` container that doesn't already show it.
    ///
    /// Use this when the target you want revealed is a real, non-virtualized
    /// child whose bounds the arena already knows — a selected radio tile, a
    /// tab header — so you don't have to compute a rect. For a target that has
    /// no distinct node (a text caret) or that may not be realized (a
    /// virtualized list/table row), use [`ensure_visible`](Self::ensure_visible)
    /// with an analytic rect instead. No-op if `id` is not currently mounted.
    pub fn ensure_widget_visible(&mut self, id: crate::widget_id::WidgetId) {
        self.scroll_widget_into_view_requests.push((id, 0.0));
    }

    /// Like [`ensure_widget_visible`](Self::ensure_widget_visible), but keeps
    /// `margin` logical pixels of breathing room around the widget.
    pub fn ensure_widget_visible_with_margin(
        &mut self,
        id: crate::widget_id::WidgetId,
        margin: f32,
    ) {
        self.scroll_widget_into_view_requests
            .push((id, margin.max(0.0)));
    }

    /// Surface the tooltip of a keyboard-highlighted item immediately (no
    /// dwell), dismissing the previously-highlighted item's tooltip. Used by
    /// `MenuList` on arrow-key navigation so a menu item's rich/composite
    /// tooltip is reachable by keyboard — real focus stays on the menu panel,
    /// so this is keyed on the item id rather than on focus. Pass the item's
    /// own widget id; a tooltip-less item simply dismisses the previous one.
    pub fn show_highlight_tooltip(&mut self, id: crate::widget_id::WidgetId) {
        self.highlight_tooltip_requests.push(id);
    }

    /// Cancel a pending delayed overlay by its content widget ID.
    /// Call this when the hover ends before the delay elapses.
    pub fn cancel_delayed_overlay(&mut self, content_id: crate::widget_id::WidgetId) {
        self.cancel_delayed_overlays.push(content_id);
    }

    /// Arm the "safe triangle" of the open overlay rooted at
    /// `content_id`, with its apex at the current pointer position.
    ///
    /// Call this from the anchor's hover-leave handler: the pointer is
    /// then exactly at the point the diagonal toward the overlay
    /// starts. While the pointer sits inside the triangle spanned by
    /// that apex and the overlay's near edge, the overlay's
    /// pointer-leave grace is held off; leaving the triangle starts the
    /// grace and re-entering it cancels the grace again, so a wobble
    /// mid-diagonal costs nothing. Throughout — cone or no cone, until
    /// the pointer arrives or the framework's budget runs out —
    /// [`overlay_safe_region_armed`](Self::overlay_safe_region_armed)
    /// reports `true` so sibling widgets stand aside and let that one
    /// re-evaluated grace own the dismissal.
    ///
    /// No-ops when the overlay is not open (a submenu whose hover-open
    /// delay was cancelled before it ever showed) or when no pointer
    /// position is known.
    pub fn arm_overlay_safe_region(&mut self, content_id: crate::widget_id::WidgetId) {
        self.safe_region_arm_requests.push(content_id);
    }

    /// Choose, **from this press's `PointerDown` handler**, when this node's own
    /// drag may begin — overriding its declared
    /// [`DragActivation`](teksilo_tokens::DragActivation) for this press alone.
    ///
    /// `.drag_activation(..)` is a node property, decided at build time. That is
    /// the right grain when a node's `on_drag` means one thing. It is the wrong
    /// grain when one handler means several: a scene viewport's single `on_drag`
    /// is its marquee *and* its item grab *and* its magnet port drag, and which
    /// of the three a press is cannot be known until the press has been
    /// hit-tested. This is the per-press door — the press handler has already
    /// done that hit test, so it can say "this one landed on an item, arm it
    /// immediately" while leaving an empty-space press to defer to the pan.
    ///
    /// Stashed on the pointer's sequence, **not** written back onto the node, so
    /// it dies with the press that chose it. That matters here more than
    /// hygiene usually does: `on_pointer_event` previews root-first over every
    /// strict ancestor of the press target, so a node that answers from it also
    /// answers for presses an interactive descendant owns, and a node write
    /// would leave the declaration changed for the *next* press.
    ///
    /// Read by the enrolment walk, which runs immediately after the press
    /// dispatch. Called from anything but a press handler it is inert for the
    /// press in flight — there is no enrolment left to read it — and applies to
    /// nothing else.
    ///
    /// Last writer wins: answering twice on one press means the second answer.
    pub fn set_drag_activation(&mut self, activation: teksilo_tokens::DragActivation) {
        self.drag_activation_override = Some(activation);
    }

    /// Capture **the pointer this handler is serving**: its subsequent
    /// `PointerMove` and `PointerUp` are routed to this widget regardless of
    /// hit test, until the capture is released.
    ///
    /// Capture is per pointer. Two fingers pressing two widgets hold two
    /// independent captures, and each is released only by its own Up or
    /// Cancel — so a second contact lifting can no longer steal the first
    /// one's stream. A mouse call site is unaffected: there is one mouse, and
    /// this captures it.
    /// **Also an arbitration act.** Taking the pointer from an undecided
    /// [`PointerSequence`](crate::gesture::PointerSequence) enrols this widget
    /// as a [`MemberRole::RawDrag`](crate::gesture::MemberRole::RawDrag)
    /// competitor, and for a precise pointer with no eligible pan competitor
    /// it decides the sequence outright — which is what makes the splitter
    /// handle, the dock resize handle and the table column grip (all of which
    /// answer `Ignored` from `on_pointer_event` and work from `PointerMove`
    /// with no recognizer at all) first-class competitors rather than widgets
    /// the arbitration cannot see.
    pub fn capture_pointer(&mut self) {
        self.pointer_capture = Some((None, true));
        self.explicit_capture = true;
    }

    /// Capture a *named* pointer, for a handler driving a pointer other than
    /// the one whose sample it is serving.
    pub fn capture_pointer_id(&mut self, pointer: crate::pointer::PointerId) {
        self.pointer_capture = Some((Some(pointer), true));
        self.explicit_capture = true;
    }

    /// Capture the pointer as **framework plumbing**, without staking an
    /// arbitration claim.
    ///
    /// The gesture arena takes the pointer for the Down..Up window so a
    /// recognizer keeps seeing moves that leave the widget's bounds, and the
    /// drag pipeline takes it for the life of a drag. Neither is a widget
    /// saying "this press is mine"; routing them through the public
    /// [`capture_pointer`](Self::capture_pointer) would enrol every
    /// arena-bearing node as a `RawDrag` member and decide every mouse
    /// sequence at press.
    pub(crate) fn capture_pointer_implicit(&mut self) {
        self.pointer_capture = Some((None, true));
    }

    /// Claim the pointer sequence for the widget whose handler is running:
    /// arbitration ends, every other competitor is cancelled.
    ///
    /// The explicit form of what a recognizer does when it recognizes. Use it
    /// from an application recognizer that decides by its own rules.
    pub fn claim_gesture(&mut self) {
        self.gesture_acts.push(GestureAct::Claim);
    }

    /// Withdraw the widget whose handler is running from the sequence. It can
    /// no longer win this press; its peers carry on.
    pub fn reject_gesture(&mut self) {
        self.gesture_acts.push(GestureAct::Reject);
    }

    /// Defer this widget's own decision without withdrawing: no peer may win
    /// while a member is holding.
    ///
    /// **The framework never holds.** This exists for an application
    /// recognizer awaiting an answer it does not have yet (a hit test against
    /// an off-thread model, a network round trip). The hold auto-releases at
    /// [`GestureProfile::max_hold`](teksilo_tokens::GestureProfile::max_hold)
    /// — 250 ms — so a holder that never answers cannot strand the press.
    pub fn hold_gesture(&mut self) {
        self.gesture_acts.push(GestureAct::Hold);
    }

    /// End this widget's hold, putting it back in the running.
    pub fn release_gesture(&mut self) {
        self.gesture_acts.push(GestureAct::Release);
    }

    /// Revoke the whole interaction of the pointer this handler is serving,
    /// for `reason`.
    ///
    /// The widget's own way into the cancel funnel, for a widget that knows
    /// the interaction can no longer mean anything — the document under a text
    /// drag was reloaded, the row being reordered was deleted by a peer. Every
    /// competitor is cancelled, the capture is given back, and a
    /// [`PointerCancel`](crate::event::WidgetEvent::PointerCancel) is
    /// delivered, all **after** this handler returns: a cancel taken inline
    /// would unwind the very sample the handler is standing on.
    ///
    /// Distinct from [`reject_gesture`](Self::reject_gesture), which withdraws
    /// only *this* widget and lets its peers carry on with a pointer that is
    /// still perfectly alive.
    pub fn cancel_pointer_sequence(&mut self, reason: crate::pointer::CancelReason) {
        self.cancel_pointer_request = Some(reason);
    }

    /// Release the capture of the pointer this handler is serving. Its events
    /// resume normal hit-test dispatch.
    pub fn release_pointer(&mut self) {
        self.pointer_capture = Some((None, false));
    }

    /// Whether the widget whose handler is running already holds the capture
    /// of the pointer it is serving.
    ///
    /// `true` also immediately after a [`capture_pointer`](Self::capture_pointer)
    /// in the same handler, even though the tree does not apply the request
    /// until the handler returns — asking "do I own this pointer?" after
    /// claiming it must not answer no.
    pub fn owns_pointer(&self) -> bool {
        match self.pointer_capture {
            Some((None, capture)) => capture,
            _ => self.dispatch_node.is_some() && self.dispatch_node == self.pointer_captor,
        }
    }

    /// Start a drag-and-drop operation from the given source widget.
    ///
    /// The `payload` carries the data being dragged. During the drag:
    /// - `PointerMove` events update the drag position and fire `on_drag_hover`
    ///   on widgets under the pointer that have drop handlers
    /// - `PointerUp` fires `on_drop` on the target widget (if any)
    /// - `Escape` cancels the drag
    pub fn start_drag(
        &mut self,
        source_widget: crate::widget_id::WidgetId,
        payload: crate::drag_payload::DragPayload,
    ) {
        self.drag_start_request = Some((source_widget, payload, None));
    }

    /// Start a drag-and-drop with a preview widget that follows the pointer.
    pub fn start_drag_with_preview(
        &mut self,
        source_widget: crate::widget_id::WidgetId,
        payload: crate::drag_payload::DragPayload,
        preview: Box<dyn crate::widget::Widget>,
    ) {
        self.drag_start_request = Some((source_widget, payload, Some(preview)));
    }

    /// Cancel the active drag-and-drop session (if any).
    pub fn cancel_drag(&mut self) {
        self.cancel_drag = true;
    }

    /// Replace the tree-level theme. Composite widgets are rebuilt so any
    /// derived values they captured at build time pick up the new tokens,
    /// and all widgets are marked dirty for repaint.
    ///
    /// An explicit theme also turns **off** OS-following: the app's theme
    /// mode is reset to manual, so a later OS light/dark change won't
    /// override the chosen theme.
    pub fn set_theme(&mut self, theme: crate::styles::Theme) {
        self.theme_request = Some(theme);
    }

    /// Switch the application to follow the OS theme (native / system mode):
    /// the app adopts the OS's colours and tracks OS light/dark changes at
    /// runtime. On platforms without OS-colour support it falls back to
    /// following the built-in light/dark presets.
    ///
    /// This is the counterpart to [`set_theme`](Self::set_theme): calling
    /// `set_theme` pins a fixed theme (manual mode), while this resumes
    /// OS-following. Parameterless by design, so widgets need not reference
    /// the app-layer theme-mode enum.
    pub fn follow_system_theme(&mut self) {
        self.follow_system_request = true;
    }

    /// Replace the tree-level locale identifier. Composite widgets are
    /// rebuilt so any tr! lookups picked up at build time are re-evaluated
    /// against the new locale.
    pub fn set_locale(&mut self, locale: impl Into<String>) {
        self.locale_request = Some(locale.into());
    }

    /// Set the user-controlled global text-scale factor (`1.0` = 100 %).
    ///
    /// The change is applied app-wide (every window) after the handler returns,
    /// mirroring [`set_theme`](Self::set_theme) / [`set_locale`](Self::set_locale).
    /// All text grows uniformly without a rebuild. Persist the value through
    /// `ctx.settings()` (e.g. `teksilo_settings::TEXT_SCALE_KEY`) so it survives
    /// a restart — the `TextScaleControl` widget does both for you.
    pub fn set_text_scale(&mut self, factor: f32) {
        self.text_scale_request = Some(factor);
    }
}

#[cfg(test)]
mod multi_window_tests {
    use super::*;
    use crate::window::state::WindowStateInit;
    use crate::window::{
        NoopWindowOps, TeksiloWindowId, WindowConfig, WindowOps, WindowPlacement, WindowState,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Recording implementation of `WindowOps` so tests can assert
    /// that `EventContext` routes each method through the trait.
    #[derive(Default)]
    struct RecordingOps {
        open_calls: RefCell<Vec<WindowConfig>>,
        focus_calls: RefCell<Vec<TeksiloWindowId>>,
        close_calls: RefCell<Vec<TeksiloWindowId>>,
        next_id: RefCell<u64>,
        // A fake registry so `find_window` / `window_state` / `windows`
        // can return values.
        states: RefCell<Vec<WindowState>>,
    }

    impl RecordingOps {
        fn alloc_id(&self) -> TeksiloWindowId {
            let mut n = self.next_id.borrow_mut();
            *n += 1;
            TeksiloWindowId::new(*n)
        }
    }

    impl WindowOps for RecordingOps {
        fn open_window(&mut self, config: WindowConfig) -> TeksiloWindowId {
            let id = self.alloc_id();
            let state = WindowState::new(WindowStateInit {
                id,
                string_id: config.string_id.clone(),
                placement: config.initial_placement,
                title: config.title.clone(),
                size: config.size,
                position: config.position.unwrap_or((0, 0)),
                focused: true,
                resizable: config.resizable,
                always_on_top: config.always_on_top,
            });
            self.states.borrow_mut().push(state);
            self.open_calls.borrow_mut().push(config);
            id
        }

        fn find_window(&self, string_id: &str) -> Option<TeksiloWindowId> {
            self.states
                .borrow()
                .iter()
                .find(|s| s.string_id() == Some(string_id))
                .map(|s| s.id())
        }

        fn window_state(&self, id: TeksiloWindowId) -> Option<WindowState> {
            self.states.borrow().iter().find(|s| s.id() == id).cloned()
        }

        fn windows(&self) -> Vec<WindowState> {
            self.states.borrow().clone()
        }

        fn focus_window(&mut self, id: TeksiloWindowId) {
            self.focus_calls.borrow_mut().push(id);
        }

        fn close_window_by_id(&mut self, id: TeksiloWindowId) {
            self.close_calls.borrow_mut().push(id);
        }
    }

    fn make_state(id: u64, string_id: Option<&str>) -> WindowState {
        WindowState::new(WindowStateInit {
            id: TeksiloWindowId::new(id),
            string_id: string_id.map(String::from),
            placement: WindowPlacement::Floating,
            title: "Test".into(),
            size: (800, 600),
            position: (0, 0),
            focused: true,
            resizable: true,
            always_on_top: false,
        })
    }

    #[test]
    fn window_returns_current_window_state() {
        let state = make_state(1, Some("main"));
        let mut noop = NoopWindowOps;
        let ctx = EventContext::new().with_window_context(&mut noop, Some(state.clone()));
        assert_eq!(ctx.window().unwrap().id(), TeksiloWindowId::new(1));
        assert_eq!(ctx.window().unwrap().string_id(), Some("main"));
    }

    #[test]
    fn window_is_none_without_context() {
        let ctx = EventContext::new();
        assert!(ctx.window().is_none());
    }

    #[test]
    fn open_window_routes_through_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, Some("main"));
        let returned_id = {
            let mut ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
            ctx.open_window(WindowConfig::new().id("help").title("Help"))
        };
        assert_eq!(ops.open_calls.borrow().len(), 1);
        assert_eq!(
            ops.open_calls.borrow()[0].string_id.as_deref(),
            Some("help")
        );
        // Recording ops allocates ids 2+; 1 was reserved for `main`
        // only in this test — Recording's counter starts from 0, so the
        // first alloc yields 1.
        assert_eq!(returned_id, TeksiloWindowId::new(1));
    }

    #[test]
    fn find_window_routes_through_ops() {
        let mut ops = RecordingOps::default();
        ops.states.borrow_mut().push(make_state(7, Some("foo")));
        let main_state = make_state(1, Some("main"));
        let ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
        assert_eq!(ctx.find_window("foo"), Some(TeksiloWindowId::new(7)));
        assert!(ctx.find_window("missing").is_none());
    }

    #[test]
    fn focus_window_records_via_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, None);
        {
            let mut ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
            ctx.focus_window(TeksiloWindowId::new(42));
        }
        assert_eq!(
            ops.focus_calls.borrow().as_slice(),
            &[TeksiloWindowId::new(42)]
        );
    }

    #[test]
    fn close_window_by_id_records_via_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, None);
        {
            let mut ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
            ctx.close_window_by_id(TeksiloWindowId::new(9));
        }
        assert_eq!(
            ops.close_calls.borrow().as_slice(),
            &[TeksiloWindowId::new(9)]
        );
    }

    #[test]
    fn close_window_sets_guarded_flag_only() {
        let mut ctx = EventContext::new();
        ctx.close_window();
        assert!(
            ctx.close_window_requested,
            "close_window must raise the guarded-close flag"
        );
        assert!(
            !ctx.force_close_requested,
            "close_window must NOT raise the forced-close flag"
        );
    }

    #[test]
    fn close_window_forced_sets_force_flag_only() {
        let mut ctx = EventContext::new();
        ctx.close_window_forced();
        assert!(
            ctx.force_close_requested,
            "close_window_forced must raise the forced-close flag"
        );
        assert!(
            !ctx.close_window_requested,
            "close_window_forced must NOT raise the guarded-close flag"
        );
    }

    /// End-to-end: a handler calling `close_window_forced` during
    /// dispatch must transfer the flag onto the `WidgetTree` (the
    /// `collect_from_ctx` teardown), where the app loop drains it via
    /// `take_force_close_request` — separately from the guarded
    /// `take_close_window_request` flag.
    #[test]
    fn forced_close_flag_propagates_to_tree_and_drains_independently() {
        use crate::test_widgets::FillWidget;
        use crate::widget_tree::WidgetTree;

        // `run_with_event_context` only runs the `collect_from_ctx`
        // teardown (which transfers ctx flags onto the tree) when the
        // tree has a root to anchor on, so give it one.
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| ctx.close_window_forced());
        assert!(
            tree.take_force_close_request(),
            "forced-close flag must reach the tree"
        );
        assert!(
            !tree.take_close_window_request(),
            "a forced close must not also raise the guarded flag"
        );

        // And the guarded path stays on its own channel.
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new());
        tree.run_with_event_context(&mut NoopWindowOps, |ctx| ctx.close_window());
        assert!(tree.take_close_window_request());
        assert!(!tree.take_force_close_request());
    }

    #[test]
    fn windows_enumerates_via_ops() {
        let mut ops = RecordingOps::default();
        ops.states.borrow_mut().push(make_state(1, Some("a")));
        ops.states.borrow_mut().push(make_state(2, Some("b")));
        let main_state = make_state(1, Some("a"));
        let ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
        let ids: Vec<_> = ctx.windows().iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![TeksiloWindowId::new(1), TeksiloWindowId::new(2)]);
    }

    #[test]
    fn standalone_context_returns_empty_windows_and_none_lookups() {
        let ctx = EventContext::new();
        assert!(ctx.find_window("anything").is_none());
        assert!(ctx.window_state(TeksiloWindowId::new(1)).is_none());
        assert!(ctx.windows().is_empty());
    }

    #[test]
    #[should_panic(expected = "open_window called outside of a dispatch")]
    fn open_window_on_standalone_context_panics() {
        let mut ctx = EventContext::new();
        let _ = ctx.open_window(WindowConfig::new());
    }

    #[test]
    fn open_modal_builds_window_config_from_request() {
        use crate::modal::{ModalContent, ModalRequest};
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, Some("main"));
        let built_widget = Rc::new(RefCell::new(false));
        let built_widget_flag = built_widget.clone();
        let request = ModalRequest {
            content: ModalContent::Deferred(Box::new(move |_tree| {
                *built_widget_flag.borrow_mut() = true;
                // Return a dummy WidgetId — not used in this test since
                // the RecordingOps doesn't actually build the tree.
                crate::widget_id::WidgetId::default()
            })),
            presentation: crate::modal::ModalPresentation::NativeWindow,
            close_behavior: crate::modal::ModalCloseBehavior::default(),
            title: Some("Confirm".to_string()),
            size: Some((420, 180)),
            focus_target: None,
            on_dismiss: None,
        };
        {
            let mut ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
            let id = ctx.open_modal(request);
            assert!(id.is_some());
        }
        // open_modal is a thin wrapper over open_window — the config
        // it built must reach RecordingOps::open_window.
        let calls = ops.open_calls.borrow();
        assert_eq!(calls.len(), 1);
        let cfg = &calls[0];
        assert_eq!(cfg.title, "Confirm");
        assert_eq!(cfg.size, (420, 180));
        assert!(cfg.is_modal());
        assert_eq!(cfg.modal_parent(), Some(TeksiloWindowId::new(1)));
        // Cell is just to let us observe something reachable via cfg.root_builder;
        // the builder hasn't been called yet (RecordingOps records the config
        // but doesn't build the tree).
        let _ = built_widget;
    }

    #[test]
    fn open_modal_requires_current_window() {
        use crate::modal::{ModalContent, ModalRequest};
        let mut ops = RecordingOps::default();
        let mut ctx = EventContext::new().with_window_context(&mut ops, None);
        let request = ModalRequest {
            content: ModalContent::Deferred(Box::new(|_tree| {
                crate::widget_id::WidgetId::default()
            })),
            presentation: crate::modal::ModalPresentation::NativeWindow,
            close_behavior: crate::modal::ModalCloseBehavior::default(),
            title: None,
            size: None,
            focus_target: None,
            on_dismiss: None,
        };
        assert!(ctx.open_modal(request).is_none());
    }
}
