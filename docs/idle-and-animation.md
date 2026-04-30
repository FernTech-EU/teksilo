# Idle and animation — the zero-frame rule

## The rule

**An idle app must draw zero frames.** Not "almost zero". Not "a
cheap 30 Hz". Zero — `rendered_frames == 0` in the
`FERN_IDLE_TRACE=1` trace, `ControlFlow::Wait` in winit, no GPU submit,
no CPU wake, no battery drain.

"Idle" means:

- No user input (no cursor move, click, key, scroll, resize).
- No pending tooltip / delayed overlay / gesture deadline.
- No accessibility rebuild in flight.

If those conditions hold and the event loop still wakes up, it is a
bug. Track it down.

## Why so absolute

FernUI is meant for long-running desktop apps. A 30 Hz idle pump costs CPU, GPU, battery, fan noise, and — on laptops — holds the package out of deep C-states. Compounded
across every running animation, every unfocused window, every
background process, it is the difference between "I left it open" and
"my battery is dead".

A framework that draws at idle normalises wasted cycles. We refuse.

## The machinery that enforces it

Four gates, all in the animation subsystem. Any new source of idle
wakes must be designed to respect them — or add its own gate with
equivalent rigor.

1. **Widget-drop / rebuild auto-cancel.** The scheduler holds strong
   `Signal<f32>` clones; without an explicit cancel on widget death,
   a rebuilt widget leaks its old animation forever and ticks against
   an orphaned signal. `WidgetTree::rebuild_single_widget` and
   `destroy_subtree` both call `scheduler.cancel_by_widget(id)` before
   reconstructing. If you add a new lifecycle path that replaces
   widget state, it must do the same.
   ([animation.rs](../crates/fern-core/src/animation.rs),
   [widget_tree.rs](../crates/fern-core/src/widget_tree.rs))

2. **Per-window active flag.** `WindowEvent::Focused(false)` (and
   on macOS, `Occluded(true)`) calls `tree.set_window_active(false)`,
   which makes `AnimationScheduler::tick` a no-op and
   `next_deadline` return `None`. The event loop falls through to
   `ControlFlow::Wait`. On resume, each animation's `start_time` is
   rebased by the paused duration so phase is continuous — a
   half-swept sweep resumes at 50%, not snapped forward.
   ([app.rs](../crates/fern-app/src/app.rs),
   [window_manager.rs](../crates/fern-app/src/window_manager.rs))

3. **Per-widget paint-epoch visibility (loops only).** `WidgetTree::paint_epoch`
   ticks on every non-cache-hit `render()`. `paint_widget_cached`
   stamps `last_painted_epoch` on each widget whose bounds survive
   clip intersection. The scheduler skips any **looping** animation
   whose widget's `last_painted_epoch + 1 < paint_epoch` — a
   scrolled-off spinner pauses itself. When the widget scrolls back
   in, the resulting paint re-stamps its epoch and
   `update_control_flow` re-queries `next_deadline` in `post_event`,
   re-arming the animation. `paint_epoch == 0` is the "never
   rendered" sentinel: always visible, so headless unit tests that
   only call `layout()` don't regress.

   **One-shots are *not* gated by visibility.** A widget like
   [`Collapse`](../crates/fern-widgets/src/collapse.rs) drives a
   one-shot 0..1 progress signal that determines its own height —
   so when collapsed, its bounds are zero, it never paints, never
   re-stamps `last_painted_epoch`, and a visibility gate would
   chicken-and-egg the expand: never tick, never grow, never paint.
   Gating only loops keeps the scrolled-off-spinner saving while
   making widget-driven layout tweens work. The cost is bounded —
   a one-shot with no observers on screen still completes in
   `duration` and stops itself, so the worst case is a single
   tween's worth of background ticks.
   ([rendering_impl.rs](../crates/fern-core/src/widget_tree/rendering_impl.rs),
   [arena.rs](../crates/fern-core/src/arena.rs),
   [animation.rs](../crates/fern-core/src/animation.rs))

4. **Pixel-stable ε, mandatory terminal bypass.** Each
   `AnimationRequest` can carry an `epsilon` (unit: the signal's own
   units, so usually logical pixels). Intermediate ticks skip
   `signal.set` when the value hasn't moved by at least ε — no dirt,
   no frame. Terminal ticks (completion, loop restart) always set
   unconditionally so one-shots land on exactly `end_value`. Ship ε
   for any looping animation whose minimum visible delta is known
   (ProgressBar → 1 px of track width is a safe choice).

## The idle-work audit

`WidgetTree::needs_redraw()` is the predicate the event loop uses to
decide between `ControlFlow::Wait` and `ControlFlow::WaitUntil`. If
it returns `true`, the app is not idle — even if nothing visible is
animating. Any new "is there work pending" signal **must** be
included in this predicate, and paused/hidden variants **must** be
excluded. The scheduler's `has_running` (pause-aware) rather than
`has_active` (pause-oblivious) is the reference pattern; a paused
scheduler that still said `has_active == true` would defeat every
gate above.

## Verifying you haven't regressed the rule

Run the catalog with the idle trace. A truly idle app emits no
trace line at all (the trace is written on each wake — no wake, no
line):

```bash
cargo build --profile profiling -p widget-catalog
FERN_IDLE_TRACE=1 timeout 10 ./target/profiling/widget-catalog 2> /tmp/idle.log
wc -l /tmp/idle.log   # expect 0
```

CPU and GPU deltas can be read from the kernel:

```bash
# Process CPU over 10s (see /tmp/measure_idle.sh in the tree for the
# full script — samples /proc/<pid>/stat and sysfs gpu_busy_percent)
/tmp/measure_idle.sh
# Expect: cpu < 0.5%, gpu delta ≈ baseline.
```

If the numbers are above baseline, something in the tree is waking
the loop. Classify it:

- **Looping animation not paused?** Check `tree.is_window_active()`
  and the widget's `last_painted_epoch` vs. `tree.paint_epoch`.
- **Timer source you forgot?** Every timer-backed deadline must flow
  through `next_timer_deadline` in
  [overlay_impl.rs](../crates/fern-core/src/widget_tree/overlay_impl.rs).
  If it doesn't, the event loop can't decide whether to sleep.
- **Poll mode forced?** `ControlFlow::Poll` is used for
  `frame_tick_requested` (caret blink, drag auto-scroll). It must
  clear itself the frame it is no longer needed.

When in doubt, bisect: remove widgets from the scene until the idle
returns to zero. The last removal is the culprit.

## For widget authors

If your widget schedules anything time-driven — animation, timer,
poll, deferred callback — it must have an explicit answer for each
of the four gates. `ctx.prefers_reduced_motion()` is a fifth pre-gate
for decorative motion: honor it, and you get the zero-motion
accessibility behavior and a free idle win.

## Two animation paths — signal vs shader

FernUI carries two animation paths that coexist. Pick by shape:

| Path | When to use | Cost when visible | `paint()` re-runs per frame? |
| --- | --- | --- | --- |
| **`Signal<f32>::animate_to` / `animate_looping`** (via `AnimationScheduler`) | Tweens driving arbitrary values: scroll offsets, sidebar slide, toggle knob, slider fill width, any custom interpolation your `paint()` consumes | CPU: `signal.set` → `paint()` → vertex-buffer rewrite → wgpu submit. Tight but per-frame. | Yes. |
| **`ctx.animated_quad(kind)`** (via `AnimatedQuadRegistry`) | Decorative motion that fits a quad + shader: `ProgressBar::indeterminate` (procedural sweep), `Spinner` (procedural arc), animated `IconWidget` (sprite-atlas frame cycling), future pulse / shimmer / skeleton | CPU: one `queue.write_buffer` of the `AnimParams` struct (64 B per active quad) + one `draw_indexed` call. `paint()` does not run. | **No.** |

Use signal when `paint()` needs the current animated value to compute
its draw commands (e.g., scroll offset shifts every child's
coordinates). Use shader when the animation's visual is expressible as
"draw a quad, let a fragment shader decide pixels from a small state
struct."

**Widget-author surface.** In `build()`:

```rust
// Procedural sweep (ProgressBar).
self.handle = Some(ctx.animated_quad(AnimatedQuadKind::IndeterminateSweep {
    period: Duration::from_millis(900),
    sweep_ratio: 0.42,
    track_color: SurfaceRole::Sunken.into(),
    fill_color:  SurfaceRole::Accent.into(),
}));

// Procedural arc (Spinner). Anti-aliased via fwidth smoothstep
// in shaders/anim_procedural.wgsl — soft alpha at radial bounds and
// arc start/end. Pipeline already uses ALPHA_BLENDING, so the soft
// alpha composites correctly.
self.handle = Some(ctx.animated_quad(AnimatedQuadKind::SpinnerArc {
    period: theme.motion.duration_indeterminate_sweep,
    arc_fraction: 0.75,
    stroke_fraction: 0.12,
    color: TextRole::Accent.into(),
}));

// Sprite atlas (animated IconWidget — frames pre-packed into a grid).
self.handle = Some(ctx.animated_quad(AnimatedQuadKind::SpriteCycle {
    image_name: atlas_name.clone(),
    frame_count, cols, rows,
    period: icon.total_duration(),
    tint: Some(TextRole::Primary.into()),  // None for FullColor icons
}));
```

In `paint()`:

```rust
canvas.draw_animated_quad(bounds, handle.slot(), AnimatedQuadClass::Procedural);
// or: AnimatedQuadClass::Sprite { image_name: atlas_name.clone() }
```

The four gates (pause-on-window-unfocused, per-widget paint-epoch
visibility, widget-drop/rebuild auto-cancel, `prefers_reduced_motion`)
apply to both paths in identical shape — the shader path reuses the
same registry infrastructure as the signal scheduler. The only
difference is where the animation tick runs: CPU-side for signal,
shader-side for the quad.

Adding a new kind: extend `AnimatedQuadKind`, add a `kind: u32`
discriminator branch in `shaders/anim_procedural.wgsl` (or
`anim_sprite.wgsl` for texture-sampling kinds), and update
`AnimatedQuadRegistry::compute_params` to populate the shared
`AnimParams` struct from the kind's fields.

## Damage rects — measured, deferred

A natural next optimisation for shader-driven animations would be
**damage rects**: track per-frame dirty regions, set
`wgpu::RenderPass::set_scissor_rect` so the GPU only rasterises the
changed pixels, and pass a damage region to the OS compositor so it
skips recompositing the rest of the window. Wayland has
`wl_surface.damage_buffer`; macOS has `CAMetalLayer` dirty rects.

**We measured before committing to this.** With three
`ProgressBar::indeterminate` on the Animated tab of
[`examples/animations`](../examples/animations/src/main.rs):

| Metric | Value |
| --- | --- |
| CPU (process) | **1.83 %** of one core |
| GPU delta vs baseline | **+0 pt** (within sysfs `gpu_busy_percent` noise) |
| Idle / Static tab CPU | **0.00 %** |

Profile breakdown (perf, 8 s window on Animated tab) — the 1.83 %
is dominated by:

- kernel + amdgpu + vulkan memory allocation via `ioctl` (~3 % of
  samples) — wgpu's internal staging for
  `queue.write_buffer(anim_uniforms, 8 KiB)` every frame,
- `Renderer::render` command encoding (1.28 %),
- winit event dispatch (0.93 %),
- miscellaneous one-shot setup (font hinting) that amortises to
  zero in longer windows.

**None of that is rasterisation time.** Damage rects reduce GPU
pixel work and compositor recomposite work — neither is on the hot
path at this scale. Implementing damage rects would require a
multi-day refactor (persistent swap-chain back buffer, per-widget
dirty-rect tracking through overlays / clips / DPI changes, per-OS
compositor integration) for no measurable win on typical UI-sized
workloads.

**Revisit when any of these trigger:**

- 60 Hz or 120 Hz looping animations become common (we're 30 Hz).
- Target display resolution goes 4K / multi-monitor.
- Many simultaneous animated widgets (dozens of spinners across a
  dashboard).
- Battery-sensitive hand-held / laptop deployment where every
  milliwatt counts.
- Real workload profiling shows rasterisation or compositor cost
  exceeding the CPU cost we measured above.

**Cheaper follow-up that would actually help today**: the wgpu
staging-buffer allocation for the per-frame `queue.write_buffer` is
the biggest remaining cost. A persistent mapped uniform buffer, or
writing only the slots that changed since the last tick, would
directly target the ~3 % kernel-side overhead. Single-file change,
no architectural lift — the natural next step if the 1.83 % ever
needs to drop further.
