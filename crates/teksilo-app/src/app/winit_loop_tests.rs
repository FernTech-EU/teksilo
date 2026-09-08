// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Driving [`TeksiloAppHandler`]'s own winit callbacks, from a real event loop.
//!
//! Every other test in this crate stops at the seam: [`crate::input_loop`]'s
//! bodies are free functions over a recorder, and what they do is pinned there.
//! What none of them can see is the *call* — `window_event` deciding to route a
//! `Touch` at all, `create_window` deciding to supply the safe area at all —
//! because both sit under a `&ActiveEventLoop`, and winit offers no way to
//! construct one outside a running loop.
//!
//! So this runs one. On Linux winit will build an event loop off the main
//! thread ([`EventLoopBuilderExtX11::with_any_thread`]), and
//! [`EventLoopExtRunOnDemand::run_app_on_demand`] pumps it until the handler
//! asks to exit — which means that inside a callback there *is* an
//! `&ActiveEventLoop`, and the real handler can be handed a hand-built
//! `WindowEvent` exactly as winit would hand it one. [`Script`] is the wrapper
//! that does the handing: it forwards every callback to the real handler and,
//! on the first `about_to_wait` (by which time `resumed` has created the
//! window), runs the assertions and exits.
//!
//! # Why it is `#[ignore]`d
//!
//! It needs a display server and a wgpu adapter, and `cargo test` in this
//! workspace is otherwise fully headless. CI runs it under Xvfb with openbox,
//! in the same job and from the same X server as `teksilo-platform`'s X11
//! protocol tests — see the `test-x11` job in `.github/workflows/ci.yml`.
//!
//! # What it decides, and what it cannot
//!
//! Seven claims, and they do not all carry the same weight. Verified by
//! mutation — each of these reverts a production line and reddens this test:
//!
//! - **(a)** the pointer arm in `handle_window_event_inner` is reached at all;
//! - **(a2)** the window system reached the translator, so X11's phantom
//!   cursor motion during a contact is suppressed;
//! - **(e)/(f)** the theme's `touch_enabled` reaches both an open window's
//!   translator (`WindowManager::set_theme`) and a window created afterwards
//!   (`input_loop::wire_translator`);
//! - **(b)** the modal-blocked guard both swallows input and still delivers
//!   `Resized`;
//! - **(d)** a *wrong* soft-keyboard row is caught.
//!
//! Two are weaker than they look, and the reason is not fixable from here.
//! **(c)**, the safe area, and the missing-override half of **(d)** compare a
//! value against the platform function that produced it — and on Linux that
//! function answers exactly what an untouched tree and the trait default
//! answer: the safe area is zero everywhere but a macOS camera-housing window,
//! and `soft_keyboard_support()` is [`SoftKeyboardSupport::None`] on every
//! desktop but Windows. Deleting `create_window`'s first safe-area read, or
//! deleting `WindowOpsImpl::soft_keyboard_support` outright, leaves this test
//! green; both mutations were run and both did.
//!
//! And this module is Linux-only — it reaches for winit's X11 extension traits,
//! which the Windows and macOS backends do not have — so "a host where those
//! two values differ would catch it" is an argument, not a run. Nothing here
//! decides the two on the platforms where they are not constants. Each site
//! says so where it stands.
//!
//! Still unwitnessed by anything, here or elsewhere, and named so a reviewer
//! knows to read them rather than trust them: the pen pump's per-turn call
//! (X11 has no pen path, so no shim is ever installed on this host), the
//! on-screen-keyboard poll and its apply (`Explicit` is a Windows-only row),
//! and the close-path contact release (the identity allocator it protects has
//! no observable side to assert on once the window is gone).

use std::cell::RefCell;
use std::rc::Rc;

use teksilo_core::window::SoftKeyboardSupport;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowId;

use super::{AppEvent, TeksiloAppBuilder, TeksiloAppHandler};
use crate::input_routing::tests::{Shared, click, cursor, logging_leaf, touch};
use crate::window_config::WindowConfig;

/// Forwards every winit callback to the real handler, then runs `step` once —
/// on the first `about_to_wait`, which is the first moment a window exists and
/// an `&ActiveEventLoop` is in hand — and exits.
struct Script<'a, F> {
    app: &'a mut TeksiloAppHandler,
    step: Option<F>,
}

impl<F> ApplicationHandler<AppEvent> for Script<'_, F>
where
    F: FnOnce(&mut TeksiloAppHandler, &ActiveEventLoop),
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.app.resumed(event_loop);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.app.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        self.app.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.about_to_wait(event_loop);
        if let Some(step) = self.step.take() {
            step(self.app, event_loop);
            event_loop.exit();
        }
    }
}

/// Build the app `builder` describes, pump one real event loop, and run `step`
/// against the live handler with a live `&ActiveEventLoop`.
fn drive(builder: TeksiloAppBuilder, step: impl FnOnce(&mut TeksiloAppHandler, &ActiveEventLoop)) {
    builder.run_with(
        || {
            EventLoop::<AppEvent>::with_user_event()
                // X11 rather than "whatever the session offers": it is the one
                // backend Xvfb provides, so the CI job and a developer's
                // Wayland session exercise the same path.
                .with_x11()
                // The whole reason this is reachable at all — the Rust test
                // harness does not run a test on the process's main thread.
                .with_any_thread(true)
                .build()
                .expect("winit could not build an X11 event loop (is DISPLAY set?)")
        },
        |mut event_loop, app| {
            let mut script = Script {
                app,
                step: Some(step),
            };
            event_loop
                .run_app_on_demand(&mut script)
                .expect("winit event loop exited with error");
        },
    );
}

/// The one managed window's winit id, when there is exactly one.
fn only_window(app: &TeksiloAppHandler) -> WindowId {
    let ids: Vec<WindowId> = app.wm.windows_map().keys().copied().collect();
    assert_eq!(
        ids.len(),
        1,
        "expected exactly one window, got {}",
        ids.len()
    );
    ids[0]
}

/// Everything the four claims need in one loop session: a 400 × 300 window
/// whose root is the same recording leaf the headless steps use.
fn app_with_recorder(log: &Shared) -> TeksiloAppBuilder {
    let log = log.clone();
    TeksiloAppBuilder::new()
        .theme(teksilo_core::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("teksilo winit-loop test")
                .size(400, 300)
                .root(move |tree, _state| tree.add(logging_leaf(&log, false))),
        )
}

#[test]
#[ignore = "needs a display server and a wgpu adapter; CI runs it under Xvfb \
            (see the test-x11 job in .github/workflows/ci.yml)"]
fn the_real_callbacks_route_input_and_supply_the_platform_facts() {
    let log: Shared = Rc::new(RefCell::new(Default::default()));
    let recorder = log.clone();

    drive(app_with_recorder(&log), move |app, event_loop| {
        let window = only_window(app);

        // -- (a) a contact handed to the real `window_event` reaches the tree
        //
        // Not `input_loop::dispatch_input` — `TeksiloAppHandler::window_event`,
        // the method winit itself calls, with the event winit itself would
        // build. Deleting the `is_pointer_input_event` arm in
        // `handle_window_event_inner` leaves nothing in the log.
        // Cleared first, not only afterwards: the physical mouse pointer may
        // already be over the new window, and a real `CursorMoved` delivered
        // between window creation and this callback would otherwise be counted
        // as one of ours.
        recorder.borrow_mut().phases.clear();
        for phase in [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended] {
            app.window_event(event_loop, window, touch(phase, 7, 120.0, 90.0));
        }
        {
            let seen = recorder.borrow();
            assert_eq!(
                seen.phases.len(),
                3,
                "down / move / up should have reached the widget through the \
                 real callback: {:?}",
                seen.phases
            );
            assert!(
                seen.phases
                    .iter()
                    .all(|(_, kind)| *kind == teksilo_tokens::PointerKind::Touch),
                "and as touch, not as a synthesised mouse: {:?}",
                seen.phases
            );
        }
        recorder.borrow_mut().phases.clear();

        // -- (a2) the window system reached the translator
        //
        // X11 warps its virtual core pointer onto a live contact and reports
        // the motion through the same device a mouse uses, so a `CursorMoved`
        // arriving while a finger is down is a ghost and is dropped — but only
        // if the translator was told which window system it is on, which is
        // knowable solely from a live display handle. Leaving
        // `wire_translator`'s window-system supply out of window creation
        // leaves every window on `WindowSystem::Unknown`, and this move gets
        // through as a mouse.
        app.window_event(
            event_loop,
            window,
            touch(TouchPhase::Started, 8, 130.0, 95.0),
        );
        app.window_event(event_loop, window, cursor(131.0, 96.0));
        app.window_event(event_loop, window, touch(TouchPhase::Ended, 8, 130.0, 95.0));
        {
            let seen = recorder.borrow();
            assert!(
                !seen
                    .phases
                    .iter()
                    .any(|(_, kind)| *kind == teksilo_tokens::PointerKind::Mouse),
                "a cursor move while a contact is live is X11's ghost and must \
                 not reach the widget as a mouse: {:?}",
                seen.phases
            );
        }
        recorder.borrow_mut().phases.clear();

        // -- (c) window creation supplied the safe area
        //
        // Compared against the same platform function `create_window` reads,
        // mapped the same way. NOTE (stated in full in the module doc): on this
        // host that function answers zero, which is also what an untouched tree
        // answers — so this catches a wrong mapping, not an absent call, and
        // since this module is Linux-only it never catches the absent call.
        {
            let managed = &app.wm.windows_map()[&window];
            let expected = teksilo_platform::window_safe_area(managed.platform_window.window())
                .to_insets(managed.tree.layout_direction());
            assert_eq!(
                managed.tree.safe_area(),
                expected,
                "the tree should carry the window's own safe area"
            );
        }

        // -- (d) a widget is told this host's soft-keyboard row
        //
        // Through a real `WindowOpsImpl` built from the live window manager and
        // the live event loop — the sink a widget actually gets. Same caveat as
        // (c): `host_soft_keyboard_support()` is `None` on this host, which is
        // also the trait's default, so this catches a wrong row rather than a
        // deleted override — and since this module is Linux-only, no run of it
        // ever catches the deleted override.
        {
            let expected = crate::input_loop::host_soft_keyboard_support();
            assert_eq!(
                expected,
                SoftKeyboardSupport::None,
                "sanity: this host's row, so the caveat above is the right one"
            );
            let mut managed = app.wm.take_managed(window).expect("window");
            let id = managed.teksilo_id;
            let arc = Some(managed.platform_window.window_arc());
            let mut seen = None;
            {
                #[cfg(not(target_os = "macos"))]
                let handle = managed
                    .platform_window
                    .window()
                    .window_handle()
                    .ok()
                    .map(|h| h.as_raw());
                let mut ops = crate::window_manager::WindowOpsImpl::new(
                    &mut app.wm,
                    event_loop,
                    id,
                    #[cfg(not(target_os = "macos"))]
                    handle,
                    arc,
                );
                managed.tree.run_with_event_context(&mut ops, |ctx| {
                    seen = Some(ctx.soft_keyboard_support())
                });
            }
            app.wm.reinsert_managed(window, managed);
            assert_eq!(seen, Some(expected));
        }

        // -- (e) / (f) the theme's touch kill switch reaches a translator
        //
        // Three windows, because two of them are controls. A window created
        // inside this callback has never been laid out, so a contact aimed at
        // it would miss whatever the theme said — which would make the claim
        // below vacuous. `RedrawRequested` is what lays it out, and the first
        // of the three proves the arrangement can deliver a contact at all
        // before the second and third are asked to refuse one.

        /// Open a window, lay it out, aim three contacts at it, and answer
        /// what reached its widget.
        fn probe_new_window(
            app: &mut TeksiloAppHandler,
            event_loop: &ActiveEventLoop,
            title: &str,
            id: u64,
        ) -> usize {
            let log: Shared = Rc::new(RefCell::new(Default::default()));
            let seen = log.clone();
            let teksilo_id = app.wm.create_window(
                WindowConfig::new()
                    .title(title)
                    .size(300, 200)
                    .root(move |tree, _state| tree.add(logging_leaf(&log, false))),
                event_loop,
            );
            let winit_id = app
                .wm
                .winit_id_for_teksilo(teksilo_id)
                .expect("the window was just created");
            app.window_event(event_loop, winit_id, WindowEvent::RedrawRequested);
            seen.borrow_mut().phases.clear();
            for phase in [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended] {
                app.window_event(event_loop, winit_id, touch(phase, id, 80.0, 60.0));
            }
            seen.borrow().phases.len()
        }

        // Control: with the stock theme a fresh window does deliver contacts,
        // so a later count of zero means the switch and not the arrangement.
        assert_eq!(
            probe_new_window(app, event_loop, "teksilo loop test: control", 20),
            3,
            "a freshly created, laid-out window must deliver a contact — \
             without this the two assertions below prove nothing"
        );

        // (e) The translator holds the input tokens, not the theme, so a theme
        // swap has to re-install them on every open window or `touch_enabled`
        // is a field nothing reads.
        {
            let mut off = teksilo_core::presets::intui::light();
            off.input.touch_enabled = false;
            app.wm.set_theme(off);
        }
        recorder.borrow_mut().phases.clear();
        for phase in [TouchPhase::Started, TouchPhase::Moved, TouchPhase::Ended] {
            app.window_event(event_loop, window, touch(phase, 21, 120.0, 90.0));
        }
        assert!(
            recorder.borrow().phases.is_empty(),
            "`touch_enabled = false` must reach the already-open window's \
             translator: {:?}",
            recorder.borrow().phases
        );

        // (f) And a window created afterwards is born with it. This is
        // `wire_translator`'s token arm: a translator that was never handed the
        // theme's tokens runs on `InputTokens::default()`, where touch is on.
        assert_eq!(
            probe_new_window(app, event_loop, "teksilo loop test: touch off", 22),
            0,
            "a window created under a touch-off theme must be born with the \
             switch already thrown"
        );

        // -- (b) a window blocked by a modal child
        //
        // Open a real modal over it, then hand the *parent* three events and
        // watch which of them survive the guard in `handle_window_event_inner`.
        let parent_id = app.wm.windows_map()[&window].teksilo_id;
        app.wm.create_window(
            WindowConfig::new()
                .title("modal")
                .size(200, 120)
                .modal_to(parent_id)
                .root(|tree, _state| tree.add(logging_leaf(&Default::default(), false))),
            event_loop,
        );
        assert!(
            app.wm.is_blocked(parent_id),
            "the modal child should have blocked its parent"
        );

        // Input is swallowed: neither a motion sample nor a press reaches the
        // blocked parent's tree.
        recorder.borrow_mut().phases.clear();
        app.window_event(event_loop, window, cursor(150.0, 150.0));
        app.window_event(event_loop, window, click(ElementState::Pressed));
        app.window_event(event_loop, window, click(ElementState::Released));
        assert!(
            recorder.borrow().phases.is_empty(),
            "a blocked window must not deliver input to its widgets: {:?}",
            recorder.borrow().phases
        );

        // Non-input still arrives: a `Resized` the blocked window never saw
        // would leave its surface and its `WindowState` at the old size.
        let before = app.wm.windows_map()[&window].state.size().get();
        let scale = app.wm.windows_map()[&window].platform_window.scale_factor();
        let grown = winit::dpi::PhysicalSize::new(
            ((before.0 + 40) as f64 * scale).round() as u32,
            ((before.1 + 30) as f64 * scale).round() as u32,
        );
        app.window_event(event_loop, window, WindowEvent::Resized(grown));
        let after = app.wm.windows_map()[&window].state.size().get();
        assert_eq!(
            after,
            (before.0 + 40, before.1 + 30),
            "a blocked window still has to hear the OS resize it — reverting \
             `BlockedDisposition::Deliver` to a swallow leaves it at {before:?}"
        );
    });
}
