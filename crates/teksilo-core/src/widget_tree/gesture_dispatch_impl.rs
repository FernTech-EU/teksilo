// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use crate::event::ButtonMask;
use crate::gesture::{GestureArenaSet, GestureEvent};

/// Which recognizers a node's handler set asks for, and on which buttons.
///
/// Read once and used twice: [`WidgetTree::ensure_gesture_arena`] installs the
/// arena from it, and [`WidgetTree::press_buttons`] decides from the very same
/// reading which buttons may raise the node's press visual. One reader, so the
/// arena and the visual cannot come to disagree about what this node acts on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArenaRecognizers {
    has_tap: bool,
    has_double_tap: bool,
    has_triple_tap: bool,
    has_long_press: bool,
    has_drag: bool,
    has_swipe: bool,
    /// Per-recognizer overrides, own bucket preferred over external. `None`
    /// leaves the recognizer on its own default, [`ButtonMask::PRIMARY`].
    tap_buttons: Option<ButtonMask>,
    double_tap_buttons: Option<ButtonMask>,
    triple_tap_buttons: Option<ButtonMask>,
    long_press_buttons: Option<ButtonMask>,
}

impl ArenaRecognizers {
    /// Read BOTH handler buckets, so a handler attached via
    /// `apply_self_handlers` and one attached through the `WidgetBuilder`
    /// chain count the same.
    pub(crate) fn read(node: &crate::arena::WidgetNode) -> Self {
        Self {
            has_tap: node.any_handler(|h| h.on_tap.is_some()),
            has_double_tap: node.any_handler(|h| h.on_double_tap.is_some()),
            has_triple_tap: node.any_handler(|h| h.on_triple_tap.is_some()),
            has_long_press: node.any_handler(|h| h.on_long_press.is_some()),
            has_drag: node.any_handler(|h| h.on_drag.is_some()),
            has_swipe: node.any_handler(|h| h.on_swipe.is_some()),
            tap_buttons: node
                .handlers
                .tap_buttons
                .or(node.external_handlers.tap_buttons),
            double_tap_buttons: node
                .handlers
                .double_tap_buttons
                .or(node.external_handlers.double_tap_buttons),
            triple_tap_buttons: node
                .handlers
                .triple_tap_buttons
                .or(node.external_handlers.triple_tap_buttons),
            long_press_buttons: node
                .handlers
                .long_press_buttons
                .or(node.external_handlers.long_press_buttons),
        }
    }

    /// Whether any recognizer at all is wanted — the gate on installing an
    /// arena.
    fn any(self) -> bool {
        self.has_tap
            || self.has_double_tap
            || self.has_triple_tap
            || self.has_drag
            || self.has_long_press
            || self.has_swipe
    }

    /// Whether the plain `TapRecognizer` is one of the installed prototypes.
    ///
    /// A multi-tap recognizer in the arena suppresses it — see the comment at
    /// the install site — so its mask must not speak for a node whose
    /// `TapRecognizer` was never built.
    fn installs_tap(self) -> bool {
        self.has_tap && !(self.has_double_tap || self.has_triple_tap)
    }

    /// The buttons a press on this node could actually act on.
    ///
    /// The four click-style recognizers each carry a [`ButtonMask`], defaulting
    /// to [`ButtonMask::PRIMARY`]; a drag and a swipe carry none and act on
    /// whatever button the press arrived with. So a node whose arena is
    /// click-style throughout answers with the union of those masks, and one
    /// that also drags or swipes — or that carries no recognizer at all,
    /// because it captured the pointer explicitly and drives its own
    /// `on_pointer_event` — has nothing to say against any button and answers
    /// [`ButtonMask::ALL`].
    fn accepted_buttons(self) -> ButtonMask {
        if self.has_drag || self.has_swipe {
            return ButtonMask::ALL;
        }
        let mut mask = ButtonMask::NONE;
        let mut add = |on: bool, declared: Option<ButtonMask>| {
            if on {
                mask = mask.union(declared.unwrap_or(ButtonMask::PRIMARY));
            }
        };
        add(self.installs_tap(), self.tap_buttons);
        add(self.has_double_tap, self.double_tap_buttons);
        add(self.has_triple_tap, self.triple_tap_buttons);
        add(self.has_long_press, self.long_press_buttons);
        if mask.is_empty() {
            ButtonMask::ALL
        } else {
            mask
        }
    }
}

impl WidgetTree {
    /// The buttons on which a press over `id` may raise its press visual.
    ///
    /// The visual says "release here and this control acts", so it must answer
    /// to the same buttons the activation does — otherwise a middle-click, or a
    /// right-click on a node with no context menu, lights a control up for a
    /// press that can never complete. See
    /// [`ArenaRecognizers::accepted_buttons`].
    pub(crate) fn press_buttons(&self, id: WidgetId) -> ButtonMask {
        self.arena.get(id).map_or(ButtonMask::ALL, |node| {
            ArenaRecognizers::read(node).accepted_buttons()
        })
    }

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
        // One reading of the handler buckets, shared with `press_buttons` so
        // the arena that fires and the visual that lights up cannot come to
        // disagree about which buttons this node acts on.
        let wanted = ArenaRecognizers::read(node);
        if !wanted.any() {
            return;
        }
        let ArenaRecognizers {
            // `has_tap` alone does not decide: a multi-tap recognizer
            // suppresses the plain one, so the install below asks
            // `installs_tap()`.
            has_tap: _,
            has_double_tap,
            has_triple_tap,
            has_long_press,
            has_drag,
            has_swipe,
            tap_buttons,
            double_tap_buttons,
            triple_tap_buttons,
            long_press_buttons,
        } = wanted;

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
        if wanted.installs_tap() {
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
                // Implicit: the recognizer has already won the arbitration,
                // so this is bookkeeping rather than a fresh claim.
                ctx.capture_pointer_implicit();
                // A drag owns the rest of the press: tell the router so the
                // sequence records this node as its winner and every peer is
                // cancelled exactly once.
                ctx.recognized_owning_gesture = true;
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
                // Like a drag, a swipe owns the press it completes.
                ctx.recognized_owning_gesture = true;
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
        // starts (it competes for the sequence while the child tap holds
        // capture), and the descendant tap does NOT fire.
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: Point::new(50.0, 25.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
        // The arbitration is observable: exactly one competitor, the ancestor,
        // enrolled as a `Gesture` member and still in the running. This is the
        // migration contract for the deleted `drag_observers`.
        assert_eq!(
            tree.sequence_members(crate::pointer::PointerId::MOUSE),
            vec![(
                _parent,
                crate::gesture::MemberRole::Gesture,
                crate::gesture::MemberState::Possible
            )],
        );
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 25.0),
        });
        assert!(
            drag_started.get(),
            "dragging from the child must start the ancestor drag"
        );
        assert_eq!(
            tree.sequence_winner(crate::pointer::PointerId::MOUSE),
            Some(_parent),
            "and the ancestor is the sequence's winner"
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
        // Enrolment walks the whole frozen path, not just the immediate
        // parent: the canvas two levels up is the one competitor.
        assert_eq!(
            tree.sequence_members(crate::pointer::PointerId::MOUSE),
            vec![(
                _canvas,
                crate::gesture::MemberRole::Gesture,
                crate::gesture::MemberState::Possible
            )],
        );
        tree.dispatch_event(WidgetEvent::PointerMove {
            position: Point::new(80.0, 25.0),
        });
        assert!(
            drag_started.get(),
            "dragging a deeply-nested tappable child must start the ancestor drag"
        );
        assert_eq!(
            tree.sequence_winner(crate::pointer::PointerId::MOUSE),
            Some(_canvas),
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
        // The release sweep closed the sequence, so nothing is competing any
        // more — the state that used to leak was an armed ancestor recognizer.
        assert!(
            tree.sequence_members(crate::pointer::PointerId::MOUSE)
                .is_empty(),
            "the release sweep ends the sequence"
        );
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

// -------------------------------------------------------------------------
// P08: the arbitration spine — `PointerSequence` and the ordered procedure
// -------------------------------------------------------------------------

#[cfg(test)]
mod arbitration_tests {
    use super::*;
    use crate::event::EventResponse;
    use crate::event::{Modifiers, PointerButton, WidgetEvent};
    use crate::gesture::{DragPhase, MemberRole, MemberState};
    use crate::pointer::touch_action::{PanClaim, TouchAction};
    use crate::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use crate::test_widgets::{FillWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_canvas::{Point, SizeProposal};
    use teksilo_tokens::{DragActivation, PointerKind, TargetDensity};

    /// A fresh contact: the platform mints a new id per press.
    fn new_contact() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(9_000);
        PointerIdAllocator::global().begin(
            BackendDeviceKey::DEFAULT,
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    fn touch(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    fn press(tree: &mut WidgetTree, at: Point) {
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    fn moved(tree: &mut WidgetTree, at: Point) {
        tree.dispatch_event(WidgetEvent::PointerMove { position: at });
    }

    fn release(tree: &mut WidgetTree, at: Point) {
        tree.dispatch_event(WidgetEvent::PointerUp {
            position: at,
            button: PointerButton::Primary,
            modifiers: Modifiers::NONE,
        });
    }

    /// **The single most important test in the package.** A mouse drag latches
    /// at 5.0 dp whatever the frozen `TouchAction` and whatever the density —
    /// reading `slop_precise` (2.0) for a precise pointer would silently
    /// retune every mouse drag in the framework.
    #[test]
    fn a_mouse_drag_latches_at_five_in_every_configuration() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            for action in [
                TouchAction::AUTO,
                TouchAction::NONE,
                TouchAction::PAN,
                TouchAction::PAN_X,
                TouchAction::PAN_Y,
                TouchAction::MANIPULATION,
            ] {
                // `DragPhase::Started` reports the press origin, not the
                // sample that latched it, so measure the latch by *when* the
                // handler fired: walk one pixel at a time and record the
                // travel at the first Started.
                let latched = Rc::new(Cell::new(None::<f32>));
                let travel = Rc::new(Cell::new(0.0_f32));
                let l = latched.clone();
                let t = travel.clone();
                let mut theme = crate::presets::intui::light();
                theme.input = teksilo_tokens::InputTokens::for_density(density);
                let mut tree = WidgetTree::new().with_theme(theme);
                let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
                tree.add(
                    StackWidget::new()
                        .add_child(child)
                        .touch_action(action)
                        .on_drag(move |phase, _c| {
                            if matches!(phase, DragPhase::Started { .. }) && l.get().is_none() {
                                l.set(Some(t.get()));
                            }
                        }),
                );
                tree.layout(SizeProposal::exact(200.0, 50.0));

                press(&mut tree, Point::new(20.0, 25.0));
                for step in 1..=10 {
                    travel.set(step as f32);
                    moved(&mut tree, Point::new(20.0 + step as f32, 25.0));
                    if latched.get().is_some() {
                        break;
                    }
                }
                let at = latched
                    .get()
                    .unwrap_or_else(|| panic!("no drag latched at {density:?} under {action:?}"));
                assert_eq!(
                    at, 5.0,
                    "a mouse latched after {at} dp of travel under {action:?} at \
                     {density:?}; the mouse drag slop is 5.0 and must stay 5.0"
                );
                release(&mut tree, Point::new(30.0, 25.0));
            }
        }
    }

    /// `slop_precise` reaches only a **direct** pointer under a frozen
    /// `TouchAction::NONE`.
    #[test]
    fn slop_precise_is_direct_pointer_only() {
        use crate::gesture::PointerSequence;

        let tokens = teksilo_tokens::InputTokens::for_density(TargetDensity::Compact);
        let path = Vec::new();
        let mouse = PointerSequence::new(
            PointerInfo::mouse(EventTime::ZERO),
            path.clone(),
            TouchAction::NONE,
            None,
            Point::ZERO,
            EventTime::ZERO,
        );
        assert_eq!(
            mouse.latch_slop(tokens.profile(PointerKind::Mouse)),
            5.0,
            "a mouse under a frozen NONE still latches at 5.0"
        );

        let contact = PointerSequence::new(
            PointerInfo::touch(new_contact(), EventTime::ZERO),
            path,
            TouchAction::NONE,
            None,
            Point::ZERO,
            EventTime::ZERO,
        );
        let touch_profile = tokens.profile(PointerKind::Touch);
        assert_eq!(
            contact.latch_slop(touch_profile),
            touch_profile.slop_precise,
            "a contact under a frozen NONE drops to the jitter floor"
        );
    }

    /// The raw-preview pass runs root-first, and the first `Handled` claims the
    /// press. A nested pair of previewers pins the order.
    #[test]
    fn nested_previewers_run_root_first() {
        let order = Rc::new(std::cell::RefCell::new(Vec::<&'static str>::new()));
        let outer_order = order.clone();
        let inner_order = order.clone();

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new());
        let inner = tree.add(StackWidget::new().add_child(leaf).on_pointer_event(
            move |event, _ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    inner_order.borrow_mut().push("inner");
                }
                EventResponse::Ignored
            },
        ));
        let outer = tree.add(StackWidget::new().add_child(inner).on_pointer_event(
            move |event, _ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    outer_order.borrow_mut().push("outer");
                    // The outer previewer claims: the inner one must never run.
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            },
        ));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        press(&mut tree, Point::new(50.0, 25.0));
        assert_eq!(
            *order.borrow(),
            vec!["outer"],
            "the preview pass is root-first and the first Handled wins"
        );
        assert_eq!(
            tree.sequence_winner(PointerId::MOUSE),
            Some(outer),
            "and that claim decides the sequence"
        );
        assert_eq!(
            tree.sequence_members(PointerId::MOUSE),
            vec![(outer, MemberRole::RawPreview, MemberState::Won)],
        );
        release(&mut tree, Point::new(50.0, 25.0));
    }

    /// An explicit `capture_pointer()` from an undecided sequence enrols the
    /// caller as a `RawDrag` member and, for a precise pointer, decides the
    /// sequence there and then — the Splitter / dock-handle / column-grip
    /// shape, which owns no recognizer at all.
    #[test]
    fn explicit_capture_enrols_a_raw_drag_and_decides_a_precise_pointer() {
        let dragged = Rc::new(Cell::new(false));
        let d = dragged.clone();

        let mut tree = WidgetTree::new();
        // The handle: captures on Down, answers Ignored, works from Move — the
        // Splitter / dock-handle / column-grip shape. Like the real Splitter it
        // also carries a double-tap handler, so it owns an arena and the press
        // stops bubbling at it.
        let handle = tree.add(
            FillWidget::new()
                .on_double_tap(|_e, _c| {})
                .on_pointer_event(|event, ctx| {
                    if matches!(event, WidgetEvent::PointerDown { .. }) {
                        ctx.capture_pointer();
                    }
                    EventResponse::Ignored
                }),
        );
        let ancestor = tree.add(
            StackWidget::new()
                .add_child(handle)
                .on_drag(move |phase, _c| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        d.set(true);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 50.0));

        press(&mut tree, Point::new(20.0, 25.0));
        assert_eq!(
            tree.sequence_winner(PointerId::MOUSE),
            Some(handle),
            "the explicit captor owns the press immediately"
        );
        let members = tree.sequence_members(PointerId::MOUSE);
        assert_eq!(members[0], (handle, MemberRole::RawDrag, MemberState::Won));
        assert_eq!(
            members[1],
            (ancestor, MemberRole::Gesture, MemberState::Rejected),
            "the ancestor is still enrolled — as a rejected competitor, which is \
             what keeps its recognizers out of the rest of the press"
        );

        // Drag well past the ancestor's threshold: it must not steal the press.
        for step in 1..=20 {
            moved(&mut tree, Point::new(20.0 + step as f32 * 4.0, 25.0));
        }
        assert!(
            !dragged.get(),
            "a decided sequence keeps the ancestor out of the running"
        );
        release(&mut tree, Point::new(100.0, 25.0));
    }

    /// A touch `RawDrag` does **not** decide at press: it defers to
    /// `drag_slop` (18) and still beats a pan claimant, whose `pan_slop` is 36.
    #[test]
    fn a_touch_raw_drag_defers_to_drag_slop_and_still_beats_a_pan() {
        let mut tree = WidgetTree::new();
        let handle = tree.add(FillWidget::new().on_pointer_event(|event, ctx| {
            if matches!(event, WidgetEvent::PointerDown { .. }) {
                ctx.capture_pointer();
            }
            EventResponse::Ignored
        }));
        let scroller = tree.add(
            StackWidget::new()
                .add_child(handle)
                .pan_claim(PanClaim::vertical()),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));

        let id = new_contact();
        let at = Point::new(100.0, 20.0);
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert_eq!(
            tree.sequence_winner(id),
            None,
            "a contact's explicit capture does NOT decide at press"
        );
        let members = tree.sequence_members(id);
        assert!(
            members
                .iter()
                .any(|(id, role, _)| *id == handle && *role == MemberRole::RawDrag),
            "the captor competes as a RawDrag: {members:?}"
        );
        assert!(
            members
                .iter()
                .any(|(id, role, _)| *id == scroller && matches!(role, MemberRole::Pan(_))),
            "and the scroller competes as a Pan: {members:?}"
        );

        // 17 dp: below the 18 dp drag slop, so nobody has won.
        tree.dispatch_pointer(touch(id, PointerPhase::Move, Point::new(100.0, 37.0), 20));
        assert_eq!(tree.sequence_winner(id), None, "17 dp is below drag_slop");

        // 19 dp: past drag_slop (18) but well below pan_slop (36) — the
        // innermost RawDrag wins.
        tree.dispatch_pointer(touch(id, PointerPhase::Move, Point::new(100.0, 39.0), 30));
        assert_eq!(
            tree.sequence_winner(id),
            Some(handle),
            "the RawDrag latches at 18 and the pan claimant needs 36"
        );
        tree.dispatch_pointer(touch(id, PointerPhase::Up, Point::new(100.0, 39.0), 40));
    }

    /// `DragActivation::AfterLongPress` cannot win before its timer, and
    /// self-rejects once the press leaves the tap boundary.
    #[test]
    fn after_long_press_defers_and_self_rejects_past_the_tap_boundary() {
        let mut tree = WidgetTree::new();
        let row = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let list = tree.add(
            StackWidget::new()
                .add_child(row)
                .drag_activation(DragActivation::AfterLongPress)
                .on_drag(|_phase, _c| {}),
        );
        tree.layout(SizeProposal::exact(200.0, 50.0));

        press(&mut tree, Point::new(20.0, 25.0));
        let member = tree
            .sequence_members(PointerId::MOUSE)
            .into_iter()
            .find(|(id, ..)| *id == list)
            .expect("the list competes");
        assert_eq!(member.1, MemberRole::Gesture);
        assert_eq!(member.2, MemberState::Possible);

        // Travelling past the tap boundary before the timer withdraws it: that
        // travel is a pan, not a considered grab.
        moved(&mut tree, Point::new(60.0, 25.0));
        assert_eq!(
            tree.sequence_winner(PointerId::MOUSE),
            None,
            "a deferred drag cannot win before its long-press timer"
        );
        let member = tree
            .sequence_members(PointerId::MOUSE)
            .into_iter()
            .find(|(id, ..)| *id == list)
            .expect("the list is still listed");
        assert_eq!(
            member.2,
            MemberState::Rejected,
            "and self-rejects once the press leaves the tap boundary"
        );
        release(&mut tree, Point::new(60.0, 25.0));
    }

    /// Losing the **captor** cancels the whole sequence: the press belongs to
    /// the node that took it, and there is nothing left to arbitrate for.
    ///
    /// Per-member death is the unit-level half of the same rule and is pinned
    /// in `gesture::sequence`'s own tests — in a tree a member is always an
    /// ancestor of the captor, so it cannot die on its own.
    #[test]
    fn losing_the_captor_cancels_the_sequence() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        let mid = tree.add(StackWidget::new().add_child(child).on_drag(|_phase, _c| {}));
        let outer = tree.add(StackWidget::new().add_child(mid).on_drag(|_phase, _c| {}));
        tree.layout(SizeProposal::exact(200.0, 50.0));

        press(&mut tree, Point::new(20.0, 25.0));
        let members: Vec<_> = tree
            .sequence_members(PointerId::MOUSE)
            .into_iter()
            .map(|(id, ..)| id)
            .collect();
        assert_eq!(members, vec![mid, outer], "innermost first");

        tree.arena.destroy(child);
        moved(&mut tree, Point::new(21.0, 25.0));
        assert!(
            tree.sequence_members(PointerId::MOUSE).is_empty(),
            "losing the captor cancels the sequence"
        );
        assert_eq!(tree.sequence_winner(PointerId::MOUSE), None);
    }

    /// `hold_gesture` blocks every peer, and auto-releases at
    /// `profile.max_hold` (250 ms) — the framework never trusts a holder to
    /// answer.
    #[test]
    fn a_hold_blocks_peers_and_auto_releases_at_max_hold() {
        use crate::pointer::clock::ManualClock;

        let dragged = Rc::new(Cell::new(false));
        let d = dragged.clone();

        let mut tree = WidgetTree::new();
        let clock = Rc::new(ManualClock::new(EventTime::ZERO));
        tree.set_input_clock(clock.clone());

        // The child holds the sequence on the press, then never answers.
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}).on_pointer_event(
            |event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    ctx.hold_gesture();
                }
                EventResponse::Ignored
            },
        ));
        tree.add(
            StackWidget::new()
                .add_child(child)
                .on_drag(move |phase, _c| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        d.set(true);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(300.0, 50.0));

        press(&mut tree, Point::new(20.0, 25.0));
        assert!(
            tree.sequence_members(PointerId::MOUSE)
                .iter()
                .any(|(id, _, state)| *id == child && *state == MemberState::Held),
            "the holder is listed as Held"
        );

        // Well past the ancestor's 5 dp threshold, but nothing may win.
        clock.set(EventTime::from_millis(100));
        moved(&mut tree, Point::new(80.0, 25.0));
        assert!(!dragged.get(), "no peer wins while a member is holding");

        // 250 ms: the hold expires and the ancestor is free to latch.
        clock.set(EventTime::from_millis(250));
        moved(&mut tree, Point::new(90.0, 25.0));
        assert!(
            dragged.get(),
            "the hold auto-releases at max_hold and the peer latches"
        );
    }

    /// `claim_gesture` / `reject_gesture` are the explicit forms of the same
    /// decision.
    #[test]
    fn claim_and_reject_are_arbitration_acts() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}).on_pointer_event(
            |event, ctx| {
                if matches!(event, WidgetEvent::PointerDown { .. }) {
                    ctx.claim_gesture();
                }
                EventResponse::Ignored
            },
        ));
        let ancestor = tree.add(StackWidget::new().add_child(child).on_drag(|_phase, _c| {}));
        tree.layout(SizeProposal::exact(200.0, 50.0));

        press(&mut tree, Point::new(20.0, 25.0));
        assert_eq!(tree.sequence_winner(PointerId::MOUSE), Some(child));
        let ancestor_state = tree
            .sequence_members(PointerId::MOUSE)
            .into_iter()
            .find(|(id, ..)| *id == ancestor)
            .map(|(_, _, state)| state);
        assert_eq!(
            ancestor_state,
            Some(MemberState::Rejected),
            "a claim knocks every peer out exactly once"
        );
        release(&mut tree, Point::new(20.0, 25.0));
    }

    /// With the touch kill switch off, a direct-pointer sample forms no
    /// sequence at all — while a mouse on the same tree still does.
    #[test]
    fn touch_disabled_forms_no_sequence() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().on_tap(|_e, _c| {}));
        tree.add(StackWidget::new().add_child(child).on_drag(|_phase, _c| {}));
        tree.layout(SizeProposal::exact(200.0, 50.0));
        tree.set_touch_enabled(false);

        let id = new_contact();
        let at = Point::new(20.0, 25.0);
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert!(
            tree.sequence_members(id).is_empty(),
            "the kill switch means a contact arbitrates nothing"
        );
        assert_eq!(tree.sequence_winner(id), None);
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 10));

        press(&mut tree, at);
        assert!(
            !tree.sequence_members(PointerId::MOUSE).is_empty(),
            "the mouse is unaffected by the touch kill switch"
        );
        release(&mut tree, at);
    }

    /// One [`TapBoundary`](crate::gesture::TapBoundary) predicate, two answers:
    /// a precise pointer keeps its `tap_slop` radius, and a coarse one keeps
    /// its target's **bounds** — a finger's reported centre wanders while
    /// resting on a control, and a tap that never left its target is a tap.
    #[test]
    fn a_coarse_tap_survives_inside_its_bounds_where_a_mouse_would_not() {
        let taps = Rc::new(Cell::new(0));
        let t = taps.clone();

        let mut tree = WidgetTree::new();
        let row = tree.add(FillWidget::new().on_tap(move |_e, _c| t.set(t.get() + 1)));
        tree.layout(SizeProposal::exact(200.0, 60.0));

        // 30 dp of travel: past the touch profile's 18 dp tap_slop, but well
        // inside the 200 x 60 row.
        let down = Point::new(40.0, 30.0);
        let up = Point::new(70.0, 30.0);

        let id = new_contact();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, down, 0));
        tree.dispatch_pointer(touch(id, PointerPhase::Move, up, 10));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, up, 20));
        assert_eq!(taps.get(), 1, "a finger that stayed on the row tapped it");

        // The same travel on a mouse is 30 dp past a 5 dp radius: not a tap.
        taps.set(0);
        press(&mut tree, down);
        moved(&mut tree, up);
        release(&mut tree, up);
        assert_eq!(taps.get(), 0, "a mouse keeps its tap_slop radius");

        // And a finger that leaves the row is not a tap either.
        taps.set(0);
        let id = new_contact();
        let out = Point::new(40.0, 200.0);
        tree.dispatch_pointer(touch(id, PointerPhase::Down, down, 30));
        tree.dispatch_pointer(touch(id, PointerPhase::Move, out, 40));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, out, 50));
        assert_eq!(taps.get(), 0, "sliding off the row abandons the activation");
        let _ = row;
    }

    /// Sliding off a control revokes its **tap family** once — WCAG 2.2
    /// SC 2.5.2's "slide off to abort" — while a drag the same press started
    /// runs on.
    #[test]
    fn leaving_the_tap_boundary_revokes_the_tap_family_but_not_the_drag() {
        let tapped = Rc::new(Cell::new(false));
        let dragged = Rc::new(Cell::new(0));
        let ta = tapped.clone();
        let dr = dragged.clone();

        let mut tree = WidgetTree::new();
        let control = tree.add(
            FillWidget::new()
                .on_tap(move |_e, _c| ta.set(true))
                .on_drag(move |phase, _c| {
                    if matches!(phase, DragPhase::Started { .. }) {
                        dr.set(dr.get() + 1);
                    }
                }),
        );
        tree.layout(SizeProposal::exact(200.0, 60.0));

        press(&mut tree, Point::new(40.0, 30.0));
        moved(&mut tree, Point::new(120.0, 30.0));
        release(&mut tree, Point::new(120.0, 30.0));

        assert!(!tapped.get(), "the activation is abandoned");
        assert_eq!(dragged.get(), 1, "the drag the same press started is not");
        let _ = control;
    }

    /// The frozen `TouchAction` is readable from inside the press handler, and
    /// is the intersection of the whole root-to-target chain.
    #[test]
    fn the_frozen_touch_action_reaches_the_press_handler() {
        let seen = Rc::new(Cell::new(TouchAction::AUTO));
        let s = seen.clone();

        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().on_pointer_event(move |event, ctx| {
            if matches!(event, WidgetEvent::PointerDown { .. }) {
                s.set(ctx.touch_action());
            }
            EventResponse::Ignored
        }));
        let inner = tree.add(
            StackWidget::new()
                .add_child(leaf)
                .touch_action(TouchAction::PAN),
        );
        tree.add(
            StackWidget::new()
                .add_child(inner)
                .touch_action(TouchAction::PAN_Y),
        );
        tree.layout(SizeProposal::exact(200.0, 50.0));

        let id = new_contact();
        let at = Point::new(20.0, 25.0);
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert_eq!(
            seen.get(),
            TouchAction::PAN_Y,
            "PAN ∩ PAN_Y = PAN_Y, frozen at press and readable from the handler"
        );
        assert_eq!(tree.sequence_touch_action(id), TouchAction::PAN_Y);
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 10));
    }
}
