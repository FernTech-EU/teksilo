//! IconWidget — a vector or raster icon rendered at a configurable size.
//!
//! Supports multiple source formats:
//! - **Path**: Programmatic vector paths (checkmark, chevrons, etc.)
//! - **SVG**: Parsed from SVG strings via [`SvgIcon`]
//! - **PNG/WebP**: Raster images via [`RasterIcon`]
//! - **Animated WebP**: Frame sequences via [`AnimatedIcon`]
//!
//! Icons can be **tintable** (rendered as alpha masks tinted with the
//! widget's color — the default) or **full-color** (original colors
//! preserved). The tintable mode enables theme-aware icon coloring.

use std::borrow::Cow;

use bastyde_canvas::svg::SvgIcon;
use bastyde_canvas::{
    AnimatedIcon, AnimatedQuadClass, Canvas, Path, PathCommand, Point, RasterIcon, Rect, Size,
    SizeProposal,
};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::animated_quad::{AnimatedQuadHandle, AnimatedQuadKind};
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};
use bastyde_tokens::{Color, Easing, TextRole};

/// Whether an icon is rendered as a theme-tinted mask or in its original colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconMode {
    /// Treat as alpha mask, tint with the widget's color property.
    Tintable,
    /// Render original colors; widget color only controls opacity.
    FullColor,
}

/// The source data for an icon.
#[derive(Debug, Clone)]
enum IconSource {
    /// A vector path (SVG or programmatic).
    Path(Path),
    /// A parsed SVG icon — scaling deferred to paint time using display_size.
    Svg(SvgIcon),
    /// A decoded raster image (PNG or static WebP).
    /// `upload_pixels` holds the ready-to-upload data (alpha mask pre-applied for tintable).
    Raster {
        name: String,
        icon: RasterIcon,
        upload_pixels: Vec<u8>,
    },
    /// An animated image (animated WebP).
    /// `frame_upload_pixels` holds pre-computed pixels per frame — used
    /// by the legacy signal-based path (reduced-motion fallback and
    /// the static first-frame render). `sprite_atlas` is the shader
    /// path's pre-packed grid of all frames, built lazily on first
    /// build() when reduced-motion is off. `anim_handle` is the
    /// registry slot returned by `ctx.animated_quad` — set in pair
    /// with `sprite_atlas`, used at paint time.
    Animated {
        name: String,
        icon: AnimatedIcon,
        frame_upload_pixels: Vec<Vec<u8>>,
        /// Legacy signal-based frame index driver. `Some` only when
        /// shader pipeline is disabled (reduced-motion, atlas build
        /// failed, etc.); otherwise frame cycling runs shader-side.
        frame_signal: Option<Signal<f32>>,
        /// Sprite-atlas state for the shader pipeline. `Some` once
        /// `build()` has packed the frames into a grid.
        sprite_atlas: Option<SpriteAtlas>,
        /// Animated-quad handle returned by `ctx.animated_quad`.
        /// `Some` only when the shader path is active (paired with
        /// `sprite_atlas`).
        anim_handle: Option<AnimatedQuadHandle>,
    },
}

/// Packed frame-grid for an animated icon, prepared once per mount
/// and reused across every paint. The renderer uploads the atlas
/// pixels as a single texture; the shader samples the cell for the
/// current frame based on `AnimParams::phase` written by the tree.
#[derive(Debug, Clone)]
struct SpriteAtlas {
    /// Unique name under which the atlas is registered with
    /// `Canvas::ensure_image_registered` / the renderer's
    /// `ImageManager`.
    name: String,
    /// Full atlas pixels in RGBA row-major. `cols × frame_w` wide,
    /// `rows × frame_h` tall. Owned `Vec<u8>` so the widget can pass
    /// `Cow::Owned` to `ensure_image_registered` each paint without
    /// recomputing; uploaded once by the renderer's image manager.
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
}

/// A leaf widget that renders an icon from various sources.
pub struct IconWidget {
    source: IconSource,
    /// Design size: the coordinate space the path was created in.
    /// Used as the denominator when scaling the path to fit bounds.
    design_size: f32,
    /// Display size: what the icon reports for layout (size_that_fits).
    /// Defaults to `design_size` but can be overridden via `icon_size()`.
    display_size: f32,
    /// Fill/tint color. Defaults to [`TextRole::Primary`] so icons follow
    /// the surrounding text color across theme switches without binding.
    color: ColorProp,
    /// Rendering mode.
    mode: IconMode,
}

// Auto-generate unique names from data pointer for embedded resources.
// Compile-time data from include_bytes! has stable pointer addresses.
fn auto_name(prefix: &str, ptr: usize) -> String {
    format!("_icon_{prefix}_{ptr:x}")
}

/// Prepare raster pixels for upload: apply alpha mask for tintable mode,
/// or use original pixels for full-color mode.
fn prepare_pixels(icon: &RasterIcon, mode: IconMode) -> Vec<u8> {
    match mode {
        IconMode::Tintable => icon.to_alpha_mask().pixels().to_vec(),
        IconMode::FullColor => icon.pixels().to_vec(),
    }
}

impl IconWidget {
    /// Create an icon from a custom path. The path should be defined
    /// in coordinates matching the given size (e.g., 0..24 for size=24).
    pub fn from_path(path: Path, size: f32) -> Self {
        Self {
            source: IconSource::Path(path),
            design_size: size,
            display_size: size,
            color: ColorProp::TextRole(TextRole::Primary),
            mode: IconMode::Tintable,
        }
    }

    /// A checkmark icon (✓) at the given size.
    pub fn checkmark(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.2, s * 0.5));
        path.line_to(Point::new(s * 0.4, s * 0.75));
        path.line_to(Point::new(s * 0.8, s * 0.25));
        Self::from_path(path, size)
    }

    /// A downward-pointing chevron (▼) at the given size.
    pub fn chevron_down(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.25, s * 0.35));
        path.line_to(Point::new(s * 0.5, s * 0.65));
        path.line_to(Point::new(s * 0.75, s * 0.35));
        Self::from_path(path, size)
    }

    /// A right-pointing chevron (▶) at the given size.
    pub fn chevron_right(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.35, s * 0.25));
        path.line_to(Point::new(s * 0.65, s * 0.5));
        path.line_to(Point::new(s * 0.35, s * 0.75));
        Self::from_path(path, size)
    }

    /// A left-pointing chevron (◀) at the given size.
    pub fn chevron_left(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.65, s * 0.25));
        path.line_to(Point::new(s * 0.35, s * 0.5));
        path.line_to(Point::new(s * 0.65, s * 0.75));
        Self::from_path(path, size)
    }

    /// An upward-pointing chevron (▲) at the given size.
    pub fn chevron_up(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.25, s * 0.65));
        path.line_to(Point::new(s * 0.5, s * 0.35));
        path.line_to(Point::new(s * 0.75, s * 0.65));
        Self::from_path(path, size)
    }

    /// Create an icon from an SVG string. Parses the SVG and extracts
    /// geometry, ignoring any colors in the SVG. Display size defaults
    /// to the SVG's viewBox dimensions; use [`icon_size`](Self::icon_size)
    /// to override.
    ///
    /// If parsing fails, logs the error in debug mode and produces an empty icon.
    pub fn from_svg(svg_str: &str) -> Self {
        match SvgIcon::parse(svg_str) {
            Ok(icon) => Self::from_svg_icon(&icon),
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("bastyde: SVG parse error: {e}");
                Self::from_path(Path::new(), 0.0)
            }
        }
    }

    /// Create an icon from a pre-parsed [`SvgIcon`]. Display size
    /// defaults to the SVG's viewBox; use [`icon_size`](Self::icon_size)
    /// to override. Scaling is deferred to paint time.
    pub fn from_svg_icon(icon: &SvgIcon) -> Self {
        let vb_size = icon.width().max(icon.height());
        Self {
            source: IconSource::Svg(icon.clone()),
            design_size: vb_size,
            display_size: vb_size,
            color: ColorProp::TextRole(TextRole::Primary),
            mode: IconMode::Tintable,
        }
    }

    /// Create an icon from PNG data.
    ///
    /// If decoding fails, logs the error in debug mode and produces an empty icon.
    pub fn from_png(data: &'static [u8], size: f32) -> Self {
        match RasterIcon::decode_png(data) {
            Ok(icon) => {
                let name = auto_name("png", data.as_ptr() as usize);
                let mode = IconMode::Tintable;
                let upload_pixels = prepare_pixels(&icon, mode);
                Self {
                    source: IconSource::Raster {
                        name,
                        icon,
                        upload_pixels,
                    },
                    design_size: size,
                    display_size: size,
                    color: ColorProp::TextRole(TextRole::Primary),
                    mode,
                }
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("bastyde: PNG decode error: {e}");
                Self::from_path(Path::new(), size)
            }
        }
    }

    /// Create an icon from WebP data. Auto-detects static vs animated.
    ///
    /// If decoding fails, logs the error in debug mode and produces an empty icon.
    pub fn from_webp(data: &'static [u8], size: f32) -> Self {
        let mode = IconMode::Tintable;
        // Try animated first
        if let Ok(anim) = AnimatedIcon::decode_webp(data) {
            let name = auto_name("webp", data.as_ptr() as usize);
            let frame_upload_pixels: Vec<Vec<u8>> = anim
                .frames()
                .iter()
                .map(|f| prepare_pixels(f, mode))
                .collect();
            return Self {
                source: IconSource::Animated {
                    name,
                    icon: anim,
                    frame_upload_pixels,
                    frame_signal: None,
                    sprite_atlas: None,
                    anim_handle: None,
                },
                design_size: size,
                display_size: size,
                color: ColorProp::TextRole(TextRole::Primary),
                mode,
            };
        }
        // Fall back to static
        match RasterIcon::decode_webp(data) {
            Ok(icon) => {
                let name = auto_name("webp", data.as_ptr() as usize);
                let upload_pixels = prepare_pixels(&icon, mode);
                Self {
                    source: IconSource::Raster {
                        name,
                        icon,
                        upload_pixels,
                    },
                    design_size: size,
                    display_size: size,
                    color: ColorProp::TextRole(TextRole::Primary),
                    mode,
                }
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("bastyde: WebP decode error: {e}");
                Self::from_path(Path::new(), size)
            }
        }
    }

    /// Create an icon from a pre-decoded [`RasterIcon`].
    /// Accepts a reference — pixel data is copied internally.
    pub fn from_raster(icon: &RasterIcon, size: f32) -> Self {
        let name = format!("_icon_raster_{:p}", icon as *const RasterIcon);
        let mode = IconMode::Tintable;
        let upload_pixels = prepare_pixels(icon, mode);
        Self {
            source: IconSource::Raster {
                name,
                icon: icon.clone(),
                upload_pixels,
            },
            design_size: size,
            display_size: size,
            color: ColorProp::TextRole(TextRole::Primary),
            mode,
        }
    }

    /// Create an icon from a pre-decoded [`AnimatedIcon`].
    /// Accepts a reference — frame data is copied internally.
    pub fn from_animated(icon: &AnimatedIcon, size: f32) -> Self {
        let name = format!("_icon_anim_{:p}", icon as *const AnimatedIcon);
        let mode = IconMode::Tintable;
        let frame_upload_pixels: Vec<Vec<u8>> = icon
            .frames()
            .iter()
            .map(|f| prepare_pixels(f, mode))
            .collect();
        Self {
            source: IconSource::Animated {
                name,
                icon: icon.clone(),
                frame_upload_pixels,
                frame_signal: None,
                sprite_atlas: None,
                anim_handle: None,
            },
            design_size: size,
            display_size: size,
            color: ColorProp::TextRole(TextRole::Primary),
            mode,
        }
    }

    /// Set the icon rendering mode (tintable or full-color).
    /// Re-computes cached pixel data for raster/animated icons.
    pub fn mode(mut self, mode: IconMode) -> Self {
        if self.mode == mode {
            return self;
        }
        self.mode = mode;
        match &mut self.source {
            IconSource::Raster {
                icon,
                upload_pixels,
                ..
            } => {
                *upload_pixels = prepare_pixels(icon, mode);
            }
            IconSource::Animated {
                icon,
                frame_upload_pixels,
                sprite_atlas,
                anim_handle,
                ..
            } => {
                *frame_upload_pixels = icon
                    .frames()
                    .iter()
                    .map(|f| prepare_pixels(f, mode))
                    .collect();
                // Invalidate the atlas — pixels bake the mode
                // (Tintable pre-applies the alpha mask, FullColor
                // keeps raw RGBA); the next build() repacks.
                *sprite_atlas = None;
                *anim_handle = None;
            }
            IconSource::Path(_) | IconSource::Svg(_) => {}
        }
        self
    }

    /// Set the tint. Accepts any `impl Into<ColorProp>`:
    ///
    /// - A raw `Color` — a frozen literal.
    /// - A [`TextRole`] / `SurfaceRole` / `BorderRole` — resolved against
    ///   the theme at paint time (reactive across theme switches).
    /// - A `Signal<Color>` — reactive state (usually interaction-driven).
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = color.into();
        self
    }

    /// Compatibility shim. Prefer `.color(signal)` or `.color(role)` in new code.
    pub fn bind_color(self, state: impl Into<ColorProp>) -> Self {
        self.color(state)
    }

    /// Set the display size of the icon. The path/image is scaled to fit
    /// this size during rendering. This does not affect the design-time
    /// coordinate space — SVG paths scale correctly.
    pub fn icon_size(mut self, size: f32) -> Self {
        self.display_size = size;
        self
    }

    /// Create a scaled copy of the path to fit within the given bounds.
    fn scaled_path(&self, bounds: Rect) -> Path {
        // For SVG sources, use SvgIcon::to_path_in_rect which handles
        // viewBox offset and aspect-ratio-preserving scaling.
        if let IconSource::Svg(icon) = &self.source {
            return icon.to_path_in_rect(bounds);
        }
        let path = match &self.source {
            IconSource::Path(p) => p,
            _ => return Path::new(),
        };
        if path.is_empty() {
            return path.clone();
        }
        let scale_x = bounds.width / self.design_size;
        let scale_y = bounds.height / self.design_size;
        let offset_x = bounds.x;
        let offset_y = bounds.y;

        let mut scaled = Path::new();
        for cmd in &path.commands {
            match *cmd {
                PathCommand::MoveTo(p) => {
                    scaled.move_to(Point::new(
                        p.x * scale_x + offset_x,
                        p.y * scale_y + offset_y,
                    ));
                }
                PathCommand::LineTo(p) => {
                    scaled.line_to(Point::new(
                        p.x * scale_x + offset_x,
                        p.y * scale_y + offset_y,
                    ));
                }
                PathCommand::QuadTo { control, to } => {
                    scaled.quad_to(
                        Point::new(
                            control.x * scale_x + offset_x,
                            control.y * scale_y + offset_y,
                        ),
                        Point::new(to.x * scale_x + offset_x, to.y * scale_y + offset_y),
                    );
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    scaled.cubic_to(
                        Point::new(
                            control1.x * scale_x + offset_x,
                            control1.y * scale_y + offset_y,
                        ),
                        Point::new(
                            control2.x * scale_x + offset_x,
                            control2.y * scale_y + offset_y,
                        ),
                        Point::new(to.x * scale_x + offset_x, to.y * scale_y + offset_y),
                    );
                }
                PathCommand::ArcTo {
                    rect,
                    start_angle,
                    sweep_angle,
                } => {
                    scaled.arc_to(
                        Rect::new(
                            rect.x * scale_x + offset_x,
                            rect.y * scale_y + offset_y,
                            rect.width * scale_x,
                            rect.height * scale_y,
                        ),
                        start_angle,
                        sweep_angle,
                    );
                }
                PathCommand::Close => {
                    scaled.close();
                }
            }
        }
        scaled
    }

    /// Paint a raster icon into the canvas using pre-computed upload pixels.
    fn paint_raster(
        &self,
        bounds: Rect,
        canvas: &mut Canvas,
        name: &str,
        width: u32,
        height: u32,
        upload_pixels: &[u8],
        color: Color,
    ) {
        if !canvas.has_pending_image(name) {
            canvas.ensure_image_registered(name, width, height, Cow::Owned(upload_pixels.to_vec()));
        }
        match self.mode {
            IconMode::Tintable => canvas.draw_tinted_image(bounds, name, color),
            IconMode::FullColor => canvas.draw_image(bounds, name),
        }
    }
}

impl std::fmt::Debug for IconWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconWidget")
            .field("display_size", &self.display_size)
            .field("mode", &self.mode)
            .finish()
    }
}

impl Widget for IconWidget {
    fn build(
        &mut self,
        ctx: &mut bastyde_core::build_context::BuildContext,
    ) -> Vec<bastyde_core::widget_id::WidgetId> {
        // Register color binding
        {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            self.color.register_if_bound(
                self_id,
                registry,
                bastyde_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // For animated icons: prefer the shader-driven sprite path
        // (paint() emits ONE AnimatedQuad and doesn't re-run per
        // frame; the GPU samples the current frame from a packed
        // atlas). When reduced-motion is on, fall back to the static
        // first-frame render — no animation scheduled at all.
        let mode = self.mode;
        let icon_color = self.color.clone();
        if let IconSource::Animated {
            name,
            icon,
            frame_upload_pixels,
            frame_signal,
            sprite_atlas,
            anim_handle,
        } = &mut self.source
        {
            if ctx.prefers_reduced_motion() {
                *frame_signal = None;
                *sprite_atlas = None;
                *anim_handle = None;
            } else {
                // Pack frames into an atlas once; reuse across rebuilds.
                if sprite_atlas.is_none() {
                    *sprite_atlas = build_sprite_atlas(name, icon, frame_upload_pixels);
                }
                if let Some(atlas) = sprite_atlas.as_ref() {
                    // Tintable icons bake an alpha mask in the pixel
                    // buffer, so the shader must multiply by the
                    // widget's color to get the final tint. FullColor
                    // icons pass pixels through untouched (no tint).
                    let tint = match mode {
                        IconMode::Tintable => Some(icon_color),
                        IconMode::FullColor => None,
                    };
                    *anim_handle = Some(ctx.animated_quad(AnimatedQuadKind::SpriteCycle {
                        image_name: atlas.name.clone(),
                        frame_count: icon.frame_count() as u32,
                        cols: atlas.cols,
                        rows: atlas.rows,
                        period: icon.total_duration(),
                        tint,
                    }));
                    // Shader drives everything now — drop the legacy
                    // signal so the scheduler doesn't tick it.
                    *frame_signal = None;
                } else {
                    // Atlas build failed (e.g. zero-size frames);
                    // fall back to the legacy signal path.
                    let signal = ctx.animated_signal(0.0);
                    {
                        let self_id = ctx.self_id();
                        let registry = ctx.binding_registry();
                        signal.bind_to(
                            self_id,
                            registry,
                            bastyde_core::binding::BindingLevel::RepaintOnly,
                        );
                    }
                    let frame_count = icon.frame_count() as f32;
                    let period = icon.total_duration();
                    signal.animate_looping(
                        frame_count,
                        period,
                        Easing::Linear,
                        Some(std::time::Duration::from_millis(33)),
                    );
                    *frame_signal = Some(signal);
                }
            }
        }

        Vec::new()
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(self.display_size, self.display_size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self.color.resolve(ctx.theme, ctx.effective_enabled);

        match &self.source {
            IconSource::Path(_) | IconSource::Svg(_) => {
                if color.a() > 0.0 {
                    let scaled = self.scaled_path(bounds);
                    if !scaled.is_empty() {
                        canvas.fill_path(&scaled, color);
                    }
                }
            }
            IconSource::Raster {
                name,
                icon,
                upload_pixels,
            } => {
                self.paint_raster(
                    bounds,
                    canvas,
                    name,
                    icon.width(),
                    icon.height(),
                    upload_pixels,
                    color,
                );
            }
            IconSource::Animated {
                name,
                icon,
                frame_upload_pixels,
                frame_signal,
                sprite_atlas,
                anim_handle,
            } => {
                // Shader path: one AnimatedQuad — the renderer
                // samples the packed atlas at the current frame's
                // cell, driven by per-frame uniforms from the tree.
                if let (Some(atlas), Some(handle)) = (sprite_atlas, anim_handle) {
                    // Register the atlas pixels (idempotent — skipped
                    // if already pending or uploaded this frame).
                    canvas.ensure_image_registered(
                        atlas.name.clone(),
                        atlas.width,
                        atlas.height,
                        std::borrow::Cow::Owned(atlas.pixels.clone()),
                    );
                    canvas.draw_animated_quad(
                        bounds,
                        handle.slot(),
                        AnimatedQuadClass::Sprite {
                            image_name: atlas.name.clone(),
                        },
                    );
                    return;
                }
                // Legacy path: signal-driven frame index with one
                // per-frame image registration. Used when
                // reduced-motion is on (frame_signal is None →
                // frame 0 shown statically) or when atlas build
                // failed and we fell back.
                let idx = frame_signal
                    .as_ref()
                    .map(|s| (s.get() as usize).min(icon.frame_count().saturating_sub(1)))
                    .unwrap_or(0);
                let frame_name = format!("{name}_f{idx}");
                let frame = &icon.frames()[idx];
                let pixels = &frame_upload_pixels[idx];
                self.paint_raster(
                    bounds,
                    canvas,
                    &frame_name,
                    frame.width(),
                    frame.height(),
                    pixels,
                    color,
                );
            }
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Icons are typically decorative — the parent widget sets the semantic role.
    }
}

/// Pack animated-icon frames into a single square-ish sprite atlas.
/// The returned `SpriteAtlas` carries the packed pixels, grid layout,
/// and a placeholder handle (`handle.slot() == 0`) that `build()`
/// overwrites with the real scheduler-issued handle before paint runs.
///
/// Frames are laid out row-major: frame index `i` occupies cell
/// `(i % cols, i / cols)`. Unused tail cells (when `frame_count <
/// cols * rows`) are left zeroed — the shader clamps the sampled
/// frame index so it never reads past the last frame.
///
/// Returns `None` when the frames have zero dimensions; the caller
/// falls back to the legacy signal-driven path.
fn build_sprite_atlas(
    name: &str,
    icon: &AnimatedIcon,
    frame_pixels: &[Vec<u8>],
) -> Option<SpriteAtlas> {
    let frames = icon.frames();
    if frames.is_empty() {
        return None;
    }
    let frame_w = frames[0].width();
    let frame_h = frames[0].height();
    if frame_w == 0 || frame_h == 0 {
        return None;
    }

    let n = frames.len() as u32;
    let cols = (n as f32).sqrt().ceil() as u32;
    let rows = n.div_ceil(cols);
    let atlas_w = cols * frame_w;
    let atlas_h = rows * frame_h;
    let mut pixels = vec![0u8; (atlas_w * atlas_h * 4) as usize];

    for (i, cell) in frame_pixels.iter().enumerate() {
        let i = i as u32;
        let col = i % cols;
        let row = i / cols;
        let dst_x = col * frame_w;
        let dst_y = row * frame_h;
        // Copy row-by-row; source is tightly packed (frame_w × 4 bytes
        // per row), destination stride is atlas_w × 4.
        for y in 0..frame_h {
            let src_start = (y * frame_w * 4) as usize;
            let src_end = src_start + (frame_w * 4) as usize;
            if src_end > cell.len() {
                break; // truncated frame — shouldn't happen, but be defensive
            }
            let dst_start = (((dst_y + y) * atlas_w + dst_x) * 4) as usize;
            let dst_end = dst_start + (frame_w * 4) as usize;
            pixels[dst_start..dst_end].copy_from_slice(&cell[src_start..src_end]);
        }
    }

    Some(SpriteAtlas {
        name: format!("{name}_sprite_atlas"),
        pixels,
        width: atlas_w,
        height: atlas_h,
        cols,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn icon_intrinsic_size() {
        let mut tree = WidgetTree::new();
        let icon = tree.add(IconWidget::checkmark(24.0));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(icon);
        assert!((b.width - 24.0).abs() < 0.01);
        assert!((b.height - 24.0).abs() < 0.01);
    }

    #[test]
    fn icon_custom_size() {
        let mut tree = WidgetTree::new();
        let icon = tree.add(IconWidget::chevron_down(16.0));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(icon);
        assert!((b.width - 16.0).abs() < 0.01);
        assert!((b.height - 16.0).abs() < 0.01);
    }

    #[test]
    fn icon_paints_path() {
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::checkmark(24.0).color(Color::BLACK));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(!frame.paths.is_empty(), "icon should render a path");
    }

    #[test]
    fn empty_path_does_not_paint() {
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_path(Path::new(), 24.0).color(Color::BLACK));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(frame.paths.is_empty(), "empty path should not render");
    }

    #[test]
    fn icon_from_svg() {
        let svg = r#"<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
            <path d="M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z"/>
        </svg>"#;
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_svg(svg).color(Color::BLACK));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(!frame.paths.is_empty(), "SVG icon should render a path");
    }

    #[test]
    fn icon_from_svg_invalid_fallback() {
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_svg("<not-svg>"));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        // Invalid SVG should produce empty icon, no panic
        assert!(frame.paths.is_empty());
    }

    #[test]
    fn icon_mode_default_is_tintable() {
        let icon = IconWidget::checkmark(24.0);
        assert_eq!(icon.mode, IconMode::Tintable);
    }

    #[test]
    fn icon_mode_can_be_set() {
        let icon = IconWidget::checkmark(24.0).mode(IconMode::FullColor);
        assert_eq!(icon.mode, IconMode::FullColor);
    }

    #[test]
    fn raster_icon_paints_image() {
        let icon = RasterIcon::from_raw(vec![255; 4], 1, 1);
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_raster(&icon, 24.0).color(Color::BLACK));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        // Raster icon should produce an image draw command
        assert!(
            !frame.images.is_empty(),
            "raster icon should render an image"
        );
    }

    #[test]
    fn raster_icon_tintable_has_tint() {
        let icon = RasterIcon::from_raw(vec![255; 4], 1, 1);
        let mut tree = WidgetTree::new();
        tree.add(
            IconWidget::from_raster(&icon, 24.0)
                .color(Color::from_hex("#FF0000"))
                .mode(IconMode::Tintable),
        );
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(
            frame.images[0].tint.is_some(),
            "tintable icon should have tint"
        );
    }

    #[test]
    fn raster_icon_fullcolor_no_tint() {
        let icon = RasterIcon::from_raw(vec![255; 4], 1, 1);
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_raster(&icon, 24.0).mode(IconMode::FullColor));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(
            frame.images[0].tint.is_none(),
            "full-color icon should not have tint"
        );
    }

    // ── Enabled-state-aware role substitution ─────────────────────
    //
    // When the leaf paints with a role-based ColorProp and any
    // ancestor's arena `enabled_state` resolves to false, the leaf
    // must substitute `TextRole::Disabled` automatically — this is
    // the lynchpin of the "composites stop owning enabled state"
    // architecture and the fix for the format-toolbar bug where
    // table-op IconButtons stayed full-color when `is_in_table` was
    // false.

    /// Color of the single rendered `path` for an `IconWidget` from
    /// `from_path`. The path-icon code puts the resolved fill on
    /// `PathEntry.color`, distinct from raster's `image.tint`.
    fn path_icon_color(frame: &bastyde_canvas::RenderFrame) -> [f32; 4] {
        frame
            .paths
            .first()
            .map(|p| p.color)
            .expect("path icon should render at least one path")
    }

    #[test]
    fn role_based_icon_uses_text_disabled_when_self_disabled() {
        // Direct case: bind `enabled_when` on the IconWidget itself.
        // Default role is `TextRole::Primary`; when disabled the leaf
        // must resolve to `theme.colors.text_disabled`.
        let mut tree = WidgetTree::new();
        let theme = bastyde_core::presets::intui::light();
        tree.set_theme(theme.clone());

        let icon = tree.add(IconWidget::checkmark(24.0));
        tree.enabled_when(icon, false);
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        let color = path_icon_color(&frame);

        let expected = theme.colors.text_disabled.to_array();
        assert_eq!(
            color, expected,
            "default-role IconWidget under enabled_when(false) must paint at text_disabled, got {color:?}"
        );
    }

    #[test]
    fn role_based_icon_uses_text_primary_when_self_enabled() {
        let mut tree = WidgetTree::new();
        let theme = bastyde_core::presets::intui::light();
        tree.set_theme(theme.clone());

        tree.add(IconWidget::checkmark(24.0));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        let color = path_icon_color(&frame);

        let expected = theme.colors.text_primary.to_array();
        assert_eq!(
            color, expected,
            "default-role IconWidget without enabled_state must paint at text_primary, got {color:?}"
        );
    }

    #[test]
    fn role_based_icon_flips_when_bound_signal_flips_without_rebuild() {
        // The FormatToolbar bug as a unit test. Bind a Signal<bool>
        // via `enabled_when`; flip it after layout+render; re-render;
        // verify the leaf's color flipped from primary to disabled
        // and back to primary, without any rebuild.
        use bastyde_core::signal::Signal;

        let mut tree = WidgetTree::new();
        let theme = bastyde_core::presets::intui::light();
        tree.set_theme(theme.clone());

        let is_enabled = Signal::new(true);
        let icon = tree.add(IconWidget::checkmark(24.0));
        tree.enabled_when(icon, is_enabled.clone());

        tree.layout(SizeProposal::exact(24.0, 24.0));
        let primary = theme.colors.text_primary.to_array();
        let disabled = theme.colors.text_disabled.to_array();
        assert_eq!(path_icon_color(&tree.render()), primary, "starts primary");

        is_enabled.set(false);
        tree.layout(SizeProposal::exact(24.0, 24.0));
        assert_eq!(
            path_icon_color(&tree.render()),
            disabled,
            "after flipping signal to false the leaf must repaint at the disabled color"
        );

        is_enabled.set(true);
        tree.layout(SizeProposal::exact(24.0, 24.0));
        assert_eq!(
            path_icon_color(&tree.render()),
            primary,
            "flipping back to true must re-resolve to primary"
        );
    }

    #[test]
    fn explicit_color_does_not_dim_when_disabled() {
        // The static-opt-out contract: when the caller passed a
        // literal `Color`, role substitution is bypassed entirely.
        // A disabled subtree with an explicit-color icon keeps the
        // caller's literal — same model as Static() everywhere else.
        let mut tree = WidgetTree::new();
        let red = Color::from_hex("#FF0000");
        let icon = tree.add(IconWidget::checkmark(24.0).color(red));
        tree.enabled_when(icon, false);
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        let color = path_icon_color(&frame);
        assert_eq!(
            color,
            red.to_array(),
            "explicit-color icons must NOT auto-dim when disabled — caller picked the literal, framework respects it"
        );
    }
}
