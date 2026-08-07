// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared scroll-chaining helpers for the scrollable widgets.
//!
//! All scrollable widgets ([`ScrollArea`](crate::ScrollArea),
//! [`ListView`](crate::ListView), [`TreeView`](crate::TreeView),
//! [`TableView`](crate::TableView)) share one boundary-chaining rule: a wheel
//! delta the widget cannot absorb on any axis (already clamped at the
//! boundary) is declined (`EventResponse::Ignored`) so the event bubbles to
//! the next ancestor scrollable — the web's `overscroll-behavior` model.

use teksilo_core::event::EventResponse;
// One shared boundary threshold across the widget scrollables and the
// teksilo-scene pan handler (defined in teksilo-core so both tiers use it).
use teksilo_core::overscroll::SCROLL_MOVE_EPSILON;

/// Clamp a single scroll axis. Returns `(new_pos, moved)` where `new_pos` is
/// `base + delta` clamped to `[0, max]` and `moved` is whether it changed by
/// more than [`SCROLL_MOVE_EPSILON`] (i.e. the axis could absorb part of the
/// delta).
pub(crate) fn scroll_clamp_axis(base: f32, delta: f32, max: f32) -> (f32, bool) {
    let new_pos = (base + delta).clamp(0.0, max);
    let moved = (new_pos - base).abs() > SCROLL_MOVE_EPSILON;
    (new_pos, moved)
}

/// Decide a scrollable's response to a wheel event from whether it absorbed
/// any movement. With [`OverscrollBehavior::Contain`] the event is always
/// absorbed (`Handled`); otherwise a fully-clamped scroll (`moved_any ==
/// false`) is declined (`Ignored`) so it chains to an ancestor scrollable.
pub(crate) fn scroll_response(moved_any: bool, contain: bool) -> EventResponse {
    if contain || moved_any {
        EventResponse::Handled
    } else {
        EventResponse::Ignored
    }
}

// `OverscrollBehavior` now lives in `teksilo-core` so `teksilo-scene` can name
// the same type without depending on `teksilo-widgets`. Re-exported here (and
// from the crate root) so the public `teksilo_widgets::OverscrollBehavior` path
// is unchanged.
pub use teksilo_core::OverscrollBehavior;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_axis_moves_within_range() {
        let (pos, moved) = scroll_clamp_axis(10.0, 5.0, 100.0);
        assert_eq!(pos, 15.0);
        assert!(moved);
    }

    #[test]
    fn clamp_axis_at_max_does_not_move() {
        let (pos, moved) = scroll_clamp_axis(100.0, 20.0, 100.0);
        assert_eq!(pos, 100.0);
        assert!(!moved);
    }

    #[test]
    fn clamp_axis_at_min_does_not_move() {
        let (pos, moved) = scroll_clamp_axis(0.0, -20.0, 100.0);
        assert_eq!(pos, 0.0);
        assert!(!moved);
    }

    #[test]
    fn clamp_axis_partial_consume_still_moves() {
        // base near max: can absorb part of the delta (5 of 20px).
        let (pos, moved) = scroll_clamp_axis(95.0, 20.0, 100.0);
        assert_eq!(pos, 100.0);
        assert!(moved);
    }

    #[test]
    fn response_chains_only_when_not_moved() {
        assert!(matches!(
            scroll_response(false, false),
            EventResponse::Ignored
        ));
        assert!(matches!(
            scroll_response(true, false),
            EventResponse::Handled
        ));
    }

    #[test]
    fn response_contain_always_handled() {
        assert!(matches!(
            scroll_response(false, true),
            EventResponse::Handled
        ));
        assert!(matches!(
            scroll_response(true, true),
            EventResponse::Handled
        ));
    }
}
