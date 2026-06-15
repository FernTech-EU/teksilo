// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Re-export shim: `PrefixSumOffsets` moved to [`crate::common::row_offsets`]
//! so the 1-D row widgets (`ListView` / `TreeView` / `TableView` /
//! `TreeTable`, via `RowMetrics`) can share it. Grid strategies keep their
//! `super::offsets::PrefixSumOffsets` paths through this shim.

pub(crate) use crate::common::row_offsets::PrefixSumOffsets;
