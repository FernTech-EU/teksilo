<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# FocusScope

`FocusScope` — a layout-transparent wrapper that declares a **traversal
boundary** for Tab / Shift+Tab focus cycling.

Descendants' `tab_index` values are scoped to the nearest enclosing
`FocusScope`: two sibling scopes that both number their children `1, 2, 3`
never interleave — each scope is an independent, ordered unit within its
parent. The `TraversalScopePolicy` controls what Tab does at the scope's
ends:

- `Continue` — Tab flows *out* of the
  scope into the enclosing scope's next member (grouping only). Use for
  logical regions in a continuous Tab order, e.g. dock panels.
- `Cycle` — Tab *wraps* within the scope and
  never leaves via keyboard. Use for modal dialogs.

```ignore
// A modal dialog whose Tab order is confined to its own content:
FocusScope::new(TraversalScopePolicy::Cycle).child(dialog_body)
```

**Do not `Cycle`-wrap a popover, menu or dropdown panel.** Those are non-modal,
and the framework dismisses a non-modal overlay when keyboard focus leaves it —
which is what their ARIA patterns (Disclosure, Menu) ask for, and what keeps an
open panel from sitting over the focus ring that left it (WCAG 2.2 SC 2.4.11,
Focus Not Obscured). Trapping focus inside one prevents that dismissal from ever
firing. A centered modal needs no wrapper at all: `cycle_focus` already roots
traversal at the topmost centered overlay's content.

## Layout & accessibility

`FocusScope` imposes no layout — it reports its child's natural size and
places the child at its own bounds (like `Fade`). It is a
structural boundary, not an AT element: the wrapped child owns its own
accessibility semantics. The scope node is never itself a Tab stop
(`BuildContext::set_traversal_scope` forces it non-focusable).

## Builder methods at a glance

`child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/focus_scope/index.html)

## `pub struct FocusScope`

Wraps a child subtree and declares it a Tab traversal scope. See the
`module documentation` for semantics.

```rust
pub struct FocusScope { /* fields */ }
```

### Methods

#### `pub fn new(policy: TraversalScopePolicy) -> Self`

Create a traversal scope with the given boundary `policy`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Inline child widget (deferred insertion — the form `teksu!` lowers to).

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Pre-registered child by `WidgetId`.
