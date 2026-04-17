//! Flexible drag region inside a `TitleBar`.
//!
//! Captures pointer events that are not consumed by inner content and
//! forwards them to the platform host: drag gestures begin a window move,
//! double taps toggle maximize, and right clicks open the system window
//! menu (Wayland only). On Windows the same hit region is also published
//! into [`HitRegions::drag`] from `paint()` so the wndproc subclass can
//! return `HTCAPTION` for the same area.

use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::gesture::DragPhase;
use fern_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_core::{HitRegions, PlatformTitleBarHost};

pub struct DragRegion {
    host: Rc<dyn PlatformTitleBarHost>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
}

impl std::fmt::Debug for DragRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragRegion")
            .field("has_child", &self.pending_child.is_some())
            .finish_non_exhaustive()
    }
}

impl DragRegion {
    pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self {
        Self {
            host,
            pending_child: None,
            child_id: None,
        }
    }

    pub fn with_child(host: Rc<dyn PlatformTitleBarHost>, child: Box<dyn Widget>) -> Self {
        Self {
            host,
            pending_child: Some(PendingChild::Deferred(child)),
            child_id: None,
        }
    }

    pub fn with_child_id(host: Rc<dyn PlatformTitleBarHost>, id: WidgetId) -> Self {
        Self {
            host,
            pending_child: Some(PendingChild::Id(id)),
            child_id: None,
        }
    }
}

impl Widget for DragRegion {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }

        // Drag gesture: begin a window move as soon as the OS recognizes
        // movement during a primary-button press. Using `on_drag` (rather
        // than `on_pointer_event` on PointerDown) means a quick click
        // without movement still flows to the double-tap recognizer, which
        // is how we get double-click-to-maximize.
        let host_drag = self.host.clone();
        let host_dbl = self.host.clone();
        let host_pointer = self.host.clone();

        let handlers = HandlerSet::new()
            .on_drag(move |phase, _ctx| {
                if let DragPhase::Started {
                    button: PointerButton::Primary,
                    ..
                } = phase
                {
                    let _ = host_drag.begin_drag();
                }
            })
            .on_double_tap(move |_pos, _ctx| {
                host_dbl.toggle_maximize();
            })
            .on_pointer_event(move |evt, _ctx| {
                if let WidgetEvent::PointerDown {
                    button: PointerButton::Secondary,
                    position,
                    ..
                } = evt
                {
                    let _ = host_pointer.show_window_menu(*position);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });

        ctx.apply_self_handlers(handlers);

        self.child_id.into_iter().collect()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // Spacer-style: report zero intrinsic width so the parent HStack
        // gives us all the leftover horizontal space via `place_children`
        // (`HStack::place_children` allocates `(bounds.width - non_spacer)
        // / spacer_count` to each child returning `is_spacer == true`).
        // Height comes from the proposal so we match the title bar's
        // configured height even when the child reports zero.
        Size::new(0.0, proposal.height.unwrap_or(0.0))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Inner content (the optional `center` widget) fills the drag
        // region's full bounds.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, _canvas: &mut fern_canvas::Canvas, _ctx: &PaintContext) {
        // Publish our physical-pixel rectangle into the host's hit-region
        // table. The Windows backend reads this from `WM_NCHITTEST` to
        // return `HTCAPTION` for the same area; other backends are no-ops.
        let mut regions = HitRegions::new();
        regions.drag.push(bounds);
        self.host.update_hit_regions(&regions);
    }

    fn is_spacer(&self) -> bool {
        // Tells the parent `HStack` to stretch us across all leftover
        // horizontal space. Critical: without this override the drag
        // region collapses to zero width and there is nothing to drag.
        true
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
