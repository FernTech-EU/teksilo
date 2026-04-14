use fern_canvas::SizeProposal;
use fern_core::app_command::{AppCommand, ErasedCommand};
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
    command_handler: Option<WindowCommandHandler>,
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
        command_handler: Option<WindowCommandHandler>,
        app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
        initial_window: WindowConfig,
        app_context_template: Option<std::rc::Rc<TreeAppContext>>,
        #[cfg(feature = "text")] typesetter: SharedTypesetter,
        i18n_watcher: Option<fern_i18n::FtlFileWatcher>,
    ) -> Self {
        let mut wm = WindowManager::new(theme);
        wm.set_theme_mode(theme_mode);
        if let Some(template) = app_context_template {
            wm.set_app_context_template(template);
        }

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
            _i18n_watcher: i18n_watcher,
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

            // The wgpu surface is Rgba8UnormSrgb: it expects linear-light color
            // values and applies sRGB encoding on write. Our Color stores sRGB-
            // encoded bytes (as designers specify them), so we must linearize
            // the clear color here the same way we do for vertex colors.
            let clear = fern_render::vertex::srgb_to_linear_rgba(
                managed.tree.theme().colors.surface_main.to_array(),
            );
            if let Err(e) = managed.platform_window.render_frame(&frame, clear) {
                eprintln!("fern-app: {e}, reconfiguring surface");
                managed.platform_window.reconfigure_surface();
                managed.platform_window.request_redraw();
                return;
            }
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
        match event {
            // Route AppEvent::Command through the normal command pipeline so
            // background-thread commands reach the on_command handler.
            AppEvent::Command(erased) => {
                if let Some(h) = self.command_handler.as_mut() {
                    let mut ctx = crate::command_context::CommandContext::new(
                        self.wm.primary_window_id(),
                        self.wm.theme().clone(),
                    );
                    h(&erased, &mut ctx);
                    if let Some(new_theme) = ctx.take_theme() {
                        self.wm.set_theme(new_theme);
                    }
                    for config in ctx.take_creates() {
                        self.wm.queue_create(config);
                    }
                    for close_id in ctx.take_closes() {
                        self.wm.queue_close(close_id);
                    }
                }
            }
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
                        let reloaded =
                            fern_i18n::thread_local::with_active(|mgr| {
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
                    Err(e) => eprintln!(
                        "fern-app: hot-reload event with invalid locale `{locale}`: {e}"
                    ),
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
    on_ready: Option<Box<dyn FnOnce(AppEventProxy)>>,
    initial_window: Option<WindowConfig>,
    window_title: String,
    window_width: u32,
    window_height: u32,
    root_builder: Option<Box<dyn FnOnce(&mut WidgetTree) -> WidgetId>>,
    custom_chrome: bool,
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
            on_ready: None,
            initial_window: None,
            window_title: "FernUI".to_string(),
            window_width: 800,
            window_height: 600,
            root_builder: None,
            custom_chrome: false,
            event_source: None,
            app_state_registry: HashMap::new(),
            i18n: None,
        }
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

    /// Register a callback that receives an `AppEventProxy` once the event loop is ready.
    /// Use this to hand the proxy to background threads that need to post commands.
    pub fn on_ready(mut self, handler: impl FnOnce(AppEventProxy) + 'static) -> Self {
        self.on_ready = Some(Box::new(handler));
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

    /// Opt the initial window into custom window chrome (no native title
    /// bar). The widget tree will be given a `PlatformTitleBarHost` that
    /// can be retrieved from inside the `root` closure via
    /// `tree.title_bar_host()`. Has no effect when using
    /// `initial_window(WindowConfig)` — set `WindowConfig::custom_chrome`
    /// directly in that case.
    pub fn custom_chrome(mut self, enabled: bool) -> Self {
        self.custom_chrome = enabled;
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

        if let Some(root_builder) = self.root_builder {
            root_builder(&mut tree);
        }

        HeadlessApp {
            tree,
            theme: self.theme,
            i18n_manager,
        }
    }

    /// Build and run the application with windowed rendering.
    pub fn run(self) {
        // Construct the i18n manager (if configured) and install it on the
        // thread-local before any window or widget tree is created. The
        // `WindowManager` will seed each new tree with the resolved locale
        // and direction at window-creation time.
        //
        // `runtime_override` entries are collected here so the hot-reload
        // watcher can be spun up after the winit event loop exists (we
        // need the `EventLoopProxy` as the sink target).
        let runtime_overrides: Vec<(LanguageIdentifier, std::path::PathBuf)> =
            self.i18n
                .as_ref()
                .map(|cfg| cfg.runtime_overrides().to_vec())
                .unwrap_or_default();

        if let Some(cfg) = self.i18n.as_ref() {
            let mgr = I18nManager::from_config(cfg);
            let initial_loc = I18nManager::resolve_initial_locale(cfg);
            mgr.set_locale(initial_loc);
            fern_i18n::thread_local::install(mgr);
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

        // If an event source OR an app-state registry is present, build
        // the per-tree app context that carries them. This single context
        // is shared with every window the WindowManager creates.
        let has_app_state = !self.app_state_registry.is_empty();
        let app_context_template = if self.event_source.is_some() || has_app_state {
            let base = match self.event_source {
                Some(adapter) => {
                    let poster: std::sync::Arc<dyn AppEventPoster> =
                        std::sync::Arc::new(WinitAppEventPoster {
                            proxy: proxy.clone(),
                        });
                    TreeAppContext::with_source_and_poster(adapter, poster)
                }
                None => TreeAppContext::empty(),
            };
            Some(std::rc::Rc::new(
                base.with_app_state(self.app_state_registry),
            ))
        } else {
            None
        };

        if let Some(on_ready) = self.on_ready {
            on_ready(proxy.clone());
        }

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
                .size(self.window_width, self.window_height)
                .custom_chrome(self.custom_chrome);
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
            app_context_template,
            #[cfg(feature = "text")]
            typesetter,
            i18n_watcher,
        );

        event_loop.run_app(&mut app).unwrap();
    }
}

impl Default for FernAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an `I18nManager` from `cfg`, install it on the thread-local, and
/// seed `tree` with the resolved initial locale and layout direction.
/// Returns the manager so the caller can hand it to `HeadlessApp` (or, in
/// `run()`, drop it because the thread-local owns it for the process).
fn install_i18n(tree: &mut WidgetTree, cfg: &I18nConfig) -> Rc<I18nManager> {
    let mgr = I18nManager::from_config(cfg);
    let initial_loc = I18nManager::resolve_initial_locale(cfg);
    mgr.set_locale(initial_loc.clone());
    fern_i18n::thread_local::install(mgr.clone());

    tree.set_locale(initial_loc.to_string());
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
            .root(|tree| tree.add(RectWidget::new().background(Color::RED)))
            .build_headless();
        let mut tree = app.tree;
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn app_state_flows_through_headless_builder() {
        use fern_canvas::Size;
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

            fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
                proposal.resolve(0.0, 0.0)
            }
        }

        let globals = Rc::new(AppGlobals {
            label: Signal::new("headless works".to_string()),
        });

        let observed = Signal::new(String::new());
        let observed_for_root = observed.clone();

        let _app = FernAppBuilder::new()
            .app_state(globals.clone())
            .root(move |tree| {
                tree.add(GlobalsReader {
                    observed: observed_for_root.clone(),
                })
            })
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
}
