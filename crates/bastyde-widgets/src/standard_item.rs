// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Canonical row layout for `ListView` / `TreeView` delegates.
//!
//! Two widgets:
//! - [`StandardListItem`] — primary line `[checkbox?] [leading_slot?]
//!   [center_slot?] [label] [Spacer] [trailing_slot?]` with optional
//!   subtitle line `[subtitle_leading_slot?] [subtitle] [Spacer]
//!   [subtitle_trailing_slot?]`.
//! - [`StandardTreeItem`] — same plus depth-driven indent + chevron
//!   column (always reserved, even for leaves, so labels at the same
//!   depth align).
//!
//! Selection / hover / pressed background mirrors `MenuItem` /
//! `ComboBox`: rounded `RectWidget` (`item_corner_radius: 8.0`),
//! horizontally inset so corners are visible, theme-driven via
//! `SurfaceRole` so light/dark/custom themes propagate without
//! rebuild.
//!
//! ## Canonical TreeView wiring
//!
//! ```ignore
//! use bastyde::data::{TreeCheckedModel, TreeModel};
//! use bastyde::widgets::{StandardTreeItem, TreeView};
//!
//! let tree: TreeModel<Item> = ...;
//! let checks = TreeCheckedModel::new(tree.clone());
//!
//! TreeView::new_with_context(tree, move |item, entry, selected, ctx| {
//!     let mut row = StandardTreeItem::new(lit!(item.title.clone()))
//!         .from_entry(entry)
//!         .selected(selected)
//!         .leading_slot(IconWidget::from_svg(FOLDER_ICON).icon_size(16.0))
//!         .on_toggle_rc(ctx.toggle_callback());
//!     if entry.has_children {
//!         row = row.tristate_checkbox(checks.signal_for(entry.node_id));
//!     } else {
//!         row = row.checkbox(checks.bool_signal_for(entry.node_id));
//!     }
//!     Box::new(row)
//! })
//! .row_click_expands(false)   // chevron is the only toggle target
//! ```
//!
//! Wiring rules:
//! - `TreeView::new_with_context` exposes a `TreeRowContext` that
//!   yields `toggle_callback()` for chevron clicks. Pair with
//!   `.row_click_expands(false)` so body clicks don't also toggle.
//! - For tristate parent rows, bind to `signal_for(node)`. For
//!   leaves, prefer `bool_signal_for(node)` — the model's bool ↔
//!   tristate bridge runs ancestor recompute on writes either way.
//! - `from_entry(&FlatEntry)` is shorthand for
//!   `.depth(entry.depth).has_children(entry.has_children)
//!   .is_expanded(entry.is_expanded)`.
//!
//! ## Accessibility
//!
//! `StandardListItem.accessibility()` sets the row's `name` (label
//! only) and `description` (subtitle, if any) — structural role +
//! position/level/expanded/selected come from the parent's
//! `ListItemA11y` / `TreeRowA11y` wrapper. The embedded `Checkbox`
//! receives an `access_label*` override carrying the row label so
//! screen readers announce "checkbox, checked, `[label]`" rather than
//! a nameless `Role::CheckBox`. The chevron's `TwistArrow` is
//! decorative (`set_hidden`); the row's expanded state is owned by
//! the wrapper.

use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{CheckState, FlatEntry};

use bastyde_core::styles::{SharedStandardItemStyle, StandardItemStyleConfig};
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{HAlignment, TextRole, TextStyleRole, VAlignment};

use crate::button::InteractionState;
use crate::checkbox::Checkbox;
use crate::primitives::{FixedSize, HStack, Spacer, TextWidget, TwistArrow, VStack};

// ---------------------------------------------------------------------------
// CheckboxKind — two-state vs tri-state, last-call-wins on the builder.
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum CheckboxKind {
    TwoState(Signal<bool>),
    TriState(Signal<CheckState>),
}

// ---------------------------------------------------------------------------
// StandardListItem
// ---------------------------------------------------------------------------

/// Canonical single-line or two-line row layout for use in a `ListView`.
///
/// See the [module-level documentation](self) for the full slot layout and
/// wiring rules.
pub struct StandardListItem {
    label: LocalizedString,
    subtitle: Option<LocalizedString>,
    leading_slot: Option<Box<dyn Widget>>,
    center_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    subtitle_leading_slot: Option<Box<dyn Widget>>,
    subtitle_trailing_slot: Option<Box<dyn Widget>>,
    checkbox: Option<CheckboxKind>,
    selected: Signal<bool>,
    enabled: Signal<bool>,
    label_style: bastyde_core::color_prop::TextStyleProp,
    subtitle_style: bastyde_core::color_prop::TextStyleProp,
    /// Per-call label text-color override. `None` ⇒ enabled-derived
    /// (`Primary` / `Disabled`).
    label_color: Option<bastyde_core::color_prop::ColorProp>,
    /// Per-call subtitle text-color override. `None` ⇒ `TextRole::Secondary`.
    subtitle_color: Option<bastyde_core::color_prop::ColorProp>,
    interaction: Signal<InteractionState>,
    style_override: Option<SharedStandardItemStyle>,
    root_child_id: Option<WidgetId>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl StandardListItem {
    /// Create a list item with the given primary label.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls,
            subtitle: None,
            leading_slot: None,
            center_slot: None,
            trailing_slot: None,
            subtitle_leading_slot: None,
            subtitle_trailing_slot: None,
            checkbox: None,
            selected: Signal::new(false),
            enabled: Signal::new(true),
            label_style: TextStyleRole::Body.into(),
            subtitle_style: TextStyleRole::Small.into(),
            label_color: None,
            subtitle_color: None,
            interaction: Signal::new(InteractionState::Idle),
            style_override: None,
            root_child_id: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Per-call style override. Replaces the theme-wide default
    /// `StandardItemStyle` for just this row instance.
    pub fn style(mut self, style: impl bastyde_core::styles::StandardItemStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Set an optional secondary line below the primary label.
    pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.subtitle = Some(ls);
        self
    }

    /// Leading slot — placed AFTER the optional checkbox, BEFORE the
    /// center slot. Typical: `IconWidget`, avatar, color swatch.
    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot = Some(Box::new(widget));
        self
    }

    /// `Box<dyn Widget>` variant of [`leading_slot`](Self::leading_slot).
    pub fn leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.leading_slot = Some(widget);
        self
    }

    /// Center slot — placed BETWEEN the leading slot and the label.
    /// Typical: status dot, colored category bar, drag-handle gripper,
    /// key-binding chip. Distinct from `leading_slot`: leading is the
    /// row's icon identity, center is label-adjacent decoration.
    pub fn center_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.center_slot = Some(Box::new(widget));
        self
    }

    /// `Box<dyn Widget>` variant of [`center_slot`](Self::center_slot).
    pub fn center_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.center_slot = Some(widget);
        self
    }

    /// Trailing slot — placed AFTER the flex Spacer on the primary
    /// line. Typical: badge, count, status pill, secondary IconButton.
    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.trailing_slot = Some(Box::new(widget));
        self
    }

    /// `Box<dyn Widget>` variant of [`trailing_slot`](Self::trailing_slot).
    pub fn trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.trailing_slot = Some(widget);
        self
    }

    /// Leading slot for the subtitle line. No-op without `subtitle(...)`.
    pub fn subtitle_leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.subtitle_leading_slot = Some(Box::new(widget));
        self
    }

    /// `Box<dyn Widget>` variant of [`subtitle_leading_slot`](Self::subtitle_leading_slot).
    pub fn subtitle_leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.subtitle_leading_slot = Some(widget);
        self
    }

    /// Trailing slot for the subtitle line. No-op without `subtitle(...)`.
    pub fn subtitle_trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.subtitle_trailing_slot = Some(Box::new(widget));
        self
    }

    /// `Box<dyn Widget>` variant of [`subtitle_trailing_slot`](Self::subtitle_trailing_slot).
    pub fn subtitle_trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.subtitle_trailing_slot = Some(widget);
        self
    }

    /// Optional two-state checkbox at the start of the row.
    /// Mutually exclusive with `tristate_checkbox` — last call wins.
    pub fn checkbox(mut self, checked: Signal<bool>) -> Self {
        self.checkbox = Some(CheckboxKind::TwoState(checked));
        self
    }

    /// Optional tri-state checkbox bound to `Signal<CheckState>`.
    /// Cycles `Unchecked → Checked → Indeterminate`. Mutually
    /// exclusive with `checkbox` — last call wins.
    pub fn tristate_checkbox(mut self, state: Signal<CheckState>) -> Self {
        self.checkbox = Some(CheckboxKind::TriState(state));
        self
    }

    /// Set the initial selection state (static value).
    pub fn selected(mut self, b: bool) -> Self {
        self.selected = Signal::new(b);
        self
    }

    /// Bind the selection state to a reactive `Signal<bool>`.
    pub fn bind_selected(mut self, s: Signal<bool>) -> Self {
        self.selected = s;
        self
    }

    /// Set the initial enabled state (static value).
    pub fn enabled(mut self, b: bool) -> Self {
        self.enabled = Signal::new(b);
        self
    }

    /// Bind the enabled state to a reactive `Signal<bool>`.
    pub fn bind_enabled(mut self, s: Signal<bool>) -> Self {
        self.enabled = s;
        self
    }

    /// Override the label's text style (font, size, weight). Accepts a
    /// `TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default is
    /// `TextStyleRole::Body`.
    pub fn label_style(
        mut self,
        style: impl Into<bastyde_core::color_prop::TextStyleProp>,
    ) -> Self {
        self.label_style = style.into();
        self
    }

    /// Override the subtitle's text style. Default is `TextStyleRole::Small`.
    pub fn subtitle_style(
        mut self,
        style: impl Into<bastyde_core::color_prop::TextStyleProp>,
    ) -> Self {
        self.subtitle_style = style.into();
        self
    }

    /// Override the label's text color. Accepts `Color`, a role, or a
    /// `Signal` of either. Default (unset) is enabled-derived
    /// (`Primary` / `Disabled`); setting this replaces that cascade.
    pub fn label_color(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.label_color = Some(color.into());
        self
    }

    /// Override the subtitle's text color. Default (unset) is
    /// `TextRole::Secondary`.
    pub fn subtitle_color(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.subtitle_color = Some(color.into());
        self
    }

    /// Attach a plain tooltip shown after the standard hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called
    /// wins and clears the other slots.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip looked up from the global tooltip registry by key.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called
    /// wins and clears the other slots.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from an inline [`TooltipContent`](crate::tooltip::TooltipContent)
    /// value (no registry lookup required).
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`composite_tooltip`](Self::composite_tooltip) — the last setter called
    /// wins and clears the other slots.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) — the last setter
    /// called wins and clears the other slots.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for StandardListItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StandardListItem")
            .field("label", &self.label)
            .field("subtitle", &self.subtitle)
            .field("has_checkbox", &self.checkbox.is_some())
            .finish()
    }
}

fn resolve_label_role(enabled: bool) -> TextRole {
    if enabled {
        TextRole::Primary
    } else {
        TextRole::Disabled
    }
}

impl StandardListItem {
    /// Build the row content (HStack of slots + label column) and
    /// register it. Returns the WidgetId of the content node (not the
    /// surrounding bg + padding).
    fn build_content(&mut self, ctx: &mut BuildContext) -> WidgetId {
        use crate::styles::recipe_standard_item_style as si;

        let label_role = self.enabled.map(|e| resolve_label_role(*e));

        // Label column: either a single TextWidget or a VStack with
        // label on top and subtitle (with its own slots) below.
        let mut label_widget = TextWidget::new(self.label.clone())
            .style(self.label_style.clone())
            .a11y_hidden();
        label_widget = match &self.label_color {
            Some(c) => label_widget.bind_color(c.clone()),
            None => label_widget.bind_color(label_role.clone()),
        };
        let label_id = ctx.add(label_widget);

        let label_column_id = if let Some(subtitle) = &self.subtitle {
            // Two-line: VStack { label, subtitle line }.
            let mut subtitle_widget = TextWidget::new(subtitle.clone())
                .style(self.subtitle_style.clone())
                .a11y_hidden();
            subtitle_widget = match &self.subtitle_color {
                Some(c) => subtitle_widget.bind_color(c.clone()),
                None => subtitle_widget.color(TextRole::Secondary),
            };
            let subtitle_text_id = ctx.add(subtitle_widget);

            // Subtitle HStack: [leading?] subtitle [Spacer] [trailing?].
            let mut sub_row = HStack::new()
                .spacing(si::STANDARD_ITEM_SUBTITLE_SLOT_GAP)
                .alignment(VAlignment::Center);
            if let Some(w) = self.subtitle_leading_slot.take() {
                let id = ctx.add_boxed(w);
                sub_row = sub_row.add_child(id);
            }
            sub_row = sub_row
                .add_child(subtitle_text_id)
                .add_child(ctx.add(Spacer::new()));
            if let Some(w) = self.subtitle_trailing_slot.take() {
                let id = ctx.add_boxed(w);
                sub_row = sub_row.add_child(id);
            }
            let sub_row_id = ctx.add(sub_row);

            ctx.add(
                VStack::new()
                    .spacing(si::STANDARD_ITEM_LABEL_SUBTITLE_GAP)
                    .alignment(HAlignment::Leading)
                    .add_child(label_id)
                    .add_child(sub_row_id),
            )
        } else {
            // Single-line: just the label.
            label_id
        };

        // Primary HStack: [checkbox?] [leading?] [center?] label_column
        // [Spacer] [trailing?].
        let mut row = HStack::new()
            .spacing(si::STANDARD_ITEM_SLOT_GAP)
            .alignment(VAlignment::Center);

        if let Some(kind) = self.checkbox.take() {
            // Propagate the row's label as the checkbox's accessible
            // name. With `labels_hidden(true)` the visual label is
            // suppressed; without an `access_label*` override the AT
            // node would be a nameless `Role::CheckBox`. Using
            // `access_label_literal` on the WidgetBuilder applies an
            // override AFTER Checkbox::accessibility runs, so the
            // screen reader announces e.g. "checkbox, checked, Save"
            // when the user navigates to it.
            use bastyde_core::widget_builder::WidgetBuilder;
            let cb = match kind {
                CheckboxKind::TwoState(s) => Checkbox::new(s),
                CheckboxKind::TriState(s) => Checkbox::tristate(s),
            }
            .labels_hidden(true);
            let cb_id = ctx.add(cb.access_label(self.label.clone()));
            row = row.add_child(cb_id);
        }
        if let Some(w) = self.leading_slot.take() {
            let id = ctx.add_boxed(w);
            row = row.add_child(id);
        }
        if let Some(w) = self.center_slot.take() {
            let id = ctx.add_boxed(w);
            row = row.add_child(id);
        }
        row = row
            .add_child(label_column_id)
            .add_child(ctx.add(Spacer::new()));
        if let Some(w) = self.trailing_slot.take() {
            let id = ctx.add_boxed(w);
            row = row.add_child(id);
        }

        ctx.add(row)
    }

    /// Wrap an already-composed row content in the active
    /// `StandardItemStyle` chrome (selection background + corner
    /// radius + padding) and attach the row-level hover handler.
    /// Shared by `StandardListItem::build` (passing its inner row)
    /// and `StandardTreeItem::build` (passing the row prefixed with
    /// indent + chevron columns).
    fn build_with_background(&mut self, ctx: &mut BuildContext, content_id: WidgetId) -> WidgetId {
        // Derive the cfg's boolean signals from the widget's existing
        // `interaction` + `selected` + `enabled` signals. The recipe
        // re-evaluates the bg role on any source change.
        let is_selected = self.selected.clone();
        let is_disabled = self.enabled.map(|e| !*e);
        let is_hovered = self
            .interaction
            .map(|s| matches!(s, InteractionState::Hovered));
        let is_pressed = self
            .interaction
            .map(|s| matches!(s, InteractionState::Pressed));
        // Focus-aware selection: `is_focused` tracks whether this item's focus
        // scope (its nearest focusable ancestor — the enclosing ListView /
        // TreeView / … or any focusable container) holds keyboard focus. The
        // recipe paints the active `Selected` chrome while it does and the muted
        // `SelectedInactive` chrome when focus is elsewhere. Items outside any
        // focusable scope read a constant `true`, so their selection always
        // looks active.
        let is_focused = ctx.view_focus_active();
        // Keyboard-vs-pointer modality so the recipe shows the focus ring only
        // during keyboard navigation (`:focus-visible`).
        let is_focus_visible = ctx.focus_visible();

        let style: SharedStandardItemStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.standard_item.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeStandardItemStyle::default()));
        let cfg = StandardItemStyleConfig {
            content: content_id,
            is_selected,
            is_hovered,
            is_pressed,
            is_focused,
            is_focus_visible,
            is_disabled,
        };
        let root_id = style.make_body(&cfg, ctx);

        // Attach hover handler to the row so hovering anywhere in the
        // row updates the interaction signal. Disabled rows still
        // track hover but the recipe's bg cascade short-circuits to
        // Transparent.
        use bastyde_core::widget_builder::HandlerSet;
        let interaction_for_hover = self.interaction.clone();
        let handlers = HandlerSet::new().on_hover(move |entered: bool, _ctx: &mut EventContext| {
            interaction_for_hover.set(if entered {
                InteractionState::Hovered
            } else {
                InteractionState::Idle
            });
        });
        ctx.apply_self_handlers(handlers);

        root_id
    }
}

impl Widget for StandardListItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Bridge the widget's owned `self.enabled` signal into the
        // arena's enabled_state. Now event-gating, focus traversal,
        // a11y disabled, and the leaves' role-substitution all
        // observe the same source. Previously `self.enabled` was
        // widget-internal — events still routed to disabled items,
        // and external `ctx.enabled_when(item_id, …)` would not
        // override the local Signal.
        ctx.enabled_when(self_id, self.enabled.clone());
        let content_id = self.build_content(ctx);
        let root_id = self.build_with_background(ctx, content_id);
        self.root_child_id = Some(root_id);

        // Attach tooltip — mutually exclusive slots, composite wins.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        use crate::styles::recipe_standard_item_style as si;
        let min_height = if self.subtitle.is_some() {
            si::STANDARD_ITEM_MIN_HEIGHT_TWO_LINE
        } else {
            si::STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE
        };
        let raw = self
            .root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, min_height));
        let height = raw.height.max(min_height);
        // Honor the proposed width when offered. The inner ZStack reports
        // only the chrome's natural width (padding insets) under any
        // proposal, so standalone rows in a VStack would collapse to ~16 px
        // and the label would render in a zero-width box. Inside a
        // ListView the row gets an exact-width proposal so this just
        // reflects that.
        let width = proposal.width.unwrap_or(raw.width);
        bastyde_canvas::Size::new(width, height).into()
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
        // The row's parent (ListView's `ListItemA11y`, TreeView's
        // `TreeRowA11y`, TreeTableView's `TreeRowA11y`) already sets the
        // structural role + position-in-set + selected/expanded
        // state. We only contribute the row's name + description
        // here.
        //
        // Name = label. Subtitle goes to `description` (a separate
        // AccessKit field) rather than concatenated into the name —
        // matches the AccessKit semantic and lets screen readers
        // present them as primary vs supplementary.
        builder.set_name(self.label.clone());
        if let Some(subtitle) = &self.subtitle {
            builder.set_description(subtitle.clone());
        }
        // Mirror enabled state. AccessKit's `set_disabled` is a flag
        // (no boolean clear); the framework's accessibility-override
        // layer can clear it via `access_disabled(false)` if needed.
        // Framework a11y walker calls `set_disabled` from arena state.
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// StandardTreeItem
// ---------------------------------------------------------------------------

/// Canonical row layout for a `TreeView` — [`StandardListItem`] plus
/// a depth-driven indent column and an always-reserved chevron column.
///
/// See the [module-level documentation](self) for the canonical `TreeView`
/// wiring pattern and wiring rules.
pub struct StandardTreeItem {
    inner: StandardListItem,
    depth: usize,
    has_children: bool,
    is_expanded: Signal<bool>,
    on_toggle: Option<Rc<dyn Fn(&mut bastyde_core::widget::EventContext)>>,
}

impl StandardTreeItem {
    /// Create a tree item with the given primary label.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            inner: StandardListItem::new(label),
            depth: 0,
            has_children: false,
            is_expanded: Signal::new(false),
            on_toggle: None,
        }
    }

    // Forward all StandardListItem builders ----------------------------------

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`subtitle`](StandardListItem::subtitle).
    pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self {
        self.inner = self.inner.subtitle(text);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`leading_slot`](StandardListItem::leading_slot).
    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.leading_slot(widget);
        self
    }

    /// `Box<dyn Widget>` variant of [`leading_slot`](Self::leading_slot).
    pub fn leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.leading_slot_boxed(widget);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`center_slot`](StandardListItem::center_slot).
    pub fn center_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.center_slot(widget);
        self
    }

    /// `Box<dyn Widget>` variant of [`center_slot`](Self::center_slot).
    pub fn center_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.center_slot_boxed(widget);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`trailing_slot`](StandardListItem::trailing_slot).
    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.trailing_slot(widget);
        self
    }

    /// `Box<dyn Widget>` variant of [`trailing_slot`](Self::trailing_slot).
    pub fn trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.trailing_slot_boxed(widget);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`subtitle_leading_slot`](StandardListItem::subtitle_leading_slot).
    pub fn subtitle_leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.subtitle_leading_slot(widget);
        self
    }

    /// `Box<dyn Widget>` variant of
    /// [`subtitle_leading_slot`](Self::subtitle_leading_slot).
    pub fn subtitle_leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.subtitle_leading_slot_boxed(widget);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`subtitle_trailing_slot`](StandardListItem::subtitle_trailing_slot).
    pub fn subtitle_trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.subtitle_trailing_slot(widget);
        self
    }

    /// `Box<dyn Widget>` variant of
    /// [`subtitle_trailing_slot`](Self::subtitle_trailing_slot).
    pub fn subtitle_trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.subtitle_trailing_slot_boxed(widget);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`checkbox`](StandardListItem::checkbox).
    pub fn checkbox(mut self, checked: Signal<bool>) -> Self {
        self.inner = self.inner.checkbox(checked);
        self
    }

    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`tristate_checkbox`](StandardListItem::tristate_checkbox).
    pub fn tristate_checkbox(mut self, state: Signal<CheckState>) -> Self {
        self.inner = self.inner.tristate_checkbox(state);
        self
    }

    /// Set the initial selection state (static value).
    pub fn selected(mut self, b: bool) -> Self {
        self.inner = self.inner.selected(b);
        self
    }

    /// Bind the selection state to a reactive `Signal<bool>`.
    pub fn bind_selected(mut self, s: Signal<bool>) -> Self {
        self.inner = self.inner.bind_selected(s);
        self
    }

    /// Set the initial enabled state (static value).
    pub fn enabled(mut self, b: bool) -> Self {
        self.inner = self.inner.enabled(b);
        self
    }

    /// Bind the enabled state to a reactive `Signal<bool>`.
    pub fn bind_enabled(mut self, s: Signal<bool>) -> Self {
        self.inner = self.inner.bind_enabled(s);
        self
    }

    /// Override the label's text style. Forwarded to the inner
    /// [`StandardListItem`] — see its
    /// [`label_style`](StandardListItem::label_style).
    pub fn label_style(
        mut self,
        style: impl Into<bastyde_core::color_prop::TextStyleProp>,
    ) -> Self {
        self.inner = self.inner.label_style(style);
        self
    }

    /// Override the subtitle's text style. Forwarded to the inner
    /// [`StandardListItem`] — see its
    /// [`subtitle_style`](StandardListItem::subtitle_style).
    pub fn subtitle_style(
        mut self,
        style: impl Into<bastyde_core::color_prop::TextStyleProp>,
    ) -> Self {
        self.inner = self.inner.subtitle_style(style);
        self
    }

    /// Override the label's text color. Forwarded to the inner
    /// [`StandardListItem`] — see its `label_color(...)`.
    pub fn label_color(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.inner = self.inner.label_color(color);
        self
    }

    /// Override the subtitle's text color. Forwarded to the inner
    /// [`StandardListItem`] — see its `subtitle_color(...)`.
    pub fn subtitle_color(mut self, color: impl Into<bastyde_core::color_prop::ColorProp>) -> Self {
        self.inner = self.inner.subtitle_color(color);
        self
    }

    /// Per-call style override for the row chrome. Forwarded to the
    /// inner [`StandardListItem`] — see its `style(...)` for the
    /// precedence rules (per-call > theme.style_slots.standard_item >
    /// `RecipeStandardItemStyle`).
    pub fn style(mut self, style: impl bastyde_core::styles::StandardItemStyle) -> Self {
        self.inner = self.inner.style(style);
        self
    }

    /// Attach a plain tooltip shown after the standard hover delay.
    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`tooltip`](StandardListItem::tooltip).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.inner = self.inner.tooltip(text);
        self
    }

    /// Attach a rich tooltip looked up from the global tooltip registry by key.
    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`rich_tooltip`](StandardListItem::rich_tooltip).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.inner = self.inner.rich_tooltip(key);
        self
    }

    /// Attach a rich tooltip from an inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) value.
    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`rich_tooltip_content`](StandardListItem::rich_tooltip_content).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.inner = self.inner.rich_tooltip_content(content);
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    /// Forwarded to the inner [`StandardListItem`] — see its
    /// [`composite_tooltip`](StandardListItem::composite_tooltip).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.inner = self.inner.composite_tooltip(content);
        self
    }

    // Tree-specific ---------------------------------------------------------

    /// Set the indent depth (0 = root level). Each level adds one
    /// `STANDARD_ITEM_TREE_INDENT_STEP` of leading whitespace.
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Declare whether the node has children, which determines whether the
    /// chevron column is interactive or decorative-only.
    pub fn has_children(mut self, has: bool) -> Self {
        self.has_children = has;
        self
    }

    /// Set the initial expanded state (static value).
    pub fn is_expanded(mut self, b: bool) -> Self {
        self.is_expanded = Signal::new(b);
        self
    }

    /// Bind the expanded state to a reactive `Signal<bool>`.
    pub fn bind_is_expanded(mut self, s: Signal<bool>) -> Self {
        self.is_expanded = s;
        self
    }

    /// Convenience for the TreeView delegate path:
    /// `.from_entry(entry)` sets depth + has_children + is_expanded.
    pub fn from_entry(self, entry: &FlatEntry) -> Self {
        self.depth(entry.depth)
            .has_children(entry.has_children)
            .is_expanded(entry.is_expanded)
    }

    /// Click handler for the chevron. Wired only when `has_children`
    /// is true. Typical use: `.on_toggle(ctx.toggle_callback())` from
    /// a `TreeRowContext` (see `TreeView::new_with_context`).
    ///
    /// The callback receives the firing [`EventContext`] so apps can
    /// dispatch an intent (e.g. lazy-load children on expand), open
    /// a dialog, or otherwise route the toggle through the framework
    /// before mutating model state.
    pub fn on_toggle(
        mut self,
        f: impl Fn(&mut bastyde_core::widget::EventContext) + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    /// Variant accepting an already-`Rc`'d callback. Useful when the
    /// same callback is shared across multiple call sites without an
    /// extra clone — e.g. `TreeRowContext::toggle_callback()` returns
    /// this shape directly.
    pub fn on_toggle_rc(mut self, f: Rc<dyn Fn(&mut bastyde_core::widget::EventContext)>) -> Self {
        self.on_toggle = Some(f);
        self
    }
}

impl std::fmt::Debug for StandardTreeItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StandardTreeItem")
            .field("inner", &self.inner)
            .field("depth", &self.depth)
            .field("has_children", &self.has_children)
            .finish()
    }
}

impl Widget for StandardTreeItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_standard_item_style as si;

        // Bridge `self.inner.enabled` into the arena — same pattern
        // as StandardListItem. The chevron + indent siblings inherit
        // disabled via the ancestor walk.
        let self_id = ctx.self_id();
        ctx.enabled_when(self_id, self.inner.enabled.clone());

        // 1. Build the StandardListItem's inner row (no bg yet).
        let inner_content_id = self.inner.build_content(ctx);

        // 2. Indent column — empty FixedSize at `depth * step` width.
        let indent_width = self.depth as f32 * si::STANDARD_ITEM_TREE_INDENT_STEP;
        let indent_id = ctx.add(FixedSize::new().bind_width(indent_width));

        // 3. Chevron column — always reserved width so siblings at
        //    the same depth align. `TwistArrow` paints nothing for
        //    leaves. The click is wired via `TwistArrow::on_click`
        //    (which already installs a transparent hit-target rect
        //    + tap recognizer on its own node) — more direct than
        //    `FixedSize.on_tap`, which routes taps through the column
        //    wrapper and the composed parent chain.
        let chevron_size = si::STANDARD_ITEM_CHEVRON_COLUMN_WIDTH;
        let mut chevron = TwistArrow::new(chevron_size, self.has_children, self.is_expanded.get());
        if self.has_children
            && let Some(cb) = self.on_toggle.clone()
        {
            chevron = chevron.on_click(move |ctx| cb(ctx));
        }
        let chevron_column_id = ctx.add(FixedSize::new().bind_width(chevron_size).child(chevron));

        // 4. Outer HStack: indent | chevron column | inner row.
        let outer_row_id = ctx.add(
            HStack::new()
                .spacing(0.0)
                .alignment(VAlignment::Center)
                .add_child(indent_id)
                .add_child(chevron_column_id)
                .add_child(inner_content_id),
        );

        // 5. Wrap with the rounded selection bg + interaction handler
        //    via the inner's helper.
        let root_id = self.inner.build_with_background(ctx, outer_row_id);

        self.inner.root_child_id = Some(root_id);

        // Attach tooltip — forwarded from the inner item's tooltip slots.
        if let Some(content) = self.inner.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, root_id, content, delay);
        } else if let Some(source) = self.inner.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, root_id, source, delay);
        } else if let Some(text) = self.inner.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(root_id, tooltip_id, delay);
        }

        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.inner.layout_response(proposal, ctx)
    }

    fn place_children(
        &self,
        bounds: Rect,
        proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        self.inner.place_children(bounds, proposal, children, ctx);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        self.inner.accessibility(builder);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.inner.children()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde_canvas::SizeProposal;
    use bastyde_core::Theme;
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;

    fn theme() -> Theme {
        bastyde_core::presets::intui::light()
    }

    #[test]
    fn list_item_layout_single_line() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Hello")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let b = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        assert!(b.height >= si::STANDARD_ITEM_MIN_HEIGHT_SINGLE_LINE - 0.5);
    }

    #[test]
    fn list_item_layout_two_line() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Title")).subtitle(lit!("Subtitle text")));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let b = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        assert!(
            b.height >= si::STANDARD_ITEM_MIN_HEIGHT_TWO_LINE - 0.5,
            "two-line height {} < expected {}",
            b.height,
            si::STANDARD_ITEM_MIN_HEIGHT_TWO_LINE
        );
    }

    #[test]
    fn list_item_a11y_name_is_label_only() {
        // Subtitle goes to `description`, not concatenated into the
        // name. Lets screen readers present primary vs supplementary
        // info distinctly.
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Title")).subtitle(lit!("Subtitle")));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.name(), Some("Title"));
    }

    #[test]
    fn list_item_a11y_name_no_subtitle() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Just a title")));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.name(), Some("Just a title"));
    }

    #[test]
    fn list_item_with_checkbox_two_state() {
        use bastyde_core::signal::Signal;
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(theme());
        let _id =
            tree.add(StandardListItem::new(lit!("Item with checkbox")).checkbox(checked.clone()));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        // Just verify the build succeeds with the checkbox attached.
        // Toggle behavior is exercised by Checkbox's own tests.
        assert!(!checked.get());
    }

    #[test]
    fn list_item_with_tristate_checkbox() {
        use bastyde_core::signal::Signal;
        let state = Signal::new(CheckState::Indeterminate);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Folder")).tristate_checkbox(state.clone()));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
    }

    #[test]
    fn tree_item_chevron_reserved_for_leaf() {
        // Leaf and branch at same depth should produce identical
        // outer widths (chevron column reserved).
        let mut tree = WidgetTree::new().with_theme(theme());
        let leaf = tree.add(
            StandardTreeItem::new(lit!("file"))
                .depth(1)
                .has_children(false),
        );
        let branch = tree.add(
            StandardTreeItem::new(lit!("folder"))
                .depth(1)
                .has_children(true),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let bl = tree.bounds(leaf);
        let bb = tree.bounds(branch);
        assert!((bl.width - bb.width).abs() < 0.5);
    }

    #[test]
    fn twist_arrow_on_click_baseline() {
        // Ensure TwistArrow's own on_click(Fn() + 'static) wiring
        // works in isolation. If this fires but the StandardTreeItem
        // chevron path doesn't, the bug is in the StandardTreeItem
        // composition, not in the underlying widgets.
        use bastyde_canvas::Point;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let f = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id =
            tree.add(TwistArrow::new(20.0, true, false).on_click(move |_ctx| f.set(f.get() + 1)));
        tree.layout(SizeProposal::exact(40.0, 40.0));
        let b = tree.bounds(id);
        dispatch_tap(
            &mut tree,
            Point::new(b.x + b.width * 0.5, b.y + b.height * 0.5),
        );
        assert_eq!(fired.get(), 1, "TwistArrow.on_click() must fire on tap");
    }

    #[test]
    fn fixed_size_wrapping_twist_arrow_on_tap_baseline() {
        // If on_tap on a FixedSize that wraps a TwistArrow works
        // here, the issue with StandardTreeItem's chevron is
        // composition (parent siblings) — not the chevron-column
        // shape itself.
        use bastyde_canvas::Point;
        use bastyde_core::widget_builder::WidgetBuilder;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let f = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            FixedSize::new()
                .bind_width(20.0_f32)
                .child(TwistArrow::new(20.0, true, false))
                .on_tap(move |_, _| f.set(f.get() + 1)),
        );
        tree.layout(SizeProposal::exact(40.0, 40.0));
        let b = tree.bounds(id);
        dispatch_tap(
            &mut tree,
            Point::new(b.x + b.width * 0.5, b.y + b.height * 0.5),
        );
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn fixed_size_on_tap_baseline() {
        // Sanity check: confirm `FixedSize::new().on_tap(...)` even
        // fires when constructed via the WidgetBuilder chain. If this
        // breaks, the StandardTreeItem chevron-tap path is doomed.
        use bastyde_canvas::Point;
        use bastyde_core::widget_builder::WidgetBuilder;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let f = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            FixedSize::new()
                .bind_width(40.0_f32)
                .bind_height(40.0_f32)
                .child(TextWidget::new(lit!("x")))
                .on_tap(move |_, _| f.set(f.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let b = tree.bounds(id);
        dispatch_tap(&mut tree, Point::new(b.x + 20.0, b.y + 20.0));
        assert_eq!(fired.get(), 1);
    }

    fn dispatch_tap(tree: &mut WidgetTree, position: bastyde_canvas::Point) {
        use bastyde_core::event::{Modifiers, PointerButton, WidgetEvent};
        tree.dispatch_event(WidgetEvent::PointerDown {
            position,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    #[test]
    fn list_item_checkbox_two_state_toggles_via_tap() {
        use bastyde_canvas::Point;
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Row")).checkbox(checked.clone()));
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        // Checkbox sits at the row's leading edge, just inside the
        // bg_horizontal_inset + padding_horizontal. Tap a few pixels
        // in from there so we land on the box visual.
        let cb_x = bounds.x
            + si::STANDARD_ITEM_BG_HORIZONTAL_INSET
            + si::STANDARD_ITEM_PADDING_HORIZONTAL
            + 4.0;
        let cb_y = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(cb_x, cb_y));
        assert!(
            checked.get(),
            "tap on checkbox should flip the bound signal"
        );
        dispatch_tap(&mut tree, Point::new(cb_x, cb_y));
        assert!(!checked.get(), "second tap should flip back");
    }

    #[test]
    fn list_item_row_tap_outside_checkbox_does_not_toggle() {
        use bastyde_canvas::Point;
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new(lit!("A long-enough label so the tap target lands on text"))
                .checkbox(checked.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        // Tap far to the right of the checkbox (well past the
        // checkbox column) — should land on the label area.
        let label_x = bounds.x + bounds.width * 0.7;
        let label_y = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(label_x, label_y));
        assert!(
            !checked.get(),
            "tap on row body must not toggle the embedded checkbox"
        );
    }

    #[test]
    fn tree_item_chevron_tap_fires_on_toggle() {
        use bastyde_canvas::Point;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let fired_clone = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardTreeItem::new(lit!("Folder"))
                .depth(0)
                .has_children(true)
                .is_expanded(false)
                .on_toggle(move |_ctx| fired_clone.set(fired_clone.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        // Inside the row's content padding the chevron column sits at
        // `padding_horizontal` (depth=0 → indent=0). Sample its
        // center.
        let cx = bounds.x
            + si::STANDARD_ITEM_PADDING_HORIZONTAL
            + si::STANDARD_ITEM_CHEVRON_COLUMN_WIDTH * 0.5;
        let cy = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(
            fired.get(),
            1,
            "tap on chevron column should fire on_toggle exactly once"
        );
    }

    #[test]
    fn tristate_checkbox_user_click_never_sets_indeterminate() {
        // The user can't set a checkbox to "half" by clicking. The
        // tristate cycle on user input is Unchecked ↔ Checked;
        // Indeterminate is reserved for model-driven aggregation.
        use bastyde_canvas::Point;
        let state = Signal::new(CheckState::Unchecked);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Folder")).tristate_checkbox(state.clone()));
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        let cx = bounds.x + si::STANDARD_ITEM_PADDING_HORIZONTAL + 8.0;
        let cy = bounds.y + bounds.height * 0.5;
        // Click 1: Unchecked → Checked
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(state.get(), CheckState::Checked);
        // Click 2: Checked → Unchecked (NOT Indeterminate)
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(state.get(), CheckState::Unchecked);
        // Click 3: Unchecked → Checked again
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(state.get(), CheckState::Checked);
    }

    #[test]
    fn tristate_checkbox_user_click_from_indeterminate_goes_to_checked() {
        // Common in tree-folder selection: when the parent shows
        // partial state (some children checked) and the user clicks
        // it, the whole subtree should become checked.
        use bastyde_canvas::Point;
        let state = Signal::new(CheckState::Indeterminate);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Folder")).tristate_checkbox(state.clone()));
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        let cx = bounds.x + si::STANDARD_ITEM_PADDING_HORIZONTAL + 8.0;
        let cy = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(state.get(), CheckState::Checked);
    }

    #[test]
    fn tree_item_no_toggle_when_no_children() {
        use bastyde_canvas::Point;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let fired_clone = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardTreeItem::new(lit!("Leaf"))
                .depth(0)
                .has_children(false)
                .on_toggle(move |_ctx| fired_clone.set(fired_clone.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        use crate::styles::recipe_standard_item_style as si;
        let cx = bounds.x
            + si::STANDARD_ITEM_PADDING_HORIZONTAL
            + si::STANDARD_ITEM_CHEVRON_COLUMN_WIDTH * 0.5;
        let cy = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(
            fired.get(),
            0,
            "leaf rows must not wire on_toggle even if a callback was set"
        );
    }

    #[test]
    fn tree_item_from_entry_sets_depth_and_state() {
        use bastyde_data::TreeModel;
        let m = TreeModel::<&str>::new();
        let root = m.insert_root(0, "r");
        let _child = m.insert_child(root, 0, "c");

        let entry = FlatEntry {
            node_id: root,
            depth: 1,
            has_children: true,
            is_expanded: true,
        };
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardTreeItem::new(lit!("x")).from_entry(&entry));
        tree.layout(SizeProposal::exact(400.0, 100.0));
        assert!(tree.bounds(id).width > 0.0);
    }

    #[test]
    fn list_item_tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new(lit!("Row")).tooltip(lit!("Tip")));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("Tip").is_some());
    }

    #[test]
    fn tree_item_tooltip_appears_on_hover() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardTreeItem::new(lit!("Node")).tooltip(lit!("TreeTip")));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.pointer_move(tree.bounds(id).center());
        tree.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "tooltip should appear on hover"
        );
        assert!(tree.find_by_label("TreeTip").is_some());
    }
}
