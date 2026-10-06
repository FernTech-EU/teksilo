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
//! Twenty claims, and they do not all carry the same weight. Verified by
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
//! - **(d)** a *wrong* soft-keyboard row is caught;
//! - **(g)** a window awaiting a redraw it requested does not make
//!   `update_control_flow` sleep until a deadline already due, so it cannot
//!   hold the loop awake;
//! - **(h)** a hidden window renders the one frame it owes on its way to
//!   hidden and then nothing; a due timer, through the loop's own
//!   `ResumeTimeReached` and `about_to_wait`, runs a non-visual tick that
//!   lays out; ticks run no more often than the hidden interval whatever asks
//!   for them, and a held one is not lost; and a window reported visible is
//!   shown and asks for a redraw even if it was also read as minimised;
//! - **(i)** a window whose redraw is withheld is ticked too, a tick ends in
//!   `post_event`, the cross-window paint pass does not re-arm a window that
//!   draws nothing, and its wake target is told it draws nothing on the next
//!   turn and told again once its redraw arrives;
//! - **(j)** a posted state wake asks a hidden window for a tick and never
//!   reaches the app's event handler;
//! - **(k)** no posted window wake reaches `on_app_event`, and a draw wake to a
//!   window that draws nothing is owed for later rather than ticked;
//! - **(l)** the window's waker is installed before its root is built;
//! - **(m)** on X11 a draw wake posts nothing and a burst of state wakes posts
//!   once;
//! - **(n)** `exiting` leaves every window's waker a no-op while the event
//!   loop still exists (checked on every run, in [`Script`]'s `exiting`), and
//!   a waker kept past the end of the loop wakes harmlessly (taking out all
//!   three disconnects reproduces winit's X11 panic);
//! - **(o)** a state wake to a window that draws runs a non-visual tick and
//!   asks winit for no redraw;
//! - **(p)** a screenshot through `capture_offscreen` shows a live picture's
//!   latest commit, counts as a capture rather than a frame drawn or
//!   displayed, and leaves the texture the next frame draws;
//! - **(q)** a window that moves reads its display's refresh rate again;
//! - **(r)** a window closing while its picture's producer commits at full
//!   speed detaches the attachment, so the commits wake nobody, and the loop
//!   polls the textures it held free without drawing a frame, waking within
//!   the poll interval while they wait;
//! - **(s)** a producer committing through the end of the loop does not panic.
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
//! **(q)** holds only on a display that reports its refresh rate; one that
//! reports none leaves the interval as it was, which is also what a missing
//! read leaves. And the read at window creation is unwitnessed: it differs
//! from the 60 Hz default only on a display that is not 60 Hz.
//!
//! Still unwitnessed by anything, here or elsewhere, and named so a reviewer
//! knows to read them rather than trust them: the accessibility waker's
//! installation in `create_window` and the handlers calling it ((j) posts the
//! wake itself; only a live assistive technology calls a handler), the pen
//! pump's per-turn call
//! (X11 has no pen path, so no shim is ever installed on this host), the
//! on-screen-keyboard poll and its apply (`Explicit` is a Windows-only row),
//! and the close-path contact release (the identity allocator it protects has
//! no observable side to assert on once the window is gone).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use teksilo_core::window::SoftKeyboardSupport;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowId;

use teksilo_canvas::live_image::{
    LiveImageDraw, LiveImageSource, LiveImageStats, LiveImageWriter, LivePixelFormat,
};
use teksilo_canvas::wake::{RedrawWaker, WakeKind};
use teksilo_canvas::{Canvas, ImageGeometry, Rect, Size, SizeProposal};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::{LiveImageAttachment, LiveImageSignals, WidgetId};

use super::{AppEvent, GPU_RECLAIM_POLL, TeksiloAppBuilder, TeksiloAppHandler, WindowWake};
use crate::input_routing::tests::{Shared, click, cursor, logging_leaf, touch};
use crate::redraw_gate::{TICK_INTERVAL, WITHHELD_AFTER};
use crate::window_config::WindowConfig;

/// Forwards every winit callback to the real handler, then runs `step` once —
/// on the first `about_to_wait`, which is the first moment a window exists and
/// an `&ActiveEventLoop` is in hand — and exits.
struct Script<'a, F> {
    app: &'a mut TeksiloAppHandler,
    step: Option<F>,
    /// `exiting` ran, and claim (n)'s check in it with it.
    exited: bool,
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

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.app.exiting(event_loop);
        // Claim (n), its first half: `exiting` alone leaves every window's
        // waker a no-op, while the event loop still exists, before the
        // driver's backstop and the window's `Drop` run.
        for managed in self.app.wm.iter() {
            let before = managed.platform_window.live_wake_stats().wakes;
            let waker = managed.platform_window.redraw_waker();
            waker.wake(WakeKind::Draw);
            waker.wake(WakeKind::Layout);
            assert_eq!(
                managed.platform_window.live_wake_stats().wakes,
                before,
                "a wake made after `exiting` reached its window"
            );
        }
        self.exited = true;
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
                exited: false,
            };
            event_loop
                .run_app_on_demand(&mut script)
                .expect("winit event loop exited with error");
            assert!(script.exited, "winit never called `exiting`");
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

/// Everything the claims need in one loop session: a 400 × 300 window whose
/// root is the same recording leaf the headless steps use. The root builder
/// also records the redraw waker its tree already holds, as a thin pointer.
fn app_with_recorder(log: &Shared, waker_at_build: &Rc<Cell<usize>>) -> TeksiloAppBuilder {
    let log = log.clone();
    let waker_at_build = waker_at_build.clone();
    TeksiloAppBuilder::new()
        .theme(teksilo_core::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("teksilo winit-loop test")
                .size(400, 300)
                .root(move |tree, _state| {
                    waker_at_build.set(
                        tree.redraw_waker()
                            .map_or(0, |waker| Arc::as_ptr(waker) as *const () as usize),
                    );
                    tree.add(logging_leaf(&log, false))
                }),
        )
}

/// Shows a live source at its frame size, attached in `build()` as any
/// live picture is: the size bound at `Relayout`, one quad per paint over
/// the widget's bounds.
#[derive(Debug)]
struct LivePicture {
    source: LiveImageSource,
    signals: LiveImageSignals,
    attachment: RefCell<Option<LiveImageAttachment>>,
}

impl LivePicture {
    fn new(source: &LiveImageSource) -> Self {
        Self {
            source: source.clone(),
            signals: LiveImageSignals::new(source),
            attachment: RefCell::new(None),
        }
    }
}

impl Widget for LivePicture {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let attachment = ctx.attach_live_image(&self.source, &self.signals);
        let id = ctx.self_id();
        self.signals
            .frame_size
            .bind_to(id, ctx.binding_registry(), BindingLevel::Relayout);
        *self.attachment.borrow_mut() = Some(attachment);
        vec![]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        let (w, h) = self.signals.frame_size.get().unwrap_or((0, 0));
        Size::new(w as f32, h as f32).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let (Some(attachment), Some(size)) = (
            self.attachment.borrow().as_ref(),
            self.signals.frame_size.get(),
        ) {
            let local = Rect::new(0.0, 0.0, bounds.width, bounds.height);
            attachment.set_geometry(ImageGeometry::new(size, Default::default(), local, local));
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        if let Some(attachment) = self.attachment.borrow().as_ref() {
            canvas.draw_live_image(attachment.consumer(), &LiveImageDraw::new(bounds, bounds));
        }
    }
}

/// A `w × h` frame of one opaque colour.
fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    rgba.repeat((w * h) as usize)
}

/// A source with one solid frame committed, and its writer.
fn solid_source(w: u32, h: u32, rgba: [u8; 4]) -> (LiveImageSource, LiveImageWriter) {
    let source = LiveImageSource::new(LivePixelFormat::Rgba8);
    let writer = source.writer();
    writer
        .write_frame(w, h, &solid(w, h, rgba), (w * 4) as usize)
        .expect("a solid frame");
    (source, writer)
}

/// Open a window whose root shows `source`, read as shown whatever the X
/// server says (a rootless Xwayland with no window manager reports its
/// windows fully obscured), and draw its first frame.
fn live_window(
    app: &mut TeksiloAppHandler,
    event_loop: &ActiveEventLoop,
    title: &str,
    source: &LiveImageSource,
) -> WindowId {
    let source = source.clone();
    let teksilo_id = app.wm.create_window(
        WindowConfig::new()
            .title(title)
            .size(200, 150)
            .root(move |tree, _state| tree.add(LivePicture::new(&source))),
        event_loop,
    );
    let window = app
        .wm
        .winit_id_for_teksilo(teksilo_id)
        .expect("the window was just created");
    let managed = app.wm.get_by_winit_mut(window).expect("the live window");
    managed.occluded = false;
    managed.minimized = false;
    managed.sync_hidden();
    managed.redraw.delivered();
    app.window_event(event_loop, window, WindowEvent::RedrawRequested);
    window
}

/// What the live window's one picture did.
fn picture_stats(app: &TeksiloAppHandler, window: WindowId) -> LiveImageStats {
    let managed = &app.wm.windows_map()[&window];
    managed
        .tree
        .live_image_stats(managed.tree.roots()[0])
        .expect("the root shows a live picture")
}

/// Commit frames to `writer` as fast as it takes them, until `stop`.
fn flood(writer: LiveImageWriter, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut shade = 0u8;
        while !stop.load(Ordering::Acquire) {
            shade = shade.wrapping_add(1);
            writer
                .write_frame(32, 24, &solid(32, 24, [shade, 0, 0, 255]), 32 * 4)
                .expect("a frame");
        }
    })
}

#[test]
#[ignore = "needs a display server and a wgpu adapter; CI runs it under Xvfb \
            (see the test-x11 job in .github/workflows/ci.yml)"]
fn the_real_callbacks_route_input_and_supply_the_platform_facts() {
    let log: Shared = Rc::new(RefCell::new(Default::default()));
    let recorder = log.clone();

    let app_events_seen = Rc::new(Cell::new(0_usize));
    let waker_at_build = Rc::new(Cell::new(0_usize));
    let saved_waker: Rc<RefCell<Option<Arc<dyn RedrawWaker>>>> = Rc::default();
    let saved = saved_waker.clone();
    // Claim (s): a producer still committing when the loop ends.
    let exit_flood: Rc<
        RefCell<
            Option<(
                LiveImageSource,
                Arc<AtomicBool>,
                std::thread::JoinHandle<()>,
            )>,
        >,
    > = Rc::default();
    let exit_flood_in = exit_flood.clone();
    let counter = app_events_seen.clone();
    let builder = app_with_recorder(&log, &waker_at_build).on_app_event(move |event| {
        if matches!(event, AppEvent::External(_)) {
            counter.set(counter.get() + 1);
        }
    });
    drive(builder, move |app, event_loop| {
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

        // -- (g) a window awaiting its redraw does not hold the loop awake
        //
        // A deadline already due, as a tween or tooltip dwell has when its
        // window stops getting frames, on a window that has asked for a redraw
        // winit has not delivered yet (on Wayland, a frame callback still to
        // come). Counting that deadline as is would put the loop to sleep
        // until a moment already past, which wakes it at once, forever.
        // Taking the outstanding-request hold out of `RedrawGate::deadline`
        // reddens the first half. The window's last frame is let age first,
        // so the hold that rate-limits ticks is not what holds the deadline.
        {
            std::thread::sleep(TICK_INTERVAL);
            let past = Instant::now() - Duration::from_millis(50);
            {
                let managed = app.wm.get_by_winit_mut(window).expect("the window");
                // Shown, whatever the X server said: a hidden window's
                // deadlines are held to the hidden tick interval, which is
                // claim (h), and would mask this one. A rootless Xwayland with
                // no window manager reports its windows fully obscured.
                managed.occluded = false;
                managed.minimized = false;
                managed.sync_hidden();
                managed.redraw.delivered();
                managed.tree.request_wake_at(past);
                managed.request_redraw();
            }
            app.update_control_flow(event_loop);
            if let ControlFlow::WaitUntil(t) = event_loop.control_flow() {
                assert!(
                    t > past,
                    "a window awaiting its redraw must not put the loop to sleep until \
                     a deadline that has already passed"
                );
            }
            // Once the redraw arrives the deadline counts again, so the half
            // above is not passing because the deadline was never there.
            app.wm
                .get_by_winit_mut(window)
                .expect("the window")
                .redraw
                .delivered();
            app.update_control_flow(event_loop);
            match event_loop.control_flow() {
                ControlFlow::WaitUntil(t) => assert!(t <= past, "the due wake drives the loop"),
                other => panic!("expected a WaitUntil for the due wake, got {other:?}"),
            }
        }

        // -- (h) a hidden window keeps its state current and draws nothing
        //
        // Occlusion is injected as winit would deliver it on X11; a window
        // manager would be needed to iconify for real, and Xvfb with openbox
        // has one but a developer's private Xwayland does not. The timer path
        // is driven as the loop drives it: a `ResumeTimeReached` wake, then
        // `about_to_wait`.
        {
            app.window_event(event_loop, window, WindowEvent::Occluded(true));
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(managed.redraw.is_hidden(), "an occluded window is hidden");

            // The first redraw is the frame owed on the way to hidden: it
            // renders. Every later one runs the non-visual frame only, so a
            // paint mark survives it.
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            let root = managed.tree.roots()[0];
            managed.tree.mark_needs_paint(root);
            let last_frame = Instant::now();
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
            assert!(
                app.wm.windows_map()[&window].tree.needs_paint(),
                "a hidden window must not render past its transition frame"
            );

            // A due timer, and nothing asking for a frame by hand. The window
            // wakes the loop at the hidden interval after its last frame, no
            // sooner and no later. (Its own deadline, not the loop's: the
            // windows (e) and (f) opened are still there, waiting for redraws
            // this callback never lets winit deliver.)
            let past = Instant::now() - Duration::from_millis(50);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            managed.tree.request_wake_at(past);
            let wake = managed
                .wake_deadline(Instant::now())
                .expect("a hidden window's due timer must wake the loop");
            assert!(
                wake >= last_frame + TICK_INTERVAL,
                "a hidden window is ticked no faster than its interval"
            );
            assert!(
                wake <= Instant::now() + TICK_INTERVAL,
                "and its hold runs from its last frame, not from each computation"
            );

            // At that wake the timer is found due and becomes a tick, which
            // lays out (consuming the wake) and does not render. Testing the
            // due timer against the held deadline instead of the tree's own
            // finds the window never due, and reddens this.
            std::thread::sleep(wake.saturating_duration_since(Instant::now()));
            app.new_events(
                event_loop,
                StartCause::ResumeTimeReached {
                    start: Instant::now(),
                    requested_resume: wake,
                },
            );
            app.about_to_wait(event_loop);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(
                managed.tree.next_timer_deadline().is_none_or(|t| t > past),
                "the hidden window's due timer ran a tick, whose layout consumed it"
            );
            assert!(managed.tree.needs_paint(), "the tick drew nothing");

            // A request right after that tick, as any event can make, is held
            // to the interval: the tick it asks for does not run yet, and the
            // loop wakes for it when the interval is up.
            let ticked_at = Instant::now();
            let past = ticked_at - Duration::from_millis(10);
            managed.tree.request_wake_at(past);
            managed.request_redraw();
            app.about_to_wait(event_loop);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(
                managed
                    .tree
                    .next_timer_deadline()
                    .is_some_and(|t| t <= past),
                "a hidden window ticks at most once per interval, whatever asks"
            );
            let held = managed
                .wake_deadline(Instant::now())
                .expect("a held tick must not be lost");
            assert!(
                held > ticked_at && held <= Instant::now() + TICK_INTERVAL,
                "the held tick wakes the loop when the interval is up"
            );

            // Shown again, the window asks for a redraw, even if a probe had
            // also read it as minimised: a restore without focus sends no
            // `Focused(true)` or `Resized` to clear that.
            app.wm
                .get_by_winit_mut(window)
                .expect("the window")
                .minimized = true;
            app.window_event(event_loop, window, WindowEvent::Occluded(false));
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(
                !managed.redraw.is_hidden(),
                "a window reported visible is shown"
            );
            assert!(
                managed.redraw.awaits_redraw(),
                "the reveal asks winit for a redraw"
            );
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
        }

        // -- (i) a window whose redraw is withheld keeps its state current
        //
        // What a Wayland compositor does to a window it does not show: the
        // redraw requested stays undelivered. winit on X11 never withholds
        // one, so the stamp is made directly, as if the request had been made
        // long enough ago, and winit is not asked.
        {
            std::thread::sleep(TICK_INTERVAL);
            let past = Instant::now() - Duration::from_millis(10);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(!managed.redraw.is_hidden());
            // Whatever the reveal frame asked for is let go first, so the
            // stamp below is the request's first.
            managed.redraw.delivered();
            managed
                .redraw
                .request_with(Instant::now() - WITHHELD_AFTER, || {});
            managed.tree.request_wake_at(past);
            let root = managed.tree.roots()[0];
            managed.tree.mark_needs_paint(root);
            // A window command waits for `post_event`, which `about_to_wait`
            // does not run on its own: only a tick that ran does.
            managed
                .state
                .title()
                .set("teksilo winit-loop test (ticked)".to_owned());
            app.about_to_wait(event_loop);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(
                managed.tree.next_timer_deadline().is_none_or(|t| t > past),
                "a withheld window still runs a tick, whose layout consumed the wake"
            );
            // The tick ended in `post_event`, whose cross-window pass skips a
            // window that draws nothing: asking it again for its paint would
            // re-arm its tick after every one.
            assert!(
                !managed
                    .redraw
                    .take_tick(Instant::now() + Duration::from_secs(1)),
                "a window that draws nothing is not asked again for its paint"
            );
            assert!(
                managed.redraw.awaits_redraw(),
                "the tick is not the withheld redraw"
            );
            assert!(
                managed.platform_window.is_hidden(),
                "the wake target knows the window draws nothing, so its draw \
                 wakes are owed rather than lost in a request winit holds"
            );
            assert!(
                managed.state.drain_os_commands().is_empty(),
                "a tick ends in `post_event`, which applies what the window queued"
            );
            // Its redraw arrives at last and draws.
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
            assert!(!app.wm.windows_map()[&window].tree.needs_paint());
            assert!(
                !app.wm.windows_map()[&window].platform_window.is_hidden(),
                "drawing again, it takes draw wakes again"
            );
        }

        // -- (j) an accessibility handler's wake reaches a window that draws
        // nothing, and is the framework's alone
        //
        // Posted as the handlers post it. On a hidden window it asks for a
        // tick through the gate; no app event handler sees it.
        {
            app.window_event(event_loop, window, WindowEvent::Occluded(true));
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
            let seen_before = app_events_seen.get();
            app.user_event(
                event_loop,
                AppEvent::External(Box::new(WindowWake {
                    window,
                    kind: WakeKind::Layout,
                })),
            );
            assert_eq!(
                app_events_seen.get(),
                seen_before,
                "the accessibility wake is not an app event"
            );
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert!(
                managed.redraw.take_tick(Instant::now() + TICK_INTERVAL),
                "it asked the hidden window for a tick"
            );
            app.window_event(event_loop, window, WindowEvent::Occluded(false));
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
        }

        // -- (k) a posted window wake is the framework's alone, and a draw wake
        // to a window that draws nothing is owed, not ticked
        //
        // Taking the interception below the app's handler, or answering a draw
        // wake with a gated request instead of deferring it, reddens this.
        {
            let seen_before = app_events_seen.get();
            for kind in [WakeKind::Draw, WakeKind::Layout] {
                app.user_event(
                    event_loop,
                    AppEvent::External(Box::new(WindowWake { window, kind })),
                );
            }
            assert_eq!(
                app_events_seen.get(),
                seen_before,
                "a window wake never reaches on_app_event"
            );
            assert!(
                app.wm.windows_map()[&window].redraw.awaits_redraw(),
                "a shown window answers with a redraw request"
            );

            app.window_event(event_loop, window, WindowEvent::Occluded(true));
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
            let far = Instant::now() + Duration::from_secs(60);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            let _ = managed.redraw.take_tick(far);
            let dropped = managed.platform_window.live_wake_stats().dropped_hidden;
            app.user_event(
                event_loop,
                AppEvent::External(Box::new(WindowWake {
                    window,
                    kind: WakeKind::Draw,
                })),
            );
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            assert_eq!(
                managed.platform_window.live_wake_stats().dropped_hidden,
                dropped + 1,
                "a draw wake to a window that draws nothing is owed for later"
            );
            assert!(
                !managed.redraw.take_tick(far),
                "and asks for no tick: it changes nothing the window keeps current"
            );
            app.window_event(event_loop, window, WindowEvent::Occluded(false));
            app.window_event(event_loop, window, WindowEvent::RedrawRequested);
        }

        // -- (l) the window's waker is installed before its root is built
        //
        // So a source a widget attaches in `build()` already wakes the window.
        // Installing it after the root builder reddens this.
        {
            let managed = &app.wm.windows_map()[&window];
            let installed = managed.platform_window.redraw_waker();
            assert_eq!(
                waker_at_build.get(),
                Arc::as_ptr(&installed) as *const () as usize,
                "the root builder saw the window's own waker"
            );
            *saved.borrow_mut() = Some(installed);
        }

        // -- (m) the routes, as the X11 backend gets them: a draw wake asks
        // winit directly and posts nothing; a burst of state wakes posts once
        //
        // Taking the state route out leaves nothing posted.
        {
            let managed = &app.wm.windows_map()[&window];
            let waker = managed.platform_window.redraw_waker();
            assert!(!waker.is_hidden());
            let before = managed.platform_window.live_wake_stats().wakes;
            let _ = managed.platform_window.take_posted_wake(WakeKind::Layout);
            std::thread::spawn(move || {
                for _ in 0..100 {
                    waker.wake(WakeKind::Draw);
                    waker.wake(WakeKind::Layout);
                }
            })
            .join()
            .expect("waking a window from another thread");
            assert_eq!(
                managed.platform_window.live_wake_stats().wakes,
                before + 200
            );
            assert!(
                !managed.platform_window.take_posted_wake(WakeKind::Draw),
                "no draw wake is posted on X11"
            );
            assert!(
                managed.platform_window.take_posted_wake(WakeKind::Layout),
                "the state wakes posted"
            );
            assert!(
                !managed.platform_window.take_posted_wake(WakeKind::Layout),
                "once for the burst"
            );
        }

        // -- (o) a state wake to a window that draws asks for a tick, not a
        // redraw
        //
        // What it carries may change nothing on screen (a terminal printing in
        // a background tab); a redraw would draw and present the same frame.
        // The tick still lays out. Answering with a redraw request reddens the
        // first assertion.
        {
            std::thread::sleep(TICK_INTERVAL);
            let past = Instant::now() - Duration::from_millis(10);
            let managed = app.wm.get_by_winit_mut(window).expect("the window");
            managed.redraw.delivered();
            managed.tree.request_wake_at(past);
            app.user_event(
                event_loop,
                AppEvent::External(Box::new(WindowWake {
                    window,
                    kind: WakeKind::Layout,
                })),
            );
            let managed = &app.wm.windows_map()[&window];
            assert!(
                !managed.redraw.awaits_redraw(),
                "a state wake asks winit for no redraw"
            );
            app.about_to_wait(event_loop);
            let managed = &app.wm.windows_map()[&window];
            assert!(
                managed.tree.next_timer_deadline().is_none_or(|t| t > past),
                "the state tick laid out, which consumed the due wake"
            );
            assert!(!managed.redraw.awaits_redraw(), "and drew nothing");
        }

        // -- (p) a screenshot shows a live picture's latest commit, and is not
        // one of the window's frames
        //
        // Through `capture_offscreen`, the path the automation bridge takes.
        // Rendering it with `render` rather than `render_capture` counts the
        // capture as a frame drawn and a frame displayed, which reddens this.
        {
            let (source, writer) = solid_source(16, 12, [255, 0, 0, 255]);
            let picture = live_window(app, event_loop, "teksilo loop test: live", &source);
            let first = picture_stats(app, picture);
            assert_eq!(
                (first.attachment.frames_drawn, first.attachment.uploads),
                (1, 1),
                "the window's first frame drew and uploaded the picture"
            );
            assert_eq!(source.displayed_generation(), 1);

            writer
                .write_frame(16, 12, &solid(16, 12, [0, 0, 255, 255]), 16 * 4)
                .expect("a blue frame");
            let managed = app.wm.get_by_winit_mut(picture).expect("the live window");
            let root = managed.tree.roots()[0];
            let scale = managed.tree.device_scale_factor();
            let b = managed.tree.bounds(root);
            let crop = Rect::new(b.x * scale, b.y * scale, b.width * scale, b.height * scale);
            let frame = managed.tree.render();
            let shot = managed
                .platform_window
                .capture_offscreen(&frame, [0.0; 4], Some(crop))
                .expect("the window's device reads back");
            assert_eq!(
                (shot.region.x, shot.region.y, shot.width, shot.height),
                (
                    crop.x.floor() as u32,
                    crop.y.floor() as u32,
                    (crop.x + crop.width).ceil() as u32 - crop.x.floor() as u32,
                    (crop.y + crop.height).ceil() as u32 - crop.y.floor() as u32
                ),
                "the capture is the widget's box, in surface pixels"
            );
            assert_eq!(shot.rgba.len(), (shot.width * shot.height * 4) as usize);
            let centre = ((shot.height / 2 * shot.width + shot.width / 2) * 4) as usize;
            assert_eq!(
                &shot.rgba[centre..centre + 4],
                &[0, 0, 255, 255],
                "the screenshot shows the commit no frame has drawn yet"
            );
            let captured = picture_stats(app, picture);
            assert_eq!(captured.attachment.captures, 1);
            assert_eq!(
                captured.attachment.frames_drawn, 1,
                "a capture is not a frame drawn"
            );
            assert_eq!(captured.attachment.uploads, 2);
            assert_eq!(
                source.displayed_generation(),
                1,
                "nor a frame displayed: the producer's commit 2 is not on screen"
            );

            // The window's next frame draws the texture the capture filled.
            app.window_event(event_loop, picture, WindowEvent::RedrawRequested);
            let shown = picture_stats(app, picture);
            assert_eq!(shown.attachment.frames_drawn, 2);
            assert_eq!(shown.attachment.uploads, 2, "nothing left to upload");
            assert_eq!(source.displayed_generation(), 2);
            assert_eq!(
                app.wm.windows_map()[&picture]
                    .platform_window
                    .live_texture_stats()
                    .textures,
                1
            );

            // -- (q) the window follows its display's refresh rate
            //
            // A move may change displays: it reads the rate again. Taking the
            // read out of the `Moved` arm reddens this, on a display that
            // reports a rate.
            let managed = app.wm.get_by_winit_mut(picture).expect("the live window");
            managed
                .platform_window
                .renderer_mut()
                .set_live_refresh_interval(Duration::from_secs(1));
            app.window_event(
                event_loop,
                picture,
                WindowEvent::Moved(winit::dpi::PhysicalPosition::new(0, 0)),
            );
            let managed = &app.wm.windows_map()[&picture];
            let rate = managed
                .platform_window
                .window()
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz());
            let interval = managed.platform_window.renderer().live_refresh_interval();
            match rate {
                Some(millihertz) => assert_eq!(
                    interval,
                    Duration::from_nanos(
                        1_000_000_000_000 / u64::from(millihertz.clamp(20_000, 1_000_000))
                    ),
                    "one refresh of the display the window is on"
                ),
                None => assert_eq!(
                    interval,
                    Duration::from_secs(1),
                    "a display that reports no rate keeps the last one"
                ),
            }
            let teksilo_id = app.wm.windows_map()[&picture].teksilo_id;
            app.wm.close_window(teksilo_id);
            drop(writer);
        }

        // -- (r) a window closing while its picture's producer commits at
        // full speed, and the textures it held freed without a frame
        //
        // The tree goes with the window, so its attachment detaches and the
        // producer's commits wake nobody. Its renderer goes too and flags the
        // device; the loop polls it until the GPU has freed the textures, and
        // draws nothing for it. Taking the poll out of `update_control_flow`
        // leaves the device flagged forever; taking the reclaim term out of
        // the control flow lets the loop sleep with it flagged.
        {
            let (source, writer) = solid_source(32, 24, [0, 255, 0, 255]);
            let closing = live_window(app, event_loop, "teksilo loop test: closing", &source);
            assert_eq!(picture_stats(app, closing).attachment.frames_drawn, 1);
            let stop = Arc::new(AtomicBool::new(false));
            let producer = flood(writer, stop.clone());
            let started = source.generation();
            while source.generation() < started + 50 {
                std::thread::yield_now();
            }
            // Every reclaim already recorded is polled empty first, so the
            // count below is this close's.
            while teksilo_render::poll_gpu_reclaim() {
                std::thread::sleep(Duration::from_millis(1));
            }
            let teksilo_id = app.wm.windows_map()[&closing].teksilo_id;
            app.wm.close_window(teksilo_id);
            let stats = source.stats();
            assert_eq!(
                (stats.attachments, stats.observed_attachments),
                (0, 0),
                "the closed window's attachment detached"
            );
            let wakes = stats.wakes;
            let after_close = source.generation();
            while source.generation() < after_close + 100 {
                std::thread::yield_now();
            }
            assert_eq!(
                source.stats().wakes,
                wakes,
                "commits after the close wake nobody"
            );
            assert_eq!(
                teksilo_render::gpu_reclaim::pending_reclaims(),
                1,
                "the closed window's renderer flagged its device"
            );

            // The reclaim term, alone: with the poll reporting textures
            // still on the GPU, the loop wakes within the poll interval, and
            // without it, nothing else would wake it that soon.
            app.gpu_reclaim = || false;
            let before = Instant::now();
            app.update_control_flow(event_loop);
            if let ControlFlow::WaitUntil(t) = event_loop.control_flow() {
                assert!(
                    t > before + GPU_RECLAIM_POLL,
                    "sanity: nothing else wakes the loop within the poll interval"
                );
            }
            app.gpu_reclaim = || true;
            app.update_control_flow(event_loop);
            let after = Instant::now();
            match event_loop.control_flow() {
                ControlFlow::WaitUntil(t) => assert!(
                    t <= after + GPU_RECLAIM_POLL,
                    "textures waiting to be freed wake the loop to poll for them"
                ),
                other => panic!("expected a WaitUntil for the reclaim poll, got {other:?}"),
            }
            app.gpu_reclaim = teksilo_render::poll_gpu_reclaim;

            // The real poll, through the loop's own turn and no frame.
            let deadline = Instant::now() + Duration::from_secs(5);
            while teksilo_render::gpu_reclaim::pending_reclaims() != 0 {
                assert!(
                    Instant::now() < deadline,
                    "the loop never polled the closed window's textures free"
                );
                app.about_to_wait(event_loop);
                std::thread::sleep(Duration::from_millis(1));
            }
            stop.store(true, Ordering::Release);
            producer.join().expect("the producer outlived its window");
        }

        // -- (s) the loop ending while a producer commits: see after the loop
        {
            let (source, writer) = solid_source(32, 24, [255, 255, 0, 255]);
            let staying = live_window(app, event_loop, "teksilo loop test: exiting", &source);
            assert_eq!(picture_stats(app, staying).attachment.frames_drawn, 1);
            let stop = Arc::new(AtomicBool::new(false));
            let producer = flood(writer, stop.clone());
            *exit_flood_in.borrow_mut() = Some((source, stop, producer));
        }

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

    // -- (n) a waker outlives the event loop harmlessly
    //
    // winit's X11 `request_redraw` unwraps a send to the event loop, which
    // panics once the loop is gone. The window's waker is disconnected three
    // times over (`exiting`, the driver's backstop, the window's `Drop`);
    // taking all three out makes this wake panic its thread.
    let waker = saved_waker
        .borrow_mut()
        .take()
        .expect("claim (l) saved the window's waker");
    std::thread::spawn(move || {
        waker.wake(WakeKind::Draw);
        waker.wake(WakeKind::Layout);
    })
    .join()
    .expect("a wake after the event loop ended must not panic");

    // -- (s) a producer that commits through the end of the loop
    //
    // It woke its window at every commit until `exiting` disconnected the
    // waker, kept committing while the windows dropped, and commits on now
    // with nobody attached. A wake reaching winit after the loop panics the
    // producer's thread on X11.
    let (source, stop, producer) = exit_flood
        .borrow_mut()
        .take()
        .expect("claim (s) started its producer");
    let ended = source.generation();
    while source.generation() < ended + 100 {
        std::thread::yield_now();
    }
    assert_eq!(
        source.stats().attachments,
        0,
        "the windows' attachments went with them"
    );
    stop.store(true, Ordering::Release);
    producer
        .join()
        .expect("a producer committing as the loop ended must not panic");
}
