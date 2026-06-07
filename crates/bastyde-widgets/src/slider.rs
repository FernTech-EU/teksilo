//! Slider — a draggable value selector.
//!
//! The widget itself owns input handling (drag, keyboard, accessibility
//! actions) and delegates all visual chrome to a [`SliderStyle`] impl.
//! The IntUI default ([`crate::styles::RecipeSliderStyle`]) ships out
//! of the box; apps install a different look per-call via
//! `Slider::style(...)` or theme-wide via `theme.style_slots.slider`.
//!
//! No `paint()` method on this widget — the only canvas work happens
//! inside the active `SliderStyle::make_body` subtree.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use bastyde_core::focus::FocusOrigin;
use bastyde_core::gesture::DragPhase;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    SharedSliderStyle, SliderOrientation, SliderStyle, SliderStyleConfig, SliderVariant,
};
use bastyde_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::Orientation;

// Re-export the variant enum at module top so callers can write
// `Slider::new(...).variant(SliderVariant::Discrete)` without a deeper
// import path.
pub use bastyde_core::styles::SliderVariant as SliderVariantExport;
use bastyde_i18n::LocalizedString;

/// A slider that drives a `Signal<f32>` between min and max.
pub struct Slider {
    value: Signal<f32>,
    min: f32,
    max: f32,
    step: Option<f32>,
    orientation: Orientation,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    /// Accessible name, announced by screen readers as the control's label.
    label: Option<LocalizedString>,
    variant: SliderVariant,
    tick_count: Option<u32>,
    style_override: Option<SharedSliderStyle>,
    hovered: Signal<bool>,
    dragging: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    cached_bounds: Rc<Cell<Rect>>,
    body_id: Option<WidgetId>,
}

impl Slider {
    pub fn new(value: Signal<f32>, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            step: None,
            orientation: Orientation::Horizontal,
            initial_enabled: true,
            label: None,
            variant: SliderVariant::default(),
            tick_count: None,
            style_override: None,
            hovered: Signal::new(false),
            dragging: Signal::new(false),
            focus_origin: Signal::new(None),
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            body_id: None,
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

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. Use `ctx.enabled_when(slider_id, signal)` for reactivity.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Pick a Tier-1 design-language variant
    /// ([`SliderVariant::Continuous`] / `Discrete` / `Range`). The
    /// active [`SliderStyle`] decides what to do with the hint —
    /// IntUI's default impl paints ticks for `Discrete` and ignores
    /// `Range` (the widget itself doesn't yet wire dual-thumb
    /// behaviour).
    pub fn variant(mut self, variant: SliderVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Configure the tick count for a `Discrete` slider. The
    /// IntUI default paints `n` evenly spaced tick marks above the
    /// track (or to the leading side for vertical orientation).
    pub fn tick_count(mut self, count: u32) -> Self {
        self.tick_count = Some(count);
        self
    }

    /// Override the active [`SliderStyle`] for this widget instance
    /// only.
    pub fn style(mut self, style: impl SliderStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Set an accessible name for the slider, announced by screen readers.
    /// ARIA requires sliders to have a label; when none is set here the
    /// caller is responsible for labelling via a wrapping element.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls);
        self
    }
}

impl std::fmt::Debug for Slider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slider")
            .field("min", &self.min)
            .field("max", &self.max)
            .field("initial_enabled", &self.initial_enabled)
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for Slider {
    fn build(
        &mut self,
        ctx: &mut bastyde_core::build_context::BuildContext,
    ) -> Vec<bastyde_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeSliderStyle` default.
        let style: SharedSliderStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.slider.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSliderStyle));

        // Derived `value_normalized` signal — re-renders the body
        // whenever the user-visible value changes.
        let min = self.min;
        let max = self.max;
        let value_normalized = self.value.map(move |v| {
            let range = max - min;
            if range <= 0.0 {
                0.0
            } else {
                ((*v - min) / range).clamp(0.0, 1.0)
            }
        });

        let orientation = match self.orientation {
            Orientation::Horizontal => SliderOrientation::Horizontal,
            Orientation::Vertical => SliderOrientation::Vertical,
        };

        let cfg = SliderStyleConfig {
            value_normalized,
            is_hovered: self.hovered.clone(),
            is_dragging: self.dragging.clone(),
            is_disabled: effective_enabled.map(|on| !*on),
            focus_origin: self.focus_origin.clone(),
            orientation,
            tick_count: self.tick_count,
            variant: self.variant,
        };
        let body_id = style.make_body(&cfg, ctx);
        self.body_id = Some(body_id);

        // Capture the thumb radius at build time. The event handlers
        // need it for value computation, but they only receive
        // `EventContext` and can't reach the theme at event time.
        // Query the *resolved* style so a custom `SliderStyle` with a
        // different thumb size keeps drag hit-testing aligned, instead of
        // baking in the recipe's design constant.
        let thumb_radius = style.thumb_diameter(&cfg) * 0.5;

        let value = self.value.clone();
        let step = self.step;
        let orientation = self.orientation;
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
                if let Some(s) = step
                    && s > 0.0
                {
                    val = ((val - min) / s).round() * s + min;
                }
                value.set(val.clamp(min, max));
            }
        };

        // Framework gates events on `arena.is_enabled(self_id)`, so
        // no per-handler enabled snapshot guards anymore.
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        // Thumb drag — routed through the typed gesture API.
        {
            let dragging = dragging.clone();
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_drag(move |phase, _ctx| match phase {
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
            });
        }

        // Track click — jump the value to the click position.
        {
            let set_value = set_value_from_position.clone();
            handlers = handlers.on_tap(move |event, _ctx| {
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
            handlers = handlers.on_key(move |event, _ctx| match event {
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
                bastyde_core::accesskit::Action::Increment => {
                    adjust(true);
                    EventResponse::Handled
                }
                bastyde_core::accesskit::Action::Decrement => {
                    adjust(false);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        ctx.apply_self_handlers(handlers);

        vec![body_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.body_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Cache bounds for event handling (needed before paint).
        self.cached_bounds.set(bounds);
        if let Some(child) = children.first_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Slider);
        if let Some(ref label) = self.label {
            builder.set_name(label.resolve_now());
        }
        builder.set_numeric_value(self.value.get() as f64);
        builder.set_min_numeric_value(self.min as f64);
        builder.set_max_numeric_value(self.max as f64);
        // Publish the keyboard step so Orca / VoiceOver can announce
        // "step by N" when the user holds an arrow key. If the caller
        // didn't configure an explicit step, fall back to 1% of the
        // range — same heuristic the keyboard handler uses.
        let step = self.step.unwrap_or((self.max - self.min) * 0.01);
        builder.set_numeric_value_step(step as f64);
        let orientation = match self.orientation {
            Orientation::Horizontal => bastyde_core::accesskit::Orientation::Horizontal,
            Orientation::Vertical => bastyde_core::accesskit::Orientation::Vertical,
        };
        builder.set_orientation(orientation);
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(bastyde_core::accesskit::Action::Increment);
        builder.add_action(bastyde_core::accesskit::Action::Decrement);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::Point;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;

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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        assert_eq!(info.role(), bastyde_core::accesskit::Role::Slider);
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
        let theme = bastyde_core::presets::intui::light();
        let thumb_radius = crate::styles::recipe_slider_style::SLIDER_THUMB_DIAMETER * 0.5;
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
                .contains(&bastyde_core::accesskit::Action::Increment)
        );
        assert!(
            info.actions()
                .contains(&bastyde_core::accesskit::Action::Decrement)
        );
    }
}
