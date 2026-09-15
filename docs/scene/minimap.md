<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneMinimap

`SceneMinimap` — a small thumbnail of a `Scene`
showing all items as dots / rects scaled down, with an overlay
highlighting the currently visible viewport rectangle.

## Use

```
use teksilo_scene::{Scene, SceneView, SceneMinimap};
use teksilo_canvas::Rect;
# use teksilo_widgets::VStack;

let mut scene = Scene::new();
/* …populate scene… */
// Build the SceneView FIRST so we can read its reactive
// viewport signal and its scene's snapshot of items.
let view = SceneView::new(scene);
let content = view
    .scene_content_bounds()
    .unwrap_or(Rect::new(0.0, 0.0, 1000.0, 1000.0));
let viewport_signal = view.viewport_in_scene_signal();
let item_thumbs = view.scene().item_thumbnails(); // Vec<(Rect, Color)>

let _w = VStack::new()
    .child(view)
    .child(
        SceneMinimap::new(content, viewport_signal)
            .items(item_thumbs)
            .size(200.0, 150.0),
    );
```

For a live "items as they move" minimap, re-call
`Scene::item_thumbnails` on
scene mutations and rebuild the widget tree (or wire a
`Signal<Vec<(Rect, Color)>>` if your app needs per-frame
reactivity).

## Design

Deliberately decoupled from `SceneView`: it doesn't reach into
the scene model. Instead it consumes a content extent (the rect
the minimap is guaranteed to show), a static `Vec<(Rect, Color)>`
of item thumbnails (refreshed by the app whenever items move),
and a `Signal<Rect>` for the live viewport rectangle.

Apps that want a live "items as they move" minimap rebuild their
widget tree on scene mutations or wire a `Signal<Vec<...>>`. The
viewport overlay is reactive on its own — the minimap re-paints
whenever the SceneView's pan / zoom changes, with no manual
plumbing.

## The projection, and why it expands

`content_bounds` is a *floor*, not a frame. The rect actually
projected onto the drawing area is the **effective extent**:

```text
effective_extent = content_bounds ∪ every item rect ∪ viewport_in_scene
```

This matters because the inputs are unrelated. `content_bounds`
is typically the union of item rects
(`SceneView::scene_content_bounds`), while `viewport_in_scene` is
the viewport projected through the inverse view transform — and
pan is **unbounded by default** (`Scene::current_pan_bounds()` is
`None`). A user who zooms in and pans away, or who zooms out past
the content, puts the viewport wholly outside `content_bounds`. A
minimap that mapped `content_bounds` alone would then draw the
indicator outside its own frame.

Three answers were possible; this is the one that keeps the
minimap *useful*:

- **Clamp** the indicator to the frame — it would lie about both
  position and size, and stop moving while the user is still
  panning.
- **Crop** it — honest, but the indicator vanishes exactly when
  the user most needs to know where they are.
- **Expand the extent** — position and size stay truthful and the
  content visibly shrinks as you wander off, which *is* the
  "you are out here, the content is over there" signal. This is
  what tldraw does (`Box.Expand(contentBounds, viewportBounds)`).

The projection is a **uniform** fit: one scale for both axes,
centred in the drawing area, so a square item reads as a square
and the picture shrinks evenly as the extent grows. Expect
letterbox margins when the widget's aspect ratio differs from the
extent's.

## The clip

`paint` clips **everything it emits** — background, outline,
thumbnails, indicator and the widget's own border — to the widget's
bounds. Not one stroke is drawn outside it, so "nothing the minimap
paints leaves its frame" holds without qualification and for every
caller-supplied width.

That is a structural backstop, not a duplicate of the extent
expansion: the expansion is a *policy* a future edit could regress,
while the clip is a guarantee.

It is also the only mechanism available here.
`Widget::clips_children` does **not** clip a widget's own
`paint()`: the render walker emits a plain clipping node's
`SetClip` *after* that node's paint (see
`teksilo-core/src/widget_tree/rendering_impl.rs`), so for a
self-painting leaf with no children it is a complete no-op. The
supported primitive is `Canvas::set_clip` / `clear_clip` inside
`paint`, as `ListView`, `TableView`, `CodeEditor` and `Terminal`
already do.

The clip is set **after** the canvas has been translated to
`bounds.origin`, and is given the widget-*local* area — so it lands
on the widget wherever the parent placed it. At the root of a tree
that translation is the identity and the ordering is unobservable,
which is why the tests exercise a nested, offset minimap too.

## The border is inside the frame

`Canvas::stroke_rect` centres each edge line on the rect boundary,
so stroking the widget area directly would hang half of the width
outside the widget — 12 px on every side for a 24 px border, over
whatever sibling is underneath. The border is therefore stroked on
the area **inset by half its width**, which puts the band wholly
inside: the CSS `border-box` convention, and the one every desktop
toolkit uses. A wide border eats into the picture rather than into
the neighbours; the clip then bounds even a pathological width.

Clamping the width instead was rejected: it would silently render
something other than what the caller asked for, and it would still
leave the last half-pixel straddling the edge.

## Accessibility

The minimap's whole job is to say *where you are* and let you go
somewhere else. Both halves are on the AT node, and the second half
has three input routes that all end in the same callback.

**The node.** `Role::Group`, named `"Scene minimap"` by default,
carrying the position as its **value**. A 2-D position has no ARIA
role — there is no `slider2d` — and `numeric_value` holds one number
where this control has four, so the reading is text, exactly as
`HsvCanvas` (`teksilo-widgets/src/color_picker/hsv_canvas.rs`, the
workspace's other 2-D picker) does it. It is deliberately not a property-less
`Role::GenericContainer`: the walker prunes those, and a pruned node
announces nothing.

**The value** is recomputed at every AT walk from the same
`effective_extent` the picture is drawn from, so it is the picture in
words: *"Viewport at 42% across, 17% down; showing 25% of the width and
33% of the height"*. The viewport signal is bound **twice** — at
`RepaintOnly` for the drawing and at `AccessibilityOnly` for this — so a
pan updates the announcement as well as the overlay, with no rebuild.
Pass `SceneMinimap::access_readout` to phrase (and translate) it
yourself; the numbers arrive as a `MinimapReadout`.

**The routes.** With an `on_click` callback
installed the node is focusable and offers, all landing in that one
callback:

| Route | Pointer | Keyboard | Assistive technology |
| --- | --- | --- | --- |
| move the view | tap a point | arrows (`Shift` = a whole viewport) | `ScrollLeft` / `Right` / `Up` / `Down` |
| centre on the content | — | `Home`, `Enter`, `Space` | `Click` |

`Action::Click` is the position-free half of the tap: a click needs a
point and an AT client has none, so the primary action is the one
well-defined destination the widget can name on its own — the centre of
the content. `Home` / `Enter` / `Space` are bound to it so the keyboard
and the AT client do the same thing.

Arrow steps are a fraction of the **viewport**, not of the scene: a
tenth of it for a nudge and a whole one for `Shift` — the line-vs-page
pair a scroll view uses, scaled to what the user is actually looking at
rather than to a count of scene units that would be a screenful at one
zoom and a hair at another. They are **not** mirrored under RTL, for
the reason `HsvCanvas` gives: the picture is not mirrored either, so an
arrow that flipped would disagree with what the user sees.

**Who speaks.** The keyboard and assistive-technology routes announce the
move through `EventContext::announce`. The pointer route does not.

An arrow press on an unannotated graphic tells a screen-reader user
nothing at all, and an AT client that invokes `Click` was never told where
"the content" is — for those two routes the utterance is the entire
feedback. A tap already has some: the user picked the destination by
aiming at the picture. And a minimap invites the gesture *repeatedly* —
click, look, click again — so speaking every one of them turns a polite
live region into a metronome talking over whatever the user was reading.

That is the rule the rest of the workspace already follows, from both
directions. `HsvCanvas` — the other 2-D manipulator, and the widget this
one is modelled on — announces from its arrows and its custom actions and
not from its drag or its tap. The five data views' row reorder announces
from `teksilo_widgets::common::ordered_move`, which is the **non-drag**
alternative; the drop itself is silent. The one pointer route in the
workspace that does speak is the charts' readout
(`teksilo_charts::hit::drive_readout`), and only for a **coarse** pointer
that pressed and released without travelling — because that tap is an
*inspection* standing in for a hover a finger cannot perform, so the
utterance is its whole product, and a scrub is deliberately coalesced into
nothing. A minimap tap is a *command* whose destination the user chose, so
that exception does not reach it.

Silence here is not lost information. The node's **value** is the same
sentence, and the `AccessibilityOnly` binding refreshes it the moment the
app applies the pan — so a client that re-reads the control after a click
gets the new position, it simply is not interrupted with it.

What *is* announced describes the viewport the minimap **asked for**, not
one read back afterwards: the app owns the move (a `with_widget_mut` pan
lands after the handler returns, an animated one later still), so
re-reading the signal here would announce the position we just left. That
is why `on_click`'s contract is specifically
*"centre the view on this scene point"* rather than "here is a point, do
as you like".

**Without `on_click`** the minimap is a read-out, not a control: it still
emits the named, valued node — the useful half — but takes no focus and
advertises no action.

That gate is worth arguing rather than assuming, because a minimap
*displays* something and a display is worth reaching. The answer is that
reaching it and focusing it are separate questions. The node is in the
accessibility tree carrying a name and a value, so a screen reader's
object / browse navigation and its rotor all arrive at it and read the
position out; what the gate withholds is the **Tab** order, and Tab is for
things you can operate. A read-only minimap answers Tab with nothing: no
arrow does anything, `Enter` does nothing, no action is advertised, and
the focus ring has parked on a picture. That is the dead stop the ARIA
practices warn about, and it costs every keyboard user a press on the way
past.

An app that wants it anyway is not blocked — `.focusable(true)` from the
framework's ordinary
`WidgetBuilder` chain puts
any widget into the Tab order, this one included. The default is the
answer that is right without knowing the app.

**Overrides** come from the framework chain, not from a second set of
builders here: `.access_label(tr!(…))` renames the node (re-resolved on
every AT walk, so it follows a locale change),
`.access_description(…)` adds long-form context, and the rest of
`docs/accessibility-overrides.md` applies unchanged.

## Builder methods at a glance

`size`, `items`, `background`, `border`, `viewport_color`, `content_outline`, `on_click`, `access_readout`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub struct SceneMinimap`

A small thumbnail rendering of a `Scene`'s
content, with the live viewport rectangle highlighted.

Paint order: **clip on** → background fill → optional
content-bounds outline → item thumbnails (dots / rects) →
viewport overlay rect → inset border → **clip off**. The clip
covers every one of those, the border included.

The projected extent is `content_bounds` expanded to contain every
item thumbnail and the live viewport — see this module's
documentation ("The projection, and why it expands") for why, and
`docs/teksilo-scene.md` § Minimap for the same story in prose.

```rust
pub struct SceneMinimap { /* fields */ }
```

### Methods

#### `pub fn new(content_bounds: Rect, viewport: Signal<Rect>) -> Self`

Construct a minimap that shows at least `content_bounds` (the
scene-coord extent guaranteed to be visible), with `viewport`
driving the live overlay rectangle.

#### `pub fn size(mut self, width: f32, height: f32) -> Self`

Override the minimap size. Default `200×150`.

#### `pub fn items(mut self, items: Vec<(Rect, Color)>) -> Self`

Static list of item thumbnails: `(scene_rect, color)`. The
minimap projects each rect onto its drawing area and fills it
with `color`. Apps refresh by rebuilding the widget tree
when items move.

Item rects widen the projected extent, so a thumbnail outside
a stale or hand-picked `content_bounds` shrinks the picture
instead of painting outside the frame.

#### `pub fn background(mut self, color: Color) -> Self`

Background fill color. Default semi-transparent white.

#### `pub fn border(mut self, border: Option<(Color, f32)>) -> Self`

Border around the minimap drawing area. Pass `None` for no
border. Default 1px @ 50% black.

The band is drawn **inside** the widget — `width` is consumed
from the drawing area, never from the neighbours (the CSS
`border-box` convention). A wide border therefore covers the
outer rim of the picture instead of the sibling next door, and
the width is honoured as given rather than clamped. A width that
is not finite and positive draws nothing.

#### `pub fn viewport_color(mut self, color: Color) -> Self`

Color of the viewport overlay rectangle. Default solid blue.

#### `pub fn content_outline(mut self, outline: Option<(Color, f32)>) -> Self`

Outline the content extent inside the minimap (gives users a
"you're somewhere inside this much scene" cue when the
minimap is taller / wider than its content). Default `None`.

The outline tracks `content_bounds` *through the projection*,
so once the viewport wanders off it the outline visibly
shrinks and offsets — that is the cue. A width that is not
finite and positive draws nothing.

#### `pub fn on_click<F>(mut self, callback: F) -> Self where F: Fn(Point, &mut EventContext) + 'static,`

Navigation handler: **centre the view on this scene point.**

The contract is that specific on purpose. It is not "here is a point,
do as you like": the keyboard and assistive-technology routes compute
their destination from the current viewport and announce where the
move leaves it, and both of those are only true if the app centres.

`SceneView` has no single "centre on this point" call, so the app
drives the camera it already owns. Either move the view's pan directly
— `SceneView::pan_x_signal` / `pan_y_signal`, or the app-owned pair
handed to `SceneView::view_state` — or, when the view is wrapped in a
`SceneScrollView`, move its `scroll_pos_x_signal` /
`scroll_pos_y_signal`: the same pan through the door the scroll bars
use, already expressed against the scrollable extent so a target can be
clamped to it. `examples/scene_showcase` wires the second.

This route is the silent one — see this module's "Accessibility" for
which routes speak and why the tap is not among them.

Installing it is also what turns the minimap from a read-out into a
control: the node becomes focusable and starts advertising its
actions (see this module's "Accessibility"). Without it the widget
still announces where the viewport is, but takes no focus — a Tab
stop with nothing behind it is worse than none.

A pointer tap inverts the **exact** projection the last paint used —
not a recomputed one — so click-to-recentre lands on what the user is
looking at, even while the viewport is off the content or the parent
handed the minimap less room than it asked for.

#### `pub fn access_readout<F>(mut self, phrasing: F) -> Self where F: Fn(MinimapReadout) -> LocalizedString + 'static,`

Phrase the accessible readout yourself.

The closure receives the geometry as a `MinimapReadout` and returns
the string the AT node announces as its value and every non-pointer
route announces after a move. Use it to translate the reading
(`tr!(minimap_position(across = …, down = …))`) or to say something
the widget cannot know — page numbers, a chapter name, map
coordinates. The default is English, built by the same numbers.

One closure for both the value and the announcement on purpose: they
are the same sentence read at two moments, and two hooks would let
them disagree.

## `pub struct MinimapReadout`

Where the viewport sits inside the picture the minimap is drawing — the
numbers behind the AT node's announced value, as plain fractions.

Handed to an `access_readout` closure so an
app can phrase and translate the announcement itself. Everything here is
derived from the same `effective_extent` the picture is projected through,
so a readout can never describe a frame other than the one on screen.

```rust
pub struct MinimapReadout { /* fields */ }
```
