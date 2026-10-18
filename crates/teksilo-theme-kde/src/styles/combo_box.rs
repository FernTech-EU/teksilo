// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze combo box.
//!
//! A combo box that cannot be typed into is a push button with an arrow:
//! the push button's frame, face, outline and shadow
//! ([`crate::styles::chrome`]), with "pressed" meaning the popup is open, so
//! an open combo box is filled 30 % toward the highlight and casts no
//! shadow. Its arrow is a 10 px chevron ([`crate::styles::arrow`]) centred in
//! a 20 px box at the trailing edge, 2 px inside it (the frame margin),
//! drawn in the arrow colour of the button text: the button text moved 15 %
//! toward the button colour. While the combo box has keyboard focus and the
//! pointer is not over it the arrow is the window text. The arrow does not
//! change under the pointer, as in Breeze.
//!
//! The combo box is `max(H, 20)` plus the 6 px frame above and below, 32 px
//! at Plasma's default font, through the density ladder, and its label
//! starts 6 px inside the leading edge. It is Breeze's width rule, the label
//! plus 38 px, when nothing proposes a width: the 6 px frame on both sides
//! of the label, the 20 px arrow box and 6 more, laid out as 6 px before the
//! label, 10 px between it and the arrow box and the 2 px frame margin after
//! it ([`crate::styles::metrics::BreezeComboMetrics`]). The gaps scale with
//! the density's spacing; the arrow box and the frame margin are painted
//! and do not. Right to left, the arrow box is at the left edge.
//!
//! ## Where this departs from Breeze
//!
//! - **D5, keyboard focus.** As on a push button, keyboard focus adds the
//!   2 px focus frame in the opaque `focus_ring` token, from 1 px inside to
//!   1 px outside the node, besides the highlight outline Breeze draws.
//! - **The arrow on any scheme.** The arrow colour is lifted in luma to the
//!   non-text floor (3:1, 4.5:1 under high contrast) on the face at rest and
//!   on the open combo box's pressed face, which a scheme whose inactive
//!   effects move the button colour toward the button text needs; stock
//!   Breeze's arrow is kept.
//! - **An empty combo box keeps its arrow colour.** Breeze draws the arrow
//!   of a combo box with no items in the disabled colours; the style is not
//!   told how many items there are, so only a disabled combo box's arrow is
//!   in them.
//!
//! ## Variants
//!
//! Breeze has one combo box that cannot be typed into. `Outlined` and
//! `Filled` are it. `Underline` (`[unpublished]`) draws only the bottom edge
//! of a line edit's outline, in its rest, hover and focus colours, the
//! popup counting as focus; `Plain` (`[unpublished]`) draws no frame, for a
//! combo box whose surroundings are its frame. Both keep the arrow and the
//! metrics.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    ComboBoxStyle, ComboBoxStyleConfig, ComboBoxVariant, TextInputValidationLevel, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::ColorState;
use teksilo_tokens::Color;

use crate::color::mark_on_faces;
use crate::palette::KdePalette;
use crate::styles::arrow::{ArrowDirection, arrow_color, paint_arrow};
use crate::styles::chrome::{
    BreezeButtonChrome, BreezeButtonColors, BreezeButtonKind, BreezeButtonState,
};
use crate::styles::metrics::{BreezeComboMetrics, breeze_combo_metrics_for};
use crate::styles::text_input::BreezeFieldChrome;

/// Breeze `ComboBoxStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeComboBoxStyle;

impl ComboBoxStyle for BreezeComboBoxStyle {
    fn make_body(&self, cfg: &ComboBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let metrics = breeze_combo_metrics_for(&theme.input, &theme.typography);
        let chrome = match cfg.variant {
            ComboBoxVariant::Outlined | ComboBoxVariant::Filled => {
                // `[Hp]` The push button, the open popup standing for the
                // press. The combo box's focus signal is already its
                // `:focus-visible` one.
                let state = BreezeButtonState::derive(
                    &cfg.is_disabled,
                    &cfg.is_open,
                    &cfg.is_hovered,
                    &cfg.is_focused,
                );
                let show_focus_frame = cfg
                    .is_focused
                    .zip(&cfg.is_disabled)
                    .map(|(focused, disabled)| *focused && !*disabled);
                Some(ctx.add(BreezeButtonChrome::new(
                    BreezeButtonKind::Push,
                    state,
                    show_focus_frame,
                )))
            }
            ComboBoxVariant::Underline => Some(ctx.add(BreezeFieldChrome::new(
                cfg.is_focused.or(&cfg.is_open),
                cfg.is_hovered.clone(),
                cfg.is_disabled.clone(),
                Signal::new(TextInputValidationLevel::None),
                true,
            ))),
            ComboBoxVariant::Plain => None,
        };
        let arrow = ctx.add(BreezeComboArrow {
            is_focused: cfg.is_focused.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_disabled: cfg.is_disabled.clone(),
        });
        ctx.add(BreezeComboLayout {
            metrics,
            chrome,
            label: cfg.selected_label,
            arrow,
        })
    }
}

/// `[Hp]` The colour of a combo box's arrow in `theme`, as it paints in a
/// window that is or is not active, with or without the high-contrast
/// preference, focused or not, enabled or not.
///
/// The arrow colour of the button text, or the window text while focused
/// (and not under the pointer, which the caller decides), or the arrow
/// colour of the disabled button text. Without a [`KdePalette`] the primary
/// text and the content surface stand in for the button text and colour.
pub fn combo_arrow_color(
    theme: &Theme,
    window_active: bool,
    high_contrast: bool,
    focused: bool,
    disabled: bool,
) -> Color {
    let c = &theme.colors;
    if focused && !disabled {
        return c.text_primary;
    }
    let Some(palette) = theme.extension::<KdePalette>() else {
        return if disabled {
            arrow_color(c.text_disabled, c.surface_disabled)
        } else {
            arrow_color(c.text_primary, c.surface_content)
        };
    };
    let state = if disabled {
        ColorState::Disabled
    } else if window_active {
        ColorState::Active
    } else {
        ColorState::Inactive
    };
    let b = palette.breeze_colors(state, high_contrast);
    let arrow = arrow_color(b.button_text, b.button);
    if disabled {
        return arrow;
    }
    // `[wcag]` The arrow lies on the face at rest and on the pressed face
    // while the popup is open: lifted to the non-text floor on both.
    let faces = BreezeButtonColors::resolve(theme, window_active, high_contrast, false);
    mark_on_faces(arrow, &[faces.button, faces.pressed], high_contrast)
}

/// `[S]` Where a combo box's arrow box lies in the node `bounds`: at the
/// trailing edge, inside the frame margin, centred vertically.
pub fn combo_arrow_rect(bounds: Rect, m: &BreezeComboMetrics, rtl: bool) -> Rect {
    let x = if rtl {
        bounds.x + m.padding_trailing
    } else {
        bounds.x + bounds.width - m.padding_trailing - m.arrow_box
    };
    Rect::new(
        x,
        bounds.y + (bounds.height - m.arrow_box) * 0.5,
        m.arrow_box,
        m.arrow_box,
    )
}

/// The arrangement of a Breeze combo box: the frame over the whole node,
/// the label after the leading inset, the arrow box at the trailing edge.
struct BreezeComboLayout {
    metrics: BreezeComboMetrics,
    chrome: Option<WidgetId>,
    label: WidgetId,
    arrow: WidgetId,
}

impl std::fmt::Debug for BreezeComboLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeComboLayout")
            .field("metrics", &self.metrics)
            .finish()
    }
}

impl BreezeComboLayout {
    /// The label's size when it is offered `width`.
    fn label_size(&self, width: Option<f32>, ctx: &LayoutContext) -> Size {
        ctx.child_size(
            self.label,
            SizeProposal {
                width,
                height: None,
            },
        )
        .unwrap_or(Size::new(0.0, 0.0))
    }
}

impl Widget for BreezeComboLayout {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.children()
    }

    fn children(&self) -> Vec<WidgetId> {
        self.chrome
            .into_iter()
            .chain([self.label, self.arrow])
            .collect()
    }

    /// The label plus the chrome when nothing proposes a width, the
    /// proposed width otherwise; the Breeze height, or the label's if it is
    /// taller.
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let chrome = self.metrics.chrome_width();
        let label = self.label_size(proposal.width.map(|w| (w - chrome).max(0.0)), ctx);
        Size::new(
            proposal.width.unwrap_or(label.width + chrome),
            self.metrics.height.max(label.height),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let m = &self.metrics;
        let rtl = ctx.is_rtl();
        let arrow = combo_arrow_rect(bounds, m, rtl);
        let room = (bounds.width - m.chrome_width()).max(0.0);
        let label = self.label_size(Some(room), ctx);
        let label_width = label.width.min(room);
        let label_x = if rtl {
            bounds.x + bounds.width - m.padding_leading - label_width
        } else {
            bounds.x + m.padding_leading
        };
        let label_rect = Rect::new(
            label_x,
            bounds.y + (bounds.height - label.height) * 0.5,
            label_width,
            label.height,
        );
        for child in children.iter_mut() {
            let rect = if Some(child.id) == self.chrome {
                bounds
            } else if child.id == self.label {
                label_rect
            } else {
                arrow
            };
            child.origin = Point::new(rect.x, rect.y);
            child.size = Size::new(rect.width, rect.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Layout only: the walker prunes it and the `ComboBox` names itself.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}

/// The arrow of a Breeze combo box, painted in its 20 px box.
struct BreezeComboArrow {
    is_focused: Signal<bool>,
    is_hovered: Signal<bool>,
    is_disabled: Signal<bool>,
}

impl std::fmt::Debug for BreezeComboArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeComboArrow").finish()
    }
}

impl Widget for BreezeComboArrow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.is_focused, &self.is_hovered, &self.is_disabled] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
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
        let color = combo_arrow_color(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            self.is_focused.get() && !self.is_hovered.get(),
            self.is_disabled.get(),
        );
        paint_arrow(canvas, bounds, ArrowDirection::Down, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the `ComboBox` emits the real node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::metrics::breeze_combo_metrics;
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    /// Plasma's 150 x 32 combo box puts its arrow box at (128, 6).
    #[test]
    fn the_arrow_box_is_where_qt_puts_it() {
        let m = breeze_combo_metrics();
        let b = Rect::new(0.0, 0.0, 150.0, 32.0);
        assert_eq!(
            combo_arrow_rect(b, &m, false),
            Rect::new(128.0, 6.0, 20.0, 20.0)
        );
        assert_eq!(
            combo_arrow_rect(b, &m, true),
            Rect::new(2.0, 6.0, 20.0, 20.0),
            "mirrored right to left"
        );
    }

    /// The arrow is the arrow colour of the button text, and the window
    /// text while focused, in Breeze Light.
    #[test]
    fn the_arrow_colours_are_breezes() {
        let t = crate::light();
        let p = t.extension::<KdePalette>().unwrap();
        let b = p.colors(ColorState::Active);
        let rest = combo_arrow_color(&t, true, false, false, false);
        assert_eq!(rest, arrow_color(b.button_text, b.button));
        assert_eq!(rgba8(rest), [68, 70, 73, 255]);
        let focused = combo_arrow_color(&t, true, false, true, false);
        assert_eq!(rgba8(focused), [35, 38, 41, 255]);
        let d = p.colors(ColorState::Disabled);
        assert_eq!(
            combo_arrow_color(&t, true, false, true, true),
            arrow_color(d.button_text, d.button),
            "disabled wins over focus"
        );
    }

    #[test]
    fn a_theme_without_the_palette_paints_from_its_roles() {
        let t = teksilo_core::presets::intui::light();
        let c = &t.colors;
        assert_eq!(
            combo_arrow_color(&t, true, false, false, false),
            arrow_color(c.text_primary, c.surface_content)
        );
        assert_eq!(
            combo_arrow_color(&t, true, false, false, true),
            arrow_color(c.text_disabled, c.surface_disabled)
        );
    }
}
