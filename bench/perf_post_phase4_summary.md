# Phase 1–4 perf verification

Re-bench of the framework-level optimisation plan
([plan-for-persistent-mapped-buzzing-shell.md](../../.claude/plans/plan-for-persistent-mapped-buzzing-shell.md)).
Compared against the Phase 0 baseline in
[perf_60hz_summary.md](perf_60hz_summary.md).

## Bench script (shader-driven scenes)

`tools/bench_examples.py --only animations animations-kit --duration 30 --warmup 5`

| Scene | Phase 0 CPU | Post-Phase 4 CPU | Δ |
| --- | --- | --- | --- |
| `animations` | 5.2 % | 5.6 % | +0.4 pt (within noise) |
| `animations-kit` | 5.1 % | 4.7 % | -0.4 pt (within noise) |

Shader-driven scenes don't exercise the per-frame-effect path
(`Pulse` / `Cycle`) — the wins from Phases 1-4 don't land here. This
matches the plan's prediction.

## Manual perf record (catalog scene)

```
./target/profiling/widget-catalog --tab animations &
perf record -F 999 -g --call-graph fp -p $PID \
    -o bench/perf_widget_catalog_animations_tab_post.data -- sleep 20
```

Sample count: 5 633 in 20 s at 999 Hz. Event count: 20.6 G cycles.
At ~3.8 GHz on this APU, that's **~28 % of one core** during the
recording — down from **~50.7 %** at Phase 0 baseline.

### Hotspot deltas

Phase-0 hotspots, re-measured after Phases 1-4:

| Symbol | Phase 0 (% of total CPU) | Post-Phase 4 (% of recording CPU) | Status |
| --- | --- | --- | --- |
| `WidgetArena::active_ids` | 9.58 % | not in top 30 | ✅ eliminated (Phase 2 streaming iter) |
| `process_state_changes` (self) | 6.25 % | parent role only | ✅ flat cost replaced by O(S) source walk (Phase 4) |
| `BindingRegistry::flush_dirty` | 6.25 % | gone | ✅ replaced by `flush_all_dirty` over source groups |
| `collect_needs_rebuild` | 3.51 % | 5.37 % (smaller pie) | partial — absolute cost similar; see notes |
| `build_accessibility_recursive` | 3.32 % | 0.03 % | ✅ Phase 1 (no longer rebuilt every frame) |
| `tick_gestures_with_ops` | 0.02 % (already low) | 0.02 % | ✅ Phase 3 keeps it low |
| `Hasher::write` cluster | aggregated 4 %+ | top 3 each ~0.7-2.5 % | ✅ Phase 4 compressed |

### About `collect_needs_rebuild` at 5.37 %

The percentage went up but the absolute cost dropped:

- Phase 0: 50 % CPU × 3.51 % = ~1.76 % of system-wide CPU
- Post: 28 % CPU × 5.37 % = ~1.50 % of system-wide CPU

It's now the highest-percentage *remaining* hotspot inside
`process_state_changes`. The call site at `layout_impl.rs:106` needs an
owned `Vec<WidgetId>` because the loop subsequently mutates the arena
(rebuild walk). Phase 2 added a streaming `needs_rebuild_iter` for
read-only paths but kept the allocating wrapper for this site. A
future optimisation could pool a `Vec` here similar to
`active_ids_scratch`, but the absolute saving is only ~0.5 pt.

## Catalog-scene CPU vs target

| | CPU | vs Phase-0 baseline | vs target |
| --- | --- | --- | --- |
| Phase 0 baseline | 50.7 % | — | — |
| **Post-Phase 4** | **~28 %** | **−22.7 pt** | target was ≤ 25 % (3 pt over) |

The plan's worst-case projection (≤ 25 %) was tight; the actual landing
of ~28 % captures most of the win. The remaining gap is explained by
`collect_needs_rebuild` and the binding-registry residual cost
(`Signal::as_sources` closures + the source-group walk itself), which
were not addressed in the original 5-phase scope.

## Behavioural regressions

`cargo test --workspace` is green except for the two pre-existing
`checkbox::tristate_*` failures (unrelated to this work, predate Phase
0). No new regressions introduced.

Phase 1 surfaced and fixed two latent bugs that had been masked by the
unconditional `a11y_dirty = true`:

1. The binding registry's separate `flush_dirty` and
   `flush_accessibility_dirty` calls raced on a shared per-Signal dirty
   flag — fixed by unifying into `flush_all_dirty`.
2. `WidgetTree::show_overlay` paths weren't dirtying the AT cache,
   which was hidden because layout always dirtied it — fixed by
   adding explicit `a11y_dirty = true` at every overlay-show site
   (programmatic, dispatch-queued, delayed).

Both have regression tests added.

## Files

- Plan: [.claude/plans/plan-for-persistent-mapped-buzzing-shell.md](../.claude/plans/plan-for-persistent-mapped-buzzing-shell.md)
- Phase 0 baseline: [perf_60hz_summary.md](perf_60hz_summary.md)
- Pre-Phase-4 catalog profile: [perf_widget_catalog_animations_tab.data](perf_widget_catalog_animations_tab.data)
- Post-Phase-4 catalog profile: [perf_widget_catalog_animations_tab_post.data](perf_widget_catalog_animations_tab_post.data)
- Post-Phase-4 bench report: [post_phase4_animations.md](post_phase4_animations.md)
