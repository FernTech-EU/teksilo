//! Small interactive chevron that toggles a tree node's expansion.
//!
//! Renders a right-pointing arrow when collapsed, down-pointing when
//! expanded; a leaf node renders nothing (just empty space) so the
//! indent column lines up. Accessibility-decorative: the parent row's
//! AT node owns `set_expanded`.

use std::rc::Rc;

use fern_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{SurfaceRole, TextRole};

use crate::primitives::rect_widget::RectWidget;

pub struct TwistArrow {
    size: f32,
    has_children: bool,
    expanded: bool,
    on_click: Option<Rc<dyn Fn()>>,
}

impl TwistArrow {
    pub fn new(size: f32, has_children: bool, expanded: bool) -> Self {
        Self {
            size,
            has_children,
            expanded,
            on_click: None,
        }
    }

    pub fn on_click(mut self, f: impl Fn() + 'static) -> Self {
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
                .on_tap(move |_pos, _ctx| {
                    cb();
                })
                .focusable(false);
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
