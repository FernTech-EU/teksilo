// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The [`Scene`] data model — the owner of all items in a pannable/zoomable
//! scene.
//!
//! `Scene` holds a flat list of entries in a parent-relative scene-graph, plus
//! a pluggable [`SpatialIndex`] for rectangular queries. Items are positioned
//! by `local_pos` (in their parent's coordinate frame, or scene-root if they
//! have none) and an optional `transform` (rotation/scale around the local
//! origin); the Scene composes those up the parent chain to derive each item's
//! `scene_transform` and axis-aligned bounding box for hit-test, paint, and
//! culling. Two content tiers coexist in one `Scene`: heavyweight `Widget`s
//! (full focus/animation/DnD/AT — placed at scene coordinates) and lightweight
//! [`SceneItem`]s (paint-only, no arena overhead, thousands
//! cheap). All mutations update the [`SpatialIndex`] in lockstep, so
//! [`Scene::items_in_rect`] and [`Scene::item_at`] stay `O(visible)`.
//!
//! `Scene` is rarely used directly. The normal entry point is
//! [`SceneModel`](crate::SceneModel), a cloneable `Rc<RefCell<Scene>>` handle
//! with `&self` mutators (the `ListModel` pattern) that lets multiple handlers
//! and multiple [`SceneView`](crate::SceneView)s share one model.
//!
//! ## When to use
//!
//! Use `Scene` (via `SceneModel`) when you need a pannable/zoomable canvas —
//! story corkboards, node-graph editors, mind maps, timeline views, CAD
//! canvases, or simple spatial maps. Prefer a plain `ListView` or `TreeView`
//! when the content is linear or tree-shaped without spatial relationships.
//!
//! ## Example
//!
//! ```rust
//! use teksilo_scene::{Scene, ItemChange, SceneLayer};
//! use teksilo_scene::{RectItem, ItemId};
//! use teksilo_canvas::{Point, Rect};
//! use teksilo_tokens::Color;
//!
//! let mut scene = Scene::new();
//!
//! // Add a lightweight rectangle item at scene coordinates (50, 50).
//! let id: ItemId = scene.add_item(
//!     RectItem::new(Rect::new(0.0, 0.0, 80.0, 40.0)).fill(Color::BLUE),
//!     Point::new(50.0, 50.0),
//! );
//!
//! // Observe every mutation — fires after the change is already applied.
//! let _guard = scene.item_change_signal().observe(|change| {
//!     if let ItemChange::LocalPosChanged { id: _, old: _, new } = change {
//!         let _ = new; // react to the new position
//!     }
//! });
//!
//! // Move the item; the observer fires and the spatial index updates.
//! scene.set_local_pos(id, Point::new(100.0, 100.0));
//! assert_eq!(scene.scene_pos(id), Some(Point::new(100.0, 100.0)));
//! ```

use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use crate::a11y::{A11yCategory, A11yGroup, A11yGroupBuilder, A11yGroupId, A11yNode, A11yRelation};
use crate::flags::ItemFlags;
use crate::index::{GridHashIndex, SpatialIndex};
use crate::item::{ItemId, SceneItem};
use crate::item_handlers::SceneItemHandlerSet;
use crate::magnet::{Magnet, MagnetId, MagnetRef, MagnetSnap, MagnetVerdict};
use crate::transform::local_to_parent;
use teksilo_canvas::{Path, Point, Rect, StrokeStyle, Transform2D, Vec2};
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;

/// A change to an item's state, fired through
/// [`Scene::item_change_signal`] for every mutation. Apps observe
/// to wire snap-to-grid, validation, side effects, etc. The model
/// is "fire after the change has been applied" — by the time the
/// observer sees the event, the Scene already reflects it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ItemChange {
    /// `set_local_pos`: position in parent coords moved.
    LocalPosChanged { id: ItemId, old: Point, new: Point },
    /// `set_local_bounds`: AABB in local coords changed.
    LocalBoundsChanged { id: ItemId, old: Rect, new: Rect },
    /// `set_transform`: local→parent transform changed.
    TransformChanged { id: ItemId },
    /// `set_visible` flipped IS_VISIBLE.
    VisibilityChanged { id: ItemId, visible: bool },
    /// `set_flags` / `set_flag` changed the bitset.
    FlagsChanged {
        id: ItemId,
        old: ItemFlags,
        new: ItemFlags,
    },
    /// `set_opacity`: local opacity multiplier changed.
    OpacityChanged { id: ItemId, old: f32, new: f32 },
    /// `set_z`: paint z-order changed.
    ZChanged { id: ItemId, old: f32, new: f32 },
    /// `set_layer`: the Under/Over paint band changed.
    LayerChanged {
        id: ItemId,
        old: SceneLayer,
        new: SceneLayer,
    },
    /// `set_item_parent`: logical parent changed.
    ParentChanged {
        id: ItemId,
        old: Option<ItemId>,
        new: Option<ItemId>,
    },
    /// `remove`: item is gone.
    Removed { id: ItemId },
    /// `add_item` / `add_widget`: item was inserted.
    Added { id: ItemId },
    /// `set_payload`: the type-erased payload of a `Delegated` heavyweight
    /// entry was replaced. A `SceneView` rebuilds that entry's widget
    /// (re-invokes its delegate) on the next build. Routed through
    /// `emit_item_change`, so `mutation_seq` advances and the AT-walk gate
    /// notices.
    PayloadChanged { id: ItemId },
    /// `set_item_fill` / `set_item_stroke` / `clear_item_*`: a lightweight
    /// item's paint-only appearance (fill / stroke colour or style) changed.
    /// Never moves geometry, so the observing `SceneView` evicts the item's
    /// cached frame and repaints **without** relayout or rebuild.
    AppearanceChanged { id: ItemId },
}

/// Which paint band a lightweight [`SceneItem`] sits in, relative to
/// the heavyweight widget tier.
///
/// A `SceneView` paints in three passes: lightweight `Under` items
/// (its `paint`, a backdrop), then the heavyweight widget children
/// (the arena child-walk), then lightweight `Over` items (its
/// `post_paint`, a foreground). Within each band, `z` still orders
/// items among themselves.
///
/// This is a binary band, not a continuous z across the tiers, because
/// the render walker offers exactly two lightweight paint positions
/// (before and after the child subtree). The heavyweight tier is one
/// contiguous block in between — to interleave a lightweight item
/// *between* two specific heavyweight nodes you must promote it to a
/// heavyweight widget. `Under` is the default (background furniture:
/// connectors, grids, decorations); `Over` is for foreground overlays
/// that must sit above the cards (selection halos, highlighted edges).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SceneLayer {
    /// Painted under the heavyweight widget children (the default).
    #[default]
    Under,
    /// Painted over the heavyweight widget children.
    Over,
}

/// Which axes a [`SceneView`](crate::SceneView) is allowed to pan
/// along. Set on the [`Scene`] (not the View) because a given scene
/// model often makes sense at one orientation only — a horizontal
/// timeline, a vertical timeline, a fixed-extent diagram. All views
/// of the same scene inherit the constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanAxes {
    /// No user-driven pan in either axis. Programmatic
    /// [`SceneView::set_pan`](crate::SceneView::set_pan) /
    /// [`pan_to`](crate::SceneView::pan_to) become no-ops too.
    None,
    /// Pan only along X. Vertical scroll deltas pass through to
    /// ancestor scrollables.
    Horizontal,
    /// Pan only along Y. Horizontal scroll deltas pass through to
    /// ancestor scrollables.
    Vertical,
    /// Default: pan freely in both axes.
    #[default]
    Both,
}

/// Reactive interaction-policy bundle owned by [`Scene`]. Apps
/// configure pan/zoom behaviour by writing to these signals; gesture
/// closures in [`SceneView`](crate::SceneView) read them live, so
/// runtime mode switches (e.g. a toolbar toggling pan locks) take
/// effect on the next event without rebuilding the view.
///
/// All four signals are exposed individually via [`Scene`] accessors
/// (`pan_axes_signal`, `pan_bounds_signal`, `zoom_range_signal`,
/// `zoomable_signal`). Per-(sub-)scene independence falls out of the
/// model: each nested `SceneView` carries its own `Scene` with its
/// own `SceneConstraints`.
///
/// View-level *tightening* overrides (`pan_bounds_override`,
/// `zoom_range_override`) layer on top per-`SceneView` — the
/// effective constraint is the intersection. Two views over the
/// same `Scene` can lock down independently; neither can loosen
/// what the `Scene` declares.
pub struct SceneConstraints {
    pan_axes: Signal<PanAxes>,
    /// Scene-coord rectangle that the visible viewport must stay
    /// inside. `None` (default) = unconstrained. When `Some(r)`,
    /// pan is clamped so the visible scene region overlaps the
    /// rect; when the viewport is bigger than the rect, the rect
    /// is centered.
    pan_bounds: Signal<Option<Rect>>,
    /// Inclusive `[min, max]` clamp on zoom factor. `None` =
    /// unconstrained from the `Scene` side (the `SceneView` may
    /// still impose its own range override).
    zoom_range: Signal<Option<std::ops::RangeInclusive<f32>>>,
    zoomable: Signal<bool>,
}

impl SceneConstraints {
    fn new() -> Self {
        Self {
            pan_axes: Signal::new(PanAxes::Both),
            pan_bounds: Signal::new(None),
            zoom_range: Signal::new(None),
            zoomable: Signal::new(true),
        }
    }

    /// Reactive pan-axes signal. Gesture handlers read live.
    pub fn pan_axes_signal(&self) -> Signal<PanAxes> {
        self.pan_axes.clone()
    }
    /// Reactive pan-bounds signal. `None` = unconstrained.
    pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>> {
        self.pan_bounds.clone()
    }
    /// Reactive zoom-range signal. `None` = unconstrained from
    /// the Scene side.
    pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>> {
        self.zoom_range.clone()
    }
    /// Reactive zoomable-on/off signal. Equivalent to a zero-width
    /// zoom_range — kept as a separate boolean for clarity and
    /// efficient short-circuit at gesture time.
    pub fn zoomable_signal(&self) -> Signal<bool> {
        self.zoomable.clone()
    }
}

impl Default for SceneConstraints {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SceneConstraints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneConstraints")
            .field("pan_axes", &self.pan_axes.get())
            .field("pan_bounds", &self.pan_bounds.get())
            .field("zoom_range", &self.zoom_range.get())
            .field("zoomable", &self.zoomable.get())
            .finish()
    }
}

/// A single entry in a [`Scene`]. The two variants mirror the two
/// content tiers: heavyweight `Widget`s consumed into the arena at
/// build time, and lightweight `SceneItem`s painted directly from the
/// SceneView's paint walk.
pub(crate) struct SceneEntry {
    pub(crate) id: ItemId,
    /// Origin of the item's local coordinate frame, in **parent**
    /// coordinates (or scene coords if `parent == None`).
    pub(crate) local_pos: Point,
    /// Item's AABB in **local** coordinates. For lightweight items
    /// this is read once at insert time from `SceneItem::local_bounds`
    /// and tracked through `Scene::set_local_bounds`. For widgets it
    /// records the size requested at `add_widget` time, anchored at
    /// the origin: `Rect::new(0, 0, w, h)`.
    pub(crate) local_bounds: Rect,
    /// Optional rotation/scale applied around the local origin
    /// before translating by `local_pos`. Default identity.
    pub(crate) transform: Transform2D,
    pub(crate) kind: SceneEntryKind,
    /// Z-order for paint — higher values paint *later* (on top).
    /// Equal-z entries fall back to insertion order. Applies to **both**
    /// tiers: lightweight items sort within their band each paint, and
    /// heavyweight widget entries restack the SceneView's arena children
    /// by z on the next rebuild (see [`Scene::set_z`]).
    pub(crate) z: f32,
    /// Which lightweight paint band this item sits in relative to the
    /// heavyweight tier — [`SceneLayer::Under`] (default, backdrop) or
    /// [`SceneLayer::Over`] (foreground). Lightweight tier only; ignored
    /// for heavyweight widget entries (they paint via the arena).
    pub(crate) layer: SceneLayer,
    /// Logical parent. `None` means the item is rooted directly in
    /// the Scene. Composes coordinate frames: a child's `local_pos`
    /// is in the parent's local frame, and the child's
    /// `scene_transform` is the parent's `scene_transform` composed
    /// with the child's `local_to_parent`.
    pub(crate) parent: Option<ItemId>,
    /// Per-item behavior flags. Read once from
    /// [`SceneItem::initial_flags`] at insert time, mutable through
    /// [`Scene::set_flags`] / [`Scene::set_flag`].
    pub(crate) flags: ItemFlags,
    /// Multiplicative opacity in `[0.0, 1.0]`. Composes through the
    /// parent chain: an item's `effective_opacity` is the product
    /// of every ancestor's opacity and its own.
    pub(crate) opacity: f32,
    /// Per-item event handlers, cursor and tooltip overrides.
    /// `None` until the app calls `Scene::set_item_handlers` /
    /// `Scene::handlers_mut`.
    pub(crate) handlers: Option<Box<SceneItemHandlerSet>>,
    /// Whether the item's `local_bounds` may change between
    /// build/layout passes (a signal-driven AABB). Static items
    /// (default) snapshot bounds at insert and only update through
    /// explicit [`Scene::set_local_bounds`]. Dynamic items added via
    /// [`Scene::add_item_dynamic`] have their `local_bounds` re-read
    /// each rebuild via [`Scene::refresh_dynamic_bounds`], with the
    /// spatial index re-bucketed when the value changes.
    pub(crate) dynamic_bounds: bool,
}

/// How a heavyweight `Widget` entry makes its instance available to a
/// `SceneView`. The two variants are the single-view and multi-view
/// production paths.
pub(crate) enum WidgetSource {
    /// Single-view sugar ([`Scene::add_widget`]). The first `SceneView` to
    /// build drains the `Option` via `take()`; subsequent views (sharing the
    /// same [`SceneModel`](crate::SceneModel)) find `None` and produce no
    /// arena child for this entry. Use [`Scene::add_widget_delegated`] +
    /// a view delegate for multi-view content.
    Once(Option<Box<dyn Widget>>),
    /// Multi-view path ([`Scene::add_widget_delegated`], surfaced as
    /// [`SceneModel::add_widget_item`](crate::SceneModel::add_widget_item)).
    /// Each view calls its own delegate with this type-erased `payload`
    /// to build a fresh `Widget` instance. The payload is `Rc` so a view
    /// can clone it out of a model borrow before invoking the delegate.
    Delegated { payload: Rc<dyn std::any::Any> },
}

pub(crate) enum SceneEntryKind {
    /// A heavyweight `Widget` materialised into the arena, via either the
    /// single-view `Once` slot or the multi-view `Delegated` payload.
    Widget(WidgetSource),
    /// A lightweight `SceneItem` that lives in the scene
    /// permanently; painted by the SceneView's paint walk.
    Item(Box<dyn SceneItem>),
}

/// The data model behind a `SceneView`: a flat list of entries in a
/// parent-relative scene-graph plus a [`SpatialIndex`] for rectangular
/// queries.
///
/// The Scene itself does no rendering — it's a passive container the view
/// reads from at build / place / paint time. Mutations (`add_widget`,
/// `add_item`, `set_local_pos`, `set_transform`, `set_local_bounds`, `remove`)
/// update the spatial index in lockstep, so `items_in_rect`, `item_at`, and
/// SceneView's viewport-cull path are all `O(visible)` instead of `O(N)`. When
/// a parent's `local_pos` or `transform` changes, every descendant's
/// scene-AABB shifts; the Scene re-buckets the entire subtree.
///
/// In practice most callers operate on a [`SceneModel`](crate::SceneModel)
/// handle (`Rc<RefCell<Scene>>` with `&self` mutators) rather than a bare
/// `Scene`. Prefer `SceneModel` for any widget or handler that needs to share
/// the scene across multiple owners.
pub struct Scene {
    pub(crate) entries: Vec<SceneEntry>,
    /// `ItemId` → index into `entries` for O(1) lookup.
    entry_index: HashMap<ItemId, usize>,
    index: Box<dyn SpatialIndex>,

    /// User-declared scene extent. `None` means "auto-compute from
    /// items each query". Set via [`Scene::set_scene_rect`]. Used
    /// by [`SceneView::adopt_scene_size`](crate::SceneView::adopt_scene_size).
    /// (Distinct from `constraints.pan_bounds` which clamps the
    /// visible viewport.)
    user_scene_rect: Option<Rect>,
    /// Reactive interaction policy: pan axes, pan bounds, zoom
    /// range, zoomable on/off. Apps mutate via the dedicated
    /// `Scene::pan_axes` / `set_pan_bounds` / `set_zoom_range` /
    /// `zoomable` methods (still classic mutator shape) or read
    /// the underlying signals via the `*_signal` accessors for
    /// live observation.
    constraints: SceneConstraints,
    /// Reactive change signal. Every mutation fires an
    /// [`ItemChange`] through this signal so apps can observe
    /// geometry / visibility / parent / z / opacity changes.
    item_change_signal: Signal<ItemChange>,
    /// Reactive change counter for the *logical AT structure* (groups,
    /// parents, relations, live, landmarks, categories). These mutations are
    /// not item geometry, so they do not flow through `item_change_signal`;
    /// `SceneView` observes this separately to re-walk the AccessKit tree. The
    /// AT tree is fully separate from the visual scene, so it needs its own
    /// notification channel.
    a11y_change_signal: Signal<u64>,
    /// Monotonic counter of *every* model mutation — item geometry / visibility
    /// / structure (each [`ItemChange`] fire) **and** logical-AT structure (each
    /// `bump_a11y_change`). Read via [`Scene::mutation_version`]. `SceneView`
    /// gates its (expensive) AccessKit re-walk on this advancing, so a `build()`
    /// triggered purely by dynamic-bounds churn it already accounted for doesn't
    /// re-walk the AT tree every frame. A plain `Cell` because the bump path
    /// (`bump_mutation`) is `&self` (shared with `bump_a11y_change`).
    mutation_seq: Cell<u64>,

    // --- logical AT structure ----------------------------------------
    pub(crate) a11y_groups: Vec<A11yGroup>,
    pub(crate) a11y_group_index: HashMap<A11yGroupId, usize>,
    pub(crate) a11y_parents: HashMap<A11yNode, A11yNode>,
    pub(crate) a11y_relations: Vec<(A11yNode, A11yRelation, A11yNode)>,
    pub(crate) a11y_live: HashMap<A11yNode, accesskit::Live>,
    pub(crate) a11y_landmarks: HashMap<A11yNode, accesskit::Role>,
    pub(crate) a11y_categories: HashMap<A11yNode, Vec<A11yCategory>>,

    // --- magnetism ---------------------------------------------------
    /// Magnets attached to each item, in insertion order. Kept in a
    /// side map (not on `SceneEntry`) so the magnet subsystem is
    /// modular — the same shape as the logical-AT maps above.
    magnets: HashMap<ItemId, Vec<(MagnetId, Magnet)>>,
    /// Reverse lookup `MagnetId -> owning ItemId` for O(1) resolution
    /// of a magnet's owner (and cleanup on `remove_magnet`).
    magnet_owner: HashMap<MagnetId, ItemId>,
}

impl Scene {
    /// An empty scene with the default [`GridHashIndex`].
    pub fn new() -> Self {
        Self::with_index(Box::new(GridHashIndex::default()))
    }

    /// An empty scene with a custom [`SpatialIndex`].
    pub fn with_index(index: Box<dyn SpatialIndex>) -> Self {
        Self {
            entries: Vec::new(),
            entry_index: HashMap::new(),
            index,
            user_scene_rect: None,
            constraints: SceneConstraints::new(),
            item_change_signal: Signal::new(ItemChange::Added { id: ItemId(0) }),
            a11y_change_signal: Signal::new(0),
            mutation_seq: Cell::new(0),
            a11y_groups: Vec::new(),
            a11y_group_index: HashMap::new(),
            a11y_parents: HashMap::new(),
            a11y_relations: Vec::new(),
            a11y_live: HashMap::new(),
            a11y_landmarks: HashMap::new(),
            a11y_categories: HashMap::new(),
            magnets: HashMap::new(),
            magnet_owner: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------
    // Insertion
    // -----------------------------------------------------------------

    /// Place a heavyweight `Widget` at `local_rect`'s origin, sized
    /// `local_rect.size`. The rect is interpreted as
    /// `(local_pos = local_rect.origin, local_bounds = (0, 0, w, h))`.
    /// Returns the [`ItemId`] for later mutation. The widget is
    /// consumed at SceneView build time and added to the arena.
    pub fn add_widget<W: Widget + 'static>(&mut self, widget: W, local_rect: Rect) -> ItemId {
        let id = ItemId::next();
        let local_pos = Point::new(local_rect.x, local_rect.y);
        let local_bounds = Rect::new(0.0, 0.0, local_rect.width, local_rect.height);
        let entry = SceneEntry {
            id,
            local_pos,
            local_bounds,
            transform: Transform2D::identity(),
            kind: SceneEntryKind::Widget(WidgetSource::Once(Some(Box::new(widget)))),
            z: 0.0,
            layer: SceneLayer::Under,
            parent: None,
            flags: ItemFlags::default(),
            opacity: 1.0,
            handlers: None,
            dynamic_bounds: false,
        };
        self.push_entry(entry)
    }

    /// Multi-view heavyweight insertion: store a type-erased `payload`; each
    /// [`SceneView`](crate::SceneView) builds its own instance via its
    /// delegate. Surfaced publicly as
    /// [`SceneModel::add_widget_item`](crate::SceneModel::add_widget_item).
    pub(crate) fn add_widget_delegated(
        &mut self,
        payload: Rc<dyn std::any::Any>,
        local_rect: Rect,
    ) -> ItemId {
        let id = ItemId::next();
        let local_pos = Point::new(local_rect.x, local_rect.y);
        let local_bounds = Rect::new(0.0, 0.0, local_rect.width, local_rect.height);
        let entry = SceneEntry {
            id,
            local_pos,
            local_bounds,
            transform: Transform2D::identity(),
            kind: SceneEntryKind::Widget(WidgetSource::Delegated { payload }),
            z: 0.0,
            layer: SceneLayer::Under,
            parent: None,
            flags: ItemFlags::default(),
            opacity: 1.0,
            handlers: None,
            dynamic_bounds: false,
        };
        self.push_entry(entry)
    }

    /// Replace the type-erased payload of a `Delegated` heavyweight entry and
    /// fire [`ItemChange::PayloadChanged`].
    ///
    /// # Panics
    ///
    /// Panics if `id` is unknown, refers to a `Once` widget entry, or refers to
    /// a lightweight item. These are all caller-side precondition violations:
    /// the caller obtained `id` from `add_widget_item` and is responsible for
    /// only passing it back to `set_payload` while the entry is alive.
    pub(crate) fn set_payload(&mut self, id: ItemId, payload: Rc<dyn std::any::Any>) {
        let Some(&pos) = self.entry_index.get(&id) else {
            panic!("set_payload: unknown ItemId {id:?}");
        };
        match &mut self.entries[pos].kind {
            SceneEntryKind::Widget(WidgetSource::Delegated { payload: slot }) => *slot = payload,
            _ => panic!("set_payload: {id:?} is not a Delegated widget entry"),
        }
        // Entry borrow dropped above; `emit_item_change` is `&self`.
        self.emit_item_change(ItemChange::PayloadChanged { id });
    }

    /// The current type-erased payload of a `Delegated` heavyweight entry.
    /// `None` for unknown ids, `Once` widget entries, and lightweight items.
    pub(crate) fn payload(&self, id: ItemId) -> Option<Rc<dyn std::any::Any>> {
        let pos = *self.entry_index.get(&id)?;
        match &self.entries[pos].kind {
            SceneEntryKind::Widget(WidgetSource::Delegated { payload }) => Some(payload.clone()),
            _ => None,
        }
    }

    /// Drain every still-pending `Once` heavyweight widget, in entry order.
    /// Each is `take()`n from its slot, so a second `SceneView` over the same
    /// model returns nothing for it — `Once` widgets are single-view. Called
    /// by `SceneView::build`.
    pub(crate) fn drain_all_once(&mut self) -> Vec<(ItemId, Box<dyn Widget>)> {
        let mut out = Vec::new();
        for entry in self.entries.iter_mut() {
            if let SceneEntryKind::Widget(WidgetSource::Once(pending)) = &mut entry.kind
                && let Some(w) = pending.take()
            {
                out.push((entry.id, w));
            }
        }
        out
    }

    /// `(id, payload)` for every `Delegated` heavyweight entry, in entry order.
    /// The payload `Rc` is cloned so the caller can drop the model borrow before
    /// invoking its delegate (the reentrancy contract). Called by `SceneView::build`.
    pub(crate) fn delegated_payloads(&self) -> Vec<(ItemId, Rc<dyn std::any::Any>)> {
        self.entries
            .iter()
            .filter_map(|e| match &e.kind {
                SceneEntryKind::Widget(WidgetSource::Delegated { payload }) => {
                    Some((e.id, payload.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Ids of every heavyweight `Widget` entry (`Once` and `Delegated`), in
    /// entry order. Used by `SceneView::build` for child ordering and the
    /// orphan-reap live-set.
    pub(crate) fn heavyweight_ids(&self) -> Vec<ItemId> {
        self.entries
            .iter()
            .filter_map(|e| match &e.kind {
                SceneEntryKind::Widget(_) => Some(e.id),
                SceneEntryKind::Item(_) => None,
            })
            .collect()
    }

    /// Place a lightweight [`SceneItem`] at `local_pos`. The item's
    /// `local_bounds` and `initial_flags` are read once at insert
    /// time. The item is **not** added to the arena — it's painted
    /// directly from `SceneView::paint`.
    pub fn add_item<I: SceneItem + 'static>(&mut self, item: I, local_pos: Point) -> ItemId {
        self.add_item_inner(item, local_pos, false)
    }

    /// Like [`add_item`](Self::add_item) but flags the entry as
    /// having signal-driven `local_bounds`. The Scene re-reads
    /// `item.local_bounds()` each rebuild via
    /// [`refresh_dynamic_bounds`](Self::refresh_dynamic_bounds) — the
    /// SceneView calls that at the start of every build pass. The
    /// spatial index gets re-bucketed when the read-back differs
    /// from the cached value, so `items_in_rect` / hit-test stay
    /// correct without app-side `set_local_bounds` plumbing.
    ///
    /// Use only when the bounds genuinely depend on a `Signal<T>`
    /// the item reads in `local_bounds`. Static items pay an
    /// unnecessary per-rebuild bounds read otherwise; prefer
    /// [`add_item`](Self::add_item) for the common case.
    pub fn add_item_dynamic<I: SceneItem + 'static>(
        &mut self,
        item: I,
        local_pos: Point,
    ) -> ItemId {
        self.add_item_inner(item, local_pos, true)
    }

    fn add_item_inner<I: SceneItem + 'static>(
        &mut self,
        item: I,
        local_pos: Point,
        dynamic_bounds: bool,
    ) -> ItemId {
        self.insert_boxed(Box::new(item), local_pos, dynamic_bounds)
    }

    /// The single lightweight-entry construction site, shared by the generic
    /// [`add_item`](Self::add_item) / [`add_item_dynamic`](Self::add_item_dynamic)
    /// path (via `add_item_inner`) and the boxed-`dyn`
    /// [`add_boxed_item`](Self::add_boxed_item) path, so a future `SceneEntry`
    /// field can't be added to one and silently missed by the other.
    fn insert_boxed(
        &mut self,
        item: Box<dyn SceneItem>,
        local_pos: Point,
        dynamic_bounds: bool,
    ) -> ItemId {
        let id = ItemId::next();
        let local_bounds = item.local_bounds();
        let flags = item.initial_flags();
        let entry = SceneEntry {
            id,
            local_pos,
            local_bounds,
            transform: Transform2D::identity(),
            kind: SceneEntryKind::Item(item),
            z: 0.0,
            layer: SceneLayer::Under,
            parent: None,
            flags,
            opacity: 1.0,
            handlers: None,
            dynamic_bounds,
        };
        self.push_entry(entry)
    }

    /// Re-read every dynamic item's current `local_bounds`, applying
    /// `set_local_bounds` (and re-bucketing the spatial index) for
    /// any entry whose value has changed. No-op for static entries.
    /// Called by [`SceneView`](crate::SceneView) at the start of each
    /// `build()` so signal-driven bounds propagate to bucketing
    /// without explicit app-side calls.
    ///
    /// Returns `true` if at least one dynamic entry's bounds changed this call.
    /// `SceneView` uses the `true → false` transition (an animation settling) as
    /// the one moment to walk the final animated bounds into the AccessKit tree,
    /// since it otherwise suppresses per-frame AT re-walks during the animation.
    pub fn refresh_dynamic_bounds(&mut self) -> bool {
        // Snapshot ids first to avoid borrow conflicts.
        let dynamic_ids: Vec<ItemId> = self
            .entries
            .iter()
            .filter(|e| e.dynamic_bounds)
            .map(|e| e.id)
            .collect();
        let mut changed = false;
        for id in dynamic_ids {
            let Some(&pos) = self.entry_index.get(&id) else {
                continue;
            };
            let SceneEntryKind::Item(item) = &self.entries[pos].kind else {
                continue;
            };
            let new = item.local_bounds();
            if new != self.entries[pos].local_bounds {
                self.set_local_bounds(id, new);
                changed = true;
            }
        }
        changed
    }

    fn push_entry(&mut self, entry: SceneEntry) -> ItemId {
        let id = entry.id;
        let pos = self.entries.len();
        self.entries.push(entry);
        self.entry_index.insert(id, pos);
        let aabb = self.compute_scene_aabb(id).unwrap_or(Rect::ZERO);
        self.index.insert(id, aabb);
        self.emit_item_change(ItemChange::Added { id });
        id
    }

    /// Reactive notification stream for every Scene mutation. Apps
    /// observe via `signal.observe(|change| …)` to wire snap-to-grid,
    /// clamping, validation, and side effects without having to
    /// poll the Scene each frame. The signal fires *after* the
    /// mutation has been applied — by the time the observer runs
    /// the Scene already reflects the new state.
    pub fn item_change_signal(&self) -> Signal<ItemChange> {
        self.item_change_signal.clone()
    }

    /// Reactive notification for logical-AT-structure mutations
    /// (`add_a11y_group` / `remove_a11y_group` / `set_a11y_parent` /
    /// `add_a11y_relation` / `set_a11y_live` / `set_a11y_landmark` /
    /// `set_a11y_categories`). A monotonic counter bumped after each such
    /// mutation. `SceneView` observes this to re-walk the AccessKit tree —
    /// these changes don't flow through [`item_change_signal`](Self::item_change_signal)
    /// because they aren't item geometry, and the AT tree is separate from the
    /// visual scene.
    pub fn a11y_change_signal(&self) -> Signal<u64> {
        self.a11y_change_signal.clone()
    }

    /// Bump the logical-AT-structure change counter. Called at the end of every
    /// a11y-structure mutator so observers re-walk AccessKit. Also advances the
    /// unified [`mutation_version`](Self::mutation_version) so a logical-AT
    /// mutation (which never fires `item_change_signal`) still un-gates the
    /// SceneView's AT re-walk.
    fn bump_a11y_change(&self) {
        self.a11y_change_signal
            .set(self.a11y_change_signal.get().wrapping_add(1));
        self.bump_mutation();
    }

    /// Fire an [`ItemChange`] through `item_change_signal` and advance the
    /// unified [`mutation_version`](Self::mutation_version). The single choke
    /// point every geometry / visibility / structure mutation routes through, so
    /// the version counts each one without per-site bookkeeping.
    fn emit_item_change(&self, change: ItemChange) {
        self.bump_mutation();
        self.item_change_signal.set(change);
    }

    /// Advance the unified model-mutation counter (wrapping). Shared by
    /// `emit_item_change` and `bump_a11y_change`; `&self` because both notify
    /// paths are `&self`.
    fn bump_mutation(&self) {
        self.mutation_seq
            .set(self.mutation_seq.get().wrapping_add(1));
    }

    /// Monotonic counter of every model mutation applied so far — item geometry
    /// / visibility / structure (each [`ItemChange`]) **and** logical-AT
    /// structure (groups, parents, relations, live, landmarks, categories).
    ///
    /// [`SceneView`](crate::SceneView) snapshots this each `build()` and only
    /// re-walks the (separate, expensive) AccessKit tree when it has advanced
    /// since the previous walk — so an actively-animating
    /// [`add_item_dynamic`](Self::add_item_dynamic) item, which rebuilds every
    /// frame, does not issue an AT re-walk per frame. The counter wraps; compare
    /// for equality, not ordering.
    pub fn mutation_version(&self) -> u64 {
        self.mutation_seq.get()
    }

    // -----------------------------------------------------------------
    // Geometry — local
    // -----------------------------------------------------------------

    /// Read an item's `local_pos` (its anchor in parent coords).
    pub fn local_pos(&self, id: ItemId) -> Option<Point> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].local_pos)
    }

    /// Move an item to a new `local_pos` in its parent's coordinate
    /// frame. Re-buckets the item *and* every descendant in the
    /// spatial index since the descendants' scene-AABBs shift along.
    /// No-op if the id is unknown.
    pub fn set_local_pos(&mut self, id: ItemId, local_pos: Point) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].local_pos;
            if old == local_pos {
                return;
            }
            self.entries[pos].local_pos = local_pos;
            self.rebucket_subtree(id);
            self.emit_item_change(ItemChange::LocalPosChanged {
                id,
                old,
                new: local_pos,
            });
        }
    }

    /// Read an item's `local_bounds` (its AABB in local coords).
    pub fn local_bounds(&self, id: ItemId) -> Option<Rect> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].local_bounds)
    }

    /// Update an item's `local_bounds`. For lightweight items this
    /// also calls [`SceneItem::set_local_bounds`] on the item so its
    /// next `paint` reflects the new geometry. The spatial index is
    /// re-bucketed; only this item moves (descendants' local frames
    /// are unchanged). No-op if the id is unknown.
    pub fn set_local_bounds(&mut self, id: ItemId, local_bounds: Rect) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].local_bounds;
            if old == local_bounds {
                return;
            }
            self.entries[pos].local_bounds = local_bounds;
            if let SceneEntryKind::Item(item) = &mut self.entries[pos].kind {
                item.set_local_bounds(local_bounds);
            }
            // Bounds are local — only this entry's scene-AABB shifts;
            // descendants' local frames are unchanged.
            let aabb = self.compute_scene_aabb(id).unwrap_or(Rect::ZERO);
            self.index.insert(id, aabb);
            self.emit_item_change(ItemChange::LocalBoundsChanged {
                id,
                old,
                new: local_bounds,
            });
        }
    }

    /// Read an item's local→parent transform (rotation/scale around
    /// the local origin). Identity by default.
    pub fn transform(&self, id: ItemId) -> Option<Transform2D> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].transform)
    }

    /// Set an item's local→parent transform. Re-buckets the item's
    /// subtree in the spatial index. No-op if the id is unknown.
    pub fn set_transform(&mut self, id: ItemId, transform: Transform2D) {
        if let Some(&pos) = self.entry_index.get(&id) {
            self.entries[pos].transform = transform;
            self.rebucket_subtree(id);
            self.emit_item_change(ItemChange::TransformChanged { id });
        }
    }

    // -----------------------------------------------------------------
    // Geometry — scene (computed via parent chain)
    // -----------------------------------------------------------------

    /// The composed local→scene transform for this item, walking up
    /// the parent chain. Identity for an item that doesn't exist.
    pub fn scene_transform(&self, id: ItemId) -> Transform2D {
        let mut acc = Transform2D::identity();
        let mut cur = Some(id);
        let cap = self.entries.len();
        let mut hops = 0;
        while let Some(cid) = cur {
            let Some(&pos) = self.entry_index.get(&cid) else {
                break;
            };
            let entry = &self.entries[pos];
            let l2p = local_to_parent(entry.local_pos, &entry.transform);
            acc = acc.then(&l2p);
            cur = entry.parent;
            hops += 1;
            if hops > cap {
                break;
            }
        }
        acc
    }

    /// The item's anchor in scene coords (its local origin
    /// transformed through the parent chain).
    pub fn scene_pos(&self, id: ItemId) -> Option<Point> {
        if !self.entry_index.contains_key(&id) {
            return None;
        }
        Some(self.scene_transform(id).apply_point(Point::ZERO))
    }

    /// The AABB enclosing the item's `local_bounds` after composing
    /// through the parent chain — i.e. the rectangle the spatial
    /// index buckets on. `None` if the id is unknown.
    pub fn scene_rect(&self, id: ItemId) -> Option<Rect> {
        let local_bounds = self.local_bounds(id)?;
        Some(self.scene_transform(id).apply_rect(local_bounds))
    }

    /// Map a point in the item's local frame to scene coords.
    pub fn map_to_scene(&self, id: ItemId, local_pt: Point) -> Option<Point> {
        if !self.entry_index.contains_key(&id) {
            return None;
        }
        Some(self.scene_transform(id).apply_point(local_pt))
    }

    /// Map a point in scene coords to the item's local frame.
    /// Returns `None` if the item is unknown or its scene transform
    /// is degenerate (zero scale).
    pub fn map_from_scene(&self, id: ItemId, scene_pt: Point) -> Option<Point> {
        if !self.entry_index.contains_key(&id) {
            return None;
        }
        self.scene_transform(id)
            .inverse()
            .map(|inv| inv.apply_point(scene_pt))
    }

    fn compute_scene_aabb(&self, id: ItemId) -> Option<Rect> {
        let pos = *self.entry_index.get(&id)?;
        let local_bounds = self.entries[pos].local_bounds;
        Some(self.scene_transform(id).apply_rect(local_bounds))
    }

    fn rebucket_subtree(&mut self, root: ItemId) {
        // Re-bucket `root` and every descendant whose scene-AABB
        // depends on the root's frame.
        //
        // Build a parent→children adjacency map once (O(N)) so the walk is
        // O(N) instead of O(N²) (the previous code rescanned every entry per
        // node).
        let mut children: HashMap<ItemId, Vec<ItemId>> = HashMap::new();
        for entry in &self.entries {
            if let Some(parent) = entry.parent {
                children.entry(parent).or_default().push(entry.id);
            }
        }

        // Cycle guard: the parent-pointer walkers (`scene_transform` etc.)
        // bound their *upward* walk with a hop cap; this *downward* walk can
        // loop forever if the parent graph ever contains a cycle (e.g. from a
        // future de-serialization bug), so we track visited nodes. A
        // well-formed tree never revisits a node, so this is also a redundant-
        // work guard.
        let mut visited: HashSet<ItemId> = HashSet::new();
        let mut stack: Vec<ItemId> = vec![root];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(aabb) = self.compute_scene_aabb(id) {
                self.index.insert(id, aabb);
            }
            if let Some(kids) = children.get(&id) {
                stack.extend(kids.iter().copied());
            }
        }
    }

    // -----------------------------------------------------------------
    // Flags, visibility, opacity (per item)
    // -----------------------------------------------------------------

    /// Read an item's [`ItemFlags`] bitset.
    pub fn flags(&self, id: ItemId) -> Option<ItemFlags> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].flags)
    }

    /// Replace an item's flags wholesale. No-op if unknown.
    pub fn set_flags(&mut self, id: ItemId, flags: ItemFlags) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].flags;
            if old == flags {
                return;
            }
            self.entries[pos].flags = flags;
            self.emit_item_change(ItemChange::FlagsChanged {
                id,
                old,
                new: flags,
            });
        }
    }

    /// Set or clear a single flag on an item. No-op if unknown.
    pub fn set_flag(&mut self, id: ItemId, flag: ItemFlags, on: bool) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].flags;
            self.entries[pos].flags.set(flag, on);
            let new = self.entries[pos].flags;
            if old != new {
                if flag == ItemFlags::IS_VISIBLE {
                    self.emit_item_change(ItemChange::VisibilityChanged { id, visible: on });
                }
                self.emit_item_change(ItemChange::FlagsChanged { id, old, new });
            }
        }
    }

    /// Toggle the [`ItemFlags::IS_VISIBLE`] bit. Convenience for
    /// the common "hide this item" operation.
    pub fn set_visible(&mut self, id: ItemId, visible: bool) {
        self.set_flag(id, ItemFlags::IS_VISIBLE, visible);
    }

    /// Whether the item is visible AND every ancestor in its chain
    /// is visible. Returns `true` when nothing in the chain has
    /// `IS_VISIBLE` cleared. `false` for unknown ids.
    pub fn is_effectively_visible(&self, id: ItemId) -> bool {
        let cap = self.entries.len();
        let mut hops = 0;
        let mut cur = Some(id);
        while let Some(cid) = cur {
            let Some(&pos) = self.entry_index.get(&cid) else {
                return false;
            };
            let entry = &self.entries[pos];
            if !entry.flags.contains(ItemFlags::IS_VISIBLE) {
                return false;
            }
            cur = entry.parent;
            hops += 1;
            if hops > cap {
                break;
            }
        }
        true
    }

    /// Read an item's local opacity multiplier (`1.0` by default).
    pub fn opacity(&self, id: ItemId) -> Option<f32> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].opacity)
    }

    /// Set an item's local opacity, clamped to `[0.0, 1.0]`.
    pub fn set_opacity(&mut self, id: ItemId, opacity: f32) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let new = opacity.clamp(0.0, 1.0);
            let old = self.entries[pos].opacity;
            if (old - new).abs() < f32::EPSILON {
                return;
            }
            self.entries[pos].opacity = new;
            self.emit_item_change(ItemChange::OpacityChanged { id, old, new });
        }
    }

    /// Replace a lightweight item's fill colour live, emitting
    /// [`ItemChange::AppearanceChanged`] — **always repaint-only**, never a
    /// relayout, rebuild, or AccessKit re-walk. The colour is a [`ColorProp`],
    /// so it accepts a plain [`Color`](teksilo_tokens::Color), a theme role, a
    /// `Signal<Color>`, or a `Signal<Role>`. No-op for item kinds without a fill
    /// (e.g. `ImageItem`).
    ///
    /// # Reactivity contract
    ///
    /// A colour becomes **continuously** reactive by being registered at build
    /// time (`SceneItem::register_bindings`). So:
    ///
    /// - **Construct** the item with a `Signal`/role colour (`.fill(my_signal)`)
    ///   for a colour that tracks its signal forever. This is the recommended
    ///   path and needs no mutator at all.
    /// - **This mutator** installs a *snapshot*: it repaints immediately, which
    ///   is all a static colour ever needs. If you pass a `Signal`/dynamic role
    ///   here, it paints the signal's current value now and starts tracking it
    ///   continuously from the owning view's next rebuild (whenever some other
    ///   structural change re-runs `register_bindings`). Deliberately *not*
    ///   forced: a colour change must never cost a rebuild + AT re-walk.
    pub fn set_item_fill(&mut self, id: ItemId, fill: impl Into<ColorProp>) {
        let prop = fill.into();
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        let applied = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_fill(Some(prop)),
            _ => false,
        };
        if applied {
            self.emit_item_change(ItemChange::AppearanceChanged { id });
        }
    }

    /// Clear a lightweight item's fill (Rect/Path/Group become fill-less),
    /// emitting [`ItemChange::AppearanceChanged`] (repaint-only). No-op for items
    /// whose fill can't be cleared (e.g. `TextItem`, which always has a
    /// foreground colour).
    pub fn clear_item_fill(&mut self, id: ItemId) {
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        let applied = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_fill(None),
            _ => false,
        };
        if applied {
            self.emit_item_change(ItemChange::AppearanceChanged { id });
        }
    }

    /// Replace a lightweight item's stroke (colour + [`StrokeStyle`]) live,
    /// emitting [`ItemChange::AppearanceChanged`] (repaint-only). No-op for item
    /// kinds without a stroke slot (`TextItem` / `ImageItem`). See
    /// [`set_item_fill`](Self::set_item_fill) for the reactivity contract.
    pub fn set_item_stroke(&mut self, id: ItemId, color: impl Into<ColorProp>, style: StrokeStyle) {
        let prop = color.into();
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        let applied = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_stroke(Some((prop, style))),
            _ => false,
        };
        if applied {
            self.emit_item_change(ItemChange::AppearanceChanged { id });
        }
    }

    /// Clear a lightweight item's stroke, emitting
    /// [`ItemChange::AppearanceChanged`] (repaint-only). No-op for item kinds
    /// without a stroke.
    pub fn clear_item_stroke(&mut self, id: ItemId) {
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        let applied = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_stroke(None),
            _ => false,
        };
        if applied {
            self.emit_item_change(ItemChange::AppearanceChanged { id });
        }
    }

    /// Insert an already-boxed lightweight item at `local_pos`, returning its
    /// id. The boxed-`dyn` counterpart of [`add_item`](Self::add_item) — used by
    /// [`SceneListAdapter`](crate::SceneListAdapter) whose delegate yields
    /// `Box<dyn SceneItem>`.
    pub fn add_boxed_item(&mut self, item: Box<dyn SceneItem>, local_pos: Point) -> ItemId {
        self.insert_boxed(item, local_pos, false)
    }

    /// Replace an item's handler set. Pass `None` to clear.
    pub fn set_item_handlers(&mut self, id: ItemId, handlers: Option<SceneItemHandlerSet>) {
        if let Some(&pos) = self.entry_index.get(&id) {
            self.entries[pos].handlers = handlers.map(Box::new);
        }
    }

    /// Mutably borrow an item's handler set, lazily creating an
    /// empty one if none exists. Returns `None` for unknown ids.
    /// Allows fluent chains: `scene.handlers_mut(id).unwrap().on_tap(…).cursor(…);`.
    pub fn handlers_mut(&mut self, id: ItemId) -> Option<&mut SceneItemHandlerSet> {
        let pos = *self.entry_index.get(&id)?;
        let entry = self.entries.get_mut(pos)?;
        if entry.handlers.is_none() {
            entry.handlers = Some(Box::new(SceneItemHandlerSet::new()));
        }
        entry.handlers.as_deref_mut()
    }

    /// Read-only access to an item's handler set, if one is set.
    pub fn handlers(&self, id: ItemId) -> Option<&SceneItemHandlerSet> {
        let pos = *self.entry_index.get(&id)?;
        self.entries[pos].handlers.as_deref()
    }

    /// Effective opacity composed up the parent chain — the product
    /// of every ancestor's opacity and this item's. `1.0` for an
    /// unknown id (so callers don't end up multiplying by a stale
    /// value).
    pub fn effective_opacity(&self, id: ItemId) -> f32 {
        let cap = self.entries.len();
        let mut hops = 0;
        let mut cur = Some(id);
        let mut acc = 1.0_f32;
        while let Some(cid) = cur {
            let Some(&pos) = self.entry_index.get(&cid) else {
                return acc;
            };
            let entry = &self.entries[pos];
            acc *= entry.opacity;
            cur = entry.parent;
            hops += 1;
            if hops > cap {
                break;
            }
        }
        acc
    }

    // -----------------------------------------------------------------
    // Scene rect (Qt setSceneRect) + pan/zoom policy
    // -----------------------------------------------------------------

    /// Declare the scene's logical extent. `None` (the default)
    /// means "auto-compute from items each query"; `Some(rect)`
    /// fixes the extent regardless of item placement. Used by
    /// `SceneView` for pan clamping and `fit_to_content`.
    pub fn set_scene_rect(&mut self, rect: Option<Rect>) {
        self.user_scene_rect = rect;
    }

    /// The resolved scene extent — user-declared via
    /// [`Scene::set_scene_rect`] if set, otherwise the AABB
    /// enclosing every item's scene rect. `None` when neither is
    /// available (the user didn't declare and the scene is empty).
    pub fn scene_rect_extent(&self) -> Option<Rect> {
        if let Some(r) = self.user_scene_rect {
            return Some(r);
        }
        let ids = self.ids();
        let mut acc: Option<Rect> = None;
        for id in ids {
            let Some(r) = self.scene_rect(id) else {
                continue;
            };
            acc = Some(match acc {
                None => r,
                Some(a) => union_two_rects(a, r),
            });
        }
        acc
    }

    /// Set the axes the view may pan along. Default
    /// [`PanAxes::Both`]. Writes to the reactive signal; gesture
    /// closures pick the change up on the next event.
    pub fn pan_axes(&mut self, axes: PanAxes) {
        self.constraints.pan_axes.set(axes);
    }

    /// The currently-declared pan axes. Live read of the signal.
    pub fn current_pan_axes(&self) -> PanAxes {
        self.constraints.pan_axes.get()
    }

    /// Set whether the view honors zoom gestures. Default `true`.
    /// Writes to the reactive signal.
    pub fn zoomable(&mut self, on: bool) {
        self.constraints.zoomable.set(on);
    }

    /// Whether the scene currently allows zoom. Live read.
    pub fn is_zoomable(&self) -> bool {
        self.constraints.zoomable.get()
    }

    /// Clamp the visible viewport to this scene-coord rect. `None`
    /// (default) leaves pan unconstrained. When `Some(r)`, the
    /// [`SceneView`](crate::SceneView)'s pan is clamped so the
    /// visible scene region overlaps `r`. When `r` is smaller than
    /// the visible viewport, the rect is centered.
    ///
    /// Distinct from [`set_scene_rect`](Self::set_scene_rect):
    /// `scene_rect` declares the scene's logical extent (used by
    /// `adopt_scene_size`); `pan_bounds` controls what region the
    /// user can scroll to. A doc-style app typically sets both to
    /// the same rect.
    pub fn set_pan_bounds(&mut self, bounds: Option<Rect>) {
        self.constraints.pan_bounds.set(bounds);
    }

    /// The currently-declared pan-bounds rect. Live read.
    pub fn current_pan_bounds(&self) -> Option<Rect> {
        self.constraints.pan_bounds.get()
    }

    /// Inclusive `[min, max]` zoom-factor clamp. `None` (default)
    /// is unconstrained from the `Scene` side — the `SceneView`
    /// may still impose its own override.
    ///
    /// The effective range applied by the `SceneView` is the
    /// intersection of `Scene` + view-level override, so apps
    /// cannot loosen a `Scene`-declared range by setting a wider
    /// override on the view.
    pub fn set_zoom_range(&mut self, range: Option<std::ops::RangeInclusive<f32>>) {
        self.constraints.zoom_range.set(range);
    }

    /// The currently-declared zoom range. Live read.
    pub fn current_zoom_range(&self) -> Option<std::ops::RangeInclusive<f32>> {
        self.constraints.zoom_range.get()
    }

    /// Reactive accessors for live observation.
    pub fn pan_axes_signal(&self) -> Signal<PanAxes> {
        self.constraints.pan_axes_signal()
    }
    /// Reactive pan-bounds signal.
    pub fn pan_bounds_signal(&self) -> Signal<Option<Rect>> {
        self.constraints.pan_bounds_signal()
    }
    /// Reactive zoom-range signal.
    pub fn zoom_range_signal(&self) -> Signal<Option<std::ops::RangeInclusive<f32>>> {
        self.constraints.zoom_range_signal()
    }
    /// Reactive zoomable on/off signal.
    pub fn zoomable_signal(&self) -> Signal<bool> {
        self.constraints.zoomable_signal()
    }

    /// Read-only view of the full constraint bundle. Useful when
    /// passing all four signals to a custom view implementation.
    pub fn constraints(&self) -> &SceneConstraints {
        &self.constraints
    }

    // -----------------------------------------------------------------
    // Z-order and parenting
    // -----------------------------------------------------------------

    /// Set paint z-order for an entry. Higher z paints later (on top);
    /// equal-z falls back to insertion order. Default 0.0.
    ///
    /// Works for **both** tiers: lightweight items re-sort within their
    /// band on the next paint, and heavyweight widget entries restack the
    /// arena children on the next rebuild (the SceneView reorders
    /// `node.children` by z without recreating the widgets, so focus /
    /// text-edit / animation state survives the restack). No-op for
    /// unknown ids.
    pub fn set_z(&mut self, id: ItemId, z: f32) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].z;
            if (old - z).abs() < f32::EPSILON {
                return;
            }
            self.entries[pos].z = z;
            self.emit_item_change(ItemChange::ZChanged { id, old, new: z });
        }
    }

    /// Raise an entry above all current entries by giving it a z one
    /// greater than the current maximum. The drag-to-front primitive —
    /// call it on drag-start so the grabbed card (and its text) renders
    /// over the others. Works for both tiers (see [`set_z`](Self::set_z)).
    pub fn bring_to_front(&mut self, id: ItemId) {
        if !self.entry_index.contains_key(&id) {
            return;
        }
        let max_z = self
            .entries
            .iter()
            .map(|e| e.z)
            .fold(f32::NEG_INFINITY, f32::max);
        let target = if max_z.is_finite() { max_z + 1.0 } else { 1.0 };
        self.set_z(id, target);
    }

    /// Lower an entry below all current entries by giving it a z one less
    /// than the current minimum. Works for both tiers (see
    /// [`set_z`](Self::set_z)).
    pub fn send_to_back(&mut self, id: ItemId) {
        if !self.entry_index.contains_key(&id) {
            return;
        }
        let min_z = self
            .entries
            .iter()
            .map(|e| e.z)
            .fold(f32::INFINITY, f32::min);
        let target = if min_z.is_finite() { min_z - 1.0 } else { -1.0 };
        self.set_z(id, target);
    }

    /// Read an entry's z-order.
    pub fn z(&self, id: ItemId) -> Option<f32> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].z)
    }

    /// Set the Under/Over paint band for a lightweight entry. `Over`
    /// items paint *after* the heavyweight widget children (in the
    /// SceneView's `post_paint`), so they sit on top of the cards;
    /// `Under` items (the default) paint before them. Within a band,
    /// [`set_z`](Self::set_z) still orders items among themselves.
    /// No-op for unknown ids.
    pub fn set_layer(&mut self, id: ItemId, layer: SceneLayer) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].layer;
            if old == layer {
                return;
            }
            self.entries[pos].layer = layer;
            self.emit_item_change(ItemChange::LayerChanged {
                id,
                old,
                new: layer,
            });
        }
    }

    /// Read an entry's Under/Over paint band. `None` for unknown ids.
    pub fn layer(&self, id: ItemId) -> Option<SceneLayer> {
        let pos = *self.entry_index.get(&id)?;
        Some(self.entries[pos].layer)
    }

    /// Whether any entry is in the [`SceneLayer::Over`] band. The
    /// SceneView consults this in `wants_post_paint` to skip the
    /// foreground pass entirely when nothing is raised above the cards.
    /// Linear in entry count, called once per frame.
    pub(crate) fn has_over_layer_items(&self) -> bool {
        self.entries.iter().any(|e| e.layer == SceneLayer::Over)
    }

    /// Declare a parent/child relationship. `child`'s `local_pos`
    /// and `transform` are reinterpreted as relative to the new
    /// parent's local frame — the visual position changes unless
    /// the caller compensates. Re-buckets `child`'s subtree.
    ///
    /// Pass `parent = None` to detach (child's local frame becomes
    /// scene-rooted again).
    ///
    /// **Cycle guard:** if the proposed parent is `child` itself
    /// or a descendant of `child`, the call is a no-op (no parent
    /// change, no rebucket, no signal fire). Without this guard
    /// the downstream `rebucket_subtree` walk loops indefinitely.
    pub fn set_item_parent(&mut self, child: ItemId, parent: Option<ItemId>) {
        if let Some(&pos) = self.entry_index.get(&child) {
            let old = self.entries[pos].parent;
            if old == parent {
                return;
            }
            // Reject self-parent and any parent in the child's
            // subtree (would create a cycle).
            if let Some(p) = parent
                && (p == child || self.is_descendant_of(p, child))
            {
                return;
            }
            self.entries[pos].parent = parent;
            self.rebucket_subtree(child);
            self.emit_item_change(ItemChange::ParentChanged {
                id: child,
                old,
                new: parent,
            });
        }
    }

    /// Parent of `id`, if any.
    pub fn parent_of(&self, id: ItemId) -> Option<ItemId> {
        let pos = *self.entry_index.get(&id)?;
        self.entries[pos].parent
    }

    /// Whether `id`'s ancestor chain contains `ancestor`.
    pub fn is_descendant_of(&self, id: ItemId, ancestor: ItemId) -> bool {
        let mut cur = self.parent_of(id);
        let cap = self.entries.len();
        let mut hops = 0;
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = self.parent_of(p);
            hops += 1;
            if hops > cap {
                break;
            }
        }
        false
    }

    /// Append every direct + transitive descendant of `id` into
    /// `out`, breadth-first across declaration order. The id
    /// itself is **not** included.
    pub fn collect_descendants(&self, id: ItemId, out: &mut Vec<ItemId>) {
        let mut frontier: Vec<ItemId> = vec![id];
        while let Some(parent) = frontier.pop() {
            for entry in &self.entries {
                if entry.parent == Some(parent) {
                    out.push(entry.id);
                    frontier.push(entry.id);
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Lookup
    // -----------------------------------------------------------------

    /// Borrow a lightweight [`SceneItem`] by id. `None` for unknown
    /// ids and for heavyweight widget entries.
    pub fn item(&self, id: ItemId) -> Option<&dyn SceneItem> {
        let pos = *self.entry_index.get(&id)?;
        match &self.entries[pos].kind {
            SceneEntryKind::Item(item) => Some(item.as_ref()),
            SceneEntryKind::Widget(_) => None,
        }
    }

    /// Sort `ids` by z-order ascending, stable for equal values.
    /// Crate-private helper for `SceneView::paint`.
    pub(crate) fn sort_by_z(&self, ids: &mut [ItemId]) {
        ids.sort_by(|a, b| {
            let za = self.z(*a).unwrap_or(0.0);
            let zb = self.z(*b).unwrap_or(0.0);
            za.partial_cmp(&zb).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // -----------------------------------------------------------------
    // Removal
    // -----------------------------------------------------------------

    /// Remove an item by id, recursively dropping every descendant.
    ///
    /// Mirrors Qt's `QGraphicsScene::removeItem` semantics: deleting
    /// a parent deletes its children too. No-op if `id` is unknown.
    /// Fires one [`ItemChange::Removed`] per id, descendants first
    /// then the named parent — observers see a consistent
    /// "leaves-then-root" order.
    ///
    /// To remove `id` without deleting its children, call
    /// [`Scene::orphan`] first to promote them to root-level, then
    /// `remove(id)`.
    pub fn remove(&mut self, id: ItemId) {
        use std::collections::HashSet;
        if !self.entry_index.contains_key(&id) {
            return;
        }
        // Descendants, deepest-first via collect_descendants's BFS
        // (the order is leaf-to-root because we push children as we
        // visit each parent). Append the named id last.
        let mut to_remove: Vec<ItemId> = Vec::new();
        self.collect_descendants(id, &mut to_remove);
        to_remove.reverse();
        to_remove.push(id);
        let removal_set: HashSet<ItemId> = to_remove.iter().copied().collect();
        self.entries.retain(|e| !removal_set.contains(&e.id));
        self.entry_index.clear();
        for (pos, entry) in self.entries.iter().enumerate() {
            self.entry_index.insert(entry.id, pos);
        }
        // The AT tree is separate from the visual tree, but a visually-removed
        // item must also vanish from AccessKit. Drop every logical-structure
        // entry that targets a removed item. For `a11y_parents` this also
        // re-roots any *still-alive* node that was AT-parented under a removed
        // item — dropping the `(child → removed)` mapping makes the child fall
        // back to the SceneView root (mirrors `remove_a11y_group`). Removal
        // itself fires `ItemChange::Removed`, so `SceneView` already re-walks
        // AT through the item-change observer; no `a11y_change_signal` bump
        // is needed here.
        let is_removed = |n: &A11yNode| matches!(n, A11yNode::Item(i) if removal_set.contains(i));
        self.a11y_parents
            .retain(|child, parent| !is_removed(child) && !is_removed(parent));
        self.a11y_relations
            .retain(|(from, _, to)| !is_removed(from) && !is_removed(to));
        for removed_id in &removal_set {
            let node = A11yNode::Item(*removed_id);
            self.a11y_live.remove(&node);
            self.a11y_landmarks.remove(&node);
            self.a11y_categories.remove(&node);
            // Drop any magnets attached to the removed item, retiring
            // their ids from the reverse-lookup map. Magnets are local
            // to the item, so a removed item takes its magnets with it.
            if let Some(magnets) = self.magnets.remove(removed_id) {
                for (mid, _) in magnets {
                    self.magnet_owner.remove(&mid);
                }
            }
        }

        for removed_id in to_remove {
            self.index.remove(removed_id);
            self.emit_item_change(ItemChange::Removed { id: removed_id });
        }
    }

    /// Promote `id`'s direct children to root-level (clear their
    /// `parent` field). Used when an app wants to remove `id` without
    /// dropping its children — call `orphan(id)` then `remove(id)`.
    /// No-op when `id` is unknown or has no children.
    ///
    /// Fires one [`ItemChange::ParentChanged`] per detached child and
    /// re-buckets every detached subtree in the spatial index — the
    /// children's `scene_transform` shifts (no longer composes
    /// `id`'s) so their scene-space AABBs change. Without re-bucketing
    /// the index, [`items_in_rect`](Self::items_in_rect) and
    /// [`item_at`](Self::item_at) would return stale results.
    ///
    /// Apps wanting *visual* stability across the orphan call should
    /// first bake `id`'s `scene_transform` into each child's
    /// `local_pos` + `transform`; otherwise children visibly jump.
    pub fn orphan(&mut self, id: ItemId) {
        if !self.entry_index.contains_key(&id) {
            return;
        }
        let children: Vec<ItemId> = self
            .entries
            .iter()
            .filter(|e| e.parent == Some(id))
            .map(|e| e.id)
            .collect();
        for child in children {
            if let Some(&pos) = self.entry_index.get(&child) {
                self.entries[pos].parent = None;
                // Re-bucket the entire detached subtree: each child's
                // scene_transform changed (no longer composes `id`'s),
                // so spatial-index AABBs are stale. Subtree-walk
                // because grandchildren depend on the chain too.
                self.rebucket_subtree(child);
                self.emit_item_change(ItemChange::ParentChanged {
                    id: child,
                    old: Some(id),
                    new: None,
                });
            }
        }
    }

    // -----------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------

    /// All items whose scene-AABB intersects `scene_rect`.
    ///
    /// Broad phase: the spatial index returns every id bucketed in
    /// any cell touched by `scene_rect`. Narrow phase: each candidate
    /// goes through [`scene_rect`](Self::scene_rect), which itself
    /// dispatches via `entry_index` (an `HashMap<ItemId, usize>`),
    /// so the per-candidate cost is O(parent-chain-depth) — not
    /// O(N). Total query is O(visible × chain) instead of O(N).
    pub fn items_in_rect(&self, scene_rect: Rect) -> Vec<ItemId> {
        self.index
            .query(scene_rect)
            .into_iter()
            .filter(|id| {
                self.scene_rect(*id)
                    .map(|r| rects_intersect(r, scene_rect))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Snapshot every visible item — **both tiers** — as a `(scene_rect,
    /// color)` pair suitable for a minimap thumbnail. Filters out items with
    /// `HAS_NO_CONTENTS` (logical-only) and items hidden by `IS_VISIBLE` / a
    /// hidden ancestor — the visible-effective set matches what the SceneView's
    /// paint walk renders.
    ///
    /// Ordered by insertion (low z first). A lightweight item's color comes
    /// from [`SceneItem::thumbnail_color`] (its fill / stroke / a neutral grey);
    /// a heavyweight widget entry has no `SceneItem`, so it's shown in a neutral
    /// tint — a minimap that omitted the heavyweight tier would misrepresent a
    /// widget-heavy scene (cards, nodes), so both tiers are included.
    pub fn item_thumbnails(&self) -> Vec<(Rect, teksilo_tokens::Color)> {
        let mut out = Vec::new();
        for entry in &self.entries {
            // Skip invisible / logical-only items (either tier).
            if !self.is_effectively_visible(entry.id) {
                continue;
            }
            if entry.flags.contains(ItemFlags::HAS_NO_CONTENTS) {
                continue;
            }
            let Some(rect) = self.scene_rect(entry.id) else {
                continue;
            };
            let color = match &entry.kind {
                SceneEntryKind::Item(item) => item.thumbnail_color(),
                // Heavyweight widget: no `thumbnail_color`, so use a neutral tint.
                SceneEntryKind::Widget(_) => teksilo_tokens::Color::new(0.45, 0.52, 0.65, 0.85),
            };
            out.push((rect, color));
        }
        out
    }

    /// Topmost lightweight item whose `shape_contains` fires for
    /// `scene_pt`. Iterates `items_in_rect` for a tiny rect around
    /// the point, sorts by z descending, and returns the first hit.
    /// Heavyweight widget entries are skipped (their hit-testing is
    /// handled by the arena event dispatch).
    ///
    /// **Limitation:** items flagged
    /// [`IGNORES_TRANSFORMATIONS`](crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS)
    /// hit-test in screen space, not scene space — so this scene-only
    /// query may incorrectly hit them or miss them depending on the
    /// current view transform. Apps that route pointer events through
    /// `SceneView`'s dispatch get screen-space hit-test for IGNORES
    /// items automatically; only use `item_at` directly for normal
    /// items, or pair with the view transform to filter.
    pub fn item_at(&self, scene_pt: Point) -> Option<ItemId> {
        let probe = Rect::new(scene_pt.x, scene_pt.y, 0.0, 0.0);
        let mut candidates = self.items_in_rect(probe);
        candidates.sort_by(|a, b| {
            let za = self.z(*a).unwrap_or(0.0);
            let zb = self.z(*b).unwrap_or(0.0);
            zb.partial_cmp(&za).unwrap_or(std::cmp::Ordering::Equal)
        });
        for id in candidates {
            let Some(item) = self.item(id) else {
                continue;
            };
            let Some(local_pt) = self.map_from_scene(id, scene_pt) else {
                continue;
            };
            if item.shape_contains(local_pt) {
                return Some(id);
            }
        }
        None
    }

    /// Items whose scene-AABB intersects the AABB of `id`. Excludes
    /// `id` itself. Apps use this for "which other items overlap
    /// this card?" queries — graph editors checking node-on-node
    /// overlap, CAD canvases finding adjacent geometry. Backed by
    /// the spatial index, so the cost is `O(visible)` not `O(N)`.
    pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId> {
        let Some(rect) = self.scene_rect(id) else {
            return Vec::new();
        };
        self.items_in_rect(rect)
            .into_iter()
            .filter(|other| *other != id)
            .collect()
    }

    /// Items whose scene-AABB intersects `path`'s bounding rect.
    /// Apps use this for "which items lie under this connector?"
    /// queries — graph editors highlighting hovered connectors,
    /// CAD canvases doing point-in-polygon style picking. The
    /// narrow phase is AABB-vs-AABB; per-segment-distance precision
    /// is left to the app.
    pub fn items_along_path(&self, path: &Path) -> Vec<ItemId> {
        let Some(rect) = path_aabb(path) else {
            return Vec::new();
        };
        self.items_in_rect(rect)
    }

    /// All lightweight items whose `shape_contains` fires for
    /// `scene_pt`, sorted topmost-first by z.
    pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId> {
        let probe = Rect::new(scene_pt.x, scene_pt.y, 0.0, 0.0);
        let mut candidates = self.items_in_rect(probe);
        candidates.sort_by(|a, b| {
            let za = self.z(*a).unwrap_or(0.0);
            let zb = self.z(*b).unwrap_or(0.0);
            zb.partial_cmp(&za).unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates
            .into_iter()
            .filter(|id| {
                let Some(item) = self.item(*id) else {
                    return false;
                };
                let Some(local_pt) = self.map_from_scene(*id, scene_pt) else {
                    return false;
                };
                item.shape_contains(local_pt)
            })
            .collect()
    }

    // -----------------------------------------------------------------
    // Metadata
    // -----------------------------------------------------------------

    /// Number of entries in the scene.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the scene is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All ids in insertion order.
    pub fn ids(&self) -> Vec<ItemId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// Borrow the spatial index (diagnostics / tests).
    pub fn index(&self) -> &dyn SpatialIndex {
        &*self.index
    }

    // -----------------------------------------------------------------
    // Magnetism
    // -----------------------------------------------------------------

    /// Attach a [`Magnet`] to `item` and return its [`MagnetId`].
    ///
    /// Magnets are local to their item (their `local_pos` is in the
    /// item's frame), so they follow the item under any move / rotate /
    /// scale via the same `scene_transform` the item uses. No-op
    /// returning a fresh-but-unowned id if `item` is unknown — callers
    /// add magnets to items they just created.
    ///
    /// Bumps the AT-structure change counter (magnets are AT structure)
    /// so a `SceneView` with magnetism enabled re-walks its synthetic
    /// magnet nodes.
    pub fn add_magnet(&mut self, item: ItemId, magnet: Magnet) -> MagnetId {
        let id = MagnetId::next();
        if !self.entry_index.contains_key(&item) {
            return id;
        }
        self.magnets.entry(item).or_default().push((id, magnet));
        self.magnet_owner.insert(id, item);
        self.bump_a11y_change();
        id
    }

    /// Remove a magnet by id. No-op if the id is unknown.
    pub fn remove_magnet(&mut self, magnet: MagnetId) {
        let Some(owner) = self.magnet_owner.remove(&magnet) else {
            return;
        };
        if let Some(list) = self.magnets.get_mut(&owner) {
            list.retain(|(mid, _)| *mid != magnet);
            if list.is_empty() {
                self.magnets.remove(&owner);
            }
        }
        self.bump_a11y_change();
    }

    /// Remove every magnet attached to `item`. No-op if none.
    pub fn clear_magnets(&mut self, item: ItemId) {
        if let Some(list) = self.magnets.remove(&item) {
            for (mid, _) in list {
                self.magnet_owner.remove(&mid);
            }
            self.bump_a11y_change();
        }
    }

    /// Move a magnet to a new position in its owning item's local
    /// frame. No-op if the id is unknown.
    pub fn set_magnet_local_pos(&mut self, magnet: MagnetId, local_pos: Point) {
        let Some(&owner) = self.magnet_owner.get(&magnet) else {
            return;
        };
        if let Some(list) = self.magnets.get_mut(&owner)
            && let Some((_, m)) = list.iter_mut().find(|(mid, _)| *mid == magnet)
        {
            m.local_pos = local_pos;
            self.bump_a11y_change();
        }
    }

    /// Enable or disable a magnet. Disabled magnets are skipped by
    /// broad-phase, feedback, the keyboard cycle, and AT emission.
    /// No-op if the id is unknown.
    pub fn set_magnet_enabled(&mut self, magnet: MagnetId, enabled: bool) {
        let Some(&owner) = self.magnet_owner.get(&magnet) else {
            return;
        };
        if let Some(list) = self.magnets.get_mut(&owner)
            && let Some((_, m)) = list.iter_mut().find(|(mid, _)| *mid == magnet)
            && m.enabled != enabled
        {
            m.enabled = enabled;
            self.bump_a11y_change();
        }
    }

    /// The ids of every magnet attached to `item`, in insertion order
    /// (enabled and disabled alike). Empty if `item` is unknown or has
    /// no magnets.
    pub fn magnet_ids_of(&self, item: ItemId) -> Vec<MagnetId> {
        self.magnets
            .get(&item)
            .map(|list| list.iter().map(|(mid, _)| *mid).collect())
            .unwrap_or_default()
    }

    /// The owning item of a magnet, or `None` if the id is unknown.
    pub fn magnet_owner(&self, magnet: MagnetId) -> Option<ItemId> {
        self.magnet_owner.get(&magnet).copied()
    }

    /// The label set on a magnet (for the AT walker). `None` if unset
    /// or the id is unknown.
    pub(crate) fn magnet_label(&self, magnet: MagnetId) -> Option<teksilo_i18n::LocalizedString> {
        let owner = self.magnet_owner.get(&magnet)?;
        let list = self.magnets.get(owner)?;
        list.iter()
            .find(|(mid, _)| *mid == magnet)
            .and_then(|(_, m)| m.label.clone())
    }

    /// Whether a magnet is enabled. `false` for an unknown id.
    pub fn magnet_enabled(&self, magnet: MagnetId) -> bool {
        let Some(owner) = self.magnet_owner.get(&magnet) else {
            return false;
        };
        self.magnets
            .get(owner)
            .and_then(|list| list.iter().find(|(mid, _)| *mid == magnet))
            .map(|(_, m)| m.enabled)
            .unwrap_or(false)
    }

    /// A magnet's position in scene coordinates (its local position
    /// projected through its owning item's `scene_transform`). `None`
    /// for an unknown id or a degenerate item transform.
    pub fn magnet_scene_pos(&self, magnet: MagnetId) -> Option<Point> {
        let &owner = self.magnet_owner.get(&magnet)?;
        let list = self.magnets.get(&owner)?;
        let (_, m) = list.iter().find(|(mid, _)| *mid == magnet)?;
        self.map_to_scene(owner, m.local_pos)
    }

    /// Resolve a magnet to a borrow-free [`MagnetRef`] snapshot (id,
    /// owning item, role, payload clone, current scene position).
    /// `None` for an unknown id or a degenerate item transform.
    pub fn magnet(&self, magnet: MagnetId) -> Option<MagnetRef> {
        let &owner = self.magnet_owner.get(&magnet)?;
        let list = self.magnets.get(&owner)?;
        let (_, m) = list.iter().find(|(mid, _)| *mid == magnet)?;
        let scene_pos = self.map_to_scene(owner, m.local_pos)?;
        Some(MagnetRef {
            id: magnet,
            item: owner,
            role: m.role,
            payload: m.payload.clone(),
            scene_pos,
        })
    }

    /// Collect every enabled magnet whose scene position lies inside
    /// `scene_rect`, as borrow-free [`MagnetRef`] snapshots, excluding
    /// any magnet on `exclude_item`. Broad-phase over the spatial index
    /// (`items_in_rect`) so the cost is `O(visible × magnets/item)`.
    ///
    /// This is the shared narrow-phase input for both snap helpers: the
    /// candidates are materialised as owned snapshots, so the predicate
    /// that runs over them touches no scene state. The predicate may read
    /// the model (a shared borrow is re-entrant) but must not mutate it;
    /// mutation belongs in the consumer's `on_connect`, which fires after
    /// the snap call returns and every borrow is dropped.
    fn collect_candidate_magnets(
        &self,
        scene_rect: Rect,
        exclude_item: Option<ItemId>,
    ) -> Vec<MagnetRef> {
        let mut out = Vec::new();
        for item in self.items_in_rect(scene_rect) {
            if Some(item) == exclude_item {
                continue;
            }
            let Some(list) = self.magnets.get(&item) else {
                continue;
            };
            let xform = self.scene_transform(item);
            for (mid, m) in list {
                if !m.enabled {
                    continue;
                }
                let scene_pos = xform.apply_point(m.local_pos);
                if !scene_rect.contains(scene_pos) {
                    continue;
                }
                out.push(MagnetRef {
                    id: *mid,
                    item,
                    role: m.role,
                    payload: m.payload.clone(),
                    scene_pos,
                });
            }
        }
        out
    }

    /// Square-rect of half-extent `radius` centred on `center`.
    fn capture_rect(center: Point, radius: f32) -> Rect {
        Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        )
    }

    /// Compute the best item-drag snap: the dragged item is visually
    /// offset by `drag_delta`, and each of its enabled magnets seeks the
    /// nearest *accepting* magnet on another item within `capture_radius`
    /// (in scene units). Returns the globally closest accepting pair, or
    /// `None` if nothing accepts within range.
    ///
    /// Pure mechanism: it collects candidates under a brief read, then
    /// runs the consumer `predicate` with no scene borrow held, so the
    /// predicate may inspect payloads freely. `snap_vector` added to
    /// `drag_delta` aligns the dragged magnet onto its target.
    pub fn compute_item_snap(
        &self,
        dragged: ItemId,
        drag_delta: Vec2,
        capture_radius: f32,
        predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict,
    ) -> Option<MagnetSnap> {
        if capture_radius <= 0.0 {
            return None;
        }
        let dragged_list = self.magnets.get(&dragged)?;
        if dragged_list.is_empty() {
            return None;
        }
        // Visual scene positions of the dragged item's enabled magnets:
        // committed scene pos + the live drag delta.
        let xform = self.scene_transform(dragged);
        let dragged_magnets: Vec<MagnetRef> = dragged_list
            .iter()
            .filter(|(_, m)| m.enabled)
            .map(|(mid, m)| {
                let committed = xform.apply_point(m.local_pos);
                MagnetRef {
                    id: *mid,
                    item: dragged,
                    role: m.role,
                    payload: m.payload.clone(),
                    scene_pos: Point::new(committed.x + drag_delta.x, committed.y + drag_delta.y),
                }
            })
            .collect();
        if dragged_magnets.is_empty() {
            return None;
        }

        let mut best: Option<MagnetSnap> = None;
        for from in &dragged_magnets {
            let rect = Self::capture_rect(from.scene_pos, capture_radius);
            let candidates = self.collect_candidate_magnets(rect, Some(dragged));
            for to in &candidates {
                let dx = to.scene_pos.x - from.scene_pos.x;
                let dy = to.scene_pos.y - from.scene_pos.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > capture_radius {
                    continue;
                }
                let MagnetVerdict::Accept(payload) = predicate(from, to) else {
                    continue;
                };
                let better = best.as_ref().map(|b| dist < b.distance).unwrap_or(true);
                if better {
                    best = Some(MagnetSnap {
                        from: from.id,
                        to: to.id,
                        snap_vector: Vec2::new(dx, dy),
                        payload,
                        distance: dist,
                    });
                }
            }
        }
        best
    }

    /// Compute the best port-drag snap: a single `source` magnet is
    /// dragging a transient wire whose free end is at `cursor_scene`.
    /// Finds the nearest *accepting* target magnet within
    /// `capture_radius` (scene units), excluding the source's own
    /// magnet. Returns the target [`MagnetRef`] and the accepting
    /// verdict's payload, or `None`.
    pub fn compute_port_snap(
        &self,
        source: MagnetId,
        cursor_scene: Point,
        capture_radius: f32,
        predicate: &dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict,
    ) -> Option<(MagnetRef, Option<Rc<dyn std::any::Any>>)> {
        if capture_radius <= 0.0 {
            return None;
        }
        let from = self.magnet(source)?;
        let rect = Self::capture_rect(cursor_scene, capture_radius);
        // Don't exclude the source's whole item — a node may legitimately
        // connect to another of its own ports in some graphs; only the
        // source magnet itself is excluded (below).
        let candidates = self.collect_candidate_magnets(rect, None);
        let mut best: Option<(MagnetRef, Option<Rc<dyn std::any::Any>>, f32)> = None;
        for to in candidates {
            if to.id == source {
                continue;
            }
            let dx = to.scene_pos.x - cursor_scene.x;
            let dy = to.scene_pos.y - cursor_scene.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > capture_radius {
                continue;
            }
            let MagnetVerdict::Accept(payload) = predicate(&from, &to) else {
                continue;
            };
            let better = best.as_ref().map(|b| dist < b.2).unwrap_or(true);
            if better {
                best = Some((to, payload, dist));
            }
        }
        best.map(|(to, payload, _)| (to, payload))
    }

    /// The nearest enabled magnet to `scene_pt` within `radius` (scene
    /// units), or `None`. Used by the view to start a port-drag from a
    /// grabbed magnet handle (the handle's grab area is a screen-pixel
    /// disc, converted to scene units by the caller).
    pub fn nearest_magnet(&self, scene_pt: Point, radius: f32) -> Option<MagnetId> {
        if radius <= 0.0 {
            return None;
        }
        let rect = Self::capture_rect(scene_pt, radius);
        let mut best: Option<(MagnetId, f32)> = None;
        for c in self.collect_candidate_magnets(rect, None) {
            let dx = c.scene_pos.x - scene_pt.x;
            let dy = c.scene_pos.y - scene_pt.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > radius {
                continue;
            }
            let better = best.map(|b| dist < b.1).unwrap_or(true);
            if better {
                best = Some((c.id, dist));
            }
        }
        best.map(|(id, _)| id)
    }

    // -----------------------------------------------------------------
    // Logical AT structure (kept verbatim from R0)
    // -----------------------------------------------------------------

    /// Declare a virtual AT group. The group has no visual
    /// counterpart — it exists so the AT walker can emit an AT node
    /// under which items / other groups / widgets can be reparented.
    pub fn add_a11y_group(&mut self, builder: A11yGroupBuilder) -> A11yGroupId {
        let id = A11yGroupId::next();
        let group = A11yGroup {
            id,
            label: builder.label,
            role: builder.role,
        };
        let pos = self.a11y_groups.len();
        self.a11y_groups.push(group);
        self.a11y_group_index.insert(id, pos);
        self.bump_a11y_change();
        id
    }

    /// Remove a logical group; orphaned references fall back to
    /// SceneView root. Relations / live / landmarks / categories
    /// targeting this group are cleaned up too.
    pub fn remove_a11y_group(&mut self, id: A11yGroupId) {
        let prev = self.a11y_groups.len();
        self.a11y_groups.retain(|g| g.id != id);
        if self.a11y_groups.len() != prev {
            self.a11y_group_index.clear();
            for (pos, group) in self.a11y_groups.iter().enumerate() {
                self.a11y_group_index.insert(group.id, pos);
            }
        }
        let target = A11yNode::Group(id);
        self.a11y_parents
            .retain(|child, parent| *child != target && *parent != target);
        self.a11y_relations
            .retain(|(from, _, to)| *from != target && *to != target);
        self.a11y_live.remove(&target);
        self.a11y_landmarks.remove(&target);
        self.a11y_categories.remove(&target);
        self.bump_a11y_change();
    }

    /// Borrow a logical group by id.
    pub fn a11y_group(&self, id: A11yGroupId) -> Option<&A11yGroup> {
        let pos = *self.a11y_group_index.get(&id)?;
        self.a11y_groups.get(pos)
    }

    /// Declare a logical-parent relationship for AT (independent of
    /// visual placement).
    pub fn set_a11y_parent(&mut self, child: A11yNode, parent: Option<A11yNode>) {
        match parent {
            Some(p) => {
                self.a11y_parents.insert(child, p);
            }
            None => {
                self.a11y_parents.remove(&child);
            }
        }
        self.bump_a11y_change();
    }

    /// The currently-declared logical parent of a node.
    pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode> {
        self.a11y_parents.get(&child).copied()
    }

    /// Declare an AT relationship between two nodes.
    pub fn add_a11y_relation(&mut self, from: A11yNode, kind: A11yRelation, to: A11yNode) {
        self.a11y_relations.push((from, kind, to));
        self.bump_a11y_change();
    }

    /// All declared AT relations.
    pub fn a11y_relations(&self) -> &[(A11yNode, A11yRelation, A11yNode)] {
        &self.a11y_relations
    }

    /// Mark a node as a live region. Pass `Live::Off` to clear.
    pub fn set_a11y_live(&mut self, node: A11yNode, live: accesskit::Live) {
        if matches!(live, accesskit::Live::Off) {
            self.a11y_live.remove(&node);
        } else {
            self.a11y_live.insert(node, live);
        }
        self.bump_a11y_change();
    }

    /// Mark a node as a landmark by overriding its role. Pass
    /// `Role::Unknown` to clear.
    pub fn set_a11y_landmark(&mut self, node: A11yNode, role: accesskit::Role) {
        if matches!(role, accesskit::Role::Unknown) {
            self.a11y_landmarks.remove(&node);
        } else {
            self.a11y_landmarks.insert(node, role);
        }
        self.bump_a11y_change();
    }

    /// Tag a node with rotor / quick-nav categories.
    pub fn set_a11y_categories(&mut self, node: A11yNode, categories: &[A11yCategory]) {
        if categories.is_empty() {
            self.a11y_categories.remove(&node);
        } else {
            self.a11y_categories.insert(node, categories.to_vec());
        }
        self.bump_a11y_change();
    }

    /// Read declared categories for a node.
    pub fn a11y_categories_of(&self, node: A11yNode) -> Option<&[A11yCategory]> {
        self.a11y_categories.get(&node).map(|v| v.as_slice())
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scene")
            .field("len", &self.entries.len())
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

/// Half-open AABB intersection: two rects intersect iff their
/// projections overlap on both axes.
pub(crate) fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// AABB of the union of two rectangles.
fn union_two_rects(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = a.right().max(b.right());
    let bot = a.bottom().max(b.bottom());
    Rect::new(x, y, r - x, bot - y)
}

/// AABB enclosing every point in a path. Returns `None` for an
/// empty path. Curves contribute their control / end points only —
/// callers needing tight bounds for cubics should pre-compute and
/// pass the AABB directly via `Scene::items_in_rect`.
fn path_aabb(path: &Path) -> Option<Rect> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut include = |p: Point| {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    };
    for cmd in &path.commands {
        match cmd {
            teksilo_canvas::PathCommand::MoveTo(p) | teksilo_canvas::PathCommand::LineTo(p) => {
                include(*p)
            }
            teksilo_canvas::PathCommand::QuadTo { control, to } => {
                include(*control);
                include(*to);
            }
            teksilo_canvas::PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                include(*control1);
                include(*control2);
                include(*to);
            }
            teksilo_canvas::PathCommand::ArcTo { rect, .. } => {
                include(Point::new(rect.x, rect.y));
                include(Point::new(rect.right(), rect.bottom()));
            }
            teksilo_canvas::PathCommand::Close => {}
        }
    }
    if !min_x.is_finite() {
        return None;
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::RectItem;
    use teksilo_canvas::{Size, SizeProposal};
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_tokens::Color;

    #[derive(Debug)]
    struct FillWidget;

    impl FillWidget {
        fn new() -> Self {
            Self
        }
    }

    impl Widget for FillWidget {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(0.0, 0.0).into()
        }
    }

    #[test]
    fn add_widget_round_trip() {
        let mut scene = Scene::new();
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let id = scene.add_widget(FillWidget::new(), r);
        assert_eq!(scene.len(), 1);
        // scene_rect is computed from local_pos + local_bounds.
        assert_eq!(scene.scene_rect(id), Some(r));
        assert_eq!(scene.local_pos(id), Some(Point::new(10.0, 20.0)));
        assert_eq!(
            scene.local_bounds(id),
            Some(Rect::new(0.0, 0.0, 100.0, 50.0))
        );
        assert_eq!(scene.ids(), vec![id]);
    }

    #[test]
    fn add_item_at_local_pos() {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 30.0, 40.0)).fill(Color::RED),
            Point::new(10.0, 20.0),
        );
        assert_eq!(
            scene.scene_rect(id),
            Some(Rect::new(10.0, 20.0, 30.0, 40.0))
        );
        assert_eq!(scene.scene_pos(id), Some(Point::new(10.0, 20.0)));
    }

    #[test]
    fn set_local_pos_updates_scene_rect_and_index() {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        scene.set_local_pos(id, Point::new(500.0, 500.0));
        assert_eq!(
            scene.scene_rect(id),
            Some(Rect::new(500.0, 500.0, 10.0, 10.0))
        );
        let near_origin = scene.items_in_rect(Rect::new(0.0, 0.0, 50.0, 50.0));
        assert!(!near_origin.contains(&id));
        let near_far = scene.items_in_rect(Rect::new(490.0, 490.0, 30.0, 30.0));
        assert!(near_far.contains(&id));
    }

    #[test]
    fn parent_relative_position_composes_through_chain() {
        let mut scene = Scene::new();
        let parent = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)),
            Point::new(50.0, 50.0),
        );
        let child = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(10.0, 10.0),
        );
        scene.set_item_parent(child, Some(parent));

        // Child's scene_pos = parent local_pos + child local_pos.
        assert_eq!(scene.scene_pos(child), Some(Point::new(60.0, 60.0)));
        // Move parent — child's scene_pos shifts in lockstep.
        scene.set_local_pos(parent, Point::new(150.0, 150.0));
        assert_eq!(scene.scene_pos(child), Some(Point::new(160.0, 160.0)));
    }

    #[test]
    fn set_local_pos_propagates_to_descendants_scene_pos() {
        // Three-deep chain: grandparent → parent → child.
        let mut scene = Scene::new();
        let gp = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        let p = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(20.0, 0.0),
        );
        let c = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(5.0, 0.0),
        );
        scene.set_item_parent(p, Some(gp));
        scene.set_item_parent(c, Some(p));

        assert_eq!(scene.scene_pos(c), Some(Point::new(25.0, 0.0)));
        scene.set_local_pos(gp, Point::new(100.0, 100.0));
        assert_eq!(scene.scene_pos(c), Some(Point::new(125.0, 100.0)));
    }

    #[test]
    fn remove_drops_the_entry() {
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::ZERO);
        let b = scene.add_widget(FillWidget::new(), Rect::ZERO);
        scene.remove(a);
        assert_eq!(scene.len(), 1);
        assert_eq!(scene.scene_rect(a), None);
        assert!(scene.scene_rect(b).is_some());
    }

    #[test]
    fn items_in_rect_brute_force() {
        let mut scene = Scene::new();
        let a = scene.add_widget(FillWidget::new(), Rect::new(0.0, 0.0, 10.0, 10.0));
        let b = scene.add_widget(FillWidget::new(), Rect::new(100.0, 100.0, 10.0, 10.0));
        let c = scene.add_widget(FillWidget::new(), Rect::new(5.0, 5.0, 10.0, 10.0));

        let near_origin = scene.items_in_rect(Rect::new(0.0, 0.0, 20.0, 20.0));
        assert!(near_origin.contains(&a));
        assert!(near_origin.contains(&c));
        assert!(!near_origin.contains(&b));

        let far = scene.items_in_rect(Rect::new(95.0, 95.0, 20.0, 20.0));
        assert_eq!(far, vec![b]);

        let empty = scene.items_in_rect(Rect::new(500.0, 500.0, 1.0, 1.0));
        assert!(empty.is_empty());
    }

    #[test]
    fn item_at_picks_topmost() {
        let mut scene = Scene::new();
        let bottom = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)),
            Point::new(0.0, 0.0),
        );
        let top = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)),
            Point::new(25.0, 25.0),
        );
        scene.set_z(top, 1.0);
        scene.set_z(bottom, 0.0);
        // Click in the overlap region.
        assert_eq!(scene.item_at(Point::new(50.0, 50.0)), Some(top));
        // Click outside the top, inside the bottom.
        assert_eq!(scene.item_at(Point::new(10.0, 10.0)), Some(bottom));
        // Click outside everything.
        assert_eq!(scene.item_at(Point::new(500.0, 500.0)), None);
    }

    #[test]
    fn item_accessor_returns_lightweight_only() {
        let mut scene = Scene::new();
        let widget_id = scene.add_widget(FillWidget::new(), Rect::ZERO);
        let item_id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        assert!(scene.item(item_id).is_some());
        assert!(scene.item(widget_id).is_none());
    }

    #[test]
    fn map_to_scene_round_trips() {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(50.0, 50.0),
        );
        let local = Point::new(3.0, 4.0);
        let scene_pt = scene.map_to_scene(id, local).unwrap();
        let back = scene.map_from_scene(id, scene_pt).unwrap();
        assert!((back.x - local.x).abs() < 1e-5);
        assert!((back.y - local.y).abs() < 1e-5);
    }

    #[test]
    fn rects_intersect_edge_touching_excluded() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);
        assert!(!rects_intersect(a, b));
        assert!(!rects_intersect(b, a));
    }

    #[test]
    fn flags_default_carries_visible_enabled_selectable() {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        let f = scene.flags(id).unwrap();
        assert!(f.contains(ItemFlags::IS_VISIBLE));
        assert!(f.contains(ItemFlags::IS_ENABLED));
        assert!(f.contains(ItemFlags::IS_SELECTABLE));
        assert!(!f.contains(ItemFlags::IS_DRAGGABLE));
    }

    #[test]
    fn draggable_builder_sets_is_draggable_flag() {
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).draggable(true),
            Point::ZERO,
        );
        assert!(scene.flags(id).unwrap().contains(ItemFlags::IS_DRAGGABLE));
    }

    #[test]
    fn set_visible_flag_chains_through_parent() {
        let mut scene = Scene::new();
        let parent = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        let child = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 5.0, 5.0)), Point::ZERO);
        scene.set_item_parent(child, Some(parent));
        assert!(scene.is_effectively_visible(child));
        scene.set_visible(parent, false);
        assert!(!scene.is_effectively_visible(child));
        assert!(!scene.is_effectively_visible(parent));
    }

    #[test]
    fn effective_opacity_composes_through_chain() {
        let mut scene = Scene::new();
        let p = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        let c = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 5.0, 5.0)), Point::ZERO);
        scene.set_item_parent(c, Some(p));
        scene.set_opacity(p, 0.5);
        scene.set_opacity(c, 0.5);
        assert!((scene.effective_opacity(c) - 0.25).abs() < 1e-5);
    }

    #[test]
    fn opacity_clamps_to_unit_range() {
        let mut scene = Scene::new();
        let id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        scene.set_opacity(id, 1.5);
        assert_eq!(scene.opacity(id), Some(1.0));
        scene.set_opacity(id, -0.3);
        assert_eq!(scene.opacity(id), Some(0.0));
    }

    #[test]
    fn scene_rect_extent_uses_user_set_when_present() {
        let mut scene = Scene::new();
        let user = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        scene.set_scene_rect(Some(user));
        assert_eq!(scene.scene_rect_extent(), Some(user));
        scene.set_scene_rect(None);
        // No items, no auto-extent.
        assert_eq!(scene.scene_rect_extent(), None);
    }

    #[test]
    fn scene_rect_extent_auto_unions_items_when_unset() {
        let mut scene = Scene::new();
        scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(5.0, 5.0),
        );
        scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(100.0, 100.0),
        );
        let extent = scene.scene_rect_extent().unwrap();
        // (5,5)-(15,15) ∪ (100,100)-(120,120) = (5,5)-(120,120).
        assert!((extent.x - 5.0).abs() < 1e-3);
        assert!((extent.y - 5.0).abs() < 1e-3);
        assert!((extent.width - 115.0).abs() < 1e-3);
        assert!((extent.height - 115.0).abs() < 1e-3);
    }

    #[test]
    fn pan_axes_default_is_both() {
        let scene = Scene::new();
        assert_eq!(scene.current_pan_axes(), PanAxes::Both);
        assert!(scene.is_zoomable());
    }

    #[test]
    fn pan_axes_set_round_trip() {
        let mut scene = Scene::new();
        scene.pan_axes(PanAxes::Horizontal);
        assert_eq!(scene.current_pan_axes(), PanAxes::Horizontal);
        scene.zoomable(false);
        assert!(!scene.is_zoomable());
    }

    #[test]
    fn item_change_signal_fires_on_set_local_pos() {
        use std::cell::Cell;
        use std::rc::Rc;
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        let last = Rc::new(Cell::new(None::<ItemChange>));
        let last_clone = last.clone();
        let _h = scene.item_change_signal().observe(move |c| {
            last_clone.set(Some(*c));
        });
        scene.set_local_pos(id, Point::new(50.0, 60.0));
        match last.get() {
            Some(ItemChange::LocalPosChanged { new, .. }) => {
                assert_eq!(new, Point::new(50.0, 60.0));
            }
            other => panic!("expected LocalPosChanged, got {:?}", other),
        }
    }

    #[test]
    fn item_change_signal_fires_on_set_visible() {
        use std::cell::Cell;
        use std::rc::Rc;
        let mut scene = Scene::new();
        let id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        let count = Rc::new(Cell::new(0_u32));
        let count_clone = count.clone();
        let _h = scene.item_change_signal().observe(move |c| {
            if matches!(c, ItemChange::VisibilityChanged { .. }) {
                count_clone.set(count_clone.get() + 1);
            }
        });
        scene.set_visible(id, false);
        scene.set_visible(id, true);
        // Same value twice: only one fire.
        scene.set_visible(id, true);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn colliding_items_returns_overlapping_set_excluding_self() {
        let mut scene = Scene::new();
        let a = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)),
            Point::new(10.0, 10.0),
        );
        let b = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)),
            Point::new(40.0, 10.0),
        );
        let c = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(500.0, 500.0),
        );
        let collisions = scene.colliding_items(a);
        assert!(collisions.contains(&b));
        assert!(!collisions.contains(&a));
        assert!(!collisions.contains(&c));
    }

    #[test]
    fn items_along_path_finds_items_under_path_aabb() {
        let mut scene = Scene::new();
        let a = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(20.0, 20.0),
        );
        let b = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(200.0, 200.0),
        );
        let mut path = Path::new();
        path.move_to(Point::new(15.0, 15.0));
        path.line_to(Point::new(40.0, 40.0));
        let hits = scene.items_along_path(&path);
        assert!(hits.contains(&a));
        assert!(!hits.contains(&b));
    }

    // -----------------------------------------------------------------
    // R6 — code-level fixes
    // -----------------------------------------------------------------

    #[test]
    fn scene_remove_recursively_removes_descendants() {
        let mut scene = Scene::new();
        let parent = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)), Point::ZERO);
        let child = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)), Point::ZERO);
        let grandchild = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 5.0, 5.0)), Point::ZERO);
        scene.set_item_parent(child, Some(parent));
        scene.set_item_parent(grandchild, Some(child));
        assert_eq!(scene.entries.len(), 3);
        scene.remove(parent);
        // Parent + child + grandchild all gone.
        assert!(scene.scene_rect(parent).is_none());
        assert!(scene.scene_rect(child).is_none());
        assert!(scene.scene_rect(grandchild).is_none());
        assert_eq!(scene.entries.len(), 0);
    }

    #[test]
    fn scene_orphan_promotes_children_to_root() {
        let mut scene = Scene::new();
        let parent = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)), Point::ZERO);
        let child = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)), Point::ZERO);
        scene.set_item_parent(child, Some(parent));
        scene.orphan(parent);
        // Child's parent is now None.
        assert_eq!(scene.parent_of(child), None);
        // Both still present.
        scene.remove(parent);
        assert!(scene.scene_rect(child).is_some());
    }

    #[test]
    fn scene_orphan_rebuckets_detached_children() {
        // After orphaning, the spatial index must reflect children's
        // new scene-AABBs. Move a parent off-origin, attach a child,
        // then orphan — items_in_rect at the child's *child-local*
        // origin must now return it (because its scene_transform no
        // longer composes the parent's offset).
        let mut scene = Scene::new();
        let parent = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(500.0, 500.0),
        );
        let child = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        scene.set_item_parent(child, Some(parent));
        // Pre-orphan: child sits at scene (500, 500).
        assert!(
            scene
                .items_in_rect(Rect::new(495.0, 495.0, 20.0, 20.0))
                .contains(&child)
        );
        scene.orphan(parent);
        // Post-orphan: child sits at scene (0, 0); the index must
        // reflect that — query at the new origin must hit, query at
        // the old origin must miss.
        assert!(
            scene
                .items_in_rect(Rect::new(-5.0, -5.0, 20.0, 20.0))
                .contains(&child)
        );
        assert!(
            !scene
                .items_in_rect(Rect::new(495.0, 495.0, 20.0, 20.0))
                .contains(&child)
        );
    }

    #[test]
    fn add_item_dynamic_re_reads_bounds_on_refresh() {
        // An item whose `local_bounds` reads from a Cell. Mutating
        // the cell + calling refresh_dynamic_bounds must update the
        // entry and re-bucket the spatial index.
        use crate::item::{SceneItem, SceneItemPaintContext};
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Debug)]
        struct DynRect {
            bounds: Rc<Cell<Rect>>,
        }
        impl SceneItem for DynRect {
            fn local_bounds(&self) -> Rect {
                self.bounds.get()
            }
            fn set_local_bounds(&mut self, b: Rect) {
                self.bounds.set(b);
            }
            fn paint(&self, _: &mut teksilo_canvas::Canvas, _: &SceneItemPaintContext<'_>) {}
        }

        let bounds = Rc::new(Cell::new(Rect::new(0.0, 0.0, 10.0, 10.0)));
        let mut scene = Scene::new();
        let id = scene.add_item_dynamic(
            DynRect {
                bounds: bounds.clone(),
            },
            Point::ZERO,
        );
        // Initially items_in_rect over the small AABB hits.
        assert!(
            scene
                .items_in_rect(Rect::new(0.0, 0.0, 50.0, 50.0))
                .contains(&id)
        );
        // Grow the bounds via the Cell — Scene's cached entry/index
        // is stale until refresh_dynamic_bounds runs.
        bounds.set(Rect::new(0.0, 0.0, 500.0, 500.0));
        scene.refresh_dynamic_bounds();
        // After refresh, the spatial index sees the larger AABB.
        assert!(
            scene
                .items_in_rect(Rect::new(400.0, 400.0, 10.0, 10.0))
                .contains(&id)
        );
    }

    #[test]
    fn add_item_static_does_not_track_signal_changes() {
        // Counterpart to the dynamic test: a static item's bounds
        // are snapshotted at insert time; refresh_dynamic_bounds
        // does not re-read them.
        use crate::item::{SceneItem, SceneItemPaintContext};
        use std::cell::Cell;
        use std::rc::Rc;

        #[derive(Debug)]
        struct DynRect {
            bounds: Rc<Cell<Rect>>,
        }
        impl SceneItem for DynRect {
            fn local_bounds(&self) -> Rect {
                self.bounds.get()
            }
            fn set_local_bounds(&mut self, b: Rect) {
                self.bounds.set(b);
            }
            fn paint(&self, _: &mut teksilo_canvas::Canvas, _: &SceneItemPaintContext<'_>) {}
        }

        let bounds = Rc::new(Cell::new(Rect::new(0.0, 0.0, 10.0, 10.0)));
        let mut scene = Scene::new();
        let id = scene.add_item(
            DynRect {
                bounds: bounds.clone(),
            },
            Point::ZERO,
        );
        bounds.set(Rect::new(0.0, 0.0, 500.0, 500.0));
        scene.refresh_dynamic_bounds();
        // Static entry's spatial index unchanged.
        assert!(
            !scene
                .items_in_rect(Rect::new(400.0, 400.0, 10.0, 10.0))
                .contains(&id)
        );
    }
}
