<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneListAdapter

`SceneListAdapter` — keep lightweight scene items in sync with a
`teksilo_data` list model or data source.

A scene's lightweight tier (`SceneItem`) has no arena-backed identity
and no built-in notion of "one item per row of some data collection" —
unlike `ListView`/`TableView`, which rebuild their child widgets from a
`ListModel<T>` / `ListDataSource<Item = T>` automatically. `SceneListAdapter`
is the scene-tier equivalent: give it a data source and a delegate
(`Fn(&T, usize) -> Box<dyn SceneItem>`), and it materialises one scene
item per row, then reconciles them whenever the source changes.

`SceneListAdapter` is a **plain struct**, not a `Widget` — it owns no
arena node. Build one from a handler or `build()`, keep it alive for as
long as you want the items tracked (typically stashed in the owning
widget), and it does its work purely through `SceneModel` mutations and
a `teksilo_data` change observer.

## Delegate contract

The delegate's return value — a `Box<dyn SceneItem>` — carries its own
**absolute scene position** via `SceneItem::local_bounds` (exactly like
any other item constructed with `RectItem::new(Rect::new(x, y, w, h))`
and typically placed at `Point::ZERO`). `SceneListAdapter` always inserts
the delegate's item at `Point::ZERO` via
`SceneModel::add_boxed_item` — it never re-positions the item. If rows
should be laid out (grid, list, freeform), the delegate itself computes
each row's `local_bounds` from its `index` (or from data on `T`) before
returning the boxed item.

**The delegate must not mutate the source model.** It is invoked *inside*
the source's row-read (`ListModel::with_item`, which holds the model's
`RefCell` borrow across the callback), so calling `push` / `set` / `remove`
/ `clear` on the same model from within the delegate panics with
`RefCell already borrowed`. Treat the delegate as a pure
`(&T, index) -> item` projection; drive data changes from outside it. (This
is the same contract `ListView`'s delegate has, for the same reason.)

## Reconciliation policy — slot identity is preserved

An adapter row's identity is its **slot**: data index *i* owns one
`ItemId`, and that id survives a change to the row's content and a change
to the rows around it. It is retired only when the row itself goes away.

That matters because the id is what everything else keys on —
`SceneSelection`, magnets, logical-AT parenting,
and any side map the app keeps (a `SceneItem` has no downcast, so a side
map is the *only* way to reach item-specific state). An adapter that retired
and re-minted ids on every source change would take all of it with them; a
one-row edit would silently deselect the list.

So every reconciliation goes through `Scene::replace_item`, which swaps
the item box **inside** the existing entry — keeping the id, the z, the
layer, the parent, the flags, the handlers, the magnets and the AT
decorations — and emits one
`ItemChange::ItemReplaced` rather than a
removal and an insertion:

- **`ItemUpdated { index }`** (single-row content change, no structural
  shift) replaces that one row's item in place. No other row is touched.
- **Structural changes** (insert / remove / move / reset, and a windowed
  source's `WindowLoaded`) re-read the source start to finish and reconcile
  slot by slot: slot *i* keeps its id and takes the new content, a slot past
  the old length gets a fresh item, a slot past the new length is removed.
  The re-read is still O(n) — the delegate takes an `index`, and an insert
  at the front changes what every later row renders — but the **ids** are
  stable for every slot that still exists.

  Slot-positional rather than domain-keyed, because this adapter has no
  domain key to work from: its delegate is `Fn(&T, usize)` and `T` need not
  be identifiable. An adapter over a source with stable keys should follow
  `TreeDataSlice`'s pattern and key on the domain id.

- **`WindowLoaded { range }`** is a structural change for the same reason a
  reset is: a row for which `ListDataSource::with_item` returns `None`
  (not yet loaded) has *no* scene item at all — there is no
  adapter-agnostic placeholder to substitute — so a partially-loaded window
  can only be positionally correct if the whole data-index → slot alignment
  is rederived. The id table tracks unloaded rows as `None` slots, so a row
  that loads later lands at its correct index with a fresh id.

Every reconciliation runs inside one
`SceneTransaction`, stamped
`ChangeSource::Programmatic` — a data-model refresh is
not a user edit — so a data layer watching the scene sees one grouped change
per source change rather than N unrelated ones.

## Borrow discipline

Every reconciliation reads the source data (via the erased
`with_item_fn`, which takes its own short-lived borrow per row) and
builds every `Box<dyn SceneItem>` into a local `Vec` **first**, then
mutates the `SceneModel` (`replace_item` / `add_boxed_item` / `remove`)
only after all reads are done. `SceneModel`'s mutators internally `borrow_mut` the
shared `RefCell<Scene>`; interleaving a read and a scene mutation inside
the same borrow would panic (or, worse, silently reenter) if the reader
and the mutator ever aliased the same `RefCell`. Mirrors `ListView`'s
"collect owned data, drop the borrow, then mutate" contract.

## Example

```ignore
use teksilo_data::ListModel;
use teksilo_scene::{RectItem, SceneListAdapter, SceneModel};
use teksilo_canvas::Rect;
use teksilo_tokens::Color;

struct Card { x: f32, y: f32, color: Color }

let scene = SceneModel::new();
let cards = ListModel::from_vec(vec![
    Card { x: 0.0, y: 0.0, color: Color::RED },
    Card { x: 140.0, y: 0.0, color: Color::BLUE },
]);

// Kept alive by the caller for as long as the sync should run.
let adapter = SceneListAdapter::from_model(&cards, scene.clone(), |card, _index| {
    Box::new(
        RectItem::new(Rect::new(card.x, card.y, 120.0, 80.0)).fill(card.color),
    )
});

assert_eq!(adapter.len(), 2);
cards.push(Card { x: 280.0, y: 0.0, color: Color::GREEN });
assert_eq!(adapter.len(), 3);
```

## Builder methods at a glance

`from_model`, `from_source`, `item_id_at`, `ids`, `len`, `is_empty`, `clear`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/scene_list_adapter/index.html)

## `pub struct SceneListAdapter`

Keeps a set of lightweight `SceneItem`s in sync with a
`teksilo_data::ListModel<T>` / `ListDataSource<Item = T>`.

Not a `Widget` — a plain handle you construct once (typically from a
composing widget's `build()` or app setup code) and keep alive for as
long as the sync should run. See the module docs for the delegate
contract, reconciliation policy, and borrow discipline.

## Dropping

Dropping a `SceneListAdapter` drops its `ObserverHandle`, which stops
the adapter from reacting to further source changes. It deliberately
does **not** remove the adapter's items from the scene — running scene
mutations from inside a `Drop` impl risks a re-entrant borrow of the
shared `RefCell<Scene>` if the drop happens while some other code
already holds a borrow (e.g. mid-notification). Call
`clear` first if you want the items gone before dropping.

```rust
pub struct SceneListAdapter<T: 'static> { /* fields */ }
```

### Methods

#### `pub fn from_model( model: &ListModel<T>, scene: SceneModel, delegate: impl Fn(&T, usize) -> Box<dyn SceneItem> + 'static, ) -> Self`

Track `model`'s rows as scene items in `scene`, built by `delegate`.

Materialises every current row immediately (as if a `DataChange::Reset`
had just fired), then keeps the scene in sync via
`ListModel::observe_changes` for as long as the returned adapter is
alive. See the module docs for the delegate contract and
reconciliation policy.

#### `pub fn from_source<S: ListDataSource<Item = T> + 'static>( source: Rc<S>, scene: SceneModel, delegate: impl Fn(&T, usize) -> Box<dyn SceneItem> + 'static, ) -> Self`

Track an external `ListDataSource`'s rows as scene items in `scene`,
built by `delegate`.

Takes `source` as an `Rc<S>` (rather than by value) so the caller can
keep its own handle to the same source alongside the adapter — the
same convention as `ListView::from_source` / `TableView`'s erasure.
See `Self::from_model` for the materialisation + reconciliation
behaviour, which is identical for both constructors.

#### `pub fn item_id_at(&self, index: usize) -> Option<ItemId>`

The scene item id materialised for data row `index`, or `None` if
`index` is out of range or the row has no materialised item (an
unloaded row of a windowed source).

#### `pub fn ids(&self) -> Vec<ItemId>`

All ids currently materialised by this adapter, in data order
(rows with no materialised item are omitted, so this may be shorter
than the source's row count).

#### `pub fn len(&self) -> usize`

Number of scene items this adapter currently owns.

#### `pub fn is_empty(&self) -> bool`

Whether this adapter currently owns no scene items.

#### `pub fn clear(&self)`

Remove every scene item this adapter owns from the scene and forget
them. The adapter keeps observing the source afterward — a later
source change re-materialises rows as usual.
