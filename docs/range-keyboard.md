<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Bounded-scalar keyboard navigation

Seven widgets hold one bounded number and let the keyboard move it —
[`Slider`](../crates/teksilo-widgets/src/slider.rs),
[`SpinBox`](../crates/teksilo-widgets/src/spin_box.rs),
[`ScrollBar`](../crates/teksilo-widgets/src/scroll_bar.rs), the colour picker's
[hue](../crates/teksilo-widgets/src/color_picker/hue_strip.rs) and
[alpha](../crates/teksilo-widgets/src/color_picker/alpha_strip.rs) strips, a
[`Splitter` handle](../crates/teksilo-widgets/src/splitter/handle.rs) and a
[dock resize handle](../crates/teksilo-widgets/src/docking/resize_handle.rs).
This page is their keyboard contract, and the places Teksilo knowingly departs
from a platform.

Before the shared module they gave **four different answers** for the same four
keys: the slider had no paging, the spin box no `Home`/`End`, the strips both,
the handles neither. None of them looked at the modifiers, so every one of them
answered `Ctrl+Home` and reported the key handled — swallowing a chord the
application had bound.

The chord table lives in one module,
[`common/range_nav.rs`](../crates/teksilo-widgets/src/common/range_nav.rs), as a
pure function over `(key, modifiers, kind, axis, direction)`. Each widget keeps
only its own arithmetic. It is the bounded-scalar sibling of
[`common/list_nav.rs`](../crates/teksilo-widgets/src/common/list_nav.rs) and
[data-view-keyboard.md](data-view-keyboard.md), deliberately the same shape.

## The three topologies

The discriminator is not the widget's name but two questions: does the control
have a range the user can page through, and does something else already own
`Home`/`End`?

- **`Scalar`** — the whole range belongs to the keyboard: `Slider`, `HueStrip`,
  `AlphaStrip`, `ScrollBar`.
- **`Divider`** — a boundary whose "value" is a position: `SplitterHandle`,
  `DockResizeHandle`.
- **`TextEditable`** — a number behind an editable field: `SpinBox`.

## The bindings

| Chord | `Scalar` | `Divider` | `TextEditable` |
|---|---|---|---|
| `←` / `→` | ± one step (horizontal or both axes) | ± one step (horizontal divider) | — *(the caret's)* |
| `↑` / `↓` | ± one step (vertical or both axes) | ± one step (vertical divider) | ± one step |
| `PageUp` / `PageDown` | ± one page | — | ± one page |
| `Home` / `End` | minimum / maximum | the two limits | — *(the caret's)* |
| `Shift` + any of the above | the same chord | the same chord | the same chord |
| `Ctrl` / `Alt` / `Super` + any | falls through | falls through | falls through |
| `Enter` | — | toggle the adjacent collapsible pane | commit |

### The modifier rule, stated once

A chord holding `Ctrl`, `Alt` or `Super` is **not** the control's, and reaches
the global Shortcut/Action pipeline instead. Before this, every one of the seven
widgets consumed such a chord: `Ctrl+Home` drove a slider to its minimum,
`Ctrl+ArrowUp` stepped a spin box, `Ctrl+PageDown` scrolled a scroll bar — each
reporting the key handled, so no ancestor ever saw it.

`Shift` is deliberately **not** rejected. It is not a distinct chord on any
bounded scalar — `QAbstractSlider` and Blink's range input never inspect the
modifiers — and Teksilo's single-line field binds nothing to `Shift+↑`, so
rejecting it would make the chord dead rather than deferential. Callers that
give `Shift` their own meaning (the date and time editors use it as a ×10
multiplier) read it themselves.

This is the same rule, for the same reason, as
[`list_nav::tree_chord`](../crates/teksilo-widgets/src/common/list_nav.rs) and
`MenuList`'s type-ahead guard.

## Step sizes

A page is **not** required to be a multiple of a step. `ScrollBar` measures its
own, exactly as `NavMove::Page` is measured from the row-offset table.

| Widget | Fine step | Coarse step |
|---|---|---|
| `Slider` | `step`, default 1 % of the range | `page_step`, default 10 × step ⇒ 10 % of the range |
| `SpinBox` | `single_step`, default 1 | `page_step`, default 10 × `single_step` |
| `HueStrip` | 1° | 15° |
| `AlphaStrip` | 0.01 | 0.10 |
| `ScrollBar` | `step_size` | one **measured** viewport, degrading to the step where the content barely overflows |
| `SplitterHandle` | `keyboard_step_px` | — |
| `DockResizeHandle` | a fixed keyboard step | — |

`Slider`'s default lands on 10 % of the range, which is simultaneously
`QAbstractSlider::pageStep`, `GtkScale`'s page increment and what WebKit and
Blink give `<input type=range>`.

## Why this carries no platform branch

Like [`list_nav`](../crates/teksilo-widgets/src/common/list_nav.rs) and unlike
[`text_nav`](../crates/teksilo-widgets/src/common/text_nav.rs), there is nothing
to branch on. Qt's `QAbstractSlider::keyPressEvent`, GTK4's `GtkScale`, the
Win32 trackbar and `<input type=range>` in both WebKit and Blink bind these keys
identically on all three platforms; none has a platform guard.

## Three deliberate deviations

Recorded here so there is a link to hand whoever files them.

### macOS has no jump-to-extremum key, and Teksilo binds one anyway

`StandardKeyBinding.dict` spends `Home`/`End` on `scrollToBeginningOfDocument:`
and `scrollToEndOfDocument:`, and `PageUp`/`PageDown` on `scrollPageUp:` /
`scrollPageDown:`. `NSStepper` answers only the arrows. On a laptop keyboard all
four need `Fn`+arrow to press at all. So a Mac has no key that drives a slider
to either end.

Teksilo does not reproduce that, for the same reason it does not reproduce
`NSTableView`'s reading of the same four keys (see
[data-view-keyboard.md](data-view-keyboard.md)): it would ship a slider a
keyboard cannot drive to its own bounds, and no cross-platform toolkit does it.

### A `SpinBox` does not bind `Home`/`End`, against the ARIA pattern

The W3C ARIA spinbutton pattern lists `Home` → minimum and `End` → maximum as
**required**, and concedes in the same breath that a text-editable spinbutton
also honours the platform's single-line text-editing keys. Every desktop
implementation resolves that tension the same way Teksilo does:

| Toolkit | Arrows | Page | `Home` / `End` |
|---|---|---|---|
| Qt `QAbstractSpinBox` | ±1 (Ctrl ⇒ ×10) | `stepBy(±10)` | → the inner `QLineEdit`; only `Shift+Home`/`End` is special-cased, to keep the selection out of the prefix and suffix |
| WinUI 3 `NumberBox` | `SmallChange` | `LargeChange`, default 10 | not bound |
| Blink `HandleKeydownEventForSpinButton` | only `↑`/`↓` | not bound | not bound |
| Avalonia `NumericUpDown` | `↑`/`↓`, `Enter` | not bound | not bound |
| jQuery UI Spinner | `↑`/`↓` | `page`, default 10 | not bound |
| GTK4 `GtkSpinButton` | step | page | bare → the entry; **`Ctrl+Home`/`Ctrl+End`** → min/max |

In a text field `Home` and `End` are the caret's, and no value is unreachable
without them: typing the number always works, and the AT `SetValue` action is
serviced. GTK's `Ctrl+Home`/`Ctrl+End` is the one outlier, and Teksilo declines
it: `Ctrl+Home` is the *document* chord on the two platforms that have one, so a
spin box inside a scrollable form would steal it — and it would be the single
binding in this table that fires *with* an accelerator, one line below the rule
that says accelerators fall through.

### A dock divider's extremes are hide and show

`DockResizeHandle` maps `Home` to hiding its side and `End` to showing it. For a
side that can collapse, "give everything to the centre" is what the low extreme
means; there is no numeric minimum short of it. `Home` falls through when
collapsing is locked by policy.

## Right-to-left

`←`/`→` mirror with the layout direction. `↑`/`↓`, `Home`/`End` and the page
keys do **not**: they name points in *value* space rather than on screen, so the
minimum is the minimum in either direction. Qt flips on `isRightToLeft()`, and
only the horizontal pair.

Who mirrors today, and why the rest do not:

| Widget | Arrows mirror? | Why |
|---|---|---|
| `SplitterHandle` | **yes** | Its pane order and drag math have been mirrored since it shipped; only the keyboard was left behind, so `→` pulled the divider left. |
| `DockResizeHandle` | **yes** | Its pointer path already inverts per side (`side_main`); the keyboard did not. |
| `Slider`, `HueStrip`, `AlphaStrip`, `ScrollBar` | **no** | Their paint *and* their pointer mapping run leading-to-trailing unconditionally. Mirroring the keys alone would leave the `←` key and a leftward drag moving the thumb in opposite directions — worse than today's uniform non-mirroring. Paint, drag and keys mirror together or not at all, and that is one change per widget rather than this one. |

## Chords Teksilo deliberately does not bind

| Chord | Why not |
|---|---|
| `Ctrl+Home` / `Ctrl+End` on a spin box | GTK alone binds it. `Ctrl+Home` is the document chord where one exists, so a spin box in a scrollable form would steal it, and typing the number already reaches either end. |
| `PageUp` / `PageDown` on a divider | No unit a page could be a multiple of. The ARIA window-splitter pattern asks only for `Home`/`End`, and `QSplitterHandle` binds no page keys. |
| `←` / `→` on a spin box | The caret's, on every platform. |
| `↑` / `↓` on a horizontal scroll bar | The vertical bar beside it owns them. |
| `Ctrl+↑` / `Ctrl+↓` as a ×10 step (Qt's `stepModifier`) | Redundant with `PageUp`/`PageDown`, which apply the same ×10 by default — and on macOS those two chords are Mission Control and Application Windows. |
| `Ctrl+Alt` + anything | Owned by NVDA and JAWS for table reading, and `Ctrl+Option+…` by VoiceOver. Same row as in [data-view-keyboard.md](data-view-keyboard.md). |
| `Escape` to revert a slider or a spin box | No platform documents it, and `Escape` already means cancel-edit → close-popup → close-dialog. |

## Accessibility

Every one of these publishes `numeric_value`, `min_numeric_value`,
`max_numeric_value` and `numeric_value_step`; the ones with a coarse step also
publish `numeric_value_jump`, so an assistive technology can announce both
distances.

`Slider` and `SpinBox` service `Action::SetValue` in **both** payload shapes,
because both are sent in the field: a number (macOS `setAccessibilityValue:`
with an `NSNumber`; AT-SPI's `Value.SetCurrentValue`, which is how Orca sets a
slider or a spin button) and a string (an `NSString`; Teksilo's own automation
`set_value` tool, which sends only strings). AT-SPI publishes the `Value`
interface off `numeric_value` alone, so `SetCurrentValue` reaches these widgets
whether or not the action is advertised; macOS instead *gates* AXValue
settability on the advertisement, which is why both widgets advertise it — and
why a read-only `SpinBox` advertises none of the three mutating actions rather
than claiming a settability it does not have.

`ScrollBar` is the exception: its node is `set_hidden()` and it is
`focusable(false)`, because assistive technology scrolls through the parent
`ScrollView`'s `Scroll*` actions. Nothing in the framework focuses a scroll bar,
so its key handler is reachable today only by a programmatic
`WidgetTree::focus` — a test, or an application that opts in. It is routed
through the shared table anyway, so the behaviour is correct if that ever
changes.

## Testing both directions from one host

`range_move` takes `rtl` as a parameter rather than reading a `cfg!` constant,
so both branches are reachable from one host's test run — the same split
[`text_nav`](../crates/teksilo-widgets/src/common/text_nav.rs) and
`list_nav::mac_alias` already use. The chord table is unit-tested in isolation;
the widget end goes through `WidgetTree::press_key(key, modifiers)`, with
`WidgetTree::set_layout_direction` covering the mirrored case.
