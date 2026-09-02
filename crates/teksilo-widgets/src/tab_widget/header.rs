// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `TabHeader` — one tab's chrome (icon + leading slot + label + trailing slot),
//! plus background/indicator/focus-ring painting and the input handlers.
//!
//! Layout contract:
//! - `layout_response(proposal)` honors `proposal.width` when the parent
//!   row forces a width (Shared-sizing path). When the parent leaves
//!   width unspecified, the header reports its natural content width
//!   clamped to `[min_tab_width, max_tab_width]` (Independent path).
//! - Height is the tab style's `editor_tab_height` plus the focus-ring
//!   envelope reserved on top and bottom — the row's place_children
//!   uniforms the height across all headers.
//!
//! Paint contract (Int UI / IntelliJ):
//! - Selected: `surface_content` background, `text_primary` label,
//!   1 dp accent bar at the top.
//! - Hovered: `surface_hover` background.
//! - Idle: transparent background.
//! - Disabled: transparent background, `text_disabled` label.
//! - Focus ring: 2 dp `focus_ring` stroke painted around the visual
//!   rect, and only on keyboard focus (pointer focus does not paint
//!   the ring — IntelliJ / VS Code convention).

use std::cell::RefCell;
use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::drag_payload::{DragPayload, DropOutcome};
use teksilo_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SharedTabStyle, TabStyleConfig};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;
use teksilo_tokens::TextRole;

use crate::primitives::{Expand, RectWidget, ZStack};
use crate::{HStack, IconButton, IconButtonSize, IconWidget, TextWidget};

/// Minimum natural width when the label is empty / extremely short.
const NATURAL_MIN_WIDTH: f32 = 72.0;
/// Vertical padding around the label inside the tab.
const HEADER_PADDING_V: f32 = 6.0;
/// Spacing inside the inner row between icon, slots, and label.
const INNER_GAP: f32 = 6.0;
/// Fallback char width for natural-size estimation when the text
/// backend is unavailable (test contexts).
const FALLBACK_CHAR_WIDTH: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabHeaderInteraction {
    Idle,
    Hovered,
}

/// Shared state plumbing the bar populates and the headers read.
///
/// `header_ids` and `panel_ids` are filled during the bar's `build()`
/// (after each header is added to the arena). `enabled_tabs` is a
/// frozen snapshot taken at build time — it does not need interior
/// mutability because the bar rebuilds when the data source changes.
pub(crate) struct HeaderShared {
    pub header_ids: Rc<RefCell<Vec<WidgetId>>>,
    pub panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    pub enabled_tabs: Rc<Vec<bool>>,
}

pub(crate) struct TabHeader {
    label: LocalizedString,
    /// Live `Signal<String>` derived from `label` via `to_signal()`.
    /// `accessibility()` reads this rather than a frozen `String`
    /// snapshot, so a locale change re-resolves the AT name without
    /// rebuilding the header. The signal is bound at
    /// `BindingLevel::AccessibilityOnly` in `build()` so the
    /// framework dirties the AT cache on update.
    label_signal: Option<Signal<String>>,
    /// Accessible name — the original title, preserved even when `label` is
    /// blanked for an icon-only display mode (see `accessibility()`).
    at_name: LocalizedString,
    icon: Option<IconWidget>,
    leading_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    /// Content presence + icon extent captured at construction — `build()`
    /// **moves** the icon / slots out (`take()`), so the layout methods can no
    /// longer read `self.icon` etc. These mirror them for width estimation and
    /// icon-only detection.
    has_icon: bool,
    icon_extent: f32,
    has_leading: bool,
    has_trailing: bool,
    tooltip: Option<LocalizedString>,
    /// Optional rich-tooltip source — registry key or inline content.
    /// Mutually exclusive with `tooltip` and `composite_tooltip`.
    rich_tooltip: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite-tooltip body. Mutually exclusive with the
    /// other two slots.
    composite_tooltip: Option<Box<dyn Widget>>,
    context_menu_factory: Option<super::delegate::ContextMenuFactory>,
    /// Per-tab close callback. Set when the tab is closable AND the
    /// bar carries an `on_close` handler (or its source is a
    /// `ListModel<T>` providing a default-remove). The header wires
    /// it to a trailing close button, a middle-click handler, and
    /// the `Delete` key. Receives the firing `EventContext` so apps
    /// can open a confirmation dialog before actually closing.
    on_close: Option<Rc<dyn Fn(&mut EventContext)>>,
    /// Per-tab reorder callback. Set when the bar is reorderable;
    /// receives the destination index and the firing
    /// [`EventContext`]. Wired to AT custom actions "Move Left" /
    /// "Move Right" / "Move Up" / "Move Down" so screen-reader
    /// users can reorder tabs without dragging.
    on_reorder_to: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    /// `Some(factory)` when this tab is a drag source (reordering on,
    /// or cross-bar transfer enabled). The header attaches `on_drag`
    /// that publishes the factory's `DragPayload` on drag-start. The
    /// bar builds the factory (it has the item and its identity in
    /// scope), so the header stays non-generic.
    make_drag_payload: Option<Rc<dyn Fn() -> DragPayload>>,
    /// `Some(handler)` when this header should react to drag
    /// completion (cross-bar transfer-out). Fired by the framework on
    /// the drag source with the final [`DropOutcome`].
    on_drag_ended: Option<Rc<dyn Fn(DropOutcome, &mut EventContext)>>,

    index: usize,
    /// Structural per-tab enabled flag. Forwarded into the arena at
    /// build time; the arena is then the single source of truth
    /// (events, focus, a11y `set_disabled`, leaf role-substitution all
    /// consult `arena.is_enabled(self_id)` / `PaintContext::effective_enabled`).
    /// Kept on the struct so `accessibility()` can decide whether to
    /// advertise the `Click` and reorder custom actions for the
    /// structural-disabled case.
    initial_enabled: bool,
    selected: Signal<usize>,
    shared: Rc<HeaderShared>,

    interaction: Signal<TabHeaderInteraction>,
    /// Raw keyboard/pointer focus (any modality). The keyboard-only focus
    /// ring is derived live from this × the input-modality signal in
    /// `build()` (`:focus-visible`).
    focused: Signal<bool>,

    /// Width clamps applied to the natural content width when the bar
    /// is in Independent sizing mode.
    min_width: f32,
    max_width: f32,
    /// `true` for pinned tabs — icon-only, fixed-width, no close
    /// button, no label visible (the label becomes the tooltip).
    pinned: bool,
    /// Bar orientation — controls whether the selected-state accent
    /// indicator paints on the top edge (horizontal bars, the
    /// browser-tab convention) or the leading edge (vertical bars,
    /// the IDE-perspective / sidebar convention).
    orientation: super::delegate::TabBarOrientation,
    /// All-states background shorthand, set by the parent
    /// [`TabBar`](super::TabBar) via `tab_background(...)`. The per-state
    /// fields below fall back to this, which falls back to transparent.
    tab_background: Option<teksilo_core::color_prop::ColorProp>,
    /// Background for this tab while it is the selected one. Falls back to
    /// `tab_background`, then transparent.
    selected_tab_background: Option<teksilo_core::color_prop::ColorProp>,
    /// Background while hovered (and not selected). Falls back to
    /// `tab_background`, then transparent.
    hover_tab_background: Option<teksilo_core::color_prop::ColorProp>,
    /// Background while idle (not selected, not hovered). Falls back to
    /// `tab_background`, then transparent.
    idle_tab_background: Option<teksilo_core::color_prop::ColorProp>,
    /// Text role used for the label (and matching icon tint) when this
    /// tab is the selected one. Default: `TextRole::Primary` (the Int
    /// UI editor-strip convention). Set by the parent `TabBar` via
    /// `selected_text_role(...)`.
    selected_text_role: TextRole,
    /// Text role used for the label (and matching icon tint) when this
    /// tab is idle (not selected, not disabled). Default:
    /// `TextRole::Secondary`. Set by the parent `TabBar` via
    /// `idle_text_role(...)`.
    idle_text_role: TextRole,
    /// Which edge the active-tab highlight indicator hugs, forwarded into
    /// the [`TabStyleConfig`].
    active_indicator: teksilo_core::styles::TabIndicatorPosition,
    /// Per-call style override propagated from the parent `TabBar`'s
    /// `.style(impl TabStyle)` builder. `None` means "use the theme
    /// slot or the bundled `RecipeTabStyle`".
    pub(crate) style_override: Option<SharedTabStyle>,

    inner_root_id: Option<WidgetId>,
}

impl std::fmt::Debug for TabHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabHeader")
            .field("index", &self.index)
            .field("initial_enabled", &self.initial_enabled)
            .field("label", &self.label.clone().resolve_now())
            .field("has_icon", &self.icon.is_some())
            .field("has_leading_slot", &self.leading_slot.is_some())
            .field("has_trailing_slot", &self.trailing_slot.is_some())
            .field("has_tooltip", &self.tooltip.is_some())
            .field("has_context_menu", &self.context_menu_factory.is_some())
            .finish()
    }
}

pub(crate) struct TabHeaderConfig {
    pub label: LocalizedString,
    /// The tab's accessible name — the *original* title, even when the visible
    /// `label` is blanked for an icon-only display mode. Keeps the AT name
    /// meaningful when the chrome shows only an icon.
    pub at_name: LocalizedString,
    pub icon: Option<IconWidget>,
    pub leading_slot: Option<Box<dyn Widget>>,
    pub trailing_slot: Option<Box<dyn Widget>>,
    pub tooltip: Option<LocalizedString>,
    pub rich_tooltip: Option<crate::tooltip::RichTooltipSource>,
    pub composite_tooltip: Option<Box<dyn Widget>>,
    pub context_menu_factory: Option<super::delegate::ContextMenuFactory>,
    pub on_close: Option<Rc<dyn Fn(&mut EventContext)>>,
    pub on_reorder_to: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    pub make_drag_payload: Option<Rc<dyn Fn() -> DragPayload>>,
    pub on_drag_ended: Option<Rc<dyn Fn(DropOutcome, &mut EventContext)>>,
    pub index: usize,
    pub initial_enabled: bool,
    pub selected: Signal<usize>,
    pub shared: Rc<HeaderShared>,
    pub min_width: f32,
    pub max_width: f32,
    pub pinned: bool,
    pub orientation: super::delegate::TabBarOrientation,
    pub tab_background: Option<teksilo_core::color_prop::ColorProp>,
    pub selected_tab_background: Option<teksilo_core::color_prop::ColorProp>,
    pub hover_tab_background: Option<teksilo_core::color_prop::ColorProp>,
    pub idle_tab_background: Option<teksilo_core::color_prop::ColorProp>,
    pub selected_text_role: TextRole,
    pub idle_text_role: TextRole,
    pub active_indicator: teksilo_core::styles::TabIndicatorPosition,
    pub style_override: Option<SharedTabStyle>,
}

impl TabHeader {
    pub(crate) fn new(cfg: TabHeaderConfig) -> Self {
        // Snapshot content presence + icon size before the fields are moved in —
        // `build()` later `take()`s the icon and slots, so the layout methods
        // can't read them.
        let has_icon = cfg.icon.is_some();
        let icon_extent = cfg.icon.as_ref().map(|i| i.display_size()).unwrap_or(0.0);
        let has_leading = cfg.leading_slot.is_some();
        let has_trailing = cfg.trailing_slot.is_some();
        Self {
            label: cfg.label,
            label_signal: None,
            at_name: cfg.at_name,
            icon: cfg.icon,
            leading_slot: cfg.leading_slot,
            trailing_slot: cfg.trailing_slot,
            has_icon,
            icon_extent,
            has_leading,
            has_trailing,
            tooltip: cfg.tooltip,
            rich_tooltip: cfg.rich_tooltip,
            composite_tooltip: cfg.composite_tooltip,
            context_menu_factory: cfg.context_menu_factory,
            on_close: cfg.on_close,
            on_reorder_to: cfg.on_reorder_to,
            make_drag_payload: cfg.make_drag_payload,
            on_drag_ended: cfg.on_drag_ended,
            index: cfg.index,
            initial_enabled: cfg.initial_enabled,
            selected: cfg.selected,
            shared: cfg.shared,
            interaction: Signal::new(TabHeaderInteraction::Idle),
            focused: Signal::new(false),
            min_width: cfg.min_width,
            max_width: cfg.max_width,
            pinned: cfg.pinned,
            orientation: cfg.orientation,
            tab_background: cfg.tab_background,
            selected_tab_background: cfg.selected_tab_background,
            hover_tab_background: cfg.hover_tab_background,
            idle_tab_background: cfg.idle_tab_background,
            selected_text_role: cfg.selected_text_role,
            idle_text_role: cfg.idle_text_role,
            active_indicator: cfg.active_indicator,
            style_override: cfg.style_override,
            inner_root_id: None,
        }
    }

    /// An icon-only tab — no visible title text, but an icon or leading slot.
    /// Sized to its icon instead of the text-tab minimum, so an icon button
    /// doesn't pad out to a full label width. (Pinned tabs are handled
    /// separately with a fixed width.) Uses the construction-time presence
    /// flags, since `build()` has already moved the icon / slots out.
    fn is_icon_only(&self) -> bool {
        let label = self
            .label_signal
            .as_ref()
            .map(|s| s.get())
            .unwrap_or_else(|| self.label.clone().resolve_now());
        label.trim().is_empty() && (self.has_icon || self.has_leading)
    }

    fn estimate_natural_width(&self, ctx: &LayoutContext) -> f32 {
        let pad_h = crate::styles::recipe_tab_style::TAB_PADDING_HORIZONTAL;
        use crate::styles::recipe_button_style as btn;

        // Icon-only: a square-ish tab — the glyph + horizontal padding only (no
        // label, no trailing-spacer gap), so it shrinks to the icon instead of
        // padding out to a text width.
        if self.is_icon_only() {
            let glyph = if self.has_icon {
                self.icon_extent
            } else {
                btn::BUTTON_ICON_SIZE
            };
            return glyph + pad_h * 2.0;
        }

        // Resolve the label once for measurement. Cheap: a literal
        // resolves to a clone; a translated key looks up Fluent.
        let label = self
            .label_signal
            .as_ref()
            .map(|s| s.get())
            .unwrap_or_else(|| self.label.clone().resolve_now());
        // Measure with the SAME style the label widget actually renders
        // with — `TextWidget` defaults to `TextStyleRole::Body`. Measuring
        // with `small` here underestimated the natural width, so the bar
        // sized too narrow and the longest label truncated once the chrome
        // was made to fill the tab (it previously overflowed, masking this).
        let text_width = if let Some(backend) = ctx.text_backend {
            backend
                .borrow_mut()
                .layout_single_line(&label, &ctx.theme.typography.body, None)
                .width
        } else {
            label.len() as f32 * FALLBACK_CHAR_WIDTH
        };
        // `build()` has moved the icon / slots out, so reserve their width from
        // the construction-time snapshot (the real icon extent, not a constant —
        // an icon larger than `BUTTON_ICON_SIZE` was previously under-reserved).
        let icon_size = if self.has_icon {
            self.icon_extent + INNER_GAP
        } else {
            0.0
        };
        let leading_size = if self.has_leading {
            btn::BUTTON_ICON_SIZE + INNER_GAP
        } else {
            0.0
        };
        let trailing_size = if self.has_trailing {
            btn::BUTTON_ICON_SIZE + INNER_GAP
        } else {
            0.0
        };
        // The non-pinned row always carries a flexible `Spacer` after the
        // label (it pins trailing controls to the edge / absorbs Shared
        // slack). It contributes zero width, but the row's HStack still
        // places an `INNER_GAP` between the label and the spacer — so the
        // widest label needs `INNER_GAP` more than the text alone, or it
        // truncates by exactly that amount. Reserve it here.
        let spacer_gap = if self.pinned { 0.0 } else { INNER_GAP };
        // Bounds == visual rect now (no focus-ring envelope), so
        // natural width is purely content + horizontal padding.
        let content =
            text_width + icon_size + leading_size + trailing_size + spacer_gap + pad_h * 2.0;
        content.max(NATURAL_MIN_WIDTH)
    }

    pub(crate) fn intrinsic_height(_ctx: &LayoutContext) -> f32 {
        // `editor_tab_height` is the **outer** measurement (the
        // total bounds height of one tab header). The focus-ring
        // envelope is reserved *inside* this — not added on top —
        // so the tab strip sits at exactly the token's value
        // regardless of focus-ring tokens. The visible pill rect
        // inside the bounds is `editor_tab_height - envelope*2`,
        // which `place_children` and `paint` shrink to.
        crate::styles::recipe_tab_style::TAB_EDITOR_HEIGHT
    }
}

impl Widget for TabHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the structural per-tab enabled hint into the arena.
        // After this point the arena is the single source of truth;
        // events are gated, focus walker skips the subtree, the a11y
        // walker auto-emits `set_disabled()`, and the leaves substitute
        // `TextRole::Disabled` via `PaintContext::effective_enabled`.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let interaction = ctx.signal(TabHeaderInteraction::Idle);
        let focused: Signal<bool> = ctx.signal(false);
        let registry = ctx.binding_registry();

        // Repaint on selection / hover / focus changes.
        self.selected
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        focused.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        self.interaction = interaction.clone();
        self.focused = focused.clone();

        // Locale-reactive label signal — used by `accessibility()`
        // to keep the AT name in sync with locale changes (and by
        // the natural-width estimate when present). Bound at
        // AccessibilityOnly so a locale change refreshes the AT
        // cache without a layout pass.
        let label_signal = self.label.to_signal();
        label_signal.bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        self.label_signal = Some(label_signal);

        // Locale signal binding for the reorder CustomAction
        // descriptions ("Move Left" / "Move Right" / "Move Up" /
        // "Move Down"). They're resolved inside `accessibility()`
        // via `lit!(...).resolve_now()`; binding
        // the locale signal at `AccessibilityOnly` dirties the AT
        // cache when the locale flips so the descriptions refresh.
        ctx.locale_signal()
            .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);

        // Roving tabindex (ARIA tabs pattern). Only the selected tab
        // is a Tab-key stop; arrow keys still move focus across
        // headers via `request_focus(headers[next])`. Disabled tabs
        // are already filtered by `is_node_focusable` upstream, so
        // we don't need to special-case `enabled` here. Bound at
        // `RepaintOnly` — the only consumer is `cycle_focus`, which
        // re-evaluates on each Tab keypress.
        let index_for_tab_stop = self.index;
        let tab_stop = self.selected.map(move |sel| *sel == index_for_tab_stop);
        ctx.set_tab_stop(self_id, tab_stop);

        // Build the inner content row.
        //
        // - Standard tab: [icon? leading? label trailing? close?]
        // - Pinned tab:   [icon] only (label collapses into the tooltip;
        //   close button suppressed; leading/trailing slots ignored).
        //   This matches Firefox's pinned-tab presentation.
        let mut row = HStack::new().spacing(INNER_GAP);

        // Per-state text role: Int UI editor-tab convention puts
        // selected at `Primary`, idle at `Secondary`. The leaves
        // (TextWidget for the label, IconWidget for the icon) consult
        // `PaintContext::effective_enabled` and substitute
        // `TextRole::Disabled` themselves when the arena says this
        // tab is disabled — no `enabled` branch needed here.
        let index_for_role = self.index;
        let selected_role = self.selected_text_role;
        let idle_role = self.idle_text_role;
        let role_signal = self.selected.map(move |sel| {
            if *sel == index_for_role {
                selected_role
            } else {
                idle_role
            }
        });

        // An icon-only display tab renders like a pinned tab: just the centred
        // icon, no label / spacer / close button (the title lives in the tooltip
        // + the AT name). This avoids the empty-label inter-child gaps that
        // otherwise pushed the lone icon off-centre.
        let icon_only = self.is_icon_only();

        if let Some(icon) = self.icon.take() {
            // Icon tint follows the same role — selected tab gets
            // a primary-tinted icon, idle gets secondary, disabled
            // gets the disabled tint.
            let icon = icon.color(role_signal.clone());
            let id = ctx.add(icon);
            row = row.add_child(id);
        }

        if self.pinned || icon_only {
            // Just the centred icon — no label / spacer / close button.
            // Pinned tabs promote the title into a tooltip so the icon stays
            // identifiable on hover (icon-only *display* tabs already had this
            // handled by the bar's display-mode transform, which also respects a
            // caller-set tooltip, so we don't clobber it here).
            if self.pinned {
                self.tooltip = Some(self.at_name.clone());
            }
        } else {
            if let Some(slot) = self.leading_slot.take() {
                let id = ctx.add_boxed(slot);
                row = row.add_child(id);
            }

            // Label as a TextWidget — reactive on locale change because
            // `From<LocalizedString> for Prop<String>` produces a bound
            // `Signal<String>` that tracks the i18n manager's locale
            // version. Color is bound to the per-state role above so
            // the selected/idle/disabled distinction is visible.
            let label_widget = TextWidget::new(self.label.clone())
                .single_line()
                .color(role_signal.clone());
            let label_id = ctx.add(label_widget);
            row = row.add_child(label_id);

            // Flexible spacer between the label and the trailing
            // controls. `Spacer::new()` reports natural size 0 with
            // `flex = 1`, so it contributes zero in Independent
            // sizing (HStack reports the natural sum and there's no
            // slack to share) and absorbs all the slack in Shared
            // sizing (the bar gives every tab the same fixed
            // extent), pushing the close button against the
            // trailing edge of the pill regardless of label length.
            let spacer_id = ctx.add(crate::primitives::Spacer::new());
            row = row.add_child(spacer_id);

            if let Some(slot) = self.trailing_slot.take() {
                let id = ctx.add_boxed(slot);
                row = row.add_child(id);
            }

            // Trailing close button for closable tabs. Visible only
            // on hover — Firefox / Chrome convention; the button's
            // tap is consumed by its own handler so selection
            // (driven by the surrounding TabHeader's `on_tap`) does
            // not fire.
            if let Some(close_fn) = self.on_close.clone() {
                // The close button must NOT be focusable: Tab
                // navigation walks between tab headers, not onto
                // each header's close button. Activation happens
                // via mouse click, middle-click on the tab body
                // (handled by the surrounding `on_pointer_event`),
                // or AT custom action.
                let close_button = IconButton::clear()
                    .embedded()
                    .size(IconButtonSize::Compact)
                    .focusable(false)
                    .tooltip(teksilo_i18n::tr_widget!(tab_close_tooltip()))
                    .on_activate_fn(move |ctx| (close_fn)(ctx));
                let close_id = ctx.add(close_button);
                // Hover-only: the button is hidden when the
                // surrounding tab header is in the Idle interaction
                // state. The interaction signal flips to Hovered
                // via the `on_hover` handler installed below.
                let visible_when = interaction.map(|s| matches!(*s, TabHeaderInteraction::Hovered));
                ctx.visible_when(close_id, visible_when);
                row = row.add_child(close_id);
            }
        }

        // Wrap the row in a Padding for breathing room.
        //
        // - Standard tab: symmetric vertical + horizontal padding.
        // - Pinned tab: NO horizontal padding (the pill is 32 dp
        //   wide and the icon is 16 dp; symmetric horizontal
        //   padding would push the icon off-center). Wrap in
        //   `Center` so the icon sits exactly in the middle of the
        //   bounds regardless of the pill width.
        // - Icon-only display tab: just the icon row with the standard padding.
        //   The tab sizes to `icon + 2·pad_h`, so the lone icon fills the padded
        //   box exactly and the symmetric padding centres it (no extra `Center`
        //   wrapper — that layer broke the icon's paint).
        let pad_h = crate::styles::recipe_tab_style::TAB_PADDING_HORIZONTAL;
        let inner_id = if self.pinned {
            let centered = crate::primitives::Center::new().child(row);
            let padded = crate::Padding::symmetric(HEADER_PADDING_V, 0.0).child(centered);
            ctx.add(padded)
        } else {
            let padded = crate::Padding::symmetric(HEADER_PADDING_V, pad_h).child(row);
            ctx.add(padded)
        };

        // Derive the cfg signals the active TabStyle needs. The widget's own
        // interaction state is finer-grained (Idle / Hovered for pointer
        // presence; plus a raw focus bool), but the trait surface is the four
        // canonical booleans. `:focus-visible`: `is_focused` flips true only
        // when focus arrived (or continues) via keyboard — raw focus gated on
        // the live input-modality signal — so the focus ring shows during
        // keyboard navigation but is suppressed on a mouse click (matches
        // IntelliJ / VS Code convention; a click-then-keypress reveals it).
        let index_for_cfg = self.index;
        let is_active = self.selected.map(move |sel| *sel == index_for_cfg);
        let is_hovered = interaction.map(|s| matches!(*s, TabHeaderInteraction::Hovered));
        let is_focused = focused.and(&ctx.focus_visible());
        // Reactive disabled view from the arena (ancestor-AND). The
        // style chrome picks the disabled-surface role from this; the
        // signal flips automatically when a parent's `enabled_when`
        // signal flips.
        let is_disabled = ctx.effective_enabled_signal(self_id).map(|on| !*on);
        let orientation_for_cfg = match self.orientation {
            super::delegate::TabBarOrientation::Horizontal => {
                teksilo_core::styles::TabBarOrientation::Horizontal
            }
            super::delegate::TabBarOrientation::Vertical => {
                teksilo_core::styles::TabBarOrientation::Vertical
            }
        };

        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeTabStyle` default. The style wraps `inner_id`
        // with the accent indicator + focus ring chrome; the
        // per-state tab background (which the trait config doesn't
        // carry through) stays as a RectWidget sibling underneath.
        let style: SharedTabStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.tab.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeTabStyle::default()));

        let cfg = TabStyleConfig {
            label: inner_id,
            leading: None,
            trailing: None,
            is_active,
            is_hovered,
            is_focused,
            is_disabled,
            orientation: orientation_for_cfg,
            indicator_position: self.active_indicator,
        };
        let chrome_id = style.make_body(&cfg, ctx);

        // Per-state background painted under the chrome. Each state
        // (selected / hover / idle) resolves to its own override, else the
        // `tab_background` shorthand, else transparent. When *any* of the
        // four are set we mount three flush `RectWidget`s in a back ZStack,
        // each shown only in its state via `ctx.visible_when` (RepaintOnly —
        // no rebuild, mirroring the hover close-button) so selection /
        // hover changes just toggle which rect paints. Adjacent tabs sit
        // flush, so no rounded corners.
        let any_bg = self.selected_tab_background.is_some()
            || self.hover_tab_background.is_some()
            || self.idle_tab_background.is_some()
            || self.tab_background.is_some();
        let root_id = if any_bg {
            let shorthand = &self.tab_background;
            let effective = |state: &Option<teksilo_core::color_prop::ColorProp>| {
                state
                    .clone()
                    .or_else(|| shorthand.clone())
                    .unwrap_or_else(|| teksilo_tokens::SurfaceRole::Transparent.into())
            };
            let index_for_bg = self.index;
            // Mutually-exclusive state gates (selected wins over hover).
            let is_selected = self.selected.map(move |sel| *sel == index_for_bg);
            let is_hover_only = self.selected.zip(&interaction).map(move |(sel, inter)| {
                *sel != index_for_bg && matches!(*inter, TabHeaderInteraction::Hovered)
            });
            let is_idle = self.selected.zip(&interaction).map(move |(sel, inter)| {
                *sel != index_for_bg && !matches!(*inter, TabHeaderInteraction::Hovered)
            });

            let sel_bg =
                ctx.add(RectWidget::new().background(effective(&self.selected_tab_background)));
            ctx.visible_when(sel_bg, is_selected);
            let hov_bg =
                ctx.add(RectWidget::new().background(effective(&self.hover_tab_background)));
            ctx.visible_when(hov_bg, is_hover_only);
            let idle_bg =
                ctx.add(RectWidget::new().background(effective(&self.idle_tab_background)));
            ctx.visible_when(idle_bg, is_idle);

            // The chrome (a `ZStack[indicator-painter, label-row]`) reports
            // its *content* width via `layout_response` — a plain `ZStack`
            // wrapper would size it to the label and CENTER it, dragging the
            // leading accent indicator inward by `(tab_width - label_width)/2`
            // and making the indicator's x drift per tab as labels differ.
            // Wrap the chrome in `Expand` so it fills the full tab bounds
            // (the indicator stays pinned to its edge).
            //
            // The three background rects are **direct** children of this
            // outer `ZStack`, NOT nested in an inner one: `ZStack` sizes to
            // its children's *intrinsic* size, and a `RectWidget` reports
            // `0×0` intrinsic, so an inner stack would collapse to zero and
            // the selected / hover fills would never paint. As direct
            // children each rect is queried with the exact bounds proposal
            // and fills; `visible_when` (RepaintOnly) shows exactly one.
            let filled_chrome = ctx.add(Expand::new().child_id(chrome_id));
            ctx.add(
                ZStack::new()
                    .add_child(sel_bg)
                    .add_child(hov_bg)
                    .add_child(idle_bg)
                    .add_child(filled_chrome),
            )
        } else {
            chrome_id
        };
        self.inner_root_id = Some(root_id);

        // Attach handlers: tap selects, hover updates state, focus
        // tracks origin, key handles arrow-nav + Enter/Space. The
        // framework gates events on `arena.is_enabled(self_id)`, so
        // no `if !enabled` guards inside handlers.
        let index = self.index;
        let selected = self.selected.clone();
        let header_ids = self.shared.header_ids.clone();
        let panel_ids = self.shared.panel_ids.clone();
        let enabled_tabs = self.shared.enabled_tabs.clone();
        let interaction_for_hover = interaction.clone();
        let focused_for_handler = focused.clone();

        let mut handler_set = HandlerSet::new()
            .on_tap(move |_event, _ctx: &mut EventContext| {
                selected.set(index);
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                interaction_for_hover.set(if entered {
                    TabHeaderInteraction::Hovered
                } else {
                    TabHeaderInteraction::Idle
                });
            })
            .on_focus({
                // Track raw focus only; the keyboard/pointer distinction is
                // derived live from the input-modality signal in `build()`
                // (`:focus-visible`).
                let focused = focused_for_handler.clone();
                move |gained: bool, _ctx: &mut EventContext| {
                    focused.set(gained);
                }
            })
            .on_key({
                let selected = self.selected.clone();
                let header_ids = header_ids.clone();
                let panel_ids = panel_ids.clone();
                let enabled_tabs = enabled_tabs.clone();
                let on_close = self.on_close.clone();
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    let headers = header_ids.borrow();
                    if headers.is_empty() {
                        return EventResponse::Ignored;
                    }
                    // ArrowLeft/Up = previous tab, ArrowRight/Down =
                    // next. We accept both axes regardless of bar
                    // orientation — pressing ArrowLeft on a vertical
                    // bar is harmless (no headers to the side; the
                    // user just gets the prev neighbor) and lets the
                    // bar work without orientation-specific wiring at
                    // the header level.
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::ArrowRight | Key::ArrowDown,
                            ..
                        } => {
                            let next = next_enabled_index(&enabled_tabs, index, 1);
                            selected.set(next);
                            ctx.request_focus(headers[next]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowLeft | Key::ArrowUp,
                            ..
                        } => {
                            let prev = next_enabled_index(&enabled_tabs, index, -1);
                            selected.set(prev);
                            ctx.request_focus(headers[prev]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown { key: Key::Home, .. } => {
                            let Some(target) = first_enabled_index(&enabled_tabs) else {
                                return EventResponse::Ignored;
                            };
                            selected.set(target);
                            ctx.request_focus(headers[target]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown { key: Key::End, .. } => {
                            let Some(target) = last_enabled_index(&enabled_tabs) else {
                                return EventResponse::Ignored;
                            };
                            selected.set(target);
                            ctx.request_focus(headers[target]);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Delete, ..
                        } => {
                            // Close the tab if closable (APG-recommended
                            // optional binding; mirrors browser
                            // convention). Selection adjustment is the
                            // close callback's responsibility.
                            let Some(close) = on_close.as_ref() else {
                                return EventResponse::Ignored;
                            };
                            (close)(ctx);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            // Activate the tab AND move focus *into* its content
                            // panel. Both Enter and Space dive in — the desktop
                            // tab-control convention screen-reader users
                            // encounter (Windows/JAWS: Space/Enter "invoke" a
                            // tab and a well-built control sets focus to the
                            // start of the panel) and the Spacebar/Enter
                            // keyboard-parity guidance for invocable controls.
                            // Activation is idempotent in the
                            // automatic-activation model (the focused header is
                            // already the selected one), so the meaningful
                            // effect is jumping focus to the panel's first
                            // focusable descendant without hunting for the Tab
                            // stop. `request_focus_into` is a no-op when the
                            // panel has no focusable content, so a content-less
                            // tab keeps focus on its header.
                            selected.set(index);
                            if let Some(&panel_id) = panel_ids.borrow().get(index) {
                                ctx.request_focus_into(panel_id);
                            }
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_access_action_request({
                let selected = self.selected.clone();
                let on_reorder_to = self.on_reorder_to.clone();
                let header_ids_for_action = self.shared.header_ids.clone();
                move |action, _node, data, ctx: &mut EventContext| -> EventResponse {
                    use teksilo_core::accesskit::{Action, ActionData};
                    match action {
                        Action::Click => {
                            selected.set(index);
                            // AT-invoked click doesn't move keyboard focus, so
                            // the framework's focus-driven follow won't reveal
                            // this header. Explicitly chase it into the strip's
                            // scroll area (and any enclosing scroller). Keyboard
                            // nav and pointer clicks already move focus, so they
                            // get the reveal for free.
                            if let Some(&hid) = header_ids_for_action.borrow().get(index) {
                                ctx.ensure_widget_visible(hid);
                            }
                            EventResponse::Handled
                        }
                        Action::CustomAction => {
                            let Some(reorder) = on_reorder_to.as_ref() else {
                                return EventResponse::Ignored;
                            };
                            // Custom action indices we advertise:
                            //   0 = Move Left  (index → index - 1)
                            //   1 = Move Right (index → index + 1)
                            let Some(ActionData::CustomAction(idx)) = data else {
                                return EventResponse::Ignored;
                            };
                            let total = header_ids_for_action.borrow().len();
                            match idx {
                                0 if index > 0 => {
                                    reorder(index - 1, ctx);
                                    EventResponse::Handled
                                }
                                1 if index + 1 < total => {
                                    reorder(index + 1, ctx);
                                    EventResponse::Handled
                                }
                                _ => EventResponse::Ignored,
                            }
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            // The focus walker skips disabled subtrees on its own
            // (see `find_focusable_at_or_above`), so we set
            // `focusable(true)` unconditionally — the static intent
            // is "this header takes keyboard focus" and the arena
            // gates whether it actually does.
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        // Drag source: publish the bar-built payload when the user
        // starts dragging this header. The factory carries the source
        // identity (and, for cross-bar transfer, a clone of the item).
        // The bar's `on_drop` matches `source_bar_id` to tell an
        // intra-bar reorder from a foreign drop.
        if let Some(make_payload) = self.make_drag_payload.clone() {
            let label_for_preview = self.label.clone();
            handler_set = handler_set.on_drag(move |phase, ctx| {
                if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                    let payload = (make_payload)();
                    let preview_inner: Box<dyn teksilo_core::widget::Widget> = Box::new(
                        crate::Padding::symmetric(HEADER_PADDING_V, INNER_GAP * 2.0)
                            .child(TextWidget::new(label_for_preview.clone()).single_line()),
                    );
                    // Hardcoded preview footprint — the live tab is
                    // not a known size at handler time (no
                    // LayoutContext). 160×32 dp matches the average
                    // tab footprint and keeps the preview legible.
                    let preview = crate::drag_preview::DragPreview::new(160.0, 32.0, preview_inner);
                    ctx.start_drag_with_preview(self_id, payload, Box::new(preview));
                }
            });
        }

        // Drag completion (cross-bar transfer-out): fire the bar's
        // on_transfer_out when one of our tabs was accepted by another
        // bar. The bar-built handler already filters on the outcome
        // and suppresses intra-bar reorders.
        if let Some(on_drag_ended) = self.on_drag_ended.clone() {
            handler_set =
                handler_set.on_drag_ended(move |outcome, ctx| (on_drag_ended)(outcome, ctx));
        }

        // Middle-click closes the tab on Up (Firefox / Chrome
        // convention). Non-Primary activation is already filtered at
        // the framework level: `TapRecognizer` defaults to
        // `ButtonMask::PRIMARY`, so a right-click or middle-click
        // press never fires the inner `on_tap`. The `on_pointer_event`
        // handler here only adds the close-on-middle-up behaviour.
        if let Some(close_fn) = self.on_close.clone() {
            handler_set = handler_set.on_pointer_event(move |event, ctx| match event {
                WidgetEvent::PointerUp {
                    button: PointerButton::Middle,
                    ..
                } => {
                    (close_fn)(ctx);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        if let Some(factory) = self.context_menu_factory.clone() {
            handler_set = handler_set.context_menu(move |pos, ctx| (factory)(pos, ctx));
        }

        ctx.apply_self_handlers(handler_set);

        // Attach tooltip via the framework helper. Three
        // mutually-exclusive sources (composite > rich > plain).
        // A vertical tab strip stacks its tabs top-to-bottom, so the
        // tooltip opens to the trailing `Side` (over the content area) —
        // a `Below` tooltip would cover the next tab down. A horizontal
        // strip keeps the default `Below`.
        let tip_placement = match self.orientation {
            super::delegate::TabBarOrientation::Vertical => crate::tooltip::TooltipPlacement::Side,
            super::delegate::TabBarOrientation::Horizontal => {
                crate::tooltip::TooltipPlacement::Below
            }
        };
        if let Some(content) = self.composite_tooltip.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed_with_placement(
                ctx,
                self_id,
                content,
                delay,
                tip_placement,
            );
        } else if let Some(source) = self.rich_tooltip.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source_with_placement(
                ctx,
                self_id,
                source,
                delay,
                tip_placement,
            );
        } else if let Some(tip) = self.tooltip.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_plain_tooltip_with_placement(
                ctx,
                self_id,
                tip,
                delay,
                tip_placement,
            );
        }

        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let height = proposal
            .height
            .unwrap_or_else(|| Self::intrinsic_height(ctx));
        // Pinned tabs are always rendered at exactly `min_width`
        // (the bar's `pinned_tab_width`), regardless of any width
        // proposal — they're icon-only squares and shouldn't stretch.
        let width = if self.pinned {
            self.min_width
        } else {
            match proposal.width {
                Some(w) => w,
                None => {
                    let natural = self.estimate_natural_width(ctx);
                    if self.is_icon_only() {
                        // Size to the icon; the text-tab minimum doesn't apply.
                        natural.min(self.max_width)
                    } else {
                        natural.clamp(self.min_width, self.max_width)
                    }
                }
            }
        };
        Size::new(width, height).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Fill the full bounds — adjacent tab headers sit flush
        // against each other (Firefox / Chrome convention; the
        // accent indicator + per-state surface fill carry the
        // selected/idle distinction without needing a gap). The
        // focus ring is painted *inside* `bounds` (inset by
        // `focus_ring_width / 2`), so it never bleeds into the
        // neighbour tab.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::Tab);
        // The accessible name is the *original* title (`at_name`), not the
        // visible `label` — so an icon-only tab (whose visible label is blanked)
        // is still announced by name. Fall back to the plain tooltip, then the
        // visible label. A locale change refreshes this via the AccessibilityOnly
        // locale binding installed in `build()`.
        let mut name = self.at_name.clone().resolve_now();
        if name.trim().is_empty() {
            name = self
                .tooltip
                .as_ref()
                .map(|t| t.clone().resolve_now())
                .filter(|s| !s.trim().is_empty())
                .or_else(|| self.label_signal.as_ref().map(|s| s.get()))
                .unwrap_or_else(|| self.label.clone().resolve_now());
        }
        builder.set_name(&name);
        // Framework a11y walker auto-emits `set_disabled()` when
        // `arena.is_enabled(self_id) == false`. We base advertised
        // actions on the structural per-tab flag so a disabled tab
        // exposes no `Click` action to AT.
        if self.initial_enabled {
            builder.add_action(teksilo_core::accesskit::Action::Click);
        }
        builder.add_action(teksilo_core::accesskit::Action::Focus);
        builder.set_selected(self.selected.get() == self.index);

        // ARIA aria-posinset — "tab 3 of 5". `self.index` is the unified
        // index across pinned + regular tabs, which share one TabList (the bar
        // emits one Role::TabList parent).
        //
        // The "of 5" half lives on that TabList, not here: unlike
        // `aria-setsize`, AccessKit's `size_of_set` belongs on the container,
        // and `size_of_set_from_container` walks up from an item — so a count
        // written on this node would be read by no adapter on any platform.
        let set_size = self.shared.header_ids.borrow().len();
        if set_size > 0 {
            builder.set_position_in_set(self.index + 1);
        }

        if let Some(&panel_id) = self.shared.panel_ids.borrow().get(self.index) {
            builder.push_controlled(teksilo_core::accessibility::widget_id_to_node_id(panel_id));
        }

        // Advertise reorder custom actions for AT users who can't drag.
        // Order matters: index 0 = "Move Left/Up", index 1 = "Move
        // Right/Down" — `on_access_action_request` reads the index
        // from `ActionData::CustomAction(idx)` and routes accordingly.
        // Suppressed for pinned tabs (whose order is conceptually fixed
        // by the pinned-strip layout — Firefox convention).
        if self.on_reorder_to.is_some() && self.initial_enabled && !self.pinned {
            let total = self.shared.header_ids.borrow().len();
            // Orientation-aware labels: a vertical bar's "Move Left"
            // would mislead a screen reader user. `LocalizedString`
            // resolves now; the locale signal binding in build()
            // dirties the AT cache on locale change so these refresh.
            let (prev_label, next_label) = match self.orientation {
                super::delegate::TabBarOrientation::Horizontal => (
                    lit!("Move Left").resolve_now(),
                    lit!("Move Right").resolve_now(),
                ),
                super::delegate::TabBarOrientation::Vertical => (
                    lit!("Move Up").resolve_now(),
                    lit!("Move Down").resolve_now(),
                ),
            };
            let mut actions = Vec::with_capacity(2);
            if self.index > 0 {
                actions.push(teksilo_core::accesskit::CustomAction {
                    id: 0,
                    description: prev_label,
                });
            }
            if self.index + 1 < total {
                actions.push(teksilo_core::accesskit::CustomAction {
                    id: 1,
                    description: next_label,
                });
            }
            if !actions.is_empty() {
                builder.add_action(teksilo_core::accesskit::Action::CustomAction);
                builder.set_custom_actions(actions);
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner_root_id.into_iter().collect()
    }
}

pub(crate) fn next_enabled_index(enabled_tabs: &[bool], current: usize, direction: isize) -> usize {
    if enabled_tabs.is_empty() {
        return current;
    }
    let len = enabled_tabs.len() as isize;
    let mut offset = 1_isize;
    while offset <= len {
        let candidate = (current as isize + direction * offset).rem_euclid(len) as usize;
        if enabled_tabs[candidate] {
            return candidate;
        }
        offset += 1;
    }
    current
}

/// First enabled tab in `enabled_tabs`, scanning from index 0.
/// Returns `None` if every tab is disabled.
pub(crate) fn first_enabled_index(enabled_tabs: &[bool]) -> Option<usize> {
    enabled_tabs.iter().position(|&e| e)
}

/// Last enabled tab in `enabled_tabs`, scanning from the end.
/// Returns `None` if every tab is disabled.
pub(crate) fn last_enabled_index(enabled_tabs: &[bool]) -> Option<usize> {
    enabled_tabs.iter().rposition(|&e| e)
}
