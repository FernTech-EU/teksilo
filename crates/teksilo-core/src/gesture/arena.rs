// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::time::Instant;

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent};

/// Arbitrates among multiple gesture recognizers competing on the same event
/// stream. All recognizers are fed each event in parallel. When one recognizes,
/// the others are reset. Failed recognizers are excluded from future events
/// until the next sequence (pointer up resets all).
pub struct GestureArena {
    entries: Vec<ArenaEntry>,
}

struct ArenaEntry {
    recognizer: Box<dyn GestureRecognizer>,
    failed: bool,
}

impl GestureArena {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a recognizer to the arena.
    pub fn add(&mut self, recognizer: impl GestureRecognizer + 'static) {
        self.entries.push(ArenaEntry {
            recognizer: Box::new(recognizer),
            failed: false,
        });
    }

    /// Feed a raw pointer event to all active recognizers.
    ///
    /// Returns the recognized gesture from the highest-priority recognizer,
    /// or `None` if no gesture was recognized yet.
    pub fn process(&mut self, event: &RawPointerEvent) -> Option<GestureEvent> {
        // On pointer down, give every recognizer a fresh chance — clear
        // the per-arena `failed` flag so a recognizer that failed on the
        // previous sequence can compete again. We deliberately do NOT call
        // `recognizer.reset()` here: the recognizers all overwrite their
        // per-sequence state on `RawPointerEvent::Down` themselves, and
        // calling `reset()` would wipe legitimate cross-sequence state
        // (notably `DoubleTapRecognizer::first_tap_time`, which must
        // survive the second `Down` for the double-tap to be recognized).
        if matches!(event, RawPointerEvent::Down { .. }) {
            for entry in &mut self.entries {
                entry.failed = false;
            }
        }

        let mut best: Option<(usize, u32, GestureEvent)> = None;

        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.failed {
                continue;
            }
            match entry.recognizer.process(event) {
                GestureResult::Recognized(gesture) => {
                    let prio = entry.recognizer.priority();
                    if best.as_ref().is_none_or(|(_, bp, _)| prio > *bp) {
                        best = Some((i, prio, gesture));
                    }
                }
                GestureResult::Failed => {
                    entry.failed = true;
                }
                GestureResult::Pending => {}
            }
        }

        if let Some((winner_idx, _, _)) = &best {
            // Winner recognized — reset all non-winning recognizers.
            // The winner keeps its state (important for multi-event gestures
            // like drag, which fire DragStart then DragUpdate then DragEnd).
            // Peers that explicitly opt out via `resets_on_peer_recognition`
            // (multi-tap family) keep their state so an escalating sequence
            // (tap → double tap → triple tap) can progress across winners.
            for (i, entry) in self.entries.iter_mut().enumerate() {
                if i == *winner_idx || entry.failed {
                    continue;
                }
                if !entry.recognizer.resets_on_peer_recognition() {
                    continue;
                }
                entry.recognizer.reset();
                entry.failed = false;
            }
        }

        best.map(|(_, _, gesture)| gesture)
    }

    /// Advance time-driven recognizers (notably `LongPressRecognizer`) and
    /// return the highest-priority gesture that just transitioned to
    /// `Recognized`, if any. Must be called by the event loop on each wake
    /// so long-press fires without requiring further pointer traffic.
    pub fn tick(&mut self, now: Instant) -> Option<GestureEvent> {
        let mut best: Option<(usize, u32, GestureEvent)> = None;

        for (i, entry) in self.entries.iter_mut().enumerate() {
            if entry.failed {
                continue;
            }
            match entry.recognizer.tick(now) {
                GestureResult::Recognized(gesture) => {
                    let prio = entry.recognizer.priority();
                    if best.as_ref().is_none_or(|(_, bp, _)| prio > *bp) {
                        best = Some((i, prio, gesture));
                    }
                }
                GestureResult::Failed => {
                    entry.failed = true;
                }
                GestureResult::Pending => {}
            }
        }

        if let Some((winner_idx, _, _)) = &best {
            for (i, entry) in self.entries.iter_mut().enumerate() {
                if i == *winner_idx || entry.failed {
                    continue;
                }
                if !entry.recognizer.resets_on_peer_recognition() {
                    continue;
                }
                entry.recognizer.reset();
                entry.failed = false;
            }
        }

        best.map(|(_, _, gesture)| gesture)
    }

    /// Earliest wall-clock instant at which any recognizer in this arena
    /// would like `tick()` to be called. Returns `None` if no recognizer
    /// has a pending time-driven transition.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.entries
            .iter()
            .filter(|entry| !entry.failed)
            .filter_map(|entry| entry.recognizer.next_deadline())
            .min()
    }

    /// Reset all recognizers in the arena.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            entry.recognizer.reset();
            entry.failed = false;
        }
    }

    /// Returns true if the arena has any recognizers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of recognizers in the arena.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for GestureArena {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureArena")
            .field("num_recognizers", &self.entries.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use teksilo_canvas::Point;

    use super::*;
    use crate::gesture::test_helpers::*;
    use crate::gesture::{DoubleTapRecognizer, DragRecognizer, TapRecognizer, TripleTapRecognizer};

    #[test]
    fn arena_tap_wins_over_nothing() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new());

        arena.process(&down(Point::new(10.0, 10.0)));
        let result = arena.process(&up(Point::new(10.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::Tap(_))));
    }

    #[test]
    fn arena_drag_wins_over_tap_on_movement() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new().max_distance(5.0));
        arena.add(DragRecognizer::new().threshold(5.0));

        arena.process(&down(Point::new(10.0, 10.0)));

        // Move beyond both thresholds — drag recognized (higher priority)
        let result = arena.process(&move_to(Point::new(30.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::DragStarted { .. })));
    }

    #[test]
    fn arena_tap_recognized_when_drag_fails() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new());
        arena.add(DragRecognizer::new().threshold(5.0));

        arena.process(&down(Point::new(10.0, 10.0)));

        // Up without significant movement — drag fails, tap wins
        let result = arena.process(&up(Point::new(10.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::Tap(_))));
    }

    #[test]
    fn arena_double_and_triple_tap_cooperate() {
        // Regression for the cooperative-recognizer contract: both
        // `DoubleTapRecognizer` and `TripleTapRecognizer` must observe
        // the full click sequence. Without `resets_on_peer_recognition =
        // false` on both, DoubleTap's win at click 2 would reset the
        // TripleTapRecognizer and click 3 would never fire TripleTap.
        let mut arena = GestureArena::new();
        arena.add(DoubleTapRecognizer::new());
        arena.add(TripleTapRecognizer::new());

        let pos = Point::new(10.0, 10.0);

        // Click 1 — both recognizers pending.
        assert!(arena.process(&down(pos)).is_none());
        assert!(arena.process(&up(pos)).is_none());

        // Click 2 — DoubleTap fires.
        assert!(arena.process(&down(pos)).is_none());
        let second = arena.process(&up(pos));
        assert!(
            matches!(second, Some(GestureEvent::DoubleTap(_))),
            "click 2 must produce DoubleTap, got {:?}",
            second
        );

        // Click 3 — TripleTap fires. If the arena reset TripleTapRecognizer
        // after DoubleTap won, this would be None.
        assert!(arena.process(&down(pos)).is_none());
        let third = arena.process(&up(pos));
        assert!(
            matches!(third, Some(GestureEvent::TripleTap(_))),
            "click 3 must produce TripleTap, got {:?}",
            third
        );
    }

    #[test]
    fn arena_resets_on_new_down() {
        let mut arena = GestureArena::new();
        arena.add(TapRecognizer::new().max_distance(5.0));

        // First sequence: fail the tap by moving
        arena.process(&down(Point::new(10.0, 10.0)));
        arena.process(&move_to(Point::new(50.0, 50.0)));

        // New sequence: should work fresh
        arena.process(&down(Point::new(10.0, 10.0)));
        let result = arena.process(&up(Point::new(10.0, 10.0)));
        assert!(matches!(result, Some(GestureEvent::Tap(_))));
    }

    #[test]
    fn arena_empty_returns_none() {
        let mut arena = GestureArena::new();
        assert!(arena.is_empty());
        let result = arena.process(&down(Point::new(10.0, 10.0)));
        assert!(result.is_none());
    }
}
