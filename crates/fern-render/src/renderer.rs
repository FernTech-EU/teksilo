use wgpu;
use wgpu::util::DeviceExt;

use fern_canvas::RenderFrame;

use crate::vertex::{QuadVertex, RectVertex, SdfVertex};

/// GPU renderer that draws a RenderFrame using three shader pipelines.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    rect_pipeline: wgpu::RenderPipeline,
    sdf_pipeline: wgpu::RenderPipeline,
    quad_pipeline: wgpu::RenderPipeline,
    atlas_texture: Option<AtlasTexture>,
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

        Self {
            device,
            queue,
            rect_pipeline,
            sdf_pipeline,
            quad_pipeline,
            atlas_texture: None,
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
        &self,
        frame: &RenderFrame,
        view: &wgpu::TextureView,
        scale_factor: f32,
        viewport_width: u32,
        viewport_height: u32,
        clear_color: [f32; 4],
    ) {
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

            // Draw in painter's order
            for cmd in &frame.draw_order {
                match cmd {
                    fern_canvas::DrawCommand::Decoration(idx) => {
                        self.draw_rect(&mut pass, &frame.decorations[*idx], scale_factor, viewport_width, viewport_height);
                    }
                    fern_canvas::DrawCommand::Shape(idx) => {
                        self.draw_sdf(&mut pass, &frame.shapes[*idx], scale_factor, viewport_width, viewport_height);
                    }
                    fern_canvas::DrawCommand::Glyph(idx) => {
                        self.draw_quad(&mut pass, &frame.glyphs[*idx], scale_factor, viewport_width, viewport_height);
                    }
                    fern_canvas::DrawCommand::SetClip(rect) => {
                        let x = (rect.x * scale_factor) as u32;
                        let y = (rect.y * scale_factor) as u32;
                        let w = (rect.width * scale_factor).ceil() as u32;
                        let h = (rect.height * scale_factor).ceil() as u32;
                        // Clamp to surface bounds
                        let w = w.min(viewport_width.saturating_sub(x));
                        let h = h.min(viewport_height.saturating_sub(y));
                        pass.set_scissor_rect(x, y, w, h);
                    }
                    fern_canvas::DrawCommand::ClearClip => {
                        pass.set_scissor_rect(0, 0, viewport_width, viewport_height);
                    }
                    // TODO: implement these when their rendering pipelines are ready
                    fern_canvas::DrawCommand::Image(_) => {}
                    fern_canvas::DrawCommand::Rasterized(_) => {}
                    fern_canvas::DrawCommand::SetOpacity(_) => {}
                    fern_canvas::DrawCommand::RestoreOpacity => {}
                    fern_canvas::DrawCommand::Path(_) => {}
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
    ) {
        let verts = RectVertex::from_decoration(rect, scale_factor);
        let indices = [0u16, 1, 2, 0, 2, 3];

        // Convert to NDC
        let ndc_verts: Vec<RectVertex> = verts
            .iter()
            .map(|v| RectVertex {
                position: pixel_to_ndc(v.position, viewport_width, viewport_height),
                color: v.color,
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
    ) {
        let verts = SdfVertex::from_shape_quad(shape, scale_factor);
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<SdfVertex> = verts
            .iter()
            .map(|v| SdfVertex {
                position: pixel_to_ndc(v.position, viewport_width, viewport_height),
                ..*v
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
    ) {
        let atlas = match &self.atlas_texture {
            Some(a) => a,
            None => return,
        };

        let verts = QuadVertex::from_glyph_quad(glyph, scale_factor, atlas.width, atlas.height);
        let indices = [0u16, 1, 2, 0, 2, 3];

        let ndc_verts: Vec<QuadVertex> = verts
            .iter()
            .map(|v| QuadVertex {
                position: pixel_to_ndc(v.position, viewport_width, viewport_height),
                ..*v
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

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
}

/// Convert pixel coordinates to NDC (-1..1).
fn pixel_to_ndc(pixel: [f32; 2], viewport_width: u32, viewport_height: u32) -> [f32; 2] {
    let x = (pixel[0] / viewport_width as f32) * 2.0 - 1.0;
    let y = 1.0 - (pixel[1] / viewport_height as f32) * 2.0; // flip Y
    [x, y]
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
