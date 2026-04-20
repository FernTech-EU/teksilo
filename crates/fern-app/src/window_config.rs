//! Window configuration.
//!
//! [`WindowConfig`], [`ModalConfig`], [`FernWindowId`] all live in
//! [`fern_core::window`] — this module re-exports them so downstream
//! `use fern_app::WindowConfig` / `fern_app::FernWindowId` imports
//! continue to work unchanged.

pub use fern_core::{FernWindowId, ModalConfig, RootBuilder, WindowConfig};
