// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze check box.
//!
//! A 20 px indicator holding a 16 px box, stroked as 15 px at 2.5 with a
//! 4 px radius and filled with the button colour. Checked or partial, the
//! box is outlined in the highlight and filled again with the highlight at
//! 30 %, then marked in the view's text: a check drawn with a 2 px pen and
//! mitred joins through (6, 10), (9, 13) and (14.5, 7.5) of the indicator,
//! or three 2 px squares 4 px apart for the partial state. Pressed, the
//! button colour darkens 110 %; under the pointer a 1 px ring of the focus
//! colour lies on the outline. The mark draws itself along its length over
//! the style's 100 ms, linearly, and the partial squares appear one at each
//! third. The colours, and where they depart from Breeze, are in
//! [`crate::styles::indicator`].
//!
//! ## D8, the check box row
//!
//! Breeze lays a labelled check box out in a row as tall as the label's
//! line box plus 2 px above and below: 22 px at Plasma's default font, the
//! indicator 1 px from its top. That is under WCAG 2.2 SC 2.5.8's 24 px
//! target. The `Checkbox` widget already wraps its row in a minimum size of
//! the density's target, 24 px at Compact, so the node is 24 px tall and
//! the indicator, still painted at 20, sits 2 px from its top. Nothing is
//! scaled: only the row grows.
//!
//! ## The label
//!
//! Breeze puts the label 24 px from the indicator's leading edge, the
//! indicator and a 4 px spacing. The widget puts its own 6 px between the
//! style's body and the label, so the body is laid out 18 px wide, the
//! indicator less the trailing 2 px of its focus margin, and the label lands
//! where Breeze puts it. The box ends at 18, so it lies in the body; only
//! the keyboard focus frame reaches past it.
//!
//! ## Variants
//!
//! Breeze has one check box. `Square` is it; `Rounded` rounds the box to
//! 6 px and `Circle` makes it a disc (`[unpublished]`), with Breeze's fills,
//! outline and mark.

use teksilo_canvas::{Canvas, LineJoin, Path, Point, Rect, Size, SizeProposal, StrokeStyle};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    CheckboxState, CheckboxStyle, CheckboxStyleConfig, CheckboxVariant, FocusRingStroke,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};
use teksilo_widgets::styles::recipe_checkbox_style::CHECKBOX_LABEL_GAP;

use crate::shape::BREEZE_CHECKBOX_RADIUS;
use crate::styles::indicator::{BreezeIndicatorColors, box_rect, indicator_rect, stroked_box_rect};
use crate::styles::metrics::{
    CHECK_MARK_PEN, INDICATOR_BOX_STROKED, INDICATOR_CHECKED_WASH, INDICATOR_SIZE,
    PARTIAL_DOT_SIZE, PARTIAL_DOT_SPACING, indicator_body_width,
};

/// `[Hp]` The check mark's three points, in the indicator's coordinates:
/// `(left + 4, centre y)`, `(centre x - 1, bottom - 5)`, `(centre x + 4.5,
/// top + 5.5)` of the 16 px box at (2, 2), whose centre is (10, 10).
pub const CHECK_MARK_POINTS: [(f32, f32); 3] = [(6.0, 10.0), (9.0, 13.0), (14.5, 7.5)];

/// `[unpublished]` The radius of the `Rounded` variant's box.
const ROUNDED_RADIUS: f32 = 6.0;

/// Breeze `CheckboxStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeCheckboxStyle;

impl CheckboxStyle for BreezeCheckboxStyle {
    fn make_body(&self, cfg: &CheckboxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let marked = |s: &CheckboxState| !matches!(s, CheckboxState::Unchecked);
        let progress = ctx.animated_signal(if marked(&cfg.state.get()) { 1.0 } else { 0.0 });
        // `[S]` Breeze's style transitions: 100 ms, linear.
        let draw = ctx.animate().fast().standard();
        {
            let progress = progress.clone();
            ctx.effect(&cfg.state, move |s| {
                draw.to_or_snap(&progress, if marked(s) { 1.0 } else { 0.0 });
            });
        }
        let show_focus_frame = cfg
            .is_focused
            .zip(&cfg.is_disabled)
            .map(|(focused, disabled)| *focused && !*disabled);
        ctx.add(BreezeCheckboxBody {
            check: cfg.state.clone(),
            progress,
            is_hovered: cfg.is_hovered.clone(),
            is_pressed: cfg.is_pressed.clone(),
            is_disabled: cfg.is_disabled.clone(),
            show_focus_frame,
            variant: cfg.variant,
        })
    }
}

struct BreezeCheckboxBody {
    check: Signal<CheckboxState>,
    /// How far the mark is drawn, 0 to 1.
    progress: Signal<f32>,
    is_hovered: Signal<bool>,
    is_pressed: Signal<bool>,
    is_disabled: Signal<bool>,
    show_focus_frame: Signal<bool>,
    variant: CheckboxVariant,
}

impl std::fmt::Debug for BreezeCheckboxBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeCheckboxBody")
            .field("variant", &self.variant)
            .finish()
    }
}

/// The radius the box of `variant` is stroked with.
pub fn variant_radius(variant: CheckboxVariant) -> f32 {
    match variant {
        CheckboxVariant::Square => BREEZE_CHECKBOX_RADIUS,
        CheckboxVariant::Rounded => ROUNDED_RADIUS,
        CheckboxVariant::Circle => INDICATOR_BOX_STROKED * 0.5,
    }
}

/// What a box paints: a face, an optional highlight wash over it, an
/// outline, and whether the hover ring lies on the outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxPaint {
    /// The button colour, darkened while pressed.
    pub face: Color,
    /// The highlight at 30 % over the face, when checked or partial.
    pub wash: Option<Color>,
    /// The 1 px outline.
    pub outline: Color,
    /// The 1 px ring of the focus colour on the outline, under the pointer.
    pub hover_ring: Option<Color>,
}

/// `[Hp]` What a box (or a radio disc) paints, marked or not, hovered or
/// pressed, enabled or not, from `c`.
pub fn box_paint(
    marked: bool,
    hovered: bool,
    pressed: bool,
    disabled: bool,
    c: &BreezeIndicatorColors,
) -> BoxPaint {
    let active = !disabled;
    BoxPaint {
        face: if pressed && active {
            c.button_pressed
        } else {
            c.button
        },
        wash: marked.then(|| c.highlight.with_alpha(INDICATOR_CHECKED_WASH)),
        outline: if marked { c.highlight } else { c.outline },
        hover_ring: (hovered && active).then_some(c.focus),
    }
}

/// Paint `paint` on the stroked box `rect` at `radius`.
pub fn paint_box(canvas: &mut Canvas, rect: Rect, radius: f32, pen: f32, paint: &BoxPaint) {
    let corner = CornerRadius::uniform(radius);
    if paint.face.a() > 0.0 {
        canvas.fill_rounded_rect(rect, corner, paint.face);
    }
    if let Some(wash) = paint.wash
        && wash.a() > 0.0
    {
        canvas.fill_rounded_rect(rect, corner, wash);
    }
    if pen <= 0.0 {
        return;
    }
    if paint.outline.a() > 0.0 {
        canvas.stroke_rounded_rect(rect, corner, paint.outline, pen);
    }
    if let Some(ring) = paint.hover_ring
        && ring.a() > 0.0
    {
        canvas.stroke_rounded_rect(rect, corner, ring, pen);
    }
}

/// `[Hp]` The keyboard focus frame around an indicator: a band as wide as
/// the focus ring whose inner edge is the 16 px box, with the box's corner
/// radius `radius`.
pub fn indicator_focus_frame(indicator: Rect, radius: f32, width: f32) -> FocusRingStroke {
    FocusRingStroke::at_distance(box_rect(indicator), radius, width * 0.5)
}

/// The points of the check mark drawn `progress` of the way along its
/// length, in canvas coordinates: none at 0, the whole polyline at 1.
pub fn check_mark(indicator: Rect, progress: f32) -> Vec<Point> {
    let at = |(x, y): (f32, f32)| Point::new(indicator.x + x, indicator.y + y);
    let points = CHECK_MARK_POINTS.map(at);
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return Vec::new();
    }
    let lengths: Vec<f32> = points
        .windows(2)
        .map(|w| ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt())
        .collect();
    let mut left = lengths.iter().sum::<f32>() * progress;
    let mut out = vec![points[0]];
    for (w, len) in points.windows(2).zip(lengths) {
        if left >= len {
            out.push(w[1]);
            left -= len;
        } else {
            let t = left / len;
            out.push(Point::new(
                w[0].x + (w[1].x - w[0].x) * t,
                w[0].y + (w[1].y - w[0].y) * t,
            ));
            break;
        }
    }
    out
}

/// The partial squares shown at `progress`: one at each third, from the
/// leading side.
pub fn partial_dots(indicator: Rect, progress: f32, direction_rtl: bool) -> Vec<Rect> {
    let shown = (progress.clamp(0.0, 1.0) * 3.0 + 1e-4).floor() as usize;
    let cx = indicator.x + INDICATOR_SIZE * 0.5;
    let cy = indicator.y + INDICATOR_SIZE * 0.5;
    let half = PARTIAL_DOT_SIZE * 0.5;
    let order: [f32; 3] = if direction_rtl {
        [1.0, 0.0, -1.0]
    } else {
        [-1.0, 0.0, 1.0]
    };
    order
        .iter()
        .take(shown)
        .map(|side| {
            Rect::new(
                cx + side * PARTIAL_DOT_SPACING - half,
                cy - half,
                PARTIAL_DOT_SIZE,
                PARTIAL_DOT_SIZE,
            )
        })
        .collect()
}

impl Widget for BreezeCheckboxBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.check.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.progress
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        for s in [
            &self.is_hovered,
            &self.is_pressed,
            &self.is_disabled,
            &self.show_focus_frame,
        ] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        vec![]
    }

    /// The 18 px body of the module documentation, as tall as the indicator.
    /// The widget's own minimum size raises the row to the density's target
    /// (D8).
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(indicator_body_width(CHECKBOX_LABEL_GAP), INDICATOR_SIZE).into()
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
        let check = self.check.get();
        let disabled = self.is_disabled.get();
        let colors = BreezeIndicatorColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            disabled,
        );
        let indicator = indicator_rect(bounds, ctx.layout_direction);
        let pen = ctx.theme.shape.border_width;
        let radius = variant_radius(self.variant);
        let paint = box_paint(
            !matches!(check, CheckboxState::Unchecked),
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

        let progress = self.progress.get();
        let rtl = ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft;
        match check {
            CheckboxState::Checked => {
                let points = check_mark(indicator, progress);
                if points.len() >= 2 && colors.text.a() > 0.0 {
                    let mut path = Path::new();
                    path.move_to(points[0]);
                    for p in &points[1..] {
                        path.line_to(*p);
                    }
                    let mut style = StrokeStyle::solid(CHECK_MARK_PEN);
                    style.line_join = LineJoin::Miter;
                    canvas.stroke_path(&path, colors.text, style);
                }
            }
            CheckboxState::Indeterminate => {
                for dot in partial_dots(indicator, progress, rtl) {
                    canvas.fill_rect(dot, colors.text);
                }
            }
            CheckboxState::Unchecked => {}
        }

        if self.show_focus_frame.get() {
            let shape = &ctx.theme.shape;
            if colors.focus_ring.a() > 0.0 && shape.focus_ring_width > 0.0 {
                let ring = indicator_focus_frame(indicator, radius, shape.focus_ring_width);
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
        // Presentational: the `Checkbox` emits the check box node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> BreezeIndicatorColors {
        BreezeIndicatorColors::resolve(&crate::light(), true, false, false)
    }

    #[test]
    fn the_box_follows_breezes_rules() {
        let c = c();
        let off = box_paint(false, false, false, false, &c);
        assert_eq!(off.face, c.button);
        assert_eq!(off.wash, None);
        assert_eq!(off.outline, c.outline);
        assert_eq!(off.hover_ring, None);

        let on = box_paint(true, false, false, false, &c);
        assert_eq!(on.outline, c.highlight);
        assert_eq!(on.wash, Some(c.highlight.with_alpha(0.3)));

        let pressed = box_paint(false, false, true, false, &c);
        assert_eq!(pressed.face, c.button_pressed);

        let hovered = box_paint(false, true, false, false, &c);
        assert_eq!(hovered.hover_ring, Some(c.focus));
        // A disabled box neither presses nor hovers.
        let d = box_paint(false, true, true, true, &c);
        assert_eq!((d.face, d.hover_ring), (c.button, None));
    }

    #[test]
    fn the_mark_runs_through_the_three_points() {
        let i = Rect::new(0.0, 0.0, 20.0, 20.0);
        let whole = check_mark(i, 1.0);
        assert_eq!(
            whole,
            vec![
                Point::new(6.0, 10.0),
                Point::new(9.0, 13.0),
                Point::new(14.5, 7.5)
            ]
        );
        assert!(check_mark(i, 0.0).is_empty());
        // Part way, it ends on the polyline, short of the end.
        let half = check_mark(i, 0.5);
        let end = *half.last().unwrap();
        assert!(end.x > 9.0 && end.x < 14.5, "{end:?}");
        assert!(
            (end.y - (13.0 - (end.x - 9.0))).abs() < 1e-3,
            "on the long arm"
        );
    }

    #[test]
    fn the_partial_squares_appear_one_a_third() {
        let i = Rect::new(0.0, 0.0, 20.0, 20.0);
        assert!(partial_dots(i, 0.0, false).is_empty());
        assert_eq!(partial_dots(i, 0.34, false).len(), 1);
        assert_eq!(partial_dots(i, 0.67, false).len(), 2);
        let all = partial_dots(i, 1.0, false);
        assert_eq!(
            all,
            vec![
                Rect::new(5.0, 9.0, 2.0, 2.0),
                Rect::new(9.0, 9.0, 2.0, 2.0),
                Rect::new(13.0, 9.0, 2.0, 2.0)
            ]
        );
        // From the trailing side's opposite in right-to-left.
        assert_eq!(
            partial_dots(i, 0.34, true)[0],
            Rect::new(13.0, 9.0, 2.0, 2.0)
        );
    }

    #[test]
    fn the_focus_frame_lies_outside_the_box() {
        let i = Rect::new(0.0, 0.0, 20.0, 20.0);
        let ring = indicator_focus_frame(i, BREEZE_CHECKBOX_RADIUS, 2.0);
        // Centred 1 px outside the 16 px box: from its edge to the
        // indicator's.
        assert_eq!(ring.rect, Rect::new(1.0, 1.0, 18.0, 18.0));
        assert_eq!(ring.corner_radius, BREEZE_CHECKBOX_RADIUS + 1.0);
    }

    #[test]
    fn variants_move_only_the_radius() {
        assert_eq!(variant_radius(CheckboxVariant::Square), 4.0);
        assert!(variant_radius(CheckboxVariant::Rounded) > 4.0);
        assert_eq!(variant_radius(CheckboxVariant::Circle), 7.5);
    }
}
