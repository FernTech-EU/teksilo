<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Density & targets

Teksilo carries one token group — [`InputTokens`] — that answers two questions
a GUI framework has to answer before it can accept a finger: **how big is a
target**, and **how far may a pointer wander before a gesture is recognised**.
It rides on `Theme` as `theme.input`, and it is pure data: no widget, no
recognizer, no dispatch path is named anywhere in it.

At the default density every value equals the constant Teksilo already shipped,
so nothing in this page changes an app's behaviour until it opts in.

## The three densities

`TargetDensity` selects one of three ladders. `Compact` is the default.

| | `Compact` | `Comfortable` | `Touch` |
| --- | --- | --- | --- |
| `target_size` | 24 dp | 32 dp | 44 dp |
| `grab_size` | 6 dp | 10 dp | 16 dp |
| `slop_budget` | 12 dp | 12 dp | 16 dp |
| `spacing_factor` | 1.00 | 1.15 | 1.30 |
| `reveal` | `OnHover` | `OnHover` | `Always` |
| `min_target_conformance` | **24 dp** | **24 dp** | **24 dp** |
| `pen_hit_slop` | 2 dp | 2 dp | 2 dp |
| `lines_per_notch` | 3.0 | 3.0 | 3.0 |
| `touch_enabled` | `true` | `true` | `true` |

`reveal` is `Always` at Touch because a finger never hovers: an affordance that
only appears on hover is unreachable with one.

### The 24 / 44 / 48 ruling

Three numbers get quoted as "the minimum touch target", and they are not
interchangeable. Teksilo commits to the following reading, and the field names
encode it:

- **24 dp is WCAG 2.2 SC 2.5.8 *Target Size (Minimum)*, level AA.** This is the
  conformance floor, and it is what `min_target_conformance` holds. It is
  **24 dp at every density and is never scaled** — not by density, not by the
  global text scale. A `TargetRole::Target` dimension may never come out below
  it.
- **44 dp is Apple's Human Interface Guidelines minimum, and WCAG 2.2
  SC 2.5.5 *Target Size (Enhanced)*, level AAA.** It is the `Touch` ladder's
  `target_size`. **44 dp must never be called AA.** Calling the enhanced
  criterion by the minimum criterion's name is the single most common way a
  conformance claim goes wrong, and it inflates what an app is promising.
- **48 dp is Material 3's touch-target minimum**, from M3's *Accessibility —
  Touch targets*. It is a design-language rule, not a WCAG level. The
  `teksilo-theme-material3` preset applies it on top of the Touch ladder via
  `material3::input_tokens`; every other preset stays at 44.

So: conformance is 24, Apple and AAA are 44, Material is 48. A target that
reaches 24 dp conforms at AA even at `Compact`, which is why the Compact ladder
is a legitimate default and not a deferred accessibility debt.

## Reading and switching density

```rust
// In build(): the values, plus the helpers that encode the floors.
let tokens = &ctx.theme().input;
let row_height = teksilo_core::styles::density::dp(28.0, TargetRole::Target, tokens);
let gap = teksilo_core::styles::density::spacing(8.0, tokens);

// Which ladder is active.
let density = ctx.density();
```

The three helpers live in `teksilo_core::styles::density` (re-exported from
`teksilo-widgets`) so `teksilo-terminal`, `teksilo-webview` and `teksilo-scene`
reach them without a widgets dependency:

| helper | effect |
| --- | --- |
| `dp(base, TargetRole::Target, tokens)` | `base.max(target_size)` |
| `dp(base, TargetRole::Grab, tokens)` | `base.max(grab_size)` |
| `dp(base, TargetRole::Decoration, tokens)` | `base`, unchanged |
| `spacing(base, tokens)` | `base * spacing_factor` |
| `density_min_size(size, axes, tokens)` | raises only the named axes to `target_size` |

Each is the **identity at `Compact`** for a value that already conforms. They
only ever raise: a control that is already generous keeps its own size.

Switch density through the tree, not by hand:

```rust
tree.set_input_density(TargetDensity::Touch);
```

`WidgetTree::set_input_density` writes `theme.with_density(d)` **and marks the
tree for a rebuild** — not `set_theme`'s layout-and-paint dirty pass. A target
size is baked in `build()` (a `MinSize` wrapper, a recipe's metrics, how many
`Toolbar` items fit), and marking layout cannot re-bake it. It reuses the same
`mark_needs_rebuild` + `mark_ancestors_need_layout` path a
`BindingLevel::Rebuild` binding takes, so focus restoration, the AccessKit
re-walk and interaction-state revalidation all come for free.

`Theme::with_density` itself is a **token projection only**. It replaces
`input` and carries `style_slots` and `extensions` across verbatim; it does not
re-run any recipe constructor, because there is nothing in a theme to re-run —
every `ComponentStyleSlots` slot is `None` in the shipped presets, and each
widget builds its `Recipe*Style` lazily at its own build site from
`ctx.theme().input`. A slot an app installed itself is preserved as it is, so a
hand-written Tier-3 style keeps whatever metrics its author gave it.

`DensityPolicy` describes *how* the density is chosen: `Fixed(Compact)` is the
default, and `FollowLastPointer { coarse, fine, hysteresis }` tracks the most
recent pointer kind with a settling delay so a stray event cannot thrash a full
tree rebuild.

## Hit targeting: three mechanisms, three domains

A target that is too small is not one problem, it is three, and they want three
different answers. Teksilo has one mechanism for each, and they do not overlap.

| | what it is | when it runs | what it costs |
| --- | --- | --- | --- |
| **`Widget::target_regions`** | a widget *reports* the sub-targets it paints inside one node | never — reporting only | nothing |
| **`Widget::hit_outset`** | a node absorbs presses past its own edges | **inside** the exact pass | nothing; hit-only |
| **The miss-only slop pass** | a press that hit nothing is re-attributed to the nearest small target | **only after** the exact pass found nothing eligible | nothing; hit-only |
| `TouchTarget` | the slot around a control actually grows | layout, at `Touch` density only | siblings reflow |

Which one do you need?

- Is your control **painted inside another widget's node** — a scroll bar's
  thumb, a slider's knob, a header cell's filter affordance? Then nothing can
  see it, and you owe the framework a `target_regions` report before either of
  the other two can help.
- Does it need to **beat a neighbour**? A splitter gutter lies across the panes
  it divides, and the panes are painted on top of it. Only `hit_outset` can win
  that, because it runs inside the exact pass.
- Is it **alone**, with nothing nearby to steal from — a radio dot, a checkbox,
  a chart mark? The slop pass. Widening its rectangle would take presses from
  the row it sits in; catching a *miss* takes them from nobody.
- Does it sit in a **tight row of other targets**, with no space to borrow? Then
  no amount of hit trickery will do, and `TouchTarget` is the honest answer.

### `Widget::target_regions` — reporting

```rust
fn target_regions(&self, bounds: Rect) -> Vec<TargetRegion> {
    let [label, filter] = partition_targets(bounds, &[0.8, 0.2], 24.0, dir)[..] else { … };
    vec![TargetRegion::target(label, 0), TargetRegion::target(filter, 1)]
}
```

`TargetRegion { rect, role, part }`. `part` is the widget's own discriminator;
`role` decides the floor an audit measures it against. Build the rectangles with
[`partition_targets`] rather than by hand, so the geometry the widget *paints*
and the geometry it *reports* cannot drift apart.

`partition_targets(bounds, fractions, min, direction)` splits one node's
rectangle **horizontally** into a zone per fraction. Weights normalise by their
own sum; every zone gets at least `min` dp by clamp-and-redistribute; the result
is indexed in **reading order**, so `zones[0]` is the leading zone and no caller
re-orders for RTL; the tiling is exact. When `min × zones` exceeds the width
there is no conforming partition, and it splits the width **evenly** — every
zone stays reachable and visibly sub-floor, which is what the target audit is
for, rather than a zone silently vanishing at a narrow width.

A vertical split (a `TreeView` row's before / into / after thirds, a drop
target's edge bands) is [`DropRegion`]'s job; it has to answer in two dimensions
anyway.

#### A zone's floor is a floor, not a fraction

Both in-node splitters state their zones as *proportions* — a header cell's
filter affordance as its glyph plus padding against the label's remainder, a
`DropTarget`'s edge band as a fraction of the axis — and a proportion of a small
node is a zone nothing can hit. Both raise a sub-floor zone to the density's
`target_size` and take the difference from its neighbour, which is the same
clamp-and-redistribute [`partition_targets`] performs, and both cap the floor so
the starved case degrades the way that function's does rather than losing a zone:
a header cell splits evenly, a drop target's floor stops at a third of the extent
so `leading | centre | trailing` keeps its middle.

Unlike an outset, a zone floor is **not** pointer-kind-gated, and it cannot be: a
zone boundary is one number per node, the widget paints its highlight from it, and
a boundary that moved with the device would mean the zone a user sees is not the
zone that acts. It is safe without a gate because it only ever bites where the
declared proportion had already produced a zone too small for anyone — a fifth of
a 100 dp pane is 20 dp for a mouse too.

### `Widget::hit_outset` — inside the exact pass

```rust
fn hit_outset(&self, kind: PointerKind, tokens: &InputTokens) -> EdgeInsets {
    if kind.is_direct() { EdgeInsets::uniform(9.0) } else { EdgeInsets::ZERO }
}
```

Within one parent, children that declare an outset are offered the point
**before** the ordinary reverse-sibling walk, nearest first. That is why a 6 dp
splitter gutter wins over the panes lying on top of it, and why two adjacent
grips split the difference at the midpoint instead of letting sibling order
decide.

Four rules, each pinned by a test:

- **Hit-only.** No layout moves, nothing repaints differently, and a Compact
  build renders byte for byte as it did.
- **It never escapes the parent.** The recursion tests the parent's own bounds
  before it looks at a child, so an outset can only claim space the parent
  already owns — through a `clips_children` ancestor included.
- **Zero for a precise pointer**, unless the widget opts every kind in
  deliberately. A mouse hot-spot is exact and occludes nothing, so widening its
  targets steals clicks. The rich-text image grip is the one control that opts
  in, because its mouse target is genuinely undersized.
- **Resolved through the child.** A point inside the child still resolves
  normally — descendants win, `hit_shape` is honoured — and only a point in the
  ring resolves to the child itself.

The conventional grip value is **9 dp direct / 0 dp precise**, which lifts a
6 dp gutter to a 24 dp target.

### The miss-only slop pass

Runs **only** when the exact pass found nothing that would act on the press.
Every node near the point is asked how far away it really is
(`Widget::hit_distance`, defaulting to the distance to its rectangle; a round
control overrides it beside its existing `hit_shape`), and the nearest one still
inside its earned outset takes the press.

A node's outset is

```text
((up_to − min(width, height)) / 2).clamp(0, radius)
```

with `radius` from the pointer's profile — **0 dp mouse / 8 dp touch / 2 dp
pen** — capped for a *coarse* pointer by `slop_budget` (12 / 12 / 16 dp), and
`up_to` the density's `target_size`. A node already at least `up_to` on its
smaller axis therefore earns **nothing**: a scrim, a page, a list row are
excluded by arithmetic rather than by a rule. And because the mouse radius is
`0.0` at every density, **every mouse hit test is exactly the one Teksilo has
always run** — the pass short-circuits before it walks anything.

#### The bubble-path rule

The exact hit's **entire bubble path** is examined. A slop candidate wins only
if that path carries no eligible handler at all, **or** the candidate is
strictly closer than the bubble owner's *uninflated* shape.

This is what keeps the obvious counter-example correct: a press on a row label
5 dp from an inline checkbox stays on the row. The row owns the press at
distance zero, and nothing beats zero. Turn the row inert and the same press
does reach the checkbox — which is the case the mechanism exists for.

#### Eligibility

| rule | why |
| --- | --- |
| earns a non-zero outset | the formula excludes anything already at `up_to` |
| would act on a press (a pointer handler, or focusable) | re-attributing to an inert node swallows a press silently |
| **enabled** — its own state and every ancestor's | a disabled control ignores presses |
| not **read-only** | likewise; answered by the `TextSurface` registry, via a probe the tree hands the arena |
| no `no_hit_slop` | the explicit opt-out |
| not `event_pass_through` — **but its children are** | it absorbs nothing, so widening it punches a hole in what is behind |
| not inside a `hit_transparent` subtree | those are pruned whole, as in the exact pass |
| no `clips_children` ancestor's **uninflated** rect excludes the point | slop never reaches out of a scroller |
| inside the **topmost overlay layer the exact pass entered** | a press in an open menu can never reach the page behind it |
| `hit_distance` answers `Some(d)` with `0 < d ≤ outset` | `d = 0` is the exact pass's business; `None` withdraws the widget |

A `ModalScrim` never participates. The size formula already excludes a
full-viewport node, but the scrim says `no_hit_slop` outright so the guarantee
does not depend on how large it happens to be.

Under a transform the point is inverse-mapped and the local distance is
converted to screen dp through the transform's **minimum singular value** —
the axis along which a local unit buys the fewest screen pixels, so the reach
never comes out shorter than the token promised on any axis. For a chain of
transforms the product of the per-node minima is a lower bound on the composed
minimum, so a deep stack errs towards being generous rather than short.

### Precedence — one chain

```text
no_hit_slop  >  node .hit_slop(..)  >  Widget::hit_outset / Widget::hit_slop  >  density default
```

`.hit_slop(HitSlop { radius, up_to })` and `.no_hit_slop()` are `WidgetBuilder`
methods, available on any widget and on `HandlerSet`. `no_hit_slop` is the head
of **one** chain covering both widening mechanisms: it silences the widget's
`hit_outset` as well as its slop. It is per-node, not per-subtree — to take a
whole subtree out of hit-testing use `hit_transparent`.

#### An outset claim is not a slop candidate

The chain is one chain in a second sense, and it has to be said outright because
the two mechanisms otherwise compete: **when the exact pass resolved its point
through some node's `hit_outset`, the miss-only pass returns that hit unchanged
and never runs its comparison.** The claim was already made, inside the pass that
is allowed to make it, so there is nothing left to re-attribute. The whole path
from the root to the exact hit is checked, because the outset pre-pass resolves
its candidate *through* the ordinary recursion — the node the exact pass returns
may be a descendant of the grip that won the point.

Without that rule the outset loses every time it is contested. A grip only ever
claims a point at a **positive** distance from its own uninflated shape, which is
exactly the condition under which the bubble-path rule lets a slop candidate
through: any slop-eligible node lying under the ring is strictly closer than the
grip, so it takes the press. The symptom is perverse rather than merely wrong: a
grip's reach *shrinks* as the density gets coarser, because raising `up_to` from
24 to 44 dp turns neighbours that could earn nothing into candidates that can.
And it is not confined to the coarse densities — **at Compact any neighbour still
under 24 dp is already a candidate**, which is a shipped control rather than a
hypothetical: a `SearchField`'s clear button reaches 24 × 24 dp at Compact with
this rule and 22 × 24 without it, while a `TableView`'s scroll bar reaches 32 dp
across its thickness at Touch with it and 18 dp without. Both figures are
assertions —
`an_outsets_claim_survives_the_slop_pass_in_the_shipped_controls` in
`crates/teksilo-widgets/tests/target_conformance.rs` — not prose. The predicate
is `won_through_outset` in `arena.rs`, consulted by `apply_slop` before it walks
any candidate; `no_hit_slop` still silences the outset and the slop together,
which is what keeps one chain one.

### `TouchTarget` — the residue

```rust
TouchTarget::new().child(close_button)          // 44 dp slot at Touch, child centred
TouchTarget::new().reserve_space(false).child(w) // nothing moves; hit area widens instead
```

Inert at `Compact` and `Comfortable`: it forwards its child's full layout
response — grow weight, shrink weight and compression floor — unchanged.
`Compact` is the density every existing layout was designed at, and
`Comfortable` is served by the recipes' own density projection, which raises a
control's *own* dimensions rather than padding around it. `Touch` is the ladder
where a 24 dp control still falls 20 dp short and no recipe can close the gap
from inside.

## Measuring conformance: the audit and its fixture lists

`teksilo_core::accessibility::target_audit` measures how big every target in a
laid-out tree actually is **to a finger**, and each crate holds a named fixture
list that drives it (`tests/target_conformance.rs`). The walker does not add the
mechanisms above up: every one of them is conditional, so it proposes a growth
and then confirms it against the real hit test, probing outward from the point a
user aims at. The module docs carry the reasoning; five rules govern writing a
fixture, and each of them was learnt by a fixture that measured nothing.

- **Build the subject inside the container it ships in.** A control alone in a
  stack over-reports: with no eligible handler on the bubble path the slop pass
  tops up every near miss, so a broken control measures as conformant. A
  tappable row around it is what makes the fixture discriminate.
- **Give the subject the handler its real container gives it.** A `ListView`
  installs its rows' press handler only when it has a `SelectionModel`; a
  `NotificationLog`'s rows are inert without `on_entry_invoked`; a `TwistArrow`
  declares no outset without an `on_click`. Omit one and the subject is not a
  target at all, and the audit measures the furniture around it.
- **Lay out twice where the affordance is gated by a value layout publishes.**
  `Toolbar::is_overflowing` and `SegmentedControl`'s overflow flag are written
  from `place_children` and bound at `Relayout`, so after one pass the overflow
  chevron is still dormant.
- **Assert what was measured, not what appears in the path.** A subject's own
  type name appears in the path of everything beneath it, so a check that reads
  the whole path is satisfied by a child; the claim has to be about the measured
  node.
- **Configure the subject so the regions it reports exist.** A
  `Widget::target_regions` implementation may return an empty list for a default
  configuration, and it takes every part with it — a table's header cell reports
  nothing at all unless one of its columns is filterable, so its label zone,
  filter zone *and* resize grip go unmeasured together. A hook left unmeasured
  this way can be deleted with the gate still green, which is the one failure the
  gate exists to prevent; the check is that every `target_regions` implementation
  in the crate contributes at least one measured part.

Two things the audit cannot see, by construction rather than by omission: a
target with **no arena node** (a `ListView` row where the view routes presses by
index, a text surface's selection handles, a close affordance carved out of one
node's rectangle) unless its widget reports it from `Widget::target_regions`;
and an affordance **revealed by hover**, which does not exist at all below
`RevealPolicy::Always` — which is the correct answer for a finger, since a
finger never hovers.

## Gesture profiles

`theme.input.gestures` holds one `GestureProfile` per pointer-kind family;
`InputTokens::profile(kind)` picks one, with `PointerKind::Unknown` mapping to
the mouse profile (the conservative choice — tightest slop, no hit outset).

Distances are logical pixels (dp), velocities are dp/second.

| field | `MOUSE` | `TOUCH` | `PEN` |
| --- | --- | --- | --- |
| `tap_slop` | 5.0 | 18.0 | 2.0 |
| `drag_slop` | 5.0 | 18.0 | 2.0 |
| `multi_tap_slop` | 10.0 | 40.0 | 12.0 |
| `slop_precise` | 2.0 | 2.0 | 2.0 |
| `long_press_slop` | 5.0 | 18.0 | 10.0 |
| `hit_slop` | 0.0 | 8.0 | 2.0 |
| `pan_slop` | `None` | 36.0 | 8.0 |
| `long_press` | 500 ms | 500 ms | 500 ms |
| `multi_tap_interval` | 300 ms | 300 ms | 300 ms |
| `press_feedback_delay` | 100 ms | 100 ms | 100 ms |
| `max_hold` | 250 ms | 250 ms | 250 ms |
| `min_fling_velocity` | 50.0 | 50.0 | 50.0 |
| `max_fling_velocity` | 8000.0 | 8000.0 | 8000.0 |
| `swipe_min_velocity` | 200.0 | 300.0 | 200.0 |
| `swipe_min_distance` | 30.0 | 40.0 | 30.0 |
| `drag_activation` | `Auto` | `Auto` | `Auto` |

Every profile satisfies
`pan_slop.unwrap_or(∞) > drag_slop >= tap_slop >= slop_precise`, asserted by a
test: a pan must be harder to start than a drag, and nothing may sink below the
precise-device jitter floor.

A pen's slops are *tighter* than a mouse's on purpose — the value of a stylus is
precision, and inheriting 5 dp would throw it away. Its `long_press_slop` is
the exception, and is deliberately larger than its drag slop: a hand resting on
a tablet drifts over half a second, and cancelling at 2 dp would make the
gesture unusable.

### Where each constant comes from

The `MOUSE` column is exactly what Teksilo shipped before the touch programme,
so selecting it is a no-op:

| constant | source |
| --- | --- |
| `tap_slop` 5.0 | `TapRecognizer::new`'s `max_distance`, `teksilo-core/src/gesture/tap.rs:28` |
| `drag_slop` 5.0 | `DragRecognizer::new`'s `threshold`, `gesture/drag.rs:23`, and the explicit `.threshold(5.0)` at `widget_tree/gesture_dispatch_impl.rs:96` |
| `multi_tap_slop` 10.0 | `max_distance` on `DoubleTapRecognizer::new` (`gesture/multi_tap.rs:35`) and `TripleTapRecognizer::new` (`:216`) |
| `long_press_slop` 5.0 | `LongPressRecognizer::new`'s `max_distance`, `gesture/long_press.rs:39` |
| `long_press` 500 ms | `LongPressRecognizer::new`'s `min_duration`, `gesture/long_press.rs:40` |
| `multi_tap_interval` 300 ms | `max_interval` on both multi-tap recognizers, `gesture/multi_tap.rs:36` and `:217` |
| `swipe_min_velocity` 200.0 | `SwipeRecognizer::new`, `gesture/swipe.rs:23` |
| `swipe_min_distance` 30.0 | `SwipeRecognizer::new`, `gesture/swipe.rs:24` |
| `hit_slop` 0.0 | no slop pass exists today — a mouse hit is exact |
| `pan_slop` `None` | a mouse scrolls with the wheel, never by dragging content |
| `lines_per_notch` 3.0 | `const LINES_PER_NOTCH` at `teksilo-platform/src/event_translation.rs:241` |

The `TOUCH` and `PEN` columns are new:

| constant | source |
| --- | --- |
| touch `tap_slop` / `drag_slop` / `long_press_slop` 18.0 | Flutter `kTouchSlop` |
| touch `pan_slop` 36.0 | Flutter `kPanSlop`, defined there as `kTouchSlop * 2` |
| touch `multi_tap_slop` 40.0 | Flutter `kDoubleTapSlop` |
| `long_press` 500 ms | Flutter `kLongPressTimeout` |
| `multi_tap_interval` 300 ms | Flutter `kDoubleTapTimeout` |
| touch `hit_slop` 8.0 | Android `ViewConfiguration`, 8 dp |
| `min_fling_velocity` 50.0 / `max_fling_velocity` 8000.0 | Android `ViewConfiguration` `MINIMUM_FLING_VELOCITY` / `MAXIMUM_FLING_VELOCITY`, dp/s |
| `press_feedback_delay` 100 ms | Flutter `kPressTimeout` / Android `ViewConfiguration.getTapTimeout()` |
| `max_hold` 250 ms | a Teksilo choice between `kPressTimeout` (100 ms) and `kLongPressTimeout` (500 ms); no upstream constant |
| pen `slop_precise` / `tap_slop` / `drag_slop` 2.0, `long_press_slop` 10.0, `multi_tap_slop` 12.0, `hit_slop` 2.0, `pan_slop` 8.0 | Teksilo choices; a digitizer's jitter is the floor, and the long-press allowance covers hand drift |
| touch `swipe_min_velocity` 300.0 / `swipe_min_distance` 40.0 | raised over the mouse column so an imprecise finger drag is not read as a deliberate swipe |

## Scroll physics

`theme.input.scroll_physics` holds the fling / settle / overscroll constants.
Nothing reads them yet — they are declared here so the whole input surface is
one struct and a theme carries it.

| field | value | source |
| --- | --- | --- |
| `physics` | `Platform` | Bouncing on macOS/iOS, Clamping elsewhere |
| `clamping_deceleration_rate` | ≈ 2.3582 | Android `OverScroller::DECELERATION_RATE`, `ln(0.78)/ln(0.9)` |
| `clamping_inflexion` | 0.35 | Android `OverScroller::INFLEXION` |
| `clamping_friction` | 0.015 | Android `ViewConfiguration` scroll friction |
| `bouncing_decay_per_second` | 0.135 | Flutter `BouncingScrollSimulation` |
| `spring_mass` | 0.5 | Flutter `SpringDescription` |
| `spring_stiffness` | 100.0 | Flutter `SpringDescription` |
| `spring_damping_ratio` | 1.1 | just overdamped — the content never overshoots on the way back |
| `rubber_band_factor` | 0.52 | iOS / Flutter `frictionFactor` |

## The touch kill switch

`theme.input.touch_enabled` is a runtime switch, `true` by default:

```rust
tree.set_touch_enabled(false);
```

With it off, the platform translator drops touch input and the router installs
no touch-only recognizers, so an app can fall back to mouse-only behaviour
without a rebuild of the binary. It is repaint-level only — turning touch off
changes which events are accepted, never a dimension.

## Environment flags

`Environment` carries three input-related platform facts, pushed in by
`teksilo-platform` exactly as `prefers_reduced_motion` is:

- `prefers_touch: Option<bool>` — the OS's stated preference for a touch-first
  UI (Windows tablet mode, a convertible in slate posture). `None` when the
  platform does not report one, which leaves the density where the app put it.
- `screen_reader: ScreenReaderState` — `Unknown` / `Inactive` / `Active`.
- `explore_by_touch: ExploreByTouch` — `Off` / `Auto` / `On`. While touch
  exploration is on (VoiceOver, TalkBack, Narrator touch mode) a touch is a
  *probe*: the first tap announces, the second activates, and gesture
  recognition must step aside for it.

All three default to today's behaviour. They are core-side enums rather than a
re-export because `AccessibilityPreferences` lives in `teksilo-platform`, and
`teksilo-core` cannot name it.

[`InputTokens`]: https://github.com/ferntech-eu/teksilo/blob/main/crates/teksilo-tokens/src/input.rs
[`partition_targets`]: https://github.com/ferntech-eu/teksilo/blob/main/crates/teksilo-core/src/partition.rs
[`DropRegion`]: https://github.com/ferntech-eu/teksilo/blob/main/crates/teksilo-core/src/styles/drop_target_style.rs
