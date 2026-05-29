//! Window identity and per-window state.
//!
//! [`BastydeWindowId`] is the opaque, `Copy` id allocated by the app-level
//! window manager and threaded through every public API that needs to
//! refer to a specific window (modal parents, cross-window focus,
//! close requests, …).
//!
//! [`WindowState`] carries the reactive, signal-bound surface for a
//! single window — placement, title, size, position, focus, etc. App
//! code writes to these signals to push state to the OS; OS-originated
//! changes write back into the same signals via `*_from_os` setters
//! guarded by `WindowStateInner::applying_from_os`, so observers that
//! propagate writes to the OS do not re-loop.
//!
//! [`WindowCommand`] is the private queue element produced by those
//! observers; the app-level window manager drains it once per tick and
//! translates each command into a winit call.

pub mod command;
pub mod config;
pub mod decorations;
pub mod icon;
pub mod id;
pub mod menubar_dispatcher;
pub mod ops;
pub mod placement;
pub mod state;

pub use command::{UserAttentionKind, WindowCommand};
pub use config::{ModalConfig, PostRootBuilder, RootBuilder, WindowConfig};
pub use decorations::DecorationsMode;
pub use icon::WindowIcon;
pub use id::BastydeWindowId;
pub use menubar_dispatcher::{MenubarAction, MenubarDispatcher, MenubarGuard, MenubarKeyEvent};
pub use ops::{NoopWindowOps, WindowOps};
pub use placement::WindowPlacement;
pub use state::WindowState;
