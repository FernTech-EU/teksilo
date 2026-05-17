# Animation

**Companion to:** [bastyde-architecture.md](bastyde-architecture.md)
**Scope:** Signal-driven animation in Bastyde — `Signal<f32>::animate_to`, the scheduler behind it, and the design rules for deciding when (and when not) to animate.

---

## 1. Why animation exists in a mostly-instant framework

Bastyde's motion vocabulary is borrowed from JetBrains's Int UI design language: hover and press are **instant**, and animation is reserved for a narrow set of floating transitions — a dialog appearing, a snackbar sliding in, an accordion expanding, a toggle thumb moving. A serious desktop application that a user drives for hours gets tiring fast if every state change fades or slides; interaction feedback needs to be crisp. Decorative animation is explicitly discouraged.

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

Easing curves and standard durations are design tokens, not ad-hoc magic numbers. They live in [crates/bastyde-tokens/src/motion.rs](../crates/bastyde-tokens/src/motion.rs):

```rust
pub enum Easing { Linear, EaseIn, EaseOut, EaseInOut }

pub struct MotionTokens {
    pub duration_instant: Duration,             //   0 ms — most state changes
    pub duration_fast:    Duration,             // 120 ms — tooltip fade, interactive feedback
    pub duration_normal:  Duration,             // 200 ms — notification slide
    pub duration_slow:    Duration,             // 300 ms — dialog scale-in
    pub duration_collapse:               Duration, // 200 ms — accordion / disclosure tween
    pub duration_indeterminate_sweep:    Duration, // 900 ms — indeterminate sweep / spinner period
    pub easing_standard:  Easing,               // mild ease-out
}
```

Widgets reach for `MotionTokens` through the current `Theme`. Int UI's guidance — one mild ease-out for everything — is the default; a theme can override if a platform target wants a different feel. Avoid hardcoding durations in widget code when a token fits; the tokens are the lever a designer rebrands a theme through.

The `Easing::apply(t)` method takes a linear parameter `t ∈ [0, 1]` and returns the eased value in the same range. `lerp(a, b, t)` in the same file does plain linear interpolation; the scheduler combines the two to produce each frame's value.

## 4. The scheduler — what the framework owns

`AnimationScheduler` is the **signal-tween** scheduler — one of three
visibility-aware motion subsystems on `WidgetTree`. The other two are
[`AnimatedQuadRegistry`](../crates/bastyde-core/src/animated_quad.rs)
(shader-driven quad uniforms — `Spinner`, `ProgressBar::indeterminate`,
animated `IconWidget`; see
[idle-and-animation.md](idle-and-animation.md#three-animation-paths-signal-vs-shader-vs-per-frame-effect))
and
[`FrameTickScheduler`](../crates/bastyde-core/src/frame_tick_scheduler.rs)
(per-frame-effect closures — `Pulse`, `Cycle`; see §5.6 below). All
three share the
[`motion_visibility`](../crates/bastyde-core/src/motion_visibility.rs)
helpers so the visibility gate is one canonical primitive.

Widget code never constructs `AnimationScheduler` directly. Its job is
small but non-trivial:

- **Tick every active animation** on each frame the tree pumps. Current value = `lerp(start, end, easing.apply(t))`; `t` is elapsed/duration clamped to `[0, 1]`.
- **Stop cleanly** when an animation reaches its target (set exactly the end value on the terminal tick, regardless of epsilon quantization).
- **Pause when the window is occluded or unfocused** (`set_window_active(false)`). The scheduler reports no next deadline to the event loop during pause, so a hidden window doesn't keep the event loop in `WaitUntil`. On resume, each animation's start time is rebased by the paused duration — a sweep paused at 50% resumes from 50%, phase-continuous, not snapped.
- **Cancel when the driving widget disappears.** `cancel_by_widget(id)` runs whenever a widget is rebuilt or destroyed. Otherwise a `Signal<f32>` clone in the scheduler would outlive its widget, silently ticking a signal whose observers no longer exist.
- **Skip offscreen ticks for *looping* animations.** If a widget hasn't painted in the latest paint epoch, the scheduler holds back ticks for any *looping* animation it owns — keeps the animation alive internally but doesn't drive work through it. This avoids re-paints for spinners and indeterminate progress bars inside a minimized split-view pane or a closed accordion subtree. **One-shot animations always tick** regardless of paint epoch: a widget like `Collapse` whose own size depends on the animated value (zero height when collapsed → never painted → never re-stamped) would deadlock if the gate also covered one-shots. The cost is bounded — a one-shot with no observers on screen still completes in `duration` and then stops itself.

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

If a widget constructs its `Signal<f32>` outside `build()` (a handful of widgets do, to share the signal with callers), it can call `ctx.register_animated_signal(&signal)` inside `build()` to associate the signal with the current widget for cancellation purposes. See [ScrollArea](../crates/bastyde-widgets/src/scroll_area.rs) and [TreeView](../crates/bastyde-widgets/src/tree_view.rs) for examples.

### 4.2 `animate_looping` for indeterminate work

For animations that should run until explicitly cancelled — spinners, marquee tickers, indeterminate progress bars — `Signal::animate_looping(target, period, easing, frame_interval)` sets the signal to its start value each time it reaches the target and loops indefinitely. This is the path used by [ProgressBar](../crates/bastyde-widgets/src/progress_bar.rs) in indeterminate mode:

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

### 4.3 `AnimationSpec` — the recommended façade

Reaching for `animate_to(target, Duration::from_millis(150), Easing::EaseInOut)` directly works, but it shifts three responsibilities onto the call site: pulling the right `MotionTokens` constant from the theme, picking pixel-stable `epsilon` / `frame_interval` defaults for looping animations, and remembering to honour `prefers_reduced_motion`. `AnimationSpec` is a fluent builder that captures all three at construction time:

```rust
// One-shot, theme-aware, accessibility-aware:
let spec = ctx.animate().fast().standard();
spec.to_or_snap(&knob_position, target);
//                ^^^^^^^^^^^^ snaps without tween under prefers-reduced-motion

// Looping with sub-perceptual epsilon and 60 Hz throttle baked in:
ctx.animate().sweep().linear().to(&sweep_pos, 1.0);
//             ^^^^^^^ implies looping(), reads duration_indeterminate_sweep
```

Duration presets (`fast()` / `normal()` / `slow()` / `collapse()` / `sweep()` / `instant()`) all read from the live theme's `MotionTokens` — no hardcoded `Duration::from_millis(...)` literals at the call site. Easing presets (`standard()` / `linear()` / `ease_in_out()` / etc.) similarly pull `easing_standard` from tokens. `looping()` flips on sub-perceptual ε = 1/255 and a 60 Hz frame interval (16.667 ms) — the safe defaults for paint-bound loops, matching the most common display refresh rate so a continuous loop advances once per vsync; `frame_interval(d)` overrides for slower loops (e.g. 66 ms = 15 Hz for a wide sweep where the eye can't resolve faster motion). `to(&signal, target)` always tweens; `to_or_snap(&signal, target)` snaps without tween when `prefers_reduced_motion` is true.

`AnimationSpec` is a thin façade — it constructs an `AnimationRequest` and calls `Signal<f32>::try_animate_with_options`. The lower-level `animate_to` / `animate_looping` paths remain public; reach for them only when you need control the spec doesn't expose (custom epsilon for non-pixel signals, `max_duration` for indefinite loops with a bounded budget). Source: [crates/bastyde-core/src/animation_builder.rs](../crates/bastyde-core/src/animation_builder.rs).

## 5. Worked examples from the widget tree

### 5.1 Toggle thumb — the canonical transform transition

[crates/bastyde-widgets/src/toggle.rs](../crates/bastyde-widgets/src/toggle.rs):

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let initial = if self.on.get() { 1.0 } else { 0.0 };
    self.knob_position = ctx.animated_signal(initial);
    let knob_spec = ctx.animate().fast().standard();
    // ...
    let toggle = move || {
        let new_on = !on.get();
        on.set(new_on);
        let target = if new_on { 1.0 } else { 0.0 };
        knob_spec.to_or_snap(&knob_position, target);
    };
    // ...
}
```

- `knob_position` is recreated each `build()` — not preserved across rebuilds. This is intentional: rebuilds are rare (theme change, structural dirty), and the value is trivially restorable from `self.on`.
- The `paint()` method reads `knob_position.get()` directly and positions the knob at `lerp(left_edge, right_edge, position)`.
- No `Signal::map`, no `Prop::Bound` wrapping needed — direct read in paint is fine because the scheduler's `signal.set()` on each tick already dirty-marks the widget for repaint via the binding registry.
- `to_or_snap` quietly snaps to the target without tweening when the platform reports `prefers_reduced_motion` — same handler code, accessible by default.

### 5.2 Accordion / Collapse — animating a layout dimension

[Accordion](../crates/bastyde-widgets/src/accordion.rs) wraps its content in a [`Collapse`](../crates/bastyde-widgets/src/collapse.rs) widget — the reusable primitive for the "animate child between hidden and natural height" pattern. `Collapse` drives an internal `Signal<f32>` from 0..1 with `ctx.animate().collapse().standard().to_or_snap(...)`, and its `layout_response` reports `natural * progress` while `place_children` always lays the child out at its full natural size. The framework's clip pass crops the overflow during the tween. Effect: the visible height interpolates over the full duration without the child being squashed (which would re-wrap text and produce flicker).

Animating a layout-participating dimension is more expensive than animating a paint-only value — the binding level is `Relayout`, not `RepaintOnly`, so the tree dirtys the accordion's ancestor's layout on each tick. Use sparingly. Paint-only targets (offsets, scales, opacities — see §5.6) are cheaper.

### 5.3 Snackbar slide-in

[crates/bastyde-widgets/src/snackbar.rs](../crates/bastyde-widgets/src/snackbar.rs) uses `animate_to` on a slide-offset signal that the paint phase applies as a `y` translation. The snackbar is placed via `OverlayPlacement::BottomCenter` and the animation slides the overlay in from below. At auto-dismiss time, the same signal animates back to its offscreen position before the overlay is removed.

### 5.4 Smooth programmatic scroll

When ScrollArea receives a `ScrollIntoView` request (for example after tab-focusing a child that is offscreen), it calls `scroll_y.animate_to(target_offset, ...)` instead of `scroll_y.set(target_offset)`. The user sees the viewport slide to the new position rather than jumping. See [scroll_area.rs:376](../crates/bastyde-widgets/src/scroll_area.rs).

### 5.5 Icon widget — sprite-sheet frame animation

[icon_widget.rs](../crates/bastyde-widgets/src/primitives/icon_widget.rs) animates a frame-index signal looping over the frame count for animated WebP icons. Frame interval comes from the asset, not from `MotionTokens` — this is a content-driven animation, not a UI transition.

### 5.6 Wrap-and-go animation widgets

Animation wrappers live under [`crates/bastyde-widgets/src/animations/`](../crates/bastyde-widgets/src/animations/) and are re-exported flat from `bastyde::widgets`. They package the common animation patterns so callers don't re-implement them:

- **[`Fade`](../crates/bastyde-widgets/src/animations/fade.rs)** wraps a child and tweens an internal opacity signal between 0 and 1 driven by a `Signal<bool>`. Layout-transparent: the child reports its full natural size at all opacity values. Built on `BuildContext::set_opacity`, a node-level opacity scope (parallel to `clips_children`) emitted by the rendering walker as `SetOpacity` / `RestoreOpacity` draw commands wrapping the subtree. Sub-perceptual opacities (`< 1/512`) are short-circuited — no draw passes.
- **[`Collapse`](../crates/bastyde-widgets/src/animations/collapse.rs)** — see §5.2. The accordion-pattern primitive.
- **[`Scale`](../crates/bastyde-widgets/src/animations/scale.rs)** wraps a child and animates a uniform 2D scale on its entire subtree, driven by a `Prop<bool>`. Built on `BuildContext::set_transform` (see §5.7) — the renderer composes the scale matrix onto its transform stack so the wrapped subtree's text and shapes visually shrink together. Two modes: **visual-only** (default, `reflow=false`) — the slot stays at the child's natural size, only the visual content scales around the chosen origin (use for overlay enter/exit, "boop" feedback); **reflow** (`.reflow(true)`) — the wrapper's reported size shrinks with progress so siblings reflow as the wrapped content disappears (use for "card removal", pair with `ScaleOrigin::TopLeading` so the visual stays anchored at the slot's top-left as it collapses). Distinct from `Collapse`: `Collapse` shrinks one axis and "wipes" content via clipping (text inside stays full-size); `Scale` shrinks uniformly and text/icons visually get smaller.
- **[`Rotate`](../crates/bastyde-widgets/src/animations/rotate.rs)** wraps a child and applies a 2D rotation (radians) to its subtree via `set_transform`. Layout-stable. No internal animation — the caller owns the angle signal and pairs it with `Signal::animate_to` for animated rotations. Use for animated chevrons (replacing the old "flip-two-static-icons" trick), spinning loaders not covered by `Spinner`, dial controls.
- **[`Blur`](../crates/bastyde-widgets/src/animations/blur.rs)** wraps a child and applies a Gaussian-equivalent blur to the entire subtree, driven by a `Prop<f32>` radius (logical pixels). Built on `BuildContext::set_blur` (see §5.7) — the renderer redirects the subtree's draws into an intermediate texture, runs a dual-Kawase chain at the requested radius, and composites the blurred result back at the widget's bounds. Layout-transparent: the child reports its full natural size at all radii. Sub-perceptual radii (`< 0.5`) are short-circuited at the walker — no offscreen pass, no allocation. Use for modal backdrops, click-to-reveal sensitive content (numerics / characters obscured by the blur), out-of-focus emphasis, animated frosted glass on modal show. Pair with an `animated_signal` and `animate_to` for animated enable/disable. See §5.8 for the offscreen-pass cost model.
- **[`Spinner`](../crates/bastyde-widgets/src/spinner.rs)** — circular-arc loading indicator backed by `AnimatedQuadKind::SpinnerArc`, the shader-driven path (see [idle-and-animation.md §"Two animation paths — signal vs shader"](idle-and-animation.md#two-animation-paths-signal-vs-shader)). One `queue.write_buffer` of `AnimParams` + one `draw_indexed` per frame; `paint()` does not run while spinning. Edges are anti-aliased via `fwidth` smoothstep ramps in the fragment shader. Honours `prefers-reduced-motion` with a static three-quarter arc fallback.

**[`Pulse`](../crates/bastyde-widgets/src/animations/pulse.rs)** and **[`Cycle`](../crates/bastyde-widgets/src/animations/cycle.rs)** drive their continuous motion through the **per-frame-effect path** rather than `AnimationScheduler` (which only knows linear tweens) or `AnimatedQuadRegistry` (which is paint-time GPU plumbing). They register a closure on `ctx.frame_tick()` for the tick action and a `ctx.subscribe_frame_tick()` RAII guard for visibility-aware chain management — the framework re-arms the frame chain after every render iff at least one subscriber's owner widget was painted in that frame, so a `Pulse` parked inside a non-selected `Switcher` branch contributes zero idle frames and resumes phase-continuous on the next show. The chain bootstrap (request_frame on subscription) and resume (post-render arm after the visible_when-driven repaint) are both handled by the framework. Widget code in the new shape:

```rust
pub struct Pulse {
    // …
    frame_tick_sub: Option<FrameTickSubscription>,
}

impl Widget for Pulse {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // … set_opacity, reduced-motion early-out, … 
        ctx.effect(&ctx.frame_tick(), move |&delta| {
            // mutate opacity from sine of accumulated phase
        });
        self.frame_tick_sub = None;                        // drop old guard first
        self.frame_tick_sub = Some(ctx.subscribe_frame_tick());
        // …
    }
}
```

If you find yourself reaching for `ctx.frame_request_handle().set(true)` from inside a `frame_tick` effect for a visual continuous animation, prefer `subscribe_frame_tick()` instead — the raw handle keeps the event loop pumping regardless of visibility, while the scheduler-backed path auto-pauses on hidden owners. The raw handle is still the right tool for short-lived, owner-driven needs that aren't visibility-bound (caret blink that depends on focus state, drag auto-scroll while the pointer is captured).

Other wrappers in the same module — `SmoothSize`, `Crossfade`, `Slide`, `Shake` — are documented inline in their source files; run `cargo run -p animations-kit` for a visual showcase of every wrapper.

### 5.7 Per-node paint scopes

The framework ships four per-node paint scopes that wrapper widgets attach to themselves:

- **`BuildContext::set_opacity(id, prop)`** — the original. Render walker emits `SetOpacity(value)` / `RestoreOpacity` around the subtree; renderer maintains a stack and multiplies through. Bound at `RepaintOnly`. Used by `Fade`, `Pulse`, `Crossfade`, `OverlayRequest::with_fade`.
- **`set_clips_children(id, true)`** — a per-node clip rectangle to the node's own bounds. Used by `ScrollArea`, `Collapse`, `MaxSize`, `Slide`, `Shake`, `SmoothSize`, `Scale`, `Rotate` — anything whose subtree may overshoot the slot.
- **`BuildContext::set_transform(id, prop)`** — added with `Scale` and `Rotate`. Render walker emits `PushTransform(matrix)` / `PopTransform` around the subtree; renderer maintains a transform stack. Bound at `RepaintOnly` by default; reflow-driving wrappers (e.g. `Scale::reflow(true)`) must additionally bind their *driver signal* to themselves at `Relayout` for layout to track the value.
- **`BuildContext::set_blur(id, prop)`** — added with `Blur`. Render walker emits `BeginBlurredSubtree { bounds, radius }` / `EndBlurredSubtree` around the subtree; the renderer redirects drawing into an intermediate texture, runs a dual-Kawase blur chain at the requested radius, and composites the blurred result back. Bound at `RepaintOnly`. The only scope that triggers an offscreen render pass and per-frame texture allocation — see §5.8.

Scope nesting order on a single node, from outermost to innermost: `BeginBlurredSubtree → SetOpacity → PushTransform → ...paint...`. Reverse on close. The blur scope is OUTER on purpose: it captures the already-faded, already-transformed subtree into the intermediate texture so animated `fade-in-and-blur` behaves intuitively (the blur is applied to whatever the user would see, post-fade, post-transform).

The transform scope has one non-obvious twist worth pinning down for future widget authors: **`SetTransform` semantics are "compose with stack-top", not "set absolute"**. A widget's own canvas-level transforms (`canvas.translate(5, 5)` etc.) emit `SetTransform` commands relative to the widget's own identity baseline; under a wrapper push, the renderer composes them onto the stack-top transform instead of clobbering it. With an empty stack (the default for any widget not under a transform wrapper), `stack_top = identity` and composition is a no-op — the change is purely additive and backwards-compatible. Identity-valued transforms are skipped at the walker layer (no push/pop emitted), so wrappers at their rest pose pay zero per-frame cost. Sub-perceptual blur radii (`< 0.5 px`) are similarly skipped — animated `0 → target_radius` enable patterns pay zero cost when fully off.

### 5.8 Offscreen render passes — when blur breaks the single-pass model

The renderer is single-pass by default: every `DrawCommand` flows into one `wgpu::RenderPass` targeting the surface texture. `Blur` is the exception. A `BeginBlurredSubtree { bounds, radius }` / `EndBlurredSubtree` pair carves out a sub-pass that:

1. Allocates an intermediate RGBA8 texture sized to `bounds × scale_factor` (drawn from a renderer-side recycled pool keyed on power-of-two sizes — no per-frame allocation hot path).
2. Renders the subtree's draw commands into that texture (with a translation pushed onto the transform stack so the subtree paints at `(0, 0)` of the intermediate).
3. Runs the dual-Kawase chain on it: `N = ceil(log2(radius))` downsample passes (each halves the texture, applies a 4-tap bilinear shader), then `N` upsample passes back to the source size with a different 4-tap shader.
4. Composites the final blurred texture into the parent pass at `bounds` via the standard quad pipeline (which already samples textures — this is just a textured-quad blit).

Each blur scope = `N + N + 1` small render passes per frame for typical UI radii (R = 8–24 → N = 3–5). Cheap individually, but every blur scope opens a new pass on the encoder and breaks batch coalescing on the surrounding draws — don't sprinkle `Blur` widgets through a list view. Stable layouts (modal backdrops, sensitive-content panels, frosted side panels) are the natural fit.

The reference for this offscreen-render pattern is [`png_export.rs`](../crates/bastyde-preview-ui/src/png_export.rs), which has been creating intermediate `RENDER_ATTACHMENT | COPY_SRC` textures and routing the renderer at them since the widget previewer shipped — the blur engine generalises that pattern into a recursive sub-pass.

For overlays, **`OverlayRequest::with_fade(duration)`** is the recommended path for tooltip / popover / snackbar fade-in / fade-out. The framework wires opacity internally — caller specifies just the duration:

```rust
tree.show_overlay(OverlayRequest {
    content_id, anchor, placement, dismiss,
    layer: OverlayLayer::InTree,
    parent_overlay: None, on_dismiss: None,
    fade_duration: Some(theme.motion.duration_fast),
});
```

When `fade_duration` is `Some`, `WidgetTree` creates an animated `Signal<f32>`, applies it as an opacity scope on the content (via `set_opacity` — same primitive `Fade` uses), kicks off the 0→1 tween at show time, and on dismiss reverses to 0 then defers the actual stack removal by `duration` so the tween plays out before the content goes dormant. The `OverlayManager` tracks fade-out state on a dual sim/real clock so headless tests can use `tree.advance_time(...)` to drive deterministic dismissal.

## 6. When NOT to use the animation system

Animation via `animate_to` is for a value that crosses time smoothly. It is **not** for:

- **Color shifts on hover / press.** Those are instant in Int UI's vocabulary. Express them as `Signal<Role>` mapped from the interaction state signal (see [reactive-theme.md](reactive-theme.md) §"Interaction-driven colors"). No scheduler involved; color resolves from the current theme per frame.
- **Caret blink.** The caret is either drawn or not, on a cadence. It uses `BuildContext::frame_tick()` + `request_frame()` to pump the event loop on a schedule and flips a boolean — no smooth interpolation happens, so `animate_to` would be the wrong tool.
- **Fade-in of a list of items appearing on filter change.** Decorative; Int UI's guidance is "don't." If the visual disruption is bad enough to warrant fade, consider whether the list widget itself should not disrupt — for example, a virtualized list that only creates newly-visible items, rather than full remount on filter change.
- **Tooltip delay.** The *opening* delay is a timer, not an animation. Once the tooltip appears, its fade-in is driven by `OverlayRequest::with_fade(theme.motion.duration_fast)` (see §5.6) — the framework owns the opacity tween, callers do not roll their own `animate_to` on the tooltip content.
- **Single-axis disclosure** (an accordion section opening, a drawer expanding to a known height). Use `Collapse` instead of `Scale` — `Collapse` clips on one axis without re-running text layout or applying a transform scope, so it's cheaper and the visual ("shutter rolls down, text stays at full size") matches what users expect for disclosure. `Scale` is for uniform shrink-around-a-pivot ("card disappears", "icon boops"), where the visual content itself should get smaller.
- **Per-list-row blur or anything that animates blur radius every frame.** `Blur` is the most expensive scope in the framework — every enabled scope opens a separate render pass and runs `2N+1` Kawase passes per frame. For "fade-blur on reveal" patterns, animate the radius up to a static value and leave it there. For per-row obscuration of sensitive data, prefer a per-row text-redaction primitive over wrapping each row in `Blur`.

## 7. Testing animations deterministically

The `WidgetTree::advance_time(duration)` method on the [test_api](../crates/bastyde-core/src/widget_tree/test_api.rs) advances the tree's simulated clock and runs the scheduler with the new `now`. This makes animation tests deterministic without needing real wall time:

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

- Recommended entry point: `ctx.animate().<duration>().<easing>().to_or_snap(&signal, target)` (§4.3). Captures `MotionTokens`, easing presets, and `prefers_reduced_motion` in one place.
- Lower-level entry points for cases the spec doesn't cover: `Signal<f32>::animate_to(...)`, `animate_looping(...)`, `try_animate_with_options(AnimationRequest)`.
- Create the signal with `BuildContext::animated_signal(value)` inside `build()`. That handles scheduler registration and widget-lifetime cancellation.
- Respect `ctx.prefers_reduced_motion()` before starting a looping animation; `to_or_snap` already does it for one-shots.
- Durations come from `MotionTokens` (`duration_fast` / `_normal` / `_slow` / `_collapse` / `_indeterminate_sweep`), not from literal constants in widget code. Literal constants are acceptable for one-off durations a designer doesn't plan to retune (icon sprite frame intervals, for example).
- Easing curves come from `Easing`. `EaseInOut` for symmetric transitions (toggle thumbs), `EaseOut` / `easing_standard` for appearance (snackbar slide-in, dialog fade), `Linear` for loops and indeterminate work.
- For common shapes — fade an overlay, collapse a section, show a spinner — reach for `Fade` / `Collapse` / `Spinner` / `OverlayRequest::with_fade` (§5.6) before hand-rolling a signal-driven path.
- Don't animate colors, hovers, presses, focus states, or anything instant in Int UI's vocabulary — those are reactive theme work, not scheduler work.

---

## See also

- [reactive-theme.md](reactive-theme.md) — reactive theming for the "it's not really animation, it's just reactive color" path (hover, press, focus).
- [bastyde-architecture.md §20 Threading](bastyde-architecture.md) — where the per-frame tick fits in the event loop.
- [crates/bastyde-core/src/animation.rs](../crates/bastyde-core/src/animation.rs) — scheduler source.
- [crates/bastyde-core/src/signal.rs](../crates/bastyde-core/src/signal.rs) (`Signal<f32>::animate_to` et al).
- [crates/bastyde-tokens/src/motion.rs](../crates/bastyde-tokens/src/motion.rs) — `Easing` and `MotionTokens`.
