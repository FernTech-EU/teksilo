// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Image texture manager: maps image names to GPU textures for DrawCommand::Image rendering.

use std::collections::HashMap;

/// Manages uploaded image textures and their bind groups.
#[derive(Default)]
pub struct ImageManager {
    images: HashMap<String, ImageEntry>,
}

struct ImageEntry {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl ImageManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an image by name. Uploads RGBA pixel data and creates a bind group
    /// compatible with the quad pipeline's bind group layout.
    #[allow(clippy::too_many_arguments)]
    pub fn register_image(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        pixels: &[u8],
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image_texture"),
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

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
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

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image_bind_group"),
            layout: bind_group_layout,
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

        self.images.insert(
            name.to_string(),
            ImageEntry {
                _texture: texture,
                bind_group,
            },
        );
    }

    /// Get the bind group for a registered image.
    pub fn get_bind_group(&self, name: &str) -> Option<&wgpu::BindGroup> {
        self.images.get(name).map(|e| &e.bind_group)
    }

    /// Check if an image is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.images.contains_key(name)
    }

    /// Remove a registered image.
    pub fn remove(&mut self, name: &str) {
        self.images.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_manager_new_is_empty() {
        let mgr = ImageManager::new();
        assert!(!mgr.contains("test"));
        assert!(mgr.get_bind_group("test").is_none());
    }
}
