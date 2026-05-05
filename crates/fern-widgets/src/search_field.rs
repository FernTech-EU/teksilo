//! SearchField — a [`TextInput`](crate::text_input::TextInput) preset
//! configured for search workflows: leading magnifier glyph, default-on
//! clear-X, optional suggestion popup with keyboard navigation, and the
//! ARIA combobox-with-listbox accessibility pattern.
//!
//! ```ignore
//! let query = ctx.signal(String::new());
//! SearchField::new(query.clone())
//!     .placeholder("Search documents")
//!     .with_suggestions(|prefix| {
//!         CITIES.iter()
//!             .filter(|c| c.to_lowercase().starts_with(&prefix.to_lowercase()))
//!             .map(|s| s.to_string())
//!             .collect()
//!     })
//!     .on_select(|value, _ctx| println!("picked: {value}"))
//!     .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
//! ```
//!
//! ## Accessibility
//!
//! The field is `Role::SearchInput` with `HasPopup::Listbox`. When the
//! popup is open it advertises `set_expanded(true)` and
//! `set_controls(listbox_id)`; the highlighted suggestion is published
//! via `set_active_descendant(option_id)` so screen readers can read
//! out the currently-focused option without focus actually leaving the
//! input. The popup itself is `Role::ListBox`; each row is
//! `Role::ListBoxOption` with `set_pos_in_set` / `set_size_of_set` /
//! `set_selected`.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::signal::Signal;
use fern_core::widget::{CursorIcon, EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_builder::{HandlerSet, WidgetBuilder};
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, HAlignment, SurfaceRole, TextRole, TextStyleRole};

use crate::built_in_button::BuiltInIcons;
use crate::primitives::{
    Center, FixedSize, MinSize, Padding, RectWidget, TextWidget, VStack, ZStack,
};
use crate::text_input::TextInput;

const SEARCH_GLYPH_SIZE: f32 = 14.0;
/// Reserved width for the magnifier slot — pushes the icon center
/// inward so it doesn't sit flush against the field's leading edge.
const SEARCH_GLYPH_SLOT_WIDTH: f32 = 22.0;
/// Extra dead space between the magnifier slot and the text column.
const SEARCH_GLYPH_TRAILING_GAP: f32 = 2.0;
/// Default cap on the number of suggestions rendered in the popup.
const DEFAULT_MAX_SUGGESTIONS: usize = 8;
const SUGGESTION_ROW_HEIGHT: f32 = 26.0;
const SUGGESTION_ROW_PADDING_X: f32 = 10.0;

type SuggestionProvider = Rc<dyn Fn(&str) -> Vec<String>>;
type OnSelect = Rc<dyn Fn(&str, &mut EventContext)>;
type OnSubmit = Rc<dyn Fn(&mut EventContext)>;

/// Build the leading-slot icon: a magnifier glyph centered inside a
/// fixed-width "slot" wider than the icon itself, so it doesn't sit
/// flush against the field's leading edge.
fn search_glyph() -> impl Widget + 'static {
    let icon = (BuiltInIcons::global().search)()
        .icon_size(SEARCH_GLYPH_SIZE)
        .color(TextRole::Secondary);
    FixedSize::new()
        .bind_width(SEARCH_GLYPH_SLOT_WIDTH)
        .child(Center::new().child(icon))
}

/// A search input with optional suggestion popup.
pub struct SearchField {
    text: Signal<String>,
    placeholder: Option<String>,
    label: Option<String>,
    enabled: bool,
    suggestion_provider: Option<SuggestionProvider>,
    max_suggestions: usize,
    min_chars: usize,
    on_select: Option<OnSelect>,
    on_submit: Option<OnSubmit>,
    /// Build state — populated in `build()`.
    root_child_id: Option<WidgetId>,
    /// Slot the SuggestionPanel writes its inner ListBox WidgetId into,
    /// so SearchField's `accessibility()` can publish `set_controls`
    /// pointing at it (ARIA `aria-controls`).
    listbox_id_slot: Rc<Cell<Option<WidgetId>>>,
    open: Signal<bool>,
}

impl SearchField {
    /// Construct a `SearchField` bound to `text`. Placeholder defaults
    /// to the localized "Search" string; override with [`Self::placeholder`].
    pub fn new(text: Signal<String>) -> Self {
        Self {
            text,
            placeholder: None,
            label: None,
            enabled: true,
            suggestion_provider: None,
            max_suggestions: DEFAULT_MAX_SUGGESTIONS,
            min_chars: 1,
            on_select: None,
            on_submit: None,
            root_child_id: None,
            listbox_id_slot: Rc::new(Cell::new(None)),
            open: Signal::new(false),
        }
    }

    /// Override the placeholder text shown when the field is empty.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Accessible name for the search field.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Disable / re-enable the field.
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// Closure invoked on Enter — typical search-trigger hook. Fires
    /// for "raw" Enter (no suggestion highlighted). When a suggestion
    /// is highlighted, [`Self::on_select`] fires instead.
    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// Provider that returns suggestions for the current query string.
    /// When set, the search field shows a popup of matching entries
    /// once the user types at least [`Self::min_chars`] characters.
    /// The provider is called on every text change — keep it cheap or
    /// memoize externally.
    pub fn with_suggestions(mut self, f: impl Fn(&str) -> Vec<String> + 'static) -> Self {
        self.suggestion_provider = Some(Rc::new(f));
        self
    }

    /// Cap on the number of suggestions rendered in the popup. Default 8.
    pub fn max_suggestions(mut self, n: usize) -> Self {
        self.max_suggestions = n.max(1);
        self
    }

    /// Minimum number of characters required before suggestions are
    /// shown. Default 1; set to 0 to show suggestions immediately on
    /// focus.
    pub fn min_chars(mut self, n: usize) -> Self {
        self.min_chars = n;
        self
    }

    /// Closure invoked when the user picks a suggestion (Enter on a
    /// highlighted row, or click). The bound text signal is updated
    /// with the selection before this callback fires.
    pub fn on_select(mut self, f: impl Fn(&str, &mut EventContext) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Drop down to a plain [`TextInput`] preset for callers that
    /// need options the wrapper doesn't surface. The returned input
    /// already has the magnifier in the leading slot and the clear
    /// button enabled — but no suggestion popup.
    pub fn into_input(self) -> TextInput {
        self.build_text_input()
    }

    fn build_text_input(&self) -> TextInput {
        let placeholder = self.placeholder.clone().unwrap_or_else(|| {
            fern_i18n::tr_widget!(a11y_builtin_search()).resolve_now()
        });
        let mut input = TextInput::new(self.text.clone())
            .placeholder(placeholder)
            .show_clear_button(true)
            .leading_slot(search_glyph())
            .enabled(self.enabled);
        if let Some(label) = &self.label {
            input = input.label(label.clone());
        }
        input
    }
}

impl std::fmt::Debug for SearchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchField")
            .field("placeholder", &self.placeholder)
            .field("max_suggestions", &self.max_suggestions)
            .field("min_chars", &self.min_chars)
            .finish_non_exhaustive()
    }
}

impl Widget for SearchField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Reset open state on rebuild.
        self.open.set(false);

        // Reactive suggestions list + highlighted index.
        let suggestions: Signal<Vec<String>> = ctx.signal(Vec::new());
        let highlighted: Signal<Option<usize>> = ctx.signal::<Option<usize>>(None);

        // Build the inner TextInput. We capture submit + key behavior
        // via the host TextInput's hooks; navigation keys (Arrow Up /
        // Down, Escape, Enter when popup open) are intercepted at the
        // SearchField root via `on_key_preview` so they never reach
        // the TextInput's caret machinery.
        let on_submit = self.on_submit.clone();
        let on_select = self.on_select.clone();
        let text_signal = self.text.clone();
        let highlighted_for_submit = highlighted.clone();
        let suggestions_for_submit = suggestions.clone();
        let open_for_submit = self.open.clone();

        let mut input = self.build_text_input();
        input = input.on_submit_fn(move |ctx| {
            let idx = highlighted_for_submit.get();
            let list = suggestions_for_submit.get();
            if let Some(i) = idx {
                if let Some(value) = list.get(i).cloned() {
                    text_signal.set(value.clone());
                    if let Some(handler) = &on_select {
                        handler(&value, ctx);
                    }
                    open_for_submit.set(false);
                    return;
                }
            }
            if let Some(handler) = &on_submit {
                handler(ctx);
            }
            open_for_submit.set(false);
        });

        let input_id = ctx.add(input);
        let self_id = ctx.self_id();

        // Pre-create the suggestions panel (dormant until opened). It
        // binds to the suggestions signal at `BindingLevel::Rebuild`
        // so its row list refreshes whenever the suggestions Vec
        // changes — same pattern as `Repeater`.
        let panel = SuggestionPanel {
            text: self.text.clone(),
            suggestions: suggestions.clone(),
            highlighted: highlighted.clone(),
            on_select: self.on_select.clone(),
            open_signal: self.open.clone(),
            listbox_id_slot: self.listbox_id_slot.clone(),
            root_child_id: None,
        };
        let panel_id = ctx.add(panel);
        ctx.set_dormant(panel_id);

        // Recompute suggestions on every text change. Effects can't
        // open / dismiss the overlay (no EventContext), so the open /
        // close decision happens in the keyboard / focus handlers
        // below; here we only mutate the suggestions Vec.
        if let Some(provider) = self.suggestion_provider.clone() {
            let suggestions = suggestions.clone();
            let highlighted = highlighted.clone();
            let max = self.max_suggestions;
            let min_chars = self.min_chars;
            ctx.effect(&self.text, move |text| {
                let len = text.chars().count();
                if len < min_chars {
                    suggestions.set(Vec::new());
                    highlighted.set(None);
                    return;
                }
                let mut matches = provider(text);
                if matches.len() > max {
                    matches.truncate(max);
                }
                suggestions.set(matches);
                highlighted.set(None);
            });
        }

        // Compose the visible root: a thin transparent container around
        // the TextInput. The panel sits at arena root (dormant) and is
        // surfaced as an overlay on demand.
        let visible_root = ctx.add(MinSize::new(0.0, 0.0).child_id(input_id));
        self.root_child_id = Some(visible_root);

        // Attached handlers — preview keys before TextInput sees them,
        // and observe focus to auto-open on focus-gain when there's
        // already non-empty matching text. The SearchField itself is
        // not focusable; the TextInput is the focus target.
        let suggestions_for_keys = suggestions.clone();
        let highlighted_for_keys = highlighted.clone();
        let open_for_keys = self.open.clone();

        let handlers = HandlerSet::new()
            .on_key_preview(move |event, ctx| -> EventResponse {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowDown, ..
                    } => {
                        let list_len = suggestions_for_keys.get().len();
                        if list_len == 0 {
                            return EventResponse::Ignored;
                        }
                        if !open_for_keys.get() {
                            open_for_keys.set(true);
                            present_overlay(
                                ctx,
                                self_id,
                                panel_id,
                                open_for_keys.clone(),
                            );
                        }
                        let next = match highlighted_for_keys.get() {
                            None => 0,
                            Some(i) if i + 1 >= list_len => 0,
                            Some(i) => i + 1,
                        };
                        highlighted_for_keys.set(Some(next));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown { key: Key::ArrowUp, .. } => {
                        let list_len = suggestions_for_keys.get().len();
                        if list_len == 0 {
                            return EventResponse::Ignored;
                        }
                        if !open_for_keys.get() {
                            open_for_keys.set(true);
                            present_overlay(
                                ctx,
                                self_id,
                                panel_id,
                                open_for_keys.clone(),
                            );
                        }
                        let prev = match highlighted_for_keys.get() {
                            None => list_len - 1,
                            Some(0) => list_len - 1,
                            Some(i) => i - 1,
                        };
                        highlighted_for_keys.set(Some(prev));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::Escape, ..
                    } if open_for_keys.get() => {
                        open_for_keys.set(false);
                        ctx.dismiss_top_overlay();
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            })
            // Auto-show on focus-gain when the field has matching
            // suggestions for the current text. Click-outside
            // dismissal goes through the overlay manager.
            .on_focus({
                let open = self.open.clone();
                let suggestions_for_focus = suggestions.clone();
                move |gained, ctx| {
                    if gained && !open.get() && !suggestions_for_focus.get().is_empty() {
                        open.set(true);
                        present_overlay(ctx, self_id, panel_id, open.clone());
                    } else if !gained && open.get() {
                        open.set(false);
                        ctx.dismiss_top_overlay();
                    }
                }
            });

        ctx.apply_self_handlers(handlers);

        vec![visible_root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        use fern_core::accessibility::widget_id_to_node_id;
        // ARIA combobox-with-listbox surface: the SearchField's a11y
        // node owns the role + popup state; the inner TextInput keeps
        // its caret + value semantics.
        builder.set_role(fern_core::accesskit::Role::SearchInput);
        builder.set_has_popup(fern_core::accesskit::HasPopup::Listbox);
        // `aria-autocomplete=list`: the popup contains a list of
        // candidate completions but does not insert text inline.
        builder.set_auto_complete(fern_core::accesskit::AutoComplete::List);
        if self.open.get() {
            builder.set_expanded(true);
        }
        if let Some(listbox_id) = self.listbox_id_slot.get() {
            // The listbox lives in an overlay (not a descendant of
            // this widget), so `aria-controls` is the right relation.
            builder.push_controlled(widget_id_to_node_id(listbox_id));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

fn present_overlay(
    ctx: &mut EventContext,
    anchor: WidgetId,
    panel_id: WidgetId,
    open_signal: Signal<bool>,
) {
    let dismiss: OverlayDismissCallback = {
        let open = open_signal.clone();
        Rc::new(move || open.set(false))
    };
    ctx.activate(panel_id);
    ctx.show_overlay(OverlayRequest {
        content_id: panel_id,
        anchor,
        placement: OverlayPlacement::BelowPreferred,
        dismiss: DismissBehavior::EscapeOrClickOutside,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: Some(dismiss),
        fade_duration: None,
    });
}

// ── SuggestionPanel — the listbox content rendered in the overlay ─

struct SuggestionPanel {
    text: Signal<String>,
    suggestions: Signal<Vec<String>>,
    highlighted: Signal<Option<usize>>,
    on_select: Option<OnSelect>,
    open_signal: Signal<bool>,
    /// Stash slot the SearchField uses to learn this panel's listbox
    /// child id, so it can populate `set_controls(listbox_id)` on the
    /// SearchField's a11y node.
    listbox_id_slot: Rc<Cell<Option<WidgetId>>>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for SuggestionPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuggestionPanel").finish_non_exhaustive()
    }
}

impl Widget for SuggestionPanel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use fern_core::binding::BindingLevel;
        // Bind the panel to the suggestions signal at `Rebuild` so it
        // re-runs `build()` whenever the Vec changes — same pattern
        // as `Repeater`. Without this the panel materializes once
        // with the initial empty Vec and never refreshes.
        self.suggestions
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let suggestions = self.suggestions.clone();
        let highlighted = self.highlighted.clone();
        let on_select = self.on_select.clone();
        let text = self.text.clone();
        let open = self.open_signal.clone();

        let list = suggestions.get();
        let total = list.len();
        let mut column = VStack::new().alignment(HAlignment::Leading);
        for (idx, value) in list.into_iter().enumerate() {
            let bg_role = highlighted.map(move |h| match h {
                Some(i) if *i == idx => SurfaceRole::Hover,
                _ => SurfaceRole::Transparent,
            });
            let bg = ctx.add(
                RectWidget::new()
                    .bind_background(bg_role)
                    .corner_radius(CornerRadius::uniform(2.0)),
            );
            let label_id = ctx.add(
                TextWidget::new_literal(&value)
                    .style(TextStyleRole::Body)
                    .single_line(),
            );
            let inner_padded = ctx.add(
                Padding::symmetric(4.0, SUGGESTION_ROW_PADDING_X).child_id(label_id),
            );
            let row_z = ctx.add(ZStack::new().add_child(bg).add_child(inner_padded));

            let value_for_tap = value.clone();
            let on_select_for_tap = on_select.clone();
            let text_for_tap = text.clone();
            let highlighted_for_hover = highlighted.clone();
            let open_for_tap = open.clone();
            let row = ctx.add(
                SuggestionRow {
                    label: value.clone(),
                    index: idx,
                    total,
                    selected_signal: highlighted.clone(),
                    inner_id: row_z,
                }
                .on_tap(move |_pos, ctx| {
                    text_for_tap.set(value_for_tap.clone());
                    if let Some(handler) = &on_select_for_tap {
                        handler(&value_for_tap, ctx);
                    }
                    open_for_tap.set(false);
                    ctx.dismiss_top_overlay();
                })
                .on_hover(move |entered, _| {
                    if entered {
                        highlighted_for_hover.set(Some(idx));
                    }
                })
                .cursor(CursorIcon::Pointer),
            );
            column = column.add_child(row);
        }

        // Listbox surface — themed background + soft border.
        let listbox_inner = ctx.add(column);
        let bg_rect = ctx.add(
            RectWidget::new()
                .background(SurfaceRole::Raised)
                .border_color(BorderRole::Default)
                .border_width(1.0)
                .corner_radius(CornerRadius::uniform(6.0)),
        );
        let padded = ctx.add(Padding::uniform(4.0).child_id(listbox_inner));
        let bordered = ctx.add(ZStack::new().add_child(bg_rect).add_child(padded));

        let listbox = ctx.add(SuggestionListBox {
            inner: bordered,
            count: total,
        });
        // Publish the listbox's WidgetId so SearchField's
        // `accessibility()` can wire `aria-controls`.
        self.listbox_id_slot.set(Some(listbox));

        self.root_child_id = Some(listbox);
        vec![listbox]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        builder.set_role(fern_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ── ListBox a11y wrapper around the styled column ─────────────────

struct SuggestionListBox {
    inner: WidgetId,
    count: usize,
}

impl std::fmt::Debug for SuggestionListBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuggestionListBox").finish_non_exhaustive()
    }
}

impl Widget for SuggestionListBox {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        ctx.child_size(self.inner, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
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
        // ARIA Listbox role; size_of_set goes onto each option, not
        // the container, but writing it here too is harmless.
        builder.set_role(fern_core::accesskit::Role::ListBox);
        let _ = self.count;
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.inner]
    }
}

// ── Per-row a11y wrapper: Role::ListBoxOption with set_selected and
//    pos_in_set / size_of_set. Also bridges tap / hover handlers
//    onto the styled inner ZStack via WidgetBuilder. ─────────────

struct SuggestionRow {
    label: String,
    index: usize,
    total: usize,
    selected_signal: Signal<Option<usize>>,
    inner_id: WidgetId,
}

impl std::fmt::Debug for SuggestionRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuggestionRow")
            .field("label", &self.label)
            .field("index", &self.index)
            .finish()
    }
}

impl Widget for SuggestionRow {
    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let inner = ctx
            .child_size(self.inner_id, proposal)
            .unwrap_or_else(|| proposal.resolve(0.0, SUGGESTION_ROW_HEIGHT));
        // Force a minimum row height so single-line rows don't squish.
        let height = inner.height.max(SUGGESTION_ROW_HEIGHT);
        let width = proposal.width.unwrap_or(inner.width).max(inner.width);
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
        builder.set_role(fern_core::accesskit::Role::ListBoxOption);
        builder.set_name(&self.label);
        let is_selected = self.selected_signal.get() == Some(self.index);
        builder.set_selected(is_selected);
        // pos_in_set / size_of_set are 1-based per ARIA. AccessKit's
        // `set_position_in_set` / `set_size_of_set` accept usize via
        // the inner builder.
        builder.inner_mut().set_position_in_set(self.index + 1);
        builder.inner_mut().set_size_of_set(self.total);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.inner_id]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn search_field_builds() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let q = Signal::new(String::new());
        let id = tree.add(SearchField::new(q.clone()).placeholder("Search docs"));
        tree.layout(SizeProposal {
            width: Some(320.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn search_field_a11y_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(
            SearchField::new(Signal::new(String::new())).with_suggestions(|q| {
                if q.is_empty() {
                    Vec::new()
                } else {
                    vec!["alpha".into(), "beta".into()]
                }
            }),
        );
        tree.layout(SizeProposal {
            width: Some(280.0),
            height: None,
        });
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::SearchInput);
        // `has_popup`, `aria_controls`, and `auto_complete` go through
        // the platform AT tree (accesskit) — `AccessibilityInfo` is a
        // framework-internal view that doesn't expose them. The
        // accessibility() implementation above writes them directly to
        // the AccessKit node; the tree-walker test in fern-core covers
        // their round-trip.
    }

    #[test]
    fn suggestions_provider_runs_on_text_change() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let q = Signal::new(String::new());
        let calls: Rc<Cell<usize>> = Rc::new(Cell::new(0));
        let calls_clone = calls.clone();
        tree.add(SearchField::new(q.clone()).with_suggestions(move |s| {
            calls_clone.set(calls_clone.get() + 1);
            if s.is_empty() {
                Vec::new()
            } else {
                vec!["alpha".into()]
            }
        }));
        tree.layout(SizeProposal::exact(300.0, 100.0));
        let baseline = calls.get();
        q.set("a".to_string());
        tree.layout(SizeProposal::exact(300.0, 100.0));
        assert!(
            calls.get() > baseline,
            "provider should fire on text change"
        );
    }
}
