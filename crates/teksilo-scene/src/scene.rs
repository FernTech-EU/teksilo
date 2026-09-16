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
//! // Each notification is a `SceneChange`: the change itself, plus the
//! // transaction it belongs to, whose it was, and whether it counts as
//! // history. See `teksilo_scene::SceneChange`.
//! let _guard = scene.item_change_signal().observe(|notification| {
//!     if let ItemChange::LocalPosChanged { id: _, old: _, new } = &notification.change {
//!         let _ = new; // react to the new position
//!     }
//! });
//!
//! // Move the item; the observer fires and the spatial index updates.
//! scene.set_local_pos(id, Point::new(100.0, 100.0));
//! assert_eq!(scene.scene_pos(id), Some(Point::new(100.0, 100.0)));
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::a11y::{A11yCategory, A11yGroup, A11yGroupBuilder, A11yGroupId, A11yNode, A11yRelation};
use crate::flags::ItemFlags;
use crate::index::{GridHashIndex, SpatialIndex};
use crate::item::{AppearanceWrite, ItemId, SceneItem};
use crate::item_handlers::SceneItemHandlerSet;
use crate::journal::{
    ChangeSource, EditJournal, EphemeralScope, HistoryMode, RemovalScope, Salvage, SceneChange,
    SceneEdit,
};
use crate::magnet::{Magnet, MagnetId, MagnetRef, MagnetSnap, MagnetVerdict};
use crate::pick::PaintKey;
use crate::salvage::{
    ItemA11yDecorations, RemovedItem, ReplaceItemError, ReplaceRejected, RestoreError,
};
use crate::shape::{ItemSelectionMode, ItemShape, SceneRegion};
use crate::transform::local_to_parent;
use teksilo_canvas::{Path, Point, Rect, StrokeStyle, Transform2D, Vec2};
use teksilo_core::color_prop::ColorProp;
use teksilo_core::signal::Signal;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;

/// A change to an item's state, fired through
/// [`Scene::item_change_signal`] for every mutation. Apps observe to wire
/// validation, persistence, telemetry, mirroring to a data layer, and other
/// side effects. The model is "fire after the change has been applied" — by
/// the time the observer sees the event, the Scene already reflects it, and
/// (when the mutation came through a [`SceneModel`](crate::SceneModel)) the
/// observer may freely read *and* write the scene back.
///
/// # Edits and derived notifications
///
/// Most variants are **edits**: one mutation, one variant, both sides of the
/// value it replaced, and [`SceneTransactionRecord::edits`](crate::SceneTransactionRecord)
/// carries them so a data layer can invert the transaction by walking them in
/// reverse.
///
/// [`VisibilityChanged`](Self::VisibilityChanged) is not one. It is a
/// *derived* notification — a convenience beside the
/// [`FlagsChanged`](Self::FlagsChanged) that actually describes the mutation,
/// emitted by both flag doors whenever `IS_VISIBLE` flips so a consumer need
/// not diff two bitsets. It rides the signal and stays out of the record, so
/// hiding a card is one edit whichever door hid it.
/// [`is_edit`](Self::is_edit) is the test, for a consumer that counts changes
/// off the signal and wants the same number the record has.
///
/// `#[non_exhaustive]`: this is the crate's outbound event vocabulary, matched
/// by every observer, and it grows whenever the scene learns to report
/// something new — `HandlersChanged` is the most recent. Without the
/// attribute each such addition would stop a downstream `match` from
/// compiling; with it, a consumer's wildcard arm keeps meaning "a change I do
/// not act on".
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ItemChange {
    /// `set_local_pos`: position in parent coords moved.
    LocalPosChanged {
        /// The item that moved.
        id: ItemId,
        /// Where it was, in its parent's frame.
        old: Point,
        /// Where it is now.
        new: Point,
    },
    /// `set_local_bounds`: AABB in local coords changed.
    LocalBoundsChanged {
        /// The item that resized.
        id: ItemId,
        /// Its AABB in local coordinates before the write.
        old: Rect,
        /// Its AABB now.
        new: Rect,
    },
    /// `set_transform`: local→parent transform changed.
    TransformChanged {
        /// The item whose basis changed.
        id: ItemId,
        /// Its local→parent transform before the write.
        old: Transform2D,
        /// Its local→parent transform now.
        new: Transform2D,
    },
    /// `IS_VISIBLE` flipped, by whichever door flipped it — `set_visible`,
    /// `set_flag` or a wholesale `set_flags`.
    ///
    /// A **derived notification**, always emitted immediately before the
    /// [`FlagsChanged`](Self::FlagsChanged) that describes the same mutation.
    /// It exists so a consumer that only cares about visibility need not diff
    /// two [`ItemFlags`] bitsets, and it is deliberately not an edit:
    /// [`is_edit`](Self::is_edit) is `false` for it and it never reaches a
    /// transaction record, so one mutation is one recorded edit no matter which
    /// door made it. (It used to be emitted by `set_flag` and not by
    /// `set_flags`, which made hiding a card two recorded changes through one
    /// door and one through the other.)
    VisibilityChanged {
        /// The item whose `IS_VISIBLE` bit flipped.
        id: ItemId,
        /// The bit's new value. Its own value, not the inherited one —
        /// [`Scene::is_effectively_visible`] chains the parents.
        visible: bool,
    },
    /// `set_flags` / `set_flag` changed the bitset. The edit that describes a
    /// flag change, visibility included.
    FlagsChanged {
        /// The item whose flags changed.
        id: ItemId,
        /// The whole bitset before the write.
        old: ItemFlags,
        /// The whole bitset now. Diff the two to see which bits moved.
        new: ItemFlags,
    },
    /// `set_opacity`: local opacity multiplier changed.
    OpacityChanged {
        /// The item whose opacity changed.
        id: ItemId,
        /// Its local multiplier before the write.
        old: f32,
        /// Its local multiplier now.
        new: f32,
    },
    /// `set_z`: paint z-order changed.
    ZChanged {
        /// The item that restacked.
        id: ItemId,
        /// Its paint z before the write.
        old: f32,
        /// Its paint z now.
        new: f32,
    },
    /// `set_layer`: the Under/Over paint band changed.
    LayerChanged {
        /// The item that changed band.
        id: ItemId,
        /// The band it was in.
        old: SceneLayer,
        /// The band it is in now.
        new: SceneLayer,
    },
    /// `set_item_parent`: logical parent changed.
    ParentChanged {
        /// The child that was reparented.
        id: ItemId,
        /// Its logical parent before the write; `None` when it was a root.
        old: Option<ItemId>,
        /// Its logical parent now; `None` when it is now a root.
        new: Option<ItemId>,
    },
    /// `remove`: item is gone.
    Removed {
        /// The item that is gone. Its contents travel the owning channel:
        /// [`Scene::take`] hands the caller a [`RemovedItem`],
        /// and [`Scene::remove`] routes one to the edit sink.
        id: ItemId,
    },
    /// `add_item` / `add_widget`: item was inserted.
    Added {
        /// The item that was inserted.
        id: ItemId,
    },
    /// `set_payload`: the type-erased payload of a `Delegated` heavyweight
    /// entry was replaced. A `SceneView` rebuilds that entry's widget
    /// (re-invokes its delegate) on the next build. Routed through
    /// `emit_item_change`, so `mutation_seq` advances and the AT-walk gate
    /// notices.
    PayloadChanged {
        /// The `Delegated` heavyweight entry whose data was replaced.
        id: ItemId,
        /// The payload every view built its widget from until now.
        old: ItemPayload,
        /// The payload every view will rebuild from.
        new: ItemPayload,
    },
    /// `set_item_fill` / `set_item_stroke` / `clear_item_*`: a lightweight
    /// item's paint-only appearance (fill / stroke colour or style) changed.
    /// Never moves geometry, so the observing `SceneView` evicts the item's
    /// cached frame and repaints **without** relayout or rebuild.
    ///
    /// It *can* move the item's hit **shape**, though: a stroked
    /// [`PathItem`](crate::PathItem) derives its hit band from the stroke it
    /// draws, so a view caching hit geometry must re-read this item's shape
    /// even while skipping relayout.
    AppearanceChanged {
        /// The lightweight item that was recoloured.
        id: ItemId,
        /// Which of fill or stroke moved, and to what.
        change: AppearanceChange,
    },
    /// `replace_item`: the lightweight item box at this id was swapped for a
    /// different one, keeping the entry — position, transform, z, layer,
    /// parent, flags, opacity, handlers, magnets and logical-AT decorations
    /// all survive, and so does the [`ItemId`].
    ///
    /// One change, not `Removed` + `Added`, because nothing about the item's
    /// identity changed. Consumers must treat it as a **geometry** change: the
    /// new item carries its own `local_bounds` and its own hit shape.
    ///
    /// `old_bounds` / `new_bounds` are the entry's AABB either side of the
    /// swap; the replaced box itself is returned to whoever called
    /// [`Scene::replace_item`].
    ItemReplaced {
        /// The entry whose item box was swapped. Its [`ItemId`] is unchanged.
        id: ItemId,
        /// The entry's AABB before the swap.
        old_bounds: Rect,
        /// The entry's AABB after it, carried by the new box.
        new_bounds: Rect,
    },
    /// `set_placement`: parent, z, local position and transform written
    /// together as one property.
    ///
    /// The atomic form of the four separate mutators. A visually-stable
    /// reparent ("drag this card into that group") is one edit here, where
    /// `set_item_parent` + `set_local_pos` + `set_transform` is three edits,
    /// three undo steps, and two intermediate states in which the item is
    /// visibly in the wrong place.
    PlacementChanged {
        /// The item that was placed.
        id: ItemId,
        /// Parent, z, local position and transform before the write.
        old: Placement,
        /// All four now.
        new: Placement,
    },
    /// `set_item_handlers` / `handlers_mut`: the item's handler set was
    /// replaced or handed out for mutation.
    ///
    /// Always means at least "this item's handlers are no longer what you last
    /// read", which is what a consumer caching them — the `SceneView`'s
    /// dispatch snapshot — needs. Without it, the two handler mutators were the
    /// only doors in the model that changed observable state silently.
    ///
    /// Whether it *also* describes the change is
    /// [`replaced`](Self::HandlersChanged::replaced); see
    /// [`HandlerReplacement`].
    HandlersChanged {
        /// The item whose handlers changed.
        id: ItemId,
        /// Both sides, when the door that fired this knew them.
        ///
        /// `Some` from [`Scene::set_item_handlers`], which is handed the new
        /// set and can clone the old one out — so that door produces a
        /// reversible edit like every other mutator.
        ///
        /// `None` from [`Scene::handlers_mut`], which fires on the way *in*: a
        /// `&mut` borrow cannot report what the caller will do with it, and
        /// there is no later moment the scene is told about. That call is an
        /// invalidation notice and nothing finer, so it is **not** recorded as
        /// an edit — a record claiming to carry both sides must not carry an
        /// entry that carries neither. An app that wants its handler edits in
        /// the history makes them through `set_item_handlers`.
        ///
        /// Boxed because a [`SceneItemHandlerSet`] is a wide struct and this is
        /// the rarest variant: inline, two of them would set the size of every
        /// [`ItemChange`], every [`SceneChange`] and every
        /// queued notification — a per-pointer-sample cost for a setup-time
        /// call.
        replaced: Option<Box<HandlerReplacement>>,
    },
}

/// The two sides of a [`Scene::set_item_handlers`], as they ride an
/// [`ItemChange::HandlersChanged`].
///
/// `None` on either side means "no handler set at all", which is a state an
/// item can be in and is distinct from an empty one.
///
/// [`SceneItemHandlerSet`] stores its closures as `Rc<dyn Fn>`, so carrying
/// both sides is a handful of refcount bumps rather than a deep copy — which
/// is why this could be carried and, until it was, simply was not.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Clone)]
#[non_exhaustive]
pub struct HandlerReplacement {
    /// What the item had before. `None` when it had none.
    pub old: Option<SceneItemHandlerSet>,
    /// What it has now. `None` when the call cleared them.
    pub new: Option<SceneItemHandlerSet>,
}

impl std::fmt::Debug for HandlerReplacement {
    /// Hand-written: [`SceneItemHandlerSet`] holds `Rc<dyn Fn>` closures and is
    /// not `Debug`, and [`ItemChange`] is. Reports which side was present,
    /// which is the part of a handler swap that reads usefully in a log — the
    /// same call the hand-written `Debug` for
    /// [`RemovedItem`] makes about its magnets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn side(set: &Option<SceneItemHandlerSet>) -> &'static str {
            match set {
                Some(_) => "SceneItemHandlerSet",
                None => "none",
            }
        }
        f.debug_struct("HandlerReplacement")
            .field("old", &side(&self.old))
            .field("new", &side(&self.new))
            .finish()
    }
}

impl ItemChange {
    /// The item this change is about.
    ///
    /// Every variant names exactly one item, so an observer that only needs
    /// *which* item moved need not match the whole enum. The fan-out's runaway
    /// detector reads it too: it charges each delivery to the subject it is
    /// about, so it needs that subject without caring what kind of change it
    /// is. (Those per-subject counts name the culprit in the panic; the bound
    /// that trips is a flat total — see [`CascadeBudget`].)
    pub fn id(&self) -> ItemId {
        match *self {
            ItemChange::LocalPosChanged { id, .. }
            | ItemChange::LocalBoundsChanged { id, .. }
            | ItemChange::TransformChanged { id, .. }
            | ItemChange::VisibilityChanged { id, .. }
            | ItemChange::FlagsChanged { id, .. }
            | ItemChange::OpacityChanged { id, .. }
            | ItemChange::ZChanged { id, .. }
            | ItemChange::LayerChanged { id, .. }
            | ItemChange::ParentChanged { id, .. }
            | ItemChange::Removed { id }
            | ItemChange::Added { id }
            | ItemChange::PayloadChanged { id, .. }
            | ItemChange::AppearanceChanged { id, .. }
            | ItemChange::ItemReplaced { id, .. }
            | ItemChange::PlacementChanged { id, .. }
            | ItemChange::HandlersChanged { id, .. } => id,
        }
    }

    /// Whether this change is an **edit** — a mutation a transaction record
    /// carries — rather than a *derived notification* the scene emits beside
    /// one for a consumer's convenience.
    ///
    /// Two variants are not edits:
    ///
    /// * [`VisibilityChanged`](Self::VisibilityChanged), which always
    ///   accompanies the [`FlagsChanged`](Self::FlagsChanged) that describes
    ///   the same mutation.
    /// * [`HandlersChanged`](Self::HandlersChanged) with no
    ///   [`replaced`](Self::HandlersChanged::replaced) — the `handlers_mut`
    ///   door, which cannot say what the handlers became.
    ///
    /// Filtering the change signal by this gives the same count the
    /// transaction record has, which is what an app showing "N changes in this
    /// edit" needs: hiding a card is one change through
    /// [`Scene::set_visible`] and one through [`Scene::set_flags`].
    pub fn is_edit(&self) -> bool {
        !matches!(
            self,
            ItemChange::VisibilityChanged { .. }
                | ItemChange::HandlersChanged { replaced: None, .. }
        )
    }
}

/// A `Delegated` heavyweight entry's type-erased payload, as it rides an
/// [`ItemChange::PayloadChanged`].
///
/// A newtype rather than a bare `Rc<dyn Any>` for one reason: `dyn Any` is not
/// `Debug`, and [`ItemChange`] is. Cloning one is a refcount bump, never a deep
/// copy, so carrying both sides of a payload swap costs two increments.
#[derive(Clone)]
pub struct ItemPayload(Rc<dyn std::any::Any>);

impl std::fmt::Debug for ItemPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Any` can report its concrete `TypeId` but not its name, and the
        // payload is the app's own type — printing the pointer would be noise.
        f.write_str("ItemPayload(<dyn Any>)")
    }
}

impl ItemPayload {
    /// The underlying handle, for a consumer that wants to downcast it itself.
    pub fn as_rc(&self) -> &Rc<dyn std::any::Any> {
        &self.0
    }

    /// Consume the wrapper for the handle.
    pub fn into_rc(self) -> Rc<dyn std::any::Any> {
        self.0
    }

    /// Downcast to the concrete payload type the app stored, or `None` when it
    /// is something else.
    pub fn downcast<T: 'static>(&self) -> Option<Rc<T>> {
        self.0.clone().downcast::<T>().ok()
    }
}

impl From<Rc<dyn std::any::Any>> for ItemPayload {
    fn from(rc: Rc<dyn std::any::Any>) -> Self {
        Self(rc)
    }
}

/// Which paint-only appearance slot changed, with both sides of the write.
///
/// One variant per slot rather than one struct carrying both, because a write
/// touches exactly one of them and a struct would have to invent a value for
/// the other — which is the class of "plausible but wrong" data a journal must
/// not contain.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers, and an item may
/// grow a third appearance slot.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AppearanceChange {
    /// [`Scene::set_item_fill`] / [`Scene::clear_item_fill`]. `None` on either
    /// side means "no fill".
    Fill {
        /// The fill the item held before the write.
        old: Option<ColorProp>,
        /// The fill it holds now.
        new: Option<ColorProp>,
    },
    /// [`Scene::set_item_stroke`] / [`Scene::clear_item_stroke`]. `None` on
    /// either side means "no stroke".
    Stroke {
        /// The stroke the item held before the write.
        old: Option<(ColorProp, StrokeStyle)>,
        /// The stroke it holds now.
        new: Option<(ColorProp, StrokeStyle)>,
    },
}

/// Where an item sits, as **one** property: logical parent, paint z, position
/// in the parent frame, and local to parent transform.
///
/// # Why these four together
///
/// [`Scene::set_item_parent`] deliberately does not rebase `local_pos` — a
/// child's position is stated in its parent's frame, so adopting a new parent
/// moves the item unless the caller compensates. A visually-stable reparent is
/// therefore `set_item_parent` + `set_local_pos` + `set_transform`: three
/// events, three things for a history to undo separately, and two intermediate
/// states in which the item is visibly somewhere it never was.
///
/// Writing all four at once removes all three, and it is the half of Figma's
/// "parent + fractional index as one property" that Teksilo was missing. The
/// *other* half — O(1) reorder with no sibling rewrite — is already here: `z`
/// is an `f32` and [`Scene::set_z`] writes one field, so
/// [`Scene::z_between`] is a fractional insert.
///
/// `#[non_exhaustive]`: the crate hands this *to* consumer code and takes it
/// back — and it is the one place a caller states a whole placement, so a fifth
/// property joining the set is exactly the growth this guards. Build one with
/// [`new`](Self::new).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Placement {
    /// Logical parent, or `None` for scene-rooted.
    pub parent: Option<ItemId>,
    /// Paint z-order within the band.
    pub z: f32,
    /// Origin of the item's local frame, in the parent's coordinates.
    pub local_pos: Point,
    /// Rotation / scale applied around the local origin.
    pub transform: Transform2D,
}

impl Placement {
    /// A placement stated field by field — the constructor
    /// [`#[non_exhaustive]`](Self) takes the place of a struct literal for.
    pub fn new(parent: Option<ItemId>, z: f32, local_pos: Point, transform: Transform2D) -> Self {
        Self {
            parent,
            z,
            local_pos,
            transform,
        }
    }
}

/// One queued notification, awaiting a drain by whoever opened the write
/// scope that produced it.
///
/// The two change channels share a single FIFO so that a mutation touching
/// item geometry **and** logical AT structure delivers in the order it
/// happened, rather than splitting into two independently-ordered streams.
///
/// Deliberately covers only the two *notification* channels. The scene's
/// constraint signals (`pan_axes` / `zoomable` / `pan_bounds` / `zoom_range`)
/// are **state**, not events — [`Scene::current_pan_axes`] and friends read
/// them back — so deferring their writes would make the scene lie about its
/// own configuration inside an open write scope. They stay synchronous.
#[derive(Debug, Clone)]
pub(crate) enum PendingNotification {
    /// One [`SceneChange`] — an [`ItemChange`] plus its transaction envelope —
    /// through `item_change_signal`.
    Item(SceneChange),
    /// One `a11y_change_signal` bump, carrying the node the mutation was
    /// *about* (groups / parents / relations / live / landmarks / categories /
    /// magnets).
    ///
    /// The delivery itself is a bare counter bump — the signal carries no
    /// payload, because an AT re-walk reads the whole logical tree back. The
    /// node rides along for one reason: the runaway detector charges each
    /// delivery to its subject, and an AT channel with no subject would put
    /// every logical-AT notification in the scene on a single budget. On a
    /// crate whose differentiator is per-item accessibility, that is the one
    /// channel that must not be the bottleneck.
    A11y(A11yNode),
}

/// What the runaway detector counts a delivery against: the **subject** a
/// notification is about, on either channel.
///
/// Per *subject*, not per change kind or per channel: a runaway rewrites the
/// same subject over and over, and which field of it — or which of the two
/// signals — is not what makes it a runaway. Keying on `(subject, kind)` would
/// let a two-field ping-pong (`set_local_pos` reacting to `ZChanged` and back)
/// spend two budgets instead of one, and keying the whole logical-AT channel on
/// one key would charge an app that maintains AT structure per item — the
/// intended use — as if every item were the same subject.
///
/// So an `ItemChange` about item *i* and an `a11y_change_signal` bump about
/// `A11yNode::Item(i)` share one budget: they are the same subject seen through
/// two channels, and a cycle that ping-pongs between them is one runaway, not
/// two half-runaways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NotifyKey {
    /// One scene entry: every [`ItemChange`] about it, and every logical-AT
    /// notification whose subject is `A11yNode::Item(id)`.
    Item(ItemId),
    /// One virtual AT group (`A11yNode::Group`).
    A11yGroup(A11yGroupId),
    /// One widget addressed directly in the logical AT tree
    /// (`A11yNode::Widget`) — a descendant relocated into the scene's AT tree
    /// without being a scene entry of its own.
    A11yWidget(WidgetId),
}

impl From<A11yNode> for NotifyKey {
    fn from(node: A11yNode) -> Self {
        match node {
            A11yNode::Item(id) => NotifyKey::Item(id),
            A11yNode::Group(id) => NotifyKey::A11yGroup(id),
            A11yNode::Widget(id) => NotifyKey::A11yWidget(id),
        }
    }
}

/// Observer-generated deliveries one drain may make before it is called a
/// runaway. See [`CascadeBudget::total`].
const DEFAULT_CASCADE_TOTAL: u64 = 100_000;

/// The smallest budget a scene will hold. A budget of zero would trip on the
/// first delivery an observer generated — i.e. it would forbid the reactive
/// write-back this whole channel is advertised for — and its diagnostic would
/// have no cascade to describe, so [`Scene::set_cascade_budget`] clamps to
/// this instead of storing it. See [`CascadeBudget::new`].
const MIN_CASCADE_TOTAL: u64 = 1;

/// The runaway-detection budget for one scene's change fan-out — how much work
/// **observers** may generate from one batch before the drain declares the
/// cascade non-terminating and panics.
///
/// # Why a budget at all
///
/// The drain runs until its queue is empty, which is what lets an observer read
/// the scene and write it back. An observer whose write never converges
/// therefore never empties it, and — unlike `Signal::try_set`, which recurses
/// and so blows the stack loudly on its own — this is an iterative loop whose
/// only other exit is an empty queue. Unchecked it is a frozen UI thread with
/// no diagnostic. The limit is enforced in **every** build profile for that
/// reason.
///
/// "Never converges" is the precise condition, and it is not the same as
/// "unguarded". The geometry mutators already suppress a write that changes
/// nothing — [`Scene::set_local_pos`] returns without emitting when the
/// position is unchanged — so an observer that keeps writing the *same*
/// position settles on its own. A geometry cascade that does not settle is one
/// computing a *different* value every time: an accumulating offset, a rounding
/// drift, a spring with no rest state. The `set_a11y_*` mutators do not
/// self-suppress; they bump on every call, so there an equality check before
/// the call is a real guard.
///
/// # One trip condition, and why it is flat
///
/// **Total observer-generated deliveries in a single drain**, counted across
/// both channels and every subject. Nothing else trips.
///
/// Two earlier shapes of this budget each fixed the previous one's false
/// positive and bought a worse problem, and the third is why this one is flat:
///
/// - A cap on **rounds** aborted a legitimate 300-link settling chain, because a
///   round is what was queued when it began, so an N-link chain costs N rounds.
/// - A cap **per subject** moved the limit onto fan-in: a guarded relaxation
///   over a hub passed at 4000 incident edges and aborted at 4200.
/// - **Scaling** the per-subject cap with the scene's entry count fixed the
///   fan-in false positive and destroyed the guard, because the bound it
///   resolved grew with the model.
///
/// A flat total is the only one of the three that bounds a runaway to the same
/// amount of work whatever the scene's size. Measured in release, changing only
/// this budget: an unguarded write-back aborts after 100 001 deliveries in a
/// 10 000-entry scene and in a 50 000-entry one alike, where the budget the
/// scaled design resolved for those scenes (640 000 and 3 200 000) let the same
/// loop run 6× and 32× longer — 3.10 s and 131 s against 520 ms and 3.95 s. A
/// spawner allocates ~100 000 items before tripping at either size, against
/// 640 002 and **3 200 002** under the scaled budget.
///
/// (The flat write-back's wall clock still grows with the scene, and that is the
/// *app's* write, not the drain: `Scene::set_local_pos` re-buckets the moved
/// subtree, which walks every entry. The identical runaway on the logical-AT
/// channel, whose mutator does not, aborts in 4.8 ms at both sizes — that is
/// what this loop itself costs. The budget bounds how many times an observer's
/// write runs, not what one costs.)
///
/// The default also clears every legitimate shape this tier runs by more than an
/// order of magnitude: a 4200-edge guarded aggregate is about 4 200 deliveries,
/// and a 2000-link chain that re-places eight port magnets and refreshes three
/// AT properties per link is about 24 000.
///
/// # What it still does not decide
///
/// "Guarded cascade that is genuinely enormous" and "unguarded write-back" are
/// not distinguishable from the queue alone. This budget does not pretend
/// otherwise: it draws the line where the shapes this tier actually runs stop,
/// reports the subject with the highest delivery count so the diagnostic points
/// somewhere, and
/// [`SceneModel::set_cascade_budget`](crate::SceneModel::set_cascade_budget) is
/// the supported answer for a graph that genuinely lives past it. Mechanism in
/// the framework, policy in the consumer.
///
/// # What is exempt
///
/// The caller's own batch — everything already queued when the drain began — is
/// not charged, however large. A bulk load, a ten-thousand-item teardown, or a
/// scene-wide a11y re-tag inside one [`SceneWriteGuard`](crate::SceneWriteGuard)
/// is finite by construction, so charging it would make the budget a cap on
/// batch size instead — the mistake a plain delivery counter makes, whose fix is
/// to raise it until it catches nothing. Cascade depth and the number of
/// *distinct* subjects a cascade touches are not limited either.
///
/// ```
/// use teksilo_scene::{CascadeBudget, SceneModel};
///
/// let model = SceneModel::new();
/// model.set_cascade_budget(CascadeBudget::new(1_000_000));
/// assert_eq!(model.cascade_budget().total, 1_000_000);
/// ```
///
/// `#[non_exhaustive]`: a budget is built through
/// [`CascadeBudget::new`](Self::new) or [`Default`], never by struct literal,
/// so a second dimension (a per-subject cap, a depth limit) can be added
/// without breaking a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CascadeBudget {
    /// Observer-generated deliveries one drain may make before it panics.
    ///
    /// Flat: not scaled by the scene's entry count, by design — see the type's
    /// own docs for the measurements that settled that. Default 100 000.
    ///
    /// Zero is not a usable budget and is clamped to 1 by
    /// [`CascadeBudget::new`] and by
    /// [`SceneModel::set_cascade_budget`](crate::SceneModel::set_cascade_budget).
    pub total: u64,
}

impl Default for CascadeBudget {
    fn default() -> Self {
        Self {
            total: DEFAULT_CASCADE_TOTAL,
        }
    }
}

impl CascadeBudget {
    /// A budget of `total` observer-generated deliveries per drain, clamped up
    /// to the smallest usable value.
    ///
    /// The clamp is here rather than at the trip so that a nonsensical budget is
    /// rejected where it is written, instead of producing a panic whose numbers
    /// describe no cascade that happened.
    pub fn new(total: u64) -> Self {
        Self {
            total: total.max(MIN_CASCADE_TOTAL),
        }
    }
}

/// The scene's deferred-notification queue and the two signals it feeds.
///
/// Lives behind an `Rc` *beside* the scene rather than inside the borrow, so a
/// drain runs holding **no** borrow on the [`Scene`] at all — which is exactly
/// what lets an observer read the scene and write it back (see
/// [`SceneModel::flush_changes`](crate::SceneModel::flush_changes)).
pub(crate) struct ChangeQueue {
    /// Queued notifications in emission order. A `VecDeque` because the drain
    /// consumes from the front one at a time while observers append to the
    /// back; see [`ChangeQueue::drain`] for why it is never emptied in bulk.
    notifications: RefCell<VecDeque<PendingNotification>>,
    /// `true` while a drain loop is running, so a drain re-entered from inside
    /// an observer returns immediately and lets the outermost loop pick that
    /// observer's changes up. Cleared by [`DrainGuard`], so an observer that
    /// panics mid-drain cannot leave it stuck — a stuck flag would silence the
    /// scene permanently, which is worse than the panic this whole mechanism
    /// replaced.
    draining: Cell<bool>,
    item_signal: Signal<SceneChange>,
    a11y_signal: Signal<u64>,
}

impl std::fmt::Debug for ChangeQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangeQueue")
            .field("queued", &self.notifications.borrow().len())
            .field("draining", &self.draining.get())
            .finish()
    }
}

/// RAII clear of [`ChangeQueue::draining`]. Holds the queue `Rc` directly, so
/// `Drop` never needs the `RefCell<Scene>` — which may be unavailable during an
/// unwind out of a panicking observer.
struct DrainGuard(Rc<ChangeQueue>);

impl Drop for DrainGuard {
    fn drop(&mut self) {
        self.0.draining.set(false);
    }
}

impl ChangeQueue {
    fn new() -> Self {
        Self {
            notifications: RefCell::new(VecDeque::new()),
            draining: Cell::new(false),
            item_signal: Signal::new(SceneChange {
                txn: crate::journal::TxnId::default(),
                source: ChangeSource::Programmatic,
                history: HistoryMode::Record,
                ephemeral: false,
                change: ItemChange::Added { id: ItemId(0) },
            }),
            a11y_signal: Signal::new(0),
        }
    }

    pub(crate) fn item_signal(&self) -> Signal<SceneChange> {
        self.item_signal.clone()
    }

    pub(crate) fn a11y_signal(&self) -> Signal<u64> {
        self.a11y_signal.clone()
    }

    /// Whether anything is waiting to be delivered.
    pub(crate) fn has_work(&self) -> bool {
        !self.notifications.borrow().is_empty()
    }

    /// Whether a drain loop is running right now.
    ///
    /// Read by [`SceneModel::deliver_records`](crate::SceneModel::deliver_records)
    /// so a committed transaction waits for the change fan-out to settle. An
    /// observer that writes the scene commits its own transaction *inside* the
    /// drain, and delivering records from there would hand the edit sink a
    /// transaction some views had not reconciled from yet — which is the one
    /// ordering this seam promises.
    pub(crate) fn is_draining(&self) -> bool {
        self.draining.get()
    }

    /// Append one notification to the back of the queue.
    fn push(&self, notification: PendingNotification) {
        self.notifications.borrow_mut().push_back(notification);
    }

    /// Deliver every queued notification, FIFO, until the queue is empty.
    ///
    /// Holds no borrow on anything across an observer call, so an observer may
    /// read the scene and write it back; a write queues at the back and is
    /// delivered by this same loop on a later round.
    ///
    /// # Rounds
    ///
    /// A round is exactly the notifications that were queued when it began.
    /// Anything an observer appends while a round runs belongs to the next one,
    /// which is what keeps delivery in emission order rather than depth-first.
    ///
    /// The round *count* is not a runaway signal and nothing here caps it: a
    /// cascade that settles link by link — the chain-drag observer this channel
    /// is advertised for — takes one round per link, so capping rounds means
    /// capping how long a legitimate chain may be.
    ///
    /// # Termination
    ///
    /// The first round is the caller's own batch: finite by construction,
    /// however large, so it is delivered free. From the second round on, every
    /// delivery is work an observer queued from inside this drain, and is
    /// counted against one flat bound — `budget.total`. Exceeding it panics,
    /// naming the notification that did it and the subject charged most often.
    /// The bound is unconditional in every build profile: unlike
    /// `Signal::try_set`, which recurses and so blows the stack loudly on its
    /// own, this is an iterative loop whose only other exit is an empty queue —
    /// unchecked it would be a silent hang.
    ///
    /// Per-subject counts are still kept, because they are what lets the panic
    /// point at a culprit — but they are **diagnostics only** and trip nothing.
    /// A per-subject *limit* is a limit on fan-in (a guarded relaxation over a
    /// hub aborted at 4200 incident edges against a flat 4096), and scaling it
    /// with the scene makes the abort cost scale with the scene too. See
    /// [`CascadeBudget`] for the measurements.
    ///
    /// # Unwind
    ///
    /// Each notification is removed from the queue immediately **before** it is
    /// delivered, and nothing is held out of the queue in a local batch, so an
    /// observer that panics costs exactly its own delivery: everything not yet
    /// delivered is still in the queue, in order, and the next
    /// [`flush_changes`](crate::SceneModel::flush_changes) delivers it. Draining
    /// into a local `Vec` instead would let the unwind discard the untouched
    /// tail with nothing left able to redeliver it — the scene would advance
    /// with no notification, permanently, which is the corruption this
    /// mechanism exists to prevent.
    pub(crate) fn drain(self: &Rc<Self>, budget: CascadeBudget) {
        if self.draining.get() {
            // Re-entered from an observer: the outermost drain owns the queue
            // and will pick up whatever that observer just emitted.
            return;
        }
        self.draining.set(true);
        // Clears the flag even if an observer panics out of the loop below.
        let _guard = DrainGuard(Rc::clone(self));

        // Observer-generated deliveries so far, and how they split across
        // subjects. The total is the trip condition; the split is the
        // diagnostic. The caller's own batch — round one — is counted in
        // neither: it is finite by construction, so charging it would make the
        // budget a cap on batch size, which is the mistake a plain delivery
        // counter makes and the reason it gets raised until it catches nothing.
        //
        // Both are bounded by `budget.total`, so this bookkeeping — and the
        // number of observer writes a runaway gets to make before it is
        // stopped — is constant in the size of the scene.
        let mut charged: HashMap<NotifyKey, u32> = HashMap::new();
        let mut charged_total: u64 = 0;
        let mut cascading = false;
        loop {
            let mut remaining = self.notifications.borrow().len();
            if remaining == 0 {
                break;
            }
            while remaining > 0 {
                // Popped before delivery, and never held in a local buffer: an
                // observer panic must cost its own delivery and nothing else.
                let next = self.notifications.borrow_mut().pop_front();
                let Some(notification) = next else {
                    break;
                };
                remaining -= 1;
                if cascading {
                    // Charged *before* delivery, so the notification named in
                    // the diagnostic is the one that broke the budget.
                    Self::charge(
                        &mut charged,
                        &mut charged_total,
                        &notification,
                        budget.total,
                    );
                }
                match notification {
                    PendingNotification::Item(change) => self.item_signal.set(change),
                    PendingNotification::A11y(_) => {
                        self.a11y_signal.set(self.a11y_signal.get().wrapping_add(1));
                    }
                }
            }
            // Everything from here on was queued by an observer this drain ran.
            cascading = true;
        }
    }

    /// Count one observer-generated delivery, and hand off to
    /// [`runaway`](Self::runaway) if that takes the drain past `limit`.
    ///
    /// Split out of [`drain`](Self::drain) so the hot path stays a counter bump
    /// and a hash-map bump, with the whole diagnostic — which walks every
    /// subject counted so far — behind a `#[cold]` call that only a trip
    /// reaches.
    fn charge(
        charged: &mut HashMap<NotifyKey, u32>,
        charged_total: &mut u64,
        notification: &PendingNotification,
        limit: u64,
    ) {
        *charged_total += 1;
        let key: NotifyKey = match notification {
            PendingNotification::Item(change) => NotifyKey::Item(change.id()),
            PendingNotification::A11y(node) => (*node).into(),
        };
        // Saturating: this count is a diagnostic, and a budget raised past
        // `u32::MAX` must not make the *counter* the thing that panics.
        let seen = charged.entry(key).or_insert(0);
        *seen = seen.saturating_add(1);
        if *charged_total > limit {
            Self::runaway(charged, *charged_total, notification, limit);
        }
    }

    /// Panic for a cascade that passed its budget, describing **what was
    /// observed** and naming the likeliest cause as a likely cause.
    ///
    /// It cannot prove one: a guarded cascade that is genuinely enormous and an
    /// unguarded write-back are the same picture from the queue's side. So the
    /// message states the bound this scene enforced, how much was delivered
    /// against it, and the subject charged most often — the number that
    /// separates the two runaway shapes from each other, since a write-back
    /// piles up on one subject while a spawner leaves every count near 1 — and
    /// then says how to raise the bound.
    #[cold]
    #[inline(never)]
    fn runaway(
        charged: &HashMap<NotifyKey, u32>,
        delivered: u64,
        notification: &PendingNotification,
        limit: u64,
    ) -> ! {
        let (top_key, top_count) = charged
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(key, count)| (*key, *count))
            .expect("every charged delivery records its subject, so a trip has at least one");
        panic!(
            "scene change fan-out did not settle: scene observers have generated \
             {delivered} notifications from one batch without the queue emptying, past \
             this scene's budget of {limit} (`CascadeBudget::total`). The last delivery \
             was {notification:?}, and the most-charged subject was {top_key:?}, with \
             {top_count} of the {delivered}. The likeliest cause is a scene observer \
             writing one subject back on every change. What to do depends on the \
             channel, because the two do not behave alike. The geometry mutators \
             already suppress a write that changes nothing (`set_local_pos` returns \
             without emitting when the position is unchanged), so a geometry trip means \
             the observer computes a *different* value each time — an accumulating \
             offset, a rounding drift, a spring that never reaches rest. Re-adding an \
             equality guard there changes nothing; make the reaction converge, or \
             record it in the observer and apply it after the mutation returns. The \
             `set_a11y_*` mutators do *not* self-suppress: they bump on every call, so \
             comparing before you call one is a real fix there. A most-charged count \
             near 1 means the other shape instead: an observer that keeps producing \
             *new* subjects, typically by adding or removing an item on every change. \
             If the cascade is genuinely this large and does settle, raise \
             `CascadeBudget::total` via `SceneModel::set_cascade_budget` — which only \
             postpones the freeze if the cycle does not terminate."
        )
    }
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
    /// Direct children, in the order they became children — the downward half
    /// of the parent pointer above, maintained by `push_entry` / `remove` /
    /// [`Scene::set_item_parent`] / [`Scene::orphan`], the only four doors that
    /// can change a parent link.
    ///
    /// Kept rather than derived because every downward walk needs it and
    /// deriving it means scanning the whole model: `rebucket_subtree` used to
    /// rebuild a scene-wide `HashMap` on **each** `set_local_pos`, which made
    /// moving one leaf cost `O(entries)` — 67 µs per call at 50 000 items,
    /// against 80 ns for the same call through `set_z`, which re-buckets
    /// nothing. See `crates/teksilo-scene/tests/mutation_scaling_probe.rs`.
    ///
    /// The invariant, asserted by `parent_and_children_agree_through_every_door`
    /// in this module's tests: `entries[p].children.contains(&c)` **iff**
    /// `entries[c].parent == Some(p)`, with no duplicates.
    pub(crate) children: Vec<ItemId>,
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

    /// Every heavyweight (`Widget`) entry, in entry order.
    ///
    /// Kept rather than filtered out of `entries` on demand because a
    /// `SceneView`'s `build()` asks for it three times — the `Once` drain, the
    /// `Delegated` payloads, and the child-ordering / orphan-reap set — and
    /// `build()` runs on **every** model mutation, so a scene of 20 000
    /// lightweight items paid three full scans to re-materialise nothing. The
    /// three are now proportional to the number of cards, which is what they
    /// were always about.
    heavyweight: Vec<ItemId>,
    /// Every entry added through [`add_item_dynamic`](Scene::add_item_dynamic),
    /// in entry order — the ones
    /// [`refresh_dynamic_bounds`](Scene::refresh_dynamic_bounds) re-reads each
    /// build. Same reason as `heavyweight`: a scene with no dynamic items used
    /// to pay a full scan and an allocation per build to discover that.
    dynamic: Vec<ItemId>,

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
    /// The deferred-notification queue plus the two signals it feeds
    /// (`item_change_signal` and `a11y_change_signal`).
    ///
    /// Behind an `Rc` so a drain can run with **no** borrow on this `Scene`,
    /// which is what lets an observer read the scene and write it back. Both
    /// notification channels share the one queue, so a mutation touching item
    /// geometry *and* logical AT structure still delivers in the order it
    /// happened rather than splitting into two independently-ordered streams.
    pending: Rc<ChangeQueue>,
    /// The transaction state machine, the edit sink, and the queue of
    /// committed [`SceneTransactionRecord`](crate::SceneTransactionRecord)s
    /// awaiting delivery.
    ///
    /// Behind an `Rc` beside this `Scene` for the same reason `pending` is: the
    /// sink is invoked with **no** borrow on the scene, which is what lets it
    /// read the scene and write it back. See [`crate::journal`].
    journal: Rc<EditJournal>,
    /// Monotonic counter of *every* model mutation — item geometry / visibility
    /// / structure (each [`ItemChange`] fire) **and** logical-AT structure (each
    /// `bump_a11y_change`). Read via [`Scene::mutation_version`], or as
    /// [`Scene::structural_version`] with the per-frame dynamic-bounds churn
    /// subtracted out — which is the form `SceneView` gates its (expensive)
    /// AccessKit re-walk on. A plain `Cell` because the bump path
    /// (`bump_mutation`) is `&self` (shared with `bump_a11y_change`).
    mutation_seq: Cell<u64>,

    /// Per-frame dynamic-bounds churn, subtracted out of
    /// [`mutation_version`](Scene::mutation_version) by
    /// [`structural_version`](Scene::structural_version).
    ///
    /// Every `LocalBoundsChanged` that [`refresh_dynamic_bounds`](Scene::refresh_dynamic_bounds)
    /// emits is counted here as well as in `mutation_seq`, so a consumer gating
    /// an expensive rebuild (the `SceneView`'s AccessKit re-walk) can exclude
    /// that churn by *name* rather than by bracketing the call with two version
    /// snapshots — which silently swallows anything an observer wrote during
    /// the refresh's fan-out.
    dynamic_seq: Cell<u64>,

    /// How many [`ItemChange`]s this scene has **emitted**, ever.
    ///
    /// Distinct from [`mutation_version`](Scene::mutation_version), which also
    /// counts logical-AT structure bumps: this one counts exactly the events
    /// that travel `item_change_signal`, so a consumer that keeps a cache
    /// invalidated *by* that signal can ask two different questions and get two
    /// honest answers — "is my cache still current?" (compare this counter with
    /// the one I last refreshed at) and "did I see every change in between?"
    /// (compare it with my own delivery count). The second is what makes the
    /// cache safe: any disagreement means an emission this consumer never saw,
    /// and the answer is a full rebuild.
    ///
    /// Bumped at **emission**, not delivery, so it is correct even while the
    /// fan-out is deferred inside an open write scope. Wraps; compare for
    /// equality, not ordering.
    item_change_seq: Cell<u64>,

    // --- deferred change fan-out -------------------------------------
    /// Number of write scopes currently open on this scene.
    ///
    /// Raised for the lifetime of the `RefMut` held by
    /// [`SceneModel`](crate::SceneModel)'s mutators and by
    /// [`SceneWriteGuard`](crate::SceneWriteGuard), so `emit_item_change`
    /// queues instead of fanning out under a live `borrow_mut()`. Zero for a
    /// bare `&mut Scene`, which keeps the historical synchronous behaviour —
    /// see the "Notification timing" section on [`Scene::item_change_signal`].
    ///
    /// A counter rather than a flag: a scope is only ever *closed* by the guard
    /// that opened it, so nesting (were a second door ever to open one inside
    /// another) must not clear the outer scope early.
    defer_depth: Cell<u32>,

    /// How much observer-generated work a single drain of this scene's queue
    /// may do before it is declared a runaway. Read once on the way into a
    /// drain and not again, so an observer cannot enlarge the drain it is
    /// already inside; a `Cell` because that read sits on the `&self` notify
    /// path.
    cascade_budget: Cell<CascadeBudget>,

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

    // --- geometry constraint -----------------------------------------
    /// The document's standing geometry rule — snap-to-grid, axis lock, page
    /// clamp — consulted by every **user-driven** gesture before anything is
    /// applied, and by no mutator at all. See [`crate::ProposedChange`].
    geometry_constraint: Option<crate::constrain::GeometryConstraint>,
    /// Whether a geometry-constraint call is running on this scene right now.
    ///
    /// Read by [`SceneModel::write_guard`](crate::SceneModel::write_guard) when
    /// its borrow fails, so a constraint that tries to *write* the scene gets a
    /// diagnostic naming the hook rather than `RefCell already borrowed`.
    ///
    /// A **flag**, not a counter, because the state it tracks is two-valued:
    /// `ConstraintCall::new` asserts that no constraint is already running (a
    /// constraint asking for another one would recurse without end, and the
    /// assertion says so), so nesting is refused rather than counted. A counter
    /// here would be a type promising a depth the code cannot reach — and
    /// `saturating_sub` would quietly absorb an unbalanced exit instead of
    /// surfacing it.
    constraint_running: Cell<bool>,
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
            heavyweight: Vec::new(),
            dynamic: Vec::new(),
            user_scene_rect: None,
            constraints: SceneConstraints::new(),
            pending: Rc::new(ChangeQueue::new()),
            journal: Rc::new(EditJournal::new()),
            mutation_seq: Cell::new(0),
            dynamic_seq: Cell::new(0),
            item_change_seq: Cell::new(0),
            defer_depth: Cell::new(0),
            cascade_budget: Cell::new(CascadeBudget::default()),
            a11y_groups: Vec::new(),
            a11y_group_index: HashMap::new(),
            a11y_parents: HashMap::new(),
            a11y_relations: Vec::new(),
            a11y_live: HashMap::new(),
            a11y_landmarks: HashMap::new(),
            a11y_categories: HashMap::new(),
            magnets: HashMap::new(),
            magnet_owner: HashMap::new(),
            geometry_constraint: None,
            constraint_running: Cell::new(false),
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
            children: Vec::new(),
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
            children: Vec::new(),
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
        let old = match &mut self.entries[pos].kind {
            SceneEntryKind::Widget(WidgetSource::Delegated { payload: slot }) => {
                std::mem::replace(slot, payload.clone())
            }
            _ => panic!("set_payload: {id:?} is not a Delegated widget entry"),
        };
        // Entry borrow dropped above; `emit_item_change` is `&self`.
        self.emit_item_change(ItemChange::PayloadChanged {
            id,
            old: ItemPayload::from(old),
            new: ItemPayload::from(payload),
        });
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
        for i in 0..self.heavyweight.len() {
            let id = self.heavyweight[i];
            let Some(&pos) = self.entry_index.get(&id) else {
                continue;
            };
            if let SceneEntryKind::Widget(WidgetSource::Once(pending)) = &mut self.entries[pos].kind
                && let Some(w) = pending.take()
            {
                out.push((id, w));
            }
        }
        out
    }

    /// `(id, payload)` for every `Delegated` heavyweight entry, in entry order.
    /// The payload `Rc` is cloned so the caller can drop the model borrow before
    /// invoking its delegate (the reentrancy contract). Called by `SceneView::build`.
    pub(crate) fn delegated_payloads(&self) -> Vec<(ItemId, Rc<dyn std::any::Any>)> {
        self.heavyweight
            .iter()
            .filter_map(|id| {
                let pos = *self.entry_index.get(id)?;
                match &self.entries[pos].kind {
                    SceneEntryKind::Widget(WidgetSource::Delegated { payload }) => {
                        Some((*id, payload.clone()))
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// Ids of every heavyweight `Widget` entry (`Once` and `Delegated`), in
    /// entry order. Used by `SceneView::build` for child ordering and the
    /// orphan-reap live-set.
    pub(crate) fn heavyweight_ids(&self) -> Vec<ItemId> {
        self.heavyweight.clone()
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
            children: Vec::new(),
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
        // Nothing declared dynamic: the common case, and it must cost nothing —
        // this runs at the top of every `build()`, and `build()` runs on every
        // model mutation.
        if self.dynamic.is_empty() {
            return false;
        }
        // Snapshot ids first to avoid borrow conflicts.
        let dynamic_ids: Vec<ItemId> = self.dynamic.clone();
        let mut changed = false;
        let seq_before = self.mutation_seq.get();
        // This is the crate's one genuine per-frame model-mutation stream: an
        // animating `add_item_dynamic` item re-reads its signal-driven AABB
        // every build and writes it back, so it emits a `LocalBoundsChanged`
        // per frame. Those changes must be *rendered* and must not be
        // *recorded*, so they are tagged ephemeral for the duration of the
        // refresh. An app driving its own per-frame stream marks it the same
        // way with `SceneTransaction::ephemeral`.
        let journal = Rc::clone(&self.journal);
        let _ephemeral = EphemeralScope::new(&journal);
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
        // Everything this pass emitted is per-frame dynamic-bounds churn, not a
        // structural change: record it so `structural_version` can exclude it
        // by name. Counting here (rather than at the call site, by bracketing
        // this call with two `mutation_version` snapshots) is what keeps a
        // mutation an *observer* makes during the fan-out out of the exclusion —
        // it happens after this line, so it lands in `mutation_seq` alone and
        // `structural_version` sees it.
        let churn = self.mutation_seq.get().wrapping_sub(seq_before);
        self.dynamic_seq
            .set(self.dynamic_seq.get().wrapping_add(churn));
        changed
    }

    fn push_entry(&mut self, entry: SceneEntry) -> ItemId {
        let id = entry.id;
        let parent = entry.parent;
        let heavyweight = matches!(entry.kind, SceneEntryKind::Widget(_));
        let dynamic = entry.dynamic_bounds;
        let pos = self.entries.len();
        self.entries.push(entry);
        self.entry_index.insert(id, pos);
        // A fresh insertion only ever appends, so both side lists stay in entry
        // order — which `heavyweight_ids` promises and `SceneView` relies on for
        // child ordering — at O(1). `push_entry_at` pays a scan to keep the same
        // promise when it inserts in the middle; this path must not.
        if heavyweight {
            self.heavyweight.push(id);
        }
        if dynamic {
            self.dynamic.push(id);
        }
        self.finish_insert(id, parent)
    }

    /// Insert an entry at a **given** position in declaration order, for
    /// [`Scene::restore`]: an item that comes back at the end of the order
    /// comes back in the wrong place in the accessibility reading order, which
    /// is the order the walk publishes siblings in.
    ///
    /// `at` is clamped to the current length, so a recorded index that no
    /// longer exists (the scene shrank while the salvage was held) appends
    /// rather than failing.
    ///
    /// Costs a reindex of the tail and a scan of each side list, which is the
    /// price of not appending. A restore is not a hot path; `push_entry` is,
    /// and keeps its O(1).
    fn push_entry_at(&mut self, entry: SceneEntry, at: usize) -> ItemId {
        let id = entry.id;
        let parent = entry.parent;
        let heavyweight = matches!(entry.kind, SceneEntryKind::Widget(_));
        let dynamic = entry.dynamic_bounds;
        let at = at.min(self.entries.len());
        self.entries.insert(at, entry);
        let reindexed: Vec<(ItemId, usize)> = self
            .entries
            .iter()
            .enumerate()
            .skip(at)
            .map(|(pos, e)| (e.id, pos))
            .collect();
        for (eid, pos) in reindexed {
            self.entry_index.insert(eid, pos);
        }
        if heavyweight {
            let slot = self
                .heavyweight
                .iter()
                .position(|hid| self.entry_index.get(hid).copied().unwrap_or(0) > at)
                .unwrap_or(self.heavyweight.len());
            self.heavyweight.insert(slot, id);
        }
        if dynamic {
            let slot = self
                .dynamic
                .iter()
                .position(|did| self.entry_index.get(did).copied().unwrap_or(0) > at)
                .unwrap_or(self.dynamic.len());
            self.dynamic.insert(slot, id);
        }
        self.finish_insert(id, parent)
    }

    /// The tail every insertion shares: adjacency, spatial index, `Added`.
    ///
    /// The `parent` ⇄ `children` adjacency is maintained here rather than
    /// assumed, so a constructor that arrives parented — which
    /// [`push_entry_at`](Self::push_entry_at) does, restoring a child — cannot
    /// silently break the invariant.
    fn finish_insert(&mut self, id: ItemId, parent: Option<ItemId>) -> ItemId {
        self.link_child(parent, id);
        let aabb = self.compute_scene_aabb(id).unwrap_or(Rect::ZERO);
        self.index.insert(id, aabb);
        self.emit_item_change(ItemChange::Added { id });
        id
    }

    /// Reactive notification stream for every Scene mutation. Apps observe via
    /// `signal.observe(|change| …)` to wire validation, persistence,
    /// telemetry, or mirroring into a data layer without polling the Scene
    /// each frame. The signal fires *after* the mutation has been applied — by
    /// the time the observer runs, the Scene already reflects the new state.
    ///
    /// # Notification timing
    ///
    /// Where the fan-out happens depends on which door mutated the scene, and
    /// the difference is exactly the `RefCell` an observer would have to
    /// re-enter:
    ///
    /// - **Through a [`SceneModel`](crate::SceneModel)** (the normal path,
    ///   including [`SceneWriteGuard`](crate::SceneWriteGuard)) the mutation
    ///   runs inside a *write scope*: changes queue in emission order and fan
    ///   out once the model has released its `borrow_mut()`. An observer may
    ///   therefore read the scene, and write it back — a write made from an
    ///   observer queues in turn and is delivered by the same drain, after the
    ///   changes already queued ahead of it.
    /// - **Through a bare `&mut Scene`** there is nothing holding a `RefCell`
    ///   open to escape from, so the fan-out is synchronous, inside the
    ///   mutator, exactly as before.
    ///
    ///   This is safe for a `Scene` you own outright — no second handle to it
    ///   can exist. It is **not** safe for a `&mut Scene` reborrowed out of a
    ///   [`SceneModel`](crate::SceneModel): a `RefMut` taken from the shared
    ///   cell leaves `defer_depth` at zero, so observers still run under the
    ///   live exclusive borrow and one that re-enters the model panics. Take
    ///   [`SceneModel::write_guard`](crate::SceneModel::write_guard) instead —
    ///   it derefs to `&mut Scene`, so the call sites are identical, and it
    ///   opens the write scope the observers need.
    ///
    /// Either way the notification has been delivered by the time the mutator
    /// call returns.
    ///
    /// # What is not deferred
    ///
    /// The scene's constraint signals — [`pan_axes_signal`](Self::pan_axes_signal),
    /// [`zoomable_signal`](Self::zoomable_signal),
    /// [`pan_bounds_signal`](Self::pan_bounds_signal),
    /// [`zoom_range_signal`](Self::zoom_range_signal) — carry *state*, not
    /// events ([`current_pan_axes`](Self::current_pan_axes) and friends read
    /// them back), so queueing their writes would make the scene contradict
    /// itself inside an open write scope. They still fan out synchronously
    /// under the borrow: an observer on one of those four must not re-enter
    /// the `SceneModel`.
    pub fn item_change_signal(&self) -> Signal<SceneChange> {
        self.pending.item_signal()
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
        self.pending.a11y_signal()
    }

    /// Bump the logical-AT-structure change counter. Called at the end of every
    /// a11y-structure mutator so observers re-walk AccessKit. Also advances the
    /// unified [`mutation_version`](Self::mutation_version) so a logical-AT
    /// mutation (which never fires `item_change_signal`) still un-gates the
    /// SceneView's AT re-walk.
    ///
    /// While a **write scope** is open the bump is queued and fans out once the
    /// scope's owner has released its `RefCell<Scene>` borrow, exactly as
    /// `emit_item_change` does — the two channels share one FIFO so their
    /// relative order survives.
    ///
    /// Note the bump/notify order: `bump_mutation` runs **first**, so an
    /// observer always sees a [`mutation_version`](Self::mutation_version)
    /// that already accounts for the change it is being told about. That
    /// matches `emit_item_change`; the two channels used to disagree.
    ///
    /// `subject` is the node the mutation was about. It never reaches an
    /// observer — the signal is a bare counter — but the runaway detector
    /// charges the delivery to it, so every a11y mutator has to name one. That
    /// is what keeps an app maintaining AT structure per item off a single
    /// scene-wide budget; see [`CascadeBudget`].
    fn bump_a11y_change(&self, subject: A11yNode) {
        self.bump_mutation();
        self.notify_or_queue(PendingNotification::A11y(subject));
    }

    /// Fire an [`ItemChange`] through `item_change_signal` and advance the
    /// unified [`mutation_version`](Self::mutation_version). The single choke
    /// point every geometry / visibility / structure mutation routes through, so
    /// the version counts each one without per-site bookkeeping.
    ///
    /// While a **write scope** is open the change is appended to
    /// `pending_notifications` instead, and fans out once the scope's owner has
    /// released its `RefCell<Scene>` borrow — see
    /// [`SceneModel::flush_changes`](crate::SceneModel::flush_changes).
    /// `mutation_seq` advances either way: the version is the scene's own
    /// state, not a notification, and the scene is exclusively borrowed for the
    /// whole window in which the two are transiently out of step.
    fn emit_item_change(&self, change: ItemChange) {
        self.bump_mutation();
        self.item_change_seq
            .set(self.item_change_seq.get().wrapping_add(1));
        let (txn, source, history, ephemeral) = self.journal.stamp();
        // Two kinds of change stay out of the record.
        //
        // A removal's edit carries ownership the notification cannot, so the
        // removal path records its own `SceneEdit::Removed` with the salvage
        // attached. Recording it again here would double it.
        //
        // A *derived notification* — `VisibilityChanged` beside the
        // `FlagsChanged` that describes the same mutation, or the
        // `HandlersChanged` that `handlers_mut` fires without knowing what the
        // handlers became — is not an edit at all. Recording the first would
        // make one mutation two edits; recording the second would put an entry
        // nothing can invert into a record whose whole claim is that every
        // entry carries both sides.
        if change.is_edit() && !matches!(change, ItemChange::Removed { .. }) {
            self.journal.record(SceneEdit::Change(change.clone()));
        }
        self.notify_or_queue(PendingNotification::Item(SceneChange {
            txn,
            source,
            history,
            ephemeral,
            change,
        }));
    }

    /// The edit journal, as a handle that outlives any borrow on this scene —
    /// the same shape as [`change_queue`](Self::change_queue), and for the same
    /// reason: a record is handed to the sink with the scene free.
    pub(crate) fn journal(&self) -> Rc<EditJournal> {
        Rc::clone(&self.journal)
    }

    /// How many [`ItemChange`]s this scene has emitted, ever — the counter a
    /// consumer caching per-item data validates against. See the field's own
    /// documentation for why it is not [`mutation_version`](Self::mutation_version).
    pub(crate) fn item_change_version(&self) -> u64 {
        self.item_change_seq.get()
    }

    /// Queue one notification, then deliver the queue if no write scope is
    /// open.
    ///
    /// The queue is appended to *unconditionally*, even at depth zero. That is
    /// deliberate: anything already queued was emitted **earlier**, and firing
    /// this change past it would deliver the scene's own history out of order —
    /// an item would appear to move backwards once the older change finally
    /// landed. FIFO is the one ordering guarantee this channel makes, and it
    /// has to hold across the seam between the deferred and the synchronous
    /// door too. (Reachable only after an observer panicked mid-drain and left
    /// a partly-delivered queue behind, which is precisely when an app is least
    /// able to reason about ordering for itself.)
    fn notify_or_queue(&self, notification: PendingNotification) {
        self.pending.push(notification);
        if self.defer_depth.get() > 0 {
            // A write scope owns the fan-out: it drains once its borrow drops.
            return;
        }
        self.pending.drain(self.cascade_budget.get());
    }

    /// The runaway-detection budget for this scene's change fan-out.
    pub fn cascade_budget(&self) -> CascadeBudget {
        self.cascade_budget.get()
    }

    /// Replace the runaway-detection budget for this scene's change fan-out.
    ///
    /// The default is tuned for the graph shapes this tier runs; see
    /// [`CascadeBudget`] for the rule and for when raising it is the right
    /// answer rather than a way to silence a real cycle.
    ///
    /// A budget of zero is clamped to the smallest usable one rather than
    /// stored: it would forbid the reactive write-back this channel exists for,
    /// and would trip with no cascade for the diagnostic to describe.
    ///
    /// Takes effect on the next drain — a budget raised from inside an observer
    /// does not enlarge the drain already running, which is why the budget is
    /// read once at its start.
    pub fn set_cascade_budget(&self, budget: CascadeBudget) {
        self.cascade_budget.set(CascadeBudget::new(budget.total));
    }

    // -----------------------------------------------------------------
    // Write scope / deferred fan-out (drained by `SceneModel`)
    // -----------------------------------------------------------------

    /// Open a write scope: every later notification queues rather than fanning
    /// out, until the matching [`exit_write_scope`](Self::exit_write_scope).
    ///
    /// Called by the owner of the `RefCell<Scene>` borrow, never by a `Scene`
    /// method — the point of the scope is to outlive the borrow's *creator*,
    /// which a `&mut self` method cannot do for its own `&mut self`.
    pub(crate) fn enter_write_scope(&self) {
        self.defer_depth.set(self.defer_depth.get() + 1);
        // A write scope is also the **implicit transaction boundary**: one
        // scope, one `TxnId`. That is what makes a subtree `remove` one
        // transaction with N edits, and a `SceneWriteGuard` block one
        // transaction, without a single mutator being wrapped by hand. An
        // explicit `SceneTransaction` has already opened a scope, so this one
        // joins it and its default stamp is ignored.
        self.journal
            .enter_scope(ChangeSource::default(), HistoryMode::default());
    }

    /// Close a write scope opened by [`enter_write_scope`](Self::enter_write_scope).
    pub(crate) fn exit_write_scope(&self) {
        self.defer_depth
            .set(self.defer_depth.get().saturating_sub(1));
        self.journal.exit_scope();
    }

    /// The notification queue, as a handle that outlives any borrow on this
    /// scene. [`SceneModel::flush_changes`](crate::SceneModel::flush_changes)
    /// takes one, releases its borrow, and only then drains — so observers run
    /// with the scene free.
    pub(crate) fn change_queue(&self) -> Rc<ChangeQueue> {
        Rc::clone(&self.pending)
    }

    /// Whether anything is waiting to be delivered. Cheaper than
    /// [`change_queue`](Self::change_queue) for the "is there anything to do?"
    /// question a flush asks on every frame.
    pub(crate) fn has_pending_notifications(&self) -> bool {
        self.pending.has_work()
    }

    /// How many transaction / write scopes are open on this scene.
    ///
    /// Zero between mutations. A non-zero value outside a mutator means a
    /// [`SceneTransaction`](crate::SceneTransaction) is being held, which is
    /// only correct within one synchronous scope — see
    /// [`SceneModel::transaction`](crate::SceneModel::transaction).
    pub fn open_transaction_depth(&self) -> u32 {
        self.journal.depth()
    }

    /// Whether any committed transaction is waiting for the edit sink. Same
    /// shape and purpose as [`has_pending_notifications`](Self::has_pending_notifications).
    pub(crate) fn has_pending_records(&self) -> bool {
        self.journal.has_records()
    }

    /// Whether the change fan-out is mid-drain — see
    /// [`ChangeQueue::is_draining`].
    pub(crate) fn changes_are_draining(&self) -> bool {
        self.pending.is_draining()
    }

    /// Fires once per committed transaction, after the edit sink, with the
    /// scene unborrowed. See
    /// [`SceneModel::transaction_signal`](crate::SceneModel::transaction_signal).
    pub fn transaction_signal(&self) -> Signal<crate::journal::TxnId> {
        self.journal.txn_signal()
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
    /// Includes the per-frame churn of
    /// [`refresh_dynamic_bounds`](Self::refresh_dynamic_bounds); a consumer
    /// gating an expensive rebuild on "did anything *meaningful* change" wants
    /// [`structural_version`](Self::structural_version) instead. The counter
    /// wraps; compare for equality, not ordering.
    pub fn mutation_version(&self) -> u64 {
        self.mutation_seq.get()
    }

    /// [`mutation_version`](Self::mutation_version) with the per-frame
    /// dynamic-bounds churn subtracted out: it advances on every mutation
    /// **except** the `LocalBoundsChanged` events
    /// [`refresh_dynamic_bounds`](Self::refresh_dynamic_bounds) emits.
    ///
    /// The version to gate an expensive rebuild on.
    /// [`SceneView`](crate::SceneView) snapshots it each `build()` and only
    /// re-walks the (separate, expensive) AccessKit tree when it has advanced
    /// since the previous walk — so an actively-animating
    /// [`add_item_dynamic`](Self::add_item_dynamic) item, which rebuilds every
    /// frame, does not issue an AT re-walk per frame for sub-pixel bounds drift
    /// a screen reader cannot use.
    ///
    /// Naming the exclusion is the point. The alternative — snapshotting
    /// `mutation_version` before `refresh_dynamic_bounds` and again after, and
    /// treating the difference as churn — is wrong now that the refresh fans
    /// its changes out: an observer may legally mutate the scene from that
    /// fan-out, and its mutation lands inside the bracket where it is
    /// indistinguishable from churn. Folded into the baseline, an AT-structural
    /// change made there would never un-gate a re-walk, in that build or any
    /// later one. The counter wraps; compare for equality, not ordering.
    pub fn structural_version(&self) -> u64 {
        self.mutation_seq.get().wrapping_sub(self.dynamic_seq.get())
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
    ///
    /// # The item has the last word, and says it once
    ///
    /// An item whose box is **derived** from its own geometry — a
    /// [`PathItem`](crate::PathItem) — treats this as *"fit yourself to this
    /// rectangle"* rather than *"adopt this rectangle"*, and the box stored
    /// here is what it settled on, read back from the item. It lands on the
    /// request on every axis the geometry has extent on; on an axis it has
    /// none (a perfectly horizontal stroke has no height) the box stays the
    /// stroke's own thickness, because there is nothing to stretch.
    ///
    /// Either way the call is **idempotent**: asking twice for the same
    /// rectangle emits one [`ItemChange::LocalBoundsChanged`] and re-buckets
    /// the index once. An app driving this per frame — a resize handle, a
    /// layout pass — therefore goes quiet as soon as it stops moving, rather
    /// than emitting an endless series of nearly-identical changes.
    pub fn set_local_bounds(&mut self, id: ItemId, local_bounds: Rect) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].local_bounds;
            if old == local_bounds {
                return;
            }
            // The entry mirrors whatever the item settled on, not what the
            // caller asked for. A geometry-bearing item may honour the request
            // only up to its own extent (a `PathItem` fits its path to the box
            // and then re-derives it from the moved geometry, band included),
            // and the entry's rectangle is what the spatial index buckets on —
            // so reading it back here is what makes "the item's box" and "the
            // index's box" one value rather than two that can drift.
            let effective = match &mut self.entries[pos].kind {
                SceneEntryKind::Item(item) => {
                    item.set_local_bounds(local_bounds);
                    item.local_bounds()
                }
                SceneEntryKind::Widget(_) => local_bounds,
            };
            self.entries[pos].local_bounds = effective;
            if old == effective {
                return;
            }
            // Bounds are local — only this entry's scene-AABB shifts;
            // descendants' local frames are unchanged.
            let aabb = self.compute_scene_aabb(id).unwrap_or(Rect::ZERO);
            self.index.insert(id, aabb);
            self.emit_item_change(ItemChange::LocalBoundsChanged {
                id,
                old,
                new: effective,
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
            let old = std::mem::replace(&mut self.entries[pos].transform, transform);
            if old == transform {
                // An app driving a rotation from a `Signal<f32>` writes this
                // every frame; without the guard that is a per-frame change
                // stream for a scene that is not moving, and — once the change
                // is recorded — a per-frame transaction too.
                return;
            }
            self.rebucket_subtree(id);
            self.emit_item_change(ItemChange::TransformChanged {
                id,
                old,
                new: transform,
            });
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

    /// Record `child` under `parent`'s `children` list. `None` parent is the
    /// scene root, which keeps no list (nothing walks down from it).
    fn link_child(&mut self, parent: Option<ItemId>, child: ItemId) {
        let Some(parent) = parent else { return };
        let Some(&pos) = self.entry_index.get(&parent) else {
            return;
        };
        let kids = &mut self.entries[pos].children;
        if !kids.contains(&child) {
            kids.push(child);
        }
    }

    /// Drop `child` from `parent`'s `children` list. The inverse of
    /// [`link_child`](Self::link_child); every parent-pointer write pairs the
    /// two so the adjacency never drifts from the pointers.
    fn unlink_child(&mut self, parent: Option<ItemId>, child: ItemId) {
        let Some(parent) = parent else { return };
        let Some(&pos) = self.entry_index.get(&parent) else {
            return;
        };
        self.entries[pos].children.retain(|c| *c != child);
    }

    /// Push `id`'s direct children onto `out`, in the order they became
    /// children. The one downward step every subtree walk in this file takes.
    fn push_children(&self, id: ItemId, out: &mut Vec<ItemId>) {
        if let Some(&pos) = self.entry_index.get(&id) {
            out.extend_from_slice(&self.entries[pos].children);
        }
    }

    fn rebucket_subtree(&mut self, root: ItemId) {
        // Re-bucket `root` and every descendant whose scene-AABB depends on the
        // root's frame. The walk reads the kept `SceneEntry::children`
        // adjacency, so it costs the moved subtree — not the model. (It used to
        // rebuild a scene-wide parent→children `HashMap` on every call, which
        // put `O(entries)` on each pointer sample of a drag.)
        //
        // Cycle guard: the parent-pointer walkers (`scene_transform` etc.)
        // bound their *upward* walk with a hop cap; this *downward* walk can
        // loop forever if the parent graph ever contains a cycle (e.g. from a
        // future de-serialization bug), so we track visited nodes. A
        // well-formed tree never revisits a node, so this is also a redundant-
        // work guard. It is sized to the subtree, not the scene: a scene of
        // 50 000 items moving one leaf allocates a one-element set.
        let mut visited: HashSet<ItemId> = HashSet::new();
        let mut stack: Vec<ItemId> = vec![root];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(aabb) = self.compute_scene_aabb(id) {
                self.index.insert(id, aabb);
            }
            self.push_children(id, &mut stack);
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
    ///
    /// Announces exactly what [`set_flag`](Self::set_flag) announces for the
    /// same net change: the derived [`ItemChange::VisibilityChanged`] when
    /// `IS_VISIBLE` flipped, then the [`ItemChange::FlagsChanged`] that
    /// describes the mutation.
    pub fn set_flags(&mut self, id: ItemId, flags: ItemFlags) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].flags;
            if old == flags {
                return;
            }
            self.entries[pos].flags = flags;
            self.announce_flags(id, old, flags);
        }
    }

    /// Set or clear a single flag on an item. No-op if unknown.
    pub fn set_flag(&mut self, id: ItemId, flag: ItemFlags, on: bool) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].flags;
            self.entries[pos].flags.set(flag, on);
            let new = self.entries[pos].flags;
            if old != new {
                self.announce_flags(id, old, new);
            }
        }
    }

    /// The one announcement both flag doors make, so a hidden card is the same
    /// pair of notifications and the same *single* recorded edit whichever door
    /// hid it.
    ///
    /// [`ItemChange::VisibilityChanged`] first when `IS_VISIBLE` flipped — a
    /// derived convenience that stays out of the transaction record
    /// ([`ItemChange::is_edit`]) — then the [`ItemChange::FlagsChanged`] that
    /// describes the mutation. `set_flag` used to emit the pair and `set_flags`
    /// only the second, so one mutation had two record shapes depending on the
    /// door, and an app counting the edits in a transaction reported 2 for
    /// hiding a card and 1 for the same hide through `set_flags`.
    fn announce_flags(&mut self, id: ItemId, old: ItemFlags, new: ItemFlags) {
        if old.contains(ItemFlags::IS_VISIBLE) != new.contains(ItemFlags::IS_VISIBLE) {
            self.emit_item_change(ItemChange::VisibilityChanged {
                id,
                visible: new.contains(ItemFlags::IS_VISIBLE),
            });
        }
        self.emit_item_change(ItemChange::FlagsChanged { id, old, new });
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
        let write = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_fill(Some(prop.clone())),
            _ => AppearanceWrite::Refused,
        };
        if let Some(old) = write.into_previous() {
            self.emit_item_change(ItemChange::AppearanceChanged {
                id,
                change: AppearanceChange::Fill {
                    old,
                    new: Some(prop),
                },
            });
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
        let write = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_fill(None),
            _ => AppearanceWrite::Refused,
        };
        if let Some(old) = write.into_previous() {
            self.emit_item_change(ItemChange::AppearanceChanged {
                id,
                change: AppearanceChange::Fill { old, new: None },
            });
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
        let write = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_stroke(Some((prop.clone(), style.clone()))),
            _ => AppearanceWrite::Refused,
        };
        if let Some(old) = write.into_previous() {
            self.emit_item_change(ItemChange::AppearanceChanged {
                id,
                change: AppearanceChange::Stroke {
                    old,
                    new: Some((prop, style)),
                },
            });
        }
    }

    /// Clear a lightweight item's stroke, emitting
    /// [`ItemChange::AppearanceChanged`] (repaint-only). No-op for item kinds
    /// without a stroke.
    pub fn clear_item_stroke(&mut self, id: ItemId) {
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        let write = match &mut self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.set_stroke(None),
            _ => AppearanceWrite::Refused,
        };
        if let Some(old) = write.into_previous() {
            self.emit_item_change(ItemChange::AppearanceChanged {
                id,
                change: AppearanceChange::Stroke { old, new: None },
            });
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
    ///
    /// Fires [`ItemChange::HandlersChanged`] carrying **both sides**
    /// ([`HandlerReplacement`]), like every other mutator on this type: a
    /// consumer that caches handlers has no other way to learn of it, and a
    /// data layer inverting a transaction has no other way to put them back.
    /// This is the handler door that produces a reversible edit; see
    /// [`handlers_mut`](Self::handlers_mut) for the one that cannot.
    pub fn set_item_handlers(&mut self, id: ItemId, handlers: Option<SceneItemHandlerSet>) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].handlers.as_deref().cloned();
            self.entries[pos].handlers = handlers.clone().map(Box::new);
            self.emit_item_change(ItemChange::HandlersChanged {
                id,
                replaced: Some(Box::new(HandlerReplacement { old, new: handlers })),
            });
        }
    }

    /// Mutably borrow an item's handler set, lazily creating an
    /// empty one if none exists. Returns `None` for unknown ids.
    /// Allows fluent chains: `scene.handlers_mut(id).unwrap().on_tap(…).cursor(…);`.
    ///
    /// Fires [`ItemChange::HandlersChanged`] *before* handing the set out, with
    /// no [`HandlerReplacement`] — `&mut` cannot report back what the caller
    /// does with it, so the notification means "no longer what you last read"
    /// and nothing finer. A caller that takes the borrow and changes nothing
    /// therefore costs one spurious invalidation, which is the right way round:
    /// the alternative is a consumer serving stale handlers.
    ///
    /// Because it describes nothing, it is **not** recorded as an edit
    /// ([`ItemChange::is_edit`]) — a transaction record whose every entry
    /// carries both sides of what it replaced must not carry one that carries
    /// neither. Make a handler change the history should be able to reverse
    /// through [`set_item_handlers`](Self::set_item_handlers), which knows both
    /// sides.
    pub fn handlers_mut(&mut self, id: ItemId) -> Option<&mut SceneItemHandlerSet> {
        let pos = *self.entry_index.get(&id)?;
        self.emit_item_change(ItemChange::HandlersChanged { id, replaced: None });
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
    ///
    /// # The no-op test is exact
    ///
    /// A write is ignored only when the entry already holds **that** value.
    /// The guard used to be an absolute `|old - z| < f32::EPSILON`, which is
    /// the wrong metric in both directions: `f32::EPSILON` is the spacing of
    /// the representable numbers at 1.0, so out at `z = 1e6` (where the real
    /// spacing is ~0.06) no two distinct floats are ever within it and the
    /// guard never fires, while near zero (where the spacing is ~1e-45) it
    /// swallows millions of distinct values.
    ///
    /// That second half was not theoretical: [`z_between`](Self::z_between)
    /// bisects **relatively** and existed precisely so a caller is never told
    /// "there is room" and then gets a silent no-op — and near zero the two
    /// disagreed, so `z_between` returned `Some(6.25e-8)` and `set_z` dropped
    /// it, emitting no [`ItemChange::ZChanged`] and leaving the order wrong
    /// with nothing to observe. One metric now, and it is the one `z_between`
    /// already used: two `z`s are the same iff they are the same float.
    pub fn set_z(&mut self, id: ItemId, z: f32) {
        if let Some(&pos) = self.entry_index.get(&id) {
            let old = self.entries[pos].z;
            if old == z {
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
            self.unlink_child(old, child);
            self.link_child(parent, child);
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

    /// Append every direct + transitive descendant of `id` into `out`, each
    /// parent before its own children. The id itself is **not** included.
    ///
    /// Costs the subtree, not the scene: the walk steps through the kept
    /// `SceneEntry::children` adjacency rather than rescanning every entry per
    /// visited node. A cycle in the parent graph is bounded by the visited set
    /// rather than looping forever.
    pub fn collect_descendants(&self, id: ItemId, out: &mut Vec<ItemId>) {
        let mut visited: HashSet<ItemId> = HashSet::new();
        visited.insert(id);
        let mut frontier: Vec<ItemId> = vec![id];
        let mut kids: Vec<ItemId> = Vec::new();
        while let Some(parent) = frontier.pop() {
            kids.clear();
            self.push_children(parent, &mut kids);
            for child in kids.iter().copied() {
                if !visited.insert(child) {
                    continue;
                }
                out.push(child);
                frontier.push(child);
            }
        }
    }

    // -----------------------------------------------------------------
    // Geometry constraint
    // -----------------------------------------------------------------

    /// Install the document's standing geometry rule — snap-to-grid, axis lock,
    /// page-bounds clamp — consulted before a **user-driven** gesture applies
    /// anything.
    ///
    /// Replaces any previous constraint; there is one per scene, because a
    /// document has one geometry. See [`ProposedChange`](crate::ProposedChange)
    /// for what the closure is handed, and
    /// [`SceneModel::set_geometry_constraint`](crate::SceneModel::set_geometry_constraint)
    /// for the door an app normally uses.
    ///
    /// ```
    /// use teksilo_canvas::{Point, Rect};
    /// use teksilo_scene::{ChangeVerdict, RectItem, Scene};
    ///
    /// let mut scene = Scene::new();
    /// scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)), Point::ZERO);
    /// // A 25-unit grid, snapping the moved box's own top-leading corner.
    /// scene.set_geometry_constraint(|c| {
    ///     let mut f = c.proposed;
    ///     f.rect.x = (f.rect.x / 25.0).round() * 25.0;
    ///     f.rect.y = (f.rect.y / 25.0).round() * 25.0;
    ///     ChangeVerdict::Adjust(f)
    /// });
    /// assert!(scene.has_geometry_constraint());
    /// ```
    pub fn set_geometry_constraint(
        &mut self,
        f: impl Fn(&crate::constrain::ProposedChange<'_>) -> crate::constrain::ChangeVerdict + 'static,
    ) {
        self.geometry_constraint = Some(Rc::new(f));
    }

    /// Remove the geometry constraint. Gestures then apply their raw proposal.
    pub fn clear_geometry_constraint(&mut self) {
        self.geometry_constraint = None;
    }

    /// Whether a geometry constraint is installed.
    pub fn has_geometry_constraint(&self) -> bool {
        self.geometry_constraint.is_some()
    }

    /// Whether a geometry constraint is running **right now** on this thread.
    ///
    /// The probe [`SceneModel::write_guard`](crate::SceneModel::write_guard)
    /// uses to turn "a constraint tried to write the scene" into a diagnostic
    /// that names the hook.
    pub fn in_geometry_constraint(&self) -> bool {
        self.constraint_running.get()
    }

    /// The installed constraint, cloned. `None` when there is none — which is
    /// the whole cost an unconstrained scene pays per gesture sample.
    pub(crate) fn geometry_constraint(&self) -> Option<crate::constrain::GeometryConstraint> {
        self.geometry_constraint.clone()
    }

    /// The running flag, for `ConstraintGuard`.
    pub(crate) fn constraint_running_cell(&self) -> &Cell<bool> {
        &self.constraint_running
    }

    // -----------------------------------------------------------------
    // Selection transforms
    // -----------------------------------------------------------------

    /// `ids` pruned to its **roots**: any item whose ancestor is also in `ids`
    /// is dropped.
    ///
    /// Moving an ancestor already moves its descendants — their `local_pos` is
    /// parent-relative and is not touched — so transforming both would apply
    /// the change twice. This is the mutation-side twin of the filter the paint
    /// preview already runs over the same set.
    ///
    /// Order is preserved. **O(n·depth)**: the set is hashed once and each id
    /// then walks its own ancestor chain, asking the set rather than asking
    /// every other id whether it is an ancestor. The hop cap is the one
    /// [`is_descendant_of`](Self::is_descendant_of) uses, so a malformed parent
    /// cycle bounds rather than hangs, and an id that is its own ancestor is
    /// kept (a cycle has no root to prefer).
    ///
    /// The complexity is load-bearing, not incidental: every selection-transform
    /// recompute runs this once (plus one
    /// [`transformable_roots`](Self::transformable_roots) per operation), and so
    /// does the item-drag group and the keyboard nudge — so a quadratic form
    /// here is a frozen window on a large selection rather than a slow one.
    /// `selection_roots_is_linear_in_the_selection`, in
    /// `tests/selection_roots_scaling_probe.rs`, pins it: 7.03 ms against
    /// 36.3 µs for a 1 000-item selection, measured.
    pub fn selection_roots(&self, ids: &[ItemId]) -> Vec<ItemId> {
        // One id cannot be pruned by itself, so the whole question is moot
        // below two — and a selection of one is the common case.
        if ids.len() < 2 {
            return ids.to_vec();
        }
        let set: HashSet<ItemId> = ids.iter().copied().collect();
        let cap = self.entries.len();
        ids.iter()
            .copied()
            .filter(|id| {
                let mut cur = self.parent_of(*id);
                let mut hops = 0usize;
                while let Some(p) = cur {
                    if p == *id {
                        // Its own ancestor: a cycle, not a nesting. Matches
                        // the old form, which never compared an id to itself.
                        break;
                    }
                    if set.contains(&p) {
                        return false;
                    }
                    cur = self.parent_of(p);
                    hops += 1;
                    if hops > cap {
                        break;
                    }
                }
                true
            })
            .collect()
    }

    /// The roots of `ids` that may take part in `op`.
    ///
    /// Two filters, in this order: the item must carry the operation's flag
    /// ([`TransformOp::required_flag`](crate::TransformOp::required_flag)), and
    /// — for [`TransformOp::Rotate`](crate::TransformOp::Rotate) — it must be a
    /// lightweight entry, because a heavyweight card is sized from the AABB of
    /// its transformed bounds and a rotation there inflates its layout box
    /// without turning anything.
    ///
    /// The flag is checked **before** the descendant pruning, so a selected
    /// child of a selected-but-locked parent still takes part on its own.
    pub fn transformable_roots(
        &self,
        ids: &[ItemId],
        op: crate::transform_session::TransformOp,
    ) -> Vec<ItemId> {
        let flag = op.required_flag();
        let eligible: Vec<ItemId> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.flags(*id).is_some_and(|f| f.contains(flag))
                    && (op != crate::transform_session::TransformOp::Rotate
                        || self.item(*id).is_some())
            })
            .collect();
        self.selection_roots(&eligible)
    }

    /// The item's own rotation in scene space, in radians — the angle of its
    /// composed `local → scene` basis. `None` for an unknown id.
    pub fn scene_rotation(&self, id: ItemId) -> Option<f32> {
        if !self.entry_index.contains_key(&id) {
            return None;
        }
        let m = self.scene_transform(id).m;
        Some(m[1].atan2(m[0]))
    }

    /// The selection frame enclosing `roots` — the box a transform controller
    /// draws its handles on. `None` when `roots` is empty or none of them
    /// resolve.
    ///
    /// A **single** root's frame takes that item's own rotation, so resizing a
    /// rotated item happens along its own axes and is exact. A multi-item
    /// frame is axis-aligned, because the union of differently-rotated boxes
    /// has no well-defined angle — Konva does the same.
    pub fn transform_frame(
        &self,
        roots: &[ItemId],
    ) -> Option<crate::transform_session::TransformFrame> {
        use crate::transform_session::TransformFrame;
        let rotation = if roots.len() == 1 {
            self.scene_rotation(roots[0]).unwrap_or(0.0)
        } else {
            0.0
        };
        let to_frame = Transform2D::rotate(-rotation);
        let mut acc: Option<(f32, f32, f32, f32)> = None;
        let mut count = 0usize;
        for id in roots.iter().copied() {
            let Some(local) = self.local_bounds(id) else {
                continue;
            };
            let to_scene = self.scene_transform(id);
            count += 1;
            for corner in [
                Point::new(local.x, local.y),
                Point::new(local.right(), local.y),
                Point::new(local.right(), local.bottom()),
                Point::new(local.x, local.bottom()),
            ] {
                let p = to_frame.apply_point(to_scene.apply_point(corner));
                acc = Some(match acc {
                    None => (p.x, p.y, p.x, p.y),
                    Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
                });
            }
        }
        let (x0, y0, x1, y1) = acc?;
        Some(TransformFrame {
            rect: Rect::new(x0, y0, x1 - x0, y1 - y0),
            rotation,
            count,
        })
    }

    /// Apply `delta` to `roots` as a **single** operation, and the only place a
    /// transform controller writes the model.
    ///
    /// Returns how many entry fields actually changed.
    ///
    /// Three writes per root at most, and each is the existing setter, so the
    /// change stream, the spatial index and every observer see nothing new:
    ///
    /// * **position** — the item's scene anchor is mapped through the delta and
    ///   restated in its parent's frame, so a scale about a pivot moves it and a
    ///   rotation orbits it.
    /// * **extent** — `local_bounds` is scaled, which makes the item *reflow*
    ///   into the new box rather than be drawn at a stretched scale. The scale
    ///   is resolved along the item's **own** axes, so a rotated item stays a
    ///   rotated rectangle instead of shearing into a parallelogram; where the
    ///   scale is non-uniform and the item is rotated relative to the frame,
    ///   that is an approximation of the true (unrepresentable) result, and it
    ///   is exact whenever the item is aligned with the frame — which includes
    ///   every single-item selection.
    /// * **orientation** — the item's own `Transform2D` is post-rotated. Skipped
    ///   for a heavyweight entry, whose layout box is the AABB of its
    ///   transformed bounds: a rotation there would inflate the box and turn
    ///   nothing.
    ///
    /// The transaction boundary is this call. One gesture is one call, so an
    /// app-level reversible-edit layer has exactly one thing to record — and
    /// this crate ships no history of its own.
    pub fn apply_transform_delta(
        &mut self,
        roots: &[ItemId],
        delta: &crate::transform_session::TransformDelta,
    ) -> usize {
        if delta.is_identity() {
            return 0;
        }
        let scene_delta = delta.to_scene_transform();
        let rotating = delta.rotation.abs() > 1e-6;
        let scaling = (delta.scale.x - 1.0).abs() > 1e-6 || (delta.scale.y - 1.0).abs() > 1e-6;
        let mut changed = 0usize;
        for id in roots.iter().copied() {
            let Some(&pos) = self.entry_index.get(&id) else {
                continue;
            };
            let is_lightweight = matches!(self.entries[pos].kind, SceneEntryKind::Item(_));
            let parent = self.entries[pos].parent;
            let parent_xform = match parent {
                Some(p) => self.scene_transform(p),
                None => Transform2D::identity(),
            };
            let Some(parent_inv) = parent_xform.inverse() else {
                continue;
            };
            let old_anchor = self.scene_transform(id).apply_point(Point::ZERO);
            let new_anchor = scene_delta.apply_point(old_anchor);

            // Orientation first: the item's own transform decides where its
            // local origin sits inside its `local → parent` map, and the new
            // position is derived against the *new* one.
            let old_transform = self.entries[pos].transform;
            let new_transform = if rotating && is_lightweight {
                old_transform.then(&Transform2D::rotate(delta.rotation))
            } else {
                old_transform
            };
            let rotated = new_transform != old_transform;
            if rotated {
                self.entries[pos].transform = new_transform;
                changed += 1;
                // Announced here, not only on the no-move path below: a
                // rotation that also moved the item used to emit nothing but
                // `LocalPosChanged`, so a consumer reconstructing the edit from
                // the change stream lost the rotation entirely.
                self.rebucket_subtree(id);
                self.emit_item_change(ItemChange::TransformChanged {
                    id,
                    old: old_transform,
                    new: new_transform,
                });
            }
            let origin_offset = new_transform.apply_point(Point::ZERO);
            let target_local = parent_inv.apply_point(new_anchor);
            let new_local_pos = Point::new(
                target_local.x - origin_offset.x,
                target_local.y - origin_offset.y,
            );

            if scaling {
                // The scale arrives in the frame's basis; resolve it onto the
                // item's own axes. `|cos| · sx + |sin| · sy` is exact at 0° and
                // at 90° (where the axes simply swap) and interpolates
                // continuously between them.
                let phi = self.scene_rotation(id).unwrap_or(0.0) - delta.basis;
                let (sin, cos) = phi.sin_cos();
                let (ac, as_) = (cos.abs(), sin.abs());
                let sx = ac * delta.scale.x + as_ * delta.scale.y;
                let sy = as_ * delta.scale.x + ac * delta.scale.y;
                let b = self.entries[pos].local_bounds;
                let scaled = Rect::new(b.x * sx, b.y * sy, b.width * sx, b.height * sy);
                if scaled != b {
                    self.set_local_bounds(id, scaled);
                    changed += 1;
                }
            }

            let old_pos = self.entries[pos].local_pos;
            if new_local_pos != old_pos {
                self.set_local_pos(id, new_local_pos);
                changed += 1;
            }
        }
        changed
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

    /// Where `id` sits in this scene's single paint order — the value every
    /// picker in the crate compares. `None` for unknown ids.
    ///
    /// Defined for **both tiers**: a lightweight entry's rank is its
    /// [`SceneLayer`] band ([`RANK_UNDER`](crate::pick::RANK_UNDER) /
    /// [`RANK_OVER`](crate::pick::RANK_OVER)), a heavyweight widget entry's is
    /// [`RANK_WIDGET`](crate::pick::RANK_WIDGET) — which is exactly where the
    /// arena's child walk paints it, between the two lightweight bands. See
    /// [`PaintKey`] for the ordering and for the equal-`z` tie-break.
    pub fn paint_key(&self, id: ItemId) -> Option<PaintKey> {
        let pos = *self.entry_index.get(&id)?;
        let entry = &self.entries[pos];
        let rank = match entry.kind {
            SceneEntryKind::Widget(_) => crate::pick::RANK_WIDGET,
            SceneEntryKind::Item(_) => match entry.layer {
                SceneLayer::Under => crate::pick::RANK_UNDER,
                SceneLayer::Over => crate::pick::RANK_OVER,
            },
        };
        Some(PaintKey::new(rank, entry.z, id.as_u64()))
    }

    /// Sort `ids` into paint order — bottom-most first, so a later element
    /// paints on top. Stable and total (see [`PaintKey`]).
    ///
    /// Crate-private helper for `SceneView::paint_band`. An unknown id sorts
    /// with the lowest possible key rather than being dropped, so a sort can
    /// never lose an entry.
    pub(crate) fn sort_by_paint_key(&self, ids: &mut [ItemId]) {
        ids.sort_by_key(|id| {
            self.paint_key(*id)
                .unwrap_or_else(|| PaintKey::new(crate::pick::RANK_UNDER, 0.0, id.as_u64()))
        });
    }

    /// [`Scene::sort_by_paint_key`] reversed — **topmost first**, the order
    /// every hit test walks.
    pub(crate) fn sort_by_paint_key_desc(&self, ids: &mut [ItemId]) {
        self.sort_by_paint_key(ids);
        ids.reverse();
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
    ///
    /// # What happens to what it destroyed
    ///
    /// This is [`Scene::take`] with the salvage routed to the edit sink
    /// instead of to the caller — identical work, identical events. With a
    /// sink installed
    /// ([`SceneModel::set_edit_sink`](crate::SceneModel::set_edit_sink)), each
    /// removed entry arrives as a
    /// [`SceneEdit::Removed`] carrying its
    /// [`RemovedItem`], which [`Scene::restore`] puts back. With none, the
    /// salvage is dropped, which is what a removal has always done.
    pub fn remove(&mut self, id: ItemId) {
        let salvage = self.take_inner(id);
        if salvage.is_empty() {
            return;
        }
        if self.journal.recording() {
            for item in salvage {
                let id = item.id();
                self.journal.record(SceneEdit::Removed {
                    id,
                    salvage: Salvage::Owned(Box::new(item)),
                });
            }
        }
    }

    /// Remove `id` and every descendant, **handing the salvage back** instead
    /// of dropping it — item box, handlers, magnets and logical-AT decorations
    /// intact, each still carrying its original [`ItemId`].
    ///
    /// The same work and the same events as [`Scene::remove`]; the difference
    /// is who ends up owning what was removed. Feed a result to
    /// [`Scene::restore_all`] to put the subtree back.
    ///
    /// # Order
    ///
    /// Deepest descendants first, the named root last — the order `remove`
    /// announces in. [`Scene::restore_all`] reverses it; restoring by hand
    /// means roots first.
    ///
    /// ```
    /// use teksilo_canvas::{Point, Rect};
    /// use teksilo_scene::{RectItem, Scene};
    ///
    /// let mut scene = Scene::new();
    /// let card = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 40.0, 40.0)), Point::ZERO);
    /// let salvage = scene.take(card);
    /// assert_eq!(scene.len(), 0);
    ///
    /// // Back at the same id, so selection, magnets and AT parenting still resolve.
    /// let restored = scene.restore_all(salvage).unwrap();
    /// assert_eq!(restored, vec![card]);
    /// assert_eq!(scene.len(), 1);
    /// ```
    #[must_use = "the salvage is the only copy of the removed items; dropping it \
                  makes the removal irreversible — call Scene::remove if that is \
                  what you meant"]
    pub fn take(&mut self, id: ItemId) -> Vec<RemovedItem> {
        let salvage = self.take_inner(id);
        if self.journal.recording() {
            for item in &salvage {
                self.journal.record(SceneEdit::Removed {
                    id: item.id(),
                    salvage: Salvage::TakenByCaller,
                });
            }
        }
        salvage
    }

    /// The one removal path. Lifts `id`'s subtree out of the scene whole and
    /// returns it; [`Scene::remove`] and [`Scene::take`] differ only in where
    /// the result goes.
    ///
    /// Written as one function on purpose: a `remove` that re-implemented the
    /// teardown would be free to forget a side map, and the salvage would then
    /// be a faithful copy of everything except the thing that was forgotten.
    fn take_inner(&mut self, id: ItemId) -> Vec<RemovedItem> {
        if !self.entry_index.contains_key(&id) {
            return Vec::new();
        }
        // A subtree removal is one logical edit, and this is the one mutator
        // that emits many changes from a single call — so it opens its own
        // scope rather than relying on the caller's. Through a `SceneModel` it
        // simply nests inside the write scope already open; through a bare
        // `&mut Scene`, which opens none, it is what keeps the removal from
        // being N unrelated transactions. Costs nothing when nothing is
        // recording: a transaction with no edits is never queued.
        let scope = RemovalScope::new(&self.journal);
        // Descendants, deepest-first via collect_descendants's BFS
        // (the order is leaf-to-root because we push children as we
        // visit each parent). Append the named id last.
        let mut to_remove: Vec<ItemId> = Vec::new();
        self.collect_descendants(id, &mut to_remove);
        to_remove.reverse();
        to_remove.push(id);
        let removal_set: HashSet<ItemId> = to_remove.iter().copied().collect();
        // Detach the subtree from whatever survives above it, before the
        // entries go: the `parent` ⇄ `children` invariant has to hold for the
        // surviving parent, whose list would otherwise name a dead id and send
        // the next `rebucket_subtree` walking into an entry that no longer
        // exists. Descendants are removed together with their parents, so only
        // the named root can have a surviving parent — the loop is written
        // against every removed id anyway, so a partial removal could never
        // leave a stale link.
        //
        // The removed root keeps its own `parent` field, because that is what a
        // restore needs in order to put it back where it was.
        for removed_id in &to_remove {
            let parent = self
                .entry_index
                .get(removed_id)
                .map(|&pos| self.entries[pos].parent)
                .unwrap_or(None);
            if let Some(parent) = parent
                && !removal_set.contains(&parent)
            {
                self.unlink_child(Some(parent), *removed_id);
            }
        }

        // Declaration order, read before the entries move. This is the order
        // the accessibility walk publishes siblings in — the order a screen
        // reader reads the scene in — so a restore puts the entry back at its
        // index rather than silently appending it to the end of the reading
        // order.
        let mut orders: HashMap<ItemId, usize> = HashMap::with_capacity(removal_set.len());
        for removed_id in &removal_set {
            if let Some(&pos) = self.entry_index.get(removed_id) {
                orders.insert(*removed_id, pos);
            }
        }

        let mut decorations = self.harvest_a11y(&removal_set);

        let mut magnets: HashMap<ItemId, Vec<(MagnetId, Magnet)>> = HashMap::new();
        for removed_id in &removal_set {
            // Magnets are local to the item, so a removed item takes them with
            // it — ids included, which is what lets a consumer keying
            // connections on `MagnetId` survive a restore.
            if let Some(attached) = self.magnets.remove(removed_id) {
                for (mid, _) in &attached {
                    self.magnet_owner.remove(mid);
                }
                magnets.insert(*removed_id, attached);
            }
        }

        // Partition rather than `retain`: the entries are moved out, not
        // dropped. That single change is what makes every removal salvageable,
        // and it costs the same walk.
        let mut taken: HashMap<ItemId, SceneEntry> = HashMap::with_capacity(removal_set.len());
        let mut kept: Vec<SceneEntry> = Vec::with_capacity(self.entries.len() - removal_set.len());
        for entry in self.entries.drain(..) {
            if removal_set.contains(&entry.id) {
                taken.insert(entry.id, entry);
            } else {
                kept.push(entry);
            }
        }
        self.entries = kept;
        self.heavyweight.retain(|id| !removal_set.contains(id));
        self.dynamic.retain(|id| !removal_set.contains(id));
        self.entry_index.clear();
        for (pos, entry) in self.entries.iter().enumerate() {
            self.entry_index.insert(entry.id, pos);
        }

        let mut salvage = Vec::with_capacity(to_remove.len());
        for removed_id in to_remove {
            self.index.remove(removed_id);
            if let Some(entry) = taken.remove(&removed_id) {
                salvage.push(RemovedItem {
                    entry,
                    entry_order: orders.get(&removed_id).copied().unwrap_or(usize::MAX),
                    magnets: magnets.remove(&removed_id).unwrap_or_default(),
                    a11y: decorations.remove(&removed_id).unwrap_or_default(),
                });
            }
            self.emit_item_change(ItemChange::Removed { id: removed_id });
        }
        drop(scope);
        salvage
    }

    /// Lift every logical-AT decoration that a removal of `removal_set` would
    /// destroy out of the scene's maps, partitioned by the removed item it
    /// belongs to.
    ///
    /// **Both directions of every edge.** A removal cuts an item's own AT
    /// parent (`removed → parent`) *and* every surviving node that was
    /// AT-parented under it (`survivor → removed`) — the second re-roots those
    /// survivors at the view root, which is correct while the item is gone and
    /// has to be undone when it comes back. Recording only the first would make
    /// a restore silently fail to re-adopt them, i.e. fail at exactly the case
    /// this salvage exists for. The same applies to relations, which a removal
    /// drops on either endpoint.
    ///
    /// A relation between two removed items is recorded once, on the `from`
    /// endpoint's salvage, so restoring the pair does not duplicate it.
    fn harvest_a11y(
        &mut self,
        removal_set: &HashSet<ItemId>,
    ) -> HashMap<ItemId, ItemA11yDecorations> {
        let mut out: HashMap<ItemId, ItemA11yDecorations> = HashMap::new();
        let removed_item = |n: &A11yNode| match n {
            A11yNode::Item(i) if removal_set.contains(i) => Some(*i),
            _ => None,
        };

        if !self.a11y_parents.is_empty() {
            let mut cut: Vec<A11yNode> = Vec::new();
            for (child, parent) in self.a11y_parents.iter() {
                if let Some(cid) = removed_item(child) {
                    out.entry(cid).or_default().parent = Some(*parent);
                    cut.push(*child);
                } else if let Some(pid) = removed_item(parent) {
                    out.entry(pid).or_default().adopted.push(*child);
                    cut.push(*child);
                }
            }
            for child in cut {
                self.a11y_parents.remove(&child);
            }
        }

        if !self.a11y_relations.is_empty() {
            let all = std::mem::take(&mut self.a11y_relations);
            let mut survivors = Vec::with_capacity(all.len());
            for (from, relation, to) in all {
                match removed_item(&from).or_else(|| removed_item(&to)) {
                    Some(owner) => out
                        .entry(owner)
                        .or_default()
                        .relations
                        .push((from, relation, to)),
                    None => survivors.push((from, relation, to)),
                }
            }
            self.a11y_relations = survivors;
        }

        for removed_id in removal_set {
            let node = A11yNode::Item(*removed_id);
            let live = self.a11y_live.remove(&node);
            let landmark = self.a11y_landmarks.remove(&node);
            let categories = self.a11y_categories.remove(&node);
            if live.is_none() && landmark.is_none() && categories.is_none() {
                continue;
            }
            let slot = out.entry(*removed_id).or_default();
            slot.live = live;
            slot.landmark = landmark;
            slot.categories = categories.unwrap_or_default();
        }
        out
    }

    /// Re-insert a salvaged entry **at its original [`ItemId`]**, with its
    /// magnets and its logical-AT decorations.
    ///
    /// # Identity is the point
    ///
    /// [`SceneSelection`](crate::SceneSelection) is keyed by `ItemId`,
    /// `MagnetId → ItemId` is keyed by it, the whole logical AT tree is keyed
    /// by `A11yNode::Item(ItemId)`, and — because a [`SceneItem`] has no
    /// downcast — an app's own side map is the *only* way to reach item-specific
    /// state, so it is keyed by it too. A restore that minted a fresh id would
    /// restore the pixels and lose every one of them.
    ///
    /// # Order
    ///
    /// Roots before children: a salvage naming a parent that is not in the
    /// scene fails with [`RestoreError::MissingParent`].
    /// [`Scene::restore_all`] orders a whole [`Scene::take`] result for you.
    ///
    /// # What comes back, and what does not
    ///
    /// Geometry, transform, z, layer, parent link, flags, opacity, handlers,
    /// magnets (ids included) and every logical-AT decoration whose other
    /// endpoint is still alive. The entry returns to its recorded position in
    /// declaration order when that index still exists, so the accessibility
    /// reading order is preserved rather than the item being appended last.
    ///
    /// An edge to a node that has since been removed is **not** re-attached:
    /// re-inserting it would name something the AT walker cannot resolve. Edges
    /// are therefore attached **after** the entry is in, and — under
    /// [`restore_all`](Self::restore_all) — after the whole batch is in, so an
    /// edge between two items of one removed subtree is not dropped merely
    /// because its far end had not arrived yet.
    ///
    /// A single-view heavyweight widget (`Scene::add_widget`) whose one
    /// `Box<dyn Widget>` a view already drained restores as an entry with no
    /// instance to materialise — unless the take and the restore happen inside
    /// one build cycle, in which case the view's orphan reap never runs and the
    /// arena instance survives. The restore is accepted either way, because
    /// refusing it would refuse the restores that work; see
    /// [`RemovedItem::widget_instance_present`]. Content that must survive an
    /// undo in every view goes in through
    /// [`SceneModel::add_widget_item`](crate::SceneModel::add_widget_item).
    ///
    /// # Restoring into a *different* scene is supported
    ///
    /// A [`RemovedItem`] taken from one scene may be restored into another,
    /// and that is the move-between-documents door: cut a card out of one
    /// board with [`take`](Self::take), restore it into a second, and it
    /// arrives whole — item box, geometry, flags, handlers and magnets, at the
    /// same [`ItemId`].
    ///
    /// Nothing special makes it work and nothing needs to refuse it.
    /// [`ItemId`] is minted from a process-global counter, so an id from
    /// another scene can never collide with one here; a salvage carries its
    /// whole entry rather than an index into the scene it came from; and the
    /// three consequences of crossing the boundary are exactly the rules that
    /// already apply within one scene:
    ///
    /// * **A parented salvage is refused** with
    ///   [`RestoreError::MissingParent`], because the parent it names is still
    ///   in the other scene. Move the parent first and the child follows — the
    ///   same roots-before-children rule, doing the same job — or call
    ///   [`RemovedItem::detach`] to bring the item over as a root.
    /// * **Logical-AT edges whose far end is not here are dropped**, by the
    ///   same test that drops an edge to a since-removed node.
    /// * **Declaration order is clamped**: the recorded index is where the
    ///   entry sat in the *source* scene's reading order, and a shorter target
    ///   takes it at the end.
    ///
    /// The salvage is consumed either way, so a refused move does not leave a
    /// second copy behind — but it does drop the item, so check the `Result`.
    pub fn restore(&mut self, salvage: RemovedItem) -> Result<ItemId, RestoreError> {
        let mut edges = Vec::new();
        let result = self.restore_entry(salvage, &mut edges);
        // Attached even on the error path: a failure to restore *this* salvage
        // must not strand edges an earlier one already deferred.
        self.attach_a11y_edges(edges);
        result
    }

    /// Restore a whole [`Scene::take`] result, roots first.
    ///
    /// `take` hands back leaves-then-root; restoring in that order would fail
    /// the first child on [`RestoreError::MissingParent`], so this reverses it
    /// for you — which also puts each parent's `children` list back in its
    /// original order.
    ///
    /// Logical-AT edges are attached once the **whole batch** is in, so an edge
    /// between two items of the removed subtree survives whatever order the two
    /// ends land in.
    ///
    /// Stops at the first failure and returns it; everything restored before it
    /// **stays restored**, because a rollback here would be the framework
    /// applying an inverse, which is the data layer's job.
    pub fn restore_all(&mut self, salvage: Vec<RemovedItem>) -> Result<Vec<ItemId>, RestoreError> {
        let mut restored = Vec::with_capacity(salvage.len());
        let mut edges = Vec::new();
        let mut failure = None;
        for item in salvage.into_iter().rev() {
            match self.restore_entry(item, &mut edges) {
                Ok(id) => restored.push(id),
                Err(err) => {
                    failure = Some(err);
                    break;
                }
            }
        }
        self.attach_a11y_edges(edges);
        match failure {
            Some(err) => Err(err),
            None => Ok(restored),
        }
    }

    /// Put one salvaged entry back and re-attach everything that depends on
    /// nothing else: magnets, live-region status, landmark role, categories.
    ///
    /// Its logical-AT **edges** are pushed onto `edges` instead, because an
    /// edge's far end may be another salvage in the same batch that has not
    /// been restored yet — and an edge dropped for that reason would be the
    /// silent a11y loss this whole door exists to prevent.
    fn restore_entry(
        &mut self,
        salvage: RemovedItem,
        edges: &mut Vec<(ItemId, ItemA11yDecorations)>,
    ) -> Result<ItemId, RestoreError> {
        let id = salvage.entry.id;
        if self.entry_index.contains_key(&id) {
            return Err(RestoreError::IdAlreadyLive(id));
        }
        if let Some(parent) = salvage.entry.parent
            && !self.entry_index.contains_key(&parent)
        {
            return Err(RestoreError::MissingParent { id, parent });
        }

        let RemovedItem {
            mut entry,
            entry_order,
            magnets,
            mut a11y,
        } = salvage;
        // `link_child` rebuilds this list as each child is restored after its
        // parent, so keeping the salvaged copy would double every entry — and a
        // child that is never restored would leave a link to nothing.
        entry.children.clear();
        self.push_entry_at(entry, entry_order);

        if !magnets.is_empty() {
            for (mid, _) in &magnets {
                self.magnet_owner.insert(*mid, id);
            }
            self.magnets.insert(id, magnets);
        }

        let node = A11yNode::Item(id);
        let mut touched = false;
        if let Some(live) = a11y.live.take() {
            self.a11y_live.insert(node, live);
            touched = true;
        }
        if let Some(landmark) = a11y.landmark.take() {
            self.a11y_landmarks.insert(node, landmark);
            touched = true;
        }
        if !a11y.categories.is_empty() {
            self.a11y_categories
                .insert(node, std::mem::take(&mut a11y.categories));
            touched = true;
        }
        if touched {
            self.bump_a11y_change(node);
        }
        if a11y.parent.is_some() || !a11y.adopted.is_empty() || !a11y.relations.is_empty() {
            edges.push((id, a11y));
        }
        Ok(id)
    }

    /// Re-attach the deferred logical-AT edges, skipping every one whose other
    /// endpoint is not in the scene.
    fn attach_a11y_edges(&mut self, edges: Vec<(ItemId, ItemA11yDecorations)>) {
        for (id, a11y) in edges {
            let node = A11yNode::Item(id);
            if let Some(parent) = a11y.parent
                && self.a11y_node_exists(parent)
            {
                self.a11y_parents.insert(node, parent);
            }
            for child in a11y.adopted {
                if self.a11y_node_exists(child) {
                    self.a11y_parents.insert(child, node);
                }
            }
            for (from, relation, to) in a11y.relations {
                if self.a11y_node_exists(from) && self.a11y_node_exists(to) {
                    self.a11y_relations.push((from, relation, to));
                }
            }
            self.bump_a11y_change(node);
        }
    }

    /// Whether a logical-AT node is resolvable in this scene right now.
    ///
    /// A `Widget` node addresses the arena rather than the scene, so the scene
    /// cannot check it and does not pretend to — an edge to one is re-attached
    /// and the AT walker skips it if the widget is gone, exactly as it does for
    /// one the app declared directly.
    fn a11y_node_exists(&self, node: A11yNode) -> bool {
        match node {
            A11yNode::Item(id) => self.entry_index.contains_key(&id),
            A11yNode::Group(id) => self.a11y_group_index.contains_key(&id),
            A11yNode::Widget(_) => true,
        }
    }

    /// Swap the lightweight item box at `id`, keeping the entry — and the
    /// [`ItemId`]. Returns the box that was there.
    ///
    /// Position, transform, z, layer, parent, flags, opacity, handlers, magnets
    /// and logical-AT decorations all survive, so a data row whose *content*
    /// changed does not lose its selection membership, its connections or its
    /// place in the accessibility tree. Emits one
    /// [`ItemChange::ItemReplaced`], not `Removed` + `Added`.
    ///
    /// # Bounds and the spatial index
    ///
    /// The entry's AABB is **derived from the item**, so the new item's
    /// `local_bounds` is read and the subtree re-bucketed. Skipping that would
    /// leave the old rectangle in the index and quietly break
    /// [`items_in_rect`](Self::items_in_rect), [`item_at`](Self::item_at), the
    /// cull path and every cached hit snapshot.
    ///
    /// # Flags
    ///
    /// The **entry's** flags are kept; the replacement's
    /// [`initial_flags`](SceneItem::initial_flags) are not consulted. They are
    /// *initial* flags — they apply where an item is inserted — and an app that
    /// has since called [`set_visible`](Self::set_visible) or
    /// [`set_flag`](Self::set_flag) would otherwise have that silently undone
    /// by a content refresh. Call [`set_flags`](Self::set_flags) afterwards to
    /// adopt the new item's instead.
    ///
    /// # Errors
    ///
    /// [`ReplaceItemError::UnknownItem`] for an id that is not in the scene,
    /// [`ReplaceItemError::NotLightweight`] for a heavyweight widget entry —
    /// swap a `Delegated` entry's *data* with
    /// [`SceneModel::set_payload`](crate::SceneModel::set_payload) instead.
    ///
    /// Either way the [`ReplaceRejected`] hands the item **back**: a refused
    /// write must not eat a value the caller cannot clone and may have paid to
    /// build.
    pub fn replace_item(
        &mut self,
        id: ItemId,
        item: Box<dyn SceneItem>,
    ) -> Result<Box<dyn SceneItem>, ReplaceRejected> {
        let Some(&pos) = self.entry_index.get(&id) else {
            return Err(ReplaceRejected {
                error: ReplaceItemError::UnknownItem(id),
                item,
            });
        };
        if !matches!(self.entries[pos].kind, SceneEntryKind::Item(_)) {
            return Err(ReplaceRejected {
                error: ReplaceItemError::NotLightweight(id),
                item,
            });
        }
        let new_bounds = item.local_bounds();
        let old_bounds = self.entries[pos].local_bounds;
        let SceneEntryKind::Item(slot) = &mut self.entries[pos].kind else {
            unreachable!("checked above")
        };
        let previous = std::mem::replace(slot, item);
        self.entries[pos].local_bounds = new_bounds;
        self.rebucket_subtree(id);
        self.emit_item_change(ItemChange::ItemReplaced {
            id,
            old_bounds,
            new_bounds,
        });
        Ok(previous)
    }

    // -----------------------------------------------------------------
    // Placement (parent + z + position + transform, atomically)
    // -----------------------------------------------------------------

    /// Where `id` sits, as one value. `None` for an unknown id.
    pub fn placement(&self, id: ItemId) -> Option<Placement> {
        let pos = *self.entry_index.get(&id)?;
        let entry = &self.entries[pos];
        Some(Placement {
            parent: entry.parent,
            z: entry.z,
            local_pos: entry.local_pos,
            transform: entry.transform,
        })
    }

    /// Write parent, z, position and transform **together**, emitting one
    /// [`ItemChange::PlacementChanged`].
    ///
    /// The atomic alternative to four mutators and four events. No-op when the
    /// placement is unchanged, when `id` is unknown, and — as with
    /// [`set_item_parent`](Self::set_item_parent) — when the proposed parent is
    /// `id` itself or one of its own descendants, which would make a cycle.
    /// The whole write is refused in that last case rather than applied with
    /// the old parent, because a partly-applied atomic write is the thing this
    /// door exists to prevent.
    pub fn set_placement(&mut self, id: ItemId, placement: Placement) {
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        let old = Placement {
            parent: self.entries[pos].parent,
            z: self.entries[pos].z,
            local_pos: self.entries[pos].local_pos,
            transform: self.entries[pos].transform,
        };
        if old == placement {
            return;
        }
        if let Some(p) = placement.parent
            && (p == id || self.is_descendant_of(p, id))
        {
            return;
        }
        if old.parent != placement.parent {
            self.entries[pos].parent = placement.parent;
            self.unlink_child(old.parent, id);
            self.link_child(placement.parent, id);
        }
        self.entries[pos].z = placement.z;
        self.entries[pos].local_pos = placement.local_pos;
        self.entries[pos].transform = placement.transform;
        self.rebucket_subtree(id);
        self.emit_item_change(ItemChange::PlacementChanged {
            id,
            old,
            new: placement,
        });
    }

    /// Reparent `id` while holding it **visually still** — "drag this card into
    /// that group", correctly.
    ///
    /// [`set_item_parent`](Self::set_item_parent) reinterprets `local_pos` and
    /// `transform` in the new parent's frame, so the item jumps unless the
    /// caller compensates. This derives the local frame that leaves the item's
    /// scene transform where it is, and applies parent and frame as **one**
    /// [`set_placement`](Self::set_placement) write.
    ///
    /// The derived frame is normalised: the rotation/scale go in `transform`
    /// (around the local origin, which is what that field means) and all of the
    /// translation in `local_pos`. An item whose `transform` carried a
    /// translation of its own comes back with the same visual result and that
    /// translation folded into its position.
    ///
    /// No-op for an unknown id, for a parent that is already the current one,
    /// for a cycle, and for a new parent whose scene transform is degenerate
    /// (a zero scale somewhere in its chain) — there is no frame under it to
    /// land in.
    pub fn reparent_keeping_scene_pos(&mut self, id: ItemId, parent: Option<ItemId>) {
        let Some(&pos) = self.entry_index.get(&id) else {
            return;
        };
        if self.entries[pos].parent == parent {
            return;
        }
        if let Some(p) = parent
            && (p == id || self.is_descendant_of(p, id) || !self.entry_index.contains_key(&p))
        {
            return;
        }
        let old_scene = self.scene_transform(id);
        let parent_scene = match parent {
            Some(p) => self.scene_transform(p),
            None => Transform2D::identity(),
        };
        let Some(parent_inverse) = parent_scene.inverse() else {
            return;
        };
        // `t.then(u)` is "apply t, then u", i.e. `u * t`. We want the local
        // frame `l` with `l.then(parent_scene) == old_scene`, so
        // `l = parent_scene⁻¹ * old_scene = old_scene.then(parent_scene⁻¹)`.
        let local = old_scene.then(&parent_inverse);
        let [a, b, c, d, tx, ty] = local.m;
        self.set_placement(
            id,
            Placement {
                parent,
                z: self.entries[pos].z,
                local_pos: Point::new(tx, ty),
                transform: Transform2D {
                    m: [a, b, c, d, 0.0, 0.0],
                },
            },
        );
    }

    /// A `z` strictly between two items' — the fractional insert that puts one
    /// item between two others without renumbering a single sibling.
    ///
    /// `None` when there is no such value: the two are at the same `z`, either
    /// id is unknown, or `f32` precision is exhausted at that locus. An `f32`
    /// carries ~24 mantissa bits, so about two dozen bisections at one point in
    /// the order run out — reachable in any card-shuffling UI.
    ///
    /// Asking first is what makes that visible. A caller that computed an
    /// exhausted midpoint itself would get `mid == lo`, hand it to
    /// [`set_z`](Self::set_z), and be given a **silent no-op**: the card simply
    /// would not move, with no error and no event. The answer to `None` is to
    /// renumber the band (a pass of evenly-spaced `z` values) and bisect again.
    ///
    /// The two agree on one metric, and it is this one. `set_z` ignores a write
    /// only when the entry already holds that exact float, so **every `Some`
    /// this returns is a value `set_z` will apply** — which is the whole
    /// contract. (An absolute epsilon there used to break it near zero: with
    /// `lo = 0.0` and `hi = 1.25e-7` this answers `Some(6.25e-8)` and the write
    /// was dropped.)
    ///
    /// ```
    /// use teksilo_canvas::{Point, Rect};
    /// use teksilo_scene::{RectItem, Scene};
    ///
    /// let mut scene = Scene::new();
    /// let r = || RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0));
    /// let lower = scene.add_item(r(), Point::ZERO);
    /// let upper = scene.add_item(r(), Point::ZERO);
    /// scene.set_z(lower, 1.0);
    /// scene.set_z(upper, 2.0);
    ///
    /// let middle = scene.add_item(r(), Point::ZERO);
    /// let z = scene.z_between(lower, upper).expect("room between 1.0 and 2.0");
    /// scene.set_z(middle, z);
    /// assert!(scene.z(middle).unwrap() > 1.0 && scene.z(middle).unwrap() < 2.0);
    ///
    /// // No room between an item and itself.
    /// assert_eq!(scene.z_between(lower, lower), None);
    /// ```
    pub fn z_between(&self, below: ItemId, above: ItemId) -> Option<f32> {
        let a = self.z(below)?;
        let b = self.z(above)?;
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        let mid = lo + (hi - lo) / 2.0;
        (mid > lo && mid < hi).then_some(mid)
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
        let children: Vec<ItemId> = {
            let pos = self.entry_index[&id];
            self.entries[pos].children.clone()
        };
        for child in children {
            if let Some(&pos) = self.entry_index.get(&child) {
                self.entries[pos].parent = None;
                self.unlink_child(Some(id), child);
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

    /// Where `id` sits in `entries`, the scene's declaration order.
    ///
    /// The accessibility walk publishes siblings in this order — it is the
    /// order a screen reader reads the scene in — so a consumer that narrows
    /// the walk to a subset (the items inside a region, say) has to restore
    /// it, and a `HashSet` of ids does not carry it.
    pub(crate) fn entry_order(&self, id: ItemId) -> Option<usize> {
        self.entry_index.get(&id).copied()
    }

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

    /// The shape of any entry, in its **local** coordinates.
    ///
    /// For a lightweight item this is [`SceneItem::shape`]. A **heavyweight**
    /// widget entry has no `SceneItem` at all — [`Scene::item`] returns `None`
    /// for it — so its shape is defined to be
    /// [`ItemShape::bounds`] of its `local_bounds`. That is not a placeholder:
    /// a widget's silhouette is its layout box, the arena hit-tests it as one,
    /// and the marquee has always selected heavyweight entries through the
    /// same index query as lightweight ones. Stating it here is what keeps a
    /// `…ItemShape` selection mode meaningful for a scene whose primary
    /// objects are cards.
    ///
    /// `None` for an unknown id.
    pub fn item_shape(&self, id: ItemId) -> Option<ItemShape> {
        let pos = *self.entry_index.get(&id)?;
        Some(match &self.entries[pos].kind {
            SceneEntryKind::Item(item) => item.shape(),
            SceneEntryKind::Widget(_) => ItemShape::bounds(self.entries[pos].local_bounds),
        })
    }

    /// Whether `id`'s shape contains `scene_pt`.
    ///
    /// Works for both tiers (see [`Scene::item_shape`]). `view_scale` is the
    /// live view zoom, consulted only by a cosmetic stroke band; pass `1.0`
    /// when there is no view.
    pub fn item_contains(&self, id: ItemId, scene_pt: Point, view_scale: f32) -> bool {
        let Some(shape) = self.item_shape(id) else {
            return false;
        };
        let Some(local_pt) = self.map_from_scene(id, scene_pt) else {
            return false;
        };
        shape.contains(local_pt, view_scale)
    }

    /// The item's own shape re-published as a **scene**-space region — what a
    /// collision query asks the rest of the scene about. `None` for an unknown
    /// id, a shape of [`ItemShape::none`], or a screen-anchored
    /// ([`IGNORES_TRANSFORMATIONS`](crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS))
    /// item, whose silhouette is not in scene space at all.
    pub fn item_region(&self, id: ItemId) -> Option<SceneRegion> {
        // A screen-anchored item has no scene-space silhouette to publish:
        // its `local_bounds` is in screen coordinates and its scene transform
        // places only its anchor. Same reason `items_in_region` skips them.
        if self.is_screen_anchored(id) {
            return None;
        }
        let shape = self.item_shape(id)?;
        if shape.is_none() {
            return None;
        }
        let xform = self.scene_transform(id);
        Some(shape.to_scene_region(&xform))
    }

    /// Items matching `region` under `mode` — **both tiers**, exactly like
    /// [`Scene::items_in_rect`], which this generalises.
    ///
    /// Broad-phased by the spatial index on `region.bounding_rect()`, then
    /// narrow-phased per `mode`: the `…ItemBoundingRect` modes compare the
    /// item's **scene** AABB against the region in scene space; the
    /// `…ItemShape` modes map the region into the item's **local** frame and
    /// compare it against [`Scene::item_shape`]. For an item with a
    /// non-identity transform those are different tests — see
    /// [`ItemSelectionMode`].
    ///
    /// `view_scale` is the live view zoom, consulted only by a cosmetic stroke
    /// band; pass `1.0` when there is no view. Visibility and selectability
    /// are **not** filtered here — this is a pure geometry query, and the
    /// caller (e.g. [`SceneSelection::commit_marquee`](crate::SceneSelection::commit_marquee))
    /// applies its own flag policy.
    ///
    /// Items flagged
    /// [`IGNORES_TRANSFORMATIONS`](crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS)
    /// **are skipped**, for the reason [`Scene::item_at`] skips them: they are
    /// anchored in screen space, so their `local_bounds` is a screen rectangle
    /// and the scene AABB the index holds for them is a fiction. Comparing a
    /// scene-space region against that fiction is a guess, and the two query
    /// families used to disagree about whether to make it — the point queries
    /// declined and the marquee did not. Reaching screen-pinned chrome needs a
    /// region that arrives in *screen* space, which is what
    /// [`Scene::item_at_in_view`] does for a point; there is no region twin of
    /// it yet, so a marquee cannot select pinned chrome at all.
    pub fn items_in_region(
        &self,
        region: &SceneRegion,
        mode: ItemSelectionMode,
        view_scale: f32,
    ) -> Vec<ItemId> {
        self.index
            .query(region.bounding_rect())
            .into_iter()
            .filter(|id| !self.is_screen_anchored(*id))
            .filter(|id| self.region_match(*id, region, mode, view_scale))
            .collect()
    }

    /// Whether `id` is pinned in screen space
    /// ([`IGNORES_TRANSFORMATIONS`](crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS)),
    /// and so cannot be placed by a scene-space query at all. `false` for an
    /// unknown id.
    fn is_screen_anchored(&self, id: ItemId) -> bool {
        self.flags(id)
            .is_some_and(|f| f.contains(ItemFlags::IGNORES_TRANSFORMATIONS))
    }

    fn region_match(
        &self,
        id: ItemId,
        region: &SceneRegion,
        mode: ItemSelectionMode,
        view_scale: f32,
    ) -> bool {
        if mode.uses_shape() {
            let Some(shape) = self.item_shape(id) else {
                return false;
            };
            // The shape is local and the region is scene-space; which frame
            // the two meet in is a decision, not a detail, and `ItemShape`
            // owns it — see `ItemShape::contained_by_scene_region`.
            let xform = self.scene_transform(id);
            if mode.requires_containment() {
                shape.contained_by_scene_region(region, &xform, view_scale)
            } else {
                shape.intersects_scene_region(region, &xform, view_scale)
            }
        } else {
            let Some(rect) = self.scene_rect(id) else {
                return false;
            };
            let shape = ItemShape::bounds(rect);
            if mode.requires_containment() {
                shape.contained_by_region(region, 1.0)
            } else {
                shape.intersects_region(region, 1.0)
            }
        }
    }

    /// Topmost **lightweight** item whose shape contains `scene_pt`, at unit
    /// view scale. See [`Scene::item_at_scaled`] for the zoom-aware form and
    /// [`Scene::item_at_in_view`] for the one that also places screen-anchored
    /// items.
    ///
    /// Heavyweight widget entries are skipped: their hit-testing is the
    /// arena's job, and a scene-space answer would contradict it.
    ///
    /// Items flagged
    /// [`IGNORES_TRANSFORMATIONS`](crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS)
    /// are **also skipped**, deterministically. They are anchored in screen
    /// space, so a scene-space point cannot place them at all; answering with
    /// a coin-flip (which is what comparing them against a scene AABB amounts
    /// to) is worse than not answering. Use [`Scene::item_at_in_view`], or
    /// `SceneView` dispatch, when the query needs them.
    ///
    /// "Topmost" is [`Scene::paint_key`] order, so an
    /// [`Over`](SceneLayer::Over)-band item beats a higher-`z`
    /// [`Under`](SceneLayer::Under) one, and two equal-`z` items resolve to the
    /// later-inserted one — the same answer `SceneView` dispatch gives.
    ///
    /// Hidden and disabled entries are excluded: this is a *hit* test, and
    /// [`ItemFlags::IS_VISIBLE`] and [`ItemFlags::IS_ENABLED`] both say so. See
    /// [`Scene::is_hit_testable`]. For a pure geometry query that ignores flags,
    /// use [`Scene::items_in_region`] or [`Scene::item_contains`].
    pub fn item_at(&self, scene_pt: Point) -> Option<ItemId> {
        self.item_at_scaled(scene_pt, 1.0)
    }

    /// [`Scene::item_at`] at an explicit view zoom.
    ///
    /// The zoom reaches exactly one thing: a **cosmetic** stroke band, whose
    /// width is in device pixels and therefore covers fewer scene units the
    /// further you zoom in. Passing the live scale is what makes this agree
    /// with `SceneView`'s own dispatch, which has always had it.
    pub fn item_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId> {
        self.hit_candidates(scene_pt)
            .into_iter()
            .find(|id| self.lightweight_scene_hit(*id, scene_pt, view_scale))
    }

    /// Topmost entry of **either** tier whose geometry contains `scene_pt`, in
    /// this scene's one paint order.
    ///
    /// The deliberate counterpart to [`Scene::item_at_scaled`], which skips
    /// heavyweight entries because the arena owns their hit-testing. This one
    /// answers the different question a *selection* asks — "which entry is on
    /// top here?" — and answers it for a card with the card's scene rectangle,
    /// which is exactly the rectangle `SceneView` laid the card out at. It is
    /// not a substitute for arena dispatch and must not be used to route an
    /// event to a widget; it is for deciding whether a press that already
    /// reached the view landed on something the view is holding.
    ///
    /// Screen-anchored entries are skipped for the same reason `item_at` skips
    /// them: a scene-space point cannot place them.
    pub fn entry_at(&self, scene_pt: Point, view_scale: f32) -> Option<ItemId> {
        self.hit_candidates(scene_pt).into_iter().find(|id| {
            if self.item(*id).is_some() {
                self.lightweight_scene_hit(*id, scene_pt, view_scale)
            } else {
                self.scene_rect(*id).is_some_and(|r| r.contains(scene_pt))
            }
        })
    }

    /// All lightweight items whose shape contains `scene_pt`, topmost-first by
    /// z. Same tier and `IGNORES_TRANSFORMATIONS` rules as [`Scene::item_at`].
    pub fn items_at(&self, scene_pt: Point) -> Vec<ItemId> {
        self.items_at_scaled(scene_pt, 1.0)
    }

    /// [`Scene::items_at`] at an explicit view zoom.
    pub fn items_at_scaled(&self, scene_pt: Point, view_scale: f32) -> Vec<ItemId> {
        self.hit_candidates(scene_pt)
            .into_iter()
            .filter(|id| self.lightweight_scene_hit(*id, scene_pt, view_scale))
            .collect()
    }

    /// Topmost lightweight item under a **screen** point, resolving *both*
    /// hit spaces the way `SceneView` dispatch does.
    ///
    /// A normal item is tested in scene space, at the view transform's zoom; a
    /// screen-anchored
    /// ([`IGNORES_TRANSFORMATIONS`](crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS))
    /// item is tested against its `local_bounds` rooted at its projected
    /// anchor, at unit scale — its local coordinates *are* screen
    /// coordinates, so it has no zoom to convert. This is the query to use
    /// when a scene may contain screen-pinned chrome; [`Scene::item_at`]
    /// deliberately declines to guess.
    pub fn item_at_in_view(&self, screen_pt: Point, view_transform: Transform2D) -> Option<ItemId> {
        let view_scale = view_transform.geometric_scale();
        let scene_pt = view_transform.inverse()?.apply_point(screen_pt);
        // Screen-anchored items live outside the spatial index's scene-space
        // buckets, so they are scanned separately and tested first: they are
        // chrome pinned over the content.
        let mut pinned: Vec<ItemId> = self
            .entries
            .iter()
            .filter(|e| {
                matches!(e.kind, SceneEntryKind::Item(_))
                    && e.flags.contains(ItemFlags::IGNORES_TRANSFORMATIONS)
            })
            .map(|e| e.id)
            .filter(|id| self.is_hit_testable(*id))
            .collect();
        self.sort_by_paint_key_desc(&mut pinned);
        for id in pinned {
            let anchor =
                view_transform.apply_point(self.scene_transform(id).apply_point(Point::ZERO));
            let local_pt = Point::new(screen_pt.x - anchor.x, screen_pt.y - anchor.y);
            let Some(shape) = self.item_shape(id) else {
                continue;
            };
            if shape.contains(local_pt, 1.0) {
                return Some(id);
            }
        }
        self.item_at_scaled(scene_pt, view_scale)
    }

    /// Topmost-first candidate ids for a point query, screen-anchored items
    /// excluded (see [`Scene::item_at`]) and non-hit-testable entries dropped
    /// (see [`crate::pick::hit_testable`]).
    fn hit_candidates(&self, scene_pt: Point) -> Vec<ItemId> {
        let probe = Rect::new(scene_pt.x, scene_pt.y, 0.0, 0.0);
        let mut candidates: Vec<ItemId> = self
            .items_in_rect(probe)
            .into_iter()
            .filter(|id| self.is_hit_testable(*id))
            .filter(|id| !self.is_screen_anchored(*id))
            .collect();
        self.sort_by_paint_key_desc(&mut candidates);
        candidates
    }

    /// Whether `id` takes part in pointer hit-testing — visible along its whole
    /// ancestor chain AND enabled.
    ///
    /// This is [`crate::pick::hit_testable`] resolved against the scene, and it
    /// is the one place the two flag contracts in [`ItemFlags`] are honoured:
    /// [`IS_VISIBLE`](ItemFlags::IS_VISIBLE) ("neither painted nor hit-tested")
    /// and [`IS_ENABLED`](ItemFlags::IS_ENABLED) ("pass clicks through to items
    /// beneath"). `false` for unknown ids.
    pub fn is_hit_testable(&self, id: ItemId) -> bool {
        let Some(flags) = self.flags(id) else {
            return false;
        };
        crate::pick::hit_testable(self.is_effectively_visible(id), flags)
    }

    fn lightweight_scene_hit(&self, id: ItemId, scene_pt: Point, view_scale: f32) -> bool {
        let Some(item) = self.item(id) else {
            return false;
        };
        let Some(local_pt) = self.map_from_scene(id, scene_pt) else {
            return false;
        };
        item.shape().contains(local_pt, view_scale)
    }

    /// Items overlapping `id`'s **shape**, excluding `id` itself.
    ///
    /// Apps use this for "which other items overlap this card?" — graph
    /// editors checking node-on-node overlap, CAD canvases finding adjacent
    /// geometry. Backed by the spatial index, so the cost is `O(visible)` not
    /// `O(N)`.
    ///
    /// Defaults to [`ItemSelectionMode::IntersectsItemShape`], so a
    /// stroke-only connector collides along its line rather than across its
    /// bounding box. Pass
    /// [`IntersectsItemBoundingRect`](ItemSelectionMode::IntersectsItemBoundingRect)
    /// to [`Scene::colliding_items_with`] for the cheaper box test.
    ///
    /// Screen-anchored items neither collide nor are collided with — see
    /// [`Scene::items_in_region`], which this is built on, and
    /// [`Scene::item_region`], which declines to publish one for them.
    pub fn colliding_items(&self, id: ItemId) -> Vec<ItemId> {
        self.colliding_items_with(id, ItemSelectionMode::default())
    }

    /// [`Scene::colliding_items`] under an explicit [`ItemSelectionMode`].
    pub fn colliding_items_with(&self, id: ItemId, mode: ItemSelectionMode) -> Vec<ItemId> {
        let Some(region) = self.item_region(id) else {
            return Vec::new();
        };
        self.items_in_region(&region, mode, 1.0)
            .into_iter()
            .filter(|other| *other != id)
            .collect()
    }

    /// Items lying along `path` — a real region query, not the AABB-of-the-path
    /// approximation this used to be.
    ///
    /// The path is treated as a zero-width closed region: an item is picked
    /// when the path crosses it or encloses it. For a *connector* — a line
    /// with a width — pass that width to [`Scene::items_along_path_with`], so
    /// the query asks about the band the user can see.
    pub fn items_along_path(&self, path: &Path) -> Vec<ItemId> {
        self.items_along_path_with(path, 0.0, ItemSelectionMode::default())
    }

    /// [`Scene::items_along_path`] with an explicit stroke width and
    /// [`ItemSelectionMode`].
    ///
    /// `stroke_width` greater than zero makes the region the path's **band**
    /// rather than its interior: "what does this 4 dp connector touch?".
    pub fn items_along_path_with(
        &self,
        path: &Path,
        stroke_width: f32,
        mode: ItemSelectionMode,
    ) -> Vec<ItemId> {
        let region = if stroke_width > 0.0 {
            SceneRegion::stroke(path.clone(), stroke_width)
        } else {
            SceneRegion::lasso(path.clone())
        };
        self.items_in_region(&region, mode, 1.0)
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
        self.bump_a11y_change(A11yNode::Item(item));
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
        self.bump_a11y_change(A11yNode::Item(owner));
    }

    /// Remove every magnet attached to `item`. No-op if none.
    pub fn clear_magnets(&mut self, item: ItemId) {
        if let Some(list) = self.magnets.remove(&item) {
            for (mid, _) in list {
                self.magnet_owner.remove(&mid);
            }
            self.bump_a11y_change(A11yNode::Item(item));
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
            self.bump_a11y_change(A11yNode::Item(owner));
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
            self.bump_a11y_change(A11yNode::Item(owner));
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
        self.bump_a11y_change(A11yNode::Group(id));
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
        self.bump_a11y_change(target);
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
        self.bump_a11y_change(child);
    }

    /// The currently-declared logical parent of a node.
    pub fn a11y_parent_of(&self, child: A11yNode) -> Option<A11yNode> {
        self.a11y_parents.get(&child).copied()
    }

    /// Declare an AT relationship between two nodes.
    pub fn add_a11y_relation(&mut self, from: A11yNode, kind: A11yRelation, to: A11yNode) {
        self.a11y_relations.push((from, kind, to));
        self.bump_a11y_change(from);
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
        self.bump_a11y_change(node);
    }

    /// Mark a node as a landmark by overriding its role. Pass
    /// `Role::Unknown` to clear.
    pub fn set_a11y_landmark(&mut self, node: A11yNode, role: accesskit::Role) {
        if matches!(role, accesskit::Role::Unknown) {
            self.a11y_landmarks.remove(&node);
        } else {
            self.a11y_landmarks.insert(node, role);
        }
        self.bump_a11y_change(node);
    }

    /// Tag a node with rotor / quick-nav categories.
    pub fn set_a11y_categories(&mut self, node: A11yNode, categories: &[A11yCategory]) {
        if categories.is_empty() {
            self.a11y_categories.remove(&node);
        } else {
            self.a11y_categories.insert(node, categories.to_vec());
        }
        self.bump_a11y_change(node);
    }

    /// Read declared categories for a node.
    pub fn a11y_categories_of(&self, node: A11yNode) -> Option<&[A11yCategory]> {
        self.a11y_categories.get(&node).map(|v| v.as_slice())
    }

    /// Read a node's declared live-region politeness. `None` when it is not a
    /// live region.
    ///
    /// The read half of [`set_a11y_live`](Self::set_a11y_live), added because a
    /// consumer restoring a [`RemovedItem`] has to be able to check that the
    /// semantics came back — and because the other four decorations already
    /// had one.
    pub fn a11y_live_of(&self, node: A11yNode) -> Option<accesskit::Live> {
        self.a11y_live.get(&node).copied()
    }

    /// Read a node's declared landmark role. `None` when it is not a landmark.
    pub fn a11y_landmark_of(&self, node: A11yNode) -> Option<accesskit::Role> {
        self.a11y_landmarks.get(&node).copied()
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
            .field("constrained", &self.geometry_constraint.is_some())
            .finish_non_exhaustive()
    }
}

/// Half-open AABB intersection: two rects intersect iff their
/// projections overlap on both axes.
pub(crate) fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// AABB of the union of two rectangles.
pub(crate) fn union_two_rects(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = a.right().max(b.right());
    let bot = a.bottom().max(b.bottom());
    Rect::new(x, y, r - x, bot - y)
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
        use std::rc::Rc;
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        let last = Rc::new(std::cell::RefCell::new(None::<ItemChange>));
        let last_clone = last.clone();
        let _h = scene.item_change_signal().observe(move |c| {
            *last_clone.borrow_mut() = Some(c.change.clone());
        });
        scene.set_local_pos(id, Point::new(50.0, 60.0));
        let taken = last.borrow().clone();
        match taken {
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
            if matches!(c.change, ItemChange::VisibilityChanged { .. }) {
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
    fn items_along_path_finds_items_the_path_actually_crosses() {
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

    #[test]
    fn items_along_path_no_longer_picks_up_the_paths_whole_bounding_box() {
        // This is the row the old AABB-vs-AABB narrow phase got wrong: the
        // corner item sits inside the diagonal's bounding box and nowhere near
        // the diagonal itself.
        let mut scene = Scene::new();
        let off_the_line = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(2.0, 85.0),
        );
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));
        assert!(!scene.items_along_path(&path).contains(&off_the_line));
        // A wide enough connector does reach it.
        assert!(
            scene
                .items_along_path_with(&path, 200.0, ItemSelectionMode::IntersectsItemShape)
                .contains(&off_the_line)
        );
    }

    #[test]
    fn colliding_items_uses_the_shape_not_the_box() {
        // A stroke-only diagonal connector crosses `crossed` and merely
        // overlaps `beside` in the bounding-box sense.
        let mut scene = Scene::new();
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 100.0));
        let wire = scene.add_item(
            crate::items::PathItem::new(path).stroke(teksilo_tokens::Color::BLACK, 2.0),
            Point::ZERO,
        );
        let crossed = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(45.0, 45.0),
        );
        let beside = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(2.0, 85.0),
        );
        let hits = scene.colliding_items(wire);
        assert!(hits.contains(&crossed));
        assert!(
            !hits.contains(&beside),
            "the empty corner of its box is not it"
        );

        // The mode is applied to the *other* item, exactly as Qt's
        // `collidesWithItem` applies it: asking what collides with `beside`,
        // the wire's own shape misses and the wire's bounding rect hits.
        assert!(
            !scene
                .colliding_items_with(beside, ItemSelectionMode::IntersectsItemShape)
                .contains(&wire),
            "the wire's stroke does not reach it"
        );
        assert!(
            scene
                .colliding_items_with(beside, ItemSelectionMode::IntersectsItemBoundingRect)
                .contains(&wire),
            "but the wire's bounding rect does — the cheap mode, by name"
        );
    }

    #[test]
    fn an_entry_box_always_mirrors_the_items_own_box() {
        // M2 / study 1.9: the rectangle the spatial index buckets on is the
        // entry's `local_bounds`, and it is read back from the item after
        // every write. A `PathItem` fits its geometry to the request and
        // re-derives its box from the result, so the index can never bucket a
        // rectangle the geometry has not occupied.
        let mut scene = Scene::new();
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        let id = scene.add_item(
            crate::items::PathItem::new(path).stroke(teksilo_tokens::Color::BLACK, 4.0),
            Point::ZERO,
        );
        for request in [
            Rect::new(0.0, 0.0, 200.0, 20.0),
            Rect::new(-50.0, -50.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, 0.0, 0.0),
        ] {
            scene.set_local_bounds(id, request);
            let entry_box = scene.local_bounds(id).expect("known id");
            let item_box = scene.item(id).expect("lightweight").local_bounds();
            assert_eq!(entry_box, item_box, "after requesting {request:?}");
            let shape_box = scene.item_shape(id).expect("has a shape").bounding_rect();
            assert!(
                entry_box.x <= shape_box.x + 1e-3
                    && entry_box.y <= shape_box.y + 1e-3
                    && entry_box.right() >= shape_box.right() - 1e-3
                    && entry_box.bottom() >= shape_box.bottom() - 1e-3,
                "the index box {entry_box:?} must enclose the shape {shape_box:?}"
            );
        }
    }

    #[test]
    fn a_derived_box_lands_on_the_request_and_stays_there() {
        // The setter used to aim the *box* at the request and then re-add the
        // band, overshooting by the band every time: three identical calls
        // gave three different boxes, none of them the one asked for, and each
        // emitted a change and re-bucketed the index. It aims at the geometry
        // now, so one call lands it and the next two are no-ops.
        use std::cell::RefCell;
        use std::rc::Rc;
        let mut scene = Scene::new();
        let mut path = Path::new();
        path.move_to(Point::ZERO).line_to(Point::new(100.0, 0.0));
        let id = scene.add_item(
            crate::items::PathItem::new(path).stroke(teksilo_tokens::Color::BLACK, 4.0),
            Point::ZERO,
        );
        assert_eq!(
            scene.local_bounds(id),
            Some(Rect::new(-4.0, -4.0, 108.0, 8.0))
        );

        let seen: Rc<RefCell<Vec<Rect>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let _h = scene.item_change_signal().observe(move |c| {
            if let ItemChange::LocalBoundsChanged { new, .. } = &c.change {
                sink.borrow_mut().push(*new);
            }
        });

        let request = Rect::new(0.0, 0.0, 200.0, 20.0);
        for attempt in 0..3 {
            scene.set_local_bounds(id, request);
            let got = scene.local_bounds(id).expect("known id");
            assert!(
                (got.x - request.x).abs() < 1e-3 && (got.width - request.width).abs() < 1e-3,
                "attempt {attempt}: x/width must be the request, got {got:?}"
            );
            // The path is perfectly horizontal, so its height is the band's
            // and no request can stretch it. Honest, and stated on the setter.
            assert!(
                (got.height - 8.0).abs() < 1e-3,
                "attempt {attempt}: {got:?}"
            );
        }
        assert_eq!(
            seen.borrow().len(),
            1,
            "three identical requests, one change: {:?}",
            seen.borrow()
        );
    }

    #[test]
    fn a_derived_box_honours_both_axes_when_the_geometry_has_both() {
        let mut scene = Scene::new();
        let mut path = Path::new();
        path.move_to(Point::ZERO)
            .line_to(Point::new(100.0, 40.0))
            .line_to(Point::new(0.0, 40.0))
            .close();
        let id = scene.add_item(
            crate::items::PathItem::new(path).stroke(teksilo_tokens::Color::BLACK, 6.0),
            Point::ZERO,
        );
        let request = Rect::new(-20.0, 5.0, 300.0, 120.0);
        scene.set_local_bounds(id, request);
        let got = scene.local_bounds(id).expect("known id");
        assert!(
            (got.x - request.x).abs() < 1e-2
                && (got.y - request.y).abs() < 1e-2
                && (got.width - request.width).abs() < 1e-2
                && (got.height - request.height).abs() < 1e-2,
            "asked {request:?}, settled on {got:?}"
        );
        // And it is a fixed point.
        scene.set_local_bounds(id, request);
        assert_eq!(scene.local_bounds(id), Some(got));
    }

    #[test]
    fn region_queries_decline_a_screen_anchored_item_exactly_as_point_queries_do() {
        // D5: the "declines to guess" rule applied to one query family only
        // was the inconsistency. A screen-pinned item's `local_bounds` is a
        // screen rectangle, so the scene AABB the index holds for it is a
        // fiction, and a scene-space band comparing itself against that
        // fiction is guessing exactly as `item_at` refused to.
        let mut scene = Scene::new();
        let pinned = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(50.0, 50.0),
        );
        let normal = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(50.0, 50.0),
        );
        let everything = SceneRegion::rect(Rect::new(-1000.0, -1000.0, 2000.0, 2000.0));
        assert!(
            scene
                .items_in_region(&everything, ItemSelectionMode::default(), 1.0)
                .contains(&pinned),
            "selectable while it is an ordinary item"
        );
        scene.set_flag(pinned, ItemFlags::IGNORES_TRANSFORMATIONS, true);
        for mode in [
            ItemSelectionMode::IntersectsItemShape,
            ItemSelectionMode::ContainsItemShape,
            ItemSelectionMode::IntersectsItemBoundingRect,
            ItemSelectionMode::ContainsItemBoundingRect,
        ] {
            let hits = scene.items_in_region(&everything, mode, 1.0);
            assert!(!hits.contains(&pinned), "{mode:?} guessed at a pinned item");
            assert!(hits.contains(&normal), "{mode:?} lost an ordinary one");
        }
        assert_eq!(scene.item_at(Point::new(55.0, 55.0)), Some(normal));
        // The collision family is built on the same query, from both ends.
        assert!(scene.item_region(pinned).is_none());
        assert!(scene.colliding_items(pinned).is_empty());
        assert!(!scene.colliding_items(normal).contains(&pinned));
    }

    #[test]
    fn a_plain_items_box_is_still_exactly_what_was_asked_for() {
        // The read-back must not change anything for an item that stores its
        // bounds verbatim, which is every built-in but `PathItem`.
        let mut scene = Scene::new();
        let id = scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        scene.set_local_bounds(id, Rect::new(3.0, 4.0, 50.0, 60.0));
        assert_eq!(
            scene.local_bounds(id),
            Some(Rect::new(3.0, 4.0, 50.0, 60.0))
        );
    }

    #[test]
    fn a_heavyweight_entry_has_a_defined_shape() {
        // `Scene::item` returns None for a widget entry, so every shape-mode
        // query would silently drop the tier the flagship corkboard's cards
        // live in unless the shape is defined for it. It is: its box.
        let mut scene = Scene::new();
        let card = scene.add_widget(FillWidget::new(), Rect::new(10.0, 10.0, 100.0, 60.0));
        assert!(scene.item(card).is_none());
        let shape = scene.item_shape(card).expect("a widget entry has a shape");
        assert_eq!(shape.bounding_rect(), Rect::new(0.0, 0.0, 100.0, 60.0));
        assert!(scene.item_contains(card, Point::new(50.0, 40.0), 1.0));
        assert!(!scene.item_contains(card, Point::new(500.0, 40.0), 1.0));
    }

    #[test]
    fn a_rotated_heavyweight_card_selects_the_same_in_both_shape_and_box_modes() {
        // B1 + C1: the corkboard's primary objects are heavyweight, and this
        // pins that rotating one does not quietly change what a marquee
        // enclosing it picks up. (The two modes disagree only at the margin —
        // see `the_two_spaces_differ_for_a_rotated_item`.)
        let mut scene = Scene::new();
        let card = scene.add_widget(FillWidget::new(), Rect::new(100.0, 100.0, 80.0, 40.0));
        let region = SceneRegion::rect(Rect::new(0.0, 0.0, 400.0, 400.0));
        for mode in [
            ItemSelectionMode::IntersectsItemShape,
            ItemSelectionMode::IntersectsItemBoundingRect,
            ItemSelectionMode::ContainsItemShape,
            ItemSelectionMode::ContainsItemBoundingRect,
        ] {
            assert!(
                scene.items_in_region(&region, mode, 1.0).contains(&card),
                "before rotation, {mode:?}"
            );
        }
        scene.set_transform(card, Transform2D::rotate(0.4));
        for mode in [
            ItemSelectionMode::IntersectsItemShape,
            ItemSelectionMode::IntersectsItemBoundingRect,
            ItemSelectionMode::ContainsItemShape,
            ItemSelectionMode::ContainsItemBoundingRect,
        ] {
            assert!(
                scene.items_in_region(&region, mode, 1.0).contains(&card),
                "after rotation, {mode:?}"
            );
        }
    }

    #[test]
    fn the_two_spaces_differ_for_a_rotated_item() {
        // C1, stated as a test rather than as a claim: a shape query maps the
        // region into the item's own frame, a bounding-rect query compares the
        // enlarged axis-aligned hull in scene space. A band that clips only
        // the hull's corner therefore picks the item in box mode and not in
        // shape mode.
        let mut scene = Scene::new();
        let id = scene.add_item(
            RectItem::new(Rect::new(-50.0, -10.0, 100.0, 20.0)),
            Point::new(100.0, 100.0),
        );
        scene.set_transform(id, Transform2D::rotate(std::f32::consts::FRAC_PI_4));
        let hull = scene.scene_rect(id).expect("has a scene rect");
        // A 4x4 band on the hull's top-left corner: inside the hull, outside
        // the rotated bar.
        let corner = SceneRegion::rect(Rect::new(hull.x, hull.y, 4.0, 4.0));
        assert!(
            scene
                .items_in_region(&corner, ItemSelectionMode::IntersectsItemBoundingRect, 1.0)
                .contains(&id),
            "the hull's corner is in the hull"
        );
        assert!(
            !scene
                .items_in_region(&corner, ItemSelectionMode::IntersectsItemShape, 1.0)
                .contains(&id),
            "but it is not on the bar"
        );
    }

    #[test]
    fn item_at_declines_to_place_a_screen_anchored_item() {
        // Its local coordinates are screen coordinates, so a scene-space point
        // cannot place it. `item_at_in_view` can, and does.
        let mut scene = Scene::new();
        let pinned = scene.add_item(
            RectItem::new(Rect::new(0.0, 0.0, 20.0, 20.0)),
            Point::new(50.0, 50.0),
        );
        scene.set_flag(pinned, ItemFlags::IGNORES_TRANSFORMATIONS, true);
        assert_eq!(scene.item_at(Point::new(55.0, 55.0)), None);
        // Under an identity view its anchor is (50, 50), so a screen point
        // 5 px in lands on it.
        assert_eq!(
            scene.item_at_in_view(Point::new(55.0, 55.0), Transform2D::identity()),
            Some(pinned)
        );
        // Pan the view: the pinned item follows its anchor's projection.
        let panned = Transform2D::translate(200.0, 0.0);
        assert_eq!(
            scene.item_at_in_view(Point::new(255.0, 55.0), panned),
            Some(pinned)
        );
    }

    #[test]
    fn item_at_scaled_agrees_with_dispatch_about_a_cosmetic_stroke() {
        // The mismatch nobody had written down: `item_at` passed no view
        // scale while `SceneView` dispatch passed the live one, so the two
        // disagreed about a cosmetic band at any zoom but 1.
        let mut scene = Scene::new();
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.line_to(Point::new(100.0, 0.0));
        let wire = scene.add_item(
            crate::items::PathItem::new(path).stroke_cosmetic(teksilo_tokens::Color::BLACK, 4.0),
            Point::ZERO,
        );
        let p = Point::new(50.0, 3.0);
        assert_eq!(scene.item_at_scaled(p, 1.0), Some(wire));
        assert_eq!(scene.item_at_scaled(p, 4.0), None);
        assert_eq!(scene.item_at(p), scene.item_at_scaled(p, 1.0));
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

    // ------------------------------------------------- parent ⇄ children

    /// The invariant the kept adjacency stands on. Every downward walk in this
    /// file — the index re-bucket, `collect_descendants`, and through them
    /// removal, drag-group moves and the view's snapshot patch — reads
    /// `children`, so a list that drifts from the parent pointers is not a
    /// slow scene, it is a wrong one.
    fn assert_parent_and_children_agree(scene: &Scene, what: &str) {
        for entry in &scene.entries {
            let mut seen = std::collections::HashSet::new();
            for child in &entry.children {
                assert!(
                    seen.insert(*child),
                    "{what}: {:?} lists {:?} twice",
                    entry.id,
                    child
                );
                assert_eq!(
                    scene.parent_of(*child),
                    Some(entry.id),
                    "{what}: {:?} lists {:?}, which does not point back",
                    entry.id,
                    child
                );
            }
        }
        for entry in &scene.entries {
            let Some(parent) = entry.parent else { continue };
            let pos = scene.entry_index[&parent];
            assert!(
                scene.entries[pos].children.contains(&entry.id),
                "{what}: {:?} points at {:?}, which does not list it",
                entry.id,
                parent
            );
        }
    }

    fn unit(scene: &mut Scene, at: Point) -> ItemId {
        scene.add_item(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), at)
    }

    #[test]
    fn parent_and_children_agree_through_every_door() {
        let mut scene = Scene::new();
        let a = unit(&mut scene, Point::ZERO);
        let b = unit(&mut scene, Point::new(10.0, 0.0));
        let c = unit(&mut scene, Point::new(20.0, 0.0));
        let d = unit(&mut scene, Point::new(30.0, 0.0));
        assert_parent_and_children_agree(&scene, "after insert");

        scene.set_item_parent(b, Some(a));
        scene.set_item_parent(c, Some(b));
        scene.set_item_parent(d, Some(b));
        assert_parent_and_children_agree(&scene, "after parenting");

        // Re-parent: the old list must lose it and the new one gain it.
        scene.set_item_parent(d, Some(a));
        assert_parent_and_children_agree(&scene, "after re-parenting");
        assert_eq!(scene.entries[scene.entry_index[&b]].children, vec![c]);

        // Rejected by the cycle guard — and must leave the adjacency alone.
        scene.set_item_parent(a, Some(c));
        assert_parent_and_children_agree(&scene, "after a rejected re-parent");
        assert_eq!(scene.parent_of(a), None);

        scene.orphan(a);
        assert_parent_and_children_agree(&scene, "after orphan");
        assert!(scene.entries[scene.entry_index[&a]].children.is_empty());

        scene.set_item_parent(c, Some(b));
        scene.remove(b);
        assert_parent_and_children_agree(&scene, "after removing a subtree");
        assert_eq!(scene.scene_rect(c), None, "the subtree went with it");
    }

    /// A removed child must not stay in its surviving parent's list: the next
    /// move would walk into an entry that no longer exists.
    #[test]
    fn removing_a_child_unlinks_it_from_a_surviving_parent() {
        let mut scene = Scene::new();
        let parent = unit(&mut scene, Point::ZERO);
        let keep = unit(&mut scene, Point::new(10.0, 0.0));
        let drop = unit(&mut scene, Point::new(20.0, 0.0));
        scene.set_item_parent(keep, Some(parent));
        scene.set_item_parent(drop, Some(parent));

        scene.remove(drop);
        assert_parent_and_children_agree(&scene, "after removing one child");
        assert_eq!(
            scene.entries[scene.entry_index[&parent]].children,
            vec![keep]
        );

        // The walk still reaches the survivor.
        scene.set_local_pos(parent, Point::new(100.0, 0.0));
        assert_eq!(scene.scene_pos(keep), Some(Point::new(110.0, 0.0)));
        // Starts past the parent's own box (100,0)–(110,10), so only the
        // survivor's re-bucketed rect (110,0)–(120,10) can answer.
        assert_eq!(
            scene.items_in_rect(Rect::new(112.0, -5.0, 20.0, 20.0)),
            vec![keep]
        );
    }

    /// `orphan` bypasses `set_item_parent`, so it has to maintain the adjacency
    /// itself — and a detached child must stop following its old parent.
    #[test]
    fn an_orphaned_child_stops_following_its_old_parent() {
        let mut scene = Scene::new();
        let parent = unit(&mut scene, Point::ZERO);
        let child = unit(&mut scene, Point::new(10.0, 0.0));
        scene.set_item_parent(child, Some(parent));
        scene.orphan(parent);

        scene.set_local_pos(parent, Point::new(500.0, 0.0));
        assert_eq!(
            scene.scene_pos(child),
            Some(Point::new(10.0, 0.0)),
            "an orphaned child keeps its own frame",
        );
        assert_eq!(
            scene.items_in_rect(Rect::new(5.0, -5.0, 20.0, 20.0)),
            vec![child],
            "…and the spatial index agrees",
        );
    }

    /// The re-bucket walks down the kept adjacency, so a *re-parented*
    /// grandchild must still follow a move of the new root. A `children` list
    /// updated on one side only would leave the index — and therefore every
    /// rect query and the view's cull — pointing at the old place.
    #[test]
    fn a_reparented_subtree_still_rebuckets_on_a_move() {
        let mut scene = Scene::new();
        let a = unit(&mut scene, Point::ZERO);
        let b = unit(&mut scene, Point::new(10.0, 0.0));
        let leaf = unit(&mut scene, Point::new(20.0, 0.0));
        scene.set_item_parent(leaf, Some(a));
        // …and then moved across to `b`.
        scene.set_item_parent(leaf, Some(b));

        scene.set_local_pos(b, Point::new(200.0, 0.0));
        assert_eq!(scene.scene_pos(leaf), Some(Point::new(220.0, 0.0)));
        assert_eq!(
            scene.items_in_rect(Rect::new(215.0, -5.0, 20.0, 20.0)),
            vec![leaf],
            "the index followed the re-parented leaf",
        );
        // And the old parent no longer drags it.
        scene.set_local_pos(a, Point::new(-500.0, 0.0));
        assert_eq!(scene.scene_pos(leaf), Some(Point::new(220.0, 0.0)));
    }

    /// `collect_descendants` reports each parent before its own children, which
    /// is what makes `remove`'s reversal a leaves-first order.
    #[test]
    fn collect_descendants_reports_parents_before_children() {
        let mut scene = Scene::new();
        let root = unit(&mut scene, Point::ZERO);
        let mid = unit(&mut scene, Point::ZERO);
        let leaf = unit(&mut scene, Point::ZERO);
        let sibling = unit(&mut scene, Point::ZERO);
        scene.set_item_parent(mid, Some(root));
        scene.set_item_parent(leaf, Some(mid));
        scene.set_item_parent(sibling, Some(root));

        let mut out = Vec::new();
        scene.collect_descendants(root, &mut out);
        assert_eq!(out.len(), 3);
        let pos = |id: ItemId| out.iter().position(|o| *o == id).expect("present");
        assert!(
            pos(mid) < pos(leaf),
            "a parent is reported before its child"
        );
        assert!(out.contains(&sibling));
        assert!(!out.contains(&root), "the id itself is not a descendant");
    }

    /// The side lists `SceneView::build` reads are kept, so they must survive
    /// the mutators that change what is in them — in entry order, which the
    /// view relies on for child ordering.
    #[test]
    fn the_heavyweight_and_dynamic_side_lists_track_the_entries() {
        let mut scene = Scene::new();
        let w1 = scene.add_widget(FillWidget::new(), Rect::ZERO);
        let light = unit(&mut scene, Point::ZERO);
        let w2 = scene.add_widget(FillWidget::new(), Rect::ZERO);
        assert_eq!(scene.heavyweight_ids(), vec![w1, w2], "in entry order");

        scene.remove(w1);
        assert_eq!(scene.heavyweight_ids(), vec![w2]);
        scene.remove(light);
        assert_eq!(scene.heavyweight_ids(), vec![w2]);

        // A dynamic item is discovered by the refresh through the same route.
        let dynamic =
            scene.add_item_dynamic(RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)), Point::ZERO);
        assert_eq!(scene.dynamic, vec![dynamic]);
        scene.remove(dynamic);
        assert!(scene.dynamic.is_empty());
        assert!(!scene.refresh_dynamic_bounds(), "nothing dynamic is left");
    }
}
