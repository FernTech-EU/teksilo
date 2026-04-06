//! SegmentedControl — mutually exclusive segments in a horizontal row.
//!
//! Level 2 widget that paints segments directly and uses cached bounds
//! for position-based click-to-select.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_tokens::{Color, CornerRadius};

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
    hovered_segment: Rc<Cell<Option<usize>>>,
    last_click_x: Rc<Cell<f32>>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    cached_bounds: Rc<Cell<Rect>>,
}

impl SegmentedControl {
    pub fn new(labels: Vec<String>, selected: Signal<usize>) -> Self {
        Self {
            labels,
            selected,
            enabled: true,
            hovered_segment: Rc::new(Cell::new(None)),
            last_click_x: Rc::new(Cell::new(0.0)),
            focus_origin: Rc::new(Cell::new(None)),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
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
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.selected.bind_to(
            self_id,
            registry,
            fern_core::state::BindingLevel::RepaintOnly,
        );

        let selected = self.selected.clone();
        let enabled = self.enabled;
        let n = self.segment_count();
        let hovered_segment = self.hovered_segment.clone();
        let last_click_x = self.last_click_x.clone();
        let focus_origin = self.focus_origin.clone();
        let cached_bounds = self.cached_bounds.clone();

        let mut handlers = HandlerSet::new().focusable(enabled).cursor(CursorIcon::Pointer);

        // Pointer event handler (click to select segment)
        {
            let selected = selected.clone();
            let cached_bounds = cached_bounds.clone();
            let last_click_x = last_click_x.clone();
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_pointer_event(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let bounds = cached_bounds.get();
                match event {
                    WidgetEvent::PointerDown { position, .. } => {
                        last_click_x.set(position.x);
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { .. } => {
                        // Simple click detection: use last_click_x to determine segment
                        if n > 0 && bounds.width > 0.0 {
                            let seg_w = bounds.width / n as f32;
                            let relative = (last_click_x.get() - bounds.x).max(0.0);
                            let index = (relative / seg_w).floor() as usize;
                            selected.set(index.min(n - 1));
                        }
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerMove { position } => {
                        if n > 0 && bounds.width > 0.0 {
                            let seg_w = bounds.width / n as f32;
                            let relative = (position.x - bounds.x).max(0.0);
                            let index = (relative / seg_w).floor() as usize;
                            let idx = index.min(n - 1);
                            let old = hovered_segment.get();
                            if old != Some(idx) {
                                hovered_segment.set(Some(idx));
                            }
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Hover handler
        {
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                if !entered {
                    hovered_segment.set(None);
                }
            });
        }

        // Key handler
        {
            let selected = selected.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled || n == 0 {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowRight,
                        ..
                    } => {
                        let current = selected.get();
                        selected.set((current + 1) % n);
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowLeft,
                        ..
                    } => {
                        let current = selected.get();
                        selected.set(if current == 0 { n - 1 } else { current - 1 });
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Focus handler
        {
            let focus_origin = focus_origin.clone();
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                if gained {
                    let origin = if hovered_segment.get().is_some() {
                        FocusOrigin::Pointer
                    } else {
                        FocusOrigin::Keyboard
                    };
                    focus_origin.set(Some(origin));
                } else {
                    focus_origin.set(None);
                }
            });
        }

        // Access action handler
        {
            let selected = selected.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                if n == 0 {
                    return EventResponse::Ignored;
                }
                if action == fern_core::accesskit::Action::Increment {
                    let current = selected.get();
                    selected.set((current + 1) % n);
                    EventResponse::Handled
                } else if action == fern_core::accesskit::Action::Decrement {
                    let current = selected.get();
                    selected.set(if current == 0 { n - 1 } else { current - 1 });
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        ctx.apply_self_handlers(handlers);

        Vec::new()
    }

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
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            canvas.stroke_rounded_rect(
                bounds,
                CornerRadius::uniform(shape.radius_sm),
                colors.focus_ring,
                2.0,
            );
        }
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
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Increment)
        );
        assert!(
            info.actions()
                .contains(&fern_core::accesskit::Action::Decrement)
        );
    }
}
