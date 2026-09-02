<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Crossfade

`Crossfade` — when an external `Signal<K>` changes, the
previous content fades out while the new content fades in over
the same window. Like `Switcher`,
but animated.

```ignore
let tab = Signal::new(Tab::Overview);
ctx.add(
    Crossfade::new(tab.clone(), |t| match t {
        Tab::Overview => Box::new(overview_panel()),
        Tab::Details  => Box::new(details_panel()),
    }),
);
```

## Behavior

On each `key` change, both the previous-key widget and the
current-key widget are rebuilt (via the supplied builder) and
mounted side-by-side in a `ZStack`. The previous fades 1→0 while
the current fades 0→1 over the configured duration. On the *next*
key change, the previously-outgoing widget is torn down and the
cycle repeats.

Builders should be cheap — they may run more than once per
lifetime as the user navigates through several keys. For data-
heavy panels, hoist expensive state out of the builder closure.

## Reduced motion

Honours `prefers-reduced-motion`: snaps the opacity changes
instead of tweening (instant swap).

## Builder methods at a glance

`duration`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/animations/crossfade/index.html)

## `pub struct Crossfade`

Animated swap between widgets keyed by an external signal.

```rust
pub struct Crossfade<K: Eq + Clone + 'static> { /* fields */ }
```

### Methods

#### `pub fn new(key_signal: Signal<K>, builder: impl Fn(&K) -> Box<dyn Widget> + 'static) -> Self`

New `Crossfade` driven by `key_signal`. The `builder` closure
constructs the widget for a given key value. Builders can be
invoked multiple times across the widget's lifetime as the
user transitions through keys.

#### `pub fn duration(mut self, duration: Duration) -> Self`

Override the crossfade duration. Default: `MotionTokens::duration_normal`.
