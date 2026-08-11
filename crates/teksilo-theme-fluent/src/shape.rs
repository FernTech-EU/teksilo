// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent shape tokens — corner radii, stroke widths, focus geometry and
//! elevation shadows.
//!
//! Fluent's whole radius vocabulary is two values: **`ControlCornerRadius`
//! = 4 dp** for anything a pointer interacts with (button, field, combo,
//! checkbox, list row) and **`OverlayCornerRadius` = 8 dp** for anything
//! that floats (flyout, menu, dialog, tooltip, teaching tip). They map
//! one-to-one onto Teksilo's `radius_control` / `radius_popup`, so the two
//! systems agree here without any reinterpretation.
//!
//! Strokes are 1 dp, as in IntUI — but Fluent's *emphasis* comes from an
//! asymmetric bottom edge on raised controls rather than from thickness;
//! that edge is drawn by [`crate::styles::button::FluentButtonStyle`], not
//! expressible as a shape token.
//!
//! The focus indicator is the one place the geometry genuinely differs:
//! Fluent draws a **two-tone** ring — a 2 dp high-contrast outer ring with
//! a 1 dp opposite-colour inner ring between it and the control. Teksilo's
//! `focus_ring_width` / `focus_ring_offset` describe the outer ring
//! (`FocusVisualPrimaryThickness = 2`, sitting ~1 dp off the control
//! edge); the inner ring is painted by the Fluent widget styles that own
//! their own focus chrome.
//!
//! Elevation: Fluent leans on flyout shadows much harder than Material 3
//! (which prefers tonal surfaces) but stays softer and *lower* than IntUI.
//! `SystemShadow` / `ThemeShadow` are composition effects with no published
//! offset/blur/alpha triple, so the values below keep the baseline
//! geometry and re-alpha it to the density Fluent surfaces read at.

use teksilo_tokens::ShapeTokens;

/// `ControlCornerRadius` — buttons, fields, combo boxes, list rows,
/// checkbox visuals, menu rows.
pub const FLUENT_CONTROL_CORNER_RADIUS: f32 = 4.0;
/// `OverlayCornerRadius` — flyouts, menus, dialogs, tooltips.
pub const FLUENT_OVERLAY_CORNER_RADIUS: f32 = 8.0;
/// `FocusVisualPrimaryThickness` — the outer, high-contrast focus ring.
pub const FLUENT_FOCUS_RING_WIDTH: f32 = 2.0;
/// `FocusVisualSecondaryThickness` — the inner ring that separates the
/// outer ring from the control edge.
pub const FLUENT_FOCUS_RING_INNER_WIDTH: f32 = 1.0;
/// Gap between the control edge and the outer focus ring. WinUI expresses
/// this as `FocusVisualMargin`; one device-independent pixel of clearance
/// is what the default visuals produce.
pub const FLUENT_FOCUS_RING_OFFSET: f32 = 1.0;

/// Fluent **light** shape tokens.
pub fn fluent_light_shape() -> ShapeTokens {
    let mut s = ShapeTokens::light_default();
    apply_common(&mut s);
    // Light elevation: flyouts read as floating but the shadow stays a
    // hint, never a bruise. Outer xs/sm/md/lg, then the sharp inner rims.
    soften_shadows(&mut s, [0.20, 0.14, 0.18, 0.26], [0.14, 0.10, 0.12, 0.18]);
    s
}

/// Fluent **dark** shape tokens.
pub fn fluent_dark_shape() -> ShapeTokens {
    let mut s = ShapeTokens::dark_default();
    apply_common(&mut s);
    // A pure-black shadow barely registers on `#202020`, so dark alphas
    // run higher than light — still below the IntUI dark baseline.
    soften_shadows(&mut s, [0.38, 0.36, 0.42, 0.56], [0.26, 0.28, 0.32, 0.42]);
    s
}

fn apply_common(s: &mut ShapeTokens) {
    s.radius_control = FLUENT_CONTROL_CORNER_RADIUS;
    s.radius_popup = FLUENT_OVERLAY_CORNER_RADIUS;
    s.border_width = 1.0;
    s.focus_ring_width = FLUENT_FOCUS_RING_WIDTH;
    s.focus_ring_offset = FLUENT_FOCUS_RING_OFFSET;
    // `radius_pill` (9999) keeps the baseline — Fluent uses fully-rounded
    // ends for the ToggleSwitch track, pips and the ScrollBar thumb.
}

/// Replace the alpha of each paired outer/inner shadow colour, keeping the
/// baseline offset/blur geometry. `outer` / `inner` are `[xs, sm, md, lg]`.
fn soften_shadows(s: &mut ShapeTokens, outer: [f32; 4], inner: [f32; 4]) {
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
    fn radii_are_the_two_fluent_values() {
        for s in [fluent_light_shape(), fluent_dark_shape()] {
            assert_eq!(s.radius_control, 4.0);
            assert_eq!(s.radius_popup, 8.0);
            assert!(s.radius_popup > s.radius_control);
            assert!(s.radius_pill > s.radius_popup);
        }
    }

    #[test]
    fn focus_ring_is_two_dp_one_dp_off_the_control() {
        for s in [fluent_light_shape(), fluent_dark_shape()] {
            assert_eq!(s.focus_ring_width, 2.0);
            assert_eq!(s.focus_ring_offset, 1.0);
        }
    }

    #[test]
    fn dark_elevation_is_stronger_than_light() {
        // Fluent softens shadows overall, but dark surfaces still need
        // more alpha than light for the same perceived lift.
        let l = fluent_light_shape();
        let d = fluent_dark_shape();
        assert!(d.shadow_lg.color.a() > l.shadow_lg.color.a());
        assert!(d.shadow_inner_lg.color.a() > l.shadow_inner_lg.color.a());
    }

    #[test]
    fn shadows_are_softer_than_the_intui_baseline() {
        let base = teksilo_tokens::ShapeTokens::light_default();
        let f = fluent_light_shape();
        assert!(f.shadow_lg.color.a() < base.shadow_lg.color.a());
        assert!(f.shadow_md.color.a() < base.shadow_md.color.a());
    }

    #[test]
    fn shadow_geometry_is_preserved() {
        // Only the alpha is re-graded; offsets and blurs stay on the
        // baseline so overlay positioning is unchanged.
        let base = teksilo_tokens::ShapeTokens::dark_default();
        let f = fluent_dark_shape();
        assert_eq!(f.shadow_md.blur, base.shadow_md.blur);
        assert_eq!(f.shadow_md.offset_y, base.shadow_md.offset_y);
    }
}
