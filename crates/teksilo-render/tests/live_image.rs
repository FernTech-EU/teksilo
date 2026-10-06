// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live pictures on the GPU (spec D.1-D.10, D.12, D.14-D.21): what the live
//! pass uploads, what the draw shows, byte for byte where the sampling is
//! 1:1, and the mirror deciding exactly as the GPU does.
//!
//! Each test returns early without an adapter unless
//! `TEKSILO_TEST_REQUIRE_ADAPTER` insists on one; every render runs inside a
//! validation error scope that must come back empty. To run them on Mesa's
//! lavapipe on a machine with a GPU:
//!
//! ```sh
//! VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan \
//!     TEKSILO_TEST_REQUIRE_ADAPTER=lavapipe cargo test -p teksilo-render --test live_image
//! ```

use std::sync::Arc;
use std::time::Duration;

use teksilo_canvas::live_image::testing::LiveImageMirror;
use teksilo_canvas::live_image::{
    ImageOrientation, LiveImageConsumer, LiveImageDraw, LiveImageSource, LiveImageWriter,
    LivePixelFormat, PixelRect, ScalingFilter,
};
use teksilo_canvas::wake::{CountingWaker, RedrawWaker};
use teksilo_canvas::{Canvas, DrawCommand, Rect, RenderFrame};
use teksilo_render::Renderer;
use teksilo_render::test_support::{read_texture_rgba, require_test_renderer};

struct Gpu {
    renderer: Renderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu(label: &'static str) -> Option<Gpu> {
    let (renderer, device, queue) = pollster::block_on(require_test_renderer(label))?;
    Some(Gpu {
        renderer,
        device,
        queue,
    })
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl Gpu {
    fn target(&self, width: u32, height: u32) -> Target {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("live_image_test_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Target {
            texture,
            view,
            width,
            height,
        }
    }

    fn render(&mut self, frame: &RenderFrame, target: &Target) {
        self.checked(|g| {
            g.renderer.render(
                frame,
                &target.view,
                1.0,
                target.width,
                target.height,
                [0.0, 0.0, 0.0, 0.0],
            )
        });
    }

    fn capture(&mut self, frame: &RenderFrame, target: &Target) {
        self.checked(|g| {
            g.renderer.render_capture(
                frame,
                &target.view,
                1.0,
                target.width,
                target.height,
                [0.0, 0.0, 0.0, 0.0],
            )
        });
    }

    /// Run `f` inside a validation scope that must come back empty.
    fn checked(&mut self, f: impl FnOnce(&mut Self)) {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        f(self);
        let error = pollster::block_on(scope.pop());
        assert!(error.is_none(), "validation error: {error:?}");
    }

    fn read(&self, target: &Target) -> Vec<u8> {
        read_texture_rgba(
            &self.device,
            &self.queue,
            &target.texture,
            target.width,
            target.height,
        )
    }
}

/// `w × h` pixels in `format`'s byte order, pixel `(x, y)` encoding
/// `(x, y, seed)` as its colour, opaque.
fn pattern(format: LivePixelFormat, w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&encode(
                format,
                [(x as u8).wrapping_mul(3), (y as u8).wrapping_mul(5), seed],
            ));
        }
    }
    px
}

/// An opaque RGB colour in `format`'s byte order; an X byte is zero.
fn encode(format: LivePixelFormat, [r, g, b]: [u8; 3]) -> [u8; 4] {
    match format {
        LivePixelFormat::Rgba8 => [r, g, b, 255],
        LivePixelFormat::Bgra8 => [b, g, r, 255],
        LivePixelFormat::Rgbx8 => [r, g, b, 0],
        LivePixelFormat::Bgrx8 => [b, g, r, 0],
        _ => unreachable!("the four formats of today"),
    }
}

/// What a 1:1 draw of `pattern` reads back as: RGBA, opaque.
fn shown(w: u32, h: u32, seed: u8) -> Vec<u8> {
    pattern(LivePixelFormat::Rgba8, w, h, seed)
}

fn live(format: LivePixelFormat, w: u32, h: u32) -> (LiveImageSource, LiveImageWriter) {
    let source = LiveImageSource::new(format);
    let writer = source.writer();
    writer
        .write_frame(w, h, &pattern(format, w, h, 0), (w * 4) as usize)
        .unwrap();
    (source, writer)
}

fn consumer(source: &LiveImageSource) -> (Arc<CountingWaker>, LiveImageConsumer) {
    let waker = Arc::new(CountingWaker::new());
    let c = source.attach(Some(waker.clone() as Arc<dyn RedrawWaker>));
    (waker, c)
}

/// The layout pre-pass, for one consumer.
fn lay_out(c: &LiveImageConsumer) {
    let _ = c.take_geometry();
    c.record_layout_meta(c.source().meta());
}

/// A frame drawing `c` at 1:1 from the origin, laid out first.
fn frame_1to1(c: &LiveImageConsumer, filter: ScalingFilter) -> RenderFrame {
    lay_out(c);
    let (w, h) = c.layout_meta().size.unwrap();
    let rect = Rect::new(0.0, 0.0, w as f32, h as f32);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(c, &LiveImageDraw::new(rect, rect).filter(filter));
    canvas.into_render_frame()
}

/// `n` opaque pixels of grey `v`.
fn solid(n: usize, v: u8) -> Vec<u8> {
    [v, v, v, 255].repeat(n)
}

fn region(px: &[u8], width: u32, rect: PixelRect) -> Vec<u8> {
    let mut out = Vec::new();
    for y in rect.y..rect.y + rect.height {
        let at = ((y * width + rect.x) * 4) as usize;
        out.extend_from_slice(&px[at..at + rect.width as usize * 4]);
    }
    out
}

// ── D.1, D.2, D.3: the picture, exact ──

#[test]
fn d01_a_new_generation_of_one_source_shows_its_new_pixels() {
    let Some(mut g) = gpu("live_d01") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 16, 12);
    let (_, c) = consumer(&source);
    let t = g.target(16, 12);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(g.read(&t), shown(16, 12, 0));
    writer
        .write_frame(16, 12, &pattern(LivePixelFormat::Rgba8, 16, 12, 9), 64)
        .unwrap();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(g.read(&t), shown(16, 12, 9), "the freeze the old path had");
}

#[test]
fn d02_a_dirty_rect_changes_only_itself_at_any_row_width() {
    for (w, h) in [(720, 4), (719, 5), (33, 17)] {
        let Some(mut g) = gpu("live_d02") else { return };
        let (source, writer) = live(LivePixelFormat::Rgba8, w, h);
        let (_, c) = consumer(&source);
        let t = g.target(w, h);
        g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
        let rect = PixelRect::new(3, 1, 7, 2);
        writer.write_rect(rect, &solid(7 * 2, 0xC0), 28).unwrap();
        g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
        let after = g.read(&t);
        let mut expected = shown(w, h, 0);
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let at = ((y * w + x) * 4) as usize;
                expected[at..at + 4].copy_from_slice(&[0xC0, 0xC0, 0xC0, 255]);
            }
        }
        assert_eq!(after, expected, "{w}x{h}");
        assert_eq!(g.renderer.live_texture_stats().uploads_partial, 1);
    }
}

#[test]
fn d03_every_format_reads_back_exactly_and_an_x_byte_draws_opaque() {
    for format in [
        LivePixelFormat::Rgba8,
        LivePixelFormat::Bgra8,
        LivePixelFormat::Rgbx8,
        LivePixelFormat::Bgrx8,
    ] {
        let Some(mut g) = gpu("live_d03") else { return };
        let source = LiveImageSource::new(format);
        let writer = source.writer();
        let colours = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [17, 130, 250]];
        let px: Vec<u8> = colours.iter().flat_map(|&c| encode(format, c)).collect();
        writer.write_frame(4, 1, &px, 16).unwrap();
        let (_, c) = consumer(&source);
        let t = g.target(4, 1);
        g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
        let expected: Vec<u8> = colours
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect();
        assert_eq!(g.read(&t), expected, "{format:?}");
    }
}

// ── D.4, D.5, D.6: cached frames, size changes, release ──

#[test]
fn d04_a_replayed_frame_after_a_newer_upload_keeps_the_newer_pixels() {
    let Some(mut g) = gpu("live_d04") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 8, 8);
    let (_, c) = consumer(&source);
    let t = g.target(8, 8);
    let cached = frame_1to1(&c, ScalingFilter::Nearest);
    g.render(&cached, &t);
    writer
        .write_frame(8, 8, &pattern(LivePixelFormat::Rgba8, 8, 8, 4), 32)
        .unwrap();
    g.render(&cached, &t);
    assert_eq!(g.read(&t), shown(8, 8, 4));
    g.render(&cached, &t);
    assert_eq!(g.read(&t), shown(8, 8, 4), "a replay uploads nothing older");
}

#[test]
fn d05_a_size_change_stages_a_new_texture_and_frees_the_old() {
    let Some(mut g) = gpu("live_d05") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 72, 128);
    let (_, c) = consumer(&source);
    let t = g.target(128, 128);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let s = g.renderer.live_texture_stats();
    assert_eq!((s.textures, s.bytes), (1, 72 * 128 * 4));
    let waits = s.blocking_waits;
    writer
        .write_frame(128, 72, &pattern(LivePixelFormat::Rgba8, 128, 72, 1), 512)
        .unwrap();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let s = g.renderer.live_texture_stats();
    assert_eq!((s.textures, s.bytes), (1, 128 * 72 * 4));
    assert_eq!(
        s.blocking_waits,
        waits + 1,
        "the resize frame waited for the lock"
    );
    assert_eq!(
        region(&g.read(&t), 128, PixelRect::new(0, 0, 128, 72)),
        shown(128, 72, 1)
    );
}

#[test]
fn d06_a_frame_without_the_quad_or_a_clear_frees_the_texture() {
    let Some(mut g) = gpu("live_d06") else { return };
    g.renderer.set_live_park_budget(0);
    let (source, writer) = live(LivePixelFormat::Rgba8, 16, 16);
    let (_, c) = consumer(&source);
    let t = g.target(16, 16);
    g.render(&frame_1to1(&c, ScalingFilter::Linear), &t);
    assert_eq!(g.renderer.live_texture_stats().textures, 1);
    g.render(&RenderFrame::new(), &t);
    let s = g.renderer.live_texture_stats();
    assert_eq!((s.textures, s.bytes, s.textures_parked), (0, 0, 0));
    g.render(&frame_1to1(&c, ScalingFilter::Linear), &t);
    writer.clear().unwrap();
    g.render(&frame_1to1(&c, ScalingFilter::Linear), &t);
    let s = g.renderer.live_texture_stats();
    assert_eq!(
        (s.textures, s.bytes),
        (0, 0),
        "cleared: painted nothing to show"
    );
    assert_eq!(g.read(&t), vec![0; 16 * 16 * 4], "and the background shows");
}

// ── D.7, D.8: filters, crops, orientations ──

#[test]
fn d07_nearest_upscales_to_hard_blocks_and_linear_blends() {
    let Some(mut g) = gpu("live_d07") else { return };
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    let px = [
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [0, 255, 0, 255],
        [255, 255, 255, 255],
    ]
    .concat();
    writer.write_frame(2, 2, &px, 8).unwrap();
    let (_, c) = consumer(&source);
    lay_out(&c);
    let t = g.target(16, 16);
    let rect = Rect::new(0.0, 0.0, 16.0, 16.0);
    let draw = |filter| {
        let mut canvas = Canvas::new();
        canvas.draw_live_image(&c, &LiveImageDraw::new(rect, rect).filter(filter));
        canvas.into_render_frame()
    };
    g.render(&draw(ScalingFilter::Nearest), &t);
    let out = g.read(&t);
    let at = |x: u32, y: u32| &out[((y * 16 + x) * 4) as usize..((y * 16 + x) * 4 + 4) as usize];
    for (x, y) in [(0, 0), (7, 7), (3, 6)] {
        assert_eq!(at(x, y), &[255, 0, 0, 255], "({x}, {y}): a hard block");
    }
    assert_eq!(at(8, 0), &[0, 0, 255, 255]);
    assert_eq!(at(8, 8), &[255, 255, 255, 255]);
    g.render(&draw(ScalingFilter::Linear), &t);
    let out = g.read(&t);
    let middle = &out[((7 * 16 + 7) * 4) as usize..((7 * 16 + 7) * 4 + 4) as usize];
    assert!(
        middle[0] > 0 && middle[0] < 255 && middle[2] > 0,
        "linear blends at the seam: {middle:?}"
    );
}

#[test]
fn d08_a_crop_and_each_orientation_put_the_source_corners_where_the_tables_say() {
    let Some(mut g) = gpu("live_d08") else { return };
    // A 4 × 2 source with a distinct colour per pixel.
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    let px: Vec<u8> = (0..8u8)
        .flat_map(|i| [i * 30, 255 - i * 30, i * 7, 255])
        .collect();
    writer.write_frame(4, 2, &px, 16).unwrap();
    let (_, c) = consumer(&source);
    lay_out(&c);
    for o in [
        ImageOrientation::Normal,
        ImageOrientation::FlipHorizontal,
        ImageOrientation::Rotate90,
        ImageOrientation::Transverse,
    ] {
        let (dw, dh) = o.displayed_size(4, 2);
        let t = g.target(dw, dh);
        let rect = Rect::new(0.0, 0.0, dw as f32, dh as f32);
        let mut canvas = Canvas::new();
        canvas.draw_live_image(
            &c,
            &LiveImageDraw::new(rect, rect)
                .filter(ScalingFilter::Nearest)
                .orientation(o),
        );
        g.render(&canvas.into_render_frame(), &t);
        let out = g.read(&t);
        let geometry = teksilo_canvas::ImageGeometry::new((4, 2), o, rect, rect);
        for yd in 0..dh {
            for xd in 0..dw {
                let (x, y) = geometry
                    .map_to_source(teksilo_canvas::Point::new(xd as f32 + 0.5, yd as f32 + 0.5))
                    .unwrap();
                let src = ((y * 4 + x) * 4) as usize;
                let dst = ((yd * dw + xd) * 4) as usize;
                assert_eq!(&out[dst..dst + 4], &px[src..src + 4], "{o:?} ({xd}, {yd})");
            }
        }
    }
    // Cover: the middle half of a 4 × 1 strip in a 2 × 1 box.
    let strip = LiveImageSource::new(LivePixelFormat::Rgba8);
    let w = strip.writer();
    let px: Vec<u8> = (0..4u8).flat_map(|i| [i * 60, 0, 0, 255]).collect();
    w.write_frame(4, 1, &px, 16).unwrap();
    let (_, c) = consumer(&strip);
    lay_out(&c);
    let t = g.target(2, 1);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(
        &c,
        &LiveImageDraw::new(
            Rect::new(-1.0, 0.0, 4.0, 1.0),
            Rect::new(0.0, 0.0, 2.0, 1.0),
        )
        .filter(ScalingFilter::Nearest),
    );
    g.render(&canvas.into_render_frame(), &t);
    assert_eq!(
        g.read(&t),
        [&px[4..8], &px[8..12]].concat(),
        "the cropped middle"
    );
}

// ── D.9: the device limit ──

#[test]
fn d09_a_side_above_the_device_limit_draws_background_without_an_error() {
    let Some(mut g) = gpu("live_d09") else { return };
    g.renderer.set_live_max_dimension(64);
    let (source, writer) = live(LivePixelFormat::Rgba8, 65, 2);
    let (waker, c) = consumer(&source);
    let t = g.target(65, 2);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(g.read(&t), vec![0; 65 * 2 * 4]);
    assert_eq!(g.renderer.live_texture_stats().textures, 0);
    let before = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), before, "parked: no redraw per commit");
    assert!(
        source.writer().resize(0, 0).is_err(),
        "and a 0 × 0 frame is refused at the writer"
    );
}

// ── D.10: two windows ──

#[test]
fn d10_two_renderers_on_one_device_each_catch_up_alone() {
    let Some(mut a) = gpu("live_d10") else { return };
    let Some(mut b) = gpu("live_d10") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 32, 4);
    let (_, ca) = consumer(&source);
    let (_, cb) = consumer(&source);
    let (ta, tb) = (a.target(32, 4), b.target(32, 4));
    a.render(&frame_1to1(&ca, ScalingFilter::Nearest), &ta);
    b.render(&frame_1to1(&cb, ScalingFilter::Nearest), &tb);
    for i in 0..20u32 {
        writer
            .write_rect(PixelRect::new(i, 0, 1, 1), &solid(1, 0xAA), 4)
            .unwrap();
        a.render(&frame_1to1(&ca, ScalingFilter::Nearest), &ta);
    }
    let lagged = b.renderer.live_texture_stats();
    b.render(&frame_1to1(&cb, ScalingFilter::Nearest), &tb);
    let caught = b.renderer.live_texture_stats();
    assert_eq!(
        caught.uploads_full - lagged.uploads_full,
        1,
        "twenty commits behind: one Full"
    );
    assert_eq!(a.read(&ta), b.read(&tb), "both show the final frame");
}

// ── D.12: a size changed after layout ──

#[test]
fn d12_a_resize_between_paint_and_the_live_pass_draws_the_old_texture() {
    let Some(mut g) = gpu("live_d12") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 8, 8);
    let (_, c) = consumer(&source);
    let t = g.target(10, 8);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let painted = frame_1to1(&c, ScalingFilter::Nearest);
    writer
        .write_frame(10, 8, &pattern(LivePixelFormat::Rgba8, 10, 8, 3), 40)
        .unwrap();
    g.render(&painted, &t);
    assert_eq!(g.renderer.live_texture_stats().stale_deferrals, 1);
    assert_eq!(
        region(&g.read(&t), 10, PixelRect::new(0, 0, 8, 8)),
        shown(8, 8, 0),
        "the old texture, in the old rect, not stretched"
    );
}

// ── D.14, D.15: bands ──

#[test]
fn d14_a_banded_full_upload_reads_back_exactly() {
    let Some(mut g) = gpu("live_d14") else { return };
    g.renderer.set_live_band_bytes(4 << 10);
    let (w, h) = (1283, 7);
    let (source, _writer) = live(LivePixelFormat::Rgba8, w, h);
    let (_, c) = consumer(&source);
    let t = g.target(w, h);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let s = g.renderer.live_texture_stats();
    assert_eq!(s.uploads_full, 1);
    // A 1283-pixel row stages above 4 KiB: one row per band.
    assert_eq!(s.upload_calls, u64::from(h));
    assert_eq!(
        g.read(&t),
        shown(w, h, 0),
        "the zero clear came before the first band"
    );
}

#[test]
fn d15_a_banded_rect_changes_only_itself() {
    let Some(mut g) = gpu("live_d15") else { return };
    g.renderer.set_live_band_bytes(1 << 10);
    let (w, h) = (100, 40);
    let (source, writer) = live(LivePixelFormat::Rgba8, w, h);
    let (_, c) = consumer(&source);
    let t = g.target(w, h);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let calls = g.renderer.live_texture_stats().upload_calls;
    let rect = PixelRect::new(10, 5, 20, 9);
    writer.write_rect(rect, &solid(20 * 9, 0x5A), 80).unwrap();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert!(
        g.renderer.live_texture_stats().upload_calls - calls > 1,
        "banded"
    );
    let mut expected = shown(w, h, 0);
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let at = ((y * w + x) * 4) as usize;
            expected[at..at + 4].copy_from_slice(&[0x5A, 0x5A, 0x5A, 255]);
        }
    }
    assert_eq!(g.read(&t), expected);
}

// ── D.16: a refused texture ──

#[test]
fn d16_a_refused_texture_draws_background_and_a_new_size_retries() {
    let Some(mut g) = gpu("live_d16") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 8, 8);
    let (_, c) = consumer(&source);
    let t = g.target(10, 8);
    g.renderer.fail_next_live_texture();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(g.renderer.live_texture_stats().alloc_failures, 1);
    assert_eq!(
        g.read(&t),
        vec![0; 10 * 8 * 4],
        "nothing to show: background"
    );
    // A new size retries at once, and draws.
    writer
        .write_frame(9, 8, &pattern(LivePixelFormat::Rgba8, 9, 8, 2), 36)
        .unwrap();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(
        region(&g.read(&t), 10, PixelRect::new(0, 0, 9, 8)),
        shown(9, 8, 2)
    );
    // Refused again at the next size: the 9 × 8 texture is not stretched
    // over the 10 × 8 the widget laid out.
    g.renderer.fail_next_live_texture();
    writer
        .write_frame(10, 8, &pattern(LivePixelFormat::Rgba8, 10, 8, 3), 40)
        .unwrap();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(g.renderer.live_texture_stats().alloc_failures, 2);
    assert!(!g.renderer.live_image_decisions()[0].draw);
    assert_eq!(
        g.read(&t),
        vec![0; 10 * 8 * 4],
        "background, never a stretched picture"
    );
}

// ── D.20, D.21: a paused picture ──

/// A frame drawing `c` at 1:1 from the origin, paused or not.
fn frame_paused(c: &LiveImageConsumer, paused: bool) -> RenderFrame {
    lay_out(c);
    let (w, h) = c.layout_meta().size.unwrap();
    let rect = Rect::new(0.0, 0.0, w as f32, h as f32);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(
        c,
        &LiveImageDraw::new(rect, rect)
            .filter(ScalingFilter::Nearest)
            .paused(paused),
    );
    canvas.into_render_frame()
}

#[test]
fn d20_a_paused_draw_shows_the_held_frame_until_unpaused() {
    let Some(mut g) = gpu("live_d20") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 10, 4);
    let (_, c) = consumer(&source);
    let t = g.target(10, 4);
    g.render(&frame_paused(&c, false), &t);
    for seed in 1..=5 {
        writer
            .write_frame(10, 4, &pattern(LivePixelFormat::Rgba8, 10, 4, seed), 40)
            .unwrap();
    }
    let uploads = c.stats().attachment.uploads;
    g.render(&frame_paused(&c, true), &t);
    assert_eq!(g.read(&t), shown(10, 4, 0), "the held frame");
    let s = c.stats().attachment;
    assert_eq!((s.uploads, s.paused_frames), (uploads, 1));
    assert!(s.paused);
    g.render(&frame_paused(&c, false), &t);
    assert_eq!(g.read(&t), shown(10, 4, 5), "the latest, once unpaused");
    assert!(!c.stats().attachment.paused);
}

#[test]
fn d21_a_capture_of_a_paused_picture_uploads_the_latest_and_stays_paused() {
    let Some(mut g) = gpu("live_d21") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 10, 4);
    let (_, c) = consumer(&source);
    let t = g.target(10, 4);
    g.render(&frame_paused(&c, true), &t);
    writer
        .write_frame(10, 4, &pattern(LivePixelFormat::Rgba8, 10, 4, 3), 40)
        .unwrap();
    g.render(&frame_paused(&c, true), &t);
    assert_eq!(
        g.read(&t),
        shown(10, 4, 0),
        "a present keeps the held frame"
    );
    g.capture(&frame_paused(&c, true), &t);
    assert_eq!(g.read(&t), shown(10, 4, 3), "a capture shows the latest");
    let s = c.stats().attachment;
    assert!(s.paused, "and the attachment stays paused");
    assert_eq!(s.captures, 1);
    // The texture the capture filled is what the next presented frame
    // keeps: the pause holds from there.
    g.render(&frame_paused(&c, true), &t);
    assert_eq!(g.read(&t), shown(10, 4, 3));
}

// ── D.17, D.18, D.19: captures ──

#[test]
fn d17_a_capture_then_a_present_show_the_same_and_upload_once() {
    let Some(mut g) = gpu("live_d17") else { return };
    let (source, _writer) = live(LivePixelFormat::Rgba8, 12, 6);
    let (_, c) = consumer(&source);
    let (ta, tb) = (g.target(12, 6), g.target(12, 6));
    let f = frame_1to1(&c, ScalingFilter::Nearest);
    g.capture(&f, &ta);
    g.render(&f, &tb);
    assert_eq!(g.read(&ta), g.read(&tb));
    let s = c.stats().attachment;
    assert_eq!((s.uploads, s.captures, s.frames_drawn), (1, 1, 1));
}

#[test]
fn d18_a_capture_of_a_lagging_window_shows_the_latest_frame() {
    let Some(mut g) = gpu("live_d18") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 16, 4);
    let (waker, c) = consumer(&source);
    let t = g.target(16, 4);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    for i in 0..20u32 {
        writer
            .write_rect(PixelRect::new(i % 16, i % 4, 1, 1), &solid(1, 0x33), 4)
            .unwrap();
    }
    let full = g.renderer.live_texture_stats().uploads_full;
    g.capture(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    assert_eq!(g.renderer.live_texture_stats().uploads_full, full + 1);
    let wakes = waker.count();
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[1; 4], 4)
        .unwrap();
    assert_eq!(waker.count(), wakes + 1, "the capture took the pixel flag");
    let _ = source;
}

#[test]
fn d19_a_capture_waits_for_a_held_lock_and_a_present_does_not() {
    let Some(mut g) = gpu("live_d19") else { return };
    let (source, writer) = live(LivePixelFormat::Rgba8, 8, 2);
    let (_, c) = consumer(&source);
    let t = g.target(8, 2);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[9; 4], 4)
        .unwrap();
    let (held, present_done) = (
        Arc::new(std::sync::Barrier::new(2)),
        Arc::new(std::sync::Barrier::new(2)),
    );
    let holder = {
        let (writer, held, present_done) = (writer.clone(), held.clone(), present_done.clone());
        std::thread::spawn(move || {
            let mut guard = writer.lock().unwrap();
            held.wait();
            present_done.wait();
            std::thread::sleep(Duration::from_millis(50));
            guard
                .fill_rect(PixelRect::full(8, 2), [0x77, 0x77, 0x77, 255])
                .unwrap();
            guard.commit()
        })
    };
    held.wait();
    let before = g.renderer.live_texture_stats();
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let after = g.renderer.live_texture_stats();
    assert_eq!(
        after.contended,
        before.contended + 1,
        "a present does not wait"
    );
    assert_eq!(g.read(&t), shown(8, 2, 0), "and draws the held texture");
    present_done.wait();
    g.capture(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    let committed = holder.join().unwrap();
    let captured = g.renderer.live_texture_stats();
    assert_eq!(
        captured.blocking_waits,
        after.blocking_waits + 1,
        "a capture waits"
    );
    assert_eq!(captured.contended, after.contended, "and is never busy");
    assert_eq!(g.read(&t), solid(8 * 2, 0x77), "the committed frame");
    assert_eq!(c.stats().attachment.window_generation, committed);
}

// ── the mirror decides as the GPU does ──

#[test]
fn the_mirror_and_the_gpu_make_the_same_decisions_and_show_the_same_pixels() {
    let Some(mut g) = gpu("live_differential") else {
        return;
    };
    let (source, writer) = live(LivePixelFormat::Rgba8, 24, 10);
    let (_, gc) = consumer(&source);
    let (_, mc) = consumer(&source);
    let mut mirror = LiveImageMirror::new();
    let t = g.target(24, 12);
    let step = |g: &mut Gpu, mirror: &mut LiveImageMirror| {
        g.render(&frame_1to1(&gc, ScalingFilter::Nearest), &t);
        mirror.consume(&frame_1to1(&mc, ScalingFilter::Nearest));
        assert_eq!(g.renderer.live_image_decisions(), mirror.decisions());
        let (gs, ms) = (g.renderer.live_texture_stats(), mirror.stats());
        assert_eq!(
            (
                gs.textures,
                gs.bytes,
                gs.uploads_full,
                gs.uploads_partial,
                gs.bytes_uploaded
            ),
            (
                ms.textures,
                ms.bytes,
                ms.uploads_full,
                ms.uploads_partial,
                ms.bytes_uploaded
            )
        );
        let (w, h, px) = mirror.pixels(source.id()).unwrap();
        assert_eq!(
            region(&g.read(&t), 24, PixelRect::full(w, h)),
            px,
            "{w}x{h}"
        );
    };
    step(&mut g, &mut mirror);
    writer
        .write_rect(PixelRect::new(3, 4, 5, 2), &solid(10, 0x11), 20)
        .unwrap();
    step(&mut g, &mut mirror);
    writer
        .write_frame(24, 12, &pattern(LivePixelFormat::Rgba8, 24, 12, 7), 96)
        .unwrap();
    step(&mut g, &mut mirror);
    for i in 0..3u32 {
        writer
            .write_rect(PixelRect::new(i * 3, i, 2, 2), &solid(4, 0x44), 8)
            .unwrap();
    }
    step(&mut g, &mut mirror);
}

// ── frames: live-only, inside a blur, snapped ──

#[test]
fn a_frame_with_only_a_live_picture_draws_it() {
    let Some(mut g) = gpu("live_only") else {
        return;
    };
    let (source, _writer) = live(LivePixelFormat::Bgrx8, 6, 6);
    let (_, c) = consumer(&source);
    let t = g.target(6, 6);
    let f = frame_1to1(&c, ScalingFilter::Nearest);
    assert_eq!(f.draw_order, vec![DrawCommand::LiveImage(0)]);
    g.render(&f, &t);
    assert_eq!(g.read(&t), shown(6, 6, 0));
}

#[test]
fn a_live_picture_inside_a_blurred_subtree_renders_without_an_error() {
    let Some(mut g) = gpu("live_blur") else {
        return;
    };
    let (source, _writer) = live(LivePixelFormat::Rgba8, 32, 32);
    let (_, c) = consumer(&source);
    lay_out(&c);
    let t = g.target(64, 64);
    let mut canvas = Canvas::new();
    let rect = Rect::new(8.0, 8.0, 32.0, 32.0);
    canvas.fill_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        teksilo_tokens::Color::from_rgb(0.0, 0.0, 1.0),
    );
    let mut f = canvas.into_render_frame();
    f.draw_order.push(DrawCommand::BeginBlurredSubtree {
        bounds: Rect::new(0.0, 0.0, 48.0, 48.0),
        radius: 4.0,
    });
    let mut inner = Canvas::new();
    inner.draw_live_image(&c, &LiveImageDraw::new(rect, rect));
    let inner = inner.into_render_frame();
    f.merge(&inner);
    f.draw_order.push(DrawCommand::EndBlurredSubtree);
    g.render(&f, &t);
    assert_eq!(g.renderer.live_texture_stats().uploads_full, 1);
    let out = g.read(&t);
    let centre = &out[((24 * 64 + 24) * 4) as usize..((24 * 64 + 24) * 4 + 4) as usize];
    assert_ne!(
        centre,
        &[0, 0, 255, 255][..],
        "the picture is in the blurred subtree"
    );
}

#[test]
fn a_one_to_one_picture_at_a_fractional_origin_is_snapped_to_the_pixel_grid() {
    let Some(mut g) = gpu("live_snap") else {
        return;
    };
    let (source, _writer) = live(LivePixelFormat::Rgba8, 4, 4);
    let (_, c) = consumer(&source);
    lay_out(&c);
    let t = g.target(8, 8);
    let rect = Rect::new(2.4, 1.6, 4.0, 4.0);
    let mut canvas = Canvas::new();
    canvas.draw_live_image(&c, &LiveImageDraw::new(rect, rect));
    g.render(&canvas.into_render_frame(), &t);
    // Rounded to (2, 2): a 1:1 copy, no texel blended with its neighbour.
    assert_eq!(
        region(&g.read(&t), 8, PixelRect::new(2, 2, 4, 4)),
        shown(4, 4, 0)
    );
}

// ── a spread fill, and the reclaim poll ──

#[test]
fn a_frame_above_the_staging_budget_fills_over_several_frames_and_reads_back_whole() {
    let Some(mut g) = gpu("live_spread") else {
        return;
    };
    let (source, writer) = live(LivePixelFormat::Rgba8, 16, 16);
    let (waker, c) = consumer(&source);
    let t = g.target(16, 20);
    g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
    // A 16-pixel row stages one 256-byte pitch: five rows a frame.
    g.renderer.set_live_staging_budget(256 * 5);
    writer
        .write_frame(16, 20, &pattern(LivePixelFormat::Rgba8, 16, 20, 6), 64)
        .unwrap();
    let mut frames = 0;
    while g.renderer.live_texture_stats().textures != 1
        || g.renderer
            .live_image_decisions()
            .first()
            .is_none_or(|d| !d.draw)
        || c.stats().attachment.window_generation != source.generation()
    {
        let before = waker.count();
        g.render(&frame_1to1(&c, ScalingFilter::Nearest), &t);
        frames += 1;
        assert!(frames < 10, "the fill makes progress");
        if c.stats().attachment.window_generation != source.generation() {
            assert_eq!(
                waker.count(),
                before + 1,
                "a partial frame asks for the next"
            );
        }
    }
    assert!(frames > 1, "spread over {frames} frames");
    assert_eq!(g.read(&t), shown(16, 20, 6));
}
