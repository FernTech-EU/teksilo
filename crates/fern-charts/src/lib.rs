//! `fern-charts` — BarChart, LineChart, and PieChart for FernUI.
//!
//! All three render via the existing [`fern_canvas`] Path / fill_rect /
//! draw_line API and integrate with the standard widget tree, signal /
//! prop binding, and theme system.
//!
//! - [`BarChart`] — vertical or horizontal bars, single or grouped series.
//! - [`LineChart`] — points connected by polylines, optional area fill
//!   and per-point hover tooltips.
//! - [`PieChart`] — pie or donut (via `inner_radius_ratio`), with an
//!   optional center widget slot for donut total / icon / drilldown.
//!
//! Series are bound through [`Prop`](fern_core::signal::Prop)
//! `<Vec<ChartSeries<T>>>`, where `T` is the category / x-axis type. Numeric
//! values are always `f32`. Default series colors come from
//! [`ChartPalette::FromTheme`], which reads the active theme's
//! `chart_palette` (Okabe-Ito by default).

pub mod axis;
pub mod bar_chart;
pub mod layout;
pub mod legend;
pub mod line_chart;
pub mod palette;
pub mod pie_chart;
pub mod series;

pub use axis::{auto_tick_count, nice_ticks, AxisConfig};
pub use bar_chart::{BarChart, BarGrouping, BarOrientation};
pub use layout::LegendPosition;
pub use legend::{ChartLegend, LegendOrientation};
pub use line_chart::LineChart;
pub use palette::ChartPalette;
pub use pie_chart::{PieChart, PieLabelMode};
pub use series::{ChartDatum, ChartSeries};
