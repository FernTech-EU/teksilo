// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A headless app's own test of a live picture, rendered offscreen (spec
//! D.13): the path an application's test takes, `build_headless`, `layout`,
//! `render`, then a renderer and a readback. It catches a stale texture or a
//! stale cached frame; a lost wake is the window's business. Needs a GPU
//! adapter; returns early without one unless `TEKSILO_TEST_REQUIRE_ADAPTER`
//! is set.

use teksilo_app::TeksiloAppBuilder;
use teksilo_canvas::SizeProposal;
use teksilo_widgets::primitives::live_image::{
    LiveImage, LiveImageSource, LivePixelFormat, ScalingFilter,
};

const SIDE: u32 = 16;

fn solid(rgba: [u8; 4]) -> Vec<u8> {
    rgba.repeat((SIDE * SIDE) as usize)
}

/// Render `frame` into a fresh offscreen target and read it back.
fn draw(
    renderer: &mut teksilo_render::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &teksilo_canvas::RenderFrame,
) -> Vec<u8> {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("live_image_offscreen"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.render(frame, &view, 1.0, SIDE, SIDE, [0.0, 0.0, 0.0, 0.0]);
    teksilo_render::test_support::read_texture_rgba(device, queue, &texture, SIDE, SIDE)
}

#[test]
fn d13_a_headless_app_shows_each_commit_without_repainting() {
    let Some((mut renderer, device, queue)) = pollster::block_on(
        teksilo_render::test_support::require_test_renderer("live_image_offscreen"),
    ) else {
        return; // no GPU adapter — skip.
    };

    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer
        .write_frame(SIDE, SIDE, &solid([255, 0, 0, 255]), (SIDE * 4) as usize)
        .unwrap();
    let image = LiveImage::new(source)
        .size(SIDE as f32, SIDE as f32)
        .scaling(ScalingFilter::Nearest)
        .alt("Screen");
    let handle = image.handle();
    let mut app = TeksiloAppBuilder::new().build_headless();
    app.tree.add(image);
    let at = SizeProposal::exact(SIDE as f32, SIDE as f32);

    app.tree.layout(at);
    let frame = app.tree.render();
    assert_eq!(
        draw(&mut renderer, &device, &queue, &frame),
        solid([255, 0, 0, 255]),
        "the first commit"
    );
    drop(frame);
    let paints = handle.stats().attachment.paints;

    // A second writer of the same session: the source stays live while
    // either is held, and frees its pixels when the last one drops.
    let producer = writer.source().writer();
    std::thread::spawn(move || {
        producer
            .write_frame(SIDE, SIDE, &solid([0, 0, 255, 255]), (SIDE * 4) as usize)
            .unwrap();
    })
    .join()
    .expect("the producer");
    app.tree.layout(at);
    let frame = app.tree.render();
    assert_eq!(
        draw(&mut renderer, &device, &queue, &frame),
        solid([0, 0, 255, 255]),
        "the commit made on another thread, no stale texture or frame"
    );
    assert_eq!(
        handle.stats().attachment.paints,
        paints,
        "and the widget did not paint for it"
    );
}
