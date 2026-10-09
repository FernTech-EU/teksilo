// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What needs a device of its own: the lost-device latch (losing the shared
//! test device would fail every other GPU test of the process) and the
//! reclaim poll (every other renderer on the shared device flags it as it
//! drops). One test, so this binary never holds two devices at once (two
//! D3D12 WARP devices rasterizing together fault on Windows).

use teksilo_canvas::live_image::{LiveImageDraw, LiveImageSource, LivePixelFormat, PixelRect};
use teksilo_canvas::{Canvas, Rect};
use teksilo_render::{DeviceHealth, Renderer};

fn private_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    descriptor.flags = teksilo_render::instance_flags();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .ok()?;
    let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("live_device_health"),
        required_limits: limits,
        ..Default::default()
    }))
    .ok()
}

#[test]
fn reclaim_then_a_lost_device_then_a_destroyed_one() {
    let Some((device, queue)) = private_device() else {
        assert!(
            !teksilo_render::test_support::adapter_required(),
            "an adapter is required and none opened"
        );
        return;
    };

    // The latch is per device: every renderer on it reads one flag.
    let health = DeviceHealth::install(&device);
    let mut renderer = Renderer::with_device_health(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
        health.clone(),
    );
    let other = Renderer::with_device_health(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
        health.clone(),
    );
    assert!(!health.is_lost());

    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer.write_frame(4, 4, &[200; 64], 16).unwrap();
    let consumer = source.attach(None);
    let frame = || {
        let _ = consumer.take_geometry();
        consumer.record_layout_meta(source.meta());
        let rect = Rect::new(0.0, 0.0, 4.0, 4.0);
        let mut canvas = Canvas::new();
        canvas.draw_live_image(&consumer, &LiveImageDraw::new(rect, rect));
        canvas.into_render_frame()
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("live_device_health_target"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
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
    renderer.render(&frame(), &view, 1.0, 4, 4, [0.0; 4]);
    let healthy = consumer.stats().attachment;
    assert_eq!((healthy.frames_drawn, healthy.uploads), (1, 1));

    // A frame without the picture drops its texture: the device is flagged
    // for the reclaim poll, which empties it without a frame.
    renderer.set_live_park_budget(0);
    renderer.render(
        &teksilo_canvas::RenderFrame::new(),
        &view,
        1.0,
        4,
        4,
        [0.0; 4],
    );
    assert_eq!(renderer.live_texture_stats().textures, 0);
    assert_eq!(
        teksilo_render::gpu_reclaim::pending_reclaims(),
        1,
        "flagged"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while teksilo_render::poll_gpu_reclaim() {
        assert!(
            std::time::Instant::now() < deadline,
            "the device never emptied"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(teksilo_render::gpu_reclaim::pending_reclaims(), 0);

    // Two windows on the device: one flags a submission still on the GPU,
    // then the other, closing, flags its own last one, older and complete.
    // The record waits for the newer.
    let older = queue.submit([]);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(older.clone()),
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .unwrap();
    let buffer = || {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("live_device_health_busy"),
            size: 8 << 20,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    let (a, b) = (buffer(), buffer());
    let mut busy = device.create_command_encoder(&Default::default());
    for _ in 0..256 {
        busy.copy_buffer_to_buffer(&a, 0, &b, 0, 8 << 20);
        busy.copy_buffer_to_buffer(&b, 0, &a, 0, 8 << 20);
    }
    let newer = queue.submit([busy.finish()]);
    teksilo_render::gpu_reclaim::flag_device(&health, &device, newer.clone());
    teksilo_render::gpu_reclaim::flag_device(&health, &device, older);
    let pending = teksilo_render::poll_gpu_reclaim();
    let still_running = matches!(
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(newer.clone()),
            timeout: Some(std::time::Duration::ZERO),
        }),
        Err(wgpu::PollError::Timeout)
    );
    assert!(still_running, "the copies outlast the poll");
    assert!(
        pending,
        "the record waits for the newer submission, flagged first"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while teksilo_render::poll_gpu_reclaim() {
        assert!(
            std::time::Instant::now() < deadline,
            "the copies never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(teksilo_render::gpu_reclaim::pending_reclaims(), 0);
    renderer.render(&frame(), &view, 1.0, 4, 4, [0.0; 4]);

    // Lost: nothing advances, so a probe sees the stream stall.
    renderer.device_health().mark_lost_for_testing();
    assert!(health.is_lost(), "one latch for the device");
    assert!(
        other.device_health().is_lost(),
        "shared by every renderer on it"
    );
    drop(other);
    writer
        .write_rect(PixelRect::new(0, 0, 1, 1), &[9; 4], 4)
        .unwrap();
    renderer.render(&frame(), &view, 1.0, 4, 4, [0.0; 4]);
    let lost = consumer.stats().attachment;
    assert_eq!(
        (lost.frames_drawn, lost.uploads, lost.window_generation),
        (
            healthy.frames_drawn + 1,
            healthy.uploads + 1,
            healthy.window_generation
        ),
        "after the last healthy frame, nothing advances: a probe sees the stream stall"
    );
    assert!(renderer.live_texture_stats().device_lost);
    drop(renderer);

    // A device destroyed on purpose is not lost.
    // wgpu compares devices by an id another instance can share: this
    // device may compare equal to the first, and still has a latch of its
    // own.
    let (device, queue) = private_device().expect("opened once already");
    let renderer = Renderer::new(device.clone(), queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    let health = renderer.device_health().clone();
    assert!(
        !health.is_lost(),
        "a device of its own, not the first one's latch"
    );
    device.destroy();
    let _ = device.poll(wgpu::PollType::Poll);
    assert!(!health.is_lost(), "Destroyed is not a loss");
    drop(renderer);
}
