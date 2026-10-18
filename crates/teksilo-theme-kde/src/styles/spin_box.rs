// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze spin box.
//!
//! A line edit ([`crate::styles::text_input`]) with a column of two arrows
//! at its trailing edge. The frame is the line edit's: the view colour in a
//! 1 px outline, the focus colour while the field has focus, the hover
//! colour under the pointer. The column is 20 px wide, 2 px inside the
//! trailing edge (the frame margin), and `min(inner height, 20)` tall,
//! centred, the inner height being the frame less the 6 px frame above and
//! below; it is split into an upper and a lower half, each half the column
//! rounded down, 10 and 10 at Plasma's default font
//! ([`crate::styles::metrics::BreezeSpinMetrics`]). Each half holds a
//! chevron ([`crate::styles::arrow`]), up and down, in the arrow colour of
//! the view's text, the hover colour on the half under the pointer. Breeze
//! draws no button in the column: no fill, no outline, no press.
//!
//! The spin box is the line edit's inner height plus the 6 px frame above
//! and below, 32 px at Plasma's default font, through the density ladder.
//! The field starts 6 px inside the leading edge and ends at the column.
//! Right to left, the column is at the left edge.
//!
//! The column's halves are the `SpinBox`'s own step buttons, which keep
//! their press, hold-to-repeat and pointer handling; the style places them
//! at Breeze's geometry and draws their arrows itself, painting the step
//! buttons' own chrome at zero opacity.
//!
//! ## Where this departs from Breeze
//!
//! - **D6, the outline at rest.** As on the line edit, the frame is
//!   outlined in `border_strong` rather than in the frame outline, which is
//!   1.5:1 on the Breeze Light window; focus is `border_focused` (D5).
//! - **The hovered arrow under high contrast.** The chevron is a half's
//!   only visual, and the view's hover decoration it turns to is 2.5:1 on
//!   the Breeze Light view. Under high contrast it is lifted to 4.5:1 on the
//!   view and the other control surfaces, as a hovered field's outline is;
//!   in the normal projection it is the scheme's own.
//! - **An arrow keeps its colour at the limit.** Breeze draws the arrow
//!   that can step no further in the disabled colours. The style is not told
//!   the value's limits, so only a disabled spin box's arrows are in them;
//!   an arrow at the limit does nothing when pressed.
//! - **The column's halves are under the target size.** Two 20 x 10
//!   halves are Breeze's geometry, below WCAG 2.2 SC 2.5.8's 24 px at every
//!   density. The value is reachable with the arrow and page keys, the
//!   mouse wheel and the assistive Increment and Decrement actions, the
//!   criterion's "equivalent" exception.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    ButtonLayout, SpinBoxStyle, SpinBoxStyleConfig, TextInputValidationLevel, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_tokens::Color;

use crate::color::high_contrast_boundary;
use crate::palette::KdePalette;
use crate::styles::arrow::{ArrowDirection, arrow_color, paint_arrow};
use crate::styles::metrics::{BreezeSpinMetrics, breeze_spin_metrics_for};
use crate::styles::text_input::BreezeFieldChrome;

/// Breeze `SpinBoxStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeSpinBoxStyle;

impl SpinBoxStyle for BreezeSpinBoxStyle {
    fn make_body(&self, cfg: &SpinBoxStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme();
        let metrics = breeze_spin_metrics_for(&theme.input, &theme.typography);
        // The frame turns to the hover colour while the pointer is anywhere
        // over the spin box, which is over one of its parts.
        let hovered = Signal::new(false);
        let chrome = ctx.add(BreezeFieldChrome::new(
            cfg.is_focused.clone(),
            hovered.clone(),
            cfg.is_disabled.clone(),
            Signal::new(TextInputValidationLevel::None),
            false,
        ));
        let column = match (cfg.layout, cfg.step_up, cfg.step_down) {
            (ButtonLayout::Stacked, Some(up), Some(down)) => {
                let halves = [up, down].map(|step| {
                    // The step button keeps its handlers and its node; its
                    // own background and icon are not Breeze's.
                    ctx.set_opacity(step, 0.0);
                    let hover = Signal::new(false);
                    let half = ctx.add(BreezeSpinHalf { step }.hover_within(hover.clone()));
                    (half, hover)
                });
                let [(up_half, up_hover), (down_half, down_hover)] = halves;
                Some(ctx.add(BreezeSpinArrows {
                    up: up_half,
                    down: down_half,
                    up_hovered: up_hover,
                    down_hovered: down_hover,
                    is_disabled: cfg.is_disabled.clone(),
                }))
            }
            _ => None,
        };
        let layout = BreezeSpinLayout {
            metrics,
            chrome,
            field: cfg.field,
            column,
        };
        ctx.add(layout.hover_within(hovered))
    }
}

/// The colours a Breeze spin box's arrows are painted in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeSpinArrowColors {
    /// `[Hp]` An arrow at rest: the arrow colour of the view's text, on the
    /// view; of the disabled view's text for a disabled spin box.
    pub arrow: Color,
    /// `[Hp]` The arrow of the half under the pointer: the view's hover
    /// decoration, lifted under high contrast to 4.5:1 on the view and the
    /// other control surfaces.
    pub hover: Color,
}

impl BreezeSpinArrowColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference, for an enabled
    /// or a disabled spin box.
    ///
    /// Without a [`KdePalette`] the arrow is the primary text over the
    /// content surface (the disabled text over the disabled surface) and the
    /// hover colour `border_focused`.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeSpinArrowColors {
        let c = &theme.colors;
        let Some(palette) = theme.extension::<KdePalette>() else {
            let arrow = if disabled {
                arrow_color(c.text_disabled, c.surface_disabled)
            } else {
                arrow_color(c.text_primary, c.surface_content)
            };
            return BreezeSpinArrowColors {
                arrow,
                hover: c.border_focused,
            };
        };
        let state = if disabled {
            ColorState::Disabled
        } else if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        // The view's text and background: the projected tokens, which the
        // high-contrast projection lifts, or the disabled view's.
        let arrow = if disabled {
            arrow_color(
                palette.role(ColorState::Disabled, PaletteRole::Text),
                palette.role(ColorState::Disabled, PaletteRole::Base),
            )
        } else {
            arrow_color(c.editor_fg, c.surface_content)
        };
        // `[wcag]` The hovered half's chevron is its only visual: under high
        // contrast the hover colour is lifted to 4.5:1 on the view it is
        // drawn on and the other control surfaces, as a hovered field's
        // outline is.
        BreezeSpinArrowColors {
            arrow,
            hover: high_contrast_boundary(
                palette.breeze_colors(state, high_contrast).hover,
                c,
                c.surface_content,
                high_contrast,
            ),
        }
    }
}

/// `[S]` The arrow column of a spin box whose node is `bounds`: 20 px wide
/// at the trailing edge, inside the frame margin, the full height.
pub fn spin_column_rect(bounds: Rect, m: &BreezeSpinMetrics, rtl: bool) -> Rect {
    let x = if rtl {
        bounds.x + m.padding_trailing
    } else {
        bounds.x + bounds.width - m.padding_trailing - m.arrow_width
    };
    Rect::new(x, bounds.y, m.arrow_width, bounds.height)
}

/// `[S]` The upper and lower halves of the arrow column `column`, centred in
/// it.
pub fn spin_halves(column: Rect) -> (Rect, Rect) {
    let half = BreezeSpinMetrics::arrow_half(column.height);
    let top = column.y + (column.height - 2.0 * half) * 0.5;
    (
        Rect::new(column.x, top, column.width, half),
        Rect::new(column.x, top + half, column.width, half),
    )
}

/// The arrangement of a Breeze spin box: the frame over the whole node, the
/// field from the leading inset to the column, the column at the trailing
/// edge.
struct BreezeSpinLayout {
    metrics: BreezeSpinMetrics,
    chrome: WidgetId,
    field: WidgetId,
    column: Option<WidgetId>,
}

impl std::fmt::Debug for BreezeSpinLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeSpinLayout")
            .field("metrics", &self.metrics)
            .finish()
    }
}

impl BreezeSpinLayout {
    /// Everything the frame adds to the field's width.
    fn chrome_width(&self) -> f32 {
        let m = &self.metrics;
        match self.column {
            Some(_) => m.padding_leading + m.arrow_width + m.padding_trailing,
            // Without the column the field is inset on both sides.
            None => 2.0 * m.padding_leading,
        }
    }
}

impl Widget for BreezeSpinLayout {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.children()
    }

    fn children(&self) -> Vec<WidgetId> {
        [self.chrome, self.field]
            .into_iter()
            .chain(self.column)
            .collect()
    }

    /// The field plus the chrome when nothing proposes a width, the
    /// proposed width otherwise; the Breeze height, or the field's if it is
    /// taller.
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let chrome = self.chrome_width();
        let field = ctx
            .child_size(
                self.field,
                SizeProposal {
                    width: proposal.width.map(|w| (w - chrome).max(0.0)),
                    height: None,
                },
            )
            .unwrap_or(Size::new(0.0, 0.0));
        Size::new(
            proposal.width.unwrap_or(field.width + chrome),
            self.metrics.height.max(field.height),
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
        let column = spin_column_rect(bounds, m, rtl);
        let field_width = (bounds.width - self.chrome_width()).max(0.0);
        let field_x = if rtl {
            bounds.x + bounds.width - m.padding_leading - field_width
        } else {
            bounds.x + m.padding_leading
        };
        let field = Rect::new(field_x, bounds.y, field_width, bounds.height);
        for child in children.iter_mut() {
            let rect = if child.id == self.chrome {
                bounds
            } else if child.id == self.field {
                field
            } else {
                column
            };
            child.origin = Point::new(rect.x, rect.y);
            child.size = Size::new(rect.width, rect.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Layout only: the `SpinBox` field carries the spin button's node.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}

/// One half of the arrow column: holds a step button and gives it the whole
/// half, so its press region is Breeze's 20 x 10.
struct BreezeSpinHalf {
    step: WidgetId,
}

impl std::fmt::Debug for BreezeSpinHalf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeSpinHalf").finish()
    }
}

impl Widget for BreezeSpinHalf {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![self.step]
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.step]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}

/// The arrow column of a Breeze spin box: places the two halves holding the
/// step buttons and draws their arrows.
struct BreezeSpinArrows {
    up: WidgetId,
    down: WidgetId,
    up_hovered: Signal<bool>,
    down_hovered: Signal<bool>,
    is_disabled: Signal<bool>,
}

impl std::fmt::Debug for BreezeSpinArrows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeSpinArrows").finish()
    }
}

impl Widget for BreezeSpinArrows {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.up_hovered, &self.down_hovered, &self.is_disabled] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        vec![self.up, self.down]
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.up, self.down]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let (up, down) = spin_halves(bounds);
        for child in children.iter_mut() {
            let rect = if child.id == self.up { up } else { down };
            child.origin = Point::new(rect.x, rect.y);
            child.size = Size::new(rect.width, rect.height);
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let disabled = self.is_disabled.get();
        let colors = BreezeSpinArrowColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            disabled,
        );
        let (up, down) = spin_halves(bounds);
        let color = |hovered: bool| {
            if hovered && !disabled {
                colors.hover
            } else {
                colors.arrow
            }
        };
        paint_arrow(canvas, up, ArrowDirection::Up, color(self.up_hovered.get()));
        paint_arrow(
            canvas,
            down,
            ArrowDirection::Down,
            color(self.down_hovered.get()),
        );
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Layout and paint only: the step buttons are the `SpinBox`'s own.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::metrics::breeze_spin_metrics;
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    /// Plasma's 100 x 32 spin box: the up half at (78, 6) and the down half
    /// at (78, 16), each 20 x 10.
    #[test]
    fn the_column_and_its_halves_are_where_qt_puts_them() {
        let m = breeze_spin_metrics();
        let b = Rect::new(0.0, 0.0, 100.0, 32.0);
        let column = spin_column_rect(b, &m, false);
        assert_eq!(column, Rect::new(78.0, 0.0, 20.0, 32.0));
        let (up, down) = spin_halves(column);
        assert_eq!(up, Rect::new(78.0, 6.0, 20.0, 10.0));
        assert_eq!(down, Rect::new(78.0, 16.0, 20.0, 10.0));
        assert_eq!(
            spin_column_rect(b, &m, true),
            Rect::new(2.0, 0.0, 20.0, 32.0),
            "mirrored right to left"
        );
    }

    /// A taller frame keeps the 20 px column, centred.
    #[test]
    fn a_touch_frame_centres_the_same_halves() {
        let (up, down) = spin_halves(Rect::new(0.0, 0.0, 20.0, 44.0));
        assert_eq!(up, Rect::new(0.0, 12.0, 20.0, 10.0));
        assert_eq!(down, Rect::new(0.0, 22.0, 20.0, 10.0));
    }

    #[test]
    fn the_arrows_are_the_views_text_and_hover_colours() {
        let t = crate::light();
        let c = BreezeSpinArrowColors::resolve(&t, true, false, false);
        assert_eq!(
            c.arrow,
            arrow_color(t.colors.editor_fg, t.colors.surface_content)
        );
        assert_eq!(rgba8(c.hover), [61, 174, 233, 255]);
        let p = t.extension::<KdePalette>().unwrap();
        let d = BreezeSpinArrowColors::resolve(&t, true, false, true);
        assert_eq!(
            d.arrow,
            arrow_color(
                p.role(ColorState::Disabled, PaletteRole::Text),
                p.role(ColorState::Disabled, PaletteRole::Base),
            )
        );
    }

    #[test]
    fn a_theme_without_the_palette_paints_from_its_roles() {
        let t = teksilo_core::presets::intui::light();
        let c = BreezeSpinArrowColors::resolve(&t, true, false, false);
        assert_eq!(
            c.arrow,
            arrow_color(t.colors.text_primary, t.colors.surface_content)
        );
        assert_eq!(c.hover, t.colors.border_focused);
    }
}
