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
