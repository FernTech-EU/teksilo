pub mod app;
pub mod command_context;
pub mod window_config;
pub mod window_manager;

pub use app::{AppEventProxy, FernAppBuilder, HeadlessApp};
pub use command_context::CommandContext;
pub use window_config::{FernWindowId, WindowConfig};
pub use window_manager::WindowManager;

// Re-export key types for convenience
pub use fern_core;
pub use fern_tokens;
pub use fern_canvas;
pub use fern_text;
