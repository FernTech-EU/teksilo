//! `HueStrip` — 1D hue slider rendered from a CPU-generated 256×1
//! rainbow texture.
//!
//! The SDF gradient pipeline caps each `Paint::LinearGradient` at four
//! stops. A perceptually smooth full-spectrum hue strip needs at least
//! seven (red → yellow → green → cyan → blue → magenta → red), so we
//! generate a 256×1 RGBA texture once via
//! `Canvas::ensure_image_registered` (idempotent — keyed by name) and
//! draw it via `draw_image`. The texture costs ~1 KB and is shared
//! across every `ColorPicker` / `ColorEdit` instance in the process.
//!
//! Accessibility is `Role::Slider` with `numeric_value=hue`, range
//! `0..360`, step 1°, jump 15° (PageUp/Down).

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::LazyLock;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
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

/// One texture per orientation — `draw_image` stretches without
/// rotation, so a horizontal texture into a vertical strip would
/// run the gradient across the strip's short axis. Picking by
/// orientation keeps the rainbow oriented along the strip's long
/// axis without paint-time rotation.
const HUE_TEXTURE_NAME_HORIZONTAL: &str = "__fern_color_picker_hue_h_256";
const HUE_TEXTURE_NAME_VERTICAL: &str = "__fern_color_picker_hue_v_256";
const HUE_TEXTURE_LENGTH: u32 = 256;

/// 256-pixel rainbow generated once per orientation. Pixel order in
/// the vertical buffer matches scan-line order: column 0, rows 0..256
/// — `draw_image` reads row-major so a single column of 256 rows is a
/// 1×256 RGBA buffer.
static HUE_PIXELS: LazyLock<Vec<u8>> = LazyLock::new(generate_hue_pixels);

pub(crate) struct HueStrip {
    hue: Signal<f32>,
    set_hue: Rc<dyn Fn(f32)>,
    dragging: Rc<Cell<bool>>,
    cached_bounds: Rc<Cell<Rect>>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    orientation: Orientation,
    enabled: bool,
    label: String,
}

impl HueStrip {
    pub(crate) fn new(
        hue: Signal<f32>,
        set_hue: Rc<dyn Fn(f32)>,
        dragging: Rc<Cell<bool>>,
    ) -> Self {
        Self {
            hue,
            set_hue,
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

impl std::fmt::Debug for HueStrip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HueStrip")
            .field("orientation", &self.orientation)
            .finish_non_exhaustive()
    }
}

impl Widget for HueStrip {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.hue.bind_to(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
        );

        let enabled = self.enabled;
        let cached_bounds = self.cached_bounds.clone();
        let dragging = self.dragging.clone();
        let set_hue = self.set_hue.clone();
        let orientation = self.orientation;

        let apply: Rc<dyn Fn(f32, f32)> = {
            let set_hue = set_hue.clone();
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
                // Map 0..1 to 0..360 — clamp to 359.999 so wrap doesn't flip
                // an end-of-strip click back to red-at-the-start.
                let h = (t * 360.0).min(359.999);
                (set_hue)(h);
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
            handlers = handlers.on_tap(move |event, _ctx| {
                if !enabled {
                    return;
                }
                apply(event.position.x, event.position.y);
            });
        }

        // Keyboard.
        {
            let set_hue = set_hue.clone();
            let hue = self.hue.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                match key {
                    Key::ArrowUp | Key::ArrowRight => {
                        let next = (hue.get() + 1.0).rem_euclid(360.0);
                        (set_hue)(next);
                        EventResponse::Handled
                    }
                    Key::ArrowDown | Key::ArrowLeft => {
                        let next = (hue.get() - 1.0).rem_euclid(360.0);
                        (set_hue)(next);
                        EventResponse::Handled
                    }
                    Key::PageUp => {
                        let next = (hue.get() + 15.0).rem_euclid(360.0);
                        (set_hue)(next);
                        EventResponse::Handled
                    }
                    Key::PageDown => {
                        let next = (hue.get() - 15.0).rem_euclid(360.0);
                        (set_hue)(next);
                        EventResponse::Handled
                    }
                    Key::Home => {
                        (set_hue)(0.0);
                        EventResponse::Handled
                    }
                    Key::End => {
                        (set_hue)(359.0);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        {
            let focus_origin = self.focus_origin.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focus_origin.set(if gained {
                    Some(FocusOrigin::Keyboard)
                } else {
                    None
                });
            });
        }

        // AccessKit Increment / Decrement.
        {
            let set_hue = set_hue.clone();
            let hue = self.hue.clone();
            handlers = handlers.on_access_action(move |action, _ctx| match action {
                Action::Increment => {
                    (set_hue)((hue.get() + 1.0).rem_euclid(360.0));
                    EventResponse::Handled
                }
                Action::Decrement => {
                    (set_hue)((hue.get() - 1.0).rem_euclid(360.0));
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        use crate::styles::recipe_color_picker_style as cp;
        let size = match self.orientation {
            Orientation::Vertical => Size::new(cp::STRIP_THICKNESS, cp::STRIP_LENGTH),
            Orientation::Horizontal => Size::new(cp::STRIP_LENGTH, cp::STRIP_THICKNESS),
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
        use crate::styles::recipe_color_picker_style as cp;
        self.cached_bounds.set(bounds);
        let radius = CornerRadius::uniform(cp::STRIP_CORNER_RADIUS);

        // Register the rainbow texture for the strip's orientation.
        // Same pixel data either way (256 RGBA quartets in hue order);
        // dimensions decide whether scan-line order maps onto the
        // strip's long axis horizontally or vertically.
        let (texture_name, tex_w, tex_h) = match self.orientation {
            Orientation::Horizontal => (HUE_TEXTURE_NAME_HORIZONTAL, HUE_TEXTURE_LENGTH, 1),
            Orientation::Vertical => (HUE_TEXTURE_NAME_VERTICAL, 1, HUE_TEXTURE_LENGTH),
        };
        canvas.ensure_image_registered(
            texture_name,
            tex_w,
            tex_h,
            Cow::Borrowed(HUE_PIXELS.as_slice()),
        );

        // Background frame (under the rainbow) — protects against
        // rounded-rect corner anti-aliasing leaving the picker surface
        // visible at the strip's rounded corners. Drawn first.
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.surface_main);
        canvas.draw_image(bounds, texture_name);

        // Border frame.
        canvas.stroke_rounded_rect(bounds, radius, ctx.theme.colors.border, 1.0);

        // Thumb at the current hue position.
        let t = (self.hue.get() / 360.0).clamp(0.0, 1.0);
        let thumb_w = cp::STRIP_THUMB_WIDTH;
        let thumb_h = cp::STRIP_THUMB_HEIGHT;
        let thumb_radius = CornerRadius::uniform(cp::STRIP_THUMB_CORNER_RADIUS);
        let (cx, cy) = match self.orientation {
            Orientation::Vertical => (bounds.x + bounds.width * 0.5, bounds.y + bounds.height * t),
            Orientation::Horizontal => {
                (bounds.x + bounds.width * t, bounds.y + bounds.height * 0.5)
            }
        };
        let thumb_rect = match self.orientation {
            Orientation::Vertical => Rect::new(
                bounds.x - 2.0,
                cy - thumb_h * 0.5,
                bounds.width + 4.0,
                thumb_h,
            ),
            Orientation::Horizontal => Rect::new(
                cx - thumb_w * 0.5,
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
        builder.set_numeric_value(self.hue.get() as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(360.0);
        builder.set_numeric_value_step(1.0);
        builder.set_numeric_value_jump(15.0);
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

/// Generate a 256×1 RGBA rainbow texture. Each pixel `i` is the color
/// `Color::from_hsv(i/256·360, 1, 1)` packed as four `u8`s.
fn generate_hue_pixels() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(HUE_TEXTURE_LENGTH as usize * 4);
    for i in 0..HUE_TEXTURE_LENGTH {
        let h = (i as f32 / HUE_TEXTURE_LENGTH as f32) * 360.0;
        let c = Color::from_hsv(h, 1.0, 1.0);
        pixels.push((c.r() * 255.0) as u8);
        pixels.push((c.g() * 255.0) as u8);
        pixels.push((c.b() * 255.0) as u8);
        pixels.push(255);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_pixels_are_rainbow() {
        let pixels = generate_hue_pixels();
        assert_eq!(pixels.len(), HUE_TEXTURE_LENGTH as usize * 4);
        // First pixel is red.
        assert_eq!(pixels[0], 255);
        assert_eq!(pixels[1], 0);
        assert_eq!(pixels[2], 0);
        // Middle pixel ≈ cyan.
        let mid = (HUE_TEXTURE_LENGTH as usize / 2) * 4;
        assert!(pixels[mid] < 50);
        assert!(pixels[mid + 1] > 200);
        assert!(pixels[mid + 2] > 200);
    }
}
