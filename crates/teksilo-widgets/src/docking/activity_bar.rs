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
//! - **[`DockAction`]s** — dockless command buttons that look and behave like
//!   activity items but open no panel. Never draggable, never hidable, never
//!   persisted; grouped into an ARIA `Role::Toolbar` beside — never inside —
//!   the tab list.
//! - **Overflow** — when the items don't all fit, the surplus are parked
//!   dormant and reached through a caller-chosen **overflow item** (an icon)
//!   that opens a popover list of the overflowed entries.
//!
//! **Accessibility structure.** ARIA's Tabs pattern restricts a `role=tablist`
//! to `role=tab` children, so the rail is NOT one flat tab list: the items live
//! in a [`DockRailTabList`] (`Role::TabList`) and the actions in one
//! [`DockRailActionGroup`] (`Role::Toolbar`) per placement, as siblings under a
//! presentational root. The slots and the overflow trigger are likewise
//! siblings, never tab-list children. Each composite is its own single Tab stop
//! with its own roving Arrow/Home/End cycle; Tab/Shift+Tab crosses between them.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::gesture::DragPhase;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::{DragPayload, DropFeedback};
use teksilo_i18n::{LocalizedString, lit};
use teksilo_tokens::{BorderRole, CornerRadius, HAlignment, SurfaceRole, TextRole, TextStyleRole};

use crate::icon_button::{IconButton, IconButtonSize};
use crate::popover_widget::PopoverIconButton;
use crate::primitives::{
    Center, FixedSize, HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack,
};
use crate::styles::recipe_icon_button_style::ICON_BUTTON_CORNER_RADIUS;
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

/// Shared list of `(visible position → WidgetId)` for the rail's items, used
/// to move keyboard focus between sibling tabs (roving focus). Keyed by the
/// same visible position as [`RailItemBounds`] so the two stay aligned.
type RailItemIds = Rc<RefCell<Vec<(usize, WidgetId)>>>;

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

/// Map an [`IconButtonSize`] to the glyph (icon) dimension (dp) drawn inside a
/// rail item's square box. Mirrors [`IconButton`]'s own
/// size → glyph scaling so a caller's rail icon tracks the rail size instead of
/// staying a fixed dp — a 40 dp `Large` box gets a 24 dp glyph, not a tiny one.
fn item_glyph_size(size: IconButtonSize) -> f32 {
    use crate::styles::recipe_icon_button_style::*;
    match size {
        IconButtonSize::Compact | IconButtonSize::Default => ICON_BUTTON_ICON_SIZE,
        IconButtonSize::Toolbar => ICON_BUTTON_ICON_SIZE_TOOLBAR,
        IconButtonSize::Large => ICON_BUTTON_ICON_SIZE_LARGE,
        IconButtonSize::Hero => ICON_BUTTON_ICON_SIZE_HERO,
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
// DockAction — a dockless command button in the rail.
// ───────────────────────────────────────────────────────────────────────

/// Stable identity for a [`DockAction`].
///
/// **Not** used for persistence — a rail action carries no user-mutable state,
/// so nothing about it is serialized (see [`DockLayoutState`](super::DockLayoutState)'s
/// "app-config is reconstructed each run" rule). It exists so the accessibility
/// tree and the automation bridge can address a given action stably across
/// runs; a fresh-per-run id would make every script that clicks a rail action
/// flaky.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DockActionId(u64);

impl DockActionId {
    /// Derive a stable id from a caller-chosen name — identical across runs,
    /// processes and machines. Prefer this over [`from_raw`](Self::from_raw):
    /// it removes the hand-picked-`u64`-literal collision hazard entirely.
    ///
    /// `const` so ids can be declared as module-scope `const` items, the same
    /// way apps already declare their [`DockWidgetId`](super::DockWidgetId)s.
    ///
    /// ```
    /// # use teksilo_widgets::docking::DockActionId;
    /// const SETTINGS: DockActionId = DockActionId::named("app.settings");
    /// assert_eq!(SETTINGS, DockActionId::named("app.settings"));
    /// assert_ne!(SETTINGS, DockActionId::named("app.about"));
    /// ```
    pub const fn named(name: &str) -> Self {
        // FNV-1a. Chosen over a stronger hash because it must run in a `const`
        // context; there is no adversarial input here, only a handful of
        // app-chosen literals.
        let bytes = name.as_bytes();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        Self(hash)
    }

    /// Wrap a raw value. Prefer [`named`](Self::named).
    pub const fn from_raw(v: u64) -> Self {
        Self(v)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Where a [`DockAction`] sits along the rail's column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockActionPlacement {
    /// Before the first activity item, in the flowing cluster.
    Start,
    /// After the last activity item **and after the overflow trigger**, still
    /// in the flowing cluster — the group grows downward with the tabs.
    End,
    /// Past the flexible spacer, anchored to the rail's far edge regardless of
    /// how many activities exist — VS Code's Accounts / Manage-gear cluster.
    /// Where a Settings gear belongs.
    Pinned,
}

/// A **dockless command button** in the activity rail: it looks and behaves
/// like an activity item, but opens no panel — activating it just runs a
/// closure.
///
/// Declared on [`DockRail::action`], so (like the rail's slots) it is per-view
/// app config, reconstructed each run. A rail action is deliberately **more
/// restricted** than a real activity: it is never draggable, never hidable, has
/// no "Move to" menu, and is never overflow-parked — it is reserved space. That
/// matches every surveyed precedent (VS Code's fixed Accounts / Manage cluster;
/// IntelliJ's stripe, whose only non-tool-window button is IDE-owned chrome).
///
/// ```ignore
/// DockRail::new(DockSide::Leading).action(
///     DockAction::new(
///         DockActionId::named("app.settings"),
///         lit!("Settings"),
///         || IconWidget::gear(),
///         |ctx| ctx.send_intent(Intent::new("app.settings")),
///     )
///     .placement(DockActionPlacement::Pinned),
/// )
/// ```
#[derive(Clone)]
pub struct DockAction {
    pub(crate) id: DockActionId,
    pub(crate) placement: DockActionPlacement,
    pub(crate) label: LocalizedString,
    pub(crate) icon: DockIconFactory,
    pub(crate) tooltip: Option<LocalizedString>,
    pub(crate) enabled: Prop<bool>,
    /// `Some` => paints the selected surface while the signal is `true`.
    ///
    /// **Reflect-only**: the rail never writes this signal — `on_activate`
    /// owns every write. That is deliberate, and differs from
    /// [`IconButton::toggle`](crate::icon_button::IconButton::toggle), which
    /// flips its signal on click: a rail action's toggled state is frequently
    /// a *derived* signal (a `map` over app state), which cannot be written at
    /// all, and a writable one would fight the model it mirrors.
    pub(crate) toggled: Option<Signal<bool>>,
    pub(crate) on_activate: Rc<dyn Fn(&mut EventContext)>,
}

impl std::fmt::Debug for DockAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockAction")
            .field("id", &self.id)
            .field("placement", &self.placement)
            .finish()
    }
}

impl DockAction {
    /// Declare a rail action. Defaults to [`DockActionPlacement::End`],
    /// enabled, untoggled, with the label as its hover tooltip.
    pub fn new(
        id: DockActionId,
        label: impl Into<LocalizedString>,
        icon: impl Fn() -> IconWidget + 'static,
        on_activate: impl Fn(&mut EventContext) + 'static,
    ) -> Self {
        Self {
            id,
            placement: DockActionPlacement::End,
            label: label.into(),
            icon: Rc::new(icon),
            tooltip: None,
            enabled: Prop::Static(true),
            toggled: None,
            on_activate: Rc::new(on_activate),
        }
    }

    /// Where the action sits along the rail. See [`DockActionPlacement`].
    pub fn placement(mut self, placement: DockActionPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Override the hover tooltip (defaults to the label). Ignored in
    /// `Icon + Label` rail mode, which paints the label inline instead.
    pub fn tooltip(mut self, tooltip: impl Into<LocalizedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Enable / disable the action. Accepts a `bool` or a `Signal<bool>`.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Paint the selected surface while `state` is `true` — the same
    /// highlight an open activity gets. **Reflect-only**: activating the
    /// action does not write `state`; `on_activate` must.
    pub fn toggled(mut self, state: Signal<bool>) -> Self {
        self.toggled = Some(state);
        self
    }

    /// The action's id.
    pub fn id(&self) -> DockActionId {
        self.id
    }
}

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
    pub(crate) background: Option<ColorProp>,
    pub(crate) divider: Option<ColorProp>,
    pub(crate) top_slot: Option<DockRailSlot>,
    pub(crate) bottom_slot: Option<DockRailSlot>,
    pub(crate) leading_slot: Option<DockRailSlot>,
    pub(crate) trailing_slot: Option<DockRailSlot>,
    pub(crate) actions: Vec<DockAction>,
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
            background: None,
            divider: None,
            top_slot: None,
            bottom_slot: None,
            leading_slot: None,
            trailing_slot: None,
            actions: Vec::new(),
            overflow_icon: None,
        }
    }

    /// Pick one size for every rail item ([`IconButtonSize::Compact`] …
    /// [`Hero`](IconButtonSize::Hero)). Default [`IconButtonSize::Large`].
    pub fn size(mut self, size: IconButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Override the rail strip's background. Accepts `Color`, a
    /// [`SurfaceRole`], or a `Signal<Color>`.
    /// Default (unset) is `SurfaceRole::Sunken`.
    pub fn background(mut self, color: impl Into<ColorProp>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Draw a 1 dp divider line between the rail and the side's content, on
    /// the rail's content-facing edge (RTL-aware). Uses `BorderRole::Divider`.
    /// Off by default. See [`divider_color`](Self::divider_color) for a custom
    /// colour.
    pub fn divider(mut self) -> Self {
        self.divider = Some(BorderRole::Divider.into());
        self
    }

    /// Like [`divider`](Self::divider), but with an explicit colour. Accepts
    /// `Color`, a [`BorderRole`], or a
    /// `Signal<Color>`.
    pub fn divider_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.divider = Some(color.into());
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

    /// Widget pinned at the **start** of this side's **Strip**-presentation tab
    /// bar (via [`TabWidget::bar_leading_slot`](crate::tab_widget::TabWidget::bar_leading_slot)).
    /// The Rail-presentation counterpart is [`top_slot`](Self::top_slot).
    ///
    /// **Weaker contract than `top_slot`.** `top_slot`/`bottom_slot` sit on the
    /// `DockActivityBar`, which is built whenever the side has a rail — they
    /// survive the side being collapsed. `leading_slot`/`trailing_slot` sit
    /// inside the side's `TabWidget`, which lives within the collapsing
    /// `SideClipPane`, so they disappear with the content when the side is
    /// hidden. If your content must survive a hidden side, use Rail
    /// presentation, or host it outside the docking system.
    pub fn leading_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.leading_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }

    /// Widget pinned at the **end** of this side's **Strip**-presentation tab
    /// bar. Composed *before* the side's own "hidden activities" hamburger when
    /// both are present, so neither is dropped. See
    /// [`leading_slot`](Self::leading_slot) for the visibility contract.
    pub fn trailing_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.trailing_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }

    /// Append a **dockless command button** to this side's rail. Declaration
    /// order is render order within a placement. See [`DockAction`].
    ///
    /// **Rail presentation only.** A side in
    /// [`TabPresentation::Strip`](super::TabPresentation::Strip) renders no
    /// actions at all — and [`set_side_rail`](super::DockingModel::set_side_rail)
    /// can flip presentation at runtime, so a side that flips Rail → Strip drops
    /// its whole action cluster. If that is reachable in your app, mirror the
    /// cluster with [`trailing_slot`](Self::trailing_slot), which the same
    /// `DockRail` can carry alongside its actions.
    pub fn action(mut self, action: DockAction) -> Self {
        // A duplicate id is always a bug: the id is the action's stable address
        // for assistive tech and the automation bridge, so two buttons sharing
        // one makes "click the settings action" ambiguous — and it fails
        // silently, because both still render. Catch it in debug the same way
        // `open_dock` catches an unregistered dock. Two distinct names can also
        // collide under `named`'s FNV-1a; astronomically unlikely, but this
        // reports it as a collision instead of letting it ship.
        debug_assert!(
            !self.actions.iter().any(|a| a.id == action.id),
            "duplicate DockActionId {:?} on the {:?} rail — ids must be unique \
             per side (two `DockAction`s declared with the same id, or an \
             FNV-1a collision between two `DockActionId::named` values)",
            action.id,
            self.side,
        );
        self.actions.push(action);
        self
    }

    /// Choose the glyph for the overflow trigger — the item shown (in place of
    /// the surplus items) when they don't all fit. Tapping it opens a popover
    /// list of the overflowed entries.
    pub fn overflow_icon(mut self, f: impl Fn() -> IconWidget + 'static) -> Self {
        self.overflow_icon = Some(Rc::new(f));
        self
    }

    /// This rail's actions for `placement`, in declaration order.
    pub(crate) fn actions_at(&self, placement: DockActionPlacement) -> Vec<DockAction> {
        self.actions
            .iter()
            .filter(|a| a.placement == placement)
            .cloned()
            .collect()
    }

    pub(crate) fn side(&self) -> DockSide {
        self.side
    }

    /// The rail strip's effective thickness for a size `mode` — the item extent
    /// (Compact shrinks it to the standard [`IconButtonSize::Default`]; Default /
    /// Labeled keep the configured size) plus the rail's padding. Drives the
    /// side's rail width so the activity bar itself follows the Default /
    /// Compact / Icon + Label switch, not just its items.
    pub(crate) fn effective_thickness(&self, mode: DockRailItemSize) -> f32 {
        let size = if matches!(mode, DockRailItemSize::Compact) {
            IconButtonSize::Default
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
    /// Shared list of `(visible position → WidgetId)` for the currently-built
    /// rail items, populated in `build()`. Drives Arrow/Home/End roving focus
    /// between sibling tabs (the `request_focus` target list), the same way
    /// `TabBar` shares its `header_ids`. Filtered by `visible_count` at nav
    /// time so overflowed (dormant) items are skipped — they live in the
    /// overflow popover instead.
    item_ids: RailItemIds,
    /// The rail's own world bounds, recorded in `place_children` so the drop
    /// handler can translate the item world rects into bar-local space.
    self_bounds: Rc<Cell<Rect>>,
    /// Per-side content-region ids (owned by the enclosing `DockingLayout`),
    /// so a rail tab can advertise an AT `controls` relationship pointing at
    /// the `DockSidePanel` it governs (the ARIA tab → tabpanel link).
    side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
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
    pub(crate) fn new(
        side: DockSide,
        model: DockingModel,
        config: DockRail,
        side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    ) -> Self {
        Self {
            side,
            model,
            config,
            visible_count: Signal::new(usize::MAX),
            item_count: 0,
            item_bounds: Rc::new(RefCell::new(Vec::new())),
            item_ids: Rc::new(RefCell::new(Vec::new())),
            self_bounds: Rc::new(Cell::new(Rect::ZERO)),
            side_panel_ids,
            drop_indicator: Signal::new(None),
            root: None,
        }
    }

    /// The effective rail-item size: compact items shrink to the standard
    /// [`IconButtonSize::Default`] (not the extra-small `Compact` — a rail item
    /// is an identify target, so its glyph must stay legible); Default and
    /// Labeled both keep the rail's configured icon size (Labeled just adds a
    /// rotated title beneath).
    fn effective_item_size(&self) -> IconButtonSize {
        match self.model.side_rail_size(self.side) {
            DockRailItemSize::Compact => IconButtonSize::Default,
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

    /// Build this rail's [`DockRailActionGroup`] for `placement`, or `None`
    /// when the app declared no action there. Returning `None` (rather than an
    /// always-built, sometimes-hidden group) is what keeps an empty
    /// `Role::Toolbar` out of the AT tree — the same "`if is_some()`" shape the
    /// slots already use.
    fn build_action_group(
        &self,
        ctx: &mut BuildContext,
        placement: DockActionPlacement,
    ) -> Option<WidgetId> {
        let actions = self.config.actions_at(placement);
        if actions.is_empty() {
            return None;
        }
        Some(ctx.add(DockRailActionGroup::new(
            self.side,
            placement,
            actions,
            self.effective_item_extent(),
            item_glyph_size(self.effective_item_size()),
            self.model.side_rail_size(self.side).shows_label(),
        )))
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

        let bg_color = self
            .config
            .background
            .clone()
            .unwrap_or_else(|| SurfaceRole::Sunken.into());
        let bg = ctx.add(RectWidget::new().background(bg_color));
        let selected = self.model.side_selected_tab_signal(self.side);
        let visible = self.model.side_visible_signal(self.side);
        let tabs = self.model.side_tabs(self.side);
        let extent = self.effective_item_extent();
        let glyph = item_glyph_size(self.effective_item_size());
        let labeled = self.model.side_rail_size(self.side).shows_label();
        let visible_count = self.visible_count.clone();

        // One item per *non-hidden* tab. The item keeps its model tab index
        // (for selection); overflow parking uses its position among the shown
        // items, so a hidden tab in the middle doesn't leave a phantom slot.
        // `model_indices` maps each shown position → model tab index, so the
        // drop handler can translate a visible insertion position into a
        // `move_tab` index (a hidden tab in the middle shifts nothing).
        self.item_bounds.borrow_mut().clear();
        self.item_ids.borrow_mut().clear();
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
            // Label / icon: explicit activity title → primary (first
            // non-collapsed) pane's dock → "Panel" / no-icon.
            let icon = self.model.activity_icon(tab);
            let label = self.model.activity_label(tab);
            let id = ctx.add(DockRailItem::new(
                self.side,
                model_i,
                p,
                tab.id,
                icon,
                label,
                extent,
                glyph,
                labeled,
                selected.clone(),
                visible.clone(),
                self.model.clone(),
                self.item_bounds.clone(),
                self.item_ids.clone(),
                self.visible_count.clone(),
                self.side_panel_ids.clone(),
            ));
            // Register the id for roving focus (keyed by visible position).
            self.item_ids.borrow_mut().push((p, id));
            ctx.visible_when(id, visible_count.map(move |c| p < *c));
            items.push(id);
        }
        self.item_count = pos;

        // The items go into their own `Role::TabList` wrapper rather than
        // sitting directly in the rail's column: ARIA's Tabs pattern restricts
        // a tablist's children to tabs, and the column also holds slots, the
        // overflow trigger and action groups — none of which are tabs. The
        // wrapper's inner VStack is a bare `GenericContainer`, so the AT pass
        // prunes it and the items read as direct tablist children.
        let mut items_stack = VStack::new().spacing(RAIL_ITEM_SPACING);
        for id in &items {
            items_stack = items_stack.add_child(*id);
        }
        let items_stack = ctx.add(items_stack);
        let tab_list = ctx.add(DockRailTabList::new(self.side, items_stack));

        // Item column (pushed to the top by a trailing Spacer).
        let mut column = VStack::new().spacing(RAIL_ITEM_SPACING);
        if let Some(top) = &self.config.top_slot {
            column = column.add_child(ctx.add_boxed((top)()));
        }
        if let Some(group) = self.build_action_group(ctx, DockActionPlacement::Start) {
            column = column.add_child(group);
        }
        column = column.add_child(tab_list);
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
                    .placement(teksilo_core::overlay::OverlayPlacement::TrailingEdge),
            );
            ctx.visible_when(overflow, visible_count.map(move |c| *c < total));
            column = column.add_child(overflow);
        }
        if let Some(group) = self.build_action_group(ctx, DockActionPlacement::End) {
            column = column.add_child(group);
        }
        let spacer = ctx.add(Spacer::new());
        column = column.add_child(spacer);
        if let Some(group) = self.build_action_group(ctx, DockActionPlacement::Pinned) {
            column = column.add_child(group);
        }
        if let Some(bottom) = &self.config.bottom_slot {
            let b = ctx.add_boxed((bottom)());
            column = column.add_child(b);
        }

        let column_id = ctx.add(column);
        let padded = ctx.add(Padding::uniform(RAIL_PADDING).child_id(column_id));
        // Insertion-line overlay (topmost) — painted while a dock tab / dock
        // widget is dragged over the rail.
        let indicator = ctx.add(RailDropIndicator::new(self.drop_indicator.clone()));
        let mut stack = ZStack::new()
            .add_child(bg)
            .add_child(padded)
            .add_child(indicator);
        // Optional divider between the rail and the side's content, on the
        // content-facing edge (drawn above the background so it isn't covered).
        if let Some(color) = self.config.divider.clone() {
            stack = stack.add_child(ctx.add(RailEdgeDivider {
                side: self.side,
                color,
            }));
        }
        let root = ctx.add(stack);
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
                    Some(Box::new(background_menu(
                        &menu_model,
                        menu_side,
                        DockMenuKind::Rail,
                    )))
                })
                .on_drag_hover(move |payload, pos, _ctx| {
                    if dropped_dock_tab(payload).is_none() && dropped_dock_widget(payload).is_none()
                    {
                        indicator_hover.set(None);
                        return DropFeedback::NoFeedback;
                    }
                    let bar = self_bounds_hover.get();
                    let shown = shown_items(
                        &item_bounds_hover,
                        &model_indices_hover,
                        &visible_count_hover,
                    );
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
                    // A disabled side never mutates from a UI drop (Path 1 of the
                    // mid-drag-disable race: without this the model silently
                    // rejects but the side would still be revealed + the drop
                    // consumed). The widget-destroyed path is handled in core.
                    if !model.is_side_enabled(side) {
                        return false;
                    }
                    let bar = self_bounds_drop.get();
                    let shown =
                        shown_items(&item_bounds_drop, &model_indices_drop, &visible_count_drop);
                    let (vpos, _) = rail_insertion(pos.y, &shown, bar.y, bar.height);
                    // Visible insertion position → model tab index; past the
                    // last shown item ⇒ just after the last *visible* tab (not
                    // past trailing hidden tabs).
                    let at = model_indices_drop
                        .get(vpos)
                        .copied()
                        .unwrap_or_else(|| model.side_append_index(side));
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
        let new_visible = shown_capacity(RailCapacity {
            height: bounds.height,
            stride,
            slots: usize::from(self.config.top_slot.is_some())
                + usize::from(self.config.bottom_slot.is_some()),
            actions: self.config.actions.len(),
            total: self.item_count,
            has_overflow_trigger: self.config.overflow_icon.is_some(),
        });
        if self.visible_count.get() != new_visible {
            self.visible_count.set(new_visible);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Deliberately property-free. `Role::TabList` now lives on the
        // `DockRailTabList` wrapper around the items alone (ARIA forbids
        // non-tab children of a tablist, and this root also holds slots, the
        // overflow trigger and the action groups). This node must stay a bare
        // `GenericContainer` — setting a name or an orientation here would
        // stop the AT pass pruning it, and a screen reader would announce
        // "Leading activity bar, group" immediately followed by "Leading
        // activity bar, tab list".
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

/// Inputs to [`shown_capacity`] — everything competing for the rail's height.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RailCapacity {
    /// The rail strip's height.
    height: f32,
    /// Per-item vertical stride (item extent + spacing, plus the Labeled
    /// title allowance when captions are shown).
    stride: f32,
    /// How many of `top_slot` / `bottom_slot` are configured. Charged one
    /// stride each — an approximation, since a caller's slot widget may be any
    /// height. (Pre-existing; fixing it needs `DockRailSlot` to report a
    /// measured extent, which is a separate change.)
    slots: usize,
    /// Declared [`DockAction`]s. Charged one stride each, which is **exact**:
    /// an action renders at the rail's own item extent, and the count is fixed
    /// at build time because an action can never be hidden.
    actions: usize,
    /// Non-hidden activity items competing for what's left.
    total: usize,
    /// Whether a caller supplied an overflow glyph. Without one the surplus is
    /// simply clipped rather than parked behind a trigger.
    has_overflow_trigger: bool,
}

/// How many activity items the rail can show, given everything else that
/// reserves space in the same column.
///
/// Pure so the arithmetic is directly testable: the widget-level effect
/// (parking the surplus dormant) depends on a signal write inside
/// `place_children` propagating to `visible_when`, which a single headless
/// layout pass does not settle.
fn shown_capacity(c: RailCapacity) -> usize {
    if c.stride <= 0.0 {
        return c.total;
    }
    let reserve = RAIL_PADDING * 2.0 + (c.slots + c.actions) as f32 * c.stride;
    let avail = (c.height - reserve).max(0.0);
    let fit = (avail / c.stride).floor() as usize;
    if c.total <= fit {
        c.total
    } else if c.has_overflow_trigger {
        // One slot goes to the overflow trigger itself.
        fit.saturating_sub(1)
    } else {
        fit
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

/// A roving-focus navigation step among the rail's shown items.
enum RailNav {
    Prev,
    Next,
    First,
    Last,
}

/// Count of rail tabs currently in the AT tree = items whose visible position
/// is below the live `visible_count` (overflowed items are dormant). Drives
/// `size_of_set` on each `Role::Tab`.
fn shown_rail_count(item_ids: &RailItemIds, visible_count: &Signal<usize>) -> usize {
    let count = visible_count.get();
    item_ids.borrow().iter().filter(|(p, _)| *p < count).count()
}

/// Roving-focus navigation among the rail's currently-shown items. Given the
/// shared id list, the live visible count, the current item's visible
/// position, and a navigation step, return the `WidgetId` to focus next
/// (arrows wrap; Home/End clamp to ends). Overflowed (dormant) items are
/// excluded — they live in the overflow popover, not the Tab cycle. Mirrors
/// `TabBar`'s `request_focus(headers[next])` roving (`tab_widget/header.rs`).
fn rail_focus_target(
    item_ids: &RailItemIds,
    visible_count: &Signal<usize>,
    current_pos: usize,
    nav: RailNav,
) -> Option<WidgetId> {
    let count = visible_count.get();
    let mut shown: Vec<(usize, WidgetId)> = item_ids
        .borrow()
        .iter()
        .filter(|(p, _)| *p < count)
        .copied()
        .collect();
    shown.sort_by_key(|(p, _)| *p);
    if shown.is_empty() {
        return None;
    }
    let cur = shown.iter().position(|(p, _)| *p == current_pos)?;
    let target = match nav {
        RailNav::Prev => (cur + shown.len() - 1) % shown.len(),
        RailNav::Next => (cur + 1) % shown.len(),
        RailNav::First => 0,
        RailNav::Last => shown.len() - 1,
    };
    Some(shown[target].1)
}

/// Count of overflow rows currently shown in the popover = items whose visible
/// position is at or above the live `visible_count` (the parked ones). Drives
/// `size_of_set` on each overflow `Role::MenuItem`.
fn overflow_shown_count(row_ids: &RailItemIds, visible_count: &Signal<usize>) -> usize {
    let count = visible_count.get();
    row_ids.borrow().iter().filter(|(p, _)| *p >= count).count()
}

/// Roving-focus navigation among the overflow popover's shown rows — the
/// counterpart of [`rail_focus_target`] for the parked (`pos >= visible_count`)
/// items. Returns the row `WidgetId` to focus next.
fn overflow_focus_target(
    row_ids: &RailItemIds,
    visible_count: &Signal<usize>,
    current_pos: usize,
    nav: RailNav,
) -> Option<WidgetId> {
    let count = visible_count.get();
    let mut shown: Vec<(usize, WidgetId)> = row_ids
        .borrow()
        .iter()
        .filter(|(p, _)| *p >= count)
        .copied()
        .collect();
    shown.sort_by_key(|(p, _)| *p);
    if shown.is_empty() {
        return None;
    }
    let cur = shown.iter().position(|(p, _)| *p == current_pos)?;
    let target = match nav {
        RailNav::Prev => (cur + shown.len() - 1) % shown.len(),
        RailNav::Next => (cur + 1) % shown.len(),
        RailNav::First => 0,
        RailNav::Last => shown.len() - 1,
    };
    Some(shown[target].1)
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
// DockRailTabList — the `Role::TabList` wrapper around the rail's items.
// ───────────────────────────────────────────────────────────────────────

/// Carries the rail's `Role::TabList` around **only** the [`DockRailItem`]s.
///
/// ARIA's Tabs pattern restricts a tablist's children to tabs, but the rail's
/// column also holds slots, the overflow trigger and the action groups. Wrapping
/// just the items keeps every one of those a sibling rather than an illegal
/// tablist child.
///
/// Layout is delegated to the caller-supplied `VStack` (which already carries
/// `RAIL_ITEM_SPACING`), so introducing this wrapper cannot change the column's
/// spacing. That stack reports a bare `Role::GenericContainer`, so the AT pass
/// prunes it and promotes the items to direct children of this node.
#[derive(Debug)]
pub(crate) struct DockRailTabList {
    side: DockSide,
    stack: WidgetId,
}

impl DockRailTabList {
    fn new(side: DockSide, stack: WidgetId) -> Self {
        Self { side, stack }
    }
}

impl Widget for DockRailTabList {
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        ctx.child_size(self.stack, proposal)
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
        use teksilo_core::accesskit::{Orientation as A11yOrientation, Role};
        builder.set_role(Role::TabList);
        builder.set_name(super::a11y::rail_label(self.side).resolve_now());
        builder.set_orientation(A11yOrientation::Vertical);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.stack]
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockRailActionGroup — the `Role::Toolbar` cluster of dockless actions.
// ───────────────────────────────────────────────────────────────────────

/// One placement's worth of [`DockAction`]s, as an ARIA toolbar sibling of the
/// rail's tab list.
///
/// Deliberately **not** a member of [`DockRailTabList`]'s children and never
/// registered into the rail's `RailItemBounds` / `RailItemIds`: the drop
/// machinery resolves a drop position through `model_indices[vpos]`, so a
/// non-tab entry sharing that indexed sequence would silently move the wrong
/// tab. Keeping the two populations structurally separate makes that class of
/// bug unreachable rather than merely guarded.
///
/// Keyboard: the group is its own single Tab stop with an internal roving
/// Arrow/Home/End cycle (the ARIA toolbar pattern), independent of the tab
/// list's. Tab / Shift+Tab crosses between the two composites; arrows never do.
pub(crate) struct DockRailActionGroup {
    side: DockSide,
    placement: DockActionPlacement,
    actions: Vec<DockAction>,
    extent: f32,
    glyph: f32,
    labeled: bool,
    /// Which action index is currently the group's single Tab stop. Local
    /// focus history — mirrors `Toolbar::roving`, NOT `DockRailItem`'s
    /// model-level `selected`: an action group has no "selected" concept.
    roving: Signal<usize>,
    item_ids: Rc<RefCell<Vec<WidgetId>>>,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for DockRailActionGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockRailActionGroup")
            .field("side", &self.side)
            .field("placement", &self.placement)
            .field("actions", &self.actions.len())
            .finish()
    }
}

impl DockRailActionGroup {
    fn new(
        side: DockSide,
        placement: DockActionPlacement,
        actions: Vec<DockAction>,
        extent: f32,
        glyph: f32,
        labeled: bool,
    ) -> Self {
        Self {
            side,
            placement,
            actions,
            extent,
            glyph,
            labeled,
            roving: Signal::new(0),
            item_ids: Rc::new(RefCell::new(Vec::new())),
            root: None,
        }
    }
}

impl Widget for DockRailActionGroup {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.item_ids.borrow_mut().clear();
        let mut stack = VStack::new().spacing(RAIL_ITEM_SPACING);
        let mut ids = Vec::with_capacity(self.actions.len());
        for (i, action) in self.actions.iter().enumerate() {
            let id = ctx.add(DockRailActionItem::new(
                action.clone(),
                i,
                self.extent,
                self.glyph,
                self.labeled,
                self.roving.clone(),
                self.item_ids.clone(),
            ));
            ids.push(id);
            stack = stack.add_child(id);
        }
        *self.item_ids.borrow_mut() = ids;
        // The roving stop can outlive a rebuild that shortened the list (an
        // app may declare a different action set per view); re-clamp so the
        // group never points its only Tab stop at a missing item.
        if self.roving.get() >= self.actions.len() {
            self.roving.set(0);
        }
        let root = ctx.add(stack);
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use teksilo_core::accesskit::{Orientation as A11yOrientation, Role};
        builder.set_role(Role::Toolbar);
        builder.set_name(super::a11y::rail_actions_label(self.side, self.placement).resolve_now());
        builder.set_orientation(A11yOrientation::Vertical);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
}

// ───────────────────────────────────────────────────────────────────────
// DockRailActionItem — one dockless action button.
// ───────────────────────────────────────────────────────────────────────

/// One [`DockAction`], rendered to match a [`DockRailItem`] pixel for pixel.
///
/// Built from the same primitives as a rail item rather than from an
/// [`IconButton`] on purpose:
/// * `IconButton::toggle` **writes** its signal on click; a `DockAction`'s
///   toggled state is reflect-only (§ [`DockAction::toggled`]).
/// * `IconButton`'s tooltip opens `Below`, which on a vertical rail drops it
///   onto the next stacked item — rail items use `TooltipPlacement::Side`.
/// * The rail owns glyph sizing and the `Icon + Label` rotated caption, so an
///   action tracks the Compact / Default / Labeled switch like a real item.
struct DockRailActionItem {
    action: DockAction,
    index: usize,
    extent: f32,
    glyph: f32,
    labeled: bool,
    roving: Signal<usize>,
    siblings: Rc<RefCell<Vec<WidgetId>>>,
    focused: Signal<bool>,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for DockRailActionItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockRailActionItem")
            .field("index", &self.index)
            .finish()
    }
}

impl DockRailActionItem {
    fn new(
        action: DockAction,
        index: usize,
        extent: f32,
        glyph: f32,
        labeled: bool,
        roving: Signal<usize>,
        siblings: Rc<RefCell<Vec<WidgetId>>>,
    ) -> Self {
        Self {
            action,
            index,
            extent,
            glyph,
            labeled,
            roving,
            siblings,
            focused: Signal::new(false),
            root: None,
        }
    }
}

impl Widget for DockRailActionItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let enabled = self.action.enabled.as_signal();
        enabled.bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);

        // Reflect-only toggled highlight, window-active-aware — the same
        // treatment `DockRailItem` gives an open activity, so a toggled action
        // reads as "on" exactly like an open panel does.
        let toggled = self
            .action
            .toggled
            .clone()
            .unwrap_or_else(|| Signal::new(false));
        toggled.bind_to(self_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        let bg = toggled.zip(&ctx.window_active_signal()).map(|(t, win)| {
            if *t {
                if *win {
                    SurfaceRole::Selected
                } else {
                    SurfaceRole::SelectedInactive
                }
            } else {
                SurfaceRole::Transparent
            }
        });
        let ring = self.focused.and(&ctx.focus_visible());
        let focus_ring_width = ctx.theme().shape.focus_ring_width;
        let border_color: ColorProp = ring
            .map(|f| {
                if *f {
                    BorderRole::Focused
                } else {
                    BorderRole::Transparent
                }
            })
            .into();
        let border_width = ring.map(move |f| if *f { focus_ring_width } else { 0.0 });
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .border_color(border_color)
                .border_width(border_width)
                .corner_radius(CornerRadius::uniform(ICON_BUTTON_CORNER_RADIUS)),
        );

        let glyph_color: ColorProp = enabled
            .map(|e| {
                if *e {
                    TextRole::Primary
                } else {
                    TextRole::Disabled
                }
            })
            .into();
        let icon = ctx.add(
            (self.action.icon)()
                .icon_size(self.glyph)
                .color(glyph_color.clone()),
        );
        let centered = ctx.add(Center::new().child_id(icon));
        let icon_box = ctx.add(
            FixedSize::new()
                .width(self.extent)
                .height(self.extent)
                .child_id(centered),
        );

        let content = if self.labeled {
            let label = ctx.add(RotatedLabel::new(
                self.action.label.clone(),
                Signal::new(TextRole::Secondary),
            ));
            let stack = ctx.add(
                VStack::new()
                    .alignment(HAlignment::Center)
                    .spacing(2.0)
                    .add_child(label)
                    .add_child(icon_box),
            );
            ctx.add(Padding::new(LABELED_TOP_MARGIN, 0.0, 0.0, 0.0).child_id(stack))
        } else {
            icon_box
        };
        let root = ctx.add(ZStack::new().add_child(bg_rect).add_child(content));
        self.root = Some(root);

        if !self.labeled {
            // `Side`, never `Below` — a `Below` tooltip would land on the next
            // item down the column (the same reason `DockRailItem` does this).
            let text = self
                .action
                .tooltip
                .clone()
                .unwrap_or_else(|| self.action.label.clone());
            let tip = ctx.add(crate::tooltip::TooltipWidget::new(text));
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip_with_placement(
                root,
                tip,
                delay,
                crate::tooltip::TooltipPlacement::Side,
            );
        }

        // One activation path for pointer, keyboard and the AT `Click` action.
        // A disabled action is inert on every one of them.
        let activate: Rc<dyn Fn(&mut EventContext)> = {
            let on_activate = self.action.on_activate.clone();
            let enabled = enabled.clone();
            let roving = self.roving.clone();
            let index = self.index;
            Rc::new(move |ctx: &mut EventContext| {
                if !enabled.get() {
                    return;
                }
                roving.set(index);
                (on_activate)(ctx);
            })
        };

        // Roving tab stop: exactly one member of the group is a Tab stop.
        let index = self.index;
        ctx.set_tab_stop(self_id, self.roving.map(move |r| *r == index));
        // A disabled action stays **focusable** on purpose — it is not
        // `enabled_when`'d out of the focus order. Two reasons, one of them a
        // real bug this avoids:
        //   * ARIA's toolbar pattern explicitly keeps disabled toolbar controls
        //     focusable so a keyboard user can discover that the command exists
        //     at all (an unreachable greyed button is invisible to them).
        //   * The group has exactly ONE Tab stop, chosen by `roving`. If that
        //     item were removed from the focus order while disabled, the whole
        //     toolbar would become unreachable by keyboard — and since
        //     `enabled` is a live `Prop`, that can happen at any time, not just
        //     at build. Staying focusable makes the trap unreachable instead of
        //     needing a re-clamp on every enablement change.
        // Activation is guarded in `activate` and the glyph dims, so a disabled
        // action is inert and reads as inert without being lost.

        let focused_sig = self.focused.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap({
                    let activate = activate.clone();
                    move |_e, ctx| activate(ctx)
                })
                .on_focus(move |gained, _ctx| focused_sig.set(gained))
                .on_key({
                    let activate = activate.clone();
                    let siblings = self.siblings.clone();
                    let roving = self.roving.clone();
                    let index = self.index;
                    move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                        let WidgetEvent::KeyDown { key, .. } = event else {
                            return EventResponse::Ignored;
                        };
                        let ids = siblings.borrow();
                        if ids.is_empty() {
                            return EventResponse::Ignored;
                        }
                        let next = match key {
                            Key::ArrowUp | Key::ArrowLeft => (index + ids.len() - 1) % ids.len(),
                            Key::ArrowDown | Key::ArrowRight => (index + 1) % ids.len(),
                            Key::Home => 0,
                            Key::End => ids.len() - 1,
                            Key::Enter | Key::Space => {
                                drop(ids);
                                activate(ctx);
                                return EventResponse::Handled;
                            }
                            _ => return EventResponse::Ignored,
                        };
                        let target = ids[next];
                        drop(ids);
                        roving.set(next);
                        ctx.request_focus(target);
                        EventResponse::Handled
                    }
                })
                .on_access_action({
                    let activate = activate.clone();
                    move |action: teksilo_core::accesskit::Action, ctx: &mut EventContext| {
                        if action == teksilo_core::accesskit::Action::Click {
                            activate(ctx);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
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
        use teksilo_core::accesskit::{Action, Role};
        // `Role::Button` — NOT `Role::Tab`. An action controls no tabpanel, so
        // announcing it as a tab would promise a panel that never appears.
        builder.set_role(Role::Button);
        builder.set_name(self.action.label.resolve_now());
        builder.add_action(Action::Focus);
        // Announce the inert state rather than dropping out of the focus order
        // (see the `set_tab_stop` comment in `build`): a disabled toolbar
        // control stays reachable so it is discoverable, and says why.
        if self.action.enabled.get() {
            builder.add_action(Action::Click);
        } else {
            builder.set_disabled();
        }
        builder.set_position_in_set(self.index + 1);
        builder.set_size_of_set(self.siblings.borrow().len());
        // A reflect-only bistate reads as a toggle button to AT.
        if let Some(t) = &self.action.toggled {
            builder.set_toggled(t.get());
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }
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
        self.y.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
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
// RailEdgeDivider — a 1 dp line between the rail and the side's content.
// ───────────────────────────────────────────────────────────────────────

/// A pure-decoration overlay (topmost child of the rail's ZStack) that paints a
/// 1 dp vertical line on the rail's content-facing edge — the boundary between
/// the activity rail and the side's resizable content. The edge is derived from
/// the side (the rail always hugs the outer / leading-cross edge, so content
/// sits on the opposite vertical edge) and the active layout direction, so it
/// stays correct under RTL. Pointer events pass straight through.
struct RailEdgeDivider {
    side: DockSide,
    color: ColorProp,
}

impl std::fmt::Debug for RailEdgeDivider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RailEdgeDivider").finish()
    }
}

impl Widget for RailEdgeDivider {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.apply_self_handlers(HandlerSet::new().event_pass_through(true));
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let rtl = matches!(
            ctx.layout_direction,
            teksilo_core::environment::LayoutDirection::RightToLeft
        );
        // The rail hugs the outer thickness edge (leading / trailing) or the
        // leading cross-edge (top / bottom), so the content is on the trailing
        // geometric edge for every side except Trailing, where it's the leading
        // edge. Resolve that to a concrete left / right under RTL.
        let content_on_right = match self.side {
            DockSide::Trailing => rtl,
            _ => !rtl,
        };
        let t = 1.0;
        let x = if content_on_right {
            bounds.x + bounds.width - t
        } else {
            bounds.x
        };
        let color = self.color.resolve(ctx.theme, true);
        canvas.fill_rect(Rect::new(x, bounds.y, t, bounds.height), color);
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
    /// Shared `(visible position → row WidgetId)` list for roving Arrow/Home/End
    /// focus among the overflowed rows.
    row_ids: RailItemIds,
    root: Option<WidgetId>,
}

impl DockOverflowMenu {
    fn new(side: DockSide, model: DockingModel, visible_count: Signal<usize>) -> Self {
        Self {
            side,
            model,
            visible_count,
            row_ids: Rc::new(RefCell::new(Vec::new())),
            root: None,
        }
    }
}

impl Widget for DockOverflowMenu {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let tabs = self.model.side_tabs(self.side);
        let mut column = VStack::new().spacing(2.0);
        self.row_ids.borrow_mut().clear();
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
            let label = self.model.activity_label(tab);
            let row = ctx.add(DockOverflowRow::new(
                self.side,
                model_i,
                p,
                tab.id,
                label,
                self.model.clone(),
                self.row_ids.clone(),
                self.visible_count.clone(),
            ));
            self.row_ids.borrow_mut().push((p, row));
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use teksilo_core::accesskit::{Orientation, Role};
        builder.set_role(Role::Menu);
        builder.set_orientation(Orientation::Vertical);
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
    /// Glyph (icon) dimension drawn inside the `extent`-sized box — derived from
    /// the rail size so the icon scales with it (see [`item_glyph_size`]).
    glyph: f32,
    /// Labeled mode: paint a 90°-rotated title under the icon (no tooltip).
    /// Icon-only modes attach the title as a hover tooltip instead.
    labeled: bool,
    selected: Signal<usize>,
    visible: Signal<bool>,
    model: DockingModel,
    /// The bar's shared item-bounds sink; this item upserts its world bounds
    /// (keyed by `pos`) here each layout pass.
    bounds_sink: RailItemBounds,
    /// Shared sibling-id list (for Arrow/Home/End roving focus) and the live
    /// overflow count (so nav and `size_of_set` skip parked items).
    item_ids: RailItemIds,
    visible_count: Signal<usize>,
    /// Per-side content-region ids, for the `controls` (tab → tabpanel) link.
    side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    /// Keyboard `:focus-visible` state — `true` only while this item holds
    /// focus AND the last input was the keyboard; drives the focus ring.
    focused: Signal<bool>,
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
        glyph: f32,
        labeled: bool,
        selected: Signal<usize>,
        visible: Signal<bool>,
        model: DockingModel,
        bounds_sink: RailItemBounds,
        item_ids: RailItemIds,
        visible_count: Signal<usize>,
        side_panel_ids: Rc<RefCell<HashMap<DockSide, WidgetId>>>,
    ) -> Self {
        Self {
            side,
            index,
            pos,
            tab_id,
            icon,
            label,
            extent,
            glyph,
            labeled,
            selected,
            visible,
            model,
            bounds_sink,
            item_ids,
            visible_count,
            side_panel_ids,
            focused: Signal::new(false),
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
        // Window-active-aware selection highlight. `surface_selected` is
        // deliberately excluded from the theme-side inactive-window accent
        // desaturation (`ColorTokens::for_inactive_window`), so — like
        // `StandardListItem` / `TableView` — the rail item must opt in
        // explicitly, swapping to the muted `SelectedInactive` token when the
        // host window loses focus (macOS / Qt `QPalette::Inactive`). The rail
        // is persistent chrome whose "active" item tracks the open side (app
        // state, not a keyboard-focus-scoped selection), so it gates on
        // window-active alone — not view focus — keeping the open-side
        // indicator vivid while the window is active regardless of where
        // keyboard focus sits.
        let bg = active.zip(&ctx.window_active_signal()).map(|(a, win)| {
            if *a {
                if *win {
                    SurfaceRole::Selected
                } else {
                    SurfaceRole::SelectedInactive
                }
            } else {
                SurfaceRole::Transparent
            }
        });
        // Keyboard focus ring, gated on `:focus-visible` (the item is focused
        // AND the last input was the keyboard) — the same pattern as
        // `IconButton` (`recipe_icon_button_style.rs`). The border IS the focus
        // indicator; it coexists with the selection background on this rect.
        let ring = self.focused.and(&ctx.focus_visible());
        let focus_ring_width = ctx.theme().shape.focus_ring_width;
        let border_color: ColorProp = ring
            .map(|f| {
                if *f {
                    BorderRole::Focused
                } else {
                    BorderRole::Transparent
                }
            })
            .into();
        let border_width = ring.map(move |f| if *f { focus_ring_width } else { 0.0 });
        // Rounded selection highlight matching the IconButton corner style, so
        // the rail items read as buttons rather than full-square fills.
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .border_color(border_color)
                .border_width(border_width)
                .corner_radius(CornerRadius::uniform(ICON_BUTTON_CORNER_RADIUS)),
        );

        let glyph = if let Some(icon) = self.icon.take() {
            // Size the caller's icon to the rail's glyph dimension so it tracks
            // the rail size (Compact…Hero) instead of whatever fixed dp the
            // factory picked — the rail owns glyph sizing, like `IconButton`.
            ctx.add((icon)().icon_size(self.glyph))
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
                .width(self.extent)
                .height(self.extent)
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
            // The activity rail is vertical-only; its icon-only items stack
            // top-to-bottom, so the title tooltip opens to the trailing `Side`
            // (a `Below` tooltip would drop onto the next rail item).
            let tip = ctx.add(crate::tooltip::TooltipWidget::new(self.label.clone()));
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip_with_placement(
                root,
                tip,
                delay,
                crate::tooltip::TooltipPlacement::Side,
            );
        }

        let self_id = ctx.self_id();
        let policy = self.model.policy();
        let side = self.side;
        let tab_id = self.tab_id;
        let menu_model = self.model.clone();
        let allow_collapse = policy.allow_side_collapse;

        // The single activation path, shared by pointer tap, keyboard
        // Enter/Space, and the AT `Click` action — so the rail item is
        // operable by mouse, keyboard, and screen reader alike. Clicking the
        // active item hides the side (a collapse toggle) unless collapse is
        // locked; any other item selects it and shows the side.
        let activate: Rc<dyn Fn(&mut EventContext)> = {
            let model = self.model.clone();
            let selected = self.selected.clone();
            let visible = self.visible.clone();
            Rc::new(move |_ctx: &mut EventContext| {
                if selected.get() == idx && visible.get() {
                    if allow_collapse {
                        model.set_side_visible(side, false);
                    }
                } else {
                    model.select_tab(side, idx);
                    model.set_side_visible(side, true);
                }
            })
        };

        // Reflect the keyboard `:focus-visible` ring.
        let focused_sig = self.focused.clone();
        // Roving tab stop (ARIA tabs pattern): only the selected item is a
        // Tab/Shift+Tab stop; siblings stay reachable via Arrow keys +
        // `request_focus`. Matches `TabBar` (`tab_widget/header.rs`).
        ctx.set_tab_stop(self_id, self.selected.map(move |s| *s == idx));

        let mut handlers = HandlerSet::new()
            .on_tap({
                let activate = activate.clone();
                move |_e, ctx| activate(ctx)
            })
            .on_focus(move |gained, _ctx| focused_sig.set(gained))
            .on_key({
                let activate = activate.clone();
                let item_ids = self.item_ids.clone();
                let visible_count = self.visible_count.clone();
                let pos = self.pos;
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    let WidgetEvent::KeyDown { key, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    let nav = match key {
                        Key::ArrowUp | Key::ArrowLeft => RailNav::Prev,
                        Key::ArrowDown | Key::ArrowRight => RailNav::Next,
                        Key::Home => RailNav::First,
                        Key::End => RailNav::Last,
                        Key::Enter | Key::Space => {
                            // Manual activation: arrows only move focus; the
                            // panel is shown/hidden on explicit Enter/Space.
                            activate(ctx);
                            return EventResponse::Handled;
                        }
                        _ => return EventResponse::Ignored,
                    };
                    if let Some(target) = rail_focus_target(&item_ids, &visible_count, pos, nav) {
                        ctx.request_focus(target);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                }
            })
            .on_access_action({
                let activate = activate.clone();
                move |action: teksilo_core::accesskit::Action, ctx: &mut EventContext| {
                    if action == teksilo_core::accesskit::Action::Click {
                        activate(ctx);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
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
        use teksilo_core::accesskit::{Action, Role};
        builder.set_role(Role::Tab);
        builder.set_name(self.label.resolve_now());
        let is_selected = self.selected.get() == self.index;
        builder.set_selected(is_selected && self.visible.get());
        builder.add_action(Action::Focus);
        builder.add_action(Action::Click);
        // "panel N of M" — M counts only the rail tabs currently in the AT
        // tree (overflowed items are dormant, represented by the popover rows).
        // `pos` is this item's 0-based visible position; only shown items run
        // `accessibility()`, so `pos < visible_count` holds here.
        builder.set_position_in_set(self.pos + 1);
        builder.set_size_of_set(shown_rail_count(&self.item_ids, &self.visible_count));
        // Communicate the collapse toggle on the active tab: expanded when its
        // panel is shown, collapsed when hidden. Omitted on the other tabs
        // (the "expanded" concept doesn't apply to an inactive tab).
        if is_selected {
            builder.set_expanded(self.visible.get());
        }
        // `controls` → the side's content region (ARIA tab → tabpanel link).
        // Only while the side is shown: a hidden side parks its `DockSidePanel`
        // dormant (pruned from the AT tree), so linking it then would dangle.
        if self.visible.get()
            && let Some(&panel_id) = self.side_panel_ids.borrow().get(&self.side)
        {
            builder.push_controlled(widget_id_to_node_id(panel_id));
        }
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
    /// Visible position among the side's non-hidden tabs (matches the rail
    /// item's `pos`); the key for roving focus + `position_in_set`.
    pos: usize,
    tab_id: DockTabId,
    label: LocalizedString,
    model: DockingModel,
    /// Shared sibling-row id list + live overflow count, for Arrow/Home/End
    /// roving focus and `size_of_set` among the shown overflow rows.
    row_ids: RailItemIds,
    visible_count: Signal<usize>,
    /// Keyboard `:focus-visible` state — drives the row's focus ring.
    focused: Signal<bool>,
    root: Option<WidgetId>,
}

impl DockOverflowRow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        side: DockSide,
        index: usize,
        pos: usize,
        tab_id: DockTabId,
        label: LocalizedString,
        model: DockingModel,
        row_ids: RailItemIds,
        visible_count: Signal<usize>,
    ) -> Self {
        Self {
            side,
            index,
            pos,
            tab_id,
            label,
            model,
            row_ids,
            visible_count,
            focused: Signal::new(false),
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
        let row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(label)
                .add_child(spacer),
        );
        let content = ctx.add(Padding::symmetric(6.0, 10.0).child_id(row));

        // Backing surface: a subtle highlight on focus + the keyboard
        // `:focus-visible` ring, so a row navigated to by keyboard is visible
        // (it reads like a menu item).
        let ring = self.focused.and(&ctx.focus_visible());
        let focus_ring_width = ctx.theme().shape.focus_ring_width;
        let bg_role: ColorProp = self
            .focused
            .map(|f| {
                if *f {
                    SurfaceRole::Hover
                } else {
                    SurfaceRole::Transparent
                }
            })
            .into();
        let border_color: ColorProp = ring
            .map(|f| {
                if *f {
                    BorderRole::Focused
                } else {
                    BorderRole::Transparent
                }
            })
            .into();
        let border_width = ring.map(move |f| if *f { focus_ring_width } else { 0.0 });
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(bg_role)
                .border_color(border_color)
                .border_width(border_width)
                .corner_radius(CornerRadius::uniform(ICON_BUTTON_CORNER_RADIUS)),
        );
        let root = ctx.add(ZStack::new().add_child(bg_rect).add_child(content));
        self.root = Some(root);

        // Single activation path (tap / Enter-Space / AT Click): select the
        // tab and show the side.
        let activate: Rc<dyn Fn(&mut EventContext)> = {
            let model = self.model.clone();
            let side = self.side;
            let idx = self.index;
            Rc::new(move |_ctx: &mut EventContext| {
                model.select_tab(side, idx);
                model.set_side_visible(side, true);
            })
        };
        let focused_sig = self.focused.clone();
        ctx.apply_self_handlers(
            HandlerSet::new()
                .on_tap({
                    let activate = activate.clone();
                    move |_e, ctx| activate(ctx)
                })
                .on_focus(move |gained, _ctx| focused_sig.set(gained))
                .on_key({
                    let activate = activate.clone();
                    let row_ids = self.row_ids.clone();
                    let visible_count = self.visible_count.clone();
                    let pos = self.pos;
                    move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                        let WidgetEvent::KeyDown { key, .. } = event else {
                            return EventResponse::Ignored;
                        };
                        let nav = match key {
                            Key::ArrowUp | Key::ArrowLeft => RailNav::Prev,
                            Key::ArrowDown | Key::ArrowRight => RailNav::Next,
                            Key::Home => RailNav::First,
                            Key::End => RailNav::Last,
                            Key::Enter | Key::Space => {
                                activate(ctx);
                                return EventResponse::Handled;
                            }
                            _ => return EventResponse::Ignored,
                        };
                        if let Some(target) =
                            overflow_focus_target(&row_ids, &visible_count, pos, nav)
                        {
                            ctx.request_focus(target);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
                })
                .on_access_action({
                    let activate = activate.clone();
                    move |action: teksilo_core::accesskit::Action, ctx: &mut EventContext| {
                        if action == teksilo_core::accesskit::Action::Click {
                            activate(ctx);
                            EventResponse::Handled
                        } else {
                            EventResponse::Ignored
                        }
                    }
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
        use teksilo_core::accesskit::{Action, Role};
        builder.set_role(Role::MenuItem);
        builder.set_name(self.label.resolve_now());
        builder.add_action(Action::Focus);
        builder.add_action(Action::Click);
        // "N of M" within the overflow set. Only shown (parked) rows run
        // `accessibility()`, so `pos >= visible_count` holds; the 1-based
        // position within the overflowed run is `pos - visible_count + 1`.
        let count = self.visible_count.get();
        builder.set_position_in_set(self.pos.saturating_sub(count) + 1);
        builder.set_size_of_set(overflow_shown_count(&self.row_ids, &self.visible_count));
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
        assert_eq!(
            rail_insertion(5.0, &items, 100.0, 300.0).0,
            0,
            "above all → front"
        );
        assert_eq!(
            rail_insertion(40.0, &items, 100.0, 300.0).0,
            1,
            "past item 0 → 1"
        );
        assert_eq!(
            rail_insertion(70.0, &items, 100.0, 300.0).0,
            2,
            "past item 1 → 2"
        );
        assert_eq!(
            rail_insertion(290.0, &items, 100.0, 300.0).0,
            3,
            "below all → end"
        );
    }

    #[test]
    fn rail_insertion_on_empty_rail_is_front() {
        assert_eq!(rail_insertion(50.0, &[], 0.0, 100.0).0, 0);
    }

    /// A 260 dp rail with 42 dp items: 252 dp usable ⇒ 6 items fit.
    fn cap() -> RailCapacity {
        RailCapacity {
            height: 260.0,
            stride: 42.0,
            slots: 0,
            actions: 0,
            total: 8,
            has_overflow_trigger: false,
        }
    }

    #[test]
    fn capacity_shows_everything_when_it_all_fits() {
        let c = RailCapacity { total: 4, ..cap() };
        assert_eq!(shown_capacity(c), 4, "no overflow ⇒ every item shows");
    }

    #[test]
    fn capacity_clips_without_a_trigger_and_reserves_one_slot_with_one() {
        assert_eq!(
            shown_capacity(cap()),
            6,
            "no trigger ⇒ the surplus is clipped"
        );
        assert_eq!(
            shown_capacity(RailCapacity {
                has_overflow_trigger: true,
                ..cap()
            }),
            5,
            "the trigger itself costs one slot"
        );
    }

    #[test]
    fn capacity_charges_actions_and_slots() {
        // Each action is reserved space, never overflow-parked, so it costs an
        // activity slot — this is the whole point of charging them here.
        assert_eq!(
            shown_capacity(RailCapacity {
                actions: 3,
                ..cap()
            }),
            3,
            "three actions cost three activity slots (6 → 3)"
        );
        assert_eq!(
            shown_capacity(RailCapacity { slots: 2, ..cap() }),
            4,
            "top_slot + bottom_slot cost one stride each"
        );
        assert_eq!(
            shown_capacity(RailCapacity {
                slots: 2,
                actions: 3,
                has_overflow_trigger: true,
                ..cap()
            }),
            0,
            "a rail crowded past its height shows no activities, and never \
             underflows"
        );
    }

    #[test]
    fn capacity_never_underflows_or_divides_by_zero() {
        assert_eq!(
            shown_capacity(RailCapacity {
                height: 0.0,
                has_overflow_trigger: true,
                ..cap()
            }),
            0,
            "a zero-height rail shows nothing rather than wrapping around"
        );
        assert_eq!(
            shown_capacity(RailCapacity {
                stride: 0.0,
                ..cap()
            }),
            8,
            "a degenerate stride falls back to showing everything, not a divide by zero"
        );
    }

    /// Fabricate a `WidgetId` without an arena — same convention as the
    /// `menu_bar` dispatcher unit tests.
    fn wid(n: u64) -> WidgetId {
        slotmap::KeyData::from_ffi(n).into()
    }

    fn ids(items: &[(usize, u64)]) -> RailItemIds {
        Rc::new(RefCell::new(
            items.iter().map(|(p, w)| (*p, wid(*w))).collect(),
        ))
    }

    #[test]
    fn rail_focus_target_wraps_among_shown_items() {
        let item_ids = ids(&[(0, 10), (1, 11), (2, 12)]);
        let vc = Signal::new(3usize);
        assert_eq!(
            rail_focus_target(&item_ids, &vc, 0, RailNav::Next),
            Some(wid(11))
        );
        assert_eq!(
            rail_focus_target(&item_ids, &vc, 2, RailNav::Next),
            Some(wid(10)),
            "ArrowDown past the last item wraps to the first"
        );
        assert_eq!(
            rail_focus_target(&item_ids, &vc, 0, RailNav::Prev),
            Some(wid(12)),
            "ArrowUp before the first item wraps to the last"
        );
        assert_eq!(
            rail_focus_target(&item_ids, &vc, 1, RailNav::First),
            Some(wid(10))
        );
        assert_eq!(
            rail_focus_target(&item_ids, &vc, 1, RailNav::Last),
            Some(wid(12))
        );
    }

    #[test]
    fn rail_focus_target_skips_overflowed_items() {
        // visible_count = 2 → only positions 0,1 are navigable; pos 2 overflowed.
        let item_ids = ids(&[(0, 10), (1, 11), (2, 12)]);
        let vc = Signal::new(2usize);
        assert_eq!(shown_rail_count(&item_ids, &vc), 2);
        assert_eq!(
            rail_focus_target(&item_ids, &vc, 1, RailNav::Next),
            Some(wid(10)),
            "nav wraps within the two shown items, skipping the overflowed one"
        );
    }

    #[test]
    fn overflow_helpers_target_the_parked_rows() {
        // 4 items, 2 shown on the rail → positions 2,3 overflow into the popover.
        let row_ids = ids(&[(0, 10), (1, 11), (2, 12), (3, 13)]);
        let vc = Signal::new(2usize);
        assert_eq!(overflow_shown_count(&row_ids, &vc), 2);
        assert_eq!(
            overflow_focus_target(&row_ids, &vc, 2, RailNav::Next),
            Some(wid(13))
        );
        assert_eq!(
            overflow_focus_target(&row_ids, &vc, 3, RailNav::Next),
            Some(wid(12)),
            "nav wraps within the overflowed set"
        );
        assert_eq!(
            overflow_focus_target(&row_ids, &vc, 2, RailNav::Prev),
            Some(wid(13))
        );
    }
}
