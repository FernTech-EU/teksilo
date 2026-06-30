<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Window-Active Appearance

Serious desktop apps change how a window looks when it loses OS focus: the text
caret stops blinking and disappears, text and list selections desaturate to a
muted grey, and accent-coloured chrome dims. Bastyde does this automatically and
gives apps an opt-in hook for custom content.

This mirrors the modern, accepted pattern across toolkits — a **reactive ambient
flag the view reads declaratively**, plus **theme-driven inactive colours**:
SwiftUI `@Environment(\.appearsActive)` (macOS 15), Jetpack Compose
`LocalWindowInfo.isWindowFocused`, GTK4 `:backdrop`, Qt `QPalette::Inactive`,
WPF `InactiveSelectionHighlightBrush`.

## What "active" means

A window is **active** when it is `focused AND not occluded` — it holds OS
keyboard focus and isn't fully hidden behind another window. This is computed
per window by `bastyde-app` from winit's `Focused` / `Occluded` events and
published as a reactive signal on each window's widget tree.

`window_active` is **distinct from view focus**. A `ListView` can hold keyboard
focus *within* its window while that window is inactive. The vivid selection
shows only when **both** are true (view focused *and* window active); otherwise
the selection is muted. The same muted colour serves both "focus is elsewhere in
this window" and "this window is inactive" — matching macOS's single
"unemphasized" selection colour and GTK's `:backdrop`.

State is **per window**: deactivating one window never affects another (there is
no app-wide fan-out, unlike theme or text-scale).

## Reading it

| Surface | API |
| --- | --- |
| In `build()`, reactive | `ctx.window_active_signal() -> Signal<bool>` (bind at `RepaintOnly`) |
| In `build()`, one-shot | `ctx.window_active() -> bool` |
| In `paint()` | `ctx.window_active: bool` (on `PaintContext`) |
| In an event handler | `ctx.window_active() -> bool` (on `EventContext`) |
| On the tree | `WidgetTree::window_active_signal()` / `is_window_active()` |

It starts `true` — a window must not be born inactive before its first focus
event arrives. A flip triggers a **repaint only** (never a relayout): geometry is
unchanged, so the caret keeps its space and nothing reflows.

## Automatic behaviour (no opt-in)

These are correctness, not features, so they are **on by default**:

- **Accent desaturation (theme-side, covers every control).** When the window is
  inactive, the paint walker swaps in a theme projection
  (`ColorTokens::for_inactive_window`) whose **accent family and focus indicators
  are desaturated toward graphite** —
  the macOS / Qt `QPalette::Inactive` model. Because every themed control
  resolves its accent from the live `ColorTokens` at paint time, this single
  swap greys out *all* of them with **no per-widget code**: the default
  (`Filled`) `Button`, `Toggle`'s on-track, checked `Checkbox` / `RadioButton`,
  the selected `TabBar` tab and `SegmentedControl` segment, `Slider` fills,
  `ProgressBar`, `Badge`, links-as-accent, and focus rings (`BorderRole::Focused`
  / `focus_ring`). It applies to **any** preset that populates these tokens —
  IntUI, Material 3 (where `accent` = M3 *primary*), and future presets — for
  free. Deliberately untouched: selection, status, and text tokens (see below).
- **Caret hiding.** The text caret hides in an inactive window for every caret
  policy, in both text stacks (`RichTextEditor` and every `TextInput` /
  `PasswordField` / `SpinBox` / `SearchField` built on `TextInputField`). It
  returns immediately when the window reactivates and the field still holds
  focus. There is no opt-out — every native toolkit hides the caret here.
- **Selection desaturation.** A selected row/cell/run shows the vivid selection
  only while its view is focused **and** the window is active; otherwise it falls
  back to the muted inactive colour. This is handled **per widget** (not
  theme-side) because it depends on *view* focus, which a theme projection can't
  express. Covered surfaces:

| Widget | Active role / token | Inactive role / token |
| --- | --- | --- |
| `StandardListItem` / `StandardTreeItem` | `SurfaceRole::Selected` | `SurfaceRole::SelectedInactive` |
| `TableView` / `TreeTableView` | `SurfaceRole::Selected` | `SurfaceRole::SelectedInactive` |
| `RichTextEditor` | `editor_selection_bg` | `selection_bg_inactive` |
| `TextInput` family | `selection_bg_active` | `selection_bg_inactive` |

Keyboard focus rings grey out (not hide) in an inactive window, uniformly, via
the theme-side accent projection above — no per-widget check. `MenuList` is
excluded by design — an open menu is always active.

### Custom selection colours stay fixed

If an app sets an explicit selection colour — e.g.
`RichTextEditor::editor(doc).selection_color(my_blue)` — that colour is used
**as-is** and is *not* auto-desaturated when the window goes inactive. This
matches macOS, where an app-set selection colour opts out of system management.
Only theme-driven (default) selections desaturate.

## `.dim_when_inactive(..)` — opt-in for custom content

The automatic layers cover stock widgets. For *custom* content an app wants to
fade back in a background window (a colourful side panel, a bespoke accent
surface), wrap it:

```rust
use bastyde::prelude::*;

ctx.add(my_panel.dim_when_inactive(0.4));   // 40 % opacity when inactive
ctx.add(my_panel.dim_when_inactive_default());   // default 70 %
```

`.dim_when_inactive(factor)` (on the `WidgetBuilder` trait) wraps the subtree in
`DimWhenInactive`, which drives a node-level opacity scope from
`window_active_signal`. It is layout- and a11y-transparent, and the opacity
**snaps** (no tween) — correct under `prefers-reduced-motion`, since window
activation is an OS state change, not a user-initiated motion.

## Keeping a widget vivid when inactive

There is no need to opt out of the automatic behaviour for normal apps. If a
widget genuinely must stay vivid regardless of window focus (a live status
indicator, a kiosk display), paint it directly from theme tokens and simply
don't consult `ctx.window_active` — or, app-wide, never call
`set_window_active(false)`.

## Accessibility

Caret hiding and selection desaturation are **paint-only**. They do not change
the AccessKit tree, the announced selection state, or any node value — a screen
reader still reports the selection and caret position normally. The visual
change is purely cosmetic.

## Testing

`WidgetTree::set_window_active(bool)` drives the state in a headless test:

```rust
let mut tree = WidgetTree::new().with_theme(intui::light());
let id = tree.add(some_editor);
tree.layout(SizeProposal::exact(400.0, 300.0));
// ...focus the widget...
tree.set_window_active(false);
let _ = tree.render();
// assert the caret is gone / the selection colour swapped
```

A fresh tree starts active (`is_window_active() == true`). See the tests in
`crates/bastyde-core/src/dim_when_inactive.rs`,
`crates/bastyde-widgets/src/rich_text/tests.rs`, and the
`window_active`-named tests under `bastyde-widgets`.

## Demo

`cargo run -p multi_window` — two windows, each with a status label, a
`TextInput`, and a `.dim_when_inactive` panel. Click between them to watch the
inactive window hide its caret, mute its selection, dim its panel, and flip its
status label.

## Implementation notes

- The reactive primitive lives on `WidgetTree` (`window_active_signal`), written
  by `set_window_active` and threaded onto `PaintContext` / `BuildContext` /
  `EventContext` exactly like the global text-scale value.
- A focus flip calls `WidgetArena::mark_all_needs_paint_only()` — a repaint of
  every active node, with no relayout and no cache clearing. Window-focus changes
  are rare (user-driven), so this is cheaper than `set_theme`'s `mark_all_dirty`
  (which also relayouts) and means any paint-time `ctx.window_active` reader is
  correct without per-widget binding ceremony.
- The frame-loop `tick()` of each text stack has no context, so `build()`
  registers an effect on `window_active_signal` that mirrors the value onto the
  editor state and, on deactivation, hides the caret *synchronously* (the frame
  loop may not tick while the window is parked).
