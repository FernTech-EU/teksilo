// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A rich-text document's tables and blockquotes, as assistive technology
//! sees them.
//!
//! These drive the real `accesskit_consumer` rather than reading the
//! `TreeUpdate` directly, because the two questions that matter here are
//! questions only the consumer can answer: which nodes a screen reader is
//! *shown* after filtering, and which node a run's text-change event is
//! *routed to*. A tree that looks right as a flat list of nodes can still be
//! one whose every keystroke is dropped.

use accesskit_consumer::{NodeRef, Tree, common_filter};
use teksilo_canvas::SizeProposal;
use teksilo_core::accesskit::Role;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{BlockFormat, MoveMode, TextDocument};
use teksilo_widgets::rich_text::RichTextEditor;

/// An editor over `text`, laid out and rendered, plus its document.
fn editor_with(text: &str) -> (TextDocument, WidgetTree) {
    let doc = TextDocument::new();
    doc.set_plain_text(text).unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let mut tree = WidgetTree::new();
    let _ = tree.add(editor);
    (doc, tree)
}

fn settle(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(500.0, 400.0));
    tree.request_frame();
    tree.tick_animations(std::time::Duration::from_millis(200));
    tree.layout(SizeProposal::exact(500.0, 400.0));
    let _ = tree.render();
}

/// Insert a 2x2 table after `at`, then fill its cells with `cells`.
fn table_document(prefix: &str, cells: &[&str]) -> (TextDocument, WidgetTree) {
    let (doc, mut tree) = editor_with(prefix);
    {
        let c = doc.cursor_at(0);
        c.set_position(prefix.chars().count(), MoveMode::MoveAnchor);
        c.insert_table(2, 2).unwrap();
    }
    settle(&mut tree);
    // Fill each cell by walking to the end of its own first block, re-reading
    // the flow each time: inserting text moves every later position.
    for (i, text) in cells.iter().enumerate() {
        let Some(pos) = nth_cell_end(&doc, i) else {
            continue;
        };
        let c = doc.cursor_at(0);
        c.set_position(pos, MoveMode::MoveAnchor);
        c.insert_text(text).unwrap();
        settle(&mut tree);
    }
    settle(&mut tree);
    (doc, tree)
}

/// Document position just past the end of the `i`th cell's first block.
fn nth_cell_end(doc: &TextDocument, i: usize) -> Option<usize> {
    use teksilo_text::text_document::FlowElementSnapshot;
    for el in &doc.snapshot_flow().elements {
        if let FlowElementSnapshot::Table(t) = el {
            let cell = t.cells.get(i)?;
            let block = cell.blocks.first()?;
            return Some(block.position + block.text.chars().count());
        }
    }
    None
}

/// Walk the consumer tree, collecting every node the predicate accepts.
fn collect<'a>(root: NodeRef<'a>, want: &dyn Fn(&NodeRef<'a>) -> bool) -> Vec<NodeRef<'a>> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if want(&node) {
            out.push(node);
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    out
}

fn editor_node<'a>(state: &'a accesskit_consumer::TreeState) -> NodeRef<'a> {
    collect(state.root(), &|n| n.role() == Role::MultilineTextInput)
        .into_iter()
        .next()
        .expect("the editor must publish a MultilineTextInput node")
}

// ── Structure ──────────────────────────────────────────────────────

#[test]
fn a_table_is_exposed_as_a_grid_of_rows_and_cells() {
    let (_doc, mut tree) = table_document("Intro", &["Alpha", "Beta", "Gamma", "Delta"]);
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let state = consumer.state();

    let tables = collect(state.root(), &|n| n.role() == Role::Table);
    assert_eq!(tables.len(), 1, "one table in, one table out");
    let table = &tables[0];
    assert_eq!(
        table.data().row_count(),
        Some(2),
        "the table must announce its own height"
    );
    assert_eq!(
        table.data().column_count(),
        Some(2),
        "the table must announce its own width"
    );

    let rows = collect(*table, &|n| n.role() == Role::Row);
    assert_eq!(rows.len(), 2, "one Role::Row per row");
    let mut row_indices: Vec<_> = rows.iter().filter_map(|r| r.data().row_index()).collect();
    row_indices.sort_unstable();
    assert_eq!(
        row_indices,
        vec![0, 1],
        "rows carry their own index. These are the raw AccessKit values, which \
         are zero-based — the builder's public surface is 1-based like ARIA and \
         converts at the boundary, and two of the three platform adapters add \
         the 1 back again. A walk that wrote the ARIA number straight through \
         would read 1, 2 here and announce every row one too high"
    );

    let cells = collect(*table, &|n| n.role() == Role::Cell);
    assert_eq!(cells.len(), 4, "one Role::Cell per cell");
    let mut coords: Vec<_> = cells
        .iter()
        .map(|c| (c.data().row_index(), c.data().column_index()))
        .collect();
    coords.sort();
    assert_eq!(
        coords,
        vec![
            (Some(0), Some(0)),
            (Some(0), Some(1)),
            (Some(1), Some(0)),
            (Some(1), Some(1)),
        ],
        "every cell must report where in the grid it sits, in the same \
         zero-based form AccessKit stores"
    );
}

#[test]
fn an_ordinary_cell_claims_no_span() {
    // Spans are written only when they exceed 1, so a plain table carries no
    // span properties at all — a reader is never told a 1x1 cell "spans 1".
    let (_doc, mut tree) = table_document("", &["a", "b", "c", "d"]);
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let cells = collect(consumer.state().root(), &|n| n.role() == Role::Cell);
    assert_eq!(
        cells.len(),
        4,
        "the table's cells must be in the tree at all"
    );
    for cell in cells {
        assert_eq!(cell.data().row_span(), None, "no row span on a 1x1 cell");
        assert_eq!(
            cell.data().column_span(),
            None,
            "no column span on a 1x1 cell"
        );
    }
}

// ── Text reaching the tree at all ──────────────────────────────────

#[test]
fn every_cell_s_text_reaches_the_accessibility_tree() {
    // The whole defect in one assertion: the editor's text document must be
    // the document's text. Before tables were walked, the cells' characters
    // were simply absent from it — a screen reader could not read them at
    // all, with or without table structure.
    let (_doc, mut tree) = table_document("Intro", &["Alpha", "Beta", "Gamma", "Delta"]);
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let editor = editor_node(consumer.state());

    let exposed = editor.document_range().text();
    for word in ["Intro", "Alpha", "Beta", "Gamma", "Delta"] {
        assert!(
            exposed.contains(word),
            "{word:?} is in the document but not in the accessibility tree \
             (exposed: {exposed:?})"
        );
    }
}

#[test]
fn a_blockquote_s_text_reaches_the_accessibility_tree() {
    // A frame is skipped by the same walk that skipped tables, so every
    // blockquote in every document was invisible too.
    let (doc, mut tree) = editor_with("Before\nQuoted line\nAfter");
    let editor_pos = "Before\n".chars().count();
    settle(&mut tree);
    {
        let c = doc.cursor_at(0);
        c.set_position(editor_pos, MoveMode::MoveAnchor);
        c.toggle_blockquote().unwrap();
    }
    settle(&mut tree);

    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let exposed = editor_node(consumer.state()).document_range().text();
    assert!(
        exposed.contains("Quoted line"),
        "a blockquote's text must reach the accessibility tree (exposed: \
         {exposed:?})"
    );
}

#[test]
fn cell_text_survives_multibyte_characters() {
    let (_doc, mut tree) = table_document("", &["café", "日本語", "e\u{0301}", "ok"]);
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let exposed = editor_node(consumer.state()).document_range().text();
    for word in ["café", "日本語"] {
        assert!(exposed.contains(word), "{word:?} missing from {exposed:?}");
    }
}

// ── The routing that makes an edit audible ─────────────────────────

/// Every text run's filtered parent, and whether that parent can carry a
/// text-change event.
///
/// This is the exact question `accesskit_atspi_common`'s
/// `emit_text_change_if_needed` asks: a changed run's event is rerouted to
/// `filtered_parent(&common_filter)`, and dropped unless that node
/// `supports_text_ranges()`. A run whose answer is `false` is a run whose
/// edits a screen reader never hears.
fn runs_with_unroutable_parents(tree: &mut WidgetTree) -> (Vec<String>, usize) {
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for run in collect(consumer.state().root(), &|n| n.role() == Role::TextRun) {
        checked += 1;
        let value = run.data().value().unwrap_or_default().to_string();
        match run.filtered_parent(&common_filter) {
            Some(parent) if parent.supports_text_ranges() => {}
            Some(parent) => bad.push(format!("{value:?} -> {:?}", parent.role())),
            None => bad.push(format!("{value:?} -> <no filtered parent>")),
        }
    }
    (bad, checked)
}

/// The text of every run under a node, so a test can say which runs it
/// actually inspected rather than trusting that any existed.
fn run_values(tree: &mut WidgetTree) -> Vec<String> {
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    collect(consumer.state().root(), &|n| n.role() == Role::TextRun)
        .into_iter()
        .map(|n| n.data().value().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn a_cell_s_edits_reach_a_text_range_capable_parent() {
    let (_doc, mut tree) = table_document("Intro", &["Alpha", "Beta", "Gamma", "Delta"]);
    // Name the cell runs first: with no cells in the tree at all, "every run
    // routes correctly" would be vacuously true and this test would keep
    // passing through exactly the bug it exists to catch.
    let values = run_values(&mut tree);
    for word in ["Alpha", "Beta", "Gamma", "Delta"] {
        assert!(
            values.iter().any(|v| v == word),
            "expected a run for the cell {word:?}; got {values:?}"
        );
    }

    let (bad, checked) = runs_with_unroutable_parents(&mut tree);
    assert!(
        checked >= 5,
        "expected the prose run and the four cell runs, checked {checked}"
    );
    assert!(
        bad.is_empty(),
        "every run's text-change event must route to a node that supports \
         text ranges, or typing there is silent. Unroutable: {bad:?}"
    );
}

#[test]
fn a_heading_s_edits_reach_a_text_range_capable_parent() {
    // `Role::Heading` is not text-range capable, so a heading's runs cannot
    // hang off it directly — the same trap a `Role::Cell` sets.
    let (doc, mut tree) = editor_with("Chapter one\nBody text");
    settle(&mut tree);
    {
        let c = doc.cursor_at(0);
        c.set_position(0, MoveMode::MoveAnchor);
        c.set_block_format(&BlockFormat {
            heading_level: Some(1),
            ..Default::default()
        })
        .unwrap();
    }
    settle(&mut tree);

    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    assert_eq!(
        collect(consumer.state().root(), &|n| n.role() == Role::Heading).len(),
        1,
        "the heading must keep its own node — it is how a reader jumps \
         through a document"
    );

    let (bad, checked) = runs_with_unroutable_parents(&mut tree);
    assert!(checked > 0, "the heading must emit runs to check");
    assert!(
        bad.is_empty(),
        "a heading's runs must route their text changes somewhere that \
         supports text ranges. Unroutable: {bad:?}"
    );
}

#[test]
fn a_blockquote_s_edits_reach_a_text_range_capable_parent() {
    let (doc, mut tree) = editor_with("Before\nQuoted line\nAfter");
    settle(&mut tree);
    {
        let c = doc.cursor_at(0);
        c.set_position("Before\n".chars().count(), MoveMode::MoveAnchor);
        c.toggle_blockquote().unwrap();
    }
    settle(&mut tree);
    let values = run_values(&mut tree);
    assert!(
        values.iter().any(|v| v == "Quoted line"),
        "the blockquote's run must exist before its routing means anything; \
         got {values:?}"
    );
    let (bad, _) = runs_with_unroutable_parents(&mut tree);
    assert!(bad.is_empty(), "unroutable runs in a blockquote: {bad:?}");
}

// ── Structural edits ───────────────────────────────────────────────

#[test]
fn inserting_a_row_grows_the_announced_grid() {
    let (doc, mut tree) = table_document("", &["a", "b", "c", "d"]);
    let table_id = {
        use teksilo_text::text_document::FlowElementSnapshot;
        doc.snapshot_flow()
            .elements
            .iter()
            .find_map(|el| match el {
                FlowElementSnapshot::Table(t) => Some(t.table_id),
                _ => None,
            })
            .expect("a table")
    };

    doc.cursor_at(0).insert_table_row(table_id, 1).unwrap();
    settle(&mut tree);

    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let table = collect(consumer.state().root(), &|n| n.role() == Role::Table)
        .into_iter()
        .next()
        .expect("the table must survive a row insertion");
    assert_eq!(
        table.data().row_count(),
        Some(3),
        "the announced row count must follow the document"
    );
    assert_eq!(
        collect(table, &|n| n.role() == Role::Row).len(),
        3,
        "a row insertion must produce a new Role::Row"
    );
}

#[test]
fn editing_a_cell_keeps_its_text_in_the_tree() {
    let (doc, mut tree) = table_document("", &["a", "b", "c", "d"]);
    let pos = nth_cell_end(&doc, 0).expect("first cell");
    {
        let c = doc.cursor_at(0);
        c.set_position(pos, MoveMode::MoveAnchor);
        c.insert_text("ppend").unwrap();
    }
    settle(&mut tree);

    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let exposed = editor_node(consumer.state()).document_range().text();
    assert!(
        exposed.contains("append"),
        "an edit inside a cell must show up in the accessibility tree \
         (exposed: {exposed:?})"
    );
    let (bad, _) = runs_with_unroutable_parents(&mut tree);
    assert!(
        bad.is_empty(),
        "an edited cell must still route its text changes: {bad:?}"
    );
}

// ── Geometry ───────────────────────────────────────────────────────

#[test]
fn cell_bounds_follow_the_table_grid() {
    // A run with no usable geometry empties `bounding_boxes()` for every
    // range that touches it, so cells need real boxes — laid out left to
    // right and top to bottom.
    let (_doc, mut tree) = table_document("Intro", &["Alpha", "Beta", "Gamma", "Delta"]);
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);

    let mut boxes = Vec::new();
    for cell in collect(consumer.state().root(), &|n| n.role() == Role::Cell) {
        let (Some(row), Some(col)) = (cell.data().row_index(), cell.data().column_index()) else {
            continue;
        };
        let bbox = cell
            .bounding_box()
            .unwrap_or_else(|| panic!("cell ({row}, {col}) must have a bounding box"));
        assert!(
            bbox.width() > 0.0 && bbox.height() > 0.0,
            "cell ({row}, {col}) has an empty box: {bbox:?}"
        );
        boxes.push((row, col, bbox));
    }
    assert_eq!(boxes.len(), 4);

    let at = |row: usize, col: usize| {
        boxes
            .iter()
            .find(|(r, c, _)| *r == row && *c == col)
            .map(|(_, _, b)| *b)
            .unwrap()
    };
    assert!(
        at(0, 0).x0 < at(0, 1).x0,
        "column 1 must sit to the right of column 0 ({:?} vs {:?})",
        at(0, 0),
        at(0, 1)
    );
    assert!(
        at(0, 0).y0 < at(1, 0).y0,
        "row 1 must sit below row 0 ({:?} vs {:?})",
        at(0, 0),
        at(1, 0)
    );
}

#[test]
fn a_cell_s_runs_carry_their_own_boxes() {
    // `Range::bounding_boxes()` throws away everything it has collected the
    // moment one run is missing geometry, so a cell run with no box does not
    // just lose its own rectangle — it blanks the whole document's.
    let (_doc, mut tree) = table_document("", &["Alpha", "Beta", "Gamma", "Delta"]);
    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let state = consumer.state();

    for word in ["Alpha", "Beta", "Gamma", "Delta"] {
        let run = collect(state.root(), &|n| {
            n.role() == Role::TextRun && n.data().value() == Some(word)
        })
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no run for the cell {word:?}"));
        let bbox = run
            .bounding_box()
            .unwrap_or_else(|| panic!("the run for {word:?} carries no bounding box"));
        assert!(
            bbox.width() > 0.0 && bbox.height() > 0.0,
            "the run for {word:?} has an empty box: {bbox:?}"
        );
    }

    let boxes = editor_node(state).document_range().bounding_boxes();
    assert!(
        !boxes.is_empty(),
        "the document range must yield geometry; one run missing a box \
         empties the whole collection"
    );
}

// ── Caret and undo ─────────────────────────────────────────────────

/// A document holding one filled 2x2 table, built before any editor exists so
/// a caret can be placed inside a cell at construction time.
fn document_with_table(cells: &[&str]) -> TextDocument {
    let doc = TextDocument::new();
    doc.set_plain_text("").unwrap();
    {
        let c = doc.cursor_at(0);
        c.set_position(0, MoveMode::MoveAnchor);
        c.insert_table(2, 2).unwrap();
    }
    for (i, text) in cells.iter().enumerate() {
        let Some(pos) = nth_cell_end(&doc, i) else {
            continue;
        };
        let c = doc.cursor_at(0);
        c.set_position(pos, MoveMode::MoveAnchor);
        c.insert_text(text).unwrap();
    }
    doc
}

#[test]
fn a_caret_inside_a_cell_is_reported_on_that_cell_s_run() {
    // Without this, a screen reader following the caret loses it the moment
    // it crosses into a table: the selection would be reported against some
    // other block's run, or not at all.
    let doc = document_with_table(&["Alpha", "Beta", "Gamma", "Delta"]);
    let third_cell = nth_cell_end(&doc, 2).expect("third cell");

    let editor = RichTextEditor::editor(doc.clone());
    editor.set_caret_position(third_cell);
    let mut tree = WidgetTree::new();
    let _ = tree.add(editor);
    settle(&mut tree);

    let update = tree.sync_accessibility();
    let selection = update
        .nodes
        .iter()
        .find_map(|(_, n)| n.text_selection())
        .expect("a document with a caret must report a text selection");

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let focus_run = collect(consumer.state().root(), &|n| {
        n.role() == Role::TextRun && n.locate().0 == selection.focus.node
    })
    .into_iter()
    .next()
    .expect("the reported selection focus must be a TextRun that exists");

    assert_eq!(
        focus_run.data().value(),
        Some("Gamma"),
        "a caret at the end of the third cell belongs to that cell's run"
    );
}

#[test]
fn undoing_a_cell_edit_restores_the_announced_text() {
    let (doc, mut tree) = table_document("", &["Alpha", "Beta", "Gamma", "Delta"]);

    let pos = nth_cell_end(&doc, 0).expect("first cell");
    {
        let c = doc.cursor_at(0);
        c.set_position(pos, MoveMode::MoveAnchor);
        c.insert_text("-edited").unwrap();
    }
    settle(&mut tree);

    let exposed = {
        let consumer = Tree::new(tree.sync_accessibility(), false);
        editor_node(consumer.state()).document_range().text()
    };
    assert!(
        exposed.contains("Alpha-edited"),
        "precondition: {exposed:?}"
    );

    doc.undo().unwrap();
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let after = editor_node(consumer.state()).document_range().text();
    assert!(
        !after.contains("-edited") && after.contains("Alpha"),
        "undo must roll the cell's announced text back too (got {after:?})"
    );
    let (bad, checked) = runs_with_unroutable_parents(&mut tree);
    assert!(checked > 0 && bad.is_empty(), "after undo: {bad:?}");
}

/// The shape the blocker was reported against: insert a table, type nothing
/// into it, and ask what assistive technology can see.
///
/// An empty cell still has to be announced — a reader navigating a freshly
/// inserted grid needs to know it is in one, and where — so the structure is
/// asserted independently of there being any text to read.
#[test]
fn an_empty_table_still_announces_its_grid() {
    let doc = TextDocument::new();
    doc.set_plain_text("Table test").unwrap();
    {
        let c = doc.cursor_at(0);
        c.set_position("Table test".chars().count(), MoveMode::MoveAnchor);
        c.insert_table(3, 3).unwrap();
    }
    let editor = RichTextEditor::editor(doc.clone());
    let mut tree = WidgetTree::new();
    let _ = tree.add(editor);
    settle(&mut tree);

    let update = tree.sync_accessibility();
    let consumer = Tree::new(update, false);
    let state = consumer.state();

    let table = collect(state.root(), &|n| n.role() == Role::Table)
        .into_iter()
        .next()
        .expect("an empty table is still a table");
    assert_eq!(table.data().row_count(), Some(3));
    assert_eq!(table.data().column_count(), Some(3));
    assert_eq!(collect(table, &|n| n.role() == Role::Row).len(), 3);
    assert_eq!(collect(table, &|n| n.role() == Role::Cell).len(), 9);

    // The prose around it is untouched.
    let exposed = editor_node(state).document_range().text();
    assert!(exposed.contains("Table test"), "exposed: {exposed:?}");
}

fn table_id_of(doc: &TextDocument) -> usize {
    use teksilo_text::text_document::FlowElementSnapshot;
    doc.snapshot_flow()
        .elements
        .iter()
        .find_map(|el| match el {
            FlowElementSnapshot::Table(t) => Some(t.table_id),
            _ => None,
        })
        .expect("a table")
}

#[test]
fn removing_a_row_shrinks_the_announced_grid() {
    let (doc, mut tree) = table_document("", &["a", "b", "c", "d"]);
    let table_id = table_id_of(&doc);

    doc.cursor_at(0).remove_table_row(table_id, 0).unwrap();
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let table = collect(consumer.state().root(), &|n| n.role() == Role::Table)
        .into_iter()
        .next()
        .expect("the table must survive a row removal");
    assert_eq!(table.data().row_count(), Some(1));
    assert_eq!(collect(table, &|n| n.role() == Role::Row).len(), 1);
    assert_eq!(collect(table, &|n| n.role() == Role::Cell).len(), 2);
}

#[test]
fn removing_a_column_shrinks_the_announced_grid() {
    let (doc, mut tree) = table_document("", &["a", "b", "c", "d"]);
    let table_id = table_id_of(&doc);

    doc.cursor_at(0).remove_table_column(table_id, 0).unwrap();
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let table = collect(consumer.state().root(), &|n| n.role() == Role::Table)
        .into_iter()
        .next()
        .expect("the table must survive a column removal");
    assert_eq!(table.data().column_count(), Some(1));

    // Every surviving cell must still report a column inside the new width —
    // a stale coordinate would send a reader to a column that is gone.
    for cell in collect(table, &|n| n.role() == Role::Cell) {
        let col = cell.data().column_index().expect("a cell coordinate");
        assert!(col < 1, "cell reports column {col} in a 1-column table");
    }
}

#[test]
fn a_cell_holding_two_paragraphs_reads_as_one_cell() {
    // A cell's blocks share one text container, so a second paragraph does
    // not split the cell into two announced pieces.
    let (doc, mut tree) = table_document("", &["first", "b", "c", "d"]);
    let pos = nth_cell_end(&doc, 0).expect("first cell");
    {
        let c = doc.cursor_at(0);
        c.set_position(pos, MoveMode::MoveAnchor);
        c.insert_block().unwrap();
        c.insert_text("second").unwrap();
    }
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let state = consumer.state();

    let cells = collect(state.root(), &|n| n.role() == Role::Cell);
    assert_eq!(cells.len(), 4, "a second paragraph adds no cell");

    let containers: Vec<_> = cells
        .iter()
        .flat_map(|c| c.children())
        .filter(|n| n.role() == Role::Label)
        .collect();
    assert_eq!(
        containers.len(),
        4,
        "one text container per cell, however many paragraphs it holds"
    );

    let exposed = editor_node(state).document_range().text();
    assert!(
        exposed.contains("first") && exposed.contains("second"),
        "both of the cell's paragraphs must be readable (exposed: {exposed:?})"
    );
}

// ── Blockquotes and merged cells ───────────────────────────────────

#[test]
fn a_blockquote_is_announced_as_a_quotation() {
    // A quotation is information no run carries. The quote node keeps its
    // role, and the text container beneath it keeps the edits audible — the
    // pairing is what lets it have both.
    let (doc, mut tree) = editor_with("Before\nQuoted line\nAfter");
    settle(&mut tree);
    {
        let c = doc.cursor_at(0);
        c.set_position("Before\n".chars().count(), MoveMode::MoveAnchor);
        c.toggle_blockquote().unwrap();
    }
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let state = consumer.state();

    let quotes = collect(state.root(), &|n| n.role() == Role::Blockquote);
    assert_eq!(quotes.len(), 1, "the blockquote must announce itself");

    let quoted = collect(quotes[0], &|n| {
        n.role() == Role::TextRun && n.data().value() == Some("Quoted line")
    });
    assert_eq!(
        quoted.len(),
        1,
        "the quoted prose belongs inside the quote node"
    );

    // Prose outside the quote must NOT have been swept into it.
    for word in ["Before", "After"] {
        assert!(
            collect(quotes[0], &|n| n.data().value() == Some(word)).is_empty(),
            "{word:?} is not part of the quotation"
        );
    }

    let (bad, checked) = runs_with_unroutable_parents(&mut tree);
    assert!(checked >= 3 && bad.is_empty(), "unroutable: {bad:?}");
}

#[test]
fn a_merged_cell_is_boxed_over_every_track_it_spans() {
    // A merged cell sized from its origin track alone leaves the rest of the
    // area it visibly covers pointing at nothing, so a magnifier or a
    // touch-explore user aiming at its far half hits the gap between boxes.
    let (doc, mut tree) = table_document("", &["a", "b", "c", "d"]);
    let table_id = table_id_of(&doc);
    doc.cursor_at(0)
        .merge_table_cells(table_id, 0, 0, 0, 1)
        .unwrap();
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    let state = consumer.state();

    let cells = collect(state.root(), &|n| n.role() == Role::Cell);
    let merged = cells
        .iter()
        .find(|c| c.data().column_span() == Some(2))
        .expect("the merged cell must announce its span");
    let plain = cells
        .iter()
        .find(|c| c.data().column_span().is_none() && c.data().row_index() == Some(1))
        .expect("an unmerged cell on the second row");

    let merged_box = merged.bounding_box().expect("merged cell box");
    let plain_box = plain.bounding_box().expect("plain cell box");
    assert!(
        merged_box.width() > plain_box.width() * 1.5,
        "a cell spanning two columns must be boxed over both (merged {:?} vs \
         single {:?})",
        merged_box,
        plain_box
    );
}

#[test]
fn a_cell_s_text_container_carries_the_cell_s_box() {
    // The container is the node a reader announces as the cell's content, so
    // a `GetExtents` on it has to answer.
    let (_doc, mut tree) = table_document("", &["Alpha", "b", "c", "d"]);
    let consumer = Tree::new(tree.sync_accessibility(), false);
    let state = consumer.state();

    let cells = collect(state.root(), &|n| n.role() == Role::Cell);
    assert_eq!(cells.len(), 4);
    for cell in cells {
        let cell_box = cell.bounding_box().expect("cell box");
        let container = cell
            .children()
            .find(|n| n.role() == Role::Label)
            .expect("each cell carries a text container");
        let container_box = container
            .bounding_box()
            .expect("the text container must carry a box of its own");
        assert!(
            (container_box.width() - cell_box.width()).abs() < 0.5
                && (container_box.height() - cell_box.height()).abs() < 0.5,
            "the container covers the cell it speaks for ({container_box:?} vs \
             {cell_box:?})"
        );
    }
}

/// The code editor's heading path had the identical defect, and its own
/// comment named the constraint it then broke.
///
/// `PlainTextEditor` and `CodeEditor` share this walk, and both accept a
/// document whose block carries a heading level — so the same runs-on-a-
/// `Role::Heading` arrangement left the same edits unannounced there.
#[test]
fn a_code_editor_heading_s_edits_reach_a_text_range_capable_parent() {
    use teksilo_widgets::code_editor::PlainTextEditor;

    let doc = TextDocument::new();
    doc.set_plain_text("Chapter one\nBody text").unwrap();
    {
        let c = doc.cursor_at(0);
        c.set_position(0, MoveMode::MoveAnchor);
        c.set_block_format(&BlockFormat {
            heading_level: Some(1),
            ..Default::default()
        })
        .unwrap();
    }

    let mut tree = WidgetTree::new();
    let _ = tree.add(PlainTextEditor::new(doc.clone()));
    settle(&mut tree);

    let consumer = Tree::new(tree.sync_accessibility(), false);
    assert_eq!(
        collect(consumer.state().root(), &|n| n.role() == Role::Heading).len(),
        1,
        "the heading keeps its own node here too"
    );

    let (bad, checked) = runs_with_unroutable_parents(&mut tree);
    assert!(checked > 0, "the editor must emit runs to check");
    assert!(
        bad.is_empty(),
        "a code-editor heading's runs must route their text changes somewhere \
         that supports text ranges. Unroutable: {bad:?}"
    );
}
