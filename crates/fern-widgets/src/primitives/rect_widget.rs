use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_tokens::{Color, CornerRadius};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, Widget};

/// A leaf widget that paints a filled and/or stroked rounded rectangle.
/// Properties can be static values or bound to reactive state.
pub struct RectWidget {
    background: Prop<Color>,
    border_color: Prop<Color>,
    border_width: Prop<f32>,
    corner_radius: Prop<CornerRadius>,
}

impl RectWidget {
    pub fn new() -> Self {
        Self {
            background: Prop::Static(Color::TRANSPARENT),
            border_color: Prop::Static(Color::TRANSPARENT),
            border_width: Prop::Static(0.0),
            corner_radius: Prop::Static(CornerRadius::ZERO),
        }
    }

    pub fn background(mut self, color: Color) -> Self {
        self.background = Prop::Static(color);
        self
    }

    pub fn border_color(mut self, color: Color) -> Self {
        self.border_color = Prop::Static(color);
        self
    }

    pub fn border_width(mut self, width: f32) -> Self {
        self.border_width = Prop::Static(width);
        self
    }

    pub fn corner_radius(mut self, radius: CornerRadius) -> Self {
        self.corner_radius = Prop::Static(radius);
        self
    }

    /// Bind the background color to a reactive state.
    pub fn bind_background(mut self, state: impl Into<Prop<Color>>) -> Self {
        self.background = state.into();
        self
    }

    /// Bind the border color to a reactive state.
    pub fn bind_border_color(mut self, state: impl Into<Prop<Color>>) -> Self {
        self.border_color = state.into();
        self
    }

    /// Bind the border width to a reactive state.
    pub fn bind_border_width(mut self, state: impl Into<Prop<f32>>) -> Self {
        self.border_width = state.into();
        self
    }

    /// Bind the corner radius to a reactive state.
    pub fn bind_corner_radius(mut self, state: impl Into<Prop<CornerRadius>>) -> Self {
        self.corner_radius = state.into();
        self
    }
}

impl Default for RectWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RectWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RectWidget").finish()
    }
}

impl Widget for RectWidget {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.background.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_color.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_width.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        self.corner_radius.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        // RectWidget has no intrinsic content — it accepts whatever space is offered.
        // With an exact proposal it fills the space; with unspecified it reports 0x0.
        proposal.resolve(0.0, 0.0)
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let bg = self.background.get();
        let radius = self.corner_radius.get();
        if bg.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, radius, bg);
        }
        let bw = self.border_width.get();
        let bc = self.border_color.get();
        if bw > 0.0 && bc.a() > 0.0 {
            canvas.stroke_rounded_rect(bounds, radius, bc, bw);
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::signal::Signal;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn static_background_paints_correctly() {
        let mut tree = WidgetTree::new();
        tree.add(
            RectWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());
    }

    #[test]
    fn bind_background_reads_from_state() {
        let color = Signal::new(Color::BLUE);
        let mut tree = WidgetTree::new();
        let w = tree.add(
            RectWidget::new()
                .bind_background(color.clone())
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        color.bind_to(
            w,
            tree.binding_registry(),
            fern_core::binding::BindingLevel::RepaintOnly,
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::BLUE.to_array());
    }

    #[test]
    fn bind_background_updates_on_state_change() {
        let color = Signal::new(Color::RED);
        let mut tree = WidgetTree::new();
        let w = tree.add(
            RectWidget::new()
                .bind_background(color.clone())
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        color.bind_to(
            w,
            tree.binding_registry(),
            fern_core::binding::BindingLevel::RepaintOnly,
        );

        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());

        // Change the state
        color.set(Color::GREEN);
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::GREEN.to_array());
    }
}
