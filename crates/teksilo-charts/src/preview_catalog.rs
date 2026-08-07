// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `WidgetCatalog` impls for `teksilo-charts`.
//!
//! Gated behind the `preview` Cargo feature so production builds and
//! headless tests don't pull in the catalog data or the `inventory`
//! submission machinery — mirrors
//! `teksilo-widgets/src/preview_catalog.rs`.
//!
//! All three chart widgets are generic over their category type `T`;
//! the catalog fixes `T = String` (the common case — string category
//! labels) and synthesizes deterministic sample data from the knob
//! values in `build()` rather than storing fixture data on the type.
//!
//! Group: "Data Visualization".

use teksilo_core::widget::Widget;
use teksilo_data::{ChartDatum, ChartModel, ChartSeries};
use teksilo_preview::{
    KnobOverrides, KnobSpec, KnobValues, PreviewVariant, WidgetCatalog, register_widget_catalog_at,
};

use crate::{
    BarChart, BarGrouping, BarOrientation, LegendPosition, LineChart, PieChart, PieLabelMode,
};

// ---------------------------------------------------------------------------
// Deterministic sample data
// ---------------------------------------------------------------------------

/// Series names used by [`sample_series`], in knob-index order. Caps
/// `series_count` at 4 (the `i32_` knob's declared max).
const SERIES_NAMES: [&str; 4] = ["Alpha", "Beta", "Gamma", "Delta"];

/// Points synthesized per series by [`sample_series`].
const SAMPLE_POINT_COUNT: usize = 6;

/// Slice labels used by [`sample_pie_data`], in knob-index order. Caps
/// `slice_count` at 6 (the `i32_` knob's declared max).
const SLICE_LABELS: [&str; 6] = ["North", "South", "East", "West", "Central", "Other"];

/// Deterministic multi-series sample data for `BarChart`/`LineChart`
/// preview knobs. No randomness — every value comes from a fixed
/// formula, so PNG exports and any future snapshot tests stay stable
/// across runs. `count` is clamped to `[1, SERIES_NAMES.len()]`.
fn sample_series(count: i32) -> Vec<ChartSeries<String>> {
    let count = count.clamp(1, SERIES_NAMES.len() as i32) as usize;
    (0..count)
        .map(|si| {
            let mut series = ChartSeries::new(SERIES_NAMES[si]);
            for i in 0..SAMPLE_POINT_COUNT {
                let value = ((si as i32 * 53 + i as i32 * 17 + 31) % 60 + 10) as f32;
                series.push(format!("Q{}", i + 1), value);
            }
            series
        })
        .collect()
}

/// Deterministic single-series sample data for `PieChart` preview
/// knobs. `slices` is clamped to `[1, SLICE_LABELS.len()]`.
fn sample_pie_data(slices: i32) -> Vec<ChartDatum<String>> {
    let slices = slices.clamp(1, SLICE_LABELS.len() as i32) as usize;
    (0..slices)
        .map(|i| {
            let value = ((i as i32 * 53 + 31) % 60 + 10) as f32;
            ChartDatum::new(SLICE_LABELS[i].to_string(), value)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Enum-knob mappers
// ---------------------------------------------------------------------------

fn bar_orientation(idx: usize) -> BarOrientation {
    match idx {
        1 => BarOrientation::Horizontal,
        _ => BarOrientation::Vertical,
    }
}

fn bar_grouping(idx: usize) -> BarGrouping {
    match idx {
        0 => BarGrouping::Single,
        _ => BarGrouping::Grouped,
    }
}

/// Maps a `legend_position` enum-knob index to `LegendPosition`
/// (declaration order: Top, Bottom, Leading, Trailing).
fn legend_position(idx: usize) -> LegendPosition {
    match idx {
        0 => LegendPosition::Top,
        2 => LegendPosition::Leading,
        3 => LegendPosition::Trailing,
        _ => LegendPosition::Bottom,
    }
}

/// Maps a `label_mode` enum-knob index to `PieLabelMode` (declaration
/// order: None, Inside, Outside, InsideWithLeaders).
fn pie_label_mode(idx: usize) -> PieLabelMode {
    match idx {
        1 => PieLabelMode::Inside,
        2 => PieLabelMode::Outside,
        3 => PieLabelMode::InsideWithLeaders,
        _ => PieLabelMode::None,
    }
}

// ---------------------------------------------------------------------------
// BarChart
// ---------------------------------------------------------------------------

impl WidgetCatalog for BarChart<String> {
    fn id() -> &'static str {
        "bar_chart"
    }
    fn group() -> &'static str {
        "Data Visualization"
    }
    fn display_name() -> &'static str {
        "BarChart"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .i32_("series_count", "Series", 2, 1, 4)
            .enum_(
                "orientation",
                "Orientation",
                "BarOrientation",
                &["Vertical", "Horizontal"],
                0,
            )
            .enum_(
                "grouping",
                "Grouping",
                "BarGrouping",
                &["Single", "Grouped"],
                1,
            )
            .bool_("grid", "Show grid", true)
            .bool_("legend", "Show legend", true)
            .enum_(
                "legend_position",
                "Legend position",
                "LegendPosition",
                &["Top", "Bottom", "Leading", "Trailing"],
                1,
            )
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("horizontal", KnobOverrides::new().enum_("orientation", 1)),
            PreviewVariant::knobs(
                "single-series",
                KnobOverrides::new()
                    .i32_("series_count", 1)
                    .enum_("grouping", 0),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let series_count = knobs.i32_("series_count").get();
        let orientation = bar_orientation(knobs.enum_("orientation").get());
        let grouping = bar_grouping(knobs.enum_("grouping").get());
        let grid = knobs.bool_("grid").get();
        let legend = knobs.bool_("legend").get();
        let pos = legend_position(knobs.enum_("legend_position").get());
        let model = ChartModel::from_series_vec(sample_series(series_count));
        Box::new(
            BarChart::new(model)
                .orientation(orientation)
                .grouping(grouping)
                .grid(grid)
                .legend(legend)
                .legend_position(pos),
        )
    }
}
register_widget_catalog_at!("crates/teksilo-charts/src/bar_chart.rs", BarChart<String>);

// ---------------------------------------------------------------------------
// LineChart
// ---------------------------------------------------------------------------

impl WidgetCatalog for LineChart<String> {
    fn id() -> &'static str {
        "line_chart"
    }
    fn group() -> &'static str {
        "Data Visualization"
    }
    fn display_name() -> &'static str {
        "LineChart"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .i32_("series_count", "Series", 2, 1, 4)
            .bool_("points", "Show points", true)
            .bool_("area_fill", "Area fill", false)
            .f32_step(
                "area_fill_opacity",
                "Area fill opacity",
                0.15,
                0.0,
                1.0,
                0.05,
            )
            .bool_("grid", "Show grid", true)
            .bool_("legend", "Show legend", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("area-fill", KnobOverrides::new().bool_("area_fill", true)),
            PreviewVariant::knobs("no-points", KnobOverrides::new().bool_("points", false)),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let series_count = knobs.i32_("series_count").get();
        let points = knobs.bool_("points").get();
        let area_fill = knobs.bool_("area_fill").get();
        let area_fill_opacity = knobs.f32_("area_fill_opacity").get();
        let grid = knobs.bool_("grid").get();
        let legend = knobs.bool_("legend").get();
        let model = ChartModel::from_series_vec(sample_series(series_count));
        Box::new(
            LineChart::new(model)
                .points(points)
                .area_fill(area_fill)
                .area_fill_opacity(area_fill_opacity)
                .grid(grid)
                .legend(legend),
        )
    }
}
register_widget_catalog_at!("crates/teksilo-charts/src/line_chart.rs", LineChart<String>);

// ---------------------------------------------------------------------------
// PieChart
// ---------------------------------------------------------------------------

impl WidgetCatalog for PieChart<String> {
    fn id() -> &'static str {
        "pie_chart"
    }
    fn group() -> &'static str {
        "Data Visualization"
    }
    fn display_name() -> &'static str {
        "PieChart"
    }
    fn knobs() -> KnobSpec {
        KnobSpec::new()
            .i32_("slice_count", "Slices", 5, 1, 6)
            .f32_step("donut_ratio", "Donut ratio", 0.55, 0.0, 0.9, 0.05)
            .enum_(
                "label_mode",
                "Label mode",
                "PieLabelMode",
                &["None", "Inside", "Outside", "InsideWithLeaders"],
                0,
            )
            .bool_("percentages", "Show percentages", false)
            .bool_("legend", "Show legend", true)
    }
    fn variants() -> Vec<PreviewVariant> {
        vec![
            PreviewVariant::defaults("default"),
            PreviewVariant::knobs("pie", KnobOverrides::new().f32_("donut_ratio", 0.0)),
            PreviewVariant::knobs(
                "with-labels",
                KnobOverrides::new()
                    .enum_("label_mode", 2)
                    .bool_("percentages", true),
            ),
        ]
    }
    fn build(_variant: &str, knobs: &KnobValues) -> Box<dyn Widget> {
        let slice_count = knobs.i32_("slice_count").get();
        let donut_ratio = knobs.f32_("donut_ratio").get();
        let label_mode = pie_label_mode(knobs.enum_("label_mode").get());
        let percentages = knobs.bool_("percentages").get();
        let legend = knobs.bool_("legend").get();
        let model = ChartModel::from_points(sample_pie_data(slice_count));
        Box::new(
            PieChart::new(model)
                .donut(donut_ratio)
                .label_mode(label_mode)
                .show_percentages(percentages)
                .legend(legend),
        )
    }
}
register_widget_catalog_at!("crates/teksilo-charts/src/pie_chart.rs", PieChart<String>);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(feature = "preview", test))]
mod tests {
    use super::*;

    #[test]
    fn bar_chart_builds_without_panic() {
        let knobs = KnobValues::from_spec(&<BarChart<String> as WidgetCatalog>::knobs(), None);
        let _ = <BarChart<String> as WidgetCatalog>::build("default", &knobs);
    }

    #[test]
    fn line_chart_builds_without_panic() {
        let knobs = KnobValues::from_spec(&<LineChart<String> as WidgetCatalog>::knobs(), None);
        let _ = <LineChart<String> as WidgetCatalog>::build("default", &knobs);
    }

    #[test]
    fn pie_chart_builds_without_panic() {
        let knobs = KnobValues::from_spec(&<PieChart<String> as WidgetCatalog>::knobs(), None);
        let _ = <PieChart<String> as WidgetCatalog>::build("default", &knobs);
    }
}
