// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-item animation helpers for [`SceneItem`](crate::SceneItem)
//! authors.
//!
//! ## The pattern
//!
//! Lightweight scene items don't have a `WidgetId` of their own —
//! they're painted from inside the [`SceneView`](crate::SceneView)'s
//! paint walk and don't enter the arena. To get framework-managed
//! animations on an item-owned `Signal<f32>` (so the four idle
//! gates apply: reduced-motion snapping, window-inactive pause,
//! drop-cancel cleanup, paint-epoch visibility), the signal must
//! register against the *SceneView's* `WidgetId`.
//!
//! That's exactly what `SceneItem::register_bindings` is for: it
//! fires inside `SceneView::build()` and receives a `BuildContext`
//! whose `self_id()` is the SceneView's id. Item authors call
//! [`register_animated_item_signal`] on every animated signal they
//! own; from then on, calling `Signal::animate_to` on it is
//! framework-managed:
//!
//! ```ignore
//! struct PulsingDot {
//!     bounds: Rect,
//!     opacity: Signal<f32>,
//! }
//!
//! impl SceneItem for PulsingDot {
//!     fn register_bindings(&self, ctx: &mut BuildContext, _view_id: WidgetId) {
//!         // Hook the signal into the SceneView's animation scheduler.
//!         register_animated_item_signal(ctx, &self.opacity);
//!         // Also bind for repaint.
//!         self.opacity.bind_to(ctx.self_id(), ctx.binding_registry(),
//!             BindingLevel::RepaintOnly);
//!     }
//!     /* …bounds_in_scene / paint that reads self.opacity.get()… */
//! }
//! ```
//!
//! For one-shot tweens like a click-feedback flash, use the
//! [`pulse_once`] helper.
//!
//! ## Caveat
//!
//! Looping animations on lightweight items register against the
//! SceneView's id — they tick whenever the SceneView ticks, even
//! if the specific item is currently culled by the spatial index.
//! Apps that need ten-thousand pulsing background dots should
//! prefer one shared `Signal<f32>` driving a parametric paint
//! function instead of one signal per item.

use std::time::Duration;

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_tokens::Easing;

/// Register an item-owned `Signal<f32>` with the SceneView's
/// animation scheduler. Call this from inside
/// [`SceneItem::register_bindings`](crate::SceneItem::register_bindings)
/// for every animated signal the item exposes.
///
/// Equivalent to `ctx.register_animated_signal(signal)` — the
/// `register_bindings` callback is invoked while the framework's
/// `self_id()` is the SceneView's `WidgetId`, so the signal ends
/// up owned by the right widget for idle-gate tracking.
///
/// Provided as a named helper so item authors don't need to know
/// the underlying `BuildContext` API surface — call this and your
/// signal's `animate_to` calls are framework-managed.
pub fn register_animated_item_signal(ctx: &mut BuildContext, signal: &Signal<f32>) {
    ctx.register_animated_signal(signal);
}

/// One-shot ease-out tween from the signal's current value to
/// `target` over `duration`. The standard "fire on click,
/// dismiss" / "flash a highlight" pattern. The signal must
/// already be registered with [`register_animated_item_signal`]
/// (or directly via `ctx.register_animated_signal`) for the tween
/// to participate in idle gating.
///
/// Doesn't reach into reduced-motion settings — apps wiring a
/// flash that should suppress under reduced motion should query
/// `BuildContext::prefers_reduced_motion()` at build time and
/// skip the tween, or use the higher-level `ctx.animate()
/// .to_or_snap()` API at the widget tier.
pub fn pulse_once(signal: &Signal<f32>, target: f32, duration: Duration) {
    signal.animate_to(target, duration, Easing::EaseOut);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_once_kicks_off_animation() {
        let signal = Signal::new_animated(0.0);
        // Without a scheduler, animate_to still updates the
        // internal target — but the signal value won't tick.
        // We verify the call doesn't panic and the target lands.
        pulse_once(&signal, 1.0, Duration::from_millis(100));
        // The target should be queryable.
        assert!(signal.animation_target().is_some());
    }
}
