// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Path atlas: CPU rasterizes paths with tiny-skia, caches results in a texture atlas with LRU eviction.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use teksilo_canvas::geometry::{Point, Rect};
use teksilo_canvas::paint::{FillRule, LineCap, LineJoin, StrokeSpace, StrokeStyle};
use teksilo_canvas::path::{Path, PathCommand};

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

/// Transparent margin reserved after each entry, so no two entries touch.
///
/// The atlas is sampled with `FilterMode::Linear` and each quad's UVs run to
/// its region's outer edge. Whenever a quad is not pixel-exact on its region
/// — any path under a transform, where snapping is deliberately off (see
/// [`PathAtlas::lookup_or_rasterize`]) — an edge fragment's bilinear kernel
/// reaches past the region, and edge-to-edge packing made that the
/// *neighbouring icon's* pixels. One transparent row and column keeps the
/// worst case a fade to nothing rather than a smear of unrelated ink. The
/// glyph atlas has always reserved the same gutter.
const ENTRY_GUTTER_PX: u32 = 1;

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

/// A rasterized path plus the **exact** rect it must be drawn at.
///
/// The two travel together because they are one decision, not two. The atlas
/// bitmap is rasterized on its own integer grid; if the quad that samples it
/// is placed or sized even slightly differently, every texel is resampled
/// through the atlas's `FilterMode::Linear` and the coverage mask smears.
/// A 16 dp line-style icon does not survive that: a 1 px stroke drawn at a
/// half-pixel offset peaks at **48 % coverage** instead of 100 %, and
/// sub-pixel dash gaps close up entirely, so a dashed ring renders as a grey
/// haze. Returning the rect from the same call that decides the raster is
/// what stops the two from ever disagreeing again.
///
/// See [`PathAtlas::lookup_or_rasterize`] for when the rect is snapped.
#[derive(Debug, Clone, Copy)]
pub struct PathPlacement {
    /// Where the coverage mask lives in the atlas texture.
    pub region: AtlasRegion,
    /// `[x, y, w, h]` in **pre-transform device pixels** — the quad the
    /// caller must emit. When snapped this is integral and exactly
    /// `region.w × region.h`, so the mask samples 1:1 onto whole pixels.
    pub device_rect: [f32; 4],
}

/// Cache key derived from path geometry + stroke style + rasterized size +
/// the device-space origin the bitmap was baked against + the geometry
/// scale it was baked at.
///
/// `geom_scale` is in the key because it is a rasterization input the size
/// does not always recover: `w`/`h` are the bounds *ceiled* into texels, so
/// a sub-pixel path aliases several scales onto one bitmap size, and a
/// cosmetic stroke's `geom_scale` moves continuously with the view zoom.
/// It scales the dash pattern (a length along the path), so a collision
/// would serve a bitmap whose dashes are cut for a different zoom.
///
/// Deliberately does **not** include color: the atlas now always
/// rasterizes an opaque-white AA coverage mask (see [`rasterize_path`]),
/// so a solid fill and a gradient fill of identical geometry share one
/// atlas entry — the color/gradient tint is applied by the GPU at draw
/// time, not baked into the bitmap.
///
/// # The geometry is read as one word, not walked
///
/// [`Path::stamp`] is a rolling 64-bit fold of the command list, maintained
/// as commands are appended, so building the key is O(1) in the path's
/// length. It used to hash every `PathCommand`, which made a **cache hit**
/// cost O(n) — linear in the point count, with the measured table in
/// docs/ink.md §5, the one place those figures are written down.
/// That is the whole of the ink cliff — a wet stroke that grows by a point
/// per pointer sample paid a full walk every frame just to discover the
/// bitmap it wanted was already resident, so one stroke was quadratic before
/// the rasterizer was even reached.
///
/// The stamp is content-addressed exactly as the walk was: two paths with the
/// same commands produce the same key, two with different commands collide
/// with 64-bit probability. Nothing else about the cache's semantics moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PathCacheKey(u64);

impl PathCacheKey {
    fn new(
        path: &Path,
        style: &StrokeStyle,
        fill_rule: FillRule,
        origin: [f32; 2],
        w: u32,
        h: u32,
        geom_scale: f32,
    ) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        // The path's geometry, as one word. See this type's doc comment.
        path.stamp().hash(&mut hasher);
        // Hash stroke style
        style.width.to_bits().hash(&mut hasher);
        std::mem::discriminant(&style.line_cap).hash(&mut hasher);
        std::mem::discriminant(&style.line_join).hash(&mut hasher);
        if let Some(ref pattern) = style.dash_pattern {
            for &v in pattern {
                v.to_bits().hash(&mut hasher);
            }
        }
        style.dash_offset.to_bits().hash(&mut hasher);
        style.miter_limit.to_bits().hash(&mut hasher);
        // Cosmetic vs logical strokes bake differently (constant device width
        // vs zoom-scaled), so they must not share a cache entry.
        std::mem::discriminant(&style.space).hash(&mut hasher);
        // Winding vs even-odd fill produce different pixels for the same path.
        std::mem::discriminant(&fill_rule).hash(&mut hasher);
        // Hash rasterized dimensions
        w.hash(&mut hasher);
        h.hash(&mut hasher);
        // And the device-space origin the bitmap was baked against. The
        // path's own commands are absolute, so two *different* paths already
        // key apart — but the SAME path drawn once under the identity
        // transform (snapped to the pixel grid) and once under a transform
        // (not snapped) wants two different bitmaps at the same dimensions.
        // Without the origin here the second draw would silently reuse the
        // first's phase.
        origin[0].to_bits().hash(&mut hasher);
        origin[1].to_bits().hash(&mut hasher);
        // And the scale the geometry (and the dash pattern along it) was
        // baked at — see this type's doc comment for why `w`/`h` don't
        // already say it.
        geom_scale.to_bits().hash(&mut hasher);
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
    ///
    /// The default is the size this renderer wants; [`Self::cap_max_size`]
    /// lowers it to what the device can actually create.
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
    /// How many paths have been skipped because they could never fit the atlas.
    ///
    /// Such a path is simply not drawn. That is a silent hole in the frame, so it is
    /// counted rather than swallowed: a non-zero value means some geometry is being
    /// asked to rasterize larger than [`max_size`](Self::max_size), which is almost
    /// always a layout bug upstream (see [`Self::lookup_or_rasterize`]).
    oversize_skips: u64,
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
            oversize_skips: 0,
        }
    }

    /// Lower the growth cap to what the GPU can actually allocate.
    ///
    /// The 4096 default is this renderer's own ceiling, not a fact about the
    /// hardware. `Limits::downlevel_defaults` guarantees only 2048, and the
    /// window path can legitimately open a device on an adapter's own limits,
    /// which on GLES-3-class hardware may sit below 4096. Growing past what the
    /// device allows is a `create_texture` validation error at the first
    /// path-heavy frame — a crash on the machine least able to report it.
    ///
    /// Only ever lowers: a device that allows more than this renderer asks for
    /// does not get a bigger atlas, because the cap is also a memory bound.
    pub fn cap_max_size(&mut self, device_max: u32) {
        self.max_size = self.max_size.min(device_max);
    }

    /// How many paths have been skipped for being too large to ever fit the atlas.
    ///
    /// Each one is a path that simply was not drawn. Non-zero means some geometry is
    /// rasterizing bigger than `max_size` — upstream, that is a
    /// layout that has run away (an overlay spanning a whole scrolled document, a
    /// shape scaled by a runaway transform), and it is worth chasing rather than
    /// leaving as a hole in the frame.
    pub fn oversize_skips(&self) -> u64 {
        self.oversize_skips
    }

    /// How many distinct masks are resident.
    ///
    /// The observable side of the cache key: two draws that share a key share
    /// an entry, and two that do not each get one. A test asserting that
    /// identical geometry is not rasterized twice reads this.
    pub fn entry_count(&self) -> usize {
        self.cache.len()
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
        let near_full =
            self.shelf_y.saturating_add(self.shelf_height) + COMPACT_SLACK_PX >= self.height;
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
    /// The rasterized bitmap is always an **opaque-white AA coverage
    /// mask** — color is applied by the GPU at draw time (solid fills tint
    /// it via the quad pipeline; gradients sample an analytic gradient in
    /// `path_gradient.wgsl` and modulate by the mask's alpha channel), so
    /// this function takes no color and two fills of identical geometry
    /// share one atlas entry regardless of their paint.
    ///
    /// `zoom` is the uniform scale of the view transform active where the path
    /// is drawn. For a **cosmetic** stroke ([`StrokeSpace::Device`]) the body
    /// is rasterized at the current zoom (so it stays sharp, matching the
    /// transform-scaled display quad 1:1) while the stroke is baked at a
    /// zoom-independent device width — the border holds a constant
    /// device-pixel thickness at any zoom. **Logical** strokes ignore `zoom`
    /// (the body bitmap is stretched by the display quad, as before).
    ///
    /// `snap` asks for the quad to be aligned to whole device pixels and the
    /// bitmap baked to match, so the mask samples 1:1 — pass it when the
    /// effective transform is the identity, and only then (see the body for
    /// why). The returned [`PathPlacement`] carries the rect the caller must
    /// draw; it is not to be re-derived from `bounds`.
    #[allow(clippy::too_many_arguments)] // rasterization params; bundling adds no clarity
    pub fn lookup_or_rasterize(
        &mut self,
        path: &Path,
        style: &StrokeStyle,
        fill_rule: FillRule,
        bounds: [f32; 4],
        scale_factor: f32,
        zoom: f32,
        snap: bool,
    ) -> Option<PathPlacement> {
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

        // The quad the caller will emit, in pre-transform device pixels.
        let dx = bounds[0] * scale_factor;
        let dy = bounds[1] * scale_factor;
        let dw = bounds[2] * scale_factor;
        let dh = bounds[3] * scale_factor;

        // Snap the quad out to whole device pixels and bake the bitmap
        // against that same origin, so one texel lands on one pixel and the
        // sampler has nothing to interpolate. Without this a path's mask is
        // rasterized on its own integer grid and then drawn wherever layout
        // put it — `Rect::expand` alone leaves a 16 dp ring's bounds at
        // `x = 1.5`, and a half-pixel bilinear smear costs that ring more
        // than half its ink (see `PathPlacement`). The glyph pipeline has
        // always done this; see `QuadVertex::from_glyph_quad_transformed`'s
        // `one_to_one` branch.
        //
        // Only under the identity transform (`snap`, decided by the caller):
        // under a scale the mask is being resampled anyway, and under a
        // translate animation rounding the origin would make the path step
        // between pixels instead of gliding. The `geom_scale` check keeps a
        // cosmetic (device-space) stroke out of it unless its zoom is 1,
        // since its bitmap is baked at zoom while its quad is not.
        let ox = dx.floor();
        let oy = dy.floor();
        let snapped_rect = [
            ox,
            oy,
            ((dx + dw).ceil() - ox).max(1.0),
            ((dy + dh).ceil() - oy).max(1.0),
        ];
        // Snapping grows the bitmap by up to a pixel on each axis. A path
        // sitting exactly on `max_size` would then be rejected below and
        // simply not drawn, so give up the sharpness rather than the path —
        // at that size it is one texel in four thousand anyway.
        let snapped = snap
            && (geom_scale - scale_factor).abs() < 1e-4
            && snapped_rect[2] as u32 <= self.max_size
            && snapped_rect[3] as u32 <= self.max_size;
        let device_rect = if snapped {
            snapped_rect
        } else {
            [dx, dy, dw, dh]
        };

        // Device-space origin the bitmap is baked against, and its size.
        let (raster_origin, raster_w, raster_h) = if snapped {
            (
                [device_rect[0], device_rect[1]],
                device_rect[2] as u32,
                device_rect[3] as u32,
            )
        } else {
            (
                [bounds[0] * geom_scale, bounds[1] * geom_scale],
                (bounds[2] * geom_scale).ceil() as u32,
                (bounds[3] * geom_scale).ceil() as u32,
            )
        };
        if raster_w == 0 || raster_h == 0 {
            return None;
        }

        // A path that can never fit the atlas must never be rasterized.
        //
        // Growth is capped at `max_size`, so `allocate_and_write` is guaranteed to
        // fail for anything larger — meaning the bitmap would be built, thrown away,
        // and rebuilt from scratch on the very next frame, forever. That is not a
        // slow frame, it is a permanent freeze: a single 7573x7563 path (one hazard
        // stripe painted across a tall overflow strip) is a 229 MB rasterization,
        // and redoing it every frame pinned the UI thread at 100% CPU for as long as
        // the path stayed on screen.
        //
        // Returning `None` here is not a new failure mode — it is the one the caller
        // already handled (and already reached, just hundreds of megabytes later):
        // the path is skipped for this frame. Bailing out *before* the raster turns
        // an unbounded stall into a dropped draw.
        if raster_w > self.max_size || raster_h > self.max_size {
            self.oversize_skips += 1;
            return None;
        }

        let key = PathCacheKey::new(
            path,
            style,
            fill_rule,
            raster_origin,
            raster_w,
            raster_h,
            geom_scale,
        );

        // Cache hit
        if let Some(region) = self.cache.get_mut(&key) {
            region.last_used_frame = self.current_frame;
            return Some(PathPlacement {
                region: *region,
                device_rect,
            });
        }

        // Rasterize — always opaque white; see PathCacheKey and this
        // function's doc comment for why color is not a parameter.
        let pixels = rasterize_path(
            path,
            style,
            fill_rule,
            raster_origin,
            raster_w,
            raster_h,
            geom_scale,
            stroke_scale,
        )?;
        let region = self.allocate_and_write(key, raster_w, raster_h, &pixels)?;
        Some(PathPlacement {
            region,
            device_rect,
        })
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
        // The region is `w × h`; the shelf cursor advances past a further
        // `ENTRY_GUTTER_PX` so the next entry cannot abut this one. Only the
        // region has to fit — a gutter running off the right edge costs
        // nothing, since the cursor is past the edge either way.
        if self.shelf_x + w <= self.width && self.shelf_y + h.max(self.shelf_height) <= self.height
        {
            let region = AtlasRegion {
                x: self.shelf_x,
                y: self.shelf_y,
                w,
                h,
                last_used_frame: self.current_frame,
            };
            self.shelf_x += w + ENTRY_GUTTER_PX;
            self.shelf_height = self.shelf_height.max(h + ENTRY_GUTTER_PX);
            return Some(region);
        }

        // Start a new shelf
        let new_y = self.shelf_y + self.shelf_height;
        if w <= self.width && new_y + h <= self.height {
            self.shelf_y = new_y;
            self.shelf_x = w + ENTRY_GUTTER_PX;
            self.shelf_height = h + ENTRY_GUTTER_PX;
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
    /// `path_placements[..]` and reads it back later in the same frame, so moving
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
    /// survivors during compaction). Returns an RGBA buffer of `w*h*4` bytes.
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

/// Rasterize a path to RGBA pixels using tiny-skia, always as an
/// **opaque-white AA coverage mask** (RGB = white, alpha = coverage).
/// Color is intentionally not a parameter — see [`PathAtlas::lookup_or_rasterize`]:
/// the mask is tinted/gradient-sampled by the GPU at draw time (matching
/// `quad.wgsl`'s `flags = 0` monochrome-mask convention), so rasterization
/// only needs to bake the geometry's AA coverage, letting solid and
/// gradient fills of the same path share one atlas entry. This also fixes
/// a pre-existing double-alpha bug: baking a translucent color into the
/// bitmap AND multiplying by that same color's alpha again at draw time
/// squared the effective alpha.
///
/// `geom_scale` scales the path **geometry** into the bitmap (= `scale_factor`
/// for logical strokes, `scale_factor × zoom` for cosmetic ones so the body is
/// sharp at the current zoom). `stroke_scale` scales the **stroke width** (=
/// `scale_factor` always; for cosmetic strokes this bakes a zoom-independent
/// device-pixel thickness). The two are equal for the logical/fill path.
///
/// `origin` is the bitmap's top-left in **device pixels**: a path point `p`
/// lands at `p * geom_scale - origin`. It is a device-space origin rather
/// than the path's own bounds because the caller may have snapped it to the
/// pixel grid, and the bitmap has to be baked against the very grid the quad
/// will be drawn on — see [`PathAtlas::lookup_or_rasterize`]. `w` / `h` are
/// the bitmap's size in texels, likewise decided by the caller.
#[allow(clippy::too_many_arguments)]
fn rasterize_path(
    path: &Path,
    style: &StrokeStyle,
    fill_rule: FillRule,
    origin: [f32; 2],
    w: u32,
    h: u32,
    geom_scale: f32,
    stroke_scale: f32,
) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }

    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;

    let sk_path = build_sk_path(path, geom_scale, origin)?;

    // Always opaque white — a pure AA coverage mask. Color/gradient tint
    // is applied by the GPU at draw time (see this function's doc comment).
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::from_rgba(1.0, 1.0, 1.0, 1.0)?),
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
        // Dash lengths are measured ALONG the path, so they live in the
        // path's units and must be scaled by `geom_scale` — the same factor
        // the geometry was baked with — not by `stroke_scale`, which is the
        // across-the-path thickness. Passing the pattern unscaled made a
        // dash shorter by exactly `1 / geom_scale`: a `dashed(2, 4, 4)` line
        // showed 3 dashes at scale 1 and 5 at scale 2, so every dashed
        // stroke in the framework — chart gridlines included — was drawn
        // with half-length dashes on a 2× HiDPI display, and a cosmetic
        // dashed stroke re-cut its pattern on every zoom step.
        //
        // `StrokeSpace::Device` still only pins the *thickness*: the doc on
        // `StrokeStyle::hairline` says position follows the full transform,
        // and a longitudinal measure is position, not thickness. So a
        // cosmetic dashed connector zooms its dashes with its geometry while
        // holding its width.
        let dash = style.dash_pattern.as_ref().and_then(|pattern| {
            tiny_skia::StrokeDash::new(
                pattern.iter().map(|d| d * geom_scale).collect(),
                style.dash_offset * geom_scale,
            )
        });
        let stroke = tiny_skia::Stroke {
            width: style.width * stroke_scale,
            line_cap,
            line_join,
            miter_limit: style.miter_limit,
            dash,
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
        let sk_rule = match fill_rule {
            FillRule::Winding => tiny_skia::FillRule::Winding,
            FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
        };
        pixmap.fill_path(
            &sk_path,
            &paint,
            sk_rule,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    Some(pixmap.data().to_vec())
}

/// Translate a [`Path`] into a tiny-skia path in bitmap space.
///
/// Scale first, then subtract the device-space `origin` — NOT the other way
/// round: the origin may be snapped to a pixel the path's own bounds do not
/// sit on, so it is not a multiple of `geom_scale` and cannot be folded into
/// the path's units.
///
/// The command walk mirrors [`Path::flatten`]'s: the flattener is the oracle
/// for where a subpath begins, because it is what the scene tier hit-tests
/// against. A rasteriser that opened a subpath somewhere else would paint ink
/// no click could reach — see [`emit_arc`], the one command where tiny-skia's
/// own defaults do not already agree.
fn build_sk_path(path: &Path, geom_scale: f32, origin: [f32; 2]) -> Option<tiny_skia::Path> {
    let bx = |x: f32| x * geom_scale - origin[0];
    let by = |y: f32| y * geom_scale - origin[1];
    let mut pb = tiny_skia::PathBuilder::new();
    let mut cursor = SubpathCursor::new();
    for cmd in path.commands() {
        match *cmd {
            PathCommand::MoveTo(p) => {
                pb.move_to(bx(p.x), by(p.y));
                cursor.open_at(p);
            }
            PathCommand::LineTo(p) => {
                open_implicit(&mut pb, &cursor, geom_scale, origin);
                pb.line_to(bx(p.x), by(p.y));
                cursor.extend_to(p);
            }
            PathCommand::QuadTo { control, to } => {
                open_implicit(&mut pb, &cursor, geom_scale, origin);
                pb.quad_to(bx(control.x), by(control.y), bx(to.x), by(to.y));
                cursor.extend_to(to);
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                open_implicit(&mut pb, &cursor, geom_scale, origin);
                pb.cubic_to(
                    bx(control1.x),
                    by(control1.y),
                    bx(control2.x),
                    by(control2.y),
                    bx(to.x),
                    by(to.y),
                );
                cursor.extend_to(to);
            }
            PathCommand::ArcTo {
                rect,
                start_angle,
                sweep_angle,
            } => {
                emit_arc(
                    &mut pb,
                    &mut cursor,
                    rect,
                    start_angle,
                    sweep_angle,
                    geom_scale,
                    origin,
                );
            }
            PathCommand::Close => {
                pb.close();
                cursor.close();
            }
        }
    }
    pb.finish()
}

/// The subpath bookkeeping [`Path::flatten`] keeps as it walks a path's
/// commands, mirrored so the rasteriser can ask the same question the
/// flattener asks: *is a subpath open, and where is its current point?*
///
/// Every segment command consults it, because tiny-skia's
/// `inject_move_to_if_needed` and `flatten` open an implicit subpath at two
/// different points. tiny-skia opens at the last `MoveTo` **as fed to the
/// builder**, i.e. already in bitmap space, and at bitmap `(0, 0)` for a path
/// that has no `MoveTo` at all; `flatten` opens at the current point in the
/// path's **own** units. After a [`PathCommand::Close`] the two agree, because
/// the last `MoveTo` is the point `flatten` re-seeds its cursor with. For a
/// path opening on a bare segment they agree only at `origin == [0, 0]` — and
/// the atlas pins the origin to zero unless the path carries a negative
/// coordinate, which is precisely when the divergence becomes reachable.
///
/// So the walk opens every implicit subpath explicitly, at the point `flatten`
/// would open it, and tiny-skia's injection never fires.
#[derive(Debug, Clone, Copy)]
struct SubpathCursor {
    /// Current point, in the path's own units.
    at: Point,
    /// First point of the subpath being built, in the path's own units.
    start: Point,
    /// Whether a subpath is open — `flatten`'s `!current.is_empty()`.
    open: bool,
}

/// Open a subpath at the flattener's current point when a segment arrives with
/// none open, so tiny-skia never injects one of its own at bitmap `(0, 0)`.
///
/// A no-op while a subpath is open, which is every command in a well-formed
/// path.
fn open_implicit(
    pb: &mut tiny_skia::PathBuilder,
    cursor: &SubpathCursor,
    geom_scale: f32,
    origin: [f32; 2],
) {
    if !cursor.open {
        pb.move_to(
            cursor.at.x * geom_scale - origin[0],
            cursor.at.y * geom_scale - origin[1],
        );
    }
}

impl SubpathCursor {
    fn new() -> Self {
        Self {
            at: Point::ZERO,
            start: Point::ZERO,
            open: false,
        }
    }

    /// A `MoveTo`: end any open subpath and start one at `p`.
    fn open_at(&mut self, p: Point) {
        self.at = p;
        self.start = p;
        self.open = true;
    }

    /// A segment drawn *from the current point*. When no subpath is open both
    /// `flatten` and tiny-skia open one at that current point, so the start
    /// is the cursor, not `to`.
    fn extend_to(&mut self, to: Point) {
        if !self.open {
            self.start = self.at;
            self.open = true;
        }
        self.at = to;
    }

    /// A `Close`: the current point returns to the subpath's start, and the
    /// next segment opens a fresh subpath.
    fn close(&mut self) {
        if self.open {
            self.at = self.start;
            self.open = false;
        }
    }
}

/// Feed one [`PathCommand::ArcTo`] into a tiny-skia path builder, scaled into
/// bitmap space.
///
/// The arc → cubic maths lives once, in [`teksilo_canvas::arc_to_cubics`] —
/// `Path::flatten` and the scene tier's hit-testing read the same conversion,
/// so a rendered arc and a clicked arc can never be different curves. This
/// function's remaining jobs are the atlas's own — scale each returned point
/// by `scale_factor`, subtract the bitmap `origin`, emit — plus the one place
/// tiny-skia's defaults diverge from the flattener: **where the subpath
/// starts.**
///
/// An arc is the only command that can begin somewhere other than the current
/// point, so it is the only one that has to say whether it *opens* a subpath
/// or *continues* one. `flatten` opens at the arc's own first point; tiny-skia's
/// `line_to` would instead inject a `MoveTo` of its own — `(0, 0)` **in bitmap
/// space** for a path that opens with a bare arc (so the phantom vertex sits
/// at the atlas bitmap's top-left corner and moves with the scale factor), or
/// the *previous* subpath's start after a `Close`. Either way the emitted
/// spoke is ink `Path::contains_point` cannot see: a lone 90° arc came out as
/// a filled triangle spanning the whole bitmap, 30× the area of the quarter-arc
/// the shape reports. So open the subpath explicitly, and keep the straight
/// connector only where `flatten` keeps one — when a subpath is already open
/// and its current point is not the arc's start (a rounded rect's edge running
/// into its corner).
///
/// `start_angle` and `sweep_angle` are in **degrees**, matching the public
/// `Path::arc_to` API and its call sites (`Path::circle`,
/// `Path::rounded_rect`). `rect` is the arc's bounding rectangle in the path's
/// own units; `origin` is the bitmap's top-left in device pixels, subtracted
/// after scaling for the reason [`build_sk_path`] gives.
fn emit_arc(
    pb: &mut tiny_skia::PathBuilder,
    cursor: &mut SubpathCursor,
    rect: Rect,
    start_angle: f32,
    sweep_angle: f32,
    scale_factor: f32,
    origin: [f32; 2],
) {
    let map = |p: Point| {
        (
            p.x * scale_factor - origin[0],
            p.y * scale_factor - origin[1],
        )
    };
    for seg in teksilo_canvas::arc_to_cubics(rect, start_angle, sweep_angle) {
        let (sx, sy) = map(seg.from);
        if !cursor.open {
            pb.move_to(sx, sy);
            cursor.open_at(seg.from);
        } else if cursor.at != seg.from {
            pb.line_to(sx, sy);
        }
        let (c1x, c1y) = map(seg.control1);
        let (c2x, c2y) = map(seg.control2);
        let (tx, ty) = map(seg.to);
        pb.cubic_to(c1x, c1y, c2x, c2y, tx, ty);
        cursor.extend_to(seg.to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_canvas::geometry::Point;

    /// The device cap lowers the growth ceiling and never raises it.
    ///
    /// Both directions matter. A device that allows less than this renderer
    /// wants must win, or the first atlas growth past its
    /// `max_texture_dimension_2d` is a `create_texture` validation error — a
    /// crash on downlevel hardware. A device that allows *more* must not win,
    /// because the ceiling is also the memory bound: a 16384-capable GPU is not
    /// an invitation to spend 1 GiB on rasterized paths.
    #[test]
    fn cap_max_size_only_lowers() {
        let mut atlas = PathAtlas::new(512, 512);
        let default_cap = atlas.max_size;

        atlas.cap_max_size(16384);
        assert_eq!(
            atlas.max_size, default_cap,
            "a device with more headroom must not raise the renderer's own ceiling"
        );

        atlas.cap_max_size(2048);
        assert_eq!(
            atlas.max_size, 2048,
            "a device that allows less than the renderer wants must lower the ceiling"
        );

        atlas.cap_max_size(4096);
        assert_eq!(
            atlas.max_size, 2048,
            "capping is a floor-taking operation, so it never undoes an earlier cap"
        );
    }

    /// Growth stops at the capped size, not at the compiled-in default.
    ///
    /// `cap_max_size` would be decorative if `grow` still doubled past it.
    #[test]
    fn growth_honours_the_device_cap() {
        let mut atlas = PathAtlas::new(512, 512);
        atlas.cap_max_size(1024);

        while atlas.try_grow() {}

        assert!(
            atlas.width <= 1024 && atlas.height <= 1024,
            "atlas grew to {}x{}, past the device cap of 1024",
            atlas.width,
            atlas.height
        );
    }

    /// A path larger than the atlas can ever hold must be rejected **before** it is
    /// rasterized — not after.
    ///
    /// The atlas grows only up to `max_size`, so `allocate_and_write` could never
    /// store such a path: it was rasterized, discarded, and rasterized again on the
    /// next frame, forever. The geometry below is the one that actually shipped the
    /// freeze — a single 45° hazard band across a 7563px-tall overflow strip, whose
    /// bounding box is a 229 MB bitmap. Redoing that every frame pinned the UI thread
    /// at 100% CPU and the app never recovered.
    ///
    /// If this test ever hangs rather than fails, the guard is gone.
    #[test]
    fn a_path_too_big_for_the_atlas_is_never_rasterized() {
        let mut atlas = PathAtlas::new(256, 256);

        // The exact parallelogram from the freeze: height 7563, width 7563 + PITCH.
        let (h, pitch) = (7563.0_f32, 10.0_f32);
        let w = h + pitch;
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(pitch, 0.0)));
        path.push(PathCommand::LineTo(Point::new(w, h)));
        path.push(PathCommand::LineTo(Point::new(h, h)));
        path.push(PathCommand::Close);

        let before = atlas.cache.len();
        let region = atlas.lookup_or_rasterize(
            &path,
            &StrokeStyle::solid(0.0),
            FillRule::Winding,
            [0.0, 0.0, w, h],
            1.0,
            1.0,
            false,
        );

        assert!(
            region.is_none(),
            "a {w}x{h} path cannot fit an atlas capped at {} — it must be skipped, \
             not rasterized into a 229 MB bitmap that is then thrown away",
            atlas.max_size
        );
        assert_eq!(
            atlas.cache.len(),
            before,
            "the rejected path must not leave a cache entry behind"
        );
        // `is_none()` alone proves nothing: BEFORE the guard existed the call also
        // returned None — it just rasterized 229 MB and failed to allocate first,
        // which is precisely the bug. What must be asserted is that we bailed out
        // *early*, so pin the counter that only the pre-raster guard increments.
        assert_eq!(
            atlas.oversize_skips(),
            1,
            "the path must be rejected BEFORE rasterizing; without the early guard \
             this call still returns None, but only after building and discarding a \
             229 MB bitmap — every frame, forever"
        );
    }

    /// The guard rejects only what genuinely cannot fit: a path right at the limit
    /// still rasterizes, so the bail-out cannot quietly swallow legitimate art.
    #[test]
    fn a_path_that_still_fits_the_atlas_is_rasterized() {
        let mut atlas = PathAtlas::new(256, 256);
        let side = atlas.max_size as f32; // exactly at the cap

        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(side, 0.0)));
        path.push(PathCommand::LineTo(Point::new(side, side)));
        path.push(PathCommand::LineTo(Point::new(0.0, side)));
        path.push(PathCommand::Close);

        let region = atlas.lookup_or_rasterize(
            &path,
            &StrokeStyle::solid(0.0),
            FillRule::Winding,
            [0.0, 0.0, side, side],
            1.0,
            1.0,
            false,
        );
        assert!(
            region.is_some(),
            "a path exactly at max_size ({side}) must still be rasterized — the guard \
             is for paths that can NEVER fit, not for merely large ones"
        );
        assert_eq!(
            atlas.oversize_skips(),
            0,
            "the guard must not fire on a path that fits"
        );
    }

    #[test]
    fn rasterize_simple_rect_path() {
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 10.0)));
        path.push(PathCommand::LineTo(Point::new(0.0, 10.0)));
        path.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let pixels = rasterize_path(
            &path,
            &style,
            FillRule::Winding,
            [0.0, 0.0],
            10,
            10,
            1.0,
            1.0,
        );
        assert!(pixels.is_some());
        let px = pixels.unwrap();
        assert_eq!(px.len(), 10 * 10 * 4);
        // Center pixel should be opaque white (a pure coverage mask —
        // color is no longer baked into the bitmap, see C3).
        let center = (5 * 10 + 5) * 4;
        assert!(px[center] > 200); // R
        assert!(px[center + 1] > 200); // G
        assert!(px[center + 2] > 200); // B
        assert!(px[center + 3] > 200); // A (coverage)
    }

    #[test]
    fn rasterize_stroke_path() {
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(1.0, 5.0)));
        path.push(PathCommand::LineTo(Point::new(9.0, 5.0)));

        let style = StrokeStyle::solid(2.0);
        let pixels = rasterize_path(
            &path,
            &style,
            FillRule::Winding,
            [0.0, 0.0],
            10,
            10,
            1.0,
            1.0,
        );
        assert!(pixels.is_some());
    }

    #[test]
    fn cache_key_distinguishes_line_join() {
        // Two strokes identical except for line join must NOT share a
        // cache entry — otherwise the atlas serves the first's pixels
        // for the second (the bug: line_join was honored in the
        // rasterizer but absent from the key).
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 10.0)));

        let miter = StrokeStyle {
            line_join: LineJoin::Miter,
            ..StrokeStyle::solid(2.0)
        };
        let round = StrokeStyle {
            line_join: LineJoin::Round,
            ..StrokeStyle::solid(2.0)
        };
        assert_ne!(
            PathCacheKey::new(&path, &miter, FillRule::Winding, [0.0, 0.0], 12, 12, 1.0),
            PathCacheKey::new(&path, &round, FillRule::Winding, [0.0, 0.0], 12, 12, 1.0),
            "miter and round joins must hash to different cache keys"
        );
    }

    #[test]
    fn cache_key_distinguishes_fill_rule() {
        // Winding vs even-odd produce different pixels for the same path, so
        // they must not share an atlas entry.
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 10.0)));
        path.push(PathCommand::Close);
        let style = StrokeStyle::solid(0.0);
        assert_ne!(
            PathCacheKey::new(&path, &style, FillRule::Winding, [0.0, 0.0], 12, 12, 1.0),
            PathCacheKey::new(&path, &style, FillRule::EvenOdd, [0.0, 0.0], 12, 12, 1.0),
            "winding and even-odd fills must hash to different cache keys"
        );
    }

    /// A hairline icon stroke is the case the snap exists for.
    ///
    /// `Rect::expand` leaves a 16 dp ring's stroke-expanded bounds at
    /// `x = 1.5` (measured: the app's "no status" glyph is exactly this),
    /// so at scale factor 1 the quad used to be emitted at a half pixel and
    /// resampled through a linear sampler. The snap must round that outward
    /// to whole pixels AND size the bitmap to match, because a quad that is
    /// integral but a different size from its region is resampled just the
    /// same.
    #[test]
    fn a_snapped_path_draws_one_texel_per_device_pixel() {
        let mut atlas = PathAtlas::new(256, 256);
        atlas.begin_frame();

        let path = Path::circle(Point::new(8.0, 8.0), 5.5);
        let style = StrokeStyle::solid(1.0);
        let bounds = path.bounds().expand(style.width).to_array();
        assert_eq!(
            [bounds[0], bounds[1]],
            [1.5, 1.5],
            "the geometry this guards against: a half-pixel bounds origin"
        );

        for sf in [1.0_f32, 1.2, 2.0] {
            let p = atlas
                .lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, sf, 1.0, true)
                .expect("ring rasterizes");
            let [x, y, w, h] = p.device_rect;
            assert_eq!(
                [x, y, w, h],
                [x.floor(), y.floor(), w.floor(), h.floor()],
                "sf {sf}: a snapped quad must land on whole device pixels"
            );
            assert_eq!(
                (w as u32, h as u32),
                (p.region.w, p.region.h),
                "sf {sf}: the quad must be exactly as many pixels as the region \
                 has texels, or the mask is resampled even on the integer grid"
            );
            assert!(
                x <= bounds[0] * sf && x + w >= (bounds[0] + bounds[2]) * sf,
                "sf {sf}: snapping must grow the rect outward, never clip the path"
            );
        }
    }

    /// The other half of the contract: under a transform the caller passes
    /// `snap: false`, and the placement must be exactly what it always was.
    /// Snapping there would be wrong twice over — the mask is being resampled
    /// by the transform anyway, and rounding a translating path's origin
    /// makes it step between pixels instead of gliding.
    #[test]
    fn an_unsnapped_path_keeps_the_raw_rect() {
        let mut atlas = PathAtlas::new(256, 256);
        atlas.begin_frame();

        let path = Path::circle(Point::new(8.0, 8.0), 5.5);
        let style = StrokeStyle::solid(1.0);
        let bounds = path.bounds().expand(style.width).to_array();

        let p = atlas
            .lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, 1.0, 1.0, false)
            .expect("ring rasterizes");
        assert_eq!(p.device_rect, [1.5, 1.5, 13.0, 13.0]);
        assert_eq!((p.region.w, p.region.h), (13, 13));
    }

    /// The same path, snapped and unsnapped, must not share one bitmap.
    ///
    /// Both rasterize at 13×13 here, and the path's commands are identical
    /// (they are absolute, so position alone never separates them), so
    /// without the raster origin in the key the second lookup would be
    /// served the first's phase.
    #[test]
    fn cache_key_distinguishes_the_snapped_phase() {
        let path = Path::circle(Point::new(8.0, 8.0), 5.5);
        let style = StrokeStyle::solid(1.0);
        assert_ne!(
            PathCacheKey::new(&path, &style, FillRule::Winding, [1.0, 1.0], 13, 13, 1.0),
            PathCacheKey::new(&path, &style, FillRule::Winding, [1.5, 1.5], 13, 13, 1.0),
            "a snapped and an unsnapped raster of one path must key apart"
        );
    }

    /// Two entries must never share an edge.
    ///
    /// The atlas sampler is bilinear and each quad's UVs run to its region's
    /// outer edge, so an edge fragment of a quad that is not pixel-exact on
    /// its region reads one texel past it. Packed edge to edge, that texel
    /// belonged to a different icon.
    #[test]
    fn atlas_entries_never_touch() {
        let mut atlas = PathAtlas::new(256, 256);
        atlas.begin_frame();

        let style = StrokeStyle::solid(0.0);
        let mut placed: Vec<AtlasRegion> = Vec::new();
        for i in 0..6 {
            let mut path = Path::new();
            let side = 10.0 + i as f32;
            path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
            path.push(PathCommand::LineTo(Point::new(side, 0.0)));
            path.push(PathCommand::LineTo(Point::new(side, side)));
            path.push(PathCommand::Close);
            let p = atlas
                .lookup_or_rasterize(
                    &path,
                    &style,
                    FillRule::Winding,
                    [0.0, 0.0, side, side],
                    1.0,
                    1.0,
                    true,
                )
                .expect("rasterizes");
            placed.push(p.region);
        }

        for (i, a) in placed.iter().enumerate() {
            for (j, b) in placed.iter().enumerate() {
                if i >= j {
                    continue;
                }
                // Grow each region by the gutter and require they still
                // don't overlap: that is exactly "at least one transparent
                // texel apart on every side".
                let overlaps = a.x < b.x + b.w + ENTRY_GUTTER_PX
                    && b.x < a.x + a.w + ENTRY_GUTTER_PX
                    && a.y < b.y + b.h + ENTRY_GUTTER_PX
                    && b.y < a.y + a.h + ENTRY_GUTTER_PX;
                assert!(
                    !overlaps,
                    "entries {i} {a:?} and {j} {b:?} are packed closer than the gutter"
                );
            }
        }
    }

    #[test]
    fn atlas_cache_hit() {
        let mut atlas = PathAtlas::new(256, 256);
        atlas.begin_frame();

        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 10.0)));
        path.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let bounds = [0.0, 0.0, 10.0, 10.0];

        let r1 = atlas
            .lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, 1.0, 1.0, false)
            .unwrap();
        let r2 = atlas
            .lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, 1.0, 1.0, false)
            .unwrap();

        // Same region (cache hit)
        assert_eq!(r1.region.x, r2.region.x);
        assert_eq!(r1.region.y, r2.region.y);
    }

    #[test]
    fn cache_hit_is_independent_of_color() {
        // C3: color is no longer part of the rasterization or the cache
        // key — two lookups with identical geometry/stroke/size but
        // DIFFERENT colors (as the caller would pass via the paint,
        // before this refactor) must now hit the SAME atlas entry, since
        // `lookup_or_rasterize` no longer takes a color at all. This is
        // what lets a solid fill and a gradient fill of the same path
        // share one atlas entry.
        let mut atlas = PathAtlas::new(256, 256);
        atlas.begin_frame();

        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(10.0, 10.0)));
        path.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let bounds = [0.0, 0.0, 10.0, 10.0];

        // Simulate two draw calls that would previously have carried
        // different colors — the API no longer distinguishes them, so
        // both lookups are for the exact same cache key.
        let r1 = atlas
            .lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, 1.0, 1.0, false)
            .expect("first lookup rasterizes and caches");
        let r2 = atlas
            .lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, 1.0, 1.0, false)
            .expect("second lookup hits the same cache entry");

        assert_eq!(r1.region.x, r2.region.x, "cache hit: same region x");
        assert_eq!(r1.region.y, r2.region.y, "cache hit: same region y");
        assert_eq!(r1.region.w, r2.region.w);
        assert_eq!(r1.region.h, r2.region.h);
        assert_eq!(atlas.cache.len(), 1, "only one atlas entry for both calls");
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
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(8.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(8.0, 8.0)));
        path.push(PathCommand::Close);
        let style = StrokeStyle::solid(0.0);
        let bounds = [0.0, 0.0, 8.0, 8.0];

        atlas.begin_frame(); // frame 1
        atlas.lookup_or_rasterize(&path, &style, FillRule::Winding, bounds, 1.0, 1.0, false);

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
        // the first — `path_placements[0]` ended up pointing at pixels
        // that now belonged to path #2. LineChart and PieChart hit this
        // routinely because their paths cover most of the plot area.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.begin_frame();

        let mut p1 = Path::new();
        p1.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p1.push(PathCommand::LineTo(Point::new(40.0, 0.0)));
        p1.push(PathCommand::LineTo(Point::new(40.0, 40.0)));
        p1.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.push(PathCommand::LineTo(Point::new(50.0, 0.0)));
        p2.push(PathCommand::LineTo(Point::new(50.0, 50.0)));
        p2.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(
                &p1,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 40.0, 40.0],
                1.0,
                1.0,
                false,
            )
            .expect("p1 fits");

        // p2 doesn't fit in the remaining space → the atlas grows rather than
        // evicting, so p1 (current-frame) survives at the same coordinates.
        let _r2 = atlas.lookup_or_rasterize(
            &p2,
            &style,
            FillRule::Winding,
            [0.0, 0.0, 50.0, 50.0],
            1.0,
            1.0,
            false,
        );

        // Looking up p1 again must still hit cache (with possibly a new
        // region, but stable across the lookup).
        let r1b = atlas
            .lookup_or_rasterize(
                &p1,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 40.0, 40.0],
                1.0,
                1.0,
                false,
            )
            .expect("p1 still cached after eviction");
        // The repacked region may have moved, but lookup_or_rasterize
        // must return a non-None region for p1 — i.e. it wasn't lost.
        let _ = (r1, r1b);
        assert!(atlas.cache.contains_key(&PathCacheKey::new(
            &p1,
            &style,
            FillRule::Winding,
            [0.0, 0.0],
            40,
            40,
            1.0,
        )));
    }

    #[test]
    fn evict_never_moves_live_entry_when_full() {
        // Core invariant for the stale-UV fix: once a region is handed out
        // this frame it is frozen. If a later path can't fit and the atlas is
        // already at max size, the new path is skipped (returns None) — the
        // live entry must NOT be repacked, or `path_placements[..]` would sample
        // the wrong pixels later in the same frame.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.max_size = 64; // forbid growth so eviction is the only path
        atlas.begin_frame();

        let mut p1 = Path::new();
        p1.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p1.push(PathCommand::LineTo(Point::new(60.0, 0.0)));
        p1.push(PathCommand::LineTo(Point::new(60.0, 60.0)));
        p1.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.push(PathCommand::LineTo(Point::new(62.0, 0.0)));
        p2.push(PathCommand::LineTo(Point::new(62.0, 62.0)));
        p2.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(
                &p1,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 60.0, 60.0],
                1.0,
                1.0,
                false,
            )
            .expect("p1 fits");

        // p2 can't fit, can't grow → must be skipped, not placed by moving p1.
        let r2 = atlas.lookup_or_rasterize(
            &p2,
            &style,
            FillRule::Winding,
            [0.0, 0.0, 62.0, 62.0],
            1.0,
            1.0,
            false,
        );
        assert!(
            r2.is_none(),
            "an unfittable path is skipped, never placed by evicting a live entry"
        );

        // p1's region is byte-for-byte unchanged.
        let r1b = atlas
            .lookup_or_rasterize(
                &p1,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 60.0, 60.0],
                1.0,
                1.0,
                false,
            )
            .expect("p1 still cached");
        assert_eq!(r1.region.x, r1b.region.x, "live entry must not move");
        assert_eq!(r1.region.y, r1b.region.y, "live entry must not move");
    }

    #[test]
    fn begin_frame_compacts_stale_entries() {
        // `begin_frame` is the safe point to repack: nothing is handed out
        // for the new frame yet. A near-full atlas with entries not used on
        // the last completed frame compacts them away.
        let mut atlas = PathAtlas::new(64, 64);
        atlas.begin_frame(); // frame 1

        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(8.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(8.0, 8.0)));
        path.push(PathCommand::Close);
        let style = StrokeStyle::solid(0.0);
        atlas
            .lookup_or_rasterize(
                &path,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 8.0, 8.0],
                1.0,
                1.0,
                false,
            )
            .expect("entry fits");
        assert_eq!(atlas.cache.len(), 1);

        atlas.begin_frame(); // frame 2 — keep_from = 1, entry (used f1) kept
        assert_eq!(
            atlas.cache.len(),
            1,
            "entry from the last completed frame is kept"
        );

        atlas.begin_frame(); // frame 3 — keep_from = 2, entry (used f1) is stale
        assert!(
            atlas.cache.is_empty(),
            "stale entry compacted away on begin_frame"
        );
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
        p1.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p1.push(PathCommand::LineTo(Point::new(50.0, 0.0)));
        p1.push(PathCommand::LineTo(Point::new(50.0, 50.0)));
        p1.push(PathCommand::Close);

        let mut p2 = Path::new();
        p2.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        p2.push(PathCommand::LineTo(Point::new(60.0, 0.0)));
        p2.push(PathCommand::LineTo(Point::new(60.0, 60.0)));
        p2.push(PathCommand::Close);

        let style = StrokeStyle::solid(0.0);
        let r1 = atlas
            .lookup_or_rasterize(
                &p1,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 50.0, 50.0],
                1.0,
                1.0,
                false,
            )
            .expect("p1 fits");

        // p2 doesn't fit alongside p1 in 64×64 → atlas should grow,
        // not evict. After growth, p1's region must still be at the
        // same coordinates we got back the first time.
        let _r2 = atlas
            .lookup_or_rasterize(
                &p2,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 60.0, 60.0],
                1.0,
                1.0,
                false,
            )
            .expect("p2 fits after grow");

        let r1_after = atlas
            .lookup_or_rasterize(
                &p1,
                &style,
                FillRule::Winding,
                [0.0, 0.0, 50.0, 50.0],
                1.0,
                1.0,
                false,
            )
            .expect("p1 still cached");
        assert_eq!(
            r1.region.x, r1_after.region.x,
            "p1 must not move when atlas grows"
        );
        assert_eq!(
            r1.region.y, r1_after.region.y,
            "p1 must not move when atlas grows"
        );
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
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(40.0, 0.0)));
        let bounds = [0.0, 0.0, 40.0, 4.0];

        let cosmetic = StrokeStyle::hairline(2.0);
        let r1 = atlas
            .lookup_or_rasterize(&path, &cosmetic, FillRule::Winding, bounds, 1.0, 1.0, false)
            .unwrap();
        let r2 = atlas
            .lookup_or_rasterize(&path, &cosmetic, FillRule::Winding, bounds, 1.0, 2.0, false)
            .unwrap();
        assert_eq!(r1.region.w, 40, "cosmetic body at zoom 1: 40·sf1·zoom1");
        assert_eq!(
            r2.region.w, 80,
            "cosmetic body at zoom 2: 40·sf1·zoom2 (zoom-aware)"
        );

        let logical = StrokeStyle::solid(2.0);
        let l1 = atlas
            .lookup_or_rasterize(&path, &logical, FillRule::Winding, bounds, 1.0, 1.0, false)
            .unwrap();
        let l2 = atlas
            .lookup_or_rasterize(&path, &logical, FillRule::Winding, bounds, 1.0, 4.0, false)
            .unwrap();
        assert_eq!(l1.region.w, l2.region.w, "logical raster size ignores zoom");
        assert_eq!(
            (l1.region.x, l1.region.y),
            (l2.region.x, l2.region.y),
            "logical hits the same cache entry"
        );

        // Same width/dims but different stroke space must not collide.
        let k_cos = PathCacheKey::new(&path, &cosmetic, FillRule::Winding, [0.0, 0.0], 40, 4, 1.0);
        let k_log = PathCacheKey::new(&path, &logical, FillRule::Winding, [0.0, 0.0], 40, 4, 1.0);
        assert_ne!(
            k_cos, k_log,
            "cache key must distinguish cosmetic vs logical"
        );
    }

    // ── Dash lengths are measured along the path, so they scale with it ──

    /// A 20-logical-px horizontal line, dashed 4 on / 4 off, rasterized at
    /// `geom_scale`. Returns how many separate ink runs the middle row has.
    fn dashed_line_runs(geom_scale: f32) -> usize {
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 4.0)));
        path.push(PathCommand::LineTo(Point::new(20.0, 4.0)));
        let style = StrokeStyle::dashed(2.0, 4.0, 4.0);

        let w = (20.0 * geom_scale).ceil() as u32;
        let h = (8.0 * geom_scale).ceil() as u32;
        let px = rasterize_path(
            &path,
            &style,
            FillRule::Winding,
            [0.0, 0.0],
            w,
            h,
            geom_scale,
            geom_scale,
        )
        .expect("rasterizes");

        let row = (4.0 * geom_scale) as u32;
        let mut runs = 0usize;
        let mut inked = false;
        for x in 0..w {
            let now = px[((row * w + x) * 4 + 3) as usize] > 100;
            if now && !inked {
                runs += 1;
            }
            inked = now;
        }
        runs
    }

    /// The defect: the dash pattern reached tiny-skia unscaled while the
    /// geometry was baked at `geom_scale`, so the dashes came out
    /// `1 / geom_scale` too short. A `dashed(2, 4, 4)` line showed 3 dashes
    /// at scale 1 and 5 at scale 2 — i.e. every dashed stroke, chart
    /// gridlines included, was drawn with half-length dashes on a 2× HiDPI
    /// display, and a cosmetic dashed stroke re-cut its pattern at every
    /// zoom step.
    #[test]
    fn dash_count_is_invariant_to_the_geometry_scale() {
        let baseline = dashed_line_runs(1.0);
        assert!(baseline > 1, "the probe line must actually dash");
        for scale in [2.0f32, 3.0, 4.0] {
            assert_eq!(
                dashed_line_runs(scale),
                baseline,
                "a dash is a length along the path: scaling the geometry by \
                 {scale} must scale the dashes with it, not cut more of them"
            );
        }
    }

    /// The dash pattern is a rasterization input that `w` / `h` do not
    /// always recover — they are the bounds *ceiled* into texels, so a
    /// sub-pixel path aliases several scales onto one bitmap size, and a
    /// cosmetic stroke's `geom_scale` slides continuously with the view
    /// zoom. Without `geom_scale` in the key, the second zoom step would
    /// be served the first's dashes.
    #[test]
    fn cache_key_distinguishes_the_geometry_scale() {
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 0.0)));
        path.push(PathCommand::LineTo(Point::new(0.4, 0.0)));
        let style = StrokeStyle::dashed(1.0, 4.0, 4.0);
        // Same ceiled bitmap size (1x1) and same origin at both scales.
        assert_ne!(
            PathCacheKey::new(&path, &style, FillRule::Winding, [0.0, 0.0], 1, 1, 1.0),
            PathCacheKey::new(&path, &style, FillRule::Winding, [0.0, 0.0], 1, 1, 2.0),
            "two geometry scales that ceil to the same bitmap must not share \
             an atlas entry — their dashes are cut differently"
        );
    }

    /// A dashed stroke really does leave gaps — the rasterizer is what the
    /// canvas routing exists to reach, so pin that it does the job.
    #[test]
    fn a_dashed_stroke_leaves_gaps_where_a_solid_one_does_not() {
        let mut path = Path::new();
        path.push(PathCommand::MoveTo(Point::new(0.0, 4.0)));
        path.push(PathCommand::LineTo(Point::new(20.0, 4.0)));

        let ink = |style: &StrokeStyle| -> usize {
            let px = rasterize_path(&path, style, FillRule::Winding, [0.0, 0.0], 20, 8, 1.0, 1.0)
                .expect("rasterizes");
            (0..20)
                .filter(|x| px[((4 * 20 + x) * 4 + 3) as usize] > 100)
                .count()
        };

        let solid = ink(&StrokeStyle::solid(2.0));
        let dashed = ink(&StrokeStyle::dashed(2.0, 4.0, 4.0));
        assert!(
            solid >= 19,
            "solid stroke inks the whole line (got {solid})"
        );
        assert!(
            dashed < solid,
            "dashed stroke must leave gaps (solid={solid}, dashed={dashed})"
        );
    }

    // ── The rasterizer opens a subpath where the flattener opens one ──
    //
    // `Path::flatten` is the oracle for both halves of the parity the
    // `ItemShape` work exists to guarantee: the scene tier hit-tests the
    // polyline it produces, so ink the rasterizer lays down outside that
    // polyline is ink no click can reach.

    /// The arc that exposes the defect: a 40x40 disc centred at (200, 0), so
    /// its own first point (220, 0) is 220 units from the origin and 220 from
    /// any bitmap corner a spoke could be welded to.
    fn probe_arc() -> teksilo_canvas::geometry::Rect {
        teksilo_canvas::geometry::Rect::new(180.0, -20.0, 40.0, 40.0)
    }

    /// Paths covering every way an arc can meet a subpath, plus non-arc
    /// shapes that must not regress.
    fn subpath_battery() -> Vec<(&'static str, Path)> {
        let mut bare_circle_arc = Path::new();
        bare_circle_arc.arc_to(probe_arc(), 0.0, 360.0);

        let mut bare_quarter_arc = Path::new();
        bare_quarter_arc.arc_to(probe_arc(), 0.0, 90.0);

        let mut arc_after_close = Path::new();
        arc_after_close
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(10.0, 10.0));
        arc_after_close.close();
        arc_after_close.arc_to(probe_arc(), 0.0, 360.0);

        let mut two_arcs = Path::new();
        two_arcs.arc_to(probe_arc(), 0.0, 180.0);
        two_arcs.close();
        two_arcs.arc_to(
            teksilo_canvas::geometry::Rect::new(0.0, 0.0, 30.0, 30.0),
            90.0,
            180.0,
        );
        two_arcs.close();

        // A `MoveTo` that lands away from the arc's start: `flatten` keeps a
        // straight connector here, so the rasterizer must keep one too —
        // this is the case the fix must NOT turn into a second subpath.
        let mut arc_from_elsewhere = Path::new();
        arc_from_elsewhere.move_to(Point::new(0.0, 0.0));
        arc_from_elsewhere.arc_to(probe_arc(), 0.0, 270.0);

        let mut arc_after_cubic = Path::new();
        arc_after_cubic.move_to(Point::new(0.0, 0.0)).cubic_to(
            Point::new(20.0, 40.0),
            Point::new(60.0, -40.0),
            Point::new(90.0, 5.0),
        );
        arc_after_cubic.arc_to(probe_arc(), 90.0, 180.0);

        let mut curves_and_lines = Path::new();
        curves_and_lines
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(40.0, 0.0))
            .quad_to(Point::new(60.0, 20.0), Point::new(40.0, 40.0))
            .line_to(Point::new(0.0, 40.0));
        curves_and_lines.close();
        curves_and_lines
            .move_to(Point::new(80.0, 10.0))
            .line_to(Point::new(120.0, 10.0))
            .line_to(Point::new(120.0, 30.0));

        // A path that opens on a bare segment, with a negative coordinate so
        // the atlas origin is non-zero. tiny-skia injects its own `MoveTo` at
        // *bitmap* (0, 0) here; `flatten` opens at *path-space* (0, 0). Those
        // agree only while the origin is zero, which is exactly what a
        // negative coordinate stops being true.
        let mut bare_line_negative = Path::new();
        bare_line_negative
            .line_to(Point::new(-30.0, -20.0))
            .line_to(Point::new(40.0, 25.0));

        let mut bare_cubic_negative = Path::new();
        bare_cubic_negative.cubic_to(
            Point::new(-20.0, 40.0),
            Point::new(60.0, -40.0),
            Point::new(-90.0, 5.0),
        );

        let mut bare_quad_negative = Path::new();
        bare_quad_negative.quad_to(Point::new(-15.0, 30.0), Point::new(35.0, -10.0));

        vec![
            ("bare 360 arc", bare_circle_arc),
            ("bare 90 arc", bare_quarter_arc),
            ("bare line, negative", bare_line_negative),
            ("bare cubic, negative", bare_cubic_negative),
            ("bare quad, negative", bare_quad_negative),
            ("arc after close", arc_after_close),
            ("two arcs split by close", two_arcs),
            ("arc reached from elsewhere", arc_from_elsewhere),
            ("arc after a cubic", arc_after_cubic),
            ("curves and lines", curves_and_lines),
            ("circle", Path::circle(Point::new(50.0, 50.0), 25.0)),
            (
                "rounded rect",
                Path::rounded_rect(
                    teksilo_canvas::geometry::Rect::new(0.0, 0.0, 100.0, 60.0),
                    teksilo_tokens::CornerRadius {
                        top_left: 12.0,
                        top_right: 4.0,
                        bottom_left: 0.0,
                        bottom_right: 20.0,
                    },
                ),
            ),
        ]
    }

    /// Where the tiny-skia path opens each of its subpaths.
    fn sk_subpath_starts(p: &tiny_skia::Path) -> Vec<(f32, f32)> {
        p.segments()
            .filter_map(|seg| match seg {
                tiny_skia::PathSegment::MoveTo(pt) => Some((pt.x, pt.y)),
                _ => None,
            })
            .collect()
    }

    /// Where the flattener opens each of its subpaths.
    ///
    /// Single-vertex subpaths are dropped: a lone `MoveTo` with no segment
    /// after it paints nothing and `contains_point` cannot see it either, but
    /// tiny-skia coalesces consecutive `MoveTo` verbs into one while `flatten`
    /// keeps a one-point subpath for each. That difference is inert on both
    /// sides, and is not the parity this test is about.
    fn flatten_subpath_starts(p: &Path) -> Vec<(f32, f32)> {
        p.flatten(0.01)
            .iter()
            .filter(|sp| sp.points.len() > 1)
            .map(|sp| (sp.points[0].x, sp.points[0].y))
            .collect()
    }

    /// The defect, stated against the oracle: the arc emitter used to emit an
    /// unconditional `line_to` for every arc segment, so tiny-skia injected a
    /// `MoveTo` of its own — `(0, 0)` **in bitmap space** for a path opening
    /// with a bare arc, or the *previous* subpath's start after a `Close` —
    /// and welded a spoke from it to the arc. `Path::flatten`, which the scene
    /// tier hit-tests, opens at the arc's own first point instead. That gap is
    /// painted ink no click can reach.
    ///
    /// Comparing against `flatten` rather than against a recorded verb list
    /// is what keeps this from going stale: change the arc expansion and both
    /// sides move together, or the test reddens.
    #[test]
    fn the_rasterizer_starts_subpaths_where_the_flattener_does() {
        for (name, path) in subpath_battery() {
            let sk = build_sk_path(&path, 1.0, [0.0, 0.0]).expect("path builds");
            let got = sk_subpath_starts(&sk);
            let want = flatten_subpath_starts(&path);
            assert_eq!(
                got, want,
                "{name}: the rasterizer must open its subpaths exactly where \
                 Path::flatten opens its own — anywhere else is ink the scene \
                 tier's hit-test cannot see"
            );
        }
    }

    /// The same claim in device space: the scale factor must not move a
    /// subpath start relative to the geometry. The injected `MoveTo` went to
    /// the **bitmap's** `(0, 0)`, so the phantom vertex slid with the scale
    /// while the real geometry scaled around it.
    #[test]
    fn subpath_starts_track_the_geometry_through_scale_and_origin() {
        for (name, path) in subpath_battery() {
            let want = flatten_subpath_starts(&path);
            for (scale, origin) in [
                (1.0_f32, [0.0_f32, 0.0]),
                (2.0, [7.0, -3.0]),
                (0.5, [1.0, 1.0]),
            ] {
                let sk = build_sk_path(&path, scale, origin).expect("path builds");
                let got = sk_subpath_starts(&sk);
                let mapped: Vec<(f32, f32)> = want
                    .iter()
                    .map(|(x, y)| (x * scale - origin[0], y * scale - origin[1]))
                    .collect();
                assert_eq!(
                    got, mapped,
                    "{name} at scale {scale} origin {origin:?}: a subpath start \
                     is a point of the geometry, so it must map through the same \
                     affine transform every other point does"
                );
            }
        }
    }

    /// Fill `path` at 1:1 and return the inked bounding box in the path's own
    /// units as `(left, top, right, bottom)`, or `None` when nothing inked.
    ///
    /// The bitmap is the path's bounds plus a margin — which is also where a
    /// welded spoke shows up, since the injected `MoveTo` lands on the
    /// bitmap's own corner.
    fn painted_fill_bounds(path: &Path) -> Option<(f32, f32, f32, f32)> {
        const MARGIN: f32 = 2.0;
        let b = path.bounds();
        let (ox, oy) = (b.x - MARGIN, b.y - MARGIN);
        let w = (b.width + 2.0 * MARGIN).ceil() as u32;
        let h = (b.height + 2.0 * MARGIN).ceil() as u32;
        let px = rasterize_path(
            path,
            &StrokeStyle::solid(0.0),
            FillRule::Winding,
            [ox, oy],
            w,
            h,
            1.0,
            1.0,
        )
        .expect("rasterizes");

        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                if px[((y * w + x) * 4 + 3) as usize] > 0 {
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        any.then_some((
            min_x as f32 + ox,
            min_y as f32 + oy,
            max_x as f32 + ox + 1.0,
            max_y as f32 + oy + 1.0,
        ))
    }

    /// End to end, in pixels: what the rasterizer paints must sit inside what
    /// the shape reports. A bare 90° arc was the worst case — `flatten` gives
    /// a quarter-arc bounded by `x ∈ [200, 220]`, while the fill came out as a
    /// solid triangle running from the bitmap's top-left corner all the way to
    /// the arc, 2728 inked pixels against the ~86 the shape covers. Every one
    /// of those pixels was unclickable.
    #[test]
    fn a_fill_paints_only_where_the_shape_says_it_is() {
        // AA writes partial coverage into the pixel a boundary crosses, so a
        // one-pixel ring around the exact outline is expected; a spoke is
        // orders of magnitude more than that.
        const SLACK: f32 = 1.5;
        for (name, path) in subpath_battery() {
            let want = path.exact_bounds(0.01);
            let (l, t, r, b) = painted_fill_bounds(&path)
                .unwrap_or_else(|| panic!("{name}: the probe must actually ink something"));
            assert!(
                l >= want.x - SLACK
                    && t >= want.y - SLACK
                    && r <= want.right() + SLACK
                    && b <= want.bottom() + SLACK,
                "{name}: painted ({l}, {t})-({r}, {b}) escapes the shape's own \
                 extent ({}, {})-({}, {}) — that ink is unreachable by a click",
                want.x,
                want.y,
                want.right(),
                want.bottom()
            );
        }
    }

    /// The same defect where it is loudest. A fill traverses the spoke and
    /// comes back along the closing edge, so a 360° arc's spoke cancels to a
    /// sub-pixel needle; a *stroke* draws it outright — a 2 dp bar running the
    /// full 220 units from the phantom vertex to the disc.
    #[test]
    fn a_stroked_bare_arc_draws_no_spoke_to_the_bitmap_corner() {
        let mut path = Path::new();
        path.arc_to(probe_arc(), 0.0, 360.0);

        // A bitmap wide enough to hold the whole spoke: x from 0 to 240.
        let (w, h) = (242u32, 44u32);
        let px = rasterize_path(
            &path,
            &StrokeStyle::solid(2.0),
            FillRule::Winding,
            [-1.0, -22.0],
            w,
            h,
            1.0,
            1.0,
        )
        .expect("rasterizes");

        // The disc's own left edge is x = 180, i.e. bitmap column 181; the
        // 2 dp stroke reaches one column further left. Nothing may ink before
        // that.
        let leftmost = (0..w)
            .find(|&x| (0..h).any(|y| px[((y * w + x) * 4 + 3) as usize] > 0))
            .expect("the arc must ink");
        assert!(
            leftmost >= 179,
            "a stroked bare arc inked from column {leftmost}: the subpath was \
             opened at the bitmap's corner and a spoke stroked from there to \
             the arc, 220 units of ink the shape does not have"
        );
    }
}
