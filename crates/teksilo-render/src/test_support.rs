// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::sync::{OnceLock, mpsc};

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

/// The process-wide GPU device every offscreen renderer shares.
///
/// One device per process, not one per caller. Two D3D12 **WARP** devices
/// rasterizing at the same time fault inside `d3d10warp.dll` — Microsoft's
/// software rasterizer, which is exactly what a GPU-less Windows host and the
/// CI runners use — so a device per caller turned any two concurrent offscreen
/// renders into a crash the faulting-module log pins on WARP itself, not on
/// wgpu or on us. It is not something we can fix downstream; the only remedy is
/// to stop creating the second device.
///
/// Sharing is also simply right: a GPU device is a process-level resource, and
/// nothing here ever wanted a private one. Callers still get their **own**
/// [`Renderer`] — that is where the glyph and path atlases live, so no caller
/// can see another's cached glyphs.
///
/// `None` means this host can open no usable device at all; it is cached too,
/// so a GPU-less machine pays the failed search once rather than per call.
static SHARED_DEVICE: OnceLock<Option<(wgpu::Device, wgpu::Queue)>> = OnceLock::new();

/// Open the one device, searching for an adapter that actually yields one.
///
/// Adapter selection is a *search*, not a single request. A host can enumerate
/// an adapter it cannot actually open — a VM's OpenGL driver is the common
/// case — while a perfectly good software device sits behind
/// `force_fallback_adapter`. Treating the first `request_device` failure as
/// fatal reports "no GPU" on a machine that has one, which is what made
/// screenshots unavailable on GPU-less Windows hosts and CI runners (where
/// DX12 WARP is present and works). So: try the preferred adapter, then an
/// explicit software fallback, and only give up when neither yields a device.
async fn open_shared_device(label: &'static str) -> Option<(wgpu::Device, wgpu::Queue)> {
    #[cfg(test)]
    DEVICE_OPENS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            return Some((device, queue));
        }
    }
    None
}

/// Build an offscreen renderer on the shared device, or `None` if this host can
/// open no usable GPU device at all.
///
/// The [`Renderer`] is fresh per call; the device and queue behind it are
/// shared process-wide, which is load-bearing rather than an optimisation: two
/// D3D12 **WARP** devices rasterizing at once fault inside Microsoft's software
/// rasterizer — exactly what a GPU-less Windows host and the CI runners use —
/// so a device per caller turns any two concurrent offscreen renders into a
/// crash. Atlases still live on the per-call `Renderer`, so no caller can see
/// another's cached glyphs.
///
/// `label` names the device, so it only takes effect on the call that actually
/// opens it; later callers join a device someone else already named.
pub async fn create_test_renderer(
    label: &'static str,
) -> Option<(Renderer, wgpu::Device, wgpu::Queue)> {
    let (device, queue) = shared_device(label)?;
    let renderer = Renderer::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    Some((renderer, device.clone(), queue.clone()))
}

/// The shared device, opening it on the first call.
///
/// Synchronous on purpose. `OnceLock::get_or_init` gives "exactly one caller
/// runs the initialiser, the rest wait" for free, and opening a GPU device is
/// blocking work whichever way it is spelled — every caller already reaches
/// this through `pollster::block_on`. The alternative, holding a lock across
/// the `await` inside an async fn, is the shape `clippy::await_holding_lock`
/// warns about, and it would deadlock the first caller that ever drove this
/// from a single-threaded executor.
fn shared_device(label: &'static str) -> Option<&'static (wgpu::Device, wgpu::Queue)> {
    SHARED_DEVICE
        .get_or_init(|| pollster::block_on(open_shared_device(label)))
        .as_ref()
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

/// How many times a GPU device has actually been opened in this process.
///
/// Exists only so [`exactly_one_device_is_opened_per_process`] can assert the
/// invariant the WARP crash depends on.
#[cfg(test)]
static DEVICE_OPENS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
mod shared_device_tests {
    use super::*;

    /// Every caller must land on the SAME device, even under contention.
    ///
    /// This is the invariant that keeps the offscreen renderer alive on a
    /// GPU-less Windows host. Two D3D12 WARP devices rasterizing concurrently
    /// fault inside `d3d10warp.dll`, which no amount of care on our side can
    /// catch — it is a wild access violation in Microsoft's software
    /// rasterizer, so the process dies mid-test. The only defence is to never
    /// open the second device, and that is what this pins.
    ///
    /// Asserted through a counter rather than by comparing handles because
    /// `wgpu::Device` exposes no identity: cloning is the supported way to
    /// share one, so two clones are indistinguishable from two devices at the
    /// type level — exactly the confusion that let a second device appear.
    #[test]
    fn exactly_one_device_is_opened_per_process() {
        use std::sync::atomic::Ordering;

        // Race several threads at the initialiser; `OnceLock` plus the init
        // lock must let exactly one of them reach `open_shared_device`.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    pollster::block_on(create_test_renderer("shared-device-test")).is_some()
                })
            })
            .collect();
        let got: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Either this host has a device and every caller got one, or it has
        // none and nobody did — never a mix.
        assert!(
            got.iter().all(|g| *g) || got.iter().all(|g| !*g),
            "callers disagreed about whether a GPU exists: {got:?}"
        );

        let opens = DEVICE_OPENS.load(Ordering::Relaxed);
        assert_eq!(
            opens, 1,
            "the device must be opened exactly once per process, not {opens} times - a second \
             concurrent WARP device is an access violation inside d3d10warp.dll, not a slowdown"
        );
    }
}
