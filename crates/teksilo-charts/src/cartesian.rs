// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The plot layout `BarChart` and `LineChart` share: the memoized
//! [`PlotGeometry`], the intrinsic compression floor and the legend's
//! placement. Each chart lends its fields through a [`CartesianFrame`] and
//! keeps only its own y-domain rule.

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal, TextBackend};
use teksilo_core::widget::{LayoutContext, LayoutResponse, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_data::{ChartSource, SeriesView};
use teksilo_tokens::{InputTokens, TextStyle, TextStyleRole};

use crate::axis::AxisConfig;
use crate::layout::{LegendPosition, PlotGeometry, PlotGeometryParams, compute_plot_geometry};
use crate::legend::{legend_main_axis_size, orientation_for_position};

/// Cache key for the memoized [`PlotGeometry`]: a miss recomputes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GeometryKey {
    bounds: Rect,
    structure_version: u64,
    /// The density the legend band was reserved at. An interactive legend's
    /// rows are sized from `InputTokens::target_size`, and a density switch
    /// changes neither the bounds nor the model version, so without this
    /// the memoized geometry would keep the band the previous density
    /// reserved.
    density: teksilo_tokens::TargetDensity,
}

/// The memoized geometry a chart holds, shared with its handlers.
pub(crate) type GeometryCache = Rc<RefCell<Option<(GeometryKey, PlotGeometry)>>>;

/// A borrowed view of the fields a cartesian chart lays its plot out from.
pub(crate) struct CartesianFrame<'a, T: Clone + 'static> {
    pub(crate) model: &'a ChartSource<T>,
    pub(crate) axis_x: &'a AxisConfig,
    pub(crate) axis_y: &'a AxisConfig,
    pub(crate) show_legend: bool,
    pub(crate) legend_position: LegendPosition,
    pub(crate) legend_interactive: bool,
    pub(crate) legend_id: Option<WidgetId>,
    pub(crate) input: &'a InputTokens,
    pub(crate) geometry_cache: &'a GeometryCache,
    /// The chart's own value range for its y axis.
    pub(crate) y_domain: fn(&AxisConfig, &[SeriesView<'_, T>]) -> (f32, f32),
}

impl<T: Clone + std::fmt::Display + 'static> CartesianFrame<'_, T> {
    fn shown_legend_position(&self) -> Option<LegendPosition> {
        self.show_legend.then_some(self.legend_position)
    }

    fn legend_size(
        &self,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &TextStyle,
        input: &InputTokens,
    ) -> f32 {
        if !self.show_legend {
            return 0.0;
        }
        legend_main_axis_size(
            backend,
            self.model,
            label_style,
            orientation_for_position(self.legend_position),
            self.legend_interactive,
            input,
        )
    }

    fn y_domain(&self) -> (f32, f32) {
        self.model
            .with_all_series(|views| (self.y_domain)(self.axis_y, views))
    }

    /// Memoized [`PlotGeometry`] for `bounds`, recomputed only when
    /// `bounds`, `model.structure_version()` or the target density changed
    /// since the last call. Shared by `paint()` and `accessibility()` so a
    /// mark's bounds never disagree between the visual tree and the AT
    /// tree, even without an intervening paint.
    pub(crate) fn ensure_geometry(
        &self,
        bounds: Rect,
        backend: Option<&Rc<RefCell<dyn TextBackend>>>,
        label_style: &TextStyle,
    ) -> PlotGeometry {
        let key = GeometryKey {
            bounds,
            structure_version: self.model.structure_version().get(),
            density: self.input.density,
        };
        if let Some((cached_key, geometry)) = self.geometry_cache.borrow().as_ref()
            && *cached_key == key
        {
            return geometry.clone();
        }

        let legend_size = self.legend_size(backend, label_style, self.input);
        let y_domain = self.y_domain();
        // Gathered here as well as at paint time: the carve needs them to size a tilted
        // label band, and the same first-visible-series rule must pick them both times.
        let x_labels: Vec<String> = self.model.with_all_series(|views| {
            let visible: Vec<&SeriesView<'_, T>> = views.iter().filter(|v| v.visible).collect();
            if visible.is_empty() {
                return Vec::new();
            }
            visible[0]
                .points
                .iter()
                .map(|d| format!("{}", d.category))
                .collect()
        });
        let geometry = compute_plot_geometry(&PlotGeometryParams {
            bounds,
            axis_x: self.axis_x,
            axis_y: self.axis_y,
            y_domain,
            legend_size,
            legend_position: self.shown_legend_position(),
            text_backend: backend,
            label_style,
            x_labels: &x_labels,
        });
        *self.geometry_cache.borrow_mut() = Some((key, geometry.clone()));
        geometry
    }

    /// The chart's layout response: the proposal (or a 320 x 200 fallback)
    /// as the ideal, shrinkable down to the intrinsic floor
    /// ([`crate::layout::compute_intrinsic_min`]).
    pub(crate) fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> LayoutResponse {
        let ideal = Size::new(
            proposal.width.unwrap_or(320.0),
            proposal.height.unwrap_or(200.0),
        );
        let plot_floor = Size::new(40.0, 40.0);
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let legend_size = self.legend_size(ctx.text_backend, &label_style, &ctx.theme.input);
        let min = crate::layout::compute_intrinsic_min(
            self.axis_x,
            self.axis_y,
            self.y_domain(),
            legend_size,
            self.shown_legend_position(),
            ctx.text_backend,
            &label_style,
            plot_floor,
        );
        LayoutResponse::shrinkable(ideal, min, 1.0)
    }

    /// Place the embedded legend, if any, in the band the geometry reserved.
    pub(crate) fn place_legend(
        &self,
        bounds: Rect,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let Some(legend_id) = self.legend_id else {
            return;
        };
        let label_style = TextStyleRole::Tiny.resolve(&ctx.theme.typography);
        let geometry = self.ensure_geometry(bounds, ctx.text_backend, &label_style);
        for child in children.iter_mut() {
            if child.id == legend_id {
                child.origin = Point::new(geometry.legend.x, geometry.legend.y);
                child.size = Size::new(geometry.legend.width, geometry.legend.height);
            }
        }
    }
}
