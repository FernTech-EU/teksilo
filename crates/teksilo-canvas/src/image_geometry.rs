// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a picture lands in its box, and which source pixel a point shows.
//!
//! A widget that shows a raster decides three things: how the source is
//! turned for display ([`ImageOrientation`]), how the displayed picture is
//! fitted into the widget's box ([`ImageFit`] and an
//! [`Alignment`]), and, for input, which source pixel lies under a point.
//! [`ImageGeometry`] holds the result of the first two and answers the third,
//! so that what a widget draws and what it maps pointer input through cannot
//! disagree: [`oriented_crop`] derives the drawn quad's texture coordinates
//! from the same tables the mapping methods use.
//!
//! Coordinates are widget-local logical pixels: the box's top-leading corner
//! is (0, 0), as in the `position` of the pointer events a widget's own
//! handlers receive. Source pixels are half-open squares: pixel `k` spans
//! `[k, k + 1)`, so a point on the right or bottom edge of the picture
//! belongs to no pixel.

use teksilo_tokens::Alignment;

use crate::geometry::{Point, Rect, Size, Transform2D};

/// How the stored pixel grid is turned to be displayed upright, with the
/// semantics of the TIFF and EXIF `Orientation` tag.
///
/// The four quarter-turn cases ([`swaps_axes`](Self::swaps_axes)) swap the
/// displayed width and height. Mapping a point back to a source pixel undoes
/// the orientation, so a press on a turned picture still finds the pixel
/// drawn under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageOrientation {
    /// 1 — stored upright. The overwhelmingly common case.
    #[default]
    Normal,
    /// 2 — mirrored left-to-right.
    FlipHorizontal,
    /// 3 — rotated 180°.
    Rotate180,
    /// 4 — mirrored top-to-bottom.
    FlipVertical,
    /// 5 — transposed (mirrored along the main diagonal).
    Transpose,
    /// 6 — rotated 90° clockwise for display.
    Rotate90,
    /// 7 — transversed (mirrored along the anti-diagonal).
    Transverse,
    /// 8 — rotated 270° clockwise for display.
    Rotate270,
}

impl ImageOrientation {
    /// The orientation a TIFF or EXIF `Orientation` value names, or `None`
    /// outside 1..=8.
    pub const fn from_exif(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::Normal,
            2 => Self::FlipHorizontal,
            3 => Self::Rotate180,
            4 => Self::FlipVertical,
            5 => Self::Transpose,
            6 => Self::Rotate90,
            7 => Self::Transverse,
            8 => Self::Rotate270,
            _ => return None,
        })
    }

    /// Map a raw TIFF `Orientation` value, leniently: anything outside 1..=8
    /// — including the 0 some buggy encoders write — is treated as
    /// [`Normal`](Self::Normal), the only safe reading of a value that cannot
    /// be trusted. [`from_exif`](Self::from_exif) is the strict form.
    pub const fn from_tiff(value: u16) -> Self {
        match Self::from_exif(value) {
            Some(orientation) => orientation,
            None => Self::Normal,
        }
    }

    /// Whether displaying this orientation swaps width and height.
    pub const fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Transpose | Self::Rotate90 | Self::Transverse | Self::Rotate270
        )
    }

    /// Whether this is a no-op, so callers can skip work entirely.
    pub const fn is_identity(self) -> bool {
        matches!(self, Self::Normal)
    }

    /// The displayed size of a `width × height` source.
    pub const fn displayed_size(self, width: u32, height: u32) -> (u32, u32) {
        if self.swaps_axes() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// The source pixel shown at displayed pixel `(xd, yd)` of a
    /// `width × height` source. Both are pixel indices, `(xd, yd)` inside
    /// the displayed size.
    const fn source_pixel(self, xd: u32, yd: u32, width: u32, height: u32) -> (u32, u32) {
        match self {
            Self::Normal => (xd, yd),
            Self::FlipHorizontal => (width - 1 - xd, yd),
            Self::Rotate180 => (width - 1 - xd, height - 1 - yd),
            Self::FlipVertical => (xd, height - 1 - yd),
            Self::Transpose => (yd, xd),
            Self::Rotate90 => (yd, height - 1 - xd),
            Self::Transverse => (width - 1 - yd, height - 1 - xd),
            Self::Rotate270 => (width - 1 - yd, xd),
        }
    }

    /// The source point shown at displayed point `(xd, yd)`, both continuous:
    /// [`source_pixel`](Self::source_pixel)'s table with each `w - 1 -`
    /// replaced by `w -`. With `width = height = 1` it maps normalised
    /// displayed coordinates to texture coordinates.
    fn source_point(self, xd: f32, yd: f32, width: f32, height: f32) -> (f32, f32) {
        match self {
            Self::Normal => (xd, yd),
            Self::FlipHorizontal => (width - xd, yd),
            Self::Rotate180 => (width - xd, height - yd),
            Self::FlipVertical => (xd, height - yd),
            Self::Transpose => (yd, xd),
            Self::Rotate90 => (yd, height - xd),
            Self::Transverse => (width - yd, height - xd),
            Self::Rotate270 => (width - yd, xd),
        }
    }

    /// The displayed pixel showing source pixel `(x, y)`: the inverse of
    /// [`source_pixel`](Self::source_pixel).
    const fn displayed_pixel(self, x: u32, y: u32, width: u32, height: u32) -> (u32, u32) {
        match self {
            Self::Normal => (x, y),
            Self::FlipHorizontal => (width - 1 - x, y),
            Self::Rotate180 => (width - 1 - x, height - 1 - y),
            Self::FlipVertical => (x, height - 1 - y),
            Self::Transpose => (y, x),
            Self::Rotate90 => (height - 1 - y, x),
            Self::Transverse => (height - 1 - y, width - 1 - x),
            Self::Rotate270 => (y, width - 1 - x),
        }
    }
}

/// How a picture is fitted within its box: the CSS `object-fit` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageFit {
    /// Scale to fit entirely within the box, preserving aspect ratio.
    /// May leave empty space (letterboxing).
    #[default]
    Contain,
    /// Scale to cover the entire box, preserving aspect ratio.
    /// May crop the picture.
    Cover,
    /// Stretch to fill the box exactly, ignoring aspect ratio.
    Fill,
    /// Like `Contain`, but never upscales: a picture smaller than the box
    /// keeps its natural size.
    ScaleDown,
    /// Draw the picture at its natural size, neither scaling up nor down. A
    /// picture larger than the box is cropped to it, one smaller sits inside
    /// with empty space; the alignment places it either way. CSS
    /// `object-fit: none`.
    None,
}

impl ImageFit {
    /// Where a picture of `content` size lands in `bounds` under this fit,
    /// placed by `alignment` where the fit leaves slack or crops (the CSS
    /// `object-position` analogue). `rtl` resolves Leading and Trailing for a
    /// right-to-left layout. The result may exceed `bounds` under `Cover`, and
    /// under `None` for a picture larger than the box. An empty `content`
    /// fills `bounds`.
    pub fn fitted_rect(self, content: Size, bounds: Rect, alignment: Alignment, rtl: bool) -> Rect {
        let (img_w, img_h) = (content.width, content.height);
        if img_w <= 0.0 || img_h <= 0.0 {
            return bounds;
        }
        let (w, h) = match self {
            Self::Fill => (bounds.width, bounds.height),
            Self::Contain => {
                let scale = (bounds.width / img_w).min(bounds.height / img_h);
                (img_w * scale, img_h * scale)
            }
            Self::Cover => {
                let scale = (bounds.width / img_w).max(bounds.height / img_h);
                (img_w * scale, img_h * scale)
            }
            Self::ScaleDown => {
                let scale = (bounds.width / img_w).min(bounds.height / img_h).min(1.0);
                (img_w * scale, img_h * scale)
            }
            Self::None => (img_w, img_h),
        };
        let x = bounds.x + alignment.horizontal.resolve(w, bounds.width, rtl);
        let y = bounds.y + alignment.vertical.resolve(h, bounds.height);
        Rect::new(x, y, w, h)
    }
}

/// An integer rectangle in source pixels, half-open: `x..x + width` by
/// `y..y + height`. Arithmetic on its edges is done in `u64`, so no
/// combination of fields overflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The whole of a `width × height` frame.
    pub const fn full(width: u32, height: u32) -> Self {
        Self::new(0, 0, width, height)
    }

    /// Whether it covers no pixel.
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The number of pixels it covers.
    pub const fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Whether it lies inside a `width × height` frame. An empty rect fits
    /// wherever its origin does.
    pub const fn fits_in(&self, width: u32, height: u32) -> bool {
        self.x as u64 + self.width as u64 <= width as u64
            && self.y as u64 + self.height as u64 <= height as u64
    }

    /// The smallest rect holding both. An empty operand contributes nothing.
    pub fn union(&self, other: &PixelRect) -> PixelRect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x as u64 + self.width as u64).max(other.x as u64 + other.width as u64);
        let y1 = (self.y as u64 + self.height as u64).max(other.y as u64 + other.height as u64);
        PixelRect::new(x0, y0, saturate(x1 - x0 as u64), saturate(y1 - y0 as u64))
    }

    /// The pixels both cover, or `None` when they share none.
    pub fn intersect(&self, other: &PixelRect) -> Option<PixelRect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x as u64 + self.width as u64).min(other.x as u64 + other.width as u64);
        let y1 = (self.y as u64 + self.height as u64).min(other.y as u64 + other.height as u64);
        (x1 > x0 as u64 && y1 > y0 as u64)
            .then(|| PixelRect::new(x0, y0, saturate(x1 - x0 as u64), saturate(y1 - y0 as u64)))
    }
}

fn saturate(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// The placement of a source raster in a widget's box, and the mapping
/// between the box's points and the source's pixels.
///
/// [`compute`](Self::compute) places it; [`snapped`](Self::snapped) moves the
/// picture's edges onto the device-pixel grid. A widget computes it once per
/// layout, paints from it and maps input through it, so the pixel under the
/// pointer is the pixel drawn there.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ImageGeometry {
    /// The source size in pixels, before orientation.
    pub source: (u32, u32),
    /// How the source is turned for display.
    pub orientation: ImageOrientation,
    /// Where the whole displayed picture lies, widget-local. It exceeds
    /// `bounds` where the fit crops.
    pub content: Rect,
    /// The widget's box, widget-local: its origin is (0, 0).
    pub bounds: Rect,
}

impl ImageGeometry {
    /// A placement computed elsewhere: `content` is where the whole
    /// displayed picture lies inside `bounds`, both widget-local.
    pub fn new(
        source: (u32, u32),
        orientation: ImageOrientation,
        content: Rect,
        bounds: Rect,
    ) -> Self {
        Self {
            source,
            orientation,
            content,
            bounds,
        }
    }

    /// A `source`-sized raster turned by `orientation` and fitted into a box
    /// of size `bounds` by `fit`, placed by `alignment` (`rtl` resolving
    /// Leading and Trailing). Not snapped.
    pub fn compute(
        bounds: Size,
        source: (u32, u32),
        fit: ImageFit,
        alignment: Alignment,
        rtl: bool,
        orientation: ImageOrientation,
    ) -> Self {
        let (w, h) = orientation.displayed_size(source.0, source.1);
        let bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        let content = fit.fitted_rect(Size::new(w as f32, h as f32), bounds, alignment, rtl);
        Self::new(source, orientation, content, bounds)
    }

    /// The same placement with the picture's four edges moved to the nearest
    /// device-pixel boundary.
    ///
    /// `to_device` maps a widget-local point to device pixels: the widget's
    /// window origin, any transform its ancestors apply, then the HiDPI
    /// scale. Snapping applies only when that keeps edges on edges, a
    /// transform with no rotation or shear; any other returns the placement
    /// unchanged. An edge moves by at most half a device pixel, and an axis
    /// snapping would collapse keeps its unsnapped edges. Snapping twice
    /// changes nothing more.
    pub fn snapped(self, to_device: Transform2D) -> Self {
        let [a, b, c, d, tx, ty] = to_device.m;
        if b != 0.0 || c != 0.0 || a == 0.0 || d == 0.0 || !(a.is_finite() && d.is_finite()) {
            return self;
        }
        let snap =
            |v: f32, scale: f32, offset: f32| ((v * scale + offset).round() - offset) / scale;
        // One axis: its origin and extent, snapped unless that collapses it.
        let axis = |origin: f32, extent: f32, scale: f32, offset: f32| {
            let (s0, s1) = (
                snap(origin, scale, offset),
                snap(origin + extent, scale, offset),
            );
            let (s0, s1) = (s0.min(s1), s0.max(s1));
            if s1 - s0 > 0.0 {
                (s0, s1 - s0)
            } else {
                (origin, extent)
            }
        };
        let c = self.content;
        let (x, width) = axis(c.x, c.width, a, tx);
        let (y, height) = axis(c.y, c.height, d, ty);
        Self {
            content: Rect::new(x, y, width, height),
            ..self
        }
    }

    /// The source size as displayed: swapped for a quarter turn.
    pub fn displayed(&self) -> (u32, u32) {
        self.orientation
            .displayed_size(self.source.0, self.source.1)
    }

    /// The part of the picture inside the box, widget-local; `None` when the
    /// two do not overlap.
    pub fn visible(&self) -> Option<Rect> {
        intersect(self.content, self.bounds)
    }

    /// The source pixel whose displayed square contains `local`, or `None`
    /// on the letterbox, outside the box, or with nothing to show.
    pub fn map_to_source(&self, local: Point) -> Option<(u32, u32)> {
        let b = self.bounds;
        if !(local.x >= b.x
            && local.x < b.x + b.width
            && local.y >= b.y
            && local.y < b.y + b.height)
        {
            return None;
        }
        let (u, v) = self.unit(local)?;
        if !((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v)) {
            return None;
        }
        self.pixel_at(u, v)
    }

    /// The source pixel nearest `local`: the point is clamped into the
    /// picture first. `None` only with nothing to show.
    pub fn map_to_source_clamped(&self, local: Point) -> Option<(u32, u32)> {
        let (u, v) = self.unit(local)?;
        self.pixel_at(u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
    }

    /// Continuous source coordinates of `local`, unclamped: pixel `k` spans
    /// `[k, k + 1)`. `None` only with nothing to show.
    pub fn map_to_source_f32(&self, local: Point) -> Option<(f32, f32)> {
        let (u, v) = self.unit(local)?;
        let (wd, hd) = self.displayed();
        let (w, h) = self.source;
        Some(
            self.orientation
                .source_point(u * wd as f32, v * hd as f32, w as f32, h as f32),
        )
    }

    /// Where the source pixels of `rect` are displayed, widget-local: the
    /// bounding box of their displayed squares. `None` when `rect` is empty
    /// or does not lie inside the source.
    pub fn map_from_source(&self, rect: PixelRect) -> Option<Rect> {
        let (w, h) = self.source;
        if rect.is_empty() || !rect.fits_in(w, h) {
            return None;
        }
        let (wd, hd) = self.displayed();
        if self.content.width <= 0.0 || self.content.height <= 0.0 {
            return None;
        }
        // Two opposite corner pixels bound the displayed rect: their images
        // under any of the eight orientations are opposite corners again.
        let (ax, ay) = self.orientation.displayed_pixel(rect.x, rect.y, w, h);
        let (bx, by) = self.orientation.displayed_pixel(
            rect.x + rect.width - 1,
            rect.y + rect.height - 1,
            w,
            h,
        );
        let (x0, x1) = (ax.min(bx), ax.max(bx) + 1);
        let (y0, y1) = (ay.min(by), ay.max(by) + 1);
        let c = self.content;
        let sx = c.width / wd as f32;
        let sy = c.height / hd as f32;
        Some(Rect::new(
            c.x + x0 as f32 * sx,
            c.y + y0 as f32 * sy,
            (x1 - x0) as f32 * sx,
            (y1 - y0) as f32 * sy,
        ))
    }

    /// `local` in units of the content rect: (0, 0) its top-leading corner,
    /// (1, 1) its bottom-trailing one. `None` with nothing to show.
    fn unit(&self, local: Point) -> Option<(f32, f32)> {
        let c = self.content;
        let (w, h) = self.source;
        if w == 0 || h == 0 || c.width <= 0.0 || c.height <= 0.0 {
            return None;
        }
        Some(((local.x - c.x) / c.width, (local.y - c.y) / c.height))
    }

    /// The source pixel at unit coordinates `(u, v)` in `[0, 1]`. The far
    /// edge belongs to the last pixel, which also absorbs float error there.
    fn pixel_at(&self, u: f32, v: f32) -> Option<(u32, u32)> {
        let (w, h) = self.source;
        let (wd, hd) = self.displayed();
        let xd = ((u * wd as f32).floor() as u32).min(wd - 1);
        let yd = ((v * hd as f32).floor() as u32).min(hd - 1);
        Some(self.orientation.source_pixel(xd, yd, w, h))
    }
}

/// The part of `content` inside `clip`, and the texture coordinates of its
/// corners for a source turned by `orientation`: top-leading, top-trailing,
/// bottom-trailing, bottom-leading, each `[u, v]` in `[0, 1]` across the
/// source. `None` when the two rects share no area.
///
/// This is how a picture is cropped without a scissor: the quad covers only
/// the visible part, and its corners sample only the matching part of the
/// texture. The tables are [`ImageGeometry`]'s, so drawing and mapping agree.
pub fn oriented_crop(
    content: Rect,
    clip: Rect,
    orientation: ImageOrientation,
) -> Option<(Rect, [[f32; 2]; 4])> {
    if content.width <= 0.0 || content.height <= 0.0 {
        return None;
    }
    let visible = intersect(content, clip)?;
    let u0 = (visible.x - content.x) / content.width;
    let v0 = (visible.y - content.y) / content.height;
    let u1 = (visible.x + visible.width - content.x) / content.width;
    let v1 = (visible.y + visible.height - content.y) / content.height;
    let corner = |u: f32, v: f32| {
        let (s, t) = orientation.source_point(u, v, 1.0, 1.0);
        [s.clamp(0.0, 1.0), t.clamp(0.0, 1.0)]
    };
    Some((
        visible,
        [
            corner(u0, v0),
            corner(u1, v0),
            corner(u1, v1),
            corner(u0, v1),
        ],
    ))
}

/// The overlap of two rects with positive area, or `None`.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width).min(b.x + b.width);
    let y1 = (a.y + a.height).min(b.y + b.height);
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests;
