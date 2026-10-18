// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze table: `TableView` and `TreeTableView`, their rows and their
//! column header.
//!
//! A header section is filled with the button colour, moved 20 % toward the
//! hover colour under the pointer, and toward the focus colour while its
//! column is being resized (Breeze's pressed section) or while it is the
//! column sorted down, with a 1 px line along its bottom and its trailing
//! edge in the window text at the frame contrast.
//!
//! Sorted down is Qt's indicator, not the arrow Breeze draws: Qt's header
//! view marks a column sorted in ascending order with its down indicator,
//! and Breeze, which inverts the sort indicator by default
//! (`ViewInvertSortIndicator`), draws that column's arrow pointing up. So
//! the ascending column is the one tinted, under an up arrow, and a
//! descending one keeps the button colour under its down arrow. Its label lies 6 px inside it, the header margin. The header
//! row is 30 px at Plasma's default font, room for the 10 px sort arrow,
//! through the density ladder.
//!
//! A body row is 24 px (D7, below). Table cells are square: a selected row
//! is the highlight across the row, a hovered one the highlight at 30 %
//! over the view, an alternate row the view's alternate background.
//!
//! The views paint their own selection band, alternate rows and grid lines
//! from the theme's tokens in one pass over the visible rows, and draw their
//! own sort arrow, so [`TableStyle::make_row_background`] and
//! [`TableStyle::make_sort_indicator`] are Breeze's for a style author who
//! composes them, and the band the views paint is the `surface_selected`
//! token; the crate's known limitations say where that differs from Breeze.
//!
//! ## Where this departs from Breeze
//!
//! - **D7, the row height.** Qt lays a Breeze table row out at the line box
//!   and its item margins and padding and adds its 1 px grid line: 23 px at
//!   Plasma's default font, under WCAG 2.2 SC 2.5.8's 24 px target. The row
//!   is raised to 24 px, then through the density ladder; at a larger font
//!   Qt's own row, already taller, is kept.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    SortDirection, TableGridRecipe, TableHeaderCellConfig, TableRowConfig, TableStyle, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_tokens::{Color, InputTokens, TypographyTokens};
use teksilo_widgets::primitives::ZStack;

use crate::color::{face_under_primary_text, mark_on_faces};
use crate::palette::KdePalette;
use crate::styles::arrow::{ArrowDirection, arrow_color, paint_arrow};
use crate::styles::chrome::kde_mix;
use crate::styles::metrics::{BreezeTableMetrics, ROW_HOVER_ALPHA, breeze_table_metrics_for};

/// `[Hp]` How far a hovered or pressed header section moves from the button
/// colour toward the hover or focus colour.
pub const HEADER_STATE_MIX: f64 = 0.2;
/// `[Hp]` The width of a header section's bottom and trailing lines.
pub const HEADER_LINE_WIDTH: f32 = 1.0;

/// Breeze `TableStyle`.
///
/// [`Default`] is the Compact density and the Breeze typography; the preset
/// installs [`BreezeTableStyle::for_theme`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeTableStyle {
    /// The table dimensions this style reports.
    pub metrics: BreezeTableMetrics,
}

impl BreezeTableStyle {
    /// The style at the density of `tokens`, for `typography`.
    pub fn for_tokens(tokens: &InputTokens, typography: &TypographyTokens) -> Self {
        Self {
            metrics: breeze_table_metrics_for(tokens, typography),
        }
    }

    /// The style at the density and typography of `theme`.
    pub fn for_theme(theme: &Theme) -> Self {
        Self::for_tokens(&theme.input, &theme.typography)
    }
}

impl Default for BreezeTableStyle {
    fn default() -> Self {
        Self::for_tokens(
            &InputTokens::default(),
            &crate::typography::breeze_typography(),
        )
    }
}

impl TableStyle for BreezeTableStyle {
    fn make_header_cell(&self, cfg: &TableHeaderCellConfig, ctx: &mut BuildContext) -> WidgetId {
        let section = ctx.add(BreezeHeaderSection {
            is_hovered: cfg.is_hovered.clone(),
            is_resizing: cfg.is_resizing.clone(),
            sorted_down: sorts_down(cfg.sort),
        });
        ctx.add(ZStack::new().child(section).child(cfg.label))
    }

    fn make_sort_indicator(&self, direction: SortDirection, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(BreezeSortArrow { direction })
    }

    fn make_row_background(&self, cfg: &TableRowConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(BreezeTableRowBackground {
            is_selected: cfg.is_selected.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_alt: cfg.is_alt,
            // `[qt]` A selection mutes with its window only.
            is_window_active: cfg.is_window_active.clone(),
        })
    }

    /// `[unpublished]` The specification records no grid-line colour of its
    /// own: the framework's 1 px lines in the divider role, Breeze's frame
    /// outline, and its frozen-column shadow.
    fn grid(&self) -> TableGridRecipe {
        TableGridRecipe::default()
    }

    /// [`BreezeTableMetrics::header_padding`]. The style was built at a
    /// density, so `tokens` is not consulted again.
    fn cell_padding_horizontal(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.header_padding
    }

    /// [`BreezeTableMetrics::header_padding`]: the label's line box and the
    /// header margin above and below fill the header.
    fn cell_padding_vertical(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.header_padding
    }

    /// [`BreezeTableMetrics::row_height`], 24 at Compact (D7).
    fn row_height(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.row_height
    }

    /// [`BreezeTableMetrics::header_height`], 30 at Compact.
    fn header_height(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.header_height
    }
}

/// The colours a Breeze header section is painted in, in one paint state.
///
/// The section carries its label's primary text at the paint state's text
/// floor: at rest it is the button colour the colour tokens lift that text
/// on, and hovered or pressed it is moved, as an accent wash is (D1 of the
/// colour mapping), until the text reaches the floor on it. The sort arrow
/// is lifted to the non-text floor on all three. In stock Breeze nothing
/// moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeHeaderColors {
    /// `[qt]` The section at rest: the button colour.
    pub rest: Color,
    /// `[Hp]` Hovered: the button colour moved 20 % toward the hover colour.
    pub hovered: Color,
    /// `[Hp]` Pressed (its column being resized) or sorted down: the button
    /// colour moved 20 % toward the focus colour.
    pub pressed: Color,
    /// `[Hp]` The bottom and trailing lines: the window text at the frame
    /// contrast, composited over the section.
    pub line: Color,
    /// `[Hp]` The sort arrow: the arrow colour of the button text.
    pub arrow: Color,
}

impl BreezeHeaderColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference.
    ///
    /// The button, hover and focus colours come from [`KdePalette`]; without
    /// it the section is the raised surface, its states the hover and pressed
    /// surfaces and its lines the border.
    pub fn resolve(theme: &Theme, window_active: bool, high_contrast: bool) -> BreezeHeaderColors {
        let c = &theme.colors;
        let Some(palette) = theme.extension::<KdePalette>() else {
            return BreezeHeaderColors {
                rest: c.surface_raised,
                hovered: c.surface_hover,
                pressed: c.surface_pressed,
                line: c.border,
                arrow: c.text_secondary,
            };
        };
        let state = if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let b = palette.breeze_colors(state, high_contrast);
        // `[derived]` The frame contrast: 1 in the high-contrast scheme the
        // palette builds, as `KdePalette::breeze_colors` computes it.
        let fc = if high_contrast {
            1.0
        } else {
            palette.frame_contrast() as f32
        };
        // `[wcag]` D1: a section mixed toward the hover or focus colour
        // carries the label's primary text, as the button colour does.
        let under_label = |face: Color| face_under_primary_text(face, c, high_contrast);
        let hovered = under_label(kde_mix(b.button, b.hover, HEADER_STATE_MIX));
        let pressed = under_label(kde_mix(b.button, b.focus, HEADER_STATE_MIX));
        BreezeHeaderColors {
            rest: b.button,
            hovered,
            pressed,
            line: palette.role(state, PaletteRole::WindowText).with_alpha(fc),
            // `[wcag]` The arrow on all three sections.
            arrow: mark_on_faces(
                arrow_color(b.button_text, b.button),
                &[b.button, hovered, pressed],
                high_contrast,
            ),
        }
    }

    /// The section's fill: pressed (or sorted down) over hovered over rest.
    pub fn fill(&self, hovered: bool, pressed: bool) -> Color {
        if pressed {
            self.pressed
        } else if hovered {
            self.hovered
        } else {
            self.rest
        }
    }
}

/// `[qt]` Whether a header sorted in `sort` carries Qt's down indicator,
/// which Breeze paints as a pressed section: Qt's header view marks an
/// ascending column down. Breeze's inverted arrow then points up, as
/// [`TableStyle::make_sort_indicator`] draws it.
pub fn sorts_down(sort: Option<SortDirection>) -> bool {
    sort == Some(SortDirection::Ascending)
}

/// `[Hp]` The bottom line and the trailing separator of a header section in
/// `bounds`, each 1 px, the separator on the left in a right-to-left window.
pub fn header_lines(bounds: Rect, direction: LayoutDirection) -> [Rect; 2] {
    let w = HEADER_LINE_WIDTH;
    let bottom = Rect::new(bounds.x, bounds.y + bounds.height - w, bounds.width, w);
    let x = match direction {
        LayoutDirection::LeftToRight => bounds.x + bounds.width - w,
        LayoutDirection::RightToLeft => bounds.x,
    };
    [bottom, Rect::new(x, bounds.y, w, bounds.height)]
}

/// The chrome of a Breeze header section, painted behind its label.
struct BreezeHeaderSection {
    is_hovered: Signal<bool>,
    is_resizing: Signal<bool>,
    /// Fixed for the build: the header cell is rebuilt when its sort
    /// changes.
    sorted_down: bool,
}

impl std::fmt::Debug for BreezeHeaderSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeHeaderSection").finish()
    }
}

impl Widget for BreezeHeaderSection {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_hovered
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_resizing
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
        let c =
            BreezeHeaderColors::resolve(ctx.theme, ctx.window_active, ctx.prefers_high_contrast);
        canvas.fill_rect(
            bounds,
            c.fill(
                self.is_hovered.get(),
                self.is_resizing.get() || self.sorted_down,
            ),
        );
        for line in header_lines(bounds, ctx.layout_direction) {
            canvas.fill_rect(line, c.line);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the header cell announces its column.
        builder.set_hidden();
    }
}

/// A header's sort arrow: the 10 px Breeze chevron, pointing down for a
/// descending sort and up for an ascending one.
struct BreezeSortArrow {
    direction: SortDirection,
}

impl std::fmt::Debug for BreezeSortArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeSortArrow").finish()
    }
}

impl Widget for BreezeSortArrow {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // `[M]` The header's 10 px arrow.
        let side = crate::geometry::HEADER_ARROW_SIZE;
        teksilo_canvas::Size::new(side, side).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let c =
            BreezeHeaderColors::resolve(ctx.theme, ctx.window_active, ctx.prefers_high_contrast);
        let direction = match self.direction {
            SortDirection::Ascending => ArrowDirection::Up,
            SortDirection::Descending => ArrowDirection::Down,
        };
        paint_arrow(canvas, bounds, direction, c.arrow);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // The header announces its sort direction.
        builder.set_hidden();
    }
}

/// `[Hp]` The fill of a table row: a selected row the highlight (the
/// inactive one in an inactive window), a hovered one the highlight at 30 %
/// over the view, an alternate one the alternate background, otherwise
/// nothing.
pub fn table_row_fill(
    theme: &Theme,
    selected: bool,
    hovered: bool,
    alt: bool,
    window_active: bool,
) -> Option<Color> {
    let c = &theme.colors;
    if selected {
        Some(if window_active {
            c.surface_selected
        } else {
            c.surface_selected_inactive
        })
    } else if hovered {
        Some(c.accent.with_alpha(ROW_HOVER_ALPHA))
    } else if alt {
        Some(c.surface_alt_row)
    } else {
        None
    }
}

/// The background of one Breeze table row, for a style author who composes
/// rows from [`TableStyle::make_row_background`]. Square, as Breeze paints
/// table cells.
struct BreezeTableRowBackground {
    is_selected: Signal<bool>,
    is_hovered: Signal<bool>,
    is_alt: bool,
    is_window_active: Option<Signal<bool>>,
}

impl std::fmt::Debug for BreezeTableRowBackground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeTableRowBackground").finish()
    }
}

impl Widget for BreezeTableRowBackground {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_selected
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        self.is_hovered
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        if let Some(active) = &self.is_window_active {
            active.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let window_active = self
            .is_window_active
            .as_ref()
            .map_or(ctx.window_active, Signal::get);
        if let Some(fill) = table_row_fill(
            ctx.theme,
            self.is_selected.get(),
            self.is_hovered.get(),
            self.is_alt,
            window_active,
        ) {
            canvas.fill_rect(bounds, fill);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;
    use teksilo_tokens::TargetDensity;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    /// Whether two colours are within one step a channel: Qt's raster
    /// painter rounds an 8-bit blend its own way.
    fn near(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    /// `c` composited over the opaque `under`, in 8 bits.
    fn over(c: Color, under: Color) -> [u8; 4] {
        rgba8(kde_mix(under, c.with_alpha(1.0), f64::from(c.a())))
    }

    #[test]
    fn the_heights_and_padding_are_breezes_at_every_density() {
        for (density, row, header) in [
            (TargetDensity::Compact, 24.0, 30.0),
            (TargetDensity::Comfortable, 32.0, 32.0),
            (TargetDensity::Touch, 44.0, 44.0),
        ] {
            let theme = crate::light().with_density(density);
            let s = BreezeTableStyle::for_theme(&theme);
            assert_eq!(s.row_height(&theme.input), row, "{density:?}");
            assert_eq!(s.header_height(&theme.input), header, "{density:?}");
        }
        let s = BreezeTableStyle::default();
        let t = InputTokens::default();
        assert_eq!(s.cell_padding_horizontal(&t), 6.0);
        assert_eq!(s.cell_padding_vertical(&t), 6.0);
        assert_eq!(s.grid(), TableGridRecipe::default());
    }

    /// The header section Plasma painted in Breeze Light: the button colour,
    /// its hover and pressed mixes, and the line in the window text at the
    /// frame contrast over each.
    #[test]
    fn breeze_lights_header_is_plasmas() {
        let c = BreezeHeaderColors::resolve(&crate::light(), true, false);
        assert_eq!(rgba8(c.rest), [252, 252, 252, 255], "#fcfcfc");
        assert_eq!(rgba8(c.hovered), [214, 236, 248, 255], "#d6ecf8");
        assert_eq!(rgba8(c.pressed), [214, 236, 248, 255], "#d6ecf8");
        assert!(near(over(c.line, c.rest), [209, 210, 210, 255]), "#d1d2d2");
        assert!(
            near(over(c.line, c.hovered), [178, 197, 206, 255]),
            "#b2c5ce"
        );
        assert_eq!(c.fill(true, true), c.pressed);
        assert_eq!(c.fill(true, false), c.hovered);
        assert_eq!(c.fill(false, false), c.rest);
    }

    /// In a scheme whose view focus and hover decorations differ, the
    /// pressed section moves toward the focus colour and the hovered one
    /// toward the hover colour. Breeze Light's two are the same colour, so
    /// only such a scheme tells the two mixes apart.
    #[test]
    fn the_pressed_section_mixes_toward_focus_and_the_hovered_toward_hover() {
        let (scheme, diagnostics) = teksilo_kde_scheme::KdeColorScheme::parse(
            "[Colors:View]\nDecorationFocus=61,174,233\nDecorationHover=246,116,0\n",
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let theme = crate::from_scheme(&scheme);
        let b = theme
            .extension::<KdePalette>()
            .unwrap()
            .breeze_colors(ColorState::Active, false);
        // Premise: the scheme's two decorations are the ones written.
        assert_eq!(rgba8(b.focus), [61, 174, 233, 255]);
        assert_eq!(rgba8(b.hover), [246, 116, 0, 255]);
        let c = BreezeHeaderColors::resolve(&theme, true, false);
        assert_eq!(c.pressed, kde_mix(b.button, b.focus, HEADER_STATE_MIX));
        assert_eq!(c.hovered, kde_mix(b.button, b.hover, HEADER_STATE_MIX));
        assert_ne!(c.pressed, c.hovered);
        assert_eq!(c.fill(true, true), c.pressed);
        assert_eq!(c.fill(true, false), c.hovered);
    }

    #[test]
    fn the_ascending_column_is_the_one_sorted_down() {
        assert!(sorts_down(Some(SortDirection::Ascending)));
        assert!(!sorts_down(Some(SortDirection::Descending)));
        assert!(!sorts_down(None));
    }

    #[test]
    fn the_header_lines_run_along_the_bottom_and_the_trailing_edge() {
        let b = Rect::new(10.0, 20.0, 100.0, 30.0);
        let [bottom, side] = header_lines(b, LayoutDirection::LeftToRight);
        assert_eq!(
            (bottom.x, bottom.y, bottom.width, bottom.height),
            (10.0, 49.0, 100.0, 1.0)
        );
        assert_eq!(
            (side.x, side.y, side.width, side.height),
            (109.0, 20.0, 1.0, 30.0)
        );
        let [_, rtl] = header_lines(b, LayoutDirection::RightToLeft);
        assert_eq!(rtl.x, 10.0);
    }

    #[test]
    fn a_table_row_is_filled_by_state() {
        let t = crate::light();
        let c = &t.colors;
        assert_eq!(
            table_row_fill(&t, true, true, true, true),
            Some(c.surface_selected)
        );
        assert_eq!(
            table_row_fill(&t, true, false, false, false),
            Some(c.surface_selected_inactive)
        );
        assert_eq!(
            table_row_fill(&t, false, true, true, true),
            Some(c.accent.with_alpha(0.3))
        );
        assert_eq!(
            table_row_fill(&t, false, false, true, true),
            Some(c.surface_alt_row)
        );
        assert_eq!(table_row_fill(&t, false, false, false, true), None);
    }

    #[test]
    fn the_header_paints_from_the_roles_without_the_palette() {
        let mut bare = crate::light();
        bare.extensions.remove::<KdePalette>();
        let c = BreezeHeaderColors::resolve(&bare, true, false);
        assert_eq!(c.rest, bare.colors.surface_raised);
        assert_eq!(c.line, bare.colors.border);
    }
}
