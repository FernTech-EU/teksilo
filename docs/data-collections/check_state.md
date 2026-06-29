<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# CheckState

`CheckState` — tri-state checkbox value shared by the data layer and widgets.

Represents the three visual states of a checkbox: unchecked, checked, and
indeterminate (partial — some but not all descendants are checked). Lives in
`bastyde-data` rather than `bastyde-widgets` so that `crate::TreeCheckedModel`
can produce `Signal<CheckState>` values without inverting the dependency graph.

`From<bool>` converts a plain two-state boolean (e.g. from a filter predicate)
into `Unchecked` or `Checked`, making it easy to bridge non-tristate sources.

```rust
# use bastyde_data::CheckState;
let state = CheckState::Indeterminate;
assert!(state.is_filled());
assert_eq!(state.next_tristate(), CheckState::Unchecked);
assert_eq!(CheckState::from(true), CheckState::Checked);
```

## Builder methods at a glance

`is_filled`, `next_tristate`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_data/check_state/index.html)

## `pub enum CheckState`

```rust
pub enum CheckState { /* variants */ }
```

### Variants

- **`Unchecked`** — The checkbox is unchecked (no fill, no mark).
- **`Checked`** — The checkbox is fully checked (filled with a check mark).
- **`Indeterminate`** — Some but not all descendants are checked; shown as a dash or partial fill.

### Methods

#### `pub fn is_filled(self) -> bool`

Whether the box shows a filled background (checked or indeterminate).

#### `pub fn next_tristate(self) -> Self`

Cycle to the next state: Unchecked → Checked → Indeterminate → Unchecked.
