# Phase 0 — perf summary at 60 Hz

## Setup

- Hardware: AMD Ryzen AI 9 HX 370 w/ Radeon 890M (integrated graphics).
- OS: Linux 6.17.0-115022-tuxedo, Wayland session.
- Build profile: `--profile profiling` with `RUSTFLAGS="-C force-frame-pointers=yes" CARGO_PROFILE_PROFILING_DEBUG=2`.
- Sampler: `perf record -F 999 -g --call-graph fp` for 20 s of steady-state animation after 5 s warmup.
- Per-process metrics also captured via `perf stat`.
- wgpu trace skipped: the framework does not initialize `env_logger`, so `RUST_LOG=wgpu_*` produces no output. Not pursued — perf data is sufficient.

## What was actually profiled (important caveat)

Both example binaries were left in their default no-interaction state:

- **`animations`** = **4 always-running indeterminate `ProgressBar`s**. All four use the `AnimatedQuadKind::IndeterminateSweep` shader-driven path. **No widget on this scene uses the per-frame-effect path** (no `Pulse`, no `Cycle`).
- **`animations_kit`** = **4 always-running `Spinner`s**. All four use the `AnimatedQuadKind::SpinnerArc` shader-driven path. The other wrappers in the kit (`Pulse`, `Cycle`, `Fade`, `Slide`, `Scale`, `Rotate`, `Blur`, `SmoothSize`, `Crossfade`) must be triggered by interaction to start animating; in this perf capture they are mounted but quiescent.

So both profiles characterise the **shader-driven AnimatedQuad scene at 60 Hz** — the path that already has the most aggressive optimisation in the codebase (visibility-gated, no `paint()` re-runs, single `queue.write_buffer` per frame). The framework-level costs surfaced below (heap churn in layout, a11y rebuild, RenderFrame merge) are general — they affect any animating scene — but **the per-frame-effect path used by `Pulse` and `Cycle` is not exercised here.** A future profile run with that path active (driven by either user interaction in `animations_kit` or a custom always-running scene) would tell us whether `Pulse`/`Cycle` introduce additional cost categories on top of what's reported here.

Raw artifacts kept in this directory: `perf_animations.data` (276 KB), `perf_animations_kit.data` (219 KB), `perf_stat_animations.txt`. Reproducible via the commands above.

## Headline finding

**`queue.write_buffer` is not in the top 22 hotspots of either example.** The 30-Hz-era profile that motivated "persistent uniform buffer" no longer holds at 60 Hz — wgpu's per-queue staging belt has fully amortised, and the wgpu-internal staging path costs roughly nothing at steady state.

Per the plan's decision matrix in §0.3:

> | `queue.write_buffer` is amortised away (steady-state cost ≈ 0) | **Drop the persistent buffer**; ship only `bytemuck::Pod` cleanup + slot-delta as a code-quality / future-proof change. |

So Phase A's substantive work (the `MAP_WRITE` staging buffer) is the wrong fix and **must not ship**. Phase A reduces to: bytemuck cleanup on `AnimParams` + `Vec<bool>` slot-delta tracking on the registry — together a small principled-groundwork PR with zero measurable impact on the bench scenes.

## Top-3 CPU cost sources (animations, 1177 samples / 20 000 slots ≈ 5.9 % CPU; matches bench 5.2 % within noise)

1. **Heap allocation churn from `layout_widget_recursive`** — combined ~6 % of CPU.
   - `_int_free` 3.26 % — call-graph rooted in `layout_with_ops → layout_widget_recursive` (1.61 % of total CPU directly attributable to this path).
   - `_int_malloc` 2.88 % — same path, 1.14 % attributable.
   - `malloc` 2.01 % — same path, 1.27 % attributable.
   - `malloc_consolidate` 1.28 %, `unlink_chunk` 0.51 %, plus contributions from `__memmove_avx512_unaligned_erms` 3.07 %.
   - `__memmove_avx512`'s call graph terminates at `handle_window_event_inner` without further breakdown — likely a mix of layout's `Vec<WidgetPlacement>` shuffling and `RenderFrame::merge`'s draw-command Vec cloning.
2. **`bastyde_render::renderer::Renderer::render`** — 2.57 % (raw function body, excluding callees).
3. **`build_accessibility_recursive`** — 1.42 %. Surprising for an animation that shouldn't touch the a11y tree. Worth investigating separately — accessibility rebuilds should fire only on layout / structural changes, not on each frame an animation tick runs.

## Top cost sources (widget_catalog `--tab animations`, 10 135 samples ≈ 50.7 % CPU)

This is the only scene profiled where the per-frame-effect path (`Pulse` + `Cycle`) is active. Sampled CPU is **roughly 10× the shader-only scenes** — the per-frame-effect path costs an order of magnitude more than `AnimatedQuadKind::*`. Top hotspots:

| % | Symbol | What it is |
| ---: | --- | --- |
| 9.58 % | `bastyde_core::arena::WidgetArena::active_ids` | Walks the arena, allocates a fresh `Vec<WidgetId>`. Called from layout, render, gesture tick, a11y rebuild — multiple times per frame. |
| 6.25 % | `WidgetTree::layout_impl::process_state_changes` | Processes pending signal-binding updates. Every Pulse/Cycle effect mutates a signal which lands here. |
| 4.94 % | `WidgetArena::visibility_checks` | The visibility predicate runner shared by all three motion subsystems. |
| 3.95 % | `_int_malloc` | Heap allocation. |
| 3.51 % | `WidgetArena::collect_needs_rebuild` | Same shape as `active_ids` — full arena walk + Vec allocation. |
| 3.33 % | `_int_free` | Heap free. |
| 3.32 % | **`build_accessibility_recursive`** | **A11y tree being rebuilt every animation tick — likely a bug.** A pure visual animation should not touch the a11y tree. |
| 2.80 % | `tick_gestures_with_ops` | Gesture state machines ticking on every frame even with no active gesture. |
| 2.41 % | `__memmove_avx512` | Vec / buffer copies. |
| 2.29 % | `malloc` | Heap allocation. |
| 1.91 % | `next_gesture_deadline` | Re-computing gesture timing every frame. |
| 1.68 % | `Hasher::write` | HashMap operations (one of several Hasher entries totalling ~3 % combined). |
| 1.52 % | `bastyde_render::renderer::Renderer::render` | The renderer is now a small fraction of cost. |
| 1.40 % | `<String as Clone>::clone` | String cloning (likely i18n LocalizedString resolves or tooltip text). |
| 0.95 % | `layout_widget_recursive` | Layout itself is now relatively minor. |
| 0.65 % | `<ColorTokens as Clone>::clone` | Theme tokens being cloned per query. |

**The optimisation landscape for the per-frame-effect path is dramatically different from the shader-driven path.** Most cost is in the widget-tree infrastructure (arena traversal, state changes, visibility, gesture, a11y), not in the renderer. Phase A (persistent uniform buffer) is irrelevant; Phase B (damage rects) addresses < 2 % of the cost here.

### Specific bugs / wins surfaced by this profile

1. **A11y rebuild every frame (~3.3 %)** — the a11y tree should only rebuild when its structure changes, not on every animation tick. Almost certainly a missing dirty-gate. Big single-fix opportunity.
2. **`active_ids` / `collect_needs_rebuild` allocate a fresh `Vec` per call (~13 % combined)** — these are arena walks that produce a snapshot for caller iteration. Reusing a per-tree `Vec<WidgetId>` cleared at the start of each call would cut both heap traffic and cache-miss rate. The arena could even hold a stable iterator, eliminating the Vec entirely.
3. **`process_state_changes` (~6.25 %)** — investigate why Pulse/Cycle's per-frame signal mutations cascade into so much work. Possibly the binding registry processes too many redundant bindings, or each mutation triggers a tree-wide walk.
4. **Gesture state machines tick every frame (~4.7 % combined)** — `tick_gestures_with_ops` + `next_gesture_deadline` should be gated by "is there at least one in-flight gesture?" If no gesture is captured / no long-press is pending, the gesture tick should be a no-op.
5. **`String::clone` and `ColorTokens::clone`** — likely indications that `LocalizedString::resolve_now` is called per-frame instead of cached in a Signal, and that ColorTokens is cloned by value somewhere instead of `&Theme`.

### Implication for the original plan

The plan's three optimisations (persistent uniform buffer, slot-delta, damage rects) **address less than 4 % of the per-frame-effect-path cost**. The biggest framework-level wins are in the widget-tree infrastructure that runs around the renderer, not in the renderer itself. The plan must pivot.

## Top-3 CPU cost sources (animations_kit, 1037 samples ≈ 5.2 % CPU — shader-driven baseline only)

The animations_kit scene is heavier (every animation wrapper visible) and the costs cluster differently:

1. **`bastyde_render::renderer::Renderer::render`** — 5.26 %. Dominant on this scene.
2. **wgpu pipeline state-change machinery** — combined ~3.2 %:
   - `wgpu_core::command::render::encode_render_pass` 1.54 %
   - `render_pass_set_pipeline` 1.04 %
   - `Binder::change_pipeline_layout` 0.62 %
3. **Heap traffic, lighter than animations** but still meaningful:
   - `__memmove_avx512` 2.82 %
   - `_int_malloc` 2.45 %
   - `Vec::clone` 1.39 % (RenderFrame::merge cloning draw-command vectors)
   - `_int_free` 0.94 %, `malloc_consolidate` 0.94 %, `malloc` 0.90 %.
   - `__GI___ioctl` 0.84 % (kernel boundary — wgpu submits, surface acquire).

`__powf_fma` 1.25 % is colour / gradient math; `tick_gestures_with_ops` 0.92 % is the gesture state machine; `WidgetArena::visibility_checks` 0.77 % is the per-frame visibility predicate. None of these are addressable by the planned phases.

## Memory-pressure metrics (perf stat, animations, 20 s)

| Counter | Value | Interpretation |
| --- | --- | --- |
| task-clock | 1.335 s / 20 s = 6.7 % CPU | matches bench 5.2 % within noise |
| IPC | 0.92 instructions / cycle | low — significant stall fraction, consistent with cache pressure |
| **cache-misses / cache-references** | **29.17 %** | very high; the workload is touching cold memory, almost certainly the freshly-malloced Vec backing storage |
| context-switches | 2 535 (127/sec) | normal |
| page-faults | 0 | steady-state, no kernel-side memory growth |
| CPU frequency | 1.73 GHz | laptop low-power scaling; single-core not boosting |

29 % cache miss rate on a single-threaded UI workload is the strongest evidence that **the bottleneck is heap-allocation traffic**, not GPU/queue work. Each per-frame Vec allocation produces cold cache lines that the next frame's processing has to load from main memory.

## GPU-side cost — what we DON'T know

`perf` is a CPU sampler. It cannot break down the 2.0 % GPU delta between fragment-shading, command submission, pipeline state changes, and surface acquire/present. To do that we'd need:

- `radv` / `amdvlk` driver tracing,
- `rocprofiler` for AMD GPU counters,
- or `RGP` / `RenderDoc` for graphics-pipeline timing.

Without that, **we cannot honestly claim that damage rects (Phase B) will save 1.5 of the 2.0 pt GPU**. The agent's projection assumed fragment-shading dominates; on this APU at this workload it's plausible the cost is split between submit, state-change, and rasterisation. From the CPU profile we already see the wgpu render-pass machinery (encode_render_pass + set_pipeline + change_pipeline_layout) is ~3 % of CPU on animations_kit — pipeline state-change cost is real and substantial on the CPU side.

Per the plan's decision matrix:

> | GPU profile shows command-submission / pipeline-state-change dominates | Phase A unchanged, but expect smaller wins. | **Re-think Phase B**: damage rects don't reduce submission cost; the real fix is reducing pipeline state changes (batch coalescing, fewer `set_pipeline` calls). Plan that instead. |

We don't have GPU-side data, but the CPU evidence already points partly that way (pipeline-state-change machinery is a top-3 cluster on animations_kit). Damage rects might still help if fragment cost is also there, but the projection becomes "GPU ~2.0 % → ~1.0–1.5 %", not "→ 0.2 %".

## Decision (per plan §0.3 — revised after the per-frame-effect profile)

| Plan element | Status post-Phase 0 |
| --- | --- |
| **Phase A — persistent `MAP_WRITE` uniform buffer** | **Drop.** `queue.write_buffer` is amortised away. Shipping it would change ~250 LoC for zero measurable win on any of the three profiled scenes. |
| **Phase A — `bytemuck::Pod` cleanup on `AnimParams`** | Ship. Removes a real `unsafe` block. Code-quality win, no perf claim. Tiny PR. |
| **Phase A — slot-delta on `AnimatedQuadRegistry`** | Ship as principled groundwork. **Does not move bench numbers**; future-proofs against scenes with paused-but-mounted animations. Tiny PR. |
| **Phase B — damage rects + scene texture** | **Drop, or at least defer indefinitely.** On the per-frame-effect scene the renderer is < 2 % of total CPU; Phase B's 800-LoC infrastructure investment buys ~0.5–1 pt at the absolute best. Wrong scope to chase. |

### New priority list (CPU-side, ranked by measured impact)

These are based on the widget_catalog `--tab animations` profile where Pulse/Cycle are active. The wins here also benefit the shader-driven scenes (since the framework code paths are shared) but the *order of magnitude* is set by this scene.

1. **A11y dirty gate — fix `build_accessibility_recursive` rebuild on animation ticks** (~3.3 % CPU recovered on the catalog scene, smaller share on simpler scenes). A pure visual signal mutation should not dirty the a11y tree. Likely a missing `BindingLevel` check or a too-broad dirty propagation.
2. **Reuse `arena.active_ids` / `collect_needs_rebuild` Vecs** (~13 % CPU combined). Both produce a per-call snapshot Vec that is iterated and discarded. Either pool the Vec on `WidgetTree` (clear + extend each call) or expose an `impl Iterator<Item = WidgetId>` that streams without allocating. The shader-driven scenes pay this cost too, just at smaller absolute weight.
3. **Gate `tick_gestures_with_ops` + `next_gesture_deadline`** (~4.7 % CPU on the catalog scene). Skip the gesture tick when there are no in-flight gestures (no captured pointer, no pending long-press deadline, no drag). Cheap to detect, large saving when nothing's being interacted with.
4. **`process_state_changes` reduction** (~6.25 % CPU on the catalog scene). Investigate what work it does per Pulse/Cycle frame-tick mutation. Likely cause: the binding registry visits more nodes per signal change than necessary, or each frame triggers multiple bind-flush cycles. Concrete diagnosis needed before designing the fix.
5. **`RenderFrame::merge` Vec cloning** (~1.4 % on animations_kit, smaller on others). RenderFrame merge clones draw-command Vecs; pooling would cut malloc cost and the 29 % cache-miss rate.
6. **`String::clone` / `ColorTokens::clone`** (smaller). Investigate per-frame `LocalizedString::resolve_now` calls and any pass-by-value `Theme` / `ColorTokens` through hot paths.

### What we are NOT doing (consciously)

- **Persistent uniform buffer.** The original first-priority item. Profile killed it.
- **Damage rects + scene texture.** The original second-priority item. Renderer cost is too small to justify the architectural lift.
- **Pipeline state-change reduction.** Plausible from the animations_kit profile but not the dominant cost on the per-frame-effect scene. Hold for later.

## Recommended next-step plan

1. **Land the bytemuck cleanup + slot-delta** as a small no-perf-claim PR ("hot-path cleanup, framework-level"). Maybe 100 LoC.
2. **Investigate the a11y-rebuild-every-frame bug.** Single-issue, single-fix. Likely high-impact-per-LoC.
3. **Refactor `arena.active_ids` to not allocate per call.** Either pool the Vec or expose an iterator. Touches every caller — manageable diff.
4. **Add a gesture-active gate so the gesture tick is a no-op on no-input frames.** Sub-day fix.
5. After (1)–(4) land, **re-profile**. Expect the catalog `--tab animations` scene to drop from ~50 % CPU to 20–25 % CPU. Re-evaluate `process_state_changes` from the new baseline.

Phase 0 paid off **substantially**. We went from "spend a multi-week plan implementing persistent uniform buffer + scene texture for ~3 pt of savings" to "investigate four concrete bugs / cheap wins for ~25 pt of savings on the worst-case scene."
