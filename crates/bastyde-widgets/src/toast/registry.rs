// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ToastRegistry` — the app-singleton service handle.
//!
//! Registered into the app-state registry by `install_toast(opts)`
//! (lives in the `bastyde` umbrella). Holds the queue + per-entry
//! state shared between:
//!
//! - [`crate::toast::ext::EventContextToastExt`] — looks up the
//!   registry to fulfil `ctx.show_toast(toast)`.
//! - [`crate::toast::host::ToastHost`] — reads `live_entries` to
//!   render each rebuild, owns the per-frame timer, and registers
//!   itself here in `build()` so `show_toast` can find it.
//! - [`crate::toast::surface::ToastSurface`] — calls back into the
//!   registry to dismiss its entry on close-click / action-invoked.
//!
//! Routing: every live entry (and its mirrored archive row) carries a
//! resolved [`ToastRoute`] — the presenting window by default, or an
//! explicit audience/broadcast target. `max_visible` admission and
//! High/Urgent eviction are bucketed per route (see [`enqueue`]) so a
//! burst of one window's/audience's toasts can never starve another's
//! slot pool. [`ToastHost`](super::host::ToastHost) does the
//! complementary filtering on the render side.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use bastyde_core::signal::Signal;
use bastyde_core::styles::{SharedToastStyle, ToastPriority};
use bastyde_core::widget::{EventContext, Widget};
use bastyde_core::window::BastydeWindowId;

use crate::notification::{
    ArchivedAction, ArchivedActionStyle, NotificationArchiveModel, NotificationEntry,
};
use crate::toast::{
    Toast, ToastAction, ToastActionStyle, ToastAudience, ToastDismissCallback, ToastDismissCause,
    ToastHandle, ToastHandleInner, ToastRoute, ToastSeverity,
};
use bastyde_i18n::{LocalizedString, tr_widget};

/// Cheap to clone (`Rc<RefCell<…>>`). All public methods take `&self`
/// and use interior mutability.
#[derive(Clone)]
pub struct ToastRegistry {
    inner: Rc<RefCell<ToastRegistryInner>>,
    /// Shared hover-pause refcount. Each `ToastSurface` increments
    /// on pointer-enter, decrements on pointer-leave. Read by the
    /// host's frame-tick effect: `count > 0` pauses every entry's
    /// timer. Exposed via [`Self::hover_count_signal`] so the surface
    /// widget can wire its handler.
    hover_count: Signal<usize>,
    /// Version signal bumped on every queue mutation. The host binds
    /// to this at `BindingLevel::Rebuild` so any
    /// show/dismiss/timer-tick triggers a fresh host rebuild.
    version: Signal<u64>,
    /// Optional persistent / in-memory notification archive. Each
    /// enqueue mirrors the toast (when its `archive` flag is true)
    /// into the model — this is what
    /// [`NotificationLog`](crate::notification) renders and what
    /// drives the bell-button badge. `None` when the install helper
    /// was configured with `archive: None`.
    archive: Option<Rc<NotificationArchiveModel>>,
    /// Per-window audience assignment. `ToastHost::build` calls
    /// [`Self::window_audience_signal`] to get-or-create a stable
    /// signal for its own window id and binds to it at
    /// `BindingLevel::Rebuild`; app code retargets a window (e.g. when
    /// its active document changes) by calling
    /// [`Self::set_window_audience`] — reached the same way the
    /// registry itself is reached (`ctx.app_state::<ToastRegistry>()`),
    /// so no new `app_state` type is needed. Lives on the registry
    /// (not as a second `app_state` entry) because `app_state` holds
    /// exactly one instance per type for the whole app — there is no
    /// per-window slot to put this in anywhere else.
    window_audiences: Rc<RefCell<HashMap<BastydeWindowId, Signal<Option<ToastAudience>>>>>,
}

pub(crate) struct ToastRegistryInner {
    pub(crate) next_entry_id: u64,
    pub(crate) live_entries: VecDeque<LiveEntry>,
    /// Pending dismissal callbacks (cause + user callback) for entries
    /// whose timer fired in a frame-tick context. The host's
    /// `.on_pointer_event` handler drains this on the next pointer
    /// event so the user callback runs with a real `EventContext`.
    pub(crate) pending_user_dismiss_callbacks: Vec<(ToastDismissCause, ToastDismissCallback)>,
    /// Maximum simultaneous live entries PER ROUTE BUCKET (window /
    /// audience / broadcast each count separately) — overflow toasts
    /// are dropped with cause `SlotPoolFull` (Normal priority) or evict
    /// the oldest Normal entry in the SAME bucket (High / Urgent
    /// priority). See `enqueue`'s bucketing.
    pub(crate) max_visible: usize,
    pub(crate) pause_on_hover_group: bool,
}

/// Per-toast state owned by the registry, snapshotted into the
/// `ToastSurface` each host rebuild.
pub(crate) struct LiveEntry {
    pub(crate) entry_id: u64,
    pub(crate) severity: ToastSeverity,
    pub(crate) priority: ToastPriority,
    pub(crate) title: LocalizedString,
    pub(crate) body: Option<LocalizedString>,
    pub(crate) announcement: Option<LocalizedString>,
    pub(crate) actions: Rc<Vec<ToastAction>>,
    pub(crate) show_close_button: bool,
    pub(crate) closable_on_escape: bool,
    pub(crate) on_click: Option<Rc<dyn Fn(&mut EventContext)>>,
    pub(crate) on_dismiss: Option<ToastDismissCallback>,
    pub(crate) style_override: Option<SharedToastStyle>,
    /// `None` for persistent toasts. Decremented each frame by the
    /// host's frame-tick effect when the hover-pause refcount is zero.
    pub(crate) time_left: Option<Duration>,
    /// Boxed custom leading widget — `take()`-able exactly once when
    /// the surface is built. After the first build, subsequent
    /// rebuilds fall back to the default severity glyph.
    pub(crate) leading: Option<Box<dyn Widget>>,
    /// `Toast::id(...)` value. Consumed by `enqueue` for live
    /// update-in-place merge (a subsequent enqueue with a matching
    /// `id` mutates this entry rather than appending) and projected
    /// into `NotificationEntry::dedup_id` for the archive-side merge.
    pub(crate) id: Option<String>,
    /// `Toast::archive(true|false)` value. `false` opts the entry
    /// out of the persistent archive mirror (transient toasts like
    /// quick "Copied!" feedback that shouldn't pollute the log).
    pub(crate) archive: bool,
    /// Resolved delivery target — see [`ToastRoute`]. Drives both
    /// `ToastHost`'s render-side filter and the per-route slot-pool
    /// admission/eviction bucketing in [`ToastRegistry::enqueue`].
    pub(crate) route: ToastRoute,
    /// Whether this entry's body is clamped, unfolded, or short enough not to care
    /// (`crate::toast::body::BodyState`, as a scalar).
    ///
    /// It lives on the **entry**, not inside the body widget, because `ToastHost` builds
    /// a fresh `ToastSurface` on every rebuild — so a signal owned by the widget would
    /// reset each time any *other* toast arrived or expired, silently re-folding
    /// something the reader had just opened. The entry outlives every rebuild, and
    /// cloning a `Signal` shares its state, so threading it through
    /// `ToastSurfaceData` keeps the disclosure sticky for as long as the toast exists.
    pub(crate) body_state: Signal<u8>,
}

impl ToastRegistry {
    /// Construct a registry with the given options and no archive.
    /// Used by tests and by apps that don't want notification
    /// persistence. The install helper in bastyde calls
    /// [`with_archive`](Self::with_archive) instead.
    pub fn new(options: super::host::ToastInstallOptions) -> Self {
        Self::build(options, None)
    }

    /// Construct a registry that mirrors every archived-eligible
    /// toast push into `archive`. Toasts presented with
    /// `archive(false)` are NOT mirrored (used for transient
    /// "Copied!" feedback that shouldn't pollute the log).
    pub fn with_archive(
        options: super::host::ToastInstallOptions,
        archive: Rc<NotificationArchiveModel>,
    ) -> Self {
        Self::build(options, Some(archive))
    }

    fn build(
        options: super::host::ToastInstallOptions,
        archive: Option<Rc<NotificationArchiveModel>>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ToastRegistryInner {
                next_entry_id: 1,
                live_entries: VecDeque::new(),
                pending_user_dismiss_callbacks: Vec::new(),
                max_visible: options.max_visible,
                pause_on_hover_group: options.pause_on_hover_group,
            })),
            hover_count: Signal::new(0),
            version: Signal::new(0),
            archive,
            window_audiences: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Access the underlying notification archive (if configured).
    /// `NotificationLog` and `NotificationCenterButton` read from
    /// this directly.
    pub fn archive(&self) -> Option<Rc<NotificationArchiveModel>> {
        self.archive.clone()
    }

    /// Reactive signal bumped on every queue mutation. Every
    /// `ToastHost` binds this at `BindingLevel::Rebuild`, in every
    /// window, and app code may also poll it directly to assert "did
    /// something change" without going through a widget tree at all.
    ///
    /// One signal is enough for N windows. It was not always: dirty
    /// tracking used to be a `bool` living on the signal that each
    /// `WidgetTree`'s reconcile pass read *and cleared*, so whichever
    /// window reconciled first consumed the flag and every other
    /// window's `ToastHost` silently — and permanently — skipped its
    /// rebuild. Toast routing was the first feature to need
    /// shared-state-fanned-out-to-every-window, so it was the first to
    /// hit that, and it carried a `HashMap<BastydeWindowId, Signal<u64>>`
    /// of per-window duplicates plus a fan-out on every bump to work
    /// around it. `Signal` now tracks a monotone generation and each
    /// `BindingRegistry` remembers what it last acted on
    /// (`bastyde_core::binding::BindingGroup::last_seen`), so consumers
    /// no longer contend and the duplicates are gone.
    pub fn version_signal(&self) -> &Signal<u64> {
        &self.version
    }

    /// Shared hover-pause refcount. Surfaces increment / decrement
    /// on hover-enter / leave; the host's frame-tick effect reads it.
    pub fn hover_count_signal(&self) -> Signal<usize> {
        self.hover_count.clone()
    }

    /// Get-or-create the audience signal for `window_id`. The first
    /// call for a given window allocates a fresh `Signal::new(None)`;
    /// every later call (from that window's `ToastHost`, or from app
    /// code) returns the SAME signal, so binding to it once and
    /// mutating it later both work through this one accessor.
    pub fn window_audience_signal(&self, window_id: BastydeWindowId) -> Signal<Option<ToastAudience>> {
        self.window_audiences
            .borrow_mut()
            .entry(window_id)
            .or_insert_with(|| Signal::new(None))
            .clone()
    }

    /// Assign (or clear, with `None`) the audience for `window_id`.
    /// Retargets that window's toast host + bell immediately — both
    /// are bound to this signal at `BindingLevel::Rebuild`. Reached
    /// exactly like the registry itself: `ctx.app_state::<ToastRegistry>()`.
    /// Typical call site: a window-activation / active-document-changed
    /// handler that keeps a window's audience in sync with what it's
    /// currently showing.
    pub fn set_window_audience(&self, window_id: BastydeWindowId, audience: Option<ToastAudience>) {
        self.window_audience_signal(window_id).set(audience);
    }

    /// Drop `window_id`'s entry from [`Self::window_audiences`].
    /// Call this from the app's window-teardown hook — the same place
    /// that tears down the `ToastHost` mounted in that window.
    ///
    /// **`set_window_audience(window_id, None)` is NOT a substitute.**
    /// That call only overwrites the signal's *value*; the map entry
    /// (and the `Signal`'s backing `Rc<RefCell<..>>` allocation) stays
    /// alive. Without a call to `forget_window`, every window ever
    /// opened for the life of the process leaves one live `Signal` in
    /// the map behind forever — an unbounded leak in exactly the
    /// shape a long-running, multi-window app has (open/close windows
    /// repeatedly across a session).
    ///
    /// Safe even if some other code still holds a clone of the
    /// removed `Signal`: a `Signal` is `Rc<RefCell<..>>` under the
    /// hood, so dropping the registry's map entry only drops *this*
    /// reference to it — any clone a still-alive holder kept keeps
    /// reading/writing exactly as before, unaffected by the map
    /// removal (`Rc` content doesn't disappear just because one owner
    /// let go of it). The only real hazard is calling this too early:
    /// [`Self::window_audience_signal`] is get-or-create, so if the
    /// torn-down window's own `ToastHost` (or any other live widget)
    /// calls it again AFTER `forget_window`, it transparently
    /// allocates a brand-new `Signal::new(_)` under the same key
    /// rather than erroring — fine for a window that is genuinely gone
    /// (nothing is bound to the discarded signal any more, so no
    /// rebuild is missed), but it means this must be called from
    /// teardown itself, not from a handler the window's own event loop
    /// might still reach afterwards.
    ///
    /// Idempotent: forgetting a window id that was never registered
    /// (or was already forgotten) is a safe no-op — `HashMap::remove`
    /// on a missing key does nothing.
    pub fn forget_window(&self, window_id: BastydeWindowId) {
        self.window_audiences.borrow_mut().remove(&window_id);
    }

    /// Bump the version every `ToastHost` binds at
    /// `BindingLevel::Rebuild` — see [`Self::version_signal`]. One
    /// write reaches every window: each window's own `BindingRegistry`
    /// tracks the generation it last reconciled, so none of them can
    /// consume the notification out from under the others.
    pub(crate) fn bump_version(&self) {
        let v = self.version.get();
        self.version.set(v.wrapping_add(1));
    }

    /// Enqueue a toast. Called by `show_toast`. Returns a stable
    /// [`ToastHandle`]. Slot-pool exhaustion is evaluated PER ROUTE
    /// BUCKET (see [`ToastRoute`]): Normal-priority toasts are dropped
    /// with cause [`ToastDismissCause::SlotPoolFull`] once their own
    /// bucket is full; High / Urgent evict the oldest Normal entry in
    /// that SAME bucket — a burst of one window's or one audience's
    /// toasts never touches another's slots.
    pub(crate) fn enqueue(
        &self,
        toast: Toast,
    ) -> (
        ToastHandle,
        Option<(ToastDismissCause, ToastDismissCallback)>,
    ) {
        // Resolved once, up front: `toast.target` is `None` for a
        // toast that reached `enqueue` directly with no `EventContext`
        // and no explicit `.target()`/`.broadcast()` (the
        // `show_settings_write_failed` path) — `Broadcast` is the only
        // sensible default for an app-wide, contextless notification.
        // Every other caller (`EventContextToastExt::show_toast`) has
        // already resolved `None` to `Window(origin)` before this runs.
        let resolved_route = toast.target.unwrap_or(ToastRoute::Broadcast);
        let mut inner = self.inner.borrow_mut();

        // Update-in-place: a toast carrying a `Toast::id(...)` value
        // that matches an existing live entry mutates that entry's
        // fields instead of appending a new one. Reuses the existing
        // entry_id so the original `ToastHandle` (returned by the
        // first call) keeps working; resets the auto-dismiss timer.
        // Bypasses slot-pool admission (an update doesn't add a new
        // slot) and the on_dismiss-for-overflow path.
        if let Some(ref dedup_id) = toast.id
            && let Some(existing) = inner
                .live_entries
                .iter_mut()
                .find(|e| e.id.as_deref() == Some(dedup_id.as_str()))
        {
            let severity_changed = existing.severity != toast.severity;
            existing.severity = toast.severity;
            // A retargeting update (e.g. a progress toast whose
            // audience becomes known partway through) takes effect —
            // subsequent admission/eviction and host filtering use the
            // new route immediately.
            existing.route = resolved_route;
            existing.priority = toast.priority;
            existing.title = toast.title;
            existing.body = toast.body;
            existing.announcement = toast.announcement;
            existing.actions = Rc::new(toast.actions);
            existing.show_close_button = toast.show_close_button;
            existing.closable_on_escape = toast.closable_on_escape;
            existing.on_click = toast.on_click;
            // Replace the on_dismiss callback only if the update
            // provided one — apps that just want to update title /
            // body don't have to re-supply on_dismiss every time.
            // When the update DOES provide a new callback, the
            // previous one is dropped silently (never fires). The
            // contract is "the most recent caller's expectations win"
            // — `on_dismiss` fires once per entry, with the
            // most-recently-supplied callback.
            if toast.on_dismiss.is_some() {
                existing.on_dismiss = toast.on_dismiss;
            }
            existing.style_override = toast.style_override;
            existing.time_left = toast.auto_dismiss_after;
            // Leading widget: replace when the update sets one. Otherwise
            // *keep* the original (typically a Spinner from `Toast::loading`)
            // for a same-severity text-only update — EXCEPT when the severity
            // changed (e.g. a loading toast is updated to `success`): then the
            // stale custom leading (the spinner) must be dropped so the surface
            // shows the new severity's glyph (the ✓/✕/… icon), not a spinner
            // that keeps spinning under a "success" title.
            if toast.leading.is_some() {
                existing.leading = toast.leading;
            } else if severity_changed {
                existing.leading = None;
            }
            // `archive` flag tracks the latest call's intent. If
            // the update sets `archive(false)` after an initial
            // archived toast, the existing archive record stays in
            // place but this and subsequent updates stop mirroring
            // (no new `NotificationUpdate` is recorded). Apps that
            // want the archive to keep capturing updates should
            // leave `archive` at its default `true` across updates.
            existing.archive = toast.archive;
            let entry_id = existing.entry_id;
            // Snapshot for archive mirror BEFORE dropping the
            // RefCell borrow — the snapshot must be consistent with
            // the mutation, and `archive.push(...)` cannot run while
            // `inner` is still borrowed.
            let archive_entry = if existing.archive {
                Some(Self::entry_to_archive(existing))
            } else {
                None
            };
            drop(inner);
            if let (Some(archive), Some(entry)) = (self.archive.as_ref(), archive_entry) {
                archive.push(entry);
            }
            self.bump_version();
            let handle = ToastHandle::new(ToastHandleInner {
                entry_id,
                dismissed: std::cell::Cell::new(false),
                registry: self.clone(),
            });
            return (handle, None);
        }

        let entry_id = inner.next_entry_id;
        inner.next_entry_id += 1;

        // Slot-pool admission, bucketed per route (decision: `max_visible`
        // is a per-audience/per-window/per-broadcast budget, not a
        // whole-app one) — a burst of one window's or one audience's
        // toasts fills only ITS bucket, so it can never starve another
        // window's or audience's admission.
        let bucket_count = inner
            .live_entries
            .iter()
            .filter(|e| e.route == resolved_route)
            .count();
        let at_capacity = bucket_count >= inner.max_visible;
        if at_capacity {
            match toast.priority {
                ToastPriority::Normal => {
                    // Drop the new entry; fire its on_dismiss with
                    // SlotPoolFull synchronously to the caller via
                    // the returned handle's "dropped" state.
                    let cb = toast.on_dismiss.clone();
                    drop(inner);
                    let handle = ToastHandle::new(ToastHandleInner {
                        entry_id,
                        dismissed: std::cell::Cell::new(true),
                        registry: self.clone(),
                    });
                    if let Some(cb) = cb {
                        return (handle, Some((ToastDismissCause::SlotPoolFull, cb)));
                    }
                    return (handle, None);
                }
                ToastPriority::High | ToastPriority::Urgent => {
                    // Evict the oldest Normal-priority entry WITHIN
                    // this same route bucket — a High/Urgent arrival
                    // for one audience must never bump an unrelated
                    // window's/audience's Normal entry out of its slot.
                    let evict_idx = inner
                        .live_entries
                        .iter()
                        .position(|e| e.route == resolved_route && matches!(e.priority, ToastPriority::Normal));
                    if let Some(idx) = evict_idx {
                        let removed = inner.live_entries.remove(idx).unwrap();
                        if let Some(cb) = removed.on_dismiss.clone() {
                            // Stash the bumped callback; the caller
                            // returns it for the framework to drain
                            // on the next pointer event.
                            inner
                                .pending_user_dismiss_callbacks
                                .push((ToastDismissCause::SlotPoolFull, cb));
                        }
                    }
                    // If no Normal entry to evict, the new toast still
                    // joins the live set (queue grows above max_visible
                    // until something dismisses).
                }
            }
        }

        let auto_dismiss = toast.auto_dismiss_after;
        let entry = LiveEntry {
            entry_id,
            severity: toast.severity,
            priority: toast.priority,
            title: toast.title,
            body: toast.body,
            announcement: toast.announcement,
            actions: Rc::new(toast.actions),
            show_close_button: toast.show_close_button,
            closable_on_escape: toast.closable_on_escape,
            on_click: toast.on_click,
            on_dismiss: toast.on_dismiss,
            style_override: toast.style_override,
            time_left: auto_dismiss,
            leading: toast.leading,
            id: toast.id,
            archive: toast.archive,
            route: resolved_route,
            // Starts at `Fits`; the body's own layout pass decides whether there is
            // anything to disclose. See `crate::toast::body`.
            body_state: Signal::new(0),
        };
        // Mirror to the archive BEFORE the entry is pushed to the
        // live queue — that way an `archive(false)` toast (e.g. a
        // quick "Copied!" feedback) is excluded, but a normal toast's
        // archive record is populated even if a subsequent
        // priority-eviction immediately knocks it out of the live set
        // (the user still saw it; the log row is what survives).
        if let Some(archive) = self.archive.as_ref() {
            if entry.archive {
                archive.push(Self::entry_to_archive(&entry));
            }
        }

        inner.live_entries.push_back(entry);
        drop(inner);
        self.bump_version();

        let handle = ToastHandle::new(ToastHandleInner {
            entry_id,
            dismissed: std::cell::Cell::new(false),
            registry: self.clone(),
        });
        (handle, None)
    }

    /// Enqueue the framework's toast for a permanently-discarded
    /// `bastyde-settings` write — the write-side counterpart of
    /// `AppEvent::SettingsWriteFailed` (a `DebouncedWriter` gave up
    /// after `MAX_WRITE_ATTEMPTS` retries, or was force-flushed still
    /// failing at process teardown, and its queued patches were
    /// dropped). This is data loss, not a status blip: `Error` severity
    /// and persistent (no auto-dismiss), naming the file that failed.
    ///
    /// Framework-level and crate-internal to the join point: the
    /// locale-validated strings can only live in bastyde-widgets
    /// (`tr_widget!` resolves against *this* crate's own
    /// `locales/*.ftl`), so the toast is built here rather than at the
    /// call site. `bastyde::install_toast` (the umbrella crate — the
    /// one place that sees both `bastyde-app`'s `AppEvent` and this
    /// `ToastRegistry`) calls this from a
    /// `BastydeAppBuilder::register_app_event_observer` closure, so
    /// every app with toast installed surfaces the loss automatically,
    /// with no per-app wiring.
    ///
    /// No `EventContext` is available at the call site — this fires
    /// from a background `AppEvent` observer, not a widget event
    /// handler — so this goes straight to `enqueue` rather than
    /// through `EventContextToastExt::show_toast`. The only situation
    /// `enqueue` needs a context for is invoking the slot-pool-overflow
    /// `on_dismiss` callback; this toast never sets one, so if the pool
    /// is already full and this arrival evicts/drops an entry, there is
    /// nothing behind that callback to lose — the overflow result is
    /// dropped here deliberately, not silently.
    pub fn show_settings_write_failed(
        &self,
        file_name: &str,
        attempts: u32,
        dropped_patches: usize,
        message: &str,
    ) {
        let toast = Toast::error(tr_widget!(settings_write_failed_toast_title()))
            .body(tr_widget!(settings_write_failed_toast_body(
                file = file_name.to_string(),
                attempts = attempts,
                dropped = dropped_patches as i64,
                message = message.to_string(),
            )))
            .persistent()
            .priority(ToastPriority::High);
        let (_handle, overflow) = self.enqueue(toast);
        // Deliberately dropped — see the doc comment above.
        drop(overflow);
    }

    /// Project a `LiveEntry` (the in-memory toast state) into a
    /// `NotificationEntry` (the persistent archive shape). Drops
    /// callbacks (`on_dismiss`, `on_click`, action callbacks) — only
    /// `intent_name` on actions survives, used by the log for replay
    /// through the existing intent dispatcher.
    fn entry_to_archive(entry: &LiveEntry) -> NotificationEntry {
        NotificationEntry {
            id: 0, // overwritten by NotificationArchiveModel::push
            severity: entry.severity,
            priority: entry.priority,
            title: entry.title.resolve_now(),
            body: entry.body.as_ref().map(|b| b.resolve_now()),
            actions: entry.actions.iter().map(Self::action_to_archive).collect(),
            timestamp: jiff::Timestamp::now(),
            group: None,
            source: None,
            read: false,
            dedup_id: entry.id.clone(),
            updates: Vec::new(),
            route: entry.route,
        }
    }

    fn action_to_archive(action: &ToastAction) -> ArchivedAction {
        ArchivedAction {
            label: action.label().to_string(),
            intent_name: action.shortcut_id_ref().map(|s| s.to_string()),
            style: match action.style_ref() {
                ToastActionStyle::Link => ArchivedActionStyle::Link,
                ToastActionStyle::Button { variant } => {
                    use crate::button::ButtonVariant;
                    match variant {
                        ButtonVariant::Filled => ArchivedActionStyle::PrimaryButton,
                        ButtonVariant::Destructive => ArchivedActionStyle::Destructive,
                        _ => ArchivedActionStyle::SecondaryButton,
                    }
                }
            },
            closes_on_invoke: action.closes_toast_flag(),
        }
    }

    /// Whether an entry with the given id is still in the live set.
    pub(crate) fn is_entry_alive(&self, entry_id: u64) -> bool {
        self.inner
            .borrow()
            .live_entries
            .iter()
            .any(|e| e.entry_id == entry_id)
    }

    /// Remove the entry from the live set and queue its on_dismiss
    /// callback to fire from `ctx`. Called from event handlers
    /// (close-click, action-invoked, escape, programmatic).
    pub(crate) fn dismiss_entry(
        &self,
        entry_id: u64,
        cause: ToastDismissCause,
        ctx: &mut EventContext,
    ) {
        let removed = {
            let mut inner = self.inner.borrow_mut();
            let idx = inner
                .live_entries
                .iter()
                .position(|e| e.entry_id == entry_id);
            idx.and_then(|i| inner.live_entries.remove(i))
        };
        if let Some(entry) = removed {
            self.bump_version();
            if let Some(cb) = entry.on_dismiss.clone() {
                cb(cause, ctx);
            }
        }
    }

    /// Same as `dismiss_entry` but defers the user callback to a
    /// later pointer event — used by the frame-tick timer path which
    /// doesn't have an `EventContext`.
    pub(crate) fn dismiss_entry_deferred(&self, entry_id: u64, cause: ToastDismissCause) {
        let removed = {
            let mut inner = self.inner.borrow_mut();
            let idx = inner
                .live_entries
                .iter()
                .position(|e| e.entry_id == entry_id);
            idx.and_then(|i| inner.live_entries.remove(i))
        };
        if let Some(entry) = removed
            && let Some(cb) = entry.on_dismiss.clone()
        {
            self.inner
                .borrow_mut()
                .pending_user_dismiss_callbacks
                .push((cause, cb));
        }
        self.bump_version();
    }

    /// Drain pending dismiss callbacks accumulated from timer-driven
    /// expiries. Called by the host's `on_pointer_event` handler so
    /// the user callbacks fire with a live `EventContext`.
    pub(crate) fn drain_pending_dismiss_callbacks(&self, ctx: &mut EventContext) {
        let drained: Vec<_> = {
            let mut inner = self.inner.borrow_mut();
            std::mem::take(&mut inner.pending_user_dismiss_callbacks)
        };
        for (cause, cb) in drained {
            cb(cause, ctx);
        }
    }

    /// Tick the per-entry timers by `dt`. When `paused` is true (any
    /// surface is hovered or focused), this is a no-op. Returns `true`
    /// if at least one entry expired (host then bumps the version
    /// signal and rebuilds, dropping the surfaces). Called from the
    /// host's frame-tick effect.
    pub(crate) fn tick_timers(&self, dt: Duration, paused: bool) -> bool {
        if paused {
            return false;
        }
        let mut expired = Vec::new();
        {
            let mut inner = self.inner.borrow_mut();
            for entry in inner.live_entries.iter_mut() {
                if let Some(remaining) = entry.time_left.as_mut() {
                    *remaining = remaining.saturating_sub(dt);
                    if remaining.is_zero() {
                        expired.push(entry.entry_id);
                    }
                }
            }
        }
        let any = !expired.is_empty();
        for entry_id in expired {
            self.dismiss_entry_deferred(entry_id, ToastDismissCause::Timeout);
        }
        any
    }

    /// Whether any live entry has a finite, still-running auto-dismiss
    /// timer. The host gates its per-frame `frame_tick` subscription on
    /// this: with no running timer (the queue is empty, or every live
    /// toast is sticky / `time_left == None`) there is nothing to
    /// decrement each frame, so the host drops the subscription and lets
    /// the event loop sleep. Without this gate an empty toast host kept
    /// the loop awake at ~60 fps forever (a steady idle-CPU drain).
    pub(crate) fn has_running_timers(&self) -> bool {
        self.inner
            .borrow()
            .live_entries
            .iter()
            .any(|e| e.time_left.is_some())
    }

    /// Smallest remaining auto-dismiss duration among live timed
    /// toasts, or `None` if no toast has a running timer. The host uses
    /// this to schedule a single `wake_at` deadline at the soonest
    /// expiry instead of polling every frame — so a visible-but-idle
    /// toast lets the event loop sleep.
    pub(crate) fn min_running_timer(&self) -> Option<std::time::Duration> {
        self.inner
            .borrow()
            .live_entries
            .iter()
            .filter_map(|e| e.time_left)
            .min()
    }

    /// Read-only snapshot of live entry ids — the host's `build()`
    /// uses this to know how many surfaces to construct, in what
    /// order. The actual entry data is read via `with_entry`.
    pub(crate) fn live_entry_ids(&self) -> Vec<u64> {
        self.inner
            .borrow()
            .live_entries
            .iter()
            .map(|e| e.entry_id)
            .collect()
    }

    pub(crate) fn with_entry<R>(
        &self,
        entry_id: u64,
        f: impl FnOnce(&LiveEntry) -> R,
    ) -> Option<R> {
        self.inner
            .borrow()
            .live_entries
            .iter()
            .find(|e| e.entry_id == entry_id)
            .map(f)
    }

    /// Cancel an entry's auto-dismiss timer, making it persistent for the rest of its
    /// life. Idempotent; a no-op for an entry that was already persistent or has gone.
    ///
    /// Called when a reader unfolds a clamped body. Hovering already pauses the timer, so
    /// the toast survives while the pointer rests on it — but a reader who unfolds three
    /// lines into ten and then moves the mouse away to read comfortably would otherwise
    /// watch it vanish mid-sentence, which is precisely the frustration the disclosure
    /// exists to remove. Asking to see more is a clear statement that the toast is being
    /// read; the close button (and the notification archive) remain the way out.
    pub(crate) fn cancel_auto_dismiss(&self, entry_id: u64) {
        let mut inner = self.inner.borrow_mut();
        if let Some(entry) = inner
            .live_entries
            .iter_mut()
            .find(|e| e.entry_id == entry_id)
        {
            entry.time_left = None;
        }
    }

    /// Take the boxed `leading` widget out of the entry — call exactly
    /// once per entry. Subsequent rebuilds get `None` and fall back
    /// to the default severity glyph.
    pub(crate) fn take_leading(&self, entry_id: u64) -> Option<Box<dyn Widget>> {
        let mut inner = self.inner.borrow_mut();
        inner
            .live_entries
            .iter_mut()
            .find(|e| e.entry_id == entry_id)
            .and_then(|e| e.leading.take())
    }

    pub(crate) fn pause_on_hover_group(&self) -> bool {
        self.inner.borrow().pause_on_hover_group
    }

    /// Test-only: how many entries are currently live.
    pub fn live_count(&self) -> usize {
        self.inner.borrow().live_entries.len()
    }
}

impl std::fmt::Debug for ToastRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.borrow();
        f.debug_struct("ToastRegistry")
            .field("live_count", &inner.live_entries.len())
            .field("max_visible", &inner.max_visible)
            .field(
                "pending_callbacks",
                &inner.pending_user_dismiss_callbacks.len(),
            )
            .field("hover_count", &self.hover_count.get())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toast::Toast;
    use crate::toast::host::ToastInstallOptions;
    use bastyde_i18n::lit;

    fn registry() -> ToastRegistry {
        ToastRegistry::new(ToastInstallOptions {
            archive: None,
            ..ToastInstallOptions::default()
        })
    }

    #[test]
    fn severity_change_drops_stale_custom_leading() {
        let r = registry();
        // A loading toast carries a Spinner as its custom leading.
        let _ = r.enqueue(Toast::loading(lit!("Working")).id("op"));
        let eid = r.live_entry_ids()[0];
        assert!(
            r.with_entry(eid, |e| e.leading.is_some()).unwrap(),
            "loading toast starts with a spinner leading"
        );

        // Update-in-place to a success toast (different severity, no custom
        // leading) → the stale spinner must be cleared so the surface shows the
        // success glyph, not a spinner spinning under a "success" title.
        let _ = r.enqueue(Toast::success(lit!("Done")).id("op"));
        assert!(
            !r.with_entry(eid, |e| e.leading.is_some()).unwrap(),
            "a severity change without a new leading drops the stale spinner"
        );
    }

    #[test]
    fn same_severity_text_update_keeps_leading() {
        let r = registry();
        // `loading` = Info severity + Spinner.
        let _ = r.enqueue(Toast::loading(lit!("0%")).id("op"));
        let eid = r.live_entry_ids()[0];
        // A same-severity (Info) text-only update with no custom leading keeps
        // the spinner — so a progress toast's spinner survives its text updates.
        let _ = r.enqueue(Toast::info(lit!("50%")).id("op"));
        assert!(
            r.with_entry(eid, |e| e.leading.is_some()).unwrap(),
            "a same-severity text update preserves the existing spinner"
        );
    }

    #[test]
    fn show_settings_write_failed_enqueues_a_persistent_error_toast_naming_the_file() {
        // F3: this is the framework-level join that turns a permanently
        // discarded `bastyde-settings` write into something the user
        // actually sees. Assert on registry state (severity, persistence,
        // the file name landing in the resolved body), not pixels.
        let r = registry();
        r.show_settings_write_failed("window_state.toml", 5, 3, "disk full");

        assert_eq!(r.live_count(), 1, "the failure enqueues exactly one toast");
        let eid = r.live_entry_ids()[0];
        r.with_entry(eid, |e| {
            assert_eq!(
                e.severity,
                ToastSeverity::Error,
                "settings data loss is Error severity, not a status blip"
            );
            assert!(
                e.time_left.is_none(),
                "the toast is persistent — no auto-dismiss for data loss"
            );
            let body = e.body.as_ref().expect("body must be set").resolve_now();
            assert!(
                body.contains("window_state.toml"),
                "the failing file's name must appear in the body: {body:?}"
            );
        })
        .unwrap();
    }

    #[test]
    fn show_settings_write_failed_is_high_priority_and_survives_pool_pressure() {
        // `show_settings_write_failed`'s toast is High priority (data
        // loss deserves to be seen even when the pool is already full of
        // routine Normal-priority toasts) and never attaches its own
        // `on_dismiss`, so there's nothing behind the evicted entry's
        // slot-pool-overflow callback path to lose. This proves the call
        // completes cleanly under pool pressure (no `EventContext`
        // available to invoke any overflow callback with) and that the
        // settings-failure toast wins the slot rather than being dropped
        // like a Normal-priority arrival would be.
        let r = ToastRegistry::new(ToastInstallOptions {
            archive: None,
            max_visible: 1,
            ..ToastInstallOptions::default()
        });
        let _ = r.enqueue(Toast::info(lit!("already here")));
        assert_eq!(r.live_count(), 1);

        r.show_settings_write_failed("settings.toml", 5, 1, "read-only filesystem");

        assert_eq!(
            r.live_count(),
            1,
            "High priority evicts the oldest Normal entry rather than growing past capacity"
        );
        let eid = r.live_entry_ids()[0];
        r.with_entry(eid, |e| {
            assert_eq!(
                e.severity,
                ToastSeverity::Error,
                "the settings-failure toast must win the slot, not the evicted one"
            );
        })
        .unwrap();
    }

    // ----- F3: `forget_window` -----

    #[test]
    fn forget_window_removes_the_windows_audience_entry() {
        let r = registry();
        let w = BastydeWindowId::new(1);

        r.set_window_audience(w, Some(ToastAudience::new(42)));
        assert!(r.window_audiences.borrow().contains_key(&w));

        r.forget_window(w);

        assert!(
            !r.window_audiences.borrow().contains_key(&w),
            "forget_window must remove the window's audience-map entry"
        );
    }

    /// Audience assignment is per-window state and stays that way.
    /// Rebuild *notification* is not: one `version_signal` reaches
    /// every window's `ToastHost`, because each window's own
    /// `BindingRegistry` tracks the generation it last reconciled.
    #[test]
    fn one_version_signal_notifies_every_windows_host() {
        use bastyde_core::binding::{BindingLevel, BindingRegistry};
        use bastyde_core::widget_id::WidgetId;

        let r = registry();
        let host: WidgetId = slotmap::KeyData::from_ffi(1).into();
        let windows: Vec<BindingRegistry> = (0..3).map(|_| BindingRegistry::new()).collect();
        for reg in &windows {
            r.version_signal().bind_to(host, reg, BindingLevel::Rebuild);
        }

        let (_handle, _overflow) = r.enqueue(Toast::info(lit!("hello")));

        for (i, reg) in windows.iter().enumerate() {
            assert!(
                reg.any_dirty(),
                "window {i}'s host missed the enqueue — an earlier window \
                 looking must not consume it"
            );
        }
    }

    #[test]
    fn forget_window_on_an_unknown_window_is_a_safe_no_op() {
        let r = registry();
        let known = BastydeWindowId::new(1);
        let unknown = BastydeWindowId::new(999);
        r.set_window_audience(known, Some(ToastAudience::new(7)));

        // Forgetting a window that was never registered must not
        // panic and must not disturb any other window's entries.
        r.forget_window(unknown);

        assert!(
            r.window_audiences.borrow().contains_key(&known),
            "an unrelated window's entry must survive forgetting a different, unknown window"
        );

        // Forgetting it twice (idempotent teardown, or a duplicate
        // teardown hook call) is equally a safe no-op.
        r.forget_window(known);
        r.forget_window(known);
        assert!(!r.window_audiences.borrow().contains_key(&known));
    }

    #[test]
    fn forgetting_a_windows_audience_does_not_panic_a_still_live_toast_routed_to_it() {
        // Reproduces the exact shape `forget_window` must handle
        // safely: a window is torn down (and forgotten) while a toast
        // that was routed to its audience is still live in the queue.
        // Nothing about tearing down the window's map entries should
        // reach into, or otherwise disturb, unrelated live entries —
        // the entry keeps existing with its already-resolved route
        // until something explicitly dismisses it.
        let r = registry();
        let w = BastydeWindowId::new(1);
        let audience = ToastAudience::new(11);
        r.set_window_audience(w, Some(audience));

        let (_handle, overflow) = r.enqueue(Toast::info(lit!("still going")).target(audience));
        assert!(overflow.is_none());
        assert_eq!(r.live_count(), 1);

        // The window closes: app code forgets it.
        r.forget_window(w);

        // The live entry (already routed to the audience, independent
        // of the now-removed per-window map entries) is untouched.
        assert_eq!(
            r.live_count(),
            1,
            "forgetting the window must not evict or otherwise touch live entries"
        );
        let eid = r.live_entry_ids()[0];
        r.with_entry(eid, |e| {
            assert_eq!(e.route, ToastRoute::Audience(audience));
        })
        .unwrap();

        // A further enqueue against the now-forgotten window's old
        // audience still works fine (routing lives on the entry /
        // resolved at enqueue time, not on the per-window maps) —
        // proves nothing panics or gets corrupted by the map removal.
        let (_h2, overflow2) = r.enqueue(Toast::info(lit!("another one")).target(audience));
        assert!(overflow2.is_none());
        assert_eq!(r.live_count(), 2);

        // And re-deriving a signal for the forgotten window id after
        // the fact is safe too — get-or-create transparently
        // allocates a fresh one (documented behaviour, not a panic).
        let fresh = r.window_audience_signal(w);
        assert!(fresh.get().is_none(), "a re-created signal starts fresh, not with the old audience");
    }

    /// An archive mirrors every enqueue, and its own version signal
    /// must reach every window too — the bell in window B updates for
    /// a toast raised from window A. Covers the registry → archive
    /// hand-off, which is where the mirrored bump originates.
    #[test]
    fn a_mirrored_push_notifies_every_windows_bell() {
        use bastyde_core::binding::{BindingLevel, BindingRegistry};
        use bastyde_core::widget_id::WidgetId;

        let archive = std::rc::Rc::new(NotificationArchiveModel::in_memory());
        let r = ToastRegistry::with_archive(ToastInstallOptions::default(), archive.clone());
        let bell: WidgetId = slotmap::KeyData::from_ffi(2).into();
        let windows: Vec<BindingRegistry> = (0..2).map(|_| BindingRegistry::new()).collect();
        for reg in &windows {
            archive
                .version_signal()
                .bind_to(bell, reg, BindingLevel::Rebuild);
        }

        let (_h, _overflow) = r.enqueue(Toast::info(lit!("mirrored")));

        for (i, reg) in windows.iter().enumerate() {
            assert!(reg.any_dirty(), "window {i}'s bell missed the mirrored push");
        }
    }

    #[test]
    fn forget_window_without_an_archive_configured_does_not_panic() {
        // Registries built via `new` (no archive) must tear down as a
        // safe no-op, not unwrap a `None`.
        let r = registry();
        assert!(r.archive().is_none());
        let w = BastydeWindowId::new(1);
        r.set_window_audience(w, Some(ToastAudience::new(3)));

        r.forget_window(w);

        assert!(!r.window_audiences.borrow().contains_key(&w));
    }
}
