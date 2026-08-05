// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DockingLayout` — a VS Code-style dockable layout: a fixed centre slot
//! (the app's main content) surrounded by four collapsible, splittable,
//! draggable side regions (leading / trailing / top / bottom), backed by a
//! cloneable, serializable [`DockingModel`].
//!
//! See `docs/docking.md` for the full reference. The structure is four
//! levels deep:
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

mod a11y;
mod activity_bar;
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

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::SurfaceRole;

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

/// The docking layout widget. See the module docs and `docs/docking.md`.
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
    progress: HashMap<DockSide, bastyde_core::signal::Signal<f32>>,
    /// Per-side activity-rail configuration (size / slots / overflow).
    rails: HashMap<DockSide, DockRail>,
    /// Per-side `WidgetId` of the `DockSidePanel` content region (the
    /// `Role::Complementary` landmark), recorded in `build()`. Threaded into
    /// each side's `DockActivityBar` so its rail tabs can advertise an AT
    /// `controls` relationship pointing at the content region they govern
    /// (the ARIA tab → tabpanel link). Owned per-`DockingLayout` instance so
    /// it stays correct even if a model is shared across views.
    side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    /// Children in a fixed order so `place_children` can index them:
    /// `[center, (content, rail, handle) × {leading, trailing, top, bottom}]`.
    ordered: Vec<WidgetId>,
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
            ordered: Vec::new(),
        }
    }

    /// Configure a side's activity rail (item size, top/bottom slots, overflow
    /// trigger). The side still needs [`DockingModel::set_side_rail`] to put it
    /// in Rail presentation; this only styles the rail. See [`DockRail`].
    pub fn rail(mut self, rail: DockRail) -> Self {
        self.rails.insert(rail.side(), rail);
        self
    }

    /// Set the always-present centre content (the app's main area).
    pub fn center(mut self, widget: impl Widget + 'static) -> Self {
        self.center = Some(Box::new(widget));
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

    /// Set the centre content by a pre-registered id.
    pub fn center_id(mut self, id: WidgetId) -> Self {
        self.center_id = Some(id);
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
            ctx.add(crate::primitives::Expand::new().child_id(inner))
        };

        let mut ordered = vec![center];
        let anim = ctx.animate().collapse().standard();

        // Re-derive the side → content-region id map on every (re)build; a
        // disabled or rail-less side leaves no entry, so a rail tab simply
        // omits its `controls` relation rather than dangling at a stale id.
        self.side_panel_ids.borrow_mut().clear();

        for side in SIDES_ORDER {
            // A disabled side renders nothing and reserves no space. Push three
            // transparent placeholders so the fixed child order
            // (`[center, (content, rail, handle) × 4]`) the placement code
            // indexes by stays intact; `place_children` gives it zero extent.
            if !self.model.is_side_enabled(side) {
                let blank = || RectWidget::new().background(SurfaceRole::Transparent);
                ordered.push(ctx.add(blank()));
                ordered.push(ctx.add(blank()));
                ordered.push(ctx.add(blank()));
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
            let content = ctx.add(SideClipPane {
                side,
                model: self.model.clone(),
                child: panel,
            });

            // Rail (always present; empty when the side has no rail).
            let rail = if self.model.side_has_rail(side) {
                ctx.add(DockActivityBar::new(
                    side,
                    self.model.clone(),
                    config,
                    self.side_panel_ids.clone(),
                ))
            } else {
                ctx.add(RectWidget::new().background(SurfaceRole::Transparent))
            };

            // Resize handle (disabled when the side is hidden).
            let handle = ctx.add(DockResizeHandle::new(DockResizeHandleConfig {
                side,
                model: self.model.clone(),
                enabled: true,
                is_rtl: false,
                container_bounds: self.container_bounds.clone(),
            }));
            ctx.enabled_when(handle, visible.clone());

            ordered.push(content);
            ordered.push(rail);
            ordered.push(handle);
        }

        self.ordered = ordered.clone();
        ordered
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
            bastyde_canvas::Size::new(min_w, min_h),
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
        // [center, L(content,rail,handle), T(content,rail,handle),
        //  Top(...), Bottom(...)]
        let place = |children: &mut [WidgetPlacement], idx: usize, rect: Rect| {
            if let Some(c) = children.get_mut(idx) {
                c.origin = rect.origin();
                c.size = rect.size();
            }
        };
        place(children, 0, rects.center);
        let side_rects = [
            (leading.content, leading.rail, leading.handle),
            (trailing.content, trailing.rail, trailing.handle),
            (rects.top.content, rects.top.rail, rects.top.handle),
            (rects.bottom.content, rects.bottom.rail, rects.bottom.handle),
        ];
        for (i, (content, rail, handle)) in side_rects.into_iter().enumerate() {
            let base = 1 + i * 3;
            place(children, base, content);
            place(children, base + 1, rail);
            place(children, base + 2, handle);
        }
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
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
