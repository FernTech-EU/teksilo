// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `Toolbar` — a command bar with automatic **overflow**.
//!
//! Excess actions collapse into a trailing chevron (`⌄`) that opens a popover
//! menu, mirroring Qt's `QToolBar` extension button, macOS `NSToolbar`'s
//! overflow menu, and WinUI `CommandBar`. Synthesized API:
//!
//! - **Actions** ([`ToolbarAction`]) — a command with a label, optional icon,
//!   tooltip, enabled state, optional toggle (checkable), an **overflow
//!   priority** (NSToolbar: lowest priority collapses first), and an
//!   **`always_overflow`** flag (WinUI secondary commands). Each action has a
//!   toolbar form (a `Button`) and a menu form (a `MenuItem`), so it renders
//!   correctly whether inline or in the overflow menu.
//! - **Pinned widgets** ([`ToolbarItem::custom`]) — arbitrary widgets (a search
//!   field, a `SegmentedControl`) that never collapse.
//! - **Collapsible widgets** — an arbitrary widget that *does* overflow, by
//!   supplying an overflow representation (NSToolbar `menuFormRepresentation` /
//!   Qt `QWidgetAction`): a **menu row** ([`ToolbarAction`]) via
//!   [`ToolbarItem::custom(w).overflow_as(action)`](ToolbarItem::overflow_as)
//!   (or [`ToolbarOverflow`] + [`ToolbarItem::collapsible`]; an icon-only
//!   control reuses its icon as the menu glyph), or a **live embedded widget**
//!   via [`ToolbarItem::custom(w).overflow_widget(f)`](ToolbarItem::overflow_widget)
//!   (the factory rebuilds the control — e.g. a `ComboBox` bound to the same
//!   signal — inside the menu so it stays usable while collapsed). When the bar
//!   is tight the inline widget is hidden and its overflow form appears in the
//!   menu.
//! - **Separators** and **flexible space** (NSToolbar `flexibleSpace`).
//! - **Display mode** (icon+text / icon-only / text-only) and **orientation**.
//!
//! Overflow is computed every layout pass from each item's intrinsic size
//! (measured even while collapsed, via
//! [`LayoutContext::measure_intrinsic`](bastyde_core::widget::LayoutContext::measure_intrinsic)),
//! so items reappear correctly as the bar widens — no stale-width glitches.
//!
//! The chevron's drop-down is a real [`MenuList`] whose rows are gated by
//! [`MenuList::item_when`],
//! so it sizes compactly to the currently-collapsed rows, carries standard
//! menu chrome, takes focus when opened, and supports arrow / `Home` / `End` /
//! `Enter` keyboard navigation (skipping the hidden rows).
//!
//! **Accessibility (ARIA toolbar pattern).** The bar emits `Role::Toolbar`
//! with its orientation and name. It is a single Tab stop with **roving
//! tab-index**: arrow keys move focus among the visible controls (and the
//! chevron), `Home`/`End` jump to the ends. The chevron announces
//! `HasPopup::Menu` and its expanded state; overflowed actions are dormant
//! (absent from the AT tree), represented instead by their menu items — so no
//! action is announced twice. Toggle actions carry `Toggled`.
//!
//! ```ignore
//! // on_activate requires an EventContext — use ignore.
//! use bastyde_widgets::toolbar::{Toolbar, ToolbarAction, ToolbarItem};
//! use bastyde_i18n::lit;
//! let _bar = Toolbar::new()
//!     .action(ToolbarAction::new(lit!("Save")).on_activate(|ctx| { /* ... */ }))
//!     .action(ToolbarAction::new(lit!("Undo")).priority(-1))
//!     .item(ToolbarItem::flexible_space());
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Point, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::HasPopup;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::OverlayPlacement;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;

use crate::Panel;
use crate::button::{Button, IconLocation};
use crate::icon_button::IconButton;
use crate::menu_item::MenuItem;
use crate::menu_list::MenuList;
use crate::popover_widget::PopoverIconButton;
use crate::primitives::icon_widget::IconWidget;
use crate::primitives::{Divider, HStack, Spacer, VStack};

/// Toolbar design tokens.
pub const TOOLBAR_HEIGHT_DEFAULT: f32 = 40.0;
pub const TOOLBAR_SPACING: f32 = 4.0;
/// Width/height reserved for the overflow chevron when it is shown.
const CHEVRON_EXTENT: f32 = 30.0;
const ICON_SIZE: f32 = 16.0;

/// How toolbar actions render their label and icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolbarDisplayMode {
    /// Icon (if any) beside the label. The default.
    #[default]
    IconAndText,
    /// Icon only; the label becomes the accessible name + tooltip.
    IconOnly,
    /// Label only; the icon is dropped.
    TextOnly,
}

/// Layout axis of the toolbar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolbarOrientation {
    /// Items flow left-to-right (default).
    #[default]
    Horizontal,
    /// Items flow top-to-bottom.
    Vertical,
}

type IconFactory = Rc<dyn Fn() -> IconWidget>;

/// A toolbar command: a label plus optional icon/tooltip/toggle, an activation
/// handler, an overflow priority, and an `always_overflow` flag. Renders as a
/// `Button` inline and as a `MenuItem` in the overflow menu.
#[derive(Clone)]
pub struct ToolbarAction {
    label: LocalizedString,
    icon: Option<IconFactory>,
    /// Plain-text tooltip shown after a hover delay.
    /// Mutually exclusive with `rich_tooltip_source` — every tooltip
    /// setter clears the other so last-call wins.
    tooltip: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    /// Mutually exclusive with `tooltip` — every tooltip setter clears
    /// the other so last-call wins. Boxed because the inline
    /// `TooltipContent` payload is large and rarely set, and
    /// `ToolbarAction` is embedded by value in `ToolbarItemKind` /
    /// `OverflowMenuForm` (keeps those enums compact).
    rich_tooltip_source: Option<Box<crate::tooltip::RichTooltipSource>>,
    /// Optional composite tooltip body, stored as a factory because
    /// `ToolbarAction` is `Clone` and `Box<dyn Widget>` is not — the
    /// factory (`Rc`, which is `Clone`) is invoked once per `make_button`
    /// to produce a fresh body. Mutually exclusive with the other two.
    composite_tooltip_factory: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    enabled: bool,
    on_activate: Rc<dyn Fn(&mut EventContext)>,
    toggle: Option<Signal<bool>>,
    priority: i32,
    always_overflow: bool,
}

impl ToolbarAction {
    /// A new action with the given (translatable) label and a no-op handler.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            tooltip: None,
            rich_tooltip_source: None,
            composite_tooltip_factory: None,
            enabled: true,
            on_activate: Rc::new(|_| {}),
            toggle: None,
            priority: 0,
            always_overflow: false,
        }
    }

    /// Icon factory — called to build the icon for both the inline button and
    /// the overflow menu item (`IconWidget` isn't `Clone`).
    pub fn icon(mut self, factory: impl Fn() -> IconWidget + 'static) -> Self {
        self.icon = Some(Rc::new(factory));
        self
    }

    /// Plain-text tooltip shown after a hover delay (also the AT name
    /// supplement in `IconOnly` mode). Overrides any previously set rich
    /// tooltip — every setter clears the other so last-call wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_factory = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip registry.
    /// The `key` is looked up via
    /// [`TooltipRegistry`](crate::tooltip::TooltipRegistry) at build
    /// time; the resolved body text supports inline markup
    /// (`[label](url)`, `*italic*`, `**bold**`) and the entry's
    /// shortcut / "more" fields are rendered automatically.
    ///
    /// Overrides any previously set plain `.tooltip(...)` — every setter
    /// clears the other so last-call wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source =
            Some(Box::new(crate::tooltip::RichTooltipSource::Key(key.into())));
        self.tooltip = None;
        self.composite_tooltip_factory = None;
        self
    }

    /// Attach a rich tooltip driven by inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) — for
    /// one-off tooltips that aren't worth registering in the central
    /// catalog. Overrides any previously set plain `.tooltip(...)`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(Box::new(crate::tooltip::RichTooltipSource::Content(
            content,
        )));
        self.tooltip = None;
        self.composite_tooltip_factory = None;
        self
    }

    /// Attach a composite tooltip whose body is built by `factory` — an
    /// arbitrary widget tree (tabbed sections, charts, conditional rows).
    /// Because `ToolbarAction` is `Clone`, the body is supplied as a
    /// factory closure (not a `Box<dyn Widget>` instance, which is not
    /// `Clone`); the closure is invoked to produce a fresh body for the
    /// inline button. Overrides any previously set tooltip — every setter
    /// clears the others so last-call wins.
    pub fn composite_tooltip(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.composite_tooltip_factory = Some(Rc::new(factory));
        self.tooltip = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Initial enabled state.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Activation handler (tap / Enter / Space / AT click / menu activate).
    pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate = Rc::new(f);
        self
    }

    /// Make this a checkable (toggle) action bound to `state`. Inline it reads
    /// as a pressed toggle button; in overflow as a checkmark menu item.
    pub fn toggle(mut self, state: Signal<bool>) -> Self {
        self.toggle = Some(state);
        self
    }

    /// Overflow priority — actions with the **lowest** priority collapse into
    /// the menu first (NSToolbar semantics). Default `0`.
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Always live in the overflow menu, never inline (WinUI secondary command).
    pub fn always_overflow(mut self) -> Self {
        self.always_overflow = true;
        self
    }

    /// Build this action's inline button.
    fn make_button(&self, display: ToolbarDisplayMode) -> Button {
        let mut btn = Button::new(self.label.clone()).enabled(self.enabled);
        if display != ToolbarDisplayMode::TextOnly {
            if let Some(ref f) = self.icon {
                let loc = if display == ToolbarDisplayMode::IconOnly {
                    IconLocation::IconOnly
                } else {
                    IconLocation::Leading
                };
                btn = btn.icon(f(), loc);
            }
        }
        // Forward tooltip (mutually-exclusive setters; at most one branch runs).
        if let Some(ref factory) = self.composite_tooltip_factory {
            btn = btn.composite_tooltip_boxed(factory());
        } else if let Some(ref source) = self.rich_tooltip_source {
            match (**source).clone() {
                crate::tooltip::RichTooltipSource::Key(key) => {
                    btn = btn.rich_tooltip(key);
                }
                crate::tooltip::RichTooltipSource::Content(content) => {
                    btn = btn.rich_tooltip_content(content);
                }
            }
        } else if let Some(ref tip) = self.tooltip {
            btn = btn.tooltip(tip.clone());
        }
        let act = self.on_activate.clone();
        if let Some(ref toggle) = self.toggle {
            let toggle = toggle.clone();
            btn = btn.on_activate_fn(move |ctx| {
                toggle.set(!toggle.get());
                act(ctx);
            });
        } else {
            btn = btn.on_activate_fn(move |ctx| act(ctx));
        }
        btn
    }

    /// Build this action's overflow row — a `MenuItem` that runs the action
    /// and closes the popover. Checkable actions show a check mark.
    fn make_menu_item(&self) -> MenuItem {
        let mut mi = MenuItem::new(self.label.clone()).enabled(self.enabled);
        if let Some(ref f) = self.icon {
            mi = mi.icon(f());
        }
        let act = self.on_activate.clone();
        if let Some(ref toggle) = self.toggle {
            mi = mi.bind_checked(toggle.clone());
            let toggle = toggle.clone();
            mi = mi.on_activate_fn(move |ctx| {
                toggle.set(!toggle.get());
                act(ctx);
                ctx.dismiss_self_overlay_chain();
            });
        } else {
            mi = mi.on_activate_fn(move |ctx| {
                act(ctx);
                ctx.dismiss_self_overlay_chain();
            });
        }
        mi
    }
}

/// A widget that knows how to represent itself in a `Toolbar`'s overflow menu
/// when it is collapsed (NSToolbar `menuFormRepresentation` / Qt
/// `QWidgetAction`). Implement this on a widget and add it with
/// [`ToolbarItem::collapsible`] to make it overflow into the chevron menu as
/// the returned [`ToolbarAction`] (a menu row), instead of staying pinned.
///
/// For widgets that are best represented in the menu *as themselves* (a
/// `ComboBox`, a slider) rather than as a one-shot menu row, use
/// [`ToolbarItem::overflow_widget`] instead — it embeds a live widget in the
/// menu.
pub trait ToolbarOverflow {
    /// The menu-form representation shown when this widget overflows.
    fn toolbar_menu_form(&self) -> ToolbarAction;
}

/// Factory that rebuilds a widget for the overflow menu (widgets aren't
/// `Clone`, and the menu builds its rows lazily).
type MenuWidgetFactory = Rc<dyn Fn() -> Box<dyn Widget>>;

/// What a collapsible toolbar item shows when it collapses into the overflow
/// (chevron) menu.
enum OverflowMenuForm {
    /// A standard menu row built from a [`ToolbarAction`] — label, optional
    /// icon (an icon-only inline control reuses its icon here as the menu
    /// item's leading glyph), and the action's activation. Used by actions
    /// and by custom widgets that opt in via
    /// [`ToolbarItem::overflow_as`] / [`ToolbarItem::collapsible`].
    Action(ToolbarAction),
    /// A live widget embedded directly in the menu — e.g. the same
    /// `ComboBox` (bound to the same signal) the inline slot shows, so the
    /// control stays fully usable while collapsed. Built by the factory when
    /// the menu is constructed. Opt in via [`ToolbarItem::overflow_widget`].
    Widget(MenuWidgetFactory),
}

/// Per-collapsible-item overflow metadata: the menu-form to show plus the
/// priority / `always_overflow` flags that drive [`compute_overflow`].
struct CollapsibleMeta {
    priority: i32,
    always_overflow: bool,
    form: OverflowMenuForm,
}

enum ToolbarItemKind {
    Action(ToolbarAction),
    /// Arbitrary widget. `menu_form == None` → pinned (never collapses);
    /// `Some(form)` → collapsible, shown as that menu row (or embedded widget)
    /// when overflowed.
    Custom {
        pending: PendingChild,
        menu_form: Option<OverflowMenuForm>,
    },
    Separator,
    FlexibleSpace,
}

/// One slot in a [`Toolbar`].
pub struct ToolbarItem {
    kind: ToolbarItemKind,
}

impl ToolbarItem {
    /// A collapsible command.
    pub fn action(action: ToolbarAction) -> Self {
        Self {
            kind: ToolbarItemKind::Action(action),
        }
    }

    /// A pinned arbitrary widget (never collapses) — e.g. a search field. Make
    /// it collapsible with [`overflow_as`](Self::overflow_as).
    pub fn custom(widget: impl Widget + 'static) -> Self {
        Self {
            kind: ToolbarItemKind::Custom {
                pending: PendingChild::Deferred(Box::new(widget)),
                menu_form: None,
            },
        }
    }

    /// A pinned arbitrary widget by pre-registered id.
    pub fn custom_id(id: WidgetId) -> Self {
        Self {
            kind: ToolbarItemKind::Custom {
                pending: PendingChild::Id(id),
                menu_form: None,
            },
        }
    }

    /// A collapsible widget that supplies its own menu form via
    /// [`ToolbarOverflow`]. When the bar is too narrow, the widget is hidden
    /// and its `toolbar_menu_form()` appears in the overflow menu.
    pub fn collapsible(widget: impl Widget + ToolbarOverflow + 'static) -> Self {
        let menu_form = widget.toolbar_menu_form();
        Self {
            kind: ToolbarItemKind::Custom {
                pending: PendingChild::Deferred(Box::new(widget)),
                menu_form: Some(OverflowMenuForm::Action(menu_form)),
            },
        }
    }

    /// Make a [`custom`](Self::custom) widget collapsible with an explicit menu
    /// **row** — the [`ToolbarAction`] shown when it overflows (NSToolbar
    /// `menuFormRepresentation`). Best for controls whose menu form is a
    /// single command; an icon-only inline control reuses its icon here as the
    /// menu item's leading glyph (set it via [`ToolbarAction::icon`]).
    pub fn overflow_as(mut self, menu_form: ToolbarAction) -> Self {
        if let ToolbarItemKind::Custom { menu_form: mf, .. } = &mut self.kind {
            *mf = Some(OverflowMenuForm::Action(menu_form));
        }
        self
    }

    /// Make a [`custom`](Self::custom) widget collapsible by embedding a **live
    /// widget** in the overflow menu — the factory rebuilds the control (e.g.
    /// a `ComboBox` bound to the same signal) so it stays fully interactive
    /// while collapsed, instead of degrading to a one-shot menu row. Best for
    /// stateful inputs (combo boxes, sliders) that have no meaningful single
    /// "command" representation.
    pub fn overflow_widget(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        if let ToolbarItemKind::Custom { menu_form: mf, .. } = &mut self.kind {
            *mf = Some(OverflowMenuForm::Widget(Rc::new(factory)));
        }
        self
    }

    /// A separator line between groups.
    pub fn separator() -> Self {
        Self {
            kind: ToolbarItemKind::Separator,
        }
    }

    /// Flexible space that pushes the following items to the trailing edge
    /// (NSToolbar `flexibleSpace`). Collapses to nothing when over-constrained.
    pub fn flexible_space() -> Self {
        Self {
            kind: ToolbarItemKind::FlexibleSpace,
        }
    }
}

/// A command bar with automatic overflow. See the [module docs](self).
pub struct Toolbar {
    items: Vec<ToolbarItem>,
    orientation: ToolbarOrientation,
    display_mode: ToolbarDisplayMode,
    spacing: f32,
    label: Option<LocalizedString>,

    // Reactive state (created in `new`, shared with build).
    /// Per-action collapsed flag (index = action declaration order).
    overflowed: Signal<Vec<bool>>,
    /// Whether any action is collapsed (drives chevron visibility).
    is_overflowing: Signal<bool>,
    /// Roving tab-index: the action index (or `action_count` for the chevron)
    /// that is currently the toolbar's single Tab stop.
    roving: Signal<usize>,

    // Build state.
    /// Overflow metadata per collapsible item (declaration order): the
    /// menu-form to show in the chevron menu plus each item's overflow
    /// priority / `always_overflow`. Drives both the overflow menu rows
    /// (gated by [`overflowed`](Self::overflowed)) and [`compute_overflow`].
    menu_forms: Rc<Vec<CollapsibleMeta>>,
    /// Inline widget id per collapsible item (a `Button` for actions, the
    /// widget itself for collapsible customs), aligned with `menu_forms`.
    collapsible_ids: Vec<WidgetId>,
    /// Pinned-item ids (non-collapsible customs / separators / flexible-space)
    /// for measurement.
    pinned_ids: Vec<WidgetId>,
    chevron_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
    /// Cached overflow flags to avoid redundant signal writes.
    last_flags: RefCell<Vec<bool>>,
}

impl Toolbar {
    /// Create an empty toolbar with the default orientation (horizontal) and
    /// `IconAndText` display mode. Add commands with [`action`](Self::action) or
    /// layout items with [`item`](Self::item).
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            orientation: ToolbarOrientation::Horizontal,
            display_mode: ToolbarDisplayMode::default(),
            spacing: TOOLBAR_SPACING,
            label: None,
            overflowed: Signal::new(Vec::new()),
            is_overflowing: Signal::new(false),
            roving: Signal::new(0),
            menu_forms: Rc::new(Vec::new()),
            collapsible_ids: Vec::new(),
            pinned_ids: Vec::new(),
            chevron_id: None,
            root_child_id: None,
            last_flags: RefCell::new(Vec::new()),
        }
    }

    /// Add an item (action, pinned widget, separator, flexible space).
    pub fn item(mut self, item: ToolbarItem) -> Self {
        self.items.push(item);
        self
    }

    /// Sugar for `.item(ToolbarItem::action(a))`.
    pub fn action(self, action: ToolbarAction) -> Self {
        self.item(ToolbarItem::action(action))
    }

    /// Add a pinned inline child widget (sugar for
    /// `.item(ToolbarItem::custom(widget))`). Pinned widgets never collapse
    /// into the overflow menu — use [`action`](Self::action) for collapsible
    /// commands.
    pub fn child(self, widget: impl Widget + 'static) -> Self {
        self.item(ToolbarItem::custom(widget))
    }

    /// Add a pinned inline child by pre-registered id (sugar for
    /// `.item(ToolbarItem::custom_id(id))`).
    pub fn add_child(self, id: WidgetId) -> Self {
        self.item(ToolbarItem::custom_id(id))
    }

    /// Set the layout axis (default [`ToolbarOrientation::Horizontal`]).
    pub fn orientation(mut self, orientation: ToolbarOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Set how inline actions render their label and icon (default
    /// [`ToolbarDisplayMode::IconAndText`]).
    pub fn display_mode(mut self, mode: ToolbarDisplayMode) -> Self {
        self.display_mode = mode;
        self
    }

    /// Gap between consecutive toolbar items in logical pixels (default
    /// [`TOOLBAR_SPACING`]).
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Override the accessible name (default: the localized "Toolbar").
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Reactive signal that is `true` whenever any action is collapsed into the
    /// overflow menu (WinUI `IsOverflowOpen`-adjacent introspection).
    pub fn is_overflowing(&self) -> Signal<bool> {
        self.is_overflowing.clone()
    }

    fn horizontal(&self) -> bool {
        self.orientation == ToolbarOrientation::Horizontal
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Toolbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toolbar")
            .field("items", &self.items.len())
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for Toolbar {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _ = ctx.theme_signal();
        let horizontal = self.horizontal();

        // Build the row in declaration order. Collapsible items (actions +
        // collapsible customs) get an index `i` into `collapsible_ids` /
        // `menu_forms`, are gated by `visible_when(!overflowed[i])`, and become
        // the roving tab-stop when `roving == i`. Pinned items always show.
        let take_items = std::mem::take(&mut self.items);
        self.collapsible_ids = Vec::new();
        self.pinned_ids = Vec::new();
        let mut menu_forms: Vec<CollapsibleMeta> = Vec::new();
        let mut child_ids: Vec<WidgetId> = Vec::new();

        for item in take_items {
            // Resolve the item to an inline widget id + an optional menu form.
            let (inline_id, menu_form): (WidgetId, Option<OverflowMenuForm>) = match item.kind {
                ToolbarItemKind::Action(action) => {
                    let id = ctx.add(action.make_button(self.display_mode));
                    (id, Some(OverflowMenuForm::Action(action)))
                }
                ToolbarItemKind::Custom { pending, menu_form } => {
                    let id = match pending {
                        PendingChild::Id(id) => id,
                        PendingChild::Deferred(w) => ctx.add_boxed(w),
                    };
                    (id, menu_form)
                }
                ToolbarItemKind::Separator => {
                    let id = ctx.add(if horizontal {
                        Divider::vertical()
                    } else {
                        Divider::horizontal()
                    });
                    self.pinned_ids.push(id);
                    child_ids.push(id);
                    continue;
                }
                ToolbarItemKind::FlexibleSpace => {
                    let id = ctx.add(Spacer::new());
                    self.pinned_ids.push(id);
                    child_ids.push(id);
                    continue;
                }
            };
            child_ids.push(inline_id);
            match menu_form {
                Some(form) => {
                    // Collapsible: gate visibility + roving tab-stop on its index.
                    let i = self.collapsible_ids.len();
                    let of = self.overflowed.clone();
                    ctx.visible_when(
                        inline_id,
                        of.map(move |flags| flags.get(i).copied() != Some(true)),
                    );
                    let rov = self.roving.clone();
                    ctx.set_tab_stop(inline_id, rov.map(move |r| *r == i));
                    self.collapsible_ids.push(inline_id);
                    // Priority / always-overflow come from the action form;
                    // an embedded-widget form defaults to ordinary priority.
                    let (priority, always_overflow) = match &form {
                        OverflowMenuForm::Action(a) => (a.priority, a.always_overflow),
                        OverflowMenuForm::Widget(_) => (0, false),
                    };
                    menu_forms.push(CollapsibleMeta {
                        priority,
                        always_overflow,
                        form,
                    });
                }
                None => self.pinned_ids.push(inline_id),
            }
        }

        let action_count = self.collapsible_ids.len();
        self.menu_forms = Rc::new(menu_forms);
        // Seed the overflowed flags (none collapsed initially → all visible).
        self.overflowed.set(vec![false; action_count]);
        *self.last_flags.borrow_mut() = vec![false; action_count];

        // Overflow chevron: a PopoverIconButton (HasPopup::Menu) whose content
        // is a real `MenuList` with ONE row per collapsible item, each gated
        // via `MenuList::item_when(overflowed[i])`. Only the currently
        // collapsed rows are shown — hidden rows collapse to zero height (no
        // gaps) and are skipped by keyboard navigation — so the menu
        // reconciles reactively as the bar resizes, with no rebuild of the
        // (dormant) popover subtree. Using `MenuList` (rather than a bare
        // column) gives the menu its compact size-to-content sizing, the
        // standard popover chrome, focus-on-open (it is a focusable Tab stop,
        // and `PopoverWidget` moves focus into it), and arrow/Home/End/Enter
        // keyboard navigation. A row may be an ordinary menu item OR a live
        // embedded widget (e.g. a `ComboBox` bound to the same signal as its
        // inline twin).
        if action_count > 0 {
            let menu_forms = self.menu_forms.clone();
            let mut menu = MenuList::new();
            for (i, meta) in menu_forms.iter().enumerate() {
                let row: Box<dyn Widget> = match &meta.form {
                    OverflowMenuForm::Action(a) => Box::new(a.make_menu_item()),
                    OverflowMenuForm::Widget(factory) => factory(),
                };
                // Show this row only while its inline twin is collapsed.
                let of = self.overflowed.clone();
                let visible = of.map(move |flags| flags.get(i).copied() == Some(true));
                menu = menu.item_boxed_when(row, visible);
            }
            let chevron = PopoverIconButton::new(
                IconButton::new(IconWidget::chevron_down(ICON_SIZE))
                    .tooltip(bastyde_i18n::tr_widget!(toolbar_more())),
            )
            .content(menu)
            // `MenuList` already routes through the Menu `PopoverStyle`
            // for its own surface — don't double-chrome it.
            .bare()
            .placement(OverlayPlacement::BelowPreferred)
            .has_popup_kind(HasPopup::Menu);
            let chevron_id = ctx.add(chevron);
            let is_of = self.is_overflowing.clone();
            ctx.visible_when(chevron_id, is_of);
            // Chevron is the roving stop when roving == action_count.
            let rov = self.roving.clone();
            ctx.set_tab_stop(chevron_id, rov.map(move |r| *r == action_count));
            self.chevron_id = Some(chevron_id);
            child_ids.push(chevron_id);
        }

        // ARIA toolbar keyboard pattern: the bar is a single Tab stop with
        // roving focus. Intercept arrow / Home / End on the preview pass (so it
        // works while a child button is focused) and move focus + the roving
        // tab-stop among the visible controls (+ chevron).
        if action_count > 0 {
            let abids = self.collapsible_ids.clone();
            let chevron_id = self.chevron_id;
            let overflowed = self.overflowed.clone();
            let roving = self.roving.clone();
            let handlers = HandlerSet::new().on_key_preview(
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    let WidgetEvent::KeyDown { key, .. } = event else {
                        return EventResponse::Ignored;
                    };
                    // Roving directions follow the *visual* axis, resolved at
                    // event time so a locale change flips them live. On a
                    // horizontal bar under RTL the layout mirrors (logical-first
                    // sits at the right edge), so ArrowRight steps to the
                    // previous control and ArrowLeft to the next — per the
                    // WAI-ARIA toolbar pattern. Vertical bars are direction-
                    // independent.
                    let (prev, next) = if horizontal {
                        if ctx.is_rtl() {
                            (Key::ArrowRight, Key::ArrowLeft)
                        } else {
                            (Key::ArrowLeft, Key::ArrowRight)
                        }
                    } else {
                        (Key::ArrowUp, Key::ArrowDown)
                    };
                    let is_nav =
                        *key == prev || *key == next || *key == Key::Home || *key == Key::End;
                    if !is_nav {
                        return EventResponse::Ignored;
                    }
                    // Focusable roving indices: visible actions then chevron.
                    let flags = overflowed.get();
                    let mut seq: Vec<usize> = (0..abids.len())
                        .filter(|i| flags.get(*i).copied() != Some(true))
                        .collect();
                    if chevron_id.is_some() && flags.iter().any(|&f| f) {
                        seq.push(abids.len());
                    }
                    if seq.is_empty() {
                        return EventResponse::Ignored;
                    }
                    let cur = roving.get();
                    let pos = seq.iter().position(|&x| x == cur).unwrap_or(0);
                    let new_pos = if *key == Key::Home {
                        0
                    } else if *key == Key::End {
                        seq.len() - 1
                    } else if *key == next {
                        (pos + 1).min(seq.len() - 1)
                    } else {
                        pos.saturating_sub(1)
                    };
                    let target = seq[new_pos];
                    roving.set(target);
                    let id = if target < abids.len() {
                        Some(abids[target])
                    } else {
                        chevron_id
                    };
                    if let Some(id) = id {
                        ctx.request_focus(id);
                    }
                    EventResponse::Handled
                },
            );
            ctx.apply_self_handlers(handlers);
        }

        let row: WidgetId = if horizontal {
            let mut r = HStack::new().spacing(self.spacing);
            for id in &child_ids {
                r = r.add_child(*id);
            }
            ctx.add(r)
        } else {
            let mut r = VStack::new().spacing(self.spacing);
            for id in &child_ids {
                r = r.add_child(*id);
            }
            ctx.add(r)
        };

        let root = ctx.add(Panel::new().a11y_presentational().child_id(row));
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let Some(root) = self.root_child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };
        // The toolbar FILLS its offered main extent and handles overflow
        // internally (collapsing actions into the chevron menu in
        // `place_children`). If it reported its natural content width instead,
        // its parent would size it to that width and it would spill outside the
        // container. Take the content size only on an axis the parent left open.
        let content = ctx
            .child_size(root, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
        let horizontal = self.horizontal();
        let (width, height) = if horizontal {
            (proposal.width.unwrap_or(content.width), content.height)
        } else {
            (content.width, proposal.height.unwrap_or(content.height))
        };
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        // Position the single root child to fill our bounds.
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = Size::new(bounds.width, bounds.height);
        }

        // Compute the overflow set from intrinsic sizes (measured even while
        // collapsed) along the main axis.
        let horizontal = self.horizontal();
        let avail = if horizontal {
            bounds.width
        } else {
            bounds.height
        };
        let main = |s: Size| if horizontal { s.width } else { s.height };

        let probe = SizeProposal::unspecified();
        let mut pinned_total = 0.0_f32;
        for &id in &self.pinned_ids {
            if let Some(s) = ctx.measure_intrinsic(id, probe) {
                pinned_total += main(s);
            }
        }
        let n = self.collapsible_ids.len();
        let mut action_w = vec![0.0_f32; n];
        for (i, &id) in self.collapsible_ids.iter().enumerate() {
            action_w[i] = ctx.measure_intrinsic(id, probe).map(main).unwrap_or(0.0);
        }

        let priorities: Vec<i32> = self.menu_forms.iter().map(|a| a.priority).collect();
        let always: Vec<bool> = self.menu_forms.iter().map(|a| a.always_overflow).collect();
        let total_slots = self.pinned_ids.len() + n; // for spacing estimate

        let flags = compute_overflow(
            avail,
            pinned_total,
            &action_w,
            &priorities,
            &always,
            self.spacing,
            total_slots,
        );

        // Publish flags (guarded) + the is_overflowing signal. The overflow
        // menu's rows are gated directly off `overflowed` via `visible_when`,
        // so flipping the signal reconciles the popover with no extra model.
        if *self.last_flags.borrow() != flags {
            *self.last_flags.borrow_mut() = flags.clone();
            let any = flags.iter().any(|&f| f);

            self.overflowed.set(flags.clone());
            if self.is_overflowing.get() != any {
                self.is_overflowing.set(any);
            }
            // Keep the roving tab-stop on a visible target.
            self.clamp_roving(&flags);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::Toolbar);
        builder.set_orientation(if self.horizontal() {
            bastyde_core::accesskit::Orientation::Horizontal
        } else {
            bastyde_core::accesskit::Orientation::Vertical
        });
        let name = self
            .label
            .as_ref()
            .map(|l| l.resolve_now())
            .unwrap_or_else(|| bastyde_i18n::tr_widget!(a11y_toolbar_name()).resolve_now());
        builder.set_name(name);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

impl Toolbar {
    /// The ordered list of currently-focusable controls: visible action
    /// buttons (declaration order) followed by the chevron when overflowing.
    /// Returned as roving indices (`0..n` for actions, `n` for the chevron).
    fn focusable_indices(&self, flags: &[bool]) -> Vec<usize> {
        let mut seq: Vec<usize> = Vec::new();
        for i in 0..self.collapsible_ids.len() {
            if flags.get(i).copied() != Some(true) {
                seq.push(i);
            }
        }
        if flags.iter().any(|&f| f) {
            seq.push(self.collapsible_ids.len()); // chevron slot
        }
        seq
    }

    /// Resolve a roving index to the widget id it controls.
    fn id_for_roving(&self, r: usize) -> Option<WidgetId> {
        if r < self.collapsible_ids.len() {
            Some(self.collapsible_ids[r])
        } else {
            self.chevron_id
        }
    }

    /// Ensure `roving` points at a focusable (visible) control.
    fn clamp_roving(&self, flags: &[bool]) {
        let seq = self.focusable_indices(flags);
        if seq.is_empty() {
            return;
        }
        let cur = self.roving.get();
        if !seq.contains(&cur) {
            self.roving.set(seq[0]);
        }
    }
}

/// Greedy priority overflow: keep actions inline in declaration order, but drop
/// the **lowest-priority** ones (ties: last declared) into the menu until the
/// rest fit, reserving the chevron once anything overflows. `always_overflow`
/// actions start collapsed. Returns a per-action collapsed flag.
fn compute_overflow(
    avail: f32,
    pinned_total: f32,
    action_w: &[f32],
    priority: &[i32],
    always: &[bool],
    spacing: f32,
    total_slots: usize,
) -> Vec<bool> {
    let n = action_w.len();
    let mut collapsed = always.to_vec();
    collapsed.resize(n, false);

    // Width of everything that is currently inline.
    let inline_width = |collapsed: &[bool], with_chevron: bool| -> f32 {
        let mut visible_slots = total_slots;
        let mut w = pinned_total;
        for i in 0..n {
            if collapsed[i] {
                visible_slots -= 1;
            } else {
                w += action_w[i];
            }
        }
        if with_chevron {
            w += CHEVRON_EXTENT;
            visible_slots += 1;
        }
        w + spacing * (visible_slots.saturating_sub(1)) as f32
    };

    let any_collapsed = |c: &[bool]| c.iter().any(|&x| x);

    // If nothing is forced into overflow and it all fits, done.
    if !any_collapsed(&collapsed) && inline_width(&collapsed, false) <= avail + 0.5 {
        return collapsed;
    }

    // Otherwise the chevron is present; drop lowest-priority inline actions
    // until the rest fit (or none remain inline).
    loop {
        if inline_width(&collapsed, true) <= avail + 0.5 {
            break;
        }
        // Pick the lowest-priority still-inline action (ties: highest index).
        let mut victim: Option<usize> = None;
        for i in 0..n {
            if collapsed[i] {
                continue;
            }
            match victim {
                None => victim = Some(i),
                Some(v) => {
                    if priority[i] < priority[v] || (priority[i] == priority[v] && i > v) {
                        victim = Some(i);
                    }
                }
            }
        }
        match victim {
            Some(v) => collapsed[v] = true,
            None => break, // nothing left inline; residual overflow of the bar
        }
    }
    collapsed
}

#[cfg(test)]
mod tests;
