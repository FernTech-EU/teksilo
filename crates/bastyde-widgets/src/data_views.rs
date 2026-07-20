// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared substrate for the data views' source-owned drag-and-drop + lazy
//! loading.
//!
//! Centralizes the vocabulary the four data views (`ListView` / `TreeView` /
//! `TableView` / `TreeTableView`) share, so DnD validation (`can_accept`) and
//! the lazy placeholder are wired one way everywhere:
//!
//! - [`RowDragData`] — the **public, generic** intra-app drag payload a row (or
//!   a whole selected set) emits. The receiving source distinguishes its OWN
//!   reorder (matching [`ViewId`]) from a foreign drop, and translates the
//!   origin's `rows` → its own key via `key_at`, so the source's `Key` type
//!   never leaks into the view. When the origin opted into export it also
//!   carries `items` (clones of the dragged `T`), so a foreign `DropTarget`,
//!   a different data view, or the OS can consume the drag.
//! - [`DropIndicator`] — what `paint` renders; `allowed == false` is the
//!   pre-commit forbidden affordance.
//! - [`flat_insertion_target`] — maps a flat insertion index to the
//!   `(target, position)` pair `can_accept` / `accept_drop` expect.
//! - [`default_placeholder`] — the skeleton for a `Loading` row.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bastyde_core::ObserverHandle;
use bastyde_core::drag_payload::{DragPayload, DropOutcome};
use bastyde_core::widget::{EventContext, Widget};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_data::{
    DataChange, DropPosition, ItemKey, KeyedSelectionModel, SelectionMode, SelectionModel,
};

/// How a data-view row/tile is *activated* (opened/committed) by pointer —
/// distinct from *selection*, which also moves on arrow-key navigation. Mirrors
/// the platform split other toolkits expose (Qt
/// `SH_ItemView_ActivateItemOnSingleClick`, GTK `activate-on-single-click`).
/// Enter/Space always activates regardless of this mode.
///
/// Pass to `ListView::activate_on`, `TreeView::activate_on`, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivateOn {
    /// One primary click activates the row (KDE / web / Scrivener convention).
    /// Selection and activation happen on the same click.
    SingleClick,
    /// A double primary click activates the row; the first click only selects
    /// it (Finder / Explorer / Qt and GTK default). This is the [`Default`].
    #[default]
    DoubleClick,
}

/// Which kind of data view minted a [`ViewId`]. Folded into the id so two
/// different widget kinds that happen to draw the same value from the shared
/// process counter can never be mistaken for one another — the reason a bare
/// `usize` id was a latent cross-widget hazard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ViewKind {
    List,
    Tree,
    Table,
    TreeTable,
    Grid,
}

/// Opaque, kind-tagged, process-unique identity of a drag-capable data-view
/// instance. Used to tell a view's OWN reorder (`SameView`) from a foreign drop
/// on the receive side. Apps only ever compare two `ViewId`s for equality (e.g.
/// out of a received [`RowDragData`]); there is no public constructor, and the
/// value is stable for a view instance's lifetime, so it is safe to compare
/// even across windows (each mint is globally unique).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(ViewKind, usize);

impl ViewId {
    /// Mint a fresh, globally-unique id for a view of the given kind.
    pub(crate) fn next(kind: ViewKind) -> Self {
        Self(kind, next_view_id())
    }
}

/// What the *origin* view does to its own rows once a drag is accepted by a
/// **foreign** target (a different `DropTarget` / view / the OS). Purely an
/// origin-side cleanup choice — the receiver is unaffected. A same-view reorder
/// is never a transfer, so this never applies to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragTransferMode {
    /// Leave the origin rows in place (the dragged data is duplicated).
    Copy,
    /// Remove the dragged rows from the origin once accepted elsewhere
    /// (or exported as an OS move). This is the [`Default`].
    #[default]
    Move,
}

/// The public, generic drag payload every data-view row (or selected set)
/// emits. It occupies the single typed slot of a
/// [`bastyde_core::drag_payload::DragPayload`] and serves both audiences:
///
/// - the origin view's own erased classifier reads [`source`](Self::source) +
///   [`rows`](Self::rows) to recognise a same-view reorder;
/// - a **foreign** consumer (another view's custom `ListDataSource`, a
///   `DropTarget::accept_typed::<RowDragData<T>>()`, or `on_rows_received`)
///   reads [`items`](Self::items).
///
/// `items` is `Some` only when the origin view opted into export via
/// `.exportable(..)` (which requires `T: Clone`); a plain `.reorderable(true)`
/// drag carries `items == None` (nothing outside the origin could use it
/// anyway), so a reorder-only view is never accidentally droppable elsewhere.
#[derive(Debug)]
pub struct RowDragData<T: 'static> {
    /// Identity of the view that started the drag.
    pub source: ViewId,
    /// The dragged rows as the origin view's flat visible indices at
    /// drag-start, ascending. Informational (row count, app callbacks): the
    /// origin's accept path resolves the dragged rows' **stable keys** at
    /// drag-start and never re-reads these indices at hover/drop time — they
    /// go stale the moment the source reflows mid-drag (a spring-load
    /// auto-expand, a peer write). A foreign consumer should read
    /// [`items`](Self::items) instead.
    pub rows: Vec<usize>,
    /// Clones of the dragged items, `rows`-ordered. `None` for a reorder-only
    /// (non-exportable) drag.
    pub items: Option<Vec<T>>,
}

impl<T: 'static> RowDragData<T> {
    /// The dragged items, if this is an export drag (`.exportable(..)` was set
    /// on the origin). `None` for a reorder-only drag.
    pub fn items(&self) -> Option<&[T]> {
        self.items.as_deref()
    }

    /// Consume the payload for its items (avoids cloning on the receive side).
    pub fn into_items(self) -> Option<Vec<T>> {
        self.items
    }

    /// Whether this drag carries exportable items — i.e. the origin opted into
    /// `.exportable(..)`. A foreign receiver should gate on this (a reorder-only
    /// payload has the same Rust type but carries nothing usable).
    pub fn is_export(&self) -> bool {
        self.items.is_some()
    }

    /// Number of dragged rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no rows are carried (never true for a real drag).
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A drop indicator the data views' `paint` renders. `allowed == false` paints a
/// muted line where an accepted-drop line would be — the pre-commit "you can't
/// drop here" affordance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DropIndicator {
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) allowed: bool,
}

/// A process-unique id distinguishing data-view instances (for SameView drop
/// detection when several views share one source).
pub(crate) fn next_view_id() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Map a flat insertion index (`0..=len`) to the `(target_index, position)` pair
/// a `ListDataSource::can_accept` / `accept_drop` understands. `None` for an
/// empty list. Insertion *before* row `i` is `(i, Before)`; insertion past the
/// end is `(len-1, After)`.
pub(crate) fn flat_insertion_target(insertion: usize, len: usize) -> Option<(usize, DropPosition)> {
    if len == 0 {
        None
    } else if insertion >= len {
        Some((len - 1, DropPosition::After))
    } else {
        Some((insertion, DropPosition::Before))
    }
}

/// The default skeleton for a `Loading` row — a muted inset bar. The row's
/// placement sizes it to the row's height and width.
pub(crate) fn default_placeholder() -> Box<dyn Widget> {
    use crate::primitives::{Padding, RectWidget};
    Box::new(
        Padding::uniform(6.0).child(
            RectWidget::new()
                .background(bastyde_tokens::SurfaceRole::Hover)
                .corner_radius(bastyde_tokens::CornerRadius::uniform(4.0)),
        ),
    )
}

/// Index-facing row-selection facade backing the four data views.
///
/// An app installs *either* the index-based [`SelectionModel`] (positions) or a
/// [`KeyedSelectionModel<K>`] (stable identities that survive reorder / filter /
/// window-slide / multi-view). The views' click / keyboard / rebuild / paint
/// paths all work in **indices**, so this facade erases the difference: the
/// keyed variant carries the view's index↔key mapping (`key_at` / `len` /
/// `contains_key`) and translates internally. The method surface deliberately
/// mirrors `SelectionModel` so call sites read identically (`rs.select(i)`,
/// `rs.is_selected(i)`, …).
#[derive(Clone)]
pub(crate) struct RowSelection {
    mode: SelectionMode,
    is_selected: Rc<dyn Fn(usize) -> bool>,
    select_fn: Rc<dyn Fn(usize)>,
    toggle_fn: Rc<dyn Fn(usize)>,
    extend_fn: Rc<dyn Fn(usize)>,
    select_all_fn: Rc<dyn Fn(usize)>,
    selected_indices_fn: Rc<dyn Fn() -> Vec<usize>>,
    clear_fn: Rc<dyn Fn()>,
    observe_fn: Rc<dyn Fn(Box<dyn Fn()>) -> ObserverHandle>,
    on_change_fn: Rc<dyn Fn(&DataChange)>,
    /// Unconditional prune for the version-signal-driven tree views (which
    /// don't emit a `DataChange`): drop orphaned keys (keyed) or no-op (index).
    prune_fn: Rc<dyn Fn()>,
}

impl RowSelection {
    /// Back the facade with the index-based [`SelectionModel`]. Index ops pass
    /// straight through; `on_data_change` index-shifts (insert / remove) or
    /// clears (reset) the selection, matching the legacy inline behaviour.
    pub(crate) fn from_index(sel: SelectionModel) -> Self {
        let (s_is, s_sel, s_tog, s_ext, s_all, s_idx, s_clr, s_obs, s_chg) = (
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
            sel.clone(),
        );
        Self {
            mode: sel.mode(),
            is_selected: Rc::new(move |i| s_is.is_selected(i)),
            select_fn: Rc::new(move |i| s_sel.select(i)),
            toggle_fn: Rc::new(move |i| s_tog.toggle(i)),
            extend_fn: Rc::new(move |i| s_ext.extend_to(i)),
            select_all_fn: Rc::new(move |count| s_all.select_all(count)),
            selected_indices_fn: Rc::new(move || s_idx.selected_indices()),
            clear_fn: Rc::new(move || s_clr.clear()),
            observe_fn: Rc::new(move |cb| s_obs.selection_signal().observe(move |_| cb())),
            on_change_fn: Rc::new(move |change| match change {
                DataChange::ItemsInserted { range } => {
                    s_chg.adjust_for_insert(range.start, range.end - range.start);
                }
                DataChange::ItemsRemoved { range } => {
                    s_chg.adjust_for_remove(range.start, range.end - range.start);
                }
                DataChange::ItemsMoved { from, to, count } => {
                    s_chg.adjust_for_move(*from, *to, *count);
                }
                DataChange::Reset => s_chg.clear(),
                _ => {}
            }),
            // The index model has no stable identity to prune against on a
            // bare version bump — tree structural adjustments stay no-ops here
            // (the legacy behaviour).
            prune_fn: Rc::new(|| {}),
        }
    }

    /// Back the facade with a [`KeyedSelectionModel<K>`] plus the view's
    /// index↔key mapping. `key_at(i)` is the key at visible index `i`, `len()`
    /// the visible count (for Shift-range ordering and `selected_indices`), and
    /// `contains_key(&k)` whether the *source* still holds the key (for
    /// prune-on-remove — a collapsed-but-present tree node must NOT be pruned,
    /// so this is supplied by the view, not derived from the visible window).
    pub(crate) fn from_keyed<K: ItemKey>(
        keyed: KeyedSelectionModel<K>,
        key_at: Rc<dyn Fn(usize) -> Option<K>>,
        len: Rc<dyn Fn() -> usize>,
        contains_key: Rc<dyn Fn(&K) -> bool>,
    ) -> Self {
        let mode = keyed.mode();
        Self {
            mode,
            is_selected: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |i| ka(i).map(|key| k.is_selected(&key)).unwrap_or(false))
            },
            select_fn: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |i| {
                    if let Some(key) = ka(i) {
                        k.select(key);
                    }
                })
            },
            toggle_fn: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |i| {
                    if let Some(key) = ka(i) {
                        k.toggle(key);
                    }
                })
            },
            extend_fn: {
                let (k, ka, l) = (keyed.clone(), key_at.clone(), len.clone());
                Rc::new(move |i| {
                    if let Some(target) = ka(i) {
                        let ordered: Vec<K> = (0..l()).filter_map(|j| ka(j)).collect();
                        k.extend_to(target, &ordered);
                    }
                })
            },
            select_all_fn: {
                let (k, ka) = (keyed.clone(), key_at.clone());
                Rc::new(move |count| {
                    let keys: Vec<K> = (0..count).filter_map(|i| ka(i)).collect();
                    k.select_keys(keys, false);
                })
            },
            selected_indices_fn: {
                let (k, ka, l) = (keyed.clone(), key_at.clone(), len.clone());
                Rc::new(move || {
                    (0..l())
                        .filter(|&i| ka(i).map(|key| k.is_selected(&key)).unwrap_or(false))
                        .collect()
                })
            },
            clear_fn: {
                let k = keyed.clone();
                Rc::new(move || k.clear())
            },
            observe_fn: {
                let k = keyed.clone();
                Rc::new(move |cb| k.selection_signal().observe(move |_| cb()))
            },
            on_change_fn: {
                let (k, c) = (keyed.clone(), contains_key.clone());
                Rc::new(move |change| match change {
                    // Keys are stable across inserts / moves; only removals and
                    // resets can orphan a selected key.
                    DataChange::ItemsRemoved { .. } | DataChange::Reset => {
                        k.prune_missing(|key| c(key));
                    }
                    _ => {}
                })
            },
            prune_fn: {
                let (k, c) = (keyed, contains_key);
                Rc::new(move || k.prune_missing(|key| c(key)))
            },
        }
    }

    pub(crate) fn mode(&self) -> SelectionMode {
        self.mode
    }
    pub(crate) fn is_selected(&self, index: usize) -> bool {
        (self.is_selected)(index)
    }
    pub(crate) fn select(&self, index: usize) {
        (self.select_fn)(index)
    }
    pub(crate) fn toggle(&self, index: usize) {
        (self.toggle_fn)(index)
    }
    pub(crate) fn extend_to(&self, index: usize) {
        (self.extend_fn)(index)
    }
    pub(crate) fn select_all(&self, count: usize) {
        (self.select_all_fn)(count)
    }
    pub(crate) fn selected_indices(&self) -> Vec<usize> {
        (self.selected_indices_fn)()
    }
    pub(crate) fn clear(&self) {
        (self.clear_fn)()
    }
    /// Subscribe to selection changes (drives the view's rebuild). Owns the
    /// returned handle for the subscription's lifetime.
    pub(crate) fn observe_for_rebuild(&self, cb: impl Fn() + 'static) -> ObserverHandle {
        (self.observe_fn)(Box::new(cb))
    }
    /// React to a source data change (index-shift for the index model, prune
    /// for the keyed model).
    pub(crate) fn on_data_change(&self, change: &DataChange) {
        (self.on_change_fn)(change)
    }
    /// Prune orphaned keys (keyed model) — used by the tree views, which drive
    /// off a version signal rather than a `DataChange`. No-op for the index
    /// model.
    pub(crate) fn prune(&self) {
        (self.prune_fn)()
    }
}

/// Resolves a set of the origin view's flat indices to a **removal thunk** at
/// drag-start. Invoked at completion, the thunk removes exactly those rows from
/// the source. Resolving eagerly (rather than re-reading flat indices at
/// completion) keeps a Move correct even if the origin's flat indices reshuffle
/// mid-drag — e.g. a `TreeView` spring-load auto-expand — since the stable keys
/// were already captured. The source erasure supplies it.
pub(crate) type SnapshotOutFn = Rc<dyn Fn(&[usize]) -> Box<dyn Fn()>>;

/// Active drag-drop feedback a tree data view paints itself: a between-rows
/// insertion line (Before/After) or a highlighted row (an into-container drop).
///
/// Shared by `TreeView` and `TreeTableView` so both render the same affordance
/// for the same source verdict.
/// A stable handle to a row in a data view.
///
/// Per-row event handlers (a chevron toggle, a click, an activation) are built
/// once and then live as long as the row widget does, so capturing the flat
/// index they were built at is fragile: expanding a branch above, applying a
/// filter, or sorting shifts every index below, and the stale handler would act
/// on whatever row moved into that slot.
///
/// A `RowAnchor` closes over the row's **source-owned identity** instead and
/// resolves the row's *current* position on demand. The key never surfaces in
/// the anchor's type — it is captured inside the resolver, so views stay
/// key-agnostic ([`TreeSource`](crate::tree_source::TreeSource) and
/// [`ListSource`](crate::list_source::ListSource) both erase it).
///
/// Sources without identity (a bare `ListModel`, or any source that leaves
/// `key_at` at its `None` default) get a fixed anchor that always reports the
/// index it was built with — no worse than capturing the index directly.
///
/// A bare `ListModel` has no identity to offer (a `Vec` row *is* its position),
/// so anchors over one are fixed. `SortFilterListModel` keys rows by their
/// **source index**, which no sort/filter reprojection renumbers — so anchors
/// over a projection do track their row across a filter change, which is the
/// flat fragility in practice. They can still mis-resolve inside the window
/// between an *upstream* insert/remove and the rebuild it schedules, since that
/// does renumber source indices; no worse than the captured index they replace.
/// The tree sources all carry real identity.
///
/// **Precondition: keys must be unique.** Resolution falls back to a lookup by
/// key, which returns the *first* match, so a source handing out duplicate keys
/// would silently redirect an anchor onto a different row — the very failure
/// this type exists to prevent.
#[derive(Clone)]
pub struct RowAnchor {
    resolve: Rc<dyn Fn() -> Option<usize>>,
}

impl RowAnchor {
    /// Build an identity-backed anchor from a resolver.
    pub(crate) fn new(resolve: Rc<dyn Fn() -> Option<usize>>) -> Self {
        Self { resolve }
    }

    /// An anchor for a source with no identity: always reports `index`.
    pub(crate) fn fixed(index: usize) -> Self {
        Self {
            resolve: Rc::new(move || Some(index)),
        }
    }

    /// The row's current flat index, or `None` if it no longer exists in the
    /// source (it was deleted, or filtered away).
    pub fn index(&self) -> Option<usize> {
        (self.resolve)()
    }

    /// Whether the row still exists.
    pub fn is_live(&self) -> bool {
        self.index().is_some()
    }
}

impl std::fmt::Debug for RowAnchor {
    /// Deliberately does NOT resolve: the resolver reads the source's
    /// interior-mutable state, so a `{:?}` from inside code already holding a
    /// borrow (a `set_source` closure, a reorder callback, a debugger's
    /// pretty-printer) would panic on a `RefCell` conflict.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RowAnchor(..)")
    }
}

/// Keep an open cell editor pointing at the row it was opened on.
///
/// `editing_cell` is a `(row, col)` pair that outlives rebuilds, so rows
/// appearing or vanishing above an open editor would slide it onto a different
/// row. The anchor is captured the first time an open editor is seen and
/// re-resolved on every later rebuild: the row index is rewritten when it moved,
/// and the editor closes outright when its row is gone — better than silently
/// editing whoever took the slot.
///
/// Called from each body pane's `build`, which is the only place that sees both
/// an editing change and a data change. It can therefore write `editing_cell`
/// while that pane is building; the write is idempotent and converges in one
/// extra pass (the next reconcile finds `cur == row` and writes nothing), which
/// `an_editing_reconcile_converges_in_one_pass` pins.
pub(crate) fn reconcile_editing_row(
    editing_cell: &bastyde_core::signal::Signal<Option<(usize, usize)>>,
    slot: &Rc<std::cell::RefCell<Option<RowAnchor>>>,
    anchor_of: &dyn Fn(usize) -> RowAnchor,
) {
    let Some((row, col)) = editing_cell.get() else {
        *slot.borrow_mut() = None;
        return;
    };
    let existing = slot.borrow().clone();
    match existing {
        None => *slot.borrow_mut() = Some(anchor_of(row)),
        Some(anchor) => match anchor.index() {
            Some(cur) if cur != row => editing_cell.set(Some((cur, col))),
            Some(_) => {}
            None => {
                editing_cell.set(None);
                *slot.borrow_mut() = None;
            }
        },
    }
}

/// Tint for the "drop into this container" row highlight. Defined once so the
/// `DropFeedback` handed to the framework and the widget's own paint cannot
/// drift into two different colors on the same row.
pub(crate) fn drop_into_tint() -> bastyde_tokens::Color {
    bastyde_tokens::Color::from_rgba(0.25, 0.47, 0.85, 0.25)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum DropViz {
    /// Horizontal insertion line at `y`, spanning `width`.
    Line { y: f32, width: f32 },
    /// Highlighted target row `[top, top + height]`, spanning `width` — the
    /// "drop into this folder" affordance.
    Rect { top: f32, height: f32, width: f32 },
}

/// The reusable export / foreign-drop machinery shared by all five data views:
/// the config fields, the drag-start payload build (selection set already
/// resolved by the caller), the foreign-receive sugar, and the `on_drag_ended`
/// move-out completion. Each view holds ONE of these instead of duplicating the
/// logic five ways (the drift that a code review caught). See
/// [docs/drag-and-drop.md §12](https://github.com/ferntech-eu/bastyde/blob/main/docs/drag-and-drop.md).
pub(crate) struct RowExport<T: 'static> {
    /// `Some` once `.exportable(..)` was called; the transfer mode also drives
    /// the move-out completion.
    pub(crate) mode: Option<DragTransferMode>,
    /// Clones `&T` → `T` for the payload (set by `.exportable`/`.export_external`,
    /// each `where T: Clone`, so the view constructor stays unconstrained).
    #[allow(clippy::type_complexity)]
    pub(crate) clone_item_fn: Option<Rc<dyn Fn(&T) -> T>>,
    /// Builds MIME reps of the dragged items for OS / `DropZone` export.
    #[allow(clippy::type_complexity)]
    pub(crate) export_mime_fn: Option<Rc<dyn Fn(&[T]) -> Vec<(String, Vec<u8>)>>>,
    /// App override for removing rows moved out to a foreign target.
    #[allow(clippy::type_complexity)]
    pub(crate) on_rows_transferred_out: Option<Rc<dyn Fn(&[usize], &mut EventContext)>>,
    /// Accept exported rows from a different view/source (zero-custom-source).
    pub(crate) accept_foreign_rows: bool,
    /// Handler for rows accepted via `accept_foreign_rows`.
    #[allow(clippy::type_complexity)]
    pub(crate) on_rows_received: Option<Rc<dyn Fn(Vec<T>, usize, &mut EventContext)>>,
    /// Set by the view's own `on_drop` when it applied a same-view reorder, so
    /// the completion skips the move-out (already applied). The TabBar pattern.
    pub(crate) self_reorder_flag: Rc<Cell<bool>>,
    /// The rows carried by the in-flight drag (for the app move-out callback).
    dragged_rows: Rc<RefCell<Vec<usize>>>,
    /// Stable-key removal thunk for the default move-out, resolved at drag-start.
    #[allow(clippy::type_complexity)]
    removal: Rc<RefCell<Option<Box<dyn Fn()>>>>,
}

impl<T: 'static> Clone for RowExport<T> {
    // Hand-written (not derived) so cloning does NOT require `T: Clone` — every
    // field is an `Rc` / `Copy`, so a clone shares the same drag stash + flags,
    // which is exactly what the per-row drag closure needs.
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            clone_item_fn: self.clone_item_fn.clone(),
            export_mime_fn: self.export_mime_fn.clone(),
            on_rows_transferred_out: self.on_rows_transferred_out.clone(),
            accept_foreign_rows: self.accept_foreign_rows,
            on_rows_received: self.on_rows_received.clone(),
            self_reorder_flag: self.self_reorder_flag.clone(),
            dragged_rows: self.dragged_rows.clone(),
            removal: self.removal.clone(),
        }
    }
}

impl<T: 'static> Default for RowExport<T> {
    fn default() -> Self {
        Self {
            mode: None,
            clone_item_fn: None,
            export_mime_fn: None,
            on_rows_transferred_out: None,
            accept_foreign_rows: false,
            on_rows_received: None,
            self_reorder_flag: Rc::new(Cell::new(false)),
            dragged_rows: Rc::new(RefCell::new(Vec::new())),
            removal: Rc::new(RefCell::new(None)),
        }
    }
}

impl<T: 'static> RowExport<T> {
    /// `.exportable(mode)` — carry item clones; `where T: Clone` at the call.
    pub(crate) fn set_exportable(&mut self, mode: DragTransferMode)
    where
        T: Clone,
    {
        self.mode = Some(mode);
        if self.clone_item_fn.is_none() {
            self.clone_item_fn = Some(Rc::new(|t: &T| t.clone()));
        }
    }

    /// `.export_external(f)` — attach MIME; implies exportable.
    pub(crate) fn set_export_external(
        &mut self,
        f: impl Fn(&[T]) -> Vec<(String, Vec<u8>)> + 'static,
    ) where
        T: Clone,
    {
        if self.clone_item_fn.is_none() {
            self.clone_item_fn = Some(Rc::new(|t: &T| t.clone()));
        }
        if self.mode.is_none() {
            self.mode = Some(DragTransferMode::default());
        }
        self.export_mime_fn = Some(Rc::new(f));
    }

    pub(crate) fn set_on_rows_transferred_out(
        &mut self,
        f: impl Fn(&[usize], &mut EventContext) + 'static,
    ) {
        self.on_rows_transferred_out = Some(Rc::new(f));
    }

    pub(crate) fn set_on_rows_received(
        &mut self,
        f: impl Fn(Vec<T>, usize, &mut EventContext) + 'static,
    ) {
        self.on_rows_received = Some(Rc::new(f));
    }

    /// Rows are a drag source when the view reorders OR exports.
    pub(crate) fn is_drag_source(&self, reorderable: bool) -> bool {
        reorderable || self.mode.is_some()
    }

    /// The view is a drop target when it reorders OR accepts foreign rows.
    pub(crate) fn is_drop_target(&self, reorderable: bool) -> bool {
        reorderable || self.accept_foreign_rows
    }

    /// Build the drag payload for the (already selection-resolved) `rows`. Drops
    /// any non-resident row (a lazy `Loading` row `read` can't serve) so `rows`
    /// and `items` stay index-aligned and a Move never deletes a row whose data
    /// wasn't transferred. Attaches MIME, and stashes the rows + a stable-key
    /// removal thunk for the completion.
    pub(crate) fn build_payload(
        &self,
        source: ViewId,
        mut rows: Vec<usize>,
        read: &dyn Fn(usize, &mut dyn FnMut(&T)) -> bool,
        snapshot_out: &SnapshotOutFn,
    ) -> DragPayload {
        let items: Option<Vec<T>> = if let Some(cf) = self.clone_item_fn.as_ref() {
            let mut out = Vec::with_capacity(rows.len());
            rows.retain(|&r| {
                let mut got = None;
                read(r, &mut |t| got = Some(cf(t)));
                match got {
                    Some(v) => {
                        out.push(v);
                        true
                    }
                    None => false,
                }
            });
            Some(out)
        } else {
            None
        };
        let mime_pairs: Vec<(String, Vec<u8>)> =
            match (self.export_mime_fn.as_ref(), items.as_ref()) {
                (Some(mf), Some(its)) => mf(its),
                _ => Vec::new(),
            };
        let mut payload = DragPayload::typed(RowDragData::<T> {
            source,
            rows: rows.clone(),
            items,
        });
        let has_mime = !mime_pairs.is_empty();
        for (mime, bytes) in mime_pairs {
            payload = payload.with_mime(&mime, bytes);
        }
        if has_mime {
            payload.enrich_external_from_mime();
        }
        *self.removal.borrow_mut() = Some((snapshot_out)(&rows));
        *self.dragged_rows.borrow_mut() = rows;
        payload
    }

    /// Whether a **foreign** exported payload would be accepted here — for the
    /// hover affordance. (Same-view / reorder-only payloads return `false`.)
    pub(crate) fn accepts_foreign_export(&self, payload: &DragPayload, source: ViewId) -> bool {
        self.accept_foreign_rows
            && self.on_rows_received.is_some()
            && payload
                .get_typed::<RowDragData<T>>()
                .is_some_and(|rd| rd.source != source && rd.is_export())
    }

    /// Foreign-receive sugar for a view's `on_drop`. Peeks before taking, so a
    /// non-matching payload is left intact for any further fallback.
    pub(crate) fn foreign_receive(
        &self,
        payload: &mut DragPayload,
        source: ViewId,
        insertion: usize,
        ctx: &mut EventContext,
    ) -> bool {
        if self.accepts_foreign_export(payload, source)
            && let Some(cb) = self.on_rows_received.as_ref()
            && let Some(rd) = payload.take_typed::<RowDragData<T>>()
            && let Some(items) = rd.items
        {
            cb(items, insertion, ctx);
            return true;
        }
        false
    }

    /// The view's own `on_drop` calls this after applying a genuine SAME-VIEW
    /// reorder, so the completion knows the change was already applied.
    pub(crate) fn note_self_reorder(&self) {
        self.self_reorder_flag.set(true);
    }

    /// Install the `on_drag_ended` move-out completion. A same-view reorder set
    /// `self_reorder_flag` (skipped here); on `Move` + accepted-elsewhere the
    /// origin rows are removed via the app override (delivered **descending** so
    /// index-by-index removal stays valid) or the stable-key removal thunk.
    pub(crate) fn install_completion(&self, handlers: HandlerSet) -> HandlerSet {
        let Some(mode) = self.mode else {
            return handlers;
        };
        let flag = self.self_reorder_flag.clone();
        let dragged = self.dragged_rows.clone();
        let removal = self.removal.clone();
        let on_out = self.on_rows_transferred_out.clone();
        handlers.on_drag_ended(move |outcome, ctx| {
            let handled_by_us = flag.replace(false);
            let rows = std::mem::take(&mut *dragged.borrow_mut());
            let thunk = removal.borrow_mut().take();
            if handled_by_us {
                return;
            }
            let accepted_elsewhere = matches!(
                outcome,
                DropOutcome::InApp { accepted: true } | DropOutcome::OsMove
            );
            if mode != DragTransferMode::Move || !accepted_elsewhere || rows.is_empty() {
                return;
            }
            if let Some(cb) = on_out.as_ref() {
                let mut desc = rows;
                desc.sort_unstable();
                desc.reverse();
                cb(&desc, ctx);
            } else if let Some(thunk) = thunk {
                thunk();
            }
        })
    }
}

/// Shared deferred-selection press logic for a data-view row (the drift a code
/// review caught: the `press_claimed` guard was missing in one view). Pressing
/// an already-selected row DEFERS the collapse-to-single to a release WITHOUT a
/// drag (an active drag consumes `PointerUp`), so grabbing a multi-selection
/// drags the whole set. `pending` is a per-row cell shared by the two calls.
pub(crate) mod deferred_select {
    use std::cell::Cell;
    use std::rc::Rc;

    use bastyde_core::event::Modifiers;
    use bastyde_core::widget::EventContext;

    use super::RowSelection;

    /// Handle a primary `PointerDown` on row `index`. Returns without selecting
    /// if the press was claimed by an interactive child (also clearing a stale
    /// `pending`). Ctrl/Shift select immediately; a plain press on an
    /// already-selected row defers; otherwise selects.
    pub(crate) fn on_down(
        sel: &RowSelection,
        index: usize,
        modifiers: Modifiers,
        pending: &Rc<Cell<bool>>,
        ctx: &mut EventContext,
    ) -> bool {
        if ctx.press_claimed_by_interactive_child() {
            pending.set(false);
            return false;
        }
        if modifiers.ctrl() {
            sel.toggle(index);
            pending.set(false);
        } else if modifiers.shift() {
            sel.extend_to(index);
            pending.set(false);
        } else if sel.is_selected(index) {
            pending.set(true);
        } else {
            sel.select(index);
            pending.set(false);
        }
        true
    }

    /// Handle a primary `PointerUp` on row `index` — reached only on a click
    /// WITHOUT a drag. Collapses the deferred multi-selection, unless the
    /// release belongs to an interactive child.
    pub(crate) fn on_up(
        sel: &RowSelection,
        index: usize,
        pending: &Rc<Cell<bool>>,
        ctx: &mut EventContext,
    ) {
        if ctx.press_claimed_by_interactive_child() {
            return;
        }
        if pending.replace(false) {
            sel.select(index);
        }
    }
}
