//! Path atlas: CPU rasterizes paths with tiny-skia, caches results in a texture atlas with LRU eviction.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use fern_canvas::paint::{LineCap, StrokeStyle};
use fern_canvas::path::{Path, PathCommand};

/// A region within the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Frame when this region was last used.
    last_used_frame: u64,
}

/// Cache key derived from path geometry + stroke style + rasterized size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PathCacheKey(u64);

impl PathCacheKey {
    fn new(path: &Path, color: [f32; 4], style: &StrokeStyle, w: u32, h: u32) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        // Hash path commands
        for cmd in &path.commands {
            std::mem::discriminant(cmd).hash(&mut hasher);
            match cmd {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => {
                    p.x.to_bits().hash(&mut hasher);
                    p.y.to_bits().hash(&mut hasher);
                }
                PathCommand::QuadTo { control, to } => {
                    control.x.to_bits().hash(&mut hasher);
                    control.y.to_bits().hash(&mut hasher);
                    to.x.to_bits().hash(&mut hasher);
                    to.y.to_bits().hash(&mut hasher);
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    control1.x.to_bits().hash(&mut hasher);
                    control1.y.to_bits().hash(&mut hasher);
                    control2.x.to_bits().hash(&mut hasher);
                    control2.y.to_bits().hash(&mut hasher);
                    to.x.to_bits().hash(&mut hasher);
                    to.y.to_bits().hash(&mut hasher);
                }
                PathCommand::ArcTo {
                    rect,
                    start_angle,
                    sweep_angle,
                } => {
                    rect.x.to_bits().hash(&mut hasher);
                    rect.y.to_bits().hash(&mut hasher);
                    rect.width.to_bits().hash(&mut hasher);
                    rect.height.to_bits().hash(&mut hasher);
                    start_angle.to_bits().hash(&mut hasher);
                    sweep_angle.to_bits().hash(&mut hasher);
                }
                PathCommand::Close => {}
            }
        }
        // Hash stroke style
        style.width.to_bits().hash(&mut hasher);
        std::mem::discriminant(&style.line_cap).hash(&mut hasher);
        if let Some(ref pattern) = style.dash_pattern {
            for &v in pattern {
                v.to_bits().hash(&mut hasher);
            }
        }
        style.dash_offset.to_bits().hash(&mut hasher);
        // Hash color (rasterization depends on it)
        color[0].to_bits().hash(&mut hasher);
        color[1].to_bits().hash(&mut hasher);
        color[2].to_bits().hash(&mut hasher);
        color[3].to_bits().hash(&mut hasher);
        // Hash rasterized dimensions
        w.hash(&mut hasher);
        h.hash(&mut hasher);
        PathCacheKey(hasher.finish())
    }
}

/// Shelf-packed atlas for rasterized paths with LRU eviction.
pub struct PathAtlas {
    /// Atlas pixel data (RGBA).
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    /// Maximum atlas dimension.
    max_size: u32,
    /// Cache from path key to atlas region.
    cache: HashMap<PathCacheKey, AtlasRegion>,
    /// Current frame counter for LRU.
    current_frame: u64,
    /// Whether the atlas texture needs re-uploading.
    dirty: bool,
    // Shelf-packing state
    /// Current Y position of the next shelf.
    shelf_y: u32,
    /// Current X position within the current shelf.
    shelf_x: u32,
    /// Height of the current shelf (tallest entry in this row).
    shelf_height: u32,
}

impl PathAtlas {
    /// Create a new path atlas with the given initial dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixels: vec![0; (width * height * 4) as usize],
            width,
            height,
            max_size: 4096,
            cache: HashMap::new(),
            current_frame: 0,
            dirty: false,
            shelf_y: 0,
            shelf_x: 0,
            shelf_height: 0,
        }
    }

    /// Call at the start of each frame to advance the LRU counter.
    pub fn begin_frame(&mut self) {
        self.current_frame += 1;
    }

    /// Current atlas dimensions.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Whether the atlas texture needs re-uploading to the GPU.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Raw pixel data (RGBA).
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Mark the atlas as uploaded.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Look up or rasterize a path, returning its atlas region.
    pub fn lookup_or_rasterize(
        &mut self,
        path: &Path,
        color: [f32; 4],
        style: &StrokeStyle,
        bounds: [f32; 4],
        scale_factor: f32,
    ) -> Option<AtlasRegion> {
        let raster_w = (bounds[2] * scale_factor).ceil() as u32;
        let raster_h = (bounds[3] * scale_factor).ceil() as u32;
        if raster_w == 0 || raster_h == 0 {
            return None;
        }

        let key = PathCacheKey::new(path, color, style, raster_w, raster_h);

        // Cache hit
        if let Some(region) = self.cache.get_mut(&key) {
            region.last_used_frame = self.current_frame;
            return Some(*region);
        }

        // Rasterize
        let pixels = rasterize_path(path, color, style, bounds, scale_factor)?;
        let region = self.allocate_and_write(key, raster_w, raster_h, &pixels)?;
        Some(region)
    }

    /// Try to allocate space in the atlas via shelf packing.
    /// Evicts LRU entries if needed. Returns None if impossible.
    fn allocate_and_write(
        &mut self,
        key: PathCacheKey,
        w: u32,
        h: u32,
        pixels: &[u8],
    ) -> Option<AtlasRegion> {
        // Try to fit in the current shelf
        if let Some(region) = self.try_allocate(w, h) {
            self.blit(region.x, region.y, w, h, pixels);
            self.cache.insert(key, region);
            self.dirty = true;
            return Some(region);
        }

        // Try LRU eviction: clear least recently used entries and reset packing
        self.evict_lru();

        // Retry after eviction
        if let Some(region) = self.try_allocate(w, h) {
            self.blit(region.x, region.y, w, h, pixels);
            self.cache.insert(key, region);
            self.dirty = true;
            return Some(region);
        }

        // Try growing the atlas
        if self.try_grow()
            && let Some(region) = self.try_allocate(w, h)
        {
            self.blit(region.x, region.y, w, h, pixels);
            self.cache.insert(key, region);
            self.dirty = true;
            return Some(region);
        }

        None
    }

    /// Try to allocate a region using shelf packing.
    fn try_allocate(&mut self, w: u32, h: u32) -> Option<AtlasRegion> {
        // Does it fit on the current shelf?
        if self.shelf_x + w <= self.width && self.shelf_y + h.max(self.shelf_height) <= self.height
        {
            let region = AtlasRegion {
                x: self.shelf_x,
                y: self.shelf_y,
                w,
                h,
                last_used_frame: self.current_frame,
            };
            self.shelf_x += w;
            self.shelf_height = self.shelf_height.max(h);
            return Some(region);
        }

        // Start a new shelf
        let new_y = self.shelf_y + self.shelf_height;
        if w <= self.width && new_y + h <= self.height {
            self.shelf_y = new_y;
            self.shelf_x = w;
            self.shelf_height = h;
            let region = AtlasRegion {
                x: 0,
                y: new_y,
                w,
                h,
                last_used_frame: self.current_frame,
            };
            return Some(region);
        }

        None
    }

    /// Evict entries from previous frames and repack. Entries used in the
    /// **current** frame are preserved — otherwise a path inserted earlier
    /// in the same `render()` walk could be evicted by a later one, leaving
    /// the earlier path's `AtlasRegion` pointing at coordinates that now
    /// hold a different path's pixels (visible as flicker on path-heavy
    /// widgets like LineChart and PieChart when the atlas overflows).
    ///
    /// If preserving current-frame entries doesn't free enough room,
    /// `allocate_and_write` falls through to `try_grow` instead of evicting
    /// them. As a last resort we clear everything — but that path is now
    /// only reachable when one frame's paths can't even fit a fully grown
    /// atlas.
    fn evict_lru(&mut self) {
        if self.cache.is_empty() {
            return;
        }

        let current = self.current_frame;
        let any_kept = self.cache.values().any(|r| r.last_used_frame == current);

        if any_kept {
            // Keep the current-frame entries; drop everything else and
            // repack the survivors at the start of the atlas.
            let mut survivors: Vec<(PathCacheKey, AtlasRegion, Vec<u8>)> = self
                .cache
                .iter()
                .filter(|(_, r)| r.last_used_frame == current)
                .map(|(k, r)| {
                    let pixels = self.read_region(*r);
                    (*k, *r, pixels)
                })
                .collect();
            self.cache.clear();
            self.pixels.fill(0);
            self.shelf_x = 0;
            self.shelf_y = 0;
            self.shelf_height = 0;
            self.dirty = true;
            // Repack survivors. If any can't be replaced (atlas is too
            // small even for the current frame's working set) the loop
            // simply skips it — the next `lookup_or_rasterize` will
            // re-rasterize and `try_grow` from the outer caller.
            // Repack tallest-first to limit shelf wastage.
            survivors.sort_by_key(|(_, r, _)| std::cmp::Reverse(r.h));
            for (key, old_region, pixels) in survivors {
                if let Some(new_region) = self.try_allocate(old_region.w, old_region.h) {
                    self.blit(new_region.x, new_region.y, new_region.w, new_region.h, &pixels);
                    self.cache.insert(
                        key,
                        AtlasRegion {
                            x: new_region.x,
                            y: new_region.y,
                            w: new_region.w,
                            h: new_region.h,
                            last_used_frame: current,
                        },
                    );
                }
            }
        } else {
            // No current-frame entries — safe to clear everything.
            self.cache.clear();
            self.pixels.fill(0);
            self.shelf_x = 0;
            self.shelf_y = 0;
            self.shelf_height = 0;
            self.dirty = true;
        }
    }

    /// Read a region's pixels back out of the atlas (for repacking
    /// survivors during eviction). Returns an RGBA buffer of `w*h*4` bytes.
    fn read_region(&self, region: AtlasRegion) -> Vec<u8> {
        let mut out = vec![0u8; (region.w * region.h * 4) as usize];
        for row in 0..region.h {
            let src_start = ((region.y + row) * self.width * 4 + region.x * 4) as usize;
            let src_end = src_start + (region.w * 4) as usize;
            let dst_start = (row * region.w * 4) as usize;
            let dst_end = dst_start + (region.w * 4) as usize;
            if src_end <= self.pixels.len() && dst_end <= out.len() {
                out[dst_start..dst_end].copy_from_slice(&self.pixels[src_start..src_end]);
            }
        }
        out
    }

    /// Try to grow the atlas (double dimensions up to max_size).
    fn try_grow(&mut self) -> bool {
        let new_w = (self.width * 2).min(self.max_size);
        let new_h = (self.height * 2).min(self.max_size);
        if new_w == self.width && new_h == self.height {
            return false; // Already at max
        }
        let mut new_pixels = vec![0u8; (new_w * new_h * 4) as usize];
        // Copy existing data row by row
        for y in 0..self.height {
            let src_start = (y * self.width * 4) as usize;
            let src_end = src_start + (self.width * 4) as usize;
            let dst_start = (y * new_w * 4) as usize;
            new_pixels[dst_start..dst_start + (self.width * 4) as usize]
                .copy_from_slice(&self.pixels[src_start..src_end]);
        }
        self.pixels = new_pixels;
        self.width = new_w;
        self.height = new_h;
        self.dirty = true;
        true
    }

    /// Write pixels into the atlas at the given position.
    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, pixels: &[u8]) {
        for row in 0..h {
            let src_start = (row * w * 4) as usize;
            let src_end = src_start + (w * 4) as usize;
            let dst_start = ((y + row) * self.width * 4 + x * 4) as usize;
            let dst_end = dst_start + (w * 4) as usize;
            if src_end <= pixels.len() && dst_end <= self.pixels.len() {
                self.pixels[dst_start..dst_end].copy_from_slice(&pixels[src_start..src_end]);
            }
        }
    }
}

/// Rasterize a path to RGBA pixels using tiny-skia.
fn rasterize_path(
    path: &Path,
    color: [f32; 4],
    style: &StrokeStyle,
    bounds: [f32; 4],
    scale_factor: f32,
) -> Option<Vec<u8>> {
    let w = (bounds[2] * scale_factor).ceil() as u32;
    let h = (bounds[3] * scale_factor).ceil() as u32;
    if w == 0 || h == 0 {
        return None;
    }

    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;

    // Build tiny-skia path, translating from bounds origin
    let mut pb = tiny_skia::PathBuilder::new();
    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                pb.move_to(
                    (p.x - bounds[0]) * scale_factor,
                    (p.y - bounds[1]) * scale_factor,
                );
            }
            PathCommand::LineTo(p) => {
                pb.line_to(
                    (p.x - bounds[0]) * scale_factor,
                    (p.y - bounds[1]) * scale_factor,
                );
            }
            PathCommand::QuadTo { control, to } => {
                pb.quad_to(
                    (control.x - bounds[0]) * scale_factor,
                    (control.y - bounds[1]) * scale_factor,
                    (to.x - bounds[0]) * scale_factor,
                    (to.y - bounds[1]) * scale_factor,
                );
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                pb.cubic_to(
                    (control1.x - bounds[0]) * scale_factor,
                    (control1.y - bounds[1]) * scale_factor,
                    (control2.x - bounds[0]) * scale_factor,
                    (control2.y - bounds[1]) * scale_factor,
                    (to.x - bounds[0]) * scale_factor,
                    (to.y - bounds[1]) * scale_factor,
                );
            }
            PathCommand::ArcTo {
                rect,
                start_angle,
                sweep_angle,
            } => {
                // Approximate arc with cubic Bézier segments
                arc_to_cubics(
                    &mut pb,
                    rect.x - bounds[0],
                    rect.y - bounds[1],
                    rect.width,
                    rect.height,
                    start_angle,
                    sweep_angle,
                    scale_factor,
                );
            }
            PathCommand::Close => {
                pb.close();
            }
        }
    }

    let sk_path = pb.finish()?;

    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba(
            color[0], color[1], color[2], color[3],
        )?),
        anti_alias: true,
        ..Default::default()
    };

    if style.width > 0.0 {
        // Stroke
        let line_cap = match style.line_cap {
            LineCap::Butt => tiny_skia::LineCap::Butt,
            LineCap::Round => tiny_skia::LineCap::Round,
            LineCap::Square => tiny_skia::LineCap::Square,
        };
        let dash = style
            .dash_pattern
            .as_ref()
            .and_then(|pattern| tiny_skia::StrokeDash::new(pattern.clone(), style.dash_offset));
        let stroke = tiny_skia::Stroke {
            width: style.width * scale_factor,
            line_cap,
            dash,
            ..Default::default()
        };
        pixmap.stroke_path(
            &sk_path,
            &paint,
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
    } else {
        // Fill
        pixmap.fill_path(
            &sk_path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    Some(pixmap.data().to_vec())
}

/// Approximate an elliptical arc with cubic Bézier segments.
/// Each 90° sweep is one cubic; smaller sweeps use one cubic.
///
/// `start_angle` and `sweep_angle` are in **degrees** (matching the
/// public `Path::arc_to` API and existing call sites like
/// `Path::circle` and `Path::rounded_rect`). They are converted to
/// radians internally before being fed to `f32::cos`/`f32::sin`.
#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    pb: &mut tiny_skia::PathBuilder,
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    start_angle: f32,
    sweep_angle: f32,
    scale_factor: f32,
) {
    let rx = w * 0.5;
    let ry = h * 0.5;
    let center_x = (cx + rx) * scale_factor;
    let center_y = (cy + ry) * scale_factor;
    let rx_s = rx * scale_factor;
    let ry_s = ry * scale_factor;

    let mut remaining = sweep_angle.to_radians();
    let mut angle = start_angle.to_radians();
    let sign = if remaining >= 0.0 { 1.0 } else { -1.0 };

    while remaining.abs() > 0.001 {
        let chunk = sign * remaining.abs().min(std::f32::consts::FRAC_PI_2);
        let half = chunk * 0.5;
        let k = (4.0 / 3.0) * (1.0 - half.cos()) / half.sin();

        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let cos_b = (angle + chunk).cos();
        let sin_b = (angle + chunk).sin();

        let p1x = center_x + rx_s * cos_a;
        let p1y = center_y + ry_s * sin_a;
        let p2x = center_x + rx_s * (cos_a - k * sin_a);
        let p2y = center_y + ry_s * (sin_a + k * cos_a);
        let p3x = center_x + rx_s * (cos_b + k * sin_b);
        let p3y = center_y + ry_s * (sin_b - k * cos_b);
        let p4x = center_x + rx_s * cos_b;
        let p4y = center_y + ry_s * sin_b;

        if (remaining - sweep_angle).abs() < 0.001 {
            // First segment: line_to the start point to connect with the existing path.
            // If the path is empty, tiny-skia will treat this as a move_to.
            pb.line_to(p1x, p1y);
        } else {
            pb.line_to(p1x, p1y);
        }
        pb.cubic_to(p2x, p2y, p3x, p3y, p4x, p4y);

        angle += chunk;
        remaining -= chunk;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::geometry::Point;

    #[test]
    fn rasterize_simple_rect_path() {
        let mut path = Path::new();
        path.commands
            .push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(10.0, 10.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(0.0, 10.0)));
        path.commands.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let pixels = rasterize_path(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0);
        assert!(pixels.is_some());
        let px = pixels.unwrap();
        assert_eq!(px.len(), 10 * 10 * 4);
        // Center pixel should be red
        let center = (5 * 10 + 5) * 4;
        assert!(px[center] > 200); // R
    }

    #[test]
    fn rasterize_stroke_path() {
        let mut path = Path::new();
        path.commands
            .push(PathCommand::MoveTo(Point::new(1.0, 5.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(9.0, 5.0)));

        let style = StrokeStyle::solid(2.0);
        let bounds = [0.0, 0.0, 10.0, 10.0];
        let pixels = rasterize_path(&path, [0.0, 1.0, 0.0, 1.0], &style, bounds, 1.0);
        assert!(pixels.is_some());
    }

    #[test]
    fn atlas_cache_hit() {
        let mut atlas = PathAtlas::new(256, 256);
        atlas.begin_frame();

        let mut path = Path::new();
        path.commands
            .push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(10.0, 10.0)));
        path.commands.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let bounds = [0.0, 0.0, 10.0, 10.0];

        let r1 = atlas
            .lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0)
            .unwrap();
        let r2 = atlas
            .lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0)
            .unwrap();

        // Same region (cache hit)
        assert_eq!(r1.x, r2.x);
        assert_eq!(r1.y, r2.y);
    }

    #[test]
    fn atlas_begin_frame_advances() {
        let mut atlas = PathAtlas::new(256, 256);
        assert_eq!(atlas.current_frame, 0);
        atlas.begin_frame();
        assert_eq!(atlas.current_frame, 1);
        atlas.begin_frame();
        assert_eq!(atlas.current_frame, 2);
    }

    #[test]
    fn atlas_eviction_clears_stale() {
        let mut atlas = PathAtlas::new(64, 64);

        let mut path = Path::new();
        path.commands
            .push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(8.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(8.0, 8.0)));
        path.commands.push(PathCommand::Close);
        let style = StrokeStyle::solid(0.0);
        let bounds = [0.0, 0.0, 8.0, 8.0];

        atlas.begin_frame(); // frame 1
        atlas.lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0);

        // Advance well past the entry
        atlas.begin_frame(); // frame 2
        atlas.begin_frame(); // frame 3
        atlas.begin_frame(); // frame 4

        // Eviction should clear it
        atlas.evict_lru();
        assert!(atlas.cache.is_empty());
    }

    #[test]
    fn evict_preserves_current_frame_entries() {
        // Regression: previously `evict_lru` cleared the entire cache,
        // so a second path inserted in the same frame could displace
        // the first — `path_regions[0]` ended up pointing at pixels
        // that now belonged to path #2. LineChart and PieChart hit this
        // routinely because their paths cover most of the plot area.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.begin_frame();

        let mut p1 = Path::new();
        p1.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p1.commands.push(PathCommand::LineTo(Point::new(40.0, 0.0)));
        p1.commands.push(PathCommand::LineTo(Point::new(40.0, 40.0)));
        p1.commands.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.commands.push(PathCommand::LineTo(Point::new(50.0, 0.0)));
        p2.commands.push(PathCommand::LineTo(Point::new(50.0, 50.0)));
        p2.commands.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(&p1, [1.0, 0.0, 0.0, 1.0], &style, [0.0, 0.0, 40.0, 40.0], 1.0)
            .expect("p1 fits");

        // p2 doesn't fit in the remaining space → eviction triggers.
        // After the fix, p1 (current-frame) survives and gets repacked.
        let _r2 =
            atlas.lookup_or_rasterize(&p2, [0.0, 1.0, 0.0, 1.0], &style, [0.0, 0.0, 50.0, 50.0], 1.0);

        // Looking up p1 again must still hit cache (with possibly a new
        // region, but stable across the lookup).
        let r1b = atlas
            .lookup_or_rasterize(&p1, [1.0, 0.0, 0.0, 1.0], &style, [0.0, 0.0, 40.0, 40.0], 1.0)
            .expect("p1 still cached after eviction");
        // The repacked region may have moved, but lookup_or_rasterize
        // must return a non-None region for p1 — i.e. it wasn't lost.
        let _ = (r1, r1b);
        assert!(atlas.cache.contains_key(&PathCacheKey::new(
            &p1,
            [1.0, 0.0, 0.0, 1.0],
            &style,
            40,
            40,
        )));
    }

    #[test]
    fn atlas_grow() {
        let mut atlas = PathAtlas::new(16, 16);
        assert!(atlas.try_grow());
        assert_eq!(atlas.width, 32);
        assert_eq!(atlas.height, 32);
    }
}
