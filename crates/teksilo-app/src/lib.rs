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

pub use app::{
    AppEventProxy, ExternalCtxHandler, HeadlessApp, SyntheticImeInject, TeksiloAppBuilder,
    ThemeMode,
};
pub use app_event_observers::AppEventObservers;
#[cfg(feature = "automation")]
pub use automation_bridge::TeksiloAppBuilderAutomationExt;
pub use default_post_root::DefaultPostRoot;
pub use window_config::{
    ModalConfig, PostRootBuilder, RootBuilder, TeksiloWindowId, WindowConfig,
    WindowRemovedCallback, WindowRemovedEvent,
};
pub use window_manager::WindowManager;

// Re-export the teksilo-core multi-window types so `use teksilo_app::...`
// continues to work after the types moved into teksilo-core.
pub use teksilo_core::{
    DecorationsMode, UserAttentionKind, WindowCommand, WindowOps, WindowPlacement, WindowState,
};

// Re-export key types for convenience
pub use teksilo_canvas;
pub use teksilo_core;
pub use teksilo_text;
pub use teksilo_tokens;
