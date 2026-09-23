<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Scrolling — making content scrollable

Wrap the content in a [`ScrollArea`](../crates/teksilo-widgets/src/scroll_area.rs).
That is the whole answer for ordinary content — a settings page, a form, a long
`VStack`, a `Panel` of cards:

```rust
use teksilo::prelude::*;                                  // lit!, Signal, …
use teksilo::widgets::{Padding, ScrollArea, TextWidget, VStack};

let page = VStack::new()
    .spacing(12.0)
    .child(TextWidget::new(lit!("Section one")))
    .children(rows);

ScrollArea::new().child(Padding::uniform(16.0).child(page))
```

The scroll offset lives in reactive `Signal<f32>`s the area owns, one per axis.
A mouse wheel, a trackpad stream, a finger's pan, a keyboard focus move into
off-screen content, and an assistive technology's scroll action all end up
writing those same signals, so there is one source of truth and nothing to wire
up.

**But do not reach for it reflexively.** The five virtualized data views and the
three text surfaces scroll themselves, and wrapping one is a bug with three
symptoms — see [§2](#2-widgets-that-scroll-themselves) before you wrap anything
that shows a list.

Verified against teksilo 0.13.1. Sources: [`scroll_area.rs`](../crates/teksilo-widgets/src/scroll_area.rs),
[`scroll_bar.rs`](../crates/teksilo-widgets/src/scroll_bar.rs),
[`common/scrollable.rs`](../crates/teksilo-widgets/src/common/scrollable.rs).
Framework internals: [`architecture.md`](architecture.md) §3. Touch physics:
[`kinetic-scrolling.md`](kinetic-scrolling.md).

## 1. The minimal case

`ScrollArea` takes exactly one content child, set with `.child(w)` (any
`impl Widget`) or `.from_id(id)` (a `WidgetId` you already registered with
`ctx.add`). Everything else is a default you can leave alone:

```rust
use teksilo::widgets::ScrollArea;

let id = ctx.add(ScrollArea::new().child(long_content));
```

Out of the box that gives you: overlay scroll bars that appear on overflow and
expand under the pointer, a 150 ms eased wheel, pan-to-scroll with a kinetic
fling for a finger, clipping to the viewport, scroll-chaining to an outer
scroller at the boundary, and a `Role::ScrollView` accessibility node with the
scroll actions the axes actually support.

The one thing the area cannot supply for itself is **height**. A scroll area
takes its height from its parent on purpose — if it sized to its content it
would grow to fit everything and there would be nothing to scroll. In a `VStack`
that means giving it the leftover space:

```rust
use teksilo::widgets::{Expand, ScrollArea, VStack};

VStack::new()
    .child(header)                                       // rigid
    .child(Expand::new().child(ScrollArea::new().child(body)))
    .child(status_bar)                                   // rigid
```

Without the `Expand` the area reports a rigid 200 dp (or whatever
`preferred_height` says) and the page scrolls inside a 200 dp slot with acres of
empty space below it. See [§9](#9-troubleshooting).

## 2. Widgets that scroll themselves

This is the distinction that matters most, and it does not follow from anything
visible in an app's own code: a `ListView` looks like content, so wrapping it in
a `ScrollArea` looks like the obvious move. It is not. The virtualized data views
and the text surfaces each install
[`ScrollableBehavior`](../crates/teksilo-widgets/src/common/scrollable.rs)
directly, build their own [`ScrollBar`](../crates/teksilo-widgets/src/scroll_bar.rs)
children, and set `clips_children` on their own node. They **are** scroll
containers.

| Widget | Scrolls itself | What to do |
| --- | --- | --- |
| `ListView`, `TreeView` | Yes (vertical) | Give it a height. Never wrap. |
| `TableView`, `TreeTableView` | Yes (both axes) | Give it a height. Never wrap. |
| `GridView` | Yes (vertical) | Give it a height. Never wrap. |
| `LogView`, `CodeEditor`, `RichTextEditor` | Yes (both axes) | Give it a height. Wrap only under the rule below. |
| `SceneView` (teksilo-scene) | Own pan/zoom camera | Never wrap. |
| `Repeater` | No | Wrap it. |
| `VStack` / `HStack` / `Grid` / `Wrap` / `ColumnFlow` / `MasonryLayout` / `FormLayout` | No | Wrap it. |
| `Panel`, `Card`, `GroupBox`, `ToolBox`, `Accordion` | No | Wrap the content, or wrap the container. |
| `MenuList`, `MessageBox`, `NotificationLog`, `TabBar`, `CompositeTooltip`, `RadioTileGroup` | Already embed a `ScrollArea` | Nothing. |

`Repeater` is the one that catches people out in the other direction: it is
data-driven like `ListView`, but it is explicitly **not** virtualized (every
item has a live widget at all times — that is what lets its children stay
stateful editors) and it does no scrolling. A `Repeater` over a long model
belongs inside a `ScrollArea`.

### What actually goes wrong when you wrap a data view

Three things, and only the third is obvious:

1. **The list collapses to its fallback size.** A `ScrollArea` proposes
   `SizeProposal { width: Some(viewport_width), height: None }` to its content.
   For a virtualizing widget an unbounded height is a *measurement* question,
   not an allocation, so it answers its fallback — 300 × 200 for
   `ListView`/`TreeView`, 400 × 300 for the tables, 400 × 400 for `GridView`
   ([`common/viewport.rs`](../crates/teksilo-widgets/src/common/viewport.rs)).
   The list is then 200 dp tall no matter how much room the page has, and the
   outer `ScrollArea` has 200 dp of content and nothing to scroll.
2. **Virtualization is measuring the wrong viewport.** These widgets decide *in
   `build()`* how many rows to realize, from a cached viewport height that only
   an allocation is allowed to write. The module docs on `common/viewport.rs`
   record what happens when a measurement's fallback reaches that cache: `build`
   realizes rows for the fallback, `place_children` sees the real rect, bumps the
   rebuild version, and `build` reads the fallback again — a rebuild every frame,
   rendering an empty hole while burning a core.
3. **Two scroll bars.** The inner view's own bar plus the outer area's, one
   inside the other, each scrolling a different thing.

### Sizing a data view instead

Give it real space rather than an unbounded proposal — the same `Expand` you
would give a `ScrollArea`:

```rust
use teksilo::widgets::{Expand, ListView, Panel, VStack};

VStack::new()
    .child(toolbar)
    .child(Expand::new().child(Panel::new().child(list_id)))   // list fills the rest
```

`FixedSize`, a `Splitter` pane, a `DockingLayout` dock, a `TabWidget` page and
the window root all allocate a definite height too. What does not is a bare
`VStack`/`HStack` child slot (rigid, `flex = 0`) or a `ScrollArea`.

### The one legitimate nesting

A text surface deliberately sized to its content inside a scrolling page — the
long-form document editor, the messenger composer. `RichTextEditor` and
`CodeEditor` take a per-axis
[`ScrollPolicy`](../crates/teksilo-widgets/src/rich_text.rs) whose `AlwaysOff`
variant exists for exactly this, and `RichTextEditor::editor(..).min_lines(n)` /
`.max_lines(n)` switch it from greedy to intrinsic sizing so it reports a real
height to the page:

```rust
RichTextEditor::editor(doc)
    .min_lines(3)
    .max_lines(12)
    .v_scroll_policy(ScrollPolicy::AlwaysOff)
```

The reveal walk handles the rest: `RichTextEditor` overrides
`Widget::focus_reveal_rect` to nominate the **caret line** rather than its whole
(page-tall) box, so focusing it scrolls the page to the caret instead of to the
editor's bottom. See [§5](#5-programmatic-scrolling-and-the-reveal-walk).

## 3. Axes, scroll bars, and visibility

Both axes are always live. A `ScrollArea` claims a pan on `PanAxes::BOTH` and
publishes a `max_scroll` per axis from layout; an axis whose content fits has
`max_scroll == 0`, declines everything, and chains outward. There is no
"direction" knob — you constrain an axis by constraining the content (a child
that sizes to the proposed width never overflows horizontally), and you hide a
bar with its policy.

**Display mode** — `ScrollBarMode`, set with `.scroll_bar_style(..)`:

| Mode | Behaviour |
| --- | --- |
| `Overlay` (default) | Floats over the content: thin indicator at rest, full interactive track on pointer proximity. Does not reduce the viewport. |
| `Permanent` | A layout sibling of the viewport, reserving its full thickness at all times — the classic Windows/GTK gutter. The viewport is narrower and stays a constant width. |
| `Thin` | Floats like `Overlay` but never expands past the thin indicator. Drag and track-click still work against the full slot. |

**Per-axis visibility** — `ScrollBarPolicy`, set with
`.vertical_scroll_bar_policy(..)` / `.horizontal_scroll_bar_policy(..)`:
`AsNeeded` (default — shown when the axis overflows), `AlwaysOn`, `AlwaysOff`.
`AlwaysOff` hides the bar; it does not stop the content scrolling on a wheel, on
a pan, or from the AT scroll actions.

```rust
ScrollArea::new()
    .child(content)
    .scroll_bar_style(ScrollBarMode::Permanent)
    .horizontal_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
    .scroll_bar_thickness(10.0)
```

**No keyboard.** Stated plainly because it is a real limitation and easy to
assume away: `ScrollArea` installs no key handler, and `ScrollBar`'s
arrow / `Home` / `End` / `Page` arms sit on a node built `focusable(false)` and
hidden from AT, so no keyboard user reaches them. A keyboard user scrolls a
`ScrollArea` by moving focus — the reveal walk scrolls the container to whatever
they Tab to. Content with no focusable descendants is, today, keyboard-
unreachable inside a `ScrollArea`; the data views bind their own `PageUp` /
`PageDown` / `Home` / `End` and are unaffected (see
[`data-view-keyboard.md`](data-view-keyboard.md)).
[`touch-and-pen.md`](touch-and-pen.md) §10.2 carries the gap as an open finding.

**Other knobs worth knowing:**

- `.smooth_scrolling(bool)` (default on) and `.smooth_scroll_duration(Duration)`
  (default 150 ms) — applies to both `ScrollDelta::Lines` and
  `ScrollDelta::Pixels`, because a high-resolution wheel on Wayland delivers
  notches as pixels and animating only one path makes a fast flick jump.
- `.line_height(f32)` (default 20 dp) — pixels per line for line-based wheels.
- `.widget_resizable(bool)` — stretch content smaller than the viewport to fill
  it (Qt's `QScrollArea::setWidgetResizable`).
- `.overscroll_behavior(OverscrollBehavior::{Chain, Contain})` — the CSS
  `overscroll-behavior` model. `Chain` (default) declines at the boundary so the
  event reaches an ancestor scroller; `Contain` absorbs it. Note that a scroller
  whose content fits entirely is *always* at its boundary, so under `Chain` it
  passes the wheel through — set `Contain` on a fit-to-content panel that should
  swallow it regardless.
- `.scroll_past_end(fraction)` — extends the scroll **range** by `fraction` of a
  viewport without adding any widget, padding, or layout. The typewriter-scroll
  case: to pin a caret at mid-viewport the view must be able to travel half a
  viewport past the last line, or the pin quietly stops working over the final
  page. Takes an `impl Into<Prop<f32>>` — a plain value, or a `Signal<f32>` so it
  can follow a setting live.

## 4. Layout: the unbounded proposal

`ScrollArea` is an ordinary container. It claims the space its parent offers in
`layout_response`, and in `place_children` it proposes
`SizeProposal { width: Some(viewport_width), height: None }` to its content and
positions it at `(viewport.x − scroll_x, viewport.y − scroll_y)`. The scroll
offset *is* a placement offset: there is no coordinate-transform layer, hit
testing needs no special case, and every bound the arena stores is already in
screen space ([`architecture.md`](architecture.md) §3.1–3.2).

Three consequences you will actually meet.

**The height comes from the parent, the width follows the content.** The
asymmetry is deliberate. An area that took its height from its content would
never need to scroll; an area that took its width from its viewport would
collapse inside a width-hugging parent (a menu, a popover) and clip every row.
So when the parent proposes no width the area measures the content's natural
width and reports that. `preferred_size(w, h)` overrides *both* axes and a `0.0`
width there means zero, not "no preference" — use `preferred_height(h)` when you
want to cap the height and let the width keep hugging, which is what a scrolling
menu needs.

**An unbounded proposal changes how the child measures.** `SizeProposal` is a
proposal, not a constraint: the child answers with the size it wants and may
exceed the proposed width (which is how horizontal overflow is detected at all).
But a child that *branches* on `proposal.height.is_none()` sees a different
question inside a `ScrollArea` than it does anywhere else — which is precisely
why the virtualized views must not be wrapped ([§2](#2-widgets-that-scroll-themselves)),
and why height-for-width content (wrapped text, aspect-ratio images) works
correctly here: it is measured at its final width, and the height it reports
becomes the scrollable range.

**Layout runs twice when a `Permanent` bar is in play.** Reserving the vertical
gutter narrows the viewport, which can make the content taller, which can bring
the horizontal axis into overflow. `place_children` measures optimistically,
resolves both bars, and re-measures only if the vertical reservation changed.

Clipping is one flag: `Widget::clips_children` (or `.clips_children(true)` on a
`HandlerSet`, `.clips_children_on(true)` through the `WidgetBuilder` trait). The paint walker pushes the node's bounds as a
clip rect before recursing and pops it after, and the renderer maps that to a
wgpu scissor rect; nested clippers intersect via a stack.

## 5. Programmatic scrolling and the reveal walk

Nothing scrolls a container by reaching into it. Everything goes through
`WidgetEvent::ScrollIntoView`, dispatched by the framework's reveal walk
(`WidgetTree::scroll_rect_into_view`,
[`focus_impl.rs`](../crates/teksilo-core/src/widget_tree/focus_impl.rs)).

**Focus.** When focus lands on a widget, the framework walks strictly outward
from it and, for every ancestor whose `clips_children` viewport does not already
contain the target, dispatches `ScrollIntoView` so that container adjusts its
offset. Two details are load-bearing:

- The walk fires only for **keyboard, programmatic and assistive-technology**
  focus, never for a pointer press. A widget the user just clicked is by definition already visible,
  and auto-scrolling on click yanks a tall editor to its far end on the stale
  pre-click caret.
- The focused widget itself is **excluded**. A scrollable widget is responsible
  for revealing an interior rect inside its own viewport; the walk handles the
  containers around it. That is what prevents a double-scroll against a widget's
  own caret-follow.

**From a handler.** `EventContext` carries the caller-driven forms. All take
absolute tree (window) coordinates and are queued, then drained after the
handler returns:

```rust
use teksilo::core::event::ScrollMotion;

ctx.ensure_visible(rect);                    // minimal reveal
ctx.ensure_visible_with_margin(rect, 12.0);  // …with breathing room
ctx.ensure_widget_visible(id);               // a mounted child, by id
ctx.ensure_visible_aligned(caret, 0.5, ScrollMotion::Instant);  // pin at mid-viewport
```

`ensure_visible` is `ScrollAlign::Minimal`: it does nothing when the rect is
already fully visible. `ensure_visible_aligned` is `ScrollAlign::Fraction(f)` —
`0.0` flush top, `0.5` centred, `1.0` flush bottom — and scrolls **whether or not
the target is already visible**, which is what makes typewriter scrolling
possible; a caret that only moved the view once it fell off the edge would not be
pinned to anything. Alignment applies to the **innermost** clipping ancestor
only; everything further out falls back to a minimal reveal, because an outer
container's job is to bring the inner viewport on screen, not to align a
rectangle it does not own.

`ScrollMotion::{Instant, Smooth}` is separate from the container's own
`smooth_scrolling` because the right answer depends on the request: a caret
pinned on every keystroke must snap (animating it is the "bouncing screen" users
complain about in other editors), while a page-down or a search hit gliding reads
as polish. A container with `smooth_scrolling(false)` jumps either way.

The `_from` variants (`ensure_visible_from(owner, rect)`,
`ensure_visible_aligned_from`) walk **another widget's** ancestors. Use them when
the handler is not inside the thing being revealed — a find banner's Next button
sits beside the scrolling page, so a reveal walked from the button climbs out
through the banner and never meets the scroll container the match is in. It fails
silently: the match is selected, the counter moves, and the viewport does not
follow.

**Nested containers.** Each handling container reports how far it scrolled
through the `applied_scroll` back-channel on the event; the walk shifts the
target rect by the negated delta before asking the next one outward, so the outer
container targets where the child will land once the inner's deferred scroll
applies, not its pre-scroll position. A handler that leaves the cell zero simply
gets no re-targeting, which is exact for the ordinary single-container case.

**What a widget must do to participate.** Two seams:

- **To be scrolled into view at a useful granularity**, override
  `Widget::focus_reveal_rect(&self, bounds) -> Option<Rect>` and return the
  sub-rectangle that matters — the caret line, the selected row. The default
  `None` reveals the whole box, which for a page-tall widget scrolls its
  container to the bottom on a click that only meant to place a caret.
  `RichTextEditor` is the shipped example.
- **To be a scroll container**, set `clips_children` and install an `on_scroll`
  handler that matches `WidgetEvent::ScrollIntoView` — the router delivers
  `Scroll` and `ScrollIntoView` to the same handler slot. `ScrollArea` does this;
  so does `teksilo-scene`'s `SceneView`, which clips and answers the reveal by
  panning its camera, so a caret moving inside a `RichTextEditor` embedded on a
  scene card moves the camera with no app wiring. Clipping without handling is
  legal and common (`MaxSize`, `Accordion`, `Splitter`) — those nodes are visited
  and simply do not move.

**Direct APIs.** The data views expose their own imperatives, which is the right
door when the target is a virtualized row that has no arena node to reveal:
`ListView::scroll_to_index(i)`, `TableView::scroll_to_row(r)` (and
`TreeTableView`), `GridView::scroll_to_index(i, ScrollAnchor::Center)` /
`ensure_index_visible`, `LogViewHandle::scroll_to_bottom()` (the handle from
`LogView::handle()`). For a `ScrollArea` itself,
write the offset signal: `area.scroll_y_signal().set(0.0)`, or
`animate_to(..)` for a glide. Read `max_scroll_y_signal()` for "is there more?"
chrome and `viewport_ratio_y_signal()` for a custom position indicator.

`ScrollArea::restore_scroll_y(offset)` is the session-restore path.
`max_scroll_y` is zero until the content has been measured, so an offset written
before that is clamped to zero and the page paints at the top for a frame before
jumping. `restore_scroll_y` stores it and applies it inside layout as soon as the
range is long enough to hold it — it is a one-shot, it stands down the moment
anything else moves the scroll, and it never yanks a reader back after a later
reflow.

## 6. Touch, pan, and kinetic behaviour

A finger scrolls a `ScrollArea` because the area **declares** itself a pan
surface, not because it handles `on_scroll`. `ScrollableBehavior::install`
attaches both halves — the handler and the `PanClaim` — and a surface that
installs one without the other scrolls on a wheel and ignores a finger entirely.
The declaration is never inferred from an `on_scroll` handler, deliberately:
`SpinBox` increments on a wheel, `TabBar` remaps a notch sideways, `SceneView`
zooms. On your own node:

```rust
use teksilo::core::pointer::touch_action::{PanAxes, TouchAction};

my_surface
    .scroll_container(PanAxes::Y)     // sugar: DIRECT pointers, kinetic on
    .touch_action(TouchAction::PAN_Y) // what a direct pointer may do here
```

`.pan_claim(PanClaim { .. })` is the escape hatch for a non-kinetic claim or a
different device mask. `.touch_action(..)` is the CSS `touch-action` model,
intersected root→target and frozen at press; a mouse never consults it.

A mouse is unchanged by any of this: `pan_slop` is `None` for the mouse profile,
so no pan member is ever eligible and the wheel remains its scroll device.

Two behaviours are worth knowing at the app level:

- **`.rubber_band(bool)` is off by default, and the default is load-bearing.** A
  surface that follows the finger past its own end has absorbed the movement, so
  a nested area that banded could never hand the gesture to the container around
  it. The band belongs to the outermost surface of a scroll chain. When on,
  `overscroll_signal() -> Signal<Vec2>` reports how far past the range the
  content is being held (the scroll offset itself never leaves the range), which
  is what a surface binds to draw a stretch or a glow. `prefers-reduced-motion`
  hard-clamps it whatever the setting says.
- **An overlay bar is revealed during a pan.** A contact produces no hover, so an
  overlay bar would otherwise stay hidden under the very gesture moving it; the
  area raises a reveal signal for as long as the pan is in flight.

Everything about the physics — the two curves, the constants, the fling
hand-off, the macOS momentum rule, reduced motion, and the four rules for
adopting `ScrollableBehavior` in a custom surface — lives in
[`kinetic-scrolling.md`](kinetic-scrolling.md). Data views under a finger
(when a press commits, reorder vs. scroll, the column-header strip) are in
[`data-view-touch.md`](data-view-touch.md).

Shift+wheel is **not** remapped to horizontal by `ScrollArea`. The two table
views opt into `shift_wheel_remap` explicitly, because a horizontally scrollable
grid is the case where it is unambiguously right.

## 7. Accessibility

Usually nothing to do. The scroll system produces two AccessKit nodes and only
one of them is visible to AT.

**`ScrollArea` → `Role::ScrollView`**, carrying `scroll_x` / `scroll_y` with
their min and max, `clips_children`, and `Action::ScrollUp` / `ScrollDown` /
`ScrollLeft` / `ScrollRight` — advertised **only for the directions that can
actually move**, so a screen reader knows which way there is content. Each action
scrolls 90 % of a viewport. Rows that are not realized do not exist in the arena
and so do not appear in the AT tree; a virtualized view carries
`pos_in_set` / `size_of_set` on its visible rows so "item 5 of 200" still reads
correctly.

**`ScrollBar` → hidden** (`set_hidden()`). It is pointer chrome. Exposing it adds
a spurious Tab stop and a second way to say the same thing, so AT scrolls through
the parent `ScrollView`'s actions and navigates the content region directly.

> Note: an earlier design gave `ScrollBar` its own `Role::ScrollBar` with
> `set_numeric_value` and `Action::SetValue`. It was not built;
> [`architecture.md`](architecture.md) §3.6 and §3.10 record why, and the roster
> above is what 0.13.1 emits.

What an app author does have to do: make sure the content is reachable. Because
`ScrollArea` has no keyboard handler ([§3](#3-axes-scroll-bars-and-visibility)),
a scrolling region whose content has no focusable descendants can be scrolled by
AT and by pointer but not by the keyboard. For a long read-only region, prefer a
widget that owns its own keyboard scrolling (`LogView`, a read-only
`RichTextEditor`) over a `ScrollArea` full of `TextWidget`s.

For a finger, the bar stays 8–12 dp wide at every density — growing it would move
the content beside it, and a scroll bar is chrome. The thumb is reached instead
through `Widget::hit_outset`, which widens the node to the 48 dp Android reserves
for a scrollbar touch target without moving or repainting anything, and the
minimum thumb length follows the density (24 dp Compact, 44 dp Touch). A precise
pointer gets no outset: a cursor's hot-spot is exact, and widening its targets
steals clicks from the content. See [`density-and-targets.md`](density-and-targets.md).

## 8. Styling

Scroll-bar chrome is a Tier-3 style protocol,
[`ScrollBarStyle`](../crates/teksilo-core/src/styles/scroll_bar_style.rs):

```rust
pub trait ScrollBarStyle: 'static {
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}
```

`ScrollBarStyleConfig` hands the style everything reactive it needs —
`scroll_ratio`, `viewport_ratio`, `is_hovered`, `is_dragging`, `is_idle`, plus
`orientation`, `variant`, `min_thumb_length` and an optional `thumb_color`
override. The default `RecipeScrollBarStyle`
([recipe](../crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs))
paints all three variants from IntUI tokens.

Install it theme-wide:

```rust
theme.style_slots.scroll_bar = Some(Rc::new(MyScrollBarStyle));
```

`ScrollArea` has no per-call `.style(..)` for its bars; per-instance overrides go
on a `ScrollBar` you construct yourself (the custom-scroll-host path). What
`ScrollArea` does forward is a thumb tint —
`.scroll_bar_thumb_color(impl Into<ColorProp>)`, accepting a `Color`, a theme
role, or a `Signal`, resolved against the live theme at paint. Use it where the
area sits on a surface the surface-relative tokens do not suit: a tooltip's
inverse chip, a branded panel.

**One obligation if you write a style.** A horizontal bar's `scroll_ratio == 0.0`
is the **start** of the content, which in a right-to-left window is the
right-hand edge — `ScrollArea` places its content that way and the bar mirrors
its hit-test and drag to match. A style that offsets the thumb from `bounds.x`
unconditionally paints it at the far end of the track while the content shows its
beginning, and grabbing it jumps. Read `PaintContext::layout_direction` at
**paint** time (a locale change repaints without rebuilding) and mirror the
offset. The widget cannot enforce this, and it is only visible in an RTL locale.

## 9. Troubleshooting

**Nothing scrolls; the content is just clipped, or the area is a short box in a
tall space.** The area's parent gave it an unbounded height, so it fell back to
`preferred_height` (or 200 dp) and its content was never taller than that. Wrap
it in `Expand`, put it in a `FixedSize`, or make it the content of something that
allocates a definite height (a `Splitter` pane, a `TabWidget` page, the window
root). The reverse spelling of the same bug: an area whose content sizes to the
proposed width can never overflow horizontally, so the horizontal bar never
appears.

**Two scroll bars, or scrolling that fights itself.** You wrapped a widget that
already scrolls — see [§2](#2-widgets-that-scroll-themselves). Remove the
`ScrollArea` and give the inner view a height instead.

**A virtualized list renders nothing, or renders an empty hole while the CPU
spins.** Same cause: its cached viewport height is the measurement fallback
rather than a real allocation, so `build()` realizes rows for a viewport that
does not exist and `place_children` immediately asks for a rebuild. Almost always
an enclosing `ScrollArea`; occasionally a hand-written parent that proposes
`height: None` in `place_children`.

**A wheel over one panel scrolls the page behind it.** `OverscrollBehavior::Chain`
is the default, and a scroller whose content fits is always at its boundary — so
it declines and the event chains outward. Set
`.overscroll_behavior(OverscrollBehavior::Contain)` on the panel that should
absorb it.

**Scroll position jumps to the top after an unrelated change.** The `ScrollArea`
was destroyed and rebuilt. A `ScrollArea` owns its offset signals in
`ScrollArea::new()`, so a parent whose `build()` re-runs and calls
`ctx.add(ScrollArea::new()...)` again gets a fresh widget at offset zero. A theme
or locale change does **not** do this (both are relayout/repaint, not rebuild) —
look for a `BindingLevel::Rebuild` binding on the parent. Fix it the way the
framework's own memoizing containers do: have the parent return `true` from
`Widget::preserves_children_on_rebuild()` and re-attach the cached `WidgetId`, so
the child subtree — focus, scroll offset, text contents, subscriptions — survives
([`architecture.md`](architecture.md) §8).

**A restored scroll position paints at the top for one frame, then jumps.** Use
`restore_scroll_y(offset)` instead of writing `scroll_y_signal()` directly: the
range is zero until the content has been measured, so a direct write is clamped
away.

**Focusing something scrolls the page to the wrong place.** The focused widget is
taller than the viewport and reveals its whole box. Override
`Widget::focus_reveal_rect` to nominate the interior rect that matters
([§5](#5-programmatic-scrolling-and-the-reveal-walk)).

**A typewriter pin stops working on the last page.** The view has run out of
range. Buy some with `.scroll_past_end(1.0 - fraction)` — it extends the range
without adding padding or layout.

**`PageUp` / `PageDown` do nothing.** `ScrollArea` has no keyboard handler; this
is expected, not a regression. See [§3](#3-axes-scroll-bars-and-visibility).
