// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`LiveImageMirror`]: the renderer's live pass with textures in CPU memory,
//! for headless tests.

use super::internal::LiveImageRead;
use super::mips::{mip_footprint, mip_level_size, mip_levels};
use super::pass::{
    LivePass, LivePassMode, LiveTextureBackend, LiveTextureStats, QuadDecision, TextureOutOfMemory,
};
use super::{LiveImageId, LiveImageSource, PixelRect, ScalingFilter};
use crate::RenderFrame;

/// A texture in CPU memory: `width × height`, 4 bytes per pixel in the
/// source's byte order, and its mip levels above level 0 when it has them.
pub(crate) struct CpuTexture {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    /// Levels 1 and up, each with its size.
    levels: Vec<(u32, u32, Vec<u8>)>,
}

impl CpuTexture {
    /// Level `k`: 0 is the texture itself.
    fn level(&self, k: u32) -> Option<(u32, u32, &[u8])> {
        match k {
            0 => Some((self.width, self.height, &self.pixels)),
            k => self
                .levels
                .get(k as usize - 1)
                .map(|(w, h, px)| (*w, *h, px.as_slice())),
        }
    }
}

/// The mirror's backend: textures are `Vec<u8>`s, writes are row copies.
pub(crate) struct CpuBackend {
    max_dimension: u32,
    fail_next_create: bool,
    lost: bool,
    /// The bands written since the last report.
    writes: Vec<PixelRect>,
    /// The mip rebuilds since the last report: `None` for a whole chain.
    mip_rebuilds: Vec<Option<PixelRect>>,
}

impl LiveTextureBackend for CpuBackend {
    type Texture = CpuTexture;

    fn max_dimension(&self) -> u32 {
        self.max_dimension
    }

    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        mipped: bool,
        _label: &str,
    ) -> Result<CpuTexture, TextureOutOfMemory> {
        if std::mem::take(&mut self.fail_next_create) {
            return Err(TextureOutOfMemory);
        }
        let levels = if mipped {
            (1..mip_levels(width, height))
                .map(|k| {
                    let (w, h) = mip_level_size(width, height, k);
                    (w, h, vec![0; w as usize * h as usize * 4])
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(CpuTexture {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
            levels,
        })
    }

    fn staged_row_bytes(&self, width: u32) -> u64 {
        u64::from(width) * 4
    }

    fn write(&mut self, texture: &mut CpuTexture, read: &LiveImageRead<'_>, band: PixelRect) {
        let stride = read.stride();
        let row = band.width as usize * 4;
        let pitch = texture.width as usize * 4;
        let pixels = read.pixels();
        for y in band.y..band.y + band.height {
            let from = y as usize * stride + band.x as usize * 4;
            let to = y as usize * pitch + band.x as usize * 4;
            texture.pixels[to..to + row].copy_from_slice(&pixels[from..from + row]);
        }
        self.writes.push(band);
    }

    fn prepare_filter(&mut self, _texture: &mut CpuTexture, _filter: ScalingFilter) {}

    fn texture_bytes(&self, texture: &CpuTexture) -> u64 {
        u64::from(texture.width) * u64::from(texture.height) * 4
            + texture
                .levels
                .iter()
                .map(|(w, h, _)| u64::from(*w) * u64::from(*h) * 4)
                .sum::<u64>()
    }

    /// Level from level, as the GPU's passes do: each texel of the region's
    /// footprint (or every texel) from the 2×2 box below it.
    fn rebuild_mips(&mut self, texture: &mut CpuTexture, region: Option<PixelRect>, opaque: bool) {
        for k in 1..=texture.levels.len() as u32 {
            let (below, rest) = texture.levels.split_at_mut(k as usize - 1);
            let (sw, sh, src): (u32, u32, &[u8]) = match below.last() {
                Some((w, h, px)) => (*w, *h, px),
                None => (texture.width, texture.height, &texture.pixels),
            };
            let (w, h, dst) = &mut rest[0];
            let area = match region {
                Some(changed) => mip_footprint(changed, k, (*w, *h)),
                None => PixelRect::full(*w, *h),
            };
            // A level above 0 is always opaque already: only level 0's
            // fourth byte can be something other than alpha.
            let not_alpha = opaque && k == 1;
            for y in area.y..area.y + area.height {
                for x in area.x..area.x + area.width {
                    let texel =
                        crate::resample::downsample_half_texel(src, sw, sh, x, y, not_alpha);
                    let at = (y as usize * *w as usize + x as usize) * 4;
                    dst[at..at + 4].copy_from_slice(&texel);
                }
            }
        }
        self.mip_rebuilds.push(region);
    }

    fn device_lost(&self) -> bool {
        self.lost
    }
}

/// The renderer's live pass with textures in CPU memory: the same engine,
/// so the same planner, flag taking and parking, lock attempts, size
/// re-check, pause rule, staging budget and release, with each texture a
/// `Vec<u8>`. A headless test that renders a tree runs it where a window
/// would run the GPU's.
pub struct LiveImageMirror {
    pass: LivePass<CpuBackend>,
}

/// What one [`LiveImageMirror::consume`] or [`capture`](LiveImageMirror::capture)
/// did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MirrorReport {
    /// Whole-frame uploads, one per source.
    pub full_uploads: u32,
    /// Rect uploads, one per source.
    pub partial_uploads: u32,
    /// The bands written, one per write.
    pub rects: Vec<PixelRect>,
    /// Bytes written.
    pub bytes: u64,
    /// Quads that drew an older picture or only their background.
    pub deferred: u32,
    /// Sources whose lock a producer held.
    pub contended: u32,
    /// Sources whose textures went because no quad drew them.
    pub pruned: u32,
    /// Sources whose texture was kept because every quad of them was paused.
    pub paused: u32,
    /// The mip chains rebuilt, one per source: `None` for a whole chain,
    /// else the rect of level 0 whose footprint each level rebuilt.
    pub mip_rebuilds: Vec<Option<PixelRect>>,
}

impl Default for LiveImageMirror {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveImageMirror {
    pub fn new() -> Self {
        Self {
            pass: LivePass::new(CpuBackend {
                max_dimension: LiveImageSource::MAX_DIMENSION,
                fail_next_create: false,
                lost: false,
                writes: Vec::new(),
                mip_rebuilds: Vec::new(),
            }),
        }
    }

    /// Run the live pass for `frame`, as a presented frame would.
    pub fn consume(&mut self, frame: &RenderFrame) -> MirrorReport {
        self.run(frame, LivePassMode::Present)
    }

    /// Run it as a screenshot would: the latest commit whatever the lock or
    /// a pause, counted as a capture.
    pub fn capture(&mut self, frame: &RenderFrame) -> MirrorReport {
        self.run(frame, LivePassMode::Capture)
    }

    fn run(&mut self, frame: &RenderFrame, mode: LivePassMode) -> MirrorReport {
        self.run_at(frame, mode, std::time::Instant::now())
    }

    fn run_at(
        &mut self,
        frame: &RenderFrame,
        mode: LivePassMode,
        now: std::time::Instant,
    ) -> MirrorReport {
        let before = self.pass.stats();
        self.pass.backend_mut().writes.clear();
        self.pass.backend_mut().mip_rebuilds.clear();
        self.pass.prepare_at(&frame.live_images, mode, now);
        let after = self.pass.stats();
        let counts = self.pass.last_counts();
        MirrorReport {
            full_uploads: (after.uploads_full - before.uploads_full) as u32,
            partial_uploads: (after.uploads_partial - before.uploads_partial) as u32,
            rects: std::mem::take(&mut self.pass.backend_mut().writes),
            bytes: after.bytes_uploaded - before.bytes_uploaded,
            deferred: self.pass.decisions().iter().filter(|d| d.deferred).count() as u32,
            contended: (after.contended - before.contended) as u32,
            pruned: counts.pruned,
            paused: counts.kept,
            mip_rebuilds: std::mem::take(&mut self.pass.backend_mut().mip_rebuilds),
        }
    }

    /// The pixels the window would draw for source `id`: its current
    /// texture, 4 bytes per pixel in the source's byte order.
    pub fn pixels(&self, id: LiveImageId) -> Option<(u32, u32, &[u8])> {
        self.pass
            .texture(id)
            .map(|t| (t.width, t.height, t.pixels.as_slice()))
    }

    /// Level `k` of source `id`'s current texture: 0 is
    /// [`pixels`](Self::pixels), the levels above exist when a frame drew the
    /// source through `ScalingFilter::Trilinear`.
    pub fn mip_level(&self, id: LiveImageId, k: u32) -> Option<(u32, u32, &[u8])> {
        self.pass.texture(id)?.level(k)
    }

    /// Textures held for the sources the last frame drew, staged ones
    /// included; parked ones are in [`stats`](Self::stats).
    pub fn texture_count(&self) -> usize {
        self.pass.stats().textures
    }

    /// What the last frame decided for each of its quads.
    pub fn decisions(&self) -> &[QuadDecision] {
        self.pass.decisions()
    }

    pub fn stats(&self) -> LiveTextureStats {
        self.pass.stats()
    }

    /// A stand-in for the device's largest texture side, to reach the
    /// oversize row with small sources.
    pub fn set_max_dimension(&mut self, max: u32) {
        self.pass.backend_mut().max_dimension = max;
    }

    /// The next texture creation reports that the device has no memory.
    #[doc(hidden)]
    pub fn fail_next_texture(&mut self) {
        self.pass.backend_mut().fail_next_create = true;
    }

    /// Behave as a lost device does: nothing uploaded reaches the screen.
    #[doc(hidden)]
    pub fn set_device_lost(&mut self, lost: bool) {
        self.pass.backend_mut().lost = lost;
    }

    /// The live pass's staging budget, for tests of a fill spread over
    /// several frames.
    #[doc(hidden)]
    pub fn set_staging_budget(&mut self, bytes: u64) {
        self.pass.set_staging_budget(bytes);
    }

    /// The largest staging one write takes, for tests of banded uploads.
    #[doc(hidden)]
    pub fn set_band_bytes(&mut self, bytes: u64) {
        self.pass.set_band_bytes(bytes);
    }

    /// The parked pool's budget; 0 frees every texture a frame does not draw.
    #[doc(hidden)]
    pub fn set_park_budget(&mut self, bytes: u64) {
        self.pass.set_park_budget(bytes);
    }

    /// [`consume`](Self::consume) at a given instant, for tests of the
    /// allocation backoff.
    #[doc(hidden)]
    pub fn consume_at(&mut self, frame: &RenderFrame, now: std::time::Instant) -> MirrorReport {
        self.run_at(frame, LivePassMode::Present, now)
    }
}
