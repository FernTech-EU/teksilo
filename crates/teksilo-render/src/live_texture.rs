// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live pictures on the GPU: the wgpu backend of the live pass
//! (`teksilo_canvas::live_image::internal::LivePass`), which decides what to
//! upload and when. This module only creates textures, writes bands of a
//! locked source into them, builds their mip chains, and readies their bind
//! groups.
//!
//! Textures are `Rgba8UnormSrgb` whatever the source's byte order: the quad
//! shader swaps red and blue for a BGR source and ignores the fourth byte of
//! an RGBX one, so no producer converts pixels and no extension is needed.
//!
//! A texture drawn through `ScalingFilter::Trilinear` has a full mip chain,
//! built on the GPU by `shaders/live_mip.wgsl` with the kernel of the CPU
//! chain static images get. The live pass asks for a rebuild outside every
//! lock; the passes are recorded into the frame's own encoder, after the
//! uploads of that frame (a queue write runs before the submission it
//! precedes) and before any draw.

use teksilo_canvas::PixelRect;
use teksilo_canvas::live_image::ScalingFilter;
use teksilo_canvas::live_image::internal::{
    LiveImageRead, LiveTextureBackend, TextureOutOfMemory, mip_bytes, mip_footprint,
    mip_level_size, mip_levels,
};

use crate::device_health::DeviceHealth;

/// One live texture and what drawing it needs.
pub(crate) struct WgpuTexture {
    pub(crate) texture: wgpu::Texture,
    width: u32,
    height: u32,
    levels: u32,
    /// One single-level view per level: level 0's is what `Linear` and
    /// `Nearest` sample (a view of every level would mipmap them too), and
    /// each is the target of the pass that builds it.
    level_views: Vec<wgpu::TextureView>,
    /// Every level, for `Trilinear`; `None` without mip levels.
    full_view: Option<wgpu::TextureView>,
    /// For each level `k >= 1`, the mip pass's bind group over level `k - 1`.
    mip_groups: Vec<wgpu::BindGroup>,
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

    /// How many levels it has, level 0 included.
    pub(crate) fn levels(&self) -> u32 {
        self.levels
    }
}

/// The two mip pipelines and their bind group layout: one non-filterable
/// texture, read with `textureLoad`. Made once per renderer, when its first
/// mipmapped texture is, so an app that never draws `Trilinear` never
/// compiles them.
struct LiveMipPipelines {
    layout: wgpu::BindGroupLayout,
    straight: wgpu::RenderPipeline,
    opaque: wgpu::RenderPipeline,
}

impl LiveMipPipelines {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("live_mip"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("shaders/fullscreen.wgsl"),
                    include_str!("shaders/live_mip.wgsl")
                )
                .into(),
            ),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("live_mip_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("live_mip_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            straight: pipeline("fs_straight"),
            opaque: pipeline("fs_opaque"),
            layout,
        }
    }
}

/// A chain to rebuild in the frame's encoder: what the passes bind and
/// write, cloned out of the texture (wgpu handles are shared).
struct MipJob {
    size: (u32, u32),
    level_views: Vec<wgpu::TextureView>,
    mip_groups: Vec<wgpu::BindGroup>,
    /// `None`: every texel of every level.
    region: Option<PixelRect>,
    opaque: bool,
}

/// The backend: the window's device and queue, the samplers, and the
/// quad pipeline's texture layout every live bind group is built against.
pub(crate) struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    /// Linear between levels too, for a texture with mip levels.
    trilinear: wgpu::Sampler,
    mip_pipelines: Option<LiveMipPipelines>,
    mip_jobs: Vec<MipJob>,
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
        let sampler =
            |label: &'static str, filter: wgpu::FilterMode, mips: wgpu::MipmapFilterMode| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some(label),
                    address_mode_u: wgpu::AddressMode::ClampToEdge,
                    address_mode_v: wgpu::AddressMode::ClampToEdge,
                    address_mode_w: wgpu::AddressMode::ClampToEdge,
                    mag_filter: filter,
                    min_filter: filter,
                    mipmap_filter: mips,
                    ..Default::default()
                })
            };
        let linear = sampler(
            "live_linear",
            wgpu::FilterMode::Linear,
            wgpu::MipmapFilterMode::Nearest,
        );
        let nearest = sampler(
            "live_nearest",
            wgpu::FilterMode::Nearest,
            wgpu::MipmapFilterMode::Nearest,
        );
        let trilinear = sampler(
            "live_trilinear",
            wgpu::FilterMode::Linear,
            wgpu::MipmapFilterMode::Linear,
        );
        let max_dimension = device.limits().max_texture_dimension_2d;
        Self {
            device,
            queue,
            layout,
            linear,
            nearest,
            trilinear,
            mip_pipelines: None,
            mip_jobs: Vec::new(),
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

    /// Record the mip passes the live pass asked for into `encoder`: one per
    /// level above 0, in order, each reading the level below. A whole chain
    /// clears each level first; a region keeps it and draws only its
    /// footprint. Run before the frame's first draw.
    pub(crate) fn encode_mips(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(pipelines) = self.mip_pipelines.as_ref() else {
            return;
        };
        for job in self.mip_jobs.drain(..) {
            let pipeline = if job.opaque {
                &pipelines.opaque
            } else {
                &pipelines.straight
            };
            for k in 1..job.level_views.len() as u32 {
                let (w, h) = mip_level_size(job.size.0, job.size.1, k);
                let (load, scissor) = match job.region {
                    None => (wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), None),
                    Some(changed) => {
                        let f = mip_footprint(changed, k, (w, h));
                        (wgpu::LoadOp::Load, Some([f.x, f.y, f.width, f.height]))
                    }
                };
                crate::fullscreen::run_fullscreen_pass(
                    encoder,
                    pipeline,
                    &job.mip_groups[k as usize - 1],
                    &[],
                    &job.level_views[k as usize],
                    w,
                    h,
                    load,
                    scissor,
                    "live_mip",
                );
            }
        }
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
    /// made only for a texture that exists.
    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        mipped: bool,
        label: &str,
    ) -> Result<WgpuTexture, TextureOutOfMemory> {
        if std::mem::take(&mut self.fail_next_create) {
            return Err(TextureOutOfMemory);
        }
        let levels = if mipped { mip_levels(width, height) } else { 1 };
        // `COPY_SRC` so a level can be read back: what the GPU tests of the
        // chain and a capture's diagnostics do.
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC;
        if mipped {
            // Each level above 0 is the target of the pass that builds it.
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
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
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage,
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
        let level_views: Vec<wgpu::TextureView> = (0..levels)
            .map(|k| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: k,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let (full_view, mip_groups) = if mipped {
            let pipelines = self
                .mip_pipelines
                .get_or_insert_with(|| LiveMipPipelines::new(&self.device));
            let groups = level_views[..levels as usize - 1]
                .iter()
                .map(|below| {
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("live_mip_bind_group"),
                        layout: &pipelines.layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(below),
                        }],
                    })
                })
                .collect();
            (Some(texture.create_view(&Default::default())), groups)
        } else {
            (None, Vec::new())
        };
        Ok(WgpuTexture {
            texture,
            width,
            height,
            levels,
            level_views,
            full_view,
            mip_groups,
            groups: [None, None, None],
        })
    }

    /// wgpu stages a write's rows at the device's copy pitch, which it does
    /// not expose; [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`] bounds it on every
    /// backend, so a band never stages more than it is allowed.
    fn staged_row_bytes(&self, width: u32) -> u64 {
        (u64::from(width) * 4).next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
    }

    /// One `write_texture` into level 0, straight from the locked buffer:
    /// no repack, no intermediate copy. The size was re-checked under the
    /// lock and every rect validated at write time, so the copy fits both
    /// buffer and texture.
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

    /// `Linear` and `Nearest` sample level 0 alone, so a mipmapped texture
    /// draws them as one without levels does; `Trilinear` samples every
    /// level, or level 0 like `Linear` on a texture without levels.
    fn prepare_filter(&mut self, texture: &mut WgpuTexture, filter: ScalingFilter) {
        let (view, sampler) = match (filter, &texture.full_view) {
            (ScalingFilter::Nearest, _) => (&texture.level_views[0], &self.nearest),
            (ScalingFilter::Trilinear, Some(full)) => (full, &self.trilinear),
            _ => (&texture.level_views[0], &self.linear),
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

    fn rebuild_mips(&mut self, texture: &mut WgpuTexture, region: Option<PixelRect>, opaque: bool) {
        if texture.levels < 2 {
            return;
        }
        self.mip_jobs.push(MipJob {
            size: (texture.width, texture.height),
            level_views: texture.level_views.clone(),
            mip_groups: texture.mip_groups.clone(),
            region,
            opaque,
        });
    }

    fn texture_bytes(&self, texture: &WgpuTexture) -> u64 {
        mip_bytes(texture.width, texture.height, texture.levels)
    }

    fn device_lost(&self) -> bool {
        self.health.is_lost()
    }

    fn released(&mut self) {
        self.released = true;
    }
}
