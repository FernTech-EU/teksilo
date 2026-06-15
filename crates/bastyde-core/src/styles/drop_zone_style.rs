// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `DropZone`. See `docs/styling-system.md`.
//!
//! Themes the external-drag "drop files here" target: the surface fill,
//! border, corner radius, and inner padding for each interaction state
//! (idle / accepting / rejecting). The `DropZone` widget owns its content
//! column (prompt, subtitle, the `Live::Polite` status line, the Browse
//! button) and its drag behaviour + accessibility; the style only paints
//! the chrome the content sits in.
//!
//! Because the chrome reacts to hover, the config carries a
//! `Signal<DropZoneVisualState>` (the same reactive pattern Button uses for
//! its interaction signal) rather than a static state — `make_body` binds
//! the surface/border colors to it so they update without a rebuild.

use std::rc::Rc;

use bastyde_tokens::{BorderRole, SurfaceRole};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Interaction state of a drop zone, driving the chrome's surface and border
/// colors. Defined here (not in `bastyde-widgets`) so the core style trait
/// and the default recipe can both name it — mirroring `BannerSeverity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZoneVisualState {
    /// At rest — no drag over the zone.
    Idle,
    /// A drag is over the zone carrying acceptable data.
    HoverAccept,
    /// A drag is over the zone but its data is rejected (wrong type / count).
    HoverReject,
}

impl DropZoneVisualState {
    /// Background surface-tint role for this state.
    pub fn surface_role(self) -> SurfaceRole {
        match self {
            Self::Idle => SurfaceRole::Sunken,
            Self::HoverAccept => SurfaceRole::AccentSubtle,
            Self::HoverReject => SurfaceRole::StatusError,
        }
    }

    /// Border role for this state.
    pub fn border_role(self) -> BorderRole {
        match self {
            Self::Idle => BorderRole::Strong,
            Self::HoverAccept => BorderRole::Accent,
            Self::HoverReject => BorderRole::Error,
        }
    }
}

/// Inputs handed to a [`DropZoneStyle`] to build the zone's chrome.
#[derive(Clone)]
pub struct DropZoneStyleConfig {
    /// Reactive interaction state — bind surface/border colors to it.
    pub state: Signal<DropZoneVisualState>,
    /// Pre-built content column (icon / prompt / subtitle / status line /
    /// Browse button) the chrome centers and pads.
    pub content: WidgetId,
}

/// Tier-3 style protocol for [`DropZone`](../../bastyde_widgets/drop_zone).
/// Produces the bordered, tinted body the content sits in.
pub trait DropZoneStyle: 'static {
    fn make_body(&self, cfg: &DropZoneStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

/// Shared, theme-installable handle to a [`DropZoneStyle`].
pub type SharedDropZoneStyle = Rc<dyn DropZoneStyle>;
