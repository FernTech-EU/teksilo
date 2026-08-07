// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Inspector panel tabs.

pub(crate) mod accessibility;
pub(crate) mod data_models;
pub(crate) mod focus;
pub(crate) mod locale;
pub(crate) mod overlays;
pub(crate) mod properties;
pub(crate) mod shortcuts;
pub(crate) mod theme;
pub(crate) mod tree;

/// Last `::`-separated segment of a fully-qualified Rust type name.
pub(crate) fn last_segment(s: &str) -> &str {
    // Strip generics first so `Switcher<...>` shows the bare segment.
    let bare = s.split_once('<').map(|(a, _)| a).unwrap_or(s);
    bare.rsplit_once("::").map(|(_, t)| t).unwrap_or(bare)
}

const ROW_HEIGHT: f32 = 18.0;
const ROW_INDENT_PX: f32 = 14.0;
const ROW_PADDING_X: f32 = 6.0;
