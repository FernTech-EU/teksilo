// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a keyed `GridView` selection costs as it grows.
//!
//! A keyed selection is asked about positions: which tiles are selected, in
//! order, for `on_selection_changed`, for the keyboard's cursor when it has
//! none, for a drag, for a reveal. Answering that asked the keyed model about
//! every position, and each question copied the model's whole set of keys, so
//! the answer cost the item count times the selection: a select-all over ten
//! thousand tiles was some hundred million hash operations. Dropping the keys
//! a removal or a reset left behind scanned the whole source once per selected
//! key, which is the same square.
//!
//! These tests count work rather than time, so a loaded machine cannot fail
//! them and a fast one cannot hide a regression. Copying a set of keys
//! allocates, so the first half shows up in the bytes allocated on the test's
//! own thread; a scan of the source allocates nothing, so the second half is
//! counted in reads of the source's keys.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Size, SizeProposal};
use teksilo_core::ObserverHandle;
use teksilo_core::event::{Key, Modifiers, WidgetEvent};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{DataChange, KeyedSelectionModel, ListDataSource, ListModel, SelectionMode};
use teksilo_widgets::GridView;

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
fn bytes_of(gesture: impl FnOnce()) -> u64 {
    let before = ALLOCATED.with(Cell::get);
    gesture();
    ALLOCATED.with(Cell::get) - before
}

/// The two grid sizes compared, in tiles: one four times the other.
const SMALL: usize = 500;
const LARGE: usize = 4 * SMALL;

/// How many times the work may grow from `SMALL` to `LARGE`. Work in
/// proportion to the grid grows at most fourfold; work in proportion to its
/// square grows sixteenfold, and did. Six sits between the two.
const MOST_GROWTH: u64 = 6;

fn assert_grows_in_proportion(what: &str, unit: &str, work: impl Fn(usize) -> u64) {
    let (small, large) = (work(SMALL), work(LARGE));
    assert!(
        large < MOST_GROWTH * small,
        "{what}: {small} {unit} over {SMALL} tiles and {large} over {LARGE}, \
         {:.1} times as much; work in proportion to the grid is at most four times",
        large as f64 / small as f64,
    );
}

/// A tile: 100 x 50 at any proposal.
#[derive(Debug)]
struct Tile;
impl Widget for Tile {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(100.0, 50.0).into()
    }
}

/// Items keyed by their own value, counting how often a key is read.
struct Counted {
    items: ListModel<u64>,
    key_reads: Rc<Cell<u64>>,
}

impl ListDataSource for Counted {
    type Item = u64;
    type Key = u64;

    fn len(&self) -> usize {
        self.items.len()
    }

    fn with_item<R>(&self, index: usize, f: impl FnOnce(&u64) -> R) -> Option<R> {
        self.items.with_item(index, f)
    }

    fn key_at(&self, index: usize) -> Option<u64> {
        self.key_reads.set(self.key_reads.get() + 1);
        self.items.with_item(index, |item| *item)
    }

    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle {
        self.items.observe_changes(f)
    }
}

struct Fixture {
    tree: WidgetTree,
    grid: WidgetId,
    items: ListModel<u64>,
    keyed: KeyedSelectionModel<u64>,
    key_reads: Rc<Cell<u64>>,
}

/// `count` tiles in a multiple selection that reports its positions, laid
/// out at three columns.
fn grid(count: usize) -> Fixture {
    let items = ListModel::from_vec((0..count as u64).collect());
    let key_reads = Rc::new(Cell::new(0));
    let keyed = KeyedSelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new();
    let grid = tree.add(
        GridView::from_source_keyed(
            Counted {
                items: items.clone(),
                key_reads: key_reads.clone(),
            },
            keyed.clone(),
            |_tc| Box::new(Tile),
        )
        .tile_size(100.0, 50.0)
        .on_selection_changed(|_positions| {}),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    Fixture {
        tree,
        grid,
        items,
        keyed,
        key_reads,
    }
}

fn key(tree: &mut WidgetTree, key: Key, modifiers: Modifiers) {
    tree.dispatch_event(WidgetEvent::KeyDown {
        key,
        modifiers,
        text: None,
    });
}

#[test]
fn selecting_every_tile_costs_work_in_proportion_to_the_grid() {
    assert_grows_in_proportion("select all", "bytes", |count| {
        let mut f = grid(count);
        f.tree.focus(f.grid);

        let work = bytes_of(|| key(&mut f.tree, Key::A, Modifiers::COMMAND));

        assert_eq!(
            f.keyed.count(),
            count,
            "the chord must have selected every tile, or its cost was not measured"
        );
        work
    });
}

#[test]
fn a_key_with_no_cursor_costs_work_in_proportion_to_the_grid() {
    // With no cursor, a key steps from the first selected tile, which the
    // grid finds by asking for the selected positions in order: the state a
    // selection made from outside leaves it in.
    assert_grows_in_proportion("an arrow with no cursor", "bytes", |count| {
        let mut f = grid(count);
        f.keyed.select_keys(0..count as u64, false);
        f.tree.focus(f.grid);

        let work = bytes_of(|| key(&mut f.tree, Key::ArrowRight, Modifiers::NONE));

        assert_eq!(
            f.keyed.selected_keys(),
            vec![1],
            "the arrow must have stepped from tile 0, or its cost was not measured"
        );
        work
    });
}

#[test]
fn removing_a_tile_from_a_full_selection_reads_keys_in_proportion_to_the_grid() {
    assert_grows_in_proportion("a removal", "key reads", |count| {
        let f = grid(count);
        f.keyed.select_keys(0..count as u64, false);
        f.key_reads.set(0);

        f.items.remove(0);

        assert_eq!(
            f.keyed.count(),
            count - 1,
            "the removed tile's key must have left the selection, or the prune \
             was not measured"
        );
        f.key_reads.get()
    });
}
