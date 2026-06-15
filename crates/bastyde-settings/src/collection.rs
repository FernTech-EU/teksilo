// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Persistence bridges for `bastyde-data` collection models.
//!
//! Each submodule wraps a `*Model<T>` so its mutations are mirrored to
//! a [`SettingsFile`](crate::SettingsFile)-managed file with debounced
//! atomic writes.

pub mod list;
pub mod tree;
