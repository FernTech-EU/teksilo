<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Touch text editing

A mouse edits text with a cursor that is one pixel wide, hovers before it
commits, and has a second button for a menu. A finger has none of that. It
covers the character it is aiming at, it cannot hover, it has no second button,
and the caret it just placed is under the fingertip that placed it. Every
platform answers with the same three pieces of chrome — **selection handles**, a
**magnifier**, and a **selection toolbar** — and Teksilo's version of them lives
in [`teksilo_core::text_touch`](../crates/teksilo-core/src/text_touch.rs).

The mouse path is untouched. Every **pointer** entry point begins by asking
`PointerKind::is_direct()` and returns `Ignored` without reading or writing any
state when the answer is no, so an editor that installs the controller edits
byte for byte as it did before. `on_long_press` asks the same question and
reaches the wrong answer, for a reason that has nothing to do with the mouse and
everything to do with where a long press comes from — see checklist step 3, where
the guard the host has to write instead is set out. A **stylus is direct** and
gets the whole affordance set: a pen selects text the way a finger does.

## Where the pieces live

| Piece | Crate | Why there |
| --- | --- | --- |
| `TextHitSource`, `TouchSelection`, `TextAffordances` | `teksilo-core` | The terminal selects text (read-only) and deliberately does not depend on `teksilo-widgets`. An embedded `WebView` is *not* a consumer — its engine owns the page's selection and paints its own handles. |
| `TextAffordanceLayer`, `SelectionHandle`, `TextMagnifier` | `teksilo-core` | Same reason, and they are single-node batched paint — nothing about them needs the widget catalogue. |
| `PaintContext::replay` | `teksilo-core` | The magnifier's mechanism; see below. |
| `TextSelectionStyle`, `TextSelectionHandleRecipe`, `TextMagnifierRecipe` | `teksilo-core` | The Tier-3 protocol, alongside every other style trait. |
| `RecipeTextSelectionStyle` | `teksilo-widgets` | The shipped IntUI look, like every other `Recipe*Style`. |
| The selection toolbar | the host's own menu rows | It *is* a context menu: same commands, same rows, one implementation of each. The controller decides which commands to offer and where to hang them; it does not paint a second menu widget. A host that raises it through `show_overlay_in_band` keeps the caret in the editor, at the cost of the menu's keyboard route — see checklist step 5. |

## The controller contract

```rust
trait TextHitSource {
    fn offset_at(&self, point: Point) -> usize;
    fn caret_rect(&self, offset: usize) -> Rect;
    fn word_range_at(&self, offset: usize) -> Range<usize>;
    fn line_range_at(&self, offset: usize) -> Range<usize>;
    fn selection(&self) -> Range<usize>;
    fn set_selection(&mut self, range: Range<usize>);
    fn selection_bounds(&self) -> Option<Rect>;
    fn viewport(&self) -> Rect;
    fn document_len(&self) -> usize;
    fn is_editable(&self) -> bool;
    fn allows_copy(&self) -> bool { true }            // a password field says no
    fn clipboard_actions(&self) -> ClipboardActions { /* derived */ }
}
```

Every point and rectangle is in **window** logical coordinates — the space an
overlay is positioned in, and the space a `PointerDown` arrives in *at the tree*.
It is not the space a handler receives one in: the router localizes before
dispatch, so a host converts on the way in and on the way out. See
[Coordinates](#coordinates-and-the-two-conversions-a-host-needs) for the two
conversions and why they differ. Anywhere else and the handles disagree with the
caret the moment the editor is scrolled.

`clipboard_actions` has a derived default: Cut needs an editable surface, a
selection and permission to copy; Copy needs the last two; Paste needs the
first; Select All is offered only while nothing is selected. A read-only surface
therefore ends up with a **Copy-only** toolbar, and a password field with neither
Cut nor Copy while it is masked, without either case being special-cased
anywhere. That second answer depends on the host resolving `allows_copy` from the
predicate its own Cut and Copy consult rather than from a developer opt-in flag —
see checklist step 1.

### What it does not answer

`TextHitSource` is geometry. *Performing* a command — cut, paste, undo — is
[`TextSurface`](../crates/teksilo-core/src/text_surface.rs), which every text
widget already registers. The two meet only at `clipboard_actions`, which says
which commands to **offer**, so there is exactly one implementation of each.

## Coordinates, and the two conversions a host needs

The contract is window coordinates. **The router hands a handler
widget-local ones**: `localize_event` rewrites every pointer position, and
`localize_gesture` every `TapEvent` position, through
`WidgetArena::local_pointer_position` before the target sees it. A host that
forwards what it was given puts each affordance one viewport origin away from
the text it marks. Two conversions, and they are not interchangeable:

* **A pointer sample** uses `EventContext::pointer_position`, which is the
  window position of *the sample being dispatched*. This is the only source that
  stays correct for the affordance widgets: those are placed on the handle
  geometry, so they move while they are being dragged, and a
  local-plus-origin conversion would have to guess which placement the router
  localized against — the published geometry can already be a sample ahead of
  the arena bounds when two moves land in one frame.
  `tree_pointer_position` is **not** a substitute: it reports the pointer
  table's elected primary, which prefers the mouse, so on a machine with both it
  answers for the wrong device.
* **A long press** has no sample — it is recognised by a timer, and
  `pointer_position` is `None` there — so it converts `local + viewport_origin`.
  That is exact, because the *editor* does not move mid-press.

## Host checklist

Written before any host existed; corrected in place by the single-line family,
which was the first (`teksilo-widgets/src/primitives/text_input_field/touch.rs`).
Where a step is a correction, it says what it used to say.

1. **Implement `TextHitSource`** over the state the editor keeps, not over the
   command handle it registers as a `TextSurface`. Every controller entry point
   wants `&mut TouchSelection` and `&mut dyn TextHitSource` at once, so a
   controller stored *inside* that state needs two overlapping borrows of one
   `RefCell` on every call: the controller is a sibling of the state, and the
   `TextHitSource` is a short-lived wrapper around a borrow of it.

   `allows_copy` is the predicate the surface's **own** Cut and Copy consult at
   the moment they run — not a developer opt-in flag that a reveal toggle
   overrides. Otherwise the touch toolbar refuses a Copy the same surface's
   keyboard and context menu allow.

   A one-line surface should **pin the vertical coordinate** to its line in
   `offset_at`. Affordances hang deliberately off the line — a handle's disc sits
   a dozen dp below the descender — so hit-testing a point at its own `y` misses
   the glyph row for every `x` and lands at the document end for all of them.

2. **Own a `TouchSelection`.** Build it in `build()`, not in a handler:
   `reduced_motion` has no accessor on `EventContext`, so the preference has to
   be captured while a `BuildContext` is in hand. Pass it the handle metrics
   **and the lens metrics** from the active style, so the rectangles it computes
   are the ones the layer paints:
   ```rust
   let style = ctx.theme().style_slots.text_selection.clone()
       .unwrap_or_else(|| Rc::new(RecipeTextSelectionStyle::for_tokens(&ctx.theme().input)));
   let handle = style.handle(ctx.theme());
   let lens = style.magnifier(ctx.theme());
   let controller = TouchSelection::new()
       .metrics(HandleMetrics::from(&handle))
       .magnifier_metrics(lens.radius, lens.half_height, lens.rise, lens.scale)
       .reduced_motion(ctx.prefers_reduced_motion());
   ```
   The `magnifier_metrics` line is the correction: without it the controller
   computes a lens from this module's own constants while the widget frames one
   from the style's, and a theme that resizes the lens gets two different
   rectangles.

3. **Wire the host's own pointer arms, and read the pointer kind off the
   gesture.** This step used to say "forward `handle_pointer` from the editor's
   own `on_pointer_event`, and `on_long_press` from its `on_long_press`. Both are
   inert for an indirect pointer, so neither needs a guard of its own." Both
   halves are wrong.

   `TouchSelection::on_long_press` gates on `EventContext::pointer_kind`, and on
   the long-press path that answer is **the mouse whatever the device was**: a
   long press is recognised by the gesture timer rather than by a pointer sample,
   and the tree's in-flight input snapshot is saved and restored around every
   dispatch, so by the time the timer runs it is back to its default — the mouse
   at the tree epoch, which `EventContext::pointer_kind`'s own documentation
   says. The truth arrives on `TapEvent::pointer`, which no `on_long_press`
   signature in core can see. So the host reads `event.pointer.kind.is_direct()`
   and drives `TouchSelection::raise`, which has no guard of its own, and
   `on_long_press` is unreachable from such a host.

   `handle_pointer`'s release arm raises unconditionally for a direct pointer,
   and a finger's press has to be able to end as a *scroll* — which the
   controller cannot see. So the host owns that arm too: a direct pointer places
   no caret on the press, and places one on a release that still belongs to it
   (`ctx.press_is_inside()`, the same predicate the data views' release-time
   commits ask). Its `PointerDown` arm is unreachable for a host that mounts the
   layer, because the handles are widgets above the editor and are offered the
   press first — which the step already said.

   A hold **spends** the press: it fires before the finger lifts, so the release
   that follows must not place a caret over the word the hold just selected.
   Record which contact the hold answered for, and skip that release.

4. **Mount a `TextAffordanceLayer`** in the `OverlayBand::TextAffordance` band
   via `EventContext::show_overlay_in_band`, built with
   `BuildContext::add_detached` — never a bare `add`, which hands back a node
   nothing owns and strands another copy on every rebuild.

   **Not** with `FullViewport`, though that is the placement this band was
   written for. An overlay is chosen by its *bounds*: `OverlayManager::hit_test`
   picks the topmost overlay whose rectangle contains the press, and the router
   then searches that overlay's content **and nothing else** — so a
   viewport-sized affordance overlay whose content misses the point answers
   "nothing here" rather than falling through, and the editor under a raised
   affordance stops taking presses at all. The `event_pass_through` on the layer
   cannot help: it is honoured inside the subtree walk, below the point at which
   the overlay was already chosen.

   So wrap the layer in a content root sized to the affordances themselves — the
   union of the handles' hit rectangles and the lens — and place it with
   `AtPointer(union.origin)`, re-placed on every publish. Presses outside that
   rectangle then reach the tree exactly as they did. Presses *inside* it that
   are on neither handle reach the wrapper, which is the right place to answer
   them the way the editor would have; the wrapper must first ask
   `TouchSelection::handle_at` (and `is_dragging`) whether a handle took the
   press, because a handle's `Handled` does not stop the press reaching an
   ancestor on the same bubble path.

   Nothing needs reclaiming on rebuild: the detached content dies with the build
   that made it, and `gc_orphaned_overlays` dismisses, at the next layout, every
   overlay whose content is no longer active.

5. **Raise the toolbar** from `controller.toolbar()`, using
   `SelectionToolbarRequest::placement()`, and re-place it as the selection moves
   with `EventContext::update_overlay_placement_by_content`.

   Two corrections. First, **not `ClickOutside`**: a direct pointer's outside
   press is *armed* rather than dismissed — the framework withholds both the down
   and the up from the tree, on the grounds that a finger covers what it is about
   to actuate — so with a click-outside toolbar up, the next touch anywhere,
   including on a selection handle, is spent closing the menu and every gesture
   needs doing twice. `EscapeKey` is skipped by that rule entirely, which leaves
   Escape working and the host owning every other way down. Second, the toolbar
   raised this way does **not** take focus: `show_overlay_in_band` deliberately
   records no focus-restore, because nothing in this band takes focus from the
   anchor. That is what keeps the caret in the editor; it also means such a
   toolbar has no keyboard route of its own, which is the correct trade for
   chrome that only a direct pointer raises.

   The controller offers a toolbar for any state with a command worth offering,
   including a bare caret — where it is Paste and Select All. Whether to *raise*
   one is the host's: a menu opening on every tap in a text field is not what any
   platform does, and the toolbar belongs to a deliberate selection (a hold, a
   multi-tap, the end of a handle drag).

6. **Report the IME area**, not the keyboard. Use the *editor's own* reporter
   rather than `TouchSelection::report_ime_area` when it has one: the shipped
   text stacks dedupe the area against the last one they sent, because
   re-forwarding an unchanged rectangle echoes back a fresh empty preedit on some
   winit IME backends and sustains a feedback loop — and the controller's version
   would leave that cache stale as well as skipping the focus and layout guard.
   Either way, report the area only. Re-asserting IME allowance cancels a live
   composition, and a touch caret placement during composition must preserve the
   preedit.

7. **Dismiss it yourself.** The affordance band is exempt from outside-press
   dismissal — every tap that moves a caret is "outside" a handle — so call
   `TouchSelection::dismiss()` when focus leaves, the content changes, the
   surface becomes read-only, or the window deactivates. The last two of those
   are the ones only the host can serve: the effects that watch window
   activation and the bound text have no `EventContext`, so they cannot dismiss
   an overlay at all, which is why retirement is the controller publishing empty
   geometry rather than an overlay teardown. Call `TouchSelection::refresh(..)`
   after any selection change the controller did not make — a keystroke, an undo,
   an assistive client's `SetTextSelection`, the editor's own multi-tap.

   Gate every affordance node on the controller's published state with
   `visible_when`, and give each overlay an `OverlayRequest::on_dismiss` that
   clears whatever the gate reads. The framework takes overlays down on paths the
   host does not drive — a right-click clears the transient overlays before
   mounting its menu, Escape closes the top one, a modal clears the stack — and a
   `visible_when(true)` node that no overlay hosts any more is an ordinary root
   that paints wherever layout puts it.

## The magnifier

A lens 96 dp wide and 44 dp tall, raised 40 dp above the contact, magnifying
1.25×. It appears only while a handle is being dragged, and not at all under
`prefers-reduced-motion` — a lens that appears unbidden and then chases the
finger is precisely the movement that preference is about, and the selection
works without it.

`1.25` is the glyph atlas's raster-scale ladder's first step above 1.0
(`quantize_raster_scale`, geometric with ratio 1.25), so a future implementation
that raised the raster scale for the replay would land on a bucket rather than
add one. The shipped replay does **not** raise it: it re-emits the host's text
layer through the same `PaintContext`, so the glyphs are shaped and rasterised
at their normal size and the lens stretches them on the GPU. The factor keeps
that door open; it is not walked through today.

It is a **replay**, not a framebuffer readback. `RenderFrame` is a display list
that the renderer consumes once per frame and never reads back, but it carries
`SetClip` / `ClearClip` and `SetTransform`, so the same content can simply be
emitted a second time inside a transform-and-clip scope. That is
`PaintContext::replay`, and it is why the host supplies its text layer as a
painting closure rather than as pixels.

Three consequences, all of them contracts:

* **The closure is re-entered during the same frame.** Anything it mutates
  happens twice per frame. Anything it *borrows* must already have been released
  by the host's own `paint` — that half is checkable, because a closure that
  re-borrows a `RefCell` the host still holds panics deterministically (overlay
  content is painted in a separate walk, after the main tree's paint has
  returned). The mutation half is not checkable at all: a counter advanced twice
  or a cache keyed by "the last paint" is invisible to the framework and shows
  up only as wrong content in the lens. A host that cannot meet this calls
  `TouchSelection::magnifier(false)` and mounts no lens.
* **Only the text layer.** A closure that draws the editor's own chrome —
  border, focus ring, scroll bars — puts a magnified copy of the frame over the
  document.
* **The lens is a rectangle.** `DrawCommand::SetClip` takes a `Rect` and the
  renderer realises it as a scissor rectangle; the pipeline has no path clip, no
  stencil and no mask pass, so a rounded clip is not expressible today. A frame
  drawn with corner radius `r` therefore leaves `r × (√2 − 1)` dp of magnified
  content standing outside each of its corners — 4 dp at a 10 dp radius, which a
  1 dp frame does not begin to cover. The shipped style answers this by drawing
  a **rectangular** frame, which is exactly the clip, so nothing escapes; a
  style may raise `magnifier_corner_radius` and accept the corners. A genuinely
  rounded loupe needs a masked clip in the renderer, which is out of this
  contract's reach.

## Handles

24 dp of painted disc inside a 44 dp square target. The disc is a *decoration*
and is the same at every density — it is already sized for a fingertip — while
the target is a `TargetRole::Target` and grows with the density ladder. Target
conformance is measured on the rectangle the pointer meets, which is the rule
`docs/density-and-targets.md` states for any affordance whose hit area is
widened rather than its ink. The `grab_size` ladder — 6 / 10 / 16 dp across
Compact / Comfortable / Touch — is deliberately **not** used: it tops out at
16 dp, so it is below the 24 dp WCAG 2.2 floor at *every* rung, Touch included.

Placement rules, all of them driven by reachability rather than by taste:

* The disc hangs **above** the line for a `Start` handle and **below** it for
  `End` and `Caret`, so the two ends of a one-line selection do not sit on top
  of each other.
* When the preferred side does not fit in the surface's viewport the handle takes
  the other one. A handle under the last line of an editor whose bottom is the
  window's would otherwise be off screen entirely.
* A target that hangs off the edge is nudged back — but only as far as it can go
  without losing the disc it belongs to. What remains on screen still clears the
  24 dp floor, which is what the nudge is for.
* A caret scrolled out of the viewport has **no** handle. Leaving one behind
  would put a live 44 dp target over unrelated content.

Logical order (`Start`, `End`) becomes a physical side in exactly one place,
[`SelectionHandleKind::side`](../crates/teksilo-core/src/overlay/text_affordance.rs),
so a right-to-left mirror can never be applied twice: `Start` is on the left in
English and on the right in Arabic.

Dragging one end past the other grows the range on the far side rather than
collapsing the selection, so the finger keeps hold of the edge it grabbed and
the range can be grown back the way it came. Which *kind* of handle is under
the finger afterwards is read back out of the resulting range, symmetrically in
either crossing direction: the finger holds the `Start` once it is before the
end that stayed put and the `End` once it is after it. Nothing records the
crossing.

## Accessibility

A handle is a `Role::Slider` whose numeric value is the text offset it marks,
with `0..document_len` as its range and a step of one character, and it accepts
`Action::SetValue`. That is the whole contract, and it is deliberately not
paired with `Action::Focus`: handles are **not** focusable. The affordance band
is anchor-independent, so the framework never moves focus into it, and a Tab stop
appearing in the middle of a sentence the instant a finger touched it would be
worse than useless to a keyboard user — who has arrow keys and Shift for the
same job.

The magnifier's node is marked hidden — `aria-hidden`, so it is out of
navigation and out of hit-testing: the lens shows what is already in the tree,
and announcing it would read the same sentence twice. The affordance
layer itself is a bare `GenericContainer`, which the accessibility walker prunes,
so it adds no traversal stop between the editor and its handles.

The selection toolbar is built from the host's own context-menu rows, so it
inherits their accessibility. Its **focus** behaviour follows the door it is
raised through, not the rows: `show_overlay_in_band` records no focus-restore and
moves no focus, so a toolbar raised that way leaves the caret in the editor and
has no keyboard route of its own. A host that wants the keyboard route raises the
same rows through the router's context-menu path instead, and gives up the
caret's focus for as long as the menu is open.

## Single-line surfaces

`TextInput`, `PasswordField`, `SearchField`, `HexColorInput`, `FilePickerField`,
`SpinBox`, `DateEdit`, `TimeEdit`, `DateTimeEdit` and `DateRangeEdit` all build on
one primitive, so one adoption inside
`primitives/text_input_field/touch.rs` reaches every one of them, and everything
composed from them. Two properties of a one-line editor shape that adoption:

* **The vertical coordinate carries no information**, so `offset_at` pins it to
  the line. Without that, every press on the strip a handle occupies — which is
  *below* the text by construction — resolves to the end of the document.
* **A handle's target is larger than the field at every density but one.** The
  target is 44 dp square by the WCAG floor; the field's own height is whichever is
  larger of its recipe constant and the density's `target_size`, so it climbs the
  ladder and only reaches 44 dp at `TargetDensity::Touch`. Below that the two
  handles of a short selection blanket the text between them and a little either
  side. A tap there adjusts that end of the selection rather than placing a caret;
  a tap clear of both ends places one. It is not something the affordances can
  shrink their way out of, because the target *is* the conformance surface.

### The password exception

A secure field gets **no magnifier**, revealed or not, and both switches are set:
the controller stops computing a request and the layer builds no lens node.
Either alone would leave the other able to reintroduce it. The reason is the
revealed case rather than the masked one — masking already keeps plaintext out of
the shaper, the atlas and the accessibility value, so a lens over a masked field
would show nothing but larger bullets, while a lens over a *revealed* one shows
the password magnified and raised 40 dp clear of the fingertip that asked for it.

The toolbar needs no exception, because `clipboard_actions` derives Cut and Copy
from `allows_copy` and the field answers that with the same predicate its own
`Ctrl+C` uses: masked, neither is offered; revealed, both are. Paste and Select
All stay on offer throughout — neither reveals anything.

Handles and the toolbar are suppressed entirely in one further state:
`EchoMode::NoEcho` while masked lays out an **empty** source, so every offset
hit-tests to zero and every caret rectangle is the same rectangle. There is no
geometry to hang an affordance off, and three handles stacked at the start of an
empty line would be visible nonsense.

## Limits

* The lens clip is rectangular, so the shipped lens frame is too. See above.
* **An `event_pass_through` overlay does not fall through.** `hit_test_with`
  returns whatever the chosen overlay's subtree answers, including nothing, so
  the band cannot be used at viewport size and every host has to size its
  affordance overlay to its affordances (checklist step 4). Letting the router
  continue past an overlay whose content root is `event_pass_through` would remove
  that constraint; it is a change to the hit-test, so no host can make it.
* The two coordinate conversions carry **no transform term**, so an editor under
  a scene or zoom transform reports affordance geometry in the untransformed
  space. The shipped single-line stack's two existing window-space paths — the OS
  IME candidate area and the context-menu caret — already share that limitation.
* `selection_bounds()` is one rectangle, not a list of per-line rectangles: the
  only consumer is the toolbar's placement, and the engines behind the shipped
  editors expose a union box. A surface with per-line geometry returns its union.
* Nothing in the framework can verify that a magnifier painter is pure.
