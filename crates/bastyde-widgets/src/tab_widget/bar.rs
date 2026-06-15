// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TabBar<T>` — header strip driven by a data source.
//!
//! Horizontal and vertical orientations, with shared / independent
//! sizing. Bar-leading and bar-trailing slots are wired. Overflow is
//! handled by a `ScrollArea` around the headers row, plus optional
//! scroll arrows and a "show all tabs" overflow dropdown (both on by
//! default). Closable tabs (with middle-click close), drag-to-reorder
//! with edge auto-scroll, and a leading icon-only pinned-tab strip are
//! all supported. Multi-line (multi-row) wrapping is the one layout
//! mode not yet implemented.
//!
//! The data source is consumed via the `pub(crate)` [`ListSource`]
//! abstraction so callers can pass either a `ListModel<T>` (clonable,
//! mutable) or any external `ListDataSource<Item = T>` (a database
//! cursor, a virtual list, …) without TabBar having to carry a generic
//! source parameter.
//!
//! ## Accessibility
//!
//! The bar emits `Role::TabList` with an `aria-orientation`
//! reflecting whether it was built with [`TabBar::horizontal`] or
//! [`TabBar::vertical`]. When a page hosts more than one tab list,
//! give each one an accessible name via
//! [`.access_label(tr!(tab_list_name()))`](bastyde_core::widget_builder::WidgetBuilder::access_label)
//! so screen readers can distinguish them (ARIA APG recommendation).

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::DropFeedback;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::drag_payload::{DragPayload, DropOutcome};
use bastyde_core::event::{EventResponse, ScrollDelta, WidgetEvent};
use bastyde_core::overlay::OverlayPlacement;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{ListDataSource, ListModel};
use bastyde_i18n::LocalizedString;
use bastyde_tokens::Easing;

use crate::list_source::ListSource;
use crate::primitives::FixedSize;
use crate::scroll_area::{ScrollArea, ScrollBarMode, ScrollBarPolicy};
use crate::tab_widget::delegate::{TabBarOrientation, TabDelegate, TabDisplayMode, TabSizing};
use crate::tab_widget::header::{HeaderShared, TabHeader, TabHeaderConfig};
use crate::tab_widget::id::TabId;
use crate::{
    Button, ButtonVariant, Expand, HStack, IconButton, IconButtonSize, IconWidget, ListView, Panel,
    PopoverIconButton,
};
use bastyde_core::accesskit::HasPopup;
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole};

use std::collections::HashMap;

/// Default min width for an unpinned tab.
pub const DEFAULT_MIN_TAB_WIDTH: f32 = 96.0;
/// Default max width for an unpinned tab.
pub const DEFAULT_MAX_TAB_WIDTH: f32 = 240.0;
/// Default spacing between tab headers in the row. `0.0` so tabs sit
/// flush against each other (Firefox / Chrome convention) — adjacent
/// tab boundaries are visually separated by the per-tab borders, not
/// by an empty gap.
pub const DEFAULT_TAB_SPACING: f32 = 0.0;
/// Default spacing between the bar's leading slot, scroll area, and
/// trailing slot.
pub const DEFAULT_BAR_SLOT_SPACING: f32 = 8.0;
/// Default width (in dp) of a pinned tab — icon-only squares.
pub const DEFAULT_PINNED_TAB_WIDTH: f32 = 32.0;
/// Distance (in dp) one click of a scroll arrow advances the
/// horizontal scroll position. Roughly one tab's worth.
const SCROLL_ARROW_STEP: f32 = 120.0;
/// Pixels-per-line conversion for `ScrollDelta::Lines`. Mouse wheels
/// send their deltas in units of "lines"; the bar treats one line as
/// roughly one tab-width's worth of scrolling so a single notch
/// scrolls one full tab into view.
const WHEEL_LINE_PIXELS: f32 = 64.0;
/// Edge-zone width inside which `on_drag_tick` ramps the auto-scroll
/// velocity up to [`DRAG_MAX_VELOCITY`].
const DRAG_EDGE_ZONE: f32 = 32.0;
/// Cap on per-frame auto-scroll velocity during a drag at the bar
/// edges.
const DRAG_MAX_VELOCITY: f32 = 12.0;

/// Drag payload published by a tab header when the user starts
/// dragging it.
///
/// Generic over the bar's item type `T` so a `TabBar<T>` only ever
/// downcasts (`get_typed::<TabBarDragData<T>>()`) a drag started by
/// another `TabBar<T>` — a drag from a `TabBar<OtherT>` simply never
/// matches, giving cross-bar transfer type-safety for free.
///
/// Two consumers:
/// - **Intra-bar reorder**: the bar's own `on_drop` matches
///   `source_bar_id == self_id` and uses `source_index` to drive
///   `move_item`. `item` is unused on this path (and may be `None`).
/// - **Cross-bar transfer**: a *different* bar that opted in via
///   [`accept_external_tabs`](TabBar::accept_external_tabs) takes
///   `item` by value and hands it to its
///   [`on_tab_received`](TabBar::on_tab_received) callback. `item` is
///   `Some` only when the source bar opted in *and* the per-tab
///   transferable predicate allows it (static tabs are excluded).
pub struct TabBarDragData<T: 'static> {
    /// Model index of the dragged tab in the *source* bar.
    pub source_index: usize,
    /// Widget id of the source `TabBar`. The receiving bar compares
    /// it to its own id to tell an intra-bar reorder from a
    /// cross-bar transfer.
    pub source_bar_id: WidgetId,
    /// Stable id of the dragged tab — handed to the source bar's
    /// `on_transfer_out` so the app can remove it by id.
    pub source_id: TabId,
    /// A clone of the dragged item, carried for cross-bar transfer.
    /// `None` when the source bar didn't opt into transfer or the tab
    /// is non-transferable (e.g. a static tab).
    pub item: Option<T>,
}

/// A reactive header strip that pulls its tab list from a data source
/// and writes the active tab into a shared `Signal<Option<TabId>>`.
///
/// Selection is **id-based**: the bar holds a stable [`TabId`] per
/// item (extracted via the `id_of` closure passed to the constructor)
/// and the public `selected_id` signal is the source of truth across
/// reorders / removals / locale changes. Internal index-based work
/// (keyboard nav, scroll-to-active, click activation) reads a
/// **private** `selected_index` signal that the bar keeps in
/// bidirectional sync with `selected_id` at build time.
pub struct TabBar<T: 'static> {
    source: ListSource<T>,
    delegate: TabDelegate<T>,
    /// Public selection signal — id-based, stable across reorders.
    selected_id: Signal<Option<TabId>>,
    /// Closure that extracts a stable [`TabId`] from each model item.
    /// Called per-item at every build.
    id_of: Rc<dyn Fn(usize, &T) -> TabId>,
    /// Private index signal used by internal index-based code
    /// (keyboard nav, scroll, click). Synced with `selected_id` at
    /// build time via two `ctx.effect`s installed in [`Widget::build`].
    selected: Signal<usize>,

    orientation: TabBarOrientation,
    sizing: TabSizing,
    tab_display: TabDisplayMode,
    min_tab_width: f32,
    max_tab_width: f32,
    pinned_tab_width: f32,
    spacing: f32,
    /// Optional tab-strip cross-axis extent override (compact bars).
    tab_height: Option<f32>,

    /// Uniform surface color/role applied to every tab header in the
    /// strip — selected, idle, and hovered all paint the same fill,
    /// so selection is conveyed by the accent indicator and the
    /// label-color shift only. Default `None` = transparent.
    tab_surface_role: Option<bastyde_core::color_prop::ColorProp>,
    /// Text role used for the label (and matching icon tint) on the
    /// selected tab. Default: `TextRole::Primary`.
    selected_text_role: TextRole,
    /// Text role used for the label (and matching icon tint) on idle
    /// tabs (not selected, not disabled). Default: `TextRole::Secondary`.
    idle_text_role: TextRole,
    /// Per-call style override propagated to every header in the bar.
    /// `None` means "use the theme slot or the bundled `RecipeTabStyle`".
    style_override: Option<bastyde_core::styles::SharedTabStyle>,

    bar_leading_slot: Option<PendingChild>,
    bar_trailing_slot: Option<PendingChild>,

    show_separator: bool,
    show_scroll_arrows: bool,
    show_overflow_dropdown: bool,
    vertical_wheel_scrolls_horizontally: bool,
    shift_wheel_scrolls_horizontally: bool,

    on_close: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    reorderable: bool,
    on_reorder: Option<Rc<dyn Fn(usize, usize, &mut EventContext)>>,
    on_pin_toggle: Option<Rc<dyn Fn(usize, bool, &mut EventContext)>>,

    /// Cross-bar transfer opt-in. When `true`, headers publish an
    /// item-carrying [`TabBarDragData`] (a drag source) AND the bar
    /// accepts foreign tabs as a drop target. Set via
    /// [`accept_external_tabs`](Self::accept_external_tabs).
    accept_external_tabs: bool,
    /// Item-clone closure, installed by
    /// [`accept_external_tabs`](Self::accept_external_tabs) where
    /// `T: Clone`. Captures the `Clone` capability so `build()` (which
    /// is not `T: Clone`-bounded) can produce the carried item clone.
    /// `None` ⇒ payloads carry `item: None` (reorder-only).
    clone_item: Option<Rc<dyn Fn(&T) -> T>>,
    /// Target-side callback: a foreign tab was dropped here. Receives
    /// the moved item, the model insertion index in *this* bar, and
    /// the firing context. The app inserts into its own model.
    on_tab_received: Option<Rc<dyn Fn(T, usize, &mut EventContext)>>,
    /// Source-side callback: one of this bar's tabs was accepted by a
    /// *different* bar. Receives the transferred tab's id; the app
    /// removes it from its own model.
    on_transfer_out: Option<Rc<dyn Fn(TabId, &mut EventContext)>>,
    /// Drop handler for **non-tab** payloads — an in-app foreign drag
    /// (a tree/list row carrying app data) or an OS file/text/URL
    /// drop. Receives the raw payload, the model insertion index, and
    /// the firing context; returns `true` if accepted. Distinct from
    /// [`on_tab_received`](Self::on_tab_received), which only handles
    /// tabs dragged from a peer `TabBar<T>`.
    on_external_drop: Option<Rc<dyn Fn(&DragPayload, usize, &mut EventContext) -> bool>>,
    /// Per-tab transferable predicate. `None` ⇒ all tabs transferable.
    /// `TabWidget` installs one that excludes static tabs.
    transferable_fn: Option<Rc<dyn Fn(usize, &T) -> bool>>,
    /// Set `true` by the bar's own `on_drop` when it consumes a drag
    /// as an intra-bar reorder; read-and-reset by the source header's
    /// `on_drag_ended` to suppress a spurious `on_transfer_out` (which
    /// would otherwise remove the just-reordered tab). `on_drop` runs
    /// before `on_drag_ended` in the same dispatch, so no reset-at-
    /// drag-start is needed.
    self_reorder_flag: Rc<std::cell::Cell<bool>>,

    /// Optional shared buffer the parent `TabWidget<T>` populates with
    /// its content panel ids so the headers can publish the
    /// `controls()` accessibility relation. `None` for stand-alone
    /// `TabBar` use — the headers simply omit the relation in that
    /// case (which is the right semantics: there is no panel to
    /// control).
    panel_ids_buffer: Option<Rc<RefCell<Vec<WidgetId>>>>,

    /// Optional shared buffer the parent `TabWidget<T>` reads after
    /// the bar builds to obtain each header's `WidgetId` (in tab
    /// order). Used to wire the `TabPanel → aria-labelledby → Tab`
    /// accessibility relation on the TabPane side. `None` for
    /// stand-alone `TabBar` use.
    header_ids_buffer: Option<Rc<RefCell<Vec<WidgetId>>>>,

    /// Drop indicator x position in bar-local coords, painted by
    /// `paint()`. `None` means no drag in progress / not dropping
    /// here. Cloned into the on_drag_hover / on_drag_leave handlers
    /// at build time and into the bar's paint via `paint_state`.
    paint_state: PaintState,

    root_child_id: Option<WidgetId>,

    /// Direct widget-id handles to the bar's natural-width
    /// contributors. In vertical orientation, `layout_response`
    /// probes each at unspecified width to compute the bar's
    /// intrinsic width (max across them), then clamps to
    /// `[min_tab_width, max_tab_width]`. Bypasses the inner
    /// `ScrollArea` whose own `layout_response` echoes its
    /// proposal, which would otherwise let the bar swallow
    /// whatever cross-axis space the parent gave it.
    header_row_id: Option<WidgetId>,
    pinned_strip_id: Option<WidgetId>,
    bar_leading_slot_id: Option<WidgetId>,
    bar_trailing_slot_id: Option<WidgetId>,
}

#[derive(Clone)]
struct PaintState {
    /// Drop-indicator x in bar-local coords (`Some(x)`) or `None`
    /// when no drag is in progress over the bar. A `Signal` (not a
    /// bare `Cell`) so the `TabStyle`-built chrome painter can bind
    /// to it and repaint when a drag updates the insertion point.
    drop_indicator_x: Signal<Option<f32>>,
    /// Cached bar world bounds recorded by `place_children`. Drop
    /// handlers use the origin to translate world-coords header
    /// bounds into bar-local space, and the size to detect when the
    /// pointer is in the edge auto-scroll zone.
    last_bar_bounds: Rc<std::cell::Cell<Rect>>,
}

impl Default for PaintState {
    fn default() -> Self {
        Self {
            drop_indicator_x: Signal::new(None),
            last_bar_bounds: Rc::new(std::cell::Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0))),
        }
    }
}

impl std::fmt::Debug for PaintState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintState")
            .field("drop_indicator_x", &self.drop_indicator_x.get())
            .field("last_bar_bounds", &self.last_bar_bounds.get())
            .finish()
    }
}

impl<T: 'static> std::fmt::Debug for TabBar<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabBar")
            .field("len", &self.source.len())
            .field("selected", &self.selected.get())
            .field("sizing", &self.sizing)
            .field("min_tab_width", &self.min_tab_width)
            .field("max_tab_width", &self.max_tab_width)
            .finish()
    }
}

impl<T: 'static> TabBar<T> {
    /// Construct a horizontal tab bar from a [`ListModel<T>`].
    /// Default sizing is [`TabSizing::Shared`].
    ///
    /// `selected_id` is the id-based selection signal — written by
    /// the bar on click / keyboard / drag-drop and observable by
    /// callers. `id_of(index, &item)` extracts the stable [`TabId`]
    /// from each model item.
    pub fn horizontal(
        model: ListModel<T>,
        delegate: TabDelegate<T>,
        selected_id: Signal<Option<TabId>>,
        id_of: impl Fn(usize, &T) -> TabId + 'static,
    ) -> Self {
        Self::from_list_source(
            ListSource::from_model(model),
            delegate,
            selected_id,
            Rc::new(id_of),
            TabBarOrientation::Horizontal,
        )
    }

    /// Construct a horizontal tab bar from any [`ListDataSource`].
    /// Default sizing is [`TabSizing::Shared`].
    pub fn horizontal_from_source<S: ListDataSource<Item = T>>(
        source: S,
        delegate: TabDelegate<T>,
        selected_id: Signal<Option<TabId>>,
        id_of: impl Fn(usize, &T) -> TabId + 'static,
    ) -> Self {
        Self::from_list_source(
            ListSource::from_data_source(source),
            delegate,
            selected_id,
            Rc::new(id_of),
            TabBarOrientation::Horizontal,
        )
    }

    /// Construct a vertical tab bar from a [`ListModel<T>`]. Tabs
    /// stack top-to-bottom as horizontal pills (icon + label + close
    /// button arranged left-to-right within each pill). Default
    /// sizing is [`TabSizing::Shared`] — uniform pill heights.
    pub fn vertical(
        model: ListModel<T>,
        delegate: TabDelegate<T>,
        selected_id: Signal<Option<TabId>>,
        id_of: impl Fn(usize, &T) -> TabId + 'static,
    ) -> Self {
        Self::from_list_source(
            ListSource::from_model(model),
            delegate,
            selected_id,
            Rc::new(id_of),
            TabBarOrientation::Vertical,
        )
    }

    /// Construct a vertical tab bar from any [`ListDataSource`].
    pub fn vertical_from_source<S: ListDataSource<Item = T>>(
        source: S,
        delegate: TabDelegate<T>,
        selected_id: Signal<Option<TabId>>,
        id_of: impl Fn(usize, &T) -> TabId + 'static,
    ) -> Self {
        Self::from_list_source(
            ListSource::from_data_source(source),
            delegate,
            selected_id,
            Rc::new(id_of),
            TabBarOrientation::Vertical,
        )
    }

    pub(crate) fn from_list_source(
        source: ListSource<T>,
        delegate: TabDelegate<T>,
        selected_id: Signal<Option<TabId>>,
        id_of: Rc<dyn Fn(usize, &T) -> TabId>,
        orientation: TabBarOrientation,
    ) -> Self {
        Self {
            source,
            delegate,
            selected_id,
            id_of,
            selected: Signal::new(0_usize),
            orientation,
            sizing: TabSizing::Shared,
            tab_display: TabDisplayMode::Auto,
            min_tab_width: DEFAULT_MIN_TAB_WIDTH,
            max_tab_width: DEFAULT_MAX_TAB_WIDTH,
            pinned_tab_width: DEFAULT_PINNED_TAB_WIDTH,
            spacing: DEFAULT_TAB_SPACING,
            tab_height: None,
            tab_surface_role: None,
            selected_text_role: TextRole::Primary,
            idle_text_role: TextRole::Secondary,
            style_override: None,
            bar_leading_slot: None,
            bar_trailing_slot: None,
            show_separator: true,
            show_scroll_arrows: true,
            show_overflow_dropdown: true,
            vertical_wheel_scrolls_horizontally: true,
            shift_wheel_scrolls_horizontally: true,
            on_close: None,
            reorderable: false,
            on_reorder: None,
            on_pin_toggle: None,
            accept_external_tabs: false,
            clone_item: None,
            on_tab_received: None,
            on_transfer_out: None,
            on_external_drop: None,
            transferable_fn: None,
            self_reorder_flag: Rc::new(std::cell::Cell::new(false)),
            panel_ids_buffer: None,
            header_ids_buffer: None,
            paint_state: PaintState::default(),
            root_child_id: None,
            header_row_id: None,
            pinned_strip_id: None,
            bar_leading_slot_id: None,
            bar_trailing_slot_id: None,
        }
    }

    /// Override the per-tab sizing strategy. See [`TabSizing`].
    pub fn tab_sizing(mut self, mode: TabSizing) -> Self {
        self.sizing = mode;
        self
    }

    /// Choose what every tab shows — icon, label, or both. See
    /// [`TabDisplayMode`]. Default [`TabDisplayMode::Auto`] (render each tab as
    /// its `TabInfo` declares).
    pub fn tab_display(mut self, mode: TabDisplayMode) -> Self {
        self.tab_display = mode;
        self
    }

    /// Minimum width (in dp) any unpinned tab will be drawn at.
    /// Default: [`DEFAULT_MIN_TAB_WIDTH`].
    ///
    /// In **horizontal** orientation this clamps the **per-tab** width.
    /// In **vertical** orientation every tab is forced to the bar's
    /// cross-axis width, so the same knob defines the bar's minimum
    /// width — the sidebar adapts to the widest piece of bar content
    /// (tab labels or a slot widget) and never shrinks below this floor.
    /// Vertical pill heights stay at `theme.components.tab.editor_tab_height`
    /// regardless of this knob.
    pub fn min_tab_width(mut self, dp: f32) -> Self {
        self.min_tab_width = dp.max(0.0);
        self
    }

    /// Override the tab-strip cross-axis extent (the strip height for a
    /// horizontal bar; the per-tab pill height for a vertical one). `None`
    /// keeps the style's `editor_tab_height`. Use for a compact bar.
    pub fn tab_bar_height(mut self, dp: f32) -> Self {
        self.tab_height = Some(dp.max(0.0));
        self
    }

    /// Maximum width (in dp) any unpinned tab will be drawn at — long
    /// labels truncate with an ellipsis at this width.
    /// Default: [`DEFAULT_MAX_TAB_WIDTH`].
    ///
    /// In **horizontal** orientation this clamps the **per-tab** width.
    /// In **vertical** orientation it caps the whole sidebar's width —
    /// see [`min_tab_width`](Self::min_tab_width) for the symmetric
    /// adapt-to-content rule.
    pub fn max_tab_width(mut self, dp: f32) -> Self {
        self.max_tab_width = dp.max(0.0);
        self
    }

    /// Override the spacing (in dp) between adjacent tab headers in
    /// the row. Default: [`DEFAULT_TAB_SPACING`].
    pub fn tab_spacing(mut self, dp: f32) -> Self {
        self.spacing = dp.max(0.0);
        self
    }

    /// Width (in dp) of an icon-only pinned tab.
    /// Default: [`DEFAULT_PINNED_TAB_WIDTH`].
    pub fn pinned_tab_width(mut self, dp: f32) -> Self {
        self.pinned_tab_width = dp.max(0.0);
        self
    }

    /// Set the surface color/role applied to **every** tab — selected,
    /// idle, and hovered all paint the same fill. Accepts any `Color`,
    /// `SurfaceRole`, or `Signal<Color>` (via [`ColorProp`](bastyde_core::color_prop::ColorProp)).
    /// Default `None` = transparent.
    pub fn tab_surface_role(
        mut self,
        color: impl Into<bastyde_core::color_prop::ColorProp>,
    ) -> Self {
        self.tab_surface_role = Some(color.into());
        self
    }

    /// Set the text role used for the label (and matching icon tint)
    /// on the **selected** tab. Default: [`TextRole::Primary`] — the
    /// Int UI editor-strip convention. Override to e.g.
    /// [`TextRole::Accent`] when the strip sits over a tinted surface.
    pub fn selected_text_role(mut self, role: TextRole) -> Self {
        self.selected_text_role = role;
        self
    }

    /// Set the text role used for the label (and matching icon tint)
    /// on **idle** tabs (not selected, not disabled). Default:
    /// [`TextRole::Secondary`]. Disabled tabs always read as
    /// [`TextRole::Disabled`] regardless of this setting.
    pub fn idle_text_role(mut self, role: TextRole) -> Self {
        self.idle_text_role = role;
        self
    }

    /// Override the active [`TabStyle`](bastyde_core::styles::TabStyle)
    /// for every header in this bar. The widget keeps responsibility
    /// for the label / icon / close button composition, the
    /// optional `tab_surface_role` background, and all input handling;
    /// the style only paints the accent indicator and focus ring
    /// chrome via `make_body`. Per-call override > theme slot >
    /// built-in `RecipeTabStyle` default.
    pub fn style(mut self, style: impl bastyde_core::styles::TabStyle) -> Self {
        self.style_override = Some(std::rc::Rc::new(style));
        self
    }

    /// Install a pin-toggle handler called whenever the user crosses
    /// a pinned tab over the unpinned region or vice-versa during a
    /// drag. Receives `(model_index, new_pinned_flag, ctx)`. The
    /// firing [`EventContext`] lets the handler confirm the
    /// transition via a dialog or route it through an intent before
    /// mutating the item; apps decide whether to actually flip the
    /// pinned state.
    pub fn on_pin_toggle(mut self, f: impl Fn(usize, bool, &mut EventContext) + 'static) -> Self {
        self.on_pin_toggle = Some(Rc::new(f));
        self
    }

    /// Bar-level leading slot — a widget rendered before the headers
    /// row (and before any pinned region in later phases).
    pub fn bar_leading_slot(mut self, w: impl Widget + 'static) -> Self {
        self.bar_leading_slot = Some(PendingChild::Deferred(Box::new(w)));
        self
    }

    /// Bar-level leading slot accepting a pre-registered widget id.
    pub fn bar_leading_slot_id(mut self, id: WidgetId) -> Self {
        self.bar_leading_slot = Some(PendingChild::Id(id));
        self
    }

    /// Bar-level trailing slot — a widget rendered after the headers
    /// row (and after any overflow dropdown in later phases).
    pub fn bar_trailing_slot(mut self, w: impl Widget + 'static) -> Self {
        self.bar_trailing_slot = Some(PendingChild::Deferred(Box::new(w)));
        self
    }

    /// Bar-level trailing slot accepting a pre-registered widget id.
    pub fn bar_trailing_slot_id(mut self, id: WidgetId) -> Self {
        self.bar_trailing_slot = Some(PendingChild::Id(id));
        self
    }

    /// Toggle the 1 dp bottom separator the bar paints under the
    /// headers. Default: on.
    pub fn separator(mut self, on: bool) -> Self {
        self.show_separator = on;
        self
    }

    /// Toggle the leading + trailing scroll-arrow buttons. They
    /// auto-show when the headers row overflows the bar's viewport,
    /// and click animates the scroll position by one tab-width.
    /// Default: on.
    pub fn show_scroll_arrows(mut self, on: bool) -> Self {
        self.show_scroll_arrows = on;
        self
    }

    /// Toggle the trailing "show all tabs" overflow dropdown — a
    /// `Popover` with a `MenuList` of every tab. Default: on.
    pub fn show_overflow_dropdown(mut self, on: bool) -> Self {
        self.show_overflow_dropdown = on;
        self
    }

    /// On a horizontal bar, treat a plain vertical-wheel event as a
    /// horizontal scroll (Firefox / Chrome convention). Has no
    /// effect on vertical or multi-line bars (those still scroll
    /// vertically). Default: on.
    pub fn vertical_wheel_scrolls_horizontally(mut self, on: bool) -> Self {
        self.vertical_wheel_scrolls_horizontally = on;
        self
    }

    /// `Shift` + vertical wheel forces a horizontal scroll regardless
    /// of orientation. Default: on.
    pub fn shift_wheel_scrolls_horizontally(mut self, on: bool) -> Self {
        self.shift_wheel_scrolls_horizontally = on;
        self
    }

    /// Install a close-tab handler called whenever the user clicks a
    /// closable tab's close button, middle-clicks the tab header, or
    /// presses `Delete` on a focused tab. The handler receives the
    /// firing [`EventContext`] so it can open a confirmation dialog
    /// (`ctx.present_modal(MessageBox::confirm(...))`), dispatch an
    /// intent, or otherwise route the close request through the
    /// framework. To veto the close, do nothing in the handler; to
    /// confirm-then-close, run the confirmation flow and only mutate
    /// the underlying model on accept.
    ///
    /// If unset and the bar is backed by a [`ListModel<T>`], the
    /// default behavior is to remove the item at the given index
    /// from the model (no confirmation, no ctx needed for that path).
    pub fn on_close(mut self, f: impl Fn(usize, &mut EventContext) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// Enable drag-to-reorder. Each tab header becomes a drag source
    /// and the bar accepts drops anywhere along the headers row,
    /// painting an insertion-line indicator at the would-be
    /// position. On drop the bar calls [`on_reorder`](Self::on_reorder)
    /// — falling back to `ListModel::move_item` when the bar is
    /// backed by a `ListModel<T>` and no explicit handler is set.
    /// Default: off.
    pub fn reorderable(mut self, on: bool) -> Self {
        self.reorderable = on;
        self
    }

    /// Install a reorder handler called whenever the user drag-drops
    /// a tab to a new position. Receives `(from, to, ctx)` —
    /// `from`/`to` are model indices and `ctx` is the firing
    /// [`EventContext`] so the handler can open a confirmation
    /// dialog or dispatch an intent before persisting the move.
    /// Implies [`reorderable(true)`](Self::reorderable).
    pub fn on_reorder(mut self, f: impl Fn(usize, usize, &mut EventContext) + 'static) -> Self {
        self.on_reorder = Some(Rc::new(f));
        self.reorderable = true;
        self
    }

    /// Opt into cross-bar tab transfer. When enabled, this bar's
    /// headers become transfer drag sources (their drag payload
    /// carries a clone of the dragged item) **and** the bar accepts
    /// tabs dragged from *other* `TabBar<T>`s, painting the same
    /// insertion-line indicator as an intra-bar reorder.
    ///
    /// Requires `T: Clone` — the dragged item is cloned into the
    /// payload (cheap for handle-like `T` whose heavy state lives
    /// behind an `Rc`). Default: off.
    ///
    /// Pair with [`on_tab_received`](Self::on_tab_received) (this bar,
    /// as a drop target — insert the item into your model) and
    /// [`on_transfer_out`](Self::on_transfer_out) (the source bar —
    /// remove the tab from your model).
    pub fn accept_external_tabs(mut self, on: bool) -> Self
    where
        T: Clone,
    {
        self.accept_external_tabs = on;
        self.clone_item = if on {
            Some(Rc::new(|t: &T| t.clone()))
        } else {
            None
        };
        self
    }

    /// Install the target-side callback fired when a foreign tab is
    /// dropped onto this bar. Receives `(item, insertion_index, ctx)`
    /// — the moved item (taken by value from the drag payload), the
    /// model index in *this* bar where it should land, and the firing
    /// context. The app inserts the item into its own model. Implies
    /// [`accept_external_tabs(true)`](Self::accept_external_tabs).
    pub fn on_tab_received(mut self, f: impl Fn(T, usize, &mut EventContext) + 'static) -> Self
    where
        T: Clone,
    {
        self.on_tab_received = Some(Rc::new(f));
        if !self.accept_external_tabs {
            self = self.accept_external_tabs(true);
        }
        self
    }

    /// Install the source-side callback fired after one of this bar's
    /// tabs has been accepted by a *different* bar. Receives the
    /// transferred tab's [`TabId`]; the app removes it from its own
    /// model. Not fired for intra-bar reorders (those go through
    /// [`on_reorder`](Self::on_reorder)) or rejected / cancelled
    /// drags. Implies [`accept_external_tabs(true)`](Self::accept_external_tabs).
    pub fn on_transfer_out(mut self, f: impl Fn(TabId, &mut EventContext) + 'static) -> Self
    where
        T: Clone,
    {
        self.on_transfer_out = Some(Rc::new(f));
        if !self.accept_external_tabs {
            self = self.accept_external_tabs(true);
        }
        self
    }

    /// Accept **non-tab** drops onto the bar — an in-app foreign drag
    /// (e.g. a file dragged from a `TreeView`, carrying app data) or an
    /// OS file/text/URL drop. The bar paints the same insertion-line
    /// indicator while such a payload hovers, and on drop calls `f`
    /// with the raw [`DragPayload`], the model insertion index, and the
    /// firing context. Return `true` if accepted — the app inspects the
    /// payload (`get_typed::<T>()` / `files()` / `text()` / `uris()`)
    /// and mints whatever it needs (e.g. opens a tab).
    ///
    /// Independent of [`accept_external_tabs`](Self::accept_external_tabs):
    /// a bar can accept foreign tabs, non-tab payloads, both, or
    /// neither. OS drops additionally require the app to have called
    /// `BastydeAppBuilder::install_external_dnd()`.
    ///
    /// Note: the hover indicator is *optimistic* — it shows for any
    /// non-tab payload while this handler is installed; `f`'s return
    /// value is authoritative at drop time.
    pub fn on_external_drop(
        mut self,
        f: impl Fn(&DragPayload, usize, &mut EventContext) -> bool + 'static,
    ) -> Self {
        self.on_external_drop = Some(Rc::new(f));
        self
    }

    /// Internal hook: install the non-tab drop handler. `pub(crate)`
    /// because `TabWidget` wires its own index-translation layer.
    pub(crate) fn on_external_drop_rc(
        mut self,
        f: Rc<dyn Fn(&DragPayload, usize, &mut EventContext) -> bool>,
    ) -> Self {
        self.on_external_drop = Some(f);
        self
    }

    /// Internal hook: install a per-tab transferable predicate.
    /// `TabWidget` uses it to exclude static tabs (whose content has
    /// no factory on a receiving bar) from cross-bar transfer. When
    /// the predicate returns `false`, the tab's drag payload carries
    /// `item: None` and a foreign bar rejects the drop.
    pub(crate) fn with_transferable_predicate(
        mut self,
        f: impl Fn(usize, &T) -> bool + 'static,
    ) -> Self {
        self.transferable_fn = Some(Rc::new(f));
        self
    }

    /// Internal hook: install the source-side transfer-out callback.
    /// `pub(crate)` because `TabWidget` wires its own translation
    /// layer; the public entry point is on `TabWidget`.
    pub(crate) fn on_transfer_out_rc(mut self, f: Rc<dyn Fn(TabId, &mut EventContext)>) -> Self {
        self.on_transfer_out = Some(f);
        self
    }

    /// Internal hook: install the target-side received callback.
    /// `pub(crate)` because `TabWidget` wires its own translation
    /// layer; the public entry point is on `TabWidget`.
    pub(crate) fn on_tab_received_rc(mut self, f: Rc<dyn Fn(T, usize, &mut EventContext)>) -> Self {
        self.on_tab_received = Some(f);
        self
    }

    /// Internal hook used by `TabWidget<T>` to share a panel-ids
    /// buffer with this bar. The wrapping widget passes its
    /// `Switcher`'s captured panel ids in; the headers read them in
    /// `accessibility()` to publish the Tab → TabPanel `controls()`
    /// relation.
    pub(crate) fn with_panel_ids(mut self, buffer: Rc<RefCell<Vec<WidgetId>>>) -> Self {
        self.panel_ids_buffer = Some(buffer);
        self
    }

    /// Share the bar's header-ids buffer with the parent so each
    /// `TabPane` can wire its `aria-labelledby` relation to the
    /// header at the matching index. Populated by `build()` once
    /// every header has been added to the arena; readers must
    /// `borrow()` after the bar's build pass.
    pub(crate) fn with_header_ids(mut self, buffer: Rc<RefCell<Vec<WidgetId>>>) -> Self {
        self.header_ids_buffer = Some(buffer);
        self
    }
}

impl<T: 'static> Widget for TabBar<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild on data-source changes. We store a `version: Signal<u64>`
        // bound at `BindingLevel::Rebuild`; the observer increments it
        // for every `DataChange`. Lifetime of the observer is tied to
        // this build pass via `ctx.own_handle(...)`.
        let self_id = ctx.self_id();
        let version = ctx.signal(0u64);
        version.bind_to(self_id, ctx.binding_registry(), BindingLevel::Rebuild);

        let data_ver = Rc::new(std::cell::Cell::new(0_u64));
        let observer_handle = (self.source.observe_fn)(Box::new({
            let version = version.clone();
            let dv = data_ver.clone();
            move |_change| {
                let next = dv.get().wrapping_add(1);
                dv.set(next);
                version.set(next);
            }
        }));
        ctx.own_handle(observer_handle);

        // Snapshot enabled + pinned flags up front. Headers need
        // the full enabled vector (for arrow-key skip-over) and we
        // need pinned[i] to partition the layout into pinned strip
        // vs scrollable region. The `ListSource::with_item_fn` API
        // is widget-shaped, so we side-channel the booleans through
        // a `Cell` and discard the throwaway widget it produces.
        let n = self.source.len();
        let mut enabled_tabs = Vec::with_capacity(n);
        let mut pinned_tabs: Vec<bool> = Vec::with_capacity(n);
        for i in 0..n {
            let cell = std::cell::Cell::new((true, false));
            (self.source.with_item_fn)(i, &|item| {
                cell.set((
                    self.delegate.resolve_enabled(i, item),
                    self.delegate.resolve_pinned(i, item),
                ));
                Box::new(EnabledProbe) as Box<dyn Widget>
            });
            let (e, p) = cell.get();
            enabled_tabs.push(e);
            pinned_tabs.push(p);
        }
        let enabled_tabs = Rc::new(enabled_tabs);

        // ── Bidirectional id ↔ index selection sync ────────────────
        //
        // The bar's PUBLIC API is id-based (`selected_id`); its
        // internal index-based code (keyboard, scroll, click) reads
        // `selected` (the private index signal). At build time we:
        //
        //   1. Compute id↔index lookup tables from the live model
        //      via `id_of`.
        //   2. Pre-build sync: bring the two signals into agreement —
        //      valid id wins, stale id falls back to the
        //      previously-selected index clamped into range
        //      (positional fallback = next neighbor of the closed
        //      tab; browser convention).
        //   3. Install two `ctx.effect`s for steady-state propagation:
        //      external id changes → index, internal index changes
        //      (from header click / keyboard) → id. No-op guards
        //      prevent ping-pong.
        let mut id_to_index: HashMap<TabId, usize> = HashMap::with_capacity(n);
        let mut index_to_id: Vec<TabId> = Vec::with_capacity(n);
        for i in 0..n {
            let cell: std::cell::Cell<Option<TabId>> = std::cell::Cell::new(None);
            (self.source.with_item_fn)(i, &|item| {
                cell.set(Some((self.id_of)(i, item)));
                Box::new(EnabledProbe) as Box<dyn Widget>
            });
            if let Some(id) = cell.get() {
                id_to_index.insert(id, i);
                index_to_id.push(id);
            }
        }
        let id_to_index = Rc::new(id_to_index);
        let index_to_id = Rc::new(index_to_id);

        if n > 0 {
            let valid = self
                .selected_id
                .get()
                .and_then(|id| id_to_index.get(&id).copied());
            if let Some(target_idx) = valid {
                if self.selected.get() != target_idx {
                    self.selected.set(target_idx);
                }
            } else {
                let clamped = self.selected.get().min(n - 1);
                if self.selected.get() != clamped {
                    self.selected.set(clamped);
                }
                let new_id = index_to_id[clamped];
                if self.selected_id.get() != Some(new_id) {
                    self.selected_id.set(Some(new_id));
                }
            }
        } else if self.selected_id.get().is_some() {
            self.selected_id.set(None);
        }

        let id_to_idx_for_eff = id_to_index.clone();
        let idx_for_id_eff = self.selected.clone();
        ctx.effect(&self.selected_id, move |maybe_id| {
            if let Some(id) = maybe_id
                && let Some(&i) = id_to_idx_for_eff.get(id)
                && idx_for_id_eff.get() != i
            {
                idx_for_id_eff.set(i);
            }
        });
        let idx_to_id_for_eff = index_to_id.clone();
        let id_for_idx_eff = self.selected_id.clone();
        ctx.effect(&self.selected, move |i| {
            let new_id = idx_to_id_for_eff.get(*i).copied();
            if id_for_idx_eff.get() != new_id {
                id_for_idx_eff.set(new_id);
            }
        });

        let header_ids_buf = self
            .header_ids_buffer
            .clone()
            .unwrap_or_else(|| Rc::new(RefCell::new(Vec::with_capacity(n))));
        // If a parent provided a pre-allocated buffer (e.g.
        // `TabWidget` rebuilding after a dynamic-model mutation),
        // clear stale entries so the new tab order replaces — never
        // appends to — the prior pass.
        header_ids_buf.borrow_mut().clear();
        let panel_ids_buf = self
            .panel_ids_buffer
            .clone()
            .unwrap_or_else(|| Rc::new(RefCell::new(Vec::new())));
        let shared = Rc::new(HeaderShared {
            header_ids: header_ids_buf.clone(),
            panel_ids: panel_ids_buf,
            enabled_tabs: enabled_tabs.clone(),
        });

        // Pinned tabs render in a leading non-scrolling strip;
        // unpinned tabs go inside the scrollable TabHeaderRow.
        // We accumulate both lists here, then compose the row_outer
        // with the strips in the right order below.
        let mut pinned_header_ids: Vec<WidgetId> = Vec::new();
        let mut unpinned_header_ids: Vec<WidgetId> = Vec::with_capacity(n);
        // Maps each unpinned-region position to its index in the
        // **model**. Used by the drop handler to translate the
        // `insertion_index_for(...)` result (which is in unpinned
        // space — `header_bounds_buf` only contains the unpinned
        // row's bounds) to a model index that `move_item` can
        // consume directly.
        let mut unpinned_to_model: Vec<usize> = Vec::with_capacity(n);
        // Collected per-tab labels are reused by the overflow
        // dropdown's MenuList. Resolved at build time → re-resolved on
        // every data-source change (the bar rebuilds via `version`)
        // and on every locale change (because the dropdown's
        // MenuItems consume `LocalizedString` directly, which carries
        // its own reactive resolver).
        let mut header_labels: Vec<LocalizedString> = Vec::with_capacity(n);

        // Reorder handler. Explicit `on_reorder` wins; otherwise
        // fall back to the source's `move_item_fn` (populated for
        // ListModel-backed bars).
        //
        // No pre-emptive `selected.set(...)` here: selection is
        // **id-based**. The id stored in `selected_id` is unchanged
        // by a reorder (the same tab is just at a different index),
        // and the bar's pre-build sync re-resolves the id → index
        // mapping during the rebuild that the model mutation
        // triggers. Writing the bar's private `selected` index
        // signal *before* the move would fire the index → id
        // effect against the pre-move `index_to_id` map and stamp
        // the wrong id into `selected_id`, which the post-rebuild
        // sync would then promote — causing the active tab to
        // change visually (and the content pane to fall out of
        // sync) on every drag.
        let reorder_handler: Option<Rc<dyn Fn(usize, usize, &mut EventContext)>> =
            if self.reorderable {
                if let Some(explicit) = self.on_reorder.clone() {
                    Some(explicit)
                } else {
                    self.source.move_item_fn.clone().map(|move_fn| {
                        Rc::new(move |from: usize, to: usize, _ctx: &mut EventContext| {
                            (move_fn)(from, to);
                        }) as Rc<dyn Fn(usize, usize, &mut EventContext)>
                    })
                }
            } else {
                None
            };

        // Close handler. The explicit `on_close` overrides everything;
        // otherwise we fall back to the source's `remove_item_fn`
        // (populated when backed by a `ListModel`) and lift it into
        // the ctx-accepting shape by ignoring ctx. Same id-based
        // discipline as reorder: don't pre-empt the index signal.
        // After model.remove the rebuild's pre-build sync handles
        // both the "selected id still valid" case (re-indexes to
        // the survivor) and the "selected id stale" case (stale-id
        // fallback picks the next neighbor, browser convention).
        let close_handler: Option<Rc<dyn Fn(usize, &mut EventContext)>> =
            if let Some(explicit) = self.on_close.clone() {
                Some(explicit)
            } else {
                self.source.remove_item_fn.clone().map(|remove| {
                    Rc::new(move |i: usize, _ctx: &mut EventContext| {
                        (remove)(i);
                    }) as Rc<dyn Fn(usize, &mut EventContext)>
                })
            };
        for i in 0..n {
            // Build the TabHeader for index i. The data-source
            // `with_item_fn` requires a `Fn(&T) -> Box<dyn Widget>`
            // closure; we use it as the bridge to construct a
            // `Box<TabHeader>` from the resolved delegate fields.
            let is_pinned = pinned_tabs[i];
            let selected = self.selected.clone();
            let shared_for_header = shared.clone();
            // Pinned tabs use the fixed pinned width; non-pinned use
            // the bar's `[min, max]` clamp.
            let (min_w, max_w) = if is_pinned {
                (self.pinned_tab_width, self.pinned_tab_width)
            } else {
                (self.min_tab_width, self.max_tab_width)
            };
            let label_capture: Rc<RefCell<Option<LocalizedString>>> = Rc::new(RefCell::new(None));
            let label_capture_clone = label_capture.clone();
            let close_handler_for_tab = close_handler.clone();
            let header = (self.source.with_item_fn)(i, &|item| -> Box<dyn Widget> {
                let label = self.delegate.resolve_label(i, item);
                // Capture the *original* title (pre display-mode transform) so
                // the overflow dropdown / a11y always read the real name even in
                // icon-only mode.
                *label_capture_clone.borrow_mut() = Some(label.clone());
                let icon = self.delegate.resolve_icon(i, item);
                let leading_slot = self.delegate.resolve_leading(i, item);
                let trailing_slot = self.delegate.resolve_trailing(i, item);
                let tooltip = self.delegate.resolve_tooltip(i, item);
                // Preserve the original title as the accessible name before the
                // display mode may blank the visible label (icon-only tabs).
                let at_name = label.clone();
                // Apply the bar-level display mode (icon / text / icon+text).
                let (label, icon, tooltip) =
                    apply_tab_display(self.tab_display, label, icon, tooltip);
                let rich_tooltip = self.delegate.resolve_rich_tooltip(i, item);
                let composite_tooltip = self.delegate.resolve_composite_tooltip(i, item);
                let context_menu_factory = self.delegate.resolve_context_menu(i, item);
                let enabled = self.delegate.resolve_enabled(i, item);
                let closable = self.delegate.resolve_closable(i, item);
                let on_close: Option<Rc<dyn Fn(&mut EventContext)>> = if closable {
                    close_handler_for_tab.clone().map(|f| {
                        Rc::new(move |ctx: &mut EventContext| (f)(i, ctx))
                            as Rc<dyn Fn(&mut EventContext)>
                    })
                } else {
                    None
                };

                let on_reorder_to: Option<Rc<dyn Fn(usize, &mut EventContext)>> = if !is_pinned {
                    reorder_handler.clone().map(|reorder| {
                        Rc::new(move |to: usize, ctx: &mut EventContext| (reorder)(i, to, ctx))
                            as Rc<dyn Fn(usize, &mut EventContext)>
                    })
                } else {
                    None
                };

                // A header is a drag source when reordering is on OR
                // cross-bar transfer is enabled. Build the payload
                // factory here (we have `&item` in scope): it carries
                // the source identity always, and a clone of the item
                // when transfer is enabled and the tab is transferable
                // (the `clone_item` closure encapsulates `T: Clone` so
                // `build()` need not be bounded on it).
                let is_drag_source = reorder_handler.is_some() || self.accept_external_tabs;
                let make_drag_payload: Option<Rc<dyn Fn() -> DragPayload>> = if is_drag_source {
                    let tab_id = (self.id_of)(i, item);
                    let transferable = self.transferable_fn.as_ref().is_none_or(|f| f(i, item));
                    let item_payload: Option<(T, Rc<dyn Fn(&T) -> T>)> = if transferable {
                        self.clone_item.as_ref().map(|cf| ((cf)(item), cf.clone()))
                    } else {
                        None
                    };
                    let src_index = i;
                    let bar_id = self_id;
                    Some(Rc::new(move || {
                        let item = item_payload.as_ref().map(|(it, cf)| (cf)(it));
                        DragPayload::typed(TabBarDragData {
                            source_index: src_index,
                            source_bar_id: bar_id,
                            source_id: tab_id,
                            item,
                        })
                    }) as Rc<dyn Fn() -> DragPayload>)
                } else {
                    None
                };

                // Source-side completion: when one of our tabs is
                // accepted by a *different* bar, fire on_transfer_out.
                // Suppressed for intra-bar reorders via the shared
                // self-reorder flag (set by our own on_drop, which
                // runs before on_drag_ended in the same dispatch).
                let on_drag_ended: Option<Rc<dyn Fn(DropOutcome, &mut EventContext)>> =
                    match (self.accept_external_tabs, self.on_transfer_out.clone()) {
                        (true, Some(transfer_out)) => {
                            let tab_id = (self.id_of)(i, item);
                            let self_reorder = self.self_reorder_flag.clone();
                            Some(
                                Rc::new(move |outcome: DropOutcome, ctx: &mut EventContext| {
                                    if matches!(outcome, DropOutcome::InApp { accepted: true })
                                        && !self_reorder.replace(false)
                                    {
                                        (transfer_out)(tab_id, ctx);
                                    }
                                })
                                    as Rc<dyn Fn(DropOutcome, &mut EventContext)>,
                            )
                        }
                        _ => None,
                    };

                Box::new(TabHeader::new(TabHeaderConfig {
                    label,
                    at_name,
                    icon,
                    leading_slot,
                    trailing_slot,
                    tooltip,
                    rich_tooltip,
                    composite_tooltip,
                    context_menu_factory,
                    // Pinned tabs suppress the close button —
                    // Firefox / Chrome convention. They're closed
                    // via the context menu only.
                    on_close: if is_pinned { None } else { on_close },
                    on_reorder_to,
                    make_drag_payload,
                    on_drag_ended,
                    index: i,
                    initial_enabled: enabled,
                    selected: selected.clone(),
                    shared: shared_for_header.clone(),
                    min_width: min_w,
                    max_width: max_w,
                    pinned: is_pinned,
                    orientation: self.orientation,
                    tab_surface_role: self.tab_surface_role.clone(),
                    selected_text_role: self.selected_text_role,
                    idle_text_role: self.idle_text_role,
                    style_override: self.style_override.clone(),
                }))
            });
            // Should never be `None` for `i < len()`, but defend:
            // skipping this index keeps the bar coherent if the source
            // mutated mid-build (e.g., another thread — though the
            // tree is single-threaded today).
            if let Some(header) = header {
                let id = ctx.add_boxed(header);
                if is_pinned {
                    pinned_header_ids.push(id);
                } else {
                    unpinned_header_ids.push(id);
                    unpinned_to_model.push(i);
                }
                header_ids_buf.borrow_mut().push(id);
                if let Some(lbl) = label_capture.borrow_mut().take() {
                    header_labels.push(lbl);
                } else {
                    header_labels.push(lit!(String::new()));
                }
            }
        }
        let unpinned_to_model = Rc::new(unpinned_to_model);
        let model_len = n;

        // ScrollArea wants a fixed `preferred_size.height` so the
        // viewport doesn't get squashed by the focus-ring envelope
        // headers reserve. Snapshot the theme values up front — we
        // don't want to hold a borrow on `ctx` while later code
        // mutates the arena.
        let (header_min_height, motion_duration_normal, motion_easing_standard) = {
            let theme = ctx.theme();
            // `editor_tab_height` is the outer bounds height of a
            // tab header — the focus-ring envelope is reserved
            // inside (see `TabHeader::intrinsic_height`), so the
            // bar's preferred row height is exactly the token (or the
            // `tab_bar_height` override for a compact strip).
            (
                self.tab_height
                    .unwrap_or(crate::styles::recipe_tab_style::TAB_EDITOR_HEIGHT),
                theme.motion.duration_normal,
                theme.motion.easing_standard,
            )
        };

        // Custom row widget: lays out the headers side-by-side with
        // shared-or-independent width semantics. The bounds buffers
        // are shared with the bar's drag-target handlers below so we
        // can map a drop-hover pointer position onto the right tab
        // boundary even when the row scrolls.
        let header_bounds_buf: Rc<RefCell<Vec<Rect>>> =
            Rc::new(RefCell::new(Vec::with_capacity(unpinned_header_ids.len())));
        let row_bounds_buf: Rc<std::cell::Cell<Rect>> =
            Rc::new(std::cell::Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
        let row = TabHeaderRow {
            header_ids: unpinned_header_ids.clone(),
            axis: self.orientation,
            sizing: self.sizing,
            min_extent: self.min_tab_width,
            max_extent: self.max_tab_width,
            spacing: self.spacing,
            tab_height: self.tab_height,
            header_bounds_buf: header_bounds_buf.clone(),
            row_bounds_buf: row_bounds_buf.clone(),
        };
        let row_id = ctx.add(row);
        self.header_row_id = Some(row_id);

        let scroll = match self.orientation {
            TabBarOrientation::Horizontal => ScrollArea::from_id(row_id)
                .scroll_bar_style(ScrollBarMode::Thin)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AsNeeded)
                .widget_resizable(true)
                .preferred_size(0.0, header_min_height),
            TabBarOrientation::Vertical => ScrollArea::from_id(row_id)
                .scroll_bar_style(ScrollBarMode::Overlay)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AsNeeded)
                .widget_resizable(true),
        };
        // Capture scroll signals BEFORE moving the ScrollArea into
        // the arena — drives arrow visibility and the wheel-mapping
        // handler. `scroll_x` / `max_scroll_x` for horizontal,
        // `scroll_y` / `max_scroll_y` for vertical.
        let scroll_x = scroll.scroll_x_signal().clone();
        let max_scroll_x = scroll.max_scroll_x_signal().clone();
        let scroll_y = scroll.scroll_y_signal().clone();
        let max_scroll_y = scroll.max_scroll_y_signal().clone();
        let scroll_id = ctx.add(scroll);

        // Wrap the scroll area in a stack so the bar slots have a
        // place to sit. The scroll area takes all the slack along
        // the layout axis. Outer container axis matches the bar's
        // orientation: HStack for horizontal, VStack for vertical
        // (slot → pinned strip → leading arrow → scroll area →
        // trailing arrow → dropdown → trailing slot).
        let scroll_main = match self.orientation {
            TabBarOrientation::Horizontal => scroll_x.clone(),
            TabBarOrientation::Vertical => scroll_y.clone(),
        };
        let max_scroll_main = match self.orientation {
            TabBarOrientation::Horizontal => max_scroll_x.clone(),
            TabBarOrientation::Vertical => max_scroll_y.clone(),
        };
        // Accumulate the outer-stack children into a Vec, then
        // construct the actual HStack / VStack at the end based on
        // orientation. Keeps the body axis-agnostic.
        let mut outer_children: Vec<WidgetId> = Vec::new();

        if let Some(slot) = self.bar_leading_slot.take() {
            let id = match slot {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            self.bar_leading_slot_id = Some(id);
            outer_children.push(id);
        }

        // Pinned strip — non-scrolling, fixed-width icons. Lives at
        // the leading edge so pinned tabs are always visible
        // regardless of how far the unpinned tabs scroll. Strip
        // orientation matches the bar.
        if !pinned_header_ids.is_empty() {
            let pinned_id = match self.orientation {
                TabBarOrientation::Horizontal => {
                    let mut pinned = HStack::new().spacing(self.spacing);
                    for id in &pinned_header_ids {
                        pinned = pinned.add_child(*id);
                    }
                    ctx.add(pinned)
                }
                TabBarOrientation::Vertical => {
                    let mut pinned = crate::VStack::new().spacing(self.spacing);
                    for id in &pinned_header_ids {
                        pinned = pinned.add_child(*id);
                    }
                    ctx.add(pinned)
                }
            };
            self.pinned_strip_id = Some(pinned_id);
            outer_children.push(pinned_id);
        }

        // Leading scroll arrow.
        if self.show_scroll_arrows {
            let arrow_id = build_scroll_arrow(
                ctx,
                ScrollArrowKind::Leading,
                self.orientation,
                scroll_main.clone(),
                max_scroll_main.clone(),
                motion_duration_normal,
                motion_easing_standard,
                self.idle_text_role,
            );
            // Visibility: only when there's something to scroll back.
            let visible = scroll_main.clone().map(|x| *x > 0.5);
            ctx.visible_when(arrow_id, visible);
            outer_children.push(arrow_id);
        }

        // The scroll area takes all the slack along the layout axis.
        let scroll_slot = match self.orientation {
            TabBarOrientation::Horizontal => ctx.add(Expand::horizontal().child_id(scroll_id)),
            TabBarOrientation::Vertical => ctx.add(Expand::vertical().child_id(scroll_id)),
        };
        outer_children.push(scroll_slot);

        // Trailing scroll arrow.
        if self.show_scroll_arrows {
            let arrow_id = build_scroll_arrow(
                ctx,
                ScrollArrowKind::Trailing,
                self.orientation,
                scroll_main.clone(),
                max_scroll_main.clone(),
                motion_duration_normal,
                motion_easing_standard,
                self.idle_text_role,
            );
            // Visibility: only when there's more to scroll forward.
            let visible = scroll_main
                .clone()
                .zip(&max_scroll_main)
                .map(|(x, max)| *x + 0.5 < *max);
            ctx.visible_when(arrow_id, visible);
            outer_children.push(arrow_id);
        }

        // Overflow dropdown — a chevron-down `PopoverIconButton` whose
        // popover content is a `ListView` mirroring the full tab
        // list. Activating an item sets `selected_id` and dismisses
        // the popover.
        if self.show_overflow_dropdown && !header_labels.is_empty() {
            // Build (id, label, enabled) entries so the dropdown can
            // route activation by stable TabId rather than by index.
            let entries: Vec<DropdownEntry> = header_labels
                .iter()
                .zip(index_to_id.iter().copied())
                .zip(enabled_tabs.iter().copied())
                .map(|((label, id), enabled)| DropdownEntry {
                    id,
                    label: label.clone(),
                    enabled,
                })
                .collect();
            let dropdown_id = build_overflow_dropdown(
                ctx,
                self.selected_id.clone(),
                entries,
                self.idle_text_role,
            );
            outer_children.push(dropdown_id);
        }

        if let Some(slot) = self.bar_trailing_slot.take() {
            let id = match slot {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            };
            self.bar_trailing_slot_id = Some(id);
            outer_children.push(id);
        }

        let root_id = match self.orientation {
            TabBarOrientation::Horizontal => {
                let mut row = HStack::new().spacing(DEFAULT_BAR_SLOT_SPACING);
                for id in &outer_children {
                    row = row.add_child(*id);
                }
                ctx.add(row)
            }
            TabBarOrientation::Vertical => {
                let mut col = crate::VStack::new().spacing(DEFAULT_BAR_SLOT_SPACING);
                for id in &outer_children {
                    col = col.add_child(*id);
                }
                ctx.add(col)
            }
        };
        // Resolve the active `TabStyle` and let it wrap the bar
        // content with the strip chrome — backdrop fill, content-pane
        // separator, drag-reorder drop indicator. Per-call override >
        // theme slot > built-in `RecipeTabStyle`. This replaces the
        // old `TabBar::paint`: the bar is now pure composition.
        let style: bastyde_core::styles::SharedTabStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.tab.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTabStyle));
        let chrome_cfg = bastyde_core::styles::TabBarChromeConfig {
            content: root_id,
            orientation: self.orientation.into(),
            show_separator: self.show_separator,
            surface_role: self.tab_surface_role.clone(),
            drop_indicator: self.paint_state.drop_indicator_x.clone(),
        };
        let bar_root = style.make_bar(&chrome_cfg, ctx);
        self.root_child_id = Some(bar_root);

        // Wheel-mapping handler. Attached via `on_pointer_event`
        // (not `on_scroll`) so the framework fires it in the
        // *preview pass* on each strict ancestor of the pointer
        // target — i.e. before the descendant ScrollArea has a
        // chance to consume the event. That's what lets us
        // remap "wheel down" → "scroll right" on a horizontal-only
        // bar; if we ran in bubble, ScrollArea would have already
        // handled the event and stopped propagation.
        //
        // We only consume events we're actively remapping; genuine
        // horizontal-wheel deltas (trackpad two-finger pan) pass
        // through to ScrollArea unchanged.
        let vert_to_horiz = self.vertical_wheel_scrolls_horizontally;
        let shift_to_horiz = self.shift_wheel_scrolls_horizontally;
        let scroll_x_for_wheel = scroll_x.clone();
        let max_scroll_x_for_wheel = max_scroll_x.clone();
        let orientation_for_wheel = self.orientation;
        let handler = HandlerSet::new().on_pointer_event(
            move |event: &WidgetEvent, _ctx: &mut EventContext| -> EventResponse {
                // Vertical bars scroll vertically — ScrollArea handles
                // wheel events natively; nothing to remap here.
                if orientation_for_wheel == TabBarOrientation::Vertical {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::Scroll { delta, modifiers } = event else {
                    return EventResponse::Ignored;
                };
                let (dx, dy) = match delta {
                    ScrollDelta::Lines { x, y } => (x * WHEEL_LINE_PIXELS, y * WHEEL_LINE_PIXELS),
                    ScrollDelta::Pixels { x, y } => (*x, *y),
                };
                let shift = modifiers.shift();
                // Decide whether *this* event is one we want to
                // remap. Shift always remaps; otherwise we only
                // remap a vertical-only wheel on a horizontal bar.
                let should_remap = if shift && shift_to_horiz {
                    true
                } else {
                    vert_to_horiz && dx.abs() < f32::EPSILON && dy.abs() > 0.0
                };
                if !should_remap {
                    return EventResponse::Ignored;
                }
                let mapped_dx = if dx.abs() > 0.0 { dx } else { dy };
                if mapped_dx.abs() < f32::EPSILON {
                    return EventResponse::Ignored;
                }
                // Sign convention matches ScrollArea: positive delta
                // moves the content (so positive `y` from a wheel-down
                // event scrolls right when remapped to horizontal).
                let new_x =
                    (scroll_x_for_wheel.get() + mapped_dx).clamp(0.0, max_scroll_x_for_wheel.get());
                scroll_x_for_wheel.set(new_x);
                EventResponse::Handled
            },
        );
        ctx.apply_self_handlers(handler);

        // Drag-target handlers: attached when reordering is on OR the
        // bar accepts cross-bar transfers. The bar acts as the single
        // drop target — we convert pointer position (delivered in
        // bar-local coords) into a tab boundary by walking
        // `header_bounds_buf` (world coords) translated into bar-local
        // space via `last_bar_bounds.origin` cached by `place_children`.
        //
        // Two payload consumers share this target:
        //   - intra-bar reorder: `data.source_bar_id == self_id` →
        //     `reorder(from, to)` (only when `reorder` is set).
        //   - cross-bar transfer: a foreign bar's payload carrying
        //     `item: Some(_)` → `on_tab_received(item, to_model)`
        //     (only when `accept_external` is on).
        if reorder_handler.is_some() || self.accept_external_tabs || self.on_external_drop.is_some()
        {
            let bar_id_for_drop = self_id;
            let axis = self.orientation;
            let accept_external = self.accept_external_tabs;
            let has_external_drop = self.on_external_drop.is_some();
            // Insertion line cross-extent used when the target bar has
            // no unpinned headers yet (empty bar): span the bar's
            // cross axis so the indicator is still visible.
            let drop_handler = HandlerSet::new()
                .on_drag_hover({
                    let header_bounds = header_bounds_buf.clone();
                    let drop_indicator = self.paint_state.drop_indicator_x.clone();
                    let bar_bounds = self.paint_state.last_bar_bounds.clone();
                    move |payload: &DragPayload,
                          position: Point,
                          _ctx: &mut EventContext|
                          -> DropFeedback {
                        match payload.get_typed::<TabBarDragData<T>>() {
                            Some(data) => {
                                // Accept an intra-bar reorder, or a
                                // foreign tab when this bar opted into
                                // transfer and the payload carries a
                                // transferable item.
                                let is_intra = data.source_bar_id == bar_id_for_drop;
                                let is_foreign_ok = accept_external && data.item.is_some();
                                if !is_intra && !is_foreign_ok {
                                    drop_indicator.set(None);
                                    return DropFeedback::NoFeedback;
                                }
                            }
                            None => {
                                // Non-tab payload (foreign in-app drag
                                // or OS drop): accepted only if a
                                // non-tab drop handler is installed.
                                // The indicator is optimistic — the
                                // handler decides for real at drop.
                                if !has_external_drop {
                                    drop_indicator.set(None);
                                    return DropFeedback::NoFeedback;
                                }
                            }
                        }
                        let bar = bar_bounds.get();
                        let bounds = header_bounds.borrow();
                        // Empty target bar (no unpinned headers): drop
                        // at the leading edge, indicator spans the
                        // bar's cross axis.
                        if bounds.is_empty() {
                            let cross = match axis {
                                TabBarOrientation::Horizontal => bar.height,
                                TabBarOrientation::Vertical => bar.width,
                            };
                            drop_indicator.set(Some(0.0));
                            return DropFeedback::InsertionLine {
                                y: 0.0,
                                width: cross,
                            };
                        }
                        // Layout-axis pointer position in world coords:
                        // x for horizontal bars, y for vertical bars.
                        let (pointer_world_main, bar_origin_main) = match axis {
                            TabBarOrientation::Horizontal => (position.x + bar.x, bar.x),
                            TabBarOrientation::Vertical => (position.y + bar.y, bar.y),
                        };
                        let insertion_world_main =
                            insertion_world_main_for(&bounds, pointer_world_main, axis);
                        let insertion_local_main = insertion_world_main - bar_origin_main;
                        drop_indicator.set(Some(insertion_local_main));
                        DropFeedback::InsertionLine {
                            y: 0.0,
                            width: bounds[0].height,
                        }
                    }
                })
                .on_drag_leave({
                    let drop_indicator = self.paint_state.drop_indicator_x.clone();
                    move |_ctx: &mut EventContext| {
                        drop_indicator.set(None);
                    }
                })
                .on_drop({
                    let header_bounds = header_bounds_buf.clone();
                    let bar_bounds = self.paint_state.last_bar_bounds.clone();
                    let drop_indicator = self.paint_state.drop_indicator_x.clone();
                    let reorder = reorder_handler.clone();
                    let on_received = self.on_tab_received.clone();
                    let on_external_drop = self.on_external_drop.clone();
                    let self_reorder = self.self_reorder_flag.clone();
                    let unpinned_to_model = unpinned_to_model.clone();
                    let bar_id = bar_id_for_drop;
                    move |mut payload: DragPayload,
                          position: Point,
                          ctx: &mut EventContext|
                          -> bool {
                        drop_indicator.set(None);
                        // Extract the tab payload if this is one; a
                        // failed downcast leaves `payload` intact for
                        // the non-tab branch below.
                        let mut data = payload.take_typed::<TabBarDragData<T>>();
                        let bar = bar_bounds.get();
                        let bounds = header_bounds.borrow();
                        // Resolve the model insertion index from the
                        // pointer. `insertion_index_for` works in
                        // **unpinned** space (the bounds buffer only
                        // holds unpinned headers); map it to a model
                        // index. An empty target bar inserts at 0.
                        let to_model = if bounds.is_empty() {
                            0
                        } else {
                            let pointer_world_main = match axis {
                                TabBarOrientation::Horizontal => position.x + bar.x,
                                TabBarOrientation::Vertical => position.y + bar.y,
                            };
                            let to_unpinned =
                                insertion_index_for(&bounds, pointer_world_main, axis);
                            if to_unpinned < unpinned_to_model.len() {
                                unpinned_to_model[to_unpinned]
                            } else {
                                // Past the trailing edge of the
                                // unpinned region — insert just after
                                // the last unpinned tab.
                                unpinned_to_model
                                    .last()
                                    .map(|&last| last + 1)
                                    .unwrap_or(model_len)
                            }
                        };

                        let Some(data) = data.as_mut() else {
                            // ── Non-tab payload (foreign drag / OS) ─
                            // `payload` is intact (downcast missed).
                            drop(bounds);
                            return match on_external_drop.as_ref() {
                                Some(cb) => (cb)(&payload, to_model, ctx),
                                None => false,
                            };
                        };

                        if data.source_bar_id == bar_id {
                            // ── Intra-bar reorder ──────────────────
                            // Mark the drag as a self-reorder so the
                            // source header's on_drag_ended suppresses
                            // on_transfer_out (which would otherwise
                            // remove the just-reordered tab).
                            self_reorder.set(true);
                            let Some(reorder) = reorder.as_ref() else {
                                return true;
                            };
                            let from = data.source_index;
                            // `move_item(from, to)` interprets `to` as
                            // the **post-removal** insertion position,
                            // so a forward drag adjusts by -1.
                            let adjusted_to = if from < to_model {
                                to_model.saturating_sub(1)
                            } else {
                                to_model
                            };
                            if from != adjusted_to {
                                (reorder)(from, adjusted_to, ctx);
                            }
                            true
                        } else if accept_external {
                            // ── Cross-bar transfer ─────────────────
                            // No `-1` correction: there is no source
                            // slot inside *this* model to compensate
                            // for. The app inserts the moved item at
                            // exactly `to_model`.
                            let Some(item) = data.item.take() else {
                                return false;
                            };
                            if let Some(cb) = on_received.as_ref() {
                                (cb)(item, to_model, ctx);
                            }
                            true
                        } else {
                            false
                        }
                    }
                })
                .on_drag_tick({
                    // Edge auto-scroll while a drag is in progress.
                    // Ramp the scroll velocity linearly inside the
                    // edge zones; cap at `DRAG_MAX_VELOCITY` so fast
                    // drags don't rocket past the content. Axis-aware:
                    // horizontal bars scroll by x, vertical by y.
                    let scroll_main = scroll_main.clone();
                    let max_scroll_main = max_scroll_main.clone();
                    let bar_bounds = self.paint_state.last_bar_bounds.clone();
                    move |position: Point, _ctx: &mut EventContext| {
                        let bar = bar_bounds.get();
                        let (pointer_main, bar_extent) = match axis {
                            TabBarOrientation::Horizontal => (position.x, bar.width),
                            TabBarOrientation::Vertical => (position.y, bar.height),
                        };
                        let max = max_scroll_main.get();
                        let cur = scroll_main.get();
                        let leading_in = (DRAG_EDGE_ZONE - pointer_main).max(0.0);
                        let trailing_in = (pointer_main - (bar_extent - DRAG_EDGE_ZONE)).max(0.0);
                        let delta = if leading_in > 0.0 {
                            -(leading_in / DRAG_EDGE_ZONE) * DRAG_MAX_VELOCITY
                        } else if trailing_in > 0.0 {
                            (trailing_in / DRAG_EDGE_ZONE) * DRAG_MAX_VELOCITY
                        } else {
                            0.0
                        };
                        if delta.abs() > 0.001 {
                            scroll_main.set((cur + delta).clamp(0.0, max));
                        }
                    }
                });
            ctx.apply_self_handlers(drop_handler);
        }

        vec![bar_root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let Some(root_id) = self.root_child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };
        let final_proposal = match self.orientation {
            TabBarOrientation::Vertical => {
                // Adapt the bar's cross-axis (width) to whichever piece
                // of bar content is widest — tab labels, the pinned
                // strip, or a leading / trailing slot widget — clamped
                // to [min_tab_width, max_tab_width]. Probing the inner
                // ScrollArea would just echo our own proposal back, so
                // we measure the row directly.
                let mut intrinsic_w = 0.0_f32;
                for opt in [
                    self.header_row_id,
                    self.pinned_strip_id,
                    self.bar_leading_slot_id,
                    self.bar_trailing_slot_id,
                ] {
                    if let Some(id) = opt
                        && let Some(s) = ctx.child_size(id, SizeProposal::unspecified())
                    {
                        intrinsic_w = intrinsic_w.max(s.width);
                    }
                }
                let mut target = intrinsic_w.clamp(self.min_tab_width, self.max_tab_width);
                if let Some(p) = proposal.width {
                    target = target.min(p).max(self.min_tab_width);
                }
                SizeProposal {
                    width: Some(target),
                    height: proposal.height,
                }
            }
            TabBarOrientation::Horizontal => proposal,
        };
        ctx.child_size(root_id, final_proposal)
            .unwrap_or_else(|| final_proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Record the bar's world bounds so the drag handlers can
        // translate bar-local pointer positions back to world coords
        // (matching the world-coords header bounds populated by
        // TabHeaderRow).
        self.paint_state.last_bar_bounds.set(bounds);
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    // No `paint()`: the bar is pure composition. Backdrop fill,
    // content-pane separator, and the drag-reorder drop indicator are
    // all drawn by the active `TabStyle`'s `make_bar` chrome (see
    // `RecipeTabStyle` / `TabBarChromePainter`).

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::TabList);
        builder.set_orientation(match self.orientation {
            TabBarOrientation::Horizontal => bastyde_core::accesskit::Orientation::Horizontal,
            TabBarOrientation::Vertical => bastyde_core::accesskit::Orientation::Vertical,
        });
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ─── Internal: the headers row / column ─────────────────────────────

/// The headers run — a horizontal row in [`TabBarOrientation::Horizontal`]
/// mode, a vertical column in [`TabBarOrientation::Vertical`] mode.
/// Owns the `Shared`/`Independent` sizing math and exposes per-tab
/// world bounds back to the bar's DnD handlers.
#[derive(Debug)]
struct TabHeaderRow {
    header_ids: Vec<WidgetId>,
    axis: TabBarOrientation,
    sizing: TabSizing,
    /// Min extent on the *layout axis* — width for horizontal,
    /// height for vertical. Reuses the same `min_tab_width` knob for
    /// the vertical case (it's about per-tab pill extent, not the
    /// width of the bar).
    min_extent: f32,
    max_extent: f32,
    spacing: f32,
    /// Optional per-tab extent override along the bar's cross axis (the tab
    /// strip height for a horizontal bar; the per-tab pill height for a
    /// vertical one). `None` → the style's `editor_tab_height`.
    tab_height: Option<f32>,
    /// Per-tab bounds in world coords, populated by `place_children`.
    /// Shared with the bar's drop handlers via `Rc<RefCell<...>>`;
    /// the bar reads this to compute drop-insertion position for an
    /// in-progress drag.
    header_bounds_buf: Rc<RefCell<Vec<Rect>>>,
    /// Cached row-level world bounds — used to map bar-local
    /// coordinates onto header bounds.
    row_bounds_buf: Rc<std::cell::Cell<Rect>>,
}

impl TabHeaderRow {
    /// The per-tab cross-axis extent: the explicit override (compact bars) or
    /// the style's `editor_tab_height`.
    fn tab_extent(&self, ctx: &LayoutContext) -> f32 {
        self.tab_height
            .unwrap_or_else(|| TabHeader::intrinsic_height(ctx))
    }

    fn compute_extents(&self, viewport_main: Option<f32>, ctx: &LayoutContext) -> Vec<f32> {
        let n = self.header_ids.len();
        if n == 0 {
            return Vec::new();
        }
        match self.sizing {
            TabSizing::Shared => {
                let target = match self.axis {
                    TabBarOrientation::Horizontal => {
                        // Divide the viewport width across tabs
                        // (Firefox / Chrome convention) and clamp by
                        // the layout-axis [min, max] knobs.
                        let total_spacing = self.spacing * (n.saturating_sub(1)) as f32;
                        let avail = viewport_main.unwrap_or(0.0).max(0.0);
                        let ideal = ((avail - total_spacing).max(0.0) / n as f32).max(0.0);
                        ideal.clamp(self.min_extent, self.max_extent)
                    }
                    TabBarOrientation::Vertical => {
                        // Vertical sidebar pills are NOT viewport-
                        // divided — that turns a tall bar into ~200 dp
                        // tab bands, which neither Firefox / Chrome
                        // (no native vertical mode) nor VS Code /
                        // IntelliJ do. Use the intrinsic per-tab
                        // height (`editor_tab_height`) so vertical
                        // tabs match horizontal tabs in size.
                        self.tab_extent(ctx)
                    }
                };
                vec![target; n]
            }
            TabSizing::Independent => self
                .header_ids
                .iter()
                .map(|&id| {
                    let s = ctx.child_size(id, SizeProposal::unspecified());
                    let raw = match self.axis {
                        TabBarOrientation::Horizontal => s.map(|s| s.width),
                        TabBarOrientation::Vertical => s.map(|s| s.height),
                    };
                    let fallback = match self.axis {
                        TabBarOrientation::Horizontal => self.min_extent,
                        TabBarOrientation::Vertical => self.tab_extent(ctx),
                    };
                    let raw = raw.unwrap_or(fallback);
                    // [min, max] are width-defaulted (96 / 240) and
                    // axis-mismatched in vertical mode where they'd
                    // force tab heights to ≥96 dp. Skip the clamp on
                    // the height axis; the intrinsic per-tab height
                    // is already the right answer.
                    match self.axis {
                        TabBarOrientation::Horizontal => {
                            raw.clamp(self.min_extent, self.max_extent)
                        }
                        TabBarOrientation::Vertical => raw,
                    }
                })
                .collect(),
        }
    }
}

impl Widget for TabHeaderRow {
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Children are pre-registered with the bar's BuildContext; the
        // row just exposes them.
        self.header_ids.clone()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let n = self.header_ids.len();
        if n == 0 {
            return Size::new(0.0, 0.0).into();
        }
        let total_spacing = self.spacing * (n - 1) as f32;
        match self.axis {
            TabBarOrientation::Horizontal => {
                // Cap the row's height at one tab header's intrinsic
                // height (= `editor_tab_height`). If the surrounding
                // outer HStack proposes a taller height because a
                // sibling (toolbar button, dropdown trigger) wants
                // more room, the row should NOT stretch — it would
                // turn the strip into a tall band with the pills
                // floating in the middle. Clamping here keeps the
                // tab strip exactly token-sized.
                let intrinsic = self.tab_extent(ctx);
                let height = proposal
                    .height
                    .map(|h| h.min(intrinsic))
                    .unwrap_or(intrinsic);
                let extents = self.compute_extents(proposal.width, ctx);
                let total = extents.iter().sum::<f32>() + total_spacing;
                Size::new(total, height).into()
            }
            TabBarOrientation::Vertical => {
                // Adapt to the longest tab label, clamped to
                // [min_extent, max_extent]. Without this, the row
                // would echo `proposal.width` and let the bar swallow
                // whatever cross-axis space the parent gave it.
                let intrinsic = self
                    .header_ids
                    .iter()
                    .filter_map(|&id| ctx.child_size(id, SizeProposal::unspecified()))
                    .map(|s| s.width)
                    .fold(0.0_f32, f32::max);
                let mut width = intrinsic.clamp(self.min_extent, self.max_extent);
                if let Some(proposed) = proposal.width {
                    width = width.min(proposed).max(self.min_extent);
                }
                let extents = self.compute_extents(proposal.height, ctx);
                let total = extents.iter().sum::<f32>() + total_spacing;
                Size::new(width, total).into()
            }
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // For Shared sizing, divide the *viewport* main extent (the
        // proposal main axis) — NOT the bounds main extent, which is
        // the content size returned by `layout_response`. ScrollArea
        // computes content size from `layout_response` and then calls
        // `place_children` with bounds = content_size, so using the
        // bounds main here would feedback-loop the layout pass.
        let viewport_main = match self.axis {
            TabBarOrientation::Horizontal => proposal.width,
            TabBarOrientation::Vertical => proposal.height,
        };
        let extents = self.compute_extents(viewport_main, ctx);
        let mut buf = self.header_bounds_buf.borrow_mut();
        buf.clear();
        match self.axis {
            TabBarOrientation::Horizontal => {
                let mut x = bounds.x;
                for (i, child) in children.iter_mut().enumerate() {
                    if i >= extents.len() {
                        break;
                    }
                    child.origin = Point::new(x, bounds.y);
                    child.size = Size::new(extents[i], bounds.height);
                    buf.push(Rect::new(x, bounds.y, extents[i], bounds.height));
                    x += extents[i] + self.spacing;
                }
            }
            TabBarOrientation::Vertical => {
                let mut y = bounds.y;
                for (i, child) in children.iter_mut().enumerate() {
                    if i >= extents.len() {
                        break;
                    }
                    child.origin = Point::new(bounds.x, y);
                    child.size = Size::new(bounds.width, extents[i]);
                    buf.push(Rect::new(bounds.x, y, bounds.width, extents[i]));
                    y += extents[i] + self.spacing;
                }
            }
        }
        self.row_bounds_buf.set(bounds);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.header_ids.clone()
    }
}

// ─── Scroll arrow + overflow dropdown construction ───────────────────

/// Apply a bar-level [`TabDisplayMode`] to one tab's resolved label / icon /
/// tooltip. Icon-only modes blank the displayed label (the header then sizes to
/// the icon) and promote the title to the hover tooltip; with no icon they fall
/// back to the title's initial letter so the tab is never blank.
fn apply_tab_display(
    mode: TabDisplayMode,
    label: LocalizedString,
    icon: Option<IconWidget>,
    tooltip: Option<LocalizedString>,
) -> (LocalizedString, Option<IconWidget>, Option<LocalizedString>) {
    match mode {
        // Render as declared (Auto) or both when available (IconText) — there
        // is nothing to force-add, so these are identical transforms.
        TabDisplayMode::Auto | TabDisplayMode::IconText => (label, icon, tooltip),
        // Title only — drop the icon.
        TabDisplayMode::Text => (label, None, tooltip),
        // Icon only — blank the displayed label, promote the title to the
        // tooltip, and fall back to the initial letter when there is no icon.
        TabDisplayMode::Icon => {
            let resolved = label.clone().resolve_now();
            let tip =
                tooltip.or_else(|| (!resolved.trim().is_empty()).then(|| label.clone()));
            if icon.is_some() {
                (lit!(""), icon, tip)
            } else {
                let initial: String = resolved.chars().take(1).collect();
                (lit!(initial), None, tip)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ScrollArrowKind {
    Leading,
    Trailing,
}

fn build_scroll_arrow(
    ctx: &mut BuildContext,
    kind: ScrollArrowKind,
    orientation: TabBarOrientation,
    scroll_main: Signal<f32>,
    max_scroll_main: Signal<f32>,
    duration: std::time::Duration,
    easing: Easing,
    icon_role: TextRole,
) -> WidgetId {
    let _ = ctx;
    let icon_size = crate::styles::recipe_button_style::BUTTON_ICON_SIZE;
    let icon = match (orientation, kind) {
        (TabBarOrientation::Horizontal, ScrollArrowKind::Leading) => {
            IconWidget::chevron_left(icon_size)
        }
        (TabBarOrientation::Horizontal, ScrollArrowKind::Trailing) => {
            IconWidget::chevron_right(icon_size)
        }
        (TabBarOrientation::Vertical, ScrollArrowKind::Leading) => {
            IconWidget::chevron_up(icon_size)
        }
        (TabBarOrientation::Vertical, ScrollArrowKind::Trailing) => {
            IconWidget::chevron_down(icon_size)
        }
    };
    let tooltip = match (orientation, kind) {
        (TabBarOrientation::Horizontal, ScrollArrowKind::Leading) => {
            lit!("Scroll tabs left")
        }
        (TabBarOrientation::Horizontal, ScrollArrowKind::Trailing) => {
            lit!("Scroll tabs right")
        }
        (TabBarOrientation::Vertical, ScrollArrowKind::Leading) => {
            lit!("Scroll tabs up")
        }
        (TabBarOrientation::Vertical, ScrollArrowKind::Trailing) => {
            lit!("Scroll tabs down")
        }
    };
    let button = IconButton::new(icon)
        .embedded()
        .size(IconButtonSize::Compact)
        .icon_role(icon_role)
        .tooltip(tooltip)
        .on_activate_fn(move |_ctx| {
            let cur = scroll_main.get();
            let target = match kind {
                ScrollArrowKind::Leading => (cur - SCROLL_ARROW_STEP).max(0.0),
                ScrollArrowKind::Trailing => (cur + SCROLL_ARROW_STEP).min(max_scroll_main.get()),
            };
            // The main-axis scroll signal is created via
            // `Signal::new_animated` inside ScrollArea, so
            // `animate_to` is supported.
            scroll_main.animate_to(target, duration, easing);
        });
    ctx.add(button)
}

/// One entry in the overflow dropdown — a stable [`TabId`], the
/// resolved label, and whether the tab is enabled. Built fresh per
/// bar build pass; cloned into the `ListView`'s underlying
/// `ListModel`.
#[derive(Clone)]
struct DropdownEntry {
    id: TabId,
    label: LocalizedString,
    enabled: bool,
}

impl std::fmt::Debug for DropdownEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropdownEntry")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Width of the overflow popover. Roughly two tab-widths so the
/// labels read at the same density as the bar itself.
const DROPDOWN_WIDTH: f32 = 240.0;
/// Cap on the popover height — beyond this many items the ListView
/// scrolls internally. Roughly ten rows of `DROPDOWN_ROW_HEIGHT`.
const DROPDOWN_MAX_HEIGHT: f32 = 320.0;
/// Per-row height. Smaller than a tab header so the dropdown reads
/// as a menu rather than a strip preview.
const DROPDOWN_ROW_HEIGHT: f32 = 28.0;
/// Padding inside the dropdown surface.
const DROPDOWN_PADDING: f32 = 4.0;

fn build_overflow_dropdown(
    ctx: &mut BuildContext,
    selected_id: Signal<Option<TabId>>,
    entries: Vec<DropdownEntry>,
    icon_role: TextRole,
) -> WidgetId {
    let _ = ctx;
    let icon_size = crate::styles::recipe_button_style::BUTTON_ICON_SIZE;
    // Same square, icon-sized control as the scroll arrows (an `IconButton`, not
    // a label-less `Button` that pads out around the glyph) so it stays adapted
    // to its icon and consistent in both bar orientations.
    let trigger = IconButton::new(IconWidget::chevron_down(icon_size))
        .embedded()
        .size(IconButtonSize::Compact)
        .icon_role(icon_role)
        .tooltip(lit!("Show all tabs"));

    // Cap each row at the dropdown height so a click still hits a
    // sensible-sized button regardless of `entries.len()`.
    let row_count = entries.len();
    let model = ListModel::from_vec(entries);
    let selected_for_delegate = selected_id.clone();
    let list = ListView::new(model, move |_i, entry: &DropdownEntry, _selected| {
        let entry_id = entry.id;
        let label = entry.label.clone();
        let enabled = entry.enabled;
        let sel = selected_for_delegate.clone();
        Box::new(
            Button::new(label)
                .variant(ButtonVariant::Ghost)
                .enabled(enabled)
                .on_activate_fn(move |ctx: &mut EventContext| {
                    sel.set(Some(entry_id));
                    ctx.dismiss_self_overlay_chain();
                }),
        ) as Box<dyn Widget>
    })
    .item_height(DROPDOWN_ROW_HEIGHT);

    // Compute a shrink-to-content height for short tab lists; cap
    // at `DROPDOWN_MAX_HEIGHT` for long ones (the ListView's
    // internal scroll bar takes over past the cap).
    let natural_h = (row_count as f32 * DROPDOWN_ROW_HEIGHT) + (DROPDOWN_PADDING * 2.0);
    let content_h = natural_h.min(DROPDOWN_MAX_HEIGHT);

    // Sized container. `FixedSize` forces both axes (content_h
    // shrinks on a short list; the constant width keeps the popover
    // from stretching to fit a long label).
    let sized = FixedSize::new()
        .bind_width(DROPDOWN_WIDTH - DROPDOWN_PADDING * 2.0)
        .bind_height(content_h - DROPDOWN_PADDING * 2.0)
        .child(list);

    // Raised surface — `SurfaceRole::Raised` is the popup-fill
    // token; the `BorderRole::Default` 1 dp border gives the
    // popover a clean edge over arbitrary backgrounds.
    let surface = Panel::new()
        .background(SurfaceRole::Raised)
        .border_color(BorderRole::Default)
        .border_width(1.0)
        .padding(DROPDOWN_PADDING)
        .child(sized);

    ctx.add(
        PopoverIconButton::new(trigger)
            // `surface` is already a chromed `Panel` (Raised) — opt out
            // of the auto popover surface to avoid double-chroming.
            .content(surface)
            .bare()
            .placement(OverlayPlacement::BelowPreferred)
            .has_popup_kind(HasPopup::Menu),
    )
}

// ─── Helper math: drop-insertion index + selection adjust ───────────

/// Pull the layout-axis range `(start, end)` out of a header's world
/// bounds. Horizontal bars use `(x, right)`; vertical bars use
/// `(y, bottom)`.
fn axis_range(rect: &Rect, axis: TabBarOrientation) -> (f32, f32) {
    match axis {
        TabBarOrientation::Horizontal => (rect.x, rect.right()),
        TabBarOrientation::Vertical => (rect.y, rect.bottom()),
    }
}

/// Find the world-coord (along the layout axis) of the insertion-line
/// position closest to `pointer_main`, given each header's world
/// bounds. The returned coordinate is a tab boundary — the leading
/// edge of a header, or the trailing edge of the last header.
fn insertion_world_main_for(bounds: &[Rect], pointer_main: f32, axis: TabBarOrientation) -> f32 {
    let n = bounds.len();
    debug_assert!(n > 0);
    let (_, last_end) = axis_range(&bounds[n - 1], axis);
    if pointer_main >= last_end {
        return last_end;
    }
    let (first_start, _) = axis_range(&bounds[0], axis);
    if pointer_main <= first_start {
        return first_start;
    }
    for header in bounds {
        let (start, end) = axis_range(header, axis);
        let mid = (start + end) * 0.5;
        if pointer_main < mid {
            return start;
        }
    }
    last_end
}

/// Find the model index where the dragged tab should be inserted.
/// `n` items → `n+1` valid insertion indices: 0 means "before the
/// first", `n` means "after the last".
fn insertion_index_for(bounds: &[Rect], pointer_main: f32, axis: TabBarOrientation) -> usize {
    let n = bounds.len();
    if n == 0 {
        return 0;
    }
    let (_, last_end) = axis_range(&bounds[n - 1], axis);
    if pointer_main >= last_end {
        return n;
    }
    let (first_start, _) = axis_range(&bounds[0], axis);
    if pointer_main <= first_start {
        return 0;
    }
    for (i, header) in bounds.iter().enumerate() {
        let (start, end) = axis_range(header, axis);
        let mid = (start + end) * 0.5;
        if pointer_main < mid {
            return i;
        }
    }
    n
}

// Selection adjustment after move/remove is unnecessary now: the
// public selection signal is `Signal<Option<TabId>>`, which is
// stable across reorders by definition (the moved tab keeps its
// id) and across removals it goes stale and the bar's pre-build
// sync routes the id-not-found case to the next-neighbor fallback
// (browser convention).

#[cfg(test)]
mod drop_math_tests {
    use super::*;

    fn three_tabs() -> Vec<Rect> {
        vec![
            Rect::new(0.0, 0.0, 100.0, 30.0),   // x ∈ [0..100)
            Rect::new(100.0, 0.0, 100.0, 30.0), // x ∈ [100..200)
            Rect::new(200.0, 0.0, 100.0, 30.0), // x ∈ [200..300)
        ]
    }

    fn three_tabs_vertical() -> Vec<Rect> {
        vec![
            Rect::new(0.0, 0.0, 200.0, 50.0),   // y ∈ [0..50)
            Rect::new(0.0, 50.0, 200.0, 50.0),  // y ∈ [50..100)
            Rect::new(0.0, 100.0, 200.0, 50.0), // y ∈ [100..150)
        ]
    }

    #[test]
    fn pointer_before_first_tab_inserts_at_zero() {
        let bounds = three_tabs();
        let axis = TabBarOrientation::Horizontal;
        assert_eq!(insertion_index_for(&bounds, -10.0, axis), 0);
        assert_eq!(insertion_world_main_for(&bounds, -10.0, axis), 0.0);
    }

    #[test]
    fn pointer_past_last_tab_appends() {
        let bounds = three_tabs();
        let axis = TabBarOrientation::Horizontal;
        assert_eq!(insertion_index_for(&bounds, 999.0, axis), 3);
        assert_eq!(insertion_world_main_for(&bounds, 999.0, axis), 300.0);
    }

    #[test]
    fn pointer_in_left_half_of_a_tab_inserts_before_it() {
        let bounds = three_tabs();
        let axis = TabBarOrientation::Horizontal;
        // Tab 1 spans 100..200; pointer at x=120 is in its left half.
        assert_eq!(insertion_index_for(&bounds, 120.0, axis), 1);
        assert_eq!(insertion_world_main_for(&bounds, 120.0, axis), 100.0);
    }

    #[test]
    fn pointer_in_right_half_of_a_tab_inserts_after_it() {
        let bounds = three_tabs();
        let axis = TabBarOrientation::Horizontal;
        // Tab 1's right half is 150..200 → insertion at index 2.
        assert_eq!(insertion_index_for(&bounds, 175.0, axis), 2);
        assert_eq!(insertion_world_main_for(&bounds, 175.0, axis), 200.0);
    }

    #[test]
    fn vertical_pointer_above_first_tab_inserts_at_zero() {
        let bounds = three_tabs_vertical();
        let axis = TabBarOrientation::Vertical;
        assert_eq!(insertion_index_for(&bounds, -10.0, axis), 0);
        assert_eq!(insertion_world_main_for(&bounds, -10.0, axis), 0.0);
    }

    #[test]
    fn vertical_pointer_past_last_tab_appends() {
        let bounds = three_tabs_vertical();
        let axis = TabBarOrientation::Vertical;
        assert_eq!(insertion_index_for(&bounds, 999.0, axis), 3);
        assert_eq!(insertion_world_main_for(&bounds, 999.0, axis), 150.0);
    }

    #[test]
    fn vertical_pointer_in_top_half_of_a_tab_inserts_before_it() {
        let bounds = three_tabs_vertical();
        let axis = TabBarOrientation::Vertical;
        // Tab 1 spans y=50..100; pointer at y=60 is in its top half.
        assert_eq!(insertion_index_for(&bounds, 60.0, axis), 1);
        assert_eq!(insertion_world_main_for(&bounds, 60.0, axis), 50.0);
    }

    #[test]
    fn vertical_pointer_in_bottom_half_of_a_tab_inserts_after_it() {
        let bounds = three_tabs_vertical();
        let axis = TabBarOrientation::Vertical;
        // Tab 1's bottom half is y=75..100 → insertion at index 2.
        assert_eq!(insertion_index_for(&bounds, 88.0, axis), 2);
        assert_eq!(insertion_world_main_for(&bounds, 88.0, axis), 100.0);
    }
}

// ─── Helper: a 0×0 widget used as a throwaway return value when we
// only need the side-effect of `ListSource::with_item_fn` (its
// closure access to `&T`), not an actual widget. The probe is
// constructed, returned to `with_item_fn`, and dropped immediately.
// ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct EnabledProbe;

impl Widget for EnabledProbe {
    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(0.0, 0.0).into()
    }
}
