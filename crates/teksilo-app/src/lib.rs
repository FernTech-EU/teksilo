#![allow(clippy::type_complexity, clippy::too_many_arguments)]
// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod app;
pub mod app_event_observers;
pub(crate) mod application_menu;
#[cfg(feature = "automation")]
pub mod automation_bridge;
pub(crate) mod cross_window;
pub mod default_post_root;
pub(crate) mod glyph_rendering;
pub(crate) mod input_loop;
pub(crate) mod input_routing;
pub(crate) mod intent_routing;
pub(crate) mod native_menu_route;
pub mod native_theme;
pub(crate) mod quit;
pub(crate) mod redraw_gate;
#[cfg(feature = "single-instance")]
pub mod single_instance;
pub mod termination_watch;
pub mod window_config;
pub(crate) mod window_identity;
pub mod window_manager;
pub(crate) mod window_persist;
pub(crate) mod window_recency;

pub use app::{
    AppEventProxy, AppRootBuilder, ExternalCtxHandler, HeadlessApp, QuitPolicy, SyntheticImeInject,
    TeksiloAppBuilder, ThemeMode, WithActivationToken,
};
pub use app_event_observers::AppEventObservers;
#[cfg(feature = "automation")]
pub use automation_bridge::TeksiloAppBuilderAutomationExt;
pub use default_post_root::DefaultPostRoot;
pub use glyph_rendering::{
    EffectiveGlyphRendering, GLYPH_RENDERING_KEY, GlyphRenderingPreference, TEXT_CONTRAST_KEY,
    TextContrastLevel,
};
pub use native_theme::{NativeThemeFactory, NativeThemeRequest, resolve_native_theme};
pub use teksilo_render::TextContrast;
pub use teksilo_text::GlyphRendering;
pub use termination_watch::{TerminationSignalKind, TerminationWatch};
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
