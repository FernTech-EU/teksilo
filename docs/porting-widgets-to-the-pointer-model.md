<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Porting a widget to the pointer model

**Scope:** the contract a widget has to satisfy to be correct under a finger and
a stylus as well as under a mouse. One numbered clause per obligation, each with
the rule, how to satisfy it, and **what checks it** — because a clause nothing
checks is a suggestion.

**Who this is for:** anyone writing a widget that handles a pointer at all, in
this repository or outside it. Nothing here is specific to `teksilo-widgets`; the
whole contract is expressed against `teksilo-core`'s public surface.

**What you inherit for free.** Identity, capture, arbitration, press state,
cancellation, hit widening, tap counting and the gesture clock are the
framework's. A widget that satisfies these clauses does not implement any of
them. Read [Touch & pen](touch-and-pen.md) for the model, [Events &
gestures](events-and-gestures.md) for the dispatch and arbitration procedure,
and [Density & targets](density-and-targets.md) for the sizing side.

**The invariant every clause is written under.** At `TargetDensity::Compact`
with a mouse, a correct port changes nothing: same latch distances, same
activation instants, same layout, same paint. Each clause below says what a
mouse does, and in every case it is what it did before.

---

## 1. Declare what a direct pointer may do to your subtree

A widget that owns its whole gesture says so, and a widget that scrolls says
so. Nothing is inferred from the handlers you attach.

```rust
// A control whose gesture is the whole press: no ancestor may pan through it.
handlers.touch_action(TouchAction::NONE)

// A surface a finger drags to scroll. One of three spellings:
handlers.scroll_container(PanAxes::Y)              // the sugar
handlers.pan_claim(PanClaim { axes, devices, kinetic: true })   // the explicit form
ScrollableBehavior::new(axes).axes(PanAxes::BOTH).install(handlers) // teksilo-widgets
```

[`TouchAction`](../crates/teksilo-core/src/pointer/touch_action.rs) is
intersected root-to-target and **frozen at the press**; a mouse never consults
it at all. `PanClaim` is what enrols your node as a pan competitor — only for
the pointer kinds its `devices` mask admits, which by default is direct pointers
only, so a mouse never pans.

Two mistakes the shipped code has already made, both worth stating:

* **An `on_scroll` handler does not make you a pan surface.** `SpinBox`
  increments on a wheel notch, `TabBar` remaps a vertical notch to horizontal,
  `SceneView` zooms on Ctrl-wheel. None is a pan, and none may absorb a finger
  that is trying to scroll the container around it. This is why pannability is
  declared and never inferred — and why the claimant walk visits only the frozen
  claimant list and never the ordinary bubble.
* **A narrower action than you need is a silent bug.** `TouchAction::NONE` on a
  control that only wants to stop a *pan* also forbids a pinch and every delayed
  gesture through it. Declare the narrowest thing that is true: `PAN_Y` says "a
  vertical pan may pass through me", not "I pan".

Declarers of `NONE` in the shipped catalogue: `Slider`, the `ColorPicker` alpha
strip and its HSV canvas, `WebView` (an engine subview owns its own gestures) —
and, outside the catalogue, the Inspector's Pointers tab. Pan claimants:
`ScrollArea` plus, through `ScrollableBehavior`, the five data views and the three
text surfaces — nine surfaces from one implementation; `Terminal` and `SceneView`
declare their own claims directly, because neither scrolls a pixel offset in
`[0, max]` (a terminal's is a ring position quantised to lines, a scene's is a
camera).

**Checked by** `crates/teksilo-core/tests/arbitration_matrix.rs` (a claimant's
axis filter, and the `NONE` rows), and by `spin_box/tests.rs`'s
`a_finger_pan_over_a_hover_wheel_spin_box_scrolls_its_container` for the
inference trap.

## 2. A press is not an activation, for a direct pointer

Commit on the **release**, and only when the release still belongs to the press
that opened it:

```rust
.on_pointer_event(|event, ctx| match event {
    WidgetEvent::PointerDown { .. } if ctx.pointer_kind().is_direct() => {
        // Decide, do not apply.
        EventResponse::Ignored
    }
    WidgetEvent::PointerUp { .. } if ctx.press_is_inside() => {
        apply(ctx);
        EventResponse::Handled
    }
    _ => EventResponse::Ignored,
})
```

`EventContext::press_is_inside` is the whole predicate. It is false once the
pointer has left the press's tap boundary — the same predicate that fails the
tap and fires `cancel_taps` — and false once a peer has claimed the sequence, so
a press that turned into a scroll commits nothing. It stays **true** through a
press-feedback delay: a finger that lands and lifts inside 100 ms has still
clicked; the delay withholds the *visual*, not the press.

No release-time predicate can rescue a value already written on `PointerDown`.
That is the whole reason the write moves rather than being guarded.

**Three things stay on the press, and the boundary is worth knowing** so you can
tell which side of it your widget is on. A **continuous manipulator** whose
output *is* the press position — a splitter divider, a scroll-bar thumb, a
slider, a text-selection anchor — has nothing to defer, because deferring would
make it jump rather than drag. An **auto-repeat** needs a press to start
counting from. And an interaction the **OS** takes over from the press onward —
`begin_resize` hands the window to the compositor's own resize loop, which never
returns a release — has no release to move to. Everything else defers. The full
census, entry by entry, is in
[touch-and-pen.md §7.3](touch-and-pen.md).

**Checked by** each widget's own release test; the shape to copy is
`deferred_select` in `crates/teksilo-widgets/src/data_views.rs`, whose
`on_down` / `on_up` pair is shared by all five data views.

## 3. `PointerCancel` is terminal

```rust
.on_pointer_cancel(|pointer, reason, ctx| {
    // Unwind. Nothing may activate.
})
```

No `PointerUp` follows a cancel for that pointer, and one that arrives anyway is
swallowed. A widget that only unwinds on `PointerUp` leaks its interaction state
the first time a modal opens, a window loses focus, an OS drag starts, or a peer
claims the sequence. The reasons are enumerated —
[`CancelReason`](../crates/teksilo-core/src/pointer.rs) has fifteen variants and
is `#[non_exhaustive]`, so match with a `_` arm.

The distinction from `PointerUp` is semantic, not cosmetic: an Up means the user
finished, so a drag drops and a tap fires; a cancel means the interaction is
being taken away, so state is unwound and **nothing may activate**.

You do not have to release capture, clear the press visual, stop a fling you
started, or drop the sequence — the funnel
([`WidgetTree::cancel_pointer`](../crates/teksilo-core/src/widget_tree/pointer_cancel.rs))
does all of that before your handler runs. What is yours is the state the
framework cannot see: a half-built preview, an anchor, a pending edit.

**Checked by** `assert_no_leaked_pointer_state()` after every sequence in the
core suites, and by one test per producer.

## 4. Branch on `ctx.pointer_kind()`, never on a global

```rust
if ctx.pointer_kind().is_direct() { /* finger or stylus on glass */ }
if ctx.pointer_kind().is_coarse() { /* a contact patch, not a hot-spot */ }
if ctx.pointer_kind().hovers()    { /* mouse, or a pen in proximity */ }
```

There is no "touch mode". A hybrid machine has a mouse, a touchscreen and a pen
live at the same time, each with its own identity, its own capture and its own
gesture profile, and the kind that matters is the kind of the pointer being
dispatched *right now*. Read it from the context rather than from the event, so
a handler that only needs "was this a finger?" does not have to destructure
`PointerInfo`.

Pick the predicate by what you are deciding, not by habit:
`is_direct()` for *what the gesture means* (a finger's press is the opening
sample of a possible scroll); `is_coarse()` for *geometry* (a contact patch
occludes what it touches, a hot-spot does not); `hovers()` for *reachability*
(see clause 6). A pen is direct **and** precise **and** hovers, which is why one
flag cannot answer all three.

Two dispatches carry a pointer without carrying a sample, and both report a
truthful one: a gesture the timer recognised reports the **contact that held**,
and a drag-and-drop handler reports the pointer **that started the drag**.
Outside any pointer, scroll, gesture or drag dispatch — an assistive-technology
action, a hand-built test context — `pointer()` is the mouse at the tree epoch,
which is the answer every such handler got before pointers were
distinguishable.

## 5. Gate every `PointerMove` arm on `owns_pointer()`

A widget that drives its interaction from `PointerMove` after an explicit
`ctx.capture_pointer()` must confirm it still holds the pointer:

```rust
WidgetEvent::PointerMove { position, .. } => {
    if !dragging.get() || !ctx.owns_pointer() {
        return EventResponse::Ignored;
    }
    // …
}
```

Capture is **per pointer** and it is also an arbitration act: capturing from an
undecided sequence enrols you as a `RawDrag` member, and you can lose — to a
peer that claims, to a drag-capable ancestor, to a cancel. Your own `dragging`
flag records that you *started*; `owns_pointer()` is the only thing that says
you still own the gesture. A second contact arriving elsewhere in the tree also
generates moves, and without the gate they reach an arm that assumes they are
yours.

Shipped captors, all four gated: `splitter/handle.rs`,
`docking/resize_handle.rs`, `table_view/header.rs`,
`teksilo-inspector/src/resize_handle.rs`.

## 6. Expect no hover, ever, for a contact

`on_hover`, `hover_within`, `PointerEnter` / `PointerLeave`, the cursor, and
tooltip dwell all follow the **hover owner** — the most recent pointer that can
hover, which is a mouse or a pen in proximity. A contact is never the hover
owner and writes no hover state at all.

So an affordance that only appears on hover is **unreachable** with a finger.
Every such affordance needs a second route, and the framework provides three:

* a **hold** — `long_press_role(LongPressRole::Tooltip | ContextMenu)` selects
  what the tree-owned long-press route does with your subtree, and your own
  `on_long_press` always takes precedence over it;
* **always-visible** at coarse densities — `InputTokens::reveal` is
  `RevealPolicy::Always` at `Touch` for exactly this reason;
* an **overflow affordance** with a keyboard and an assistive-technology route,
  which is what a row's `⋮` is.

The census of every hover-gated affordance in the framework, with the route each
one took, is [hover-affordance-census.md](hover-affordance-census.md).

## 7. Route dimensions through `dp`, `density_min_size` and `spacing`

```rust
use teksilo_core::styles::density::{density_min_size, dp, spacing};

let h = dp(28.0, TargetRole::Target, &ctx.theme.input);      // 28 / 32 / 44
let s = density_min_size(base, TargetAxes::HEIGHT, tokens);  // one or both axes
let gap = spacing(8.0, tokens);                              // ×1.00 / 1.15 / 1.30
```

The tokens are `theme.input`, reached as `ctx.theme.input` from `layout_response`
and `paint` (where the theme is a field on the context) and `ctx.theme().input`
from `build`.

**`dp` is a floor, not a scale.** It is `base.max(target_size)`, so routing a
dimension whose Compact value is *below* 24 dp through it **raises that
dimension at Compact** and breaks the invariant this whole document is written
under. The rule, enforced by a test that parses
[density-inventory.md](density-inventory.md):

> A `Target` dimension routes through `dp` only if its Compact value already
> clears 24 dp. Everything below that keeps its paint at every density and takes
> its conformance from the hit mechanisms in clause 8.

A `TargetRole::Decoration` — a rule, an icon, a badge — is returned unchanged
and must never be routed as a target.

Two further consequences of the floor, both of which have already cost a package
a rewrite:

* Once a control's own box is projected **to** `target_size`, its miss-only slop
  top-up is exactly zero. The slop pass serves controls that stay small; it is
  not a fallback for one the projection has already grown.
* A density assertion must be an **equality**, or must say why a bound is
  enough. `>=` passes for the wrong reason more often than it fails for the
  right one — a root row handed the whole viewport satisfies any floor at every
  density.

## 8. Implement the hit hooks your shape actually needs

Four hooks, four disjoint jobs. Implement the ones that describe your widget and
none of the others.

| Hook | Answers | Implement when |
| --- | --- | --- |
| `hit_shape(local, bounds) -> bool` | is this point inside my silhouette? | your paint is not your rectangle — a disc, a wedge, a rounded handle |
| `hit_distance(local, bounds) -> Option<f32>` | how far outside am I? | same, *and* you want near-misses measured to the shape rather than to the box |
| `hit_outset(kind, tokens) -> EdgeInsets` | how far beyond my bounds do I still take a press? | you are a thin **grip** — a gutter, a divider, a resize edge |
| `target_regions(bounds) -> Vec<TargetRegion>` | which targets do I paint inside my one node? | you draw several controls on one canvas — a thumb in a lane, a knob on a track, a label beside a filter |

Three things about them that are easy to get wrong:

* **`hit_outset` is zero for a precise pointer** unless you deliberately say
  otherwise. A mouse hot-spot is exact and occludes nothing, so widening its
  targets steals clicks from neighbours. Check `kind.is_direct()`.
* **`hit_outset` never escapes its parent.** It is consulted among a node's
  direct children, inside a recursion that has already tested the parent's own
  bounds — so a grip whose wrapper hugs it claims nothing at all. If the outset
  has to reach, the parent must be big enough to lend the space.
* **`target_regions` is reporting only, and its one consumer today is the
  audit.** Implementing it changes no layout and no hit test — the router does
  not route by it. What it buys is that the conformance audit can *see* the
  targets a widget paints inside its own node; without it those targets do not
  exist as far as any gate is concerned, and a 12 dp thumb inside a 200 dp lane
  measures as a 200 dp target. Build the rects with
  [`partition_targets`](../crates/teksilo-core/src/partition.rs) where the split
  is a division of one axis, so the geometry you paint and the geometry you
  report cannot drift.

**How to know your hook is doing anything.** `hit_outset` and the framework's
own miss-only slop pass are *redundant* in the geometry a naive test builds:
both deliver a near-miss press, and a subject sitting in a bare stack has no
eligible handler on its bubble path, so the slop pass catches the press whether
or not your outset exists. Five of seven `hit_outset` implementations in one
package initially passed their own deletion for exactly this reason. The
discriminating fixture puts an **eligible bubble owner** on the path — a
tappable row containing the control, which is what a swatch in a picker row or a
chevron in a tree row actually is — because the rule then requires a slop
candidate to be *strictly closer* than the row, which a mere near-miss is not.

**Checked by** `crates/teksilo-core/src/widget_tree/hit_targeting_tests.rs`, and
by the fixture-driven `tests/target_conformance.rs` in `teksilo-widgets`,
`teksilo-charts` and `teksilo-scene`. Note the boundary: those three crates have
fixture lists and four others that own targets do not
(`teksilo-inspector`, `teksilo-terminal`, `teksilo-preview-ui`,
`teksilo-webview`), and the lists install the IntUI preset only. A green gate says
three crates' named fixtures conform — not that the framework does. The table is
in [accessibility-internal-audit.md §3.7](accessibility-internal-audit.md).

## 9. Never read the wall clock in a recognizer

Time arrives in [`RecognizerContext::now`](../crates/teksilo-core/src/gesture/config.rs),
an `EventTime` measured from the one tree epoch. Thresholds arrive in the same
context as `profile`, the `GestureProfile` for *this pointer's kind*. A
recognizer therefore holds no thresholds and reads no clock, which is what lets
a test install a `ManualClock` and have a long press, a double-tap window, a
fling and an animation resolve exactly when it says, with no sleeping.

`Instant::now()` is banned from the gesture layer outside its own test blocks,
and a source scan enforces it: `no_wall_clock_in_gestures` in
[gesture.rs](../crates/teksilo-core/src/gesture.rs), with two further tests
asserting that the scan's own source stripper has not eaten the file it was
meant to read.

The same rule reaches beyond recognizers: a widget that needs a deadline asks
the tree for one rather than sampling a clock, so
`WidgetTree::next_input_deadline` can fold it into the single
`ControlFlow::WaitUntil`.

## 10. Two positional fields are window-space, and their names say so

Every position a handler receives is **widget-local** — the router localises on
delivery — with exactly two exceptions, both named for it:

* `WidgetEvent::Scroll::window_position`
* `WidgetEvent::PointerCancel::window_position`

Both are window-logical **by contract**, because both are routing and velocity
coordinates rather than content coordinates. The router routes by the first, and
routing is necessarily window-space. And a pan's samples feed a velocity tracker
that follows the *pointer*: localisation resolves against the captor's
**current** bounds on every event, so a localised position would credit that
tracker with the motion of the very widget being measured — reachable on a
chained pan, where the inner claimant is offered every sample while the outer
absorbs and translates it.

**A widget that needs content coordinates converts at the use site**, from its
own bounds. `Terminal` learned this the expensive way: it read the field raw and
reported the wrong cell to the child application, reproducing a defect its own
comment records having made once already. The frame is in the name now because a
name is checked by the compiler at every read and a paragraph like this one is
checked by nobody.

Note the asymmetry at the door: `ScrollSample::position`, the *ingress* type, is
plain `position` — at the door there is only one frame it could be in. The
rename is on the delivered event, where a handler could mistake it for a local
point.

**Checked by** `scroll_and_cancel_stay_in_window_space_while_a_press_is_localised`
(in [`pointer_state.rs`](../crates/teksilo-core/src/widget_tree/pointer_state.rs)),
so a later "consistency fix" goes red instead of quiet. Its two assertions are
guarded by two *different* mechanisms — `Scroll` by `localize_event` having no arm
for it, `PointerCancel` by the cancel funnel recording the table's own position
verbatim on a route that never localises at all — and the test's own doc says
which is which, because neither can stand in for the other.

## 11. One contact by default; a hold has a tree-owned meaning

`MultiContact::First` is the default and is what every widget written before the
pointer model assumes: your node serves its first contact, and an **extra**
contact is terminated at your node — not delivered to you, and not bubbled to an
ancestor either. Two fingers on one button fire one tap.

What that does *not* do is silence the second contact. Capture, arbitration and
press ownership are per pointer, so the refused contact still opens a sequence
of its own and can still win an enclosing pan claim — a second finger inside a
scroll area scrolls it while the first goes on holding the button. Opt into
`MultiContact::All` only for a genuine multi-touch surface (a pinch canvas, a
keyboard of keys).

A **hold** that you do not handle is not wasted: the tree's own long-press route
opens a context menu or shows a tooltip, resolved from what your subtree offers.
`long_press_role(..)` picks; `LongPressRole::DragHandle` says the hold *is* your
grab, and suppresses long-press recognition on your subtree — declare it when
your grab is one the framework's own deferral cannot see, i.e. an explicit
`capture_pointer` rather than a drag recognizer.

## 12. A new `Widget` hook has to be forwarded, and that is now a compile error

`WidgetWithHandlers<W>` and the `TeksiBranch` family forward the `Widget` trait
method by method. A hook missing from those lists is **silently inert** the
moment any builder method touches the widget — which is every widget that takes
an attached handler. This was a real defect twice: `hit_shape` was missing from
the list, and later both `hit_shape` and `hit_distance` could be *deleted* from
it with thousands of tests green, because every test called the hooks on a bare
struct.

All four impls now `#[deny(clippy::missing_trait_methods)]`, so an
unforwarded method fails the lint rather than passing the suite — including a
method that does not exist yet. A new hook still owes a test that reaches it
**through** a builder method, because the lint proves the forward exists and not
that it forwards to the right place.

---

## Checking a port

The test API ([`widget_tree/test_api.rs`](../crates/teksilo-core/src/widget_tree/test_api.rs))
drives every kind of pointer headlessly on a manual clock — no display server,
no GPU, no sleeps:

```rust
let mut tree = WidgetTree::new();
tree.set_density(TargetDensity::Touch);
let id = tree.add(subject);
tree.layout(SizeProposal::exact(400.0, 300.0));

let finger = tree.new_contact();                        // mint an identity
tree.touch_down(finger, Point::new(50.0, 50.0));
tree.touch_move(finger, Point::new(50.0, 90.0));
tree.touch_up(finger, Point::new(50.0, 90.0));
tree.assert_no_leaked_pointer_state();
```

`new_contact()` mints a fresh `PointerId` the way the platform allocator does —
one per press, because a backend reuses its own contact ids the moment a finger
lifts. Reusing one across two presses is not a shortcut; it is a different
scenario.

Also available: `pen_down` / `pen_move` / `pen_up` / `pen_hover`,
`tap_with(kind, point)`, `long_press_at`, `touch_drag`, `fling`, `pinch`,
`touch_cancel`, `set_density`, `advance_time`, and the arbitration queries
`sequence_winner` / `sequence_members` / `touch_action_for` / `live_pointers` /
`is_pressed`.

Two habits that decide whether your test is worth having:

* **Delete the mechanism and confirm the test goes red.** Reading cannot tell a
  test that pins a mechanism from a test that merely happens to pass; only
  mutation can. Every finding in the list above was found that way.
* **A test that focuses a node and dispatches a key proves the handler runs, and
  says nothing about reachability.** `WidgetTree::focus(id)` does not check that
  the node is focusable. A claim of keyboard reachability must assert that the
  node appears in `test_api::tab_stops_within(root)`.

## See also

- [Touch & pen](touch-and-pen.md) — the pointer model, the clock, the platform
  seam, the press.
- [Events & gestures](events-and-gestures.md) — dispatch, recognizers, the
  sequence and the ordered decision procedure.
- [Density & targets](density-and-targets.md) — the three ladders, the three hit
  mechanisms, the conformance audit.
- [Kinetic scrolling](kinetic-scrolling.md) — adopting `ScrollableBehavior`.
- [Touch text editing](text-touch-editing.md) — the host checklist for a text
  surface.
- [Single-pointer alternatives to dragging](a11y/non-drag-alternatives.md) — the
  obligation a draggable widget carries.
