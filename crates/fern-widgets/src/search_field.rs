//! SearchField — a [`TextInput`](crate::text_input::TextInput) preset
//! configured for search workflows: leading magnifier glyph, default-on
//! clear-X, and an optional inline suggestions popup with keyboard
//! navigation and the ARIA combobox-with-listbox accessibility pattern.
//!
//! ```ignore
//! let query = ctx.signal(String::new());
//! SearchField::new(query.clone())
//!     .placeholder("Search documents")
//!     .with_suggestions(|prefix| {
//!         FRUITS.iter()
//!             .filter(|f| f.to_lowercase().starts_with(&prefix.to_lowercase()))
//!             .map(|s| s.to_string())
//!             .collect()
//!     })
//!     .on_select(|value, _ctx| println!("picked: {value}"))
//!     .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
//! ```
//!
//! ## Design — comparison with searchable [`ComboBox`](crate::combo_box::ComboBox)
//!
//! A searchable `ComboBox` and a `SearchField` are visually similar
//! but semantically different:
//!
//! - **ComboBox** is a *value picker* — the bound state is the
//!   selected item from a known list. The text input is a transient
//!   filter, embedded inside the dropdown popup; the closed combo
//!   shows the selected value, not the user's query.
//! - **SearchField** is a *query input* — the bound state is the
//!   query string itself. The text input is always visible at the
//!   top level; suggestions are completion hints, not the source of
//!   truth. The bound `Signal<String>` keeps whatever the user
//!   typed, even if no suggestion matches.
//!
//! The two share the same dropdown-of-options machinery in spirit;
//! a future refactor could lift a common `OverlayList<T>` primitive
//! out of both. For now they're separate so each can keep a small
//! API surface tuned to its semantics.
//!
//! ## Accessibility
//!
//! The field is `Role::SearchInput` with `HasPopup::Listbox` and
//! `AutoComplete::List`. When the popup is open it advertises
//! `set_expanded(true)` and `set_controls(listbox_id)` (mapped to
//! `accesskit::NodeId` via `widget_id_to_node_id`). Each row is
//! `Role::ListBoxOption` with `set_selected(is_highlighted)`,
//! `set_position_in_set(idx + 1)`, and `set_size_of_set(total)` so
//! screen readers can announce "Apple, 1 of 5".

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
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
const DEFAULT_MAX_SUGGESTIONS: usize = 8;
const SUGGESTION_ROW_HEIGHT: f32 = 26.0;
const SUGGESTION_ROW_PADDING_X: f32 = 10.0;

type SuggestionProvider = Rc<dyn Fn(&str) -> Vec<String>>;
type OnSelect = Rc<dyn Fn(&str, &mut EventContext)>;
type OnSubmit = Rc<dyn Fn(&mut EventContext)>;

fn search_glyph() -> impl Widget + 'static {
    let icon = (BuiltInIcons::global().search)()
        .icon_size(SEARCH_GLYPH_SIZE)
        .color(TextRole::Secondary);
    FixedSize::new()
        .bind_width(SEARCH_GLYPH_SLOT_WIDTH)
        .child(Center::new().child(icon))
}

/// A search input with optional inline suggestions popup.
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
    /// so SearchField's `accessibility()` can publish `set_controls`.
    listbox_id_slot: Rc<Cell<Option<WidgetId>>>,
    /// Tracks whether any descendant of this SearchField currently has
    /// focus — used to drive popup visibility. The framework writes
    /// this signal when focus enters or leaves the subtree.
    focus_within: Signal<bool>,
    /// User-visible popup-open state — read by `accessibility()`.
    /// `RefCell<Option<...>>` because the derived signal is built
    /// during the first `build()` call and can't be created in `new()`
    /// (the upstream signals don't exist yet).
    is_open: std::cell::RefCell<Option<Signal<bool>>>,
}

impl SearchField {
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
            focus_within: Signal::new(false),
            is_open: std::cell::RefCell::new(None),
        }
    }

    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// Provider that returns suggestions for the current query string.
    /// When set, the popup appears below the field as soon as the
    /// user types at least [`Self::min_chars`] characters and the
    /// provider returns a non-empty list.
    pub fn with_suggestions(mut self, f: impl Fn(&str) -> Vec<String> + 'static) -> Self {
        self.suggestion_provider = Some(Rc::new(f));
        self
    }

    pub fn max_suggestions(mut self, n: usize) -> Self {
        self.max_suggestions = n.max(1);
        self
    }

    pub fn min_chars(mut self, n: usize) -> Self {
        self.min_chars = n;
        self
    }

    pub fn on_select(mut self, f: impl Fn(&str, &mut EventContext) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Drop down to a plain [`TextInput`] preset — no suggestions
    /// popup. Already configured with the magnifier in the leading
    /// slot and the clear button enabled.
    pub fn into_input(self) -> TextInput {
        self.build_text_input()
    }

    fn build_text_input(&self) -> TextInput {
        let placeholder = self
            .placeholder
            .clone()
            .unwrap_or_else(|| fern_i18n::tr_widget!(a11y_builtin_search()).resolve_now());
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
        // ── Reactive state ──────────────────────────────────────────
        // Suggestions list — recomputed on every text change.
        let suggestions: Signal<Vec<String>> = ctx.signal(Vec::new());
        // Currently highlighted row inside the popup. Driven by
        // ArrowUp / ArrowDown and by hover.
        let highlighted: Signal<Option<usize>> = ctx.signal::<Option<usize>>(None);
        // "User pressed Escape" / "User picked a suggestion" flag —
        // suppresses the popup until the user starts typing again or
        // refocuses the field. Reset on focus-gain and on any
        // non-Escape KeyDown.
        let dismissed: Signal<bool> = ctx.signal(false);

        // ── TextInput with submit hook ──────────────────────────────
        let on_submit = self.on_submit.clone();
        let on_select = self.on_select.clone();
        let text_signal = self.text.clone();
        let highlighted_for_submit = highlighted.clone();
        let suggestions_for_submit = suggestions.clone();
        let dismissed_for_submit = dismissed.clone();

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
                    dismissed_for_submit.set(true);
                    return;
                }
            }
            if let Some(handler) = &on_submit {
                handler(ctx);
            }
            dismissed_for_submit.set(true);
        });
        let input_id = ctx.add(input);

        // ── Suggestions provider effect ─────────────────────────────
        // Recomputes the list on every text change. The popup's
        // visibility is driven by a separate derived signal further
        // down, so this effect only mutates `suggestions` /
        // `highlighted`.
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

        // Resetting `dismissed` when focus enters the subtree gives
        // users a clean second pass: pick a suggestion → popup hides;
        // come back to the field → popup re-opens if there's still a
        // matching list.
        let dismissed_for_focus = dismissed.clone();
        ctx.effect(&self.focus_within, move |gained| {
            if *gained {
                dismissed_for_focus.set(false);
            }
        });

        // ── Derived popup-open signal ───────────────────────────────
        // Visible iff focus is in the subtree, suggestions exist, and
        // the user hasn't explicitly dismissed via Escape / select.
        // Three-way zip: (focus, dismissed, suggestions). Each
        // upstream root is registered in the binding registry so the
        // panel's `visible_when` re-evaluates on any change.
        let is_open_derived = self
            .focus_within
            .zip(&dismissed)
            .zip(&suggestions)
            .map(|((focus, dismissed), list)| *focus && !*dismissed && !list.is_empty());
        // Stash the derived signal so `accessibility()` can read its
        // value for `set_expanded`. Cleared and rebuilt on every
        // `build()` call so the upstream signal graph stays fresh.
        *self.is_open.borrow_mut() = Some(is_open_derived.clone());

        // ── Suggestions panel (in-tree sibling, not overlay) ────────
        let panel = SuggestionPanel {
            text: self.text.clone(),
            suggestions: suggestions.clone(),
            highlighted: highlighted.clone(),
            on_select: self.on_select.clone(),
            dismissed: dismissed.clone(),
            listbox_id_slot: self.listbox_id_slot.clone(),
            root_child_id: None,
        };
        let panel_id = ctx.add(panel);
        ctx.visible_when(panel_id, is_open_derived);

        // ── Compose ─────────────────────────────────────────────────
        let column = ctx.add(VStack::new().spacing(2.0).add_child(input_id).add_child(panel_id));
        let visible_root = ctx.add(MinSize::new(0.0, 0.0).child_id(column));
        self.root_child_id = Some(visible_root);

        // ── Handlers ───────────────────────────────────────────────
        let suggestions_for_keys = suggestions.clone();
        let highlighted_for_keys = highlighted.clone();
        let dismissed_for_keys = dismissed.clone();

        let handlers = HandlerSet::new()
            // `focus_within` is the parent-side mirror: framework
            // sets it true when any descendant (the TextInput) has
            // focus. Strict-ancestors-only — the descendant itself
            // still sees its own focus normally.
            .focus_within(self.focus_within.clone())
            .on_key_preview(move |event, _ctx| -> EventResponse {
                match event {
                    WidgetEvent::KeyDown {
                        key: Key::ArrowDown,
                        ..
                    } => {
                        let list_len = suggestions_for_keys.get().len();
                        if list_len == 0 {
                            return EventResponse::Ignored;
                        }
                        // Re-open if user previously dismissed.
                        dismissed_for_keys.set(false);
                        let next = match highlighted_for_keys.get() {
                            None => 0,
                            Some(i) if i + 1 >= list_len => 0,
                            Some(i) => i + 1,
                        };
                        highlighted_for_keys.set(Some(next));
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown {
                        key: Key::ArrowUp, ..
                    } => {
                        let list_len = suggestions_for_keys.get().len();
                        if list_len == 0 {
                            return EventResponse::Ignored;
                        }
                        dismissed_for_keys.set(false);
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
                    } => {
                        dismissed_for_keys.set(true);
                        highlighted_for_keys.set(None);
                        EventResponse::Handled
                    }
                    WidgetEvent::KeyDown { .. } => {
                        // Any other key — character input, Backspace,
                        // Delete, etc — clears the dismissed flag so
                        // the popup can re-appear after the user
                        // resumes typing.
                        dismissed_for_keys.set(false);
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
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
        builder.set_role(fern_core::accesskit::Role::SearchInput);
        builder.set_has_popup(fern_core::accesskit::HasPopup::Listbox);
        builder.set_auto_complete(fern_core::accesskit::AutoComplete::List);
        if let Some(sig) = self.is_open.borrow().as_ref() {
            if sig.get() {
                builder.set_expanded(true);
            }
        }
        if let Some(listbox_id) = self.listbox_id_slot.get() {
            builder.push_controlled(widget_id_to_node_id(listbox_id));
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

// ── SuggestionPanel — the in-tree listbox rendered below the field ─

struct SuggestionPanel {
    text: Signal<String>,
    suggestions: Signal<Vec<String>>,
    highlighted: Signal<Option<usize>>,
    on_select: Option<OnSelect>,
    dismissed: Signal<bool>,
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
        // Bind the panel to `suggestions` at `Rebuild` so its row
        // list refreshes whenever the Vec changes — same pattern
        // `Repeater` uses for ListModel-backed dynamic content.
        self.suggestions
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let suggestions = self.suggestions.clone();
        let highlighted = self.highlighted.clone();
        let on_select = self.on_select.clone();
        let text = self.text.clone();
        let dismissed = self.dismissed.clone();

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
            let dismissed_for_tap = dismissed.clone();
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
                    dismissed_for_tap.set(true);
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

        // Listbox surface — themed background + border.
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

        let listbox = ctx.add(SuggestionListBox { inner: bordered });
        // Publish the listbox WidgetId so SearchField's a11y can wire
        // `aria-controls`.
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
        builder.set_role(fern_core::accesskit::Role::ListBox);
    }

    fn children(&self) -> Vec<WidgetId> {
        vec![self.inner]
    }
}

// ── Per-row a11y wrapper: Role::ListBoxOption with ARIA position
//    metadata. Bridges tap / hover handlers onto the styled inner
//    ZStack via WidgetBuilder. ─────────────────────────────────────

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
