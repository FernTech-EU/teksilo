<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Touch verification — the hardware procedure

Everything in Teksilo's pointer model that a headless Linux CI host can check is
checked by the suite. This page is the rest: the claims that need a real
touchscreen, a real stylus, a real trackpad and a real compositor, written as a
procedure someone else can execute.

**This document is the procedure. [`touch-verification-signoff.md`](touch-verification-signoff.md)
is where its results go, and nothing has been recorded there yet.** Until that
file is filled in at a release SHA, the touch programme is not signed off,
however green the suite is.

Three things this page is not. It is not a list of suspected bugs — each check
exists because the *host* cannot answer it, not because the answer is doubted. It
is not a substitute for the automated suite, which is broader and runs on every
commit. And it is not optional: §9 of [Touch & pen](touch-and-pen.md) enumerates
call sites carrying a platform answer that is a constant on Linux, so a green
suite there cannot tell a right answer from a missing one.

## Contents

- [1. Before you start](#1-before-you-start)
- [2. The device matrix](#2-the-device-matrix)
- [3. Reading a trace](#3-reading-a-trace)
- [4. Per-platform setup](#4-per-platform-setup)
- [5. A — Samples arrive, with an identity](#5-a--samples-arrive-with-an-identity)
- [6. B — Hover, primacy and the cursor](#6-b--hover-primacy-and-the-cursor)
- [7. C — Arbitration](#7-c--arbitration)
- [8. D — Density and targets](#8-d--density-and-targets)
- [9. E — Kinetic feel](#9-e--kinetic-feel)
- [10. F — Touch text editing](#10-f--touch-text-editing)
- [11. G — Pen and stylus](#11-g--pen-and-stylus)
- [12. H — Trackpad gestures](#12-h--trackpad-gestures)
- [13. I — Overlays and dismissal](#13-i--overlays-and-dismissal)
- [14. J — The on-screen keyboard](#14-j--the-on-screen-keyboard)
- [15. K — Drag and drop across the OS boundary](#15-k--drag-and-drop-across-the-os-boundary)
- [16. L — Cancellation](#16-l--cancellation)
- [17. M — With a screen reader attached](#17-m--with-a-screen-reader-attached)
- [18. N — The reviewed-rather-than-tested call sites](#18-n--the-reviewed-rather-than-tested-call-sites)
- [19. O — HiDPI](#19-o--hidpi)
- [20. What to do with a failure](#20-what-to-do-with-a-failure)

---

## 1. Before you start

Record the build once, at the top of the sign-off sheet: `git rev-parse HEAD`,
the OS and its version, the session type (Wayland / X11 / Windows / macOS), the
device make and model, and the display scale factor.

Two binaries carry the whole procedure.

```bash
# The instrumented one. Reports every pointer it is handed, names which
# contender won each press, switches density and the kinetic constants live.
TEKSILO_TRACE_INPUT=all cargo run -p touch-playground

# The catalog, at one density per launch.
cargo run -p widget-catalog -- --tab touch
cargo run -p widget-catalog -- --tab touch --density comfortable
cargo run -p widget-catalog -- --tab touch --density touch
```

A few checks name another example instead, because it is the only place a
behaviour lives: `scene-showcase` for pinch and rotation, `terminal-demo` for the
terminal, `drag-and-drop` and `file-drop` for the OS drag boundary,
`rich-text-editor` for the full text stack, `title-bar-demo` for window chrome.

Run with `--release` if the machine is slow; a debug build's frame time can make
a fling look like a stutter that is not there. Note which you used — a
feel judgement recorded against a debug build is worth less than one against a
release build, and saying which is the difference.

Keep the trace visible. Half of these checks are answered by a line appearing or
not appearing, and none of them is answered by a screenshot.

## 2. The device matrix

Every row is required. A row with no hardware available is **not** a pass — leave
it blank and say so in the sign-off; a blank row is information and a guessed one
is not.

| # | Platform | Device | Also at |
|---|---|---|---|
| 1 | Windows | 2-in-1 touchscreen **and** its stylus | display scale 200 % |
| 2 | Linux / Wayland | graphics tablet with a stylus (`zwp_tablet_v2`) | fractional scale |
| 3 | Linux / Wayland | touchscreen | display scale 200 % |
| 4 | Linux / X11 | touchscreen | display scale 200 % |
| 5 | macOS | trackpad | a Retina display |

Rows 3 and 4 are the *same hardware in two sessions*, and they are separate rows
because the translator's work is different in each: X11 promotes a
pointer-emulating touch onto the virtual core pointer and Wayland does not.

Where a check applies only to some rows it says so. Where it applies to all of
them, run it on all of them: the interesting failures in this area have all been
one-platform failures.

## 3. Reading a trace

`TEKSILO_TRACE_INPUT` takes `samples`, `gestures` or `all`; an unrecognised value
means off, so a typo cannot silently select a different level. Every line is
prefixed `[teksilo input] `.

The lines below are the **whole** vocabulary. If a check says "expect a `touch`
line" and you see nothing, the sample never reached the translator, which is a
different failure from a sample that reached it and was dropped — and the drop
lines say which. Angle brackets are placeholders; the real text is a Rust `{:?}`
of the named value, so a pointer id prints as `PointerId(<n>)`.

**Samples level — the translator (`teksilo-platform`).**

| Line | Means |
|---|---|
| `touch <phase> <id> os_id=<n> at <point>` | a touch packet was translated |
| `pen <id> in proximity (<tool>) at <point>` | a stylus entered proximity |
| `pen <id> left proximity` | and left it |
| `scroll <delta> <phase>/<source>` | a wheel notch or a trackpad scroll |
| `cursor move suppressed (emulated) at <point>` | a phantom cursor move while a contact is live |
| `mouse <button> suppressed (promoted from touch)` | a synthetic click the OS made from a contact |
| `cancel_all <id>` | every live contact revoked at the translator |
| `touch dropped: touch_enabled=false (os id <n>)` | the kill switch is off |
| `pen: SetWindowSubclass returned FALSE; this window has no pen input` | the Windows pen shim did not install |
| `pen: the compositor advertises no zwp_tablet_manager_v2` | no Wayland tablet protocol |
| `mouse button dropped: no cursor position yet (…)` | a press before any cursor position |

**Samples level — the tree (`teksilo-core`).**

| Line | Means |
|---|---|
| `<phase> <id> at <point> buttons=<mask> t=<time>` | the sample entered the router |
| `scroll <delta> <phase>/<source> at <point>` | a scroll entered the router |
| `dropping <id>: the backend classified it as a palm` | refused at the pointer table |
| `dropping <id>: the pointer table already holds <n> pointers` | the contact cap |
| `<id> released as a palm: large, and it never moved` | released, then reclassified |
| `swallowing the Up for <id>: its press was cancelled` | no tap completes |
| `pan absorbed by <id>` / `pan contained at <id>: the chain stops here` | where a pan stopped |
| `cancel queued for <id>: <reason>` / `cancelling <id>: <reason>` | the cancel funnel |
| `cancel for <id> (<reason>) dropped: nothing left to revoke` | nothing to revoke |
| `no press visual for <id>: <button> is not one of its accepted buttons` | a press that raises no pressed state |

**Gestures level.**

| Line | Means |
|---|---|
| `sequence opened for <id>: action=<touch action> dead_zone=<boundary> pan_members=<n>` | the press's arbitration began |
| `sequence for <id> decided: <winner>` | one contender won |
| `member <m> of <id> revoked: <reason>` | a loser was told |
| `sequence for <id> cancelled: its owner is gone (<reason>)` | the pressed node went away |
| `no sequence for <id>: touch_enabled=false` | the kill switch again |
| `<id> is inside the parked subtree but its press has already ended: not cancelled` | a dormancy teardown that had nothing to do |
| `the OS drag this window held as re-entered has ended elsewhere: dropping the session` | an inbound OS drag cleaned up |

A line you see that is not in those three tables is a line this document is out
of date about. Say so in the sign-off.

## 4. Per-platform setup

**Windows.** The pen path is a `WM_POINTER*` window subclass installed at window
creation; if it did not install, the trace says so on the first pen sample and
the stylus behaves as a mouse. Set the display scale to 200 % for the HiDPI half
of row 1 and restart the app — scale is read at window creation.

**Linux / Wayland.** A stylus needs the compositor to advertise
`zwp_tablet_manager_v2`; without it the trace says so once and there is no pen
path at all. Note the compositor and its version: tablet support varies. Window
*position* is not part of the protocol, so nothing on this page asks you to check
it. **A finger cannot drag a window on Wayland** — that is a protocol fact, not a
defect, and is recorded as such in [Touch & pen](touch-and-pen.md) §5.4.

**Linux / X11.** Needs a window manager implementing `_NET_WM_MOVERESIZE` for
custom chrome; the probe runs before window creation and falls back to native
decorations, so `title-bar-demo` may legitimately show no custom title bar. X11
promotes a pointer-emulating touch onto the virtual core pointer, buttons
included, which is what makes the outbound-drag path work for a finger there —
and what makes the `cursor move suppressed (emulated)` line expected rather than
alarming.

**macOS.** The OS accent colour is not read by the platform layer, so a theme
check is not part of this page. Force Touch has no arm in the pointer family by
design.

## 5. A — Samples arrive, with an identity

Rows: all. Binary: `touch-playground`, the pointer pad.

**A1 — one contact.** Touch the pad with one finger and hold. The readout shows
one row: kind `touch`, `down`, a position, and a speed once you move. Trace: a
`touch Down` line from the translator, then a `Down PointerId(<n>)` line from the
router, then `sequence opened for PointerId(<n>): action=NONE …` — `NONE` because
the pad declares it.

**A2 — the identity is per press.** Lift and touch again. The row's `#<n>` is a
*different* number. An identity that repeats across presses is a real defect:
the framework mints one per press precisely so that an OS that reuses slot ids
cannot make two gestures look like one.

**A3 — two contacts.** Put two fingers on the pad. Two rows, two distinct ids.
Lift one; the other survives with its own id unchanged.

**A4 — five contacts.** Put five fingers down. Count the rows. Beyond the contact
cap the trace says `dropping …: the pointer table already holds <n> pointers`
rather than silently losing one — that line appearing is a pass, not a failure.

**A5 — a lifted contact ceases to exist.** Lift every finger: the live list is
empty. A mouse and a pen in proximity stay on the list, hovering; a contact does
not.

**A6 — the kill switch.** Not reachable from the playground's UI. If you can
build with `InputTokens::touch_enabled` set to `false`, every touch produces
`touch dropped: touch_enabled=false` and nothing else, and the app behaves
exactly as it does with a mouse. Skip this row if you cannot rebuild.

**A7 — palm rejection.** Rows 1 and 2 only, and only if the digitizer reports it.
Rest a palm on the glass while writing with the stylus. Expect
`dropping …: the backend classified it as a palm` and no pad row for the palm. A
digitizer that does not advertise palm reporting cannot fail this check; record
"not reported" rather than "pass".

**A8 — the contact patch.** Rows 1, 3, 4. Press hard with a fingertip and then
with a flat thumb. If the digitizer sizes contacts, the pad's disc grows and the
`contact <w>x<h>` axis appears. Many do not report it; "none reported" is a
legitimate result.

## 6. B — Hover, primacy and the cursor

Rows: all.

**B1 — a contact never hovers.** With the mouse cursor parked somewhere else,
touch the pad. The `hover owner (pad's own)` line must **not** change to the
contact's id. A contact that becomes the hover owner is a defect: hover, the
cursor and tooltip dwell all follow a hovering-capable pointer, and a finger is
not one.

**B2 — a pen in proximity is a hover owner.** Rows 1 and 2. Bring the stylus near
the glass without touching. The pad shows a `pen:…` row that is *not* `down`, and
the hover-owner line names it.

**B3 — primary is per kind, not one per machine.** Row 1, and any machine with
both a mouse and a touchscreen. Move the mouse over the pad, then touch it with
one finger without moving the mouse. Both rows can read `primary` at once —
that is the W3C per-kind flag, and it is correct. Do not report it as a bug.
(What is at most one is the pointer *table's* elected primary, which no widget
can read; see the ledger in [Touch & pen](touch-and-pen.md) §10.)

**B4 — no phantom cursor.** Rows 3 and 4. Touch the pad and watch for
`cursor move suppressed (emulated)`. On X11 that line is expected before the
first packet of the first contact. What must **not** happen is the cursor visibly
jumping to the contact and a hover highlight following it.

**B5 — the promoted click.** Rows 3 and 4. A tap must produce exactly one
activation. If the OS also synthesises a mouse click, the trace shows
`mouse <button> suppressed (promoted from touch)` and the control fires once. A
control that fires twice is a real defect.

**B6 — hover-revealed affordances.** `widget-catalog --tab tab_widget`… in
practice: open the catalog's Containers tab and find the `TabWidget`. At the
default density the close `×` is revealed by hover, so a finger cannot reach it;
relaunch with `--density touch` and it is there unconditionally. Both halves are
the expected behaviour. What a finger always has is `Delete` on the focused
header and the tab's assistive-technology close action.

## 7. C — Arbitration

Rows: all, and this is the section where a platform difference would be most
surprising. Binary: `touch-playground`, the scenario column. Each scenario names
its contenders and shows which one won; the block near the bottom of the readout
lists all five verdicts at once.

The same five are asserted headlessly by the playground's own tests, so a
disagreement here means the *platform* is delivering something the injected
samples do not.

**C1 — list row: a short drag pans.** Drag a row without pausing. Verdict:
`pan — the scroller`. Trace: `sequence opened … pan_members=1` or more, then
`sequence for … decided: <the scroller>`, then `member <the row> … revoked:
PeerClaimed`.

**C2 — list row: a hold then a drag reorders.** Press a row, wait, then drag.
Verdict: `reorder — the row (<from> → <to>)`. If it reports `pan` instead, note
how long you held: the hold is the profile's `long_press`, shown in the
scenario's own heading.

**C3 — list row: a tap selects, on the release.** Tap a row. Verdict:
`select — the row (#<n>)`. Now press a row and slide your finger off it before
lifting: the selection must **not** move. That is the release-commit rule, and it
is the one behaviour change in this section a mouse user would also notice.

**C4 — the slider never pans.** Drag the slider's thumb. Verdict:
`the slider (<value>)`, never `pan`. Then drag anywhere else inside that box:
`pan — the scroller`.

**C5 — text: drag pans, hold selects.** In the text scenario, drag: verdict
`pan — the editor`. Hold on a word: verdict `select — a word, from the hold`, and
two handles appear. Tap: `caret — placed at <offset>`.

**C6 — the splitter grip.** Drag the gutter between the two panes. Verdict
`the grip (<a> / <b> dp)`. The gutter is painted at the theme's grab size and
reaches further for a coarse pointer; C6 passes if you can grab it *without
aiming*, which is the point of the outset.

**C7 — nested scrollers chain.** Pan inside the inner list to its end and keep
going. The verdict changes from `the inner list` to `the outer area` without
lifting. Trace: `pan absorbed by <inner>` then, past the end,
`pan absorbed by <outer>`.

**C8 — a mouse never pans.** With a mouse, drag inside any of those scrollers.
Nothing scrolls. The wheel does. This is the mouse-unchanged half and it is worth
checking on every platform, because it is the guarantee most of the suite exists
to protect.

**C9 — the scene: one finger marquees, two pinch.** `cargo run -p scene-showcase`.
A one-finger drag in empty space draws a marquee and does **not** pan the camera —
measured behaviour, not an oversight, and the example's own header says so. Two
fingers zoom. The wheel and a two-finger trackpad scroll pan.

## 8. D — Density and targets

Rows: all, and the HiDPI variant of each.

**D1 — the ladder is what the tab says.** Launch the catalog three times, once
per `--density`, on the Touch tab. The active rung is marked, and the numbers in
the table are the ones every widget below was built with. Compare the visible
size of the same control across the three launches.

**D2 — the live switch.** In the playground, move the density toggle. Every
target grows or shrinks, the scroll offsets reset (a density switch rebuilds the
tree, deliberately) and, with a screen reader running, it says so once.

**D3 — a 24 dp target is reachable without aiming.** At Compact, on the Touch
tab, try the smallest controls with a fingertip: the splitter gutter, a spin
box's step buttons, a colour swatch. The conformance floor is 24 dp at every
rung and never scales. A control you cannot hit without looking is a finding —
record which one, at which density, and its measured size if you can read it off
the debug inspector's target rows (F12).

**D4 — nothing moved that should not have.** At Compact the layout is meant to be
byte-for-byte what it was before the touch programme: the hit mechanisms are
hit-only. If a control's *painted* geometry differs from a pre-programme build at
Compact, that is a regression.

**D5 — HiDPI.** Repeat D1 and D3 at 200 % (or a Retina display, or a Wayland
fractional scale). A dp is a logical pixel: the numbers in the tab must not
change with the scale factor, and the physical size of a target must roughly
double. Both halves matter — a target that stays physically the same size at 200 %
means a dp got confused with a device pixel somewhere.

## 9. E — Kinetic feel

Rows: all. Binary: `touch-playground`, the kinetic panel plus the nested-scroller
scenario.

**E1 — a flick coasts.** Flick the inner list and let go while still moving. It
keeps going and decelerates smoothly to a stop. A flick that stops dead on
release means no velocity reached the fling driver.

**E2 — a slow drag does not.** Drag slowly and release. No coast. The gate is a
minimum fling velocity; this check is what tells you it is doing its job.

**E3 — the friction knob bites.** Halve `clamping friction`, press Apply, flick
again. It coasts noticeably further. Restore with "Reset to shipped".

**E4 — the two families feel different.** Switch to `Bouncing`, flick into the
end of the list. Switch to `Clamping`, do it again. Clamping stops dead at the
bound; Bouncing decays differently. Then switch to `Platform` and record which of
the two it picked on this OS — Bouncing on macOS, Clamping elsewhere.

**E5 — past the end.** Turn on "follow the finger past the end", Apply, and drag
beyond the list's end. The content follows with decreasing gain and springs back.
Off (the default) it stops dead. The default is load-bearing: a nested surface
that banded at its own end could never hand the gesture outward, which is C7.

**E6 — reduced motion.** Turn the OS "reduce motion" setting on and repeat E1.
The band hard-clamps. A coast that still overshoots under reduced motion is a
finding.

## 10. F — Touch text editing

Rows: all. Binaries: `touch-playground`'s text scenario for the contract,
`rich-text-editor` for the full surface, `terminal-demo` for the terminal's own.

**F1 — the caret lands on the release.** Tap in the text. The caret appears when
you lift, not when you touch. Press and slide off before lifting: no caret moves.

**F2 — a hold selects the word and raises handles.** Hold on a word. It is
selected, two handles appear at the ends, and a selection toolbar comes up.

**F3 — a handle drags the end it marks.** Drag the trailing handle. The selection
grows from that end only. Drag it past the other handle: the range inverts rather
than collapsing.

**F4 — the handle does not fight the glyph row.** Drag a handle slowly along a
line. The selection end follows the *text under the handle's point*, not a dozen
pixels above it. This is the one check aimed at a known contract weakness — the
core hit-tests the raw contact position and each host compensates — so a host
that got the compensation wrong shows up as a consistent vertical offset.

**F5 — the magnifier.** While dragging a handle, a magnified strip of the text
under it appears. Note whether it is legible and whether it lags.

**F6 — a handle at the edge scrolls the view.** Drag a handle to the top or
bottom edge. The view scrolls to follow it. It scrolls per *sample*, so a finger
held perfectly still inside the band does not keep scrolling — that is documented
behaviour, not a defect.

**F7 — the on-screen keyboard follows the caret.** Rows 1 and 3 (and any platform
with an OS keyboard). Place the caret with a tap near the bottom of a long
document: the reported IME area is the caret's, so the keyboard should not cover
it. A keyboard that covers the caret a *finger* placed — while behaving correctly
for a caret an arrow key placed — is the exact shape of a stale IME-area report.

**F8 — the terminal.** `terminal-demo`. A one-finger drag scrolls the scrollback
and coasts. Double- and triple-tap select the word and the line. A hold opens the
terminal's menu (Copy / Paste / Select all / Clear); there is no separate
selection toolbar, deliberately. Run a program that requests mouse reporting
(`htop`) and confirm a finger is **not** reported to it as a click.

**F9 — a password field.** `password-field`. Masked text selects and the caret
places by the same gestures, and the plaintext never appears in the selection
toolbar or to a screen reader while masked.

## 11. G — Pen and stylus

Rows 1 and 2 only. Skip every check here on a machine without a stylus and say so.

**G1 — proximity is a state.** Bring the stylus near the glass. Trace:
`pen PointerId(<n>) in proximity (<tool>) at <point>`. The pad shows a pen row,
hovering. Move it away: `pen … left proximity`, and the row goes.

**G2 — pressure.** Touch down and vary the pressure. The pad's `pressure <v>`
axis moves between 0 and 1. A pen that reports no pressure at all shows no
pressure axis — check the device's own driver before recording a failure.

**G3 — tilt.** Tilt the stylus. `tilt <x>/<y>deg` moves, each in −90…90. Tilt is
often unreported; record "none reported" if so.

**G4 — barrel rotation.** Only some styluses. `twist <v>deg`, 0…359.

**G5 — the eraser.** Flip the stylus (or press its eraser button). The pad's kind
reads `pen:eraser` rather than `pen:pen`.

**G6 — the barrel button.** Press it while drawing. The pad's `buttons` change.
Note what the OS mapped it to.

**G7 — a pen is precise but direct.** Drag the splitter grip in scenario C6 with
the stylus. It latches in about 2 dp of travel, far tighter than a finger's 18 —
that is the pen profile, and it is the point of having one. And the grip does
**not** decide at the press the way a mouse's capture does, because a pen is a
direct pointer.

**G8 — a pen beside a live mouse.** Move the mouse to one control and hover the
stylus over another. Each hover follows its own device; the mouse's highlight
must not jump to the stylus, nor the reverse.

**G9 — one completion per press.** Watch the trace across a full stylus stroke.
A pen's sequence is Down → Up; a stroke that also produces a cancel for the same
identity is a defect.

## 12. H — Trackpad gestures

Row 5 primarily; rows 1–4 wherever the platform reports trackpad gestures.
Binary: `scene-showcase`.

**H1 — two-finger scroll pans.** The scene pans. Trace: `scroll <delta> …` lines
with a trackpad source.

**H2 — pinch zooms by the amount pinched.** Spread to about twice the starting
span. The scene ends at about twice the zoom — **not** at its maximum. Each
sample carries the factor since the *previous* sample and the consumer folds it
in; a spread that runs into `max_zoom` means something is reading a cumulative
value as a delta.

**H3 — rotation turns by the angle twisted.** Twist two fingers by roughly 90°.
The content turns by roughly 90°, not by 90 radians. One degree of twist turning
the content about 57° means the degrees-to-radians conversion at the platform
seam is gone.

**H4 — THE OPEN QUESTION: the rotation sign.** *This check is the reason §12
exists and it cannot be answered anywhere but here.*

Twist two fingers **clockwise** on the trackpad. Record which way the scene's
content turns.

The framework's own geometry is self-consistent in y-down screen space: a
positive angle turns content clockwise, and the touchscreen arm derives its angle
with `atan2` in that same space. AppKit documents `NSEvent.rotation` as positive
for **counter-clockwise**, and winit documents no sign convention at all for
`RotationGesture.delta` — so the source tree cannot settle whether a positive
trackpad delta should be negated at the seam. It was deliberately not guessed at.

- If the content follows your fingers, record **correct** and the programme is
  done with this question.
- If the content turns the **opposite** way from your fingers, that is a real
  defect: the fix is one negation at the platform seam —
  `teksilo-platform`'s `event_translation::rotation_gesture`, where the
  degrees-to-radians conversion already lives — and it must be paired with a test
  that pins the sign so it cannot drift back.

Either answer closes the question; not running the check leaves it open. Do not
infer the answer from the touchscreen arm — H3's magnitude passing tells you
nothing about H4's sign.

**H5 — a touchscreen pinch, for comparison.** Rows 1, 3, 4. Repeat H2 with two
fingers on glass. Both producers deliver the same payload contract, so the
outcome should be indistinguishable from the trackpad's. A disagreement between
the two is the defect this contract was written to prevent.

## 13. I — Overlays and dismissal

Rows: all. Binaries: `menus-and-dropdowns`, `dialogs-and-popovers`.

**I1 — an outside press dismisses on the release.** Open a dropdown and press
outside it. It closes when you lift.

**I2 — and does not reach what is under it.** Open a dropdown and press outside
it, directly on top of a button. The dropdown closes and the button does **not**
fire. Pressing it again does. Two presses to dismiss-then-activate is the
intended behaviour, on every pointer kind.

**I3 — a menu clears the contact.** Hold on something with a context menu so the
menu opens under your finger. The menu is placed clear of the contact patch —
you can read the item under your own finger. A mouse's placement is unchanged.

**I4 — a tap on a menu item that closes its menu.** Tap an item whose action
dismisses the menu. It fires exactly once and nothing is left on screen.

**I5 — a submenu by tap.** Tap a submenu trigger. The submenu opens immediately;
there is no hover and therefore no safe triangle for a finger, and none is
needed.

**I6 — a tooltip by hold.** Hold a control with a plain tooltip. The tip appears.
Escape, a press elsewhere, or waiting retires it. A plain tooltip has no focus
route, so this is the *only* way a finger reaches it.

## 14. J — The on-screen keyboard

Rows 1 and 3 chiefly; X11 has no protocol for this and the framework says so.

**J1 — it appears for a text field.** Tap into a text field. The keyboard comes
up.

**J2 — it does not appear for a non-text control.** Tap a button. It does not.

**J3 — the safe-area inset.** With the keyboard up, the app's content is not
hidden behind it and the caret stays visible.

**J4 — X11 reports none.** Row 4. The framework advertises no soft-keyboard
support on X11, deliberately: no protocol exists. Nothing should appear and
nothing should be logged as an error.

## 15. K — Drag and drop across the OS boundary

Rows: all. Binaries: `file-drop` (inbound), `drag-and-drop` (outbound).

**K1 — an inbound file drop, with a finger.** Drag a file from the OS file
manager onto the drop zone using touch, if the platform lets you. The zone
highlights and the drop is reported.

**K2 — the cursor does not promise a drop the target refuses.** Drag a file of
the wrong type over the extension-filtered zone. The OS cursor shows refusal.
This is the revised-accept path, and it needs a real source application — GTK and
Qt both track the latest status, which is the premise.

**K3 — an outbound drag from a finger.** In `drag-and-drop`, hold a library row
and drag it out of the window onto another application. Rows 2, 3 (Wayland) and 5
(macOS) have verified backends; Windows and X11 decline the escalation and the
in-app drag stays alive, which is the expected result there rather than a
failure.

**K4 — the Wayland touch serial.** Row 3, and the failure mode is **silent**: no
drag starts and no terminal event arrives. A finger's outbound drag must be
started with a touch-down serial rather than a pointer-button one; a compositor
that refuses the request produces nothing at all.

**K5 — the coarse auto-scroll band.** Hold a row and drag it to within about
64 dp of the list's edge. The list auto-scrolls. A finger's band is wider than a
mouse's; a band that only engages in the last few pixels means the drag session
is reporting the wrong device.

**K6 — an aborted drag over a second window.** Open two windows of the same app,
start an OS drag, and release it over the second one having moved away. The
session tears down; nothing is left highlighted.

**K7 — X11's promoted touch.** Row 4. An outbound drag reads the core pointer's
button mask, which answers for a finger only because X11 promotes a
pointer-emulating touch onto that pointer. The promotion itself is what a
touchscreen on X11 has to confirm; a suite assertion pins the capability row that
claims it.

## 16. L — Cancellation

Rows: all. Watch the trace for these; the visible half is that nothing is left
half-done.

**L1 — a system grab.** Start a drag with a finger, then trigger something that
takes the pointer away — a notification, a system gesture, a compositor grab.
Trace: `cancelling PointerId(<n>): <reason>`. Visibly: the drag ends, nothing is
left stuck to the pointer, and no `Up` arrives afterwards.

**L2 — the window loses focus mid-press.** Hold a control and alt-tab away.
Every live pointer is revoked in the tree *and* at the translator. Come back: the
control is not stuck pressed.

**L3 — the window is occluded.** Cover the window entirely with another. Same
revocation.

**L4 — a swallowed Up.** After a cancel, lift the finger. Trace:
`swallowing the Up for PointerId(<n>): its press was cancelled`. No tap fires.

**L5 — closing a window with contacts down.** Close the window with a finger
still down elsewhere in it. The translator drains, so the process-global
identities those contacts held are returned. The observable half is that the next
window's first contact does not get a strange identity.

## 17. M — With a screen reader attached

Rows: all, with the platform's screen reader running (Narrator, Orca, VoiceOver).

**M1 — touch still works.** With the screen reader on, every gesture in §7 still
does what it did. An accessibility client attaching must not change the pointer
model.

**M2 — geometry is right at 200 %.** The screen reader's focus rectangle lands on
the control it is reading, at the HiDPI scale. A rectangle offset by a factor of
two is the classic per-rect scaling bug; the framework applies one transform at
the root instead.

**M3 — a density switch is announced once.** In the playground, switch density.
The screen reader says so exactly once — not once per widget, and not silently.

**M4 — explore-by-touch.** Off by default. If you turn it on, dragging a finger
around reads what is under it instead of activating. Record which of the two
modes you tested.

## 18. N — The reviewed-rather-than-tested call sites

These are the six call sites [Touch & pen](touch-and-pen.md) §9 lists as carrying
a platform answer no Linux host can vary, plus the external-drag ones. They are
the reason a sign-off exists at all. Each is a *yes/no* observation rather than a
gesture.

**N1 — the first safe-area read.** Row 5, and only on a macOS window with a
camera housing. Non-zero safe-area insets at window creation; zero everywhere
else.

**N2 — the soft-keyboard-support override.** Row 1. Windows reports `Explicit`;
every other desktop reports none, which is also the trait default — so on any
other row this check can only be "consistent with the default".

**N3 — the on-screen-keyboard poll and its apply.** Row 1. J1 and J2 exercise it;
this row records that the Windows-only path ran at all.

**N4 — the pen pump's per-turn call.** Rows 1 and 2. G1 exercises it. On X11
there is no pen path, so no shim is installed — which is why the host's suite
cannot see this.

**N5 — the close path's contact release.** L5.

**N6 — the Wayland tablet seat publishing tool presence.** Row 2. Add and remove
a stylus from the tablet while the app runs. The poll rate should follow. Without
a compositor that has a tablet manager the call is never reached, and a missing
call leaves the suite green while a real stylus drops to the idle rate — so this
row is the only evidence either way.

**N7 — the winit-loop integration test.** Not a hardware check but recorded here
because it has never been executed: CI's X11 job runs an `#[ignore]`d test under
Xvfb with software drivers, and its wgpu-adapter assumption is reasoned rather
than observed. Record whether that CI step has run by the time of sign-off.

## 19. O — HiDPI

Rows: every row, at its second scale.

**O1 — pixels are physical, everything else is logical.** A screenshot's pixels
are device pixels; every coordinate the framework reports is logical. If you are
driving the app through `teksilo-automation-mcp`, its screenshot reply carries a
`scale` for exactly this reason.

**O2 — a tap lands where you touched.** At 200 %, tap small controls near the
right and bottom edges of the window. An error that grows with distance from the
origin is a scale factor applied once too many or once too few times.

**O3 — D5 and M2** are the other two HiDPI checks; they are listed with their own
sections.

## 20. What to do with a failure

Record it in the sign-off row with: the check id, the platform row, what you saw,
and the trace excerpt around it — the excerpt is the part a maintainer cannot
reconstruct. A trace of the *whole* session is better than a summary; the sample
that mattered is usually three packets before the one that looked wrong.

Do not fix and re-run in the same sitting without recording the original
observation. A check that passed after a change is a different fact from a check
that passed, and the sign-off is a record of the second.

If a check is inapplicable — no such hardware, no such platform feature, the
device does not report that axis — write "n/a" and the reason. A blank is
information; a guessed pass is not.

## See also

- [Touch & pen](touch-and-pen.md) — the pointer model, the platform matrix, and
  §10's ledger of open findings.
- [Events & gestures](events-and-gestures.md) — the arbitration procedure and the
  generated fixture matrix behind §7.
- [Density & targets](density-and-targets.md) — the ladder behind §8.
- [Text touch editing](text-touch-editing.md) — the contract behind §10.
- [Kinetic scrolling](kinetic-scrolling.md) — the simulations behind §9.
- [`touch-verification-signoff.md`](touch-verification-signoff.md) — where the
  results go.
