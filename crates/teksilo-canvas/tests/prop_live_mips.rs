// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Property test for the mip chain of a live picture drawn with
//! `ScalingFilter::Trilinear` (crates/teksilo-canvas/src/live_image/mips.rs
//! and the live pass's rebuild): rebuilding only each level's footprint of
//! what a frame changed leaves every level as a whole rebuild would. A
//! footprint one texel short is a thumbnail that keeps a stale corner until
//! the next whole-frame commit.
//!
//! The unit tests in `live_image/pass/tests.rs` pin the rebuild for chosen
//! rects; this generalises it over sizes, rect sequences, the frames that
//! skip `Trilinear` in between, and both alpha kinds. Manual override knob:
//! `PROPTEST_CASES=N cargo test -p teksilo-canvas --test prop_live_mips`.

use proptest::prelude::*;
use teksilo_canvas::live_image::internal::mip_levels;
use teksilo_canvas::live_image::testing::LiveImageMirror;
use teksilo_canvas::live_image::{
    LiveImageConsumer, LiveImageDraw, LiveImageSource, LivePixelFormat, ScalingFilter,
};
use teksilo_canvas::resample::{downsample_half, downsample_half_opaque};
use teksilo_canvas::{Canvas, PixelRect, Rect, RenderFrame};

/// One commit: a rect of the source and the seed its pixels are made from,
/// and whether the frame after it samples the chain.
#[derive(Debug, Clone)]
struct Step {
    rect: PixelRect,
    seed: u8,
    trilinear: bool,
}

/// A source size up to 48 px a side, then up to six commits of rects inside
/// it (drawn from the size with `prop_flat_map`, never independently). Cost:
/// at most 48 × 48 texels per level and step, a few thousand per case.
fn arb_case() -> impl Strategy<Value = ((u32, u32), bool, Vec<Step>)> {
    ((1u32..=48, 1u32..=48), any::<bool>()).prop_flat_map(|((w, h), opaque)| {
        let step = (0..w, 0..h, any::<u8>(), any::<bool>()).prop_flat_map(
            move |(x, y, seed, trilinear)| {
                (1..=w - x, 1..=h - y).prop_map(move |(rw, rh)| Step {
                    rect: PixelRect::new(x, y, rw, rh),
                    seed,
                    trilinear,
                })
            },
        );
        (
            Just((w, h)),
            Just(opaque),
            prop::collection::vec(step, 1..=6),
        )
    })
}

/// `w × h` pixels with some transparent and half-transparent texels.
fn texels(w: u32, h: u32, seed: u8) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let v = (i as u8).wrapping_mul(37).wrapping_add(seed);
            let a = match i % 4 {
                0 => 0,
                1 => 128,
                _ => 255,
            };
            [v, v.wrapping_mul(3), seed, a]
        })
        .collect()
}

fn frame(c: &LiveImageConsumer, filter: ScalingFilter) -> RenderFrame {
    let _ = c.take_geometry();
    c.record_layout_meta(c.source().meta());
    let rect = Rect::new(0.0, 0.0, 64.0, 64.0);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(c, &LiveImageDraw::new(rect, rect).filter(filter));
    canvas.into_render_frame()
}

/// Every level of the texture equals the chain halved from its level 0.
fn chain_matches(mirror: &LiveImageMirror, source: &LiveImageSource, opaque: bool) -> bool {
    let (w, h, level0) = mirror.pixels(source.id()).expect("a texture");
    let (mut cw, mut ch, mut cur) = (w, h, level0.to_vec());
    for k in 1..mip_levels(w, h) {
        let (nw, nh, next) = if opaque && k == 1 {
            downsample_half_opaque(&cur, cw, ch)
        } else {
            downsample_half(&cur, cw, ch)
        };
        match mirror.mip_level(source.id(), k) {
            Some((lw, lh, px)) if (lw, lh) == (nw, nh) && px == next.as_slice() => {}
            _ => return false,
        }
        (cw, ch, cur) = (nw, nh, next);
    }
    true
}

// ── 1. After any sequence of rect commits, with frames that sample the
//      chain and frames that do not, the chain a `Trilinear` frame leaves
//      is the chain of its level 0 ──
proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]
    #[test]
    fn footprint_rebuilds_leave_the_whole_chains_texels(
        ((w, h), opaque, steps) in arb_case()
    ) {
        let format = if opaque { LivePixelFormat::Rgbx8 } else { LivePixelFormat::Rgba8 };
        let source = LiveImageSource::new(format);
        let writer = source.writer();
        writer.write_frame(w, h, &texels(w, h, 0), (w * 4) as usize).unwrap();
        let consumer = source.attach(None);
        let mut mirror = LiveImageMirror::new();
        mirror.consume(&frame(&consumer, ScalingFilter::Trilinear));
        prop_assert!(chain_matches(&mirror, &source, opaque));
        for step in &steps {
            let px = texels(step.rect.width, step.rect.height, step.seed);
            writer.write_rect(step.rect, &px, (step.rect.width * 4) as usize).unwrap();
            let filter = if step.trilinear { ScalingFilter::Trilinear } else { ScalingFilter::Linear };
            mirror.consume(&frame(&consumer, filter));
            if step.trilinear {
                prop_assert!(
                    chain_matches(&mirror, &source, opaque),
                    "after {:?} on {}x{}", step, w, h
                );
            }
        }
        // A last `Trilinear` frame catches up whatever the steps left.
        mirror.consume(&frame(&consumer, ScalingFilter::Trilinear));
        prop_assert!(chain_matches(&mirror, &source, opaque));
    }
}
