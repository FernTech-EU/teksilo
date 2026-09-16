<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DeadZone

`DeadZone` — a gesture **dead zone** wrapper.

## Touch and pen

The no-op tap and drag pair is **kept**, and it is not redundant with the
`gesture_dead_zone` flag: the flag governs whether an ancestor may enrol this
subtree's press as a member of its own gesture, while the absorbers are what
give the wrapper a gesture arena — and the arena is what stops the bubble for a
press on the wrapper's own bare area, the gap between the controls it protects.
Reviewed under the arbitration package with that conclusion; nothing about it is
pointer-kind-specific.

## Builder methods at a glance

`child`, `child_opt`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/primitives/dead_zone/index.html)

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
`child`.

#### `pub fn child(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

Wrap an inline widget.

#### `pub fn child_opt(self, widget: Option<impl teksilo_core::IntoTeksiChild>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.
