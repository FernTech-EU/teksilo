// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use crate::gesture::{GestureArenaSet, GestureEvent};

impl WidgetTree {
    /// Lazily install a gesture arena set populated with whichever recognizers
    /// the widget's handler set actually needs. Without this, a widget
    /// that wires `on_drag` or `on_double_tap` (but not `on_tap`) would
    /// never get a gesture arena and the handlers would never fire.
    ///
    /// What is installed is a list of *prototypes*, not live recognizers: the
    /// set instantiates an arena per contact as contacts arrive. The node's
    /// tap streak is installed with them and outlives every one of those
    /// arenas — which is what makes a touch double tap, two presses with two
    /// different pointer ids, recognizable at all.
    ///
    /// Checks BOTH handler buckets (own + external) so a recognizer gets
    /// installed whether the handler was attached via
    /// `apply_self_handlers` or via the `WidgetBuilder` chain.
    pub(crate) fn ensure_gesture_arena(
        node: &mut crate::arena::WidgetNode,
        id: WidgetId,
        gesture_owners: &mut std::collections::HashSet<WidgetId>,
    ) {
        if let Some(set) = node.handlers.gesture_arena.as_mut() {
            // Already installed — refresh the policy (a rebuild may have
            // changed it) and make sure the owners set is in sync (covers a
            // widget that re-enters dispatch after a pre-existing arena was
            // carried across rebuild).
            set.set_multi_contact(node.multi_contact);
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

        let mut set = GestureArenaSet::new();
        set.set_multi_contact(node.multi_contact);
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
            set.add(move || {
                let rec = crate::gesture::TapRecognizer::new();
                match tap_buttons {
                    Some(mask) => rec.accept_buttons(mask),
                    None => rec,
                }
            });
        }
        if has_double_tap {
            set.add(move || {
                let rec = crate::gesture::DoubleTapRecognizer::new();
                match double_tap_buttons {
                    Some(mask) => rec.accept_buttons(mask),
                    None => rec,
                }
            });
        }
        if has_triple_tap {
            set.add(move || {
                let rec = crate::gesture::TripleTapRecognizer::new();
                match triple_tap_buttons {
                    Some(mask) => rec.accept_buttons(mask),
                    None => rec,
                }
            });
        }
        if has_drag {
            // No `.threshold(..)`: the drag slop is the active profile's, so a
            // finger gets 18 dp where a mouse keeps its 5.
            set.add(crate::gesture::DragRecognizer::new);
        }
        if has_long_press {
            set.add(move || {
                let rec = crate::gesture::LongPressRecognizer::new();
                match long_press_buttons {
                    Some(mask) => rec.accept_buttons(mask),
                    None => rec,
                }
            });
        }
        if has_swipe {
            set.add(crate::gesture::SwipeRecognizer::new);
        }
        node.handlers.gesture_arena = Some(set);
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
            GestureEvent::DragStarted {
                position,
                button,
                pointer,
            } => {
                // Auto-capture the pointer for the duration of the drag so
                // the widget keeps receiving `Moved` / `Ended` even when
                // the cursor leaves its bounds. Released on `DragEnded`.
                ctx.capture_pointer();
                let phase = DragPhase::Started {
                    position,
                    button,
                    pointer,
                };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::DragMoved {
                position,
                delta,
                pointer,
            } => {
                let phase = DragPhase::Moved {
                    position,
                    delta,
                    pointer,
                };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::DragEnded { position, pointer } => {
                let phase = DragPhase::Ended { position, pointer };
                if let Some(h) = node.external_handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_drag.as_mut() {
                    h(phase, ctx);
                }
                ctx.release_pointer();
            }
            GestureEvent::DragCancelled {
                position,
                pointer,
                reason,
            } => {
                // Same shape as `DragEnded`: the handler is told, then the
                // implicit capture the drag took is given back. A handler that
                // committed as it went unwinds here.
                let phase = DragPhase::Cancelled {
                    position,
                    pointer,
                    reason,
                };
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
                let phase = PinchPhase::Started {
                    center,
                    pointer: ctx.pointer(),
                };
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(phase, ctx);
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
                    pointer: ctx.pointer(),
                };
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::PinchEnded => {
                let phase = PinchPhase::Ended {
                    pointer: ctx.pointer(),
                };
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
            }
            GestureEvent::PinchCancelled { reason } => {
                let phase = PinchPhase::Cancelled {
                    pointer: ctx.pointer(),
                    reason,
                };
                if let Some(h) = node.external_handlers.on_pinch.as_mut() {
                    h(phase, ctx);
                }
                if let Some(h) = node.handlers.on_pinch.as_mut() {
                    h(phase, ctx);
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
            PinchPhase::Ended { .. } => ended_flag.set(true),
            _ => {}
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
            _ => {}
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
            tree.pointer_captured_by(),
            None,
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

    #[test]
    fn click_on_tap_child_then_hover_does_not_start_ancestor_drag() {
        // Regression: press+RELEASE on a descendant tap child (no drag), THEN
        // move the mouse with no button held. Arming the ancestor drag on the
        // press fed the ancestor's DragRecognizer a `Down`; the release must
        // clear that armed state, or the *next* hover move crosses the drag
        // threshold and starts a phantom drag. This is the corkboard bug: a
        // card's read-only RichTextEditor captures the press as an interactive
        // child, and a later hover-move dragged the card.
        use crate::gesture::DragPhase;
        use crate::test_widgets::StackWidget;
        use std::cell::Cell;
        use std::rc::Rc;

        let drag_started = Rc::new(Cell::new(false));
        let d = drag_started.clone();

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(move |_p, _c| { /* select */ }));
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

        // Click (press then release at the same point — the tap resolves).
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
        // Now hover somewhere (no button down). Must NOT start the drag.
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(85.0, 25.0),
        });
        assert!(
            !drag_started.get(),
            "a hover after a click must not start a phantom ancestor drag"
        );
    }
    // ---------------------------------------------------------------
    // P06: per-contact arenas, the node tap streak, and the profile
    // ---------------------------------------------------------------

    use crate::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };

    /// A fresh touch contact — the platform mints a new id per press, which is
    /// exactly why the tap streak cannot live inside a recognizer.
    fn new_contact() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        PointerIdAllocator::global().begin(
            BackendDeviceKey::DEFAULT,
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    fn touch_sample(
        id: PointerId,
        phase: PointerPhase,
        position: Point,
        time: EventTime,
    ) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, time),
            phase,
            position,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// Press and release one fresh contact at `at`.
    fn touch_tap(tree: &mut WidgetTree, at: Point, down_ms: u64, up_ms: u64) {
        let id = new_contact();
        tree.dispatch_pointer(touch_sample(
            id,
            PointerPhase::Down,
            at,
            EventTime::from_millis(down_ms),
        ));
        tree.dispatch_pointer(touch_sample(
            id,
            PointerPhase::Up,
            at,
            EventTime::from_millis(up_ms),
        ));
    }

    #[test]
    fn a_touch_double_tap_survives_two_pointer_ids_through_the_tree() {
        use std::cell::Cell;
        use std::rc::Rc;

        let doubles = Rc::new(Cell::new(0));
        let flag = doubles.clone();

        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().on_double_tap(move |_e, _ctx| {
            flag.set(flag.get() + 1);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let at = Point::new(50.0, 25.0);
        touch_tap(&mut tree, at, 0, 40);
        assert_eq!(doubles.get(), 0, "one tap is not a double tap");
        touch_tap(&mut tree, at, 150, 190);
        assert_eq!(
            doubles.get(),
            1,
            "the pair must be recognized across two contacts"
        );
    }

    #[test]
    fn gesture_owners_holds_exactly_the_arena_owning_nodes() {
        use std::cell::Cell;
        use std::rc::Rc;

        let ticks = Rc::new(Cell::new(0));
        let flag = ticks.clone();

        let mut tree = WidgetTree::new();
        // Two nodes, only one of which wires a gesture handler.
        let plain = tree.add(FillWidget::new());
        let gestured = tree.add(FillWidget::new().on_long_press(move |_e, _ctx| {
            flag.set(flag.get() + 1);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        // Prototypes are installed on the first press that reaches the node…
        assert!(tree.gesture_owners.is_empty());
        let center = tree.bounds(gestured).center();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            tree.gesture_owners.iter().copied().collect::<Vec<_>>(),
            vec![gestured],
            "only the node that actually carries recognizers is an owner"
        );
        assert!(!tree.gesture_owners.contains(&plain));

        // …and the install is what the sweep is bounded by, not the live
        // per-contact arenas: the owner is visited and does fire.
        tree.tick_gestures(std::time::Instant::now() + std::time::Duration::from_millis(600));
        assert_eq!(ticks.get(), 1, "the tick sweep must visit the owner");

        // Destroying the widget takes it back out of the set.
        tree.destroy_subtree(gestured);
        assert!(!tree.gesture_owners.contains(&gestured));
    }

    #[test]
    fn under_multi_contact_first_only_one_contact_is_served() {
        use std::cell::Cell;
        use std::rc::Rc;

        let taps = Rc::new(Cell::new(0));
        let flag = taps.clone();

        let mut tree = WidgetTree::new();
        // No `.multi_contact(..)`: `First` is the default, and it is what every
        // widget written before the touch programme assumes.
        tree.add(FillWidget::new().on_tap(move |_e, _ctx| {
            flag.set(flag.get() + 1);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let a = new_contact();
        let b = new_contact();
        tree.dispatch_pointer(touch_sample(
            a,
            PointerPhase::Down,
            Point::new(40.0, 25.0),
            EventTime::from_millis(0),
        ));
        // Second finger while the first is still down: terminated at the node.
        tree.dispatch_pointer(touch_sample(
            b,
            PointerPhase::Down,
            Point::new(60.0, 25.0),
            EventTime::from_millis(5),
        ));
        tree.dispatch_pointer(touch_sample(
            b,
            PointerPhase::Up,
            Point::new(60.0, 25.0),
            EventTime::from_millis(20),
        ));
        tree.dispatch_pointer(touch_sample(
            a,
            PointerPhase::Up,
            Point::new(40.0, 25.0),
            EventTime::from_millis(30),
        ));

        assert_eq!(taps.get(), 1, "the refused contact must produce no tap");
    }

    #[test]
    fn under_multi_contact_all_every_contact_is_served() {
        use crate::gesture::MultiContact;
        use std::cell::Cell;
        use std::rc::Rc;

        let taps = Rc::new(Cell::new(0));
        let flag = taps.clone();

        let mut tree = WidgetTree::new();
        tree.add(
            FillWidget::new()
                .on_tap(move |_e, _ctx| {
                    flag.set(flag.get() + 1);
                })
                .multi_contact(MultiContact::All),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let a = new_contact();
        let b = new_contact();
        // Overlapping presses: a goes down, b goes down, b lifts, a lifts.
        for (id, x, ms) in [(a, 40.0, 0u64), (b, 60.0, 5)] {
            tree.dispatch_pointer(touch_sample(
                id,
                PointerPhase::Down,
                Point::new(x, 25.0),
                EventTime::from_millis(ms),
            ));
        }
        for (id, x, ms) in [(b, 60.0, 20u64), (a, 40.0, 30)] {
            tree.dispatch_pointer(touch_sample(
                id,
                PointerPhase::Up,
                Point::new(x, 25.0),
                EventTime::from_millis(ms),
            ));
        }

        assert_eq!(taps.get(), 2, "both contacts must tap");
    }

    #[test]
    fn a_mouse_drag_still_arms_at_five_pixels_and_a_finger_does_not() {
        use crate::gesture::DragPhase;
        use std::cell::Cell;
        use std::rc::Rc;

        fn armed_after(travel: f32, touch: bool) -> bool {
            let started = Rc::new(Cell::new(false));
            let flag = started.clone();
            let mut tree = WidgetTree::new();
            tree.add(FillWidget::new().on_drag(move |phase, _ctx| {
                if matches!(phase, DragPhase::Started { .. }) {
                    flag.set(true);
                }
            }));
            tree.layout(SizeProposal::exact(200.0, 50.0));

            let from = Point::new(20.0, 25.0);
            let to = Point::new(20.0 + travel, 25.0);
            if touch {
                let id = new_contact();
                tree.dispatch_pointer(touch_sample(
                    id,
                    PointerPhase::Down,
                    from,
                    EventTime::from_millis(0),
                ));
                tree.dispatch_pointer(touch_sample(
                    id,
                    PointerPhase::Move,
                    to,
                    EventTime::from_millis(10),
                ));
            } else {
                tree.dispatch_event(WidgetEvent::PointerDown {
                    position: from,
                    button: PointerButton::Primary,
                    modifiers: Modifiers::NONE,
                });
                tree.dispatch_event(WidgetEvent::PointerMove { position: to });
            }
            started.get()
        }

        // The mouse column of `GestureProfile::MOUSE` is byte-for-byte the
        // pre-P06 constant: 5 dp, `>=`.
        assert!(!armed_after(4.0, false), "4 dp is under the mouse slop");
        assert!(armed_after(5.0, false), "5 dp is the mouse slop");

        // The same travel on a finger is nothing — 18 dp of touch slop.
        assert!(!armed_after(5.0, true));
        assert!(!armed_after(17.0, true));
        assert!(armed_after(18.0, true), "18 dp is the touch slop");
    }

    #[test]
    fn a_long_press_fires_on_a_simulated_clock() {
        use crate::pointer::clock::ManualClock;
        use std::cell::Cell;
        use std::rc::Rc;

        let pressed = Rc::new(Cell::new(false));
        let flag = pressed.clone();

        let mut tree = WidgetTree::new();
        let clock = std::rc::Rc::new(ManualClock::new(EventTime::ZERO));
        tree.set_input_clock(clock.clone());
        let widget = tree.add(FillWidget::new().on_long_press(move |_e, _ctx| {
            flag.set(true);
        }));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let center = tree.bounds(widget).center();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: center,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });

        // Not yet: 499 ms is under the mouse profile's 500 ms hold.
        clock.set(EventTime::from_millis(499));
        tree.tick_gestures(std::time::Instant::now());
        assert!(!pressed.get());

        // Exactly at the hold, with no sleeping anywhere.
        clock.set(EventTime::from_millis(500));
        tree.tick_gestures(std::time::Instant::now());
        assert!(pressed.get(), "the hold is 500 ms on the mouse profile");
    }

    #[test]
    fn a_mouse_triple_click_still_escalates_through_the_tree() {
        use std::cell::Cell;
        use std::rc::Rc;

        let doubles = Rc::new(Cell::new(0));
        let triples = Rc::new(Cell::new(0));
        let d = doubles.clone();
        let t = triples.clone();

        let mut tree = WidgetTree::new();
        let widget = tree.add(
            FillWidget::new()
                .on_double_tap(move |_e, _ctx| d.set(d.get() + 1))
                .on_triple_tap(move |_e, _ctx| t.set(t.get() + 1)),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let center = tree.bounds(widget).center();
        for _ in 0..3 {
            tree.dispatch_event(WidgetEvent::PointerDown {
                position: center,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
            tree.dispatch_event(WidgetEvent::PointerUp {
                position: center,
                button: PointerButton::Primary,
                modifiers: Modifiers::NONE,
            });
        }
        assert_eq!(doubles.get(), 1, "click 2 fires DoubleTap");
        assert_eq!(triples.get(), 1, "click 3 fires TripleTap");
    }
}
