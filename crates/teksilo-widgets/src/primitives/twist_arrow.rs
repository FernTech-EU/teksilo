// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TwistArrow — a small chevron that indicates and toggles a tree node's expansion.
//!
//! Renders a right-pointing arrow when collapsed and a down-pointing arrow when
//! expanded; a leaf node (where `has_children` is false) paints nothing but
//! reserves its slot so the indent column stays aligned across all rows.
//! The glyph flips direction under right-to-left layout.
//! Accessibility-decorative: the chevron hides itself from the AT tree and
//! the parent row's node owns `set_expanded`.
//!
//! ```ignore
//! // TwistArrow is typically instantiated by TreeView row delegates and requires
//! // an EventContext to wire the tap callback. The snippet below shows the
//! // construction pattern used inside a custom tree-row build().
//! let arrow = TwistArrow::new(16.0, true, false)
//!     .on_click(|ctx| ctx.send_intent(teksilo_core::Intent::new("tree.toggle")));
//! ```

use std::rc::Rc;

use teksilo_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{SurfaceRole, TextRole};

use crate::primitives::rect_widget::RectWidget;

/// Small interactive chevron rendered in the leading indent column of a tree row.
pub struct TwistArrow {
    size: f32,
    has_children: bool,
    expanded: bool,
    on_click: Option<Rc<dyn Fn(&mut EventContext)>>,
}

impl TwistArrow {
    /// Construct a chevron. `size` is the square side length in logical pixels;
    /// `has_children` determines whether the glyph is painted; `expanded`
    /// determines the glyph direction (down = expanded, right/left = collapsed).
    pub fn new(size: f32, has_children: bool, expanded: bool) -> Self {
        Self {
            size,
            has_children,
            expanded,
            on_click: None,
        }
    }

    /// Install a tap handler. Receives the firing [`EventContext`]
    /// so consumers can dispatch intents (e.g. lazy-load children on
    /// expand) or open dialogs from the chevron toggle.
    pub fn on_click(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for TwistArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwistArrow")
            .field("size", &self.size)
            .field("has_children", &self.has_children)
            .field("expanded", &self.expanded)
            .finish()
    }
}

impl Widget for TwistArrow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(cb) = self.on_click.clone() {
            let handlers = HandlerSet::new()
                .on_tap(move |_pos, ctx| {
                    cb(ctx);
                })
                .focusable(false)
                // The chevron lives inside a reorderable tree row that owns a drag
                // recognizer. Without this, a press here arms the row's ancestor
                // drag, and the few px of jitter a real click carries (especially
                // right after a drag) crosses the drag threshold and steals the
                // gesture — the toggle never fires and a row drag starts instead.
                // A gesture dead zone stops ancestor drag-arming at this boundary,
                // exactly as the docking accordion's trailing controls do.
                .gesture_dead_zone(true);
            ctx.apply_self_handlers(handlers);
        }
        let rect = ctx.add(RectWidget::new().background(SurfaceRole::Transparent));
        vec![rect]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.size, self.size).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if !self.has_children {
            return;
        }
        let color = TextRole::Secondary.resolve(&ctx.theme.colors);
        let cx = bounds.x + bounds.width / 2.0;
        let cy = bounds.y + bounds.height / 2.0;
        let r = bounds.width.min(bounds.height) * 0.4;
        let mut path = Path::new();
        if self.expanded {
            path.move_to(Point::new(cx - r, cy - r * 0.4));
            path.line_to(Point::new(cx + r, cy - r * 0.4));
            path.line_to(Point::new(cx, cy + r * 0.6));
            path.close();
        } else if ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft {
            // Collapsed glyph points toward the leading edge — left under
            // RTL — so it mirrors the direction the subtree expands into.
            path.move_to(Point::new(cx + r * 0.4, cy - r));
            path.line_to(Point::new(cx - r * 0.6, cy));
            path.line_to(Point::new(cx + r * 0.4, cy + r));
            path.close();
        } else {
            path.move_to(Point::new(cx - r * 0.4, cy - r));
            path.line_to(Point::new(cx + r * 0.6, cy));
            path.line_to(Point::new(cx - r * 0.4, cy + r));
            path.close();
        }
        canvas.fill_path(&path, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}
