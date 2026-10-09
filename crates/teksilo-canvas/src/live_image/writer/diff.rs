// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`LiveImageDiffWriter`]: a writer for a producer that hands over whole
//! frames, committing only what changed.

use super::super::damage::RING;
use super::super::source::validate_size;
use super::super::{LiveImageError, LiveImageStatus, PixelRect};
use super::{LiveImageWriter, check_source};

/// A [`LiveImageWriter`] for a producer that redraws, or copies, a whole
/// frame every time, whether or not anything in it moved: a VM's
/// framebuffer copied at each vsync, a remote desktop's screen, a renderer
/// with no damage tracking of its own.
///
/// [`write_frame`](Self::write_frame) compares the frame with the last one it
/// wrote and commits only the rows that changed, each cut to the span of
/// pixels that changed in it. A frame identical to the last one commits
/// nothing: it takes no lock, wakes no window, uploads nothing and draws
/// nothing, so a still picture fed at 60 Hz costs the window nothing.
///
/// The comparison runs on the producer's thread, against a copy of the last
/// frame it keeps, never under the source's lock: a renderer is kept waiting
/// only while the changed rows are copied in. That copy costs one frame of
/// memory, and the comparison a read of both frames, a fraction of a
/// millisecond for 720 × 1280.
///
/// The result is the frame handed over. When the source no longer holds the
/// last frame this writer wrote (another writer committed, cleared or
/// resized it, or left a transaction abandoned), the next frame is written
/// whole. The one change it cannot see is another writer's write through
/// [`pixels_mut`](super::LiveImageWriteGuard::pixels_mut) left unmarked in a
/// transaction that ended without a commit or a panic: such pixels reach no
/// window either, until something marks them.
///
/// ```
/// # use teksilo_canvas::live_image::*;
/// let source = LiveImageSource::new(LivePixelFormat::Bgrx8);
/// let mut writer = LiveImageDiffWriter::new(source.writer());
/// let mut frame = vec![0u8; 64 * 48 * 4];
/// assert!(writer.write_frame(64, 48, &frame, 64 * 4).unwrap().is_some());
/// // The same frame again: nothing is committed.
/// assert_eq!(writer.write_frame(64, 48, &frame, 64 * 4).unwrap(), None);
/// frame[0] = 255;
/// assert!(writer.write_frame(64, 48, &frame, 64 * 4).unwrap().is_some());
/// ```
pub struct LiveImageDiffWriter {
    writer: LiveImageWriter,
    /// The last frame written, rows `width × bpp` bytes apart.
    copy: Vec<u8>,
    /// Its size; `None` before the first frame and after `forget`.
    size: Option<(u32, u32)>,
    /// The generation it was committed as.
    generation: u64,
    /// Scratch for the changed rows' rects, reused from frame to frame.
    rects: Vec<PixelRect>,
}

impl LiveImageDiffWriter {
    pub fn new(writer: LiveImageWriter) -> Self {
        Self {
            writer,
            copy: Vec::new(),
            size: None,
            generation: 0,
            rects: Vec::new(),
        }
    }

    /// Write `src`, a `width × height` frame whose rows are `src_stride`
    /// bytes apart, committing only what differs from the last frame
    /// written. `Some(generation)` when it committed, `None` when the frame
    /// was identical and the source still shows it.
    ///
    /// # Errors
    ///
    /// As [`LiveImageWriter::write_frame`]. A refused frame changes nothing,
    /// here or in the source.
    pub fn write_frame(
        &mut self,
        width: u32,
        height: u32,
        src: &[u8],
        src_stride: usize,
    ) -> Result<Option<u64>, LiveImageError> {
        validate_size(width, height)?;
        let bpp = self.writer.source_ref().format().bytes_per_pixel();
        check_source(width, height, src, src_stride, bpp)?;
        let same_size = self.size == Some((width, height));
        self.rects.clear();
        if same_size {
            changed_rects(
                &mut self.rects,
                &self.copy,
                src,
                src_stride,
                width,
                height,
                bpp,
            );
            if self.rects.is_empty() && self.source_holds_copy(width, height) {
                return Ok(None);
            }
        }

        let mut guard = self.writer.lock()?;
        // The buffer holds the copy exactly when nothing touched it since
        // this writer's commit: no other commit (the generation), and nothing
        // pending. A mark sets `wrote`; a resize, a free (`clear`, a new
        // session) and a panic inside a transaction set `force_full`, and
        // both stay set until a commit, which moves the generation. Leaving
        // `Live` goes through a free or a revoked session, which `lock`
        // refused above.
        let exact = same_size && {
            let state = guard.state();
            state.generation == self.generation && !state.pending.force_full && !state.pending.wrote
        };
        // The check above saw the source change, or rows changed: the
        // source cannot have come back to this writer's copy since, because
        // the generation only moves forward and `force_full` stays set until
        // it does.
        debug_assert!(
            !exact || !self.rects.is_empty(),
            "the source holds this writer's copy, yet its lock-free check said otherwise"
        );
        let whole = !exact;
        if whole {
            if let Err(error) = guard.resize(width, height) {
                guard.counted = false;
                return Err(error);
            }
            guard.write_rect(PixelRect::full(width, height), src, src_stride)?;
        } else {
            for rect in &self.rects {
                let at = rect.y as usize * src_stride + rect.x as usize * bpp;
                guard.write_rect(*rect, &src[at..], src_stride)?;
            }
        }
        let generation = guard.commit();

        // Bring the copy up to date, outside the lock.
        let row = width as usize * bpp;
        if whole {
            self.copy.clear();
            self.copy.reserve(row * height as usize);
            for y in 0..height as usize {
                self.copy
                    .extend_from_slice(&src[y * src_stride..y * src_stride + row]);
            }
        } else {
            for rect in &self.rects {
                let (x, span) = (rect.x as usize * bpp, rect.width as usize * bpp);
                for y in rect.y as usize..(rect.y + rect.height) as usize {
                    let from = y * src_stride + x;
                    self.copy[y * row + x..y * row + x + span]
                        .copy_from_slice(&src[from..from + span]);
                }
            }
        }
        self.size = Some((width, height));
        self.generation = generation;
        Ok(Some(generation))
    }

    /// Whether the source, read without its lock, still holds the frame this
    /// writer last committed. A transaction another writer abandoned is not
    /// visible from here; it shows only once something commits, and then
    /// the next frame this writer hands over is written whole.
    fn source_holds_copy(&self, width: u32, height: u32) -> bool {
        let source = self.writer.source_ref();
        let meta = source.meta();
        source.generation() == self.generation
            && meta.status == LiveImageStatus::Live
            && meta.size == Some((width, height))
    }

    /// Drop the copy of the last frame, so the next frame is written whole.
    pub fn forget(&mut self) {
        self.copy = Vec::new();
        self.size = None;
    }

    /// The writer, for the source's other operations: `resize`, `clear`,
    /// `is_displayed`. A frame it writes makes the next one handed to this
    /// writer go out whole.
    pub fn writer(&self) -> &LiveImageWriter {
        &self.writer
    }

    /// The writer back, and the copy freed.
    pub fn into_writer(self) -> LiveImageWriter {
        self.writer
    }
}

impl std::fmt::Debug for LiveImageDiffWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveImageDiffWriter")
            .field("writer", &self.writer)
            .field("size", &self.size)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// The rects of `src` that differ from `copy`, both `width × height` with
/// `bpp` bytes a pixel, into `out`: each run of consecutive changed rows is
/// one rect, as wide as the union of its rows' changed spans, and past
/// [`RING`] rects the two neighbours whose union adds the fewest pixels
/// merge, so a commit keeps its rects rather than widening to their
/// bounding box.
pub(super) fn changed_rects(
    out: &mut Vec<PixelRect>,
    copy: &[u8],
    src: &[u8],
    src_stride: usize,
    width: u32,
    height: u32,
    bpp: usize,
) {
    out.clear();
    let row = width as usize * bpp;
    let mut band: Option<(u32, u32, u32)> = None; // (first row, x0, x1)
    for y in 0..height {
        let theirs = &src[y as usize * src_stride..][..row];
        let ours = &copy[y as usize * row..][..row];
        if theirs == ours {
            if let Some((top, x0, x1)) = band.take() {
                out.push(PixelRect::new(x0, top, x1 - x0, y - top));
            }
            continue;
        }
        let first = first_difference(theirs, ours) / bpp;
        let last = last_difference(theirs, ours) + 1;
        let (x0, x1) = (first as u32, last.div_ceil(bpp) as u32);
        band = Some(match band {
            Some((top, b0, b1)) => (top, b0.min(x0), b1.max(x1)),
            None => (y, x0, x1),
        });
    }
    if let Some((top, x0, x1)) = band {
        out.push(PixelRect::new(x0, top, x1 - x0, height - top));
    }
    while out.len() > RING {
        let (at, _) = out
            .windows(2)
            .enumerate()
            .map(|(i, pair)| {
                let merged = pair[0].union(&pair[1]);
                (i, merged.area() - pair[0].area() - pair[1].area())
            })
            .min_by_key(|&(_, added)| added)
            .expect("more than RING rects have neighbours");
        let merged = out[at].union(&out[at + 1]);
        out[at] = merged;
        out.remove(at + 1);
    }
}

/// Bytes compared at a time before the one chunk that differs is scanned:
/// slice equality is a `memcmp`, a byte loop is not.
const CHUNK: usize = 64;

/// The index of the first byte where two rows of one length differ; they
/// do differ.
fn first_difference(a: &[u8], b: &[u8]) -> usize {
    let start = a
        .chunks(CHUNK)
        .zip(b.chunks(CHUNK))
        .position(|(x, y)| x != y)
        .map_or(0, |chunk| chunk * CHUNK);
    start
        + a[start..]
            .iter()
            .zip(&b[start..])
            .position(|(x, y)| x != y)
            .unwrap_or(0)
}

/// The index of the last byte where two rows of one length differ; they do
/// differ.
fn last_difference(a: &[u8], b: &[u8]) -> usize {
    let tail = a.len() % CHUNK;
    let end = if a[a.len() - tail..] != b[b.len() - tail..] {
        a.len()
    } else {
        let body = a.len() - tail;
        let (ours, _) = a[..body].as_chunks::<CHUNK>();
        let (theirs, _) = b[..body].as_chunks::<CHUNK>();
        ours.iter()
            .zip(theirs)
            .rposition(|(x, y)| x != y)
            .map_or(0, |chunk| (chunk + 1) * CHUNK)
    };
    let from = end.saturating_sub(CHUNK.max(tail));
    from + a[from..end]
        .iter()
        .zip(&b[from..end])
        .rposition(|(x, y)| x != y)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
