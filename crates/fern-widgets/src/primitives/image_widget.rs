//! ImageWidget — displays a raster image (PNG, WebP) at its natural
//! aspect ratio with configurable fit modes.
//!
//! Unlike [`IconWidget`](super::icon_widget::IconWidget) which is designed
//! for small square tintable icons, `ImageWidget` handles arbitrary aspect
//! ratios and defaults to full-color rendering.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};

use fern_canvas::{Canvas, RasterIcon, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::widget::{LayoutContext, PaintContext, Widget};

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
}

/// A widget that displays a raster image.
pub struct ImageWidget {
    name: String,
    width: u32,
    height: u32,
    upload_pixels: Vec<u8>,
    fit: ImageFit,
    /// Optional fixed display size. If None, uses the image's natural
    /// pixel dimensions as logical size.
    display_width: Option<f32>,
    display_height: Option<f32>,
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
            display_width: None,
            display_height: None,
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
            display_width: None,
            display_height: None,
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
        let (mut cropped, side) =
            center_crop_square(&self.upload_pixels, self.width, self.height);
        apply_alpha_mask(&mut cropped, side, side, shape);
        self.upload_pixels = cropped;
        self.width = side;
        self.height = side;
        // Update the natural display size if the user hadn't pinned
        // one — they constructed against the original aspect ratio
        // and the centre-crop has changed both dimensions.
        if self.display_width.is_none() && self.display_height.is_none() {
            // No-op: aspect_ratio() will report 1.0 from the new w/h.
        }
        // Bump the texture name so the old un-masked entry is
        // distinct in the per-frame `pending_images` map.
        static NEXT_MASK_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_MASK_ID.fetch_add(1, Ordering::Relaxed);
        self.name = format!("{}_masked_{id}", self.name);
        self
    }

    /// Set the fit mode.
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Set a fixed display width (in logical pixels).
    pub fn width(mut self, w: f32) -> Self {
        self.display_width = Some(w);
        self
    }

    /// Set a fixed display height (in logical pixels).
    pub fn height(mut self, h: f32) -> Self {
        self.display_height = Some(h);
        self
    }

    /// Set both display width and height.
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.display_width = Some(w);
        self.display_height = Some(h);
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

    /// Compute the image rectangle within bounds based on the fit mode.
    fn fitted_rect(&self, bounds: Rect) -> Rect {
        let img_w = self.width as f32;
        let img_h = self.height as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return bounds;
        }

        match self.fit {
            ImageFit::Fill => bounds,
            ImageFit::Contain => {
                let scale = (bounds.width / img_w).min(bounds.height / img_h);
                let w = img_w * scale;
                let h = img_h * scale;
                Rect::new(
                    bounds.x + (bounds.width - w) / 2.0,
                    bounds.y + (bounds.height - h) / 2.0,
                    w,
                    h,
                )
            }
            ImageFit::Cover => {
                let scale = (bounds.width / img_w).max(bounds.height / img_h);
                let w = img_w * scale;
                let h = img_h * scale;
                Rect::new(
                    bounds.x + (bounds.width - w) / 2.0,
                    bounds.y + (bounds.height - h) / 2.0,
                    w,
                    h,
                )
            }
            ImageFit::ScaleDown => {
                let scale = (bounds.width / img_w).min(bounds.height / img_h).min(1.0);
                let w = img_w * scale;
                let h = img_h * scale;
                Rect::new(
                    bounds.x + (bounds.width - w) / 2.0,
                    bounds.y + (bounds.height - h) / 2.0,
                    w,
                    h,
                )
            }
        }
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
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        let natural_w = self.display_width.unwrap_or(self.width as f32);
        let natural_h = self.display_height.unwrap_or(self.height as f32);
        let ar = self.aspect_ratio();

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
        }.into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        // Only clone pixels if not already queued — avoid per-frame allocation
        if !canvas.has_pending_image(&self.name) {
            canvas.ensure_image_registered(
                &self.name,
                self.width,
                self.height,
                Cow::Owned(self.upload_pixels.clone()),
            );
        }
        let rect = self.fitted_rect(bounds);
        canvas.draw_image(rect, &self.name);
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
        builder.set_role(fern_core::accesskit::Role::Image);
        if let Some(ref alt) = self.alt {
            builder.set_name(alt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

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
        let theme = fern_tokens::Theme::light_default();
        let ctx = LayoutContext::for_testing(&theme);
        // Width constrained to 100, no height constraint → 100x50
        let size = widget
            .layout_response(SizeProposal { width: Some(100.0), height: None }, &ctx)
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
        assert!(!frame.pending_images.is_empty(), "should register pending image");
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
    fn mask_circle_alpha_zero_at_corners() {
        // The reusable `.mask(Circle)` modifier — anywhere a photo
        // needs a circular crop, not just inside Avatar.
        let icon = RasterIcon::from_raw(vec![255; 32 * 32 * 4], 32, 32);
        let widget = ImageWidget::from_raw(
            icon.pixels().to_vec(),
            icon.width(),
            icon.height(),
        )
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
        let widget = ImageWidget::from_raw(original.clone(), 2, 1)
            .mask(ImageMaskShape::None);
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
