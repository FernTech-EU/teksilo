// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`SceneListAdapter`] — keep lightweight scene items in sync with a
//! `teksilo_data` list model or data source.
//!
//! A scene's lightweight tier ([`SceneItem`]) has no arena-backed identity
//! and no built-in notion of "one item per row of some data collection" —
//! unlike `ListView`/`TableView`, which rebuild their child widgets from a
//! `ListModel<T>` / `ListDataSource<Item = T>` automatically. `SceneListAdapter`
//! is the scene-tier equivalent: give it a data source and a delegate
//! (`Fn(&T, usize) -> Box<dyn SceneItem>`), and it materialises one scene
//! item per row, then reconciles them whenever the source changes.
//!
//! `SceneListAdapter` is a **plain struct**, not a `Widget` — it owns no
//! arena node. Build one from a handler or `build()`, keep it alive for as
//! long as you want the items tracked (typically stashed in the owning
//! widget), and it does its work purely through [`SceneModel`] mutations and
//! a `teksilo_data` change observer.
//!
//! ## Delegate contract
//!
//! The delegate's return value — a `Box<dyn SceneItem>` — carries its own
//! **absolute scene position** via [`SceneItem::local_bounds`] (exactly like
//! any other item constructed with `RectItem::new(Rect::new(x, y, w, h))`
//! and typically placed at `Point::ZERO`). `SceneListAdapter` always inserts
//! the delegate's item at [`Point::ZERO`] via
//! [`SceneModel::add_boxed_item`] — it never re-positions the item. If rows
//! should be laid out (grid, list, freeform), the delegate itself computes
//! each row's `local_bounds` from its `index` (or from data on `T`) before
//! returning the boxed item.
//!
//! **The delegate must not mutate the source model.** It is invoked *inside*
//! the source's row-read (`ListModel::with_item`, which holds the model's
//! `RefCell` borrow across the callback), so calling `push` / `set` / `remove`
//! / `clear` on the same model from within the delegate panics with
//! `RefCell already borrowed`. Treat the delegate as a pure
//! `(&T, index) -> item` projection; drive data changes from outside it. (This
//! is the same contract `ListView`'s delegate has, for the same reason.)
//!
//! ## Reconciliation policy
//!
//! A lightweight item has no inherent identity beyond the id the Scene
//! mints for it — there is nothing to "patch" in place, only remove-and-add.
//! `SceneListAdapter` picks the simplest policy that is always correct:
//!
//! - **Structural changes** (insert / remove / move / reset — and a windowed
//!   source's `WindowLoaded`, see below) rebuild **every** item: every
//!   adapter-owned id is removed from the scene, then the current source
//!   is re-read start to finish and one item is built per row. This is
//!   O(n) but never leaks an item and never desyncs data-index → item
//!   mapping, even when the delegate's output depends on `index` (which
//!   shifts on insert/remove/move). Incremental insert/remove that spares
//!   unaffected rows is a possible future optimisation, not implemented here.
//! - **`ItemUpdated { index }`** (single-row content change, no structural
//!   shift) rebuilds only that one row: the old scene item is removed and a
//!   fresh one built from the current data at `index` replaces it.
//! - **`WindowLoaded { range }`** is treated as a structural change (full
//!   rebuild), not a per-row patch. A row for which
//!   [`ListDataSource::with_item`] returns `None` (not yet loaded) has *no*
//!   scene item at all — there is no adapter-agnostic placeholder item to
//!   substitute — so a partially-loaded window can only be positionally
//!   correct if data-index → adapter-slot alignment is rederived from
//!   scratch. Since `WindowLoaded` fires rarely (after a batch fetch, not
//!   per frame), the O(n) cost is a non-issue; internally the id table
//!   tracks unloaded rows as `None` slots so a later full rebuild always
//!   lands loaded rows back at their correct index.
//!
//! ## Borrow discipline
//!
//! Every reconciliation reads the source data (via the erased
//! `with_item_fn`, which takes its own short-lived borrow per row) and
//! builds every `Box<dyn SceneItem>` into a local `Vec` **first**, then
//! mutates the [`SceneModel`] (`remove` / `add_boxed_item`) only after all
//! reads are done. `SceneModel`'s mutators internally `borrow_mut` the
//! shared `RefCell<Scene>`; interleaving a read and a scene mutation inside
//! the same borrow would panic (or, worse, silently reenter) if the reader
//! and the mutator ever aliased the same `RefCell`. Mirrors `ListView`'s
//! "collect owned data, drop the borrow, then mutate" contract.
//!
//! ## Example
//!
//! ```ignore
//! use teksilo_data::ListModel;
//! use teksilo_scene::{RectItem, SceneListAdapter, SceneModel};
//! use teksilo_canvas::Rect;
//! use teksilo_tokens::Color;
//!
//! struct Card { x: f32, y: f32, color: Color }
//!
//! let scene = SceneModel::new();
//! let cards = ListModel::from_vec(vec![
//!     Card { x: 0.0, y: 0.0, color: Color::RED },
//!     Card { x: 140.0, y: 0.0, color: Color::BLUE },
//! ]);
//!
//! // Kept alive by the caller for as long as the sync should run.
//! let adapter = SceneListAdapter::from_model(&cards, scene.clone(), |card, _index| {
//!     Box::new(
//!         RectItem::new(Rect::new(card.x, card.y, 120.0, 80.0)).fill(card.color),
//!     )
//! });
//!
//! assert_eq!(adapter.len(), 2);
//! cards.push(Card { x: 280.0, y: 0.0, color: Color::GREEN });
//! assert_eq!(adapter.len(), 3);
//! ```

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

use teksilo_canvas::Point;
use teksilo_core::signal::ObserverHandle;
use teksilo_data::{DataChange, ListDataSource, ListModel};

use crate::item::{ItemId, SceneItem};
use crate::scene_model::SceneModel;

/// Erased `Fn(&T, usize) -> Box<dyn SceneItem>` delegate, shared between the
/// initial materialisation and every later reconciliation.
type Delegate<T> = Rc<dyn Fn(&T, usize) -> Box<dyn SceneItem>>;
/// Erased row-count reader over the underlying source.
type LenFn = Rc<dyn Fn() -> usize>;
/// Erased single-row reader: invokes the callback with `&T` if row `index`
/// is resident, otherwise does nothing (mirrors
/// [`ListDataSource::with_item`] returning `None`).
type WithItemFn<T> = Rc<dyn Fn(usize, &mut dyn FnMut(&T))>;
/// Per-data-index scene item id. `None` marks a row with no materialised
/// item yet (an unloaded row of a windowed [`ListDataSource`]).
type IdSlots = Rc<RefCell<Vec<Option<ItemId>>>>;

/// Keeps a set of lightweight [`SceneItem`]s in sync with a
/// `teksilo_data::ListModel<T>` / `ListDataSource<Item = T>`.
///
/// Not a `Widget` — a plain handle you construct once (typically from a
/// composing widget's `build()` or app setup code) and keep alive for as
/// long as the sync should run. See the module docs for the delegate
/// contract, reconciliation policy, and borrow discipline.
///
/// ## Dropping
///
/// Dropping a `SceneListAdapter` drops its [`ObserverHandle`], which stops
/// the adapter from reacting to further source changes. It deliberately
/// does **not** remove the adapter's items from the scene — running scene
/// mutations from inside a `Drop` impl risks a re-entrant borrow of the
/// shared `RefCell<Scene>` if the drop happens while some other code
/// already holds a borrow (e.g. mid-notification). Call
/// [`clear`](Self::clear) first if you want the items gone before dropping.
pub struct SceneListAdapter<T: 'static> {
    model: SceneModel,
    ids: IdSlots,
    _handle: ObserverHandle,
    _marker: PhantomData<T>,
}

impl<T: 'static> SceneListAdapter<T> {
    /// Track `model`'s rows as scene items in `scene`, built by `delegate`.
    ///
    /// Materialises every current row immediately (as if a [`DataChange::Reset`]
    /// had just fired), then keeps the scene in sync via
    /// [`ListModel::observe_changes`] for as long as the returned adapter is
    /// alive. See the module docs for the delegate contract and
    /// reconciliation policy.
    pub fn from_model(
        model: &ListModel<T>,
        scene: SceneModel,
        delegate: impl Fn(&T, usize) -> Box<dyn SceneItem> + 'static,
    ) -> Self {
        let len_model = model.clone();
        let item_model = model.clone();
        let observe_model = model.clone();
        let len_fn: LenFn = Rc::new(move || len_model.len());
        let with_item_fn: WithItemFn<T> = Rc::new(move |index, f| {
            item_model.with_item(index, |item| f(item));
        });
        Self::build_from(scene, len_fn, with_item_fn, delegate, move |cb| {
            observe_model.observe_changes(move |change| cb(change))
        })
    }

    /// Track an external [`ListDataSource`]'s rows as scene items in `scene`,
    /// built by `delegate`.
    ///
    /// Takes `source` as an `Rc<S>` (rather than by value) so the caller can
    /// keep its own handle to the same source alongside the adapter — the
    /// same convention as `ListView::from_source` / `TableView`'s erasure.
    /// See [`Self::from_model`] for the materialisation + reconciliation
    /// behaviour, which is identical for both constructors.
    pub fn from_source<S: ListDataSource<Item = T> + 'static>(
        source: Rc<S>,
        scene: SceneModel,
        delegate: impl Fn(&T, usize) -> Box<dyn SceneItem> + 'static,
    ) -> Self {
        let len_source = source.clone();
        let item_source = source.clone();
        let observe_source = source.clone();
        let len_fn: LenFn = Rc::new(move || len_source.len());
        let with_item_fn: WithItemFn<T> = Rc::new(move |index, f| {
            item_source.with_item(index, |item| f(item));
        });
        Self::build_from(scene, len_fn, with_item_fn, delegate, move |cb| {
            observe_source.observe_changes(move |change| cb(change))
        })
    }

    /// Shared construction path for both public constructors: materialise
    /// the current rows, then register the reconciling observer.
    fn build_from(
        scene: SceneModel,
        len_fn: LenFn,
        with_item_fn: WithItemFn<T>,
        delegate: impl Fn(&T, usize) -> Box<dyn SceneItem> + 'static,
        observe_register: impl FnOnce(Box<dyn Fn(&DataChange)>) -> ObserverHandle,
    ) -> Self {
        let delegate: Delegate<T> = Rc::new(delegate);
        let ids: IdSlots = Rc::new(RefCell::new(Vec::new()));

        // Materialise all current rows, as if a `Reset` had just fired.
        Self::rebuild_all(&scene, &ids, &len_fn, &with_item_fn, &delegate);

        let obs_scene = scene.clone();
        let obs_ids = ids.clone();
        let obs_len_fn = len_fn.clone();
        let obs_with_item_fn = with_item_fn.clone();
        let obs_delegate = delegate.clone();

        let handle = observe_register(Box::new(move |change| match change {
            // A single row's content changed in place — no structural shift,
            // so only that row needs a fresh item.
            DataChange::ItemUpdated { index } => {
                Self::rebuild_one(
                    &obs_scene,
                    &obs_ids,
                    *index,
                    &obs_with_item_fn,
                    &obs_delegate,
                );
            }
            // Every other variant either shifts indices (Inserted / Removed /
            // Moved), discards all state (Reset), or can only be applied
            // correctly by rederiving the whole data-index -> id mapping from
            // scratch (WindowLoaded — see the module docs). Rebuild-all is
            // always correct for all of these.
            DataChange::ItemsInserted { .. }
            | DataChange::ItemsRemoved { .. }
            | DataChange::ItemsMoved { .. }
            | DataChange::WindowLoaded { .. }
            | DataChange::Reset => {
                Self::rebuild_all(
                    &obs_scene,
                    &obs_ids,
                    &obs_len_fn,
                    &obs_with_item_fn,
                    &obs_delegate,
                );
            }
        }));

        Self {
            model: scene,
            ids,
            _handle: handle,
            _marker: PhantomData,
        }
    }

    /// Rebuild every adapter-owned scene item from the current source
    /// contents. Reads every resident row and builds its item *before*
    /// touching the scene (see the module docs' borrow-discipline section),
    /// then removes every previously-owned id and adds the freshly built
    /// ones in data order.
    fn rebuild_all(
        scene: &SceneModel,
        ids: &IdSlots,
        len_fn: &LenFn,
        with_item_fn: &WithItemFn<T>,
        delegate: &Delegate<T>,
    ) {
        let len = (len_fn)();
        let mut built: Vec<Option<Box<dyn SceneItem>>> = Vec::with_capacity(len);
        for index in 0..len {
            let mut out: Option<Box<dyn SceneItem>> = None;
            (with_item_fn)(index, &mut |item: &T| {
                out = Some((delegate)(item, index));
            });
            built.push(out);
        }

        // Drop every id this adapter currently owns before re-adding — the
        // borrow ends with `drain`/`collect`, well before any scene mutation.
        let old_ids: Vec<Option<ItemId>> = ids.borrow_mut().drain(..).collect();
        for id in old_ids.into_iter().flatten() {
            scene.remove(id);
        }

        let new_ids: Vec<Option<ItemId>> = built
            .into_iter()
            .map(|item| item.map(|item| scene.add_boxed_item(item, Point::ZERO)))
            .collect();
        *ids.borrow_mut() = new_ids;
    }

    /// Rebuild the single scene item at data `index`: remove the old one (if
    /// any) and, if the row is currently resident, add a fresh one built
    /// from the current data. No-op if `index` is outside the currently
    /// tracked slot count (defensive — a well-behaved source only emits
    /// `ItemUpdated`/`WindowLoaded` for in-range indices).
    fn rebuild_one(
        scene: &SceneModel,
        ids: &IdSlots,
        index: usize,
        with_item_fn: &WithItemFn<T>,
        delegate: &Delegate<T>,
    ) {
        let old_id = {
            let guard = ids.borrow();
            match guard.get(index) {
                Some(slot) => *slot,
                None => return,
            }
        };

        let mut built: Option<Box<dyn SceneItem>> = None;
        (with_item_fn)(index, &mut |item: &T| {
            built = Some((delegate)(item, index));
        });

        if let Some(old_id) = old_id {
            scene.remove(old_id);
        }
        let new_id = built.map(|item| scene.add_boxed_item(item, Point::ZERO));
        ids.borrow_mut()[index] = new_id;
    }

    /// The scene item id materialised for data row `index`, or `None` if
    /// `index` is out of range or the row has no materialised item (an
    /// unloaded row of a windowed source).
    pub fn item_id_at(&self, index: usize) -> Option<ItemId> {
        self.ids.borrow().get(index).copied().flatten()
    }

    /// All ids currently materialised by this adapter, in data order
    /// (rows with no materialised item are omitted, so this may be shorter
    /// than the source's row count).
    pub fn ids(&self) -> Vec<ItemId> {
        self.ids.borrow().iter().filter_map(|slot| *slot).collect()
    }

    /// Number of scene items this adapter currently owns.
    pub fn len(&self) -> usize {
        self.ids
            .borrow()
            .iter()
            .filter(|slot| slot.is_some())
            .count()
    }

    /// Whether this adapter currently owns no scene items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove every scene item this adapter owns from the scene and forget
    /// them. The adapter keeps observing the source afterward — a later
    /// source change re-materialises rows as usual.
    pub fn clear(&self) {
        let old_ids: Vec<Option<ItemId>> = self.ids.borrow_mut().drain(..).collect();
        for id in old_ids.into_iter().flatten() {
            self.model.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::RectItem;
    use teksilo_canvas::Rect;

    #[derive(Debug, Clone)]
    struct Row {
        n: i32,
    }

    fn row(n: i32) -> Row {
        Row { n }
    }

    fn delegate(row: &Row, _index: usize) -> Box<dyn SceneItem> {
        Box::new(RectItem::new(Rect::new(row.n as f32, 0.0, 10.0, 10.0)))
    }

    #[test]
    fn construction_materialises_current_rows() {
        let model = ListModel::from_vec(vec![row(1), row(2), row(3)]);
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_model(&model, scene.clone(), delegate);

        assert_eq!(adapter.len(), 3);
        assert_eq!(scene.len(), 3);
        assert!(adapter.item_id_at(0).is_some());
        assert!(adapter.item_id_at(1).is_some());
        assert!(adapter.item_id_at(2).is_some());
        assert!(adapter.item_id_at(3).is_none());
    }

    #[test]
    fn push_and_remove_track_len() {
        let model = ListModel::from_vec(vec![row(1), row(2)]);
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_model(&model, scene.clone(), delegate);
        assert_eq!(adapter.len(), 2);

        model.push(row(3));
        assert_eq!(adapter.len(), 3);
        assert_eq!(scene.len(), 3);

        model.remove(0);
        assert_eq!(adapter.len(), 2);
        assert_eq!(scene.len(), 2);
    }

    #[test]
    fn replace_all_rebuilds_with_fresh_ids() {
        let model = ListModel::from_vec(vec![row(1), row(2)]);
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_model(&model, scene.clone(), delegate);
        let before = adapter.ids();

        model.replace_all(vec![row(10), row(20), row(30)]);

        assert_eq!(adapter.len(), 3);
        assert_eq!(scene.len(), 3);
        let after = adapter.ids();
        assert_eq!(after.len(), 3);
        // Every id is fresh — none of the old ones survive a Reset rebuild.
        for id in &after {
            assert!(!before.contains(id));
        }
    }

    #[test]
    fn set_rebuilds_only_the_updated_row() {
        let model = ListModel::from_vec(vec![row(1), row(2), row(3)]);
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_model(&model, scene.clone(), delegate);

        let id0_before = adapter.item_id_at(0).unwrap();
        let id1_before = adapter.item_id_at(1).unwrap();
        let id2_before = adapter.item_id_at(2).unwrap();

        model.set(1, row(99));

        assert_eq!(adapter.len(), 3);
        assert_eq!(scene.len(), 3);
        assert_eq!(adapter.item_id_at(0).unwrap(), id0_before);
        assert_ne!(adapter.item_id_at(1).unwrap(), id1_before);
        assert_eq!(adapter.item_id_at(2).unwrap(), id2_before);
    }

    #[test]
    fn clear_removes_all_adapter_items_from_the_scene() {
        let model = ListModel::from_vec(vec![row(1), row(2), row(3)]);
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_model(&model, scene.clone(), delegate);
        assert_eq!(scene.len(), 3);

        adapter.clear();

        assert_eq!(adapter.len(), 0);
        assert!(adapter.is_empty());
        assert_eq!(scene.len(), 0);
    }

    #[test]
    fn dropping_the_adapter_stops_observing() {
        let model = ListModel::from_vec(vec![row(1), row(2)]);
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_model(&model, scene.clone(), delegate);
        assert_eq!(scene.len(), 2);

        drop(adapter);

        model.push(row(3));
        // No live adapter to react — the scene is untouched by the push.
        assert_eq!(scene.len(), 2);
    }

    #[test]
    fn from_source_tracks_a_list_data_source() {
        let model = ListModel::from_vec(vec![row(1), row(2)]);
        let source = Rc::new(model.clone());
        let scene = SceneModel::new();
        let adapter = SceneListAdapter::from_source(source, scene.clone(), delegate);

        assert_eq!(adapter.len(), 2);
        assert_eq!(scene.len(), 2);

        model.push(row(3));
        assert_eq!(adapter.len(), 3);
        assert_eq!(scene.len(), 3);
    }
}
