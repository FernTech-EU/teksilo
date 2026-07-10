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

use bastyde_canvas::{Point, Rect, Size};
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

/// One of the five drop regions a [`DropTarget`](../../bastyde_widgets/drop_target)
/// can expose. Each is independently enable-able and carries an optional hint.
///
/// `Leading`/`Trailing` are writing-direction-relative *by intent*; the v1
/// hit-test maps `Leading`→left and `Trailing`→right (LTR only — the framework
/// exposes no writing direction on the layout/paint contexts yet, so RTL
/// mirroring is a documented follow-up).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropRegion {
    /// The middle of the target — whatever the side zones don't claim.
    Center,
    /// The top edge strip.
    Top,
    /// The bottom edge strip.
    Bottom,
    /// The leading edge strip (left in LTR).
    Leading,
    /// The trailing edge strip (right in LTR).
    Trailing,
}

impl DropRegion {
    /// All five regions, in the hit-test priority order (side zones before
    /// centre; leading→trailing→top→bottom among the sides).
    pub const ALL: [DropRegion; 5] = [
        DropRegion::Leading,
        DropRegion::Trailing,
        DropRegion::Top,
        DropRegion::Bottom,
        DropRegion::Center,
    ];

    /// True for the four side (edge) zones — the ones sized by `size_factor`.
    /// `Center` is the leftover middle and is never a side zone.
    pub fn is_side(self) -> bool {
        !matches!(self, DropRegion::Center)
    }
}

/// Which [`DropRegion`]s a [`DropTarget`](../../bastyde_widgets/drop_target)
/// currently exposes. The default (`Center` only) reproduces the classic
/// whole-bounds single-zone behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropRegionSet {
    /// Whether the centre region is enabled.
    pub center: bool,
    /// Whether the top edge region is enabled.
    pub top: bool,
    /// Whether the bottom edge region is enabled.
    pub bottom: bool,
    /// Whether the leading edge region is enabled.
    pub leading: bool,
    /// Whether the trailing edge region is enabled.
    pub trailing: bool,
}

impl Default for DropRegionSet {
    /// Centre only — the whole-bounds single-zone default.
    fn default() -> Self {
        Self {
            center: true,
            top: false,
            bottom: false,
            leading: false,
            trailing: false,
        }
    }
}

impl DropRegionSet {
    /// An empty set — no region enabled. Enable regions with [`Self::with`].
    pub fn none() -> Self {
        Self {
            center: false,
            top: false,
            bottom: false,
            leading: false,
            trailing: false,
        }
    }

    /// Is `region` enabled in this set?
    pub fn contains(self, region: DropRegion) -> bool {
        match region {
            DropRegion::Center => self.center,
            DropRegion::Top => self.top,
            DropRegion::Bottom => self.bottom,
            DropRegion::Leading => self.leading,
            DropRegion::Trailing => self.trailing,
        }
    }

    /// A copy with `region` enabled.
    pub fn with(mut self, region: DropRegion) -> Self {
        match region {
            DropRegion::Center => self.center = true,
            DropRegion::Top => self.top = true,
            DropRegion::Bottom => self.bottom = true,
            DropRegion::Leading => self.leading = true,
            DropRegion::Trailing => self.trailing = true,
        }
        self
    }

    /// The enabled regions, in [`DropRegion::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = DropRegion> {
        DropRegion::ALL
            .into_iter()
            .filter(move |r| self.contains(*r))
    }
}

/// Clamp a caller-supplied side-zone size factor to the supported `0.1..=1.0`
/// range. The factor is the fraction of the relevant axis each **side** zone
/// occupies (e.g. `0.5` bisects; the docking default was `0.2`).
pub fn clamp_size_factor(factor: f32) -> f32 {
    factor.clamp(0.1, 1.0)
}

/// Classify a pointer in widget-local coordinates into an enabled [`DropRegion`],
/// or `None` when it lands in a middle with no `Center` enabled.
///
/// Generalizes docking's `compute_drop_zone`: each side zone is a
/// `size_factor`-thick strip along its edge (no fixed pixel cap — the factor is
/// the caller's knob). Only **enabled** edges are tested, in
/// leading→trailing→top→bottom priority (so with a large factor an overlapping
/// leading strip wins over trailing); the remaining middle resolves to `Center`
/// when enabled, else `None`.
pub fn region_at(
    local: Point,
    size: Size,
    set: DropRegionSet,
    size_factor: f32,
) -> Option<DropRegion> {
    let f = clamp_size_factor(size_factor);
    let ex = size.width * f;
    let ey = size.height * f;
    if set.leading && local.x < ex {
        Some(DropRegion::Leading)
    } else if set.trailing && local.x > size.width - ex {
        Some(DropRegion::Trailing)
    } else if set.top && local.y < ey {
        Some(DropRegion::Top)
    } else if set.bottom && local.y > size.height - ey {
        Some(DropRegion::Bottom)
    } else if set.center {
        Some(DropRegion::Center)
    } else {
        None
    }
}

/// The highlight / hint rectangle for `region` within `bounds`: `Center` is the
/// whole rectangle; a side zone is the `size_factor`-thick strip along its edge
/// (visual == hit region — the zone the user sees is the zone that drops).
///
/// Note: a `Top`/`Bottom` strip spans the **full width** (and `Leading`/`Trailing`
/// the full height). When both a horizontal and a vertical edge are enabled, the
/// corner cells of a `Top`/`Bottom` rect visually overlap the area that
/// [`region_at`] actually classifies as `Leading`/`Trailing` (which win the
/// priority tie). This matches the docking overlay's half-rect precedent and is
/// harmless — only one region is ever highlighted at a time — but the painted rect
/// is a superset of that region's true hit area at the corners.
pub fn region_rect(region: DropRegion, bounds: Rect, size_factor: f32) -> Rect {
    let f = clamp_size_factor(size_factor);
    let ex = bounds.width * f;
    let ey = bounds.height * f;
    match region {
        DropRegion::Center => bounds,
        DropRegion::Leading => Rect::new(bounds.x, bounds.y, ex, bounds.height),
        DropRegion::Trailing => {
            Rect::new(bounds.x + bounds.width - ex, bounds.y, ex, bounds.height)
        }
        DropRegion::Top => Rect::new(bounds.x, bounds.y, bounds.width, ey),
        DropRegion::Bottom => Rect::new(bounds.x, bounds.y + bounds.height - ey, bounds.width, ey),
    }
}

/// Inputs handed to a [`DropTargetStyle`] to build the wrapping chrome.
#[derive(Clone)]
pub struct DropTargetStyleConfig {
    /// The user's child widget — fills the full bounds and is always visible.
    pub content_id: WidgetId,
    /// Reactive overall interaction state (idle / accepting / rejecting) —
    /// bind overlay border colors and the reject tint to it.
    pub drag_state: Signal<DropTargetDragState>,
    /// Which region the pointer is currently over while an accepted payload
    /// hovers (`None` when idle, rejecting, or over a disabled middle). Drives
    /// the per-zone highlight and which hint is shown.
    pub active_region: Signal<Option<DropRegion>>,
    /// Which regions this target exposes. `Center`-only is the classic
    /// whole-bounds single-zone case.
    pub regions: DropRegionSet,
    /// Pre-built per-region hint content (user slots), each centered inside a
    /// popup card within its region's rect while that region is the active
    /// accepted-hover. Empty when no hints were set.
    pub region_hints: Vec<(DropRegion, WidgetId)>,
    /// Side-zone size factor (already clamped to `0.1..=1.0`): the fraction of
    /// the axis each edge zone occupies.
    pub size_factor: f32,
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

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: DropRegionSet = DropRegionSet {
        center: true,
        top: true,
        bottom: true,
        leading: true,
        trailing: true,
    };

    #[test]
    fn center_only_classifies_everything_as_center() {
        let set = DropRegionSet::default();
        let size = Size::new(400.0, 300.0);
        for &(x, y) in &[(0.0, 0.0), (200.0, 150.0), (399.0, 299.0)] {
            assert_eq!(
                region_at(Point::new(x, y), size, set, 0.2),
                Some(DropRegion::Center)
            );
        }
    }

    #[test]
    fn full_five_zone_edges_and_center() {
        let size = Size::new(400.0, 300.0);
        // 20% strips: ex = 80, ey = 60.
        assert_eq!(
            region_at(Point::new(5.0, 150.0), size, FULL, 0.2),
            Some(DropRegion::Leading)
        );
        assert_eq!(
            region_at(Point::new(395.0, 150.0), size, FULL, 0.2),
            Some(DropRegion::Trailing)
        );
        assert_eq!(
            region_at(Point::new(200.0, 5.0), size, FULL, 0.2),
            Some(DropRegion::Top)
        );
        assert_eq!(
            region_at(Point::new(200.0, 295.0), size, FULL, 0.2),
            Some(DropRegion::Bottom)
        );
        assert_eq!(
            region_at(Point::new(200.0, 150.0), size, FULL, 0.2),
            Some(DropRegion::Center)
        );
    }

    #[test]
    fn only_enabled_edges_are_tested() {
        // Leading + trailing only, no center: the middle is `None` (rejected).
        let set = DropRegionSet::none()
            .with(DropRegion::Leading)
            .with(DropRegion::Trailing);
        let size = Size::new(400.0, 300.0);
        assert_eq!(
            region_at(Point::new(5.0, 150.0), size, set, 0.2),
            Some(DropRegion::Leading)
        );
        assert_eq!(
            region_at(Point::new(200.0, 150.0), size, set, 0.2),
            None,
            "middle with no Center enabled must reject"
        );
        // A point in the top strip is NOT top (top disabled) → falls to middle → None.
        assert_eq!(region_at(Point::new(200.0, 5.0), size, set, 0.2), None);
    }

    #[test]
    fn factor_half_bisects_left_right() {
        let set = DropRegionSet::none()
            .with(DropRegion::Leading)
            .with(DropRegion::Trailing);
        let size = Size::new(400.0, 300.0);
        // ex = 200: x<200 → leading, x>200 → trailing.
        assert_eq!(
            region_at(Point::new(199.0, 150.0), size, set, 0.5),
            Some(DropRegion::Leading)
        );
        assert_eq!(
            region_at(Point::new(201.0, 150.0), size, set, 0.5),
            Some(DropRegion::Trailing)
        );
    }

    #[test]
    fn leading_wins_overlap_with_large_factor() {
        // factor 0.9 overlaps leading and trailing across most of the width;
        // priority order (leading first) resolves the overlap.
        let size = Size::new(400.0, 300.0);
        assert_eq!(
            region_at(Point::new(200.0, 150.0), size, FULL, 0.9),
            Some(DropRegion::Leading)
        );
    }

    #[test]
    fn size_factor_is_clamped() {
        assert_eq!(clamp_size_factor(0.05), 0.1);
        assert_eq!(clamp_size_factor(2.0), 1.0);
        assert_eq!(clamp_size_factor(0.3), 0.3);
    }

    #[test]
    fn region_rect_strips() {
        let b = Rect::new(10.0, 20.0, 400.0, 300.0);
        assert_eq!(region_rect(DropRegion::Center, b, 0.25), b);
        // ex = 100, ey = 75.
        let lead = region_rect(DropRegion::Leading, b, 0.25);
        assert_eq!(
            (lead.x, lead.y, lead.width, lead.height),
            (10.0, 20.0, 100.0, 300.0)
        );
        let trail = region_rect(DropRegion::Trailing, b, 0.25);
        assert_eq!(
            (trail.x, trail.y, trail.width, trail.height),
            (310.0, 20.0, 100.0, 300.0)
        );
        let top = region_rect(DropRegion::Top, b, 0.25);
        assert_eq!(
            (top.x, top.y, top.width, top.height),
            (10.0, 20.0, 400.0, 75.0)
        );
        let bottom = region_rect(DropRegion::Bottom, b, 0.25);
        assert_eq!(
            (bottom.x, bottom.y, bottom.width, bottom.height),
            (10.0, 245.0, 400.0, 75.0)
        );
    }
}
