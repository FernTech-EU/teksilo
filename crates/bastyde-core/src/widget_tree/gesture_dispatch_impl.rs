// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use crate::gesture::{GestureArena, GestureEvent};

impl WidgetTree {
    /// Lazily install a gesture arena populated with whichever recognizers
    /// the widget's handler set actually needs. Without this, a widget
    /// that wires `on_drag` or `on_double_tap` (but not `on_tap`) would
    /// never get a gesture arena and the handlers would never fire.
    ///
    /// Checks BOTH handler buckets (own + external) so a recognizer gets
    /// installed whether the handler was attached via
    /// `apply_self_handlers` or via the `WidgetBuilder` chain.
    pub(crate) fn ensure_gesture_arena(
        node: &mut crate::arena::WidgetNode,
        id: WidgetId,
        gesture_owners: &mut std::collections::HashSet<WidgetId>,
    ) {
        if node.handlers.gesture_arena.is_some() {
            // Already installed — make sure the owners set is in sync
            // (covers a widget that re-enters dispatch after a
            // pre-existing arena was carried across rebuild).
            gesture_owners.insert(id);
            return;
        }
        let has_tap = node.any_handler(|h| h.on_tap.is_some());
        let has_double_tap = node.any_handler(|h| h.on_double_tap.is_some());
        let has_triple_tap = node.any_handler(|h| h.on_triple_tap.is_some());
        let has_drag = node.any_handler(|h| h.on_drag.is_some());
        let has_long_press = node.any_handler(|h| h.on_long_press.is_some());
        let has_swipe = node.any_handler(|h| h.on_swipe.is_some());

        if !(has_tap || has_double_tap || has_triple_tap || has_drag || has_long_press || has_swipe)
        {
            return;
        }

        // Read per-handler button-mask overrides from BOTH buckets,
        // preferring the own (`handlers`) bucket. Falls back to the
        // recognizer's own default (`ButtonMask::PRIMARY`) when neither
        // bucket sets a mask.
        let tap_buttons = node
            .handlers
            .tap_buttons
            .or(node.external_handlers.tap_buttons);
        let double_tap_buttons = node
            .handlers
            .double_tap_buttons
            .or(node.external_handlers.double_tap_buttons);
        let triple_tap_buttons = node
            .handlers
            .triple_tap_buttons
            .or(node.external_handlers.triple_tap_buttons);
        let long_press_buttons = node
            .handlers
            .long_press_buttons
            .or(node.external_handlers.long_press_buttons);

        let mut arena = GestureArena::new();
        // Important: install `TapRecognizer` ONLY when the widget actually
        // wired `on_tap` AND no multi-tap recognizer is in the arena. A
        // parallel `TapRecognizer` would let `Tap` win on the first up
        // (it returns `Recognized` while `DoubleTap` / `TripleTap` return
        // `Pending`), and the arena's reset loop would wipe the multi-tap
        // state. Multi-tap recognizers opt out of that reset via
        // `resets_on_peer_recognition = false`, so once we install a
        // multi-tap recognizer, we intentionally skip `TapRecognizer` —
        // callers that need click-1 behaviour under a multi-tap widget
        // use `on_pointer_event::PointerDown` (which fires before the
        // gesture arena and runs regardless of multi-tap state).
        if has_tap && !(has_double_tap || has_triple_tap) {
            let mut rec = crate::gesture::TapRecognizer::new();
            if let Some(mask) = tap_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_double_tap {
            let mut rec = crate::gesture::DoubleTapRecognizer::new();
            if let Some(mask) = double_tap_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_triple_tap {
            let mut rec = crate::gesture::TripleTapRecognizer::new();
            if let Some(mask) = triple_tap_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_drag {
            arena.add(crate::gesture::DragRecognizer::new().threshold(5.0));
        }
        if has_long_press {
            let mut rec = crate::gesture::LongPressRecognizer::new();
            if let Some(mask) = long_press_buttons {
                rec = rec.accept_buttons(mask);
            }
            arena.add(rec);
        }
        if has_swipe {
            arena.add(crate::gesture::SwipeRecognizer::new());
        }
        node.handlers.gesture_arena = Some(arena);
        gesture_owners.insert(id);
    }

    /// Route a gesture recognized by the arena (or the OS pinch/rotate
    /// stream) to the matching handler on the node.
    pub(crate) fn dispatch_recognized_gesture(
        node: &mut crate::arena::WidgetNode,
        gesture: GestureEvent,
        ctx: &mut EventContext,
    ) {
        use crate::gesture::{DragPhase, PinchPhase};
        // Every gesture handler invocation runs under a
        // `Handler` source label. Any `ctx.send_intent(...)` issued
        // from inside a tap / double-tap / drag / etc. handler
        // emits with `IntentSource::Handler`. The label is restored
        // at the bottom of this fn so nested dispatch doesn't
        // pollute the wrong bucket.
        let saved_source = ctx
            .current_source
            .replace(crate::telemetry::IntentSource::Handler);
        // For every gesture handler, fire BOTH the external and own slot
        // in that order so a widget that wired an on_tap via the
        // WidgetBuilder AND via apply_self_handlers sees both callbacks —
        // and more importantly, so widgets that rely on one bucket don't
        // miss the gesture when the other is empty.
        match gesture {
            GestureEvent::Tap(event) => {
                if let Some(h) = node.external_handlers.on_tap.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_tap.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::DoubleTap(event) => {
                if let Some(h) = node.external_handlers.on_double_tap.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_double_tap.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::TripleTap(event) => {
                if let Some(h) = node.external_handlers.on_triple_tap.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_triple_tap.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::LongPress(event) => {
                if let Some(h) = node.external_handlers.on_long_press.as_mut() {
                    h(&event, ctx);
                }
                if let Some(h) = node.handlers.on_long_press.as_mut() {
                    h(&event, ctx);
                }
            }
            GestureEvent::DragStarted { position, button } => {
                // Auto-capture the pointer for the duration of the drag so
                // the widget keeps receiving `Moved` / `Ended` even when
                // the cursor leaves its bounds. Released on `DragEnded`.
                ctx.capture_pointer();
                let phase = DragPhase::Started { position, button };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::DragMoved { position, delta } => {
                let phase = DragPhase::Moved { position, delta };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::DragEnded { position } => {
                let phase = DragPhase::Ended { position };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                ctx.release_pointer();
            }
            GestureEvent::Swipe {
                direction,
                velocity,
            } => {
                if let Some(h) = node.external_handlers.on_swipe.as_mut() {
                    h(direction, velocity, ctx);
                }
                if let Some(h) = node.handlers.on_swipe.as_mut() {
                    h(direction, velocity, ctx);
                }
            }
            GestureEvent::PinchStarted { center } => {
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(PinchPhase::Started { center }, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(PinchPhase::Started { center }, ctx);
                }
            }
            GestureEvent::PinchChanged {
                center,
                scale,
                rotation,
            } => {
                let phase = PinchPhase::Changed {
                    center,
                    scale,
                    rotation,
                };
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::PinchEnded => {
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(PinchPhase::Ended, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(PinchPhase::Ended, ctx);
                }
            }
        }
        // Restore — see the matching `replace` at the
        // top of this function.
        ctx.current_source = saved_source;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;
    use crate::widget_builder::WidgetBuilder;

    #[test]
    fn gesture_tap_recognized_on_click() {
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let tapped_flag = tapped.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_tap(move |_pos, _ctx| {
            tapped_flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(widget);
        assert!(tapped.get());
    }

    #[test]
    fn gesture_drag_recognized_on_drag() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        let drag_started = Rc::new(Cell::new(false));
        let drag_ended = Rc::new(Cell::new(false));
        let start_flag = drag_started.clone();
        let end_flag = drag_ended.clone();

        let mut tree = WidgetTree::new();
        let _widget = tree.add(FillWidget::new().on_drag(move |phase, _ctx| match phase {
            DragPhase::Started { .. } => start_flag.set(true),
            DragPhase::Ended { .. } => end_flag.set(true),
            _ => {}
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));

        assert!(drag_started.get());
        assert!(drag_ended.get());
    }

    #[test]
    fn gesture_handler_called_on_tap() {
        use std::cell::Cell;
        use std::rc::Rc;

        let handler_called = Rc::new(Cell::new(false));
        let handler_flag = handler_called.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_tap(move |_pos, _ctx| {
            handler_flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(widget);
        assert!(handler_called.get());
    }

    #[test]
    fn on_swipe_fires_from_platform_gesture_event() {
        use crate::gesture::{GestureEvent, SwipeDirection};
        use std::cell::Cell;
        use std::rc::Rc;

        let observed: Rc<Cell<Option<(SwipeDirection, i32)>>> = Rc::new(Cell::new(None));
        let flag = observed.clone();

        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new().on_swipe(move |direction, velocity, _ctx| {
                flag.set(Some((direction, velocity as i32)));
            }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::Swipe {
                direction: SwipeDirection::Left,
                velocity: 450.0,
            },
        });

        let got = observed.get();
        assert!(matches!(got, Some((SwipeDirection::Left, 450))));
    }

    #[test]
    fn on_pinch_fires_from_platform_gesture_event() {
        use crate::gesture::{GestureEvent, PinchPhase};
        use std::cell::Cell;
        use std::rc::Rc;

        let started = Rc::new(Cell::new(false));
        let scale_seen = Rc::new(Cell::new(0.0_f32));
        let ended = Rc::new(Cell::new(false));
        let started_flag = started.clone();
        let scale_flag = scale_seen.clone();
        let ended_flag = ended.clone();

        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_pinch(move |phase, _ctx| match phase {
            PinchPhase::Started { .. } => started_flag.set(true),
            PinchPhase::Changed { scale, .. } => scale_flag.set(scale),
            PinchPhase::Ended => ended_flag.set(true),
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.pointer_move(Point::new(50.0, 25.0));
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::PinchStarted {
                center: Point::new(50.0, 25.0),
            },
        });
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::PinchChanged {
                center: Point::new(50.0, 25.0),
                scale: 1.5,
                rotation: 0.0,
            },
        });
        tree.dispatch_event(WidgetEvent::Gesture {
            gesture: GestureEvent::PinchEnded,
        });

        assert!(started.get());
        assert!((scale_seen.get() - 1.5).abs() < 0.001);
        assert!(ended.get());
    }

    #[test]
    fn drag_auto_captures_pointer_until_ended() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        let started = Rc::new(Cell::new(false));
        let moved = Rc::new(Cell::new(0));
        let ended = Rc::new(Cell::new(false));
        let started_flag = started.clone();
        let moved_flag = moved.clone();
        let ended_flag = ended.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_drag(move |phase, _ctx| match phase {
            DragPhase::Started { .. } => started_flag.set(true),
            DragPhase::Moved { .. } => moved_flag.set(moved_flag.get() + 1),
            DragPhase::Ended { .. } => ended_flag.set(true),
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Press inside, move past the 5px threshold while still inside —
        // DragRecognizer emits DragStarted, and auto-capture kicks in.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(70.0, 25.0),
        });
        assert!(started.get(), "DragStarted must fire");

        // Move the pointer well outside the widget bounds. Without
        // auto-capture this event would hit-test to another widget and
        // the scrollbar would never see it.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(500.0, 500.0),
        });
        assert!(
            moved.get() >= 1,
            "Move outside bounds must still reach drag handler"
        );

        // Release outside bounds — must still fire DragEnded on the
        // original widget, and pointer capture must be released.
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(500.0, 500.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(ended.get(), "DragEnded must fire on the original widget");
        assert_eq!(
            tree.pointer_captured_by, None,
            "pointer capture must be released after DragEnded"
        );

        // Sanity: the widget we instantiated is the one we hooked.
        let _ = widget;
    }

    #[test]
    fn on_long_press_fires_from_tick_gestures() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::time::{Duration, Instant};

        let pressed = Rc::new(Cell::new(false));
        let pressed_flag = pressed.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().on_long_press(move |_pos, _ctx| {
            pressed_flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let center = tree.bounds(widget).center();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Before the timeout, tick does nothing.
        tree.tick_gestures(Instant::now());
        assert!(!pressed.get());

        // After the configured 500ms, tick fires the handler.
        tree.tick_gestures(Instant::now() + Duration::from_millis(600));
        assert!(pressed.get());

        // After firing there is no remaining deadline.
        assert!(tree.next_gesture_deadline().is_none());
    }

    #[test]
    fn multiple_recognizers_on_same_widget() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let dragged = Rc::new(Cell::new(false));
        let tapped_flag = tapped.clone();
        let dragged_flag = dragged.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .on_tap(move |_pos, _ctx| {
                    tapped_flag.set(true);
                })
                .on_drag(move |phase, _ctx| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        dragged_flag.set(true);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.click(widget);
        assert!(tapped.get());
        assert!(!dragged.get());

        tapped.set(false);
        dragged.set(false);

        tree.drag(Point::new(50.0, 25.0), Point::new(80.0, 25.0));
        assert!(dragged.get());
    }

    #[test]
    fn ancestor_drag_starts_through_descendant_tap_capture() {
        // The cross-widget tap-vs-drag disambiguation: a descendant `on_tap`
        // (which captures the pointer on PointerDown) must NOT permanently
        // shadow an ancestor `on_drag`. A plain click fires the descendant tap;
        // a press-then-move starts the ANCESTOR drag instead.
        use crate::gesture::DragPhase;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let tapped = Rc::new(Cell::new(false));
        let drag_started = Rc::new(Cell::new(false));
        let t = tapped.clone();
        let d = drag_started.clone();

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(move |_p, _c| t.set(true)));
        // Ancestor container carries the drag; child sits on top and taps.
        let _parent = tree.add(
            StackWidget::new()
                .add_child(child)
                .on_drag(move |phase, _c| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        d.set(true);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // 1) A plain click on the child fires the child's tap, not the drag.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(
            tapped.get(),
            "a click on the child fires the descendant tap"
        );
        assert!(
            !drag_started.get(),
            "a click must not start the ancestor drag"
        );

        tapped.set(false);
        drag_started.set(false);

        // 2) Press on the child, then move past threshold → the ANCESTOR drag
        // starts (it observed the pointer while the child tap held capture),
        // and the descendant tap does NOT fire.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 25.0),
        });
        assert!(
            drag_started.get(),
            "dragging from the child must start the ancestor drag"
        );
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: Point::new(80.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert!(!tapped.get(), "a drag must not fire the descendant tap");
    }

    #[test]
    fn ancestor_drag_starts_through_deeply_nested_tap_capture() {
        // The SceneView shape: an `on_drag` container whose child is an
        // `on_tap`-wrapped card with a DEEPER inner leaf (the hit target). The
        // press bubbles up to the card's tap (which captures), and the
        // container's drag must still start on move — proving the disambiguation
        // walks multiple levels, not just an immediate parent.
        use crate::gesture::DragPhase;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let drag_started = Rc::new(Cell::new(false));
        let d = drag_started.clone();

        let mut tree = WidgetTree::new();
        // Deepest leaf — the hit target, no handlers of its own.
        let inner = tree.add(FillWidget::new());
        // Card: an on_tap wrapper around a container holding the inner leaf.
        let card = tree.add(
            StackWidget::new()
                .add_child(inner)
                .on_tap(move |_p, _c| { /* select */ }),
        );
        // Canvas: an on_drag container holding the card.
        let _canvas = tree.add(
            StackWidget::new()
                .add_child(card)
                .on_drag(move |phase, _c| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        d.set(true);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 25.0),
        });
        assert!(
            drag_started.get(),
            "dragging a deeply-nested tappable child must start the ancestor drag"
        );
    }
}
