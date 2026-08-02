// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The cached viewport height every virtualizing widget keeps, and the one
//! rule about where its value may come from.
//!
//! A virtualizing widget ([`ListView`](crate::ListView),
//! [`TreeView`](crate::TreeView), the table/grid body panes) decides *in
//! `build`* how many rows to realize, long before `place_children` hands it a
//! rect. It therefore caches the height it was last given, and `build` sizes
//! its realization window from that cache.
//!
//! ## Why a proposal is not a viewport
//!
//! `layout_response` runs for two different questions:
//!
//! * **allocation** — "you get 654 px, lay yourself out", with
//!   `proposal.height == Some(654.0)`; and
//! * **measurement** — "how tall would you like to be at this width?", with
//!   `proposal.height == None`, which is what a `VStack` asks each child
//!   before distributing space.
//!
//! Both used to write the resolved height into the cache. The second one
//! writes the *fallback* — a constant like 200 px that has nothing to do with
//! the real viewport — and it runs on every layout pass, so the cache ends up
//! holding the fallback essentially all the time.
//!
//! Nothing goes wrong until the widget rebuilds. Then:
//!
//! 1. `build` reads the fallback (200) and realizes the rows for a 200 px
//!    viewport — 13 of them, say;
//! 2. `place_children` gets the true 654 px rect, recomputes the visible range
//!    honestly (24 rows), sees it reach past what `build` realized, and bumps
//!    the rebuild version to go and realize the rest;
//! 3. `build` reads the fallback again.
//!
//! That is a rebuild every frame, for ever. The rows are replaced before
//! layout can place them, so they keep their default zero rect: the widget
//! renders as an empty hole while burning a core. It survives scrolling,
//! clicking and re-selection, because every one of those triggers another
//! rebuild into the same loop.
//!
//! So: **only an allocation may write the cache** ([`viewport_size`]), and
//! `place_children` overwrites it with the rect it was actually given
//! ([`record_viewport_height`]) — the one number that is true by construction.

use std::cell::Cell;

use bastyde_canvas::geometry::{Size, SizeProposal};

/// Resolve a virtualizing widget's size from `proposal`, caching the height in
/// `viewport_height` **only when the parent actually offered one**.
///
/// `fallback` supplies each unspecified dimension for the returned size (the
/// widget still has to answer the measurement question); the height half of it
/// is deliberately never cached. See the module docs for what happens when it
/// is.
pub(crate) fn viewport_size(
    proposal: SizeProposal,
    viewport_height: &Cell<f32>,
    fallback: Size,
) -> Size {
    let size = proposal.resolve(fallback.width, fallback.height);
    if proposal.height.is_some() {
        viewport_height.set(size.height);
    }
    size
}

/// Record the height the widget was actually allocated. Call from
/// `place_children`, before any early return, so the cache reflects the last
/// real layout even for a pass that places nothing.
pub(crate) fn record_viewport_height(viewport_height: &Cell<f32>, height: f32) {
    viewport_height.set(height);
}

#[cfg(test)]
mod tests {
    use super::*;

    const FALLBACK: Size = Size {
        width: 300.0,
        height: 200.0,
    };

    #[test]
    fn an_allocation_updates_the_cache() {
        let cache = Cell::new(600.0);
        let size = viewport_size(SizeProposal::exact(400.0, 654.0), &cache, FALLBACK);
        assert_eq!(size, Size::new(400.0, 654.0));
        assert_eq!(cache.get(), 654.0);
    }

    /// The regression: a `VStack` measuring its children must not be able to
    /// overwrite the viewport with the fallback constant.
    #[test]
    fn a_measurement_leaves_the_cache_alone() {
        let cache = Cell::new(654.0);
        let size = viewport_size(SizeProposal::with_width(400.0), &cache, FALLBACK);
        // The widget still answers the question…
        assert_eq!(size, Size::new(400.0, FALLBACK.height));
        // …without claiming that answer is the viewport.
        assert_eq!(cache.get(), 654.0, "measurement must not touch the cache");
    }

    #[test]
    fn a_fully_unspecified_proposal_leaves_the_cache_alone() {
        let cache = Cell::new(654.0);
        let size = viewport_size(SizeProposal::unspecified(), &cache, FALLBACK);
        assert_eq!(size, Size::new(FALLBACK.width, FALLBACK.height));
        assert_eq!(cache.get(), 654.0);
    }

    /// A height-only proposal is still an allocation of the axis that matters.
    #[test]
    fn a_height_only_proposal_updates_the_cache() {
        let cache = Cell::new(600.0);
        let size = viewport_size(SizeProposal::with_height(120.0), &cache, FALLBACK);
        assert_eq!(size, Size::new(FALLBACK.width, 120.0));
        assert_eq!(cache.get(), 120.0);
    }

    #[test]
    fn placement_is_authoritative_over_any_earlier_proposal() {
        let cache = Cell::new(0.0);
        viewport_size(SizeProposal::exact(400.0, 200.0), &cache, FALLBACK);
        record_viewport_height(&cache, 654.0);
        assert_eq!(cache.get(), 654.0);
    }
}
