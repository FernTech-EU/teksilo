// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! EXIF orientation: the one metadata tag a decoder cannot ignore.
//!
//! A phone camera's sensor has a fixed physical orientation. Rotating the phone
//! does not rotate the sensor, so the JPEG it writes always holds the pixels in
//! sensor order and records how the camera was held in EXIF tag `0x0112`
//! (`Orientation`). A decoder that returns the raw pixel grid and drops the tag
//! shows a large fraction of real-world photographs on their side.
//!
//! This module reads that single tag out of an EXIF/TIFF block and applies the
//! corresponding transform to an RGBA8 buffer. It deliberately parses nothing
//! else: the full EXIF specification is a large surface, and every other tag in
//! it is presentation metadata a renderer does not need.
//!
//! The eight values are the TIFF standard's, and four of them mirror as well as
//! rotate — a scanned original fed through some workflows really can be
//! flipped, so the mirroring cases are implemented rather than treated as
//! nonsense.

pub use crate::image_geometry::ImageOrientation;

/// The orientation an EXIF block names.
#[deprecated(since = "0.16.0", note = "renamed `ImageOrientation`")]
pub type Orientation = ImageOrientation;

/// Read the `Orientation` tag out of an EXIF block.
///
/// `data` is the payload of a JPEG `APP1` segment, with or without the leading
/// `"Exif\0\0"` marker — `zune-jpeg` hands over the block either way depending
/// on the file, so both are accepted. Returns [`ImageOrientation::Normal`] for any
/// input this parser cannot make sense of; a photograph displayed unrotated is
/// a far better failure than a parse error propagating out of image decoding.
pub fn orientation_from_exif(data: &[u8]) -> ImageOrientation {
    parse_orientation(data).map_or(ImageOrientation::Normal, ImageOrientation::from_tiff)
}

/// The fallible core, kept separate so every bail-out is a plain `?`.
fn parse_orientation(data: &[u8]) -> Option<u16> {
    // Some producers include the "Exif\0\0" preamble ahead of the TIFF header.
    let tiff = match data.get(..6) {
        Some(b"Exif\0\0") => data.get(6..)?,
        _ => data,
    };

    // TIFF header: byte order, magic 42, then the offset of the first IFD.
    // Every offset below is relative to the start of this header, not to the
    // start of the file — the single easiest thing to get wrong here.
    let big_endian = match tiff.get(..2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let u16_at = |off: usize| -> Option<u16> {
        let b = tiff.get(off..off + 2)?;
        Some(if big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    };
    let u32_at = |off: usize| -> Option<u32> {
        let b = tiff.get(off..off + 4)?;
        Some(if big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    };

    if u16_at(2)? != 42 {
        return None;
    }
    let ifd0 = u32_at(4)? as usize;
    let entry_count = u16_at(ifd0)? as usize;

    // Each IFD entry is 12 bytes: tag, type, count, then 4 bytes that hold the
    // value inline when it fits and an offset when it does not. Orientation is
    // a single SHORT, so it always fits inline — and, because the field is
    // 4 bytes wide but the value is 2, it sits in the *first* two bytes on
    // big-endian and equally in the first two on little-endian by construction
    // of `u16_at`.
    for i in 0..entry_count {
        let entry = ifd0 + 2 + i * 12;
        if u16_at(entry)? == 0x0112 {
            return u16_at(entry + 8);
        }
    }
    None
}

/// Apply `orientation` to a straight-alpha RGBA8 buffer.
///
/// Returns the transformed pixels together with their new dimensions, which are
/// swapped for the four rotating/transposing cases. `Normal` returns the input
/// untouched so the common path costs nothing.
pub fn apply_orientation(
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    orientation: ImageOrientation,
) -> (Vec<u8>, u32, u32) {
    if orientation.is_identity() {
        return (pixels, width, height);
    }
    let (w, h) = (width as usize, height as usize);
    if pixels.len() < w * h * 4 {
        return (pixels, width, height);
    }

    let (dst_w, dst_h) = if orientation.swaps_axes() {
        (h, w)
    } else {
        (w, h)
    };
    let mut out = vec![0u8; dst_w * dst_h * 4];

    for y in 0..h {
        for x in 0..w {
            // Where the source texel lands in the display-oriented image.
            let (dx, dy) = match orientation {
                ImageOrientation::Normal => (x, y),
                ImageOrientation::FlipHorizontal => (w - 1 - x, y),
                ImageOrientation::Rotate180 => (w - 1 - x, h - 1 - y),
                ImageOrientation::FlipVertical => (x, h - 1 - y),
                ImageOrientation::Transpose => (y, x),
                ImageOrientation::Rotate90 => (h - 1 - y, x),
                ImageOrientation::Transverse => (h - 1 - y, w - 1 - x),
                ImageOrientation::Rotate270 => (y, w - 1 - x),
            };
            let src = (y * w + x) * 4;
            let dst = (dy * dst_w + dx) * 4;
            out[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }

    (out, dst_w as u32, dst_h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal little-endian EXIF block carrying one Orientation tag.
    fn exif_le(value: u16) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"Exif\0\0");
        v.extend_from_slice(b"II"); // little endian
        v.extend_from_slice(&42u16.to_le_bytes());
        v.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8
        v.extend_from_slice(&1u16.to_le_bytes()); // one entry
        v.extend_from_slice(&0x0112u16.to_le_bytes()); // tag
        v.extend_from_slice(&3u16.to_le_bytes()); // type SHORT
        v.extend_from_slice(&1u32.to_le_bytes()); // count
        v.extend_from_slice(&value.to_le_bytes());
        v.extend_from_slice(&[0, 0]); // pad the 4-byte value field
        v
    }

    /// The same, big-endian, and without the "Exif\0\0" preamble.
    fn exif_be_bare(value: u16) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"MM");
        v.extend_from_slice(&42u16.to_be_bytes());
        v.extend_from_slice(&8u32.to_be_bytes());
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&0x0112u16.to_be_bytes());
        v.extend_from_slice(&3u16.to_be_bytes());
        v.extend_from_slice(&1u32.to_be_bytes());
        v.extend_from_slice(&value.to_be_bytes());
        v.extend_from_slice(&[0, 0]);
        v
    }

    #[test]
    fn reads_little_endian_orientation() {
        assert_eq!(
            orientation_from_exif(&exif_le(6)),
            ImageOrientation::Rotate90
        );
    }

    #[test]
    fn reads_big_endian_orientation_without_preamble() {
        assert_eq!(
            orientation_from_exif(&exif_be_bare(8)),
            ImageOrientation::Rotate270
        );
    }

    #[test]
    fn garbage_reads_as_normal() {
        assert_eq!(orientation_from_exif(b"not exif"), ImageOrientation::Normal);
        assert_eq!(orientation_from_exif(&[]), ImageOrientation::Normal);
        // A truncated block must not panic on the slice arithmetic.
        let full = exif_le(6);
        for cut in 0..full.len() {
            let _ = orientation_from_exif(&full[..cut]);
        }
    }

    #[test]
    fn out_of_range_value_reads_as_normal() {
        assert_eq!(orientation_from_exif(&exif_le(0)), ImageOrientation::Normal);
        assert_eq!(orientation_from_exif(&exif_le(9)), ImageOrientation::Normal);
    }

    /// A 2×1 image: left pixel red, right pixel green.
    fn two_by_one() -> Vec<u8> {
        vec![255, 0, 0, 255, 0, 255, 0, 255]
    }

    #[test]
    fn flip_horizontal_swaps_the_two_pixels() {
        let (out, w, h) = apply_orientation(two_by_one(), 2, 1, ImageOrientation::FlipHorizontal);
        assert_eq!((w, h), (2, 1));
        assert_eq!(&out[0..4], &[0, 255, 0, 255]);
        assert_eq!(&out[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn rotate90_swaps_dimensions_and_stacks_the_pixels() {
        let (out, w, h) = apply_orientation(two_by_one(), 2, 1, ImageOrientation::Rotate90);
        // A 2-wide, 1-tall strip rotated 90° clockwise is 1 wide and 2 tall.
        // Write "RG" on paper and turn it clockwise: it reads top-to-bottom,
        // so the originally-left (red) pixel ends up on *top*.
        assert_eq!((w, h), (1, 2));
        assert_eq!(&out[0..4], &[255, 0, 0, 255], "left pixel goes to the top");
        assert_eq!(&out[4..8], &[0, 255, 0, 255]);
    }

    #[test]
    fn rotate270_is_the_inverse_of_rotate90() {
        let src: Vec<u8> = (0..(3 * 2 * 4)).map(|i| i as u8).collect();
        let (px, w, h) = apply_orientation(src.clone(), 3, 2, ImageOrientation::Rotate90);
        let (back, bw, bh) = apply_orientation(px, w, h, ImageOrientation::Rotate270);
        assert_eq!((bw, bh), (3, 2));
        assert_eq!(back, src);
    }

    #[test]
    fn normal_is_a_passthrough() {
        let (out, w, h) = apply_orientation(two_by_one(), 2, 1, ImageOrientation::Normal);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, two_by_one());
    }

    #[test]
    fn every_orientation_preserves_the_pixel_count() {
        for v in 1..=8u16 {
            let o = ImageOrientation::from_tiff(v);
            let (out, w, h) = apply_orientation(vec![9u8; 6 * 4], 3, 2, o);
            assert_eq!(out.len(), 6 * 4, "value {v}");
            assert_eq!((w * h) as usize, 6, "value {v}");
            assert_eq!(o.swaps_axes(), w == 2, "value {v}");
        }
    }

    #[test]
    fn rotating_four_times_by_90_returns_the_original() {
        let src: Vec<u8> = (0..(3 * 2 * 4)).map(|i| i as u8).collect();
        let (mut px, mut w, mut h) = (src.clone(), 3u32, 2u32);
        for _ in 0..4 {
            let r = apply_orientation(px, w, h, ImageOrientation::Rotate90);
            px = r.0;
            w = r.1;
            h = r.2;
        }
        assert_eq!((w, h), (3, 2));
        assert_eq!(px, src);
    }
}
