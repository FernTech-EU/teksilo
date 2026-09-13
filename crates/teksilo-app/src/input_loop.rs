// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The input half of one event-loop turn, with the window taken out of it.
//!
//! [`crate::input_routing`] is the layer below: it turns one winit event into
//! tree calls and knows nothing about a window. This layer is what the loop
//! *wraps around* that — the modal guard, the cursor push, the redraw request,
//! the safe-area and keyboard-band supply, the IME reconcile's decision, the
//! pen pump's per-window step, and a new window's translator wiring and a
//! closing one's teardown. All of it used to sit inline in
//! `TeksiloAppHandler`'s event-loop methods or in `WindowManager`'s window
//! lifecycle, where a `&ActiveEventLoop` parameter or a live winit window put
//! it out of reach of a test — winit offers no way to construct an
//! `ActiveEventLoop` outside a running loop, so nothing below that signature
//! could be driven at all.
//!
//! So the window is behind [`InputChrome`] — six operations, no winit types
//! among them — and everything with a decision in it is a free function taking
//! that trait, a [`WidgetTree`], a translator and a
//! [`WindowOps`] sink. `app.rs` and
//! `window_manager.rs` keep the take-aside, the trace and the real chrome; the
//! behaviour lives here.
//!
//! # What is not reachable from a test *here*
//!
//! Moving a body into this module does not witness the *call* to it. The tests
//! below pin that each body does what its name says; whether the loop still
//! asks is a separate question, and it is answered — for the calls that can be
//! answered — one module over, in `crate::app::winit_loop_tests`. That builds
//! a real winit event loop off the main thread and hands
//! `TeksiloAppHandler`'s own callbacks hand-built events, so the pointer arm,
//! the modal guard, the window-system supply and the touch kill switch are
//! witnessed at their call sites rather than only in the bodies below. It is
//! `#[ignore]`d (it needs a display server and a wgpu adapter) and CI runs it
//! under Xvfb with openbox.
//!
//! What remains reviewed rather than tested, on every host, is listed in that
//! module's own docs — and it is the calls whose platform answer is a constant
//! this host cannot vary: the pen pump, the on-screen-keyboard poll and apply,
//! the first safe-area read, and the close-path contact release. Every
//! function here is still called from as few places as it can be — one
//! each, except [`refresh_safe_area`], which the loop and window creation
//! share, and [`host_soft_keyboard_support`], which the ops sink and the
//! settle both read — because the shorter that list is the less there is to
//! review.

use std::time::Instant;

use teksilo_canvas::Rect;
use teksilo_core::WidgetTree;
use teksilo_core::window::{SoftKeyboardSupport, WindowOps};
use teksilo_platform::TranslationState;
use teksilo_platform::pen::PenSource;
use teksilo_platform::pointer_backend::PointerBackend;
use teksilo_platform::safe_area::SafeAreaSides;
use teksilo_platform::window_system::WindowSystem;
use teksilo_tokens::InputTokens;
use winit::event::WindowEvent;

/// The window-shaped operations the input half of a turn performs.
///
/// Six, and none of them takes a winit type: a test supplies a recorder, the
/// loop supplies the real window. Everything else the input path touches — the
/// tree, the translator, the ops sink — is already constructible headless.
pub(crate) trait InputChrome {
    /// Push a cursor shape to the OS.
    fn set_cursor(&mut self, icon: teksilo_core::CursorIcon);

    /// Ask for a frame, naming the arm that asked for the idle trace.
    fn request_redraw(&mut self, reason: &'static str);

    /// The window's platform safe area, in physical-edge terms.
    fn safe_area(&self) -> SafeAreaSides;

    /// The part of this window's client area a screen-space rectangle covers,
    /// in window-logical pixels, or `None` when it misses the window.
    fn occluded_band(&self, screen: (i32, i32, i32, i32)) -> Option<Rect>;

    /// Show (`true`) or hide (`false`) the platform's on-screen keyboard.
    fn set_soft_keyboard_visible(&mut self, visible: bool);

    /// Push IME state to the OS: the purpose where one is given, the allowance
    /// where one is given, neither where the argument is `None`.
    ///
    /// This is behind the trait rather than inline in the callback for one
    /// reason: the framework's composition guarantee is a statement about a
    /// call that must **not** happen (`set_ime_allowed` on an already-allowed
    /// window is a fresh `enable`, and `enable` resets the preedit — see
    /// [`teksilo_platform::soft_keyboard`]). A guarantee about an absent call
    /// can only be asserted where the call would have been observable, so the
    /// call is made observable. [`settle_ime`] is the only caller.
    fn set_ime(&mut self, purpose: Option<teksilo_core::ImePurpose>, allowed: Option<bool>);
}

/// What a window blocked by a modal child does with one event.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum BlockedDisposition {
    /// Not input at all — the OS telling the window about itself. A blocked
    /// window still has to hear it: one that never sees `Resized` leaves its
    /// surface at the old size, one that never sees `RedrawRequested` paints
    /// no frame while the modal is open, one that never sees `Focused(false)`
    /// keeps blinking a caret behind a dialog.
    Deliver,
    /// Input, but not an attempt to interact. Swallowed silently.
    Swallow,
    /// The user tried to interact — press, contact landing, key down. Swallowed,
    /// and the modal child is re-surfaced (raised where the window system
    /// allows it, an attention request where it does not) so the user can see
    /// what is in the way.
    SwallowAndRaise,
}

/// What a blocked window does with `event`.
///
/// The narrow half is the raise: a bare `CursorMoved` must not raise anything,
/// because the parent sees one of those per motion sample and raising on each
/// is a window-manager request and a child repaint per pixel of travel.
pub(crate) fn blocked_disposition(event: &WindowEvent) -> BlockedDisposition {
    if !crate::input_routing::is_user_input_event(event) {
        return BlockedDisposition::Deliver;
    }
    if crate::input_routing::bounces_to_modal(event) {
        BlockedDisposition::SwallowAndRaise
    } else {
        BlockedDisposition::Swallow
    }
}

/// What to do after a pointer event has been routed: whether to push the
/// tree's cursor to the OS, whether to force a redraw regardless of the dirty
/// flags, and what to call the redraw in an idle trace.
///
/// These are the pre-existing per-arm chores, kept per-arm on purpose. A mouse
/// press and a wheel notch have always requested a frame unconditionally
/// (a press changes visuals the dirty flags do not always catch, and a scroll
/// moves content); a bare move has always asked only when the tree said it
/// needed one. Collapsing them onto one policy would change the frame count of
/// every mouse interaction.
fn input_chores(event: &WindowEvent) -> (bool, bool, &'static str) {
    match event {
        WindowEvent::CursorMoved { .. } => (true, false, "cursor"),
        // Hover has just been cleared, so the cursor the tree wants may have
        // changed; the frame follows only if something actually went dirty.
        WindowEvent::CursorLeft { .. } => (true, false, "cursor_left"),
        WindowEvent::MouseInput { .. } => (true, true, "mouse_input"),
        WindowEvent::MouseWheel { .. } => (false, true, "mouse_wheel"),
        WindowEvent::Touch(_) => (false, true, "touch"),
        _ => (false, true, "os_gesture"),
    }
}

/// Route one pointer / touch / gesture event into `tree`, then do the chores
/// its arm owes.
///
/// Returns whether the event was one of the arms
/// [`crate::input_routing`] owns — `false` means the caller's ordinary
/// lifecycle `match` still has to run.
pub(crate) fn dispatch_input(
    chrome: &mut dyn InputChrome,
    backend: &mut TranslationState,
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    event: &WindowEvent,
) -> bool {
    if !crate::input_routing::route_input_event(backend, tree, ops, event) {
        return false;
    }
    let (apply_cursor, force_redraw, trace_label) = input_chores(event);
    if apply_cursor {
        chrome.set_cursor(tree.current_cursor());
    }
    if force_redraw || tree.needs_redraw() {
        chrome.request_redraw(trace_label);
    }
    true
}

/// Drain this window's pen shim and route whatever it buffered.
///
/// Returns whether anything came out, which is also whether a frame was asked
/// for: a poll that finds an empty buffer — the common case on every turn a
/// tool is not on the tablet — costs the tree nothing and the compositor
/// nothing.
pub(crate) fn pump_pen(
    chrome: &mut dyn InputChrome,
    backend: &mut TranslationState,
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
) -> bool {
    let produced = crate::input_routing::route_pen(backend, tree, ops);
    if produced {
        chrome.request_redraw("pen");
    }
    produced
}

/// One window's contribution to [`pen_deadline`].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct PenWindow {
    /// Whether this window's shim fills its buffer from a thread of its own.
    pub off_thread: bool,
    /// Whether a tool is currently over the tablet.
    pub in_proximity: bool,
}

/// When the pen shims next have to be looked at, if ever.
///
/// Two terms, and both are bounded so that an idle app pays nothing:
///
/// - **A tool in proximity, on an off-thread shim** keeps the loop ticking at
///   that shim's poll rate, because a stroke is a continuous stream and nothing
///   else will wake us for it. This is the cost an animation already carries,
///   and it lasts exactly as long as the tool is over the tablet. A shim that
///   reads on the winit thread is woken by its own messages and contributes
///   nothing here.
/// - **One catch-up look** after an external wake, for the packet that woke us
///   but had not reached an off-thread shim's buffer when we looked. Without it
///   a proximity *enter* — the first packet of a session, when nothing is yet
///   in proximity to keep the first term alive — would wait for an unrelated
///   event. `recheck_at` is that armed look; it is consumed once it is past.
///
/// With no off-thread shim open at all, neither term can apply and `recheck_at`
/// is cleared rather than left armed against a window that has since closed.
pub(crate) fn pen_deadline(
    now: Instant,
    windows: impl Iterator<Item = PenWindow>,
    recheck_at: &mut Option<Instant>,
) -> Option<Instant> {
    let mut deadline: Option<Instant> = None;
    let mut any_off_thread = false;
    for window in windows {
        if !window.off_thread {
            continue;
        }
        any_off_thread = true;
        if window.in_proximity {
            let next = now + teksilo_platform::pen::PEN_POLL_INTERVAL;
            deadline = Some(match deadline {
                Some(current) => current.min(next),
                None => next,
            });
        }
    }
    if !any_off_thread {
        *recheck_at = None;
        return deadline;
    }
    match *recheck_at {
        Some(at) if at > now => Some(match deadline {
            Some(current) => current.min(at),
            None => at,
        }),
        Some(_) => {
            *recheck_at = None;
            deadline
        }
        None => deadline,
    }
}

/// Read the window's platform safe area and hand it to the tree.
///
/// The insets are a platform fact the tree cannot deduce, so they reach it the
/// way the reduced-motion preference does — pushed in from outside. `ZERO` on
/// every desktop platform but macOS, and zero there too unless the window
/// covers the camera housing; the tree's setter is change-gated, so the common
/// case costs a comparison.
///
/// The physical-to-leading/trailing mapping happens here, against the tree's
/// own layout direction, because that is the only place both facts are known.
pub(crate) fn refresh_safe_area(chrome: &dyn InputChrome, tree: &mut WidgetTree) {
    let direction = tree.layout_direction();
    tree.set_safe_area(chrome.safe_area().to_insets(direction));
}

/// Report the part of this window the on-screen keyboard covers.
///
/// `screen` is the keyboard's screen-space physical rectangle, or `None` when
/// no keyboard is up — which is also what a window the keyboard misses ends up
/// reporting, since a band that does not intersect the client area is no band.
pub(crate) fn refresh_occluded_band(
    chrome: &dyn InputChrome,
    tree: &mut WidgetTree,
    screen: Option<(i32, i32, i32, i32)>,
) {
    tree.set_occluded_inset(screen.and_then(|screen| chrome.occluded_band(screen)));
}

/// Whether the on-screen keyboard's rectangle is due for a re-read.
///
/// Polled rather than notified, because the only platform whose keyboard a
/// client can find at all is the one whose keyboard sends no notification. Two
/// things keep the cost at nothing: the whole poll is skipped where the
/// platform reports no explicit keyboard control (a compile-time constant for
/// the host this binary was built for, and not `Explicit` on three of the four
/// desktop platforms), and it runs at most once per `interval`, on a turn the
/// loop was awake for anyway.
///
/// The consequence, stated plainly: a keyboard the *user* dismissed is noticed
/// within that interval rather than immediately.
pub(crate) fn osk_poll_due(
    support: SoftKeyboardSupport,
    last: Option<Instant>,
    now: Instant,
    interval: std::time::Duration,
) -> bool {
    if support != SoftKeyboardSupport::Explicit {
        return false;
    }
    match last {
        Some(last) => now.duration_since(last) >= interval,
        None => true,
    }
}

/// Apply a pending `EventContext::request_soft_keyboard`.
///
/// Called after the IME reconcile, and that order is the whole rule. On a
/// platform whose keyboard follows the IME enable, the reconcile *is* the
/// request — and asking again would mean re-asserting allowance, which is what
/// cancels a live composition. So the request resolves to nothing there, and
/// nothing on this path ever calls `set_ime_allowed`: placing a caret with a
/// finger mid-composition keeps the preedit because the code that would have
/// destroyed it is not reachable from here.
///
/// The request is taken off the tree either way, so a dropped one does not
/// accumulate and fire on a later turn against a platform that would honour it.
///
/// See [`teksilo_platform::soft_keyboard::resolve`] for the decision, which is
/// a pure function of the platform's capability and is tested per row.
pub(crate) fn apply_soft_keyboard_request(
    chrome: &mut dyn InputChrome,
    tree: &mut WidgetTree,
    support: SoftKeyboardSupport,
) {
    let Some(visible) = tree.take_soft_keyboard_request() else {
        return;
    };
    if let teksilo_platform::soft_keyboard::SoftKeyboardAction::Ask(visible) =
        teksilo_platform::soft_keyboard::resolve(support, visible)
    {
        chrome.set_soft_keyboard_visible(visible);
    }
}

/// What the IME reconcile still owes the window this turn.
///
/// Both fields are `None` when the window is already in the state the tree
/// wants, and that emptiness is the load-bearing case. Re-asserting IME
/// allowance is not free to assume idempotent: under `zwp_text_input_v3` an
/// `enable` is specified to reset the preedit / commit / delete-surrounding
/// state (see [`teksilo_platform::soft_keyboard`]), so the framework declines
/// to issue a second one rather than trust each platform to swallow it. A turn
/// that finds nothing to change therefore pushes nothing.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct ImeReconcile {
    /// The purpose to push, or `None` to leave the window's alone.
    pub purpose: Option<teksilo_core::ImePurpose>,
    /// The allowance to push, or `None` to leave the window's alone.
    pub allowed: Option<bool>,
}

/// Decide what the window has to be told, given what the tree's focus wants
/// (`wanted`) and what the window was last told.
///
/// `last_purpose` and `last_allowed` are the window's remembered state and are
/// updated to match what the caller is about to push. Disabling IME also
/// forgets the purpose, so it re-applies the next time IME is enabled.
pub(crate) fn ime_reconcile(
    wanted: Option<teksilo_core::ImePurpose>,
    last_purpose: &mut Option<teksilo_core::ImePurpose>,
    last_allowed: &mut Option<bool>,
) -> ImeReconcile {
    let mut out = ImeReconcile::default();
    match wanted {
        Some(purpose) => {
            if *last_purpose != Some(purpose) {
                *last_purpose = Some(purpose);
                out.purpose = Some(purpose);
            }
            if *last_allowed != Some(true) {
                *last_allowed = Some(true);
                out.allowed = Some(true);
            }
        }
        None => {
            if *last_allowed != Some(false) {
                *last_allowed = Some(false);
                *last_purpose = None;
                out.allowed = Some(false);
            }
        }
    }
    out
}

/// The soft-keyboard row this host answers to a widget.
///
/// Named, rather than written inline in
/// [`WindowOpsImpl::soft_keyboard_support`](crate::window_manager::WindowOpsImpl),
/// so the production sink and a test can name the same thing. Without the
/// override a widget asking whether a keyboard can be raised is told the
/// trait's `None` default on every platform — including the one whose row is
/// [`SoftKeyboardSupport::Explicit`].
///
/// A caveat worth stating where it bites: on a host whose row *is* `None`
/// (macOS, Wayland, X11 — see [`teksilo_platform::soft_keyboard`]) the
/// override and its absence produce the same value, so no test run on such a
/// host can tell them apart. What such a test does catch is a *wrong* row.
pub(crate) fn host_soft_keyboard_support() -> SoftKeyboardSupport {
    teksilo_platform::soft_keyboard::support()
}

/// Bring the window's IME state in line with what the tree's focus wants, then
/// apply whatever soft-keyboard request the tree has pending.
///
/// The order is the whole rule, and the emptiness in the middle is the
/// guarantee: a turn that finds the focus unchanged pushes **nothing** at the
/// window, so a live composition is not reset by the framework's own
/// bookkeeping. See [`ime_reconcile`] for the decision and
/// [`apply_soft_keyboard_request`] for why the second half cannot reach
/// `set_ime_allowed` either.
///
/// `last_purpose` and `last_allowed` are the window's remembered state.
pub(crate) fn settle_ime(
    chrome: &mut dyn InputChrome,
    tree: &mut WidgetTree,
    support: SoftKeyboardSupport,
    last_purpose: &mut Option<teksilo_core::ImePurpose>,
    last_allowed: &mut Option<bool>,
) {
    let wanted = tree
        .ime_context_for_focused()
        .map(|context| context.purpose);
    let decision = ime_reconcile(wanted, last_purpose, last_allowed);
    if decision.purpose.is_some() || decision.allowed.is_some() {
        chrome.set_ime(decision.purpose, decision.allowed);
    }
    apply_soft_keyboard_request(chrome, tree, support);
}

/// Revoke every live contact at the translator as its window closes.
///
/// The tree goes with the window, so widget teardown is moot — but the identity
/// allocator behind `PointerId` is *process-global*, and its entries are
/// released only by the translator ending each contact. A window closed with
/// fingers still on the glass would otherwise leave one stale mapping per
/// contact behind. (Bounded, not unbounded: a later contact on the same OS id
/// replaces the entry. Bounded is still a leak.)
pub(crate) fn release_contacts(
    state: &mut TranslationState,
    now: teksilo_core::pointer::EventTime,
) {
    let _ = PointerBackend::cancel_all(state, now);
}

/// Finish wiring a freshly created window's input translator.
///
/// Three facts that are only knowable once a real surface exists, and each one
/// is load-bearing:
///
/// - **The window system**, read from the live display handle and never from an
///   environment variable (`WAYLAND_DISPLAY` and `DISPLAY` are both set in any
///   modern session). It selects the dual-stream suppressors: without it every
///   window runs as [`WindowSystem::Unknown`], and X11's phantom cursor motion
///   — which precedes every packet of the first contact, through the same
///   virtual device a mouse uses — is never suppressed.
/// - **The theme's input tokens**, which carry the touch kill switch and the
///   wheel-notch scale. Without them the translator runs on defaults and
///   `touch_enabled = false` would not turn touch off.
/// - **The pen shim**, where the platform has one. A source reporting
///   [`PenCaps::NONE`](teksilo_platform::PenCaps::NONE) is not installed at
///   all, and not installing it is what keeps `has_pen_source()` false, which
///   is what keeps the pen pump out of the event loop's idle path.
pub(crate) fn wire_translator(
    state: &mut TranslationState,
    window_system: Option<WindowSystem>,
    tokens: InputTokens,
    pen: Option<Box<dyn PenSource>>,
) {
    if let Some(window_system) = window_system {
        state.set_window_system(window_system);
    }
    state.set_input_tokens(tokens);
    if let Some(pen) = pen
        && pen.capabilities() != teksilo_platform::PenCaps::NONE
    {
        state.set_pen_source(pen);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use teksilo_core::CursorIcon;
    use teksilo_core::window::NoopWindowOps;
    use teksilo_tokens::{PenKind, PointerKind};
    use winit::event::{ElementState, TouchPhase};

    use super::*;
    use crate::input_routing::tests::{backend, click, cursor, touch, tree_with_log};

    /// Everything a seam asked of its window, in order.
    #[derive(Debug, Default)]
    struct Chrome {
        cursors: Vec<CursorIcon>,
        redraws: Vec<&'static str>,
        keyboard: Vec<bool>,
        /// Every [`InputChrome::set_ime`] this seam made, in order. Its
        /// *emptiness* is what the composition guarantee is stated against.
        ime: Vec<(Option<teksilo_core::ImePurpose>, Option<bool>)>,
        /// What [`InputChrome::safe_area`] answers.
        sides: SafeAreaSides,
        /// What [`InputChrome::occluded_band`] answers, and what it was asked.
        band: Option<Rect>,
        asked: RefCell<Vec<(i32, i32, i32, i32)>>,
    }

    impl InputChrome for Chrome {
        fn set_cursor(&mut self, icon: CursorIcon) {
            self.cursors.push(icon);
        }
        fn request_redraw(&mut self, reason: &'static str) {
            self.redraws.push(reason);
        }
        fn safe_area(&self) -> SafeAreaSides {
            self.sides
        }
        fn occluded_band(&self, screen: (i32, i32, i32, i32)) -> Option<Rect> {
            self.asked.borrow_mut().push(screen);
            self.band
        }
        fn set_soft_keyboard_visible(&mut self, visible: bool) {
            self.keyboard.push(visible);
        }
        fn set_ime(&mut self, purpose: Option<teksilo_core::ImePurpose>, allowed: Option<bool>) {
            self.ime.push((purpose, allowed));
        }
    }

    // -- (a) the pointer arm, through the loop's own step ----------------

    #[test]
    fn a_contact_reaches_the_tree_and_asks_for_a_frame() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        let mut chrome = Chrome::default();
        let mut ops = NoopWindowOps;

        for event in [
            touch(TouchPhase::Started, 3, 100.0, 100.0),
            touch(TouchPhase::Moved, 3, 120.0, 110.0),
            touch(TouchPhase::Ended, 3, 120.0, 110.0),
        ] {
            assert!(
                dispatch_input(&mut chrome, &mut backend, &mut tree, &mut ops, &event),
                "Touch is one of the arms this step owns"
            );
        }

        let log = log.borrow();
        assert!(
            log.phases
                .iter()
                .all(|(_, kind)| *kind == PointerKind::Touch),
            "a contact must reach the widget as touch, not as a mouse: {:?}",
            log.phases
        );
        assert_eq!(
            log.phases.len(),
            3,
            "down / move / up, one each: {:?}",
            log.phases
        );
        assert_eq!(
            chrome.redraws,
            vec!["touch", "touch", "touch"],
            "every contact sample forces a frame — the dirty flags do not \
             always catch what a press changes"
        );
        assert!(
            chrome.cursors.is_empty(),
            "a finger does not move the mouse cursor"
        );
    }

    #[test]
    fn a_lifecycle_event_is_not_this_step_s_business() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        let mut chrome = Chrome::default();
        let mut ops = NoopWindowOps;

        assert!(
            !dispatch_input(
                &mut chrome,
                &mut backend,
                &mut tree,
                &mut ops,
                &winit::event::WindowEvent::RedrawRequested,
            ),
            "a redraw is for the caller's lifecycle match, not for the pointer path"
        );
        assert!(log.borrow().phases.is_empty());
        assert!(chrome.redraws.is_empty());
    }

    #[test]
    fn the_mouse_arms_keep_their_own_chores() {
        let (mut tree, _log) = tree_with_log();
        let mut backend = backend();
        let mut chrome = Chrome::default();
        let mut ops = NoopWindowOps;

        // A bare move pushes the cursor and asks for a frame only if the tree
        // went dirty; a press pushes the cursor AND forces one.
        dispatch_input(
            &mut chrome,
            &mut backend,
            &mut tree,
            &mut ops,
            &cursor(50.0, 50.0),
        );
        let after_move = chrome.redraws.clone();
        dispatch_input(
            &mut chrome,
            &mut backend,
            &mut tree,
            &mut ops,
            &click(ElementState::Pressed),
        );

        assert_eq!(
            chrome.cursors.len(),
            2,
            "both mouse arms push the tree's cursor to the OS"
        );
        assert_eq!(
            &chrome.redraws[after_move.len()..],
            ["mouse_input"],
            "a press forces a frame regardless of the dirty flags"
        );
    }

    /// Two side-by-side panes, each naming itself when a pinch reaches it.
    /// Left is `x < 200`, right is `x >= 200`, in a 400 × 300 tree.
    fn tree_with_two_pinch_panes() -> (WidgetTree, Rc<RefCell<Vec<&'static str>>>) {
        use teksilo_canvas::{Size, SizeProposal};
        use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
        use teksilo_core::widget_builder::WidgetBuilder;

        #[derive(Debug)]
        struct Pane;
        impl Widget for Pane {
            fn layout_response(
                &self,
                _proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                Size::new(200.0, 300.0).into()
            }
        }

        let seen: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let (left, right) = (seen.clone(), seen.clone());
        let mut tree = WidgetTree::new();
        tree.add(
            teksilo_widgets::primitives::HStack::new()
                .child(Pane.on_pinch(move |_phase, _ctx| left.borrow_mut().push("left")))
                .child(Pane.on_pinch(move |_phase, _ctx| right.borrow_mut().push("right"))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));
        (tree, seen)
    }

    #[test]
    fn an_os_gesture_goes_where_the_os_says_not_to_whoever_owns_hover() {
        // The mouse is over the left pane; a pen hovers over the right one and
        // takes the hover-owner role from it (a contact never could — the
        // pointer table refuses one). A trackpad pinch then arrives, carrying
        // the translator's record of the *cursor*.
        let (mut tree, seen) = tree_with_two_pinch_panes();
        let mut backend = backend();
        let mut chrome = Chrome::default();
        let mut ops = NoopWindowOps;
        dispatch_input(
            &mut chrome,
            &mut backend,
            &mut tree,
            &mut ops,
            &cursor(50.0, 50.0),
        );
        backend.set_pen_source(Box::new(FakePen {
            packets: vec![pen_packet(300.0, 150.0, false)],
            caps: teksilo_platform::PenCaps::FULL_PEN,
            off_thread: true,
        }));
        pump_pen(&mut chrome, &mut backend, &mut tree, &mut ops);

        dispatch_input(
            &mut chrome,
            &mut backend,
            &mut tree,
            &mut ops,
            &winit::event::WindowEvent::PinchGesture {
                device_id: winit::event::DeviceId::dummy(),
                delta: 0.1,
                phase: TouchPhase::Started,
            },
        );

        assert_eq!(
            seen.borrow().as_slice(),
            ["left"],
            "the pinch belongs to the pane the cursor is over; letting              dispatch_os_gesture fall back to the hover owner would send it to              the pane the pen happens to be hovering"
        );
    }

    // -- (b) the modal guard --------------------------------------------

    #[test]
    fn a_blocked_window_still_hears_what_the_os_says_about_it() {
        for event in [
            winit::event::WindowEvent::RedrawRequested,
            winit::event::WindowEvent::Resized(winit::dpi::PhysicalSize::new(800, 600)),
            winit::event::WindowEvent::Focused(false),
            winit::event::WindowEvent::Occluded(true),
            winit::event::WindowEvent::CloseRequested,
        ] {
            assert_eq!(
                blocked_disposition(&event),
                BlockedDisposition::Deliver,
                "a blocked parent that never hears {event:?} cannot resize, \
                 repaint or stop its caret"
            );
        }
    }

    #[test]
    fn a_blocked_window_swallows_input_and_raises_only_on_an_interaction() {
        use BlockedDisposition::{Swallow, SwallowAndRaise};
        let cases: [(winit::event::WindowEvent, BlockedDisposition); 5] = [
            (cursor(10.0, 10.0), Swallow),
            (touch(TouchPhase::Moved, 1, 10.0, 10.0), Swallow),
            (click(ElementState::Released), Swallow),
            (click(ElementState::Pressed), SwallowAndRaise),
            (touch(TouchPhase::Started, 1, 10.0, 10.0), SwallowAndRaise),
        ];
        for (event, want) in cases {
            assert_eq!(
                blocked_disposition(&event),
                want,
                "raising per motion sample is a window-manager request and a \
                 child repaint per pixel of travel: {event:?}"
            );
        }
    }

    // -- (c) the safe area ----------------------------------------------

    #[test]
    fn the_safe_area_reaches_the_tree_mirrored_for_the_reading_direction() {
        use teksilo_core::environment::LayoutDirection;

        let chrome = Chrome {
            sides: SafeAreaSides {
                top: 32.0,
                bottom: 4.0,
                left: 16.0,
                right: 2.0,
            },
            ..Chrome::default()
        };

        let mut tree = WidgetTree::new();
        assert_eq!(
            tree.safe_area(),
            teksilo_canvas::EdgeInsets::ZERO,
            "a tree nobody has told starts with no inset"
        );
        refresh_safe_area(&chrome, &mut tree);
        let ltr = tree.safe_area();
        assert_eq!((ltr.top, ltr.bottom), (32.0, 4.0));
        assert_eq!(
            (ltr.leading, ltr.trailing),
            (16.0, 2.0),
            "left is leading while the reading direction is left-to-right"
        );

        let mut tree = WidgetTree::new();
        tree.set_layout_direction(LayoutDirection::RightToLeft);
        refresh_safe_area(&chrome, &mut tree);
        let rtl = tree.safe_area();
        assert_eq!(
            (rtl.leading, rtl.trailing),
            (2.0, 16.0),
            "the physical sides swap roles under a right-to-left reading direction"
        );
    }

    // -- the keyboard band ----------------------------------------------

    #[test]
    fn the_keyboard_band_reaches_the_tree_and_clears_when_it_goes() {
        let mut chrome = Chrome {
            band: Some(Rect::new(0.0, 500.0, 800.0, 100.0)),
            ..Chrome::default()
        };
        let mut tree = WidgetTree::new();

        refresh_occluded_band(&chrome, &mut tree, Some((0, 1000, 1600, 1200)));
        assert_eq!(
            tree.occluded_inset(),
            Some(Rect::new(0.0, 500.0, 800.0, 100.0))
        );
        assert_eq!(
            chrome.asked.borrow().as_slice(),
            [(0, 1000, 1600, 1200)],
            "the screen rectangle is clipped by the window, not by the caller"
        );

        refresh_occluded_band(&chrome, &mut tree, None);
        assert_eq!(
            tree.occluded_inset(),
            None,
            "no keyboard on screen means no band on any window"
        );

        chrome.band = None;
        refresh_occluded_band(&chrome, &mut tree, Some((0, 1000, 1600, 1200)));
        assert_eq!(
            tree.occluded_inset(),
            None,
            "a keyboard in front of another window is no band on this one"
        );
    }

    #[test]
    fn the_keyboard_rectangle_is_re_read_at_most_once_per_interval() {
        let interval = Duration::from_millis(250);
        let t0 = Instant::now();

        assert!(
            osk_poll_due(SoftKeyboardSupport::Explicit, None, t0, interval),
            "the first turn has nothing to compare against"
        );
        assert!(!osk_poll_due(
            SoftKeyboardSupport::Explicit,
            Some(t0),
            t0 + Duration::from_millis(249),
            interval
        ));
        assert!(osk_poll_due(
            SoftKeyboardSupport::Explicit,
            Some(t0),
            t0 + interval,
            interval
        ));

        for support in [
            SoftKeyboardSupport::None,
            SoftKeyboardSupport::ViaAccessibility,
        ] {
            assert!(
                !osk_poll_due(support, None, t0, interval),
                "{support:?} has no keyboard rectangle to find, so the poll is \
                 skipped whole rather than throttled"
            );
        }
    }

    // -- (e) the soft-keyboard request ----------------------------------

    #[test]
    fn a_keyboard_request_is_asked_for_only_where_asking_is_the_verb() {
        let cases = [
            (SoftKeyboardSupport::Explicit, vec![true]),
            (SoftKeyboardSupport::ViaAccessibility, vec![]),
            (SoftKeyboardSupport::None, vec![]),
        ];
        for (support, want) in cases {
            let mut chrome = Chrome::default();
            // A tree with a root: `run_with_event_context` anchors what a
            // handler asked for on one, and drops the lot when there is none.
            let (mut tree, _log) = tree_with_log();
            let mut ops = NoopWindowOps;
            tree.run_with_event_context(&mut ops, |ctx| ctx.request_soft_keyboard());

            apply_soft_keyboard_request(&mut chrome, &mut tree, support);
            assert_eq!(chrome.keyboard, want, "support row {support:?}");
            assert_eq!(
                tree.take_soft_keyboard_request(),
                None,
                "the request is consumed on every row, so a dropped one cannot \
                 fire on a later turn"
            );
        }
    }

    #[test]
    fn nothing_at_all_happens_without_a_request() {
        let mut chrome = Chrome::default();
        let mut tree = WidgetTree::new();
        apply_soft_keyboard_request(&mut chrome, &mut tree, SoftKeyboardSupport::Explicit);
        assert!(chrome.keyboard.is_empty());
    }

    // -- the query half: what a widget is told it may promise -----------

    /// A sink that answers one capability row and nothing else.
    #[derive(Debug)]
    struct CapabilityOps(SoftKeyboardSupport);

    impl WindowOps for CapabilityOps {
        fn open_window(
            &mut self,
            _config: teksilo_core::WindowConfig,
        ) -> teksilo_core::TeksiloWindowId {
            unreachable!("this sink exists only to answer the capability query")
        }
        fn find_window(&self, _string_id: &str) -> Option<teksilo_core::TeksiloWindowId> {
            None
        }
        fn window_state(
            &self,
            _id: teksilo_core::TeksiloWindowId,
        ) -> Option<teksilo_core::WindowState> {
            None
        }
        fn windows(&self) -> Vec<teksilo_core::WindowState> {
            Vec::new()
        }
        fn focus_window(&mut self, _id: teksilo_core::TeksiloWindowId) {}
        fn close_window_by_id(&mut self, _id: teksilo_core::TeksiloWindowId) {}
        fn soft_keyboard_support(&self) -> SoftKeyboardSupport {
            self.0
        }
    }

    #[test]
    fn a_widget_is_told_the_window_s_row_not_the_trait_s_default() {
        for row in [
            SoftKeyboardSupport::None,
            SoftKeyboardSupport::ViaAccessibility,
            SoftKeyboardSupport::Explicit,
        ] {
            let (mut tree, _log) = tree_with_log();
            let mut ops = CapabilityOps(row);
            let mut seen = None;
            tree.run_with_event_context(&mut ops, |ctx| seen = Some(ctx.soft_keyboard_support()));
            assert_eq!(
                seen,
                Some(row),
                "a widget deciding whether to offer its own keyboard affordance                  reads this; a sink that does not override it answers None on                  every platform, including the one whose row is Explicit"
            );
        }
    }

    #[test]
    fn a_tree_with_no_window_under_it_promises_nothing() {
        let (mut tree, _log) = tree_with_log();
        let mut ops = NoopWindowOps;
        let mut seen = None;
        tree.run_with_event_context(&mut ops, |ctx| seen = Some(ctx.soft_keyboard_support()));
        assert_eq!(seen, Some(SoftKeyboardSupport::None));
    }

    // -- (item 3) the composition-safety guarantee ----------------------

    #[test]
    fn the_reconcile_pushes_nothing_at_a_focus_that_has_not_moved() {
        use teksilo_core::ImePurpose;

        // First turn on a focused field: the window is told both facts.
        let mut purpose = None;
        let mut allowed = None;
        assert_eq!(
            ime_reconcile(Some(ImePurpose::Normal), &mut purpose, &mut allowed),
            ImeReconcile {
                purpose: Some(ImePurpose::Normal),
                allowed: Some(true),
            },
        );

        // Every later turn while that field keeps focus — which is every turn
        // a composition is live — pushes nothing. This is the whole of the
        // composition guarantee: `set_ime_allowed(true)` on an already-allowed
        // window is a fresh `enable`, and `enable` resets the preedit.
        for _ in 0..3 {
            assert_eq!(
                ime_reconcile(Some(ImePurpose::Normal), &mut purpose, &mut allowed),
                ImeReconcile::default(),
                "re-asserting allowance is what cancels a composition mid-word"
            );
        }

        // Focus leaving the field disables IME and forgets the purpose, so it
        // re-applies on the next field.
        assert_eq!(
            ime_reconcile(None, &mut purpose, &mut allowed),
            ImeReconcile {
                purpose: None,
                allowed: Some(false),
            },
        );
        assert_eq!(purpose, None);
        assert_eq!(
            ime_reconcile(Some(ImePurpose::Password), &mut purpose, &mut allowed),
            ImeReconcile {
                purpose: Some(ImePurpose::Password),
                allowed: Some(true),
            },
        );
    }

    #[test]
    fn a_first_focus_is_pushed_and_a_settled_one_is_not() {
        use teksilo_core::ImePurpose;

        let (mut tree, _log) = tree_with_log();
        let mut chrome = Chrome::default();
        let mut purpose = None;
        let mut allowed = None;

        // No focused text widget: the window is told IME is off, once.
        settle_ime(
            &mut chrome,
            &mut tree,
            SoftKeyboardSupport::None,
            &mut purpose,
            &mut allowed,
        );
        assert_eq!(
            chrome.ime,
            vec![(None, Some(false))],
            "the first turn has to tell the window something — the seam that \
             pushes is the same one the composition test asserts is silent"
        );

        // Every later turn at the same (absent) focus pushes nothing at all.
        chrome.ime.clear();
        for _ in 0..3 {
            settle_ime(
                &mut chrome,
                &mut tree,
                SoftKeyboardSupport::None,
                &mut purpose,
                &mut allowed,
            );
        }
        assert!(
            chrome.ime.is_empty(),
            "a settled focus pushes nothing: {:?}",
            chrome.ime
        );

        // And the purpose half travels too, when the remembered state says a
        // field has taken focus.
        purpose = None;
        allowed = None;
        let mut chrome = Chrome::default();
        let decision = ime_reconcile(Some(ImePurpose::Password), &mut purpose, &mut allowed);
        assert_eq!(decision.purpose, Some(ImePurpose::Password));
        chrome.set_ime(decision.purpose, decision.allowed);
        assert_eq!(chrome.ime, vec![(Some(ImePurpose::Password), Some(true))]);
    }

    #[test]
    fn the_platform_row_travels_through_the_settle() {
        // The row `settle_ime` is handed is the row the request is resolved
        // against: passing a hard-coded one instead would make an `Explicit`
        // host silently stop raising its keyboard.
        for (row, expected) in [
            (SoftKeyboardSupport::Explicit, vec![true]),
            (SoftKeyboardSupport::ViaAccessibility, vec![]),
            (SoftKeyboardSupport::None, vec![]),
        ] {
            let (mut tree, _log) = tree_with_log();
            let mut ops = NoopWindowOps;
            tree.run_with_event_context(&mut ops, |ctx| ctx.request_soft_keyboard());
            let mut chrome = Chrome::default();
            let mut purpose = None;
            let mut allowed = Some(false);
            settle_ime(&mut chrome, &mut tree, row, &mut purpose, &mut allowed);
            assert_eq!(chrome.keyboard, expected, "{row:?}");
        }
    }

    #[cfg(feature = "text")]
    #[test]
    fn a_soft_keyboard_request_during_a_composition_keeps_the_preedit() {
        use teksilo_canvas::SizeProposal;
        use teksilo_core::ImePurpose;
        use teksilo_core::event::WidgetEvent;
        use teksilo_text::text_document::TextDocument;
        use teksilo_widgets::rich_text::RichTextEditor;

        for row in [
            SoftKeyboardSupport::None,
            SoftKeyboardSupport::ViaAccessibility,
            SoftKeyboardSupport::Explicit,
        ] {
            let doc = TextDocument::new();
            doc.set_plain_text("").unwrap();
            let mut tree = WidgetTree::new();
            let id = tree.add(RichTextEditor::editor(doc.clone()));
            tree.layout(SizeProposal::exact(400.0, 300.0));
            let _ = tree.render();
            tree.focus(id);
            // A composition in flight, exactly as an IME leaves it between
            // keystrokes.
            tree.dispatch_event(WidgetEvent::ImeComposition {
                text: "ni".to_string(),
                cursor: None,
            });
            tree.tick_animations(Duration::from_millis(16));
            assert_eq!(
                doc.to_plain_text().unwrap_or_default(),
                "ni",
                "the preedit is in the document before the request"
            );

            // The finger lands in the field and the widget asks for a keyboard.
            let mut ops = NoopWindowOps;
            tree.run_with_event_context(&mut ops, |ctx| ctx.request_soft_keyboard());

            // The window has already been told about this focus — which is the
            // state every turn of a live composition is in.
            let mut purpose = Some(ImePurpose::Normal);
            let mut allowed = Some(true);
            let mut chrome = Chrome::default();

            settle_ime(&mut chrome, &mut tree, row, &mut purpose, &mut allowed);

            // The guarantee, stated where it can fail: the turn that carries
            // the request pushes NO IME state. `set_ime_allowed(true)` on an
            // already-allowed window is a fresh `enable`, and `enable` is
            // specified to reset the preedit — so a single entry here is the
            // composition destroyed, whether it came from the reconcile
            // re-asserting or from the request path reaching for the IME
            // channel.
            assert!(
                chrome.ime.is_empty(),
                "{row:?}: the turn a soft-keyboard request rides must push no \
                 IME state at all, and pushed {:?}",
                chrome.ime
            );
            assert_eq!(
                doc.to_plain_text().unwrap_or_default(),
                "ni",
                "{row:?}: the composition survives the request"
            );
            assert_eq!(
                tree.take_soft_keyboard_request(),
                None,
                "{row:?}: the request is consumed either way"
            );
        }
    }

    // -- (f) the translator's teardown ----------------------------------

    #[test]
    fn closing_a_window_releases_every_contact_it_still_held() {
        let (mut tree, _log) = tree_with_log();
        let mut backend = backend();
        let mut chrome = Chrome::default();
        let mut ops = NoopWindowOps;

        for id in [1_u64, 2] {
            dispatch_input(
                &mut chrome,
                &mut backend,
                &mut tree,
                &mut ops,
                &touch(TouchPhase::Started, id, 40.0 * id as f64, 60.0),
            );
        }
        assert_eq!(
            backend.live_contact_count(),
            2,
            "two fingers are on the glass as the window closes"
        );

        release_contacts(&mut backend, tree.input_now());

        assert_eq!(
            backend.live_contact_count(),
            0,
            "the process-global identity allocator releases an entry only when \
             the translator ends the contact"
        );
    }

    // -- (d) the pen ----------------------------------------------------

    /// A shim that yields `packets` once and then nothing.
    #[derive(Debug)]
    struct FakePen {
        packets: Vec<teksilo_platform::pen::PenPacket>,
        caps: teksilo_platform::PenCaps,
        off_thread: bool,
    }

    impl PenSource for FakePen {
        fn poll(&mut self, out: &mut Vec<teksilo_platform::pen::PenPacket>) {
            out.append(&mut self.packets);
        }
        fn capabilities(&self) -> teksilo_platform::PenCaps {
            self.caps
        }
        fn polls_off_thread(&self) -> bool {
            self.off_thread
        }
    }

    fn pen_packet(x: f32, y: f32, down: bool) -> teksilo_platform::pen::PenPacket {
        let mut packet = teksilo_platform::pen::PenPacket::hovering(
            PenKind::Pen,
            teksilo_canvas::Point::new(x, y),
        );
        packet.down = down;
        packet.pressure = if down { 0.5 } else { 0.0 };
        packet
    }

    #[test]
    fn a_pen_packet_reaches_the_tree_and_asks_for_a_frame() {
        let (mut tree, log) = tree_with_log();
        let mut backend = backend();
        let mut chrome = Chrome::default();
        let mut ops = NoopWindowOps;
        backend.set_pen_source(Box::new(FakePen {
            packets: vec![
                pen_packet(120.0, 90.0, false),
                pen_packet(120.0, 90.0, true),
            ],
            caps: teksilo_platform::PenCaps::FULL_PEN,
            off_thread: true,
        }));

        assert!(
            pump_pen(&mut chrome, &mut backend, &mut tree, &mut ops),
            "the shim had packets buffered, so the pump produced samples"
        );
        assert_eq!(chrome.redraws, vec!["pen"]);
        let before = {
            let seen = log.borrow();
            assert!(
                seen.phases
                    .iter()
                    .any(|(_, kind)| matches!(kind, PointerKind::Pen(_))),
                "a digitizer packet must reach the widget as a pen, not a mouse: {:?}",
                seen.phases
            );
            seen.phases.len()
        };

        // Second turn: the buffer is empty, so nothing is routed and no frame
        // is asked for. This is what an idle turn with a tablet attached costs.
        assert!(!pump_pen(&mut chrome, &mut backend, &mut tree, &mut ops));
        assert_eq!(
            chrome.redraws,
            vec!["pen"],
            "an empty poll asks for nothing"
        );
        assert_eq!(log.borrow().phases.len(), before);
    }

    #[test]
    fn the_pen_deadline_follows_the_tool_and_the_catch_up_look() {
        let now = Instant::now();
        let poll = teksilo_platform::pen::PEN_POLL_INTERVAL;

        // No off-thread shim: nothing to wake for, and an armed catch-up look
        // is dropped rather than left pointing at a window that has closed.
        let mut recheck = Some(now + poll);
        assert_eq!(
            pen_deadline(
                now,
                [PenWindow {
                    off_thread: false,
                    in_proximity: true,
                }]
                .into_iter(),
                &mut recheck,
            ),
            None,
            "a shim that reads on the winit thread is woken by its own messages"
        );
        assert_eq!(recheck, None);

        // A tool over the tablet paces the loop at the shim's poll rate.
        let mut recheck = None;
        assert_eq!(
            pen_deadline(
                now,
                [PenWindow {
                    off_thread: true,
                    in_proximity: true,
                }]
                .into_iter(),
                &mut recheck,
            ),
            Some(now + poll),
        );

        // Nothing in proximity, but a catch-up look armed: that look is the
        // deadline — this is the proximity *enter* that would otherwise wait
        // for an unrelated event.
        let mut recheck = Some(now + Duration::from_millis(1));
        assert_eq!(
            pen_deadline(
                now,
                [PenWindow {
                    off_thread: true,
                    in_proximity: false,
                }]
                .into_iter(),
                &mut recheck,
            ),
            Some(now + Duration::from_millis(1)),
        );

        // A catch-up look already past is consumed, leaving nothing behind.
        let mut recheck = Some(now - Duration::from_millis(1));
        assert_eq!(
            pen_deadline(
                now,
                [PenWindow {
                    off_thread: true,
                    in_proximity: false,
                }]
                .into_iter(),
                &mut recheck,
            ),
            None,
        );
        assert_eq!(recheck, None);
    }

    // -- (g) the new window's translator ---------------------------------

    #[test]
    fn a_new_window_s_translator_gets_the_three_facts_only_a_surface_answers() {
        let mut state = TranslationState::new();
        assert_eq!(
            state.window_system(),
            WindowSystem::Unknown,
            "a fresh translator knows nothing about the session it is in"
        );

        let tokens = InputTokens {
            touch_enabled: false,
            ..InputTokens::default()
        };
        wire_translator(
            &mut state,
            Some(WindowSystem::X11),
            tokens,
            Some(Box::new(FakePen {
                packets: Vec::new(),
                caps: teksilo_platform::PenCaps::FULL_PEN,
                off_thread: true,
            })),
        );

        assert_eq!(
            state.window_system(),
            WindowSystem::X11,
            "without this the X11 phantom cursor motion is never suppressed"
        );
        assert!(
            !state.input_tokens().touch_enabled,
            "without the theme's tokens the kill switch would not turn touch off"
        );
        assert!(state.has_pen_source());
        assert!(state.pen_polls_off_thread());
    }

    #[test]
    fn a_source_that_reports_nothing_is_not_installed_as_a_pen() {
        let mut state = TranslationState::new();
        wire_translator(
            &mut state,
            None,
            InputTokens::default(),
            Some(Box::new(FakePen {
                packets: Vec::new(),
                caps: teksilo_platform::PenCaps::NONE,
                off_thread: false,
            })),
        );
        assert!(
            !state.has_pen_source(),
            "not installing it is what keeps the pen pump out of the idle path"
        );
        assert_eq!(
            state.window_system(),
            WindowSystem::Unknown,
            "a window with no display handle leaves the session unknown rather \
             than guessing at one"
        );
    }
}
