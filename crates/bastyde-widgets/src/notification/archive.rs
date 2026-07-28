// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `NotificationArchiveModel` — the persistent list backing
//! [`NotificationLog`](crate::notification::log::NotificationLog) and the bell-icon badge.
//!
//! Wraps a [`ListModel<NotificationEntry>`](bastyde_data::ListModel)
//! with two extras:
//! - bounded eviction (oldest entries drop when the configured
//!   `limit` is exceeded);
//! - a `Signal<usize>` `unread_count` that increments on push and
//!   resets to zero on [`mark_all_read`](NotificationArchiveModel::mark_all_read).
//!
//! Two storage variants are supported via [`NotificationArchive`]:
//! - `InMemory` — session-only.
//! - `Persistent { path }` — file-backed via
//!   [`PersistedListModel`].
//!   Apps install via `ToastInstallOptions::archive = Some(
//!   NotificationArchive::persistent(...))`; the registry's
//!   `enqueue` push goes through the model and the on-disk file
//!   gets re-serialized on the shared bastyde-settings I/O thread.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use bastyde_core::signal::Signal;
use bastyde_core::window::BastydeWindowId;
use bastyde_data::ListModel;
use bastyde_settings::{AppPaths, Migrator, PersistedListModel, SettingsFileError};

use crate::notification::NotificationEntry;

/// Default per-archive entry cap. IntelliJ's notification log keeps
/// hundreds of entries with no cap visible to the user; we pick a
/// pragmatic limit so persistent files don't grow unbounded.
pub const DEFAULT_ARCHIVE_LIMIT: usize = 200;

/// File-name (without extension) used for the persistent archive.
/// Resolved through [`AppPaths::config_file`] into
/// `<config_dir>/<app>/notifications.toml`.
pub const ARCHIVE_FILE_NAME: &str = "notifications";

/// Storage mode for the notification archive. Passed inside
/// `ToastInstallOptions::archive` to the install helper.
#[derive(Debug, Clone)]
pub enum NotificationArchive {
    /// Session-only — entries live in a `ListModel` for the running
    /// session. Cheap, no disk I/O. Default for apps that don't
    /// install a `SettingsBundle`.
    InMemory { limit: usize },
    /// File-backed via `PersistedListModel`. The path is built at
    /// install time from [`AppPaths::config_file`] using the configured
    /// `file_name`.
    Persistent { file_name: String, limit: usize },
}

impl NotificationArchive {
    /// In-memory archive with the default 200-entry cap.
    pub fn in_memory() -> Self {
        Self::InMemory {
            limit: DEFAULT_ARCHIVE_LIMIT,
        }
    }

    /// In-memory archive with a custom cap.
    pub fn in_memory_with_limit(limit: usize) -> Self {
        Self::InMemory { limit }
    }

    /// File-backed archive resolved through `AppPaths::config_file`
    /// at install time. The default file name (`"notifications"`)
    /// yields `<config_dir>/<app>/notifications.toml`. Apps that
    /// want a different name pass it here; tests pass an arbitrary
    /// name and use `AppPaths::for_testing(tmpdir)`.
    pub fn persistent(file_name: impl Into<String>) -> Self {
        Self::Persistent {
            file_name: file_name.into(),
            limit: DEFAULT_ARCHIVE_LIMIT,
        }
    }

    pub fn persistent_with_limit(file_name: impl Into<String>, limit: usize) -> Self {
        Self::Persistent {
            file_name: file_name.into(),
            limit,
        }
    }

    pub fn limit(&self) -> usize {
        match self {
            Self::InMemory { limit } | Self::Persistent { limit, .. } => *limit,
        }
    }
}

/// Errors during archive construction or persistence I/O.
#[derive(Debug, thiserror::Error)]
pub enum NotificationArchiveError {
    /// Couldn't load / write the persistent-backing file. Maps to a
    /// `SettingsFileError`. Apps usually surface this once at startup
    /// and fall back to `InMemory` for the session.
    #[error("notification archive file I/O failed: {0}")]
    File(#[from] SettingsFileError),
}

/// Either an in-memory `ListModel` or a persistent one — exposed
/// uniformly through `NotificationArchiveModel::entries()`. Internal
/// detail; apps work with the model.
///
/// Every *mutation* goes through one of this type's own methods
/// (`upsert_front` / `update_in_place` / `remove` / `clear`), never
/// through `model()` directly: for the `Persistent` variant,
/// `PersistedListModel::model()` is read/reactive-binding-only —
/// mutating it directly would update the live `ListModel` but never
/// touch disk. Each method updates both variants identically from the
/// caller's point of view (id-keyed, matching
/// [`NotificationEntry`]'s [`Keyed`](bastyde_settings::Keyed) impl), so
/// `NotificationArchiveModel` never has to branch on which backend it
/// holds.
enum ArchiveBackend {
    InMemory(ListModel<NotificationEntry>),
    Persistent(PersistedListModel<NotificationEntry>),
}

impl ArchiveBackend {
    fn model(&self) -> &ListModel<NotificationEntry> {
        match self {
            Self::InMemory(m) => m,
            Self::Persistent(p) => p.model(),
        }
    }

    /// Find the entry with `id` and its current index, via the
    /// live reactive model (works identically for both variants: for
    /// `Persistent`, the live model always mirrors on-disk content).
    fn find_by_id(&self, id: u64) -> Option<(usize, NotificationEntry)> {
        let model = self.model();
        (0..model.len()).find_map(|i| {
            model
                .with_item(i, |e| e.clone())
                .filter(|e| e.id == id)
                .map(|e| (i, e))
        })
    }

    /// Insert `entry` at the front. `entry.id` is always freshly
    /// stamped and therefore unique, so this never actually collides
    /// with (and removes) an existing row — it's a plain prepend.
    fn upsert_front(&self, entry: NotificationEntry) {
        match self {
            Self::InMemory(m) => m.insert(0, entry),
            Self::Persistent(p) => p.upsert_front(entry),
        }
    }

    /// Replace the entry with `entry.id` in place (no reordering).
    /// Returns whether an entry with that id existed.
    fn update_in_place(&self, entry: NotificationEntry) -> bool {
        match self {
            Self::InMemory(m) => match self.find_by_id(entry.id) {
                Some((idx, _)) => {
                    m.set(idx, entry);
                    true
                }
                None => false,
            },
            Self::Persistent(p) => p.update_in_place(entry),
        }
    }

    /// Remove the entry with `id`, if present. Returns whether
    /// anything was removed.
    fn remove(&self, id: u64) -> bool {
        match self {
            Self::InMemory(m) => match self.find_by_id(id) {
                Some((idx, _)) => {
                    m.remove(idx);
                    true
                }
                None => false,
            },
            Self::Persistent(p) => p.remove(&id),
        }
    }

    fn clear(&self) {
        match self {
            Self::InMemory(m) => m.clear(),
            Self::Persistent(p) => p.clear(),
        }
    }

    fn flush_now(&self) -> Result<(), SettingsFileError> {
        match self {
            Self::InMemory(_) => Ok(()),
            Self::Persistent(p) => p.flush_now(),
        }
    }
}

/// Shared model — clones share state. Constructed by the install
/// helper from `NotificationArchive` + `AppPaths`; apps reach it
/// via `ctx.app_state::<Rc<RefCell<NotificationArchiveModel>>>()`.
///
/// `NotificationLog` and `NotificationCenterButton`
/// consume this model directly.
pub struct NotificationArchiveModel {
    backend: ArchiveBackend,
    limit: usize,
    /// Stable monotonically-increasing per-archive id stamped onto
    /// each new entry via `next_id.update(|n| n+1)`. Independent of
    /// the runtime `entry_id` on `ToastHandle` (the toast IDs are
    /// per-session; archive IDs persist across restarts).
    next_id: Cell<u64>,
    /// Live unread count. Increments on `push` of an unread entry,
    /// resets to zero on `mark_all_read`. Drives the bell-button
    /// badge.
    unread_count: Signal<usize>,
    /// Monotonic version bumped on every mutation (push, in-place
    /// update, mark_all_read, clear, remove). The
    /// [`NotificationLog`](super::log::NotificationLog) binds to
    /// this at `BindingLevel::Rebuild` so any archive change
    /// triggers a fresh log rebuild — needed for the day-bucket
    /// header re-computation. Same shape as
    /// [`OverlayManager::version`](bastyde_core::overlay::OverlayManager::version)
    /// and [`ToastRegistry::version_signal`](crate::toast::ToastRegistry::version_signal).
    version: Signal<u64>,
    /// Per-window rebuild signal — mirrors
    /// [`ToastRegistry::window_version_signal`](crate::toast::ToastRegistry::window_version_signal).
    /// A single `Signal` shared by every window's `NotificationCenterButton`
    /// / `NotificationLog` has exactly the same failure mode as the
    /// toast registry's shared `version` would: only the first
    /// window's `WidgetTree` to reconcile after a mutation observes
    /// the dirty flag before clearing it, so every other open window's
    /// bell/log silently never rebuilds for that mutation. See that
    /// method's doc comment for the full mechanism.
    window_versions: Rc<RefCell<HashMap<BastydeWindowId, Signal<u64>>>>,
}

impl NotificationArchiveModel {
    /// Construct from a [`NotificationArchive`] config. For
    /// `Persistent` mode, resolves the path through `AppPaths`.
    /// Tests use `AppPaths::for_testing(tmpdir)` + `Duration::ZERO`
    /// debounce.
    pub fn open(
        archive: &NotificationArchive,
        paths: &AppPaths,
        debounce: Duration,
    ) -> Result<Self, NotificationArchiveError> {
        let limit = archive.limit();
        let backend = match archive {
            NotificationArchive::InMemory { .. } => ArchiveBackend::InMemory(ListModel::new()),
            NotificationArchive::Persistent { file_name, .. } => {
                let path: PathBuf = paths.config_file(file_name);
                let plm: PersistedListModel<NotificationEntry> =
                    PersistedListModel::open(path, debounce, Migrator::new())?;
                ArchiveBackend::Persistent(plm)
            }
        };
        // Initialize next_id past the largest existing id so persistent
        // archives don't collide ids across restarts.
        let model = backend.model();
        let next_id_seed = (0..model.len())
            .filter_map(|i| model.with_item(i, |e| e.id))
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);
        // Initial unread_count reflects what's on disk.
        let initial_unread = (0..model.len())
            .filter_map(|i| model.with_item(i, |e| !e.read))
            .filter(|x| *x)
            .count();
        Ok(Self {
            backend,
            limit,
            next_id: Cell::new(next_id_seed),
            unread_count: Signal::new(initial_unread),
            version: Signal::new(0),
            window_versions: Rc::new(RefCell::new(HashMap::new())),
        })
    }

    /// Convenience: construct an [`NotificationArchive::InMemory`]
    /// archive with the default cap, without going through paths.
    /// Mostly useful for tests and apps that explicitly want no
    /// persistence.
    pub fn in_memory() -> Self {
        Self {
            backend: ArchiveBackend::InMemory(ListModel::new()),
            limit: DEFAULT_ARCHIVE_LIMIT,
            next_id: Cell::new(1),
            unread_count: Signal::new(0),
            version: Signal::new(0),
            window_versions: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Reactive handle on the entries. Bind to a `ListView` /
    /// `Repeater` for live UI.
    pub fn entries(&self) -> &ListModel<NotificationEntry> {
        self.backend.model()
    }

    /// Signal of the unread count. Drives the bell-button badge.
    pub fn unread_count(&self) -> &Signal<usize> {
        &self.unread_count
    }

    /// Reactive handle on the archive's mutation version. Widgets
    /// that render the archive (e.g. `NotificationLog`) bind to
    /// this at `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> &Signal<u64> {
        &self.version
    }

    /// Get-or-create the rebuild signal for `window_id` — the
    /// window-scoped counterpart of [`Self::version_signal`]. A real
    /// `NotificationCenterButton` / `NotificationLog` attached to a
    /// window should bind to this (via [`Self::rebuild_signal_for`])
    /// instead of the shared `version_signal`, for the same reason
    /// [`ToastRegistry::window_version_signal`](crate::toast::ToastRegistry::window_version_signal)
    /// exists: sharing one `Signal` across independently-reconciled
    /// windows means only the first window to reconcile after a
    /// mutation ever sees it dirty.
    pub(crate) fn window_version_signal(&self, window_id: BastydeWindowId) -> Signal<u64> {
        self.window_versions
            .borrow_mut()
            .entry(window_id)
            .or_insert_with(|| Signal::new(0))
            .clone()
    }

    /// The rebuild signal a window-attached widget should bind to:
    /// window-specific when a real window id is known, the legacy
    /// shared signal otherwise (headless unit-test trees, which never
    /// have more than one such consumer).
    pub(crate) fn rebuild_signal_for(&self, window_id: Option<BastydeWindowId>) -> Signal<u64> {
        match window_id {
            Some(w) => self.window_version_signal(w),
            None => self.version.clone(),
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Bump the legacy shared signal AND every per-window signal — see
    /// [`Self::window_version_signal`] for why a single shared bump
    /// alone cannot reliably reach more than one window's bell/log.
    /// Collects the per-window signals into a `Vec` before calling
    /// `set` on any of them, so a rebuild triggered synchronously by
    /// one of these `set` calls can't panic on a re-entrant `RefCell`
    /// borrow if it happens to allocate a new window's entry in this
    /// same map mid-iteration.
    fn bump_version(&self) {
        let v = self.version.get();
        self.version.set(v.wrapping_add(1));
        let per_window: Vec<Signal<u64>> = self.window_versions.borrow().values().cloned().collect();
        for sig in per_window {
            let v = sig.get();
            sig.set(v.wrapping_add(1));
        }
    }

    /// Force the persistent backing file to disk synchronously.
    /// No-op for `InMemory`. Tests call this between mutations and
    /// re-opening the file to verify persistence.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.backend.flush_now()
    }

    /// Push a new entry. Inserts at index 0 (newest first), evicts
    /// the oldest if the resulting length exceeds `limit`. Stamps
    /// the entry's `id` field from `next_id`. Bumps `unread_count`
    /// when the entry is unread (which is the typical case from a
    /// toast push).
    ///
    /// If `entry.dedup_id` matches an existing entry, the existing
    /// entry is updated in place (title / body / progress collapsed
    /// into a `NotificationUpdate` appended to `updates`) and no
    /// new row is inserted. Unread count increments either way (an
    /// in-place update IS new information for the user).
    pub fn push(&self, mut entry: NotificationEntry) {
        self.bump_version();
        let model = self.backend.model();

        // Update-in-place merge: scan for a matching `dedup_id`.
        if let Some(ref new_dedup) = entry.dedup_id {
            let merge_idx = (0..model.len()).find(|&i| {
                model
                    .with_item(i, |e| e.dedup_id.as_deref() == Some(new_dedup.as_str()))
                    .unwrap_or(false)
            });
            if let Some(idx) = merge_idx {
                // Read the existing entry, append an update, and write
                // it back **in place** (same id, no reordering) — this
                // is exactly `PersistedListModel::update_in_place`'s
                // contract, and matches it identically for the
                // in-memory backend too. We preserve the original `id`
                // + `timestamp` and append a `NotificationUpdate`
                // describing the mutation.
                if let Some(mut existing) = model.with_item(idx, |e| e.clone()) {
                    let now = entry.timestamp;
                    let title_changed = existing.title != entry.title;
                    let body_changed = existing.body != entry.body;
                    existing
                        .updates
                        .push(crate::notification::NotificationUpdate {
                            timestamp: now,
                            title: if title_changed {
                                Some(entry.title.clone())
                            } else {
                                None
                            },
                            body: if body_changed {
                                entry.body.clone()
                            } else {
                                None
                            },
                            progress: None,
                        });
                    existing.title = entry.title;
                    existing.body = entry.body;
                    existing.read = false;
                    self.backend.update_in_place(existing);
                    self.bump_unread();
                    return;
                }
            }
        }

        // New entry: stamp the id (always fresh, so `upsert_front` is
        // a plain prepend — it never collides with an existing key),
        // then evict overflow.
        let next = self.next_id.get();
        entry.id = next;
        self.next_id.set(next.wrapping_add(1));
        let is_unread = !entry.read;
        self.backend.upsert_front(entry);
        if model.len() > self.limit {
            // Evict the oldest entry. The model has no `pop_back`;
            // remove-by-id of the last row is the equivalent.
            let last = model.len() - 1;
            if let Some(evicted) = model.with_item(last, |e| e.clone()) {
                // If the evicted entry was unread, decrement the
                // unread count so the badge doesn't lie about how
                // many sit on disk.
                if !evicted.read {
                    let n = self.unread_count.get();
                    self.unread_count.set(n.saturating_sub(1));
                }
                self.backend.remove(evicted.id);
            }
        }
        if is_unread {
            self.bump_unread();
        }
    }

    fn bump_unread(&self) {
        let n = self.unread_count.get();
        self.unread_count.set(n.saturating_add(1));
    }

    /// Mark every UNREAD entry matching `predicate` as read,
    /// decrementing `unread_count` by exactly how many were flipped.
    /// This is the scoped counterpart of [`mark_all_read`](Self::mark_all_read):
    /// a bell scoped to one window/audience must only mark ITS
    /// entries read on close — calling the unscoped `mark_all_read`
    /// from a scoped bell would incorrectly clear every OTHER
    /// window's/audience's unread state too.
    pub fn mark_read_where(&self, mut predicate: impl FnMut(&NotificationEntry) -> bool) {
        let model = self.backend.model();
        let ids: Vec<u64> = (0..model.len())
            .filter_map(|i| {
                model
                    .with_item(i, |e| (!e.read && predicate(e)).then_some(e.id))
                    .flatten()
            })
            .collect();
        if ids.is_empty() {
            return;
        }
        let mut mutated = false;
        for id in ids {
            if let Some((_, mut entry)) = self.backend.find_by_id(id) {
                entry.read = true;
                self.backend.update_in_place(entry);
                mutated = true;
                let n = self.unread_count.get();
                self.unread_count.set(n.saturating_sub(1));
            }
        }
        if mutated {
            self.bump_version();
        }
    }

    /// Mark every archived entry as read; reset `unread_count` to 0.
    /// Called by `NotificationCenterButton` when its popover opens.
    pub fn mark_all_read(&self) {
        let model = self.backend.model();
        // Collect the ids to flip first: mutating the persisted
        // backend's live model mid-scan (`update_in_place` writes
        // straight into `model`, same length, no reordering) is safe
        // for this loop either way, but reading into an owned `Vec`
        // up front keeps the read and the mutation cleanly separated.
        let unread_ids: Vec<u64> = (0..model.len())
            .filter_map(|i| model.with_item(i, |e| (!e.read).then_some(e.id)).flatten())
            .collect();
        let mut mutated = false;
        for id in unread_ids {
            if let Some((_, mut entry)) = self.backend.find_by_id(id) {
                entry.read = true;
                self.backend.update_in_place(entry);
                mutated = true;
            }
        }
        self.unread_count.set(0);
        if mutated {
            self.bump_version();
        }
    }

    /// Clear the entire archive (resets `unread_count` to 0).
    pub fn clear(&self) {
        let was_empty = self.backend.model().is_empty();
        self.backend.clear();
        self.unread_count.set(0);
        if !was_empty {
            self.bump_version();
        }
    }

    /// Remove every entry matching `predicate`, decrementing
    /// `unread_count` for each removed entry that was unread. The
    /// scoped counterpart of [`clear`](Self::clear): a bell scoped to
    /// one window/audience must only clear ITS entries — the unscoped
    /// `clear()` wipes the ENTIRE shared archive (every window's
    /// history), which would be wrong for a scoped "Clear" button.
    pub fn clear_where(&self, mut predicate: impl FnMut(&NotificationEntry) -> bool) {
        let model = self.backend.model();
        let matches: Vec<(u64, bool)> = (0..model.len())
            .filter_map(|i| {
                model
                    .with_item(i, |e| predicate(e).then_some((e.id, !e.read)))
                    .flatten()
            })
            .collect();
        if matches.is_empty() {
            return;
        }
        let mut removed_any = false;
        for (id, was_unread) in matches {
            if self.backend.remove(id) {
                removed_any = true;
                if was_unread {
                    let n = self.unread_count.get();
                    self.unread_count.set(n.saturating_sub(1));
                }
            }
        }
        if removed_any {
            self.bump_version();
        }
    }

    /// Remove the entry with the given **stable** id (see
    /// [`NotificationEntry::id`] — "assigned by the archive on first
    /// push; never reused"). Updates `unread_count` if the removed entry
    /// was unread. No-op (no version bump) when no entry has that id.
    ///
    /// Deliberately id-based rather than index-based: an index is a
    /// snapshot of the list's shape at the moment it was read, and is
    /// meaningless once anything else — a concurrent peer-process reload
    /// merged in via the live archive, another `push`, another `remove` —
    /// has shifted rows out from under it. A caller that captured "the row
    /// I want to dismiss" as an index earlier and replays it later against
    /// a since-mutated list can silently remove the *wrong* entry; keying
    /// off `id` instead re-resolves the row's current position at the
    /// moment of removal, so it always removes the entry the caller meant.
    pub fn remove_by_id(&self, id: u64) {
        let Some((_, entry)) = self.backend.find_by_id(id) else {
            return;
        };
        let was_unread = !entry.read;
        self.backend.remove(id);
        if was_unread {
            let n = self.unread_count.get();
            self.unread_count.set(n.saturating_sub(1));
        }
        self.bump_version();
    }
}

impl std::fmt::Debug for NotificationArchiveModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationArchiveModel")
            .field("entries", &self.entries().len())
            .field("limit", &self.limit)
            .field("unread_count", &self.unread_count.get())
            .field(
                "backend",
                &match &self.backend {
                    ArchiveBackend::InMemory(_) => "InMemory",
                    ArchiveBackend::Persistent(_) => "Persistent",
                },
            )
            .finish()
    }
}

// `NotificationArchiveModel` deliberately does NOT implement `Clone`.
// The persistent backend's `PersistedListModel` observer captures a
// strong reference to the inner `SettingsFile`; a naive Clone would
// create divergent writers writing the same file. Apps share a
// single archive through an `Rc<NotificationArchiveModel>` (or
// `Rc<RefCell<…>>` if mutation through shared handles is needed) —
// the install helper in `bastyde` puts the model in `app_state` as
// `Rc<NotificationArchiveModel>`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::ArchivedActionStyle;
    use crate::toast::ToastRoute;
    use bastyde_core::styles::{BannerSeverity, ToastPriority};

    fn entry(title: &str) -> NotificationEntry {
        NotificationEntry {
            id: 0, // overwritten by push()
            severity: BannerSeverity::Info,
            priority: ToastPriority::Normal,
            title: title.to_string(),
            body: None,
            actions: Vec::new(),
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            group: None,
            source: None,
            read: false,
            dedup_id: None,
            updates: Vec::new(),
            route: ToastRoute::Broadcast,
        }
    }

    #[test]
    fn in_memory_starts_empty() {
        let m = NotificationArchiveModel::in_memory();
        assert_eq!(m.entries().len(), 0);
        assert_eq!(m.unread_count().get(), 0);
        assert_eq!(m.limit(), DEFAULT_ARCHIVE_LIMIT);
    }

    #[test]
    fn push_inserts_newest_first_and_bumps_unread() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("first"));
        m.push(entry("second"));
        m.push(entry("third"));
        assert_eq!(m.entries().len(), 3);
        assert_eq!(m.unread_count().get(), 3);
        // Newest at index 0.
        assert_eq!(
            m.entries().with_item(0, |e| e.title.clone()),
            Some("third".to_string())
        );
        assert_eq!(
            m.entries().with_item(2, |e| e.title.clone()),
            Some("first".to_string())
        );
    }

    #[test]
    fn push_stamps_distinct_increasing_ids() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.push(entry("b"));
        m.push(entry("c"));
        let id0 = m.entries().with_item(0, |e| e.id).unwrap();
        let id1 = m.entries().with_item(1, |e| e.id).unwrap();
        let id2 = m.entries().with_item(2, |e| e.id).unwrap();
        // Newest = index 0 has the highest id.
        assert!(id0 > id1);
        assert!(id1 > id2);
    }

    #[test]
    fn bounded_eviction_drops_oldest() {
        let m = NotificationArchiveModel {
            backend: ArchiveBackend::InMemory(ListModel::new()),
            limit: 3,
            next_id: Cell::new(1),
            unread_count: Signal::new(0),
            version: Signal::new(0),
            window_versions: Rc::new(RefCell::new(HashMap::new())),
        };
        for i in 0..5 {
            m.push(entry(&format!("t{i}")));
        }
        assert_eq!(m.entries().len(), 3, "bounded to limit");
        assert_eq!(
            m.unread_count().get(),
            3,
            "unread count tracks live entries"
        );
        // Newest preserved (t4, t3, t2).
        assert_eq!(
            m.entries().with_item(0, |e| e.title.clone()),
            Some("t4".into())
        );
        assert_eq!(
            m.entries().with_item(1, |e| e.title.clone()),
            Some("t3".into())
        );
        assert_eq!(
            m.entries().with_item(2, |e| e.title.clone()),
            Some("t2".into())
        );
    }

    #[test]
    fn mark_all_read_zeros_count_and_flips_entries() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.push(entry("b"));
        assert_eq!(m.unread_count().get(), 2);

        m.mark_all_read();
        assert_eq!(m.unread_count().get(), 0);
        assert!(m.entries().with_item(0, |e| e.read).unwrap());
        assert!(m.entries().with_item(1, |e| e.read).unwrap());
    }

    #[test]
    fn clear_empties_and_zeros_count() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.push(entry("b"));
        m.clear();
        assert_eq!(m.entries().len(), 0);
        assert_eq!(m.unread_count().get(), 0);
    }

    #[test]
    fn remove_by_id_unread_decrements_count() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.push(entry("b"));
        assert_eq!(m.unread_count().get(), 2);

        let b_id = m.entries().with_item(0, |e| e.id).unwrap(); // "b" is newest
        m.remove_by_id(b_id);
        assert_eq!(m.entries().len(), 1);
        assert_eq!(m.unread_count().get(), 1);
        assert_eq!(
            m.entries().with_item(0, |e| e.title.clone()),
            Some("a".to_string())
        );
    }

    #[test]
    fn remove_by_id_read_does_not_change_count() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.mark_all_read();
        assert_eq!(m.unread_count().get(), 0);
        let a_id = m.entries().with_item(0, |e| e.id).unwrap();
        m.remove_by_id(a_id);
        assert_eq!(m.unread_count().get(), 0);
        assert!(m.entries().is_empty());
    }

    #[test]
    fn remove_by_id_unknown_id_is_a_noop() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        let v_before = m.version_signal().get();
        m.remove_by_id(999_999);
        assert_eq!(m.entries().len(), 1, "nothing removed");
        assert_eq!(
            v_before,
            m.version_signal().get(),
            "no version bump for a no-op"
        );
    }

    #[test]
    fn remove_by_id_removes_the_right_entry_after_a_concurrent_insert_shifts_indices() {
        // Bug repro for the index-based API this replaces: a caller reads
        // "the row to dismiss" as an index, but before it acts, a
        // concurrent insert (a peer process's reload merged into the live
        // archive, or just another `push`) shifts every row after it down
        // by one. An index-based `remove(stale_index)` would then delete
        // whatever row happens to occupy that index NOW — not the one the
        // caller meant. `remove_by_id` re-resolves the row's position at
        // the moment of removal, so it is immune to this.
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a")); // index 1 after "b" below
        m.push(entry("b")); // index 0
        assert_eq!(
            m.entries().with_item(1, |e| e.title.clone()),
            Some("a".to_string()),
            "precondition: a is at index 1"
        );
        // Caller observes "a" at index 1 and remembers its id to dismiss
        // it later.
        let a_id = m
            .entries()
            .with_item(1, |e| e.id)
            .expect("a's id at index 1");

        // Concurrent insert (simulating a peer's write landing directly in
        // the live model) shifts "a" from index 1 to index 2.
        m.entries().insert(0, entry("peer-inserted"));
        assert_eq!(
            m.entries().with_item(2, |e| e.title.clone()),
            Some("a".to_string()),
            "precondition: the insert shifted a to index 2"
        );

        // A stale `remove(1)` would now delete "b", not "a". `remove_by_id`
        // must remove "a" regardless of where it ended up.
        m.remove_by_id(a_id);

        assert_eq!(m.entries().len(), 2, "exactly one entry removed");
        let remaining: Vec<String> = (0..m.entries().len())
            .map(|i| m.entries().with_item(i, |e| e.title.clone()).unwrap())
            .collect();
        assert!(
            remaining.contains(&"b".to_string()),
            "b survives: {remaining:?}"
        );
        assert!(
            remaining.contains(&"peer-inserted".to_string()),
            "peer-inserted survives: {remaining:?}"
        );
        assert!(
            !remaining.contains(&"a".to_string()),
            "a — the one actually targeted by id — is gone: {remaining:?}"
        );
    }

    #[test]
    fn update_in_place_merges_by_dedup_id() {
        let m = NotificationArchiveModel::in_memory();
        let mut first = entry("Uploading 1 of 7");
        first.dedup_id = Some("upload".to_string());
        m.push(first);
        assert_eq!(m.entries().len(), 1);
        assert_eq!(m.unread_count().get(), 1);

        // Mark read so the update bumps unread back up.
        m.mark_all_read();
        assert_eq!(m.unread_count().get(), 0);

        let mut second = entry("Uploading 4 of 7");
        second.dedup_id = Some("upload".to_string());
        m.push(second);
        assert_eq!(m.entries().len(), 1, "update merges into existing row");
        assert_eq!(
            m.unread_count().get(),
            1,
            "in-place update is also new info"
        );
        let merged = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(merged.title, "Uploading 4 of 7");
        assert_eq!(merged.updates.len(), 1);
        assert_eq!(merged.updates[0].title.as_deref(), Some("Uploading 4 of 7"));
        assert!(!merged.read, "in-place update resets read state");
    }

    #[test]
    fn update_in_place_only_merges_on_dedup_match() {
        let m = NotificationArchiveModel::in_memory();
        let mut a = entry("first");
        a.dedup_id = Some("x".to_string());
        m.push(a);
        let mut b = entry("second");
        b.dedup_id = Some("y".to_string());
        m.push(b);
        // Different dedup_ids — both rows alive.
        assert_eq!(m.entries().len(), 2);
        // Third entry with NO dedup_id never merges.
        m.push(entry("third"));
        assert_eq!(m.entries().len(), 3);
    }

    #[test]
    fn persistent_round_trip() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("notifications_test");

        // First open: push two entries, flush.
        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(entry("first"));
            m.push(entry("second"));
            m.flush_now().unwrap();
            assert_eq!(m.entries().len(), 2);
        }

        // Re-open: same entries, ids stamped previously survive.
        let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(m.entries().len(), 2);
        // Newest first.
        assert_eq!(
            m.entries().with_item(0, |e| e.title.clone()),
            Some("second".into())
        );
        // unread_count reseeded from the file (entries had read=false).
        assert_eq!(m.unread_count().get(), 2);
        // Next push gets id past the highest persisted id.
        m.push(entry("third"));
        let third_id = m.entries().with_item(0, |e| e.id).unwrap();
        let second_id = m.entries().with_item(1, |e| e.id).unwrap();
        assert!(
            third_id > second_id,
            "ids continue increasing across restarts (third {third_id} > second {second_id})"
        );
    }

    /// Bug-repro for the raw-`ListModel`-mutation hazard flagged when
    /// `PersistedListModel::model()` became read/reactive-binding-only:
    /// every mutating `NotificationArchiveModel` method must persist
    /// through the backend's `upsert_front` / `update_in_place` /
    /// `remove` / `clear`, never by mutating `backend.model()`
    /// directly (which would update the live in-memory `ListModel` but
    /// silently never reach disk). Exercises each one and reopens a
    /// fresh handle over the same file to prove the effect actually
    /// landed, not just that the live model looks right.
    #[test]
    fn mark_all_read_persists_across_reopen() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("mark_read_test");

        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(entry("a"));
            m.push(entry("b"));
            m.mark_all_read();
            m.flush_now().unwrap();
        }

        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(
            reopened.unread_count().get(),
            0,
            "read state must have been persisted, not just live-mutated"
        );
        assert!(reopened.entries().with_item(0, |e| e.read).unwrap());
        assert!(reopened.entries().with_item(1, |e| e.read).unwrap());
    }

    #[test]
    fn remove_by_id_persists_across_reopen() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("remove_test");

        let removed_title;
        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(entry("a"));
            m.push(entry("b"));
            let b_id = m.entries().with_item(0, |e| e.id).unwrap();
            removed_title = m.entries().with_item(0, |e| e.title.clone()).unwrap();
            m.remove_by_id(b_id);
            m.flush_now().unwrap();
            assert_eq!(m.entries().len(), 1);
        }

        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(
            reopened.entries().len(),
            1,
            "the removal must have reached disk, not just the live model"
        );
        assert_eq!(
            reopened.entries().with_item(0, |e| e.title.clone()),
            Some("a".to_string())
        );
        assert_ne!(
            reopened.entries().with_item(0, |e| e.title.clone()),
            Some(removed_title)
        );
    }

    #[test]
    fn dedup_merge_update_in_place_persists_across_reopen() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("dedup_test");

        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            let mut first = entry("Uploading 1 of 7");
            first.dedup_id = Some("upload".to_string());
            m.push(first);
            let mut second = entry("Uploading 4 of 7");
            second.dedup_id = Some("upload".to_string());
            m.push(second);
            m.flush_now().unwrap();
            assert_eq!(m.entries().len(), 1, "merged into one row");
        }

        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(reopened.entries().len(), 1, "still one row after reopen");
        let merged = reopened.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(
            merged.title, "Uploading 4 of 7",
            "the in-place update's title must have persisted, not the original"
        );
        assert_eq!(
            merged.updates.len(),
            1,
            "the appended NotificationUpdate must have persisted"
        );
    }

    #[test]
    fn clear_persists_across_reopen() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("clear_test");

        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(entry("a"));
            m.push(entry("b"));
            m.clear();
            m.flush_now().unwrap();
            assert_eq!(m.entries().len(), 0);
        }

        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(
            reopened.entries().len(),
            0,
            "the clear must have reached disk, not just the live model"
        );
    }

    #[test]
    fn bounded_eviction_persists_across_reopen() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent_with_limit("eviction_test", 2);

        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(entry("t0"));
            m.push(entry("t1"));
            m.push(entry("t2")); // evicts t0
            m.flush_now().unwrap();
            assert_eq!(m.entries().len(), 2);
        }

        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(
            reopened.entries().len(),
            2,
            "the eviction must have reached disk, not just the live model"
        );
        let titles: Vec<String> = (0..reopened.entries().len())
            .map(|i| {
                reopened
                    .entries()
                    .with_item(i, |e| e.title.clone())
                    .unwrap()
            })
            .collect();
        assert!(
            !titles.contains(&"t0".to_string()),
            "t0 was evicted: {titles:?}"
        );
        assert!(titles.contains(&"t1".to_string()));
        assert!(titles.contains(&"t2".to_string()));
    }

    #[test]
    fn version_signal_bumps_on_push_mark_clear_remove() {
        let m = NotificationArchiveModel::in_memory();
        let v0 = m.version_signal().get();
        m.push(entry("a"));
        let v1 = m.version_signal().get();
        assert_ne!(v0, v1, "push bumps version");

        m.push(entry("b"));
        m.mark_all_read();
        let v2 = m.version_signal().get();
        assert_ne!(v1, v2, "mark_all_read bumps version");

        let id0 = m.entries().with_item(0, |e| e.id).unwrap();
        m.remove_by_id(id0);
        let v3 = m.version_signal().get();
        assert_ne!(v2, v3, "remove bumps version");

        m.clear();
        let v4 = m.version_signal().get();
        assert_ne!(v3, v4, "clear bumps version");
    }

    #[test]
    fn version_signal_does_not_bump_for_noops() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        let v_before = m.version_signal().get();
        // mark_all_read on a fully-read archive — no mutation, no bump.
        m.mark_all_read();
        let v_after_mark1 = m.version_signal().get();
        m.mark_all_read();
        let v_after_mark2 = m.version_signal().get();
        assert_eq!(
            v_after_mark1, v_after_mark2,
            "second mark_all_read with nothing to flip is a no-op (no version bump)"
        );

        // clear on already-empty archive — no bump.
        m.clear();
        let v_after_clear1 = m.version_signal().get();
        m.clear();
        let v_after_clear2 = m.version_signal().get();
        assert_eq!(v_after_clear1, v_after_clear2, "clear on empty is a no-op");
        let _ = v_before;
    }

    #[test]
    fn mark_read_where_only_flips_matching_unread_entries() {
        use crate::toast::ToastAudience;
        let m = NotificationArchiveModel::in_memory();
        let mut a = entry("audience a");
        a.route = ToastRoute::Audience(ToastAudience::new(1));
        m.push(a);
        let mut b = entry("audience b");
        b.route = ToastRoute::Audience(ToastAudience::new(2));
        m.push(b);
        assert_eq!(m.unread_count().get(), 2);

        // Scoped mark-read for audience 1 only.
        m.mark_read_where(|e| e.route == ToastRoute::Audience(ToastAudience::new(1)));
        assert_eq!(
            m.unread_count().get(),
            1,
            "only audience 1's entry was marked read"
        );
        let a_read = m
            .entries()
            .with_item(1, |e| e.read)
            .expect("audience a is the oldest, at index 1");
        let b_read = m
            .entries()
            .with_item(0, |e| e.read)
            .expect("audience b is newest, at index 0");
        assert!(a_read, "audience a's entry is now read");
        assert!(!b_read, "audience b's entry is untouched");
    }

    #[test]
    fn clear_where_only_removes_matching_entries() {
        use crate::toast::ToastAudience;
        let m = NotificationArchiveModel::in_memory();
        let mut a = entry("audience a");
        a.route = ToastRoute::Audience(ToastAudience::new(1));
        m.push(a);
        let mut b = entry("audience b");
        b.route = ToastRoute::Audience(ToastAudience::new(2));
        m.push(b);
        assert_eq!(m.entries().len(), 2);
        assert_eq!(m.unread_count().get(), 2);

        m.clear_where(|e| e.route == ToastRoute::Audience(ToastAudience::new(1)));
        assert_eq!(m.entries().len(), 1, "only audience 1's entry is removed");
        assert_eq!(
            m.unread_count().get(),
            1,
            "unread_count decrements for the removed unread entry"
        );
        assert_eq!(
            m.entries().with_item(0, |e| e.title.clone()),
            Some("audience b".to_string()),
            "audience b's entry survives"
        );
    }

    #[test]
    fn entry_serde_round_trip() {
        // NotificationEntry must be round-trippable through TOML
        // (PersistedListModel's serialization format).
        let original = NotificationEntry {
            id: 42,
            severity: BannerSeverity::Warning,
            priority: ToastPriority::High,
            title: "Heads up".into(),
            body: Some("Details here".into()),
            actions: vec![crate::notification::ArchivedAction {
                label: "Open".into(),
                intent_name: Some("app.open".into()),
                style: ArchivedActionStyle::PrimaryButton,
                closes_on_invoke: true,
            }],
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            group: Some("build".into()),
            source: Some("build.success".into()),
            read: false,
            dedup_id: Some("build-1".into()),
            updates: vec![],
            route: ToastRoute::Audience(crate::toast::ToastAudience::new(7)),
        };
        // Wrap in a Vec because TOML doesn't allow a top-level
        // non-table value, and our ListFile is `{ version, items }`.
        let wrapper = bastyde_settings::ListFile {
            version: 1,
            items: vec![original.clone()],
        };
        let serialized = toml::to_string(&wrapper).expect("serialize");
        let parsed: bastyde_settings::ListFile<NotificationEntry> =
            toml::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0], original);
    }
}
