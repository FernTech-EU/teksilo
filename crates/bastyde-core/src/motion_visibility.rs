// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared visibility predicates used by every motion subsystem
//! (the looping-tween [`AnimationScheduler`](crate::animation::AnimationScheduler),
//! the shader-driven [`AnimatedQuadRegistry`](crate::animated_quad::AnimatedQuadRegistry),
//! and the per-frame-effect
//! [`FrameTickScheduler`](crate::frame_tick_scheduler::FrameTickScheduler))
//! to decide whether their owner widget is visible enough to keep
//! pumping frames for.
//!
//! Two flavours are exposed because the schedulers have different
//! constraints:
//!
//! - **Strict** ([`painted_this_frame`]) — `last_painted_epoch == paint_epoch`.
//!   Required by paths whose tick does NOT dirty the owner widget
//!   (shader uniforms, per-frame effects whose only output is mutating
//!   a signal that may or may not propagate to a paint dirty mark).
//!   Without strict equality, a never-painted widget
//!   (`last_painted_epoch == 0`, `paint_epoch == 1`) would be treated
//!   as visible forever and drive the event loop at full frame rate
//!   for off-screen owners.
//!
//! - **Tolerant** ([`painted_recently`]) — `last_painted_epoch + 1 >= paint_epoch`.
//!   Used by the signal-tween path, which by construction dirties its
//!   owner on every tick (the `Signal<f32>::set(v)` call propagates
//!   through bound props). The dirty mark causes a cache-miss paint,
//!   which bumps `paint_epoch` AND stamps `last_painted_epoch` to
//!   match — so the `+1` slack is self-correcting and avoids a
//!   false-negative on the first tick after a hidden→visible
//!   transition (where the layout pass and paint pass happen in
//!   adjacent frames).
//!
//! Both helpers treat `paint_epoch == 0` as "always visible". That
//! sentinel value means `render()` has never run, which is the common
//! case in unit tests that only call `tree.layout()`; refusing
//! visibility there would silently break test fixtures.

use crate::arena::WidgetArena;
use crate::widget_id::WidgetId;

/// Whether the widget is alive in the arena (present and not dormant).
/// Used by all motion schedulers to drop entries whose owner has been
/// removed or deactivated since registration.
pub fn alive(arena: &WidgetArena, id: WidgetId) -> bool {
    arena.get(id).is_some() && arena.is_active(id)
}

/// Strict visibility — true iff the widget was painted **in the most
/// recent paint pass** (or `paint_epoch == 0`). See module docs.
pub fn painted_this_frame(arena: &WidgetArena, id: WidgetId, paint_epoch: u64) -> bool {
    if paint_epoch == 0 {
        return true;
    }
    match arena.get(id) {
        Some(node) => arena.is_active(id) && node.last_painted_epoch == paint_epoch,
        None => false,
    }
}

/// Tolerant visibility — true iff the widget was painted **in the
/// most recent or previous paint pass** (or `paint_epoch == 0`). See
/// module docs.
pub fn painted_recently(arena: &WidgetArena, id: WidgetId, paint_epoch: u64) -> bool {
    if paint_epoch == 0 {
        return true;
    }
    match arena.get(id) {
        Some(node) => node.last_painted_epoch + 1 >= paint_epoch,
        None => false,
    }
}
