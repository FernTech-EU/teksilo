// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live pictures on the GPU: the wgpu backend of the live pass
//! (`teksilo_canvas::live_image::internal::LivePass`), which decides what to
//! upload and when. This module only creates textures, writes bands of a
//! locked source into them, and readies their bind groups.
//!
//! Textures are `Rgba8UnormSrgb` whatever the source's byte order: the quad
//! shader swaps red and blue for a BGR source and ignores the fourth byte of
//! an RGBX one, so no producer converts pixels and no extension is needed.

use teksilo_canvas::PixelRect;
use teksilo_canvas::live_image::ScalingFilter;
use teksilo_canvas::live_image::internal::{LiveImageRead, LiveTextureBackend, TextureOutOfMemory};

use crate::device_health::DeviceHealth;

/// One live texture and what drawing it needs.
pub(crate) struct WgpuTexture {
    pub(crate) texture: wgpu::Texture,
    width: u32,
    height: u32,
    view: Option<wgpu::TextureView>,
    /// One bind group per filter, built outside every lock.
    groups: [Option<wgpu::BindGroup>; 3],
}

impl WgpuTexture {
    /// The bind group to draw it with `filter`, once readied.
    pub(crate) fn bind_group(&self, filter: ScalingFilter) -> Option<&wgpu::BindGroup> {
        self.groups[filter_index(filter)].as_ref()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// The backend: the window's device and queue, the samplers, and the
/// quad pipeline's texture layout every live bind group is built against.
pub(crate) struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    health: DeviceHealth,
    max_dimension: u32,
    /// A texture was dropped since the renderer last flagged its device for
    /// the reclaim poll.
    released: bool,
    /// Test hook: the next creation reports no memory.
    fail_next_create: bool,
    /// Creations refused for another reason than memory, logged once.
    refusal_logged: bool,
}

impl WgpuBackend {
    pub(crate) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        layout: wgpu::BindGroupLayout,
        health: DeviceHealth,
    ) -> Self {
        let sampler = |label: &'static str, filter: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            })
        };
        let linear = sampler("live_linear", wgpu::FilterMode::Linear);
        let nearest = sampler("live_nearest", wgpu::FilterMode::Nearest);
        let max_dimension = device.limits().max_texture_dimension_2d;
        Self {
            device,
            queue,
            layout,
            linear,
            nearest,
            health,
            max_dimension,
            released: false,
            fail_next_create: false,
            refusal_logged: false,
        }
    }

    /// Whether a texture was dropped since the last call.
    pub(crate) fn take_released(&mut self) -> bool {
        std::mem::take(&mut self.released)
    }

    pub(crate) fn set_max_dimension(&mut self, max: u32) {
        self.max_dimension = max.min(self.device.limits().max_texture_dimension_2d);
    }

    pub(crate) fn fail_next_create(&mut self) {
        self.fail_next_create = true;
    }

    pub(crate) fn health(&self) -> &DeviceHealth {
        &self.health
    }
}

fn filter_index(filter: ScalingFilter) -> usize {
    match filter {
        ScalingFilter::Nearest => 1,
        ScalingFilter::Trilinear => 2,
        // `Linear`, and any filter a later canvas adds: the default sampling.
        _ => 0,
    }
}

impl LiveTextureBackend for WgpuBackend {
    type Texture = WgpuTexture;

    fn max_dimension(&self) -> u32 {
        self.max_dimension
    }

    /// Inside an `OutOfMemory` scope, itself inside a `Validation` one: a
    /// device that cannot hold the texture reports it as a value instead of
    /// to wgpu's default handler, which panics. Views and bind groups are
    /// made only for a texture that exists, when it is first drawn.
    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        label: &str,
    ) -> Result<WgpuTexture, TextureOutOfMemory> {
        if std::mem::take(&mut self.fail_next_create) {
            return Err(TextureOutOfMemory);
        }
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // A native backend's pop returns a ready future.
        let out_of_memory = pollster::block_on(memory.pop());
        let invalid = pollster::block_on(validation.pop());
        if out_of_memory.is_some() {
            return Err(TextureOutOfMemory);
        }
        if let Some(error) = invalid {
            if !std::mem::replace(&mut self.refusal_logged, true) {
                eprintln!(
                    "teksilo-render: the device refused a {width}x{height} live texture: {error}"
                );
            }
            return Err(TextureOutOfMemory);
        }
        Ok(WgpuTexture {
            texture,
            width,
            height,
            view: None,
            groups: [None, None, None],
        })
    }

    /// wgpu stages a write's rows at the device's copy pitch, which it does
    /// not expose; [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`] bounds it on every
    /// backend, so a band never stages more than it is allowed.
    fn staged_row_bytes(&self, width: u32) -> u64 {
        (u64::from(width) * 4).next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
    }

    /// One `write_texture`, straight from the locked buffer: no repack, no
    /// intermediate copy. The size was re-checked under the lock and every
    /// rect validated at write time, so the copy fits both buffer and texture.
    fn write(&mut self, texture: &mut WgpuTexture, read: &LiveImageRead<'_>, band: PixelRect) {
        let stride = read.stride();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: band.x,
                    y: band.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            read.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: band.y as u64 * stride as u64 + u64::from(band.x) * 4,
                bytes_per_row: Some(stride as u32),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: band.width,
                height: band.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn prepare_filter(&mut self, texture: &mut WgpuTexture, filter: ScalingFilter) {
        let view = texture
            .view
            .get_or_insert_with(|| texture.texture.create_view(&Default::default()));
        // `Trilinear` samples like `Linear` on a texture of one level.
        let sampler = match filter {
            ScalingFilter::Nearest => &self.nearest,
            _ => &self.linear,
        };
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("live_image_bind_group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        texture.groups[filter_index(filter)] = Some(group);
    }

    fn texture_bytes(&self, texture: &WgpuTexture) -> u64 {
        u64::from(texture.width) * u64::from(texture.height) * 4
    }

    fn device_lost(&self) -> bool {
        self.health.is_lost()
    }

    fn released(&mut self) {
        self.released = true;
    }
}
