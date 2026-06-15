<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Reactive Data Models

**Companion to:** [architecture.md](architecture.md)
**Scope:** The `bastyde-data` crate — `ListModel`, `TreeModel`, `TreeSlice`, `SelectionModel`, `CheckedModel`, `TreeCheckedModel`, `CheckState`, `ListDataSource`, and the change-notification enums that connect them to data-driven widgets (`ListView`, `TreeView`, `Repeater`).

---

## 1. Why bastyde-data is its own crate

Data models sit *above* the widget tree conceptually: a `ListModel<String>` has no idea whether it is rendered by a `ListView`, a `Repeater`, or pretty-printed to stdout. Keeping them in their own crate enforces that separation in the dependency graph.

- `bastyde-core` → widgets, layout, events — the retained-tree infrastructure.
- `bastyde-data` → reactive collections — depends on `bastyde-core` only for `Signal<T>` and `ObserverHandle` (utility plumbing).
- `bastyde-widgets` → depends on both and consumes bastyde-data through its widgets.

Application code that wants to share a `ListModel<Project>` between a Bastyde view and, say, a headless validation pipeline can depend on `bastyde-data` without pulling in the renderer. The Qleany Clean-Architecture consumer is the main beneficiary: a domain layer holds its entity collections as `ListModel<Entity>`, a view-model layer observes and transforms, and the view layer binds a `ListView` to the result. See §6 on MVVM below.

Cloning any bastyde-data handle produces a second handle to the same underlying data. There is no deep-copy semantics and no ownership complication — the models are `Rc<RefCell<…>>` inside, so clones cost two pointer copies and all see the same items.

## 2. `ListModel<T>` — the common case

`ListModel<T>` is a concrete reactive list: a `Vec<T>` plus an observer list, behind an `Rc<RefCell<…>>`. Every mutation method (`push`, `insert`, `remove`, `set`, `move_item`, `replace_all`, `clear`) drops the mutable borrow *before* notifying observers, so a callback that reads `len()` or `with_item(...)` during the notification does not deadlock the cell.

```rust
use bastyde_data::{ListModel, DataChange};

let projects: ListModel<Project> = ListModel::new();
projects.push(Project::new("Widget Catalog"));
projects.insert(0, Project::new("Onboarding Tutorial"));

// Observe changes:
let _handle = projects.observe_changes(|change| match change {
    DataChange::ItemsInserted { range } => println!("{} item(s) inserted at {}", range.len(), range.start),
    DataChange::ItemsRemoved  { range } => println!("{} item(s) removed at {}",  range.len(), range.start),
    DataChange::ItemsMoved    { from, to, count } => println!("moved {count} items from {from} to {to}"),
    DataChange::ItemUpdated   { index } => println!("item {index} updated"),
    DataChange::Reset => println!("list reset"),
});
```

`with_item(index, f)` is the item-access primitive — a callback-taking accessor rather than a reference return, so the `RefCell` borrow is scoped to the callback lifetime and can't escape. This is the same pattern `Signal<T>::with(f)` uses.

Observation returns an `ObserverHandle` — drop the handle to unsubscribe. Widgets that observe a `ListModel` in their `build()` typically store the handle on `self` so it lives as long as the widget. `ctx.effect(...)` wraps this pattern.

## 3. `ListDataSource` — the escape hatch

`ListModel<T>` holds items in memory. A database-paged view, a filesystem directory listing, or a 10-GB log file doesn't fit that model. `ListDataSource` is a trait for implementors that own the data in some other form and emit `DataChange` notifications manually:

```rust
pub trait ListDataSource: 'static {
    type Item: 'static;
    fn len(&self) -> usize;
    fn with_item<R>(&self, index: usize, f: impl FnOnce(&Self::Item) -> R) -> Option<R>;
    fn observe_changes(&self, f: impl Fn(&DataChange) + 'static) -> ObserverHandle;
}
```

`ListDataSource` is *not* related to `ListModel<T>` by inheritance — they are two separate input paths. `ListView` provides both `ListView::new(model, delegate)` and `ListView::from_source(source, delegate)` constructors to consume either.

The trait is not object-safe (associated type + generic methods), which is deliberate: widgets consume it generically. Implementors are free to keep internal locks, LRU caches, or network state across calls; `with_item` passing the item by callback rather than reference means the implementor controls the borrow lifetime.

Anything that can be presented as "an indexed sequence with change notifications" fits here. Database cursor: implement `len()` as the cached page count, `with_item(i, f)` as "fetch if not cached, else look up," `observe_changes` as "forward the DB replication stream after translating to `DataChange`." Directory listing: `len()` is `readdir()` result length, `observe_changes` is a file-watcher backed stream.

## 4. `TreeModel<T>` + `TreeSlice<T>` — hierarchies with independent views

### 4.1 `TreeModel<T>`

The tree equivalent of `ListModel<T>`. Nodes are stored in a `SlotMap<DefaultKey, TreeNode<T>>`; each node carries its data, a parent reference, and a `Vec<NodeId>` of children. `NodeId` is an opaque handle that is **stable across mutations** — inserting or removing other nodes does not invalidate existing handles. This is what lets a view (see TreeSlice below) remember an expanded-set of node IDs across a model mutation.

```rust
use bastyde_data::{TreeModel, NodeId};

let fs: TreeModel<FsEntry> = TreeModel::new();
let docs: NodeId = fs.insert_root(0, FsEntry::dir("docs"));
let readme: NodeId = fs.insert_child(docs, 0, FsEntry::file("README.md"));
let inner: NodeId = fs.insert_child(docs, 1, FsEntry::dir("inner"));
```

Mutations emit `TreeChange::{NodeInserted, NodeRemoved, NodeMoved, NodeUpdated, Reset}`. `NodeRemoved` removes the entire subtree; observers see a single event but the subtree is gone.

### 4.2 `TreeSlice<T>` — per-view flattening

A `TreeModel<T>` describes a hierarchy but does not decide what's expanded, what's visible, or how it lays out. That's the `TreeView`'s (or any other consumer's) choice. `TreeSlice<T>` is the bridge:

- Owns an `expanded: HashSet<NodeId>` — the set of nodes whose children are shown.
- Maintains a flat `Vec<FlatEntry>` of the currently-visible nodes with depth information.
- Re-flattens on every `TreeChange` from the underlying model.
- Publishes a `version: Signal<u64>` that bumps on each re-flatten — consumers bind to this signal to know when to repaint.

```rust
pub struct FlatEntry {
    pub node_id: NodeId,
    pub depth: usize,          // 0 for roots
    pub has_children: bool,    // whether the node has any children in the model
    pub is_expanded: bool,     // whether this slice shows them
}
```

Two `TreeView` widgets bound to the same `TreeModel` each hold their own `TreeSlice`, so each has its own independent expand state. Opening a folder in one does not open it in the other. This matters for dual-pane file managers, for hierarchical search results shown alongside a full tree, and for any "overview" pane.

Consumers access entries via `slice.with_entry(index, |data, entry| …)` and get a reactive rebuild signal via `slice.version_signal()`.

### 4.3 `TreeSliceHandle` — the consumer API

`TreeView` doesn't re-implement expand/collapse; it holds a `TreeSliceHandle` and calls `toggle_expand(node_id)`, `expand(node_id)`, `collapse(node_id)`. The handle is `Clone` — widgets can share access to the same slice without ambient state. (`expand_all()` and `collapse_all()` are available on the owning `TreeSlice` itself.)

## 5. `SelectionModel` — one rule set, two widgets

Both `ListView` and `TreeView` share selection semantics, so selection lives in its own type in bastyde-data:

```rust
pub enum SelectionMode { None, Single, Multi }

pub struct SelectionModel {
    mode: SelectionMode,
    selection: Signal<BTreeSet<usize>>,  // indices into the flat view
    anchor: Rc<Cell<Option<usize>>>,      // for Shift+click range extend
}
```

The selection exposes a `Signal<BTreeSet<usize>>` (via `selection_signal()`), so any widget can bind to it and repaint on selection change without manual subscription. Methods:

- `select(index)` — replace selection with a single item; set anchor.
- `toggle(index)` — Ctrl+click: add or remove. In `Single` mode, degrades to `select`.
- `extend_to(index)` — Shift+click: select the range from anchor to index, keeping anchor.
- `select_all(range)` / `clear()` — bulk ops.

In `None` mode every operation is a no-op; widgets can construct a disabled selection model when selection doesn't apply (a toolbar's action list, for instance).

The selection is stored as flat indices into the view. For a `TreeView`, those are indices into the `TreeSlice`'s flat list — which means expanding or collapsing a parent changes which `NodeId`s those indices correspond to. Widgets translate at interaction time (e.g., on Ctrl+click): take the clicked `FlatEntry.node_id`, find its current flat index via the slice, then call `selection.toggle(index)`. Alternative designs where selection stores `NodeId`s directly have their own trade-offs (expansion doesn't lose selection, but the signal type changes per-widget); keeping selection index-based keeps the type uniform.

## 6. `CheckedModel` and `TreeCheckedModel` — per-row checkbox state

Selection (where the cursor is) and checkedness (which rows are *marked*) are orthogonal axes — Outlook / Files-app convention. So checkbox state lives in its own type pair, parallel to `SelectionModel`:

```rust
// Flat-list checkbox state.
pub struct CheckedModel {
    checked: Signal<BTreeSet<usize>>,
    per_index: Rc<RefCell<HashMap<usize, Signal<bool>>>>,
}

impl CheckedModel {
    pub fn new() -> Self;
    pub fn signal_for(&self, index: usize) -> Signal<bool>;   // shared per index
    pub fn checked_indices(&self) -> Vec<usize>;
    pub fn check(&self, index: usize);
    pub fn uncheck(&self, index: usize);
    pub fn toggle(&self, index: usize);
    pub fn check_all(&self, count: usize);
    pub fn clear(&self);
}

// Tree checkbox state with optional descendant→ancestor aggregation.
pub enum AggregateMode { None, DescendantsDriveAncestors }

pub struct TreeCheckedModel<T: 'static> { /* per-NodeId Signal<CheckState> */ }

impl<T: 'static> TreeCheckedModel<T> {
    pub fn new(tree: TreeModel<T>) -> Self;
    pub fn with_mode(tree: TreeModel<T>, mode: AggregateMode) -> Self;
    pub fn signal_for(&self, node: NodeId) -> Signal<CheckState>;  // tristate per node
    pub fn check(&self, node: NodeId);
    pub fn uncheck(&self, node: NodeId);
    pub fn toggle(&self, node: NodeId);
    pub fn checked_nodes(&self) -> Vec<NodeId>;                    // Checked only — Indeterminate excluded
    pub fn aggregate_mode(&self) -> AggregateMode;
    pub fn set_aggregate_mode(&self, mode: AggregateMode);
}
```

`signal_for(...)` is **cached per key**: repeat calls with the same index/`NodeId` return signals sharing the same root, so widgets bound to it re-render whenever the model is mutated through any other accessor.

`TreeCheckedModel` defaults to `DescendantsDriveAncestors`: a parent's `CheckState` is `Checked` when all descendants are, `Unchecked` when none are, `Indeterminate` otherwise. Setting a parent cascades to all descendants. The `None` mode disables aggregation when nodes own their state independently.

The `CheckState` enum (`Unchecked | Checked | Indeterminate`) lives in `bastyde-data` (re-exported from `bastyde::widgets::CheckState` for convenience). The `Checkbox` widget consumes it via `Checkbox::tristate(Signal<CheckState>)`; `StandardListItem.tristate_checkbox(...)` and `StandardTreeItem.tristate_checkbox(...)` accept the same signal.

Wiring with the new row widgets:

```rust
let checks: CheckedModel = state.app_state();
ListView::new(model, move |idx, item, _sel| {
    Box::new(
        StandardListItem::new(lit!(&item.name))
            .checkbox(checks.signal_for(idx))
    )
})

let tree_checks: TreeCheckedModel<Item> = state.app_state();
TreeView::new_with_context(tree, move |item, entry, _sel, ctx| {
    Box::new(
        StandardTreeItem::new(lit!(&item.title))
            .from_entry(entry)
            .tristate_checkbox(tree_checks.signal_for(entry.node_id))
            .on_toggle_rc(ctx.toggle_callback())
    )
})
```

Path A (ad-hoc `Signal<bool>` stored on each row's view-model item) remains valid — it's the right answer for fixed dialog lists and small settings panels. Reach for `CheckedModel` / `TreeCheckedModel` once item types are domain models you don't want to retrofit a signal field onto.

## 7. MVVM flow

Typical data flow for a list-backed view:

```text
    Domain        ViewModel              View
    ──────        ─────────              ────
   Entities  ─►   ListModel<ProjectVM> ─► ListView ─► paint
      ▲                  │                   │
      │                  │ delegate          │ on_tap / on_drop
      │                  │ closure           │
      │                  ▼                   ▼
      │           Widget subtree        intent (enum variant)
      │           per item               │
      │                                  ▼
      └── apply command ◄───── Action::on_invoke
```

- The **domain** owns entities (rows in a DB, nodes in a file tree, whatever the app is actually about).
- The **view-model** layer maps entities to display types (`ProjectVM { title, subtitle, icon, status_color }`) and holds them in a `ListModel<ProjectVM>`. The view-model observes the domain; when a domain entity changes, it updates the model, which emits `DataChange`.
- The **view** is a `ListView` bound to the `ListModel<ProjectVM>` via a delegate closure. Item interaction fires typed intents (see [shortcut-intent-action.md](shortcut-intent-action.md)); an ancestor `Action::on_invoke` translates the intent into a domain command.

The view never mutates the model directly. The intent/action split means "what the user did" and "what the app does about it" are separable layers — testable independently, replaceable independently, reconfigurable via `Action::enabled_when`.

## 8. `Repeater` vs `ListView` — when to use which

The widget catalog provides two data-driven collection consumers; pick by the size and scroll behavior of the collection.

### `Repeater` — dynamic, non-virtualized

A `Repeater` takes a `ListModel<T>` and a delegate, creates one child subtree per item, and lives inside whatever container the widget tree nests it in. On `DataChange::ItemsInserted { range }` the delegate fires for each new item; on `ItemsRemoved` the corresponding subtrees are destroyed; on `ItemsMoved` children are reordered without recreation. On `ItemUpdated { index }` the current implementation destroys and recreates that item's subtree; a future optimization path re-uses the existing subtree by pushing updates through reactive bindings on the delegate's Signals (no structural mutation).

Use `Repeater` for **bounded collections where every item should produce a widget**:

- Toolbar button lists.
- Tab headers.
- Form fields generated from a schema.
- Chapter lists in a side panel (author has tens of chapters, not thousands).

`Repeater` does not scroll or clip — it produces siblings. Wrap it in a `ScrollArea` to get scrolling; every item is still laid out whether visible or not.

### `ListView` — virtualized, scrollable

`ListView` creates widget subtrees **only for the items currently in the viewport plus a small buffer**. A `ListModel<Row>` with 100,000 rows consumes ~20 widgets' worth of tree memory when rendered through a `ListView` fitting 15 rows on screen. On scroll, newly-visible items get subtrees built and departing items are destroyed.

Use `ListView` for **unbounded or large collections that scroll**:

- Database-driven row lists.
- File managers.
- Chat histories.
- Log viewers.

`ListView` accepts both `ListModel<T>` and `ListDataSource` via separate constructors. Selection is driven by a shared `SelectionModel`. Drag-and-drop (intra-widget reorder) produces insertion-line feedback and emits typed reorder commands; see §8.

## 9. Drag-and-drop integration

`ListView` and `TreeView` are drag sources and drop targets out of the box. Intra-widget reorder routes through `ListModel::move_item(from, to)` / `TreeModel::move_node(node, new_parent, new_index)`; the widget produces visual feedback (insertion lines, depth-tinted highlight on tree drop targets) and emits typed reorder intents. Cross-widget drag flows through `DragPayload`; external (OS) drops (files / text / URLs from another app) arrive as a `DragPayload` with `origin() == External` through the same handlers — see [architecture.md §14 Drag and Drop](architecture.md) and [drag-and-drop.md §11](drag-and-drop.md) for the full picture.

The relevant bastyde-data hook is the `DataChange::ItemsMoved { from, to, count }` / `TreeChange::NodeMoved { node, old_parent, new_parent, new_index }` notifications. The source widget emits the mutation on the model; every observer of the model — including other `ListView`s sharing the data — receives the notification and updates consistently.

## 10. Qleany and adjacent-app integration

For applications that already have a Clean Architecture split, bastyde-data sits naturally at the ViewModel layer:

- Qleany entities live in the domain crate, no bastyde dependency.
- The view-model crate depends on bastyde-data to publish entity collections as `ListModel<EntityVM>`.
- The view crate (widgets + windows) depends on bastyde-widgets and binds `ListView` / `TreeView` to those models.

The architecture doc's `EntityListModel` example shows the shape: a wrapper that observes a Qleany store, maps its entities through a presentation transform on change, and holds the result in a `ListModel<EntityVM>`. The widget side is unaware of Qleany.

Nothing in bastyde-data requires Qleany. An application that uses `diesel` + raw structs, or one that streams events off a Kafka topic, follows the same pattern with whatever domain-layer types it prefers.

## 11. Testing patterns

bastyde-data is headless. Tests hold models, mutate them, and assert observer callbacks received the right `DataChange` / `TreeChange`:

```rust
let model = ListModel::<String>::new();
let log = Rc::new(RefCell::new(Vec::<DataChange>::new()));
let log_c = log.clone();
let _handle = model.observe_changes(move |change| log_c.borrow_mut().push(change.clone()));

model.push("alice".to_string());
model.push("bob".to_string());
model.insert(0, "zoe".to_string());
model.remove(1);

let events = log.borrow().clone();
assert_eq!(events, vec![
    DataChange::ItemsInserted { range: 0..1 },
    DataChange::ItemsInserted { range: 1..2 },
    DataChange::ItemsInserted { range: 0..1 },
    DataChange::ItemsRemoved  { range: 1..2 },
]);
```

Widget-tree tests that want a representative model use `ListModel::from_vec(vec![...])` and pass the model clone to the `ListView` under test. Selection tests construct a `SelectionModel::new(SelectionMode::Multi)` and drive it with `select` / `toggle` / `extend_to` calls, asserting on `selection_signal().get()`.

## 12. Design rules in one list

- Models are `Rc<RefCell<…>>` internally and `Clone`-friendly. Share by cloning; there's no ownership transfer cost.
- Every mutation notifies observers *after* dropping the mutable borrow, so observer callbacks can freely read the model.
- Access items through callbacks (`with_item`, `with_entry`) rather than returning references. The `RefCell` borrow stays internal.
- `NodeId` is stable across mutations; index-based addressing is not (indices change when items insert or move). Store IDs, not indices, in long-lived state.
- Selection is a separate concern (`SelectionModel`) — not part of `ListModel` / `TreeModel`.
- `ListModel<T>` in memory, `ListDataSource` for external; pick one per view — they are not composable.
- `Repeater` for bounded non-scrollable collections, `ListView` for scrollable or large ones.
- Two `TreeView`s sharing a `TreeModel` get independent `TreeSlice`s; expand state is per-view.
- Mutations flow one-way: widget emits typed intent → `Action` translates → model mutates → change notification → widgets repaint. The widget never writes directly to the model.

## 13. Divergence reporting — `first_changed_index()`

The three projection layers — `TreeSlice`, `SortFilterTreeModel`, and
`SortFilterListModel` — rebuild their visible list wholesale on every
change, and (for the sort/filter proxies) notify observers with a
blanket `Reset`. That is the safe *notification* contract, but it
destroys information a consumer may need: which prefix of the visible
list is actually unchanged. The canonical consumer is variable-row-height
virtualization (`ListView` / `TreeView` / `TableView` / `TreeTable`
keep per-visible-row measured heights), but anything caching per-row
derived state can use it.

Each projection therefore computes, during the rebuild it already
performs, the **first visible index whose content may differ** and
exposes it as a side-channel:

```rust
proxy.first_changed_index() // -> Option<usize>
```

- `Some(d)` — rows `0..d` show the same items, in the same order, at
  the same depth/expand state as before the rebuild. `d == len()` means
  nothing visible changed.
- `None` — unknown (no rebuild observed yet); treat as a full change.

Semantics per type:

- **`TreeSlice` / `SortFilterTreeModel`** — prefix-compare of the old
  and new flat lists (`NodeId`s are stable, so equality means identity).
  An expand/collapse at flat index *k* reports *k* (the toggled row's
  own entry changed); a `NodeUpdated` with unchanged structure reports
  that node's flat index.
- **`SortFilterListModel`** — prefix-compare of the projected
  source-index map, with an *identity floor*: upstream inserts/removes/
  moves renumber source indices, so equal index values are only trusted
  below the change point. An `ItemUpdated` that didn't move under the
  sort reports that row's visible position; an append reports the old
  length.

The value describes the **latest** rebuild only and is overwritten by
the next one. Read it synchronously from a change observer
(`observe_changes` callbacks and `version_signal()` observers fire
inline on every rebuild, so per-change reads cannot miss a value). The
external `DataChange::Reset` contract of the proxies is unchanged.
`ListDataSource` carries a defaulted `first_changed_index()` (returning
`None`) so generic consumers reach the side-channel without downcasts.

---

## See also

- [architecture.md §6 UI Construction Patterns](architecture.md) — `Repeater` in context, static-vs-dynamic children.
- [architecture.md §14 Drag and Drop](architecture.md) — `DragPayload`, cross-widget reorder.
- [shortcut-intent-action.md](shortcut-intent-action.md) — typed intents, ancestor `Action`s, how the MVVM command layer lands in Rust.
- [crates/bastyde-data/src/list_model.rs](../crates/bastyde-data/src/list_model.rs), [tree_model.rs](../crates/bastyde-data/src/tree_model.rs), [tree_slice.rs](../crates/bastyde-data/src/tree_slice.rs), [selection_model.rs](../crates/bastyde-data/src/selection_model.rs), [list_data_source.rs](../crates/bastyde-data/src/list_data_source.rs).
- [crates/bastyde-data/src/data_change.rs](../crates/bastyde-data/src/data_change.rs), [tree_change.rs](../crates/bastyde-data/src/tree_change.rs).
- [examples/data_collections](../examples/data_collections/) — runnable demonstration of ListView, TreeView, Repeater, SelectionModel, and intra-widget DnD.
