// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The mip chain of a live picture drawn with `ScalingFilter::Trilinear`, on
//! the GPU (spec D.22-D.25): each level is the CPU kernel's halving of the
//! level below, a footprint rebuild equals a whole one byte for byte, one
//! texture serves a `Trilinear` and a `Linear` quad, and the texture's shape
//! changes only when it must.
//!
//! Each test returns early without an adapter unless
//! `TEKSILO_TEST_REQUIRE_ADAPTER` insists on one; every render runs inside a
//! validation error scope that must come back empty. To run them on Mesa's
//! lavapipe on a machine with a GPU:
//!
//! ```sh
//! VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json WGPU_BACKEND=vulkan \
//!     TEKSILO_TEST_REQUIRE_ADAPTER=lavapipe cargo test -p teksilo-render --test live_mips
//! ```

use teksilo_canvas::live_image::internal::{mip_bytes, mip_levels};
use teksilo_canvas::live_image::{
    LiveImageConsumer, LiveImageDraw, LiveImageSource, LiveImageWriter, LivePixelFormat, PixelRect,
    ScalingFilter,
};
use teksilo_canvas::resample::{downsample_half, downsample_half_opaque};
use teksilo_canvas::{Canvas, Rect, RenderFrame};
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
            label: Some("live_mips_target"),
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
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        self.renderer.render(
            frame,
            &target.view,
            1.0,
            target.width,
            target.height,
            [0.0, 0.0, 0.0, 1.0],
        );
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

    fn level(&self, source: &LiveImageSource, k: u32) -> (u32, u32, Vec<u8>) {
        self.renderer
            .read_live_texture_level(source.id(), k)
            .unwrap_or_else(|| panic!("level {k} of {source:?}"))
    }
}

fn live(format: LivePixelFormat, w: u32, h: u32, px: &[u8]) -> (LiveImageSource, LiveImageWriter) {
    let source = LiveImageSource::new(format);
    let writer = source.writer();
    writer.write_frame(w, h, px, (w * 4) as usize).unwrap();
    (source, writer)
}

fn lay_out(c: &LiveImageConsumer) {
    let _ = c.take_geometry();
    c.record_layout_meta(c.source().meta());
}

/// A frame drawing each `(consumer, rect, filter)`, laid out first.
fn frame(quads: &[(&LiveImageConsumer, Rect, ScalingFilter)]) -> RenderFrame {
    let mut canvas = Canvas::new();
    for (c, rect, filter) in quads {
        lay_out(c);
        canvas.draw_live_image(c, &LiveImageDraw::new(*rect, *rect).filter(*filter));
    }
    canvas.into_render_frame()
}

/// Straight-alpha pixels with transparent and half-transparent texels.
fn speckled(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let a = match i % 7 {
                0 => 0,
                3 => 128,
                _ => 255,
            };
            px.extend_from_slice(&[
                (x as u8).wrapping_mul(7).wrapping_add(seed),
                (y as u8).wrapping_mul(5),
                seed.wrapping_mul(3),
                a,
            ]);
        }
    }
    px
}

/// Whether two buffers agree within `tolerance` per channel.
fn within(a: &[u8], b: &[u8], tolerance: u8) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= tolerance)
}

// ── D.22: each level is the CPU kernel's halving of the GPU's level below ──

fn check_chain(label: &'static str, format: LivePixelFormat, w: u32, h: u32, px: &[u8]) {
    let Some(mut g) = gpu(label) else { return };
    let (source, _writer) = live(format, w, h, px);
    let c = source.attach(None);
    let t = g.target(64, 64);
    g.render(
        &frame(&[(
            &c,
            Rect::new(0.0, 0.0, 40.0, 40.0),
            ScalingFilter::Trilinear,
        )]),
        &t,
    );
    let opaque = format.is_opaque();
    let top = mip_levels(w, h).min(5);
    assert!(top >= 2, "{w}x{h} has a level to check");
    for k in 1..top {
        let (bw, bh, below) = g.level(&source, k - 1);
        let (rw, rh, reference) = if opaque && k == 1 {
            downsample_half_opaque(&below, bw, bh)
        } else {
            downsample_half(&below, bw, bh)
        };
        let (lw, lh, level) = g.level(&source, k);
        assert_eq!((lw, lh), (rw, rh), "{label}: level {k} size");
        assert!(
            within(&level, &reference, 1),
            "{label}: level {k} differs from the kernel by more than 1"
        );
        if opaque {
            assert!(
                level.chunks(4).all(|t| t[3] == 255),
                "{label}: level {k} is opaque, exactly"
            );
        }
    }
}

#[test]
fn d22_levels_follow_the_cpu_kernel_on_straight_alpha_sources() {
    check_chain(
        "live_d22_720x1280",
        LivePixelFormat::Rgba8,
        720,
        1280,
        &speckled(720, 1280, 1),
    );
    check_chain(
        "live_d22_719x37",
        LivePixelFormat::Rgba8,
        719,
        37,
        &speckled(719, 37, 2),
    );
    check_chain(
        "live_d22_5x3",
        LivePixelFormat::Rgba8,
        5,
        3,
        &speckled(5, 3, 3),
    );
}

#[test]
fn d22_an_x_byte_of_zero_builds_an_opaque_chain() {
    // What QEMU hands over: BGRX with the fourth byte 0.
    let mut px = speckled(97, 61, 4);
    for texel in px.chunks_mut(4) {
        texel[3] = 0;
    }
    check_chain("live_d22_bgrx", LivePixelFormat::Bgrx8, 97, 61, &px);
}

// ── D.23: a footprint rebuild leaves what a whole rebuild leaves ──

#[test]
fn d23_after_a_rect_upload_every_level_equals_a_whole_rebuild() {
    let Some(mut a) = gpu("live_d23_rect") else {
        return;
    };
    let Some(mut b) = gpu("live_d23_whole") else {
        return;
    };
    let (w, h) = (720, 1280);
    let (source, writer) = live(LivePixelFormat::Rgba8, w, h, &speckled(w, h, 5));
    let ca = source.attach(None);
    let quad = Rect::new(0.0, 0.0, 40.0, 70.0);
    let t = a.target(64, 80);
    a.render(&frame(&[(&ca, quad, ScalingFilter::Trilinear)]), &t);
    let rect = PixelRect::new(101, 203, 297, 311);
    writer
        .write_rect(
            rect,
            &speckled(rect.width, rect.height, 9),
            rect.width as usize * 4,
        )
        .unwrap();
    let full = a.renderer.live_texture_stats().uploads_full;
    a.render(&frame(&[(&ca, quad, ScalingFilter::Trilinear)]), &t);
    assert_eq!(
        a.renderer.live_texture_stats().uploads_full,
        full,
        "a rect upload, not a whole one"
    );
    assert_eq!(a.renderer.live_texture_stats().mip_updates, 2);

    // Another renderer builds the whole chain of the same frame.
    let cb = source.attach(None);
    b.render(&frame(&[(&cb, quad, ScalingFilter::Trilinear)]), &t);
    for k in 0..mip_levels(w, h) {
        assert_eq!(
            a.level(&source, k),
            b.level(&source, k),
            "level {k} differs between the footprint and the whole rebuild"
        );
    }
}

// ── D.24: one texture for a `Trilinear` and a `Linear` quad ──

/// A one-pixel checkerboard of black and white: its average is 0.5 in
/// linear light, 188 once encoded.
fn checkerboard(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let v = if ((i % w) + (i / w)).is_multiple_of(2) {
                0
            } else {
                255
            };
            [v, v, v, 255]
        })
        .collect()
}

fn region(px: &[u8], width: u32, rect: PixelRect) -> Vec<u8> {
    let mut out = Vec::new();
    for y in rect.y..rect.y + rect.height {
        let at = ((y * width + rect.x) * 4) as usize;
        out.extend_from_slice(&px[at..at + rect.width as usize * 4]);
    }
    out
}

#[test]
fn d24_a_thumbnail_and_a_main_view_share_one_mipped_texture() {
    let Some(mut g) = gpu("live_d24") else { return };
    let (w, h) = (720, 1280);
    let (source, _writer) = live(LivePixelFormat::Rgba8, w, h, &checkerboard(w, h));
    let (thumb, main) = (source.attach(None), source.attach(None));
    let t = g.target(200, 180);
    let thumb_rect = Rect::new(0.0, 0.0, 97.0, 173.0);
    let main_rect = Rect::new(100.0, 0.0, 97.0, 173.0);
    g.render(
        &frame(&[
            (&thumb, thumb_rect, ScalingFilter::Trilinear),
            (&main, main_rect, ScalingFilter::Linear),
        ]),
        &t,
    );
    let stats = g.renderer.live_texture_stats();
    assert_eq!((stats.textures, stats.uploads_full), (1, 1));
    assert_eq!(stats.bytes, mip_bytes(w, h, mip_levels(w, h)));
    let px = g.read(&t);
    let shown = region(&px, 200, PixelRect::new(0, 0, 97, 173));
    assert!(
        shown
            .chunks(4)
            .all(|t| t[..3].iter().all(|v| v.abs_diff(188) <= 1)),
        "the thumbnail averages the checkerboard everywhere"
    );

    // The `Linear` quad draws as it does with no `Trilinear` beside it.
    let Some(mut alone) = gpu("live_d24_alone") else {
        return;
    };
    let single = source.attach(None);
    alone.render(&frame(&[(&single, main_rect, ScalingFilter::Linear)]), &t);
    assert_eq!(
        alone.renderer.live_texture_stats().bytes,
        u64::from(w * h * 4)
    );
    let main_px = PixelRect::new(100, 0, 97, 173);
    assert_eq!(
        region(&px, 200, main_px),
        region(&alone.read(&t), 200, main_px),
        "level 0 alone, aliasing as `Linear` promises"
    );
}

// ── D.25: the texture's shape changes only when it must ──

#[test]
fn d25_trilinear_restages_once_and_the_chain_stays_when_it_goes() {
    let Some(mut g) = gpu("live_d25") else { return };
    let (w, h) = (64, 40);
    let (source, writer) = live(LivePixelFormat::Rgba8, w, h, &speckled(w, h, 1));
    let c = source.attach(None);
    let t = g.target(32, 20);
    let quad = Rect::new(0.0, 0.0, 16.0, 10.0);
    let stats = |g: &Gpu| g.renderer.live_texture_stats();

    g.render(&frame(&[(&c, quad, ScalingFilter::Linear)]), &t);
    assert_eq!(
        (stats(&g).bytes, stats(&g).uploads_full),
        (u64::from(w * h * 4), 1)
    );

    // `Trilinear` appears: a new texture with a chain, one whole upload.
    g.render(&frame(&[(&c, quad, ScalingFilter::Trilinear)]), &t);
    let mipped = mip_bytes(w, h, mip_levels(w, h));
    assert_eq!((stats(&g).bytes, stats(&g).uploads_full), (mipped, 2));
    assert_eq!((stats(&g).textures, stats(&g).mip_updates), (1, 1));

    // It goes: the texture keeps its levels, and a commit uploads into it
    // without rebuilding them.
    writer
        .write_rect(PixelRect::new(3, 4, 5, 6), &speckled(5, 6, 8), 20)
        .unwrap();
    g.render(&frame(&[(&c, quad, ScalingFilter::Linear)]), &t);
    let s = stats(&g);
    assert_eq!((s.bytes, s.uploads_full, s.uploads_partial), (mipped, 2, 1));
    assert_eq!(s.mip_updates, 1);

    // It returns: nothing to upload, the chain catches up.
    g.render(&frame(&[(&c, quad, ScalingFilter::Trilinear)]), &t);
    let s = stats(&g);
    assert_eq!(
        (s.uploads_full, s.uploads_partial, s.mip_updates),
        (2, 1, 2)
    );
    let (bw, bh, below) = g.level(&source, 0);
    let (_, _, level1) = g.level(&source, 1);
    assert!(within(&level1, &downsample_half(&below, bw, bh).2, 1));
}
