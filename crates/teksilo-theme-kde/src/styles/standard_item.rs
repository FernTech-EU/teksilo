// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze list and tree row, `StandardListItem` and `StandardTreeItem`.
//!
//! A row at rest paints nothing. Its selection is a rounded rectangle 2 px
//! inside the row's leading and trailing edges and 1 px inside its top and
//! bottom, the item margins, stroked with a 1 px pen on that rectangle inset
//! by half the pen at radius 5, the frame radius itself
//! ([`selection_radius`]):
//!
//! - **selected:** filled and outlined in the highlight, the row's label,
//!   subtitle and chevron in the text on it
//!   ([`StandardItemStyle::selected_label_role`]);
//! - **hovered and selected:** the highlight lightened 110 %;
//! - **hovered:** the highlight at 30 % over the view, outlined in the
//!   highlight at 80 %;
//! - **disabled:** never hovered; a selected disabled row in the disabled
//!   highlight.
//!
//! Qt picks the colour group by window, so a selection mutes only when the
//! window goes inactive, never when the view loses keyboard focus
//! ([`SelectionEmphasis::WindowActive`]): an inactive window paints the
//! scheme's inactive highlight, for Breeze the window colour tinted toward
//! the selection colour, with the row's label in the window text.
//!
//! A row of text is 24 px at Plasma's default font, a row with an icon or a
//! check box 26 px, as Qt measures them, and a row with a subtitle 42 px,
//! all through the density ladder ([`BreezeRowMetrics`]). The content lies
//! 7 px inside the leading and trailing edges, where Qt puts a row's check
//! box, and 1 px inside the top and bottom. The row arithmetic is the
//! shipped [`RecipeStandardItemStyle`]'s, built with these numbers and its
//! own chrome held off, so the selection painted here is the only one.
//!
//! ## Where this departs from Breeze
//!
//! - **D1, the selection fill.** Breeze writes a selected row in its
//!   selection text; Teksilo's text on the accent is chosen against the
//!   accent (D1 of the colour mapping), so a selected row is filled with
//!   `accent`, which [`TextRole::OnAccent`] is readable on, and a hovered
//!   one is washed with `surface_hover`, the highlight at 30 % over the
//!   view moved until the primary text is readable on it. In the stock
//!   Breeze schemes both are Breeze's own colours.
//! - **Keyboard focus** on the selected row of a focused view adds the 2 px
//!   focus frame (D5) around the selection, as on the other controls;
//!   Breeze draws no frame on a selected row, and Teksilo's rows have no way
//!   to tell the current row from the selected one.
//! - **A pressed row** paints as it does at rest or hovered: Breeze has no
//!   pressed item.

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SelectionEmphasis, StandardItemStyle, StandardItemStyleConfig, Theme};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::KdeColor;
use teksilo_kde_scheme::math::hsv_lighter;
use teksilo_tokens::{Color, CornerRadius, InputTokens, ShapeTokens, TextRole, TypographyTokens};
use teksilo_widgets::styles::{RecipeStandardItemStyle, StandardItemRecipe};

use crate::geometry::SMALL_ICON_SIZE;
use crate::shape::{BREEZE_FOCUS_RING_WIDTH, BREEZE_STROKED_FRAME_RADIUS};
use crate::styles::chrome::paint_focus_frame;
use crate::styles::metrics::{
    BreezeRowMetrics, ROW_HOVER_OUTLINE_ALPHA, ROW_SELECTED_HOVER_LIGHTER, breeze_row_metrics_for,
};

/// The Breeze [`StandardItemRecipe`] at the density of `tokens`, for
/// `typography`: the row arithmetic of [`RecipeStandardItemStyle`] with the
/// dimensions of [`BreezeRowMetrics`].
///
/// The slot gaps and the tree indent are the recipe's own: `StandardListItem`
/// lays them out from its own constants, so a value written here would not
/// reach the row.
pub fn breeze_standard_item_recipe_for(
    tokens: &InputTokens,
    typography: &TypographyTokens,
) -> StandardItemRecipe {
    let m = breeze_row_metrics_for(tokens, typography);
    StandardItemRecipe {
        // `[K]` The small icon size.
        icon_size: SMALL_ICON_SIZE,
        padding_horizontal: m.padding_horizontal,
        padding_vertical: m.padding_vertical,
        min_height_single_line: m.height,
        min_height_two_line: m.height_two_line,
        // `[Hp]` Only the radius of the recipe's own chrome, which this style
        // holds off.
        item_corner_radius: BREEZE_STROKED_FRAME_RADIUS,
        bg_horizontal_inset: m.selection_inset_horizontal,
        focus_ring_width: BREEZE_FOCUS_RING_WIDTH,
        // The selection painted here is the only boundary.
        selection_edge_width: 0.0,
        ..StandardItemRecipe::for_tokens(tokens)
    }
}

/// [`breeze_standard_item_recipe_for`] at the Compact density and the Breeze
/// typography.
pub fn breeze_standard_item_recipe() -> StandardItemRecipe {
    breeze_standard_item_recipe_for(
        &InputTokens::default(),
        &crate::typography::breeze_typography(),
    )
}

/// Breeze `StandardItemStyle`.
///
/// [`Default`] is the Compact density and the Breeze typography; the preset
/// installs [`BreezeStandardItemStyle::for_theme`] of the theme it builds,
/// so the row heights follow the density the theme is projected to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeStandardItemStyle {
    /// The row dimensions this style lays out and reports.
    pub metrics: BreezeRowMetrics,
    /// The recipe the row's content is laid out with.
    pub recipe: StandardItemRecipe,
}

impl BreezeStandardItemStyle {
    /// The style at the density of `tokens`, for `typography`.
    pub fn for_tokens(tokens: &InputTokens, typography: &TypographyTokens) -> Self {
        Self {
            metrics: breeze_row_metrics_for(tokens, typography),
            recipe: breeze_standard_item_recipe_for(tokens, typography),
        }
    }

    /// The style at the density and typography of `theme`.
    pub fn for_theme(theme: &Theme) -> Self {
        Self::for_tokens(&theme.input, &theme.typography)
    }
}

impl Default for BreezeStandardItemStyle {
    fn default() -> Self {
        Self::for_tokens(
            &InputTokens::default(),
            &crate::typography::breeze_typography(),
        )
    }
}

impl StandardItemStyle for BreezeStandardItemStyle {
    fn make_body(&self, cfg: &StandardItemStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let selection = BreezeRowSelection {
            is_selected: cfg.is_selected.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_disabled: cfg.is_disabled.clone(),
            emphasised: self
                .selection_emphasis()
                .emphasised(&cfg.is_focused, &cfg.is_window_active),
            show_focus: cfg
                .is_selected
                .zip3(&cfg.is_focused, &cfg.is_focus_visible)
                .map(|(selected, focused, visible)| *selected && *focused && *visible),
            inset_horizontal: self.metrics.selection_inset_horizontal,
            inset_vertical: self.metrics.selection_inset_vertical,
        };
        // The selection is listed first, so it paints behind the row; it is
        // still a sibling the hit walk visits, and it spans the whole row, so
        // it is taken out of hit testing and a press reaches the row's own
        // controls (a check box) wherever it lands.
        let selection = ctx.add(selection.hit_transparent(true));

        // The recipe's row, with its interaction signals held low so its own
        // chrome never paints under this selection.
        let quiet = Signal::new(false);
        let inner = StandardItemStyleConfig {
            content: cfg.content,
            is_selected: quiet.clone(),
            is_hovered: quiet.clone(),
            is_pressed: quiet,
            is_focused: cfg.is_focused.clone(),
            is_focus_visible: cfg.is_focus_visible.clone(),
            is_disabled: cfg.is_disabled.clone(),
            is_window_active: cfg.is_window_active.clone(),
        };
        let row = RecipeStandardItemStyle::new(self.recipe).make_body(&inner, ctx);
        ctx.add(BreezeRowFrame { selection, row })
    }

    /// `[wcag]` D1: the text on the selection's accent fill.
    fn selected_label_role(&self) -> Option<TextRole> {
        Some(TextRole::OnAccent)
    }

    /// `[qt]` Qt picks a palette colour group per window, so only the window
    /// mutes a selection.
    fn selection_emphasis(&self) -> SelectionEmphasis {
        SelectionEmphasis::WindowActive
    }

    /// [`BreezeRowMetrics::height`]. The style was built at a density, so
    /// `tokens` is not consulted again.
    fn min_height_single_line(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.height
    }

    /// [`BreezeRowMetrics::height_decorated`].
    fn min_height_decorated(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.height_decorated
    }

    /// [`BreezeRowMetrics::height_two_line`].
    fn min_height_two_line(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.height_two_line
    }
}

/// Stacks the selection under the delegated row, stretching both to the
/// frame's bounds, so the selection spans the row's full width.
///
/// A `ZStack` would measure the row at an unspecified width, where the
/// row's `Expand` reports only its padding, and centre a narrow row in a
/// wide list; this places both children at the frame's bounds, as
/// `StandardListItem` places its one root.
struct BreezeRowFrame {
    selection: WidgetId,
    row: WidgetId,
}

impl std::fmt::Debug for BreezeRowFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeRowFrame").finish()
    }
}

impl Widget for BreezeRowFrame {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        vec![self.selection, self.row]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // The row owns the frame's size, its shrink weight and floor
        // included; the selection is decoration.
        ctx.child_layout_response(self.row, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into())
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
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.selection, self.row]
    }
}

/// The colours a Breeze row's selection is painted in, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeRowColors {
    /// `[wcag]` D1: an emphasised selected row's fill, `accent`.
    pub selected: Color,
    /// `[Hp]` `[wcag]` D1: the same row hovered, the highlight lightened
    /// 110 %, `accent_hover` (moved, as D1 moves it, where the text on the
    /// accent would fall short on it).
    pub selected_hover: Color,
    /// `[Hp]` The fill of a selected row in an inactive window: the scheme's
    /// inactive highlight, `surface_selected_inactive`.
    pub selected_inactive: Color,
    /// `[wcag]` D1: a hovered row's wash, `surface_hover`.
    pub hover: Color,
    /// `[Hp]` A selected disabled row's fill, the disabled highlight.
    pub disabled: Color,
}

impl BreezeRowColors {
    /// The colours of `theme`, the theme the paint walker hands a widget,
    /// already projected for its window state.
    pub fn resolve(theme: &Theme) -> BreezeRowColors {
        let c = &theme.colors;
        BreezeRowColors {
            selected: c.accent,
            selected_hover: c.accent_hover,
            selected_inactive: c.surface_selected_inactive,
            hover: c.surface_hover,
            disabled: c.accent_disabled,
        }
    }
}

/// What a row's selection paints: a fill and a 1 px outline, each optional.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowPaint {
    /// The fill of the selection, or nothing.
    pub fill: Option<Color>,
    /// The outline of the selection, or nothing.
    pub outline: Option<Color>,
}

impl RowPaint {
    const NOTHING: RowPaint = RowPaint {
        fill: None,
        outline: None,
    };
}

/// `[Hp]` What a row paints, from `c`: disabled first, then selected (and
/// hovered), then hovered, then nothing. `emphasised` says whether a
/// selected row is drawn in the active highlight.
pub fn row_paint(
    selected: bool,
    hovered: bool,
    disabled: bool,
    emphasised: bool,
    c: &BreezeRowColors,
) -> RowPaint {
    let solid = |fill: Color| RowPaint {
        fill: Some(fill),
        outline: Some(fill),
    };
    if disabled {
        return if selected {
            solid(c.disabled)
        } else {
            RowPaint::NOTHING
        };
    }
    if selected {
        return solid(match (emphasised, hovered) {
            (true, false) => c.selected,
            (true, true) => c.selected_hover,
            (false, false) => c.selected_inactive,
            (false, true) => lighter(c.selected_inactive),
        });
    }
    if hovered {
        // The outline is the highlight of the row's colour group at 80 %;
        // in an inactive window the projected accent is that group's.
        let highlight = if emphasised {
            c.selected
        } else {
            c.selected_inactive
        };
        return RowPaint {
            fill: Some(c.hover),
            outline: Some(highlight.with_alpha(ROW_HOVER_OUTLINE_ALPHA)),
        };
    }
    RowPaint::NOTHING
}

/// `[Hp]` Qt's lighter at 110 %, in KDE's arithmetic.
fn lighter(c: Color) -> Color {
    hsv_lighter(KdeColor::from_color(c), ROW_SELECTED_HOVER_LIGHTER).to_color()
}

/// `[M]` The rectangle a row's selection lies in: the row inset by the item
/// margins.
pub fn selection_rect(bounds: Rect, inset_horizontal: f32, inset_vertical: f32) -> Rect {
    Rect::new(
        bounds.x + inset_horizontal,
        bounds.y + inset_vertical,
        (bounds.width - 2.0 * inset_horizontal).max(0.0),
        (bounds.height - 2.0 * inset_vertical).max(0.0),
    )
}

/// `[measured-qt]` The radius a row's selection is stroked and filled at:
/// the control radius, 5 for Breeze, not the 4.5 of a stroked control frame.
///
/// The specification gives 4.5, the frame radius less half the pen, as for
/// the push button; Plasma 6.8's painted rows say otherwise. Their corner
/// pixels (`item_view_row.selected` and `.hover` in the golden pixel
/// samples, `tests/breeze_reference.rs`) fit a radius of 5 on the stroked
/// rectangle to within 23 levels at every pixel, where 4.5 leaves up to 75:
/// at 4.5 the pixel where the arc crosses the diagonal is 0.58 covered,
/// where Plasma covers 0.22 of it. The push button's corners, painted by
/// the same painter, fit 4.5.
pub fn selection_radius(shape: &ShapeTokens) -> f32 {
    shape.radius_control.max(0.0)
}

/// The selection layer of a Breeze row, painted behind the row's content.
///
/// A leaf rather than a composition: a virtualized list realizes and
/// recycles rows constantly. Sized like `RectWidget`: it takes whatever the
/// proposal offers and reports zero when unbounded.
struct BreezeRowSelection {
    is_selected: Signal<bool>,
    is_hovered: Signal<bool>,
    is_disabled: Signal<bool>,
    emphasised: Signal<bool>,
    show_focus: Signal<bool>,
    inset_horizontal: f32,
    inset_vertical: f32,
}

impl std::fmt::Debug for BreezeRowSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeRowSelection").finish()
    }
}

impl Widget for BreezeRowSelection {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [
            &self.is_selected,
            &self.is_hovered,
            &self.is_disabled,
            &self.emphasised,
            &self.show_focus,
        ] {
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
        let theme = ctx.theme;
        let paint = row_paint(
            self.is_selected.get(),
            self.is_hovered.get(),
            self.is_disabled.get(),
            self.emphasised.get(),
            &BreezeRowColors::resolve(theme),
        );
        let area = selection_rect(bounds, self.inset_horizontal, self.inset_vertical);
        let pen = theme.shape.border_width;
        let half = pen * 0.5;
        let stroked = Rect::new(
            area.x + half,
            area.y + half,
            (area.width - pen).max(0.0),
            (area.height - pen).max(0.0),
        );
        let corner = CornerRadius::uniform(selection_radius(&theme.shape));
        if let Some(fill) = paint.fill
            && fill.a() > 0.0
        {
            canvas.fill_rounded_rect(stroked, corner, fill);
        }
        if let Some(outline) = paint.outline
            && outline.a() > 0.0
            && pen > 0.0
        {
            canvas.stroke_rounded_rect(stroked, corner, outline, pen);
        }
        if self.show_focus.get() && !self.is_disabled.get() {
            paint_focus_frame(canvas, area, theme);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the row announces its own selection.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::styles::PaintState;
    use teksilo_tokens::TargetDensity;
    use teksilo_tokens::contrast::composite_contrast;

    use crate::PlasmaAccent;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    #[test]
    fn the_recipe_carries_breezes_row_metrics() {
        let r = breeze_standard_item_recipe();
        assert_eq!(r.min_height_single_line, 24.0);
        assert_eq!(r.min_height_two_line, 42.0);
        assert_eq!(r.padding_horizontal, 7.0);
        assert_eq!(r.padding_vertical, 1.0);
        assert_eq!(r.icon_size, 16.0);
        assert_eq!(r.bg_horizontal_inset, 2.0);
        assert_eq!(r.selection_edge_width, 0.0);
        // The fields the row lays out from its own constants are left alone.
        let shipped = StandardItemRecipe::default();
        assert_eq!(r.slot_gap, shipped.slot_gap);
        assert_eq!(r.subtitle_slot_gap, shipped.subtitle_slot_gap);
        assert_eq!(r.label_subtitle_gap, shipped.label_subtitle_gap);
        assert_eq!(r.tree_indent_step, shipped.tree_indent_step);
        assert_eq!(BreezeStandardItemStyle::default().recipe, r);
        assert_eq!(
            BreezeStandardItemStyle::for_theme(&crate::light()).recipe,
            r
        );
    }

    /// The floors a row asks for: 24, 26 with a decoration, 42 with a
    /// subtitle at Compact, through the density ladder above it.
    #[test]
    fn the_floors_are_the_row_metrics_at_every_density() {
        for (density, floors) in [
            (TargetDensity::Compact, (24.0, 26.0, 42.0)),
            (TargetDensity::Comfortable, (32.0, 32.0, 42.0)),
            (TargetDensity::Touch, (44.0, 44.0, 44.0)),
        ] {
            let theme = crate::light().with_density(density);
            let style = BreezeStandardItemStyle::for_theme(&theme);
            let t = &theme.input;
            assert_eq!(
                (
                    style.min_height_single_line(t),
                    style.min_height_decorated(t),
                    style.min_height_two_line(t)
                ),
                floors,
                "{density:?}"
            );
        }
    }

    #[test]
    fn the_style_flips_the_label_and_follows_the_window() {
        let s = BreezeStandardItemStyle::default();
        assert_eq!(s.selected_label_role(), Some(TextRole::OnAccent));
        assert_eq!(s.selection_emphasis(), SelectionEmphasis::WindowActive);
    }

    #[test]
    fn a_row_paints_breezes_selection_in_each_state() {
        let c = BreezeRowColors::resolve(&crate::light());
        assert_eq!(row_paint(false, false, false, true, &c), RowPaint::NOTHING);
        assert_eq!(row_paint(false, true, true, true, &c), RowPaint::NOTHING);
        let selected = row_paint(true, false, false, true, &c);
        assert_eq!(selected.fill, Some(c.selected));
        assert_eq!(selected.outline, Some(c.selected), "opaque outline");
        let hovered = row_paint(false, true, false, true, &c);
        assert_eq!(hovered.fill, Some(c.hover));
        assert_eq!(hovered.outline, Some(c.selected.with_alpha(0.8)));
        let selected_hovered = row_paint(true, true, false, true, &c);
        assert_ne!(selected_hovered.fill, Some(c.selected));
        assert_eq!(selected_hovered.fill, selected_hovered.outline);
        assert_eq!(
            row_paint(true, false, false, false, &c).fill,
            Some(c.selected_inactive)
        );
        assert_eq!(row_paint(true, true, true, true, &c).fill, Some(c.disabled));
    }

    /// The colours Plasma painted in a 200 x 24 row: the highlight, its
    /// lightened hover, the 30 % wash and the inactive highlight.
    #[test]
    fn breeze_lights_row_colours_are_plasmas() {
        let light = crate::light();
        let c = BreezeRowColors::resolve(&light);
        assert_eq!(rgba8(c.selected), [61, 174, 233, 255]);
        assert_eq!(
            rgba8(row_paint(true, true, false, true, &c).fill.unwrap()),
            [68, 191, 255, 255],
            "#44bfff"
        );
        let wash = rgba8(c.hover);
        for (got, want) in wash.iter().zip([196u8, 230, 248]) {
            assert!(got.abs_diff(want) <= 1, "{wash:?}, Plasma #c4e6f8");
        }
        let inactive = light.for_paint_state(PaintState {
            window_active: false,
            high_contrast: false,
        });
        let ci = BreezeRowColors::resolve(&inactive);
        assert_eq!(
            rgba8(row_paint(true, false, false, false, &ci).fill.unwrap()),
            [194, 224, 245, 255],
            "#c2e0f5"
        );
    }

    /// The label on a selected row is readable on the fill and its hover,
    /// with every swatch, light and dark.
    #[test]
    fn the_label_is_readable_on_the_selection_with_every_accent() {
        for accent in PlasmaAccent::ALL {
            for theme in [
                crate::light_with_accent(accent.color()),
                crate::dark_with_accent(accent.color()),
            ] {
                let c = BreezeRowColors::resolve(&theme);
                let text = theme.colors.text_on_accent;
                for selected_hover in [false, true] {
                    let fill = row_paint(true, selected_hover, false, true, &c)
                        .fill
                        .unwrap();
                    assert!(
                        composite_contrast(text, fill) >= 4.5,
                        "{accent:?}: {:.2}:1",
                        composite_contrast(text, fill)
                    );
                }
                let wash = row_paint(false, true, false, true, &c).fill.unwrap();
                assert!(composite_contrast(theme.colors.text_primary, wash) >= 4.5);
            }
        }
    }

    #[test]
    fn the_selection_lies_inside_the_item_margins() {
        let r = selection_rect(Rect::new(0.0, 0.0, 200.0, 24.0), 2.0, 1.0);
        assert_eq!((r.x, r.y, r.width, r.height), (2.0, 1.0, 196.0, 22.0));
        let tiny = selection_rect(Rect::new(0.0, 0.0, 3.0, 1.0), 2.0, 1.0);
        assert_eq!((tiny.width, tiny.height), (0.0, 0.0));
    }

    /// Without the palette the colours are the theme's own tokens, so a row
    /// in another theme still paints.
    #[test]
    fn the_colours_are_tokens_and_need_no_palette() {
        let mut bare = crate::light();
        bare.extensions.remove::<crate::KdePalette>();
        assert_eq!(
            BreezeRowColors::resolve(&bare),
            BreezeRowColors::resolve(&crate::light())
        );
    }
}
