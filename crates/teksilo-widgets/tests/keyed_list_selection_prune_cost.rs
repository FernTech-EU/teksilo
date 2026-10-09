// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What dropping removed keys from a keyed `ListView` or `TableView`
//! selection costs as the view grows.
//!
//! A removal or a reset prunes the keys the source no longer holds. Asking the
//! source about one key was a scan of the whole source, made once per selected
//! key, so a removal from a select-all cost the row count squared. The prune
//! now gathers the source's keys once, as `GridView`'s does (see
//! `grid_view_keyed_selection_cost.rs`).
//!
//! A scan allocates nothing, so the work is counted in reads of the source's
//! keys rather than in time: a loaded machine cannot fail these tests and a
//! fast one cannot hide a regression.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Size, SizeProposal};
use teksilo_core::ObserverHandle;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_tree::WidgetTree;
use teksilo_data::{DataChange, KeyedSelectionModel, ListDataSource, ListModel, SelectionMode};
use teksilo_i18n::lit;
use teksilo_widgets::{CellContext, Column, ListView, TableView};

/// The two view sizes compared, in rows: one four times the other.
const SMALL: usize = 500;
const LARGE: usize = 4 * SMALL;

/// How many times the work may grow from `SMALL` to `LARGE`. Work in
/// proportion to the rows grows at most fourfold; work in proportion to their
/// square grows sixteenfold, and did. Six sits between the two.
const MOST_GROWTH: u64 = 6;

fn assert_grows_in_proportion(what: &str, work: impl Fn(usize) -> u64) {
    let (small, large) = (work(SMALL), work(LARGE));
    assert!(
        large < MOST_GROWTH * small,
        "{what}: {small} key reads over {SMALL} rows and {large} over {LARGE}, \
         {:.1} times as much; work in proportion to the rows is at most four times",
        large as f64 / small as f64,
    );
}

/// A row: 20 high at any width.
#[derive(Debug)]
struct Line;
impl Widget for Line {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(proposal.width.unwrap_or(100.0), 20.0).into()
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

/// Mount `view` over `count` rows, select every row, remove the first, and
/// return the keys the removal read.
fn removal_from_a_full_selection(
    count: usize,
    view: impl FnOnce(Counted, KeyedSelectionModel<u64>) -> Box<dyn Widget>,
) -> u64 {
    let items = ListModel::from_vec((0..count as u64).collect());
    let key_reads = Rc::new(Cell::new(0));
    let keyed = KeyedSelectionModel::new(SelectionMode::Multi);
    let mut tree = WidgetTree::new();
    tree.add_boxed(view(
        Counted {
            items: items.clone(),
            key_reads: key_reads.clone(),
        },
        keyed.clone(),
    ));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    keyed.select_keys(0..count as u64, false);
    key_reads.set(0);

    items.remove(0);

    assert_eq!(
        keyed.count(),
        count - 1,
        "the removed row's key must have left the selection, or the prune was \
         not measured"
    );
    key_reads.get()
}

#[test]
fn removing_a_row_from_a_full_list_view_selection_reads_keys_in_proportion() {
    assert_grows_in_proportion("a ListView removal", |count| {
        removal_from_a_full_selection(count, |source, keyed| {
            Box::new(ListView::from_source_keyed(
                source,
                keyed,
                |_i, _item, _selected| Box::new(Line),
            ))
        })
    });
}

#[test]
fn removing_a_row_from_a_full_table_view_selection_reads_keys_in_proportion() {
    assert_grows_in_proportion("a TableView removal", |count| {
        removal_from_a_full_selection(count, |source, keyed| {
            Box::new(
                TableView::from_source_keyed(source, keyed).add_column(Column::new(
                    "value",
                    lit!("Value"),
                    |_item: &u64, _: &CellContext| Box::new(Line),
                )),
            )
        })
    });
}
