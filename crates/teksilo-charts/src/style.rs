// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Chart design tokens — relocated from `theme.components.chart` into
//! this standalone module. Charts pull their
//! *colours* from theme roles + `ColorTokens::chart_palette`; this
//! module only carries dimensions. Themes that want to nudge density
//! either fork `teksilo-charts` or install a custom layout pass on top
//! of these primitives.

/// Plot padding — gap between the chart's outer bounds and the data
/// area. Top is larger so series-end labels and tooltip arrows have
/// breathing room; bottom is tight because axis labels live there.
pub const PLOT_PADDING_TOP: f32 = 12.0;
pub const PLOT_PADDING_RIGHT: f32 = 12.0;
pub const PLOT_PADDING_BOTTOM: f32 = 4.0;
pub const PLOT_PADDING_LEADING: f32 = 4.0;

/// Axis tick stub length (the small line perpendicular to the axis
/// at every tick).
pub const AXIS_TICK_LENGTH: f32 = 4.0;
/// Gap between axis tick label baseline and the tick stub.
pub const AXIS_LABEL_GAP: f32 = 4.0;
/// Gap between axis title baseline and the axis label row.
pub const AXIS_TITLE_GAP: f32 = 8.0;
/// Grid-line stroke width inside the plot area.
pub const GRIDLINE_WIDTH: f32 = 1.0;

/// Bar floor — minimum drawn width per bar so 1px-wide bars stay
/// visible at large data sets.
pub const BAR_MIN_WIDTH: f32 = 4.0;
/// Line chart series-line default stroke width.
pub const LINE_DEFAULT_WIDTH: f32 = 1.5;
/// Line chart point default radius.
pub const POINT_DEFAULT_RADIUS: f32 = 3.0;

/// Legend swatch (colour chip) edge length.
pub const LEGEND_SWATCH_SIZE: f32 = 10.0;
/// Gap between adjacent legend items.
pub const LEGEND_ITEM_GAP: f32 = 12.0;
/// Gap between the legend strip and the plot area.
pub const LEGEND_TO_PLOT_GAP: f32 = 8.0;

/// Tooltip-card padding.
pub const TOOLTIP_PADDING: f32 = 8.0;

/// Pie chart — outer padding inside the plot bounds.
pub const PIE_PADDING: f32 = 8.0;
/// Pie chart — gap between the slice arc and its leader line endpoint.
pub const PIE_LABEL_GAP: f32 = 4.0;
/// Pie chart — leader line length from the slice to the label.
pub const PIE_LEADER_LENGTH: f32 = 12.0;
/// Pie chart — minimum slice angle (in degrees) that still gets a
/// label drawn. Smaller slices fold into a generic "Other" leader.
pub const PIE_MIN_SLICE_LABEL_DEGREES: f32 = 12.0;
/// Donut chart — default `inner_radius / outer_radius` ratio. `0.0`
/// would be a solid pie; `0.55` is the conventional donut thickness.
pub const DONUT_DEFAULT_INNER_RATIO: f32 = 0.55;

/// Selection highlight — outward padding applied when stroking a
/// selected bar's outline, so the accent ring sits just outside the
/// bar fill instead of overlapping its edge.
pub const SELECTION_BAR_OUTLINE_PAD: f32 = 3.0;
/// Selection highlight — ring radius drawn around a selected line
/// point. Deliberately larger than the hover marker's 6.0 px ring so a
/// point that is both hovered and selected shows two distinguishable
/// rings rather than one indistinguishable one.
pub const SELECTION_POINT_RING_RADIUS: f32 = 9.0;
/// Selection highlight stroke width, shared by the bar outline,
/// line-point ring, and pie/donut wedge outline.
pub const SELECTION_STROKE_WIDTH: f32 = 2.5;
