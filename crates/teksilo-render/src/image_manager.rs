// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Image texture manager: maps image names to GPU textures for DrawCommand::Image rendering.
//!
//! Every image is uploaded with a full **mip chain** and sampled trilinearly, so
//! a large source drawn small (a 512 px app icon in a 25 dp title bar, a photo
//! in a thumbnail strip) resolves cleanly instead of aliasing. See
//! the `mipmap` module for how the chain is built — and for the two things that
//! make it correct rather than merely present (linear-light averaging, and
//! premultiplied filtering so transparent texels can't darken their
//! neighbours).

use std::collections::HashMap;

use crate::mipmap::build_mip_chain;

/// Manages uploaded image textures and their bind groups.
///
/// A texture, with its mip chain, stays registered until the renderer that
/// owns this manager is dropped, which for a window's renderer is when the
/// window closes. Nothing in the framework calls [`remove`](Self::remove), so
/// every distinct image name a window has drawn keeps its texture.
#[derive(Default)]
pub struct ImageManager {
    images: HashMap<String, ImageEntry>,
    #[cfg(debug_assertions)]
    texture_count_warning: TextureCountWarning,
}

/// How many image textures one renderer may hold before a debug build warns.
#[cfg(debug_assertions)]
const TEXTURE_COUNT_WARNING_THRESHOLD: usize = 256;

/// Decides when a debug build warns that a renderer holds too many image
/// textures: once, the first time the count goes past
/// [`TEXTURE_COUNT_WARNING_THRESHOLD`].
#[cfg(debug_assertions)]
#[derive(Debug, Default)]
struct TextureCountWarning {
    warned: bool,
}

#[cfg(debug_assertions)]
impl TextureCountWarning {
    /// Whether to warn now that the renderer holds `count` textures.
    fn should_warn(&mut self, count: usize) -> bool {
        if self.warned || count <= TEXTURE_COUNT_WARNING_THRESHOLD {
            return false;
        }
        self.warned = true;
        true
    }
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

        // Levels 1..N (level 0 is `pixels`). Built once, at upload.
        let mips = build_mip_chain(pixels, width, height);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1 + mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Level 0, then each generated level. A texture declaring mip levels it
        // never receives samples as transparent black wherever the sampler
        // reaches them, so every declared level must be written.
        let upload = |level: u32, w: u32, h: u32, data: &[u8]| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        };
        upload(0, width, height, pixels);
        for (level, (w, h, data)) in mips.iter().enumerate() {
            upload(level as u32 + 1, *w, *h, data);
        }

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Trilinear: `mipmap_filter` is what actually engages the chain. Left at
        // its `Nearest` default, a minified image snaps between whole levels and
        // visibly pops as the scale crosses a power of two — and with a
        // single-level texture (the pre-mip behaviour) it would never leave
        // level 0 at all, which is the aliasing this exists to remove.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
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

        #[cfg(debug_assertions)]
        if self.texture_count_warning.should_warn(self.images.len()) {
            eprintln!(
                "teksilo-render: this window holds {} image textures and frees none before it \
                 closes, because every decoded RasterIcon and every ImageWidget::from_raw gets a \
                 texture of its own; decode each image once and clone the RasterIcon.",
                self.images.len()
            );
        }
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

    #[cfg(debug_assertions)]
    #[test]
    fn texture_count_warning_fires_once_past_the_threshold() {
        let mut warning = TextureCountWarning::default();
        for count in 0..=TEXTURE_COUNT_WARNING_THRESHOLD {
            assert!(!warning.should_warn(count), "warned at {count}");
        }
        assert!(
            warning.should_warn(TEXTURE_COUNT_WARNING_THRESHOLD + 1),
            "must warn as the count first goes past the threshold"
        );
        for count in [
            TEXTURE_COUNT_WARNING_THRESHOLD + 1,
            TEXTURE_COUNT_WARNING_THRESHOLD + 2,
            TEXTURE_COUNT_WARNING_THRESHOLD,
            10 * TEXTURE_COUNT_WARNING_THRESHOLD,
        ] {
            assert!(
                !warning.should_warn(count),
                "warned a second time at {count}"
            );
        }
    }
}
