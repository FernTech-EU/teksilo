//! SegmentedControl — mutually exclusive segments in a horizontal row.
//!
//! Level 2 widget that paints segments directly and uses cached bounds
//! for position-based click-to-select.

use std::cell::Cell;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::{
    GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer,
};
use fern_core::signal::Signal;
use fern_core::state::BindingLevel;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, TextStyle};

/// Padding inside each segment.
const SEGMENT_PADDING_H: f32 = 12.0;
const SEGMENT_PADDING_V: f32 = 8.0;
/// Fallback character width when no text backend is available.
const FALLBACK_CHAR_WIDTH: f32 = 8.0;
const FALLBACK_LINE_HEIGHT: f32 = 16.0;

/// A segmented control with mutually exclusive segments.
pub struct SegmentedControl {
    labels: Vec<String>,
    selected: Signal<usize>,
    enabled: bool,
    hovered_segment: Cell<Option<usize>>,
    last_click_x: Cell<f32>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
    cached_bounds: Cell<Rect>,
}

impl SegmentedControl {
    pub fn new(labels: Vec<String>, selected: Signal<usize>) -> Self {
        Self {
            labels,
            selected,
            enabled: true,
            hovered_segment: Cell::new(None),
            last_click_x: Cell::new(0.0),
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            cached_bounds: Cell::new(Rect::ZERO),
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn segment_count(&self) -> usize {
        self.labels.len()
    }

    fn segment_width(&self, bounds: Rect) -> f32 {
        let n = self.segment_count();
        if n == 0 {
            return 0.0;
        }
        bounds.width / n as f32
    }

    fn segment_index_at(&self, x: f32, bounds: Rect) -> usize {
        let n = self.segment_count();
        if n == 0 || bounds.width <= 0.0 {
            return 0;
        }
        let relative = (x - bounds.x).max(0.0);
        let index = (relative / self.segment_width(bounds)).floor() as usize;
        index.min(n - 1)
    }

    fn segment_rect(&self, index: usize, bounds: Rect) -> Rect {
        let w = self.segment_width(bounds);
        Rect::new(bounds.x + index as f32 * w, bounds.y, w, bounds.height)
    }

    fn segment_corner_radius(&self, index: usize, radius: f32) -> CornerRadius {
        let n = self.segment_count();
        if n <= 1 {
            return CornerRadius::uniform(radius);
        }
        if index == 0 {
            CornerRadius {
                top_left: radius,
                bottom_left: radius,
                top_right: 0.0,
                bottom_right: 0.0,
            }
        } else if index == n - 1 {
            CornerRadius {
                top_left: 0.0,
                bottom_left: 0.0,
                top_right: radius,
                bottom_right: radius,
            }
        } else {
            CornerRadius::ZERO
        }
    }

    /// Estimate the intrinsic width of all segments.
    fn estimate_width(&self) -> f32 {
        let n = self.segment_count();
        if n == 0 {
            return 0.0;
        }
        let max_label_width = self
            .labels
            .iter()
            .map(|l| l.len() as f32 * FALLBACK_CHAR_WIDTH)
            .fold(0.0_f32, f32::max);
        (max_label_width + SEGMENT_PADDING_H * 2.0) * n as f32
    }
}

impl std::fmt::Debug for SegmentedControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControl")
            .field("labels", &self.labels)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for SegmentedControl {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        let width = proposal.width.unwrap_or(self.estimate_width());
        let height = (FALLBACK_LINE_HEIGHT + SEGMENT_PADDING_V * 2.0).max(48.0);
        Size::new(width, height)
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
        self.cached_bounds.set(bounds);
        let colors = &ctx.theme.colors;
        let shape = &ctx.theme.shape;
        let n = self.segment_count();
        if n == 0 {
            return;
        }

        let selected = self.selected.get();
        let hovered = self.hovered_segment.get();

        for i in 0..n {
            let rect = self.segment_rect(i, bounds);
            let cr = self.segment_corner_radius(i, shape.radius_sm);

            // Background
            let bg = if !self.enabled {
                colors.disabled_fill
            } else if i == selected {
                colors.primary
            } else if hovered == Some(i) {
                colors.primary.with_alpha(0.08)
            } else {
                Color::TRANSPARENT
            };
            if bg.a() > 0.0 {
                canvas.fill_rounded_rect(rect, cr, bg);
            }

            // Border
            canvas.stroke_rounded_rect(rect, cr, colors.border, shape.border_width);

            // Text
            let text_color = if !self.enabled {
                colors.disabled_text
            } else if i == selected {
                colors.on_primary
            } else {
                colors.on_surface
            };
            let text_rect = Rect::new(
                rect.x + SEGMENT_PADDING_H,
                rect.y + SEGMENT_PADDING_V,
                (rect.width - SEGMENT_PADDING_H * 2.0).max(0.0),
                (rect.height - SEGMENT_PADDING_V * 2.0).max(0.0),
            );
            canvas.draw_text(
                &self.labels[i],
                text_rect,
                &ctx.theme.typography.label,
                text_color,
            );
        }

        // Focus ring around the whole control
        if self.focus_origin == Some(FocusOrigin::Keyboard) {
            canvas.stroke_rounded_rect(
                bounds,
                CornerRadius::uniform(shape.radius_sm),
                colors.focus_ring,
                2.0,
            );
        }
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }

        let bounds = self.cached_bounds.get();

        match event {
            WidgetEvent::PointerDown { position, button } => {
                self.last_click_x.set(position.x);
                self.tap_recognizer.process(&RawPointerEvent::Down {
                    position: *position,
                    button: *button,
                });
                EventResponse::Handled
            }
            WidgetEvent::PointerUp { position, button } => {
                let result = self.tap_recognizer.process(&RawPointerEvent::Up {
                    position: *position,
                    button: *button,
                });
                if matches!(result, GestureResult::Recognized(GestureEvent::Tap { .. })) {
                    let index = self.segment_index_at(self.last_click_x.get(), bounds);
                    self.selected.set(index);
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position } => {
                self.tap_recognizer.process(&RawPointerEvent::Move {
                    position: *position,
                });
                let index = self.segment_index_at(position.x, bounds);
                let old = self.hovered_segment.get();
                if old != Some(index) {
                    self.hovered_segment.set(Some(index));
                }
                EventResponse::Ignored
            }
            WidgetEvent::PointerEnter => {
                ctx.set_cursor(CursorIcon::Pointer);
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.hovered_segment.set(None);
                self.tap_recognizer.reset();
                ctx.set_cursor(CursorIcon::Default);
                EventResponse::Handled
            }
            WidgetEvent::KeyDown { key: Key::ArrowRight, .. } => {
                let n = self.segment_count();
                if n > 0 {
                    let current = self.selected.get();
                    self.selected.set((current + 1) % n);
                }
                EventResponse::Handled
            }
            WidgetEvent::KeyDown { key: Key::ArrowLeft, .. } => {
                let n = self.segment_count();
                if n > 0 {
                    let current = self.selected.get();
                    self.selected.set(if current == 0 { n - 1 } else { current - 1 });
                }
                EventResponse::Handled
            }
            WidgetEvent::FocusGained { origin } => {
                self.focus_origin = Some(*origin);
                EventResponse::Handled
            }
            WidgetEvent::FocusLost => {
                self.focus_origin = None;
                EventResponse::Handled
            }
            WidgetEvent::AccessAction { action, .. } => {
                if *action == fern_core::accesskit::Action::Increment {
                    let n = self.segment_count();
                    if n > 0 {
                        let current = self.selected.get();
                        self.selected.set((current + 1) % n);
                    }
                    EventResponse::Handled
                } else if *action == fern_core::accesskit::Action::Decrement {
                    let n = self.segment_count();
                    if n > 0 {
                        let current = self.selected.get();
                        self.selected.set(if current == 0 { n - 1 } else { current - 1 });
                    }
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        self.enabled
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::TabList);
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Focus);
        builder.add_action(fern_core::accesskit::Action::Increment);
        builder.add_action(fern_core::accesskit::Action::Decrement);
    }

    fn register_bindings(&self, id: WidgetId, registry: &fern_core::state::BindingRegistry) {
        self.selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn click_selects_segment_by_position() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        tree.render(); // cache bounds

        // Click at center of 300px-wide control with 3 segments (100px each).
        // Center is x=150 → segment 1 (the middle one).
        tree.click(sc);
        assert_eq!(selected.get(), 1, "click at center should select segment 1");
    }

    #[test]
    fn keyboard_navigation() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.focus(sc);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 2);
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn keyboard_wraps_around() {
        let selected = Signal::new(2_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.focus(sc);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(selected.get(), 0, "should wrap from last to first");
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(selected.get(), 2, "should wrap from first to last");
    }

    #[test]
    fn accessibility() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert_eq!(info.role(), fern_core::accesskit::Role::TabList);
    }

    #[test]
    fn paints_selected_segment_with_primary_color() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let frame = tree.render();
        let primary = Theme::light_default().colors.primary.to_array();
        assert!(
            frame.shapes.iter().any(|s| s.color == primary),
            "selected segment should render with primary color"
        );
    }

    #[test]
    fn only_selected_segment_has_primary_color() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into(), "C".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let frame = tree.render();
        let primary = Theme::light_default().colors.primary.to_array();
        let primary_count = frame.shapes.iter().filter(|s| s.color == primary).count();
        assert_eq!(
            primary_count, 1,
            "exactly one segment should have primary color, got {}",
            primary_count
        );
    }

    #[test]
    fn accessibility_has_actions() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let sc = tree.add(SegmentedControl::new(
            vec!["A".into(), "B".into()],
            selected,
        ));
        tree.layout(SizeProposal::exact(300.0, 60.0));
        let info = tree.accessibility_node(sc);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Increment));
        assert!(info.actions().contains(&fern_core::accesskit::Action::Decrement));
    }
}
