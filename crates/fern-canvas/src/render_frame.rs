use std::borrow::Cow;

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
    /// Animated quads (procedural or sprite-atlas kinds). Emitted by
    /// widgets that opt into the shader-driven animation pipeline via
    /// `ctx.animated_quad()`. The fragment shader samples per-slot
    /// state from a renderer-side uniform buffer updated each frame by
    /// the widget tree — the widget's own `paint()` runs only when
    /// layout changes, not once per animation frame.
    pub animated_quads: Vec<AnimatedQuadDraw>,
    /// Per-slot `AnimParams`, indexed by the `slot` field of each
    /// `AnimatedQuadDraw`. Recomputed by the widget tree every frame
    /// (phase advanced, colors resolved against the current theme)
    /// and uploaded to the renderer's uniform buffer at the top of
    /// `Renderer::render`. Slots whose widget is dormant / offscreen /
    /// in an inactive window keep their last-written values — the
    /// fragment shader still renders, just with stale phase for one
    /// frame until the next tick resumes.
    pub anim_params: Vec<AnimParams>,
    pub draw_order: Vec<DrawCommand>,
    /// Images that need GPU registration before rendering this frame.
    pub pending_images: Vec<PendingImage>,
    /// Opaque [`TextLayout::layout_key`](crate::text_backend::TextLayout::layout_key)
    /// values for every `draw_text*` call that produced glyphs in this
    /// frame. When a widget's `cached_paint` is reused without re-running
    /// `paint()`, the renderer calls `TextBackend::touch_layout(key)` for
    /// each stored key so the backend can refresh the underlying glyph
    /// cache timestamps and avoid evicting still-visible glyphs.
    pub layout_keys: Vec<u64>,
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
        let animated_offset = self.animated_quads.len();

        self.glyphs.extend_from_slice(&other.glyphs);
        self.images.extend_from_slice(&other.images);
        self.decorations.extend_from_slice(&other.decorations);
        self.shapes.extend_from_slice(&other.shapes);
        self.shadows.extend_from_slice(&other.shadows);
        self.rasterized.extend_from_slice(&other.rasterized);
        self.paths.extend_from_slice(&other.paths);
        self.animated_quads.extend_from_slice(&other.animated_quads);
        // `anim_params` is NOT merged index-wise — the widget tree
        // writes one authoritative slice per frame (indexed by
        // registry slot, which is global across the tree). Cached
        // sub-frames carry empty `anim_params`; the outer tree
        // replaces it wholesale after `render()` is called.
        self.layout_keys.extend_from_slice(&other.layout_keys);
        // Merge pending image registrations (deduped by renderer)
        for pending in &other.pending_images {
            if !self.pending_images.iter().any(|p| p.name == pending.name) {
                self.pending_images.push(pending.clone());
            }
        }

        for cmd in &other.draw_order {
            let shifted = match cmd {
                DrawCommand::Glyph(i) => DrawCommand::Glyph(i + glyph_offset),
                DrawCommand::Image(i) => DrawCommand::Image(i + image_offset),
                DrawCommand::Decoration(i) => DrawCommand::Decoration(i + decoration_offset),
                DrawCommand::Shape(i) => DrawCommand::Shape(i + shape_offset),
                DrawCommand::Shadow(i) => DrawCommand::Shadow(i + shadow_offset),
                DrawCommand::Rasterized(i) => DrawCommand::Rasterized(i + rasterized_offset),
                DrawCommand::Path(i) => DrawCommand::Path(i + path_offset),
                DrawCommand::AnimatedQuad(i) => DrawCommand::AnimatedQuad(i + animated_offset),
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
        let mut transform_depth: i32 = 0;
        let mut blur_depth: i32 = 0;
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
                DrawCommand::PushTransform(_) => transform_depth += 1,
                DrawCommand::PopTransform => {
                    transform_depth -= 1;
                    debug_assert!(
                        transform_depth >= 0,
                        "RenderFrame: PopTransform without matching PushTransform at draw_order[{i}]"
                    );
                }
                DrawCommand::BeginBlurredSubtree { .. } => blur_depth += 1,
                DrawCommand::EndBlurredSubtree => {
                    blur_depth -= 1;
                    debug_assert!(
                        blur_depth >= 0,
                        "RenderFrame: EndBlurredSubtree without matching BeginBlurredSubtree at draw_order[{i}]"
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
        debug_assert!(
            transform_depth == 0,
            "RenderFrame: {transform_depth} unmatched PushTransform(s) without PopTransform"
        );
        debug_assert!(
            blur_depth == 0,
            "RenderFrame: {blur_depth} unmatched BeginBlurredSubtree(s) without EndBlurredSubtree"
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
    /// When `Some(color)`, the image is rendered as an alpha mask tinted
    /// with this color (shader flag=0). When `None`, the image is rendered
    /// in full color (shader flag=1, existing behavior).
    pub tint: Option<[f32; 4]>,
}

/// An image that needs to be registered (uploaded to GPU) before rendering.
/// Widgets emit these during paint for embedded raster resources.
#[derive(Debug, Clone)]
pub struct PendingImage {
    /// Resource name to register under.
    pub name: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// RGBA pixel data. Uses `Cow` for zero-copy with compile-time data.
    pub pixels: Cow<'static, [u8]>,
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
    /// Shader-driven animated quad — index into `RenderFrame::animated_quads`.
    /// The per-frame state (phase, frame_index, colors) is NOT in the
    /// vertex data; the renderer looks it up from its uniform buffer
    /// via the `slot` stored in `AnimatedQuadDraw`.
    AnimatedQuad(usize),
    SetClip(Rect),
    ClearClip,
    SetOpacity(f32),
    RestoreOpacity,
    SetBlendMode(BlendMode),
    RestoreBlendMode,
    /// Set the renderer's current transform. **Composes** with the
    /// top of the renderer's transform stack: the new current is
    /// `t.then(stack_top)` — the supplied transform is applied to a
    /// local point *first*, then the stack's outer ancestors compose
    /// outward. For widgets not under any `PushTransform` scope the
    /// stack is `[identity]`, so this behaves as "set absolute" —
    /// backwards compatible.
    SetTransform(Transform2D),
    /// Push a new transform onto the renderer's transform stack.
    /// The new top becomes `t.then(prev_top)` — the deepest (innermost)
    /// `t` applies to a pre-transform local point first, then outer
    /// ancestors compose outward. That becomes the renderer's
    /// `current_transform` until the matching
    /// [`DrawCommand::PopTransform`]. Emitted by the render walker
    /// around a subtree whose root has a `transform_prop` set. See
    /// `WidgetArena::effective_transform` in `fern-core` for the
    /// composition mirrored on the arena side (used by hit-testing
    /// and a11y bounds projection).
    PushTransform(Transform2D),
    /// Pop the renderer's transform stack, restoring the previous
    /// top as the new `current_transform`. Must be paired with a
    /// [`DrawCommand::PushTransform`].
    PopTransform,
    /// Begin an offscreen-rendered, blurred subtree. The renderer
    /// allocates an intermediate texture sized to `bounds` (in logical
    /// pixels), redirects subsequent drawing into it, and on the
    /// matching [`DrawCommand::EndBlurredSubtree`] runs a dual-Kawase
    /// blur chain at the requested `radius` and composites the result
    /// back into the parent pass at `bounds`.
    BeginBlurredSubtree { bounds: Rect, radius: f32 },
    /// End an offscreen-rendered, blurred subtree. Must be paired with
    /// a preceding [`DrawCommand::BeginBlurredSubtree`].
    EndBlurredSubtree,
}

/// An animated quad to render with one of the shader-animation pipelines.
/// The fragment shader samples per-slot state from the renderer's uniform
/// buffer (updated each frame by the widget tree's animated-quad
/// registry) — the `slot` field selects which entry to read.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimatedQuadDraw {
    /// Screen-space bounds: [x, y, width, height] in logical pixels.
    pub screen: [f32; 4],
    /// Dense index into the renderer's `AnimParams` uniform array. Owned
    /// and allocated by the widget tree's `AnimatedQuadRegistry`; stable
    /// for the lifetime of one widget mount (freed on rebuild/destroy).
    pub slot: u32,
    /// Which pipeline draws this quad — procedural (sweep, pulse…) or
    /// sprite (texture-atlas frame cycling). Picked once at emit time.
    pub class: AnimatedQuadClass,
}

/// Which shader pipeline a [`DrawCommand::AnimatedQuad`] is routed to.
/// Chosen by the widget at `Canvas::draw_animated_quad` time based on
/// its `AnimatedQuadKind`; the renderer binds the matching pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimatedQuadClass {
    /// Fully procedural — no texture binding. IndeterminateSweep,
    /// Pulse, Shimmer, etc.
    Procedural,
    /// Samples a texture atlas. Carries the image name so the renderer
    /// can resolve the bind group (same path registered images use).
    Sprite { image_name: String },
}

/// GPU-visible per-slot state for a shader-driven animated quad.
/// Layout must match the WGSL `AnimParams` struct in
/// `fern-render/src/shaders/anim_procedural.wgsl` (and the sprite
/// variant). `repr(C)` with explicit `_pad` fields for `std140`
/// compatibility.
///
/// Lives in `fern-canvas` (not `fern-core`) because it is the
/// serialized-over-the-wire data type between the tree's animated-quad
/// registry and the renderer, and `RenderFrame` is already the
/// tree→renderer data channel.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnimParams {
    /// Discriminator — 0 = IndeterminateSweep, 1 = SpriteCycle,
    /// 2 = SpinnerArc, ... Matches the `kind: u32` constant in the
    /// fragment shader switch.
    pub kind: u32,
    /// Continuous phase for procedural kinds (0..1) OR integer frame
    /// index (as f32) for sprite kinds. SpinnerArc: rotation phase
    /// (0..1, one full rotation per period).
    pub phase: f32,
    /// IndeterminateSweep: sweep band width (0..1).
    /// SpinnerArc: arc length as a fraction of the full circle.
    /// Other kinds unused.
    pub sweep_ratio: f32,
    /// Generic per-kind parameter slot. SpinnerArc: stroke thickness
    /// as a fraction of the smaller extent (0..0.5). Other kinds
    /// treat this as padding for std140 alignment.
    pub _pad0: f32,
    /// IndeterminateSweep: track color. Unused for sprite and spinner.
    pub color0: [f32; 4],
    /// IndeterminateSweep: fill color. SpriteCycle: tint (alpha 0 = no
    /// tint). SpinnerArc: arc color.
    pub color1: [f32; 4],
    /// Sprite atlas grid width (cols). Unused for procedural.
    pub atlas_cols: f32,
    /// Sprite atlas grid height (rows). Unused for procedural.
    pub atlas_rows: f32,
    pub _pad1: [f32; 2],
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
