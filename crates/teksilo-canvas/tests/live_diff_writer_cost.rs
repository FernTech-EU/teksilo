// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What `LiveImageDiffWriter::write_frame` costs the producer for a
//! 720 × 1280 frame, printed rather than asserted: an identical frame (the
//! comparison alone), a frame with one small change, and a plain
//! `LiveImageWriter::write_frame` of the same frame for scale. Run it in an
//! optimized build:
//!
//! ```sh
//! cargo test -p teksilo-canvas --release --test live_diff_writer_cost -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use teksilo_canvas::live_image::{LiveImageDiffWriter, LiveImageSource, LivePixelFormat};

const W: u32 = 720;
const H: u32 = 1280;
const ROUNDS: usize = 101;

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

#[test]
#[ignore = "a measurement, printed: run it in an optimized build"]
fn measure_the_diff_writer() {
    let stride = (W * 4) as usize;
    let mut frame: Vec<u8> = (0..W * H * 4).map(|i| (i % 251) as u8).collect();

    let source = LiveImageSource::new(LivePixelFormat::Bgrx8);
    let mut diff = LiveImageDiffWriter::new(source.writer());
    diff.write_frame(W, H, &frame, stride).unwrap();
    let identical: Vec<Duration> = (0..ROUNDS)
        .map(|_| {
            let start = Instant::now();
            assert_eq!(diff.write_frame(W, H, &frame, stride).unwrap(), None);
            start.elapsed()
        })
        .collect();
    let changed: Vec<Duration> = (0..ROUNDS)
        .map(|i| {
            // A 64 × 64 square moved by one pixel, as a cursor or a ball.
            let x = i % 600;
            for y in 300..364 {
                frame[y * stride + x * 4..][..64 * 4].fill(i as u8);
            }
            let start = Instant::now();
            assert!(diff.write_frame(W, H, &frame, stride).unwrap().is_some());
            start.elapsed()
        })
        .collect();

    let plain = source.writer();
    let whole: Vec<Duration> = (0..ROUNDS)
        .map(|_| {
            let start = Instant::now();
            plain.write_frame(W, H, &frame, stride).unwrap();
            start.elapsed()
        })
        .collect();

    println!(
        "720x1280: identical frame {:?}, a 64x64 change {:?}, plain write_frame {:?} (medians of {ROUNDS})",
        median(identical),
        median(changed),
        median(whole),
    );
}
