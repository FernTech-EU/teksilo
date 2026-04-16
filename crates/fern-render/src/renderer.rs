use wgpu;

use fern_canvas::RenderFrame;
use fern_canvas::geometry::Transform2D;

use crate::image_manager::ImageManager;
use crate::path_atlas::PathAtlas;
use crate::stream_buffer::StreamBuffers;
use crate::vertex::{QuadVertex, RectVertex, SdfVertex, ShadowVertex};

/// GPU renderer that draws a RenderFrame using four shader pipelines.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    rect_pipeline: wgpu::RenderPipeline,
    sdf_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    atlas_texture: Option<AtlasTexture>,
    path_atlas: PathAtlas,
    path_atlas_texture: Option<AtlasTexture>,
    image_manager: ImageManager,
    /// Persistent per-pipeline streaming buffers. Resized on demand at
    /// the top of each `render()` call, then reused via `write_buffer`
    /// for every batch flush in that frame — replaces the historical
    /// per-flush `create_buffer_init` antipattern.
    streams: StreamBuffers,
}

struct AtlasTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
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

        Self {
            device,
            queue,
            rect_pipeline,
            sdf_pipeline,
            quad_pipeline,
            shadow_pipeline,
            atlas_texture: None,
            path_atlas: PathAtlas::new(512, 512),
            path_atlas_texture: None,
            image_manager: ImageManager::new(),
            streams: StreamBuffers::new(),
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
        let quad_quads = frame.glyphs.len() + frame.paths.len() + frame.images.len();
        let shadow_quads = frame.shadows.len();
        let max_quads = rect_quads
            .max(sdf_quads)
            .max(quad_quads)
            .max(shadow_quads);

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
        self.streams.index.ensure_capacity(
            &self.device,
            (max_quads * 6 * std::mem::size_of::<u16>()) as u64,
        );
        self.streams.reset();

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

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fern_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

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

            // Transform stack — applied CPU-side to pixel positions before NDC conversion
            let mut current_transform = Transform2D::IDENTITY;

            // --- Batched rendering ---
            // Accumulate vertices per pipeline, flush on state/pipeline changes.
            // This produces one GPU buffer + one draw call per contiguous batch
            // instead of two buffers per quad.
            let mut rect_batch: Vec<RectVertex> = Vec::new();
            let mut sdf_batch: Vec<SdfVertex> = Vec::new();
            let mut quad_batch: Vec<QuadVertex> = Vec::new();
            let mut shadow_batch: Vec<ShadowVertex> = Vec::new();

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
                };
            }

            // Draw in painter's order
            for cmd in &frame.draw_order {
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
                            &mut pass,
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
                        current_transform = *t;
                    }
                }
            }

            // Flush remaining batches
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
