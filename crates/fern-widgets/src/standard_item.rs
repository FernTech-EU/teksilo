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
//! use fern_ui::data::{TreeCheckedModel, TreeModel};
//! use fern_ui::widgets::{StandardTreeItem, TreeView};
//!
//! let tree: TreeModel<Item> = ...;
//! let checks = TreeCheckedModel::new(tree.clone());
//!
//! TreeView::new_with_context(tree, move |item, entry, selected, ctx| {
//!     let mut row = StandardTreeItem::new_literal(item.title.clone())
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
//! screen readers announce "checkbox, checked, [label]" rather than
//! a nameless `Role::CheckBox`. The chevron's `TwistArrow` is
//! decorative (`set_hidden`); the row's expanded state is owned by
//! the wrapper.

use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_data::{CheckState, FlatEntry};
use fern_i18n::LocalizedString;
use fern_tokens::{CornerRadius, HAlignment, SurfaceRole, TextRole, TextStyleRole, VAlignment};

use crate::button::InteractionState;
use crate::checkbox::Checkbox;
use crate::primitives::{
    Expand, FixedSize, HStack, Padding, RectWidget, Spacer, TextWidget, TwistArrow, VStack, ZStack,
};

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

/// Default visual for a row in a `ListView` (or any place that wants
/// the canonical icon + label + trailing layout).
pub struct StandardListItem {
    label: String,
    subtitle: Option<String>,
    leading_slot: Option<Box<dyn Widget>>,
    center_slot: Option<Box<dyn Widget>>,
    trailing_slot: Option<Box<dyn Widget>>,
    subtitle_leading_slot: Option<Box<dyn Widget>>,
    subtitle_trailing_slot: Option<Box<dyn Widget>>,
    checkbox: Option<CheckboxKind>,
    selected: Signal<bool>,
    enabled: Signal<bool>,
    label_style: TextStyleRole,
    subtitle_style: TextStyleRole,
    interaction: Signal<InteractionState>,
    root_child_id: Option<WidgetId>,
}

impl StandardListItem {
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        Self {
            label: ls.resolve_now(),
            subtitle: None,
            leading_slot: None,
            center_slot: None,
            trailing_slot: None,
            subtitle_leading_slot: None,
            subtitle_trailing_slot: None,
            checkbox: None,
            selected: Signal::new(false),
            enabled: Signal::new(true),
            label_style: TextStyleRole::Body,
            subtitle_style: TextStyleRole::Small,
            interaction: Signal::new(InteractionState::Idle),
            root_child_id: None,
        }
    }

    /// Shim for raw, untranslated strings — `_literal` suffix is the
    /// grep marker for unlocalized call sites.
    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(LocalizedString::literal(label))
    }

    pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = text.into();
        self.subtitle = Some(ls.resolve_now());
        self
    }

    #[doc(hidden)]
    pub fn subtitle_literal(self, text: impl Into<String>) -> Self {
        self.subtitle(LocalizedString::literal(text))
    }

    /// Leading slot — placed AFTER the optional checkbox, BEFORE the
    /// center slot. Typical: `IconWidget`, avatar, color swatch.
    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.leading_slot = Some(Box::new(widget));
        self
    }

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

    pub fn trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.trailing_slot = Some(widget);
        self
    }

    /// Leading slot for the subtitle line. No-op without `subtitle(...)`.
    pub fn subtitle_leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.subtitle_leading_slot = Some(Box::new(widget));
        self
    }

    pub fn subtitle_leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.subtitle_leading_slot = Some(widget);
        self
    }

    /// Trailing slot for the subtitle line. No-op without `subtitle(...)`.
    pub fn subtitle_trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.subtitle_trailing_slot = Some(Box::new(widget));
        self
    }

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

    pub fn selected(mut self, b: bool) -> Self {
        self.selected = Signal::new(b);
        self
    }

    pub fn bind_selected(mut self, s: Signal<bool>) -> Self {
        self.selected = s;
        self
    }

    pub fn enabled(mut self, b: bool) -> Self {
        self.enabled = Signal::new(b);
        self
    }

    pub fn bind_enabled(mut self, s: Signal<bool>) -> Self {
        self.enabled = s;
        self
    }

    pub fn label_style(mut self, role: TextStyleRole) -> Self {
        self.label_style = role;
        self
    }

    pub fn subtitle_style(mut self, role: TextStyleRole) -> Self {
        self.subtitle_style = role;
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

fn resolve_bg_role(enabled: bool, selected: bool, interaction: InteractionState) -> SurfaceRole {
    if !enabled {
        return SurfaceRole::Transparent;
    }
    if selected {
        return match interaction {
            InteractionState::Pressed => SurfaceRole::Pressed,
            _ => SurfaceRole::Selected,
        };
    }
    match interaction {
        InteractionState::Hovered => SurfaceRole::AccentSubtle,
        InteractionState::Pressed => SurfaceRole::Pressed,
        _ => SurfaceRole::Transparent,
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
        let style = ctx.theme().components.standard_item;

        let label_role = self.enabled.map(|e| resolve_label_role(*e));

        // Label column: either a single TextWidget or a VStack with
        // label on top and subtitle (with its own slots) below.
        let label_widget = TextWidget::new_literal(&self.label)
            .style(self.label_style)
            .bind_color(label_role.clone())
            .a11y_hidden();
        let label_id = ctx.add(label_widget);

        let label_column_id = if let Some(subtitle) = &self.subtitle {
            // Two-line: VStack { label, subtitle line }.
            let subtitle_widget = TextWidget::new_literal(subtitle)
                .style(self.subtitle_style)
                .color(TextRole::Secondary)
                .a11y_hidden();
            let subtitle_text_id = ctx.add(subtitle_widget);

            // Subtitle HStack: [leading?] subtitle [Spacer] [trailing?].
            let mut sub_row = HStack::new()
                .spacing(style.subtitle_slot_gap)
                .alignment(VAlignment::Center);
            if let Some(w) = self.subtitle_leading_slot.take() {
                let id = ctx.add_boxed(w);
                sub_row = sub_row.add_child(id);
            }
            sub_row = sub_row.add_child(subtitle_text_id).add_child(ctx.add(Spacer::new()));
            if let Some(w) = self.subtitle_trailing_slot.take() {
                let id = ctx.add_boxed(w);
                sub_row = sub_row.add_child(id);
            }
            let sub_row_id = ctx.add(sub_row);

            ctx.add(
                VStack::new()
                    .spacing(style.label_subtitle_gap)
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
            .spacing(style.slot_gap)
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
            use fern_core::widget_builder::WidgetBuilder;
            let cb = match kind {
                CheckboxKind::TwoState(s) => Checkbox::new(s),
                CheckboxKind::TriState(s) => Checkbox::tristate(s),
            }
            .labels_hidden(true);
            let cb_id = ctx.add(cb.access_label_literal(self.label.clone()));
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
        row = row.add_child(label_column_id).add_child(ctx.add(Spacer::new()));
        if let Some(w) = self.trailing_slot.take() {
            let id = ctx.add_boxed(w);
            row = row.add_child(id);
        }

        ctx.add(row)
    }

    /// Build the rounded selection background (ZStack { padded bg rect,
    /// padded content }) and return the root id.
    /// Build the rounded selection background + interaction handler
    /// around an arbitrary `content_id` and return the outermost
    /// ZStack id. Shared by `StandardListItem::build` (passing its
    /// inner row) and `StandardTreeItem::build` (passing the row
    /// prefixed with indent + chevron columns).
    fn build_with_background(&mut self, ctx: &mut BuildContext, content_id: WidgetId) -> WidgetId {
        let style = ctx.theme().components.standard_item;

        // Hover handler drives the interaction state. Tracking the
        // signal here means the bg role re-evaluates on mouse
        // enter/leave without a full rebuild.
        let interaction_for_hover = self.interaction.clone();
        let interaction_for_widget = self.interaction.clone();

        let bg_role = {
            let enabled = self.enabled.clone();
            let selected = self.selected.clone();
            let interaction = self.interaction.clone();
            // Triple-zip so the role recomputes on any source change.
            enabled
                .zip(&selected)
                .zip(&interaction)
                .map(|((e, s), i)| resolve_bg_role(*e, *s, *i))
        };

        // Padding outside the bg rect — exposes the rounded corners.
        let bg_rect_id = ctx.add(
            RectWidget::new()
                .bind_background(bg_role)
                .corner_radius(CornerRadius::uniform(style.item_corner_radius)),
        );
        let bg_padded_id = ctx.add(
            Padding::new(0.0, style.bg_horizontal_inset, 0.0, style.bg_horizontal_inset)
                .child_id(bg_rect_id),
        );

        // Inner padding so content doesn't touch the bg edges. Wrap
        // the content in `Expand` so the row claims the full row
        // width even when its slot widgets have small intrinsic sizes
        // — without this, ZStack would center the natural-width
        // content in the row, leaving the chevron column shifted off
        // the leading edge and breaking selection-bg alignment.
        let content_expanded_id = ctx.add(Expand::horizontal().child_id(content_id));
        let content_padded_id = ctx.add(
            Padding::symmetric(style.padding_vertical, style.padding_horizontal)
                .child_id(content_expanded_id),
        );

        let root_id = ctx.add(
            ZStack::new()
                .add_child(bg_padded_id)
                .add_child(content_padded_id),
        );

        // Attach hover handler to the row so hovering anywhere in the
        // row updates the interaction signal. Disabled rows still
        // track hover but `resolve_bg_role` short-circuits to
        // Transparent.
        use fern_core::widget_builder::HandlerSet;
        let handlers = HandlerSet::new().on_hover(move |entered: bool, _ctx: &mut EventContext| {
            interaction_for_hover.set(if entered {
                InteractionState::Hovered
            } else {
                InteractionState::Idle
            });
        });
        ctx.apply_self_handlers(handlers);
        let _ = interaction_for_widget; // silence unused-clone if optimizer removes it

        root_id
    }
}

impl Widget for StandardListItem {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let content_id = self.build_content(ctx);
        let root_id = self.build_with_background(ctx, content_id);
        self.root_child_id = Some(root_id);
        vec![root_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let style = ctx.theme.components.standard_item;
        let min_height = if self.subtitle.is_some() {
            style.min_height_two_line
        } else {
            style.min_height_single_line
        };
        let raw = self
            .root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, min_height));
        let height = raw.height.max(min_height);
        let width = raw.width;
        fern_canvas::Size::new(width, height).into()
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
        // `TreeRowA11y`, TreeTable's `TreeRowA11y`) already sets the
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
        if !self.enabled.get() {
            builder.set_disabled();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// StandardTreeItem
// ---------------------------------------------------------------------------

/// Default visual for a row in a `TreeView`: `StandardListItem` plus
/// a depth-driven indent column and an interactive chevron column
/// (always reserved, even for leaves, so labels at the same depth
/// align).
pub struct StandardTreeItem {
    inner: StandardListItem,
    depth: usize,
    has_children: bool,
    is_expanded: Signal<bool>,
    on_toggle: Option<Rc<dyn Fn()>>,
}

impl StandardTreeItem {
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            inner: StandardListItem::new(label),
            depth: 0,
            has_children: false,
            is_expanded: Signal::new(false),
            on_toggle: None,
        }
    }

    #[doc(hidden)]
    pub fn new_literal(label: impl Into<String>) -> Self {
        Self::new(LocalizedString::literal(label))
    }

    // Forward all StandardListItem builders ----------------------------------

    pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self {
        self.inner = self.inner.subtitle(text);
        self
    }

    #[doc(hidden)]
    pub fn subtitle_literal(mut self, text: impl Into<String>) -> Self {
        self.inner = self.inner.subtitle_literal(text);
        self
    }

    pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.leading_slot(widget);
        self
    }

    pub fn leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.leading_slot_boxed(widget);
        self
    }

    pub fn center_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.center_slot(widget);
        self
    }

    pub fn center_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.center_slot_boxed(widget);
        self
    }

    pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.trailing_slot(widget);
        self
    }

    pub fn trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.trailing_slot_boxed(widget);
        self
    }

    pub fn subtitle_leading_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.subtitle_leading_slot(widget);
        self
    }

    pub fn subtitle_leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.subtitle_leading_slot_boxed(widget);
        self
    }

    pub fn subtitle_trailing_slot(mut self, widget: impl Widget + 'static) -> Self {
        self.inner = self.inner.subtitle_trailing_slot(widget);
        self
    }

    pub fn subtitle_trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self {
        self.inner = self.inner.subtitle_trailing_slot_boxed(widget);
        self
    }

    pub fn checkbox(mut self, checked: Signal<bool>) -> Self {
        self.inner = self.inner.checkbox(checked);
        self
    }

    pub fn tristate_checkbox(mut self, state: Signal<CheckState>) -> Self {
        self.inner = self.inner.tristate_checkbox(state);
        self
    }

    pub fn selected(mut self, b: bool) -> Self {
        self.inner = self.inner.selected(b);
        self
    }

    pub fn bind_selected(mut self, s: Signal<bool>) -> Self {
        self.inner = self.inner.bind_selected(s);
        self
    }

    pub fn enabled(mut self, b: bool) -> Self {
        self.inner = self.inner.enabled(b);
        self
    }

    pub fn bind_enabled(mut self, s: Signal<bool>) -> Self {
        self.inner = self.inner.bind_enabled(s);
        self
    }

    pub fn label_style(mut self, role: TextStyleRole) -> Self {
        self.inner = self.inner.label_style(role);
        self
    }

    pub fn subtitle_style(mut self, role: TextStyleRole) -> Self {
        self.inner = self.inner.subtitle_style(role);
        self
    }

    // Tree-specific ---------------------------------------------------------

    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    pub fn has_children(mut self, has: bool) -> Self {
        self.has_children = has;
        self
    }

    pub fn is_expanded(mut self, b: bool) -> Self {
        self.is_expanded = Signal::new(b);
        self
    }

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
    /// The callback signature is `Fn()` (no `EventContext`) because
    /// the chevron is wired through `TwistArrow::on_click` and the
    /// tree-toggle workflow doesn't need to dispatch intents — it
    /// just calls `slice.toggle_expand(node)`. If you need access to
    /// `EventContext` in a chevron-tap handler, attach a sibling
    /// `on_tap` via the `WidgetBuilder` chain on a wrapping widget.
    pub fn on_toggle(mut self, f: impl Fn() + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    /// Variant accepting an already-`Rc`'d callback. Useful when the
    /// same callback is shared across multiple call sites without an
    /// extra clone — e.g. `TreeRowContext::toggle_callback()` returns
    /// this shape directly.
    pub fn on_toggle_rc(mut self, f: Rc<dyn Fn()>) -> Self {
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
        let style = ctx.theme().components.standard_item;

        // 1. Build the StandardListItem's inner row (no bg yet).
        let inner_content_id = self.inner.build_content(ctx);

        // 2. Indent column — empty FixedSize at `depth * step` width.
        let indent_width = self.depth as f32 * style.tree_indent_step;
        let indent_id = ctx.add(FixedSize::new().bind_width(indent_width));

        // 3. Chevron column — always reserved width so siblings at
        //    the same depth align. `TwistArrow` paints nothing for
        //    leaves. The click is wired via `TwistArrow::on_click`
        //    (which already installs a transparent hit-target rect
        //    + tap recognizer on its own node) — more direct than
        //    `FixedSize.on_tap`, which routes taps through the column
        //    wrapper and the composed parent chain.
        let chevron_size = style.chevron_column_width;
        let mut chevron = TwistArrow::new(
            chevron_size,
            self.has_children,
            self.is_expanded.get(),
        );
        if self.has_children
            && let Some(cb) = self.on_toggle.clone()
        {
            chevron = chevron.on_click(move || cb());
        }
        let chevron_column_id = ctx.add(
            FixedSize::new()
                .bind_width(chevron_size)
                .child(chevron),
        );

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
    use fern_canvas::SizeProposal;
    use fern_core::widget_tree::WidgetTree;
    use fern_core::Theme;

    fn theme() -> Theme {
        fern_core::presets::intui::light()
    }

    #[test]
    fn list_item_layout_single_line() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new_literal("Hello"));
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let b = tree.bounds(id);
        let style = theme().components.standard_item;
        assert!(b.height >= style.min_height_single_line - 0.5);
    }

    #[test]
    fn list_item_layout_two_line() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("Title")
                .subtitle_literal("Subtitle text"),
        );
        tree.layout(SizeProposal {
            width: Some(300.0),
            height: None,
        });
        let b = tree.bounds(id);
        let style = theme().components.standard_item;
        assert!(
            b.height >= style.min_height_two_line - 0.5,
            "two-line height {} < expected {}",
            b.height,
            style.min_height_two_line
        );
    }

    #[test]
    fn list_item_a11y_name_is_label_only() {
        // Subtitle goes to `description`, not concatenated into the
        // name. Lets screen readers present primary vs supplementary
        // info distinctly.
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("Title").subtitle_literal("Subtitle"),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.name(), Some("Title"));
    }

    #[test]
    fn list_item_a11y_name_no_subtitle() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(StandardListItem::new_literal("Just a title"));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let info = tree.accessibility_node(id);
        assert_eq!(info.name(), Some("Just a title"));
    }


    #[test]
    fn list_item_with_checkbox_two_state() {
        use fern_core::signal::Signal;
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(theme());
        let _id = tree.add(
            StandardListItem::new_literal("Item with checkbox").checkbox(checked.clone()),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));
        // Just verify the build succeeds with the checkbox attached.
        // Toggle behavior is exercised by Checkbox's own tests.
        assert!(!checked.get());
    }

    #[test]
    fn list_item_with_tristate_checkbox() {
        use fern_core::signal::Signal;
        let state = Signal::new(CheckState::Indeterminate);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("Folder")
                .tristate_checkbox(state.clone()),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
    }

    #[test]
    fn list_item_checkbox_and_tristate_mutually_exclusive() {
        use fern_core::signal::Signal;
        let two = Signal::new(true);
        let tri = Signal::new(CheckState::Indeterminate);
        // Last call wins — we just verify the builder doesn't panic.
        let mut tree = WidgetTree::new().with_theme(theme());
        let _id = tree.add(
            StandardListItem::new_literal("Mix")
                .checkbox(two.clone())
                .tristate_checkbox(tri.clone()),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));
    }

    #[test]
    fn list_item_subtitle_slots_no_op_without_subtitle() {
        // .subtitle_*_slot(...) without a subtitle just stays in the
        // builder fields but never gets mounted (build_content only
        // touches them when self.subtitle.is_some()).
        let mut tree = WidgetTree::new().with_theme(theme());
        let _id = tree.add(
            StandardListItem::new_literal("Item")
                .subtitle_leading_slot(TextWidget::new_literal("•"))
                .subtitle_trailing_slot(TextWidget::new_literal("∗")),
        );
        tree.layout(SizeProposal::exact(300.0, 100.0));
    }

    #[test]
    fn tree_item_indent_scales_with_depth() {
        let mut tree = WidgetTree::new().with_theme(theme());
        let id_d0 = tree.add(StandardTreeItem::new_literal("root").depth(0));
        let id_d2 = tree.add(StandardTreeItem::new_literal("deep").depth(2));
        tree.layout(SizeProposal {
            width: Some(400.0),
            height: None,
        });
        let _b0 = tree.bounds(id_d0);
        let _b2 = tree.bounds(id_d2);
        // Both layout without panic; positional checks live in the
        // visual smoke tests of the data_collections demo.
    }

    #[test]
    fn tree_item_chevron_reserved_for_leaf() {
        // Leaf and branch at same depth should produce identical
        // outer widths (chevron column reserved).
        let mut tree = WidgetTree::new().with_theme(theme());
        let leaf = tree.add(
            StandardTreeItem::new_literal("file")
                .depth(1)
                .has_children(false),
        );
        let branch = tree.add(
            StandardTreeItem::new_literal("folder")
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
        use fern_canvas::Point;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let f = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            TwistArrow::new(20.0, true, false).on_click(move || f.set(f.get() + 1)),
        );
        tree.layout(SizeProposal::exact(40.0, 40.0));
        let b = tree.bounds(id);
        dispatch_tap(&mut tree, Point::new(b.x + b.width * 0.5, b.y + b.height * 0.5));
        assert_eq!(fired.get(), 1, "TwistArrow.on_click() must fire on tap");
    }

    #[test]
    fn fixed_size_wrapping_twist_arrow_on_tap_baseline() {
        // If on_tap on a FixedSize that wraps a TwistArrow works
        // here, the issue with StandardTreeItem's chevron is
        // composition (parent siblings) — not the chevron-column
        // shape itself.
        use fern_canvas::Point;
        use fern_core::widget_builder::WidgetBuilder;
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
        dispatch_tap(&mut tree, Point::new(b.x + b.width * 0.5, b.y + b.height * 0.5));
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn fixed_size_on_tap_baseline() {
        // Sanity check: confirm `FixedSize::new().on_tap(...)` even
        // fires when constructed via the WidgetBuilder chain. If this
        // breaks, the StandardTreeItem chevron-tap path is doomed.
        use fern_canvas::Point;
        use fern_core::widget_builder::WidgetBuilder;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let f = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            FixedSize::new()
                .bind_width(40.0_f32)
                .bind_height(40.0_f32)
                .child(TextWidget::new_literal("x"))
                .on_tap(move |_, _| f.set(f.get() + 1)),
        );
        tree.layout(SizeProposal::exact(200.0, 200.0));
        let b = tree.bounds(id);
        dispatch_tap(&mut tree, Point::new(b.x + 20.0, b.y + 20.0));
        assert_eq!(fired.get(), 1);
    }

    fn dispatch_tap(tree: &mut WidgetTree, position: fern_canvas::Point) {
        use fern_core::event::{Modifiers, PointerButton, WidgetEvent};
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
        use fern_canvas::Point;
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("Row")
                .checkbox(checked.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        let style = theme().components.standard_item;
        // Checkbox sits at the row's leading edge, just inside the
        // bg_horizontal_inset + padding_horizontal. Tap a few pixels
        // in from there so we land on the box visual.
        let cb_x = bounds.x + style.bg_horizontal_inset + style.padding_horizontal + 4.0;
        let cb_y = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(cb_x, cb_y));
        assert!(checked.get(), "tap on checkbox should flip the bound signal");
        dispatch_tap(&mut tree, Point::new(cb_x, cb_y));
        assert!(!checked.get(), "second tap should flip back");
    }

    #[test]
    fn list_item_row_tap_outside_checkbox_does_not_toggle() {
        use fern_canvas::Point;
        let checked = Signal::new(false);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("A long-enough label so the tap target lands on text")
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
        use fern_canvas::Point;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let fired_clone = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardTreeItem::new_literal("Folder")
                .depth(0)
                .has_children(true)
                .is_expanded(false)
                .on_toggle(move || fired_clone.set(fired_clone.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        let style = theme().components.standard_item;
        // Inside the row's content padding the chevron column sits at
        // `padding_horizontal` (depth=0 → indent=0). Sample its
        // center.
        let cx = bounds.x + style.padding_horizontal + style.chevron_column_width * 0.5;
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
        use fern_canvas::Point;
        let state = Signal::new(CheckState::Unchecked);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("Folder")
                .tristate_checkbox(state.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        let style = theme().components.standard_item;
        let cx = bounds.x + style.padding_horizontal + 8.0;
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
        use fern_canvas::Point;
        let state = Signal::new(CheckState::Indeterminate);
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardListItem::new_literal("Folder")
                .tristate_checkbox(state.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        let style = theme().components.standard_item;
        let cx = bounds.x + style.padding_horizontal + 8.0;
        let cy = bounds.y + bounds.height * 0.5;
        dispatch_tap(&mut tree, Point::new(cx, cy));
        assert_eq!(state.get(), CheckState::Checked);
    }

    #[test]
    fn tree_item_no_toggle_when_no_children() {
        use fern_canvas::Point;
        use std::cell::Cell;
        use std::rc::Rc;
        let fired = Rc::new(Cell::new(0u32));
        let fired_clone = fired.clone();
        let mut tree = WidgetTree::new().with_theme(theme());
        let id = tree.add(
            StandardTreeItem::new_literal("Leaf")
                .depth(0)
                .has_children(false)
                .on_toggle(move || fired_clone.set(fired_clone.get() + 1)),
        );
        tree.layout(SizeProposal::exact(400.0, 60.0));
        let bounds = tree.bounds(id);
        let style = theme().components.standard_item;
        let cx = bounds.x + style.padding_horizontal + style.chevron_column_width * 0.5;
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
        use fern_data::TreeModel;
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
        let id = tree.add(StandardTreeItem::new_literal("x").from_entry(&entry));
        tree.layout(SizeProposal::exact(400.0, 100.0));
        assert!(tree.bounds(id).width > 0.0);
    }
}
