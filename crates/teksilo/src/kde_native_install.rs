// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `native_theme_kde`: give [`ThemeMode::Native`](teksilo_app::ThemeMode::Native)
//! Breeze's geometry on a KDE Plasma desktop.
//!
//! Mirrors the other install hooks in this crate (`toast_install`,
//! `webview_install`): an extension trait on [`TeksiloAppBuilder`](teksilo_app::TeksiloAppBuilder) declared
//! next to its impl, here over `TeksiloAppBuilder::native_theme`, the seam
//! that asks the app for a theme whenever Native mode needs one.
//!
//! ```ignore
//! use teksilo::prelude::*;
//!
//! TeksiloAppBuilder::new()
//!     .theme_mode(ThemeMode::Native)
//!     .native_theme_kde()
//!     .initial_window(WindowConfig::new()
//!         .root(|tree, _state| tree.add(MyRoot::new())))
//!     .run();
//! ```
//!
//! The factory answers with [`kde::native_theme`](teksilo_theme_kde::native_theme):
//! the desktop's own colour scheme, fonts and animation speed in Breeze's
//! shapes, at the density the window is for. It answers only when the
//! desktop reports KDE settings and uses the Breeze widget style. On any
//! other desktop, and on a KDE desktop styled with Fusion, Oxygen or
//! Kvantum, it answers `None`, and the seam falls back to IntUI coloured by
//! the desktop's report. The theme a window shows is given the id `system`,
//! as every theme that follows the OS is.
//!
//! Setting the factory does not select Native mode; combine it with
//! [`theme_mode`](teksilo_app::TeksiloAppBuilder::theme_mode). Only a window
//! built while the app is in Native mode asks the factory, so a switch to
//! Native at runtime re-tints and keeps the shapes the windows were built
//! with, and [`ThemeMode::FollowSystem`](teksilo_app::ThemeMode::FollowSystem)
//! never asks it.

use teksilo_app::{NativeThemeRequest, TeksiloAppBuilder};
use teksilo_core::Theme;

/// Extension trait on [`TeksiloAppBuilder`] that installs the KDE Plasma
/// native theme.
pub trait TeksiloAppBuilderKdeExt {
    /// Make Native mode show Breeze on a Breeze desktop.
    ///
    /// Installs [`kde_native_theme`] as the builder's native-theme factory,
    /// replacing any factory set before.
    fn native_theme_kde(self) -> Self;
}

impl TeksiloAppBuilderKdeExt for TeksiloAppBuilder {
    fn native_theme_kde(self) -> Self {
        self.native_theme(kde_native_theme)
    }
}

/// The factory [`native_theme_kde`](TeksiloAppBuilderKdeExt::native_theme_kde)
/// installs: the Breeze theme of the request's KDE desktop at the request's
/// density, or `None` when the desktop reports no KDE settings or does not
/// use the Breeze widget style.
///
/// Public so an app that composes its own factory (for example one that
/// answers for another desktop too) can call it from there.
pub fn kde_native_theme(request: &NativeThemeRequest<'_>) -> Option<Theme> {
    let desktop = request.desktop.kde.as_ref()?;
    teksilo_theme_kde::native_theme(desktop, request.density)
}
