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

use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::DragPayload;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::gesture::DragPhase;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{CursorIcon, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, lit};
use bastyde_tokens::{SurfaceRole, TextRole, TextStyleRole};

use crate::icon_button::{IconButton, IconButtonSize};
use crate::popover_widget::PopoverIconButton;
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack,
};

use super::context_menu::{DockMenuKind, activity_context_menu, background_menu};
use super::drag::DockTabDragData;
use super::geometry::DockSide;
use super::model::{DockIconFactory, DockTabId, DockingModel};

/// Factory for a rail slot widget (rebuilt on each rail rebuild).
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

    /// Widget pinned **above** the items (e.g. a logo / hamburger).
    pub fn top_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.top_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }

    /// Widget pinned at the **bottom** of the rail (e.g. settings / account).
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
            root: None,
        }
    }

    /// The effective rail-item size: the configured size, or compact when this
    /// side's `rail_size` selector is set to compact.
    fn effective_item_size(&self) -> IconButtonSize {
        if self.model.rail_size_signal(self.side).get() == 1 {
            IconButtonSize::Compact
        } else {
            self.config.size
        }
    }

    fn effective_item_extent(&self) -> f32 {
        item_extent(self.effective_item_size())
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
        let visible_count = self.visible_count.clone();

        // One item per *non-hidden* tab. The item keeps its model tab index
        // (for selection); overflow parking uses its position among the shown
        // items, so a hidden tab in the middle doesn't leave a phantom slot.
        let mut items: Vec<WidgetId> = Vec::with_capacity(tabs.len());
        let mut pos = 0usize;
        for (model_i, tab) in tabs.iter().enumerate() {
            if tab.hidden {
                continue;
            }
            let p = pos;
            pos += 1;
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
                tab.id,
                icon,
                label,
                extent,
                selected.clone(),
                visible.clone(),
                self.model.clone(),
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
        let root = ctx.add(ZStack::new().add_child(bg).add_child(padded));
        self.root = Some(root);

        // Right-click on empty rail space → the activities checklist + Activity
        // bar size (the affordance to restore a hidden activity once every item
        // is hidden, and to resize the rail).
        let menu_model = self.model.clone();
        let menu_side = self.side;
        ctx.apply_self_handlers(HandlerSet::new().context_menu(move |_pos, _ctx| {
            Some(Box::new(background_menu(&menu_model, menu_side, DockMenuKind::Rail)))
        }));

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

        // Capacity: how many items fit in the column once the padding, the
        // optional slots, and (if overflowing) the overflow trigger are
        // reserved. Slots are treated as roughly one item tall — a good
        // estimate for a square rail glyph.
        let extent = self.effective_item_extent();
        let stride = extent + RAIL_ITEM_SPACING;
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
    tab_id: DockTabId,
    icon: Option<DockIconFactory>,
    label: LocalizedString,
    extent: f32,
    selected: Signal<usize>,
    visible: Signal<bool>,
    model: DockingModel,
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
        tab_id: DockTabId,
        icon: Option<DockIconFactory>,
        label: LocalizedString,
        extent: f32,
        selected: Signal<usize>,
        visible: Signal<bool>,
        model: DockingModel,
    ) -> Self {
        Self {
            side,
            index,
            tab_id,
            icon,
            label,
            extent,
            selected,
            visible,
            model,
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
        let sized = ctx.add(
            FixedSize::new()
                .bind_width(self.extent)
                .bind_height(self.extent)
                .child_id(centered),
        );
        let root = ctx.add(ZStack::new().add_child(bg_rect).add_child(sized));
        self.root = Some(root);

        let self_id = ctx.self_id();
        let model = self.model.clone();
        let selected = self.selected.clone();
        let visible = self.visible.clone();
        let side = self.side;
        let tab_id = self.tab_id;
        let menu_model = self.model.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap(move |_e, _ctx| {
                    if selected.get() == idx && visible.get() {
                        model.set_side_visible(side, false);
                    } else {
                        model.select_tab(side, idx);
                        model.set_side_visible(side, true);
                    }
                })
                .on_drag(move |phase, ctx| {
                    if let DragPhase::Started { .. } = phase {
                        ctx.start_drag(
                            self_id,
                            DragPayload::typed(DockTabDragData {
                                tab_id,
                                source_side: side,
                            }),
                        );
                    }
                })
                .context_menu(move |_pos, _ctx| {
                    Some(Box::new(activity_context_menu(
                        &menu_model,
                        side,
                        tab_id,
                        DockMenuKind::Rail,
                    )))
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
