//! Headless tests for `GridView` (Phase 1: uniform grid).
//!
//! No GPU / display server needed — exercises virtualization, column-count
//! derivation, tile placement, selection, keyboard navigation, data-change
//! reconciliation, and accessibility roles.

use super::*;
use bastyde_canvas::SizeProposal;
use bastyde_core::widget::LayoutContext;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_data::{ListModel, SelectionMode, SelectionModel};

#[derive(Debug)]
struct FixedLeaf(f32, f32);
impl Widget for FixedLeaf {
    fn layout_response(
        &self,
        _proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        Size::new(self.0, self.1).into()
    }
}

/// A grid of `count` items, fixed 100×50 tiles, default 8px gaps.
fn make_grid(count: usize) -> (WidgetTree, WidgetId, ListModel<usize>) {
    let model = ListModel::from_vec((0..count).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0))).tile_size(100.0, 50.0),
    );
    (tree, id, model)
}

/// The body pane is the first child; its children are the tile wrappers.
fn tiles(tree: &WidgetTree, grid_id: WidgetId) -> Vec<WidgetId> {
    let children = tree.children(grid_id);
    let body = children[0];
    tree.children(body)
}

#[test]
fn virtualization_realizes_only_visible_tiles() {
    // 300 items, 3 columns → 100 rows. Viewport shows ~6 rows; with the
    // 5-row buffer that's ~12 rows × 3 = ~36 tiles, far fewer than 300.
    let (mut tree, id, _model) = make_grid(300);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let n = tiles(&tree, id).len();
    assert!(n < 60, "expected far fewer than 300 tiles, got {n}");
    assert!(n >= 18, "expected at least 18 tiles realized, got {n}");
}

#[test]
fn fixed_tile_size_derives_three_columns() {
    let (mut tree, id, _model) = make_grid(30);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    // tile 0 at (0,0); tile 1 at x = 100 + 8 = 108; tile 3 starts row 1.
    let b0 = tree.bounds(t[0]);
    let b1 = tree.bounds(t[1]);
    let b3 = tree.bounds(t[3]);
    assert!((b0.x - 0.0).abs() < 0.01, "tile 0 x = {}", b0.x);
    assert!((b1.x - 108.0).abs() < 0.01, "tile 1 x = {}", b1.x);
    assert!((b0.y - 0.0).abs() < 0.01, "tile 0 y = {}", b0.y);
    // Row 1 = tile_height (50) + row_gap (8) = 58.
    assert!((b3.y - 58.0).abs() < 0.01, "tile 3 y = {}", b3.y);
    assert!((b3.x - 0.0).abs() < 0.01, "tile 3 x = {}", b3.x);
}

#[test]
fn fixed_column_count_uses_exact_columns() {
    let model = ListModel::from_vec((0..40).collect());
    let mut tree = WidgetTree::new();
    let id =
        tree.add(GridView::new(model, |_tc| Box::new(FixedLeaf(10.0, 40.0))).column_count(4, 40.0));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    // 4 columns: tile 4 wraps to row 1.
    let b0 = tree.bounds(t[0]);
    let b4 = tree.bounds(t[4]);
    assert!((b0.y - 0.0).abs() < 0.01);
    assert!((b4.y - 48.0).abs() < 0.01, "tile 4 y = {} (row 1)", b4.y);
}

#[test]
fn empty_model_realizes_no_tiles() {
    let (mut tree, id, _model) = make_grid(0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Children: scrollbar + (no body, no overlay). No tile body pane.
    let children = tree.children(id);
    // With no items there's no body pane; only the scrollbar remains.
    assert!(
        children.len() <= 1,
        "empty grid should have at most a scrollbar child, got {}",
        children.len()
    );
}

#[test]
fn data_change_triggers_rebuild() {
    let (mut tree, id, model) = make_grid(6);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id).len(), 6);

    model.push(99);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id).len(), 7);

    model.remove(0);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(tiles(&tree, id).len(), 6);
}

#[test]
fn click_selects_tile() {
    let model = ListModel::from_vec((0..12).collect());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    tree.click(t[2]);
    assert!(selection.is_selected(2), "tile 2 should be selected");
    assert!(!selection.is_selected(0));
}

#[test]
fn arrow_keys_move_focus_and_selection() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec((0..30).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);

    // Start at 0; ArrowRight → 1 (same row).
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert!(selection.is_selected(1), "ArrowRight selects index 1");

    // ArrowDown → 1 + 3 columns = 4.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowDown,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert!(
        selection.is_selected(4),
        "ArrowDown moves by one row (cols)"
    );
}

#[test]
fn ctrl_a_selects_all() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec((0..12).collect());
    let selection = SelectionModel::new(SelectionMode::Multi);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::A,
        modifiers: Modifiers::CTRL,
        text: None,
    });
    assert_eq!(selection.count(), 12, "Ctrl+A selects every item");
}

#[test]
fn container_has_grid_role_and_counts() {
    let (mut tree, id, _model) = make_grid(30);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let info = tree.accessibility_node(id);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::Grid);
}

#[test]
fn tiles_have_gridcell_role() {
    let (mut tree, id, _model) = make_grid(12);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let t = tiles(&tree, id);
    let info = tree.accessibility_node(t[0]);
    assert_eq!(info.role(), bastyde_core::accesskit::Role::GridCell);
}

// ── Phase 2: variable row heights + anchoring ───────────────────────────

#[test]
fn exact_item_height_positions_rows_by_height() {
    // Single column → row r == item r. Exact heights [100, 50, 50, ...].
    let model = ListModel::from_vec((0..20).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |tc| {
            let h = if tc.index == 0 { 100.0 } else { 50.0 };
            Box::new(FixedLeaf(50.0, h))
        })
        .column_count(1, 50.0)
        .item_height(|i| if i == 0 { 100.0 } else { 50.0 }),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));
    let t = tiles(&tree, id);
    // Row 0 height 100 at y=0; row 1 at y = 100 + gap(8) = 108; height 50.
    assert!((tree.bounds(t[0]).y - 0.0).abs() < 0.01);
    assert!((tree.bounds(t[0]).height - 100.0).abs() < 0.01);
    assert!(
        (tree.bounds(t[1]).y - 108.0).abs() < 0.01,
        "row 1 y = {}",
        tree.bounds(t[1]).y
    );
    assert!((tree.bounds(t[2]).y - 166.0).abs() < 0.01); // 108 + 50 + 8
}

#[test]
fn auto_measure_places_rows_at_measured_heights() {
    // Estimate 50 but every tile actually measures 30. After one layout the
    // realized rows are placed using the measured height, not the estimate.
    let model = ListModel::from_vec((0..40).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 30.0)))
            .column_count(1, 50.0)
            .variable_row_heights(50.0),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));
    // A second layout lets the anchored scroll settle; positions are stable.
    tree.layout(SizeProposal::exact(200.0, 300.0));
    let t = tiles(&tree, id);
    // Measured: row 1 at 30 + gap(8) = 38 (not the 58 an estimate would give).
    assert!(
        (tree.bounds(t[1]).y - 38.0).abs() < 0.5,
        "row 1 y = {} (expected ~38 from measured height)",
        tree.bounds(t[1]).y
    );
    assert!((tree.bounds(t[1]).height - 30.0).abs() < 0.5);
}

#[test]
fn auto_measure_under_realization_converges_without_scroll() {
    // Estimate 50, actual 20: the first build realizes far too few rows
    // for the viewport (the estimated offsets say ~6 rows fill 300 px,
    // the measured ones say 15 do). The post-measure realization
    // re-check must request rebuilds until realized tiles cover the
    // whole viewport — previously the bottom gap only healed on the
    // next scroll event.
    let model = ListModel::from_vec((0..200).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 20.0)))
            .column_count(1, 50.0)
            .row_spacing(0.0)
            .variable_row_heights(50.0),
    );
    // Let the measure → re-check → rebuild cycle settle. No scroll input.
    for _ in 0..6 {
        tree.layout(SizeProposal::exact(200.0, 300.0));
    }
    let t = tiles(&tree, id);
    let last_bottom = t
        .iter()
        .map(|id| {
            let b = tree.bounds(*id);
            b.y + b.height
        })
        .fold(0.0_f32, f32::max);
    assert!(
        last_bottom >= 300.0,
        "realized tiles must cover the viewport bottom without scrolling, got {last_bottom}"
    );
}

#[test]
fn variable_grid_virtualizes() {
    let model = ListModel::from_vec((0..500).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 40.0)))
            .column_count(2, 40.0)
            .variable_row_heights(40.0),
    );
    tree.layout(SizeProposal::exact(200.0, 300.0));
    let n = tiles(&tree, id).len();
    assert!(n < 80, "variable grid should virtualize, got {n} tiles");
    assert!(n >= 10);
}

// ── Phase 4: sections ───────────────────────────────────────────────────

struct TwoSections;
impl super::sections::SectionProvider for TwoSections {
    fn section_count(&self) -> usize {
        2
    }
    fn items_in_section(&self, _s: usize) -> usize {
        3
    }
    fn section_title(&self, s: usize) -> String {
        format!("Section {s}")
    }
}

#[test]
fn sections_offset_tiles_below_headers() {
    // 2 columns, 50px tiles, 28px headers, 8px gaps. Section 0 header at
    // y=0, its band starts at y=28. Section 1 (after section 0's 2-row band
    // = 28 + 108 + 8 gap = 144) header at y=144, band at 172.
    let model = ListModel::from_vec((0..6).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(50.0, 50.0)))
            .column_count(2, 50.0)
            .section_header_height(28.0)
            .sections(TwoSections),
    );
    tree.layout(SizeProposal::exact(300.0, 600.0));
    let kids = tree.children(id);
    let body = kids[0];
    let body_kids = tree.children(body);
    // First 6 body children are tiles (flat order), then 2 headers.
    let tile0 = tree.bounds(body_kids[0]);
    let tile3 = tree.bounds(body_kids[3]);
    assert!((tile0.y - 28.0).abs() < 0.5, "tile 0 y = {}", tile0.y);
    assert!((tile3.y - 172.0).abs() < 0.5, "tile 3 y = {}", tile3.y);
    // The header wrappers carry RowHeader role.
    let header0 = tree.accessibility_node(body_kids[6]);
    assert_eq!(
        header0.role(),
        bastyde_core::accesskit::Role::RowHeader,
        "section header should be RowHeader"
    );
}

// ── Phase 4: waterfall ──────────────────────────────────────────────────

#[test]
fn waterfall_places_into_shortest_column() {
    // 2 columns, exact heights [60, 100, 40]. Item 0 → col 0; item 1 → col 1;
    // item 2 → col 0 (shorter). Gaps 8.
    let heights = [60.0_f32, 100.0, 40.0, 80.0, 50.0];
    let model = ListModel::from_vec((0..heights.len()).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, move |tc| {
            Box::new(FixedLeaf(80.0, heights[tc.index]))
        })
        .column_count(2, 60.0)
        .waterfall(60.0)
        .item_height(move |i| heights[i]),
    );
    tree.layout(SizeProposal::exact(200.0, 400.0));
    let t = tiles(&tree, id);
    let b0 = tree.bounds(t[0]);
    let b1 = tree.bounds(t[1]);
    let b2 = tree.bounds(t[2]);
    // Item 0 in column 0 at y=0; item 1 in column 1 at y=0; item 2 stacks
    // under item 0 (column 0) at y = 60 + gap(8) = 68.
    assert!((b0.y - 0.0).abs() < 0.5);
    assert!((b1.y - 0.0).abs() < 0.5);
    assert!(b1.x > b0.x, "item 1 should be in a column to the right");
    assert!((b2.x - b0.x).abs() < 0.5, "item 2 shares column 0");
    assert!((b2.y - 68.0).abs() < 0.5, "item 2 y = {}", b2.y);
}

// ── Phase 3: reorder / activation / type-ahead / incremental loading ────

#[test]
fn alt_arrow_reorders_tile() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let model = ListModel::from_vec(vec![10usize, 20, 30, 40]);
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model.clone(), |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .reorderable(true),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    // Focus starts at 0; Alt+ArrowRight moves item 0 forward by one.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::ALT,
        text: None,
    });
    assert_eq!(model.with_item(0, |v| *v), Some(20));
    assert_eq!(model.with_item(1, |v| *v), Some(10));
}

#[test]
fn enter_activates_focused_tile() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    use std::cell::Cell;
    use std::rc::Rc;
    let model = ListModel::from_vec((0..12).collect());
    let activated = Rc::new(Cell::new(usize::MAX));
    let a = activated.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .on_tile_activate(move |idx, _ctx| a.set(idx)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    // Move focus to index 1, then Enter activates it.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::ArrowRight,
        modifiers: Modifiers::default(),
        text: None,
    });
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
        text: None,
    });
    assert_eq!(activated.get(), 1);
}

#[test]
fn type_ahead_jumps_to_match() {
    use bastyde_core::event::{Key, Modifiers, WidgetEvent};
    let names = vec![
        "apple".to_string(),
        "banana".to_string(),
        "cherry".to_string(),
        "date".to_string(),
    ];
    let model = ListModel::from_vec(names.clone());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel)
            .type_ahead_label(move |i| names[i].clone()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(id);
    // Typing 'c' jumps to "cherry" (index 2).
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Character('c'),
        modifiers: Modifiers::default(),
        text: Some("c".to_string()),
    });
    assert!(selection.is_selected(2), "type-ahead 'c' selects cherry");
}

#[test]
fn on_near_end_fires_when_scrolled_to_bottom() {
    use std::cell::Cell;
    use std::rc::Rc;
    let model = ListModel::from_vec((0..300).collect());
    let fired = Rc::new(Cell::new(false));
    let f = fired.clone();
    let mut tree = WidgetTree::new();
    let gv = GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
        .tile_size(100.0, 50.0)
        .on_near_end(10, move || f.set(true));
    let scroll = gv.scroll_y_signal().clone();
    let max_sig = gv.max_scroll_y_signal().clone();
    let _id = tree.add(gv);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    // Scroll to the bottom; the scroll observer fires the hook.
    scroll.set(max_sig.get());
    assert!(fired.get(), "on_near_end should fire near the bottom");
}

#[test]
fn custom_grid_view_style_is_accepted() {
    // A Tier-3 style override installs without breaking layout.
    struct LoudStyle;
    impl bastyde_core::styles::GridViewStyle for LoudStyle {
        fn focus_ring(&self) -> bastyde_core::styles::GridFocusRingRecipe {
            bastyde_core::styles::GridFocusRingRecipe {
                role: bastyde_tokens::BorderRole::Accent,
                thickness: 3.0,
                inset: 0.0,
            }
        }
    }
    let model = ListModel::from_vec((0..12).collect());
    let mut tree = WidgetTree::new();
    let id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .style(LoudStyle),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert!(!tiles(&tree, id).is_empty());
}

#[test]
fn on_selection_changed_fires() {
    use std::cell::Cell;
    use std::rc::Rc;
    let model = ListModel::from_vec((0..6).collect());
    let selection = SelectionModel::new(SelectionMode::Single);
    let sel = selection.clone();
    let fired = Rc::new(Cell::new(0u32));
    let f = fired.clone();
    let mut tree = WidgetTree::new();
    let _id = tree.add(
        GridView::new(model, |_tc| Box::new(FixedLeaf(100.0, 50.0)))
            .tile_size(100.0, 50.0)
            .selection(sel)
            .on_selection_changed(move |_set| f.set(f.get() + 1)),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    selection.select(2);
    assert!(fired.get() >= 1, "on_selection_changed should fire");
}
