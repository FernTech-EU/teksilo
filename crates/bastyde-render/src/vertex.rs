// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use bytemuck::{Pod, Zeroable};

use bastyde_canvas::render_frame::PaintData;
use bastyde_canvas::{DecorationRect, GlyphQuad, ShadowQuad, ShapeQuad, Transform2D};

/// Convert a single sRGB channel (0..1) to linear light (0..1).
///
/// `bastyde_tokens::Color::from_hex` parses hex values as sRGB-encoded f32
/// without gamma conversion, which matches how designers specify colors.
/// The wgpu surface is `Rgba8UnormSrgb`, which expects **linear** shader
/// output and applies sRGB encoding on write. To avoid double gamma-
/// encoding we linearize color data at the vertex-packing boundary.
#[inline]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linearize an RGBA color for GPU upload. Alpha passes through unchanged
/// because `Rgba8UnormSrgb` only gamma-encodes RGB.
#[inline]
pub fn srgb_to_linear_rgba(c: [f32; 4]) -> [f32; 4] {
    [
        srgb_to_linear(c[0]),
        srgb_to_linear(c[1]),
        srgb_to_linear(c[2]),
        c[3],
    ]
}

/// Per-vertex flag: sample atlas `texture.rgb` directly (color emoji)
/// instead of using the texture as an alpha mask tinted by vertex color.
pub const QUAD_FLAG_COLOR_GLYPH: u32 = 1;

/// Vertex for the textured quad pipeline (glyphs, images).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuadVertex {
    pub position: [f32; 2],
    pub tex_coord: [f32; 2],
    pub color: [f32; 4],
    /// Per-vertex bitfield. Bit 0 ([`QUAD_FLAG_COLOR_GLYPH`]) selects the
    /// color emoji path in the fragment shader.
    pub flags: u32,
    /// Padding so the struct stride stays a multiple of 8 bytes, matching
    /// wgpu's vertex buffer layout expectations.
    pub _pad: u32,
}

/// Off-diagonal tolerance below which a transform counts as axis-aligned
/// for glyph pixel-snapping. Matrix composition leaves float dust on `b`/`c`;
/// any intentional rotation puts `|sin θ|` there, far above this bound.
const GLYPH_SNAP_AXIS_EPS: f32 = 1e-4;

/// Tolerance (in physical pixels) between the transformed glyph quad size
/// and its atlas bitmap size for the quad to count as 1:1. Tight enough
/// that residual mid-bucket zoom scaling (≥1% on any glyph bigger than
/// ~6 px) never snaps, loose enough to absorb the float error of the
/// logical→physical round-trip (`bitmap / (sf·rs) · sf · scale`).
const GLYPH_SNAP_SIZE_EPS: f32 = 1.0 / 16.0;

/// Apply a 2D affine transform to a physical-pixel position.
/// Same convention as the renderer: `m = [a, b, c, d, tx, ty]` maps
/// `(x, y) → (a·x + c·y + tx, b·x + d·y + ty)`.
#[inline]
fn apply_affine(p: [f32; 2], t: &Transform2D) -> [f32; 2] {
    let [a, b, c, d, tx, ty] = t.m;
    [a * p[0] + c * p[1] + tx, b * p[0] + d * p[1] + ty]
}

impl QuadVertex {
    /// Convert a glyph quad to 4 vertices (two triangles via index buffer),
    /// applying `scale_factor` (logical → physical pixels) and the current
    /// affine `transform` (linear part unitless, translation already in
    /// physical pixels). Atlas coordinates are in texels and normalized to
    /// 0..1 using the atlas dimensions. Returned positions are in physical
    /// pixels — the caller converts to NDC; it must NOT transform again.
    ///
    /// **Pixel-snap invariant.** The glyph atlas is sampled with bilinear
    /// filtering so the residual GPU scaling between raster-scale buckets
    /// (≤ ~12%, see `quantize_raster_scale`) stays smooth. Bilinear is only
    /// loss-free when each texel maps exactly onto one framebuffer pixel,
    /// and glyph origins are inherently fractional: shaping pen advances,
    /// widget offsets, and scroll positions are all fractional floats. An
    /// unsnapped origin makes the 2×2 kernel mix neighboring texels at
    /// every edge (uniform blur) and bleed the glyph's last row into the
    /// transparent atlas gutter (visibly cropping the bottom of letters
    /// like "c"/"e"). So whenever the transformed quad maps 1:1 onto its
    /// atlas bitmap — axis-aligned transform and transformed size equal to
    /// the bitmap size within [`GLYPH_SNAP_SIZE_EPS`] — the origin is
    /// rounded to the integer pixel grid and the opposite corner pinned at
    /// exactly `origin + bitmap size`, making linear sampling an identity.
    /// This covers identity, fractional DPI, pure translations, and
    /// exact-bucket zoom (e.g. a 1.25× transform over a 1.25-bucket
    /// raster). Rotated or residually scaled quads (mid-bucket zoom) skip
    /// the snap and ride bilinear filtering as intended.
    pub fn from_glyph_quad_transformed(
        quad: &GlyphQuad,
        scale_factor: f32,
        atlas_width: u32,
        atlas_height: u32,
        transform: &Transform2D,
    ) -> [QuadVertex; 4] {
        let [x, y, w, h] = quad.screen;
        let [ax, ay, aw, ah] = quad.atlas;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        // Normalize atlas pixel coords to 0..1 UVs
        let aw_f = atlas_width.max(1) as f32;
        let ah_f = atlas_height.max(1) as f32;
        let u0 = ax / aw_f;
        let v0 = ay / ah_f;
        let u1 = (ax + aw) / aw_f;
        let v1 = (ay + ah) / ah_f;

        let [a, b, c, d, _, _] = transform.m;
        let axis_aligned = b.abs() < GLYPH_SNAP_AXIS_EPS && c.abs() < GLYPH_SNAP_AXIS_EPS;
        let one_to_one = axis_aligned
            && (a * sw - aw).abs() < GLYPH_SNAP_SIZE_EPS
            && (d * sh - ah).abs() < GLYPH_SNAP_SIZE_EPS;
        let positions: [[f32; 2]; 4] = if one_to_one {
            let [ox, oy] = apply_affine([sx, sy], transform);
            let ox = ox.round();
            let oy = oy.round();
            [[ox, oy], [ox + aw, oy], [ox + aw, oy + ah], [ox, oy + ah]]
        } else {
            [
                apply_affine([sx, sy], transform),
                apply_affine([sx + sw, sy], transform),
                apply_affine([sx + sw, sy + sh], transform),
                apply_affine([sx, sy + sh], transform),
            ]
        };

        // Color emoji glyphs carry their RGB in the atlas bitmap. Mark
        // them with a per-vertex flag so the fragment shader can sample
        // `texture.rgb` directly instead of applying the alpha-mask path.
        //
        // The upstream color for color emoji is already `[1, 1, 1, 1]`
        // (see text-typeset's `rasterize_glyph_quad`); srgb_to_linear
        // leaves that unchanged, so the cached value still multiplies
        // cleanly against the sampled RGB as an opacity factor.
        let flags = if quad.is_color {
            QUAD_FLAG_COLOR_GLYPH
        } else {
            0
        };
        let color = srgb_to_linear_rgba(quad.color);

        [
            QuadVertex {
                position: positions[0],
                tex_coord: [u0, v0],
                color,
                flags,
                _pad: 0,
            },
            QuadVertex {
                position: positions[1],
                tex_coord: [u1, v0],
                color,
                flags,
                _pad: 0,
            },
            QuadVertex {
                position: positions[2],
                tex_coord: [u1, v1],
                color,
                flags,
                _pad: 0,
            },
            QuadVertex {
                position: positions[3],
                tex_coord: [u0, v1],
                color,
                flags,
                _pad: 0,
            },
        ]
    }
}

/// Vertex for the colored rectangle pipeline (decorations).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RectVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl RectVertex {
    /// Convert a decoration rect to 4 vertices.
    pub fn from_decoration(rect: &DecorationRect, scale_factor: f32) -> [RectVertex; 4] {
        let [x, y, w, h] = rect.rect;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        [
            RectVertex {
                position: [sx, sy],
                color: srgb_to_linear_rgba(rect.color),
            },
            RectVertex {
                position: [sx + sw, sy],
                color: srgb_to_linear_rgba(rect.color),
            },
            RectVertex {
                position: [sx + sw, sy + sh],
                color: srgb_to_linear_rgba(rect.color),
            },
            RectVertex {
                position: [sx, sy + sh],
                color: srgb_to_linear_rgba(rect.color),
            },
        ]
    }
}

/// Vertex for the SDF shape pipeline (rounded rects, circles).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SdfVertex {
    pub position: [f32; 2],
    /// Local UV coordinates (0..1) within the shape bounds.
    pub local_uv: [f32; 2],
    pub color: [f32; 4],
    pub corner_radii: [f32; 4],
    /// Shape bounds in pixels: [width, height, stroke_width, paint_type].
    /// paint_type: 0=solid, 1=linear, 2=radial, 3=conic
    pub shape_params: [f32; 4],
    /// Gradient geometry: [start_x, start_y, end_x, end_y] (or center/radius for radial)
    pub gradient_geo: [f32; 4],
    /// Gradient stop 0: [r, g, b, a]
    pub gradient_color0: [f32; 4],
    /// Gradient stop 1: [r, g, b, a]
    pub gradient_color1: [f32; 4],
    /// Gradient stop 2: [r, g, b, a]
    pub gradient_color2: [f32; 4],
    /// Gradient stop 3: [r, g, b, a]
    pub gradient_color3: [f32; 4],
    /// Gradient stop offsets: [offset0, offset1, offset2, offset3]
    pub gradient_offsets: [f32; 4],
}

impl SdfVertex {
    /// Convert a shape quad to 4 vertices with a **logical** stroke width —
    /// the border scales with the view transform (the default).
    ///
    /// The rasterized quad is expanded outward by `stroke_width / 2 + 1` on
    /// every side. The SDF shader paints strokes **centered** on the rect
    /// edge, so the outer half of the stroke falls outside the shape's
    /// bounds — if the quad isn't padded, those fragments are never
    /// rasterized and the stroke is visibly truncated by 1 dp on every
    /// side (most noticeable on focus rings). `local_uv` is extrapolated
    /// past `[0, 1]` for the padding fragments; the SDF still clips
    /// correctly because `sd_rounded_rect` returns positive distances
    /// outside the shape.
    pub fn from_shape_quad(shape: &ShapeQuad, scale_factor: f32) -> [SdfVertex; 4] {
        Self::shape_quad_verts(shape, scale_factor, shape.stroke_width * scale_factor)
    }

    /// Convert a shape quad to 4 vertices with a **cosmetic** stroke: the
    /// border width is held constant in device pixels (`width × scale_factor`)
    /// regardless of `zoom`, while the shape body still scales with the
    /// transform. `zoom` is the uniform scale of the active view transform
    /// (`hypot(m[0], m[1])`).
    ///
    /// The shader measures the SDF in `shape_params.xy = [w·sf, h·sf]` units,
    /// and one such unit maps to `zoom` device px on screen, so baking the
    /// stroke param as `width·sf / zoom` lands the rendered border at exactly
    /// `width·sf` device px at every zoom. Assumes a uniform (non-anisotropic)
    /// transform — the scene zoom is uniform and rotation preserves the column
    /// norm.
    pub fn from_shape_quad_cosmetic(
        shape: &ShapeQuad,
        scale_factor: f32,
        zoom: f32,
    ) -> [SdfVertex; 4] {
        let zoom = zoom.max(1e-3);
        Self::shape_quad_verts(
            shape,
            scale_factor,
            shape.stroke_width * scale_factor / zoom,
        )
    }

    /// Shared core for [`from_shape_quad`](Self::from_shape_quad) and
    /// [`from_shape_quad_cosmetic`](Self::from_shape_quad_cosmetic). Bakes
    /// `stroke_px` (physical device pixels) as the SDF stroke param and
    /// derives the rasterization pad from it.
    fn shape_quad_verts(shape: &ShapeQuad, scale_factor: f32, stroke_px: f32) -> [SdfVertex; 4] {
        let [x, y, w, h] = shape.screen;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        // Encode paint type and gradient data
        let (paint_type, gradient_geo, colors, offsets) =
            encode_paint_data(&shape.paint_data, w, h);

        let stroke = stroke_px;
        // Rasterization padding: enough to contain the outer half of the
        // centered stroke plus a 1 px anti-aliasing margin. Filled shapes
        // (stroke = 0) still get the AA margin so their edges don't clip.
        let pad = stroke * 0.5 + 1.0;
        let u_pad = if sw > 0.0 { pad / sw } else { 0.0 };
        let v_pad = if sh > 0.0 { pad / sh } else { 0.0 };

        let params = [sw, sh, stroke, paint_type as f32];
        // Corner radii are authored in logical px but the shader compares
        // them against `shape_params.xy`, which is in physical px after the
        // scale_factor multiply above. Without scaling here, a 19×19
        // logical circle (radius 9.5) renders as a 38×38 physical rect
        // with 9.5 px corners on Retina — a rounded square instead of a
        // circle.
        let scaled_corner_radii = [
            shape.corner_radii[0] * scale_factor,
            shape.corner_radii[1] * scale_factor,
            shape.corner_radii[2] * scale_factor,
            shape.corner_radii[3] * scale_factor,
        ];

        let base = SdfVertex {
            position: [0.0, 0.0],
            local_uv: [0.0, 0.0],
            color: srgb_to_linear_rgba(shape.color),
            corner_radii: scaled_corner_radii,
            shape_params: params,
            gradient_geo,
            gradient_color0: srgb_to_linear_rgba(colors[0]),
            gradient_color1: srgb_to_linear_rgba(colors[1]),
            gradient_color2: srgb_to_linear_rgba(colors[2]),
            gradient_color3: srgb_to_linear_rgba(colors[3]),
            gradient_offsets: offsets,
        };

        [
            SdfVertex {
                position: [sx - pad, sy - pad],
                local_uv: [-u_pad, -v_pad],
                ..base
            },
            SdfVertex {
                position: [sx + sw + pad, sy - pad],
                local_uv: [1.0 + u_pad, -v_pad],
                ..base
            },
            SdfVertex {
                position: [sx + sw + pad, sy + sh + pad],
                local_uv: [1.0 + u_pad, 1.0 + v_pad],
                ..base
            },
            SdfVertex {
                position: [sx - pad, sy + sh + pad],
                local_uv: [-u_pad, 1.0 + v_pad],
                ..base
            },
        ]
    }
}

/// Encode PaintData into vertex-friendly arrays.
/// Returns (paint_type, gradient_geo, [4 colors], [4 offsets]).
fn encode_paint_data(
    paint_data: &PaintData,
    width: f32,
    height: f32,
) -> (u32, [f32; 4], [[f32; 4]; 4], [f32; 4]) {
    let zero_colors = [[0.0; 4]; 4];
    let zero_offsets = [0.0; 4];

    match paint_data {
        PaintData::Solid => (0, [0.0; 4], zero_colors, zero_offsets),
        PaintData::LinearGradient { start, end, stops } => {
            // Normalize coordinates to 0..1 UV space
            let geo = [
                start[0] / width,
                start[1] / height,
                end[0] / width,
                end[1] / height,
            ];
            let (colors, offsets) = encode_stops(stops);
            (1, geo, colors, offsets)
        }
        PaintData::RadialGradient {
            center,
            radius,
            stops,
        } => {
            // Normalize center and radius to UV space, accounting for aspect ratio.
            // The shader computes distance in UV space where both axes span 0..1,
            // so we normalize the radius relative to width (x-axis) and let the
            // shader use aspect-corrected distance.
            let aspect = height / width.max(0.0001);
            let geo = [
                center[0] / width,
                center[1] / height,
                *radius / width,
                aspect,
            ];
            let (colors, offsets) = encode_stops(stops);
            (2, geo, colors, offsets)
        }
        PaintData::ConicGradient {
            center,
            start_angle,
            stops,
        } => {
            let geo = [center[0] / width, center[1] / height, *start_angle, 0.0];
            let (colors, offsets) = encode_stops(stops);
            (3, geo, colors, offsets)
        }
    }
}

/// Encode up to 4 gradient stops into arrays.
fn encode_stops(stops: &[bastyde_canvas::GradientStop]) -> ([[f32; 4]; 4], [f32; 4]) {
    let mut colors = [[0.0f32; 4]; 4];
    let mut offsets = [0.0f32; 4];
    for (i, stop) in stops.iter().take(4).enumerate() {
        colors[i] = stop.color.to_array();
        offsets[i] = stop.offset;
    }
    // If fewer than 4 stops, repeat last to fill
    if !stops.is_empty() {
        let last_idx = stops.len().min(4) - 1;
        for i in stops.len()..4 {
            colors[i] = colors[last_idx];
            offsets[i] = offsets[last_idx];
        }
    }
    (colors, offsets)
}

/// Standard quad indices for two triangles from 4 vertices.
pub const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// Generate indices for N quads.
pub fn generate_quad_indices(count: usize) -> Vec<u16> {
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count {
        let base = (i * 4) as u16;
        for &offset in &QUAD_INDICES {
            indices.push(base + offset);
        }
    }
    indices
}

/// Vertex for the shadow pipeline (box shadows with Gaussian blur).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ShadowVertex {
    pub position: [f32; 2],
    /// Local UV coordinates (0..1) within the shadow quad bounds.
    pub local_uv: [f32; 2],
    pub shadow_color: [f32; 4],
    pub corner_radii: [f32; 4],
    /// [shape_width, shape_height, blur_radius, spread].
    pub shadow_params: [f32; 4],
    /// [offset_x, offset_y, 0, 0] — offset of inner shape center within shadow quad.
    pub shape_offset: [f32; 4],
}

impl ShadowVertex {
    /// Convert a shadow quad to 4 vertices.
    pub fn from_shadow_quad(shadow: &ShadowQuad, scale_factor: f32) -> [ShadowVertex; 4] {
        let [x, y, w, h] = shadow.screen;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        let [sr_x, sr_y, sr_w, sr_h] = shadow.shape_rect;
        let shape_w = sr_w * scale_factor;
        let shape_h = sr_h * scale_factor;

        // Offset of shape center relative to shadow quad center
        let shadow_cx = sx + sw * 0.5;
        let shadow_cy = sy + sh * 0.5;
        let shape_cx = (sr_x + sr_w * 0.5) * scale_factor;
        let shape_cy = (sr_y + sr_h * 0.5) * scale_factor;
        let offset_x = shape_cx - shadow_cx;
        let offset_y = shape_cy - shadow_cy;

        let params = [
            shape_w,
            shape_h,
            shadow.blur_radius * scale_factor,
            shadow.spread * scale_factor,
        ];
        let offset = [offset_x, offset_y, 0.0, 0.0];
        // Match the SDF pipeline: shadow_params.xy is in physical px after
        // scale_factor, so the matching corner radii also have to be in
        // physical px. Otherwise circular/pill shadow shapes degenerate
        // into rounded squares on Retina.
        let scaled_corner_radii = [
            shadow.corner_radii[0] * scale_factor,
            shadow.corner_radii[1] * scale_factor,
            shadow.corner_radii[2] * scale_factor,
            shadow.corner_radii[3] * scale_factor,
        ];

        [
            ShadowVertex {
                position: [sx, sy],
                local_uv: [0.0, 0.0],
                shadow_color: srgb_to_linear_rgba(shadow.color),
                corner_radii: scaled_corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
            ShadowVertex {
                position: [sx + sw, sy],
                local_uv: [1.0, 0.0],
                shadow_color: srgb_to_linear_rgba(shadow.color),
                corner_radii: scaled_corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
            ShadowVertex {
                position: [sx + sw, sy + sh],
                local_uv: [1.0, 1.0],
                shadow_color: srgb_to_linear_rgba(shadow.color),
                corner_radii: scaled_corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
            ShadowVertex {
                position: [sx, sy + sh],
                local_uv: [0.0, 1.0],
                shadow_color: srgb_to_linear_rgba(shadow.color),
                corner_radii: scaled_corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
        ]
    }
}

/// Vertex for the shader-driven animated-quad pipeline (procedural
/// and sprite kinds). All four vertices of a quad carry the same
/// `slot`, which the fragment shader uses to look up per-frame state
/// (phase, resolved colors, atlas dims) in the `anim_uniforms` buffer.
/// No color or timing is baked in the vertex — that's the whole point:
/// rebuilding the vertex batch is unnecessary when only the phase
/// changes, so the widget's `paint()` doesn't re-run per frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AnimQuadVertex {
    /// Pixel position; converted to NDC in the render loop.
    pub position: [f32; 2],
    /// Local UV within the quad (0..1 across each axis). The fragment
    /// shader uses `uv.x` to decide sweep inclusion; the sprite shader
    /// combines it with `AnimParams::atlas_cols`/`atlas_rows` to sample
    /// the atlas cell.
    pub uv: [f32; 2],
    /// Index into the renderer's `AnimParams` uniform array. Same for
    /// all four vertices of a quad; declared `@interpolate(flat)` in
    /// WGSL to preserve the integer across rasterization.
    pub slot: u32,
    /// Struct padding to keep stride a multiple of 8 bytes (matches
    /// `QuadVertex` convention for wgpu vertex-buffer layouts).
    pub _pad: u32,
}

impl AnimQuadVertex {
    pub fn from_animated_quad(
        draw: &bastyde_canvas::AnimatedQuadDraw,
        scale_factor: f32,
    ) -> [AnimQuadVertex; 4] {
        let [x, y, w, h] = draw.screen;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;
        [
            AnimQuadVertex {
                position: [sx, sy],
                uv: [0.0, 0.0],
                slot: draw.slot,
                _pad: 0,
            },
            AnimQuadVertex {
                position: [sx + sw, sy],
                uv: [1.0, 0.0],
                slot: draw.slot,
                _pad: 0,
            },
            AnimQuadVertex {
                position: [sx + sw, sy + sh],
                uv: [1.0, 1.0],
                slot: draw.slot,
                _pad: 0,
            },
            AnimQuadVertex {
                position: [sx, sy + sh],
                uv: [0.0, 1.0],
                slot: draw.slot,
                _pad: 0,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::{DecorationKind, GradientStop, PaintData, ShapeKind, StrokeSpace};
    use bastyde_tokens::Color;

    /// Build a glyph quad with the given screen rect and atlas rect.
    fn glyph(screen: [f32; 4], atlas: [f32; 4], is_color: bool) -> GlyphQuad {
        GlyphQuad {
            screen,
            atlas,
            color: [1.0, 1.0, 1.0, 1.0],
            is_color,
        }
    }

    fn assert_pos_near(actual: [f32; 2], expected: [f32; 2]) {
        assert!(
            (actual[0] - expected[0]).abs() < 1e-3 && (actual[1] - expected[1]).abs() < 1e-3,
            "position {actual:?} != expected {expected:?}"
        );
    }

    #[test]
    fn glyph_quad_to_vertices() {
        // Quad size (30×40) ≠ atlas size (64×64) → no snap; identity
        // transform passes positions through unchanged.
        let quad = glyph([10.0, 20.0, 30.0, 40.0], [0.0, 0.0, 64.0, 64.0], false);
        let verts = QuadVertex::from_glyph_quad_transformed(
            &quad,
            1.0,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        assert_eq!(verts.len(), 4);
        assert_eq!(verts[0].position, [10.0, 20.0]);
        assert_eq!(verts[1].position, [40.0, 20.0]); // x + w
        assert_eq!(verts[2].position, [40.0, 60.0]); // x + w, y + h
        // Atlas coords normalized: 64/256 = 0.25
        assert_eq!(verts[0].tex_coord, [0.0, 0.0]);
        assert_eq!(verts[2].tex_coord, [0.25, 0.25]);
    }

    #[test]
    fn scale_factor_applied_to_glyph_coords() {
        // Physical size 60×80 ≠ atlas 128×128 → no snap.
        let quad = glyph([10.0, 20.0, 30.0, 40.0], [0.0, 0.0, 128.0, 128.0], false);
        let verts = QuadVertex::from_glyph_quad_transformed(
            &quad,
            2.0,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        assert_eq!(verts[0].position, [20.0, 40.0]);
        assert_eq!(verts[1].position, [80.0, 40.0]);
    }

    #[test]
    fn glyph_snap_identity_fractional_origin() {
        // 1:1 quad (30×40 == atlas 30×40) at a fractional origin: the
        // origin rounds to the pixel grid and the far corner is pinned at
        // exactly origin + bitmap size.
        let quad = glyph([10.3, 20.7, 30.0, 40.0], [0.0, 0.0, 30.0, 40.0], false);
        let verts = QuadVertex::from_glyph_quad_transformed(
            &quad,
            1.0,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        assert_eq!(verts[0].position, [10.0, 21.0]);
        assert_eq!(verts[1].position, [40.0, 21.0]);
        assert_eq!(verts[2].position, [40.0, 61.0]);
        assert_eq!(verts[3].position, [10.0, 61.0]);
    }

    #[test]
    fn glyph_snap_hidpi_scale_factor() {
        // sf=2: logical 16×16 → physical 32×32 == atlas bitmap. Fractional
        // logical origin (5.7, 8.3) → physical (11.4, 16.6) → snaps to
        // (11, 17).
        let quad = glyph([5.7, 8.3, 16.0, 16.0], [0.0, 0.0, 32.0, 32.0], false);
        let verts = QuadVertex::from_glyph_quad_transformed(
            &quad,
            2.0,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        assert_eq!(verts[0].position, [11.0, 17.0]);
        assert_eq!(verts[2].position, [43.0, 49.0]);
    }

    #[test]
    fn glyph_snap_fractional_dpi() {
        // sf=1.25 (Linux fractional scaling): logical 20×20 → physical
        // 25×25 == atlas bitmap. Origin (4.2, 7.8) → (5.25, 9.75) →
        // snaps to (5, 10).
        let quad = glyph([4.2, 7.8, 20.0, 20.0], [0.0, 0.0, 25.0, 25.0], false);
        let verts = QuadVertex::from_glyph_quad_transformed(
            &quad,
            1.25,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        assert_eq!(verts[0].position, [5.0, 10.0]);
        assert_eq!(verts[2].position, [30.0, 35.0]);
    }

    #[test]
    fn glyph_snap_exact_bucket_zoom() {
        // A 1.25× zoom transform over a raster_scale=1.25 bucket: the
        // bitmap is 1.25× denser (atlas 50×50 for a 20×20-logical glyph at
        // sf=2 → pre-transform physical 40×40), so the transformed size
        // (1.25·40 = 50) matches the bitmap exactly → snap fires even
        // under zoom. Fractional translation rounds away.
        let quad = glyph([4.0, 8.0, 20.0, 20.0], [0.0, 0.0, 50.0, 50.0], false);
        let zoom = Transform2D {
            m: [1.25, 0.0, 0.0, 1.25, 3.3, 7.8],
        };
        let verts = QuadVertex::from_glyph_quad_transformed(&quad, 2.0, 256, 256, &zoom);
        // origin: (1.25·8 + 3.3, 1.25·16 + 7.8) = (13.3, 27.8) → (13, 28)
        assert_eq!(verts[0].position, [13.0, 28.0]);
        assert_eq!(verts[2].position, [63.0, 78.0]);
    }

    #[test]
    fn glyph_no_snap_mid_bucket_residual() {
        // A 1.1× zoom over a 1.25-bucket raster: transformed size
        // (1.1·40 = 44) ≠ bitmap (50) → residual GPU scaling, no snap;
        // all corners go through the plain affine transform.
        let quad = glyph([4.0, 8.0, 20.0, 20.0], [0.0, 0.0, 50.0, 50.0], false);
        let zoom = Transform2D {
            m: [1.1, 0.0, 0.0, 1.1, 3.3, 7.8],
        };
        let verts = QuadVertex::from_glyph_quad_transformed(&quad, 2.0, 256, 256, &zoom);
        assert_pos_near(verts[0].position, [1.1 * 8.0 + 3.3, 1.1 * 16.0 + 7.8]);
        assert_pos_near(verts[2].position, [1.1 * 48.0 + 3.3, 1.1 * 56.0 + 7.8]);
    }

    #[test]
    fn glyph_no_snap_rotation() {
        // Rotated transform (b, c ≠ 0) never snaps, even at matching size.
        let quad = glyph([10.0, 20.0, 30.0, 40.0], [0.0, 0.0, 30.0, 40.0], false);
        let (s, c) = (0.1_f32.sin(), 0.1_f32.cos());
        let rot = Transform2D {
            m: [c, s, -s, c, 0.0, 0.0],
        };
        let verts = QuadVertex::from_glyph_quad_transformed(&quad, 1.0, 256, 256, &rot);
        assert_pos_near(
            verts[0].position,
            [c * 10.0 - s * 20.0, s * 10.0 + c * 20.0],
        );
        assert_pos_near(
            verts[2].position,
            [c * 40.0 - s * 60.0, s * 40.0 + c * 60.0],
        );
    }

    #[test]
    fn glyph_snap_translation_only_transform() {
        // Pure fractional translation (e.g. scroll offset) still maps 1:1
        // → snapped.
        let quad = glyph([10.3, 20.0, 30.0, 40.0], [0.0, 0.0, 30.0, 40.0], false);
        let pan = Transform2D {
            m: [1.0, 0.0, 0.0, 1.0, 5.7, 3.2],
        };
        let verts = QuadVertex::from_glyph_quad_transformed(&quad, 1.0, 256, 256, &pan);
        // origin: (10.3 + 5.7, 20.0 + 3.2) = (16.0, 23.2) → (16, 23)
        assert_eq!(verts[0].position, [16.0, 23.0]);
        assert_eq!(verts[2].position, [46.0, 63.0]);
    }

    #[test]
    fn glyph_snap_color_emoji_flag_preserved() {
        let quad = glyph([10.3, 20.7, 30.0, 40.0], [0.0, 0.0, 30.0, 40.0], true);
        let verts = QuadVertex::from_glyph_quad_transformed(
            &quad,
            1.0,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        assert_eq!(verts[0].position, [10.0, 21.0]);
        for v in &verts {
            assert_eq!(v.flags, QUAD_FLAG_COLOR_GLYPH);
        }
    }

    #[test]
    fn glyph_uvs_independent_of_snapping() {
        // UVs come from the atlas rect alone — identical whether the
        // position path snapped or not.
        let quad = glyph([10.3, 20.7, 30.0, 40.0], [16.0, 32.0, 30.0, 40.0], false);
        let snapped = QuadVertex::from_glyph_quad_transformed(
            &quad,
            1.0,
            256,
            256,
            &Transform2D::IDENTITY,
        );
        let residual = Transform2D {
            m: [1.1, 0.0, 0.0, 1.1, 0.0, 0.0],
        };
        let unsnapped =
            QuadVertex::from_glyph_quad_transformed(&quad, 1.0, 256, 256, &residual);
        for (a, b) in snapped.iter().zip(unsnapped.iter()) {
            assert_eq!(a.tex_coord, b.tex_coord);
        }
        assert_eq!(snapped[0].tex_coord, [16.0 / 256.0, 32.0 / 256.0]);
        assert_eq!(snapped[2].tex_coord, [46.0 / 256.0, 72.0 / 256.0]);
    }

    #[test]
    fn decoration_rect_to_vertices() {
        let rect = DecorationRect {
            rect: [0.0, 0.0, 100.0, 2.0],
            color: [1.0, 0.0, 0.0, 1.0],
            kind: DecorationKind::FocusRing,
        };
        let verts = RectVertex::from_decoration(&rect, 1.0);
        assert_eq!(verts.len(), 4);
        assert_eq!(verts[0].position, [0.0, 0.0]);
        assert_eq!(verts[2].position, [100.0, 2.0]);
    }

    #[test]
    fn shape_quad_to_sdf_vertices() {
        let shape = ShapeQuad {
            screen: [0.0, 0.0, 100.0, 40.0],
            color: [0.0, 0.5, 0.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [6.0, 6.0, 6.0, 6.0],
            paint_data: PaintData::Solid,
        };
        let verts = SdfVertex::from_shape_quad(&shape, 1.0);
        assert_eq!(verts.len(), 4);
        assert_eq!(verts[0].corner_radii, [6.0, 6.0, 6.0, 6.0]);
        // Unfilled: quad is padded by the 1 dp AA margin on each side.
        // local_uv is extrapolated correspondingly.
        assert_eq!(verts[0].position, [-1.0, -1.0]);
        assert_eq!(verts[2].position, [101.0, 41.0]);
        assert!((verts[0].local_uv[0] - (-0.01)).abs() < 1e-5);
        assert!((verts[0].local_uv[1] - (-0.025)).abs() < 1e-5);
        assert!((verts[2].local_uv[0] - 1.01).abs() < 1e-5);
        assert!((verts[2].local_uv[1] - 1.025).abs() < 1e-5);
    }

    #[test]
    fn sdf_scale_factor() {
        let shape = ShapeQuad {
            screen: [10.0, 10.0, 100.0, 40.0],
            color: [0.0, 0.0, 0.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 2.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [4.0; 4],
            paint_data: PaintData::Solid,
        };
        let verts = SdfVertex::from_shape_quad(&shape, 2.0);
        // Scaled origin (20, 20) is further offset by the rasterization pad
        // (stroke/2 + 1) = (2*2)/2 + 1 = 3 pixels.
        assert_eq!(verts[0].position, [17.0, 17.0]);
        assert_eq!(verts[0].shape_params[2], 4.0); // stroke_width * 2
        // Corner radii must scale with the rect so a circle stays a circle
        // on HiDPI. shape_params.xy is in physical px; corner_radii has to
        // match or radius/half_size diverges.
        assert_eq!(verts[0].corner_radii, [8.0; 4]);
    }

    #[test]
    fn sdf_circle_stays_circle_on_hidpi() {
        // Regression: a 19×19 logical rect with 9.5 px corner radius is a
        // perfect circle. On Retina (scale_factor 2) the shader works in
        // physical px against `shape_params.xy`. If corner_radii is left in
        // logical px, the radio button / toggle pill renders as a rounded
        // square instead of a circle.
        let shape = ShapeQuad {
            screen: [0.0, 0.0, 19.0, 19.0],
            color: [0.0, 0.0, 0.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [9.5; 4],
            paint_data: PaintData::Solid,
        };
        let verts = SdfVertex::from_shape_quad(&shape, 2.0);
        assert_eq!(verts[0].shape_params[0], 38.0);
        assert_eq!(verts[0].shape_params[1], 38.0);
        assert_eq!(verts[0].corner_radii, [19.0; 4]);
    }

    #[test]
    fn cosmetic_shape_stroke_param_is_inverse_zoom() {
        // Cosmetic border: the baked SDF stroke param = width·sf / zoom, so
        // after the shader's per-unit ×zoom mapping the border lands at a
        // constant width·sf device px at any zoom. The body size params stay
        // put (the body still zooms via the view transform).
        let shape = ShapeQuad {
            screen: [0.0, 0.0, 100.0, 100.0],
            color: [0.0, 0.0, 0.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 2.0,
            stroke_space: StrokeSpace::Device,
            corner_radii: [10.0; 4],
            paint_data: PaintData::Solid,
        };
        let sf = 2.0;
        let logical = SdfVertex::from_shape_quad(&shape, sf);
        let z1 = SdfVertex::from_shape_quad_cosmetic(&shape, sf, 1.0);
        let z2 = SdfVertex::from_shape_quad_cosmetic(&shape, sf, 2.0);
        // zoom 1 matches the logical bake: width·sf = 2·2 = 4.
        assert!((z1[0].shape_params[2] - logical[0].shape_params[2]).abs() < 1e-4);
        assert!((z1[0].shape_params[2] - 4.0).abs() < 1e-4);
        // zoom 2 halves the param so the on-screen width stays width·sf.
        assert!((z2[0].shape_params[2] - 2.0).abs() < 1e-4);
        // Body size params unchanged across zoom (the quad corners zoom, not
        // the SDF body units): width·sf = 100·2 = 200.
        assert_eq!(z1[0].shape_params[0], z2[0].shape_params[0]);
        assert_eq!(z2[0].shape_params[0], 200.0);
    }

    #[test]
    fn sdf_linear_gradient_encoding() {
        let shape = ShapeQuad {
            screen: [0.0, 0.0, 100.0, 50.0],
            color: [1.0, 1.0, 1.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [0.0; 4],
            paint_data: PaintData::LinearGradient {
                start: [0.0, 0.0],
                end: [100.0, 0.0],
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Color::RED,
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Color::BLUE,
                    },
                ],
            },
        };
        let verts = SdfVertex::from_shape_quad(&shape, 1.0);
        // paint_type = 1 (linear)
        assert!((verts[0].shape_params[3] - 1.0).abs() < 0.01);
        // gradient_geo: start=(0,0), end=(1,0) in UV
        assert!((verts[0].gradient_geo[0]).abs() < 0.01);
        assert!((verts[0].gradient_geo[2] - 1.0).abs() < 0.01);
        // First stop is red
        assert!((verts[0].gradient_color0[0] - 1.0).abs() < 0.01);
        // Offsets
        assert!((verts[0].gradient_offsets[0]).abs() < 0.01);
        assert!((verts[0].gradient_offsets[1] - 1.0).abs() < 0.01);
    }

    #[test]
    fn linear_gradient_endpoints_are_rect_local_not_absolute() {
        // Regression for the HSV-canvas bug: the gradient endpoints
        // are normalized by the rect's width/height (`encode_paint_data`
        // doesn't see the rect origin), so callers MUST pass them in
        // rect-local coordinates. A rect at non-origin with rect-local
        // endpoints (0,0)→(0,h) must encode to start_uv=(0,0) and
        // end_uv=(0,1) — full gradient sampling across the rect.
        // Passing absolute coords would shift the endpoints away and
        // visibly squash the gradient.
        let shape = ShapeQuad {
            screen: [50.0, 100.0, 200.0, 200.0],
            color: [1.0, 1.0, 1.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            stroke_space: StrokeSpace::Logical,
            corner_radii: [0.0; 4],
            paint_data: PaintData::LinearGradient {
                start: [0.0, 0.0],
                end: [0.0, 200.0],
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Color::new(0.0, 0.0, 0.0, 0.0),
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Color::BLACK,
                    },
                ],
            },
        };
        let verts = SdfVertex::from_shape_quad(&shape, 1.0);
        assert!((verts[0].gradient_geo[0]).abs() < 1e-5, "start_uv.x");
        assert!((verts[0].gradient_geo[1]).abs() < 1e-5, "start_uv.y");
        assert!((verts[0].gradient_geo[2]).abs() < 1e-5, "end_uv.x");
        assert!((verts[0].gradient_geo[3] - 1.0).abs() < 1e-5, "end_uv.y");
    }

    #[test]
    fn generate_indices_for_multiple_quads() {
        let indices = generate_quad_indices(2);
        assert_eq!(indices.len(), 12);
        assert_eq!(&indices[0..6], &[0, 1, 2, 0, 2, 3]);
        assert_eq!(&indices[6..12], &[4, 5, 6, 4, 6, 7]);
    }

    #[test]
    fn shadow_quad_to_vertices() {
        let shadow = ShadowQuad {
            screen: [0.0, 0.0, 120.0, 60.0],
            color: [0.0, 0.0, 0.0, 0.3],
            corner_radii: [6.0; 4],
            shape_rect: [10.0, 8.0, 100.0, 40.0],
            blur_radius: 4.0,
            spread: 0.0,
        };
        let verts = ShadowVertex::from_shadow_quad(&shadow, 1.0);
        assert_eq!(verts.len(), 4);
        assert_eq!(verts[0].position, [0.0, 0.0]);
        assert_eq!(verts[2].position, [120.0, 60.0]);
        assert_eq!(verts[0].shadow_params[2], 4.0); // blur_radius
        assert_eq!(verts[0].corner_radii, [6.0; 4]);
    }

    #[test]
    fn shadow_scale_factor() {
        let shadow = ShadowQuad {
            screen: [10.0, 10.0, 120.0, 60.0],
            color: [0.0, 0.0, 0.0, 0.3],
            corner_radii: [6.0; 4],
            shape_rect: [20.0, 18.0, 100.0, 40.0],
            blur_radius: 4.0,
            spread: 2.0,
        };
        let verts = ShadowVertex::from_shadow_quad(&shadow, 2.0);
        assert_eq!(verts[0].position, [20.0, 20.0]);
        assert_eq!(verts[0].shadow_params[0], 200.0); // shape_w * 2
        assert_eq!(verts[0].shadow_params[2], 8.0); // blur * 2
        assert_eq!(verts[0].shadow_params[3], 4.0); // spread * 2
        // Corner radii are compared against shadow_params.xy in the
        // shadow shader's SDF; both have to live in the same px space.
        assert_eq!(verts[0].corner_radii, [12.0; 4]);
    }
}
