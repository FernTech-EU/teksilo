// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `GridView`. See `docs/styling-system.md`.
//!
//! `GridView` renders its tiles through the app-supplied delegate, so the
//! only widget-owned chrome is the paint-time decoration: the keyboard focus
//! ring, the rubber-band marquee rectangle, the drag-reorder insertion bar,
//! and the sticky pinned-header background. This trait exposes that chrome as
//! plain recipe data (roles + dimensions) resolved against the active theme
//! each frame — the same data-returning pattern as
//! [`ListContainerStyle`](super::list_container_style::ListContainerStyle).

use std::rc::Rc;

use bastyde_tokens::{BorderRole, SurfaceRole};

/// Focus-ring chrome: border role, stroke thickness, and inset from the tile
/// edge (all in logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridFocusRingRecipe {
    pub role: BorderRole,
    pub thickness: f32,
    pub inset: f32,
}

impl Default for GridFocusRingRecipe {
    fn default() -> Self {
        Self {
            role: BorderRole::Focused,
            thickness: 1.5,
            inset: 1.0,
        }
    }
}

/// Rubber-band marquee chrome: the accent role (used for both the
/// translucent fill and the stroke), the fill alpha, and the stroke width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridMarqueeRecipe {
    pub role: BorderRole,
    pub fill_alpha: f32,
    pub stroke_width: f32,
}

impl Default for GridMarqueeRecipe {
    fn default() -> Self {
        Self {
            role: BorderRole::Focused,
            fill_alpha: 0.18,
            stroke_width: 1.0,
        }
    }
}

/// Drag-reorder insertion-bar chrome: border role and bar thickness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridInsertionRecipe {
    pub role: BorderRole,
    pub thickness: f32,
}

impl Default for GridInsertionRecipe {
    fn default() -> Self {
        Self {
            role: BorderRole::Accent,
            thickness: 2.0,
        }
    }
}

/// Tier-3 style protocol for [`GridView`](../../bastyde_widgets/grid_view).
/// Every method has a default returning the stock recipe, so a custom style
/// only overrides the decoration it cares about.
pub trait GridViewStyle: 'static {
    /// Focus-ring chrome painted around the keyboard-focused tile.
    fn focus_ring(&self) -> GridFocusRingRecipe {
        GridFocusRingRecipe::default()
    }
    /// Rubber-band marquee rectangle chrome.
    fn marquee(&self) -> GridMarqueeRecipe {
        GridMarqueeRecipe::default()
    }
    /// Drag-reorder insertion-bar chrome.
    fn insertion(&self) -> GridInsertionRecipe {
        GridInsertionRecipe::default()
    }
    /// Opaque background surface role for the sticky pinned section header.
    fn pinned_header_surface(&self) -> SurfaceRole {
        SurfaceRole::Raised
    }
}

pub type SharedGridViewStyle = Rc<dyn GridViewStyle>;
