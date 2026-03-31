use bytemuck::{Pod, Zeroable};

use fern_canvas::{DecorationRect, GlyphQuad, ShapeQuad};

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
    /// Shape bounds in pixels: [width, height, stroke_width, 0].
    pub shape_params: [f32; 4],
}

impl SdfVertex {
    /// Convert a shape quad to 4 vertices.
    pub fn from_shape_quad(shape: &ShapeQuad, scale_factor: f32) -> [SdfVertex; 4] {
        let [x, y, w, h] = shape.screen;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        let params = [sw, sh, shape.stroke_width * scale_factor, 0.0];

        [
            SdfVertex {
                position: [sx, sy],
                local_uv: [0.0, 0.0],
                color: shape.color,
                corner_radii: shape.corner_radii,
                shape_params: params,
            },
            SdfVertex {
                position: [sx + sw, sy],
                local_uv: [1.0, 0.0],
                color: shape.color,
                corner_radii: shape.corner_radii,
                shape_params: params,
            },
            SdfVertex {
                position: [sx + sw, sy + sh],
                local_uv: [1.0, 1.0],
                color: shape.color,
                corner_radii: shape.corner_radii,
                shape_params: params,
            },
            SdfVertex {
                position: [sx, sy + sh],
                local_uv: [0.0, 1.0],
                color: shape.color,
                corner_radii: shape.corner_radii,
                shape_params: params,
            },
        ]
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::{DecorationKind, PaintData, ShapeKind};

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
    fn generate_indices_for_multiple_quads() {
        let indices = generate_quad_indices(2);
        assert_eq!(indices.len(), 12);
        assert_eq!(&indices[0..6], &[0, 1, 2, 0, 2, 3]);
        assert_eq!(&indices[6..12], &[4, 5, 6, 4, 6, 7]);
    }
}
