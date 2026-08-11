// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `TableStyle` impl + table-family design tokens.
//!
//! Design tokens for the table family live as `pub const`s on this
//! module, shared by `TableView` and `TreeTableView` (TreeTableView adds
//! tree-only indent / twist sizing on top of the standard table dims).
//!
//! ## Wiring status
//!
//! The `make_*` trait methods produce reference subtrees for custom
//! styles that want to install themselves via
//! `style_slots.table = Some(...)`. The default IntUI shape is the
//! one `TableView` / `TreeTableView` paint today inline; they continue to
//! own their paint passes for performance reasons (grid lines need a
//! single batched pass over the virtualized viewport — composing one
//! `RectWidget` per line would defeat virtualization). The full
//! chrome decomposition is deferred.

use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    SortDirection, TableGridRecipe, TableHeaderCellConfig, TableRowConfig, TableStyle,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{RectWidget, ZStack};

// ─── IntUI design tokens for TableView / TreeTableView ─────────────────

/// Body row height. Headers use `HEADER_HEIGHT`.
pub const ROW_HEIGHT: f32 = 28.0;
/// Sticky header row height.
pub const HEADER_HEIGHT: f32 = 32.0;
/// Horizontal padding inside each cell (also applied to header cells).
pub const CELL_PADDING_HORIZONTAL: f32 = 8.0;
/// Vertical padding inside each cell.
pub const CELL_PADDING_VERTICAL: f32 = 4.0;
/// Half-width of a column's resize grip on the header strip.
///
/// The grabbable band is centred on the divider and reaches this far into the
/// cell on **each** side of it — 8 dp total, the same `PM_HeaderGripMargin`
/// convention `QHeaderView` uses. A one-sided band would leave the outer half
/// of every divider owned by the neighbouring cell's label, where a grab that
/// missed by a pixel cycles the sort or starts a column-reorder drag instead.
pub const RESIZE_HANDLE_WIDTH: f32 = 4.0;
/// Step, in logical pixels, applied by one assistive-technology
/// `Increment` / `Decrement` on a resizable column header — the non-pointer
/// path to the resize grip.
pub const COLUMN_RESIZE_STEP: f32 = 8.0;
/// Stroke width of grid lines drawn between rows / columns.
pub const GRID_LINE_THICKNESS: f32 = 1.0;
/// Outer-frame corner radius.
pub const CORNER_RADIUS: f32 = 4.0;
/// Edge length of the sort-direction chevron in the header.
pub const SORT_INDICATOR_SIZE: f32 = 10.0;
/// Edge length of the filter glyph in the header.
pub const FILTER_INDICATOR_SIZE: f32 = 12.0;
/// Spacing between adjacent header cells (in addition to grid lines).
pub const HEADER_INTER_CELL_SPACING: f32 = 0.0;
/// Inset between the focused-cell bounds and the focus-ring stroke.
pub const FOCUS_RING_INSET: f32 = 1.0;
/// Default minimum column width, used when a column does not set its own.
pub const MIN_COLUMN_WIDTH_DEFAULT: f32 = 32.0;
/// `TreeTableView` only — pixels per indent level on the tree column.
pub const TREE_INDENT_PER_LEVEL: f32 = 16.0;
/// `TreeTableView` only — edge length of the twist (expand/collapse) chevron.
pub const TREE_TWIST_SIZE: f32 = 12.0;
/// `TreeTableView` only — gap between the twist chevron and the cell content.
pub const TREE_TWIST_LABEL_GAP: f32 = 4.0;

/// Configurable dimensions for [`RecipeTableStyle`].
///
/// All fields default to the corresponding `pub const` in this module so
/// that `TableRecipe::default()` reproduces the IntUI look exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableRecipe {
    /// Body row height. Headers use `header_height`.
    pub row_height: f32,
    /// Sticky header row height.
    pub header_height: f32,
    /// Horizontal padding inside each cell (also applied to header cells).
    pub cell_padding_horizontal: f32,
    /// Vertical padding inside each cell.
    pub cell_padding_vertical: f32,
    /// Half-width of a column's resize grip: the grabbable band reaches this
    /// far on **each** side of a divider (see [`RESIZE_HANDLE_WIDTH`]).
    pub resize_handle_width: f32,
    /// Stroke width of grid lines drawn between rows / columns.
    pub grid_line_thickness: f32,
    /// Outer-frame corner radius.
    pub corner_radius: f32,
    /// Edge length of the sort-direction chevron in the header.
    pub sort_indicator_size: f32,
    /// Edge length of the filter glyph in the header.
    pub filter_indicator_size: f32,
    /// Spacing between adjacent header cells (in addition to grid lines).
    pub header_inter_cell_spacing: f32,
    /// Inset between the focused-cell bounds and the focus-ring stroke.
    pub focus_ring_inset: f32,
    /// Default minimum column width, used when a column does not set its own.
    pub min_column_width_default: f32,
    /// `TreeTableView` only — pixels per indent level on the tree column.
    pub tree_indent_per_level: f32,
    /// `TreeTableView` only — edge length of the twist (expand/collapse) chevron.
    pub tree_twist_size: f32,
    /// `TreeTableView` only — gap between the twist chevron and the cell content.
    pub tree_twist_label_gap: f32,
}

impl Default for TableRecipe {
    fn default() -> Self {
        Self {
            row_height: ROW_HEIGHT,
            header_height: HEADER_HEIGHT,
            cell_padding_horizontal: CELL_PADDING_HORIZONTAL,
            cell_padding_vertical: CELL_PADDING_VERTICAL,
            resize_handle_width: RESIZE_HANDLE_WIDTH,
            grid_line_thickness: GRID_LINE_THICKNESS,
            corner_radius: CORNER_RADIUS,
            sort_indicator_size: SORT_INDICATOR_SIZE,
            filter_indicator_size: FILTER_INDICATOR_SIZE,
            header_inter_cell_spacing: HEADER_INTER_CELL_SPACING,
            focus_ring_inset: FOCUS_RING_INSET,
            min_column_width_default: MIN_COLUMN_WIDTH_DEFAULT,
            tree_indent_per_level: TREE_INDENT_PER_LEVEL,
            tree_twist_size: TREE_TWIST_SIZE,
            tree_twist_label_gap: TREE_TWIST_LABEL_GAP,
        }
    }
}

/// Default `TableStyle` shipped with Teksilo. The trait methods return
/// reference subtrees; the widgets themselves still own their batched
/// paint passes for performance (pending the chrome-decomposition
/// follow-up).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTableStyle {
    pub recipe: TableRecipe,
}

impl RecipeTableStyle {
    pub fn new(recipe: TableRecipe) -> Self {
        Self { recipe }
    }
}

impl TableStyle for RecipeTableStyle {
    fn make_header_cell(&self, cfg: &TableHeaderCellConfig, ctx: &mut BuildContext) -> WidgetId {
        // Reference shape: rounded surface that flips Hover when the
        // pointer is over the cell, Pressed while a resize drag is
        // active. The widget side handles label placement + sort
        // indicator stacking; this is the body the cell sits on.
        let hovered = cfg.is_hovered.clone();
        let resizing = cfg.is_resizing.clone();
        let role: Signal<SurfaceRole> = hovered.zip(&resizing).map(|(h, r)| {
            if *r {
                SurfaceRole::Pressed
            } else if *h {
                SurfaceRole::Hover
            } else {
                SurfaceRole::Transparent
            }
        });
        let bg = ctx.add(
            RectWidget::new()
                .background(ColorProp::DynamicSurfaceRole(role))
                .corner_radius(CornerRadius::uniform(self.recipe.corner_radius)),
        );
        ctx.add(ZStack::new().add_child(bg).add_child(cfg.label))
    }

    fn make_sort_indicator(&self, _direction: SortDirection, ctx: &mut BuildContext) -> WidgetId {
        // Placeholder — TableView paints the indicator chevron directly
        // for performance. Custom styles override.
        ctx.add(crate::primitives::Spacer::new())
    }

    fn make_row_background(&self, cfg: &TableRowConfig, ctx: &mut BuildContext) -> WidgetId {
        let alt = cfg.is_alt;
        // Effective focus = the view holds keyboard focus AND the host window is
        // active. Either signal absent → treat as satisfied (the stock TableView
        // passes `None` and paints its own focus-/window-aware band directly).
        let effective_focus: Option<Signal<bool>> = match (&cfg.is_focused, &cfg.is_window_active) {
            (Some(f), Some(wa)) => Some(f.and(wa)),
            (Some(f), None) => Some(f.clone()),
            (None, Some(wa)) => Some(wa.clone()),
            (None, None) => None,
        };
        let role: Signal<SurfaceRole> = match effective_focus {
            // Focus-aware: vivid `Selected` only while focused+active, else the
            // muted `SelectedInactive`.
            Some(focus) => cfg
                .is_selected
                .clone()
                .zip(&cfg.is_hovered)
                .zip(&focus)
                .map(move |((sel, hov), foc)| {
                    if *sel {
                        if *foc {
                            SurfaceRole::Selected
                        } else {
                            SurfaceRole::SelectedInactive
                        }
                    } else if *hov {
                        SurfaceRole::Hover
                    } else if alt {
                        SurfaceRole::AltRow
                    } else {
                        SurfaceRole::Transparent
                    }
                }),
            None => cfg
                .is_selected
                .clone()
                .zip(&cfg.is_hovered)
                .map(move |(sel, hov)| {
                    if *sel {
                        SurfaceRole::Selected
                    } else if *hov {
                        SurfaceRole::Hover
                    } else if alt {
                        SurfaceRole::AltRow
                    } else {
                        SurfaceRole::Transparent
                    }
                }),
        };
        ctx.add(RectWidget::new().background(ColorProp::DynamicSurfaceRole(role)))
    }

    fn grid(&self) -> TableGridRecipe {
        TableGridRecipe::default()
    }
}
