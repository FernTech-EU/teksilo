// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An offscreen capture of a headless app draws its text.
//!
//! `Renderer::render` does not upload the glyph atlas, so a capture that only
//! renders the tree draws every glyph blank. `HeadlessApp::render_for_capture`
//! uploads it first. Needs a GPU adapter; returns early without one.

#![cfg(feature = "text")]

use teksilo_app::TeksiloAppBuilder;
use teksilo_canvas::SizeProposal;
use teksilo_i18n::lit;
use teksilo_tokens::Color;
use teksilo_widgets::TextWidget;

const W: u32 = 160;
const H: u32 = 40;

/// Render `frame` through `renderer` into a transparent target and count the
/// pixels with any coverage.
fn covered_pixels(
    renderer: &mut teksilo_render::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &teksilo_canvas::RenderFrame,
) -> usize {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless_capture_text"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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
    renderer.render(frame, &view, 1.0, W, H, [0.0, 0.0, 0.0, 0.0]);
    let pixels = teksilo_render::test_support::read_texture_rgba(device, queue, &texture, W, H);
    pixels.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count()
}

fn text_app() -> teksilo_app::HeadlessApp {
    let mut app = TeksiloAppBuilder::new().build_headless();
    app.tree
        .add(TextWidget::new(lit!("Capture me")).color(Color::BLACK));
    app.tree.layout(SizeProposal::exact(W as f32, H as f32));
    app
}

#[test]
fn a_capture_draws_text_rasterised_by_its_own_render() {
    let Some((mut renderer, device, queue)) = pollster::block_on(
        teksilo_render::test_support::create_test_renderer("headless_capture_text"),
    ) else {
        return; // no GPU adapter — skip.
    };

    let mut app = text_app();
    let mut atlas_version = 0;
    let frame = app.render_for_capture(&mut renderer, &mut atlas_version);
    assert!(!frame.glyphs.is_empty(), "the label emitted glyph quads");
    assert_ne!(atlas_version, 0, "the atlas was uploaded");
    assert!(
        covered_pixels(&mut renderer, &device, &queue, &frame) > 0,
        "the label's glyphs reach the capture"
    );

    // An unchanged atlas is not uploaded again: the bridge, asked from the
    // version the renderer holds, has no new pixels to hand over.
    let uploaded = atlas_version;
    let _ = app.render_for_capture(&mut renderer, &mut atlas_version);
    assert_eq!(atlas_version, uploaded);
    let typesetter = app
        .tree
        .app_context()
        .app_state::<teksilo_app::teksilo_text::SharedTypesetter>()
        .cloned()
        .expect("build_headless registers the typesetter");
    let info = typesetter.bridge().borrow_mut().atlas_info(atlas_version);
    assert!(
        info.pixels.is_empty(),
        "the renderer already holds the current atlas"
    );
}

#[test]
fn a_bare_render_draws_no_text_which_is_why_captures_upload_the_atlas() {
    let Some((mut renderer, device, queue)) = pollster::block_on(
        teksilo_render::test_support::create_test_renderer("headless_capture_text_bare"),
    ) else {
        return; // no GPU adapter — skip.
    };
    let mut app = text_app();
    let frame = app.tree.render();
    assert!(!frame.glyphs.is_empty());
    assert_eq!(
        covered_pixels(&mut renderer, &device, &queue, &frame),
        0,
        "without the atlas upload every glyph samples an empty atlas"
    );
}
