<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CheckState

`CheckState` — tri-state checkbox value shared by the data layer and widgets.

## Public functions

### `CheckState`

| Returns | Function |
| ---: | :--- |
| | **Builder methods** |
| `Self` | [`next_tristate()`](#checkstate-next_tristate) |
| | **Methods** |
| `bool` | [`is_filled()`](#checkstate-is_filled) |

## Detailed description

Represents the three visual states of a checkbox: unchecked, checked, and
indeterminate (partial — some but not all descendants are checked). Lives in
`teksilo-data` rather than `teksilo-widgets` so that `crate::TreeCheckedModel`
can produce `Signal<CheckState>` values without inverting the dependency graph.

`From<bool>` converts a plain two-state boolean (e.g. from a filter predicate)
into `Unchecked` or `Checked`, making it easy to bridge non-tristate sources.

```rust
# use teksilo_data::CheckState;
let state = CheckState::Indeterminate;
assert!(state.is_filled());
assert_eq!(state.next_tristate(), CheckState::Unchecked);
assert_eq!(CheckState::from(true), CheckState::Checked);
```

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-data/latest/teksilo_data/check_state/index.html)

<a id="checkstate"></a>

## `pub enum CheckState`

```rust
pub enum CheckState { /* variants */ }
```

### Variants

- **`Unchecked`** — The checkbox is unchecked (no fill, no mark).
- **`Checked`** — The checkbox is fully checked (filled with a check mark).
- **`Indeterminate`** — Some but not all descendants are checked; shown as a dash or partial fill.

### Methods

<a id="checkstate-is_filled"></a>

#### `pub fn is_filled(self) -> bool`

Whether the box shows a filled background (checked or indeterminate).

<a id="checkstate-next_tristate"></a>

#### `pub fn next_tristate(self) -> Self`

Cycle to the next state: Unchecked → Checked → Indeterminate → Unchecked.
