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
use fern_core::gesture::DragPhase;
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_tokens::{CornerRadius, Orientation};

/// Minimum cross-axis size of the slider row, in dp. Sized to accommodate
/// the thumb plus the focus-ring envelope.
const MIN_CROSS_SIZE: f32 = 24.0;

/// A slider that drives a `Signal<f32>` between min and max.
pub struct Slider {
    value: Signal<f32>,
    min: f32,
    max: f32,
    step: Option<f32>,
    orientation: Orientation,
    enabled: bool,
    /// Accessible name, announced by screen readers as the control's label.
    label: Option<String>,
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
            label: None,
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

    /// Set an accessible name for the slider, announced by screen readers.
    /// ARIA requires sliders to have a label; when none is set here the
    /// caller is responsible for labelling via a wrapping element.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Shim (permanent, `#[doc(hidden)]`) for `label(...)` accepting a raw string.
    #[doc(hidden)]
    pub fn label_literal(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
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
    fn thumb_center(&self, bounds: Rect, thumb_radius: f32) -> f32 {
        let usable = self.primary_length(bounds) - thumb_radius * 2.0;
        self.primary_start(bounds) + thumb_radius + usable * self.normalized()
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
            fern_core::binding::BindingLevel::RepaintOnly,
        );

        // Capture the thumb radius at build time. The event handlers need
        // it for hit-testing, but they only receive `EventContext` and can't
        // reach the theme at event time. Theme changes between builds
        // would give a slightly stale hit region (single-digit pixels);
        // paint-time reads via `ctx.theme` keep the rendered thumb
        // correct. Trade-off accepted here rather than threading
        // `theme_signal` through every event handler closure.
        let thumb_radius = ctx.theme_signal().get().components.slider.thumb_diameter * 0.5;

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
                } - thumb_radius * 2.0;
                if usable <= 0.0 {
                    return;
                }
                let start = match orientation {
                    Orientation::Horizontal => bounds.x,
                    Orientation::Vertical => bounds.y,
                };
                let t = ((pos - start - thumb_radius) / usable).clamp(0.0, 1.0);
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

        // Thumb drag — routed through the typed gesture API. The
        // framework auto-captures the pointer at `DragPhase::Started`
        // and releases it at `DragPhase::Ended`, so the slider keeps
        // receiving `Moved` events even when the cursor leaves its
        // bounds (the old `on_pointer_event` path silently stopped
        // updating when the pointer moved off the slider).
        {
            let dragging = dragging.clone();
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_drag(move |phase, _ctx| {
                if !enabled {
                    return;
                }
                match phase {
                    DragPhase::Started {
                        position,
                        button: PointerButton::Primary,
                    } => {
                        dragging.set(true);
                        set_value(position.x, position.y);
                    }
                    DragPhase::Moved { position, .. } if dragging.get() => {
                        set_value(position.x, position.y);
                    }
                    DragPhase::Ended { .. } => {
                        dragging.set(false);
                    }
                    _ => {}
                }
            });
        }

        // Track click — jump the value to the click position without
        // entering a drag. A press+release without movement past the
        // 5 px drag threshold lands here; a longer press that slides
        // past threshold goes to `on_drag` instead.
        {
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_tap(move |event, _ctx| {
                if !enabled {
                    return;
                }
                set_value(event.position.x, event.position.y);
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

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        let style = ctx.theme.components.slider;
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        // Reserve the focus-ring envelope around the thumb plus a dp of slack
        // so the row has a comfortable hit area (matches the 24 dp Int UI
        // control-row height).
        let cross = (style.thumb_diameter + envelope * 2.0).max(MIN_CROSS_SIZE);
        match self.orientation {
            Orientation::Horizontal => {
                let width = proposal.width.unwrap_or(200.0);
                Size::new(width, cross)
            }
            Orientation::Vertical => {
                let height = proposal.height.unwrap_or(200.0);
                Size::new(cross, height)
            }
        }.into()
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
        let shape = &ctx.theme.shape;
        let style = ctx.theme.components.slider;
        let track_height = style.track_height;
        let thumb_diameter = style.thumb_diameter;
        let thumb_radius = thumb_diameter * 0.5;
        self.cached_bounds.set(bounds);

        let radius = CornerRadius::uniform(track_height * 0.5);
        let track_color = if self.enabled {
            colors.surface_sunken
        } else {
            colors.accent_disabled
        };
        let fill_color = if self.enabled {
            colors.accent
        } else {
            colors.text_disabled
        };
        let thumb_pos = self.thumb_center(bounds, thumb_radius);

        let (track_rect, fill_rect, thumb_cx, thumb_cy) = match self.orientation {
            Orientation::Horizontal => {
                let ty = bounds.y + (bounds.height - track_height) * 0.5;
                let track = Rect::new(
                    bounds.x + thumb_radius,
                    ty,
                    bounds.width - thumb_radius * 2.0,
                    track_height,
                );
                let fill_w = thumb_pos - track.x;
                let fill = Rect::new(track.x, ty, fill_w.max(0.0), track_height);
                (track, fill, thumb_pos, bounds.y + bounds.height * 0.5)
            }
            Orientation::Vertical => {
                let tx = bounds.x + (bounds.width - track_height) * 0.5;
                let track = Rect::new(
                    tx,
                    bounds.y + thumb_radius,
                    track_height,
                    bounds.height - thumb_radius * 2.0,
                );
                let fill_h = thumb_pos - track.y;
                let fill = Rect::new(tx, track.y, track_height, fill_h.max(0.0));
                (track, fill, bounds.x + bounds.width * 0.5, thumb_pos)
            }
        };

        canvas.fill_rounded_rect(track_rect, radius, track_color);
        if fill_rect.width > 0.0 && fill_rect.height > 0.0 {
            canvas.fill_rounded_rect(fill_rect, radius, fill_color);
        }

        // Thumb
        let thumb_color = if !self.enabled {
            colors.text_disabled
        } else if self.dragging.get() {
            colors.accent_pressed
        } else if self.hovered.get() {
            colors.accent_hover
        } else {
            colors.accent
        };
        let thumb_rect = Rect::new(
            thumb_cx - thumb_radius,
            thumb_cy - thumb_radius,
            thumb_diameter,
            thumb_diameter,
        );
        canvas.fill_rounded_rect(thumb_rect, CornerRadius::uniform(thumb_radius), thumb_color);

        // Focus ring — drawn outside the thumb using theme offset/width.
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let offset = shape.focus_ring_offset;
            let half_stroke = shape.focus_ring_width * 0.5;
            // Ring rect: outer edge at `thumb + offset + half_stroke`, drawn
            // with stroke width `focus_ring_width` centered on that rect
            // boundary. The stroke's outer edge lands at
            // `thumb + offset + focus_ring_width`.
            let ring_inset = offset + half_stroke;
            let ring_rect = Rect::new(
                thumb_rect.x - ring_inset,
                thumb_rect.y - ring_inset,
                thumb_rect.width + ring_inset * 2.0,
                thumb_rect.height + ring_inset * 2.0,
            );
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(thumb_radius + ring_inset),
                colors.focus_ring,
                shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::Slider);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        builder.set_numeric_value(self.value.get() as f64);
        builder.set_min_numeric_value(self.min as f64);
        builder.set_max_numeric_value(self.max as f64);
        // Publish the keyboard step so Orca / VoiceOver can announce
        // "step by N" when the user holds an arrow key. If the caller
        // didn't configure an explicit step, fall back to 1% of the
        // range — same heuristic the keyboard handler uses.
        let step = self.step.unwrap_or_else(|| (self.max - self.min) * 0.01);
        builder.set_numeric_value_step(step as f64);
        let orientation = match self.orientation {
            Orientation::Horizontal => fern_core::accesskit::Orientation::Horizontal,
            Orientation::Vertical => fern_core::accesskit::Orientation::Vertical,
        };
        builder.set_orientation(orientation);
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
        let theme = fern_tokens::Theme::light_default();
        let thumb_radius = theme.components.slider.thumb_diameter * 0.5;
        let value = Signal::new(50.0_f32);
        let mut tree = WidgetTree::new().with_theme(theme);
        let s = tree.add(Slider::new(value.clone(), 0.0, 100.0));
        tree.layout(SizeProposal::exact(200.0, 60.0));
        tree.render(); // cache bounds for event handling

        let bounds = tree.bounds(s);
        // Thumb center for value=50: bounds.x + r + (width - 2r) * 0.5
        let thumb_cx = bounds.x + thumb_radius + (bounds.width - thumb_radius * 2.0) * 0.5;
        let center_y = bounds.y + bounds.height / 2.0;

        // Pointer down on thumb
        tree.pointer_down_button(Point::new(thumb_cx, center_y), PointerButton::Primary);

        // Drag to 75% position. DragRecognizer needs one move past its
        // 5 px threshold to emit `DragStarted` (which carries the *down*
        // position, leaving value at 50%), and a second move to emit
        // `DragMoved` — the latter is what actually drives the value.
        let target_x = bounds.x + thumb_radius + (bounds.width - thumb_radius * 2.0) * 0.75;
        tree.pointer_move(Point::new(thumb_cx + 10.0, center_y));
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
