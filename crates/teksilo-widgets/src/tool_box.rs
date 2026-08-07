// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! ToolBox — a vertical stack of collapsible sections, exactly one expanded
//! at a time.
//!
//! Semantic cousin of Qt's `QToolBox` and the collapsible groups in
//! IntelliJ's Settings dialog. Differs from [`Accordion`](crate::Accordion)
//! (single-item independent disclosure) and [`TabWidget`](crate::TabWidget)
//! (horizontal tab bar with dormant panes) by combining vertical layout,
//! always-visible headers, and exclusive expansion in one widget.
//!
//! Int UI visual language:
//! - flat, borderless headers (no corner radius)
//! - 1 dp accent indicator bar on the leading edge of the active header
//! - color-only emphasis (selected / hover / pressed surface roles)
//! - border IS the focus ring: 1 dp accent border appears on the focused
//!   header, no separate ring primitive
//! - content swaps are **instant** — Int UI's house rule is to avoid
//!   decorative animation for inline transitions; see
//!   [`MotionTokens`](teksilo_tokens::MotionTokens). Matches the existing
//!   [`TabWidget`](crate::TabWidget) precedent where pane swaps have no
//!   transition.
//!
//! ```ignore
//! let selected = ctx.signal(0_usize);
//! ToolBox::new(selected.clone())
//!     .item("Outline",    outline_widget)
//!     .item("Properties", properties_widget)
//!     .add(ToolBoxItem::new("Build", build_widget).enabled(false))
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal, Transform2D};
use teksilo_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::{ColorProp, TextStyleProp};
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;
use teksilo_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{
    Divider, FixedSize, HStack, IconWidget, MinSize, RectWidget, Spacer, TextWidget, VStack, ZStack,
};
use crate::tooltip::{
    RichTooltipSource, TooltipContent, TooltipWidget, attach_rich_tooltip_source,
};

/// Orientation of a [`ToolBox`]: how its collapsible sections are arranged.
///
/// [`Vertical`](ToolBoxOrientation::Vertical) (the default) stacks sections
/// top-to-bottom with horizontal headers and an up/down chevron — the
/// classic `QToolBox`. [`Horizontal`](ToolBoxOrientation::Horizontal) lays
/// sections left-to-right; each header becomes a narrow **vertical strip**
/// with its label rotated 90° and a left/right chevron. The horizontal form
/// is used by side-docks anchored to the top/bottom edges (where the wide,
/// short region calls for vertical header strips).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolBoxOrientation {
    /// Sections stacked top-to-bottom; horizontal headers (default).
    #[default]
    Vertical,
    /// Sections arranged left-to-right; vertical header strips with
    /// rotated labels and left/right chevrons.
    Horizontal,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// One section of a [`ToolBox`]. Construct with [`ToolBoxItem::new`] and pass
/// to [`ToolBox::add`], or use the convenience [`ToolBox::item`] /
/// [`ToolBox::item_id`] builders directly when leading / trailing slots
/// and tooltip are not needed.
///
/// Layout of the header row:
///
/// ```text
/// [indicator] [leading?] [label] [spacer] [trailing?] [chevron]
/// ```
///
/// Both `leading` and `trailing` accept any `impl Widget` — typical uses
/// are a small `IconWidget`, a `Checkbox` (checkable section), a
/// `Badge` (count), or a `Button` (per-row action).
pub struct ToolBoxItem {
    label: LocalizedString,
    leading: Option<Box<dyn Widget>>,
    trailing: Option<Box<dyn Widget>>,
    /// Plain-text tooltip body. Mutually exclusive with `rich_tooltip` and
    /// `composite_tooltip_content` (the last tooltip setter called wins).
    /// Kept as a `LocalizedString` so a `tr!(...)` source stays locale-reactive.
    tooltip_text: Option<LocalizedString>,
    /// Rich (registry-key or inline `TooltipContent`) tooltip.
    rich_tooltip: Option<RichTooltipSource>,
    /// Composite (arbitrary widget body) tooltip. Mutually exclusive with the
    /// plain and rich variants — the last setter called wins.
    composite_tooltip_content: Option<Box<dyn Widget>>,
    content: PendingChild,
    /// Enabled state, static or reactive. Forwarded into the arena via
    /// `ctx.enabled_when(header_id, self.enabled.clone())` at build time.
    /// After build the arena is the single source of truth and ANDs
    /// with ancestors — so a disabled `ToolBox` ancestor disables every
    /// item header regardless of its own `enabled`.
    enabled: Prop<bool>,
}

impl std::fmt::Debug for ToolBoxItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolBoxItem")
            .field("label", &self.label)
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

impl ToolBoxItem {
    /// Build an item with an inline content widget. The label may come from
    /// `tr!(...)` (translated) or `lit!(...)`.
    pub fn new(label: impl Into<LocalizedString>, content: impl Widget + 'static) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            leading: None,
            trailing: None,
            tooltip_text: None,
            rich_tooltip: None,
            composite_tooltip_content: None,
            content: PendingChild::Deferred(Box::new(content)),
            enabled: Prop::Static(true),
        }
    }

    /// Build an item whose content is a pre-registered widget id.
    pub fn new_id(label: impl Into<LocalizedString>, content_id: WidgetId) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            leading: None,
            trailing: None,
            tooltip_text: None,
            rich_tooltip: None,
            composite_tooltip_content: None,
            content: PendingChild::Id(content_id),
            enabled: Prop::Static(true),
        }
    }

    /// Attach a leading-slot widget rendered before the label (after
    /// the selection indicator bar). Use for a small `IconWidget`, a
    /// `Checkbox` for checkable sections, a `Badge`, or any other
    /// label-sized widget. The slot widget owns its own events — a
    /// `Checkbox` inside the leading slot toggles independently of
    /// the header's own tap.
    pub fn leading(mut self, widget: impl Widget + 'static) -> Self {
        self.leading = Some(Box::new(widget));
        self
    }

    /// Attach a trailing-slot widget rendered between the row's flexible
    /// spacer and the chevron. Use for per-row actions — a dismiss
    /// button, a badge, a secondary `Toggle`. The slot widget owns its
    /// own events: tapping a `Button` inside the trailing slot fires the
    /// button's action; gesture recognisers on the trailing widget stop
    /// the header's own tap from firing, so a close-button click does
    /// not also select the section.
    pub fn trailing(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing = Some(Box::new(widget));
        self
    }

    /// Attach a plain-text tooltip shown after a hover delay on the header
    /// row. The text may come from `tr!(...)` (translated, locale-reactive)
    /// or `lit!(...)`. Mirrors `.tooltip(...)` on Button / IconButton /
    /// MenuItem. Clears any previously set rich or composite tooltip (the
    /// last tooltip setter called wins).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide
    /// [`TooltipRegistry`](crate::tooltip::TooltipRegistry) by key.
    /// Clears any previously set plain or composite tooltip (the last
    /// tooltip setter called wins).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip = Some(RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline [`TooltipContent`] — for
    /// one-offs that don't belong in the registry. Clears any previously
    /// set plain or composite tooltip (the last tooltip setter called wins).
    pub fn rich_tooltip_content(mut self, content: TooltipContent) -> Self {
        self.rich_tooltip = Some(RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip — an arbitrary `impl Widget` body shown
    /// in a larger, scrollable overlay after a longer hover delay. Use for
    /// rich on-demand previews: charts, property tables, image thumbnails.
    /// Clears any previously set plain or rich tooltip (the last tooltip
    /// setter called wins).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip = None;
        self
    }

    /// Disable the item: its header renders in the disabled text role,
    /// click and keyboard activation are ignored, and arrow navigation
    /// skips it. Accepts a static bool or a reactive `Signal<bool>`.
    ///
    /// Forwarded to the arena via
    /// `ctx.enabled_when(header_id, self.enabled.clone())` at build time;
    /// the arena is then the single source of truth and ANDs with
    /// ancestors — disabling the surrounding `ToolBox` (or any ancestor)
    /// disables every item regardless of this flag.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }
}

/// ToolBox design tokens.
pub const TOOL_BOX_HEADER_MIN_HEIGHT: f32 = 28.0;
pub const TOOL_BOX_HEADER_PADDING_HORIZONTAL: f32 = 12.0;
pub const TOOL_BOX_ICON_TEXT_SPACING: f32 = 8.0;
pub const TOOL_BOX_CHEVRON_SIZE: f32 = 12.0;
pub const TOOL_BOX_INDICATOR_THICKNESS: f32 = 1.0;

/// `selected` value meaning "no section open" — used only in
/// [`ToolBox::collapsible`] mode (every section collapsed). Out of range of any
/// real index, so [`ToolBoxPanel`] treats every panel as inactive.
const COLLAPSED_SENTINEL: usize = usize::MAX;

/// A vertical container of collapsible sections with exactly one expanded
/// at a time — the Int UI / `QToolBox` pattern.
///
/// The active section is driven by a caller-owned `Signal<usize>`; mirrors
/// [`TabWidget::new`](crate::TabWidget::new) so persistence, synchronised
/// windows, and programmatic activation work identically.
pub struct ToolBox {
    selected: Signal<usize>,
    items: Vec<ToolBoxItem>,
    show_dividers: bool,
    orientation: ToolBoxOrientation,
    /// When set, the active section's panel **fills** the ToolBox's allotted
    /// space instead of sizing to its content's natural extent. See
    /// [`ToolBox::fill`].
    fill: bool,
    /// When set, clicking the **active** header collapses it (all sections may
    /// be closed at once). See [`ToolBox::collapsible`].
    collapsible: bool,
    /// Optional drag-source hook: when set, each section header becomes a
    /// drag source. Fired (with the section index) when a drag gesture
    /// *starts* on a header — the callback typically calls
    /// `ctx.start_drag(...)`. Tap-to-select still works (the gesture arena
    /// disambiguates a tap from a drag).
    header_drag: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    root_child_id: Option<WidgetId>,
}

impl ToolBox {
    /// Create a ToolBox driven by `selected` (visible section index). Set the
    /// signal to `0` to open the first section by default; modify it
    /// programmatically or share it across windows for synchronized state.
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            items: Vec::new(),
            show_dividers: false,
            orientation: ToolBoxOrientation::Vertical,
            fill: false,
            collapsible: false,
            header_drag: None,
            root_child_id: None,
        }
    }

    /// Set the section arrangement orientation (default
    /// [`ToolBoxOrientation::Vertical`]).
    pub fn orientation(mut self, orientation: ToolBoxOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Make the active section's panel **fill** the ToolBox's allotted space
    /// rather than size to its content's natural extent.
    ///
    /// With `fill` on, the active panel stretches to the full cross axis and
    /// flexes / shrinks (and clips) along the main axis, so a ToolBox placed
    /// in a bounded region lays its content out at *exactly* the available
    /// size — the `QToolBox` convention. A panel whose content carries a
    /// trailing `Spacer` therefore pins a bottom toolbar to the visible
    /// bottom edge instead of overflowing past it.
    ///
    /// Default `false` (the panel keeps its content's natural size — the
    /// historical behaviour, appropriate when the ToolBox itself lives inside
    /// a scroll area).
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    /// Allow **collapsing** the active section: clicking (or Enter/Space on, or
    /// the AT `Collapse` action of) the already-expanded header closes it, so
    /// *all* sections can be collapsed at once. A subsequent click re-expands.
    ///
    /// Default `false` — the classic "exactly one section open" behaviour. This
    /// is what makes a **single-section** ToolBox a plain collapsible panel
    /// (header toggles its content), e.g. a dock panel.
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = collapsible;
        self
    }

    /// Shorthand for [`ToolBox::orientation`]`(`[`ToolBoxOrientation::Horizontal`]`)`.
    pub fn horizontal(mut self) -> Self {
        self.orientation = ToolBoxOrientation::Horizontal;
        self
    }

    /// Make each section header a drag source. `f` is invoked (with the
    /// section index) when a drag gesture *starts* on a header; it should
    /// begin a drag (e.g. `ctx.start_drag(source, payload)`). Tapping a
    /// header still selects it — the gesture arena tells a tap from a drag.
    pub fn on_header_drag(mut self, f: impl Fn(usize, &mut EventContext) + 'static) -> Self {
        self.header_drag = Some(Rc::new(f));
        self
    }

    /// Append an item with an inline content widget. Convenience wrapper
    /// around [`ToolBox::add`] that skips the [`ToolBoxItem`] builder for
    /// the common label-plus-content case.
    pub fn item(self, label: impl Into<LocalizedString>, content: impl Widget + 'static) -> Self {
        self.add(ToolBoxItem::new(label, content))
    }

    /// Append an item whose content is a pre-registered widget id.
    pub fn item_id(self, label: impl Into<LocalizedString>, content_id: WidgetId) -> Self {
        self.add(ToolBoxItem::new_id(label, content_id))
    }

    /// Append a fully-built [`ToolBoxItem`] — required when an icon,
    /// tooltip, or disabled flag is needed.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, item: ToolBoxItem) -> Self {
        self.items.push(item);
        self
    }

    /// Append multiple items from an iterator.
    pub fn items<I>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = ToolBoxItem>,
    {
        self.items.extend(items);
        self
    }

    /// Show a 1 dp `BorderRole::Divider` line between consecutive header /
    /// panel rows. Default: `false` — IntelliJ Settings-style collapsibles
    /// stack without explicit dividers, letting the flat background roles
    /// delineate the rows.
    pub fn show_dividers(mut self, show: bool) -> Self {
        self.show_dividers = show;
        self
    }
}

impl std::fmt::Debug for ToolBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolBox")
            .field("items", &self.items.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Keyboard navigation helpers — mirror `next_enabled_index` in tab_widget.rs
// ---------------------------------------------------------------------------

fn next_enabled_index(enabled: &[bool], current: usize, direction: isize) -> usize {
    if enabled.is_empty() {
        return current;
    }
    let len = enabled.len() as isize;
    let mut offset = 1_isize;
    while offset <= len {
        let candidate = (current as isize + direction * offset).rem_euclid(len) as usize;
        if enabled[candidate] {
            return candidate;
        }
        offset += 1;
    }
    current
}

fn first_enabled_index(enabled: &[bool]) -> Option<usize> {
    enabled.iter().position(|&e| e)
}

fn last_enabled_index(enabled: &[bool]) -> Option<usize> {
    enabled.iter().rposition(|&e| e)
}

// ---------------------------------------------------------------------------
// ToolBoxHeader — one button-like row per item
// ---------------------------------------------------------------------------

struct ToolBoxHeader {
    label: LocalizedString,
    index: usize,
    /// Structural per-item enabled flag. Forwarded into the arena at
    /// build time; the arena is then the single source of truth (events,
    /// focus, a11y `set_disabled`, leaf role-substitution all consult
    /// `arena.is_enabled(self_id)` / `PaintContext::effective_enabled`).
    /// Kept on the struct only so `accessibility()` can decide whether
    /// to advertise the `Click` / `Expand` / `Collapse` actions for the
    /// structural-disabled case.
    initial_enabled: bool,
    selected: Signal<usize>,
    /// Shared ordered list of header widget ids. Populated by
    /// [`ToolBox::build`] as each header is registered. Headers read this to
    /// focus siblings from the arrow-key / Home / End handlers.
    header_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// Shared ordered list of panel widget ids, used to publish the
    /// ARIA `controls` relation in [`ToolBoxHeader::accessibility`].
    panel_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// One entry per item — `true` if that header is structurally
    /// enabled (per its own `initial_enabled`). Used by arrow / Home /
    /// End navigation to skip structurally-disabled siblings. Ancestor-
    /// driven disable cascades through the arena and the focus walker,
    /// so it doesn't need to be re-evaluated here.
    enabled_flags: Rc<Vec<bool>>,
    pending_leading: Option<Box<dyn Widget>>,
    pending_trailing: Option<Box<dyn Widget>>,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip: Option<RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn Widget>>,
    orientation: ToolBoxOrientation,
    /// When set, clicking the active header collapses it (see
    /// [`ToolBox::collapsible`]).
    collapsible: bool,
    on_header_drag: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for ToolBoxHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolBoxHeader")
            .field("index", &self.index)
            .field("orientation", &self.orientation)
            .field("draggable", &self.on_header_drag.is_some())
            .finish()
    }
}

impl ToolBoxHeader {
    #[allow(clippy::too_many_arguments)]
    fn new(
        label: LocalizedString,
        index: usize,
        initial_enabled: bool,
        selected: Signal<usize>,
        header_ids: Rc<RefCell<Vec<WidgetId>>>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
        enabled_flags: Rc<Vec<bool>>,
        pending_leading: Option<Box<dyn Widget>>,
        pending_trailing: Option<Box<dyn Widget>>,
        tooltip_text: Option<LocalizedString>,
        rich_tooltip: Option<RichTooltipSource>,
        composite_tooltip_content: Option<Box<dyn Widget>>,
        orientation: ToolBoxOrientation,
        collapsible: bool,
        on_header_drag: Option<Rc<dyn Fn(usize, &mut EventContext)>>,
    ) -> Self {
        Self {
            label,
            index,
            initial_enabled,
            selected,
            header_ids,
            panel_ids,
            enabled_flags,
            pending_leading,
            pending_trailing,
            tooltip_text,
            rich_tooltip,
            composite_tooltip_content,
            orientation,
            collapsible,
            on_header_drag,
            root_child_id: None,
        }
    }
}

impl Widget for ToolBoxHeader {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let theme = ctx.theme();
        let focus_ring_width = theme.shape.focus_ring_width;

        let idx = self.index;
        // Forward the structural per-item enabled hint into the arena.
        // After this point the arena is the single source of truth:
        // events are gated by `arena.is_enabled(self_id)`, the focus
        // walker skips disabled subtrees, the a11y walker auto-emits
        // `set_disabled()`, and the leaf widgets (TextWidget /
        // IconWidget for label + chevron) substitute `TextRole::Disabled`
        // at paint time via `PaintContext::effective_enabled`.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // Derived read-only signal: am I the active section?
        let is_selected = self.selected.map(move |s| *s == idx);

        // Interaction state — Hovered / Pressed / Idle. `selected` is
        // orthogonal: a selected-but-not-hovered header is still `Idle`
        // here, with `sel = true` driving the selected-surface branch in
        // the role-resolver below.
        let interaction = ctx.signal(HeaderInteraction::Idle);
        // Track focus origin so the focus border only appears when focus
        // was gained via the keyboard — pointer clicks move focus here but
        // must not show the ring. Same pattern as `TabHeader`
        // ([tab_widget.rs:259-278]) and used by SegmentedControl/Slider/Toggle.
        let focus_origin: Signal<Option<teksilo_core::focus::FocusOrigin>> = ctx.signal(None);

        let registry = ctx.binding_registry();
        self.selected
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);
        interaction.bind_to(self_id, registry, BindingLevel::RepaintOnly);
        focus_origin.bind_to(self_id, registry, BindingLevel::RepaintOnly);

        // Derived roles — Signal<SurfaceRole> / Signal<TextRole> (see
        // CLAUDE.md "Theming"). No `enabled` branch: the leaves
        // (TextWidget for the label, IconWidget for the chevron)
        // consult `PaintContext::effective_enabled` and substitute
        // `TextRole::Disabled` themselves. `SurfaceRole` has no
        // `Disabled` token by design — a disabled header simply
        // renders with its idle background (Transparent / Hover /
        // Selected per interaction).
        let bg_role = interaction.zip(&is_selected).map(move |(state, sel)| {
            if *state == HeaderInteraction::Pressed {
                return SurfaceRole::Pressed;
            }
            if *sel {
                return SurfaceRole::Selected;
            }
            if *state == HeaderInteraction::Hovered {
                return SurfaceRole::Hover;
            }
            SurfaceRole::Transparent
        });
        let text_role = interaction.zip(&is_selected).map(move |(state, sel)| {
            if *sel || *state == HeaderInteraction::Hovered {
                return TextRole::Primary;
            }
            TextRole::Secondary
        });

        // Int UI: the border IS the focus ring. Rest-state border is
        // width-zero and transparent; on *keyboard* focus it snaps to
        // `focus_ring_width` with the accent colour. A `Pointer` focus
        // origin leaves the ring hidden.
        let focus_border_width = focus_origin.map(move |o| match o {
            Some(teksilo_core::focus::FocusOrigin::Keyboard) => focus_ring_width,
            _ => 0.0,
        });
        let focus_border_color = focus_origin.map(|o| match o {
            Some(teksilo_core::focus::FocusOrigin::Keyboard) => BorderRole::Focused,
            _ => BorderRole::Transparent,
        });

        // Leading 1 dp indicator: accent fill when selected, transparent
        // otherwise. Always occupies the same pixel column so labels line
        // up across selection states. `SurfaceRole::Accent` is the
        // semantic "accent solid fill" — same colour value as
        // `BorderRole::Accent` but correctly scoped as a fill.
        let indicator_bg = is_selected.map(|sel| {
            if *sel {
                SurfaceRole::Accent
            } else {
                SurfaceRole::Transparent
            }
        });
        let is_horizontal = self.orientation == ToolBoxOrientation::Horizontal;

        // Selection indicator: a 1 dp accent bar on the header's leading
        // edge — a vertical bar for a vertical toolbox, a top bar for a
        // horizontal (vertical-strip) header.
        let indicator_rect_id = ctx.add(RectWidget::new().background(indicator_bg));
        let indicator_id = if is_horizontal {
            ctx.add(
                FixedSize::new()
                    .height(TOOL_BOX_INDICATOR_THICKNESS)
                    .child_id(indicator_rect_id),
            )
        } else {
            ctx.add(
                FixedSize::new()
                    .width(TOOL_BOX_INDICATOR_THICKNESS)
                    .child_id(indicator_rect_id),
            )
        };

        // Optional leading / trailing slot widgets.
        let leading_id = self.pending_leading.take().map(|w| ctx.add_boxed(w));
        let trailing_id = self.pending_trailing.take().map(|w| ctx.add_boxed(w));
        let spacer_id = ctx.add(Spacer::new());

        // Compose the header content along the appropriate axis.
        let padded_content_id = if is_horizontal {
            // Vertical strip, top → bottom:
            //   [indicator] [leading?] [chevron L/R] [rotated label] [trailing?] [spacer]
            // Chevron points right while collapsed (content expands to the
            // trailing side) and left once expanded.
            let chevron_right_id =
                ctx.add(IconWidget::chevron_right(TOOL_BOX_CHEVRON_SIZE).color(text_role.clone()));
            let chevron_left_id =
                ctx.add(IconWidget::chevron_left(TOOL_BOX_CHEVRON_SIZE).color(text_role.clone()));
            ctx.visible_when(chevron_left_id, is_selected.clone());
            ctx.visible_when(chevron_right_id, is_selected.map(|v| !*v));
            let label_id = ctx.add(RotatedLabel::new(self.label.clone(), text_role));

            let mut col = VStack::new().spacing(TOOL_BOX_ICON_TEXT_SPACING);
            col = col.add_child(indicator_id);
            if let Some(id) = leading_id {
                col = col.add_child(id);
            }
            col = col
                .add_child(chevron_left_id)
                .add_child(chevron_right_id)
                .add_child(label_id);
            if let Some(id) = trailing_id {
                col = col.add_child(id);
            }
            col = col.add_child(spacer_id);
            let col_id = ctx.add(col);
            ctx.add(
                crate::primitives::Padding::symmetric(TOOL_BOX_HEADER_PADDING_HORIZONTAL, 0.0)
                    .child_id(col_id),
            )
        } else {
            // Horizontal row:
            //   [indicator] [leading?] [label] [spacer] [trailing?] [chevron]
            let label_id = ctx.add(
                TextWidget::new(self.label.clone())
                    .color(text_role.clone())
                    .style(TextStyleRole::Body)
                    .single_line()
                    .a11y_hidden(),
            );
            let chevron_down_id =
                ctx.add(IconWidget::chevron_down(TOOL_BOX_CHEVRON_SIZE).color(text_role.clone()));
            let chevron_right_id =
                ctx.add(IconWidget::chevron_right(TOOL_BOX_CHEVRON_SIZE).color(text_role));
            ctx.visible_when(chevron_down_id, is_selected.clone());
            ctx.visible_when(chevron_right_id, is_selected.map(|v| !*v));

            let mut row = HStack::new().spacing(TOOL_BOX_ICON_TEXT_SPACING);
            row = row.add_child(indicator_id);
            if let Some(id) = leading_id {
                row = row.add_child(id);
            }
            row = row.add_child(label_id).add_child(spacer_id);
            if let Some(id) = trailing_id {
                row = row.add_child(id);
            }
            row = row.add_child(chevron_down_id).add_child(chevron_right_id);
            let row_id = ctx.add(row);
            // The indicator sits inset by the container's padding (IntelliJ
            // Settings convention).
            ctx.add(
                crate::primitives::Padding::symmetric(0.0, TOOL_BOX_HEADER_PADDING_HORIZONTAL)
                    .child_id(row_id),
            )
        };

        // Background fills the whole header.
        let bg_rect_id = ctx.add(RectWidget::new().background(bg_role));

        // Focus-border rect is inset by half the focus stroke width on
        // every side so the centred stroke fits *entirely* inside the
        // ZStack bounds (otherwise the parent clips the outer half and the
        // ring reads as truncated).
        let focus_inset = focus_ring_width * 0.5;
        let focus_rect_id = ctx.add(
            RectWidget::new()
                .border_color(focus_border_color)
                .border_width(focus_border_width),
        );
        let focus_padded_id =
            ctx.add(crate::primitives::Padding::uniform(focus_inset).child_id(focus_rect_id));
        let zstack_id = ctx.add(
            ZStack::new()
                .add_child(bg_rect_id)
                .add_child(focus_padded_id)
                .add_child(padded_content_id),
        );

        // Enforce the Int UI 28 dp extent on the cross axis: min height
        // for a horizontal header row, min width for a vertical strip.
        let root_id = if is_horizontal {
            ctx.add(MinSize::new(TOOL_BOX_HEADER_MIN_HEIGHT, 0.0).child_id(zstack_id))
        } else {
            ctx.add(MinSize::new(0.0, TOOL_BOX_HEADER_MIN_HEIGHT).child_id(zstack_id))
        };
        self.root_child_id = Some(root_id);

        // Attach tooltip if configured. The three variants are mutually
        // exclusive (last setter on `ToolBoxItem` wins); composite takes
        // precedence over rich which takes precedence over plain.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(text) = self.tooltip_text.take() {
            let tip_id = ctx.add(TooltipWidget::new(text));
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tip_id, delay);
        }

        // --- V2 attached handlers on the header's own node ---
        let collapsible = self.collapsible;
        let selected_tap = self.selected.clone();
        let selected_key = self.selected.clone();
        let selected_access = self.selected.clone();
        let header_ids_for_key = self.header_ids.clone();
        let enabled_flags_for_key = self.enabled_flags.clone();
        let interaction_for_tap = interaction.clone();
        let interaction_for_hover = interaction.clone();
        let interaction_for_key = interaction.clone();
        let interaction_for_focus = interaction.clone();
        let focus_origin_for_focus = focus_origin.clone();

        let mut handler_set = HandlerSet::new()
            .on_tap(move |_pos, _ctx| {
                if collapsible && selected_tap.get() == idx {
                    selected_tap.set(COLLAPSED_SENTINEL);
                } else {
                    selected_tap.set(idx);
                }
                interaction_for_tap.set(HeaderInteraction::Hovered);
            })
            .on_hover(move |entered, _ctx| {
                interaction_for_hover.set(if entered {
                    HeaderInteraction::Hovered
                } else {
                    HeaderInteraction::Idle
                });
            })
            .on_focus(move |gained, _ctx| {
                if !gained {
                    focus_origin_for_focus.set(None);
                    return;
                }
                // If the pointer is over the header at the moment focus
                // arrives, the focus came from a click — record Pointer
                // so the focus border stays hidden. Otherwise treat it
                // as keyboard-driven.
                let origin = if interaction_for_focus.get() == HeaderInteraction::Hovered {
                    teksilo_core::focus::FocusOrigin::Pointer
                } else {
                    teksilo_core::focus::FocusOrigin::Keyboard
                };
                focus_origin_for_focus.set(Some(origin));
            })
            .on_key(
                move |event: &WidgetEvent, ctx: &mut EventContext| match event {
                    WidgetEvent::KeyDown {
                        key: Key::Space | Key::Enter,
                        ..
                    } => {
                        interaction_for_key.set(HeaderInteraction::Pressed);
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyUp {
                        key: Key::Space | Key::Enter,
                        ..
                    } => {
                        if collapsible && selected_key.get() == idx {
                            selected_key.set(COLLAPSED_SENTINEL);
                        } else {
                            selected_key.set(idx);
                        }
                        interaction_for_key.set(HeaderInteraction::Hovered);
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowDown,
                        ..
                    } => {
                        let headers = header_ids_for_key.borrow();
                        if headers.is_empty() {
                            return EventResponse::Ignored;
                        }
                        let next = next_enabled_index(&enabled_flags_for_key, idx, 1);
                        if next != idx {
                            ctx.request_focus(headers[next]);
                        }
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowUp, ..
                    } => {
                        let headers = header_ids_for_key.borrow();
                        if headers.is_empty() {
                            return EventResponse::Ignored;
                        }
                        let prev = next_enabled_index(&enabled_flags_for_key, idx, -1);
                        if prev != idx {
                            ctx.request_focus(headers[prev]);
                        }
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown { key: Key::Home, .. } => {
                        let headers = header_ids_for_key.borrow();
                        if let Some(first) = first_enabled_index(&enabled_flags_for_key)
                            && let Some(&target) = headers.get(first)
                        {
                            ctx.request_focus(target);
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    }
                    WidgetEvent::KeyDown { key: Key::End, .. } => {
                        let headers = header_ids_for_key.borrow();
                        if let Some(last) = last_enabled_index(&enabled_flags_for_key)
                            && let Some(&target) = headers.get(last)
                        {
                            ctx.request_focus(target);
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                },
            )
            .on_access_action(move |action, _ctx| {
                match action {
                    teksilo_core::accesskit::Action::Click
                    | teksilo_core::accesskit::Action::Expand => {
                        selected_access.set(idx);
                        EventResponse::Handled
                    }
                    teksilo_core::accesskit::Action::Collapse => {
                        // Collapsible: close the active section. Otherwise
                        // exclusive disclosure forbids collapsing the only open
                        // section — swallow.
                        if collapsible && selected_access.get() == idx {
                            selected_access.set(COLLAPSED_SENTINEL);
                        }
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            })
            // The focus walker skips disabled subtrees on its own, so
            // we set `focusable(true)` unconditionally — the static
            // intent is "this header takes keyboard focus" and the
            // arena gates whether it actually does.
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        // Drag source: when configured, a drag gesture starting on this
        // header fires the hook with the section index. Tap-to-select is
        // unaffected — the gesture arena disambiguates tap from drag.
        if let Some(drag) = self.on_header_drag.clone() {
            handler_set = handler_set.on_drag(move |phase, ctx| {
                if let teksilo_core::gesture::DragPhase::Started { .. } = phase {
                    (drag)(idx, ctx);
                }
            });
        }

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
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
        use teksilo_core::accesskit::{Action, Role};
        builder.set_role(Role::Button);
        builder.set_name(self.label.resolve_now());
        let is_active = self.selected.get() == self.index;
        builder.set_expanded(is_active);
        // Framework a11y walker auto-emits `set_disabled()` when
        // `arena.is_enabled(self_id) == false`, so we don't call it
        // here. The action set still reflects the structural
        // per-item enabled flag — a disabled header advertises no
        // Click / Expand / Collapse actions to AT.
        if self.initial_enabled {
            builder.add_action(Action::Click);
            builder.add_action(Action::Expand);
            builder.add_action(Action::Collapse);
        }
        builder.add_action(Action::Focus);
        // ARIA `controls`: this header controls the matching panel.
        if let Some(&panel_id) = self.panel_ids.borrow().get(self.index) {
            builder.push_controlled(widget_id_to_node_id(panel_id));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderInteraction {
    Idle,
    Hovered,
    Pressed,
}

// ---------------------------------------------------------------------------
// ToolBoxPanel — content wrapper that clamps height to 0 when inactive.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ToolBoxPanel {
    label: LocalizedString,
    selected: Signal<usize>,
    index: usize,
    content: Option<PendingChild>,
    /// When set, the active panel fills its allotted space (see
    /// [`ToolBox::fill`]); the inner content is held directly (no `MaxSize`
    /// clamp) and the cross-/main-axis behaviour is computed in
    /// [`ToolBoxPanel::layout_response`].
    fill: bool,
    orientation: ToolBoxOrientation,
    root_child_id: Option<WidgetId>,
}

impl ToolBoxPanel {
    fn new(
        label: LocalizedString,
        selected: Signal<usize>,
        index: usize,
        content: PendingChild,
        fill: bool,
        orientation: ToolBoxOrientation,
    ) -> Self {
        Self {
            label,
            selected,
            index,
            content: Some(content),
            fill,
            orientation,
            root_child_id: None,
        }
    }
}

impl Widget for ToolBoxPanel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let content_id = match self.content.take().expect("ToolBoxPanel built twice") {
            PendingChild::Id(id) => id,
            PendingChild::Deferred(w) => ctx.add_boxed(w),
        };

        // An inactive section's content is parked **dormant** (out of
        // layout / paint / focus / AT) via `visible_when`, instead of being
        // laid out and clamped to zero. A clamped-to-zero panel still lays its
        // content out (overflowing the 0-px slot), which the inspector flags
        // and which costs real layout work; dormancy avoids both. ToolBox
        // section swaps are instant (no animation), so there's nothing to
        // tween — dormancy is the right tool.
        let idx = self.index;
        let is_selected = self.selected.map(move |s| *s == idx);
        ctx.visible_when(content_id, is_selected);
        self.root_child_id = Some(content_id);
        // Re-measure the panel when the active section changes.
        self.selected.bind_to(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        vec![content_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let Some(root) = self.root_child_id else {
            return proposal.resolve(0.0, 0.0).into();
        };

        // Inactive → zero (the content is dormant, contributing nothing).
        if self.selected.get() != self.index {
            return Size::ZERO.into();
        }

        let content = ctx.child_size(root, proposal).unwrap_or(Size::ZERO);
        if self.fill {
            // Active: fill the cross axis and report flex + shrink on the main
            // axis so the parent stack grows / shrinks us into the leftover
            // space. `min = 0` lets us shrink fully under over-constraint; the
            // content is clipped (`clips_children`) if it can't fit.
            let size = match self.orientation {
                ToolBoxOrientation::Vertical => {
                    Size::new(proposal.width.unwrap_or(content.width), content.height)
                }
                ToolBoxOrientation::Horizontal => {
                    Size::new(content.width, proposal.height.unwrap_or(content.height))
                }
            };
            return teksilo_core::widget::LayoutResponse::shrinkable(size, Size::ZERO, 1.0)
                .with_flex(1.0);
        }
        content.into()
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

    fn clips_children(&self) -> bool {
        // Fill mode clips so an over-tall active panel (or a collapsed
        // zero-size one) never bleeds past its slot. Natural mode keeps the
        // historical non-clipping behaviour.
        self.fill
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use teksilo_core::accesskit::Role;
        // `Region` is the ARIA role for a labelled landmark section. Used
        // by screen readers to announce the panel as "Region: <label>".
        builder.set_role(Role::Region);
        builder.set_name(self.label.resolve_now());
        // Collapsed panels are hidden from AT. Their content is parked dormant
        // (`visible_when`), so it's already out of the AT tree; this also hides
        // the panel landmark node itself so a collapsed section isn't announced.
        if self.selected.get() != self.index {
            builder.set_hidden();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// ToolBox Widget impl
// ---------------------------------------------------------------------------
// RotatedLabel — a single-line label painted rotated 90° for horizontal
// (vertical-strip) ToolBox headers. The footprint is the label's natural
// size with width/height swapped; the label glyphs are rotated via a
// transform scope while the header rect itself stays axis-aligned, so
// hit-testing / layout / drag all work in normal coordinates.
// ---------------------------------------------------------------------------

/// `T(pivot) · R(theta) · T(-pivot)` — rotation about a world-space pivot.
fn pivoted_rotation(pivot: Point, theta: f32) -> Transform2D {
    let (s, c) = theta.sin_cos();
    Transform2D {
        m: [
            c,
            s,
            -s,
            c,
            pivot.x * (1.0 - c) + pivot.y * s,
            pivot.y * (1.0 - c) - pivot.x * s,
        ],
    }
}

#[derive(Debug)]
pub(crate) struct RotatedLabel {
    label: LocalizedString,
    color: ColorProp,
    style: TextStyleProp,
    child_id: Option<WidgetId>,
    natural: Cell<Size>,
    transform_signal: Option<Signal<Transform2D>>,
}

impl RotatedLabel {
    pub(crate) fn new(label: LocalizedString, color: impl Into<ColorProp>) -> Self {
        Self {
            label,
            color: color.into(),
            style: TextStyleRole::Body.into(),
            child_id: None,
            natural: Cell::new(Size::ZERO),
            transform_signal: None,
        }
    }

    /// Override the rotated label's text style (defaults to
    /// [`TextStyleRole::Body`]).
    pub(crate) fn style(mut self, style: impl Into<TextStyleProp>) -> Self {
        self.style = style.into();
        self
    }
}

impl Widget for RotatedLabel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let child = ctx.add(
            TextWidget::new(self.label.clone())
                .color(self.color.clone())
                .style(self.style.clone())
                .single_line()
                .a11y_hidden(),
        );
        self.child_id = Some(child);
        let t = ctx.signal(Transform2D::IDENTITY);
        ctx.set_transform(ctx.self_id(), t.clone());
        self.transform_signal = Some(t);
        vec![child]
    }

    fn layout_response(
        &self,
        _proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Measure the label at its single-line intrinsic size, then swap
        // width/height for the rotated footprint.
        let natural = self
            .child_id
            .and_then(|id| {
                ctx.child_size(
                    id,
                    SizeProposal {
                        width: None,
                        height: None,
                    },
                )
            })
            .unwrap_or(Size::ZERO);
        self.natural.set(natural);
        Size::new(natural.height, natural.width).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let natural = self.natural.get();
        // Centre the (un-rotated) child on the slot centre; a 90° rotation
        // about that centre maps its W×H onto the slot's H×W exactly.
        let cx = bounds.x + bounds.width * 0.5;
        let cy = bounds.y + bounds.height * 0.5;
        let origin = Point::new(cx - natural.width * 0.5, cy - natural.height * 0.5);
        for child in children.iter_mut() {
            child.origin = origin;
            child.size = natural;
        }
        if let Some(t) = &self.transform_signal {
            // -90°: text reads bottom-to-top (the desktop convention for a
            // leading-edge vertical tab/strip).
            t.set(pivoted_rotation(
                Point::new(cx, cy),
                -std::f32::consts::FRAC_PI_2,
            ));
        }
    }

    fn clips_children(&self) -> bool {
        false
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // The header node carries the accessible name; the rotated label
        // is decorative chrome.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------

impl Widget for ToolBox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let items = std::mem::take(&mut self.items);
        let enabled_flags: Rc<Vec<bool>> = Rc::new(items.iter().map(|i| i.enabled.get()).collect());
        let header_ids: Rc<RefCell<Vec<WidgetId>>> =
            Rc::new(RefCell::new(Vec::with_capacity(items.len())));
        let panel_ids: Rc<RefCell<Vec<WidgetId>>> =
            Rc::new(RefCell::new(Vec::with_capacity(items.len())));

        let orientation = self.orientation;
        let show_dividers = self.show_dividers;
        let item_count = items.len();

        // Collect the ordered child ids, then wrap in a VStack (vertical)
        // or HStack (horizontal). Each section contributes its header then
        // its panel; the panel collapses to zero on the main axis when
        // inactive (it already clamps *both* axes), so a collapsed
        // horizontal section shrinks to just its header strip.
        let mut child_ids: Vec<WidgetId> = Vec::with_capacity(item_count * 3);

        for (index, item) in items.into_iter().enumerate() {
            let label = item.label.clone();
            let header_id = ctx.add(ToolBoxHeader::new(
                item.label.clone(),
                index,
                item.enabled.get(),
                self.selected.clone(),
                header_ids.clone(),
                panel_ids.clone(),
                enabled_flags.clone(),
                item.leading,
                item.trailing,
                item.tooltip_text,
                item.rich_tooltip,
                item.composite_tooltip_content,
                orientation,
                self.collapsible,
                self.header_drag.clone(),
            ));
            header_ids.borrow_mut().push(header_id);

            let panel_id = ctx.add(ToolBoxPanel::new(
                label,
                self.selected.clone(),
                index,
                item.content,
                self.fill,
                orientation,
            ));
            panel_ids.borrow_mut().push(panel_id);

            child_ids.push(header_id);
            child_ids.push(panel_id);

            if show_dividers && index + 1 < item_count {
                // The divider runs across the section boundary: a
                // horizontal toolbox needs a vertical divider and vice
                // versa.
                let divider = match orientation {
                    ToolBoxOrientation::Vertical => Divider::new(),
                    ToolBoxOrientation::Horizontal => Divider::vertical(),
                };
                child_ids.push(ctx.add(divider.color(BorderRole::Divider)));
            }
        }

        let root = match orientation {
            ToolBoxOrientation::Vertical => {
                let mut stack = VStack::new().spacing(0.0);
                for id in child_ids {
                    stack = stack.add_child(id);
                }
                ctx.add(stack)
            }
            ToolBoxOrientation::Horizontal => {
                let mut stack = HStack::new().spacing(0.0);
                for id in child_ids {
                    stack = stack.add_child(id);
                }
                ctx.add(stack)
            }
        };
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        if let Some(root) = self.root_child_id
            && let Some(size) = ctx.child_size(root, proposal)
        {
            return (size).into();
        }
        proposal.resolve(0.0, 0.0).into()
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
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::TextWidget;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::accesskit;
    use teksilo_core::event::Modifiers;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    fn tree() -> WidgetTree {
        WidgetTree::new().with_theme(teksilo_core::presets::intui::light())
    }

    /// Walks the ToolBox widget tree to return the `ToolBoxHeader` id for
    /// the given item index. The structure is:
    ///     ToolBox → VStack → [header_0, panel_0, header_1, panel_1, …]
    fn header_id(tree: &WidgetTree, toolbox: WidgetId, index: usize) -> WidgetId {
        let vstack = tree.child_widget(toolbox, 0);
        // Two children per item (header + panel). Dividers would add more,
        // but tests don't enable them.
        tree.child_widget(vstack, index * 2)
    }

    fn panel_id(tree: &WidgetTree, toolbox: WidgetId, index: usize) -> WidgetId {
        let vstack = tree.child_widget(toolbox, 0);
        tree.child_widget(vstack, index * 2 + 1)
    }

    #[test]
    fn tool_box_builds_with_first_selected() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("Outline"), TextWidget::new(lit!("Outline content")))
                .item(lit!("Props"), TextWidget::new(lit!("Props content")))
                .item(lit!("Refs"), TextWidget::new(lit!("Refs content"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let b = t.bounds(tb);
        assert!(b.width > 0.0, "ToolBox width = {}", b.width);
        assert!(b.height > 0.0, "ToolBox height = {}", b.height);
    }

    #[test]
    fn clicking_header_changes_selection() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .item(lit!("B"), TextWidget::new(lit!("B")))
                .item(lit!("C"), TextWidget::new(lit!("C"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        t.click(header_id(&t, tb, 2));
        assert_eq!(selected.get(), 2);

        t.click(header_id(&t, tb, 0));
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn panel_heights_swap_on_selection_change() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("AAAAAAAA")))
                .item(lit!("B"), TextWidget::new(lit!("BBBBBBBB"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let panel_a_before = t.bounds(panel_id(&t, tb, 0)).height;
        let panel_b_before = t.bounds(panel_id(&t, tb, 1)).height;
        assert!(
            panel_a_before > 0.0,
            "active panel should have nonzero height"
        );
        assert!(panel_b_before < 0.5, "inactive panel should be collapsed");

        selected.set(1);
        t.layout(SizeProposal::exact(300.0, 600.0));

        let panel_a_after = t.bounds(panel_id(&t, tb, 0)).height;
        let panel_b_after = t.bounds(panel_id(&t, tb, 1)).height;
        assert!(panel_a_after < 0.5, "formerly active panel collapsed");
        assert!(panel_b_after > 0.0, "newly active panel expanded");
    }

    #[test]
    fn programmatic_selection_drives_swap_like_click() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("AAA")))
                .item(lit!("B"), TextWidget::new(lit!("BBB"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        // Programmatic: no event dispatch — purely via the signal.
        selected.set(1);
        t.layout(SizeProposal::exact(300.0, 600.0));

        let panel_a = t.bounds(panel_id(&t, tb, 0)).height;
        let panel_b = t.bounds(panel_id(&t, tb, 1)).height;
        assert!(panel_a < 0.5);
        assert!(panel_b > 0.0);
    }

    #[test]
    fn disabled_item_ignores_click() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .add(ToolBoxItem::new(lit!("B"), TextWidget::new(lit!("B"))).enabled(false))
                .item(lit!("C"), TextWidget::new(lit!("C"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let disabled = header_id(&t, tb, 1);
        t.click(disabled);
        assert_eq!(selected.get(), 0, "disabled header should not activate");
    }

    #[test]
    fn arrow_down_skips_disabled_header() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .add(ToolBoxItem::new(lit!("B"), TextWidget::new(lit!("B"))).enabled(false))
                .item(lit!("C"), TextWidget::new(lit!("C"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        // Tab into the toolbox — lands on the first enabled header.
        t.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(t.focused(), Some(header_id(&t, tb, 0)));

        t.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(t.focused(), Some(header_id(&t, tb, 2)));
    }

    #[test]
    fn home_and_end_jump_to_first_and_last_enabled() {
        let selected = Signal::new(1_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .add(ToolBoxItem::new(lit!("Locked"), TextWidget::new(lit!("x"))).enabled(false))
                .item(lit!("Middle"), TextWidget::new(lit!("m")))
                .item(lit!("Last"), TextWidget::new(lit!("l"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        // Focus the second (index=1) header so we can test Home/End
        // from a middle position.
        t.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(t.focused(), Some(header_id(&t, tb, 1)));

        t.press_key(Key::End, Modifiers::NONE);
        assert_eq!(t.focused(), Some(header_id(&t, tb, 2)));

        t.press_key(Key::Home, Modifiers::NONE);
        // Home jumps to first *enabled* header — index 0 is disabled, so
        // index 1 is first.
        assert_eq!(t.focused(), Some(header_id(&t, tb, 1)));
    }

    #[test]
    fn accessibility_marks_selected_expanded_and_controls_panel() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .item(lit!("B"), TextWidget::new(lit!("B"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let h0 = t.accessibility_node(header_id(&t, tb, 0));
        let h1 = t.accessibility_node(header_id(&t, tb, 1));
        assert!(h0.is_expanded());
        assert!(!h1.is_expanded());
        assert_eq!(h0.role(), accesskit::Role::Button);

        // Swap selection; header 1 should now be expanded.
        selected.set(1);
        t.layout(SizeProposal::exact(300.0, 600.0));
        let h0b = t.accessibility_node(header_id(&t, tb, 0));
        let h1b = t.accessibility_node(header_id(&t, tb, 1));
        assert!(!h0b.is_expanded());
        assert!(h1b.is_expanded());

        // Panel role is Region with the item label as name.
        let p0 = t.accessibility_node(panel_id(&t, tb, 0));
        assert_eq!(p0.role(), accesskit::Role::Region);
        assert_eq!(p0.name(), Some("A"));
    }

    #[test]
    fn access_action_expand_selects_item() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .item(lit!("B"), TextWidget::new(lit!("B")))
                .item(lit!("C"), TextWidget::new(lit!("C"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let third = header_id(&t, tb, 2);
        t.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Expand,
            target: Some(third),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        assert_eq!(selected.get(), 2);
    }

    #[test]
    fn access_action_collapse_is_swallowed() {
        let selected = Signal::new(1_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .item(lit!("B"), TextWidget::new(lit!("B"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        // Collapse on the active header should not change `selected`.
        let active = header_id(&t, tb, 1);
        t.dispatch_event(WidgetEvent::AccessAction {
            action: accesskit::Action::Collapse,
            target: Some(active),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        assert_eq!(selected.get(), 1);
    }

    #[test]
    fn leading_slot_widget_is_placed_inside_the_header() {
        use crate::Button;

        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone()).add(
                ToolBoxItem::new(lit!("A"), TextWidget::new(lit!("A")))
                    .leading(Button::new(lit!("start"))),
            ),
        );
        t.layout(SizeProposal::exact(300.0, 200.0));

        let header = header_id(&t, tb, 0);
        let header_bounds = t.bounds(header);

        fn find_button_inside(t: &WidgetTree, root: WidgetId, outer: WidgetId) -> Option<WidgetId> {
            for child in t.children(root) {
                if child != outer {
                    let info = t.accessibility_node(child);
                    if info.role() == teksilo_core::accesskit::Role::Button {
                        return Some(child);
                    }
                }
                if let Some(found) = find_button_inside(t, child, outer) {
                    return Some(found);
                }
            }
            None
        }

        let leading_btn = find_button_inside(&t, header, header)
            .expect("leading Button should be a descendant of the header");
        let btn_bounds = t.bounds(leading_btn);
        assert!(
            btn_bounds.x >= header_bounds.x && btn_bounds.right() <= header_bounds.right() + 0.01,
            "leading button bounds must fit inside header row"
        );
    }

    #[test]
    fn trailing_slot_widget_is_placed_inside_the_header() {
        use crate::Button;

        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone()).add(
                ToolBoxItem::new(lit!("A"), TextWidget::new(lit!("A")))
                    .trailing(Button::new(lit!("x"))),
            ),
        );
        t.layout(SizeProposal::exact(300.0, 200.0));

        let header = header_id(&t, tb, 0);
        let header_bounds = t.bounds(header);

        // Walk descendants looking for the inner Button (the outer
        // header itself also has Role::Button — we want the trailing
        // one, characterised by sitting on the trailing edge of the
        // header row).
        fn find_button_inside(t: &WidgetTree, root: WidgetId, outer: WidgetId) -> Option<WidgetId> {
            for child in t.children(root) {
                if child != outer {
                    let info = t.accessibility_node(child);
                    if info.role() == teksilo_core::accesskit::Role::Button {
                        return Some(child);
                    }
                }
                if let Some(found) = find_button_inside(t, child, outer) {
                    return Some(found);
                }
            }
            None
        }

        let trailing_btn = find_button_inside(&t, header, header)
            .expect("trailing Button should be a descendant of the header");
        let btn_bounds = t.bounds(trailing_btn);
        assert!(
            btn_bounds.x >= header_bounds.x && btn_bounds.right() <= header_bounds.right() + 0.01,
            "trailing button bounds must fit inside header row"
        );
    }

    #[test]
    fn disabled_header_has_no_click_action() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("A")))
                .add(ToolBoxItem::new(lit!("B"), TextWidget::new(lit!("B"))).enabled(false)),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let disabled = header_id(&t, tb, 1);
        let info = t.accessibility_node(disabled);
        assert!(!info.actions().contains(&accesskit::Action::Click));
        assert!(!info.actions().contains(&accesskit::Action::Expand));
    }

    // ─── orientation + drag ────────────────────────────────────────────

    #[test]
    fn vertical_orientation_stacks_top_to_bottom() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("a")))
                .item(lit!("B"), TextWidget::new(lit!("b"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));
        let h0 = t.bounds(header_id(&t, tb, 0));
        let h1 = t.bounds(header_id(&t, tb, 1));
        assert!(h1.y > h0.y, "vertical headers stack top→bottom");
        // Header is a wide, short row.
        assert!(h0.width > h0.height, "vertical header is a horizontal row");
    }

    #[test]
    fn horizontal_orientation_lays_sections_left_to_right() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .horizontal()
                .item(lit!("Terminal"), TextWidget::new(lit!("term")))
                .item(lit!("Problems"), TextWidget::new(lit!("prob")))
                .item(lit!("Output"), TextWidget::new(lit!("out"))),
        );
        t.layout(SizeProposal::exact(900.0, 220.0));

        let h0 = t.bounds(header_id(&t, tb, 0));
        let h1 = t.bounds(header_id(&t, tb, 1));
        let h2 = t.bounds(header_id(&t, tb, 2));
        assert!(
            h0.x < h1.x && h1.x < h2.x,
            "horizontal headers run left→right: {} {} {}",
            h0.x,
            h1.x,
            h2.x
        );
        // Each header is a tall, narrow vertical strip.
        assert!(
            h0.height > h0.width,
            "horizontal header is a vertical strip ({}×{})",
            h0.width,
            h0.height
        );
        assert!(
            h0.width <= TOOL_BOX_HEADER_MIN_HEIGHT + 24.0,
            "strip stays narrow (got width {})",
            h0.width
        );
    }

    #[test]
    fn horizontal_collapsed_panel_has_zero_main_extent() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .horizontal()
                .item(lit!("A"), TextWidget::new(lit!("aaaa")))
                .item(lit!("B"), TextWidget::new(lit!("bbbb"))),
        );
        t.layout(SizeProposal::exact(900.0, 220.0));
        // Section 0 is selected → its panel has width; section 1's panel
        // collapses to zero width.
        assert!(t.bounds(panel_id(&t, tb, 0)).width > 0.0);
        assert!(t.bounds(panel_id(&t, tb, 1)).width.abs() < 0.5);
    }

    #[test]
    fn header_drag_hook_fires_with_section_index() {
        use std::cell::Cell as StdCell;
        let dragged: Rc<StdCell<Option<usize>>> = Rc::new(StdCell::new(None));
        let selected = Signal::new(0_usize);
        let sink = dragged.clone();
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .on_header_drag(move |idx, _ctx| sink.set(Some(idx)))
                .item(lit!("A"), TextWidget::new(lit!("a")))
                .item(lit!("B"), TextWidget::new(lit!("b"))),
        );
        t.layout(SizeProposal::exact(300.0, 600.0));

        let h1 = t.bounds(header_id(&t, tb, 1));
        let from = teksilo_canvas::Point::new(h1.x + h1.width * 0.5, h1.y + h1.height * 0.5);
        // Drag well past the threshold to trigger DragPhase::Started.
        t.drag(
            from,
            teksilo_canvas::Point::new(from.x + 120.0, from.y + 40.0),
        );
        assert_eq!(
            dragged.get(),
            Some(1),
            "dragging header #1 must fire the hook with index 1"
        );
    }

    // ─── fill mode ─────────────────────────────────────────────────────

    #[test]
    fn fill_active_panel_fills_width_and_leftover_height() {
        // A narrow-content section in a tall box: with `.fill(true)` the
        // active panel stretches to the full width and grows into the leftover
        // height after the headers, so the ToolBox fills its slot exactly.
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .fill(true)
                .item(lit!("A"), TextWidget::new(lit!("a")))
                .item(lit!("B"), TextWidget::new(lit!("b"))),
        );
        t.layout(SizeProposal::exact(300.0, 400.0));

        // The ToolBox fills the proposed height exactly (no under-fill gap,
        // no overflow).
        assert!(
            (t.bounds(tb).height - 400.0).abs() < 1.0,
            "fill ToolBox should occupy its full slot height, got {}",
            t.bounds(tb).height
        );

        let panel_a = t.bounds(panel_id(&t, tb, 0));
        // Active panel fills the cross axis (width).
        assert!(
            panel_a.width > 290.0,
            "active panel should fill the width, got {}",
            panel_a.width
        );
        // …and grows into the leftover main-axis space (well beyond a single
        // text line).
        assert!(
            panel_a.height > 200.0,
            "active panel should grow into leftover height, got {}",
            panel_a.height
        );
        // Inactive panel stays collapsed.
        assert!(
            t.bounds(panel_id(&t, tb, 1)).height < 0.5,
            "inactive panel collapsed"
        );
    }

    #[test]
    fn fill_active_panel_does_not_overflow_oversized_content() {
        // A section whose content wants 1000 px in a 200 px box: with
        // `.fill(true)` the active panel shrinks to fit (and clips) instead of
        // pushing the ToolBox past its slot — so a bottom toolbar inside the
        // content never lands below the visible area.
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tall = FixedSize::new()
            .width(120.0_f32)
            .height(1000.0_f32)
            .child(TextWidget::new(lit!("x")));
        let tb = t.add(
            ToolBox::new(selected.clone())
                .fill(true)
                .item(lit!("A"), tall)
                .item(lit!("B"), TextWidget::new(lit!("b"))),
        );
        t.layout(SizeProposal::exact(300.0, 200.0));

        assert!(
            (t.bounds(tb).height - 200.0).abs() < 1.0,
            "fill ToolBox must not overflow its slot, got {}",
            t.bounds(tb).height
        );
        let panel_a = t.bounds(panel_id(&t, tb, 0));
        assert!(
            panel_a.height < 200.0,
            "oversized active panel shrinks to fit, got {}",
            panel_a.height
        );
    }

    #[test]
    fn non_fill_panel_keeps_natural_size() {
        // Without `.fill`, the historical behaviour stands: the active panel
        // sizes to its content's natural extent (a short text line), leaving
        // the box partly empty rather than stretching.
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("a")))
                .item(lit!("B"), TextWidget::new(lit!("b"))),
        );
        t.layout(SizeProposal::exact(300.0, 400.0));
        // Natural mode: the active panel is just a text line tall, far short
        // of the 400 px slot.
        assert!(
            t.bounds(panel_id(&t, tb, 0)).height < 100.0,
            "non-fill panel keeps natural height, got {}",
            t.bounds(panel_id(&t, tb, 0)).height
        );
    }

    // ─── collapsible mode ──────────────────────────────────────────────

    #[test]
    fn collapsible_active_header_click_collapses_then_reexpands() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .collapsible(true)
                .item(lit!("A"), TextWidget::new(lit!("aaa")))
                .item(lit!("B"), TextWidget::new(lit!("bbb"))),
        );
        t.layout(SizeProposal::exact(300.0, 400.0));
        assert!(
            t.bounds(panel_id(&t, tb, 0)).height > 0.0,
            "A starts expanded"
        );

        // Click the active header → collapse it (all sections closed).
        t.click(header_id(&t, tb, 0));
        t.layout(SizeProposal::exact(300.0, 400.0));
        assert!(
            t.bounds(panel_id(&t, tb, 0)).height < 0.5,
            "active header click collapses its content"
        );
        assert!(
            t.bounds(panel_id(&t, tb, 1)).height < 0.5,
            "B stays collapsed"
        );

        // Click again → re-expand.
        t.click(header_id(&t, tb, 0));
        t.layout(SizeProposal::exact(300.0, 400.0));
        assert!(
            t.bounds(panel_id(&t, tb, 0)).height > 0.0,
            "re-expands on next click"
        );
    }

    #[test]
    fn non_collapsible_active_header_click_stays_open() {
        // Default (exclusive): clicking the active header keeps it open.
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(
            ToolBox::new(selected.clone())
                .item(lit!("A"), TextWidget::new(lit!("aaa")))
                .item(lit!("B"), TextWidget::new(lit!("bbb"))),
        );
        t.layout(SizeProposal::exact(300.0, 400.0));
        t.click(header_id(&t, tb, 0));
        t.layout(SizeProposal::exact(300.0, 400.0));
        assert_eq!(selected.get(), 0);
        assert!(
            t.bounds(panel_id(&t, tb, 0)).height > 0.0,
            "non-collapsible active section stays open"
        );
    }

    // ─── tooltip ───────────────────────────────────────────────────────

    #[test]
    fn tooltip_appears_on_hover() {
        let selected = Signal::new(0_usize);
        let mut t = tree();
        let tb = t.add(ToolBox::new(selected.clone()).add(
            ToolBoxItem::new(lit!("A"), TextWidget::new(lit!("content"))).tooltip(lit!("Tip")),
        ));
        t.layout(SizeProposal::exact(300.0, 200.0));
        t.pointer_move(t.bounds(header_id(&t, tb, 0)).center());
        t.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            t.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(t.find_by_label("Tip").is_some());
    }
}
