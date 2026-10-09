// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub(crate) mod blur;
pub mod device_health;
pub(crate) mod fullscreen;
pub mod gpu_reclaim;
pub mod image_manager;
pub mod instance;
pub(crate) mod live_staging;
pub(crate) mod live_texture;
#[cfg(any(debug_assertions, feature = "live-image-timings"))]
pub mod live_timings;
pub(crate) mod mipmap;
pub mod path_atlas;
pub mod renderer;
pub mod stream_buffer;
pub mod test_support;
pub mod vertex;

pub use device_health::DeviceHealth;
pub use gpu_reclaim::poll_gpu_reclaim;
pub use image_manager::ImageManager;
pub use instance::instance_flags;
#[cfg(any(debug_assertions, feature = "live-image-timings"))]
pub use live_timings::{LiveImageTimings, Percentiles};
pub use path_atlas::PathAtlas;
pub use renderer::Renderer;
pub use teksilo_canvas::live_image::LiveTextureStats;
pub use vertex::{QuadVertex, RectVertex, SdfVertex, ShadowVertex};
