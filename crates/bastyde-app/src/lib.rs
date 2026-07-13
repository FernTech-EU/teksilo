#![allow(clippy::type_complexity, clippy::too_many_arguments)]
// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod app;
pub mod app_event_observers;
#[cfg(feature = "automation")]
pub mod automation_bridge;
pub mod default_post_root;
pub mod window_config;
pub mod window_manager;
pub(crate) mod window_persist;

pub use app::{AppEventProxy, BastydeAppBuilder, HeadlessApp, SyntheticImeInject, ThemeMode};
pub use app_event_observers::AppEventObservers;
#[cfg(feature = "automation")]
pub use automation_bridge::BastydeAppBuilderAutomationExt;
pub use default_post_root::DefaultPostRoot;
pub use window_config::{BastydeWindowId, ModalConfig, PostRootBuilder, RootBuilder, WindowConfig};
pub use window_manager::WindowManager;

// Re-export the bastyde-core multi-window types so `use bastyde_app::...`
// continues to work after the types moved into bastyde-core.
pub use bastyde_core::{
    DecorationsMode, UserAttentionKind, WindowCommand, WindowOps, WindowPlacement, WindowState,
};

// Re-export key types for convenience
pub use bastyde_canvas;
pub use bastyde_core;
pub use bastyde_text;
pub use bastyde_tokens;
