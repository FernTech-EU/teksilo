use bastyde_canvas::SizeProposal;
use bastyde_core::Theme;
use bastyde_core::app_event::AppEvent;
use bastyde_core::event::WidgetEvent;
use bastyde_core::event_source::{
    AppEventPoster, EventSource, EventSourceAdapter, SubscriptionId, TreeAppContext,
};
use bastyde_core::modal::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
use bastyde_core::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::{WidgetId, WidgetTree};
use bastyde_i18n::{I18nConfig, I18nManager, LanguageIdentifier};
use bastyde_platform::event_translation;
use bastyde_tokens::ColorTokens;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
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
    /// Follow the OS light/dark preference using Bastyde's built-in themes.
    FollowSystem,
    /// Adopt colors read directly from the OS/DE config files (GNOME/KDE/Cinnamon).
    /// Falls back to `FollowSystem` on unsupported platforms or DEs.
    Native,
}

#[cfg(feature = "text")]
use bastyde_text::SharedTypesetter;

use crate::window_config::{BastydeWindowId, WindowConfig};
use crate::window_manager::WindowManager;
use bastyde_core::WindowPlacement;

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

fn present_in_tree_modal_request(
    tree: &mut WidgetTree,
    source_widget: WidgetId,
    request: ModalRequest,
) {
    let dismiss = modal_close_behavior_to_overlay_dismiss(request.close_behavior);
    let requested_focus = request.focus_target;
    let on_dismiss = request.on_dismiss;
    let close_behavior = request.close_behavior;
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
    let dismiss_target: std::rc::Rc<std::cell::Cell<Option<bastyde_core::overlay::OverlayId>>> =
        std::rc::Rc::new(std::cell::Cell::new(None));
    let scrim_id = tree.add(
        bastyde_widgets::ModalScrim::new()
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
    let modal_overlay = tree.show_overlay_from_source(
        source_widget,
        OverlayRequest {
            content_id,
            anchor: source_widget,
            placement: OverlayPlacement::Centered,
            dismiss,
            layer: OverlayLayer::InTree,
            parent_overlay: None,
            on_dismiss,
            fade_duration: None,
        },
    );
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

fn apply_cursor_to_window(
    platform_window: &bastyde_platform::PlatformWindow,
    cursor: bastyde_core::CursorIcon,
) {
    let winit_cursor = match cursor {
        bastyde_core::CursorIcon::Default => winit::window::CursorIcon::Default,
        bastyde_core::CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
        bastyde_core::CursorIcon::Text => winit::window::CursorIcon::Text,
        bastyde_core::CursorIcon::Crosshair => winit::window::CursorIcon::Crosshair,
        bastyde_core::CursorIcon::Move => winit::window::CursorIcon::Move,
        bastyde_core::CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
        bastyde_core::CursorIcon::Grab => winit::window::CursorIcon::Grab,
        bastyde_core::CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
        bastyde_core::CursorIcon::ColResize => winit::window::CursorIcon::ColResize,
        bastyde_core::CursorIcon::RowResize => winit::window::CursorIcon::RowResize,
        bastyde_core::CursorIcon::NeswResize => winit::window::CursorIcon::NeswResize,
        bastyde_core::CursorIcon::NwseResize => winit::window::CursorIcon::NwseResize,
    };
    platform_window.window().set_cursor(winit_cursor);
}

#[derive(Debug)]
struct IdleTrace {
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
    idle_callbacks_run: u64,
    control_flow_wait: u64,
    control_flow_wait_until: u64,
    timer_windows: usize,
    animation_timers: usize,
    tooltip_timers: usize,
}

impl IdleTrace {
    fn from_env() -> Option<Self> {
        match std::env::var("Bastyde_IDLE_TRACE") {
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

    fn note_redraw_request(&mut self, reason: &'static str) {
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

    fn note_frame_request_redraw(&mut self) {
        self.frame_request_redraws += 1;
        self.maybe_report();
    }

    fn maybe_report(&mut self) {
        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        eprintln!(
            "bastyde_idle_trace redraw_requested={} rendered_frames={} resume_time_reached={} request_redraw_all={} input_redraws={{cursor:{},mouse_input:{},mouse_wheel:{},keyboard:{},resize:{},frame_request:{}}} idle_callbacks={} control_flow={{wait:{},wait_until:{}}} timers={{windows:{},animations:{},tooltips:{}}}",
            self.redraw_requested,
            self.rendered_frames,
            self.resume_time_reached,
            self.request_redraw_all,
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

struct BastydeAppHandler {
    wm: WindowManager,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    initial_window: Option<WindowConfig>,
    initial_created: bool,
    idle_budget: Duration,
    idle_trace: Option<IdleTrace>,
    theme_mode: ThemeMode,
    #[cfg(feature = "text")]
    typesetter: SharedTypesetter,
    /// Kept alive for the lifetime of the event loop so that the
    /// `notify::RecommendedWatcher` background thread keeps running.
    /// Created in `BastydeAppBuilder::run` when the `I18nConfig` registers
    /// any `runtime_override`s; otherwise `None`.
    _i18n_watcher: Option<bastyde_i18n::FtlFileWatcher>,
    /// Optional per-loop-turn closure (e.g. an async executor poll) installed
    /// via [`BastydeAppBuilder::on_loop_tick`]. Runs at the top of
    /// `about_to_wait`; returning `true` means tasks advanced and a repaint is
    /// needed. Async-agnostic — the loop only ever sees `FnMut`.
    loop_tick: Option<Box<dyn FnMut() -> bool>>,
    /// Shared flag a `loop_tick` owner sets while it wants continuous polling.
    /// Read in `update_control_flow` to force `ControlFlow::Poll`; when clear,
    /// the loop sleeps until the next event (off-thread wakes via the proxy).
    loop_tick_poll: Option<std::rc::Rc<std::cell::Cell<bool>>>,
}

impl BastydeAppHandler {
    fn new(
        theme: Theme,
        theme_mode: ThemeMode,
        app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
        initial_window: WindowConfig,
        app_context_template: Option<std::rc::Rc<TreeAppContext>>,
        #[cfg(feature = "text")] typesetter: SharedTypesetter,
        i18n_watcher: Option<bastyde_i18n::FtlFileWatcher>,
        event_proxy: AppEventProxy,
    ) -> Self {
        let mut wm = WindowManager::new(theme);
        wm.set_theme_mode(theme_mode);
        wm.set_event_proxy(event_proxy);
        if let Some(template) = app_context_template {
            wm.set_app_context_template(template);
        }

        #[cfg(feature = "text")]
        {
            wm.set_typesetter(typesetter.clone());
        }

        Self {
            wm,
            app_event_handler,
            initial_window: Some(initial_window),
            initial_created: false,
            idle_budget: Duration::from_millis(4),
            idle_trace: IdleTrace::from_env(),
            theme_mode,
            #[cfg(feature = "text")]
            typesetter,
            _i18n_watcher: i18n_watcher,
            loop_tick: None,
            loop_tick_poll: None,
        }
    }

    fn process_pending(&mut self, event_loop: &ActiveEventLoop) {
        self.wm.process_pending(event_loop);
    }

    fn process_modal_requests(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let native_supported = bastyde_platform::supports_native_modal_windows();
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
                        if let Some(managed) = self.wm.get_by_bastyde_mut(source_window) {
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
                            config = config.size(width, height).min_size(width, height);
                        }
                        self.wm.create_window(
                            config.root(move |tree, _state| builder(tree)),
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
        let mut any_frame_requested = false;
        for managed in self.wm.iter() {
            let animation_count = managed.tree.active_animation_count();
            let tooltip_count = managed.tree.pending_tooltip_count();
            if animation_count > 0 || tooltip_count > 0 {
                timer_windows += 1;
            }
            animation_timers += animation_count;
            tooltip_timers += tooltip_count;
            if let Some(deadline) = managed.tree.next_timer_deadline() {
                earliest_deadline = Some(match earliest_deadline {
                    Some(current) => current.min(deadline),
                    None => deadline,
                });
            }
            if managed.tree.frame_requested() {
                any_frame_requested = true;
            }
        }

        // An installed loop-tick owner (e.g. the `bastyde-async` executor)
        // can request continuous polling while it still has runnable work.
        if let Some(poll) = &self.loop_tick_poll
            && poll.get()
        {
            any_frame_requested = true;
        }

        if any_frame_requested {
            // A widget has a per-frame effect actively running
            // (caret blink, drag auto-scroll, continuous animation
            // that drives the state via its tick closure). Poll
            // mode keeps winit pumping events at the OS's maximum
            // rate instead of sleeping — the only way to get a
            // visibly regular blink cadence without a dedicated
            // timer wake. Other widgets that only need deadline
            // wakes keep doing that; frame pumping is additive.
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

    fn post_event(&mut self, event_loop: &ActiveEventLoop) {
        // App-wide environment changes (theme / locale) raised by a handler
        // in one window fan out to every window's tree, marking the
        // non-originating windows dirty. Those windows never received the
        // triggering event, so they would otherwise stay un-repainted —
        // `request_redraw_all()` below (gated on these flags) fixes that.
        let had_locale = self.wm.drain_pending_locale_requests();
        let had_theme = self.wm.drain_pending_theme_requests();
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
        if had_locale || had_theme || had_commands || had_modal_requests || had_modal_dismissals {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_request_redraw_all();
            }
            self.wm.request_redraw_all();
        }
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }

    /// Dispatch a widget event into the named window's `WidgetTree`
    /// with a real [`bastyde_core::WindowOps`] sink so handlers can
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
        let current_id = current.bastyde_id;

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

        Self::reconcile_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
    }

    /// Apply a [`MenubarAction`](bastyde_core::window::MenubarAction)
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
        action: bastyde_core::window::MenubarAction,
        event_loop: &ActiveEventLoop,
    ) {
        use bastyde_core::window::MenubarAction;
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.bastyde_id;

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
                    let pointer = current.tree.bounds(trigger_id).center();
                    current.tree.dispatch_event_with_ops(
                        WidgetEvent::PointerDown {
                            position: pointer,
                            button: bastyde_core::event::PointerButton::Primary,
                            modifiers: bastyde_core::event::Modifiers::NONE,
                        },
                        &mut ops,
                    );
                    current.tree.dispatch_event_with_ops(
                        WidgetEvent::PointerUp {
                            position: pointer,
                            button: bastyde_core::event::PointerButton::Primary,
                            modifiers: bastyde_core::event::Modifiers::NONE,
                        },
                        &mut ops,
                    );
                }
            }
        }

        Self::reconcile_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
    }

    /// Bring the winit window's OS-IME state in line with the focused
    /// widget's descriptor. Enablement + purpose are declarative: a focused
    /// text widget carries `Some(ImeContext { purpose })`, everything else
    /// `None`. Applied only on change vs. the per-window cache — repeated
    /// `set_ime_allowed(true)` can cancel an active composition. The caret
    /// area is reported separately (and idempotently) by the focused widget
    /// via `WindowOps::set_ime_cursor_area`.
    fn reconcile_ime(managed: &mut crate::window_manager::ManagedWindow) {
        match managed.tree.ime_context_for_focused() {
            Some(ctx) => {
                if managed.ime_purpose != Some(ctx.purpose) {
                    managed
                        .platform_window
                        .window()
                        .set_ime_purpose(Self::map_ime_purpose(ctx.purpose));
                    managed.ime_purpose = Some(ctx.purpose);
                }
                if managed.ime_allowed != Some(true) {
                    managed.platform_window.window().set_ime_allowed(true);
                    managed.ime_allowed = Some(true);
                }
            }
            None => {
                if managed.ime_allowed != Some(false) {
                    managed.platform_window.window().set_ime_allowed(false);
                    managed.ime_allowed = Some(false);
                    // Force the purpose to re-apply when IME is next enabled.
                    managed.ime_purpose = None;
                }
            }
        }
    }

    /// Map the core `ImePurpose` onto winit's enum at the platform boundary.
    fn map_ime_purpose(purpose: bastyde_core::ImePurpose) -> winit::window::ImePurpose {
        match purpose {
            bastyde_core::ImePurpose::Normal => winit::window::ImePurpose::Normal,
            bastyde_core::ImePurpose::Password => winit::window::ImePurpose::Password,
            bastyde_core::ImePurpose::Terminal => winit::window::ImePurpose::Terminal,
        }
    }

    /// Run `f` against window `winit_id`'s tree with a real
    /// [`WindowOps`](bastyde_core::WindowOps) sink (so `open_window`,
    /// `parent_window_handle`, etc. work). Encapsulates the take-out /
    /// build-`WindowOpsImpl` / reinsert dance that the `AppEvent::External`
    /// routers and the mount-action drain all share — keeping the reinsert
    /// (whose omission silently freezes a window) in exactly one place.
    /// No-op if `winit_id` is not a managed window.
    fn run_in_window(
        &mut self,
        winit_id: winit::window::WindowId,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(&mut WidgetTree, &mut crate::window_manager::WindowOpsImpl),
    ) {
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return;
        };
        let current_id = current.bastyde_id;

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

    /// Try to route an `AppEvent::External` payload as a
    /// [`FileDialogEventPayload`](bastyde_platform::file_dialog::FileDialogEventPayload).
    /// Returns `Ok(())` if the payload matched and was delivered to
    /// the originating window's tree, `Err(payload)` to hand the
    /// box back for fallthrough to other downcast attempts.
    ///
    /// Routing details:
    /// - Resolves `payload.window_id_owner` to the matching winit
    ///   `WindowId` via `WindowManager::bastyde_to_winit_map`.
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
            use bastyde_platform::file_dialog::{FileDialogEventPayload, FileDialogHandle};

            let payload = match payload.downcast::<FileDialogEventPayload>() {
                Ok(boxed) => *boxed,
                Err(other) => return Err(other),
            };

            // Find the originating window.
            let target_winit = self
                .wm
                .bastyde_to_winit_map()
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
            let current_id = current.bastyde_id;

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
    /// with a real [`EventContext`](bastyde_core::widget::EventContext) (so `ctx.parent_window_handle()` resolves).
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
    /// [`WebViewEventPayload`](bastyde_webview::WebViewEventPayload) posted by a
    /// web-view engine backend, delivering it to the originating window's tree
    /// via [`WebViewRegistry::deliver`](bastyde_webview::WebViewRegistry::deliver).
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
            use bastyde_webview::{WebViewEventPayload, WebViewRegistry};

            let payload = match payload.downcast::<WebViewEventPayload>() {
                Ok(boxed) => *boxed,
                Err(other) => return Err(other),
            };

            let target_winit = self
                .wm
                .bastyde_to_winit_map()
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
    /// [`AsyncCompletionPayload`](bastyde_core::AsyncCompletionPayload) posted
    /// by the `bastyde-async` executor when a `spawn_local_with` future
    /// resolves. Returns `Ok(())` if matched and delivered, `Err(payload)` to
    /// hand the box back for fallthrough.
    ///
    /// Uses only bastyde-core types ([`AsyncCompletionHandle`](bastyde_core::AsyncCompletionHandle)),
    /// so `bastyde-async` (which depends on `bastyde-app`) never has to be a
    /// dependency here — the same take/run-with-context/reinsert pattern as
    /// the file-dialog path. On any miss (window gone, runtime not installed,
    /// already-purged completion) the result is dropped silently.
    fn try_route_async_completion_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use bastyde_core::{AsyncCompletionHandle, AsyncCompletionPayload};

        let payload = match payload.downcast::<AsyncCompletionPayload>() {
            Ok(boxed) => *boxed,
            Err(other) => return Err(other),
        };

        let target_winit = self
            .wm
            .bastyde_to_winit_map()
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
        let current_id = current.bastyde_id;

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

    /// Try to route an `AppEvent::External` payload as a
    /// [`NativeMenuEventPayload`](bastyde_platform::native_menu::NativeMenuEventPayload)
    /// posted when the user chose an item in the platform's native menu bar.
    /// Resolves the item's [`MenuItemId`](bastyde_core::MenuItemId) to its
    /// recorded intent / action via the [`NativeMenuHandle`](bastyde_platform::native_menu::NativeMenuHandle)
    /// and fires it inside the originating window's `EventContext` with
    /// `IntentSource::Menu` — the same pipeline an in-window `MenuItem` uses.
    /// Same take/run-with-context/reinsert shape as the file-dialog router; any
    /// miss is dropped silently.
    fn try_route_native_menu_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use bastyde_core::Intent;
        use bastyde_core::telemetry::IntentSource;
        use bastyde_platform::native_menu::{NativeMenuEventPayload, NativeMenuHandle};

        let payload = match payload.downcast::<NativeMenuEventPayload>() {
            Ok(boxed) => *boxed,
            Err(other) => return Err(other),
        };

        let target_winit = self
            .wm
            .bastyde_to_winit_map()
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
        let current_id = current.bastyde_id;

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
    /// [`ExternalDndEventPayload`](bastyde_platform::external_dnd::ExternalDndEventPayload)
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
        use bastyde_platform::external_dnd::{ExternalDndEventPayload, ExternalDragEvent};

        let payload = match payload.downcast::<ExternalDndEventPayload>() {
            Ok(boxed) => *boxed,
            Err(other) => return Err(other),
        };

        let Some(winit_id) = self
            .wm
            .bastyde_to_winit_map()
            .get(&payload.window_id_owner)
            .copied()
        else {
            // Window already torn down — drop silently.
            return Ok(());
        };
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return Ok(());
        };
        let current_id = current.bastyde_id;

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
        let current_id = current.bastyde_id;

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
                let target_widget = if bastyde_core::accessibility::is_synthetic(req.target_node) {
                    managed.tree.widget_for_synthetic(req.target_node)
                } else {
                    Some(bastyde_core::accessibility::node_id_to_widget_id(
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
        let current_id = current.bastyde_id;
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

        let a11y_update = current.tree.sync_accessibility();
        current.platform_window.update_accessibility(a11y_update);

        // Catch-all IME reconcile: covers focus changes from any source
        // (access actions, programmatic focus, rebuild) that didn't go
        // through `dispatch_in_window`. Layout has settled, so the focused
        // node's descriptor is current. Cheap + deduped, safe every frame.
        Self::reconcile_ime(&mut current);

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
        let clear = bastyde_render::vertex::srgb_to_linear_rgba(
            managed.tree.theme().colors.surface_main.to_array(),
        );
        match managed.platform_window.render_frame(&frame, clear) {
            bastyde_platform::FrameOutcome::Rendered => {
                if let Some(trace) = &mut self.idle_trace {
                    trace.note_rendered_frame();
                }
            }
            bastyde_platform::FrameOutcome::Skipped => {
                if !managed.occluded {
                    managed.platform_window.request_redraw();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            bastyde_platform::FrameOutcome::NeedsReconfigure => {
                managed.platform_window.reconfigure_surface();
                managed.platform_window.request_redraw();
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            bastyde_platform::FrameOutcome::Error(e) => {
                eprintln!("bastyde-app: {e}, reconfiguring surface");
                managed.platform_window.reconfigure_surface();
                managed.platform_window.request_redraw();
                self.wm.reinsert_managed(window_id, current);
                return;
            }
        }

        if managed.tree.frame_requested() {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_frame_request_redraw();
            }
            managed.platform_window.request_redraw();
        }

        self.wm.reinsert_managed(window_id, current);
    }

    fn handle_window_event_inner(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let bastyde_id = self.wm.bastyde_id_for_winit(window_id);

        if let Some(fid) = bastyde_id
            && self.wm.is_blocked(fid)
            && !matches!(event, WindowEvent::CloseRequested)
        {
            self.wm.refocus_modal_child(fid);
            self.update_control_flow(event_loop);
            return;
        }

        self.handle_accessibility_actions(window_id, &event, event_loop);

        match event {
            WindowEvent::CloseRequested => {
                if let Some(fid) = bastyde_id {
                    self.wm.close_window(fid);
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
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.translation_state.set_scale_factor(scale_factor);
                    managed.platform_window.set_scale_factor(scale_factor);
                    managed.tree.set_device_scale_factor(scale_factor as f32);
                }
                #[cfg(feature = "text")]
                {
                    self.typesetter.set_scale_factor(scale_factor as f32);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let maybe_evt = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    event_translation::translate_cursor_moved(
                        position.x,
                        position.y,
                        &mut managed.translation_state,
                    )
                } else {
                    None
                };
                if let Some(evt) = maybe_evt {
                    self.dispatch_in_window(window_id, evt, event_loop);
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    apply_cursor_to_window(&managed.platform_window, managed.tree.current_cursor());
                    if managed.tree.needs_redraw() {
                        if let Some(trace) = &mut self.idle_trace {
                            trace.note_redraw_request("cursor");
                        }
                        managed.platform_window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let maybe_evt = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    event_translation::translate_mouse_input(
                        state,
                        button,
                        &managed.translation_state,
                    )
                } else {
                    None
                };
                if let Some(evt) = maybe_evt {
                    self.dispatch_in_window(window_id, evt, event_loop);
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    apply_cursor_to_window(&managed.platform_window, managed.tree.current_cursor());
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("mouse_input");
                    }
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let maybe_evt = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    event_translation::translate_mouse_wheel(
                        delta,
                        phase,
                        &managed.translation_state,
                    )
                } else {
                    None
                };
                if let Some(evt) = maybe_evt {
                    self.dispatch_in_window(window_id, evt, event_loop);
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("mouse_wheel");
                    }
                    managed.platform_window.request_redraw();
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
                // Track Caps Lock from the discrete key press — winit's
                // `ModifiersState` carries no lock state — toggling on
                // each key-down edge and pushing the result to
                // `WindowState::caps_lock` for the password-field warning.
                if key_event.state == winit::event::ElementState::Pressed
                    && matches!(
                        event_translation::translate_key(&key_event.logical_key),
                        Some(bastyde_core::event::Key::CapsLock)
                    )
                    && let Some(managed) = self.wm.get_by_winit_mut(window_id)
                {
                    managed.caps_lock_active = !managed.caps_lock_active;
                    managed
                        .state
                        .set_caps_lock_from_os(managed.caps_lock_active);
                }

                // Bare-Alt-tap detection: every non-Alt KeyDown while
                // Alt is held flips the sticky flag, so the falling
                // edge of `alt_down` only counts as a tap when no
                // chord was composed. winit fires modifier keys
                // through `ModifiersChanged`, not `KeyboardInput`, so
                // every KeyDown we see here is a non-modifier and
                // qualifies as an "other key" press.
                if key_event.state == winit::event::ElementState::Pressed
                    && event_translation::translate_key(&key_event.logical_key).is_some()
                    && let Some(managed) = self.wm.get_by_winit_mut(window_id)
                {
                    managed.state.note_non_alt_keydown_during_alt();
                }
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
                                    d.try_handle(&bastyde_core::window::MenubarKeyEvent {
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
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.focused = focused;
                    let active = managed.focused && !managed.occluded;
                    managed.tree.set_window_active(active);
                    managed.state.set_focused_from_os(focused);
                    if focused {
                        newly_focused = Some(managed.bastyde_id);
                    }
                }
                // The global native menu (macOS) follows window focus: make the
                // focused window's installed menu the visible one.
                if let Some(bastyde_id) = newly_focused
                    && let Some(handle) = self.wm.app_context_template().and_then(|t| {
                        t.app_state::<bastyde_platform::native_menu::NativeMenuHandle>()
                            .cloned()
                    })
                {
                    handle.activate_window(bastyde_id);
                }
            }
            // macOS-only in winit 0.30 (X11/Wayland/Windows never emit
            // this). Handled for parity with Focused so a macOS app
            // that is hidden behind another window — still focused —
            // also parks its animations.
            WindowEvent::Occluded(occluded) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.occluded = occluded;
                    let active = managed.focused && !managed.occluded;
                    managed.tree.set_window_active(active);
                    // When the window becomes visible again, drive a
                    // fresh redraw — the render loop stopped pinging
                    // while we were occluded, so without this nudge
                    // the window stays frozen until the user moves
                    // the mouse or hits a key.
                    if !occluded {
                        managed.platform_window.request_redraw();
                    }
                }
            }
            _ => {}
        }

        self.post_event(event_loop);
    }

    fn handle_theme_changed(&mut self, winit_theme: winit::window::Theme) {
        match self.theme_mode {
            ThemeMode::Manual => {} // ignore OS theme changes
            ThemeMode::FollowSystem => {
                let theme = match winit_theme {
                    winit::window::Theme::Dark => bastyde_core::presets::intui::dark(),
                    winit::window::Theme::Light => bastyde_core::presets::intui::light(),
                };
                self.wm.set_theme(theme);
            }
            ThemeMode::Native => {
                // Re-query OS colors and rebuild theme
                let os = bastyde_platform::os_theme::query_os_theme_colors();
                let base = if os.color_scheme.is_dark() {
                    bastyde_core::presets::intui::dark()
                } else {
                    bastyde_core::presets::intui::light()
                };
                let theme = Theme {
                    colors: ColorTokens::from_os_colors(&os),
                    ..base
                };
                self.wm.set_theme(theme);
            }
        }
    }
}

impl ApplicationHandler<AppEvent> for BastydeAppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_resume_time_reached();
                trace.note_request_redraw_all();
            }
            self.wm.request_redraw_all();
        }
        self.update_control_flow(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        if let Some(handler) = &mut self.app_event_handler {
            handler(&event);
        }
        match event {
            // Backend-event subscription delivery (architecture §9.4): look
            // up the UI-side callback in the shared app context and invoke
            // it with the downcast event payload. The shared template is
            // the same Rc held by every window's tree, so we don't need to
            // route by window.
            AppEvent::SubscriptionEvent { sub_id, event } => {
                if let Some(template) = self.wm.app_context_template() {
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
                let parsed: Result<bastyde_i18n::LanguageIdentifier, _> = locale.parse();
                match parsed {
                    Ok(loc) => {
                        let reloaded = bastyde_i18n::thread_local::with_active(|mgr| {
                            mgr.reload_from_path(&loc, &path)
                        });
                        match reloaded {
                            Some(Ok(())) => {}
                            Some(Err(e)) => eprintln!(
                                "bastyde-app: hot-reload failed for {loc} ({}): {e}",
                                path.display()
                            ),
                            None => eprintln!(
                                "bastyde-app: hot-reload event for {loc} but no i18n manager installed"
                            ),
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "bastyde-app: hot-reload event with invalid locale `{locale}`: {e}"
                        )
                    }
                }
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
                if let Some(payload) = payload {
                    {
                        if let Some(req) = payload.downcast_ref::<CloseWindowRequest>() {
                            self.wm.queue_close(req.bastyde_id);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarSyntheticEvent>() {
                            // Windows custom-chrome wndproc sends this when
                            // `WM_NCLBUTTONUP` fires over a control-button
                            // hit-region. The button's pixels are owned by
                            // the OS so the widget tree never saw the click;
                            // re-issue it as a synthetic tap on the
                            // matching `ControlButton`.
                            self.wm
                                .route_title_bar_synthetic_tap(evt.bastyde_id, evt.target);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarHoverEvent>() {
                            // Same idea for hover: `WM_NCMOUSEMOVE` over a
                            // control-button hit-region delivers an
                            // entered/leave event the widget tree never
                            // sees, so we drive the matching button's
                            // hover signal explicitly.
                            self.wm.route_title_bar_synthetic_hover(
                                evt.bastyde_id,
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
                        }
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
        // `bastyde-async` is installed) before computing the next control
        // flow. A `true` return means tasks advanced and may have mutated
        // reactive state, so repaint the open windows — mirroring the
        // subscription-delivery redraw in `user_event`.
        if let Some(tick) = &mut self.loop_tick
            && tick()
        {
            self.wm.request_redraw_all();
        }
        self.process_pending(event_loop);
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }
}

/// Payload used by `TitleBarHostCallbacks::request_close` to route a
/// host-initiated close back to the main event loop. The host's
/// close callback boxes one of these through `AppEventProxy::send_external`;
/// `BastydeAppHandler::user_event` downcasts the payload and calls
/// `WindowManager::queue_close` so the window tears down on the next tick
/// (matching the `WindowEvent::CloseRequested` path).
#[derive(Debug, Clone, Copy)]
pub struct CloseWindowRequest {
    pub bastyde_id: BastydeWindowId,
}

/// Test / demo payload that replays a scripted IME sequence into the
/// focused window's focused widget, through the same dispatch path the
/// real `WindowEvent::Ime` arm uses — so the full preedit pipeline
/// (document mutation, underline, caret-area reporting, AT selection) can
/// be exercised without an OS input method installed.
///
/// Post it via [`AppEventPoster::post_external`](bastyde_core::AppEventPoster)
/// (reachable from a handler with `ctx.poster()`).
#[derive(Debug, Clone)]
pub struct SyntheticImeInject {
    pub events: Vec<bastyde_core::event::WidgetEvent>,
}

// `TitleBarSyntheticEvent` and `TitleBarHoverEvent` live in
// `bastyde_core::window_chrome` so bastyde-platform (which posts them from
// the Windows wndproc subclass) and bastyde-app (which routes them) can
// both name the type without bastyde-platform depending on bastyde-app.
pub use bastyde_core::{TitleBarHoverEvent, TitleBarSyntheticEvent};

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
    /// posting mechanism behind a closure that bastyde-core can hold
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
/// integrations (e.g. the `bastyde-async` executor's cross-thread waker, wired
/// via [`BastydeAppBuilder::on_ready`]). bastyde-core cannot import winit, so
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

/// Builder for a Bastyde application.
pub struct BastydeAppBuilder {
    theme: Theme,
    theme_mode: ThemeMode,
    #[cfg(feature = "text")]
    typesetter: Option<SharedTypesetter>,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
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
    tooltip_contents: Vec<bastyde_widgets::tooltip::TooltipContent>,
    /// OS-correct application paths (config / data dirs). Set via
    /// [`application`](Self::application) or [`app_paths`](Self::app_paths).
    /// Required when `settings_bundle` is set.
    app_paths: Option<bastyde_settings::AppPaths>,
    /// Persistence configuration. When present, the bundle is opened
    /// at startup and each enabled service is registered into the
    /// `app_state` registry under its concrete type.
    settings_bundle: Option<bastyde_settings::SettingsBundle>,
    /// Telemetry configuration. When present, the bundle is opened
    /// after `settings_bundle` (it depends on `SettingsStore`) and the
    /// resulting `OpenedTelemetry` + `TelemetryContext` are registered
    /// into the `app_state` registry. The `TelemetryContext` is the
    /// hook the dispatch tap in
    /// [`bastyde_core::widget_tree::WidgetTree::dispatch_intent`] uses to
    /// emit `intent.dispatched` events.
    #[cfg(feature = "telemetry")]
    telemetry_bundle: Option<bastyde_telemetry::TelemetryBundle>,
    /// Per-loop-turn closure + poll flag installed via
    /// [`on_loop_tick`](Self::on_loop_tick). Async-agnostic; moved into the
    /// handler at `run`.
    loop_tick: Option<Box<dyn FnMut() -> bool>>,
    loop_tick_poll: Option<std::rc::Rc<std::cell::Cell<bool>>>,
}

impl BastydeAppBuilder {
    pub fn new() -> Self {
        Self {
            theme: bastyde_core::presets::intui::light(),
            theme_mode: ThemeMode::Manual,
            #[cfg(feature = "text")]
            typesetter: None,
            app_event_handler: None,
            on_ready: Vec::new(),
            initial_window: None,
            event_source: None,
            app_state_registry: HashMap::new(),
            i18n: None,
            tooltip_contents: Vec::new(),
            app_paths: None,
            settings_bundle: None,
            #[cfg(feature = "telemetry")]
            telemetry_bundle: None,
            loop_tick: None,
            loop_tick_poll: None,
        }
    }

    /// Identify the application for OS-correct path resolution. The
    /// `(qualifier, organization, application)` triple follows the
    /// `directories` convention (e.g. `("com", "FernTech", "Skribisto")`).
    /// Required when [`settings`](Self::settings) is used.
    ///
    /// # Panics
    ///
    /// Panics if the OS does not expose a usable home directory
    /// (typically a sandboxed environment with `HOME` unset). Use
    /// [`app_paths`](Self::app_paths) to supply an explicit path
    /// in that situation.
    pub fn application(mut self, qualifier: &str, organization: &str, application: &str) -> Self {
        let paths = bastyde_settings::AppPaths::new(qualifier, organization, application)
            .unwrap_or_else(|| {
                panic!(
                    "BastydeAppBuilder::application(\"{qualifier}\", \"{organization}\", \
                     \"{application}\"): could not resolve a usable OS config directory. \
                     This typically happens in sandboxed environments with no HOME set. \
                     Use BastydeAppBuilder::app_paths(AppPaths::for_testing(...) or \
                     AppPaths::from_dirs(...)) to supply an explicit location.",
                )
            });
        self.app_paths = Some(paths);
        self
    }

    /// Provide an explicit [`AppPaths`](bastyde_settings::AppPaths). Used
    /// for portable-mode apps and tests.
    pub fn app_paths(mut self, paths: bastyde_settings::AppPaths) -> Self {
        self.app_paths = Some(paths);
        self
    }

    /// Read the currently-configured `AppPaths`, if any. Used by
    /// builder-extension traits (e.g. `install_toast` in `bastyde`)
    /// that need to open persistent files at install time before
    /// `run` fires.
    pub fn configured_app_paths(&self) -> Option<&bastyde_settings::AppPaths> {
        self.app_paths.as_ref()
    }

    /// Configure the persistence bundle. When `run`/`build_headless`
    /// fires, the bundle is opened against the configured `AppPaths`
    /// and every active service is registered in `app_state`, where
    /// it becomes reachable via the
    /// [`SettingsExt`](bastyde_settings::SettingsExt) trait.
    ///
    /// # Panics
    ///
    /// Panics during `run` / `build_headless` if no `AppPaths` was
    /// configured first via [`application`](Self::application) or
    /// [`app_paths`](Self::app_paths).
    pub fn settings(mut self, bundle: bastyde_settings::SettingsBundle) -> Self {
        self.settings_bundle = Some(bundle);
        self
    }

    /// Configure the telemetry stack (`bastyde-telemetry`). Mirrors
    /// [`settings`](Self::settings): the bundle is opened during
    /// `run` / `build_headless` against the configured `AppPaths`
    /// **and** the live `SettingsStore`, and the resulting handles
    /// (`OpenedTelemetry`, `TelemetryContext`, `DynamicReporter`) are
    /// registered into `app_state`. Apps reach them via
    /// [`bastyde_telemetry::TelemetryExt`] (`use bastyde_telemetry::TelemetryExt;`).
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
    pub fn telemetry(mut self, bundle: bastyde_telemetry::TelemetryBundle) -> Self {
        self.telemetry_bundle = Some(bundle);
        self
    }

    /// Register the application's tooltip string catalog.
    ///
    /// Each [`TooltipContent`](bastyde_widgets::tooltip::TooltipContent)
    /// in the list maps a short stable key (referenced from inline
    /// markup as `[label](:key)`) to a translatable body, an optional
    /// long-form "more" body revealed by the Accordion disclosure
    /// inside a sticky rich tooltip, and an optional keyboard shortcut
    /// (literal label — registry-backed auto-lookup is a follow-up).
    ///
    /// This is a **single-call registration**: the list is the
    /// application's complete tooltip catalog. Call once at app boot,
    /// before `run()`. Calling multiple times panics in debug builds.
    ///
    /// ```ignore
    /// use bastyde_widgets::tooltip::TooltipContent;
    ///
    /// BastydeAppBuilder::new()
    ///     .register_tooltips(vec![
    ///         TooltipContent::new("save-as", tr!(save_as_tooltip))
    ///             .for_shortcut("app.save_as"),
    ///         TooltipContent::new("autosave", tr!(autosave_tooltip))
    ///             .with_more(tr!(autosave_tooltip_more)),
    ///     ])
    ///     // …
    /// ```
    pub fn register_tooltips(
        mut self,
        contents: Vec<bastyde_widgets::tooltip::TooltipContent>,
    ) -> Self {
        self.tooltip_contents = contents;
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

    /// Install the rfd-backed native file-dialog service. Registers a
    /// [`FileDialogHandle`](bastyde_platform::file_dialog::FileDialogHandle)
    /// wrapping an
    /// [`RfdAsyncBackend`](bastyde_platform::file_dialog::RfdAsyncBackend)
    /// into the app-state registry. Reachable from any handler via
    /// `ctx.app_state::<FileDialogHandle>()`, or — with
    /// `use bastyde_platform::file_dialog::EventContextFileDialogExt;` —
    /// directly via `ctx.pick_file(req, |result, ctx| ...)`.
    ///
    /// Apps that ship a custom or mock backend bypass this and call
    /// `.app_state(FileDialogHandle::new(my_backend))` directly.
    #[cfg(feature = "rfd-backend")]
    pub fn install_file_dialog(mut self) -> Self {
        use bastyde_platform::file_dialog::{FileDialogHandle, RfdAsyncBackend};
        let handle = FileDialogHandle::new(RfdAsyncBackend::new());
        self.app_state_registry
            .insert(TypeId::of::<FileDialogHandle>(), Box::new(handle));
        self
    }

    /// Install the external (OS) drag-and-drop service. Registers an
    /// [`ExternalDndHandle`](bastyde_platform::external_dnd::ExternalDndHandle)
    /// wrapping the platform's default backend
    /// ([`default_backend`](bastyde_platform::external_dnd::default_backend) —
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
        use bastyde_platform::external_dnd::{ExternalDndHandle, default_backend};
        let handle = ExternalDndHandle::new(default_backend());
        self.app_state_registry
            .insert(TypeId::of::<ExternalDndHandle>(), Box::new(handle));
        self
    }

    /// Install the native (OS) menu service. Registers a
    /// [`NativeMenuHandle`](bastyde_platform::native_menu::NativeMenuHandle)
    /// wrapping the platform's default backend (a real `NSMenu` on macOS, a
    /// no-op elsewhere) into the app-state registry.
    ///
    /// Once installed, a [`MenuBar`](bastyde_widgets::MenuBar) built with
    /// `from_model(..).native_on_macos(..)` mirrors its [`MenuModel`](bastyde_widgets::MenuModel) into the
    /// global menu bar on macOS, and item activations route back through the
    /// usual `Intent`/`Action` pipeline. The global menu follows window focus
    /// automatically (see the `WindowEvent::Focused` arm).
    ///
    /// Apps that ship a custom backend bypass this and call
    /// `.app_state(NativeMenuHandle::new(my_backend))` directly.
    pub fn install_native_menu(mut self) -> Self {
        use bastyde_platform::native_menu::{NativeMenuHandle, default_backend};
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

    #[cfg(feature = "text")]
    pub fn typesetter(mut self, typesetter: SharedTypesetter) -> Self {
        self.typesetter = Some(typesetter);
        self
    }

    /// Register a handler for `AppEvent`s received from background threads.
    pub fn on_app_event(mut self, handler: impl FnMut(&AppEvent) + 'static) -> Self {
        self.app_event_handler = Some(Box::new(handler));
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
    /// General-purpose and async-agnostic — `bastyde-app` only ever sees
    /// `FnMut`. The optional `bastyde-async` crate uses this to drive a
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
    /// BastydeAppBuilder::new()
    ///     .theme(bastyde_core::presets::intui::light())
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
    fn install_settings(&mut self) -> Option<bastyde_settings::OpenedSettings> {
        let bundle = self.settings_bundle.take()?;
        let paths = self.app_paths.clone().expect(
            "BastydeAppBuilder::settings(...) requires .application(...) or .app_paths(...) \
             to be set first so persistence has a target directory.",
        );
        match bundle.open(&paths) {
            Ok(opened) => {
                self.app_state_registry.insert(
                    TypeId::of::<bastyde_settings::SettingsStore>(),
                    Box::new(opened.store.clone()),
                );
                if let Some(w) = &opened.window_state {
                    self.app_state_registry.insert(
                        TypeId::of::<bastyde_settings::WindowStateService>(),
                        Box::new(w.clone()),
                    );
                }
                Some(opened)
            }
            Err(e) => {
                eprintln!("bastyde-app: failed to open settings bundle: {e}");
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
    fn install_telemetry(&mut self, settings: Option<&bastyde_settings::SettingsStore>) {
        let Some(bundle) = self.telemetry_bundle.take() else {
            return;
        };
        let paths = self.app_paths.clone().expect(
            "BastydeAppBuilder::telemetry(...) requires .application(...) or .app_paths(...) \
             to be set first so the consent file has a target directory.",
        );
        let store = settings.expect(
            "BastydeAppBuilder::telemetry(...) requires .settings(...) so the runtime \
             endpoint-override key can be read from the SettingsStore. \
             Add .settings(SettingsBundle::new()) before .telemetry(...).",
        );
        match bundle.open(&paths, store) {
            Ok(opened) => {
                // Register the OpenedTelemetry under its concrete type
                // so widgets can access it via TelemetryExt::telemetry().
                self.app_state_registry.insert(
                    TypeId::of::<bastyde_telemetry::OpenedTelemetry>(),
                    Box::new(opened.clone()),
                );
                // Register the dispatch hook under the bastyde-core type.
                // The dispatch tap looks this up by TypeId.
                let session_id = generate_session_id();
                let tcx = bastyde_core::telemetry::TelemetryContext {
                    reporter: opened.reporter.clone()
                        as std::rc::Rc<dyn bastyde_core::telemetry::UsageReporter>,
                    session_id,
                    schema_version: opened.event_schema_version,
                };
                self.app_state_registry.insert(
                    TypeId::of::<bastyde_core::telemetry::TelemetryContext>(),
                    Box::new(tcx),
                );
            }
            Err(e) => {
                eprintln!("bastyde-app: failed to open telemetry bundle: {e}");
            }
        }
    }

    /// Build a headless app for testing (no window, no GPU).
    pub fn build_headless(mut self) -> HeadlessApp {
        // Install the tooltip registry before anything else — widgets
        // that read from it during their first build (e.g. rich
        // tooltips looking up their :key) need it available.
        if !self.tooltip_contents.is_empty() {
            bastyde_widgets::tooltip::install_tooltip_registry(std::mem::take(
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
            let stub_state = bastyde_core::WindowState::new(bastyde_core::WindowStateInit {
                id: crate::BastydeWindowId::new(0),
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
    pub fn run(mut self) {
        // Install the tooltip registry before the window manager
        // starts building trees — rich tooltips read from it during
        // their first build.
        if !self.tooltip_contents.is_empty() {
            bastyde_widgets::tooltip::install_tooltip_registry(std::mem::take(
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

        let event_loop = winit::event_loop::EventLoop::<AppEvent>::with_user_event()
            .build()
            .expect("winit event loop creation failed");
        event_loop.set_control_flow(ControlFlow::Wait);

        // Always create a proxy: it's needed by both `on_ready` (if set)
        // and by the event-source poster (if a source is registered). The
        // proxy is cheap to clone.
        let proxy = AppEventProxy {
            inner: event_loop.create_proxy(),
        };

        // Build the i18n hot-reload watcher if any `runtime_override`s
        // were registered. The sink posts `AppEvent::I18nReload` through
        // the event loop proxy; the watcher's background thread converts
        // file-change events into these messages. The watcher handle is
        // handed to `BastydeAppHandler` which keeps it alive for the loop
        // lifetime. Construction failures log and fall back to no
        // hot-reload (the rest of i18n still works).
        let i18n_watcher = if runtime_overrides.is_empty() {
            None
        } else {
            let proxy_for_sink = proxy.inner.clone();
            let sink: bastyde_i18n::ReloadSink = std::sync::Arc::new(move |locale, path| {
                let _ = proxy_for_sink.send_event(AppEvent::I18nReload {
                    locale: locale.to_string(),
                    path,
                });
            });
            match bastyde_i18n::FtlFileWatcher::new(runtime_overrides, sink) {
                Ok(watcher) => Some(watcher),
                Err(e) => {
                    eprintln!("bastyde-app: failed to start i18n file watcher: {e}");
                    None
                }
            }
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
            use bastyde_platform::clipboard::{ArboardClipboard, ClipboardHandle, MemoryClipboard};
            use std::any::TypeId;
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
            .expect("BastydeAppBuilder::initial_window(WindowConfig) is required");

        let mut app = BastydeAppHandler::new(
            self.theme,
            self.theme_mode,
            self.app_event_handler,
            initial_config,
            app_context_template,
            #[cfg(feature = "text")]
            typesetter,
            i18n_watcher,
            proxy.clone(),
        );
        // Hand over any registered loop-tick hook (e.g. the `bastyde-async`
        // executor poll). Async-agnostic: just a closure + a poll flag.
        app.loop_tick = self.loop_tick;
        app.loop_tick_poll = self.loop_tick_poll;

        event_loop
            .run_app(&mut app)
            .expect("winit event loop exited with error");

        // Flush any pending settings writes synchronously before the
        // process exits. The `DebouncedWriter` background threads also
        // flush on Drop, but doing it synchronously here also surfaces
        // any I/O errors to stderr before the binding goes out of
        // scope.
        if let Some(opened) = opened_settings
            && let Err(e) = opened.flush_all()
        {
            eprintln!("bastyde-app: settings flush on exit failed: {e}");
        }
    }
}

impl Default for BastydeAppBuilder {
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
    bastyde_i18n::thread_local::install(mgr.clone());
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
    /// Active i18n manager, if `BastydeAppBuilder::i18n(...)` was used. Tests
    /// can reach the bundles, version signal, and locale signal directly
    /// through this handle.
    pub i18n_manager: Option<Rc<I18nManager>>,
    /// Active persistence services, if `BastydeAppBuilder::settings(...)`
    /// was used. Held here so the underlying `SettingsFile` clones
    /// (and their I/O threads) live as long as the headless app.
    pub settings: Option<bastyde_settings::OpenedSettings>,
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
    use bastyde_i18n::lit;
    use bastyde_tokens::Color;
    use bastyde_widgets::{Button, ModalContainer};

    #[test]
    fn builder_accepts_theme() {
        let app = BastydeAppBuilder::new()
            .theme(bastyde_core::presets::intui::light())
            .build_headless();
        assert_ne!(app.theme().colors.accent, Color::TRANSPARENT);
    }

    #[test]
    fn builder_with_root() {
        use bastyde_widgets::RectWidget;
        let app = BastydeAppBuilder::new()
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
        use bastyde_core::build_context::BuildContext;
        use bastyde_core::signal::Signal;
        use bastyde_core::widget::{LayoutContext, Widget};
        use std::rc::Rc;

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
            ) -> bastyde_core::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
        }

        let globals = Rc::new(AppGlobals {
            label: Signal::new("headless works".to_string()),
        });

        let observed = Signal::new(String::new());
        let observed_for_root = observed.clone();

        let _app = BastydeAppBuilder::new()
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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

    #[test]
    fn dismissing_modal_cascades_to_scrim() {
        // The scrim's `parent_overlay` is patched to the modal id
        // after both are pushed. Dismissing the modal must therefore
        // also dismiss the scrim through the cascade walk in
        // `dismiss_immediate`.
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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

    /// Test content widget: a focusable container with two focusable
    /// button descendants. `hint` controls which (if any) the widget
    /// reports as its `initial_focus_hint`.
    #[derive(Debug)]
    struct TwoButtonContent {
        root: Option<WidgetId>,
        second: Option<WidgetId>,
        hint_to_second: bool,
    }

    impl bastyde_core::Widget for TwoButtonContent {
        fn build(&mut self, ctx: &mut bastyde_core::BuildContext) -> Vec<WidgetId> {
            let first = ctx.add(Button::new(lit!("First")));
            let second = ctx.add(Button::new(lit!("Second")));
            let row = ctx.add(
                bastyde_widgets::HStack::new()
                    .add_child(first)
                    .add_child(second),
            );
            self.root = Some(row);
            self.second = Some(second);
            vec![row]
        }

        fn layout_response(
            &self,
            proposal: bastyde_canvas::SizeProposal,
            ctx: &bastyde_core::LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
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
