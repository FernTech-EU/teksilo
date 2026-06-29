<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DropTarget

`DropTarget` — a transparent wrapping drop container.

Where `DropZone` is a *standalone* "drop files
here" placeholder with its own label / icon / Browse button, `DropTarget` is
a *wrapping* container: it turns any existing widget subtree into a drop
target without replacing its visual identity. The wrapped child fills the
bounds and is always visible; the widget adds a reactive highlight border +
tint while a drag hovers and, if a hint slot is set, fades in a centered
popup card ("Drop your image here").

It reacts to **both** internal drags (typed `DragPayload`) and external
(OS) drops (files / text / URIs), through the framework's normal drag
pipeline (`on_drag_hover` / `on_drag_leave` / `on_drop`).

```ignore
// Wrap a panel; accept image files; show a hint while hovering.
DropTarget::new()
    .child(my_panel)
    .hint(TextWidget::new(lit!("Drop your image here")))
    .accept_external_extensions(["png", "jpg", "jpeg"])
    .on_drop(|payload, _pos, _ctx| { import(payload.files()); true });

// Typed internal drag — recovers the value even after an OS round-trip
// or across windows (the framework's typed re-entry).
DropTarget::new()
    .child(project_card)
    .on_drop_typed::<ProjectRef>(|project, _pos, ctx| {
        ctx.send_intent(AppIntent::Link(project));
        true
    });
```

# Styling

The highlight overlay + popup chrome is a Tier-3 `DropTargetStyle`; the
default `RecipeDropTargetStyle`
tracks the interaction state. Override per-call with `DropTarget::style` or
theme-wide via `theme.style_slots.drop_target`.

# Accessibility

The wrapper is a `Role::Group`. `Live` is intentionally **not** set on the
group (that would announce every change to the wrapped child); instead the
recipe scopes `Live::Polite` to the hint card so a screen reader announces
the hint *appearing*. The hint is gated by `visible_when`, so it leaves the
AT tree entirely while idle.

## Keyboard accessibility is the caller's responsibility

An OS drag cannot be initiated from the keyboard, and — unlike
`DropZone`, which ships a keyboard-operable
**Browse…** button as its WCAG 2.1.1 equivalent — `DropTarget` adds **no**
keyboard affordance of its own. That is by design: `DropTarget` *wraps*
existing content that is expected to already offer a keyboard path to the
same outcome (e.g. a card you can drop a project onto *or* open with a
context-menu "Link…" command). The drop is an **enhancement**, not the sole
path.

If you use `DropTarget` for an action that has *no* other affordance, you
must add a keyboard equivalent yourself (a button, menu item, or shortcut) —
otherwise the action is unreachable for keyboard-only users, and entirely
unavailable on platforms with no external-DnD backend (e.g. X11, where OS
drag-and-drop is a no-op). `DropZone` is the better choice when the drop
*is* the primary action.

## Builder methods at a glance

`child`, `child_id`, `hint`, `hint_id`, `accept_any`, `accept_external`, `accept_external_files`, `accept_external_text`, `accept_external_extensions`, `accept_typed`, `accept_when`, `bind_is_targeted`, `bind_drag_state`, `on_drop`, `on_drop_typed`, `on_drag_leave`, `variant`, `style`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/drop_target/index.html)

## `pub struct DropTarget`

A transparent container that turns its child into a drop target. See the
module docs.

```rust
pub struct DropTarget { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

A drop target with no child yet — call `Self::child` (required).

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

The wrapped content — fills the bounds and is always visible.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

The wrapped content by pre-registered `WidgetId`.

#### `pub fn hint(mut self, widget: impl Widget + 'static) -> Self`

Widget shown centered inside a popup card while a drag with an accepted
payload hovers. Simple use: `TextWidget::new(lit!("Drop here"))`.

#### `pub fn hint_id(mut self, id: WidgetId) -> Self`

Hint content by pre-registered `WidgetId`.

#### `pub fn accept_any(mut self) -> Self`

Accept any payload (internal or external). Explicit form of the default.

#### `pub fn accept_external(mut self) -> Self`

Accept any external (OS) drop, regardless of content.

#### `pub fn accept_external_files(mut self) -> Self`

Accept external drops that carry at least one file. Optimistic at hover
on Wayland (where the file bytes only arrive at drop) if the source
advertises a `text/uri-list`.

#### `pub fn accept_external_text(mut self) -> Self`

Accept external text drops. Optimistic at hover on Wayland if the source
advertises a text format.

#### `pub fn accept_external_extensions<I, S>(mut self, extensions: I) -> Self where I: IntoIterator<Item = S>, S: AsRef<str>,`

Accept external file drops whose extension is in `extensions`
(case-insensitive). At hover on Wayland the real check is deferred to
drop (no file bytes yet); it is optimistic if a `text/uri-list` is
advertised.

#### `pub fn accept_typed<T: 'static>(mut self) -> Self`

Accept internal drags whose payload carries a value of type `T`.
Ergonomic companion to `Self::on_drop_typed`.

#### `pub fn accept_when(mut self, f: impl Fn(&DragPayload) -> bool + 'static) -> Self`

Custom predicate — full control over payload inspection.

#### `pub fn bind_is_targeted(mut self, signal: Signal<bool>) -> Self`

The widget writes `true` while a drag with an *accepted* payload is over
the target, `false` otherwise — SwiftUI's `isTargeted` pattern. Drive
custom visuals off this signal.

#### `pub fn bind_drag_state(mut self, signal: Signal<DropTargetDragState>) -> Self`

Full three-state version of `Self::bind_is_targeted`.

#### `pub fn on_drop( mut self, f: impl FnMut(DragPayload, Point, &mut EventContext) -> bool + 'static, ) -> Self`

Handle a drop. Return `true` to accept, `false` to reject. Invoked only
when the accept filter passes.

#### `pub fn on_drop_typed<T: 'static>( mut self, mut f: impl FnMut(T, Point, &mut EventContext) -> bool + 'static, ) -> Self`

Ergonomic typed drop: implicitly sets `accept_typed::<T>()` and extracts
the typed value before invoking `f`. Last-call-wins with `Self::on_drop`.

#### `pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self`

Called when a drag leaves the target (pointer exit, drop completion, or
cancel).

#### `pub fn variant(mut self, variant: DropTargetVariant) -> Self`

Visual prominence of the hover indicator.

#### `pub fn style(mut self, style: impl DropTargetStyle) -> Self`

Per-call style override (Tier-3). Wins over the theme slot and the
default recipe.
