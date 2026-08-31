// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Raster decoding: PNG, JPEG and static WebP → RGBA pixel data.
//!
//! Three formats, one output contract: straight-alpha (non-premultiplied) RGBA8,
//! row-major, `width * height * 4` bytes. Callers downstream — the texture
//! atlas, the mip builder, the alpha-mask path — all assume that shape, so each
//! decoder normalises into it rather than exposing its format's native layout.
//!
//! [`RasterIcon::decode`] sniffs the format from the leading magic bytes and
//! dispatches. Prefer it over the per-format entry points for anything the user
//! supplied: a file's extension is a claim, not evidence, and a `.png` that is
//! really a JPEG is common enough to be worth being immune to.

use crate::exif::{apply_orientation, orientation_from_exif};

/// Error type for image decoding failures.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ImageDecodeError {
    /// The image data is malformed or unsupported.
    #[error("image decode error: {0}")]
    InvalidData(String),
    /// The image has zero dimensions.
    #[error("image has zero dimensions")]
    EmptyImage,
    /// The leading bytes match no format this crate can decode.
    #[error("unsupported image format (supported: PNG, JPEG, WebP)")]
    UnsupportedFormat,
}

/// A raster format this crate can decode, as identified from magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG, including palette and 16-bit variants.
    Png,
    /// Baseline or progressive JPEG.
    Jpeg,
    /// Static WebP. Animated WebP is [`crate::AnimatedIcon`]'s job.
    Webp,
}

impl ImageFormat {
    /// The IANA media type, for callers that must record or transmit one.
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    /// The conventional lowercase file extension, without a dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    /// Identify the format from leading magic bytes, ignoring any filename.
    pub fn sniff(data: &[u8]) -> Option<Self> {
        if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
            return Some(Self::Png);
        }
        // Every JPEG variant opens with SOI (FFD8) followed by a marker.
        if data.starts_with(&[0xff, 0xd8, 0xff]) {
            return Some(Self::Jpeg);
        }
        // RIFF container whose form type is WEBP: bytes 0..4 and 8..12.
        if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
            return Some(Self::Webp);
        }
        None
    }
}

/// A decoded raster icon: RGBA pixel data at a fixed size.
#[derive(Debug, Clone)]
pub struct RasterIcon {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl RasterIcon {
    /// Decode an image, identifying the format from its magic bytes.
    ///
    /// This is the entry point for user-supplied data. It never consults a
    /// filename: extensions are frequently wrong, and a mislabelled file should
    /// open rather than fail. An unrecognised format yields
    /// [`ImageDecodeError::UnsupportedFormat`], whose message names what *is*
    /// supported so the error can be shown to a user unchanged.
    pub fn decode(data: &[u8]) -> Result<Self, ImageDecodeError> {
        match ImageFormat::sniff(data).ok_or(ImageDecodeError::UnsupportedFormat)? {
            ImageFormat::Png => Self::decode_png(data),
            ImageFormat::Jpeg => Self::decode_jpeg(data),
            ImageFormat::Webp => Self::decode_webp(data),
        }
    }

    /// Decode a PNG image from raw bytes.
    ///
    /// Palette, grayscale, `tRNS`-keyed and 16-bit PNGs are all normalised to
    /// 8-bit colour before the channel expansion below, so the match on
    /// `color_type` only ever sees the four direct forms.
    pub fn decode_png(data: &[u8]) -> Result<Self, ImageDecodeError> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
        // Palette PNGs are extremely common (every screenshot tool and every
        // "save for web" path emits them) and 16-bit ones are not rare either.
        // Without this the former was rejected outright and the latter decoded
        // as if it were 8-bit, silently halving the image and shredding it.
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder
            .read_info()
            .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;
        let buffer_size = reader.output_buffer_size().ok_or_else(|| {
            ImageDecodeError::InvalidData("PNG output buffer size unavailable".into())
        })?;
        let mut buf = vec![0u8; buffer_size];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;
        buf.truncate(info.buffer_size());
        let width = info.width;
        let height = info.height;
        if width == 0 || height == 0 {
            return Err(ImageDecodeError::EmptyImage);
        }
        // Convert to RGBA if needed
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => {
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for chunk in buf.chunks(3) {
                    rgba.extend_from_slice(chunk);
                    rgba.push(255);
                }
                rgba
            }
            png::ColorType::GrayscaleAlpha => {
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for chunk in buf.chunks(2) {
                    let g = chunk[0];
                    let a = chunk[1];
                    rgba.extend_from_slice(&[g, g, g, a]);
                }
                rgba
            }
            png::ColorType::Grayscale => {
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for &g in &buf {
                    rgba.extend_from_slice(&[g, g, g, 255]);
                }
                rgba
            }
            // `normalize_to_color8` expands the palette before we get here, so
            // this arm is unreachable in practice; treat it as malformed rather
            // than panicking if a future png release changes that.
            png::ColorType::Indexed => {
                return Err(ImageDecodeError::InvalidData(
                    "palette PNG was not expanded by the decoder".into(),
                ));
            }
        };
        Ok(Self {
            pixels: rgba,
            width,
            height,
        })
    }

    /// Decode a JPEG image from raw bytes, honouring its EXIF orientation.
    ///
    /// JPEG has no alpha channel, so every pixel comes back fully opaque. The
    /// orientation tag *is* applied here rather than being reported to the
    /// caller: a decoded buffer that still needs an out-of-band rotation is a
    /// trap, since every consumer would have to remember to ask.
    pub fn decode_jpeg(data: &[u8]) -> Result<Self, ImageDecodeError> {
        use zune_core::bytestream::ZCursor;
        use zune_core::colorspace::ColorSpace;
        use zune_core::options::DecoderOptions;

        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
        let mut pixels = decoder
            .decode()
            .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;
        // JPEG has no alpha channel, so the fourth byte is ours to define and
        // must be fully opaque. This is not belt-and-braces: for a 4-component
        // (CMYK/YCCK) JPEG — what Adobe and most print workflows emit — the
        // decoder's RGBA path leaves a colour channel sitting in the alpha
        // slot, and the photo composites semi-transparently over the page.
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let info = decoder
            .info()
            .ok_or_else(|| ImageDecodeError::InvalidData("JPEG headers not decoded".into()))?;

        let width = u32::from(info.width);
        let height = u32::from(info.height);
        if width == 0 || height == 0 {
            return Err(ImageDecodeError::EmptyImage);
        }
        if pixels.len() < (width as usize) * (height as usize) * 4 {
            return Err(ImageDecodeError::InvalidData(
                "JPEG decoded to fewer pixels than its declared size".into(),
            ));
        }

        let orientation = info
            .exif_data
            .as_deref()
            .map(orientation_from_exif)
            .unwrap_or_default();
        let (pixels, width, height) = apply_orientation(pixels, width, height, orientation);

        Ok(Self {
            pixels,
            width,
            height,
        })
    }

    /// Decode a static WebP image from raw bytes.
    pub fn decode_webp(data: &[u8]) -> Result<Self, ImageDecodeError> {
        let decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(data))
            .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;

        let (width, height) = decoder.dimensions();
        if width == 0 || height == 0 {
            return Err(ImageDecodeError::EmptyImage);
        }

        let buf_size = decoder
            .output_buffer_size()
            .unwrap_or((width * height * 4) as usize);
        let mut buf = vec![0u8; buf_size];
        let mut decoder = decoder;
        decoder
            .read_image(&mut buf)
            .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;

        // The output is always RGBA when output_buffer_size indicates 4 bytes/pixel.
        // If the buffer is smaller (RGB), expand to RGBA.
        let expected_rgba = (width * height * 4) as usize;
        if buf.len() < expected_rgba {
            let mut rgba = Vec::with_capacity(expected_rgba);
            for chunk in buf.chunks(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            buf = rgba;
        }

        Ok(Self {
            pixels: buf,
            width,
            height,
        })
    }

    /// Convert to an alpha mask for tintable rendering.
    /// The alpha channel is computed from luminance: `alpha = lum * original_alpha`.
    /// RGB is set to white (255, 255, 255) so the shader's monochrome
    /// path (`vertex.rgb * tex.a`) produces the tint color correctly.
    pub fn to_alpha_mask(&self) -> Self {
        let mut mask = Vec::with_capacity(self.pixels.len());
        for chunk in self.pixels.chunks(4) {
            let r = chunk[0] as f32 / 255.0;
            let g = chunk[1] as f32 / 255.0;
            let b = chunk[2] as f32 / 255.0;
            let a = chunk[3] as f32 / 255.0;
            // sRGB luminance
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let alpha = (lum * a * 255.0) as u8;
            mask.extend_from_slice(&[255, 255, 255, alpha]);
        }
        Self {
            pixels: mask,
            width: self.width,
            height: self.height,
        }
    }

    /// Scale down so neither side exceeds `max_edge`, preserving aspect ratio.
    ///
    /// Returns `None` when the image already fits, so a caller can keep the
    /// original buffer without copying it. Upscaling is never performed: this
    /// exists to bound work and memory, and enlarging would do the opposite.
    ///
    /// The reduction runs as repeated halving down to within 2× of the target,
    /// then one area-average step to the exact size. Halving first is a large
    /// constant-factor win on big photographs — each pass reads a quarter of
    /// the pixels of the one before — and the final area step is what allows an
    /// arbitrary, non-power-of-two result.
    pub fn downsample_to_max(&self, max_edge: u32) -> Option<Self> {
        let (target_w, target_h) = crate::resample::fit_within(self.width, self.height, max_edge)?;

        let mut pixels = self.pixels.clone();
        let mut w = self.width;
        let mut h = self.height;
        // Halve while the next halving would still not undershoot the target.
        while w / 2 >= target_w && h / 2 >= target_h && w > 1 && h > 1 {
            let (nw, nh, next) = crate::resample::downsample_half(&pixels, w, h);
            pixels = next;
            w = nw;
            h = nh;
        }
        if (w, h) != (target_w, target_h) {
            pixels = crate::resample::resample_area(&pixels, w, h, target_w, target_h);
        }

        Some(Self {
            pixels,
            width: target_w,
            height: target_h,
        })
    }

    /// Create from pre-decoded RGBA pixel data.
    pub fn from_raw(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        Self {
            pixels,
            width,
            height,
        }
    }

    /// Image width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Image height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Raw RGBA pixel data.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid 1x1 white RGBA PNG (generated from spec).
    fn minimal_png_rgba() -> Vec<u8> {
        // Create a 1x1 white RGBA PNG in memory using the png crate
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 255, 255, 255]).unwrap();
        }
        buf
    }

    /// Minimal 2x2 RGBA PNG with varying alpha.
    fn test_png_2x2() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            #[rustfmt::skip]
            writer.write_image_data(&[
                255, 0, 0, 255,    // red, opaque
                0, 255, 0, 128,    // green, half-transparent
                0, 0, 255, 255,    // blue, opaque
                0, 0, 0, 0,        // black, transparent
            ]).unwrap();
        }
        buf
    }

    #[test]
    fn decode_png_1x1() {
        let data = minimal_png_rgba();
        let icon = RasterIcon::decode_png(&data).unwrap();
        assert_eq!(icon.width(), 1);
        assert_eq!(icon.height(), 1);
        assert_eq!(icon.pixels(), &[255, 255, 255, 255]);
    }

    #[test]
    fn decode_png_2x2() {
        let data = test_png_2x2();
        let icon = RasterIcon::decode_png(&data).unwrap();
        assert_eq!(icon.width(), 2);
        assert_eq!(icon.height(), 2);
        assert_eq!(icon.pixels().len(), 16);
    }

    #[test]
    fn decode_png_invalid() {
        let result = RasterIcon::decode_png(b"not a png");
        assert!(result.is_err());
    }

    #[test]
    fn to_alpha_mask_white_stays_opaque() {
        let icon = RasterIcon {
            pixels: vec![255, 255, 255, 255],
            width: 1,
            height: 1,
        };
        let mask = icon.to_alpha_mask();
        // White pixel → lum=1.0, alpha=255 → mask alpha=255
        assert_eq!(mask.pixels()[0], 255); // R = white
        assert_eq!(mask.pixels()[3], 255); // A = full
    }

    #[test]
    fn to_alpha_mask_black_becomes_transparent() {
        let icon = RasterIcon {
            pixels: vec![0, 0, 0, 255],
            width: 1,
            height: 1,
        };
        let mask = icon.to_alpha_mask();
        // Black pixel → lum=0, alpha=0
        assert_eq!(mask.pixels()[3], 0);
    }

    #[test]
    fn to_alpha_mask_preserves_dimensions() {
        let data = test_png_2x2();
        let icon = RasterIcon::decode_png(&data).unwrap();
        let mask = icon.to_alpha_mask();
        assert_eq!(mask.width(), 2);
        assert_eq!(mask.height(), 2);
        assert_eq!(mask.pixels().len(), 16);
    }
}
