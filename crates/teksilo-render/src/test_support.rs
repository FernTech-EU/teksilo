// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::sync::mpsc;

use crate::Renderer;

/// Why an offscreen readback could not produce pixels.
///
/// Every variant means the same thing to a caller — no image this time — but
/// they are kept apart because they point at different causes: a lost device
/// is a driver / compositor event, a failed map is usually memory pressure.
#[derive(Debug, thiserror::Error)]
pub enum ReadbackError {
    /// `map_async`'s callback was dropped without firing — the device was lost
    /// before the mapping completed.
    #[error("readback failed: the GPU device was lost while mapping")]
    DeviceLost,
    /// The buffer mapping itself failed.
    #[error("readback failed: buffer mapping was refused ({0})")]
    MapFailed(String),
    /// `poll` reported a failure before the mapping could be observed.
    #[error("readback failed: polling the device failed ({0})")]
    PollFailed(String),
}

/// Build an offscreen renderer plus its device/queue, or `None` if this host
/// can open no usable GPU device at all.
///
/// Adapter selection is a *search*, not a single request. A host can enumerate
/// an adapter it cannot actually open — a VM's OpenGL driver is the common
/// case — while a perfectly good software device sits behind
/// `force_fallback_adapter`. Treating the first `request_device` failure as
/// fatal reports "no GPU" on a machine that has one, which is what made
/// screenshots unavailable on GPU-less Windows hosts and CI runners (where
/// DX12 WARP is present and works). So: try the preferred adapter, then an
/// explicit software fallback, and only give up when neither yields a device.
pub async fn create_test_renderer(
    label: &'static str,
) -> Option<(Renderer, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    for force_fallback_adapter in [false, true] {
        let Ok(adapter) = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter,
                ..Default::default()
            })
            .await
        else {
            continue;
        };

        // `downlevel_defaults` caps `max_texture_dimension_2d` at 2048, but the
        // path atlas grows to 4096 — so a path-heavy frame would fail offscreen
        // while rendering fine in a live window (which uses `Limits::default`).
        // `using_resolution` lifts exactly the resolution limits to whatever
        // this adapter really supports, keeping every other downlevel bound.
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());

        if let Ok((device, queue)) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(label),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
        {
            let renderer = Renderer::new(
                device.clone(),
                queue.clone(),
                wgpu::TextureFormat::Rgba8UnormSrgb,
            );
            return Some((renderer, device, queue));
        }
    }

    None
}

/// Read a texture back as tightly-packed RGBA, panicking on GPU failure.
///
/// Kept for tests, where a lost device is a test failure and a panic is the
/// clearest report. Anything user-facing — a screenshot tool that must survive
/// a driver restart — should call [`try_read_texture_rgba`] instead.
pub fn read_texture_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    try_read_texture_rgba(device, queue, texture, width, height).expect("texture readback failed")
}

/// Read a texture back as tightly-packed RGBA.
///
/// The GPU copy needs each row aligned to
/// [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`]; the padding is added for the copy
/// and stripped back out here, so the returned buffer is exactly
/// `width * height * 4` bytes with no stride.
pub fn try_read_texture_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ReadbackError> {
    let bytes_per_pixel = 4u32;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer_size = padded_bytes_per_row as u64 * height as u64;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("teksilo_render_test_readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("teksilo_render_test_copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| ReadbackError::PollFailed(e.to_string()))?;
    rx.recv()
        .map_err(|_| ReadbackError::DeviceLost)?
        .map_err(|e| ReadbackError::MapFailed(e.to_string()))?;

    let mapped = slice
        .get_mapped_range()
        .map_err(|e| ReadbackError::MapFailed(e.to_string()))?;
    let mut pixels = vec![0u8; (width * height * bytes_per_pixel) as usize];
    for row in 0..height as usize {
        let src_offset = row * padded_bytes_per_row as usize;
        let dst_offset = row * unpadded_bytes_per_row as usize;
        pixels[dst_offset..dst_offset + unpadded_bytes_per_row as usize]
            .copy_from_slice(&mapped[src_offset..src_offset + unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}
