// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze radio button.
//!
//! The check box's twin: a 20 px indicator holding a 15 px disc at 2.5,
//! filled with the button colour and outlined in the separator colour.
//! Selected, the disc is outlined in the highlight and filled again with
//! the highlight at 30 %, and a 6 px dot of the view's text sits at its
//! centre, growing to its size over the style's 100 ms. Pressed, the button
//! colour darkens 110 %; under the pointer a 1 px ring of the focus colour
//! lies on the outline.
//!
//! The row, the label position and the departures from Breeze are the check
//! box's: see [`crate::styles::checkbox`] (D8, the 18 px body) and
//! [`crate::styles::indicator`] (D5 and D6).
//!
//! ## Variants
//!
//! Breeze has one radio button. `Circle` is it; `Square` and `Rounded`
//! (`[unpublished]`) paint the disc as the check box's box at its 4 px and
//! the check box's `Rounded` 6 px radius, with the same dot.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{CheckboxVariant, RadioStyle, RadioStyleConfig, RadioVariant};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::CornerRadius;
use teksilo_widgets::styles::recipe_radio_style::RADIO_LABEL_GAP;

use crate::styles::checkbox::{box_paint, indicator_focus_frame, paint_box, variant_radius};
use crate::styles::indicator::{BreezeIndicatorColors, indicator_rect, stroked_box_rect};
use crate::styles::metrics::{INDICATOR_SIZE, RADIO_DOT_RADIUS, indicator_body_width};

/// Breeze `RadioStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeRadioStyle;

impl RadioStyle for BreezeRadioStyle {
    fn make_body(&self, cfg: &RadioStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let progress = ctx.animated_signal(if cfg.is_selected.get() { 1.0 } else { 0.0 });
        // `[S]` Breeze's style transitions: 100 ms, linear.
        let grow = ctx.animate().fast().standard();
        {
            let progress = progress.clone();
            ctx.effect(&cfg.is_selected, move |on| {
                grow.to_or_snap(&progress, if *on { 1.0 } else { 0.0 });
            });
        }
        let show_focus_frame = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);
        ctx.add(BreezeRadioBody {
            selected: cfg.is_selected.clone(),
            progress,
            is_hovered: cfg.is_hovered.clone(),
            is_pressed: cfg.is_pressed.clone(),
            is_disabled: cfg.is_disabled.clone(),
            show_focus_frame,
            variant: cfg.variant,
        })
    }
}

struct BreezeRadioBody {
    selected: Signal<bool>,
    /// How far the dot has grown, 0 to 1.
    progress: Signal<f32>,
    is_hovered: Signal<bool>,
    is_pressed: Signal<bool>,
    is_disabled: Signal<bool>,
    show_focus_frame: Signal<bool>,
    variant: RadioVariant,
}

impl std::fmt::Debug for BreezeRadioBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeRadioBody")
            .field("variant", &self.variant)
            .finish()
    }
}

/// The radius the disc of `variant` is stroked with.
pub fn variant_radius_of(variant: RadioVariant) -> f32 {
    match variant {
        RadioVariant::Circle => variant_radius(CheckboxVariant::Circle),
        RadioVariant::Square => variant_radius(CheckboxVariant::Square),
        RadioVariant::Rounded => variant_radius(CheckboxVariant::Rounded),
    }
}

/// `[Hp]` The dot's radius `progress` of the way to its full size.
pub fn dot_radius(progress: f32) -> f32 {
    RADIO_DOT_RADIUS * progress.clamp(0.0, 1.0)
}

impl Widget for BreezeRadioBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.progress
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        for s in [
            &self.selected,
            &self.is_hovered,
            &self.is_pressed,
            &self.is_disabled,
            &self.show_focus_frame,
        ] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        vec![]
    }

    /// The check box's 18 px body; the widget raises the row (D8).
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(indicator_body_width(RADIO_LABEL_GAP), INDICATOR_SIZE).into()
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
        let selected = self.selected.get();
        let disabled = self.is_disabled.get();
        let colors = BreezeIndicatorColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            disabled,
        );
        let indicator = indicator_rect(bounds, ctx.layout_direction);
        let pen = ctx.theme.shape.border_width;
        let radius = variant_radius_of(self.variant);
        let paint = box_paint(
            selected,
            self.is_hovered.get(),
            self.is_pressed.get(),
            disabled,
            &colors,
        );
        paint_box(
            canvas,
            stroked_box_rect(indicator, pen),
            radius,
            pen,
            &paint,
        );

        let r = if selected {
            dot_radius(self.progress.get())
        } else {
            0.0
        };
        if r > 0.0 && colors.text.a() > 0.0 {
            let centre = Point::new(
                indicator.x + INDICATOR_SIZE * 0.5,
                indicator.y + INDICATOR_SIZE * 0.5,
            );
            canvas.fill_circle(centre, r, colors.text);
        }

        if self.show_focus_frame.get() {
            let shape = &ctx.theme.shape;
            if colors.focus_ring.a() > 0.0 && shape.focus_ring_width > 0.0 {
                // The disc's frame is a circle: the box's radius is half of
                // it, which the ring grows with.
                let box_radius = if self.variant == RadioVariant::Circle {
                    (INDICATOR_SIZE * 0.5) - crate::styles::metrics::INDICATOR_BOX_INSET
                } else {
                    radius
                };
                let ring = indicator_focus_frame(indicator, box_radius, shape.focus_ring_width);
                canvas.stroke_rounded_rect(
                    ring.rect,
                    CornerRadius::uniform(ring.corner_radius),
                    colors.focus_ring,
                    shape.focus_ring_width,
                );
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the `RadioButton` emits the radio node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_is_six_pixels_and_grows_with_the_progress() {
        assert_eq!(dot_radius(1.0) * 2.0, 6.0);
        assert_eq!(dot_radius(0.0), 0.0);
        assert_eq!(dot_radius(0.5), 1.5);
        assert_eq!(dot_radius(2.0), 3.0, "clamped");
    }

    #[test]
    fn the_circle_is_breezes_disc() {
        assert_eq!(variant_radius_of(RadioVariant::Circle), 7.5);
        assert_eq!(variant_radius_of(RadioVariant::Square), 4.0);
        assert!(variant_radius_of(RadioVariant::Rounded) > 4.0);
    }

    #[test]
    fn the_focus_frame_of_the_disc_is_a_circle_outside_it() {
        let i = Rect::new(0.0, 0.0, 20.0, 20.0);
        let ring = indicator_focus_frame(i, 8.0, 2.0);
        assert_eq!(ring.rect, Rect::new(1.0, 1.0, 18.0, 18.0));
        assert_eq!(ring.corner_radius, 9.0, "half its side: a circle");
    }
}
