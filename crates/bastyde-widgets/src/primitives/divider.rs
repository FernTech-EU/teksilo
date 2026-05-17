//! Divider — a themed separator line.
//!
//! A thin line that separates content visually. Horizontal by default.
//! Color defaults to `theme.colors.border`.

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal, StrokeStyle};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};
#[cfg(test)]
use bastyde_tokens::Color;
use bastyde_tokens::Orientation;

/// A themed separator line. Thickness defaults to `DividerStyle::thickness`
/// and the color defaults to `BorderRole::Divider`; both can be overridden.
#[derive(Debug)]
pub struct Divider {
    orientation: Orientation,
    thickness: Option<f32>,
    color: Option<ColorProp>,
}

impl Divider {
    pub fn new() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            thickness: None,
            color: None,
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
        self.thickness = Some(thickness);
        self
    }

    /// Override the line color. Accepts `Color`, a role (typically
    /// [`BorderRole`](bastyde_tokens::BorderRole)), or a `Signal<Color>`.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = Some(color.into());
        self
    }

    fn resolved_thickness(&self, _theme: &bastyde_core::Theme) -> f32 {
        self.thickness.unwrap_or(DIVIDER_THICKNESS)
    }
}

/// Default visual thickness of a `Divider` stroke. Relocated here
/// from `theme.components.divider.thickness` as part of the Stage G
/// teardown — divider has no per-widget `Recipe*Style` module to host
/// it, so the constant lives alongside the widget that reads it.
pub const DIVIDER_THICKNESS: f32 = 1.0;

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Divider {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let thickness = self.resolved_thickness(ctx.theme);
        match self.orientation {
            Orientation::Horizontal => {
                let width = proposal.width.unwrap_or(0.0);
                Size::new(width, thickness)
            }
            Orientation::Vertical => {
                let height = proposal.height.unwrap_or(0.0);
                Size::new(thickness, height)
            }
        }
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let color = self
            .color
            .as_ref()
            .map(|c| c.resolve(ctx.theme))
            .unwrap_or(ctx.theme.colors.divider);
        let thickness = self.resolved_thickness(ctx.theme);
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
        canvas.draw_line(from, to, color, StrokeStyle::solid(thickness));
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Splitter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_core::widget_tree::WidgetTree;

    #[test]
    fn horizontal_divider_size() {
        let mut tree = WidgetTree::new();
        let d = tree.add(Divider::new());
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let b = tree.bounds(d);
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 1.0).abs() < 0.01);
    }

    #[test]
    fn vertical_divider_size() {
        let mut tree = WidgetTree::new();
        let d = tree.add(Divider::vertical());
        tree.layout(SizeProposal {
            width: None,
            height: Some(100.0),
        });
        let b = tree.bounds(d);
        assert!((b.width - 1.0).abs() < 0.01);
        assert!((b.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn custom_thickness() {
        let mut tree = WidgetTree::new();
        let d = tree.add(Divider::new().thickness(3.0));
        tree.layout(SizeProposal {
            width: Some(200.0),
            height: None,
        });
        let b = tree.bounds(d);
        assert!((b.height - 3.0).abs() < 0.01);
    }

    #[test]
    fn divider_paints_line() {
        let mut tree = WidgetTree::new();
        tree.add(Divider::new().color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(
            !frame.decorations.is_empty(),
            "divider should paint a decoration"
        );
    }
}
