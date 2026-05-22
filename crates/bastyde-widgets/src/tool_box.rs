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
//!   [`MotionTokens`](bastyde_tokens::MotionTokens). Matches the existing
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

use bastyde_i18n::lit;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, PendingChild, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{BorderRole, SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{
    Divider, FixedSize, HStack, IconWidget, MaxSize, MinSize, RectWidget, Spacer, TextWidget,
    VStack, ZStack,
};
use crate::tooltip::{
    DEFAULT_RICH_TOOLTIP_DELAY, RichTooltipSource, TooltipContent, attach_rich_tooltip_source,
};

// Large sentinel value used when binding `MaxSize::max_height` / `max_width`
// to mean "no upper bound" — the clamp in `MaxSize` reduces this to the
// child's intrinsic size. Mirrors the constant in [`Accordion`].
const UNBOUNDED: f32 = 10_000.0;

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
    label: String,
    leading: Option<Box<dyn Widget>>,
    trailing: Option<Box<dyn Widget>>,
    tooltip: Option<RichTooltipSource>,
    content: PendingChild,
    /// Initial-enabled hint. Forwarded into the arena via
    /// `ctx.enabled_when(header_id, false)` at build time when `false`.
    /// After build the arena is the single source of truth and ANDs
    /// with ancestors — so a disabled `ToolBox` ancestor disables every
    /// item header regardless of its own `initial_enabled`.
    initial_enabled: bool,
}

impl std::fmt::Debug for ToolBoxItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolBoxItem")
            .field("label", &self.label)
            .field("initial_enabled", &self.initial_enabled)
            .finish()
    }
}

impl ToolBoxItem {
    /// Build an item with an inline content widget. The label may come from
    /// `tr!(...)` (translated) or `lit!(...)`.
    pub fn new(label: impl Into<LocalizedString>, content: impl Widget + 'static) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            leading: None,
            trailing: None,
            tooltip: None,
            content: PendingChild::Deferred(Box::new(content)),
            initial_enabled: true,
        }
    }

    /// Build an item whose content is a pre-registered widget id.
    pub fn new_id(label: impl Into<LocalizedString>, content_id: WidgetId) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            leading: None,
            trailing: None,
            tooltip: None,
            content: PendingChild::Id(content_id),
            initial_enabled: true,
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

    /// Attach a rich tooltip shown after a hover delay on the header row.
    /// Accepts either a registry key (`"save-as"`) or inline
    /// [`TooltipContent`].
    pub fn tooltip(mut self, source: impl Into<RichTooltipSource>) -> Self {
        self.tooltip = Some(source.into());
        self
    }

    /// Attach an inline tooltip without using the registry.
    pub fn tooltip_content(mut self, content: TooltipContent) -> Self {
        self.tooltip = Some(RichTooltipSource::Content(content));
        self
    }

    /// Disable the item: its header renders in the disabled text role,
    /// click and keyboard activation are ignored, and arrow navigation
    /// skips it.
    ///
    /// Forwarded to the arena via `ctx.enabled_when(header_id, false)`
    /// at build time; the arena is then the single source of truth and
    /// ANDs with ancestors — disabling the surrounding `ToolBox` (or
    /// any ancestor) disables every item regardless of this flag.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }
}

/// ToolBox design tokens.
pub const TOOL_BOX_HEADER_MIN_HEIGHT: f32 = 28.0;
pub const TOOL_BOX_HEADER_PADDING_HORIZONTAL: f32 = 12.0;
pub const TOOL_BOX_ICON_TEXT_SPACING: f32 = 8.0;
pub const TOOL_BOX_CHEVRON_SIZE: f32 = 12.0;
pub const TOOL_BOX_INDICATOR_THICKNESS: f32 = 1.0;

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
    root_child_id: Option<WidgetId>,
}

impl ToolBox {
    pub fn new(selected: Signal<usize>) -> Self {
        Self {
            selected,
            items: Vec::new(),
            show_dividers: false,
            root_child_id: None,
        }
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

#[derive(Debug)]
struct ToolBoxHeader {
    label: String,
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
    tooltip: Option<RichTooltipSource>,
    root_child_id: Option<WidgetId>,
}

impl ToolBoxHeader {
    #[allow(clippy::too_many_arguments)]
    fn new(
        label: String,
        index: usize,
        initial_enabled: bool,
        selected: Signal<usize>,
        header_ids: Rc<RefCell<Vec<WidgetId>>>,
        panel_ids: Rc<RefCell<Vec<WidgetId>>>,
        enabled_flags: Rc<Vec<bool>>,
        pending_leading: Option<Box<dyn Widget>>,
        pending_trailing: Option<Box<dyn Widget>>,
        tooltip: Option<RichTooltipSource>,
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
            tooltip,
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
        let focus_origin: Signal<Option<bastyde_core::focus::FocusOrigin>> = ctx.signal(None);

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
            Some(bastyde_core::focus::FocusOrigin::Keyboard) => focus_ring_width,
            _ => 0.0,
        });
        let focus_border_color = focus_origin.map(|o| match o {
            Some(bastyde_core::focus::FocusOrigin::Keyboard) => BorderRole::Focused,
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
        let indicator_rect_id = ctx.add(RectWidget::new().background(indicator_bg));
        let indicator_id = ctx.add(
            FixedSize::new()
                .bind_width(TOOL_BOX_INDICATOR_THICKNESS)
                .child_id(indicator_rect_id),
        );

        // Optional leading-slot widget. Registered here so its id can be
        // inserted before the label.
        let leading_id = self.pending_leading.take().map(|w| ctx.add_boxed(w));

        // Label — single-line, clips with ellipsis if the header is
        // narrower than the text.
        let label_id = ctx.add(
            TextWidget::new(lit!(&self.label))
                .bind_color(text_role.clone())
                .style(TextStyleRole::Body)
                .single_line()
                .a11y_hidden(),
        );

        let spacer_id = ctx.add(Spacer::new());

        // Optional trailing-slot widget. Registered here so its id can
        // be placed between the spacer and the chevrons.
        let trailing_id = self.pending_trailing.take().map(|w| ctx.add_boxed(w));

        // Two chevron glyphs toggled via `visible_when` — cheaper than
        // re-rendering a single glyph at runtime.
        let chevron_down_id =
            ctx.add(IconWidget::chevron_down(TOOL_BOX_CHEVRON_SIZE).bind_color(text_role.clone()));
        let chevron_right_id =
            ctx.add(IconWidget::chevron_right(TOOL_BOX_CHEVRON_SIZE).bind_color(text_role));
        ctx.visible_when(chevron_down_id, is_selected.clone());
        ctx.visible_when(chevron_right_id, is_selected.map(|v| !*v));

        // Compose the header row:
        //   [indicator] [leading?] [label] [spacer] [trailing?] [chevron]
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

        // Wrap the row in horizontal padding. The indicator sits at
        // `x = padding_horizontal` inside the header rather than flush
        // to the widget's leading edge — this matches how IntelliJ's
        // Settings panels render their selection bar (inset from the
        // row edge by the container's padding).
        let padded_row_id = ctx.add(
            crate::primitives::Padding::symmetric(0.0, TOOL_BOX_HEADER_PADDING_HORIZONTAL)
                .child_id(row_id),
        );

        // Background fills the whole header row.
        let bg_rect_id = ctx.add(RectWidget::new().bind_background(bg_role));

        // Focus-border rect is inset by half the focus stroke width on
        // every side so the centred stroke fits *entirely* inside the
        // ZStack bounds. Without the inset the stroke bleeds outside and
        // the VStack (or a sibling row) clips the outer half, so the
        // ring appears truncated on all four edges. Wrapping the rect
        // in a `Padding(focus_ring_width / 2)` gives it bounds that,
        // when stroked with `focus_ring_width`, reach the ZStack edge
        // exactly.
        let focus_inset = focus_ring_width * 0.5;
        let focus_rect_id = ctx.add(
            RectWidget::new()
                .bind_border_color(focus_border_color)
                .bind_border_width(focus_border_width),
        );
        let focus_padded_id =
            ctx.add(crate::primitives::Padding::uniform(focus_inset).child_id(focus_rect_id));
        let zstack_id = ctx.add(
            ZStack::new()
                .add_child(bg_rect_id)
                .add_child(focus_padded_id)
                .add_child(padded_row_id),
        );

        // Enforce the Int UI 28 dp row height.
        let root_id = ctx.add(MinSize::new(0.0, TOOL_BOX_HEADER_MIN_HEIGHT).child_id(zstack_id));
        self.root_child_id = Some(root_id);

        // Attach rich tooltip if configured.
        if let Some(source) = self.tooltip.take() {
            attach_rich_tooltip_source(ctx, root_id, source, DEFAULT_RICH_TOOLTIP_DELAY);
        }

        // --- V2 attached handlers on the header's own node ---
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

        let handler_set = HandlerSet::new()
            .on_tap(move |_pos, _ctx| {
                selected_tap.set(idx);
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
                    bastyde_core::focus::FocusOrigin::Pointer
                } else {
                    bastyde_core::focus::FocusOrigin::Keyboard
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
                        selected_key.set(idx);
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
                    bastyde_core::accesskit::Action::Click
                    | bastyde_core::accesskit::Action::Expand => {
                        selected_access.set(idx);
                        EventResponse::Handled
                    }
                    // Exclusive disclosure: collapsing the active section
                    // without picking a replacement would violate the
                    // invariant. Swallow.
                    bastyde_core::accesskit::Action::Collapse => EventResponse::Handled,
                    _ => EventResponse::Ignored,
                }
            })
            // The focus walker skips disabled subtrees on its own, so
            // we set `focusable(true)` unconditionally — the static
            // intent is "this header takes keyboard focus" and the
            // arena gates whether it actually does.
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        use bastyde_core::accesskit::{Action, Role};
        builder.set_role(Role::Button);
        builder.set_name(&self.label);
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
    label: String,
    selected: Signal<usize>,
    index: usize,
    content: Option<PendingChild>,
    root_child_id: Option<WidgetId>,
}

impl ToolBoxPanel {
    fn new(label: String, selected: Signal<usize>, index: usize, content: PendingChild) -> Self {
        Self {
            label,
            selected,
            index,
            content: Some(content),
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

        let idx = self.index;
        // `MaxSize` clamps the child to `min(max, intrinsic)`. `UNBOUNDED`
        // is a large constant that effectively says "no upper limit"; the
        // child's natural size wins. `0.0` collapses the panel fully.
        let is_selected = self.selected.map(move |s| *s == idx);
        let height_prop = is_selected.map(|sel| if *sel { UNBOUNDED } else { 0.0 });
        // Also clamp width so a collapsed panel's intrinsic width does
        // not leak into the parent's size_that_fits via the VStack —
        // same trick Accordion uses ([accordion.rs:200-208]).
        let width_prop = is_selected.map(|sel| if *sel { UNBOUNDED } else { 0.0 });

        let root = ctx.add(
            MaxSize::new(UNBOUNDED, UNBOUNDED)
                .bind_max_width(width_prop)
                .bind_max_height(height_prop)
                .child_id(content_id),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        use bastyde_core::accesskit::Role;
        // `Region` is the ARIA role for a labelled landmark section. Used
        // by screen readers to announce the panel as "Region: <label>".
        builder.set_role(Role::Region);
        builder.set_name(&self.label);
        // Collapsed panels must be hidden from AT — they're kept in the
        // widget tree for animation purposes, but their content is at
        // height=0 and should not be navigable by screen readers.
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

impl Widget for ToolBox {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let items = std::mem::take(&mut self.items);
        let enabled_flags: Rc<Vec<bool>> =
            Rc::new(items.iter().map(|i| i.initial_enabled).collect());
        let header_ids: Rc<RefCell<Vec<WidgetId>>> =
            Rc::new(RefCell::new(Vec::with_capacity(items.len())));
        let panel_ids: Rc<RefCell<Vec<WidgetId>>> =
            Rc::new(RefCell::new(Vec::with_capacity(items.len())));

        let mut vstack = VStack::new().spacing(0.0);
        let show_dividers = self.show_dividers;
        let item_count = items.len();

        for (index, item) in items.into_iter().enumerate() {
            let label = item.label.clone();
            let header_id = ctx.add(ToolBoxHeader::new(
                item.label.clone(),
                index,
                item.initial_enabled,
                self.selected.clone(),
                header_ids.clone(),
                panel_ids.clone(),
                enabled_flags.clone(),
                item.leading,
                item.trailing,
                item.tooltip,
            ));
            header_ids.borrow_mut().push(header_id);

            let panel_id = ctx.add(ToolBoxPanel::new(
                label,
                self.selected.clone(),
                index,
                item.content,
            ));
            panel_ids.borrow_mut().push(panel_id);

            vstack = vstack.add_child(header_id).add_child(panel_id);

            if show_dividers && index + 1 < item_count {
                let divider_id = ctx.add(Divider::new().color(BorderRole::Divider));
                vstack = vstack.add_child(divider_id);
            }
        }

        let root = ctx.add(vstack);
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
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
        builder.set_role(bastyde_core::accesskit::Role::GenericContainer);
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
    use bastyde_canvas::SizeProposal;
    use bastyde_core::accesskit;
    use bastyde_core::event::Modifiers;
    use bastyde_core::widget_tree::WidgetTree;

    fn tree() -> WidgetTree {
        WidgetTree::new().with_theme(bastyde_core::presets::intui::light())
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
            target_node: bastyde_core::accessibility::root_node_id(),
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
            target_node: bastyde_core::accessibility::root_node_id(),
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
                    if info.role() == bastyde_core::accesskit::Role::Button {
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
                    if info.role() == bastyde_core::accesskit::Role::Button {
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
}
