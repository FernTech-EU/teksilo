// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Layered drop-shadow helper for elevated surfaces.
//!
//! Composes two [`Shadow`]s underneath a rounded rect:
//! - `outer` — the wide soft halo (typically `theme.shape.shadow_*`).
//! - `inner` — the sharp short-blur rim that gives the surface a clearly
//!   "lifted" edge instead of a vague glow (typically the matching
//!   `theme.shape.shadow_inner_*`).
//!
//! The `inner` token's geometry (`offset_y`, `blur`, `color.rgb`) is used
//! verbatim. Only `color.a` is modulated: the painted alpha is
//! `density × inner.color.a()`, with `density ∈ [0.0, 1.0]` provided by
//! the per-component `shadow_density` field. This keeps every visual
//! knob in the theme while letting individual surfaces dial intensity.
//!
//! Common density presets:
//! - `1.0` — tooltips (full inner-rim alpha, punchy "lift").
//! - `~0.5` — cards, popovers, menus (moderate).
//! - `0.0` — disable inner rim entirely (single-layer outer only).
//!
//! ## Attached side
//!
//! Popovers, menus and combo-box dropdowns sit *attached* to the widget
//! that opened them. On the side that touches the trigger, drawing a
//! halo would visually cut the surface off from its anchor. Pass an
//! [`AttachedSide`] to suppress shadow on that side.
//!
//! ```ignore
//! // Typical usage inside a custom widget's paint() method:
//! use teksilo_widgets::shadow::{paint_layered_shadow, DENSITY_SURFACE};
//! paint_layered_shadow(
//!     canvas, bounds, radius,
//!     &ctx.theme.shape.shadow_sm,
//!     &ctx.theme.shape.shadow_inner_sm,
//!     DENSITY_SURFACE,
//!     None,
//! );
//! ```

use teksilo_canvas::{Canvas, Rect};
use teksilo_tokens::{Color, CornerRadius, Shadow};

/// Inner-rim alpha multiplier for tooltips — full intensity for maximum lift.
pub const DENSITY_TOOLTIP: f32 = 1.0;
/// Inner-rim alpha multiplier for cards, popovers, and menus — moderate lift.
pub const DENSITY_SURFACE: f32 = 0.5;
/// Inner-rim alpha multiplier for snackbars and dialogs — subtle lift.
pub const DENSITY_DIALOG: f32 = 0.3;

/// Sub-perceptual alpha cutoff: below this no human eye registers the
/// difference even on a fresh CRT, and the GPU cost is the same as a
/// fully-opaque draw. Used to short-circuit invisible shadow draws.
const SUB_PERCEPTUAL: f32 = 1.0 / 255.0;

/// Which geometric edge of the surface is attached to its trigger and
/// should have shadow drawing suppressed on that side. Geometric (Top
/// / Bottom / Left / Right), not RTL-aware — callers working in
/// Leading/Trailing terms must resolve to a geometric side using the
/// active layout direction before calling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachedSide {
    /// Suppress the shadow halo on the top edge (e.g. a dropdown opening downward).
    Top,
    /// Suppress the shadow halo on the bottom edge (e.g. a popover opening upward).
    Bottom,
    /// Suppress the shadow halo on the left edge.
    Left,
    /// Suppress the shadow halo on the right edge.
    Right,
}

/// Paint a two-layer drop shadow behind a rounded rect.
///
/// The `outer` shadow is drawn unchanged. If `density × inner.color.a()`
/// is above the sub-perceptual threshold (1/255), the `inner` shadow is
/// drawn on top with its alpha scaled by `density`. This gives a "lift"
/// look — a wide soft halo with a sharp close rim.
///
/// When `attached` is `Some(side)`, both shadow draws are clipped so
/// the penumbra on that side is hidden — matching the visual where
/// the surface is attached to its anchor (popover under its trigger,
/// dropdown under its combo box, etc.).
///
/// If both layers would be sub-perceptual (e.g. theme has zero alphas
/// or `density` of 0), this function returns without emitting any draw
/// commands.
///
/// ```ignore
/// // In a widget's paint() method:
/// use teksilo_widgets::shadow::{paint_layered_shadow, AttachedSide, DENSITY_SURFACE};
/// paint_layered_shadow(
///     canvas, bounds, radius,
///     &ctx.theme.shape.shadow_sm, &ctx.theme.shape.shadow_inner_sm,
///     DENSITY_SURFACE, None,
/// );
/// ```
pub fn paint_layered_shadow(
    canvas: &mut Canvas,
    bounds: Rect,
    radius: CornerRadius,
    outer: &Shadow,
    inner: &Shadow,
    density: f32,
    attached: Option<AttachedSide>,
) {
    let density = density.clamp(0.0, 1.0);
    let inner_alpha = density * inner.color.a();
    let outer_visible = outer.color.a() >= SUB_PERCEPTUAL;
    let inner_visible = inner_alpha >= SUB_PERCEPTUAL;
    if !outer_visible && !inner_visible {
        return;
    }

    let clip = attached.map(|s| suppress_clip(bounds, outer, inner, s));
    if let Some(c) = clip {
        canvas.set_clip(c);
    }

    if outer_visible {
        canvas.draw_shadow(bounds, radius, outer);
    }
    if inner_visible {
        let scaled_inner = Shadow {
            color: Color::new(
                inner.color.r(),
                inner.color.g(),
                inner.color.b(),
                inner_alpha.min(1.0),
            ),
            ..*inner
        };
        canvas.draw_shadow(bounds, radius, &scaled_inner);
    }

    if clip.is_some() {
        canvas.clear_clip();
    }
}

/// Build a clip rect that includes everything around `bounds` that
/// shadow could reach EXCEPT the attached side. Each non-attached
/// side uses the directional shadow extent for that axis (a `Top`
/// suppression doesn't need slack on the X axis from `offset_y`, and
/// vice versa), so the scissor stays as tight as possible while still
/// admitting the full penumbra on every drawn side.
fn suppress_clip(bounds: Rect, outer: &Shadow, inner: &Shadow, side: AttachedSide) -> Rect {
    // +1 dp gives the shader's anti-aliased Gaussian falloff a clean
    // edge to fade against; without it the cut can show a 1 px banding
    // line along the suppressed edge.
    let extent_left = max_extent(outer, inner, Direction::Left) + 1.0;
    let extent_right = max_extent(outer, inner, Direction::Right) + 1.0;
    let extent_top = max_extent(outer, inner, Direction::Up) + 1.0;
    let extent_bottom = max_extent(outer, inner, Direction::Down) + 1.0;

    let l = bounds.x - extent_left;
    let r = bounds.x + bounds.width + extent_right;
    let t = bounds.y - extent_top;
    let b = bounds.y + bounds.height + extent_bottom;
    match side {
        AttachedSide::Top => Rect::new(l, bounds.y, r - l, b - bounds.y),
        AttachedSide::Bottom => {
            let bot = bounds.y + bounds.height;
            Rect::new(l, t, r - l, bot - t)
        }
        AttachedSide::Left => Rect::new(bounds.x, t, r - bounds.x, b - t),
        AttachedSide::Right => {
            let right = bounds.x + bounds.width;
            Rect::new(l, t, right - l, b - t)
        }
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// How far a single shadow's penumbra reaches past `bounds` in one
/// direction. The shadow's `offset_*` shifts the quad as a whole, so
/// a positive `offset_y` *reduces* the upward reach and *increases*
/// the downward one (and similarly for X).
fn shadow_extent_in(shadow: &Shadow, dir: Direction) -> f32 {
    let blur_spread = shadow.blur + shadow.spread;
    match dir {
        Direction::Up => (blur_spread - shadow.offset_y).max(0.0),
        Direction::Down => (blur_spread + shadow.offset_y).max(0.0),
        Direction::Left => (blur_spread - shadow.offset_x).max(0.0),
        Direction::Right => (blur_spread + shadow.offset_x).max(0.0),
    }
}

fn max_extent(a: &Shadow, b: &Shadow, dir: Direction) -> f32 {
    shadow_extent_in(a, dir).max(shadow_extent_in(b, dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::{DrawCommand, RenderFrame};

    fn capture_frame<F: FnOnce(&mut Canvas)>(f: F) -> RenderFrame {
        let mut canvas = Canvas::new();
        f(&mut canvas);
        canvas.into_render_frame()
    }

    fn shadow_count(frame: &RenderFrame) -> usize {
        frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, DrawCommand::Shadow(_)))
            .count()
    }

    fn clip_count(frame: &RenderFrame) -> usize {
        frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, DrawCommand::SetClip(_)))
            .count()
    }

    #[test]
    fn full_density_emits_two_shadows_no_clip() {
        let theme = teksilo_core::presets::intui::light();
        let frame = capture_frame(|c| {
            paint_layered_shadow(
                c,
                Rect::new(10.0, 10.0, 100.0, 50.0),
                CornerRadius::uniform(8.0),
                &theme.shape.shadow_xs,
                &theme.shape.shadow_inner_xs,
                1.0,
                None,
            );
        });
        assert_eq!(shadow_count(&frame), 2, "outer + inner expected");
        assert_eq!(clip_count(&frame), 0, "no suppression ⇒ no clip");
    }

    #[test]
    fn zero_density_skips_inner() {
        let theme = teksilo_core::presets::intui::light();
        let frame = capture_frame(|c| {
            paint_layered_shadow(
                c,
                Rect::new(0.0, 0.0, 50.0, 50.0),
                CornerRadius::uniform(4.0),
                &theme.shape.shadow_xs,
                &theme.shape.shadow_inner_xs,
                0.0,
                None,
            );
        });
        assert_eq!(shadow_count(&frame), 1, "only outer drawn at density=0");
    }

    #[test]
    fn attached_side_emits_clip() {
        let theme = teksilo_core::presets::intui::light();
        for side in [
            AttachedSide::Top,
            AttachedSide::Bottom,
            AttachedSide::Left,
            AttachedSide::Right,
        ] {
            let frame = capture_frame(|c| {
                paint_layered_shadow(
                    c,
                    Rect::new(10.0, 10.0, 100.0, 50.0),
                    CornerRadius::uniform(8.0),
                    &theme.shape.shadow_xs,
                    &theme.shape.shadow_inner_xs,
                    1.0,
                    Some(side),
                );
            });
            assert_eq!(clip_count(&frame), 1, "{:?} should emit one SetClip", side);
            assert!(
                frame
                    .draw_order
                    .iter()
                    .any(|c| matches!(c, DrawCommand::ClearClip)),
                "{:?} should emit a matching ClearClip",
                side,
            );
        }
    }

    #[test]
    fn top_clip_excludes_top_penumbra_only() {
        // A shadow with offset=0 reaches `blur` past every side. A Top
        // suppression must keep the body in the clip and exclude the
        // region above it.
        let theme = teksilo_core::presets::intui::light();
        let bounds = Rect::new(10.0, 100.0, 80.0, 40.0);
        let frame = capture_frame(|c| {
            paint_layered_shadow(
                c,
                bounds,
                CornerRadius::uniform(4.0),
                &theme.shape.shadow_xs,
                &theme.shape.shadow_inner_xs,
                1.0,
                Some(AttachedSide::Top),
            );
        });
        let clip = frame
            .draw_order
            .iter()
            .find_map(|c| match c {
                DrawCommand::SetClip(r) => Some(*r),
                _ => None,
            })
            .expect("clip command present");
        assert!(
            (clip.y - bounds.y).abs() < 0.001,
            "Top clip must start at body's top edge, got {:?}",
            clip,
        );
        assert!(
            clip.y + clip.height >= bounds.y + bounds.height,
            "clip must include body bottom"
        );
        assert!(clip.x <= bounds.x, "clip must include body left side");
        assert!(
            clip.x + clip.width >= bounds.x + bounds.width,
            "clip must include body right side"
        );
    }

    #[test]
    fn fully_invisible_shadow_emits_nothing() {
        // Both alphas zero ⇒ no draw commands at all.
        let zero = Shadow {
            color: Color::new(0.0, 0.0, 0.0, 0.0),
            ..Default::default()
        };
        let frame = capture_frame(|c| {
            paint_layered_shadow(
                c,
                Rect::new(0.0, 0.0, 50.0, 50.0),
                CornerRadius::uniform(4.0),
                &zero,
                &zero,
                1.0,
                Some(AttachedSide::Top),
            );
        });
        assert_eq!(shadow_count(&frame), 0);
        assert_eq!(clip_count(&frame), 0, "no clip needed when nothing draws");
    }

    #[test]
    fn directional_extent_respects_offset() {
        // Offset y = +blur + 1: shadow shifts down enough that nothing
        // pokes past the top edge. shadow_extent_in(Up) should clamp
        // to zero. shadow_extent_in(Down) should be 2*blur + 1.
        let s = Shadow {
            blur: 10.0,
            spread: 0.0,
            offset_y: 11.0,
            color: Color::new(0.0, 0.0, 0.0, 0.5),
            ..Default::default()
        };
        assert_eq!(shadow_extent_in(&s, Direction::Up), 0.0);
        assert!((shadow_extent_in(&s, Direction::Down) - 21.0).abs() < 0.001);
        assert!((shadow_extent_in(&s, Direction::Left) - 10.0).abs() < 0.001);
        assert!((shadow_extent_in(&s, Direction::Right) - 10.0).abs() < 0.001);
    }
}
