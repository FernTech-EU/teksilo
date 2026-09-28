// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a frame costs the editor while a selection holding a table stands.
//!
//! The paint hands the typesetter every table cell the selection holds, so it
//! can draw them whole, and a frame asks for them twice: once from the frame
//! tick and once from the paint. Asked of the document each time, a selection
//! over a long text walked the whole text twice a frame to find its tables,
//! and a selection holding a table looked each of its cells up among all of
//! the table's cells, a cost in the square of the table's size: a frame took a
//! tenth of a second over a table of a thousand cells and six seconds over one
//! of five thousand, for as long as it stayed selected, the caret's blink
//! included.
//!
//! These tests count work rather than time, as `rich_text_quote_probe_cost`
//! does, so a loaded machine cannot fail them and a fast one cannot hide a
//! regression. The work is the bytes allocated on the test's own thread over
//! a frame: every lookup in the document model copies what it reads.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

use teksilo_canvas::SizeProposal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::TextDocument;
use teksilo_widgets::rich_text::{EditorHandle, RichTextEditor};

/// The system allocator, counting what each thread asks it for.
struct CountingAllocator;

thread_local! {
    /// Bytes this thread has asked for so far. A `const` thread-local of a
    /// type with no destructor never allocates, so the allocator may use it.
    static ALLOCATED: Cell<u64> = const { Cell::new(0) };
}

fn count(bytes: usize) {
    // `try_with` because a thread being torn down may still free and allocate.
    let _ = ALLOCATED.try_with(|total| total.set(total.get().saturating_add(bytes as u64)));
}

// SAFETY: every method forwards its arguments unchanged to `System`, which
// upholds the `GlobalAlloc` contract; counting touches no allocator state.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller's contract is `System::alloc`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller's contract is `System::alloc_zeroed`'s.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        // SAFETY: the caller's contract is `System::realloc`'s.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is `System::dealloc`'s.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Bytes allocated on this thread while `gesture` runs.
fn work_of(gesture: impl FnOnce()) -> u64 {
    let before = ALLOCATED.with(Cell::get);
    gesture();
    ALLOCATED.with(Cell::get) - before
}

/// How many times the work may grow when the thing measured grows fourfold.
/// Work in proportion to it grows at most fourfold, work in proportion to its
/// square sixteenfold; six sits between the two.
const MOST_GROWTH: u64 = 6;

/// One frame of a mounted editor: the frame tick, then the paint.
fn frame(tree: &mut WidgetTree) {
    tree.request_frame();
    tree.tick_animations(Duration::from_millis(16));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
}

fn mounted(markdown: &str) -> (TextDocument, EditorHandle, WidgetTree) {
    let doc = TextDocument::new();
    doc.set_markdown(markdown).unwrap().wait().unwrap();
    let editor = RichTextEditor::editor(doc.clone());
    let handle = editor.handle();
    let mut tree = WidgetTree::new();
    let _ = tree.add(editor);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = tree.render();
    (doc, handle, tree)
}

/// Where the paragraph reading `text` starts.
fn paragraph(doc: &TextDocument, text: &str) -> usize {
    (0..doc.block_count())
        .filter_map(|n| doc.block_by_number(n))
        .find(|block| block.text() == text)
        .map(|block| block.position())
        .unwrap_or_else(|| panic!("no paragraph reads {text:?}"))
}

/// A paragraph, a table `rows` rows of five cells, and a paragraph.
fn around_a_table(rows: usize) -> String {
    let mut markdown =
        String::from("Before the table.\n\n| a | b | c | d | e |\n|---|---|---|---|---|\n");
    for row in 0..rows {
        markdown.push_str(&format!("| {row}a | {row}b | {row}c | {row}d | {row}e |\n"));
    }
    markdown.push_str("\nAfter the table.\n");
    markdown
}

/// `paragraphs` paragraphs with a small table in the middle of them.
fn a_long_text_holding_a_table(paragraphs: usize) -> String {
    let prose = |from: usize| -> String {
        (from..from + paragraphs / 2)
            .map(|i| format!("Paragraph {i} of the scene, with a few words in it.\n\n"))
            .collect()
    };
    format!(
        "{}| a1 | b1 |\n|---|---|\n| c1 | d1 |\n\n{}Last\n",
        prose(0),
        prose(paragraphs / 2)
    )
}

#[test]
fn a_frame_over_a_selected_table_costs_work_in_proportion_to_the_table() {
    let work = |rows: usize| {
        let (doc, handle, mut tree) = mounted(&around_a_table(rows));
        handle.select_range(
            paragraph(&doc, "Before the table.") + 3,
            paragraph(&doc, "After the table.") + 3,
        );
        frame(&mut tree);
        work_of(|| frame(&mut tree))
    };
    let (small, large) = (work(40), work(160));
    assert!(
        large < MOST_GROWTH * small,
        "{small} bytes a frame over a table of 40 rows and {large} over one of 160, \
         {:.1} times as much; work in proportion to the table is at most four times",
        large as f64 / small as f64,
    );
}

#[test]
fn a_frame_over_a_selection_of_a_long_text_does_not_walk_the_text() {
    let work = |paragraphs: usize| {
        let (_doc, handle, mut tree) = mounted(&a_long_text_holding_a_table(paragraphs));
        handle.select_all();
        frame(&mut tree);
        work_of(|| frame(&mut tree))
    };
    let (small, large) = (work(1_000), work(4_000));
    // Nothing in a frame that changes nothing needs to read the text past what
    // is on the screen: the work may not grow with the text at all. Half as
    // much again leaves room for what the frame's own bookkeeping varies by.
    assert!(
        2 * large < 3 * small,
        "{small} bytes a frame over 1000 paragraphs and {large} over 4000, \
         {:.1} times as much; a frame that repaints the same selection must not walk the text",
        large as f64 / small as f64,
    );
}

#[test]
fn extending_a_rectangle_of_cells_costs_work_in_proportion_to_the_table() {
    use teksilo_canvas::Point;
    use teksilo_core::event::{Key, Modifiers, PointerButton, WidgetEvent};

    let work = |rows: usize| {
        let (doc, handle, mut tree) = mounted(&around_a_table(rows));
        // Focus the editor, then take the whole table with Ctrl+A's third
        // press in its first cell.
        let corner = Point::new(20.0, 10.0);
        tree.dispatch_event(WidgetEvent::pointer_down(
            corner,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            corner,
            PointerButton::Primary,
            Modifiers::NONE,
        ));
        let first_cell = paragraph(&doc, "a") + 1;
        handle.select_range(first_cell, first_cell);
        let press = |tree: &mut WidgetTree, key: Key, modifiers: Modifiers| {
            tree.dispatch_event(WidgetEvent::KeyDown {
                key,
                modifiers,
                text: None,
            });
        };
        for _ in 0..3 {
            press(&mut tree, Key::A, Modifiers::COMMAND);
        }
        frame(&mut tree);
        work_of(|| press(&mut tree, Key::ArrowLeft, Modifiers::SHIFT))
    };
    let (small, large) = (work(40), work(160));
    assert!(
        large < MOST_GROWTH * small,
        "{small} bytes for Shift+Left over a table of 40 rows and {large} over one of 160, \
         {:.1} times as much; work in proportion to the table is at most four times",
        large as f64 / small as f64,
    );
}
