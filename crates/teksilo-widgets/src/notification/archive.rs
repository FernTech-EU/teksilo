// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `NotificationArchiveModel` — the persistent list backing
//! [`NotificationLog`](crate::notification::log::NotificationLog) and the bell-icon badge.
//!
//! Wraps a [`ListModel<NotificationEntry>`](teksilo_data::ListModel)
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
//!   gets re-serialized on the shared teksilo-settings I/O thread.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use teksilo_core::signal::Signal;
use teksilo_data::ListModel;
use teksilo_settings::{AppPaths, Migrator, PersistedListModel, SettingsFileError};

use crate::notification::NotificationEntry;

/// Default per-archive entry cap. IntelliJ's notification log keeps
/// hundreds of entries with no cap visible to the user; we pick a
/// pragmatic limit so persistent files don't grow unbounded.
pub const DEFAULT_ARCHIVE_LIMIT: usize = 200;

/// File-name (without extension) used for the persistent archive.
/// Resolved through [`AppPaths::config_file`] into
/// `<config_dir>/<app>/notifications.toml`.
pub const ARCHIVE_FILE_NAME: &str = "notifications";

/// How many [`NotificationUpdate`](crate::notification::NotificationUpdate)
/// records one row keeps. A progress notice that reports each step in its
/// wording records one update per step, and an id an app reuses for every
/// run of an operation gathers them from every run, so a row keeps the most
/// recent ones and drops the oldest. The row itself always shows the notice
/// as it now stands, whatever the history kept.
pub const UPDATE_HISTORY_LIMIT: usize = 20;

/// How an entry reaches [`NotificationArchiveModel`]: raised as a notice, or
/// as an in-place update of a notice still on screen. The two differ only
/// when the entry merges into an existing row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrival {
    Raised,
    LiveUpdate,
}

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
/// [`NotificationEntry`]'s [`Keyed`](teksilo_settings::Keyed) impl), so
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

    /// Insert `entry` at the front, first removing the row that has the
    /// same `id` if there is one, so a row raised again moves to the top
    /// rather than appearing twice. A freshly stamped id matches no row, and
    /// this is then a plain prepend.
    fn upsert_front(&self, entry: NotificationEntry) {
        match self {
            Self::InMemory(m) => {
                if let Some((idx, _)) = self.find_by_id(entry.id) {
                    m.remove(idx);
                }
                m.insert(0, entry);
            }
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
/// via `ctx.app_state::<Rc<NotificationArchiveModel>>()`.
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
    /// [`OverlayManager::version`](teksilo_core::overlay::OverlayManager::version)
    /// and [`ToastRegistry::version_signal`](crate::toast::ToastRegistry::version_signal).
    version: Signal<u64>,
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
    /// that render the archive (`NotificationLog`,
    /// `NotificationCenterButton`) bind to this at
    /// `BindingLevel::Rebuild`, in every window — one signal is enough
    /// for N of them, see
    /// [`ToastRegistry::version_signal`](crate::toast::ToastRegistry::version_signal)
    /// for the history of why that had to be said out loud.
    pub fn version_signal(&self) -> &Signal<u64> {
        &self.version
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Bump the version every bell / log binds at
    /// `BindingLevel::Rebuild` — see [`Self::version_signal`]. One
    /// write reaches every window: each window's own `BindingRegistry`
    /// tracks the generation it last reconciled, so none of them can
    /// consume the notification out from under the others.
    fn bump_version(&self) {
        let v = self.version.get();
        self.version.set(v.wrapping_add(1));
    }

    /// Force the persistent backing file to disk synchronously.
    /// No-op for `InMemory`. Tests call this between mutations and
    /// re-opening the file to verify persistence.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.backend.flush_now()
    }

    /// Push a notice that has just been raised. Inserts at index 0
    /// (newest first), evicts the oldest if the resulting length exceeds
    /// `limit`. Stamps the entry's `id` field from `next_id`. Bumps
    /// `unread_count` when the entry is unread (which is the typical case
    /// from a toast push).
    ///
    /// If `entry.dedup_id` matches an existing row, the notice is that row
    /// raised again: an operation that failed this morning and fails again
    /// now, under the same [`Toast::id`](crate::toast::Toast::id). No new row
    /// is inserted. The row moves to the front, takes the entry's
    /// `timestamp`, records a
    /// [`NotificationUpdate`](crate::notification::NotificationUpdate) and
    /// takes the entry's read state, which makes it unread again, even when
    /// the notice says exactly what it said before: the log and the bell show
    /// that it happened again. Every other field is merged as
    /// [`push_update`](Self::push_update) describes.
    pub fn push(&self, entry: NotificationEntry) {
        self.arrive(entry, Arrival::Raised);
    }

    /// Push an in-place update of a notice that is still on screen: a
    /// progress notice's next step, or its result. This is what the toast
    /// registry calls when a `Toast::id` matches a live toast.
    ///
    /// If `entry.dedup_id` matches an existing row, that row is updated in
    /// place: same position, and it keeps the time the notice was raised.
    /// The row then shows the notice **as it now stands**, the way the live
    /// toast it mirrors does:
    ///
    /// - `title`, `body`, `severity`, `priority`, `actions` and `route`
    ///   are the update's. An update offering no actions leaves the row
    ///   offering none: a progress notice's Cancel must not outlive the
    ///   notice that said the work was cancelled, nor stand in for the
    ///   Open and See report a finished one offers.
    /// - `group` and `source` are the update's when it names them, and
    ///   stay as they were when it does not.
    /// - `id`, `timestamp` (when the notice was raised, which places the row
    ///   in the log) and `dedup_id` stay.
    /// - A [`NotificationUpdate`](crate::notification::NotificationUpdate)
    ///   is appended when the update changes what the row shows (its
    ///   wording, severity or actions), carrying the update's timestamp and
    ///   whichever of title and body changed. A row keeps the most recent
    ///   [`UPDATE_HISTORY_LIMIT`] of them. An update that repeats the row
    ///   exactly records nothing and leaves its read state alone.
    /// - A change gives the row the update's read state, which for a toast
    ///   is unread. `unread_count` counts rows, so it grows only when the
    ///   row had been read.
    ///
    /// With no matching row (the user cleared it, or it was evicted), the
    /// update is pushed as a new row, like [`push`](Self::push).
    pub fn push_update(&self, entry: NotificationEntry) {
        self.arrive(entry, Arrival::LiveUpdate);
    }

    fn arrive(&self, mut entry: NotificationEntry, arrival: Arrival) {
        self.bump_version();
        let model = self.backend.model();

        // Merge: scan for a matching `dedup_id`.
        if let Some(ref new_dedup) = entry.dedup_id {
            let merge_idx = (0..model.len()).find(|&i| {
                model
                    .with_item(i, |e| e.dedup_id.as_deref() == Some(new_dedup.as_str()))
                    .unwrap_or(false)
            });
            if let Some(idx) = merge_idx
                && let Some(existing) = model.with_item(idx, |e| e.clone())
            {
                self.merge(existing, entry, arrival);
                return;
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

    /// Bring `existing` up to `entry` and write it back: in place for a live
    /// update, at the front for a notice raised again. See
    /// [`push`](Self::push) and [`push_update`](Self::push_update) for what
    /// each field becomes.
    fn merge(&self, mut existing: NotificationEntry, entry: NotificationEntry, arrival: Arrival) {
        let title_changed = existing.title != entry.title;
        let body_changed = existing.body != entry.body;
        let changed = title_changed
            || body_changed
            || existing.severity != entry.severity
            || existing.actions != entry.actions;
        let raised = arrival == Arrival::Raised;
        let recorded = changed || raised;
        if recorded {
            existing
                .updates
                .push(crate::notification::NotificationUpdate {
                    timestamp: entry.timestamp,
                    title: title_changed.then(|| entry.title.clone()),
                    body: if body_changed {
                        entry.body.clone()
                    } else {
                        None
                    },
                    progress: None,
                });
            let excess = existing.updates.len().saturating_sub(UPDATE_HISTORY_LIMIT);
            existing.updates.drain(..excess);
        }
        let was_unread = !existing.read;
        existing.title = entry.title;
        existing.body = entry.body;
        existing.severity = entry.severity;
        existing.priority = entry.priority;
        existing.actions = entry.actions;
        existing.route = entry.route;
        if entry.group.is_some() {
            existing.group = entry.group;
        }
        if entry.source.is_some() {
            existing.source = entry.source;
        }
        if recorded {
            existing.read = entry.read;
        }
        let is_unread = !existing.read;
        if raised {
            existing.timestamp = entry.timestamp;
            self.backend.upsert_front(existing);
        } else {
            self.backend.update_in_place(existing);
        }
        match (was_unread, is_unread) {
            (false, true) => self.bump_unread(),
            (true, false) => {
                let n = self.unread_count.get();
                self.unread_count.set(n.saturating_sub(1));
            }
            _ => {}
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
    /// Called by `NotificationCenterButton` when its popover closes.
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
// the install helper in `teksilo` puts the model in `app_state` as
// `Rc<NotificationArchiveModel>`.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::{ArchivedAction, ArchivedActionStyle};
    use crate::toast::ToastRoute;
    use teksilo_core::styles::{BannerSeverity, ToastPriority};

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
        m.push_update(second);
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

    fn action(label: &str, intent: Option<&str>, style: ArchivedActionStyle) -> ArchivedAction {
        ArchivedAction {
            label: label.to_string(),
            intent_name: intent.map(str::to_string),
            style,
            closes_on_invoke: true,
        }
    }

    fn notice(
        title: &str,
        severity: BannerSeverity,
        actions: Vec<ArchivedAction>,
    ) -> NotificationEntry {
        let mut e = entry(title);
        e.dedup_id = Some("import".to_string());
        e.severity = severity;
        e.actions = actions;
        e
    }

    /// A notice updated in place is archived as it now stands: the latest
    /// update's actions, severity, priority and audience, not the first
    /// notice's. A progress notice offering Cancel that became a result
    /// offering Open now and See report used to stay a Cancel row in the
    /// log, which could then never open the report.
    #[test]
    fn a_merged_row_carries_the_latest_notice() {
        let m = NotificationArchiveModel::in_memory();
        let mut progress = notice(
            "Importing",
            BannerSeverity::Info,
            vec![action("Cancel", None, ArchivedActionStyle::Destructive)],
        );
        progress.priority = ToastPriority::Normal;
        m.push(progress);
        let first = m.entries().with_item(0, |e| e.clone()).unwrap();

        let mut result = notice(
            "Imported",
            BannerSeverity::Success,
            vec![
                action("Open now", None, ArchivedActionStyle::PrimaryButton),
                action(
                    "See report",
                    Some("app.import.report"),
                    ArchivedActionStyle::SecondaryButton,
                ),
            ],
        );
        result.priority = ToastPriority::High;
        result.route = ToastRoute::Audience(crate::toast::ToastAudience::new(3));
        result.timestamp = jiff::Timestamp::from_second(60).unwrap();
        m.push_update(result.clone());

        assert_eq!(m.entries().len(), 1, "an update stays one row");
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.actions, result.actions, "the result's actions");
        assert_eq!(row.severity, BannerSeverity::Success);
        assert_eq!(row.priority, ToastPriority::High);
        assert_eq!(row.route, result.route, "the audience it was retargeted to");
        assert_eq!(row.title, "Imported");
        // What identifies the row and places it in the log does not move.
        assert_eq!(row.id, first.id);
        assert_eq!(row.timestamp, first.timestamp, "first raised at");
        assert_eq!(
            row.updates.last().map(|u| u.timestamp),
            Some(result.timestamp),
            "the update records when it came"
        );
    }

    /// An update that offers no actions leaves the row offering none, as the
    /// live notice it mirrors does: a Cancel kept from a notice that has since
    /// said the import was cancelled would offer to stop something already
    /// stopped.
    #[test]
    fn an_update_without_actions_leaves_the_row_without_them() {
        let m = NotificationArchiveModel::in_memory();
        m.push(notice(
            "Importing",
            BannerSeverity::Info,
            vec![action("Cancel", None, ArchivedActionStyle::Destructive)],
        ));
        m.push_update(notice("Import cancelled", BannerSeverity::Info, Vec::new()));
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert!(row.actions.is_empty(), "kept {:?}", row.actions);
    }

    /// An update names its group and source only when it has one: a row
    /// stays where it was filed rather than being moved out of it by an
    /// update that does not say.
    #[test]
    fn an_update_keeps_the_group_and_source_it_does_not_name() {
        let m = NotificationArchiveModel::in_memory();
        let mut first = notice("Importing", BannerSeverity::Info, Vec::new());
        first.group = Some("imports".to_string());
        first.source = Some("scrivener".to_string());
        m.push(first);
        m.push_update(notice("Imported", BannerSeverity::Success, Vec::new()));
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.group.as_deref(), Some("imports"));
        assert_eq!(row.source.as_deref(), Some("scrivener"));

        let mut moved = notice("Imported", BannerSeverity::Success, Vec::new());
        moved.group = Some("done".to_string());
        m.push_update(moved);
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(
            row.group.as_deref(),
            Some("done"),
            "one that does, moves it"
        );
    }

    /// The unread count counts rows. Updating a row nobody has read yet adds
    /// nothing to it: a progress notice ticked fifty times is one unread row,
    /// and marking it read must bring the badge back to zero.
    #[test]
    fn updating_an_unread_row_counts_it_once() {
        let m = NotificationArchiveModel::in_memory();
        m.push(notice("Importing 1%", BannerSeverity::Info, Vec::new()));
        for percent in 2..=50 {
            m.push_update(notice(
                &format!("Importing {percent}%"),
                BannerSeverity::Info,
                Vec::new(),
            ));
        }
        assert_eq!(m.unread_count().get(), 1, "one unread row");
        m.mark_read_where(|_| true);
        assert_eq!(m.unread_count().get(), 0, "read, the badge is clear");

        // Read, then updated: unread again, once.
        m.push_update(notice("Imported", BannerSeverity::Success, Vec::new()));
        assert_eq!(m.unread_count().get(), 1);
    }

    /// An update that changes nothing the row shows is not recorded: a
    /// progress notice re-raised with the same words on every tick would
    /// otherwise grow its row, and the archive file, by one record a tick.
    #[test]
    fn a_repeated_notice_records_no_update() {
        let m = NotificationArchiveModel::in_memory();
        let cancel = vec![action("Cancel", None, ArchivedActionStyle::Destructive)];
        m.push(notice("Importing", BannerSeverity::Info, cancel.clone()));
        for _ in 0..20 {
            m.push_update(notice("Importing", BannerSeverity::Info, cancel.clone()));
        }
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert!(row.updates.is_empty(), "{} records", row.updates.len());

        // A change of severity alone is a change, recorded without wording.
        m.push_update(notice("Importing", BannerSeverity::Warning, cancel));
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.updates.len(), 1);
        assert_eq!(row.updates[0].title, None);
        assert_eq!(row.updates[0].body, None);
    }

    /// The same notice raised again after the first had left the screen is a
    /// second occurrence: the row comes back to the top, unread, dated when
    /// it came back, with a record of it, even though nothing it says has
    /// changed. Treated as a live tick, it stayed read, under its first date,
    /// below every notice raised since.
    #[test]
    fn a_notice_raised_again_comes_back_unread_at_the_top() {
        let m = NotificationArchiveModel::in_memory();
        let mut first = notice("Sync failed", BannerSeverity::Error, Vec::new());
        first.timestamp = jiff::Timestamp::from_second(9 * 3600).unwrap();
        m.push(first);
        let mut later = entry("Saved");
        later.timestamp = jiff::Timestamp::from_second(10 * 3600).unwrap();
        m.push(later);
        m.mark_all_read();
        let before = m.entries().with_item(1, |e| e.clone()).unwrap();

        let mut again = notice("Sync failed", BannerSeverity::Error, Vec::new());
        again.timestamp = jiff::Timestamp::from_second(15 * 3600).unwrap();
        m.push(again.clone());

        assert_eq!(m.entries().len(), 2, "still one row for the id");
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.id, before.id, "the same row, at the top");
        assert!(!row.read, "unread again");
        assert_eq!(m.unread_count().get(), 1);
        assert_eq!(row.timestamp, again.timestamp, "dated when it came back");
        assert_eq!(row.updates.len(), 1, "the second occurrence is recorded");
        assert_eq!(row.updates[0].timestamp, again.timestamp);
        assert_eq!(row.updates[0].title, None, "the wording did not change");

        // Raised again while still unread: recorded, but counted once.
        m.push(notice("Sync failed", BannerSeverity::Error, Vec::new()));
        assert_eq!(m.unread_count().get(), 1);
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.updates.len(), 2);
    }

    /// An entry raised as already read merges as read, and leaves the unread
    /// count where the row's own state puts it.
    #[test]
    fn a_notice_raised_again_takes_its_read_state() {
        let m = NotificationArchiveModel::in_memory();
        m.push(notice("Sync failed", BannerSeverity::Error, Vec::new()));
        assert_eq!(m.unread_count().get(), 1);
        let mut seen = notice("Sync failed", BannerSeverity::Error, Vec::new());
        seen.read = true;
        m.push(seen);
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert!(row.read);
        assert_eq!(m.unread_count().get(), 0);
    }

    /// A row keeps a bounded history: a progress notice that words each step
    /// differently records one update a step, and an id reused for every run
    /// of an operation gathers them from every run.
    #[test]
    fn a_rows_update_history_is_bounded() {
        let m = NotificationArchiveModel::in_memory();
        m.push(notice("Importing 0%", BannerSeverity::Info, Vec::new()));
        for percent in 1..=100 {
            m.push_update(notice(
                &format!("Importing {percent}%"),
                BannerSeverity::Info,
                Vec::new(),
            ));
        }
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.updates.len(), UPDATE_HISTORY_LIMIT);
        assert_eq!(
            row.updates.last().and_then(|u| u.title.as_deref()),
            Some("Importing 100%"),
            "the newest records are the ones kept"
        );
        assert_eq!(
            row.updates.first().and_then(|u| u.title.as_deref()),
            Some(format!("Importing {}%", 101 - UPDATE_HISTORY_LIMIT).as_str())
        );

        // A notice raised again under the same id is bounded the same way.
        for _ in 0..5 {
            m.push(notice("Importing 0%", BannerSeverity::Info, Vec::new()));
        }
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.updates.len(), UPDATE_HISTORY_LIMIT);
    }

    /// An update whose row is gone (cleared by the user, or evicted) is not
    /// lost: it becomes a row of its own.
    #[test]
    fn an_update_with_no_row_is_pushed_as_one() {
        let m = NotificationArchiveModel::in_memory();
        m.push(notice("Importing", BannerSeverity::Info, Vec::new()));
        m.clear();
        m.push_update(notice("Imported", BannerSeverity::Success, Vec::new()));
        assert_eq!(m.entries().len(), 1);
        let row = m.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.title, "Imported");
        assert!(!row.read);
        assert_eq!(m.unread_count().get(), 1);
    }

    /// The row a notice was raised again into reaches disk at the top, under
    /// its new date: the persistent backend moves it like the in-memory one.
    #[test]
    fn a_notice_raised_again_persists_at_the_top() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("raised_again_test");
        let again = jiff::Timestamp::from_second(15 * 3600).unwrap();
        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(notice("Sync failed", BannerSeverity::Error, Vec::new()));
            m.push(entry("Saved"));
            m.mark_all_read();
            let mut raised = notice("Sync failed", BannerSeverity::Error, Vec::new());
            raised.timestamp = again;
            m.push(raised);
            m.flush_now().unwrap();
        }
        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        assert_eq!(reopened.entries().len(), 2);
        let row = reopened.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.dedup_id.as_deref(), Some("import"));
        assert_eq!(row.timestamp, again);
        assert!(!row.read);
        assert_eq!(reopened.unread_count().get(), 1);
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
            m.push_update(second);
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

    /// The row a notice was updated into is the row that reaches disk: its
    /// actions, replay names included, and its severity survive a restart.
    #[test]
    fn a_merged_rows_actions_persist_across_reopen() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let archive = NotificationArchive::persistent("merged_actions_test");
        let report = vec![action(
            "See report",
            Some("app.import.report"),
            ArchivedActionStyle::Link,
        )];
        {
            let m = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
            m.push(notice(
                "Importing",
                BannerSeverity::Info,
                vec![action("Cancel", None, ArchivedActionStyle::Destructive)],
            ));
            m.push_update(notice("Imported", BannerSeverity::Success, report.clone()));
            m.flush_now().unwrap();
        }
        let reopened = NotificationArchiveModel::open(&archive, &paths, Duration::ZERO).unwrap();
        let row = reopened.entries().with_item(0, |e| e.clone()).unwrap();
        assert_eq!(row.actions, report);
        assert_eq!(row.severity, BannerSeverity::Success);
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
        let wrapper = teksilo_settings::ListFile {
            version: 1,
            items: vec![original.clone()],
        };
        let serialized = toml::to_string(&wrapper).expect("serialize");
        let parsed: teksilo_settings::ListFile<NotificationEntry> =
            toml::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0], original);
    }

    // ----- multi-window rebuild notification -----

    /// What the deleted `window_versions` map used to buy, now a
    /// property of the shared signal itself: N windows' bells / logs
    /// each bind the SAME `version_signal` at `Rebuild`, and every one
    /// of them registers the mutation.
    ///
    /// Three independent `BindingRegistry`s stand in for three windows,
    /// bound the way `NotificationCenterButton::build` and
    /// `NotificationLog::build` bind. The reconcile-order half of the
    /// property — that one window's flush does not consume another's —
    /// needs real trees, and is covered by
    /// `center_button::tests::two_windowless_trees_both_rebuild_on_one_archive_push`.
    #[test]
    fn every_windows_binding_sees_an_archive_mutation() {
        use teksilo_core::binding::{BindingLevel, BindingRegistry};
        use teksilo_core::widget_id::WidgetId;

        let m = NotificationArchiveModel::in_memory();
        let bell: WidgetId = slotmap::KeyData::from_ffi(1).into();
        let windows: Vec<BindingRegistry> = (0..3).map(|_| BindingRegistry::new()).collect();
        for reg in &windows {
            m.version_signal().bind_to(bell, reg, BindingLevel::Rebuild);
        }
        for reg in &windows {
            assert!(!reg.any_dirty(), "a fresh binding starts clean");
        }

        m.push(entry("first"));

        for (i, reg) in windows.iter().enumerate() {
            assert!(
                reg.any_dirty(),
                "window {i} missed the archive mutation — asking window 0 \
                 must not have consumed it"
            );
        }
    }
}
