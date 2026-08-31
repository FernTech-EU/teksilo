<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# DropTarget

![DropTarget preview](img/drop_target.png)

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

# Multi-zone drops

Beyond the single whole-bounds target, a `DropTarget` can expose up to five
independently enable-able `DropRegion`s — `Center` / `Top` / `Bottom` /
`Leading` / `Trailing` — each with its own optional hint, and route the drop
by which zone the pointer released over. This is the VS Code-style
"drop on the centre to add, drop on an edge to split" affordance
(`DockingLayout` computes the same five zones by hand). Declare regions with
`DropTarget::region`; the side zones share one `DropTarget::zone_size_factor`
(`0.1..=1.0`, the fraction of the axis each edge strip occupies — `0.2` is the
default fifth, `0.5` bisects) so you size them to the context. Route with
`DropTarget::on_region_drop` (or observe `DropTarget::active_region_signal`).

```ignore
DropTarget::new()
    .child(editor_pane)
    .zone_size_factor(0.25)
    .region(DropRegion::Center,   |z| z.hint(TextWidget::new(lit!("Add as tab"))))
    .region(DropRegion::Leading,  |z| z.hint(TextWidget::new(lit!("Split left"))))
    .region(DropRegion::Trailing, |z| z.hint(TextWidget::new(lit!("Split right"))))
    .on_region_drop(|region, payload, _pos, ctx| { route(region, payload); true });
```

Declaring **any** region switches the target to exactly the declared regions;
declaring none keeps the `Center`-only whole-bounds default (`.hint(w)` is
sugar for `.region(DropRegion::Center, |z| z.hint(w))`). `Leading` / `Trailing`
map to left / right — the framework surfaces no writing direction on the
layout context yet, so RTL mirroring is a follow-up.

Each zone can be **reactively enabled** with `z.enabled(signal)` (default
`true`): a bound `Signal<bool>` disables the zone live — no rebuild — and its
strip then falls through to the next-priority enabled zone (or `Center`, or
rejects). A drop landing in a middle covered by no *enabled* zone is rejected;
`on_region_drop` therefore only ever receives an enabled region.

# Styling

The per-zone highlight overlay + hint chrome is a Tier-3 `DropTargetStyle`;
the default `RecipeDropTargetStyle`
paints the active zone (centre → frame only, so the wrapped content shows
through; an edge strip → translucent fill + accent frame) and a full-bounds
error border on reject. Override per-call with `DropTarget::style` or
theme-wide via `theme.style_slots.drop_target`.

# Accessibility

The wrapper is a `Role::Group`. `Live` is intentionally **not** set on the
group (that would announce every change to the wrapped child); instead the
recipe scopes `Live::Polite` to each hint card so a screen reader announces
the active zone's hint *appearing*. Each hint is gated by `visible_when`, so a
non-active zone's hint leaves the AT tree entirely.

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

`child`, `child_id`, `region`, `zone_size_factor`, `hint`, `hint_id`, `accept_any`, `accept_external`, `accept_external_files`, `accept_external_text`, `accept_external_extensions`, `accept_typed`, `accept_when`, `targeted_signal`, `drag_state_signal`, `active_region_signal`, `on_drop`, `on_drop_typed`, `on_region_drop`, `on_drag_leave`, `variant`, `style`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/drop_target/index.html)

## `pub struct DropRegionSpec`

Per-region configuration for a multi-zone [`DropTarget`]: an optional hint
plus a reactive enabled flag. Kept as a struct so more per-zone knobs can
land without a signature churn.

```rust
pub struct DropRegionSpec { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

An enabled spec with no hint.

#### `pub fn hint(mut self, widget: impl Widget + 'static) -> Self`

Widget shown (centered in this region's rect, inside a popup card) while
a drag with an accepted payload hovers **this** region.

#### `pub fn hint_id(mut self, id: WidgetId) -> Self`

This region's hint content by pre-registered `WidgetId`.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Whether this zone is active — static or signal-bound (default `true`). A
bound `Signal<bool>` enables/disables the zone **live, without a rebuild**:
while disabled the zone stops hit-testing (its area falls through to the
next-priority enabled zone, or `Center`, or rejects), never highlights,
and never shows its hint. The enabled state is resolved on every drag
tick, so a `.set(false)` mid-drag takes effect on the next hover.

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

#### `pub fn region( mut self, region: DropRegion, f: impl FnOnce(DropRegionSpec) -> DropRegionSpec, ) -> Self`

Enable and configure a drop `DropRegion`. Declaring **any** region
switches the target to exactly the declared regions; declaring none
leaves the implicit `Center`-only whole-bounds default. The spec closure
configures the region (currently: an optional hint).

```ignore
DropTarget::new()
    .child(editor)
    .zone_size_factor(0.25)
    .region(DropRegion::Center,   |z| z.hint(TextWidget::new(lit!("Add tab"))))
    .region(DropRegion::Leading,  |z| z.hint(TextWidget::new(lit!("Split left"))))
    .region(DropRegion::Trailing, |z| z.hint(TextWidget::new(lit!("Split right"))))
    .on_region_drop(|region, payload, _pos, ctx| { route(region, payload); true });
```

#### `pub fn zone_size_factor(mut self, factor: f32) -> Self`

The fraction of the axis each **side** zone occupies (clamped to
`0.1..=1.0`). `0.2` is the default fifth; `0.5` bisects. Applies to all
four edge zones in common; `Center` takes the leftover middle.

#### `pub fn hint(mut self, widget: impl Widget + 'static) -> Self`

Widget shown centered inside a popup card while a drag with an accepted
payload hovers. Sugar for `.region(DropRegion::Center, |z| z.hint(w))` —
the classic whole-bounds single-zone case.

#### `pub fn hint_id(mut self, id: WidgetId) -> Self`

Hint content by pre-registered `WidgetId` (Center region).

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

#### `pub fn targeted_signal(mut self, signal: Signal<bool>) -> Self`

The widget writes `true` while a drag with an *accepted* payload is over
the target, `false` otherwise — SwiftUI's `isTargeted` pattern. Drive
custom visuals off this signal.

#### `pub fn drag_state_signal(mut self, signal: Signal<DropTargetDragState>) -> Self`

Full three-state version of `Self::targeted_signal`.

#### `pub fn active_region_signal(mut self, signal: Signal<Option<DropRegion>>) -> Self`

The widget writes which `DropRegion` an *accepted* drag is currently
over (`None` when idle, rejecting, or over a disabled middle). Drive
custom per-zone visuals off this.

#### `pub fn on_drop( mut self, f: impl FnMut(DragPayload, Point, &mut EventContext) -> bool + 'static, ) -> Self`

Handle a drop. Return `true` to accept, `false` to reject. Invoked only
when the accept filter passes.

#### `pub fn on_drop_typed<T: 'static>( mut self, mut f: impl FnMut(T, Point, &mut EventContext) -> bool + 'static, ) -> Self`

Ergonomic typed drop: implicitly sets `accept_typed::<T>()` and extracts
the typed value before invoking `f`. Last-call-wins with `Self::on_drop`.

#### `pub fn on_region_drop( mut self, f: impl FnMut(DropRegion, DragPayload, Point, &mut EventContext) -> bool + 'static, ) -> Self`

Region-aware drop: receives which `DropRegion` the pointer released
over, plus the payload. Last-call-wins with `Self::on_drop` — when set,
it is used instead of the plain `on_drop`. Invoked only when the accept
filter passes; return `true` to accept.

#### `pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self`

Called when a drag leaves the target (pointer exit, drop completion, or
cancel).

#### `pub fn variant(mut self, variant: DropTargetVariant) -> Self`

Visual prominence of the hover indicator.

#### `pub fn style(mut self, style: impl DropTargetStyle) -> Self`

Per-call style override (Tier-3). Wins over the theme slot and the
default recipe.
