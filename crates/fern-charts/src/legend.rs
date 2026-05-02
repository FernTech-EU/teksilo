//! `ChartLegend` — series swatch + label list.
//!
//! Use the standalone widget when you want the legend in a different
//! container than the chart (or with custom layout). Charts also embed
//! this internally when constructed with `.legend(true)`, sharing the
//! same `series` and `palette` props.

use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal, TextBackend};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::{TextRole, TextStyleRole};

use crate::palette::ChartPalette;
use crate::series::ChartSeries;
use crate::text::{measure_text_width, measure_text_width_via};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendOrientation {
    Horizontal,
    Vertical,
}

pub struct ChartLegend<T: Clone + 'static> {
    series: Prop<Vec<ChartSeries<T>>>,
    palette: Prop<ChartPalette>,
    orientation: LegendOrientation,
    interactive: bool,
}

impl<T: Clone + 'static> ChartLegend<T> {
    pub fn new(series: impl Into<Prop<Vec<ChartSeries<T>>>>) -> Self {
        Self {
            series: series.into(),
            palette: Prop::Static(ChartPalette::FromTheme),
            orientation: LegendOrientation::Horizontal,
            interactive: false,
        }
    }

    pub fn palette(mut self, p: impl Into<Prop<ChartPalette>>) -> Self {
        self.palette = p.into();
        self
    }

    pub fn orientation(mut self, o: LegendOrientation) -> Self {
        self.orientation = o;
        self
    }

    pub fn interactive(mut self, on: bool) -> Self {
        self.interactive = on;
        self
    }
}

impl<T: Clone + 'static> std::fmt::Debug for ChartLegend<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartLegend")
            .field("orientation", &self.orientation)
            .field("interactive", &self.interactive)
            .finish()
    }
}

impl<T: Clone + 'static> Widget for ChartLegend<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        // Series add/remove → relayout (count affects width). Any
        // .visible toggle is read at paint, no binding needed because
        // legend repaints on click via the interactive handler.
        self.series
            .register_if_bound(id, registry, BindingLevel::Relayout);
        self.palette
            .register_if_bound(id, registry, BindingLevel::RepaintOnly);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> fern_core::widget::LayoutResponse {
        let series_vec = self.series.get();
        if series_vec.is_empty() {
            return (Size::ZERO).into();
        }
        let style = &ctx.theme.components.chart;
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);

        // Item = swatch + small gap + label text.
        let label_widths: Vec<f32> = series_vec
            .iter()
            .map(|s| measure_text_width_via(ctx.text_backend, &s.name, &label_style))
            .collect();
        let item_widths: Vec<f32> = label_widths
            .iter()
            .map(|w| style.legend_swatch_size + 4.0 + w)
            .collect();
        let line_height = (style.legend_swatch_size).max(label_style.size * 1.2);

        match self.orientation {
            LegendOrientation::Horizontal => {
                let total_w: f32 = item_widths.iter().sum::<f32>()
                    + style.legend_item_gap * (item_widths.len() as f32 - 1.0).max(0.0);
                let height = line_height;
                Size::new(
                    proposal.width.unwrap_or(total_w).min(total_w.max(0.0)),
                    proposal.height.unwrap_or(height),
                )
            }
            LegendOrientation::Vertical => {
                let max_w = item_widths.iter().copied().fold(0.0_f32, f32::max);
                let total_h = line_height * item_widths.len() as f32;
                Size::new(
                    proposal.width.unwrap_or(max_w),
                    proposal.height.unwrap_or(total_h),
                )
            }
        }.into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.components.chart;
        let series_vec = self.series.get();
        if series_vec.is_empty() {
            return;
        }
        let palette = self.palette.get();
        let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
        let label_color = TextRole::Primary.resolve(&theme.colors);
        let line_height = style.legend_swatch_size.max(label_style.size * 1.2);

        match self.orientation {
            LegendOrientation::Horizontal => {
                let mut x = bounds.x;
                let center_y = bounds.y + line_height * 0.5;
                for (i, series) in series_vec.iter().enumerate() {
                    let visible = series.visible.get();
                    let color = series
                        .color
                        .as_ref()
                        .map(|c| c.resolve(theme))
                        .unwrap_or_else(|| palette.color_for(i, theme));
                    let final_color = if visible {
                        color
                    } else {
                        color.with_alpha(0.35)
                    };
                    let swatch = Rect::new(
                        x,
                        center_y - style.legend_swatch_size * 0.5,
                        style.legend_swatch_size,
                        style.legend_swatch_size,
                    );
                    canvas.fill_rounded_rect(
                        swatch,
                        fern_tokens::CornerRadius::uniform(2.0),
                        final_color,
                    );
                    x += style.legend_swatch_size + 4.0;
                    let label_w = measure_text_width(canvas, &series.name, &label_style);
                    let text_color = if visible {
                        label_color
                    } else {
                        label_color.with_alpha(0.5)
                    };
                    canvas.draw_text(
                        &series.name,
                        Rect::new(
                            x,
                            center_y - label_style.size * 0.6,
                            label_w,
                            label_style.size * 1.2,
                        ),
                        &label_style,
                        text_color,
                    );
                    x += label_w + style.legend_item_gap;
                }
            }
            LegendOrientation::Vertical => {
                for (i, series) in series_vec.iter().enumerate() {
                    let visible = series.visible.get();
                    let color = series
                        .color
                        .as_ref()
                        .map(|c| c.resolve(theme))
                        .unwrap_or_else(|| palette.color_for(i, theme));
                    let final_color = if visible {
                        color
                    } else {
                        color.with_alpha(0.35)
                    };
                    let row_y = bounds.y + i as f32 * line_height;
                    let center_y = row_y + line_height * 0.5;
                    let swatch = Rect::new(
                        bounds.x,
                        center_y - style.legend_swatch_size * 0.5,
                        style.legend_swatch_size,
                        style.legend_swatch_size,
                    );
                    canvas.fill_rounded_rect(
                        swatch,
                        fern_tokens::CornerRadius::uniform(2.0),
                        final_color,
                    );
                    let label_w = measure_text_width(canvas, &series.name, &label_style);
                    let text_color = if visible {
                        label_color
                    } else {
                        label_color.with_alpha(0.5)
                    };
                    canvas.draw_text(
                        &series.name,
                        Rect::new(
                            bounds.x + style.legend_swatch_size + 4.0,
                            center_y - label_style.size * 0.6,
                            label_w,
                            label_style.size * 1.2,
                        ),
                        &label_style,
                        text_color,
                    );
                }
            }
        }
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(fern_core::accesskit::Role::List);
        builder.set_name("Chart legend");
    }
}

/// Compute the size the legend would occupy along its main axis (used by
/// charts to reserve space for an embedded legend). For Horizontal returns
/// height; for Vertical returns width.
///
/// `backend` is the live text backend if one is wired (canvas at paint
/// time, layout context at layout time) — `None` falls back to the same
/// `chars * size * 0.7 + 4` heuristic used by `measure_text_width` so
/// reservation matches what the painter will later request.
pub(crate) fn legend_main_axis_size<T: Clone + 'static>(
    backend: Option<&Rc<RefCell<dyn TextBackend>>>,
    series_vec: &[ChartSeries<T>],
    style: &fern_tokens::ChartStyle,
    label_style: &fern_tokens::TextStyle,
    orientation: LegendOrientation,
) -> f32 {
    let line_height = style.legend_swatch_size.max(label_style.size * 1.2);
    match orientation {
        LegendOrientation::Horizontal => line_height,
        LegendOrientation::Vertical => {
            let max_w = series_vec
                .iter()
                .map(|s| measure_text_width_via(backend, &s.name, label_style))
                .fold(0.0_f32, f32::max);
            style.legend_swatch_size + 4.0 + max_w
        }
    }
}

/// Pick the appropriate orientation for an embedded legend given its
/// position around the plot.
pub(crate) fn orientation_for_position(
    pos: crate::layout::LegendPosition,
) -> LegendOrientation {
    use crate::layout::LegendPosition;
    match pos {
        LegendPosition::Top | LegendPosition::Bottom => LegendOrientation::Horizontal,
        LegendPosition::Leading | LegendPosition::Trailing => LegendOrientation::Vertical,
    }
}

/// Helper called by charts during paint to draw an embedded legend in
/// the band reserved by `carve_plot_area`. Centers horizontally when the
/// orientation is Horizontal, top-aligns when Vertical.
pub(crate) fn paint_embedded_legend<T: Clone + 'static>(
    canvas: &mut Canvas,
    band: Rect,
    series_vec: &[ChartSeries<T>],
    palette: &ChartPalette,
    orientation: LegendOrientation,
    theme: &fern_tokens::Theme,
) {
    if series_vec.is_empty() || band.width <= 0.0 || band.height <= 0.0 {
        return;
    }
    let style = &theme.components.chart;
    let label_style = TextStyleRole::Tiny.resolve(&theme.typography);
    let label_color = TextRole::Primary.resolve(&theme.colors);
    let line_height = style.legend_swatch_size.max(label_style.size * 1.2);

    match orientation {
        LegendOrientation::Horizontal => {
            // Center horizontally inside `band`. Measure label widths via
            // the live text backend so longer series names don't get
            // truncated to "…" by `draw_text`'s max_width gate.
            let label_widths: Vec<f32> = series_vec
                .iter()
                .map(|s| measure_text_width(canvas, &s.name, &label_style))
                .collect();
            let item_widths: Vec<f32> = label_widths
                .iter()
                .map(|w| style.legend_swatch_size + 4.0 + w)
                .collect();
            let total_w: f32 = item_widths.iter().sum::<f32>()
                + style.legend_item_gap * (item_widths.len() as f32 - 1.0).max(0.0);
            let mut x = band.x + (band.width - total_w) * 0.5;
            let center_y = band.y + line_height * 0.5;
            for (i, series) in series_vec.iter().enumerate() {
                let visible = series.visible.get();
                let color = series
                    .color
                    .as_ref()
                    .map(|c| c.resolve(theme))
                    .unwrap_or_else(|| palette.color_for(i, theme));
                let final_color = if visible {
                    color
                } else {
                    color.with_alpha(0.35)
                };
                let swatch = Rect::new(
                    x,
                    center_y - style.legend_swatch_size * 0.5,
                    style.legend_swatch_size,
                    style.legend_swatch_size,
                );
                canvas.fill_rounded_rect(
                    swatch,
                    fern_tokens::CornerRadius::uniform(2.0),
                    final_color,
                );
                x += style.legend_swatch_size + 4.0;
                let label_w = label_widths[i];
                let text_color = if visible {
                    label_color
                } else {
                    label_color.with_alpha(0.5)
                };
                canvas.draw_text(
                    &series.name,
                    Rect::new(
                        x,
                        center_y - label_style.size * 0.6,
                        label_w,
                        label_style.size * 1.2,
                    ),
                    &label_style,
                    text_color,
                );
                x += label_w + style.legend_item_gap;
            }
        }
        LegendOrientation::Vertical => {
            for (i, series) in series_vec.iter().enumerate() {
                let visible = series.visible.get();
                let color = series
                    .color
                    .as_ref()
                    .map(|c| c.resolve(theme))
                    .unwrap_or_else(|| palette.color_for(i, theme));
                let final_color = if visible {
                    color
                } else {
                    color.with_alpha(0.35)
                };
                let row_y = band.y + i as f32 * line_height;
                let center_y = row_y + line_height * 0.5;
                let swatch = Rect::new(
                    band.x,
                    center_y - style.legend_swatch_size * 0.5,
                    style.legend_swatch_size,
                    style.legend_swatch_size,
                );
                canvas.fill_rounded_rect(
                    swatch,
                    fern_tokens::CornerRadius::uniform(2.0),
                    final_color,
                );
                let label_w = measure_text_width(canvas, &series.name, &label_style);
                let text_color = if visible {
                    label_color
                } else {
                    label_color.with_alpha(0.5)
                };
                canvas.draw_text(
                    &series.name,
                    Rect::new(
                        band.x + style.legend_swatch_size + 4.0,
                        center_y - label_style.size * 0.6,
                        label_w,
                        label_style.size * 1.2,
                    ),
                    &label_style,
                    text_color,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    fn three_series() -> Vec<ChartSeries<String>> {
        vec![
            ChartSeries::<String>::new("A"),
            ChartSeries::<String>::new("B"),
            ChartSeries::<String>::new("C"),
        ]
    }

    #[test]
    fn one_swatch_per_series() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(ChartLegend::new(three_series()));
        tree.layout(SizeProposal::exact(300.0, 30.0));
        let frame = tree.render();
        // 3 swatches as rounded rects (Tier 2 shapes).
        assert_eq!(frame.shapes.len(), 3, "expected 3 swatch shapes");
    }

    #[test]
    fn empty_legend_has_zero_size() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(ChartLegend::new(Vec::<ChartSeries<String>>::new()));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        assert_eq!(b.width, 0.0);
        assert_eq!(b.height, 0.0);
    }

    #[test]
    fn vertical_orientation_changes_height() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id =
            tree.add(ChartLegend::new(three_series()).orientation(LegendOrientation::Vertical));
        tree.layout(SizeProposal::unspecified());
        let b = tree.bounds(id);
        // Vertical has 3 rows of swatches → height ≥ 3 * swatch size.
        assert!(b.height >= 30.0, "vertical should stack rows: got {}", b.height);
    }
}
