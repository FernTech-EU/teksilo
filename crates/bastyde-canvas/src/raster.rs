// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Raster icon decoding: PNG and static WebP → RGBA pixel data.

/// Error type for image decoding failures.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ImageDecodeError {
    /// The image data is malformed or unsupported.
    #[error("image decode error: {0}")]
    InvalidData(String),
    /// The image has zero dimensions.
    #[error("image has zero dimensions")]
    EmptyImage,
}

/// A decoded raster icon: RGBA pixel data at a fixed size.
#[derive(Debug, Clone)]
pub struct RasterIcon {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl RasterIcon {
    /// Decode a PNG image from raw bytes.
    pub fn decode_png(data: &[u8]) -> Result<Self, ImageDecodeError> {
        let decoder = png::Decoder::new(std::io::Cursor::new(data));
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
            png::ColorType::Indexed => {
                return Err(ImageDecodeError::InvalidData(
                    "indexed/palette PNG not supported; re-export as RGBA".into(),
                ));
            }
        };
        Ok(Self {
            pixels: rgba,
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
