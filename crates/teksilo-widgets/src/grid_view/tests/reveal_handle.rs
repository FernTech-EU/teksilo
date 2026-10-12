// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `GridViewHandle`, and the band's visible span.
//!
//! The grids here are `band_grid`'s: tiles of 100x50, three columns at
//! 400 dp, rows stepping 58 dp, so row `r` spans `58r .. 58r + 50`, and a
//! band 80 dp tall (one row gap under its row) unless a test sizes it.

use std::cell::Cell;
use std::rc::Rc;

use super::*;

/// The grid's scroll offset and a handle on it.
fn scroll_and_handle(tree: &WidgetTree, id: WidgetId) -> (Signal<f32>, GridViewHandle) {
    let grid = the_grid(tree, id);
    (grid.scroll_y_signal().clone(), grid.handle())
}

/// Opening a band above a tile and asking for the tile at the top, in one
/// turn, lands it exactly there. Tile 30 is on row 10, at 580 without the
/// band and at 580 + 8 + 80 = 668 with the band of tile 0 open. An outside
/// `scroll.set(668)` in the same turn ends at 756: the layout keeps the
/// content under the top edge still across the band opening, and takes the
/// offset just written for one of the old layout.
#[test]
fn opening_a_band_and_scrolling_to_a_tile_in_one_turn_lands_exactly() {
    let (mut tree, id, _model, expanded) = band_grid(300, 400.0, 300.0, |g| g);
    let (scroll, handle) = scroll_and_handle(&tree, id);

    expanded.set(Some(0));
    handle.scroll_to_index(30, ScrollAnchor::Start);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 668.0, "the offset of tile 30 with the band");
    assert_close(tile_y(&tree, id, 30), 0.0, "tile 30 is at the top");
}

/// A request made before the grid is mounted is kept, and applied by the
/// first layout. The builder's own method does nothing then, as it says.
#[test]
fn a_request_made_before_mount_is_applied_at_the_first_layout() {
    let model = ListModel::from_vec((0..300).collect::<Vec<usize>>());
    let grid = GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0))).tile_size(100.0, 50.0);
    let handle = grid.handle();
    let scroll = grid.scroll_y_signal().clone();
    grid.scroll_to_index(60, ScrollAnchor::Start);
    assert_close(
        scroll.get(),
        0.0,
        "the builder's method has no layout to use",
    );
    handle.scroll_to_index(20, ScrollAnchor::Start);
    handle.scroll_to_index(30, ScrollAnchor::Start);

    let mut tree = WidgetTree::new();
    let id = tree.add(grid);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 580.0, "row 10, the later request");
    assert_close(tile_y(&tree, id, 30), 0.0, "tile 30 is at the top");
}

/// The handle keeps working through the grid's rebuilds: a change to the
/// data rebuilds it, and a request made afterwards is still applied.
#[test]
fn the_handle_works_across_rebuilds() {
    let (mut tree, id, model, _expanded) = band_grid(300, 400.0, 300.0, |g| g);
    let (scroll, handle) = scroll_and_handle(&tree, id);
    model.push(300);
    settle(&mut tree, 400.0, 300.0);
    handle.scroll_to_index(30, ScrollAnchor::Start);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 580.0, "row 10");
    model.push(301);
    settle(&mut tree, 400.0, 300.0);
    handle.scroll_to_index(3, ScrollAnchor::Start);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 58.0, "row 1");
}

/// `ensure_index_visible` leaves a tile that is already fully visible where
/// it is, whatever the anchor, and scrolls a tile that is not.
#[test]
fn ensure_index_visible_does_not_scroll_to_a_visible_tile() {
    let (mut tree, id, _model, _expanded) = band_grid(300, 400.0, 300.0, |g| g);
    let (scroll, handle) = scroll_and_handle(&tree, id);
    scroll.set(100.0);
    settle(&mut tree, 400.0, 300.0);

    // Row 2 is 116..166, inside 100..400.
    handle.ensure_index_visible(6, ScrollAnchor::Start);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 100.0, "tile 6 was visible: no scroll");

    handle.ensure_index_visible(30, ScrollAnchor::Start);
    settle(&mut tree, 400.0, 300.0);
    assert_close(
        scroll.get(),
        580.0,
        "tile 30 was not: to its row, at the top",
    );
}

/// `reveal_band` shows a tile and its band together: from above, the least
/// scroll that brings the band's bottom in; from below, the tile's row at
/// the top.
#[test]
fn reveal_band_shows_the_tile_and_its_band() {
    let (mut tree, id, _model, expanded) = band_grid(300, 400.0, 300.0, |g| g);
    let (scroll, handle) = scroll_and_handle(&tree, id);

    // Row 10 is 580..630, its band 638..718.
    expanded.set(Some(30));
    handle.reveal_band(30);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 418.0, "the band's bottom at the viewport's");
    assert!(
        within(tree.bounds(tile_at(&tree, id, 30)), 0.0, 300.0),
        "the tile is on screen"
    );
    assert!(
        within(tree.bounds(band_of(&tree, id)), 0.0, 300.0),
        "and so is its band, at {:?}",
        tree.bounds(band_of(&tree, id))
    );

    scroll.set(3000.0);
    settle(&mut tree, 400.0, 300.0);
    handle.reveal_band(30);
    settle(&mut tree, 400.0, 300.0);
    assert_close(scroll.get(), 580.0, "from below, the row at the top");
}

/// The band's visible span follows a scroll through a band taller than the
/// viewport, is `None` above and below it and while it is closed, and does
/// not notify a layout that leaves it as it was.
#[test]
fn the_bands_visible_span_follows_the_scroll() {
    let (mut tree, id, _model, expanded) =
        band_grid(300, 400.0, 300.0, |g| g.detail_row_height(|_i| 1000.0));
    let (scroll, _handle) = scroll_and_handle(&tree, id);
    let span = the_grid(&tree, id).detail_visible_span_signal().clone();
    let notified = Rc::new(Cell::new(0));
    let count = notified.clone();
    let _observer = span.observe(move |_| count.set(count.get() + 1));
    assert_eq!(span.get(), None, "no band is open");

    // The band of tile 0 spans 58..1058.
    expanded.set(Some(0));
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(span.get(), Some(0.0..242.0), "its top 242 dp, under row 0");

    for (y, expected) in [
        (500.0, Some(442.0..742.0)),
        (900.0, Some(842.0..1000.0)),
        (1100.0, None),
    ] {
        scroll.set(y);
        settle(&mut tree, 400.0, 300.0);
        assert_eq!(span.get(), expected, "scrolled to {y}");
    }

    let before = notified.get();
    scroll.set(1200.0);
    settle(&mut tree, 400.0, 300.0);
    scroll.set(1200.0);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(notified.get(), before, "still off screen: no notification");

    scroll.set(500.0);
    settle(&mut tree, 400.0, 300.0);
    let before = notified.get();
    scroll.set(500.0);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(notified.get(), before, "the same span: no notification");

    expanded.set(None);
    settle(&mut tree, 400.0, 300.0);
    assert_eq!(span.get(), None, "the band is closed");
}
