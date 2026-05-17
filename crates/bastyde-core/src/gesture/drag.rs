use bastyde_canvas::{Point, Vec2};

use crate::event::PointerButton;

use super::{GestureEvent, GestureRecognizer, GestureResult, RawPointerEvent};

/// Recognizes a drag gesture (pointer down + move beyond threshold).
#[derive(Debug)]
pub struct DragRecognizer {
    threshold: f32,
    down_position: Option<Point>,
    down_button: Option<PointerButton>,
    dragging: bool,
    last_position: Option<Point>,
}

impl DragRecognizer {
    pub fn new() -> Self {
        Self {
            threshold: 5.0,
            down_position: None,
            down_button: None,
            dragging: false,
            last_position: None,
        }
    }

    pub fn threshold(mut self, t: f32) -> Self {
        self.threshold = t;
        self
    }
}

impl Default for DragRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GestureRecognizer for DragRecognizer {
    fn process(&mut self, event: &RawPointerEvent) -> GestureResult {
        match event {
            RawPointerEvent::Down {
                position, button, ..
            } => {
                self.down_position = Some(*position);
                self.down_button = Some(*button);
                self.last_position = Some(*position);
                self.dragging = false;
                GestureResult::Pending
            }
            RawPointerEvent::Move { position } => {
                let Some(down) = self.down_position else {
                    return GestureResult::Pending;
                };

                if self.dragging {
                    let last = self.last_position.unwrap_or(down);
                    let delta = Vec2::new(position.x - last.x, position.y - last.y);
                    self.last_position = Some(*position);
                    return GestureResult::Recognized(GestureEvent::DragMoved {
                        position: *position,
                        delta,
                    });
                }

                let dx = position.x - down.x;
                let dy = position.y - down.y;
                if (dx * dx + dy * dy).sqrt() >= self.threshold {
                    self.dragging = true;
                    self.last_position = Some(*position);
                    // `DragStarted.position` reports the *initial press*
                    // (where `down_position` was stored on `Down`), not the
                    // threshold-crossing position. Widgets that need to
                    // classify where the grab originated (e.g. ScrollBar's
                    // thumb-vs-track hit test) can rely on this; widgets
                    // that want the current pointer will get it in the
                    // immediately-following `DragMoved`.
                    return GestureResult::Recognized(GestureEvent::DragStarted {
                        position: down,
                        button: self.down_button.unwrap_or(PointerButton::Primary),
                    });
                }

                GestureResult::Pending
            }
            RawPointerEvent::Up { position, .. } => {
                if self.dragging {
                    self.dragging = false;
                    self.down_position = None;
                    return GestureResult::Recognized(GestureEvent::DragEnded {
                        position: *position,
                    });
                }
                self.down_position = None;
                GestureResult::Failed
            }
        }
    }

    fn reset(&mut self) {
        self.down_position = None;
        self.down_button = None;
        self.dragging = false;
        self.last_position = None;
    }

    fn priority(&self) -> u32 {
        20
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::test_helpers::*;

    #[test]
    fn drag_recognizer_fires_after_threshold() {
        let mut rec = DragRecognizer::new().threshold(5.0);

        assert!(matches!(
            rec.process(&down(Point::new(10.0, 10.0))),
            GestureResult::Pending
        ));

        // Small move — still pending
        assert!(matches!(
            rec.process(&move_to(Point::new(12.0, 10.0))),
            GestureResult::Pending
        ));

        // Large move — drag started
        assert!(matches!(
            rec.process(&move_to(Point::new(20.0, 10.0))),
            GestureResult::Recognized(GestureEvent::DragStarted { .. })
        ));
    }

    #[test]
    fn drag_emits_moved_and_ended() {
        let mut rec = DragRecognizer::new().threshold(1.0);
        rec.process(&down(Point::new(0.0, 0.0)));
        // Cross threshold
        rec.process(&move_to(Point::new(10.0, 0.0)));

        // Subsequent move
        let result = rec.process(&move_to(Point::new(15.0, 0.0)));
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DragMoved { .. })
        ));

        // Release
        let result = rec.process(&up(Point::new(15.0, 0.0)));
        assert!(matches!(
            result,
            GestureResult::Recognized(GestureEvent::DragEnded { .. })
        ));
    }

    #[test]
    fn drag_fails_on_up_without_movement() {
        let mut rec = DragRecognizer::new().threshold(5.0);
        rec.process(&down(Point::new(10.0, 10.0)));
        assert!(matches!(
            rec.process(&up(Point::new(10.0, 10.0))),
            GestureResult::Failed
        ));
    }

    #[test]
    fn reset_clears_state() {
        let mut rec = DragRecognizer::new().threshold(1.0);
        rec.process(&down(Point::new(0.0, 0.0)));
        rec.process(&move_to(Point::new(10.0, 0.0)));
        rec.reset();

        // After reset, move without down should be pending (not drag)
        assert!(matches!(
            rec.process(&move_to(Point::new(20.0, 0.0))),
            GestureResult::Pending
        ));
    }
}
