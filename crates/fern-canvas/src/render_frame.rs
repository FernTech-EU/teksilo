use crate::geometry::{Rect, Transform2D};
use crate::paint::StrokeStyle;

/// The complete render output for one frame. This is the boundary between
/// platform-independent widget code and GPU-specific rendering code.
#[derive(Debug, Clone, Default)]
pub struct RenderFrame {
    pub glyphs: Vec<GlyphQuad>,
    pub images: Vec<ImageQuad>,
    pub decorations: Vec<DecorationRect>,
    pub shapes: Vec<ShapeQuad>,
    pub shadows: Vec<ShadowQuad>,
    pub rasterized: Vec<RasterizedQuad>,
    pub paths: Vec<PathEntry>,
    pub draw_order: Vec<DrawCommand>,
}

impl RenderFrame {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.draw_order.is_empty()
    }

    /// Merge another frame into this one, appending all entries
    /// and adjusting draw_order indices.
    pub fn merge(&mut self, other: &RenderFrame) {
        let glyph_offset = self.glyphs.len();
        let image_offset = self.images.len();
        let decoration_offset = self.decorations.len();
        let shape_offset = self.shapes.len();
        let shadow_offset = self.shadows.len();
        let rasterized_offset = self.rasterized.len();
        let path_offset = self.paths.len();

        self.glyphs.extend_from_slice(&other.glyphs);
        self.images.extend_from_slice(&other.images);
        self.decorations.extend_from_slice(&other.decorations);
        self.shapes.extend_from_slice(&other.shapes);
        self.shadows.extend_from_slice(&other.shadows);
        self.rasterized.extend_from_slice(&other.rasterized);
        self.paths.extend_from_slice(&other.paths);

        for cmd in &other.draw_order {
            let shifted = match cmd {
                DrawCommand::Glyph(i) => DrawCommand::Glyph(i + glyph_offset),
                DrawCommand::Image(i) => DrawCommand::Image(i + image_offset),
                DrawCommand::Decoration(i) => DrawCommand::Decoration(i + decoration_offset),
                DrawCommand::Shape(i) => DrawCommand::Shape(i + shape_offset),
                DrawCommand::Shadow(i) => DrawCommand::Shadow(i + shadow_offset),
                DrawCommand::Rasterized(i) => DrawCommand::Rasterized(i + rasterized_offset),
                DrawCommand::Path(i) => DrawCommand::Path(i + path_offset),
                other => other.clone(),
            };
            self.draw_order.push(shifted);
        }
    }
}

impl RenderFrame {
    /// Validate that clip and opacity stacks are balanced in the draw order.
    /// Only runs in debug builds. Panics with a descriptive message if
    /// any push/pop pair is unbalanced.
    pub fn debug_validate_stacks(&self) {
        if !cfg!(debug_assertions) {
            return;
        }
        let mut clip_depth: i32 = 0;
        let mut opacity_depth: i32 = 0;
        let mut blend_depth: i32 = 0;
        for (i, cmd) in self.draw_order.iter().enumerate() {
            match cmd {
                DrawCommand::SetClip(_) => clip_depth += 1,
                DrawCommand::ClearClip => {
                    clip_depth -= 1;
                    debug_assert!(
                        clip_depth >= 0,
                        "RenderFrame: ClearClip without matching SetClip at draw_order[{i}]"
                    );
                }
                DrawCommand::SetOpacity(_) => opacity_depth += 1,
                DrawCommand::RestoreOpacity => {
                    opacity_depth -= 1;
                    debug_assert!(
                        opacity_depth >= 0,
                        "RenderFrame: RestoreOpacity without matching SetOpacity at draw_order[{i}]"
                    );
                }
                DrawCommand::SetBlendMode(_) => blend_depth += 1,
                DrawCommand::RestoreBlendMode => {
                    blend_depth -= 1;
                    debug_assert!(
                        blend_depth >= 0,
                        "RenderFrame: RestoreBlendMode without matching SetBlendMode at draw_order[{i}]"
                    );
                }
                _ => {}
            }
        }
        debug_assert!(
            clip_depth == 0,
            "RenderFrame: {clip_depth} unmatched SetClip(s) without ClearClip"
        );
        debug_assert!(
            opacity_depth == 0,
            "RenderFrame: {opacity_depth} unmatched SetOpacity(s) without RestoreOpacity"
        );
        debug_assert!(
            blend_depth == 0,
            "RenderFrame: {blend_depth} unmatched SetBlendMode(s) without RestoreBlendMode"
        );
    }
}

/// A positioned glyph to render as a textured rectangle from the glyph atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphQuad {
    /// Screen position and size: [x, y, width, height] in logical pixels.
    pub screen: [f32; 4],
    /// Atlas position and size: [x, y, width, height] in atlas texture coordinates.
    pub atlas: [f32; 4],
    /// Glyph color: [r, g, b, a]. For monochrome glyphs this is the text
    /// tint. For color emoji it is `[1, 1, 1, 1]` — the atlas region
    /// already holds the pre-multiplied RGBA bitmap, so the renderer
    /// samples `texture.rgb` directly.
    pub color: [f32; 4],
    /// `true` if the atlas region holds a pre-multiplied RGBA color
    /// bitmap (color emoji via COLR/CBDT/sbix). When set, the renderer
    /// must sample `texture.rgb` instead of using the texture as an
    /// alpha mask.
    pub is_color: bool,
}

/// An image quad to render as a textured rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageQuad {
    /// Screen position and size: [x, y, width, height] in logical pixels.
    pub screen: [f32; 4],
    /// Resource name of the image.
    pub name: String,
}

/// A colored rectangle for decorations (selections, cursors, underlines, borders, etc.).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorationRect {
    /// Position and size: [x, y, width, height] in logical pixels.
    pub rect: [f32; 4],
    /// Color: [r, g, b, a].
    pub color: [f32; 4],
    /// What kind of decoration this is.
    pub kind: DecorationKind,
}

/// The kind of decoration a DecorationRect represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationKind {
    WidgetBackground,
    Selection,
    Cursor,
    Underline,
    Overline,
    Strikeout,
    FocusRing,
    DropIndicator,
    TableBorder,
    TableCellBackground,
    BlockBackground,
    TextBackground,
    CellSelection,
}

/// A shape rendered via SDF (signed distance field) shaders.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeQuad {
    /// Screen position and size: [x, y, width, height] in logical pixels.
    pub screen: [f32; 4],
    /// Fill color: [r, g, b, a].
    pub color: [f32; 4],
    /// What shape to render.
    pub shape: ShapeKind,
    /// Stroke width (0.0 for filled shapes).
    pub stroke_width: f32,
    /// Corner radii: [top_left, top_right, bottom_right, bottom_left].
    pub corner_radii: [f32; 4],
    /// Paint type for the shape.
    pub paint_data: PaintData,
}

/// The kind of SDF shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    RoundedRect,
    Circle,
    Ellipse,
}

/// A CPU-rasterized path result, stored in the shape atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterizedQuad {
    /// Screen position and size: [x, y, width, height] in logical pixels.
    pub screen: [f32; 4],
    /// Shape atlas position and size: [x, y, width, height] in atlas coordinates.
    pub atlas: [f32; 4],
    /// Tint color: [r, g, b, a].
    pub color: [f32; 4],
}

/// A shadow rendered behind a shape using a separate GPU pipeline with Gaussian blur.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowQuad {
    /// Shadow bounding box (expanded by blur + spread + offset): [x, y, width, height].
    pub screen: [f32; 4],
    /// Shadow color: [r, g, b, a].
    pub color: [f32; 4],
    /// Corner radii matching the shape: [top_left, top_right, bottom_right, bottom_left].
    pub corner_radii: [f32; 4],
    /// The original shape rect (before offset/spread): [x, y, width, height].
    pub shape_rect: [f32; 4],
    /// Gaussian blur radius.
    pub blur_radius: f32,
    /// Shadow spread amount.
    pub spread: f32,
}

/// A path to be rasterized on the CPU (Tier 3). Stored in the RenderFrame
/// until the renderer rasterizes it into the shape atlas and converts it
/// to a [`RasterizedQuad`].
#[derive(Debug, Clone, PartialEq)]
pub struct PathEntry {
    /// The path commands to rasterize.
    pub path: crate::path::Path,
    /// Fill color: [r, g, b, a].
    pub color: [f32; 4],
    /// Stroke style (width, dash pattern, line cap).
    pub stroke_style: StrokeStyle,
    /// Bounding rect in logical pixels (computed from path bounds).
    pub bounds: [f32; 4],
}

/// Paint data for SDF shapes, passed to the GPU shader.
#[derive(Debug, Default, Clone, PartialEq)]
pub enum PaintData {
    #[default]
    Solid,
    LinearGradient {
        start: [f32; 2],
        end: [f32; 2],
        stops: Vec<crate::paint::GradientStop>,
    },
    RadialGradient {
        center: [f32; 2],
        radius: f32,
        stops: Vec<crate::paint::GradientStop>,
    },
    ConicGradient {
        center: [f32; 2],
        start_angle: f32,
        stops: Vec<crate::paint::GradientStop>,
    },
}

/// Compositing blend mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
}

/// A draw command referencing an entry in one of the RenderFrame arrays.
/// Commands are recorded in painter's order (back-to-front).
#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    Glyph(usize),
    Image(usize),
    Decoration(usize),
    Shape(usize),
    Shadow(usize),
    Rasterized(usize),
    Path(usize),
    SetClip(Rect),
    ClearClip,
    SetOpacity(f32),
    RestoreOpacity,
    SetBlendMode(BlendMode),
    RestoreBlendMode,
    SetTransform(Transform2D),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_render_frame() {
        let frame = RenderFrame::new();
        assert!(frame.is_empty());
        assert!(frame.glyphs.is_empty());
        assert!(frame.images.is_empty());
        assert!(frame.decorations.is_empty());
        assert!(frame.shapes.is_empty());
        assert!(frame.rasterized.is_empty());
    }

    #[test]
    fn merge_frames() {
        let mut a = RenderFrame::new();
        a.shapes.push(ShapeQuad {
            screen: [0.0, 0.0, 10.0, 10.0],
            color: [1.0, 0.0, 0.0, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            corner_radii: [0.0; 4],
            paint_data: PaintData::Solid,
        });
        a.draw_order.push(DrawCommand::Shape(0));

        let mut b = RenderFrame::new();
        b.decorations.push(DecorationRect {
            rect: [0.0, 0.0, 5.0, 5.0],
            color: [0.0, 0.0, 1.0, 1.0],
            kind: DecorationKind::FocusRing,
        });
        b.draw_order.push(DrawCommand::Decoration(0));

        a.merge(&b);
        assert_eq!(a.shapes.len(), 1);
        assert_eq!(a.decorations.len(), 1);
        assert_eq!(a.draw_order.len(), 2);
        assert_eq!(a.draw_order[1], DrawCommand::Decoration(0));
    }

    #[test]
    fn merge_preserves_state_commands() {
        let mut a = RenderFrame::new();
        a.draw_order.push(DrawCommand::SetOpacity(0.5));
        let mut b = RenderFrame::new();
        b.draw_order.push(DrawCommand::RestoreOpacity);
        a.merge(&b);
        assert_eq!(a.draw_order.len(), 2);
        assert_eq!(a.draw_order[0], DrawCommand::SetOpacity(0.5));
        assert_eq!(a.draw_order[1], DrawCommand::RestoreOpacity);
    }
}
