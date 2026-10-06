// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`LiveImageWriter`] and the transactions it opens.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use super::held;
use super::source::{StateGuard, validate_size};
use super::stats::LiveImageSourceStats;
use super::{LiveImageError, LiveImageSource, LivePixelFormat, PixelRect, RejectedFrame};
use crate::sync::Ordering;

/// A producer's handle on a [`LiveImageSource`]. `Send + Sync`; clones share
/// one session token, and the source frees its pixels and becomes
/// `Disconnected` when the session's last token drops.
///
/// Writes happen in transactions: [`lock`](Self::lock), any number of
/// writes, then [`commit`](LiveImageWriteGuard::commit). One commit is one
/// generation however many writes it holds. The helpers
/// ([`write_frame`](Self::write_frame), [`swap_frame`](Self::swap_frame),
/// [`write_rect`](Self::write_rect), [`resize`](Self::resize),
/// [`clear`](Self::clear)) each run one transaction of their own.
///
/// A writer waits only while a renderer copies bytes out of the buffer, a
/// fraction of a millisecond for a 1080p frame.
#[derive(Clone)]
pub struct LiveImageWriter {
    token: Arc<WriterToken>,
}

/// One writer session's share: the session's last token dropped frees the
/// pixels.
pub(crate) struct WriterToken {
    source: LiveImageSource,
    session: u64,
}

impl WriterToken {
    pub(crate) fn new(source: LiveImageSource, session: u64) -> Self {
        Self { source, session }
    }
}

impl Drop for WriterToken {
    fn drop(&mut self) {
        self.source.shared.release_session(self.session);
    }
}

/// `!Send`, `Sync`: what a guard holding the lock of the thread that took it
/// must be, whatever features another crate enables on parking_lot.
struct NotSend(PhantomData<*const ()>);

// SAFETY: it holds no data; it exists only to withhold `Send`.
unsafe impl Sync for NotSend {}

/// One write transaction: the source's lock, held. Only
/// [`commit`](Self::commit) publishes it; dropping it abandons it, keeping
/// whatever it marked pending for the next commit, and a panic that drops it
/// makes the next commit upload the whole frame.
///
/// `!Send`, so `tokio::spawn` rejects a future holding it across an
/// `.await`. A single-threaded executor (`spawn_local`, a `LocalSet`,
/// `block_on`) does not, and there a suspended transaction keeps the lock
/// from every producer and stalls the window: lint for it with clippy's
/// `await_holding_invalid_type`, configured with
/// `await-holding-invalid-types = ["teksilo_canvas::live_image::LiveImageWriteGuard"]`.
///
/// While it is open, only its own methods may be called on the source from
/// this thread: another lock of the source panics instead of waiting for
/// itself.
///
/// ```compile_fail,E0277
/// # use teksilo_canvas::live_image::*;
/// fn send<T: Send>(_: T) {}
/// let writer = LiveImageSource::new(LivePixelFormat::Rgba8).writer();
/// send(writer.lock().unwrap());
/// ```
///
/// ```
/// # use teksilo_canvas::live_image::*;
/// fn sync<T: Sync>(_: &T) {}
/// let writer = LiveImageSource::new(LivePixelFormat::Rgba8).writer();
/// sync(&writer.lock().unwrap());
/// ```
#[clippy::has_significant_drop]
#[must_use = "a transaction publishes only on `commit()`"]
pub struct LiveImageWriteGuard<'a> {
    guard: Option<StateGuard<'a>>,
    format: LivePixelFormat,
    /// Count a drop without `commit()` as abandoned. A helper refusing its
    /// input after locking changed nothing, and does not count.
    counted: bool,
    _not_send: NotSend,
}

impl LiveImageWriter {
    pub(crate) fn new(token: WriterToken) -> Self {
        Self {
            token: Arc::new(token),
        }
    }

    fn source_ref(&self) -> &LiveImageSource {
        &self.token.source
    }

    /// Start a write transaction. Only [`commit`](LiveImageWriteGuard::commit)
    /// publishes it.
    ///
    /// # Errors
    ///
    /// [`LiveImageError::Revoked`] once a newer session started.
    ///
    /// # Panics
    ///
    /// When this thread already holds a transaction on the source.
    pub fn lock(&self) -> Result<LiveImageWriteGuard<'_>, LiveImageError> {
        let shared = &*self.source_ref().shared;
        shared.assert_not_held("LiveImageWriter::lock");
        self.begin(shared.lock_state())
    }

    /// As [`lock`](Self::lock), without waiting: [`LiveImageError::WouldBlock`]
    /// while another transaction or a renderer holds the lock, this thread's
    /// own transaction included.
    pub fn try_lock(&self) -> Result<LiveImageWriteGuard<'_>, LiveImageError> {
        let shared = &*self.source_ref().shared;
        let guard = shared.try_lock_state().ok_or(LiveImageError::WouldBlock)?;
        self.begin(guard)
    }

    fn begin<'a>(
        &'a self,
        guard: StateGuard<'a>,
    ) -> Result<LiveImageWriteGuard<'a>, LiveImageError> {
        if guard.state().session != self.token.session {
            return Err(LiveImageError::Revoked);
        }
        held::enter(guard.shared().id);
        Ok(LiveImageWriteGuard {
            guard: Some(guard),
            format: self.source_ref().format(),
            counted: true,
            _not_send: NotSend(PhantomData),
        })
    }

    /// Replace the whole frame, resizing if needed, and commit: one copy of
    /// `src`, whose rows are `src_stride` bytes apart. Returns the new
    /// generation.
    pub fn write_frame(
        &self,
        width: u32,
        height: u32,
        src: &[u8],
        src_stride: usize,
    ) -> Result<u64, LiveImageError> {
        validate_size(width, height)?;
        let bpp = self.source_ref().format().bytes_per_pixel();
        check_source(width, height, src, src_stride, bpp)?;
        let mut guard = self.lock()?;
        if let Err(error) = guard.resize(width, height) {
            guard.counted = false;
            return Err(error);
        }
        if let Err(error) = guard.write_rect(PixelRect::full(width, height), src, src_stride) {
            guard.counted = false;
            return Err(error);
        }
        Ok(guard.commit())
    }

    /// Install `pixels` as the frame without copying it, commit, and return
    /// the previous buffer for reuse: empty on the first call and after the
    /// pixels were freed. Its rows are `stride` bytes apart, at least
    /// `width × 4`. The previous buffer holds older content, which the
    /// producer must rewrite in full before its next swap.
    ///
    /// # Errors
    ///
    /// A [`RejectedFrame`] carrying the error and `pixels`, untouched.
    pub fn swap_frame(
        &self,
        width: u32,
        height: u32,
        stride: usize,
        pixels: Vec<u8>,
    ) -> Result<Vec<u8>, RejectedFrame> {
        let bpp = self.source_ref().format().bytes_per_pixel();
        let check = validate_size(width, height)
            .and_then(|()| check_source(width, height, &pixels, stride, bpp));
        if let Err(error) = check {
            return Err(RejectedFrame { error, pixels });
        }
        let mut guard = match self.lock() {
            Ok(guard) => guard,
            Err(error) => return Err(RejectedFrame { error, pixels }),
        };
        let written = (width as usize * bpp * height as usize) as u64;
        let state = guard.state_mut();
        let previous = std::mem::replace(&mut state.pixels, pixels);
        if state.size != Some((width, height)) {
            state.pending.resized = true;
        }
        state.size = Some((width, height));
        state.stride = stride;
        state.pending.force_full = true;
        state.pending.wrote = true;
        state.pending.damage.add(PixelRect::full(width, height));
        guard.count_written(written);
        guard.commit();
        Ok(previous)
    }

    /// Copy one rect from `src`, whose row 0 is the rect's top row and whose
    /// rows are `src_stride` bytes apart, and commit.
    pub fn write_rect(
        &self,
        rect: PixelRect,
        src: &[u8],
        src_stride: usize,
    ) -> Result<u64, LiveImageError> {
        let mut guard = self.lock()?;
        if let Err(error) = guard.write_rect(rect, src, src_stride) {
            guard.counted = false;
            return Err(error);
        }
        Ok(guard.commit())
    }

    /// Resize, zero-filled, and commit. A no-op at the current size. The
    /// status is kept: a resize while `Waiting` stays `Waiting`.
    pub fn resize(&self, width: u32, height: u32) -> Result<u64, LiveImageError> {
        let mut guard = self.lock()?;
        if let Err(error) = guard.resize(width, height) {
            guard.counted = false;
            return Err(error);
        }
        Ok(guard.commit())
    }

    /// Free the pixels and return to `Waiting`; their size becomes the size
    /// hint. Every window frees its texture at its next render.
    pub fn clear(&self) -> Result<(), LiveImageError> {
        let mut guard = self.lock()?;
        guard.counted = false;
        let state = guard.state_mut();
        state.free();
        state.status = super::LiveImageStatus::Waiting;
        Ok(())
    }

    /// As [`LiveImageSource::is_observed`].
    pub fn is_observed(&self) -> bool {
        self.source_ref().is_observed()
    }

    /// As [`LiveImageSource::is_displayed`].
    pub fn is_displayed(&self) -> bool {
        self.source_ref().is_displayed()
    }

    pub fn source(&self) -> LiveImageSource {
        self.source_ref().clone()
    }

    pub fn stats(&self) -> LiveImageSourceStats {
        self.source_ref().stats()
    }
}

impl std::fmt::Debug for LiveImageWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "LiveImageWriter(session {} of {:?})",
            self.token.session,
            self.source_ref()
        )
    }
}

/// `StrideTooSmall` or `BufferTooSmall` for a `width × height` source of
/// rows `stride` bytes apart that `src` cannot hold.
fn check_source(
    width: u32,
    height: u32,
    src: &[u8],
    stride: usize,
    bpp: usize,
) -> Result<(), LiveImageError> {
    let row = width as usize * bpp;
    if stride < row {
        return Err(LiveImageError::StrideTooSmall { stride, min: row });
    }
    if height == 0 || width == 0 {
        return Ok(());
    }
    let needed = (height as usize - 1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(row))
        .unwrap_or(usize::MAX);
    if src.len() < needed {
        return Err(LiveImageError::BufferTooSmall {
            len: src.len(),
            needed,
        });
    }
    Ok(())
}

impl LiveImageWriteGuard<'_> {
    fn state(&self) -> &super::source::State {
        self.guard
            .as_ref()
            .expect("open until commit or drop")
            .state()
    }

    fn state_mut(&mut self) -> &mut super::source::State {
        self.guard
            .as_mut()
            .expect("open until commit or drop")
            .state_mut()
    }

    fn bpp(&self) -> usize {
        self.format.bytes_per_pixel()
    }

    fn count_written(&self, bytes: u64) {
        if let Some(guard) = &self.guard {
            guard
                .shared()
                .counters
                .bytes_written
                .fetch_add(bytes, Ordering::Relaxed);
        }
    }

    /// `NoFrame` without a buffer, `OutOfBounds` for a rect outside it; the
    /// frame size otherwise.
    fn frame_for(&self, rect: PixelRect) -> Result<(u32, u32), LiveImageError> {
        let (width, height) = self.state().size.ok_or(LiveImageError::NoFrame)?;
        if !rect.fits_in(width, height) {
            return Err(LiveImageError::OutOfBounds {
                rect,
                width,
                height,
            });
        }
        Ok((width, height))
    }

    fn mark(&mut self, rect: PixelRect) {
        if rect.is_empty() {
            return;
        }
        let pending = &mut self.state_mut().pending;
        pending.damage.add(rect);
        pending.wrote = true;
    }

    /// The buffer's size; `None` before any.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.state().size
    }

    /// Bytes per row: `width × 4`, unless [`swap_frame`](LiveImageWriter::swap_frame)
    /// installed a wider stride.
    pub fn stride(&self) -> usize {
        self.state().stride
    }

    pub fn format(&self) -> LivePixelFormat {
        self.format
    }

    /// Zero-fill at a new size and mark everything dirty. A no-op at the
    /// current size. The next commit uploads the whole frame.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), LiveImageError> {
        let bpp = self.bpp();
        self.state_mut().resize(width, height, bpp)
    }

    /// Copy `rect` from `src`, whose row 0 is the rect's top row and whose
    /// rows are `src_stride` bytes apart. Marks `rect`.
    pub fn write_rect(
        &mut self,
        rect: PixelRect,
        src: &[u8],
        src_stride: usize,
    ) -> Result<(), LiveImageError> {
        self.frame_for(rect)?;
        let bpp = self.bpp();
        check_source(rect.width, rect.height, src, src_stride, bpp)?;
        if rect.is_empty() {
            return Ok(());
        }
        let row = rect.width as usize * bpp;
        let state = self.state_mut();
        let stride = state.stride;
        if src_stride == stride && rect.x == 0 && row == stride {
            let start = rect.y as usize * stride;
            let len = rect.height as usize * stride;
            state.pixels[start..start + len].copy_from_slice(&src[..len]);
        } else {
            for y in 0..rect.height as usize {
                let dst = (rect.y as usize + y) * stride + rect.x as usize * bpp;
                let from = y * src_stride;
                state.pixels[dst..dst + row].copy_from_slice(&src[from..from + row]);
            }
        }
        self.count_written((row * rect.height as usize) as u64);
        self.mark(rect);
        Ok(())
    }

    /// Move `from` to `(to_x, to_y)` inside the frame, overlap-safe (an RFB
    /// CopyRect). Marks the destination.
    pub fn copy_within(
        &mut self,
        from: PixelRect,
        to_x: u32,
        to_y: u32,
    ) -> Result<(), LiveImageError> {
        self.frame_for(from)?;
        let to = PixelRect::new(to_x, to_y, from.width, from.height);
        self.frame_for(to)?;
        if from.is_empty() {
            return Ok(());
        }
        let bpp = self.bpp();
        let row = from.width as usize * bpp;
        let state = self.state_mut();
        let stride = state.stride;
        let copy_row = |pixels: &mut Vec<u8>, y: usize| {
            let src = (from.y as usize + y) * stride + from.x as usize * bpp;
            let dst = (to.y as usize + y) * stride + to.x as usize * bpp;
            pixels.copy_within(src..src + row, dst);
        };
        // Rows in the order that never reads a row already overwritten.
        if to.y > from.y {
            for y in (0..from.height as usize).rev() {
                copy_row(&mut state.pixels, y);
            }
        } else {
            for y in 0..from.height as usize {
                copy_row(&mut state.pixels, y);
            }
        }
        self.count_written((row * from.height as usize) as u64);
        self.mark(to);
        Ok(())
    }

    /// Fill `rect` with `pixel`, in the source's byte order. Marks `rect`.
    pub fn fill_rect(&mut self, rect: PixelRect, pixel: [u8; 4]) -> Result<(), LiveImageError> {
        self.frame_for(rect)?;
        if rect.is_empty() {
            return Ok(());
        }
        let bpp = self.bpp();
        let state = self.state_mut();
        let stride = state.stride;
        for y in 0..rect.height as usize {
            let start = (rect.y as usize + y) * stride + rect.x as usize * bpp;
            for px in state.pixels[start..start + rect.width as usize * bpp].chunks_exact_mut(bpp) {
                px.copy_from_slice(&pixel[..bpp]);
            }
        }
        self.count_written(rect.area() * bpp as u64);
        self.mark(rect);
        Ok(())
    }

    /// The rows of `rect`, `rect.width × 4` bytes each, for cheap work in
    /// place. Marks `rect`. Heavy decoding belongs outside the transaction,
    /// into a buffer of the producer's own, copied in with
    /// [`write_rect`](Self::write_rect).
    pub fn rows_mut(&mut self, rect: PixelRect) -> Result<RowsMut<'_>, LiveImageError> {
        self.frame_for(rect)?;
        self.mark(rect);
        let bpp = self.bpp();
        let state = self.state_mut();
        let stride = state.stride;
        Ok(RowsMut {
            pixels: &mut state.pixels,
            stride,
            rect,
            bpp,
        })
    }

    /// The whole buffer, `stride` bytes per row; empty before any frame.
    /// Writes through it mark nothing: only regions marked with
    /// [`mark_dirty`](Self::mark_dirty) reach the screen with this commit.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.state_mut().pixels
    }

    /// Mark `rect`, clamped to the frame. An empty rect, or one outside the
    /// frame, marks nothing.
    pub fn mark_dirty(&mut self, rect: PixelRect) {
        let Some((width, height)) = self.state().size else {
            return;
        };
        if let Some(clamped) = rect.intersect(&PixelRect::full(width, height)) {
            self.mark(clamped);
        }
    }

    /// Mark the whole frame.
    pub fn mark_all_dirty(&mut self) {
        if let Some((width, height)) = self.state().size {
            self.mark(PixelRect::full(width, height));
            self.state_mut().pending.force_full = true;
        }
    }

    /// Publish the transaction: the new generation, or the current one when
    /// nothing was marked or resized, here or in a transaction abandoned
    /// since the last commit. An empty commit wakes nobody.
    pub fn commit(mut self) -> u64 {
        self.finish(true)
    }

    fn finish(&mut self, commit: bool) -> u64 {
        let Some(mut guard) = self.guard.take() else {
            return 0;
        };
        let shared = guard.shared();
        let state = guard.state_mut();
        let published = if commit {
            state.publish(Instant::now())
        } else {
            if std::thread::panicking() {
                state.pending.force_full = true;
            }
            if self.counted {
                shared.counters.abandoned.fetch_add(1, Ordering::Relaxed);
            }
            None
        };
        for session in held::leave(shared.id) {
            shared.apply_release(guard.state_mut(), session);
        }
        if let Some(generation) = published {
            shared.publish_generation(generation);
        }
        let meta = shared.publish_meta(guard.state());
        let generation = guard.state().generation;
        drop(guard);
        shared.notify(published.is_some(), meta);
        generation
    }
}

impl Drop for LiveImageWriteGuard<'_> {
    /// Abandon: publish nothing, keep the damage pending for the next commit.
    /// A size the transaction changed still reaches layout, since the buffer
    /// has that size now.
    fn drop(&mut self) {
        self.finish(false);
    }
}

impl std::fmt::Debug for LiveImageWriteGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveImageWriteGuard")
            .field("size", &self.size())
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

/// The rows of a rect, for in-place work inside a transaction.
pub struct RowsMut<'a> {
    pixels: &'a mut [u8],
    stride: usize,
    rect: PixelRect,
    bpp: usize,
}

impl RowsMut<'_> {
    pub fn width(&self) -> u32 {
        self.rect.width
    }

    pub fn height(&self) -> u32 {
        self.rect.height
    }

    /// Row `y` of the rect, `width × 4` bytes.
    ///
    /// # Panics
    ///
    /// When `y` is not below [`height`](Self::height).
    pub fn row_mut(&mut self, y: u32) -> &mut [u8] {
        assert!(
            y < self.rect.height,
            "row {y} of a rect {} rows tall",
            self.rect.height
        );
        let start = (self.rect.y + y) as usize * self.stride + self.rect.x as usize * self.bpp;
        &mut self.pixels[start..start + self.rect.width as usize * self.bpp]
    }
}
