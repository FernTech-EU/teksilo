// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `DropTarget`. See `docs/styling-system.md`.
//!
//! `DropTarget` is the *wrapping* counterpart to [`DropZone`]: it turns any
//! existing widget subtree into a drop target without replacing its visual
//! identity. The wrapped child fills the bounds and is always visible; the
//! style adds a reactive border + tint overlay that tracks the drag state
//! (idle / accepting / rejecting) and, when a hint slot is set, a centered
//! popup card.
//!
//! Like [`DropZoneStyle`](crate::styles::DropZoneStyle), the chrome reacts to
//! hover, so the config carries a `Signal<DropTargetDragState>` — `make_body`
//! binds the overlay's surface/border colors to it so they update without a
//! rebuild.
//!
//! [`DropZone`]: ../../bastyde_widgets/drop_zone

use std::rc::Rc;

use bastyde_tokens::{BorderRole, SurfaceRole};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Interaction state of a drop target, driving the overlay's surface and
/// border colors and the hint card's visibility. Defined here (not in
/// `bastyde-widgets`) so the core style trait and the default recipe can both
/// name it — mirroring [`DropZoneVisualState`](crate::styles::DropZoneVisualState).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetDragState {
    /// At rest — no drag over the target. Overlay is fully transparent so the
    /// wrapped child shows through untouched.
    Idle,
    /// A drag is over the target carrying acceptable data.
    HoverAccept,
    /// A drag is over the target but its data is rejected by the accept filter.
    HoverReject,
}

impl DropTargetDragState {
    /// Background surface-tint role for this state.
    pub fn surface_role(self) -> SurfaceRole {
        match self {
            Self::Idle => SurfaceRole::Transparent,
            Self::HoverAccept => SurfaceRole::AccentSubtle,
            Self::HoverReject => SurfaceRole::StatusError,
        }
    }

    /// Border role for this state.
    pub fn border_role(self) -> BorderRole {
        match self {
            Self::Idle => BorderRole::Transparent,
            Self::HoverAccept => BorderRole::Accent,
            Self::HoverReject => BorderRole::Error,
        }
    }
}

/// Visual prominence of the drop target's hover indicator.
///
/// The default recipe draws the highlight as a **border only** (a solid stroke
/// over the child) so the wrapped content is never hidden — an opaque surface
/// tint would cover it. A translucent wash, dashed border, or glow requires a
/// custom [`DropTargetStyle`]; the [`DropTargetDragState::surface_role`] helper
/// is provided for styles that want a fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropTargetVariant {
    /// 2 px solid role-colored highlight border. Default.
    #[default]
    Default,
    /// 3 px solid border — visually heavier, for primary drop zones.
    Prominent,
    /// 1 px thin border. Minimal visual footprint.
    Subtle,
    /// No built-in feedback; the style returns only the user's child (and the
    /// hint, if any). For fully custom visuals driven from a bound signal.
    None,
}

/// Inputs handed to a [`DropTargetStyle`] to build the wrapping chrome.
#[derive(Clone)]
pub struct DropTargetStyleConfig {
    /// The user's child widget — fills the full bounds and is always visible.
    pub content_id: WidgetId,
    /// Pre-built hint content (user slot), centered inside a popup card while
    /// hovering with an accepted payload. `None` if no hint was set.
    pub hint_id: Option<WidgetId>,
    /// Reactive interaction state — bind overlay surface/border colors and
    /// hint visibility to it.
    pub drag_state: Signal<DropTargetDragState>,
    /// Visual prominence requested by the caller.
    pub variant: DropTargetVariant,
}

/// Tier-3 style protocol for [`DropTarget`](../../bastyde_widgets/drop_target).
/// Produces the body: the wrapped child plus the reactive overlay and the
/// optional centered hint.
pub trait DropTargetStyle: 'static {
    fn make_body(&self, cfg: &DropTargetStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

/// Shared, theme-installable handle to a [`DropTargetStyle`].
pub type SharedDropTargetStyle = Rc<dyn DropTargetStyle>;
