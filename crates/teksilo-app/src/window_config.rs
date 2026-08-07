// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Window configuration.
//!
//! [`WindowConfig`], [`ModalConfig`], [`TeksiloWindowId`] all live in
//! [`teksilo_core::window`] — this module re-exports them so downstream
//! `use teksilo_app::WindowConfig` / `teksilo_app::TeksiloWindowId` imports
//! continue to work unchanged.

pub use teksilo_core::window::SizeToContent;
pub use teksilo_core::{
    ModalConfig, PostRootBuilder, RootBuilder, TeksiloWindowId, WindowConfig,
    WindowRemovedCallback, WindowRemovedEvent,
};
