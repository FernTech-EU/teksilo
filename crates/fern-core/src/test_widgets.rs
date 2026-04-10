//! Minimal test widgets for fern-core's headless tests.
//! These live in fern-core (not fern-widgets) to avoid a circular dependency.
//! They exercise the Widget trait without bringing in real rendering.

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use fern_tokens::{Color, CornerRadius};

use crate::accessibility::AccessNodeBuilder;
use crate::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use crate::widget_id::WidgetId;

/// A minimal leaf widget for testing. Fills proposed size, optionally paints a shape.
#[derive(Debug)]
pub struct FillWidget {
    background: Option<Color>,
    corner_radius: CornerRadius,
    label: Option<String>,
    focusable: bool,
}

impl FillWidget {
    pub fn new() -> Self {
        Self {
            background: None,
            corner_radius: CornerRadius::ZERO,
            label: None,
            focusable: false,
        }
    }

    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    pub fn corner_radius(mut self, r: CornerRadius) -> Self {
        self.corner_radius = r;
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }
}

impl Default for FillWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for FillWidget {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        if let Some(bg) = self.background {
            canvas.fill_rounded_rect(bounds, self.corner_radius, bg);
        }
    }

    fn build(
        &mut self,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        if self.focusable {
            ctx.apply_self_handlers(crate::widget_builder::HandlerSet::new().focusable(true));
        }
        Vec::new()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        if let Some(label) = &self.label {
            builder.set_role(accesskit::Role::Label);
            builder.set_name(label.as_str());
        }
    }
}

/// A minimal layout container that stacks children at the same origin.
#[derive(Debug)]
pub struct StackWidget {
    child_ids: Vec<WidgetId>,
}

impl StackWidget {
    pub fn new() -> Self {
        Self {
            child_ids: Vec::new(),
        }
    }

    pub fn add_child(mut self, id: WidgetId) -> Self {
        self.child_ids.push(id);
        self
    }
}

impl Widget for StackWidget {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
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

    fn children(&self) -> Vec<WidgetId> {
        self.child_ids.clone()
    }
}

/// A minimal inset container for testing padding/layout.
#[derive(Debug)]
pub struct InsetWidget {
    inset: f32,
    child_id: Option<WidgetId>,
}

impl InsetWidget {
    pub fn new(inset: f32) -> Self {
        Self {
            inset,
            child_id: None,
        }
    }

    pub fn set_child(mut self, id: WidgetId) -> Self {
        self.child_id = Some(id);
        self
    }
}

impl Widget for InsetWidget {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        let total = self.inset * 2.0;
        let size = proposal.resolve(total, total);
        Size::new(size.width.max(total), size.height.max(total))
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + self.inset, bounds.y + self.inset);
            child.size = Size::new(
                (bounds.width - self.inset * 2.0).max(0.0),
                (bounds.height - self.inset * 2.0).max(0.0),
            );
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}
