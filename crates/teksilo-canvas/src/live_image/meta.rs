// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A source's status and size in one atomic word, so the layout pre-pass
//! reads both with one load and no lock: 2 bits of status, 15 of width and
//! 15 of height. A side of 0 means no size; a live image's sides are at
//! least 1 and at most [`MAX_DIMENSION`](super::LiveImageSource::MAX_DIMENSION).

use super::{LiveImageSource, LiveImageStatus};

const STATUS_BITS: u32 = 2;
const SIDE_BITS: u32 = 15;
const SIDE_MASK: u32 = (1 << SIDE_BITS) - 1;

const _: () = assert!(
    LiveImageSource::MAX_DIMENSION <= SIDE_MASK,
    "a side must fit in its 15 bits"
);
const _: () = assert!(STATUS_BITS + 2 * SIDE_BITS <= 32);

/// What the packed meta word holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveMeta {
    pub status: LiveImageStatus,
    /// The buffer's size, else the size hint; `None` without either.
    pub size: Option<(u32, u32)>,
}

impl LiveMeta {
    pub(crate) fn pack(self) -> u32 {
        let status = match self.status {
            LiveImageStatus::Disconnected => 0,
            LiveImageStatus::Waiting => 1,
            LiveImageStatus::Live => 2,
        };
        let (w, h) = self.size.unwrap_or((0, 0));
        debug_assert!(w <= SIDE_MASK && h <= SIDE_MASK, "{w}x{h} outside the word");
        status | (w & SIDE_MASK) << STATUS_BITS | (h & SIDE_MASK) << (STATUS_BITS + SIDE_BITS)
    }

    pub(crate) fn unpack(word: u32) -> Self {
        let status = match word & ((1 << STATUS_BITS) - 1) {
            1 => LiveImageStatus::Waiting,
            2 => LiveImageStatus::Live,
            _ => LiveImageStatus::Disconnected,
        };
        let w = word >> STATUS_BITS & SIDE_MASK;
        let h = word >> (STATUS_BITS + SIDE_BITS) & SIDE_MASK;
        Self {
            status,
            size: (w != 0 && h != 0).then_some((w, h)),
        }
    }
}

#[cfg(all(test, not(teksilo_loom)))]
mod tests {
    use super::*;

    #[test]
    fn every_status_and_size_round_trips() {
        let max = LiveImageSource::MAX_DIMENSION;
        for status in [
            LiveImageStatus::Disconnected,
            LiveImageStatus::Waiting,
            LiveImageStatus::Live,
        ] {
            for size in [
                None,
                Some((1, 1)),
                Some((720, 1280)),
                Some((max, 1)),
                Some((max, max)),
            ] {
                let meta = LiveMeta { status, size };
                assert_eq!(LiveMeta::unpack(meta.pack()), meta);
            }
        }
    }
}
