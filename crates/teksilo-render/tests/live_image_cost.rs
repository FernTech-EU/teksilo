// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a live picture's upload costs (spec AC15), for a 720 × 1280 frame: a
//! phone-shaped VM screen.
//!
//! Two kinds of claim, kept apart because only one of them is exact.
//!
//! - **What is uploaded**, counted by the renderer, so asserted in every
//!   build on every adapter: a full commit uploads the frame's bytes in one
//!   copy, and a commit that changes two rects (54 × 1040 and 672 × 24, the
//!   282 KiB a guest's caret column and status bar repaint) uploads exactly
//!   their bytes in one copy each, never the frame around them.
//! - **What it costs in time**, as ratios between two measurements taken in
//!   this process and never as durations, like `wet_stroke_cost.rs`. Each
//!   sample times one render, with the GPU idle before it so a queue the
//!   previous sample left behind is not billed to this one. A full upload
//!   costs at most a twentieth of `Renderer::register_image` for the same
//!   picture (a new texture and a mip chain built on the CPU), in every
//!   build; it has held by a factor of three or more on RADV and lavapipe in
//!   both profiles. A dirty upload costs at most half a full one only in an
//!   optimized build: at opt-level 0 wgpu's per-row staging copy dominates,
//!   the dirty set has 1 064 rows against the full frame's 1 280, and on a
//!   GPU the ratio lands around a half. That test is `#[ignore]`d and
//!   asserts its ratio only without debug assertions:
//!
//! ```sh
//! cargo test -p teksilo-render --release --test live_image_cost -- --ignored --nocapture
//! ```
//!
//! Each test returns early without an adapter unless
//! `TEKSILO_TEST_REQUIRE_ADAPTER` insists on one, and prints the adapter and
//! its medians.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use teksilo_canvas::live_image::{
    LiveImageConsumer, LiveImageDraw, LiveImageSource, LiveImageWriter, LivePixelFormat,
    LiveTextureStats, PixelRect,
};
use teksilo_canvas::wake::{CountingWaker, RedrawWaker};
use teksilo_canvas::{Canvas, Rect, RenderFrame};
use teksilo_render::Renderer;
use teksilo_render::test_support::require_test_renderer;

const W: u32 = 720;
const H: u32 = 1280;

/// The two rects of the dirty workload: 54 × 1040 and 672 × 24 px, 1 064
/// rows, 282 KiB. Their bounding box is the whole frame, so the planner
/// keeps them apart.
const DIRTY: [PixelRect; 2] = [
    PixelRect::new(0, 0, 54, 1040),
    PixelRect::new(48, 1256, 672, 24),
];

/// The timing tests share the process's one device: run them one at a time.
static TIMED: Mutex<()> = Mutex::new(());

struct Gpu {
    renderer: Renderer,
    device: wgpu::Device,
    view: wgpu::TextureView,
}

/// The renderer, and a 16 × 16 target: the picture is drawn into it scaled
/// down, so the draw is a few hundred fragments and the upload is the cost.
fn gpu(label: &'static str) -> Option<Gpu> {
    let (renderer, device, _queue) = pollster::block_on(require_test_renderer(label))?;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("live_image_cost_target"),
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    Some(Gpu {
        renderer,
        device,
        view,
    })
}

impl Gpu {
    fn render(&mut self, frame: &RenderFrame) {
        self.renderer
            .render(frame, &self.view, 1.0, 16, 16, [0.0, 0.0, 0.0, 0.0]);
    }

    /// Wait until the GPU has run everything submitted.
    fn idle(&self) {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("the device stays up");
    }

    /// How long one render of `frame` takes, from an idle GPU.
    fn timed_render(&mut self, frame: &RenderFrame) -> Duration {
        self.idle();
        let start = Instant::now();
        self.render(frame);
        start.elapsed()
    }

    fn adapter(&self) -> String {
        let info = self.device.adapter_info();
        format!("{} ({:?}, {})", info.name, info.backend, info.driver)
    }
}

/// `W × H` opaque pixels, every one distinct from its neighbours and from
/// the frame `seed` before it.
fn frame_pixels(seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            px.extend_from_slice(&[
                (x as u8).wrapping_add(seed),
                (y as u8).wrapping_mul(3),
                seed,
                255,
            ]);
        }
    }
    px
}

fn live() -> (LiveImageSource, LiveImageWriter, LiveImageConsumer) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer
        .write_frame(W, H, &frame_pixels(0), (W * 4) as usize)
        .expect("a valid frame");
    let waker = Arc::new(CountingWaker::new());
    let consumer = source.attach(Some(waker as Arc<dyn RedrawWaker>));
    (source, writer, consumer)
}

/// A frame drawing the picture over the whole target, after the layout
/// pre-pass has recorded its size.
fn drawing(consumer: &LiveImageConsumer) -> RenderFrame {
    let _ = consumer.take_geometry();
    consumer.record_layout_meta(consumer.source().meta());
    let rect = Rect::new(0.0, 0.0, 16.0, 16.0);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(consumer, &LiveImageDraw::new(rect, rect));
    canvas.into_render_frame()
}

fn write_dirty(writer: &LiveImageWriter, value: u8) {
    let mut guard = writer.lock().expect("the writer's session");
    for rect in DIRTY {
        let px = [value, value, value, 255].repeat((rect.width * rect.height) as usize);
        guard
            .write_rect(rect, &px, (rect.width * 4) as usize)
            .expect("inside the frame");
    }
    guard.commit();
}

fn dirty_bytes() -> u64 {
    DIRTY
        .iter()
        .map(|r| u64::from(r.width) * u64::from(r.height) * 4)
        .sum()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// What the renderer counted between two readings: full uploads, partial
/// uploads, bytes uploaded and upload calls.
fn delta(before: LiveTextureStats, after: LiveTextureStats) -> (u64, u64, u64, u64) {
    (
        after.uploads_full - before.uploads_full,
        after.uploads_partial - before.uploads_partial,
        after.bytes_uploaded - before.bytes_uploaded,
        after.upload_calls - before.upload_calls,
    )
}

/// A full commit uploads the frame in one copy; a commit of the two dirty
/// rects uploads their bytes, one copy each, and nothing else.
#[test]
fn an_upload_is_the_bytes_that_changed() {
    let Some(mut g) = gpu("live_cost_counts") else {
        return;
    };
    let (_source, writer, consumer) = live();
    let before = g.renderer.live_texture_stats();
    g.render(&drawing(&consumer));
    let first = g.renderer.live_texture_stats();
    assert_eq!(
        delta(before, first),
        (1, 0, u64::from(W * H * 4), 1),
        "the first frame: one full upload of {W} x {H}"
    );

    writer
        .write_frame(W, H, &frame_pixels(1), (W * 4) as usize)
        .expect("a valid frame");
    g.render(&drawing(&consumer));
    let full = g.renderer.live_texture_stats();
    assert_eq!(
        delta(first, full),
        (1, 0, u64::from(W * H * 4), 1),
        "a whole new frame: one full upload"
    );

    write_dirty(&writer, 0x40);
    g.render(&drawing(&consumer));
    let dirty = g.renderer.live_texture_stats();
    assert_eq!(
        delta(full, dirty),
        (0, 1, dirty_bytes(), 2),
        "two rects: their {} bytes, in one copy each",
        dirty_bytes()
    );

    g.render(&drawing(&consumer));
    assert_eq!(
        delta(dirty, g.renderer.live_texture_stats()),
        (0, 0, 0, 0),
        "no commit, no upload"
    );
}

/// A full live upload costs at most a twentieth of registering the same
/// picture through the image path, in every build.
#[test]
fn a_full_upload_costs_a_twentieth_of_registering_the_picture() {
    let _one_at_a_time = TIMED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut g) = gpu("live_cost_full") else {
        return;
    };
    let (_source, writer, consumer) = live();
    g.render(&drawing(&consumer));

    let mut uploads = Vec::new();
    for i in 0..31u8 {
        writer
            .write_frame(W, H, &frame_pixels(i + 1), (W * 4) as usize)
            .expect("a valid frame");
        uploads.push(g.timed_render(&drawing(&consumer)));
    }
    // Each registration is a texture the renderer never frees: fifteen of
    // them, 74 MiB with their mip chains.
    let pixels = frame_pixels(0);
    let mut registrations = Vec::new();
    for i in 0..15 {
        g.idle();
        let start = Instant::now();
        g.renderer
            .register_image(&format!("live_cost_{i}"), W, H, &pixels);
        registrations.push(start.elapsed());
    }
    g.idle();

    let (upload, register) = (median(uploads), median(registrations));
    let ratio = upload.as_secs_f64() / register.as_secs_f64();
    println!(
        "{}: full live upload {upload:?}, register_image {register:?}, ratio 1/{:.0}",
        g.adapter(),
        1.0 / ratio
    );
    assert!(
        ratio <= 1.0 / 20.0,
        "a full upload ({upload:?}) costs more than a twentieth of register_image ({register:?})"
    );
}

/// A dirty upload of 282 KiB costs at most half a full one, in an optimized
/// build. See the module docs for why not at opt-level 0.
#[test]
#[ignore = "a timing ratio that holds only in an optimized build: run it with \
            `cargo test -p teksilo-render --release --test live_image_cost -- --ignored`"]
fn a_dirty_upload_costs_at_most_half_a_full_one() {
    let _one_at_a_time = TIMED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut g) = gpu("live_cost_dirty") else {
        return;
    };
    let (_source, writer, consumer) = live();
    g.render(&drawing(&consumer));

    let (mut fulls, mut dirties) = (Vec::new(), Vec::new());
    for i in 0..31u8 {
        writer
            .write_frame(W, H, &frame_pixels(i + 1), (W * 4) as usize)
            .expect("a valid frame");
        fulls.push(g.timed_render(&drawing(&consumer)));
        write_dirty(&writer, i);
        dirties.push(g.timed_render(&drawing(&consumer)));
    }
    g.idle();
    let stats = g.renderer.live_texture_stats();
    assert_eq!(
        (stats.uploads_full, stats.uploads_partial),
        (32, 31),
        "every sample uploaded what it was meant to"
    );

    let (full, dirty) = (median(fulls), median(dirties));
    let ratio = dirty.as_secs_f64() / full.as_secs_f64();
    println!(
        "{}: full upload {full:?}, dirty upload {dirty:?}, ratio {ratio:.2}",
        g.adapter()
    );
    if cfg!(debug_assertions) {
        println!("debug assertions are on: the ratio is reported, not asserted");
        return;
    }
    assert!(
        ratio <= 0.5,
        "a dirty upload ({dirty:?}) costs more than half a full one ({full:?})"
    );
}

/// The live pass's frame time, lock holds and commit-to-upload delay in
/// steady state, printed rather than asserted, for the measurements AC6, AC12
/// and AC13 name: a producer thread committing at 60 Hz against a render
/// loop at 60 Hz, for five seconds per workload and phase. Offscreen, so no
/// acquire wait is included, and a render that finds the lock held is not
/// followed by the extra frame the producer's unlock asks a window for.
///
/// Each workload runs twice. With the two loops in phase, every render wakes
/// as the producer writes, the worst case for contention; half a period
/// apart, they never meet. A real window and producer drift between the two.
/// Run it in an optimized build:
///
/// ```sh
/// cargo test -p teksilo-render --release --features live-image-timings \
///     --test live_image_cost -- --ignored --nocapture measure
/// ```
#[cfg(any(debug_assertions, feature = "live-image-timings"))]
#[test]
#[ignore = "a measurement, printed: run it in an optimized build with the live-image-timings feature"]
fn measure_the_live_pass_at_60_hz() {
    let _one_at_a_time = TIMED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(g) = gpu("live_cost_measure") else {
        return;
    };
    // AC7's workload: four rects covering 8 % of a 720 × 1280 frame per
    // commit; then full 1920 × 1080 frames.
    let workloads: [(&str, u32, u32, bool); 2] = [
        ("720x1280, 4 rects, 8 %", W, H, false),
        ("1920x1080 full", 1920, 1080, true),
    ];
    for ((name, w, h, full), phase) in workloads
        .into_iter()
        .flat_map(|workload| [(workload, "in phase"), (workload, "half a period apart")])
    {
        let source = LiveImageSource::new(LivePixelFormat::Bgrx8);
        let writer = source.writer();
        let frame = vec![0x80u8; (w * h * 4) as usize];
        writer
            .write_frame(w, h, &frame, (w * 4) as usize)
            .expect("a valid frame");
        let waker = Arc::new(CountingWaker::new());
        let consumer = source.attach(Some(waker as Arc<dyn RedrawWaker>));
        let mut renderer = pollster::block_on(require_test_renderer("live_cost_measure"))
            .expect("the adapter opened above")
            .0;
        renderer.render(&drawing(&consumer), &g.view, 1.0, 16, 16, [0.0; 4]);

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let producer = {
            let (writer, stop) = (writer.clone(), stop.clone());
            std::thread::spawn(move || {
                // 4 × 192 × 96 = 73 728 px, 8 % of 720 × 1280.
                let rects = [
                    PixelRect::new(16, 16, 192, 96),
                    PixelRect::new(500, 300, 192, 96),
                    PixelRect::new(40, 900, 192, 96),
                    PixelRect::new(480, 1150, 192, 96),
                ];
                let rect_px = vec![0x40u8; 192 * 96 * 4];
                let mut next = Instant::now();
                let mut shade = 0u8;
                while !stop.load(std::sync::atomic::Ordering::Acquire) {
                    shade = shade.wrapping_add(1);
                    if full {
                        let px = vec![shade; (w * h * 4) as usize];
                        writer.write_frame(w, h, &px, (w * 4) as usize).unwrap();
                    } else {
                        let mut guard = writer.lock().unwrap();
                        for rect in rects {
                            guard.write_rect(rect, &rect_px, 192 * 4).unwrap();
                        }
                        guard.commit();
                    }
                    next += Duration::from_micros(16_667);
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
            })
        };
        if phase != "in phase" {
            std::thread::sleep(Duration::from_micros(8_333));
        }
        let started = Instant::now();
        let mut next = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            renderer.render(&drawing(&consumer), &g.view, 1.0, 16, 16, [0.0; 4]);
            next += Duration::from_micros(16_667);
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
        stop.store(true, std::sync::atomic::Ordering::Release);
        producer.join().expect("the producer");
        g.idle();
        let t = renderer.live_texture_timings();
        let s = renderer.live_texture_stats();
        println!(
            "{} — {name}, {phase}: prepare p50 {} p99 {} max {} µs; lock hold p50 {} p99 {} max {} µs; \
             commit to upload p50 {} p99 {} max {} µs; {} uploads, {} contended, {} blocking \
             waits, over {} renders",
            g.adapter(),
            t.prepare.p50,
            t.prepare.p99,
            t.prepare.max,
            t.lock_hold.p50,
            t.lock_hold.p99,
            t.lock_hold.max,
            t.commit_to_upload.p50,
            t.commit_to_upload.p99,
            t.commit_to_upload.max,
            s.uploads_full + s.uploads_partial,
            s.contended,
            s.blocking_waits,
            t.prepare.samples,
        );
    }
}
