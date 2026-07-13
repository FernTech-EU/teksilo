// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `bastyde-settings` — persistent, reactive user preferences for Bastyde.
//!
//! Two persistence shapes share one storage backbone:
//!
//! * [`SettingsStore`] — dynamic, dotted-key K/V for **scalar** values
//!   (numbers, strings, bools, arrays of those), surfaced as
//!   `Signal<T>`. The QSettings analogue. Struct values aren't supported
//!   here because TOML serializes them as tables, indistinguishable from
//!   nested key paths; use [`SettingsFile<T>`] for those instead.
//! * [`SettingsFile<T>`] — typed single-struct persistence with
//!   migrations.
//! * [`PersistedListModel<T>`] — a reactive, [`Keyed`]-item ordered
//!   collection persisted to a single file, merging by **op** (upsert /
//!   remove / clear) rather than by whole-document snapshot — so a peer
//!   process's concurrent addition survives.
//! * [`MruList<T>`] — generic dedupe + pin + cap list over an
//!   app-defined item type implementing [`MruEntry`]. Apps register
//!   their own via `BastydeAppBuilder::app_state(handle)`.
//! * [`WindowStateService`] — per-window geometry persistence. When
//!   registered via `BastydeAppBuilder::settings(...)`, every
//!   `WindowConfig` carrying an `id(...)` is automatically restored
//!   on creation (sanitized against the current monitor) and recorded
//!   on every change by `bastyde-app`'s window manager. No widget-side
//!   wiring required.
//!
//! All disk writes are atomic (write-temp + rename). Every write **merges**
//! against the document read fresh off disk under an exclusive lock —
//! never a blind whole-document overwrite — so two processes sharing a
//! file (Skribisto's one-process-per-project model shares `general.toml`,
//! `recents.toml`, `window_state.toml` across every open project) cannot
//! silently clobber each other. [`Reloadable`] is the matching read-side
//! contract: a hook a file-system watcher calls to push a peer's write into
//! this process's live signals/models — and [`SettingsWatcher`] +
//! [`SettingsRegistry`] are the actual watcher: a `notify`-based directory
//! watch (parent-directory, not file, so an atomic rename-over doesn't
//! invalidate it) plus the path → [`Reloadable`] lookup a changed-file
//! event dispatches through. `BastydeAppBuilder::settings(...)` wires one
//! up automatically (opt out via `.settings_watch(false)`); every service
//! [`SettingsBundle::open`] opens is pre-registered into
//! [`OpenedSettings::registry`], and application code can register its
//! own ad hoc `SettingsFile` / `PersistedListModel` / `MruList` handles
//! into that same registry.
//!
//! ```
//! use bastyde_settings::{AppPaths, SettingsStore, SettingsKey};
//!
//! const FONT_SIZE: SettingsKey<f32> =
//!     SettingsKey::new("editor.font_size", || 14.0);
//!
//! // Tests / docs use `for_testing(...)` against a tempdir so they
//! // never touch the user's real config tree. Production apps use
//! // `AppPaths::new(qualifier, organization, application)`.
//! let tmp = tempfile::tempdir().unwrap();
//! let paths = AppPaths::for_testing(tmp.path());
//! let store = SettingsStore::open(paths.config_file("general")).unwrap();
//!
//! let font_size = store.signal_for(&FONT_SIZE);
//! font_size.set(18.0); // persisted on the next debounce tick
//! ```

mod bundle;
mod collection;
mod ext;
mod file;
mod flush;
mod lock;
mod migration;
mod mru;
mod path;
mod reload;
mod store;
mod watch;
mod window_state;

pub use crate::bundle::{OpenedSettings, SettingsBundle, SettingsBundleError};
pub use crate::collection::list::{Keyed, ListFile, ListOp, PersistedListModel};
pub use crate::ext::SettingsExt;
pub use crate::file::{SettingsFile, SettingsFileError};
pub use crate::flush::{
    DebouncedWriter, FlushError, LandedStamp, WriteFailureSink, WriteLandedSink,
    set_write_failure_sink,
};
pub use crate::migration::{MigrationError, Migrator, Versioned};
pub use crate::mru::{MruEntry, MruList};
pub use crate::path::AppPaths;
pub use crate::reload::Reloadable;
pub use crate::store::{
    DEFAULT_DEBOUNCE, SettingsKey, SettingsStore, SettingsStoreError, TEXT_SCALE_KEY,
};
pub use crate::watch::{SettingsRegistry, SettingsReloadSink, SettingsWatcher};
pub use crate::window_state::{PerWindowState, WindowStateService};
