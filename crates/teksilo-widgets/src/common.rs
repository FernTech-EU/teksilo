// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Cross-widget shared infrastructure.
//!
//! Helpers and types reused by more than one widget in `teksilo-widgets` but
//! that don't fit cleanly under any single widget's module. Members are
//! re-exported by their submodule name; rebinding through `common::*`
//! is intentional so the widget code reads `common::datetime::Date`
//! without dragging the underlying crate name (`jiff`) into the public
//! surface.

pub(crate) mod column_geometry;
pub mod datetime;
pub(crate) mod editor_runtime;
pub(crate) mod list_nav;
#[cfg(test)]
pub(crate) mod locale_switch_test;
pub(crate) mod row_metrics;
pub(crate) mod row_offsets;
pub(crate) mod scroll;
pub(crate) mod text_nav;
#[cfg(test)]
pub(crate) mod thumb_drag_test;
pub(crate) mod tree_expand;
pub(crate) mod type_ahead;
pub(crate) mod viewport;
