// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! MenuList — a themed vertical menu container with keyboard navigation.
//!
//! `MenuList` is the dropdown panel used by `MenuBar`, `MenuContext`, and
//! popover-style menus. It provides a themed surface (background, rounded
//! border, drop shadow) and owns the full keyboard navigation stack:
//! ArrowUp/Down moves focus, Enter activates, Escape bubbles to the
//! enclosing overlay host, Home and End jump to the first/last enabled item.
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
//! **Safe-triangle hover gate.** When a submenu item opens its child overlay,
//! `MenuList` stamps a shared anchor so sibling items can skip their
//! hover-switch while the cursor travels diagonally toward the submenu.
//!
//! ## Accessibility
//!
//! `Role::Menu`; each row is `Role::MenuItem` / `Role::MenuItemCheckBox` /
//! `Role::MenuItemRadio` as declared by the item. Radio items in the same
//! list auto-group via `push_to_radio_group` so AT announces "2 of 3".
//!
//! ```rust
//! # use bastyde_widgets::{MenuList, MenuItem};
//! # use bastyde_i18n::lit;
//! # use bastyde_core::Intent;
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

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::OverlayPlacement;
use bastyde_core::signal::Signal;
use bastyde_core::styles::{PopoverStyleConfig, PopoverVariant};
use bastyde_core::widget::{
    EventContext, LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::SurfaceRole;

use crate::primitives::{MaxSize, Padding, RectWidget, VStack, ZStack};
use crate::scroll_area::ScrollArea;

/// Marker for whether a pending item is a menu item or a separator.
enum MenuEntry {
    /// A menu item with an optional reactive visibility gate. When the gate
    /// is `Some(false)` the item's row collapses to zero height (no gap) and
    /// is skipped by keyboard navigation — the conditionally-shown menu row.
    Item {
        pending: PendingChild,
        visible: Option<bastyde_core::signal::Prop<bool>>,
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

/// A 1 dp horizontal divider line between groups of menu items.
#[derive(Debug)]
pub struct MenuSeparator;

impl Widget for MenuSeparator {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let _ = ctx;
        let width = proposal.width.unwrap_or(0.0);
        Size::new(
            width,
            crate::styles::recipe_menu_item_style::MENU_SEPARATOR_HEIGHT,
        )
        .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut bastyde_canvas::Canvas, ctx: &PaintContext) {
        // Int UI menu separator: a flush-edge 1 dp line in `divider` color,
        // vertically centered in the `separator_height` (9 dp) slot — that
        // slot provides 4 dp top/bottom breathing room around the line.
        let color = ctx.theme.colors.divider;
        let thickness = ctx.theme.shape.border_width;
        let y = bounds.y + (bounds.height - thickness) * 0.5;
        canvas.fill_rect(Rect::new(bounds.x, y, bounds.width, thickness), color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Splitter);
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

        let zstack = ZStack::new().add_child(bg_id).add_child(self.item_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
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
    item_visibility: Vec<Option<bastyde_core::signal::Prop<bool>>>,
    /// Whether each item (by index into item_widget_ids) is a submenu trigger.
    submenu_flags: Vec<bool>,
    /// When set and the item count (counting every entry — items *and*
    /// separators — against the row count, not pixels) exceeds the
    /// limit, the content column is wrapped in a `ScrollArea` and the
    /// panel height is capped to `n * item_height`. `None` (default)
    /// lets the menu grow with its content.
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
}

/// Per-MenuList shared state for the safe-triangle submenu hover gate.
/// Set by a submenu-trigger MenuItem when its submenu opens, cleared
/// when the submenu closes. Consulted by sibling MenuItems before
/// they fire `dismiss_child_overlays` / `show_overlay_after_with_focus`
/// — if the cursor is currently inside the triangle apex'd at
/// `anchor` and based at the open submenu's near edge, the sibling's
/// hover-switch is skipped so the user can travel diagonally to the
/// submenu without losing focus on the way.
///
/// `pub` for cross-crate access (MenuItem reads & writes it through a
/// shared `Rc`-handle) but in practice only `MenuList`'s scope wires
/// it up.
#[derive(Debug, Default)]
pub(crate) struct SafeTriangleState {
    /// The currently-open submenu's root content widget id, or
    /// `None` when no submenu is open. Looked up against the
    /// per-dispatch overlay-bounds snapshot to recover the screen
    /// rect.
    pub submenu_content_id: Option<WidgetId>,
    /// Pointer position at the moment the submenu opened — the
    /// triangle apex. `None` when no submenu is open.
    pub anchor: Option<bastyde_canvas::Point>,
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
        }
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
        // Detect submenu items via Any downcast before boxing
        let is_submenu = (&widget as &dyn std::any::Any)
            .downcast_ref::<crate::menu_item::MenuItem>()
            .is_some_and(|mi| mi.is_submenu());
        self.submenu_flags.push(is_submenu);
        self.entries.push(MenuEntry::Item {
            pending: PendingChild::Deferred(Box::new(widget)),
            visible: None,
        });
        self
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
        visible: impl Into<bastyde_core::signal::Prop<bool>>,
    ) -> Self {
        self.item_boxed_when(Box::new(widget), visible)
    }

    /// [`item_when`](Self::item_when) for an already-boxed widget — used when
    /// the row type is decided at runtime (e.g. a menu row that is either a
    /// `MenuItem` or an embedded control).
    pub fn item_boxed_when(
        mut self,
        widget: Box<dyn Widget>,
        visible: impl Into<bastyde_core::signal::Prop<bool>>,
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
        self.entries
            .push(MenuEntry::Header(PendingChild::Deferred(Box::new(widget))));
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
        use crate::styles::recipe_menu_item_style as menu;
        let _theme_signal = ctx.theme_signal();

        // Keyboard-focused item index (shared with the key handler and wrappers).
        // The binding registry propagates repaints when this changes.
        let focused_index: Signal<Option<usize>> = ctx.signal(None);

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
                    let (item_id, radio_buf, item_label, item_mnemonic) = match pending {
                        PendingChild::Id(id) => (id, None, None, None),
                        PendingChild::Deferred(mut w) => {
                            // Single downcast pass: read the radio
                            // selection signal AND the parsed mnemonic
                            // AND install the safe-triangle shared
                            // state, before moving the box into the
                            // arena.
                            let (radio_buf, item_label, item_mnemonic) = w
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
                                    (radio, label, mnemonic)
                                })
                                .unwrap_or((None, None, None));
                            (ctx.add_boxed(w), radio_buf, item_label, item_mnemonic)
                        }
                    };
                    self.item_widget_ids.push(item_id);
                    self.item_visibility.push(visible.clone());
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
                    // at the real, clickable item for `synthetic_click`.
                    let wrapper_id = ctx.add(KeyboardHighlightWrapper {
                        item_id,
                        index: item_counter,
                        focused_index: focused_index.clone(),
                        root_child_id: None,
                    });
                    if let Some(vis) = visible {
                        ctx.visible_when(wrapper_id, vis);
                    }
                    vstack = vstack.add_child(wrapper_id);
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
                    vstack = vstack.add_child(header_id);
                }
            }
        }

        // Fill each radio group's id list now that every item has a
        // WidgetId. Each id is pushed exactly once.
        for (item_idx, buf) in pending_radio_pushes {
            buf.borrow_mut().push(self.item_widget_ids[item_idx]);
        }

        let vstack_id = ctx.add(vstack);

        let padding = Padding::uniform(4.0).child_id(vstack_id);
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
                let max_height = cap as f32 * menu::MENU_ITEM_HEIGHT + 8.0;
                // Cap the HEIGHT only. `preferred_size(0.0, ..)` would set the preferred
                // *width* to zero — and a popover proposes an unconstrained width (it
                // hugs its content), so the zero was taken literally: the menu collapsed
                // to its minimum width and every row was clipped to a middle slice.
                let scrollable = ScrollArea::from_id(padding_id).preferred_height(max_height);
                let scrollable_id = ctx.add(scrollable);
                ctx.add(MaxSize::height(max_height).child_id(scrollable_id))
            }
            _ => padding_id,
        };

        // Themed surface — routed through `PopoverStyle` (the
        // `Menu`-flavoured variant), so the menu panel's background,
        // border, corner radius, and drop shadow are all owned by the
        // active popover style instead of a hand-rolled bg `RectWidget`
        // + `MenuList::paint`. The full-halo vs trigger-attached
        // shadow choice is derived from `attached_side`.
        let popover_style: bastyde_core::styles::SharedPopoverStyle = ctx
            .theme()
            .style_slots
            .popover
            .clone()
            .unwrap_or_else(|| Rc::new(crate::styles::RecipePopoverStyle::default()));
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
                            EventResponse::Handled
                        }
                        Key::Home => {
                            let Some(&first) = visible_indices.first() else {
                                return EventResponse::Ignored;
                            };
                            focused_index.set(Some(first));
                            ctx.show_highlight_tooltip(item_ids[first]);
                            EventResponse::Handled
                        }
                        Key::End => {
                            let Some(&last) = visible_indices.last() else {
                                return EventResponse::Ignored;
                            };
                            focused_index.set(Some(last));
                            ctx.show_highlight_tooltip(item_ids[last]);
                            EventResponse::Handled
                        }
                        Key::Enter | Key::Space => {
                            // Activate the focused item via synthetic click —
                            // but only if it is currently visible.
                            if let Some(idx) = focused_index.get()
                                && visible_indices.contains(&idx)
                                && idx < item_ids.len()
                            {
                                ctx.synthetic_click(item_ids[idx]);
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
                                ctx.synthetic_click(item_ids[idx]);
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
                            // global Shortcut/Action pipeline.
                            if modifiers.ctrl() || modifiers.alt() || modifiers.super_key() {
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
                                ctx.synthetic_click(item_ids[idx]);
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
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::Menu);
    }

    fn children(&self) -> Vec<WidgetId> {
        match self.root_child_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_item::MenuItem;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
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
        use bastyde_core::signal::Signal;
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

    use bastyde_core::event::{Key, Modifiers};
    use bastyde_core::signal::Signal;
    use std::cell::Cell as StdCell;
    use std::rc::Rc as StdRc;

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
        tree.set_layout_direction(bastyde_core::environment::LayoutDirection::RightToLeft);
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

    // Silence the unused-variable warning on the unused `list_id`
    // binding inside `menu_label`-style tests above, since each test
    // uses its locals.
    #[allow(dead_code)]
    fn _ignore_unused() {
        let _: Option<Signal<bool>> = None;
    }
}
