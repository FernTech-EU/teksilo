//! ComboBox — dropdown selection widget.
//!
//! Generic over the item type `T: Clone + PartialEq + 'static`. Selection is
//! value-based: the bound `Signal<Option<T>>` survives reorder and insertion
//! of the backing model. Items come from one of four input paths:
//!
//! - [`ComboBox::new`] — static list of localizable strings (the 90% case).
//! - [`ComboBox::from_items`] — static list of typed values.
//! - [`ComboBox::from_model`] — reactive [`ListModel<T>`].
//! - [`ComboBox::from_source`] — external [`ListDataSource<Item = T>`].
//!
//! The dropdown panel is pre-created during `build()` and kept dormant until
//! opened via click, Enter, Space, or ArrowDown/ArrowUp.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::ObserverHandle;
use fern_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_data::{DataChange, ListDataSource, ListModel};
use fern_tokens::{Color, CornerRadius};

use crate::primitives::{
    HStack, IconWidget, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack,
};

/// Interaction state for the trigger button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComboBoxState {
    Idle,
    Hovered,
    Focused,
    Open,
    Disabled,
}

/// Default maximum number of items shown before the dropdown scrolls.
const DEFAULT_MAX_VISIBLE_ITEMS: usize = 8;

/// Typed accessors shared between the trigger (for keyboard navigation and
/// label resolution) and the dropdown panel (for item rendering).
#[derive(Clone)]
struct ItemSource<T: Clone + 'static> {
    len: Rc<dyn Fn() -> usize>,
    item_at: Rc<dyn Fn(usize) -> Option<T>>,
    observe: Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>,
}

impl<T: Clone + 'static> ItemSource<T> {
    fn from_vec(items: Vec<T>) -> Self {
        Self::from_model(ListModel::from_vec(items))
    }

    fn from_model(model: ListModel<T>) -> Self {
        let m_len = model.clone();
        let m_item = model.clone();
        let m_obs = model.clone();
        Self {
            len: Rc::new(move || m_len.len()),
            item_at: Rc::new(move |i| m_item.with_item(i, |t| t.clone())),
            observe: Rc::new(move |f| m_obs.observe_changes(move |c| f(c))),
        }
    }

    fn from_data_source<S: ListDataSource<Item = T> + 'static>(source: S) -> Self {
        let s = Rc::new(source);
        let s_len = s.clone();
        let s_item = s.clone();
        let s_obs = s.clone();
        Self {
            len: Rc::new(move || s_len.len()),
            item_at: Rc::new(move |i| s_item.with_item(i, |t| t.clone())),
            observe: Rc::new(move |f| s_obs.observe_changes(move |c| f(c))),
        }
    }

    fn len(&self) -> usize {
        (self.len)()
    }

    fn get(&self, index: usize) -> Option<T> {
        (self.item_at)(index)
    }
}

/// A dropdown selection widget.
///
/// ```ignore
/// // Simple: list of strings.
/// let selected = ctx.signal(None::<String>);
/// ComboBox::new(["Apple", "Banana", "Cherry"], selected)
///     .placeholder("Select a fruit...")
///
/// // Typed items: any T: Clone + PartialEq, plus a label extractor.
/// #[derive(Clone, PartialEq)] struct Fruit { name: String, emoji: &'static str }
/// let selected = ctx.signal(None::<Fruit>);
/// ComboBox::from_items(fruits, selected)
///     .item_label(|f: &Fruit| format!("{} {}", f.emoji, f.name))
///
/// // Model-backed: reactive.
/// let model = ListModel::from_vec(fruits);
/// ComboBox::from_model(model, selected)
///     .item_label(|f: &Fruit| f.name.clone())
///     .max_visible_items(6)
/// ```
pub struct ComboBox<T: Clone + PartialEq + 'static> {
    source: ItemSource<T>,
    selected: Signal<Option<T>>,
    item_label: Rc<dyn Fn(&T) -> String>,
    render_item: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    placeholder: String,
    /// Accessible label — independent of placeholder and current selection.
    /// Screen readers announce this as the name of the control.
    label: Option<String>,
    enabled: bool,
    max_visible_items: usize,
    /// Cached index of the currently-selected value in `source`. Validated
    /// on every read; a miss triggers a fresh O(n) scan. Shared across the
    /// keyboard handler and the label-derive closure so both benefit from
    /// the cache across selection changes.
    selected_index_hint: Rc<Cell<Option<usize>>>,
    // Build state
    interaction: Signal<ComboBoxState>,
    root_child_id: Option<WidgetId>,
    dropdown_content_id: Option<WidgetId>,
}

impl ComboBox<String> {
    /// Create a ComboBox from a list of strings.
    ///
    /// Accepts any `impl Into<String>` — string literals (`&str`),
    /// owned `String`s, resolved `LocalizedString`s, etc. For
    /// translated items, resolve translations before passing in,
    /// e.g. `vec![tr!("apple").resolve_now(), ...]`.
    pub fn new(
        items: impl IntoIterator<Item = impl Into<String>>,
        selected: Signal<Option<String>>,
    ) -> Self {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        Self::new_with_item_source(
            ItemSource::from_vec(items),
            selected,
            Rc::new(|s: &String| s.clone()),
        )
    }
}

impl<T: Clone + PartialEq + 'static> ComboBox<T> {
    fn new_with_item_source(
        source: ItemSource<T>,
        selected: Signal<Option<T>>,
        item_label: Rc<dyn Fn(&T) -> String>,
    ) -> Self {
        Self {
            source,
            selected,
            item_label,
            render_item: None,
            placeholder: String::new(),
            label: None,
            enabled: true,
            max_visible_items: DEFAULT_MAX_VISIBLE_ITEMS,
            interaction: Signal::new(ComboBoxState::Idle),
            root_child_id: None,
            dropdown_content_id: None,
            selected_index_hint: Rc::new(Cell::new(None)),
        }
    }

    /// Static list of typed items. `item_label` is the display extractor —
    /// it's required at construction so the compiler enforces it rather
    /// than a runtime check. For `T = String`, use [`ComboBox::new`] which
    /// defaults to the identity label.
    pub fn from_items<F>(
        items: impl IntoIterator<Item = T>,
        selected: Signal<Option<T>>,
        item_label: F,
    ) -> Self
    where
        F: Fn(&T) -> String + 'static,
    {
        Self::new_with_item_source(
            ItemSource::from_vec(items.into_iter().collect()),
            selected,
            Rc::new(item_label),
        )
    }

    /// Backed by a reactive [`ListModel<T>`]. Inserts, removes, and reorders
    /// propagate into the dropdown automatically. If the currently-selected
    /// value disappears from the model, `selected` becomes `None`.
    pub fn from_model<F>(
        model: ListModel<T>,
        selected: Signal<Option<T>>,
        item_label: F,
    ) -> Self
    where
        F: Fn(&T) -> String + 'static,
    {
        Self::new_with_item_source(ItemSource::from_model(model), selected, Rc::new(item_label))
    }

    /// Backed by a custom [`ListDataSource`] — for external or paged data.
    pub fn from_source<S, F>(
        source: S,
        selected: Signal<Option<T>>,
        item_label: F,
    ) -> Self
    where
        S: ListDataSource<Item = T> + 'static,
        F: Fn(&T) -> String + 'static,
    {
        Self::new_with_item_source(
            ItemSource::from_data_source(source),
            selected,
            Rc::new(item_label),
        )
    }

    /// Override the display-label extractor. Rarely needed — prefer passing
    /// `item_label` to the constructor. Useful for the `ComboBox<String>`
    /// path when you want a non-identity projection.
    pub fn item_label(mut self, f: impl Fn(&T) -> String + 'static) -> Self {
        self.item_label = Rc::new(f);
        self
    }

    /// Custom cell rendering. The closure receives the item and a flag
    /// indicating whether it is the currently-selected value.
    ///
    /// The framework wraps the returned widget with the correct
    /// `Role::ListBoxOption` accessibility and tap handler, so callers
    /// do not need to manage a11y or selection dispatch themselves.
    ///
    /// **Reactivity.** The closure is re-run whenever the item's
    /// highlighted state changes (selection flipped on or off for this
    /// row) — the `bool` parameter always reflects the current state at
    /// render time.
    ///
    /// **Accessibility.** The wrapper's `set_name(label)` (from
    /// `item_label`) is what screen readers announce. If the returned
    /// widget includes its own text nodes (e.g. a bare `TextWidget`), the
    /// label may be announced twice — one from the wrapper, one from the
    /// inner text. Wrap primary text nodes in `.a11y_hidden()` to avoid
    /// duplication, and reserve visible widgets for presentation only.
    pub fn render_item(
        mut self,
        f: impl Fn(&T, bool) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.render_item = Some(Rc::new(f));
        self
    }

    /// Maximum number of items shown before the dropdown becomes scrollable.
    /// Defaults to 8. Clamped to at least 1.
    pub fn max_visible_items(mut self, n: usize) -> Self {
        self.max_visible_items = n.max(1);
        self
    }

    /// Placeholder text shown in the trigger when `selected` is `None`.
    /// For translated text, resolve via `tr!("key").resolve_now()` first.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Accessible label describing what this combo box is for
    /// (e.g. "Fruit", "Font family"). Independent of the visible
    /// placeholder and of the current selection — screen readers
    /// announce this as the name of the control.
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl<T: Clone + PartialEq + 'static> std::fmt::Debug for ComboBox<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComboBox")
            .field("items", &self.source.len())
            .field("enabled", &self.enabled)
            .finish()
    }
}

fn resolve_bg(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Idle | ComboBoxState::Focused => colors.surface_main,
        ComboBoxState::Hovered => colors.text_primary.with_alpha(0.04),
        ComboBoxState::Open => colors.text_primary.with_alpha(0.04),
        ComboBoxState::Disabled => colors.accent_disabled,
    }
}

fn resolve_border(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    // Int UI: focus uses the FocusRing wrapper, not a border color change.
    match state {
        ComboBoxState::Disabled => colors.accent_disabled,
        _ => colors.border,
    }
}

fn resolve_text(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Disabled => colors.text_disabled,
        _ => colors.text_primary,
    }
}

/// Find the index of `value` in `source`, or `None` if absent.
fn index_of<T: Clone + PartialEq + 'static>(
    source: &ItemSource<T>,
    value: &T,
) -> Option<usize> {
    let n = source.len();
    for i in 0..n {
        if source.get(i).as_ref() == Some(value) {
            return Some(i);
        }
    }
    None
}

/// Resolve the index of `value` in `source`, consulting `hint` first.
/// If the hint is still valid (`source[hint] == *value`), returns it
/// in O(1). Otherwise falls back to a linear scan and writes the fresh
/// index back into `hint` (or clears it on miss).
fn resolve_index<T: Clone + PartialEq + 'static>(
    source: &ItemSource<T>,
    value: &T,
    hint: &Cell<Option<usize>>,
) -> Option<usize> {
    if let Some(i) = hint.get()
        && source.get(i).as_ref() == Some(value)
    {
        return Some(i);
    }
    let found = index_of(source, value);
    hint.set(found);
    found
}

/// Build the default label-plus-background item widget used when
/// `render_item` is not provided. Matches the pre-generic appearance.
fn default_item_widget(label: &str, theme: &fern_tokens::Theme) -> Box<dyn Widget> {
    let text = TextWidget::new_literal(label)
        .style(theme.typography.body.clone())
        .color(theme.colors.text_primary)
        .single_line()
        .a11y_hidden();
    let menu_style = theme.components.menu;
    let pad_v =
        ((menu_style.item_height - theme.typography.body.size).max(0.0) * 0.5).max(0.0);
    Box::new(Padding::symmetric(pad_v, menu_style.item_padding_horizontal).child(text))
}

/// A single row in the dropdown. Wraps the user-rendered (or default)
/// subtree with the `Role::ListBoxOption` accessibility role, a
/// tap-to-select handler, and a selection-driven highlight background.
struct DropdownItem<T: Clone + PartialEq + 'static> {
    value: T,
    label: String,
    /// 1-based index for `position_in_set`.
    position: usize,
    total: usize,
    selected_signal: Signal<Option<T>>,
    render: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    root_child_id: Option<WidgetId>,
}

impl<T: Clone + PartialEq + 'static> std::fmt::Debug for DropdownItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropdownItem")
            .field("label", &self.label)
            .field("position", &self.position)
            .finish()
    }
}

impl<T: Clone + PartialEq + 'static> Widget for DropdownItem<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use fern_core::binding::BindingLevel;

        let theme = ctx.theme().clone();
        let selected_signal = self.selected_signal.clone();
        let value_for_tap = self.value.clone();

        // Track whether this item is highlighted (hovered or selected).
        let highlighted = ctx.signal(false);
        let is_currently_selected = selected_signal.get().as_ref() == Some(&self.value);
        highlighted.set(is_currently_selected);

        // Sync highlight with the currently-selected value.
        {
            let highlighted = highlighted.clone();
            let value = self.value.clone();
            ctx.effect(&self.selected_signal, move |sel| {
                highlighted.set(sel.as_ref() == Some(&value));
            });
        }

        // Rebuild this row whenever its highlight flips — necessary so
        // custom `render_item` closures that depend on the `selected`
        // bool actually observe the change. Only the two items involved
        // in a selection transition (old and new) rebuild; unaffected
        // items stay put.
        highlighted.bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let bg_color = highlighted.map({
            let primary = theme.colors.accent;
            move |h| {
                if *h {
                    primary.with_alpha(0.12)
                } else {
                    Color::TRANSPARENT
                }
            }
        });

        // Build the inner content — either the user's render_item or the
        // default label row. `render_item` is re-invoked on rebuild so
        // callers see a fresh `selected` bool each time the highlight
        // flips.
        let inner: Box<dyn Widget> = match &self.render {
            Some(r) => (r)(&self.value, is_currently_selected),
            None => default_item_widget(&self.label, &theme),
        };
        let inner_id = ctx.add_boxed(inner);

        let bg = RectWidget::new().bind_background(bg_color);
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(inner_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        let handler_set = HandlerSet::new()
            .on_tap(move |_pos, ctx: &mut EventContext| {
                selected_signal.set(Some(value_for_tap.clone()));
                ctx.dismiss_all_overlays();
            })
            .on_hover({
                let highlighted = highlighted.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    highlighted.set(entered);
                }
            })
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let min_h = ctx.theme.components.menu.item_height;
        match self.root_child_id {
            Some(id) => {
                let s = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(s.width, s.height.max(min_h))
            }
            None => proposal.resolve(120.0, min_h),
        }
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
        builder.set_role(fern_core::accesskit::Role::ListBoxOption);
        builder.set_name(&self.label);
        // A11y gap #1: announce selection state so screen readers can
        // say "selected, Apple" instead of just "Apple".
        let is_selected = self.selected_signal.get().as_ref() == Some(&self.value);
        builder.set_selected(is_selected);
        builder.inner_mut().set_position_in_set(self.position);
        builder.inner_mut().set_size_of_set(self.total);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

/// Dropdown panel content (internal widget — shown as overlay).
struct DropdownPanel<T: Clone + PartialEq + 'static> {
    source: ItemSource<T>,
    selected: Signal<Option<T>>,
    item_label: Rc<dyn Fn(&T) -> String>,
    render_item: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    max_visible_items: usize,
    /// Bumped on every model mutation so the panel rebuilds.
    version: Signal<u64>,
    root_child_id: Option<WidgetId>,
}

impl<T: Clone + PartialEq + 'static> std::fmt::Debug for DropdownPanel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropdownPanel")
            .field("item_count", &self.source.len())
            .finish()
    }
}

impl<T: Clone + PartialEq + 'static> Widget for DropdownPanel<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();

        // Rebuild on model changes.
        use fern_core::binding::BindingLevel;
        self.version
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let total = self.source.len();
        let mut vstack = VStack::new();
        for i in 0..total {
            if let Some(value) = self.source.get(i) {
                let label = (self.item_label)(&value);
                let item = DropdownItem {
                    value,
                    label,
                    position: i + 1,
                    total,
                    selected_signal: self.selected.clone(),
                    render: self.render_item.clone(),
                    root_child_id: None,
                };
                vstack = vstack.child(item);
            }
        }

        let menu_style = theme.components.menu;
        let padded = Padding::uniform(4.0).child(vstack);

        // Cap the panel height at max_visible_items * item_height so
        // overflowing item counts scroll rather than run off-screen.
        // +8 accounts for the 4px outer padding on both sides.
        let max_height = self.max_visible_items as f32 * menu_style.item_height + 8.0;
        let scrollable = crate::scroll_area::ScrollArea::new().child(padded);
        let clamped = ctx.add(crate::primitives::MaxSize::height(max_height).child(scrollable));

        // Dropdown panel — same surface treatment as MenuList (raised + popup radius)
        let bg = RectWidget::new()
            .background(theme.colors.surface_raised)
            .border_color(theme.colors.border)
            .border_width(menu_style.popup_border_width)
            .corner_radius(CornerRadius::uniform(menu_style.popup_corner_radius));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(clamped);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(120.0, 0.0)),
            None => proposal.resolve(120.0, 0.0),
        }
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
        builder.set_role(fern_core::accesskit::Role::ListBox);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

impl<T: Clone + PartialEq + 'static> Widget for ComboBox<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let enabled = self.enabled;

        let interaction = ctx.signal(if enabled {
            ComboBoxState::Idle
        } else {
            ComboBoxState::Disabled
        });
        self.interaction = interaction.clone();

        // Observe model changes so the dropdown panel rebuilds when the
        // backing data mutates, and so selection is cleared when the
        // currently-selected value disappears from the model.
        //
        // Trigger-level rebuild is NOT required: the trigger's label binds
        // via `self.selected.map(...)`, which re-fires whenever `selected`
        // itself changes. The observer already clears `selected` when the
        // value vanishes, so the derived label updates automatically.
        let panel_version = ctx.signal(0_u64);
        let pv = panel_version.clone();
        let observe_handle = (self.source.observe)(Box::new({
            let source = self.source.clone();
            let selected = self.selected.clone();
            let hint = self.selected_index_hint.clone();
            move |_change: &DataChange| {
                // If the currently-selected value is no longer present
                // in the model, clear selection. Works for Reset,
                // ItemsRemoved, and ItemUpdated. The hint is also
                // invalidated unconditionally: any mutation may have
                // shifted the index of the selected value.
                hint.set(None);
                if let Some(cur) = selected.get()
                    && resolve_index(&source, &cur, &hint).is_none()
                {
                    selected.set(None);
                }
                pv.set(pv.get().wrapping_add(1));
            }
        }));
        ctx.own_handle(observe_handle);

        // Derive label text from selected signal + source.
        let source_for_label = self.source.clone();
        let item_label_for_trigger = self.item_label.clone();
        let placeholder = self.placeholder.clone();
        let hint_for_label = self.selected_index_hint.clone();
        let label_text = self.selected.map(move |sel| match sel {
            Some(v) => match resolve_index(&source_for_label, v, &hint_for_label) {
                Some(_) => (item_label_for_trigger)(v),
                None => placeholder.clone(),
            },
            None => placeholder.clone(),
        });

        let bg_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_bg(*s, &colors))
        };
        let border_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_border(*s, &colors))
        };
        let text_color = {
            let colors = theme.colors.clone();
            interaction.map(move |s| resolve_text(*s, &colors))
        };

        // Build trigger: [label | Spacer | chevron]
        let label = TextWidget::new_literal("")
            .style(theme.typography.body.clone())
            .bind_text(label_text)
            .bind_color(text_color)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);

        let chevron =
            IconWidget::chevron_down(12.0).color(theme.colors.text_primary.with_alpha(0.5));
        let chevron_id = ctx.add(chevron);

        let row = HStack::new()
            .spacing(8.0)
            .add_child(label_id)
            .child(Spacer::new())
            .add_child(chevron_id);
        let row_id = ctx.add(row);

        let combo_style = theme.components.combo_box;
        let padding = Padding::symmetric(
            combo_style.padding_horizontal * 0.5,
            combo_style.padding_horizontal,
        )
        .child_id(row_id);
        let padding_id = ctx.add(padding);

        let bg = RectWidget::new()
            .bind_background(bg_color)
            .bind_border_color(border_color)
            .border_width(theme.shape.border_width)
            .corner_radius(CornerRadius::uniform(combo_style.corner_radius));
        let bg_id = ctx.add(bg);

        let visual_zstack = ZStack::new().add_child(bg_id).add_child(padding_id);
        let visual_id = ctx.add(visual_zstack);
        let sized_id = ctx.add(
            crate::primitives::MinSize::new(0.0, combo_style.height).child_id(visual_id),
        );

        // Wrap in a FocusRing — drawn outside the control on keyboard focus.
        let focused = interaction.map(|s| *s == ComboBoxState::Focused);
        let root_id = ctx.add(
            crate::primitives::FocusRing::new(focused)
                .corner_radius(combo_style.corner_radius)
                .child_id(sized_id),
        );
        self.root_child_id = Some(root_id);

        // Pre-create the dropdown panel (dormant until opened)
        let dropdown_panel = DropdownPanel {
            source: self.source.clone(),
            selected: self.selected.clone(),
            item_label: self.item_label.clone(),
            render_item: self.render_item.clone(),
            max_visible_items: self.max_visible_items,
            version: panel_version,
            root_child_id: None,
        };
        let dropdown_id = ctx.add(dropdown_panel);
        self.dropdown_content_id = Some(dropdown_id);
        ctx.set_dormant(dropdown_id);

        // --- Handlers ---
        let self_id = ctx.self_id();
        let int_hover = interaction.clone();
        let int_focus = interaction.clone();

        // Shared dismiss callback — invoked by the overlay manager
        // whenever the dropdown is dismissed, regardless of path
        // (our own Enter/Escape handlers, framework-level
        // EscapeOrClickOutside, pointer-leave, cascade). Resets
        // `interaction` back to `Focused` so `set_expanded`
        // reported by `accessibility()` stays truthful.
        let dismiss_callback: OverlayDismissCallback = {
            let interaction = interaction.clone();
            Rc::new(move || {
                if interaction.get() == ComboBoxState::Open {
                    interaction.set(ComboBoxState::Focused);
                }
            })
        };

        // Helper to open the overlay — used by tap and several key handlers.
        let open_overlay = {
            let interaction = interaction.clone();
            let dismiss_callback = dismiss_callback.clone();
            Rc::new(move |ctx: &mut EventContext| {
                interaction.set(ComboBoxState::Open);
                ctx.activate(dropdown_id);
                ctx.show_overlay(OverlayRequest {
                    content_id: dropdown_id,
                    anchor: self_id,
                    placement: OverlayPlacement::BelowPreferred,
                    dismiss: DismissBehavior::EscapeOrClickOutside,
                    layer: OverlayLayer::InTree,
                    parent_overlay: None,
                    on_dismiss: Some(dismiss_callback.clone()),
                });
            })
        };

        let handler_set = HandlerSet::new()
            .on_tap({
                let open_overlay = open_overlay.clone();
                move |_pos, ctx: &mut EventContext| {
                    if !enabled {
                        return;
                    }
                    open_overlay(ctx);
                }
            })
            .on_hover(move |entered: bool, _ctx: &mut EventContext| {
                if !enabled {
                    return;
                }
                let current = int_hover.get();
                if current == ComboBoxState::Open {
                    return;
                }
                if entered {
                    int_hover.set(ComboBoxState::Hovered);
                } else {
                    int_hover.set(ComboBoxState::Idle);
                }
            })
            .on_key({
                let interaction = interaction.clone();
                let selected = self.selected.clone();
                let source = self.source.clone();
                let item_label_for_keys = self.item_label.clone();
                let hint = self.selected_index_hint.clone();
                let open_overlay = open_overlay.clone();
                // Type-ahead buffer: (prefix, last_keystroke_time)
                let typeahead: Rc<RefCell<(String, Instant)>> =
                    Rc::new(RefCell::new((String::new(), Instant::now())));
                // Helper: set selection to the item at `index` and update
                // the cached hint in one shot.
                let pick_at = {
                    let source = source.clone();
                    let selected = selected.clone();
                    let hint = hint.clone();
                    Rc::new(move |index: usize| {
                        if let Some(v) = source.get(index) {
                            hint.set(Some(index));
                            selected.set(Some(v));
                        }
                    })
                };
                move |event: &WidgetEvent, ctx: &mut EventContext| -> EventResponse {
                    if !enabled {
                        return EventResponse::Ignored;
                    }
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            if interaction.get() == ComboBoxState::Open {
                                interaction.set(ComboBoxState::Focused);
                                ctx.dismiss_all_overlays();
                            } else {
                                open_overlay(ctx);
                            }
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            if interaction.get() == ComboBoxState::Open {
                                interaction.set(ComboBoxState::Focused);
                                ctx.dismiss_all_overlays();
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            if interaction.get() != ComboBoxState::Open {
                                open_overlay(ctx);
                            }
                            let n = source.len();
                            if n == 0 {
                                return EventResponse::Handled;
                            }
                            // Treat "no selection" as an implicit cursor at
                            // index 0 — ArrowDown advances to index 1 from
                            // nothing (matching the framework convention
                            // across widgets that keyboard-navigate lists).
                            let current_idx = selected
                                .get()
                                .as_ref()
                                .and_then(|v| resolve_index(&source, v, &hint))
                                .unwrap_or(0);
                            let target = (current_idx + 1) % n;
                            pick_at(target);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowUp, ..
                        } => {
                            if interaction.get() != ComboBoxState::Open {
                                open_overlay(ctx);
                            }
                            let n = source.len();
                            if n == 0 {
                                return EventResponse::Handled;
                            }
                            let current_idx = selected
                                .get()
                                .as_ref()
                                .and_then(|v| resolve_index(&source, v, &hint))
                                .unwrap_or(0);
                            let target = if current_idx == 0 {
                                n - 1
                            } else {
                                current_idx - 1
                            };
                            pick_at(target);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Home, ..
                        } => {
                            if source.len() == 0 {
                                return EventResponse::Handled;
                            }
                            pick_at(0);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::End, ..
                        } => {
                            let n = source.len();
                            if n == 0 {
                                return EventResponse::Handled;
                            }
                            pick_at(n - 1);
                            EventResponse::Handled
                        }
                        // Type-ahead: letter/character keys jump to matching item.
                        WidgetEvent::KeyDown { key, .. } if key.to_char().is_some() => {
                            let ch = key.to_char().unwrap();
                            let mut ta = typeahead.borrow_mut();
                            let now = Instant::now();
                            // Reset buffer if more than 500ms since last keystroke
                            if now.duration_since(ta.1).as_millis() > 500 {
                                ta.0.clear();
                            }
                            ta.0.push(ch.to_ascii_lowercase());
                            ta.1 = now;
                            let prefix = ta.0.clone();
                            drop(ta);

                            // Find first item whose label starts with the prefix
                            // (case-insensitive).
                            let n = source.len();
                            for i in 0..n {
                                if let Some(v) = source.get(i) {
                                    let label = (item_label_for_keys)(&v);
                                    if label.to_lowercase().starts_with(&prefix) {
                                        pick_at(i);
                                        break;
                                    }
                                }
                            }
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                }
            })
            .on_focus(move |gained: bool, _ctx: &mut EventContext| {
                if gained {
                    let current = int_focus.get();
                    if current == ComboBoxState::Idle {
                        int_focus.set(ComboBoxState::Focused);
                    }
                } else {
                    int_focus.set(ComboBoxState::Idle);
                }
            })
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        vec![root_id]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => {
                let child_size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(child_size.width.max(120.0), child_size.height.max(36.0))
            }
            None => proposal.resolve(120.0, 36.0),
        }
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
        builder.set_role(fern_core::accesskit::Role::ComboBox);
        builder.set_has_popup(fern_core::accesskit::HasPopup::Listbox);

        if let Some(name) = self.label.as_deref() {
            builder.set_name(name);
        }

        // A11y gap #3: use `placeholder` when nothing is selected, `value`
        // when something is. The two are distinct ARIA properties; screen
        // readers announce placeholders as hints rather than current values.
        match self.selected.get() {
            Some(v) => {
                let label = (self.item_label)(&v);
                if !label.is_empty() {
                    builder.set_value(label);
                }
            }
            None => {
                if !self.placeholder.is_empty() {
                    builder.set_placeholder(self.placeholder.clone());
                }
            }
        }

        builder.set_expanded(self.interaction.get() == ComboBoxState::Open);

        // A11y gap #2: trigger points at the popup listbox via aria-controls
        // so AT can jump from the combobox into its options.
        if let Some(popup_id) = self.dropdown_content_id {
            builder.push_controlled(widget_id_to_node_id(popup_id));
        }

        if !self.enabled {
            builder.set_disabled();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_data::ListModel;
    use fern_tokens::Theme;

    // ─── Helpers ──────────────────────────────────────────────────────

    fn light_tree() -> WidgetTree {
        WidgetTree::new().with_theme(Theme::light_default())
    }

    fn fruits() -> Vec<&'static str> {
        vec!["Apple", "Banana", "Cherry"]
    }

    // ─── Basic layout & role ──────────────────────────────────────────

    #[test]
    fn combo_box_builds_and_lays_out() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(
            ComboBox::new(fruits(), selected.clone()).placeholder("Select..."),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));
        let bounds = tree.bounds(cb);
        assert!(bounds.width >= 120.0);
        assert!(bounds.height >= 36.0);
    }

    #[test]
    fn combo_box_accessibility_role() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(vec!["A", "B"], selected.clone()));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(cb);
        assert_eq!(info.role(), fern_core::accesskit::Role::ComboBox);
        assert!(!info.is_expanded());
    }

    #[test]
    fn accessibility_exposes_label_via_set_name() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(
            ComboBox::new(vec!["Apple", "Banana"], selected.clone()).label("Fruit"),
        );
        tree.layout(SizeProposal::exact(200.0, 50.0));
        let info = tree.accessibility_node(cb);
        assert_eq!(info.name(), Some("Fruit"));
    }

    #[test]
    fn accessibility_expanded_flips_on_open_close() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        assert!(!tree.accessibility_node(cb).is_expanded());

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert!(tree.accessibility_node(cb).is_expanded());

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert!(!tree.accessibility_node(cb).is_expanded());
    }

    #[test]
    fn accessibility_expanded_resets_on_framework_dismiss() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert!(tree.accessibility_node(cb).is_expanded());

        let overlay_id = tree
            .active_overlays()
            .first()
            .copied()
            .expect("dropdown overlay should be active");
        tree.dismiss_overlay(overlay_id);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert!(
            !tree.accessibility_node(cb).is_expanded(),
            "framework overlay dismiss must reset is_expanded() to false"
        );
    }

    // ─── Keyboard ─────────────────────────────────────────────────────

    #[test]
    fn arrow_keys_cycle_selection() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Banana"));

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Cherry"));

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Apple"));
    }

    #[test]
    fn selected_updates_label() {
        let mut tree = light_tree();
        let selected = Signal::new(Some("Banana".to_string()));
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(cb).width > 0.0);
    }

    #[test]
    fn click_opens_overlay() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert!(tree.active_overlays().is_empty());

        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert_eq!(tree.active_overlays().len(), 1);
    }

    #[test]
    fn type_ahead_jumps_to_matching_item() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry", "Blueberry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        tree.press_key(Key::B, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Banana"));

        tree.press_key(Key::L, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Blueberry"));
    }

    #[test]
    fn type_ahead_with_character_key() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(
            vec!["100px", "200px", "300px"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        tree.press_key(Key::Character('2'), fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("200px"));
    }

    #[test]
    fn type_ahead_case_insensitive() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        tree.press_key(Key::C, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Cherry"));
    }

    #[test]
    fn type_ahead_no_match_keeps_selection() {
        let mut tree = light_tree();
        let selected = Signal::new(Some("Banana".to_string()));
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 50.0));
        tree.focus(cb);

        tree.press_key(Key::Z, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Banana"));
    }

    #[test]
    fn enter_toggles_dropdown_open_close() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Banana"));

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());
        assert_eq!(selected.get().as_deref(), Some("Banana"));
    }

    #[test]
    fn escape_closes_dropdown() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        tree.press_key(Key::Enter, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Escape, fern_core::event::Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn arrow_down_opens_dropdown_when_closed() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        tree.press_key(Key::ArrowDown, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(300.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1);
        assert_eq!(selected.get().as_deref(), Some("Banana"));
    }

    #[test]
    fn type_ahead_highlights_in_open_dropdown() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 300.0));
        tree.focus(cb);

        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 300.0));
        assert_eq!(tree.active_overlays().len(), 1);
        let frame_before = tree.render();

        tree.press_key(Key::B, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Banana"));

        tree.layout(SizeProposal::exact(300.0, 300.0));
        let frame_after = tree.render();

        assert_ne!(frame_before.shapes, frame_after.shapes);
    }

    #[test]
    fn below_preferred_opens_above_when_no_space() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 60.0));

        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 60.0));

        assert_eq!(tree.active_overlays().len(), 1);

        let content_ids = tree.overlay_manager().active_content_ids();
        let overlay_bounds = tree.bounds(content_ids[0]);
        let cb_bounds = tree.bounds(cb);

        assert!(
            overlay_bounds.y + overlay_bounds.height <= cb_bounds.y + 5.0,
            "overlay should be positioned above when no space below"
        );
    }

    // ─── Model-backed & typed selection (new) ─────────────────────────

    #[derive(Clone, Debug, PartialEq)]
    struct Fruit {
        name: &'static str,
        emoji: &'static str,
    }

    fn fruit_list() -> Vec<Fruit> {
        vec![
            Fruit { name: "Apple", emoji: "🍎" },
            Fruit { name: "Banana", emoji: "🍌" },
            Fruit { name: "Cherry", emoji: "🍒" },
        ]
    }

    #[test]
    fn typed_combo_box_renders_with_item_label() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<Fruit>);
        let cb = tree.add(
            ComboBox::from_items(fruit_list(), selected.clone(), |f: &Fruit| {
                f.name.to_string()
            })
            .placeholder("Pick a fruit"),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));
        assert!(tree.bounds(cb).width >= 120.0);
    }

    #[test]
    fn model_backed_combo_reflects_insertions_via_clicks() {
        // Strong form of the insertion test: insert a new item, then
        // click it in the open dropdown and verify `selected` reflects
        // the click. Exercises the full observer → panel rebuild →
        // click path, not just the signal plumbing.
        let mut tree = light_tree();
        let model = ListModel::from_vec(vec!["Apple".to_string(), "Cherry".to_string()]);
        let selected = Signal::new(None::<String>);
        let cb = tree.add(
            ComboBox::from_model(model.clone(), selected.clone(), |s: &String| s.clone()),
        );
        tree.layout(SizeProposal::exact(300.0, 400.0));

        // Insert Banana between Apple and Cherry — model is now [Apple, Banana, Cherry].
        model.insert(1, "Banana".to_string());
        tree.layout(SizeProposal::exact(300.0, 400.0));

        // Open the dropdown.
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);

        // Find the newly-inserted "Banana" row via its accessibility name
        // and click it. If the panel did not rebuild on insertion, this
        // lookup would fail.
        let banana = tree
            .find_by_label("Banana")
            .expect("Banana row should be present in the dropdown");
        tree.click(banana);
        tree.layout(SizeProposal::exact(300.0, 400.0));

        assert_eq!(selected.get().as_deref(), Some("Banana"));
        // Clicking an item dismisses the overlay.
        assert!(tree.active_overlays().is_empty());
    }

    #[test]
    fn model_backed_combo_resets_selection_on_remove() {
        let mut tree = light_tree();
        let model = ListModel::from_vec(vec![
            "Apple".to_string(),
            "Banana".to_string(),
            "Cherry".to_string(),
        ]);
        let selected = Signal::new(Some("Banana".to_string()));
        tree.add(
            ComboBox::from_model(model.clone(), selected.clone(), |s: &String| s.clone()),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Remove the selected item.
        model.remove(1);
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Selection should have been cleared by the observe hook.
        assert_eq!(selected.get(), None);
    }

    #[test]
    fn typed_selection_survives_reorder() {
        let mut tree = light_tree();
        let model = ListModel::from_vec(fruit_list());
        let selected = Signal::new(Some(Fruit {
            name: "Banana",
            emoji: "🍌",
        }));
        tree.add(
            ComboBox::from_model(model.clone(), selected.clone(), |f: &Fruit| {
                f.name.to_string()
            }),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Move Banana from index 1 to index 0.
        model.move_item(1, 0);
        tree.layout(SizeProposal::exact(300.0, 200.0));

        // Selection unchanged — same T, regardless of index.
        assert_eq!(selected.get().map(|f| f.name), Some("Banana"));
    }

    #[test]
    fn home_end_keys_jump_to_first_and_last() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry", "Date"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.focus(cb);

        tree.press_key(Key::End, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Date"));

        tree.press_key(Key::Home, fern_core::event::Modifiers::NONE);
        assert_eq!(selected.get().as_deref(), Some("Apple"));
    }

    #[test]
    fn render_item_reruns_on_selection_change() {
        // Regression guard: the `bool selected` argument to `render_item`
        // must reflect the current highlight state at every render, not a
        // snapshot from the first build. Flipping selection across two
        // items must cause the render closure to observe `selected = true`
        // for each of them in turn.
        use std::sync::Mutex;
        let observed: Rc<Mutex<Vec<(String, bool)>>> = Rc::new(Mutex::new(Vec::new()));

        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let items = vec!["Apple".to_string(), "Banana".to_string()];
        let obs = observed.clone();
        let cb = tree.add(
            ComboBox::from_items(items, selected.clone(), |s: &String| s.clone())
                .render_item(move |item, is_selected| {
                    obs.lock().unwrap().push((item.clone(), is_selected));
                    Box::new(crate::primitives::MinSize::new(10.0, 10.0))
                }),
        );
        tree.layout(SizeProposal::exact(300.0, 300.0));
        // Open so items build for the first time.
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 300.0));

        // Select Apple; the Apple row must rebuild with is_selected=true.
        selected.set(Some("Apple".to_string()));
        tree.layout(SizeProposal::exact(300.0, 300.0));
        // Now select Banana; Apple rebuilds with false, Banana with true.
        selected.set(Some("Banana".to_string()));
        tree.layout(SizeProposal::exact(300.0, 300.0));

        let calls = observed.lock().unwrap().clone();
        assert!(
            calls.contains(&("Apple".to_string(), true)),
            "render_item should have been called for Apple with selected=true; got {:?}",
            calls
        );
        assert!(
            calls.contains(&("Banana".to_string(), true)),
            "render_item should have been called for Banana with selected=true; got {:?}",
            calls
        );
    }

    #[test]
    fn custom_render_item_used() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        CALLS.store(0, Ordering::SeqCst);

        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(
            ComboBox::new(fruits(), selected.clone()).render_item(|_item, _selected| {
                CALLS.fetch_add(1, Ordering::SeqCst);
                // A distinctive leaf — a small fixed rect is enough to show
                // that our closure was called instead of the default row.
                Box::new(crate::primitives::MinSize::new(10.0, 10.0))
            }),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 200.0));

        assert!(
            CALLS.load(Ordering::SeqCst) >= 3,
            "render_item should have been called at least once per item"
        );
    }

    // ─── Accessibility gap fixes ──────────────────────────────────────

    /// Invoke `Widget::accessibility` on the widget at `id` and return the
    /// resulting raw `accesskit::Node` for inspection of properties not
    /// surfaced by `AccessibilityInfo` (placeholder, controls, auto_complete).
    fn build_raw_a11y_node(
        tree: &mut WidgetTree,
        id: WidgetId,
    ) -> fern_core::accesskit::Node {
        use fern_core::accessibility::widget_id_to_node_id;
        let update = tree.sync_accessibility();
        let target = widget_id_to_node_id(id);
        update
            .nodes
            .into_iter()
            .find(|(node_id, _)| *node_id == target)
            .map(|(_, n)| n)
            .expect("accessibility node should be present for widget")
    }

    #[test]
    fn accessibility_trigger_controls_popup() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let node = build_raw_a11y_node(&mut tree, cb);
        assert!(
            !node.controls().is_empty(),
            "combo box trigger should point at its listbox via aria-controls"
        );
    }

    #[test]
    fn accessibility_placeholder_when_no_selection() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(
            ComboBox::new(fruits(), selected.clone()).placeholder("Select a fruit"),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let node = build_raw_a11y_node(&mut tree, cb);
        assert_eq!(node.placeholder(), Some("Select a fruit"));
        assert_eq!(node.value(), None);
    }

    #[test]
    fn accessibility_value_when_selection_present() {
        let mut tree = light_tree();
        let selected = Signal::new(Some("Banana".to_string()));
        let cb = tree.add(
            ComboBox::new(fruits(), selected.clone()).placeholder("Select a fruit"),
        );
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let node = build_raw_a11y_node(&mut tree, cb);
        assert_eq!(node.value(), Some("Banana"));
        assert_eq!(node.placeholder(), None);
    }
}
