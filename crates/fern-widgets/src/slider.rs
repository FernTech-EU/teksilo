//! Slider — a draggable value selector.
//!
//! Level 2 widget with track, filled portion, and draggable thumb.
//! Supports keyboard adjustment and accessibility.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_tokens::{CornerRadius, Orientation};

const TRACK_HEIGHT: f32 = 4.0;
const THUMB_RADIUS: f32 = 10.0;
const MIN_SIZE: f32 = 48.0;

/// A slider that drives a `Signal<f32>` between min and max.
pub struct Slider {
    value: Signal<f32>,
    min: f32,
    max: f32,
    step: Option<f32>,
    orientation: Orientation,
    enabled: bool,
    hovered: Rc<Cell<bool>>,
    dragging: Rc<Cell<bool>>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    cached_bounds: Rc<Cell<Rect>>,
}

impl Slider {
    pub fn new(value: Signal<f32>, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            step: None,
            orientation: Orientation::Horizontal,
            enabled: true,
            hovered: Rc::new(Cell::new(false)),
            dragging: Rc::new(Cell::new(false)),
            focus_origin: Rc::new(Cell::new(None)),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
        }
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Primary axis length of the slider.
    fn primary_length(&self, bounds: Rect) -> f32 {
        match self.orientation {
            Orientation::Horizontal => bounds.width,
            Orientation::Vertical => bounds.height,
        }
    }

    /// Primary axis start of bounds.
    fn primary_start(&self, bounds: Rect) -> f32 {
        match self.orientation {
            Orientation::Horizontal => bounds.x,
            Orientation::Vertical => bounds.y,
        }
    }

    fn normalized(&self) -> f32 {
        let range = self.max - self.min;
        if range <= 0.0 {
            return 0.0;
        }
        ((self.value.get() - self.min) / range).clamp(0.0, 1.0)
    }

    /// Thumb center position on the primary axis.
    fn thumb_center(&self, bounds: Rect) -> f32 {
        let usable = self.primary_length(bounds) - THUMB_RADIUS * 2.0;
        self.primary_start(bounds) + THUMB_RADIUS + usable * self.normalized()
    }
}

impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("min", &self.min)
            .field("max", &self.max)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Slider {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value.bind_to(
            self_id,
            registry,
            fern_core::state::BindingLevel::RepaintOnly,
        );

        let value = self.value.clone();
        let min = self.min;
        let max = self.max;
        let step = self.step;
        let orientation = self.orientation;
        let enabled = self.enabled;
        let hovered = self.hovered.clone();
        let dragging = self.dragging.clone();
        let focus_origin = self.focus_origin.clone();
        let cached_bounds = self.cached_bounds.clone();

        let adjust_by_step = {
            let value = value.clone();
            move |positive: bool| {
                let s = step.unwrap_or((max - min) * 0.01);
                let current = value.get();
                let new_val = if positive { current + s } else { current - s };
                value.set(new_val.clamp(min, max));
            }
        };

        let set_value_from_position = {
            let value = value.clone();
            let cached_bounds = cached_bounds.clone();
            move |x: f32, y: f32| {
                let bounds = cached_bounds.get();
                let pos = match orientation {
                    Orientation::Horizontal => x,
                    Orientation::Vertical => y,
                };
                let usable = match orientation {
                    Orientation::Horizontal => bounds.width,
                    Orientation::Vertical => bounds.height,
                } - THUMB_RADIUS * 2.0;
                if usable <= 0.0 {
                    return;
                }
                let start = match orientation {
                    Orientation::Horizontal => bounds.x,
                    Orientation::Vertical => bounds.y,
                };
                let t = ((pos - start - THUMB_RADIUS) / usable).clamp(0.0, 1.0);
                let mut val = min + t * (max - min);
                if let Some(s) = step {
                    if s > 0.0 {
                        val = ((val - min) / s).round() * s + min;
                    }
                }
                value.set(val.clamp(min, max));
            }
        };

        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        // Pointer event handler (drag to change value)
        {
            let dragging = dragging.clone();
            let cached_bounds = cached_bounds.clone();
            let value = value.clone();
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_pointer_event(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let bounds = cached_bounds.get();
                match event {
                    WidgetEvent::PointerDown { position, button } => {
                        if *button == PointerButton::Primary {
                            // Check if click is on the thumb
                            let pos = match orientation {
                                Orientation::Horizontal => position.x,
                                Orientation::Vertical => position.y,
                            };
                            let usable = match orientation {
                                Orientation::Horizontal => bounds.width,
                                Orientation::Vertical => bounds.height,
                            } - THUMB_RADIUS * 2.0;
                            let start = match orientation {
                                Orientation::Horizontal => bounds.x,
                                Orientation::Vertical => bounds.y,
                            };
                            let range = max - min;
                            let normalized = if range > 0.0 {
                                ((value.get() - min) / range).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let thumb_center = start + THUMB_RADIUS + usable * normalized;
                            let on_thumb = (pos - thumb_center).abs() <= THUMB_RADIUS;
                            if !on_thumb {
                                set_value(position.x, position.y);
                            }
                            dragging.set(true);
                        }
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { .. } => {
                        dragging.set(false);
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerMove { position } => {
                        if dragging.get() {
                            set_value(position.x, position.y);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Hover handler
        {
            let hovered = hovered.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                hovered.set(entered);
            });
        }

        // Key handler
        {
            let adjust = adjust_by_step.clone();
            let value = value.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::KeyDown { key, .. } => match key {
                        Key::ArrowRight | Key::ArrowUp => {
                            adjust(true);
                            EventResponse::Handled
                        }
                        Key::ArrowLeft | Key::ArrowDown => {
                            adjust(false);
                            EventResponse::Handled
                        }
                        Key::Home => {
                            value.set(min);
                            EventResponse::Handled
                        }
                        Key::End => {
                            value.set(max);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    },
                    _ => EventResponse::Ignored,
                }
            });
        }

        // Focus handler
        {
            let focus_origin = focus_origin.clone();
            let hovered_for_focus = hovered.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                if gained {
                    let origin = if hovered_for_focus.get() {
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
            let adjust = adjust_by_step.clone();
            handlers = handlers.on_access_action(move |action, _ctx| match action {
                fern_core::accesskit::Action::Increment => {
                    adjust(true);
                    EventResponse::Handled
                }
                fern_core::accesskit::Action::Decrement => {
                    adjust(false);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        ctx.apply_self_handlers(handlers);

        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        match self.orientation {
            Orientation::Horizontal => {
                let width = proposal.width.unwrap_or(200.0);
                Size::new(width, MIN_SIZE)
            }
            Orientation::Vertical => {
                let height = proposal.height.unwrap_or(200.0);
                Size::new(MIN_SIZE, height)
            }
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Cache bounds for event handling (needed before paint)
        self.cached_bounds.set(bounds);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let colors = &ctx.theme.colors;
        self.cached_bounds.set(bounds);
        let radius = CornerRadius::uniform(TRACK_HEIGHT / 2.0);
        let track_color = if self.enabled {
            colors.surface_tertiary
        } else {
            colors.disabled_fill
        };
        let fill_color = if self.enabled {
            colors.primary
        } else {
            colors.disabled_text
        };
        let thumb_pos = self.thumb_center(bounds);

        let (track_rect, fill_rect, thumb_cx, thumb_cy) = match self.orientation {
            Orientation::Horizontal => {
                let ty = bounds.y + (bounds.height - TRACK_HEIGHT) / 2.0;
                let track = Rect::new(
                    bounds.x + THUMB_RADIUS,
                    ty,
                    bounds.width - THUMB_RADIUS * 2.0,
                    TRACK_HEIGHT,
                );
                let fill_w = thumb_pos - track.x;
                let fill = Rect::new(track.x, ty, fill_w.max(0.0), TRACK_HEIGHT);
                (track, fill, thumb_pos, bounds.y + bounds.height / 2.0)
            }
            Orientation::Vertical => {
                let tx = bounds.x + (bounds.width - TRACK_HEIGHT) / 2.0;
                let track = Rect::new(
                    tx,
                    bounds.y + THUMB_RADIUS,
                    TRACK_HEIGHT,
                    bounds.height - THUMB_RADIUS * 2.0,
                );
                let fill_h = thumb_pos - track.y;
                let fill = Rect::new(tx, track.y, TRACK_HEIGHT, fill_h.max(0.0));
                (track, fill, bounds.x + bounds.width / 2.0, thumb_pos)
            }
        };

        canvas.fill_rounded_rect(track_rect, radius, track_color);
        if fill_rect.width > 0.0 && fill_rect.height > 0.0 {
            canvas.fill_rounded_rect(fill_rect, radius, fill_color);
        }

        // Thumb
        let thumb_color = if !self.enabled {
            colors.disabled_text
        } else if self.dragging.get() {
            colors.primary_pressed
        } else if self.hovered.get() {
            colors.primary_hover
        } else {
            colors.primary
        };
        let thumb_rect = Rect::new(
            thumb_cx - THUMB_RADIUS,
            thumb_cy - THUMB_RADIUS,
            THUMB_RADIUS * 2.0,
            THUMB_RADIUS * 2.0,
        );
        canvas.fill_rounded_rect(thumb_rect, CornerRadius::uniform(THUMB_RADIUS), thumb_color);

        // Focus ring with offset so it's visible over the primary-colored thumb
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let offset = 3.0;
            let ring_rect = Rect::new(
                thumb_rect.x - offset,
                thumb_rect.y - offset,
                thumb_rect.width + offset * 2.0,
                thumb_rect.height + offset * 2.0,
            );
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(THUMB_RADIUS + offset),
                colors.focus_ring,
                2.0,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Slider);
        builder.set_numeric_value(self.value.get() as f64);
        builder.set_min_numeric_value(self.min as f64);
        builder.set_max_numeric_value(self.max as f64);
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Increment);
        builder.add_action(fern_core::accesskit::Action::Decrement);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_canvas::Point;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn keyboard_adjusts_value() {
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(10.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 60.0).abs() < 0.01, "value={}", value.get());

        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert!((value.get() - 50.0).abs() < 0.01);
    }

    #[test]
    fn home_end_jump_to_bounds() {
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        tree.press_key(Key::Home, Modifiers::NONE);
        assert!((value.get() - 0.0).abs() < 0.01);

        tree.press_key(Key::End, Modifiers::NONE);
        assert!((value.get() - 100.0).abs() < 0.01);
    }

    #[test]
    fn track_click_sets_value() {
        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        // Render to trigger paint() which caches bounds for event handling
        tree.render();

        // Click at the widget center
        tree.click(s);

        // Value should be approximately 50 (midpoint of 0..100)
        let val = value.get();
        assert!(
            (val - 50.0).abs() < 15.0,
            "track click at center should set value near 50, got {}",
            val
        );
    }

    #[test]
    fn accessibility() {
        let value = Signal::new(25.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value, 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let info = tree.accessibility_node(s);
        assert_eq!(info.role(), fern_core::accesskit::Role::Slider);
    }

    #[test]
    fn step_snaps_value() {
        let value = Signal::new(0.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0).step(25.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        tree.focus(s);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 25.0).abs() < 0.01);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!((value.get() - 50.0).abs() < 0.01);
    }

    #[test]
    fn thumb_drag_updates_value() {
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new().with_theme(fern_tokens::Theme::light_default());
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.render(); // cache bounds for event handling

        let bounds = tree.bounds(s);
        // Thumb center for value=50: bounds.x + THUMB_RADIUS + (bounds.width - 2*THUMB_RADIUS) * 0.5
        let thumb_cx = bounds.x + THUMB_RADIUS + (bounds.width - THUMB_RADIUS * 2.0) * 0.5;
        let center_y = bounds.y + bounds.height / 2.0;

        // Pointer down on thumb
        tree.pointer_down_button(Point::new(thumb_cx, center_y), PointerButton::Primary);

        // Drag to 75% position
        let target_x = bounds.x + THUMB_RADIUS + (bounds.width - THUMB_RADIUS * 2.0) * 0.75;
        tree.pointer_move(Point::new(target_x, center_y));

        let val = value.get();
        assert!(
            (val - 75.0).abs() < 5.0,
            "dragging to 75% should set value near 75, got {}",
            val
        );

        // Release
        tree.pointer_up_button(Point::new(target_x, center_y), PointerButton::Primary);
    }

    #[test]
    fn accessibility_has_actions() {
        let value = Signal::new(25.0_f32);
        let mut tree = WidgetTree::new();
        let s = tree.add(Slider::new(value, 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        let info = tree.accessibility_node(s);
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
