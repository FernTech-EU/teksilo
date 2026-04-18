# Animation

**Companion to:** [fern-ui-architecture.md](fern-ui-architecture.md)
**Scope:** Signal-driven animation in FernUI — `Signal<f32>::animate_to`, the scheduler behind it, and the design rules for deciding when (and when not) to animate.

---

## 1. Why animation exists in a mostly-instant framework

FernUI's motion vocabulary is borrowed from JetBrains's Int UI design language: hover and press are **instant**, and animation is reserved for a narrow set of floating transitions — a dialog appearing, a snackbar sliding in, an accordion expanding, a toggle thumb moving. A serious desktop application that a user drives for hours gets tiring fast if every state change fades or slides; interaction feedback needs to be crisp. Decorative animation is explicitly discouraged.

What's left is the minimum set of places where motion helps a user track a change:

- **Transform transitions** that would otherwise teleport the eye (toggle thumb 0 → 1, accordion height 0 → full).
- **Floating element appearance** that should not pop into existence (tooltip fade at ~120 ms, balloon slide at ~200 ms, dialog scale-in at ~300 ms).
- **Indeterminate progress** where a looping animation communicates "still working" (progress bar indeterminate mode).
- **Smooth scrolling** when programmatic `scroll_to` would otherwise jump the viewport.

Everything else — hover color shifts, focus ring appearance, press feedback, checkmark toggles — is instant. The framework's theming pipeline (reactive `Signal<Role>` → theme lookup per frame) covers "the color changed" without a scheduler at all; see [reactive-theme.md](reactive-theme.md).

## 2. Signal<f32> as the animation substrate

The entire animation API is attached to `Signal<f32>`. There is no separate `Animation` type for widget authors to manage, no `AnimationController`, no lifetime tracking by hand:

```rust
// Somewhere in build() or a handler:
knob_position.animate_to(1.0, Duration::from_millis(150), Easing::EaseInOut);
```

The `knob_position: Signal<f32>` then interpolates from its current value to the target over the given duration. Any widget observing the signal (via `Prop<f32>`, via `observe()`, or via `binding.bind_to(..., BindingLevel::RepaintOnly)`) re-paints on each tick as the value slides. Because animation flows through the same signal plumbing as any other reactive value, animated widgets do not need special awareness of the scheduler.

A signal created with `Signal::new(0.0_f32)` does **not** support animation — `animate_to` panics. Animation-capable signals are created with `Signal::new_animated(value)`, or — the usual path inside a widget's `build()` — with `BuildContext::animated_signal(value)`, which also registers the signal with the tree's scheduler so the scheduler can cancel the animation if the owning widget is rebuilt or destroyed (see §4 below).

## 3. Easing and durations live in tokens

Easing curves and standard durations are design tokens, not ad-hoc magic numbers. They live in [crates/fern-tokens/src/motion.rs](../crates/fern-tokens/src/motion.rs):

```rust
pub enum Easing { Linear, EaseIn, EaseOut, EaseInOut }

pub struct MotionTokens {
    pub duration_instant: Duration,  //   0 ms — most state changes
    pub duration_fast:    Duration,  // 120 ms — tooltip fade
    pub duration_normal:  Duration,  // 200 ms — notification slide
    pub duration_slow:    Duration,  // 300 ms — dialog scale-in
    pub easing_standard:  Easing,    // EaseOut
}
```

Widgets reach for `MotionTokens` through the current `Theme`. Int UI's guidance — one mild ease-out for everything — is the default; a theme can override if a platform target wants a different feel. Avoid hardcoding durations in widget code when a token fits; the tokens are the lever a designer rebrands a theme through.

The `Easing::apply(t)` method takes a linear parameter `t ∈ [0, 1]` and returns the eased value in the same range. `lerp(a, b, t)` in the same file does plain linear interpolation; the scheduler combines the two to produce each frame's value.

## 4. The scheduler — what the framework owns

`AnimationScheduler` lives on `WidgetTree` (one per tree / one per window). Widget code never constructs one directly. Its job is small but non-trivial:

- **Tick every active animation** on each frame the tree pumps. Current value = `lerp(start, end, easing.apply(t))`; `t` is elapsed/duration clamped to `[0, 1]`.
- **Stop cleanly** when an animation reaches its target (set exactly the end value on the terminal tick, regardless of epsilon quantization).
- **Pause when the window is occluded or unfocused** (`set_window_active(false)`). The scheduler reports no next deadline to the event loop during pause, so a hidden window doesn't keep the event loop in `WaitUntil`. On resume, each animation's start time is rebased by the paused duration — a sweep paused at 50% resumes from 50%, phase-continuous, not snapped.
- **Cancel when the driving widget disappears.** `cancel_by_widget(id)` runs whenever a widget is rebuilt or destroyed. Otherwise a `Signal<f32>` clone in the scheduler would outlive its widget, silently ticking a signal whose observers no longer exist.
- **Skip offscreen ticks.** If a widget hasn't painted in the latest paint epoch, the scheduler does not call `signal.set(...)` for it — it keeps the animation alive internally but doesn't drive work through it. This avoids re-paints for animations inside a minimized split-view pane or a closed accordion subtree.

The frame-loop integration point is `WidgetTree::process_pending_animations` (called from `layout()`) plus `scheduler.tick(now, &arena, paint_epoch)` called from the event loop. Widget authors don't invoke either directly.

### 4.1 Why `animated_signal` specifically

`BuildContext::animated_signal(value)` is the one-line way to get an animation-capable `Signal<f32>` that is correctly tied to the calling widget's lifetime:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    self.knob_position = ctx.animated_signal(if self.on.get() { 1.0 } else { 0.0 });
    // ...
}
```

The signal it returns supports `animate_to`, is registered with the scheduler, and has its owner recorded as `ctx.self_id()`. When the widget is rebuilt (or destroyed), the scheduler cancels all animations on signals owned by that widget — no orphan tickers.

If a widget constructs its `Signal<f32>` outside `build()` (a handful of widgets do, to share the signal with callers), it can call `ctx.register_animated_signal(&signal)` inside `build()` to associate the signal with the current widget for cancellation purposes. See [ScrollArea](../crates/fern-widgets/src/scroll_area.rs) and [TreeView](../crates/fern-widgets/src/tree_view.rs) for examples.

### 4.2 `animate_looping` for indeterminate work

For animations that should run until explicitly cancelled — spinners, marquee tickers, indeterminate progress bars — `Signal::animate_looping(target, period, easing, frame_interval)` sets the signal to its start value each time it reaches the target and loops indefinitely. This is the path used by [ProgressBar](../crates/fern-widgets/src/progress_bar.rs) in indeterminate mode:

```rust
self.indeterminate_pos = ctx.animated_signal(0.0);
self.indeterminate_pos.animate_looping(
    1.0,
    INDETERMINATE_SWEEP_DURATION,
    Easing::Linear,
    Some(INDETERMINATE_FRAME_INTERVAL),
);
```

Looping animations respect `prefers_reduced_motion`: widgets check `ctx.prefers_reduced_motion()` before starting them and fall back to a static representation when the user has disabled motion. Non-looping transitions typically don't need the check — a one-shot 150 ms ease is below the threshold most accessibility guidance worries about — but looping ones always do.

## 5. Worked examples from the widget tree

### 5.1 Toggle thumb — the canonical transform transition

[crates/fern-widgets/src/toggle.rs](../crates/fern-widgets/src/toggle.rs):

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let initial = if self.on.get() { 1.0 } else { 0.0 };
    self.knob_position = ctx.animated_signal(initial);
    // ...
    let toggle = move || {
        let new_on = !on.get();
        on.set(new_on);
        let target = if new_on { 1.0 } else { 0.0 };
        knob_position.animate_to(target, Duration::from_millis(150), Easing::EaseInOut);
    };
    // ...
}
```

- `knob_position` is recreated each `build()` — not preserved across rebuilds. This is intentional: rebuilds are rare (theme change, structural dirty), and the value is trivially restorable from `self.on`.
- The `paint()` method reads `knob_position.get()` directly and positions the knob at `lerp(left_edge, right_edge, position)`.
- No `Signal::map`, no `Prop::Bound` wrapping needed — direct read in paint is fine because the scheduler's `signal.set()` on each tick already dirty-marks the widget for repaint via the binding registry.

### 5.2 Accordion height — animating a layout dimension

[crates/fern-widgets/src/accordion.rs](../crates/fern-widgets/src/accordion.rs):

```rust
let height_state = ctx.animated_signal(initial_height);
// Later, from the expand/collapse handler:
height.animate_to(target, Duration::from_millis(200), Easing::EaseInOut);
```

Animating a layout-participating dimension is more expensive than animating a paint-only value — the binding level has to be `Relayout`, not `RepaintOnly`, so the tree dirtys the layout of the accordion's ancestor on each tick. Use this sparingly. Most animations should target paint-only properties (offsets, scales, opacities projected into paint).

### 5.3 Snackbar slide-in

[crates/fern-widgets/src/snackbar.rs](../crates/fern-widgets/src/snackbar.rs) uses `animate_to` on a slide-offset signal that the paint phase applies as a `y` translation. The snackbar is placed via `OverlayPlacement::BottomCenter` and the animation slides the overlay in from below. At auto-dismiss time, the same signal animates back to its offscreen position before the overlay is removed.

### 5.4 Smooth programmatic scroll

When ScrollArea receives a `ScrollIntoView` request (for example after tab-focusing a child that is offscreen), it calls `scroll_y.animate_to(target_offset, ...)` instead of `scroll_y.set(target_offset)`. The user sees the viewport slide to the new position rather than jumping. See [scroll_area.rs:376](../crates/fern-widgets/src/scroll_area.rs).

### 5.5 Icon widget — sprite-sheet frame animation

[icon_widget.rs](../crates/fern-widgets/src/primitives/icon_widget.rs) animates a frame-index signal looping over the frame count for animated WebP icons. Frame interval comes from the asset, not from `MotionTokens` — this is a content-driven animation, not a UI transition.

## 6. When NOT to use the animation system

Animation via `animate_to` is for a value that crosses time smoothly. It is **not** for:

- **Color shifts on hover / press.** Those are instant in Int UI's vocabulary. Express them as `Signal<Role>` mapped from the interaction state signal (see [reactive-theme.md](reactive-theme.md) §"Interaction-driven colors"). No scheduler involved; color resolves from the current theme per frame.
- **Caret blink.** The caret is either drawn or not, on a cadence. It uses `BuildContext::frame_tick()` + `request_frame()` to pump the event loop on a schedule and flips a boolean — no smooth interpolation happens, so `animate_to` would be the wrong tool.
- **Fade-in of a list of items appearing on filter change.** Decorative; Int UI's guidance is "don't." If the visual disruption is bad enough to warrant fade, consider whether the list widget itself should not disrupt — for example, a virtualized list that only creates newly-visible items, rather than full remount on filter change.
- **Tooltip delay.** The *opening* delay is a timer, not an animation. Once the tooltip appears, its fade-in can use `animate_to` on an opacity signal if a theme wants that — but the delay before it shows is a scheduled task, not a running interpolation.

## 7. Testing animations deterministically

The `WidgetTree::advance_time(duration)` method on the [test_api](../crates/fern-core/src/widget_tree/test_api.rs) advances the tree's simulated clock and runs the scheduler with the new `now`. This makes animation tests deterministic without needing real wall time:

```rust
let mut tree = WidgetTree::new();
let toggle_id = tree.add(Toggle::new(Signal::new(false)));
tree.layout(SizeProposal::exact(200.0, 100.0));

// Trigger the toggle's tap handler:
tree.dispatch_synthetic_click(toggle_id);

// Advance past the 150 ms animation:
tree.advance_time(Duration::from_millis(150));

// Assert the knob is fully at position 1.0:
// (read through a test-api accessor, not shown)
```

Headless tests that never call `render()` have `paint_epoch == 0`; the scheduler treats that as "all widgets visible" so the per-widget visibility gate doesn't make tests flaky.

`AnimationScheduler::active_count()` and `has_active()` are public for tests that want to assert the scheduler is (or isn't) still running.

## 8. Design rules in one list

- One entry point: `Signal<f32>::animate_to(target, duration, easing)`.
- One looping entry point for indeterminate work: `animate_looping(target, period, easing, frame_interval)`.
- Create the signal with `BuildContext::animated_signal(value)` inside `build()`. That handles scheduler registration and widget-lifetime cancellation.
- Respect `ctx.prefers_reduced_motion()` before starting a looping animation. One-shot transitions under ~300 ms rarely need the check.
- Durations come from `MotionTokens` (`duration_fast` / `_normal` / `_slow`), not from literal constants in widget code. Literal constants are acceptable for one-off durations a designer doesn't plan to retune (icon sprite frame intervals, for example).
- Easing curves come from `Easing`. `EaseInOut` for symmetric transitions (toggle thumbs), `EaseOut` for appearance (snackbar slide-in, dialog fade), `Linear` for loops and indeterminate work.
- Don't animate colors, hovers, presses, focus states, or anything instant in Int UI's vocabulary — those are reactive theme work, not scheduler work.

---

## See also

- [reactive-theme.md](reactive-theme.md) — reactive theming for the "it's not really animation, it's just reactive color" path (hover, press, focus).
- [fern-ui-architecture.md §20 Threading](fern-ui-architecture.md) — where the per-frame tick fits in the event loop.
- [crates/fern-core/src/animation.rs](../crates/fern-core/src/animation.rs) — scheduler source.
- [crates/fern-core/src/signal.rs](../crates/fern-core/src/signal.rs) (`Signal<f32>::animate_to` et al).
- [crates/fern-tokens/src/motion.rs](../crates/fern-tokens/src/motion.rs) — `Easing` and `MotionTokens`.
