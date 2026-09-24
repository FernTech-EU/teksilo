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

`Enter` sits **under** the modifier rule like everything else in the table. It
was matched before the modifier test on both handles, so `Ctrl+Enter` collapsed
a splitter pane or hid a dock side and reported the key handled — the one row of
this table those two did not honour, while every other key on them did.

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
`MenuList`'s type-ahead guard — and it is now literally the same function,
`range_nav::is_accelerator_chord`. It had been spelled out at five call sites,
and they had already diverged.

**`AltGr` is the one exception, and it belongs to text, not to this table.** The
third-level shift on every non-US layout reaches an application as `Ctrl+Alt` on
Windows, X11 and Wayland alike — it is how a German keyboard types `@`, a French
one `€`, a Polish one `ą`. Refusing it in a *type-ahead* arm made every
character behind `AltGr` dead while the unshifted ones kept working, so the
feature half-worked in exactly the locales that needed it most. That question is
`range_nav::is_text_entry_chord`, deliberately **not** the negation of the
accelerator test: an `AltGr+↓` is still not this table's chord, because an arrow
key is not a character. macOS has no `AltGr` — it composes with `Option`, and
the OS rewrites the keystroke before the application sees it, exactly as the
menu bar's mnemonics document — so neither predicate needs a platform branch.

`range_nav` also owns `disclosure_chord`, the `Alt+↓` / `Alt+↑` / `F4` table
that shows and hides a drop-down. `ComboBox`, `DateEdit` and `PopoverWidget`
each hand-rolled it, and disagreed about `F4`: it is a drop-down *field's*
chord, and `PopoverWidget` — which also backs toolbar chevrons and menu buttons
— deliberately leaves that variant unmatched.

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

The page keys name a direction in the **content**, not on screen: `PageUp` is
one viewport back and `PageDown` one forward on either axis. Unlike the arrows
they therefore do not follow a scroll bar's orientation — reading them
geometrically made a horizontal bar's `PageUp` scroll *forward*, the opposite of
the vertical bar beside it.

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

A control mirrors in **all** of its readings or in none of them. Mirroring the
keys alone would leave the `←` key and a leftward drag moving the thumb in
opposite directions, which is worse than not mirroring at all — so each of
these moved its paint, its pointer mapping and its arrows together.

| Widget | Mirrors? | What that meant |
|---|---|---|
| `SplitterHandle` | **yes** | Its pane order and drag math had been mirrored since it shipped; only the keyboard was left behind, so `→` pulled the divider left. |
| `DockResizeHandle` | **yes** | Its pointer path already inverted per side (`side_main`); the keyboard did not. |
| `Slider` | **yes** | The minimum sits at the leading edge, so the fill grows leftward from a thumb that travels the other way. Read from `PaintContext::layout_direction` at paint time and `EventContext::is_rtl` at event time, so a locale flip needs no rebuild. A custom `SliderStyle` must do the same — the widget cannot enforce it, and the trait says so. |
| `ScrollBar` (horizontal) | **yes** | `ScrollArea` had *already* mirrored: it anchors content to the right and grows `scroll_x` leftward. The thumb did not, so at `scroll_x = 0` the content showed its beginning while the thumb sat at the far end of the track. The hit-test, the drag delta and the track-click direction key off one predicate — and so does the **painted** thumb, which was mirrored last: a `ScrollBarStyle` that offsets from `bounds.x` unconditionally draws the thumb at one end of the track while the grab region sits at the other, so it jumps the moment it is touched. The trait says so, the way `SliderStyle`'s does. |
| `HueStrip`, `AlphaStrip` | **n/a** | Only ever built vertical (their `orientation` builder is `pub(crate)` and the colour picker passes `Vertical`), and the vertical axis has no leading/trailing to mirror. Nothing to do until a horizontal strip exists. |

## The vertical axis puts the maximum at the top

Independent of the layout direction, and now uniform across every vertical
bounded scalar in the framework: `Slider`, `HueStrip` and `AlphaStrip` all place
their minimum at the **bottom**. Qt's `QSlider`, GTK4's `GtkScale`, the Win32
trackbar and `<input type=range>` agree, and it is what makes `ArrowUp` — which
this table reports as an *increase* — raise the thumb.

All three grew their value downward before, so `ArrowUp` increased the value and
moved the thumb **down**: the keys and the pointer drove the control in opposite
directions on screen. The geometry moved rather than the chord table, because
the chord table is the part every other toolkit agrees on. For the hue strip
that also reverses the rainbow texture, so hue 0 is at the bottom.

A vertical `ScrollBar` is not an exception to this and never was: a scroll
*offset* grows downward by definition, which is why `range_nav::towards_trailing`
exists to convert an increase in the value into a direction on screen.

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

An assistive technology's write lands on the **same grid the arrows walk**: the
advertised `numeric_value_step`, an arrow press and `Increment` all move by the
control's effective step, so a write that snapped to something else produced a
value no other path could reach — and one a screen reader would then announce.
A `Slider` that configures no step gets the 1 %-of-range grid its arrows already
use, a hundred positions, which is what `<input type=range>` gives a stepless
range too.

A write the widget **refuses** reports the action unhandled. `SetValue` on a
text-projected composite used to answer `Handled` whatever the host's parse
said, so `"twelve"` in Orca's value entry and in macOS's
`setAccessibilityValue:` both read back as success while the field quietly
reverted. A read-only field refuses the two writing actions outright — on the
*inner text node* as well as on the composite root, because not advertising an
action is not the same as refusing it: an adapter dispatches what the technology
asks for, and AT-SPI publishes `EditableText` off the interface set rather than
off the action list.

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

A date or time editor publishes **two** nodes, its own root and the
`Role::TextInput` of its field beneath it, and an assistive technology may
resolve either. Setting the root goes through the widget's own handler; setting
the *field* used to replace the displayed string and stop there, leaving the
typed value stale until the next blur and never firing `on_value_changed`. The
field now commits through its host when the host asks it to
(`TextInputField::on_access_set_value`, which `SpinBox` and the date and time
editors install), so both nodes land the same value by the same parse. A
`SpinBox` publishes one node, because its editing field is the
`Role::SpinButton` (see `Widget::accessibility_proxy` in
[accessibility-overrides.md](accessibility-overrides.md)): a string written to
it takes the same route, and a number or a step bubbles from the field to the
spin box's own handler. The host is handed the string rather than left to
re-read the bound signal, which the field syncs only on the next frame tick. A
plain `TextInput` installs nothing: its bound `Signal<String>` *is* the value,
so the write has already landed.

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
