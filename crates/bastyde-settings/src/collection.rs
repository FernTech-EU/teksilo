// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Persistence bridges for `bastyde-data` collection models.
//!
//! Each submodule wraps a `bastyde-data` model so that every mutation is
//! automatically mirrored to a single TOML file via a
//! [`SettingsFile`](crate::SettingsFile)-managed debounced atomic write.
//! In-memory state is the source of truth: the bridge seeds the model
//! from disk on construction and installs an `observe_changes` callback
//! that re-serializes the whole collection whenever the model changes.
//!
//! ## When to use
//!
//! * [`list::PersistedListModel<T>`] — flat ordered collections (pinned
//!   items, recent searches, palette entries) below ~1 k items.
//! * [`tree::PersistedTreeModel<T>`] — nested hierarchies (saved
//!   queries, custom menu trees) below ~1 k total nodes.
//!
//! For larger or rapidly-mutating collections prefer SQLite.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_settings::collection::list::{ListFile, PersistedListModel};
//! use bastyde_settings::migration::Migrator;
//! use serde::{Deserialize, Serialize};
//! use std::time::Duration;
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! struct Tag { name: String }
//!
//! let path = std::env::temp_dir().join("tags.toml");
//! let plm: PersistedListModel<Tag> =
//!     PersistedListModel::open(path, Duration::ZERO, Migrator::new())
//!         .expect("open failed");
//! plm.model().push(Tag { name: "rust".into() });
//! plm.flush_now().expect("flush failed");
//! ```

pub mod list;
pub mod tree;
