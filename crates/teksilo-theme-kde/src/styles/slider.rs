// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze slider.
//!
//! Breeze's slider is 20 px thick. Its **groove** is 6 px, centred, as long
//! as the slider, filled and stroked on itself inset by half the pen at a
//! 3 px radius: the window text at 14 % over the window, outlined in the
//! window text at the frame contrast, and from the start up to the handle's
//! centre the highlight at 70 % over the window, outlined in the highlight at
//! the frame contrast. The start is the leading end of a horizontal groove
//! and the top of a vertical one, as Breeze fills a slider that is not
//! inverted. A `Slider` puts a vertical slider's minimum at the bottom, as
//! Qt does, so a vertical groove is filled on its maximum side, from the top
//! down to the handle. Its **handle** is a 20 px box, centred across the
//! groove and placed along it by the value over the length less 20, painted
//! as a 17 px circle (the box inset by 1, then stroked) in the button colour
//! with a 1 px outline: the slider outline at rest, the hover colour under
//! the pointer, the focus colour while focused. A 1 px ring of the shadow
//! colour, black at 12.5 %, shows at the handle's bottom right, unless the
//! handle is being dragged. A disabled slider shows only the groove's
//! background part, in the disabled colours.
//!
//! **Tick marks**, for a slider given a tick count, are 1 px lines 8 px long,
//! 2 px from the groove; they take the slider 3 px further, 23 px thick, and
//! are drawn in the highlight up to the value and in the separator colour
//! after it. Breeze draws them on the side the application asks for; a
//! `Slider` names no side, so they are below a horizontal slider and to the
//! right of a vertical one (`[unpublished]`), Qt's ticks below.
//!
//! Right to left, a horizontal slider runs from the right: the handle, the
//! fill and the ticks are mirrored, as the `Slider` mirrors its pointer and
//! arrow keys.
//!
//! The handle box is the knob the `Slider` maps the pointer with
//! ([`SliderStyle::thumb_diameter_for`]): 20 px at every density, the
//! distance Breeze divides the value over. The node is raised to the
//! density's target size across the groove (24 px at Compact), with the
//! 20 or 23 px of the slider centred in it; along the groove it is 84 px
//! when nothing proposes a length, Qt's size hint.
//!
//! ## Where this departs from Breeze
//!
//! - **D6, the handle's outline.** The slider outline, the button colour
//!   moved toward the button text by the frame contrast, is 1.3:1 on the
//!   Breeze Light window, and the groove is no further from the window; the
//!   handle is the one part of the slider that marks where it is and what it
//!   is set to, so its outline at rest is `border_strong`, lifted to SC
//!   1.4.11's 3:1 (D6 of the colour mapping). A scheme whose outline passes
//!   keeps it. The groove keeps its colours.
//! - **D5, focus.** The focused outline is `border_focused`, the focus
//!   colour lifted to 3:1, and keyboard focus adds the 2 px focus frame
//!   around the handle in the opaque `focus_ring` token, from 1 px inside
//!   the box to 1 px outside it, both edges circles.
//! - **D6, the tick marks.** The separator colour is 1.5:1 on the Breeze
//!   Light window and Breeze Light's highlight 2.2:1, and a tick marks a
//!   value the slider can take, so the ticks after the value are
//!   `border_strong` and those up to it the highlight lifted to 3:1, as a
//!   checked switch's outline ([`crate::styles::indicator`]).
//! - **Hover is the slider's.** Breeze outlines the handle in the hover
//!   colour while the pointer is over the handle; the style is told only
//!   whether the pointer is over the slider, so the outline turns while the
//!   pointer is anywhere on it.
//! - **The node is the target size.** Breeze's 20 px slider is under WCAG
//!   2.2 SC 2.5.8's 24 px, so the node is raised; nothing painted grows.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::density::density_min_size;
use teksilo_core::styles::{
    FocusRingStroke, SliderOrientation, SliderStyle, SliderStyleConfig, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_tokens::{Color, CornerRadius, InputTokens, TargetAxes};

use crate::geometry::{
    SLIDER_CONTROL_THICKNESS, SLIDER_GROOVE_THICKNESS, SLIDER_HANDLE_PAINTED, SLIDER_TICK_LENGTH,
    SLIDER_TICK_MARGIN,
};
use crate::palette::KdePalette;
use crate::styles::chrome::kde_mix;
use crate::styles::indicator::BreezeIndicatorColors;
use crate::styles::metrics::{
    SLIDER_DEFAULT_LENGTH, SLIDER_FILL_ALPHA, SLIDER_GROOVE_ALPHA, SLIDER_GROOVE_RADIUS,
    SLIDER_TICK_OVERHANG,
};

/// `[Hp]` How far the handle's circle lies inside its box, before it is
/// stroked.
const HANDLE_INSET: f32 = 1.0;
/// `[derived]` How far the handle's shadow ring is moved toward the bottom
/// right. Breeze draws it as an ellipse turned 45 degrees so that it shows
/// at the bottom right; the handle's own circle moved half a pixel each way
/// shows the same crescent.
const SHADOW_OFFSET: f32 = 0.5;
/// `[Hp]` The pen a tick mark is drawn with.
const TICK_PEN: f32 = 1.0;

// The painted handle is the box inset by one and then by half the pen.
const _: () = assert!(SLIDER_HANDLE_PAINTED == SLIDER_CONTROL_THICKNESS - 2.0 * HANDLE_INSET - 1.0);

/// Breeze `SliderStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeSliderStyle;

impl SliderStyle for BreezeSliderStyle {
    fn thumb_diameter(&self, _cfg: &SliderStyleConfig) -> f32 {
        SLIDER_CONTROL_THICKNESS
    }

    /// `[Hp]` The handle box at every density: Breeze places the handle by
    /// the value over the slider's length less the box, which is what the
    /// `Slider` maps the pointer with. The node, not the handle, grows to
    /// the density's target size, and a press anywhere on it moves the
    /// value.
    fn thumb_diameter_for(&self, cfg: &SliderStyleConfig, _tokens: &InputTokens) -> f32 {
        self.thumb_diameter(cfg)
    }

    fn make_body(&self, cfg: &SliderStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(BreezeSliderBody {
            value: cfg.value_normalized.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_dragging: cfg.is_dragging.clone(),
            is_disabled: cfg.is_disabled.clone(),
            focus_origin: cfg.focus_origin.clone(),
            orientation: cfg.orientation,
            tick_count: cfg.tick_count.filter(|n| *n >= 2),
        })
    }
}

/// The colours a Breeze slider is painted from, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeSliderColors {
    /// `[Hp]` The groove: the window text at 14 % over the window.
    pub groove: Color,
    /// `[Hp]` The groove's outline: the window text at the frame contrast.
    pub groove_outline: Color,
    /// `[Hp]` The groove up to the handle: the highlight at 70 % over the
    /// window.
    pub fill: Color,
    /// `[Hp]` Its outline: the highlight at the frame contrast.
    pub fill_outline: Color,
    /// `[qt]` The handle's face, the button colour.
    pub handle: Color,
    /// `[wcag]` D6: the handle's outline at rest, `border_strong`;
    /// `border_disabled` for a disabled slider.
    pub handle_outline: Color,
    /// `[Hp]` The handle's outline under the pointer: the view's hover
    /// decoration.
    pub hover: Color,
    /// `[wcag]` D5: the handle's outline while focused, `border_focused`.
    pub focus: Color,
    /// `[wcag]` D5: the keyboard focus frame, `focus_ring`.
    pub focus_ring: Color,
    /// `[Hp]` The handle's shadow, black at 12.5 %.
    pub shadow: Color,
    /// `[wcag]` D6: a tick up to the value, the highlight lifted to the
    /// non-text floor.
    pub tick_on: Color,
    /// `[wcag]` D6: a tick after the value, `border_strong`.
    pub tick_off: Color,
}

impl BreezeSliderColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference, for an enabled
    /// or a disabled slider.
    ///
    /// The window, its text and the highlight are the theme's projected
    /// tokens (the disabled palette's window and window text for a disabled
    /// slider); the button colour, the frame contrast and the hover colour
    /// come from [`KdePalette`]. Without it the button colour is the content
    /// surface, the frame contrast Breeze's 0.2 and the hover colour
    /// `border_focused`.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeSliderColors {
        let c = &theme.colors;
        let palette = theme.extension::<KdePalette>();
        let state = if disabled {
            ColorState::Disabled
        } else if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let (window, window_text) = match (disabled, palette) {
            (true, Some(p)) => (
                p.role(ColorState::Disabled, PaletteRole::Window),
                p.role(ColorState::Disabled, PaletteRole::WindowText),
            ),
            (true, None) => (c.surface_main, c.text_disabled),
            (false, _) => (c.surface_main, c.text_primary),
        };
        let highlight = if disabled {
            c.accent_disabled
        } else {
            c.accent
        };
        // `[derived]` The frame contrast: 1 in the high-contrast scheme the
        // palette builds, as `KdePalette::breeze_colors` computes it.
        let fc = match palette {
            Some(_) if high_contrast => 1.0,
            Some(p) => p.frame_contrast(),
            None => teksilo_kde_scheme::scheme::DEFAULT_FRAME_CONTRAST,
        } as f32;
        let (handle, hover, shadow) = match palette {
            Some(p) => {
                let b = p.breeze_colors(state, high_contrast);
                (b.button, b.hover, b.shadow)
            }
            None => (
                if disabled {
                    c.surface_disabled
                } else {
                    c.surface_content
                },
                c.border_focused,
                theme.shape.shadow_xs.color,
            ),
        };
        let indicator =
            BreezeIndicatorColors::resolve(theme, window_active, high_contrast, disabled);
        BreezeSliderColors {
            groove: kde_mix(window, window_text, SLIDER_GROOVE_ALPHA),
            groove_outline: window_text.with_alpha(fc),
            fill: kde_mix(window, highlight, SLIDER_FILL_ALPHA),
            fill_outline: highlight.with_alpha(fc),
            handle,
            handle_outline: if disabled {
                c.border_disabled
            } else {
                c.border_strong
            },
            hover,
            focus: c.border_focused,
            focus_ring: c.focus_ring,
            shadow,
            tick_on: indicator.checked_outline,
            tick_off: indicator.outline,
        }
    }

    /// `[Hp]` The handle's outline: disabled first, then focus, then hover,
    /// then rest.
    pub fn handle_outline_for(&self, focused: bool, hovered: bool, disabled: bool) -> Color {
        if disabled {
            self.handle_outline
        } else if focused {
            self.focus
        } else if hovered {
            self.hover
        } else {
            self.handle_outline
        }
    }
}

/// Where a Breeze slider's parts lie in its node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderGeometry {
    /// `[Hp]` The groove, 6 px thick and the node's full length, before it
    /// is inset by half the pen.
    pub groove: Rect,
    /// `[Hp]` The handle's 20 px box.
    pub handle: Rect,
    /// `[Hp]` The groove from its start to the handle's centre, before it is
    /// inset by half the pen: from the minimum's end of a horizontal groove,
    /// and from the top, the maximum's end, of a vertical one.
    pub fill: Rect,
    /// The main-axis coordinate of the handle's centre at the value 0 and at
    /// the value 1.
    pub travel: (f32, f32),
    /// The cross-axis coordinate of the groove's centre.
    pub centre: f32,
}

/// `[Hp]` The parts of a slider whose node is `bounds`, at the value
/// `value` (0 to 1), laid out `orientation`, `direction` and with or without
/// a row of tick marks.
pub fn slider_geometry(
    bounds: Rect,
    value: f32,
    orientation: SliderOrientation,
    direction: LayoutDirection,
    ticks: bool,
) -> SliderGeometry {
    let t = value.clamp(0.0, 1.0);
    let box_ = SLIDER_CONTROL_THICKNESS;
    let half = box_ * 0.5;
    let thickness = slider_thickness(ticks);
    match orientation {
        SliderOrientation::Horizontal => {
            // The slider (with its ticks below) centred across the node.
            let top = bounds.y + (bounds.height - thickness) * 0.5;
            let cy = top + half;
            let (start, end) = match direction {
                LayoutDirection::LeftToRight => (bounds.x + half, bounds.x + bounds.width - half),
                LayoutDirection::RightToLeft => (bounds.x + bounds.width - half, bounds.x + half),
            };
            let cx = start + (end - start) * t;
            let groove = Rect::new(
                bounds.x,
                cy - SLIDER_GROOVE_THICKNESS * 0.5,
                bounds.width,
                SLIDER_GROOVE_THICKNESS,
            );
            let fill = match direction {
                LayoutDirection::LeftToRight => {
                    Rect::new(groove.x, groove.y, (cx - groove.x).max(0.0), groove.height)
                }
                LayoutDirection::RightToLeft => Rect::new(
                    cx,
                    groove.y,
                    (groove.x + groove.width - cx).max(0.0),
                    groove.height,
                ),
            };
            SliderGeometry {
                groove,
                handle: Rect::new(cx - half, cy - half, box_, box_),
                fill,
                travel: (start, end),
                centre: cy,
            }
        }
        SliderOrientation::Vertical => {
            // The ticks are to the right, so the slider is centred with them.
            let left = bounds.x + (bounds.width - thickness) * 0.5;
            let cx = left + half;
            let (start, end) = (bounds.y + bounds.height - half, bounds.y + half);
            let cy = start + (end - start) * t;
            let groove = Rect::new(
                cx - SLIDER_GROOVE_THICKNESS * 0.5,
                bounds.y,
                SLIDER_GROOVE_THICKNESS,
                bounds.height,
            );
            // Breeze fills a vertical groove that is not inverted from its
            // top, and a `Slider`'s maximum is at the top.
            let fill = Rect::new(groove.x, groove.y, groove.width, (cy - groove.y).max(0.0));
            SliderGeometry {
                groove,
                handle: Rect::new(cx - half, cy - half, box_, box_),
                fill,
                travel: (start, end),
                centre: cx,
            }
        }
    }
}

/// `[Hp]` `[measured-qt]` How thick a slider is: 20, and 23 with a row of
/// tick marks.
pub fn slider_thickness(ticks: bool) -> f32 {
    if ticks {
        SLIDER_CONTROL_THICKNESS + SLIDER_TICK_OVERHANG
    } else {
        SLIDER_CONTROL_THICKNESS
    }
}

/// `[Hp]` The tick mark for the stop `stop` of `count` (from the minimum) of
/// a slider laid out as `g`: a line 8 px long, 2 px from the groove, as the
/// two points it runs between.
pub fn tick_line(
    g: &SliderGeometry,
    orientation: SliderOrientation,
    stop: u32,
    count: u32,
) -> (Point, Point) {
    let f = if count > 1 {
        stop as f32 / (count - 1) as f32
    } else {
        0.0
    };
    let (start, end) = g.travel;
    let at = start + (end - start) * f;
    let near = g.centre + SLIDER_GROOVE_THICKNESS * 0.5 + SLIDER_TICK_MARGIN;
    let far = near + SLIDER_TICK_LENGTH;
    match orientation {
        SliderOrientation::Horizontal => (Point::new(at, near), Point::new(at, far)),
        SliderOrientation::Vertical => (Point::new(near, at), Point::new(far, at)),
    }
}

/// `[Hp]` A rectangle inset by half of `pen` on every side.
fn inset_half(r: Rect, pen: f32) -> Rect {
    let d = pen * 0.5;
    Rect::new(
        r.x + d,
        r.y + d,
        (r.width - pen).max(0.0),
        (r.height - pen).max(0.0),
    )
}

struct BreezeSliderBody {
    value: Signal<f32>,
    is_hovered: Signal<bool>,
    is_dragging: Signal<bool>,
    is_disabled: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    orientation: SliderOrientation,
    tick_count: Option<u32>,
}

impl std::fmt::Debug for BreezeSliderBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeSliderBody")
            .field("orientation", &self.orientation)
            .field("tick_count", &self.tick_count)
            .finish()
    }
}

impl Widget for BreezeSliderBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.value.bind_to(id, registry, BindingLevel::RepaintOnly);
        for s in [&self.is_hovered, &self.is_dragging, &self.is_disabled] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        self.focus_origin
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    /// The proposed length (84 when none is), and the slider's thickness
    /// raised to the density's target size.
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let thickness = slider_thickness(self.tick_count.is_some());
        let (size, axes) = match self.orientation {
            SliderOrientation::Horizontal => (
                Size::new(proposal.width.unwrap_or(SLIDER_DEFAULT_LENGTH), thickness),
                TargetAxes::HEIGHT,
            ),
            SliderOrientation::Vertical => (
                Size::new(thickness, proposal.height.unwrap_or(SLIDER_DEFAULT_LENGTH)),
                TargetAxes::WIDTH,
            ),
        };
        density_min_size(size, axes, &ctx.theme.input).into()
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
        let disabled = self.is_disabled.get();
        let colors = BreezeSliderColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            disabled,
        );
        let pen = ctx.theme.shape.border_width;
        let value = self.value.get().clamp(0.0, 1.0);
        let g = slider_geometry(
            bounds,
            value,
            self.orientation,
            ctx.layout_direction,
            self.tick_count.is_some(),
        );
        let corner = CornerRadius::uniform(SLIDER_GROOVE_RADIUS);

        // The groove, then its part up to the handle.
        let groove = inset_half(g.groove, pen);
        canvas.fill_rounded_rect(groove, corner, colors.groove);
        if pen > 0.0 && colors.groove_outline.a() > 0.0 {
            canvas.stroke_rounded_rect(groove, corner, colors.groove_outline, pen);
        }
        let fill = inset_half(g.fill, pen);
        if !disabled && fill.width > 0.0 && fill.height > 0.0 {
            canvas.fill_rounded_rect(fill, corner, colors.fill);
            if pen > 0.0 && colors.fill_outline.a() > 0.0 {
                canvas.stroke_rounded_rect(fill, corner, colors.fill_outline, pen);
            }
        }

        // The ticks, up to the value in the highlight.
        if let Some(n) = self.tick_count {
            for stop in 0..n {
                let f = stop as f32 / (n - 1) as f32;
                let color = if f <= value + 1e-4 {
                    colors.tick_on
                } else {
                    colors.tick_off
                };
                let (a, b) = tick_line(&g, self.orientation, stop, n);
                let rect = match self.orientation {
                    SliderOrientation::Horizontal => {
                        Rect::new(a.x - TICK_PEN * 0.5, a.y, TICK_PEN, b.y - a.y)
                    }
                    SliderOrientation::Vertical => {
                        Rect::new(a.x, a.y - TICK_PEN * 0.5, b.x - a.x, TICK_PEN)
                    }
                };
                canvas.fill_rect(rect, color);
            }
        }

        // The handle: its shadow, its face and its outline.
        let centre = Point::new(
            g.handle.x + g.handle.width * 0.5,
            g.handle.y + g.handle.height * 0.5,
        );
        let radius = (g.handle.width * 0.5 - HANDLE_INSET - pen * 0.5).max(0.0);
        if !self.is_dragging.get() && colors.shadow.a() > 0.0 && pen > 0.0 {
            canvas.stroke_circle(
                Point::new(centre.x + SHADOW_OFFSET, centre.y + SHADOW_OFFSET),
                radius,
                colors.shadow,
                pen,
            );
        }
        canvas.fill_circle(centre, radius, colors.handle);
        let focused = self.focus_origin.get().is_some();
        let outline = colors.handle_outline_for(focused, self.is_hovered.get(), disabled);
        if pen > 0.0 && outline.a() > 0.0 {
            canvas.stroke_circle(centre, radius, outline, pen);
        }

        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) && !disabled {
            let shape = &ctx.theme.shape;
            if colors.focus_ring.a() > 0.0 && shape.focus_ring_width > 0.0 {
                let ring = slider_focus_frame(g.handle, shape.focus_ring_width);
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
        // Presentational: the `Slider` emits the slider node.
        builder.set_hidden();
    }
}

/// `[Hp]` The keyboard focus frame around a slider's handle box `handle`: a
/// band `width` wide whose inner edge is the box inset by 1, a circle.
pub fn slider_focus_frame(handle: Rect, width: f32) -> FocusRingStroke {
    let inner = Rect::new(
        handle.x + HANDLE_INSET,
        handle.y + HANDLE_INSET,
        (handle.width - 2.0 * HANDLE_INSET).max(0.0),
        (handle.height - 2.0 * HANDLE_INSET).max(0.0),
    );
    FocusRingStroke::at_distance(inner, inner.width * 0.5, width * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    const LTR: LayoutDirection = LayoutDirection::LeftToRight;
    const RTL: LayoutDirection = LayoutDirection::RightToLeft;

    /// Plasma's 200 x 20 slider at 30 % puts its groove at (0, 7, 200, 6)
    /// and its handle box at (54, 0, 20, 20).
    #[test]
    fn the_groove_and_the_handle_are_where_qt_puts_them() {
        let b = Rect::new(0.0, 0.0, 200.0, 20.0);
        let g = slider_geometry(b, 0.3, SliderOrientation::Horizontal, LTR, false);
        assert_eq!(g.groove, Rect::new(0.0, 7.0, 200.0, 6.0));
        assert!((g.handle.x - 54.0).abs() < 1e-4, "{:?}", g.handle);
        assert_eq!(
            (g.handle.y, g.handle.width, g.handle.height),
            (0.0, 20.0, 20.0)
        );
        assert_eq!(g.fill.x, 0.0);
        assert_eq!(g.fill.width, g.handle.x + 10.0, "up to the handle's centre");
    }

    #[test]
    fn right_to_left_mirrors_the_handle_and_the_fill() {
        let b = Rect::new(0.0, 0.0, 200.0, 20.0);
        let l = slider_geometry(b, 0.25, SliderOrientation::Horizontal, LTR, false);
        let r = slider_geometry(b, 0.25, SliderOrientation::Horizontal, RTL, false);
        assert_eq!(r.handle.x, 200.0 - l.handle.x - 20.0);
        assert_eq!(r.fill.x + r.fill.width, 200.0, "from the right end");
        assert_eq!(r.fill.x, r.handle.x + 10.0);
        // The stops mirror too.
        let (a, _) = tick_line(&r, SliderOrientation::Horizontal, 0, 5);
        assert_eq!(a.x, 190.0, "the minimum's tick at the right");
    }

    /// The handle's travel is the one the `Slider` maps the pointer over:
    /// the length less the 20 px box.
    #[test]
    fn the_travel_is_the_widgets() {
        for orientation in [SliderOrientation::Horizontal, SliderOrientation::Vertical] {
            let b = Rect::new(10.0, 20.0, 200.0, 300.0);
            let g = slider_geometry(b, 0.0, orientation, LTR, false);
            let length = match orientation {
                SliderOrientation::Horizontal => b.width,
                SliderOrientation::Vertical => b.height,
            };
            assert_eq!((g.travel.1 - g.travel.0).abs(), length - 20.0);
        }
        let style = BreezeSliderStyle;
        let cfg = SliderStyleConfig {
            value_normalized: Signal::new(0.0),
            is_hovered: Signal::new(false),
            is_dragging: Signal::new(false),
            is_disabled: Signal::new(false),
            focus_origin: Signal::new(None),
            orientation: SliderOrientation::Horizontal,
            tick_count: None,
            variant: teksilo_core::styles::SliderVariant::Continuous,
        };
        assert_eq!(style.thumb_diameter(&cfg), 20.0);
        for density in [
            teksilo_tokens::TargetDensity::Compact,
            teksilo_tokens::TargetDensity::Comfortable,
            teksilo_tokens::TargetDensity::Touch,
        ] {
            assert_eq!(
                style.thumb_diameter_for(&cfg, &InputTokens::for_density(density)),
                20.0
            );
        }
    }

    /// A vertical slider's minimum is at the bottom, and its groove is
    /// filled from the top down to the handle, as Breeze fills a vertical
    /// slider that is not inverted (spec k.5.10).
    #[test]
    fn a_vertical_slider_fills_from_the_top() {
        let b = Rect::new(0.0, 0.0, 20.0, 120.0);
        let g = slider_geometry(b, 0.25, SliderOrientation::Vertical, LTR, false);
        assert_eq!(g.travel, (110.0, 10.0));
        assert_eq!(g.handle.y + 10.0, 110.0 - 25.0);
        assert_eq!(g.fill.y, 0.0, "from the groove's top");
        assert_eq!(g.fill.y + g.fill.height, g.handle.y + 10.0, "to the handle");
        assert_eq!(g.groove, Rect::new(7.0, 0.0, 6.0, 120.0));
        // At the minimum the whole groove above the handle is filled; at the
        // maximum the fill is the half handle above its centre.
        let low = slider_geometry(b, 0.0, SliderOrientation::Vertical, LTR, false);
        assert_eq!(low.fill, Rect::new(7.0, 0.0, 6.0, 110.0));
        let high = slider_geometry(b, 1.0, SliderOrientation::Vertical, LTR, false);
        assert_eq!(high.fill, Rect::new(7.0, 0.0, 6.0, 10.0));
    }

    /// Qt's slider with ticks below is 23 high: the ticks run from 5 to 13
    /// px below the groove's centre.
    #[test]
    fn the_ticks_lie_below_the_groove_and_add_three_pixels() {
        assert_eq!(slider_thickness(false), 20.0);
        assert_eq!(slider_thickness(true), 23.0);
        let b = Rect::new(0.0, 0.0, 200.0, 23.0);
        let g = slider_geometry(b, 0.5, SliderOrientation::Horizontal, LTR, true);
        assert_eq!(g.centre, 10.0, "the slider at the top, its ticks below");
        let (a, z) = tick_line(&g, SliderOrientation::Horizontal, 0, 3);
        assert_eq!((a, z), (Point::new(10.0, 15.0), Point::new(10.0, 23.0)));
        let (a, _) = tick_line(&g, SliderOrientation::Horizontal, 2, 3);
        assert_eq!(a.x, 190.0);
    }

    /// Whether two colours are within one step a channel: Qt's raster
    /// painter rounds an 8-bit blend its own way.
    fn near(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    /// The Breeze Light colours, against Plasma's painted samples.
    #[test]
    fn breeze_light_paints_the_specifications_colours() {
        let t = crate::light();
        let c = BreezeSliderColors::resolve(&t, true, false, false);
        assert!(
            near(rgba8(c.groove), [210, 211, 213, 255]),
            "{:?}",
            rgba8(c.groove)
        );
        assert!(
            near(rgba8(c.fill), [114, 194, 235, 255]),
            "{:?}",
            rgba8(c.fill)
        );
        assert_eq!(rgba8(c.handle), [252, 252, 252, 255]);
        assert_eq!(rgba8(c.hover), [61, 174, 233, 255]);
        assert_eq!(c.handle_outline, t.colors.border_strong);
        assert_eq!(c.focus, t.colors.border_focused);
        assert_eq!(c.groove_outline.a(), 0.2);
        let d = BreezeSliderColors::resolve(&t, true, false, true);
        assert!(
            near(rgba8(d.groove), [217, 220, 221, 255]),
            "{:?}",
            rgba8(d.groove)
        );
        assert_eq!(rgba8(d.handle), [240, 240, 240, 255]);
    }

    #[test]
    fn focus_wins_over_hover_and_disabled_over_both() {
        let c = BreezeSliderColors::resolve(&crate::light(), true, false, false);
        assert_eq!(c.handle_outline_for(false, false, false), c.handle_outline);
        assert_eq!(c.handle_outline_for(false, true, false), c.hover);
        assert_eq!(c.handle_outline_for(true, true, false), c.focus);
        assert_eq!(c.handle_outline_for(true, true, true), c.handle_outline);
    }

    /// D6: the handle's outline and the ticks reach 3:1 on the window, which
    /// Breeze's slider outline, separator and Breeze Light's highlight do
    /// not.
    #[test]
    fn the_handle_and_tick_colours_are_lifted_and_their_premises_hold() {
        for t in [crate::light(), crate::dark()] {
            let window = t.colors.surface_main;
            let c = BreezeSliderColors::resolve(&t, true, false, false);
            let b = t
                .extension::<KdePalette>()
                .unwrap()
                .colors(ColorState::Active);
            assert!(
                b.slider_outline.contrast_ratio(window) < 3.0,
                "premise: Breeze's slider outline falls short"
            );
            assert!(b.separator.contrast_ratio(window) < 3.0);
            assert!(c.handle_outline.contrast_ratio(window) >= 3.0);
            assert!(c.tick_off.contrast_ratio(window) >= 3.0);
            assert!(c.tick_on.contrast_ratio(window) >= 3.0);
        }
        let t = crate::light();
        assert!(t.colors.accent.contrast_ratio(t.colors.surface_main) < 3.0);
    }

    #[test]
    fn the_focus_frame_is_a_circle_around_the_handle() {
        let ring = slider_focus_frame(Rect::new(54.0, 0.0, 20.0, 20.0), 2.0);
        assert_eq!(ring.rect, Rect::new(54.0, 0.0, 20.0, 20.0));
        assert_eq!(ring.corner_radius, 10.0);
    }

    #[test]
    fn a_theme_without_the_palette_paints_from_its_roles() {
        let t = teksilo_core::presets::intui::light();
        let c = BreezeSliderColors::resolve(&t, true, false, false);
        assert_eq!(c.handle, t.colors.surface_content);
        assert_eq!(c.hover, t.colors.border_focused);
        assert!((c.groove_outline.a() - 0.2).abs() < 1e-6);
    }
}
