// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a finger's drag does to a data view, and what a mouse's still does.
//!
//! A drag and a scroll want the same gesture from the same contact, and the
//! framework settles that with [`DragActivation`]: a precise pointer latches
//! immediately, a direct one waits for a long press so the scrollable
//! underneath gets first refusal. The policy is read on two paths — the ancestor
//! walk of the tree's sequence enrolment, and `PointerSequence::defer_own_drag`
//! for a node that is *already* the sequence's pan claimant — so a drag on the
//! capturing node binds it only when that node holds the claim itself. A row
//! never does: the claim is the scrollable's, several levels out. So a drag on
//! the row that captured the press is invisible to the policy and latches at
//! `drag_slop` regardless, which is why every data view hangs its drag on an
//! enclosing `DragSurface`.
//!
//! `GridView`'s rubber-band marquee used to be exactly that: on the same node as
//! the `PanClaim`, latching at 18 dp before the claim could win at 36 — so a
//! `Multi`-selection grid did not scroll under a finger from anywhere, and a
//! press that landed on a tile got nothing at all, because the marquee declines
//! such a press after winning the arbitration for it. A finger on the empty
//! background did sweep a band, immediately rather than after a hold. All three
//! are asserted here, plus the mouse behaviour that must not move.
//!
//! [`DragActivation`]: teksilo_tokens::DragActivation

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{ListModel, SelectionMode, SelectionModel};
use teksilo_i18n::lit;
use teksilo_widgets::primitives::TextWidget;
use teksilo_widgets::{GridSizing, GridView};

const VIEWPORT: f32 = 400.0;
const TILE_W: f32 = 160.0;
const TILE_H: f32 = 40.0;
const ITEMS: usize = 200;

/// x of a column of background: two 160 dp columns fit in 400, leaving an 80 dp
/// gutter every row where a marquee may legitimately begin. Kept well clear of
/// the trailing edge, because the scroll bar's touch grab reaches inward from
/// there (`hit_outset`) and a press it takes is neither a pan nor a marquee.
const BACKGROUND_X: f32 = 330.0;

fn tree_with(root: impl teksilo_core::widget::Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(root);
    tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    (tree, id)
}

fn multi_grid(sel: &SelectionModel) -> GridView<usize> {
    GridView::new(ListModel::from_vec((0..ITEMS).collect()), |tc| {
        Box::new(TextWidget::new(lit!(tc.item.to_string())))
    })
    .sizing(GridSizing::Fixed {
        width: TILE_W,
        height: TILE_H,
    })
    .selection(sel.clone())
}

fn hold() -> std::time::Duration {
    teksilo_tokens::GestureProfile::TOUCH.long_press
}

fn drag_slop() -> f32 {
    teksilo_tokens::GestureProfile::TOUCH.drag_slop
}

fn pan_slop() -> f32 {
    teksilo_core::gesture::default_profile(teksilo_tokens::PointerKind::Touch)
        .pan_slop
        .expect("a touch profile pans")
}

// ---------------------------------------------------------------------------
// the mouse half — no regression
// ---------------------------------------------------------------------------

/// A mouse still sweeps a rubber band from the grid's background, and still
/// latches it at the mouse's own `drag_slop`.
#[test]
fn a_mouse_drag_on_the_grid_background_still_marquees() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    let (mut tree, _id) = tree_with(multi_grid(&sel));

    let from = Point::new(BACKGROUND_X, 300.0);
    tree.pointer_down_button(from, teksilo_core::event::PointerButton::Primary);
    tree.pointer_move(Point::new(BACKGROUND_X, 290.0));
    tree.pointer_move(Point::new(10.0, 20.0));
    tree.pointer_up_button(
        Point::new(10.0, 20.0),
        teksilo_core::event::PointerButton::Primary,
    );

    assert!(
        sel.count() > 0,
        "the mouse marquee selected nothing; it is the no-regression case",
    );
}

// ---------------------------------------------------------------------------
// the finger half
// ---------------------------------------------------------------------------

/// The defect this package inherited, from the other side: a finger's pan wins
/// on a `Multi`-selection grid, and the marquee does not steal it.
#[test]
fn a_finger_pan_on_a_multi_select_grid_scrolls_and_selects_nothing() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    let view = multi_grid(&sel);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let from = Point::new(BACKGROUND_X, 300.0);
    tree.touch_down(finger, from);
    tree.touch_move(finger, Point::new(from.x, from.y - pan_slop() - 1.0));
    tree.touch_move(finger, Point::new(from.x, from.y - pan_slop() - 121.0));
    tree.touch_up(finger, Point::new(from.x, from.y - pan_slop() - 121.0));

    assert!(
        offset.get() > 0.0,
        "the finger must scroll the grid; the offset stayed at {}",
        offset.get(),
    );
    assert_eq!(sel.count(), 0, "and must not have swept a rubber band");
}

/// …and the marquee is not lost, only deferred: hold the contact still past the
/// profile's `long_press` and the same drag sweeps.
///
/// The hold ripens because the one virtual input clock was advanced, never
/// because the test took long enough.
#[test]
fn a_finger_marquees_the_grid_after_a_long_press() {
    let sel = SelectionModel::new(SelectionMode::Multi);
    let view = multi_grid(&sel);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let from = Point::new(BACKGROUND_X, 300.0);
    tree.touch_down(finger, from);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    // Past `drag_slop`, which before the hold would have gone to the pan.
    tree.touch_move(finger, Point::new(from.x, from.y - drag_slop() - 2.0));
    tree.touch_move(finger, Point::new(10.0, 20.0));
    tree.touch_up(finger, Point::new(10.0, 20.0));

    assert!(
        sel.count() > 0,
        "the held contact must sweep a rubber band, not scroll",
    );
    assert_eq!(offset.get(), 0.0, "and the grid must not have scrolled");
}

// ---------------------------------------------------------------------------
// row reorder: the same rule, one level down
// ---------------------------------------------------------------------------

/// A reorderable list is a scrollable whose every row is a drag source, so it
/// is the same collision as the grid's marquee and it has the same fix: the
/// reorder drag hangs on a `DragSurface` enclosing the row, never on the row
/// itself.
fn reorderable_list(model: &ListModel<usize>) -> teksilo_widgets::ListView<usize> {
    teksilo_widgets::ListView::new(model.clone(), |i, _item, _s| {
        Box::new(TextWidget::new(lit!(i.to_string())))
    })
    .item_height(TILE_H)
    .reorderable(true)
}

#[test]
fn a_finger_pans_a_reorderable_list_rather_than_reordering_it() {
    let model = ListModel::from_vec((0..ITEMS).collect());
    let view = reorderable_list(&model);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let from = Point::new(200.0, 300.0);
    tree.touch_down(finger, from);
    tree.touch_move(finger, Point::new(from.x, from.y - pan_slop() - 1.0));
    tree.touch_move(finger, Point::new(from.x, from.y - pan_slop() - 121.0));
    tree.touch_up(finger, Point::new(from.x, from.y - pan_slop() - 121.0));

    assert!(
        offset.get() > 0.0,
        "a reorderable list must still scroll under a finger; offset {}",
        offset.get(),
    );
    assert_eq!(
        model.with_item(0, |v| *v),
        Some(0),
        "and the pan must not have reordered anything",
    );
}

/// A mouse still starts the reorder drag at its own `drag_slop`, from the same
/// press — the no-regression statement for moving the handler outward.
#[test]
fn a_mouse_still_reorders_a_list_row() {
    let model = ListModel::from_vec((0..ITEMS).collect());
    let view = reorderable_list(&model);
    let (mut tree, _id) = tree_with(view);

    // Row 1 (y 40..80) dragged down onto row 4.
    tree.pointer_down_button(
        Point::new(200.0, 60.0),
        teksilo_core::event::PointerButton::Primary,
    );
    tree.pointer_move(Point::new(200.0, 70.0));
    tree.pointer_move(Point::new(200.0, 190.0));
    tree.pointer_up_button(
        Point::new(200.0, 190.0),
        teksilo_core::event::PointerButton::Primary,
    );

    assert_ne!(
        model.with_item(1, |v| *v),
        Some(1),
        "the mouse drag must still have reordered the list",
    );
}

/// …and the reorder is deferred, not lost: hold the contact on the row past the
/// profile's `long_press` and the drag latches on the next sample.
///
/// This is half of the plan's collision rule: the hold is what *arms* the
/// reorder, so a row that is a drag source can no longer offer it for anything
/// else and its context menu has to move to an overflow affordance plus
/// Secondary / Shift+F10 / the AccessKit `ShowContextMenu` action. The other
/// half — that nothing else fires on that same hold — is asserted by
/// `a_reorderable_rows_hold_does_not_also_fire_its_own_long_press`, together
/// with the mouse twin that pins the hold the rule must *not* take.
#[test]
fn a_finger_reorders_a_list_row_after_a_long_press() {
    let model = ListModel::from_vec((0..ITEMS).collect());
    let view = reorderable_list(&model);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let from = Point::new(200.0, 60.0); // row 1
    tree.touch_down(finger, from);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    tree.touch_move(finger, Point::new(from.x, from.y + drag_slop() + 2.0));
    tree.touch_move(finger, Point::new(from.x, 190.0));
    tree.touch_up(finger, Point::new(from.x, 190.0));

    assert_ne!(
        model.with_item(1, |v| *v),
        Some(1),
        "the held contact must reorder rather than scroll",
    );
    assert_eq!(offset.get(), 0.0, "and must not have scrolled");
}

/// The same question of the other four views. A reorderable data view is a
/// scrollable every one of whose rows is a drag source, so without the
/// enclosing `DragSurface` the row's drag latches at 18 dp and the view does
/// not scroll under a finger at all — for any of them.
///
/// The mouse half needs no new test here: each view already has a pointer
/// drag-reorder test of its own beside its widget, and those are what say the
/// 5 dp latch and the drop still work with the handler one node further out.
fn assert_a_finger_pans_a_reorderable_view(
    view: impl teksilo_core::widget::Widget + 'static,
    offset: teksilo_core::signal::Signal<f32>,
    at: Point,
    what: &str,
) {
    let (mut tree, _id) = tree_with(view);
    let finger = tree.new_contact();
    tree.touch_down(finger, at);
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 1.0));
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 121.0));
    tree.touch_up(finger, Point::new(at.x, at.y - pan_slop() - 121.0));
    assert!(
        offset.get() > 0.0,
        "a reorderable {what} must still scroll under a finger; offset {}",
        offset.get(),
    );
}

#[test]
fn a_finger_pans_a_reorderable_grid() {
    let view = GridView::new(ListModel::from_vec((0..ITEMS).collect()), |tc| {
        Box::new(TextWidget::new(lit!(tc.item.to_string())))
    })
    .sizing(GridSizing::Fixed {
        width: TILE_W,
        height: TILE_H,
    })
    .reorderable(true);
    let offset = view.scroll_y_signal().clone();
    // On a tile, not the gutter: the tile is the drag source here.
    assert_a_finger_pans_a_reorderable_view(view, offset, Point::new(80.0, 300.0), "grid");
}

#[test]
fn a_finger_pans_a_reorderable_tree() {
    let model: teksilo_data::TreeModel<usize> = teksilo_data::TreeModel::new();
    for i in 0..ITEMS {
        model.insert_root(i, i);
    }
    let offset = teksilo_core::signal::Signal::new_animated(0.0);
    let view = teksilo_widgets::TreeView::new(model, |item, _entry, _sel| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(TILE_H)
    .scroll_signal(offset.clone())
    .reorderable(true);
    assert_a_finger_pans_a_reorderable_view(view, offset, Point::new(200.0, 300.0), "tree");
}

#[test]
fn a_finger_pans_a_reorderable_table() {
    let view = teksilo_widgets::table_view::TableView::new(ListModel::from_vec(
        (0..ITEMS).collect::<Vec<usize>>(),
    ))
    .add_column(
        teksilo_widgets::table_view::Column::new("n", lit!("N"), |v: &usize, _cx| {
            Box::new(TextWidget::new(lit!(v.to_string())))
        })
        .width(teksilo_widgets::table_view::ColumnWidth::Fixed(300.0)),
    )
    .row_height(TILE_H)
    .reorderable(true);
    let offset = view.scroll_y_signal().clone();
    assert_a_finger_pans_a_reorderable_view(view, offset, Point::new(150.0, 300.0), "table");
}

#[test]
fn a_finger_pans_a_reorderable_tree_table() {
    let model: teksilo_data::TreeModel<usize> = teksilo_data::TreeModel::new();
    for i in 0..ITEMS {
        model.insert_root(i, i);
    }
    let view = teksilo_widgets::TreeTableView::new(model)
        .add_column(
            teksilo_widgets::table_view::Column::new("n", lit!("N"), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(teksilo_widgets::table_view::ColumnWidth::Fixed(300.0)),
        )
        .row_height(TILE_H)
        .reorderable(true);
    let offset = view.scroll_y_signal().clone();
    assert_a_finger_pans_a_reorderable_view(view, offset, Point::new(150.0, 300.0), "tree table");
}

// ---------------------------------------------------------------------------
// the column header strip
// ---------------------------------------------------------------------------

fn header_table(col_reorderable: bool) -> teksilo_widgets::table_view::TableView<usize> {
    teksilo_widgets::table_view::TableView::new(ListModel::from_vec(
        (0..ITEMS).collect::<Vec<usize>>(),
    ))
    .header_height(32.0)
    .add_column(
        teksilo_widgets::table_view::Column::new("n", lit!("N"), |v: &usize, _cx| {
            Box::new(TextWidget::new(lit!(v.to_string())))
        })
        .width(teksilo_widgets::table_view::ColumnWidth::Fixed(300.0))
        .reorderable(col_reorderable),
    )
    .row_height(TILE_H)
}

/// A finger scrolls the table from its column-header strip.
///
/// It could not, and nothing said so. The header cell answered `Handled` to
/// the `PointerDown`, which the router reads as a preview claim and uses to
/// decide the sequence — after which the table's `PanClaim` is never evaluated.
/// Not the column reorder: the same zero was measured with every column
/// `reorderable(false)`.
#[test]
fn a_finger_pans_a_table_from_its_column_header() {
    let view = header_table(true);
    let offset = view.scroll_y_signal().clone();
    offset.set(400.0);
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let at = Point::new(100.0, 16.0); // inside the 32 dp header strip
    tree.touch_down(finger, at);
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 1.0));
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 121.0));
    tree.touch_up(finger, Point::new(at.x, at.y - pan_slop() - 121.0));

    assert!(
        offset.get() > 400.0,
        "the header strip must be a pan surface; the offset stayed at {}",
        offset.get(),
    );
}

/// …and that pan must not also sort the column it started on. The header's
/// sort was a sixth unguarded release-time commit; it now asks
/// `data_views::release_completes_the_press`, which a won `PanClaim` answers
/// `false`.
#[test]
fn a_finger_pan_from_a_column_header_does_not_sort_it() {
    let view = header_table(true).add_column(
        teksilo_widgets::table_view::Column::new("m", lit!("M"), |v: &usize, _cx| {
            Box::new(TextWidget::new(lit!(v.to_string())))
        })
        .sortable(true),
    );
    let sort = view.sort_signal().clone();
    let offset = view.scroll_y_signal().clone();
    offset.set(400.0);
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let at = Point::new(100.0, 16.0);
    tree.touch_down(finger, at);
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 1.0));
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 121.0));
    tree.touch_up(finger, Point::new(at.x, at.y - pan_slop() - 121.0));

    assert!(offset.get() > 400.0, "precondition: the pan scrolled");
    assert_eq!(
        sort.get(),
        None,
        "the pan sorted the column the finger happened to start on",
    );
}

/// The no-regression half: a real click on a sortable header still cycles the
/// sort, and a press that wanders off the header before release does not —
/// which is what the `PointerDown` arm's own comment always claimed and the
/// code did not check.
#[test]
fn a_mouse_click_on_a_column_header_sorts_it_and_a_wander_off_does_not() {
    // Columns are NOT reorderable here, and that is load-bearing for the
    // second half: with the default `Column::reorderable(true)` a press that
    // travels more than `DRAG_REORDER_THRESHOLD` escalates to a column-reorder
    // drag, which clears the recorded press itself — so the sort would be
    // suppressed by the reorder rather than by the movement check under test.
    let sortable = || {
        header_table(false).add_column(
            teksilo_widgets::table_view::Column::new("m", lit!("M"), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .sortable(true)
            .reorderable(false),
        )
    };

    let view = sortable();
    let sort = view.sort_signal().clone();
    let (mut tree, _id) = tree_with(view);
    let at = Point::new(320.0, 16.0); // the second column's header
    tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
    tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
    assert_eq!(
        sort.get().map(|(id, _)| id),
        Some("m".to_string()),
        "a plain click must still cycle the sort",
    );

    let view = sortable();
    let sort = view.sort_signal().clone();
    let (mut tree, _id) = tree_with(view);
    tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
    // Far past `DRAG_REORDER_THRESHOLD`, and deliberately still inside the SAME
    // header cell: a release that lands on a different cell finds no recorded
    // press there and would not sort for a reason that has nothing to do with
    // the movement check.
    let away = Point::new(at.x + 50.0, at.y);
    tree.pointer_move(away);
    tree.pointer_up_button(away, teksilo_core::event::PointerButton::Primary);
    assert_eq!(
        sort.get(),
        None,
        "a press dragged across the header is not a click and must not sort",
    );
}

/// The other half of the sort guard, and the only shape that isolates it: a
/// contact that travels far enough for the table's `PanClaim` to win, then
/// comes back and lifts **within** `DRAG_REORDER_THRESHOLD` of where it
/// pressed. The movement check is satisfied and cannot help; only
/// `release_completes_the_press` — which a won claim answers `false` — stops
/// the header sorting a column the user was scrolling.
#[test]
fn a_pan_that_returns_to_its_origin_still_does_not_sort_the_header() {
    let view = header_table(true).add_column(
        teksilo_widgets::table_view::Column::new("m", lit!("M"), |v: &usize, _cx| {
            Box::new(TextWidget::new(lit!(v.to_string())))
        })
        .sortable(true),
    );
    let sort = view.sort_signal().clone();
    let offset = view.scroll_y_signal().clone();
    offset.set(400.0);
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let at = Point::new(320.0, 16.0);
    tree.touch_down(finger, at);
    tree.touch_move(finger, Point::new(at.x, at.y - pan_slop() - 20.0));
    let back = Point::new(at.x, at.y + 2.0);
    tree.touch_move(finger, back);
    tree.touch_up(finger, back);

    assert_ne!(offset.get(), 400.0, "precondition: the claim was taken");
    assert_eq!(
        sort.get(),
        None,
        "the release landed 2 dp from the press, but the press was long gone",
    );
}

/// A short, narrow, sortable table — one wide column, four rows, nothing to
/// scroll in either axis. What it isolates is the *tap*: a header press here
/// competes with nothing but itself.
fn unscrollable_sortable_table() -> teksilo_widgets::table_view::TableView<usize> {
    teksilo_widgets::table_view::TableView::new(ListModel::from_vec((0..4).collect::<Vec<usize>>()))
        .header_height(32.0)
        .row_height(TILE_H)
        .add_column(
            teksilo_widgets::table_view::Column::new("m", lit!("M"), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(teksilo_widgets::table_view::ColumnWidth::Fixed(380.0))
            .sortable(true)
            // Off, so a slide cannot be suppressed by the reorder clearing the
            // recorded press instead of by the press itself ending.
            .reorderable(false),
        )
}

/// A finger tap on a sortable column header cycles its sort.
#[test]
fn a_finger_tap_on_a_column_header_sorts_it() {
    let view = unscrollable_sortable_table();
    let sort = view.sort_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let at = Point::new(100.0, 16.0);
    tree.touch_down(finger, at);
    tree.touch_up(finger, at);

    assert_eq!(
        sort.get().map(|(id, _)| id),
        Some("m".to_string()),
        "a finger must be able to sort by tapping the header",
    );
}

/// …and a finger that *slides* across the header does not sort it, even with the
/// contact never leaving the cell it pressed.
///
/// A finger's press survives movement further than a mouse's does — its
/// `TapBoundary` is the pressed node's own bounds, not a `tap_slop` radius — so
/// a 200 dp slide inside a 380 dp header cell does not leave it. What closes the
/// press is the table's `PanClaim` winning the contact, and a claim is won on
/// the gesture, not on there being anywhere to scroll: this fixture has
/// `max_scroll_y` 0 and the press ends all the same.
#[test]
fn a_finger_slide_across_a_column_header_does_not_sort_it() {
    let view = unscrollable_sortable_table();
    let sort = view.sort_signal().clone();
    let max_y = view.max_scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    assert_eq!(
        max_y.get(),
        0.0,
        "precondition: the fixture really has nothing to scroll",
    );

    let finger = tree.new_contact();
    let at = Point::new(100.0, 16.0);
    tree.touch_down(finger, at);
    let away = Point::new(300.0, 16.0); // still inside the same 380 dp cell
    tree.touch_move(finger, away);
    tree.touch_up(finger, away);

    assert_eq!(
        sort.get(),
        None,
        "a 200 dp slide is not a tap and must not sort",
    );
}

/// A table wide enough to pan sideways, with two equal columns so which one is
/// first identifies a reorder.
fn wide_table() -> teksilo_widgets::table_view::TableView<usize> {
    let mut v = teksilo_widgets::table_view::TableView::new(ListModel::from_vec(
        (0..ITEMS).collect::<Vec<usize>>(),
    ))
    .header_height(32.0)
    .row_height(TILE_H);
    for name in ["a", "b", "c", "d", "e", "f"] {
        v = v.add_column(
            teksilo_widgets::table_view::Column::new(name, lit!(name), |v: &usize, _cx| {
                Box::new(TextWidget::new(lit!(v.to_string())))
            })
            .width(teksilo_widgets::table_view::ColumnWidth::Fixed(150.0)),
        );
    }
    v
}

/// Swipe the second column's header leftward, far enough that a drop would put
/// it first. `hold` inserts a long press before the first sample.
fn swipe_header_leftward(tree: &mut WidgetTree, hold_first: bool) {
    let finger = tree.new_contact();
    let at = Point::new(220.0, 16.0); // inside column `b` (150..300)
    tree.touch_down(finger, at);
    if hold_first {
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    }
    for x in [210.0_f32, 180.0, 100.0, 30.0] {
        tree.touch_move(finger, Point::new(x, 16.0));
    }
    tree.touch_up(finger, Point::new(30.0, 16.0));
}

/// A finger's **horizontal** swipe along the header strip pans the table
/// sideways; it does not pick the column up.
///
/// The vertical case above is answered by the strip no longer claiming the
/// press. This one is not: the cell keeps receiving moves while the contact is
/// over it, and the reorder escalates from a raw `PointerMove` at
/// `DRAG_REORDER_THRESHOLD` — 5 dp, well under any pan slop. Measured before
/// the hold gate, on exactly this gesture: `max_scroll_x` 512, `scroll_x` **0**,
/// and the columns swapped. A horizontal swipe on a horizontally scrollable
/// table is a scroll on every touch platform there is, so it wins.
#[test]
fn a_finger_swipe_along_the_column_header_pans_sideways_and_does_not_reorder() {
    let view = wide_table();
    let order = view.column_order_signal().clone();
    let scroll_x = view.scroll_x_signal().clone();
    let max_x = view.max_scroll_x_signal().clone();
    let (mut tree, _id) = tree_with(view);

    assert!(
        max_x.get() > 0.0,
        "precondition: the fixture must be able to pan sideways",
    );
    // Nothing has written the order yet — only a reorder does, which is what
    // makes an unchanged value here a real statement rather than a tautology.
    let before = order.get();
    swipe_header_leftward(&mut tree, false);

    assert!(
        scroll_x.get() > 0.0,
        "the swipe must scroll the table sideways; scroll_x stayed {}",
        scroll_x.get(),
    );
    assert_eq!(
        order.get(),
        before,
        "and must not have reordered the columns",
    );
}

/// …and the reorder is deferred, not removed: hold the contact on the header
/// past `long_press` and the same swipe carries the column.
///
/// The row reorder gets this from `DragActivation`, which the tree resolves for
/// a drag enrolled as a sequence member. A raw `start_drag` out of
/// `PointerMove` is not one, so the header reads a `long_press` recognizer
/// instead — the same ruling, reached by the only hold source that path has.
#[test]
fn a_finger_reorders_a_column_after_a_long_press_on_its_header() {
    let view = wide_table();
    let order = view.column_order_signal().clone();
    let scroll_x = view.scroll_x_signal().clone();
    let (mut tree, _id) = tree_with(view);

    assert!(
        order.get().is_empty(),
        "precondition: nothing has written the column order yet",
    );
    swipe_header_leftward(&mut tree, true);

    assert_eq!(
        order.get().first().map(String::as_str),
        Some("b"),
        "the held contact must carry the column; order {:?}",
        order.get(),
    );
    assert_eq!(
        scroll_x.get(),
        0.0,
        "and must not have panned as well; scroll_x {}",
        scroll_x.get(),
    );
}

// ---------------------------------------------------------------------------
// the wrapper must be invisible to assistive technology
// ---------------------------------------------------------------------------

/// The `DragSurface` sits between the container and the row, so an AT client
/// walking `ListBox > … > ListBoxOption` would balk if it emitted a node of its
/// own. It sets no accessibility properties, which is what makes the walker
/// prune it — and the statement of that is the strongest one available: a
/// reorderable view's accessibility tree has the **same shape** as the same view
/// without the wrapper. Asserted through the real consumer rather than trusted,
/// because a property added to the wrapper later would break object navigation
/// silently.
#[test]
fn a_reorderable_view_has_the_same_accessibility_shape() {
    fn shape(reorderable: bool) -> String {
        let view = teksilo_widgets::ListView::new(
            ListModel::from_vec((0..8).collect::<Vec<usize>>()),
            |i, _item, _s| Box::new(TextWidget::new(lit!(i.to_string()))),
        )
        .item_height(TILE_H)
        .reorderable(reorderable);
        let (tree, _id) = tree_with(view);
        let snapshot = tree.accessibility_tree_snapshot();
        let consumer = accesskit_consumer::Tree::new(snapshot, true);

        fn walk(node: accesskit_consumer::NodeRef<'_>, depth: usize, out: &mut String) {
            out.push_str(&"  ".repeat(depth));
            out.push_str(&format!("{:?}\n", node.role()));
            for child in node.children() {
                walk(child, depth + 1, out);
            }
        }
        let mut out = String::new();
        walk(consumer.state().root(), 0, &mut out);
        out
    }

    let plain = shape(false);
    assert!(
        plain.contains("ListBoxOption"),
        "precondition: the fixture publishes rows\n{plain}",
    );
    assert_eq!(
        shape(true),
        plain,
        "the drag wrapper must be invisible to assistive technology",
    );
}

// ---------------------------------------------------------------------------
// drop bands on a tree row
// ---------------------------------------------------------------------------

/// 200 roots at a row height short enough for the coarse floor to bite.
///
/// The floor only changes anything below `COARSE_MIN_BAND / MAX_EDGE_FRACTION`
/// of row — at 40 dp the plain third is already wider than the floor and both
/// pointer kinds get the same bands, so a taller fixture could not tell them
/// apart. At 28 dp the edge band is 9.33 dp for a cursor and 11.2 for a finger,
/// and a drop at y 10 falls on opposite sides of that.
fn short_row_tree() -> (
    teksilo_data::TreeModel<usize>,
    teksilo_widgets::TreeView<usize>,
) {
    let model: teksilo_data::TreeModel<usize> = teksilo_data::TreeModel::new();
    for i in 0..200 {
        model.insert_root(i, i);
    }
    let view = teksilo_widgets::TreeView::new(model.clone(), |item, _e, _s| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(28.0)
    .reorderable(true);
    (model, view)
}

/// Drag row 2 onto row 0 and release 10 dp down its 28 dp height. `finger`
/// picks the pointer; the intermediate sample is what latches the drag and is
/// kept inside the pressed row (see the `#[ignore]`d test below for why that
/// matters).
fn drop_row_two_at(tree: &mut WidgetTree, finger: bool, y: f32) {
    let from = Point::new(200.0, 60.0); // row 2 spans 56..84
    let latch = Point::new(200.0, if finger { 80.0 } else { 68.0 });
    let to = Point::new(200.0, y);
    if finger {
        let f = tree.new_contact();
        tree.touch_down(f, from);
        tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
        tree.touch_move(f, latch);
        tree.touch_move(f, to);
        tree.touch_up(f, to);
    } else {
        tree.pointer_down_button(from, teksilo_core::event::PointerButton::Primary);
        tree.pointer_move(latch);
        tree.pointer_move(to);
        tree.pointer_up_button(to, teksilo_core::event::PointerButton::Primary);
    }
}

/// The drop bands widen for a finger **through a real view**, not just in the
/// pure function that computes them.
///
/// One gesture, one release point, two pointer kinds, opposite outcomes: 10 dp
/// into a 28 dp row is the middle third for a cursor, so the mouse reparents;
/// it is inside the widened leading band for a finger, so the finger inserts
/// before. Without this the whole feature is pinned only by
/// `common::drop_bands`' own unit tests, which pass just as well with the call
/// sites handing them a hardcoded `PointerKind::Mouse` — measured: they do.
#[test]
fn a_finger_and_a_mouse_read_the_same_row_offset_as_different_drop_bands() {
    let (model, view) = short_row_tree();
    let (mut tree, _id) = tree_with(view);
    drop_row_two_at(&mut tree, false, 10.0);
    assert_eq!(
        model.children(model.root(0)).len(),
        1,
        "a cursor 10 dp into a 28 dp row is in the `Into` third and must reparent",
    );

    let (model, view) = short_row_tree();
    let (mut tree, _id) = tree_with(view);
    drop_row_two_at(&mut tree, true, 10.0);
    assert_eq!(
        model.children(model.root(0)).len(),
        0,
        "a finger at the same offset must not reparent",
    );
    assert_eq!(
        model.with_item(model.root(0), |v| *v),
        Some(2),
        "…it is inside the widened `Before` band, so the row lands first",
    );
}

/// The same statement for `TreeTableView`, whose two band sites are the other
/// half of the four `common::drop_bands` reads and are a separate wiring.
#[test]
fn a_finger_and_a_mouse_read_a_tree_table_row_offset_as_different_drop_bands() {
    fn run(finger: bool) -> usize {
        let model: teksilo_data::TreeModel<usize> = teksilo_data::TreeModel::new();
        for i in 0..200 {
            model.insert_root(i, i);
        }
        let view = teksilo_widgets::TreeTableView::new(model.clone())
            .add_column(
                teksilo_widgets::table_view::Column::new("n", lit!("N"), |v: &usize, _cx| {
                    Box::new(TextWidget::new(lit!(v.to_string())))
                })
                .width(teksilo_widgets::table_view::ColumnWidth::Fixed(300.0)),
            )
            .header_height(0.0)
            .row_height(28.0)
            .reorderable(true);
        let (mut tree, _id) = tree_with(view);
        let from = Point::new(150.0, 60.0); // row 2 spans 56..84
        let to = Point::new(150.0, 10.0);
        if finger {
            let f = tree.new_contact();
            tree.touch_down(f, from);
            tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
            tree.touch_move(f, Point::new(from.x, 80.0));
            tree.touch_move(f, to);
            tree.touch_up(f, to);
        } else {
            tree.pointer_down_button(from, teksilo_core::event::PointerButton::Primary);
            tree.pointer_move(Point::new(from.x, 68.0));
            tree.pointer_move(to);
            tree.pointer_up_button(to, teksilo_core::event::PointerButton::Primary);
        }
        model.children(model.root(0)).len()
    }
    assert_eq!(
        run(false),
        1,
        "a cursor 10 dp into a 28 dp row is in the `Into` third and must reparent",
    );
    assert_eq!(
        run(true),
        0,
        "a finger at the same offset must not reparent"
    );
}

// ---------------------------------------------------------------------------
// known defect
// ---------------------------------------------------------------------------

/// **Triage: bug, not test — and one this package introduced.** A finger's
/// reorder is lost if the first sample after the hold lands outside the row it
/// pressed.
///
/// Measured on a tree of 80 dp rows, pressing row 1 (which spans 80..160) at
/// y 120 and holding past `long_press`, varying only the next sample:
///
/// | first sample | travel | leaves the row | sequence winner | reordered |
/// | --- | --- | --- | --- | --- |
/// | y 150 | 30 dp | no | the row's `DragSurface` | yes |
/// | y 158 | 38 dp | no | the row's `DragSurface` | yes |
/// | y 175 | 55 dp | yes | the view (its `PanClaim`) | no |
/// | y 65 | 55 dp | yes | the view (its `PanClaim`) | no |
///
/// 38 dp clears the 36 dp `pan_slop` as well as the 18 dp `drag_slop`, and the
/// drag still wins — so this is **not** a slop race between the two. It is the
/// press: a coarse pointer's `TapBoundary` is the pressed node's own bounds,
/// leaving them ends the press, and ending the press revokes the deferred drag
/// member the hold had just made eligible.
///
/// It matters because 18 dp of `drag_slop` is most of a 28 dp row: on the
/// default `TreeView` row a real finger's first reported sample after a hold is
/// as likely as not to be outside it, and the reorder silently becomes a scroll.
///
/// **Not reachable from this crate.** The rule is `TapBoundary::for_pointer`
/// and the revocation is the sequence's, both in `teksilo-core`; the widget
/// side cannot widen a row's bounds without changing its layout. The question
/// for whoever owns it: a deferred drag member has *already served* its hold,
/// and travelling out of the pressed node is what a drag then does — so should
/// the press ending revoke it at all, or only revoke members that have not yet
/// become eligible?
#[test]
#[ignore = "known defect: a deferred drag is revoked when its press leaves the pressed row"]
fn a_finger_reorder_survives_its_first_sample_leaving_the_row() {
    let model: teksilo_data::TreeModel<usize> = teksilo_data::TreeModel::new();
    for i in 0..200 {
        model.insert_root(i, i);
    }
    let view = teksilo_widgets::TreeView::new(model.clone(), |item, _e, _s| {
        Box::new(TextWidget::new(lit!(item.to_string())))
    })
    .item_height(80.0)
    .reorderable(true);
    let (mut tree, _id) = tree_with(view);

    let f = tree.new_contact();
    tree.touch_down(f, Point::new(200.0, 120.0)); // row 1, which spans 80..160
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    tree.touch_move(f, Point::new(200.0, 175.0)); // 55 dp: past the row's edge
    tree.touch_move(f, Point::new(200.0, 400.0));
    tree.touch_up(f, Point::new(200.0, 400.0));

    assert_ne!(
        model.with_item(model.root(1), |v| *v),
        Some(1),
        "the held contact must reorder wherever its first sample lands",
    );
}

/// A finger sweeping a marquee starts auto-scrolling further from the viewport
/// edge than a mouse does.
///
/// The band itself is `common::drag_autoscroll`'s, and its own tests pin the
/// ramp. What this pins is the *wiring*: the tick that reads it runs from a
/// frame effect with no `EventContext`, so the pointer kind has to be captured
/// onto `MarqueeState` when the drag starts — and a `MarqueeState` built with a
/// hardcoded `PointerKind::Mouse` passes every other test in this crate
/// (measured).
///
/// 48 dp in from the bottom of a 400 dp viewport is inside the coarse band (64)
/// and outside the precise one (32), so the two kinds answer differently at one
/// position.
#[test]
fn a_finger_marquee_auto_scrolls_from_further_out_than_a_mouse_marquee() {
    const NEAR_EDGE: f32 = VIEWPORT - 48.0;

    fn sweep_to_near_edge(finger: bool) -> f32 {
        let sel = SelectionModel::new(SelectionMode::Multi);
        let view = multi_grid(&sel);
        let offset = view.scroll_y_signal().clone();
        let (mut tree, _id) = tree_with(view);
        let at = Point::new(BACKGROUND_X, 200.0);
        if finger {
            let f = tree.new_contact();
            tree.touch_down(f, at);
            tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
            tree.touch_move(f, Point::new(at.x, at.y + drag_slop() + 2.0));
            tree.touch_move(f, Point::new(at.x, NEAR_EDGE));
        } else {
            tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
            tree.pointer_move(Point::new(at.x, at.y + 10.0));
            tree.pointer_move(Point::new(at.x, NEAR_EDGE));
        }
        assert_eq!(
            offset.get(),
            0.0,
            "precondition: the sweep itself scrolls nothing"
        );
        for _ in 0..4 {
            tree.advance_time(std::time::Duration::from_millis(16));
        }
        offset.get()
    }

    assert_eq!(
        sweep_to_near_edge(false),
        0.0,
        "a mouse 48 dp from the edge is outside its band and must not scroll",
    );
    assert!(
        sweep_to_near_edge(true) > 0.0,
        "a finger at the same position is inside its wider band and must scroll",
    );
}

/// One hold cannot mean two things: on a reorderable row the hold arms the
/// reorder, so the row's own `on_long_press` does not also fire.
///
/// A19's ruling is that where a row has both a reorder and a context menu the
/// reorder wins the hold, and the menu moves to an overflow affordance plus
/// Secondary / `Shift+F10` / `ShowContextMenu`. It used to fire both: the
/// deferral makes the drag member *eligible* at the hold's deadline but cancels
/// nothing, so the row's own recognizer reached its own deadline and nothing had
/// silenced it.
///
/// The rule needs no cooperation from the row, whose handler belongs to the
/// application's delegate and which no data view could gate. It is the row's
/// `DragSurface` that says so, with `LongPressRole::DragHandle` (see
/// `data_views::row_grab_surface`) — the explicit, subtree-wide form of "one
/// hold, one meaning", which `WidgetTree::long_press_is_a_grab` walks from the
/// pressed node to the root.
///
/// The framework's *implicit* form — a live sequence member whose activation was
/// put off to the long-press deadline — is deliberately keyed on that member's
/// own node and does not reach here, because the drag lives one level out on the
/// wrapper. Read across the whole sequence instead it would also have silenced
/// every control inside any deferred container, which for a finger — with no
/// secondary button — removes the only route they have to a context menu. The
/// wrapper is transparent and its subtree is exactly this row, so it is entitled
/// to the claim and makes it in the open.
///
/// What silences the row here is the **recognition** gate in
/// `WidgetTree::tick_gestures_with_ops`, which drops a recognized `LongPress`
/// whose hold `long_press_is_a_grab` says is already a grab — not
/// `arm_touch_route`, which only decides whether the *tree's* menu-or-tip route
/// is armed and never touches a widget's own handler. So the two clauses that
/// used to stand here explained neither half: the deferral is not the mechanism
/// in play (the drag lives on the wrapper, and the rule is keyed on the member's
/// own node), and `arm_touch_route`'s touch-only gate is a different question.
///
/// A mouse is unaffected because `long_press_is_a_grab` gates its role walk on a
/// direct pointer: a hold is a drag-start route only where a drag waits for one,
/// and a mouse's never does. Its twin below,
/// `a_mouse_hold_on_a_reorderable_row_still_fires_its_own_long_press`, is the
/// half that pins that, and the two must disagree — a gate wide enough to take
/// both, or narrow enough to take neither, reddens one of them.
#[test]
fn a_reorderable_rows_hold_does_not_also_fire_its_own_long_press() {
    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let model = ListModel::from_vec((0..ITEMS).collect::<Vec<usize>>());
    let fired_for_rows = fired.clone();
    let view = teksilo_widgets::ListView::new(model.clone(), move |i, _item, _s| {
        let f = fired_for_rows.clone();
        Box::new(teksilo_core::widget_builder::WidgetBuilder::on_long_press(
            TextWidget::new(lit!(i.to_string())),
            move |_e, _c| f.set(true),
        ))
    })
    .item_height(TILE_H)
    .reorderable(true);
    let (mut tree, _id) = tree_with(view);

    let from = Point::new(200.0, 60.0); // row 1
    let f = tree.new_contact();
    tree.touch_down(f, from);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    tree.touch_move(f, Point::new(from.x, from.y + drag_slop() + 2.0));
    tree.touch_move(f, Point::new(from.x, 190.0));
    tree.touch_up(f, Point::new(from.x, 190.0));

    assert_ne!(
        model.with_item(1, |v| *v),
        Some(1),
        "precondition: the held contact reordered the list",
    );
    assert!(
        !fired.get(),
        "the reorder took the hold, so the row's own long press must not fire",
    );
}

/// The other half of that rule, and the one it silently cost: a **mouse** hold
/// on the very same reorderable row must still fire the row's own long press.
///
/// `LongPressRole::DragHandle` means *the hold is this node's drag-start route*
/// — which a hold only ever is for a **direct** pointer. A mouse starts a drag
/// by moving while pressed; it never spends a hold on one, so there is nothing
/// for the declaration to be protecting and the hold stays the application's.
/// Before the gate in `WidgetTree::long_press_is_a_grab` the role's ancestor
/// walk answered for every pointer kind, so merely turning on `.reorderable(true)`
/// — or `.exportable(..)`, the same wrapper — deleted an app's `on_long_press`
/// under the mouse, where nothing was competing for the hold at all.
///
/// This is the pair that discriminates: the finger case above must still be
/// silent, this one must still fire. A gate that took both would pass one and
/// redden the other.
#[test]
fn a_mouse_hold_on_a_reorderable_row_still_fires_its_own_long_press() {
    let fired = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let model = ListModel::from_vec((0..ITEMS).collect::<Vec<usize>>());
    let fired_for_rows = fired.clone();
    let view = teksilo_widgets::ListView::new(model, move |i, _item, _s| {
        let f = fired_for_rows.clone();
        Box::new(teksilo_core::widget_builder::WidgetBuilder::on_long_press(
            TextWidget::new(lit!(i.to_string())),
            move |_e, _c| f.set(f.get() + 1),
        ))
    })
    .item_height(TILE_H)
    .reorderable(true);
    let (mut tree, _id) = tree_with(view);

    let at = Point::new(200.0, 60.0); // row 1
    tree.pointer_move(at);
    tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));

    assert_eq!(
        fired.get(),
        1,
        "a mouse spends no hold arming a drag, so the row's own long press is \
         still the application's to hear",
    );
    tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
}

/// The same for a `GridView` tile, which reaches `row_grab_surface` through its
/// own body pane rather than through a row body.
///
/// Worth its own case because the grid is the one view whose body pane *also*
/// carries a `DragSurface` (the marquee) and a press absorber, so the press it
/// hands a tile has travelled a different route than a list row's.
#[test]
fn a_mouse_hold_on_a_reorderable_grid_tile_still_fires_its_own_long_press() {
    let fired = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let fired_for_tiles = fired.clone();
    let view = GridView::new(
        ListModel::from_vec((0..ITEMS).collect::<Vec<usize>>()),
        move |tc| {
            let f = fired_for_tiles.clone();
            Box::new(teksilo_core::widget_builder::WidgetBuilder::on_long_press(
                TextWidget::new(lit!(tc.item.to_string())),
                move |_e, _c| f.set(f.get() + 1),
            ))
        },
    )
    .sizing(GridSizing::Fixed {
        width: TILE_W,
        height: TILE_H,
    })
    .reorderable(true);
    let (mut tree, _id) = tree_with(view);

    let at = Point::new(TILE_W / 2.0, TILE_H / 2.0); // tile 0
    tree.pointer_move(at);
    tree.pointer_down_button(at, teksilo_core::event::PointerButton::Primary);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));

    assert_eq!(
        fired.get(),
        1,
        "the tile's own long press survives the grab surface under a mouse",
    );
    tree.pointer_up_button(at, teksilo_core::event::PointerButton::Primary);
}

/// And the finger half of that grid pair, without which the mouse half alone
/// would pass with the whole rule deleted.
///
/// A contact's hold on a reorderable tile arms the tile's grab, so the tile's
/// own `on_long_press` must stay silent — the same ruling the list row gets, on
/// the one view whose body pane also carries a marquee `DragSurface`.
#[test]
fn a_finger_hold_on_a_reorderable_grid_tile_does_not_fire_its_own_long_press() {
    let fired = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let fired_for_tiles = fired.clone();
    let view = GridView::new(
        ListModel::from_vec((0..ITEMS).collect::<Vec<usize>>()),
        move |tc| {
            let f = fired_for_tiles.clone();
            Box::new(teksilo_core::widget_builder::WidgetBuilder::on_long_press(
                TextWidget::new(lit!(tc.item.to_string())),
                move |_e, _c| f.set(f.get() + 1),
            ))
        },
    )
    .sizing(GridSizing::Fixed {
        width: TILE_W,
        height: TILE_H,
    })
    .reorderable(true);
    let (mut tree, _id) = tree_with(view);

    let at = Point::new(TILE_W / 2.0, TILE_H / 2.0); // tile 0
    let f = tree.new_contact();
    tree.touch_down(f, at);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));

    assert_eq!(
        fired.get(),
        0,
        "the reorder took the hold, so the tile's own long press must not fire",
    );
    tree.touch_up(f, at);
}

// ---------------------------------------------------------------------------
// The drag auto-scroll band follows the device, from a tick that has no sample
// ---------------------------------------------------------------------------

/// Where the two bands disagree: inside the coarse band, outside the precise
/// one.
///
/// `common::drag_autoscroll` widens the edge band for a coarse pointer because a
/// contact patch's reported centre cannot be parked as finely as a cursor. The
/// band is read in `on_drag_tick`, which fires from `WidgetTree::layout` and so
/// has no pointer sample behind it — the drag's own pointer is what answers
/// there. This y is chosen so that only the wider band reaches it, which is what
/// makes the two assertions below discriminate rather than agree.
fn only_the_coarse_band_reaches() -> f32 {
    let precise = teksilo_widgets::common::drag_autoscroll::EDGE_BAND_PRECISE;
    let coarse = teksilo_widgets::common::drag_autoscroll::EDGE_BAND_COARSE;
    assert!(coarse > precise, "the coarse band must be the wider one");
    VIEWPORT - (precise + coarse) / 2.0
}

/// A finger's reorder auto-scrolls from inside the coarse band.
///
/// Before the drag session carried a pointer identity this was unreachable:
/// `process_drag_tick` runs from `layout()`, outside any dispatch, so
/// `ctx.pointer_kind()` in the tick handler answered `Mouse` whatever the device
/// was, and the wider band never applied to a single finger drag.
#[test]
fn a_finger_reorder_auto_scrolls_from_inside_the_coarse_band() {
    let model = ListModel::from_vec((0..ITEMS).collect());
    let view = reorderable_list(&model);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let finger = tree.new_contact();
    let from = Point::new(200.0, 60.0); // row 1
    tree.touch_down(finger, from);
    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    tree.touch_move(finger, Point::new(from.x, from.y + drag_slop() + 2.0));
    tree.touch_move(finger, Point::new(from.x, only_the_coarse_band_reaches()));

    for _ in 0..8 {
        tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    }
    assert!(
        offset.get() > 0.0,
        "a finger held inside the coarse band must auto-scroll; offset {}",
        offset.get(),
    );
    tree.touch_up(finger, Point::new(from.x, only_the_coarse_band_reaches()));
}

/// And a mouse at the very same position does not, because its band is the
/// narrower one it has always had.
///
/// The pair is the point: an assertion that a finger scrolls says nothing on its
/// own, since a band that widened for *every* pointer would pass it too — and
/// that would be a change to mouse behaviour.
#[test]
fn a_mouse_reorder_keeps_the_narrower_band_at_the_same_position() {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let model = ListModel::from_vec((0..ITEMS).collect());
    let view = reorderable_list(&model);
    let offset = view.scroll_y_signal().clone();
    let (mut tree, _id) = tree_with(view);

    let from = Point::new(200.0, 60.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(from.x, from.y + 8.0)));
    let at = only_the_coarse_band_reaches();
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(from.x, at)));

    for _ in 0..8 {
        tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    }
    assert_eq!(
        offset.get(),
        0.0,
        "a cursor at {at} is outside its own 32 dp band and must not scroll",
    );

    // …and it does scroll once it is inside that band, so the assertion above
    // is about the band and not about the mouse drag failing to arm at all.
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(
        from.x,
        VIEWPORT - 8.0,
    )));
    for _ in 0..8 {
        tree.layout(SizeProposal::exact(VIEWPORT, VIEWPORT));
    }
    assert!(
        offset.get() > 0.0,
        "the mouse drag is live and does scroll inside 32 dp; offset {}",
        offset.get(),
    );
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(from.x, VIEWPORT - 8.0),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
}

/// A tile inside a marquee-sweeping grid keeps its **own** touch context menu.
///
/// The marquee's `DragSurface` encloses the whole body pane, and its drag is
/// deferred to the long-press deadline — but that deferral is the surface's
/// business, not a claim on every tile beneath it. Read across the sequence
/// instead of per node, it silenced them all: a `Multi`-selection grid had no
/// touch route to a tile's context menu at all. A finger has no secondary
/// button, so that is the whole route, and losing it is an accessibility loss
/// rather than a trade-off.
///
/// The tiles here are not drag sources, so nothing declares
/// `LongPressRole::DragHandle` over them — which is the distinction
/// `data_views::row_grab_surface` draws and this test is the other side of.
#[test]
fn a_tile_in_a_marquee_grid_keeps_its_touch_context_menu() {
    let opened = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let counter = opened.clone();
    let sel = SelectionModel::new(SelectionMode::Multi);
    let view = multi_grid(&sel).tile_context_menu(move |_i, _pos, _ctx| {
        counter.set(counter.get() + 1);
        Some(Box::new(TextWidget::new(lit!("menu"))) as Box<dyn teksilo_core::widget::Widget>)
    });
    let (mut tree, _id) = tree_with(view);

    // On a tile, not on the background.
    let at = Point::new(80.0, 20.0);
    let f = tree.new_contact();
    tree.touch_down(f, at);
    assert_eq!(opened.get(), 0, "not on the press");

    tree.advance_input_time(hold() + std::time::Duration::from_millis(10));
    assert_eq!(
        opened.get(),
        1,
        "the hold opens the tile's menu; the marquee surface's deferral is not \
         a claim on it",
    );

    tree.touch_up(f, at);
    tree.assert_no_leaked_pointer_state();
}
