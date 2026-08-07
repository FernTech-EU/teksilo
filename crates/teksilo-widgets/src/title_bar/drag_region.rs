// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DragRegion` — flexible drag region inside a `TitleBar`.
//!
//! Captures pointer events that are not consumed by inner content and
//! forwards them to the platform host: drag gestures begin a window move,
//! double taps toggle maximize, and right-clicks open the system window
//! menu (Wayland only). On Windows the drag rect is published into
//! `HitRegions::drag` so the wndproc subclass returns `HTCAPTION` for
//! the same area — but the actual publish happens from
//! [`crate::title_bar::TitleBar::after_paint`], which aggregates this
//! drag region and the three control buttons into one snapshot per
//! frame. This widget no longer publishes from `paint()`.
//!
//! The region grows via `flex = 1.0` to claim all remaining horizontal
//! space in the parent `HStack`, so it naturally sits between any leading
//! widgets (app icon, document title) and the trailing `WindowControls`
//! cluster. An optional child widget — typically a centered title — is
//! placed at the full region bounds and passes pointer events upward to
//! the drag handler when it does not consume them.
//!
//! ```ignore
//! // Used internally by TitleBar; the snippet shows the construction pattern.
//! let region = DragRegion::with_child(host.clone(), TextWidget::new(lit!("My App")));
//! ```

use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::PlatformTitleBarHost;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::gesture::DragPhase;
use teksilo_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

/// Flexible, hit-transparent region inside a title bar that routes pointer events to the
/// platform host for window dragging, maximize-toggle, and the system window menu.
pub struct DragRegion {
    host: Rc<dyn PlatformTitleBarHost>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Forwarded from [`TitleBar::close_action`](crate::TitleBar::close_action)
    /// so the fallback menu's Close entry does exactly what the close *button*
    /// does. Unused when the platform has its own window menu.
    close_action: Option<Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
}

impl std::fmt::Debug for DragRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragRegion")
            .field("has_child", &self.pending_child.is_some())
            .finish_non_exhaustive()
    }
}

impl DragRegion {
    /// Create a drag region with no inner content — the entire region is a pure drag handle.
    pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self {
        Self {
            host,
            pending_child: None,
            child_id: None,
            close_action: None,
        }
    }

    /// Create a drag region wrapping an arbitrary boxed child widget (typically a centered
    /// title). Pointer events not consumed by the child bubble up to the drag handler.
    pub fn with_child(host: Rc<dyn PlatformTitleBarHost>, child: Box<dyn Widget>) -> Self {
        Self {
            host,
            pending_child: Some(PendingChild::Deferred(child)),
            child_id: None,
            close_action: None,
        }
    }

    /// Create a drag region with an already-registered child identified by `id`.
    /// Use this when the child widget was added to the tree before constructing the
    /// region (e.g. when you need the child's `WidgetId` for another reference).
    pub fn with_child_id(host: Rc<dyn PlatformTitleBarHost>, id: WidgetId) -> Self {
        Self {
            host,
            pending_child: Some(PendingChild::Id(id)),
            child_id: None,
            close_action: None,
        }
    }

    /// Forward the title bar's close-action override, so the fallback window
    /// menu's Close entry matches the close button. No effect on platforms
    /// that provide their own window menu.
    pub fn close_action(
        mut self,
        action: Option<Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
    ) -> Self {
        self.close_action = action;
        self
    }
}

impl Widget for DragRegion {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
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
        let host_pointer = self.host.clone();

        // Right-click opens the window menu. Where the OS provides one we ask
        // for it; where it does not (X11 — see `window_menu`), we build our
        // own via the framework's context-menu factory, which handles the
        // at-pointer overlay, dismissal, and focus for us.
        let has_os_window_menu = self.host.has_window_menu();
        let close_action = self.close_action.clone();

        let mut handlers = HandlerSet::new()
            .on_drag(move |phase, _ctx| {
                if let DragPhase::Started {
                    button: PointerButton::Primary,
                    ..
                } = phase
                {
                    let _ = host_drag.begin_drag();
                }
            })
            .on_double_tap(move |_pos, ctx| {
                if let Some(w) = ctx.window() {
                    let next = if w.placement().get().is_maximized() {
                        teksilo_core::WindowPlacement::Floating
                    } else {
                        teksilo_core::WindowPlacement::Maximized
                    };
                    w.placement().set(next);
                }
            })
            .on_pointer_event(move |evt, _ctx| {
                if !has_os_window_menu {
                    // The context-menu factory below owns the secondary
                    // button; consuming it here would suppress the menu.
                    return EventResponse::Ignored;
                }
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

        if !has_os_window_menu {
            handlers = handlers.context_menu(move |_at, ctx| {
                super::window_menu::build_window_menu(ctx, close_action.clone())
            });
        }

        ctx.apply_self_handlers(handlers);

        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Wanted width is 0 — we want pure slack from the parent HStack.
        // Height matches the title bar's configured height so we paint
        // through even when the inner child reports zero.
        // `flex = 1.0` claims the leftover horizontal space; without it the
        // drag region collapses and there is nothing to drag.
        teksilo_core::widget::LayoutResponse::flexible(
            Size::new(0.0, proposal.height.unwrap_or(0.0)),
            1.0,
        )
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

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        // No paint — our parent `TitleBar::after_paint` reads our
        // bounds and publishes them as part of the aggregated
        // `HitRegions` snapshot.
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Pointer-only affordance — no keyboard or AT analogue for
        // "drag the window by its title". Hide the node so it doesn't
        // show up as an unnamed Unknown stop between the title bar
        // landmark and its real content.
        builder.set_hidden();
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
