// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS shape tokens — corner radii, stroke widths, focus geometry and
//! elevation shadows.
//!
//! ## Radii
//!
//! Apple publishes no corner-radius table at all. Big Sur is documented
//! only as *"larger than Catalina"*, and macOS 26 Tahoe makes radii
//! "concentric" and window-style-dependent, still without numbers. The two
//! values here are measured off Sequoia at 2×:
//!
//! - **6 dp** for an in-page control — push button, popup button, text
//!   field, segmented control, list row capsule.
//! - **10 dp** for something that floats — sheet, panel, popover, HUD.
//!
//! A menu is the one measured exception at **9 dp**
//! ([`MACOS_MENU_CORNER_RADIUS`]) and is applied to the menu flyout
//! specifically rather than rounded off to the overlay radius.
//!
//! Note how much rounder this is than Fluent's 4/8 and how much less
//! round than Material 3's fully-pilled buttons: macOS sits deliberately
//! in the middle, and 6 dp on a 22 dp-tall button is the single geometric
//! fact that most makes a control read as Mac rather than Windows.
//!
//! ## The focus ring
//!
//! This is where macOS diverges most sharply from Fluent. Fluent draws a
//! two-tone **high-contrast neutral** ring — near-black in light, white in
//! dark — precisely so it never depends on the accent. macOS draws the
//! **accent itself**, as a soft halo hugging the control's own outline.
//!
//! Drawn naively that fails accessibility: the accent at the ~50 % alpha
//! the halo appears to have measures 1.86:1 against `windowBackgroundColor`,
//! well under WCAG SC 1.4.11's 3:1 floor for a focus indicator. So the ring
//! is built as two bands — a [`MACOS_FOCUS_RING_WIDTH`]-wide band at **full**
//! accent that carries the contrast, and a [`MACOS_FOCUS_RING_HALO_WIDTH`]
//! band at [`MACOS_FOCUS_RING_HALO_ALPHA`] outside it that supplies the
//! macOS softness. The measured contrast comes from the solid band; the
//! *look* comes from the halo. [`crate::styles::chrome::paint_focus_ring`]
//! paints both.
//!
//! ## Elevation
//!
//! macOS leans on shadows harder than Material 3 (which prefers tonal
//! surfaces) and much harder than IntUI: a menu, popover or sheet casts a
//! wide, soft, unmistakable shadow, while a *control* casts almost none —
//! a hairline's worth, just enough to lift it off the window. Neither has
//! a published offset/blur/alpha triple (`NSShadow` on a control is
//! private, and `shadowColor` is one of the values Apple does not
//! publish), so the tokens below keep the framework's baseline geometry
//! and re-grade its alpha to the density macOS surfaces read at. The
//! control-sized shadow is carried separately on
//! [`MacOsBezel::shadow`](crate::palette::MacOsBezel::shadow) because it is
//! painted by the control chrome, not by an overlay.

use teksilo_tokens::ShapeTokens;

/// In-page control radius — push button, popup button, text field,
/// segmented control, list-row capsule.
pub const MACOS_CONTROL_CORNER_RADIUS: f32 = 6.0;
/// Floating-surface radius — sheet, panel, popover, HUD, dialog.
pub const MACOS_OVERLAY_CORNER_RADIUS: f32 = 10.0;
/// A menu's own radius. The one radius with a real measurement behind it,
/// and it is neither of the other two.
pub const MACOS_MENU_CORNER_RADIUS: f32 = 9.0;

/// Width of the focus ring's **solid** inner band — the part that carries
/// the WCAG 1.4.11 contrast. See the module doc.
pub const MACOS_FOCUS_RING_WIDTH: f32 = 2.0;
/// Width of the translucent halo outside the solid band, which is what
/// makes the ring read as a macOS glow rather than an outline.
pub const MACOS_FOCUS_RING_HALO_WIDTH: f32 = 2.0;
/// Alpha of the halo.
pub const MACOS_FOCUS_RING_HALO_ALPHA: f32 = 0.35;
/// Gap between the control edge and the ring. Zero: a macOS focus ring
/// hugs the control's outline rather than standing off it the way IntUI's
/// and Fluent's do.
pub const MACOS_FOCUS_RING_OFFSET: f32 = 0.0;

/// The standard **regular** control height (dp).
///
/// `[unpublished]` — Apple states control heights nowhere in prose; they
/// exist only in Interface Builder's size-class metrics and in the
/// non-textual design kits. 22 dp is the regular-size push button and text
/// field measured off Sequoia, and it is the number every control in this
/// preset lines up on. Small and mini size classes (19 / 16 dp) are not
/// modelled: Teksilo has no control-size concept to hang them on.
pub const MACOS_CONTROL_HEIGHT: f32 = 22.0;

/// The square (or circle) a **glyph control** occupies at the regular
/// control size — the checkbox's box and the radio's ring, which AppKit
/// draws as a matched pair.
///
/// `[measured]`, like [`MACOS_CONTROL_HEIGHT`]. Worth noting how small it
/// is: Fluent's checkbox is 20 dp, so a macOS form of the same rows is
/// visibly tighter.
pub const MACOS_SMALL_CONTROL_SIZE: f32 = 14.0;

// Relationships between the constants above, checked at compile time
// rather than in a test. The menu radius is measured independently of the
// other two and has to stay between them; the focus ring needs both of its
// bands to exist, with the halo translucent; and the control height has to
// stay denser than Fluent's 32 dp while still clearing the 16 dp Body line
// box with air to spare.
const _: () = assert!(MACOS_MENU_CORNER_RADIUS > MACOS_CONTROL_CORNER_RADIUS);
const _: () = assert!(MACOS_MENU_CORNER_RADIUS < MACOS_OVERLAY_CORNER_RADIUS);
const _: () = assert!(MACOS_CONTROL_CORNER_RADIUS < MACOS_OVERLAY_CORNER_RADIUS);
const _: () = assert!(MACOS_FOCUS_RING_WIDTH > 0.0);
const _: () = assert!(MACOS_FOCUS_RING_HALO_WIDTH > 0.0);
const _: () = assert!(MACOS_FOCUS_RING_HALO_ALPHA > 0.0);
const _: () = assert!(MACOS_FOCUS_RING_HALO_ALPHA < 1.0);
// Denser than Fluent's 32 dp — the whole point of the value.
const _: () = assert!(MACOS_CONTROL_HEIGHT < 32.0);
// …and still tall enough for a 16 dp Body line box plus air.
const _: () = assert!(MACOS_CONTROL_HEIGHT >= 20.0);
const _: () = assert!(MACOS_SMALL_CONTROL_SIZE < MACOS_CONTROL_HEIGHT);

/// macOS **Aqua** shape tokens.
pub fn macos_light_shape() -> ShapeTokens {
    let mut s = ShapeTokens::light_default();
    apply_common(&mut s);
    // A light-theme macOS shadow is a soft grey halo, not a bruise —
    // noticeably lighter than IntUI's baseline. Outer xs/sm/md/lg, then
    // the sharp inner rims.
    regrade_shadows(&mut s, [0.22, 0.15, 0.19, 0.28], [0.10, 0.08, 0.10, 0.14]);
    s
}

/// macOS **Dark Aqua** shape tokens.
pub fn macos_dark_shape() -> ShapeTokens {
    let mut s = ShapeTokens::dark_default();
    apply_common(&mut s);
    // A pure-black shadow barely registers on `#323232`, so dark alphas
    // run much higher than light — and a Dark Aqua menu really does read
    // as floating well clear of the window behind it.
    regrade_shadows(&mut s, [0.46, 0.42, 0.50, 0.64], [0.24, 0.24, 0.28, 0.36]);
    s
}

fn apply_common(s: &mut ShapeTokens) {
    s.radius_control = MACOS_CONTROL_CORNER_RADIUS;
    s.radius_popup = MACOS_OVERLAY_CORNER_RADIUS;
    s.border_width = 1.0;
    s.focus_ring_width = MACOS_FOCUS_RING_WIDTH;
    s.focus_ring_offset = MACOS_FOCUS_RING_OFFSET;
    // `radius_pill` (9999) keeps the baseline — macOS uses fully-rounded
    // ends for the switch track, the slider knob and the scroller thumb.
}

/// Replace the alpha of each paired outer/inner shadow colour, keeping the
/// baseline offset/blur geometry so overlay positioning is unchanged.
/// `outer` / `inner` are `[xs, sm, md, lg]`.
fn regrade_shadows(s: &mut ShapeTokens, outer: [f32; 4], inner: [f32; 4]) {
    s.shadow_xs.color = s.shadow_xs.color.with_alpha(outer[0]);
    s.shadow_sm.color = s.shadow_sm.color.with_alpha(outer[1]);
    s.shadow_md.color = s.shadow_md.color.with_alpha(outer[2]);
    s.shadow_lg.color = s.shadow_lg.color.with_alpha(outer[3]);
    s.shadow_inner_xs.color = s.shadow_inner_xs.color.with_alpha(inner[0]);
    s.shadow_inner_sm.color = s.shadow_inner_sm.color.with_alpha(inner[1]);
    s.shadow_inner_md.color = s.shadow_inner_md.color.with_alpha(inner[2]);
    s.shadow_inner_lg.color = s.shadow_inner_lg.color.with_alpha(inner[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radii_are_the_measured_macos_values() {
        for s in [macos_light_shape(), macos_dark_shape()] {
            assert_eq!(s.radius_control, 6.0);
            assert_eq!(s.radius_popup, 10.0);
            assert!(s.radius_control < s.radius_popup);
            assert!(s.radius_popup < s.radius_pill);
        }
    }

    #[test]
    fn the_menu_radius_is_its_own_measured_value() {
        // Measured at 9 dp — neither the control radius nor the overlay one.
        // That it *sits between* them is a compile-time invariant above.
        assert_eq!(MACOS_MENU_CORNER_RADIUS, 9.0);
        assert_ne!(MACOS_MENU_CORNER_RADIUS, MACOS_CONTROL_CORNER_RADIUS);
        assert_ne!(MACOS_MENU_CORNER_RADIUS, MACOS_OVERLAY_CORNER_RADIUS);
    }

    #[test]
    fn controls_are_rounder_than_fluent_and_squarer_than_a_pill() {
        // The geometric fact that most distinguishes the three presets.
        let s = macos_light_shape();
        assert!(s.radius_control > 4.0, "Fluent's ControlCornerRadius");
        assert!(s.radius_control < MACOS_CONTROL_HEIGHT / 2.0, "not a pill");
    }

    #[test]
    fn the_focus_ring_hugs_the_control() {
        // Unlike IntUI (2 dp gap) and Fluent (1 dp), a macOS ring starts
        // at the control's own outline.
        for s in [macos_light_shape(), macos_dark_shape()] {
            assert_eq!(s.focus_ring_offset, 0.0);
            assert_eq!(s.focus_ring_width, MACOS_FOCUS_RING_WIDTH);
        }
    }

    #[test]
    fn the_ring_envelope_is_split_between_a_solid_band_and_a_halo() {
        // That both bands exist and the halo is translucent are
        // compile-time invariants above; this pins the literals, and the
        // total envelope against the ~3–4 pt a macOS ring occupies.
        assert_eq!(MACOS_FOCUS_RING_WIDTH, 2.0);
        assert_eq!(MACOS_FOCUS_RING_HALO_WIDTH, 2.0);
        let envelope =
            MACOS_FOCUS_RING_OFFSET + MACOS_FOCUS_RING_WIDTH + MACOS_FOCUS_RING_HALO_WIDTH;
        assert!((3.0..=5.0).contains(&envelope), "envelope is {envelope} dp");
    }

    #[test]
    fn dark_elevation_is_stronger_than_light() {
        let l = macos_light_shape();
        let d = macos_dark_shape();
        assert!(d.shadow_lg.color.a() > l.shadow_lg.color.a());
        assert!(d.shadow_inner_lg.color.a() > l.shadow_inner_lg.color.a());
    }

    #[test]
    fn light_shadows_are_softer_than_the_intui_baseline() {
        let base = teksilo_tokens::ShapeTokens::light_default();
        let m = macos_light_shape();
        assert!(m.shadow_lg.color.a() < base.shadow_lg.color.a());
        assert!(m.shadow_md.color.a() < base.shadow_md.color.a());
        assert!(m.shadow_inner_lg.color.a() < base.shadow_inner_lg.color.a());
    }

    #[test]
    fn shadow_geometry_is_preserved() {
        // Only the alpha is re-graded; offsets and blurs stay on the
        // baseline so overlay positioning is unchanged.
        let base = teksilo_tokens::ShapeTokens::dark_default();
        let m = macos_dark_shape();
        assert_eq!(m.shadow_md.blur, base.shadow_md.blur);
        assert_eq!(m.shadow_md.offset_y, base.shadow_md.offset_y);
        assert_eq!(m.shadow_lg.blur, base.shadow_lg.blur);
    }

    #[test]
    fn the_control_heights_are_the_measured_values() {
        // The *relationships* — denser than Fluent, taller than a Body
        // line box, and the glyph control smaller than the row height —
        // are compile-time invariants above. This pins the literals.
        assert_eq!(MACOS_CONTROL_HEIGHT, 22.0);
        assert_eq!(MACOS_SMALL_CONTROL_SIZE, 14.0);
    }
}
