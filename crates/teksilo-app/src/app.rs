// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use teksilo_canvas::SizeProposal;
use teksilo_core::Theme;
use teksilo_core::app_event::AppEvent;
use teksilo_core::event::WidgetEvent;
use teksilo_core::event_source::{
    AppEventPoster, EventSource, EventSourceAdapter, SubscriptionId, TreeAppContext,
};
use teksilo_core::modal::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
use teksilo_core::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use teksilo_core::{WidgetId, WidgetTree};
use teksilo_i18n::{I18nConfig, I18nManager, LanguageIdentifier};
use teksilo_platform::event_translation;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
#[allow(unused_imports)]
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowId;

/// How the application resolves its theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Use a specific fixed theme (current behavior, default).
    #[default]
    Manual,
    /// Follow the OS light/dark preference using Teksilo's built-in themes.
    FollowSystem,
    /// Adopt colors read directly from the OS/DE config files (GNOME/KDE/Cinnamon).
    /// Falls back to `FollowSystem` on unsupported platforms or DEs.
    Native,
}

#[cfg(feature = "text")]
use teksilo_text::SharedTypesetter;

use crate::window_config::{SizeToContent, TeksiloWindowId, WindowConfig};
use crate::window_manager::WindowManager;
use teksilo_core::WindowPlacement;

/// Interrogate the winit window for its current placement so an
/// `OS-initiated` state change can be mirrored into the corresponding
/// `WindowState::placement` signal without the observer pushing it
/// back out as a `WindowCommand` (re-entrancy guard on `from_os`).
fn query_window_placement(win: &winit::window::Window) -> WindowPlacement {
    if win.is_minimized() == Some(true) {
        WindowPlacement::Minimized
    } else if win.fullscreen().is_some() {
        WindowPlacement::Fullscreen
    } else if win.is_maximized() {
        WindowPlacement::Maximized
    } else {
        WindowPlacement::Floating
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedModalPresentation {
    InTree,
    NativeWindow,
}

/// Generate a per-process random session id for telemetry.
///
/// Not persisted across restarts — by design (a stable id would be
/// pseudonymous tracking, distinct from `InstallId`'s 13-month UUID).
/// The first 16 hex chars of a fresh UUID are sufficient for grouping
/// events within one process lifetime.
#[cfg(feature = "telemetry")]
fn generate_session_id() -> String {
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    uuid[..16].to_string()
}

fn resolve_modal_presentation(
    requested: ModalPresentation,
    content: &ModalContent,
    native_supported: bool,
) -> ResolvedModalPresentation {
    let can_use_native = native_supported && matches!(content, ModalContent::Deferred(_));

    match requested {
        ModalPresentation::InTree => ResolvedModalPresentation::InTree,
        ModalPresentation::NativeWindow => {
            if can_use_native {
                ResolvedModalPresentation::NativeWindow
            } else {
                ResolvedModalPresentation::InTree
            }
        }
        ModalPresentation::Auto => {
            if can_use_native {
                ResolvedModalPresentation::NativeWindow
            } else {
                ResolvedModalPresentation::InTree
            }
        }
    }
}

fn modal_close_behavior_to_overlay_dismiss(behavior: ModalCloseBehavior) -> DismissBehavior {
    match behavior {
        ModalCloseBehavior::ClickOutside => DismissBehavior::ClickOutside,
        ModalCloseBehavior::EscapeKey => DismissBehavior::EscapeKey,
        ModalCloseBehavior::EscapeOrClickOutside => DismissBehavior::EscapeOrClickOutside,
        ModalCloseBehavior::Manual => DismissBehavior::Manual,
    }
}

/// Whether a modal presented as a **native OS window** dismisses on Escape.
///
/// The native presentation can honour only the Escape half of
/// [`ModalCloseBehavior`]: while the modal is up the parent window is
/// input-blocked by the OS, so there is no "outside" left to click. So
/// `ClickOutside` resolves to `false` here — the same answer as `Manual` —
/// rather than to a silently different second route.
fn native_modal_escape_dismisses(behavior: ModalCloseBehavior) -> bool {
    match behavior {
        ModalCloseBehavior::EscapeKey | ModalCloseBehavior::EscapeOrClickOutside => true,
        ModalCloseBehavior::ClickOutside | ModalCloseBehavior::Manual => false,
    }
}

/// Splice the Escape route around a native modal's root and return the new root.
///
/// Bubble, not preview: a focused descendant — and any overlay it owns, which
/// the tree dismisses before dispatch even begins — gets first refusal on
/// Escape, exactly as it does inside an in-tree modal. Only an unclaimed
/// Escape reaches this wrapper, which asks the window manager to close the
/// modal window.
fn wrap_native_modal_in_escape_route(tree: &mut WidgetTree, content_id: WidgetId) -> WidgetId {
    use teksilo_core::widget_builder::WidgetBuilder as _;
    tree.add(
        teksilo_widgets::ZStack::new()
            .child(content_id)
            .on_key(|event, ctx| match event {
                teksilo_core::event::WidgetEvent::KeyDown {
                    key: teksilo_core::event::Key::Escape,
                    ..
                } => {
                    ctx.dismiss_modal();
                    teksilo_core::event::EventResponse::Handled
                }
                _ => teksilo_core::event::EventResponse::Ignored,
            }),
    )
}

fn present_in_tree_modal_request(
    tree: &mut WidgetTree,
    source_widget: WidgetId,
    request: ModalRequest,
) {
    let dismiss = modal_close_behavior_to_overlay_dismiss(request.close_behavior);
    let requested_focus = request.focus_target;
    let user_on_dismiss = request.on_dismiss;
    let close_behavior = request.close_behavior;
    // Capture the focus owner BEFORE the modal moves focus into itself
    // (below). Recorded as the modal overlay's `focus_restore` so that
    // dismissing the dialog returns keyboard focus to the trigger — e.g.
    // tabbing to a "Rename…" button, opening the InputDialog, then
    // accepting/cancelling lands back on that button. Without this, the
    // modal shows via `show_overlay` (which, unlike
    // `show_overlay_from_source`, records no restore target) and focus is
    // dropped on dismiss.
    let focus_before_modal = tree.focused();
    // Capture the `:focus-visible` input modality at the same instant. A
    // modal is a transient interruption: when it closes and focus snaps
    // back to the trigger, the trigger's focus ring should look exactly as
    // it did before the modal opened — NOT inherit keyboard modality from
    // input directed *at the dialog* (typing a name, pressing Enter to
    // accept). Without restoring this, mouse-clicking the trigger then
    // pressing Enter inside the dialog leaves the global modality "keyboard"
    // and the trigger sprouts a focus ring it never had. We restore it on
    // dismiss alongside focus. (`focus_ops` itself never touches this
    // signal, so the value we restore is the value that sticks.)
    let focus_visible_before = tree.focus_visible_signal().get();
    let focus_visible_signal = tree.focus_visible_signal();
    let content_id = match request.content {
        ModalContent::ExistingWidget(id) => id,
        ModalContent::Deferred(builder) => {
            let id = builder(tree);
            tree.set_dormant(id);
            id
        }
    };

    // Mount the dialog scrim FIRST so it z-orders below the modal
    // panel in the overlay stack. The scrim chrome (a full-viewport
    // dim) comes from the active `DialogStyle::make_scrim`; clicks on
    // it dismiss the modal when its `ModalCloseBehavior` permits
    // click-outside dismissal. The framework patches the scrim's
    // `parent_overlay` after the modal is pushed so that dismissing
    // the modal cascades through and also dismisses the scrim.
    let click_to_dismiss = matches!(
        close_behavior,
        ModalCloseBehavior::ClickOutside | ModalCloseBehavior::EscapeOrClickOutside,
    );
    let dismiss_target: std::rc::Rc<std::cell::Cell<Option<teksilo_core::overlay::OverlayId>>> =
        std::rc::Rc::new(std::cell::Cell::new(None));
    let scrim_id = tree.add(
        teksilo_widgets::ModalScrim::new()
            .dismiss_target(dismiss_target.clone())
            .click_to_dismiss(click_to_dismiss),
    );
    let scrim_overlay = tree.show_overlay(OverlayRequest {
        content_id: scrim_id,
        anchor: source_widget,
        placement: OverlayPlacement::FullViewport,
        dismiss: DismissBehavior::Manual,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
        fade_duration: None,
    });

    tree.activate(content_id);
    // Wrap the caller's `on_dismiss` so the framework also restores the
    // pre-modal `:focus-visible` modality when the dialog closes (by any
    // path: OK, Cancel, Escape, click-outside). Only when a focus owner
    // was captured — if nothing was focused before, there's no prior state
    // to return to. The overlay fires `on_dismiss` during dismissal, just
    // before focus is restored to the trigger, so the value we set here is
    // the one the trigger paints with.
    let restore_modality = focus_before_modal.is_some();
    let on_dismiss: Option<teksilo_core::overlay::OverlayDismissCallback> =
        if restore_modality || user_on_dismiss.is_some() {
            Some(std::rc::Rc::new(
                move |reason, ctx: &mut teksilo_core::widget::EventContext| {
                    if restore_modality {
                        focus_visible_signal.set(focus_visible_before);
                    }
                    if let Some(cb) = &user_on_dismiss {
                        cb(reason, ctx);
                    }
                },
            ))
        } else {
            None
        };
    // Present the modal as a WINDOW-LEVEL overlay via `show_overlay` rather than
    // `show_overlay_from_source`: the latter re-parents the overlay to the source
    // widget's overlay ancestor, so a modal opened from a menu item would be
    // trapped in (and positioned relative to) the transient menu overlay instead
    // of centering on the window. `Centered` already ignores the anchor; keeping
    // `parent_overlay: None` makes it center on the viewport.
    let modal_overlay = tree.show_overlay(OverlayRequest {
        content_id,
        anchor: source_widget,
        placement: OverlayPlacement::Centered,
        dismiss,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss,
        fade_duration: None,
    });
    // The modal is now the topmost overlay; record where focus should
    // return when it dismisses. Mirrors `show_overlay_from_source`'s
    // capture-then-set-top pattern. The `is_active` guard on the restore
    // side makes a stale id (e.g. a menu trigger that went dormant) a
    // graceful no-op.
    if let Some(restore) = focus_before_modal {
        tree.overlay_manager_mut().set_top_focus_restore(restore);
    }
    // Cascade-dismiss the scrim when the modal is dismissed (by any
    // path: Escape, click-outside, manual). The scrim is below the
    // modal in the stack but counts as its "child" in the parent-
    // overlay graph, so `dismiss_immediate` walks the descendants and
    // dismisses it too.
    tree.overlay_manager_mut()
        .set_parent_overlay(scrim_overlay, Some(modal_overlay));
    // Fill in the dismiss target NOW that the modal id is known. The
    // scrim's on-tap reads through this `Cell` at click time.
    dismiss_target.set(Some(modal_overlay));

    let focus_target = requested_focus
        .filter(|id| tree.is_active(*id) && tree.is_descendant_of(*id, content_id))
        .or_else(|| tree.widget_initial_focus_hint(content_id))
        .or_else(|| tree.first_focusable_descendant(content_id));
    if let Some(id) = focus_target {
        tree.focus(id);
    }
}

/// How often the on-screen keyboard's rectangle is re-read. See
/// [`TeksiloAppHandler::refresh_occluded_band`].
const OSK_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Convert a screen-space physical rectangle into the part of `window`'s client
/// area it covers, in window-logical pixels.
///
/// `None` when the rectangle misses the window entirely, which is the common
/// case for every window a keyboard is not in front of.
fn occluded_band_in_window(
    window: &winit::window::Window,
    screen: (i32, i32, i32, i32),
) -> Option<teksilo_canvas::Rect> {
    let origin = window.inner_position().ok()?;
    let size = window.inner_size();
    occluded_band_in_client(
        (origin.x, origin.y),
        (size.width, size.height),
        window.scale_factor() as f32,
        screen,
    )
}

/// The arithmetic behind [`occluded_band_in_window`], with the window replaced
/// by the three numbers it contributes — so the clipping can be checked on a
/// host with no keyboard, no display and no window.
fn occluded_band_in_client(
    client_origin: (i32, i32),
    client_size: (u32, u32),
    scale_factor: f32,
    (left, top, right, bottom): (i32, i32, i32, i32),
) -> Option<teksilo_canvas::Rect> {
    if scale_factor <= 0.0 {
        return None;
    }
    let x0 = (left - client_origin.0).max(0) as f32;
    let y0 = (top - client_origin.1).max(0) as f32;
    let x1 = (right - client_origin.0).min(client_size.0 as i32) as f32;
    let y1 = (bottom - client_origin.1).min(client_size.1 as i32) as f32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(teksilo_canvas::Rect::new(
        x0 / scale_factor,
        y0 / scale_factor,
        (x1 - x0) / scale_factor,
        (y1 - y0) / scale_factor,
    ))
}

fn apply_cursor_to_window(
    platform_window: &teksilo_platform::PlatformWindow,
    cursor: teksilo_core::CursorIcon,
) {
    let winit_cursor = match cursor {
        teksilo_core::CursorIcon::Default => winit::window::CursorIcon::Default,
        teksilo_core::CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
        teksilo_core::CursorIcon::Text => winit::window::CursorIcon::Text,
        teksilo_core::CursorIcon::Crosshair => winit::window::CursorIcon::Crosshair,
        teksilo_core::CursorIcon::Move => winit::window::CursorIcon::Move,
        teksilo_core::CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
        teksilo_core::CursorIcon::Grab => winit::window::CursorIcon::Grab,
        teksilo_core::CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
        teksilo_core::CursorIcon::ColResize => winit::window::CursorIcon::ColResize,
        teksilo_core::CursorIcon::RowResize => winit::window::CursorIcon::RowResize,
        teksilo_core::CursorIcon::NeswResize => winit::window::CursorIcon::NeswResize,
        teksilo_core::CursorIcon::NwseResize => winit::window::CursorIcon::NwseResize,
    };
    platform_window.window().set_cursor(winit_cursor);
}

/// The real window behind [`crate::input_loop::InputChrome`].
///
/// Holds the platform window and, when one is running, the idle trace — the
/// two things the input half of a turn reaches for that a headless test has no
/// equivalent of. Everything with a decision in it lives in `input_loop`; this
/// is the adapter, and it is deliberately without one.
pub(crate) struct WindowChrome<'a> {
    window: &'a teksilo_platform::PlatformWindow,
    trace: Option<&'a mut IdleTrace>,
}

impl<'a> WindowChrome<'a> {
    /// Wrap `window`, optionally noting redraw reasons into `trace`.
    pub(crate) fn new(
        window: &'a teksilo_platform::PlatformWindow,
        trace: Option<&'a mut IdleTrace>,
    ) -> Self {
        Self { window, trace }
    }
}

impl crate::input_loop::InputChrome for WindowChrome<'_> {
    fn set_cursor(&mut self, icon: teksilo_core::CursorIcon) {
        apply_cursor_to_window(self.window, icon);
    }

    fn request_redraw(&mut self, reason: &'static str) {
        if let Some(trace) = &mut self.trace {
            trace.note_redraw_request(reason);
        }
        self.window.request_redraw();
    }

    fn safe_area(&self) -> teksilo_platform::safe_area::SafeAreaSides {
        teksilo_platform::window_safe_area(self.window.window())
    }

    fn occluded_band(&self, screen: (i32, i32, i32, i32)) -> Option<teksilo_canvas::Rect> {
        occluded_band_in_window(self.window.window(), screen)
    }

    fn set_soft_keyboard_visible(&mut self, visible: bool) {
        teksilo_platform::soft_keyboard::set_visible(self.window.window(), visible);
    }

    fn set_ime(&mut self, purpose: Option<teksilo_core::ImePurpose>, allowed: Option<bool>) {
        // The adapter, and nothing else: which of the two to push, and whether
        // to push at all, is `input_loop::settle_ime`'s decision.
        if let Some(purpose) = purpose {
            self.window
                .window()
                .set_ime_purpose(TeksiloAppHandler::map_ime_purpose(purpose));
        }
        if let Some(allowed) = allowed {
            self.window.window().set_ime_allowed(allowed);
        }
    }
}

#[derive(Debug)]
pub(crate) struct IdleTrace {
    last_report: Instant,
    resume_time_reached: u64,
    redraw_requested: u64,
    rendered_frames: u64,
    request_redraw_all: u64,
    cursor_redraw_requests: u64,
    mouse_input_redraw_requests: u64,
    mouse_wheel_redraw_requests: u64,
    keyboard_redraw_requests: u64,
    resize_redraw_requests: u64,
    /// Post-render redraw requests caused by `tree.frame_requested()`
    /// (a widget asked for another frame from a `frame_tick` effect or
    /// similar). Surfaces the only redraw source that was previously
    /// invisible to the trace.
    frame_request_redraws: u64,
    /// Windows poked by `WindowManager::request_redraw_needing_render`
    /// (a sibling window dirtied by another window's `Signal` mutation),
    /// distinct from `request_redraw_all` — surfaces how often the
    /// targeted cross-window path actually fires versus the blanket one.
    cross_window_redraws: u64,
    idle_callbacks_run: u64,
    control_flow_wait: u64,
    control_flow_wait_until: u64,
    timer_windows: usize,
    animation_timers: usize,
    tooltip_timers: usize,
}

impl IdleTrace {
    fn from_env() -> Option<Self> {
        match std::env::var("TEKSILO_IDLE_TRACE") {
            Ok(value) if value != "0" && !value.is_empty() => Some(Self {
                last_report: Instant::now(),
                resume_time_reached: 0,
                redraw_requested: 0,
                rendered_frames: 0,
                request_redraw_all: 0,
                cursor_redraw_requests: 0,
                mouse_input_redraw_requests: 0,
                mouse_wheel_redraw_requests: 0,
                keyboard_redraw_requests: 0,
                resize_redraw_requests: 0,
                frame_request_redraws: 0,
                cross_window_redraws: 0,
                idle_callbacks_run: 0,
                control_flow_wait: 0,
                control_flow_wait_until: 0,
                timer_windows: 0,
                animation_timers: 0,
                tooltip_timers: 0,
            }),
            _ => None,
        }
    }

    fn note_control_flow(
        &mut self,
        has_deadline: bool,
        timer_windows: usize,
        animation_timers: usize,
        tooltip_timers: usize,
    ) {
        if has_deadline {
            self.control_flow_wait_until += 1;
        } else {
            self.control_flow_wait += 1;
        }
        self.timer_windows = timer_windows;
        self.animation_timers = animation_timers;
        self.tooltip_timers = tooltip_timers;
        self.maybe_report();
    }

    fn note_request_redraw_all(&mut self) {
        self.request_redraw_all += 1;
        self.maybe_report();
    }

    pub(crate) fn note_redraw_request(&mut self, reason: &'static str) {
        match reason {
            "cursor" => self.cursor_redraw_requests += 1,
            "mouse_input" => self.mouse_input_redraw_requests += 1,
            "mouse_wheel" => self.mouse_wheel_redraw_requests += 1,
            "keyboard" => self.keyboard_redraw_requests += 1,
            "resize" => self.resize_redraw_requests += 1,
            _ => {}
        }
        self.maybe_report();
    }

    fn note_cross_window_redraw(&mut self, windows: usize) {
        self.cross_window_redraws += windows as u64;
        self.maybe_report();
    }

    fn note_resume_time_reached(&mut self) {
        self.resume_time_reached += 1;
        self.maybe_report();
    }

    fn note_redraw_requested(&mut self) {
        self.redraw_requested += 1;
        self.maybe_report();
    }

    fn note_rendered_frame(&mut self) {
        self.rendered_frames += 1;
        self.maybe_report();
    }

    fn note_idle_callbacks_run(&mut self) {
        self.idle_callbacks_run += 1;
        self.maybe_report();
    }

    fn maybe_report(&mut self) {
        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        eprintln!(
            "teksilo_idle_trace redraw_requested={} rendered_frames={} resume_time_reached={} request_redraw_all={} cross_window_redraws={} input_redraws={{cursor:{},mouse_input:{},mouse_wheel:{},keyboard:{},resize:{},frame_request:{}}} idle_callbacks={} control_flow={{wait:{},wait_until:{}}} timers={{windows:{},animations:{},tooltips:{}}}",
            self.redraw_requested,
            self.rendered_frames,
            self.resume_time_reached,
            self.request_redraw_all,
            self.cross_window_redraws,
            self.cursor_redraw_requests,
            self.mouse_input_redraw_requests,
            self.mouse_wheel_redraw_requests,
            self.keyboard_redraw_requests,
            self.resize_redraw_requests,
            self.frame_request_redraws,
            self.idle_callbacks_run,
            self.control_flow_wait,
            self.control_flow_wait_until,
            self.timer_windows,
            self.animation_timers,
            self.tooltip_timers,
        );

        self.last_report = Instant::now();
        self.resume_time_reached = 0;
        self.redraw_requested = 0;
        self.rendered_frames = 0;
        self.request_redraw_all = 0;
        self.cross_window_redraws = 0;
        self.cursor_redraw_requests = 0;
        self.mouse_input_redraw_requests = 0;
        self.mouse_wheel_redraw_requests = 0;
        self.keyboard_redraw_requests = 0;
        self.resize_redraw_requests = 0;
        self.frame_request_redraws = 0;
        self.idle_callbacks_run = 0;
        self.control_flow_wait = 0;
        self.control_flow_wait_until = 0;
    }
}

/// An app-supplied router for [`AppEvent::External`] payloads that need to
/// perform **window operations** — open a window, focus one, look one up by its
/// string id.
///
/// Registered with [`TeksiloAppBuilder::on_external_with_ctx`]. Returns `true`
/// to say "this payload was mine"; `false` leaves it unclaimed.
///
/// The plain [`on_app_event`](TeksiloAppBuilder::on_app_event) hook receives only
/// `&AppEvent` — no tree, no [`WindowOps`](teksilo_core::WindowOps) — so a handler
/// there cannot call `open_window` at all: `EventContext::open_window` panics on a
/// standalone context. This one runs against a real window's tree with a real ops
/// sink, which is what makes the multi-window recipes in `docs/multi-window.md`
/// reachable from a background thread (a single-instance app's IPC listener being
/// the motivating case: a second launch forwards its command line and the running
/// process opens the document window).
pub type ExternalCtxHandler =
    Box<dyn FnMut(&(dyn std::any::Any + Send), &mut teksilo_core::widget::EventContext) -> bool>;

struct TeksiloAppHandler {
    wm: WindowManager,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    /// App-supplied `AppEvent::External` router with window ops — see
    /// [`ExternalCtxHandler`]. Consulted only for payloads no framework router
    /// and no built-in downcast arm claimed.
    external_ctx_handler: Option<ExternalCtxHandler>,
    initial_window: Option<WindowConfig>,
    initial_created: bool,
    idle_budget: Duration,
    idle_trace: Option<IdleTrace>,
    #[cfg(feature = "text")]
    typesetter: SharedTypesetter,
    /// Kept alive for the lifetime of the event loop so that the
    /// `notify::RecommendedWatcher` background thread keeps running.
    /// Created in `TeksiloAppBuilder::run` when the `I18nConfig` registers
    /// any `runtime_override`s; otherwise `None`.
    _i18n_watcher: Option<teksilo_i18n::FtlFileWatcher>,
    /// Kept alive for the lifetime of the event loop so that the
    /// settings directory watcher's background thread keeps running.
    /// Created in `TeksiloAppBuilder::run` when a settings bundle was
    /// opened and live-reload was not disabled; otherwise `None`.
    _settings_watcher: Option<teksilo_settings::SettingsWatcher>,
    /// Optional per-loop-turn closure (e.g. an async executor poll) installed
    /// via [`TeksiloAppBuilder::on_loop_tick`]. Runs at the top of
    /// `about_to_wait`; returning `true` means tasks advanced and a repaint is
    /// needed. Async-agnostic — the loop only ever sees `FnMut`.
    loop_tick: Option<Box<dyn FnMut() -> bool>>,
    /// Shared flag a `loop_tick` owner sets while it wants continuous polling.
    /// Read in `update_control_flow` to force `ControlFlow::Poll`; when clear,
    /// the loop sleeps until the next event (off-thread wakes via the proxy).
    loop_tick_poll: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    /// Whether the last loop turn was started by the OS rather than by our own
    /// `WaitUntil`. Read by the pen pump: an external wake with a digitizer
    /// attached is the moment a packet may be one shim-poll away.
    woken_externally: bool,
    /// One-shot catch-up deadline for the pen shim. See
    /// [`TeksiloAppHandler::pen_deadline`].
    pen_recheck_at: Option<Instant>,
    /// When the on-screen keyboard's rectangle was last read. See
    /// [`TeksiloAppHandler::refresh_occluded_band`].
    osk_polled_at: Option<Instant>,
    /// Tells an attached screen reader of each key before the application
    /// acts on it, where the reader cannot see the keyboard itself (Orca
    /// under Wayland), and holds back the keys the reader took. One per
    /// application: a key pressed in one window can come up in another.
    key_report: teksilo_platform::key_report::KeyReportGate,
}

impl TeksiloAppHandler {
    fn new(
        theme: Theme,
        theme_mode: ThemeMode,
        app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
        initial_window: WindowConfig,
        app_context_template: Option<std::rc::Rc<TreeAppContext>>,
        #[cfg(feature = "text")] typesetter: SharedTypesetter,
        i18n_watcher: Option<teksilo_i18n::FtlFileWatcher>,
        settings_watcher: Option<teksilo_settings::SettingsWatcher>,
        event_proxy: AppEventProxy,
    ) -> Self {
        let mut wm = WindowManager::new(theme);
        wm.set_theme_mode(theme_mode);
        wm.set_event_proxy(event_proxy);
        if let Some(template) = app_context_template {
            // Seed the persisted user text-scale factor (if settings are
            // installed) so every initially-created window opens at the saved
            // scale. No per-app boilerplate: apps without settings stay at 1.0.
            if let Some(store) = template.app_state::<teksilo_settings::SettingsStore>() {
                let scale = store.signal_for(&teksilo_settings::TEXT_SCALE_KEY).get();
                wm.set_initial_text_scale(scale);
            }
            wm.set_app_context_template(template);
        }

        #[cfg(feature = "text")]
        {
            wm.set_typesetter(typesetter.clone());
        }

        Self {
            wm,
            app_event_handler,
            external_ctx_handler: None,
            initial_window: Some(initial_window),
            initial_created: false,
            idle_budget: Duration::from_millis(4),
            idle_trace: IdleTrace::from_env(),
            #[cfg(feature = "text")]
            typesetter,
            _i18n_watcher: i18n_watcher,
            _settings_watcher: settings_watcher,
            loop_tick: None,
            loop_tick_poll: None,
            woken_externally: true,
            pen_recheck_at: None,
            osk_polled_at: None,
            key_report: teksilo_platform::key_report::KeyReportGate::for_platform(),
        }
    }

    fn process_pending(&mut self, event_loop: &ActiveEventLoop) {
        self.wm.process_pending(event_loop);
    }

    fn process_modal_requests(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let native_supported = teksilo_platform::supports_native_modal_windows();
        let requests = self.wm.drain_pending_modal_requests();
        let had_requests = !requests.is_empty();

        for (source_window, requests) in requests {
            for queued in requests {
                let resolved = resolve_modal_presentation(
                    queued.request.presentation,
                    &queued.request.content,
                    native_supported,
                );

                match resolved {
                    ResolvedModalPresentation::InTree => {
                        if let Some(managed) = self.wm.get_by_teksilo_mut(source_window) {
                            present_in_tree_modal_request(
                                &mut managed.tree,
                                queued.source_widget,
                                queued.request,
                            );
                        }
                    }
                    ResolvedModalPresentation::NativeWindow => {
                        let ModalRequest {
                            content,
                            title,
                            size,
                            focus_target,
                            close_behavior,
                            ..
                        } = queued.request;

                        let ModalContent::Deferred(builder) = content else {
                            continue;
                        };

                        let mut config =
                            WindowConfig::new().modal(crate::window_config::ModalConfig {
                                parent: source_window,
                                focus_target,
                            });
                        if let Some(title) = title {
                            config = config.title(title);
                        }
                        if let Some((width, height)) = size {
                            // Native modals size their height to content: the
                            // requested (width, height) is the floor and the OS
                            // window grows to fit taller content (e.g. a
                            // MessageBox "Show details" expander). Without this
                            // the fixed height clips content that exceeds it —
                            // the footer buttons fall below the client edge and
                            // stop receiving clicks. NOTE: deliberately NOT
                            // `resizable(false)` — winit encodes that as
                            // min==max size hints on X11, which would clamp away
                            // the programmatic growth this relies on.
                            config = config
                                .size(width, height)
                                .min_size(width, height)
                                .size_to_content(SizeToContent::Height);
                        }
                        // Honour the request's `ModalCloseBehavior`. Only the
                        // Escape half of it is expressible for a native
                        // window: there is no "outside" to click, because the
                        // parent window is input-blocked for as long as the
                        // modal is up — which is what the OS does with a click
                        // there, and is why `ClickOutside` resolves to
                        // "nothing dismisses this but the app" here rather
                        // than to a second route. Before this the whole field
                        // fell into the struct's `..` and was never read, so a
                        // `Dialog` taking BOTH defaults (`Auto` presentation,
                        // `EscapeOrClickOutside`) got neither behaviour on
                        // every platform that hosts a native modal.
                        let escape_dismisses = native_modal_escape_dismisses(close_behavior);
                        self.wm.create_window(
                            config.root(move |tree, _state| {
                                let content_id = builder(tree);
                                if escape_dismisses {
                                    wrap_native_modal_in_escape_route(tree, content_id)
                                } else {
                                    content_id
                                }
                            }),
                            event_loop,
                        );
                    }
                }
            }
        }

        had_requests
    }

    fn process_modal_dismissals(&mut self) -> bool {
        let windows_to_close = self.wm.drain_pending_modal_dismissals();
        let had_dismissals = !windows_to_close.is_empty();

        for window_id in windows_to_close {
            self.wm.queue_close(window_id);
        }

        had_dismissals
    }

    fn maybe_exit(&self, event_loop: &ActiveEventLoop) {
        if self.wm.is_empty() {
            event_loop.exit();
        }
    }

    fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        // Tick time-driven gesture recognizers (long-press) on every tree
        // before computing the next deadline. Without this, a long-press
        // that expired between frames would never fire until the next
        // unrelated pointer event. Handlers that run may emit commands
        // and mark nodes dirty — request a redraw on those windows.
        let now = Instant::now();
        // Collect winit ids up front so we can safely iterate without
        // holding a borrow on `self.wm.windows` across the
        // `tick_gestures_in_window` calls (each of which briefly
        // takes a window out of the map).
        let winit_ids: Vec<_> = self.wm.windows_map().keys().copied().collect();
        for winit_id in winit_ids {
            let before = self
                .wm
                .get_by_winit_mut(winit_id)
                .map(|m| m.tree.has_idle_work())
                .unwrap_or(false);
            self.tick_gestures_in_window(winit_id, now, event_loop);
            if let Some(managed) = self.wm.get_by_winit_mut(winit_id)
                && managed.tree.has_idle_work() != before
            {
                managed.platform_window.request_redraw();
            }
        }

        let mut earliest_deadline: Option<Instant> = None;
        let mut timer_windows = 0_usize;
        let mut animation_timers = 0_usize;
        let mut tooltip_timers = 0_usize;
        for managed in self.wm.iter() {
            let animation_count = managed.tree.active_animation_count();
            let tooltip_count = managed.tree.pending_tooltip_count();
            if animation_count > 0 || tooltip_count > 0 {
                timer_windows += 1;
            }
            animation_timers += animation_count;
            tooltip_timers += tooltip_count;
            // `next_timer_deadline` now folds in the per-frame-effect
            // path's fixed 60 Hz deadline (Pulse / Cycle / caret blink /
            // drag auto-scroll) alongside the tween + shader schedulers,
            // so continuous animations pace through `WaitUntil` below
            // instead of forcing `ControlFlow::Poll` (which free-ran at
            // the display's refresh rate — 300 fps on a 300 Hz panel).
            if let Some(deadline) = managed.tree.next_timer_deadline() {
                earliest_deadline = Some(match earliest_deadline {
                    Some(current) => current.min(deadline),
                    None => deadline,
                });
            }
        }

        // The digitizer, whose packets arrive on a thread of its own and so
        // cannot wake the loop by themselves. A bounded `WaitUntil` term, not
        // a `Poll`: see `input_loop::pen_deadline` for when each of its two
        // terms applies and when neither does.
        if let Some(pen) = self.pen_deadline(now) {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => current.min(pen),
                None => pen,
            });
        }

        // The ONLY consumer that forces true `ControlFlow::Poll`: an installed
        // loop-tick owner (e.g. the `teksilo-async` executor) with runnable
        // work. Async task processing wants to run as fast as possible and is
        // not an animation, so it is deliberately *not* 60 Hz-capped. Every
        // per-frame *animation* effect paces through the `WaitUntil` deadline
        // instead.
        let force_poll = self.loop_tick_poll.as_ref().is_some_and(|poll| poll.get());

        if force_poll {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(deadline) = earliest_deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if let Some(trace) = &mut self.idle_trace {
            trace.note_control_flow(
                earliest_deadline.is_some(),
                timer_windows,
                animation_timers,
                tooltip_timers,
            );
        }
    }

    /// Read the window's platform safe area and hand it to the tree.
    ///
    /// The take-aside; the reading and the mapping are
    /// [`crate::input_loop::refresh_safe_area`].
    fn refresh_safe_area(managed: &mut crate::window_manager::ManagedWindow) {
        let chrome = WindowChrome::new(&managed.platform_window, None);
        crate::input_loop::refresh_safe_area(&chrome, &mut managed.tree);
    }

    /// Re-read the on-screen keyboard's rectangle and report it to every
    /// window it covers.
    ///
    /// Whether this turn is the one that re-reads is
    /// [`crate::input_loop::osk_poll_due`]; what each window makes of the
    /// rectangle is [`crate::input_loop::refresh_occluded_band`].
    fn refresh_occluded_band(&mut self) {
        let now = Instant::now();
        if !crate::input_loop::osk_poll_due(
            teksilo_platform::soft_keyboard::support(),
            self.osk_polled_at,
            now,
            OSK_POLL_INTERVAL,
        ) {
            return;
        }
        self.osk_polled_at = Some(now);
        let screen = teksilo_platform::soft_keyboard::keyboard_screen_rect();
        for managed in self.wm.iter_mut() {
            let chrome = WindowChrome::new(&managed.platform_window, None);
            crate::input_loop::refresh_occluded_band(&chrome, &mut managed.tree, screen);
        }
    }

    /// Whether any window's pen shim fills its buffer from a thread of its own.
    ///
    /// The Wayland tablet listener does; the Windows `WM_POINTER` subclass does
    /// not — its packets are already in the buffer by the time the turn that
    /// carried the message reaches the pump. Only the first kind needs the
    /// loop to look a second time.
    fn any_pen_source_polls_off_thread(&self) -> bool {
        self.wm
            .iter()
            .any(|managed| managed.translation_state.pen_polls_off_thread())
    }

    /// When the pen shim next has to be looked at, if ever.
    ///
    /// The take-aside; the folding is [`crate::input_loop::pen_deadline`],
    /// which is where the two terms and their bounds are described.
    fn pen_deadline(&mut self, now: Instant) -> Option<Instant> {
        let windows: Vec<_> = self
            .wm
            .iter()
            .map(|managed| crate::input_loop::PenWindow {
                off_thread: managed.translation_state.pen_polls_off_thread(),
                in_proximity: managed.translation_state.pen_in_proximity(),
            })
            .collect();
        crate::input_loop::pen_deadline(now, windows.into_iter(), &mut self.pen_recheck_at)
    }

    fn post_event(&mut self, event_loop: &ActiveEventLoop) {
        // App-wide environment changes (theme / locale) raised by a handler
        // in one window fan out to every window's tree, marking the
        // non-originating windows dirty. Those windows never received the
        // triggering event, so they would otherwise stay un-repainted —
        // `request_redraw_all()` below (gated on these flags) fixes that.
        let had_locale = self.wm.drain_pending_locale_requests();
        let had_theme = self.wm.drain_pending_theme_requests();
        let had_follow_system = self.wm.drain_pending_follow_system_requests();
        let had_text_scale = self.wm.drain_pending_text_scale_requests();
        let had_commands = self.wm.drain_close_window_requests();
        let had_modal_requests = self.process_modal_requests(event_loop);
        let had_modal_dismissals = self.process_modal_dismissals();
        self.process_pending(event_loop);
        // Drain post-mount actions (e.g. a WebView opening its native engine
        // subview, which needs the OS parent handle only reachable here).
        self.process_pending_mount_actions(event_loop);
        // Drain per-window command queues: app-side writes to
        // WindowState signals emitted WindowCommand values that the
        // registry routes through the per-window queue. Translate each
        // into the appropriate winit call.
        self.wm.drain_window_commands();
        if had_locale
            || had_theme
            || had_follow_system
            || had_text_scale
            || had_commands
            || had_modal_requests
            || had_modal_dismissals
        {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_request_redraw_all();
            }
            self.wm.request_redraw_all();
        }
        // Targeted counterpart to the blanket call above: a handler may have
        // mutated an app-level `Signal` that sibling windows also read,
        // dirtying their trees without those windows ever seeing the
        // triggering event. See `WindowManager::request_redraw_needing_render`
        // for why this is filtered rather than another `request_redraw_all()`.
        let cross_window_redraws = self.wm.request_redraw_needing_render();
        if cross_window_redraws > 0
            && let Some(trace) = &mut self.idle_trace
        {
            trace.note_cross_window_redraw(cross_window_redraws);
        }
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }

    /// Dispatch a widget event into the named window's `WidgetTree`
    /// with a real [`teksilo_core::WindowOps`] sink so handlers can
    /// synchronously `open_window`, `focus_window`, etc.
    ///
    /// Re-entry pattern: the current `ManagedWindow` is temporarily
    /// removed from `WindowManager::windows` before dispatch and put
    /// back afterwards. The removed tree is borrowed mutably for the
    /// handler run; the `WindowOpsImpl` holds `&mut WindowManager`
    /// (with the tree out of the way) plus `&ActiveEventLoop`. Opening
    /// a new window from a handler therefore goes straight into
    /// `wm.create_window` without borrow-checker conflicts.
    fn dispatch_in_window(
        &mut self,
        window_id: WindowId,
        event: WidgetEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            current.tree.dispatch_event_with_ops(event, &mut ops);
        }

        Self::settle_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
    }

    /// Take the window aside and run `f` with its translator, its tree and a
    /// real [`WindowOps`](teksilo_core::WindowOps) sink all borrowed at once.
    ///
    /// The pointer path needs all three — the backend to translate, the tree
    /// to receive, the ops sink so a handler can still open a window — and the
    /// only way to hold them together is the same take-aside
    /// [`Self::dispatch_in_window`] uses. Returns `f`'s value, or `None` when
    /// the window is gone.
    fn with_input_in_window<R>(
        &mut self,
        window_id: WindowId,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(
            &mut WindowChrome<'_>,
            &mut teksilo_platform::TranslationState,
            &mut teksilo_core::WidgetTree,
            &mut crate::window_manager::WindowOpsImpl<'_>,
        ) -> R,
    ) -> Option<R> {
        let mut current = self.wm.take_managed(window_id)?;
        let current_id = current.teksilo_id;
        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        let result = {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            let mut chrome = WindowChrome::new(&current.platform_window, self.idle_trace.as_mut());
            f(
                &mut chrome,
                &mut current.translation_state,
                &mut current.tree,
                &mut ops,
            )
        };

        Self::settle_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
        Some(result)
    }

    /// Route one pointer / gesture winit event into the named window.
    ///
    /// Nothing but the take-aside: the routing and the chores it owes are
    /// [`crate::input_loop::dispatch_input`], which a test can drive with a
    /// hand-written winit event, a bare tree and a recording chrome.
    fn dispatch_input_in_window(
        &mut self,
        window_id: WindowId,
        event: &WindowEvent,
        event_loop: &ActiveEventLoop,
    ) {
        self.with_input_in_window(window_id, event_loop, |chrome, backend, tree, ops| {
            crate::input_loop::dispatch_input(chrome, backend, tree, ops, event);
        });
    }

    /// Drain every window's pen shim.
    ///
    /// Once per event-loop turn, which is what
    /// [`TranslationState::poll_pen`](teksilo_platform::TranslationState::poll_pen)
    /// asks for. What it costs is decided by what
    /// [`create_pen_source`](teksilo_platform::create_pen_source) answered when
    /// the window was made, and **no platform's answer depends on a digitizer
    /// being attached**: the shim is installed whenever the platform has a pen
    /// *path* — a compositor advertising `zwp_tablet_manager_v2`, or a Win32
    /// window whose subclass installs — and on X11 and macOS there is no path,
    /// so the null source is answered and `has_pen_source()` is false for every
    /// window. Where no shim was installed this returns before it allocates;
    /// where one was, a walk and one empty poll per window per turn are paid
    /// even on a machine that has never seen a stylus. What that shim costs
    /// *between* those polls is its own business and is bounded there —
    /// `WaylandPenSource::poll_interval` stands its dispatch thread down to a
    /// quarter-second tick until the seat announces a tool.
    fn pump_pen_sources(&mut self, event_loop: &ActiveEventLoop) {
        let winit_ids: Vec<_> = self
            .wm
            .windows_map()
            .iter()
            .filter(|(_, managed)| managed.translation_state.has_pen_source())
            .map(|(id, _)| *id)
            .collect();
        if winit_ids.is_empty() {
            self.pen_recheck_at = None;
            return;
        }

        // An external wake with a shim installed means a packet may be one
        // shim-poll away. Arm a single catch-up look; a session that produces
        // more will keep the loop ticking through `pen_deadline` on its own.
        if self.woken_externally && self.any_pen_source_polls_off_thread() {
            self.pen_recheck_at = Some(Instant::now() + teksilo_platform::pen::PEN_POLL_INTERVAL);
        }
        for winit_id in winit_ids {
            self.with_input_in_window(winit_id, event_loop, |chrome, backend, tree, ops| {
                crate::input_loop::pump_pen(chrome, backend, tree, ops)
            });
        }
    }

    /// Flip a window's active state, revoking every live pointer at both the
    /// backend and the tree when it goes inactive.
    fn set_window_active_in_window(
        &mut self,
        window_id: WindowId,
        active: bool,
        reason: teksilo_core::pointer::CancelReason,
        event_loop: &ActiveEventLoop,
    ) {
        self.with_input_in_window(window_id, event_loop, |_chrome, backend, tree, ops| {
            crate::input_routing::set_window_active(backend, tree, ops, active, reason);
        });
    }

    /// Apply a [`MenubarAction`](teksilo_core::window::MenubarAction)
    /// decision from a window-level menubar dispatcher. Takes the
    /// managed window aside the same way
    /// [`Self::dispatch_in_window`] does so the action runs with
    /// `WindowOps` wired up (focus changes need to repaint, etc.).
    ///
    /// - `OpenMenu`: focus the trigger and synthesise a primary click
    ///   on it. The MenuBarTrigger's `on_tap` handler then runs the
    ///   normal `MenuContext::open_at` path.
    /// - `FocusTrigger`: focus the trigger and stop. Matches Win32
    ///   F10 behaviour (menubar mode, no menu).
    /// - `Intercept`: do nothing — the key was swallowed.
    fn apply_menubar_action(
        &mut self,
        window_id: WindowId,
        action: teksilo_core::window::MenubarAction,
        event_loop: &ActiveEventLoop,
    ) {
        use teksilo_core::window::MenubarAction;
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        // For a collapsed (hamburger) MenuBar, the action carries a
        // `reveal` closure. We must run it (it shows the bar as a
        // floating overlay) and then re-layout synchronously, so the
        // trigger has valid bounds before we focus / synthesise the
        // click on it. Compute the same layout proposal the redraw
        // path uses.
        let proposal = {
            let size = current.platform_window.surface_size();
            let sf = current.platform_window.scale_factor() as f32;
            SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf)
        };

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            match action {
                MenubarAction::Intercept => {}
                MenubarAction::FocusTrigger { trigger_id, reveal } => {
                    if let Some(reveal) = reveal {
                        current
                            .tree
                            .run_with_event_context(&mut ops, |ctx| reveal(ctx));
                        current.tree.layout_with_ops(proposal, &mut ops);
                    }
                    current.tree.focus_ops(trigger_id, &mut ops);
                }
                MenubarAction::OpenMenu { trigger_id, reveal } => {
                    if let Some(reveal) = reveal {
                        current
                            .tree
                            .run_with_event_context(&mut ops, |ctx| reveal(ctx));
                        current.tree.layout_with_ops(proposal, &mut ops);
                    }
                    current.tree.focus_ops(trigger_id, &mut ops);
                    // A keyboard chord (F10, Alt+letter) opened this menu, so
                    // there is no pointer to describe: the constructors' mouse
                    // default is the honest answer, and it is what the trigger
                    // saw before the press carried a pointer at all.
                    let at = current.tree.bounds(trigger_id).center();
                    current.tree.dispatch_event_with_ops(
                        WidgetEvent::pointer_down(
                            at,
                            teksilo_core::event::PointerButton::Primary,
                            teksilo_core::event::Modifiers::NONE,
                        ),
                        &mut ops,
                    );
                    current.tree.dispatch_event_with_ops(
                        WidgetEvent::pointer_up(
                            at,
                            teksilo_core::event::PointerButton::Primary,
                            teksilo_core::event::Modifiers::NONE,
                        ),
                        &mut ops,
                    );
                }
            }
        }

        Self::settle_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
    }

    /// Bring the winit window's OS-IME state in line with the focused
    /// widget's descriptor, then apply any pending
    /// `EventContext::request_soft_keyboard`.
    ///
    /// The take-aside; the decision — including why a turn at an unmoved focus
    /// must push nothing at all, and why the request half can never reach
    /// `set_ime_allowed` — is [`crate::input_loop::settle_ime`]. The caret
    /// area is reported separately (and idempotently) by the focused widget
    /// via `WindowOps::set_ime_cursor_area`.
    fn settle_ime(managed: &mut crate::window_manager::ManagedWindow) {
        let mut chrome = WindowChrome::new(&managed.platform_window, None);
        crate::input_loop::settle_ime(
            &mut chrome,
            &mut managed.tree,
            crate::input_loop::host_soft_keyboard_support(),
            &mut managed.ime_purpose,
            &mut managed.ime_allowed,
        );
    }

    /// Map the core `ImePurpose` onto winit's enum at the platform boundary.
    fn map_ime_purpose(purpose: teksilo_core::ImePurpose) -> winit::window::ImePurpose {
        match purpose {
            teksilo_core::ImePurpose::Normal => winit::window::ImePurpose::Normal,
            teksilo_core::ImePurpose::Password => winit::window::ImePurpose::Password,
            teksilo_core::ImePurpose::Terminal => winit::window::ImePurpose::Terminal,
        }
    }

    /// Route a debug-bridge [`AutomationPayload`](crate::automation_bridge::AutomationPayload):
    /// resolve the target window, then run the op against the live tree
    /// (and, for screenshots, the live `PlatformWindow`). `list_windows` and
    /// `screenshot` are served here (they need the window manager / platform
    /// window); everything else goes through [`teksilo_automation::execute`]
    /// with a real `WindowOps`. The settle runs synchronously on this (the
    /// main) thread, never across a frame boundary.
    /// Run `f` against window `winit_id`'s tree with a real
    /// [`WindowOps`](teksilo_core::WindowOps) sink (so `open_window`,
    /// `parent_window_handle`, etc. work). Encapsulates the take-out /
    /// build-`WindowOpsImpl` / reinsert dance that the `AppEvent::External`
    /// routers and the mount-action drain all share — keeping the reinsert
    /// (whose omission silently freezes a window) in exactly one place.
    /// No-op if `winit_id` is not a managed window.
    /// Route a debug-bridge [`AutomationPayload`](crate::automation_bridge::AutomationPayload):
    /// resolve the target window, then run the op against the live tree
    /// (and, for screenshots, the live `PlatformWindow`). `list_windows` and
    /// `screenshot` are served here (they need the window manager / platform
    /// window); everything else goes through [`teksilo_automation::execute`]
    /// with a real `WindowOps`. The settle runs synchronously on this (the
    /// main) thread, never across a frame boundary.
    #[cfg(all(feature = "automation", debug_assertions))]
    fn try_route_automation_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_automation::dto::{AutomationOp, AutomationReply, WindowInfo, codes};

        let payload = *payload.downcast::<crate::automation_bridge::AutomationPayload>()?;

        // Resolve target window: explicit id, else focused, else primary.
        let bid = match payload.window_id {
            Some(raw) => crate::window_config::TeksiloWindowId::new(raw),
            None => self
                .wm
                .iter()
                .find(|m| m.focused)
                .map(|m| m.teksilo_id)
                .unwrap_or_else(|| self.wm.primary_window_id()),
        };
        let Some(winit_id) = self.wm.winit_id_for_teksilo(bid) else {
            let _ = payload
                .reply_tx
                .send(AutomationReply::err(codes::NOT_FOUND, "no such window"));
            return Ok(());
        };

        // `list_windows` is served straight from the window manager.
        if matches!(payload.op, AutomationOp::ListWindows) {
            let windows: Vec<WindowInfo> = self
                .wm
                .iter()
                .map(|m| WindowInfo {
                    id: m.teksilo_id.raw(),
                    label: m.string_id.clone(),
                    title: Some(m.state.title().get()),
                    focused: m.focused,
                })
                .collect();
            let _ = payload.reply_tx.send(AutomationReply::ok_json(&windows));
            return Ok(());
        }

        // Screenshots reach the `ManagedWindow` (tree + platform window).
        if matches!(payload.op, AutomationOp::Screenshot { .. }) {
            self.automation_screenshot(winit_id, event_loop, &payload);
            return Ok(());
        }

        // Everything else: a per-tree op with a real `WindowOps`.
        let crate::automation_bridge::AutomationPayload {
            op,
            settle,
            reply_tx,
            ..
        } = payload;
        // Clamp the settle: this runs on the winit main thread, so an
        // unbounded wait/settle would freeze the live UI (see Risk 1).
        let settle = crate::automation_bridge::clamp_live_settle(&settle);
        self.run_in_window(winit_id, event_loop, move |tree, ops| {
            let reply = teksilo_automation::execute(tree, ops, &op, &settle);
            let _ = reply_tx.send(reply);
        });
        if let Some(m) = self.wm.windows_map().get(&winit_id) {
            m.platform_window.request_redraw();
        }
        Ok(())
    }

    /// The screenshot arm of the automation bridge: take the window out of
    /// the manager (so we can borrow both its tree and its platform window),
    /// settle, render, capture offscreen, reinsert, then reply with a
    /// base64-PNG.
    #[cfg(all(feature = "automation", debug_assertions))]
    fn automation_screenshot(
        &mut self,
        winit_id: winit::window::WindowId,
        event_loop: &ActiveEventLoop,
        payload: &crate::automation_bridge::AutomationPayload,
    ) {
        use teksilo_automation::dto::{AutomationOp, AutomationReply, codes};

        let node = match &payload.op {
            AutomationOp::Screenshot { node } => *node,
            _ => None,
        };

        let Some(mut current) = self.wm.take_managed(winit_id) else {
            let _ = payload
                .reply_tx
                .send(AutomationReply::err(codes::NOT_FOUND, "window vanished"));
            return;
        };
        let current_id = current.teksilo_id;
        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        // Settle synchronously on the main thread with a real `WindowOps`.
        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            let settle = crate::automation_bridge::clamp_live_settle(&payload.settle);
            let _ = teksilo_automation::run_settle(&mut current.tree, &mut ops, &settle);
        }

        // Optional crop rect in physical pixels (logical bounds × scale).
        let scale = current.tree.device_scale_factor();
        let crop = node.and_then(|n| {
            let nid = teksilo_core::accesskit::NodeId(n);
            let wid = teksilo_core::accessibility::node_id_to_widget_id_maybe(nid)
                .or_else(|| current.tree.widget_for_synthetic(nid))?;
            let b = current.tree.bounds(wid);
            Some(teksilo_canvas::Rect {
                x: b.x * scale,
                y: b.y * scale,
                width: b.width * scale,
                height: b.height * scale,
            })
        });

        // WebView blind-spot warning.
        let warnings = {
            // `accessibility_tree_snapshot`, not `sync_accessibility`: this
            // update is inspected and dropped. Delivering it would consume one
            // step of the framework's live regions, so taking a screenshot
            // while something was being announced would eat the announcement.
            let update = current.tree.accessibility_tree_snapshot();
            if update
                .nodes
                .iter()
                .any(|(_, nd)| nd.role() == teksilo_core::accesskit::Role::WebView)
            {
                vec!["webview_hole_possible".to_string()]
            } else {
                Vec::new()
            }
        };

        let clear = teksilo_render::vertex::srgb_to_linear_rgba(
            current.tree.theme().colors.surface_main.to_array(),
        );
        let frame = current.tree.render();
        // The GPU readback inside `capture_offscreen` can `.expect()`-panic on
        // device loss (compositor restart, driver crash, memory pressure).
        // Catch it so the window is still reinserted (no zombie) and the app
        // survives — a screenshot failure must not abort a live session.
        let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            current
                .platform_window
                .capture_offscreen(&frame, clear, crop)
        }));

        current.platform_window.request_redraw();
        self.wm.reinsert_managed(winit_id, current);

        let reply = match captured {
            Ok((rgba, w, h)) if w != 0 && h != 0 => {
                crate::automation_bridge::screenshot_reply(&rgba, w, h, scale, warnings)
            }
            Ok(_) => {
                AutomationReply::err(codes::BAD_ARGUMENT, "crop region empty / outside window")
            }
            Err(_) => AutomationReply::err(
                codes::GPU_READBACK_FAILED,
                "offscreen capture failed (GPU device lost?)",
            ),
        };
        let _ = payload.reply_tx.send(reply);
    }

    fn run_in_window(
        &mut self,
        winit_id: winit::window::WindowId,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(&mut WidgetTree, &mut crate::window_manager::WindowOpsImpl),
    ) {
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            f(&mut current.tree, &mut ops);
        }

        self.wm.reinsert_managed(winit_id, current);
    }

    /// Last stop for an `AppEvent::External` payload: hand it to the app's own
    /// [`ExternalCtxHandler`] (if one was registered) with a live
    /// [`EventContext`](teksilo_core::widget::EventContext), so it can open,
    /// find and focus windows.
    ///
    /// **Target window** = the focused one, else the primary — the same
    /// resolution [`try_route_automation_payload`](Self::try_route_automation_payload)
    /// uses. The handler is about *application*-level intent ("open this
    /// document"), so which window hosts the context is an implementation
    /// detail; it just has to be a real one, because `open_window` on a
    /// standalone context panics.
    ///
    /// The handler is `take`n for the duration of the call and put back
    /// afterwards: [`run_in_window`](Self::run_in_window) needs `&mut self`, and
    /// the handler lives on `self`. Re-entrancy (a handler whose body somehow
    /// pumps another external event) therefore sees `None` and is a no-op rather
    /// than a double borrow.
    ///
    /// No window open (the instant between the last close and loop exit) is a
    /// silent no-op — there is nowhere to mint a context from.
    fn route_external_with_ctx(
        &mut self,
        payload: &(dyn std::any::Any + Send),
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut handler) = self.external_ctx_handler.take() else {
            return;
        };
        let target = self
            .wm
            .iter()
            .find(|m| m.focused)
            .map(|m| m.teksilo_id)
            .unwrap_or_else(|| self.wm.primary_window_id());
        if let Some(winit_id) = self.wm.winit_id_for_teksilo(target) {
            let handler = &mut handler;
            self.run_in_window(winit_id, event_loop, move |tree, ops| {
                tree.run_with_event_context(ops, |ctx| {
                    handler(payload, ctx);
                });
            });
        }
        self.external_ctx_handler = Some(handler);
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`FileDialogEventPayload`](teksilo_platform::file_dialog::FileDialogEventPayload).
    /// Returns `Ok(())` if the payload matched and was delivered to
    /// the originating window's tree, `Err(payload)` to hand the
    /// box back for fallthrough to other downcast attempts.
    ///
    /// Routing details:
    /// - Resolves `payload.window_id_owner` to the matching winit
    ///   `WindowId` via `WindowManager::teksilo_to_winit_map`.
    /// - Temporarily takes the window out of `WindowManager::windows`
    ///   (matches the `dispatch_in_window` re-entry pattern) so
    ///   `open_window` / other ops calls inside the result callback
    ///   can run.
    /// - Builds a `WidgetTree::run_with_event_context` closure that
    ///   pops the pending callback from `FileDialogHandle` and
    ///   invokes it.
    /// - On any miss (no matching window, no handle in app-state,
    ///   already-purged callback) the result is silently dropped —
    ///   no panic, no leaked callback.
    #[cfg_attr(not(feature = "file-dialog"), allow(unused_variables))]
    fn try_route_file_dialog_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        #[cfg(feature = "file-dialog")]
        {
            use teksilo_platform::file_dialog::{FileDialogEventPayload, FileDialogHandle};

            let payload = *payload.downcast::<FileDialogEventPayload>()?;

            // Find the originating window.
            let target_winit = self
                .wm
                .teksilo_to_winit_map()
                .get(&payload.window_id_owner)
                .copied();
            let Some(winit_id) = target_winit else {
                // Window already torn down — drop silently.
                return Ok(());
            };

            // Pull the FileDialogHandle out of the shared app context
            // template. Same Rc held by every window's tree, so this
            // does not fight take_managed below.
            let handle = self
                .wm
                .app_context_template()
                .and_then(|t| t.app_state::<FileDialogHandle>().cloned());
            let Some(handle) = handle else {
                // Application did not install a FileDialogHandle —
                // shouldn't happen if a payload was dispatched, but
                // drop silently rather than panic.
                return Ok(());
            };

            let Some(mut current) = self.wm.take_managed(winit_id) else {
                return Ok(());
            };
            let current_id = current.teksilo_id;

            #[cfg(not(target_os = "macos"))]
            let current_handle = current
                .platform_window
                .window()
                .window_handle()
                .ok()
                .map(|h| h.as_raw());
            let current_arc = Some(current.platform_window.window_arc());

            {
                let mut ops = crate::window_manager::WindowOpsImpl::new(
                    &mut self.wm,
                    event_loop,
                    current_id,
                    #[cfg(not(target_os = "macos"))]
                    current_handle,
                    current_arc,
                );
                current
                    .tree
                    .run_with_event_context(&mut ops, |ctx| handle.deliver(payload, ctx));
            }

            self.wm.reinsert_managed(winit_id, current);
            Ok(())
        }
        #[cfg(not(feature = "file-dialog"))]
        {
            Err(payload)
        }
    }

    /// Drain queued post-mount actions for every window that has any, each
    /// with a real [`EventContext`](teksilo_core::widget::EventContext) (so `ctx.parent_window_handle()` resolves).
    /// Modal-blocked windows are skipped — their actions (e.g. a WebView
    /// opening its native engine subview) stay queued until the modal closes,
    /// so a native surface can't appear over a modal. Cheap when nothing is
    /// queued (the common case): one map scan, the returned Vec is empty and
    /// unallocated.
    fn process_pending_mount_actions(&mut self, event_loop: &ActiveEventLoop) {
        let winit_ids = self.wm.winit_ids_with_pending_mount_actions();
        for winit_id in winit_ids {
            self.run_in_window(winit_id, event_loop, |tree, ops| {
                tree.run_mount_actions(ops)
            });
        }
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`WebViewEventPayload`](teksilo_webview::WebViewEventPayload) posted by a
    /// web-view engine backend, delivering it to the originating window's tree
    /// via [`WebViewRegistry::deliver`](teksilo_webview::WebViewRegistry::deliver).
    /// Returns `Ok(())` if matched and delivered, `Err(payload)` to hand the
    /// box back for fallthrough. Same take/run-with-context/reinsert dance as
    /// [`Self::try_route_file_dialog_payload`].
    #[cfg_attr(not(feature = "web-view"), allow(unused_variables))]
    fn try_route_web_view_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        #[cfg(feature = "web-view")]
        {
            use teksilo_webview::{WebViewEventPayload, WebViewRegistry};

            let payload = *payload.downcast::<WebViewEventPayload>()?;

            let target_winit = self
                .wm
                .teksilo_to_winit_map()
                .get(&payload.window_id_owner)
                .copied();
            let Some(winit_id) = target_winit else {
                return Ok(());
            };

            let registry = self
                .wm
                .app_context_template()
                .and_then(|t| t.app_state::<WebViewRegistry>().cloned());
            let Some(registry) = registry else {
                return Ok(());
            };

            self.run_in_window(winit_id, event_loop, move |tree, ops| {
                tree.run_with_event_context(ops, |ctx| registry.deliver(payload, ctx));
            });
            Ok(())
        }
        #[cfg(not(feature = "web-view"))]
        {
            Err(payload)
        }
    }

    /// Try to route an `AppEvent::External` payload as an
    /// [`AsyncCompletionPayload`](teksilo_core::AsyncCompletionPayload) posted
    /// by the `teksilo-async` executor when a `spawn_local_with` future
    /// resolves. Returns `Ok(())` if matched and delivered, `Err(payload)` to
    /// hand the box back for fallthrough.
    ///
    /// Uses only teksilo-core types ([`AsyncCompletionHandle`](teksilo_core::AsyncCompletionHandle)),
    /// so `teksilo-async` (which depends on `teksilo-app`) never has to be a
    /// dependency here — the same take/run-with-context/reinsert pattern as
    /// the file-dialog path. On any miss (window gone, runtime not installed,
    /// already-purged completion) the result is dropped silently.
    fn try_route_async_completion_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_core::{AsyncCompletionHandle, AsyncCompletionPayload};

        let payload = *payload.downcast::<AsyncCompletionPayload>()?;

        let target_winit = self
            .wm
            .teksilo_to_winit_map()
            .get(&payload.window_id)
            .copied();
        let Some(winit_id) = target_winit else {
            // Window already torn down — drop silently.
            return Ok(());
        };

        let handle = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<AsyncCompletionHandle>().cloned());
        let Some(handle) = handle else {
            // No async runtime installed — drop silently.
            return Ok(());
        };

        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return Ok(());
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            current.tree.run_with_event_context(&mut ops, |ctx| {
                handle.deliver(payload.id, payload.window_id, ctx)
            });
        }

        self.wm.reinsert_managed(winit_id, current);
        Ok(())
    }

    /// Deliver a backend `AppEvent::SubscriptionEvent` to a *context-bearing*
    /// subscription registered via
    /// [`BuildContext::subscribe_event_with_ctx`](teksilo_core::BuildContext::subscribe_event_with_ctx):
    /// mint a fresh [`EventContext`](teksilo_core::EventContext) from the
    /// subscriber's window tree and invoke the stored callback inside it.
    ///
    /// Returns `true` iff `sub_id` names a context-bearing subscription — the
    /// caller then skips the plain, context-free dispatch (a `sub_id` lives in
    /// exactly one callback map). A `true` return with the window torn down (or
    /// mid-teardown) drops the event, exactly like the async-completion path;
    /// it still returns `true` so the stale event never falls through to the
    /// plain map.
    ///
    /// Mirrors [`try_route_async_completion_payload`](Self::try_route_async_completion_payload)'s
    /// take / run-with-context / reinsert dance — the one supported way to run
    /// application code with a fresh `EventContext` from the event loop.
    fn try_dispatch_subscription_with_ctx(
        &mut self,
        sub_id: SubscriptionId,
        event: &dyn std::any::Any,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        let Some(template) = self.wm.app_context_template().cloned() else {
            return false;
        };
        let Some(window_id) = template.ctx_subscription_window(sub_id) else {
            return false;
        };
        // From here `sub_id` IS a context-bearing subscription: consume it
        // (return `true`) even if the window is gone, so a late event never
        // falls back to the plain, context-free map.
        let Some(winit_id) = self.wm.teksilo_to_winit_map().get(&window_id).copied() else {
            return true;
        };
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return true;
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            current.tree.run_with_event_context(&mut ops, |ctx| {
                template.dispatch_subscription_event_with_ctx(sub_id, event, ctx);
            });
        }

        self.wm.reinsert_managed(winit_id, current);
        true
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`NativeMenuEventPayload`](teksilo_platform::native_menu::NativeMenuEventPayload)
    /// posted when the user chose an item in the platform's native menu bar.
    /// Resolves the item's [`MenuItemId`](teksilo_core::MenuItemId) to its
    /// recorded intent / action via the [`NativeMenuHandle`](teksilo_platform::native_menu::NativeMenuHandle)
    /// and fires it inside the originating window's `EventContext` with
    /// `IntentSource::Menu` — the same pipeline an in-window `MenuItem` uses.
    /// Same take/run-with-context/reinsert shape as the file-dialog router; any
    /// miss is dropped silently.
    fn try_route_native_menu_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_core::Intent;
        use teksilo_core::telemetry::IntentSource;
        use teksilo_platform::native_menu::{NativeMenuEventPayload, NativeMenuHandle};

        let payload = *payload.downcast::<NativeMenuEventPayload>()?;

        let target_winit = self
            .wm
            .teksilo_to_winit_map()
            .get(&payload.window_id_owner)
            .copied();
        let Some(winit_id) = target_winit else {
            return Ok(());
        };

        let handle = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<NativeMenuHandle>().cloned());
        let Some(handle) = handle else {
            return Ok(());
        };
        let Some(activation) = handle.activation(payload.window_id_owner, payload.item_id) else {
            // Item not found (menu replaced / window torn down) — drop.
            return Ok(());
        };

        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return Ok(());
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            current.tree.run_with_event_context(&mut ops, |ctx| {
                ctx.with_intent_source(IntentSource::Menu, |ctx| {
                    if let Some(name) = activation.intent {
                        ctx.send_intent(Intent::new(name));
                    }
                    if let Some(action) = &activation.action {
                        action(ctx);
                    }
                });
            });
        }

        self.wm.reinsert_managed(winit_id, current);
        Ok(())
    }

    /// Try to interpret an `AppEvent::External` payload as an
    /// [`ExternalDndEventPayload`](teksilo_platform::external_dnd::ExternalDndEventPayload)
    /// posted by a platform drag backend and route it to the originating
    /// window's tree, driving the matching `*_external_drag` method.
    ///
    /// Returns `Ok(())` if the payload was an external-drag event (consumed),
    /// or `Err(payload)` to hand it back for other downcast attempts. Mirrors
    /// [`Self::try_route_file_dialog_payload`]'s take/dispatch/reinsert dance.
    fn try_route_external_dnd_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_platform::external_dnd::{
            ExternalDndEventPayload, ExternalDndHandle, ExternalDragEvent, OutboundOsDragRequest,
        };

        // Deferred blocking outbound (app → OS) drag: run OLE DoDragDrop here,
        // outside the in-app dispatch that started it (Windows). No window is
        // taken out of the manager at this point, so the drag's modal message
        // loop can't strand a borrowed window.
        let payload = match payload.downcast::<OutboundOsDragRequest>() {
            Ok(req) => {
                if let Some(handle) = self
                    .wm
                    .app_context_template()
                    .and_then(|t| t.app_state::<ExternalDndHandle>().cloned())
                {
                    handle.run_pending_outbound_drag(req.window_id);
                }
                return Ok(());
            }
            Err(other) => other,
        };

        let payload = *payload.downcast::<ExternalDndEventPayload>()?;

        let Some(winit_id) = self
            .wm
            .teksilo_to_winit_map()
            .get(&payload.window_id_owner)
            .copied()
        else {
            // Window already torn down — drop silently.
            return Ok(());
        };
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return Ok(());
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            match payload.event {
                ExternalDragEvent::Entered { data, position } => {
                    current.tree.begin_external_drag(position, data, &mut ops);
                }
                ExternalDragEvent::Moved { position } => {
                    current.tree.update_external_drag(position, &mut ops);
                }
                ExternalDragEvent::Left => {
                    current.tree.cancel_external_drag(&mut ops);
                }
                ExternalDragEvent::Cancelled => {
                    current.tree.abort_external_drag(&mut ops);
                }
                ExternalDragEvent::Dropped { data, position } => {
                    current.tree.end_external_drag(position, data, &mut ops);
                }
                ExternalDragEvent::DragEnded { outcome } => {
                    current.tree.handle_os_drag_ended(outcome, &mut ops);
                }
            }
        }

        // Repaint so hover feedback / drop results show promptly.
        current.platform_window.request_redraw();
        self.wm.reinsert_managed(winit_id, current);
        Ok(())
    }

    /// Tick gestures on every window with a real `WindowOps` sink so
    /// long-press / drag-tick handlers can open windows.
    fn tick_gestures_in_window(
        &mut self,
        window_id: WindowId,
        now: Instant,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            current.tree.tick_gestures_with_ops(now, &mut ops);
        }

        self.wm.reinsert_managed(window_id, current);
    }

    /// Report a key to an attached screen reader and say whether the
    /// application still gets it. See [`teksilo_platform::key_report`].
    fn report_key(
        &mut self,
        window_id: WindowId,
        key_event: &winit::event::KeyEvent,
        is_synthetic: bool,
    ) -> teksilo_platform::key_report::KeyDisposition {
        use teksilo_platform::key_report::{KeyDisposition, KeyInput, KeyTarget, KeyboardState};
        let Some(managed) = self.wm.get_by_winit_mut(window_id) else {
            return KeyDisposition::Deliver;
        };
        let target = KeyTarget {
            reader_attached: managed.platform_window.accessibility_active(),
            // A secure field declares a password IME purpose while focused
            // (`TextInputField::secure`, so every `PasswordField`).
            secure_field: managed
                .tree
                .ime_context_for_focused()
                .is_some_and(|ime| ime.purpose == teksilo_core::ime::ImePurpose::Password),
        };
        let keyboard = KeyboardState {
            modifiers: managed.current_modifiers,
            caps_lock: managed.caps_lock_active,
        };
        self.key_report.filter(
            &KeyInput::from_winit(key_event, is_synthetic),
            keyboard,
            target,
        )
    }

    /// What a key press says about the keyboard itself, kept whether or not
    /// the key reaches a widget: the OS toggles Caps Lock, and a chord made
    /// while Alt is held is not an Alt tap, even when a screen reader took
    /// the key.
    fn note_physical_key(&mut self, window_id: WindowId, key_event: &winit::event::KeyEvent) {
        if key_event.state != winit::event::ElementState::Pressed {
            return;
        }
        let key = event_translation::translate_key(&key_event.logical_key);
        let Some(managed) = self.wm.get_by_winit_mut(window_id) else {
            return;
        };
        // Track Caps Lock from the discrete key press (winit's
        // `ModifiersState` carries no lock state), toggling on each key-down
        // edge and pushing the result to `WindowState::caps_lock` for the
        // password-field warning.
        if matches!(key, Some(teksilo_core::event::Key::CapsLock)) {
            managed.caps_lock_active = !managed.caps_lock_active;
            managed
                .state
                .set_caps_lock_from_os(managed.caps_lock_active);
        }
        // Bare-Alt-tap detection: every other KeyDown while Alt is held
        // flips the sticky flag, so the falling edge of `alt_down` only
        // counts as a tap when no chord was composed. winit 0.30 reports the
        // modifier keys as `KeyboardInput` too, on Wayland and X11 alike, but
        // `translate_key` has no Teksilo key for Shift, Control, Alt or Super,
        // so they never count: Alt's own press, and Shift added to it, leave
        // the tap intact. Every key that does translate counts, Caps Lock
        // included.
        if key.is_some() {
            managed.state.note_non_alt_keydown_during_alt();
        }
    }

    fn handle_accessibility_actions(
        &mut self,
        window_id: WindowId,
        event: &WindowEvent,
        event_loop: &ActiveEventLoop,
    ) {
        // Collect events while holding the `ManagedWindow` borrow;
        // dispatch them below through `dispatch_in_window`, which
        // needs the borrow to be released first.
        let mut a11y_events: Vec<WidgetEvent> = Vec::new();
        if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
            managed.platform_window.process_accessibility_event(event);

            let actions = managed.platform_window.drain_accessibility_actions();
            for req in actions {
                // Synthetic NodeIds (TextRun children emitted by the
                // rich text editor) can't be decoded back to a
                // WidgetId by value alone — look them up via the
                // tree's reverse-map. For plain widget NodeIds the
                // infallible converter is fine.
                let target_widget = if teksilo_core::accessibility::is_synthetic(req.target_node) {
                    managed.tree.widget_for_synthetic(req.target_node)
                } else {
                    Some(teksilo_core::accessibility::node_id_to_widget_id(
                        req.target_node,
                    ))
                };
                let evt = WidgetEvent::AccessAction {
                    action: req.action,
                    target: target_widget,
                    target_node: req.target_node,
                    data: req.data,
                };
                a11y_events.push(evt);
            }
        }
        for evt in a11y_events {
            self.dispatch_in_window(window_id, evt, event_loop);
        }
    }

    fn handle_redraw_requested(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        // Pre-render: take the window out so we can construct a real
        // WindowOpsImpl and pass it into layout + render. This lets
        // rebuild-triggered handlers (data-driven state changes,
        // delayed-overlay activation, drag-tick) open windows.
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;
        #[cfg(not(target_os = "macos"))]
        let current_handle = current
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(current.platform_window.window_arc());

        if let Some(trace) = &mut self.idle_trace {
            trace.note_redraw_requested();
        }
        if current.tree.has_idle_work() {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_idle_callbacks_run();
            }
            current.tree.run_idle_callbacks(self.idle_budget);
        }

        let size = current.platform_window.surface_size();
        let sf = current.platform_window.scale_factor() as f32;
        let proposal = SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf);

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc.clone(),
            );
            current.tree.layout_with_ops(proposal, &mut ops);
        }

        // Size-to-content: after layout, measure the content's intrinsic height
        // at the fixed width and grow/shrink the OS window to fit. The native-
        // window modal path lays the tree out at the window's *exact* size, so
        // (unlike the in-tree overlay) the content's natural height never
        // reaches the OS window on its own — a `MessageBox` taller than its
        // fixed height clips, dropping the footer buttons below the client edge.
        // Drive the size through the reactive `WindowState::size()` → `SetSize`
        // path (drained in `post_event`); `last_autosize_height` guards against
        // a measure → resize → re-measure oscillation.
        if current.size_to_content.sizes_height() {
            let width_logical = size.0 as f32 / sf;
            if let Some(intrinsic) = current.tree.measure_root_intrinsic(SizeProposal {
                width: Some(width_logical),
                height: None,
            }) {
                let target_h = intrinsic.height.ceil().max(1.0) as u32;
                let cur_h = (size.1 as f32 / sf).round() as u32;
                if target_h != cur_h && current.last_autosize_height != Some(target_h) {
                    current.last_autosize_height = Some(target_h);
                    current
                        .state
                        .size()
                        .set((width_logical.round() as u32, target_h));
                }
            }
        }

        // Whether an assistive technology is attached to this window's adapter.
        // The adapter's handlers run off the UI thread and can only leave a
        // flag, so this is where the tree learns of it; both handlers request a
        // redraw, so a change never waits for unrelated activity. Attaching
        // proves nothing about screen readers (a magnifier or an automation
        // harness activates the adapter too) — detaching does, and
        // `set_at_client_attached` is where that asymmetry lives.
        let at_attached = current.platform_window.accessibility_active();
        current.tree.set_at_client_attached(at_attached);
        if at_attached {
            // Connect to the registry now rather than on the first key.
            self.key_report.warm_up();
        }

        // Kept unconditional: `sync_accessibility` is not a pure builder —
        // it steps the framework's live-region announcers, fills the
        // automation announcement ring and maintains `at_version` — and all
        // three must run every frame whether or not anything is listening.
        let a11y_update = current.tree.sync_accessibility();
        let walk = current.tree.a11y_walk_generation();
        let delivered_walk = current.a11y_delivered_walk;
        let delivered_at = current.a11y_delivered_at;
        let now = std::time::Instant::now();
        let needs_full = current.platform_window.take_accessibility_needs_full_tree();
        // Ten a second is fast enough that a magnifier tracking a scroll
        // never looks stuck, and slow enough that a flung list does not
        // bury AT-SPI under a bounds-changed signal per node per frame.
        const MOVE_DELIVERY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
        let mut attached = false;
        let mut delivered = false;
        {
            let a11y_update = &a11y_update;
            current.platform_window.update_accessibility_with(
                || {
                    // Decided in here, not outside: the closure runs only
                    // when an assistive technology is attached, so a window
                    // nobody is reading neither builds nor throttles.
                    attached = true;
                    let due = needs_full
                        || walk != delivered_walk
                        || delivered_at
                            .is_none_or(|at| now.duration_since(at) >= MOVE_DELIVERY_INTERVAL);
                    due.then(|| {
                        delivered = true;
                        a11y_update.clone()
                    })
                },
                || a11y_update.clone(),
            );
        }
        if delivered {
            current.a11y_delivered_walk = walk;
            current.a11y_delivered_at = Some(now);
        } else if attached {
            // Held back by the throttle, and nothing else will wake the loop
            // once the scroll stops — so the last frame's moves would sit
            // undelivered. Ask for one frame at the end of the window.
            //
            // Gated on `attached`: with no assistive technology listening the
            // closure never runs, and asking for a wake here would spin the
            // event loop at full rate forever on every app nobody is reading.
            let deadline = delivered_at
                .map(|at| at + MOVE_DELIVERY_INTERVAL)
                .unwrap_or(now);
            current.tree.request_wake_at(deadline);
        }

        // Catch-all IME reconcile: covers focus changes from any source
        // (access actions, programmatic focus, rebuild) that didn't go
        // through `dispatch_in_window`. Layout has settled, so the focused
        // node's descriptor is current. Cheap + deduped, safe every frame.
        Self::settle_ime(&mut current);

        let mut frame = {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc.clone(),
            );
            current.tree.render_with_ops(&mut ops)
        };
        let managed = &mut current;

        #[cfg(feature = "text")]
        {
            let atlas = self
                .typesetter
                .bridge()
                .borrow_mut()
                .atlas_info(managed.atlas_uploaded_version);
            if atlas.version != managed.atlas_uploaded_version
                && atlas.width > 0
                && atlas.height > 0
            {
                managed.platform_window.renderer_mut().upload_atlas(
                    atlas.width,
                    atlas.height,
                    &atlas.pixels,
                );
                managed.atlas_uploaded_version = atlas.version;
            }

            if atlas.glyphs_evicted {
                // Glyphs were evicted since the previous atlas_info call
                // (any path: snapshot scan, rich-text render scan, or
                // scale-factor reset). Every retained paint frame in
                // EVERY window may hold quads whose atlas UVs now point
                // at recycled slots — and invalidate_cache() below clears
                // the bridge's layout/glyph caches, which also kills the
                // touch_layout keep-alive for frames baked before the
                // clear. Invalidate all windows, not just the current
                // one; the others re-render at their own requested
                // redraw with fresh layouts and pull the current atlas
                // pixels through the version comparison above.
                self.typesetter.bridge().borrow_mut().invalidate_cache();
                managed.tree.invalidate_all_paints();
                for other in self.wm.iter_mut() {
                    other.tree.invalidate_all_paints();
                    other.platform_window.request_redraw();
                }
                // Re-render after atlas invalidation with a real ops
                // sink so rebuild-triggered handlers on this recovery
                // path can still open windows.
                let mut ops = crate::window_manager::WindowOpsImpl::new(
                    &mut self.wm,
                    event_loop,
                    current_id,
                    #[cfg(not(target_os = "macos"))]
                    current_handle,
                    current_arc.clone(),
                );
                frame = managed.tree.render_with_ops(&mut ops);
                let atlas2 = self
                    .typesetter
                    .bridge()
                    .borrow_mut()
                    .atlas_info(managed.atlas_uploaded_version);
                // The recovery re-render cannot legitimately evict again
                // (the eviction scan's generation-cadence gate just
                // reset), but atlas_info consumes the epoch delta — a
                // report here would be silently lost, so check the
                // assumption instead of assuming it.
                debug_assert!(
                    !atlas2.glyphs_evicted,
                    "glyph eviction during eviction recovery — epoch delta would be lost"
                );
                if atlas2.version != managed.atlas_uploaded_version
                    && atlas2.width > 0
                    && atlas2.height > 0
                {
                    managed.platform_window.renderer_mut().upload_atlas(
                        atlas2.width,
                        atlas2.height,
                        &atlas2.pixels,
                    );
                    managed.atlas_uploaded_version = atlas2.version;
                }
            }
        }

        // The wgpu surface is Rgba8UnormSrgb: it expects linear-light color
        // values and applies sRGB encoding on write. Our Color stores sRGB-
        // encoded bytes (as designers specify them), so we must linearize
        // the clear color here the same way we do for vertex colors.
        let clear = teksilo_render::vertex::srgb_to_linear_rgba(
            managed.tree.theme().colors.surface_main.to_array(),
        );
        match managed.platform_window.render_frame(&frame, clear) {
            teksilo_platform::FrameOutcome::Rendered => {
                if let Some(trace) = &mut self.idle_trace {
                    trace.note_rendered_frame();
                }
            }
            teksilo_platform::FrameOutcome::Skipped => {
                if !managed.occluded {
                    managed.platform_window.request_redraw();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            teksilo_platform::FrameOutcome::NeedsReconfigure => {
                if managed.platform_window.reconfigure_surface() {
                    managed.platform_window.request_redraw();
                } else {
                    event_loop.exit();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            teksilo_platform::FrameOutcome::DisplayLost => {
                // Every window in the process shares the one connection, so
                // there is nothing left to draw anywhere. Ask the loop to
                // unwind rather than request a frame that would return here.
                event_loop.exit();
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            teksilo_platform::FrameOutcome::Error(e) => {
                eprintln!("teksilo-app: {e}, reconfiguring surface");
                if managed.platform_window.reconfigure_surface() {
                    managed.platform_window.request_redraw();
                } else {
                    event_loop.exit();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
        }

        // A live per-frame effect (Pulse / Cycle / caret blink / drag
        // auto-scroll) leaves `frame_requested()` armed after this render.
        // We deliberately do NOT `request_redraw()` here: an immediate
        // redraw request makes winit skip the `WaitUntil` sleep and
        // free-run at the display's refresh rate — the exact 300 fps
        // uncapped behaviour we're removing. Instead the fixed 60 Hz
        // deadline published by `WidgetTree::frame_tick_deadline` (folded
        // into `next_timer_deadline`) drives the next frame: at the
        // deadline, `new_events(ResumeTimeReached)` calls
        // `request_redraw_all()`. This mirrors how the shader-quad
        // animation path has always paced itself, so per-frame animations
        // now show in the idle trace as `resume_time_reached` /
        // `request_redraw_all` rather than `frame_request`.

        self.wm.reinsert_managed(window_id, current);
    }

    fn handle_window_event_inner(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let teksilo_id = self.wm.teksilo_id_for_winit(window_id);

        // An attached screen reader hears of the key before anything acts on
        // it, and may take it for itself (Orca's own commands): a key taken
        // there goes no further. First of all, so that the reader's commands
        // work in a window a modal child blocks too.
        if let WindowEvent::KeyboardInput {
            event: key_event,
            is_synthetic,
            ..
        } = &event
            && self.report_key(window_id, key_event, *is_synthetic)
                == teksilo_platform::key_report::KeyDisposition::Drop
        {
            self.note_physical_key(window_id, key_event);
            self.update_control_flow(event_loop);
            return;
        }

        // A window blocked by a modal child swallows the user's *input* and
        // bounces it to the child, while still hearing everything the OS says
        // about the window itself. Which is which — and which of the swallowed
        // events is worth raising the child for — is
        // `input_loop::blocked_disposition`.
        if let Some(fid) = teksilo_id
            && self.wm.is_blocked(fid)
        {
            use crate::input_loop::BlockedDisposition;
            match crate::input_loop::blocked_disposition(&event) {
                BlockedDisposition::Deliver => {}
                BlockedDisposition::Swallow => {
                    self.update_control_flow(event_loop);
                    return;
                }
                BlockedDisposition::SwallowAndRaise => {
                    self.wm.refocus_modal_child(fid);
                    self.update_control_flow(event_loop);
                    return;
                }
            }
        }

        self.handle_accessibility_actions(window_id, &event, event_loop);

        // The pointer, touch and OS-gesture family goes through the platform
        // backend and the tree's sample doors rather than through an arm of
        // its own. One place, one clock, one set of suppressors — and the arms
        // themselves are `crate::input_routing`, which a test can drive with a
        // hand-written winit event and a bare tree.
        if crate::input_routing::is_pointer_input_event(&event) {
            self.dispatch_input_in_window(window_id, &event, event_loop);
            self.post_event(event_loop);
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(fid) = teksilo_id {
                    // Guarded close: the OS close button / Alt+F4 / Cmd+W
                    // is an interactive gesture, so it runs through the
                    // window's close guard (if any) on the next
                    // `process_pending` tick and may be vetoed.
                    self.wm.request_close(fid);
                }
            }
            WindowEvent::Resized(new_size) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.platform_window.resize(new_size);
                    // Mirror OS-initiated geometry / placement changes
                    // into WindowState so widgets bound to those signals
                    // re-render. The `*_from_os` setters flip the
                    // re-entrancy guard so observers on the signal do
                    // not push the change back out as a WindowCommand.
                    // Covers OS-initiated maximize (drag-to-top-snap on
                    // Wayland/Windows, green-light zoom on macOS) —
                    // query_window_placement reads the winit state and
                    // the Switcher glyph swap on `TitleBar`'s maximize
                    // button (bound to `WindowState::placement`) stays
                    // in sync.
                    let sf = managed.platform_window.scale_factor();
                    let logical_w = (new_size.width as f64 / sf).round().max(0.0) as u32;
                    let logical_h = (new_size.height as f64 / sf).round().max(0.0) as u32;
                    managed.state.set_size_from_os((logical_w, logical_h));
                    let placement = query_window_placement(managed.platform_window.window());
                    managed.state.set_placement_from_os(placement);
                    // A resize is when the safe area changes: on the one
                    // desktop platform that has one, it is zero in a window
                    // and non-zero once the window covers the camera housing,
                    // which is a resize into full screen.
                    Self::refresh_safe_area(managed);
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("resize");
                    }
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::Moved(pos) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    let sf = managed.platform_window.scale_factor();
                    let lx = (pos.x as f64 / sf).round() as i32;
                    let ly = (pos.y as f64 / sf).round() as i32;
                    managed.state.set_position_from_os((lx, ly));
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let mut teksilo_id = None;
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.translation_state.set_scale_factor(scale_factor);
                    managed.platform_window.set_scale_factor(scale_factor);
                    managed.tree.set_device_scale_factor(scale_factor as f32);
                    // The safe area is reported in points; moving between
                    // displays can change both the scale and the housing.
                    Self::refresh_safe_area(managed);
                    teksilo_id = Some(managed.teksilo_id);
                }
                // Keep the external-DnD backend's idea of the scale current:
                // dragging the window onto a monitor with a different scale
                // mid-drag would otherwise start reporting drops at the wrong
                // place (X11 only — see `ExternalDndGuard::set_scale_factor`).
                if let Some(teksilo_id) = teksilo_id
                    && let Some(handle) = self
                        .wm
                        .app_context_template()
                        .and_then(|t| {
                            t.app_state::<teksilo_platform::external_dnd::ExternalDndHandle>()
                        })
                        .cloned()
                {
                    handle.set_scale_factor(teksilo_id, scale_factor);
                }
                #[cfg(feature = "text")]
                {
                    self.typesetter.set_scale_factor(scale_factor as f32);
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                // Capture state before the alt_down write so we can
                // detect the falling edge without re-reading after.
                let alt_tap_action = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.current_modifiers = mods.state();
                    managed
                        .translation_state
                        .set_modifiers(event_translation::translate_modifiers(mods.state()));

                    let new_alt = mods.state().alt_key();
                    let prev_alt = managed.state.alt_down().get();
                    let other_pressed = managed.state.other_key_pressed_during_alt();
                    // Alt-tap tracking: surface the OS Alt-held edge on
                    // the window's `alt_down` signal so `MenuLabel` can
                    // gate mnemonic underlines and `MenuBar` can detect
                    // bare-Alt-tap on the falling edge. winit reports
                    // Alt presses through `ModifiersChanged` (not as a
                    // `Key::Alt` KeyDown, which doesn't exist in our
                    // Key enum), so this is the only correct hook.
                    managed.state.set_alt_from_os(new_alt);
                    // Detect the bare-Alt-tap pattern: true → false
                    // with no non-Alt KeyDowns during the hold.
                    if prev_alt && !new_alt && !other_pressed {
                        managed
                            .state
                            .menubar_dispatcher()
                            .and_then(|d| d.on_alt_tap())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(action) = alt_tap_action {
                    self.apply_menubar_action(window_id, action, event_loop);
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                self.note_physical_key(window_id, &key_event);
                let maybe_evt = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    event_translation::translate_key(&key_event.logical_key).map(|key| {
                        let modifiers =
                            event_translation::translate_modifiers(managed.current_modifiers);
                        let text = key_event.text.as_ref().map(|t| t.to_string());
                        match key_event.state {
                            winit::event::ElementState::Pressed => WidgetEvent::KeyDown {
                                key,
                                modifiers,
                                text,
                            },
                            winit::event::ElementState::Released => {
                                WidgetEvent::KeyUp { key, modifiers }
                            }
                        }
                    })
                } else {
                    None
                };
                if let Some(evt) = maybe_evt {
                    // Window-level menubar pre-dispatch (F10 / Alt+letter):
                    // intercepts BEFORE the normal focus-based path so the
                    // event reaches the menubar even when focus is in a
                    // TextInput or some other unrelated widget. Matches
                    // Win32's `WM_SYSKEYDOWN` → `DefWindowProc` route.
                    let intercept = if let WidgetEvent::KeyDown { key, modifiers, .. } = &evt {
                        self.wm
                            .get_by_winit_mut(window_id)
                            .and_then(|m| {
                                m.state.menubar_dispatcher().map(|d| {
                                    d.try_handle(&teksilo_core::window::MenubarKeyEvent {
                                        key: *key,
                                        modifiers: *modifiers,
                                    })
                                })
                            })
                            .flatten()
                    } else {
                        None
                    };
                    if let Some(action) = intercept {
                        self.apply_menubar_action(window_id, action, event_loop);
                    } else {
                        self.dispatch_in_window(window_id, evt, event_loop);
                    }
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("keyboard");
                    }
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                // Dedup consecutive empty preedits at the funnel. Some Linux IME
                // backends (ibus / fcitx via winit) flood empty `Ime::Preedit("")`
                // events while a field is focused. The first is meaningful (it
                // clears any active composition); every consecutive repeat is a
                // no-op that would still translate + dispatch through the tree AND
                // wake a full unconditional layout+render pass here. Skip the
                // repeats entirely — neither dispatch nor redraw. Any non-empty
                // preedit (or a Commit / Enabled / Disabled) resets the flag so
                // the next empty preedit is again treated as meaningful.
                let empty_preedit =
                    matches!(&ime, winit::event::Ime::Preedit(t, _) if t.is_empty());
                let skip = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    crate::window_manager::ime_should_skip_empty_preedit(
                        &mut managed.last_ime_preedit_empty,
                        empty_preedit,
                    )
                } else {
                    false
                };
                if !skip {
                    let maybe_evt = if self.wm.get_by_winit_mut(window_id).is_some() {
                        event_translation::translate_ime(ime)
                    } else {
                        None
                    };
                    if let Some(evt) = maybe_evt {
                        self.dispatch_in_window(window_id, evt, event_loop);
                    }
                    if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                        if let Some(trace) = &mut self.idle_trace {
                            trace.note_redraw_request("ime");
                        }
                        managed.platform_window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw_requested(window_id, event_loop);
            }
            WindowEvent::ThemeChanged(winit_theme) => {
                self.handle_theme_changed(winit_theme);
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.platform_window.request_redraw();
                }
            }
            // Pause all looping animations on the unfocused window so it
            // stops waking the event loop at the animation frame
            // interval. The scheduler rebases start_time on resume so
            // the animation phase is continuous — a half-swept
            // indeterminate bar picks up at exactly the same position,
            // not snapped forward by the elapsed unfocused time.
            //
            // On Linux/Windows (winit 0.30) minimize fires `Focused(false)`
            // — no separate minimize event — so this path covers it.
            WindowEvent::Focused(focused) => {
                let mut newly_focused = None;
                let active = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.focused = focused;
                    managed.state.set_focused_from_os(focused);
                    Some(managed.focused && !managed.occluded)
                } else {
                    None
                };
                // Every live pointer is revoked at both ends on the way out —
                // the user releases the button over whatever took focus, and
                // this window is never told. See `input_routing`.
                if let Some(active) = active {
                    self.set_window_active_in_window(
                        window_id,
                        active,
                        teksilo_core::pointer::CancelReason::WindowDeactivated,
                        event_loop,
                    );
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    // Drive a redraw on every focus transition so the
                    // window-active observers (caret hide/restore, selection
                    // desaturation, DimWhenInactive) reach a paint pass
                    // promptly — the OS does not reliably emit RedrawRequested
                    // on focus change across all platforms.
                    managed.platform_window.request_redraw();
                    if focused {
                        newly_focused = Some(managed.teksilo_id);
                    }
                }
                // A window regaining focus is the natural, zero-idle-cost moment
                // to re-check the OS accessibility preferences (WCAG / EN 301
                // 549 §11.7): the user may have toggled "increase contrast" /
                // "reduce motion" / text scale in System Settings and switched
                // back. `refresh_accessibility_preferences` applies any change
                // to every window (marking them dirty for repaint).
                if focused {
                    self.wm.refresh_accessibility_preferences();
                }
                // The global native menu (macOS) follows window focus: make the
                // focused window's installed menu the visible one.
                if let Some(teksilo_id) = newly_focused
                    && let Some(handle) = self.wm.app_context_template().and_then(|t| {
                        t.app_state::<teksilo_platform::native_menu::NativeMenuHandle>()
                            .cloned()
                    })
                {
                    handle.activate_window(teksilo_id);
                }
            }
            // macOS-only in winit 0.30 (X11/Wayland/Windows never emit
            // this). Handled for parity with Focused so a macOS app
            // that is hidden behind another window — still focused —
            // also parks its animations.
            WindowEvent::Occluded(occluded) => {
                let active = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.occluded = occluded;
                    Some(managed.focused && !managed.occluded)
                } else {
                    None
                };
                // `Occluded`, not `WindowDeactivated`: a widget told its
                // pointer was revoked is told *why*, and "the window went
                // behind something" is not "the user went elsewhere".
                if let Some(active) = active {
                    self.set_window_active_in_window(
                        window_id,
                        active,
                        teksilo_core::pointer::CancelReason::Occluded,
                        event_loop,
                    );
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    // Drive a redraw on both directions. On reveal
                    // (`!occluded`) the render loop stopped pinging while we
                    // were occluded, so without this nudge the window stays
                    // frozen until the user moves the mouse or hits a key. On
                    // occlusion (`occluded`) the active-state flip must reach a
                    // paint pass so the caret hides / selection desaturates
                    // before the window is hidden behind another.
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::ActivationTokenDone { token, .. } => {
                // A `request_activation_token` we issued resolved — hand the
                // freshly-minted token to whoever asked (child-process spawn or
                // an IPC peer). One request outstanding per window, so key by
                // window id and ignore the serial.
                if let Some(cb) = self.wm.take_activation_token_callback(window_id) {
                    cb(Some(token.into_raw()));
                }
            }
            _ => {}
        }

        self.post_event(event_loop);
    }

    fn handle_theme_changed(&mut self, winit_theme: winit::window::Theme) {
        // Read the mode from the WindowManager (the live owner) so a runtime
        // switch to "follow system" via `EventContext::follow_system_theme`
        // is honoured here too. OS-following results carry the id "system".
        match self.wm.theme_mode() {
            ThemeMode::Manual => {} // ignore OS theme changes
            ThemeMode::FollowSystem => {
                // Trust winit's per-window signal (authoritative on
                // macOS/Windows where OS-colour querying is unimplemented).
                let theme = match winit_theme {
                    winit::window::Theme::Dark => teksilo_core::presets::intui::dark(),
                    winit::window::Theme::Light => teksilo_core::presets::intui::light(),
                }
                .with_id("system");
                self.wm.set_theme(theme);
            }
            // Native adopts the OS's actual colours on Linux; on macOS/Windows
            // (no OS-colour query) it follows winit's authoritative light/dark
            // hint. The shared helper stamps the "system" id.
            ThemeMode::Native => self
                .wm
                .apply_os_theme(Some(matches!(winit_theme, winit::window::Theme::Dark))),
        }
    }
}

impl ApplicationHandler<AppEvent> for TeksiloAppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Before the first window, and so before the wgpu instance exists: the
        // OpenGL backend cannot present to a window whose display connection it
        // was never told about, and that backend is all a machine without a
        // Vulkan driver has. Idempotent, so a resume after suspend is a no-op.
        teksilo_platform::install_display_handle(event_loop.owned_display_handle());

        if !self.initial_created
            && let Some(config) = self.initial_window.take()
        {
            self.wm.create_window(config, event_loop);
            self.initial_created = true;
        }

        self.process_pending(event_loop);
        self.update_control_flow(event_loop);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // Whether the OS woke us or our own timer did. A pen shim reads the
        // digitizer on a thread of its own and buffers what it sees, so an
        // external wake is the signal that a packet may be in flight: the
        // event that woke us and the packet came from the same source, but the
        // shim has not necessarily dispatched it yet. `about_to_wait` arms one
        // catch-up look for exactly that case — see `Self::pen_deadline`.
        self.woken_externally = !matches!(cause, StartCause::ResumeTimeReached { .. });
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_resume_time_reached();
                trace.note_request_redraw_all();
            }
            // Redraw only the windows whose frame deadline is actually due —
            // NOT every window. A blanket redraw here pins non-animating
            // windows at the animation frame rate and, on Windows (one
            // RedrawRequested serviced per loop iteration), starves an inactive
            // window's own pending repaint so it freezes. See
            // `WindowManager::request_redraw_due`.
            self.wm.request_redraw_due(Instant::now());
        }
        self.update_control_flow(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        if let Some(handler) = &mut self.app_event_handler {
            handler(&event);
        }
        // Composed framework observers (see `AppEventObservers` /
        // `TeksiloAppBuilder::register_app_event_observer`) run in
        // addition to the app's own `on_app_event` handler above — this
        // is what lets `teksilo::install_toast` react to
        // `AppEvent::SettingsWriteFailed` without clobbering (or being
        // clobbered by) an app that also called `on_app_event`.
        if let Some(template) = self.wm.app_context_template()
            && let Some(observers) =
                template.app_state::<crate::app_event_observers::AppEventObservers>()
        {
            (observers.0)(&event);
        }
        match event {
            // Backend-event subscription delivery (architecture §9.4): look
            // up the UI-side callback in the shared app context and invoke
            // it with the downcast event payload. The shared template is
            // the same Rc held by every window's tree, so we don't need to
            // route by window.
            AppEvent::SubscriptionEvent { sub_id, event } => {
                // A context-bearing subscription (`subscribe_event_with_ctx`)
                // needs a fresh `EventContext` minted from its window's tree;
                // a plain one dispatches against the shared template with no
                // context. `try_dispatch_subscription_with_ctx` returns `true`
                // when `sub_id` names a context-bearing subscription (so we
                // skip the plain path — a sub_id lives in exactly one map).
                if !self.try_dispatch_subscription_with_ctx(sub_id, &*event, event_loop)
                    && let Some(template) = self.wm.app_context_template()
                {
                    template.dispatch_subscription_event(sub_id, &*event);
                }
            }
            // Hot-reload of an `.ftl` file registered via
            // `I18nConfig::runtime_override(...)`. Architecture §12.7:
            // the reload must *not* trigger a composite rebuild — only
            // the version signal is bumped, and the existing binding
            // system propagates the change to every `LocalizedString`
            // observer. Direction and active locale are unchanged.
            AppEvent::I18nReload { locale, path } => {
                let parsed: Result<teksilo_i18n::LanguageIdentifier, _> = locale.parse();
                match parsed {
                    Ok(loc) => {
                        let reloaded = teksilo_i18n::thread_local::with_active(|mgr| {
                            mgr.reload_from_path(&loc, &path)
                        });
                        match reloaded {
                            Some(Ok(())) => {}
                            Some(Err(e)) => eprintln!(
                                "teksilo-app: hot-reload failed for {loc} ({}): {e}",
                                path.display()
                            ),
                            None => eprintln!(
                                "teksilo-app: hot-reload event for {loc} but no i18n manager installed"
                            ),
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "teksilo-app: hot-reload event with invalid locale `{locale}`: {e}"
                        )
                    }
                }
            }
            // Live cross-process settings sync: a `teksilo-settings`
            // managed file changed on disk (a peer process's write, or
            // harmlessly this process's own write being noticed by its
            // own watcher). Look the path up in the app's
            // `SettingsRegistry` and let it dispatch to whichever
            // `Reloadable` owns it. This must *not* trigger a composite
            // rebuild — `reload_from_disk` only mutates signals/models
            // in place, and the existing reactive binding system
            // propagates the change to every observer, exactly like
            // `I18nReload` above.
            AppEvent::SettingsReload { path } => {
                if let Some(template) = self.wm.app_context_template()
                    && let Some(registry) =
                        template.app_state::<teksilo_settings::SettingsRegistry>()
                    && let Err(e) = registry.dispatch(&path)
                {
                    eprintln!(
                        "teksilo-app: settings reload failed for {}: {e}",
                        path.display()
                    );
                }
            }
            // F3: a `teksilo-settings` `DebouncedWriter` permanently gave
            // up on a queued write (retry cap reached, or a still-failing
            // write forced by process teardown) — the patches for `path`
            // were discarded. `teksilo-app` itself stays widget-agnostic
            // (it cannot depend on `teksilo-widgets`' `Toast` /
            // `NotificationArchive`), so this log is only half the
            // story: the composed `AppEventObservers` dispatched just
            // above also sees this event, and `teksilo::install_toast`
            // (the umbrella crate, which sees both `AppEvent` and
            // `Toast`) registers an observer that turns it into a
            // persistent error toast — see
            // `ToastRegistry::show_settings_write_failed`. This log
            // stays too: a headless/CI app with no toast host installed
            // still needs *some* signal that a write was lost.
            AppEvent::SettingsWriteFailed {
                path,
                attempts,
                dropped_patches,
                message,
            } => {
                eprintln!(
                    "teksilo-app: settings write permanently failed for {} after {} attempts ({} patches dropped): {}",
                    path.display(),
                    attempts,
                    dropped_patches,
                    message
                );
            }
            // Title-bar hosts route their `close()` through this variant so
            // the operation hops back onto the main thread before touching
            // `WindowManager` (see `title_bar_host.rs`). File-dialog
            // backends post their results through the same variant. The
            // arm tries each known payload type in turn; unrecognized
            // payloads are ignored — application-authored `send_external`
            // payloads can coexist with framework-internal ones.
            AppEvent::External(payload) => {
                // Try each framework-internal payload type in turn; the first
                // that consumes it wins. Unrecognized payloads fall through to
                // the title-bar / close-request downcast chain.
                let payload = self
                    .try_route_file_dialog_payload(payload, event_loop)
                    .err();
                let payload = match payload {
                    None => None,
                    Some(payload) => self
                        .try_route_external_dnd_payload(payload, event_loop)
                        .err(),
                };
                let payload = match payload {
                    None => None,
                    Some(payload) => self
                        .try_route_async_completion_payload(payload, event_loop)
                        .err(),
                };
                let payload = match payload {
                    None => None,
                    Some(payload) => self
                        .try_route_native_menu_payload(payload, event_loop)
                        .err(),
                };
                let payload = match payload {
                    None => None,
                    Some(payload) => self.try_route_web_view_payload(payload, event_loop).err(),
                };
                #[cfg(all(feature = "automation", debug_assertions))]
                let payload = match payload {
                    None => None,
                    Some(payload) => self.try_route_automation_payload(payload, event_loop).err(),
                };
                if let Some(payload) = payload {
                    // Did one of the framework's own built-in arms below claim
                    // it? Only what is left over is offered to the app's
                    // `on_external_with_ctx` router (see
                    // `route_external_with_ctx`). The framework arms run FIRST,
                    // so an app router that returns `true` too eagerly can never
                    // swallow a `CloseWindowRequest` or a title-bar synthetic
                    // event; and because the answer is the chain's own trailing
                    // `else`, a built-in arm added later is withheld from the
                    // app router automatically — there is no second list of
                    // "framework-owned types" to keep in step.
                    let mut consumed = true;
                    {
                        if let Some(req) = payload.downcast_ref::<CloseWindowRequest>() {
                            // Custom-chrome (Teksilo-drawn) title-bar close
                            // button — an interactive gesture, so it runs
                            // through the window's close guard (guarded
                            // close), matching the OS close button.
                            self.wm.request_close(req.teksilo_id);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarSyntheticEvent>() {
                            // Windows custom-chrome wndproc sends this when
                            // `WM_NCLBUTTONUP` fires over a control-button
                            // hit-region. The button's pixels are owned by
                            // the OS so the widget tree never saw the click;
                            // re-issue it as a synthetic tap on the
                            // matching `ControlButton`.
                            self.wm
                                .route_title_bar_synthetic_tap(evt.teksilo_id, evt.target);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarHoverEvent>() {
                            // Same idea for hover: `WM_NCMOUSEMOVE` over a
                            // control-button hit-region delivers an
                            // entered/leave event the widget tree never
                            // sees, so we drive the matching button's
                            // hover signal explicitly.
                            self.wm.route_title_bar_synthetic_hover(
                                evt.teksilo_id,
                                evt.target,
                                evt.entered,
                            );
                        } else if let Some(inject) = payload.downcast_ref::<SyntheticImeInject>() {
                            // Test / demo hook: replay a scripted IME
                            // sequence into the focused window's focused
                            // widget through the real dispatch path — no OS
                            // IME needed. Mirrors exactly what the
                            // `WindowEvent::Ime` arm produces.
                            let target = self
                                .wm
                                .windows_map()
                                .iter()
                                .find(|(_, m)| m.focused)
                                .or_else(|| self.wm.windows_map().iter().next())
                                .map(|(id, _)| *id);
                            if let Some(winit_id) = target {
                                for evt in inject.events.clone() {
                                    self.dispatch_in_window(winit_id, evt, event_loop);
                                }
                            }
                        } else if let Some(req) =
                            payload.downcast_ref::<teksilo_core::RepaintWindowRequest>()
                        {
                            // Off-thread "repaint this window" — e.g. a
                            // terminal's PTY-reader thread whose bytes changed a
                            // widget's content outside the UI thread. A bare
                            // redraw re-presents the cached frame, so mark the
                            // window's tree paint-dirty; the unconditional
                            // `request_redraw_all()` below then re-runs the
                            // changed widget's `paint()`.
                            let winit_id =
                                self.wm.teksilo_to_winit_map().get(&req.window_id).copied();
                            if let Some(winit_id) = winit_id
                                && let Some(managed) = self.wm.get_by_winit_mut(winit_id)
                            {
                                managed.tree.mark_all_needs_paint_only();
                            }
                        } else {
                            consumed = false;
                        }
                    }
                    if !consumed {
                        self.route_external_with_ctx(&*payload, event_loop);
                    }
                }
            }
            _ => {}
        }
        if let Some(trace) = &mut self.idle_trace {
            trace.note_request_redraw_all();
        }
        self.wm.request_redraw_all();
        self.post_event(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.handle_window_event_inner(event_loop, window_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Drive any registered per-turn closure (the async executor poll when
        // `teksilo-async` is installed) before computing the next control
        // flow. A `true` return means tasks advanced and may have mutated
        // reactive state, so repaint the open windows — mirroring the
        // subscription-delivery redraw in `user_event`.
        if let Some(tick) = &mut self.loop_tick
            && tick()
        {
            self.wm.request_redraw_all();
        }
        // Once per event-loop turn, which is what the shim's own contract
        // asks for. A digitizer reads on its own thread and buffers; this is
        // the only place that asks it what it saw.
        self.pump_pen_sources(event_loop);
        self.refresh_occluded_band();
        self.process_pending(event_loop);
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }
}

/// Payload used by `TitleBarHostCallbacks::request_close` to route a
/// host-initiated close back to the main event loop. The host's
/// close callback boxes one of these through `AppEventProxy::send_external`;
/// `TeksiloAppHandler::user_event` downcasts the payload and calls
/// `WindowManager::request_close` so the window runs its close guard and
/// tears down on the next tick (matching the `WindowEvent::CloseRequested`
/// path).
#[derive(Debug, Clone, Copy)]
pub struct CloseWindowRequest {
    pub teksilo_id: TeksiloWindowId,
}

/// Test / demo payload that replays a scripted IME sequence into the
/// focused window's focused widget, through the same dispatch path the
/// real `WindowEvent::Ime` arm uses — so the full preedit pipeline
/// (document mutation, underline, caret-area reporting, AT selection) can
/// be exercised without an OS input method installed.
///
/// Post it via [`AppEventPoster::post_external`](teksilo_core::AppEventPoster)
/// (reachable from a handler with `ctx.poster()`).
#[derive(Debug, Clone)]
pub struct SyntheticImeInject {
    pub events: Vec<teksilo_core::event::WidgetEvent>,
}

// `TitleBarSyntheticEvent` and `TitleBarHoverEvent` live in
// `teksilo_core::window_chrome` so teksilo-platform (which posts them from
// the Windows wndproc subclass) and teksilo-app (which routes them) can
// both name the type without teksilo-platform depending on teksilo-app.
pub use teksilo_core::{TitleBarHoverEvent, TitleBarSyntheticEvent};

/// A thread-safe handle for posting `AppEvent`s to the UI thread.
///
/// Clone and send to background threads. The event loop wakes up
/// and processes the event like any other input.
#[derive(Clone)]
pub struct AppEventProxy {
    inner: winit::event_loop::EventLoopProxy<AppEvent>,
}

impl AppEventProxy {
    /// Post a background completion event.
    pub fn send_background_complete(&self, operation_id: String) {
        let _ = self
            .inner
            .send_event(AppEvent::BackgroundComplete { operation_id });
    }

    /// Post a background progress event.
    pub fn send_background_progress(&self, operation_id: String, percent: f32, message: String) {
        let _ = self.inner.send_event(AppEvent::BackgroundProgress {
            operation_id,
            percent,
            message,
        });
    }

    /// Post an arbitrary external event.
    pub fn send_external(&self, payload: impl std::any::Any + Send + 'static) {
        let _ = self.inner.send_event(AppEvent::External(Box::new(payload)));
    }

    /// Post a pre-boxed external event. Used by callers that already
    /// hold a `Box<dyn Any + Send>` (notably
    /// `TitleBarHostCallbacks::post_external`, which abstracts the
    /// posting mechanism behind a closure that teksilo-core can hold
    /// without depending on winit).
    pub fn send_external_boxed(&self, payload: Box<dyn std::any::Any + Send>) {
        let _ = self.inner.send_event(AppEvent::External(payload));
    }

    /// Post a backend-event delivery for the given subscription id. Called
    /// by the framework's event-source wrapper from the publisher thread.
    pub fn post_subscription_event(
        &self,
        sub_id: SubscriptionId,
        event: Box<dyn std::any::Any + Send>,
    ) {
        let _ = self
            .inner
            .send_event(AppEvent::SubscriptionEvent { sub_id, event });
    }
}

/// `AppEventProxy` implements [`AppEventPoster`] directly so it can be both the
/// `Arc<dyn AppEventPoster>` every widget tree holds AND handed to background
/// integrations (e.g. the `teksilo-async` executor's cross-thread waker, wired
/// via [`TeksiloAppBuilder::on_ready`]). teksilo-core cannot import winit, so
/// this trait implementation lives here.
impl AppEventPoster for AppEventProxy {
    fn post_subscription_event(
        &self,
        sub_id: SubscriptionId,
        event: Box<dyn std::any::Any + Send>,
    ) {
        let _ = self
            .inner
            .send_event(AppEvent::SubscriptionEvent { sub_id, event });
    }

    fn post_external(&self, payload: Box<dyn std::any::Any + Send>) {
        let _ = self.inner.send_event(AppEvent::External(payload));
    }
}

/// Builder for a Teksilo application.
pub struct TeksiloAppBuilder {
    theme: Theme,
    theme_mode: ThemeMode,
    pen_batching: teksilo_platform::PenBatching,
    #[cfg(feature = "text")]
    typesetter: Option<SharedTypesetter>,
    #[cfg(feature = "text")]
    font_registrars: Vec<Box<dyn teksilo_text::FontRegistrar>>,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    external_ctx_handler: Option<ExternalCtxHandler>,
    on_ready: Vec<Box<dyn FnOnce(AppEventProxy)>>,
    initial_window: Option<WindowConfig>,
    /// Type-erased adapter for the application's backend event source.
    /// Installed via `event_source<S>(source)`.
    event_source: Option<EventSourceAdapter>,
    /// Application-scoped values keyed by `TypeId`.
    /// Installed via `app_state::<T>(value)` and reachable from any
    /// `BuildContext` via `ctx.app_state::<T>()`.
    app_state_registry: HashMap<TypeId, Box<dyn Any>>,
    /// Internationalization configuration. Installed
    /// via `i18n(I18nConfig)`. When present, an `I18nManager` is built at
    /// `build_headless` / `run` time and registered on the thread-local so
    /// `tr!`-expanded code can resolve translations.
    i18n: Option<I18nConfig>,
    /// Tooltip content entries registered via
    /// [`register_tooltips`](Self::register_tooltips). Frozen into a
    /// thread-local registry in `run` / `build_headless` before the
    /// first frame builds.
    tooltip_contents: Vec<teksilo_widgets::tooltip::TooltipContent>,
    /// OS-correct application paths (config / data dirs). Set via
    /// [`application`](Self::application) or [`app_paths`](Self::app_paths).
    /// Required when `settings_bundle` is set.
    app_paths: Option<teksilo_settings::AppPaths>,
    /// Persistence configuration. When present, the bundle is opened
    /// at startup and each enabled service is registered into the
    /// `app_state` registry under its concrete type.
    settings_bundle: Option<teksilo_settings::SettingsBundle>,
    /// Whether `run()` should start a `SettingsWatcher` over the settings
    /// directories so a peer process's write is picked up live. On by
    /// default whenever a settings bundle is configured — this is the
    /// entire point of `SettingsBundle`'s cross-process-safe writes.
    /// Toggle off via [`settings_watch`](Self::settings_watch) for tests
    /// or environments without a usable filesystem watcher.
    settings_watch_enabled: bool,
    /// Telemetry configuration. When present, the bundle is opened
    /// after `settings_bundle` (it depends on `SettingsStore`) and the
    /// resulting `OpenedTelemetry` + `TelemetryContext` are registered
    /// into the `app_state` registry. The `TelemetryContext` is the
    /// hook the dispatch tap in
    /// [`teksilo_core::widget_tree::WidgetTree::dispatch_intent`] uses to
    /// emit `intent.dispatched` events.
    #[cfg(feature = "telemetry")]
    telemetry_bundle: Option<teksilo_telemetry::TelemetryBundle>,
    /// Per-loop-turn closure + poll flag installed via
    /// [`on_loop_tick`](Self::on_loop_tick). Async-agnostic; moved into the
    /// handler at `run`.
    loop_tick: Option<Box<dyn FnMut() -> bool>>,
    loop_tick_poll: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    /// Whether the windows' trees record announcements. Set by the automation
    /// bridge, their one reader in a running application.
    records_announcements: bool,
}

impl TeksiloAppBuilder {
    pub fn new() -> Self {
        Self {
            theme: teksilo_core::presets::intui::light(),
            theme_mode: ThemeMode::Manual,
            pen_batching: teksilo_platform::PenBatching::default(),
            #[cfg(feature = "text")]
            typesetter: None,
            #[cfg(feature = "text")]
            font_registrars: Vec::new(),
            app_event_handler: None,
            external_ctx_handler: None,
            on_ready: Vec::new(),
            initial_window: None,
            event_source: None,
            app_state_registry: HashMap::new(),
            i18n: None,
            tooltip_contents: Vec::new(),
            app_paths: None,
            settings_bundle: None,
            settings_watch_enabled: true,
            #[cfg(feature = "telemetry")]
            telemetry_bundle: None,
            loop_tick: None,
            loop_tick_poll: None,
            records_announcements: false,
        }
    }

    /// Make the windows' trees record what the platform adapters announce,
    /// for a reader in process. The automation bridge is the one reader a
    /// running application has; see
    /// [`WidgetTree::set_records_announcements`].
    // The bridge exists only in a debug build with the `automation` feature,
    // so any other build has no caller.
    #[cfg_attr(not(all(feature = "automation", debug_assertions)), allow(dead_code))]
    pub(crate) fn record_announcements(mut self) -> Self {
        self.records_announcements = true;
        self
    }

    /// Whether [`record_announcements`](Self::record_announcements) was asked.
    #[cfg(all(test, feature = "automation", debug_assertions))]
    pub(crate) fn records_announcements(&self) -> bool {
        self.records_announcements
    }

    /// Identify the application for OS-correct path resolution. The
    /// `(qualifier, organization, application)` triple follows the
    /// `directories` convention (e.g. `("eu", "FernTech", "Skribisto")`).
    /// Required when [`settings`](Self::settings) is used.
    ///
    /// # Panics
    ///
    /// Panics if the OS does not expose a usable home directory
    /// (typically a sandboxed environment with `HOME` unset). Use
    /// [`app_paths`](Self::app_paths) to supply an explicit path
    /// in that situation.
    pub fn application(mut self, qualifier: &str, organization: &str, application: &str) -> Self {
        let paths = teksilo_settings::AppPaths::new(qualifier, organization, application)
            .unwrap_or_else(|| {
                panic!(
                    "TeksiloAppBuilder::application(\"{qualifier}\", \"{organization}\", \
                     \"{application}\"): could not resolve a usable OS config directory. \
                     This typically happens in sandboxed environments with no HOME set. \
                     Use TeksiloAppBuilder::app_paths(AppPaths::for_testing(...) or \
                     AppPaths::from_dirs(...)) to supply an explicit location.",
                )
            });
        self.app_paths = Some(paths);
        self
    }

    /// Provide an explicit [`AppPaths`](teksilo_settings::AppPaths). Used
    /// for portable-mode apps and tests.
    pub fn app_paths(mut self, paths: teksilo_settings::AppPaths) -> Self {
        self.app_paths = Some(paths);
        self
    }

    /// Read the currently-configured `AppPaths`, if any. Used by
    /// builder-extension traits (e.g. `install_toast` in `teksilo`)
    /// that need to open persistent files at install time before
    /// `run` fires.
    pub fn configured_app_paths(&self) -> Option<&teksilo_settings::AppPaths> {
        self.app_paths.as_ref()
    }

    /// Configure the persistence bundle. When `run`/`build_headless`
    /// fires, the bundle is opened against the configured `AppPaths`
    /// and every active service is registered in `app_state`, where
    /// it becomes reachable via the
    /// [`SettingsExt`](teksilo_settings::SettingsExt) trait.
    ///
    /// # Panics
    ///
    /// Panics during `run` / `build_headless` if no `AppPaths` was
    /// configured first via [`application`](Self::application) or
    /// [`app_paths`](Self::app_paths).
    pub fn settings(mut self, bundle: teksilo_settings::SettingsBundle) -> Self {
        self.settings_bundle = Some(bundle);
        self
    }

    /// Enable or disable the live cross-process settings-reload watcher
    /// started in [`run`](Self::run) (windowed apps only —
    /// [`build_headless`](Self::build_headless) never starts one, since
    /// there is no event loop to post the reload event through).
    ///
    /// **On by default** whenever [`settings`](Self::settings) is
    /// configured: this is what makes a peer process's write to a
    /// shared settings file (Skribisto's one-process-per-project model
    /// shares `general.toml` / `recents.toml` / `window_state.toml`
    /// across every open project) show up in this process's UI with no
    /// restart and no polling. Pass `false` to opt out — e.g. a
    /// sandboxed test environment with no usable filesystem watcher, or
    /// an app that wants to poll `Reloadable::reload_from_disk` on its
    /// own schedule instead.
    pub fn settings_watch(mut self, enabled: bool) -> Self {
        self.settings_watch_enabled = enabled;
        self
    }

    /// Configure the telemetry stack (`teksilo-telemetry`). Mirrors
    /// [`settings`](Self::settings): the bundle is opened during
    /// `run` / `build_headless` against the configured `AppPaths`
    /// **and** the live `SettingsStore`, and the resulting handles
    /// (`OpenedTelemetry`, `TelemetryContext`, `DynamicReporter`) are
    /// registered into `app_state`. Apps reach them via
    /// [`teksilo_telemetry::TelemetryExt`] (`use teksilo_telemetry::TelemetryExt;`).
    ///
    /// # Panics
    ///
    /// Panics during `run` / `build_headless` if no `AppPaths` was
    /// configured first via [`application`](Self::application) or
    /// [`app_paths`](Self::app_paths), or if no
    /// [`settings`](Self::settings) bundle was registered (the
    /// telemetry consent file is opened via the same `AppPaths` and
    /// the endpoint-override key is read from the `SettingsStore`).
    #[cfg(feature = "telemetry")]
    pub fn telemetry(mut self, bundle: teksilo_telemetry::TelemetryBundle) -> Self {
        self.telemetry_bundle = Some(bundle);
        self
    }

    /// Register the application's tooltip string catalog.
    ///
    /// Each [`TooltipContent`](teksilo_widgets::tooltip::TooltipContent)
    /// in the list maps a short stable key (referenced from inline
    /// markup as `[label](:key)`) to a translatable body, an optional
    /// long-form "more" body revealed by the Accordion disclosure
    /// inside a sticky rich tooltip, and an optional keyboard shortcut
    /// (a literal label, or a registered shortcut id via
    /// [`TooltipContent::for_shortcut`](teksilo_widgets::tooltip::TooltipContent::for_shortcut),
    /// which tracks user rebinds).
    ///
    /// The list is merged into the application's tooltip catalog. Register
    /// at app boot, before `run()` — the accumulated catalog is frozen into
    /// a read-only registry before the first frame builds, and installing it
    /// twice panics in debug builds.
    ///
    /// ```ignore
    /// use teksilo_widgets::tooltip::TooltipContent;
    ///
    /// TeksiloAppBuilder::new()
    ///     .register_tooltips(vec![
    ///         TooltipContent::new("save-as", tr!(save_as_tooltip))
    ///             .for_shortcut("app.save_as"),
    ///         TooltipContent::new("autosave", tr!(autosave_tooltip))
    ///             .with_more(tr!(autosave_tooltip_more)),
    ///     ])
    ///     // …
    /// ```
    ///
    /// **Multiple calls accumulate**, like [`Self::register_fonts`] and
    /// [`I18nConfig::compile_in`](teksilo_i18n::I18nConfig::compile_in), so an
    /// application can compose its own catalogue with catalogues shipped by
    /// plugins, extensions or sibling crates. Assigning here instead would mean
    /// a contributor registering one tooltip silently deleted every tooltip the
    /// application had — the failure has no error and no warning, it just makes
    /// rich tooltips stop resolving their `[label](:key)` links.
    ///
    /// On a duplicate key the **first** registration wins; see
    /// [`install_tooltip_registry`](teksilo_widgets::tooltip::install_tooltip_registry).
    pub fn register_tooltips(
        mut self,
        contents: Vec<teksilo_widgets::tooltip::TooltipContent>,
    ) -> Self {
        self.tooltip_contents.extend(contents);
        self
    }

    /// Register a backend event source. Widgets can
    /// then call `BuildContext::subscribe_event(origin, callback)` from
    /// inside their `build()` method to receive events on the UI thread.
    ///
    /// Only one source per application is supported. Subsequent calls
    /// replace the previously registered source.
    pub fn event_source<S: EventSource>(mut self, source: S) -> Self {
        self.event_source = Some(EventSourceAdapter::new(source));
        self
    }

    /// Register an application-defined value of type `T` that any widget
    /// can retrieve via `BuildContext::app_state::<T>()`.
    ///
    /// Each type `T` may be registered at most once; a subsequent call
    /// with the same type replaces the previous value. To share multiple
    /// values of the same logical kind, wrap each in a distinct newtype.
    pub fn app_state<T: 'static>(mut self, value: T) -> Self {
        self.app_state_registry
            .insert(TypeId::of::<T>(), Box::new(value));
        self
    }

    /// Register an app-wide [`DefaultPostRoot`](crate::DefaultPostRoot) hook that wraps every
    /// window's root after its `root_builder` runs.
    ///
    /// Unlike `app_state(DefaultPostRoot::new(..))` — which stores a single
    /// type-keyed value and so silently replaces any previously-registered
    /// hook — this **composes**: each registered hook runs in call order,
    /// each wrapping the previous one's result. So an app that installs the
    /// debug inspector AND the toast host (or any other post-root chrome)
    /// gets both wrappers, not just whichever was installed last. The
    /// earlier-registered hook is the innermost wrapper (it sees the raw
    /// user root); the latest is outermost.
    ///
    /// All framework installers that splice window-level chrome
    /// (`install_inspector_in_debug`, `install_toast*`) route through this,
    /// so their order of installation no longer matters for correctness.
    pub fn register_post_root(mut self, hook: crate::DefaultPostRoot) -> Self {
        use crate::DefaultPostRoot;
        let key = TypeId::of::<DefaultPostRoot>();
        let composed = match self.app_state_registry.remove(&key) {
            Some(existing) => {
                let existing = *existing
                    .downcast::<DefaultPostRoot>()
                    .expect("DefaultPostRoot slot held a non-DefaultPostRoot value");
                let prev = existing.0;
                let next = hook.0;
                DefaultPostRoot(std::rc::Rc::new(move |tree, root_id| {
                    let inner = prev(tree, root_id);
                    next(tree, inner)
                }))
            }
            None => hook,
        };
        self.app_state_registry.insert(key, Box::new(composed));
        self
    }

    /// Register a composable observer that runs on every `AppEvent`,
    /// in addition to (never instead of) the single
    /// [`on_app_event`](Self::on_app_event) handler.
    ///
    /// Unlike `on_app_event` — which stores a single `Option<Box<dyn
    /// FnMut(&AppEvent)>>` and so silently replaces any previously
    /// registered handler — this **composes**: each registered observer
    /// runs, in call order, on every `AppEvent` delivered to the UI
    /// thread. So a framework extension that needs to react to
    /// `AppEvent`s (e.g. `teksilo::install_toast` turning
    /// `AppEvent::SettingsWriteFailed` into a toast) can register its
    /// own observer without clobbering the application's own
    /// `on_app_event` handler, or being clobbered by it, regardless of
    /// install order. Mirrors [`register_post_root`](Self::register_post_root)'s
    /// type-keyed `app_state` composition pattern exactly, but for
    /// event observation instead of post-root window chrome.
    ///
    /// See `TeksiloAppHandler::user_event` for the dispatch order: the
    /// `on_app_event` handler runs first, then every composed observer.
    pub fn register_app_event_observer(mut self, observer: impl Fn(&AppEvent) + 'static) -> Self {
        use crate::app_event_observers::AppEventObservers;
        let key = TypeId::of::<AppEventObservers>();
        let observer = AppEventObservers::new(observer);
        let composed = match self.app_state_registry.remove(&key) {
            Some(existing) => {
                let existing = *existing
                    .downcast::<AppEventObservers>()
                    .expect("AppEventObservers slot held a non-AppEventObservers value");
                let prev = existing.0;
                let next = observer.0;
                AppEventObservers(std::rc::Rc::new(move |event: &AppEvent| {
                    prev(event);
                    next(event);
                }))
            }
            None => observer,
        };
        self.app_state_registry.insert(key, Box::new(composed));
        self
    }

    /// Install the rfd-backed native file-dialog service. Registers a
    /// [`FileDialogHandle`](teksilo_platform::file_dialog::FileDialogHandle)
    /// wrapping an
    /// [`RfdAsyncBackend`](teksilo_platform::file_dialog::RfdAsyncBackend)
    /// into the app-state registry. Reachable from any handler via
    /// `ctx.app_state::<FileDialogHandle>()`, or — with
    /// `use teksilo_platform::file_dialog::EventContextFileDialogExt;` —
    /// directly via `ctx.pick_file(req, |result, ctx| ...)`.
    ///
    /// Apps that ship a custom or mock backend bypass this and call
    /// `.app_state(FileDialogHandle::new(my_backend))` directly.
    #[cfg(feature = "rfd-backend")]
    pub fn install_file_dialog(mut self) -> Self {
        use teksilo_platform::file_dialog::{FileDialogHandle, RfdAsyncBackend};
        let handle = FileDialogHandle::new(RfdAsyncBackend::new());
        self.app_state_registry
            .insert(TypeId::of::<FileDialogHandle>(), Box::new(handle));
        self
    }

    /// Install the external (OS) drag-and-drop service. Registers an
    /// [`ExternalDndHandle`](teksilo_platform::external_dnd::ExternalDndHandle)
    /// wrapping the platform's default backend
    /// ([`default_backend`](teksilo_platform::external_dnd::default_backend) —
    /// raw `NSDraggingDestination` on macOS, OLE on Windows, `wl_data_device`
    /// on Wayland, a no-op on X11) into the app-state registry.
    ///
    /// Once installed, every window is registered as an OS drop target on
    /// creation (and detached on close) by the window manager. Drops surface
    /// to widgets through the normal drag handlers (`on_drag_hover` /
    /// `on_drag_leave` / `on_drop`) with `payload.is_external()` true — the
    /// ready-made `DropZone` widget consumes them.
    ///
    /// Apps that ship a custom backend bypass this and call
    /// `.app_state(ExternalDndHandle::new(my_backend))` directly.
    pub fn install_external_dnd(mut self) -> Self {
        use teksilo_platform::external_dnd::{ExternalDndHandle, default_backend};
        let handle = ExternalDndHandle::new(default_backend());
        self.app_state_registry
            .insert(TypeId::of::<ExternalDndHandle>(), Box::new(handle));
        self
    }

    /// Install the native (OS) menu service. Registers a
    /// [`NativeMenuHandle`](teksilo_platform::native_menu::NativeMenuHandle)
    /// wrapping the platform's default backend (a real `NSMenu` on macOS, a
    /// no-op elsewhere) into the app-state registry.
    ///
    /// Once installed, a [`MenuBar`](teksilo_widgets::MenuBar) built with
    /// `from_model(..).native_on_macos(..)` mirrors its [`MenuModel`](teksilo_widgets::MenuModel) into the
    /// global menu bar on macOS, and item activations route back through the
    /// usual `Intent`/`Action` pipeline. The global menu follows window focus
    /// automatically (see the `WindowEvent::Focused` arm).
    ///
    /// Apps that ship a custom backend bypass this and call
    /// `.app_state(NativeMenuHandle::new(my_backend))` directly.
    pub fn install_native_menu(mut self) -> Self {
        use teksilo_platform::native_menu::{NativeMenuHandle, default_backend};
        let handle = NativeMenuHandle::new(default_backend());
        self.app_state_registry
            .insert(TypeId::of::<NativeMenuHandle>(), Box::new(handle));
        self
    }

    /// Register an `I18nConfig`. Constructs an
    /// `I18nManager` at startup, installs it on the thread-local, and
    /// seeds the widget tree with the resolved initial locale and layout
    /// direction. Without this call, `tr!`-expanded code falls back to
    /// returning the literal key as a placeholder.
    pub fn i18n(mut self, config: I18nConfig) -> Self {
        self.i18n = Some(config);
        self
    }

    /// Set a fixed theme (implies `ThemeMode::Manual`).
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self.theme_mode = ThemeMode::Manual;
        self
    }

    /// Set how the application resolves its theme.
    ///
    /// - `ThemeMode::Manual` — use the theme set via `.theme()` (default).
    /// - `ThemeMode::FollowSystem` — auto-switch between light/dark built-in themes.
    /// - `ThemeMode::Native` — read colors from OS desktop environment config.
    pub fn theme_mode(mut self, mode: ThemeMode) -> Self {
        self.theme_mode = mode;
        self
    }

    /// Choose how a drained pen batch reaches the tree.
    ///
    /// A digitizer outruns the window's message rate, so one poll routinely
    /// drains several packets.
    /// [`PerPacket`](teksilo_platform::PenBatching::PerPacket) — the default —
    /// spends a whole tree dispatch on each;
    /// [`Coalesce`](teksilo_platform::PenBatching::Coalesce) spends one per
    /// drain and hands the intermediate positions to handlers through
    /// [`EventContext::coalesced`](teksilo_core::EventContext::coalesced),
    /// each keeping its own time and axes.
    ///
    /// A drawing surface wants `Coalesce` and must read `coalesced` — see
    /// `docs/ink.md`. Anything that only watches moves should stay on the
    /// default, because `Coalesce` genuinely produces fewer `PointerMove`s.
    ///
    /// Applies to every window; set it before `run`.
    pub fn pen_batching(mut self, mode: teksilo_platform::PenBatching) -> Self {
        self.pen_batching = mode;
        self
    }

    #[cfg(feature = "text")]
    pub fn typesetter(mut self, typesetter: SharedTypesetter) -> Self {
        self.typesetter = Some(typesetter);
        self
    }

    /// Register additional fonts (e.g. a theme's font family) into the
    /// shared typesetter at startup, *before* any text is shaped — so a
    /// theme that sets `typography.body.family = "Roboto"` resolves
    /// correctly instead of silently falling back to the bundled Inter.
    ///
    /// A theme preset typically exposes a `FontRegistrar` the app passes
    /// here:
    /// ```ignore
    /// TeksiloAppBuilder::new()
    ///     .theme(material3::light())
    ///     .register_fonts(material3::font_registrar())
    ///     .run();
    /// ```
    #[cfg(feature = "text")]
    pub fn register_fonts(mut self, registrar: impl teksilo_text::FontRegistrar + 'static) -> Self {
        self.font_registrars.push(Box::new(registrar));
        self
    }

    /// Register a handler for `AppEvent`s received from background threads.
    pub fn on_app_event(mut self, handler: impl FnMut(&AppEvent) + 'static) -> Self {
        self.app_event_handler = Some(Box::new(handler));
        self
    }

    /// Register a router for [`AppEvent::External`] payloads that needs to
    /// **open, find or focus windows** — see [`ExternalCtxHandler`].
    ///
    /// [`on_app_event`](Self::on_app_event) is the hook for reacting to an event;
    /// this is the hook for *acting on the window set* because of one. The
    /// difference is not stylistic: `on_app_event` receives `&AppEvent` and
    /// nothing else, and `EventContext::open_window` panics on a standalone
    /// context, so there is no way to open a window from there at all.
    ///
    /// The handler runs against the focused window's tree (or the primary
    /// window's) with a real [`WindowOps`](teksilo_core::WindowOps) sink, and is
    /// consulted **only** for payloads that no framework router and no built-in
    /// downcast arm claimed — so it never has to defend against
    /// `CloseWindowRequest` and friends. Return `true` when the payload was
    /// yours.
    ///
    /// Unlike [`register_app_event_observer`](Self::register_app_event_observer),
    /// this is a single slot: calling it twice replaces the first router, the
    /// same way `on_app_event` does.
    ///
    /// ```ignore
    /// // A single-instance app: a second launch forwards its argv over a socket,
    /// // the listener posts it with `AppEventProxy::send_external`, and this
    /// // opens (or raises) the document window — the "document window" recipe in
    /// // docs/multi-window.md, driven from off the UI thread.
    /// .on_external_with_ctx(move |payload, ctx| {
    ///     let Some(req) = payload.downcast_ref::<OpenDocument>() else {
    ///         return false;
    ///     };
    ///     let wid = window_id_for(&req.path);
    ///     match ctx.find_window(&wid) {
    ///         Some(id) => ctx.focus_window(id),
    ///         None => { ctx.open_window(document_window_config(&req.path)); }
    ///     }
    ///     true
    /// })
    /// ```
    pub fn on_external_with_ctx(
        mut self,
        handler: impl FnMut(
            &(dyn std::any::Any + Send),
            &mut teksilo_core::widget::EventContext,
        ) -> bool
        + 'static,
    ) -> Self {
        self.external_ctx_handler = Some(Box::new(handler));
        self
    }

    /// Register a callback that receives an `AppEventProxy` once the event loop is ready.
    /// Use this to hand the proxy to background threads that need to post commands.
    /// May be called more than once; all registered callbacks fire in order
    /// (e.g. `install_async` registers one to wire the executor's waker).
    pub fn on_ready(mut self, handler: impl FnOnce(AppEventProxy) + 'static) -> Self {
        self.on_ready.push(Box::new(handler));
        self
    }

    /// Register a closure run once per event-loop turn (at the top of
    /// `about_to_wait`) plus a shared poll flag. Returning `true` from the
    /// closure means it advanced work that may have mutated UI state, which
    /// triggers a repaint of all windows. While `poll_source` is set the loop
    /// stays in [`ControlFlow::Poll`] so the closure keeps running; when it
    /// clears, the loop sleeps until the next event (off-thread wakes arrive
    /// via [`AppEventProxy`]).
    ///
    /// General-purpose and async-agnostic — `teksilo-app` only ever sees
    /// `FnMut`. The optional `teksilo-async` crate uses this to drive a
    /// main-thread executor; nothing in the core loop depends on a runtime.
    pub fn on_loop_tick(
        mut self,
        poll_source: std::rc::Rc<std::cell::Cell<bool>>,
        tick: impl FnMut() -> bool + 'static,
    ) -> Self {
        self.loop_tick = Some(Box::new(tick));
        self.loop_tick_poll = Some(poll_source);
        self
    }

    /// Configure the initial window. Required — every app must open at
    /// least one window at startup. The single canonical entry point:
    /// build a [`WindowConfig`] and pass it here.
    ///
    /// ```ignore
    /// TeksiloAppBuilder::new()
    ///     .theme(teksilo_core::presets::intui::light())
    ///     .initial_window(
    ///         WindowConfig::new()
    ///             .title("My App")
    ///             .size(800, 600)
    ///             .root(|tree, _state| tree.add(MyRoot::new())),
    ///     )
    ///     .run();
    /// ```
    pub fn initial_window(mut self, config: WindowConfig) -> Self {
        self.initial_window = Some(config);
        self
    }

    /// Open the configured settings bundle (if any) and register
    /// each service in the app-state registry.
    fn install_settings(&mut self) -> Option<teksilo_settings::OpenedSettings> {
        let bundle = self.settings_bundle.take()?;
        let paths = self.app_paths.clone().expect(
            "TeksiloAppBuilder::settings(...) requires .application(...) or .app_paths(...) \
             to be set first so persistence has a target directory.",
        );
        match bundle.open(&paths) {
            Ok(opened) => {
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_settings::SettingsStore>(),
                    Box::new(opened.store.clone()),
                );
                if let Some(w) = &opened.window_state {
                    self.app_state_registry.insert(
                        TypeId::of::<teksilo_settings::WindowStateService>(),
                        Box::new(w.clone()),
                    );
                }
                // Reachable from any handler via
                // `ctx.app_state::<teksilo_settings::SettingsRegistry>()`,
                // so application code can register its own ad hoc
                // `SettingsFile` / `PersistedListModel` / `MruList`
                // handles into the very same registry a `SettingsWatcher`
                // event gets dispatched through — not just the two
                // services the bundle itself opens.
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_settings::SettingsRegistry>(),
                    Box::new(opened.registry.clone()),
                );
                Some(opened)
            }
            Err(e) => {
                eprintln!("teksilo-app: failed to open settings bundle: {e}");
                None
            }
        }
    }

    /// Open the configured telemetry bundle (if any) and register the
    /// resulting handles into `app_state` so the dispatch tap and the
    /// `TelemetryExt` accessors can reach them. Must be called *after*
    /// `install_settings`, because `TelemetryBundle::open` reads the
    /// endpoint-override key from the live `SettingsStore`.
    ///
    /// # Panics
    ///
    /// Panics if `.telemetry(...)` was called without prior
    /// `.application(...)` / `.app_paths(...)`, or without a
    /// `.settings(...)` bundle. Both are hard requirements: the
    /// consent file needs an `AppPaths` target, and the runtime
    /// endpoint-override key lives in the `SettingsStore`.
    /// Fail-closed by design — a misconfigured app must not silently
    /// skip telemetry installation.
    #[cfg(feature = "telemetry")]
    fn install_telemetry(&mut self, settings: Option<&teksilo_settings::SettingsStore>) {
        let Some(bundle) = self.telemetry_bundle.take() else {
            return;
        };
        let paths = self.app_paths.clone().expect(
            "TeksiloAppBuilder::telemetry(...) requires .application(...) or .app_paths(...) \
             to be set first so the consent file has a target directory.",
        );
        let store = settings.expect(
            "TeksiloAppBuilder::telemetry(...) requires .settings(...) so the runtime \
             endpoint-override key can be read from the SettingsStore. \
             Add .settings(SettingsBundle::new()) before .telemetry(...).",
        );
        match bundle.open(&paths, store) {
            Ok(opened) => {
                // Register the OpenedTelemetry under its concrete type
                // so widgets can access it via TelemetryExt::telemetry().
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_telemetry::OpenedTelemetry>(),
                    Box::new(opened.clone()),
                );
                // Register the dispatch hook under the teksilo-core type.
                // The dispatch tap looks this up by TypeId.
                let session_id = generate_session_id();
                let tcx = teksilo_core::telemetry::TelemetryContext {
                    reporter: opened.reporter.clone()
                        as std::rc::Rc<dyn teksilo_core::telemetry::UsageReporter>,
                    session_id,
                    schema_version: opened.event_schema_version,
                };
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_core::telemetry::TelemetryContext>(),
                    Box::new(tcx),
                );
            }
            Err(e) => {
                eprintln!("teksilo-app: failed to open telemetry bundle: {e}");
            }
        }
    }

    /// Build a headless app for testing (no window, no GPU).
    pub fn build_headless(mut self) -> HeadlessApp {
        // Install the tooltip registry before anything else — widgets
        // that read from it during their first build (e.g. rich
        // tooltips looking up their :key) need it available.
        if !self.tooltip_contents.is_empty() {
            teksilo_widgets::tooltip::install_tooltip_registry(std::mem::take(
                &mut self.tooltip_contents,
            ));
        }

        // Open settings (if a bundle was configured) and register the
        // services into `app_state_registry` so they're reachable from
        // any handler via the SettingsExt trait.
        let opened_settings = self.install_settings();

        // Open telemetry (if a bundle was configured). Must come after
        // install_settings — TelemetryBundle reads the endpoint-override
        // key from the SettingsStore.
        #[cfg(feature = "telemetry")]
        self.install_telemetry(opened_settings.as_ref().map(|s| &s.store));

        let mut tree = WidgetTree::new().with_theme(self.theme.clone());

        #[cfg(feature = "text")]
        let typesetter = {
            let ts = self
                .typesetter
                .take()
                .unwrap_or_else(SharedTypesetter::new_with_default_font);
            // Install app/theme fonts before any text is shaped, so a
            // theme's `typography.*.family` resolves instead of falling
            // back to the bundled default.
            for registrar in &self.font_registrars {
                ts.apply_font_registrar(registrar.as_ref());
            }
            tree = tree.with_text_backend(ts.as_text_backend());
            // Auto-register so rich-text widgets can reach the shared
            // typesetter via `ctx.app_state::<SharedTypesetter>()` in
            // headless tests too.
            use std::any::TypeId;
            self.app_state_registry
                .insert(TypeId::of::<SharedTypesetter>(), Box::new(ts.clone()));
            ts
        };
        #[cfg(not(feature = "text"))]
        let _ = &mut self;

        // Install the i18n manager (if any) and seed the tree with the
        // resolved initial locale and layout direction. Must happen before
        // the root builder runs so that any `tr!` calls inside `build()`
        // resolve against the correct locale on first build.
        let i18n_manager = self.i18n.as_ref().map(|cfg| install_i18n(&mut tree, cfg));

        // Install the app-state registry (if any) before running the root
        // builder so that widgets' `build()` methods can call
        // `ctx.app_state::<T>()`.
        if !self.app_state_registry.is_empty() {
            let ctx = TreeAppContext::empty().with_app_state(self.app_state_registry);
            tree.set_app_context(std::rc::Rc::new(ctx));
        }
        #[cfg(feature = "text")]
        let _ = &typesetter;

        // Build the root from the `initial_window`'s builder if one was
        // provided. Headless apps without an `initial_window` run with an
        // empty tree — tests add widgets via `tree.add(...)` directly.
        if let Some(mut config) = self.initial_window.take()
            && let Some(root_builder) = config.take_root_builder()
        {
            // Headless has no real WindowState; construct a stub so
            // widgets that bind against their own window signals
            // still get a valid handle.
            let stub_state = teksilo_core::WindowState::new(teksilo_core::WindowStateInit {
                id: crate::TeksiloWindowId::new(0),
                string_id: config.string_id.clone(),
                placement: config.initial_placement,
                title: config.title.clone(),
                size: config.size,
                position: config.position.unwrap_or((0, 0)),
                focused: true,
                resizable: config.resizable,
                always_on_top: config.always_on_top,
            });
            tree.set_window_state(stub_state.clone());
            root_builder(&mut tree, stub_state);
        }

        HeadlessApp {
            tree,
            theme: self.theme,
            i18n_manager,
            settings: opened_settings,
        }
    }

    /// Build and run the application with windowed rendering.
    pub fn run(self) {
        self.run_with(
            || {
                winit::event_loop::EventLoop::<AppEvent>::with_user_event()
                    .build()
                    .expect("winit event loop creation failed")
            },
            |event_loop, app| match event_loop.run_app(app) {
                Ok(()) => {}
                Err(err) if display_connection_lost(&err) => {
                    // Not a fault of this application, and emphatically not a
                    // crash: the compositor exited or died underneath it.
                    // Panicking here produced a stack trace, a crash report
                    // and a bug filed against the renderer, for an event the
                    // application had no part in.
                    eprintln!("teksilo-app: the display server went away ({err}); shutting down");
                }
                Err(err) => panic!("winit event loop exited with error: {err:?}"),
            },
        );
    }

    /// The whole of [`Self::run`] except how the loop is built and how the
    /// finished handler is driven.
    ///
    /// Those two are parameters for exactly one reason: nothing else in the
    /// workspace can construct an `&ActiveEventLoop`, so nothing else can
    /// witness a single one of `TeksiloAppHandler`'s winit callbacks. A test
    /// supplies an off-main-thread loop (`with_any_thread`) and a
    /// `run_app_on_demand` driver that wraps the handler in a script; the
    /// production path supplies `EventLoop::build` and `run_app`, which never
    /// returns. See `crate::app::winit_loop_tests`.
    fn run_with(
        mut self,
        build_loop: impl FnOnce() -> winit::event_loop::EventLoop<AppEvent>,
        drive: impl FnOnce(winit::event_loop::EventLoop<AppEvent>, &mut TeksiloAppHandler),
    ) {
        // Install the tooltip registry before the window manager
        // starts building trees — rich tooltips read from it during
        // their first build.
        if !self.tooltip_contents.is_empty() {
            teksilo_widgets::tooltip::install_tooltip_registry(std::mem::take(
                &mut self.tooltip_contents,
            ));
        }

        // Open settings (if a bundle was configured) so the services
        // are present in the app_state registry when window trees
        // start being built. The `OpenedSettings` handle is kept on
        // the stack so its inner `SettingsFile` clones live long
        // enough to flush on shutdown.
        let opened_settings = self.install_settings();

        // Open telemetry (if a bundle was configured). Must come after
        // install_settings — TelemetryBundle reads the endpoint-override
        // key from the SettingsStore.
        #[cfg(feature = "telemetry")]
        self.install_telemetry(opened_settings.as_ref().map(|s| &s.store));

        // Construct the i18n manager (if configured) and install it on
        // the thread-local before any window or widget tree is created.
        // `WindowManager::create_window` seeds every new tree from the
        // thread-local, so each window inherits the manager's active
        // locale and layout direction on construction — no separate
        // post-create seeding step needed here.
        //
        // `runtime_override` entries are collected before the install
        // so the hot-reload watcher can be spun up after the winit
        // event loop exists (we need the `EventLoopProxy` as the sink
        // target) without a second borrow of `self.i18n`.
        let runtime_overrides: Vec<(LanguageIdentifier, std::path::PathBuf)> = self
            .i18n
            .as_ref()
            .map(|cfg| cfg.runtime_overrides().to_vec())
            .unwrap_or_default();

        if let Some(cfg) = self.i18n.as_ref() {
            install_i18n_manager(cfg);
        }

        let event_loop = build_loop();
        event_loop.set_control_flow(ControlFlow::Wait);

        // Always create a proxy: it's needed by both `on_ready` (if set)
        // and by the event-source poster (if a source is registered). The
        // proxy is cheap to clone.
        let proxy = AppEventProxy {
            inner: event_loop.create_proxy(),
        };

        // Register the process-wide sink for permanently-discarded
        // `teksilo-settings` writes (F3): a `DebouncedWriter` gave up
        // after `MAX_WRITE_ATTEMPTS` retries, or was dropped at teardown
        // with a write still failing. Previously this only reached an
        // `eprintln!` on the settings crate's own background I/O thread
        // and was otherwise invisible; this posts a typed `AppEvent`
        // through the event loop proxy so it reaches the UI thread like
        // every other backend->UI channel (see `user_event` above).
        let proxy_for_write_failure = proxy.inner.clone();
        teksilo_settings::set_write_failure_sink(std::sync::Arc::new(
            move |path, attempts, dropped_patches, message| {
                let _ = proxy_for_write_failure.send_event(AppEvent::SettingsWriteFailed {
                    path,
                    attempts,
                    dropped_patches,
                    message,
                });
            },
        ));

        // Build the i18n hot-reload watcher if any `runtime_override`s
        // were registered. The sink posts `AppEvent::I18nReload` through
        // the event loop proxy; the watcher's background thread converts
        // file-change events into these messages. The watcher handle is
        // handed to `TeksiloAppHandler` which keeps it alive for the loop
        // lifetime. Construction failures log and fall back to no
        // hot-reload (the rest of i18n still works).
        let i18n_watcher = if runtime_overrides.is_empty() {
            None
        } else {
            let proxy_for_sink = proxy.inner.clone();
            let sink: teksilo_i18n::ReloadSink = std::sync::Arc::new(move |locale, path| {
                let _ = proxy_for_sink.send_event(AppEvent::I18nReload {
                    locale: locale.to_string(),
                    path,
                });
            });
            match teksilo_i18n::FtlFileWatcher::new(runtime_overrides, sink) {
                Ok(watcher) => Some(watcher),
                Err(e) => {
                    eprintln!("teksilo-app: failed to start i18n file watcher: {e}");
                    None
                }
            }
        };

        // Build the live cross-process settings-reload watcher, mirroring
        // the i18n watcher immediately above: on by default whenever a
        // settings bundle was actually opened (`opened_settings.is_some()`),
        // opt-out via `.settings_watch(false)`. The sink posts
        // `AppEvent::SettingsReload` through the event loop proxy; the
        // handler (see `user_event` above) dispatches the changed path
        // through the app's `SettingsRegistry` (installed into `app_state`
        // by `install_settings`). Construction failures log and fall back
        // to no live reload — the rest of settings persistence still
        // works, peers just won't be noticed until this process happens
        // to touch the same key itself.
        let settings_watcher = if self.settings_watch_enabled && opened_settings.is_some() {
            self.app_paths.as_ref().and_then(|paths| {
                let proxy_for_sink = proxy.inner.clone();
                let sink: teksilo_settings::SettingsReloadSink = std::sync::Arc::new(move |path| {
                    let _ = proxy_for_sink.send_event(AppEvent::SettingsReload { path });
                });
                let dirs = vec![
                    paths.config_dir().to_path_buf(),
                    paths.data_dir().to_path_buf(),
                ];
                match teksilo_settings::SettingsWatcher::new(dirs, sink) {
                    Ok(watcher) => Some(watcher),
                    Err(e) => {
                        eprintln!("teksilo-app: failed to start settings file watcher: {e}");
                        None
                    }
                }
            })
        } else {
            None
        };

        // Build the typesetter first so we can auto-register it into
        // the per-tree app-state registry below. This gives rich-text
        // widgets (and anything else that needs direct typesetter
        // access) a reachable handle via `ctx.app_state::<SharedTypesetter>()`
        // without forcing the application author to wire it manually.
        #[cfg(feature = "text")]
        let typesetter = self
            .typesetter
            .unwrap_or_else(SharedTypesetter::new_with_default_font);

        #[cfg(feature = "text")]
        // Install app/theme fonts before any text is shaped.
        for registrar in &self.font_registrars {
            typesetter.apply_font_registrar(registrar.as_ref());
        }

        #[cfg(feature = "text")]
        {
            use std::any::TypeId;
            self.app_state_registry.insert(
                TypeId::of::<SharedTypesetter>(),
                Box::new(typesetter.clone()),
            );
        }

        // Auto-install a system clipboard handle so `RichTextEditor::editor`
        // (and any future clipboard-aware widget) can reach it via
        // `EventContext::app_state::<ClipboardHandle>()`. Behind the
        // `clipboard` feature because it pulls `arboard` into the build.
        // Falls back to `MemoryClipboard` if the OS backend fails to
        // initialize (headless CI, missing display, …) so the editor
        // still works in-process.
        #[cfg(feature = "clipboard")]
        {
            use std::any::TypeId;
            use teksilo_platform::clipboard::{ArboardClipboard, ClipboardHandle, MemoryClipboard};
            let handle = match ArboardClipboard::new() {
                Ok(backend) => ClipboardHandle::new(backend),
                Err(_) => ClipboardHandle::new(MemoryClipboard::new()),
            };
            self.app_state_registry
                .insert(TypeId::of::<ClipboardHandle>(), Box::new(handle));
        }

        // Always build the per-tree app context — the poster is cheap
        // and lets background-work integrations (file dialogs, future
        // async-result features) reach the event loop without forcing
        // an event-source registration. Apps without an event source,
        // app-state registry, or background-work feature simply pay an
        // unused Arc<AppEventPoster> per tree.
        let poster: std::sync::Arc<dyn AppEventPoster> = std::sync::Arc::new(proxy.clone());
        let base = match self.event_source {
            Some(adapter) => TreeAppContext::with_source_and_poster(adapter, poster.clone()),
            None => TreeAppContext::empty(),
        };
        let app_context_template = Some(std::rc::Rc::new(
            base.with_app_state(self.app_state_registry)
                .with_poster(poster),
        ));

        for on_ready in self.on_ready {
            on_ready(proxy.clone());
        }

        let initial_config = self
            .initial_window
            .expect("TeksiloAppBuilder::initial_window(WindowConfig) is required");

        let mut app = TeksiloAppHandler::new(
            self.theme,
            self.theme_mode,
            self.app_event_handler,
            initial_config,
            app_context_template,
            #[cfg(feature = "text")]
            typesetter,
            i18n_watcher,
            settings_watcher,
            proxy.clone(),
        );
        // Hand over the app's own ops-bearing external-event router, if any —
        // moved onto the handler after construction (like `loop_tick` below)
        // rather than threaded through `TeksiloAppHandler::new`'s already long
        // parameter list.
        app.wm.set_pen_batching(self.pen_batching);
        app.wm.set_records_announcements(self.records_announcements);
        app.external_ctx_handler = self.external_ctx_handler;
        // Hand over any registered loop-tick hook (e.g. the `teksilo-async`
        // executor poll). Async-agnostic: just a closure + a poll flag.
        app.loop_tick = self.loop_tick;
        app.loop_tick_poll = self.loop_tick_poll;

        drive(event_loop, &mut app);

        // Flush any pending settings writes synchronously before the
        // process exits. The `DebouncedWriter` background threads also
        // flush on Drop, but doing it synchronously here also surfaces
        // any I/O errors to stderr before the binding goes out of
        // scope.
        if let Some(opened) = opened_settings
            && let Err(e) = opened.flush_all()
        {
            eprintln!("teksilo-app: settings flush on exit failed: {e}");
        }
    }
}

// Linux-only: it reaches for winit's X11 extension traits (`with_x11`,
// `with_any_thread`), which exist on the free-unix backends and nowhere else.
// The Windows and macOS jobs therefore never build it, which is also why the
// claims below can only ever be judged against what a Linux host answers.
#[cfg(all(test, target_os = "linux"))]
mod winit_loop_tests;

impl Default for TeksiloAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an `I18nManager` from `cfg`, pre-resolve its initial locale,
/// and install it on the thread-local. Shared by `build_headless` and
/// `run` so both paths use identical setup. Returns the manager so the
/// headless caller can hand it to `HeadlessApp`; in the windowed `run`
/// path the thread-local owns it for the process lifetime.
fn install_i18n_manager(cfg: &I18nConfig) -> Rc<I18nManager> {
    let mgr = I18nManager::from_config(cfg);
    let initial_loc = I18nManager::resolve_initial_locale(cfg);
    mgr.set_locale(initial_loc);
    teksilo_i18n::thread_local::install(mgr.clone());
    mgr
}

/// Headless-only helper: install the i18n manager AND seed the single
/// `WidgetTree` with the resolved locale and direction. The windowed
/// path doesn't need this because `WindowManager::create_window` reads
/// the thread-local and seeds each new tree at construction time; the
/// headless path has no WindowManager so it seeds its one tree here.
fn install_i18n(tree: &mut WidgetTree, cfg: &I18nConfig) -> Rc<I18nManager> {
    let mgr = install_i18n_manager(cfg);
    tree.set_locale(mgr.locale_signal().get().to_string());
    tree.set_layout_direction(mgr.direction_signal().get());
    mgr
}

/// A headless app for testing (no window, no GPU).
pub struct HeadlessApp {
    pub tree: WidgetTree,
    pub theme: Theme,
    /// Active i18n manager, if `TeksiloAppBuilder::i18n(...)` was used. Tests
    /// can reach the bundles, version signal, and locale signal directly
    /// through this handle.
    pub i18n_manager: Option<Rc<I18nManager>>,
    /// Active persistence services, if `TeksiloAppBuilder::settings(...)`
    /// was used. Held here so the underlying `SettingsFile` clones
    /// (and their I/O threads) live as long as the headless app.
    pub settings: Option<teksilo_settings::OpenedSettings>,
}

impl HeadlessApp {
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The active i18n manager, if `i18n(...)` was registered on the
    /// builder.
    pub fn i18n_manager(&self) -> Option<&Rc<I18nManager>> {
        self.i18n_manager.as_ref()
    }

    /// Switch the active locale. Updates the manager (which increments the
    /// version signal so any `LocalizedString::to_signal()` observers
    /// re-resolve), then seeds the tree with the new direction (only when
    /// it actually changed) and triggers a composite rebuild via
    /// `WidgetTree::set_locale`. No-op if no `I18nConfig` was registered.
    pub fn set_locale(&mut self, locale: LanguageIdentifier) {
        let Some(mgr) = self.i18n_manager.clone() else {
            return;
        };
        let outcome = mgr.set_locale(locale.clone());
        if outcome.direction_changed {
            self.tree.set_layout_direction(mgr.direction_signal().get());
        }
        self.tree.set_locale(locale.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_i18n::lit;
    use teksilo_tokens::Color;
    use teksilo_widgets::{Button, ModalContainer};

    #[test]
    fn builder_accepts_theme() {
        let app = TeksiloAppBuilder::new()
            .theme(teksilo_core::presets::intui::light())
            .build_headless();
        assert_ne!(app.theme().colors.accent, Color::TRANSPARENT);
    }

    #[test]
    fn register_post_root_composes_instead_of_clobbering() {
        // Regression: installing two post-root chrome wrappers (e.g. the
        // debug inspector AND the toast host) must run BOTH, not just the
        // last-installed one — `app_state(DefaultPostRoot)` is type-keyed
        // and silently overwrote the earlier hook, killing F12 / overflow
        // stripes in any app that also installed toast.
        use crate::DefaultPostRoot;
        use std::cell::RefCell;
        use std::rc::Rc;

        let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let (o1, o2) = (order.clone(), order.clone());

        let builder = TeksiloAppBuilder::new()
            .register_post_root(DefaultPostRoot::new(move |_t, id| {
                o1.borrow_mut().push("inspector");
                id
            }))
            .register_post_root(DefaultPostRoot::new(move |_t, id| {
                o2.borrow_mut().push("toast");
                id
            }));

        let composed = builder
            .app_state_registry
            .get(&TypeId::of::<DefaultPostRoot>())
            .and_then(|b| b.downcast_ref::<DefaultPostRoot>())
            .expect("composed DefaultPostRoot must be present")
            .clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(Button::new(lit!("root")));
        let out = (composed.0)(&mut tree, root);

        assert_eq!(
            *order.borrow(),
            vec!["inspector", "toast"],
            "both hooks run, earliest-registered innermost (first)"
        );
        assert_eq!(out, root, "passthrough hooks return the same root id");
    }

    #[test]
    fn register_app_event_observer_composes_instead_of_clobbering() {
        // Mirrors `register_post_root_composes_instead_of_clobbering`
        // above: two extensions each registering their own `AppEvent`
        // observer (e.g. a future telemetry hook AND
        // `teksilo::install_toast`'s settings-write-failure toast) must
        // both fire, not just the last-installed one.
        use crate::app_event_observers::AppEventObservers;
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::app_event::AppEvent;

        let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let (o1, o2) = (order.clone(), order.clone());

        let builder = TeksiloAppBuilder::new()
            .register_app_event_observer(move |_event| {
                o1.borrow_mut().push("first");
            })
            .register_app_event_observer(move |_event| {
                o2.borrow_mut().push("second");
            });

        let composed = builder
            .app_state_registry
            .get(&TypeId::of::<AppEventObservers>())
            .and_then(|b| b.downcast_ref::<AppEventObservers>())
            .expect("composed AppEventObservers must be present")
            .clone();

        let event = AppEvent::BackgroundComplete {
            operation_id: "op".to_string(),
        };
        (composed.0)(&event);

        assert_eq!(
            *order.borrow(),
            vec!["first", "second"],
            "both observers run, in registration order"
        );
    }

    #[test]
    fn register_app_event_observer_does_not_suppress_on_app_event_handler() {
        // The composable observer slot and the single `on_app_event`
        // handler slot are independent storage (`app_state_registry` vs
        // `app_event_handler`), so registering one must never clear or
        // shadow the other. `TeksiloAppHandler::user_event` dispatches
        // both (handler first, then composed observers) — this test
        // proves the two slots coexist and mirrors that dispatch order
        // directly, since driving the real `ApplicationHandler::user_event`
        // requires a live winit event loop unavailable in a unit test.
        use crate::app_event_observers::AppEventObservers;
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::app_event::AppEvent;

        let handler_fired = Rc::new(RefCell::new(false));
        let observer_fired = Rc::new(RefCell::new(false));
        let (h1, h2) = (handler_fired.clone(), observer_fired.clone());

        let mut builder = TeksiloAppBuilder::new()
            .on_app_event(move |_event| {
                *h1.borrow_mut() = true;
            })
            .register_app_event_observer(move |_event| {
                *h2.borrow_mut() = true;
            });

        let mut handler = builder
            .app_event_handler
            .take()
            .expect("on_app_event handler must survive register_app_event_observer");
        let observers = builder
            .app_state_registry
            .get(&TypeId::of::<AppEventObservers>())
            .and_then(|b| b.downcast_ref::<AppEventObservers>())
            .expect("registered observer must survive on_app_event")
            .clone();

        let event = AppEvent::BackgroundComplete {
            operation_id: "op".to_string(),
        };
        // Mirrors the dispatch order in `user_event`: handler first, then
        // composed observers.
        handler(&event);
        (observers.0)(&event);

        assert!(
            *handler_fired.borrow(),
            "on_app_event's handler must still fire"
        );
        assert!(
            *observer_fired.borrow(),
            "the registered observer must also fire"
        );
    }

    /// `on_external_with_ctx` is a third, independent slot: registering it must
    /// not disturb `on_app_event`'s handler or the composable observers, and
    /// they must not disturb it. Same shape (and same limitation) as the test
    /// above — driving the real `ApplicationHandler::user_event` needs a live
    /// winit event loop, so this proves slot independence and the router's own
    /// claim contract; that it truly receives a window-capable `EventContext`
    /// is proven end-to-end by Skribisto's `scripts/automation_single_instance.py`.
    #[test]
    fn on_external_with_ctx_is_a_slot_of_its_own() {
        use crate::app_event_observers::AppEventObservers;

        let builder = TeksiloAppBuilder::new()
            .on_external_with_ctx(|_payload, _ctx| true)
            .on_app_event(|_event| {})
            .register_app_event_observer(|_event| {});

        assert!(
            builder.external_ctx_handler.is_some(),
            "the external router must survive a later on_app_event/observer registration"
        );
        assert!(
            builder.app_event_handler.is_some(),
            "on_app_event must survive on_external_with_ctx"
        );
        assert!(
            builder
                .app_state_registry
                .contains_key(&TypeId::of::<AppEventObservers>()),
            "observers must survive on_external_with_ctx"
        );

        // Single slot, like `on_app_event`: registering twice replaces.
        let builder = builder.on_external_with_ctx(|_payload, _ctx| false);
        let mut router = builder
            .external_ctx_handler
            .expect("the second registration is the live one");
        // Exercise the claim contract — the `bool` `user_event` branches on to
        // decide whether the payload was the app's — through a real, headless
        // `EventContext`. `NoopWindowOps` is the same sink `window_manager`'s
        // own close-guard tests use; the live `WindowOpsImpl` only arrives with
        // a winit event loop.
        let mut tree = teksilo_core::WidgetTree::new();
        let mut claimed = true;
        tree.run_with_event_context(
            &mut teksilo_core::NoopWindowOps,
            |ctx: &mut teksilo_core::widget::EventContext| {
                claimed = router(&42i32, ctx);
            },
        );
        assert!(
            !claimed,
            "the replacing router's answer is the one that decides"
        );
    }

    #[test]
    fn builder_with_root() {
        use teksilo_widgets::RectWidget;
        let app = TeksiloAppBuilder::new()
            .initial_window(
                WindowConfig::new()
                    .root(|tree, _state| tree.add(RectWidget::new().background(Color::RED))),
            )
            .build_headless();
        let mut tree = app.tree;
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn app_state_flows_through_headless_builder() {
        use std::rc::Rc;
        use teksilo_core::build_context::BuildContext;
        use teksilo_core::signal::Signal;
        use teksilo_core::widget::{LayoutContext, Widget};

        struct AppGlobals {
            label: Signal<String>,
        }

        #[derive(Debug)]
        struct GlobalsReader {
            observed: Signal<String>,
        }

        impl Widget for GlobalsReader {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let globals = ctx
                    .app_state::<Rc<AppGlobals>>()
                    .expect("AppGlobals not registered");
                self.observed.set(globals.label.get());
                Vec::new()
            }

            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> teksilo_core::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
        }

        let globals = Rc::new(AppGlobals {
            label: Signal::new("headless works".to_string()),
        });

        let observed = Signal::new(String::new());
        let observed_for_root = observed.clone();

        let _app = TeksiloAppBuilder::new()
            .app_state(globals.clone())
            .initial_window(WindowConfig::new().root(move |tree, _state| {
                tree.add(GlobalsReader {
                    observed: observed_for_root.clone(),
                })
            }))
            .build_headless();

        assert_eq!(observed.get(), "headless works");
    }

    #[test]
    fn auto_prefers_native_for_deferred_content_when_supported() {
        let request = ModalRequest::deferred(|tree| tree.add(Button::new(lit!("Deferred"))));

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::NativeWindow
        );
    }

    #[test]
    fn a_native_modal_honours_the_escape_half_of_its_close_behavior() {
        // The whole point of finding 1: `close_behavior` used to fall into the
        // `ModalRequest` destructure's `..` in the NativeWindow arm and was
        // never read, so the DEFAULT behaviour on the DEFAULT presentation did
        // nothing. This is the table that arm now consults.
        assert!(native_modal_escape_dismisses(
            ModalCloseBehavior::EscapeOrClickOutside
        ));
        assert!(native_modal_escape_dismisses(ModalCloseBehavior::EscapeKey));
        // No "outside" exists while the OS blocks the parent, so these two
        // agree — deliberately, and not by omission.
        assert!(!native_modal_escape_dismisses(
            ModalCloseBehavior::ClickOutside
        ));
        assert!(!native_modal_escape_dismisses(ModalCloseBehavior::Manual));
    }

    #[test]
    fn the_native_modal_escape_wrapper_asks_to_dismiss_the_window() {
        // The wrapper the NativeWindow arm splices around the modal's root:
        // an unclaimed Escape reaching it must request the window's dismissal.
        let mut tree = WidgetTree::new();
        let content = tree.add(Button::new(lit!("Inside")));
        let _root = wrap_native_modal_in_escape_route(&mut tree, content);
        tree.layout(teksilo_canvas::SizeProposal::exact(300.0, 200.0));
        tree.focus(content);
        assert!(!tree.has_pending_modal_dismissal());

        tree.press_key(
            teksilo_core::event::Key::Escape,
            teksilo_core::event::Modifiers::NONE,
        );

        assert!(
            tree.has_pending_modal_dismissal(),
            "an unclaimed Escape inside a native modal must ask to close its window"
        );
    }

    #[test]
    fn existing_widget_forces_in_tree_even_if_native_requested() {
        let mut tree = WidgetTree::new();
        let content = tree.add(Button::new(lit!("Existing")));
        let request = ModalRequest::in_tree(content).presentation(ModalPresentation::NativeWindow);

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::InTree
        );
    }

    #[test]
    fn present_in_tree_modal_request_shows_centered_overlay() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Two overlays: the modal-panel overlay AND the dialog scrim
        // pushed below it by the modal-presentation pipeline.
        assert_eq!(tree.active_overlays().len(), 2);
        assert!(tree.find_by_label("Modal content").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_builds_deferred_content() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| tree.add(Button::new(lit!("Deferred modal"))))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Two overlays: the modal-panel overlay AND the dialog scrim
        // pushed below it by the modal-presentation pipeline.
        assert_eq!(tree.active_overlays().len(), 2);
        assert!(tree.find_by_label("Deferred modal").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_mounts_scrim_below_modal() {
        // The scrim must be pushed BEFORE the modal so it z-orders
        // below the panel. `active_content_ids()` returns ids in
        // stack order (oldest → newest), so the first id is the
        // scrim and the second is the modal content.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );

        let stack = tree.overlay_manager().active_content_ids();
        assert_eq!(stack.len(), 2, "scrim + modal");
        // Scrim is the first one; modal content the second.
        assert_eq!(stack[1], content, "modal content sits above scrim");
    }

    // ─── A dismissal must report its result ────────────────────────
    //
    // These drive a modal through the REAL in-tree path —
    // `ctx.present_modal` → `drain_pending_modal_requests` →
    // `present_in_tree_modal_request` — because that is the path that is
    // broken and the one nothing covered.
    //
    // Every existing test of this behaviour builds the modal's content
    // straight into the tree and never calls `show_overlay`
    // (`message_box.rs`'s `present_and_lay_out`, and the `InputDialog`
    // tests). With no overlay on the stack, `pointer_router`'s Escape arm
    // is skipped and the widget's own Escape shortcut resolves — which is
    // the NATIVE topology. So the suite has been testing the arm this
    // machine cannot execute, and is green while a Linux user loses their
    // answer: `supports_native_modal_windows()` is false on every unix but
    // macOS, so `Auto` resolves here to the overlay these tests omit.
    //
    // Measured against the live `dialogs-and-popovers` demo: clicking a
    // button reports, Escape does not, clicking outside does not.

    /// Present `mb` the way an app does, dismiss it with `dismiss`, and
    /// return whatever `on_result` was handed — `None` if it never ran.
    fn message_box_dismissed_by(
        mb: teksilo_widgets::MessageBox,
        dismiss: impl FnOnce(&mut WidgetTree),
    ) -> Option<teksilo_widgets::MessageBoxResult> {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Option<teksilo_widgets::MessageBoxResult>>> =
            Rc::new(RefCell::new(None));
        let sink = seen.clone();
        let pending = Rc::new(RefCell::new(Some(mb.on_result(move |r, _ctx| {
            *sink.borrow_mut() = Some(r);
        }))));

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let trigger = tree.add(Button::new(lit!("Open")).on_activate_fn(move |ctx| {
            if let Some(mb) = pending.borrow_mut().take() {
                mb.present(ctx);
            }
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        let queued = tree
            .drain_pending_modal_requests()
            .pop()
            .expect("MessageBox::present must queue a modal request");
        present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            2,
            "precondition: the in-tree arm ran (scrim + panel)"
        );

        dismiss(&mut tree);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            0,
            "precondition: the modal actually closed — otherwise a missing result \
             would mean 'still open', which is a different bug"
        );
        *seen.borrow()
    }

    /// A point well outside the centred 460×140 panel in an 800×600
    /// viewport, so a press there lands on the scrim.
    fn click_outside(tree: &mut WidgetTree) {
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        let at = teksilo_canvas::Point::new(8.0, 8.0);
        tree.pointer_move(at);
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::default(),
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::default(),
        ));
    }

    fn a_question() -> teksilo_widgets::MessageBox {
        use teksilo_widgets::{MessageBox, MessageBoxButtons, StandardButton};
        MessageBox::question(lit!("Save changes?"))
            .buttons(MessageBoxButtons::SaveDiscardCancel)
            .escape_button(StandardButton::Cancel)
    }

    /// The positive control, and the reason the three failures below can be
    /// read as a framework defect rather than a broken rig: identical
    /// harness, identical presentation, identical sink — the only thing
    /// that differs is the route out. This one passes today.
    #[test]
    fn clicking_a_button_on_an_in_tree_message_box_reports_its_result() {
        let seen = message_box_dismissed_by(a_question(), |tree| {
            let cancel = tree
                .find_by_label("Cancel")
                .expect("the Cancel button is in the modal's content");
            tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
                action: teksilo_core::accesskit::Action::Click,
                target: Some(cancel),
                target_node: teksilo_core::accessibility::root_node_id(),
                data: None,
            });
        })
        .expect("clicking a button reports its result");

        assert_eq!(seen.button, teksilo_widgets::StandardButton::Cancel);
        assert_eq!(
            seen.dismissal,
            teksilo_widgets::MessageBoxDismissal::Button,
            "a deliberate button press is a choice, not a dismissal"
        );
    }

    #[test]
    fn escape_on_an_in_tree_message_box_reports_its_result() {
        let seen = message_box_dismissed_by(a_question(), |tree| {
            tree.press_key(
                teksilo_core::event::Key::Escape,
                teksilo_core::event::Modifiers::NONE,
            );
        });

        let seen = seen.expect(
            "Escape closed the modal but `on_result` never ran, so the app cannot \
             learn the user's answer. `pointer_router`'s overlay Escape arm consumes \
             the key and returns before shortcut resolution, so MessageBox's own \
             Escape shortcut never fires on the in-tree arm",
        );
        assert_eq!(
            seen.button,
            teksilo_widgets::StandardButton::Cancel,
            "Escape must resolve to the declared escape button"
        );
        assert_eq!(
            seen.dismissal,
            teksilo_widgets::MessageBoxDismissal::Escape,
            "and must name the route, not merely report that something happened"
        );
    }

    #[test]
    fn clicking_outside_an_in_tree_message_box_reports_its_result() {
        let seen = message_box_dismissed_by(a_question(), click_outside);

        let seen = seen.expect(
            "a click outside closed the modal but `on_result` never ran. \
             `MessageBoxResult::dismissed_by_escape` is documented as covering \
             scrim-click, and no code path can currently produce it",
        );
        assert_eq!(seen.button, teksilo_widgets::StandardButton::Cancel);
        assert_eq!(
            seen.dismissal,
            teksilo_widgets::MessageBoxDismissal::ClickOutside,
            "a press outside is not Escape, and the caller can now tell"
        );
    }

    /// `InputDialog` is the worse case: `None` *is* its cancellation
    /// payload, so a dismissal that reports nothing is indistinguishable
    /// from a dialog still sitting open — and unlike `MessageBox` it
    /// registers no Escape route of its own, so this fails on the native
    /// arm too.
    #[test]
    fn escape_on_an_in_tree_input_dialog_reports_its_cancellation() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
        let sink = seen.clone();
        let pending = Rc::new(RefCell::new(Some(
            teksilo_widgets::InputDialog::new(lit!("Rename")).on_result(move |v, _ctx| {
                *sink.borrow_mut() = Some(v);
            }),
        )));

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let trigger = tree.add(Button::new(lit!("Open")).on_activate_fn(move |ctx| {
            if let Some(d) = pending.borrow_mut().take() {
                d.present(ctx);
            }
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        let queued = tree
            .drain_pending_modal_requests()
            .pop()
            .expect("InputDialog::present must queue a modal request");
        present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.press_key(
            teksilo_core::event::Key::Escape,
            teksilo_core::event::Modifiers::NONE,
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let reported = seen.borrow().clone().expect(
            "Escape closed the InputDialog but `on_result` never ran. Its own rustdoc \
             says it is 'invoked exactly once when the user accepts (Some) or cancels \
             (None)' — a dismissal is a cancellation, and the caller is left unable to \
             tell it from a dialog that is still open",
        );
        assert!(
            reported.is_none(),
            "a dismissal is a cancellation, so the payload must be None"
        );
    }

    #[test]
    fn dismissing_modal_cascades_to_scrim() {
        // The scrim's `parent_overlay` is patched to the modal id
        // after both are pushed. Dismissing the modal must therefore
        // also dismiss the scrim through the cascade walk in
        // `dismiss_immediate`.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        assert_eq!(tree.active_overlays().len(), 2);

        // Find the modal's overlay id (the one whose content is the
        // modal content widget) and dismiss it.
        let modal_overlay = tree
            .overlay_manager()
            .find_by_content(content)
            .expect("modal overlay registered");
        tree.overlay_manager_mut().dismiss(modal_overlay);

        assert!(
            tree.active_overlays().is_empty(),
            "scrim must cascade away with the modal",
        );
    }

    #[test]
    fn scrim_uses_full_viewport_placement() {
        // The scrim's overlay placement determines its bounds during
        // `position_overlays`. It must be `FullViewport` so the dim
        // covers the entire window regardless of the modal's size or
        // position.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // The scrim is at the bottom of the stack — first content id.
        let scrim_content_id = tree.overlay_manager().active_content_ids()[0];
        let scrim_bounds = tree.bounds(scrim_content_id);
        assert!(
            (scrim_bounds.width - 800.0).abs() < 0.01,
            "scrim spans the viewport width",
        );
        assert!(
            (scrim_bounds.height - 600.0).abs() < 0.01,
            "scrim spans the viewport height",
        );
    }

    #[test]
    fn present_in_tree_modal_request_moves_focus_into_modal() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        let continue_button = tree.find_by_label("Continue").unwrap();
        assert_eq!(tree.focused(), Some(continue_button));
    }

    /// **A modal whose content is a text editor opens with the caret in it.**
    ///
    /// The editors are the one focusable widget family that carries no label,
    /// so `first_focusable_descendant` is the only thing that can find them —
    /// and a modal that fails to focus one opens with no caret at all, which
    /// reads as a broken surface rather than an unfocused one.
    #[test]
    fn present_in_tree_modal_focuses_a_rich_text_editor() {
        use teksilo_widgets::rich_text::RichTextEditor;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(ModalContainer::new(RichTextEditor::editor(doc)))
            })
            .presentation(ModalPresentation::InTree),
        );

        let focused = tree.focused().expect("the modal moved focus into itself");
        assert_ne!(
            focused, source,
            "focus must leave the trigger and land inside the modal"
        );
        let name = tree.widget_type_name(focused).unwrap_or("<none>");
        assert!(
            name.contains("RichTextEditor"),
            "focus landed on {name}, not the editor — the modal opens caretless"
        );
    }

    /// **A modal that opens over a text editor shows its caret.**
    ///
    /// Focus landing on the editor is not enough: the caret is gated on the
    /// editor's *own* `has_focus`, and `present_in_tree_modal_request` parks
    /// the content dormant and re-activates it in the same batch, before
    /// moving focus in. Those two activation edges used to be replayed in
    /// order *after* the focus dispatch, so the superseded `false` arrived
    /// last and the editor's dormancy handler wiped the focus it had just
    /// been granted — the dialog opened with the text visible and no caret,
    /// which reads as a dead surface rather than an unfocused one.
    ///
    /// Asserted on the painted frame rather than on any internal flag,
    /// because the caret is the whole point: a thin, full-line-height rect
    /// in the theme's `editor_caret` colour, at the editor's origin.
    #[test]
    fn present_in_tree_modal_paints_the_editor_caret() {
        use teksilo_widgets::rich_text::RichTextEditor;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(ModalContainer::new(RichTextEditor::editor(doc)))
            })
            .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let editor = tree.focused().expect("the modal moved focus into itself");
        let editor_bounds = tree.bounds(editor);
        let frame = tree.render();

        // The caret is emitted through `Canvas::fill_rect`, which lands in the
        // frame as a `WidgetBackground` decoration — so identify it by shape
        // and colour rather than by kind.
        let caret_color = teksilo_core::presets::intui::light()
            .colors
            .editor_caret
            .to_array();
        let caret = frame.decorations.iter().find(|d| {
            d.color == caret_color && d.rect[2] > 0.0 && d.rect[2] <= 4.0 && d.rect[3] > 4.0
        });
        let caret = caret.unwrap_or_else(|| {
            panic!(
                "the modal painted no caret — {} glyphs and {} decorations, none caret-shaped: {:?}",
                frame.glyphs.len(),
                frame.decorations.len(),
                frame.decorations,
            )
        });

        // ...and it sits inside the editor, not stranded at the viewport origin.
        assert!(
            caret.rect[0] >= editor_bounds.x
                && caret.rect[0] <= editor_bounds.x + editor_bounds.width
                && caret.rect[1] >= editor_bounds.y
                && caret.rect[1] <= editor_bounds.y + editor_bounds.height,
            "caret at {:?} must fall inside the editor's bounds {editor_bounds:?}",
            caret.rect,
        );
    }

    /// Same, but with the editor buried under the chrome a real dialog wraps it
    /// in — a titled panel, a column, a fixed-size box, padding. The walk has to
    /// reach through all of it.
    #[test]
    fn present_in_tree_modal_focuses_an_editor_under_chrome() {
        use teksilo_widgets::rich_text::RichTextEditor;
        use teksilo_widgets::{Divider, FixedSize, Padding, Panel, TextWidget, VStack};
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(900.0, 700.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(ModalContainer::new(
                    Panel::new().corner_radius(10.0).padding(0.0).child(
                        VStack::new()
                            .spacing(0.0)
                            .child(
                                Padding::symmetric(8.0, 14.0)
                                    .child(TextWidget::new(lit!("Synopsis"))),
                            )
                            .child(Divider::new())
                            .child(
                                FixedSize::new().width(600.0).height(400.0).child(
                                    Padding::uniform(16.0).child(RichTextEditor::editor(doc)),
                                ),
                            ),
                    ),
                ))
            })
            .presentation(ModalPresentation::InTree),
        );

        let focused = tree.focused().expect("the modal moved focus into itself");
        let name = tree.widget_type_name(focused).unwrap_or("<none>");
        assert!(
            name.contains("RichTextEditor"),
            "focus landed on {name}, not the editor — chrome between the modal root \
             and the editor is hiding it from the focus walk"
        );
    }

    /// And with **no `ModalContainer`** — the shape an app takes when its dialog
    /// owns its own chrome (Skribisto's synopsis / picker panels do). The focus
    /// walk starts at whatever the deferred builder returned.
    #[test]
    fn present_in_tree_modal_focuses_an_editor_without_a_modal_container() {
        use teksilo_widgets::rich_text::RichTextEditor;
        use teksilo_widgets::{FixedSize, Padding, Panel, VStack};
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(900.0, 700.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(
                    Panel::new().corner_radius(10.0).padding(0.0).child(
                        VStack::new().spacing(0.0).child(
                            FixedSize::new()
                                .width(600.0)
                                .height(400.0)
                                .child(Padding::uniform(16.0).child(RichTextEditor::editor(doc))),
                        ),
                    ),
                )
            })
            .presentation(ModalPresentation::InTree),
        );

        let focused = tree.focused().expect("the modal moved focus into itself");
        let name = tree.widget_type_name(focused).unwrap_or("<none>");
        assert!(
            name.contains("RichTextEditor"),
            "focus landed on {name}, not the editor"
        );
    }

    #[test]
    fn present_in_tree_modal_restores_focus_to_trigger_on_dismiss() {
        // Regression: tabbing to a trigger, opening a modal, then
        // dismissing it must return keyboard focus to the trigger. The
        // modal overlay carries the pre-modal focus owner as its
        // `focus_restore`, which every dismiss path replays.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Rename")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);
        assert_eq!(
            tree.focused(),
            Some(source),
            "precondition: trigger focused"
        );

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        // Focus moved into the modal (existing behavior).
        let continue_button = tree.find_by_label("Continue").unwrap();
        assert_eq!(tree.focused(), Some(continue_button));

        // The modal overlay is the topmost; dismissing it must surface
        // the trigger as the focus_restore target.
        let modal_overlay = *tree
            .active_overlays()
            .last()
            .expect("modal overlay registered");
        let (_dismissed, focus_restore) = tree
            .overlay_manager_mut()
            .dismiss_with_focus_restore(modal_overlay);
        assert_eq!(
            focus_restore,
            Some(source),
            "dismissing the modal must restore focus to the trigger that opened it",
        );
    }

    #[test]
    fn mouse_opened_modal_restores_pointer_modality_on_dismiss() {
        // Regression: a modal opened by mouse (focus_visible = false) must
        // not leave the trigger sporting a keyboard `:focus-visible` ring
        // after the user types / presses Enter inside the dialog — which
        // flips the global modality to keyboard. The pre-modal modality is
        // captured and replayed when the overlay dismisses.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Rename")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Mouse-style entry: pointer modality, trigger focused.
        let focus_visible = tree.focus_visible_signal();
        focus_visible.set(false);
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        // Keyboard input *inside* the dialog (typing the name, Enter to
        // accept) flips the global modality to keyboard.
        focus_visible.set(true);

        // Dismiss fires the overlay's on_dismiss, which restores modality.
        let modal_overlay = *tree
            .active_overlays()
            .last()
            .expect("modal overlay registered");
        // Through the tree, not `overlay_manager_mut()`: the manager parks a
        // dismissal callback rather than running it (it takes an
        // `EventContext` and the manager has no tree), and `dismiss_overlay`
        // is the door that drains it.
        tree.dismiss_overlay(modal_overlay);

        assert!(
            !focus_visible.get(),
            "a mouse-opened modal must restore pointer modality on dismiss, \
             not leave a keyboard focus ring on the trigger",
        );
    }

    #[test]
    fn keyboard_opened_modal_keeps_focus_visible_on_dismiss() {
        // Invariant guard for the fix above: a modal opened while in
        // keyboard modality must KEEP the focus ring on the trigger when it
        // closes — restoring the captured modality must not blanket-clear it.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Rename")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Keyboard-style entry: keyboard modality, trigger focused.
        let focus_visible = tree.focus_visible_signal();
        focus_visible.set(true);
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        // Even if a pointer event flipped modality off inside the dialog,
        // dismiss restores the captured (keyboard) modality.
        focus_visible.set(false);

        let modal_overlay = *tree
            .active_overlays()
            .last()
            .expect("modal overlay registered");
        // Through the tree, not `overlay_manager_mut()`: the manager parks a
        // dismissal callback rather than running it (it takes an
        // `EventContext` and the manager has no tree), and `dismiss_overlay`
        // is the door that drains it.
        tree.dismiss_overlay(modal_overlay);

        assert!(
            focus_visible.get(),
            "a keyboard-opened modal must restore keyboard modality on dismiss",
        );
    }

    /// Test content widget: a focusable container with two focusable
    /// button descendants. `hint` controls which (if any) the widget
    /// reports as its `initial_focus_hint`.
    #[derive(Debug)]
    struct TwoButtonContent {
        root: Option<WidgetId>,
        second: Option<WidgetId>,
        hint_to_second: bool,
    }

    impl teksilo_core::Widget for TwoButtonContent {
        fn build(&mut self, ctx: &mut teksilo_core::BuildContext) -> Vec<WidgetId> {
            let first = ctx.add(Button::new(lit!("First")));
            let second = ctx.add(Button::new(lit!("Second")));
            let row = ctx.add(teksilo_widgets::HStack::new().child(first).child(second));
            self.root = Some(row);
            self.second = Some(second);
            vec![row]
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            ctx: &teksilo_core::LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            self.root
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into()
        }

        fn initial_focus_hint(&self) -> Option<WidgetId> {
            if self.hint_to_second {
                self.second
            } else {
                None
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.root.into_iter().collect()
        }
    }

    #[test]
    fn present_in_tree_modal_consults_initial_focus_hint() {
        // When `focus_target` is None, the framework must consult the
        // content widget's `initial_focus_hint` before falling back to
        // `first_focusable_descendant`.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(TwoButtonContent {
                    root: None,
                    second: None,
                    hint_to_second: true,
                })
            })
            .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Two "Second" labels may exist globally (source isn't one), so
        // find_by_label is unambiguous here.
        let second = tree.find_by_label("Second").unwrap();
        assert_eq!(
            tree.focused(),
            Some(second),
            "initial_focus_hint must redirect focus away from first focusable",
        );
    }

    #[test]
    fn present_in_tree_modal_falls_back_to_first_focusable_without_hint() {
        // Baseline: content without an initial_focus_hint gets the first
        // focusable descendant, matching prior behavior.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(TwoButtonContent {
                    root: None,
                    second: None,
                    hint_to_second: false,
                })
            })
            .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let first = tree.find_by_label("First").unwrap();
        assert_eq!(
            tree.focused(),
            Some(first),
            "without focus_target or initial_focus_hint, first focusable wins",
        );
    }

    #[test]
    fn present_in_tree_modal_rejects_focus_target_outside_content_subtree() {
        // A focus_target pointing at a widget that exists but is NOT a
        // descendant of content_id must be rejected. The framework falls
        // back to initial_focus_hint → first_focusable_descendant.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(TwoButtonContent {
                    root: None,
                    second: None,
                    hint_to_second: false,
                })
            })
            .presentation(ModalPresentation::InTree)
            .focus_target(source), // active but outside modal subtree
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let first = tree.find_by_label("First").unwrap();
        assert_eq!(
            tree.focused(),
            Some(first),
            "focus_target outside content subtree must be rejected",
        );
    }
}

#[cfg(test)]
mod occluded_band_tests {
    use super::occluded_band_in_client;

    /// A keyboard across the bottom of the screen, under a window that starts
    /// 100 physical pixels down. The band reported to the tree is in the
    /// window's own logical coordinates, clipped to its client area.
    #[test]
    fn a_keyboard_band_is_clipped_and_delogicalised() {
        let band = occluded_band_in_client(
            (0, 100),
            (800, 600),
            2.0,
            // Screen-space: the bottom 400 physical pixels, wider than the
            // window on both sides.
            (-50, 400, 1000, 900),
        )
        .expect("the band covers the window");
        // x clips to 0..800 physical → 0..400 logical; y is 300..600 physical
        // (400-100 .. clipped at 600) → 150..300 logical.
        assert_eq!(band.x, 0.0);
        assert_eq!(band.width, 400.0);
        assert_eq!(band.y, 150.0);
        assert_eq!(band.height, 150.0);
    }

    #[test]
    fn a_keyboard_that_misses_the_window_reports_nothing() {
        // Below a window that ends at y = 100 + 600.
        assert!(occluded_band_in_client((0, 100), (800, 600), 1.0, (0, 800, 800, 1000)).is_none());
        // Beside it.
        assert!(occluded_band_in_client((0, 0), (800, 600), 1.0, (900, 0, 1200, 600)).is_none());
    }

    #[test]
    fn a_degenerate_scale_reports_nothing_rather_than_dividing_by_it() {
        assert!(occluded_band_in_client((0, 0), (800, 600), 0.0, (0, 0, 800, 600)).is_none());
    }
}

/// Whether a winit event-loop failure means the connection to the display
/// server was lost.
///
/// winit has no variant that says so. Both Linux backends turn a failed
/// dispatch or flush into `ExitFailure(errno)` and unwind, deliberately:
/// "crashing downstream is not really an option", as their own comment puts
/// it. Nothing in teksilo ever asks the loop to exit with a non-zero code
/// (`ActiveEventLoop::exit` sets zero), so a non-zero `ExitFailure` is that
/// and nothing else.
///
/// The remaining variants stay fatal. `NotSupported`, `Os` and
/// `RecreationAttempt` are genuine faults at startup, where a panic and its
/// backtrace are the useful answer.
fn display_connection_lost(err: &winit::error::EventLoopError) -> bool {
    matches!(err, winit::error::EventLoopError::ExitFailure(code) if *code != 0)
}

#[cfg(test)]
mod display_connection_tests {
    use super::display_connection_lost;
    use winit::error::EventLoopError;

    /// The defect this pins: a compositor that exits under a running
    /// application took the process down through a panic and a crash report,
    /// when the only honest reading is that the session ended.
    #[test]
    fn a_failed_dispatch_reads_as_a_lost_display_server() {
        assert!(display_connection_lost(&EventLoopError::ExitFailure(1)));
        assert!(display_connection_lost(&EventLoopError::ExitFailure(32)));
    }

    /// A clean `exit()` carries code zero, and must not be mistaken for the
    /// compositor leaving.
    #[test]
    fn a_clean_exit_is_not_a_lost_display_server() {
        assert!(!display_connection_lost(&EventLoopError::ExitFailure(0)));
    }

    /// Everything else is a real fault and stays fatal, so that narrowing the
    /// panic does not quietly swallow the failures it was there for.
    #[test]
    fn a_loop_that_could_not_be_built_stays_fatal() {
        assert!(!display_connection_lost(&EventLoopError::RecreationAttempt));
    }
}
