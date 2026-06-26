// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Path atlas: CPU rasterizes paths with tiny-skia, caches results in a texture atlas with LRU eviction.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bastyde_canvas::paint::{LineCap, LineJoin, StrokeSpace, StrokeStyle};
use bastyde_canvas::path::{Path, PathCommand};

/// Upper bound on a cosmetic path's rasterized dimension (device px). At
/// extreme zoom the body would otherwise exceed the atlas; beyond this the
/// body softens and the stroke drifts slightly off-cosmetic — an accepted
/// degradation far past normal zoom. Kept well under [`PathAtlas::max_size`]
/// (4096) to leave room for shelf packing.
const MAX_COSMETIC_RASTER_DIM: f32 = 2048.0;

/// Free vertical headroom (device px) below which `begin_frame` treats the
/// atlas as near-full and compacts. Roughly one tall shelf — enough that a
/// frame rarely runs out of room mid-walk (where reclaiming is unsafe).
const COMPACT_SLACK_PX: u32 = 256;

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
        // Cosmetic vs logical strokes bake differently (constant device width
        // vs zoom-scaled), so they must not share a cache entry.
        std::mem::discriminant(&style.space).hash(&mut hasher);
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
    ///
    /// This is also the only point at which the atlas may safely **repack**
    /// itself: no `AtlasRegion` has been handed out for the new frame yet, so
    /// moving surviving entries to fresh coordinates cannot invalidate any
    /// region the renderer is still holding from the current frame. When the
    /// atlas is near-full and there are stale entries (not touched on the last
    /// completed frame), we compact — dropping the stale entries and repacking
    /// the rest tightly — so steady-state reclamation never has to happen
    /// mid-frame (which would corrupt already-placed paths).
    pub fn begin_frame(&mut self) {
        self.current_frame += 1;

        // Only the just-completed frame's working set is worth keeping
        // (temporal locality); anything older is fragmentation to reclaim.
        let keep_from = self.current_frame - 1;
        let near_full = self.shelf_y.saturating_add(self.shelf_height) + COMPACT_SLACK_PX
            >= self.height;
        let has_stale = self.cache.values().any(|r| r.last_used_frame < keep_from);
        if near_full && has_stale {
            self.compact(keep_from);
        }
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
    ///
    /// `zoom` is the uniform scale of the view transform active where the path
    /// is drawn. For a **cosmetic** stroke ([`StrokeSpace::Device`]) the body
    /// is rasterized at the current zoom (so it stays sharp, matching the
    /// transform-scaled display quad 1:1) while the stroke is baked at a
    /// zoom-independent device width — the border holds a constant
    /// device-pixel thickness at any zoom. **Logical** strokes ignore `zoom`
    /// (the body bitmap is stretched by the display quad, as before).
    pub fn lookup_or_rasterize(
        &mut self,
        path: &Path,
        color: [f32; 4],
        style: &StrokeStyle,
        bounds: [f32; 4],
        scale_factor: f32,
        zoom: f32,
    ) -> Option<AtlasRegion> {
        // Cosmetic paths rasterize the body at the current zoom (so it stays
        // sharp 1:1 with the transform-scaled display quad). Cost: the zoom is
        // baked into the raster dimensions, which are part of the cache key,
        // so a CONTINUOUS zoom gesture is a cache miss every frame — each
        // visible cosmetic path is re-rasterized per frame while zooming (the
        // per-frame LRU keeps current-frame entries and evicts the rest, so
        // the atlas stays bounded, but CPU rasterization scales with the
        // visible cosmetic-path count). Cache hits resume once the zoom
        // settles. This is the cost of "full-fidelity" cosmetic paths; coarse
        // zoom-quantization would cut the re-raster rate but reintroduce the
        // sub-pixel width drift the zoom-aware path was chosen to avoid.
        let (geom_scale, stroke_scale) = if style.space == StrokeSpace::Device {
            let mut g = scale_factor * zoom.max(1e-3);
            // Keep the bitmap under the atlas budget at extreme zoom.
            let cap = MAX_COSMETIC_RASTER_DIM / bounds[2].max(bounds[3]).max(1.0);
            if g > cap {
                g = cap;
            }
            (g, scale_factor)
        } else {
            (scale_factor, scale_factor)
        };

        let raster_w = (bounds[2] * geom_scale).ceil() as u32;
        let raster_h = (bounds[3] * geom_scale).ceil() as u32;
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
        let pixels = rasterize_path(path, color, style, bounds, geom_scale, stroke_scale)?;
        let region = self.allocate_and_write(key, raster_w, raster_h, &pixels)?;
        Some(region)
    }

    /// Try to allocate space in the atlas via shelf packing.
    ///
    /// Strategy, in order:
    ///   1. Try the current shelf / a new shelf at the existing size.
    ///   2. Grow the atlas (doubles up to `max_size`). Growth preserves
    ///      every existing entry's `(x, y)` so any `AtlasRegion` values
    ///      handed out earlier in the same render pass stay valid.
    ///   3. Last resort, evict. Eviction never moves entries already handed
    ///      out this frame (that would invalidate `AtlasRegion`s the caller
    ///      cached earlier in the same render walk → wrong-pixel sampling). It
    ///      can only reclaim space when nothing has been handed out yet this
    ///      frame; otherwise the allocation fails and the path is skipped for
    ///      this frame. Steady-state reclamation happens safely in
    ///      [`PathAtlas::begin_frame`] (compaction) before any region is
    ///      handed out.
    fn allocate_and_write(
        &mut self,
        key: PathCacheKey,
        w: u32,
        h: u32,
        pixels: &[u8],
    ) -> Option<AtlasRegion> {
        if let Some(region) = self.try_allocate(w, h) {
            self.blit(region.x, region.y, w, h, pixels);
            self.cache.insert(key, region);
            self.dirty = true;
            return Some(region);
        }

        // Grow first — keeps every existing entry at the same coordinates.
        while self.try_grow() {
            if let Some(region) = self.try_allocate(w, h) {
                self.blit(region.x, region.y, w, h, pixels);
                self.cache.insert(key, region);
                self.dirty = true;
                return Some(region);
            }
        }

        // Atlas at max size and still no room. Try eviction — but it will
        // refuse to move any entry already handed out this frame, so if the
        // frame's live working set already fills a max-size atlas this is a
        // no-op and we return `None` (the path is skipped this frame, which is
        // correct: it genuinely doesn't fit). It never corrupts placed paths.
        self.evict_lru();
        if let Some(region) = self.try_allocate(w, h) {
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

    /// Mid-frame, last-resort space reclamation.
    ///
    /// Eviction must **never** move an entry that has already been handed out
    /// this frame: the renderer's pre-pass caches each path's `AtlasRegion` in
    /// `path_regions[..]` and reads it back later in the same frame, so moving
    /// those pixels makes the cached region sample the wrong location (flicker
    /// / wrong-pixel rendering on path-heavy widgets like LineChart and
    /// PieChart). A shelf packer cannot reclaim the fragmented space held by
    /// older entries without repacking the live ones, so:
    ///
    /// * If **no** region has been handed out this frame, clearing the whole
    ///   atlas is safe — do it (the next lookups re-rasterize from a clean
    ///   atlas, and `try_grow` already ran).
    /// * If **any** region is live this frame, we leave the atlas untouched.
    ///   `allocate_and_write` then returns `None` and the path is skipped for
    ///   one frame — never corrupted.
    ///
    /// Steady-state reclamation that *does* repack happens in
    /// [`PathAtlas::begin_frame`], where no region is live yet.
    fn evict_lru(&mut self) {
        if self.cache.is_empty() {
            return;
        }

        let current = self.current_frame;
        let any_live = self.cache.values().any(|r| r.last_used_frame == current);
        if any_live {
            // Can't reclaim without moving a live entry — bail out.
            return;
        }

        // No live entries — safe to clear everything.
        self.cache.clear();
        self.pixels.fill(0);
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.shelf_height = 0;
        self.dirty = true;
    }

    /// Drop every entry not used on or after `keep_from_frame` and repack the
    /// survivors tightly from the top of the atlas.
    ///
    /// This **moves** surviving entries, so it is only sound when no
    /// `AtlasRegion` has been handed out for the current frame yet — i.e. it
    /// must be called only from [`PathAtlas::begin_frame`].
    fn compact(&mut self, keep_from_frame: u64) {
        // Read survivors out before we wipe the backing pixels. `read_region`
        // and `cache.iter()` both borrow `&self` immutably, so this is fine.
        let mut survivors: Vec<(PathCacheKey, AtlasRegion, Vec<u8>)> = self
            .cache
            .iter()
            .filter(|(_, r)| r.last_used_frame >= keep_from_frame)
            .map(|(k, r)| (*k, *r, self.read_region(*r)))
            .collect();

        self.cache.clear();
        self.pixels.fill(0);
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.shelf_height = 0;
        self.dirty = true;

        // Repack tallest-first to limit shelf wastage.
        survivors.sort_by_key(|(_, r, _)| std::cmp::Reverse(r.h));
        for (key, old_region, pixels) in survivors {
            if let Some(new_region) = self.try_allocate(old_region.w, old_region.h) {
                self.blit(
                    new_region.x,
                    new_region.y,
                    new_region.w,
                    new_region.h,
                    &pixels,
                );
                self.cache.insert(
                    key,
                    AtlasRegion {
                        x: new_region.x,
                        y: new_region.y,
                        w: new_region.w,
                        h: new_region.h,
                        last_used_frame: old_region.last_used_frame,
                    },
                );
            }
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
///
/// `geom_scale` scales the path **geometry** into the bitmap (= `scale_factor`
/// for logical strokes, `scale_factor × zoom` for cosmetic ones so the body is
/// sharp at the current zoom). `stroke_scale` scales the **stroke width** (=
/// `scale_factor` always; for cosmetic strokes this bakes a zoom-independent
/// device-pixel thickness). The two are equal for the logical/fill path.
fn rasterize_path(
    path: &Path,
    color: [f32; 4],
    style: &StrokeStyle,
    bounds: [f32; 4],
    geom_scale: f32,
    stroke_scale: f32,
) -> Option<Vec<u8>> {
    let w = (bounds[2] * geom_scale).ceil() as u32;
    let h = (bounds[3] * geom_scale).ceil() as u32;
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
                    (p.x - bounds[0]) * geom_scale,
                    (p.y - bounds[1]) * geom_scale,
                );
            }
            PathCommand::LineTo(p) => {
                pb.line_to(
                    (p.x - bounds[0]) * geom_scale,
                    (p.y - bounds[1]) * geom_scale,
                );
            }
            PathCommand::QuadTo { control, to } => {
                pb.quad_to(
                    (control.x - bounds[0]) * geom_scale,
                    (control.y - bounds[1]) * geom_scale,
                    (to.x - bounds[0]) * geom_scale,
                    (to.y - bounds[1]) * geom_scale,
                );
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                pb.cubic_to(
                    (control1.x - bounds[0]) * geom_scale,
                    (control1.y - bounds[1]) * geom_scale,
                    (control2.x - bounds[0]) * geom_scale,
                    (control2.y - bounds[1]) * geom_scale,
                    (to.x - bounds[0]) * geom_scale,
                    (to.y - bounds[1]) * geom_scale,
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
                    geom_scale,
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
        let line_join = match style.line_join {
            LineJoin::Miter => tiny_skia::LineJoin::Miter,
            LineJoin::Round => tiny_skia::LineJoin::Round,
            LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
        };
        let dash = style
            .dash_pattern
            .as_ref()
            .and_then(|pattern| tiny_skia::StrokeDash::new(pattern.clone(), style.dash_offset));
        let stroke = tiny_skia::Stroke {
            width: style.width * stroke_scale,
            line_cap,
            line_join,
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
    use bastyde_canvas::geometry::Point;

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
        let pixels = rasterize_path(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0, 1.0);
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
        let pixels = rasterize_path(&path, [0.0, 1.0, 0.0, 1.0], &style, bounds, 1.0, 1.0);
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
            .lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0, 1.0)
            .unwrap();
        let r2 = atlas
            .lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0, 1.0)
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
        atlas.lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, bounds, 1.0, 1.0);

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
        p1.commands
            .push(PathCommand::LineTo(Point::new(40.0, 40.0)));
        p1.commands.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.commands.push(PathCommand::LineTo(Point::new(50.0, 0.0)));
        p2.commands
            .push(PathCommand::LineTo(Point::new(50.0, 50.0)));
        p2.commands.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(
                &p1,
                [1.0, 0.0, 0.0, 1.0],
                &style,
                [0.0, 0.0, 40.0, 40.0],
                1.0,
                1.0,
            )
            .expect("p1 fits");

        // p2 doesn't fit in the remaining space → eviction triggers.
        // After the fix, p1 (current-frame) survives and gets repacked.
        let _r2 = atlas.lookup_or_rasterize(
            &p2,
            [0.0, 1.0, 0.0, 1.0],
            &style,
            [0.0, 0.0, 50.0, 50.0],
            1.0,
            1.0,
        );

        // Looking up p1 again must still hit cache (with possibly a new
        // region, but stable across the lookup).
        let r1b = atlas
            .lookup_or_rasterize(
                &p1,
                [1.0, 0.0, 0.0, 1.0],
                &style,
                [0.0, 0.0, 40.0, 40.0],
                1.0,
                1.0,
            )
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
    fn evict_never_moves_live_entry_when_full() {
        // Core invariant for the stale-UV fix: once a region is handed out
        // this frame it is frozen. If a later path can't fit and the atlas is
        // already at max size, the new path is skipped (returns None) — the
        // live entry must NOT be repacked, or `path_regions[..]` would sample
        // the wrong pixels later in the same frame.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.max_size = 64; // forbid growth so eviction is the only path
        atlas.begin_frame();

        let mut p1 = Path::new();
        p1.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p1.commands.push(PathCommand::LineTo(Point::new(60.0, 0.0)));
        p1.commands
            .push(PathCommand::LineTo(Point::new(60.0, 60.0)));
        p1.commands.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.commands.push(PathCommand::LineTo(Point::new(62.0, 0.0)));
        p2.commands
            .push(PathCommand::LineTo(Point::new(62.0, 62.0)));
        p2.commands.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(&p1, [1.0, 0.0, 0.0, 1.0], &style, [0.0, 0.0, 60.0, 60.0], 1.0, 1.0)
            .expect("p1 fits");

        // p2 can't fit, can't grow → must be skipped, not placed by moving p1.
        let r2 =
            atlas.lookup_or_rasterize(&p2, [0.0, 1.0, 0.0, 1.0], &style, [0.0, 0.0, 62.0, 62.0], 1.0, 1.0);
        assert!(r2.is_none(), "an unfittable path is skipped, never placed by evicting a live entry");

        // p1's region is byte-for-byte unchanged.
        let r1b = atlas
            .lookup_or_rasterize(&p1, [1.0, 0.0, 0.0, 1.0], &style, [0.0, 0.0, 60.0, 60.0], 1.0, 1.0)
            .expect("p1 still cached");
        assert_eq!(r1.x, r1b.x, "live entry must not move");
        assert_eq!(r1.y, r1b.y, "live entry must not move");
    }

    #[test]
    fn begin_frame_compacts_stale_entries() {
        // `begin_frame` is the safe point to repack: nothing is handed out
        // for the new frame yet. A near-full atlas with entries not used on
        // the last completed frame compacts them away.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.begin_frame(); // frame 1

        let mut path = Path::new();
        path.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.commands.push(PathCommand::LineTo(Point::new(8.0, 0.0)));
        path.commands.push(PathCommand::LineTo(Point::new(8.0, 8.0)));
        path.commands.push(PathCommand::Close);
        let style = StrokeStyle::solid(0.0);
        atlas
            .lookup_or_rasterize(&path, [1.0, 0.0, 0.0, 1.0], &style, [0.0, 0.0, 8.0, 8.0], 1.0, 1.0)
            .expect("entry fits");
        assert_eq!(atlas.cache.len(), 1);

        atlas.begin_frame(); // frame 2 — keep_from = 1, entry (used f1) kept
        assert_eq!(atlas.cache.len(), 1, "entry from the last completed frame is kept");

        atlas.begin_frame(); // frame 3 — keep_from = 2, entry (used f1) is stale
        assert!(atlas.cache.is_empty(), "stale entry compacted away on begin_frame");
    }

    #[test]
    fn atlas_grow() {
        let mut atlas = PathAtlas::new(16, 16);
        assert!(atlas.try_grow());
        assert_eq!(atlas.width, 32);
        assert_eq!(atlas.height, 32);
    }

    #[test]
    fn growth_preserves_earlier_frame_regions() {
        // Regression: when a single frame inserts more paths than fit in
        // the initial atlas, we must grow rather than evict — eviction
        // repacks current-frame survivors at fresh coordinates,
        // invalidating any AtlasRegion the renderer already cached for
        // them earlier in the same frame. With grow-first, the first
        // entry's region stays valid throughout the frame.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.begin_frame();

        let mut p1 = Path::new();
        p1.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p1.commands.push(PathCommand::LineTo(Point::new(50.0, 0.0)));
        p1.commands
            .push(PathCommand::LineTo(Point::new(50.0, 50.0)));
        p1.commands.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.commands.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.commands.push(PathCommand::LineTo(Point::new(60.0, 0.0)));
        p2.commands
            .push(PathCommand::LineTo(Point::new(60.0, 60.0)));
        p2.commands.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(
                &p1,
                [1.0, 0.0, 0.0, 1.0],
                &style,
                [0.0, 0.0, 50.0, 50.0],
                1.0,
                1.0,
            )
            .expect("p1 fits");

        // p2 doesn't fit alongside p1 in 64×64 → atlas should grow,
        // not evict. After growth, p1's region must still be at the
        // same coordinates we got back the first time.
        let _r2 = atlas
            .lookup_or_rasterize(
                &p2,
                [0.0, 1.0, 0.0, 1.0],
                &style,
                [0.0, 0.0, 60.0, 60.0],
                1.0,
                1.0,
            )
            .expect("p2 fits after grow");

        let r1_after = atlas
            .lookup_or_rasterize(
                &p1,
                [1.0, 0.0, 0.0, 1.0],
                &style,
                [0.0, 0.0, 50.0, 50.0],
                1.0,
                1.0,
            )
            .expect("p1 still cached");
        assert_eq!(r1.x, r1_after.x, "p1 must not move when atlas grows");
        assert_eq!(r1.y, r1_after.y, "p1 must not move when atlas grows");
    }

    #[test]
    fn cosmetic_path_raster_is_zoom_aware_logical_is_not() {
        // A cosmetic stroke rasterizes its body at the view zoom (so it stays
        // sharp and matches the transform-scaled display quad 1:1) — the
        // raster dimensions scale with zoom. A logical stroke ignores zoom
        // (one bitmap, stretched by the quad), so its raster size and cache
        // entry are zoom-independent.
        let mut atlas = PathAtlas::new(512, 512);
        atlas.begin_frame();
        let mut path = Path::new();
        path.commands
            .push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.commands
            .push(PathCommand::LineTo(Point::new(40.0, 0.0)));
        let bounds = [0.0, 0.0, 40.0, 4.0];
        let color = [0.0, 0.0, 0.0, 1.0];

        let cosmetic = StrokeStyle::hairline(2.0);
        let r1 = atlas
            .lookup_or_rasterize(&path, color, &cosmetic, bounds, 1.0, 1.0)
            .unwrap();
        let r2 = atlas
            .lookup_or_rasterize(&path, color, &cosmetic, bounds, 1.0, 2.0)
            .unwrap();
        assert_eq!(r1.w, 40, "cosmetic body at zoom 1: 40·sf1·zoom1");
        assert_eq!(
            r2.w, 80,
            "cosmetic body at zoom 2: 40·sf1·zoom2 (zoom-aware)"
        );

        let logical = StrokeStyle::solid(2.0);
        let l1 = atlas
            .lookup_or_rasterize(&path, color, &logical, bounds, 1.0, 1.0)
            .unwrap();
        let l2 = atlas
            .lookup_or_rasterize(&path, color, &logical, bounds, 1.0, 4.0)
            .unwrap();
        assert_eq!(l1.w, l2.w, "logical raster size ignores zoom");
        assert_eq!(
            (l1.x, l1.y),
            (l2.x, l2.y),
            "logical hits the same cache entry"
        );

        // Same width/color/dims but different stroke space must not collide.
        let k_cos = PathCacheKey::new(&path, color, &cosmetic, 40, 4);
        let k_log = PathCacheKey::new(&path, color, &logical, 40, 4);
        assert_ne!(
            k_cos, k_log,
            "cache key must distinguish cosmetic vs logical"
        );
    }
}
