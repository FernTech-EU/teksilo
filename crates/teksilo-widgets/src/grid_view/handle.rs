// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`GridViewHandle`]: scroll a [`GridView`](super::GridView) from outside,
//! before it is laid out and across its rebuilds.
//!
//! The builder's own `scroll_to_index` / `ensure_index_visible` compute an
//! offset at call time, from the geometry of the last layout. Before the
//! first layout there is none, so they do nothing; and a change written in
//! the same turn, a detail band opened above the tile, is not in it yet. The
//! layout then keeps the content under the viewport's top edge still across
//! that change, treating the offset just written as one of the old layout,
//! and the tile lands a band's height away from where it was asked to be.
//!
//! A handle computes nothing when it is called. It records the request and
//! asks for a relayout, and `GridView::place_children` applies the request
//! once the band is measured and that anchoring has run: the offset is
//! computed from the layout it will be shown in.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_core::signal::Signal;

use super::layout::ScrollAnchor;

/// A reveal recorded by a [`GridViewHandle`], for the next layout to apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RevealRequest {
    /// Put the tile where `anchor` says, whatever is on screen now.
    ScrollTo { index: usize, anchor: ScrollAnchor },
    /// Scroll only when the tile is not fully visible, then per `anchor`.
    Ensure { index: usize, anchor: ScrollAnchor },
    /// The tile's row and its open detail band.
    Band(usize),
}

impl RevealRequest {
    /// The tile the request is about.
    pub(crate) fn index(self) -> usize {
        match self {
            Self::ScrollTo { index, .. } | Self::Ensure { index, .. } | Self::Band(index) => index,
        }
    }
}

/// The state a grid and every handle on it share. Made with the grid, so a
/// handle taken while the tree is being described reaches the same state the
/// mounted grid reads, and keeps reaching it through the grid's rebuilds.
#[derive(Debug)]
pub(crate) struct RevealShared {
    /// The latest request not yet applied. A later one replaces it.
    pending: Cell<Option<RevealRequest>>,
    /// Bound by the grid at `Relayout` in each build: bumped to have the
    /// next frame lay the grid out and apply `pending`. Unbound before the
    /// grid is mounted, when the first layout applies it anyway.
    relayout: Signal<u64>,
}

impl RevealShared {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            pending: Cell::new(None),
            relayout: Signal::new(0),
        })
    }

    /// The trigger the grid binds at `Relayout`.
    pub(crate) fn relayout_signal(&self) -> &Signal<u64> {
        &self.relayout
    }

    /// Take the pending request, leaving none.
    pub(crate) fn take(&self) -> Option<RevealRequest> {
        self.pending.take()
    }

    fn request(&self, request: RevealRequest) {
        self.pending.set(Some(request));
        self.relayout.set(self.relayout.get().wrapping_add(1));
    }
}

/// A handle on a [`GridView`](super::GridView)'s scroll position, for a
/// caller outside the widget: a toolbar, a "show current" command, a search
/// result, an observer of the application's own state.
///
/// Taken with [`GridView::handle`](super::GridView::handle), cheap to clone,
/// and usable at once: a request made before the grid is mounted is kept and
/// applied at its first layout, and the handle keeps working through every
/// rebuild of the grid. Each method records a request and asks for a
/// relayout; the layout applies it after it has measured the detail band and
/// settled the scroll anchoring, so a request made in the same turn as a
/// change of layout lands where the new layout puts the tile:
///
/// ```ignore
/// // Open the band of tile 0 and put tile 30 at the top, in one turn.
/// expanded.set(Some(0));
/// handle.scroll_to_index(30, ScrollAnchor::Start);
/// ```
///
/// Only the latest request is applied: a later one replaces one still
/// pending. A request for a tile the grid does not have when it is applied
/// does nothing.
#[derive(Clone)]
pub struct GridViewHandle {
    shared: Rc<RevealShared>,
}

impl std::fmt::Debug for GridViewHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GridViewHandle")
            .field("pending", &self.shared.pending.get())
            .finish()
    }
}

impl GridViewHandle {
    pub(crate) fn new(shared: Rc<RevealShared>) -> Self {
        Self { shared }
    }

    /// Scroll to tile `index`, forcing its position in the viewport per
    /// `anchor`: [`ScrollAnchor::Start`] puts its top at the viewport's top,
    /// [`Center`](ScrollAnchor::Center) centres it,
    /// [`End`](ScrollAnchor::End) puts its bottom at the viewport's bottom,
    /// and [`Auto`](ScrollAnchor::Auto) scrolls the least that shows it.
    /// Clamped to the scroll range. Applied at the next layout.
    pub fn scroll_to_index(&self, index: usize, anchor: ScrollAnchor) {
        self.shared
            .request(RevealRequest::ScrollTo { index, anchor });
    }

    /// Bring tile `index` into view: nothing when it is already fully
    /// visible, otherwise a scroll per `anchor`, as
    /// [`scroll_to_index`](Self::scroll_to_index). Applied at the next
    /// layout, against the layout it will be shown in.
    pub fn ensure_index_visible(&self, index: usize, anchor: ScrollAnchor) {
        self.shared.request(RevealRequest::Ensure { index, anchor });
    }

    /// Scroll so tile `index` and its open detail band are both visible:
    /// the least scroll that shows the span from the top of the tile's row
    /// to the bottom of its band, and the row first when the two do not fit
    /// together, so the band reads as the tile's. This is what the grid does
    /// itself when a user opens a band.
    ///
    /// When `index`'s band is not open at the layout that applies it (no
    /// [`detail_row`](super::GridView::detail_row), another tile disclosed,
    /// or a tile with nothing to disclose), the tile alone is brought into
    /// view, as [`ensure_index_visible`](Self::ensure_index_visible) with
    /// [`ScrollAnchor::Auto`]. Open the band and call this in the same turn:
    /// the band is measured before the request is applied.
    pub fn reveal_band(&self, index: usize) {
        self.shared.request(RevealRequest::Band(index));
    }
}
