// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Material 3 shape tokens.
//!
//! M3's corner-radius scale is xs 4 / sm 8 / md 12 / lg 16 / xl 28 dp.
//! Teksilo's `ShapeTokens` only carries three radii: `radius_control`
//! (small components — fields, checkboxes, combos), `radius_popup`
//! (menus / dialogs / panels) and `radius_pill`. We keep
//! `radius_control` at M3's xs (4 dp) and raise `radius_popup` to M3's
//! md (12 dp). The distinctive M3 component shapes (the 40 dp pill
//! button, the 12 dp card) are applied by the per-widget styles, not by
//! these shared radii.
//!
//! M3 expresses elevation primarily through *tonal* surface tints (the
//! surface-container ladder, handled in [`crate::color`]) rather than
//! heavy drop shadows, so the shadows here are softened well below the
//! IntUI baseline.

use teksilo_tokens::ShapeTokens;

/// M3 focus indicator width (dp). M3 uses a 3 dp focus ring.
const M3_FOCUS_RING_WIDTH: f32 = 3.0;
/// M3 medium corner radius (dp) — menus, dialogs, panels.
const M3_RADIUS_POPUP: f32 = 12.0;

/// Material 3 **light** shape tokens.
pub fn m3_light_shape() -> ShapeTokens {
    let mut s = ShapeTokens::light_default();
    apply_common(&mut s);
    // M3 light elevation: subtle. Outer xs/sm/md/lg, then inner rims.
    soften_shadows(&mut s, [0.16, 0.10, 0.14, 0.20], [0.12, 0.08, 0.10, 0.15]);
    s
}

/// Material 3 **dark** shape tokens.
pub fn m3_dark_shape() -> ShapeTokens {
    let mut s = ShapeTokens::dark_default();
    apply_common(&mut s);
    // Pure-black shadows read weakly on dark surfaces, so dark alphas are
    // higher than light — but still below the IntUI dark baseline.
    soften_shadows(&mut s, [0.30, 0.30, 0.35, 0.50], [0.20, 0.22, 0.25, 0.35]);
    s
}

fn apply_common(s: &mut ShapeTokens) {
    s.radius_popup = M3_RADIUS_POPUP;
    s.focus_ring_width = M3_FOCUS_RING_WIDTH;
    // radius_control (4 dp), radius_pill (9999), border_width (1 dp),
    // focus_ring_offset (2 dp) keep the baseline values.
}

/// Replace the alpha of each paired outer/inner shadow color, keeping the
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
