// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `TableView` and `TreeTableView`. See
//! `docs/styling-system.md`.
//!
//! Multi-method trait shared by both data-grid widgets. Chrome is split
//! between *composed* widgets (one widget per header cell, sort indicator,
//! and row band) and a *batched paint pass* for grid lines + frozen-column
//! shadow. Grid lines genuinely need the batched path — composing one
//! `RectWidget` per line on a 1000-row virtualized viewport would defeat
//! the virtualization budget. The "recipe describes, widget paints the
//! batched case" split applies here for specialty widgets.
//!
//! ## Wiring status
//!
//! The trait surface, the `TableGridRecipe`, and the
//! `style_slots.table` slot are in place. Wiring `TableView` /
//! `TreeTableView` through `make_*` is intentionally deferred. The
//! widgets currently still own their cell / row / header / grid-line
//! chrome directly; every dimension lives on
//! `bastyde_widgets::styles::recipe_table_style` as `pub const`s.

use std::rc::Rc;

use bastyde_tokens::BorderRole;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

/// Sort direction for header cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

pub struct TableHeaderCellConfig {
    pub label: WidgetId,
    pub sort: Option<SortDirection>,
    pub is_hovered: Signal<bool>,
    pub is_resizing: Signal<bool>,
}

pub struct TableRowConfig {
    pub index: usize,
    pub is_selected: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_alt: bool,
}

/// Recipe — non-widget data describing the batched paint pass for
/// grid lines and the frozen-column shadow. Consumed by
/// `TableView::paint` / `TreeTableView::paint` directly. Custom styles
/// override the entire recipe via `TableStyle::grid()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableGridRecipe {
    /// Vertical and horizontal grid-line stroke width.
    pub line_thickness: f32,
    /// Border role for grid lines. Defaults to `Divider`.
    pub line_role: BorderRole,
    /// Width of the shadow drawn at the frozen-column boundary. `0.0`
    /// disables the shadow.
    pub frozen_shadow_width: f32,
}

impl Default for TableGridRecipe {
    fn default() -> Self {
        Self {
            line_thickness: 1.0,
            line_role: BorderRole::Divider,
            frozen_shadow_width: 4.0,
        }
    }
}

pub trait TableStyle: 'static {
    fn make_header_cell(&self, cfg: &TableHeaderCellConfig, ctx: &mut BuildContext) -> WidgetId;
    fn make_sort_indicator(&self, direction: SortDirection, ctx: &mut BuildContext) -> WidgetId;
    /// Row-band chrome (selection / hover / alt) — composed *behind*
    /// the cells.
    fn make_row_background(&self, cfg: &TableRowConfig, ctx: &mut BuildContext) -> WidgetId;
    /// Grid-line + frozen-column-shadow recipe — the table's own paint
    /// pass batches over the virtualized viewport using this data.
    fn grid(&self) -> TableGridRecipe;
}

pub type SharedTableStyle = Rc<dyn TableStyle>;
