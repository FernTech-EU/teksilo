// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared chart input for a complete model or a live tail projection.

use crate::{ChartChange, ChartModel, ChartWindow, SeriesId, SeriesView};
use teksilo_core::color_prop::ColorProp;
use teksilo_core::{ObserverHandle, Signal};

/// A reactive chart input. Conversions retain shared handles and copy no points.
///
/// Window point indices (including hover and selection indices) are local to
/// the visible tail. A window bounds the displayed data, not source storage.
#[derive(Debug)]
pub enum ChartSource<T: 'static> {
    /// All points in the model.
    Model(ChartModel<T>),
    /// The last configured number of points in each series.
    Window(ChartWindow<T>),
}

impl<T: 'static> Clone for ChartSource<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Model(model) => Self::Model(model.clone()),
            Self::Window(window) => Self::Window(window.clone()),
        }
    }
}

impl<T: 'static> From<ChartModel<T>> for ChartSource<T> {
    fn from(model: ChartModel<T>) -> Self {
        Self::Model(model)
    }
}
impl<T: 'static> From<ChartWindow<T>> for ChartSource<T> {
    fn from(window: ChartWindow<T>) -> Self {
        Self::Window(window)
    }
}

impl<T: 'static> ChartSource<T> {
    fn source(&self) -> ChartModel<T> {
        match self {
            Self::Model(model) => model.clone(),
            Self::Window(window) => window.source(),
        }
    }

    /// Read the visible series and borrowed point slices.
    pub fn with_all_series<R>(&self, f: impl FnOnce(&[SeriesView<'_, T>]) -> R) -> R {
        match self {
            Self::Model(model) => model.with_all_series(f),
            Self::Window(window) => window.with_all_series(f),
        }
    }

    /// Read one visible series, using window-local point indices.
    pub fn with_series_view<R>(
        &self,
        series: SeriesId,
        f: impl FnOnce(SeriesView<'_, T>) -> R,
    ) -> Option<R> {
        match self {
            Self::Model(model) => model.with_series_view(series, f),
            Self::Window(window) => window.with_series_view(series, f),
        }
    }

    /// Read series metadata from the source.
    pub fn with_series<R>(
        &self,
        series: SeriesId,
        f: impl FnOnce(&str, Option<&ColorProp>, bool) -> R,
    ) -> Option<R> {
        self.source().with_series(series, f)
    }

    /// Change source visibility, for interactive legends.
    pub fn set_series_visible(&self, series: SeriesId, visible: bool) {
        self.source().set_series_visible(series, visible);
    }

    /// Ordered series identifiers.
    pub fn series_ids(&self) -> Vec<SeriesId> {
        self.source().series_ids()
    }
    /// Number of series, including hidden series.
    pub fn series_count(&self) -> usize {
        self.source().series_count()
    }
    /// Series at the given display position.
    pub fn series_id_at(&self, index: usize) -> Option<SeriesId> {
        self.source().series_id_at(index)
    }
    /// Sole series, when the source has exactly one.
    pub fn only_series(&self) -> Option<SeriesId> {
        self.source().only_series()
    }
    /// Number of visible points in a series.
    pub fn point_count(&self, series: SeriesId) -> usize {
        self.with_series_view(series, |view| view.points.len())
            .unwrap_or(0)
    }

    /// Reactive version for geometry and accessible data.
    pub fn structure_version(&self) -> Signal<u64> {
        match self {
            Self::Model(model) => model.structure_version(),
            Self::Window(window) => window.structure_version(),
        }
    }

    /// Reactive version for colors and patterns.
    pub fn style_version(&self) -> Signal<u64> {
        match self {
            Self::Model(model) => model.style_version(),
            Self::Window(window) => window.style_version(),
        }
    }

    /// Observe changes using indices relative to this input.
    pub fn observe_changes(&self, f: impl Fn(&ChartChange) + 'static) -> ObserverHandle {
        match self {
            Self::Model(model) => model.observe_changes(f),
            Self::Window(window) => window.observe_changes(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_borrows_points_and_tracks_structure_separately_from_style() {
        struct NonClone;
        let model = ChartModel::new();
        let series = model.add_series("sensor");
        for value in 0..4 {
            model.push_point(series, NonClone, value as f32);
        }
        let window = ChartWindow::new(model.clone(), 2);
        let input = ChartSource::from(window.clone());
        model.with_series_view(series, |original| {
            input.with_series_view(series, |view| {
                assert!(std::ptr::eq(&original.points[2], &view.points[0]));
                assert_eq!(view.points.len(), 2);
            });
        });
        let version = input.structure_version().get();
        window.set_window_size(1);
        assert!(input.structure_version().get() > version);
        assert_eq!(input.point_count(series), 1);
        let version = input.structure_version().get();
        model.set_series_pattern(series, crate::SeriesPattern::Dotted);
        assert_eq!(input.structure_version().get(), version);
        assert!(input.style_version().get() > 0);
        input.set_series_visible(series, false);
        assert_eq!(
            model.with_series(series, |_, _, visible| visible),
            Some(false)
        );
        model.remove_series(series);
        assert_eq!(input.series_count(), 0);
        assert!(input.with_series_view(series, |_| ()).is_none());
    }
}
