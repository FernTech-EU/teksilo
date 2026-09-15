// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_tokens::Color;

use crate::geometry::Point;

/// A paint type for filling shapes.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Color),
    /// Linear gradient between two points in **rect-local** pixel
    /// coordinates: `(0, 0)` is the top-left of the filled rect,
    /// `(width, height)` is the bottom-right. **Not** absolute window
    /// coordinates — passing `bounds.x + bounds.width` would shift
    /// the gradient endpoints away from the rect when the rect is
    /// positioned anywhere other than the origin (or a scrolled
    /// child whose bounds change with the scroll offset), causing
    /// the visible gradient to drift / clip / squash.
    ///
    /// Conventional axis endpoints, for a rect of size `(w, h)`:
    /// - Horizontal left→right: `start = (0, 0), end = (w, 0)`.
    /// - Vertical top→bottom:   `start = (0, 0), end = (0, h)`.
    /// - Diagonal:              `start = (0, 0), end = (w, h)`.
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        center: Point,
        radius: f32,
        stops: Vec<GradientStop>,
    },
    ConicGradient {
        center: Point,
        start_angle: f32,
        stops: Vec<GradientStop>,
    },
    Image(ImageHandle),
}

/// Handle to an image resource.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageHandle {
    pub name: String,
}

/// A single stop in a gradient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

/// Line cap style for strokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Line join style for strokes — how two connected segments meet at a
/// shared vertex. Line-style SVG icons (Feather / Lucide / Tabler) set
/// `stroke-linejoin="round"`; the default [`Miter`](LineJoin::Miter)
/// matches every pre-existing stroke and SVG's own default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Fill rule for a filled path — how self-intersections and nested
/// sub-paths decide inside vs outside. [`Winding`](FillRule::Winding)
/// (non-zero) is the default; [`EvenOdd`](FillRule::EvenOdd) is SVG's
/// `fill-rule="evenodd"` (rings / donuts / counters authored with holes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillRule {
    #[default]
    Winding,
    EvenOdd,
}

/// Whether a stroke's width is interpreted in logical pixels (and so scales
/// with the canvas transform) or held transform-invariant ("cosmetic").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StrokeSpace {
    /// Width is in logical pixels and scales with the canvas transform
    /// (zoom / scale / rotation). The default; matches every pre-existing
    /// stroke.
    #[default]
    Logical,
    /// "Cosmetic" stroke: the width is held constant regardless of the
    /// canvas transform (it does not grow with a `SceneView` zoom), while
    /// still respecting the display scale factor (HiDPI). The stroke's
    /// *position* still follows the full transform; only its *thickness* is
    /// invariant. Same intent as Qt's cosmetic pen / a CSS 1px border.
    Device,
}

/// Stroke style configuration, supporting dashed/dotted strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    pub width: f32,
    /// Alternating dash/gap lengths. `None` means solid stroke.
    pub dash_pattern: Option<Vec<f32>>,
    /// Phase offset for the dash pattern.
    pub dash_offset: f32,
    pub line_cap: LineCap,
    /// How connected segments meet at a vertex. Defaults to
    /// [`LineJoin::Miter`] (SVG's default and every pre-existing stroke).
    pub line_join: LineJoin,
    /// Miter length / stroke width ratio beyond which a miter join falls
    /// back to bevel. Defaults to `4.0` (SVG's and tiny-skia's default).
    pub miter_limit: f32,
    /// Whether `width` scales with the canvas transform
    /// ([`StrokeSpace::Logical`], default) or is held transform-invariant
    /// ([`StrokeSpace::Device`], a cosmetic / hairline stroke). See
    /// [`StrokeStyle::hairline`].
    pub space: StrokeSpace,
}

impl StrokeStyle {
    pub fn solid(width: f32) -> Self {
        Self {
            width,
            dash_pattern: None,
            dash_offset: 0.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 4.0,
            space: StrokeSpace::Logical,
        }
    }

    /// A dashed stroke: `dash` logical pixels of ink, then `gap` of nothing,
    /// repeating along the path.
    ///
    /// Only the Tier-3 (CPU-rasterized) path pipeline can express a dash, so
    /// [`Canvas`](crate::Canvas) re-expresses a dashed rect / rounded rect /
    /// circle / ellipse / line as a path rather than emitting the cheap
    /// primitive that would drop the pattern. Dashing therefore costs a
    /// rasterized path wherever it is used — not a reason to avoid it, but
    /// the reason `StrokeStyle::solid` stays on the fast route.
    pub fn dashed(width: f32, dash: f32, gap: f32) -> Self {
        Self {
            width,
            dash_pattern: Some(vec![dash, gap]),
            dash_offset: 0.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 4.0,
            space: StrokeSpace::Logical,
        }
    }

    /// A dotted stroke: round dots of `width` diameter, `spacing` apart.
    /// Carries the same Tier-3 cost as [`dashed`](Self::dashed) — and its
    /// round cap is likewise only expressible there.
    pub fn dotted(width: f32, spacing: f32) -> Self {
        Self {
            width,
            dash_pattern: Some(vec![width, spacing]),
            dash_offset: 0.0,
            line_cap: LineCap::Round,
            line_join: LineJoin::Miter,
            miter_limit: 4.0,
            space: StrokeSpace::Logical,
        }
    }

    /// A cosmetic / hairline stroke: `width` logical pixels of thickness held
    /// **invariant to the canvas transform** (it does not grow with a
    /// `SceneView` zoom), still scaled for HiDPI. Use for grid lines, 1px
    /// borders, focus rings, and connectors that must stay crisp at any zoom.
    ///
    /// Honored by every stroke method: [`Canvas::draw_line`](crate::Canvas::draw_line)
    /// / [`Canvas::stroke_rect`](crate::Canvas::stroke_rect) (exact, hard-edged
    /// quad); [`Canvas::stroke_rounded_rect`](crate::Canvas::stroke_rounded_rect)
    /// / `stroke_circle` / `stroke_ellipse` (SDF — constant device-pixel
    /// width); and [`Canvas::stroke_path`](crate::Canvas::stroke_path)
    /// (zoom-aware atlas).
    pub fn hairline(width: f32) -> Self {
        Self {
            width,
            dash_pattern: None,
            dash_offset: 0.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 4.0,
            space: StrokeSpace::Device,
        }
    }

    /// Whether this style will actually *break* the stroke into dashes.
    ///
    /// The rule is SVG's `stroke-dasharray`, which is also exactly what the
    /// renderer's rasterizer (`tiny_skia::StrokeDash`) accepts: an even
    /// count of at least two lengths, none negative, summing to something
    /// positive. A pattern that fails any of those renders solid.
    ///
    /// This is the predicate to ask — **not** `dash_pattern.is_some()`.
    /// [`Canvas`](crate::Canvas) routes a dashing stroke away from the
    /// Tier-1 / Tier-2 primitives that cannot express one (see
    /// [`Canvas::stroke_rect`](crate::Canvas::stroke_rect)), and routing on
    /// the weaker test would send a would-be-solid pattern down the slow
    /// CPU-rasterized path for no visible difference.
    pub fn is_dashed(&self) -> bool {
        let Some(pattern) = self.dash_pattern.as_ref() else {
            return false;
        };
        pattern.len() >= 2
            && pattern.len().is_multiple_of(2)
            && self.dash_offset.is_finite()
            && pattern.iter().all(|d| d.is_finite() && *d >= 0.0)
            && pattern.iter().sum::<f32>() > 0.0
    }
}

impl Default for StrokeStyle {
    // Delegates to `solid(0.0)` so `miter_limit` defaults to 4.0 (a derived
    // Default would zero it, making every miter join bevel).
    fn default() -> Self {
        Self::solid(0.0)
    }
}

impl From<f32> for StrokeStyle {
    fn from(width: f32) -> Self {
        Self::solid(width)
    }
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Paint::Solid(color)
    }
}
