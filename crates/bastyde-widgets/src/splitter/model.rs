//! [`SplitterModel`] — the shared, cloneable, serializable state behind a
//! [`Splitter`](crate::splitter::Splitter).
//!
//! Mirrors the `SceneModel = Rc<RefCell<…>>` handle pattern: cloning a
//! `SplitterModel` produces a **second handle to the same data**, so the
//! app keeps a clone to read/mutate/persist while the widget renders it,
//! and a future `DockingLayout` composes a tree of them. Every mutator
//! takes `&self`, borrows the inner `RefCell` mutably, mutates, drops the
//! borrow, then bumps a `version: Signal<u64>` — the widget binds that
//! signal at `BindingLevel::Relayout`, so any external change reflows the
//! panes with no rebuild.
//!
//! ## Source of truth: pixel sizes
//!
//! Each pane stores an absolute `stored_size` (logical px along the main
//! axis). This is the user's intent. The widget projects it onto the
//! current bounds every layout pass via the pure
//! [`distribute`](super::distribute::distribute) function; a container
//! resize never writes back, so drag positions survive resizes. Stored
//! sizes change **only** on drag, programmatic mutation, or structural
//! insert/remove.
//!
//! ## Borrow / observer contract
//!
//! `version.set` snapshots its observers and releases the signal's cell
//! before invoking them, and every mutator drops its `RefCell` borrow
//! before bumping. So the one rule (same as `SceneModel`) is: **an
//! observer on [`version`](SplitterModel::version) must not mutate the
//! model re-entrantly from inside its own callback.** The widget's
//! observers only read the model and set their own signals, so they are
//! safe.

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_core::signal::Signal;
use bastyde_settings::Versioned;
use bastyde_tokens::Orientation;
use serde::{Deserialize, Serialize};

/// Default gutter (handle) thickness in logical px. Resolved into the
/// model at construction; override with [`SplitterModel::set_gutter_thickness`].
pub const SPLITTER_GUTTER_THICKNESS: f32 = 6.0;
/// Default minimum pane size in logical px.
pub const SPLITTER_MIN_PANE_SIZE: f32 = 96.0;
/// Default keyboard resize step in logical px (per arrow press).
pub const SPLITTER_KEYBOARD_STEP: f32 = 24.0;
/// Default drag-past-min snap-to-collapse threshold in logical px.
pub const SPLITTER_SNAP_OFFSET: f32 = 30.0;

// ---------------------------------------------------------------------
// Pane descriptor (construction-time per-pane config)
// ---------------------------------------------------------------------

/// Per-pane configuration passed to [`SplitterModel::from_panes`] /
/// [`SplitterModel::insert_pane`]. Public fields + [`Default`] so it can
/// be built with struct-literal `..Default::default()` syntax, or via the
/// fluent setters.
#[derive(Debug, Clone)]
pub struct PaneDescriptor {
    /// Initial main-axis size in px. `None` ⇒ take an equal share (the
    /// first layout equalizes via the stretch path).
    pub initial_size: Option<f32>,
    /// Hard compression floor in px.
    pub min_size: f32,
    /// Optional growth ceiling in px.
    pub max_size: Option<f32>,
    /// Container-resize slack weight (Qt `setStretchFactor`). `0.0` ⇒
    /// rigid (keeps its size on resize); `>0` ⇒ absorbs slack ∝ weight.
    pub stretch: f32,
    /// Whether the user may collapse this pane (drag-snap / double-click /
    /// keyboard). Programmatic [`set_collapsed`](SplitterModel::set_collapsed)
    /// ignores this flag (it governs *interactive* collapse only, like Qt's
    /// `childrenCollapsible`).
    pub collapsible: bool,
    /// Initial collapsed state.
    pub collapsed: bool,
    /// The main-axis size a collapsed pane folds down to (default `0` ⇒ fully
    /// gone). Set this to keep a sliver visible while collapsed — e.g. an
    /// accordion's header height, so the pane shrinks to just its header and
    /// can be re-expanded from there. The pane restores to its prior size on
    /// expand regardless.
    pub collapsed_size: f32,
    /// Whether the pane is present at all. Unlike `collapsed` (which folds
    /// the pane but keeps its grabbable gutter), a hidden pane removes both
    /// the pane *and* an adjacent gutter from the layout — it reads as
    /// absent. Toggled reactively via
    /// [`set_pane_visible`](SplitterModel::set_pane_visible).
    pub visible: bool,
}

impl Default for PaneDescriptor {
    fn default() -> Self {
        Self {
            initial_size: None,
            min_size: SPLITTER_MIN_PANE_SIZE,
            max_size: None,
            stretch: 1.0,
            collapsible: false,
            collapsed: false,
            collapsed_size: 0.0,
            visible: true,
        }
    }
}

impl PaneDescriptor {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn size(mut self, size: f32) -> Self {
        self.initial_size = Some(size);
        self
    }
    pub fn min_size(mut self, min: f32) -> Self {
        self.min_size = min;
        self
    }
    pub fn max_size(mut self, max: f32) -> Self {
        self.max_size = Some(max);
        self
    }
    pub fn stretch(mut self, stretch: f32) -> Self {
        self.stretch = stretch;
        self
    }
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = collapsible;
        self
    }
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
    /// Size a collapsed pane folds down to (default `0`). See
    /// [`collapsed_size`](Self::collapsed_size).
    pub fn collapsed_size(mut self, px: f32) -> Self {
        self.collapsed_size = px.max(0.0);
        self
    }
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

// ---------------------------------------------------------------------
// Internal pane entry + immutable snapshot for the sizing engine
// ---------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PaneEntry {
    stored_size: f32,
    min_size: f32,
    max_size: Option<f32>,
    stretch: f32,
    collapsible: bool,
    collapsed: bool,
    collapsed_size: f32,
    visible: bool,
}

impl PaneEntry {
    fn from_descriptor(d: &PaneDescriptor, fallback_size: f32) -> Self {
        let min = d.min_size.max(0.0);
        // Enforce max ≥ min on the way in so `distribute` never has to
        // resolve an impossible [min,max].
        let max = d.max_size.map(|m| m.max(min));
        let stored = d.initial_size.unwrap_or(fallback_size).max(0.0);
        Self {
            stored_size: stored,
            min_size: min,
            max_size: max,
            stretch: d.stretch.max(0.0),
            collapsible: d.collapsible,
            collapsed: d.collapsed,
            collapsed_size: d.collapsed_size.max(0.0),
            visible: d.visible,
        }
    }
}

/// Immutable per-pane view handed to the pure
/// [`distribute`](super::distribute::distribute) sizing function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneSnapshot {
    pub stored_size: f32,
    pub min_size: f32,
    pub max_size: Option<f32>,
    pub stretch: f32,
    pub collapsed: bool,
    pub collapsed_size: f32,
    pub visible: bool,
}

// ---------------------------------------------------------------------
// Serde DTO (export / import — the persistence surface)
// ---------------------------------------------------------------------

/// Persistable per-pane layout state. Captures the user-controllable
/// values (size + collapsed); structural config (min/max/stretch/
/// collapsible) is app-declared and not serialized — Qt `saveState`
/// parity.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PaneState {
    pub stored_size: f32,
    pub collapsed: bool,
}

/// Full serializable snapshot of a [`SplitterModel`]'s sizes + collapsed
/// flags. Round-trips through [`SplitterModel::export_state`] /
/// [`import_state`](SplitterModel::import_state) and implements
/// [`Versioned`] so apps persist it through
/// `SettingsFile<SplitterState>` + `Migrator` (TOML).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SplitterState {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub panes: Vec<PaneState>,
}

fn default_version() -> u32 {
    SplitterState::CURRENT_VERSION
}

impl Default for SplitterState {
    fn default() -> Self {
        Self {
            version: SplitterState::CURRENT_VERSION,
            panes: Vec::new(),
        }
    }
}

impl Versioned for SplitterState {
    const CURRENT_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

// ---------------------------------------------------------------------
// The model handle
// ---------------------------------------------------------------------

struct SplitterModelInner {
    panes: Vec<PaneEntry>,
    orientation: Orientation,
    gutter_thickness: f32,
    keyboard_step_px: f32,
    snap_offset: f32,
    version: Signal<u64>,
    /// `true` ⇒ the next collapse-flag change should *animate*; `false`
    /// ⇒ snap instantly (drag-driven). Read-and-reset by the widget's
    /// collapse effect via [`consume_animate_flag`](SplitterModel::consume_animate_flag).
    animate_next_collapse: bool,
}

/// A shared, cloneable handle to a splitter's layout state. `Clone` =
/// share-by-handle (cheap `Rc` bump).
pub struct SplitterModel(Rc<RefCell<SplitterModelInner>>);

impl Clone for SplitterModel {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for SplitterModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_borrow() {
            Ok(inner) => f
                .debug_struct("SplitterModel")
                .field("handles", &Rc::strong_count(&self.0))
                .field("panes", &inner.panes.len())
                .field("orientation", &inner.orientation)
                .finish(),
            Err(_) => f
                .debug_struct("SplitterModel")
                .field("handles", &Rc::strong_count(&self.0))
                .field("panes", &"<borrowed>")
                .finish(),
        }
    }
}

impl SplitterModel {
    // ---- Construction -------------------------------------------------

    /// `n` equal-share panes (each `stretch = 1`, `min = SPLITTER_MIN_PANE_SIZE`).
    pub fn new(n: usize, orientation: Orientation) -> Self {
        let panes = (0..n)
            .map(|_| PaneEntry::from_descriptor(&PaneDescriptor::default(), 0.0))
            .collect();
        Self::from_inner(panes, orientation)
    }

    /// Build from explicit per-pane descriptors.
    pub fn from_panes(panes: Vec<PaneDescriptor>, orientation: Orientation) -> Self {
        let entries = panes
            .iter()
            .map(|d| PaneEntry::from_descriptor(d, d.initial_size.unwrap_or(0.0)))
            .collect();
        Self::from_inner(entries, orientation)
    }

    fn from_inner(panes: Vec<PaneEntry>, orientation: Orientation) -> Self {
        Self(Rc::new(RefCell::new(SplitterModelInner {
            panes,
            orientation,
            gutter_thickness: SPLITTER_GUTTER_THICKNESS,
            keyboard_step_px: SPLITTER_KEYBOARD_STEP,
            snap_offset: SPLITTER_SNAP_OFFSET,
            version: Signal::new(0),
            animate_next_collapse: true,
        })))
    }

    /// Number of distinct handles to this model (1 = unshared).
    pub fn handle_count(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    // ---- Version bump -------------------------------------------------

    fn bump_version(&self) {
        // Clone the signal out and drop the borrow before `set`, so an
        // observer may safely read the model from its callback.
        let version = self.0.borrow().version.clone();
        version.set(version.get().wrapping_add(1));
    }

    // ---- Per-pane size mutators --------------------------------------

    pub fn set_stored_size(&self, index: usize, size: f32) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            p.stored_size = size.max(0.0);
        }
        self.bump_version();
    }

    /// Like [`set_stored_size`](Self::set_stored_size) but **without** a version
    /// bump — for writes made from inside a layout/effect pass that is already
    /// relaying out (e.g. capturing the displayed size as the collapse
    /// reference), where a bump would re-enter the effect.
    pub fn set_stored_size_silent(&self, index: usize, size: f32) {
        let mut inner = self.0.borrow_mut();
        if let Some(p) = inner.panes.get_mut(index) {
            p.stored_size = size.max(0.0);
        }
    }

    /// Set both sides of handle `index` (panes `index` and `index+1`) in
    /// one mutation — a single version bump, so a drag produces exactly
    /// one relayout per move.
    pub fn set_pair_sizes(&self, index: usize, size_a: f32, size_b: f32) {
        {
            let mut inner = self.0.borrow_mut();
            if index + 1 >= inner.panes.len() {
                return;
            }
            inner.panes[index].stored_size = size_a.max(0.0);
            inner.panes[index + 1].stored_size = size_b.max(0.0);
        }
        self.bump_version();
    }

    pub fn set_min_size(&self, index: usize, min: f32) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            p.min_size = min.max(0.0);
            // Keep max ≥ min.
            if let Some(m) = p.max_size {
                p.max_size = Some(m.max(p.min_size));
            }
        }
        self.bump_version();
    }

    pub fn set_max_size(&self, index: usize, max: Option<f32>) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            p.max_size = max.map(|m| m.max(p.min_size));
        }
        self.bump_version();
    }

    pub fn set_stretch(&self, index: usize, stretch: f32) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            p.stretch = stretch.max(0.0);
        }
        self.bump_version();
    }

    pub fn set_collapsible(&self, index: usize, collapsible: bool) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            p.collapsible = collapsible;
        }
        self.bump_version();
    }

    // ---- Collapse mutators -------------------------------------------

    /// Programmatically collapse/expand pane `index`, *animated*. Ignores
    /// the `collapsible` flag (that flag only gates interactive triggers).
    pub fn set_collapsed(&self, index: usize, collapsed: bool) {
        self.set_collapsed_inner(index, collapsed, true);
    }

    /// Collapse/expand pane `index` *instantly* (no tween). Used by the
    /// drag handlers — the pointer is already the motion.
    pub fn set_collapsed_immediate(&self, index: usize, collapsed: bool) {
        self.set_collapsed_inner(index, collapsed, false);
    }

    /// Toggle pane `index`'s collapsed state, animated.
    pub fn toggle_collapsed(&self, index: usize) {
        let current = self.is_collapsed(index);
        self.set_collapsed(index, !current);
    }

    /// Set the size pane `index` folds down to when collapsed (default `0`).
    /// See [`PaneDescriptor::collapsed_size`]. No version bump on its own — it
    /// only affects the next collapse.
    pub fn set_collapsed_size(&self, index: usize, px: f32) {
        let mut inner = self.0.borrow_mut();
        if let Some(p) = inner.panes.get_mut(index) {
            p.collapsed_size = px.max(0.0);
        }
    }

    fn set_collapsed_inner(&self, index: usize, collapsed: bool, animate: bool) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            if p.collapsed == collapsed {
                return; // no-op — avoid a spurious version bump
            }
            p.collapsed = collapsed;
            inner.animate_next_collapse = animate;
        }
        self.bump_version();
    }

    /// Show or hide pane `index` (animated). A hidden pane removes both the
    /// pane and an adjacent gutter from the layout — it reads as absent,
    /// unlike a collapsed pane (which keeps its grabbable gutter). The pane
    /// must be pre-mounted in the `Splitter`; this is the reactive "add /
    /// remove a pane from a fixed set" trick (no rebuild).
    pub fn set_pane_visible(&self, index: usize, visible: bool) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            if p.visible == visible {
                return;
            }
            p.visible = visible;
            inner.animate_next_collapse = true;
        }
        self.bump_version();
    }

    pub fn is_pane_visible(&self, index: usize) -> bool {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.visible)
            .unwrap_or(false)
    }

    /// Read-and-reset the "animate the next collapse change?" latch. The
    /// widget's collapse effect calls this once per version bump; it
    /// resets to `true` so the default (programmatic) path animates.
    pub fn consume_animate_flag(&self) -> bool {
        let mut inner = self.0.borrow_mut();
        let f = inner.animate_next_collapse;
        inner.animate_next_collapse = true;
        f
    }

    // ---- Structural mutators -----------------------------------------

    /// Insert a pane at `index` (clamped to `[0, len]`). A `None`
    /// `initial_size` takes the average of the existing panes' sizes; the
    /// next layout rebalances. The app must rebuild the `Splitter` widget
    /// to supply the new pane's content (retained-mode: changing a
    /// container's child *set* is a rebuild; the model keeps the
    /// persistent size/collapse state across it).
    pub fn insert_pane(&self, index: usize, desc: PaneDescriptor) {
        {
            let mut inner = self.0.borrow_mut();
            let idx = index.min(inner.panes.len());
            let fallback = if inner.panes.is_empty() {
                SPLITTER_MIN_PANE_SIZE
            } else {
                inner.panes.iter().map(|p| p.stored_size).sum::<f32>() / inner.panes.len() as f32
            };
            inner
                .panes
                .insert(idx, PaneEntry::from_descriptor(&desc, fallback));
        }
        self.bump_version();
    }

    /// Remove the pane at `index` (no-op if out of range). The app must
    /// rebuild the `Splitter` widget to drop the corresponding content.
    pub fn remove_pane(&self, index: usize) {
        {
            let mut inner = self.0.borrow_mut();
            if index >= inner.panes.len() {
                return;
            }
            inner.panes.remove(index);
        }
        self.bump_version();
    }

    /// Replace the metadata of pane `index` (keeps its current size unless
    /// the descriptor specifies one).
    pub fn replace_pane_desc(&self, index: usize, desc: PaneDescriptor) {
        {
            let mut inner = self.0.borrow_mut();
            let Some(p) = inner.panes.get_mut(index) else {
                return;
            };
            let fallback = p.stored_size;
            *p = PaneEntry::from_descriptor(&desc, fallback);
        }
        self.bump_version();
    }

    // ---- Global mutators ---------------------------------------------

    pub fn set_gutter_thickness(&self, thickness: f32) {
        {
            self.0.borrow_mut().gutter_thickness = thickness.max(1.0);
        }
        self.bump_version();
    }

    pub fn set_snap_offset(&self, offset: f32) {
        {
            self.0.borrow_mut().snap_offset = offset.max(0.0);
        }
        self.bump_version();
    }

    pub fn set_keyboard_step_px(&self, step: f32) {
        {
            self.0.borrow_mut().keyboard_step_px = step.max(1.0);
        }
        self.bump_version();
    }

    pub fn set_orientation(&self, orientation: Orientation) {
        {
            self.0.borrow_mut().orientation = orientation;
        }
        self.bump_version();
    }

    // ---- Queries ------------------------------------------------------

    pub fn pane_count(&self) -> usize {
        self.0.borrow().panes.len()
    }
    pub fn stored_size(&self, index: usize) -> f32 {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.stored_size)
            .unwrap_or(0.0)
    }
    pub fn min_size(&self, index: usize) -> f32 {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.min_size)
            .unwrap_or(0.0)
    }
    pub fn max_size(&self, index: usize) -> Option<f32> {
        self.0.borrow().panes.get(index).and_then(|p| p.max_size)
    }
    pub fn stretch(&self, index: usize) -> f32 {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.stretch)
            .unwrap_or(0.0)
    }
    pub fn is_collapsible(&self, index: usize) -> bool {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.collapsible)
            .unwrap_or(false)
    }
    /// The size pane `index` folds to when collapsed (default `0`). See
    /// [`PaneDescriptor::collapsed_size`].
    pub fn collapsed_size(&self, index: usize) -> f32 {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.collapsed_size)
            .unwrap_or(0.0)
    }
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.0
            .borrow()
            .panes
            .get(index)
            .map(|p| p.collapsed)
            .unwrap_or(false)
    }
    pub fn orientation(&self) -> Orientation {
        self.0.borrow().orientation
    }
    pub fn gutter_thickness(&self) -> f32 {
        self.0.borrow().gutter_thickness
    }
    pub fn snap_offset(&self) -> f32 {
        self.0.borrow().snap_offset
    }
    pub fn keyboard_step_px(&self) -> f32 {
        self.0.borrow().keyboard_step_px
    }

    /// The reactive version signal. The `Splitter` widget binds this at
    /// `BindingLevel::Relayout`.
    pub fn version(&self) -> Signal<u64> {
        self.0.borrow().version.clone()
    }

    /// Immutable per-pane snapshot for the pure sizing engine.
    pub fn pane_snapshots(&self) -> Vec<PaneSnapshot> {
        self.0
            .borrow()
            .panes
            .iter()
            .map(|p| PaneSnapshot {
                stored_size: p.stored_size,
                min_size: p.min_size,
                max_size: p.max_size,
                stretch: p.stretch,
                collapsed: p.collapsed,
                collapsed_size: p.collapsed_size,
                visible: p.visible,
            })
            .collect()
    }

    // ---- Import / export ---------------------------------------------

    /// Snapshot the per-pane sizes + collapsed flags into a serializable
    /// [`SplitterState`].
    pub fn export_state(&self) -> SplitterState {
        let inner = self.0.borrow();
        SplitterState {
            version: SplitterState::CURRENT_VERSION,
            panes: inner
                .panes
                .iter()
                .map(|p| PaneState {
                    stored_size: p.stored_size,
                    collapsed: p.collapsed,
                })
                .collect(),
        }
    }

    /// Restore sizes + collapsed flags from a [`SplitterState`]. Returns
    /// `false` (and changes nothing) if the pane count doesn't match — the
    /// structural config must be reconstructed first. Restoration is
    /// instant (collapsed panes don't animate open on load).
    pub fn import_state(&self, state: &SplitterState) -> bool {
        let ok = {
            let mut inner = self.0.borrow_mut();
            if state.panes.len() != inner.panes.len() {
                false
            } else {
                for (p, s) in inner.panes.iter_mut().zip(&state.panes) {
                    p.stored_size = s.stored_size.max(0.0);
                    p.collapsed = s.collapsed;
                }
                inner.animate_next_collapse = false;
                true
            }
        };
        if ok {
            self.bump_version();
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_shares_state() {
        let a = SplitterModel::new(3, Orientation::Horizontal);
        let b = a.clone();
        assert_eq!(b.pane_count(), 3);
        a.set_stored_size(0, 200.0);
        assert_eq!(b.stored_size(0), 200.0);
        assert_eq!(a.handle_count(), 2);
    }

    #[test]
    fn version_bumps_on_mutation() {
        let m = SplitterModel::new(2, Orientation::Horizontal);
        let v = m.version();
        let v0 = v.get();
        m.set_stored_size(0, 150.0);
        assert_ne!(v.get(), v0);
        // No-op collapse change must NOT bump.
        let v1 = v.get();
        m.set_collapsed(0, false); // already false
        assert_eq!(v.get(), v1);
    }

    #[test]
    fn export_import_round_trips() {
        let m = SplitterModel::new(3, Orientation::Horizontal);
        m.set_stored_size(0, 120.0);
        m.set_stored_size(1, 340.0);
        m.set_collapsed(2, true);
        let state = m.export_state();

        let restored = SplitterModel::new(3, Orientation::Horizontal);
        assert!(restored.import_state(&state));
        assert_eq!(restored.stored_size(0), 120.0);
        assert_eq!(restored.stored_size(1), 340.0);
        assert!(restored.is_collapsed(2));
    }

    #[test]
    fn import_rejects_pane_count_mismatch() {
        let m = SplitterModel::new(3, Orientation::Horizontal);
        let state = m.export_state();
        let two = SplitterModel::new(2, Orientation::Horizontal);
        assert!(!two.import_state(&state));
    }

    #[test]
    fn insert_remove_change_count() {
        let m = SplitterModel::new(2, Orientation::Horizontal);
        m.insert_pane(1, PaneDescriptor::new().size(100.0));
        assert_eq!(m.pane_count(), 3);
        assert_eq!(m.stored_size(1), 100.0);
        m.remove_pane(0);
        assert_eq!(m.pane_count(), 2);
    }

    #[test]
    fn max_size_enforced_ge_min() {
        let m = SplitterModel::from_panes(
            vec![PaneDescriptor::new().min_size(200.0).max_size(100.0)],
            Orientation::Horizontal,
        );
        // max was clamped up to min.
        assert_eq!(m.max_size(0), Some(200.0));
    }

    #[test]
    fn animate_flag_consumed_and_resets() {
        let m = SplitterModel::new(2, Orientation::Horizontal);
        m.set_collapsed_immediate(0, true);
        assert!(!m.consume_animate_flag()); // immediate path
        // After consuming, it defaults back to animated.
        assert!(m.consume_animate_flag());
    }
}
