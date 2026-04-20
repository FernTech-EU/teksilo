//! `DropdownPanel` — the overlay-content widget shown when the combo
//! is open. Owns the Tab / ArrowDown / ArrowUp key handling, the
//! `TextInput`-backed search field (under `rich-text`), and the inner
//! `FilteredItemList` child that binds the query + version signals.
//!
//! The non-searchable path uses `build_static_item_list` to assemble a
//! padded `VStack` of `DropdownItem`s directly; the searchable path
//! instead renders a stable `TextInput` above a `FilteredItemList`
//! child that rebuilds on query changes — the sibling `TextInput`
//! survives each keystroke so the cursor doesn't jump.

#[cfg(feature = "rich-text")]
use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::list_view::ListView;
use crate::primitives::{FixedSize, HStack, Padding, RectWidget, VStack, ZStack};
use crate::scroll_bar::{ScrollBar, ScrollBarOrientation};

use super::item::DropdownItem;
use super::state::ItemSource;

/// Build the static (unfiltered) item list subtree.
///
/// Two paths:
/// - **Small list** (`total <= max_visible_items`): a padded `VStack`
///   of eagerly-materialized `DropdownItem`s. Simple, cheap — no
///   reason to pay the `ListView` bookkeeping cost for a handful of
///   rows.
/// - **Large list**: a virtualized `ListView` sized to
///   `max_visible_items * item_height`, wrapped in the same 4 px
///   padding. Only the rows in the visible range (plus ListView's
///   small buffer) are actually built, so a 10 000-item combo no
///   longer materializes 10 000 widget subtrees on open.
///
/// Returns the root id for insertion into the panel's `ZStack`.
/// Shared by the non-searchable path of `DropdownPanel` and —
/// indirectly via `FilteredItemList` — the searchable path.
pub(super) fn build_static_item_list<T: Clone + PartialEq + 'static>(
    ctx: &mut BuildContext,
    source: &ItemSource<T>,
    selected: &Signal<Option<T>>,
    item_label: &Rc<dyn Fn(&T) -> String>,
    render_item: &Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    max_visible_items: usize,
    menu_style: &fern_tokens::MenuStyle,
) -> WidgetId {
    let total = source.len();
    if total <= max_visible_items {
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
        return ctx.add(Padding::uniform(4.0).child_id(vstack_id));
    }

    build_virtualized_list(
        ctx,
        source,
        selected,
        item_label,
        render_item,
        max_visible_items,
        menu_style,
        None,
    )
}

/// Virtualized variant of the dropdown item list. When
/// `filtered_indices` is `None`, the list addresses the full source
/// directly; when `Some`, it projects each filtered position through
/// the stored index list. In both cases `ListView` only materializes
/// the rows in the viewport (plus its internal buffer) — the large-
/// list regression (combos with thousands of items building a full
/// widget subtree per row) is fixed here.
fn build_virtualized_list<T: Clone + PartialEq + 'static>(
    ctx: &mut BuildContext,
    source: &ItemSource<T>,
    selected: &Signal<Option<T>>,
    item_label: &Rc<dyn Fn(&T) -> String>,
    render_item: &Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    max_visible_items: usize,
    menu_style: &fern_tokens::MenuStyle,
    filtered_indices: Option<Vec<usize>>,
) -> WidgetId {
    let item_height = menu_style.item_height;
    let viewport_height = max_visible_items as f32 * item_height;

    // Either address the raw source directly (unfiltered, saves an
    // O(n) identity-mapping allocation for large lists) or project
    // through a filter index list shared between the ListView source
    // and the scroll-into-view effect.
    let filtered_indices: Option<Rc<Vec<usize>>> = filtered_indices.map(Rc::new);
    let visible_count = match &filtered_indices {
        Some(indices) => indices.len(),
        None => source.len(),
    };

    // Build a `ListSource<T>` that resolves rows back into the
    // underlying `ItemSource` so the raw source keeps its reactivity
    // and observer plumbing.
    let list_source = match &filtered_indices {
        None => source.to_list_source(),
        Some(indices) => {
            let src = source.clone();
            let indices = indices.clone();
            let len_fn: Rc<dyn Fn() -> usize> = Rc::new({
                let indices = indices.clone();
                move || indices.len()
            });
            let item_at: Rc<dyn Fn(usize) -> Option<T>> = Rc::new({
                let src = src.clone();
                let indices = indices.clone();
                move |pos| indices.get(pos).and_then(|&i| src.get(i))
            });
            let observe_fn = src.observe.clone();
            crate::list_source::ListSource::from_cloning_accessors(len_fn, item_at, observe_fn)
        }
    };

    // Delegate: wrap each visible row in the existing `DropdownItem`
    // so tap-to-select, highlight, and a11y keep working unchanged.
    // `index` here is the *filtered* position — the one visible to the
    // user — which is also what `position_in_set` / `size_of_set`
    // should reflect.
    let selected_for_delegate = selected.clone();
    let item_label_for_delegate = item_label.clone();
    let render_item_for_delegate = render_item.clone();
    let delegate = move |index: usize, value: &T, _selected: bool| -> Box<dyn Widget> {
        let label = (item_label_for_delegate)(value);
        Box::new(DropdownItem {
            value: value.clone(),
            label,
            position: index + 1,
            total: visible_count,
            selected_signal: selected_for_delegate.clone(),
            render: render_item_for_delegate.clone(),
            root_child_id: None,
        })
    };

    // Build the ListView with the internal scrollbar disabled. The
    // panel mounts its own `ScrollBar` as a sibling (see below) so
    // that thumb drags survive the ListView's scroll-driven rebuilds:
    // if the scrollbar lived inside the ListView, each rebuild
    // (triggered when the visible range crosses the buffer mid-drag)
    // would destroy the scrollbar widget and cancel its in-flight
    // gesture, making the drag look like it does nothing.
    let list_view = ListView::from_list_source(list_source, delegate)
        .item_height(item_height)
        .show_scrollbar(false);
    let scroll_y = list_view.scroll_y_signal().clone();
    let max_scroll_y = list_view.max_scroll_y_signal().clone();
    let viewport_ratio_y = list_view.viewport_ratio_y_signal().clone();
    let list_view_id = ctx.add(list_view);

    // External scrollbar — lives as a sibling of the ListView inside
    // the panel so ListView rebuilds leave it untouched. Wired to the
    // same reactive state the list already updates during layout.
    let external_scrollbar = ScrollBar::new(
        ScrollBarOrientation::Vertical,
        scroll_y.clone(),
        max_scroll_y,
        viewport_ratio_y,
    );
    let scrollbar_id = ctx.add(external_scrollbar);

    // HStack { ListView | ScrollBar } — the list fills the remaining
    // width, the scrollbar takes its fixed thickness. Both ride
    // together under the outer `FixedSize` so the viewport height
    // exactly equals `max_visible_items * item_height`.
    let row_id = ctx.add(
        HStack::new()
            .spacing(0.0)
            .add_child(list_view_id)
            .add_child(scrollbar_id),
    );

    let padded_id = ctx.add(Padding::uniform(4.0).child_id(row_id));
    let outer_height = viewport_height + 8.0;
    let sized_id = ctx.add(
        FixedSize::new()
            .bind_height(outer_height)
            .child_id(padded_id),
    );

    register_scroll_into_view(
        ctx,
        source.clone(),
        selected.clone(),
        scroll_y,
        filtered_indices,
        item_height,
        viewport_height,
    );

    sized_id
}

/// Register a `ctx.effect` on `selected` that scrolls the `ListView`'s
/// scroll signal so the currently-selected row stays inside the
/// viewport. Called once synchronously to sync the initial scroll
/// position, then registered as an effect for subsequent selection
/// changes. Shared by both `build_static_item_list` and
/// `FilteredItemList` so non-searchable and searchable paths behave
/// identically.
///
/// `filtered_indices` maps the filtered-list position to the raw
/// `source` index — `None` for the non-searchable path (filtered
/// position equals source index), `Some` for searchable mode's
/// current filter output. The scroll coordinate space is the
/// ListView's own content-space (row 0 starts at y=0), so no extra
/// padding offset is applied here — the outer `Padding::uniform(4.0)`
/// lives outside the scroll region.
fn register_scroll_into_view<T: Clone + PartialEq + 'static>(
    ctx: &mut BuildContext,
    source: ItemSource<T>,
    selected: Signal<Option<T>>,
    scroll_y: Signal<f32>,
    filtered_indices: Option<Rc<Vec<usize>>>,
    item_height: f32,
    viewport_height: f32,
) {
    let scroll_into_view = {
        let source = source.clone();
        let scroll_y = scroll_y.clone();
        move |sel: &Option<T>| {
            let Some(v) = sel.as_ref() else { return };
            let pos = match &filtered_indices {
                None => match super::state::index_of(&source, v) {
                    Some(i) => i,
                    None => return,
                },
                Some(indices) => match indices
                    .iter()
                    .position(|&i| source.get(i).as_ref() == Some(v))
                {
                    Some(p) => p,
                    None => return,
                },
            };
            let item_top = pos as f32 * item_height;
            let item_bot = item_top + item_height;
            let cur_scroll = scroll_y.get();
            let cur_bot = cur_scroll + viewport_height;
            if item_top < cur_scroll {
                scroll_y.set(item_top);
            } else if item_bot > cur_bot {
                scroll_y.set(item_bot - viewport_height);
            }
        }
    };
    // Initial sync so a pre-selected value opens already in view.
    scroll_into_view(&selected.get());
    ctx.effect(&selected, move |sel| scroll_into_view(sel));
}

/// Dropdown panel content (internal widget — shown as overlay).
///
/// In non-searchable mode the panel's own `build` renders the item
/// `VStack` directly. In searchable mode it instead renders a static
/// `TextInput` above a `FilteredItemList` child — only the inner list
/// binds the query signal at `BindingLevel::Rebuild`, so typing a
/// character re-filters the items without destroying (and un-focusing)
/// the search field.
pub(super) struct DropdownPanel<T: Clone + PartialEq + 'static> {
    pub(super) source: ItemSource<T>,
    pub(super) selected: Signal<Option<T>>,
    pub(super) item_label: Rc<dyn Fn(&T) -> String>,
    pub(super) render_item: Option<Rc<dyn Fn(&T, bool) -> Box<dyn Widget>>>,
    pub(super) max_visible_items: usize,
    /// Bumped on every model mutation so the panel rebuilds.
    pub(super) version: Signal<u64>,
    /// Active search query (searchable mode only).
    #[cfg(feature = "rich-text")]
    pub(super) search_query: Option<Signal<String>>,
    /// Custom filter predicate for searchable mode. When `None`, the
    /// default is a case-insensitive substring match on the label.
    #[cfg(feature = "rich-text")]
    pub(super) filter: Option<Rc<dyn Fn(&str, &T) -> bool>>,
    /// Shared slot populated during `build` with the `TextInput`'s
    /// widget id so the owning `ComboBox` can `ctx.request_focus(..)`
    /// the field when the overlay opens.
    #[cfg(feature = "rich-text")]
    pub(super) search_input_slot: Rc<Cell<Option<WidgetId>>>,
    pub(super) root_child_id: Option<WidgetId>,
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
        let theme = ctx.theme_signal().get();

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
        let menu_style = theme.components.menu;

        // Large filtered result — hand off to the shared virtualized
        // path so only the visible rows are materialized even when the
        // query matches thousands of items.
        let root_id = if visible_count > self.max_visible_items {
            build_virtualized_list(
                ctx,
                &self.source,
                &self.selected,
                &self.item_label,
                &self.render_item,
                self.max_visible_items,
                &menu_style,
                Some(visible_indices),
            )
        } else {
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
            ctx.add(Padding::uniform(4.0).child_id(vstack_id))
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
        let theme_signal = ctx.theme_signal();
        let menu_style = theme_signal.get().components.menu;

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
                    //
                    // `on_submit` dismisses the overlay on Enter. The
                    // `TextInputField` consumes `Enter` before it can
                    // bubble to the panel's own key handler, so we
                    // rely on this hook instead. The selection
                    // tracked in `selected` (driven by the panel's
                    // ArrowDown/ArrowUp handler) is already correct
                    // when the user confirms.
                    let search_input = crate::text_input::TextInput::new(query.clone())
                        .placeholder("Search…")
                        .show_clear_button(true)
                        .on_submit_fn(|ctx| ctx.dismiss_top_overlay());
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
            .background(SurfaceRole::Raised)
            .border_color(BorderRole::Default)
            .bind_border_width(menu_style.popup_border_width)
            .corner_radius(CornerRadius::uniform(menu_style.popup_corner_radius));
        let bg_id = ctx.add(bg);

        let zstack = ZStack::new().add_child(bg_id).add_child(content_id);
        let root_id = ctx.add(zstack);
        self.root_child_id = Some(root_id);

        // Panel-level key handler. Events bubble up from the focused
        // descendant; in practice that's the search `TextInputField`
        // in searchable mode (non-searchable combos keep focus on the
        // trigger and navigate there). Handles:
        //
        // - Tab / Shift+Tab: close the popup. `dismiss_top_overlay`
        //   restores focus to whatever held it before the overlay
        //   opened (the combo trigger), so the user's next Tab walks
        //   the main focus order from there.
        // - ArrowDown / ArrowUp / Home / End: navigate the filtered
        //   item list while the search field retains focus, so the
        //   user can type a query then arrow through the matches
        //   without losing the cursor.
        // - Enter: confirm the current selection and close. The
        //   `selected` signal was already updated by the arrow keys;
        //   the item's own tap handler would duplicate that, so here
        //   we just dismiss.
        //
        // Returning `Handled` suppresses both the framework's default
        // focus cycle (Tab) and the `TextInputField`'s downstream key
        // handling (arrows would otherwise fall through as printable-
        // character candidates and be rejected as non-text).
        let source_for_nav = self.source.clone();
        let selected_for_nav = self.selected.clone();
        let item_label_for_nav = self.item_label.clone();
        #[cfg(feature = "rich-text")]
        let search_query_for_nav = self.search_query.clone();
        #[cfg(feature = "rich-text")]
        let filter_for_nav = self.filter.clone();
        let panel_handlers = HandlerSet::new().on_key(move |event, ctx| {
            // `TextInputField` consumes `Enter`, `Home`, and `End` for
            // its own cursor semantics and never lets them bubble — so
            // Home/End naturally move the caret inside the search
            // string (expected text-field behavior), and Enter is
            // routed through `TextInput::on_submit(..)` set below. Only
            // `ArrowDown`/`ArrowUp` fall through as unhandled from the
            // text field and reach this handler; we use them to walk
            // the filtered item list.
            let nav_key = match event {
                WidgetEvent::KeyDown {
                    key: Key::Tab, ..
                } => {
                    ctx.dismiss_top_overlay();
                    return EventResponse::Handled;
                }
                WidgetEvent::KeyDown {
                    key: k @ (Key::ArrowDown | Key::ArrowUp),
                    ..
                } => *k,
                _ => return EventResponse::Ignored,
            };

            // Compute the visible-index list under the current filter.
            // Mirrors `FilteredItemList::build` — kept in sync manually
            // because the panel's key handler needs to navigate the
            // same filtered subset without reaching into the child's
            // internal state.
            let total = source_for_nav.len();
            let filtered: Vec<usize> = {
                #[cfg(feature = "rich-text")]
                {
                    if let Some(query) = &search_query_for_nav {
                        let q = query.get();
                        if q.is_empty() {
                            (0..total).collect()
                        } else {
                            let q_lower = q.to_lowercase();
                            (0..total)
                                .filter(|&i| {
                                    source_for_nav
                                        .get(i)
                                        .map(|v| match &filter_for_nav {
                                            Some(f) => f(&q, &v),
                                            None => (item_label_for_nav)(&v)
                                                .to_lowercase()
                                                .contains(&q_lower),
                                        })
                                        .unwrap_or(false)
                                })
                                .collect()
                        }
                    } else {
                        (0..total).collect()
                    }
                }
                #[cfg(not(feature = "rich-text"))]
                {
                    (0..total).collect()
                }
            };
            let n = filtered.len();
            if n == 0 {
                return EventResponse::Handled;
            }

            // Find the currently-selected value's position within the
            // filtered list. A selection that has been filtered out
            // counts as no-selection for navigation purposes.
            let current = selected_for_nav.get();
            let current_in_filtered = current.as_ref().and_then(|v| {
                filtered
                    .iter()
                    .position(|&i| source_for_nav.get(i).as_ref() == Some(v))
            });

            let next_idx = match nav_key {
                Key::ArrowDown => match current_in_filtered {
                    None => 0,
                    Some(i) => (i + 1) % n,
                },
                Key::ArrowUp => match current_in_filtered {
                    None | Some(0) => n - 1,
                    Some(i) => i - 1,
                },
                _ => return EventResponse::Handled,
            };
            if let Some(v) = source_for_nav.get(filtered[next_idx]) {
                selected_for_nav.set(Some(v));
            }
            EventResponse::Handled
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
