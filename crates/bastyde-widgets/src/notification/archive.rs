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

use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use bastyde_core::signal::Signal;
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
    /// that render the archive (e.g. `NotificationLog`) bind to
    /// this at `BindingLevel::Rebuild`.
    pub fn version_signal(&self) -> &Signal<u64> {
        &self.version
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

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
                // Read the existing entry, append an update, and set
                // it back. `ListModel::set` swaps the whole entry —
                // we preserve the original `id` + `timestamp` and
                // append a `NotificationUpdate` describing the
                // mutation.
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
                    model.set(idx, existing);
                    self.bump_unread();
                    return;
                }
            }
        }

        // New entry: stamp the id, insert at index 0, evict overflow.
        let next = self.next_id.get();
        entry.id = next;
        self.next_id.set(next.wrapping_add(1));
        let is_unread = !entry.read;
        model.insert(0, entry);
        if model.len() > self.limit {
            // Evict the oldest entry. The model has no `pop_back`;
            // `remove(last_idx)` is the equivalent.
            let last = model.len() - 1;
            // If the evicted entry was unread, decrement the unread
            // count so the badge doesn't lie about how many sit on
            // disk.
            if let Some(was_unread) = model.with_item(last, |e| !e.read) {
                if was_unread {
                    let n = self.unread_count.get();
                    self.unread_count.set(n.saturating_sub(1));
                }
            }
            model.remove(last);
        }
        if is_unread {
            self.bump_unread();
        }
    }

    fn bump_unread(&self) {
        let n = self.unread_count.get();
        self.unread_count.set(n.saturating_add(1));
    }

    /// Mark every archived entry as read; reset `unread_count` to 0.
    /// Called by `NotificationCenterButton` when its popover opens.
    pub fn mark_all_read(&self) {
        let model = self.backend.model();
        let mut mutated = false;
        for i in 0..model.len() {
            if let Some(mut entry) = model.with_item(i, |e| e.clone()) {
                if !entry.read {
                    entry.read = true;
                    model.set(i, entry);
                    mutated = true;
                }
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
        self.backend.model().clear();
        self.unread_count.set(0);
        if !was_empty {
            self.bump_version();
        }
    }

    /// Remove the entry at the given list index. Updates
    /// `unread_count` if the removed entry was unread. No-op when
    /// `index` is out of bounds.
    pub fn remove(&self, index: usize) {
        let model = self.backend.model();
        if index >= model.len() {
            return;
        }
        let was_unread = model.with_item(index, |e| !e.read).unwrap_or(false);
        model.remove(index);
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
    fn remove_unread_decrements_count() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.push(entry("b"));
        assert_eq!(m.unread_count().get(), 2);

        m.remove(0); // removes "b"
        assert_eq!(m.entries().len(), 1);
        assert_eq!(m.unread_count().get(), 1);
    }

    #[test]
    fn remove_read_does_not_change_count() {
        let m = NotificationArchiveModel::in_memory();
        m.push(entry("a"));
        m.mark_all_read();
        assert_eq!(m.unread_count().get(), 0);
        m.remove(0);
        assert_eq!(m.unread_count().get(), 0);
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

        m.remove(0);
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
