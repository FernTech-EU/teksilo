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
//! * [`PersistedListModel`] / [`PersistedTreeModel`] — typed reactive
//!   collections persisted to a single file each.
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
//! All disk writes are atomic (write-temp + rename) and debounced. The store
//! is the source of truth in memory; disk is a flushed projection.
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
mod migration;
mod mru;
mod path;
mod store;
mod window_state;

pub use crate::bundle::{OpenedSettings, SettingsBundle, SettingsBundleError};
pub use crate::collection::list::{ListFile, PersistedListModel};
pub use crate::collection::tree::{PersistedTreeModel, PersistedTreeNode, TreeFile};
pub use crate::ext::SettingsExt;
pub use crate::file::{SettingsFile, SettingsFileError};
pub use crate::flush::{DebouncedWriter, FlushError};
pub use crate::migration::{MigrationError, Migrator, Versioned};
pub use crate::mru::{MruEntry, MruList};
pub use crate::path::AppPaths;
pub use crate::store::{DEFAULT_DEBOUNCE, SettingsKey, SettingsStore, SettingsStoreError};
pub use crate::window_state::{PerWindowState, WindowStateService};
