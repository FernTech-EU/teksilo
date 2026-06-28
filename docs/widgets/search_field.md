<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SearchField

SearchField — a [`TextInput`] preset
configured for search workflows: leading magnifier glyph, default-on
clear-X, and an optional anchored suggestions popover with keyboard
navigation and the ARIA combobox-with-listbox accessibility pattern.
The popover is shown via `OverlayRequest` so it floats above sibling
content and escapes ancestor clipping (same pattern as `ComboBox`).

```ignore
let query = ctx.signal(String::new());
SearchField::new(query.clone())
    .placeholder("Search documents")
    .with_suggestions(|prefix| {
        FRUITS.iter()
            .filter(|f| f.to_lowercase().starts_with(&prefix.to_lowercase()))
            .map(|s| s.to_string())
            .collect()
    })
    .on_select(|value, _ctx| println!("picked: {value}"))
    .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
```

## Design — comparison with searchable `ComboBox`

A searchable `ComboBox` and a `SearchField` are visually similar
but semantically different:

- **ComboBox** is a *value picker* — the bound state is the
  selected item from a known list. The text input is a transient
  filter, embedded inside the dropdown popup; the closed combo
  shows the selected value, not the user's query.
- **SearchField** is a *query input* — the bound state is the
  query string itself. The text input is always visible at the
  top level; suggestions are completion hints, not the source of
  truth. The bound `Signal<String>` keeps whatever the user
  typed, even if no suggestion matches.

The two share the same dropdown-of-options machinery in spirit;
a future refactor could lift a common `OverlayList<T>` primitive
out of both. For now they're separate so each can keep a small
API surface tuned to its semantics.

## Accessibility

The field is `Role::SearchInput` with `HasPopup::Listbox` and
`AutoComplete::List`. When the popup is open it advertises
`set_expanded(true)` and `set_controls(listbox_id)` (mapped to
`accesskit::NodeId` via `widget_id_to_node_id`). Each row is
`Role::ListBoxOption` with `set_selected(is_highlighted)`,
`set_position_in_set(idx + 1)`, and `set_size_of_set(total)` so
screen readers can announce "Apple, 1 of 5".

## Builder methods at a glance

`style`, `placeholder`, `label`, `enabled`, `on_submit_fn`, `with_suggestions`, `max_suggestions`, `min_chars`, `on_select`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/search_field/index.html)

## `pub struct SearchField`

A search input with optional inline suggestions popup.

```rust
pub struct SearchField { /* fields */ }
```

### Methods

#### `pub fn new(text: Signal<String>) -> Self`

Create a search field bound to `text`, the reactive query string.

#### `pub fn style(mut self, style: impl bastyde_core::styles::SearchFieldStyle) -> Self`

Per-call SearchFieldStyle override.

#### `pub fn placeholder(mut self, text: impl Into<LocalizedString>) -> Self`

Set the placeholder text shown when the query is empty.

#### `pub fn label(mut self, label: impl Into<LocalizedString>) -> Self`

Set an accessible label for the field (announced by screen readers, not visually shown).

#### `pub fn enabled(mut self, on: bool) -> Self`

Set the initial enabled state. Forwarded to the arena at build time.

#### `pub fn on_submit_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Install a callback invoked when the user presses Enter (or activates the search action).

#### `pub fn with_suggestions(mut self, f: impl Fn(&str) -> Vec<String> + 'static) -> Self`

Provider that returns suggestions for the current query string.
When set, the popup appears below the field as soon as the
user types at least [`Self::min_chars`] characters and the
provider returns a non-empty list.

#### `pub fn max_suggestions(mut self, n: usize) -> Self`

Cap the number of suggestions shown in the popup (default 8, minimum 1).

#### `pub fn min_chars(mut self, n: usize) -> Self`

Minimum number of characters the user must type before suggestions appear (default 1).

#### `pub fn on_select(mut self, f: impl Fn(&str, &mut EventContext) + 'static) -> Self`

Install a callback invoked when the user picks a suggestion (tap, Enter, or Space).
