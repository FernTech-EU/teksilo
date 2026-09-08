// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The winit event loop's input arms, as functions.
//!
//! Everything here takes a [`PointerBackend`], a [`WidgetTree`] and a
//! [`WindowOps`] and nothing else — no `TeksiloAppHandler`, no
//! `WindowManager`, no winit window. That is deliberate: the routing is the
//! part of the loop with the most behaviour and the least platform, so it is
//! the part that can be driven from a test with a hand-written
//! `winit::event::WindowEvent` and a bare tree. `app.rs` keeps only the
//! taking-the-window-aside and the redraw bookkeeping.
//!
//! # What routes where
//!
//! One event in, zero or more [`InputSample`]s out, each to the tree door that
//! matches its shape:
//!
//! | winit event | sample | door |
//! | --- | --- | --- |
//! | `CursorMoved`, `MouseInput` | pointer | [`WidgetTree::dispatch_pointer_with_ops`] |
//! | `Touch` | pointer | same |
//! | `MouseWheel` | scroll | [`WidgetTree::dispatch_scroll_with_ops`] |
//! | `PinchGesture`, `RotationGesture`, `DoubleTapGesture` | gesture | [`WidgetTree::dispatch_os_gesture`] |
//! | `CursorLeft` | none — no sample exists | [`WidgetTree::pointer_left_window`] |
//!
//! A row can also produce *nothing*: the translator suppresses X11's phantom
//! cursor motion and any promoted click an OS sends for a tap that already
//! arrived as touch, and the touch kill switch empties the `Touch` row
//! entirely.
//!
//! `CursorEntered` is deliberately absent, and so is `TouchpadPressure`. The
//! enter carries no position, and a position only ever arrives with a
//! `CursorMoved` — which re-arms hover through the ordinary path, leaving an
//! arm for the enter with nothing to do. `TouchpadPressure` is macOS Force Touch, whose `stage` has
//! no counterpart in Teksilo's pointer vocabulary and no consumer anywhere in
//! the framework; translating it and dropping the result would be a stub
//! wearing an arm's clothes, so it stays unhandled until something wants it.
//!
//! # The clock
//!
//! Every call reads [`WidgetTree::input_now`], never `Instant::now()`. That is
//! what keeps the automation bridge's simulated time axis in charge of touch:
//! freeze the clock and a long press stops advancing, exactly as it does for a
//! mouse.

use teksilo_core::WidgetTree;
use teksilo_core::pointer::CancelReason;
use teksilo_core::window::WindowOps;
use teksilo_platform::pointer_backend::{BackendEvent, InputSample, PointerBackend};
use winit::event::WindowEvent;

/// Whether this event is one of the pointer/gesture arms
/// [`route_input_event`] owns.
///
/// Keyboard, IME and every lifecycle event answer `false`: they have arms of
/// their own in `app.rs` and no business going through a pointer backend.
pub(crate) fn is_pointer_input_event(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::Touch(_)
            | WindowEvent::PinchGesture { .. }
            | WindowEvent::RotationGesture { .. }
            | WindowEvent::DoubleTapGesture { .. }
    )
}

/// Whether this event is a *user input* of any kind.
///
/// The set a window blocked by a modal child must swallow. Everything outside
/// it — resize, move, scale change, theme change, focus, occlusion, redraw —
/// is the OS telling the window about itself, and a blocked window still has
/// to hear that: it still has a surface to resize, a caret to stop blinking
/// when it loses focus, and a frame to paint.
pub(crate) fn is_user_input_event(event: &WindowEvent) -> bool {
    is_pointer_input_event(event)
        || matches!(
            event,
            WindowEvent::CursorEntered { .. }
                | WindowEvent::KeyboardInput { .. }
                | WindowEvent::ModifiersChanged(_)
                | WindowEvent::Ime(_)
                | WindowEvent::TouchpadPressure { .. }
        )
}

/// Whether a blocked window should raise its modal child for this event.
///
/// Only for an event that means *the user tried to interact*: a press, a
/// contact landing, a key. A mere `CursorMoved` over the blocked parent must
/// not raise anything — the parent sees one of those per motion sample, and
/// raising on each is a window-manager request and a child repaint per pixel
/// of travel.
pub(crate) fn bounces_to_modal(event: &WindowEvent) -> bool {
    match event {
        WindowEvent::MouseInput { state, .. } => *state == winit::event::ElementState::Pressed,
        WindowEvent::Touch(touch) => touch.phase == winit::event::TouchPhase::Started,
        WindowEvent::KeyboardInput { event, .. } => event.state.is_pressed(),
        _ => false,
    }
}

/// Route one pointer/gesture event into `tree`.
///
/// Returns `true` when the event was one of the arms this module owns —
/// which is exactly [`is_pointer_input_event`], and is what the caller uses to
/// decide whether the ordinary `match` still has to run.
pub(crate) fn route_input_event(
    backend: &mut dyn PointerBackend,
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    event: &WindowEvent,
) -> bool {
    if !is_pointer_input_event(event) {
        return false;
    }
    if matches!(event, WindowEvent::CursorLeft { .. }) {
        // The backend has no sample for a boundary crossing — winit reports no
        // position with it — so this one goes to the tree directly.
        tree.pointer_left_window(ops);
        return true;
    }
    let now = tree.input_now();
    let samples = backend.translate(&BackendEvent::Winit(event), now);
    route_samples(tree, ops, samples);
    true
}

/// Drain the pen shim and route whatever it buffered.
///
/// Separate from [`route_input_event`] because a digitizer packet is not a
/// winit event: the shim collects packets on its own — on its own thread under
/// Wayland, from the window's message stream on Windows — and this is the pump
/// that asks it what it has.
pub(crate) fn route_pen(
    backend: &mut teksilo_platform::TranslationState,
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
) -> bool {
    if !backend.has_pen_source() {
        return false;
    }
    let now = tree.input_now();
    let samples = backend.poll_pen(now);
    let produced = !samples.is_empty();
    route_samples(tree, ops, samples);
    produced
}

/// Revoke every live pointer, at both ends.
///
/// Two ends because there are two pieces of state: the tree's pointer table,
/// press arbitration and capture, and the backend's own contact map plus the
/// process-global identity allocator behind it. Cancelling one and not the
/// other leaves either a widget holding a press nothing will ever release, or
/// an OS contact id whose identity is never returned.
///
/// `reason` reaches the widget, so it is raised **first**: the cancel funnel is
/// first-writer-wins, and the backend's own revocations arrive as
/// [`CancelReason::Platform`], which would otherwise be what a deactivated
/// window told every one of its widgets.
pub(crate) fn cancel_all_input(
    backend: &mut dyn PointerBackend,
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    reason: CancelReason,
) {
    tree.cancel_all_pointers(reason, ops);
    let now = tree.input_now();
    let samples = backend.cancel_all(now);
    // The tree has already revoked each press, so these run through the funnel
    // as no-ops; what they still do is end the table entries, which is the
    // difference between a contact that lifted and one that is merely
    // forgotten.
    route_samples(tree, ops, samples);
}

/// Flip the window-active state, cancelling every live pointer on the way out.
///
/// The reason is a parameter because the two ways a window goes inactive are
/// not the same thing to a widget: losing focus means the user is somewhere
/// else, being occluded means the window is behind something. The funnel is
/// first-writer-wins, so the reason has to be raised *before*
/// [`WidgetTree::set_window_active_with_ops`], which raises
/// [`CancelReason::WindowDeactivated`] on its own — otherwise an occlusion
/// would tell every widget it lost focus.
///
/// The backend's own revocation comes last, and it is not redundant: the tree
/// knows about presses and captures, the backend knows about OS contact ids
/// and the process-global identity allocator behind them. Only draining both
/// leaves a window that regains focus starting from nothing.
pub(crate) fn set_window_active(
    backend: &mut dyn PointerBackend,
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    active: bool,
    reason: CancelReason,
) {
    if !active {
        cancel_all_input(backend, tree, ops, reason);
    }
    tree.set_window_active_with_ops(active, ops);
}

/// Send each sample to the door its shape names.
fn route_samples(tree: &mut WidgetTree, ops: &mut dyn WindowOps, samples: Vec<InputSample>) {
    for sample in samples {
        match sample {
            InputSample::Pointer(p) => tree.dispatch_pointer_with_ops(p, ops),
            InputSample::Scroll(s) => tree.dispatch_scroll_with_ops(s, ops),
            InputSample::Gesture(g) => {
                let at = gesture_position(&g);
                tree.dispatch_os_gesture(g, at, ops);
            }
        }
    }
}

/// Where a pre-recognized OS gesture happened, when it says.
///
/// winit reports pinch and rotation without a position, so the translator
/// stamps its own record of the **cursor** onto them; a double tap carries the
/// same. Dropping the stamp and letting `dispatch_os_gesture` fall back is not
/// equivalent: the fallback resolves through the *hover owner*, and the hover
/// owner is not always the mouse. A hovering pen takes the role from a
/// stationary cursor (a contact never can — `PointerTable::claim_hover_owner`
/// refuses one), so with the pen over one pane and the cursor over another, the
/// fallback sends a trackpad pinch to the pen's pane. The stamp is the only
/// thing that keeps an OS gesture going to where the OS says it happened.
fn gesture_position(
    gesture: &teksilo_core::gesture::GestureEvent,
) -> Option<teksilo_canvas::Point> {
    use teksilo_core::gesture::GestureEvent;
    match gesture {
        GestureEvent::PinchStarted { center } => Some(*center),
        GestureEvent::PinchChanged { center, .. } => Some(*center),
        GestureEvent::DoubleTap(tap) => Some(tap.position),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use teksilo_canvas::Size;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::pointer::{CancelReason, PointerPhase, ScrollPhase};
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_builder::WidgetBuilder;
    use teksilo_core::window::NoopWindowOps;
    use teksilo_platform::event_translation::TranslationState;
    use teksilo_platform::window_system::WindowSystem;
    use teksilo_tokens::PointerKind;
    use winit::event::{DeviceId, ElementState, MouseScrollDelta, Touch, TouchPhase};

    use super::*;

    /// Everything a routed sample did to the widget, in order.
    #[derive(Debug, Default)]
    pub(crate) struct Log {
        pub(crate) phases: Vec<(PointerPhase, PointerKind)>,
        pub(crate) scrolls: Vec<ScrollPhase>,
        pub(crate) gestures: usize,
        pub(crate) enters: usize,
        pub(crate) leaves: usize,
        pub(crate) cancels: Vec<CancelReason>,
    }

    pub(crate) type Shared = Rc<RefCell<Log>>;

    /// A leaf that fills whatever it is given.
    #[derive(Debug)]
    struct Leaf;

    impl Widget for Leaf {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(
                proposal.width.unwrap_or(200.0),
                proposal.height.unwrap_or(200.0),
            )
            .into()
        }
    }

    /// A 400 × 300 tree whose single widget records what reaches it.
    pub(crate) fn tree_with_log() -> (WidgetTree, Shared) {
        tree_with_log_capturing(false)
    }

    /// The same, with the widget taking the pointer capture on every press —
    /// the state a drag leaves behind, reached through the production path
    /// (`EventContext::capture_pointer`) rather than by poking the arena.
    fn tree_with_log_capturing(capture_on_down: bool) -> (WidgetTree, Shared) {
        let log: Shared = Rc::new(RefCell::new(Log::default()));
        let mut tree = WidgetTree::new();
        tree.add(logging_leaf(&log, capture_on_down));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        (tree, log)
    }

    /// A leaf that fills its parent and records every sample that reaches it.
    ///
    /// Shared with `crate::app::winit_loop_tests`, which builds its window's
    /// root out of this so what a real winit callback delivers is judged by
    /// the same recorder the headless steps are.
    pub(crate) fn logging_leaf(log: &Shared, capture_on_down: bool) -> impl Widget + 'static {
        let (a, b, c, d, e, f) = (
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
        );
        Leaf.on_pointer_event(move |event, ctx| {
            use teksilo_core::event::WidgetEvent;
            let kind = ctx.pointer().kind;
            let phase = match event {
                WidgetEvent::PointerDown { .. } => Some(PointerPhase::Down),
                WidgetEvent::PointerUp { .. } => Some(PointerPhase::Up),
                WidgetEvent::PointerMove { .. } => Some(PointerPhase::Move),
                _ => None,
            };
            if let Some(phase) = phase {
                a.borrow_mut().phases.push((phase, kind));
                if capture_on_down && phase == PointerPhase::Down {
                    ctx.capture_pointer();
                }
            }
            teksilo_core::event::EventResponse::Ignored
        })
        .on_scroll(move |event, _ctx| {
            if let teksilo_core::event::WidgetEvent::Scroll { phase, .. } = event {
                b.borrow_mut().scrolls.push(*phase);
            }
            teksilo_core::event::EventResponse::Ignored
        })
        .on_pinch(move |_phase, _ctx| {
            c.borrow_mut().gestures += 1;
        })
        .on_double_tap(move |_tap, _ctx| {
            f.borrow_mut().gestures += 1;
        })
        .on_hover(move |entered, _ctx| {
            let mut log = d.borrow_mut();
            if entered {
                log.enters += 1;
            } else {
                log.leaves += 1;
            }
        })
        .on_pointer_cancel(move |_info, reason, _ctx| {
            e.borrow_mut().cancels.push(reason);
        })
    }

    pub(crate) fn backend() -> TranslationState {
        let mut state = TranslationState::new();
        state.set_window_system(WindowSystem::Unknown);
        state
    }

    pub(crate) fn touch(phase: TouchPhase, id: u64, x: f64, y: f64) -> WindowEvent {
        WindowEvent::Touch(Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: winit::dpi::PhysicalPosition::new(x, y),
            force: None,
            id,
        })
    }

    pub(crate) fn cursor(x: f64, y: f64) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        }
    }

    pub(crate) fn click(state: ElementState) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button: winit::event::MouseButton::Left,
        }
    }

    fn wheel(delta: MouseScrollDelta, phase: TouchPhase) -> WindowEvent {
        WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta,
            phase,
        }
    }

    fn feed(
        backend: &mut TranslationState,
        tree: &mut WidgetTree,
        events: &[WindowEvent],
    ) -> Vec<bool> {
        let mut ops = NoopWindowOps;
        events
            .iter()
            .map(|event| route_input_event(backend, tree, &mut ops, event))
            .collect()
    }

    // -- the arms -------------------------------------------------------

    #[test]
    fn a_contact_reaches_the_tree_as_a_touch_pointer() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                touch(TouchPhase::Started, 7, 100.0, 100.0),
                touch(TouchPhase::Moved, 7, 108.0, 104.0),
                touch(TouchPhase::Ended, 7, 108.0, 104.0),
            ],
        );
        let phases = &log.borrow().phases;
        assert_eq!(
            phases,
            &[
                (PointerPhase::Down, PointerKind::Touch),
                (PointerPhase::Move, PointerKind::Touch),
                (PointerPhase::Up, PointerKind::Touch),
            ],
            "the whole contact reaches the widget, as touch and not as a mouse"
        );
    }

    #[test]
    fn a_contact_writes_no_hover() {
        // Not a detail: a finger has no hover, and a tree that let one write
        // hover would leave a widget lit up after the finger lifted.
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                touch(TouchPhase::Started, 1, 100.0, 100.0),
                touch(TouchPhase::Ended, 1, 100.0, 100.0),
            ],
        );
        assert_eq!(log.borrow().enters, 0);
        assert!(tree.hovered().is_none());
    }

    #[test]
    fn a_cursor_leave_clears_hover() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(&mut backend, &mut tree, &[cursor(100.0, 100.0)]);
        assert_eq!(log.borrow().enters, 1, "the move armed hover");
        assert!(tree.hovered().is_some());

        let handled = feed(
            &mut backend,
            &mut tree,
            &[WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            }],
        );
        assert_eq!(handled, vec![true], "the leave is an arm this module owns");
        assert_eq!(log.borrow().leaves, 1, "the widget was told");
        assert!(
            tree.hovered().is_none(),
            "and hover did not survive the pointer leaving the window"
        );
    }

    #[test]
    fn a_cursor_leave_does_not_disturb_a_captured_pointer() {
        // A drag whose pointer wanders off the window keeps its target — which
        // is what makes dragging past the edge, and the OS-drag escalation
        // built on it, work at all.
        let (mut tree, _log) = tree_with_log_capturing(true);
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[cursor(100.0, 100.0), click(ElementState::Pressed)],
        );
        let hovered = tree.hovered();
        assert!(
            tree.pointer_captured_by().is_some(),
            "the press took the capture"
        );

        feed(
            &mut backend,
            &mut tree,
            &[WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            }],
        );
        assert_eq!(
            tree.hovered(),
            hovered,
            "the capture outranks the boundary crossing"
        );
    }

    #[test]
    fn an_os_pinch_reaches_the_widget() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                cursor(100.0, 100.0),
                WindowEvent::PinchGesture {
                    device_id: DeviceId::dummy(),
                    delta: 0.2,
                    phase: TouchPhase::Started,
                },
                WindowEvent::PinchGesture {
                    device_id: DeviceId::dummy(),
                    delta: 0.3,
                    phase: TouchPhase::Moved,
                },
                WindowEvent::PinchGesture {
                    device_id: DeviceId::dummy(),
                    delta: 0.0,
                    phase: TouchPhase::Ended,
                },
            ],
        );
        assert_eq!(
            log.borrow().gestures,
            3,
            "started, changed and ended all reach the tree's one pinch door"
        );
    }

    #[test]
    fn a_rotation_gesture_reaches_the_widget() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                cursor(100.0, 100.0),
                WindowEvent::RotationGesture {
                    device_id: DeviceId::dummy(),
                    delta: 4.0,
                    phase: TouchPhase::Started,
                },
            ],
        );
        assert_eq!(log.borrow().gestures, 1);
    }

    #[test]
    fn a_double_tap_gesture_reaches_the_widget() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                cursor(100.0, 100.0),
                WindowEvent::DoubleTapGesture {
                    device_id: DeviceId::dummy(),
                },
            ],
        );
        assert_eq!(log.borrow().gestures, 1);
    }

    // -- the mouse, unchanged -------------------------------------------

    #[test]
    fn a_mouse_click_is_a_mouse_pointer_down_and_up() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                cursor(100.0, 100.0),
                click(ElementState::Pressed),
                click(ElementState::Released),
            ],
        );
        assert_eq!(
            log.borrow().phases,
            vec![
                (PointerPhase::Move, PointerKind::Mouse),
                (PointerPhase::Down, PointerKind::Mouse),
                (PointerPhase::Up, PointerKind::Mouse),
            ]
        );
    }

    #[test]
    fn a_wheel_notch_is_still_one_discrete_scroll() {
        // The acceptance criterion for this package: a mouse behaves exactly as
        // it did before the arms moved onto the backend surface. A real wheel
        // sends `LineDelta` with `TouchPhase::Moved`, and the phase machine
        // answers `Discrete` — which is what the old positionless
        // `WidgetEvent::Scroll` meant.
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                cursor(100.0, 100.0),
                wheel(MouseScrollDelta::LineDelta(0.0, -1.0), TouchPhase::Moved),
                wheel(MouseScrollDelta::LineDelta(0.0, -1.0), TouchPhase::Moved),
            ],
        );
        assert_eq!(
            log.borrow().scrolls,
            vec![ScrollPhase::Discrete, ScrollPhase::Discrete]
        );
    }

    #[test]
    fn a_precision_trackpad_scroll_arrives_phased() {
        // The one deliberate behaviour change routing brings: a trackpad that
        // reports phases used to be flattened into a run of notches by the free
        // translator, which discarded `phase` outright. It now arrives as the
        // gesture it is, which is what a phase-aware scrollable was built for.
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        let pixels =
            |y: f64| MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, y));
        feed(
            &mut backend,
            &mut tree,
            &[
                cursor(100.0, 100.0),
                wheel(pixels(6.0), TouchPhase::Started),
                wheel(pixels(12.0), TouchPhase::Moved),
                wheel(pixels(3.0), TouchPhase::Ended),
            ],
        );
        assert_eq!(
            log.borrow().scrolls,
            vec![ScrollPhase::Began, ScrollPhase::Changed, ScrollPhase::Ended]
        );
    }

    // -- cancels ---------------------------------------------------------

    #[test]
    fn deactivating_the_window_cancels_every_live_contact_with_its_reason() {
        // What is new here is the **backend** drain — the last assertion. The
        // widget-side revocation and the `WindowDeactivated` reason it carries
        // are P09's: `WidgetTree::set_window_active_with_ops` raises that reason
        // on its own, so this path would revoke the presses even if it passed
        // the backend nothing. What it would leave behind is the translator's
        // own contact map and, under it, entries in the process-global
        // `PointerId` allocator — a window that regains focus starting from two
        // ghosts. `an_occluded_window_says_occluded_not_deactivated` is where
        // the reason *parameter* earns its place; here it is only the default
        // spelled out.
        //
        // Capturing, because a cancel revokes a *press*: a contact whose widget
        // holds nothing has nothing to take away, and the funnel says so by
        // returning before it dispatches.
        let (mut tree, log) = tree_with_log_capturing(true);
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[
                touch(TouchPhase::Started, 1, 90.0, 90.0),
                touch(TouchPhase::Started, 2, 140.0, 90.0),
            ],
        );
        assert_eq!(tree.live_pointers().count(), 2);

        let mut ops = NoopWindowOps;
        set_window_active(
            &mut backend,
            &mut tree,
            &mut ops,
            false,
            CancelReason::WindowDeactivated,
        );

        assert_eq!(
            log.borrow().cancels,
            vec![
                CancelReason::WindowDeactivated,
                CancelReason::WindowDeactivated
            ],
            "both contacts were revoked, and each widget was told why"
        );
        assert_eq!(
            tree.live_pointers().count(),
            0,
            "the tree's pointer table is empty afterwards"
        );
        // And the backend's own contact map with it — the half of this that is
        // not already P09's. A second `cancel_all` finds nothing to revoke only
        // if the first one emptied the map.
        let mut ops = NoopWindowOps;
        let leftover = PointerBackend::cancel_all(&mut backend, tree.input_now());
        assert!(
            leftover.is_empty(),
            "the backend kept no contact after the deactivation"
        );
        let _ = &mut ops;
    }

    #[test]
    fn an_occluded_window_says_occluded_not_deactivated() {
        // The reason reaches the widget, and the two ways a window goes
        // inactive are not the same thing to it.
        let (mut tree, log) = tree_with_log_capturing(true);
        let mut backend = backend();
        feed(
            &mut backend,
            &mut tree,
            &[touch(TouchPhase::Started, 1, 90.0, 90.0)],
        );
        let mut ops = NoopWindowOps;
        set_window_active(
            &mut backend,
            &mut tree,
            &mut ops,
            false,
            CancelReason::Occluded,
        );
        assert_eq!(log.borrow().cancels, vec![CancelReason::Occluded]);
    }

    #[test]
    fn reactivating_a_window_cancels_nothing() {
        let (mut tree, log) = tree_with_log_capturing(true);
        let mut backend = backend();
        let mut ops = NoopWindowOps;
        set_window_active(
            &mut backend,
            &mut tree,
            &mut ops,
            false,
            CancelReason::WindowDeactivated,
        );
        feed(
            &mut backend,
            &mut tree,
            &[touch(TouchPhase::Started, 1, 90.0, 90.0)],
        );
        let mut ops = NoopWindowOps;
        set_window_active(
            &mut backend,
            &mut tree,
            &mut ops,
            true,
            CancelReason::WindowDeactivated,
        );
        assert!(
            log.borrow().cancels.is_empty(),
            "a window coming back does not revoke the finger that is on it"
        );
    }

    // -- classification --------------------------------------------------

    #[test]
    fn the_pointer_family_is_exactly_the_arms_this_module_owns() {
        for event in [
            cursor(1.0, 1.0),
            click(ElementState::Pressed),
            wheel(MouseScrollDelta::LineDelta(0.0, 1.0), TouchPhase::Moved),
            touch(TouchPhase::Started, 0, 1.0, 1.0),
            WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
            WindowEvent::PinchGesture {
                device_id: DeviceId::dummy(),
                delta: 0.0,
                phase: TouchPhase::Moved,
            },
            WindowEvent::RotationGesture {
                device_id: DeviceId::dummy(),
                delta: 0.0,
                phase: TouchPhase::Moved,
            },
            WindowEvent::DoubleTapGesture {
                device_id: DeviceId::dummy(),
            },
        ] {
            assert!(is_pointer_input_event(&event), "{event:?} is a pointer arm");
            assert!(is_user_input_event(&event), "{event:?} is user input");
        }
    }

    #[test]
    fn lifecycle_events_are_not_input_and_a_blocked_window_still_hears_them() {
        // The modal-guard fix, stated as a table. A window blocked by a modal
        // child renders no frame, resizes no surface and never learns it lost
        // focus if these are swallowed with the input.
        for event in [
            WindowEvent::RedrawRequested,
            WindowEvent::Resized(winit::dpi::PhysicalSize::new(800, 600)),
            WindowEvent::Moved(winit::dpi::PhysicalPosition::new(10, 10)),
            WindowEvent::Focused(false),
            WindowEvent::Occluded(true),
            WindowEvent::ThemeChanged(winit::window::Theme::Dark),
            WindowEvent::CloseRequested,
        ] {
            assert!(!is_user_input_event(&event), "{event:?} is not user input");
            assert!(
                !is_pointer_input_event(&event),
                "{event:?} is not a pointer arm"
            );
        }
    }

    #[test]
    fn touchpad_pressure_is_swallowed_by_a_modal_but_routed_by_nothing() {
        // macOS Force Touch. It is user input — a blocked parent must not act
        // on it — but Teksilo's pointer vocabulary has no `stage` and no
        // consumer for a trackpad force, so there is no arm to route it to.
        let event = WindowEvent::TouchpadPressure {
            device_id: DeviceId::dummy(),
            pressure: 0.8,
            stage: 1,
        };
        assert!(is_user_input_event(&event));
        assert!(!is_pointer_input_event(&event));
    }

    #[test]
    fn only_an_attempt_to_interact_raises_the_modal() {
        assert!(bounces_to_modal(&click(ElementState::Pressed)));
        assert!(bounces_to_modal(&touch(TouchPhase::Started, 0, 1.0, 1.0)));
        // A release is the tail of an interaction that was already bounced,
        // and a move is the pointer merely crossing the blocked window: raising
        // on either is a window-manager request per motion sample.
        assert!(!bounces_to_modal(&click(ElementState::Released)));
        assert!(!bounces_to_modal(&cursor(1.0, 1.0)));
        assert!(!bounces_to_modal(&touch(TouchPhase::Moved, 0, 1.0, 1.0)));
        assert!(!bounces_to_modal(&WindowEvent::RedrawRequested));
    }
}
