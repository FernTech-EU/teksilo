// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The hit-targeting suite: the algebra of [`HitSlop`], and the adversarial
//! cases that pin the three mechanisms' eligibility rules against a real arena.

use super::*;

fn tokens(d: TargetDensity) -> InputTokens {
    InputTokens::for_density(d)
}

/// The mouse never earns slop, at any density. This is the invariant the
/// "mouse hit tests are byte-identical" guarantee rests on.
#[test]
fn a_mouse_has_no_slop_at_any_density() {
    for d in [
        TargetDensity::Compact,
        TargetDensity::Comfortable,
        TargetDensity::Touch,
    ] {
        let s = HitSlop::for_pointer(PointerKind::Mouse, &tokens(d));
        assert!(s.is_none(), "{d:?}: mouse slop was {s:?}");
        assert_eq!(s.outset_for(Size::new(4.0, 4.0)), 0.0);
    }
}

/// A coarse pointer's radius is capped by `slop_budget`; a precise one's is
/// not (its profile value already describes the tool).
#[test]
fn the_budget_caps_a_coarse_pointer_only() {
    let mut t = tokens(TargetDensity::Compact);
    t.slop_budget = 3.0;
    // Touch profile radius is 8 dp — the budget wins.
    assert_eq!(HitSlop::for_pointer(PointerKind::Touch, &t).radius, 3.0);
    // Pen profile radius is 2 dp and the budget does not apply, so a budget
    // *below* it leaves it alone.
    assert_eq!(
        HitSlop::for_pointer(PointerKind::Pen(teksilo_tokens::PenKind::Pen), &t).radius,
        2.0
    );
}

/// The Compact budget is deliberately non-zero: a zero there would make the
/// whole mechanism a no-op at the density CI runs at.
#[test]
fn compact_still_affords_a_budget() {
    assert_eq!(tokens(TargetDensity::Compact).slop_budget, 12.0);
    let s = HitSlop::for_pointer(PointerKind::Touch, &tokens(TargetDensity::Compact));
    assert_eq!(s.radius, 8.0, "touch profile 8 dp, under the 12 dp budget");
}

/// A node already at least `up_to` on its smaller axis earns nothing — the
/// arithmetic, not a rule, is what keeps scrims and rows out of the pass.
#[test]
fn a_large_node_earns_no_outset() {
    let s = HitSlop {
        radius: 8.0,
        up_to: 44.0,
    };
    assert_eq!(s.outset_for(Size::new(1000.0, 800.0)), 0.0);
    assert_eq!(s.outset_for(Size::new(44.0, 44.0)), 0.0);
    assert_eq!(s.outset_for(Size::new(1000.0, 44.0)), 0.0);
    // Small on ONE axis is small: a 2 dp divider is topped up.
    assert_eq!(s.outset_for(Size::new(1000.0, 2.0)), 8.0);
}

/// The top-up is half the shortfall, then clamped by the radius.
#[test]
fn the_outset_is_half_the_shortfall_clamped() {
    let s = HitSlop {
        radius: 8.0,
        up_to: 24.0,
    };
    assert_eq!(s.outset_for(Size::new(16.0, 16.0)), 4.0);
    assert_eq!(s.outset_for(Size::new(20.0, 20.0)), 2.0);
    // Shortfall 24 → half is 12 → clamped to the 8 dp radius.
    assert_eq!(s.outset_for(Size::new(0.0, 0.0)), 8.0);
}

/// A `NaN` measurement must not reach `f32::clamp`, which panics on one.
#[test]
fn degenerate_sizes_and_radii_are_inert() {
    let s = HitSlop {
        radius: f32::NAN,
        up_to: 24.0,
    };
    assert_eq!(s.outset_for(Size::new(4.0, 4.0)), 0.0);
    let s = HitSlop {
        radius: 8.0,
        up_to: 24.0,
    };
    assert_eq!(s.outset_for(Size::new(f32::NAN, f32::NAN)), 8.0);
    assert_eq!(s.outset_for(Size::new(-5.0, -5.0)), 8.0);
}

#[test]
fn rect_distance_is_zero_inside_and_euclidean_outside() {
    let r = Rect::new(10.0, 10.0, 20.0, 20.0);
    assert_eq!(rect_distance(r, Point::new(15.0, 15.0)), 0.0);
    assert_eq!(rect_distance(r, Point::new(34.0, 20.0)), 4.0);
    assert_eq!(rect_distance(r, Point::new(20.0, 6.0)), 4.0);
    // Corner: the diagonal, not the axis distance.
    assert!((rect_distance(r, Point::new(33.0, 34.0)) - 5.0).abs() < 1e-4);
}

#[test]
fn circle_distance_follows_the_silhouette() {
    let c = Point::new(0.0, 0.0);
    assert_eq!(circle_distance(c, 10.0, Point::new(3.0, 4.0)), 0.0);
    assert!((circle_distance(c, 10.0, Point::new(13.0, 0.0)) - 3.0).abs() < 1e-4);
    // The corner of the bounding box is further than its edge — the whole
    // reason a round control reports its own distance.
    let corner = circle_distance(c, 10.0, Point::new(10.0, 10.0));
    assert!((corner - (200.0_f32.sqrt() - 10.0)).abs() < 1e-3);
}

#[test]
fn min_singular_value_of_the_usual_transforms() {
    assert!((min_singular_value(&Transform2D::IDENTITY) - 1.0).abs() < 1e-5);
    assert!((min_singular_value(&Transform2D::translate(50.0, -9.0)) - 1.0).abs() < 1e-5);
    assert!((min_singular_value(&Transform2D::rotate(0.7)) - 1.0).abs() < 1e-5);
    assert!((min_singular_value(&Transform2D::scale(2.0, 2.0)) - 2.0).abs() < 1e-5);
    // Anisotropic: the SMALLER axis scale.
    assert!((min_singular_value(&Transform2D::scale(3.0, 0.5)) - 0.5).abs() < 1e-5);
    // Rotation composed with a scale keeps the scale's singular values.
    let t = Transform2D::scale(4.0, 0.25).then(&Transform2D::rotate(1.1));
    assert!((min_singular_value(&t) - 0.25).abs() < 1e-4);
    // Degenerate.
    assert_eq!(min_singular_value(&Transform2D::scale(1.0, 0.0)), 0.0);
}

/// The pointer-less door means "mouse, exact" — and says so.
#[test]
fn the_mouse_context_disables_the_slop_pass() {
    let ctx = HitContext::mouse();
    assert_eq!(ctx.kind(), PointerKind::Mouse);
    assert_eq!(ctx.tokens().density, TargetDensity::Compact);
    assert!(!ctx.slop_enabled());
    assert_eq!(ctx.layout_direction(), LayoutDirection::LeftToRight);
}

#[test]
fn a_touch_context_enables_the_pass_and_carries_its_probe() {
    let t = tokens(TargetDensity::Touch);
    let seven: WidgetId = slotmap::KeyData::from_ffi(7).into();
    let eight: WidgetId = slotmap::KeyData::from_ffi(8).into();
    let probe = |id: WidgetId| id == seven;
    let ctx = HitContext::new(PointerKind::Touch, &t)
        .direction(LayoutDirection::RightToLeft)
        .read_only_probe(&probe);
    assert!(ctx.slop_enabled());
    assert_eq!(ctx.layout_direction(), LayoutDirection::RightToLeft);
    assert!(ctx.is_read_only(seven));
    assert!(!ctx.is_read_only(eight));
}
