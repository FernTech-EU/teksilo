// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`DockingModel`] — the shared, cloneable, serializable handle that backs a
//! [`DockingLayout`](super::DockingLayout). Mirrors the
//! [`SplitterModel`] / `SceneModel` pattern:
//! an `Rc<RefCell<…>>` with `&self` mutators that drop the borrow before
//! bumping `version`, so observers run with the model unlocked.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use bastyde_canvas::Size;
use bastyde_core::signal::Signal;
use bastyde_settings::Versioned;
use bastyde_i18n::LocalizedString;
use bastyde_tokens::Orientation;

use crate::primitives::IconWidget;
use crate::splitter::{PaneDescriptor, SplitterModel};

pub use super::geometry::{CornerOwners, DockCorner, DockSide};

/// Builds an icon for a dock widget's tab / rail item on demand.
pub type DockIconFactory = Rc<dyn Fn() -> IconWidget>;

/// Process-unique identity for a registered dock widget (the atomic unit).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct DockWidgetId(pub u64);

impl DockWidgetId {
    /// Mint a fresh, process-unique id.
    pub fn fresh() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
    pub fn from_raw(v: u64) -> Self {
        Self(v)
    }
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Process-unique identity for a dock tab (a tab of a side's TabWidget).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct DockTabId(pub u64);

impl DockTabId {
    pub fn fresh() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
    pub fn from_raw(v: u64) -> Self {
        Self(v)
    }
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// How a side surfaces its tabs: an in-side strip (the TabWidget's own bar) or
/// an always-visible activity rail outboard of the collapsible content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TabPresentation {
    /// In-side tab strip (hidden when a single tab is present).
    Strip,
    /// External always-visible activity rail; the in-side strip is suppressed.
    Rail,
}

/// Placement mode for a programmatically-opened dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockOpenMode {
    /// Stack into the side's currently-selected tab (as a ToolBox section).
    Stack,
    /// Create a brand-new tab holding just this dock.
    NewTab,
}

/// Target for [`DockingModel::open_dock`] / [`DockingModel::move_dock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockOpenLocation {
    pub side: DockSide,
    pub mode: DockOpenMode,
}

impl DockOpenLocation {
    /// Default placement on a side (stack into the active tab).
    pub fn side(side: DockSide) -> Self {
        Self {
            side,
            mode: DockOpenMode::Stack,
        }
    }
    /// Stack into the side's active tab.
    pub fn stack(mut self) -> Self {
        self.mode = DockOpenMode::Stack;
        self
    }
    /// Open as a fresh tab.
    pub fn new_tab(mut self) -> Self {
        self.mode = DockOpenMode::NewTab;
        self
    }
}

/// Activity-bar item size for a side's rail (context-menu "Activity bar size").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockRailItemSize {
    /// The rail's configured size (`DockRail::size`); icon only, title on hover.
    Default = 0,
    /// Compact items ([`IconButtonSize::Compact`](crate::icon_button::IconButtonSize::Compact));
    /// icon only, title on hover.
    Compact = 1,
    /// Icon at the configured size **plus** a 90°-rotated title beneath it (the
    /// vertical-accordion look). The title shows inline, so no hover tooltip.
    Labeled = 2,
}

impl DockRailItemSize {
    fn from_usize(v: usize) -> Self {
        match v {
            1 => Self::Compact,
            2 => Self::Labeled,
            _ => Self::Default,
        }
    }

    /// Whether this mode paints the title inline (rotated) rather than only as a
    /// hover tooltip.
    pub fn shows_label(self) -> bool {
        matches!(self, Self::Labeled)
    }
}

/// How a side's dock tabs render (context-menu "Tab size").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockTabDisplay {
    /// Title text only (the default).
    Text = 0,
    /// The dock's icon only (falls back to the title initial if it has none).
    Icon = 1,
    /// Icon + title.
    IconText = 2,
}

impl DockTabDisplay {
    fn from_usize(v: usize) -> Self {
        match v {
            1 => Self::Icon,
            2 => Self::IconText,
            _ => Self::Text,
        }
    }
    /// Whether this mode shows the icon glyph.
    pub fn shows_icon(self) -> bool {
        matches!(self, Self::Icon | Self::IconText)
    }
    /// Whether this mode shows the title text.
    pub fn shows_text(self) -> bool {
        matches!(self, Self::Text | Self::IconText)
    }
}

/// Static metadata for a registered dock widget. App-declared; reconstructed
/// each run (never serialized).
pub(crate) struct DockWidgetMeta {
    pub title: LocalizedString,
    pub icon: Option<DockIconFactory>,
    pub closable: bool,
    #[allow(dead_code)]
    pub min_size: Option<Size>,
    pub default: DockOpenLocation,
}

/// One tab of a side's TabWidget: a `Splitter` arrangement of panes, each pane
/// a single dock. Stacking two docks side-by-side adds a Splitter pane (each
/// rendered as a single-item ToolBox) — there is no multi-section pane.
#[derive(Clone)]
pub(crate) struct DockTab {
    pub id: DockTabId,
    pub title: Option<LocalizedString>,
    pub icon: Option<DockIconFactory>,
    pub splitter: SplitterModel,
    /// One dock per Splitter pane.
    pub panes: Vec<DockWidgetId>,
    /// User-hidden ("hide this activity"): kept in the model so it stays
    /// listable + restorable, but not shown in the rail / tab strip.
    pub hidden: bool,
}

/// Runtime state for one side.
pub(crate) struct SideState {
    pub tabs: Vec<DockTab>,
    pub selected_tab: usize,
    pub presentation: TabPresentation,
    pub rail_thickness: f32,
    pub size: f32,
    pub min_size: f32,
    pub visible: bool,
    pub visible_sig: Signal<bool>,
    pub selected_tab_sig: Signal<usize>,
    /// Activity-bar item size for this side's rail: `0` = the rail's configured
    /// (default) size, `1` = compact. Reactive — the rail binds it at `Rebuild`
    /// and re-reads the size when it flips.
    pub rail_size_sig: Signal<usize>,
    /// How this side's dock tabs render: `0` = text only, `1` = icon only,
    /// `2` = icon + text. Reactive — the tab strip binds it at `Rebuild`.
    pub tab_display_sig: Signal<usize>,
}

/// Where a dock currently lives. One dock per Splitter pane, so a
/// `(side, tab_idx, pane_idx)` triple addresses it exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockLoc {
    pub side: DockSide,
    pub tab_idx: usize,
    pub pane_idx: usize,
}

/// What [`DockingModel::detach`] pruned, so a caller holding a target index in
/// the same side can correct for the resulting shift before re-inserting.
#[derive(Debug, Clone, Copy)]
struct DetachOutcome {
    loc: DockLoc,
    removed_pane: bool,
    removed_tab: bool,
}

impl DetachOutcome {
    /// Adjust a `(tab_idx, pane_idx)` drop target on `side` for the panes /
    /// tabs this detach removed *before* it. (A removal *at* the target index
    /// is the drop-on-self case, handled by an explicit no-op guard upstream.)
    fn adjust_target(&self, side: DockSide, tab_idx: usize, pane_idx: usize) -> (usize, usize) {
        if self.loc.side != side {
            return (tab_idx, pane_idx);
        }
        if self.removed_tab {
            // A whole tab vanished: indices after it shift down by one.
            let tab_idx = if self.loc.tab_idx < tab_idx {
                tab_idx - 1
            } else {
                tab_idx
            };
            return (tab_idx, pane_idx);
        }
        if self.removed_pane && self.loc.tab_idx == tab_idx && self.loc.pane_idx < pane_idx {
            return (tab_idx, pane_idx - 1);
        }
        (tab_idx, pane_idx)
    }
}

/// The splitter orientation a side uses to arrange its panes: leading /
/// trailing columns stack vertically; top / bottom strips run horizontally.
pub(crate) fn side_orientation(side: DockSide) -> Orientation {
    if side.is_horizontal_axis() {
        Orientation::Vertical
    } else {
        Orientation::Horizontal
    }
}

fn default_side_state(side: DockSide) -> SideState {
    let (size, rail, min) = if side.is_horizontal_axis() {
        (260.0, 0.0, 120.0)
    } else {
        (200.0, 0.0, 80.0)
    };
    SideState {
        tabs: Vec::new(),
        selected_tab: 0,
        presentation: TabPresentation::Strip,
        rail_thickness: rail,
        size,
        min_size: min,
        visible: false,
        visible_sig: Signal::new(false),
        selected_tab_sig: Signal::new(0),
        rail_size_sig: Signal::new(DockRailItemSize::Default as usize),
        tab_display_sig: Signal::new(DockTabDisplay::Text as usize),
    }
}

struct Inner {
    sides: HashMap<DockSide, SideState>,
    docks: HashMap<DockWidgetId, DockWidgetMeta>,
    locations: HashMap<DockWidgetId, DockLoc>,
    open_sigs: HashMap<DockWidgetId, Signal<bool>>,
    corners: CornerOwners,
    /// Bumped on **structural** change (tab / pane / section / side
    /// add-remove) — the widget binds it at `BindingLevel::Rebuild`.
    version: Signal<u64>,
    /// Bumped on **geometry** change (side size / visibility / corners) —
    /// the widget binds it at `BindingLevel::Relayout`. Avoids a full rebuild
    /// (and content teardown) for resize / show-hide / corner flips.
    geometry_version: Signal<u64>,
    animate_next: bool,
}

/// A read-only view of one tab, handed to the widget layer for rendering.
/// `panes` holds one dock per Splitter pane.
#[derive(Clone)]
pub(crate) struct DockTabView {
    pub id: DockTabId,
    pub title: Option<LocalizedString>,
    pub icon: Option<DockIconFactory>,
    pub splitter: SplitterModel,
    pub panes: Vec<DockWidgetId>,
    pub hidden: bool,
}

/// The shared docking-layout model. `Clone` = share-by-handle.
pub struct DockingModel(Rc<RefCell<Inner>>);

impl Clone for DockingModel {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for DockingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.0.borrow();
        f.debug_struct("DockingModel")
            .field("docks", &inner.docks.len())
            .field("open", &inner.locations.len())
            .finish()
    }
}

impl Default for DockingModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DockingModel {
    /// A fresh model: four empty, hidden sides and the default corner owners.
    pub fn new() -> Self {
        let mut sides = HashMap::new();
        for side in DockSide::ALL {
            sides.insert(side, default_side_state(side));
        }
        Self(Rc::new(RefCell::new(Inner {
            sides,
            docks: HashMap::new(),
            locations: HashMap::new(),
            open_sigs: HashMap::new(),
            corners: CornerOwners::default(),
            version: Signal::new(0),
            geometry_version: Signal::new(0),
            animate_next: true,
        })))
    }

    /// Structural version — bump on tab / pane / section / side add-remove.
    /// The widget binds this at `BindingLevel::Rebuild`.
    pub fn version(&self) -> Signal<u64> {
        self.0.borrow().version.clone()
    }

    /// Geometry version — bump on side size / visibility / corner change.
    /// The widget binds this at `BindingLevel::Relayout`.
    pub fn geometry_version(&self) -> Signal<u64> {
        self.0.borrow().geometry_version.clone()
    }

    /// Read-and-reset the "animate the next side show/hide" latch.
    pub fn consume_animate_flag(&self) -> bool {
        let mut inner = self.0.borrow_mut();
        std::mem::replace(&mut inner.animate_next, true)
    }

    // ─── notify ────────────────────────────────────────────────────────

    /// Re-sync all derived signals (visible / selected-tab / open) with the
    /// borrow dropped first so observers may read the model.
    fn sync_derived(&self) {
        let mut updates_b: Vec<(Signal<bool>, bool)> = Vec::new();
        let mut updates_u: Vec<(Signal<usize>, usize)> = Vec::new();
        {
            let inner = self.0.borrow();
            for st in inner.sides.values() {
                updates_b.push((st.visible_sig.clone(), st.visible));
                updates_u.push((st.selected_tab_sig.clone(), st.selected_tab));
            }
            for (id, sig) in &inner.open_sigs {
                updates_b.push((sig.clone(), inner.locations.contains_key(id)));
            }
        }
        for (sig, val) in updates_b {
            if sig.get() != val {
                sig.set(val);
            }
        }
        for (sig, val) in updates_u {
            if sig.get() != val {
                sig.set(val);
            }
        }
    }

    /// Structural change: bump `version` (→ Rebuild) + re-sync derived.
    fn notify(&self) {
        let version = self.0.borrow().version.clone();
        version.set(version.get().wrapping_add(1));
        self.sync_derived();
    }

    /// Geometry change: bump `geometry_version` (→ Relayout) + re-sync derived.
    /// Does NOT rebuild, so content subtrees are preserved.
    fn relayout(&self) {
        let gv = self.0.borrow().geometry_version.clone();
        gv.set(gv.get().wrapping_add(1));
        self.sync_derived();
    }

    /// Mark the next side-visibility change as non-animated (snap).
    fn set_no_animate(&self) {
        self.0.borrow_mut().animate_next = false;
    }

    // ─── registration ──────────────────────────────────────────────────

    /// Register a dock widget's metadata. App-declared; idempotent (last
    /// wins). Does not place the dock — use [`open_dock`](Self::open_dock).
    pub(crate) fn register_meta(&self, id: DockWidgetId, meta: DockWidgetMeta) {
        self.0.borrow_mut().docks.insert(id, meta);
    }

    /// Whether a dock id is known (its content factory + meta are registered).
    pub fn is_registered(&self, id: DockWidgetId) -> bool {
        self.0.borrow().docks.contains_key(&id)
    }

    // ─── side configuration ────────────────────────────────────────────

    /// Set a side's activity-rail thickness and presentation. A non-zero rail
    /// switches the side to [`TabPresentation::Rail`]; the in-side strip is
    /// then suppressed.
    pub fn set_side_rail(&self, side: DockSide, thickness: f32) {
        {
            let mut inner = self.0.borrow_mut();
            if let Some(st) = inner.sides.get_mut(&side) {
                st.rail_thickness = thickness.max(0.0);
                st.presentation = if thickness > 0.0 {
                    TabPresentation::Rail
                } else {
                    TabPresentation::Strip
                };
            }
        }
        self.notify();
    }

    /// Set a side's stored content size (px). Relayout only (no rebuild).
    pub fn set_side_size(&self, side: DockSide, size: f32) {
        {
            let mut inner = self.0.borrow_mut();
            if let Some(st) = inner.sides.get_mut(&side) {
                st.size = size.max(0.0);
            }
        }
        self.relayout();
    }

    /// Set a side's minimum content size (px).
    pub fn set_side_min_size(&self, side: DockSide, min: f32) {
        {
            let mut inner = self.0.borrow_mut();
            if let Some(st) = inner.sides.get_mut(&side) {
                st.min_size = min.max(0.0);
            }
        }
        self.relayout();
    }

    /// Show / hide a whole side (animated).
    pub fn set_side_visible(&self, side: DockSide, visible: bool) {
        self.set_side_visible_inner(side, visible, true);
    }

    /// Show / hide a side immediately (no animation — drag-driven).
    pub fn set_side_visible_immediate(&self, side: DockSide, visible: bool) {
        self.set_side_visible_inner(side, visible, false);
    }

    fn set_side_visible_inner(&self, side: DockSide, visible: bool, animate: bool) {
        let changed = {
            let mut inner = self.0.borrow_mut();
            match inner.sides.get_mut(&side) {
                Some(st) if st.visible != visible => {
                    st.visible = visible;
                    true
                }
                _ => false,
            }
        };
        if changed {
            if !animate {
                self.set_no_animate();
            }
            // Relayout only: the side's content is pre-built and parked
            // dormant when hidden, so showing / hiding animates without a
            // rebuild (content preserved).
            self.relayout();
        }
    }

    /// Toggle a side's visibility (animated).
    pub fn toggle_side_visible(&self, side: DockSide) {
        let cur = self.is_side_visible(side);
        self.set_side_visible(side, !cur);
    }

    /// Select the active tab of a side. Repaint only (the Switcher swaps via
    /// its bound `selected_tab` signal — no rebuild, no relayout).
    pub fn select_tab(&self, side: DockSide, tab_idx: usize) {
        {
            let mut inner = self.0.borrow_mut();
            if let Some(st) = inner.sides.get_mut(&side)
                && tab_idx < st.tabs.len()
            {
                st.selected_tab = tab_idx;
            }
        }
        self.sync_derived();
    }

    /// Select a side's active tab by id (position-independent — used by the
    /// rail / strip, whose visible order may skip hidden tabs).
    pub fn select_tab_by_id(&self, side: DockSide, tab_id: DockTabId) {
        let idx = {
            let inner = self.0.borrow();
            inner
                .sides
                .get(&side)
                .and_then(|st| st.tabs.iter().position(|t| t.id == tab_id))
        };
        if let Some(i) = idx {
            self.select_tab(side, i);
        }
    }

    // ─── per-activity hide (context-menu "Hide" + checkable list) ──────────

    /// Hide / show one activity (tab). A hidden activity stays registered (so
    /// it remains listable + restorable) but is dropped from the rail and tab
    /// strip. Hiding the selected tab moves the selection to the nearest still-
    /// visible tab. Structural → rebuild.
    pub fn set_tab_hidden(&self, tab_id: DockTabId, hidden: bool) {
        let mut changed = false;
        {
            let mut inner = self.0.borrow_mut();
            'outer: for st in inner.sides.values_mut() {
                if let Some(ti) = st.tabs.iter().position(|t| t.id == tab_id) {
                    if st.tabs[ti].hidden == hidden {
                        break 'outer;
                    }
                    st.tabs[ti].hidden = hidden;
                    changed = true;
                    // If we just hid the selected tab, move selection to the
                    // nearest non-hidden tab (forward first, then backward).
                    if hidden && st.selected_tab == ti {
                        let n = st.tabs.len();
                        let next = (ti + 1..n)
                            .find(|&j| !st.tabs[j].hidden)
                            .or_else(|| (0..ti).rev().find(|&j| !st.tabs[j].hidden));
                        if let Some(j) = next {
                            st.selected_tab = j;
                        }
                    }
                    break 'outer;
                }
            }
        }
        if changed {
            self.notify();
        }
    }

    /// Whether an activity (tab) is currently hidden.
    pub fn is_tab_hidden(&self, tab_id: DockTabId) -> bool {
        let inner = self.0.borrow();
        inner
            .sides
            .values()
            .any(|st| st.tabs.iter().any(|t| t.id == tab_id && t.hidden))
    }

    /// Count of a side's non-hidden tabs (the rail / strip item count).
    pub(crate) fn side_visible_tab_count(&self, side: DockSide) -> usize {
        let inner = self.0.borrow();
        inner
            .sides
            .get(&side)
            .map(|st| st.tabs.iter().filter(|t| !t.hidden).count())
            .unwrap_or(0)
    }

    // ─── per-side display prefs (rail item size / tab display) ─────────────

    /// The reactive rail-size selector for a side (`0` = configured size, `1` =
    /// compact). The rail binds it at `Rebuild`; the context-menu radio writes
    /// it.
    pub(crate) fn rail_size_signal(&self, side: DockSide) -> Signal<usize> {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|st| st.rail_size_sig.clone())
            .unwrap_or_else(|| Signal::new(0))
    }

    /// Current activity-bar item size for a side.
    pub fn side_rail_size(&self, side: DockSide) -> DockRailItemSize {
        DockRailItemSize::from_usize(self.rail_size_signal(side).get())
    }

    /// Set a side's activity-bar item size (reactive → the rail rebuilds).
    pub fn set_side_rail_size(&self, side: DockSide, size: DockRailItemSize) {
        self.rail_size_signal(side).set(size as usize);
    }

    /// Reactive activity-bar **size mode** for a side — fires whenever the user
    /// switches Default / Compact / Icon + Label (via the context menu or
    /// [`set_side_rail_size`](Self::set_side_rail_size)). Bind it to adapt any
    /// external widget — a rail's slotted controls, an app toolbar — to the
    /// rail's current item size. (The rail rebuilds its slots on every change,
    /// so a slot factory that reads this signal stays in step.)
    pub fn rail_size_mode_signal(&self, side: DockSide) -> Signal<DockRailItemSize> {
        self.rail_size_signal(side)
            .map(|v| DockRailItemSize::from_usize(*v))
    }

    /// The reactive tab-display mode for a side (`0`/`1`/`2`). The tab strip
    /// binds it at `Rebuild`; the context-menu radio writes it.
    pub(crate) fn tab_display_signal(&self, side: DockSide) -> Signal<usize> {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|st| st.tab_display_sig.clone())
            .unwrap_or_else(|| Signal::new(0))
    }

    /// Current dock-tab display mode for a side.
    pub fn side_tab_display(&self, side: DockSide) -> DockTabDisplay {
        DockTabDisplay::from_usize(self.tab_display_signal(side).get())
    }

    /// Set a side's dock-tab display mode (reactive → the strip rebuilds).
    pub fn set_side_tab_display(&self, side: DockSide, display: DockTabDisplay) {
        self.tab_display_signal(side).set(display as usize);
    }

    /// Set the owner of a corner (must be one of its two adjacent sides).
    pub fn set_corner(&self, corner: DockCorner, owner: DockSide) {
        {
            let mut inner = self.0.borrow_mut();
            inner.corners.set(corner, owner);
        }
        self.relayout();
    }

    // ─── placement / movement ──────────────────────────────────────────

    /// Detach a dock from wherever it currently lives, pruning the emptied
    /// pane (and the tab, if that was its last pane). Returns a
    /// [`DetachOutcome`] describing the dock's prior location and whether the
    /// prune removed the enclosing pane / tab (so callers holding a target
    /// index in the same side can adjust for the shift). Caller must
    /// `notify()` afterwards.
    fn detach(inner: &mut Inner, id: DockWidgetId) -> Option<DetachOutcome> {
        let loc = inner.locations.remove(&id)?;
        let mut removed_pane = false;
        let mut removed_tab = false;
        if let Some(st) = inner.sides.get_mut(&loc.side)
            && let Some(tab) = st.tabs.get_mut(loc.tab_idx)
            && loc.pane_idx < tab.panes.len()
        {
            // One dock per pane → detaching always removes the pane.
            removed_pane = true;
            tab.panes.remove(loc.pane_idx);
            tab.splitter.remove_pane(loc.pane_idx);
            if tab.panes.is_empty() {
                removed_tab = true;
                st.tabs.remove(loc.tab_idx);
                if st.selected_tab >= st.tabs.len() && !st.tabs.is_empty() {
                    st.selected_tab = st.tabs.len() - 1;
                }
                if st.tabs.is_empty() {
                    st.selected_tab = 0;
                    st.visible = false;
                }
            }
        }
        // Rebuild every location for the affected side (indices shifted).
        Self::reindex_side(inner, loc.side);
        Some(DetachOutcome {
            loc,
            removed_pane,
            removed_tab,
        })
    }

    /// Recompute `locations` for every dock on a side after structural edits.
    fn reindex_side(inner: &mut Inner, side: DockSide) {
        let entries: Vec<(DockWidgetId, DockLoc)> = {
            let Some(st) = inner.sides.get(&side) else {
                return;
            };
            let mut out = Vec::new();
            for (ti, tab) in st.tabs.iter().enumerate() {
                for (pi, dock) in tab.panes.iter().enumerate() {
                    out.push((
                        *dock,
                        DockLoc {
                            side,
                            tab_idx: ti,
                            pane_idx: pi,
                        },
                    ));
                }
            }
            out
        };
        // Drop stale entries for this side, then re-insert.
        inner.locations.retain(|_, l| l.side != side);
        for (id, loc) in entries {
            inner.locations.insert(id, loc);
        }
    }

    fn new_tab(side: DockSide, dock: DockWidgetId) -> DockTab {
        DockTab {
            id: DockTabId::fresh(),
            title: None,
            icon: None,
            splitter: SplitterModel::new(1, side_orientation(side)),
            panes: vec![dock],
            hidden: false,
        }
    }

    /// Open (or move) a dock onto a side. Already-open docks are relocated
    /// (never duplicated).
    pub fn open_dock(&self, id: DockWidgetId, loc: DockOpenLocation) {
        {
            let mut inner = self.0.borrow_mut();
            if !inner.docks.contains_key(&id) {
                debug_assert!(false, "open_dock on unregistered dock {id:?}");
                return;
            }
            // Ensure-open semantics: re-opening a dock already on the target
            // side is a no-op (no version churn). Moving across sides, and the
            // explicit promote/split/move mutators, still re-place it.
            if inner.locations.get(&id).map(|l| l.side) == Some(loc.side) {
                return;
            }
            Self::detach(&mut inner, id);
            Self::place(&mut inner, id, loc);
        }
        self.notify();
    }

    fn place(inner: &mut Inner, id: DockWidgetId, loc: DockOpenLocation) {
        let side = loc.side;
        let orientation = side_orientation(side);
        if let Some(st) = inner.sides.get_mut(&side) {
            st.visible = true;
            match loc.mode {
                DockOpenMode::NewTab => {
                    st.tabs.push(DockTab {
                        id: DockTabId::fresh(),
                        title: None,
                        icon: None,
                        splitter: SplitterModel::new(1, orientation),
                        panes: vec![id],
                        hidden: false,
                    });
                    st.selected_tab = st.tabs.len() - 1;
                }
                DockOpenMode::Stack => {
                    if st.tabs.is_empty() {
                        st.tabs.push(DockTab {
                            id: DockTabId::fresh(),
                            title: None,
                            icon: None,
                            splitter: SplitterModel::new(1, orientation),
                            panes: vec![id],
                            hidden: false,
                        });
                        st.selected_tab = 0;
                    } else {
                        // Stacking adds a Splitter pane to the selected tab
                        // (each pane is its own single-item ToolBox), not a
                        // multi-section pane.
                        let ti = st.selected_tab.min(st.tabs.len() - 1);
                        let tab = &mut st.tabs[ti];
                        let at = tab.panes.len();
                        tab.panes.push(id);
                        tab.splitter.insert_pane(at, PaneDescriptor::new().stretch(1.0));
                        st.selected_tab = ti;
                    }
                }
            }
        }
        Self::reindex_side(inner, side);
    }

    /// Drag a dock out into its own new tab on `side`, inserted at `at_tab`.
    pub fn promote_to_tab(&self, id: DockWidgetId, side: DockSide, at_tab: usize) {
        {
            let mut inner = self.0.borrow_mut();
            if !inner.docks.contains_key(&id) {
                return;
            }
            // Already its own sole tab on this side at this index → no-op.
            if let Some(loc) = inner.locations.get(&id)
                && loc.side == side
                && loc.tab_idx == at_tab
                && inner
                    .sides
                    .get(&side)
                    .and_then(|st| st.tabs.get(at_tab))
                    .map(|t| t.panes.len() == 1)
                    .unwrap_or(false)
            {
                return;
            }
            let mut at_tab = at_tab;
            if let Some(outcome) = Self::detach(&mut inner, id) {
                (at_tab, _) = outcome.adjust_target(side, at_tab, 0);
            }
            let tab = Self::new_tab(side, id);
            if let Some(st) = inner.sides.get_mut(&side) {
                let at = at_tab.min(st.tabs.len());
                st.tabs.insert(at, tab);
                st.selected_tab = at;
                st.visible = true;
            }
            Self::reindex_side(&mut inner, side);
        }
        self.notify();
    }

    /// Drop a dock into an existing tab's Splitter as a new `Single` pane,
    /// before (`before = true`) or after the pane at `pane_idx`.
    pub fn split_into_tab(
        &self,
        id: DockWidgetId,
        side: DockSide,
        tab_idx: usize,
        pane_idx: usize,
        before: bool,
    ) {
        {
            let mut inner = self.0.borrow_mut();
            if !inner.docks.contains_key(&id) {
                return;
            }
            // Drop-on-self: splitting the pane a dock already solely occupies
            // with that same dock changes nothing. No-op (no churn).
            if let Some(loc) = inner.locations.get(&id)
                && loc.side == side
                && loc.tab_idx == tab_idx
                && loc.pane_idx == pane_idx
            {
                return;
            }
            // Detach first, then correct the target for any pane / tab the
            // detach pruned at an earlier index in this side.
            let (mut tab_idx, mut pane_idx) = (tab_idx, pane_idx);
            if let Some(outcome) = Self::detach(&mut inner, id) {
                (tab_idx, pane_idx) = outcome.adjust_target(side, tab_idx, pane_idx);
            }
            if let Some(st) = inner.sides.get_mut(&side)
                && let Some(tab) = st.tabs.get_mut(tab_idx)
            {
                let at = if before { pane_idx } else { pane_idx + 1 };
                let at = at.min(tab.panes.len());
                tab.panes.insert(at, id);
                tab.splitter.insert_pane(at, PaneDescriptor::new().stretch(1.0));
                st.visible = true;
                st.selected_tab = tab_idx;
            }
            Self::reindex_side(&mut inner, side);
        }
        self.notify();
    }

    /// Drop a dock into a tab as a new Splitter pane appended after its
    /// existing panes (the "centre" drop — join this group without choosing a
    /// split direction). Each pane is its own single-item ToolBox.
    pub fn stack_into_tab(&self, id: DockWidgetId, side: DockSide, tab_idx: usize) {
        {
            let mut inner = self.0.borrow_mut();
            if !inner.docks.contains_key(&id) {
                return;
            }
            // Drop-on-self: a dock that is already the sole pane of this tab.
            if let Some(loc) = inner.locations.get(&id)
                && loc.side == side
                && loc.tab_idx == tab_idx
                && inner
                    .sides
                    .get(&side)
                    .and_then(|st| st.tabs.get(tab_idx))
                    .map(|t| t.panes.len() == 1)
                    .unwrap_or(false)
            {
                return;
            }
            let mut tab_idx = tab_idx;
            if let Some(outcome) = Self::detach(&mut inner, id) {
                (tab_idx, _) = outcome.adjust_target(side, tab_idx, 0);
            }
            if let Some(st) = inner.sides.get_mut(&side)
                && let Some(tab) = st.tabs.get_mut(tab_idx)
            {
                let at = tab.panes.len();
                tab.panes.push(id);
                tab.splitter.insert_pane(at, PaneDescriptor::new().stretch(1.0));
                st.visible = true;
                st.selected_tab = tab_idx;
            }
            Self::reindex_side(&mut inner, side);
        }
        self.notify();
    }

    /// Move a dock to another location (close + open in one notify).
    pub fn move_dock(&self, id: DockWidgetId, loc: DockOpenLocation) {
        self.open_dock(id, loc);
    }

    /// Close a whole tab (and every dock it holds).
    pub fn close_tab(&self, tab_id: DockTabId) {
        {
            let mut inner = self.0.borrow_mut();
            let mut found: Option<(DockSide, usize)> = None;
            for side in DockSide::ALL {
                if let Some(st) = inner.sides.get(&side)
                    && let Some(ti) = st.tabs.iter().position(|t| t.id == tab_id)
                {
                    found = Some((side, ti));
                    break;
                }
            }
            let Some((side, ti)) = found else {
                return;
            };
            if let Some(st) = inner.sides.get_mut(&side) {
                st.tabs.remove(ti);
                if st.selected_tab >= st.tabs.len() && !st.tabs.is_empty() {
                    st.selected_tab = st.tabs.len() - 1;
                }
                if st.tabs.is_empty() {
                    st.selected_tab = 0;
                    st.visible = false;
                }
            }
            Self::reindex_side(&mut inner, side);
        }
        self.notify();
    }

    /// Move a whole tab (its arrangement + every dock + selection) to another
    /// side, re-deriving the Splitter orientation. Inserted at `at_tab`.
    pub fn move_tab(&self, tab_id: DockTabId, target_side: DockSide, at_tab: usize) {
        {
            let mut inner = self.0.borrow_mut();
            // Find + remove the tab from its current side.
            let mut found: Option<(DockSide, usize)> = None;
            for side in DockSide::ALL {
                if let Some(st) = inner.sides.get(&side)
                    && let Some(ti) = st.tabs.iter().position(|t| t.id == tab_id)
                {
                    found = Some((side, ti));
                    break;
                }
            }
            let Some((src_side, src_ti)) = found else {
                return;
            };
            if src_side == target_side {
                // Same-side reorder.
                if let Some(st) = inner.sides.get_mut(&src_side) {
                    let tab = st.tabs.remove(src_ti);
                    let at = at_tab.min(st.tabs.len());
                    st.tabs.insert(at, tab);
                    st.selected_tab = at;
                }
                Self::reindex_side(&mut inner, src_side);
            } else {
                let tab = {
                    let st = inner.sides.get_mut(&src_side).unwrap();
                    let tab = st.tabs.remove(src_ti);
                    if st.selected_tab >= st.tabs.len() && !st.tabs.is_empty() {
                        st.selected_tab = st.tabs.len() - 1;
                    }
                    if st.tabs.is_empty() {
                        st.selected_tab = 0;
                        st.visible = false;
                    }
                    tab
                };
                // Re-derive orientation for the destination side.
                tab.splitter.set_orientation(side_orientation(target_side));
                if let Some(dst) = inner.sides.get_mut(&target_side) {
                    let at = at_tab.min(dst.tabs.len());
                    dst.tabs.insert(at, tab);
                    dst.selected_tab = at;
                    dst.visible = true;
                }
                Self::reindex_side(&mut inner, src_side);
                Self::reindex_side(&mut inner, target_side);
            }
        }
        self.notify();
    }

    /// Close (remove) a dock from the layout.
    pub fn close_dock(&self, id: DockWidgetId) {
        {
            let mut inner = self.0.borrow_mut();
            Self::detach(&mut inner, id);
        }
        self.notify();
    }

    /// Toggle a dock: close it if open, else open it on its default location.
    pub fn toggle_dock(&self, id: DockWidgetId) {
        if self.is_dock_open(id) {
            self.close_dock(id);
        } else {
            let default = self.0.borrow().docks.get(&id).map(|m| m.default);
            if let Some(loc) = default {
                self.open_dock(id, loc);
            }
        }
    }

    /// Reveal a dock: ensure it is open, show + select its side / tab.
    pub fn reveal_dock(&self, id: DockWidgetId) {
        let loc = self.0.borrow().locations.get(&id).copied();
        match loc {
            Some(loc) => {
                {
                    let mut inner = self.0.borrow_mut();
                    if let Some(st) = inner.sides.get_mut(&loc.side) {
                        st.visible = true;
                        st.selected_tab = loc.tab_idx.min(st.tabs.len().saturating_sub(1));
                    }
                }
                self.notify();
            }
            None => {
                let default = self.0.borrow().docks.get(&id).map(|m| m.default);
                if let Some(loc) = default {
                    self.open_dock(id, loc);
                }
            }
        }
    }

    // ─── queries ───────────────────────────────────────────────────────

    pub fn is_dock_open(&self, id: DockWidgetId) -> bool {
        self.0.borrow().locations.contains_key(&id)
    }

    /// Every currently-open dock id (used by the layout to materialize
    /// content).
    pub(crate) fn open_dock_ids(&self) -> Vec<DockWidgetId> {
        self.0.borrow().locations.keys().copied().collect()
    }

    pub fn dock_location(&self, id: DockWidgetId) -> Option<DockLoc> {
        self.0.borrow().locations.get(&id).copied()
    }

    /// A reactive `true`-while-open signal for an external rail / toolbar.
    pub fn dock_open_signal(&self, id: DockWidgetId) -> Signal<bool> {
        let mut inner = self.0.borrow_mut();
        let open = inner.locations.contains_key(&id);
        inner
            .open_sigs
            .entry(id)
            .or_insert_with(|| Signal::new(open))
            .clone()
    }

    pub fn is_side_visible(&self, side: DockSide) -> bool {
        self.0.borrow().sides.get(&side).is_some_and(|s| s.visible)
    }

    pub fn side_visible_signal(&self, side: DockSide) -> Signal<bool> {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| s.visible_sig.clone())
            .unwrap_or_else(|| Signal::new(false))
    }

    pub fn side_selected_tab_signal(&self, side: DockSide) -> Signal<usize> {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| s.selected_tab_sig.clone())
            .unwrap_or_else(|| Signal::new(0))
    }

    pub fn side_selected_tab(&self, side: DockSide) -> usize {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| s.selected_tab)
            .unwrap_or(0)
    }

    pub fn side_presentation(&self, side: DockSide) -> TabPresentation {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| s.presentation)
            .unwrap_or(TabPresentation::Strip)
    }

    pub fn side_size(&self, side: DockSide) -> f32 {
        self.0.borrow().sides.get(&side).map(|s| s.size).unwrap_or(0.0)
    }

    pub fn side_min_size(&self, side: DockSide) -> f32 {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| s.min_size)
            .unwrap_or(0.0)
    }

    pub fn side_rail_thickness(&self, side: DockSide) -> f32 {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| if matches!(s.presentation, TabPresentation::Rail) {
                s.rail_thickness
            } else {
                0.0
            })
            .unwrap_or(0.0)
    }

    pub fn side_has_rail(&self, side: DockSide) -> bool {
        self.side_rail_thickness(side) > 0.0
    }

    pub fn corner_owner(&self, corner: DockCorner) -> DockSide {
        self.0.borrow().corners.owner(corner)
    }

    pub(crate) fn corners(&self) -> CornerOwners {
        self.0.borrow().corners
    }

    pub fn tab_count(&self, side: DockSide) -> usize {
        self.0
            .borrow()
            .sides
            .get(&side)
            .map(|s| s.tabs.len())
            .unwrap_or(0)
    }

    /// Title + icon for a registered dock (for tab / rail rendering).
    pub(crate) fn dock_title(&self, id: DockWidgetId) -> Option<LocalizedString> {
        self.0.borrow().docks.get(&id).map(|m| m.title.clone())
    }

    pub(crate) fn dock_icon(&self, id: DockWidgetId) -> Option<DockIconFactory> {
        self.0.borrow().docks.get(&id).and_then(|m| m.icon.clone())
    }

    pub(crate) fn dock_closable(&self, id: DockWidgetId) -> bool {
        self.0.borrow().docks.get(&id).map(|m| m.closable).unwrap_or(true)
    }

    /// Snapshot a side's tabs for rendering.
    pub(crate) fn side_tabs(&self, side: DockSide) -> Vec<DockTabView> {
        let inner = self.0.borrow();
        let Some(st) = inner.sides.get(&side) else {
            return Vec::new();
        };
        st.tabs
            .iter()
            .map(|tab| DockTabView {
                id: tab.id,
                title: tab.title.clone(),
                icon: tab.icon.clone(),
                splitter: tab.splitter.clone(),
                panes: tab.panes.clone(),
                hidden: tab.hidden,
            })
            .collect()
    }

    /// All docks held by a tab (one per pane).
    pub(crate) fn tab_docks(&self, tab_id: DockTabId) -> Vec<DockWidgetId> {
        let inner = self.0.borrow();
        for st in inner.sides.values() {
            if let Some(tab) = st.tabs.iter().find(|t| t.id == tab_id) {
                return tab.panes.clone();
            }
        }
        Vec::new()
    }

    /// Find a tab anywhere by id, returning its current side + a render view.
    pub(crate) fn tab_view_by_id(&self, tab_id: DockTabId) -> Option<(DockSide, DockTabView)> {
        for side in DockSide::ALL {
            if let Some(view) = self.side_tabs(side).into_iter().find(|t| t.id == tab_id) {
                return Some((side, view));
            }
        }
        None
    }

    /// The active dock on a side (the first pane of the given tab), used to
    /// derive a tab/rail label when none is explicit.
    pub(crate) fn side_active_dock(&self, side: DockSide, tab_idx: usize) -> Option<DockWidgetId> {
        let inner = self.0.borrow();
        let st = inner.sides.get(&side)?;
        let tab = st.tabs.get(tab_idx)?;
        tab.panes.first().copied()
    }

    /// The id of the tab at `idx` in a side's full tab list. The live inverse
    /// of [`select_tab_by_id`](Self::select_tab_by_id) — the strip's
    /// index → id selection sync uses it so both directions resolve against the
    /// *current* order and agree across a reorder (a build-time snapshot would
    /// disagree and feed back unboundedly).
    pub(crate) fn side_tab_id_at(&self, side: DockSide, idx: usize) -> Option<DockTabId> {
        let inner = self.0.borrow();
        inner
            .sides
            .get(&side)
            .and_then(|st| st.tabs.get(idx).map(|t| t.id))
    }

    // ─── persistence ───────────────────────────────────────────────────

    /// Serialize the user-controllable layout state (sizes / visibility /
    /// selections / arrangement structure / corners). App-config (rail
    /// thickness, mins, content factories) is reconstructed each run.
    pub fn export_state(&self) -> super::state::DockLayoutState {
        use super::state::*;
        let inner = self.0.borrow();
        let side_state = |side: DockSide| -> DockSideState {
            let Some(st) = inner.sides.get(&side) else {
                return DockSideState::default();
            };
            DockSideState {
                presentation: st.presentation,
                size_px: st.size,
                visible: st.visible,
                selected_tab: st.selected_tab,
                tabs: st
                    .tabs
                    .iter()
                    .map(|tab| DockTabState {
                        id: tab.id.raw(),
                        splitter: tab.splitter.export_state(),
                        panes: tab.panes.iter().map(|d| d.raw()).collect(),
                        hidden: tab.hidden,
                    })
                    .collect(),
                rail_size: st.rail_size_sig.get().min(DockRailItemSize::Labeled as usize),
                tab_display: st.tab_display_sig.get(),
            }
        };
        DockLayoutState {
            version: DockLayoutState::CURRENT_VERSION,
            leading: side_state(DockSide::Leading),
            trailing: side_state(DockSide::Trailing),
            top: side_state(DockSide::Top),
            bottom: side_state(DockSide::Bottom),
            corners: inner.corners,
        }
    }

    /// Restore a previously-exported state. Unknown dock ids are dropped,
    /// emptied panes / tabs pruned, selections clamped. Bumps `version`.
    pub fn import_state(&self, state: &super::state::DockLayoutState) {
        use super::state::*;
        {
            let mut inner = self.0.borrow_mut();
            inner.corners = state.corners;
            let known: std::collections::HashSet<u64> =
                inner.docks.keys().map(|k| k.raw()).collect();

            let restore = |inner: &mut Inner, side: DockSide, dto: &DockSideState| {
                let orientation = side_orientation(side);
                let mut tabs: Vec<DockTab> = Vec::new();
                for tab_dto in &dto.tabs {
                    let mut panes: Vec<DockWidgetId> = Vec::new();
                    for dock in &tab_dto.panes {
                        if known.contains(dock) {
                            panes.push(DockWidgetId::from_raw(*dock));
                        }
                    }
                    if !panes.is_empty() {
                        let splitter = SplitterModel::new(panes.len(), orientation);
                        // Best-effort: import sizes only when the pane count
                        // matches the surviving panes.
                        if splitter.pane_count() == tab_dto.panes.len() {
                            splitter.import_state(&tab_dto.splitter);
                        }
                        tabs.push(DockTab {
                            id: DockTabId::from_raw(tab_dto.id),
                            title: None,
                            icon: None,
                            splitter,
                            panes,
                            hidden: tab_dto.hidden,
                        });
                    }
                }
                let selected_tab = if tabs.is_empty() {
                    0
                } else {
                    dto.selected_tab.min(tabs.len() - 1)
                };
                if let Some(st) = inner.sides.get_mut(&side) {
                    st.presentation = dto.presentation;
                    st.size = dto.size_px;
                    st.visible = dto.visible && !tabs.is_empty();
                    st.selected_tab = selected_tab;
                    st.tabs = tabs;
                    st.rail_size_sig
                        .set(dto.rail_size.min(DockRailItemSize::Labeled as usize));
                    st.tab_display_sig.set(dto.tab_display.min(2));
                }
            };

            restore(&mut inner, DockSide::Leading, &state.leading);
            restore(&mut inner, DockSide::Trailing, &state.trailing);
            restore(&mut inner, DockSide::Top, &state.top);
            restore(&mut inner, DockSide::Bottom, &state.bottom);

            inner.locations.clear();
            for side in DockSide::ALL {
                Self::reindex_side(&mut inner, side);
            }
        }
        self.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_i18n::lit;

    fn model() -> DockingModel {
        DockingModel::new()
    }

    fn reg(m: &DockingModel, side: DockSide) -> DockWidgetId {
        let id = DockWidgetId::fresh();
        m.register_meta(
            id,
            DockWidgetMeta {
                title: lit!("Dock"),
                icon: None,
                closable: true,
                min_size: None,
                default: DockOpenLocation::side(side),
            },
        );
        id
    }

    #[test]
    fn open_dock_places_and_shows_side() {
        let m = model();
        let id = reg(&m, DockSide::Leading);
        assert!(!m.is_side_visible(DockSide::Leading));
        m.open_dock(id, DockOpenLocation::side(DockSide::Leading));
        assert!(m.is_dock_open(id));
        assert!(m.is_side_visible(DockSide::Leading));
        assert_eq!(m.dock_location(id).unwrap().side, DockSide::Leading);
    }

    #[test]
    fn stack_adds_a_splitter_pane_to_one_tab() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        m.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
        let tabs = m.side_tabs(DockSide::Leading);
        assert_eq!(tabs.len(), 1, "stacking shares one tab");
        assert_eq!(tabs[0].panes, vec![a, b], "stacking splits into two panes");
        assert_eq!(tabs[0].splitter.pane_count(), 2);
    }

    #[test]
    fn new_tab_creates_a_second_tab() {
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        let b = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
        m.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
        assert_eq!(m.tab_count(DockSide::Bottom), 2);
    }

    #[test]
    fn single_location_invariant_no_duplicate() {
        let m = model();
        let id = reg(&m, DockSide::Leading);
        m.open_dock(id, DockOpenLocation::side(DockSide::Leading));
        m.open_dock(id, DockOpenLocation::side(DockSide::Trailing));
        // Only one location; it moved to trailing.
        assert_eq!(m.dock_location(id).unwrap().side, DockSide::Trailing);
        assert!(!m.is_side_visible(DockSide::Leading), "leading emptied → hidden");
    }

    #[test]
    fn close_last_dock_hides_side() {
        let m = model();
        let id = reg(&m, DockSide::Trailing);
        m.open_dock(id, DockOpenLocation::side(DockSide::Trailing));
        m.close_dock(id);
        assert!(!m.is_dock_open(id));
        assert!(!m.is_side_visible(DockSide::Trailing));
        assert_eq!(m.tab_count(DockSide::Trailing), 0);
    }

    #[test]
    fn close_one_of_two_stacked_panes_leaves_the_other() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        m.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
        m.close_dock(b);
        // The tab collapses back to a single pane holding `a`.
        let tabs = m.side_tabs(DockSide::Leading);
        assert_eq!(tabs[0].panes, vec![a]);
        assert_eq!(tabs[0].splitter.pane_count(), 1);
    }

    #[test]
    fn toggle_dock_round_trips() {
        let m = model();
        let id = reg(&m, DockSide::Leading);
        m.toggle_dock(id);
        assert!(m.is_dock_open(id));
        m.toggle_dock(id);
        assert!(!m.is_dock_open(id));
    }

    #[test]
    fn reveal_closed_dock_opens_on_default_side() {
        let m = model();
        let id = reg(&m, DockSide::Bottom);
        m.reveal_dock(id);
        assert!(m.is_dock_open(id));
        assert_eq!(m.dock_location(id).unwrap().side, DockSide::Bottom);
    }

    #[test]
    fn split_into_tab_adds_a_splitter_pane() {
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        let b = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
        m.split_into_tab(b, DockSide::Bottom, 0, 0, false);
        let tabs = m.side_tabs(DockSide::Bottom);
        assert_eq!(tabs[0].panes.len(), 2, "split added a second pane");
        assert_eq!(tabs[0].splitter.pane_count(), 2);
    }

    #[test]
    fn promote_to_tab_makes_its_own_tab() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        m.open_dock(b, DockOpenLocation::side(DockSide::Leading).stack());
        // a + b share one tab; promote b into its own.
        m.promote_to_tab(b, DockSide::Leading, 1);
        assert_eq!(m.tab_count(DockSide::Leading), 2);
        assert_eq!(m.dock_location(b).unwrap().tab_idx, 1);
    }

    #[test]
    fn move_tab_relocates_whole_tab_and_reorients() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        let tab_id = m.side_tabs(DockSide::Leading)[0].id;
        // Leading uses a vertical splitter; bottom a horizontal one.
        let splitter = m.side_tabs(DockSide::Leading)[0].splitter.clone();
        assert_eq!(splitter.orientation(), Orientation::Vertical);
        m.move_tab(tab_id, DockSide::Bottom, 0);
        assert_eq!(m.dock_location(a).unwrap().side, DockSide::Bottom);
        assert!(!m.is_side_visible(DockSide::Leading));
        assert_eq!(splitter.orientation(), Orientation::Horizontal, "re-derived");
    }

    #[test]
    fn version_bumps_on_mutation() {
        let m = model();
        let id = reg(&m, DockSide::Leading);
        let v = m.version();
        let before = v.get();
        m.open_dock(id, DockOpenLocation::side(DockSide::Leading));
        assert!(v.get() > before);
    }

    #[test]
    fn dock_open_signal_and_side_visible_signal_track() {
        let m = model();
        let id = reg(&m, DockSide::Leading);
        let open = m.dock_open_signal(id);
        let vis = m.side_visible_signal(DockSide::Leading);
        assert!(!open.get());
        assert!(!vis.get());
        m.open_dock(id, DockOpenLocation::side(DockSide::Leading));
        assert!(open.get());
        assert!(vis.get());
        m.close_dock(id);
        assert!(!open.get());
        assert!(!vis.get());
    }

    #[test]
    fn export_import_round_trips() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        m.open_dock(b, DockOpenLocation::side(DockSide::Bottom));
        let state = m.export_state();

        // Fresh model with the same registry restores the same shape.
        let m2 = DockingModel::new();
        m2.register_meta(
            a,
            DockWidgetMeta {
                title: lit!("A"),
                icon: None,
                closable: true,
                min_size: None,
                default: DockOpenLocation::side(DockSide::Leading),
            },
        );
        m2.register_meta(
            b,
            DockWidgetMeta {
                title: lit!("B"),
                icon: None,
                closable: true,
                min_size: None,
                default: DockOpenLocation::side(DockSide::Bottom),
            },
        );
        m2.import_state(&state);
        assert!(m2.is_dock_open(a));
        assert!(m2.is_dock_open(b));
        assert_eq!(m2.dock_location(a).unwrap().side, DockSide::Leading);
    }

    #[test]
    fn import_drops_unknown_dock_ids() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        let state = m.export_state();
        // Fresh model that does NOT know `a`.
        let m2 = DockingModel::new();
        m2.import_state(&state);
        assert!(!m2.is_dock_open(a), "unknown dock id dropped on import");
        assert!(!m2.is_side_visible(DockSide::Leading));
    }

    // ─── drop edge cases ────────────────────────────────────────────────

    #[test]
    fn split_a_dock_onto_its_own_pane_is_a_noop() {
        // Drop a dock onto the very `Single` pane it solely occupies → nothing
        // changes, no version churn (the classic "drop on itself").
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom));
        let loc_before = m.dock_location(a).unwrap();
        let v = m.version().get();

        m.split_into_tab(a, DockSide::Bottom, 0, 0, true);
        assert_eq!(m.version().get(), v, "no-op must not bump the version");
        assert_eq!(m.dock_location(a).unwrap(), loc_before, "location unchanged");
        assert_eq!(m.side_tabs(DockSide::Bottom)[0].panes.len(), 1, "still one pane");

        m.split_into_tab(a, DockSide::Bottom, 0, 0, false);
        assert_eq!(m.version().get(), v, "after-split self-drop also a no-op");
        assert_eq!(m.side_tabs(DockSide::Bottom)[0].panes.len(), 1);
    }

    #[test]
    fn stack_a_dock_onto_its_own_sole_tab_is_a_noop() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        let v = m.version().get();
        m.stack_into_tab(a, DockSide::Leading, 0);
        assert_eq!(m.version().get(), v, "stacking a dock on itself is a no-op");
        let tabs = m.side_tabs(DockSide::Leading);
        assert_eq!(tabs[0].panes, vec![a], "still a single pane");
    }

    #[test]
    fn stack_into_tab_appends_a_splitter_pane() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading));
        m.stack_into_tab(b, DockSide::Leading, 0);
        let tabs = m.side_tabs(DockSide::Leading);
        assert_eq!(tabs.len(), 1, "stacked into the same tab");
        assert_eq!(tabs[0].panes, vec![a, b], "appended as a second pane");
        assert_eq!(tabs[0].splitter.pane_count(), 2);
    }

    #[test]
    fn promote_a_dock_already_its_own_sole_tab_is_a_noop() {
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom).new_tab());
        let v = m.version().get();
        m.promote_to_tab(a, DockSide::Bottom, 0);
        assert_eq!(m.version().get(), v, "promoting an already-sole tab is a no-op");
        assert_eq!(m.tab_count(DockSide::Bottom), 1);
    }

    #[test]
    fn split_targets_the_right_pane_after_an_earlier_pane_is_pruned() {
        // Tab = [A(0), B(1), C(2)]; drop A *after* B. Detaching A removes pane 0
        // and shifts B/C down, so the naive index would land A after C. The
        // adjustment keeps it where the user aimed: [B, A, C].
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        let b = reg(&m, DockSide::Bottom);
        let c = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom).new_tab());
        m.split_into_tab(b, DockSide::Bottom, 0, 0, false); // [A, B]
        m.split_into_tab(c, DockSide::Bottom, 0, 1, false); // [A, B, C]

        m.split_into_tab(a, DockSide::Bottom, 0, 1, false); // drop A after B

        assert_eq!(m.side_tabs(DockSide::Bottom)[0].panes.len(), 3, "still 3 panes");
        assert_eq!(m.dock_location(b).unwrap().pane_idx, 0, "B leads");
        assert_eq!(m.dock_location(a).unwrap().pane_idx, 1, "A lands after B");
        assert_eq!(m.dock_location(c).unwrap().pane_idx, 2, "C trails");
    }

    #[test]
    fn stack_into_a_later_tab_after_an_earlier_tab_is_pruned_keeps_the_dock() {
        // Side = [tab0 = Single(A)], [tab1 = Single(B)]; stack A into tab1.
        // Detaching A removes tab0 (tab1 shifts to index 0); without the index
        // fix the stale tab index misses and the dock is lost.
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        let b = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom).new_tab());
        m.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
        assert_eq!(m.tab_count(DockSide::Bottom), 2);

        m.stack_into_tab(a, DockSide::Bottom, 1); // drop A into B's tab

        assert!(m.is_dock_open(a), "A must not be lost");
        assert_eq!(m.tab_count(DockSide::Bottom), 1, "A's old tab was pruned");
        assert_eq!(
            m.side_tabs(DockSide::Bottom)[0].panes,
            vec![b, a],
            "A appended to B's tab"
        );
    }

    // ─── context-menu state: hide / select-by-id / rail size / tab display ──

    #[test]
    fn set_tab_hidden_hides_and_restores_an_activity() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading).new_tab());
        m.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
        let tabs = m.side_tabs(DockSide::Leading);
        let (ta, tb) = (tabs[0].id, tabs[1].id);

        assert_eq!(m.side_visible_tab_count(DockSide::Leading), 2);
        m.set_tab_hidden(ta, true);
        assert!(m.is_tab_hidden(ta));
        assert_eq!(
            m.side_visible_tab_count(DockSide::Leading),
            1,
            "hidden activity drops out of the visible count"
        );
        // The tab still exists in the model (restorable).
        assert_eq!(m.tab_count(DockSide::Leading), 2);
        // Restore.
        m.set_tab_hidden(ta, false);
        assert!(!m.is_tab_hidden(ta));
        assert_eq!(m.side_visible_tab_count(DockSide::Leading), 2);
        let _ = tb;
    }

    #[test]
    fn hiding_the_selected_tab_moves_selection_to_a_visible_one() {
        let m = model();
        let a = reg(&m, DockSide::Bottom);
        let b = reg(&m, DockSide::Bottom);
        m.open_dock(a, DockOpenLocation::side(DockSide::Bottom).new_tab());
        m.open_dock(b, DockOpenLocation::side(DockSide::Bottom).new_tab());
        let tabs = m.side_tabs(DockSide::Bottom);
        let (ta, tb) = (tabs[0].id, tabs[1].id);

        m.select_tab_by_id(DockSide::Bottom, tb);
        assert_eq!(m.side_selected_tab(DockSide::Bottom), 1);
        // Hide the selected tab → selection must move to the other visible one.
        m.set_tab_hidden(tb, true);
        assert_eq!(
            m.side_selected_tab(DockSide::Bottom),
            0,
            "selection moved off the hidden tab"
        );
        let _ = ta;
    }

    #[test]
    fn select_tab_by_id_is_position_independent() {
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading).new_tab());
        m.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
        let id_b = m.side_tabs(DockSide::Leading)[1].id;
        m.select_tab_by_id(DockSide::Leading, id_b);
        assert_eq!(m.side_selected_tab(DockSide::Leading), 1);
    }

    #[test]
    fn rail_size_and_tab_display_round_trip_reactively() {
        use crate::docking::{DockRailItemSize, DockTabDisplay};
        let m = model();
        // Defaults.
        assert_eq!(m.side_rail_size(DockSide::Leading), DockRailItemSize::Default);
        assert_eq!(m.side_tab_display(DockSide::Leading), DockTabDisplay::Text);

        // The reactive signals the rail / strip bind to.
        let rail_sig = m.rail_size_signal(DockSide::Leading);
        let disp_sig = m.tab_display_signal(DockSide::Leading);

        m.set_side_rail_size(DockSide::Leading, DockRailItemSize::Compact);
        assert_eq!(m.side_rail_size(DockSide::Leading), DockRailItemSize::Compact);
        assert_eq!(rail_sig.get(), 1, "signal reflects the change (drives rebuild)");
        assert!(!DockRailItemSize::Compact.shows_label());

        // The third "icon + 90° label" rail mode.
        m.set_side_rail_size(DockSide::Leading, DockRailItemSize::Labeled);
        assert_eq!(m.side_rail_size(DockSide::Leading), DockRailItemSize::Labeled);
        assert_eq!(rail_sig.get(), 2, "labeled mode drives a rebuild too");
        assert!(DockRailItemSize::Labeled.shows_label());
        assert!(!DockRailItemSize::Default.shows_label());

        m.set_side_tab_display(DockSide::Leading, DockTabDisplay::IconText);
        assert_eq!(m.side_tab_display(DockSide::Leading), DockTabDisplay::IconText);
        assert_eq!(disp_sig.get(), 2);
    }

    #[test]
    fn rail_size_mode_signal_tracks_the_mode() {
        use crate::docking::DockRailItemSize;
        let m = model();
        // The public signal external widgets (rail slots, toolbars) bind to.
        let sig = m.rail_size_mode_signal(DockSide::Leading);
        assert_eq!(sig.get(), DockRailItemSize::Default);

        m.set_side_rail_size(DockSide::Leading, DockRailItemSize::Compact);
        assert_eq!(sig.get(), DockRailItemSize::Compact, "mode signal fires");
        m.set_side_rail_size(DockSide::Leading, DockRailItemSize::Labeled);
        assert_eq!(sig.get(), DockRailItemSize::Labeled);
    }

    #[test]
    fn export_import_round_trips_hidden_and_display_prefs() {
        use crate::docking::{DockRailItemSize, DockTabDisplay};
        let m = model();
        let a = reg(&m, DockSide::Leading);
        let b = reg(&m, DockSide::Leading);
        m.open_dock(a, DockOpenLocation::side(DockSide::Leading).new_tab());
        m.open_dock(b, DockOpenLocation::side(DockSide::Leading).new_tab());
        let tb = m.side_tabs(DockSide::Leading)[1].id;
        m.set_tab_hidden(tb, true);
        m.set_side_rail_size(DockSide::Leading, DockRailItemSize::Labeled);
        m.set_side_tab_display(DockSide::Leading, DockTabDisplay::Icon);

        let state = m.export_state();

        // Restore into a fresh model with the *same* dock ids registered (import
        // matches by id, dropping unknowns).
        let m2 = model();
        for id in [a, b] {
            m2.register_meta(
                id,
                DockWidgetMeta {
                    title: lit!("Dock"),
                    icon: None,
                    closable: true,
                    min_size: None,
                    default: DockOpenLocation::side(DockSide::Leading),
                },
            );
        }
        m2.import_state(&state);

        assert!(m2.is_tab_hidden(tb), "hidden activity restored");
        assert_eq!(m2.side_rail_size(DockSide::Leading), DockRailItemSize::Labeled);
        assert_eq!(m2.side_tab_display(DockSide::Leading), DockTabDisplay::Icon);
    }
}
