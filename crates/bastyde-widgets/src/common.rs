// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Cross-widget shared infrastructure.
//!
//! Helpers and types reused by more than one widget in `bastyde-widgets` but
//! that don't fit cleanly under any single widget's module. Members are
//! re-exported by their submodule name; rebinding through `common::*`
//! is intentional so the widget code reads `common::datetime::Date`
//! without dragging the underlying crate name (`jiff`) into the public
//! surface.

pub mod datetime;
pub(crate) mod row_metrics;
pub(crate) mod row_offsets;
pub(crate) mod scroll;
