// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The live pass: what one window's renderer does with the live pictures of
//! a frame before it draws them. One engine, generic over where textures
//! live: the wgpu renderer runs it on the GPU,
//! [`LiveImageMirror`](super::testing::LiveImageMirror) on the CPU, so the
//! headless tests exercise the renderer's own decisions.
//!
//! The steps are on [`LivePass`].

use std::collections::HashMap;
use std::sync::Weak;
use std::time::{Duration, Instant};

use super::consumer::RenderRecord;
use super::internal::{LiveImageRead, ReadAttempt, UploadPlan};
use super::source::Shared;
use super::{
    LiveImageConsumer, LiveImageId, LiveImageQuad, LiveImageSource, PixelRect, ScalingFilter,
};
use crate::wake::WakeKind;

/// The largest staging one write may take, 8 MiB: a failed staging
/// allocation loses the device, and this keeps every one far from any limit.
pub const BAND_BYTES: u64 = 8 << 20;

/// The staging a presented frame spends on whole-frame uploads before the
/// rest of a large one waits for the next frame.
pub const STAGING_BUDGET: u64 = 128 << 20;

/// The parked-texture pool: textures up to [`PARK_MAX_TEXTURE`] each, up to
/// this many bytes in all.
pub const PARK_BUDGET: u64 = 16 << 20;

/// The largest texture the pool parks.
pub const PARK_MAX_TEXTURE: u64 = 4 << 20;

/// How long a refused texture creation is not retried, unless the size
/// changes.
const ALLOC_BACKOFF: Duration = Duration::from_secs(1);

/// How long a capture waits for a producer's transaction.
const CAPTURE_WAIT: Duration = Duration::from_secs(1);

/// A whole-frame fill restarted this many times (its damage grew past a
/// plan) completes in one frame the next time.
const MAX_RESTARTS: u8 = 2;

/// Where a live pass's textures live: GPU memory for the renderer, a
/// `Vec<u8>` for the mirror.
pub trait LiveTextureBackend {
    type Texture;

    /// The largest side the device accepts.
    fn max_dimension(&self) -> u32;

    /// A texture of `width × height`, with a full chain of mip levels
    /// above level 0 when `mipped`, or `Err` when the device has no memory
    /// for it. Called outside every source lock.
    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        mipped: bool,
        label: &str,
    ) -> Result<Self::Texture, TextureOutOfMemory>;

    /// Bytes one row of `width` pixels takes in staging: the device's copy
    /// pitch, which can be wider than the row.
    fn staged_row_bytes(&self, width: u32) -> u64;

    /// Copy `band` of the locked frame into the same place of `texture`: one
    /// write, of at most [`BAND_BYTES`] of staging.
    fn write(&mut self, texture: &mut Self::Texture, read: &LiveImageRead<'_>, band: PixelRect);

    /// Ready `texture` to be drawn with `filter`; outside every lock.
    /// `Trilinear` on a texture without mip levels samples like `Linear`.
    fn prepare_filter(&mut self, texture: &mut Self::Texture, filter: ScalingFilter);

    /// Rebuild the mip levels of a mipped `texture` from its level 0, each
    /// from the one below with [`crate::resample::downsample_half`]'s box:
    /// every texel when `region` is `None`, else the texels each level's
    /// [`mip_footprint`](super::mips::mip_footprint) of `region` names.
    /// `opaque`: the source's fourth byte is not alpha, so the average is
    /// plain and every level above 0 is opaque. Outside every lock; level 0
    /// holds what was written to it.
    fn rebuild_mips(
        &mut self,
        texture: &mut Self::Texture,
        region: Option<PixelRect>,
        opaque: bool,
    );

    /// Bytes `texture` holds.
    fn texture_bytes(&self, texture: &Self::Texture) -> u64;

    /// Whether the device is lost: nothing written reaches the screen again.
    fn device_lost(&self) -> bool {
        false
    }

    /// A texture was dropped; the device frees it at its next maintenance.
    fn released(&mut self) {}
}

/// A texture creation the device refused for want of memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureOutOfMemory;

/// Why a live pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePassMode {
    /// A frame the window presents.
    Present,
    /// A screenshot: the latest commit whatever the lock or a pause, every
    /// band of a large frame before the draw, counted as a capture.
    Capture,
}

/// What the draw does with one quad, and what a screenshot reports of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct QuadDecision {
    /// Draw the source's current texture: it has the quad's painted size.
    pub draw: bool,
    /// The quad shows an older picture or only its background: the lock was
    /// busy, the size changed after layout, or nothing is uploaded yet.
    pub deferred: bool,
    /// The generation the drawn texture holds; 0 with none.
    pub generation: u64,
}

/// What one renderer holds and did for live pictures. Monotonic counters,
/// except the four sizes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveTextureStats {
    /// Textures held for the sources the last render drew, staged ones
    /// included.
    pub textures: usize,
    /// Their bytes, every level included.
    pub bytes: u64,
    /// Parked textures: small ones kept for sources the last render did not
    /// draw.
    pub textures_parked: usize,
    pub bytes_parked: u64,
    pub uploads_full: u64,
    pub uploads_partial: u64,
    pub bytes_uploaded: u64,
    /// Writes; a banded upload counts one per band.
    pub upload_calls: u64,
    /// Bands of whole-frame uploads spread over several frames.
    pub progressive_bands: u64,
    /// Texture creations the device refused for want of memory.
    pub alloc_failures: u64,
    /// Frames that found a producer holding a source's lock.
    pub contended: u64,
    /// Locks taken with a bounded wait.
    pub blocking_waits: u64,
    /// Frames whose locked buffer had another size than the one laid out.
    pub stale_deferrals: u64,
    /// Mip chains rebuilt, wholly or in part: at most one per source and
    /// render, however many commits the upload folded in.
    pub mip_updates: u64,
    pub device_lost: bool,
}

struct Tex<T> {
    texture: T,
    size: (u32, u32),
    /// The generation the texture holds.
    generation: u64,
    /// Filters the backend has readied it for, one bit each.
    filters: u8,
    /// It has mip levels above level 0. Once it has, it keeps them for its
    /// life: a source whose `Trilinear` quads come and go is not uploaded
    /// again for it.
    mipped: bool,
    /// What of level 0 changed since its mip levels were last built: the
    /// next render that draws the source through `Trilinear` rebuilds that
    /// much.
    stale: Option<Stale>,
}

impl<T> Tex<T> {
    fn new(texture: T, size: (u32, u32), mipped: bool) -> Self {
        Self {
            texture,
            size,
            generation: 0,
            filters: 0,
            mipped,
            stale: mipped.then_some(Stale::Whole),
        }
    }

    /// Record that `changed` of level 0 was written.
    fn changed(&mut self, changed: Stale) {
        if self.mipped {
            self.stale = Some(match self.stale {
                Some(before) => before.with(changed),
                None => changed,
            });
        }
    }

    /// Whether it can be drawn for a source painted at `size`, by quads of
    /// which some sample through `Trilinear` when `mips`.
    fn fits(&self, size: (u32, u32), mips: bool) -> bool {
        self.size == size && (self.mipped || !mips)
    }
}

/// The part of a texture's level 0 changed since its mip levels were built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stale {
    Whole,
    /// Within this rect.
    Within(PixelRect),
}

impl Stale {
    fn with(self, other: Stale) -> Stale {
        match (self, other) {
            (Stale::Within(a), Stale::Within(b)) => Stale::Within(a.union(&b)),
            _ => Stale::Whole,
        }
    }
}

/// A whole-frame fill spread over several frames.
#[derive(Clone, Copy)]
struct Fill {
    /// The generation the first band was copied at.
    first: u64,
    /// The first row not copied yet.
    next_row: u32,
    restarts: u8,
}

struct Staged<T> {
    tex: Tex<T>,
    fill: Option<Fill>,
}

struct Entry<T> {
    source: LiveImageSource,
    current: Option<Tex<T>>,
    staged: Option<Staged<T>>,
    alloc_failed: Option<((u32, u32), Instant)>,
    alloc_logged: Option<(u32, u32)>,
    oversize_logged: Option<(u32, u32)>,
    /// Busy frames in a row.
    contended: u8,
    /// A plan too large for one frame found no room for a second texture:
    /// upload it in place, once.
    in_place: bool,
}

struct Parked<T> {
    source: Weak<Shared>,
    id: LiveImageId,
    tex: Tex<T>,
    bytes: u64,
}

/// What one pass did, by source.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassCounts {
    /// Sources whose textures went, freed or parked, because no quad drew
    /// them.
    pub pruned: u32,
    /// Sources whose texture was kept because every quad of them was paused.
    pub kept: u32,
}

/// How long the parts of the last prepare that hold a source's lock took,
/// in real time: what a renderer keeping timing histograms reads after
/// each render. The vectors keep their capacity from one prepare to the
/// next.
#[doc(hidden)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassTimings {
    /// Each hold of a source's lock, in the order they were taken.
    pub lock_holds: Vec<Duration>,
    /// Each upload that completed, from the commit of the generation it
    /// uploaded to the end of its writes.
    pub commit_to_upload: Vec<Duration>,
}

/// End a hold of `read`'s lock begun at `locked`, and record it.
fn end_hold(timings: &mut PassTimings, read: LiveImageRead<'_>, locked: Instant) {
    drop(read);
    timings.lock_holds.push(locked.elapsed());
}

/// Record the delay of an upload of `read`'s generation that just ended.
fn record_upload(timings: &mut PassTimings, read: &LiveImageRead<'_>) {
    if let Some(committed) = read.commit_instant(read.generation()) {
        timings
            .commit_to_upload
            .push(Instant::now().saturating_duration_since(committed));
    }
}

/// What processing one source decided.
#[derive(Default)]
struct Outcome {
    uploaded: bool,
    deferred: bool,
    kept: bool,
    /// The device is lost: record nothing, so counters stop.
    lost: bool,
}

/// One window's live pass: what its renderer does with a frame's live
/// pictures before it draws them.
///
/// For each source a frame draws, in order:
///
/// 1. Every quad marks its consumer observed and takes its pixel flag
///    before anything reads the generation; a paused quad parks it instead,
///    so commits stop waking a paused window.
/// 2. The wanted size is the painted size; when two quads of the source
///    were painted at different sizes (a widget attached across a resize),
///    the one the source has now wins, else the larger.
/// 3. A size the device cannot hold, a texture creation the device refused
///    less than a second ago, and a lost device draw what can be drawn and
///    upload nothing; each parks the pixel flags, so a broken source costs
///    no redraw per commit.
/// 4. Every quad of the source paused, a texture of the wanted size held,
///    a presented frame: keep it, take no lock.
/// 5. A texture of the wanted size is staged, outside the lock, when none is
///    current or staged.
/// 6. Nothing new (the generation is the texture's): no lock.
/// 7. Otherwise lock: without waiting, or for a bounded time when a staged
///    texture waits for its first upload, after two busy frames in a row,
///    and always in a capture. A busy lock draws the held texture; the
///    producer's unlock wakes the window. A buffer of another size than the
///    one laid out draws the held texture; the size change woke layout.
/// 8. Under the lock, the staged texture is filled whole and becomes current,
///    or the current one gets the plan's rects. Nothing is created under the
///    lock. A whole-frame upload larger than the staging budget fills a
///    staged texture over several presented frames instead, waking the
///    window for each, while the previous picture keeps drawing; the render
///    that completes it writes the damage since its first band, under the
///    same lock, then swaps. No frame ever shows half of two generations.
/// 9. Every source no quad drew loses its textures; a small texture of a
///    source that committed once moves to a pool of parked textures, so an
///    image scrolled out and back is not uploaded again. Consumers that were
///    in the last render and are not in this one are marked unobserved.
pub struct LivePass<B: LiveTextureBackend> {
    backend: B,
    entries: HashMap<LiveImageId, Entry<B::Texture>>,
    /// Most recently parked last.
    parked: Vec<Parked<B::Texture>>,
    /// The consumers of the last render.
    previous: Vec<LiveImageConsumer>,
    decisions: Vec<QuadDecision>,
    counts: PassCounts,
    timings: PassTimings,
    stats: LiveTextureStats,
    staging_budget: u64,
    band_bytes: u64,
    park_budget: u64,
    refresh: Duration,
}

impl<B: LiveTextureBackend> LivePass<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            entries: HashMap::new(),
            parked: Vec::new(),
            previous: Vec::new(),
            decisions: Vec::new(),
            counts: PassCounts::default(),
            timings: PassTimings::default(),
            stats: LiveTextureStats::default(),
            staging_budget: STAGING_BUDGET,
            band_bytes: BAND_BYTES,
            park_budget: PARK_BUDGET,
            refresh: Duration::from_micros(16_667),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// The display's refresh interval: about how long a presented frame
    /// waits for a producer before it draws the previous picture.
    pub fn set_refresh_interval(&mut self, refresh: Duration) {
        self.refresh = refresh;
    }

    /// The refresh interval last set: 60 Hz until one is.
    pub fn refresh_interval(&self) -> Duration {
        self.refresh
    }

    /// Test hook: the staging a presented frame spends before a large
    /// whole-frame upload waits for the next frame.
    #[doc(hidden)]
    pub fn set_staging_budget(&mut self, bytes: u64) {
        self.staging_budget = bytes.max(1);
    }

    /// Test hook: the largest staging one write takes.
    #[doc(hidden)]
    pub fn set_band_bytes(&mut self, bytes: u64) {
        self.band_bytes = bytes.max(1);
    }

    /// Test hook: the parked pool's budget; 0 frees every texture a render
    /// does not draw, at once.
    #[doc(hidden)]
    pub fn set_park_budget(&mut self, bytes: u64) {
        self.park_budget = bytes;
    }

    /// The current texture for source `id`, to draw.
    pub fn texture(&self, id: LiveImageId) -> Option<&B::Texture> {
        self.entries
            .get(&id)
            .and_then(|e| e.current.as_ref())
            .map(|t| &t.texture)
    }

    /// The last [`prepare`](Self::prepare)'s decisions, one per quad.
    pub fn decisions(&self) -> &[QuadDecision] {
        &self.decisions
    }

    /// What the last [`prepare`](Self::prepare) did, by source.
    #[doc(hidden)]
    pub fn last_counts(&self) -> PassCounts {
        self.counts
    }

    /// How long the last [`prepare`](Self::prepare) held each lock, and
    /// how long each of its uploads came after its commit.
    #[doc(hidden)]
    pub fn last_timings(&self) -> &PassTimings {
        &self.timings
    }

    pub fn stats(&self) -> LiveTextureStats {
        let mut stats = self.stats;
        stats.textures = 0;
        stats.bytes = 0;
        for entry in self.entries.values() {
            for tex in entry
                .current
                .iter()
                .chain(entry.staged.iter().map(|s| &s.tex))
            {
                stats.textures += 1;
                stats.bytes += self.backend.texture_bytes(&tex.texture);
            }
        }
        stats.textures_parked = self.parked.len();
        stats.bytes_parked = self.parked.iter().map(|p| p.bytes).sum();
        stats.device_lost = self.backend.device_lost();
        stats
    }

    /// Run the live pass for `quads`, the frame's live pictures in draw
    /// order: upload what each window's textures lack, drop what no quad
    /// draws, and decide what each quad draws.
    pub fn prepare(&mut self, quads: &[LiveImageQuad], mode: LivePassMode) -> &[QuadDecision] {
        self.prepare_at(quads, mode, Instant::now())
    }

    /// [`prepare`](Self::prepare) at a given instant, for tests of the
    /// allocation backoff.
    #[doc(hidden)]
    pub fn prepare_at(
        &mut self,
        quads: &[LiveImageQuad],
        mode: LivePassMode,
        now: Instant,
    ) -> &[QuadDecision] {
        // 1. Flags first: taken before any generation is read.
        for quad in quads {
            quad.consumer.set_observed(true);
            if quad.paused {
                quad.consumer.park_pixels();
            } else {
                let _ = quad.consumer.take_pixels();
            }
        }
        // The sources, in the order the frame first draws them.
        let mut order: Vec<(LiveImageId, Vec<usize>)> = Vec::new();
        for (i, quad) in quads.iter().enumerate() {
            let id = quad.consumer.source().id();
            match order.iter_mut().find(|(other, _)| *other == id) {
                Some((_, indices)) => indices.push(i),
                None => order.push((id, vec![i])),
            }
        }
        self.decisions.clear();
        self.decisions.resize(quads.len(), QuadDecision::default());
        self.counts = PassCounts::default();
        self.timings.lock_holds.clear();
        self.timings.commit_to_upload.clear();
        let mut staging_left = self.staging_budget;
        let mut drawn: Vec<LiveImageId> = Vec::with_capacity(order.len());
        for (id, indices) in &order {
            let group: Vec<&LiveImageQuad> = indices.iter().map(|&i| &quads[i]).collect();
            let Some(wanted) = wanted_size(&group) else {
                // Nothing to show for any of them: the source's textures go.
                self.record(&group, &Outcome::default(), None, mode);
                let paused = group.iter().all(|q| q.paused);
                for quad in &group {
                    quad.consumer.set_paused(paused);
                }
                for &i in indices {
                    self.decisions[i] = QuadDecision {
                        draw: false,
                        deferred: false,
                        generation: 0,
                    };
                }
                continue;
            };
            drawn.push(*id);
            let paused = group.iter().all(|q| q.paused);
            let mips = group
                .iter()
                .any(|q| q.filter == ScalingFilter::Trilinear && q.painted.is_some());
            let outcome = self.process(
                *id,
                &group,
                (wanted, mips),
                paused,
                mode,
                now,
                &mut staging_left,
            );
            self.counts.kept += u32::from(outcome.kept);
            let current = self
                .entries
                .get(id)
                .and_then(|e| e.current.as_ref())
                .map(|t| (t.size, t.generation));
            for (&i, quad) in indices.iter().zip(&group) {
                let draw = current.is_some_and(|(size, _)| Some(size) == quad.painted);
                self.decisions[i] = QuadDecision {
                    draw,
                    deferred: quad.painted.is_some() && (outcome.deferred || !draw),
                    generation: current.map_or(0, |(_, g)| g),
                };
            }
            // Bind what each drawing quad needs, and bring the mip chain up
            // to date for a quad that samples it, outside every lock.
            if let Some(entry) = self.entries.get_mut(id)
                && let Some(tex) = entry.current.as_mut()
            {
                let samples_chain = indices
                    .iter()
                    .zip(&group)
                    .any(|(&i, q)| q.filter == ScalingFilter::Trilinear && self.decisions[i].draw);
                if samples_chain
                    && !outcome.lost
                    && tex.mipped
                    && let Some(stale) = tex.stale.take()
                {
                    let region = match stale {
                        Stale::Whole => None,
                        Stale::Within(rect) => Some(rect),
                    };
                    let opaque = entry.source.format().is_opaque();
                    self.backend.rebuild_mips(&mut tex.texture, region, opaque);
                    self.stats.mip_updates += 1;
                }
                for (&i, quad) in indices.iter().zip(&group) {
                    let bit = filter_bit(quad.filter);
                    if self.decisions[i].draw && tex.filters & bit == 0 {
                        self.backend.prepare_filter(&mut tex.texture, quad.filter);
                        tex.filters |= bit;
                    }
                }
            }
            let window_generation = current.map(|(_, g)| g);
            self.record(&group, &outcome, window_generation, mode);
            for quad in &group {
                quad.consumer.set_paused(paused && !outcome.lost);
            }
        }
        // 9. What no quad drew goes, or parks.
        let gone: Vec<LiveImageId> = self
            .entries
            .keys()
            .filter(|id| !drawn.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(entry) = self.entries.remove(&id) {
                self.counts.pruned += 1;
                self.retire(id, entry);
            }
        }
        self.trim_parked();
        for consumer in &self.previous {
            if !quads.iter().any(|q| q.consumer.ptr_eq(consumer)) {
                consumer.set_observed(false);
                consumer.set_paused(false);
            }
        }
        self.previous = quads.iter().map(|q| q.consumer.clone()).collect();
        &self.decisions
    }

    /// Steps 3 to 8 for one source.
    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        id: LiveImageId,
        group: &[&LiveImageQuad],
        (wanted, mips): ((u32, u32), bool),
        paused: bool,
        mode: LivePassMode,
        now: Instant,
        staging_left: &mut u64,
    ) -> Outcome {
        let source = group[0].consumer.source().clone();
        if !self.entries.contains_key(&id) {
            let restored = self.unpark(id);
            self.entries.insert(
                id,
                Entry {
                    source: source.clone(),
                    current: restored,
                    staged: None,
                    alloc_failed: None,
                    alloc_logged: None,
                    oversize_logged: None,
                    contended: 0,
                    in_place: false,
                },
            );
        }
        let max = self.backend.max_dimension();
        if wanted.0 > max || wanted.1 > max {
            let entry = self.entries.get_mut(&id).expect("inserted above");
            if entry.oversize_logged != Some(wanted) {
                entry.oversize_logged = Some(wanted);
                eprintln!(
                    "teksilo: {source:?} is {}x{}, larger than this device's largest texture side \
                     of {max}: it draws its background only",
                    wanted.0, wanted.1
                );
            }
            let dropped = drop_mismatched(entry, wanted);
            self.release(dropped);
            park_all(group);
            return Outcome {
                deferred: true,
                ..Outcome::default()
            };
        }
        if self.backend.device_lost() {
            park_all(group);
            return Outcome {
                lost: true,
                deferred: true,
                ..Outcome::default()
            };
        }
        let entry = self.entries.get_mut(&id).expect("inserted above");
        let current_fits = entry.current.as_ref().is_some_and(|t| t.fits(wanted, mips));
        // A texture staged at this size keeps mip levels the current one
        // has: a source does not lose its chain to a restage.
        let mipped = mips
            || entry
                .current
                .as_ref()
                .is_some_and(|t| t.size == wanted && t.mipped);
        if paused && mode == LivePassMode::Present && current_fits {
            return Outcome {
                kept: true,
                ..Outcome::default()
            };
        }
        if let Some((shape, at)) = entry.alloc_failed {
            if shape == wanted && now.saturating_duration_since(at) < ALLOC_BACKOFF {
                park_all(group);
                return Outcome {
                    deferred: true,
                    ..Outcome::default()
                };
            }
            entry.alloc_failed = None;
        }
        let staged_fits = entry
            .staged
            .as_ref()
            .is_some_and(|s| s.tex.fits(wanted, mips));
        if !current_fits && !staged_fits {
            let stale_staged = entry.staged.take();
            if stale_staged.is_some() {
                self.backend.released();
            }
            drop(stale_staged);
            match self
                .backend
                .create_texture(wanted.0, wanted.1, mipped, &texture_label(&source))
            {
                Ok(texture) => {
                    let entry = self.entries.get_mut(&id).expect("inserted above");
                    entry.staged = Some(Staged {
                        tex: Tex::new(texture, wanted, mipped),
                        fill: None,
                    });
                }
                Err(TextureOutOfMemory) => {
                    self.stats.alloc_failures += 1;
                    let entry = self.entries.get_mut(&id).expect("inserted above");
                    entry.alloc_failed = Some((wanted, now));
                    if entry.alloc_logged != Some(wanted) {
                        entry.alloc_logged = Some(wanted);
                        eprintln!(
                            "teksilo: the device has no memory for a {}x{} texture of {source:?}: \
                             it is retried in a second or at another size",
                            wanted.0, wanted.1
                        );
                    }
                    park_all(group);
                    return Outcome {
                        deferred: true,
                        ..Outcome::default()
                    };
                }
            }
        }
        let entry = self.entries.get_mut(&id).expect("inserted above");
        let has_staged = entry.staged.is_some();
        if !has_staged
            && entry
                .current
                .as_ref()
                .is_some_and(|t| t.generation == source.generation())
        {
            return Outcome::default();
        }
        let frame_bytes = u64::from(wanted.0) * u64::from(wanted.1) * 4;
        let blocking = mode == LivePassMode::Capture || has_staged || entry.contended >= 2;
        let consumer = &group[0].consumer;
        let attempt = if blocking {
            self.stats.blocking_waits += 1;
            let wait = match mode {
                LivePassMode::Capture => CAPTURE_WAIT,
                // About one refresh, plus the time a producer takes to copy
                // a frame of this size at a conservative 2 GB/s.
                LivePassMode::Present => {
                    self.refresh + Duration::from_nanos(frame_bytes.saturating_div(2))
                }
            };
            consumer.read_for(wait)
        } else {
            consumer.try_read()
        };
        let read = match attempt {
            ReadAttempt::Locked(read) => read,
            ReadAttempt::Busy => {
                self.stats.contended += 1;
                let entry = self.entries.get_mut(&id).expect("inserted above");
                entry.contended = entry.contended.saturating_add(1);
                return Outcome {
                    deferred: true,
                    ..Outcome::default()
                };
            }
        };
        let locked = Instant::now();
        let entry = self.entries.get_mut(&id).expect("inserted above");
        entry.contended = 0;
        if read.size() != Some(wanted) {
            self.stats.stale_deferrals += 1;
            end_hold(&mut self.timings, read, locked);
            return Outcome {
                deferred: true,
                ..Outcome::default()
            };
        }
        let progressive_allowed = mode == LivePassMode::Present;
        if let Some(mut staged) = entry.staged.take() {
            let outcome = fill_staged(
                &mut self.backend,
                &mut self.stats,
                &read,
                &mut staged,
                wanted,
                progressive_allowed,
                staging_left,
                self.band_bytes,
            );
            let entry = self.entries.get_mut(&id).expect("inserted above");
            match outcome {
                FillOutcome::Complete => {
                    staged.tex.generation = read.generation();
                    record_upload(&mut self.timings, &read);
                    end_hold(&mut self.timings, read, locked);
                    if entry.current.replace(staged.tex).is_some() {
                        self.backend.released();
                    }
                    Outcome {
                        uploaded: true,
                        ..Outcome::default()
                    }
                }
                FillOutcome::Partial => {
                    // The previous picture keeps drawing, if any: older than
                    // the latest commit either way.
                    end_hold(&mut self.timings, read, locked);
                    entry.staged = Some(staged);
                    wake_window(group);
                    Outcome {
                        deferred: true,
                        ..Outcome::default()
                    }
                }
            }
        } else {
            let in_place = std::mem::take(&mut entry.in_place);
            let current = entry.current.as_mut().expect("current fits: checked above");
            let plan = read.plan(Some(current.generation));
            let bytes = plan_bytes(&self.backend, &plan, wanted);
            if progressive_allowed && bytes > self.staging_budget && !in_place {
                // Too much for one frame: fill a second texture over the next
                // frames rather than stall this one or tear the picture. It is
                // created outside the lock; the next frame starts filling it.
                end_hold(&mut self.timings, read, locked);
                let created = self.backend.create_texture(
                    wanted.0,
                    wanted.1,
                    mipped,
                    &texture_label(&source),
                );
                let entry = self.entries.get_mut(&id).expect("inserted above");
                match created {
                    Ok(texture) => {
                        entry.staged = Some(Staged {
                            tex: Tex::new(texture, wanted, mipped),
                            fill: None,
                        });
                    }
                    // No room for a second copy: the next frame uploads in
                    // place, whole, as a smaller frame would.
                    Err(TextureOutOfMemory) => {
                        self.stats.alloc_failures += 1;
                        entry.in_place = true;
                    }
                }
                wake_window(group);
                return Outcome {
                    deferred: true,
                    ..Outcome::default()
                };
            }
            let uploaded = match plan {
                UploadPlan::Nothing => false,
                UploadPlan::Full => {
                    debug_assert_eq!(read.size(), Some(current.size), "checked above");
                    write_rects(
                        &mut self.backend,
                        &mut self.stats,
                        &mut current.texture,
                        &read,
                        &[PixelRect::full(wanted.0, wanted.1)],
                        self.band_bytes,
                    );
                    current.changed(Stale::Whole);
                    self.stats.uploads_full += 1;
                    true
                }
                UploadPlan::Rects(rects) => {
                    write_rects(
                        &mut self.backend,
                        &mut self.stats,
                        &mut current.texture,
                        &read,
                        rects.as_slice(),
                        self.band_bytes,
                    );
                    let bounds = rects.as_slice().iter().copied().reduce(|a, b| a.union(&b));
                    if let Some(bounds) = bounds {
                        current.changed(Stale::Within(bounds));
                    }
                    self.stats.uploads_partial += 1;
                    true
                }
            };
            *staging_left = staging_left.saturating_sub(bytes);
            current.generation = read.generation();
            if uploaded {
                record_upload(&mut self.timings, &read);
            }
            end_hold(&mut self.timings, read, locked);
            Outcome {
                uploaded,
                ..Outcome::default()
            }
        }
    }

    /// Record what this render did for each consumer of a source.
    fn record(
        &self,
        group: &[&LiveImageQuad],
        outcome: &Outcome,
        window_generation: Option<u64>,
        mode: LivePassMode,
    ) {
        if outcome.lost {
            return;
        }
        let mut seen: Vec<&LiveImageConsumer> = Vec::with_capacity(group.len());
        for quad in group {
            if seen.iter().any(|c| c.ptr_eq(&quad.consumer)) {
                continue;
            }
            seen.push(&quad.consumer);
            quad.consumer.record_render(RenderRecord {
                drawn: true,
                uploaded: outcome.uploaded,
                deferred: outcome.deferred,
                paused: group.iter().all(|q| q.paused),
                kept: outcome.kept,
                capture: mode == LivePassMode::Capture,
                window_generation: window_generation.unwrap_or(0),
            });
        }
    }

    /// A source no quad drew: park its texture when it is small and its
    /// source committed once, else free it.
    fn retire(&mut self, id: LiveImageId, entry: Entry<B::Texture>) {
        let Entry {
            source,
            current,
            staged,
            ..
        } = entry;
        if staged.is_some() {
            self.backend.released();
        }
        drop(staged);
        let Some(tex) = current else {
            return;
        };
        let bytes = self.backend.texture_bytes(&tex.texture);
        let one_commit = source.stats().commits <= 1;
        let live = source.status() == super::LiveImageStatus::Live;
        if self.park_budget > 0
            && bytes <= PARK_MAX_TEXTURE.min(self.park_budget)
            && one_commit
            && live
        {
            self.parked.push(Parked {
                source: std::sync::Arc::downgrade(&source.shared),
                id,
                tex,
                bytes,
            });
        } else {
            drop(tex);
            self.backend.released();
        }
    }

    /// A parked texture for `id`, if the pool holds one of a live source.
    fn unpark(&mut self, id: LiveImageId) -> Option<Tex<B::Texture>> {
        let at = self.parked.iter().position(|p| p.id == id)?;
        let parked = self.parked.remove(at);
        if parked.source.upgrade().is_none() {
            drop(parked);
            self.backend.released();
            return None;
        }
        Some(parked.tex)
    }

    /// Free parked textures of dead sources, then the oldest past the budget.
    fn trim_parked(&mut self) {
        let before = self.parked.len();
        self.parked.retain(|p| p.source.strong_count() > 0);
        let mut freed = before - self.parked.len();
        let mut total: u64 = self.parked.iter().map(|p| p.bytes).sum();
        while total > self.park_budget && !self.parked.is_empty() {
            total -= self.parked.remove(0).bytes;
            freed += 1;
        }
        for _ in 0..freed {
            self.backend.released();
        }
    }

    fn release(&mut self, dropped: usize) {
        for _ in 0..dropped {
            self.backend.released();
        }
    }
}

impl<B: LiveTextureBackend> Drop for LivePass<B> {
    /// The window closes: every consumer of its last render is no longer
    /// observed, and every texture goes.
    fn drop(&mut self) {
        for consumer in &self.previous {
            consumer.set_observed(false);
            consumer.set_paused(false);
        }
        let held = self
            .entries
            .values()
            .map(|e| usize::from(e.current.is_some()) + usize::from(e.staged.is_some()))
            .sum::<usize>()
            + self.parked.len();
        if held > 0 {
            self.backend.released();
        }
    }
}

/// The size to keep a texture at for a source drawn by `group`: `None` when
/// none of them has anything to show.
fn wanted_size(group: &[&LiveImageQuad]) -> Option<(u32, u32)> {
    let painted: Vec<(u32, u32)> = group.iter().filter_map(|q| q.painted).collect();
    let first = *painted.first()?;
    if painted.iter().all(|&p| p == first) {
        return Some(first);
    }
    // Two sizes in one frame: the source's own size wins, else the larger,
    // so every window decides alike.
    let now = group[0].consumer.source().size();
    if let Some(now) = now
        && painted.contains(&now)
    {
        return Some(now);
    }
    painted
        .into_iter()
        .max_by_key(|&(w, h)| (u64::from(w) * u64::from(h), w, h))
}

/// `live_image`, or `live_image:<label>` for a labelled source.
fn texture_label(source: &LiveImageSource) -> String {
    match source.label() {
        Some(label) => format!("live_image:{label}"),
        None => "live_image".to_owned(),
    }
}

/// Drop every texture of `entry` that is not `wanted`-sized: how many.
/// Whether it has mip levels does not matter here: this is for a size the
/// device cannot hold at all.
fn drop_mismatched<T>(entry: &mut Entry<T>, wanted: (u32, u32)) -> usize {
    let mut dropped = 0;
    if entry.current.as_ref().is_some_and(|t| t.size != wanted) {
        entry.current = None;
        dropped += 1;
    }
    if entry.staged.as_ref().is_some_and(|s| s.tex.size != wanted) {
        entry.staged = None;
        dropped += 1;
    }
    dropped
}

fn park_all(group: &[&LiveImageQuad]) {
    for quad in group {
        quad.consumer.park_pixels();
    }
}

/// Ask for another frame: a whole-frame fill continues there.
fn wake_window(group: &[&LiveImageQuad]) {
    if let Some(waker) = group[0].consumer.shared.waker() {
        waker.wake(WakeKind::Draw);
    }
}

fn filter_bit(filter: ScalingFilter) -> u8 {
    match filter {
        ScalingFilter::Linear => 1,
        ScalingFilter::Nearest => 2,
        ScalingFilter::Trilinear => 4,
    }
}

/// The staging `plan` takes.
fn plan_bytes<B: LiveTextureBackend>(backend: &B, plan: &UploadPlan, size: (u32, u32)) -> u64 {
    match plan {
        UploadPlan::Nothing => 0,
        UploadPlan::Full => backend.staged_row_bytes(size.0) * u64::from(size.1),
        UploadPlan::Rects(rects) => rects
            .as_slice()
            .iter()
            .map(|r| backend.staged_row_bytes(r.width) * u64::from(r.height))
            .sum(),
    }
}

/// Write `rects` of the locked frame into `texture`, each cut into bands of
/// at most `band_bytes` of staging.
fn write_rects<B: LiveTextureBackend>(
    backend: &mut B,
    stats: &mut LiveTextureStats,
    texture: &mut B::Texture,
    read: &LiveImageRead<'_>,
    rects: &[PixelRect],
    band_bytes: u64,
) {
    for &rect in rects {
        write_rows(
            backend,
            stats,
            texture,
            read,
            rect,
            rect.y,
            rect.y + rect.height,
            band_bytes,
        );
    }
}

/// Write rows `from..to` of `rect` in bands.
#[allow(clippy::too_many_arguments)]
fn write_rows<B: LiveTextureBackend>(
    backend: &mut B,
    stats: &mut LiveTextureStats,
    texture: &mut B::Texture,
    read: &LiveImageRead<'_>,
    rect: PixelRect,
    from: u32,
    to: u32,
    band_bytes: u64,
) {
    if rect.width == 0 || from >= to {
        return;
    }
    let row = backend.staged_row_bytes(rect.width).max(1);
    let rows_per_band = (band_bytes / row).clamp(1, u64::from(to - from)) as u32;
    let mut y = from;
    while y < to {
        let rows = rows_per_band.min(to - y);
        let band = PixelRect::new(rect.x, y, rect.width, rows);
        backend.write(texture, read, band);
        stats.upload_calls += 1;
        stats.bytes_uploaded += u64::from(rect.width) * u64::from(rows) * 4;
        y += rows;
    }
}

enum FillOutcome {
    Complete,
    Partial,
}

/// Fill `staged` from the locked frame: whole, or, when the frame needs
/// more staging than this frame has left and the fill may spread, the next
/// rows up to that budget, completing with the damage since its first band.
#[allow(clippy::too_many_arguments)]
fn fill_staged<B: LiveTextureBackend>(
    backend: &mut B,
    stats: &mut LiveTextureStats,
    read: &LiveImageRead<'_>,
    staged: &mut Staged<B::Texture>,
    size: (u32, u32),
    progressive: bool,
    staging_left: &mut u64,
    band_bytes: u64,
) -> FillOutcome {
    let full = PixelRect::full(size.0, size.1);
    let row = backend.staged_row_bytes(size.0).max(1);
    let total = row * u64::from(size.1);
    let spread = staged.fill.is_some() || (progressive && total > *staging_left);
    if !spread {
        write_rects(
            backend,
            stats,
            &mut staged.tex.texture,
            read,
            &[full],
            band_bytes,
        );
        *staging_left = staging_left.saturating_sub(total);
        stats.uploads_full += 1;
        return FillOutcome::Complete;
    }
    let fill = staged.fill.get_or_insert(Fill {
        first: read.generation(),
        next_row: 0,
        restarts: 0,
    });
    if !progressive {
        // A capture: finish now, every band before the draw.
        let from = fill.next_row;
        write_rows(
            backend,
            stats,
            &mut staged.tex.texture,
            read,
            full,
            from,
            size.1,
            band_bytes,
        );
        let first = fill.first;
        finish_fill(backend, stats, read, staged, first, size, band_bytes, true);
        return FillOutcome::Complete;
    }
    let affordable = (*staging_left / row).max(1) as u32;
    let from = fill.next_row;
    let to = from.saturating_add(affordable).min(size.1);
    write_rows(
        backend,
        stats,
        &mut staged.tex.texture,
        read,
        full,
        from,
        to,
        band_bytes,
    );
    stats.progressive_bands += 1;
    *staging_left = staging_left.saturating_sub(row * u64::from(to - from));
    let fill = staged.fill.as_mut().expect("set above");
    fill.next_row = to;
    if to < size.1 {
        return FillOutcome::Partial;
    }
    let (first, restarts) = (fill.first, fill.restarts);
    if finish_fill(
        backend,
        stats,
        read,
        staged,
        first,
        size,
        band_bytes,
        restarts >= MAX_RESTARTS,
    ) {
        FillOutcome::Complete
    } else {
        // The damage since the first band is the whole frame: start over
        // from the current generation, the picture still the previous one.
        staged.fill = Some(Fill {
            first: read.generation(),
            next_row: 0,
            restarts: restarts + 1,
        });
        FillOutcome::Partial
    }
}

/// Every row is copied: write what changed since generation `first`, so
/// the texture holds the locked generation whole. `false` when that is the
/// whole frame and `force` is not set.
#[allow(clippy::too_many_arguments)]
fn finish_fill<B: LiveTextureBackend>(
    backend: &mut B,
    stats: &mut LiveTextureStats,
    read: &LiveImageRead<'_>,
    staged: &mut Staged<B::Texture>,
    first: u64,
    size: (u32, u32),
    band_bytes: u64,
    force: bool,
) -> bool {
    match read.plan(Some(first)) {
        UploadPlan::Nothing => {}
        UploadPlan::Rects(rects) => {
            write_rects(
                backend,
                stats,
                &mut staged.tex.texture,
                read,
                rects.as_slice(),
                band_bytes,
            );
        }
        UploadPlan::Full if force => {
            write_rects(
                backend,
                stats,
                &mut staged.tex.texture,
                read,
                &[PixelRect::full(size.0, size.1)],
                band_bytes,
            );
        }
        UploadPlan::Full => return false,
    }
    staged.fill = None;
    stats.uploads_full += 1;
    true
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests;
