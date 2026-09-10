// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Inspector panel tabs.

pub(crate) mod accessibility;
pub(crate) mod data_models;
pub(crate) mod focus;
pub(crate) mod locale;
pub(crate) mod overlays;
pub(crate) mod pointers;
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

/// Row height at [`TargetDensity::Compact`](teksilo_tokens::TargetDensity) —
/// a dense debug listing read with a mouse.
///
/// Deliberately below the 24 dp conformance floor, and deliberately **not**
/// routed through [`dp`](teksilo_core::styles::density::dp): that helper is a
/// floor, so passing 18 through it would raise every inspector row at Compact
/// and change the layout of a panel nobody asked to change. See
/// [`row_height`].
const ROW_HEIGHT: f32 = 18.0;
const ROW_INDENT_PX: f32 = 14.0;
const ROW_PADDING_X: f32 = 6.0;

/// The row height in force at the active density, **for a tab whose rows are
/// pressed**.
///
/// Those rows are painted into a single leaf node and picked by dividing the
/// press's y by this number, so the row height *is* the target height — there
/// is no per-row node for A10's hit mechanisms to grow. It therefore answers to
/// the density directly: [`Compact`] keeps the dense 18 dp, and any density
/// that admits a finger takes the density's own `target_size`.
///
/// A tab that only *displays* rows — the overlay, shortcut, focus,
/// accessibility and theme dumps — keeps [`ROW_HEIGHT`] at every density. A row
/// nothing can press is not a target, and growing it would trade legibility of
/// the whole listing for a hit box no pointer wants.
///
/// Within a tab that does use it, every producer and every consumer of a row
/// index must go through it: the paint, the height the tab reports, and the
/// press-to-row division (which happens in a layout pass, or reads a height a
/// layout pass published — an event handler has no theme). A tab that painted
/// at one height and divided by another would select the wrong row.
///
/// [`Compact`]: teksilo_tokens::TargetDensity::Compact
pub(crate) fn row_height(tokens: &teksilo_tokens::InputTokens) -> f32 {
    match tokens.density {
        teksilo_tokens::TargetDensity::Compact => ROW_HEIGHT,
        _ => teksilo_core::styles::density::dp(
            ROW_HEIGHT,
            teksilo_tokens::TargetRole::Target,
            tokens,
        ),
    }
}
