// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! GUI for Teksilo's widget previewer.
//!
//! This crate is a *library*. Per-application binaries
//! (`teksilo-widgets-previewer`, `<app>-previewer`) call
//! [`run_previewer`] from their `main`, linking against this crate
//! plus `teksilo-preview` plus their own widget set with the `preview`
//! feature enabled. Each binary's `inventory` link graph determines
//! which widgets the previewer surfaces.

mod app_state;
mod canvas;
mod cli;
mod doc_export;
mod inspector;
mod knob_form;
mod navigator;
mod png_export;
mod shot;
mod toolbar;

pub use cli::PreviewerOptions;
pub use doc_export::{
    DocExportOptions, DocExportReport, SubjectOutcome, export_doc_images, print_report,
};

use teksilo_app::{TeksiloAppBuilder, ThemeMode};
use teksilo_core::WindowConfig;

/// Launch the previewer window with the given options. Blocks until
/// the window is closed.
pub fn run_previewer(opts: PreviewerOptions) {
    let title = opts.window_title.clone();
    let initial_size = opts.window_size;
    let initial_widget = opts.initial_widget.clone();
    let initial_variant = opts.initial_variant.clone();

    // Resolve the initial theme from the OS desktop palette. This
    // matches the toolbar's default `CanvasTheme::Native` selection
    // so the chrome (which `ctx.set_theme` reskins app-wide) and the
    // toolbar's highlighted button agree on frame 1. We use
    // `ThemeMode::Manual` here rather than `ThemeMode::Native` /
    // `ThemeMode::FollowSystem` because the toolbar's pickers later
    // call `ctx.set_theme(...)` directly — those builder-time modes
    // would override on the next OS event and fight the user's
    // explicit choice.
    let initial_canvas_theme = crate::app_state::CanvasTheme::Native;
    TeksiloAppBuilder::new()
        .theme(initial_canvas_theme.theme())
        .theme_mode(ThemeMode::Manual)
        .initial_window(
            WindowConfig::new()
                .title(title)
                .size(initial_size.0, initial_size.1)
                .root(move |tree, _state| {
                    let root = crate::app_state::PreviewerRoot::new(
                        initial_widget.clone(),
                        initial_variant.clone(),
                    );
                    tree.add(root)
                }),
        )
        .run();
}
