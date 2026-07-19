// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TreeView — a virtualized, expandable/collapsible hierarchical list widget.
//!
//! Displays a [`TreeModel<T>`](bastyde_data::TreeModel) as an indented tree.
//! Internally each view owns a [`TreeSlice`] for independent
//! expand state, so two `TreeView`s on the same model can be open at different
//! depths simultaneously. Only rows in the visible viewport + a small buffer have
//! live widgets — rows outside the buffer are dormant, matching `ListView`'s
//! virtualization model. An external [`TreeDataSource`]
//! is also accepted via [`TreeView::from_source`] when the data lives outside a
//! `TreeModel`.
//!
//! Row heights come in three modes: uniform (`item_height`, default fast path),
//! exact per-flat-index callback (`item_height_fn`), and auto-measured
//! (`auto_item_height` — height-for-width per row, scroll-anchored).
//!
//! ## Example
//!
//! ```rust
//! # use bastyde_widgets::TreeView;
//! # use bastyde_widgets::primitives::{HStack, Padding, TextWidget};
//! # use bastyde_data::TreeModel;
//! # use bastyde_i18n::lit;
//! # struct Item { title: String }
//! # let tree_model: TreeModel<Item> = TreeModel::new();
//! let _w = TreeView::new(tree_model, |item, entry, _selected| {
//!     let indent = entry.depth as f32 * 20.0;
//!     Box::new(HStack::new()
//!         .child(Padding::new(0.0, 0.0, 0.0, indent))
//!         .child(TextWidget::new(lit!(&item.title))))
//! })
//! .item_height(28.0);
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_tokens::{BorderRole, Easing};

use bastyde_core::DropFeedback;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::signal::{Prop, Signal};
use bastyde_core::widget::{LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

use bastyde_data::selection_model::SelectionModel;
use bastyde_data::tree_slice::{TreeSlice, TreeSliceHandle};
use bastyde_data::{
    DragEligibility, DropPosition, DropResponse, FlatEntry, ItemKey, KeyedSelectionModel, NodeId,
    RowState, TreeDataSource, TreeModel,
};

use crate::common::row_metrics::{HeightSource, RowMetrics, SharedRowMetrics};
use crate::common::scroll::OverscrollBehavior;
use crate::data_views::{DragTransferMode, RowDragData, RowSelection, ViewId, ViewKind};
use crate::scroll_area::ScrollBarMode;
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation, ScrollBarVisual};
use crate::tree_source::{TreeRow, TreeRowMeta, TreeSource};

const BUFFER_ITEMS: usize = 5;
const DEFAULT_ITEM_HEIGHT: f32 = 28.0;
const SCROLLBAR_THICKNESS: f32 = 12.0;

/// Per-row context passed to a 4-arg TreeView delegate. Carries a
/// reference to the slice handle and the row's `NodeId` so the
/// delegate can wire chevron toggles and other tree-aware behavior
/// without manually cloning state outside the closure.
///
/// Created internally by [`TreeView::new_with_context`]. Not
/// constructed directly by user code.
pub struct TreeRowContext<'a, T: 'static> {
    slice: &'a TreeSliceHandle<T>,
    node_id: bastyde_data::NodeId,
}

impl<'a, T: 'static> TreeRowContext<'a, T> {
    /// Toggle callback for this row's chevron. Wires in one line:
    /// `.on_toggle_rc(ctx.toggle_callback())`.
    pub fn toggle_callback(&self) -> std::rc::Rc<dyn Fn(&mut bastyde_core::widget::EventContext)> {
        let slice = self.slice.clone();
        let node = self.node_id;
        std::rc::Rc::new(move |_ctx| slice.toggle_expand(node))
    }

    /// Cloned handle to the slice — call `.toggle_expand(node)`,
    /// `.expand(node)`, `.collapse(node)` directly.
    pub fn slice_handle(&self) -> TreeSliceHandle<T> {
        self.slice.clone()
    }

    /// The `NodeId` of this row in the backing `TreeModel`.
    pub fn node_id(&self) -> bastyde_data::NodeId {
        self.node_id
    }
}

/// Delegate type for the built-in `TreeModel` path: takes the inputs the 3-arg
/// form gets plus the optional `TreeRowContext`. Both the 3-arg `new` and the
/// 4-arg `new_with_context` produce a closure of this shape.
type TreeDelegate<T> = dyn Fn(&T, &FlatEntry, bool, &TreeRowContext<'_, T>) -> Box<dyn Widget>;

/// Delegate type for the generic [`TreeView::from_source`] path: key-erased, so
/// it receives a [`TreeRow`] (flat metadata + a chevron toggle) instead of the
/// `NodeId`-typed `FlatEntry` / `TreeRowContext`.
type SourceTreeDelegate<T> = dyn Fn(&T, &TreeRow, bool) -> Box<dyn Widget>;

/// Internal, uniform per-row builder both constructors lower to:
/// `(visible_index, &item, &meta, selected) -> row widget`. The built-in
/// wrapper rebuilds the `NodeId` `TreeRowContext` from the index; the generic
/// wrapper builds a key-erased `TreeRow`.
type RowDelegate<T> = dyn Fn(usize, &T, &TreeRowMeta, bool) -> Box<dyn Widget>;

/// A virtualized hierarchical tree widget backed by a `TreeModel<T>`.
///
/// ```rust
/// # use bastyde_widgets::{TreeView};
/// # use bastyde_widgets::primitives::{HStack, Padding, TextWidget};
/// # use bastyde_data::TreeModel;
/// # use bastyde_i18n::lit;
/// # struct Item { title: String }
/// # let tree_model: TreeModel<Item> = TreeModel::new();
/// let _w = TreeView::new(tree_model, |item, entry, _selected| {
///     let indent = entry.depth as f32 * 20.0;
///     Box::new(HStack::new()
///         .child(Padding::new(0.0, 0.0, 0.0, indent))
///         .child(TextWidget::new(lit!(&item.title))))
/// })
/// .item_height(28.0);
/// ```
use crate::data_views::DropViz;

pub struct TreeView<T: 'static> {
    /// Index-keyed erased backing — the built-in `TreeSlice` or an external
    /// `TreeDataSource`. All virtualization / DnD / keyboard work goes through
    /// this in flat indices.
    source: Rc<TreeSource<T>>,
    /// Present only for the built-in `TreeModel` path; backs the `NodeId`-typed
    /// public expand API + [`tree_slice`](Self::tree_slice). `None` for
    /// [`from_source`](Self::from_source).
    slice: Option<Rc<TreeSlice<T>>>,
    /// Uniform per-row builder produced by whichever constructor was used.
    row_delegate: Rc<RowDelegate<T>>,
    item_height: f32,
    /// Height-mode selection (uniform / exact callback / auto-measure).
    height_source: HeightSource,
    /// Row geometry — all virtualization consumers go through this.
    metrics: SharedRowMetrics,
    /// Row selection — index-based `SelectionModel` or keyed
    /// `KeyedSelectionModel<NodeId>`, unified behind the index-facing facade.
    row_selection: Option<RowSelection>,

    /// Keyboard-focused flat index.
    focused_index: Rc<Cell<Option<usize>>>,

    /// Type-ahead ("type to jump") label extractor — opt-in via
    /// [`type_ahead_label`](Self::type_ahead_label).
    type_ahead_label: Option<Rc<dyn Fn(&T) -> String>>,
    /// Reset window for the type-ahead search term.
    type_ahead_timeout: Duration,
    /// Persistent type-ahead buffer (survives the per-keystroke rebuild).
    type_ahead: Rc<crate::common::type_ahead::TypeAheadState>,

    /// Enable intra-widget drag reordering.
    reorderable: bool,

    /// Cross-widget export / foreign-receive machinery — the builders
    /// (`.exportable`, `.export_external`, `.accept_foreign_rows`,
    /// `.on_rows_received`, `.on_rows_transferred_out`), the drag-start payload
    /// build, and the move-out completion, shared by all five data views.
    export: crate::data_views::RowExport<T>,

    /// Whether a row-body PointerUp on a branch row auto-toggles its
    /// expansion. Defaults to `true` (legacy behavior — convenient
    /// for hand-built delegates without an explicit chevron). Set to
    /// `false` when the delegate provides its own chevron tap target
    /// (e.g. `StandardTreeItem`) to avoid the auto-toggle firing in
    /// addition to the chevron's own click and cancelling out.
    row_click_expands: bool,

    /// Active drop feedback (set by on_drag_hover, cleared by on_drag_leave,
    /// read by paint). Reactive Signal — bound at `RepaintOnly` so any
    /// `set(...)` call dirties the TreeView for repaint automatically.
    drop_feedback: Signal<Option<DropViz>>, // insertion line OR folder highlight

    /// Optional row-activation callback (a click on the row body per
    /// `activate_on`, or Enter/Space on the focused row) — distinct from
    /// *selection*, which also moves on arrow navigation. Lets a view
    /// open/commit a row without firing on every navigation step.
    on_activate: Option<Rc<dyn Fn(usize)>>,
    /// Whether activation is a single or double click (default `DoubleClick`).
    activate_on: crate::data_views::ActivateOn,

    /// `true` while this view (root or descendant) holds keyboard focus — the
    /// root's inclusive [`BuildContext::view_focus_active`](bastyde_core::BuildContext::view_focus_active) signal, bound
    /// `RepaintOnly`. With [`focus_visible`](Self::focus_visible) it drives the
    /// **container focus ring**: when the view is Tab-focused but nothing is
    /// selected, no row ring shows, so the whole view outlines itself instead —
    /// the user can see where keyboard focus landed before they arrow.
    view_focused: Signal<bool>,
    /// Input-modality `:focus-visible`. Gates the container ring (and row rings)
    /// to keyboard navigation, never a mouse click. Bound `RepaintOnly`.
    focus_visible: Signal<bool>,

    // Persistent scroll state
    scroll_y: Signal<f32>,
    max_scroll_y: Signal<f32>,
    /// Scroll-chaining behavior at the boundary (default `Chain`).
    overscroll_behavior: OverscrollBehavior,
    viewport_ratio_y: Signal<f32>,

    /// Animate wheel scrolling instead of snapping to the new offset.
    /// Enabled by default — mirrors `ScrollArea`. Without it, each wheel
    /// notch jumps by `item_height` per delivered line (typically 3),
    /// which reads as a coarse multi-row jump rather than a smooth glide.
    smooth_scrolling: bool,
    /// Duration of the smooth scroll animation.
    smooth_scroll_duration: Duration,

    /// How the scroll bar is displayed. Defaults to `Permanent` — a
    /// layout sibling that reserves its own width. `Overlay` / `Thin`
    /// float over the content instead, like `ScrollArea`.
    scroll_bar_style: ScrollBarMode,

    /// Rebuild trigger. A persistent field (re-bound each build) so
    /// `place_children`'s post-measure realization re-check can request
    /// a rebuild when corrected offsets reveal unrealized viewport rows.
    version: Signal<u64>,
    /// Buffered row range materialized by the latest build.
    prev_built_start: Rc<Cell<usize>>,
    prev_built_end: Rc<Cell<usize>>,

    // Set during build
    item_entries: Vec<(usize, WidgetId)>, // (flat_index, widget_id)
    scrollbar_id: Option<WidgetId>,
    viewport_height: Rc<Cell<f32>>,
    /// The TreeView's own absolute (window) bounds, cached from
    /// `place_children` so the keyboard handler can chase the selected row
    /// into enclosing scroll areas via
    /// [`EventContext::ensure_visible`](bastyde_core::widget::EventContext::ensure_visible).
    /// Rows are not distinct focusable nodes, so the focus-driven follow never
    /// reveals the selected row in an outer scroller — this closes that gap.
    viewport_bounds: Rc<Cell<Rect>>,
    tree_id: ViewId,

    /// Whole-view enabled state, statically or reactively. Forwarded to the
    /// arena via `ctx.enabled_when(self_id, self.enabled.clone())` at build
    /// time; a disabled view greys out and stops accepting focus /
    /// selection / keyboard input (arena-gated).
    enabled: Prop<bool>,
}

mod builder;
mod widget_impl;

// std::fmt::Debug for the (non-Debug) generic fields.
impl<T: 'static> std::fmt::Debug for TreeView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreeView")
            .field("visible_count", &self.source.visible_count())
            .field("item_height", &self.item_height)
            .field("scroll_bar_style", &self.scroll_bar_style)
            .field("scroll_y", &self.scroll_y.get())
            .finish()
    }
}

#[cfg(test)]
mod tests;
