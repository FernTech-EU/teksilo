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
//! containing [`ColorPicker`] excludes this widget's subtree from the
//! AT tree via `.access_exclude_subtree()`. This widget itself emits a
//! `Role::GenericContainer` placeholder so the override has something
//! to prune.

use std::cell::Cell;
use std::rc::Rc;

use bastyde_canvas::paint::GradientStop;
use bastyde_canvas::{Canvas, Paint, Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::Role;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::PointerButton;
use bastyde_core::gesture::DragPhase;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{Color, CornerRadius};

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
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        self.saturation.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
        );
        self.value_hsv.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::RepaintOnly,
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
                let sat = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
                // Visual convention: top = high value (bright), bottom = low value.
                let val = (1.0 - (y - bounds.y) / bounds.height).clamp(0.0, 1.0);
                (set_hsv)(hue_for_drag.get(), sat, val);
            })
        };

        let mut handlers = HandlerSet::new()
            .focusable(false)
            .cursor(CursorIcon::Crosshair);

        {
            let dragging = dragging.clone();
            let apply = apply.clone();
            handlers = handlers.on_drag(move |phase, _ctx| match phase {
                DragPhase::Started {
                    position,
                    button: PointerButton::Primary,
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
        // Placeholder role. The containing ColorPicker excludes this
        // node's subtree from the AT tree via `.access_exclude_subtree()`,
        // since 2D pointer gestures have no ARIA equivalent and AT users
        // rely on the H/S/V/A sliders + RGB/HSV/hex spinners instead.
        builder.set_role(Role::GenericContainer);
    }
}
