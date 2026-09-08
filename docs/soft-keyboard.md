<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Soft keyboard

A finger landing in a text field is the one input the desktop has no answer
for. There is no physical keyboard behind it, and nothing in the ordinary focus
path summons a soft one. This page is what each desktop platform will do about
that, what the framework promises, and what it deliberately refuses to do.

The short version: **on three of four desktop platforms the framework will not
raise a keyboard, and that is a contract, not a gap.** A text surface built for
touch has to know which it is on.

---

## 1. The capability

```rust
pub enum SoftKeyboardSupport { None, ViaAccessibility, Explicit }
```

Read it from a handler:

```rust
if ctx.soft_keyboard_support() == SoftKeyboardSupport::None {
    // Nothing will rise. Offer an in-app affordance instead.
}
```

The three answers differ in **what a caller may promise a user**, not in how
much code stands behind them.

- **`None`** — the framework has no keyboard request to send, and makes no
  promise that anything will rise on its own. Either the platform has no
  software keyboard at all, or it has one whose appearance is the platform's
  business and not reliable enough to promise. A request is dropped. A text
  surface that expects a finger has to offer its own affordance.
- **`ViaAccessibility`** — a keyboard exists and is *guaranteed* to rise when a
  text control takes focus through the accessibility layer, so a touch-driven
  text surface needs no affordance of its own. There is still no request to
  make: the framework's ordinary IME-allowance reconcile is what summons it,
  and an explicit ask would at best duplicate that and at worst cancel a live
  composition (§3). The guarantee is the whole difference from `None`.
- **`Explicit`** — a keyboard exists and can be shown *and hidden* on demand.
  Only a backend that can honour **both** directions may report this. A toggle
  whose current state is unknown cannot, because "show" would sometimes hide.

`BackendCaps::osk` carries the same value, and `soft_keyboard::support_for` is
the pure function behind both — so every row is asserted from any host,
including the two a Linux runner cannot boot.

## 2. Per platform

| | answer | why |
| --- | --- | --- |
| Windows | `Explicit` | `ITipInvocation::Toggle` on the `UIHostNoLaunch` coclass, plus a visibility probe |
| macOS | `None` | no client-facing request exists |
| Wayland | `None` | the `zwp_text_input_v3` version winit binds has no request to make, and no panel can be promised |
| X11 | `None` | no request in the core protocol, XInput2 or EWMH |

### Windows

The touch keyboard is `TabTip.exe`, reached through the undocumented
`ITipInvocation` COM interface. Its one method is `Toggle` — there is no *Show*
and no *Hide* — so honouring both directions means finding the keyboard's own
window (`IPTip_Main_Window`, or the `ApplicationFrameWindow` that hosts it on
Windows 10 and later), asking whether it is visible, and poking the toggle only
when the state is the wrong one. That decision is
`soft_keyboard::should_toggle`, a pure function, and it is what makes
`Explicit` honest here rather than a toggle wearing a promise.

Windows also raises the keyboard for a UIA text pattern under touch focus, so
the accessibility path works too; the probe is what stops the two from fighting.

> **Verification status.** Written against the `windows` crate 0.62 API and the
> published GUIDs, and not yet exercised on a Windows host. Windows 11 22H2
> changed the touch-keyboard model, so this is a hardware sign-off item.
> Everything decidable without an OS — the capability row, the toggle decision,
> the request resolution — is unit-tested.

### macOS

No API lets a desktop app raise a software keyboard. The Accessibility Keyboard
is a user setting under System Settings ▸ Accessibility ▸ Keyboard and has no
client-facing request. `NSTextInputClient` raises the IME *candidate* window,
which is not a keyboard.

### Wayland

This is the answer worth spelling out, because it is not the one you would
guess.

`zwp_text_input_v3` **as winit speaks it** has no way to ask. The interface's
version 1 — the version winit 0.30 binds
(`globals.bind(queue_handle, 1..=1, ..)`) — has no `show_input_panel` request:
v1 and v2 of the *protocol family* had one, v3 dropped it, and version **2** of
the v3 interface has since added `show_input_panel` / `hide_input_panel` back
(they are in the `wayland-protocols` this workspace ships). Nothing in winit
0.30 binds that version or exposes the requests, so from where Teksilo stands
there is no explicit verb to send, and what a panel does instead is follow the
`enable` + `commit` pair the framework already issues when a text widget takes
focus.

That is the shape `ViaAccessibility` describes, and Wayland still does not get
that row — because that row is a *guarantee*, and this is not one.

Mutter needs that pair **twice** before it shows the panel
([GNOME/mutter#1506](https://gitlab.gnome.org/GNOME/mutter/-/issues/1506)), and
winit 0.30 sends it exactly once per `set_ime_allowed(true)` — so a GNOME
session can end up with a focused field and no keyboard. Teksilo does not work
around it, and the reason is not laziness:

- The only reachable second `enable` is a second `set_ime_allowed(true)`, and
  `enable` is specified to reset "the state associated with `preedit_string`,
  `commit_string`, and `delete_surrounding_text` events". It destroys a live
  composition.
- Binding a second `zwp_text_input_v3` of our own — the pattern the pen and
  drag-and-drop backends use to reach the compositor past winit — does not
  help *for the double enable*. The protocol says requests to enable a text
  input while another is enabled on the same seat must be ignored.

So the choice is between a keyboard that sometimes does not appear and a
composition that sometimes vanishes mid-word. This is the side of it that loses
no user data — and `None` is the capability row that matches it. A widget that
offers its own affordance is right on the session where nothing rises, and
merely redundant on the session where something does; a widget told
`ViaAccessibility` would offer nothing, and on GNOME the field would be
unreachable.

**The route to `Explicit` on Wayland, when it opens.** Bind our own
`zwp_text_input_manager_v3` at interface version 2 on winit's display — the
pattern `pen/wayland.rs` already uses — and send `show_input_panel` /
`hide_input_panel`, which are ordinary requests and not the `enable` the
seat serialises. That is not done here because the value of it turns entirely
on compositor support for a version-2 interface that is new, and a capability
row is a promise: `Explicit` may not be claimed on the strength of a request
the compositor is free to ignore. Verifying it is a hardware sign-off item, not
a code change.

### X11

On-screen keyboards are separate clients driven by AT-SPI or by the user. There
is no client request in the core protocol, in XInput2, or in any EWMH hint.

## 3. Why a request never re-asserts IME allowance

`EventContext::request_soft_keyboard` records a request; the app layer applies
it once per dispatch, **after** its IME-allowance reconcile. That order is the
whole rule.

On a platform whose keyboard follows the IME enable, "asking" means
re-asserting allowance — and re-asserting allowance is what destroys a live
composition. So the request resolves to nothing there, and nothing on that path
calls `set_ime_allowed`. Placing a caret with a finger while a composition is
in flight keeps the preedit because the code that would have destroyed it is
not reachable from the request.

`soft_keyboard::resolve` is that decision as a pure function, and it is tested
per capability row. `Explicit` has no such hazard — its request goes to the
keyboard's own control, not through the IME channel — so it is passed through,
and the "is it already up?" question belongs to `should_toggle` inside the
platform call.

**How the guarantee is stated.** A promise about a call that must *not* happen
can only be tested where that call would have been visible, so the two winit
pushes (`set_ime_purpose` / `set_ime_allowed`) are behind
`input_loop::InputChrome::set_ime` rather than written inline in the event
loop. `input_loop::settle_ime` is the reconcile and the request in one function,
in that order, and the assertion is that a turn at a focus that has not moved —
which is every turn of a live composition — pushes **nothing** at all. The
alternative, asserting that a document still reads `"ni"` after the request,
holds no matter what the code does, because `apply_soft_keyboard_request` has no
route to a `TextDocument` in the first place.

## 4. The occluded band

A keyboard covers part of the window. `WidgetTree::set_occluded_inset` is
where that rectangle is reported, and overlay placement keeps the largest free
slab: a
`Centered` modal recomputes against the band above the keyboard, and pins to
the top of it when it is taller than the band rather than sliding off-screen.

A **rectangle**, not a named edge, because that is what a platform reports and
because a candidate window docked to a side is the same problem with a
different geometry.

Scope: this reaches **overlay placement only**. The root layout proposal is
still the whole window, so a keyboard rising does not reflow the document
behind it — which is what the desktop convention wants, and what keeps a
keyboard appearing from being a full relayout. Bringing a focused field out
from behind the band is a scroll against `WidgetTree::usable_viewport`, not a
resize.

Where the rectangle comes from is the same undocumented window lookup as §2's
Windows path, and for the same reason it exists nowhere else: macOS has no
keyboard to find, `zwp_text_input_v3` has no event that carries the input
panel's geometry at all, and under X11 the keyboard is an unrelated client with
no hint saying where it is.

Nothing notifies us when it moves or goes away, so `teksilo-app` re-reads it —
at most four times a second, and only on an event-loop turn it was awake for
anyway. An idle app never looks, and an idle app is not placing overlays. The
consequence to state plainly: a keyboard the *user* dismissed is noticed within
that quarter second rather than immediately.

## 5. The safe area, which is the same shape of problem

A window owns a rectangle; it does not always get all of it. A display cutout
eats the top, a rounded corner clips the corners, a home indicator reserves the
bottom. `teksilo_platform::safe_area` reads it and `teksilo-app` hands it to
`WidgetTree::set_safe_area` when a window is created and after every resize and
scale change, from where it reaches the same overlay viewport as the occluded
band.

Only **macOS** reports one on the desktop (`NSView.safeAreaInsets`, macOS 11+),
and there only when the window covers the camera housing — in practice, full
screen on a 14"/16" MacBook Pro. Windows has no cutout and no client-area
inset API; neither Wayland nor X11 carries a display cutout in any stable
protocol. Those three zeroes are answers, stated at the branch that returns
them.

`SafeAreaSides` names physical edges — `left` and `right`, not `leading` and
`trailing` — because that is what a platform reports, and it is the widget
tree, which knows the layout direction, that decides which is which.

## 6. What this does not do

- It does not decide **when** to ask. A touch landing in a text field is a
  question about the touch text contract, not about the keyboard, and no widget
  calls `request_soft_keyboard` yet.
- It does not learn instantly that the user dismissed the keyboard by hand;
  see the polling note in §4.
- It does not raise a keyboard on `None`, and it will not pretend to.

## Source

- `crates/teksilo-platform/src/soft_keyboard.rs` — the capability, the
  resolution, the toggle decision, the Windows path
- `crates/teksilo-platform/src/safe_area.rs` — `SafeAreaSides` and the per-OS
  read
- `crates/teksilo-core/src/window/ops.rs` — `SoftKeyboardSupport`,
  `WindowOps::soft_keyboard_support`
- `crates/teksilo-core/src/overlay/viewport.rs` — how the two insets become one
  usable rectangle
