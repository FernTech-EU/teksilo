use bytemuck::{Pod, Zeroable};

use fern_canvas::render_frame::PaintData;
use fern_canvas::{DecorationRect, GlyphQuad, ShadowQuad, ShapeQuad};

/// Vertex for the textured quad pipeline (glyphs, images).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct QuadVertex {
    pub position: [f32; 2],
    pub tex_coord: [f32; 2],
    pub color: [f32; 4],
}

impl QuadVertex {
    /// Convert a glyph quad to 4 vertices (two triangles via index buffer).
    /// Applies scale_factor to screen coordinates.
    /// Atlas coordinates are in pixels and normalized to 0..1 using atlas dimensions.
    pub fn from_glyph_quad(
        quad: &GlyphQuad,
        scale_factor: f32,
        atlas_width: u32,
        atlas_height: u32,
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

        [
            QuadVertex {
                position: [sx, sy],
                tex_coord: [u0, v0],
                color: quad.color,
            },
            QuadVertex {
                position: [sx + sw, sy],
                tex_coord: [u1, v0],
                color: quad.color,
            },
            QuadVertex {
                position: [sx + sw, sy + sh],
                tex_coord: [u1, v1],
                color: quad.color,
            },
            QuadVertex {
                position: [sx, sy + sh],
                tex_coord: [u0, v1],
                color: quad.color,
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
                color: rect.color,
            },
            RectVertex {
                position: [sx + sw, sy],
                color: rect.color,
            },
            RectVertex {
                position: [sx + sw, sy + sh],
                color: rect.color,
            },
            RectVertex {
                position: [sx, sy + sh],
                color: rect.color,
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
    /// Convert a shape quad to 4 vertices.
    pub fn from_shape_quad(shape: &ShapeQuad, scale_factor: f32) -> [SdfVertex; 4] {
        let [x, y, w, h] = shape.screen;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        // Encode paint type and gradient data
        let (paint_type, gradient_geo, colors, offsets) =
            encode_paint_data(&shape.paint_data, w, h);

        let params = [sw, sh, shape.stroke_width * scale_factor, paint_type as f32];

        let base = SdfVertex {
            position: [0.0, 0.0],
            local_uv: [0.0, 0.0],
            color: shape.color,
            corner_radii: shape.corner_radii,
            shape_params: params,
            gradient_geo,
            gradient_color0: colors[0],
            gradient_color1: colors[1],
            gradient_color2: colors[2],
            gradient_color3: colors[3],
            gradient_offsets: offsets,
        };

        [
            SdfVertex {
                position: [sx, sy],
                local_uv: [0.0, 0.0],
                ..base
            },
            SdfVertex {
                position: [sx + sw, sy],
                local_uv: [1.0, 0.0],
                ..base
            },
            SdfVertex {
                position: [sx + sw, sy + sh],
                local_uv: [1.0, 1.0],
                ..base
            },
            SdfVertex {
                position: [sx, sy + sh],
                local_uv: [0.0, 1.0],
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
fn encode_stops(stops: &[fern_canvas::GradientStop]) -> ([[f32; 4]; 4], [f32; 4]) {
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

        [
            ShadowVertex {
                position: [sx, sy],
                local_uv: [0.0, 0.0],
                shadow_color: shadow.color,
                corner_radii: shadow.corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
            ShadowVertex {
                position: [sx + sw, sy],
                local_uv: [1.0, 0.0],
                shadow_color: shadow.color,
                corner_radii: shadow.corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
            ShadowVertex {
                position: [sx + sw, sy + sh],
                local_uv: [1.0, 1.0],
                shadow_color: shadow.color,
                corner_radii: shadow.corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
            ShadowVertex {
                position: [sx, sy + sh],
                local_uv: [0.0, 1.0],
                shadow_color: shadow.color,
                corner_radii: shadow.corner_radii,
                shadow_params: params,
                shape_offset: offset,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::{DecorationKind, GradientStop, PaintData, ShapeKind};
    use fern_tokens::Color;

    #[test]
    fn glyph_quad_to_vertices() {
        let quad = GlyphQuad {
            screen: [10.0, 20.0, 30.0, 40.0],
            atlas: [0.0, 0.0, 64.0, 64.0],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let verts = QuadVertex::from_glyph_quad(&quad, 1.0, 256, 256);
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
        let quad = GlyphQuad {
            screen: [10.0, 20.0, 30.0, 40.0],
            atlas: [0.0, 0.0, 128.0, 128.0],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let verts = QuadVertex::from_glyph_quad(&quad, 2.0, 256, 256);
        assert_eq!(verts[0].position, [20.0, 40.0]);
        assert_eq!(verts[1].position, [80.0, 40.0]);
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
            corner_radii: [6.0, 6.0, 6.0, 6.0],
            paint_data: PaintData::Solid,
        };
        let verts = SdfVertex::from_shape_quad(&shape, 1.0);
        assert_eq!(verts.len(), 4);
        assert_eq!(verts[0].corner_radii, [6.0, 6.0, 6.0, 6.0]);
        assert_eq!(verts[0].local_uv, [0.0, 0.0]);
        assert_eq!(verts[2].local_uv, [1.0, 1.0]);
    }

    #[test]
    fn sdf_scale_factor() {
        let shape = ShapeQuad {
            screen: [10.0, 10.0, 100.0, 40.0],
            color: [0.0, 0.0, 0.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 2.0,
            corner_radii: [4.0; 4],
            paint_data: PaintData::Solid,
        };
        let verts = SdfVertex::from_shape_quad(&shape, 2.0);
        assert_eq!(verts[0].position, [20.0, 20.0]);
        assert_eq!(verts[0].shape_params[2], 4.0); // stroke_width * 2
    }

    #[test]
    fn sdf_linear_gradient_encoding() {
        let shape = ShapeQuad {
            screen: [0.0, 0.0, 100.0, 50.0],
            color: [1.0, 1.0, 1.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
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
    }
}
