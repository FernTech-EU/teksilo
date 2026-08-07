// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`OverscrollBehavior`] — boundary scroll-chaining policy.
//!
//! Shared by every scrollable surface (the `teksilo-widgets` scrollables —
//! `ScrollArea` / `ListView` / `TreeView` / `TableView` — and the
//! `teksilo-scene` `SceneView` pan handler). It lives in `teksilo-core` so
//! both tiers can name it without `teksilo-scene` depending on
//! `teksilo-widgets`. `teksilo-widgets` re-exports it as
//! `teksilo_widgets::OverscrollBehavior` for backwards compatibility.

/// Below this many logical pixels a scroll/pan axis is considered "did not
/// move" (so a fully-clamped boundary event chains). Shared by the widget
/// scrollables and the `teksilo-scene` pan handler so the boundary threshold
/// is defined once. Tighter than a display pixel, looser than f32 clamp noise.
pub const SCROLL_MOVE_EPSILON: f32 = 1e-3;

/// Controls whether a scrollable surface chains scroll events to its ancestor
/// when it reaches a boundary — the equivalent of CSS `overscroll-behavior`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverscrollBehavior {
    /// At a boundary, decline the event (`Ignored`) so it propagates to the
    /// next ancestor scrollable. The default (`overscroll-behavior: auto`).
    ///
    /// Note: a scrollable whose content fits entirely (nothing to scroll on
    /// either axis) is *always* at its boundary, so under `Chain` it lets the
    /// wheel through to an ancestor rather than swallowing it. This matches
    /// the web; set [`Contain`](Self::Contain) on a fit-to-content panel that
    /// should absorb the wheel regardless.
    #[default]
    Chain,
    /// Always absorb the event (`Handled`), even at the boundary — no
    /// chaining. Equivalent to `overscroll-behavior: contain`.
    Contain,
}
