use fern_canvas::SizeProposal;
use fern_core::app_event::AppEvent;
use fern_core::event::WidgetEvent;
use fern_core::event_source::{
    AppEventPoster, EventSource, EventSourceAdapter, SubscriptionId, TreeAppContext,
};
use fern_core::modal::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
use fern_core::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::{WidgetId, WidgetTree};
use fern_i18n::{I18nConfig, I18nManager, LanguageIdentifier};
use fern_platform::event_translation;
use fern_tokens::{ColorTokens, Theme};
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
    /// Follow the OS light/dark preference using FernUI's built-in themes.
    FollowSystem,
    /// Adopt colors read directly from the OS/DE config files (GNOME/KDE/Cinnamon).
    /// Falls back to `FollowSystem` on unsupported platforms or DEs.
    Native,
}

#[cfg(feature = "text")]
use fern_text::SharedTypesetter;

use crate::window_config::{FernWindowId, WindowConfig};
use crate::window_manager::WindowManager;
use fern_core::WindowPlacement;

/// Interrogate the winit window for its current placement so an
/// `OS-initiated` state change can be mirrored into the corresponding
/// [`WindowState::placement`] signal without the observer pushing it
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
    let content_id = match request.content {
        ModalContent::ExistingWidget(id) => id,
        ModalContent::Deferred(builder) => {
            let id = builder(tree);
            tree.set_dormant(id);
            id
        }
    };

    tree.activate(content_id);
    tree.show_overlay_from_source(
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

    let focus_target = requested_focus
        .filter(|id| tree.is_active(*id) && tree.is_descendant_of(*id, content_id))
        .or_else(|| tree.widget_initial_focus_hint(content_id))
        .or_else(|| tree.first_focusable_descendant(content_id));
    if let Some(id) = focus_target {
        tree.focus(id);
    }
}

fn apply_cursor_to_window(
    platform_window: &fern_platform::PlatformWindow,
    cursor: fern_core::CursorIcon,
) {
    let winit_cursor = match cursor {
        fern_core::CursorIcon::Default => winit::window::CursorIcon::Default,
        fern_core::CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
        fern_core::CursorIcon::Text => winit::window::CursorIcon::Text,
        fern_core::CursorIcon::Crosshair => winit::window::CursorIcon::Crosshair,
        fern_core::CursorIcon::Move => winit::window::CursorIcon::Move,
        fern_core::CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
        fern_core::CursorIcon::Grab => winit::window::CursorIcon::Grab,
        fern_core::CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
        fern_core::CursorIcon::ColResize => winit::window::CursorIcon::ColResize,
        fern_core::CursorIcon::RowResize => winit::window::CursorIcon::RowResize,
        fern_core::CursorIcon::NeswResize => winit::window::CursorIcon::NeswResize,
        fern_core::CursorIcon::NwseResize => winit::window::CursorIcon::NwseResize,
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
    idle_callbacks_run: u64,
    control_flow_wait: u64,
    control_flow_wait_until: u64,
    timer_windows: usize,
    animation_timers: usize,
    tooltip_timers: usize,
}

impl IdleTrace {
    fn from_env() -> Option<Self> {
        match std::env::var("FERN_IDLE_TRACE") {
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

    fn maybe_report(&mut self) {
        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        eprintln!(
            "fern_idle_trace redraw_requested={} rendered_frames={} resume_time_reached={} request_redraw_all={} input_redraws={{cursor:{},mouse_input:{},mouse_wheel:{},keyboard:{},resize:{}}} idle_callbacks={} control_flow={{wait:{},wait_until:{}}} timers={{windows:{},animations:{},tooltips:{}}}",
            self.redraw_requested,
            self.rendered_frames,
            self.resume_time_reached,
            self.request_redraw_all,
            self.cursor_redraw_requests,
            self.mouse_input_redraw_requests,
            self.mouse_wheel_redraw_requests,
            self.keyboard_redraw_requests,
            self.resize_redraw_requests,
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
        self.idle_callbacks_run = 0;
        self.control_flow_wait = 0;
        self.control_flow_wait_until = 0;
    }
}

struct FernAppHandler {
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
    /// Created in `FernAppBuilder::run` when the `I18nConfig` registers
    /// any `runtime_override`s; otherwise `None`.
    _i18n_watcher: Option<fern_i18n::FtlFileWatcher>,
}

impl FernAppHandler {
    fn new(
        theme: Theme,
        theme_mode: ThemeMode,
        app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
        initial_window: WindowConfig,
        app_context_template: Option<std::rc::Rc<TreeAppContext>>,
        #[cfg(feature = "text")] typesetter: SharedTypesetter,
        i18n_watcher: Option<fern_i18n::FtlFileWatcher>,
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
        }
    }

    fn process_pending(&mut self, event_loop: &ActiveEventLoop) {
        self.wm.process_pending(event_loop);
    }

    fn process_modal_requests(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let native_supported = fern_platform::supports_native_modal_windows();
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
                        if let Some(managed) = self.wm.get_by_fern_mut(source_window) {
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
        self.wm.drain_pending_locale_requests();
        let had_commands = self.wm.drain_close_window_requests();
        let had_modal_requests = self.process_modal_requests(event_loop);
        let had_modal_dismissals = self.process_modal_dismissals();
        self.process_pending(event_loop);
        // Drain per-window command queues: app-side writes to
        // WindowState signals emitted WindowCommand values that the
        // registry routes through the per-window queue. Translate each
        // into the appropriate winit call.
        self.wm.drain_window_commands();
        if had_commands || had_modal_requests || had_modal_dismissals {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_request_redraw_all();
            }
            self.wm.request_redraw_all();
        }
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }

    /// Dispatch a widget event into the named window's `WidgetTree`
    /// with a real [`fern_core::WindowOps`] sink so handlers can
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
        let current_id = current.fern_id;

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

        self.wm.reinsert_managed(window_id, current);
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`FileDialogEventPayload`](fern_platform::file_dialog::FileDialogEventPayload).
    /// Returns `Ok(())` if the payload matched and was delivered to
    /// the originating window's tree, `Err(payload)` to hand the
    /// box back for fallthrough to other downcast attempts.
    ///
    /// Routing details:
    /// - Resolves `payload.window_id_owner` to the matching winit
    ///   `WindowId` via `WindowManager::fern_to_winit_map`.
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
            use fern_platform::file_dialog::{FileDialogEventPayload, FileDialogHandle};

            let payload = match payload.downcast::<FileDialogEventPayload>() {
                Ok(boxed) => *boxed,
                Err(other) => return Err(other),
            };

            // Find the originating window.
            let target_winit = self
                .wm
                .fern_to_winit_map()
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
            let current_id = current.fern_id;

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
        let current_id = current.fern_id;

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
                let target_widget = if fern_core::accessibility::is_synthetic(req.target_node) {
                    managed.tree.widget_for_synthetic(req.target_node)
                } else {
                    Some(fern_core::accessibility::node_id_to_widget_id(
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
        let current_id = current.fern_id;
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
            let atlas = self.typesetter.bridge().borrow_mut().atlas_info();
            if atlas.dirty && atlas.width > 0 && atlas.height > 0 {
                managed.platform_window.renderer_mut().upload_atlas(
                    atlas.width,
                    atlas.height,
                    &atlas.pixels,
                );
            }

            if atlas.glyphs_evicted {
                self.typesetter.bridge().borrow_mut().invalidate_cache();
                managed.tree.invalidate_all_paints();
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
                let atlas2 = self.typesetter.bridge().borrow_mut().atlas_info();
                if atlas2.dirty && atlas2.width > 0 && atlas2.height > 0 {
                    managed.platform_window.renderer_mut().upload_atlas(
                        atlas2.width,
                        atlas2.height,
                        &atlas2.pixels,
                    );
                }
            }
        }

        // The wgpu surface is Rgba8UnormSrgb: it expects linear-light color
        // values and applies sRGB encoding on write. Our Color stores sRGB-
        // encoded bytes (as designers specify them), so we must linearize
        // the clear color here the same way we do for vertex colors.
        let clear = fern_render::vertex::srgb_to_linear_rgba(
            managed.tree.theme().colors.surface_main.to_array(),
        );
        match managed.platform_window.render_frame(&frame, clear) {
            fern_platform::FrameOutcome::Rendered => {
                if let Some(trace) = &mut self.idle_trace {
                    trace.note_rendered_frame();
                }
            }
            fern_platform::FrameOutcome::Skipped => {
                if !managed.occluded {
                    managed.platform_window.request_redraw();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            fern_platform::FrameOutcome::NeedsReconfigure => {
                managed.platform_window.reconfigure_surface();
                managed.platform_window.request_redraw();
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            fern_platform::FrameOutcome::Error(e) => {
                eprintln!("fern-app: {e}, reconfiguring surface");
                managed.platform_window.reconfigure_surface();
                managed.platform_window.request_redraw();
                self.wm.reinsert_managed(window_id, current);
                return;
            }
        }

        if managed.tree.frame_requested() {
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
        let fern_id = self.wm.fern_id_for_winit(window_id);

        if let Some(fid) = fern_id
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
                if let Some(fid) = fern_id {
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
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.current_modifiers = mods.state();
                    managed
                        .translation_state
                        .set_modifiers(event_translation::translate_modifiers(mods.state()));
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
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
                    self.dispatch_in_window(window_id, evt, event_loop);
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("keyboard");
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
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.focused = focused;
                    let active = managed.focused && !managed.occluded;
                    managed.tree.set_window_active(active);
                    managed.state.set_focused_from_os(focused);
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
                    winit::window::Theme::Dark => Theme::dark_default(),
                    winit::window::Theme::Light => Theme::light_default(),
                };
                self.wm.set_theme(theme);
            }
            ThemeMode::Native => {
                // Re-query OS colors and rebuild theme
                let os = fern_platform::os_theme::query_os_theme_colors();
                let base = if os.color_scheme.is_dark() {
                    Theme::dark_default()
                } else {
                    Theme::light_default()
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

impl ApplicationHandler<AppEvent> for FernAppHandler {
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
                let parsed: Result<fern_i18n::LanguageIdentifier, _> = locale.parse();
                match parsed {
                    Ok(loc) => {
                        let reloaded = fern_i18n::thread_local::with_active(|mgr| {
                            mgr.reload_from_path(&loc, &path)
                        });
                        match reloaded {
                            Some(Ok(())) => {}
                            Some(Err(e)) => eprintln!(
                                "fern-app: hot-reload failed for {loc} ({}): {e}",
                                path.display()
                            ),
                            None => eprintln!(
                                "fern-app: hot-reload event for {loc} but no i18n manager installed"
                            ),
                        }
                    }
                    Err(e) => {
                        eprintln!("fern-app: hot-reload event with invalid locale `{locale}`: {e}")
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
                match self.try_route_file_dialog_payload(payload, event_loop) {
                    // File-dialog payload was consumed.
                    Ok(()) => {}
                    Err(payload) => {
                        if let Some(req) = payload.downcast_ref::<CloseWindowRequest>() {
                            self.wm.queue_close(req.fern_id);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarSyntheticEvent>() {
                            // Windows custom-chrome wndproc sends this when
                            // `WM_NCLBUTTONUP` fires over a control-button
                            // hit-region. The button's pixels are owned by
                            // the OS so the widget tree never saw the click;
                            // re-issue it as a synthetic tap on the
                            // matching `ControlButton`.
                            self.wm
                                .route_title_bar_synthetic_tap(evt.fern_id, evt.target);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarHoverEvent>() {
                            // Same idea for hover: `WM_NCMOUSEMOVE` over a
                            // control-button hit-region delivers an
                            // entered/leave event the widget tree never
                            // sees, so we drive the matching button's
                            // hover signal explicitly.
                            self.wm.route_title_bar_synthetic_hover(
                                evt.fern_id,
                                evt.target,
                                evt.entered,
                            );
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
        self.process_pending(event_loop);
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }
}

/// Payload used by `TitleBarHostCallbacks::request_close` to route a
/// host-initiated close back to the main event loop. The host's
/// close callback boxes one of these through `AppEventProxy::send_external`;
/// `FernAppHandler::user_event` downcasts the payload and calls
/// `WindowManager::queue_close` so the window tears down on the next tick
/// (matching the `WindowEvent::CloseRequested` path).
#[derive(Debug, Clone, Copy)]
pub struct CloseWindowRequest {
    pub fern_id: FernWindowId,
}

// `TitleBarSyntheticEvent` and `TitleBarHoverEvent` live in
// `fern_core::window_chrome` so fern-platform (which posts them from
// the Windows wndproc subclass) and fern-app (which routes them) can
// both name the type without fern-platform depending on fern-app.
pub use fern_core::{TitleBarHoverEvent, TitleBarSyntheticEvent};

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
    /// posting mechanism behind a closure that fern-core can hold
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

/// Bridges fern-core's `AppEventPoster` trait to the winit-backed
/// `AppEventProxy`. fern-core cannot import winit, so this trait
/// implementation lives in fern-app.
struct WinitAppEventPoster {
    proxy: AppEventProxy,
}

impl AppEventPoster for WinitAppEventPoster {
    fn post_subscription_event(
        &self,
        sub_id: SubscriptionId,
        event: Box<dyn std::any::Any + Send>,
    ) {
        self.proxy.post_subscription_event(sub_id, event);
    }

    fn post_external(&self, payload: Box<dyn std::any::Any + Send>) {
        let _ = self.proxy.inner.send_event(AppEvent::External(payload));
    }
}

/// Builder for a FernUI application.
pub struct FernAppBuilder {
    theme: Theme,
    theme_mode: ThemeMode,
    #[cfg(feature = "text")]
    typesetter: Option<SharedTypesetter>,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    on_ready: Option<Box<dyn FnOnce(AppEventProxy)>>,
    initial_window: Option<WindowConfig>,
    /// Type-erased adapter for the application's backend event source
    /// (architecture §9.4). Installed via `event_source<S>(source)`.
    event_source: Option<EventSourceAdapter>,
    /// Application-scoped values keyed by `TypeId` (architecture §9.5).
    /// Installed via `app_state::<T>(value)` and reachable from any
    /// `BuildContext` via `ctx.app_state::<T>()`.
    app_state_registry: HashMap<TypeId, Box<dyn Any>>,
    /// Internationalization configuration (architecture §12). Installed
    /// via `i18n(I18nConfig)`. When present, an `I18nManager` is built at
    /// `build_headless` / `run` time and registered on the thread-local so
    /// `tr!`-expanded code can resolve translations.
    i18n: Option<I18nConfig>,
    /// Tooltip content entries registered via
    /// [`register_tooltips`](Self::register_tooltips). Frozen into a
    /// thread-local registry in `run` / `build_headless` before the
    /// first frame builds.
    tooltip_contents: Vec<fern_widgets::tooltip::TooltipContent>,
    /// OS-correct application paths (config / data dirs). Set via
    /// [`application`](Self::application) or [`app_paths`](Self::app_paths).
    /// Required when `settings_bundle` is set.
    app_paths: Option<fern_settings::AppPaths>,
    /// Persistence configuration. When present, the bundle is opened
    /// at startup and each enabled service is registered into the
    /// `app_state` registry under its concrete type.
    settings_bundle: Option<fern_settings::SettingsBundle>,
    /// Telemetry configuration. When present, the bundle is opened
    /// after `settings_bundle` (it depends on `SettingsStore`) and the
    /// resulting `OpenedTelemetry` + `TelemetryContext` are registered
    /// into the `app_state` registry. The `TelemetryContext` is the
    /// hook the dispatch tap in
    /// [`fern_core::widget_tree::WidgetTree::dispatch_intent`] uses to
    /// emit `intent.dispatched` events.
    #[cfg(feature = "telemetry")]
    telemetry_bundle: Option<fern_telemetry::TelemetryBundle>,
}

impl FernAppBuilder {
    pub fn new() -> Self {
        Self {
            theme: Theme::light_default(),
            theme_mode: ThemeMode::Manual,
            #[cfg(feature = "text")]
            typesetter: None,
            app_event_handler: None,
            on_ready: None,
            initial_window: None,
            event_source: None,
            app_state_registry: HashMap::new(),
            i18n: None,
            tooltip_contents: Vec::new(),
            app_paths: None,
            settings_bundle: None,
            #[cfg(feature = "telemetry")]
            telemetry_bundle: None,
        }
    }

    /// Identify the application for OS-correct path resolution. The
    /// `(qualifier, organization, application)` triple follows the
    /// [`directories`] convention (e.g. `("com", "FernTech", "Skribisto")`).
    /// Required when [`settings`](Self::settings) is used.
    ///
    /// # Panics
    ///
    /// Panics if the OS does not expose a usable home directory
    /// (typically a sandboxed environment with `HOME` unset). Use
    /// [`app_paths`](Self::app_paths) to supply an explicit path
    /// in that situation.
    pub fn application(mut self, qualifier: &str, organization: &str, application: &str) -> Self {
        let paths = fern_settings::AppPaths::new(qualifier, organization, application)
            .unwrap_or_else(|| {
                panic!(
                    "FernAppBuilder::application(\"{qualifier}\", \"{organization}\", \
                     \"{application}\"): could not resolve a usable OS config directory. \
                     This typically happens in sandboxed environments with no HOME set. \
                     Use FernAppBuilder::app_paths(AppPaths::for_testing(...) or \
                     AppPaths::from_dirs(...)) to supply an explicit location.",
                )
            });
        self.app_paths = Some(paths);
        self
    }

    /// Provide an explicit [`AppPaths`](fern_settings::AppPaths). Used
    /// for portable-mode apps and tests.
    pub fn app_paths(mut self, paths: fern_settings::AppPaths) -> Self {
        self.app_paths = Some(paths);
        self
    }

    /// Configure the persistence bundle. When `run`/`build_headless`
    /// fires, the bundle is opened against the configured `AppPaths`
    /// and every active service is registered in `app_state`, where
    /// it becomes reachable via the
    /// [`SettingsExt`](fern_settings::SettingsExt) trait.
    ///
    /// # Panics
    ///
    /// Panics during `run` / `build_headless` if no `AppPaths` was
    /// configured first via [`application`](Self::application) or
    /// [`app_paths`](Self::app_paths).
    pub fn settings(mut self, bundle: fern_settings::SettingsBundle) -> Self {
        self.settings_bundle = Some(bundle);
        self
    }

    /// Configure the telemetry stack (`fern-telemetry`). Mirrors
    /// [`settings`](Self::settings): the bundle is opened during
    /// `run` / `build_headless` against the configured `AppPaths`
    /// **and** the live `SettingsStore`, and the resulting handles
    /// (`OpenedTelemetry`, `TelemetryContext`, `DynamicReporter`) are
    /// registered into `app_state`. Apps reach them via
    /// [`fern_telemetry::TelemetryExt`] (`use fern_telemetry::TelemetryExt;`).
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
    pub fn telemetry(mut self, bundle: fern_telemetry::TelemetryBundle) -> Self {
        self.telemetry_bundle = Some(bundle);
        self
    }

    /// Register the application's tooltip string catalog.
    ///
    /// Each [`TooltipContent`](fern_widgets::tooltip::TooltipContent)
    /// in the list maps a short stable key (referenced from inline
    /// markup as `[label](:key)`) to a translatable body, an optional
    /// long-form "more" body revealed by the Accordion disclosure
    /// inside a sticky rich tooltip, and an optional keyboard shortcut
    /// (literal label — registry-backed auto-lookup returns in step 3).
    ///
    /// This is a **single-call registration**: the list is the
    /// application's complete tooltip catalog. Call once at app boot,
    /// before `run()`. Calling multiple times panics in debug builds.
    ///
    /// ```ignore
    /// use fern_widgets::tooltip::TooltipContent;
    ///
    /// FernAppBuilder::new()
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
        contents: Vec<fern_widgets::tooltip::TooltipContent>,
    ) -> Self {
        self.tooltip_contents = contents;
        self
    }

    /// Register a backend event source (architecture §9.4). Widgets can
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
    /// can retrieve via `BuildContext::app_state::<T>()` (architecture §9.5).
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
    /// [`FileDialogHandle`](fern_platform::file_dialog::FileDialogHandle)
    /// wrapping an
    /// [`RfdAsyncBackend`](fern_platform::file_dialog::RfdAsyncBackend)
    /// into the app-state registry. Reachable from any handler via
    /// `ctx.app_state::<FileDialogHandle>()`, or — with
    /// `use fern_platform::file_dialog::EventContextFileDialogExt;` —
    /// directly via `ctx.pick_file(req, |result, ctx| ...)`.
    ///
    /// Apps that ship a custom or mock backend bypass this and call
    /// `.app_state(FileDialogHandle::new(my_backend))` directly.
    #[cfg(feature = "rfd-backend")]
    pub fn install_file_dialog(mut self) -> Self {
        use fern_platform::file_dialog::{FileDialogHandle, RfdAsyncBackend};
        let handle = FileDialogHandle::new(RfdAsyncBackend::new());
        self.app_state_registry
            .insert(TypeId::of::<FileDialogHandle>(), Box::new(handle));
        self
    }

    /// Register an `I18nConfig` (architecture §12). Constructs an
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
    pub fn on_ready(mut self, handler: impl FnOnce(AppEventProxy) + 'static) -> Self {
        self.on_ready = Some(Box::new(handler));
        self
    }

    /// Configure the initial window. Required — every app must open at
    /// least one window at startup. The single canonical entry point:
    /// build a [`WindowConfig`] and pass it here.
    ///
    /// ```ignore
    /// FernAppBuilder::new()
    ///     .theme(Theme::light_default())
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
    fn install_settings(&mut self) -> Option<fern_settings::OpenedSettings> {
        let bundle = self.settings_bundle.take()?;
        let paths = self.app_paths.clone().expect(
            "FernAppBuilder::settings(...) requires .application(...) or .app_paths(...) \
             to be set first so persistence has a target directory.",
        );
        match bundle.open(&paths) {
            Ok(opened) => {
                self.app_state_registry.insert(
                    TypeId::of::<fern_settings::SettingsStore>(),
                    Box::new(opened.store.clone()),
                );
                if let Some(w) = &opened.window_state {
                    self.app_state_registry.insert(
                        TypeId::of::<fern_settings::WindowStateService>(),
                        Box::new(w.clone()),
                    );
                }
                Some(opened)
            }
            Err(e) => {
                eprintln!("fern-app: failed to open settings bundle: {e}");
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
    fn install_telemetry(&mut self, settings: Option<&fern_settings::SettingsStore>) {
        let Some(bundle) = self.telemetry_bundle.take() else {
            return;
        };
        let paths = self.app_paths.clone().expect(
            "FernAppBuilder::telemetry(...) requires .application(...) or .app_paths(...) \
             to be set first so the consent file has a target directory.",
        );
        let store = settings.expect(
            "FernAppBuilder::telemetry(...) requires .settings(...) so the runtime \
             endpoint-override key can be read from the SettingsStore. \
             Add .settings(SettingsBundle::new()) before .telemetry(...).",
        );
        match bundle.open(&paths, store) {
            Ok(opened) => {
                // Register the OpenedTelemetry under its concrete type
                // so widgets can access it via TelemetryExt::telemetry().
                self.app_state_registry.insert(
                    TypeId::of::<fern_telemetry::OpenedTelemetry>(),
                    Box::new(opened.clone()),
                );
                // Register the dispatch hook under the fern-core type.
                // The dispatch tap looks this up by TypeId.
                let session_id = generate_session_id();
                let tcx = fern_core::telemetry::TelemetryContext {
                    reporter: opened.reporter.clone()
                        as std::rc::Rc<dyn fern_core::telemetry::UsageReporter>,
                    session_id,
                    schema_version: opened.event_schema_version,
                };
                self.app_state_registry.insert(
                    TypeId::of::<fern_core::telemetry::TelemetryContext>(),
                    Box::new(tcx),
                );
            }
            Err(e) => {
                eprintln!("fern-app: failed to open telemetry bundle: {e}");
            }
        }
    }

    /// Build a headless app for testing (no window, no GPU).
    pub fn build_headless(mut self) -> HeadlessApp {
        // Install the tooltip registry before anything else — widgets
        // that read from it during their first build (e.g. rich
        // tooltips looking up their :key) need it available.
        if !self.tooltip_contents.is_empty() {
            fern_widgets::tooltip::install_tooltip_registry(std::mem::take(
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
            let stub_state = fern_core::WindowState::new(fern_core::WindowStateInit {
                id: crate::FernWindowId::new(0),
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
            fern_widgets::tooltip::install_tooltip_registry(std::mem::take(
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
            .unwrap();
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
        // handed to `FernAppHandler` which keeps it alive for the loop
        // lifetime. Construction failures log and fall back to no
        // hot-reload (the rest of i18n still works).
        let i18n_watcher = if runtime_overrides.is_empty() {
            None
        } else {
            let proxy_for_sink = proxy.inner.clone();
            let sink: fern_i18n::ReloadSink = std::sync::Arc::new(move |locale, path| {
                let _ = proxy_for_sink.send_event(AppEvent::I18nReload {
                    locale: locale.to_string(),
                    path,
                });
            });
            match fern_i18n::FtlFileWatcher::new(runtime_overrides, sink) {
                Ok(watcher) => Some(watcher),
                Err(e) => {
                    eprintln!("fern-app: failed to start i18n file watcher: {e}");
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
            use fern_platform::clipboard::{ArboardClipboard, ClipboardHandle, MemoryClipboard};
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
        let poster: std::sync::Arc<dyn AppEventPoster> = std::sync::Arc::new(WinitAppEventPoster {
            proxy: proxy.clone(),
        });
        let base = match self.event_source {
            Some(adapter) => TreeAppContext::with_source_and_poster(adapter, poster.clone()),
            None => TreeAppContext::empty(),
        };
        let app_context_template = Some(std::rc::Rc::new(
            base.with_app_state(self.app_state_registry)
                .with_poster(poster),
        ));

        if let Some(on_ready) = self.on_ready {
            on_ready(proxy.clone());
        }

        let initial_config = self
            .initial_window
            .expect("FernAppBuilder::initial_window(WindowConfig) is required");

        let mut app = FernAppHandler::new(
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

        event_loop.run_app(&mut app).unwrap();

        // Flush any pending settings writes synchronously before the
        // process exits. The `DebouncedWriter` background threads also
        // flush on Drop, but doing it synchronously here also surfaces
        // any I/O errors to stderr before the binding goes out of
        // scope.
        if let Some(opened) = opened_settings
            && let Err(e) = opened.flush_all()
        {
            eprintln!("fern-app: settings flush on exit failed: {e}");
        }
    }
}

impl Default for FernAppBuilder {
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
    fern_i18n::thread_local::install(mgr.clone());
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
    /// Active i18n manager, if `FernAppBuilder::i18n(...)` was used. Tests
    /// can reach the bundles, version signal, and locale signal directly
    /// through this handle.
    pub i18n_manager: Option<Rc<I18nManager>>,
    /// Active persistence services, if `FernAppBuilder::settings(...)`
    /// was used. Held here so the underlying `SettingsFile` clones
    /// (and their I/O threads) live as long as the headless app.
    pub settings: Option<fern_settings::OpenedSettings>,
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
    use fern_tokens::Color;
    use fern_widgets::{Button, ModalContainer};

    #[test]
    fn builder_accepts_theme() {
        let app = FernAppBuilder::new()
            .theme(Theme::light_default())
            .build_headless();
        assert_ne!(app.theme().colors.accent, Color::TRANSPARENT);
    }

    #[test]
    fn builder_with_root() {
        use fern_widgets::RectWidget;
        let app = FernAppBuilder::new()
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
        use fern_core::build_context::BuildContext;
        use fern_core::signal::Signal;
        use fern_core::widget::{LayoutContext, Widget};
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
            ) -> fern_core::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
        }

        let globals = Rc::new(AppGlobals {
            label: Signal::new("headless works".to_string()),
        });

        let observed = Signal::new(String::new());
        let observed_for_root = observed.clone();

        let _app = FernAppBuilder::new()
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
        let request = ModalRequest::deferred(|tree| tree.add(Button::new_literal("Deferred")));

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::NativeWindow
        );
    }

    #[test]
    fn existing_widget_forces_in_tree_even_if_native_requested() {
        let mut tree = WidgetTree::new();
        let content = tree.add(Button::new_literal("Existing"));
        let request = ModalRequest::in_tree(content).presentation(ModalPresentation::NativeWindow);

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::InTree
        );
    }

    #[test]
    fn present_in_tree_modal_request_shows_centered_overlay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new_literal("Trigger"));
        let content = tree.add(Button::new_literal("Modal content"));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(tree.find_by_label("Modal content").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_builds_deferred_content() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new_literal("Trigger"));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| tree.add(Button::new_literal("Deferred modal")))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(tree.find_by_label("Deferred modal").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_moves_focus_into_modal() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new_literal("Trigger"));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new_literal("Continue")))
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

    impl fern_core::Widget for TwoButtonContent {
        fn build(&mut self, ctx: &mut fern_core::BuildContext) -> Vec<WidgetId> {
            let first = ctx.add(Button::new_literal("First"));
            let second = ctx.add(Button::new_literal("Second"));
            let row = ctx.add(
                fern_widgets::HStack::new()
                    .add_child(first)
                    .add_child(second),
            );
            self.root = Some(row);
            self.second = Some(second);
            vec![row]
        }

        fn layout_response(
            &self,
            proposal: fern_canvas::SizeProposal,
            ctx: &fern_core::LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new_literal("Trigger"));
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new_literal("Trigger"));
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
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new_literal("Trigger"));
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
