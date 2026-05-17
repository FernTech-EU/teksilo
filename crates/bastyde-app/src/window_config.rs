//! Window configuration.
//!
//! [`WindowConfig`], [`ModalConfig`], [`BastydeWindowId`] all live in
//! [`bastyde_core::window`] — this module re-exports them so downstream
//! `use bastyde_app::WindowConfig` / `bastyde_app::BastydeWindowId` imports
//! continue to work unchanged.

pub use bastyde_core::{BastydeWindowId, ModalConfig, PostRootBuilder, RootBuilder, WindowConfig};
