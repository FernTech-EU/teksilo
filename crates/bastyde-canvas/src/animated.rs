// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Animated icon decoding: animated WebP → frame sequence.

use std::time::Duration;

use crate::raster::{ImageDecodeError, RasterIcon};

/// An animated icon: a sequence of frames with per-frame timing.
#[derive(Debug, Clone)]
pub struct AnimatedIcon {
    frames: Vec<RasterIcon>,
    durations: Vec<Duration>,
    total_duration: Duration,
}

impl AnimatedIcon {
    /// Decode an animated WebP from raw bytes.
    ///
    /// Returns `Err` if the data is not a valid animated WebP (use
    /// [`RasterIcon::decode_webp`] for static WebP images).
    pub fn decode_webp(data: &[u8]) -> Result<Self, ImageDecodeError> {
        let decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(data))
            .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;

        if !decoder.is_animated() {
            return Err(ImageDecodeError::InvalidData(
                "WebP is not animated (use RasterIcon::decode_webp for static images)".into(),
            ));
        }

        let (width, height) = decoder.dimensions();
        if width == 0 || height == 0 {
            return Err(ImageDecodeError::EmptyImage);
        }

        let num_frames = decoder.num_frames() as usize;
        let buf_size = decoder
            .output_buffer_size()
            .unwrap_or((width * height * 4) as usize);

        let mut frames = Vec::with_capacity(num_frames);
        let mut durations = Vec::with_capacity(num_frames);
        let mut total = Duration::ZERO;
        let mut decoder = decoder;

        for _ in 0..num_frames {
            let mut buf = vec![0u8; buf_size];
            let duration_ms = decoder
                .read_frame(&mut buf)
                .map_err(|e| ImageDecodeError::InvalidData(e.to_string()))?;

            frames.push(RasterIcon::from_raw(buf, width, height));
            let dur = Duration::from_millis(duration_ms.max(1) as u64);
            durations.push(dur);
            total += dur;
        }

        if frames.is_empty() {
            return Err(ImageDecodeError::InvalidData(
                "animated WebP contained no frames".into(),
            ));
        }

        Ok(Self {
            frames,
            durations,
            total_duration: total,
        })
    }

    /// Number of frames in the animation.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Total animation duration.
    pub fn total_duration(&self) -> Duration {
        self.total_duration
    }

    /// Get the frame for a given elapsed time (loops automatically).
    pub fn frame_at(&self, elapsed: Duration) -> &RasterIcon {
        if self.frames.len() == 1 || self.total_duration.is_zero() {
            return &self.frames[0];
        }
        let elapsed_ms = elapsed.as_millis() % self.total_duration.as_millis();
        let mut acc = 0u128;
        for (i, dur) in self.durations.iter().enumerate() {
            acc += dur.as_millis();
            if elapsed_ms < acc {
                return &self.frames[i];
            }
        }
        self.frames
            .last()
            .expect("AnimatedIcon constructor enforces frames.len() >= 1")
    }

    /// Access all frames.
    pub fn frames(&self) -> &[RasterIcon] {
        &self.frames
    }

    /// Access per-frame durations.
    pub fn frame_durations(&self) -> &[Duration] {
        &self.durations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_at_loops() {
        let frame_a = RasterIcon::from_raw(vec![255; 4], 1, 1);
        let frame_b = RasterIcon::from_raw(vec![0; 4], 1, 1);
        let icon = AnimatedIcon {
            frames: vec![frame_a, frame_b],
            durations: vec![Duration::from_millis(100), Duration::from_millis(100)],
            total_duration: Duration::from_millis(200),
        };

        // First frame at t=0
        assert_eq!(icon.frame_at(Duration::ZERO).pixels()[0], 255);
        // Second frame at t=150ms
        assert_eq!(icon.frame_at(Duration::from_millis(150)).pixels()[0], 0);
        // Loops: t=250ms → same as t=50ms → first frame
        assert_eq!(icon.frame_at(Duration::from_millis(250)).pixels()[0], 255);
    }

    #[test]
    fn single_frame_always_returns_same() {
        let frame = RasterIcon::from_raw(vec![128; 4], 1, 1);
        let icon = AnimatedIcon {
            frames: vec![frame],
            durations: vec![Duration::from_millis(100)],
            total_duration: Duration::from_millis(100),
        };
        assert_eq!(icon.frame_at(Duration::from_secs(999)).pixels()[0], 128);
    }

    #[test]
    fn decode_invalid_returns_error() {
        let result = AnimatedIcon::decode_webp(b"not a webp");
        assert!(result.is_err());
    }
}
