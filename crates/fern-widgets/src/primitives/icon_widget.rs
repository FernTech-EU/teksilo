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

use fern_canvas::{AnimatedIcon, Canvas, Path, PathCommand, Point, Rect, RasterIcon, Size, SizeProposal};
use fern_canvas::svg::SvgIcon;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::color_prop::ColorProp;
use fern_core::signal::{Prop, Signal};
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_tokens::{Color, Easing, TextRole};

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
    /// `frame_upload_pixels` holds pre-computed pixels per frame.
    /// `frame_signal` is a looping animated Signal<f32> from 0 to frame_count,
    /// driven by the animation scheduler.
    Animated {
        name: String,
        icon: AnimatedIcon,
        frame_upload_pixels: Vec<Vec<u8>>,
        frame_signal: Option<Signal<f32>>,
    },
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
                eprintln!("fern-ui: SVG parse error: {e}");
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
                    source: IconSource::Raster { name, icon, upload_pixels },
                    design_size: size,
                    display_size: size,
                    color: ColorProp::TextRole(TextRole::Primary),
                    mode,
                }
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("fern-ui: PNG decode error: {e}");
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
                    frame_signal: None, // initialized in build()
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
                    source: IconSource::Raster { name, icon, upload_pixels },
                    design_size: size,
                    display_size: size,
                    color: ColorProp::TextRole(TextRole::Primary),
                    mode,
                }
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                eprintln!("fern-ui: WebP decode error: {e}");
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
            source: IconSource::Raster { name, icon: icon.clone(), upload_pixels },
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
                frame_signal: None, // initialized in build()
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
            IconSource::Raster { icon, upload_pixels, .. } => {
                *upload_pixels = prepare_pixels(icon, mode);
            }
            IconSource::Animated { icon, frame_upload_pixels, .. } => {
                *frame_upload_pixels = icon
                    .frames()
                    .iter()
                    .map(|f| prepare_pixels(f, mode))
                    .collect();
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
            canvas.ensure_image_registered(
                name,
                width,
                height,
                Cow::Owned(upload_pixels.to_vec()),
            );
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
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        // Register color binding
        {
            let self_id = ctx.self_id();
            let registry = ctx.binding_registry();
            self.color.register_if_bound(
                self_id,
                registry,
                fern_core::binding::BindingLevel::RepaintOnly,
            );
        }

        // For animated icons: create a looping animation signal that
        // drives frame cycling. Capped at 30 fps. Skipped entirely
        // under the OS reduced-motion preference — animated icons are
        // decorative, and staying on the first frame respects the
        // user's stated motion tolerance while drawing zero CPU/GPU.
        if let IconSource::Animated { icon, frame_signal, .. } = &mut self.source {
            if ctx.prefers_reduced_motion() {
                *frame_signal = None;
            } else {
                let signal = ctx.animated_signal(0.0);
                {
                    let self_id = ctx.self_id();
                    let registry = ctx.binding_registry();
                    signal.bind_to(
                        self_id,
                        registry,
                        fern_core::binding::BindingLevel::RepaintOnly,
                    );
                }
                let frame_count = icon.frame_count() as f32;
                let period = icon.total_duration();
                signal.animate_looping(
                    frame_count,
                    period,
                    Easing::Linear,
                    Some(std::time::Duration::from_millis(33)), // 30fps cap
                );
                *frame_signal = Some(signal);
            }
        }

        Vec::new()
    }

    fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        Size::new(self.display_size, self.display_size)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self.color.resolve(ctx.theme);

        match &self.source {
            IconSource::Path(_) | IconSource::Svg(_) => {
                if color.a() > 0.0 {
                    let scaled = self.scaled_path(bounds);
                    if !scaled.is_empty() {
                        canvas.fill_path(&scaled, color);
                    }
                }
            }
            IconSource::Raster { name, icon, upload_pixels } => {
                self.paint_raster(bounds, canvas, name, icon.width(), icon.height(), upload_pixels, color);
            }
            IconSource::Animated {
                name, icon, frame_upload_pixels, frame_signal, ..
            } => {
                let idx = frame_signal
                    .as_ref()
                    .map(|s| (s.get() as usize).min(icon.frame_count().saturating_sub(1)))
                    .unwrap_or(0);
                let frame_name = format!("{name}_f{idx}");
                let frame = &icon.frames()[idx];
                let pixels = &frame_upload_pixels[idx];
                self.paint_raster(bounds, canvas, &frame_name, frame.width(), frame.height(), pixels, color);
            }
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Icons are typically decorative — the parent widget sets the semantic role.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

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
        assert!(!frame.images.is_empty(), "raster icon should render an image");
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
        assert!(frame.images[0].tint.is_some(), "tintable icon should have tint");
    }

    #[test]
    fn raster_icon_fullcolor_no_tint() {
        let icon = RasterIcon::from_raw(vec![255; 4], 1, 1);
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_raster(&icon, 24.0).mode(IconMode::FullColor));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(frame.images[0].tint.is_none(), "full-color icon should not have tint");
    }
}
