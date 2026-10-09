// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property test for `LiveImageDiffWriter`
//! (crates/teksilo-canvas/src/live_image/writer/diff.rs): whatever frames a
//! producer hands over, and whatever another writer does to the source in
//! between, the window ends up showing exactly the last frame handed over,
//! and the writer commits nothing exactly when that frame is unchanged and
//! nothing visible touched the source since its last commit.
//!
//! The unit tests in `writer/diff/tests.rs` pin chosen cases; this
//! generalises them over sizes, edit shapes (one rect, scattered pixels,
//! nothing), resizes, and three kinds of interference: a commit, a clear,
//! and a transaction abandoned without one. Manual override knob:
//! `PROPTEST_CASES=N cargo test -p teksilo-canvas --test prop_live_diff_writer`.

use proptest::prelude::*;
use teksilo_canvas::live_image::testing::LiveImageMirror;
use teksilo_canvas::live_image::{
    LiveImageConsumer, LiveImageDiffWriter, LiveImageDraw, LiveImageSource, LivePixelFormat,
};
use teksilo_canvas::{Canvas, PixelRect, Rect, RenderFrame};

/// What happens before one frame is handed over.
#[derive(Debug, Clone)]
enum Step {
    /// The same frame again.
    Same,
    /// A rect of the frame repainted (clamped into the frame).
    Rect {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        seed: u8,
    },
    /// Single pixels changed here and there: many bands.
    Scatter(Vec<(u32, u32, u8)>),
    /// A new size, every pixel new.
    Resize { w: u32, h: u32, seed: u8 },
    /// Another writer commits a rect, then the frame is handed over again.
    OtherCommits(u8),
    /// Another writer clears the source.
    OtherClears,
    /// Another writer abandons a transaction that wrote a rect.
    OtherAbandons(u8),
}

fn arb_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => Just(Step::Same),
        3 => (0u32..24, 0u32..24, 1u32..24, 1u32..24, any::<u8>())
            .prop_map(|(x, y, w, h, seed)| Step::Rect { x, y, w, h, seed }),
        2 => prop::collection::vec((0u32..24, 0u32..24, any::<u8>()), 1..40)
            .prop_map(Step::Scatter),
        1 => (1u32..=24, 1u32..=24, any::<u8>())
            .prop_map(|(w, h, seed)| Step::Resize { w, h, seed }),
        1 => any::<u8>().prop_map(Step::OtherCommits),
        1 => Just(Step::OtherClears),
        1 => any::<u8>().prop_map(Step::OtherAbandons),
    ]
}

/// A starting size up to 24 px a side and up to twelve steps. Cost: at most
/// 24 × 24 pixels compared, written and uploaded per step.
fn arb_case() -> impl Strategy<Value = ((u32, u32), u8, bool, Vec<Step>)> {
    (
        (1u32..=24, 1u32..=24),
        any::<u8>(),
        any::<bool>(),
        prop::collection::vec(arb_step(), 1..=12),
    )
}

fn pixels(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let v = (i as u8).wrapping_mul(29).wrapping_add(seed);
            [v, seed, v ^ seed, 255]
        })
        .collect()
}

fn drawing(c: &LiveImageConsumer) -> RenderFrame {
    let _ = c.take_geometry();
    c.record_layout_meta(c.source().meta());
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(c, &LiveImageDraw::new(rect, rect));
    canvas.into_render_frame()
}

proptest! {
    #[test]
    fn the_window_shows_the_last_frame_and_unchanged_frames_commit_nothing(
        ((w0, h0), seed, bgrx, steps) in arb_case()
    ) {
        let format = if bgrx { LivePixelFormat::Bgrx8 } else { LivePixelFormat::Rgba8 };
        let source = LiveImageSource::new(format);
        let consumer = source.attach(None);
        let other = source.writer();
        let mut writer = LiveImageDiffWriter::new(source.writer());
        let mut mirror = LiveImageMirror::new();

        let (mut w, mut h) = (w0, h0);
        let mut frame = pixels(w, h, seed);
        prop_assert!(writer.write_frame(w, h, &frame, (w * 4) as usize).unwrap().is_some());
        mirror.consume(&drawing(&consumer));

        for step in steps {
            let before = frame.clone();
            let mut touched = false;
            match &step {
                Step::Same => {}
                Step::Rect { x, y, w: rw, h: rh, seed } => {
                    let rect = PixelRect::new(x % w, y % h, 0, 0);
                    let rect = PixelRect::new(
                        rect.x,
                        rect.y,
                        (*rw).min(w - rect.x),
                        (*rh).min(h - rect.y),
                    );
                    for yy in rect.y..rect.y + rect.height {
                        for xx in rect.x..rect.x + rect.width {
                            let at = ((yy * w + xx) * 4) as usize;
                            frame[at..at + 4].copy_from_slice(&[*seed, xx as u8, yy as u8, 255]);
                        }
                    }
                }
                Step::Scatter(points) => {
                    for &(x, y, v) in points {
                        let at = (((y % h) * w + x % w) * 4) as usize;
                        frame[at] = v;
                    }
                }
                Step::Resize { w: nw, h: nh, seed } => {
                    (w, h) = (*nw, *nh);
                    frame = pixels(w, h, *seed);
                }
                Step::OtherCommits(v) => {
                    other.write_rect(PixelRect::new(0, 0, 1, 1), &[*v; 4], 4).unwrap();
                    touched = true;
                }
                Step::OtherClears => {
                    other.clear().unwrap();
                    touched = true;
                }
                Step::OtherAbandons(v) => {
                    let mut guard = other.lock().unwrap();
                    guard.write_rect(PixelRect::new(0, 0, 1, 1), &[*v; 4], 4).unwrap();
                    drop(guard);
                }
            }
            let unchanged = frame == before;
            let generation = source.generation();
            let result = writer.write_frame(w, h, &frame, (w * 4) as usize).unwrap();
            if unchanged && !touched {
                prop_assert_eq!(result, None, "{:?}: an unchanged frame commits nothing", step);
                prop_assert_eq!(source.generation(), generation);
            } else {
                prop_assert_eq!(result, Some(source.generation()), "{:?}", step);
            }
            mirror.consume(&drawing(&consumer));
            let shown = mirror.pixels(source.id()).expect("a texture");
            prop_assert_eq!((shown.0, shown.1), (w, h));
            prop_assert!(shown.2 == frame.as_slice(), "{:?}: the window shows the frame", step);
        }
    }
}
