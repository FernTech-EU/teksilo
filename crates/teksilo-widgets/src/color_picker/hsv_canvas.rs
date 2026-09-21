// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `HsvCanvas` — 2D saturation × value picker rendered as three stacked
//! gradients.
//!
//! Layer 1: solid fill with the current hue at full saturation + full
//! value. Layer 2: white→transparent linear gradient left→right
//! (saturation axis). Layer 3: transparent→black linear gradient
//! top→bottom (value axis, inverted so the top of the canvas is the
//! bright end). The three layers composite under standard SrcOver
//! blending to form the HSV picker's familiar square gradient.
//!
//! The current selection is shown as a double-ring indicator (white
//! outer, dark inner) so it stays visible against any underlying
//! color combination.
//!
//! # Accessibility
//!
//! The canvas is fundamentally a 2D pointer gesture with no ARIA
//! precedent. Screen-reader users navigate the picker via the hue /
//! saturation / value sliders or RGB / HSV / hex spinners — the
//! containing `ColorPicker` excludes this widget's descendants from the
//! AT tree via `.access_subtree(AccessSubtreeMode::Exclude)` (it has
//! none). The widget's own node still emits normally: a named
//! `Role::Group` carrying the saturation/brightness value as text plus
//! four custom actions (one per `CanvasStep`), so the control stays
//! reachable and drivable by assistive technology rather than being a
//! placeholder the walker prunes.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::paint::GradientStop;
use teksilo_canvas::{Canvas, Paint, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::Role;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use teksilo_core::gesture::DragPhase;
use teksilo_core::pointer::touch_action::TouchAction;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{
    CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, lit};
use teksilo_tokens::{Color, CornerRadius};

/// One step of the 2-D value, in the four directions the arrows name.
///
/// A saturation-and-brightness field is the one control in the picker whose
/// value is a *pair*, so `Action::Increment` cannot serve it: an assistive
/// client asking to "increment" would not be saying which axis. Four named
/// custom actions do say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CanvasStep {
    SaturationUp,
    SaturationDown,
    BrightnessUp,
    BrightnessDown,
}

/// One arrow press, as a fraction of the axis.
const FINE_STEP: f32 = 0.01;

/// One arrow press with `Shift` held.
const COARSE_STEP: f32 = 0.10;

impl CanvasStep {
    const ALL: [CanvasStep; 4] = [
        Self::SaturationDown,
        Self::SaturationUp,
        Self::BrightnessDown,
        Self::BrightnessUp,
    ];

    /// The arrow that means this step.
    ///
    /// Deliberately **not** mirrored under RTL, unlike `Slider`: the canvas
    /// paints its saturation gradient left-to-right in every direction, so an
    /// arrow that mirrored would disagree with the picture the user is looking
    /// at. A slider mirrors because its paint does.
    fn from_key(key: Key) -> Option<Self> {
        match key {
            Key::ArrowRight => Some(Self::SaturationUp),
            Key::ArrowLeft => Some(Self::SaturationDown),
            Key::ArrowUp => Some(Self::BrightnessUp),
            Key::ArrowDown => Some(Self::BrightnessDown),
            _ => None,
        }
    }

    fn label(self) -> LocalizedString {
        match self {
            Self::SaturationUp => lit!("Increase saturation"),
            Self::SaturationDown => lit!("Decrease saturation"),
            Self::BrightnessUp => lit!("Increase brightness"),
            Self::BrightnessDown => lit!("Decrease brightness"),
        }
    }
}

pub(crate) struct HsvCanvas {
    hue: Signal<f32>,
    saturation: Signal<f32>,
    value_hsv: Signal<f32>,
    set_hsv: Rc<dyn Fn(f32, f32, f32)>,
    dragging: Rc<Cell<bool>>,
    cached_bounds: Rc<Cell<Rect>>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
}

impl HsvCanvas {
    pub(crate) fn new(
        hue: Signal<f32>,
        saturation: Signal<f32>,
        value_hsv: Signal<f32>,
        set_hsv: Rc<dyn Fn(f32, f32, f32)>,
        dragging: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            hue,
            saturation,
            value_hsv,
            set_hsv,
            dragging,
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            initial_enabled: true,
        }
    }

    /// Set the initial enabled state. Forwarded to the arena at build time.
    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }
}

impl std::fmt::Debug for HsvCanvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HsvCanvas").finish_non_exhaustive()
    }
}

impl Widget for HsvCanvas {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let registry = ctx.binding_registry();
        // Bind for repaint when any of the HSV channels move; layout
        // is fixed, so RepaintOnly is the right level.
        self.hue.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.saturation.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.value_hsv.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );

        // Framework gates events on `arena.is_enabled(self_id)`; no
        // per-handler enabled snapshot.
        let cached_bounds = self.cached_bounds.clone();
        let dragging = self.dragging.clone();
        let set_hsv = self.set_hsv.clone();
        let hue_for_drag = self.hue.clone();

        let apply: Rc<dyn Fn(f32, f32)> = {
            let set_hsv = set_hsv.clone();
            Rc::new(move |x: f32, y: f32| {
                let bounds = cached_bounds.get();
                if bounds.width <= 0.0 || bounds.height <= 0.0 {
                    return;
                }
                // `x` / `y` arrive widget-local (origin at the canvas's own
                // top-left), so no `bounds.x` / `bounds.y` subtraction.
                let sat = (x / bounds.width).clamp(0.0, 1.0);
                // Visual convention: top = high value (bright), bottom = low value.
                let val = (1.0 - y / bounds.height).clamp(0.0, 1.0);
                (set_hsv)(hue_for_drag.get(), sat, val);
            })
        };

        let mut handlers = HandlerSet::new()
            // Focusable, because the 2-D drag is the only way to reach a
            // saturation-and-brightness pair on this control and WCAG 2.2
            // SC 2.5.7 wants one that is not a drag. The arrows below are that
            // route; the numeric entry beside the canvas is the other.
            .focusable(true)
            .cursor(CursorIcon::Crosshair)
            // A continuous manipulator: the value it produces IS the press
            // position, so no default touch behaviour may be run on it. `NONE`
            // freezes the hit path's touch action at the press
            // (`docs/touch-and-pen.md` §7.3); what it forbids, with a test on
            // it, is a two-contact pinch started on this control reaching the
            // surface underneath. The press capture the drag takes is what
            // separately keeps a scroller the picker sits in from taking the
            // gesture away.
            .touch_action(TouchAction::NONE);

        // The non-drag route: the arrows step saturation and brightness, Shift
        // steps by ten times as much, and the same four steps are advertised as
        // AccessKit custom actions for a client with no keyboard either. All of
        // them go through one closure, so the routes cannot disagree, and each
        // one announces the pair it produced — a screen-reader user pressing an
        // arrow on a colour field otherwise hears nothing at all.
        let step_hsv: Rc<dyn Fn(CanvasStep, bool, &mut teksilo_core::widget::EventContext)> = {
            let set_hsv = set_hsv.clone();
            let hue = self.hue.clone();
            let saturation = self.saturation.clone();
            let value_hsv = self.value_hsv.clone();
            Rc::new(move |step, coarse, ctx| {
                let delta = if coarse { COARSE_STEP } else { FINE_STEP };
                let (mut sat, mut val) = (
                    saturation.get().clamp(0.0, 1.0),
                    value_hsv.get().clamp(0.0, 1.0),
                );
                match step {
                    CanvasStep::SaturationUp => sat = (sat + delta).min(1.0),
                    CanvasStep::SaturationDown => sat = (sat - delta).max(0.0),
                    CanvasStep::BrightnessUp => val = (val + delta).min(1.0),
                    CanvasStep::BrightnessDown => val = (val - delta).max(0.0),
                }
                (set_hsv)(hue.get(), sat, val);
                ctx.announce(format!(
                    "Saturation {}%, brightness {}%",
                    (sat * 100.0).round() as i32,
                    (val * 100.0).round() as i32
                ));
            })
        };

        {
            let step_hsv = step_hsv.clone();
            handlers = handlers.on_key(move |event, ctx| {
                if let WidgetEvent::KeyDown { key, modifiers, .. } = event
                    && let Some(step) = CanvasStep::from_key(*key)
                {
                    step_hsv(step, modifiers.shift(), ctx);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        }
        for step in CanvasStep::ALL {
            let step_hsv = step_hsv.clone();
            handlers =
                handlers.access_custom_action(step.label(), move |ctx| step_hsv(step, false, ctx));
        }

        {
            let dragging = dragging.clone();
            let apply = apply.clone();
            handlers = handlers.on_drag(move |phase, _ctx| match phase {
                DragPhase::Started {
                    position,
                    button: PointerButton::Primary,
                    ..
                } => {
                    dragging.set(true);
                    apply(position.x, position.y);
                }
                DragPhase::Moved { position, .. } if dragging.get() => {
                    apply(position.x, position.y);
                }
                DragPhase::Ended { .. } => {
                    dragging.set(false);
                }
                _ => {}
            });
        }
        {
            let apply = apply.clone();
            handlers = handlers.on_tap(move |event, _ctx| {
                apply(event.position.x, event.position.y);
            });
        }

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        use crate::styles::recipe_color_picker_style as cp;
        Size::new(cp::CANVAS_WIDTH, cp::CANVAS_HEIGHT).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.cached_bounds.set(bounds);
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        use crate::styles::recipe_color_picker_style as cp;
        self.cached_bounds.set(bounds);
        let radius = CornerRadius::uniform(cp::CANVAS_CORNER_RADIUS);

        // Layer 1: solid base — the pure hue at full saturation + value.
        let hue = self.hue.get();
        let base = Color::from_hsv(hue, 1.0, 1.0);
        canvas.fill_rounded_rect(bounds, radius, base);

        // Layer 2: white → transparent (saturation axis, left → right).
        // `Paint::LinearGradient` endpoints are rect-local (origin at
        // the rect's top-left, in pixels); see `Paint::LinearGradient`
        // docs for the rationale.
        canvas.fill_rounded_rect(
            bounds,
            radius,
            Paint::LinearGradient {
                start: Point::new(0.0, 0.0),
                end: Point::new(bounds.width, 0.0),
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Color::WHITE,
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Color::WHITE.with_alpha(0.0),
                    },
                ],
            },
        );

        // Layer 3: transparent → black (value axis, top → bottom).
        canvas.fill_rounded_rect(
            bounds,
            radius,
            Paint::LinearGradient {
                start: Point::new(0.0, 0.0),
                end: Point::new(0.0, bounds.height),
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Color::BLACK.with_alpha(0.0),
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Color::BLACK,
                    },
                ],
            },
        );

        // Indicator — double ring (white outer, dark inner) for legibility on any background.
        let sat = self.saturation.get().clamp(0.0, 1.0);
        let val = self.value_hsv.get().clamp(0.0, 1.0);
        let cx = bounds.x + bounds.width * sat;
        let cy = bounds.y + bounds.height * (1.0 - val);
        let center = Point::new(cx, cy);

        canvas.stroke_circle(
            center,
            cp::INDICATOR_RADIUS + cp::INDICATOR_INNER_STROKE_WIDTH,
            cp::INDICATOR_OUTER_COLOR,
            cp::INDICATOR_OUTER_STROKE_WIDTH,
        );
        canvas.stroke_circle(
            center,
            cp::INDICATOR_RADIUS,
            cp::INDICATOR_INNER_COLOR,
            cp::INDICATOR_INNER_STROKE_WIDTH,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A 2-D value has no ARIA role — there is no `slider2d` — so the node is
        // a named group carrying the four steps as custom actions. It is NOT a
        // `GenericContainer` with no properties: the walker prunes those, and a
        // pruned node advertises nothing, which is what left this control
        // undriveable by assistive technology.
        //
        // The value is on the node as text rather than as a number, because
        // `numeric_value` holds one number and this control has two.
        builder.set_role(Role::Group);
        builder.set_name(lit!("Saturation and brightness").resolve_now());
        builder.set_value(format!(
            "Saturation {}%, brightness {}%",
            (self.saturation.get().clamp(0.0, 1.0) * 100.0).round() as i32,
            (self.value_hsv.get().clamp(0.0, 1.0) * 100.0).round() as i32
        ));
    }
}
