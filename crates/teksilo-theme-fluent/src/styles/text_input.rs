// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Fluent `TextBox`.
//!
//! The signature detail is the **accent focus underline**. A Fluent field
//! is a 32 dp, 4 dp-rounded `ControlFillColorDefault` rectangle with a 1 dp
//! hairline everywhere *except* its bottom edge, which carries a heavier
//! `ControlStrongStrokeColorDefault` line at rest. On focus the fill
//! brightens to `ControlFillColorInputActive` and that bottom edge grows to
//! 2 dp and turns accent — `TextControlBorderThemeThicknessFocused` is
//! literally `1,1,1,2`.
//!
//! WinUI produces it with a gradient brush anchored to a 2 dp band and two
//! gradient stops at the same offset, which is a way of saying "the bottom
//! two pixels are a different colour" in a system that only has one border
//! brush. Teksilo can say that directly, so the edge is painted as its own
//! sliver by [`crate::styles::chrome::paint_edge`].
//!
//! Validation still wins over focus: an errored field paints its bottom
//! edge — and its hairline — in the error colour, so the state that matters
//! most is the one you see.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    TextInputStyle, TextInputStyleConfig, TextInputValidationLevel, TextInputVariant,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};
use teksilo_widgets::primitives::{MinSize, Padding, ZStack};

use crate::palette::{FluentEdgeSide, FluentPalette};
use crate::shape::FLUENT_CONTROL_CORNER_RADIUS;
use crate::styles::chrome::paint_edge;

/// `TextControlThemeMinHeight` (dp).
const MIN_HEIGHT: f32 = 32.0;
/// The horizontal half of `TextControlThemePadding` (`10,5,6,6`).
///
/// **Only the horizontal half.** `TextInput` has already wrapped the editor
/// in its own vertical padding by the time a style sees it — "wrapped in
/// vertical padding so slots (IconButton etc.) sit flush against top/bottom
/// of the inner border area", per the widget — which is why the shipped
/// `RecipeTextInputStyle` also pads horizontally only. Restoring WinUI's
/// 5 dp top / 6 dp bottom here *looks* like fidelity but double-applies the
/// inset: the field grows to 40 dp and stops lining up with the 32 dp button
/// beside it. The height comes from `MIN_HEIGHT` instead.
const PADDING_LEADING: f32 = 10.0;
const PADDING_TRAILING: f32 = 6.0;
/// The bottom edge at rest, and when focused (`1,1,1,2`).
const EDGE_REST: f32 = 1.0;
const EDGE_FOCUSED: f32 = 2.0;

/// Fluent `TextInputStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct FluentTextInputStyle;

impl TextInputStyle for FluentTextInputStyle {
    fn make_body(&self, cfg: &TextInputStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // `Bare` is the embedded case — a ComboBox's text part, a search
        // field's inner editor — where the surrounding control owns the
        // chrome. Adding a second frame there would double every border.
        if cfg.variant == TextInputVariant::Bare {
            return cfg.editor;
        }

        let chrome = ctx.add(FluentFieldChrome {
            is_focused: cfg.is_focused.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_disabled: cfg.is_disabled.clone(),
            validation: cfg.validation.clone(),
            variant: cfg.variant,
        });
        // Horizontal only — see `PADDING_LEADING`.
        let padded =
            ctx.add(Padding::new(0.0, PADDING_TRAILING, 0.0, PADDING_LEADING).child_id(cfg.editor));
        let stack = ctx.add(ZStack::new().add_child(chrome).add_child(padded));
        ctx.add(MinSize::new(0.0, MIN_HEIGHT).child_id(stack))
    }
}

struct FluentFieldChrome {
    is_focused: Signal<bool>,
    is_hovered: Signal<bool>,
    is_disabled: Signal<bool>,
    validation: Signal<TextInputValidationLevel>,
    variant: TextInputVariant,
}

impl std::fmt::Debug for FluentFieldChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluentFieldChrome")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for FluentFieldChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.is_focused, &self.is_hovered, &self.is_disabled] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        self.validation
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
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
        let focused = self.is_focused.get();
        let hovered = self.is_hovered.get();
        let disabled = self.is_disabled.get();
        let validation = self.validation.get();
        let c = &ctx.theme.colors;
        let p = ctx.theme.extension::<FluentPalette>();

        let radius = FLUENT_CONTROL_CORNER_RADIUS;
        let corner = CornerRadius::uniform(radius);

        // Fill. `Filled` and `Underline` are Teksilo variants Fluent has no
        // twin for; both take the same surface, and `Underline` simply
        // skips the hairline below.
        let fill = match (p, disabled, focused, hovered) {
            (Some(p), true, _, _) => p.control_fill_disabled,
            (Some(p), _, true, _) => p.control_fill_input_active,
            (Some(p), _, _, true) => p.control_fill_secondary,
            (Some(p), ..) => p.control_fill_default,
            (None, true, ..) => c.surface_disabled,
            (None, ..) => c.surface_content,
        };
        if fill.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, corner, fill);
        }

        // Hairline on the three quiet sides. `Underline` drops it entirely
        // so only the accent edge remains.
        let hairline = validation_color(validation, ctx).unwrap_or(match (p, disabled) {
            (Some(p), _) => p.control_stroke_default,
            (None, true) => c.border_disabled,
            (None, false) => c.border,
        });
        if self.variant != TextInputVariant::Underline
            && hairline.a() > 0.0
            && ctx.theme.shape.border_width > 0.0
        {
            canvas.stroke_rounded_rect(bounds, corner, hairline, ctx.theme.shape.border_width);
        }

        // The bottom edge: heavier than the hairline at rest, accent and
        // 2 dp on focus, validation-tinted whenever a level is set.
        let (edge_w, edge_color) = if disabled {
            (
                EDGE_REST,
                p.map_or(c.border_disabled, |p| p.control_strong_stroke_disabled),
            )
        } else if let Some(v) = validation_color(validation, ctx) {
            (if focused { EDGE_FOCUSED } else { EDGE_REST }, v)
        } else if focused {
            (EDGE_FOCUSED, c.accent)
        } else {
            (
                EDGE_REST,
                p.map_or(c.border_strong, |p| p.control_strong_stroke_default),
            )
        };
        paint_edge(
            canvas,
            bounds,
            radius,
            FluentEdgeSide::Bottom,
            edge_w,
            edge_color,
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

/// The tint a validation level imposes, or `None` for
/// [`TextInputValidationLevel::None`].
fn validation_color(level: TextInputValidationLevel, ctx: &PaintContext) -> Option<Color> {
    let c = &ctx.theme.colors;
    match level {
        TextInputValidationLevel::None => None,
        TextInputValidationLevel::Info => Some(c.status_info_fg),
        TextInputValidationLevel::Warning => Some(c.border_warning),
        TextInputValidationLevel::Error => Some(c.border_error),
        // A brief accent flash after an auto-correction — the same colour
        // focus uses, which is the point: it reads as "we changed this".
        TextInputValidationLevel::Corrected => Some(c.accent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_doubles_the_bottom_edge() {
        // `TextControlBorderThemeThicknessFocused` = 1,1,1,2.
        assert_eq!(EDGE_REST, 1.0);
        assert_eq!(EDGE_FOCUSED, 2.0);
    }

    #[test]
    fn min_height_is_the_standard_control_height() {
        assert_eq!(MIN_HEIGHT, 32.0);
    }

    #[test]
    fn padding_is_the_horizontal_half_of_the_winui_text_control_padding() {
        // `TextControlThemePadding` = 10,5,6,6 (left, top, right, bottom).
        // The vertical half is the widget's to apply, not the style's — see
        // the constants' doc comment.
        assert_eq!(PADDING_LEADING, 10.0);
        assert_eq!(PADDING_TRAILING, 6.0);
    }
}
