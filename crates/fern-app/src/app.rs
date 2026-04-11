use fern_canvas::SizeProposal;
use fern_core::app_command::{AppCommand, ErasedCommand};
use fern_core::app_event::AppEvent;
use fern_core::event::WidgetEvent;
use fern_core::modal::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
use fern_core::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::{WidgetId, WidgetTree};
use fern_platform::event_translation;
use fern_tokens::{ColorTokens, Theme};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
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

use crate::command_context::CommandContext;
use crate::window_config::WindowConfig;
use crate::window_manager::WindowManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedModalPresentation {
    InTree,
    NativeWindow,
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
        ModalCloseBehavior::ClickOutside | ModalCloseBehavior::EscapeOrClickOutside => {
            DismissBehavior::ClickOutside
        }
        ModalCloseBehavior::EscapeKey | ModalCloseBehavior::Manual => DismissBehavior::Manual,
    }
}

fn present_in_tree_modal_request(
    tree: &mut WidgetTree,
    source_widget: WidgetId,
    request: ModalRequest,
) {
    let dismiss = modal_close_behavior_to_overlay_dismiss(request.close_behavior);
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
        },
    );

    if let Some(focus_target) = tree.first_focusable_descendant(content_id) {
        tree.focus(focus_target);
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
    command_handler: Option<WindowCommandHandler>,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    initial_window: Option<WindowConfig>,
    initial_created: bool,
    idle_budget: Duration,
    idle_trace: Option<IdleTrace>,
    theme_mode: ThemeMode,
    #[cfg(feature = "text")]
    typesetter: SharedTypesetter,
}

impl FernAppHandler {
    fn new(
        theme: Theme,
        theme_mode: ThemeMode,
        command_handler: Option<WindowCommandHandler>,
        app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
        initial_window: WindowConfig,
        #[cfg(feature = "text")] typesetter: SharedTypesetter,
    ) -> Self {
        let mut wm = WindowManager::new(theme);
        wm.set_theme_mode(theme_mode);

        #[cfg(feature = "text")]
        {
            wm.set_typesetter(typesetter.clone());
        }

        Self {
            wm,
            command_handler,
            app_event_handler,
            initial_window: Some(initial_window),
            initial_created: false,
            idle_budget: Duration::from_millis(4),
            idle_trace: IdleTrace::from_env(),
            theme_mode,
            #[cfg(feature = "text")]
            typesetter,
        }
    }

    fn process_pending(&mut self, event_loop: &ActiveEventLoop) {
        self.wm.process_pending(event_loop);
    }

    fn flush_commands(&mut self) -> bool {
        self.wm.flush_commands_through(&mut self.command_handler)
    }

    fn process_modal_requests(&mut self) -> bool {
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
                            ..
                        } = queued.request;

                        let ModalContent::Deferred(builder) = content else {
                            continue;
                        };

                        let mut config = WindowConfig::new().modal(true).parent(source_window);
                        if let Some(title) = title {
                            config = config.title(title);
                        }
                        if let Some((width, height)) = size {
                            config = config.size(width, height);
                        }
                        self.wm.queue_create(config.root(builder));
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
            if let Some(deadline) = managed.tree.next_timer_deadline() {
                earliest_deadline = Some(match earliest_deadline {
                    Some(current) => current.min(deadline),
                    None => deadline,
                });
            }
        }

        if let Some(deadline) = earliest_deadline {
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
        let had_commands = self.flush_commands();
        let had_modal_requests = self.process_modal_requests();
        let had_modal_dismissals = self.process_modal_dismissals();
        self.process_pending(event_loop);
        if had_commands || had_modal_requests || had_modal_dismissals {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_request_redraw_all();
            }
            self.wm.request_redraw_all();
        }
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }

    fn handle_accessibility_actions(&mut self, window_id: WindowId, event: &WindowEvent) {
        if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
            managed.platform_window.process_accessibility_event(event);

            let actions = managed.platform_window.drain_accessibility_actions();
            for req in actions {
                let target_widget = fern_core::accessibility::node_id_to_widget_id(req.target_node);
                managed.tree.dispatch_event(WidgetEvent::AccessAction {
                    action: req.action,
                    target: Some(target_widget),
                });
            }
        }
    }

    fn handle_redraw_requested(&mut self, window_id: WindowId) {
        if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_redraw_requested();
            }
            if managed.tree.has_idle_work() {
                if let Some(trace) = &mut self.idle_trace {
                    trace.note_idle_callbacks_run();
                }
                managed.tree.run_idle_callbacks(self.idle_budget);
            }

            let size = managed.platform_window.surface_size();
            let sf = managed.platform_window.scale_factor() as f32;
            let proposal = SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf);

            managed.tree.layout(proposal);

            let a11y_update = managed.tree.sync_accessibility();
            managed.platform_window.update_accessibility(a11y_update);

            let mut frame = managed.tree.render();

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
                    frame = managed.tree.render();
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

            let clear = managed.tree.theme().colors.surface.to_array();
            let _ = managed.platform_window.render_frame(&frame, clear);
            if let Some(trace) = &mut self.idle_trace {
                trace.note_rendered_frame();
            }
        }
    }

    fn handle_window_event_inner(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let fern_id = self.wm.fern_id_for_winit(window_id);

        if let Some(fid) = fern_id {
            if self.wm.is_blocked(fid) && !matches!(event, WindowEvent::CloseRequested) {
                self.wm.refocus_modal_child(fid);
                self.update_control_flow(event_loop);
                return;
            }
        }

        self.handle_accessibility_actions(window_id, &event);

        match event {
            WindowEvent::CloseRequested => {
                if let Some(fid) = fern_id {
                    self.wm.close_window(fid);
                }
            }
            WindowEvent::Resized(new_size) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.platform_window.resize(new_size);
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("resize");
                    }
                    managed.platform_window.request_redraw();
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
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(evt) = event_translation::translate_cursor_moved(
                        position.x,
                        position.y,
                        &mut managed.translation_state,
                    ) {
                        managed.tree.dispatch_event(evt);
                    }
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
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(evt) = event_translation::translate_mouse_input(
                        state,
                        button,
                        &managed.translation_state,
                    ) {
                        managed.tree.dispatch_event(evt);
                    }
                    apply_cursor_to_window(&managed.platform_window, managed.tree.current_cursor());
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("mouse_input");
                    }
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(evt) = event_translation::translate_mouse_wheel(
                        delta,
                        phase,
                        &managed.translation_state,
                    ) {
                        managed.tree.dispatch_event(evt);
                    }
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("mouse_wheel");
                    }
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.current_modifiers = mods.state();
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(key) = event_translation::translate_key(&key_event.logical_key) {
                        let modifiers =
                            event_translation::translate_modifiers(managed.current_modifiers);
                        let text = key_event.text.as_ref().map(|t| t.to_string());
                        match key_event.state {
                            winit::event::ElementState::Pressed => {
                                managed.tree.dispatch_event(WidgetEvent::KeyDown {
                                    key,
                                    modifiers,
                                    text,
                                });
                            }
                            winit::event::ElementState::Released => {
                                managed
                                    .tree
                                    .dispatch_event(WidgetEvent::KeyUp { key, modifiers });
                            }
                        }
                    }
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("keyboard");
                    }
                    managed.platform_window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw_requested(window_id);
            }
            WindowEvent::ThemeChanged(winit_theme) => {
                self.handle_theme_changed(winit_theme);
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.platform_window.request_redraw();
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
        if !self.initial_created {
            if let Some(config) = self.initial_window.take() {
                self.wm.create_window(config, event_loop);
                self.initial_created = true;
            }
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

/// A thread-safe handle for posting `AppEvent`s to the UI thread.
///
/// Clone and send to background threads. The event loop wakes up
/// and processes the event like any other input.
#[derive(Clone)]
pub struct AppEventProxy {
    inner: winit::event_loop::EventLoopProxy<AppEvent>,
}

impl AppEventProxy {
    /// Post a typed command to the UI thread.
    pub fn send_command<C: AppCommand>(&self, cmd: C) {
        let _ = self
            .inner
            .send_event(AppEvent::Command(ErasedCommand::new(cmd)));
    }

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
}

/// Type for the window-aware command handler.
pub(crate) type WindowCommandHandler = Box<dyn FnMut(&ErasedCommand, &mut CommandContext)>;

/// Builder for a FernUI application.
pub struct FernAppBuilder {
    theme: Theme,
    theme_mode: ThemeMode,
    #[cfg(feature = "text")]
    typesetter: Option<SharedTypesetter>,
    command_handler: Option<WindowCommandHandler>,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    initial_window: Option<WindowConfig>,
    window_title: String,
    window_width: u32,
    window_height: u32,
    root_builder: Option<Box<dyn FnOnce(&mut WidgetTree) -> WidgetId>>,
}

impl FernAppBuilder {
    pub fn new() -> Self {
        Self {
            theme: Theme::light_default(),
            theme_mode: ThemeMode::Manual,
            #[cfg(feature = "text")]
            typesetter: None,
            command_handler: None,
            app_event_handler: None,
            initial_window: None,
            window_title: "FernUI".to_string(),
            window_width: 800,
            window_height: 600,
            root_builder: None,
        }
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

    /// Register a window-aware command handler.
    /// The handler receives the command and a `CommandContext` that identifies
    /// the source window and allows creating/closing windows.
    pub fn on_command<C: AppCommand>(
        mut self,
        mut handler: impl FnMut(&C, &mut CommandContext) + 'static,
    ) -> Self {
        self.command_handler = Some(Box::new(move |erased: &ErasedCommand, ctx| {
            if let Some(cmd) = erased.downcast_ref::<C>() {
                handler(cmd, ctx);
            }
        }));
        self
    }

    /// Register a handler for `AppEvent`s received from background threads.
    pub fn on_app_event(mut self, handler: impl FnMut(&AppEvent) + 'static) -> Self {
        self.app_event_handler = Some(Box::new(handler));
        self
    }

    /// Set the root widget builder for the initial window (convenience API).
    /// For multi-window apps, use `initial_window()` with a `WindowConfig`.
    pub fn root(mut self, builder: impl FnOnce(&mut WidgetTree) -> WidgetId + 'static) -> Self {
        self.root_builder = Some(Box::new(builder));
        self
    }

    /// Configure the initial window explicitly (for multi-window apps).
    pub fn initial_window(mut self, config: WindowConfig) -> Self {
        self.initial_window = Some(config);
        self
    }

    pub fn window_title(mut self, title: impl Into<String>) -> Self {
        self.window_title = title.into();
        self
    }

    pub fn window_size(mut self, width: u32, height: u32) -> Self {
        self.window_width = width;
        self.window_height = height;
        self
    }

    /// Build a headless app for testing (no window, no GPU).
    pub fn build_headless(self) -> HeadlessApp {
        let mut tree = WidgetTree::new().with_theme(self.theme.clone());

        #[cfg(feature = "text")]
        {
            let typesetter = self
                .typesetter
                .unwrap_or_else(SharedTypesetter::new_with_default_font);
            tree = tree.with_text_backend(typesetter.as_text_backend());
        }

        if let Some(root_builder) = self.root_builder {
            root_builder(&mut tree);
        }

        HeadlessApp {
            tree,
            theme: self.theme,
        }
    }

    /// Build and run the application with windowed rendering.
    pub fn run(self) {
        let event_loop = winit::event_loop::EventLoop::<AppEvent>::with_user_event()
            .build()
            .unwrap();
        event_loop.set_control_flow(ControlFlow::Wait);

        #[cfg(feature = "text")]
        let typesetter = self
            .typesetter
            .unwrap_or_else(SharedTypesetter::new_with_default_font);

        // Build the initial window config
        let initial_config = if let Some(config) = self.initial_window {
            config
        } else {
            let mut config = WindowConfig::new()
                .title(self.window_title)
                .size(self.window_width, self.window_height);
            if let Some(root_builder) = self.root_builder {
                config = config.root(root_builder);
            }
            config
        };

        let mut app = FernAppHandler::new(
            self.theme,
            self.theme_mode,
            self.command_handler,
            self.app_event_handler,
            initial_config,
            #[cfg(feature = "text")]
            typesetter,
        );

        event_loop.run_app(&mut app).unwrap();
    }
}

impl Default for FernAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A headless app for testing (no window, no GPU).
pub struct HeadlessApp {
    pub tree: WidgetTree,
    pub theme: Theme,
}

impl HeadlessApp {
    pub fn theme(&self) -> &Theme {
        &self.theme
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
        assert_ne!(app.theme().colors.primary, Color::TRANSPARENT);
    }

    #[test]
    fn builder_with_root() {
        use fern_widgets::RectWidget;
        let app = FernAppBuilder::new()
            .root(|tree| tree.add(RectWidget::new().background(Color::RED)))
            .build_headless();
        let mut tree = app.tree;
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn auto_prefers_native_for_deferred_content_when_supported() {
        let request = ModalRequest::deferred(|tree| tree.add(Button::new("Deferred")));

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::NativeWindow
        );
    }

    #[test]
    fn existing_widget_forces_in_tree_even_if_native_requested() {
        let mut tree = WidgetTree::new();
        let content = tree.add(Button::new("Existing"));
        let request = ModalRequest::in_tree(content).presentation(ModalPresentation::NativeWindow);

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::InTree
        );
    }

    #[test]
    fn present_in_tree_modal_request_shows_centered_overlay() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new("Trigger"));
        let content = tree.add(Button::new("Modal content"));
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
        let source = tree.add(Button::new("Trigger"));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| tree.add(Button::new("Deferred modal")))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert_eq!(tree.active_overlays().len(), 1);
        assert!(tree.find_by_label("Deferred modal").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_moves_focus_into_modal() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let source = tree.add(Button::new("Trigger"));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new("Continue")))
            })
            .presentation(ModalPresentation::InTree),
        );

        let continue_button = tree.find_by_label("Continue").unwrap();
        assert_eq!(tree.focused(), Some(continue_button));
    }
}
