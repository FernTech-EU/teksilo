pub mod app;
pub mod window_config;
pub mod window_manager;

pub use app::{AppEventProxy, FernAppBuilder, HeadlessApp, ThemeMode};
pub use window_config::{FernWindowId, ModalConfig, RootBuilder, WindowConfig};
pub use window_manager::WindowManager;

// Re-export the fern-core multi-window types so `use fern_app::...`
// continues to work after the types moved into fern-core.
pub use fern_core::{
    DecorationsMode, UserAttentionKind, WindowCommand, WindowOps, WindowPlacement, WindowState,
};

// Re-export key types for convenience
pub use fern_canvas;
pub use fern_core;
pub use fern_text;
pub use fern_tokens;
