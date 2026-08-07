// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! RectWidget — a leaf widget that paints a filled and/or stroked rounded rectangle.
//!
//! `RectWidget` has no intrinsic content: it fills whatever space its parent
//! proposes (or reports `0×0` when unconstrained) and draws a fill (solid color
//! or gradient), an optional border (a uniform stroke positioned inside / center
//! / outside, or per-side edge fills for an underline), and an optional corner
//! radius. It is the low-level building block for card backgrounds, focus rings,
//! dividers, underlined fields, and highlight overlays.
//!
//! The fill accepts `impl Into<PaintProp>` — anything `Into<ColorProp>` (a raw
//! `Color`, a theme role such as `SurfaceRole::Hover`, or a `Signal<Color>`) for
//! a solid, plus `PaintProp::Linear` / `Radial` for a gradient. Border color
//! accepts `impl Into<ColorProp>`, so reactive interaction-driven colors require
//! no extra wiring.
//!
//! ```rust
//! # use teksilo_tokens::{Color, CornerRadius};
//! # use teksilo_widgets::primitives::RectWidget;
//! // A pill-shaped accent badge background:
//! let _w = RectWidget::new()
//!     .background(Color::from_rgba(0.2, 0.5, 1.0, 1.0))
//!     .corner_radius(CornerRadius::uniform(12.0));
//! ```

use teksilo_canvas::{Canvas, Paint, Rect, Size, SizeProposal};
use teksilo_tokens::{Color, CornerRadius};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::paint_prop::PaintProp;
use teksilo_core::signal::Prop;
use teksilo_core::styles::{BorderPosition, BorderSides, apply_border_position};
use teksilo_core::widget::{LayoutContext, PaintContext, Widget};

/// A leaf widget that paints a filled and/or stroked rounded rectangle.
///
/// See the [module documentation](self) for the full feature description.
/// All visual properties accept `impl Into<ColorProp>` (colors/roles/signals) or
/// `impl Into<Prop<f32>>` / `impl Into<Prop<CornerRadius>>` (static or reactive)
/// — so the common "fill with theme surface, border with theme border" setup is
/// just `.background(SurfaceRole::Main).border_color(BorderRole::Default)`.
pub struct RectWidget {
    background: PaintProp,
    border_color: ColorProp,
    border_width: Prop<f32>,
    corner_radius: Prop<CornerRadius>,
    /// `None` = a uniform stroke on all four sides (honouring
    /// `border_position`). `Some(..)` = per-side edge fills (e.g. a
    /// bottom-only underline), drawn with `border_color`.
    border_sides: Prop<Option<BorderSides>>,
    /// Where a uniform stroke sits relative to the rect edge. Default
    /// `Center` matches the SDF stroke's native behaviour.
    border_position: BorderPosition,
}

impl RectWidget {
    /// Create a fully transparent, zero-border rectangle with no corner radius.
    pub fn new() -> Self {
        Self {
            background: PaintProp::Solid(ColorProp::Static(Color::TRANSPARENT)),
            border_color: ColorProp::Static(Color::TRANSPARENT),
            border_width: Prop::Static(0.0),
            corner_radius: Prop::Static(CornerRadius::ZERO),
            border_sides: Prop::Static(None),
            border_position: BorderPosition::Center,
        }
    }

    /// Fill. Accepts `Color`, a theme role (`SurfaceRole`, etc.), a
    /// `Signal<Color>`, or a [`PaintProp`] (e.g. a gradient).
    pub fn background(mut self, paint: impl Into<PaintProp>) -> Self {
        self.background = paint.into();
        self
    }

    /// Per-side border widths (e.g. [`BorderSides::bottom`] for an
    /// underline). When set, overrides the uniform stroke; sides are
    /// drawn as edge fills in `border_color`.
    pub fn border_sides(mut self, sides: impl Into<Prop<Option<BorderSides>>>) -> Self {
        self.border_sides = sides.into();
        self
    }

    /// Where a uniform stroke sits relative to the rect edge
    /// (inside / center / outside). Ignored when `border_sides` is set.
    pub fn border_position(mut self, position: BorderPosition) -> Self {
        self.border_position = position;
        self
    }

    /// Border color. Accepts `Color`, a theme role (`BorderRole`, etc.),
    /// or a `Signal<Color>`.
    pub fn border_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.border_color = color.into();
        self
    }

    /// Stroke width, in logical pixels. Accepts a static value or a reactive `Signal<f32>`.
    pub fn border_width(mut self, width: impl Into<Prop<f32>>) -> Self {
        self.border_width = width.into();
        self
    }

    /// Corner radius for the fill and stroke. Accepts a `CornerRadius` (per-corner
    /// control) or a reactive `Signal<CornerRadius>`.
    pub fn corner_radius(mut self, radius: impl Into<Prop<CornerRadius>>) -> Self {
        self.corner_radius = radius.into();
        self
    }
}

impl Default for RectWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RectWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RectWidget").finish()
    }
}

impl Widget for RectWidget {
    fn build(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> Vec<teksilo_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.background.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_color.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_width.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.corner_radius.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.border_sides.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // RectWidget has no intrinsic content — it accepts whatever space is offered.
        // With an exact proposal it fills the space; with unspecified it reports 0x0.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = self.corner_radius.get();

        // Fill — a solid color or a gradient. Gradient endpoints are
        // rect-local, so only the size is needed.
        let paint = self.background.resolve(
            ctx.theme,
            ctx.effective_enabled,
            Size::new(bounds.width, bounds.height),
        );
        let skip_fill = matches!(&paint, Paint::Solid(c) if c.a() <= 0.0);
        if !skip_fill {
            canvas.fill_rounded_rect(bounds, radius, paint);
        }

        // Border — per-side edge fills, or a uniform stroke.
        let bc = self.border_color.resolve(ctx.theme, ctx.effective_enabled);
        if bc.a() <= 0.0 {
            return;
        }
        match self.border_sides.get() {
            Some(sides) => paint_border_sides(canvas, bounds, sides, bc),
            None => {
                let bw = self.border_width.get();
                if bw > 0.0 {
                    let stroke_rect = apply_border_position(bounds, bw, self.border_position);
                    canvas.stroke_rounded_rect(stroke_rect, radius, bc, bw);
                }
            }
        }
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Draw the non-zero edges of a per-side border as filled rects.
/// `Leading`/`Trailing` map to left/right (LTR); RTL flipping is a
/// follow-up — the common case (a bottom underline) is direction-neutral.
fn paint_border_sides(canvas: &mut Canvas, bounds: Rect, sides: BorderSides, color: Color) {
    if sides.top > 0.0 {
        canvas.fill_rect(
            Rect::new(bounds.x, bounds.y, bounds.width, sides.top),
            color,
        );
    }
    if sides.bottom > 0.0 {
        canvas.fill_rect(
            Rect::new(
                bounds.x,
                bounds.y + bounds.height - sides.bottom,
                bounds.width,
                sides.bottom,
            ),
            color,
        );
    }
    if sides.leading > 0.0 {
        canvas.fill_rect(
            Rect::new(bounds.x, bounds.y, sides.leading, bounds.height),
            color,
        );
    }
    if sides.trailing > 0.0 {
        canvas.fill_rect(
            Rect::new(
                bounds.x + bounds.width - sides.trailing,
                bounds.y,
                sides.trailing,
                bounds.height,
            ),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::signal::Signal;
    use teksilo_core::widget_tree::WidgetTree;

    #[test]
    fn static_background_paints_correctly() {
        let mut tree = WidgetTree::new();
        tree.add(
            RectWidget::new()
                .background(Color::RED)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());
    }

    #[test]
    fn accent_role_desaturates_when_window_inactive() {
        use teksilo_tokens::SurfaceRole;

        let theme = teksilo_core::presets::intui::light();
        let accent = theme.colors.accent.to_array();
        let inactive_accent = theme.colors.for_inactive_window().accent.to_array();
        assert_ne!(accent, inactive_accent);

        let mut tree = WidgetTree::new().with_theme(theme);
        tree.add(
            RectWidget::new()
                .background(SurfaceRole::Accent)
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));

        // Active: the role resolves to the vivid accent.
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(
            frame.shapes[0].color, accent,
            "active window paints the vivid accent"
        );

        // Inactive: the paint walker swaps in the accent-desaturated theme
        // projection, so the *same* SurfaceRole::Accent resolves to the muted
        // colour — the systemic theme-side path that greys every accent control
        // (Toggle, Button, Tab, Segment, Checkbox/Radio, Slider, ProgressBar)
        // with no per-widget code.
        tree.set_window_active(false);
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(
            frame.shapes[0].color, inactive_accent,
            "inactive window desaturates the accent"
        );

        // Reactivate: vivid accent returns.
        tree.set_window_active(true);
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, accent);
    }

    #[test]
    fn background_reads_from_state() {
        let color = Signal::new(Color::BLUE);
        let mut tree = WidgetTree::new();
        let w = tree.add(
            RectWidget::new()
                .background(color.clone())
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        color.bind_to(
            w,
            tree.binding_registry(),
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::BLUE.to_array());
    }

    #[test]
    fn underline_draws_a_bottom_decoration() {
        let mut tree = WidgetTree::new();
        tree.add(
            RectWidget::new()
                .border_color(Color::RED)
                .border_sides(Some(BorderSides::bottom(2.0))),
        );
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        // The bottom underline is an edge-fill decoration in the border color.
        let underline = frame
            .decorations
            .iter()
            .find(|d| d.color == Color::RED.to_array())
            .expect("underline decoration present");
        // rect = [x, y, w, h]; bottom edge sits at y = height - width.
        assert_eq!(underline.rect[1], 38.0);
        assert_eq!(underline.rect[3], 2.0);
        // No uniform stroke shape was emitted.
        assert!(frame.shapes.iter().all(|s| s.stroke_width == 0.0));
    }

    #[test]
    fn gradient_background_emits_linear_gradient_paint() {
        use teksilo_canvas::render_frame::PaintData;
        use teksilo_core::paint_prop::{GradientStopProp, PaintProp};

        let mut tree = WidgetTree::new();
        tree.add(RectWidget::new().background(PaintProp::Linear {
            stops: vec![
                GradientStopProp {
                    offset: 0.0,
                    color: Color::RED.into(),
                },
                GradientStopProp {
                    offset: 1.0,
                    color: Color::BLUE.into(),
                },
            ],
            angle_deg: 90.0,
        }));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        assert!(matches!(
            frame.shapes[0].paint_data,
            PaintData::LinearGradient { .. }
        ));
    }

    #[test]
    fn background_updates_on_state_change() {
        let color = Signal::new(Color::RED);
        let mut tree = WidgetTree::new();
        let w = tree.add(
            RectWidget::new()
                .background(color.clone())
                .corner_radius(CornerRadius::uniform(4.0)),
        );
        color.bind_to(
            w,
            tree.binding_registry(),
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );

        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::RED.to_array());

        // Change the state
        color.set(Color::GREEN);
        tree.layout(SizeProposal::exact(100.0, 40.0));
        let frame = tree.render();
        assert_eq!(frame.shapes[0].color, Color::GREEN.to_array());
    }
}
