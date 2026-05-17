//! Default `TableStyle` impl + table-family design tokens.
//!
//! Relocates all `theme.components.table` constants to `pub const`s
//! on this module — Stage F1 of the group-5 styling migration. Shared
//! by `TableView` and `TreeTable` (TreeTable adds tree-only indent /
//! twist sizing on top of the standard table dims).
//!
//! ## Wiring status
//!
//! The `make_*` trait methods produce reference subtrees for custom
//! styles that want to install themselves via
//! `style_slots.table = Some(...)`. The default IntUI shape is the
//! one `TableView` / `TreeTable` paint today inline; they continue to
//! own their paint passes for performance reasons (grid lines need a
//! single batched pass over the virtualized viewport — composing one
//! `RectWidget` per line would defeat virtualization). The full
//! chrome decomposition is deferred.

use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{
    SortDirection, TableGridRecipe, TableHeaderCellConfig, TableRowConfig, TableStyle,
};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, SurfaceRole};

use crate::primitives::{RectWidget, ZStack};

// ─── IntUI design tokens for TableView / TreeTable ─────────────────
// Relocated from `theme.components.table` in Stage F1.

/// Body row height. Headers use `HEADER_HEIGHT`.
pub const ROW_HEIGHT: f32 = 28.0;
/// Sticky header row height.
pub const HEADER_HEIGHT: f32 = 32.0;
/// Horizontal padding inside each cell (also applied to header cells).
pub const CELL_PADDING_HORIZONTAL: f32 = 8.0;
/// Vertical padding inside each cell.
pub const CELL_PADDING_VERTICAL: f32 = 4.0;
/// Width of the right-edge resize hit zone on header cells.
pub const RESIZE_HANDLE_WIDTH: f32 = 4.0;
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
/// `TreeTable` only — pixels per indent level on the tree column.
pub const TREE_INDENT_PER_LEVEL: f32 = 16.0;
/// `TreeTable` only — edge length of the twist (expand/collapse) chevron.
pub const TREE_TWIST_SIZE: f32 = 12.0;
/// `TreeTable` only — gap between the twist chevron and the cell content.
pub const TREE_TWIST_LABEL_GAP: f32 = 4.0;

/// Default `TableStyle` shipped with Bastyde. The trait methods return
/// reference subtrees; the widgets themselves still own their batched
/// paint passes for performance (pending the chrome-decomposition
/// follow-up).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeTableStyle;

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
                .corner_radius(CornerRadius::uniform(0.0)),
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
        let role: Signal<SurfaceRole> =
            cfg.is_selected
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
                });
        ctx.add(RectWidget::new().background(ColorProp::DynamicSurfaceRole(role)))
    }

    fn grid(&self) -> TableGridRecipe {
        TableGridRecipe::default()
    }
}
