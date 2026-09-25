// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MenuList — a themed vertical menu container with keyboard navigation.
//!
//! `MenuList` is the dropdown panel used by `MenuBar`, `MenuContext`, and
//! popover-style menus. It provides a themed surface (background, rounded
//! border, drop shadow) and owns the full keyboard navigation stack:
//! ArrowUp/Down moves focus, Enter activates, Escape bubbles to the
//! enclosing overlay host, Home and End jump to the first/last visible item.
//! Type-ahead search jumps to the next item whose stripped label starts with
//! the accumulated keystrokes (500 ms reset window by default).
//!
//! Items are added with `.item(widget)` (any `impl Widget`, but typically a
//! `MenuItem`); separators with `.separator()`. Conditional rows use
//! `.item_when(widget, visible_prop)` — a hidden row collapses to zero height
//! and is skipped by keyboard navigation. For very long lists (recent files,
//! etc.) call `.max_visible_items(n)` to cap the panel height and wrap the
//! content in a `ScrollArea`.
//!
//! **Safe-triangle hover gate.** When the pointer leaves a submenu trigger's
//! row with the submenu still up, the trigger arms the safe region (the apex
//! and the cone live in `teksilo_core::overlay`) and publishes that submenu's
//! id on a `MenuList`-wide shared state, so sibling items can skip their
//! hover-switch while the cursor travels diagonally toward the submenu.
//!
//! ## Accessibility
//!
//! `Role::Menu`, named after what opened it (a `MenuBar` trigger, the
//! `MenuItem::submenu` row, a `PopoverButton`'s button), with the keyboard
//! highlight as its active descendant: the menu keeps keyboard focus, and a
//! screen reader is told of each move of the highlight as a focus change to
//! the row. Each row is `Role::MenuItem` / `Role::MenuItemCheckBox` /
//! `Role::MenuItemRadio` as declared by the item, and carries its position
//! among the menu's shown rows, whose count the menu carries. Radio items in
//! the same list also auto-group via `push_to_radio_group`.
//!
//! ```rust
//! # use teksilo_widgets::{MenuList, MenuItem};
//! # use teksilo_i18n::lit;
//! # use teksilo_core::Intent;
//! let _w = MenuList::new()
//!     .item(MenuItem::new(lit!("Cut")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.cut"))))
//!     .item(MenuItem::new(lit!("Copy")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.copy"))))
//!     .separator()
//!     .item(MenuItem::new(lit!("Paste")).on_activate_fn(|ctx| ctx.send_intent(Intent::new("app.paste"))));
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::overlay::OverlayPlacement;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{PopoverStyleConfig, PopoverVariant};
use teksilo_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::SurfaceRole;

use crate::primitives::{MaxSize, Padding, RectWidget, VStack, ZStack};
use crate::scroll_area::ScrollArea;

/// Marker for whether a pending entry is a menu item, a separator, or a header.
enum MenuEntry {
    /// A menu item with an optional reactive visibility gate. When the gate
    /// is `Some(false)` the item's row collapses to zero height (no gap) and
    /// is skipped by keyboard navigation — the conditionally-shown menu row.
    Item {
        pending: PendingChild,
        visible: Option<teksilo_core::signal::Prop<bool>>,
    },
    Separator,
    /// A non-interactive section caption (e.g. a `GroupHeader`). Excluded from
    /// keyboard navigation and type-ahead exactly like `Separator` — it never
    /// occupies a slot in `item_widget_ids`/`resolved_labels`, so no runtime
    /// "skip if header" branch is needed anywhere. Still reachable by assistive
    /// technology: the wrapped widget declares its own name/role (`GroupHeader`
    /// sets `Role::Label` + the caption), which survives a11y-tree pruning as a
    /// flat sibling under the menu, exactly like `MenuSeparator`'s `Role::Splitter`.
    Header(PendingChild),
}

/// The active `MenuItemStyle`'s row metrics, from the theme slot.
///
/// A `MenuSeparator` and the scroll viewport are siblings of the rows, not
/// rows themselves, so there is no per-call `.style(...)` to consult — the
/// theme slot is the only style they can share with the items around them.
fn menu_metrics(theme: &teksilo_core::Theme) -> teksilo_core::styles::MenuItemMetrics {
    use teksilo_core::styles::MenuItemStyle;

    theme
        .style_slots
        .menu_item
        .as_ref()
        .map(|s| s.metrics())
        .unwrap_or_else(|| crate::styles::RecipeMenuItemStyle::for_tokens(&theme.input).metrics())
}

/// A 1 dp horizontal divider line between groups of menu items.
#[derive(Debug)]
pub struct MenuSeparator;

impl Widget for MenuSeparator {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let width = proposal.width.unwrap_or(0.0);
        Size::new(width, menu_metrics(ctx.theme).separator_height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut teksilo_canvas::Canvas, ctx: &PaintContext) {
        // Int UI menu separator: a flush-edge 1 dp line in `divider` color,
        // vertically centered in the `separator_height` (9 dp) slot — that
        // slot provides 4 dp top/bottom breathing room around the line.
        let color = ctx.theme.colors.divider;
        let thickness = ctx.theme.shape.border_width;
        let y = bounds.y + (bounds.height - thickness) * 0.5;
        canvas.fill_rect(Rect::new(bounds.x, y, bounds.width, thickness), color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Splitter);
    }
}

// Note: MenuList's `max_visible_items` caps the panel height and wraps
// the item column in a `ScrollArea`, but does **not** yet virtualize —
// every item widget (plus separators) is still built eagerly. True
// virtualization requires a model-backed MenuList API (item descriptor
// → delegate builds the row) because today's surface accepts arbitrary
// `impl Widget` children directly. Tracked as follow-up; eager build
// is cheap enough that ScrollArea-capped panels of 100+ items are
// already fine in practice.

/// Wrapper that adds a keyboard-focus highlight behind a menu item.
/// The highlight is driven by a shared `focused_index` signal — when
/// `focused_index == Some(my_index)`, a subtle background appears.
/// The binding registry automatically marks this widget for repaint
/// when the signal changes (same mechanism as ComboBox DropdownItem).
#[derive(Debug)]
struct KeyboardHighlightWrapper {
    item_id: WidgetId,
    index: usize,
    focused_index: Signal<Option<usize>>,
    root_child_id: Option<WidgetId>,
}

impl Widget for KeyboardHighlightWrapper {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let index = self.index;

        // Keyboard focus highlight uses the dedicated `surface_selected`
        // token (not an alpha wash over `accent`) so it tracks theme
        // changes and stays distinct from mouse hover (`surface_hover`).
        // Role-based: no theme_signal zip; paint resolves the role.
        let bg_role = self.focused_index.map(move |focused| {
            if *focused == Some(index) {
                SurfaceRole::Selected
            } else {
                SurfaceRole::Transparent
            }
        });

        let bg = RectWidget::new().background(bg_role);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().child(bg_id).child(self.item_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Forward the proposal to the wrapped MenuItem directly rather than
        // going through the internal ZStack. ZStack::size_that_fits always
        // queries its children with `unspecified` (correct for most uses,
        // since ZStack layers typically have independent natural sizes),
        // which would strip the parent's width proposal. But for this
        // wrapper the whole point is that the MenuItem fills the VStack's
        // cross-axis width — bypass the ZStack in the sizing path so the
        // width propagates to the MenuItem → HStack → spacer chain.
        let item_size = ctx
            .child_size(self.item_id, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 32.0));
        // Respect the proposed width when offered, so VStack::place_children
        // places this wrapper at the full popup width.
        let width = proposal.width.unwrap_or(item_size.width);
        Size::new(width, item_size.height).into()
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
        self.root_child_id.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational wrapper — the real semantics live on the
        // wrapped MenuItem. Without this, the default node would
        // insert an unannotated container between `Role::Menu` and
        // `Role::MenuItem` in the a11y tree.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }
}

/// Scroll the row at `idx` into view after the keyboard highlight moved onto it.
///
/// Arrow / Home / End / type-ahead navigation moves `focused_index`, **not**
/// real tree focus (which stays on the panel so the key handler keeps
/// receiving keys; assistive technology follows the highlight through the
/// panel's active descendant), so the framework's own focus-follow scroll
/// never runs.
/// Past `max_visible_items` the panel is a `ScrollArea`, and without this the
/// highlight walks straight out of the viewport and the menu looks frozen.
///
/// The id-based reveal is the right one here: a menu row is a real, mounted,
/// non-virtualized child, so the arena already knows its bounds. It is a no-op
/// when the row is already visible or nothing above it scrolls.
fn reveal(idx: usize, item_ids: &[WidgetId], ctx: &mut EventContext) {
    if let Some(&id) = item_ids.get(idx) {
        ctx.ensure_widget_visible(id);
    }
}

/// Activate the row at `idx` from the keyboard: through the `MenuItem`'s own
/// keyboard route when it is one (see [`KeyboardActivationSlot`]), and with a
/// click on any other row, which has no such route.
fn activate_row(
    idx: usize,
    item_ids: &[WidgetId],
    activations: &[Option<KeyboardActivationSlot>],
    ctx: &mut EventContext,
) {
    let keyboard = activations
        .get(idx)
        .and_then(|slot| slot.as_ref())
        .and_then(|slot| slot.borrow().clone());
    match keyboard {
        Some(activate) => activate(ctx),
        None => ctx.synthetic_click(item_ids[idx]),
    }
}

/// A themed vertical dropdown menu panel with keyboard navigation and type-ahead.
///
/// See the module documentation for the full feature description.
pub struct MenuList {
    entries: Vec<MenuEntry>,
    root_child_id: Option<WidgetId>,
    /// Widget IDs of actual menu items (not separators), for keyboard navigation.
    item_widget_ids: Vec<WidgetId>,
    /// Per-item reactive visibility gate (parallel to `item_widget_ids`).
    /// `None` → always visible; `Some(prop)` → the item is shown only while
    /// the prop is `true`. Keyboard navigation skips items whose gate is
    /// currently `false`.
    item_visibility: Vec<Option<teksilo_core::signal::Prop<bool>>>,
    /// Whether each item (by index into item_widget_ids) is a submenu trigger.
    submenu_flags: Vec<bool>,
    /// When set and the item count (counting items only — separators and
    /// headers contribute nothing — against the row count, not pixels)
    /// exceeds the limit, the content column is wrapped in a `ScrollArea`
    /// and the panel height is capped to `n * item_height`. `None`
    /// (default) lets the menu grow with its content.
    max_visible_items: Option<usize>,
    /// Side of the menu panel that is visually attached to its trigger
    /// (e.g. a menu button or combo-box). When set, drop shadow
    /// drawing is suppressed on that side so the menu reads as one
    /// piece with the trigger. Set by the opener based on the chosen
    /// placement; `None` leaves the full halo intact.
    attached_side: Option<crate::shadow::AttachedSide>,
    /// Type-ahead buffer reset window. After this much time since the
    /// last typed character with no match-extension, the buffer is
    /// cleared on the next keypress. Defaults to 500 ms (Windows
    /// menubar convention).
    type_ahead_timeout: Duration,
    /// The keyboard highlight: the index into `item_widget_ids` of the row
    /// arrows, Home / End, page keys and type-ahead last moved to. Read by
    /// `accessibility()` to name that row as the menu's active descendant.
    highlight: Signal<Option<usize>>,
    /// The rows a reader counts, shared with every `MenuItem` in the list
    /// so each can say where it stands ("3 of 7").
    rows: SharedMenuRows,
    /// What opened this menu, when the opener said so (a menu-bar trigger,
    /// a submenu row, a popover's button). The menu is named after it.
    opener: Option<WidgetId>,
}

/// The rows of one [`MenuList`], as assistive technology counts them.
///
/// A separator and a header are not rows, and a row an `item_when` gate
/// hides is not one while it is hidden: none of them is in the platform's
/// tree. Shared with each `MenuItem` so the item can publish its own
/// position, and read by the list for the set size, because AccessKit reads
/// an item's set size from its container, never from the item
/// (`accesskit_consumer` `node.rs:629-641`).
#[derive(Debug, Default)]
pub(crate) struct MenuRows {
    ids: Vec<WidgetId>,
    visible: Vec<Option<teksilo_core::signal::Prop<bool>>>,
}

impl MenuRows {
    fn is_shown(&self, index: usize) -> bool {
        self.visible
            .get(index)
            .and_then(|gate| gate.as_ref())
            .is_none_or(|gate| gate.get())
    }

    /// How many rows are shown.
    fn shown_count(&self) -> usize {
        (0..self.ids.len()).filter(|&i| self.is_shown(i)).count()
    }

    /// The 1-based position of the row `id` among the shown rows, or `None`
    /// when it is not a shown row of this menu.
    pub(crate) fn position_of(&self, id: WidgetId) -> Option<usize> {
        let index = self.ids.iter().position(|&row| row == id)?;
        self.is_shown(index)
            .then(|| (0..=index).filter(|&i| self.is_shown(i)).count())
    }
}

/// Shared handle to a menu's [`MenuRows`].
pub(crate) type SharedMenuRows = Rc<RefCell<MenuRows>>;

/// How a `MenuItem` is activated from its menu's keyboard: filled in by the
/// item when it builds, called by the enclosing [`MenuList`] on Enter, Space,
/// the inline-forward arrow and a mnemonic.
///
/// The menu keeps focus while its rows are highlighted, so the item's own key
/// handler never runs. Activating the row with a synthesised click instead
/// made every submenu the keyboard opened a *mouse*-opened one, whose
/// dismissal is the 150 ms pointer-leave grace: a mouse resting anywhere else
/// closed it at once. This is the item's keyboard and assistive-technology
/// route, the one `Action::Click` takes.
pub(crate) type KeyboardActivationSlot = Rc<RefCell<Option<Rc<dyn Fn(&mut EventContext)>>>>;

/// Per-MenuList shared state for the safe-triangle submenu hover gate:
/// which submenu in this list is currently open, so a sibling row can
/// ask the framework whether the pointer is inside *that* overlay's
/// armed safe region before it fires `dismiss_child_overlays`.
///
/// The geometry — the apex, the cone, its budget — lives in
/// `teksilo_core::overlay`, because the overlay's own pointer-leave
/// grace has to honour the same region; the two would drift if the
/// widget kept a private copy. All this side has to carry is the
/// identity of the overlay to ask about.
#[derive(Debug, Default)]
pub(crate) struct SafeTriangleState {
    /// The currently-open submenu's root content widget id, or `None`
    /// when no submenu in this list is open. Published by the trigger
    /// when the pointer leaves its row with the submenu up.
    pub submenu_content_id: Option<WidgetId>,
}

/// Shared handle installed on every MenuItem that participates in
/// safe-triangle gating. The same `Rc` is held by the MenuList and
/// by each child MenuItem; updates flow both directions.
pub(crate) type SharedSafeTriangleState = Rc<RefCell<SafeTriangleState>>;

impl MenuList {
    /// Create an empty menu list with no items, no height cap, and the default
    /// 500 ms type-ahead reset window.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            root_child_id: None,
            item_widget_ids: Vec::new(),
            item_visibility: Vec::new(),
            submenu_flags: Vec::new(),
            max_visible_items: None,
            attached_side: None,
            type_ahead_timeout: Duration::from_millis(500),
            highlight: Signal::new(None),
            rows: SharedMenuRows::default(),
            opener: None,
        }
    }

    /// Name this menu after the widget that opened it: a menu-bar trigger,
    /// the submenu row it hangs from, the button of the popover it sits in.
    /// A reader then hears "File menu" as focus enters it, where it heard
    /// "menu" and nothing else. Set by the opener before the menu is built.
    pub(crate) fn set_opener(&mut self, opener: WidgetId) {
        self.opener = Some(opener);
    }

    /// Override the type-ahead buffer reset window. Defaults to 500ms
    /// to match Windows' menubar convention. Tests use
    /// `Duration::ZERO` to force every keypress to start a fresh
    /// search.
    pub fn type_ahead_timeout(mut self, d: Duration) -> Self {
        self.type_ahead_timeout = d;
        self
    }

    /// Suppress drop-shadow drawing on the side that visually merges
    /// with the menu's trigger. See [`crate::shadow::AttachedSide`]
    /// for the available edges.
    pub fn attached_side(mut self, side: crate::shadow::AttachedSide) -> Self {
        self.attached_side = Some(side);
        self
    }

    /// Add a menu item (typically a `MenuItem`).
    pub fn item(mut self, widget: impl Widget + 'static) -> Self {
        // Probe through the `as_any` hook rather than downcasting the generic
        // directly: a `MenuItem` carrying any builder method (`.context_menu`,
        // `.focusable`, …) arrives here as `WidgetWithHandlers<MenuItem>`, which
        // a concrete-type downcast misses while `as_any` forwards through it.
        // This is the probe [`item_boxed_when`](Self::item_boxed_when) already
        // uses; the two disagreeing is what let a decorated submenu trigger lose
        // its inline-forward arrow.
        let is_submenu = widget
            .as_any()
            .and_then(|a| a.downcast_ref::<crate::menu_item::MenuItem>())
            .is_some_and(|mi| mi.is_submenu());
        self.submenu_flags.push(is_submenu);
        self.entries.push(MenuEntry::Item {
            pending: teksilo_core::IntoTeksiChild::into_pending(widget),
            visible: None,
        });
        self
    }

    /// Add several menu items from an iterator, in order.
    ///
    /// The loop form of [`item`](Self::item): reach for it when the rows come
    /// from data rather than being written out one call at a time.
    pub fn items(self, iter: impl IntoIterator<Item = impl Widget + 'static>) -> Self {
        iter.into_iter().fold(self, Self::item)
    }

    /// Add a menu item that is shown only while `visible` is `true`. When the
    /// gate is `false` the row collapses to zero height (no gap) and keyboard
    /// navigation skips it — arrows, `Home`/`End`, `Enter`, type-ahead, and
    /// mnemonic activation all ignore it. Used e.g. by a `Toolbar`'s overflow
    /// menu, where each row is present only while its inline twin is collapsed.
    ///
    /// Because a hidden row never claims its mnemonic letter, two gated rows
    /// that are mutually exclusive may share one — the letter resolves to
    /// whichever is visible when it is pressed.
    pub fn item_when(
        self,
        widget: impl Widget + 'static,
        visible: impl Into<teksilo_core::signal::Prop<bool>>,
    ) -> Self {
        self.item_boxed_when(Box::new(widget), visible)
    }

    /// Add several gated rows from an iterator of `(widget, visible)` pairs.
    ///
    /// The loop form of [`item_when`](Self::item_when), for a gated row set
    /// built from data. Each pair carries its own gate, so the rows appear and
    /// disappear independently.
    pub fn items_when<W, V>(self, iter: impl IntoIterator<Item = (W, V)>) -> Self
    where
        W: Widget + 'static,
        V: Into<teksilo_core::signal::Prop<bool>>,
    {
        iter.into_iter().fold(self, |list, (widget, visible)| {
            list.item_when(widget, visible)
        })
    }

    /// [`item_when`](Self::item_when) for an already-boxed widget — used when
    /// the row type is decided at runtime (e.g. a menu row that is either a
    /// `MenuItem` or an embedded control).
    pub fn item_boxed_when(
        mut self,
        widget: Box<dyn Widget>,
        visible: impl Into<teksilo_core::signal::Prop<bool>>,
    ) -> Self {
        let is_submenu = widget
            .as_any()
            .and_then(|a| a.downcast_ref::<crate::menu_item::MenuItem>())
            .is_some_and(|mi| mi.is_submenu());
        self.submenu_flags.push(is_submenu);
        self.entries.push(MenuEntry::Item {
            pending: PendingChild::Deferred(widget),
            visible: Some(visible.into()),
        });
        self
    }

    /// Add several gated rows from an iterator of `(widget, visible)` pairs.
    ///
    /// The loop form of [`item_boxed_when`](Self::item_boxed_when), for a gated
    /// row set built from data. Each pair carries its own gate, so the rows may
    /// appear and disappear independently.
    pub fn items_boxed_when<V>(self, iter: impl IntoIterator<Item = (Box<dyn Widget>, V)>) -> Self
    where
        V: Into<teksilo_core::signal::Prop<bool>>,
    {
        iter.into_iter().fold(self, |list, (widget, visible)| {
            list.item_boxed_when(widget, visible)
        })
    }

    /// Add a separator line.
    pub fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }

    /// Add a non-interactive section caption (typically a [`crate::GroupHeader`]).
    /// Skipped by Arrow/Home/End navigation and type-ahead, exactly like
    /// [`separator`](Self::separator). The caller passes any `impl Widget`, but it
    /// must expose its own accessible name/role via `accessibility()` (as
    /// `GroupHeader` does) or it is silently pruned from the AT tree as a
    /// content-free container.
    pub fn header(mut self, widget: impl Widget + 'static) -> Self {
        self.entries.push(MenuEntry::Header(
            teksilo_core::IntoTeksiChild::into_pending(widget),
        ));
        self
    }

    /// Derive the `OverlayPlacement` the `PopoverStyle` needs from the
    /// caller-supplied `attached_side`. `PopoverSurface` re-resolves the
    /// concrete suppressed shadow edge from this placement plus the live
    /// layout direction, so the menu reads as one piece with its trigger.
    fn derived_placement(&self) -> OverlayPlacement {
        match self.attached_side {
            Some(crate::shadow::AttachedSide::Top) => OverlayPlacement::Below,
            Some(crate::shadow::AttachedSide::Bottom) => OverlayPlacement::Above,
            // A trigger on the leading edge → menu opens trailing.
            // `Right` (trigger on the trailing edge, menu opens leading)
            // has no dedicated placement; fall back to the full halo.
            Some(crate::shadow::AttachedSide::Left) => OverlayPlacement::TrailingEdge,
            Some(crate::shadow::AttachedSide::Right) | None => OverlayPlacement::Centered,
        }
    }

    /// Cap the panel height to roughly `n * item_height` and make the
    /// content scrollable when that height is exceeded. Clamped to at
    /// least 1. Useful for long menus (e.g. a "Recent files" list) —
    /// without this, a very long menu grows to exceed the window.
    ///
    /// Note: items are still materialized eagerly; this is a viewport
    /// cap, not virtualization. See the module-level note.
    pub fn max_visible_items(mut self, n: usize) -> Self {
        self.max_visible_items = Some(n.max(1));
        self
    }
}

impl Default for MenuList {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MenuList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuList")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Widget for MenuList {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _theme_signal = ctx.theme_signal();

        // Keyboard-focused item index (shared with the key handler and wrappers).
        // The binding registry propagates repaints when this changes, and
        // re-walks this node's accessibility, which names the highlighted row
        // as the menu's active descendant.
        let focused_index: Signal<Option<usize>> = ctx.signal(None);
        focused_index.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.highlight = focused_index.clone();
        // The rows as a reader counts them, filled in once every row has an id.
        let rows: SharedMenuRows = Rc::new(RefCell::new(MenuRows::default()));
        self.rows = rows.clone();
        // Each `MenuItem`'s keyboard activation, parallel to `item_widget_ids`;
        // `None` for a row that is not a `MenuItem`, which is clicked instead.
        let mut activations: Vec<Option<KeyboardActivationSlot>> = Vec::new();

        // Build all entries into a VStack, wrapping items in highlight wrappers
        let mut vstack = VStack::new();
        self.item_widget_ids.clear();
        self.item_visibility.clear();
        let mut item_counter = 0_usize;

        // Radio-group buffers keyed by `Signal<usize>` identity. Linear
        // search is fine — a single menu rarely carries more than a
        // handful of radio groups. The same shared `Rc<RefCell<Vec<…>>>`
        // is installed on every member via
        // [`MenuItem::set_radio_group_ids`](crate::menu_item::MenuItem::set_radio_group_ids)
        // BEFORE the items reach the arena; the buffer's contents are
        // filled in below as each member id is allocated. By the time
        // the AT walker reads `MenuItem::accessibility`, all sibling
        // ids are in place.
        let mut radio_buffers: Vec<(Signal<usize>, Rc<RefCell<Vec<WidgetId>>>)> = Vec::new();
        // Tracks which radio buffer (if any) each newly-added item
        // belongs to, so we can push the item's id once known.
        let mut pending_radio_pushes: Vec<(usize, Rc<RefCell<Vec<WidgetId>>>)> = Vec::new();

        // Keyboard-navigation caches:
        // * `resolved_labels[i]` is the ASCII-lowercased stripped
        //   label of the item at item-array position `i`. Used by
        //   the type-ahead branch in the keyboard handler.
        // * `mnemonic_table[c]` maps a lowercase mnemonic char to every
        //   item-array position claiming it, in declaration order. Used
        //   by the in-menu mnemonic branch ("press the underlined letter
        //   to activate"), which picks the first claimant that is
        //   currently visible — so two `item_when`-gated rows that are
        //   mutually exclusive may share one letter.
        // * `unconditional[i]` is `true` when the item at `i` has no
        //   visibility gate. Two unconditional rows sharing a mnemonic
        //   can never disambiguate, which is the one statically-decidable
        //   authoring bug — see the `debug_assert` below.
        // All three are sized to `item_widget_ids.len()`; separators
        // contribute nothing.
        let mut resolved_labels: Vec<String> = Vec::new();
        let mut unconditional: Vec<bool> = Vec::new();
        let mut mnemonic_table: HashMap<char, Vec<usize>> = HashMap::new();

        // Safe-triangle shared state. Installed on every MenuItem in
        // this list so a submenu trigger can stamp the anchor and
        // sibling items can read it from their hover gate.
        let safe_triangle: SharedSafeTriangleState =
            Rc::new(RefCell::new(SafeTriangleState::default()));

        for entry in self.entries.drain(..) {
            match entry {
                MenuEntry::Item { pending, visible } => {
                    let (item_id, radio_buf, item_label, item_mnemonic, activation) = match pending
                    {
                        PendingChild::Id(id) => (id, None, None, None, None),
                        PendingChild::Deferred(mut w) => {
                            // Single downcast pass: read the radio
                            // selection signal AND the parsed mnemonic
                            // AND install the safe-triangle shared
                            // state, the shared rows and the keyboard
                            // activation slot, before moving the box
                            // into the arena.
                            let (radio_buf, item_label, item_mnemonic, activation) = w
                                .as_any_mut()
                                .and_then(|a| a.downcast_mut::<crate::menu_item::MenuItem>())
                                .map(|mi| {
                                    // Ensure the label has been parsed
                                    // for `&`-markers BEFORE the item
                                    // builds — so `mnemonic()` returns
                                    // a value even pre-build.
                                    mi.ensure_mnemonic_parsed();
                                    let label =
                                        mi.mnemonic().map(|p| p.stripped.to_ascii_lowercase());
                                    let mnemonic = mi.mnemonic().and_then(|p| p.key_lower);
                                    let radio = mi.radio_selection_handle().map(|(_, sig)| {
                                        let buf = if let Some((_, b)) = radio_buffers
                                            .iter()
                                            .find(|(s, _)| Signal::same(s, &sig))
                                        {
                                            b.clone()
                                        } else {
                                            let b = Rc::new(RefCell::new(Vec::new()));
                                            radio_buffers.push((sig.clone(), b.clone()));
                                            b
                                        };
                                        mi.set_radio_group_ids(buf.clone());
                                        buf
                                    });
                                    mi.set_safe_triangle_state(safe_triangle.clone());
                                    mi.set_menu_rows(rows.clone());
                                    let activation = KeyboardActivationSlot::default();
                                    mi.set_keyboard_activation_slot(activation.clone());
                                    (radio, label, mnemonic, Some(activation))
                                })
                                .unwrap_or((None, None, None, None));
                            (
                                ctx.add_boxed(w),
                                radio_buf,
                                item_label,
                                item_mnemonic,
                                activation,
                            )
                        }
                    };
                    self.item_widget_ids.push(item_id);
                    self.item_visibility.push(visible.clone());
                    activations.push(activation);
                    let item_idx = self.item_widget_ids.len() - 1;
                    if let Some(buf) = radio_buf {
                        pending_radio_pushes.push((item_idx, buf));
                    }
                    resolved_labels.push(item_label.unwrap_or_default());
                    unconditional.push(visible.is_none());
                    if let Some(c) = item_mnemonic {
                        let claims = mnemonic_table.entry(c).or_default();
                        // A collision only *has* to be a bug when both
                        // rows are always on screen. Gated rows are
                        // typically mutually exclusive (`item_when`), and
                        // dispatch resolves those to whichever is visible
                        // at the time — so don't cry wolf on them.
                        debug_assert!(
                            !unconditional[item_idx] || !claims.iter().any(|&p| unconditional[p]),
                            "MenuList: duplicate item mnemonic {c:?} — item {item_idx} and an \
                             earlier item among {claims:?} are both unconditionally visible, \
                             so the letter is ambiguous"
                        );
                        claims.push(item_idx);
                    }

                    // Wrap in a highlight container driven by focused_index.
                    // A per-item visibility gate is applied to the WRAPPER (not
                    // the inner item) so a hidden row collapses to zero height
                    // — no empty gap — while keeping `item_widget_ids` pointing
                    // at the real item, the one keyboard activation reaches and
                    // the active descendant names.
                    let wrapper_id = ctx.add(KeyboardHighlightWrapper {
                        item_id,
                        index: item_counter,
                        focused_index: focused_index.clone(),
                        root_child_id: None,
                    });
                    if let Some(vis) = visible {
                        ctx.visible_when(wrapper_id, vis);
                    }
                    vstack = vstack.child(wrapper_id);
                    item_counter += 1;
                }
                MenuEntry::Separator => {
                    vstack = vstack.child(MenuSeparator);
                }
                MenuEntry::Header(pending) => {
                    // Rendered as a plain child — never pushed into
                    // `item_widget_ids`/`resolved_labels`/`item_counter`, so it is
                    // structurally excluded from keyboard nav + type-ahead (same
                    // mechanism as `Separator`). Its own `accessibility()` carries
                    // the section name for screen readers.
                    let header_id = match pending {
                        PendingChild::Id(id) => id,
                        PendingChild::Deferred(w) => ctx.add_boxed(w),
                    };
                    vstack = vstack.child(header_id);
                }
            }
        }

        // Fill each radio group's id list now that every item has a
        // WidgetId. Each id is pushed exactly once.
        for (item_idx, buf) in pending_radio_pushes {
            buf.borrow_mut().push(self.item_widget_ids[item_idx]);
        }
        *rows.borrow_mut() = MenuRows {
            ids: self.item_widget_ids.clone(),
            visible: self.item_visibility.clone(),
        };

        let vstack_id = ctx.add(vstack);

        let padding = Padding::uniform(4.0).child(vstack_id);
        let padding_id = ctx.add(padding);

        // Viewport cap. When `max_visible_items` is set and the real
        // item count exceeds it, wrap the padded column in a
        // `ScrollArea` + `MaxSize` pair sized to `cap * item_height`
        // + the 4 px outer padding on each edge. Separators don't
        // count against the cap — they're visually small and no real
        // menu stacks enough of them for the slight under-shoot to
        // matter.
        let visible_cap_id = match self.max_visible_items {
            Some(cap) if self.item_widget_ids.len() > cap => {
                let max_height = cap as f32 * menu_metrics(ctx.theme()).item_height + 8.0;
                // Cap the HEIGHT only. `preferred_size(0.0, ..)` would set the preferred
                // *width* to zero — and a popover proposes an unconstrained width (it
                // hugs its content), so the zero was taken literally: the menu collapsed
                // to its minimum width and every row was clipped to a middle slice.
                let scrollable = ScrollArea::from_id(padding_id).preferred_height(max_height);
                let scrollable_id = ctx.add(scrollable);
                ctx.add(MaxSize::height(max_height).child(scrollable_id))
            }
            _ => padding_id,
        };

        // Themed surface — routed through `PopoverStyle` (the
        // `Menu`-flavoured variant), so the menu panel's background,
        // border, corner radius, and drop shadow are all owned by the
        // active popover style instead of a hand-rolled bg `RectWidget`
        // + `MenuList::paint`. The full-halo vs trigger-attached
        // shadow choice is derived from `attached_side`.
        let popover_style: teksilo_core::styles::SharedPopoverStyle =
            ctx.theme().style_slots.popover.clone().unwrap_or_else(|| {
                Rc::new(crate::styles::RecipePopoverStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
        let surface_cfg = PopoverStyleConfig {
            content: visible_cap_id,
            variant: PopoverVariant::Menu,
            name: String::new(),
            placement: self.derived_placement(),
            show_caret: false,
            caret_size: 0.0,
        };
        let root_id = popover_style.make_body(&surface_cfg, ctx);

        self.root_child_id = Some(root_id);

        // Keyboard navigation handler
        let item_count = self.item_widget_ids.len();
        let item_ids = self.item_widget_ids.clone();
        let sub_flags = self.submenu_flags.clone();
        // Type-ahead state. Shared across keypresses via `Rc` so the
        // `Fn` closure can mutate the buffer without taking `&mut self`.
        let type_ahead_buffer: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let type_ahead_last_input: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let type_ahead_timeout = self.type_ahead_timeout;
        let resolved_labels = Rc::new(resolved_labels);
        let mnemonic_table = Rc::new(mnemonic_table);
        // Per-item visibility gates, so navigation skips collapsed rows.
        let visibilities = Rc::new(self.item_visibility.clone());
        let activations = Rc::new(activations);
        let handler_set = HandlerSet::new()
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    // Inline-forward (open submenu) vs inline-back arrows
                    // mirror under RTL: forward is ArrowRight in LTR /
                    // ArrowLeft in RTL; back is the opposite.
                    let open_submenu_key = if ctx.is_rtl() {
                        Key::ArrowLeft
                    } else {
                        Key::ArrowRight
                    };
                    let back_key = if ctx.is_rtl() {
                        Key::ArrowRight
                    } else {
                        Key::ArrowLeft
                    };
                    // Currently-visible item indices, in order. Hidden
                    // (collapsed) rows are skipped by arrow / Home / End nav.
                    let visible_indices: Vec<usize> = (0..item_count)
                        .filter(|&i| {
                            visibilities
                                .get(i)
                                .and_then(|o| o.as_ref())
                                .map(|p| p.get())
                                .unwrap_or(true)
                        })
                        .collect();
                    match key {
                        Key::ArrowDown => {
                            if visible_indices.is_empty() {
                                return EventResponse::Ignored;
                            }
                            let pos = focused_index
                                .get()
                                .and_then(|c| visible_indices.iter().position(|&x| x == c));
                            let next = match pos {
                                Some(p) => visible_indices[(p + 1) % visible_indices.len()],
                                None => visible_indices[0],
                            };
                            focused_index.set(Some(next));
                            ctx.show_highlight_tooltip(item_ids[next]);
                            reveal(next, &item_ids, ctx);
                            EventResponse::Handled
                        }
                        Key::ArrowUp => {
                            if visible_indices.is_empty() {
                                return EventResponse::Ignored;
                            }
                            let n = visible_indices.len();
                            let pos = focused_index
                                .get()
                                .and_then(|c| visible_indices.iter().position(|&x| x == c));
                            let next = match pos {
                                Some(p) => visible_indices[(p + n - 1) % n],
                                None => visible_indices[n - 1],
                            };
                            focused_index.set(Some(next));
                            ctx.show_highlight_tooltip(item_ids[next]);
                            reveal(next, &item_ids, ctx);
                            EventResponse::Handled
                        }
                        Key::Home => {
                            let Some(&first) = visible_indices.first() else {
                                return EventResponse::Ignored;
                            };
                            focused_index.set(Some(first));
                            ctx.show_highlight_tooltip(item_ids[first]);
                            reveal(first, &item_ids, ctx);
                            EventResponse::Handled
                        }
                        Key::End => {
                            let Some(&last) = visible_indices.last() else {
                                return EventResponse::Ignored;
                            };
                            focused_index.set(Some(last));
                            ctx.show_highlight_tooltip(item_ids[last]);
                            reveal(last, &item_ids, ctx);
                            EventResponse::Handled
                        }
                        // A long menu (a language list, a recent-files list)
                        // is the only place these earn their keep, and they
                        // were the one list chord `MenuList` lacked. A page is
                        // ten visible rows — menus have no viewport of their
                        // own to measure, and ten is the step every menu
                        // implementation that has one uses.
                        Key::PageUp | Key::PageDown => {
                            const MENU_PAGE: usize = 10;
                            let n = visible_indices.len();
                            if n == 0 {
                                return EventResponse::Ignored;
                            }
                            let pos = focused_index
                                .get()
                                .and_then(|f| visible_indices.iter().position(|&v| v == f));
                            let next_pos = match (*key == Key::PageDown, pos) {
                                (true, Some(p)) => (p + MENU_PAGE).min(n - 1),
                                (true, None) => 0,
                                (false, Some(p)) => p.saturating_sub(MENU_PAGE),
                                (false, None) => n - 1,
                            };
                            let next = visible_indices[next_pos];
                            focused_index.set(Some(next));
                            ctx.show_highlight_tooltip(item_ids[next]);
                            reveal(next, &item_ids, ctx);
                            EventResponse::Handled
                        }
                        Key::Enter | Key::Space => {
                            // Activate the focused item (see `activate_row`),
                            // but only if it is currently visible.
                            if let Some(idx) = focused_index.get()
                                && visible_indices.contains(&idx)
                                && idx < item_ids.len()
                            {
                                activate_row(idx, &item_ids, &activations, ctx);
                                return EventResponse::Handled;
                            }
                            EventResponse::Ignored
                        }
                        k if *k == open_submenu_key => {
                            // Inline-forward arrow: only opens submenus; for
                            // non-submenu items let it bubble to
                            // MenuOverlayHost, which navigates to the next bar
                            // menu. RTL-flipped via `open_submenu_key`.
                            if let Some(idx) = focused_index.get()
                                && idx < sub_flags.len()
                                && sub_flags[idx]
                            {
                                activate_row(idx, &item_ids, &activations, ctx);
                                return EventResponse::Handled;
                            }
                            EventResponse::Ignored
                        }
                        k if *k == back_key => {
                            // Inline-back arrow: bubble to MenuOverlayHost (bar
                            // navigation) or the tree-level back/overlay
                            // dismissal. RTL-flipped via `back_key`.
                            EventResponse::Ignored
                        }
                        Key::Escape => {
                            // Bubble to the tree-level Escape overlay dismissal.
                            EventResponse::Ignored
                        }
                        _ => {
                            // Letter handling: in-menu mnemonic
                            // activation (bare letter) wins over
                            // type-ahead, which wins over ignored.
                            // We accept Shift here because Windows /
                            // GNOME convention activates the
                            // mnemonic regardless of Shift state
                            // (otherwise Shift-Lock users couldn't
                            // mnemonic-activate items at all). Ctrl
                            // / Alt / Cmd chords fall through to the
                            // global Shortcut/Action pipeline —
                            // except `AltGr`, which is how a non-US
                            // layout types a character rather than an
                            // accelerator. See
                            // `range_nav::is_text_entry_chord`.
                            if !crate::common::range_nav::is_text_entry_chord(*modifiers) {
                                return EventResponse::Ignored;
                            }
                            let ch = match key {
                                Key::Character(c) => Some(c.to_ascii_lowercase()),
                                k => k.to_char().map(|c| c.to_ascii_lowercase()),
                            };
                            let Some(ch) = ch else {
                                return EventResponse::Ignored;
                            };
                            if item_count == 0 {
                                return EventResponse::Ignored;
                            }

                            // 1) Mnemonic match — explicit accelerator,
                            //    activates the item. A hidden
                            //    (`item_when`-gated) row never claims its
                            //    letter, matching the visibility gate the
                            //    arrow / Enter branches apply; the first
                            //    currently-visible claimant wins. Falls
                            //    through to type-ahead when every claimant
                            //    is hidden.
                            if let Some(idx) = mnemonic_table.get(&ch).and_then(|claims| {
                                claims.iter().copied().find(|i| visible_indices.contains(i))
                            }) {
                                activate_row(idx, &item_ids, &activations, ctx);
                                return EventResponse::Handled;
                            }

                            // 2) Type-ahead — incremental prefix match
                            //    against the resolved labels of the
                            //    currently-visible rows.
                            let now = Instant::now();
                            let mut buf = type_ahead_buffer.borrow_mut();
                            if let Some(prev) = type_ahead_last_input.get() {
                                if now.duration_since(prev) > type_ahead_timeout {
                                    buf.clear();
                                }
                            }
                            buf.push(ch);
                            type_ahead_last_input.set(Some(now));

                            if visible_indices.is_empty() {
                                return EventResponse::Ignored;
                            }
                            let n = visible_indices.len();
                            // Position of the focused row *within the
                            // visible run*; an unfocused (or hidden-row)
                            // focus anchors at the first visible row.
                            let start = focused_index
                                .get()
                                .and_then(|c| visible_indices.iter().position(|&x| x == c))
                                .unwrap_or(0);
                            // Search wrapping from start+1 through start
                            // itself, so a single repeated letter cycles
                            // through matching items.
                            for offset in 1..=n {
                                let i = visible_indices[(start + offset) % n];
                                if let Some(label) = resolved_labels.get(i)
                                    && label.starts_with(buf.as_str())
                                {
                                    focused_index.set(Some(i));
                                    ctx.show_highlight_tooltip(item_ids[i]);
                                    reveal(i, &item_ids, ctx);
                                    return EventResponse::Handled;
                                }
                            }
                            EventResponse::Ignored
                        }
                    }
                },
            )
            .focusable(true);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        match self.root_child_id {
            Some(id) => {
                // Menu lists size to their content, with a minimum width
                let child_size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(child_size.width.max(120.0), child_size.height)
            }
            None => proposal.resolve(120.0, 0.0),
        }
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

    // No `paint()`: the menu panel's surface (background, border,
    // corner radius) and drop shadow are owned by the `PopoverStyle`
    // wrapper resolved in `build()`.

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use teksilo_core::accessibility::widget_id_to_node_id;

        builder.set_role(teksilo_core::accesskit::Role::Menu);
        // "File menu", not "menu": named after what opened it, through the
        // opener's own name, so the two cannot disagree.
        if let Some(opener) = self.opener {
            builder.push_labelled_by(widget_id_to_node_id(opener));
        }
        let rows = self.rows.borrow();
        builder.set_size_of_set(rows.shown_count());

        // The highlighted row, as the menu's active descendant.
        //
        // Focus stays on this panel while the arrows, Home / End, the page
        // keys and type-ahead move a highlight through its rows, so a reader
        // is told about the highlight only if it becomes a focus change.
        // `accesskit_consumer` resolves the platform's focus as
        // `focused.active_descendant().unwrap_or(focused)` (`tree.rs:537-543`)
        // and hands that node to all three adapters: AT-SPI raises
        // `state-changed:focused` on it (`accesskit_atspi_common`
        // `adapter.rs:324-340`), which Orca speaks as its new locus of focus;
        // UIA raises its focus-changed event (`accesskit_windows`
        // `adapter.rs:341-345`); macOS `FocusedUIElementChanged`
        // (`accesskit_macos` `event.rs:319-326`). Without it every move was
        // silent: Orca said "menu." as the menu opened and nothing after, and
        // Enter ran a command the reader had never heard. It is `ListView`'s
        // current row again (see `list_view/widget_impl.rs`). A highlight on a
        // row that is hidden names nothing, and the menu stays the focus.
        if let Some(index) = self.highlight.get()
            && rows.is_shown(index)
            && let Some(&row) = rows.ids.get(index)
        {
            builder.set_active_descendant(widget_id_to_node_id(row));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    /// Opt into reflection so an opener holding the menu as a boxed widget
    /// can name the menu after itself.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod reader_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_item::MenuItem;
    use teksilo_core::WidgetBuilder;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
    }

    #[test]
    fn unbounded_menu_grows_with_content() {
        // Without `max_visible_items`, a long menu should size to its
        // content — not be silently clipped. Use `with_width` so the
        // root takes its natural height from `size_that_fits` rather
        // than the proposal's exact height.
        let mut tree = light_tree();
        let mut menu = MenuList::new();
        for i in 0..20 {
            menu = menu.item(MenuItem::new(lit!(format!("Entry {i}"))));
        }
        let id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        let h = tree.bounds(id).height;
        // 20 items × 24 px ≈ 480 px — well above a capped viewport.
        assert!(
            h > 400.0,
            "uncapped menu should grow to fit all items, got height={}",
            h
        );
    }

    #[test]
    fn item_when_collapses_a_hidden_row_to_zero_height() {
        use teksilo_core::signal::Signal;
        // A gated row that is currently hidden must add no height — the menu
        // is the same height as if the row weren't there; revealing it grows
        // the menu by one row.
        let gate = Signal::new(false);

        let mut tree_gated = light_tree();
        let menu_gated = MenuList::new()
            .item(MenuItem::new(lit!("A")))
            .item_when(MenuItem::new(lit!("Gated")), gate.clone())
            .item(MenuItem::new(lit!("B")));
        let id_gated = tree_gated.add(menu_gated);
        tree_gated.layout(SizeProposal::with_width(300.0));
        let h_hidden = tree_gated.bounds(id_gated).height;

        let mut tree_two = light_tree();
        let menu_two = MenuList::new()
            .item(MenuItem::new(lit!("A")))
            .item(MenuItem::new(lit!("B")));
        let id_two = tree_two.add(menu_two);
        tree_two.layout(SizeProposal::with_width(300.0));
        let h_two = tree_two.bounds(id_two).height;

        assert!(
            (h_hidden - h_two).abs() < 0.5,
            "a hidden item_when row must add no height: {h_hidden} vs {h_two}"
        );

        gate.set(true);
        tree_gated.layout(SizeProposal::with_width(300.0));
        let h_shown = tree_gated.bounds(id_gated).height;
        assert!(
            h_shown > h_hidden + 10.0,
            "revealing the gated row must add a row's height: {h_shown} vs {h_hidden}"
        );
    }

    #[test]
    fn max_visible_items_caps_height() {
        // With `max_visible_items(5)`, a 20-entry menu must cap near
        // `5 * item_height + outer padding` rather than growing to fit
        // every row.
        let mut tree = light_tree();
        let mut menu = MenuList::new().max_visible_items(5);
        for i in 0..20 {
            menu = menu.item(MenuItem::new(lit!(format!("Entry {i}"))));
        }
        let id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        let h = tree.bounds(id).height;
        // 5 rows × 24 px + 8 px padding = 128. Give a generous
        // tolerance band (theme may tweak item_height); the key
        // regression to catch is "grew to fit everything" (~480 px).
        assert!(
            h < 200.0,
            "capped menu height should be bounded by max_visible_items, got {}",
            h
        );
        assert!(h > 0.0, "capped menu should have positive height");
    }

    #[test]
    fn max_visible_items_below_count_has_no_effect() {
        // When item count fits under the cap, the ScrollArea wrapper
        // must not be inserted — sanity check that we don't pay the
        // wrapper cost (or its minor layout overhead) for small menus.
        let mut tree = light_tree();
        let menu = MenuList::new()
            .max_visible_items(10)
            .item(MenuItem::new(lit!("A")))
            .item(MenuItem::new(lit!("B")));
        let id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        let h = tree.bounds(id).height;
        // 2 items × 24 = 48 px + padding ≈ 56 px. Much less than the
        // cap of 10 × 24 = 240 px.
        assert!(h < 100.0, "small menu should size to content, got {}", h);
    }

    // --- Keyboard activation: mnemonic, type-ahead, Home/End ---

    use std::cell::Cell as StdCell;
    use std::rc::Rc as StdRc;
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_core::signal::Signal;

    /// Build a menu list with an `on_activate_fn` for each entry that
    /// flips the matching slot in `fired`. Returns the list's
    /// `WidgetId` so the test can focus it and dispatch keys.
    fn menu_with_activation_probe(
        tree: &mut WidgetTree,
        labels: &[&str],
        fired: StdRc<StdCell<Option<usize>>>,
    ) -> WidgetId {
        let mut menu = MenuList::new();
        for (i, label) in labels.iter().enumerate() {
            let fired_for_this = fired.clone();
            menu = menu.item(
                MenuItem::new(lit!(*label)).on_activate_fn(move |_| fired_for_this.set(Some(i))),
            );
        }
        tree.add(menu)
    }

    /// A `WindowOps` that only counts `open_window`. Enough to tell "the
    /// row's handler reached the app's window sink" from the panic a
    /// standalone dispatch used to raise there.
    #[derive(Default)]
    struct CountingWindowOps {
        opened: usize,
    }

    impl teksilo_core::WindowOps for CountingWindowOps {
        fn open_window(
            &mut self,
            _config: teksilo_core::WindowConfig,
        ) -> teksilo_core::window::TeksiloWindowId {
            self.opened += 1;
            teksilo_core::window::TeksiloWindowId::new(1)
        }

        fn find_window(&self, _string_id: &str) -> Option<teksilo_core::window::TeksiloWindowId> {
            None
        }

        fn window_state(
            &self,
            _id: teksilo_core::window::TeksiloWindowId,
        ) -> Option<teksilo_core::window::WindowState> {
            None
        }

        fn windows(&self) -> Vec<teksilo_core::window::WindowState> {
            Vec::new()
        }

        fn focus_window(&mut self, _id: teksilo_core::window::TeksiloWindowId) {}

        fn close_window_by_id(&mut self, _id: teksilo_core::window::TeksiloWindowId) {}
    }

    #[test]
    fn keyboard_activation_keeps_the_window_ops() {
        // Enter on a menu row does not dispatch the click the pointer
        // would: it runs the row's keyboard activation inside the menu's own
        // key dispatch (a row that is not a `MenuItem` gets a queued
        // `EventContext::synthetic_click`, which the tree drains as a
        // *nested* dispatch). Draining that tap standalone once handed the
        // handler a context with no window sink, so a row that opened a
        // window by mouse panicked in `NoopWindowOps::open_window` by
        // keyboard (Skribisto's Help ▸ Help Topics). Same for Space, a
        // mnemonic and type-ahead.
        for activate in [Key::Enter, Key::Space] {
            let mut tree = light_tree();
            let menu = MenuList::new().item(MenuItem::new(lit!("Help")).on_activate_fn(|ctx| {
                ctx.open_window(teksilo_core::WindowConfig::new().title(lit!("Help")));
            }));
            let menu_id = tree.add(menu);
            tree.layout(SizeProposal::with_width(300.0));
            tree.focus(menu_id);

            let mut ops = CountingWindowOps::default();
            for key in [Key::ArrowDown, activate] {
                tree.dispatch_event_with_ops(
                    teksilo_core::event::WidgetEvent::KeyDown {
                        key,
                        modifiers: Modifiers::NONE,
                        text: None,
                    },
                    &mut ops,
                );
            }
            assert_eq!(
                ops.opened, 1,
                "{activate:?} on a menu row must reach the caller's WindowOps"
            );
        }
    }

    #[test]
    fn keyboard_activation_leaves_a_disabled_row_alone() {
        // Enter, Space and a mnemonic reach a `MenuItem` through its keyboard
        // route, not through a click the framework would refuse a disabled
        // row: the route has to refuse it itself.
        for key in [Key::Enter, Key::Space, Key::D] {
            let fired = StdRc::new(StdCell::new(false));
            let mut tree = light_tree();
            let fired_for_row = fired.clone();
            let menu_id = tree.add(
                MenuList::new().item(
                    MenuItem::new(lit!("&Delete"))
                        .enabled(false)
                        .on_activate_fn(move |_| fired_for_row.set(true)),
                ),
            );
            tree.layout(SizeProposal::with_width(300.0));
            tree.focus(menu_id);
            tree.press_key(Key::ArrowDown, Modifiers::NONE);
            tree.press_key(key, Modifiers::NONE);
            assert!(!fired.get(), "{key:?} must not run a disabled row");
        }
    }

    #[test]
    fn mnemonic_letter_activates_matching_item() {
        // Bare letter that matches an item's `&`-marker activates it
        // immediately (no Enter required).
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id =
            menu_with_activation_probe(&mut tree, &["&Save", "&Open", "&Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::O, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1), "Alt+O should activate 'Open'");
    }

    #[test]
    fn mnemonic_letter_is_case_insensitive() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["&Save", "&Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        // The `S` Key variant produces lowercase 's' via `to_char`,
        // matching the mnemonic 's' regardless of case.
        tree.press_key(Key::S, Modifiers::NONE);
        assert_eq!(fired.get(), Some(0));
    }

    #[test]
    fn mnemonic_does_not_fire_with_ctrl_modifier() {
        // Ctrl+S is an accelerator chord, not a menu mnemonic. The
        // dispatcher should leave it alone so the Shortcut/Action
        // pipeline can handle it instead.
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["&Save"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::S, Modifiers::CTRL);
        assert_eq!(fired.get(), None);
    }

    #[test]
    fn mnemonic_fires_with_shift_modifier() {
        // Windows / GNOME convention: bare letter activation works
        // regardless of the Shift state (Shift-Lock users would
        // otherwise be locked out of mnemonic activation). Only
        // Ctrl / Alt / Cmd disqualify the keystroke from in-menu
        // activation.
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["&Save", "&Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::S, Modifiers::SHIFT);
        assert_eq!(fired.get(), Some(0));
    }

    /// [`menu_with_activation_probe`] with a per-entry static visibility
    /// gate. The type-ahead timeout is zeroed so each keystroke starts a
    /// fresh prefix — these tests probe several letters in a row and
    /// aren't about buffer accumulation.
    fn menu_with_gated_probe(
        tree: &mut WidgetTree,
        entries: &[(&str, bool)],
        fired: StdRc<StdCell<Option<usize>>>,
    ) -> WidgetId {
        let mut menu = MenuList::new().type_ahead_timeout(Duration::ZERO);
        for (i, (label, visible)) in entries.iter().enumerate() {
            let fired_for_this = fired.clone();
            menu = menu.item_when(
                MenuItem::new(lit!(*label)).on_activate_fn(move |_| fired_for_this.set(Some(i))),
                *visible,
            );
        }
        tree.add(menu)
    }

    #[test]
    fn mnemonic_ignores_a_hidden_item() {
        // A row collapsed by `item_when(.., false)` must not be reachable
        // by its mnemonic — the same visibility gate the arrow / Home /
        // End / Enter branches already apply.
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_gated_probe(
            &mut tree,
            &[("&Save", false), ("&Quit", true)],
            fired.clone(),
        );
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::S, Modifiers::NONE);
        assert_eq!(fired.get(), None, "hidden 'Save' must not activate");
        tree.press_key(Key::Q, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1), "visible 'Quit' still activates");
    }

    #[test]
    fn mnemonic_resolves_to_the_visible_claimant() {
        // Two mutually-exclusive rows may share a letter — the `item_when`
        // pattern behind a Toolbar overflow menu's inline/collapsed twins.
        // Whichever is visible when the letter is pressed wins.
        for visible_idx in [0usize, 1] {
            let fired = StdRc::new(StdCell::new(None));
            let mut tree = light_tree();
            let mut entries = [("&Stop", false), ("&Start", false)];
            entries[visible_idx].1 = true;
            let menu_id = menu_with_gated_probe(&mut tree, &entries, fired.clone());
            tree.layout(SizeProposal::with_width(300.0));
            tree.focus(menu_id);
            tree.press_key(Key::S, Modifiers::NONE);
            assert_eq!(
                fired.get(),
                Some(visible_idx),
                "'s' should reach the visible claimant"
            );
        }
    }

    #[test]
    fn type_ahead_ignores_a_hidden_item() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_gated_probe(
            &mut tree,
            &[("Save", false), ("Open", true), ("Quit", true)],
            fired.clone(),
        );
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        // "s" matches only the hidden row, so nothing takes focus and
        // the following Enter has nothing to activate.
        tree.press_key(Key::S, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), None);
        // A visible row is still reachable.
        tree.press_key(Key::O, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1));
    }

    #[test]
    fn type_ahead_fires_with_shift_modifier() {
        // Same Shift-tolerance applies to type-ahead navigation.
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id =
            menu_with_activation_probe(&mut tree, &["Save", "Open", "Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::O, Modifiers::SHIFT);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1));
    }

    #[test]
    fn type_ahead_first_letter_focuses_and_enter_activates() {
        // No `&`-markers — letters drive type-ahead, not mnemonics.
        // Pressing 'o' focuses the matching item; Enter activates it.
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id =
            menu_with_activation_probe(&mut tree, &["Save", "Open", "Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::O, Modifiers::NONE);
        // Type-ahead only focuses; nothing fired yet.
        assert_eq!(fired.get(), None);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1));
    }

    #[test]
    fn type_ahead_extends_prefix_within_timeout() {
        // Typing 'q' then 'u' selects "Quit" (only item starting with "qu").
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(
            &mut tree,
            &["Save", "Open", "Quack", "Quit"],
            fired.clone(),
        );
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::Q, Modifiers::NONE);
        // 'q' alone matches "Quack" first (start+1 wrap → Save..Quack).
        tree.press_key(Key::U, Modifiers::NONE);
        // 'qu' still matches "Quack" — but the search starts from the
        // currently focused item ("Quack"), and from current+1 wraps
        // around to "Quit", which also starts with "qu". So Quit wins.
        tree.press_key(Key::I, Modifiers::NONE);
        // 'qui' — only "Quit" matches.
        tree.press_key(Key::T, Modifiers::NONE);
        // 'quit' — still "Quit".
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(3));
    }

    #[test]
    fn type_ahead_zero_timeout_treats_each_key_independently() {
        // With `type_ahead_timeout(Duration::ZERO)`, every keypress
        // clears the buffer first, so the search always restarts from
        // a single-character prefix.
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = {
            let mut menu = MenuList::new().type_ahead_timeout(Duration::ZERO);
            for (i, label) in ["Save", "Open", "Quit"].iter().enumerate() {
                let fired_for_this = fired.clone();
                menu = menu.item(
                    MenuItem::new(lit!(*label))
                        .on_activate_fn(move |_| fired_for_this.set(Some(i))),
                );
            }
            tree.add(menu)
        };
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::S, Modifiers::NONE);
        tree.press_key(Key::Q, Modifiers::NONE);
        // 'q' wins the most recent search; Enter activates Quit.
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(2));
    }

    #[test]
    fn page_keys_step_a_long_menu_and_clamp_at_the_ends() {
        // A language list or a recents list is long enough for the arrows to
        // be tedious; these were the one list chord `MenuList` did not answer.
        let labels: Vec<String> = (0..25).map(|i| format!("Item {i}")).collect();
        let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &refs, fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);

        // With no focus yet, PageDown enters at the top.
        tree.press_key(Key::PageDown, Modifiers::NONE);
        tree.press_key(Key::PageDown, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(10), "one page in from the first row");

        fired.set(None);
        tree.press_key(Key::PageDown, Modifiers::NONE);
        tree.press_key(Key::PageDown, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(24), "and it clamps at the last row");

        fired.set(None);
        tree.press_key(Key::PageUp, Modifiers::NONE);
        tree.press_key(Key::PageUp, Modifiers::NONE);
        tree.press_key(Key::PageUp, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(0), "and at the first");
    }

    #[test]
    fn home_focuses_first_item() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id =
            menu_with_activation_probe(&mut tree, &["Save", "Open", "Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        // Navigate down twice to land on index 2, then Home → index 0.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.press_key(Key::Home, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(0));
    }

    #[test]
    fn end_focuses_last_item() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id =
            menu_with_activation_probe(&mut tree, &["Save", "Open", "Quit"], fired.clone());
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::End, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(2));
    }

    #[test]
    fn arrow_down_wraps_past_last() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["A", "B", "C"], fired.clone());
        tree.layout(SizeProposal::with_width(200.0));
        tree.focus(menu_id);
        for _ in 0..4 {
            tree.press_key(Key::ArrowDown, Modifiers::NONE);
        }
        // After 4 downs from "no focus", focus lands on index 0 (wrap).
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(0));
    }

    #[test]
    fn arrow_up_wraps_to_last() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["A", "B", "C"], fired.clone());
        tree.layout(SizeProposal::with_width(200.0));
        tree.focus(menu_id);
        tree.press_key(Key::ArrowUp, Modifiers::NONE);
        // From "no focus" (treated as index 0), Up wraps to last (index 2).
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(2));
    }

    fn menu_with_submenu(tree: &mut WidgetTree) -> WidgetId {
        // Index 0 is a submenu trigger; index 1 is a plain item.
        let menu = MenuList::new()
            .item(MenuItem::submenu(lit!("More"), || {
                Box::new(MenuList::new().item(MenuItem::new(lit!("Child"))))
            }))
            .item(MenuItem::new(lit!("Plain")));
        tree.add(menu)
    }

    #[test]
    fn submenu_opens_on_arrow_right_under_ltr() {
        let mut tree = light_tree();
        let menu_id = menu_with_submenu(&mut tree);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::ArrowDown, Modifiers::NONE); // focus submenu item (idx 0)
        assert!(tree.active_overlays().is_empty());

        // Inline-back arrow under LTR (ArrowLeft) does not open.
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());

        // Inline-forward arrow (ArrowRight) opens the submenu.
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn submenu_opens_on_arrow_left_under_rtl() {
        let mut tree = light_tree();
        tree.set_layout_direction(teksilo_core::environment::LayoutDirection::RightToLeft);
        let menu_id = menu_with_submenu(&mut tree);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::ArrowDown, Modifiers::NONE); // focus submenu item (idx 0)
        assert!(tree.active_overlays().is_empty());

        // Under RTL, ArrowRight is the inline-back key — must NOT open.
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());

        // ArrowLeft is inline-forward under RTL — opens the submenu.
        tree.press_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn type_ahead_no_match_does_not_change_focus() {
        // Typing a letter that doesn't prefix any label should leave
        // focus untouched — Enter then activates whatever was focused
        // before (or nothing).
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["Save", "Open"], fired.clone());
        tree.layout(SizeProposal::with_width(200.0));
        tree.focus(menu_id);
        // Focus the first item explicitly.
        tree.press_key(Key::Home, Modifiers::NONE);
        // Type a no-match letter.
        tree.press_key(Key::Z, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        // Save (index 0) should still fire.
        assert_eq!(fired.get(), Some(0));
    }

    #[test]
    fn mnemonic_beats_type_ahead_when_both_match() {
        // If a label like "&Open" is set up, pressing 'o' fires the
        // mnemonic directly, even though type-ahead would also match
        // "Open".
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = menu_with_activation_probe(&mut tree, &["&Save", "&Open"], fired.clone());
        tree.layout(SizeProposal::with_width(200.0));
        tree.focus(menu_id);
        tree.press_key(Key::O, Modifiers::NONE);
        // Mnemonic fires immediately — no Enter needed.
        assert_eq!(fired.get(), Some(1));
    }

    #[test]
    fn separator_does_not_interfere_with_navigation() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = {
            let mut menu = MenuList::new();
            for (i, label) in ["Save", "Open", "Quit"].iter().enumerate() {
                let fired_for_this = fired.clone();
                menu = menu.item(
                    MenuItem::new(lit!(*label))
                        .on_activate_fn(move |_| fired_for_this.set(Some(i))),
                );
                if i == 0 {
                    menu = menu.separator();
                }
            }
            tree.add(menu)
        };
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        // Type-ahead should still find "Open" — separator skipped.
        tree.press_key(Key::O, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1));
    }

    #[test]
    fn header_does_not_interfere_with_navigation() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let menu_id = {
            let mut menu = MenuList::new();
            for (i, label) in ["Save", "Open", "Quit"].iter().enumerate() {
                let fired_for_this = fired.clone();
                menu = menu.item(
                    MenuItem::new(lit!(*label))
                        .on_activate_fn(move |_| fired_for_this.set(Some(i))),
                );
                if i == 0 {
                    // A non-navigable section caption between item 0 and item 1.
                    menu = menu.header(crate::GroupHeader::new(lit!("Recent")));
                }
            }
            tree.add(menu)
        };
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        // Type-ahead resolves "Open" at item index 1 — the header occupies no
        // slot in the item/label index space, exactly like a separator.
        tree.press_key(Key::O, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(fired.get(), Some(1));
    }

    // Keeps the test module's `Signal` import referenced; nothing else in
    // these tests names the type at module scope.
    #[allow(dead_code)]
    fn _ignore_unused() {
        let _: Option<Signal<bool>> = None;
    }

    #[derive(Debug)]
    struct FocusableLeaf;
    impl Widget for FocusableLeaf {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.apply_self_handlers(
                teksilo_core::widget_builder::HandlerSet::new().focusable(true),
            );
            vec![]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            proposal.resolve(12.0, 12.0).into()
        }
    }

    /// Opening a submenu must not be mistaken for leaving the parent menu.
    ///
    /// A submenu's content is `add_detached_boxed`, so it is never an arena
    /// descendant of the menu that owns it — the only thing relating the two is
    /// the overlay manager's `parent_overlay` graph. A focus-out rule that
    /// asked the arena instead would close the parent the instant its own
    /// submenu opened.
    #[test]
    fn opening_a_submenu_keeps_the_parent_menu_open() {
        let mut tree = light_tree();
        let menu_id = menu_with_submenu(&mut tree);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the submenu is up and the parent menu is untouched"
        );
        assert!(
            tree.is_active(menu_id),
            "the parent MenuList must not have been dormanted"
        );
    }

    /// Tab out of a submenu closes the whole cascade, not one level.
    ///
    /// APG is unqualified and plural about it: Tab "closes all menus and
    /// submenus". Walking up `parent_overlay` and dismissing the outermost
    /// level gets that for free — `dismiss_immediate` already cascades back
    /// down to every descendant.
    #[test]
    fn tab_out_of_a_submenu_closes_the_whole_cascade() {
        let mut tree = light_tree();
        let menu_id = menu_with_submenu(&mut tree);
        let after = tree.add(FocusableLeaf);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "precondition: submenu open"
        );

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(after));
        assert!(
            tree.active_overlays().is_empty(),
            "one Tab must leave no menu behind"
        );
    }

    // --- Decorated rows: a `MenuItem` carrying a builder method ---
    //
    // Any `WidgetBuilder` call (`.context_menu`, `.focusable`, …) wraps the
    // item in a `WidgetWithHandlers<MenuItem>`. Every `MenuList` feature that
    // reads the item's concrete type has to keep working through that wrapper,
    // or a row silently degrades with no error anywhere.

    /// Wrap a `MenuItem` the way a caller that needs a per-row context menu
    /// does — the shape that used to de-register the row from `MenuList`.
    fn decorated(item: MenuItem) -> impl Widget + 'static {
        item.context_menu(|_pos, _ctx| None)
    }

    #[test]
    fn a_decorated_item_keeps_its_mnemonic() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let mut menu = MenuList::new();
        for (i, label) in ["&Save", "&Open", "&Quit"].iter().enumerate() {
            let fired_for_this = fired.clone();
            menu = menu.item(decorated(
                MenuItem::new(lit!(*label)).on_activate_fn(move |_| fired_for_this.set(Some(i))),
            ));
        }
        let menu_id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);

        tree.press_key(Key::O, Modifiers::NONE);
        assert_eq!(
            fired.get(),
            Some(1),
            "the mnemonic is read off the MenuItem; decorating it must not hide it"
        );
    }

    #[test]
    fn a_decorated_item_keeps_its_type_ahead_label() {
        let fired = StdRc::new(StdCell::new(None));
        let mut tree = light_tree();
        let mut menu = MenuList::new();
        // No `&` markers here, so only the type-ahead path can reach a row.
        for (i, label) in ["Alpha", "Beta", "Gamma"].iter().enumerate() {
            let fired_for_this = fired.clone();
            menu = menu.item(decorated(
                MenuItem::new(lit!(*label)).on_activate_fn(move |_| fired_for_this.set(Some(i))),
            ));
        }
        let menu_id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);

        tree.press_key(Key::G, Modifiers::NONE);
        tree.press_key(Key::Enter, Modifiers::NONE);
        assert_eq!(
            fired.get(),
            Some(2),
            "type-ahead reads the label off the MenuItem, through any wrapper"
        );
    }

    #[test]
    fn a_decorated_submenu_trigger_still_opens_on_the_inline_arrow() {
        let mut tree = light_tree();
        let menu = MenuList::new()
            .item(decorated(MenuItem::submenu(lit!("More"), || {
                Box::new(MenuList::new().item(MenuItem::new(lit!("Child"))))
            })))
            .item(MenuItem::new(lit!("Plain")));
        let menu_id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);

        tree.press_key(Key::ArrowDown, Modifiers::NONE); // highlight the trigger
        assert!(tree.active_overlays().is_empty(), "precondition: closed");

        tree.press_key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "the submenu flag is read off the MenuItem, through any wrapper"
        );
    }

    // --- Keyboard navigation scrolls a capped menu ---

    /// A menu row that records the absolute bounds it was last laid out at.
    ///
    /// The accessibility tree is not a usable probe here: its node bounds are
    /// captured when the node is emitted and a pure scroll does not re-emit
    /// them, so a stale rect reads back as "nothing moved" whether or not the
    /// scroll happened. `place_children` is the layout's own answer.
    #[derive(Debug)]
    struct ProbeRow {
        seen: StdRc<Cell<Rect>>,
    }

    impl Widget for ProbeRow {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            proposal.resolve(200.0, 24.0).into()
        }

        fn place_children(
            &self,
            bounds: Rect,
            _proposal: SizeProposal,
            _children: &mut [WidgetPlacement],
            _ctx: &LayoutContext,
        ) {
            self.seen.set(bounds);
        }
    }

    #[test]
    fn keyboard_navigation_scrolls_a_capped_menu_to_the_highlight() {
        // Past `max_visible_items` the panel is a `ScrollArea`, and arrow / End
        // navigation moves `focused_index` rather than real tree focus — so the
        // framework's own focus-follow scroll never runs. Without an explicit
        // reveal the highlight walks straight out of the viewport and the menu
        // looks frozen from the fifth row down.
        let seen = StdRc::new(Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)));
        let mut tree = light_tree();
        let mut menu = MenuList::new().max_visible_items(4);
        for i in 0..19 {
            menu = menu.item(MenuItem::new(lit!(format!("Entry {i}"))));
        }
        // The last row is the probe, so `End` lands on it.
        menu = menu.item(ProbeRow { seen: seen.clone() });
        let menu_id = tree.add(menu);
        tree.layout(SizeProposal::with_width(300.0));
        tree.focus(menu_id);

        let panel = tree.bounds(menu_id);
        let before = seen.get();
        assert!(
            before.y > panel.bottom(),
            "precondition: the last row starts below the capped panel \
             (row y={}, panel bottom={})",
            before.y,
            panel.bottom()
        );

        tree.press_key(Key::End, Modifiers::NONE);
        // The reveal is queued from the handler and applied by the enclosing
        // ScrollArea; bounds only move on the next layout pass.
        tree.layout(SizeProposal::with_width(300.0));

        let after = seen.get();
        assert!(
            after.y >= panel.y - 0.5 && after.bottom() <= panel.bottom() + 0.5,
            "End must scroll the last row into the panel, got {}..{} for a panel of {}..{}",
            after.y,
            after.bottom(),
            panel.y,
            panel.bottom()
        );
    }
}
