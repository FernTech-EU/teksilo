// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Drag-and-drop plumbing for [`DockingLayout`](super::DockingLayout):
//! the two typed payloads, the five-zone hit-test, and the signal-driven
//! drop-zone overlay. The actual mutations are routed by the panel / tab
//! layer through [`DockingModel`](super::DockingModel).

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::DragPayload;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{BorderRole, SurfaceRole};

use crate::tab_widget::{TabBarDragData, TabHandle};

use super::model::{DockTabId, DockWidgetId};

/// Payload for dragging a single dock widget (promote / split / stack / move).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockDragData {
    pub dock_id: DockWidgetId,
}

/// Payload for dragging a whole tab (its arrangement + every dock) to another
/// side.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DockTabDragData {
    pub tab_id: DockTabId,
    pub source_side: super::geometry::DockSide,
}

/// Extract a dock **tab** id from a drag payload — recognising BOTH dock tab
/// drag sources: an activity-rail item (`DockTabDragData`) and a tab dragged
/// from the in-strip `TabWidget` (`TabBarDragData<TabHandle>`, whose
/// `source_id` the dock builds straight from the `DockTabId`). Every dock drop
/// site uses this so a tab drops the same way wherever it lands.
pub(crate) fn dropped_dock_tab(p: &DragPayload) -> Option<DockTabId> {
    if let Some(d) = p.get_typed::<DockTabDragData>() {
        return Some(d.tab_id);
    }
    if let Some(tbd) = p.get_typed::<TabBarDragData<TabHandle>>() {
        return Some(DockTabId::from_raw(tbd.source_id.raw().get()));
    }
    None
}

/// Extract a single **dock widget** id from a drag payload (a split-pane
/// header drag).
pub(crate) fn dropped_dock_widget(p: &DragPayload) -> Option<DockWidgetId> {
    p.get_typed::<DockDragData>().map(|d| d.dock_id)
}

/// Where within a target a drop landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropZone {
    /// Centre — stack into / add to the pane.
    Center,
    SplitLeading,
    SplitTrailing,
    SplitTop,
    SplitBottom,
}

impl DropZone {
    pub(crate) fn is_split(self) -> bool {
        !matches!(self, DropZone::Center)
    }
}

/// Five-zone hit-test: an edge fifth (capped at 48 px so the centre stays
/// reachable on small panes) snaps to a split, otherwise centre.
pub(crate) fn compute_drop_zone(local: Point, size: Size) -> DropZone {
    let ex = (size.width * 0.2).min(48.0);
    let ey = (size.height * 0.2).min(48.0);
    if local.x < ex {
        DropZone::SplitLeading
    } else if local.x > size.width - ex {
        DropZone::SplitTrailing
    } else if local.y < ey {
        DropZone::SplitTop
    } else if local.y > size.height - ey {
        DropZone::SplitBottom
    } else {
        DropZone::Center
    }
}

/// A translucent overlay highlighting the active drop zone. A ZStack child of
/// a pane; stretched to the pane bounds (like a focus rect). Paints nothing
/// when its `zone` signal is `None`.
pub(crate) struct DockDropOverlay {
    zone: Signal<Option<DropZone>>,
    fill: ColorProp,
    border: ColorProp,
}

impl std::fmt::Debug for DockDropOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockDropOverlay").finish()
    }
}

impl DockDropOverlay {
    pub(crate) fn new(zone: Signal<Option<DropZone>>) -> Self {
        Self {
            zone,
            fill: ColorProp::from(SurfaceRole::Accent),
            border: ColorProp::from(BorderRole::Accent),
        }
    }
}

impl Widget for DockDropOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.zone
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::RepaintOnly);
        // Pure decoration: let pointer events pass straight through to the
        // content below, so the panel stays interactive even though the
        // overlay sits on top of it in the ZStack.
        ctx.apply_self_handlers(
            bastyde_core::widget_builder::HandlerSet::new().event_pass_through(true),
        );
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(zone) = self.zone.get() else {
            return;
        };
        let rect = match zone {
            DropZone::Center => bounds,
            DropZone::SplitLeading => {
                Rect::new(bounds.x, bounds.y, bounds.width * 0.5, bounds.height)
            }
            DropZone::SplitTrailing => Rect::new(
                bounds.x + bounds.width * 0.5,
                bounds.y,
                bounds.width * 0.5,
                bounds.height,
            ),
            DropZone::SplitTop => Rect::new(bounds.x, bounds.y, bounds.width, bounds.height * 0.5),
            DropZone::SplitBottom => Rect::new(
                bounds.x,
                bounds.y + bounds.height * 0.5,
                bounds.width,
                bounds.height * 0.5,
            ),
        };
        let fill = self.fill.resolve(ctx.theme, true).with_alpha(0.22);
        canvas.fill_rect(rect, fill);
        let border = self.border.resolve(ctx.theme, true);
        // Thin accent frame around the highlighted region.
        let t = 2.0;
        canvas.fill_rect(Rect::new(rect.x, rect.y, rect.width, t), border);
        canvas.fill_rect(
            Rect::new(rect.x, rect.y + rect.height - t, rect.width, t),
            border,
        );
        canvas.fill_rect(Rect::new(rect.x, rect.y, t, rect.height), border);
        canvas.fill_rect(
            Rect::new(rect.x + rect.width - t, rect.y, t, rect.height),
            border,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Decorative; hide from AT.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_zone_for_middle() {
        let z = compute_drop_zone(Point::new(200.0, 150.0), Size::new(400.0, 300.0));
        assert_eq!(z, DropZone::Center);
    }

    #[test]
    fn edges_snap_to_splits() {
        let s = Size::new(400.0, 300.0);
        assert_eq!(compute_drop_zone(Point::new(5.0, 150.0), s), DropZone::SplitLeading);
        assert_eq!(
            compute_drop_zone(Point::new(395.0, 150.0), s),
            DropZone::SplitTrailing
        );
        assert_eq!(compute_drop_zone(Point::new(200.0, 5.0), s), DropZone::SplitTop);
        assert_eq!(
            compute_drop_zone(Point::new(200.0, 295.0), s),
            DropZone::SplitBottom
        );
    }

    #[test]
    fn edge_threshold_capped_keeps_center_reachable_on_small_pane() {
        // A 500-px-wide pane: 20% = 100, but the cap is 48, so x=60 is centre.
        let s = Size::new(500.0, 80.0);
        assert_eq!(compute_drop_zone(Point::new(60.0, 40.0), s), DropZone::Center);
        assert_eq!(compute_drop_zone(Point::new(20.0, 40.0), s), DropZone::SplitLeading);
    }
}
