// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS bezelled text field.
//!
//! The signature detail is what happens on **focus**: the whole field
//! grows an accent halo around its outline. Nothing else moves — the fill
//! does not brighten, the border does not thicken, no edge turns accent.
//! That is the opposite of Fluent, whose field announces focus by growing
//! its *bottom* edge to 2 dp and turning it accent, and of Material 3,
//! which thickens the whole outline. If a Teksilo app is going to be
//! mistaken for a Mac app or not, this is one of the two or three places
//! it gets decided.
//!
//! The field itself is `textBackgroundColor` — pure white in Aqua and
//! `#1E1E1E` in Dark Aqua, which makes it *darker* than the `#323232`
//! window around it. Editable text on macOS sits in a well, not on a
//! raised surface, and getting that relationship backwards is the fastest
//! way to make a dark theme look like a web page.
//!
//! **One deviation.** The outline is `border_strong` rather than
//! AppKit's own hairline, for the reason documented on
//! [`crate::styles::checkbox`]: a text field's boundary is a UI component
//! boundary under WCAG SC 1.4.11, the fill alone only separates it from
//! the window at 1.2:1, and AppKit's hairline does not close the gap.
//!
//! ## Padding
//!
//! Horizontal only. `TextInput` has **already** wrapped the editor in its
//! own vertical inset by the time a style sees it — "wrapped in vertical
//! padding so slots (IconButton etc.) sit flush against top/bottom of the
//! inner border area", per the widget — which is why the shipped
//! `RecipeTextInputStyle` also pads horizontally only. Adding a vertical
//! inset here *looks* like fidelity and double-applies: the field grows
//! past the control height and stops lining up with the button beside it.
//! The height comes from the `MinSize` floor instead.

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

use crate::palette::MacOsPalette;
use crate::shape::{MACOS_CONTROL_CORNER_RADIUS, MACOS_CONTROL_HEIGHT};
use crate::styles::chrome::{paint_focus_ring, paint_ring};

/// Horizontal gutter (dp) — the inset AppKit gives a bezelled field's
/// text.
const PADDING_H: f32 = 6.0;

/// macOS `TextInputStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsTextInputStyle;

impl TextInputStyle for MacOsTextInputStyle {
    fn make_body(&self, cfg: &TextInputStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // `Bare` is the embedded case — a ComboBox's text part, a search
        // field's inner editor — where the surrounding control owns the
        // chrome. Adding a second frame there would double every border.
        if cfg.variant == TextInputVariant::Bare {
            return cfg.editor;
        }

        let chrome = ctx.add(MacOsFieldChrome {
            is_focused: cfg.is_focused.clone(),
            is_disabled: cfg.is_disabled.clone(),
            validation: cfg.validation.clone(),
            variant: cfg.variant,
        });
        // Horizontal only — see the module doc.
        let padded = ctx.add(Padding::new(0.0, PADDING_H, 0.0, PADDING_H).child_id(cfg.editor));
        let stack = ctx.add(ZStack::new().add_child(chrome).add_child(padded));
        ctx.add(MinSize::new(0.0, MACOS_CONTROL_HEIGHT).child_id(stack))
    }
}

struct MacOsFieldChrome {
    is_focused: Signal<bool>,
    is_disabled: Signal<bool>,
    validation: Signal<TextInputValidationLevel>,
    variant: TextInputVariant,
}

impl std::fmt::Debug for MacOsFieldChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOsFieldChrome")
            .field("variant", &self.variant)
            .finish()
    }
}

impl Widget for MacOsFieldChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.is_focused, &self.is_disabled] {
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
        let disabled = self.is_disabled.get();
        let validation = self.validation.get();
        let c = &ctx.theme.colors;
        let p = ctx.theme.extension::<MacOsPalette>();

        let radius = MACOS_CONTROL_CORNER_RADIUS;
        let corner = CornerRadius::uniform(radius);

        // Fill: `textBackgroundColor` at rest — the well an editable field
        // sits in. Focus does **not** brighten it; on macOS the ring is
        // the whole announcement.
        let fill = if disabled {
            p.map_or(c.surface_disabled, |p| p.disabled_control_face)
        } else {
            p.map_or(c.surface_content, |p| p.text_background)
        };
        if fill.a() > 0.0 {
            canvas.fill_rounded_rect(bounds, corner, fill);
        }

        // Outline. `Underline` is a Teksilo variant AppKit has no twin
        // for; it drops the frame and keeps only the bottom rule.
        let outline = validation_color(validation, ctx).unwrap_or(if disabled {
            c.border_disabled
        } else {
            c.border_strong
        });
        let width = ctx.theme.shape.border_width;
        if outline.a() > 0.0 && width > 0.0 {
            if self.variant == TextInputVariant::Underline {
                canvas.draw_border_bottom(bounds, outline, width);
            } else {
                canvas.stroke_rounded_rect(bounds, corner, outline, width);
            }
        }

        // …and the halo, which is the entire focus affordance.
        if focused && !disabled {
            match validation_color(validation, ctx) {
                // An errored field rings in its own colour: the state that
                // matters most is the one you see.
                Some(tint) => paint_ring(canvas, bounds, radius, tint, ctx),
                None => paint_focus_ring(canvas, bounds, radius, ctx),
            }
        }
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
    use teksilo_core::styles::Theme;

    #[test]
    fn the_gutter_is_horizontal_only() {
        // The vertical half belongs to the widget, not the style — see
        // the module doc. There is deliberately no PADDING_V constant to
        // reach for.
        assert_eq!(PADDING_H, 6.0);
    }

    #[test]
    fn the_field_stands_at_the_standard_control_height() {
        assert_eq!(MACOS_CONTROL_HEIGHT, 22.0);
    }

    /// The relationship that makes a Dark Aqua field look right: the well
    /// is *darker* than the window it sits in, not lighter.
    #[test]
    fn an_editable_field_is_a_well_not_a_raised_surface() {
        for theme in [crate::light(), crate::dark()] {
            let p = theme.extension::<MacOsPalette>().copied().unwrap();
            let field = p.text_background.relative_luminance();
            let window = p.window_background.relative_luminance();
            if theme.is_dark() {
                assert!(field < window, "a Dark Aqua field must recede");
            } else {
                // In Aqua the window is already grey, so the well is the
                // *lighter* of the two — the same "different from the
                // chrome" relationship, mirrored.
                assert!(field > window);
            }
        }
    }

    /// A field's frame is a UI component boundary; the fill alone does not
    /// separate it from the window.
    #[test]
    fn the_outline_carries_the_boundary_the_fill_cannot() {
        for theme in [crate::light(), crate::dark()] {
            let c = &theme.colors;
            let fill_only = c.surface_content.contrast_ratio(c.surface_main);
            assert!(
                fill_only < 3.0,
                "the fill alone already separates the field — the outline \
                 lift could be reconsidered"
            );
            let outline = crate::palette::over(c.border_strong, c.surface_content);
            assert!(outline.contrast_ratio(c.surface_content) >= 3.0);
        }
    }

    #[test]
    fn every_validation_level_but_none_tints_something() {
        // A level that resolved to `None` would silently paint an errored
        // field as if it were clean.
        let theme = crate::light();
        let seen: Vec<_> = [
            TextInputValidationLevel::Info,
            TextInputValidationLevel::Warning,
            TextInputValidationLevel::Error,
            TextInputValidationLevel::Corrected,
        ]
        .into_iter()
        .map(|l| tint_of(l, &theme))
        .collect();
        assert!(seen.iter().all(Option::is_some));
        assert!(tint_of(TextInputValidationLevel::None, &theme).is_none());
    }

    /// `validation_color` needs a `PaintContext`, which a unit test cannot
    /// build; this mirrors its mapping against the same theme so a new
    /// level that forgets a case is still caught.
    fn tint_of(level: TextInputValidationLevel, theme: &Theme) -> Option<Color> {
        let c = &theme.colors;
        match level {
            TextInputValidationLevel::None => None,
            TextInputValidationLevel::Info => Some(c.status_info_fg),
            TextInputValidationLevel::Warning => Some(c.border_warning),
            TextInputValidationLevel::Error => Some(c.border_error),
            TextInputValidationLevel::Corrected => Some(c.accent),
        }
    }
}
