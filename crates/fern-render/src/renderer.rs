use wgpu;
use wgpu::util::DeviceExt;

use fern_canvas::geometry::Transform2D;
use fern_canvas::RenderFrame;

use crate::image_manager::ImageManager;
use crate::path_atlas::PathAtlas;
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
    surface_format: wgpu::TextureFormat,
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
            surface_format,
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
            .map_or(true, |t| t.width != width || t.height != height);

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

            // Draw in painter's order
            for cmd in &frame.draw_order {
                match cmd {
                    fern_canvas::DrawCommand::Decoration(idx) => {
                        self.draw_rect(&mut pass, &frame.decorations[*idx], scale_factor, viewport_width, viewport_height, current_opacity, &current_transform);
                    }
                    fern_canvas::DrawCommand::Shape(idx) => {
                        self.draw_sdf(&mut pass, &frame.shapes[*idx], scale_factor, viewport_width, viewport_height, current_opacity, &current_transform);
                    }
                    fern_canvas::DrawCommand::Glyph(idx) => {
                        self.draw_quad(&mut pass, &frame.glyphs[*idx], scale_factor, viewport_width, viewport_height, current_opacity, &current_transform);
                    }
                    fern_canvas::DrawCommand::Shadow(idx) => {
                        self.draw_shadow(&mut pass, &frame.shadows[*idx], scale_factor, viewport_width, viewport_height, current_opacity, &current_transform);
                    }
                    fern_canvas::DrawCommand::SetClip(rect) => {
                        let x = (rect.x * scale_factor) as u32;
                        let y = (rect.y * scale_factor) as u32;
                        let w = (rect.width * scale_factor).ceil() as u32;
                        let h = (rect.height * scale_factor).ceil() as u32;
                        let w = w.min(viewport_width.saturating_sub(x));
                        let h = h.min(viewport_height.saturating_sub(y));
                        // Intersect with current clip (if any) for nesting
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
                        clip_stack.pop();
                        if let Some(&[x, y, w, h]) = clip_stack.last() {
                            pass.set_scissor_rect(x, y, w, h);
                        } else {
                            pass.set_scissor_rect(0, 0, viewport_width, viewport_height);
                        }
                    }
                    fern_canvas::DrawCommand::SetOpacity(opacity) => {
                        opacity_stack.push(current_opacity);
                        current_opacity *= opacity;
                    }
                    fern_canvas::DrawCommand::RestoreOpacity => {
                        current_opacity = opacity_stack.pop().unwrap_or(1.0);
                    }
                    fern_canvas::DrawCommand::Image(idx) => {
                        let image = &frame.images[*idx];
                        self.draw_image(&mut pass, image, scale_factor, viewport_width, viewport_height, current_opacity, &current_transform);
                    }
                    fern_canvas::DrawCommand::Rasterized(_) => {}
                    fern_canvas::DrawCommand::Path(idx) => {
                        if let Some(Some(region)) = path_regions.get(*idx) {
                            let entry = &frame.paths[*idx];
                            self.draw_path_quad(&mut pass, entry, region, scale_factor, viewport_width, viewport_height, current_opacity, &current_transform);
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
                        current_transform = *t;
                    }
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn draw_rect(
        &self,
        pass: &mut wgpu::RenderPass,
        rect: &fern_canvas::DecorationRect,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
    ) {
        let verts = RectVertex::from_decoration(rect, scale_factor);
        let indices = [0u16, 1, 2, 0, 2, 3];

        // Apply transform in pixel space, then convert to NDC
        let ndc_verts: Vec<RectVertex> = verts
            .iter()
            .map(|v| {
                let tp = apply_transform_pixel(v.position, transform);
                RectVertex {
                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                    color: [v.color[0], v.color[1], v.color[2], v.color[3] * opacity],
                }
            })
            .collect();

        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&ndc_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        pass.set_pipeline(&self.rect_pipeline);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    fn draw_sdf(
        &self,
        pass: &mut wgpu::RenderPass,
        shape: &fern_canvas::ShapeQuad,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
    ) {
        let verts = SdfVertex::from_shape_quad(shape, scale_factor);
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<SdfVertex> = verts
            .iter()
            .map(|v| {
                let tp = apply_transform_pixel(v.position, transform);
                SdfVertex {
                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                    color: [v.color[0], v.color[1], v.color[2], v.color[3] * opacity],
                    ..*v
                }
            })
            .collect();

        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&ndc_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        pass.set_pipeline(&self.sdf_pipeline);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    fn draw_quad(
        &self,
        pass: &mut wgpu::RenderPass,
        glyph: &fern_canvas::GlyphQuad,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
    ) {
        let atlas = match &self.atlas_texture {
            Some(a) => a,
            None => return,
        };

        let verts = QuadVertex::from_glyph_quad(glyph, scale_factor, atlas.width, atlas.height);
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<QuadVertex> = verts
            .iter()
            .map(|v| {
                let tp = apply_transform_pixel(v.position, transform);
                QuadVertex {
                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                    color: [v.color[0], v.color[1], v.color[2], v.color[3] * opacity],
                    ..*v
                }
            })
            .collect();

        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&ndc_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        pass.set_pipeline(&self.quad_pipeline);
        pass.set_bind_group(0, &atlas.bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    fn draw_shadow(
        &self,
        pass: &mut wgpu::RenderPass,
        shadow: &fern_canvas::ShadowQuad,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
    ) {
        let verts = ShadowVertex::from_shadow_quad(shadow, scale_factor);
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<ShadowVertex> = verts
            .iter()
            .map(|v| {
                let tp = apply_transform_pixel(v.position, transform);
                ShadowVertex {
                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                    shadow_color: [v.shadow_color[0], v.shadow_color[1], v.shadow_color[2], v.shadow_color[3] * opacity],
                    ..*v
                }
            })
            .collect();

        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&ndc_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        pass.set_pipeline(&self.shadow_pipeline);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    fn draw_path_quad(
        &self,
        pass: &mut wgpu::RenderPass,
        entry: &fern_canvas::PathEntry,
        region: &crate::path_atlas::AtlasRegion,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
    ) {
        let atlas = match &self.path_atlas_texture {
            Some(a) => a,
            None => return,
        };

        let [bx, by, bw, bh] = entry.bounds;
        let sx = bx * scale_factor;
        let sy = by * scale_factor;
        let sw = bw * scale_factor;
        let sh = bh * scale_factor;

        // Normalize atlas region to 0..1 UVs
        let aw = atlas.width.max(1) as f32;
        let ah = atlas.height.max(1) as f32;
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

        let verts = [
            QuadVertex { position: [sx, sy], tex_coord: [u0, v0], color },
            QuadVertex { position: [sx + sw, sy], tex_coord: [u1, v0], color },
            QuadVertex { position: [sx + sw, sy + sh], tex_coord: [u1, v1], color },
            QuadVertex { position: [sx, sy + sh], tex_coord: [u0, v1], color },
        ];
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<QuadVertex> = verts
            .iter()
            .map(|v| {
                let tp = apply_transform_pixel(v.position, transform);
                QuadVertex {
                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                    ..*v
                }
            })
            .collect();

        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&ndc_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        pass.set_pipeline(&self.quad_pipeline);
        pass.set_bind_group(0, &atlas.bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    fn draw_image(
        &self,
        pass: &mut wgpu::RenderPass,
        image: &fern_canvas::ImageQuad,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        opacity: f32,
        transform: &Transform2D,
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

        let color = [1.0, 1.0, 1.0, opacity];

        let verts = [
            QuadVertex { position: [sx, sy], tex_coord: [0.0, 0.0], color },
            QuadVertex { position: [sx + sw, sy], tex_coord: [1.0, 0.0], color },
            QuadVertex { position: [sx + sw, sy + sh], tex_coord: [1.0, 1.0], color },
            QuadVertex { position: [sx, sy + sh], tex_coord: [0.0, 1.0], color },
        ];
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<QuadVertex> = verts
            .iter()
            .map(|v| {
                let tp = apply_transform_pixel(v.position, transform);
                QuadVertex {
                    position: pixel_to_ndc(tp, viewport_width, viewport_height),
                    ..*v
                }
            })
            .collect();

        let vertex_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&ndc_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        let index_buffer =
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        pass.set_pipeline(&self.quad_pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
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
            .map_or(true, |t| t.width != width || t.height != height);

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
        self.image_manager
            .register_image(name, width, height, pixels, &self.device, &self.queue, &layout);
    }

    /// Remove a registered image.
    pub fn remove_image(&mut self, name: &str) {
        self.image_manager.remove(name);
    }
}

/// Convert pixel coordinates to NDC (-1..1).
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

fn create_sdf_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
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
