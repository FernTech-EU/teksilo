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
//!
//! The widget is split across four internal modules:
//! - [`state`] holds the interaction-state enum, the `ItemSource` accessor,
//!   and color/index helpers.
//! - [`item`] holds the single-row `DropdownItem` widget.
//! - [`panel`] holds the `DropdownPanel` overlay content and — under
//!   `rich-text` — the `FilteredItemList` inner widget.
//! - [`tests`] holds the headless unit tests.

use bastyde_i18n::lit;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use bastyde_canvas::{Rect, Size, SizeProposal};
use bastyde_core::accessibility::{AccessNodeBuilder, widget_id_to_node_id};
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use bastyde_core::signal::Signal;
use bastyde_core::styles::{ComboBoxStyle, ComboBoxStyleConfig, SharedComboBoxStyle};
use bastyde_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_data::{DataChange, ListDataSource, ListModel};
use bastyde_tokens::{TextRole, TextStyleRole};

use crate::primitives::TextWidget;

mod item;
mod panel;
mod state;

#[cfg(test)]
mod tests;

use self::panel::DropdownPanel;
use self::state::{DEFAULT_MAX_VISIBLE_ITEMS, ItemSource, resolve_index};

// Re-export so callers can write `ComboBox::new(...).variant(ComboBoxVariant::Filled)`
// without reaching into `bastyde::core::styles`.
pub use bastyde_core::styles::ComboBoxVariant;

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
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
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
    /// Tier-1 design-language variant. The active `ComboBoxStyle`
    /// decides how to paint each variant; IntUI's default ships
    /// `Outlined` (bordered) and `Plain` (chrome-less) out of the box,
    /// with `Filled` falling back to `Outlined` until per-variant
    /// recipes land.
    variant: ComboBoxVariant,
    /// Per-call style override.
    style_override: Option<SharedComboBoxStyle>,
    // Build state — four mutable signals replace the legacy
    // `ComboBoxState` enum. `is_open` survives until the dropdown
    // dismisses (overlay callback resets it); `is_focused` /
    // `is_hovered` flip on the corresponding handlers; `is_disabled`
    // mirrors `!self.enabled` (snapshotted at build because
    // `.enabled(bool)` is an immutable builder option).
    is_open: Signal<bool>,
    is_hovered: Signal<bool>,
    is_focused: Signal<bool>,
    is_disabled: Signal<bool>,
    root_child_id: Option<WidgetId>,
    dropdown_content_id: Option<WidgetId>,
}

impl ComboBox<String> {
    /// Create a ComboBox from a list of strings.
    ///
    /// Accepts any `impl Into<String>` — string literals (`&str`),
    /// owned `String`s, resolved `LocalizedString`s, etc. For
    /// translated items, resolve translations before passing in,
    /// e.g. `vec![tr!(apple()).resolve_now(), ...]`.
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
            initial_enabled: true,
            max_visible_items: DEFAULT_MAX_VISIBLE_ITEMS,
            #[cfg(feature = "rich-text")]
            searchable: false,
            #[cfg(feature = "rich-text")]
            filter: None,
            #[cfg(feature = "rich-text")]
            search_query: None,
            variant: ComboBoxVariant::default(),
            style_override: None,
            is_open: Signal::new(false),
            is_hovered: Signal::new(false),
            is_focused: Signal::new(false),
            is_disabled: Signal::new(false),
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
    pub fn from_model<F>(model: ListModel<T>, selected: Signal<Option<T>>, item_label: F) -> Self
    where
        F: Fn(&T) -> String + 'static,
    {
        Self::new_with_item_source(ItemSource::from_model(model), selected, Rc::new(item_label))
    }

    /// Backed by a custom [`ListDataSource`] — for external or paged data.
    pub fn from_source<S, F>(source: S, selected: Signal<Option<T>>, item_label: F) -> Self
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
    pub fn render_item(mut self, f: impl Fn(&T, bool) -> Box<dyn Widget> + 'static) -> Self {
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
    /// Accepts a `tr!(...)` directly (resolved at build); use
    /// [`placeholder_literal`](Self::placeholder_literal) for an
    /// untranslated string.
    pub fn placeholder(mut self, text: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = text.into();
        self.placeholder = ls.resolve_now();
        self
    }

    /// Accessible label describing what this combo box is for
    /// (e.g. "Fruit", "Font family"). Independent of the visible
    /// placeholder and of the current selection — screen readers
    /// announce this as the name of the control.
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Set the initial enabled state. Forwarded to the arena at build
    /// time. For reactive enable/disable use `ctx.enabled_when(id, signal)`.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Pick a Tier-1 design-language variant
    /// ([`ComboBoxVariant::Outlined`] / `Filled` / `Underline` / `Plain`).
    /// The active [`ComboBoxStyle`] decides what to do with the hint —
    /// IntUI's default impl honours `Outlined` (default) and `Plain`;
    /// a custom impl (Material 3, macOS, etc.) might paint differently.
    pub fn variant(mut self, variant: ComboBoxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the active [`ComboBoxStyle`] for this widget instance
    /// only. The default IntUI chrome ([`crate::styles::RecipeComboBoxStyle`])
    /// reads its tokens from `theme.components.combo_box`; custom impls
    /// can paint anything they want around the selected-label slot.
    pub fn style(mut self, style: impl ComboBoxStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }
}

/// Searchable-mode builders. Gated behind the `rich-text` feature
/// because the search field is a `TextInput`, which shares the
/// `RichTextEditor` engine and therefore the `bastyde-text` dependency.
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
            .field("initial_enabled", &self.initial_enabled)
            .finish()
    }
}

impl<T: Clone + PartialEq + 'static> Widget for ComboBox<T> {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward initial-enabled to the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // Refresh the four interaction signals every build. The three
        // non-disabled ones start in their resting state; `is_disabled`
        // now mirrors the arena's effective enabled-state reactively
        // (replaced the build-time snapshot — see IconButton). We
        // wire `effective_enabled.not()` into `self.is_disabled` so
        // existing observers keep working without rewiring.
        self.is_open.set(false);
        self.is_hovered.set(false);
        self.is_focused.set(false);
        // Drive `self.is_disabled` from the arena's effective_enabled.
        // Replace with a derived signal — but `self.is_disabled` is
        // owned by the widget and may have observers, so push the
        // current value and register an effect to keep it in sync.
        self.is_disabled.set(!effective_enabled.get());
        {
            let is_disabled = self.is_disabled.clone();
            ctx.effect(&effective_enabled, move |on| {
                let want = !*on;
                if is_disabled.get() != want {
                    is_disabled.set(want);
                }
            });
        }

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

        // Label colour follows the disabled signal — the chrome style
        // owns bg / border / focus ring; the widget owns its label.
        let text_role = self.is_disabled.map(|d| {
            if *d {
                TextRole::Disabled
            } else {
                TextRole::Primary
            }
        });

        // Build the selected-label subtree the style will host. Wrapped
        // in `.a11y_hidden()` because the combo box's own
        // `accessibility(builder)` already announces the selected value
        // via `set_value`, so a screen reader exposed to the inner text
        // node would double-announce.
        let label = TextWidget::new(lit!(""))
            .style(TextStyleRole::Body)
            .bind_text(label_text)
            .bind_color(text_role)
            .single_line()
            .a11y_hidden();
        let label_id = ctx.add(label);

        // Resolve the active style: per-call override > theme slot >
        // built-in `RecipeComboBoxStyle` default. The style produces
        // the entire trigger chrome (bg + border + padding + divider +
        // chevron + min-height) around our `selected_label`.
        let style: SharedComboBoxStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.combo_box.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeComboBoxStyle));

        let cfg = ComboBoxStyleConfig {
            selected_label: label_id,
            is_open: self.is_open.clone(),
            is_hovered: self.is_hovered.clone(),
            is_focused: self.is_focused.clone(),
            is_disabled: self.is_disabled.clone(),
            variant: self.variant,
        };
        let root_id = style.make_body(&cfg, ctx);
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
        let is_open_h = self.is_open.clone();
        let is_hovered_h = self.is_hovered.clone();
        let is_focused_h = self.is_focused.clone();

        // Shared dismiss callback — invoked by the overlay manager
        // whenever the dropdown is dismissed, regardless of path
        // (our own Enter/Escape handlers, framework-level
        // EscapeOrClickOutside, pointer-leave, cascade). Flips
        // `is_open` back to false so `accessibility(builder)` stays
        // truthful about the popup state.
        let dismiss_callback: OverlayDismissCallback = {
            let is_open = self.is_open.clone();
            Rc::new(move || {
                if is_open.get() {
                    is_open.set(false);
                }
            })
        };

        // Helper to open the overlay — used by tap and several key handlers.
        let open_overlay = {
            let is_open = self.is_open.clone();
            let dismiss_callback = dismiss_callback.clone();
            #[cfg(feature = "rich-text")]
            let search_input_slot = search_input_slot.clone();
            Rc::new(move |ctx: &mut EventContext| {
                is_open.set(true);
                ctx.activate(dropdown_id);
                ctx.show_overlay(OverlayRequest {
                    content_id: dropdown_id,
                    anchor: self_id,
                    placement: OverlayPlacement::BelowPreferred,
                    dismiss: DismissBehavior::EscapeOrClickOutside,
                    layer: OverlayLayer::InTree,
                    parent_overlay: None,
                    on_dismiss: Some(dismiss_callback.clone()),
                    fade_duration: None,
                });
                // Searchable mode: land focus in the search field so
                // the user can start typing immediately after opening.
                #[cfg(feature = "rich-text")]
                if let Some(input_id) = search_input_slot.get() {
                    ctx.request_focus(input_id);
                }
            })
        };

        // Framework gates events on `arena.is_enabled` — no per-
        // handler enabled snapshot guards anymore.
        let handler_set = HandlerSet::new()
            .on_tap({
                let open_overlay = open_overlay.clone();
                move |_pos, ctx: &mut EventContext| {
                    open_overlay(ctx);
                }
            })
            .on_hover({
                let is_open = is_open_h.clone();
                let is_hovered = is_hovered_h.clone();
                move |entered: bool, _ctx: &mut EventContext| {
                    // Don't churn the hovered signal while the dropdown
                    // is open — the bg stays in its open colour until
                    // the overlay dismisses.
                    if is_open.get() {
                        return;
                    }
                    is_hovered.set(entered);
                }
            })
            .on_key({
                let is_open = self.is_open.clone();
                let selected = self.selected.clone();
                let source = self.source.clone();
                let item_label_for_keys = self.item_label.clone();
                let hint = self.selected_index_hint.clone();
                let open_overlay = open_overlay.clone();
                // PageUp/PageDown step by one visible page (clamped to 1
                // so a `max_visible_items(1)` combo still moves).
                let page_size = self.max_visible_items.max(1);
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
                    match event {
                        WidgetEvent::KeyDown {
                            key: Key::Enter | Key::Space,
                            ..
                        } => {
                            if is_open.get() {
                                is_open.set(false);
                                ctx.dismiss_all_except_hosts();
                            } else {
                                open_overlay(ctx);
                            }
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::Escape, ..
                        } => {
                            if is_open.get() {
                                is_open.set(false);
                                ctx.dismiss_all_except_hosts();
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        // Tab / Shift+Tab while open: dismiss the dropdown
                        // and let focus flow naturally. The trigger itself
                        // receives Tab (focus is on the combo, not inside
                        // the panel) for non-searchable combos, so the
                        // panel's own Tab handler wouldn't fire here. In
                        // searchable mode the panel's handler covers it
                        // because focus is on the inner TextInputField.
                        // Consuming the event suppresses the framework's
                        // built-in cycle so nothing else stole focus while
                        // the overlay tore down.
                        WidgetEvent::KeyDown { key: Key::Tab, .. } => {
                            if is_open.get() {
                                is_open.set(false);
                                ctx.dismiss_all_except_hosts();
                                EventResponse::Handled
                            } else {
                                EventResponse::Ignored
                            }
                        }
                        WidgetEvent::KeyDown {
                            key: Key::ArrowDown,
                            ..
                        } => {
                            if !is_open.get() {
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
                            if !is_open.get() {
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
                        WidgetEvent::KeyDown { key: Key::Home, .. } => {
                            if source.len() == 0 {
                                return EventResponse::Handled;
                            }
                            pick_at(0);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown { key: Key::End, .. } => {
                            let n = source.len();
                            if n == 0 {
                                return EventResponse::Handled;
                            }
                            pick_at(n - 1);
                            EventResponse::Handled
                        }
                        // PageDown / PageUp — advance or retreat selection
                        // by one page, where a page is `max_visible_items`
                        // rows. Mirrors the standard combo-box keyboard
                        // convention and also gets the visible range to
                        // follow via `register_scroll_into_view`.
                        WidgetEvent::KeyDown {
                            key: Key::PageDown, ..
                        } => {
                            let n = source.len();
                            if n == 0 {
                                return EventResponse::Handled;
                            }
                            if !is_open.get() {
                                open_overlay(ctx);
                            }
                            let current_idx = selected
                                .get()
                                .as_ref()
                                .and_then(|v| resolve_index(&source, v, &hint))
                                .unwrap_or(0);
                            let target = current_idx.saturating_add(page_size).min(n - 1);
                            pick_at(target);
                            EventResponse::Handled
                        }
                        WidgetEvent::KeyDown {
                            key: Key::PageUp, ..
                        } => {
                            let n = source.len();
                            if n == 0 {
                                return EventResponse::Handled;
                            }
                            if !is_open.get() {
                                open_overlay(ctx);
                            }
                            let current_idx = selected
                                .get()
                                .as_ref()
                                .and_then(|v| resolve_index(&source, v, &hint))
                                .unwrap_or(0);
                            let target = current_idx.saturating_sub(page_size);
                            pick_at(target);
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
                is_focused_h.set(gained);
            })
            // Focus walker skips disabled subtrees on its own.
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        ctx.apply_self_handlers(handler_set);

        // Return BOTH the trigger root AND the dormant dropdown as
        // children so the framework links `dropdown_id` under this
        // widget in the arena instead of leaving it an orphan root.
        // Hit-test walks all arena roots; an orphan dormant subtree
        // can leak into hit-tests at fallback bounds and intercept
        // clicks meant for siblings. See popover_button.rs for the
        // same pattern.
        vec![root_id, dropdown_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        let min_height = crate::styles::recipe_combo_box_style::COMBO_BOX_HEIGHT;
        match self.root_child_id {
            Some(id) => {
                let child_size = ctx
                    .child_size(id, proposal)
                    .unwrap_or_else(|| proposal.resolve(0.0, 0.0));
                Size::new(
                    child_size.width.max(120.0),
                    child_size.height.max(min_height),
                )
            }
            None => proposal.resolve(120.0, min_height),
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
        // The trigger fills our bounds; the dropdown's bounds are
        // owned by the overlay manager when shown (`position_overlays`),
        // so we zero-size it here.
        for child in children.iter_mut() {
            if Some(child.id) == self.dropdown_content_id {
                child.size = bastyde_canvas::Size::ZERO;
                continue;
            }
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(bastyde_core::accesskit::Role::ComboBox);
        builder.set_has_popup(bastyde_core::accesskit::HasPopup::Listbox);

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

        builder.set_expanded(self.is_open.get());

        // Only set aria-controls when the popup is open — the listbox node is
        // absent from the tree when closed, and pointing at a missing node
        // causes AT crashes (VoiceOver unwrap in linked_ui_elements).
        if self.is_open.get()
            && let Some(popup_id) = self.dropdown_content_id
        {
            builder.push_controlled(widget_id_to_node_id(popup_id));
        }

        // ARIA combobox pattern: when the popup is a filtered list, mark
        // `aria-autocomplete="list"` so assistive tech announces the
        // filter behavior. Only applied in searchable mode.
        #[cfg(feature = "rich-text")]
        if self.searchable {
            builder.set_auto_complete(bastyde_core::accesskit::AutoComplete::List);
        }

        // Always advertise actions — framework gates them at dispatch
        // via `arena.is_enabled`, and the a11y walker handles
        // `set_disabled` from the same arena state.
        builder.add_action(bastyde_core::accesskit::Action::Click);
        builder.add_action(bastyde_core::accesskit::Action::Focus);
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut out = Vec::new();
        if let Some(id) = self.root_child_id {
            out.push(id);
        }
        if let Some(id) = self.dropdown_content_id {
            out.push(id);
        }
        out
    }
}
