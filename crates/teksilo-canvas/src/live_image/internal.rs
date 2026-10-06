// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The surface teksilo-core and teksilo-render reach the source through.
//! Not part of Teksilo's API: it changes in any minor release.

use std::time::Instant;

use super::LivePixelFormat;
use super::source::StateGuard;

pub use super::damage::{DamageRects, UploadPlan};
pub use super::meta::LiveMeta;
pub use super::pass::{
    BAND_BYTES, LivePass, LivePassMode, LiveTextureBackend, PARK_BUDGET, PARK_MAX_TEXTURE,
    PassCounts, QuadDecision, STAGING_BUDGET, TextureOutOfMemory,
};

/// The source, locked for a renderer to copy bytes out of. Nothing is
/// created while it is held; it unlocks on drop, and an unwinding panic
/// releases it too.
pub struct LiveImageRead<'a> {
    guard: StateGuard<'a>,
}

/// How a lock attempt for a read ended.
pub(crate) enum ReadAttempt<'a> {
    Locked(LiveImageRead<'a>),
    /// A producer holds the lock: the unlock that follows wakes the window.
    Busy,
}

impl<'a> LiveImageRead<'a> {
    pub(crate) fn new(guard: StateGuard<'a>) -> Self {
        Self { guard }
    }

    /// `None` when no buffer exists.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.guard.state().size
    }

    /// Bytes per row.
    pub fn stride(&self) -> usize {
        self.guard.state().stride
    }

    /// The whole buffer: `stride` bytes per row.
    pub fn pixels(&self) -> &[u8] {
        &self.guard.state().pixels
    }

    pub fn format(&self) -> LivePixelFormat {
        self.guard.shared().format
    }

    /// The generation the buffer holds.
    pub fn generation(&self) -> u64 {
        self.guard.state().generation
    }

    /// What a texture holding generation `cursor` uploads to hold this one;
    /// `None` means a new texture.
    pub fn plan(&self, cursor: Option<u64>) -> UploadPlan {
        let state = self.guard.state();
        let (w, h) = state.size.unwrap_or((0, 0));
        state.ring.plan(cursor, state.generation, w, h)
    }

    /// When commit `generation` was made, while the log still holds it.
    pub fn commit_instant(&self, generation: u64) -> Option<Instant> {
        self.guard.state().ring.commit_instant(generation)
    }
}

impl std::fmt::Debug for LiveImageRead<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveImageRead")
            .field("size", &self.size())
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}
