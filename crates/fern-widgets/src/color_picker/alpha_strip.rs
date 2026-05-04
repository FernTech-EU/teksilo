//! `AlphaStrip` — 1D opacity slider with a checkerboard background +
//! tinted gradient overlay.
//!
//! Background: a checkerboard pattern in two neutral grays (so the
//! transparency reads correctly against any underlying surface). The
//! foreground is a `Paint::LinearGradient` fading from
//! `current_color.with_alpha(0.0)` to `current_color.with_alpha(1.0)`
//! along the strip's primary axis. Vertical orientation reads
//! transparent at the top (alpha=0) and opaque at the bottom (alpha=1);
//! horizontal reads transparent on the leading edge.
//!
//! Accessibility is `Role::Slider`, name "Opacity", `numeric_value =
//! alpha × 100`, range `0..100`, step 1, jump 10.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Paint, Point, Rect, Size, SizeProposal};
use fern_canvas::paint::GradientStop;
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::gesture::DragPhase;
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{Color, CornerRadius, Orientation};

pub(crate) struct AlphaStrip {
    /// Current bound color — for the foreground gradient color.
    current_color: Signal<Color>,
    alpha: Signal<f32>,
    set_alpha: Rc<dyn Fn(f32)>,
    dragging: Rc<Cell<bool>>,
    cached_bounds: Rc<Cell<Rect>>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    orientation: Orientation,
    enabled: bool,
    label: String,
}

impl AlphaStrip {
    pub(crate) fn new(
        current_color: Signal<Color>,
        alpha: Signal<f32>,
        set_alpha: Rc<dyn Fn(f32)>,
        dragging: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            current_color,
            alpha,
            set_alpha,
            dragging,
            cached_bounds: Rc::new(Cell::new(Rect::ZERO)),
            focus_origin: Rc::new(Cell::new(None)),
            orientation: Orientation::Vertical,
            enabled: true,
            label: String::new(),
        }
    }

    pub(crate) fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub(crate) fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
}

impl std::fmt::Debug for AlphaStrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlphaStrip").finish_non_exhaustive()
    }
}

impl Widget for AlphaStrip {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Color drives the gradient colour; alpha drives the thumb position.
        self.current_color
            .bind_to(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);
        self.alpha
            .bind_to(self_id, registry, fern_core::binding::BindingLevel::RepaintOnly);

        let enabled = self.enabled;
        let cached_bounds = self.cached_bounds.clone();
        let dragging = self.dragging.clone();
        let set_alpha = self.set_alpha.clone();
        let orientation = self.orientation;

        let apply: Rc<dyn Fn(f32, f32)> = {
            let set_alpha = set_alpha.clone();
            Rc::new(move |x: f32, y: f32| {
            let bounds = cached_bounds.get();
            let t = match orientation {
                Orientation::Vertical => {
                    if bounds.height <= 0.0 {
                        return;
                    }
                    ((y - bounds.y) / bounds.height).clamp(0.0, 1.0)
                }
                Orientation::Horizontal => {
                    if bounds.width <= 0.0 {
                        return;
                    }
                    ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
                }
            };
            // Visual: top/leading = transparent (alpha 0), bottom/trailing = opaque (alpha 1).
            (set_alpha)(t.clamp(0.0, 1.0));
        })
        };

        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        {
            let dragging = dragging.clone();
            let apply = apply.clone();
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
                        apply(position.x, position.y);
                    }
                    DragPhase::Moved { position, .. } if dragging.get() => {
                        apply(position.x, position.y);
                    }
                    DragPhase::Ended { .. } => {
                        dragging.set(false);
                    }
                    _ => {}
                }
            });
        }
        {
            let apply = apply.clone();
            handlers = handlers.on_tap(move |position, _ctx| {
                if !enabled {
                    return;
                }
                apply(position.x, position.y);
            });
        }

        // Keyboard.
        {
            let set_alpha = set_alpha.clone();
            let alpha = self.alpha.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                match key {
                    Key::ArrowUp | Key::ArrowRight => {
                        (set_alpha)((alpha.get() + 0.01).clamp(0.0, 1.0));
                        EventResponse::Handled
                    }
                    Key::ArrowDown | Key::ArrowLeft => {
                        (set_alpha)((alpha.get() - 0.01).clamp(0.0, 1.0));
                        EventResponse::Handled
                    }
                    Key::PageUp => {
                        (set_alpha)((alpha.get() + 0.10).clamp(0.0, 1.0));
                        EventResponse::Handled
                    }
                    Key::PageDown => {
                        (set_alpha)((alpha.get() - 0.10).clamp(0.0, 1.0));
                        EventResponse::Handled
                    }
                    Key::Home => {
                        (set_alpha)(0.0);
                        EventResponse::Handled
                    }
                    Key::End => {
                        (set_alpha)(1.0);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        {
            let focus_origin = self.focus_origin.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focus_origin.set(if gained { Some(FocusOrigin::Keyboard) } else { None });
            });
        }

        {
            let set_alpha = set_alpha.clone();
            let alpha = self.alpha.clone();
            handlers = handlers.on_access_action(move |action, _ctx| match action {
                Action::Increment => {
                    (set_alpha)((alpha.get() + 0.01).clamp(0.0, 1.0));
                    EventResponse::Handled
                }
                Action::Decrement => {
                    (set_alpha)((alpha.get() - 0.01).clamp(0.0, 1.0));
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let style = ctx.theme.components.color_picker;
        let size = match self.orientation {
            Orientation::Vertical => Size::new(style.strip_thickness, style.strip_length),
            Orientation::Horizontal => Size::new(style.strip_length, style.strip_thickness),
        };
        size.into()
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

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        self.cached_bounds.set(bounds);
        let style = ctx.theme.components.color_picker;
        let radius = CornerRadius::uniform(style.strip_corner_radius);

        // Checkerboard background — many small fill_rect calls. For a
        // 14×192 strip with 6 px cells that's ~64 calls per paint, well
        // within the Tier-1 budget. Drawn first; the gradient overlay
        // handles transparency.
        paint_checkerboard(
            canvas,
            bounds,
            style.checker_cell,
            style.checker_color_a,
            style.checker_color_b,
        );

        // Gradient overlay — current color from transparent to opaque
        // along the primary axis.
        let opaque = self.current_color.get().with_alpha(1.0);
        let transparent = opaque.with_alpha(0.0);
        let (start, end) = match self.orientation {
            Orientation::Vertical => (
                Point::new(bounds.x, bounds.y),
                Point::new(bounds.x, bounds.y + bounds.height),
            ),
            Orientation::Horizontal => (
                Point::new(bounds.x, bounds.y),
                Point::new(bounds.x + bounds.width, bounds.y),
            ),
        };
        canvas.fill_rounded_rect(
            bounds,
            radius,
            Paint::LinearGradient {
                start,
                end,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: transparent,
                    },
                    GradientStop {
                        offset: 1.0,
                        color: opaque,
                    },
                ],
            },
        );

        // Border frame.
        canvas.stroke_rounded_rect(bounds, radius, ctx.theme.colors.border, 1.0);

        // Thumb.
        let t = self.alpha.get().clamp(0.0, 1.0);
        let thumb_w = style.strip_thumb_width;
        let thumb_h = style.strip_thumb_height;
        let thumb_radius = CornerRadius::uniform(style.strip_thumb_corner_radius);
        let thumb_rect = match self.orientation {
            Orientation::Vertical => Rect::new(
                bounds.x - 2.0,
                bounds.y + bounds.height * t - thumb_h * 0.5,
                bounds.width + 4.0,
                thumb_h,
            ),
            Orientation::Horizontal => Rect::new(
                bounds.x + bounds.width * t - thumb_w * 0.5,
                bounds.y - 2.0,
                thumb_w,
                bounds.height + 4.0,
            ),
        };
        canvas.fill_rounded_rect(thumb_rect, thumb_radius, Color::WHITE);
        canvas.stroke_rounded_rect(
            thumb_rect,
            thumb_radius,
            Color::new(0.0, 0.0, 0.0, 0.5),
            1.0,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Slider);
        if !self.label.is_empty() {
            builder.set_name(&self.label);
        }
        builder.set_numeric_value((self.alpha.get() * 100.0) as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(100.0);
        builder.set_numeric_value_step(1.0);
        builder.set_numeric_value_jump(10.0);
        let orientation = match self.orientation {
            Orientation::Vertical => fern_core::accesskit::Orientation::Vertical,
            Orientation::Horizontal => fern_core::accesskit::Orientation::Horizontal,
        };
        builder.set_orientation(orientation);
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(Action::Increment);
        builder.add_action(Action::Decrement);
        builder.add_action(Action::Focus);
    }
}

/// Tile a two-color checkerboard inside `bounds` using `cell`-pixel
/// squares. Handles partial trailing cells so the pattern reads
/// correctly at non-integer multiples of `cell`.
pub(crate) fn paint_checkerboard(
    canvas: &mut Canvas,
    bounds: Rect,
    cell: f32,
    color_a: Color,
    color_b: Color,
) {
    if cell <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }
    let cols = (bounds.width / cell).ceil() as i32;
    let rows = (bounds.height / cell).ceil() as i32;
    for row in 0..rows {
        for col in 0..cols {
            let dark = (row + col) & 1 == 1;
            let color = if dark { color_a } else { color_b };
            let x = bounds.x + col as f32 * cell;
            let y = bounds.y + row as f32 * cell;
            let w = (bounds.x + bounds.width - x).min(cell);
            let h = (bounds.y + bounds.height - y).min(cell);
            if w > 0.0 && h > 0.0 {
                canvas.fill_rect(Rect::new(x, y, w, h), color);
            }
        }
    }
}
