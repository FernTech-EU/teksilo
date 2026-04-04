//! Toggle — an animated on/off switch.
//!
//! Level 2 widget that paints a track and knob directly. The knob position
//! is animated via `State<f32>::set_animated()`.

use std::cell::Cell;
use std::time::Duration;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent, TapRecognizer};
use fern_core::state::{BindingLevel, Reactive, State};
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, Easing};

const TRACK_WIDTH: f32 = 44.0;
const TRACK_HEIGHT: f32 = 24.0;
const KNOB_SIZE: f32 = 20.0;
const KNOB_MARGIN: f32 = 2.0;

/// An animated toggle switch bound to a `State<bool>`.
pub struct Toggle {
    on: State<bool>,
    knob_position: State<f32>,
    label: Option<String>,
    enabled: bool,
    hovered: Cell<bool>,
    focus_origin: Option<FocusOrigin>,
    tap_recognizer: TapRecognizer,
    visible_when_state: Option<Reactive<bool>>,
    enabled_when_state: Option<Reactive<bool>>,
}

impl Toggle {
    pub fn new(on: State<bool>) -> Self {
        let initial = if *on.get() { 1.0 } else { 0.0 };
        Self {
            on,
            knob_position: State::new_animated(initial),
            label: None,
            enabled: true,
            hovered: Cell::new(false),
            focus_origin: None,
            tap_recognizer: TapRecognizer::new(),
            visible_when_state: None,
            enabled_when_state: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn visible_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.visible_when_state = Some(state.into());
        self
    }

    pub fn enabled_when(mut self, state: impl Into<Reactive<bool>>) -> Self {
        self.enabled_when_state = Some(state.into());
        self
    }

    fn toggle(&self) {
        let new_on = !*self.on.get();
        self.on.set(new_on);
        let target = if new_on { 1.0 } else { 0.0 };
        self.knob_position.set_animated(
            target,
            Duration::from_millis(150),
            Easing::EaseInOut,
        );
    }

    fn knob_x(&self, bounds: Rect) -> f32 {
        let t = self.knob_position.get().clamp(0.0, 1.0);
        let min_x = bounds.x + KNOB_MARGIN;
        let max_x = bounds.x + TRACK_WIDTH - KNOB_SIZE - KNOB_MARGIN;
        fern_tokens::lerp(min_x, max_x, t)
    }
}

impl std::fmt::Debug for Toggle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toggle")
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Widget for Toggle {
    fn size_that_fits(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let track_w = TRACK_WIDTH.max(48.0);
        let h = TRACK_HEIGHT.max(48.0);
        if let Some(ref label) = self.label {
            let label_w = if let Some(backend) = ctx.text_backend {
                let mut b = backend.borrow_mut();
                let layout = b.layout_single_line(label, &ctx.theme.typography.body, None);
                layout.width
            } else {
                label.len() as f32 * 8.0
            };
            Size::new(track_w + 8.0 + label_w, h)
        } else {
            Size::new(track_w, h)
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
        let colors = &ctx.theme.colors;

        // Center the track within the (possibly 48×48) bounds
        let track_x = bounds.x + (bounds.width - TRACK_WIDTH) / 2.0;
        let track_y = bounds.y + (bounds.height - TRACK_HEIGHT) / 2.0;
        let track_rect = Rect::new(track_x, track_y, TRACK_WIDTH, TRACK_HEIGHT);

        // Track color based on on-state
        let t = *self.knob_position.get();
        let track_color = if !self.enabled {
            colors.disabled_fill
        } else {
            // Interpolate between off and on colors
            let off = colors.surface_tertiary;
            let on = colors.primary;
            Color::new(
                fern_tokens::lerp(off.r(), on.r(), t),
                fern_tokens::lerp(off.g(), on.g(), t),
                fern_tokens::lerp(off.b(), on.b(), t),
                fern_tokens::lerp(off.a(), on.a(), t),
            )
        };
        canvas.fill_rounded_rect(
            track_rect,
            CornerRadius::uniform(TRACK_HEIGHT / 2.0),
            track_color,
        );

        // Focus ring — only for keyboard navigation, not pointer clicks.
        // Drawn with a 2px offset outside the track so it's visible over any fill color.
        if self.focus_origin == Some(FocusOrigin::Keyboard) {
            let offset = 3.0; // 2px gap + half of 2px stroke
            let ring_rect = Rect::new(
                track_rect.x - offset,
                track_rect.y - offset,
                track_rect.width + offset * 2.0,
                track_rect.height + offset * 2.0,
            );
            canvas.stroke_rounded_rect(
                ring_rect,
                CornerRadius::uniform(TRACK_HEIGHT / 2.0 + offset),
                colors.focus_ring,
                2.0,
            );
        }

        // Knob
        let knob_x = self.knob_x(Rect::new(track_x, track_y, TRACK_WIDTH, TRACK_HEIGHT));
        let knob_y = track_y + (TRACK_HEIGHT - KNOB_SIZE) / 2.0;
        let knob_rect = Rect::new(knob_x, knob_y, KNOB_SIZE, KNOB_SIZE);
        let knob_color = if !self.enabled {
            colors.disabled_text
        } else {
            Color::WHITE
        };
        canvas.fill_rounded_rect(
            knob_rect,
            CornerRadius::uniform(KNOB_SIZE / 2.0),
            knob_color,
        );

        // Label text (drawn to the right of the track)
        if let Some(ref label) = self.label {
            let text_color = if self.enabled {
                colors.on_surface
            } else {
                colors.disabled_text
            };
            let text_x = track_x + TRACK_WIDTH + 8.0;
            let text_y = bounds.y;
            let text_rect = Rect::new(
                text_x,
                text_y,
                (bounds.width - TRACK_WIDTH - 8.0).max(0.0),
                bounds.height,
            );
            canvas.draw_text(label, text_rect, &ctx.theme.typography.body, text_color);
        }
    }

    fn event(&mut self, event: &WidgetEvent, ctx: &mut EventContext) -> EventResponse {
        if !self.enabled {
            return EventResponse::Ignored;
        }

        match event {
            WidgetEvent::PointerDown { position, button } => {
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
                    self.toggle();
                }
                EventResponse::Handled
            }
            WidgetEvent::PointerMove { position } => {
                self.tap_recognizer.process(&RawPointerEvent::Move {
                    position: *position,
                });
                EventResponse::Ignored
            }
            WidgetEvent::PointerEnter => {
                self.hovered.set(true);
                ctx.set_cursor(CursorIcon::Pointer);
                EventResponse::Handled
            }
            WidgetEvent::PointerLeave => {
                self.hovered.set(false);
                self.tap_recognizer.reset();
                ctx.set_cursor(CursorIcon::Default);
                EventResponse::Handled
            }
            WidgetEvent::KeyDown { key: Key::Space, .. } => EventResponse::Handled,
            WidgetEvent::KeyUp { key: Key::Space, .. } => {
                self.toggle();
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
                if *action == fern_core::accesskit::Action::Click {
                    self.toggle();
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
        builder.set_role(fern_core::accesskit::Role::Switch);
        if let Some(ref label) = self.label {
            builder.set_name(label);
        }
        builder.set_toggled(*self.on.get());
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(fern_core::accesskit::Action::Click);
        builder.add_action(fern_core::accesskit::Action::Focus);
    }

    fn register_bindings(&self, id: WidgetId, registry: &fern_core::state::BindingRegistry) {
        self.knob_position.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.on.bind_to(id, registry, BindingLevel::RepaintOnly);
    }

    fn animated_states(&self) -> Vec<State<f32>> {
        vec![self.knob_position.clone()]
    }

    fn take_visible_when(&mut self) -> Option<Reactive<bool>> {
        self.visible_when_state.take()
    }

    fn take_enabled_when(&mut self) -> Option<Reactive<bool>> {
        self.enabled_when_state.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::event::Modifiers;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn click_toggles_state() {
        let on = State::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.click(t);
        assert!(*on.get());
        tree.click(t);
        assert!(!*on.get());
    }

    #[test]
    fn space_toggles_state() {
        let on = State::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.focus(t);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(*on.get());
    }

    #[test]
    fn animation_interpolates_knob_position() {
        let on = State::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on.clone()));
        tree.layout(SizeProposal::exact(100.0, 60.0));

        tree.click(t); // toggles on, starts animation to 1.0
        assert!(*on.get());

        // At midpoint, knob_position should be between 0 and 1
        tree.tick_animations(Duration::from_millis(75));
        // We can't easily read knob_position from outside, but the animation
        // should still be running (not yet at 1.0)
        assert!(tree.has_active_animations());

        // After full duration, animation should be complete
        tree.tick_animations(Duration::from_millis(100));
        assert!(!tree.has_active_animations());
    }

    #[test]
    fn accessibility() {
        let on = State::new(true);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on));
        tree.layout(SizeProposal::exact(100.0, 60.0));
        let info = tree.accessibility_node(t);
        assert_eq!(info.role(), fern_core::accesskit::Role::Switch);
        assert!(info.is_toggled());
    }

    #[test]
    fn accessibility_has_actions() {
        let on = State::new(false);
        let mut tree = WidgetTree::new();
        let t = tree.add(Toggle::new(on));
        tree.layout(SizeProposal::exact(100.0, 60.0));
        let info = tree.accessibility_node(t);
        assert!(info.actions().contains(&fern_core::accesskit::Action::Click));
    }
}
