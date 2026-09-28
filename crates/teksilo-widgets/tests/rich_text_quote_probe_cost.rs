// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a blockquote gesture costs before it acts, as the selection grows.
//!
//! Tab in a quote, `increase_blockquote_depth`, `toggle_blockquote` and the
//! default context menu's blockquote row all ask first how deep the deepest
//! quote under the selection sits, so a wrap never takes one past the nesting
//! ceiling. That question used to cost a document-wide lookup for every
//! selected paragraph, so the cost grew with the square of the selection: half
//! a second for a select-all over twenty thousand paragraphs, paid on every
//! right-click.
//!
//! These tests count work rather than time, so a loaded machine cannot fail
//! them and a fast one cannot hide a regression. The work is the bytes
//! allocated on the test's own thread while the gesture runs. A document-wide
//! lookup in the document model copies its table of frames, so its cost shows
//! up there in proportion to the document; a lookup per paragraph shows up as
//! the square.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::accesskit::Role;
use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_text::text_document::{MoveMode, TextDocument};
use teksilo_widgets::rich_text::RichTextEditor;

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

/// The two selection sizes compared, in paragraphs: one four times the other.
const SMALL: usize = 500;
const LARGE: usize = 4 * SMALL;

/// How many times the work may grow from `SMALL` to `LARGE`. Work in
/// proportion to the selection grows at most fourfold; work in proportion to
/// its square grows sixteenfold, and did, less the part of the cost that does
/// not depend on the selection. Six sits between the two.
const MOST_GROWTH: u64 = 6;

fn assert_grows_in_proportion(gesture: &str, work: impl Fn(usize) -> u64) {
    let (small, large) = (work(SMALL), work(LARGE));
    assert!(
        large < MOST_GROWTH * small,
        "{gesture}: {small} bytes over {SMALL} paragraphs and {large} over {LARGE}, \
         {:.1} times as much; work in proportion to the selection is at most four times",
        large as f64 / small as f64,
    );
}

/// `count` paragraphs, each line opened by `prefix` (`"> "` puts them in a
/// quote).
fn paragraphs(count: usize, prefix: &str) -> String {
    (0..count)
        .map(|i| format!("{prefix}Paragraph {i} of the scene, with a few words in it.\n{prefix}\n"))
        .collect()
}

/// `count` paragraphs with a quote in the middle of them, then `Last`.
fn paragraphs_around_a_quote(count: usize) -> TextDocument {
    let half = paragraphs(count / 2, "");
    load(&format!("{half}> Quoted\n\n{half}Last\n"))
}

/// A quote holding `count` paragraphs with a quote in the middle of them,
/// then `Last`.
fn a_quote_holding_paragraphs_around_a_quote(count: usize) -> TextDocument {
    let half = paragraphs(count / 2, "> ");
    load(&format!("{half}> > Inner\n>\n{half}> Last\n"))
}

fn load(djot: &str) -> TextDocument {
    let doc = TextDocument::new();
    doc.set_djot_sync(djot).unwrap();
    doc
}

fn length(doc: &TextDocument) -> usize {
    doc.to_plain_text().unwrap().chars().count()
}

fn quote_depth_at(doc: &TextDocument, position: usize) -> usize {
    let probe = doc.cursor();
    probe.set_position(position, MoveMode::MoveAnchor);
    probe.blockquote_depth_at_cursor()
}

#[test]
fn toggling_a_quote_over_a_selection_costs_work_in_proportion_to_it() {
    assert_grows_in_proportion("toggle_blockquote", |count| {
        let doc = paragraphs_around_a_quote(count);
        let editor = RichTextEditor::editor(doc.clone());
        editor.select_range(0, length(&doc));

        let work = work_of(|| editor.toggle_blockquote());

        assert_eq!(
            quote_depth_at(&doc, 0),
            1,
            "the toggle must have wrapped the selection, or its cost was not measured"
        );
        work
    });
}

#[test]
fn tab_in_a_quote_over_a_selection_costs_work_in_proportion_to_it() {
    // `increase_blockquote_depth` is the command behind Tab in a quote, and
    // asks the same question before it wraps.
    assert_grows_in_proportion("increase_blockquote_depth", |count| {
        let doc = a_quote_holding_paragraphs_around_a_quote(count);
        let editor = RichTextEditor::editor(doc.clone());
        editor.select_range(0, length(&doc));

        let work = work_of(|| editor.increase_blockquote_depth());

        assert_eq!(
            quote_depth_at(&doc, 0),
            2,
            "the command must have wrapped the selection, or its cost was not measured"
        );
        work
    });
}

#[test]
fn right_clicking_a_selection_costs_work_in_proportion_to_it() {
    // The default context menu greys its blockquote row out by the same
    // question, and asks it on every right-click.
    assert_grows_in_proportion("the context menu", |count| {
        let doc = paragraphs_around_a_quote(count);
        let editor = RichTextEditor::editor(doc);
        let handle = editor.handle();
        let mut tree = WidgetTree::new();
        let _ = tree.add(editor);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let _ = tree.render();
        handle.select_all();
        let _ = tree.render();

        // Inside the selection, so the right-click keeps it.
        let work = work_of(|| {
            tree.dispatch_event(WidgetEvent::pointer_down(
                Point::new(30.0, 10.0),
                PointerButton::Secondary,
                Modifiers::NONE,
            ));
        });

        tree.layout(SizeProposal::exact(400.0, 300.0));
        let update = tree.sync_accessibility();
        let row = update
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == Role::MenuItem && node.label() == Some("Toggle blockquote")
            })
            .map(|(_, node)| node);
        assert!(
            row.is_some_and(|node| !node.is_disabled()),
            "the menu must offer the toggle over the selection, or its cost was not measured"
        );
        work
    });
}
