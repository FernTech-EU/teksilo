// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Persistence bridges for `teksilo-data` collection models.
//!
//! [`list::PersistedListModel<T>`] wraps a `teksilo-data` `ListModel<T>` so
//! that mutations are automatically mirrored to a single TOML file via a
//! debounced, cross-process-safe **op** merge (upsert / remove / clear by
//! key) — never a whole-document snapshot overwrite. In-memory is the
//! source of truth: the bridge seeds the model from disk on construction.
//!
//! ## When to use
//!
//! Flat ordered collections (pinned items, recent searches, palette
//! entries) below ~1k items, where each item has a stable identity (see
//! [`list::Keyed`]). For larger or rapidly-mutating collections prefer
//! SQLite.
//!
//! There used to be a `tree` sibling module (`PersistedTreeModel`) for
//! nested hierarchies; it had zero consumers anywhere in this workspace or
//! in Skribisto and carried the identical whole-snapshot-clobber defect
//! this crate now cross-process-hardens everything else against, so it was
//! deleted rather than dragged through that hardening. Nothing currently
//! depends on it; reintroduce it (ops-based, from scratch) if a consumer
//! actually needs a persisted tree.
//!
//! ## Example
//!
//! ```
//! use teksilo_settings::{Keyed, Migrator, PersistedListModel};
//! use serde::{Deserialize, Serialize};
//! use std::time::Duration;
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! struct Tag { name: String }
//!
//! impl Keyed for Tag {
//!     type Key = String;
//!     fn key(&self) -> String { self.name.clone() }
//! }
//!
//! let path = std::env::temp_dir().join("tags-doctest.toml");
//! let plm: PersistedListModel<Tag> =
//!     PersistedListModel::open(path, Duration::ZERO, Migrator::new())
//!         .expect("open failed");
//! plm.upsert_front(Tag { name: "rust".into() });
//! plm.flush_now().expect("flush failed");
//! ```

pub mod list;
