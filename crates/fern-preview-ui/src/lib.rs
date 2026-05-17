//! GUI for FernUI's widget previewer.
//!
//! This crate is a *library*. Per-application binaries
//! (`fern-widgets-previewer`, `<app>-previewer`) call
//! [`run_previewer`] from their `main`, linking against this crate
//! plus `fern-preview` plus their own widget set with the `preview`
//! feature enabled. Each binary's `inventory` link graph determines
//! which widgets the previewer surfaces.

mod app_state;
mod canvas;
mod cli;
mod inspector;
mod knob_form;
mod navigator;
mod png_export;
mod toolbar;

pub use cli::PreviewerOptions;

use fern_app::{FernAppBuilder, ThemeMode};
use fern_core::WindowConfig;

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
    FernAppBuilder::new()
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
