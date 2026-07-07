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
//! Current limitation: a single shared host state. Multi-window apps
//! that install `ToastHost` in every window share one queue; toasts
//! show up in whichever window's host last bound. Multi-window
//! routing is planned as a follow-up.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use bastyde_core::signal::Signal;
use bastyde_core::styles::{SharedToastStyle, ToastPriority};
use bastyde_core::widget::{EventContext, Widget};

use crate::notification::{
    ArchivedAction, ArchivedActionStyle, NotificationArchiveModel, NotificationEntry,
};
use crate::toast::{
    Toast, ToastAction, ToastActionStyle, ToastDismissCallback, ToastDismissCause, ToastHandle,
    ToastHandleInner, ToastSeverity,
};
use bastyde_i18n::LocalizedString;

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
}

pub(crate) struct ToastRegistryInner {
    pub(crate) next_entry_id: u64,
    pub(crate) live_entries: VecDeque<LiveEntry>,
    /// Pending dismissal callbacks (cause + user callback) for entries
    /// whose timer fired in a frame-tick context. The host's
    /// `.on_pointer_event` handler drains this on the next pointer
    /// event so the user callback runs with a real `EventContext`.
    pub(crate) pending_user_dismiss_callbacks: Vec<(ToastDismissCause, ToastDismissCallback)>,
    /// Maximum simultaneous live entries — overflow toasts are dropped
    /// with cause `SlotPoolFull` (Normal priority) or evict the
    /// oldest Normal entry (High / Urgent priority).
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
        }
    }

    /// Access the underlying notification archive (if configured).
    /// `NotificationLog` and `NotificationCenterButton` read from
    /// this directly.
    pub fn archive(&self) -> Option<Rc<NotificationArchiveModel>> {
        self.archive.clone()
    }

    /// Reactive signal bumped on every queue mutation. The host binds
    /// to this at `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> &Signal<u64> {
        &self.version
    }

    /// Shared hover-pause refcount. Surfaces increment / decrement
    /// on hover-enter / leave; the host's frame-tick effect reads it.
    pub fn hover_count_signal(&self) -> Signal<usize> {
        self.hover_count.clone()
    }

    pub(crate) fn bump_version(&self) {
        let v = self.version.get();
        self.version.set(v.wrapping_add(1));
    }

    /// Enqueue a toast. Called by `show_toast`. Returns a stable
    /// [`ToastHandle`]. Slot-pool exhaustion: Normal-priority toasts
    /// are dropped with cause [`ToastDismissCause::SlotPoolFull`];
    /// High / Urgent evict the oldest Normal entry.
    pub(crate) fn enqueue(
        &self,
        toast: Toast,
    ) -> (
        ToastHandle,
        Option<(ToastDismissCause, ToastDismissCallback)>,
    ) {
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

        // Slot-pool admission.
        let at_capacity = inner.live_entries.len() >= inner.max_visible;
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
                    // Evict the oldest Normal-priority entry.
                    let evict_idx = inner
                        .live_entries
                        .iter()
                        .position(|e| matches!(e.priority, ToastPriority::Normal));
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
}
