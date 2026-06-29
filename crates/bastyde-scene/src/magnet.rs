// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Magnetism: typed snap-and-connect between anchor points on scene items.
//!
//! A **magnet** is a local point on an item (relative to the item's
//! anchor, like a child point), carrying a type-erased payload
//! (`'static`, downcastable) and a directional [`MagnetRole`]. An item
//! can carry several. During an interaction the scene broad-phases
//! nearby magnets, runs an accept/reject [predicate](MagnetVerdict) per
//! candidate pair, snaps so the closest accepting pair aligns, and on
//! release a connection event carries the payloads to the consumer.
//!
//! # Mechanism in scene, policy in the consumer
//!
//! This module and [`Scene`](crate::Scene) own the *mechanism*: magnet
//! geometry, broad-phase, snap math, and the connection result. They do
//! **not** own *policy* — which magnet types are compatible, what a
//! connection means, or whether a connection persists. Compatibility is
//! decided entirely by the predicate the consumer supplies to
//! [`Scene::compute_item_snap`](crate::Scene::compute_item_snap) /
//! [`Scene::compute_port_snap`](crate::Scene::compute_port_snap); the
//! meaning of a formed connection is decided by the consumer's
//! `on_connect` handler. No widget-tree or designer concept (slot,
//! category, insertion index) leaks into this API; those live in the
//! payloads and the predicate.
//!
//! [`MagnetRole`] is generic node-graph / diagram vocabulary used by the
//! scene only for default feedback (which end is the source) and for
//! ordering the keyboard connect flow. It is advisory: the predicate is
//! always authoritative on whether two magnets may connect.
//!
//! ## Example — two items connected by a typed magnet pair
//!
//! ```rust
//! use bastyde_scene::{Scene, RectItem, Magnet, MagnetRole, MagnetRef, MagnetVerdict};
//! use bastyde_canvas::{Point, Rect, Vec2};
//!
//! // A predicate that accepts Source → Target pairs on different items.
//! fn source_to_target(a: &MagnetRef, b: &MagnetRef) -> MagnetVerdict {
//!     if a.item == b.item { return MagnetVerdict::Reject; }
//!     match (a.role, b.role) {
//!         (MagnetRole::Source, MagnetRole::Target)
//!         | (MagnetRole::Target, MagnetRole::Source) => MagnetVerdict::accept(),
//!         _ => MagnetVerdict::Reject,
//!     }
//! }
//!
//! let mut scene = Scene::new();
//!
//! // Dragged item with a Source magnet at its local origin.
//! let dragged = scene.add_item(
//!     RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
//!     Point::ZERO,
//! );
//! scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));
//!
//! // Target item 100 px to the right with a Target magnet at its local origin.
//! let target = scene.add_item(
//!     RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)),
//!     Point::new(100.0, 0.0),
//! );
//! scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Target));
//!
//! // The dragged item is 5 px away from snapping; capture radius 20 px.
//! if let Some(snap) = scene.compute_item_snap(dragged, Vec2::new(95.0, 0.0), 20.0, &source_to_target) {
//!     // snap_vector carries the dragged item exactly onto the target magnet.
//!     assert!((snap.snap_vector.x - 5.0).abs() < 1e-3);
//! }
//! ```

use std::any::Any;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use bastyde_canvas::{Canvas, Point, Vec2};
use bastyde_core::event::Key;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, PaintContext};
use bastyde_i18n::LocalizedString;

use crate::item::ItemId;

/// Opaque identifier for a [`Magnet`] inside a [`Scene`](crate::Scene).
///
/// Globally unique within a process, minted by `MagnetId::next`. Stable
/// across the magnet's lifetime; removing a magnet (or its owning item)
/// retires its id permanently — ids are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MagnetId(pub(crate) u64);

impl MagnetId {
    /// Mint a fresh globally-unique id. Used internally by Scene.
    pub(crate) fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Raw numeric value, used by AccessKit's synthetic-NodeId derivation.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// The direction a magnet faces in a connection.
///
/// Advisory only — the scene uses it for default feedback (arrow
/// direction, which end starts the keyboard flow), but the
/// accept/reject predicate is always the authority on compatibility. A
/// node-graph output port is a [`Source`](MagnetRole::Source), an input
/// port is a [`Target`](MagnetRole::Target); a snap point that can be
/// either end is [`Bidirectional`](MagnetRole::Bidirectional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MagnetRole {
    /// Originates a connection (e.g. a node-graph output port).
    Source,
    /// Receives a connection (e.g. a node-graph input port).
    Target,
    /// Can be either end of a connection.
    Bidirectional,
}

/// A magnetism anchor attached to a scene item.
///
/// Built fluently and handed to
/// [`SceneModel::add_magnet`](crate::SceneModel::add_magnet). Carries a
/// local-frame position, a [`MagnetRole`], an optional type-erased
/// payload, an enabled flag, and an optional accessibility label.
#[derive(Clone)]
pub struct Magnet {
    pub(crate) local_pos: Point,
    pub(crate) role: MagnetRole,
    pub(crate) payload: Option<Rc<dyn Any>>,
    pub(crate) enabled: bool,
    pub(crate) label: Option<LocalizedString>,
}

impl std::fmt::Debug for Magnet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Magnet")
            .field("local_pos", &self.local_pos)
            .field("role", &self.role)
            .field("has_payload", &self.payload.is_some())
            .field("enabled", &self.enabled)
            .field("has_label", &self.label.is_some())
            .finish()
    }
}

impl Magnet {
    /// A magnet at `local_pos` in the owning item's local frame, role
    /// [`Bidirectional`](MagnetRole::Bidirectional), no payload, enabled.
    pub fn new(local_pos: Point) -> Self {
        Self {
            local_pos,
            role: MagnetRole::Bidirectional,
            payload: None,
            enabled: true,
            label: None,
        }
    }

    /// Set the connection direction (advisory — see [`MagnetRole`]).
    pub fn role(mut self, role: MagnetRole) -> Self {
        self.role = role;
        self
    }

    /// Attach a type-erased payload the predicate and the connection
    /// event can downcast. Cheap to carry around (held in an `Rc`).
    pub fn payload<P: 'static>(mut self, payload: P) -> Self {
        self.payload = Some(Rc::new(payload));
        self
    }

    /// Attach an already-`Rc`-wrapped payload (use when several magnets
    /// share one payload object).
    pub fn payload_rc(mut self, payload: Rc<dyn Any>) -> Self {
        self.payload = Some(payload);
        self
    }

    /// The accessibility name announced for this magnet's synthetic AT
    /// node. Defaults to a generic role-based label when unset.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Disabled magnets are skipped by broad-phase, feedback, the
    /// keyboard cycle, and AT emission. Enabled by default.
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

/// An owned, borrow-free snapshot of one magnet, handed to the
/// accept/reject predicate and carried in a [`MagnetConnection`].
///
/// The payload is an `Rc` clone, so a snapshot can outlive the borrow
/// taken to collect candidates. The predicate inspects these snapshots
/// while a shared (read-only) scene borrow is held — it may read the
/// model but must not mutate it. The `on_connect` handler, by contrast,
/// runs after every borrow is dropped and may freely mutate the model
/// (add an edge item, reparent, fire an intent).
#[derive(Clone)]
pub struct MagnetRef {
    /// The magnet's id.
    pub id: MagnetId,
    /// The item the magnet is attached to.
    pub item: ItemId,
    /// The magnet's advisory direction.
    pub role: MagnetRole,
    /// The magnet's payload, if any (an `Rc` clone of the stored one).
    pub payload: Option<Rc<dyn Any>>,
    /// The magnet's current position in scene coordinates.
    pub scene_pos: Point,
}

impl std::fmt::Debug for MagnetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MagnetRef")
            .field("id", &self.id)
            .field("item", &self.item)
            .field("role", &self.role)
            .field("has_payload", &self.payload.is_some())
            .field("scene_pos", &self.scene_pos)
            .finish()
    }
}

impl MagnetRef {
    /// Borrow the payload downcast to `P`, or `None` if absent or a
    /// different type. The ergonomic way to read a typed payload inside
    /// a predicate.
    pub fn payload_as<P: 'static>(&self) -> Option<&P> {
        self.payload.as_ref().and_then(|p| p.downcast_ref::<P>())
    }
}

/// The result of running the accept/reject predicate on a candidate
/// magnet pair. "Both payloads in, reject or accept-with-payload out."
pub enum MagnetVerdict {
    /// The pair may not connect; the scene skips it.
    Reject,
    /// The pair may connect. The optional payload is attached to the
    /// resulting [`MagnetConnection`] (e.g. a derived edge descriptor).
    Accept(Option<Rc<dyn Any>>),
}

impl MagnetVerdict {
    /// Accept with no extra connection payload.
    pub fn accept() -> Self {
        MagnetVerdict::Accept(None)
    }

    /// Accept and attach a typed connection payload.
    pub fn accept_with<P: 'static>(payload: P) -> Self {
        MagnetVerdict::Accept(Some(Rc::new(payload)))
    }

    /// Whether this verdict accepts the pair.
    pub fn is_accept(&self) -> bool {
        matches!(self, MagnetVerdict::Accept(_))
    }
}

/// A formed connection between two magnets, delivered to the consumer's
/// `on_connect` handler on release (mouse) or confirm (keyboard).
///
/// `from` is the magnet that initiated the connection (the dragged
/// item's magnet, the grabbed port, or the keyboard-activated source);
/// `to` is the magnet it connected onto. `payload` is whatever the
/// predicate's [`MagnetVerdict::Accept`] carried.
#[derive(Clone)]
pub struct MagnetConnection {
    /// The initiating magnet.
    pub from: MagnetRef,
    /// The receiving magnet.
    pub to: MagnetRef,
    /// The connection payload from the accepting verdict, if any.
    pub payload: Option<Rc<dyn Any>>,
}

impl std::fmt::Debug for MagnetConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MagnetConnection")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("has_payload", &self.payload.is_some())
            .finish()
    }
}

impl MagnetConnection {
    /// Borrow the connection payload downcast to `P`.
    pub fn payload_as<P: 'static>(&self) -> Option<&P> {
        self.payload.as_ref().and_then(|p| p.downcast_ref::<P>())
    }
}

/// The chosen snap when a dragged item's magnet aligns onto another
/// item's magnet. Returned by
/// [`Scene::compute_item_snap`](crate::Scene::compute_item_snap).
///
/// A heavyweight consumer that drives its own drag uses `snap_vector` to
/// place the item so `from` lands on `to`, and resolves `from` / `to`
/// via [`Scene::magnet`](crate::Scene::magnet) to build the connection
/// for its own `on_connect`.
#[derive(Clone)]
pub struct MagnetSnap {
    /// The dragged item's magnet that is snapping.
    pub from: MagnetId,
    /// The stationary magnet it snaps onto.
    pub to: MagnetId,
    /// Add this to the drag delta (or the item's position) so `from`'s
    /// scene position coincides with `to`'s.
    pub snap_vector: Vec2,
    /// The accepting verdict's payload, if any.
    pub payload: Option<Rc<dyn Any>>,
    /// Scene-space distance between the pair before snapping (the
    /// tie-break used to pick the closest accepting pair).
    pub distance: f32,
}

impl std::fmt::Debug for MagnetSnap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MagnetSnap")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("snap_vector", &self.snap_vector)
            .field("has_payload", &self.payload.is_some())
            .field("distance", &self.distance)
            .finish()
    }
}

/// When the [`SceneView`](crate::SceneView) paints magnet markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerVisibility {
    /// Always draw a marker for every enabled magnet (busy, but the
    /// clearest discoverability — good for a dedicated editor).
    Always,
    /// Draw markers only while an interaction is in progress (an item
    /// drag, a port drag, or keyboard connect mode). The default — keeps
    /// an idle scene clean.
    DuringInteraction,
    /// Never draw markers (the consumer paints its own via the feedback
    /// hook, or wants no visual at all).
    Never,
}

/// The visual state of a magnet as the feedback renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagnetVisualState {
    /// A normal, idle magnet.
    Idle,
    /// A magnet the current interaction could connect to (it passes the
    /// predicate against the active source).
    Candidate,
    /// The magnet the active interaction is currently snapped onto.
    Snapped,
    /// The keyboard-focused magnet (connect mode).
    Focused,
    /// The keyboard-activated source magnet awaiting a target.
    PendingSource,
}

/// One magnet's render data, handed to the feedback renderer.
#[derive(Debug, Clone, Copy)]
pub struct MagnetMarker {
    /// The magnet's id.
    pub id: MagnetId,
    /// Its current scene position.
    pub scene_pos: Point,
    /// Its advisory role.
    pub role: MagnetRole,
    /// Its visual state for this frame.
    pub state: MagnetVisualState,
}

/// Everything the magnetism feedback renderer needs for one frame, in
/// scene coordinates (the canvas is already in the view-transform
/// scope). The built-in renderer draws markers plus a connector; a
/// custom [`MagnetismConfig::feedback`] closure receives the same data.
#[derive(Debug, Clone)]
pub struct MagnetFeedback {
    /// The view's current geometric zoom, so the renderer can size
    /// constant-pixel chrome as `pixels / zoom` in scene units.
    pub zoom: f32,
    /// Eligible magnets to mark, with their per-frame state.
    pub markers: Vec<MagnetMarker>,
    /// A connector to draw between two scene points, if an interaction
    /// is forming one: the active item-drag snap pair, the port-drag
    /// wire (source to snapped target or cursor), or the keyboard
    /// preview (pending source to focused candidate). `true` in the
    /// second field marks an *accepted* connector (drawn solid /
    /// highlighted) versus a tentative one (the free port-drag wire).
    pub connector: Option<(Point, Point, bool)>,
}

/// Per-view magnetism configuration, installed via
/// [`SceneView::magnetism`](crate::SceneView::magnetism).
///
/// Holds the consumer's *policy* — the accept/reject predicate and the
/// `on_connect` handler — plus presentation knobs. The scene supplies
/// the mechanism (snap math, broad-phase, feedback rendering, the
/// connection event); this config is where the consumer plugs its
/// policy in.
#[derive(Clone)]
pub struct MagnetismConfig {
    pub(crate) predicate: Rc<dyn Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict>,
    pub(crate) on_connect: Rc<dyn Fn(&MagnetConnection, &mut EventContext)>,
    pub(crate) capture_px: f32,
    pub(crate) markers: MarkerVisibility,
    pub(crate) feedback: Option<Rc<dyn Fn(&mut Canvas, &PaintContext, &MagnetFeedback)>>,
    pub(crate) connect_key: Key,
    pub(crate) enabled: Signal<bool>,
}

impl std::fmt::Debug for MagnetismConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MagnetismConfig")
            .field("capture_px", &self.capture_px)
            .field("markers", &self.markers)
            .field("has_custom_feedback", &self.feedback.is_some())
            .field("connect_key", &self.connect_key)
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

impl MagnetismConfig {
    /// A config with the given accept/reject `predicate` and defaults:
    /// 14 px capture radius, markers during interaction, the built-in
    /// feedback renderer, `m` to toggle keyboard connect mode, enabled.
    /// Install an `on_connect` handler to actually do something on
    /// connect.
    pub fn new(predicate: impl Fn(&MagnetRef, &MagnetRef) -> MagnetVerdict + 'static) -> Self {
        Self {
            predicate: Rc::new(predicate),
            on_connect: Rc::new(|_, _| {}),
            capture_px: 14.0,
            markers: MarkerVisibility::DuringInteraction,
            feedback: None,
            connect_key: Key::Character('m'),
            enabled: Signal::new(true),
        }
    }

    /// The handler invoked when a connection is formed (mouse release or
    /// keyboard confirm). Runs with a live `EventContext` and no scene
    /// borrow held, so it may mutate the model (add an edge item,
    /// reparent), call `scene.add_a11y_relation`, or fire an intent.
    pub fn on_connect(
        mut self,
        f: impl Fn(&MagnetConnection, &mut EventContext) + 'static,
    ) -> Self {
        self.on_connect = Rc::new(f);
        self
    }

    /// Capture and grab radius in **screen pixels** (converted to scene
    /// units by dividing by the live zoom, so snapping feels consistent
    /// at any zoom). Default 14.
    pub fn capture_px(mut self, px: f32) -> Self {
        self.capture_px = px.max(0.0);
        self
    }

    /// When magnet markers are painted. Default
    /// [`MarkerVisibility::DuringInteraction`].
    pub fn markers(mut self, markers: MarkerVisibility) -> Self {
        self.markers = markers;
        self
    }

    /// Replace the built-in feedback renderer with a custom one. The
    /// closure paints in scene coordinates (the canvas already has the
    /// view transform pushed).
    pub fn feedback(
        mut self,
        f: impl Fn(&mut Canvas, &PaintContext, &MagnetFeedback) + 'static,
    ) -> Self {
        self.feedback = Some(Rc::new(f));
        self
    }

    /// The key that toggles keyboard connect mode while the SceneView is
    /// focused. Default `m`.
    pub fn connect_key(mut self, key: Key) -> Self {
        self.connect_key = key;
        self
    }

    /// Set the initial enabled state (default enabled). Replaces the
    /// internal signal.
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = Signal::new(on);
        self
    }

    /// Drive enabled/disabled from an app-owned signal (toolbar toggle).
    pub fn bind_enabled(mut self, signal: Signal<bool>) -> Self {
        self.enabled = signal;
        self
    }

    /// The reactive enabled signal, for a toolbar to read or bind.
    pub fn enabled_signal(&self) -> Signal<bool> {
        self.enabled.clone()
    }

    /// Whether magnetism is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;
    use bastyde_canvas::{Point, Transform2D};

    /// Helper: a predicate that accepts any Source -> Target pair on
    /// different items, rejecting same-role and same-item pairs.
    fn source_to_target(a: &MagnetRef, b: &MagnetRef) -> MagnetVerdict {
        if a.item == b.item {
            return MagnetVerdict::Reject;
        }
        match (a.role, b.role) {
            (MagnetRole::Source, MagnetRole::Target) | (MagnetRole::Target, MagnetRole::Source) => {
                MagnetVerdict::accept()
            }
            _ => MagnetVerdict::Reject,
        }
    }

    #[test]
    fn magnet_ids_are_unique_and_monotonic() {
        let a = MagnetId::next();
        let b = MagnetId::next();
        assert_ne!(a, b);
        assert!(b.as_u64() > a.as_u64());
    }

    #[test]
    fn magnet_builder_defaults_and_setters() {
        let m = Magnet::new(Point::new(3.0, 4.0));
        assert_eq!(m.local_pos, Point::new(3.0, 4.0));
        assert_eq!(m.role, MagnetRole::Bidirectional);
        assert!(m.enabled);
        assert!(m.payload.is_none());

        let m = Magnet::new(Point::ZERO)
            .role(MagnetRole::Source)
            .payload(42_u32)
            .enabled(false);
        assert_eq!(m.role, MagnetRole::Source);
        assert!(!m.enabled);
        assert_eq!(
            m.payload.as_ref().unwrap().downcast_ref::<u32>(),
            Some(&42_u32)
        );
    }

    #[test]
    fn verdict_helpers() {
        assert!(MagnetVerdict::accept().is_accept());
        assert!(MagnetVerdict::accept_with(7_i32).is_accept());
        assert!(!MagnetVerdict::Reject.is_accept());
    }

    // --- compute_item_snap ------------------------------------------

    #[test]
    fn item_snap_snaps_to_nearest_accepting_magnet() {
        let mut scene = Scene::new();
        // Dragged item at origin with a Source magnet at its local (0,0).
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(0.0, 0.0),
        );
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));

        // Target item at (100, 0) with a Target magnet at its local (0,0),
        // i.e. scene (100, 0).
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(100.0, 0.0),
        );
        let tmag = scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Target));

        // Drag so the source magnet sits at scene (95, 0): 5 px shy of the
        // target. With a 20 px capture radius it should snap.
        let snap = scene
            .compute_item_snap(dragged, Vec2::new(95.0, 0.0), 20.0, &source_to_target)
            .expect("expected a snap");
        assert_eq!(snap.to, tmag);
        // snap_vector should carry the source from (95,0) to (100,0): +5 x.
        assert!((snap.snap_vector.x - 5.0).abs() < 1e-3);
        assert!(snap.snap_vector.y.abs() < 1e-3);
    }

    #[test]
    fn item_snap_ignores_pairs_beyond_radius() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(100.0, 0.0),
        );
        scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Target));

        // Source dragged to (50,0): 50 px from the target, capture radius 20.
        let snap = scene.compute_item_snap(dragged, Vec2::new(50.0, 0.0), 20.0, &source_to_target);
        assert!(snap.is_none());
    }

    #[test]
    fn item_snap_respects_rejecting_predicate() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        // Both Source — source_to_target rejects same-role pairs.
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(100.0, 0.0),
        );
        scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Source));

        let snap = scene.compute_item_snap(dragged, Vec2::new(98.0, 0.0), 20.0, &source_to_target);
        assert!(snap.is_none());
    }

    #[test]
    fn item_snap_excludes_dragged_items_own_magnets() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        // Two magnets on the SAME item that would otherwise satisfy the
        // predicate (Source + Target) and be near each other.
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));
        scene.add_magnet(
            dragged,
            Magnet::new(Point::new(2.0, 0.0)).role(MagnetRole::Target),
        );

        let snap = scene.compute_item_snap(dragged, Vec2::ZERO, 20.0, &source_to_target);
        assert!(snap.is_none(), "a dragged item must not snap to itself");
    }

    #[test]
    fn item_snap_picks_global_minimum_distance() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));

        // Two candidate targets; the nearer one wins.
        let near = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(10.0, 0.0),
        );
        let near_mag = scene.add_magnet(near, Magnet::new(Point::ZERO).role(MagnetRole::Target));
        let far = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(18.0, 0.0),
        );
        scene.add_magnet(far, Magnet::new(Point::ZERO).role(MagnetRole::Target));

        // Source dragged to (8,0): 2 px from `near`, 10 px from `far`.
        let snap = scene
            .compute_item_snap(dragged, Vec2::new(8.0, 0.0), 20.0, &source_to_target)
            .expect("snap");
        assert_eq!(snap.to, near_mag);
    }

    #[test]
    fn item_snap_carries_verdict_payload() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(100.0, 0.0),
        );
        scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Target));

        let pred = |a: &MagnetRef, b: &MagnetRef| {
            if a.item != b.item {
                MagnetVerdict::accept_with(String::from("edge"))
            } else {
                let _ = b;
                MagnetVerdict::Reject
            }
        };
        let snap = scene
            .compute_item_snap(dragged, Vec2::new(98.0, 0.0), 20.0, &pred)
            .expect("snap");
        assert_eq!(
            snap.payload.as_ref().unwrap().downcast_ref::<String>(),
            Some(&String::from("edge"))
        );
    }

    #[test]
    fn item_snap_honors_item_transform() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));

        // Target item scaled 2x with a magnet at local (10,0) -> scene (20,0).
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        scene.set_transform(target, Transform2D::scale(2.0, 2.0));
        let tmag = scene.add_magnet(
            target,
            Magnet::new(Point::new(10.0, 0.0)).role(MagnetRole::Target),
        );
        // Magnet scene pos should be (20, 0).
        assert_eq!(scene.magnet_scene_pos(tmag), Some(Point::new(20.0, 0.0)));

        let snap = scene
            .compute_item_snap(dragged, Vec2::new(19.0, 0.0), 20.0, &source_to_target)
            .expect("snap");
        assert_eq!(snap.to, tmag);
        assert!((snap.snap_vector.x - 1.0).abs() < 1e-3);
    }

    // --- compute_port_snap ------------------------------------------

    #[test]
    fn port_snap_returns_nearest_accepting_target() {
        let mut scene = Scene::new();
        let src_item = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        let source = scene.add_magnet(src_item, Magnet::new(Point::ZERO).role(MagnetRole::Source));
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(50.0, 0.0),
        );
        let tmag = scene.add_magnet(target, Magnet::new(Point::ZERO).role(MagnetRole::Target));

        // Cursor near the target magnet.
        let res = scene
            .compute_port_snap(source, Point::new(52.0, 1.0), 20.0, &source_to_target)
            .expect("port snap");
        assert_eq!(res.0.id, tmag);
    }

    #[test]
    fn port_snap_excludes_source_and_rejects_far() {
        let mut scene = Scene::new();
        let src_item = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        let source = scene.add_magnet(src_item, Magnet::new(Point::ZERO).role(MagnetRole::Source));

        // No accepting target near the cursor -> None.
        let res = scene.compute_port_snap(source, Point::new(0.0, 0.0), 20.0, &source_to_target);
        assert!(res.is_none());
    }

    // --- storage & cleanup -----------------------------------------

    #[test]
    fn magnet_storage_add_remove_clear() {
        let mut scene = Scene::new();
        let item = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        let a = scene.add_magnet(item, Magnet::new(Point::ZERO));
        let b = scene.add_magnet(item, Magnet::new(Point::new(5.0, 5.0)));
        assert_eq!(scene.magnet_ids_of(item).len(), 2);

        scene.remove_magnet(a);
        assert_eq!(scene.magnet_ids_of(item), vec![b]);
        assert!(scene.magnet(a).is_none());

        scene.clear_magnets(item);
        assert!(scene.magnet_ids_of(item).is_empty());
    }

    #[test]
    fn removing_item_drops_its_magnets() {
        let mut scene = Scene::new();
        let item = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        let m = scene.add_magnet(item, Magnet::new(Point::ZERO));
        assert!(scene.magnet(m).is_some());
        scene.remove(item);
        assert!(scene.magnet(m).is_none());
        assert!(scene.magnet_ids_of(item).is_empty());
    }

    #[test]
    fn disabled_magnets_are_not_snap_candidates() {
        let mut scene = Scene::new();
        let dragged = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::ZERO,
        );
        scene.add_magnet(dragged, Magnet::new(Point::ZERO).role(MagnetRole::Source));
        let target = scene.add_item(
            crate::RectItem::new(bastyde_canvas::Rect::new(0.0, 0.0, 10.0, 10.0)),
            Point::new(100.0, 0.0),
        );
        // Disabled target magnet — should be ignored.
        scene.add_magnet(
            target,
            Magnet::new(Point::ZERO)
                .role(MagnetRole::Target)
                .enabled(false),
        );
        let snap = scene.compute_item_snap(dragged, Vec2::new(98.0, 0.0), 20.0, &source_to_target);
        assert!(snap.is_none());
    }
}
