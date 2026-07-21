// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 styling for the terminal's chrome (the frame *around* the grid — the
//! grid's own colours are the [`ColorScheme`]'s job). Follows the framework's
//! per-widget style-protocol pattern: a `TerminalStyle` trait, a default
//! `RecipeTerminalStyle`, resolved per-call (`.style(...)`) or left to default.

use bastyde_canvas::{Canvas, Rect, StrokeStyle};
use bastyde_core::styles::Theme;

use crate::color_scheme::ColorScheme;

/// The interaction state passed to a [`TerminalStyle`] when painting chrome.
#[derive(Debug, Clone, Copy)]
pub struct TerminalChrome {
    pub focused: bool,
    pub window_active: bool,
}

/// Paints the chrome (background, border, focus ring) around a terminal grid.
pub trait TerminalStyle: 'static {
    /// Padding, in logical pixels, between the widget bounds and the grid's
    /// content area. The grid is inset by this on every edge.
    fn content_inset(&self) -> f32 {
        4.0
    }

    /// Paint the frame. Called before the grid is painted, so the background
    /// fill here sits behind the cells (which paint their own backgrounds).
    fn paint_frame(
        &self,
        canvas: &mut Canvas,
        bounds: Rect,
        theme: &Theme,
        scheme: &ColorScheme,
        chrome: &TerminalChrome,
    );
}

/// The default terminal chrome: the scheme background plus a focus ring that
/// desaturates when the window is inactive (matching the framework's
/// window-active convention).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTerminalStyle;

impl TerminalStyle for RecipeTerminalStyle {
    fn paint_frame(
        &self,
        canvas: &mut Canvas,
        bounds: Rect,
        theme: &Theme,
        scheme: &ColorScheme,
        chrome: &TerminalChrome,
    ) {
        // Fill the whole widget (including the inset padding) with the scheme
        // background so the padding matches the terminal, not the app surface.
        canvas.fill_rect(bounds, scheme.background);

        if chrome.focused {
            let ring = if chrome.window_active {
                theme.colors.focus_ring
            } else {
                theme.colors.border_focused
            };
            canvas.stroke_rect(
                bounds.inset(1.0, 1.0, 1.0, 1.0),
                ring,
                StrokeStyle::solid(1.5),
            );
        }
    }
}
