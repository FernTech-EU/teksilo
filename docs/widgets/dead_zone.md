<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DeadZone

`DeadZone` — a gesture **dead zone** wrapper.

## Builder methods at a glance

`child`, `child_id`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/primitives/dead_zone/index.html)

## `pub struct DeadZone`

A layout-transparent wrapper whose subtree is a **gesture dead zone**: a
pointer press inside it never arms a drag/swipe recognizer on any ancestor.

Wrap interactive controls (buttons, a `⋮` options menu, a slider) that sit
**inside a draggable / swipeable container** — a dock-panel header, a card, a
list row, a scene item — so clicking them, *even with the few pixels of
pointer jitter a real click carries*, can never start the ancestor's drag.
The container's own drag still works everywhere outside the dead zone. This
is the framework counterpart of Electron's `-webkit-app-region: no-drag`.

It is robust **structurally**, not by a timing-dependent gesture race: it
sets the node-level `gesture_dead_zone`
flag, which the framework's drag-arming honours by refusing to arm any
ancestor above this node. (It also carries a no-op tap/drag so a press on the
dead zone's own bare area — a gap between controls — is absorbed too.)

```ignore
// A draggable dock header whose action buttons don't drag the panel:
HStack::new()
    .child(title)
    .child(DeadZone::new().child(
        HStack::new()
            .child(IconButton::new(new_icon).on_activate_fn(..))
            .child(options_button),
    ))
```

Layout-transparent: it reports its child's size and fills the child to its
own bounds, so dropping it in is size-neutral.

```rust
pub struct DeadZone { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

A new, empty dead zone. Attach content with `child` or
`child_id`.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Wrap an inline widget.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Wrap a pre-registered widget by id.
