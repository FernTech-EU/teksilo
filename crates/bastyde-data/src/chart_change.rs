// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ChartChange — change notifications and stable series identifiers for chart collections.
//!
//! [`SeriesId`] is an opaque, stable handle for a series in a [`crate::ChartModel`].
//! Because `ChartModel` is backed by a slotmap, `SeriesId` values survive arbitrary
//! series insertions, removals, and reorders — only removing the series itself
//! invalidates it. [`ChartChange`] describes exactly what mutated (at the series
//! level or the point level within a series) so that projections
//! (`ChartWindow`, `ChartAggregate`) and consumers (`ChartSelection`) can refresh
//! or adjust incrementally instead of rebuilding from scratch.
//!
//! Consumers typically receive `ChartChange` values through an observer
//! registered via [`crate::ChartModel::observe_changes`], which fires
//! synchronously (before the registering call returns) after each mutation.
//!
//! ```ignore
//! // ChartModel::observe_changes returns an ObserverHandle whose drop
//! // unregisters the callback — keep it alive for the observer's lifetime.
//! use bastyde_data::{ChartModel, ChartChange};
//! let model: ChartModel<String> = ChartModel::new();
//! let _handle = model.observe_changes(|change| {
//!     println!("{change:?}");
//! });
//! model.add_series("Revenue");
//! // prints: SeriesInserted { index: 0, series: SeriesId(...) }
//! ```

/// Opaque identifier for a series in a `ChartModel`.
///
/// `SeriesId` values are stable across mutations — inserting or removing
/// other series does not invalidate existing `SeriesId` handles (they are
/// SlotMap keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesId(slotmap::DefaultKey);

impl SeriesId {
    pub(crate) fn from_key(key: slotmap::DefaultKey) -> Self {
        Self(key)
    }

    pub(crate) fn key(self) -> slotmap::DefaultKey {
        self.0
    }
}

/// Describes a mutation to a chart's series or point data. Emitted by
/// `ChartModel<T>` automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartChange {
    /// A series was inserted at the given index.
    SeriesInserted { index: usize, series: SeriesId },

    /// A series (and all of its points) was removed.
    SeriesRemoved { series: SeriesId },

    /// A series was moved to a new position among its siblings.
    SeriesMoved {
        series: SeriesId,
        from: usize,
        to: usize,
    },

    /// A series' display name changed.
    SeriesRenamed { series: SeriesId },

    /// A series' explicit color changed (set or cleared). The only variant
    /// that bumps [`crate::ChartModel::style_version`] rather than
    /// [`crate::ChartModel::structure_version`].
    SeriesColorChanged { series: SeriesId },

    /// A series' visibility flag changed.
    SeriesVisibilityChanged { series: SeriesId },

    /// Points were inserted; `range` holds the indices of the newly
    /// inserted points within `series`.
    PointsInserted {
        series: SeriesId,
        range: std::ops::Range<usize>,
    },

    /// Points were removed; `range` holds the indices they occupied
    /// *before* removal within `series`.
    PointsRemoved {
        series: SeriesId,
        range: std::ops::Range<usize>,
    },

    /// A single point's data changed in place without any structural shift.
    PointUpdated { series: SeriesId, index: usize },

    /// A series' entire point list was replaced; consumers must discard
    /// cached state for that series and rebuild it.
    SeriesDataReplaced { series: SeriesId },

    /// The entire chart was replaced. Consumers should discard all state
    /// and rebuild.
    Reset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_id_equality() {
        use slotmap::SlotMap;
        let mut sm: SlotMap<slotmap::DefaultKey, ()> = SlotMap::new();
        let k1 = sm.insert(());
        let k2 = sm.insert(());
        let id1 = SeriesId::from_key(k1);
        let id1_clone = SeriesId::from_key(k1);
        let id2 = SeriesId::from_key(k2);

        assert_eq!(id1, id1_clone);
        assert_ne!(id1, id2);
    }
}
