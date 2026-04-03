//! Divider — a themed separator line.
//!
//! A thin line that separates content visually. Horizontal by default.
//! Color defaults to `theme.colors.border`.

use fern_canvas::{Canvas, Point, Rect, Size, SizeProposal, StrokeStyle};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::state::State;
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_tokens::{Color, Orientation};

/// A themed 1px separator line.
#[derive(Debug)]
pub struct Divider {
    orientation: Orientation,
    thickness: f32,
    color: Option<Color>,
    visible_when_state: Option<State<bool>>,
    enabled_when_state: Option<State<bool>>,
}

impl Divider {
    pub fn new() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            thickness: 1.0,
            color: None,
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn horizontal() -> Self {
        Self::new()
    }

    pub fn vertical() -> Self {
        Self {
            orientation: Orientation::Vertical,
            ..Self::new()
        }
    }

    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn visible_when(mut self, state: State<bool>) -> Self {
        self.visible_when_state = Some(state);
        self
    }

    pub fn enabled_when(mut self, state: State<bool>) -> Self {
        self.enabled_when_state = Some(state);
        self
    }
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Divider {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        match self.orientation {
            Orientation::Horizontal => {
                let width = proposal.width.unwrap_or(0.0);
                Size::new(width, self.thickness)
            }
            Orientation::Vertical => {
                let height = proposal.height.unwrap_or(0.0);
                Size::new(self.thickness, height)
            }
        }
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self.color.unwrap_or(ctx.theme.colors.border);
        let (from, to) = match self.orientation {
            Orientation::Horizontal => {
                let y = bounds.y + bounds.height / 2.0;
                (Point::new(bounds.x, y), Point::new(bounds.right(), y))
            }
            Orientation::Vertical => {
                let x = bounds.x + bounds.width / 2.0;
                (Point::new(x, bounds.y), Point::new(x, bounds.bottom()))
            }
        };
        canvas.draw_line(from, to, color, StrokeStyle::solid(self.thickness));
    }

    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Splitter);
    }

    fn take_visible_when(&mut self) -> Option<State<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<State<bool>> {
        self.enabled_when_state.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn horizontal_divider_size() {
        let mut tree = WidgetTree::new();
        let d = tree.add(Divider::new());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let b = tree.bounds(d);
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 1.0).abs() < 0.01);
    }

    #[test]
    fn vertical_divider_size() {
        let mut tree = WidgetTree::new();
        let d = tree.add(Divider::vertical());
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let b = tree.bounds(d);
        assert!((b.width - 1.0).abs() < 0.01);
        assert!((b.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn custom_thickness() {
        let mut tree = WidgetTree::new();
        let d = tree.add(Divider::new().thickness(3.0));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let b = tree.bounds(d);
        assert!((b.height - 3.0).abs() < 0.01);
    }

    #[test]
    fn divider_paints_line() {
        let mut tree = WidgetTree::new();
        tree.add(Divider::new().color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(!frame.decorations.is_empty(), "divider should paint a decoration");
    }
}
