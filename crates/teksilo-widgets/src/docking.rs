// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DockingLayout` — a VS Code-style dockable layout: a fixed centre slot
//! (the app's main content) surrounded by four collapsible, splittable,
//! draggable side regions (leading / trailing / top / bottom), backed by a
//! cloneable, serializable [`DockingModel`].
//!
//! The structure is four levels deep:
//!
//! ```text
//! DockingLayout
//! └── Centre + 4 Sides
//!     └── Side = [optional DockActivityBar rail] + collapsible content region
//!         └── content region holds ONE TabWidget (strip optional / replaced
//!             by the rail)
//!             └── Tab → DockArrangement (a Splitter of panes, each a single
//!                 DockWidget or a ToolBox of DockWidgets)
//!                 └── DockWidget — the atomic dockable unit
//! ```
//!
//! The centre clips its content, so nothing in it paints over a side. The
//! clip reaches a focus ring's width past the centre's edge, so a control
//! flush with that edge keeps its whole ring. Content that overflows the
//! centre can therefore paint up to that margin, about 6 dp, past it: into
//! a side's resize-handle gutter, or past the layout's own edge where a side
//! is hidden.
//!
//! # Keyboard order
//!
//! Tab moves through the dock in reading order: the top panel, the leading
//! side, the centre, the trailing side, the bottom panel. Leading and
//! trailing are logical sides, drawn mirrored in right-to-left, so the same
//! order reads from the right there. Within a side Tab takes its activity
//! bar, panel and resize handle in the order they are drawn: rail, panel,
//! handle on the leading and top sides; handle, panel, rail on the trailing
//! side; handle, rail, panel at the bottom. A screen reader's browse mode
//! and landmark list meet the sides in the same order.
//!
//! The dock is one Tab group among the window's controls, and each region
//! is a group inside it: a `tab_index` an application sets on a control in
//! a dock orders it only within that dock's region. The dock's first
//! control is the top or leading panel's, so a fallback that focuses the
//! dock's first control (a modal window with no focus hint) lands there;
//! give the editor an `initial_focus_hint` to start in the centre.
//!
//! # Revealing and focusing a dock
//!
//! [`DockingModel::reveal_dock`] shows a dock and leaves keyboard focus
//! where it is: for "show me this result", where the user keeps typing.
//! [`DockingModel::focus_dock`] is for a "focus this panel" command: it
//! reveals the dock, shows its activity and expands its pane if needed,
//! moves focus to the control that last had it inside the dock (the one at
//! the same place, when the dock's content was rebuilt since; else the
//! dock's first control), and a screen reader hears the dock's title. It
//! acts within the layout's own window; while a modal is open there it still
//! reveals the dock, but moves no focus and says nothing.

mod a11y;
mod activity_bar;
mod centre;
mod context_menu;
mod drag;
mod geometry;
mod model;
mod panel;
mod resize_handle;
mod state;
#[cfg(test)]
mod tests;

pub use activity_bar::{DockAction, DockActionId, DockActionPlacement, DockRail, DockRailSlot};
pub use geometry::{CornerOwners, DockCorner, DockSide, DockingRects, SideLayout, SideRects};
pub use model::{
    DockIconFactory, DockLoc, DockOpenLocation, DockOpenMode, DockPolicy, DockRailItemSize,
    DockTabDisplay, DockTabId, DockWidgetId, DockingModel, TabPresentation,
};
pub use panel::{DockContentFactory, DockWidget};
pub use state::{DockLayoutState, DockSideState, DockTabState};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::focus::TraversalScopePolicy;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;
use teksilo_tokens::SurfaceRole;

use crate::primitives::RectWidget;

use activity_bar::DockActivityBar;
use geometry::compute_rects;
use panel::{DockContentRegistry, DockSidePanel};
use resize_handle::{DockResizeHandle, DockResizeHandleConfig};

/// Below this collapse progress a side's content is parked dormant (out of
/// paint / focus / AT), so a fully-collapsed side never bleeds past its 0-size
/// clip. Matches the Splitter `ClipPane` epsilon.
const COLLAPSED_EPS: f32 = 0.01;
/// Default resize-gutter thickness between a side and the centre.
const DOCK_GUTTER: f32 = 6.0;

/// The docking layout widget. See the module docs.
///
/// ```ignore
/// let model = DockingModel::new();
/// // …declare panels + an initial layout on `model`…
/// DockingLayout::new(model.clone())
///     .center(editor)
///     .dock(DockWidget::new(EXPLORER, lit!("Explorer"), |_| Explorer::new()))
/// ```
pub struct DockingLayout {
    model: DockingModel,
    registry: Rc<RefCell<DockContentRegistry>>,
    center: Option<Box<dyn Widget>>,
    center_id: Option<WidgetId>,
    container_bounds: Rc<Cell<Rect>>,
    progress: HashMap<DockSide, teksilo_core::signal::Signal<f32>>,
    /// Per-side activity-rail configuration (size / slots / overflow).
    rails: HashMap<DockSide, DockRail>,
    /// Per-side `WidgetId` of the `DockSidePanel` content region (the
    /// `Role::Complementary` landmark), recorded in `build()`. Threaded into
    /// each side's `DockActivityBar` so its rail tabs can advertise an AT
    /// `controls` relationship pointing at the content region they govern
    /// (the ARIA tab → tabpanel link). Owned per-`DockingLayout` instance so
    /// it stays correct even if a model is shared across views.
    side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    /// Per-side `WidgetId` of the activity rail, recorded in `build()`: the
    /// last place [`DockingModel::focus_dock`] puts focus.
    rail_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    /// The control each dock last had focus on, so `focus_dock` can return
    /// to it. Kept across rebuilds of the dock's content (see
    /// [`RememberedFocus`]).
    focus_memory: Rc<RefCell<HashMap<DockWidgetId, RememberedFocus>>>,
    /// Bumped when focus moves inside a side, which rebuilds the courier to
    /// record where it went: focus moves in the centre cost nothing.
    focus_moved_in_side: Signal<u64>,
    /// Whether focus is inside each side region, refilled on each build.
    sides_focus_within: Rc<RefCell<Vec<Signal<bool>>>>,
    /// The focus request generation when the layout was first built: a
    /// request made before it was mounted is not applied (it would move
    /// focus long after the command that asked for it).
    mounted_at: Rc<Cell<Option<u64>>>,
    /// Children in a fixed order so `place_children` can index them:
    /// `[centre region, side region × {leading, trailing, top, bottom},
    /// focus courier]`. Each region is a Tab group (see [`DockRegion`]).
    ordered: Vec<WidgetId>,
    /// Where the centre region places its one child, written by
    /// `place_children`. Kept across rebuilds with the centre region.
    center_slot: RegionSlot,
    /// Where each side region places its rail, content and handle, in
    /// `SIDES_ORDER`, written by `place_children`.
    side_slots: Vec<RegionSlot>,
}

impl std::fmt::Debug for DockingLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockingLayout").finish()
    }
}

impl DockingLayout {
    /// Create a docking layout over a model.
    pub fn new(model: DockingModel) -> Self {
        Self {
            model,
            registry: Rc::new(RefCell::new(DockContentRegistry::default())),
            center: None,
            center_id: None,
            container_bounds: Rc::new(Cell::new(Rect::ZERO)),
            progress: HashMap::new(),
            rails: HashMap::new(),
            side_panel_ids: Rc::new(RefCell::new(HashMap::new())),
            rail_ids: Rc::new(RefCell::new(HashMap::new())),
            focus_memory: Rc::new(RefCell::new(HashMap::new())),
            focus_moved_in_side: Signal::new(0),
            sides_focus_within: Rc::new(RefCell::new(Vec::new())),
            mounted_at: Rc::new(Cell::new(None)),
            ordered: Vec::new(),
            center_slot: RegionSlot::default(),
            side_slots: Vec::new(),
        }
    }

    /// Configure a side's activity rail (item size, top/bottom slots, overflow
    /// trigger). The side still needs [`DockingModel::set_side_rail`] to put it
    /// in Rail presentation; this only styles the rail. See [`DockRail`].
    pub fn rail(mut self, rail: DockRail) -> Self {
        self.rails.insert(rail.side(), rail);
        self
    }

    /// Configure several sides' activity rails from an iterator.
    ///
    /// The loop form of [`rail`](Self::rail). Each rail carries its own side, so
    /// a later entry for a side already configured replaces it.
    pub fn rails(self, rails: impl IntoIterator<Item = DockRail>) -> Self {
        rails.into_iter().fold(self, Self::rail)
    }

    /// Set the always-present centre content (the app's main area).
    pub fn center(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(widget) {
            teksilo_core::PendingChild::Id(id) => {
                self.center_id = Some(id);
                self
            }
            teksilo_core::PendingChild::Deferred(w) => {
                self.center = Some(w);
                self
            }
        }
    }

    /// Name a side region for assistive technology: its `Role::Complementary`
    /// landmark and, from it, its activity bar, its action groups and its
    /// entry in the "Move to" menus. Replaces the stock "Left panel", "Right
    /// panel", "Top panel" and "Bottom panel", which follow where the side is
    /// on screen (mirrored in right-to-left). A `tr!` label follows a locale
    /// switch like any other. Set at construction; not persisted, like
    /// [`DockPolicy`].
    pub fn side_label(self, side: DockSide, label: impl Into<LocalizedString>) -> Self {
        self.model.set_app_side_label(side, label.into());
        self
    }

    /// Lock down end-user layout edits (sugar for [`DockingModel::set_policy`]).
    /// See [`DockPolicy`].
    pub fn policy(self, policy: DockPolicy) -> Self {
        self.model.set_policy(policy);
        self
    }

    /// Disable a side (sugar for [`DockingModel::set_side_enabled`]`(side, false)`):
    /// it renders nothing, reserves no space, and rejects docks.
    pub fn disable_side(self, side: DockSide) -> Self {
        self.model.set_side_enabled(side, false);
        self
    }

    /// Declare a dock widget (its content factory + chrome metadata). The
    /// dock is registered immediately, so the app may set the initial layout
    /// on the model (`open_dock` / `import_state`) before mounting.
    pub fn dock(self, dock: DockWidget) -> Self {
        let (id, meta, factory) = dock.into_parts();
        self.model.register_meta(id, meta);
        self.registry.borrow_mut().insert(id, factory);
        self
    }

    /// Declare several dock widgets from an iterator, in order.
    ///
    /// The loop form of [`dock`](Self::dock), and the usual one once an app has
    /// more than a couple of panels to register.
    pub fn docks(self, docks: impl IntoIterator<Item = DockWidget>) -> Self {
        docks.into_iter().fold(self, Self::dock)
    }
}

const SIDES_ORDER: [DockSide; 4] = [
    DockSide::Leading,
    DockSide::Trailing,
    DockSide::Top,
    DockSide::Bottom,
];

impl Widget for DockingLayout {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();

        // The dock is one Tab group among the window's controls, at its own
        // place in the tree. Without it the regions' `tab_index` below would
        // be sorted in the window's list, ahead of every control there.
        ctx.set_traversal_scope(TraversalScopePolicy::Continue);

        // Structural change → Rebuild; geometry change → Relayout.
        self.model
            .version()
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);
        self.model.geometry_version().bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );

        // Content is built **in-context** by each side's panels (via the
        // registry handle passed down) — never pre-built here, so it is
        // correctly parented where it is placed. (v1: rebuilt on each
        // structural change; the Rebuild/Relayout split keeps resize / show-
        // hide / tab-switch from rebuilding.)

        // Centre preservation across rebuilds. `self.center` is a one-shot
        // `take()`, so a rebuild (a rail / dock / side change re-runs `build()`)
        // would otherwise find it `None` and fall back to a blank placeholder —
        // blanking the editor. `preserves_children_on_rebuild()` (below) stops
        // the framework from auto-destroying our children on a rebuild, so we
        // manage them here: keep the centre subtree (index 0) and destroy +
        // rebuild only the model-derived sides.
        let prior = std::mem::take(&mut self.ordered);
        let preserved_center = prior.first().copied();
        for &old_side in prior.iter().skip(1) {
            ctx.destroy_subtree(old_side);
        }

        // Centre.
        let center = if let Some(c) = preserved_center {
            c
        } else {
            let inner = if let Some(id) = self.center_id {
                id
            } else if let Some(w) = self.center.take() {
                ctx.add_boxed(w)
            } else {
                ctx.add(RectWidget::new().background(SurfaceRole::Content))
            };
            // The centre clips its content, so whatever it holds (a
            // `Splitter` too small for its panes' minimums, an oversized
            // child) cannot paint over a side; the clip reaches past the
            // edge by a focus ring's width, so a control flush with the
            // edge keeps its ring (see `centre`).
            let frame = ctx.add(centre::CentreFrame::new(inner));
            ctx.add(
                DockRegion {
                    children: vec![frame],
                    slot: self.center_slot.clone(),
                    forwards_response: true,
                }
                .tab_index(region_tab_index(None)),
            )
        };

        // The build order stays centre first, then the sides: the hit walk
        // tries later siblings first, so a side's handle grip, which reaches
        // over the centre, is tried before the centre; and the centre's
        // preservation across rebuilds relies on index 0. Tab and the
        // accessibility tree read in another order, by `tab_index` and
        // `accessibility_children`.
        let mut ordered = vec![center];
        self.side_slots.clear();
        let anim = ctx.animate().collapse().standard();

        // Re-derive the side → content-region id map on every (re)build; a
        // disabled or rail-less side leaves no entry, so a rail tab simply
        // omits its `controls` relation rather than dangling at a stale id.
        self.side_panel_ids.borrow_mut().clear();
        self.rail_ids.borrow_mut().clear();
        if self.mounted_at.get().is_none() {
            self.mounted_at
                .set(Some(self.model.focus_request_signal().get()));
        }
        self.registry
            .borrow_mut()
            .reset_focus_targets(self.model.version().get());

        // Remember where focus goes in a side, for `focus_dock` to return a
        // dock to the control it had. An effect has no tree to tell which dock
        // a widget is in, so it only bumps a signal that rebuilds the courier,
        // which records it (see `DockFocusCourier::record`). A move between
        // two controls of one side keeps its region's `focus_within` true, so
        // the focused widget is watched too; a move from the centre into a
        // side flips the region's flag, which `add_side_region` watches. The
        // flag still reads its old value when the focused widget changes.
        self.sides_focus_within.borrow_mut().clear();
        {
            let within = self.sides_focus_within.clone();
            let moved = self.focus_moved_in_side.clone();
            let focused = ctx.focused_signal();
            ctx.effect(&focused, move |_| {
                if within.borrow().iter().any(Signal::get) {
                    moved.set(moved.get().wrapping_add(1));
                }
            });
        }

        for side in SIDES_ORDER {
            // A disabled side renders nothing and reserves no space. Push three
            // transparent placeholders so the fixed child order
            // (`[center, (content, rail, handle) × 4]`) the placement code
            // indexes by stays intact; `place_children` gives it zero extent.
            if !self.model.is_side_enabled(side) {
                let blank = || RectWidget::new().background(SurfaceRole::Transparent);
                let children = vec![ctx.add(blank()), ctx.add(blank()), ctx.add(blank())];
                ordered.push(self.add_side_region(ctx, side, children));
                continue;
            }

            let visible = self.model.side_visible_signal(side);
            let progress = ctx.animated_signal(if visible.get() { 1.0 } else { 0.0 });
            progress.bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
            self.progress.insert(side, progress.clone());

            // A rail size-mode change (Default / Compact / Labeled) changes the
            // rail strip's width → relayout so the activity bar itself follows
            // the switch, not just its items.
            self.model.rail_size_signal(side).bind_to(
                self_id,
                ctx.binding_registry(),
                BindingLevel::Relayout,
            );

            // Animate progress toward the side's visibility.
            {
                let spec = anim.clone();
                let p = progress.clone();
                ctx.effect(&visible, move |&v| {
                    spec.to_or_snap(&p, if v { 1.0 } else { 0.0 });
                });
            }

            // Content is laid out at full size and clipped (sliding out the
            // side's outer edge) by `SideClipPane` — never reflowed at the
            // shrinking width, so the collapse animation costs nothing per
            // frame beyond moving + clipping. Disabled when hidden so Tab
            // skips it; gate on `visible` (one change per toggle), never on the
            // per-frame `progress` signal.
            // One rail config per side, shared by both presentations: the Rail
            // half (items, slots, actions) is `DockActivityBar`'s, the Strip
            // half (`leading_slot`/`trailing_slot`) is `DockSidePanel`'s. Built
            // once here so a side declared with `.rail(..)` keeps its chrome
            // whichever presentation it is currently in.
            let config = self
                .rails
                .get(&side)
                .cloned()
                .unwrap_or_else(|| DockRail::new(side));
            let panel = ctx.add(DockSidePanel::new(
                side,
                self.model.clone(),
                self.registry.clone(),
                config.clone(),
            ));
            // Record the content region's id so this side's rail tabs can
            // advertise `controls` → this panel (ARIA tab → tabpanel link).
            self.side_panel_ids.borrow_mut().insert(side, panel);
            // Park the content dormant (out of paint/focus/AT) once the side is
            // fully collapsed, so it never bleeds past its 0-size clip. This is
            // `visible_when` (dormancy toggled only on the flip) — NOT
            // `enabled_when` (which would repaint the subtree every frame).
            ctx.visible_when(panel, progress.map(|p| *p > COLLAPSED_EPS));
            let content = ctx.add(
                SideClipPane {
                    side,
                    model: self.model.clone(),
                    child: panel,
                }
                .tab_index(side_part_tab_index(side, SidePart::Content)),
            );

            // Rail (always present; empty when the side has no rail).
            let rail = if self.model.side_has_rail(side) {
                let rail = ctx.add(
                    DockActivityBar::new(
                        side,
                        self.model.clone(),
                        config,
                        self.side_panel_ids.clone(),
                    )
                    .tab_index(side_part_tab_index(side, SidePart::Rail)),
                );
                self.rail_ids.borrow_mut().insert(side, rail);
                rail
            } else {
                ctx.add(RectWidget::new().background(SurfaceRole::Transparent))
            };

            // Resize handle (disabled when the side is hidden).
            let handle = ctx.add(
                DockResizeHandle::new(DockResizeHandleConfig {
                    side,
                    model: self.model.clone(),
                    enabled: true,
                    is_rtl: false,
                    container_bounds: self.container_bounds.clone(),
                })
                .tab_index(side_part_tab_index(side, SidePart::Handle)),
            );
            ctx.enabled_when(handle, visible.clone());

            // Built rail, content, handle on every side (the region places
            // and hit-tests them in that order); Tab takes them in the order
            // the side is drawn in, by `side_part_tab_index`.
            ordered.push(self.add_side_region(ctx, side, vec![rail, content, handle]));
        }

        // Last, so the sides it moves focus into are built when it builds.
        ordered.push(ctx.add(DockFocusCourier {
            model: self.model.clone(),
            registry: self.registry.clone(),
            side_panel_ids: self.side_panel_ids.clone(),
            rail_ids: self.rail_ids.clone(),
            memory: self.focus_memory.clone(),
            focus_moved_in_side: self.focus_moved_in_side.clone(),
            mounted_at: self.mounted_at.clone(),
        }));

        self.ordered = ordered.clone();
        ordered
    }

    /// The regions in reading order, the order Tab takes them in: what a
    /// screen reader's browse mode and landmark list meet first must be what
    /// Tab reaches first (WCAG 1.3.2). The arena keeps the build order.
    fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        if self.ordered.len() < 1 + SIDES_ORDER.len() {
            return None;
        }
        let side = |s: DockSide| {
            let i = SIDES_ORDER
                .iter()
                .position(|&x| x == s)
                .expect("SIDES_ORDER holds every side");
            self.ordered[1 + i]
        };
        let mut out = vec![
            side(DockSide::Top),
            side(DockSide::Leading),
            self.ordered[0],
            side(DockSide::Trailing),
            side(DockSide::Bottom),
        ];
        out.extend(self.ordered.iter().skip(1 + SIDES_ORDER.len()).copied());
        Some(out)
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Min = Σ visible side (rail + min + gutter) + centre child min.
        let mut min_w = 0.0_f32;
        let mut min_h = 0.0_f32;
        if let Some(&center) = self.ordered.first()
            && let Some(c) = ctx.child_size(
                center,
                SizeProposal {
                    width: None,
                    height: None,
                },
            )
        {
            min_w += c.width.min(80.0);
            min_h += c.height.min(80.0);
        }
        for side in SIDES_ORDER {
            if self.model.is_side_enabled(side) && self.model.is_side_visible(side) {
                let extent = self.model.side_min_size(side)
                    + DOCK_GUTTER
                    + self.model.side_rail_thickness(side);
                if side.is_horizontal_axis() {
                    min_w += extent;
                } else {
                    min_h += extent;
                }
            }
        }
        LayoutResponse::shrinkable(
            proposal.resolve(min_w, min_h),
            teksilo_canvas::Size::new(min_w, min_h),
            1.0,
        )
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.container_bounds.set(bounds);
        let rtl = ctx.is_rtl();
        // The one direction every side name reads (see `a11y`), the same
        // value the geometry swap below uses.
        self.model.set_layout_rtl(rtl);

        let side_layout = |side: DockSide| -> SideLayout {
            // A disabled side contributes nothing (its placeholders are placed
            // at the zero rect compute_rects returns; the centre reclaims it).
            if !self.model.is_side_enabled(side) {
                return SideLayout {
                    size: 0.0,
                    visible_progress: 0.0,
                    gutter: DOCK_GUTTER,
                    min_size: 0.0,
                    rail_thickness: 0.0,
                    has_rail: false,
                };
            }
            let p = self.progress.get(&side).map(|s| s.get()).unwrap_or(
                if self.model.is_side_visible(side) {
                    1.0
                } else {
                    0.0
                },
            );
            // The rail strip width follows the side's size mode (it shrinks for
            // Compact), derived from the rail's configured item size.
            let rail_thickness = if self.model.side_has_rail(side) {
                let mode = self.model.side_rail_size(side);
                self.rails
                    .get(&side)
                    .map(|r| r.effective_thickness(mode))
                    .unwrap_or_else(|| DockRail::new(side).effective_thickness(mode))
            } else {
                0.0
            };
            SideLayout {
                size: self.model.side_size(side),
                visible_progress: p,
                gutter: DOCK_GUTTER,
                min_size: self.model.side_min_size(side),
                rail_thickness,
                has_rail: self.model.side_has_rail(side),
            }
        };

        // RTL: swap leading/trailing inputs, then swap the outputs back.
        let (lead_in, trail_in) = if rtl {
            (
                side_layout(DockSide::Trailing),
                side_layout(DockSide::Leading),
            )
        } else {
            (
                side_layout(DockSide::Leading),
                side_layout(DockSide::Trailing),
            )
        };
        let rects = compute_rects(
            bounds,
            lead_in,
            trail_in,
            side_layout(DockSide::Top),
            side_layout(DockSide::Bottom),
            self.model.corners(),
            rtl,
        );
        let leading = if rtl { rects.trailing } else { rects.leading };
        let trailing = if rtl { rects.leading } else { rects.trailing };

        // children order matches `self.ordered`:
        // [centre region, Leading, Trailing, Top, Bottom regions, courier].
        // Each region is placed at the union of the rects of what it holds
        // (never over a neighbour, which would take its hits and drops), and
        // places those children at the rects computed here.
        let place = |children: &mut [WidgetPlacement], idx: usize, rect: Rect| {
            if let Some(c) = children.get_mut(idx) {
                c.origin = rect.origin();
                c.size = rect.size();
            }
        };
        let nowhere = Rect::new(bounds.x, bounds.y, 0.0, 0.0);
        self.center_slot.set(vec![rects.center]);
        place(children, 0, rects.center);
        let side_rects = [
            (leading.rail, leading.content, leading.handle),
            (trailing.rail, trailing.content, trailing.handle),
            (rects.top.rail, rects.top.content, rects.top.handle),
            (rects.bottom.rail, rects.bottom.content, rects.bottom.handle),
        ];
        for (i, (rail, content, handle)) in side_rects.into_iter().enumerate() {
            let held = [rail, content, handle];
            if let Some(slot) = self.side_slots.get(i) {
                slot.set(held.to_vec());
            }
            // The handle's touch grip reaches past its bounds over the
            // content beside it (`DockResizeHandle::hit_outset`), and a hit
            // outset never leaves its parent: the region takes in the
            // widest grip, and passes every other point through.
            let horizontal = SIDES_ORDER[i].is_horizontal_axis();
            let thickness = if horizontal {
                handle.width
            } else {
                handle.height
            };
            let out = resize_handle::grab_outset(
                thickness,
                teksilo_tokens::PointerKind::Touch,
                &ctx.theme.input,
            );
            let grip = if horizontal {
                Rect::new(
                    handle.x - out,
                    handle.y,
                    handle.width + 2.0 * out,
                    handle.height,
                )
            } else {
                Rect::new(
                    handle.x,
                    handle.y - out,
                    handle.width,
                    handle.height + 2.0 * out,
                )
            };
            place(
                children,
                1 + i,
                union_of(&[rail, content, handle, grip]).unwrap_or(nowhere),
            );
        }
        // The focus courier, last: no extent, so it is never hit.
        place(children, 1 + SIDES_ORDER.len(), nowhere);
    }

    fn clips_children(&self) -> bool {
        true
    }

    /// We manage our own children across rebuilds (see `build`): the centre is
    /// a one-shot passed-in widget that must survive structural rebuilds, so we
    /// preserve it and explicitly destroy + rebuild only the model-derived
    /// sides. Without this the framework auto-destroys every child on rebuild,
    /// and the centre (already `take()`n) falls back to a blank placeholder.
    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.ordered.clone()
    }
}

/// Wraps a side's content: lays it out at its **full** size and clips, so a
/// collapsing side **slides its content out** the outer edge instead of
/// reflowing it at the shrinking width (the Splitter `ClipPane` trick). The
/// child's layout stays at a stable full size every frame — the animation
/// only moves + clips.
struct SideClipPane {
    side: DockSide,
    model: DockingModel,
    child: WidgetId,
}

impl std::fmt::Debug for SideClipPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SideClipPane")
            .field("side", &self.side)
            .finish()
    }
}

impl Widget for SideClipPane {
    /// A Tab group of its own, so the side's region can order the panel
    /// against its rail and handle (`side_part_tab_index`), and a
    /// `tab_index` set inside the panel orders only within it.
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.set_traversal_scope(TraversalScopePolicy::Continue);
        vec![self.child]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        // Measure the child at the side's FULL extent (not the shrinking
        // proposal), so its whole subtree lays out at full size — content fills
        // the dock width, and it stays stable across the collapse (full size
        // doesn't change), so there's still no per-frame reflow. We then report
        // the proposal size (the orchestrator forces our actual bounds).
        let full = self.model.side_size(self.side).max(0.0);
        let full_proposal = if self.side.is_horizontal_axis() {
            SizeProposal {
                width: Some(full.max(proposal.width.unwrap_or(0.0))),
                height: proposal.height,
            }
        } else {
            SizeProposal {
                width: proposal.width,
                height: Some(full.max(proposal.height.unwrap_or(0.0))),
            }
        };
        let _ = ctx.child_size(self.child, full_proposal);
        proposal
            .resolve(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Full main extent = the side's stored size (≥ the current, shrinking
        // bounds). Anchor the content's INNER edge to the bounds' inner edge so
        // it slides out the OUTER edge as the side collapses.
        let full = self.model.side_size(self.side).max(0.0);
        let (size, origin) = match self.side {
            DockSide::Leading => {
                let w = full.max(bounds.width);
                (
                    Size::new(w, bounds.height),
                    Point::new(bounds.x + bounds.width - w, bounds.y),
                )
            }
            DockSide::Trailing => {
                let w = full.max(bounds.width);
                (Size::new(w, bounds.height), Point::new(bounds.x, bounds.y))
            }
            DockSide::Top => {
                let h = full.max(bounds.height);
                (
                    Size::new(bounds.width, h),
                    Point::new(bounds.x, bounds.y + bounds.height - h),
                )
            }
            DockSide::Bottom => {
                let h = full.max(bounds.height);
                (Size::new(bounds.width, h), Point::new(bounds.x, bounds.y))
            }
        };
        for child in children.iter_mut() {
            child.origin = origin;
            child.size = size;
        }
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.child]
    }
}

/// Applies [`DockingModel::focus_dock`]: a zero-size child, last in the
/// layout's order, rebuilt alone by each request.
///
/// It decides in a `build()`, after the sides it moves focus into: a request
/// that also revealed a hidden side rebuilds the whole layout, which adds a
/// new courier after the new sides; a request for a dock already shown
/// rebuilds only this. An effect could not do it: it fires as the request is
/// made, before the rebuild, at widgets the rebuild destroys.
///
/// It moves focus after the build, from a post-mount action: a side built in
/// this pass is linked under the layout only once the layout's build
/// returns, and focus moved before that would leave `focus_within` false on
/// the layout and everything above it.
///
/// It also rebuilds when focus moves inside a side, to record which control
/// of which dock has it ([`record`](Self::record)): the one place with a tree
/// to ask, while the control is still shown.
struct DockFocusCourier {
    model: DockingModel,
    registry: Rc<RefCell<panel::DockContentRegistry>>,
    side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    rail_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    memory: Rc<RefCell<HashMap<DockWidgetId, RememberedFocus>>>,
    focus_moved_in_side: Signal<u64>,
    mounted_at: Rc<Cell<Option<u64>>>,
}

/// The control a dock last had focus on: the widget, and its place among the
/// dock content's controls in Tab order. Revealing a hidden side rebuilds its
/// docks, which destroys the widget; the place finds the control the rebuilt
/// content has there.
#[derive(Debug, Clone, Copy)]
struct RememberedFocus {
    control: WidgetId,
    ordinal: usize,
}

impl std::fmt::Debug for DockFocusCourier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockFocusCourier").finish()
    }
}

impl DockFocusCourier {
    /// Remember the focused control's place in the dock that holds it. A
    /// control no dock holds, or one that cannot take focus now (in a side
    /// being hidden), leaves every dock's memory as it was.
    fn record(&self, ctx: &BuildContext) {
        let Some(focused) = ctx.focused_signal().get() else {
            return;
        };
        let registry = self.registry.borrow();
        let Some((dock, target)) = registry
            .focus_targets()
            .find(|(_, target)| ctx.is_descendant_of(focused, target.content))
        else {
            return;
        };
        if let Some(ordinal) = ctx
            .focusables_in_tab_order(target.content)
            .iter()
            .position(|&w| w == focused)
        {
            self.memory.borrow_mut().insert(
                dock,
                RememberedFocus {
                    control: focused,
                    ordinal,
                },
            );
        }
    }

    /// The control to return to in `dock`, whose content is `content`: the
    /// remembered one while it lives and can take focus; once it is gone (the
    /// content was rebuilt), the control at its place; else none.
    fn remembered(
        &self,
        ctx: &BuildContext,
        dock: DockWidgetId,
        content: WidgetId,
    ) -> Option<WidgetId> {
        let memory = self.memory.borrow().get(&dock).copied()?;
        let controls = ctx.focusables_in_tab_order(content);
        if ctx.is_descendant_of(memory.control, content) {
            // Still there: itself, or the first control when it is disabled
            // or parked now (the next one would be a guess).
            controls.contains(&memory.control).then_some(memory.control)
        } else {
            controls.get(memory.ordinal).copied()
        }
    }

    fn apply(&self, ctx: &mut BuildContext) {
        let Some((dock, generation)) = self.model.pending_focus_request() else {
            return;
        };
        // Asked for before this layout was mounted: dropped, not applied
        // whenever the layout happens to appear.
        if self.mounted_at.get().is_some_and(|at| generation <= at) {
            self.model.take_focus_request(generation);
            return;
        }
        let version = self.model.version().get();
        let (target, current) = {
            let registry = self.registry.borrow();
            (
                registry.focus_target(dock, version),
                registry.focus_targets_version() == version,
            )
        };
        let Some(target) = target else {
            // Built before a change that is about to rebuild the layout: the
            // courier the rebuild adds applies the request. Built at this
            // version with no such dock: nothing will ever take it.
            if current {
                self.model.take_focus_request(generation);
            }
            return;
        };
        let remembered = self.remembered(ctx, dock, target.content);
        // Where focus goes, and whether the title still needs saying there:
        // a split pane's header, a tab or a rail item is named by it already.
        let (to, announce) = if let Some(control) = remembered {
            (Some(control), true)
        } else if let Some(first) = ctx.first_focusable_descendant(target.content) {
            (Some(first), true)
        } else if let Some(first) = ctx.first_focusable_descendant(target.pane) {
            (Some(first), !target.titled_header)
        } else {
            let side = self.model.dock_location(dock).map(|l| l.side);
            let side_panel = side.and_then(|s| self.side_panel_ids.borrow().get(&s).copied());
            let rail = side.and_then(|s| self.rail_ids.borrow().get(&s).copied());
            match [side_panel, rail]
                .into_iter()
                .flatten()
                .find_map(|region| ctx.first_focusable_descendant(region))
            {
                Some(first) => (Some(first), false),
                // Nothing to focus: focus stays, and the title is the one
                // thing that says the panel came up.
                None => (None, true),
            }
        };
        self.model.take_focus_request(generation);
        let title = announce
            .then(|| {
                self.model.dock_title(dock).or_else(|| {
                    let tab = self.model.activity_of(dock)?;
                    let (_, view) = self.model.tab_view_by_id(tab)?;
                    Some(self.model.activity_label(&view))
                })
            })
            .flatten();
        ctx.run_after_mount(move |ectx| {
            // Refused under an open modal: focus there would sit under its
            // scrim, in a page the user cannot reach.
            if ectx.is_modal_open() {
                return;
            }
            if let Some(control) = to {
                ectx.request_focus(control);
            }
            // Held by the announcer until focus has landed, so it is heard
            // after the control.
            if let Some(title) = title {
                let text = title.resolve_now();
                if !text.is_empty() {
                    ectx.announce(text);
                }
            }
        });
    }
}

impl Widget for DockFocusCourier {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.model.focus_request_signal().bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        self.focus_moved_in_side.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );
        self.record(ctx);
        self.apply(ctx);
        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::ZERO.into()
    }
}

/// Where a [`DockRegion`] places its children, in their order. Written by
/// `DockingLayout::place_children`, which computes every rect; read by the
/// region's own `place_children` in the same pass, parent first.
#[derive(Clone, Default)]
struct RegionSlot(Rc<RefCell<Vec<Rect>>>);

impl RegionSlot {
    fn set(&self, rects: Vec<Rect>) {
        *self.0.borrow_mut() = rects;
    }
    fn get(&self, i: usize) -> Option<Rect> {
        self.0.borrow().get(i).copied()
    }
}

/// The smallest rect holding every non-empty rect of `rects`, if any.
fn union_of(rects: &[Rect]) -> Option<Rect> {
    rects
        .iter()
        .filter(|r| r.width > 0.0 && r.height > 0.0)
        .fold(None, |acc: Option<Rect>, r| {
            Some(match acc {
                None => *r,
                Some(a) => {
                    let x = a.x.min(r.x);
                    let y = a.y.min(r.y);
                    let right = (a.x + a.width).max(r.x + r.width);
                    let bottom = (a.y + a.height).max(r.y + r.height);
                    Rect::new(x, y, right - x, bottom - y)
                }
            })
        })
}

/// The three parts of a side's region.
#[derive(Clone, Copy)]
enum SidePart {
    Rail,
    Content,
    Handle,
}

/// A part's place in its side's Tab order: the order the side is drawn in,
/// read left to right and top to bottom (mirrored in right-to-left, where the
/// leading side is drawn on the right with its rail outermost).
///
/// - Leading: the rail on the outer edge, the panel, the handle by the centre.
/// - Top: the rail (a column on the leading edge), the panel, the handle
///   drawn below it.
/// - Trailing: the handle by the centre, the panel, the rail on the outer edge.
/// - Bottom: the handle drawn above, then the rail and the panel.
fn side_part_tab_index(side: DockSide, part: SidePart) -> i32 {
    use SidePart::{Content, Handle, Rail};
    let order: [SidePart; 3] = match side {
        DockSide::Leading | DockSide::Top => [Rail, Content, Handle],
        DockSide::Trailing => [Handle, Content, Rail],
        DockSide::Bottom => [Handle, Rail, Content],
    };
    let at = order
        .iter()
        .position(|p| std::mem::discriminant(p) == std::mem::discriminant(&part))
        .expect("every part is in the order");
    at as i32 + 1
}

/// A region's place in the dock's Tab order: top, leading side, centre,
/// trailing side, bottom. Leading and trailing are logical, already drawn
/// mirrored in right-to-left, so the same table reads right to left there.
fn region_tab_index(side: Option<DockSide>) -> i32 {
    match side {
        Some(DockSide::Top) => 1,
        Some(DockSide::Leading) => 2,
        None => 3,
        Some(DockSide::Trailing) => 4,
        Some(DockSide::Bottom) => 5,
    }
}

impl DockingLayout {
    /// Wrap a side's `[rail, content, handle]` in its Tab group.
    fn add_side_region(
        &mut self,
        ctx: &mut BuildContext,
        side: DockSide,
        children: Vec<WidgetId>,
    ) -> WidgetId {
        let slot = RegionSlot::default();
        self.side_slots.push(slot.clone());
        let within = Signal::new(false);
        {
            let moved = self.focus_moved_in_side.clone();
            ctx.effect(&within, move |&inside| {
                if inside {
                    moved.set(moved.get().wrapping_add(1));
                }
            });
        }
        self.sides_focus_within.borrow_mut().push(within.clone());
        ctx.add(
            DockRegion {
                children,
                slot,
                forwards_response: false,
            }
            .tab_index(region_tab_index(Some(side)))
            .focus_within(within),
        )
    }
}

/// One region of the dock (the centre, or a side's rail, content and
/// handle) as a Tab group of its own.
///
/// The dock orders its regions by `tab_index`, so Tab reads top, leading,
/// centre, trailing, bottom whatever order they are built in, and a
/// `tab_index` an application sets inside a dock orders only within that
/// dock's region. The region has no layout of its own (the dock computes
/// every rect) and no accessibility node a reader keeps: a property-free
/// `GenericContainer`, which the walker prunes.
struct DockRegion {
    children: Vec<WidgetId>,
    slot: RegionSlot,
    /// The centre region reports its child's full response (flex, shrink,
    /// min), which the dock measures; a side region reports its proposal.
    forwards_response: bool,
}

impl std::fmt::Debug for DockRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockRegion").finish_non_exhaustive()
    }
}

impl Widget for DockRegion {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.set_traversal_scope(TraversalScopePolicy::Continue);
        // A point no child takes falls through to the neighbour below, so
        // the region never takes a hit or a drop meant for the centre.
        ctx.apply_self_handlers(
            teksilo_core::widget_builder::HandlerSet::new().event_pass_through(true),
        );
        self.children.clone()
    }

    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if self.forwards_response
            && let Some(&child) = self.children.first()
            && let Some(response) = ctx.child_layout_response(child, proposal)
        {
            return response;
        }
        proposal
            .resolve(
                proposal.width.unwrap_or(0.0),
                proposal.height.unwrap_or(0.0),
            )
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for (i, child) in children.iter_mut().enumerate() {
            let rect = self
                .slot
                .get(i)
                .unwrap_or_else(|| Rect::new(bounds.x, bounds.y, 0.0, 0.0));
            child.origin = rect.origin();
            child.size = rect.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }
}
