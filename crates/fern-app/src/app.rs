use std::sync::Arc;

use fern_canvas::SizeProposal;
use fern_core::app_command::{AppCommand, ErasedCommand};
use fern_core::app_event::AppEvent;
use fern_core::event::WidgetEvent;
use fern_core::{WidgetId, WidgetTree};
use fern_platform::event_translation;
use fern_platform::PlatformWindow;
use fern_tokens::Theme;

#[cfg(feature = "text")]
use fern_text::SharedTypesetter;

use crate::command_context::CommandContext;
use crate::window_config::{FernWindowId, WindowConfig};
use crate::window_manager::WindowManager;

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
        let _ = self
            .inner
            .send_event(AppEvent::External(Box::new(payload)));
    }
}

/// Type for the window-aware command handler.
pub(crate) type WindowCommandHandler = Box<dyn FnMut(&ErasedCommand, &mut CommandContext)>;

/// Builder for a FernUI application.
pub struct FernAppBuilder {
    theme: Theme,
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

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
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
    pub fn root(
        mut self,
        builder: impl FnOnce(&mut WidgetTree) -> WidgetId + 'static,
    ) -> Self {
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
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        #[cfg(feature = "text")]
        let typesetter = self
            .typesetter
            .unwrap_or_else(SharedTypesetter::new_with_default_font);

        let mut wm = WindowManager::new(self.theme.clone());

        #[cfg(feature = "text")]
        {
            wm.set_typesetter(typesetter.clone());
        }

        // Build the initial window config
        let mut initial_config = if let Some(config) = self.initial_window {
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

        let mut command_handler = self.command_handler;
        let mut app_event_handler = self.app_event_handler;
        let mut initial_created = false;

        let idle_budget = std::time::Duration::from_millis(4);

        #[allow(deprecated)]
        event_loop
            .run(move |event, target| {
                use winit::event::{Event, WindowEvent};

                // Create the initial window on first Resumed event
                if !initial_created {
                    if matches!(event, Event::Resumed) {
                        // Take the initial config out — we need ownership
                        let config = std::mem::replace(
                            &mut initial_config,
                            WindowConfig::new(), // placeholder, won't be used again
                        );
                        wm.create_window(config, target);
                        initial_created = true;
                    }
                    return;
                }

                // Process any pending window creates/closes
                wm.process_pending(target);

                match event {
                    Event::Resumed => {
                        // Already handled above for initial creation.
                    }

                    Event::NewEvents(winit::event::StartCause::ResumeTimeReached { .. }) => {
                        // Timer fired (e.g., tooltip delay). Request redraw to process.
                        wm.request_redraw_all();
                    }

                    Event::UserEvent(ref app_event) => {
                        if let Some(handler) = &mut app_event_handler {
                            handler(app_event);
                        }
                        wm.request_redraw_all();
                    }

                    Event::WindowEvent {
                        window_id, event, ..
                    } => {
                        // Look up which FernWindowId this belongs to
                        let fern_id = wm.fern_id_for_winit(window_id);

                        // Block events if this window is modal-blocked
                        if let Some(fid) = fern_id {
                            if wm.is_blocked(fid) {
                                // Only allow close requests through on blocked windows
                                if !matches!(event, WindowEvent::CloseRequested) {
                                    return;
                                }
                            }
                        }

                        // Forward all window events to AccessKit adapter
                        if let Some(managed) = wm.get_by_winit_mut(window_id) {
                            managed.platform_window.process_accessibility_event(&event);

                            // Drain AccessKit action requests and dispatch as events
                            let actions = managed.platform_window.drain_accessibility_actions();
                            for req in actions {
                                let target_widget = fern_core::accessibility::node_id_to_widget_id(req.target_node);
                                managed.tree.dispatch_event(WidgetEvent::AccessAction {
                                    action: req.action,
                                    target: Some(target_widget),
                                });
                            }
                        }

                        match event {
                            WindowEvent::CloseRequested => {
                                if let Some(fid) = fern_id {
                                    wm.close_window(fid);
                                }
                                if wm.is_empty() {
                                    target.exit();
                                }
                            }
                            WindowEvent::Resized(new_size) => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    managed.platform_window.resize(new_size);
                                    managed.platform_window.request_redraw();
                                }
                            }
                            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    managed.translation_state.set_scale_factor(scale_factor);
                                    managed.platform_window.set_scale_factor(scale_factor);
                                }
                                #[cfg(feature = "text")]
                                {
                                    typesetter.set_scale_factor(scale_factor as f32);
                                }
                            }
                            WindowEvent::CursorMoved { position, .. } => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    if let Some(evt) = event_translation::translate_cursor_moved(
                                        position.x,
                                        position.y,
                                        &mut managed.translation_state,
                                    ) {
                                        managed.tree.dispatch_event(evt);
                                    }
                                    // Only redraw if the event changed something (hover state, etc.)
                                    if managed.tree.needs_redraw() {
                                        managed.platform_window.request_redraw();
                                    }
                                }
                            }
                            WindowEvent::MouseInput { state, button, .. } => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    if let Some(evt) = event_translation::translate_mouse_input(
                                        state,
                                        button,
                                        &managed.translation_state,
                                    ) {
                                        managed.tree.dispatch_event(evt);
                                    }
                                    managed.platform_window.request_redraw();
                                }
                            }
                            WindowEvent::MouseWheel { delta, phase, .. } => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    if let Some(evt) = event_translation::translate_mouse_wheel(
                                        delta,
                                        phase,
                                        &managed.translation_state,
                                    ) {
                                        managed.tree.dispatch_event(evt);
                                    }
                                    managed.platform_window.request_redraw();
                                }
                            }
                            WindowEvent::ModifiersChanged(mods) => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    managed.current_modifiers = mods.state();
                                }
                            }
                            WindowEvent::KeyboardInput {
                                event: key_event, ..
                            } => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    if let Some(key) = event_translation::translate_key(
                                        &key_event.logical_key,
                                    ) {
                                        let modifiers = event_translation::translate_modifiers(
                                            managed.current_modifiers,
                                        );
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
                                                managed.tree.dispatch_event(WidgetEvent::KeyUp {
                                                    key,
                                                    modifiers,
                                                });
                                            }
                                        }
                                    }
                                    managed.platform_window.request_redraw();
                                }
                            }
                            WindowEvent::RedrawRequested => {
                                if let Some(managed) = wm.get_by_winit_mut(window_id) {
                                    // Run idle callbacks
                                    if managed.tree.has_idle_work() {
                                        managed.tree.run_idle_callbacks(idle_budget);
                                    }

                                    let size = managed.platform_window.surface_size();
                                    let sf = managed.platform_window.scale_factor() as f32;
                                    let proposal = SizeProposal::exact(
                                        size.0 as f32 / sf,
                                        size.1 as f32 / sf,
                                    );

                                    managed.tree.layout(proposal);

                                    let a11y_update = managed.tree.sync_accessibility();
                                    managed.platform_window.update_accessibility(a11y_update);

                                    let mut frame = managed.tree.render();

                                    #[cfg(feature = "text")]
                                    {
                                        let atlas =
                                            typesetter.bridge().borrow_mut().atlas_info();
                                        if atlas.dirty && atlas.width > 0 && atlas.height > 0 {
                                            managed
                                                .platform_window
                                                .renderer_mut()
                                                .upload_atlas(
                                                    atlas.width,
                                                    atlas.height,
                                                    &atlas.pixels,
                                                );
                                        }
                                        // Glyph eviction freed atlas space that may be reused
                                        // by future allocations. Invalidate all paint caches so
                                        // widgets repaint with fresh glyph data.
                                        if atlas.glyphs_evicted {
                                            managed.tree.invalidate_all_paints();
                                            frame = managed.tree.render();
                                            // Re-upload atlas: the repaint may have rasterized
                                            // new glyphs into the freed atlas space.
                                            let atlas2 =
                                                typesetter.bridge().borrow_mut().atlas_info();
                                            if atlas2.dirty && atlas2.width > 0 && atlas2.height > 0
                                            {
                                                managed
                                                    .platform_window
                                                    .renderer_mut()
                                                    .upload_atlas(
                                                        atlas2.width,
                                                        atlas2.height,
                                                        &atlas2.pixels,
                                                    );
                                            }
                                        }
                                    }

                                    let clear = managed.tree.theme().colors.surface.to_array();
                                    let _ = managed.platform_window.render_frame(&frame, clear);
                                }
                            }
                            _ => {}
                        }

                        // Route widget-emitted commands through the app-level handler
                        // with a window-aware CommandContext.
                        let had_commands = wm.flush_commands_through(&mut command_handler);
                        wm.process_pending(target);
                        // Only request redraw if commands were processed (theme change, etc.)
                        if had_commands {
                            wm.request_redraw_all();
                        }

                        if wm.is_empty() {
                            target.exit();
                        }
                    }
                    _ => {}
                }

                // Update control flow for pending timers (tooltips).
                // Check all windows for the earliest pending deadline.
                let mut earliest_deadline: Option<std::time::Instant> = None;
                for managed in wm.iter() {
                    if let Some(deadline) = managed.tree.next_timer_deadline() {
                        earliest_deadline = Some(match earliest_deadline {
                            Some(current) => current.min(deadline),
                            None => deadline,
                        });
                    }
                }
                if let Some(deadline) = earliest_deadline {
                    target.set_control_flow(
                        winit::event_loop::ControlFlow::WaitUntil(deadline),
                    );
                } else {
                    target.set_control_flow(winit::event_loop::ControlFlow::Wait);
                }
            })
            .unwrap();
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
}
