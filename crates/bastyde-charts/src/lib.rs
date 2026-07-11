// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `bastyde-charts` — BarChart, LineChart, and PieChart for Bastyde.
//!
//! All three render via the existing [`bastyde_canvas`] Path / fill_rect /
//! draw_line API and integrate with the standard widget tree, signal /
//! prop binding, and theme system.
//!
//! - [`BarChart`] — vertical or horizontal bars, single or grouped series.
//! - [`LineChart`] — points connected by polylines, optional area fill
//!   and per-point hover tooltips.
//! - [`PieChart`] — pie or donut (via `inner_radius_ratio`), with an
//!   optional center widget slot for donut total / icon / drilldown.
//!
//! Series data lives in a [`bastyde_data::ChartModel`] — the reactive,
//! multi-series data model shared by all three chart widgets (mirroring
//! `ListModel`/`TreeModel` in shape). `T` is the category / x-axis type.
//! Numeric values are always `f32`. Default series colors come from
//! [`ChartPalette::FromTheme`], which reads the active theme's
//! `chart_palette` (Okabe-Ito by default). Per-datum marks are exposed to
//! assistive technology as synthetic AT nodes and are the basis of the
//! hover/selection hit-testing shared across the three chart kinds.

pub mod axis;
pub mod bar_chart;
pub(crate) mod hit;
pub mod layout;
pub mod legend;
pub mod line_chart;
pub mod palette;
pub mod pie_chart;
#[cfg(feature = "preview")]
mod preview_catalog;
pub mod recipe_style;
pub mod style;
pub(crate) mod text;

pub use axis::{AxisConfig, auto_tick_count, nice_ticks};
pub use bar_chart::{BarChart, BarGrouping, BarOrientation};
pub use bastyde_data::{
    ChartAggregate, ChartAggregateFn, ChartDatum, ChartModel, ChartSelection, ChartSeries,
    ChartWindow, SeriesId, SeriesView,
};
pub use layout::LegendPosition;
pub use legend::{ChartLegend, LegendOrientation};
pub use line_chart::LineChart;
pub use palette::ChartPalette;
pub use pie_chart::{PieChart, PieLabelMode};
pub use recipe_style::RecipeChartStyle;
