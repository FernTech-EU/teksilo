<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Reactive Data Models

**Companion to:** [architecture.md](architecture.md), [charts.md](charts.md)
**Scope:** The `bastyde-data` crate — `ListModel`, `TreeModel`, `TreeSlice`, `TreeDataSlice`, `TreeRowFilter`, `SelectionModel`, `CheckedModel`, `TreeCheckedModel`, `KeyedTreeCheckedModel`, `CheckState`, `ListDataSource`, `TreeDataSource`, `ChartModel`, `ChartWindow`, `ChartAggregate`, `ChartSelection`, and the change-notification enums that connect them to data-driven widgets (`ListView`, `TreeView`, `Repeater`) and to `bastyde-charts` (`BarChart` / `LineChart` / `PieChart`).
**API reference:** the full rustdoc for every type lives at [`/api/bastyde_data/`](api/bastyde_data/index.html).

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

The tree side has grown a small family. They layer as **source → projection → view**: a *source* holds the tree, a *projection* turns it into the flat, expandable, per-view row list a `TreeView` reads (every projection implements `TreeDataSource`), and `TreeRowFilter` is a pre-transform that reshapes the rows before a `TreeDataSlice`. Pick by *where the tree lives* and *what you need on top*:

| Type | Layer | Data from | Key | Reach for it when |
| --- | --- | --- | --- | --- |
| **`TreeModel<T>`** (§4.1) | source | you build it in memory | `NodeId` | the tree lives in memory and you own it — the in-memory container |
| **`TreeSlice<T>`** (§4.2) | projection | wraps a `TreeModel` | `NodeId` | you need per-view expand + flatten over a `TreeModel` (the built-in `TreeView` source) |
| **`SortFilterTreeModel<T>`** (§13) | projection | wraps a `TreeModel` | `NodeId` | …and sort / tree-aware filter too; it owns *its own* expand state |
| **`TreeDataSlice<K, T>`** (§4.4) | projection | `Vec<TreeRow>` you supply (indent-ordered) | your domain `K` | the tree lives in an **external** store (Qleany / DB) as an outline — no `TreeModel` mirror |
| **`TreeRowFilter<K, T>`** (§4.5) | pre-transform | `Vec<TreeRow>` → `Vec<TreeRow>` | — | sort / filter the rows feeding a `TreeDataSlice` (wire into `set_source`; pair with `set_all_expanded` to reveal matches) |

Two orthogonal companions ride *alongside* a tree view rather than being sources themselves — pick the `NodeId` or domain-keyed variant to match your projection: **selection** — `SelectionModel` / `KeyedSelectionModel<K>` (§5); **checkboxes** — `TreeCheckedModel<T>` / `KeyedTreeCheckedModel<K>` (§6, §6.1). Rule of thumb: everything on a `TreeModel` is `NodeId`-keyed; everything on a `TreeDataSlice`/external source is domain-`K`-keyed.

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

### 4.4 `TreeDataSlice<K, T>` — the same, over an *external* tree

`TreeSlice` needs a `TreeModel` to wrap. When the tree's source of truth lives **outside** bastyde — a Qleany entity store, a database, a virtual filesystem — and you don't want to mirror it into a `TreeModel`, `TreeDataSlice<K, T>` gives the same machinery over your own data. It is the tree counterpart of the `ListDataSource` escape hatch (§3), but *ready-made* rather than a bare trait — it implements `TreeDataSource` for you.

You hand it the tree as a flat, **indent-ordered** row stream — the shape an outline is actually stored in (binders / chapters / scenes, OPML, Markdown headings) — and it derives the hierarchy:

```rust
use bastyde_data::{TreeDataSlice, TreeRow};

let slice: TreeDataSlice<EntityId, Row> = TreeDataSlice::new();
slice.set_expand_new_nodes(true);          // new nodes appear expanded
slice.set_source(move || load_rows());     // your `rows::load` -> Vec<TreeRow>
slice.reload();
// let view = TreeView::from_source(slice.clone(), delegate);
```

Each `TreeRow<K, T>` is `{ key, item, depth }` in document order. The engine derives every row's parent (the nearest preceding row of strictly smaller depth), its children, the roots (depth-0 rows), and its structural depth — then owns, exactly like `TreeSlice`:

- a per-view **expand set keyed by `K`** — so expand state (and keyed selection) survive a full re-source, which a `TreeModel` mirror can't guarantee because `NodeId`s are reassigned on rebuild;
- the collapse-aware **flatten** into visible rows;
- the **`version_signal()`** and **`first_changed_index()`** side-channel (§13);
- the DnD **cycle guard**.

Identity is *your domain key* `K` (an `i64` entity id, a tagged enum) — not a positional `NodeId`. Domain *policy* is injected as closures, so the mechanism stays in the slice and the meaning stays in your code:

| Setter | Purpose |
| --- | --- |
| `set_source(\|\| …)` | re-materialise the rows (called by `reload()` and after a drop) |
| `set_reorder(\|dragged, target, pos\| …)` | apply a move through the backend (with undo); returns whether it took |
| `set_drag_policy(\|key\| …)` | which rows may be dragged |
| `set_drop_resolver(\|dragged, target, target_item, pos\| …)` | domain drop rules; receives the hovered target's **item**, so it decides without capturing the slice (no `Rc` cycle). The cycle guard runs first. |

`T: PartialEq` is required: the divergence compares item *content*, so a re-source that changed only a row's text still narrows the height cache to that row.

**When to use which:** `TreeSlice` if the data lives in a `TreeModel`; `TreeDataSlice` if it lives in an external store as an indent-ordered outline. Implement `TreeDataSource` by hand only for a source that *isn't* a resolved indent sequence — a huge/lazy tree that pages children on demand (§14).

### 4.5 `TreeRowFilter<K, T>` — sort + filter for the `TreeDataSlice` pipeline

`SortFilterTreeModel` (§below, the `TreeModel`-backed sort/filter projection) owns its *own* expand state, so stacking it on a `TreeDataSlice` — which already has one — would give you two projections and two expand states. For an external tree, sort/filter belongs **below** the slice, on its raw indent-ordered input:

```text
rows::load()  →  TreeRowFilter::apply  →  TreeDataSlice::set_source  →  TreeView
              \___ Vec<TreeRow> → Vec<TreeRow> ___/     \___ the one projection ___/
```

`TreeRowFilter` is a pure `Vec<TreeRow<K, T>>` → `Vec<TreeRow<K, T>>` transform you build once and apply to each freshly-sourced stream (usually inside the `set_source` closure; re-apply to cached rows on a filter change to avoid re-querying the backend):

```rust
let sieve = TreeRowFilter::new()
    .filter_mode(TreeFilterMode::KeepAncestors)
    .filter(move |item: &Row| item.title.contains(&query))   // outline search
    .sort(|a: &Row, b: &Row| a.title.cmp(&b.title));
slice.set_source(move || sieve.apply(rows::load()));
slice.reload();
slice.set_all_expanded(true);   // reveal the whole filtered result (see below)
```

`TreeRowFilter` reshapes the *rows* but not the slice's per-view expand state, so `KeepAncestors` keeps the ancestor rows without expanding them — the matches would sit hidden under collapsed ancestors. While a filter is active, call **`slice.set_all_expanded(true)`** to reveal the narrowed result, and `set_all_expanded(false)` when it clears; the user's persistent collapse state is preserved underneath (it's a display override, not a mutation of the expand set).

It reuses the three `TreeFilterMode` strategies and sorts siblings per parent, then re-emits a valid indent-ordered stream (survivors' depths compact onto their nearest surviving ancestor, which `TreeDataSlice` re-derives). Two mode details worth knowing: **`HideNonMatching`** keeps a node only if it *and every ancestor* match (children of a hidden parent stay hidden), and **`KeepDescendants`** surfaces a matching subtree even when the match's own ancestors don't match — deliberately unlike `SortFilterTreeModel`'s flatten, which drops such a match. **`KeepAncestors`** (show the path to each match) is the usual outline-search mode.

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

### 6.1 `KeyedTreeCheckedModel<K>` — the same, for an external tree

`TreeCheckedModel` is bound to a `TreeModel` and keyed by `NodeId`. `KeyedTreeCheckedModel<K>` is its **domain-keyed** twin — the checkbox counterpart of `KeyedSelectionModel` (§5) — for the "select scenes to export" tristate over a `TreeDataSlice` / any `TreeDataSource`. It takes the tree *shape* as two injected closures (`children` + `parent`), so `from_source(slice.clone())` wires it to a slice with no `TreeModel` to mirror:

```rust
let checked = KeyedTreeCheckedModel::from_source(outline_slice.clone());
// bind each row's checkbox to `checked.signal_for(key)` / `.bool_signal_for(key)`;
// read the result with `checked.checked_keys()`.
```

Because state is keyed by your stable domain id, a checked node survives a full re-source. After a reload call **`prune_missing(|k| source.contains_key(k))`** (drops deleted nodes' state *and* recomputes the ancestors they affected) or **`reaggregate()`** (recompute every parent from the new shape) so the tristates stay correct across structural changes. Same cascade / `Signal<CheckState>` ↔ `Signal<bool>` bridge / `AggregateMode` as `TreeCheckedModel`.

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
- `ChartModel<T>` (§15) follows the same `Rc<RefCell<…>>` / mutate-then-notify discipline as every other model here, keyed by `SeriesId` — a slotmap key that survives series reorder, exactly like `NodeId` survives tree mutation.

## 13. Divergence reporting — `first_changed_index()`

The four projection layers — `TreeSlice`, `TreeDataSlice`,
`SortFilterTreeModel`, and `SortFilterListModel` — rebuild their visible
list wholesale on every change, and (for the sort/filter proxies) notify
observers with a blanket `Reset`. That is the safe *notification* contract, but it
destroys information a consumer may need: which prefix of the visible
list is actually unchanged. The canonical consumer is variable-row-height
virtualization (`ListView` / `TreeView` / `TableView` / `TreeTableView`
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
- **`TreeDataSlice`** — prefix-compare of the old and new visible lists,
  comparing key, depth, has-children, expand state, **and item content**
  (hence the `T: PartialEq` bound). Unlike `TreeSlice` it has no
  `NodeUpdated` event — a whole re-source (`set_rows`) replaces the row
  stream — so folding the item comparison into the prefix check is what
  catches a pure rename (structure unchanged, one row's text differs) and
  reports that row's flat index; an expand/collapse reports the toggled
  row, an append reports the old length. A stable domain key `K` is
  required for equality to mean identity.
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

## 14. Projecting an external source of truth

§7 and §10 assume the view-model *owns* its model (the `QStandardItemModel`
shape). When the **domain owns the data** and the bastyde model is a projection
over it (a Qleany entity store, a DB, an event stream — the
`QAbstractItemModel` shape), **implement a data source over the domain instead
of mirroring it into a built-in model.**

Implement [`ListDataSource`] / [`TreeDataSource`] directly over the domain and
feed the view with `ListView::from_source` / `TreeView::from_source`. The view
*reads* through `with_item`/`with_entry` and *commands* through the DnD + lazy
capability methods (`drag` / `can_accept` / `accept_drop` / `request_window` /
`fetch_more`), so there is **no second in-memory copy to keep in sync** — the
domain stays the single source of truth, keyed by its own id (`i64`, a UUID,
…). Because identity is the domain key (not a positional `NodeId`), keyed
selection, tree expand state, and scroll follow a domain refresh automatically;
`version_signal()` + `first_changed_index()` (§13) narrow row-height caches
across the refresh. This is the path the designer's outline uses, and the one
to reach for whenever the data is a tree, lives behind a query, or doesn't fit
in memory. Full protocol reference: [data-source.md](data-source.md).

**Trees: reach for `TreeDataSlice` before hand-rolling `TreeDataSource`.** For
the common case — a tree stored as an indent-ordered row sequence (an outline:
binders/chapters/scenes, OPML, headings) — you rarely implement `TreeDataSource`
by hand. `TreeDataSlice<K, T>` (§4.4) is the ready-made engine: hand it
`Vec<TreeRow{ key, item, depth }>` on
each (re)load via `set_source`, and it derives the tree from the indent depth and
owns the per-view expand set, the collapse-aware flatten, `version_signal()`,
`first_changed_index()` (§13), and the DnD cycle guard — exactly what `TreeSlice`
gives a `TreeModel`, but keyed by your domain id and with no `TreeModel` to
mirror. Inject only the domain policy (`set_reorder`, `set_drag_policy`,
`set_drop_resolver`). Implement `TreeDataSource` directly only when the source
*isn't* a resolved indent sequence — a huge/lazy tree that pages children on
demand, where the eager flatten doesn't fit. Add view-layer sort/filter with a
`TreeRowFilter` on the row stream (§4.5), and tree checkboxes with a
`KeyedTreeCheckedModel` beside it (§6.1) — both compose without a second
projection.

A bounded, fully-resident, flat list is the one case where a `ListModel`
projection can still be simpler than a source impl. There is no `reconcile`
helper for it — keep the projection in sync by emitting the minimal `insert` /
`remove` / `move_item` / `set` mutations yourself (a `Repeater` then reorders
subtrees instead of recreating them); accept a `Reset` only when per-item state
loss is acceptable.

**Tables.** A `TableView`'s rows are a `ListDataSource` (or a `ListModel` for
the bounded-projection case); a `TreeTableView`'s rows are a `TreeDataSource`. A
cell edit is an in-place value update the source emits. Columns are
configuration, not data.

## 15. `ChartModel`, projections, and selection

`bastyde-charts` (`BarChart` / `LineChart` / `PieChart`) is the other
consumer of bastyde-data besides the widget-catalog row views — it
gets its own model family rather than reusing `ListModel<T>` because
chart data is two-level (series, then points within a series) and
carries chart-specific concerns (a color per series, a visibility
flag, a distinct paint-only vs. relayout change class) that a flat
list model has no vocabulary for. Full widget-side usage and the
reactivity/binding-level mapping live in
[charts.md §3](charts.md) and
[charts.md §8](charts.md); this section
covers the data-layer mechanism.

### 15.1 `ChartModel<T>` — the source

[`ChartModel<T>`](../crates/bastyde-data/src/chart_model.rs) is a
concrete reactive multi-series chart data model, `Rc<RefCell<…>>`
inside like every other model here — cloning shares the same series
and points and all clones see the same `ChartChange` notifications.
Series live in a flat `SlotMap` arena keyed by
[`SeriesId`](../crates/bastyde-data/src/chart_change.rs) (an opaque,
stable handle — the chart counterpart of `NodeId`: removing other
series never invalidates an existing `SeriesId`), plus a separate
`order: Vec<SeriesId>` giving display order independent of arena
layout. Each series holds a `Vec<ChartDatum<T>>` (`{ category: T,
value: f32 }`).

```rust
use bastyde_data::{ChartModel, ChartSeries, ChartDatum};

let model = ChartModel::from_series_vec(vec![
    ChartSeries::new("Revenue").data(vec![
        ChartDatum::new("Q1".to_string(), 10.0),
        ChartDatum::new("Q2".to_string(), 20.0),
    ]),
]);
let revenue = model.series_id_at(0).unwrap();
model.push_point(revenue, "Q3".to_string(), 30.0);
```

Every mutation method follows the mutate-then-notify discipline
(drop the `RefCell` borrow, *then* notify) and does two things:

1. Emits a [`ChartChange`](../crates/bastyde-data/src/chart_change.rs)
   describing exactly what changed — `SeriesInserted` / `SeriesRemoved`
   / `SeriesMoved` / `SeriesRenamed` / `SeriesColorChanged` /
   `SeriesVisibilityChanged` / `PointsInserted` / `PointsRemoved` /
   `PointUpdated` / `SeriesDataReplaced` / `Reset` — to every observer
   registered via `model.observe_changes(|change| …) -> ObserverHandle`
   (RAII, same as every other model's observer handle).
2. Bumps exactly one of two `Signal<u64>` version counters:
   `structure_version()` for everything that can move the y-domain,
   tick positions, or bar/point layout (series add/remove/move/rename,
   *and* visibility toggles, plus every point mutation), or
   `style_version()` for the one variant that's paint-only —
   `SeriesColorChanged`, from `set_series_color` /
   `clear_series_color`. This binary split is deliberately coarse: a
   consumer that only cares "did *anything* change" can bind either
   signal at `Rebuild`; a chart widget that wants to skip a relayout
   for a pure color change binds `structure_version` at `Relayout` and
   `style_version` at `RepaintOnly` separately (see charts.md §8 for
   the exact wiring).

Construction: `ChartModel::new()` (empty) + `add_series` /
`insert_series`, `ChartModel::from_series_vec(vec![ChartSeries...])`
(the common multi-series case, no per-item notification — mirrors
`ListModel::from_vec`), or `ChartModel::from_points(vec![ChartDatum...])`
(a single anonymous, visible series — the flat, one-dimensional path
`PieChart` uses). `ChartSeries<T>` (the construction DTO) carries
`visible: bool` — a **plain bool**, not a `Signal<bool>`: it only
describes a series' desired shape at construction time. Once a series
is in the model, mutate it through the model's own methods
(`set_series_visible`, `set_series_color`, `rename_series`,
`move_series`, `push_point` / `insert_point` / `remove_point` /
`update_point` / `replace_series_data`), not by reaching back into the
DTO. Read access is callback-scoped like every other model here —
`with_series`, `with_point`, `with_series_view` (one series, metadata
+ points slice), `with_all_series` (every series as an ordered slice
of views) — so the `RefCell` borrow never escapes.

### 15.2 `ChartWindow<T>` — last-N-points streaming projection

[`ChartWindow<T>`](../crates/bastyde-data/src/chart_window.rs) wraps a
`ChartModel<T>` and exposes only the tail `window_size` points of
every series — the live-scrolling-strip-chart pattern (a sensor feed,
a log-rate graph, a stock ticker). Unlike `ChartAggregate` below, it
copies **no point data**: it tracks, per series, the source index of
the window's first visible point and delegates every read straight
through to the source, so it needs no `T: Clone` bound at all.

```rust
let window = ChartWindow::new(model.clone(), 10);
assert_eq!(window.point_count(revenue), 10.min(model.point_count(revenue)));
```

The upstream `ChartChange` stream is *translated*, not collapsed to a
blanket `Reset` — a fixed-size tail window has no sort-key-move hazard
the way `SortFilterListModel` does, so fine-grained translation is
safe: a tail append into a full window becomes a `PointsRemoved` +
`PointsInserted` pair (the window slides), a tail append into a
still-growing window becomes a plain `PointsInserted`, and anything
that isn't a clean tail append (a mid-series insert, any removal)
falls back to a per-series rebuild reported as `SeriesDataReplaced`.
`set_window_size(n)` rebuilds every series and emits `Reset`.
`first_changed_index(series)` reports the first window-local index
that may differ since the latest translated change — per-series,
unlike the single flat value the list/tree proxies expose (§13),
because chart data is naturally two-level.

### 15.3 `ChartAggregate<T>` — bucket/rollup projection

[`ChartAggregate<T>`](../crates/bastyde-data/src/chart_aggregate.rs)
wraps a `ChartModel<T>` and reduces each series into fixed-size
buckets of `bucket_size` source points, each collapsed to one
`ChartDatum` via a [`ChartAggregateFn`](../crates/bastyde-data/src/chart_aggregate.rs)
— `Mean` / `Sum` / `Min` / `Max` / `First` / `Last` / `Custom(Rc<dyn
Fn(&[f32]) -> f32>)`. The "downsample a long series for display"
pattern — a year of daily sensor readings shown as weekly means, a
tick feed shown as 1-minute bars. Bucket `b` covers source indices
`[b*bucket_size, min((b+1)*bucket_size, n))`; a trailing partial
bucket is included; a bucket's category is its first member's.

```rust
let weekly = ChartAggregate::new(model.clone(), 7, ChartAggregateFn::Mean);
```

Unlike `ChartWindow`, `ChartAggregate` **materializes** its buckets —
a bucket's category is a *clone* of a source point's category, so
building or rebuilding one requires `T: Clone` (read-only queries
afterward need only `T: 'static`). Reactivity: a tail append that
doesn't change the bucket count updates the not-yet-full last bucket
in place (`PointUpdated`); a tail append that starts a new bucket
finalizes the previous last bucket (`PointUpdated`) and appends the
new one(s) (`PointsInserted`); a mid-series insert or any removal
falls back to a full per-series rebuild (`SeriesDataReplaced`).
`set_bucket_size(n)` / `set_aggregate_fn(f)` rebuild and emit `Reset`.
Same per-series `first_changed_index()` side-channel as `ChartWindow`.

### 15.4 `ChartSelection` — point-level selection

[`ChartSelection`](../crates/bastyde-data/src/chart_selection.rs) is
the chart counterpart of `SelectionModel` (§5) / `KeyedSelectionModel`
— it manages which `(SeriesId, usize)` pairs are selected across a
`ChartModel`, share-by-clone like the model itself, with the current
selection exposed as a reactive `Signal<HashSet<(SeriesId, usize)>>`
via `selection_signal()`. It uses a `HashSet`, not the `BTreeSet` flat
`SelectionModel` uses, because `SeriesId` is intentionally not `Ord`
(an opaque SlotMap key, mirroring `NodeId`) — there is no natural
ordering across series, only within one series' point indices.

```rust
let sel = ChartSelection::new(SelectionMode::Multi);
sel.select_point(revenue, 1);
sel.extend_to(revenue, 3);          // (revenue,1), (revenue,2), (revenue,3)
sel.toggle_point(revenue, 5);       // Ctrl+click
```

Same three `SelectionMode`s as `SelectionModel` (`None` / `Single` /
`Multi`, with anchor-based range extension in `Multi`).
`extend_to(series, target)` only extends **within the anchor's own
series** — a cross-series "range" has no natural order, so it falls
back to a single-point select of `(series, target)`. `adjust(&change)`
keeps the selection consistent as the source model mutates: a removed
or wholesale-replaced series drops its selected points (and the
anchor, if it pointed there); point insertions/removals shift or drop
indices within their series; series metadata changes (rename / recolor
/ visibility / move / insert) and in-place point updates never affect
which points are selected. `prune(exists)` drops any selected point
`exists` rejects — the same shape as `KeyedTreeCheckedModel::prune_missing`
(§6.1).

`ChartWindow` and `ChartAggregate` stay pure `bastyde-data` building
blocks an app composes on top of a `ChartModel` (feed a `ChartWindow`'s
or `ChartAggregate`'s output into a fresh `ChartModel::from_series_vec`
snapshot). `ChartSelection` is wired in directly, though: all three
chart widgets (`BarChart` / `LineChart` / `PieChart`) accept a shared
handle via `.selection(ChartSelection)` — the chart reuses its own
hover hit-test to select the tapped mark (Ctrl/Cmd-click toggles it in
`Multi` mode), clears the selection on a tap that misses every mark,
and paints an accent highlight on every selected mark. See
[charts.md §9](charts.md) for the paint/interaction details and
[charts.md §13](charts.md) for
the current state of the remaining `ChartWindow` / `ChartAggregate`
wiring.

---

## See also

- [architecture.md §6 UI Construction Patterns](architecture.md) — `Repeater` in context, static-vs-dynamic children.
- [architecture.md §14 Drag and Drop](architecture.md) — `DragPayload`, cross-widget reorder.
- [shortcut-intent-action.md](shortcut-intent-action.md) — typed intents, ancestor `Action`s, how the MVVM command layer lands in Rust.
- [crates/bastyde-data/src/list_model.rs](../crates/bastyde-data/src/list_model.rs), [tree_model.rs](../crates/bastyde-data/src/tree_model.rs), [tree_slice.rs](../crates/bastyde-data/src/tree_slice.rs), [tree_data_slice.rs](../crates/bastyde-data/src/tree_data_slice.rs), [tree_row_filter.rs](../crates/bastyde-data/src/tree_row_filter.rs), [selection_model.rs](../crates/bastyde-data/src/selection_model.rs), [list_data_source.rs](../crates/bastyde-data/src/list_data_source.rs), [tree_data_source.rs](../crates/bastyde-data/src/tree_data_source.rs), [keyed_tree_checked_model.rs](../crates/bastyde-data/src/keyed_tree_checked_model.rs).
- [crates/bastyde-data/src/data_change.rs](../crates/bastyde-data/src/data_change.rs), [tree_change.rs](../crates/bastyde-data/src/tree_change.rs).
- [crates/bastyde-data/src/chart_model.rs](../crates/bastyde-data/src/chart_model.rs) (§15) — `ChartModel<T>`, `ChartChange`, `SeriesId`; see also [chart_window.rs](../crates/bastyde-data/src/chart_window.rs), [chart_aggregate.rs](../crates/bastyde-data/src/chart_aggregate.rs), [chart_selection.rs](../crates/bastyde-data/src/chart_selection.rs).
- [examples/data_collections](../examples/data_collections/) — runnable demonstration of ListView, TreeView, Repeater, SelectionModel, and intra-widget DnD.
- [examples/chart_demo](../examples/chart_demo/) — `bastyde-charts` demo; see [charts.md](charts.md) for the chart-widget side of `ChartModel`.
