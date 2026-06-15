// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DockActivityBar` — the tailored VS Code-style **vertical** icon rail. One
//! item per tab of a side; clicking an inactive item selects + shows the side,
//! clicking the active item hides the side. Always visible (it lives in the
//! layout chrome, outboard of the collapsible content), so it is the reopen
//! affordance while the side is hidden.
//!
//! Features (configured via [`DockRail`]):
//! - **Vertical only** — a column of items, pushed to the **top**.
//! - **Selectable item size** ([`IconButtonSize`]) — one size for all items.
//! - **`top_slot` / `bottom_slot`** — fixed widgets pinned above the items and
//!   at the very bottom of the rail (e.g. a logo on top, settings/account at
//!   the bottom, the VS Code convention).
//! - **Overflow** — when the items don't all fit, the surplus are parked
//!   dormant and reached through a caller-chosen **overflow item** (an icon)
//!   that opens a popover list of the overflowed entries.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use bastyde_canvas::{Canvas, Rect, SizeProposal};
use bastyde_core::{DragPayload, DropFeedback};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::gesture::DragPhase;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, lit};
use bastyde_tokens::{BorderRole, HAlignment, SurfaceRole, TextRole, TextStyleRole};

use crate::icon_button::{IconButton, IconButtonSize};
use crate::popover_widget::PopoverIconButton;
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack,
};
use crate::tool_box::RotatedLabel;

use super::context_menu::{DockMenuKind, activity_context_menu, background_menu};
use super::drag::{DockTabDragData, dropped_dock_tab, dropped_dock_widget};
use super::geometry::DockSide;
use super::model::{DockIconFactory, DockRailItemSize, DockTabId, DockingModel};

/// Shared sink each rail item upserts its `(visible position, world bounds)`
/// into during layout, so the bar's drop handler can compute an insertion
/// index from the pointer position. Keyed by visible position so a stale entry
/// for a now-overflowed item is filtered out (the handler only considers
/// positions below the current shown count).
type RailItemBounds = Rc<RefCell<Vec<(usize, Rect)>>>;

/// Factory for a rail slot widget (rebuilt on each rail rebuild).
///
/// A slot that wants to match the rail's current item size binds
/// [`DockingModel::rail_size_mode_signal`](super::DockingModel::rail_size_mode_signal)
/// — the rail rebuilds its slots whenever the size mode changes, so reading the
/// signal in the factory is enough to keep the slot in step.
pub type DockRailSlot = Rc<dyn Fn() -> Box<dyn Widget>>;

/// Map an [`IconButtonSize`] to the rail item's square extent (dp).
fn item_extent(size: IconButtonSize) -> f32 {
    use crate::styles::recipe_icon_button_style::*;
    match size {
        IconButtonSize::Compact => ICON_BUTTON_SIZE_COMPACT,
        IconButtonSize::Default => ICON_BUTTON_SIZE_DEFAULT,
        IconButtonSize::Toolbar => ICON_BUTTON_SIZE_TOOLBAR,
        IconButtonSize::Large => ICON_BUTTON_SIZE_LARGE,
        IconButtonSize::Hero => ICON_BUTTON_SIZE_HERO,
    }
}

/// Spacing between rail items.
const RAIL_ITEM_SPACING: f32 = 2.0;
/// Padding around the rail's item column.
const RAIL_PADDING: f32 = 4.0;
/// Rough vertical room a Labeled item's rotated title needs beyond its icon
/// square, used only by the overflow capacity estimate.
const LABELED_TITLE_ALLOWANCE: f32 = 72.0;
/// Top breathing room above a Labeled item's rotated title (so its top
/// character isn't flush against the rail item's top edge).
const LABELED_TOP_MARGIN: f32 = 6.0;

// ───────────────────────────────────────────────────────────────────────
// DockRail — app-facing configuration of a side's activity rail.
// ───────────────────────────────────────────────────────────────────────

/// App-facing configuration for a side's activity rail (Rail presentation).
///
/// Pass to [`DockingLayout::rail`](super::DockingLayout::rail). All knobs are
/// optional; an unconfigured rail uses [`IconButtonSize::Large`] items, no
/// slots, and no overflow affordance (items just clip if the side is too
/// short).
#[derive(Clone)]
pub struct DockRail {
    pub(crate) side: DockSide,
    pub(crate) size: IconButtonSize,
    pub(crate) top_slot: Option<DockRailSlot>,
    pub(crate) bottom_slot: Option<DockRailSlot>,
    pub(crate) overflow_icon: Option<DockIconFactory>,
}

impl std::fmt::Debug for DockRail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockRail")
            .field("side", &self.side)
            .field("size", &self.size)
            .finish()
    }
}

impl DockRail {
    /// Configure the rail for `side`.
    pub fn new(side: DockSide) -> Self {
        Self {
            side,
            size: IconButtonSize::Large,
            top_slot: None,
            bottom_slot: None,
            overflow_icon: None,
        }
    }

    /// Pick one size for every rail item ([`IconButtonSize::Compact`] …
    /// [`Hero`](IconButtonSize::Hero)). Default [`IconButtonSize::Large`].
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Widget pinned **above** the items (e.g. a logo / hamburger). To track the
    /// rail's item size, bind
    /// [`DockingModel::rail_size_mode_signal`](super::DockingModel::rail_size_mode_signal)
    /// inside the factory.
    pub fn top_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.top_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }

    /// Widget pinned at the **bottom** of the rail (e.g. settings / account). To
    /// track the rail's item size, bind
    /// [`DockingModel::rail_size_mode_signal`](super::DockingModel::rail_size_mode_signal)
    /// inside the factory.
    pub fn bottom_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.bottom_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }

    /// Choose the glyph for the overflow trigger — the item shown (in place of
    /// the surplus items) when they don't all fit. Tapping it opens a popover
    /// list of the overflowed entries.
    pub fn overflow_icon(mut self, f: impl Fn() -> IconWidget + 'static) -> Self {
        self.overflow_icon = Some(Rc::new(f));
        self
    }

    pub(crate) fn side(&self) -> DockSide {
        self.side
    }

    /// The rail strip's effective thickness for a size `mode` — the item extent
    /// (Compact shrinks it; Default / Labeled keep the configured size) plus the
    /// rail's padding. Drives the side's rail width so the activity bar itself
    /// follows the Default / Compact / Icon + Label switch, not just its items.
    pub(crate) fn effective_thickness(&self, mode: DockRailItemSize) -> f32 {
        let size = if matches!(mode, DockRailItemSize::Compact) {
            IconButtonSize::Compact
        } else {
            self.size
        };
        item_extent(size) + RAIL_PADDING * 2.0
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockActivityBar
// ───────────────────────────────────────────────────────────────────────

pub(crate) struct DockActivityBar {
    side: DockSide,
    model: DockingModel,
    config: DockRail,
    /// Number of leading items currently shown; the rest overflow. Set in
    /// `place_children` from the available height. `usize::MAX` until first
    /// layout (everything visible).
    visible_count: Signal<usize>,
    item_count: usize,
    /// Per-item world bounds (visible position → rect), populated by the rail
    /// items during layout; read by the drop handler to place the insertion
    /// line and compute the drop index. Like a TabBar that accepts external
    /// tabs + internal reorders, the rail is a drop target for whole dock tabs
    /// (`move_tab`) and single docks (`promote_to_tab`).
    item_bounds: RailItemBounds,
    /// The rail's own world bounds, recorded in `place_children` so the drop
    /// handler can translate the item world rects into bar-local space.
    self_bounds: Rc<Cell<Rect>>,
    /// Bar-local y of the active drop insertion line (`None` = no drag over the
    /// rail). Painted by the `RailDropIndicator` overlay.
    drop_indicator: Signal<Option<f32>>,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for DockActivityBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockActivityBar")
            .field("side", &self.side)
            .finish()
    }
}

impl DockActivityBar {
    pub(crate) fn new(side: DockSide, model: DockingModel, config: DockRail) -> Self {
        Self {
            side,
            model,
            config,
            visible_count: Signal::new(usize::MAX),
            item_count: 0,
            item_bounds: Rc::new(RefCell::new(Vec::new())),
            self_bounds: Rc::new(Cell::new(Rect::ZERO)),
            drop_indicator: Signal::new(None),
            root: None,
        }
    }

    /// The effective rail-item size: compact items shrink the glyph; Default and
    /// Labeled both keep the rail's configured icon size (Labeled just adds a
    /// rotated title beneath).
    fn effective_item_size(&self) -> IconButtonSize {
        match self.model.side_rail_size(self.side) {
            DockRailItemSize::Compact => IconButtonSize::Compact,
            DockRailItemSize::Default | DockRailItemSize::Labeled => self.config.size,
        }
    }

    fn effective_item_extent(&self) -> f32 {
        item_extent(self.effective_item_size())
    }

    /// Per-item vertical stride used by the overflow capacity estimate. Labeled
    /// items are taller (icon + rotated title), so reserve extra room — a rough
    /// allowance, since each title's length differs.
    fn item_stride(&self) -> f32 {
        let mut s = self.effective_item_extent() + RAIL_ITEM_SPACING;
        if self.model.side_rail_size(self.side).shows_label() {
            s += LABELED_TITLE_ALLOWANCE;
        }
        s
    }
}

impl Widget for DockActivityBar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild when this side's rail-item size flips (context-menu "Activity
        // bar size").
        let self_id = ctx.self_id();
        self.model.rail_size_signal(self.side).bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Rebuild,
        );

        let bg = ctx.add(RectWidget::new().background(SurfaceRole::Sunken));
        let selected = self.model.side_selected_tab_signal(self.side);
        let visible = self.model.side_visible_signal(self.side);
        let tabs = self.model.side_tabs(self.side);
        let extent = self.effective_item_extent();
        let labeled = self.model.side_rail_size(self.side).shows_label();
        let visible_count = self.visible_count.clone();

        // One item per *non-hidden* tab. The item keeps its model tab index
        // (for selection); overflow parking uses its position among the shown
        // items, so a hidden tab in the middle doesn't leave a phantom slot.
        // `model_indices` maps each shown position → model tab index, so the
        // drop handler can translate a visible insertion position into a
        // `move_tab` index (a hidden tab in the middle shifts nothing).
        self.item_bounds.borrow_mut().clear();
        let mut model_indices: Vec<usize> = Vec::with_capacity(tabs.len());
        let mut items: Vec<WidgetId> = Vec::with_capacity(tabs.len());
        let mut pos = 0usize;
        for (model_i, tab) in tabs.iter().enumerate() {
            if tab.hidden {
                continue;
            }
            let p = pos;
            pos += 1;
            model_indices.push(model_i);
            let icon = self
                .model
                .side_active_dock(self.side, model_i)
                .and_then(|d| self.model.dock_icon(d));
            let label = self
                .model
                .side_active_dock(self.side, model_i)
                .and_then(|d| self.model.dock_title(d))
                .unwrap_or_else(|| lit!("Panel"));
            let id = ctx.add(DockRailItem::new(
                self.side,
                model_i,
                p,
                tab.id,
                icon,
                label,
                extent,
                labeled,
                selected.clone(),
                visible.clone(),
                self.model.clone(),
                self.item_bounds.clone(),
            ));
            ctx.visible_when(id, visible_count.map(move |c| p < *c));
            items.push(id);
        }
        self.item_count = pos;

        // Item column (pushed to the top by a trailing Spacer).
        let mut column = VStack::new().spacing(RAIL_ITEM_SPACING);
        if let Some(top) = &self.config.top_slot {
            column = column.add_child(ctx.add_boxed((top)()));
        }
        for id in &items {
            column = column.add_child(*id);
        }
        // Overflow trigger: a caller-chosen glyph that opens a popover list of
        // the overflowed entries. Shown only while something overflows.
        if let Some(of_icon) = &self.config.overflow_icon {
            let total = pos;
            let trigger = IconButton::new((of_icon)())
                .size(self.effective_item_size())
                .tooltip(lit!("More panels"));
            let overflow = ctx.add(
                PopoverIconButton::new(trigger)
                    .content(DockOverflowMenu::new(
                        self.side,
                        self.model.clone(),
                        visible_count.clone(),
                    ))
                    .placement(bastyde_core::overlay::OverlayPlacement::TrailingEdge),
            );
            ctx.visible_when(overflow, visible_count.map(move |c| *c < total));
            column = column.add_child(overflow);
        }
        let spacer = ctx.add(Spacer::new());
        column = column.add_child(spacer);
        if let Some(bottom) = &self.config.bottom_slot {
            let b = ctx.add_boxed((bottom)());
            column = column.add_child(b);
        }

        let column_id = ctx.add(column);
        let padded = ctx.add(Padding::uniform(RAIL_PADDING).child_id(column_id));
        // Insertion-line overlay (topmost) — painted while a dock tab / dock
        // widget is dragged over the rail.
        let indicator = ctx.add(RailDropIndicator::new(self.drop_indicator.clone()));
        let root = ctx.add(
            ZStack::new()
                .add_child(bg)
                .add_child(padded)
                .add_child(indicator),
        );
        self.root = Some(root);

        // Right-click on empty rail space → the activities checklist + Activity
        // bar size (the affordance to restore a hidden activity once every item
        // is hidden, and to resize the rail).
        let menu_model = self.model.clone();
        let menu_side = self.side;
        // Drag-and-drop: the rail accepts external activities (a whole tab
        // dragged from another side's rail / strip → `move_tab`, a single dock
        // → `promote_to_tab`) AND internal moves (dragging one of its own rail
        // items reorders the side's tabs — same `move_tab`, the source-side ==
        // target-side path). The insertion index comes from the pointer vs the
        // recorded item bounds; the overlay paints the line.
        let side = self.side;
        let model = self.model.clone();
        let item_bounds_hover = self.item_bounds.clone();
        let item_bounds_drop = self.item_bounds.clone();
        let self_bounds_hover = self.self_bounds.clone();
        let self_bounds_drop = self.self_bounds.clone();
        let indicator_hover = self.drop_indicator.clone();
        let indicator_leave = self.drop_indicator.clone();
        let indicator_drop = self.drop_indicator.clone();
        let visible_count_hover = self.visible_count.clone();
        let visible_count_drop = self.visible_count.clone();
        let model_indices_hover = model_indices.clone();
        let model_indices_drop = model_indices;
        ctx.apply_self_handlers(
            HandlerSet::new()
                .context_menu(move |_pos, _ctx| {
                    Some(Box::new(background_menu(&menu_model, menu_side, DockMenuKind::Rail)))
                })
                .on_drag_hover(move |payload, pos, _ctx| {
                    if dropped_dock_tab(payload).is_none() && dropped_dock_widget(payload).is_none()
                    {
                        indicator_hover.set(None);
                        return DropFeedback::NoFeedback;
                    }
                    let bar = self_bounds_hover.get();
                    let shown =
                        shown_items(&item_bounds_hover, &model_indices_hover, &visible_count_hover);
                    let (_, line_y) = rail_insertion(pos.y, &shown, bar.y, bar.height);
                    indicator_hover.set(Some(line_y));
                    DropFeedback::InsertionLine {
                        y: line_y,
                        width: bar.width,
                    }
                })
                .on_drag_leave(move |_ctx| indicator_leave.set(None))
                .on_drop(move |payload, pos, ctx| {
                    indicator_drop.set(None);
                    let bar = self_bounds_drop.get();
                    let shown =
                        shown_items(&item_bounds_drop, &model_indices_drop, &visible_count_drop);
                    let (vpos, _) = rail_insertion(pos.y, &shown, bar.y, bar.height);
                    // Visible insertion position → model tab index; past the
                    // last shown item ⇒ the side's end.
                    let at = model_indices_drop
                        .get(vpos)
                        .copied()
                        .unwrap_or_else(|| model.tab_count(side));
                    if let Some(tab_id) = dropped_dock_tab(&payload) {
                        model.move_tab(tab_id, side, at);
                        model.set_side_visible(side, true);
                        ctx.request_accessibility_update();
                        true
                    } else if let Some(dock_id) = dropped_dock_widget(&payload) {
                        model.promote_to_tab(dock_id, side, at);
                        model.set_side_visible(side, true);
                        ctx.request_accessibility_update();
                        true
                    } else {
                        false
                    }
                }),
        );

        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Record the rail's world bounds so the drop handler can translate the
        // item world rects (recorded by the rail items) into bar-local space.
        self.self_bounds.set(bounds);
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }

        // Capacity: how many items fit in the column once the padding, the
        // optional slots, and (if overflowing) the overflow trigger are
        // reserved. Slots are treated as roughly one item tall — a good
        // estimate for a square rail glyph.
        let stride = self.item_stride();
        if stride <= 0.0 {
            return;
        }
        let mut reserve = RAIL_PADDING * 2.0;
        if self.config.top_slot.is_some() {
            reserve += stride;
        }
        if self.config.bottom_slot.is_some() {
            reserve += stride;
        }
        let avail = (bounds.height - reserve).max(0.0);
        let fit = (avail / stride).floor() as usize;

        let total = self.item_count;
        let new_visible = if total <= fit {
            total
        } else if self.config.overflow_icon.is_some() {
            // Reserve one slot for the overflow trigger.
            fit.saturating_sub(1)
        } else {
            // No overflow affordance → just clip the surplus.
            fit
        };
        if self.visible_count.get() != new_visible {
            self.visible_count.set(new_visible);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use bastyde_core::accesskit::{Orientation as A11yOrientation, Role};
        builder.set_role(Role::TabList);
        builder.set_name(super::a11y::rail_label(self.side).resolve_now());
        builder.set_orientation(A11yOrientation::Vertical);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

/// Snapshot the currently-shown rail items (visible position, world bounds),
/// sorted by position, dropping any stale entry beyond the live shown count
/// (an item parked by overflow keeps a lingering bound until it next lays out).
fn shown_items(
    bounds: &RailItemBounds,
    model_indices: &[usize],
    visible_count: &Signal<usize>,
) -> Vec<(usize, Rect)> {
    let shown = model_indices.len().min(visible_count.get());
    let mut out: Vec<(usize, Rect)> = bounds
        .borrow()
        .iter()
        .filter(|(p, _)| *p < shown)
        .copied()
        .collect();
    out.sort_by_key(|(p, _)| *p);
    out
}

/// Given the pointer's bar-local y, the shown items (sorted by position, world
/// bounds), and the bar's world origin/height, return the visible insertion
/// position (`0..=count`) and the indicator's bar-local y.
fn rail_insertion(
    local_y: f32,
    shown: &[(usize, Rect)],
    bar_origin_y: f32,
    bar_height: f32,
) -> (usize, f32) {
    if shown.is_empty() {
        return (0, (RAIL_PADDING).min(bar_height));
    }
    // Insertion position = number of items whose vertical centre is above the
    // pointer.
    let mut vpos = shown.len();
    for (i, (_, b)) in shown.iter().enumerate() {
        let center = (b.y + b.height * 0.5) - bar_origin_y;
        if local_y < center {
            vpos = i;
            break;
        }
    }
    let line_y = if vpos == 0 {
        let top = shown[0].1.y - bar_origin_y;
        (top - RAIL_ITEM_SPACING * 0.5).max(0.0)
    } else if vpos >= shown.len() {
        let last = &shown[shown.len() - 1].1;
        (last.y + last.height - bar_origin_y + RAIL_ITEM_SPACING * 0.5).min(bar_height)
    } else {
        let prev = &shown[vpos - 1].1;
        let next = &shown[vpos].1;
        let prev_bottom = prev.y + prev.height - bar_origin_y;
        let next_top = next.y - bar_origin_y;
        (prev_bottom + next_top) * 0.5
    };
    (vpos, line_y.clamp(0.0, bar_height))
}

// ───────────────────────────────────────────────────────────────────────
// RailDropIndicator — the horizontal insertion line painted over the rail.
// ───────────────────────────────────────────────────────────────────────

/// A pure-decoration overlay (topmost child of the rail's ZStack) that paints a
/// horizontal accent line at the bar-local y in its `y` signal — the rail's
/// equivalent of a `TabBar` insertion indicator. Paints nothing when `y` is
/// `None`. Pointer events pass straight through so the rail items below stay
/// interactive.
struct RailDropIndicator {
    y: Signal<Option<f32>>,
    color: ColorProp,
}

impl std::fmt::Debug for RailDropIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RailDropIndicator").finish()
    }
}

impl RailDropIndicator {
    fn new(y: Signal<Option<f32>>) -> Self {
        Self {
            y,
            color: ColorProp::from(BorderRole::Accent),
        }
    }
}

impl Widget for RailDropIndicator {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.y
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::RepaintOnly);
        ctx.apply_self_handlers(HandlerSet::new().event_pass_through(true));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let Some(y) = self.y.get() else {
            return;
        };
        let color = self.color.resolve(ctx.theme, true);
        let t = 2.0;
        let yy = bounds.y + y - t * 0.5;
        // Inset a touch from the rail's padding so the line reads as "between
        // items", not flush to the edge.
        let x = bounds.x + RAIL_PADDING;
        let w = (bounds.width - RAIL_PADDING * 2.0).max(0.0);
        canvas.fill_rect(Rect::new(x, yy, w, t), color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockOverflowMenu — the popover content listing the overflowed entries.
// ───────────────────────────────────────────────────────────────────────

/// A column of rows (one per tab), each shown only while that tab is
/// overflowed (`index >= visible_count`). Selecting a row activates its tab
/// and shows the side.
#[derive(Debug)]
struct DockOverflowMenu {
    side: DockSide,
    model: DockingModel,
    visible_count: Signal<usize>,
    root: Option<WidgetId>,
}

impl DockOverflowMenu {
    fn new(side: DockSide, model: DockingModel, visible_count: Signal<usize>) -> Self {
        Self {
            side,
            model,
            visible_count,
            root: None,
        }
    }
}

impl Widget for DockOverflowMenu {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let tabs = self.model.side_tabs(self.side);
        let mut column = VStack::new().spacing(2.0);
        // Mirror the rail: only non-hidden tabs are rail items, and overflow is
        // keyed on the position among shown items (so an overflowed row appears
        // here exactly when its rail item is parked).
        let mut pos = 0usize;
        for (model_i, tab) in tabs.iter().enumerate() {
            if tab.hidden {
                continue;
            }
            let p = pos;
            pos += 1;
            let label = self
                .model
                .side_active_dock(self.side, model_i)
                .and_then(|d| self.model.dock_title(d))
                .unwrap_or_else(|| lit!("Panel"));
            let row = ctx.add(DockOverflowRow::new(
                self.side,
                model_i,
                tab.id,
                label,
                self.model.clone(),
            ));
            ctx.visible_when(row, self.visible_count.map(move |c| p >= *c));
            column = column.add_child(row);
        }
        let column_id = ctx.add(column);
        let root = ctx.add(Padding::uniform(4.0).child_id(column_id));
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockRailItem — one activity-rail item.
// ───────────────────────────────────────────────────────────────────────

struct DockRailItem {
    side: DockSide,
    index: usize,
    /// Position among the *shown* (non-hidden) items — the key the drop
    /// handler indexes by when computing an insertion position.
    pos: usize,
    tab_id: DockTabId,
    icon: Option<DockIconFactory>,
    label: LocalizedString,
    extent: f32,
    /// Labeled mode: paint a 90°-rotated title under the icon (no tooltip).
    /// Icon-only modes attach the title as a hover tooltip instead.
    labeled: bool,
    selected: Signal<usize>,
    visible: Signal<bool>,
    model: DockingModel,
    /// The bar's shared item-bounds sink; this item upserts its world bounds
    /// (keyed by `pos`) here each layout pass.
    bounds_sink: RailItemBounds,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for DockRailItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockRailItem")
            .field("index", &self.index)
            .finish()
    }
}

impl DockRailItem {
    #[allow(clippy::too_many_arguments)]
    fn new(
        side: DockSide,
        index: usize,
        pos: usize,
        tab_id: DockTabId,
        icon: Option<DockIconFactory>,
        label: LocalizedString,
        extent: f32,
        labeled: bool,
        selected: Signal<usize>,
        visible: Signal<bool>,
        model: DockingModel,
        bounds_sink: RailItemBounds,
    ) -> Self {
        Self {
            side,
            index,
            pos,
            tab_id,
            icon,
            label,
            extent,
            labeled,
            selected,
            visible,
            model,
            bounds_sink,
            root: None,
        }
    }
}

impl Widget for DockRailItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let idx = self.index;
        let active = self
            .selected
            .zip(&self.visible)
            .map(move |(s, v)| *s == idx && *v);
        let bg = active.map(|a| {
            if *a {
                SurfaceRole::Selected
            } else {
                SurfaceRole::Transparent
            }
        });
        let bg_rect = ctx.add(RectWidget::new().bind_background(bg));

        let glyph = if let Some(icon) = self.icon.take() {
            ctx.add((icon)())
        } else {
            let s = self.label.resolve_now();
            let ch: String = s.chars().take(1).collect();
            ctx.add(
                TextWidget::new(lit!(ch))
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
        };
        let centered = ctx.add(Center::new().child_id(glyph));
        let icon_box = ctx.add(
            FixedSize::new()
                .bind_width(self.extent)
                .bind_height(self.extent)
                .child_id(centered),
        );

        // Labeled mode: a 90°-rotated title above the icon square (the
        // vertical-accordion look). The title is painted, so no tooltip. Icon
        // modes show the icon alone and surface the title as a hover tooltip.
        let content = if self.labeled {
            let label = ctx.add(RotatedLabel::new(
                self.label.clone(),
                Signal::new(TextRole::Secondary),
            ));
            let stack = ctx.add(
                VStack::new()
                    .alignment(HAlignment::Center)
                    .spacing(2.0)
                    .add_child(label)
                    .add_child(icon_box),
            );
            // A bit of top breathing room so the rotated title's top character
            // isn't flush against the rail item's top edge.
            ctx.add(Padding::new(LABELED_TOP_MARGIN, 0.0, 0.0, 0.0).child_id(stack))
        } else {
            icon_box
        };
        let root = ctx.add(ZStack::new().add_child(bg_rect).add_child(content));
        self.root = Some(root);

        if !self.labeled {
            let tip = ctx.add(crate::tooltip::TooltipWidget::new(self.label.clone()));
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root, tip, delay);
        }

        let self_id = ctx.self_id();
        let policy = self.model.policy();
        let model = self.model.clone();
        let selected = self.selected.clone();
        let visible = self.visible.clone();
        let side = self.side;
        let tab_id = self.tab_id;
        let menu_model = self.model.clone();
        let allow_collapse = policy.allow_side_collapse;
        let mut handlers = HandlerSet::new().on_tap(move |_e, _ctx| {
            if selected.get() == idx && visible.get() {
                // Click on the active item hides the side — unless collapsing is
                // locked, in which case it stays shown (a no-op).
                if allow_collapse {
                    model.set_side_visible(side, false);
                }
            } else {
                model.select_tab(side, idx);
                model.set_side_visible(side, true);
            }
        });
        // Drag a rail item to reorder / move the activity — only when allowed.
        if policy.allow_activity_drag {
            handlers = handlers.on_drag(move |phase, ctx| {
                if let DragPhase::Started { .. } = phase {
                    ctx.start_drag(
                        self_id,
                        DragPayload::typed(DockTabDragData {
                            tab_id,
                            source_side: side,
                        }),
                    );
                }
            });
        }
        handlers = handlers
            .context_menu(move |_pos, _ctx| {
                Some(Box::new(activity_context_menu(
                    &menu_model,
                    side,
                    tab_id,
                    DockMenuKind::Rail,
                )))
            })
            .focusable(true)
            .cursor(CursorIcon::Pointer);
        ctx.apply_self_handlers(handlers);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Upsert this item's world bounds (keyed by its shown position) so the
        // bar's drop handler can compute an insertion line.
        {
            let mut sink = self.bounds_sink.borrow_mut();
            if let Some(slot) = sink.iter_mut().find(|(p, _)| *p == self.pos) {
                slot.1 = bounds;
            } else {
                sink.push((self.pos, bounds));
            }
        }
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use bastyde_core::accesskit::{Action, Role};
        builder.set_role(Role::Tab);
        builder.set_name(self.label.resolve_now());
        builder.set_selected(self.selected.get() == self.index && self.visible.get());
        builder.add_action(Action::Focus);
        builder.add_action(Action::Click);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockOverflowRow — one row in the overflow popover.
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct DockOverflowRow {
    side: DockSide,
    index: usize,
    tab_id: DockTabId,
    label: LocalizedString,
    model: DockingModel,
    root: Option<WidgetId>,
}

impl DockOverflowRow {
    fn new(
        side: DockSide,
        index: usize,
        tab_id: DockTabId,
        label: LocalizedString,
        model: DockingModel,
    ) -> Self {
        Self {
            side,
            index,
            tab_id,
            label,
            model,
            root: None,
        }
    }
}

impl Widget for DockOverflowRow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let label = ctx.add(
            TextWidget::new(self.label.clone())
                .style(TextStyleRole::Body)
                .color(TextRole::Primary)
                .single_line(),
        );
        let spacer = ctx.add(Spacer::new());
        let row = ctx.add(HStack::new().spacing(8.0).add_child(label).add_child(spacer));
        let root = ctx.add(Padding::symmetric(6.0, 10.0).child_id(row));
        self.root = Some(root);

        let model = self.model.clone();
        let side = self.side;
        let idx = self.index;
        let _tab = self.tab_id;
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap(move |_e, _ctx| {
                    model.select_tab(side, idx);
                    model.set_side_visible(side, true);
                })
                .focusable(true)
                .cursor(CursorIcon::Pointer),
        );
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use bastyde_core::accesskit::{Action, Role};
        builder.set_role(Role::MenuItem);
        builder.set_name(self.label.resolve_now());
        builder.add_action(Action::Click);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_insertion_picks_the_gap_under_the_pointer() {
        // Three items stacked at world y = 100 / 142 / 184 (40 tall each); the
        // bar's world origin y is 100, height 300, so item local centres are
        // 20 / 62 / 104.
        let items = vec![
            (0usize, Rect::new(100.0, 100.0, 40.0, 40.0)),
            (1, Rect::new(100.0, 142.0, 40.0, 40.0)),
            (2, Rect::new(100.0, 184.0, 40.0, 40.0)),
        ];
        assert_eq!(rail_insertion(5.0, &items, 100.0, 300.0).0, 0, "above all → front");
        assert_eq!(rail_insertion(40.0, &items, 100.0, 300.0).0, 1, "past item 0 → 1");
        assert_eq!(rail_insertion(70.0, &items, 100.0, 300.0).0, 2, "past item 1 → 2");
        assert_eq!(rail_insertion(290.0, &items, 100.0, 300.0).0, 3, "below all → end");
    }

    #[test]
    fn rail_insertion_on_empty_rail_is_front() {
        assert_eq!(rail_insertion(50.0, &[], 0.0, 100.0).0, 0);
    }
}
