<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# SceneCard

`SceneCard` — the chrome and the gesture regime of a heavyweight scene
item, with **no opinion about what is inside it**.

# Why it is a card and not a note

A note container, a pinned image, an embedded web page, a chart, a group of
dried ink and a sub-page thumbnail want the same five things — a surface, a
grab handle, a selection state, an edit state, and an accessibility shape —
and differ only in their body. Naming the type after one of those bodies
would put text-specific policy (a document, a commit, a word count) into a
crate whose whole claim is that the heavyweight tier is *any widget*. So the
card owns the container and the app owns the content:

```ignore
SceneCard::new(model.clone(), id)
    .height_for_width()
    .label(title.clone())
    .header(TextWidget::new(title))
    .body(RichTextEditor::editor(doc))
```

# The gesture regime, which is the part that could not be written outside

Three presses, three different owners, decided **structurally** rather than
by a recognizer race:

| press lands on | who owns the gesture | why |
| --- | --- | --- |
| the header | the header's own `on_drag` | the innermost node with a drag owns the sequence outright, and no ancestor is enrolled |
| the body | whatever the body installed (a text field's selection drag) | no drag on the captured node, so the walk climbs — and stops at the card root's dead zone |
| the card background | nothing but the card's own tap | the card root **is** the dead-zone boundary, so nothing above it arms |

The load-bearing line is
`gesture_dead_zone(true)`
on the card root. Without it the `SceneView`'s marquee is enrolled as an
ancestor of every press inside the card, and dragging from the middle of a
note rubber-bands the page behind it. With it, a drag inside a card is the
card's business and the canvas never sees it.

The trailing header slot is wrapped in a
`DeadZone` of its own — the
`Accordion` header precedent exactly — so the `⋮` button can be clicked
*with the jitter a real click carries* without starting a move.

# What drags a card, and what resizes it

The **header** moves it, always. The selection frame the
`transform controller` draws moves and resizes it
too, once the card is selected — and that frame is drawn `padding` outside
the selection precisely so it lands on pixels the card does not own. There
is deliberately no third route: a body drag belongs to the body (that is how
you select text in an embedded editor), and a card with no header is a card
you move by its frame.

Both routes end in the same model write. The header drag accumulates a
scene-space translation, previews it as a node transform, runs it past the
document's standing rule with
`SceneModel::constrain_move` — the same
door the built-in drag uses, so a snap-to-grid rule cannot mean two things —
and commits **once**, on release, through
`SceneModel::apply_transform_delta`.
One gesture is one reversible step, and a cancelled gesture has nothing to
roll back because nothing was written.

# Modes, and why there is no `on_commit`

`CardMode` is a `Signal` the **app owns**. The card writes into it
(`Editing` on activation, `Selected` when focus leaves its subtree or `Esc`
is pressed) and reads it for its chrome and its accessibility state. An app An app
that wants to persist on commit observes that signal; there is no second
`on_commit` callback, because the trigger for the most important case —
focus leaving the subtree — is the framework's `focus_within` signal, which
is written outside event dispatch. A callback there could not be handed an
`EventContext`, so it would be a worse `Signal` with a misleading shape.
`on_activate` *does* take one, because an
activation is a gesture or a key and has a dispatch to belong to.

# Accessibility

**One** `Role::Group` per card, named by `label`,
carrying `selected` when the mode says so and a custom **Edit** action that
is the non-pointer twin of the double-click. It is a tab stop, so a keyboard
user reaches every card with Tab, enters one with Enter and leaves with Esc.

One, not two: the default surface is announced as a
`Role::GenericContainer` with no properties, which is the role the
accessibility walker prunes, promoting its children in order. Left alone,
`Card` publishes a second, nameless `Role::Group` *inside* the card's own,
and a screen reader reads that as a container within a container. The net
result is node-for-node identical to the hand-rolled `Panel` a note page
used before — with the container **named**, where the panel's was not.
Pinned by `a_card_publishes_no_more_nodes_than_the_hand_rolled_panel_it_replaces`.

Everything inside the card is walked by the framework's ordinary walker —
the header's text, the trailing button, and a body that publishes
`Role::TextRun` children all hang off the group in the emitted tree.

⚠ **They do not currently reach a screen reader, and the card is not what
stops them.** The default surface is `teksilo_widgets::Card`, whose
`RecipeCardStyle` body calls `AccessNodeBuilder::set_hidden()` on itself to
mean "presentational". `set_hidden` is
`FilterResult::ExcludeSubtree` in `accesskit_consumer::common_filter` — the
filter every platform adapter and this repo's own `accessibility::audit`
read through — so it removes the surface **and everything under it**, not
just itself. Measured through a real consumer tree, a card with a title and
a body reads `Window > Pane > Group "Note"` and nothing else; the
hand-rolled `Panel` it replaces reads `Window > Group` and nothing else, and
a `StatusBar` reads `Window > Status` and nothing else. The parity test
below passes because the baseline is broken in exactly the same way.

The intended "presentational" is one line away and the card already uses it
on the surface's own node: `Role::GenericContainer` with no properties,
which is `ExcludeNode` — the node goes, its children are promoted. Fixing it
is a sweep across every content-wrapping recipe surface rather than a change
to this file, so it is recorded here rather than made here.

## Tab stops, and the one thing the card does not decide

The card contributes exactly **one** tab stop — itself, the object handle.
It does not take its body's away while idle, and the reason is not
reluctance: `set_tab_stop` reaches one node, and a composite body's tab
stops are its own inner nodes. Nothing short of parking the body dormant
takes a subtree out of the Tab ring.

So a card with a focusable body is two stops, always. An app that wants one
stop per idle note puts a `Switcher` in the
body — a read-only viewer and an editor, driven by the same
`CardMode` — because a `Switcher` parks its hidden branch dormant, which
takes it out of focus, hit-testing *and* the accessibility tree while
keeping it mounted, so a mode round-trip does not destroy the caret. That
shape is the framework's answer to this question and the card defers to it
rather than building a worse one.

## …and the one frame that shape costs

Deferring to the `Switcher` has a consequence the card has to answer for:
**the editor does not exist yet at the moment the user asks for it.**
A double-click flips `CardMode` inside one dispatch, and
`EventContext::request_focus_into` is drained at the end of that same
dispatch — but the `Switcher`'s branch is parked until its `visible_when`
gate is evaluated, which happens in the *next* layout pass. So the obvious
code focuses a subtree that has nothing focusable in it, the caret never
appears, and the two recommended shapes — one tab stop, and focus on
activation — are mutually exclusive.

The card closes that by asking twice. `CardHandle::activate` asks straight
away, which is what a plain focusable body wants (instant, same dispatch),
and a private zero-size `EditFocusGate` child asks again from
`BuildContext::run_after_mount`
— the one hook that hands a widget a real `EventContext` *after* the pass
that mounts its siblings. The second ask is skipped when the card's
`focus_within` already says the keyboard is inside, so a body that took the
focus itself (or an `on_activate` that placed it deliberately) is never
yanked back to the first field. The caret lands one frame after the
double-click instead of zero, and it lands.

The same gate is what makes the two-way `mode` contract
true from the outside: a toolbar button that writes `Editing` into the
signal gets the body focused too, with no dispatch of its own to do it in.

## Builder methods at a glance

`header`, `header_trailing`, `body`, `surface`, `mode`, `mode_signal`, `selection`, `label`, `activate_label`, `movable`, `height_for_width`, `size_policy`, `on_activate`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-scene/latest/teksilo_scene/index.html)

## `pub enum CardMode`

What a `SceneCard` is doing.

Three states rather than a `bool`, because `Selected` and `Editing` differ
in *both* the accessibility shape and the gesture regime: a selected card is
an object the canvas can move, and an editing card has handed the keyboard —
arrows, Backspace, type-ahead — to its content.

`#[non_exhaustive]`: this crate has out-of-tree consumers.

```rust
pub enum CardMode { /* variants */ }
```

### Variants

- **`Idle`** — Not selected, not editing. The resting state.
- **`Selected`** — Part of the scene selection: move, resize and delete apply.
- **`Editing`** — The body has the keyboard.

### Methods

#### `pub fn is_selected(self) -> bool`

Whether this mode reads as "selected" to assistive technology. `Editing`
does: a card being edited is the selected one.

## `pub struct SceneCard`

A heavyweight scene item's container: surface, header, body, modes,
gestures and accessibility. See the `scene_card` module docs.

```rust
pub struct SceneCard { /* fields */ }
```

### Methods

#### `pub fn new(model: SceneModel, item: ItemId) -> Self`

A card bound to one heavyweight entry of `model`.

The card **is** the widget the view's delegate returns for `item`:

```ignore
SceneView::with_model(model.clone())
    .delegate_typed::<Note>(move |note, id| {
        Box::new(SceneCard::new(model.clone(), id).body(view_of(note)))
    })
```

#### `pub fn header(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

The grab handle, and the card's title strip.

A press here moves the card and nothing else — see the
`scene_card` module docs for the three-row table that makes that true.

Takes a widget, a `Box<dyn Widget>` (the shape a card factory hands
back), or a `WidgetId` already in the tree — one method per slot, so
the caller does not pick a spelling to match what they happen to hold.

#### `pub fn header_trailing(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

A control at the trailing end of the header — a `⋮` menu, a colour
swatch, a pin.

Wrapped in a `DeadZone`, so the
header drags everywhere **except** here and a click on the control with
a few pixels of jitter still reads as a click.

Takes a widget, a boxed widget or a `WidgetId`, like
`header`.

#### `pub fn body(mut self, widget: impl teksilo_core::IntoTeksiChild) -> Self`

The content. Anything at all: a text editor, an image, a chart, a
`SceneView` of its own.

Not wrapped in anything — the card root's dead zone already covers it,
and wrapping would put a node between the body and the card that the
body's own gesture arena would have to argue with.

Takes a widget, a boxed widget or a `WidgetId`, like
`header`.

#### `pub fn surface(mut self, f: impl Fn(WidgetId) -> Box<dyn Widget> + 'static) -> Self`

Replace the chrome. The closure is handed the id of the card's content
(header plus body, already stacked) and returns the widget that wraps
it.

The default is
`Card::new().variant(CardVariant::Elevated).content(content)`, so the
card is Tier-3 themed through the existing `style_slots.card` with no
new style protocol. Override it for a different variant, a per-card
colour, or a surface of your own:

```ignore
card.surface(|content| {
    Box::new(Card::new().variant(CardVariant::Outlined).content(content))
})
```

#### `pub fn mode(mut self, mode: Signal<CardMode>) -> Self`

Bind the card's mode to an app-owned signal.

Two-way: the card writes into it and reads from it, so a toolbar button
or a shortcut can put a card into `Editing` and the card will focus its
body. Observe it to persist on commit — see the `scene_card` module docs for
why there is no separate `on_commit`.

#### `pub fn mode_signal(&self) -> Signal<CardMode>`

The card's mode signal, for an app that did not supply one.

#### `pub fn selection(mut self, selection: SceneSelection) -> Self`

The scene selection this card takes part in.

Pass the same handle the view was given
(`SceneView::selection_model`), so
a tap on the card and a marquee across it mean the same thing. Without
one the card still tracks its own `CardMode`, but a tap selects
nothing.

#### `pub fn label(mut self, label: impl Into<Prop<String>>) -> Self`

The card's accessible name. Locale-reactive when given a `tr!` string.

#### `pub fn activate_label(mut self, label: impl Into<Prop<String>>) -> Self`

The name of the custom accessibility action that enters edit mode.
Defaults to an untranslated `"Edit"`.

#### `pub fn movable(mut self, movable: bool) -> Self`

Whether the header moves the card. `true` by default; `false` leaves the
header as an ordinary strip (and the selection frame as the only way to
move the card).

#### `pub fn height_for_width(self) -> Self`

Give the card's **height** to its content:
`SizePolicy::HeightForWidth` on the entry.

The width stays the model's — you resize a note by dragging its edge —
and the height follows the words on every pass that lays the card out.

One consequence worth knowing before you reach for it: **the height axis
of the selection frame does nothing on such a card.** Not "writes a value
that is then corrected" — nothing at all. The scale's vertical component
is neutralised at the model door
(`Scene::apply_transform_delta`),
because a height written there is one the next pass measures straight
back over: the gesture's whole contribution would be a reversible step
that undoes nothing, and a top-edge drag would *move* the card rather
than resize it. Dragging a side or corner still changes the width, and
the words decide the rest. That is the intended reading of "the content
decides" — and it is why this is opt-in rather than the default.

The **horizontal** half is an ordinary resize and converges: the preview
reflows the card live as the handle moves, nothing reaches the model
until the release, and the release writes the width once.

#### `pub fn size_policy(self, policy: SizePolicy) -> Self`

`height_for_width`, stated in full. See
`SizePolicy`.

#### `pub fn on_activate(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Double-click, `Enter` on the focused card, or the AT **Edit** action.

Runs **before** the mode flips, and **any write it makes to the mode is
the decision**: the card leaves the mode alone and does nothing further.
Write `CardMode::Editing` to drive an edit flow of your own, or
anything else to refuse this activation. A hook that writes nothing lets
the card flip to `Editing` as usual.

The comparison is against the mode the hook *found*, not against a fixed
value, because a double-click arrives with the card already
`Selected` — the press selected it — and a rule
phrased against a fixed value could not tell a refusal from the state
the gesture had already produced.
