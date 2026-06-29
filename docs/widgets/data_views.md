<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# ActivateOn

Shared substrate for the data views' source-owned drag-and-drop + lazy
loading.

Centralizes the vocabulary the four data views (`ListView` / `TreeView` /
`TableView` / `TreeTableView`) share, so DnD validation (`can_accept`) and
the lazy placeholder are wired one way everywhere:

- `RowDrag` — the non-generic intra-app drag payload a row emits. The
  receiving source distinguishes its OWN reorder (matching `source_view_id`)
  from a foreign drop, and translates `source_index` → its own key via
  `key_at`, so the source's `Key` type never leaks into the view.
- `DropIndicator` — what `paint` renders; `allowed == false` is the
  pre-commit forbidden affordance.
- `flat_insertion_target` — maps a flat insertion index to the
  `(target, position)` pair `can_accept` / `accept_drop` expect.
- `default_placeholder` — the skeleton for a `Loading` row.

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/index.html)

## `pub enum ActivateOn`

How a data-view row/tile is *activated* (opened/committed) by pointer —
distinct from *selection*, which also moves on arrow-key navigation. Mirrors
the platform split other toolkits expose (Qt
`SH_ItemView_ActivateItemOnSingleClick`, GTK `activate-on-single-click`).
Enter/Space always activates regardless of this mode.

Pass to `ListView::activate_on`, `TreeView::activate_on`, etc.

```rust
pub enum ActivateOn { /* variants */ }
```

### Variants

- **`SingleClick`** — One primary click activates the row (KDE / web / Scrivener convention). Selection and activation happen on the same click.
- **`DoubleClick`** — A double primary click activates the row; the first click only selects it (Finder / Explorer / Qt and GTK default). This is the `Default`.
