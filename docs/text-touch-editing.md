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

The mouse path is untouched. Every entry point begins by asking
`PointerKind::is_direct()` and returns `Ignored` without reading or writing any
state when the answer is no, so an editor that installs the controller edits
byte for byte as it did before. A **stylus is direct** and gets the whole
affordance set: a pen selects text the way a finger does.

## Where the pieces live

| Piece | Crate | Why there |
| --- | --- | --- |
| `TextHitSource`, `TouchSelection`, `TextAffordances` | `teksilo-core` | The terminal selects text (read-only) and deliberately does not depend on `teksilo-widgets`. An embedded `WebView` is *not* a consumer — its engine owns the page's selection and paints its own handles. |
| `TextAffordanceLayer`, `SelectionHandle`, `TextMagnifier` | `teksilo-core` | Same reason, and they are single-node batched paint — nothing about them needs the widget catalogue. |
| `PaintContext::replay` | `teksilo-core` | The magnifier's mechanism; see below. |
| `TextSelectionStyle`, `TextSelectionHandleRecipe`, `TextMagnifierRecipe` | `teksilo-core` | The Tier-3 protocol, alongside every other style trait. |
| `RecipeTextSelectionStyle` | `teksilo-widgets` | The shipped IntUI look, like every other `Recipe*Style`. |
| The selection toolbar | the host's own `.context_menu(..)` | It *is* a context menu: same commands, same rows, same keyboard route. The controller decides which commands to offer and where to hang them; it does not paint a second menu widget. |

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

Every point and rectangle is in **window** logical coordinates — the space a
`PointerDown` arrives in and the space an overlay is positioned in. A host whose
engine works in document coordinates converts on the way in and on the way out;
anywhere else and the handles disagree with the caret the moment the editor is
scrolled.

`clipboard_actions` has a derived default: Cut needs an editable surface, a
selection and permission to copy; Copy needs the last two; Paste needs the
first; Select All is offered only while nothing is selected. A read-only surface
therefore ends up with a **Copy-only** toolbar and a password field with neither
Cut nor Copy, without either case being special-cased anywhere.

### What it does not answer

`TextHitSource` is geometry. *Performing* a command — cut, paste, undo — is
[`TextSurface`](../crates/teksilo-core/src/text_surface.rs), which every text
widget already registers. The two meet only at `clipboard_actions`, which says
which commands to **offer**, so there is exactly one implementation of each.

## Host checklist

1. **Implement `TextHitSource`** on the same handle that already implements
   `TextSurface`.
2. **Own a `TouchSelection`.** Build it in `build()`, not in a handler:
   `reduced_motion` has no accessor on `EventContext`, so the preference has to
   be captured while a `BuildContext` is in hand. Pass it the metrics from the
   active style, so the rectangle it hit-tests is the one the layer paints:
   ```rust
   let style = ctx.theme().style_slots.text_selection.clone()
       .unwrap_or_else(|| Rc::new(RecipeTextSelectionStyle::for_tokens(&ctx.theme().input)));
   let handle = style.handle(ctx.theme());
   let controller = TouchSelection::new()
       .metrics(HandleMetrics::from(&handle))
       .reduced_motion(ctx.prefers_reduced_motion());
   ```
3. **Forward events.** `handle_pointer` from the editor's own
   `on_pointer_event`, and `on_long_press` from its `on_long_press`. Both are
   inert for an indirect pointer, so neither needs a guard of its own — but note
   that the gesture arena installs a long-press recognizer on the *presence* of
   the handler, with no pointer-kind condition. That is exactly why the guard is
   the controller's first statement rather than the host's.
4. **Mount a `TextAffordanceLayer`** built in `build()` and raised as the
   content of a `FullViewport` overlay in the
   `OverlayBand::TextAffordance` band, via
   `EventContext::show_overlay_in_band`. Feed it `controller.affordances()` and
   a delegate that routes `handle_drag` / `set_handle_offset` back into the
   controller.
5. **Raise the toolbar** from `controller.toolbar()`, using
   `SelectionToolbarRequest::placement()` — an `AboveSelection` placement that
   already flips below when the selection is against the top of the usable area.
   Re-place it as the selection moves with
   `EventContext::update_overlay_placement_by_content`.
6. **Report the IME area**, not the keyboard: `controller.report_ime_area(..)`.
   Re-asserting IME allowance cancels a live composition, and a touch caret
   placement during composition must preserve the preedit.
7. **Dismiss it yourself.** The affordance band is exempt from outside-press
   dismissal — every tap that moves a caret is "outside" a handle — and it is
   anchor-independent, so focus moving away does not retire it either. Call
   `TouchSelection::dismiss()` when focus leaves, the content changes, the
   surface becomes read-only, or the window deactivates. Call
   `TouchSelection::refresh(..)` after any selection change the controller did
   not make.

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

The selection toolbar is the host's context menu, so it inherits that menu's
accessibility, its keyboard route and its focus behaviour — including the fact
that, unlike the band, it *does* take focus.

## Limits

* The lens clip is rectangular, so the shipped lens frame is too. See above.
* `selection_bounds()` is one rectangle, not a list of per-line rectangles: the
  only consumer is the toolbar's placement, and the engines behind the shipped
  editors expose a union box. A surface with per-line geometry returns its union.
* Nothing in the framework can verify that a magnifier painter is pure.
