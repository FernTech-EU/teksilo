// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Off-screen widget snapshots — the shared engine behind the toolbar's
//! "Export PNG" button and the documentation image exporter.
//!
//! A [`Shooter`] owns the three expensive, reusable pieces: the wgpu
//! device/queue, the production [`teksilo_render::Renderer`], and one
//! [`SharedTypesetter`]. Building them costs ~100 ms, so a batch export of
//! the whole catalog constructs a single `Shooter` and calls
//! [`Shooter::capture`] once per widget.
//!
//! Two details make the difference between a usable snapshot and the
//! chrome-only, text-free image the naive path produces:
//!
//! 1. **A real text backend.** A bare `WidgetTree` has no `TextBackend`, so
//!    every label measures and paints as nothing. The shooter installs a
//!    `SharedTypesetter` with the bundled font.
//! 2. **The glyph atlas upload.** Glyphs are rasterized into the
//!    typesetter's atlas during layout/paint; the renderer only knows about
//!    them once `Renderer::upload_atlas` has pushed the pixels to the GPU.
//!    The live app does this every frame ([`teksilo_app`]); an offscreen
//!    capture must do it too, *after* `WidgetTree::render` has shaped the
//!    frame and *before* `Renderer::render` consumes it.
//!
//! Sizing is content-driven: the subtree is laid out once at the maximum
//! canvas, measured with `WidgetTree::measure_root_intrinsic` (the
//! size-to-content window path — every widget reports the size it *wants*
//! at that bound), clamped into
//! [`ShotOptions::min_size`]/[`ShotOptions::max_size`], then laid out again
//! at that exact size. A greedy widget (a `ScrollArea`, a `ListView`)
//! reports the whole proposal back, so callers pin those with
//! [`ShotOptions::exact_size`].

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use teksilo_canvas::SizeProposal;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_render::test_support;
use teksilo_text::SharedTypesetter;
use teksilo_widgets::primitives::Padding;

/// Texture format for every capture. Matches the renderer's own default
/// so the pipelines' sRGB handling is identical to an on-screen frame.
const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// How to frame one capture. All sizes are **logical** pixels; the output
/// texture is `size × scale`.
#[derive(Debug, Clone)]
pub struct ShotOptions {
    /// Inset painted in the theme background around the widget.
    pub padding: f32,
    /// Lower clamp for the content-driven size.
    pub min_size: (f32, f32),
    /// Upper clamp for the content-driven size.
    pub max_size: (f32, f32),
    /// Canvas width handed to a widget that turns out to be greedy (it
    /// answers a bounded-width measurement with the whole bound). Without
    /// it a `Slider` or a `Toolbar` would be published at the full
    /// [`ShotOptions::max_size`] width.
    pub preferred_width: f32,
    /// Pin the canvas instead of measuring the widget. Needed for greedy
    /// widgets that fill whatever they are given.
    pub exact_size: Option<(f32, f32)>,
}

impl Default for ShotOptions {
    fn default() -> Self {
        Self {
            padding: 16.0,
            min_size: (96.0, 48.0),
            max_size: (880.0, 620.0),
            preferred_width: 520.0,
            exact_size: None,
        }
    }
}

impl ShotOptions {
    /// Pin the canvas to an exact logical size (padding still applies
    /// inside it).
    pub fn with_exact_size(mut self, width: f32, height: f32) -> Self {
        self.exact_size = Some((width, height));
        self
    }
}

/// One rendered snapshot: tightly packed RGBA8 plus the fraction of pixels
/// that differ from the background.
pub struct Shot {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Share of pixels that differ from the top-left (background) pixel.
    /// A widget that paints nothing — a `Spacer`, an invisible layout
    /// wrapper — lands near `0.0`, which lets a batch exporter drop the
    /// image rather than publish an empty rectangle.
    pub ink_ratio: f32,
}

/// Reusable offscreen renderer. Construct once, capture many.
pub struct Shooter {
    renderer: teksilo_render::Renderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
    typesetter: SharedTypesetter,
    atlas_version: u64,
    scale: f32,
}

impl Shooter {
    /// Acquire a wgpu adapter and build the renderer + typesetter.
    /// `scale` is the HiDPI factor every capture renders at — it is baked
    /// into the typesetter's glyph rasterization, so it belongs to the
    /// shooter rather than to an individual [`ShotOptions`].
    pub fn new(scale: f32) -> Result<Self, String> {
        let (renderer, device, queue) =
            match pollster::block_on(test_support::create_test_renderer("teksilo preview shot")) {
                Some(t) => t,
                None => {
                    return Err(
                        "wgpu adapter unavailable — no GPU backend present for snapshot rendering"
                            .into(),
                    );
                }
            };
        install_framework_locales();
        let typesetter = SharedTypesetter::new_with_default_font();
        typesetter.set_scale_factor(scale);
        Ok(Self {
            renderer,
            device,
            queue,
            typesetter,
            atlas_version: 0,
            scale,
        })
    }

    /// Render one widget against `theme` and read the result back.
    pub fn capture(
        &mut self,
        widget: Box<dyn Widget>,
        theme: Theme,
        opts: &ShotOptions,
    ) -> Result<Shot, String> {
        let mut tree = WidgetTree::new()
            .with_theme(theme)
            .with_text_backend(self.typesetter.as_text_backend());
        // Editable-text widgets (TextInput, SpinBox, the colour inputs,
        // RichTextEditor) shape through their own `RichTextEngine`, and
        // reach for the app's typesetter as *app state* — without it they
        // fall back to a private engine whose glyphs land in a second,
        // never-uploaded atlas and paint as garbage. Register it the way
        // `teksilo-app` does.
        let mut app_state: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
        app_state.insert(
            TypeId::of::<SharedTypesetter>(),
            Box::new(self.typesetter.clone()),
        );
        tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(app_state)));
        // Glyphs rasterize at `scale`; widgets that consult
        // `LayoutContext::scale_factor` must agree with the typesetter.
        tree.set_device_scale_factor(self.scale);

        let inner = tree.add_boxed(widget);
        // The insets are *bound*, not fixed: `Padding` stretches its child
        // to `bounds − insets`, so a canvas larger than the content would
        // smear a 24 dp icon across the whole frame. Widening the inset
        // instead of the child is what centres an under-sized widget in
        // the minimum canvas (see `pad_x_final` / `pad_y_final` below).
        let pad_x = Signal::new(opts.padding);
        let pad_y = Signal::new(opts.padding);
        let _root = tree.add(
            Padding::new(pad_y.clone(), pad_x.clone(), pad_y.clone(), pad_x.clone())
                .child_id(inner),
        );

        let base = opts.padding * 2.0;
        let max_w = opts.max_size.0 + base;
        let max_h = opts.max_size.1 + base;
        // Content size, excluding the insets.
        let (content_w, content_h) = match opts.exact_size {
            Some((w, h)) => (w, h),
            None => {
                // The warm-up pass at the maximum canvas builds the
                // subtree and shapes its text — a widget measured before
                // its first build reports nothing.
                tree.layout(SizeProposal::exact(max_w, max_h));
                // Measure height-for-width: a *bounded width, open height*
                // proposal. Both alternatives are wrong here — a fully
                // open proposal makes a distributing stack collapse its
                // shrinkable children (a `Checkbox` measures 68 dp instead
                // of 146 and ellipsizes its own label), while an exact
                // proposal is simply echoed back on the height axis.
                let measure = |tree: &WidgetTree, w: f32| {
                    tree.measure_root_intrinsic(SizeProposal::with_width(w))
                        .unwrap_or(teksilo_canvas::Size::new(0.0, 0.0))
                };
                let mut natural = measure(&tree, max_w);
                if natural.width >= max_w - 0.5 {
                    // The widget filled whatever it was offered. Re-measure
                    // at a documentation-sized canvas so a greedy control
                    // isn't published 880 dp wide, and take its
                    // height-for-width at that narrower width.
                    let preferred = (opts.preferred_width + base).min(max_w);
                    natural = measure(&tree, preferred);
                    natural.width = preferred;
                }
                (
                    clamp_extent(natural.width - base, 1.0, opts.max_size.0),
                    clamp_extent(natural.height - base, 1.0, opts.max_size.1),
                )
            }
        };

        // Reach the minimum canvas by widening the insets, never by
        // stretching the widget.
        let pad_x_final = opts.padding.max((opts.min_size.0 - content_w) / 2.0);
        let pad_y_final = opts.padding.max((opts.min_size.1 - content_h) / 2.0);
        pad_x.set(pad_x_final);
        pad_y.set(pad_y_final);
        let logical_w = (content_w + pad_x_final * 2.0).min(max_w);
        let logical_h = (content_h + pad_y_final * 2.0).min(max_h);

        tree.layout(SizeProposal::exact(logical_w, logical_h));
        let frame = tree.render();

        // Glyphs were rasterized during the layout/render above; push the
        // atlas before the renderer resolves the frame's glyph quads.
        let atlas = self
            .typesetter
            .bridge()
            .borrow_mut()
            .atlas_info(self.atlas_version);
        if !atlas.pixels.is_empty() && atlas.width > 0 && atlas.height > 0 {
            self.renderer
                .upload_atlas(atlas.width, atlas.height, &atlas.pixels);
            self.atlas_version = atlas.version;
        }

        let clear = teksilo_render::vertex::srgb_to_linear_rgba(
            tree.theme().colors.surface_main.to_array(),
        );

        let width = ((logical_w * self.scale).round() as u32).max(1);
        let height = ((logical_h * self.scale).round() as u32).max(1);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("teksilo preview shot texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render(&frame, &view, self.scale, width, height, clear);

        let rgba =
            test_support::read_texture_rgba(&self.device, &self.queue, &texture, width, height);
        let ink_ratio = ink_ratio(&rgba);
        Ok(Shot {
            rgba,
            width,
            height,
            ink_ratio,
        })
    }
}

/// Install an i18n manager carrying `teksilo-widgets`' own `.ftl`
/// catalogue, unless the process already has one.
///
/// Widget-internal labels go through `tr!`, which resolves against the
/// thread-local manager and falls back to the *message key* when none is
/// installed — so a `Calendar` captured without this renders
/// "calendar-month-label" where a month name belongs, and the colour
/// picker's channel fields read "color-picker-red-short".
fn install_framework_locales() {
    if teksilo_i18n::thread_local::with_active(|_| ()).is_some() {
        return;
    }
    let cfg =
        teksilo_i18n::I18nConfig::new().framework_locales(teksilo_widgets::framework_locales());
    let mgr = teksilo_i18n::I18nManager::from_config(&cfg);
    teksilo_i18n::thread_local::install(mgr);
}

/// Clamp a measured extent, treating a degenerate or non-finite intrinsic
/// (what a greedy widget reports at an open proposal) as "use the minimum".
fn clamp_extent(measured: f32, min: f32, max: f32) -> f32 {
    if !measured.is_finite() || measured <= 0.0 {
        return min;
    }
    measured.clamp(min, max)
}

/// Fraction of pixels differing from the top-left pixel. The corner sits
/// inside the padding, so it is the background by construction.
fn ink_ratio(rgba: &[u8]) -> f32 {
    if rgba.len() < 4 {
        return 0.0;
    }
    let bg = [rgba[0], rgba[1], rgba[2], rgba[3]];
    let total = rgba.len() / 4;
    let differing = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| {
            px.iter()
                .zip(bg.iter())
                .any(|(a, b)| a.abs_diff(*b) > BACKGROUND_TOLERANCE)
        })
        .count();
    differing as f32 / total as f32
}

/// Per-channel tolerance when deciding whether a pixel is "background".
/// Generous enough to ignore the renderer's sRGB rounding, tight enough
/// that a subtle 1 dp divider still counts as ink.
const BACKGROUND_TOLERANCE: u8 = 6;

/// Encode a tightly packed RGBA8 buffer as a PNG.
pub fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create_dir_all {parent:?}: {e}"))?;
    }
    let file = std::fs::File::create(path).map_err(|e| format!("create {path:?}: {e}"))?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png write: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_extent_treats_degenerate_intrinsics_as_minimum() {
        assert_eq!(clamp_extent(0.0, 100.0, 800.0), 100.0);
        assert_eq!(clamp_extent(f32::NAN, 100.0, 800.0), 100.0);
        assert_eq!(clamp_extent(f32::INFINITY, 100.0, 800.0), 100.0);
        assert_eq!(clamp_extent(400.0, 100.0, 800.0), 400.0);
        assert_eq!(clamp_extent(9000.0, 100.0, 800.0), 800.0);
    }

    #[test]
    fn ink_ratio_is_zero_for_a_uniform_image() {
        let rgba = vec![32u8; 4 * 64];
        assert_eq!(ink_ratio(&rgba), 0.0);
    }

    #[test]
    fn ink_ratio_counts_pixels_that_differ_from_the_corner() {
        let mut rgba = vec![32u8; 4 * 4];
        rgba[4..8].copy_from_slice(&[200, 200, 200, 255]);
        assert!((ink_ratio(&rgba) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn ink_ratio_ignores_srgb_rounding_noise() {
        let mut rgba = vec![32u8; 4 * 4];
        rgba[4..8].copy_from_slice(&[34, 33, 32, 32]);
        assert_eq!(ink_ratio(&rgba), 0.0);
    }
}
