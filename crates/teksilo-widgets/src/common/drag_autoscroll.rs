// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The drag auto-scroll edge ramp, implemented once.
//!
//! Every view that accepts a drag over a scrolling viewport — `ListView`,
//! `TreeView`, `TableView`, `TreeTableView`, `GridView`'s marquee and the
//! `TabBar` strip — scrolls when the pointer lingers near an edge. Each one
//! carried its own `EDGE = 32.0` / `MAX_VELOCITY = 12.0` pair and its own copy
//! of the ramp; this module is the single definition they now share.
//!
//! # Why the band is a pointer-kind question, not a density one
//!
//! The edge band is the distance from the viewport edge at which auto-scroll
//! begins. What makes it right or wrong is how precisely the user can park a
//! pointer near an edge, which is a property of the *device*: a mouse cursor
//! lands where it is put, a fingertip covers roughly 9 mm of screen and its
//! reported centre wanders. So the band widens for a coarse pointer and is
//! unchanged for a precise one — it does not move with [`TargetDensity`],
//! which describes how big the *UI's* targets are.
//!
//! The velocity cap is neither: it is a rate, not a dimension, and the same
//! 12 dp per tick reads correctly under every pointer and every density.
//!
//! Reference: `docs/density-inventory.md` §3b, and the touch design's
//! Constants table ("DnD edge auto-scroll band / max velocity").
//!
//! [`TargetDensity`]: teksilo_tokens::TargetDensity

use teksilo_tokens::PointerKind;

/// Edge band for a precise pointer (mouse, pen), in dp. The value every data
/// view used before the touch programme, kept exactly.
pub const EDGE_BAND_PRECISE: f32 = 32.0;

/// Edge band for a coarse pointer (a finger), in dp — twice the precise band,
/// because a contact patch's reported centre cannot be parked as finely as a
/// cursor.
pub const EDGE_BAND_COARSE: f32 = 64.0;

/// Cap on the per-tick auto-scroll step, in dp. A rate, not a dimension: the
/// same under every pointer kind and every density.
pub const MAX_VELOCITY: f32 = 12.0;

/// The edge band for a pointer kind.
///
/// [`PointerKind::Mouse`], [`PointerKind::Pen`] and [`PointerKind::Unknown`]
/// get [`EDGE_BAND_PRECISE`]; only [`PointerKind::Touch`] widens.
pub fn band_for(kind: PointerKind) -> f32 {
    if kind.is_coarse() {
        EDGE_BAND_COARSE
    } else {
        EDGE_BAND_PRECISE
    }
}

/// The auto-scroll step, in dp, for a pointer at `pointer_main` along a
/// viewport of `viewport_extent`.
///
/// Zero outside both bands; ramps linearly inside a band and saturates at
/// [`MAX_VELOCITY`] past the viewport edge (a captured drag keeps reporting
/// positions beyond the boundary). Negative scrolls toward the leading edge.
///
/// Pure, so every caller's ramp can be unit tested without a widget tree.
pub fn step(pointer_main: f32, viewport_extent: f32, band: f32) -> f32 {
    if band <= 0.0 {
        return 0.0;
    }
    let leading_in = (band - pointer_main).max(0.0);
    let trailing_in = (pointer_main - (viewport_extent - band)).max(0.0);
    if leading_in > 0.0 {
        -(leading_in / band).min(1.0) * MAX_VELOCITY
    } else if trailing_in > 0.0 {
        (trailing_in / band).min(1.0) * MAX_VELOCITY
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mouse keeps the exact band every view hard-coded before this module
    /// existed — the no-regression statement for the whole de-duplication.
    #[test]
    fn a_precise_pointer_keeps_the_original_band() {
        assert_eq!(band_for(PointerKind::Mouse), 32.0);
        assert_eq!(
            band_for(PointerKind::Pen(teksilo_tokens::PenKind::Pen)),
            32.0
        );
        assert_eq!(band_for(PointerKind::Unknown), 32.0);
    }

    #[test]
    fn a_finger_gets_a_wider_band() {
        assert_eq!(band_for(PointerKind::Touch), 64.0);
    }

    #[test]
    fn the_ramp_is_zero_in_the_middle_and_saturates_past_the_edge() {
        let band = EDGE_BAND_PRECISE;
        assert_eq!(step(200.0, 400.0, band), 0.0);
        assert!(step(10.0, 400.0, band) < 0.0);
        assert!(step(390.0, 400.0, band) > 0.0);
        assert!((step(-50.0, 400.0, band) + MAX_VELOCITY).abs() < 1e-6);
        assert!((step(500.0, 400.0, band) - MAX_VELOCITY).abs() < 1e-6);
    }

    #[test]
    fn a_zero_band_never_scrolls() {
        assert_eq!(step(0.0, 400.0, 0.0), 0.0);
    }
}
