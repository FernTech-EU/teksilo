// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What changed: the damage a transaction marks, the log of the last
//! [`RING`] commits, and the plan a consumer uploads from.
//!
//! Each consumer uploads the union of the damage since its own last upload,
//! so a consumer that falls behind still receives a correct picture and the
//! frames it never showed cost nothing. Overlapping rects are uploaded as
//! they are: the overlap is written twice, which is cheaper than splitting
//! rects and never wrong.

use std::time::Instant;

use super::PixelRect;

/// Commits the log keeps, and rects a commit or a plan carries: past
/// either, the damage is the whole frame.
pub(crate) const RING: usize = 16;

/// Up to sixteen rects, kept inline: no allocation per commit or plan.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DamageRects {
    rects: [PixelRect; RING],
    len: u8,
}

impl DamageRects {
    pub(crate) const fn new() -> Self {
        Self {
            rects: [PixelRect::new(0, 0, 0, 0); RING],
            len: 0,
        }
    }

    /// The rects, in the order they were added.
    pub fn as_slice(&self) -> &[PixelRect] {
        &self.rects[..self.len as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Add `rect` unless it is already there. `false` when it would be the
    /// 17th: the caller widens.
    fn push_unique(&mut self, rect: PixelRect) -> bool {
        if self.as_slice().contains(&rect) {
            return true;
        }
        if self.len() == RING {
            return false;
        }
        self.rects[self.len()] = rect;
        self.len += 1;
        true
    }

    fn bounding_box(&self) -> PixelRect {
        self.as_slice()
            .iter()
            .fold(PixelRect::default(), |acc, r| acc.union(r))
    }

    /// The pixels these rects cover together, overlaps counted once.
    fn union_area(&self) -> u64 {
        // Coordinate compression over at most 16 rects: at most 31 × 31
        // cells, each tested against at most 16 rects.
        let rects = self.as_slice();
        let mut xs = [0u64; 2 * RING];
        let mut ys = [0u64; 2 * RING];
        for (i, r) in rects.iter().enumerate() {
            xs[2 * i] = r.x as u64;
            xs[2 * i + 1] = r.x as u64 + r.width as u64;
            ys[2 * i] = r.y as u64;
            ys[2 * i + 1] = r.y as u64 + r.height as u64;
        }
        let xs = sorted_unique(&mut xs[..2 * rects.len()]);
        let ys = sorted_unique(&mut ys[..2 * rects.len()]);
        let mut area = 0;
        for xw in xs.windows(2) {
            for yw in ys.windows(2) {
                let covered = rects.iter().any(|r| {
                    (r.x as u64) <= xw[0]
                        && xw[1] <= r.x as u64 + r.width as u64
                        && (r.y as u64) <= yw[0]
                        && yw[1] <= r.y as u64 + r.height as u64
                });
                if covered {
                    area += (xw[1] - xw[0]) * (yw[1] - yw[0]);
                }
            }
        }
        area
    }
}

/// Sort `values` and move each distinct one to the front, in order: the
/// distinct prefix.
fn sorted_unique(values: &mut [u64]) -> &[u64] {
    values.sort_unstable();
    let mut len = 0;
    for i in 0..values.len() {
        if len == 0 || values[i] != values[len - 1] {
            values[len] = values[i];
            len += 1;
        }
    }
    &values[..len]
}

impl std::fmt::Debug for DamageRects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

/// Damage: rects, or the whole frame.
// The rects are inline on purpose: a commit and a plan allocate nothing.
// Sixteen entries of the log are about 4 KiB.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Damage {
    Rects(DamageRects),
    Full,
}

impl Damage {
    pub(crate) const NONE: Damage = Damage::Rects(DamageRects::new());

    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Damage::Rects(r) if r.is_empty())
    }

    /// Add a non-empty `rect`; past [`RING`] rects, they widen to their
    /// bounding box.
    pub(crate) fn add(&mut self, rect: PixelRect) {
        let Damage::Rects(rects) = self else {
            return;
        };
        if !rects.push_unique(rect) {
            let bbox = rects.bounding_box().union(&rect);
            *rects = DamageRects::new();
            rects.push_unique(bbox);
        }
    }

    /// Whole-frame when the rects cover at least half of a `width × height`
    /// frame: one upload is then cheaper than many.
    fn settled(self, width: u32, height: u32) -> Damage {
        match self {
            Damage::Rects(rects) if covers_half(&rects, width, height) => Damage::Full,
            other => other,
        }
    }
}

fn covers_half(rects: &DamageRects, width: u32, height: u32) -> bool {
    let frame = width as u64 * height as u64;
    frame > 0 && rects.union_area() * 2 >= frame
}

/// What a transaction has marked since the last publishing commit, its own
/// and those of transactions abandoned since.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pending {
    pub(crate) damage: Damage,
    /// The buffer was reallocated or zero-filled at a size.
    pub(crate) resized: bool,
    /// Pixels were written or marked: what moves `Waiting` to `Live`.
    pub(crate) wrote: bool,
    /// The next publishing commit uploads the whole frame, whatever is
    /// marked: the buffer was resized or freed, a new writer session started,
    /// or a panic abandoned a transaction that may have written anywhere.
    pub(crate) force_full: bool,
}

impl Pending {
    pub(crate) const fn new() -> Self {
        Self {
            damage: Damage::NONE,
            resized: false,
            wrote: false,
            force_full: false,
        }
    }

    /// Whether a commit would publish.
    pub(crate) fn publishes(&self) -> bool {
        self.resized || !self.damage.is_none()
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    generation: u64,
    damage: Damage,
    at: Option<Instant>,
}

/// The last [`RING`] commits' damage, oldest overwritten. About 4 KiB, no
/// allocation after construction.
pub(crate) struct DamageRing {
    entries: [Entry; RING],
    /// The next slot to write.
    next: usize,
}

impl DamageRing {
    pub(crate) fn new() -> Self {
        Self {
            entries: [Entry {
                generation: 0,
                damage: Damage::Full,
                at: None,
            }; RING],
            next: 0,
        }
    }

    /// Record commit `generation`'s damage, made at `at`, for a frame of
    /// `width × height`.
    pub(crate) fn record(
        &mut self,
        generation: u64,
        damage: Damage,
        width: u32,
        height: u32,
        at: Instant,
    ) {
        self.entries[self.next] = Entry {
            generation,
            damage: damage.settled(width, height),
            at: Some(at),
        };
        self.next = (self.next + 1) % RING;
    }

    fn entry(&self, generation: u64) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.generation == generation && e.at.is_some())
    }

    /// When commit `generation` was made, while the log still holds it.
    pub(crate) fn commit_instant(&self, generation: u64) -> Option<Instant> {
        self.entry(generation).and_then(|e| e.at)
    }

    /// What a consumer whose texture holds generation `cursor` uploads to
    /// reach `latest`, for a frame of `width × height`. `None` means a new
    /// texture.
    pub(crate) fn plan(
        &self,
        cursor: Option<u64>,
        latest: u64,
        width: u32,
        height: u32,
    ) -> UploadPlan {
        let Some(cursor) = cursor else {
            return UploadPlan::Full;
        };
        if cursor == latest {
            return UploadPlan::Nothing;
        }
        if cursor > latest || latest - cursor > RING as u64 {
            return UploadPlan::Full;
        }
        let mut merged = DamageRects::new();
        for generation in cursor + 1..=latest {
            match self.entry(generation).map(|e| e.damage) {
                Some(Damage::Rects(rects)) => {
                    for &rect in rects.as_slice() {
                        if !merged.push_unique(rect) {
                            return UploadPlan::Full;
                        }
                    }
                }
                Some(Damage::Full) | None => return UploadPlan::Full,
            }
        }
        if merged.is_empty() {
            // Only resize-free commits with nothing marked are never
            // recorded, so an empty merge cannot happen; Full is the safe
            // reading of one.
            return UploadPlan::Full;
        }
        if covers_half(&merged, width, height) {
            return UploadPlan::Full;
        }
        UploadPlan::Rects(merged)
    }
}

/// What a consumer uploads to bring its texture to the latest generation.
// Inline rects, as for `Damage`: planning allocates nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadPlan {
    /// The texture already holds the latest generation.
    Nothing,
    /// The whole frame.
    Full,
    /// These rects, each inside the frame; overlaps are written twice.
    Rects(DamageRects),
}
