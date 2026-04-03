//! ProgressBar — a bar showing progress from 0.0 to 1.0.
//!
//! Supports determinate mode (bound to `State<f32>`) and indeterminate mode
//! (animated sweep when progress is unknown). Horizontal or vertical.

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, WidgetEvent};
use fern_core::state::{BindingLevel, Reactive, State};
use fern_core::widget::{EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, Easing, Orientation};

const DEFAULT_THICKNESS: f32 = 4.0;

/// A progress bar — determinate or indeterminate, horizontal or vertical.
pub struct ProgressBar {
    value: Reactive<f32>,
    indeterminate: bool,
    /// Animated position for indeterminate sweep (0.0–1.0, loops).
    indeterminate_pos: State<f32>,
    orientation: Orientation,
    thickness: f32,
    track_color: Option<Color>,
    fill_color: Option<Color>,
}

impl ProgressBar {
    /// Create a determinate progress bar with a static value (0.0–1.0).
    pub fn new(value: f32) -> Self {
        Self {
            value: Reactive::Static(value.clamp(0.0, 1.0)),
            indeterminate: false,
            indeterminate_pos: State::new(0.0),
            orientation: Orientation::Horizontal,
            thickness: DEFAULT_THICKNESS,
            track_color: None,
            fill_color: None,
        }
    }

    /// Create an indeterminate progress bar (animated sweep).
    pub fn indeterminate() -> Self {
        let pos = State::new(0.0);
        // Start the animation loop — the first set_animated kicks it off.
        pos.set_animated(1.0, std::time::Duration::from_millis(1500), Easing::EaseInOut);
        Self {
            value: Reactive::Static(0.0),
            indeterminate: true,
            indeterminate_pos: pos,
            orientation: Orientation::Horizontal,
            thickness: DEFAULT_THICKNESS,
            track_color: None,
            fill_color: None,
        }
    }

    /// Bind the progress value to a reactive state.
    pub fn bind_value(mut self, state: impl Into<Reactive<f32>>) -> Self {
        self.value = state.into();
        self
    }

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    /// Deprecated: use `.thickness()` instead.
    pub fn height(mut self, height: f32) -> Self {
        self.thickness = height;
        self
    }

    pub fn track_color(mut self, color: Color) -> Self {
        self.track_color = Some(color);
        self
    }

    pub fn fill_color(mut self, color: Color) -> Self {
        self.fill_color = Some(color);
        self
    }
}

impl std::fmt::Debug for ProgressBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressBar")
            .field("thickness", &self.thickness)
            .finish()
    }
}

impl Widget for ProgressBar {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        match self.orientation {
            Orientation::Horizontal => {
                let width = proposal.width.unwrap_or(100.0);
                Size::new(width, self.thickness)
            }
            Orientation::Vertical => {
                let height = proposal.height.unwrap_or(100.0);
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
        let track_color = self.track_color.unwrap_or(ctx.theme.colors.surface_tertiary);
        let fill_color = self.fill_color.unwrap_or(ctx.theme.colors.primary);
        let radius = CornerRadius::uniform(ctx.theme.shape.radius_sm);

        // Track
        canvas.fill_rounded_rect(bounds, radius, track_color);

        if self.indeterminate {
            // Indeterminate: animated sweep (30% of track length)
            let pos = self.indeterminate_pos.get().clamp(0.0, 1.0);
            let sweep_ratio = 0.3;
            let fill_rect = match self.orientation {
                Orientation::Horizontal => {
                    let sweep_w = bounds.width * sweep_ratio;
                    let x = bounds.x + (bounds.width - sweep_w) * pos;
                    Rect::new(x, bounds.y, sweep_w, bounds.height)
                }
                Orientation::Vertical => {
                    let sweep_h = bounds.height * sweep_ratio;
                    let y = bounds.y + (bounds.height - sweep_h) * pos;
                    Rect::new(bounds.x, y, bounds.width, sweep_h)
                }
            };
            canvas.fill_rounded_rect(fill_rect, radius, fill_color);

            // Re-trigger animation to loop (ping-pong: 1→0 or 0→1)
            let target = if pos >= 0.99 { 0.0 } else if pos <= 0.01 { 1.0 } else { return };
            self.indeterminate_pos.set_animated(
                target,
                std::time::Duration::from_millis(1500),
                Easing::EaseInOut,
            );
        } else {
            // Determinate: fill proportional to value
            let value = self.value.get().clamp(0.0, 1.0);
            if value > 0.0 {
                let fill_rect = match self.orientation {
                    Orientation::Horizontal => {
                        Rect::new(bounds.x, bounds.y, bounds.width * value, bounds.height)
                    }
                    Orientation::Vertical => {
                        let fill_h = bounds.height * value;
                        Rect::new(bounds.x, bounds.y + bounds.height - fill_h, bounds.width, fill_h)
                    }
                };
                canvas.fill_rounded_rect(fill_rect, radius, fill_color);
            }
        }
    }

    fn event(&mut self, _event: &WidgetEvent, _ctx: &mut EventContext) -> EventResponse {
        EventResponse::Ignored
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::ProgressIndicator);
        let value = self.value.get();
        builder.set_numeric_value(value as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(1.0);
    }

    fn register_bindings(&self, id: WidgetId, registry: &fern_core::state::BindingRegistry) {
        self.value.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        if self.indeterminate {
            self.indeterminate_pos.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
    }

    fn animated_states(&self) -> Vec<State<f32>> {
        if self.indeterminate {
            vec![self.indeterminate_pos.clone()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn progress_bar_size() {
        let mut tree = WidgetTree::new();
        let pb = tree.add(ProgressBar::new(0.5));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let b = tree.bounds(pb);
        assert!((b.width - 200.0).abs() < 0.01);
        assert!((b.height - 4.0).abs() < 0.01);
    }

    #[test]
    fn progress_bar_paints_track_and_fill() {
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        tree.add(ProgressBar::new(0.5));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        // Should have at least 2 shapes: track + fill
        assert!(frame.shapes.len() >= 2, "should have track and fill shapes");
    }

    #[test]
    fn progress_bar_fill_width_proportional() {
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        let pb = tree.add(ProgressBar::new(0.5).fill_color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        // The fill rect should be about half width (100px out of 200px)
        let fill_shapes: Vec<_> = frame
            .shapes
            .iter()
            .filter(|s| s.color == Color::RED.to_array())
            .collect();
        assert!(!fill_shapes.is_empty(), "should have a red fill shape");
        // Shape width is approximately 100 (half of 200)
        let fill = &fill_shapes[0];
        let fill_width = fill.screen[2]; // [x, y, w, h]
        assert!((fill_width - 100.0).abs() < 1.0, "fill width should be ~100, got {}", fill_width);
    }

    #[test]
    fn zero_value_no_fill() {
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        tree.add(ProgressBar::new(0.0).fill_color(Color::RED));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        // Only track, no fill
        let fill_shapes: Vec<_> = frame
            .shapes
            .iter()
            .filter(|s| s.color == Color::RED.to_array())
            .collect();
        assert!(fill_shapes.is_empty(), "zero progress should have no fill");
    }

    #[test]
    fn accessibility_values() {
        let mut tree = WidgetTree::new();
        let pb = tree.add(ProgressBar::new(0.75));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let info = tree.accessibility_node(pb);
        assert_eq!(info.role(), fern_core::accesskit::Role::ProgressIndicator);
    }
}
