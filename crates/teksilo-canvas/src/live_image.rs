// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Off-thread pixels: a raster another thread rewrites many times a second,
//! shown at display rate with flat GPU memory.
//!
//! A VM screen, a video frame, a camera preview: content that changes on a
//! thread of its own, faster than anything else in the window. A
//! [`LiveImageSource`] holds the latest frame. Producers write it through a
//! [`LiveImageWriter`], from any thread; the `LiveImage` widget shows it, in
//! any number of windows.
//!
//! - **Pixels stay in the source.** A frame a widget paints carries a
//!   reference to the source, never bytes, and each window's renderer pulls
//!   what it lacks under the source's lock, into a texture it reuses. A
//!   commit that changes only pixels marks no widget: no `paint()` runs, the
//!   window replays its cached frame, and only the changed rects are written
//!   to the texture.
//! - **Latest wins.** Nothing is queued. Each window uploads the union of
//!   the damage since its own last upload, so a producer faster than the
//!   display costs one upload per displayed frame, and a window that falls
//!   behind still shows a correct picture.
//! - **Each change wakes each window once.** A commit raises a flag per
//!   attached widget and wakes its window only on the clear-to-set edge;
//!   commits that follow merge into the wake already on its way until the
//!   window has read them. A window that is not drawing (hidden, or the
//!   widget scrolled away) costs one wake, then nothing. See
//!   [`crate::wake`].
//!
//! # Threading
//!
//! The source and its writers are `Send + Sync`. A write transaction
//! ([`LiveImageWriteGuard`]) holds the source's lock and is `!Send`; keep it
//! short and never hold it across an `.await`. A renderer takes the lock
//! only to copy bytes out, and never waits for a producer longer than about
//! one refresh: a frame that finds the lock held draws the previous picture,
//! and the producer's unlock wakes the window again.
//!
//! # Transactions
//!
//! [`LiveImageWriter::lock`], any number of writes, then
//! [`commit`](LiveImageWriteGuard::commit): one commit is one generation. A
//! transaction dropped without `commit()` publishes nothing; what it wrote
//! stays in the buffer and what it marked stays pending for the next commit.
//! Validate before writing when a half-applied update must never show: a
//! window that uploads the whole frame (a new texture) shows whatever the
//! buffer holds.
//!
//! # Sessions and status
//!
//! A source is [`Disconnected`](LiveImageStatus::Disconnected) until a writer
//! exists, [`Waiting`](LiveImageStatus::Waiting) until a commit writes
//! pixels, then [`Live`](LiveImageStatus::Live). When a session's last writer
//! drops, its pixels are freed and every window frees its texture.
//! [`LiveImageSource::writer_exclusive`] starts a new session and revokes
//! the old one's writers, for a stream that restarts.
//!
//! # Formats
//!
//! Four bytes per pixel, sRGB-encoded colour, straight alpha, in the byte
//! orders producers already have ([`LivePixelFormat`]); renderers handle
//! the byte order and the ignored alpha byte in the shader, so no producer
//! converts pixels.

mod consumer;
mod damage;
mod held;
#[doc(hidden)]
pub mod internal;
mod lock;
mod meta;
mod source;
mod stats;
mod writer;

pub use crate::image_geometry::{ImageOrientation, PixelRect};
pub use consumer::LiveImageConsumer;
pub use source::{LiveImageSource, LiveImageSourceBuilder};
pub use stats::{LiveImageAttachmentStats, LiveImageSourceStats, LiveImageStats};
pub use writer::{LiveImageWriteGuard, LiveImageWriter, RowsMut};

/// The byte order of one pixel: 4 bytes, row-major from the top-left
/// corner, sRGB-encoded colour, straight (not premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LivePixelFormat {
    /// R, G, B, A.
    #[default]
    Rgba8,
    /// B, G, R, A.
    Bgra8,
    /// R, G, B, X. The fourth byte is ignored: the picture is opaque.
    Rgbx8,
    /// B, G, R, X: RFB's 32 bits per pixel, little-endian, QEMU's default.
    /// Opaque.
    Bgrx8,
}

impl LivePixelFormat {
    /// Bytes one pixel takes: 4 for every format today.
    pub const fn bytes_per_pixel(self) -> usize {
        4
    }

    /// `true` for `Rgbx8` and `Bgrx8`, whose fourth byte is not alpha.
    pub const fn is_opaque(self) -> bool {
        matches!(self, Self::Rgbx8 | Self::Bgrx8)
    }

    /// `true` for `Bgra8` and `Bgrx8`, which store blue first.
    pub const fn is_bgr(self) -> bool {
        matches!(self, Self::Bgra8 | Self::Bgrx8)
    }
}

/// How a live picture is sampled when it is drawn at another size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ScalingFilter {
    /// Bilinear, from the full-size picture only. The sharpest choice from
    /// half size up.
    #[default]
    Linear,
    /// The nearest texel, from the full-size picture only: crisp integer
    /// upscaling, and exact at 1:1.
    Nearest,
    /// Trilinear, through a chain of downscaled copies the renderer rebuilds
    /// on the GPU after each upload. For pictures drawn below half size, such
    /// as thumbnails, where `Linear` aliases. Costs a third more texture
    /// memory and a small render pass per level on each upload; above half
    /// size it is softer than `Linear`.
    Trilinear,
}

/// What a source can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LiveImageStatus {
    /// No writer exists, and no pixels are held.
    #[default]
    Disconnected,
    /// A writer exists, but no pixels were written since it appeared, since
    /// [`LiveImageWriter::clear`] or since a new session started. A resize
    /// alone keeps this status.
    Waiting,
    /// At least one commit wrote pixels. A resize keeps this status.
    Live,
}

/// A process-unique source id: what a renderer keys its textures by.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LiveImageId(u64);

impl LiveImageId {
    fn next() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }

    /// The id as a number, for labels and logs.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Debug for LiveImageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl std::fmt::Display for LiveImageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Why a call was refused. A refused call changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LiveImageError {
    #[error("a live image cannot be {width}x{height}: both sides must be at least 1")]
    ZeroSize { width: u32, height: u32 },
    /// A side above [`LiveImageSource::MAX_DIMENSION`], or a frame above
    /// [`LiveImageSource::MAX_BYTES`].
    #[error(
        "a live image cannot be {width}x{height}: the largest side is {} pixels",
        LiveImageSource::MAX_DIMENSION
    )]
    TooLarge { width: u32, height: u32 },
    /// The buffer could not grow: the allocator refused `bytes`.
    #[error("no memory for a live image buffer of {bytes} bytes")]
    OutOfMemory { bytes: u64 },
    /// A pixel write before any size was set.
    #[error("the live image has no frame yet: resize it or write a whole frame first")]
    NoFrame,
    #[error("{rect:?} is not inside the {width}x{height} live image")]
    OutOfBounds {
        rect: PixelRect,
        width: u32,
        height: u32,
    },
    #[error("a stride of {stride} bytes is shorter than a row of {min}")]
    StrideTooSmall { stride: usize, min: usize },
    #[error("a buffer of {len} bytes is shorter than the {needed} the rows need")]
    BufferTooSmall { len: usize, needed: usize },
    /// `try_lock` only: a transaction or a renderer holds the lock.
    #[error("the live image is locked")]
    WouldBlock,
    /// A newer session started with `writer_exclusive()`: this writer can no
    /// longer write.
    #[error("this writer's session was revoked by a newer one")]
    Revoked,
}

/// A frame [`LiveImageWriter::swap_frame`] refused, and the buffer it was
/// handed, untouched.
#[derive(thiserror::Error)]
#[error("frame refused: {error}")]
pub struct RejectedFrame {
    pub error: LiveImageError,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for RejectedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RejectedFrame")
            .field("error", &self.error)
            .field("pixels", &format_args!("{} bytes", self.pixels.len()))
            .finish()
    }
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests;

#[cfg(all(test, teksilo_loom))]
mod loom_tests;
