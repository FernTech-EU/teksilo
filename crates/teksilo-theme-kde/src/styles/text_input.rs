// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze line edit, the frame a `TextInput` is drawn in.
//!
//! A flat fill of the view background in a 1 px outline stroked on the
//! node inset by half the pen, at radius 4.5, with no shadow and no inset
//! for one. The outline is the input-frame outline: the focus colour while
//! the field has focus, the hover colour under the pointer, otherwise the
//! frame outline. Focus changes only the outline's colour; its width stays
//! 1 px and no band is drawn around the field. A disabled field fills with
//! the disabled view background in the disabled frame outline. The hover
//! outline follows the configuration's hover signal, which `TextInput`
//! itself never sets, so a plain `TextInput` shows only the rest and focus
//! outlines.
//!
//! The field is the body line box plus 14 px, 32 px at Plasma's default
//! font, through the density ladder, and its text starts 6 px inside the
//! frame ([`crate::styles::metrics::breeze_field_metrics_for`]). An app's
//! `TextInput::min_height` replaces that height. `TextInput` has already
//! inset its editor vertically by the time a style sees it, so the style
//! pads only horizontally.
//!
//! ## Where this departs from Breeze
//!
//! - **D6, the outline at rest.** The frame outline is the window colour
//!   moved a fifth of the way to the window text, 1.5:1 on the Breeze Light
//!   window, and the field's view background is no further from the window
//!   (1.1:1), so nothing else marks where the field is. The outline at rest
//!   is `border_strong`, lifted to SC 1.4.11's 3:1 (D6 of the colour
//!   mapping).
//! - **D5, focus.** The focus outline is `border_focused`, the focus colour
//!   lifted to 3:1, which Breeze Light's 61,174,233 does not reach on the
//!   white view. The hover outline is the scheme's own hover colour, which
//!   shows where the pointer is; under high contrast it is lifted to 4.5:1
//!   on the control surfaces and the view, so a hovered field keeps the
//!   boundary the high-contrast outline gives it at rest.
//! - **A short field keeps its frame.** Breeze paints only the fill of a
//!   line edit shorter than the line box plus 12 px. This keeps the frame
//!   at every height, for the reason of D6.
//! - **Validation** (`[unpublished]`): Breeze has none. A field with a
//!   validation level is outlined in its colour, as in the other presets.
//!
//! ## Variants
//!
//! `Outlined` and `Filled` are the line edit (Breeze has one field);
//! `Underline` keeps only the bottom edge of the outline; `Bare` returns the
//! editor untouched, for a control that draws its own frame around it.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    TextInputStyle, TextInputStyleConfig, TextInputValidationLevel, TextInputVariant, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_tokens::{Color, CornerRadius};
use teksilo_widgets::primitives::{MinSize, Padding, ZStack};

use crate::color::high_contrast_boundary;
use crate::palette::KdePalette;
use crate::styles::chrome::frame_radius;
use crate::styles::metrics::breeze_field_metrics_for;

/// Breeze `TextInputStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeTextInputStyle;

impl TextInputStyle for BreezeTextInputStyle {
    fn make_body(&self, cfg: &TextInputStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // The embedded case: the surrounding control draws the frame.
        if cfg.variant == TextInputVariant::Bare {
            return cfg.editor;
        }
        let theme = ctx.theme();
        let m = breeze_field_metrics_for(&theme.input, &theme.typography);
        let chrome = ctx.add(BreezeFieldChrome::new(
            cfg.is_focused.clone(),
            cfg.is_hovered.clone(),
            cfg.is_disabled.clone(),
            cfg.validation.clone(),
            cfg.variant == TextInputVariant::Underline,
        ));
        let padded = ctx.add(
            Padding::new(0.0, m.padding_horizontal, 0.0, m.padding_horizontal).child(cfg.editor),
        );
        let stack = ctx.add(ZStack::new().child(chrome).child(padded));
        // An app's `TextInput::min_height` (or a styled field's floor) wins.
        ctx.add(MinSize::new(0.0, cfg.min_height.unwrap_or(m.height)).child(stack))
    }
}

/// The colours a Breeze field is painted from, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeFieldColors {
    /// `[qt]` The view background; the disabled one for a disabled field.
    pub fill: Color,
    /// `[wcag]` D6: the outline at rest, `border_strong`; `border_disabled`
    /// for a disabled field.
    pub outline: Color,
    /// `[Hp]` The outline under the pointer: the view's hover decoration,
    /// lifted to 4.5:1 on the control surfaces and the fill under high
    /// contrast, where it stands in for the lifted outline.
    pub hover: Color,
    /// `[wcag]` D5: the outline while focused, `border_focused`.
    pub focus: Color,
}

impl BreezeFieldColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference, for an enabled
    /// or a disabled field.
    ///
    /// Without a [`KdePalette`] the disabled fill is `surface_disabled` and
    /// the hover outline `border_focused`.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeFieldColors {
        let c = &theme.colors;
        let palette = theme.extension::<KdePalette>();
        let state = if disabled {
            ColorState::Disabled
        } else if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let fill = match (disabled, palette) {
            (false, _) => c.surface_content,
            (true, Some(p)) => p.role(ColorState::Disabled, PaletteRole::Base),
            (true, None) => c.surface_disabled,
        };
        let hover = palette.map_or(c.border_focused, |p| {
            p.breeze_colors(state, high_contrast).hover
        });
        BreezeFieldColors {
            fill,
            outline: if disabled {
                c.border_disabled
            } else {
                c.border_strong
            },
            hover: high_contrast_boundary(hover, c, fill, high_contrast),
            focus: c.border_focused,
        }
    }

    /// `[Hp]` The outline in a state: disabled first, then a validation
    /// colour, then focus, then hover, then rest.
    pub fn outline_for(
        &self,
        focused: bool,
        hovered: bool,
        disabled: bool,
        validation: Option<Color>,
    ) -> Color {
        if disabled {
            self.outline
        } else if let Some(tint) = validation {
            tint
        } else if focused {
            self.focus
        } else if hovered {
            self.hover
        } else {
            self.outline
        }
    }
}

/// The tint a validation level outlines a field in, or `None` for
/// [`TextInputValidationLevel::None`].
pub fn validation_color(level: TextInputValidationLevel, theme: &Theme) -> Option<Color> {
    let c = &theme.colors;
    match level {
        TextInputValidationLevel::None => None,
        TextInputValidationLevel::Info => Some(c.status_info_fg),
        TextInputValidationLevel::Warning => Some(c.border_warning),
        TextInputValidationLevel::Error => Some(c.border_error),
        TextInputValidationLevel::Corrected => Some(c.accent),
    }
}

/// `[Hp]` The rectangle a field's frame is filled and stroked on: the node
/// inset by half the pen. A field casts no shadow, so there is no further
/// inset.
pub fn field_frame_rect(bounds: Rect, pen: f32) -> Rect {
    let d = pen * 0.5;
    Rect::new(
        bounds.x + d,
        bounds.y + d,
        (bounds.width - 2.0 * d).max(0.0),
        (bounds.height - 2.0 * d).max(0.0),
    )
}

/// The frame layer of a Breeze field, painted behind the editor.
///
/// Sized like `RectWidget`: it takes whatever the proposal offers, so it can
/// sit in a `ZStack` without changing the field's size.
pub struct BreezeFieldChrome {
    is_focused: Signal<bool>,
    is_hovered: Signal<bool>,
    is_disabled: Signal<bool>,
    validation: Signal<TextInputValidationLevel>,
    underline: bool,
    fill: Option<ColorProp>,
}

impl BreezeFieldChrome {
    /// A field frame outlined by the focus, hover and disabled states and a
    /// validation level; with `underline`, only its bottom edge is outlined.
    ///
    /// The spin box and an editable rich-text editor frame their fields with
    /// it, and a combo box's `Underline` variant draws its bottom edge.
    pub fn new(
        is_focused: Signal<bool>,
        is_hovered: Signal<bool>,
        is_disabled: Signal<bool>,
        validation: Signal<TextInputValidationLevel>,
        underline: bool,
    ) -> Self {
        Self {
            is_focused,
            is_hovered,
            is_disabled,
            validation,
            underline,
            fill: None,
        }
    }

    /// Fill the frame with `fill` in place of the view background, as a
    /// `RichTextEditor::background` asks; the outline is unchanged.
    pub fn fill(mut self, fill: impl Into<ColorProp>) -> Self {
        self.fill = Some(fill.into());
        self
    }
}

impl std::fmt::Debug for BreezeFieldChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeFieldChrome")
            .field("underline", &self.underline)
            .finish()
    }
}

impl Widget for BreezeFieldChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.is_focused, &self.is_hovered, &self.is_disabled] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        self.validation
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        if let Some(fill) = &self.fill {
            fill.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }
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
        let disabled = self.is_disabled.get();
        let colors = BreezeFieldColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            disabled,
        );
        let pen = ctx.theme.shape.border_width;
        let rect = field_frame_rect(bounds, pen);
        let corner = CornerRadius::uniform(frame_radius(&ctx.theme.shape));
        let fill = self
            .fill
            .as_ref()
            .map_or(colors.fill, |f| f.resolve(ctx.theme, !disabled));
        if fill.a() > 0.0 {
            canvas.fill_rounded_rect(rect, corner, fill);
        }
        let outline = colors.outline_for(
            self.is_focused.get(),
            self.is_hovered.get(),
            disabled,
            validation_color(self.validation.get(), ctx.theme),
        );
        if outline.a() > 0.0 && pen > 0.0 {
            if self.underline {
                canvas.draw_border_bottom(bounds, outline, pen);
            } else {
                canvas.stroke_rounded_rect(rect, corner, outline, pen);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the `TextInput` emits the field's node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    #[test]
    fn focus_wins_over_hover_and_disabled_over_both() {
        let c = BreezeFieldColors::resolve(&crate::light(), true, false, false);
        assert_eq!(c.outline_for(false, false, false, None), c.outline);
        assert_eq!(c.outline_for(false, true, false, None), c.hover);
        assert_eq!(c.outline_for(true, true, false, None), c.focus);
        let red = Color::from_hex("#FF0000");
        assert_eq!(c.outline_for(true, true, false, Some(red)), red);
        assert_eq!(c.outline_for(true, true, true, Some(red)), c.outline);
    }

    /// The Breeze Light colours, against Plasma's painted samples.
    #[test]
    fn breeze_light_paints_the_specifications_colours() {
        let t = crate::light();
        let c = BreezeFieldColors::resolve(&t, true, false, false);
        assert_eq!(rgba8(c.fill), [255, 255, 255, 255]);
        assert_eq!(rgba8(c.hover), [61, 174, 233, 255]);
        assert_eq!(c.outline, t.colors.border_strong);
        assert_eq!(c.focus, t.colors.border_focused);
        let d = BreezeFieldColors::resolve(&t, true, false, true);
        assert_eq!(rgba8(d.fill), [243, 243, 243, 255]);
        assert_eq!(d.outline, t.colors.border_disabled);
    }

    #[test]
    fn the_rest_and_focus_outlines_are_lifted_and_their_premises_hold() {
        for t in [crate::light(), crate::dark()] {
            let p = t.extension::<KdePalette>().unwrap();
            let b = p.colors(ColorState::Active);
            let view = t.colors.surface_content;
            let window = t.colors.surface_main;
            assert!(
                b.frame_outline.contrast_ratio(view) < 3.0,
                "premise: Breeze's frame outline falls short on the view"
            );
            let c = BreezeFieldColors::resolve(&t, true, false, false);
            assert!(c.outline.contrast_ratio(view) >= 3.0);
            assert!(c.outline.contrast_ratio(window) >= 3.0);
            assert!(c.focus.contrast_ratio(view) >= 3.0);
        }
        // Breeze Light's focus colour itself falls short on the white view.
        let t = crate::light();
        let focus = t
            .extension::<KdePalette>()
            .unwrap()
            .colors(ColorState::Active)
            .focus;
        assert!(focus.contrast_ratio(t.colors.surface_content) < 3.0);
    }

    #[test]
    fn the_frame_lies_inside_the_node() {
        let r = field_frame_rect(Rect::new(0.0, 0.0, 150.0, 32.0), 1.0);
        assert_eq!(r, Rect::new(0.5, 0.5, 149.0, 31.0));
        assert_eq!(frame_radius(&crate::light().shape), 4.5);
    }

    #[test]
    fn every_validation_level_but_none_tints_the_outline() {
        let t = crate::light();
        for level in [
            TextInputValidationLevel::Info,
            TextInputValidationLevel::Warning,
            TextInputValidationLevel::Error,
            TextInputValidationLevel::Corrected,
        ] {
            assert!(validation_color(level, &t).is_some(), "{level:?}");
        }
        assert!(validation_color(TextInputValidationLevel::None, &t).is_none());
    }

    #[test]
    fn a_theme_without_the_palette_paints_from_its_roles() {
        let t = teksilo_core::presets::intui::light();
        let c = BreezeFieldColors::resolve(&t, true, false, false);
        assert_eq!(c.fill, t.colors.surface_content);
        assert_eq!(c.hover, t.colors.border_focused);
        let d = BreezeFieldColors::resolve(&t, true, false, true);
        assert_eq!(d.fill, t.colors.surface_disabled);
    }
}
