// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a frame that changes nothing costs the accessibility tree, as the tree
//! grows.
//!
//! A focused text field blinks its caret twice a second, and every blink is a
//! frame. While an assistive technology is attached, each of those frames used
//! to copy the whole accessibility tree twice: once out of the cache, thrown
//! away by the event loop, and once more to hand the platform adapter what it
//! already held, which it then compared node by node. In the debug
//! `rich-text-editor` that was 5 to 7 ms of a caret blink.
//!
//! These tests count work rather than time, so a loaded machine cannot fail
//! them and a fast one cannot hide a regression: the bytes allocated on the
//! test's own thread while the frame's accessibility step runs, as the event
//! loop runs it. A copy of the tree shows up there in proportion to the tree.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accesskit::Role;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;

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

/// Bytes allocated on this thread while `step` runs.
fn work_of(step: impl FnOnce()) -> u64 {
    let before = ALLOCATED.with(Cell::get);
    step();
    ALLOCATED.with(Cell::get) - before
}

#[derive(Debug)]
struct Row;

impl Widget for Row {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(200.0, 20.0).into()
    }
}

/// A column of rows, each its own accessibility node.
#[derive(Debug)]
struct Column {
    rows: Vec<WidgetId>,
}

impl Widget for Column {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal
            .resolve(200.0, 20.0 * self.rows.len() as f32)
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for (index, child) in children.iter_mut().enumerate() {
            child.origin = Point::new(bounds.x, bounds.y + 20.0 * index as f32);
            child.size = Size::new(200.0, 20.0);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.rows.clone()
    }
}

/// A tree of `rows` labelled rows, laid out, its accessibility tree delivered
/// once to an adapter, as a window is after its first frames.
fn delivered_tree(rows: usize) -> WidgetTree {
    let mut tree = WidgetTree::new();
    let rows = (0..rows)
        .map(|index| {
            tree.add(
                Row.access_role(Role::Label)
                    .access_label(format!("Row {index}")),
            )
        })
        .collect();
    tree.add(Column { rows });
    tree.layout(SizeProposal::exact(200.0, 600.0));
    tree.refresh_accessibility();
    let _ = tree.deliver_accessibility(true);
    tree
}

/// The accessibility step of one frame that changes nothing, as `teksilo-app`
/// runs it with an assistive technology attached: refresh, and hand the
/// adapter what leaves its tree as it is.
fn idle_frame_work(tree: &mut WidgetTree) -> u64 {
    tree.layout(SizeProposal::exact(200.0, 600.0));
    work_of(|| {
        tree.refresh_accessibility();
        assert!(!tree.accessibility_delivery_owed(true), "an idle frame");
        let update = tree.accessibility_unchanged(true);
        assert!(update.nodes.is_empty());
    })
}

#[test]
fn an_idle_frame_costs_the_accessibility_tree_nothing_that_grows_with_it() {
    let mut small = delivered_tree(50);
    let mut large = delivered_tree(2_000);
    // The first idle frame after a delivery, then a steady one.
    let _ = idle_frame_work(&mut small);
    let _ = idle_frame_work(&mut large);
    let small_work = idle_frame_work(&mut small);
    let large_work = idle_frame_work(&mut large);
    // Forty times the nodes. A copy of the tree is in the tens of kilobytes
    // at 50 rows and the megabyte at 2,000; what stays is the window title
    // and the like.
    assert!(
        large_work <= small_work + 1_024,
        "an idle frame allocated {small_work} bytes over 50 rows and {large_work} over \
         2,000: something in it copies the tree"
    );
}
