//! Shader-driven animated quads.
//!
//! An **alternative to `Signal<f32>::animate_looping`** for decorative
//! motion that fits a shader model — indeterminate progress sweeps,
//! pulses, shimmer, sprite-atlas frame cycling. Instead of dirtying the
//! widget every tick and re-running `paint()`, the widget emits one
//! [`crate::arena`]-cached draw command, and a renderer-side uniform
//! buffer carries the live phase per slot. Each frame the widget
//! tree's [`AnimatedQuadRegistry::tick`] computes new
//! [`AnimParams`] and uploads them; the fragment shader reads the
//! slot's params to decide the pixel output. `paint()` only re-runs
//! when layout changes (resize, reflow, theme change).
//!
//! The existing [`crate::animation::AnimationScheduler`] path stays
//! for anything that isn't a quad — scroll offsets, sidebar slides,
//! toggle knob transitions, custom interactive tweens.
//!
//! ## Opt-in
//!
//! ```ignore
//! fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
//!     self.handle = Some(ctx.animated_quad(AnimatedQuadKind::IndeterminateSweep {
//!         period: Duration::from_millis(900),
//!         sweep_ratio: 0.42,
//!         track_color: SurfaceRole::Sunken.into(),
//!         fill_color:  SurfaceRole::Accent.into(),
//!     }));
//!     vec![]
//! }
//! fn paint(&self, bounds: Rect, canvas: &mut Canvas, _: &PaintContext) {
//!     if let Some(h) = self.handle {
//!         canvas.draw_animated_quad(bounds, h.slot(), AnimatedQuadClass::Procedural);
//!     }
//! }
//! ```
//!
//! ## Four-gate model
//!
//! Matches [`crate::animation::AnimationScheduler`] exactly — a
//! slot is skipped on the per-frame tick when any of:
//! - the window is unfocused / occluded (`window_active == false`),
//! - the owning widget is dormant or destroyed,
//! - the owning widget was not painted in the most-recent paint pass
//!   (paint-epoch gate — same semantics as the signal scheduler),
//! - (build-time) the user has `prefers-reduced-motion` enabled, in
//!   which case the widget doesn't register the quad at all.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use fern_canvas::AnimParams;
use fern_tokens::{Color, Easing, Theme};

use crate::arena::WidgetArena;
use crate::color_prop::ColorProp;
use crate::widget_id::WidgetId;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Opaque handle returned by `BuildContext::animated_quad`. Stored on
/// the widget and threaded to `Canvas::draw_animated_quad` at paint
/// time to identify which uniform-buffer slot drives the quad.
///
/// Valid for exactly one widget-mount lifetime. On rebuild or destroy
/// the registry frees the slot; a fresh `build()` allocates a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimatedQuadHandle {
    slot: u32,
}

impl AnimatedQuadHandle {
    /// The dense slot index. Widgets pass this to
    /// [`fern_canvas::Canvas::draw_animated_quad`] at paint time.
    pub fn slot(self) -> u32 {
        self.slot
    }
}

/// What the shader draws into the quad's bounds. Every variant maps
/// to a `kind: u32` constant read by the renderer's fragment shader
/// (see `anim_procedural.wgsl` / `anim_sprite.wgsl` in `fern-render`).
#[derive(Debug, Clone)]
pub enum AnimatedQuadKind {
    /// Indeterminate progress sweep — a moving band of `fill_color`
    /// over a `track_color` background, `sweep_ratio` wide, looping
    /// left→right with period `period`. Matches
    /// [`fern_widgets::ProgressBar::indeterminate`](https://docs.rs/fern-widgets/latest/fern_widgets/struct.ProgressBar.html).
    IndeterminateSweep {
        period: Duration,
        /// Width of the moving band as a fraction of the quad width
        /// (0..1). Typical value ≈ 0.42.
        sweep_ratio: f32,
        track_color: ColorProp,
        fill_color: ColorProp,
    },
    /// Sprite-atlas frame cycling. The atlas is an image registered
    /// via the normal `Canvas` image path (keyed by `image_name`);
    /// the shader samples cell `(frame_index % cols, frame_index / cols)`.
    SpriteCycle {
        image_name: String,
        frame_count: u32,
        cols: u32,
        rows: u32,
        period: Duration,
        tint: Option<ColorProp>,
    },
    // Future kinds: Pulse, Shimmer, Skeleton, Spinner (circular rotation).
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

struct AnimatedQuadEntry {
    owner: WidgetId,
    kind: AnimatedQuadKind,
    /// Wall-clock when the animation entered the registry — drives phase.
    started_at: Instant,
    /// Phase-continuous pause support, mirroring `AnimationScheduler`.
    /// Set when the window goes inactive; on resume, the elapsed
    /// paused duration is added to `started_at` so `t` doesn't jump.
    paused_at: Option<Instant>,
}

/// Per-tree registry of shader-driven animated quads.
///
/// Owns dense `u32` slots into the renderer's per-frame uniform
/// buffer. Each widget opting into the shader path gets a slot at
/// `build()` time and gives it back on rebuild/destroy.
pub struct AnimatedQuadRegistry {
    entries: HashMap<u32, AnimatedQuadEntry>,
    owners: HashMap<WidgetId, Vec<u32>>,
    free_slots: Vec<u32>,
    next_slot: u32,
    /// Cached `AnimParams` buffer, sized to `next_slot`. Recomputed
    /// in-place each tick; freed slots keep their last-written values
    /// until reallocated (cheap and avoids branches in `tick`).
    scratch: Vec<AnimParams>,
    /// Whether the owning window is active (focused && !occluded).
    /// Inactive pauses the whole tick — no params are rewritten, so
    /// the GPU keeps drawing the last frame's phase.
    window_active: bool,
    /// Wall-clock when the window last went inactive — used on
    /// inactive→active transition to rebase every entry's
    /// `started_at` by the paused duration (phase continuity).
    paused_at: Option<Instant>,
    /// Wall-clock of the last successful `tick` (when `window_active`
    /// was true). `None` before the first tick. Drives
    /// [`Self::next_deadline`] so the event loop wakes at the
    /// animation frame interval even when no widget is dirty —
    /// without this, the `ControlFlow::WaitUntil` path never fires
    /// and the animation only advances on unrelated events.
    last_tick_at: Option<Instant>,
    /// Frame interval for shader-driven animations. 33 ms (~30 Hz)
    /// by default — same cadence as the signal-based animation
    /// scheduler's `DEFAULT_FRAME_INTERVAL`. Per-frame cost on the
    /// shader path is just `queue.write_buffer(64 B) + draw_indexed`,
    /// so driving at 30 Hz is cheap even with many animated quads.
    frame_interval: Duration,
}

const DEFAULT_SHADER_FRAME_INTERVAL: Duration = Duration::from_millis(33);

impl AnimatedQuadRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            owners: HashMap::new(),
            free_slots: Vec::new(),
            next_slot: 0,
            scratch: Vec::new(),
            window_active: true,
            paused_at: None,
            last_tick_at: None,
            frame_interval: DEFAULT_SHADER_FRAME_INTERVAL,
        }
    }

    /// Register a new animated quad for `owner` with `kind`. Allocates
    /// a slot (reuses a freed one when available), records the entry,
    /// and returns an opaque handle the widget stashes for its
    /// `paint()` call.
    pub fn register(
        &mut self,
        owner: WidgetId,
        kind: AnimatedQuadKind,
        now: Instant,
    ) -> AnimatedQuadHandle {
        let slot = self.free_slots.pop().unwrap_or_else(|| {
            let s = self.next_slot;
            self.next_slot = self.next_slot.saturating_add(1);
            s
        });
        self.entries.insert(
            slot,
            AnimatedQuadEntry {
                owner,
                kind,
                started_at: now,
                paused_at: None,
            },
        );
        self.owners.entry(owner).or_default().push(slot);
        if (slot as usize) >= self.scratch.len() {
            self.scratch.resize((slot as usize) + 1, AnimParams::default());
        }
        AnimatedQuadHandle { slot }
    }

    /// Free every slot owned by `widget_id`. Called from
    /// `rebuild_single_widget` (before `build()`) and
    /// `destroy_subtree`, mirroring
    /// [`crate::animation::AnimationScheduler::cancel_by_widget`].
    pub fn cancel_by_widget(&mut self, widget_id: WidgetId) {
        if let Some(slots) = self.owners.remove(&widget_id) {
            for slot in slots {
                self.entries.remove(&slot);
                self.free_slots.push(slot);
            }
        }
    }

    /// Mark the owning window active/inactive. Inactive: `tick` is a
    /// no-op. On inactive→active transition, each entry's
    /// `started_at` is rebased by the paused duration so phase is
    /// continuous (no snap).
    pub fn set_window_active(&mut self, active: bool, now: Instant) {
        if self.window_active == active {
            return;
        }
        if active {
            if let Some(paused_at) = self.paused_at.take() {
                let offset = now.saturating_duration_since(paused_at);
                for entry in self.entries.values_mut() {
                    entry.started_at += offset;
                }
            }
        } else {
            self.paused_at = Some(now);
        }
        self.window_active = active;
    }

    pub fn is_window_active(&self) -> bool {
        self.window_active
    }

    /// Total registered slots (alive + freed-but-reservable). Test /
    /// debug API.
    pub fn active_count(&self) -> usize {
        self.entries.len()
    }

    /// Capacity of the params buffer. Indicates the high-water mark —
    /// the next upload will always be this many params.
    pub fn params_capacity(&self) -> usize {
        self.scratch.len()
    }

    /// Borrow the params buffer populated by the most recent
    /// [`Self::tick`]. Used by `WidgetTree::render` to copy fresh
    /// values into the outgoing `RenderFrame` without allocating an
    /// intermediate `Vec`. Slots that were skipped on the last tick
    /// (offscreen / paused) keep their previous values — the fragment
    /// shader still draws them, just with a stale phase that the next
    /// tick will overwrite.
    pub fn scratch_slice(&self) -> &[AnimParams] {
        &self.scratch
    }

    /// Compute fresh [`AnimParams`] for every live slot and return a
    /// `&[AnimParams]` indexed by slot. Slots whose widget is dormant,
    /// offscreen, or whose window is inactive keep their previous
    /// values (so a one-frame stale phase on resume is acceptable; the
    /// next tick writes fresh values).
    ///
    /// `paint_epoch == 0` means `render()` has never run — common in
    /// headless tests — treated as "always visible" to avoid
    /// regressing unit tests.
    pub fn tick(
        &mut self,
        now: Instant,
        arena: &WidgetArena,
        paint_epoch: u64,
        theme: &Theme,
    ) -> &[AnimParams] {
        if !self.window_active {
            return &self.scratch;
        }
        // Keep scratch big enough for the largest slot we've ever
        // allocated, even if some slots are freed — the `slot` stored
        // in a DrawCommand::AnimatedQuad may index anywhere up to
        // `next_slot` and we don't want to panic on a stale command.
        if self.scratch.len() < self.next_slot as usize {
            self.scratch
                .resize(self.next_slot as usize, AnimParams::default());
        }

        for (&slot, entry) in self.entries.iter() {
            if !widget_visible(arena, entry.owner, paint_epoch) {
                continue;
            }
            let params = compute_params(entry, now, theme);
            self.scratch[slot as usize] = params;
        }
        self.last_tick_at = Some(now);
        &self.scratch
    }

    /// Whether any animation is *eligible to advance* on the next tick.
    /// Mirrors `AnimationScheduler::has_running`: pause-aware so the
    /// idle-work predicate stays off while the window is inactive.
    pub fn has_running(&self) -> bool {
        self.window_active && !self.entries.is_empty()
    }

    /// Earliest deadline at which a visible, not-paused animation
    /// wants the event loop to wake and call `render()`. Returns
    /// `None` when the scheduler is window-paused, has no entries, or
    /// all entries are hidden.
    ///
    /// Without this contribution to the tree's `next_timer_deadline`,
    /// the event loop would sleep on `ControlFlow::Wait` between
    /// unrelated events and the animation would only advance when the
    /// user moves the mouse or scrolls — exactly the staircase
    /// behaviour a missing deadline produces.
    pub fn next_deadline(&self, arena: &WidgetArena, paint_epoch: u64) -> Option<Instant> {
        if !self.window_active {
            return None;
        }
        let any_visible = self
            .entries
            .values()
            .any(|entry| widget_visible(arena, entry.owner, paint_epoch));
        if !any_visible {
            return None;
        }
        // First tick ever: wake immediately. Otherwise: last tick +
        // frame interval. The event loop clamps against `Instant::now()`
        // when setting `ControlFlow::WaitUntil`, so a past deadline
        // just means "wake on the next poll" — which is fine.
        Some(match self.last_tick_at {
            Some(t) => t + self.frame_interval,
            None => Instant::now(),
        })
    }
}

impl Default for AnimatedQuadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AnimatedQuadRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedQuadRegistry")
            .field("active_count", &self.entries.len())
            .field("capacity", &self.scratch.len())
            .field("window_active", &self.window_active)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn widget_visible(arena: &WidgetArena, id: WidgetId, paint_epoch: u64) -> bool {
    if paint_epoch == 0 {
        return true;
    }
    // Strict equality (NOT `last_painted_epoch + 1 >= paint_epoch` like
    // the signal scheduler). The signal path dirties its widget on
    // every tick, which causes a non-cache-hit paint and bumps
    // paint_epoch — so its `+1` tolerance is self-correcting. The
    // shader path, by design, never dirties widgets (the fragment
    // shader reads per-frame state from a uniform); paint_epoch stays
    // frozen on cache-hit frames, so a `+1` tolerance would treat a
    // widget that has NEVER been painted (last_painted_epoch = 0,
    // paint_epoch = 1) as visible forever, driving the event loop at
    // the animation frame rate for quads that are actually off-screen.
    //
    // With `==`, freshly-visible widgets miss exactly one tick (the
    // one before their first post-scroll paint stamps them); that's
    // ~33 ms, imperceptible.
    match arena.get(id) {
        Some(node) => arena.is_active(id) && node.last_painted_epoch == paint_epoch,
        None => false,
    }
}

fn compute_params(entry: &AnimatedQuadEntry, now: Instant, theme: &Theme) -> AnimParams {
    match &entry.kind {
        AnimatedQuadKind::IndeterminateSweep {
            period,
            sweep_ratio,
            track_color,
            fill_color,
        } => {
            let phase = looping_phase(entry.started_at, now, *period, Easing::Linear);
            AnimParams {
                kind: 0,
                phase,
                sweep_ratio: *sweep_ratio,
                color0: color_to_rgba(&track_color.resolve(theme)),
                color1: color_to_rgba(&fill_color.resolve(theme)),
                ..AnimParams::default()
            }
        }
        AnimatedQuadKind::SpriteCycle {
            period,
            frame_count,
            cols,
            rows,
            tint,
            ..
        } => {
            let t = looping_phase(entry.started_at, now, *period, Easing::Linear);
            let frame_index = (t * *frame_count as f32).floor().min((*frame_count - 1) as f32);
            let tint_rgba = tint
                .as_ref()
                .map(|c| color_to_rgba(&c.resolve(theme)))
                .unwrap_or([0.0; 4]);
            AnimParams {
                kind: 1,
                phase: frame_index,
                color1: tint_rgba,
                atlas_cols: *cols as f32,
                atlas_rows: *rows as f32,
                ..AnimParams::default()
            }
        }
    }
}

fn looping_phase(started_at: Instant, now: Instant, period: Duration, easing: Easing) -> f32 {
    if period.is_zero() {
        return 0.0;
    }
    let elapsed = now.saturating_duration_since(started_at).as_secs_f32();
    let period_s = period.as_secs_f32();
    let t = (elapsed % period_s) / period_s;
    easing.apply(t)
}

fn color_to_rgba(c: &Color) -> [f32; 4] {
    [c.r(), c.g(), c.b(), c.a()]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::WidgetArena;
    use crate::test_widgets::FillWidget;
    use fern_tokens::SurfaceRole;

    fn arena_with(n: usize) -> (WidgetArena, Vec<WidgetId>) {
        let mut arena = WidgetArena::new();
        let ids = (0..n)
            .map(|_| arena.insert(Box::new(FillWidget::new())))
            .collect();
        (arena, ids)
    }

    fn sweep_kind() -> AnimatedQuadKind {
        AnimatedQuadKind::IndeterminateSweep {
            period: Duration::from_millis(100),
            sweep_ratio: 0.42,
            track_color: SurfaceRole::Sunken.into(),
            fill_color: SurfaceRole::Accent.into(),
        }
    }

    #[test]
    fn register_allocates_unique_slots() {
        let mut reg = AnimatedQuadRegistry::new();
        let (_arena, ids) = arena_with(3);
        let now = Instant::now();

        let h0 = reg.register(ids[0], sweep_kind(), now);
        let h1 = reg.register(ids[1], sweep_kind(), now);
        let h2 = reg.register(ids[2], sweep_kind(), now);

        assert_ne!(h0.slot(), h1.slot());
        assert_ne!(h1.slot(), h2.slot());
        assert_eq!(reg.active_count(), 3);
    }

    #[test]
    fn cancel_by_widget_frees_slots() {
        let mut reg = AnimatedQuadRegistry::new();
        let (_arena, ids) = arena_with(2);
        let now = Instant::now();

        let h0 = reg.register(ids[0], sweep_kind(), now);
        let h1 = reg.register(ids[1], sweep_kind(), now);
        assert_eq!(reg.active_count(), 2);

        reg.cancel_by_widget(ids[0]);
        assert_eq!(reg.active_count(), 1);

        // Freed slot is reusable for a subsequent register.
        let h2 = reg.register(ids[1], sweep_kind(), now);
        assert_eq!(h2.slot(), h0.slot(), "freed slot should be reused");
        assert_ne!(h2.slot(), h1.slot());
    }

    #[test]
    fn tick_writes_phase_for_visible_widget() {
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let start = Instant::now();
        let theme = Theme::light_default();

        let h = reg.register(ids[0], sweep_kind(), start);

        // paint_epoch == 0 → bypass visibility gate.
        let params = reg.tick(start + Duration::from_millis(50), &arena, 0, &theme);
        let p = params[h.slot() as usize];
        assert_eq!(p.kind, 0);
        assert!(
            (p.phase - 0.5).abs() < 0.01,
            "phase at 50% should be ~0.5, got {}",
            p.phase
        );
        assert!((p.sweep_ratio - 0.42).abs() < 1e-6);
        // Fill color has non-zero alpha (SurfaceRole::Accent is opaque).
        assert!(p.color1[3] > 0.0);
    }

    #[test]
    fn tick_skips_offscreen_widgets() {
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let start = Instant::now();
        let theme = Theme::light_default();

        let h = reg.register(ids[0], sweep_kind(), start);
        // paint_epoch=5 but the widget's last_painted_epoch stays at 0 —
        // gate closes. Leave the scratch value default (zeroed),
        // verify it's unchanged.
        let params = reg.tick(start + Duration::from_millis(50), &arena, 5, &theme);
        let p = params[h.slot() as usize];
        assert_eq!(p.phase, 0.0, "offscreen widget must not have its phase updated");
    }

    #[test]
    fn window_inactive_is_noop() {
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let start = Instant::now();
        let theme = Theme::light_default();

        let h = reg.register(ids[0], sweep_kind(), start);
        reg.set_window_active(false, start);

        let params = reg.tick(start + Duration::from_millis(50), &arena, 0, &theme);
        let p = params[h.slot() as usize];
        assert_eq!(p.phase, 0.0, "paused tick must not advance phase");
    }

    #[test]
    fn resume_rebases_phase_continuously() {
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let start = Instant::now();
        let theme = Theme::light_default();

        let h = reg.register(ids[0], sweep_kind(), start);

        // Run one tick at t=25ms → phase ≈ 0.25.
        let params = reg.tick(start + Duration::from_millis(25), &arena, 0, &theme);
        assert!((params[h.slot() as usize].phase - 0.25).abs() < 0.02);

        // Window hides at 25ms, returns 10s later.
        reg.set_window_active(false, start + Duration::from_millis(25));
        let resume_at = start + Duration::from_millis(25) + Duration::from_secs(10);
        reg.set_window_active(true, resume_at);

        // 25ms after resume: phase should be ~0.5, NOT jumped by 10s of
        // period cycles (100ms period × 100 cycles = 10s).
        let params = reg.tick(resume_at + Duration::from_millis(25), &arena, 0, &theme);
        let p = params[h.slot() as usize].phase;
        assert!(
            (p - 0.5).abs() < 0.02,
            "phase-continuous resume expected ~0.5, got {p}"
        );
    }

    #[test]
    fn next_deadline_advances_on_each_tick() {
        // Regression test for a real bug: without this deadline
        // contribution the event loop parks on ControlFlow::Wait
        // between frame intervals and the animation only advances on
        // unrelated wakes (mouse move, scroll) — visible as a 1-2
        // step staircase.
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let theme = Theme::light_default();
        let start = Instant::now();

        reg.register(ids[0], sweep_kind(), start);

        // Before any tick: deadline is "wake immediately" — ensures
        // the first animation frame runs on the next event-loop turn.
        let d0 = reg.next_deadline(&arena, 0).expect("must be scheduled");
        assert!(
            d0 <= Instant::now() + Duration::from_millis(1),
            "first deadline should be ~now, got {:?} from now",
            d0.saturating_duration_since(Instant::now())
        );

        // After a tick at time t, deadline moves to t + frame_interval.
        reg.tick(start, &arena, 0, &theme);
        let d1 = reg.next_deadline(&arena, 0).expect("must stay scheduled");
        assert_eq!(
            d1,
            start + Duration::from_millis(33),
            "deadline must advance by frame_interval after each tick"
        );

        // Another tick 33ms later → deadline pushes another 33ms.
        reg.tick(start + Duration::from_millis(33), &arena, 0, &theme);
        let d2 = reg.next_deadline(&arena, 0).expect("still scheduled");
        assert_eq!(d2, start + Duration::from_millis(66));
    }

    #[test]
    fn next_deadline_none_when_window_inactive() {
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let start = Instant::now();
        reg.register(ids[0], sweep_kind(), start);
        reg.set_window_active(false, start);
        assert!(
            reg.next_deadline(&arena, 0).is_none(),
            "paused registry must not contribute to next_timer_deadline"
        );
    }

    #[test]
    fn next_deadline_none_when_all_entries_offscreen() {
        let mut reg = AnimatedQuadRegistry::new();
        let (arena, ids) = arena_with(1);
        let start = Instant::now();
        reg.register(ids[0], sweep_kind(), start);
        // paint_epoch > 0 but widget's last_painted_epoch stays 0 →
        // gate closes. Deadline should drop so the loop can park.
        assert!(
            reg.next_deadline(&arena, 5).is_none(),
            "offscreen-only registry must not keep the event loop awake"
        );
    }

    #[test]
    fn rebuild_pattern_frees_and_reallocates() {
        // Simulates the rebuild_single_widget pattern: cancel old,
        // register new, scratch capacity stays stable.
        let mut reg = AnimatedQuadRegistry::new();
        let (_arena, ids) = arena_with(1);
        let now = Instant::now();

        let h0 = reg.register(ids[0], sweep_kind(), now);
        let cap_before = reg.params_capacity();

        reg.cancel_by_widget(ids[0]);
        let h1 = reg.register(ids[0], sweep_kind(), now);

        assert_eq!(h0.slot(), h1.slot(), "slot should be reused from free list");
        assert_eq!(reg.params_capacity(), cap_before);
        assert_eq!(reg.active_count(), 1);
    }
}
