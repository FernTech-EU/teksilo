// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The mip chain of a live texture drawn with `ScalingFilter::Trilinear`:
//! its levels, their sizes, and which texels of each a changed rect of
//! level 0 reaches. Both backends build the chain with
//! [`crate::resample::downsample_half`]'s 2×2 box, level from level, so the
//! GPU's chain and the mirror's agree.

use super::PixelRect;

/// The levels of a full chain for a `width × height` texture, level 0
/// included: down to 1×1.
pub fn mip_levels(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// The size of level `k`: each side halved `k` times, rounding down, never
/// below 1.
pub fn mip_level_size(width: u32, height: u32, k: u32) -> (u32, u32) {
    let halve = |side: u32| side.checked_shr(k).unwrap_or(0).max(1);
    (halve(width), halve(height))
}

/// The texels of level `k` (of size `level`) whose 2×2 inputs, followed
/// down from level 0, touch `changed`. Level `k` reads only level `k - 1`,
/// and the floors and ceilings of the halvings compose, so rebuilding these
/// gives what rebuilding the whole level gives.
pub fn mip_footprint(changed: PixelRect, k: u32, level: (u32, u32)) -> PixelRect {
    let shift = |v: u32| v.checked_shr(k).unwrap_or(0);
    let up = |v: u32| {
        let unit = 1u64 << k.min(63);
        u64::from(v).div_ceil(unit).min(u64::from(u32::MAX)) as u32
    };
    let x0 = shift(changed.x).min(level.0 - 1);
    let y0 = shift(changed.y).min(level.1 - 1);
    let x1 = up(changed.x.saturating_add(changed.width)).clamp(x0 + 1, level.0);
    let y1 = up(changed.y.saturating_add(changed.height)).clamp(y0 + 1, level.1);
    PixelRect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Bytes a chain of `levels` takes for a `width × height` texture, four per
/// texel.
pub fn mip_bytes(width: u32, height: u32, levels: u32) -> u64 {
    (0..levels)
        .map(|k| {
            let (w, h) = mip_level_size(width, height, k);
            u64::from(w) * u64::from(h) * 4
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_goes_down_to_one_texel() {
        assert_eq!(mip_levels(1, 1), 1);
        assert_eq!(mip_levels(2, 1), 2);
        assert_eq!(mip_levels(720, 1280), 11);
        assert_eq!(mip_levels(1920, 1080), 11);
        assert_eq!(mip_levels(3840, 2160), 12);
        assert_eq!(mip_level_size(720, 1280, 10), (1, 1));
        assert_eq!(mip_level_size(5, 3, 1), (2, 1));
        assert_eq!(mip_level_size(5, 3, 2), (1, 1));
    }

    #[test]
    fn a_chain_adds_a_third() {
        // The spec's measurement: 1.333 times level 0 at all three sizes.
        for (w, h) in [(720u32, 1280u32), (1920, 1080), (3840, 2160)] {
            let ratio = mip_bytes(w, h, mip_levels(w, h)) as f64 / (w as f64 * h as f64 * 4.0);
            assert!((ratio - 4.0 / 3.0).abs() < 0.001, "{w}x{h}: {ratio}");
        }
    }

    #[test]
    fn a_footprint_covers_what_its_rect_reaches_and_stays_in_the_level() {
        // Level 1 texel x reads level 0 columns 2x and 2x + 1.
        assert_eq!(
            mip_footprint(PixelRect::new(3, 0, 1, 1), 1, (8, 8)),
            PixelRect::new(1, 0, 1, 1)
        );
        assert_eq!(
            mip_footprint(PixelRect::new(3, 2, 3, 3), 1, (8, 8)),
            PixelRect::new(1, 1, 2, 2)
        );
        // Deep levels collapse to one texel, never outside the level.
        assert_eq!(
            mip_footprint(PixelRect::new(700, 1270, 20, 10), 10, (1, 1)),
            PixelRect::new(0, 0, 1, 1)
        );
        // The last column of an odd level 0, which no level-1 texel reads,
        // is kept inside the level.
        assert_eq!(
            mip_footprint(PixelRect::new(4, 0, 1, 1), 1, (2, 1)),
            PixelRect::new(1, 0, 1, 1)
        );
    }
}
