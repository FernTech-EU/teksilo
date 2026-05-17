//! Dual-Kawase blur engine.
//!
//! Consumes the [`BeginBlurredSubtree`](bastyde_canvas::DrawCommand::BeginBlurredSubtree)
//! / [`EndBlurredSubtree`](bastyde_canvas::DrawCommand::EndBlurredSubtree)
//! pair emitted by the widget tree's render walker. For each scope the
//! renderer:
//!
//! 1. Allocates an intermediate RGBA8 texture sized to the widget's
//!    bounds × scale_factor (drawn from a recycled pool keyed on the
//!    next-power-of-two of the requested size).
//! 2. Suspends the surface render pass and begins a new pass against
//!    the intermediate, with a translation pushed onto the transform
//!    stack so the subtree paints at the intermediate's origin.
//! 3. After processing every command up to `EndBlurredSubtree`, runs
//!    the dual-Kawase chain: `N = ceil(log2(radius))` downsample
//!    passes (each halves the texture, applies a 4-tap bilinear
//!    shader), then `N` upsample passes back to the source size with
//!    a different 4-tap shader.
//! 4. Resumes the surface pass with `LoadOp::Load` (so previous draws
//!    survive) and composites the blurred result as a textured quad
//!    via the existing quad pipeline.
//!
//! See `docs/animation.md` §5.8 for the architectural rationale and
//! cost model.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

/// Per-pass uniform buffer entry. `offset.xy` is the kernel offset in
/// UV units (texels-per-pixel × `0.5`); `.zw` is std140 alignment
/// padding. Both shaders read the same struct so a single uniform
/// layout serves both pipelines.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct BlurParams {
    pub offset: [f32; 4],
}

/// Maximum dual-Kawase chain depth. At N=6 the smallest intermediate
/// is `source / 64` — well past the point where the kernel covers the
/// texture, so further levels add cost without quality. For UI radii
/// (R ≤ 32 px) we cap N = ceil(log2(R)) ≤ 5; this constant only
/// matters as a defensive upper bound.
const MAX_KAWASE_LEVELS: u32 = 6;

// Intermediate format is the surface format, threaded through from
// Renderer construction. Same format means the existing rect/sdf/quad
// pipelines (built against `surface_format`) accept the intermediate
// as a render target without revalidation. With an sRGB surface, this
// also gives free linear-space blur math: sampling auto-linearizes,
// the Kawase shader does linear ops, write re-encodes — no double sRGB.

/// Recycled intermediate-texture pool. Textures are keyed on
/// `(width, height)` rounded up to the next power of two — typical
/// blur scopes within a frame share the same size buckets, so the
/// pool gives us O(1) lookup with at most a handful of distinct
/// allocations across the lifetime of the pool.
///
/// Reset at the top of each frame: we mark every texture as available
/// without freeing it, so steady-state usage allocates zero textures
/// per frame after warm-up. Textures that go unused for `EVICT_FRAMES`
/// consecutive frames are dropped to keep VRAM usage bounded under
/// pathological "sometimes-blurred" workloads.
pub(crate) struct BlurPool {
    /// All pooled textures, grouped by their power-of-two size bucket.
    /// Each entry holds the texture, its view, the bind group binding
    /// it as input to the Kawase pipelines, and per-frame bookkeeping.
    buckets: HashMap<(u32, u32), Vec<PooledTexture>>,
    /// Filtering sampler shared by every intermediate's bind group.
    /// Bilinear since the Kawase shader counts on `textureSample` to
    /// interpolate between texels.
    sampler: wgpu::Sampler,
    /// Cached bind group layout for sampling an intermediate as input
    /// to one of the Kawase pipelines (texture + sampler + uniform).
    pub bind_group_layout: wgpu::BindGroupLayout,
    /// Frame counter used to age out textures that go unused.
    frame: u64,
    /// Format every pooled texture is created with. Mirrors the
    /// renderer's surface format so the existing pipelines render into
    /// it without format mismatch (wgpu validates pipeline target
    /// format against pass attachment format).
    format: wgpu::TextureFormat,
}

struct PooledTexture {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    /// Frame in which this texture was last handed out. Textures unused
    /// for `EVICT_FRAMES` consecutive frames are dropped at the next
    /// `BlurPool::begin_frame`.
    last_used_frame: u64,
    /// Has this texture been handed out for the current frame?
    /// Reset to false at the top of every frame.
    in_use_this_frame: bool,
}

const EVICT_FRAMES: u64 = 60;

impl BlurPool {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur_pool_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        Self {
            buckets: HashMap::new(),
            sampler,
            bind_group_layout,
            frame: 0,
            format,
        }
    }

    /// The format every pooled texture is created with.
    #[allow(dead_code)]
    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Begin a new frame: reset every texture's `in_use_this_frame`
    /// flag, drop textures that have been idle for too long.
    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        let frame = self.frame;
        let cutoff = frame.saturating_sub(EVICT_FRAMES);
        for textures in self.buckets.values_mut() {
            textures.retain(|t| t.last_used_frame >= cutoff);
            for t in textures.iter_mut() {
                t.in_use_this_frame = false;
            }
        }
    }

    /// Acquire (or allocate) an intermediate texture sized exactly to
    /// `(width, height)` device pixels. Textures are reused when the
    /// caller requests the same exact size (typical for stable blur
    /// scopes). Power-of-two padding was tried but rejected: the
    /// Kawase shader samples beyond `used_w/used_h` into the
    /// cleared-zero padding, producing visible dark edges on the
    /// blurred result.
    pub(crate) fn acquire(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> AcquiredTexture {
        let bw = width.max(1);
        let bh = height.max(1);
        let bucket = self.buckets.entry((bw, bh)).or_default();

        // Reuse the first available texture in the bucket.
        if let Some(idx) = bucket.iter().position(|t| !t.in_use_this_frame) {
            bucket[idx].in_use_this_frame = true;
            bucket[idx].last_used_frame = self.frame;
            return AcquiredTexture {
                bucket_key: (bw, bh),
                index: idx,
            };
        }

        // No texture available — allocate one.
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur_intermediate"),
            size: wgpu::Extent3d {
                width: bw,
                height: bh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        bucket.push(PooledTexture {
            texture,
            view,
            width: bw,
            height: bh,
            last_used_frame: self.frame,
            in_use_this_frame: true,
        });
        AcquiredTexture {
            bucket_key: (bw, bh),
            index: bucket.len() - 1,
        }
    }

    pub(crate) fn view(&self, h: AcquiredTexture) -> &wgpu::TextureView {
        &self.buckets[&h.bucket_key][h.index].view
    }

    pub(crate) fn dimensions(&self, h: AcquiredTexture) -> (u32, u32) {
        let t = &self.buckets[&h.bucket_key][h.index];
        (t.width, t.height)
    }

    pub(crate) fn make_bind_group(
        &self,
        device: &wgpu::Device,
        h: AcquiredTexture,
        params_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let view = self.view(h);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        })
    }
}

/// Handle to a pool-acquired texture. Cheap to copy; valid only until
/// the next `BlurPool::begin_frame()`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AcquiredTexture {
    bucket_key: (u32, u32),
    index: usize,
}

/// Builds the two Kawase pipelines (downsample + upsample). They share
/// the same bind group layout (texture + sampler + uniform); the only
/// difference is the fragment shader.
pub(crate) struct BlurPipelines {
    pub down: wgpu::RenderPipeline,
    pub up: wgpu::RenderPipeline,
    pub params_buffer: wgpu::Buffer,
}

impl BlurPipelines {
    pub(crate) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let down_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kawase_down"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/kawase_down.wgsl").into()),
        });
        let up_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kawase_up"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/kawase_up.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kawase_pipeline_layout"),
            bind_group_layouts: &[Some(layout)],
            immediate_size: 0,
        });

        let down = create_kawase_pipeline(
            device,
            &pipeline_layout,
            &down_shader,
            "kawase_down_pipeline",
            target_format,
        );
        let up = create_kawase_pipeline(
            device,
            &pipeline_layout,
            &up_shader,
            "kawase_up_pipeline",
            target_format,
        );

        // Per-pass params live in a single uniform buffer that we
        // rewrite immediately before each Kawase pass. The buffer is
        // 16 bytes (one vec4) — well below any uniform-buffer minimum.
        let params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_params"),
            size: std::mem::size_of::<BlurParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            down,
            up,
            params_buffer,
        }
    }
}

fn create_kawase_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    label: &str,
    target_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
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
}

/// Decide the dual-Kawase chain depth for a given Gaussian-equivalent
/// radius. Caps at [`MAX_KAWASE_LEVELS`] for sanity. Sub-perceptual
/// radii are filtered out at the walker layer; this function assumes
/// `radius >= 0.5`.
pub(crate) fn kawase_levels(radius: f32) -> u32 {
    let n = radius.max(1.0).log2().ceil() as u32;
    n.clamp(1, MAX_KAWASE_LEVELS)
}

/// Compute the Kawase per-pass offset for the given mip level and
/// requested radius. The offset scales with the source-texel size
/// (passed in as `(1.0 / src_w, 1.0 / src_h)`), modulated by a small
/// fudge factor that keeps the visual radius matched to the requested
/// pixel radius across chain depths.
pub(crate) fn kawase_offset(src_w: u32, src_h: u32, radius_scale: f32) -> [f32; 4] {
    let ox = radius_scale / src_w.max(1) as f32;
    let oy = radius_scale / src_h.max(1) as f32;
    [ox, oy, 0.0, 0.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_for_typical_ui_radii() {
        // Documents the chain depth schedule. R=8 → N=3 (3 down +
        // 3 up = 6 small passes plus the composite).
        assert_eq!(kawase_levels(8.0), 3);
        assert_eq!(kawase_levels(12.0), 4);
        assert_eq!(kawase_levels(16.0), 4);
        assert_eq!(kawase_levels(24.0), 5);
        assert_eq!(kawase_levels(32.0), 5);
    }

    #[test]
    fn levels_clamped_for_extreme_radii() {
        // Even an absurd 256 px radius caps at MAX_KAWASE_LEVELS.
        assert_eq!(kawase_levels(256.0), MAX_KAWASE_LEVELS);
        // Sub-1 falls back to the floor of 1 — caller is expected to
        // have filtered sub-perceptual at the walker, but we don't
        // crash if they didn't.
        assert_eq!(kawase_levels(0.5), 1);
        assert_eq!(kawase_levels(0.1), 1);
    }

    #[test]
    fn offset_scales_inversely_with_source_size() {
        // Larger source texture → smaller UV offset for the same
        // pixel-equivalent radius. The Kawase chain hands progressively
        // smaller textures to subsequent passes, so the UV offset
        // grows naturally even at constant radius_scale.
        let big = kawase_offset(1024, 1024, 0.5);
        let small = kawase_offset(64, 64, 0.5);
        assert!(big[0] < small[0]);
        assert!(big[1] < small[1]);
    }
}
