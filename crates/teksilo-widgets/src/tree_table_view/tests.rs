// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tests for [`TreeTableView`].
use super::*;
use crate::table_view::column::{CellContext, ColumnWidth};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{SortFilterTreeModel, TreeFilterMode, TreeModel};
use teksilo_i18n::lit;

fn sample_tree() -> TreeModel<&'static str> {
    let t = TreeModel::new();
    let docs = t.insert_root(0, "docs");
    t.insert_child(docs, 0, "readme");
    t.insert_child(docs, 1, "guide");
    let src = t.insert_root(1, "src");
    t.insert_child(src, 0, "main.rs");
    t
}

fn name_col() -> Column<&'static str> {
    Column::<&str>::new("name", lit!("Name"), |row, _: &CellContext| {
        Box::new(crate::primitives::TextWidget::new(lit!(*row)))
    })
    .width(ColumnWidth::Flex(1.0))
}

fn size_col() -> Column<&'static str> {
    Column::<&str>::new("size", lit!("Size"), |_row, _: &CellContext| {
        Box::new(crate::primitives::TextWidget::new(lit!("0")))
    })
    .width(ColumnWidth::Fixed(60.0))
}

#[test]
fn row_selection_click_repaints_immediately_without_expand_collapse() {
    // Regression for "row selection in TreeTableView only fires on
    // expand/collapse": before the selection_signal was observed,
    // calling `sel.select(row)` mutated the model but the rendered
    // `BodyRow.selected` flag (computed at build time from
    // `sel.is_selected(...)`) was stale until something else
    // bumped the version signal — typically a twist toggle.
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    use teksilo_data::{SelectionMode, SelectionModel};
    let proxy = SortFilterTreeModel::new(sample_tree());
    let selection = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(selection.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Selection starts empty.
    assert_eq!(selection.selected_indices().len(), 0);
    // Click on the first body row — visible at flat_idx 0
    // ("docs"), which sits below the header at y ≈ header + 0.
    let header_h = cp::HEADER_HEIGHT;
    let click_y = header_h + 10.0;
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(40.0, click_y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(40.0, click_y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    // Selection updated.
    assert_eq!(selection.selected_indices(), vec![0]);
    // And — the regression — the rendered tree must reflect the
    // new selection without us manually expanding/collapsing.
    // We trigger a layout (which renders the selection bg paint
    // path) and verify the selection IS still there: i.e., a
    // version-signal observer on `selection_signal` would have
    // fired and queued a rebuild.
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(selection.selected_indices(), vec![0]);
}

#[test]
fn first_arrow_lands_on_an_end_row_instead_of_skipping_it() {
    // `TreeTableView` plugs its own hierarchical `RowNavigator` into
    // `TableView`'s key handler, so it inherited the same bug: "no cursor
    // yet" was read as "cursor on (0, 0)", which made the first ArrowDown
    // step to flat row 1 (skipping row 0) and the first ArrowUp a DEAD KEY
    // (`prev_row(0)` is `None`). Entry now uses the navigator's own
    // first/last visible row, so it is hierarchy-aware.
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    for (key, want, what) in [
        (
            Key::ArrowDown,
            0usize,
            "first ArrowDown enters at the first visible row",
        ),
        (
            Key::ArrowUp,
            3usize,
            "first ArrowUp enters at the last visible row",
        ),
    ] {
        let t = TreeModel::new();
        t.insert_root(0, "a");
        t.insert_root(1, "b");
        t.insert_root(2, "c");
        t.insert_root(3, "d");
        let proxy = SortFilterTreeModel::new(t);
        let selection = SelectionModel::new(SelectionMode::Single);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let id = tree.add(
            TreeTableView::from_projection(proxy.clone())
                .add_column(name_col())
                .selection_mode(TableSelectionMode::SingleRow)
                .selection(selection.clone())
                .row_height(20.0),
        );
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: Some(200.0),
        });
        tree.focus(id);
        assert_eq!(proxy.visible_count(), 4, "four flat roots");
        assert!(
            selection.selected_indices().is_empty(),
            "precondition: no cursor, nothing selected"
        );

        tree.press_key(key, Modifiers::NONE);
        assert_eq!(selection.selected_indices(), vec![want], "{what}");
    }
}

#[test]
fn expanded_children_are_reachable_by_the_first_arrow() {
    // Hierarchy-aware entry: with "docs" expanded, the last VISIBLE row is a
    // child, not a root — so the first ArrowUp must land on that child. A
    // raw `row_count - 1` would happen to agree here, but going through the
    // navigator is what keeps it correct for any projection (filtered,
    // sorted, partially collapsed).
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_data::{SelectionMode, SelectionModel};

    let proxy = SortFilterTreeModel::new(sample_tree()); // docs{readme,guide}, src{main.rs}
    let selection = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(selection.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);

    let last = proxy.visible_count() - 1;
    tree.press_key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        vec![last],
        "first ArrowUp enters at the last VISIBLE row, whatever the hierarchy shows"
    );
}

#[test]
fn row_click_moves_focus_so_arrow_nav_resumes_there() {
    // Regression: in row-selection mode a row click set the selection but
    // NOT `focused_cell` (the arrow-nav origin, `unwrap_or((0,0))`), so the
    // next Arrow stepped from row 0 rather than the clicked row. Click flat
    // row 1 with ≥3 visible rows so the fall-back-to-0 bug is observable
    // (buggy: 0 → 1; fixed: 1 → 2).
    use teksilo_canvas::Point;
    use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};
    use teksilo_data::{SelectionMode, SelectionModel};
    let t = TreeModel::new();
    t.insert_root(0, "a");
    t.insert_root(1, "b");
    t.insert_root(2, "c");
    t.insert_root(3, "d");
    let proxy = SortFilterTreeModel::new(t);
    let selection = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(selection.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    assert_eq!(proxy.visible_count(), 4, "four flat roots");

    // Click flat row 1 ("b"): 20px rows starting below the header.
    let click_y = cp::HEADER_HEIGHT + 1.0 * 20.0 + 10.0;
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(40.0, click_y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(40.0, click_y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    assert_eq!(
        selection.selected_indices(),
        vec![1],
        "click selects flat row 1"
    );

    // ArrowDown must resume from the clicked row (1 → 2), not from row 0.
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        selection.selected_indices(),
        vec![2],
        "ArrowDown after a click resumes from the clicked row (1 → 2)"
    );
}

#[test]
fn role_is_treegrid() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), Role::TreeGrid);
}

#[test]
fn initial_state_shows_only_roots() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(proxy.visible_count(), 2); // docs, src
}

#[test]
fn expand_via_widget_reveals_children() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let docs = proxy.tree().root(0);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.expand(docs);
    }
    assert_eq!(proxy.visible_count(), 4); // docs, readme, guide, src
}

#[test]
fn ctrl_home_keeps_the_column_in_a_treegrid() {
    use teksilo_core::event::{Key, Modifiers};
    // The ARIA grid and treegrid patterns disagree here on purpose. A flat
    // cell grid escalates to the corner; a treegrid "moves focus to the
    // cell in the first row in the same column as the cell that had
    // focus". The shared keyboard module used to send both to the corner.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(CellSelectionModel::new(TableSelectionMode::MultiCell)),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    let read = |tree: &WidgetTree| {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.focused_cell_signal().get()
    };
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(1, 1);
    }

    tree.press_key(Key::Home, Modifiers::COMMAND);
    assert_eq!(read(&tree), Some((0, 1)), "column 1 is kept");
    tree.press_key(Key::End, Modifiers::COMMAND);
    assert_eq!(read(&tree), Some((1, 1)), "and kept going the other way");
}

#[test]
fn a_row_answers_the_expand_action_it_advertises() {
    use teksilo_core::accesskit::Action;
    // A row that sets `expanded` already advertises UIA's ExpandCollapse
    // pattern — `accesskit_consumer` derives support from the property,
    // not from the action list — so Windows sends Expand and Collapse
    // whether or not anything answers. Nothing did.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(proxy.visible_count(), 2, "roots only");

    // Find the first body row (the header shares Role::Row, so pick the
    // one that declares an expanded state).
    let row = {
        let mut found = None;
        let mut walker = vec![id];
        while let Some(w) = walker.pop() {
            // Discriminate on the advertised action itself: the header
            // shares `Role::Row` but has no Expand to offer, and a body
            // row that failed to advertise one is exactly the bug.
            let node = tree.accessibility_node(w);
            if node.role() == teksilo_core::accesskit::Role::Row
                && node.actions().contains(&Action::Expand)
            {
                found = Some(w);
                break;
            }
            for c in tree.children(w) {
                walker.push(c);
            }
        }
        found.expect("a body row advertising Action::Expand")
    };

    let mut ops = teksilo_core::window::NoopWindowOps;
    let handled = tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(row),
        Action::Expand,
        None,
        &mut ops,
    );
    assert!(handled, "the advertised Expand action must be serviced");
    // Whichever root the walk reached, it opened: the pattern is no longer
    // advertised-and-inert. (The walk is stack-ordered, so which of the
    // two roots it finds is not something to pin down here.)
    assert!(
        proxy.visible_count() > 2,
        "the row actually opened, rather than reporting success and doing nothing"
    );
}

#[test]
fn a_leaf_row_advertises_no_expand() {
    use teksilo_core::accesskit::Action;
    // `access_action` advertises as well as handles, so attaching the
    // ExpandCollapse pair to every row put the pattern on leaves — which
    // declare no `expanded` state and have nothing to open. That is the
    // advertised-and-inert bug this whole change set out to remove,
    // reappearing one level down.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Open "docs" so its two leaves are realized.
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    tree.press_key(
        teksilo_core::event::Key::ArrowRight,
        teksilo_core::event::Modifiers::NONE,
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(proxy.visible_count(), 4);

    // Four visible rows — docs, readme, guide, src — of which exactly two
    // are branches. Only those two may offer the pattern.
    let mut rows_with_expand = 0;
    let mut walker = vec![id];
    while let Some(w) = walker.pop() {
        let node = tree.accessibility_node(w);
        if node.role() == teksilo_core::accesskit::Role::Row
            && node.actions().contains(&Action::Expand)
        {
            rows_with_expand += 1;
        }
        for c in tree.children(w) {
            walker.push(c);
        }
    }
    assert_eq!(
        rows_with_expand, 2,
        "only docs and src have children; readme and guide must not \
         advertise an Expand they cannot perform"
    );
}

#[test]
fn a_tree_table_is_one_tab_stop_and_space_checks_the_focused_cell() {
    use crate::table_view::CellContext;
    use teksilo_core::event::{Key, Modifiers};

    let checked = teksilo_core::signal::Signal::new(false);
    let ck = checked.clone();
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(Column::<&str>::new(
                "done",
                lit!("Done"),
                move |_r, _: &CellContext| Box::new(crate::Checkbox::new(ck.clone())),
            ))
            .row_height(20.0),
    );
    let p = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(p);
    let stops = tree.tab_stops_within(id);
    assert_eq!(stops.len(), 1, "one Tab stop; got {}", stops.len());

    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 1); // the checkbox column
    }
    tree.press_key(Key::Space, Modifiers::NONE);
    tree.layout(p);
    assert!(checked.get(), "Space reaches the focused cell's checkbox");
}

#[test]
fn left_on_a_leaf_ascends_to_the_parent() {
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    let focus_at = |tree: &WidgetTree, row: usize| {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(row, 0);
    };
    let read = |tree: &WidgetTree| {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.focused_cell_signal().get()
    };

    focus_at(&tree, 0);
    tree.press_key(Key::ArrowRight, Modifiers::NONE); // open "docs"
    assert_eq!(proxy.visible_count(), 4);

    // "readme" is a leaf, so Left ascends rather than collapsing. Left was
    // a dead key here before — `TreeView` has ascended since it shipped.
    focus_at(&tree, 1);
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(read(&tree), Some((0, 0)), "back on docs");
    assert_eq!(proxy.visible_count(), 4, "and nothing collapsed on the way");

    // A second press does collapse, because docs is open.
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(proxy.visible_count(), 2);
}

#[test]
fn asterisk_expands_the_subtree_of_a_tree_table_row() {
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    assert_eq!(proxy.visible_count(), 2, "roots only");

    tree.press_key(Key::Character('*'), Modifiers::NONE);
    assert_eq!(proxy.visible_count(), 4, "docs and its two children");

    // `-` folds it back one level.
    tree.press_key(Key::Character('-'), Modifiers::NONE);
    assert_eq!(proxy.visible_count(), 2);
}

#[test]
fn a_shifted_arrow_does_not_expand_a_tree_table_row() {
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    tree.press_key(Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(proxy.visible_count(), 2, "Shift belongs to the selection");
}

#[test]
fn arrow_right_expands_and_left_collapses_on_tree_column() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    // ArrowRight on first row (docs, has children, collapsed) →
    // expand.
    tree.press_key(
        teksilo_core::event::Key::ArrowRight,
        teksilo_core::event::Modifiers::NONE,
    );
    assert_eq!(proxy.visible_count(), 4);
    // ArrowLeft on first row (now expanded) → collapse.
    tree.press_key(
        teksilo_core::event::Key::ArrowLeft,
        teksilo_core::event::Modifiers::NONE,
    );
    assert_eq!(proxy.visible_count(), 2);
}

/// Rows for the external-source tests: an indent-ordered stream keyed by a
/// domain id, the shape `TreeDataSlice` derives a hierarchy from.
fn slice_rows() -> Vec<teksilo_data::TreeRow<u64, &'static str>> {
    use teksilo_data::TreeRow;
    vec![
        TreeRow::new(1, "docs", 0),
        TreeRow::new(2, "readme", 1),
        TreeRow::new(3, "guide", 1),
        TreeRow::new(4, "src", 0),
    ]
}

fn external_slice() -> teksilo_data::TreeDataSlice<u64, &'static str> {
    let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
    slice.set_source(slice_rows);
    slice.reload();
    slice
}

#[test]
fn from_source_renders_an_external_tree_without_a_tree_model() {
    // The point of `from_source`: no `TreeModel` mirror anywhere. The slice
    // owns identity (`u64`), derives the hierarchy from row depths, and the
    // table reads it through the erased `TreeDataSource`.
    let slice = external_slice();
    slice.expand(&1);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(slice.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(slice.visible_count(), 4, "docs + 2 children + src");

    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert!(
        tt.projection().is_none(),
        "a source-backed view has no TreeModel projection to expose"
    );
    assert!(tt.body_pane_id.is_some(), "rows rendered from the source");
}

#[test]
fn from_source_keyed_selection_survives_a_full_resource() {
    // The property a `TreeModel` mirror cannot offer: `NodeId`s are
    // reassigned on rebuild, but a domain key is not — so a keyed selection
    // still points at the same row after the source is re-materialised.
    let slice = external_slice();
    slice.expand(&1);
    let keyed = KeyedSelectionModel::<u64>::new(teksilo_data::SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _id = tree.add(
        TreeTableView::from_source_keyed(slice.clone(), keyed.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    keyed.select(3); // "guide"
    assert!(keyed.is_selected(&3));

    // Re-source from scratch — every row is rebuilt.
    slice.reload();
    assert!(
        keyed.is_selected(&3),
        "a domain-keyed selection must survive a re-source"
    );
}

#[test]
fn from_source_supports_drag_reorder_like_the_tree_view() {
    // Parity check: a source-backed table reorders through the source's own
    // `accept_drop`, the same path `TreeView` uses — no `TreeModel`, no
    // `NodeId` anywhere. Here the slice commits the move into its own store.
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::Point;

    // The store the slice re-sources from; the reorder mutates it.
    let order: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(vec![1, 4]));
    let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
    {
        let order = order.clone();
        slice.set_source(move || {
            let names: std::collections::HashMap<u64, &'static str> =
                [(1, "docs"), (4, "src")].into_iter().collect();
            order
                .borrow()
                .iter()
                .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
                .collect()
        });
    }
    {
        let order = order.clone();
        // Domain policy: apply the move to the backing store.
        slice.set_reorder(move |dragged, target, _pos| {
            let mut o = order.borrow_mut();
            let Some(from) = o.iter().position(|k| *k == dragged) else {
                return false;
            };
            let item = o.remove(from);
            let to = o
                .iter()
                .position(|k| *k == target)
                .map_or(o.len(), |i| i + 1);
            o.insert(to, item);
            true
        });
    }
    // An external source must opt into dragging: `TreeDataSlice::drag`
    // defaults to `NoDrag` (pinned by its own `drag_default_is_nodrag`).
    slice.set_drag_policy(|_| teksilo_data::DragEligibility::CanDrag);
    slice.reload();
    assert_eq!(*order.borrow(), vec![1, 4], "docs, src");

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_source(slice.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // Drag docs (flat 0) onto the bottom third of src (flat 1) → After src.
    let h = cp::HEADER_HEIGHT;
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 38.0),
    );
    assert_eq!(
        *order.borrow(),
        vec![4, 1],
        "the source applied the reorder: src now precedes docs"
    );
}

#[test]
fn a_source_that_forbids_dragging_a_row_is_honored() {
    // The source owns drag eligibility. A view that ignored it would happily
    // move a row the store considers locked.
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::Point;

    let order: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(vec![1, 4]));
    let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
    {
        let order = order.clone();
        slice.set_source(move || {
            let names: std::collections::HashMap<u64, &'static str> =
                [(1, "docs"), (4, "src")].into_iter().collect();
            order
                .borrow()
                .iter()
                .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
                .collect()
        });
    }
    {
        let order = order.clone();
        slice.set_reorder(move |dragged, target, _pos| {
            let mut o = order.borrow_mut();
            let Some(from) = o.iter().position(|k| *k == dragged) else {
                return false;
            };
            let item = o.remove(from);
            let to = o
                .iter()
                .position(|k| *k == target)
                .map_or(o.len(), |i| i + 1);
            o.insert(to, item);
            true
        });
    }
    // Row 1 ("docs") is pinned in place by the store.
    slice.set_drag_policy(|k| {
        if *k == 1 {
            teksilo_data::DragEligibility::NoDrag
        } else {
            teksilo_data::DragEligibility::CanDrag
        }
    });
    slice.reload();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_source(slice.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    let h = cp::HEADER_HEIGHT;
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 38.0),
    );
    assert_eq!(
        *order.borrow(),
        vec![1, 4],
        "a NoDrag row must not move, even onto a valid target"
    );
}

#[test]
fn drop_on_the_middle_third_reparents_into_the_target() {
    // The Into zone: dropping on a row's middle third makes the dragged node
    // that row's child, rather than a sibling before/after it.
    use teksilo_canvas::Point;
    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.collapse_all(); // roots only: docs@0, src@1
    let docs = proxy.tree().root(0);
    let src = proxy.tree().root(1);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    let h = cp::HEADER_HEIGHT;
    // Drag docs (flat 0) onto the MIDDLE third of src (flat 1, [h+20, h+40])
    // → Into src.
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 30.0),
    );
    assert_eq!(proxy.tree().root_count(), 1, "docs is no longer a root");
    assert_eq!(
        proxy.tree().parent(docs),
        Some(src),
        "docs became a child of src"
    );
}

#[test]
fn the_into_box_is_inset_and_the_insertion_line_is_indented() {
    // The twin of `TreeView`'s pair: the two drop affordances must not read
    // alike. Flush to the row, the Into box's top edge is the very pixel a
    // Before line occupies — and the drag ghost hides the vertical sides
    // that would have told them apart.
    use teksilo_canvas::{DrawCommand, Point, ShapeKind};
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};

    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.expand_all(); // docs@0 readme@1 guide@2 src@3 main.rs@4
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    let h = cp::HEADER_HEIGHT;

    // Hold a drag from "main.rs" (flat 4) — nothing is inside its subtree,
    // so every target below accepts.
    let start = Point::new(40.0, h + 90.0);
    tree.dispatch_event(WidgetEvent::pointer_down(
        start,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(52.0, start.y)));

    // Bottom third of "readme" (flat 1, depth 1) → After, at depth 1.
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(52.0, h + 38.0)));
    let frame = tree.render();
    let line_recipe = teksilo_core::styles::ListInsertionRecipe::default();
    // The insertion line is the only decoration exactly `thickness` tall
    // that spans the body — identify it by that, not by "something at x>0",
    // which any future row stripe would satisfy vacuously.
    let lines: Vec<_> = frame
        .decorations
        .iter()
        .filter(|d| (d.rect[3] - line_recipe.thickness).abs() < 0.01 && d.rect[2] > 100.0)
        .collect();
    assert_eq!(lines.len(), 1, "exactly one insertion line, got {lines:?}");
    assert!(
        lines[0].rect[0] >= line_recipe.indent_step,
        "the After line must start one indent step in for a depth-1 target, \
         got x = {} (step {})",
        lines[0].rect[0],
        line_recipe.indent_step
    );

    // Middle third of "docs" (flat 0, depth 0) → Into, a box round the row.
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(52.0, h + 10.0)));
    let frame = tree.render();
    let recipe = teksilo_core::styles::ListDropIntoRecipe::default();
    let boxes: Vec<_> = frame
        .draw_order
        .iter()
        .filter_map(|c| match c {
            DrawCommand::Shape(i) => frame.shapes.get(*i),
            _ => None,
        })
        .filter(|s| s.shape == ShapeKind::RoundedRect && s.corner_radii[0] > 0.0)
        .filter(|s| (s.screen[3] - (20.0 - recipe.inset * 2.0)).abs() < 0.01)
        .collect();
    assert!(
        !boxes.is_empty(),
        "no inset rounded box for the Into hover; shapes = {:?}",
        frame.shapes.iter().map(|s| s.screen).collect::<Vec<_>>()
    );
    // Row 0 spans [h, h + 20]. The box's top edge must sit *inside* that
    // band — on the boundary it is pixel-identical to a Before line.
    assert!(
        boxes
            .iter()
            .all(|s| (s.screen[1] - (h + recipe.inset)).abs() < 0.01),
        "the Into box must be inset from the row's top edge ({}), got {:?}",
        h,
        boxes.iter().map(|s| s.screen).collect::<Vec<_>>()
    );
    assert!(
        boxes.iter().any(|s| s.stroke_width > 0.0) && boxes.iter().any(|s| s.stroke_width == 0.0),
        "the Into box needs both a wash and an outline"
    );
}

#[test]
fn an_active_sort_suppresses_drag_reorder() {
    // With the visible order driven by a sort, a manual reorder would have no
    // visible effect — so it must be refused outright rather than silently
    // mutating the tree behind the sort.
    use teksilo_canvas::Point;
    let proxy = SortFilterTreeModel::new(sample_tree())
        .with_comparator("name", |a: &&'static str, b: &&'static str| a.cmp(b));
    proxy.collapse_all();
    let docs = proxy.tree().root(0);
    let src = proxy.tree().root(1);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_sort(Some("name"), SortDirection::Ascending);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    let h = cp::HEADER_HEIGHT;
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 38.0),
    );
    assert_eq!(
        proxy.tree().root(0),
        docs,
        "structure unchanged while sorted"
    );
    assert_eq!(
        proxy.tree().root(1),
        src,
        "structure unchanged while sorted"
    );
}

#[test]
fn an_open_cell_editor_follows_its_row_and_closes_if_the_row_vanishes() {
    // `editing_cell` is a (row, col) pair that outlives rebuilds. Without
    // reconciliation, filtering a row away above an open editor slides the
    // editor onto a different row and silently edits the wrong item.
    let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
    let all: Vec<u64> = vec![1, 2, 3];
    slice.set_source(move || {
        let names: std::collections::HashMap<u64, &'static str> =
            [(1, "one"), (2, "two"), (3, "three")].into_iter().collect();
        all.iter()
            .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
            .collect()
    });
    slice.reload();

    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(slice.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);

    // Edit row 2 ("three" sits at index 2).
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.begin_edit(2, "name");
        assert_eq!(tt.editing_cell_signal().get(), Some((2, 0)));
    }
    tree.layout(proposal); // captures the anchor

    // Drop the FIRST row: "three" is now at index 1.
    let fewer: Vec<u64> = vec![2, 3];
    slice.set_source(move || {
        let names: std::collections::HashMap<u64, &'static str> =
            [(2, "two"), (3, "three")].into_iter().collect();
        fewer
            .iter()
            .map(|k| teksilo_data::TreeRow::new(*k, names[k], 0))
            .collect()
    });
    slice.reload();
    tree.layout(proposal);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.editing_cell_signal().get(),
            Some((1, 0)),
            "the editor must follow its row to index 1, not stay on index 2"
        );
    }

    // Now delete the edited row itself: the editor must close, not move.
    let last: Vec<u64> = vec![2];
    slice.set_source(move || {
        last.iter()
            .map(|k| teksilo_data::TreeRow::new(*k, "two", 0))
            .collect()
    });
    slice.reload();
    tree.layout(proposal);
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.editing_cell_signal().get(),
        None,
        "the editor must close when its row is gone"
    );
}

#[test]
fn default_selection_mode_is_multi_row() {
    // The doc claimed `RowSingle` — a variant that does not exist. Pin the
    // real default behaviorally so prose can't drift from it again:
    // Shift+ArrowDown twice extends to 3 rows, which only MultiRow allows.
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(wide_tree(10));
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .selection(selection.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    selection.select(0);
    tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
    tree.press_key(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(
        selection.selection_signal().get().len(),
        3,
        "default mode must extend a multi-row selection"
    );
}

#[test]
fn ctrl_arrow_moves_cursor_without_touching_selection() {
    // Explorer/Finder convention (shared with `TableView` via the
    // common `keyboard::build_key_handler`): Ctrl+Arrow repositions the
    // keyboard cursor without touching selection; plain Arrow keeps its
    // existing select-follow behavior.
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(wide_tree(5));
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .selection(selection.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    selection.select(0);

    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.focused_cell_signal().get(),
            Some((1, 0)),
            "cursor advances"
        );
    }
    assert_eq!(
        selection.selected_indices(),
        vec![0],
        "Ctrl+Arrow must not touch selection"
    );

    // Plain Arrow (no Ctrl) resumes select-follow from the cursor.
    tree.press_key(Key::ArrowDown, Modifiers::NONE);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(tt.focused_cell_signal().get(), Some((2, 0)));
    }
    assert_eq!(
        selection.selected_indices(),
        vec![2],
        "plain Arrow selects the row it lands on"
    );
}

#[test]
fn ctrl_space_toggles_the_cursor_row_after_a_ctrl_arrow_move() {
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(wide_tree(5));
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .selection(selection.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    tree.press_key(Key::ArrowDown, Modifiers::CTRL);
    assert!(selection.selected_indices().is_empty());

    tree.press_key(Key::Space, Modifiers::CTRL);
    assert_eq!(
        selection.selected_indices(),
        vec![2],
        "Ctrl+Space toggles the focused row on"
    );

    tree.press_key(Key::Space, Modifiers::CTRL);
    assert!(
        selection.selected_indices().is_empty(),
        "Ctrl+Space toggles it back off"
    );
}

#[test]
fn empty_view_renders_when_the_tree_has_no_rows() {
    let proxy = SortFilterTreeModel::new(TreeModel::<&'static str>::new());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .empty_view(|| Box::new(crate::primitives::TextWidget::new(lit!("Nothing here"))))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert!(tt.empty_id.is_some(), "placeholder should be built");
    assert!(tt.body_pane_id.is_none(), "no body pane for zero rows");
}

#[test]
fn empty_view_appears_when_live_rows_drop_to_zero() {
    // The transition case: rows exist, the widget is live, then a filter
    // removes them all. The body pane must be torn down and the
    // placeholder built — constructing already-empty (the two tests below)
    // never exercises that path.
    let proxy = SortFilterTreeModel::new(sample_tree()).with_predicate("name", |t| {
        let needle = t.to_string();
        Box::new(move |r: &&'static str| r.contains(&needle))
    });
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .empty_view(|| Box::new(crate::primitives::TextWidget::new(lit!("No matches"))))
            .row_height(20.0),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert!(tt.body_pane_id.is_some(), "starts with a body pane");
        assert!(tt.empty_id.is_none(), "no placeholder while rows exist");
    }

    proxy.set_filter("name", "zzz-no-such-row");
    tree.layout(proposal);
    assert_eq!(proxy.visible_count(), 0);
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert!(
        tt.empty_id.is_some(),
        "placeholder must appear once rows drop to zero"
    );
    assert!(tt.body_pane_id.is_none(), "stale body pane must be gone");
}

#[test]
fn empty_view_renders_when_a_filter_matches_nothing() {
    // The other half of the empty state: rows exist, but none survive the
    // filter. Without this the user sees a blank pane and no explanation.
    let proxy = SortFilterTreeModel::new(sample_tree()).with_predicate("name", |t| {
        let needle = t.to_string();
        Box::new(move |r: &&'static str| r.contains(&needle))
    });
    proxy.set_filter("name", "zzz-no-such-row");
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .empty_view(|| Box::new(crate::primitives::TextWidget::new(lit!("No matches"))))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(proxy.visible_count(), 0);
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert!(tt.empty_id.is_some());
}

#[test]
fn scroll_to_row_and_ensure_row_visible_move_the_offset() {
    let proxy = SortFilterTreeModel::new(wide_tree(100));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();

    // Aligns the row to the top: row 50 × 20 px.
    tt.scroll_to_row(50);
    assert!((tt.scroll_y_signal().get() - 1000.0).abs() < 1.0);

    // Already-visible row: minimum scroll means no movement.
    let before = tt.scroll_y_signal().get();
    tt.ensure_row_visible(51);
    assert!((tt.scroll_y_signal().get() - before).abs() < f32::EPSILON);

    // Off-screen upward: scrolls back just far enough.
    tt.ensure_row_visible(10);
    assert!((tt.scroll_y_signal().get() - 200.0).abs() < 1.0);
}

#[test]
fn begin_edit_resolves_a_column_id_and_end_edit_clears() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();

    tt.begin_edit(1, "size");
    assert_eq!(tt.editing_cell_signal().get(), Some((1, 1)));
    tt.end_edit();
    assert_eq!(tt.editing_cell_signal().get(), None);

    // Unknown id is a silent no-op, not a panic or a bogus position.
    tt.begin_edit(0, "no-such-column");
    assert_eq!(tt.editing_cell_signal().get(), None);

    // An out-of-range row is refused too: without the bounds check this
    // stranded `editing_cell` on a row nothing could ever match, and only
    // an explicit `end_edit` would clear it.
    tt.begin_edit(9999, "name");
    assert_eq!(tt.editing_cell_signal().get(), None);

    // ...and a refused call must not clobber a live editor.
    tt.begin_edit(1, "size");
    tt.begin_edit(9999, "size");
    assert_eq!(tt.editing_cell_signal().get(), Some((1, 1)));
}

#[test]
fn begin_edit_resolves_before_the_view_is_mounted() {
    // Seeding a freshly constructed view with an edit target it already
    // holds is only possible on the builder — a rebuild makes a brand-new
    // view whose `editing_cell` starts `None`, and there is no post-mount
    // handle (`as_any_mut` is not overridden). `display_indices` is filled
    // by `build()`, so before the fix this resolved against an empty cache
    // and silently did nothing: the caller's edit request vanished.
    //
    // `size` is pinned Leading, so display order is [size, name] and the
    // correct answer for "name" is 1, not its declaration index 0 — which
    // is what makes this a test of `display_order()` and not of a shortcut
    // that happens to agree when nothing is pinned.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let view = TreeTableView::from_projection(proxy)
        .add_column(name_col())
        .add_column(size_col().pinned(PinnedSide::Leading))
        .row_height(20.0);

    view.begin_edit(1, "name");
    assert_eq!(view.editing_cell_signal().get(), Some((1, 1)));

    // The documented no-ops still hold with no cache to consult.
    view.end_edit();
    view.begin_edit(0, "no-such-column");
    assert_eq!(view.editing_cell_signal().get(), None);
    view.begin_edit(9999, "name");
    assert_eq!(view.editing_cell_signal().get(), None);

    // And the seed survives mounting: the target it resolved is the one
    // the body pane reads back.
    view.begin_edit(1, "name");
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(view);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let tt = tree
        .widget_as_any(id)
        .unwrap()
        .downcast_ref::<TreeTableView<&'static str>>()
        .unwrap();
    assert_eq!(tt.editing_cell_signal().get(), Some((1, 1)));
}

#[test]
fn column_imperatives_write_their_signals() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();

    tt.set_column_width("name", 123.0);
    assert_eq!(tt.column_widths_signal().get().get("name"), Some(&123.0));
    // A non-positive width removes the override rather than pinning 0 px.
    tt.set_column_width("name", 0.0);
    assert!(!tt.column_widths_signal().get().contains_key("name"));

    // Order and pinning must actually reach `display_order()`, not just sit
    // in a signal nothing reads. Columns are declared name(0), size(1).
    assert_eq!(
        tt.display_order(),
        vec![0, 1],
        "declaration order initially"
    );

    tt.set_column_order(vec!["size".into(), "name".into()]);
    assert_eq!(tt.column_order_signal().get(), vec!["size", "name"]);
    assert_eq!(
        tt.display_order(),
        vec![1, 0],
        "set_column_order must reorder the display, not only the signal"
    );

    // Pinning outranks the order list: a Leading-pinned column sorts into
    // the leading band regardless of where the order puts it.
    tt.set_column_pinning("name", PinnedSide::Leading);
    assert_eq!(
        tt.column_pinning_signal().get().get("name"),
        Some(&PinnedSide::Leading)
    );
    assert_eq!(
        tt.display_order(),
        vec![0, 1],
        "set_column_pinning must pull the pinned column back to the front"
    );
    tt.set_column_pinning("name", PinnedSide::None);
    assert!(!tt.column_pinning_signal().get().contains_key("name"));
    assert_eq!(
        tt.display_order(),
        vec![1, 0],
        "clearing the pin restores the order list's arrangement"
    );

    tt.set_sort(Some("name"), SortDirection::Ascending);
    assert!(tt.sort_signal().get().is_some());
    tt.clear_sort();
    assert_eq!(tt.sort_signal().get(), None);
}

// ── Cell state survives a column reorder/pin ───────────────────────
//
// `focused_cell`, `editing_cell`, and `CellSelectionModel` all store
// `(row, display_position)`. A drag-to-reorder or a pin toggle only
// bumps the rebuild version — without a remap, the stored display
// position would silently relabel onto whatever column now sits
// there. Pinning makes display order diverge from declaration order
// (columns are declared name(0), size(1)), so a shortcut that merely
// keeps the same index would fail these.

#[test]
fn column_pinning_remaps_focused_cell_to_follow_its_column() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 1); // focus `size`, at display position 1
        // Pinning `size` Leading swaps it ahead of `name` — display
        // order becomes [size, name]. A stale (0, 1) would now land
        // on `name`.
        tt.set_column_pinning("size", PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.focused_cell_signal().get(),
        Some((0, 0)),
        "focus must follow `size` to its new display position"
    );
}

#[test]
fn column_pinning_remaps_editing_cell_to_follow_its_column() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.begin_edit(0, "size"); // size @ display position 1
        assert_eq!(tt.editing_cell_signal().get(), Some((0, 1)));
        tt.set_column_pinning("size", PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.editing_cell_signal().get(),
        Some((0, 0)),
        "the open editor must follow `size` to its new display \
         position, not relabel onto whatever column now sits at \
         position 1"
    );
}

#[test]
fn column_pinning_remaps_cell_selection_to_follow_its_column() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let cs = CellSelectionModel::new(TableSelectionMode::MultiCell);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(cs.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    cs.select(0, 1); // select `size` at display position 1
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_column_pinning("size", PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert!(
        cs.is_selected(0, 0),
        "selection must follow `size` to its new display position"
    );
    assert!(!cs.is_selected(0, 1));
}

#[test]
fn collapsing_a_node_above_a_selected_cell_clears_stale_cell_selection() {
    // Cell selection is index-based; a `TreeDataSource`'s flattening
    // gives no per-row delta to reindex it by (unlike `TableView`'s
    // `ListModel` `DataChange`), so the honest fix on a structural
    // change is to drop the selection rather than let a stale flat
    // row index silently point at whatever node now occupies it.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let docs = proxy.tree().root(0);
    let cs = CellSelectionModel::new(TableSelectionMode::MultiCell);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(cs.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.expand(docs);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(proxy.visible_count(), 4); // docs, readme, guide, src
    cs.select(3, 0); // `src`, the last flat row
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.collapse(docs);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert_eq!(proxy.visible_count(), 2); // docs, src — `src` is now row 1
    assert_eq!(
        cs.count(),
        0,
        "a stale (row, col) surviving the collapse must be dropped, not \
         silently point at whatever node now sits at flat row 3"
    );
}

#[test]
fn content_only_update_leaves_cell_selection_untouched() {
    // A version bump that doesn't change the flat row count — an
    // in-place item edit, no expand/collapse/insert/remove — must not
    // disturb an existing cell selection.
    let model = sample_tree();
    let proxy = SortFilterTreeModel::new(model);
    let docs = proxy.tree().root(0);
    let cs = CellSelectionModel::new(TableSelectionMode::MultiCell);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(cs.clone()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    cs.select(0, 0); // `docs`
    // In-place content update — same node, same position, new label.
    proxy.tree().update(docs, "docs-renamed");
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    assert!(
        cs.is_selected(0, 0),
        "a content-only update must leave an unrelated selection alone"
    );
}

// ── AT active_descendant follows cell focus ─────────────────────────

#[test]
fn focused_cell_sets_active_descendant_to_the_cell_node() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 1);
    }
    let update = tree.sync_accessibility();
    let root_node_id = widget_id_to_node_id(id);
    let root_node = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == root_node_id)
        .map(|(_, n)| n)
        .expect("root node present in the AT tree");
    let active = root_node
        .active_descendant()
        .expect("a focused cell must set active_descendant");
    let cell_node = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == active)
        .map(|(_, n)| n)
        .expect("active_descendant must reference a node present in the TreeUpdate");
    assert_eq!(cell_node.role(), Role::Cell);
}

#[test]
fn active_descendant_clears_after_the_focused_cell_scrolls_out_of_realization() {
    let proxy = SortFilterTreeModel::new(wide_tree(1000));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(1, 0);
    }
    let root_node_id = widget_id_to_node_id(id);
    let update = tree.sync_accessibility();
    let active_before = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == root_node_id)
        .and_then(|(_, n)| n.active_descendant());
    assert!(active_before.is_some(), "row 1 is realized initially");

    // Scroll far enough that row 1 leaves the realized+buffer window.
    // Nothing clears `focused_cell` on scroll, so this exercises the
    // "stale id" hazard directly: the pre-scroll build's cell WidgetId
    // has no live AT node once the pane rebuilds without it.
    let signal = {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.scroll_y_signal().clone()
    };
    signal.set(2000.0);
    tree.request_frame();
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let update = tree.sync_accessibility();
    let active_after = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == root_node_id)
        .and_then(|(_, n)| n.active_descendant());
    assert_eq!(
        active_after, None,
        "a focused cell that scrolled out of realization must not leave \
         a stale active_descendant pointing at a destroyed node"
    );
}

/// Taking focus reveals the row the keyboard cursor sits on.
///
/// Only the rows near the viewport are realized, so a cursor placed before
/// the view is ever looked at (a restored position, a preselected row)
/// usually has no cell widget at all. Nothing then speaks for it:
/// `accessibility()` finds no entry in `cell_map`, so it nominates no
/// `active_descendant`, and a screen reader arriving here is told nothing.
/// The first arrow press steps *past* that row as well, because the cursor
/// was somewhere nobody was shown.
///
/// Asserted on the accessibility tree, since that is what the failure was
/// about: the cell has to be a node a platform can name.
#[test]
fn taking_focus_reveals_the_cursor_row() {
    let proxy = SortFilterTreeModel::new(wide_tree(1000));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    let viewport = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(viewport);

    // Place the cursor far below the viewport WITHOUT giving the view
    // focus. `set_focused_cell` never scrolls on its own: only the key
    // handler does (`table_view/keyboard.rs:445`), and no key was pressed.
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(500, 0);
    }
    tree.request_frame();
    tree.layout(viewport);

    let root_node_id = widget_id_to_node_id(id);
    // What a platform adapter is handed: the container's
    // `active_descendant`, resolved inside the same `TreeUpdate` that
    // published it.
    let nominated = |tree: &mut WidgetTree| -> Option<(Role, Option<usize>)> {
        let update = tree.sync_accessibility();
        let active = update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == root_node_id)
            .and_then(|(_, n)| n.active_descendant())?;
        update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == active)
            .map(|(_, n)| (n.role(), n.row_index()))
    };

    assert_eq!(
        nominated(&mut tree),
        None,
        "row 500 starts far outside the realized window, which is the case \
         this is about"
    );

    tree.focus(id);
    tree.layout(viewport);

    assert_eq!(
        nominated(&mut tree),
        // `row_index` is stored zero-based and the adapters add the 1 back,
        // so the cursor's row 500 reads as 501 with the header counted.
        Some((Role::Cell, Some(501))),
        "taking focus has to bring the cursor's row into the realized \
         window, or nothing in the tree can be told about it"
    );
}

/// With no cell navigated to yet, the selected row is the one revealed.
///
/// A view restored into a selection has no `focused_cell`, so reading the
/// cursor only from that signal would leave the selection off-screen and
/// unrealized: no row node carrying `selected` for AT-SPI to announce
/// either, and the next arrow press stepping from a row nobody saw. The
/// fallback order is the keyboard handler's own
/// (`table_view/keyboard.rs:134-139`).
#[test]
fn taking_focus_reveals_the_selected_row_when_no_cell_has_been_navigated_to() {
    use teksilo_data::{SelectionMode, SelectionModel};

    let proxy = SortFilterTreeModel::new(wide_tree(1000));
    let sel = SelectionModel::new(SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0)
            .selection_mode(TableSelectionMode::SingleRow)
            .selection(sel.clone()),
    );
    let viewport = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(viewport);

    sel.select(500);
    tree.request_frame();
    tree.layout(viewport);

    let selected_rows = |tree: &mut WidgetTree| -> Vec<usize> {
        tree.sync_accessibility()
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == Role::Row && n.is_selected() == Some(true))
            .filter_map(|(_, n)| n.row_index())
            .collect()
    };

    assert!(
        selected_rows(&mut tree).is_empty(),
        "row 500 starts far outside the realized window, which is the case \
         this is about"
    );

    tree.focus(id);
    tree.layout(viewport);

    assert_eq!(
        selected_rows(&mut tree),
        vec![501],
        "taking focus has to realize the selected row, or there is no node \
         carrying `selected` for a screen reader to find"
    );
}

/// The index revealed is a **visible** row, not a position in the
/// unflattened tree.
///
/// The two differ by every descendant hidden above the cursor, so a tree
/// with collapsed branches is where a confusion between them shows. Here
/// the first twenty roots keep their nine children each and show none of
/// them, which slides every row below 180 places up the flat order while
/// nothing about the tree itself moves. The gap is deliberately far wider
/// than the realized window: a reveal aimed at the unflattened position
/// would leave the cursor's row with no widget at all, which is the state
/// this whole change is about.
///
/// The reveal is fed `focused_cell`, whose row the keyboard handler clamps
/// against `TreeNavigator::row_count()` = `TreeSource::visible_count()`
/// (`tree_table_view.rs:129-131`), and it spends that index on
/// `RowMetrics`, which `place_children` sizes from the same
/// `visible_count()`. Both ends are therefore the flat order this test
/// reads through `SortFilterTreeModel::visible_node_id`.
///
/// Checked by identity rather than by arithmetic: the nominated cell has
/// to be the one holding the node the cursor was put on.
#[test]
fn the_revealed_row_is_a_visible_index_not_a_position_in_the_unflattened_tree() {
    use teksilo_core::accessibility::node_id_to_widget_id;

    let model = TreeModel::new();
    let mut to_collapse = Vec::new();
    let mut needle = None;
    for r in 0..40usize {
        let root = model.insert_root(r, "root");
        if r < 20 {
            to_collapse.push(root);
        }
        for c in 0..9usize {
            let label = if r == 30 && c == 4 { "needle" } else { "leaf" };
            let child = model.insert_child(root, c, label);
            if r == 30 && c == 4 {
                needle = Some(child);
            }
        }
    }
    let needle = needle.expect("the needle inserted");

    let proxy = SortFilterTreeModel::new(model);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    let viewport = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(viewport);
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .expand_all();
    }
    tree.layout(viewport);

    let flat_of = |node| {
        (0..proxy.visible_count())
            .find(|&i| proxy.visible_node_id(i) == Some(node))
            .expect("the node is visible")
    };
    let flat_expanded = flat_of(needle);

    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        for root in to_collapse {
            tt.collapse(root);
        }
    }
    tree.layout(viewport);

    let flat = flat_of(needle);
    assert_eq!(
        flat,
        flat_expanded - 180,
        "collapsing the first twenty roots has to move the needle up the \
         flat order without moving it in the tree, or this test proves \
         nothing"
    );

    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(flat, 0);
    }
    tree.request_frame();
    tree.layout(viewport);
    tree.focus(id);
    tree.layout(viewport);

    let root_node_id = widget_id_to_node_id(id);
    let update = tree.sync_accessibility();
    let active = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == root_node_id)
        .and_then(|(_, n)| n.active_descendant())
        .expect(
            "taking focus has to bring the cursor's row into the realized \
             window, or nothing in the tree can be told about it",
        );
    let cell = update
        .nodes
        .iter()
        .find(|(nid, _)| *nid == active)
        .map(|(_, n)| n)
        .expect("active_descendant must reference a node in the TreeUpdate");
    assert_eq!(cell.row_index(), Some(flat + 1));

    // And it is the needle's own cell: walk the nominated cell's widget
    // subtree for the label the delegate rendered.
    let mut q = vec![node_id_to_widget_id(active)];
    let mut names = Vec::new();
    while let Some(w) = q.pop() {
        if let Some(name) = tree.accessibility_node(w).name() {
            names.push(name.to_string());
        }
        for c in tree.children(w) {
            q.push(c);
        }
    }
    assert!(
        names.iter().any(|n| n == "needle"),
        "the revealed row must be the node the cursor was put on, got {names:?}"
    );
}

#[test]
fn lazy_loading_rows_render_placeholder_cells_and_request_the_window() {
    // A windowed tree source with nothing resident: every visible row
    // is `Loading`, so the pane must render placeholder cells (not
    // skip the rows — `meta()` returning `None` used to mean "off the
    // end of `start..end`" unconditionally) and the view must nudge
    // the source to load the realized window. Mirrors TableView's
    // `lazy_loading_rows_render_placeholder_cells_and_request_the_window`.
    use std::cell::RefCell;
    use std::ops::Range;
    use teksilo_data::{FlatEntry, RowState};

    struct Windowed {
        total: usize,
        requested: Rc<RefCell<Vec<Range<usize>>>>,
        version: Signal<u64>,
    }
    impl TreeDataSource for Windowed {
        type Item = &'static str;
        type Key = usize;
        fn visible_count(&self) -> usize {
            self.total
        }
        fn with_entry<R>(
            &self,
            _i: usize,
            _f: impl FnOnce(&&'static str, &FlatEntry<usize>) -> R,
        ) -> Option<R> {
            None // nothing resident yet
        }
        fn key_at(&self, i: usize) -> Option<usize> {
            (i < self.total).then_some(i)
        }
        fn flat_index_of(&self, key: &usize) -> Option<usize> {
            (*key < self.total).then_some(*key)
        }
        fn parent(&self, _key: &usize) -> Option<usize> {
            None
        }
        fn child_keys(&self, _key: &usize) -> Vec<usize> {
            vec![]
        }
        fn version_signal(&self) -> Signal<u64> {
            self.version.clone()
        }
        fn is_expanded(&self, _key: &usize) -> bool {
            false
        }
        fn set_expanded(&self, _key: &usize, _expanded: bool) {}
        fn row_state(&self, _flat_index: usize) -> RowState {
            RowState::Loading
        }
        fn request_window(&self, range: Range<usize>) {
            self.requested.borrow_mut().push(range);
        }
    }

    let requested = Rc::new(RefCell::new(Vec::new()));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(Windowed {
            total: 1000,
            requested: requested.clone(),
            version: Signal::new(0),
        })
        .add_column(name_col())
        .show_header(false)
        .row_height(30.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // The body pane is the view's first child (header suppressed).
    // 300px / 30px = 10 visible + buffer → the loading rows realize
    // as placeholder row widgets, NOT skipped.
    let body_pane = tree.children(id)[0];
    let placeholder_rows = tree.children(body_pane).len();
    assert!(
        placeholder_rows >= 10,
        "loading rows must render as placeholders, got {placeholder_rows}"
    );
    // And the source was asked to load the realized window.
    assert!(
        !requested.borrow().is_empty(),
        "request_window must be called for the visible range"
    );
}

#[test]
fn arrow_expand_collapse_follows_a_non_leading_tree_column() {
    // Regression: the key handler hardcoded `col == 0` as "the tree
    // column", so designating any other column via `.tree_column()` moved
    // the twist visually but left ArrowLeft/ArrowRight expanding nothing.
    // Here the tree column is "size", at display position 1.
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .add_column(size_col())
            .tree_column("size")
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);

    // Off the tree column: the arrows are pure cursor movement, so the
    // visible set must not change.
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 0);
    }
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        proxy.visible_count(),
        2,
        "ArrowRight off the tree column must not expand"
    );

    // On the tree column (display position 1): expand, then collapse.
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 1);
    }
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        proxy.visible_count(),
        4,
        "docs expands to reveal 2 children"
    );
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(proxy.visible_count(), 2, "docs collapses again");
}

#[test]
fn arrow_nav_scroll_follows_focused_row() {
    // 100 flat rows × 20 px in a 200 px viewport. Walking focus down
    // past the visible window must scroll to keep the focused row on
    // screen ("selection always visible"), matching TreeView / the
    // newly-fixed TableView. Regression for: TreeTableView keyboard
    // nav left scroll_y untouched.
    use teksilo_core::event::{Key, Modifiers};
    let proxy = SortFilterTreeModel::new(wide_tree(100));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    tree.focus(id);
    let read_scroll = |tree: &WidgetTree| {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .scroll_y_signal()
            .get()
    };
    let read_focus = |tree: &WidgetTree| {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .focused_cell_signal()
            .get()
    };
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(0, 0);
    }
    assert_eq!(read_scroll(&tree), 0.0, "starts at top");

    for _ in 0..20 {
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.layout(proposal);
    }
    assert_eq!(read_focus(&tree), Some((20, 0)));
    assert!(
        read_scroll(&tree) > 200.0,
        "arrow-down nav must scroll to reveal row 20, got {}",
        read_scroll(&tree)
    );

    // Ctrl+Home returns focus AND scroll to the top.
    tree.press_key(Key::Home, Modifiers::COMMAND);
    tree.layout(proposal);
    assert_eq!(read_focus(&tree), Some((0, 0)));
    assert_eq!(read_scroll(&tree), 0.0, "Ctrl+Home scrolls to top");
}

#[test]
fn type_ahead_jumps_to_matching_row() {
    use teksilo_core::event::{Key, Modifiers};
    let model = TreeModel::new();
    model.insert_root(0, "Apple");
    model.insert_root(1, "Banana");
    model.insert_root(2, "Cherry");
    model.insert_root(3, "Cranberry");
    let proxy = SortFilterTreeModel::new(model);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0)
            .type_ahead_label(|s: &&'static str| s.to_string()),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.focus(id);
    let read_focus = |tree: &WidgetTree| {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .focused_cell_signal()
            .get()
    };
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(0, 0);
    }
    tree.press_key(Key::C, Modifiers::NONE);
    assert_eq!(read_focus(&tree), Some((2, 0)), "'c' → Cherry");
    tree.press_key(Key::R, Modifiers::NONE);
    assert_eq!(read_focus(&tree), Some((3, 0)), "'cr' → Cranberry");
}

#[test]
fn ctrl_tab_escapes_the_cell_grid() {
    use crate::primitives::{TextWidget, VStack};
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_core::widget_builder::WidgetBuilder;

    let proxy = SortFilterTreeModel::new(wide_tree(5));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    let sink = tree.add(TextWidget::new(lit!("sink")).focusable(true));
    let _root = tree.add(VStack::new().child(id).child(sink));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let read_focus = |tree: &WidgetTree| {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .focused_cell_signal()
            .get()
    };
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(0, 0);
    }
    let before = read_focus(&tree);
    tree.press_key(Key::Tab, Modifiers::CTRL);
    assert_eq!(
        read_focus(&tree),
        before,
        "Ctrl+Tab must not navigate cells"
    );
    assert_eq!(
        tree.focused(),
        Some(sink),
        "Ctrl+Tab moves focus out of the tree-table"
    );
}

#[test]
fn rows_carry_role_row_with_level_indicator() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let docs = proxy.tree().root(0);
    proxy.expand(docs);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Walk the tree and count Role::Row entries.
    let mut q = vec![id];
    let mut row_count = 0;
    while let Some(n) = q.pop() {
        if tree.accessibility_node(n).role() == Role::Row {
            row_count += 1;
        }
        for c in tree.children(n) {
            q.push(c);
        }
    }
    // 1 header + 4 visible body rows (docs, readme, guide, src).
    assert!(
        row_count >= 5,
        "expected at least 5 Role::Row nodes, got {row_count}"
    );
}

#[test]
fn filter_mode_keep_ancestors_works_via_proxy() {
    let proxy = SortFilterTreeModel::new(sample_tree())
        .filter_mode(TreeFilterMode::KeepAncestors)
        .with_predicate("name", |t| {
            let needle = t.to_string();
            Box::new(move |row: &&str| row.contains(&needle))
        });
    proxy.expand_all();
    proxy.set_filter("name", "main");
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let _id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Visible: src (ancestor), main.rs (matches).
    assert_eq!(proxy.visible_count(), 2);
}

#[test]
fn collapse_all_then_expand_all_round_trips() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.expand_all();
    }
    assert_eq!(proxy.visible_count(), 5);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.collapse_all();
    }
    assert_eq!(proxy.visible_count(), 2);
}

#[test]
fn rows_report_sibling_position_and_size_among_siblings() {
    // docs (root 1/2) -> readme (child 1/2), guide (child 2/2)
    // src  (root 2/2) -> main.rs (child 1/1)
    //
    // `TreeView`'s `TreeItemWrapper` already announces
    // position_in_set/size_of_set (`list_item_a11y.rs`);
    // `TreeTableView` never wired `TreeSource::sibling_pos` into its own
    // row wrapper (`TreeRowA11y`) despite the data being one call away.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .expand_all();
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    assert_eq!(proxy.visible_count(), 5);

    // Collect all Role::Row body widgets (the header shares the role but
    // is excluded below by having no accesskit node y inside the body
    // band — simplest: sort every Role::Row by y and drop the topmost
    // one, which is always the header).
    let mut rows: Vec<WidgetId> = Vec::new();
    let mut q = vec![id];
    while let Some(n) = q.pop() {
        if tree.accessibility_node(n).role() == Role::Row {
            rows.push(n);
        }
        for c in tree.children(n) {
            q.push(c);
        }
    }
    rows.sort_by(|a, b| tree.bounds(*a).y.partial_cmp(&tree.bounds(*b).y).unwrap());
    assert_eq!(rows.len(), 6, "header + five body rows");
    let body_rows = &rows[1..];

    // `position_in_set`/`size_of_set` aren't on the summarized
    // `AccessibilityInfo` — read them off the real accesskit node via a
    // fresh `TreeUpdate`, mirroring `docking::tests::find_a11y_node`.
    let update = tree.sync_accessibility();
    let find = |wid: WidgetId| -> &teksilo_core::accesskit::Node {
        let nid = widget_id_to_node_id(wid);
        update
            .nodes
            .iter()
            .find(|(n, _)| *n == nid)
            .map(|(_, n)| n)
            .expect("row must be in the a11y tree")
    };
    let positions: Vec<usize> = body_rows
        .iter()
        .map(|&r| find(r).position_in_set().expect("position_in_set"))
        .collect();
    // The row passes ARIA's 1-based sibling position; AccessKit stores it
    // zero-based, and the Windows and AT-SPI adapters add the 1 back — so
    // "the first of two siblings" is 0 on the node and "1" to the user.
    assert_eq!(
        positions,
        vec![0, 0, 1, 1, 0],
        "docs(1st) readme(1st) guide(2nd) src(2nd) main.rs(1st)"
    );
    // No sibling *count*, deliberately. AccessKit resolves a set size by
    // walking up from an item, so the only value a flattened tree could
    // publish is one shared by every row at every depth — which is not what
    // "of 2 siblings" means. See `TreeRowA11y::accessibility`.
    for &r in body_rows {
        assert_eq!(
            find(r).size_of_set(),
            None,
            "a per-sibling count is unrepresentable and must not be faked"
        );
    }
}
#[test]
fn tree_column_cells_mirror_the_rows_level_and_other_columns_do_not() {
    // The depth is published on the row, and the row is an *ancestor* of the
    // node this view nominates as `active_descendant` — which is the one
    // place NVDA refuses to read a level from (it clears
    // `positionInfo_level` for `OutputReason.FOCUSENTERED`). So a correct
    // row level reached no screen reader until the tree column's cell
    // carried it too. See `CellA11y::with_level`.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .expand_all();
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(400.0),
    });
    assert_eq!(proxy.visible_count(), 5);

    // Every body cell, in reading order. The header carries
    // `Role::ColumnHeader`, so it never lands in here.
    let mut cells: Vec<WidgetId> = Vec::new();
    let mut q = vec![id];
    while let Some(n) = q.pop() {
        if matches!(
            tree.accessibility_node(n).role(),
            Role::Cell | Role::GridCell
        ) {
            cells.push(n);
        }
        for c in tree.children(n) {
            q.push(c);
        }
    }
    cells.sort_by(|a, b| {
        let (ba, bb) = (tree.bounds(*a), tree.bounds(*b));
        ba.y.partial_cmp(&bb.y)
            .unwrap()
            .then(ba.x.partial_cmp(&bb.x).unwrap())
    });
    assert_eq!(cells.len(), 10, "five visible rows x two columns");

    // `level` isn't on the summarized `AccessibilityInfo` — read it off the
    // real accesskit node, as the sibling-position test above does.
    let update = tree.sync_accessibility();
    let level = |wid: WidgetId| -> Option<usize> {
        let nid = widget_id_to_node_id(wid);
        update
            .nodes
            .iter()
            .find(|(n, _)| *n == nid)
            .map(|(_, n)| n)
            .expect("cell must be in the a11y tree")
            .level()
    };

    // docs(root) -> readme, guide; src(root) -> main.rs. AccessKit stores a
    // level zero-based and the Windows adapter adds the 1 back, so a root
    // row reads 0 here and "level 1" to the user.
    let name_levels: Vec<Option<usize>> = cells.iter().step_by(2).map(|&c| level(c)).collect();
    assert_eq!(
        name_levels,
        vec![Some(0), Some(1), Some(1), Some(0), Some(1)],
        "the tree column announces its row's depth"
    );

    // The size column draws no indent, so it says nothing about depth rather
    // than repeating it on every horizontal move within one row.
    let size_levels: Vec<Option<usize>> =
        cells.iter().skip(1).step_by(2).map(|&c| level(c)).collect();
    assert_eq!(size_levels, vec![None; 5], "a non-tree column carries none");
}

#[test]
fn row_count_in_a11y_includes_header() {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), Role::TreeGrid);
    // We can't read row_count from AccessibilityInfo directly,
    // but we can verify Role::TreeGrid + Role::Row count matches
    // (header + 2 body rows = 3).
    let mut q = vec![id];
    let mut rows = 0;
    while let Some(n) = q.pop() {
        if tree.accessibility_node(n).role() == Role::Row {
            rows += 1;
        }
        for c in tree.children(n) {
            q.push(c);
        }
    }
    assert_eq!(rows, 3); // header + docs + src
}

// ── RTL (right-to-left) ──────────────────────────────────────────────

/// A tree of `n` collapsed roots — enough to force a vertical scrollbar.
fn wide_tree(n: u32) -> TreeModel<&'static str> {
    let t = TreeModel::new();
    for i in 0..n {
        t.insert_root(i as usize, "node");
    }
    t
}

/// All `Role::Row` node bounds (header + body), for picking a body row.
fn row_bounds(tree: &WidgetTree, root: WidgetId) -> Vec<teksilo_canvas::Rect> {
    let mut q = vec![root];
    let mut out = Vec::new();
    while let Some(n) = q.pop() {
        if tree.accessibility_node(n).role() == Role::Row {
            out.push(tree.bounds(n));
        }
        for c in tree.children(n) {
            q.push(c);
        }
    }
    out
}

#[test]
fn rtl_swaps_tree_expand_collapse_keys() {
    use teksilo_core::environment::LayoutDirection;
    use teksilo_core::event::{Key, Modifiers};

    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    // Roots start collapsed: docs + src visible.
    assert_eq!(proxy.visible_count(), 2);

    tree.set_layout_direction(LayoutDirection::RightToLeft);
    tree.focus(table);
    {
        let any = tree.widget_as_any(table).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(0, 0);
    }

    // Under RTL the collapsed chevron points left, so ArrowLeft expands
    // (toward the children) and ArrowRight collapses.
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(
        proxy.visible_count(),
        4,
        "RTL ArrowLeft on the tree column should expand docs"
    );
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        proxy.visible_count(),
        2,
        "RTL ArrowRight on the tree column should collapse docs"
    );
}

#[test]
fn rtl_tree_band_shifts_for_left_scrollbar() {
    use teksilo_core::environment::LayoutDirection;
    // 50 roots → vertical scrollbar present. Under RTL it sits on the
    // physical left, so the body band (and its rows) shift right by
    // SCROLLBAR_THICKNESS.
    let proxy = SortFilterTreeModel::new(wide_tree(50));
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    tree.set_layout_direction(LayoutDirection::RightToLeft);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let table_bounds = tree.bounds(table);
    // Pick a body row (below the header, which sits at the top).
    let body_row = row_bounds(&tree, table)
        .into_iter()
        .filter(|r| r.y > table_bounds.y + 5.0)
        .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
        .expect("a body row");
    assert!(
        (body_row.x - SCROLLBAR_THICKNESS).abs() < 0.5,
        "RTL body row should start at SCROLLBAR_THICKNESS, got x={}",
        body_row.x
    );
    // LTR control: same table laid out left-to-right starts at 0.
    let mut tree2 = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let proxy2 = SortFilterTreeModel::new(wide_tree(50));
    let table2 = tree2.add(
        TreeTableView::from_projection(proxy2)
            .add_column(name_col())
            .row_height(20.0),
    );
    tree2.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let tb2 = tree2.bounds(table2);
    let body_row2 = row_bounds(&tree2, table2)
        .into_iter()
        .filter(|r| r.y > tb2.y + 5.0)
        .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
        .expect("a body row");
    assert!(body_row2.x.abs() < 0.5, "LTR body row x={}", body_row2.x);
}

// ── Boundary scroll chaining ─────────────────────────────────────────

/// A TreeTableView (40 root rows × 20 px in a ~120 px viewport) above a
/// filler inside an outer ScrollArea, so chaining from the inner
/// tree-table to the outer area is observable.
fn nested_tree_table_fixture(inner: OverscrollBehavior) -> (WidgetTree, Signal<f32>, Signal<f32>) {
    use crate::ScrollArea;
    use crate::primitives::{FixedSize, TextWidget, VStack};
    let model = TreeModel::new();
    for i in 0..40 {
        model.insert_root(i, "row");
    }
    let proxy = SortFilterTreeModel::new(model);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let tt = TreeTableView::from_projection(proxy)
        .add_column(name_col())
        .show_header(false)
        .row_height(20.0)
        .overscroll_behavior(inner);
    let inner_y = tt.scroll_y_signal().clone();
    let tt_id = tree.add(tt);
    let viewport = tree.add(FixedSize::new().width(220.0).height(120.0).child(tt_id));
    let filler = tree.add(
        FixedSize::new()
            .width(220.0)
            .height(300.0)
            .child(TextWidget::new(lit!(""))),
    );
    let outer_content = tree.add(VStack::new().child(viewport).child(filler));
    let outer = ScrollArea::from_id(outer_content).smooth_scrolling(false);
    let outer_y = outer.scroll_y_signal().clone();
    let _outer = tree.add(outer);
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    (tree, inner_y, outer_y)
}

#[test]
fn nested_tree_table_chains_to_outer_at_boundary() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, inner_y, outer_y) = nested_tree_table_fixture(OverscrollBehavior::Chain);
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    let inner_bottom = inner_y.get();
    assert!(
        inner_bottom > 0.0,
        "inner tree-table should scroll down; got {inner_bottom}"
    );
    // A second wheel at the boundary must chain to the outer area.
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    assert!(
        (inner_y.get() - inner_bottom).abs() < 0.01,
        "inner stays clamped at bottom"
    );
    assert!(
        outer_y.get() > 0.01,
        "outer must scroll because the inner chained the boundary"
    );
}

#[test]
fn nested_tree_table_contain_blocks_chaining() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, _inner_y, outer_y) = nested_tree_table_fixture(OverscrollBehavior::Contain);
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Pixels { x: 0.0, y: 9999.0 },
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    tree.pointer_move(Point::new(50.0, 40.0));
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        Modifiers::NONE,
    ));
    tree.layout(SizeProposal {
        width: Some(220.0),
        height: Some(150.0),
    });
    assert!(
        outer_y.get() < 0.01,
        "Contain must prevent chaining: outer stays put"
    );
}

// ── TreeBodyPane split + variable row heights ───────────────────────

fn count_role(tree: &WidgetTree, root: WidgetId, role: Role) -> usize {
    let mut walker = vec![root];
    let mut n = 0;
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == role {
            n += 1;
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    n
}

/// Collect the (y, height) bounds of the materialised `Role::Row`
/// widgets, sorted by y.
fn row_spans(tree: &WidgetTree, root: WidgetId) -> Vec<(f32, f32)> {
    let mut walker = vec![root];
    let mut spans = Vec::new();
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == Role::Row {
            let b = tree.bounds(id);
            spans.push((b.y, b.height));
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    spans
}

#[test]
fn rows_rebuild_during_scrollbar_thumb_drag() {
    // The reason `TreeBodyPane` exists — see `common::thumb_drag_test`'s
    // module docs for the invariant, and for why every virtualized view
    // asserts it through the same driver.
    let model = TreeModel::new();
    for i in 0..500 {
        model.insert_root(i, "root");
    }
    let proxy = SortFilterTreeModel::new(model);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    crate::common::thumb_drag_test::assert_body_survives_thumb_drag(
        &mut tree,
        table,
        400.0,
        200.0,
        cp::HEADER_HEIGHT,
        "TreeTableView",
        |t| {
            let mut n = 0;
            let mut walker = vec![table];
            while let Some(id) = walker.pop() {
                if t.accessibility_node(id).role() == Role::Row {
                    let b = t.bounds(id);
                    if b.y >= 0.0 && b.y < 200.0 {
                        n += 1;
                    }
                }
                for c in t.children(id) {
                    walker.push(c);
                }
            }
            n
        },
    );
}

#[test]
fn exact_row_height_fn_positions_tree_rows() {
    let heights = [60.0_f32, 20.0, 40.0];
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .show_header(false)
            .row_height_fn(move |i| heights.get(i).copied().unwrap_or(28.0)),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // Roots only: docs (60), src (20).
    let spans = row_spans(&tree, table);
    assert_eq!(spans.len(), 2);
    assert!((spans[0].0 - 0.0).abs() < 0.01 && (spans[0].1 - 60.0).abs() < 0.01);
    assert!((spans[1].0 - 60.0).abs() < 0.01 && (spans[1].1 - 20.0).abs() < 0.01);
}

#[test]
fn auto_row_height_measures_tree_cells() {
    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(
            &self,
            _proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }
    let col = Column::<&str>::new("name", lit!("Name"), |_row, _: &CellContext| {
        Box::new(FixedLeaf(50.0, 30.0))
    })
    .width(ColumnWidth::Flex(1.0));
    let proxy = SortFilterTreeModel::new(sample_tree());
    let docs = proxy.tree().root(0);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let table = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(col)
            .show_header(false)
            .auto_row_height(50.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // Rows measured to 30 from the 50 estimate.
    let spans = row_spans(&tree, table);
    assert!(
        (spans[1].0 - 30.0).abs() < 0.01,
        "row 1 should sit at measured 30, got {}",
        spans[1].0
    );

    // Expanding docs (flat 0) keeps measured heights — the
    // divergence is the toggled row, not a full reset, so the
    // expanded children appear right below the measured row 0.
    proxy.expand(docs);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    let spans = row_spans(&tree, table);
    assert_eq!(spans.len(), 4); // docs, readme, guide, src
    assert!(
        (spans[1].0 - 30.0).abs() < 0.01,
        "measured row 0 must survive the expand, got {}",
        spans[1].0
    );
}

// ── Row reorder (Stage 5) ──────────────────────────────────────────────

/// Full drag gesture: down on source, move to cross the threshold, move to
/// target, up.
fn drag(tree: &mut WidgetTree, from: teksilo_canvas::Point, to: teksilo_canvas::Point) {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    tree.dispatch_event(WidgetEvent::pointer_down(
        from,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(from.x + 10.0, from.y)));
    tree.dispatch_event(WidgetEvent::pointer_move(to));
    tree.dispatch_event(WidgetEvent::pointer_up(
        to,
        PointerButton::Primary,
        Modifiers::NONE,
    ));
}

#[test]
fn drag_reorders_roots_after() {
    use teksilo_canvas::Point;
    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.collapse_all(); // roots only: docs@0, src@1
    let docs = proxy.tree().root(0);
    let src = proxy.tree().root(1);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    let h = cp::HEADER_HEIGHT;
    // Drag docs (flat 0, [h, h+20]) onto the bottom third of src (flat 1,
    // [h+20, h+40]) → After src.
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 38.0),
    );
    assert_eq!(proxy.tree().root_count(), 2);
    assert_eq!(proxy.tree().root(0), src, "src becomes the first root");
    assert_eq!(proxy.tree().root(1), docs, "docs moves after src");
}

#[test]
fn drag_into_own_descendant_is_refused() {
    use teksilo_canvas::Point;
    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.expand_all(); // docs@0, readme@1, guide@2, src@3, main.rs@4
    let docs = proxy.tree().root(0);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    let h = cp::HEADER_HEIGHT;
    // Drag docs (flat 0) into the middle third of readme (flat 1, a child
    // of docs) → cycle → refused; tree unchanged, no panic.
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 30.0),
    );
    assert_eq!(proxy.tree().parent(docs), None, "docs stays a root");
    assert_eq!(proxy.tree().root_count(), 2);
}

#[test]
fn reorder_is_suppressed_while_sorted() {
    use teksilo_canvas::Point;
    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.collapse_all();
    let docs = proxy.tree().root(0);
    let src = proxy.tree().root(1);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .reorderable(true)
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });
    // Activate a sort: the drop gate must refuse the reorder (a manual
    // reorder is meaningless once the visible order is sort-driven).
    tree.widget_as_any(id)
        .and_then(|a| a.downcast_ref::<TreeTableView<&str>>())
        .expect("TreeTableView")
        .set_sort(Some("name"), teksilo_data::SortDirection::Ascending);
    let h = cp::HEADER_HEIGHT;
    drag(
        &mut tree,
        Point::new(40.0, h + 10.0),
        Point::new(40.0, h + 38.0),
    );
    assert_eq!(proxy.tree().root(0), docs, "docs unchanged while sorted");
    assert_eq!(proxy.tree().root(1), src, "src unchanged while sorted");
}

#[test]
fn keyed_selection_survives_collapse() {
    // Keyed (identity) selection: a node selected by NodeId stays selected
    // when its parent collapses (the row scrolls out of the projection).
    // The prune on every projection change must NOT drop a collapsed-but-
    // present node — existence is checked against the tree, not visibility.
    use teksilo_data::{KeyedSelectionModel, SelectionMode};
    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.expand_all();
    let docs = proxy.tree().root(0);
    let readme = proxy.tree().children(docs)[0];
    let keyed = KeyedSelectionModel::<NodeId>::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .selection_mode(TableSelectionMode::MultiRow)
            .keyed_selection(keyed.clone())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    keyed.select(readme);
    assert!(keyed.is_selected(&readme));

    // Collapse docs → readme leaves the visible projection, bumping the
    // version (which runs the prune). It must survive (still in the tree).
    proxy.collapse(docs);
    assert!(
        keyed.is_selected(&readme),
        "a collapsed-but-present node stays selected by identity"
    );

    // Re-expand → still selected.
    proxy.expand(docs);
    assert!(keyed.is_selected(&readme));
}

// ── Horizontal scroll ───────────────────────────────────────────────
//
// TreeTableView reuses TableView's `body::BodyRow` / `header::HeaderRow`
// / `layout::` pane machinery wholesale, so these mirror the TableView
// suite (`table_view::tests`) at reduced breadth: enough to confirm the
// shared plumbing threads through this widget's own `build()` /
// `place_children()` / `paint()` / `on_scroll` correctly, not to
// re-verify the pane math itself (already unit-tested in `layout.rs`
// and exercised end-to-end by TableView's suite).

/// Expand any AT-transparent id (the pane-band wrapper `RowBand`
/// inserts under column pinning — see `table_view::body`'s module
/// docs — never calls `set_role`, so it reads back as the
/// `AccessNodeBuilder` default `Role::Unknown`) into its own children,
/// recursively.
fn tt_flatten_through_bands(tree: &WidgetTree, ids: Vec<WidgetId>) -> Vec<WidgetId> {
    let mut out = Vec::new();
    for id in ids {
        if matches!(
            tree.accessibility_node(id).role(),
            Role::GenericContainer | Role::Unknown
        ) {
            out.extend(tt_flatten_through_bands(tree, tree.children(id)));
        } else {
            out.push(id);
        }
    }
    out
}

/// The first BODY `Role::Row` (band-flattened children include a
/// `Role::Cell`) — distinguishes it from the header, which shares
/// `Role::Row` but has only `Role::ColumnHeader` children.
fn tt_first_body_row_id(tree: &WidgetTree, root: WidgetId) -> WidgetId {
    let mut walker = vec![root];
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == Role::Row {
            let flat = tt_flatten_through_bands(tree, tree.children(id));
            if flat
                .iter()
                .any(|&c| tree.accessibility_node(c).role() == Role::Cell)
            {
                return id;
            }
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    panic!("no body Role::Row found");
}

fn tt_header_row_id(tree: &WidgetTree, root: WidgetId) -> WidgetId {
    let mut walker = vec![root];
    while let Some(id) = walker.pop() {
        if tree.accessibility_node(id).role() == Role::Row {
            let flat = tt_flatten_through_bands(tree, tree.children(id));
            if !flat.is_empty()
                && flat
                    .iter()
                    .all(|&c| tree.accessibility_node(c).role() == Role::ColumnHeader)
            {
                return id;
            }
        }
        for c in tree.children(id) {
            walker.push(c);
        }
    }
    panic!("no header Role::Row found");
}

fn tt_body_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    tt_flatten_through_bands(tree, tree.children(tt_first_body_row_id(tree, root)))
}

fn tt_header_row_cells(tree: &WidgetTree, root: WidgetId) -> Vec<WidgetId> {
    tt_flatten_through_bands(tree, tree.children(tt_header_row_id(tree, root)))
}

/// Leading `lead` (60px, pinned) + unpinned `mid` (`middle_w` px) +
/// Trailing `trail` (60px, pinned), over the default `sample_tree()`
/// (roots collapsed — 2 visible rows).
fn build_tt_pinned_scroll_table(middle_w: f32, table_w: f32) -> (WidgetTree, WidgetId) {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(
                Column::<&'static str>::new("lead", lit!("Lead"), |row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new(lit!(*row)))
                })
                .width(ColumnWidth::Fixed(60.0))
                .pinned(PinnedSide::Leading),
            )
            .add_column(
                Column::<&'static str>::new("mid", lit!("Mid"), |row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new(lit!(*row)))
                })
                .width(ColumnWidth::Fixed(middle_w)),
            )
            .add_column(
                Column::<&'static str>::new("trail", lit!("Trail"), |_row, _: &CellContext| {
                    Box::new(crate::primitives::TextWidget::new(lit!("x")))
                })
                .width(ColumnWidth::Fixed(60.0))
                .pinned(PinnedSide::Trailing),
            )
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(table_w),
        height: Some(200.0),
    });
    (tree, id)
}

/// `n` unpinned Fixed columns of `col_w` px each.
fn build_tt_wide_unpinned_table(col_w: f32, n: usize, table_w: f32) -> (WidgetTree, WidgetId) {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let mut tv = TreeTableView::from_projection(proxy);
    for i in 0..n {
        let col_id = format!("c{i}");
        tv = tv.add_column(
            Column::<&'static str>::new(
                col_id.clone(),
                lit!(col_id.clone()),
                |row, _: &CellContext| Box::new(crate::primitives::TextWidget::new(lit!(*row))),
            )
            .width(ColumnWidth::Fixed(col_w)),
        );
    }
    // Cell selection: this fixture drives the *column* cursor.
    let id = tree.add(
        tv.row_height(20.0)
            .selection_mode(TableSelectionMode::MultiCell)
            .cell_selection(CellSelectionModel::new(TableSelectionMode::MultiCell)),
    );
    tree.layout(SizeProposal {
        width: Some(table_w),
        height: Some(200.0),
    });
    (tree, id)
}

fn tt_scroll_x(tree: &WidgetTree, id: WidgetId) -> f32 {
    tree.widget_as_any(id)
        .unwrap()
        .downcast_ref::<TreeTableView<&'static str>>()
        .unwrap()
        .scroll_x_signal()
        .get()
}

fn tt_max_scroll_x(tree: &WidgetTree, id: WidgetId) -> f32 {
    tree.widget_as_any(id)
        .unwrap()
        .downcast_ref::<TreeTableView<&'static str>>()
        .unwrap()
        .max_scroll_x_signal()
        .get()
}

fn tt_set_scroll_x(tree: &WidgetTree, id: WidgetId, x: f32) {
    tree.widget_as_any(id)
        .unwrap()
        .downcast_ref::<TreeTableView<&'static str>>()
        .unwrap()
        .scroll_x_signal()
        .set(x);
}

#[test]
fn tt_scroll_x_clamps_after_the_pane_widens() {
    let (mut tree, id) = build_tt_wide_unpinned_table(200.0, 3, 300.0);
    let max = tt_max_scroll_x(&tree, id);
    assert!(max > 0.0, "columns must overflow the narrow table");
    tt_set_scroll_x(&tree, id, max);
    assert_eq!(tt_scroll_x(&tree, id), max);

    tree.layout(SizeProposal {
        width: Some(700.0),
        height: Some(200.0),
    });
    assert_eq!(tt_max_scroll_x(&tree, id), 0.0, "content now fits");
    assert_eq!(
        tt_scroll_x(&tree, id),
        0.0,
        "scroll_x must clamp down with the new (smaller) max_scroll_x"
    );
}

#[test]
fn tt_pinned_columns_keep_their_bands_under_scroll() {
    let (mut tree, id) = build_tt_pinned_scroll_table(400.0, 200.0);

    let cells0 = tt_body_row_cells(&tree, id);
    assert_eq!(cells0.len(), 3, "lead, mid, trail");
    let lead_x0 = tree.bounds(cells0[0]).x;
    let mid_x0 = tree.bounds(cells0[1]).x;
    let trail_x0 = tree.bounds(cells0[2]).x;

    // `tt_first_body_row_id` returns the `TreeRowA11y` wrapper (the
    // `Role::Row` carrier); its sole child is the `.a11y_hidden()`
    // `BodyRow`, one level further in, whose own children are the
    // pane bands.
    let tree_row_a11y = tt_first_body_row_id(&tree, id);
    let body_row = tree.children(tree_row_a11y)[0];
    let raw_bands = tree.children(body_row);
    assert_eq!(raw_bands.len(), 3, "leading + middle + trailing bands");
    assert!(!tree.widget_clips_children(raw_bands[0]));
    assert!(
        tree.widget_clips_children(raw_bands[1]),
        "the Middle band must clip"
    );
    assert!(!tree.widget_clips_children(raw_bands[2]));

    let max = tt_max_scroll_x(&tree, id);
    assert!(max > 0.0);
    tt_set_scroll_x(&tree, id, 50.0_f32.min(max));
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: Some(200.0),
    });

    let cells1 = tt_body_row_cells(&tree, id);
    assert_eq!(tree.bounds(cells1[0]).x, lead_x0, "Leading never moves");
    assert_eq!(tree.bounds(cells1[2]).x, trail_x0, "Trailing never moves");
    let mid_x1 = tree.bounds(cells1[1]).x;
    assert!(
        (mid_x1 - (mid_x0 - 50.0)).abs() < 0.5,
        "the Middle column shifts left by exactly scroll_x: got {mid_x1}, want ~{}",
        mid_x0 - 50.0
    );
}

#[test]
fn tt_header_and_body_x_offsets_agree_under_scroll() {
    let (mut tree, id) = build_tt_pinned_scroll_table(400.0, 200.0);
    tt_set_scroll_x(&tree, id, 37.0);
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: Some(200.0),
    });

    let header_cells = tt_header_row_cells(&tree, id);
    let body_cells = tt_body_row_cells(&tree, id);
    assert_eq!(header_cells.len(), body_cells.len());
    for (i, (&h, &b)) in header_cells.iter().zip(body_cells.iter()).enumerate() {
        let hx = tree.bounds(h).x;
        let bx = tree.bounds(b).x;
        assert!(
            (hx - bx).abs() < 0.01,
            "column {i}: header x {hx} must equal body x {bx}"
        );
    }
}

#[test]
fn tt_shift_wheel_scrolls_horizontally() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, ScrollDelta, WidgetEvent};
    let (mut tree, id) = build_tt_wide_unpinned_table(200.0, 4, 300.0);
    tree.pointer_move(Point::new(50.0, 60.0));
    tree.dispatch_event(WidgetEvent::scroll(
        ScrollDelta::Lines { x: 0.0, y: 3.0 },
        Modifiers::SHIFT,
    ));
    tree.layout(SizeProposal {
        width: Some(300.0),
        height: Some(200.0),
    });
    assert!(
        tt_scroll_x(&tree, id) > 0.0,
        "Shift+wheel must remap a vertical-only wheel to horizontal scroll"
    );
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.scroll_y_signal().get(),
        0.0,
        "Shift+wheel must not also scroll vertically"
    );
}

#[test]
fn tt_ensure_col_visible_follows_focus_in_both_directions() {
    use teksilo_core::event::{Key, Modifiers};
    let (mut tree, id) = build_tt_wide_unpinned_table(150.0, 5, 300.0);
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(0, 0);
    }
    assert_eq!(tt_scroll_x(&tree, id), 0.0);

    tree.press_key(Key::End, Modifiers::NONE);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(tt.focused_cell_signal().get(), Some((0, 4)));
    }
    assert!(
        tt_scroll_x(&tree, id) > 0.0,
        "ensure-column-visible must scroll right to reveal column 4"
    );

    tree.press_key(Key::Home, Modifiers::NONE);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(tt.focused_cell_signal().get(), Some((0, 0)));
    }
    assert_eq!(
        tt_scroll_x(&tree, id),
        0.0,
        "ensure-column-visible must scroll left back to 0 for column 0"
    );
}

// ── Column header drag-to-reorder ───────────────────────────────────
//
// `HeaderCell` escalates a header press into a `ColumnReorderDragData`
// drag past a 5px threshold (`table_view::header`); the drop-target
// half — hover feedback, insertion-slot math, pane classification,
// `column_order_signal`/`column_pinning_signal` writes — is
// `header::attach_header_reorder_handlers`, shared verbatim with
// `TableView` (moved there by this commit, not duplicated). These
// tests drive the mechanism end-to-end through real pointer events
// (`drag`, defined above for row reorder — the header strip is just
// another drop target) rather than the imperative
// `set_column_order`/`set_column_pinning` setters already covered
// above, and additionally confirm the tree column carries no special
// case through the shared path: its indent/twist gutter and the
// ArrowLeft/Right expand-collapse binding both re-resolve from
// `display_indices` on every rebuild, so they follow it to wherever a
// drag lands it — including into a pinned pane, same as any other
// column.

/// Column `id` at a distinct `width`, so a header/body cell's bounds
/// alone identify which column it is after a reorder.
fn reorder_col(id: &'static str, width: f32) -> Column<&'static str> {
    Column::<&'static str>::new(id, lit!(id), |row, _: &CellContext| {
        Box::new(crate::primitives::TextWidget::new(lit!(*row)))
    })
    .width(ColumnWidth::Fixed(width))
}

/// Four unpinned columns "a" (60px, the default tree column since it's
/// declared first), "b" (70px), "c" (80px), "d" (90px) — over
/// `sample_tree()` (2 visible roots, "docs" has children).
fn build_tt_reorder_table() -> (WidgetTree, WidgetId, SortFilterTreeModel<&'static str>) {
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(reorder_col("a", 60.0))
            .add_column(reorder_col("b", 70.0))
            .add_column(reorder_col("c", 80.0))
            .add_column(reorder_col("d", 90.0))
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    (tree, id, proxy)
}

/// Whether `id` or any descendant is a `TwistArrow` — the indent/twist
/// gutter `TreeBodyPane` wraps around whichever cell is currently the
/// tree column. Identified by `widget_type_name` (a plain `type_name`
/// readout, no opt-in needed) rather than `widget_as_any` downcast,
/// since `TwistArrow` — a layout-only primitive nobody has needed to
/// downcast before — doesn't override `Widget::as_any`.
fn tt_subtree_has_twist_arrow(tree: &WidgetTree, id: WidgetId) -> bool {
    if tree.widget_type_name(id) == Some("teksilo_widgets::primitives::twist_arrow::TwistArrow") {
        return true;
    }
    tree.children(id)
        .into_iter()
        .any(|c| tt_subtree_has_twist_arrow(tree, c))
}

#[test]
fn header_drag_reorders_column_before_an_earlier_sibling() {
    // Drag "d" (display 3) to a slot strictly inside the unpinned band
    // (before "b") — a plain reorder with no pane-boundary side effect.
    let (mut tree, id, _proxy) = build_tt_reorder_table();
    let header = tt_header_row_cells(&tree, id);
    assert_eq!(header.len(), 4);
    let from = tree.bounds(header[3]).center(); // "d"
    let to = teksilo_canvas::Point::new(65.0, from.y); // inside "b"'s leading half
    drag(&mut tree, from, to);

    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.column_order_signal().get(),
            vec![
                "a".to_string(),
                "d".to_string(),
                "b".to_string(),
                "c".to_string()
            ],
            "dropping \"d\" before \"b\" must write [a, d, b, c]"
        );
        assert_eq!(
            tt.column_pinning_signal().get().get("d"),
            None,
            "a mid-band drop must not pin the moved column"
        );
    }

    // display_indices re-derive: a fresh layout must actually reflow
    // the header cells into the new order (Fixed widths, so an exact
    // width sequence identifies each column unambiguously).
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let after = tt_header_row_cells(&tree, id);
    let widths: Vec<f32> = after.iter().map(|&c| tree.bounds(c).width).collect();
    assert!(
        widths
            .iter()
            .zip([60.0, 90.0, 70.0, 80.0])
            .all(|(&w, want)| (w - want).abs() < 0.5),
        "header cells must reflow to widths [60, 90, 70, 80], got {widths:?}"
    );
}

#[test]
fn header_drag_to_the_leading_edge_pins_only_into_an_existing_leading_pane() {
    // The pane classification in `attach_header_reorder_handlers` is the
    // exact same code TableView's header shares. A pane exists only while
    // a column is pinned to it: with nothing pinned, a drop at the very
    // leading edge is a plain move to the first slot — it used to pin the
    // column Leading, into a pane the user never saw. Once a leading pane
    // exists, the same drop lands inside it and pins.
    let (mut tree, id, _proxy) = build_tt_reorder_table();
    let header = tt_header_row_cells(&tree, id);
    let from = tree.bounds(header[3]).center(); // "d"
    let to = teksilo_canvas::Point::new(5.0, from.y); // before "a"
    drag(&mut tree, from, to);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.column_order_signal().get(),
            vec![
                "d".to_string(),
                "a".to_string(),
                "b".to_string(),
                "c".to_string()
            ],
        );
        assert_eq!(
            tt.column_pinning_signal().get().get("d").copied(),
            None,
            "with no leading pane a drop at the leading edge must not pin"
        );
        // Now there *is* a leading pane.
        tt.set_column_pinning("d", PinnedSide::Leading);
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    let header = tt_header_row_cells(&tree, id);
    let from = tree.bounds(header[2]).center(); // "b"
    drag(&mut tree, from, to);

    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.column_pinning_signal().get().get("b").copied(),
        Some(PinnedSide::Leading),
        "dropping inside an existing leading pane pins the column"
    );
    assert_eq!(
        tt.column_order_signal().get(),
        vec![
            "b".to_string(),
            "d".to_string(),
            "a".to_string(),
            "c".to_string()
        ],
    );
}

#[test]
fn header_drag_reorder_remaps_focused_and_editing_cell_to_follow_their_columns() {
    // `focused_cell` / `editing_cell` store `(row, display_position)` —
    // `imperative::remap_cell_state` (already exercised by the
    // `column_pinning_remaps_*` tests above via the imperative setters)
    // must fire the same way when the reorder arrives through a real
    // header drag.
    let (mut tree, id, _proxy) = build_tt_reorder_table();
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.set_focused_cell(0, 1); // "b"
        tt.begin_edit(0, "d"); // "d"
    }

    let header = tt_header_row_cells(&tree, id);
    let from = tree.bounds(header[3]).center(); // "d"
    let to = teksilo_canvas::Point::new(65.0, from.y); // before "b" — see the plain-reorder test above
    drag(&mut tree, from, to);
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.column_order_signal().get(),
        vec![
            "a".to_string(),
            "d".to_string(),
            "b".to_string(),
            "c".to_string()
        ],
    );
    assert_eq!(
        tt.focused_cell_signal().get(),
        Some((0, 2)),
        "focus must follow \"b\" to its new display position"
    );
    assert_eq!(
        tt.editing_cell_signal().get(),
        Some((0, 1)),
        "the open editor must follow \"d\" to its new display position"
    );
}

#[test]
fn header_drag_moves_the_tree_column_and_twist_follows() {
    // The tree column carries no special case anywhere in the reorder
    // path: `is_tree_column` in `TreeBodyPane::build` is a plain
    // `display_pos == tree_display_pos` comparison, and
    // `tree_display_pos` is re-resolved from `display_indices` on
    // every rebuild (see the comment on `TreeTableView::build`'s
    // `key_cfg.tree_column_display_pos`). So dragging "a" (the tree
    // column) to a later, unpinned slot must carry the indent/twist
    // gutter with it, and ArrowLeft/Right must stay bound to it there.
    let (mut tree, id, proxy) = build_tt_reorder_table();
    let header = tt_header_row_cells(&tree, id);
    let from = tree.bounds(header[0]).center(); // "a", the tree column
    let to = teksilo_canvas::Point::new(220.0, from.y); // lands "a" between "c" and "d"
    drag(&mut tree, from, to);

    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        assert_eq!(
            tt.column_order_signal().get(),
            vec![
                "b".to_string(),
                "c".to_string(),
                "a".to_string(),
                "d".to_string()
            ],
        );
        assert_eq!(
            tt.column_pinning_signal().get().get("a"),
            None,
            "a mid-band drop must not pin the tree column either"
        );
    }
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let body = tt_body_row_cells(&tree, id);
    assert_eq!(body.len(), 4);
    assert!(
        !tt_subtree_has_twist_arrow(&tree, body[0]),
        "\"b\" is no longer the tree column"
    );
    assert!(
        !tt_subtree_has_twist_arrow(&tree, body[1]),
        "\"c\" is no longer the tree column"
    );
    assert!(
        tt_subtree_has_twist_arrow(&tree, body[2]),
        "the twist must follow \"a\" to its new display position"
    );
    assert!(
        !tt_subtree_has_twist_arrow(&tree, body[3]),
        "\"d\" is not the tree column"
    );

    // ArrowLeft/Right stay bound to the tree column at its new slot.
    use teksilo_core::event::{Key, Modifiers};
    tree.focus(id);
    {
        let any = tree.widget_as_any(id).unwrap();
        any.downcast_ref::<TreeTableView<&'static str>>()
            .unwrap()
            .set_focused_cell(0, 2); // row 0 ("docs"), tree column's new slot
    }
    tree.press_key(Key::ArrowRight, Modifiers::NONE);
    assert_eq!(
        proxy.visible_count(),
        4,
        "ArrowRight on the relocated tree column must expand \"docs\""
    );
    tree.press_key(Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(proxy.visible_count(), 2, "and ArrowLeft collapses it again");
}

#[test]
fn header_drag_from_a_different_table_is_rejected() {
    // Each TreeTableView mints its own `table_id`; a drop whose
    // `ColumnReorderDragData::source_table_id` doesn't match the
    // hovered header's own id must be a no-op — otherwise dragging a
    // column between two independent tree-tables on screen would
    // silently reorder the wrong one.
    use crate::primitives::{FixedSize, HStack};
    let proxy1 = SortFilterTreeModel::new(sample_tree());
    let proxy2 = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());

    let tt1 = TreeTableView::from_projection(proxy1)
        .add_column(reorder_col("x", 100.0))
        .add_column(reorder_col("y", 100.0))
        .row_height(20.0);
    let order1 = tt1.column_order_signal().clone();
    let id1 = tree.add(tt1);
    let tt2 = TreeTableView::from_projection(proxy2)
        .add_column(reorder_col("x", 100.0))
        .add_column(reorder_col("y", 100.0))
        .row_height(20.0);
    let order2 = tt2.column_order_signal().clone();
    let id2 = tree.add(tt2);

    let fixed1 = tree.add(FixedSize::new().width(200.0).height(150.0).child(id1));
    let fixed2 = tree.add(FixedSize::new().width(200.0).height(150.0).child(id2));
    tree.add(HStack::new().child(fixed1).child(fixed2));
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(150.0),
    });

    // tt1 occupies window x[0, 200), tt2 x[200, 400) — drag tt1's
    // leading header cell into tt2's header strip.
    let from = tree.bounds(tt_header_row_cells(&tree, id1)[0]).center();
    let to = teksilo_canvas::Point::new(250.0, from.y); // inside tt2's "x" cell
    drag(&mut tree, from, to);

    assert!(order1.get().is_empty(), "tt1's own order must be untouched");
    assert!(
        order2.get().is_empty(),
        "tt2 must reject a drop whose payload names a different table_id"
    );
}

#[test]
fn header_drag_released_over_the_body_does_not_trigger_foreign_row_drop() {
    // Regression: `on_foreign_drop` fires for "any payload NOT
    // recognized as this view's own row drag" — without the
    // `ColumnReorderDragData` bail at the top of the row-level
    // `on_drag_hover`/`on_drop` (added alongside wiring up header
    // reorder — TreeTableView never carried a `ColumnReorderDragData`
    // payload before), a header drag released past the header strip's
    // own y-range would fall through into this hatch, or into a
    // row-insertion-line hover affordance, for a drag the header is
    // already handling.
    use std::cell::Cell;
    let foreign_fired = Rc::new(Cell::new(false));
    let flag = foreign_fired.clone();
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .on_foreign_drop(move |_payload, _node, _pos, _ctx| {
                flag.set(true);
                true
            })
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let header = tt_header_row_cells(&tree, id);
    let from = tree.bounds(header[0]).center();
    let to = teksilo_canvas::Point::new(from.x, cp::HEADER_HEIGHT + 10.0); // below the header
    drag(&mut tree, from, to);

    assert!(
        !foreign_fired.get(),
        "a column-reorder drag must never reach on_foreign_drop"
    );
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert!(
        tt.column_order_signal().get().is_empty(),
        "no header drop occurred either — the release point was outside the header strip"
    );
}

#[test]
fn header_drag_insertion_is_scroll_aware() {
    // The insertion-slot math (`layout::insertion_slot_at_x`) is unit
    // tested directly for scroll-awareness; this proves the SHARED
    // drop-target wiring actually reaches it under a nonzero
    // `scroll_x`, for TreeTableView same as TableView.
    let (mut tree, id) = build_tt_wide_unpinned_table(100.0, 4, 200.0);
    let max = tt_max_scroll_x(&tree, id);
    assert!(max > 0.0, "4×100px columns must overflow a 200px viewport");
    tt_set_scroll_x(&tree, id, max); // scrolled fully right
    tree.layout(SizeProposal {
        width: Some(200.0),
        height: Some(200.0),
    });

    // At full scroll the 200px viewport shows logical [200, 400): "c2"
    // fills local [0, 100), "c3" fills local [100, 200). Dropping "c3"
    // at local x=10 (deep in "c2"'s own zone) must resolve against the
    // scrolled position and land before "c2" — an unscrolled read of
    // the same raw x=10 would instead land before "c0".
    let header = tt_header_row_cells(&tree, id);
    let from = tree.bounds(header[3]).center(); // "c3"
    let to = teksilo_canvas::Point::new(10.0, from.y);
    drag(&mut tree, from, to);

    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(
        tt.column_order_signal().get(),
        vec![
            "c0".to_string(),
            "c1".to_string(),
            "c3".to_string(),
            "c2".to_string()
        ],
        "\"c3\" must land before \"c2\" (scroll-aware), not before \"c0\""
    );
}

// ── Column resize grip (parity with TableView) ─────────────────────────
//
// The grip machinery lives in the shared `table_view::header::HeaderCell`,
// but `TreeTableView` fills its own `HeaderCellSpec` and owns its own
// `resize_state` / `resize_target` / `resize_preview_x` handles — so the
// wiring is asserted here too rather than assumed from the TableView side.

fn tt_resize_table() -> (WidgetTree, WidgetId) {
    // `name` Flex(1) then `size` Fixed(60) at a 400 px viewport: `name`
    // spans [0, 340], `size` spans [340, 400], divider at x = 340.
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    (tree, id)
}

fn tt_overrides(tree: &WidgetTree, id: WidgetId) -> std::collections::HashMap<String, f32> {
    let any = tree.widget_as_any(id).unwrap();
    any.downcast_ref::<TreeTableView<&'static str>>()
        .unwrap()
        .column_widths_signal()
        .get()
}

#[test]
fn tt_grip_reaches_into_the_next_column() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let (mut tree, id) = tt_resize_table();
    let y = cp::HEADER_HEIGHT * 0.5;
    // One pixel PAST the name/size divider, i.e. inside `size`.
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(341.0, y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(311.0, y)));
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(311.0, y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    let w = tt_overrides(&tree, id);
    assert!(
        (w.get("name").copied().unwrap_or(0.0) - 310.0).abs() < 0.5,
        "dragging the divider left from `size` must shrink `name` from 340 \
         to 310; got {w:?}"
    );
}

#[test]
fn tt_resize_keeps_preceding_flex_column_and_tracks_the_pointer() {
    // `name` Flex(1) | `size` Fixed(60) | `kind` Flex(1) at 400 px: name
    // and kind get 170 each, size sits at [170, 230]. Drag the size|kind
    // divider right by 30: `size` grows to 90, `kind` absorbs it, and
    // `name` — the flex column *before* the grip — must stay at 170. The
    // header is shared with `TableView`, but the resize table that carries
    // each column's flex flag is filled here, so assert it from this side.
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let kind_col = Column::<&str>::new("kind", lit!("Kind"), |_row, _: &CellContext| {
        Box::new(crate::primitives::TextWidget::new(lit!("k")))
    })
    .width(ColumnWidth::Flex(1.0));
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(name_col())
            .add_column(size_col())
            .add_column(kind_col)
            .row_height(20.0)
            .show_internal_scrollbars(false),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    let y = cp::HEADER_HEIGHT * 0.5;
    let down_x = 230.0 - cp::RESIZE_HANDLE_WIDTH * 0.5;
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(down_x, y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(down_x + 30.0, y)));
    tree.layout(proposal);
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(down_x + 30.0, y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    let w = tt_overrides(&tree, id);
    assert!(
        (w.get("size").copied().unwrap_or(0.0) - 90.0).abs() < 0.5,
        "size grows by the pointer travel; got {w:?}"
    );
    assert!(
        (w.get("name").copied().unwrap_or(0.0) - 170.0).abs() < 0.5,
        "name, ahead of the grip, is frozen where it was; got {w:?}"
    );
    assert!(
        !w.contains_key("kind"),
        "kind absorbs the change; got {w:?}"
    );
}

#[test]
fn tt_stretch_last_column_fills_the_gap_and_reflows_on_a_resize() {
    // `size` Fixed(60) | `kind` Fixed(100) at 400 px would leave a 240 px
    // gap; with `stretch_last_column` `kind` spans [60, 400]. Widening
    // `size` by 40 then hands `kind` 40 px less.
    use teksilo_canvas::Point;
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    let kind_col = Column::<&str>::new("kind", lit!("Kind"), |_row, _: &CellContext| {
        Box::new(crate::primitives::TextWidget::new(lit!("k")))
    })
    .width(ColumnWidth::Fixed(100.0));
    let proxy = SortFilterTreeModel::new(sample_tree());
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_projection(proxy)
            .add_column(size_col())
            .add_column(kind_col)
            .row_height(20.0)
            .stretch_last_column(true)
            .show_internal_scrollbars(false),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    fn header_widths(tree: &WidgetTree, root: WidgetId) -> Vec<f32> {
        fn walk(tree: &WidgetTree, id: WidgetId, out: &mut Vec<WidgetId>) {
            if tree
                .widget_type_name(id)
                .is_some_and(|n| n.ends_with("HeaderCell"))
            {
                out.push(id);
                return;
            }
            for k in tree.children(id) {
                walk(tree, k, out);
            }
        }
        let mut cells = Vec::new();
        walk(tree, root, &mut cells);
        cells.sort_by(|x, y| tree.bounds(*x).x.total_cmp(&tree.bounds(*y).x));
        cells.iter().map(|c| tree.bounds(*c).width).collect()
    }
    assert_eq!(header_widths(&tree, id), vec![60.0, 340.0]);

    let y = cp::HEADER_HEIGHT * 0.5;
    let down_x = 60.0 - cp::RESIZE_HANDLE_WIDTH * 0.5;
    tree.dispatch_event(WidgetEvent::pointer_down(
        Point::new(down_x, y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.dispatch_event(WidgetEvent::pointer_move(Point::new(down_x + 40.0, y)));
    tree.dispatch_event(WidgetEvent::pointer_up(
        Point::new(down_x + 40.0, y),
        PointerButton::Primary,
        Modifiers::NONE,
    ));
    tree.layout(proposal);
    assert_eq!(header_widths(&tree, id), vec![100.0, 300.0]);
    let w = tt_overrides(&tree, id);
    assert!(
        !w.contains_key("kind"),
        "the stretched column carries no override of its own; got {w:?}"
    );
}

#[test]
fn tt_header_strip_paints_column_separators() {
    let (mut tree, _id) = tt_resize_table();
    let frame = tree.render();
    let found = frame.decorations.iter().any(|d| {
        let [x, y, w, h] = d.rect;
        (x - 339.0).abs() < 0.6 && w <= 1.5 && y.abs() < 0.6 && (h - cp::HEADER_HEIGHT).abs() < 0.6
    });
    assert!(
        found,
        "expected a header separator at the name/size divider (x≈339); \
         decorations={:?}",
        frame.decorations.iter().map(|d| d.rect).collect::<Vec<_>>()
    );
}

#[test]
fn tree_column_chrome_is_clipped_to_its_column() {
    // The indent gutter and the twist chevron are rigid: a tree column
    // dragged narrower than `depth * indent + twist + gap` cannot shrink
    // to fit, and without a clip the chevron — and the whole label after
    // it — draws on top of the next column. Clipping the chrome wrapper
    // is what lets the grip shrink the tree column all the way to its
    // floor without the row bleeding sideways.
    let (tree, id) = tt_resize_table();
    // Find the first body cell of the tree column (column index 1 in the
    // 1-based AccessKit numbering) and check its chrome wrapper clips.
    let mut walker = vec![id];
    let mut checked = false;
    while let Some(node) = walker.pop() {
        if tree.accessibility_node(node).role() == Role::Cell {
            let kids = tree.children(node);
            if let Some(&wrapper) = kids.first()
                && tree.widget_clips_children(wrapper)
            {
                checked = true;
                break;
            }
        }
        for c in tree.children(node) {
            walker.push(c);
        }
    }
    assert!(
        checked,
        "the tree column's indent + twist wrapper must clip its children"
    );
}

/// An editable column whose delegate swaps in a real `TextInput`, so a test
/// can ask where the keyboard actually went.
fn editable_name_col() -> Column<&'static str> {
    Column::<&str>::new("name", lit!("Name"), |row, cx: &CellContext| {
        if cx.is_editing {
            Box::new(crate::text_input::TextInput::new(Signal::new(
                (*row).to_string(),
            )))
        } else {
            Box::new(crate::primitives::TextWidget::new(lit!(*row)))
        }
    })
    .width(ColumnWidth::Flex(1.0))
    .editable(true)
}

fn three_row_slice() -> teksilo_data::TreeDataSlice<u64, &'static str> {
    let slice = teksilo_data::TreeDataSlice::<u64, &'static str>::new();
    slice.set_source(move || {
        [(1_u64, "one"), (2, "two"), (3, "three")]
            .into_iter()
            .map(|(k, n)| teksilo_data::TreeRow::new(k, n, 0))
            .collect()
    });
    slice.reload();
    slice
}

/// Two primary clicks at one point, close enough together to read as a
/// double-click. `WidgetTree::click` twice would be two separate taps.
fn double_click_at(tree: &mut WidgetTree, at: Point) {
    use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
    for _ in 0..2 {
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
    }
}

/// **An open cell editor holds the keyboard.**
///
/// `TableView`'s body pane has always focused into the editing cell; the
/// line was left behind when the tree table was split out of it, so
/// `TreeTableView`'s inline editing was reachable only with the mouse. With
/// focus still on the table, every keystroke went to the table's own key
/// handler instead: Escape cancelled nothing, Enter activated the row, and
/// typing ran type-ahead over the value being edited.
#[test]
fn opening_a_cell_editor_moves_the_keyboard_into_it() {
    let slice = three_row_slice();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(slice)
            .add_column(editable_name_col())
            .row_height(20.0),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    tree.focus(id);

    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.begin_edit(1, "name");
    }
    tree.layout(proposal);

    let focused = tree.focused().expect("something must hold focus");
    assert_ne!(
        focused, id,
        "focus is still on the table, not in the editor"
    );
    let cell = {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.realized_cell(1, 0).expect("the edited cell is realized")
    };
    assert!(
        tree.is_descendant_of(focused, cell),
        "focus must land inside the edited cell, not on {:?}",
        tree.widget_type_name(focused)
    );
}

/// ...and it still holds it after the pane rebuilds under it.
///
/// A table rebuilds its rows constantly — selection, filtering, scroll, the
/// edit signal itself — and each rebuild destroys and re-creates every cell
/// widget, the open editor included. Restoring focus is therefore not a
/// one-shot at edit-open: without it the first click on another row would
/// silently deafen the editor the writer is still typing into. Driven
/// through a selection change because that is the rebuild a click produces.
#[test]
fn an_open_editor_still_holds_the_keyboard_after_the_pane_rebuilds() {
    let slice = three_row_slice();
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(slice)
            .selection(selection.clone())
            .add_column(editable_name_col())
            .row_height(20.0),
    );
    let proposal = SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    };
    tree.layout(proposal);
    {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.begin_edit(1, "name");
    }
    tree.layout(proposal);
    tree.focused().expect("the editor took focus");

    selection.select(2);
    tree.layout(proposal);

    let focused = tree.focused().expect("focus survived the rebuild");
    assert_ne!(focused, id, "the rebuild dropped focus back onto the table");
    let cell = {
        let any = tree.widget_as_any(id).unwrap();
        let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
        tt.realized_cell(1, 0)
            .expect("the edited cell is still realized")
    };
    assert!(
        tree.is_descendant_of(focused, cell),
        "focus must still be inside the edited cell, not on {:?}",
        tree.widget_type_name(focused)
    );
}

/// **Double-click opens the editor on an editable cell** — one arm of
/// [`EditTriggers`], and one that had no implementation anywhere.
/// `F2 | ANY_KEY | DOUBLE_CLICK` is the default set, so every table has
/// been promising this; only `keyboard.rs`'s F2 and type-to-edit ever
/// reached `on_cell_edit_request`.
#[test]
fn a_double_click_on_an_editable_cell_opens_its_editor() {
    let (mut tree, id, seen, _) = click_probe(EditTriggers::DOUBLE_CLICK);
    let cell = realized(&tree, id, 1, 0);
    let at = tree.bounds(cell).center();
    double_click_at(&mut tree, at);

    assert_eq!(
        seen.borrow().as_slice(),
        &[(1, "name".to_string())],
        "a double-click on an editable cell must request its editor"
    );
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    assert_eq!(tt.editing_cell_signal().get(), Some((1, 0)));
}

/// **One click opens it** when the column asks for `SINGLE_CLICK` — the
/// case the old closed enum could not express at all.
#[test]
fn a_single_click_opens_the_editor_when_the_column_asks_for_it() {
    let (mut tree, id, seen, _) = click_probe(EditTriggers::SINGLE_CLICK);
    let cell = realized(&tree, id, 1, 0);
    tree.click(cell);

    assert_eq!(
        seen.borrow().as_slice(),
        &[(1, "name".to_string())],
        "one click on a SINGLE_CLICK column must request its editor"
    );
}

/// ...and a column that asked for neither is not opened by any click.
/// `NONE` has to mean none, or "read-only in practice" would be
/// unexpressible for an otherwise editable column.
#[test]
fn a_click_opens_nothing_when_the_column_asks_for_no_click_trigger() {
    let (mut tree, id, seen, _) = click_probe(EditTriggers::F2);
    let cell = realized(&tree, id, 1, 0);
    tree.click(cell);
    let at = tree.bounds(cell).center();
    double_click_at(&mut tree, at);

    assert!(
        seen.borrow().is_empty(),
        "an F2-only column opened an editor from a click: {:?}",
        seen.borrow()
    );
}

/// A double-click that opens an editor does **not** also activate the row.
///
/// The collision this rules out is opening the item *and* starting to edit
/// it on one gesture, which is why the click arm could not simply be
/// switched on. The framework settles it with no guard in the pane: the
/// cell's gesture arena answers `Handled` to the press, so the bubble never
/// reaches the row.
///
/// One gesture per tree, and the read-only baseline is the **separate**
/// test below: a second synthetic double-click in the same tree never
/// reaches the row's `on_double_tap` at all (the recognizer reads clicks 3
/// and 4 as a continuing run), so a single test doing both would pass with
/// the behaviour removed — an earlier draft did, which is why this note
/// exists.
#[test]
fn editing_a_cell_by_double_click_does_not_also_activate_the_row() {
    let (mut tree, id, _, activated) = click_probe(EditTriggers::DOUBLE_CLICK);
    let cell = realized(&tree, id, 1, 0);
    let at = tree.bounds(cell).center();
    double_click_at(&mut tree, at);
    assert_eq!(
        activated.get(),
        0,
        "double-clicking an editable cell opened the item as well as the editor"
    );
}

/// The read-only column beside it still activates, which is what makes the
/// guard a rule about *this gesture on an editable cell* rather than about
/// the whole table.
#[test]
fn a_double_click_off_an_editable_cell_still_activates_the_row() {
    let (mut tree, id, _, activated) = click_probe(EditTriggers::DOUBLE_CLICK);
    let cell = realized(&tree, id, 1, 1);
    let at = tree.bounds(cell).center();
    double_click_at(&mut tree, at);
    assert_eq!(
        activated.get(),
        1,
        "a double-click away from an editable cell must still activate the row"
    );
}

/// **A cell that edits on double-click still lets its row select on a
/// plain click.**
///
/// `press_claimed_by_interactive_child` counted `on_double_tap` as owning
/// the press, so merely giving a cell double-click-to-edit silently stopped
/// its row selecting — while every file manager selects a row on the first
/// click of the double-click that opens it. The claim is now about
/// handlers that act on a single press (`on_tap` / `on_long_press`).
#[test]
fn a_double_click_editable_cell_still_lets_its_row_select_on_one_click() {
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(three_row_slice())
            .selection(selection.clone())
            .add_column(editable_name_col().edit_triggers(EditTriggers::DOUBLE_CLICK))
            .row_height(20.0)
            .on_cell_edit_request(|_row, _col, _ctx| {}),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let cell = realized(&tree, id, 1, 0);
    tree.click(cell);
    assert!(
        selection.is_selected(1),
        "one click on a double-click-editable cell must still select its row"
    );
}

/// ...whereas `SINGLE_CLICK` deliberately does claim the press: that cell's
/// click means "edit this value", not "select this row". Documented on
/// [`EditTriggers::SINGLE_CLICK`] and the reason the set is per column.
#[test]
fn a_single_click_editable_cell_claims_the_press_from_row_selection() {
    let selection = teksilo_data::SelectionModel::new(teksilo_data::SelectionMode::Single);
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(three_row_slice())
            .selection(selection.clone())
            .add_column(editable_name_col().edit_triggers(EditTriggers::SINGLE_CLICK))
            .add_column(size_col())
            .row_height(20.0)
            .on_cell_edit_request(|_row, _col, _ctx| {}),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });

    let editable = realized(&tree, id, 1, 0);
    tree.click(editable);
    assert!(
        !selection.is_selected(1),
        "a SINGLE_CLICK cell's click must go to the editor, not to selection"
    );

    // The column beside it selects as always — which is what makes this a
    // property of the column rather than of the table.
    let plain = realized(&tree, id, 2, 1);
    tree.click(plain);
    assert!(
        selection.is_selected(2),
        "a click on a non-editing column must still select its row"
    );
}

/// The cell realized at `(row, display column)`.
fn realized(tree: &WidgetTree, id: WidgetId, row: usize, col: usize) -> WidgetId {
    let any = tree.widget_as_any(id).unwrap();
    let tt = any.downcast_ref::<TreeTableView<&'static str>>().unwrap();
    tt.realized_cell(row, col)
        .unwrap_or_else(|| panic!("cell ({row}, {col}) is not realized"))
}

/// A laid-out table whose first column is editable under `triggers` and
/// whose second is read-only, with the edit requests it receives and a
/// count of row activations.
#[allow(clippy::type_complexity)]
fn click_probe(
    triggers: EditTriggers,
) -> (
    WidgetTree,
    WidgetId,
    Rc<RefCell<Vec<(usize, String)>>>,
    Rc<Cell<usize>>,
) {
    let seen: Rc<RefCell<Vec<(usize, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let activated: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let counter = activated.clone();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let id = tree.add(
        TreeTableView::from_source(three_row_slice())
            .add_column(editable_name_col().edit_triggers(triggers))
            .add_column(size_col())
            .row_height(20.0)
            .on_cell_edit_request(move |row, col, _ctx| {
                sink.borrow_mut().push((row, col.to_string()));
            })
            .on_row_activate(move |_row, _ctx| counter.set(counter.get() + 1)),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(200.0),
    });
    (tree, id, seen, activated)
}

/// A **leaf** row's chevron is not a pointer target; a **branch** row's is.
///
/// Both halves are asserted, and that is the point of the test: a leaf-only
/// assertion passes just as well if chevrons stop being targets altogether, and
/// a branch-only one passes while every leaf carries an invisible 12 dp target
/// that does nothing.
///
/// The leaf half is what [`TreeBodyPane`] used to get wrong. It wired
/// `on_click` on every chevron, including the ones that paint nothing, and an
/// `on_click` is exactly what makes a node a pointer target — a 12 dp one, two
/// dp of it below the WCAG 2.2 SC 2.5.8 floor at every density, actuating
/// `toggle_at` on a row with nothing to toggle. `StandardTreeItem::build` has
/// always guarded the same wiring behind `has_children`, and
/// `TwistArrow::hit_outset` returns [`EdgeInsets::ZERO`] for a leaf, so the node
/// could not have grown to the floor even in principle.
///
/// "Is a pointer target" is asked through
/// [`measure_targets`](teksilo_core::accessibility::target_audit::measure_targets),
/// whose candidate set is `WidgetArena::takes_a_press` — the framework's own
/// definition of "would act on a press", and the same predicate the miss-only
/// slop pass reads for eligibility. Asked at Compact because the answer is
/// structural rather than dimensional: a chevron either carries the handler or
/// it does not, at every density.
#[test]
fn only_a_branch_rows_chevron_is_a_pointer_target() {
    use teksilo_core::accessibility::target_audit::measure_targets;
    use teksilo_tokens::TargetDensity;

    let proxy = SortFilterTreeModel::new(sample_tree());
    proxy.expand_all(); // docs@0 readme@1 guide@2 src@3 main.rs@4
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let view = tree.add(
        TreeTableView::from_projection(proxy.clone())
            .add_column(name_col())
            .row_height(20.0),
    );
    tree.layout(SizeProposal {
        width: Some(400.0),
        height: Some(300.0),
    });

    // Every chevron in the body, with the flat row index its bounds put it in.
    fn chevrons(tree: &WidgetTree, id: WidgetId, out: &mut Vec<(usize, WidgetId)>) {
        if tree.widget_type_name(id) == Some("teksilo_widgets::primitives::twist_arrow::TwistArrow")
        {
            let y = tree.bounds(id).center().y;
            let row = ((y - cp::HEADER_HEIGHT) / 20.0).floor().max(0.0) as usize;
            out.push((row, id));
        }
        for child in tree.children(id) {
            chevrons(tree, child, out);
        }
    }
    let mut found = Vec::new();
    chevrons(&tree, view, &mut found);
    found.sort_by_key(|(row, _)| *row);
    assert_eq!(
        found.iter().map(|(row, _)| *row).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4],
        "one chevron per visible row, leaves included — a leaf's chevron still \
         occupies the indent column, it is only not a target: {found:?}",
    );

    let targets: Vec<WidgetId> = measure_targets(&tree, TargetDensity::Compact)
        .into_iter()
        .filter(|m| m.part.is_none())
        .map(|m| m.node)
        .collect();

    for (row, chevron) in found {
        let is_branch = row == 0 || row == 3; // "docs" and "src"
        assert_eq!(
            targets.contains(&chevron),
            is_branch,
            "row {row}'s chevron is {}a pointer target, and it should {}be: \
             rows 0 and 3 are the branches, 1, 2 and 4 are leaves",
            if targets.contains(&chevron) {
                ""
            } else {
                "not "
            },
            if is_branch { "" } else { "not " },
        );
    }
}
