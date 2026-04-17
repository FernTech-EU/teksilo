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
    /// When `true`, the dropdown panel includes a search field at the top
    /// and the list is filtered live against the query. Only exposed under
    /// the `rich-text` feature because it relies on `TextInput`.
    #[cfg(feature = "rich-text")]
    searchable: bool,
    /// Custom match predicate used in searchable mode. If unset, the
    /// default is a case-insensitive substring match on the label.
    #[cfg(feature = "rich-text")]
    filter: Option<Rc<dyn Fn(&str, &T) -> bool>>,
    /// Search query signal, created lazily on the first build when
    /// `searchable` is enabled. Shared with the `DropdownPanel` so both
    /// the trigger-side a11y state and the panel's filter see the same
    /// value.
    #[cfg(feature = "rich-text")]
    search_query: Option<Signal<String>>,
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
            #[cfg(feature = "rich-text")]
            searchable: false,
            #[cfg(feature = "rich-text")]
            filter: None,
            #[cfg(feature = "rich-text")]
            search_query: None,
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
    /// **Reactivity.** The `bool` argument is a snapshot at build time.
    /// If the selection flips after the dropdown is open, the user's
    /// subtree is not automatically re-rendered; the framework-managed
    /// highlight background (behind the custom widget) does update, and
    /// closing and re-opening the dropdown picks up the new state. If
    /// you need a reactive appearance that tracks selection, close over
    /// a `Signal<Option<T>>` in your closure and compare against the
    /// item value inside a `.map()` / `bind_*` on primitives.
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

/// Searchable-mode builders. Gated behind the `rich-text` feature
/// because the search field is a `TextInput`, which shares the
/// `RichTextEditor` engine and therefore the `fern-text` dependency.
#[cfg(feature = "rich-text")]
impl<T: Clone + PartialEq + 'static> ComboBox<T> {
    /// Show a search field at the top of the dropdown panel and filter
    /// the list live against the user's query. When `true`, items are
    /// matched by the closure passed to [`filter`](Self::filter), or —
    /// if no filter is set — by a case-insensitive substring match on
    /// the [`item_label`](Self::item_label).
    ///
    /// The search input becomes a child of the dropdown panel only,
    /// not of the trigger: the closed combo box looks identical
    /// whether searchable or not.
    ///
    /// The query signal is created internally. Use
    /// [`search_query`](Self::search_query) to supply your own if you
    /// want to observe or drive the query externally.
    pub fn searchable(mut self, enabled: bool) -> Self {
        self.searchable = enabled;
        if !enabled {
            self.search_query = None;
        }
        self
    }

    /// Bind the search field to an external `Signal<String>`. Implies
    /// [`searchable(true)`](Self::searchable). Useful for observing or
    /// programmatically setting the query from outside the widget
    /// (e.g. a "Clear" button, persistence across sessions).
    pub fn search_query(mut self, query: Signal<String>) -> Self {
        self.search_query = Some(query);
        self.searchable = true;
        self
    }

    /// Custom match predicate for searchable mode. Called on every
    /// visible-item pass with the current query string (as typed, not
    /// normalized) and a reference to the item; return `true` to keep
    /// the item in the filtered list. Only consulted when
    /// [`searchable`](Self::searchable) is `true`. Ignored otherwise.
    pub fn filter(mut self, f: impl Fn(&str, &T) -> bool + 'static) -> Self {
        self.filter = Some(Rc::new(f));
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

/// Add the default label-plus-padding subtree into the arena and return
/// its root id. Used when `render_item` is not provided.
///
/// The label is wrapped in an `HStack` with a trailing `Spacer` so the
/// inner content stretches to the full item width — without that
/// stretch, the `ZStack` in `DropdownItem` (which defaults to
/// `Alignment::CENTER`) would center the narrow text inside the wide
/// row, producing visibly centered labels instead of left-aligned
/// ones. This mirrors the pattern used by `MenuItem`'s row.
fn build_default_item(
    ctx: &mut BuildContext,
    label: &str,
    theme: &fern_tokens::Theme,
) -> WidgetId {
    let text = TextWidget::new_literal(label)
        .style(theme.typography.body.clone())
        .color(theme.colors.text_primary)
        .single_line()
        .a11y_hidden();
    let text_id = ctx.add(text);

    // HStack { label | Spacer } fills the available width, which forces
    // the enclosing `Padding` to stretch to its full proposal rather
    // than shrinking to the label's intrinsic width.
    let row = HStack::new()
        .spacing(0.0)
        .add_child(text_id)
        .child(Spacer::new());
    let row_id = ctx.add(row);

    let menu_style = theme.components.menu;
    let pad_v =
        ((menu_style.item_height - theme.typography.body.size).max(0.0) * 0.5).max(0.0);
    let padding =
        Padding::symmetric(pad_v, menu_style.item_padding_horizontal).child_id(row_id);
    ctx.add(padding)
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
        let theme = ctx.theme().clone();
        let selected_signal = self.selected_signal.clone();
        let value_for_tap = self.value.clone();

        // Track whether this item is highlighted (hovered or selected).
        let highlighted = ctx.signal(false);

        // Sync highlight with the currently-selected value.
        {
            let highlighted = highlighted.clone();
            let value = self.value.clone();
            ctx.effect(&self.selected_signal, move |sel| {
                highlighted.set(sel.as_ref() == Some(&value));
            });
        }

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
        // default label row. The default path adds widgets directly via
        // `ctx.add` so every child is in the arena at layout time.
        let is_currently_selected = self.selected_signal.get().as_ref() == Some(&self.value);
        let inner_id = match &self.render {
            Some(r) => {
                let widget = (r)(&self.value, is_currently_selected);
                ctx.add_boxed(widget)
            }
            None => build_default_item(ctx, &self.label, &theme),
        };

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
        // Forward the width proposal so each row stretches the full panel
        // width instead of collapsing to its text's intrinsic width —
        // ZStack::size_that_fits queries children with `unspecified`,
        // stripping the proposed width, so we can't just delegate to the
        // root ZStack. Same pattern as `menu_list::KeyboardHighlightWrapper`.
        let child_size = self
            .root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, min_h));
        let width = proposal.width.unwrap_or(child_size.width.max(120.0));
        let height = child_size.height.max(min_h);
        Size::new(width, height)
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

/// Build the static (unfiltered) item list subtree: a padded `VStack`
/// of `DropdownItem`s, optionally wrapped in a `ScrollArea` + `MaxSize`
/// when the item count exceeds the visibility cap. Returns the root id
/// for insertion into the panel's `ZStack`. Shared by the
/// non-searchable path of `DropdownPanel` and — indirectly via
/// `FilteredItemList` — the searchable path.
fn build_static_item_list<T: Clone + PartialEq + 'static>(
    ctx: &mut BuildContext,
    source: &ItemSource<T>,
    selected: &Signal<Option<T>>,
    item_label: &Rc<dyn Fn(&T) -> String>,
    render_item: &Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    max_visible_items: usize,
    menu_style: &fern_tokens::MenuStyle,
) -> WidgetId {
    let total = source.len();
    let mut vstack = VStack::new();
    for i in 0..total {
        if let Some(value) = source.get(i) {
            let label = (item_label)(&value);
            vstack = vstack.child(DropdownItem {
                value,
                label,
                position: i + 1,
                total,
                selected_signal: selected.clone(),
                render: render_item.clone(),
                root_child_id: None,
            });
        }
    }
    let vstack_id = ctx.add(vstack);
    let padded_id = ctx.add(Padding::uniform(4.0).child_id(vstack_id));
    if total > max_visible_items {
        let max_height = max_visible_items as f32 * menu_style.item_height + 8.0;
        let scrollable_id = ctx.add(
            crate::scroll_area::ScrollArea::from_id(padded_id)
                .preferred_size(0.0, max_height),
        );
        ctx.add(crate::primitives::MaxSize::height(max_height).child_id(scrollable_id))
    } else {
        padded_id
    }
}

/// Dropdown panel content (internal widget — shown as overlay).
///
/// In non-searchable mode the panel's own `build` renders the item
/// `VStack` directly. In searchable mode it instead renders a static
/// `TextInput` above a `FilteredItemList` child — only the inner list
/// binds the query signal at `BindingLevel::Rebuild`, so typing a
/// character re-filters the items without destroying (and un-focusing)
/// the search field.
struct DropdownPanel<T: Clone + PartialEq + 'static> {
    source: ItemSource<T>,
    selected: Signal<Option<T>>,
    item_label: Rc<dyn Fn(&T) -> String>,
    render_item: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    max_visible_items: usize,
    /// Bumped on every model mutation so the panel rebuilds.
    version: Signal<u64>,
    /// Active search query (searchable mode only).
    #[cfg(feature = "rich-text")]
    search_query: Option<Signal<String>>,
    /// Custom filter predicate for searchable mode. When `None`, the
    /// default is a case-insensitive substring match on the label.
    #[cfg(feature = "rich-text")]
    filter: Option<Rc<dyn Fn(&str, &T) -> bool>>,
    /// Shared slot populated during `build` with the `TextInput`'s
    /// widget id so the owning `ComboBox` can `ctx.request_focus(..)`
    /// the field when the overlay opens.
    #[cfg(feature = "rich-text")]
    search_input_slot: Rc<Cell<Option<WidgetId>>>,
    root_child_id: Option<WidgetId>,
}

/// Inner widget for the searchable dropdown's filtered item list.
/// Binds both the model-version signal and the search-query signal at
/// `BindingLevel::Rebuild`, while the sibling `TextInput` remains a
/// stable arena child of `DropdownPanel` across query-driven rebuilds —
/// so focus stays on the search field as the user types.
#[cfg(feature = "rich-text")]
struct FilteredItemList<T: Clone + PartialEq + 'static> {
    source: ItemSource<T>,
    selected: Signal<Option<T>>,
    item_label: Rc<dyn Fn(&T) -> String>,
    render_item: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    max_visible_items: usize,
    version: Signal<u64>,
    search_query: Signal<String>,
    filter: Option<Rc<dyn Fn(&str, &T) -> bool>>,
    root_child_id: Option<WidgetId>,
}

#[cfg(feature = "rich-text")]
impl<T: Clone + PartialEq + 'static> std::fmt::Debug for FilteredItemList<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilteredItemList")
            .field("item_count", &self.source.len())
            .finish()
    }
}

#[cfg(feature = "rich-text")]
impl<T: Clone + PartialEq + 'static> Widget for FilteredItemList<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use fern_core::binding::BindingLevel;
        let theme = ctx.theme().clone();

        // Rebuild on model mutation AND on query change. Both bindings
        // sit here rather than on the outer panel so the sibling
        // `TextInput` is not torn down on every keystroke.
        self.version
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.search_query
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let total = self.source.len();
        let q = self.search_query.get();
        let visible_indices: Vec<usize> = if q.is_empty() {
            (0..total).collect()
        } else {
            let q_lower = q.to_lowercase();
            let mut keep = Vec::new();
            for i in 0..total {
                if let Some(value) = self.source.get(i) {
                    let matches = match &self.filter {
                        Some(f) => f(&q, &value),
                        None => (self.item_label)(&value).to_lowercase().contains(&q_lower),
                    };
                    if matches {
                        keep.push(i);
                    }
                }
            }
            keep
        };
        let visible_count = visible_indices.len();

        let mut vstack = VStack::new();
        for (pos, &i) in visible_indices.iter().enumerate() {
            if let Some(value) = self.source.get(i) {
                let label = (self.item_label)(&value);
                vstack = vstack.child(DropdownItem {
                    value,
                    label,
                    position: pos + 1,
                    total: visible_count,
                    selected_signal: self.selected.clone(),
                    render: self.render_item.clone(),
                    root_child_id: None,
                });
            }
        }
        let vstack_id = ctx.add(vstack);
        let padded_id = ctx.add(Padding::uniform(4.0).child_id(vstack_id));

        let menu_style = theme.components.menu;
        let root_id = if visible_count > self.max_visible_items {
            let max_height =
                self.max_visible_items as f32 * menu_style.item_height + 8.0;
            let scrollable_id = ctx.add(
                crate::scroll_area::ScrollArea::from_id(padded_id)
                    .preferred_size(0.0, max_height),
            );
            ctx.add(crate::primitives::MaxSize::height(max_height).child_id(scrollable_id))
        } else {
            padded_id
        };
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

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
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
        let menu_style = theme.components.menu;

        // In non-searchable mode the panel itself binds the model-version
        // signal so the item list rebuilds on mutation. In searchable mode
        // both the model-version AND query bindings live on the inner
        // `FilteredItemList`, keeping the panel (and the `TextInput`
        // inside it) stable across query-driven rebuilds.
        let searchable = {
            #[cfg(feature = "rich-text")]
            {
                self.search_query.is_some()
            }
            #[cfg(not(feature = "rich-text"))]
            {
                false
            }
        };
        if !searchable {
            use fern_core::binding::BindingLevel;
            self.version.bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                BindingLevel::Rebuild,
            );
        }

        // Build the item-list portion of the panel. With `rich-text` +
        // searchable, that's a `FilteredItemList` child widget that owns
        // the query binding. Otherwise it's the static padded VStack.
        let list_id = {
            #[cfg(feature = "rich-text")]
            {
                if let Some(query) = &self.search_query {
                    ctx.add(FilteredItemList {
                        source: self.source.clone(),
                        selected: self.selected.clone(),
                        item_label: self.item_label.clone(),
                        render_item: self.render_item.clone(),
                        max_visible_items: self.max_visible_items,
                        version: self.version.clone(),
                        search_query: query.clone(),
                        filter: self.filter.clone(),
                        root_child_id: None,
                    })
                } else {
                    build_static_item_list(
                        ctx,
                        &self.source,
                        &self.selected,
                        &self.item_label,
                        &self.render_item,
                        self.max_visible_items,
                        &menu_style,
                    )
                }
            }
            #[cfg(not(feature = "rich-text"))]
            {
                build_static_item_list(
                    ctx,
                    &self.source,
                    &self.selected,
                    &self.item_label,
                    &self.render_item,
                    self.max_visible_items,
                    &menu_style,
                )
            }
        };

        // Searchable mode: prepend a `TextInput` with a trailing
        // `BuiltInButton::clear()` so the user can wipe the query back
        // to empty. Both sit in a VStack above the filtered items. The
        // input's widget id is captured in a shared slot so the owning
        // `ComboBox` can programmatically focus it when the overlay opens.
        let content_id = {
            #[cfg(feature = "rich-text")]
            {
                if let Some(query) = &self.search_query {
                    // `show_clear_button(true)` inserts a clear button
                    // into `TextInput`'s trailing slot, wired to empty
                    // the bound text signal, and — critically — binds
                    // its visibility to `text.is_empty().not()` so the
                    // button only appears once something has been typed.
                    // Using the built-in option here rather than a
                    // hand-wired `BuiltInButton::clear()` in the
                    // trailing slot avoids reaching for the trailing
                    // widget's id from the outside (it lives inside
                    // TextInput's build) just to register
                    // `ctx.visible_when`.
                    let search_input = crate::text_input::TextInput::new(query.clone())
                        .placeholder("Search…")
                        .show_clear_button(true);
                    let search_id = ctx.add(search_input);
                    self.search_input_slot.set(Some(search_id));
                    let search_wrapped = ctx.add(
                        Padding::new(4.0, 4.0, 0.0, 4.0).child_id(search_id),
                    );
                    let col = VStack::new()
                        .spacing(0.0)
                        .add_child(search_wrapped)
                        .add_child(list_id);
                    ctx.add(col)
                } else {
                    list_id
                }
            }
            #[cfg(not(feature = "rich-text"))]
            {
                list_id
            }
        };

        // Dropdown panel — same surface treatment as MenuList (raised + popup radius)
        let bg = RectWidget::new()
            .background(theme.colors.surface_raised)
            .border_color(theme.colors.border)
            .border_width(menu_style.popup_border_width)
            .corner_radius(CornerRadius::uniform(menu_style.popup_corner_radius));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(content_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        // Panel-level key handler: consume Tab / Shift+Tab and dismiss
        // the overlay. The framework dispatches Tab to the focused
        // widget first (and bubbles up through its ancestors) before
        // falling back to built-in focus cycling — returning `Handled`
        // here both closes the panel and suppresses the default cycle.
        // The next user action (or Shift+Tab) then moves focus through
        // the main tree naturally, starting from where focus lived
        // before the combo opened. `TextInput` itself returns
        // `Ignored` for Tab, so the bubble reaches the panel whether
        // focus is in the search field or on an item.
        let panel_handlers = HandlerSet::new().on_key(|event, ctx| match event {
            WidgetEvent::KeyDown {
                key: Key::Tab, ..
            } => {
                ctx.dismiss_all_overlays();
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        });
        ctx.apply_self_handlers(panel_handlers);

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

        // Build trigger: [label | Spacer | divider | chevron]
        let label = TextWidget::new_literal("")
            .style(theme.typography.body.clone())
            .bind_text(label_text)
            .bind_color(text_color)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);

        // Divider between the selected-value area and the chevron,
        // matching the `SplitButton` visual pattern — a thin vertical
        // rule in the `border` token that visually separates the
        // display region from the dropdown trigger indicator.
        let combo_style = theme.components.combo_box;
        let divider_fill_id =
            ctx.add(RectWidget::new().background(theme.colors.border));
        let divider_id = ctx.add(
            crate::primitives::FixedSize::new()
                .bind_width(theme.shape.border_width)
                .bind_height(combo_style.height * 0.6)
                .child_id(divider_fill_id),
        );

        let chevron =
            IconWidget::chevron_down(12.0).color(theme.colors.text_primary.with_alpha(0.5));
        let chevron_id = ctx.add(chevron);

        let row = HStack::new()
            .spacing(8.0)
            .add_child(label_id)
            .child(Spacer::new())
            .add_child(divider_id)
            .add_child(chevron_id);
        let row_id = ctx.add(row);

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

        // Pre-create the dropdown panel (dormant until opened). On
        // rebuild, first tear down the previous panel subtree — it was
        // inserted as an arena root via `ctx.add(..)` + `set_dormant`,
        // so the framework's rebuild path (which only destroys this
        // widget's direct arena children) would otherwise leave it
        // behind as an orphan on every model mutation.
        if let Some(old_id) = self.dropdown_content_id.take() {
            ctx.destroy_subtree(old_id);
        }

        // Searchable mode: allocate the query signal lazily so toggling
        // `searchable(true)` → `false` between rebuilds doesn't keep a
        // stale signal alive, while `true` → `true` preserves the
        // in-progress query across model mutations.
        #[cfg(feature = "rich-text")]
        let search_query = if self.searchable {
            let existing = self.search_query.clone();
            let q = existing.unwrap_or_else(|| Signal::new(String::new()));
            self.search_query = Some(q.clone());
            Some(q)
        } else {
            self.search_query = None;
            None
        };

        // Shared slot carrying the search `TextInput`'s widget id —
        // populated by the panel during its own `build` so the open
        // path below can `ctx.request_focus(..)` the search field as
        // soon as the overlay activates.
        #[cfg(feature = "rich-text")]
        let search_input_slot: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
        let dropdown_panel = DropdownPanel {
            source: self.source.clone(),
            selected: self.selected.clone(),
            item_label: self.item_label.clone(),
            render_item: self.render_item.clone(),
            max_visible_items: self.max_visible_items,
            version: panel_version,
            #[cfg(feature = "rich-text")]
            search_query,
            #[cfg(feature = "rich-text")]
            filter: self.filter.clone(),
            #[cfg(feature = "rich-text")]
            search_input_slot: search_input_slot.clone(),
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
            #[cfg(feature = "rich-text")]
            let search_input_slot = search_input_slot.clone();
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
                // Searchable mode: land focus in the search field so
                // the user can start typing immediately after opening.
                #[cfg(feature = "rich-text")]
                if let Some(input_id) = search_input_slot.get() {
                    ctx.request_focus(input_id);
                }
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

        // ARIA combobox pattern: when the popup is a filtered list, mark
        // `aria-autocomplete="list"` so assistive tech announces the
        // filter behavior. Only applied in searchable mode.
        #[cfg(feature = "rich-text")]
        if self.searchable {
            builder.set_auto_complete(fern_core::accesskit::AutoComplete::List);
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
    fn dropdown_items_span_panel_width() {
        // Regression: DropdownItem::size_that_fits delegates to the root
        // ZStack, which queries children with UNSPECIFIED — so items
        // collapsed to the intrinsic width of their text label and
        // appeared as narrow centered stripes inside the wider dropdown
        // panel. The panel bg and RectWidgets filled the panel area but
        // the items (containing the labels) did not, producing a
        // visually-blank dropdown where only clicks that happened to
        // land on the narrow label strip changed the selection.
        //
        // Each item's bounds must match the panel's inner width
        // (accounting for the 4px outer padding on both sides).
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(400.0, 500.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(400.0, 500.0));
        assert_eq!(tree.active_overlays().len(), 1);

        let content_ids = tree.overlay_manager().active_content_ids();
        let panel_width = tree.bounds(content_ids[0]).width;
        assert!(panel_width > 100.0, "panel should be reasonably wide");

        for name in ["Apple", "Banana", "Cherry"] {
            let id = tree
                .find_by_label(name)
                .unwrap_or_else(|| panic!("dropdown should contain {name}"));
            let w = tree.bounds(id).width;
            // Panel has 4px padding on each side — items should fill
            // the inner width (panel_width - 8).
            assert!(
                w >= panel_width - 10.0,
                "row {name} should span panel width: item={}, panel={}",
                w,
                panel_width
            );
        }
    }

    #[test]
    fn dropdown_items_have_nonzero_bounds_when_open() {
        // Regression guard for a rendering bug where the dropdown panel
        // showed a blank surface: the item rows must each occupy a visible
        // rectangle after the overlay opens. Without this, the widget-
        // catalog demo regressed to an empty-looking dropdown even though
        // the logic tests all passed.
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(
            vec!["Apple", "Banana", "Cherry"],
            selected.clone(),
        ));
        tree.layout(SizeProposal::exact(300.0, 400.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 400.0));
        assert_eq!(tree.active_overlays().len(), 1);

        for name in ["Apple", "Banana", "Cherry"] {
            let id = tree
                .find_by_label(name)
                .unwrap_or_else(|| panic!("dropdown should contain {name}"));
            let b = tree.bounds(id);
            assert!(
                b.width > 0.0 && b.height > 0.0,
                "{name} row should have nonzero bounds, got {:?}",
                b
            );
        }
    }

    #[test]
    fn many_items_scroll_without_overflow_past_overlay() {
        // More items than max_visible_items (default 8): the dropdown
        // must cap at roughly max_visible * item_height, not grow to
        // fit every row.
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let many: Vec<String> = (0..20).map(|i| format!("Item {i}")).collect();
        let cb = tree.add(ComboBox::new(many, selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 800.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 800.0));

        let content_ids = tree.overlay_manager().active_content_ids();
        let panel_bounds = tree.bounds(content_ids[0]);
        // 20 rows × 32px = 640px uncapped; expect well under that.
        assert!(
            panel_bounds.height < 400.0,
            "panel should be capped, was {}",
            panel_bounds.height
        );
        assert!(
            panel_bounds.height > 0.0,
            "panel should have visible height"
        );
    }

    #[test]
    fn render_item_closure_receives_selection_snapshot_at_build() {
        // Documents current behavior: the `bool selected` passed to
        // `render_item` reflects the selection state at the moment the
        // dropdown panel was built. It is NOT automatically re-fired when
        // the selection changes while the dropdown is open — consumers
        // that need a reactive appearance should close over a Signal and
        // bind primitives directly. See `.render_item()` rustdoc.
        use std::sync::Mutex;
        let observed: Rc<Mutex<Vec<(String, bool)>>> = Rc::new(Mutex::new(Vec::new()));

        let mut tree = light_tree();
        let selected = Signal::new(Some("Banana".to_string()));
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
        tree.click(cb);
        tree.layout(SizeProposal::exact(300.0, 300.0));

        let calls = observed.lock().unwrap().clone();
        assert!(
            calls.contains(&("Apple".to_string(), false)),
            "Apple row should have been rendered with selected=false; got {:?}",
            calls
        );
        assert!(
            calls.contains(&("Banana".to_string(), true)),
            "Banana row should have been rendered with selected=true; got {:?}",
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

    // ─── Searchable mode (rich-text feature) ──────────────────────────

    #[cfg(feature = "rich-text")]
    #[test]
    fn searchable_filters_list_to_matching_items() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let query = Signal::new(String::new());
        let cb = tree.add(
            ComboBox::new(
                vec!["Apple", "Banana", "Blueberry", "Cherry"],
                selected.clone(),
            )
            .search_query(query.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 500.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(400.0, 500.0));

        // All four items visible initially.
        for name in ["Apple", "Banana", "Blueberry", "Cherry"] {
            assert!(
                tree.find_by_label(name).is_some(),
                "expected {name} before filtering",
            );
        }

        // Set the query to "B" — only Banana and Blueberry should remain.
        query.set("B".to_string());
        tree.layout(SizeProposal::exact(400.0, 500.0));

        assert!(tree.find_by_label("Apple").is_none(), "Apple should be filtered out");
        assert!(tree.find_by_label("Cherry").is_none(), "Cherry should be filtered out");
        assert!(tree.find_by_label("Banana").is_some(), "Banana should still be visible");
        assert!(tree.find_by_label("Blueberry").is_some(), "Blueberry should still be visible");
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn searchable_custom_filter_is_consulted() {
        // Filter is called with (query, item). Route every item through
        // a closure that accepts only items whose label length equals
        // the query length — a contrived but easily-asserted predicate.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        CALLS.store(0, Ordering::SeqCst);

        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let query = Signal::new(String::new());
        let cb = tree.add(
            ComboBox::new(vec!["ab", "abc", "abcd"], selected.clone())
                .search_query(query.clone())
                .filter(|q, v: &String| {
                    CALLS.fetch_add(1, Ordering::SeqCst);
                    v.len() == q.len()
                }),
        );
        tree.layout(SizeProposal::exact(400.0, 500.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(400.0, 500.0));

        query.set("xyz".to_string()); // length 3 → only "abc" matches
        tree.layout(SizeProposal::exact(400.0, 500.0));

        assert!(CALLS.load(Ordering::SeqCst) >= 3, "filter should have been called per item");
        assert!(tree.find_by_label("ab").is_none());
        assert!(tree.find_by_label("abc").is_some());
        assert!(tree.find_by_label("abcd").is_none());
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn accessibility_searchable_sets_autocomplete() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()).searchable(true));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let node = build_raw_a11y_node(&mut tree, cb);
        assert_eq!(
            node.auto_complete(),
            Some(fern_core::accesskit::AutoComplete::List),
            "searchable combobox must expose aria-autocomplete=list",
        );
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn tab_dismisses_open_searchable_dropdown() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let query = Signal::new(String::new());
        let cb = tree.add(
            ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
                .search_query(query.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 500.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(400.0, 500.0));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Tab, fern_core::event::Modifiers::NONE);
        tree.layout(SizeProposal::exact(400.0, 500.0));
        assert!(
            tree.active_overlays().is_empty(),
            "Tab should dismiss the open dropdown so focus can leave the popup"
        );
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn shift_tab_dismisses_open_searchable_dropdown() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let query = Signal::new(String::new());
        let cb = tree.add(
            ComboBox::new(vec!["Apple", "Banana", "Cherry"], selected.clone())
                .search_query(query.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 500.0));
        tree.click(cb);
        tree.layout(SizeProposal::exact(400.0, 500.0));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Tab, fern_core::event::Modifiers::SHIFT);
        tree.layout(SizeProposal::exact(400.0, 500.0));
        assert!(
            tree.active_overlays().is_empty(),
            "Shift+Tab should also dismiss the open dropdown"
        );
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn searchable_opens_with_focus_in_search_field() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let query = Signal::new(String::new());
        let cb = tree.add(
            ComboBox::new(vec!["Apple", "Banana"], selected.clone())
                .search_query(query.clone()),
        );
        tree.layout(SizeProposal::exact(400.0, 500.0));

        tree.click(cb);
        tree.layout(SizeProposal::exact(400.0, 500.0));

        let focused = tree
            .focused()
            .expect("something inside the dropdown should be focused after open");
        assert_ne!(focused, cb, "focus must leave the combo trigger");
        // The focused widget should be inside the overlay content subtree.
        let overlay_root = tree.overlay_manager().active_content_ids()[0];
        let mut cur = Some(focused);
        let mut in_overlay = false;
        while let Some(id) = cur {
            if id == overlay_root {
                in_overlay = true;
                break;
            }
            cur = tree.parent(id);
        }
        assert!(
            in_overlay,
            "focused widget should be inside the dropdown panel"
        );
    }

    #[cfg(feature = "rich-text")]
    #[test]
    fn accessibility_non_searchable_omits_autocomplete() {
        let mut tree = light_tree();
        let selected = Signal::new(None::<String>);
        let cb = tree.add(ComboBox::new(fruits(), selected.clone()));
        tree.layout(SizeProposal::exact(300.0, 200.0));

        let node = build_raw_a11y_node(&mut tree, cb);
        assert_eq!(
            node.auto_complete(),
            None,
            "non-searchable combobox must not advertise autocomplete",
        );
    }

}
