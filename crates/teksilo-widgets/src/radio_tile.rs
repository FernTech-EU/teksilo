// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! RadioTile — a "selectable card" radio option.
//!
//! A `RadioTile` behaves as a single radio button (`Role::RadioButton`,
//! `set_toggled`) rendered as a bordered, rounded card: a leading icon, a
//! bold title, an inline radio indicator, and a muted, wrapping description.
//! Multiple tiles share a `Signal<usize>` — selecting one writes its `value`,
//! which deselects every sibling observing the same signal (the `RadioButton`
//! model). Group them with
//! [`RadioTileGroup`](crate::radio_tile_group::RadioTileGroup) for layout,
//! roving keyboard navigation, and the AT "N of M" positional announcement.
//!
//! ## Content model
//!
//! Typed slots cover the common case (matching the reference design):
//! `.icon(..)`, `.title(..)`, `.description(..)`. For arbitrary content, the
//! `.body(..)` slot replaces the description column with any widget subtree.
//!
//! ## Accessibility
//!
//! Reports `Role::RadioButton` with `set_toggled` mirroring selection, the
//! title as the accessible name, and the description as the accessible
//! description. When grouped, each tile emits
//! `push_to_radio_group([sibling_ids])` plus `set_position_in_set` /
//! `set_size_of_set` for "N of M". Inside a `RadioTileGroup` the tile is not
//! individually focusable — focus roves on the group (WAI-ARIA radiogroup),
//! and the group publishes `active_descendant`. A standalone tile is
//! focusable and responds to `Space` / `Action::Click`.
//!
//! ```ignore
//! let selected = ctx.signal(0_usize);
//! RadioTileGroup::new(selected)
//!     .tile(RadioTile::new().icon(icon).title(tr!(single_file())).description(tr!(single_file_desc())))
//!     .tile(RadioTile::new().icon(icon2).title(tr!(bundle())).description(tr!(bundle_desc())))
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::{ColorProp, TextStyleProp};
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{
    RadioStyleConfig, RadioTileStyle, RadioTileStyleConfig, RadioTileVariant, RadioVariant,
    SharedRadioStyle, SharedRadioTileStyle,
};
use teksilo_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{HAlignment, TextRole, TextStyleRole, VAlignment};

use crate::button::InteractionState;
use crate::primitives::{HStack, Spacer, TextWidget, VStack};
use crate::styles::{RecipeRadioStyle, RecipeRadioTileStyle};
use teksilo_i18n::LocalizedString;

/// Horizontal gap between the icon / title / indicator on a tile's top row.
const TILE_ROW_GAP: f32 = 10.0;
/// Vertical gap between the tile's title row and its description.
const TILE_TITLE_DESC_GAP: f32 = 6.0;

/// Which side of the top row the radio indicator sits on. Defaults to
/// `Trailing` (top-right in LTR), matching the reference design.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub enum RadioTileIndicatorSide {
    /// Trailing edge of the row — top-right in LTR, top-left in RTL.
    #[default]
    Trailing,
    /// Leading edge of the row — top-left in LTR, top-right in RTL.
    Leading,
}

/// A single selectable-card radio option. See the [module docs](self).
pub struct RadioTile {
    value: usize,
    selected: Signal<usize>,
    icon: Option<Box<dyn Widget>>,
    title: Option<LocalizedString>,
    description: Option<LocalizedString>,
    body: Option<Box<dyn Widget>>,
    /// Right-aligned trailing meta text (e.g. "20 chapters") — tints to accent
    /// when selected. Ignored when a `trailing_slot` is set.
    trailing: Option<LocalizedString>,
    trailing_slot: Option<Box<dyn Widget>>,
    /// Compact single-line arrangement: `[indicator] [icon] [title] [Spacer]
    /// [trailing]`, no description row (the vertical-list look). Set by
    /// `RadioTileGroup::layout(TileLayout::Vertical)` or `.compact(true)`.
    compact: bool,
    title_style: Option<TextStyleProp>,
    title_color: Option<ColorProp>,
    description_style: Option<TextStyleProp>,
    description_color: Option<ColorProp>,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    variant: RadioTileVariant,
    show_indicator: bool,
    indicator_side: RadioTileIndicatorSide,
    tooltip_text: Option<LocalizedString>,
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    composite_tooltip_content: Option<Box<dyn Widget>>,
    /// Where the tooltip opens relative to the tile. `Below` (default) suits
    /// horizontal (`Row`) and 2-D (`Grid`) group layouts; a vertical group
    /// (`Column` / `Vertical`) sets this to `Side` via
    /// [`set_tooltip_placement`](Self::set_tooltip_placement) so the tooltip
    /// doesn't cover the tile below.
    tooltip_placement: crate::tooltip::TooltipPlacement,
    style_override: Option<SharedRadioTileStyle>,
    /// Set by `RadioTileGroup`: the tile is part of a roving radiogroup, so it
    /// is not individually focusable and its focus ring follows the group.
    grouped: bool,
    group_focused: Option<Signal<bool>>,
    group_ids: Option<Rc<RefCell<Vec<WidgetId>>>>,
    pos_in_set: Option<usize>,
    size_of_set: Option<usize>,
    root_child_id: Option<WidgetId>,
}

impl RadioTile {
    /// Create a tile with no selection binding. The enclosing
    /// [`RadioTileGroup`](crate::radio_tile_group::RadioTileGroup) assigns
    /// this tile's `value` (its position) and shared selection signal. Use
    /// [`selection`](Self::selection) for a standalone tile.
    pub fn new() -> Self {
        Self {
            value: 0,
            selected: Signal::new(0),
            icon: None,
            title: None,
            description: None,
            body: None,
            trailing: None,
            trailing_slot: None,
            compact: false,
            title_style: None,
            title_color: None,
            description_style: None,
            description_color: None,
            enabled: Prop::Static(true),
            variant: RadioTileVariant::default(),
            show_indicator: true,
            indicator_side: RadioTileIndicatorSide::default(),
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            tooltip_placement: crate::tooltip::TooltipPlacement::Below,
            style_override: None,
            grouped: false,
            group_focused: None,
            group_ids: None,
            pos_in_set: None,
            size_of_set: None,
            root_child_id: None,
        }
    }

    /// Bind this tile to an explicit `value` + shared `Signal<usize>` for use
    /// **outside** a `RadioTileGroup`. Inside a group this is set automatically.
    pub fn selection(mut self, value: usize, selected: Signal<usize>) -> Self {
        self.value = value;
        self.selected = selected;
        self
    }

    /// Leading icon slot (top-left of the tile). Any widget — typically an
    /// [`IconWidget`](crate::primitives::IconWidget).
    pub fn icon(mut self, widget: impl Widget + 'static) -> Self {
        self.icon = Some(Box::new(widget));
        self
    }

    /// Leading icon slot, pre-boxed.
    pub fn icon_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.icon = Some(widget);
        self
    }

    /// Bold title text (the tile's accessible name).
    pub fn title(mut self, title: impl Into<LocalizedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Muted, multi-line description (the tile's accessible description).
    /// Ignored when a [`body`](Self::body) is set.
    pub fn description(mut self, text: impl Into<LocalizedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// Replace the description column with an arbitrary widget subtree. Takes
    /// precedence over [`description`](Self::description). Note: a body's own
    /// content is exposed to assistive technology as-is (unlike the typed
    /// description, which is folded into the tile's accessible description).
    pub fn body(mut self, widget: impl Widget + 'static) -> Self {
        self.body = Some(Box::new(widget));
        self
    }

    /// Custom body slot, pre-boxed.
    pub fn body_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.body = Some(widget);
        self
    }

    /// Right-aligned trailing meta text (e.g. "20 chapters", "free-form
    /// notes"). Tints to the accent color when the tile is selected. Most
    /// useful with the compact vertical arrangement. Ignored when a
    /// [`trailing_slot`](Self::trailing_slot) is set.
    pub fn trailing(mut self, text: impl Into<LocalizedString>) -> Self {
        self.trailing = Some(text.into());
        self
    }

    /// Arbitrary right-aligned trailing widget (badge, count, chevron, …).
    /// Takes precedence over [`trailing`](Self::trailing).
    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(Box::new(widget));
        self
    }

    /// Compact single-line arrangement: `[indicator] [icon] [title] [Spacer]
    /// [trailing]` with no description row — the vertical settings-list look.
    /// `RadioTileGroup::layout(TileLayout::Vertical)` sets this automatically
    /// (and moves the indicator to the leading edge).
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Override the title text style (default `TextStyleRole::BodyBold`).
    pub fn title_style(mut self, style: impl Into<TextStyleProp>) -> Self {
        self.title_style = Some(style.into());
        self
    }

    /// Override the title text color (default `TextRole::Primary`).
    pub fn title_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.title_color = Some(color.into());
        self
    }

    /// Override the description text style (default `TextStyleRole::Small`).
    pub fn description_style(mut self, style: impl Into<TextStyleProp>) -> Self {
        self.description_style = Some(style.into());
        self
    }

    /// Override the description text color (default `TextRole::Secondary`).
    pub fn description_color(mut self, color: impl Into<ColorProp>) -> Self {
        self.description_color = Some(color.into());
        self
    }

    /// Set the enabled state, statically or reactively. A disabled tile
    /// is skipped by the group's keyboard navigation and cannot be
    /// selected.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Pick the card variant (default `Outlined`).
    pub fn variant(mut self, variant: RadioTileVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Whether to render the inline radio indicator (default `true`). When
    /// `false`, the selection cue is the card highlight alone.
    pub fn show_indicator(mut self, show: bool) -> Self {
        self.show_indicator = show;
        self
    }

    /// Which side of the top row the radio indicator sits on (default `Trailing`).
    pub fn indicator_side(mut self, side: RadioTileIndicatorSide) -> Self {
        self.indicator_side = side;
        self
    }

    /// Per-call style override — replaces the theme-wide `RadioTileStyle`
    /// for just this tile.
    pub fn style(mut self, style: impl RadioTileStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Attach a plain single-line tooltip shown on hover.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip resolved from the app-wide tooltip registry.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip driven by inline `TooltipContent`.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip hosting an arbitrary widget tree.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }

    // --- Injected by RadioTileGroup at build time (not public API) ---

    pub(crate) fn set_selection(&mut self, value: usize, selected: Signal<usize>) {
        self.value = value;
        self.selected = selected;
    }

    pub(crate) fn set_grouped(
        &mut self,
        group_focused: Signal<bool>,
        group_ids: Rc<RefCell<Vec<WidgetId>>>,
        pos: usize,
        size: usize,
    ) {
        self.grouped = true;
        self.group_focused = Some(group_focused);
        self.group_ids = Some(group_ids);
        self.pos_in_set = Some(pos);
        self.size_of_set = Some(size);
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Switch this tile to the compact vertical-list arrangement with a
    /// leading radio indicator. Called by `RadioTileGroup` for
    /// [`TileLayout::Vertical`](crate::radio_tile_group::TileLayout::Vertical).
    pub(crate) fn set_vertical_arrangement(&mut self) {
        self.compact = true;
        self.indicator_side = RadioTileIndicatorSide::Leading;
    }

    /// Set where this tile's tooltip opens. Called by `RadioTileGroup` — a
    /// vertical group (`Column` / `Vertical`) passes `Side` so the tooltip
    /// opens beside the tile instead of covering the tile below.
    pub(crate) fn set_tooltip_placement(&mut self, placement: crate::tooltip::TooltipPlacement) {
        self.tooltip_placement = placement;
    }

    /// Apply a group-level style only when this tile has no per-call style of
    /// its own (the tile's own `.style(...)` wins). Called by `RadioTileGroup`.
    pub(crate) fn set_style_if_unset(&mut self, style: SharedRadioTileStyle) {
        if self.style_override.is_none() {
            self.style_override = Some(style);
        }
    }

    fn is_selected(&self) -> bool {
        self.selected.get() == self.value
    }
}

impl Default for RadioTile {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RadioTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioTile")
            .field("value", &self.value)
            .field("title", &self.title)
            .field("grouped", &self.grouped)
            .finish()
    }
}

impl Widget for RadioTile {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let selected = self.selected.clone();
        let value = self.value;
        let variant = self.variant;
        let self_id = ctx.self_id();

        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Re-walk the AT tree when selection changes so `set_toggled` (and the
        // group's `active_descendant`) stay current — selection is otherwise a
        // repaint-only change. Matches GridView's selection binding.
        {
            let registry = ctx.binding_registry();
            self.selected
                .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
        }

        let interaction = ctx.signal(InteractionState::Idle);

        let is_selected = selected.map(move |s| *s == value);
        let is_hovered = interaction.map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = interaction.map(|s| matches!(s, InteractionState::Pressed));
        let is_disabled = effective_enabled.map(|on| !*on);
        // Focus source: the group's focus when grouped (roving radiogroup),
        // else this tile's own focus.
        let is_focused = if let Some(gf) = &self.group_focused {
            gf.clone()
        } else {
            interaction.map(|s| matches!(s, InteractionState::Focused))
        };
        let is_focus_visible = ctx.focus_visible();
        let is_window_active = ctx.window_active_signal();

        // --- Radio indicator: reuse the theme's RadioStyle so the glyph
        // matches a standalone RadioButton. The glyph never draws its own
        // focus ring (the tile owns the ring), so pass a constant `false`.
        let indicator_id = if self.show_indicator {
            let radio_style: SharedRadioStyle = ctx
                .theme()
                .style_slots
                .radio
                .clone()
                .unwrap_or_else(|| Rc::new(RecipeRadioStyle::default()));
            let radio_cfg = RadioStyleConfig {
                is_selected: is_selected.clone(),
                is_hovered: is_hovered.clone(),
                is_pressed: is_pressed.clone(),
                is_focused: Signal::new(false),
                is_disabled: is_disabled.clone(),
                variant: RadioVariant::Circle,
            };
            Some(radio_style.make_body(&radio_cfg, ctx))
        } else {
            None
        };

        // --- Top row: [icon?] [title] [Spacer] [indicator?] (indicator side
        // configurable; RTL handled by HStack + Spacer).
        let mut top_row = HStack::new()
            .spacing(TILE_ROW_GAP)
            .alignment(VAlignment::Center);

        if self.indicator_side == RadioTileIndicatorSide::Leading
            && let Some(id) = indicator_id
        {
            top_row = top_row.add_child(id);
        }
        if let Some(icon) = self.icon.take() {
            let icon_id = ctx.add_boxed(icon);
            top_row = top_row.add_child(icon_id);
        }
        if let Some(title) = &self.title {
            let title_widget = TextWidget::new(title.clone())
                .style(
                    self.title_style
                        .clone()
                        .unwrap_or(TextStyleProp::Role(TextStyleRole::BodyBold)),
                )
                .color(
                    self.title_color
                        .clone()
                        .unwrap_or(ColorProp::TextRole(TextRole::Primary)),
                )
                .single_line()
                .a11y_hidden();
            let title_id = ctx.add(title_widget);
            top_row = top_row.add_child(title_id);
        }
        top_row = top_row.add_child(ctx.add(Spacer::new()));
        // Trailing meta (right-aligned). Typed text tints to accent when
        // selected (the "20 chapters" cue); a custom slot is used as-is.
        if let Some(slot) = self.trailing_slot.take() {
            top_row = top_row.add_child(ctx.add_boxed(slot));
        } else if let Some(trailing) = &self.trailing {
            let trailing_color = is_selected.map(|s| {
                if *s {
                    TextRole::Accent
                } else {
                    TextRole::Secondary
                }
            });
            let trailing_widget = TextWidget::new(trailing.clone())
                .style(TextStyleProp::Role(TextStyleRole::Small))
                .color(trailing_color)
                .single_line()
                .a11y_hidden();
            top_row = top_row.add_child(ctx.add(trailing_widget));
        }
        if self.indicator_side == RadioTileIndicatorSide::Trailing
            && let Some(id) = indicator_id
        {
            top_row = top_row.add_child(id);
        }
        let top_row_id = ctx.add(top_row);

        // --- Content column: top row + (description|body, unless compact).
        let mut content_col = VStack::new()
            .spacing(TILE_TITLE_DESC_GAP)
            .alignment(HAlignment::Leading)
            .add_child(top_row_id);

        if !self.compact {
            if let Some(body) = self.body.take() {
                let body_id = ctx.add_boxed(body);
                content_col = content_col.add_child(body_id);
            } else if let Some(description) = &self.description {
                let desc_widget = TextWidget::new(description.clone())
                    .style(
                        self.description_style
                            .clone()
                            .unwrap_or(TextStyleProp::Role(TextStyleRole::Small)),
                    )
                    .color(
                        self.description_color
                            .clone()
                            .unwrap_or(ColorProp::TextRole(TextRole::Secondary)),
                    )
                    .a11y_hidden();
                let desc_id = ctx.add(desc_widget);
                content_col = content_col.add_child(desc_id);
            }
        }
        let content_id = ctx.add(content_col);

        // --- Card chrome via the resolved RadioTileStyle.
        let style: SharedRadioTileStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.radio_tile.clone())
            .unwrap_or_else(|| Rc::new(RecipeRadioTileStyle::default()));
        let cfg = RadioTileStyleConfig {
            content: content_id,
            is_selected: is_selected.clone(),
            is_hovered: is_hovered.clone(),
            is_pressed: is_pressed.clone(),
            is_focused,
            is_focus_visible,
            is_disabled,
            is_window_active,
            variant,
            is_compact: self.compact,
        };
        let root_id = style.make_body(&cfg, ctx);

        // Placement is `Below` by default; a vertical group (`Column` /
        // `Vertical`) injects `Side` via `set_tooltip_placement` so a tile's
        // tooltip doesn't cover the tile below.
        let tip_placement = self.tooltip_placement;
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed_with_placement(
                ctx,
                root_id,
                content,
                delay,
                tip_placement,
            );
        } else if let Some(source) = self.rich_tooltip_source.take() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source_with_placement(
                ctx,
                root_id,
                source,
                delay,
                tip_placement,
            );
        } else if let Some(tooltip_text) = self.tooltip_text.clone() {
            let tw = crate::tooltip::TooltipWidget::new(tooltip_text);
            let tid = ctx.add(tw);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip_with_placement(root_id, tid, delay, tip_placement);
        }

        self.root_child_id = Some(root_id);

        // --- Handlers. A grouped tile is not individually focusable (focus
        // roves on the group); a standalone tile is focusable and takes Space.
        let sel_tap = self.selected.clone();
        let sel_access = self.selected.clone();
        let int_tap = interaction.clone();
        let int_hover = interaction.clone();

        let mut handler_set = HandlerSet::new()
            .on_tap(move |_pos, _ctx: &mut EventContext| {
                sel_tap.set(value);
                int_tap.set(InteractionState::Hovered);
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                if entered {
                    int_hover.set(InteractionState::Hovered);
                } else {
                    int_hover.set(InteractionState::Idle);
                }
            })
            .on_access_action(
                move |action: teksilo_core::accesskit::Action, _ctx: &mut EventContext| {
                    if action == teksilo_core::accesskit::Action::Click {
                        sel_access.set(value);
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                },
            )
            .cursor(CursorIcon::Pointer);

        if !self.grouped {
            let sel_key = self.selected.clone();
            let int_key = interaction.clone();
            let int_focus = interaction.clone();
            handler_set = handler_set
                .focusable(true)
                .on_key(
                    move |event: &WidgetEvent, _ctx: &mut EventContext| match event {
                        WidgetEvent::KeyDown {
                            key: Key::Space, ..
                        } => {
                            int_key.set(InteractionState::Pressed);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyUp {
                            key: Key::Space, ..
                        } => {
                            // Lone-KeyUp guard (see RadioButton).
                            if int_key.get() != InteractionState::Pressed {
                                return EventResponse::Ignored;
                            }
                            sel_key.set(value);
                            int_key.set(InteractionState::Focused);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    },
                )
                .on_focus(move |gained: bool, _ctx: &mut EventContext| {
                    if gained {
                        if int_focus.get() == InteractionState::Idle {
                            int_focus.set(InteractionState::Focused);
                        }
                    } else {
                        int_focus.set(InteractionState::Idle);
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
            return size.into();
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
        builder.set_role(teksilo_core::accesskit::Role::RadioButton);
        if let Some(ref title) = self.title {
            builder.set_name(title.resolve_now());
        }
        if let Some(ref description) = self.description {
            builder.set_description(description.resolve_now());
        } else if let Some(ref trailing) = self.trailing {
            // In the compact arrangement the trailing meta carries the
            // secondary info, so expose it as the accessible description.
            builder.set_description(trailing.resolve_now());
        }
        // ARIA role="radio" uses aria-checked (→ AccessKit `toggled`).
        builder.set_toggled(self.is_selected());
        // "N of M" positional info (set by the group).
        if let Some(pos) = self.pos_in_set {
            builder.set_position_in_set(pos);
        }
        if let Some(size) = self.size_of_set {
            builder.set_size_of_set(size);
        }
        // Radio-group membership — each tile declares every sibling (incl.
        // itself) so AT can announce positional info.
        if let Some(group_ids) = &self.group_ids {
            for &id in group_ids.borrow().iter() {
                builder.push_to_radio_group(teksilo_core::accessibility::widget_id_to_node_id(id));
            }
        }
        builder.add_action(teksilo_core::accesskit::Action::Click);
        // Only a standalone tile is a direct focus target; a grouped tile is
        // reached via the group's roving `active_descendant`.
        if !self.grouped {
            builder.add_action(teksilo_core::accesskit::Action::Focus);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_core::event::Modifiers;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;
    use teksilo_tokens::Color;

    #[test]
    fn standalone_tap_and_space_select() {
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let t0 = tree.add(
            RadioTile::new()
                .selection(0, selected.clone())
                .title(lit!("A")),
        );
        let t1 = tree.add(
            RadioTile::new()
                .selection(1, selected.clone())
                .title(lit!("B")),
        );
        let _root = tree.add(crate::primitives::VStack::new().add_child(t0).add_child(t1));
        tree.layout(SizeProposal::exact(300.0, 300.0));

        assert_eq!(selected.get(), 0);
        tree.click(t1);
        assert_eq!(selected.get(), 1);

        // A standalone tile is focusable and Space-selectable.
        tree.focus(t0);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert_eq!(selected.get(), 0);
    }

    #[test]
    fn accessibility_role_and_toggled() {
        let selected = Signal::new(1_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let t0 = tree.add(
            RadioTile::new()
                .selection(0, selected.clone())
                .title(lit!("A"))
                .description(lit!("first choice")),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        let info = tree.accessibility_node(t0);
        assert_eq!(info.role(), teksilo_core::accesskit::Role::RadioButton);
        assert_eq!(info.name(), Some("A"));
        assert!(!info.is_toggled());
    }

    #[test]
    fn compact_tile_omits_description_and_is_shorter() {
        use crate::primitives::{FixedSize, VStack};
        let long = "a long description that would wrap across several lines inside the tile body";
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let compact = tree.add(
            FixedSize::new().width(300.0).child(
                RadioTile::new()
                    .selection(0, selected.clone())
                    .title(lit!("A"))
                    .description(lit!(long))
                    .compact(true),
            ),
        );
        let card = tree.add(
            FixedSize::new().width(300.0).child(
                RadioTile::new()
                    .selection(0, selected.clone())
                    .title(lit!("B"))
                    .description(lit!(long)),
            ),
        );
        let _root = tree.add(VStack::new().add_child(compact).add_child(card));
        tree.layout(SizeProposal::exact(320.0, 600.0));
        let a = tree.find_by_label("A").unwrap();
        let b = tree.find_by_label("B").unwrap();
        assert!(
            tree.bounds(a).height < tree.bounds(b).height,
            "compact tile drops the wrapping description row, so it is shorter"
        );
    }

    // Sentinel style painting a distinctive fill, to exercise Tier-3 precedence.
    #[derive(Debug)]
    struct SentinelTile(Color);
    impl RadioTileStyle for SentinelTile {
        fn make_body(&self, cfg: &RadioTileStyleConfig, ctx: &mut BuildContext) -> WidgetId {
            let rect = ctx.add(crate::primitives::RectWidget::new().background(self.0));
            ctx.add(
                crate::primitives::ZStack::new()
                    .add_child(rect)
                    .add_child(cfg.content),
            )
        }
    }

    fn renders_color(tree: &mut WidgetTree, color: Color) -> bool {
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        frame.shapes.iter().any(|s| s.color == color.to_array())
    }

    #[test]
    fn theme_slot_supplies_style_when_no_override() {
        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.radio_tile =
            Some(Rc::new(SentinelTile(Color::from_rgba(1.0, 0.0, 1.0, 1.0))));
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(theme);
        tree.add(RadioTile::new().selection(0, selected).title(lit!("X")));
        assert!(
            renders_color(&mut tree, Color::from_rgba(1.0, 0.0, 1.0, 1.0)),
            "theme slot style should paint the sentinel fill"
        );
    }

    #[test]
    fn per_call_style_override_wins_over_theme_slot() {
        let mut theme = teksilo_core::presets::intui::light();
        theme.style_slots.radio_tile =
            Some(Rc::new(SentinelTile(Color::from_rgba(1.0, 0.0, 1.0, 1.0))));
        let per_call = Color::from_rgba(0.0, 1.0, 0.0, 1.0);
        let selected = Signal::new(0_usize);
        let mut tree = WidgetTree::new().with_theme(theme);
        tree.add(
            RadioTile::new()
                .selection(0, selected)
                .title(lit!("X"))
                .style(SentinelTile(per_call)),
        );
        assert!(
            renders_color(&mut tree, per_call),
            "per-call .style() should win over the theme slot"
        );
        assert!(
            !renders_color(&mut tree, Color::from_rgba(1.0, 0.0, 1.0, 1.0)),
            "theme-slot fill must not appear when overridden per-call"
        );
    }
}
