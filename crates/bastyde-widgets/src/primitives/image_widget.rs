// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ImageWidget — displays a raster image (PNG, WebP) with a configurable
//! sizing policy, content-fit mode, and intra-box alignment.
//!
//! Unlike [`IconWidget`](super::icon_widget::IconWidget) which is designed
//! for small square tintable icons, `ImageWidget` handles arbitrary aspect
//! ratios and defaults to full-color rendering.
//!
//! # Sizing model
//!
//! Two independent concerns, mirroring Qt's `QLabel`/`QPixmap`, SwiftUI's
//! `Image`, and CSS's replaced-element model:
//!
//! 1. **Box size** — how big the widget's layout rectangle is.
//!    - [`width`](ImageWidget::width) / [`height`](ImageWidget::height) /
//!      [`size`](ImageWidget::size) pin a **fixed** logical extent. A pinned
//!      axis is *rigid*: it is reported as-is and is never scaled up to a
//!      parent's proposal (this is the SwiftUI `.frame(width:height:)` /
//!      Qt fixed-size behaviour). Pinning only one axis derives the other
//!      from the image's aspect ratio (CSS `width: Npx; height: auto`).
//!    - With no axis pinned the widget reports its **natural pixel size**.
//!      By default ([`resizable`](ImageWidget::resizable) `= true`) a
//!      constraining proposal scales that natural size down/up while
//!      preserving aspect ratio; `resizable(false)` locks it to the raw
//!      pixel dimensions (SwiftUI's default non-`.resizable()` image).
//! 2. **Content fit** — how the image pixels map into that box, via
//!    [`ImageFit`] (`Contain` / `Cover` / `Fill` / `ScaleDown` / `None`,
//!    the CSS `object-fit` set) plus
//!    [`alignment`](ImageWidget::alignment) (the CSS `object-position`
//!    equivalent) for where slack/overflow lands. Modes that overflow the
//!    box (`Cover`, and `None` on an oversized image) are clipped to the
//!    box so the image never bleeds past its layout rectangle.
//!
//! For a fixed 32×32 logo: `ImageWidget::new(icon).size(32.0, 32.0)` — the
//! box is exactly 32×32 and the artwork is letterboxed inside it
//! (`Contain`, the default).

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use bastyde_canvas::{Canvas, RasterIcon, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::environment::LayoutDirection;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};
use bastyde_tokens::Alignment;

use super::image_mask::{ImageMaskShape, apply_alpha_mask, center_crop_square};

/// How the image is fitted within its layout bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFit {
    /// Scale to fit entirely within bounds, preserving aspect ratio.
    /// May leave empty space (letterboxing).
    #[default]
    Contain,
    /// Scale to cover the entire bounds, preserving aspect ratio.
    /// May crop the image.
    Cover,
    /// Stretch to fill bounds exactly, ignoring aspect ratio.
    Fill,
    /// Like Contain but never upscales — if the image is smaller than
    /// bounds, it is centered at its natural size.
    ScaleDown,
    /// Draw the image at its natural pixel size, neither scaling up nor
    /// down. If the image is larger than the box it is cropped to the box
    /// (positioned by [`alignment`](ImageWidget::alignment)); if smaller it
    /// sits inside with empty space. CSS `object-fit: none`.
    None,
}

/// A widget that displays a raster image.
pub struct ImageWidget {
    name: String,
    width: u32,
    height: u32,
    upload_pixels: Vec<u8>,
    fit: ImageFit,
    /// Where the fitted image sits within the box when the active fit
    /// leaves slack (`Contain`/`ScaleDown`/`None` smaller than the box) or
    /// crops (`Cover`/`None` larger than the box). The CSS
    /// `object-position` analogue. Defaults to centered.
    alignment: Alignment,
    /// Optional fixed display size. A pinned axis is rigid — reported as-is
    /// and never scaled to a parent proposal. `None` means "derive": from
    /// the aspect ratio when the other axis is pinned, otherwise from the
    /// natural pixel dimensions.
    display_width: Option<f32>,
    display_height: Option<f32>,
    /// When no axis is pinned, whether a constraining proposal scales the
    /// natural size (`true`, the default) or the box stays locked to the
    /// raw pixel dimensions (`false`, SwiftUI's default non-resizable
    /// image). Has no effect once a dimension is pinned.
    resizable: bool,
    /// Accessibility description.
    alt: Option<String>,
    /// When true, hide from the accessibility tree entirely —
    /// appropriate for purely decorative images whose semantic
    /// content is already conveyed by adjacent text.
    a11y_hidden: bool,
}

impl ImageWidget {
    /// Create from a decoded [`RasterIcon`] (e.g., from `res!()`).
    pub fn new(icon: &RasterIcon) -> Self {
        let name = format!("_img_{:p}", icon as *const RasterIcon);
        Self {
            name,
            width: icon.width(),
            height: icon.height(),
            upload_pixels: icon.pixels().to_vec(),
            fit: ImageFit::Contain,
            alignment: Alignment::CENTER,
            display_width: None,
            display_height: None,
            resizable: true,
            alt: None,
            a11y_hidden: false,
        }
    }

    /// Create from raw RGBA pixel data.
    ///
    /// Each call gets a unique texture-atlas key (via a process-local
    /// atomic counter), so two `from_raw` widgets with the same
    /// dimensions but different bytes don't alias in the renderer's
    /// pending-image cache. Without this, the first writer per frame
    /// would silently win and subsequent ones would render the wrong
    /// pixels — a latent bug fixed alongside the dynamic-image use
    /// cases that need many short-lived `from_raw` widgets.
    pub fn from_raw(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        static NEXT_RAW_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_RAW_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            name: format!("_img_raw_{id}_{width}x{height}"),
            width,
            height,
            upload_pixels: pixels,
            fit: ImageFit::Contain,
            alignment: Alignment::CENTER,
            display_width: None,
            display_height: None,
            resizable: true,
            alt: None,
            a11y_hidden: false,
        }
    }

    /// Apply an anti-aliased alpha mask to the image at construction
    /// time. The pixels are first centre-cropped to the shorter side
    /// (so the mask shape is geometrically consistent regardless of
    /// the source aspect ratio), then their alpha channel is
    /// modulated by the mask coverage. RGB is preserved.
    ///
    /// `Cover` fit is the natural pairing — the masked square fills
    /// the avatar/thumbnail bounds and the masked-out corners stay
    /// transparent. `Contain` works but may letterbox. The default
    /// fit (`Contain`) is left unchanged so callers explicitly pick
    /// a fit when they apply a mask.
    ///
    /// `ImageMaskShape::None` is a no-op. Re-uploading is keyed off a
    /// fresh per-mask name so the un-masked version of the same
    /// source doesn't shadow the masked one in the texture atlas.
    pub fn mask(mut self, shape: ImageMaskShape) -> Self {
        if matches!(shape, ImageMaskShape::None) {
            return self;
        }
        let (mut cropped, side) = center_crop_square(&self.upload_pixels, self.width, self.height);
        apply_alpha_mask(&mut cropped, side, side, shape);
        self.upload_pixels = cropped;
        self.width = side;
        self.height = side;
        // Bump the texture name so the old un-masked entry is
        // distinct in the per-frame `pending_images` map.
        static NEXT_MASK_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_MASK_ID.fetch_add(1, Ordering::Relaxed);
        self.name = format!("{}_masked_{id}", self.name);
        self
    }

    /// Set the content-fit mode — how the image pixels map into the box.
    /// See [`ImageFit`].
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Set where the fitted image sits within the box when the active fit
    /// leaves slack or crops (the CSS `object-position` analogue). Defaults
    /// to [`Alignment::CENTER`]. Leading/Trailing resolve against the
    /// active layout direction (RTL-aware).
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Pin a fixed display width (in logical pixels). The width axis
    /// becomes rigid — reported as-is and never scaled to a parent
    /// proposal. With no height pinned, the height derives from the
    /// image's aspect ratio (CSS `width: Npx; height: auto`).
    pub fn width(mut self, w: f32) -> Self {
        self.display_width = Some(w);
        self
    }

    /// Pin a fixed display height (in logical pixels). The height axis
    /// becomes rigid. With no width pinned, the width derives from the
    /// image's aspect ratio.
    pub fn height(mut self, h: f32) -> Self {
        self.display_height = Some(h);
        self
    }

    /// Pin both display width and height (in logical pixels). The box is
    /// exactly this size, rigid on both axes; the image content is fitted
    /// inside it via the [`fit`](Self::fit) mode. This is the
    /// fixed-size-logo case — `.size(32.0, 32.0)`.
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.display_width = Some(w);
        self.display_height = Some(h);
        self
    }

    /// Control whether, with no axis pinned, a constraining parent
    /// proposal scales the natural pixel size (`true`, the default) or the
    /// box stays locked to the raw pixel dimensions (`false`). Equivalent
    /// to opting out of SwiftUI's `.resizable()`. No effect once a
    /// dimension is pinned via [`width`](Self::width) /
    /// [`height`](Self::height) / [`size`](Self::size).
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set the accessibility alt text.
    pub fn alt(mut self, text: impl Into<String>) -> Self {
        self.alt = Some(text.into());
        self
    }

    /// Mark this image as decorative — hidden from the accessibility
    /// tree. Use when the image's semantic content is already conveyed
    /// by adjacent text (e.g. a hero image next to its caption). ARIA
    /// equivalent of `alt=""` / `role="presentation"`.
    pub fn a11y_hidden(mut self) -> Self {
        self.a11y_hidden = true;
        self
    }

    /// Natural aspect ratio (width / height).
    fn aspect_ratio(&self) -> f32 {
        if self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// Compute the image rectangle within `bounds` for the active fit mode,
    /// positioned by [`alignment`](Self::alignment). `rtl` flips the
    /// horizontal Leading/Trailing axis.
    fn fitted_rect(&self, bounds: Rect, rtl: bool) -> Rect {
        let img_w = self.width as f32;
        let img_h = self.height as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return bounds;
        }

        let (content_w, content_h) = match self.fit {
            ImageFit::Fill => (bounds.width, bounds.height),
            ImageFit::Contain => {
                let scale = (bounds.width / img_w).min(bounds.height / img_h);
                (img_w * scale, img_h * scale)
            }
            ImageFit::Cover => {
                let scale = (bounds.width / img_w).max(bounds.height / img_h);
                (img_w * scale, img_h * scale)
            }
            ImageFit::ScaleDown => {
                let scale = (bounds.width / img_w).min(bounds.height / img_h).min(1.0);
                (img_w * scale, img_h * scale)
            }
            ImageFit::None => (img_w, img_h),
        };

        let x = bounds.x + self.alignment.horizontal.resolve(content_w, bounds.width, rtl);
        let y = bounds.y + self.alignment.vertical.resolve(content_h, bounds.height);
        Rect::new(x, y, content_w, content_h)
    }
}

impl std::fmt::Debug for ImageWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageWidget")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("fit", &self.fit)
            .finish()
    }
}

impl Widget for ImageWidget {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let ar = self.aspect_ratio();

        let size = match (self.display_width, self.display_height) {
            // Both axes pinned → rigid box. The proposal cannot override an
            // explicit size; the image content is fitted inside via `fit`.
            (Some(w), Some(h)) => Size::new(w, h),
            // One axis pinned → the other derives from the aspect ratio
            // (CSS `width: Npx; height: auto`). Still rigid.
            (Some(w), None) => Size::new(w, w / ar),
            (None, Some(h)) => Size::new(h * ar, h),
            // Neither pinned → natural pixel size, scaled to a constraining
            // proposal only when `resizable` (the default).
            (None, None) => {
                let natural_w = self.width as f32;
                let natural_h = self.height as f32;
                if !self.resizable {
                    Size::new(natural_w, natural_h)
                } else {
                    match (proposal.width, proposal.height) {
                        // Both constrained: fit within, preserving aspect ratio
                        (Some(pw), Some(ph)) => {
                            let scale = (pw / natural_w).min(ph / natural_h);
                            Size::new(natural_w * scale, natural_h * scale)
                        }
                        // Width constrained: compute height from aspect ratio
                        (Some(pw), None) => Size::new(pw, pw / ar),
                        // Height constrained: compute width from aspect ratio
                        (None, Some(ph)) => Size::new(ph * ar, ph),
                        // Unconstrained: natural size
                        (None, None) => Size::new(natural_w, natural_h),
                    }
                }
            }
        };
        size.into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Only clone pixels if not already queued — avoid per-frame allocation
        if !canvas.has_pending_image(&self.name) {
            canvas.ensure_image_registered(
                &self.name,
                self.width,
                self.height,
                Cow::Owned(self.upload_pixels.clone()),
            );
        }
        let rtl = matches!(ctx.layout_direction, LayoutDirection::RightToLeft);
        let rect = self.fitted_rect(bounds, rtl);

        // Modes that overflow the box (`Cover`, or `None` on an oversized
        // image) must not bleed past the widget's layout rectangle. Clip
        // to `bounds` only when the fitted rect actually exceeds it — the
        // renderer intersects this with any ancestor clip and pops it
        // cleanly, so it composes with ScrollAreas etc. `Contain` /
        // `ScaleDown` / a within-box image never overflow, so they skip the
        // clip commands entirely.
        const EPS: f32 = 0.01;
        let overflows = rect.x < bounds.x - EPS
            || rect.y < bounds.y - EPS
            || rect.x + rect.width > bounds.x + bounds.width + EPS
            || rect.y + rect.height > bounds.y + bounds.height + EPS;
        if overflows {
            canvas.set_clip(bounds);
            canvas.draw_image(rect, &self.name);
            canvas.clear_clip();
        } else {
            canvas.draw_image(rect, &self.name);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if self.a11y_hidden {
            builder.set_hidden();
            return;
        }
        debug_assert!(
            self.alt.is_some(),
            "ImageWidget has no alt text — call .alt(\"…\") for meaningful images or .a11y_hidden() for decorative ones"
        );
        builder.set_role(bastyde_core::accesskit::Role::Image);
        if let Some(ref alt) = self.alt {
            builder.set_name(alt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn natural_size() {
        let icon = RasterIcon::from_raw(vec![255; 400], 10, 10);
        let mut tree = WidgetTree::new();
        let img = tree.add(ImageWidget::new(&icon));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(img);
        assert!((b.width - 10.0).abs() < 0.01);
        assert!((b.height - 10.0).abs() < 0.01);
    }

    #[test]
    fn explicit_display_size() {
        let icon = RasterIcon::from_raw(vec![255; 400], 10, 10);
        let mut tree = WidgetTree::new();
        let img = tree.add(ImageWidget::new(&icon).size(200.0, 100.0));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(img);
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn size_that_fits_preserves_aspect_ratio() {
        let icon = RasterIcon::from_raw(vec![255; 800], 20, 10); // 2:1 aspect
        let widget = ImageWidget::new(&icon);
        let theme = bastyde_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        // Width constrained to 100, no height constraint → 100x50
        let size = widget
            .layout_response(
                SizeProposal {
                    width: Some(100.0),
                    height: None,
                },
                &ctx,
            )
            .size;
        assert!((size.width - 100.0).abs() < 0.5, "width: {}", size.width);
        assert!((size.height - 50.0).abs() < 0.5, "height: {}", size.height);
    }

    #[test]
    fn paints_image_quad() {
        let icon = RasterIcon::from_raw(vec![255; 400], 10, 10);
        let mut tree = WidgetTree::new();
        tree.add(ImageWidget::new(&icon));
        tree.layout(SizeProposal::exact(10.0, 10.0));
        let frame = tree.render();
        assert!(!frame.images.is_empty(), "should render an image");
        assert!(frame.images[0].tint.is_none(), "should be full-color");
    }

    #[test]
    fn pending_image_registered() {
        let icon = RasterIcon::from_raw(vec![255; 400], 10, 10);
        let mut tree = WidgetTree::new();
        tree.add(ImageWidget::new(&icon));
        tree.layout(SizeProposal::exact(10.0, 10.0));
        let frame = tree.render();
        assert!(
            !frame.pending_images.is_empty(),
            "should register pending image"
        );
    }

    #[test]
    fn from_raw_unique_name_per_call() {
        // Two ImageWidgets with identical dimensions but different
        // bytes used to alias on the per-frame `pending_images` key
        // (`_img_raw_{w}x{h}`), causing one to silently render the
        // other's pixels. The atomic-counter-tagged name fixes this.
        let a = ImageWidget::from_raw(vec![255; 16], 2, 2);
        let b = ImageWidget::from_raw(vec![0; 16], 2, 2);
        assert_ne!(a.name, b.name);
    }

    #[test]
    fn fixed_size_is_rigid_against_constraining_proposal() {
        // Regression: a pinned `.size(32, 32)` must stay 32×32 no matter
        // what the parent proposes. The old layout path treated the
        // explicit size as a "natural size" and scaled it up to a
        // width-constraining proposal — a 512px source logo placed in a
        // wide title bar ballooned to the bar's full width.
        let icon = RasterIcon::from_raw(vec![255; 512 * 512 * 4], 512, 512);
        let widget = ImageWidget::new(&icon).size(32.0, 32.0);
        let theme = bastyde_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        // Wide width proposal, no height constraint — the VStack-cross-axis
        // case that triggered the skribisto bug.
        let size = widget
            .layout_response(
                SizeProposal {
                    width: Some(600.0),
                    height: None,
                },
                &ctx,
            )
            .size;
        assert!((size.width - 32.0).abs() < 0.01, "width: {}", size.width);
        assert!((size.height - 32.0).abs() < 0.01, "height: {}", size.height);
    }

    #[test]
    fn single_axis_pin_derives_other_from_aspect_ratio() {
        let icon = RasterIcon::from_raw(vec![255; 40 * 10 * 4], 40, 10); // 4:1
        let theme = bastyde_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        // Width pinned to 200 → height = 200 / 4 = 50, ignoring proposal.
        let w_pinned = ImageWidget::new(&icon)
            .width(200.0)
            .layout_response(SizeProposal::exact(999.0, 999.0), &ctx)
            .size;
        assert!((w_pinned.width - 200.0).abs() < 0.01);
        assert!((w_pinned.height - 50.0).abs() < 0.01, "h: {}", w_pinned.height);
        // Height pinned to 20 → width = 20 * 4 = 80.
        let h_pinned = ImageWidget::new(&icon)
            .height(20.0)
            .layout_response(SizeProposal::exact(999.0, 999.0), &ctx)
            .size;
        assert!((h_pinned.width - 80.0).abs() < 0.01, "w: {}", h_pinned.width);
        assert!((h_pinned.height - 20.0).abs() < 0.01);
    }

    #[test]
    fn resizable_false_locks_natural_pixel_size() {
        let icon = RasterIcon::from_raw(vec![255; 64 * 64 * 4], 64, 64);
        let theme = bastyde_core::presets::intui::light();
        let ctx = LayoutContext::for_testing(&theme);
        // Default (resizable) scales to a constraining proposal.
        let scaled = ImageWidget::new(&icon)
            .layout_response(SizeProposal::exact(16.0, 16.0), &ctx)
            .size;
        assert!((scaled.width - 16.0).abs() < 0.01);
        // resizable(false) ignores the proposal and keeps 64×64.
        let locked = ImageWidget::new(&icon)
            .resizable(false)
            .layout_response(SizeProposal::exact(16.0, 16.0), &ctx)
            .size;
        assert!((locked.width - 64.0).abs() < 0.01, "w: {}", locked.width);
        assert!((locked.height - 64.0).abs() < 0.01);
    }

    #[test]
    fn contain_centers_inside_a_wider_box() {
        // 1:1 image in a 200×100 box → 100×100 letterboxed, centered.
        let icon = RasterIcon::from_raw(vec![255; 10 * 10 * 4], 10, 10);
        let widget = ImageWidget::new(&icon).fit(ImageFit::Contain);
        let r = widget.fitted_rect(Rect::new(0.0, 0.0, 200.0, 100.0), false);
        assert!((r.width - 100.0).abs() < 0.01);
        assert!((r.height - 100.0).abs() < 0.01);
        assert!((r.x - 50.0).abs() < 0.01, "x: {}", r.x); // (200-100)/2
        assert!((r.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn alignment_positions_content_within_box() {
        use bastyde_tokens::{HAlignment, VAlignment};
        let icon = RasterIcon::from_raw(vec![255; 10 * 10 * 4], 10, 10);
        let widget = ImageWidget::new(&icon).fit(ImageFit::Contain).alignment(Alignment {
            horizontal: HAlignment::Trailing,
            vertical: VAlignment::Bottom,
        });
        let r = widget.fitted_rect(Rect::new(0.0, 0.0, 200.0, 100.0), false);
        // 100×100 pushed to bottom-trailing: x = 200-100, y = 100-100.
        assert!((r.x - 100.0).abs() < 0.01, "x: {}", r.x);
        assert!((r.y - 0.0).abs() < 0.01, "y: {}", r.y);
        // RTL flips Trailing to the left edge.
        let r_rtl = widget.fitted_rect(Rect::new(0.0, 0.0, 200.0, 100.0), true);
        assert!((r_rtl.x - 0.0).abs() < 0.01, "rtl x: {}", r_rtl.x);
    }

    #[test]
    fn fit_none_draws_at_natural_pixel_size() {
        let icon = RasterIcon::from_raw(vec![255; 20 * 20 * 4], 20, 20);
        let widget = ImageWidget::new(&icon).fit(ImageFit::None);
        // In a 8×8 box the natural 20×20 image overflows; rect stays 20×20.
        let r = widget.fitted_rect(Rect::new(0.0, 0.0, 8.0, 8.0), false);
        assert!((r.width - 20.0).abs() < 0.01);
        assert!((r.height - 20.0).abs() < 0.01);
        // Centered → negative offset (overflows on all sides).
        assert!((r.x - (-6.0)).abs() < 0.01, "x: {}", r.x); // (8-20)/2
    }

    #[test]
    fn cover_overflow_is_clipped_to_bounds() {
        // A 2:1 image covering a square box overflows horizontally and must
        // be clipped — the frame should carry a SetClip/ClearClip pair.
        let icon = RasterIcon::from_raw(vec![255; 20 * 10 * 4], 20, 10);
        let mut tree = WidgetTree::new();
        tree.add(ImageWidget::new(&icon).size(50.0, 50.0).fit(ImageFit::Cover));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let frame = tree.render();
        let has_set_clip = frame
            .draw_order
            .iter()
            .any(|c| matches!(c, bastyde_canvas::DrawCommand::SetClip(_)));
        let has_clear_clip = frame
            .draw_order
            .iter()
            .any(|c| matches!(c, bastyde_canvas::DrawCommand::ClearClip));
        assert!(has_set_clip, "Cover overflow should emit SetClip");
        assert!(has_clear_clip, "Cover overflow should emit ClearClip");
    }

    #[test]
    fn contain_within_box_emits_no_clip() {
        // Contain never overflows, so no clip commands are emitted.
        let icon = RasterIcon::from_raw(vec![255; 10 * 10 * 4], 10, 10);
        let mut tree = WidgetTree::new();
        tree.add(ImageWidget::new(&icon).size(50.0, 50.0).fit(ImageFit::Contain));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let frame = tree.render();
        let has_set_clip = frame
            .draw_order
            .iter()
            .any(|c| matches!(c, bastyde_canvas::DrawCommand::SetClip(_)));
        assert!(!has_set_clip, "Contain should not clip");
    }

    #[test]
    fn mask_circle_alpha_zero_at_corners() {
        // The reusable `.mask(Circle)` modifier — anywhere a photo
        // needs a circular crop, not just inside Avatar.
        let icon = RasterIcon::from_raw(vec![255; 32 * 32 * 4], 32, 32);
        let widget = ImageWidget::from_raw(icon.pixels().to_vec(), icon.width(), icon.height())
            .mask(ImageMaskShape::Circle);
        // After cropping to a square (already 32×32 here) and
        // masking, corner pixels have alpha 0.
        let stride = (widget.width * 4) as usize;
        let top_left_alpha = widget.upload_pixels[3];
        let top_right_alpha = widget.upload_pixels[stride - 1];
        assert_eq!(top_left_alpha, 0);
        assert_eq!(top_right_alpha, 0);
        // Centre pixel still opaque.
        let center_idx = (((widget.height / 2) * widget.width + widget.width / 2) * 4 + 3) as usize;
        assert_eq!(widget.upload_pixels[center_idx], 255);
    }

    #[test]
    fn mask_none_is_passthrough() {
        let original = vec![123, 45, 67, 200, 8, 9, 10, 200];
        let widget = ImageWidget::from_raw(original.clone(), 2, 1).mask(ImageMaskShape::None);
        assert_eq!(widget.upload_pixels, original);
    }

    #[test]
    fn mask_crops_non_square_to_square() {
        // 8×4 source → centre-cropped to 4×4 inscribed circle.
        let pixels = vec![255; 8 * 4 * 4];
        let widget = ImageWidget::from_raw(pixels, 8, 4).mask(ImageMaskShape::Circle);
        assert_eq!(widget.width, 4);
        assert_eq!(widget.height, 4);
    }

    #[test]
    fn mask_bumps_name_to_avoid_atlas_collision() {
        // The masked widget must not share a name with the un-masked
        // version, otherwise the renderer would reuse the un-masked
        // pixels in the atlas.
        let icon = RasterIcon::from_raw(vec![255; 16 * 16 * 4], 16, 16);
        let unmasked = ImageWidget::new(&icon);
        let masked = ImageWidget::new(&icon).mask(ImageMaskShape::Circle);
        assert_ne!(unmasked.name, masked.name);
    }
}
