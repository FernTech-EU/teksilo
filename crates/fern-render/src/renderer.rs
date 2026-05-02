use wgpu;

use fern_canvas::RenderFrame;
use fern_canvas::geometry::Transform2D;

use crate::blur::{BlurPipelines, BlurPool};
use crate::image_manager::ImageManager;
use crate::path_atlas::PathAtlas;
use crate::stream_buffer::StreamBuffers;
use crate::vertex::{AnimQuadVertex, QuadVertex, RectVertex, SdfVertex, ShadowVertex};

/// How many animated-quad slots the uniform buffer holds. Must match
/// the array size in `shaders/anim_procedural.wgsl`. Bumping this
/// requires updating the WGSL constant too (WGSL array sizes are
/// static). 128 × 64 B = 8 KiB — well within UBO caps.
const MAX_ANIM_SLOTS: usize = 128;

/// GPU renderer that draws a RenderFrame using five shader pipelines.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    rect_pipeline: wgpu::RenderPipeline,
    sdf_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    /// Procedural animated-quad pipeline — IndeterminateSweep and
    /// future Pulse / Shimmer kinds. Binds group 0 to a uniform buffer
    /// holding an array of `AnimParams` (one per slot).
    anim_proc_pipeline: wgpu::RenderPipeline,
    /// Sprite-atlas animated-quad pipeline — frame-cycling for
    /// `AnimatedQuadKind::SpriteCycle`. Shares the same uniform buffer
    /// as the procedural pipeline at group 0; group 1 carries the
    /// per-atlas texture bind group. Reuses the quad_pipeline's
    /// bind-group layout for group 1, so the bind groups that
    /// `ImageManager` builds for static images are also usable here
    /// without a second registration.
    anim_sprite_pipeline: wgpu::RenderPipeline,
    /// Uniform buffer backing both animated-quad pipelines' per-slot
    /// state. Rewritten wholesale at the top of each `render()` from
    /// `frame.anim_params`. Fixed size (`MAX_ANIM_SLOTS * 64 B`); the
    /// tree's registry truncates if it ever exceeds.
    anim_uniform_buffer: wgpu::Buffer,
    /// Bind group for the animated pipelines (group 0 on both).
    anim_uniform_bind_group: wgpu::BindGroup,
    atlas_texture: Option<AtlasTexture>,
    path_atlas: PathAtlas,
    path_atlas_texture: Option<AtlasTexture>,
    image_manager: ImageManager,
    /// Persistent per-pipeline streaming buffers. Resized on demand at
    /// the top of each `render()` call, then reused via `write_buffer`
    /// for every batch flush in that frame — replaces the historical
    /// per-flush `create_buffer_init` antipattern.
    streams: StreamBuffers,
    /// Dual-Kawase blur pipelines (downsample + upsample) and per-pass
    /// uniform buffer. Built once at construction; consumed by the
    /// `BeginBlurredSubtree` / `EndBlurredSubtree` handler in `render`.
    blur_pipelines: BlurPipelines,
    /// Recycled intermediate-texture pool for blur scopes. Begin-of-
    /// frame resets per-texture in-use flags; textures unused for
    /// several frames evict.
    blur_pool: BlurPool,
    /// Cached bind group layout for the quad pipeline's group(0)
    /// (texture + sampler). Used to build per-frame bind groups that
    /// expose blur-pool intermediates as image sources for the
    /// compositing blit at the end of each blur scope.
    quad_bind_group_layout: wgpu::BindGroupLayout,
    /// Sampler used by the blur composite blit. Linear filtering so
    /// the over-allocated bucket texture's used sub-rect samples
    /// cleanly when composited onto a non-aligned target rect.
    blur_composite_sampler: wgpu::Sampler,
}

struct AtlasTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
}

/// Active render target — the bottom of the stack is always the
/// surface; intermediates push above it for the duration of a blur
/// scope. Each entry tracks both the target's identity and per-target
/// state that survives across multiple segment passes against the
/// same target (e.g. when an inner blur scope ends and we re-open
/// the parent intermediate to draw additional commands).
struct ActiveTarget {
    /// `None` ⇒ surface (the caller-provided texture view).
    /// `Some(handle)` ⇒ a blur intermediate from `BlurPool`.
    intermediate: Option<crate::blur::AcquiredTexture>,
    /// Viewport dimensions for NDC conversion in this scope.
    viewport_w: u32,
    viewport_h: u32,
    /// `false` until the first segment pass against this target runs;
    /// controls whether the next pass uses Clear or Load.
    opened: bool,
    /// Blurred sub-tree results that nested scopes have queued for
    /// compositing into THIS target on its next segment open. Drained
    /// at the top of each segment.
    pending_composites: Vec<PendingComposite>,
    /// Intermediate-only metadata, populated when `intermediate.is_some()`.
    /// Carried here (rather than in a separate `BlurScope` stack)
    /// because End needs to look these up after popping the target.
    blur_bounds: Option<fern_canvas::Rect>,
    blur_radius_logical: Option<f32>,
    used_w: Option<u32>,
    used_h: Option<u32>,
    bucket_w: Option<u32>,
    bucket_h: Option<u32>,
}

impl ActiveTarget {
    fn surface(viewport_w: u32, viewport_h: u32) -> Self {
        Self {
            intermediate: None,
            viewport_w,
            viewport_h,
            opened: false,
            pending_composites: Vec::new(),
            blur_bounds: None,
            blur_radius_logical: None,
            used_w: None,
            used_h: None,
            bucket_w: None,
            bucket_h: None,
        }
    }
}

/// One blurred sub-tree result waiting to be composited into a parent
/// target's next render pass. Lives on `ActiveTarget::pending_composites`
/// for the parent target.
struct PendingComposite {
    blurred_texture: crate::blur::AcquiredTexture,
    used_w: u32,
    used_h: u32,
    bucket_w: u32,
    bucket_h: u32,
    bounds: fern_canvas::Rect,
}

impl Renderer {
    /// Create a new renderer from an existing wgpu device and queue.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let rect_pipeline = create_rect_pipeline(&device, surface_format);
        let sdf_pipeline = create_sdf_pipeline(&device, surface_format);
        let quad_pipeline = create_quad_pipeline(&device, surface_format);
        let shadow_pipeline = create_shadow_pipeline(&device, surface_format);
        let (
            anim_proc_pipeline,
            anim_uniform_buffer,
            anim_uniform_bind_group,
            anim_uniform_layout,
        ) = create_anim_proc_pipeline(&device, surface_format);
        // Reuse the quad pipeline's texture/sampler layout so bind
        // groups registered by `ImageManager` for static images work
        // equally well as the sprite animation's atlas binding.
        let quad_texture_layout = quad_pipeline.get_bind_group_layout(0);
        let anim_sprite_pipeline = create_anim_sprite_pipeline(
            &device,
            surface_format,
            &anim_uniform_layout,
            &quad_texture_layout,
        );

        let quad_bind_group_layout = quad_pipeline.get_bind_group_layout(0);
        let blur_pool = BlurPool::new(&device, surface_format);
        let blur_pipelines =
            BlurPipelines::new(&device, &blur_pool.bind_group_layout, surface_format);
        let blur_composite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur_composite_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            device,
            queue,
            rect_pipeline,
            sdf_pipeline,
            quad_pipeline,
            shadow_pipeline,
            anim_proc_pipeline,
            anim_sprite_pipeline,
            anim_uniform_buffer,
            anim_uniform_bind_group,
            atlas_texture: None,
            path_atlas: PathAtlas::new(512, 512),
            path_atlas_texture: None,
            image_manager: ImageManager::new(),
            streams: StreamBuffers::new(),
            blur_pipelines,
            blur_pool,
            quad_bind_group_layout,
            blur_composite_sampler,
        }
    }

    /// Upload atlas texture data from the text backend.
    pub fn upload_atlas(&mut self, width: u32, height: u32, pixels: &[u8]) {
        if width == 0 || height == 0 {
            return;
        }

        let needs_recreate = self
            .atlas_texture
            .as_ref()
            .is_none_or(|t| t.width != width || t.height != height);

        if needs_recreate {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("glyph_atlas"),
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

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });

            let bind_group_layout = self.quad_pipeline.get_bind_group_layout(0);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("atlas_bind_group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            self.atlas_texture = Some(AtlasTexture {
                texture,
                bind_group,
                width,
                height,
            });
        }

        if let Some(atlas) = &self.atlas_texture {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Render a frame to the given surface texture view.
    pub fn render(
        &mut self,
        frame: &RenderFrame,
        view: &wgpu::TextureView,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        clear_color: [f32; 4],
    ) {
        // Begin frame for path atlas LRU tracking
        self.path_atlas.begin_frame();
        // Reset blur intermediate-texture pool — marks every texture
        // available, evicts ones unused for too long.
        self.blur_pool.begin_frame();

        // Process pending images: upload textures for newly embedded resources
        for pending in &frame.pending_images {
            if !self.image_manager.contains(&pending.name) {
                let layout = self.quad_pipeline.get_bind_group_layout(0);
                self.image_manager.register_image(
                    &pending.name,
                    pending.width,
                    pending.height,
                    &pending.pixels,
                    &self.device,
                    &self.queue,
                    &layout,
                );
            }
        }

        // Pre-rasterize all paths in this frame into the path atlas
        let mut path_regions: Vec<Option<crate::path_atlas::AtlasRegion>> =
            Vec::with_capacity(frame.paths.len());
        for entry in &frame.paths {
            let region = self.path_atlas.lookup_or_rasterize(
                &entry.path,
                entry.color,
                &entry.stroke_style,
                entry.bounds,
                scale_factor,
            );
            path_regions.push(region);
        }

        // Upload path atlas to GPU if dirty
        if self.path_atlas.is_dirty() {
            let (pw, ph) = self.path_atlas.size();
            self.upload_path_atlas(pw, ph, self.path_atlas.pixels().to_vec());
            self.path_atlas.mark_clean();
        }

        // Grow persistent streaming buffers to fit this frame's worst case.
        // Upper bound per pipeline = `items * 4 vertices` because every
        // drawable produces exactly 4 vertices. The shared index buffer
        // sizes to the largest per-pipeline quad count so one index stream
        // serves all pipelines.
        let rect_quads = frame.decorations.len();
        let sdf_quads = frame.shapes.len();
        // Each blur scope produces one composite-blit quad on End,
        // emitted via the quad pipeline. Account for it in the stream
        // sizing so `composite_blur_quad` can `.write` without growing.
        let composite_quads = frame
            .draw_order
            .iter()
            .filter(|c| matches!(c, fern_canvas::DrawCommand::BeginBlurredSubtree { .. }))
            .count();
        let quad_quads =
            frame.glyphs.len() + frame.paths.len() + frame.images.len() + composite_quads;
        let shadow_quads = frame.shadows.len();
        let anim_proc_quads = frame
            .animated_quads
            .iter()
            .filter(|a| matches!(a.class, fern_canvas::AnimatedQuadClass::Procedural))
            .count();
        let max_quads = rect_quads
            .max(sdf_quads)
            .max(quad_quads)
            .max(shadow_quads)
            .max(anim_proc_quads);

        self.streams.rect.ensure_capacity(
            &self.device,
            (rect_quads * 4 * std::mem::size_of::<RectVertex>()) as u64,
        );
        self.streams.sdf.ensure_capacity(
            &self.device,
            (sdf_quads * 4 * std::mem::size_of::<SdfVertex>()) as u64,
        );
        self.streams.quad.ensure_capacity(
            &self.device,
            (quad_quads * 4 * std::mem::size_of::<QuadVertex>()) as u64,
        );
        self.streams.shadow.ensure_capacity(
            &self.device,
            (shadow_quads * 4 * std::mem::size_of::<ShadowVertex>()) as u64,
        );
        self.streams.anim_proc.ensure_capacity(
            &self.device,
            (anim_proc_quads * 4 * std::mem::size_of::<AnimQuadVertex>()) as u64,
        );
        self.streams.index.ensure_capacity(
            &self.device,
            (max_quads * 6 * std::mem::size_of::<u16>()) as u64,
        );
        self.streams.reset();

        // Upload animated-quad per-slot state for this frame. Truncate
        // past MAX_ANIM_SLOTS — the registry currently caps at
        // 128 slots and growing the buffer would require recreating
        // the bind group, so we just drop excess slots and warn in
        // debug builds. In practice, 128 is well beyond typical UIs.
        if !frame.anim_params.is_empty() {
            let n = frame.anim_params.len().min(MAX_ANIM_SLOTS);
            debug_assert!(
                frame.anim_params.len() <= MAX_ANIM_SLOTS,
                "AnimParams exceeds MAX_ANIM_SLOTS ({}); tail will be dropped",
                MAX_ANIM_SLOTS
            );
            // Safety: `fern_canvas::AnimParams` is `#[repr(C)]` with
            // explicit padding and only contains `u32`/`f32`/fixed
            // arrays thereof — layout-compatible with raw bytes.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    frame.anim_params.as_ptr() as *const u8,
                    n * std::mem::size_of::<fern_canvas::AnimParams>(),
                )
            };
            self.queue.write_buffer(&self.anim_uniform_buffer, 0, bytes);
        }

        // Upload the full quad index pattern once — 6 u16s per quad, shared
        // across every quad-based pipeline this frame.
        let index_data: Vec<u16> = crate::vertex::generate_quad_indices(max_quads);
        let index_binding = self
            .streams
            .index
            .write(&self.queue, bytemuck::cast_slice(&index_data));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fern_render"),
            });

        // Per-frame mutable viewport — overridden inside blur scopes
        // (the offscreen intermediate is sized differently from the
        // surface). Restored on `EndBlurredSubtree`.
        let mut viewport_width = viewport_width;
        let mut viewport_height = viewport_height;

        {
            let surface_clear_color = wgpu::Color {
                r: clear_color[0] as f64,
                g: clear_color[1] as f64,
                b: clear_color[2] as f64,
                a: clear_color[3] as f64,
            };

            // Target stack — bottom is the surface (never popped),
            // intermediates pushed on `BeginBlurredSubtree` and popped
            // on `EndBlurredSubtree`. The active target is always
            // `target_stack.last_mut()`. Each target carries:
            //   - opened: false until the first segment runs against
            //     it (controls Clear vs Load on the next open)
            //   - viewport dimensions for NDC conversion in this scope
            //   - pending_composites: blurred quads that nested scopes
            //     have queued for compositing into THIS target on its
            //     next segment
            let mut target_stack: Vec<ActiveTarget> = vec![ActiveTarget::surface(
                viewport_width,
                viewport_height,
            )];

            // Clip rect stack for nested scroll areas.
            // Each SetClip pushes a rect; the effective clip is the intersection.
            // ClearClip pops the top and restores the previous intersection.
            let mut clip_stack: Vec<[u32; 4]> = Vec::new(); // [x, y, w, h]

            // Opacity stack for nested opacity groups
            let mut opacity_stack: Vec<f32> = vec![1.0];
            let mut current_opacity: f32 = 1.0;

            // Blend mode stack
            let mut blend_stack: Vec<fern_canvas::BlendMode> = Vec::new();
            let mut current_blend = fern_canvas::BlendMode::Normal;
            let _ = current_blend; // used to track state for future pipeline switching

            // Transform stack — applied CPU-side to pixel positions before NDC conversion.
            // The stack tracks subtree-level transforms pushed by the render walker
            // (`PushTransform` / `PopTransform`); `current_transform` is always the
            // top of the stack composed with whatever the most recent `SetTransform`
            // command set within the current scope.
            let mut transform_stack: Vec<Transform2D> = vec![Transform2D::IDENTITY];
            let mut current_transform = Transform2D::IDENTITY;

            // --- Batched rendering ---
            // Accumulate vertices per pipeline, flush on state/pipeline changes.
            // This produces one GPU buffer + one draw call per contiguous batch
            // instead of two buffers per quad.
            let mut rect_batch: Vec<RectVertex> = Vec::new();
            let mut sdf_batch: Vec<SdfVertex> = Vec::new();
            let mut quad_batch: Vec<QuadVertex> = Vec::new();
            let mut shadow_batch: Vec<ShadowVertex> = Vec::new();
            let mut anim_proc_batch: Vec<AnimQuadVertex> = Vec::new();

            // Which pipeline the current quad batch uses (glyph atlas, path atlas, or image).
            // Flushed when the bind group source changes.
            #[derive(Clone, Copy, PartialEq, Eq)]
            enum QuadSource {
                GlyphAtlas,
                PathAtlas,
            }
            let mut quad_source: Option<QuadSource> = None;

            // Flush helpers — each writes one batch into the persistent
            // stream buffer and issues one draw call. The index buffer was
            // written once at the top of `render()` and is shared.
            //
            // `$index_binding` is `Option<(&Buffer, u64 offset, u64 len)>`
            // — `None` only if the frame had zero quads, in which case
            // every batch is also empty and the flush is a no-op anyway.
            macro_rules! flush_stream {
                ($pass:expr, $queue:expr, $stream:expr, $pipeline:expr,
                 $batch:expr, $index_binding:expr) => {
                    if !$batch.is_empty() {
                        let bytes: &[u8] = bytemuck::cast_slice(&$batch);
                        if let (Some((vb, v_off, v_len)), Some((ib, _, _))) =
                            ($stream.write($queue, bytes), $index_binding)
                        {
                            let quads = ($batch.len() / 4) as u32;
                            let index_count = quads * 6;
                            let index_bytes = (index_count as u64) * 2;
                            $pass.set_pipeline($pipeline);
                            $pass.set_vertex_buffer(0, vb.slice(v_off..v_off + v_len));
                            $pass
                                .set_index_buffer(ib.slice(0..index_bytes), wgpu::IndexFormat::Uint16);
                            $pass.draw_indexed(0..index_count, 0, 0..1);
                        }
                        $batch.clear();
                    }
                };
            }

            // Flush all pending batches (called on state changes).
            macro_rules! flush_all {
                ($pass:expr, $queue:expr, $streams:expr,
                 $rp:expr, $sp:expr, $qp:expr, $shp:expr,
                 $rb:expr, $sb:expr, $qb:expr, $shb:expr,
                 $atlas:expr, $path_atlas:expr, $qs:expr, $index_binding:expr) => {
                    flush_stream!($pass, $queue, &$streams.rect, $rp, $rb, $index_binding);
                    flush_stream!($pass, $queue, &$streams.sdf, $sp, $sb, $index_binding);
                    // Quad batch needs bind group
                    if !$qb.is_empty() {
                        let bg = match $qs {
                            Some(QuadSource::PathAtlas) => {
                                $path_atlas.as_ref().map(|a: &AtlasTexture| &a.bind_group)
                            }
                            _ => $atlas.as_ref().map(|a: &AtlasTexture| &a.bind_group),
                        };
                        if let (Some(bind_group), Some((ib, _, _))) = (bg, $index_binding) {
                            let bytes: &[u8] = bytemuck::cast_slice(&$qb);
                            if let Some((vb, v_off, v_len)) =
                                $streams.quad.write($queue, bytes)
                            {
                                let quads = ($qb.len() / 4) as u32;
                                let index_count = quads * 6;
                                let index_bytes = (index_count as u64) * 2;
                                $pass.set_pipeline($qp);
                                $pass.set_bind_group(0, bind_group, &[]);
                                $pass.set_vertex_buffer(0, vb.slice(v_off..v_off + v_len));
                                $pass.set_index_buffer(
                                    ib.slice(0..index_bytes),
                                    wgpu::IndexFormat::Uint16,
                                );
                                $pass.draw_indexed(0..index_count, 0, 0..1);
                            }
                        }
                        $qb.clear();
                    }
                    flush_stream!($pass, $queue, &$streams.shadow, $shp, $shb, $index_binding);
                    // Animated-quad procedural batch. Unlike the shared
                    // atlas quad pipeline above, this always binds the
                    // same uniform bind group (per-slot state read by
                    // shader) so there's no source-switching. Accesses
                    // `self.anim_proc_pipeline` / `.anim_uniform_bind_group`
                    // and the local `anim_proc_batch` via macro hygiene —
                    // all three are in scope inside `render()` at every
                    // flush_all! call site.
                    if !anim_proc_batch.is_empty()
                        && let Some((ib, _, _)) = $index_binding
                    {
                        let bytes: &[u8] = bytemuck::cast_slice(&anim_proc_batch);
                        if let Some((vb, v_off, v_len)) =
                            $streams.anim_proc.write($queue, bytes)
                        {
                            let quads = (anim_proc_batch.len() / 4) as u32;
                            let index_count = quads * 6;
                            let index_bytes = (index_count as u64) * 2;
                            $pass.set_pipeline(&self.anim_proc_pipeline);
                            $pass.set_bind_group(0, &self.anim_uniform_bind_group, &[]);
                            $pass.set_vertex_buffer(0, vb.slice(v_off..v_off + v_len));
                            $pass.set_index_buffer(
                                ib.slice(0..index_bytes),
                                wgpu::IndexFormat::Uint16,
                            );
                            $pass.draw_indexed(0..index_count, 0, 0..1);
                        }
                        anim_proc_batch.clear();
                    }
                };
            }

            // Draw in painter's order. Outer loop iterates render
            // segments — one segment per `RenderPass`. A blur Begin/End
            // boundary opens a new segment. The pass lives in its own
            // scope so the encoder borrow is released at each boundary
            // (allowing the next pass open or any in-between Kawase
            // work on the encoder).
            let mut cmd_idx = 0;
            while cmd_idx <= frame.draw_order.len() {
                // Resolve current target. We `match` the intermediate
                // handle vs. surface here; the resulting `target_view`
                // lifetime ties to one of self.blur_pool / `view` arg.
                let (target_view, load_op): (&wgpu::TextureView, wgpu::LoadOp<wgpu::Color>) = {
                    let t = target_stack.last_mut().expect("surface target always present");
                    let v: &wgpu::TextureView = match t.intermediate {
                        Some(h) => self.blur_pool.view(h),
                        None => view,
                    };
                    let lo = if t.opened {
                        wgpu::LoadOp::Load
                    } else if t.intermediate.is_none() {
                        wgpu::LoadOp::Clear(surface_clear_color)
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    };
                    t.opened = true;
                    viewport_width = t.viewport_w;
                    viewport_height = t.viewport_h;
                    (v, lo)
                };

                // Drain pending composites — these are blurred sub-tree
                // results from nested blur scopes that finished while
                // we weren't drawing into THIS target. They paint first
                // in the new segment so subsequent commands stack on
                // top of the blurred quad.
                let composites_to_draw: Vec<PendingComposite> = std::mem::take(
                    &mut target_stack.last_mut().unwrap().pending_composites,
                );

                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("fern_segment_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: load_op,
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });

                    // Composite pending blurred sub-trees first.
                    for pc in &composites_to_draw {
                        composite_blur_quad(
                            &self.device,
                            &self.queue,
                            &mut pass,
                            &self.blur_pool,
                            &self.quad_pipeline,
                            &self.quad_bind_group_layout,
                            &self.blur_composite_sampler,
                            &self.streams.quad,
                            index_binding,
                            pc.blurred_texture,
                            pc.used_w,
                            pc.used_h,
                            pc.bucket_w,
                            pc.bucket_h,
                            pc.bounds,
                            scale_factor,
                            viewport_width,
                            viewport_height,
                        );
                        // The composite uses the quad pipeline with a
                        // fresh bind group → invalidate any cached
                        // glyph/path-atlas binding for the next quad
                        // batch.
                        quad_source = None;
                    }

                    let pass = &mut pass;

                    // Inner loop: process commands until we hit a blur
                    // boundary or run out.
                    while cmd_idx < frame.draw_order.len() {
                        let cmd = &frame.draw_order[cmd_idx];
                        if matches!(
                            cmd,
                            fern_canvas::DrawCommand::BeginBlurredSubtree { .. }
                                | fern_canvas::DrawCommand::EndBlurredSubtree
                        ) {
                            break;
                        }
                        match cmd {
                    fern_canvas::DrawCommand::Decoration(idx) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        let Some(rect) = frame.decorations.get(*idx) else {
                            continue;
                        };
                        let verts = RectVertex::from_decoration(rect, scale_factor);
                        for v in &verts {
                            let tp = apply_transform_pixel(v.position, &current_transform);
                            rect_batch.push(RectVertex {
                                position: pixel_to_ndc(tp, viewport_width, viewport_height),
                                color: [
                                    v.color[0],
                                    v.color[1],
                                    v.color[2],
                                    v.color[3] * current_opacity,
                                ],
                            });
                        }
                    }
                    fern_canvas::DrawCommand::Shape(idx) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        let Some(shape) = frame.shapes.get(*idx) else {
                            continue;
                        };
                        let verts = SdfVertex::from_shape_quad(shape, scale_factor);
                        for v in &verts {
                            let tp = apply_transform_pixel(v.position, &current_transform);
                            sdf_batch.push(SdfVertex {
                                position: pixel_to_ndc(tp, viewport_width, viewport_height),
                                color: [
                                    v.color[0],
                                    v.color[1],
                                    v.color[2],
                                    v.color[3] * current_opacity,
                                ],
                                ..*v
                            });
                        }
                    }
                    fern_canvas::DrawCommand::Glyph(idx) => {
                        // Only flush when the quad source changes — consecutive
                        // glyphs batch into one draw call.
                        if quad_source != Some(QuadSource::GlyphAtlas) {
                            flush_all!(
                                pass,
                                &self.queue,
                                self.streams,
                                &self.rect_pipeline,
                                &self.sdf_pipeline,
                                &self.quad_pipeline,
                                &self.shadow_pipeline,
                                rect_batch,
                                sdf_batch,
                                quad_batch,
                                shadow_batch,
                                self.atlas_texture,
                                self.path_atlas_texture,
                                quad_source,
                                index_binding
                            );
                            quad_source = Some(QuadSource::GlyphAtlas);
                        }
                        if let Some(atlas) = &self.atlas_texture {
                            let Some(glyph) = frame.glyphs.get(*idx) else {
                                continue;
                            };
                            let verts = QuadVertex::from_glyph_quad(
                                glyph,
                                scale_factor,
                                atlas.width,
                                atlas.height,
                            );
                            for v in &verts {
                                let tp = apply_transform_pixel(v.position, &current_transform);
                                quad_batch.push(QuadVertex {
                                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                                    color: [
                                        v.color[0],
                                        v.color[1],
                                        v.color[2],
                                        v.color[3] * current_opacity,
                                    ],
                                    ..*v
                                });
                            }
                        }
                    }
                    fern_canvas::DrawCommand::Shadow(idx) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        let Some(shadow) = frame.shadows.get(*idx) else {
                            continue;
                        };
                        let verts = ShadowVertex::from_shadow_quad(shadow, scale_factor);
                        for v in &verts {
                            let tp = apply_transform_pixel(v.position, &current_transform);
                            shadow_batch.push(ShadowVertex {
                                position: pixel_to_ndc(tp, viewport_width, viewport_height),
                                shadow_color: [
                                    v.shadow_color[0],
                                    v.shadow_color[1],
                                    v.shadow_color[2],
                                    v.shadow_color[3] * current_opacity,
                                ],
                                ..*v
                            });
                        }
                    }
                    fern_canvas::DrawCommand::Image(idx) => {
                        // Images use per-image bind groups — flush and draw individually
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        let Some(image) = frame.images.get(*idx) else {
                            continue;
                        };
                        self.draw_image(
                            pass,
                            image,
                            scale_factor,
                            viewport_width,
                            viewport_height,
                            current_opacity,
                            &current_transform,
                            index_binding,
                        );
                    }
                    fern_canvas::DrawCommand::Path(idx) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        if let Some(Some(region)) = path_regions.get(*idx) {
                            quad_source = Some(QuadSource::PathAtlas);

                            let Some(entry) = frame.paths.get(*idx) else {
                                continue;
                            };
                            let Some(path_atlas) = self.path_atlas_texture.as_ref() else {
                                continue;
                            };
                            let verts = path_quad_verts(
                                entry,
                                region,
                                scale_factor,
                                path_atlas.width,
                                path_atlas.height,
                                current_opacity,
                                &current_transform,
                            );
                            for v in &verts {
                                quad_batch.push(QuadVertex {
                                    position: pixel_to_ndc(
                                        v.position,
                                        viewport_width,
                                        viewport_height,
                                    ),
                                    ..*v
                                });
                            }
                        }
                    }
                    // --- State changes flush all batches ---
                    fern_canvas::DrawCommand::SetClip(rect) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        let x = (rect.x * scale_factor).max(0.0) as u32;
                        let y = (rect.y * scale_factor).max(0.0) as u32;
                        let w = (rect.width * scale_factor).ceil().max(0.0) as u32;
                        let h = (rect.height * scale_factor).ceil().max(0.0) as u32;
                        // Clamp to viewport — wgpu requires x+w <= width, y+h <= height.
                        let x = x.min(viewport_width);
                        let y = y.min(viewport_height);
                        let w = w.min(viewport_width.saturating_sub(x));
                        let h = h.min(viewport_height.saturating_sub(y));
                        let clipped = if let Some(&[cx, cy, cw, ch]) = clip_stack.last() {
                            let ix = x.max(cx);
                            let iy = y.max(cy);
                            let ir = (x + w).min(cx + cw);
                            let ib = (y + h).min(cy + ch);
                            [ix, iy, ir.saturating_sub(ix), ib.saturating_sub(iy)]
                        } else {
                            [x, y, w, h]
                        };
                        clip_stack.push(clipped);
                        pass.set_scissor_rect(clipped[0], clipped[1], clipped[2], clipped[3]);
                    }
                    fern_canvas::DrawCommand::ClearClip => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        clip_stack.pop();
                        if let Some(&[x, y, w, h]) = clip_stack.last() {
                            pass.set_scissor_rect(x, y, w, h);
                        } else {
                            pass.set_scissor_rect(0, 0, viewport_width, viewport_height);
                        }
                    }
                    fern_canvas::DrawCommand::SetOpacity(opacity) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        opacity_stack.push(current_opacity);
                        current_opacity *= opacity;
                    }
                    fern_canvas::DrawCommand::RestoreOpacity => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        current_opacity = opacity_stack.pop().unwrap_or(1.0);
                    }
                    fern_canvas::DrawCommand::Rasterized(_) => {}
                    fern_canvas::DrawCommand::AnimatedQuad(idx) => {
                        let Some(draw) = frame.animated_quads.get(*idx) else {
                            continue;
                        };
                        // Flush every other pipeline first so painter's
                        // order is preserved across pipeline boundaries.
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        match &draw.class {
                            fern_canvas::AnimatedQuadClass::Procedural => {
                                let verts =
                                    AnimQuadVertex::from_animated_quad(draw, scale_factor);
                                for v in &verts {
                                    let tp =
                                        apply_transform_pixel(v.position, &current_transform);
                                    anim_proc_batch.push(AnimQuadVertex {
                                        position: pixel_to_ndc(
                                            tp,
                                            viewport_width,
                                            viewport_height,
                                        ),
                                        uv: v.uv,
                                        slot: v.slot,
                                        _pad: v._pad,
                                    });
                                }
                            }
                            fern_canvas::AnimatedQuadClass::Sprite { image_name } => {
                                // Sprite quads need a per-atlas bind
                                // group, so each draws individually —
                                // same shape as the static Image path.
                                // Typical scene has ~1 animated sprite
                                // icon at a time, so batching is moot.
                                let Some(atlas_bg) =
                                    self.image_manager.get_bind_group(image_name)
                                else {
                                    continue;
                                };
                                let verts =
                                    AnimQuadVertex::from_animated_quad(draw, scale_factor);
                                let mut ndc_verts = [AnimQuadVertex {
                                    position: [0.0; 2],
                                    uv: [0.0; 2],
                                    slot: 0,
                                    _pad: 0,
                                }; 4];
                                for (i, v) in verts.iter().enumerate() {
                                    let tp =
                                        apply_transform_pixel(v.position, &current_transform);
                                    ndc_verts[i] = AnimQuadVertex {
                                        position: pixel_to_ndc(
                                            tp,
                                            viewport_width,
                                            viewport_height,
                                        ),
                                        uv: v.uv,
                                        slot: v.slot,
                                        _pad: v._pad,
                                    };
                                }
                                let bytes: &[u8] = bytemuck::cast_slice(&ndc_verts);
                                if let (Some((vb, v_off, v_len)), Some((ib, _, _))) = (
                                    self.streams.anim_proc.write(&self.queue, bytes),
                                    index_binding,
                                ) {
                                    let index_bytes: u64 = 6 * 2;
                                    pass.set_pipeline(&self.anim_sprite_pipeline);
                                    pass.set_bind_group(
                                        0,
                                        &self.anim_uniform_bind_group,
                                        &[],
                                    );
                                    pass.set_bind_group(1, atlas_bg, &[]);
                                    pass.set_vertex_buffer(
                                        0,
                                        vb.slice(v_off..v_off + v_len),
                                    );
                                    pass.set_index_buffer(
                                        ib.slice(0..index_bytes),
                                        wgpu::IndexFormat::Uint16,
                                    );
                                    pass.draw_indexed(0..6, 0, 0..1);
                                }
                            }
                        }
                    }
                    fern_canvas::DrawCommand::SetBlendMode(mode) => {
                        blend_stack.push(current_blend);
                        current_blend = *mode;
                    }
                    fern_canvas::DrawCommand::RestoreBlendMode => {
                        current_blend = blend_stack.pop().unwrap_or(fern_canvas::BlendMode::Normal);
                    }
                    fern_canvas::DrawCommand::SetTransform(t) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        // Widgets author transforms in logical pixels, but
                        // vertices arrive pre-multiplied by scale_factor (HiDPI
                        // device pixels). Scale the translation column so the
                        // pivot lands at the same physical point in either
                        // coordinate space.
                        let device_t = Transform2D {
                            m: [
                                t.m[0], t.m[1], t.m[2], t.m[3],
                                t.m[4] * scale_factor, t.m[5] * scale_factor,
                            ],
                        };
                        // Compose with the current transform-stack top so a
                        // widget's canvas-local transform respects any wrapper
                        // transform pushed by the render walker. With an
                        // identity stack top this is identical to the old
                        // "absolute" semantics — backwards compatible for any
                        // widget not under a transform scope.
                        let stack_top = transform_stack
                            .last()
                            .copied()
                            .unwrap_or(Transform2D::IDENTITY);
                        current_transform = device_t.then(&stack_top);
                    }
                    fern_canvas::DrawCommand::PushTransform(t) => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        // See SetTransform: scale the translation column to
                        // device pixels before composing.
                        let device_t = Transform2D {
                            m: [
                                t.m[0], t.m[1], t.m[2], t.m[3],
                                t.m[4] * scale_factor, t.m[5] * scale_factor,
                            ],
                        };
                        let prev_top = transform_stack
                            .last()
                            .copied()
                            .unwrap_or(Transform2D::IDENTITY);
                        let new_top = device_t.then(&prev_top);
                        transform_stack.push(new_top);
                        current_transform = new_top;
                    }
                    fern_canvas::DrawCommand::PopTransform => {
                        flush_all!(
                            pass,
                            &self.queue,
                            self.streams,
                            &self.rect_pipeline,
                            &self.sdf_pipeline,
                            &self.quad_pipeline,
                            &self.shadow_pipeline,
                            rect_batch,
                            sdf_batch,
                            quad_batch,
                            shadow_batch,
                            self.atlas_texture,
                            self.path_atlas_texture,
                            quad_source,
                            index_binding
                        );
                        quad_source = None;
                        if transform_stack.len() > 1 {
                            transform_stack.pop();
                        }
                        current_transform = transform_stack
                            .last()
                            .copied()
                            .unwrap_or(Transform2D::IDENTITY);
                    }
                    fern_canvas::DrawCommand::BeginBlurredSubtree { .. }
                    | fern_canvas::DrawCommand::EndBlurredSubtree => {
                        // Unreachable — the inner-loop guard above
                        // breaks before we enter the match for these.
                        unreachable!(
                            "blur boundaries are handled at the segment level"
                        );
                    }
                        }
                        cmd_idx += 1;
                    }

                    // End-of-segment flush.
                    flush_all!(
                        pass,
                        &self.queue,
                        self.streams,
                        &self.rect_pipeline,
                        &self.sdf_pipeline,
                        &self.quad_pipeline,
                        &self.shadow_pipeline,
                        rect_batch,
                        sdf_batch,
                        quad_batch,
                        shadow_batch,
                        self.atlas_texture,
                        self.path_atlas_texture,
                        quad_source,
                        index_binding
                    );
                    quad_source = None;
                } // pass dropped here, encoder borrow released

                // Boundary handling. EOF, Begin, or End.
                if cmd_idx >= frame.draw_order.len() {
                    break;
                }
                match &frame.draw_order[cmd_idx] {
                    fern_canvas::DrawCommand::BeginBlurredSubtree { bounds, radius } => {
                        // Allocate intermediate sized to bounds × scale.
                        let device_w = (bounds.width * scale_factor)
                            .ceil()
                            .max(1.0) as u32;
                        let device_h = (bounds.height * scale_factor)
                            .ceil()
                            .max(1.0) as u32;
                        let intermediate =
                            self.blur_pool.acquire(&self.device, device_w, device_h);
                        let (bucket_w, bucket_h) = self.blur_pool.dimensions(intermediate);

                        // Push a translation so the subtree renders at
                        // (0, 0) of the intermediate. Device-pixel
                        // translation since vertices arrive pre-scaled
                        // (see SetTransform handler for the same trick).
                        let translate = Transform2D {
                            m: [
                                1.0, 0.0, 0.0, 1.0,
                                -bounds.x * scale_factor,
                                -bounds.y * scale_factor,
                            ],
                        };
                        let prev_top = transform_stack
                            .last()
                            .copied()
                            .unwrap_or(Transform2D::IDENTITY);
                        let new_top = translate.then(&prev_top);
                        transform_stack.push(new_top);
                        current_transform = new_top;

                        target_stack.push(ActiveTarget {
                            intermediate: Some(intermediate),
                            viewport_w: bucket_w,
                            viewport_h: bucket_h,
                            opened: false,
                            pending_composites: Vec::new(),
                            blur_bounds: Some(*bounds),
                            blur_radius_logical: Some(*radius),
                            used_w: Some(device_w),
                            used_h: Some(device_h),
                            bucket_w: Some(bucket_w),
                            bucket_h: Some(bucket_h),
                        });
                    }
                    fern_canvas::DrawCommand::EndBlurredSubtree => {
                        let scope = target_stack
                            .pop()
                            .expect("EndBlurredSubtree without matching Begin");
                        debug_assert!(
                            scope.intermediate.is_some(),
                            "End popped the surface (impossible if walker is balanced)"
                        );
                        let intermediate = scope.intermediate.unwrap();
                        let bounds = scope.blur_bounds.unwrap();
                        let radius = scope.blur_radius_logical.unwrap();
                        let used_w = scope.used_w.unwrap();
                        let used_h = scope.used_h.unwrap();
                        let bucket_w = scope.bucket_w.unwrap();
                        let bucket_h = scope.bucket_h.unwrap();

                        // Pop the translation pushed in Begin.
                        if transform_stack.len() > 1 {
                            transform_stack.pop();
                        }
                        current_transform = transform_stack
                            .last()
                            .copied()
                            .unwrap_or(Transform2D::IDENTITY);

                        // Run dual-Kawase. The chain begins its own
                        // sub-passes against pool textures — the outer
                        // segment's pass is already dropped.
                        let blurred = run_kawase_chain(
                            &self.device,
                            &self.queue,
                            &mut encoder,
                            &mut self.blur_pool,
                            &self.blur_pipelines,
                            intermediate,
                            used_w,
                            used_h,
                            bucket_w,
                            bucket_h,
                            radius * scale_factor,
                        );

                        // Schedule a composite into the parent target's
                        // next segment open.
                        target_stack
                            .last_mut()
                            .unwrap()
                            .pending_composites
                            .push(PendingComposite {
                                blurred_texture: blurred.texture,
                                used_w: blurred.used_w,
                                used_h: blurred.used_h,
                                bucket_w: blurred.bucket_w,
                                bucket_h: blurred.bucket_h,
                                bounds,
                            });
                    }
                    _ => unreachable!("inner loop only breaks on Begin/End"),
                }
                cmd_idx += 1;
            }

            debug_assert!(
                target_stack.len() == 1,
                "target_stack not balanced at EOF — unmatched Begin/End in walker output"
            );
            // The remaining surface target may still have a pending
            // composite (an outermost blur scope ending at end-of-frame
            // with no further commands). Drain it in one final pass.
            let final_composites = std::mem::take(
                &mut target_stack.last_mut().unwrap().pending_composites,
            );
            if !final_composites.is_empty() {
                let surface = target_stack.last_mut().unwrap();
                let load_op = if surface.opened {
                    wgpu::LoadOp::Load
                } else {
                    wgpu::LoadOp::Clear(surface_clear_color)
                };
                surface.opened = true;
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("fern_final_composite_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                for pc in &final_composites {
                    composite_blur_quad(
                        &self.device,
                        &self.queue,
                        &mut pass,
                        &self.blur_pool,
                        &self.quad_pipeline,
                        &self.quad_bind_group_layout,
                        &self.blur_composite_sampler,
                        &self.streams.quad,
                        index_binding,
                        pc.blurred_texture,
                        pc.used_w,
                        pc.used_h,
                        pc.bucket_w,
                        pc.bucket_h,
                        pc.bounds,
                        scale_factor,
                        viewport_width,
                        viewport_height,
                    );
                }
            } else if !target_stack.last().unwrap().opened {
                // Empty frame — open one pass to apply the clear.
                let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("fern_empty_clear_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(surface_clear_color),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    // draw_rect, draw_sdf, draw_quad, draw_shadow, draw_path_quad removed —
    // replaced by batched rendering in render().

    #[allow(clippy::too_many_arguments)]
    fn draw_image(
        &self,
        pass: &mut wgpu::RenderPass,
        image: &fern_canvas::ImageQuad,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
        index_binding: Option<(&wgpu::Buffer, u64, u64)>,
    ) {
        let bind_group = match self.image_manager.get_bind_group(&image.name) {
            Some(bg) => bg,
            None => return,
        };

        let [x, y, w, h] = image.screen;
        let sx = x * scale_factor;
        let sy = y * scale_factor;
        let sw = w * scale_factor;
        let sh = h * scale_factor;

        // Tintable mode: image is an alpha mask tinted with the given color (flag=0).
        // Full-color mode: image RGB used directly (flag=1, existing behavior).
        let (color, flags) = if let Some(tint) = image.tint {
            // Tint colors are sRGB-encoded (from fern_tokens::Color) — linearize
            // for the Rgba8UnormSrgb surface, same as all other vertex colors.
            (crate::vertex::srgb_to_linear_rgba([tint[0], tint[1], tint[2], tint[3] * opacity]), 0)
        } else {
            ([1.0, 1.0, 1.0, opacity], crate::vertex::QUAD_FLAG_COLOR_GLYPH)
        };

        let verts = [
            QuadVertex {
                position: [sx, sy],
                tex_coord: [0.0, 0.0],
                color,
                flags,
                _pad: 0,
            },
            QuadVertex {
                position: [sx + sw, sy],
                tex_coord: [1.0, 0.0],
                color,
                flags,
                _pad: 0,
            },
            QuadVertex {
                position: [sx + sw, sy + sh],
                tex_coord: [1.0, 1.0],
                color,
                flags,
                _pad: 0,
            },
            QuadVertex {
                position: [sx, sy + sh],
                tex_coord: [0.0, 1.0],
                color,
                flags,
                _pad: 0,
            },
        ];

        let ndc_verts: [QuadVertex; 4] = std::array::from_fn(|i| {
            let v = verts[i];
            let tp = apply_transform_pixel(v.position, transform);
            QuadVertex {
                position: pixel_to_ndc(tp, viewport_width, viewport_height),
                ..v
            }
        });

        // Reuse the persistent quad stream buffer instead of allocating
        // a fresh vertex buffer per image. Indices come from the shared
        // index stream populated at the top of `render()`.
        let bytes: &[u8] = bytemuck::cast_slice(&ndc_verts);
        let Some((vb, v_off, v_len)) = self.streams.quad.write(&self.queue, bytes) else {
            return;
        };
        let Some((ib, _, _)) = index_binding else {
            return;
        };

        pass.set_pipeline(&self.quad_pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(v_off..v_off + v_len));
        pass.set_index_buffer(ib.slice(0..12), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    /// Upload path atlas texture data.
    fn upload_path_atlas(&mut self, width: u32, height: u32, pixels: Vec<u8>) {
        if width == 0 || height == 0 {
            return;
        }

        let needs_recreate = self
            .path_atlas_texture
            .as_ref()
            .is_none_or(|t| t.width != width || t.height != height);

        if needs_recreate {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("path_atlas"),
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

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });

            let bind_group_layout = self.quad_pipeline.get_bind_group_layout(0);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("path_atlas_bind_group"),
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });

            self.path_atlas_texture = Some(AtlasTexture {
                texture,
                bind_group,
                width,
                height,
            });
        }

        if let Some(atlas) = &self.path_atlas_texture {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &atlas.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Register an image for rendering by name.
    pub fn register_image(&mut self, name: &str, width: u32, height: u32, pixels: &[u8]) {
        let layout = self.quad_pipeline.get_bind_group_layout(0);
        self.image_manager.register_image(
            name,
            width,
            height,
            pixels,
            &self.device,
            &self.queue,
            &layout,
        );
    }

    /// Remove a registered image.
    pub fn remove_image(&mut self, name: &str) {
        self.image_manager.remove(name);
    }
}

/// Convert pixel coordinates to NDC (-1..1).
/// Build 4 QuadVertex for a path entry (in pixel space, pre-NDC).
fn path_quad_verts(
    entry: &fern_canvas::PathEntry,
    region: &crate::path_atlas::AtlasRegion,
    scale_factor: f32,
    atlas_width: u32,
    atlas_height: u32,
    opacity: f32,
    transform: &Transform2D,
) -> [QuadVertex; 4] {
    let [bx, by, bw, bh] = entry.bounds;
    let sx = bx * scale_factor;
    let sy = by * scale_factor;
    let sw = bw * scale_factor;
    let sh = bh * scale_factor;

    let aw = atlas_width.max(1) as f32;
    let ah = atlas_height.max(1) as f32;
    let u0 = region.x as f32 / aw;
    let v0 = region.y as f32 / ah;
    let u1 = (region.x + region.w) as f32 / aw;
    let v1 = (region.y + region.h) as f32 / ah;

    let color = [
        entry.color[0],
        entry.color[1],
        entry.color[2],
        entry.color[3] * opacity,
    ];

    let positions = [
        apply_transform_pixel([sx, sy], transform),
        apply_transform_pixel([sx + sw, sy], transform),
        apply_transform_pixel([sx + sw, sy + sh], transform),
        apply_transform_pixel([sx, sy + sh], transform),
    ];
    let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];

    // Path atlas contents are pre-multiplied RGBA matching `entry.color`,
    // so the monochrome path (`flags = 0`) renders correctly: the shader
    // outputs `vertex.rgb * tex.a`, equivalent to `path_color * path_alpha`.
    [
        QuadVertex {
            position: positions[0],
            tex_coord: uvs[0],
            color,
            flags: 0,
            _pad: 0,
        },
        QuadVertex {
            position: positions[1],
            tex_coord: uvs[1],
            color,
            flags: 0,
            _pad: 0,
        },
        QuadVertex {
            position: positions[2],
            tex_coord: uvs[2],
            color,
            flags: 0,
            _pad: 0,
        },
        QuadVertex {
            position: positions[3],
            tex_coord: uvs[3],
            color,
            flags: 0,
            _pad: 0,
        },
    ]
}

fn pixel_to_ndc(pixel: [f32; 2], viewport_width: u32, viewport_height: u32) -> [f32; 2] {
    let x = (pixel[0] / viewport_width as f32) * 2.0 - 1.0;
    let y = 1.0 - (pixel[1] / viewport_height as f32) * 2.0; // flip Y
    [x, y]
}

/// Apply a 2D affine transform to pixel coordinates.
fn apply_transform_pixel(pixel: [f32; 2], transform: &Transform2D) -> [f32; 2] {
    let [a, b, c, d, tx, ty] = transform.m;
    [
        a * pixel[0] + c * pixel[1] + tx,
        b * pixel[0] + d * pixel[1] + ty,
    ]
}

/// Result of running the dual-Kawase chain on a `BlurScope`'s
/// intermediate. The returned texture is the final upsampled level —
/// it shares the same bucket-size convention as the input (only
/// `(used_w, used_h)` of `(bucket_w, bucket_h)` holds rendered
/// content), so the caller maps UVs as `used / bucket`.
struct KawaseResult {
    texture: crate::blur::AcquiredTexture,
    used_w: u32,
    used_h: u32,
    bucket_w: u32,
    bucket_h: u32,
}

/// Run a dual-Kawase blur chain on `source`. The chain depth is chosen
/// from the requested radius; each pass halves (downsample) or doubles
/// (upsample) the active region's size. Returns the final upsampled
/// texture handle (which may be the input handle itself if the chain
/// is a single round-trip).
#[allow(clippy::too_many_arguments)]
fn run_kawase_chain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pool: &mut crate::blur::BlurPool,
    pipelines: &crate::blur::BlurPipelines,
    source: crate::blur::AcquiredTexture,
    used_w: u32,
    used_h: u32,
    bucket_w: u32,
    bucket_h: u32,
    radius_device_px: f32,
) -> KawaseResult {
    let levels = crate::blur::kawase_levels(radius_device_px);

    // Track the chain as (handle, used_w, used_h, bucket_w, bucket_h).
    // Each downsample halves used_w/h; the bucket size we sample from
    // is the *previous* level's bucket.
    let mut current = (source, used_w, used_h, bucket_w, bucket_h);

    // Upsample needs to know all intermediate bucket sizes so we can
    // walk back up. Stash one entry per chain level (input + each
    // downsample target).
    let mut chain: Vec<(crate::blur::AcquiredTexture, u32, u32, u32, u32)> =
        Vec::with_capacity(levels as usize + 1);
    chain.push(current);

    // Per-pass kernel offset multiplier. Bjørge's reference uses 0.5
    // for both passes; the actual blur radius this produces is
    // proportional to `2^levels * 0.5`, which roughly matches the
    // requested Gaussian-equivalent radius for typical UI values.
    const KERNEL_OFFSET: f32 = 0.5;

    // Downsample chain: source → mip1 → mip2 → ...
    for _ in 0..levels {
        let (src_handle, src_used_w, src_used_h, src_bucket_w, src_bucket_h) = current;
        let dst_used_w = (src_used_w / 2).max(1);
        let dst_used_h = (src_used_h / 2).max(1);
        let dst = pool.acquire(device, dst_used_w, dst_used_h);
        let (dst_bucket_w, dst_bucket_h) = pool.dimensions(dst);

        // Build per-pass uniforms: source-bucket UV-offset.
        let params = crate::blur::BlurParams {
            offset: crate::blur::kawase_offset(src_bucket_w, src_bucket_h, KERNEL_OFFSET),
        };
        queue.write_buffer(&pipelines.params_buffer, 0, bytemuck::bytes_of(&params));
        let bind_group = pool.make_bind_group(device, src_handle, &pipelines.params_buffer);

        run_kawase_pass(
            encoder,
            &pipelines.down,
            &bind_group,
            pool.view(dst),
            dst_used_w,
            dst_used_h,
            "kawase_down_pass",
        );

        current = (dst, dst_used_w, dst_used_h, dst_bucket_w, dst_bucket_h);
        chain.push(current);
    }

    // Upsample chain: mipN → mipN-1 → ... → mip0 (a fresh allocation;
    // we don't write back into the source texture because some Kawase
    // implementations rely on the source bucket's content surviving).
    for level in (0..levels).rev() {
        let (src_handle, _src_used_w, _src_used_h, src_bucket_w, src_bucket_h) = current;
        let target = chain[level as usize];
        let dst_used_w = target.1;
        let dst_used_h = target.2;
        let dst = pool.acquire(device, dst_used_w, dst_used_h);
        let (dst_bucket_w, dst_bucket_h) = pool.dimensions(dst);

        let params = crate::blur::BlurParams {
            offset: crate::blur::kawase_offset(src_bucket_w, src_bucket_h, KERNEL_OFFSET),
        };
        queue.write_buffer(&pipelines.params_buffer, 0, bytemuck::bytes_of(&params));
        let bind_group = pool.make_bind_group(device, src_handle, &pipelines.params_buffer);

        run_kawase_pass(
            encoder,
            &pipelines.up,
            &bind_group,
            pool.view(dst),
            dst_used_w,
            dst_used_h,
            "kawase_up_pass",
        );

        current = (dst, dst_used_w, dst_used_h, dst_bucket_w, dst_bucket_h);
    }

    KawaseResult {
        texture: current.0,
        used_w: current.1,
        used_h: current.2,
        bucket_w: current.3,
        bucket_h: current.4,
    }
}

/// Run one full-screen-triangle Kawase pass. The viewport is set to
/// `(used_w, used_h)` — the destination bucket may be larger but we
/// only write the upper-left sub-rect that the next pass will sample
/// from.
fn run_kawase_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    target_view: &wgpu::TextureView,
    used_w: u32,
    used_h: u32,
    label: &str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    // Full-screen triangle covers the whole viewport — restricting the
    // viewport to the used sub-rect keeps the over-allocated bucket
    // clean and (more importantly) limits the fragment work.
    pass.set_viewport(0.0, 0.0, used_w as f32, used_h as f32, 0.0, 1.0);
    pass.draw(0..3, 0..1);
}

/// Composite the final blurred intermediate onto the parent target as
/// a textured quad at `bounds` (logical pixels). Uses the same quad
/// pipeline as static images: builds 4 vertices in NDC with image
/// flag set, binds the intermediate texture + sampler, and issues one
/// indexed draw.
///
/// `index_binding` is the per-frame index buffer (the first 6 u16s
/// already encode the standard quad index pattern, so we slice 12
/// bytes off the front).
#[allow(clippy::too_many_arguments)]
fn composite_blur_quad(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
    pool: &crate::blur::BlurPool,
    quad_pipeline: &wgpu::RenderPipeline,
    quad_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    quad_stream: &crate::stream_buffer::StreamBuffer,
    index_binding: Option<(&wgpu::Buffer, u64, u64)>,
    blurred: crate::blur::AcquiredTexture,
    used_w: u32,
    used_h: u32,
    bucket_w: u32,
    bucket_h: u32,
    bounds: fern_canvas::Rect,
    scale_factor: f32,
    viewport_width: u32,
    viewport_height: u32,
) {
    let view = pool.view(blurred);
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur_composite_bind_group"),
        layout: quad_bind_group_layout,
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

    // Vertex positions in device pixels, converted to NDC.
    let sx = bounds.x * scale_factor;
    let sy = bounds.y * scale_factor;
    let sw = bounds.width * scale_factor;
    let sh = bounds.height * scale_factor;

    // UVs map the used sub-rect inside the bucket. The bucket's
    // upper-left holds the rendered content; the rest is the
    // cleared-to-transparent padding from the bucket's allocation.
    let u_max = used_w as f32 / bucket_w as f32;
    let v_max = used_h as f32 / bucket_h as f32;

    // Image flag (bit 0 = 1 → fragment shader uses tex.rgb directly).
    let flags = 1u32;
    let color = [1.0, 1.0, 1.0, 1.0];

    let p_tl = pixel_to_ndc([sx, sy], viewport_width, viewport_height);
    let p_tr = pixel_to_ndc([sx + sw, sy], viewport_width, viewport_height);
    let p_br = pixel_to_ndc([sx + sw, sy + sh], viewport_width, viewport_height);
    let p_bl = pixel_to_ndc([sx, sy + sh], viewport_width, viewport_height);

    let verts: [QuadVertex; 4] = [
        QuadVertex { position: p_tl, tex_coord: [0.0, 0.0],   color, flags, _pad: 0 },
        QuadVertex { position: p_tr, tex_coord: [u_max, 0.0], color, flags, _pad: 0 },
        QuadVertex { position: p_br, tex_coord: [u_max, v_max], color, flags, _pad: 0 },
        QuadVertex { position: p_bl, tex_coord: [0.0, v_max], color, flags, _pad: 0 },
    ];

    // Caller has already sized `quad_stream` for the worst-case quad
    // count *including composites* (see render()'s up-front sizing).
    // The index buffer's first 6 u16s = `[0, 1, 2, 0, 2, 3]` (the
    // standard quad pattern), reused here.
    let _ = device; // device is only used for bind-group creation above
    let Some((vb, v_off, v_len)) = quad_stream.write(queue, bytemuck::cast_slice(&verts))
    else {
        return;
    };
    let Some((ib, _, _)) = index_binding else { return };
    let composite_index_bytes: u64 = 6 * std::mem::size_of::<u16>() as u64;

    pass.set_pipeline(quad_pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.set_viewport(0.0, 0.0, viewport_width as f32, viewport_height as f32, 0.0, 1.0);
    pass.set_vertex_buffer(0, vb.slice(v_off..v_off + v_len));
    pass.set_index_buffer(ib.slice(0..composite_index_bytes), wgpu::IndexFormat::Uint16);
    pass.draw_indexed(0..6, 0, 0..1);
}

// --- Pipeline creation ---

fn create_rect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("rect_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/rect.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rect_pipeline_layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rect_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<RectVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    },
                    wgpu::VertexAttribute {
                        offset: 8,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x4,
                    },
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

fn create_sdf_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sdf_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sdf.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sdf_pipeline_layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sdf_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SdfVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2, // position
                    },
                    wgpu::VertexAttribute {
                        offset: 8,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x2, // local_uv
                    },
                    wgpu::VertexAttribute {
                        offset: 16,
                        shader_location: 2,
                        format: wgpu::VertexFormat::Float32x4, // color
                    },
                    wgpu::VertexAttribute {
                        offset: 32,
                        shader_location: 3,
                        format: wgpu::VertexFormat::Float32x4, // corner_radii
                    },
                    wgpu::VertexAttribute {
                        offset: 48,
                        shader_location: 4,
                        format: wgpu::VertexFormat::Float32x4, // shape_params
                    },
                    wgpu::VertexAttribute {
                        offset: 64,
                        shader_location: 5,
                        format: wgpu::VertexFormat::Float32x4, // gradient_geo
                    },
                    wgpu::VertexAttribute {
                        offset: 80,
                        shader_location: 6,
                        format: wgpu::VertexFormat::Float32x4, // gradient_color0
                    },
                    wgpu::VertexAttribute {
                        offset: 96,
                        shader_location: 7,
                        format: wgpu::VertexFormat::Float32x4, // gradient_color1
                    },
                    wgpu::VertexAttribute {
                        offset: 112,
                        shader_location: 8,
                        format: wgpu::VertexFormat::Float32x4, // gradient_color2
                    },
                    wgpu::VertexAttribute {
                        offset: 128,
                        shader_location: 9,
                        format: wgpu::VertexFormat::Float32x4, // gradient_color3
                    },
                    wgpu::VertexAttribute {
                        offset: 144,
                        shader_location: 10,
                        format: wgpu::VertexFormat::Float32x4, // gradient_offsets
                    },
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

fn create_quad_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("quad_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/quad.wgsl").into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("quad_bind_group_layout"),
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
        ],
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("quad_pipeline_layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("quad_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<QuadVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2, // position
                    },
                    wgpu::VertexAttribute {
                        offset: 8,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x2, // tex_coord
                    },
                    wgpu::VertexAttribute {
                        offset: 16,
                        shader_location: 2,
                        format: wgpu::VertexFormat::Float32x4, // color
                    },
                    wgpu::VertexAttribute {
                        offset: 32,
                        shader_location: 3,
                        format: wgpu::VertexFormat::Uint32, // flags (bit 0 = color glyph)
                    },
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

fn create_shadow_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow_pipeline_layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ShadowVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2, // position
                    },
                    wgpu::VertexAttribute {
                        offset: 8,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x2, // local_uv
                    },
                    wgpu::VertexAttribute {
                        offset: 16,
                        shader_location: 2,
                        format: wgpu::VertexFormat::Float32x4, // shadow_color
                    },
                    wgpu::VertexAttribute {
                        offset: 32,
                        shader_location: 3,
                        format: wgpu::VertexFormat::Float32x4, // corner_radii
                    },
                    wgpu::VertexAttribute {
                        offset: 48,
                        shader_location: 4,
                        format: wgpu::VertexFormat::Float32x4, // shadow_params
                    },
                    wgpu::VertexAttribute {
                        offset: 64,
                        shader_location: 5,
                        format: wgpu::VertexFormat::Float32x4, // shape_offset
                    },
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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

/// Build the procedural-animation pipeline plus its per-slot uniform
/// buffer, bind group, and bind-group layout. The layout is returned
/// so the sprite pipeline can reuse it as its `group 0`. Buffer is
/// sized for [`MAX_ANIM_SLOTS`] × `size_of::<fern_canvas::AnimParams>()`;
/// the tree's registry truncates writes past that cap.
fn create_anim_proc_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (
    wgpu::RenderPipeline,
    wgpu::Buffer,
    wgpu::BindGroup,
    wgpu::BindGroupLayout,
) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("anim_procedural_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/anim_procedural.wgsl").into()),
    });

    let buffer_size = (MAX_ANIM_SLOTS * std::mem::size_of::<fern_canvas::AnimParams>()) as u64;
    let anim_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("anim_uniform_buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("anim_uniform_bind_group_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let anim_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("anim_uniform_bind_group"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: anim_uniform_buffer.as_entire_binding(),
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("anim_proc_pipeline_layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("anim_proc_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[anim_quad_vertex_layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
    });

    (
        pipeline,
        anim_uniform_buffer,
        anim_uniform_bind_group,
        bind_group_layout,
    )
}

/// Build the sprite-atlas animation pipeline. Shares group 0 (the
/// per-slot uniform buffer) with the procedural pipeline; adds group
/// 1 = sprite atlas texture + sampler, resolved per-draw via
/// `ImageManager::get_bind_group(image_name)`. Returns the pipeline
/// and the texture bind-group layout (so `ImageManager` can register
/// images under the same layout).
fn create_anim_sprite_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    uniform_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("anim_sprite_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/anim_sprite.wgsl").into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("anim_sprite_pipeline_layout"),
        bind_group_layouts: &[Some(uniform_layout), Some(texture_layout)],
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("anim_sprite_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[anim_quad_vertex_layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
    });

    pipeline
}

/// Vertex buffer layout shared by both animated-quad pipelines.
fn anim_quad_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 3] = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: 8,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: 16,
            shader_location: 2,
            format: wgpu::VertexFormat::Uint32,
        },
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<AnimQuadVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    }
}

#[cfg(test)]
mod tests {
    use fern_canvas::RenderFrame;
    use fern_canvas::render_frame::{DrawCommand, GlyphQuad, PaintData, ShapeKind, ShapeQuad};

    use super::*;

    #[test]
    fn glyph_quad_renders_over_shape_in_offscreen_target() {
        let Some((mut renderer, device, queue)) = pollster::block_on(
            crate::test_support::create_test_renderer("fern_render_test_device"),
        ) else {
            return;
        };

        renderer.upload_atlas(
            2,
            2,
            &[
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ],
        );

        let mut frame = RenderFrame::new();
        frame.shapes.push(ShapeQuad {
            screen: [4.0, 4.0, 24.0, 24.0],
            color: [0.2, 0.6, 0.9, 1.0],
            shape: ShapeKind::RoundedRect,
            stroke_width: 0.0,
            corner_radii: [0.0; 4],
            paint_data: PaintData::Solid,
        });
        frame.draw_order.push(DrawCommand::Shape(0));

        frame.glyphs.push(GlyphQuad {
            screen: [10.0, 10.0, 8.0, 8.0],
            atlas: [0.0, 0.0, 2.0, 2.0],
            color: [1.0, 1.0, 1.0, 1.0],
            is_color: false,
        });
        frame.draw_order.push(DrawCommand::Glyph(0));

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fern_render_test_target"),
            size: wgpu::Extent3d {
                width: 32,
                height: 32,
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

        renderer.render(&frame, &view, 1.0, 32, 32, [0.0, 0.0, 0.0, 0.0]);

        let pixels = crate::test_support::read_texture_rgba(&device, &queue, &texture, 32, 32);
        let center = ((14 * 32 + 14) * 4) as usize;
        let blue_only = [
            pixels[center],
            pixels[center + 1],
            pixels[center + 2],
            pixels[center + 3],
        ];

        assert!(
            blue_only[0] > 200 && blue_only[1] > 200 && blue_only[2] > 200,
            "expected glyph pixel to be visible over shape, got {:?}",
            blue_only
        );
    }
}
