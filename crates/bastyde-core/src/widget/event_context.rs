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
    /// Overlay ids whose `auto_dismiss_after` timer should be paused
    /// or resumed after the handler returns (`true` = pause, `false`
    /// = resume). Drained by `WidgetTree::process_overlay_requests`
    /// against `OverlayManager::pause_auto_dismiss` /
    /// `resume_auto_dismiss`. Used by `ToastHost` for hover-pause.
    pub(crate) overlay_pause_requests: Vec<(crate::overlay::OverlayId, bool)>,
    /// The dismissal scope chosen by the handler, if any. Set by
    /// `dismiss_all_overlays()` / `dismiss_all_except_hosts()` /
    /// `dismiss_self_overlay_chain()` / `dismiss_top_overlay()` —
    /// last setter wins. `None` falls through to draining the
    /// per-id `overlay_dismissals` vec instead.
    pub(crate) dismiss_scope: Option<DismissScope>,
    /// Request to capture or release the pointer.
    pub(crate) pointer_capture: Option<bool>,
    /// Delayed overlay requests (request, delay, optional focus target).
    pub(crate) delayed_overlay_requests: Vec<(
        crate::overlay::OverlayRequest,
        std::time::Duration,
        Option<crate::widget_id::WidgetId>,
    )>,
    /// Timed overlay requests (request, auto-dismiss delay).
    pub(crate) timed_overlay_requests: Vec<(crate::overlay::OverlayRequest, std::time::Duration)>,
    /// Dismiss descendant overlays of the source widget's containing overlay.
    /// Optionally preserve the subtree rooted at a specific content widget ID.
    pub(crate) dismiss_descendant_overlays: Vec<Option<crate::widget_id::WidgetId>>,
    /// Cancel pending delayed overlays by content widget ID.
    pub(crate) cancel_delayed_overlays: Vec<crate::widget_id::WidgetId>,
    /// Widget IDs that need repainting (cross-widget signal propagation).
    pub(crate) repaint_requests: Vec<crate::widget_id::WidgetId>,
    /// Synthetic clicks to dispatch on target widgets after event processing.
    pub(crate) synthetic_clicks: Vec<crate::widget_id::WidgetId>,
    /// Focus requests — transfer focus to a specific widget (e.g., overlay content on open).
    pub(crate) focus_requests: Vec<crate::widget_id::WidgetId>,
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
    /// Replace the tree-level locale identifier. Drained after dispatch;
    /// triggers a composite-widget rebuild and full repaint.
    pub(crate) locale_request: Option<String>,
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
    /// `WidgetTree::take_close_window_request`.
    pub(crate) close_window_requested: bool,
    /// Set by [`request_accessibility_update`](EventContext::request_accessibility_update);
    /// drained in `collect_from_ctx` to set `WidgetTree::a11y_dirty`, forcing the
    /// next `sync_accessibility` to re-walk the AccessKit tree. The general lever for a
    /// composing widget that restructured its subtree in a way that changes the AT tree
    /// (relayout alone no longer re-walks AT).
    pub(crate) request_a11y_update: bool,
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
}

// Manual `Debug`: the `WithWidgetMut` closure is not `Debug`.
impl std::fmt::Debug for TreeMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetDormant(id) => f.debug_tuple("SetDormant").field(id).finish(),
            Self::Activate(id) => f.debug_tuple("Activate").field(id).finish(),
            Self::Destroy(id) => f.debug_tuple("Destroy").field(id).finish(),
            Self::WithWidgetMut { id, dirty, .. } => f
                .debug_struct("WithWidgetMut")
                .field("id", id)
                .field("dirty", dirty)
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
            overlay_pause_requests: Vec::new(),
            dismiss_scope: None,
            pointer_capture: None,
            delayed_overlay_requests: Vec::new(),
            timed_overlay_requests: Vec::new(),
            dismiss_descendant_overlays: Vec::new(),
            cancel_delayed_overlays: Vec::new(),
            repaint_requests: Vec::new(),
            synthetic_clicks: Vec::new(),
            focus_requests: Vec::new(),
            drag_start_request: None,
            cancel_drag: false,
            drag_is_external: false,
            theme_request: None,
            locale_request: None,
            frame_requested: false,
            app_context: None,
            pending_intents: Vec::new(),
            current_source: None,
            pending_key_capture: None,
            cancel_key_capture: false,
            pending_shortcut_mutations: Vec::new(),
            close_window_requested: false,
            request_a11y_update: false,
            window_ops: None,
            current_window: None,
        }
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
    /// (`bastyde_platform::file_dialog`'s `RfdAsyncBackend`, future
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

    /// Run a closure with the given [`IntentSource`] active. Any
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
    /// [`WidgetTree::begin_key_capture`] before the handler ran.
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
    pub fn close_window(&mut self) {
        self.close_window_requested = true;
    }

    // -------------------- Multi-window API --------------------

    /// The [`WindowState`](crate::window::WindowState) for the window
    /// hosting this handler. `None` only for handlers run outside
    /// of an app (hand-constructed `EventContext` in tests).
    pub fn window(&self) -> Option<&crate::window::WindowState> {
        self.current_window.as_ref()
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
    ) -> crate::window::BastydeWindowId {
        self.window_ops
            .as_deref_mut()
            .expect("open_window called outside of a dispatch")
            .open_window(config)
    }

    /// Find a window by the string id assigned via
    /// [`WindowConfig::id`](crate::window::WindowConfig::id). Returns
    /// `None` if no open window carries that id.
    pub fn find_window(&self, string_id: &str) -> Option<crate::window::BastydeWindowId> {
        self.window_ops.as_deref()?.find_window(string_id)
    }

    /// Read the [`WindowState`](crate::window::WindowState) for a
    /// specific window.
    pub fn window_state(
        &self,
        id: crate::window::BastydeWindowId,
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
    pub fn focus_window(&mut self, id: crate::window::BastydeWindowId) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.focus_window(id);
        }
    }

    /// Close a specific window by id. Equivalent to
    /// [`close_window`](Self::close_window) when `id` is the current
    /// window's id.
    pub fn close_window_by_id(&mut self, id: crate::window::BastydeWindowId) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.close_window_by_id(id);
        }
    }

    /// Report the focused text widget's caret rectangle (window-logical
    /// pixels) so the platform can position the OS IME candidate window at
    /// the insertion point. Text-editing widgets call this whenever the
    /// caret moves. No-op outside a dispatch / on a standalone tree.
    pub fn set_ime_cursor_area(&mut self, area: bastyde_canvas::Rect) {
        if let Some(ops) = self.window_ops.as_deref_mut() {
            ops.set_ime_cursor_area(area);
        }
    }

    /// Resolve the platform parent handle of the window currently
    /// dispatching the event. Used by native-dialog integrations
    /// (`bastyde_platform::file_dialog`) to parent OS dialogs to the
    /// originating Bastyde window.
    ///
    /// Returns `None` when called from a standalone `WidgetTree` (no
    /// app-level [`WindowOps`] sink), or when the platform refuses to
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
    /// The target widget must override [`Widget::as_any_mut`] to return
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

    /// Request that the AccessKit tree be re-walked after this handler
    /// returns. Use after a mutation that changes the accessibility tree
    /// **shape** in a way the framework doesn't already detect (relayout
    /// alone no longer re-walks AT; only events that change the AT tree
    /// — focus, overlays, locale/shortcut rebinds — set the dirty flag).
    /// The companion [`BuildContext::request_accessibility_update`] covers
    /// the build-time path.
    pub fn request_accessibility_update(&mut self) {
        self.request_a11y_update = true;
    }

    /// Show an overlay (tooltip, menu, popover).
    pub fn show_overlay(&mut self, request: crate::overlay::OverlayRequest) {
        self.overlay_requests.push(request);
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
    ) -> Option<crate::window::BastydeWindowId> {
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
        self.delayed_overlay_requests.push((request, delay, None));
    }

    /// Show an overlay after a delay and move focus when it opens.
    pub fn show_overlay_after_with_focus(
        &mut self,
        request: crate::overlay::OverlayRequest,
        delay: std::time::Duration,
        focus_target: crate::widget_id::WidgetId,
    ) {
        self.delayed_overlay_requests
            .push((request, delay, Some(focus_target)));
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

    /// Cancel a pending delayed overlay by its content widget ID.
    /// Call this when the hover ends before the delay elapses.
    pub fn cancel_delayed_overlay(&mut self, content_id: crate::widget_id::WidgetId) {
        self.cancel_delayed_overlays.push(content_id);
    }

    /// Capture the pointer: all subsequent `PointerMove` and `PointerUp`
    /// events will be routed to the capturing widget until the capture is
    /// released. Use this when starting a drag operation.
    pub fn capture_pointer(&mut self) {
        self.pointer_capture = Some(true);
    }

    /// Release a previously captured pointer. Pointer events resume normal
    /// hit-test dispatch.
    pub fn release_pointer(&mut self) {
        self.pointer_capture = Some(false);
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
    pub fn set_theme(&mut self, theme: crate::styles::Theme) {
        self.theme_request = Some(theme);
    }

    /// Replace the tree-level locale identifier. Composite widgets are
    /// rebuilt so any tr! lookups picked up at build time are re-evaluated
    /// against the new locale.
    pub fn set_locale(&mut self, locale: impl Into<String>) {
        self.locale_request = Some(locale.into());
    }
}

#[cfg(test)]
mod multi_window_tests {
    use super::*;
    use crate::window::state::WindowStateInit;
    use crate::window::{
        BastydeWindowId, NoopWindowOps, WindowConfig, WindowOps, WindowPlacement, WindowState,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Recording implementation of `WindowOps` so tests can assert
    /// that `EventContext` routes each method through the trait.
    #[derive(Default)]
    struct RecordingOps {
        open_calls: RefCell<Vec<WindowConfig>>,
        focus_calls: RefCell<Vec<BastydeWindowId>>,
        close_calls: RefCell<Vec<BastydeWindowId>>,
        next_id: RefCell<u64>,
        // A fake registry so `find_window` / `window_state` / `windows`
        // can return values.
        states: RefCell<Vec<WindowState>>,
    }

    impl RecordingOps {
        fn alloc_id(&self) -> BastydeWindowId {
            let mut n = self.next_id.borrow_mut();
            *n += 1;
            BastydeWindowId::new(*n)
        }
    }

    impl WindowOps for RecordingOps {
        fn open_window(&mut self, config: WindowConfig) -> BastydeWindowId {
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

        fn find_window(&self, string_id: &str) -> Option<BastydeWindowId> {
            self.states
                .borrow()
                .iter()
                .find(|s| s.string_id() == Some(string_id))
                .map(|s| s.id())
        }

        fn window_state(&self, id: BastydeWindowId) -> Option<WindowState> {
            self.states.borrow().iter().find(|s| s.id() == id).cloned()
        }

        fn windows(&self) -> Vec<WindowState> {
            self.states.borrow().clone()
        }

        fn focus_window(&mut self, id: BastydeWindowId) {
            self.focus_calls.borrow_mut().push(id);
        }

        fn close_window_by_id(&mut self, id: BastydeWindowId) {
            self.close_calls.borrow_mut().push(id);
        }
    }

    fn make_state(id: u64, string_id: Option<&str>) -> WindowState {
        WindowState::new(WindowStateInit {
            id: BastydeWindowId::new(id),
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
        assert_eq!(ctx.window().unwrap().id(), BastydeWindowId::new(1));
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
        assert_eq!(returned_id, BastydeWindowId::new(1));
    }

    #[test]
    fn find_window_routes_through_ops() {
        let mut ops = RecordingOps::default();
        ops.states.borrow_mut().push(make_state(7, Some("foo")));
        let main_state = make_state(1, Some("main"));
        let ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
        assert_eq!(ctx.find_window("foo"), Some(BastydeWindowId::new(7)));
        assert!(ctx.find_window("missing").is_none());
    }

    #[test]
    fn focus_window_records_via_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, None);
        {
            let mut ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
            ctx.focus_window(BastydeWindowId::new(42));
        }
        assert_eq!(
            ops.focus_calls.borrow().as_slice(),
            &[BastydeWindowId::new(42)]
        );
    }

    #[test]
    fn close_window_by_id_records_via_ops() {
        let mut ops = RecordingOps::default();
        let main_state = make_state(1, None);
        {
            let mut ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
            ctx.close_window_by_id(BastydeWindowId::new(9));
        }
        assert_eq!(
            ops.close_calls.borrow().as_slice(),
            &[BastydeWindowId::new(9)]
        );
    }

    #[test]
    fn windows_enumerates_via_ops() {
        let mut ops = RecordingOps::default();
        ops.states.borrow_mut().push(make_state(1, Some("a")));
        ops.states.borrow_mut().push(make_state(2, Some("b")));
        let main_state = make_state(1, Some("a"));
        let ctx = EventContext::new().with_window_context(&mut ops, Some(main_state));
        let ids: Vec<_> = ctx.windows().iter().map(|s| s.id()).collect();
        assert_eq!(ids, vec![BastydeWindowId::new(1), BastydeWindowId::new(2)]);
    }

    #[test]
    fn standalone_context_returns_empty_windows_and_none_lookups() {
        let ctx = EventContext::new();
        assert!(ctx.find_window("anything").is_none());
        assert!(ctx.window_state(BastydeWindowId::new(1)).is_none());
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
        assert_eq!(cfg.modal_parent(), Some(BastydeWindowId::new(1)));
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
