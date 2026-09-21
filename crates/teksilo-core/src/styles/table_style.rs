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
//! `style_slots.table` slot are in place. Header cells route through
//! `make_header_cell` — the shared `table_view::header::HeaderCell`,
//! used by both views — and their gutter comes from
//! `cell_padding_horizontal` / `cell_padding_vertical` rather than from
//! the module constants. Wiring `make_row_background` and
//! `make_sort_indicator` is intentionally deferred: the widgets still
//! own their cell / row / grid-line chrome directly, and those
//! dimensions live on `teksilo_widgets::styles::recipe_table_style` as
//! `pub const`s.

use std::rc::Rc;

use teksilo_tokens::{BorderRole, InputTokens};

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::styles::density::spacing;
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
    /// Whether the view holds keyboard focus. `None` = "treat as always
    /// focused" (the stock `TableView` paints its selection band directly and
    /// passes `None` here). A custom style that paints row backgrounds reactively
    /// can supply this — combined with [`is_window_active`](Self::is_window_active)
    /// — to desaturate the selection (`SelectedInactive`) when focus is elsewhere.
    pub is_focused: Option<Signal<bool>>,
    /// Whether the host window is active (`focused AND not occluded`). `None` =
    /// "treat as always active". Custom styles combine this with
    /// [`is_focused`](Self::is_focused) so a selected row desaturates in a
    /// background window, matching the stock views.
    pub is_window_active: Option<Signal<bool>>,
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

    /// Horizontal padding inside a cell, in logical pixels — the same gutter
    /// on the leading and trailing edge of every header and body cell.
    ///
    /// A metrics accessor for the same reason [`grid`](Self::grid) is one: the
    /// header cell composes its own `Padding` and the table computes its
    /// filter-affordance zone from this number, and neither can reach a
    /// recipe field through an `Rc<dyn TableStyle>`. Without it a preset's
    /// gutter is written and never rendered.
    ///
    /// **Defaulted**, so a `TableStyle` implemented outside this workspace
    /// keeps compiling and keeps the ladder it had. The default is
    /// `teksilo_widgets::styles::recipe_table_style::CELL_PADDING_HORIZONTAL`
    /// put through [`spacing`] — restated as a literal because `teksilo-core`
    /// cannot name a `teksilo-widgets` constant, and pinned equal to it by
    /// `the_table_trait_defaults_restate_the_module_constants`.
    fn cell_padding_horizontal(&self, tokens: &InputTokens) -> f32 {
        spacing(8.0, tokens)
    }

    /// Vertical padding inside a cell, in logical pixels. Defaulted on the
    /// same terms as [`cell_padding_horizontal`](Self::cell_padding_horizontal).
    fn cell_padding_vertical(&self, tokens: &InputTokens) -> f32 {
        spacing(4.0, tokens)
    }
}

pub type SharedTableStyle = Rc<dyn TableStyle>;
