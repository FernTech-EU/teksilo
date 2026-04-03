//! IconWidget — a vector icon rendered via the Path/PathAtlas system.
//!
//! Takes a `Path` and renders it at a configurable size. Provides factory
//! methods for common icons (checkmark, chevrons) used by other widgets.

use fern_canvas::{Canvas, Path, PathCommand, Point, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::state::{BindingLevel, Reactive, State};
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

/// A leaf widget that renders a vector icon path.
pub struct IconWidget {
    /// The path to render, defined in a coordinate space matching `size`.
    path: Path,
    /// Icon size (width = height).
    size: f32,
    /// Fill color.
    color: Reactive<Color>,
    visible_when_state: Option<State<bool>>,
    enabled_when_state: Option<State<bool>>,
}

impl IconWidget {
    /// Create an icon from a custom path. The path should be defined
    /// in coordinates matching the given size (e.g., 0..24 for size=24).
    pub fn from_path(path: Path, size: f32) -> Self {
        Self {
            path,
            size,
            color: Reactive::Static(Color::BLACK),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    /// A checkmark icon (✓) at the given size.
    pub fn checkmark(size: f32) -> Self {
        let mut path = Path::new();
        // Checkmark stroke: from bottom-left area through bottom-center to top-right
        let s = size;
        path.move_to(Point::new(s * 0.2, s * 0.5));
        path.line_to(Point::new(s * 0.4, s * 0.75));
        path.line_to(Point::new(s * 0.8, s * 0.25));
        Self::from_path(path, size)
    }

    /// A downward-pointing chevron (▼) at the given size.
    pub fn chevron_down(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.25, s * 0.35));
        path.line_to(Point::new(s * 0.5, s * 0.65));
        path.line_to(Point::new(s * 0.75, s * 0.35));
        Self::from_path(path, size)
    }

    /// A right-pointing chevron (▶) at the given size.
    pub fn chevron_right(size: f32) -> Self {
        let mut path = Path::new();
        let s = size;
        path.move_to(Point::new(s * 0.35, s * 0.25));
        path.line_to(Point::new(s * 0.65, s * 0.5));
        path.line_to(Point::new(s * 0.35, s * 0.75));
        Self::from_path(path, size)
    }

    /// Set the icon fill color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Reactive::Static(color);
        self
    }

    /// Bind the icon color to a reactive state.
    pub fn bind_color(mut self, state: impl Into<Reactive<Color>>) -> Self {
        self.color = state.into();
        self
    }

    /// Set the icon size.
    pub fn icon_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Bind visibility to a boolean state.
    pub fn visible_when(mut self, state: State<bool>) -> Self {
        self.visible_when_state = Some(state);
        self
    }

    /// Bind enabled state to a boolean state.
    pub fn enabled_when(mut self, state: State<bool>) -> Self {
        self.enabled_when_state = Some(state);
        self
    }

    /// Create a scaled copy of the path to fit within the given bounds.
    fn scaled_path(&self, bounds: Rect) -> Path {
        if self.path.is_empty() {
            return self.path.clone();
        }
        let scale_x = bounds.width / self.size;
        let scale_y = bounds.height / self.size;
        let offset_x = bounds.x;
        let offset_y = bounds.y;

        let mut scaled = Path::new();
        for cmd in &self.path.commands {
            match *cmd {
                PathCommand::MoveTo(p) => {
                    scaled.move_to(Point::new(
                        p.x * scale_x + offset_x,
                        p.y * scale_y + offset_y,
                    ));
                }
                PathCommand::LineTo(p) => {
                    scaled.line_to(Point::new(
                        p.x * scale_x + offset_x,
                        p.y * scale_y + offset_y,
                    ));
                }
                PathCommand::QuadTo { control, to } => {
                    scaled.quad_to(
                        Point::new(control.x * scale_x + offset_x, control.y * scale_y + offset_y),
                        Point::new(to.x * scale_x + offset_x, to.y * scale_y + offset_y),
                    );
                }
                PathCommand::CubicTo { control1, control2, to } => {
                    scaled.cubic_to(
                        Point::new(control1.x * scale_x + offset_x, control1.y * scale_y + offset_y),
                        Point::new(control2.x * scale_x + offset_x, control2.y * scale_y + offset_y),
                        Point::new(to.x * scale_x + offset_x, to.y * scale_y + offset_y),
                    );
                }
                PathCommand::ArcTo { rect, start_angle, sweep_angle } => {
                    scaled.arc_to(
                        Rect::new(
                            rect.x * scale_x + offset_x,
                            rect.y * scale_y + offset_y,
                            rect.width * scale_x,
                            rect.height * scale_y,
                        ),
                        start_angle,
                        sweep_angle,
                    );
                }
                PathCommand::Close => {
                    scaled.close();
                }
            }
        }
        scaled
    }
}

impl std::fmt::Debug for IconWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconWidget")
            .field("size", &self.size)
            .finish()
    }
}

impl Widget for IconWidget {
    fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        Size::new(self.size, self.size)
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let color = self.color.get();
        if color.a() > 0.0 && !self.path.is_empty() {
            let scaled = self.scaled_path(bounds);
            canvas.fill_path(&scaled, color);
        }
    }

    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Icons are typically decorative — the parent widget sets the semantic role.
    }

    fn register_bindings(&self, id: WidgetId, registry: &fern_core::state::BindingRegistry) {
        self.color.register_if_bound(id, registry, BindingLevel::RepaintOnly);
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
    fn icon_intrinsic_size() {
        let mut tree = WidgetTree::new();
        let icon = tree.add(IconWidget::checkmark(24.0));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(icon);
        assert!((b.width - 24.0).abs() < 0.01);
        assert!((b.height - 24.0).abs() < 0.01);
    }

    #[test]
    fn icon_custom_size() {
        let mut tree = WidgetTree::new();
        let icon = tree.add(IconWidget::chevron_down(16.0));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(icon);
        assert!((b.width - 16.0).abs() < 0.01);
        assert!((b.height - 16.0).abs() < 0.01);
    }

    #[test]
    fn icon_paints_path() {
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::checkmark(24.0).color(Color::BLACK));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(!frame.paths.is_empty(), "icon should render a path");
    }

    #[test]
    fn empty_path_does_not_paint() {
        let mut tree = WidgetTree::new();
        tree.add(IconWidget::from_path(Path::new(), 24.0).color(Color::BLACK));
        tree.layout(SizeProposal::exact(24.0, 24.0));
        let frame = tree.render();
        assert!(frame.paths.is_empty(), "empty path should not render");
    }
}
