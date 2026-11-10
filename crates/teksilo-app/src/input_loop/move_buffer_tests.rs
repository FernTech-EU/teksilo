// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`MoveBuffer`] and the release rule the loop applies around it.
//!
//! The rule itself lives in `TeksiloAppHandler` (`window_event` releases the
//! held move before every event that is not another move from the same
//! window and device, `user_event` and `about_to_wait` release it first
//! thing). [`Turn`] applies the same rule to one window here, over the same
//! functions the loop calls: [`crate::input_routing::translate`],
//! [`MoveBuffer::offer`], [`MoveBuffer::take`],
//! [`dispatch_released_moves`] and [`dispatch_input`], so what is asserted
//! is what those bodies do in that order. `app::winit_loop_tests` witnesses
//! the calls in the real callbacks.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, SizeProposal};
use teksilo_core::event::{EventResponse, Key, Modifiers, WidgetEvent};
use teksilo_core::pointer::{EventTime, PointerPhase, PointerSample};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::window::NoopWindowOps;
use teksilo_core::{CursorIcon, WidgetTree};
use teksilo_platform::TranslationState;
use teksilo_platform::pointer_backend::InputSample;
use teksilo_platform::window_system::WindowSystem;
use winit::event::{DeviceId, ElementState, MouseScrollDelta, TouchPhase, WindowEvent};

use super::*;
use crate::input_routing::tests::{backend, click, cursor, touch};

/// What a window asked of its chrome: only the two calls the move path makes
/// are recorded; the rest answer a desktop window's defaults.
#[derive(Default)]
struct Chrome {
    cursors: Vec<CursorIcon>,
    redraws: Vec<&'static str>,
}

impl InputChrome for Chrome {
    fn set_cursor(&mut self, icon: CursorIcon) {
        self.cursors.push(icon);
    }
    fn request_redraw(&mut self, reason: &'static str) {
        self.redraws.push(reason);
    }
    fn safe_area(&self) -> SafeAreaSides {
        SafeAreaSides::ZERO
    }
    fn occluded_band(&self, _screen: (i32, i32, i32, i32)) -> Option<Rect> {
        None
    }
    fn set_soft_keyboard_visible(&mut self, _visible: bool) {}
    fn set_ime(&mut self, _purpose: Option<teksilo_core::ImePurpose>, _allowed: Option<bool>) {}
    fn ime_host(&self) -> ImeHost {
        ImeHost::Other
    }
    fn text_store_mode(&self) -> TextStoreMode {
        TextStoreMode::Off
    }
    fn set_text_store_focus(&mut self, _focus: StoreFocus) {}
    fn terminate_text_store_composition(&mut self) -> bool {
        false
    }
}

/// What reached the widget, in order.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    /// A move: where, and the positions batched behind it, oldest first.
    Move(Point, Vec<Point>),
    Down(Point),
    Up(Point),
    Key,
    Scroll,
    Enter,
    Leave,
}

/// A move as dispatched, with the times its batch carried.
#[derive(Debug, Clone)]
struct Timed {
    kind: teksilo_tokens::PointerKind,
    time: EventTime,
    batched: Vec<EventTime>,
}

#[derive(Default)]
struct Log {
    seen: Vec<Seen>,
    timed: Vec<Timed>,
}

type Shared = Rc<RefCell<Log>>;

/// A 400 × 300 tree whose one focused, filling widget records what reaches
/// it. The widget sits at the origin, so its local positions are window
/// positions and the batched ones can be compared with them directly.
fn recording_tree() -> (WidgetTree, Shared) {
    let log: Shared = Rc::new(RefCell::new(Log::default()));
    let (a, b, c, d) = (log.clone(), log.clone(), log.clone(), log.clone());
    let mut tree = WidgetTree::new();
    let leaf = tree.add(
        teksilo_widgets::primitives::RectWidget::new()
            .focusable(true)
            .on_pointer_event(move |event, ctx| {
                let entry = match event {
                    WidgetEvent::PointerMove { position, .. } => {
                        a.borrow_mut().timed.push(Timed {
                            kind: ctx.pointer().kind,
                            time: ctx.pointer().time,
                            batched: ctx.coalesced().iter().map(|c| c.time).collect(),
                        });
                        Some(Seen::Move(
                            *position,
                            ctx.coalesced().iter().map(|c| c.window_position).collect(),
                        ))
                    }
                    WidgetEvent::PointerDown { position, .. } => Some(Seen::Down(*position)),
                    WidgetEvent::PointerUp { position, .. } => Some(Seen::Up(*position)),
                    _ => None,
                };
                if let Some(entry) = entry {
                    a.borrow_mut().seen.push(entry);
                }
                EventResponse::Ignored
            })
            .on_key(move |event, _ctx| {
                if matches!(event, WidgetEvent::KeyDown { .. }) {
                    b.borrow_mut().seen.push(Seen::Key);
                }
                EventResponse::Ignored
            })
            .on_scroll(move |_event, _ctx| {
                c.borrow_mut().seen.push(Seen::Scroll);
                EventResponse::Ignored
            })
            .on_hover(move |entered, _ctx| {
                d.borrow_mut()
                    .seen
                    .push(if entered { Seen::Enter } else { Seen::Leave });
            }),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.focus(leaf);
    (tree, log)
}

/// One input to a [`Turn`]: a winit event, or a key, which winit gives no
/// way to build (`KeyEvent` carries a private platform field) and which the
/// loop treats as it treats every event that is not a move.
enum Input {
    Winit(WindowEvent),
    Key,
}

/// One window's share of an event loop, applying the loop's release rule.
struct Turn {
    tree: WidgetTree,
    backend: TranslationState,
    chrome: Chrome,
    buffer: MoveBuffer<u8>,
}

impl Turn {
    fn new(tree: WidgetTree, backend: TranslationState) -> Self {
        Self {
            tree,
            backend,
            chrome: Chrome::default(),
            buffer: MoveBuffer::new(),
        }
    }

    /// The window and device every event here comes from.
    const KEY: u8 = 1;

    /// One callback: release the held move unless this is the next move,
    /// then hold the move or dispatch the event, as the loop does.
    fn feed(&mut self, input: Input) {
        // A millisecond between events, so every sample has a time of its own
        // and the batch's times can be checked for order.
        self.tree.advance_time(Duration::from_millis(1));
        if let Input::Winit(event) = &input
            && dropped_on_arrival(event)
        {
            return;
        }
        let is_move = matches!(input, Input::Winit(WindowEvent::CursorMoved { .. }));
        if !(is_move && self.buffer.holds_for(Self::KEY)) {
            self.release();
        }
        let mut ops = NoopWindowOps;
        match input {
            Input::Winit(event) if is_move => {
                let samples =
                    crate::input_routing::translate(&mut self.backend, &self.tree, &event);
                let released = self.buffer.offer(Self::KEY, samples);
                dispatch_released_moves(
                    &mut self.chrome,
                    &mut self.tree,
                    &mut ops,
                    released.into_iter().map(|(_, sample)| sample).collect(),
                );
            }
            Input::Winit(event) => {
                dispatch_input(
                    &mut self.chrome,
                    &mut self.backend,
                    &mut self.tree,
                    &mut ops,
                    &event,
                );
            }
            Input::Key => self.tree.press_key(Key::A, Modifiers::NONE),
        }
    }

    /// What `about_to_wait` does first.
    fn release(&mut self) {
        if let Some((_, sample)) = self.buffer.take() {
            dispatch_released_moves(
                &mut self.chrome,
                &mut self.tree,
                &mut NoopWindowOps,
                vec![InputSample::Pointer(sample)],
            );
        }
    }

    fn moves(&mut self, points: &[(f64, f64)]) {
        for &(x, y) in points {
            self.feed(Input::Winit(cursor(x, y)));
        }
    }
}

fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn wheel() -> WindowEvent {
    WindowEvent::MouseWheel {
        device_id: DeviceId::dummy(),
        delta: MouseScrollDelta::LineDelta(0.0, -1.0),
        phase: TouchPhase::Moved,
    }
}

/// A plain mouse move at `x`, stamped `ms`.
fn mouse_move(x: f32, ms: u64) -> PointerSample {
    PointerSample::mouse(PointerPhase::Move, p(x, 0.0), EventTime::from_millis(ms))
}

/// The positions a released dispatch carries: its own last.
fn positions(released: &[(u8, InputSample)]) -> Vec<(u8, Point, usize)> {
    released
        .iter()
        .map(|(key, sample)| match sample {
            InputSample::Pointer(p) => (*key, p.position, p.coalesced.len()),
            other => panic!("only pointer samples are offered here: {other:?}"),
        })
        .collect()
}

// -- the burst ----------------------------------------------------------------

/// A burst of moves reaches the widget as **one** `PointerMove`, at the last
/// position, with every earlier position batched behind it in the order the
/// device produced them and each with its own time.
#[test]
fn a_burst_of_moves_is_one_dispatch_with_the_rest_batched_in_order() {
    let (tree, log) = recording_tree();
    let mut turn = Turn::new(tree, backend());
    turn.moves(&[(10.0, 10.0), (20.0, 12.0), (30.0, 14.0), (40.0, 16.0)]);
    assert!(
        log.borrow().seen.is_empty(),
        "nothing reaches the widget while the turn is still delivering moves"
    );
    turn.release();

    let log = log.borrow();
    let moves: Vec<&Seen> = log
        .seen
        .iter()
        .filter(|s| matches!(s, Seen::Move(..)))
        .collect();
    assert_eq!(
        moves,
        vec![&Seen::Move(
            p(40.0, 16.0),
            vec![p(10.0, 10.0), p(20.0, 12.0), p(30.0, 14.0)]
        )],
        "one dispatch, at the newest position, the rest batched oldest first"
    );
    let timed = &log.timed[0];
    assert_eq!(timed.batched.len(), 3);
    assert!(
        timed.batched.windows(2).all(|w| w[0] < w[1]) && timed.batched[2] < timed.time,
        "each batched position keeps the time it arrived at: {timed:?}"
    );
    assert_eq!(
        turn.chrome.cursors.len(),
        1,
        "the cursor is pushed once per dispatch, not once per move"
    );
    assert!(
        turn.chrome.redraws.iter().all(|r| *r == "cursor") && turn.chrome.redraws.len() <= 1,
        "a move asks for a frame only as a `CursorMoved` does, once: {:?}",
        turn.chrome.redraws
    );
}

/// One valuator of an X11 motion, as winit 0.30 reports it after the motion's
/// `CursorMoved` (`xinput2_mouse_motion`).
fn axis(axis: u32, value: f64) -> WindowEvent {
    WindowEvent::AxisMotion {
        device_id: DeviceId::dummy(),
        axis,
        value,
    }
}

/// X11 moves as winit delivers them: each `CursorMoved` followed by one
/// `AxisMotion` for x and one for y. The axes are dropped on arrival, so they
/// neither release the held move nor reach the tree, and the burst folds as
/// it does on Wayland. Releasing on them gives three dispatches.
#[test]
fn x11_axis_motion_between_moves_does_not_break_the_burst() {
    let (tree, log) = recording_tree();
    let mut turn = Turn::new(tree, backend());
    for (x, y) in [(10.0, 10.0), (20.0, 12.0), (30.0, 14.0)] {
        turn.feed(Input::Winit(cursor(x, y)));
        turn.feed(Input::Winit(axis(0, x)));
        turn.feed(Input::Winit(axis(1, y)));
    }
    assert!(
        log.borrow().seen.is_empty(),
        "the axes after a move release nothing: {:?}",
        log.borrow().seen
    );
    turn.release();
    let log = log.borrow();
    let moves: Vec<&Seen> = log
        .seen
        .iter()
        .filter(|s| matches!(s, Seen::Move(..)))
        .collect();
    assert_eq!(
        moves,
        vec![&Seen::Move(
            p(30.0, 14.0),
            vec![p(10.0, 10.0), p(20.0, 12.0)]
        )],
        "one dispatch for the three moves"
    );
    for kept in [cursor(1.0, 1.0), wheel(), click(ElementState::Pressed)] {
        assert!(
            !dropped_on_arrival(&kept),
            "only the axes are dropped, not {kept:?}"
        );
    }
}

/// The batch reaches a velocity tracker whole: fed through
/// `VelocityTracker::add_coalesced` the way the pan arbiter feeds it, the one
/// dispatch gives the estimate the separate moves gave.
#[test]
fn a_folded_burst_gives_a_tracker_every_sample() {
    use teksilo_core::VelocityTracker;

    let mut buffer = MoveBuffer::new();
    let mut separate = VelocityTracker::new();
    for (i, x) in [0.0_f32, 4.0, 9.0, 15.0, 22.0, 30.0]
        .into_iter()
        .enumerate()
    {
        let sample = mouse_move(x, 4 * i as u64);
        separate.add(sample.pointer.time, sample.position);
        assert!(
            buffer
                .offer(1_u8, vec![InputSample::Pointer(sample)])
                .is_empty()
        );
    }
    let (_, folded) = buffer.take().expect("the burst is held");
    let mut batched = VelocityTracker::new();
    batched.add_coalesced(folded.coalesced.iter().map(|c| (c.time, c.window_position)));
    batched.add(folded.pointer.time, folded.position);
    let (batched, separate) = (batched.estimate(), separate.estimate());
    assert!(separate.is_some(), "six samples are enough for a fit");
    assert_eq!(batched, separate);
}

// -- what never folds ---------------------------------------------------------

/// Moves fold only with a move from the same key: another window or another
/// device releases the held move first, in order.
#[test]
fn a_move_from_another_window_or_device_releases_the_held_one() {
    let mut buffer: MoveBuffer<(u8, u8)> = MoveBuffer::new();
    let offer = |buffer: &mut MoveBuffer<(u8, u8)>, key, x, ms| {
        buffer.offer(key, vec![InputSample::Pointer(mouse_move(x, ms))])
    };
    assert!(offer(&mut buffer, (1, 1), 10.0, 1).is_empty());
    assert!(offer(&mut buffer, (1, 1), 20.0, 2).is_empty());

    let released = offer(&mut buffer, (2, 1), 30.0, 3);
    let released: Vec<_> = released
        .into_iter()
        .map(|(key, sample)| match sample {
            InputSample::Pointer(p) => (key, p.position, p.coalesced.len()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        released,
        vec![((1, 1), p(20.0, 0.0), 1)],
        "another window: the held move goes first, with its own batch"
    );

    let released = offer(&mut buffer, (2, 2), 40.0, 4);
    assert_eq!(released.len(), 1, "another device: the same");
    assert!(buffer.holds_for((2, 2)));
    assert!(!buffer.holds_for((2, 1)));
}

/// A held move and a move holding other buttons, under other modifiers or
/// from another pointer do not fold, even from one key.
#[test]
fn a_button_modifier_or_pointer_change_never_folds() {
    let held = || mouse_move(10.0, 1);
    let mut pressed = mouse_move(20.0, 2);
    pressed.pointer.buttons = teksilo_core::event::ButtonMask::PRIMARY;
    let shifted = mouse_move(20.0, 2).with_modifiers(Modifiers::SHIFT);
    let mut other_pointer = mouse_move(20.0, 2);
    other_pointer.pointer.id = teksilo_core::pointer::PointerIdAllocator::global()
        .begin(teksilo_core::pointer::BackendDeviceKey::DEFAULT, 0x4d42);

    for next in [pressed, shifted, other_pointer] {
        let mut buffer = MoveBuffer::new();
        assert!(
            buffer
                .offer(1_u8, vec![InputSample::Pointer(held())])
                .is_empty()
        );
        let released = buffer.offer(1_u8, vec![InputSample::Pointer(next.clone())]);
        assert_eq!(
            positions(&released),
            vec![(1, p(10.0, 0.0), 0)],
            "the held move is released whole, not folded into {next:?}"
        );
        let (_, kept) = buffer.take().expect("the new move is held");
        assert!(kept.coalesced.is_empty());
    }
}

/// Anything but a lone pure move releases the held move and comes back behind
/// it: no sample is lost, none overtakes another.
#[test]
fn a_sample_that_is_not_a_move_comes_back_behind_the_held_one() {
    let mut buffer = MoveBuffer::new();
    assert!(
        buffer
            .offer(1_u8, vec![InputSample::Pointer(mouse_move(10.0, 1))])
            .is_empty()
    );
    let down = PointerSample::mouse(PointerPhase::Down, p(10.0, 0.0), EventTime::from_millis(2))
        .with_button(teksilo_core::event::PointerButton::Primary);
    let released = buffer.offer(1_u8, vec![InputSample::Pointer(down)]);
    let phases: Vec<PointerPhase> = released
        .iter()
        .map(|(_, sample)| match sample {
            InputSample::Pointer(p) => p.phase,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(phases, vec![PointerPhase::Move, PointerPhase::Down]);
    assert!(buffer.take().is_none(), "and nothing stays held");
}

/// A move the translator dropped (X11's emulated pointer) is no sample, so
/// the held move stays held: there is nothing to order it against.
#[test]
fn a_suppressed_move_leaves_the_held_one_alone() {
    let mut buffer = MoveBuffer::new();
    assert!(
        buffer
            .offer(1_u8, vec![InputSample::Pointer(mouse_move(10.0, 1))])
            .is_empty()
    );
    assert!(buffer.offer(1_u8, Vec::new()).is_empty());
    let (_, held) = buffer.take().expect("still held");
    assert_eq!(held.position, p(10.0, 0.0));
}

// -- order around everything else ----------------------------------------------

/// Moves around a press, a release, a key, a wheel notch and a leave and
/// enter keep their order and their positions: each transition finds the
/// pointer where the last move before it left it, and the moves between two
/// transitions are one dispatch.
#[test]
fn moves_around_transitions_keep_their_order_and_positions() {
    let (tree, log) = recording_tree();
    let mut turn = Turn::new(tree, backend());

    turn.moves(&[(10.0, 10.0), (20.0, 10.0)]);
    turn.feed(Input::Winit(click(ElementState::Pressed)));
    turn.moves(&[(30.0, 10.0), (40.0, 10.0)]);
    turn.feed(Input::Winit(click(ElementState::Released)));
    turn.moves(&[(50.0, 10.0), (60.0, 10.0)]);
    turn.feed(Input::Key);
    turn.moves(&[(70.0, 10.0)]);
    turn.feed(Input::Winit(wheel()));
    turn.moves(&[(80.0, 10.0), (90.0, 10.0)]);
    turn.feed(Input::Winit(WindowEvent::CursorLeft {
        device_id: DeviceId::dummy(),
    }));
    turn.feed(Input::Winit(WindowEvent::CursorEntered {
        device_id: DeviceId::dummy(),
    }));
    turn.moves(&[(100.0, 10.0), (110.0, 10.0)]);
    turn.release();

    let seen = log.borrow().seen.clone();
    assert_eq!(
        seen,
        vec![
            Seen::Enter,
            Seen::Move(p(20.0, 10.0), vec![p(10.0, 10.0)]),
            Seen::Down(p(20.0, 10.0)),
            Seen::Move(p(40.0, 10.0), vec![p(30.0, 10.0)]),
            Seen::Up(p(40.0, 10.0)),
            Seen::Move(p(60.0, 10.0), vec![p(50.0, 10.0)]),
            Seen::Key,
            Seen::Move(p(70.0, 10.0), vec![]),
            Seen::Scroll,
            Seen::Move(p(90.0, 10.0), vec![p(80.0, 10.0)]),
            Seen::Leave,
            Seen::Enter,
            Seen::Move(p(110.0, 10.0), vec![p(100.0, 10.0)]),
        ]
    );
}

/// On X11 a live contact makes every `CursorMoved` the emulated pointer, and
/// the translator drops it as it arrives: held or not, none reaches the
/// widget as a mouse. The move held before the contact is released by the
/// touch packet, ahead of it.
#[test]
fn x11_phantom_motion_is_still_dropped_while_moves_are_held() {
    let (tree, log) = recording_tree();
    let mut x11 = TranslationState::new();
    x11.set_window_system(WindowSystem::X11);
    let mut turn = Turn::new(tree, x11);

    turn.moves(&[(10.0, 10.0), (20.0, 10.0)]);
    turn.feed(Input::Winit(touch(TouchPhase::Started, 4, 100.0, 100.0)));
    turn.moves(&[(100.0, 100.0), (110.0, 104.0)]);
    turn.feed(Input::Winit(touch(TouchPhase::Moved, 4, 110.0, 104.0)));
    turn.moves(&[(110.0, 104.0)]);
    turn.feed(Input::Winit(touch(TouchPhase::Ended, 4, 110.0, 104.0)));
    turn.release();

    let log = log.borrow();
    assert_eq!(
        log.seen.iter().find(|s| matches!(s, Seen::Move(..))),
        Some(&Seen::Move(p(20.0, 10.0), vec![p(10.0, 10.0)])),
        "the mouse's own burst, released by the contact"
    );
    let kinds: Vec<_> = log.timed.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            teksilo_tokens::PointerKind::Mouse,
            teksilo_tokens::PointerKind::Touch
        ],
        "after the mouse's burst only the contact's own move reached the \
         widget: no emulated cursor position did, held or not"
    );
}

// -- what a burst ends at -------------------------------------------------------

/// A drag fed by a burst of moves hovers, once the turn releases the burst,
/// where the burst ended, and drops there.
#[test]
fn a_drag_fed_by_a_burst_drops_at_the_last_position() {
    use teksilo_widgets::primitives::{FixedSize, HStack, RectWidget};

    let dropped: Rc<RefCell<Vec<Point>>> = Rc::default();
    let hovered: Rc<RefCell<Vec<Point>>> = Rc::default();
    let source_id: Rc<Cell<Option<WidgetId>>> = Rc::default();
    let mut tree = WidgetTree::new();
    let source = {
        let source_id = source_id.clone();
        tree.add(
            FixedSize::new()
                .width(100.0)
                .height(100.0)
                .child(RectWidget::new())
                .on_pointer_event(move |event, ctx| {
                    if let (WidgetEvent::PointerDown { .. }, Some(id)) = (event, source_id.get()) {
                        ctx.start_drag(id, teksilo_core::DragPayload::typed(7_u32));
                    }
                    EventResponse::Ignored
                }),
        )
    };
    source_id.set(Some(source));
    let target = {
        let dropped = dropped.clone();
        let hovered = hovered.clone();
        tree.add(
            FixedSize::new()
                .width(200.0)
                .height(100.0)
                .child(RectWidget::new())
                .on_drag_hover(move |_payload, at, _ctx| {
                    hovered.borrow_mut().push(at);
                    teksilo_core::DropFeedback::Accept
                })
                .on_drop(move |_payload, at, _ctx| {
                    dropped.borrow_mut().push(at);
                    true
                }),
        )
    };
    tree.add(HStack::new().child(source).child(target));
    tree.layout(SizeProposal::exact(300.0, 100.0));

    let mut turn = Turn::new(tree, backend());
    turn.moves(&[(50.0, 50.0)]);
    turn.feed(Input::Winit(click(ElementState::Pressed)));
    turn.moves(&[(80.0, 50.0), (120.0, 50.0), (160.0, 50.0), (230.0, 60.0)]);
    turn.release();
    let last = turn.tree.window_to_local(target, p(230.0, 60.0));
    assert_eq!(
        hovered.borrow().last(),
        Some(&last),
        "the turn's one dispatch moves the drag to where the burst ended: {:?}",
        hovered.borrow()
    );
    turn.feed(Input::Winit(click(ElementState::Released)));

    assert_eq!(
        *dropped.borrow(),
        vec![last],
        "one drop, where the burst ended"
    );
}

/// A text selection extended by a burst of moves ends where the burst ended,
/// in one dispatch.
#[test]
fn a_selection_extended_by_a_burst_ends_at_the_last_position() {
    use teksilo_text::text_document::TextDocument;
    use teksilo_widgets::rich_text::RichTextEditor;

    let document = TextDocument::new();
    document
        .set_plain_text("hello world and a longer line of prose")
        .expect("plain text is accepted");
    let editor = RichTextEditor::editor(document);
    let handle = editor.handle();
    let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
    let editor = tree.add(editor);
    let viewport = SizeProposal::exact(480.0, 200.0);
    tree.layout(viewport);
    // The engine lays its text out in `paint`; a tree that never renders
    // hit-tests an empty layout.
    let _ = tree.render();
    let editor = tree
        .first_focusable_descendant(editor)
        .expect("the editor is focusable");
    tree.focus(editor);
    tree.layout(viewport);
    let _ = tree.render();

    let at = |offset: usize| {
        let rect = handle.offset_rect(offset).expect("the editor has laid out");
        (
            f64::from(rect.x + 0.5),
            f64::from(rect.y + rect.height / 2.0),
        )
    };
    let mut turn = Turn::new(tree, backend());
    turn.moves(&[at(2)]);
    turn.feed(Input::Winit(click(ElementState::Pressed)));
    turn.moves(&[at(4), at(6), at(7), at(9)]);
    turn.release();
    assert_eq!(
        handle.selection(),
        (2, 9),
        "the one dispatch extends the selection to where the burst ended"
    );
    turn.feed(Input::Winit(click(ElementState::Released)));
    assert_eq!(handle.selection(), (2, 9));
}

// -- which windows hold --------------------------------------------------------

/// Only the two window systems that deliver every motion event hold moves.
#[test]
fn only_x11_and_wayland_hold_moves() {
    assert!(holds_mouse_moves(WindowSystem::X11));
    assert!(holds_mouse_moves(WindowSystem::Wayland));
    assert!(
        !holds_mouse_moves(WindowSystem::Unknown),
        "Windows and macOS fold motion before winit sees it"
    );
}
