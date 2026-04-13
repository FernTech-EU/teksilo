//! Multi-window management.
//!
//! The `WindowManager` maintains a collection of active windows, each with its
//! own `WidgetTree`, `PlatformWindow`, and translation state. It routes events
//! by winit `WindowId`, broadcasts environment changes to all windows, and
//! handles modal dialog blocking.

use std::collections::HashMap;
use std::rc::Rc;

use fern_core::event_source::TreeAppContext;
use fern_core::{PlatformTitleBarHost, WidgetTree};
use fern_platform::AccessibilityPreferences;
use fern_platform::PlatformWindow;
use fern_platform::create_title_bar_host;
use fern_platform::event_translation::TranslationState;
use fern_tokens::{ColorTokens, Theme};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::UserAttentionType;
use winit::window::WindowLevel;

use crate::app::ThemeMode;

use crate::window_config::{FernWindowId, WindowConfig};

/// Per-window state managed by the WindowManager.
pub(crate) struct ManagedWindow {
    pub fern_id: FernWindowId,
    pub string_id: Option<String>,
    pub tree: WidgetTree,
    pub platform_window: PlatformWindow,
    pub translation_state: TranslationState,
    pub current_modifiers: winit::keyboard::ModifiersState,
    pub modal: bool,
    pub parent: Option<FernWindowId>,
    /// Custom-chrome host, if the window opted in via
    /// `WindowConfig::custom_chrome(true)` and the platform supports it.
    /// The same `Rc` is also stored on the `WidgetTree` so the root-builder
    /// closure can hand it to a `TitleBar` widget.
    pub title_bar_host: Option<Rc<dyn PlatformTitleBarHost>>,
}

/// Manages multiple application windows.
///
/// Each window owns its own `WidgetTree` and `PlatformWindow`. The manager
/// routes events by winit `WindowId`, broadcasts environment changes, and
/// handles modal dialog blocking.
pub struct WindowManager {
    windows: HashMap<winit::window::WindowId, ManagedWindow>,
    fern_to_winit: HashMap<FernWindowId, winit::window::WindowId>,
    next_id: u64,
    theme: Theme,
    #[cfg(feature = "text")]
    typesetter: Option<fern_text::SharedTypesetter>,
    /// Windows that are blocked by a modal child.
    modal_blocked: HashMap<FernWindowId, FernWindowId>,
    /// Windows pending creation (deferred to event loop).
    pending_creates: Vec<WindowConfig>,
    /// Windows pending closure.
    pending_closes: Vec<FernWindowId>,
    /// OS-level accessibility preferences, queried once at startup.
    a11y_prefs: AccessibilityPreferences,
    /// How the app resolves its theme (Manual, FollowSystem, Native).
    theme_mode: ThemeMode,
    /// Per-tree app context shared with every window's WidgetTree when an
    /// event source is registered on the FernAppBuilder. Each window
    /// receives a clone of this Rc so subscriptions land in a single
    /// shared `subscription_callbacks` map.
    app_context_template: Option<Rc<TreeAppContext>>,
}

impl WindowManager {
    pub fn new(theme: Theme) -> Self {
        let a11y_prefs = AccessibilityPreferences::query();
        Self {
            windows: HashMap::new(),
            fern_to_winit: HashMap::new(),
            next_id: 1,
            theme,
            #[cfg(feature = "text")]
            typesetter: None,
            modal_blocked: HashMap::new(),
            pending_creates: Vec::new(),
            pending_closes: Vec::new(),
            a11y_prefs,
            theme_mode: ThemeMode::Manual,
            app_context_template: None,
        }
    }

    /// Set the theme mode (called by FernAppHandler during initialization).
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
    }

    /// Install the per-tree app context template that every newly created
    /// window's WidgetTree should adopt. Called by FernAppHandler when the
    /// application registered an event source on the builder.
    pub fn set_app_context_template(&mut self, template: Rc<TreeAppContext>) {
        self.app_context_template = Some(template);
    }

    /// The shared per-tree app context, if any, used by FernAppHandler to
    /// look up subscription callbacks when delivering
    /// `AppEvent::SubscriptionEvent` to the UI thread.
    pub(crate) fn app_context_template(&self) -> Option<&Rc<TreeAppContext>> {
        self.app_context_template.as_ref()
    }

    /// Get the current theme mode.
    pub fn theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }

    #[cfg(feature = "text")]
    pub fn set_typesetter(&mut self, typesetter: fern_text::SharedTypesetter) {
        self.typesetter = Some(typesetter);
    }

    fn alloc_id(&mut self) -> FernWindowId {
        let id = FernWindowId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Create a window immediately (called from the event loop with access to `target`).
    pub fn create_window(
        &mut self,
        config: WindowConfig,
        target: &winit::event_loop::ActiveEventLoop,
    ) -> FernWindowId {
        let fern_id = self.alloc_id();

        let mut window_attrs = winit::window::Window::default_attributes()
            .with_title(&config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(config.width, config.height))
            .with_visible(false); // Must be invisible for AccessKit adapter creation

        // When the application opts into custom chrome, suppress the
        // server-side decorations on platforms where they're entirely
        // client-drawn (Wayland). On Windows we keep `with_decorations(true)`
        // because the M4 recipe relies on the native frame still being present
        // (DwmExtendFrameIntoClientArea + WM_NCCALCSIZE), and on macOS the
        // M3 recipe sets the relevant attributes via `WindowAttributesExtMacOS`
        // — neither needs the toggle here.
        #[cfg(all(unix, not(target_os = "macos")))]
        if config.custom_chrome
            && fern_platform::active_window_system() == fern_platform::WindowSystem::Wayland
        {
            window_attrs = window_attrs.with_decorations(false);
        }

        if config.modal {
            window_attrs = window_attrs.with_window_level(WindowLevel::AlwaysOnTop);

            if let Some(parent_id) = config.parent
                && let Some(parent_winit) = self.winit_id_for_fern(parent_id)
                && let Some(parent_managed) = self.windows.get(&parent_winit)
                && let Ok(parent_handle) = parent_managed.platform_window.window().window_handle()
            {
                // Safe: the parent window is managed by the WindowManager and remains
                // alive for the lifetime of the modal child.
                window_attrs =
                    unsafe { window_attrs.with_parent_window(Some(parent_handle.as_raw())) };
            }
        }

        let window = target.create_window(window_attrs).unwrap();
        let winit_id = window.id();
        let scale_factor = window.scale_factor();

        let mut translation_state = TranslationState::new();
        translation_state.set_scale_factor(scale_factor);

        // Resolve the initial theme from ThemeMode before building the tree
        let initial_theme = match self.theme_mode {
            ThemeMode::Manual => self.theme.clone(),
            ThemeMode::FollowSystem => match window.theme() {
                Some(winit::window::Theme::Dark) => Theme::dark_default(),
                _ => Theme::light_default(),
            },
            ThemeMode::Native => {
                let os = fern_platform::os_theme::query_os_theme_colors();
                let base = if os.color_scheme.is_dark() {
                    Theme::dark_default()
                } else {
                    Theme::light_default()
                };
                Theme {
                    colors: ColorTokens::from_os_colors(&os),
                    ..base
                }
            }
        };
        // Update the shared theme so all subsequent windows use the same base
        if self.theme_mode != ThemeMode::Manual {
            self.theme = initial_theme.clone();
        }

        // Create with AccessKit adapter (shows window after adapter is ready)
        let pw = pollster::block_on(PlatformWindow::new_with_a11y(window, target));

        if config.modal {
            pw.window().set_window_level(WindowLevel::AlwaysOnTop);
            pw.window().focus_window();
        }

        // Construct the platform title bar host if custom chrome was
        // requested. On unsupported platforms (X11, no host backend) the
        // factory logs a warning and returns `Unsupported`; we silently
        // continue with native decorations and leave the host slot empty.
        let title_bar_host: Option<Rc<dyn PlatformTitleBarHost>> = if config.custom_chrome {
            match create_title_bar_host(pw.window_arc()) {
                Ok(host) => Some(host),
                Err(_) => None,
            }
        } else {
            None
        };

        let mut tree = WidgetTree::new().with_theme(initial_theme);
        if let Some(template) = self.app_context_template.as_ref() {
            tree.set_app_context(template.clone());
        }
        if let Some(ref host) = title_bar_host {
            tree.set_title_bar_host(host.clone());
        }
        tree.set_accessibility_preferences(
            self.a11y_prefs.high_contrast,
            self.a11y_prefs.reduced_motion,
            self.a11y_prefs.text_scale_factor,
        );

        #[cfg(feature = "text")]
        {
            if let Some(ref typesetter) = self.typesetter {
                typesetter.set_scale_factor(scale_factor as f32);
                tree = tree.with_text_backend(typesetter.as_text_backend());
            }
        }

        if let Some(root_builder) = config.root_builder {
            let root_id = root_builder(&mut tree);
            if config.modal
                && let Some(focus_target) = tree.first_focusable_descendant(root_id)
            {
                tree.focus(focus_target);
            }
        }

        // Handle modal blocking
        if config.modal {
            if let Some(parent_id) = config.parent {
                self.modal_blocked.insert(parent_id, fern_id);
            }
        }

        let managed = ManagedWindow {
            fern_id,
            string_id: config.string_id,
            tree,
            platform_window: pw,
            translation_state,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            modal: config.modal,
            parent: config.parent,
            title_bar_host,
        };

        self.windows.insert(winit_id, managed);
        self.fern_to_winit.insert(fern_id, winit_id);

        fern_id
    }

    /// Close a window by its FernWindowId.
    pub fn close_window(&mut self, fern_id: FernWindowId) {
        if let Some(winit_id) = self.fern_to_winit.remove(&fern_id) {
            if let Some(managed) = self.windows.remove(&winit_id) {
                // Unblock parent if this was a modal
                if managed.modal {
                    if let Some(parent_id) = managed.parent {
                        self.modal_blocked.remove(&parent_id);
                    }
                }
            }
        }
        // Also remove any modal children blocking this window
        self.modal_blocked.remove(&fern_id);
    }

    /// Queue a window creation (processed in the next event loop tick).
    pub fn queue_create(&mut self, config: WindowConfig) {
        self.pending_creates.push(config);
    }

    /// Queue a window closure (processed in the next event loop tick).
    pub fn queue_close(&mut self, fern_id: FernWindowId) {
        self.pending_closes.push(fern_id);
    }

    /// Process pending creates and closes. Called from the event loop.
    pub fn process_pending(&mut self, target: &winit::event_loop::ActiveEventLoop) {
        let creates: Vec<_> = self.pending_creates.drain(..).collect();
        for config in creates {
            self.create_window(config, target);
        }
        let closes: Vec<_> = self.pending_closes.drain(..).collect();
        for fern_id in closes {
            self.close_window(fern_id);
        }
    }

    /// Get a mutable ManagedWindow for a winit WindowId.
    pub(crate) fn get_by_winit_mut(
        &mut self,
        id: winit::window::WindowId,
    ) -> Option<&mut ManagedWindow> {
        self.windows.get_mut(&id)
    }

    pub(crate) fn get_by_fern_mut(&mut self, id: FernWindowId) -> Option<&mut ManagedWindow> {
        let winit_id = self.fern_to_winit.get(&id).copied()?;
        self.windows.get_mut(&winit_id)
    }

    /// Get the FernWindowId for a winit WindowId.
    pub fn fern_id_for_winit(&self, id: winit::window::WindowId) -> Option<FernWindowId> {
        self.windows.get(&id).map(|w| w.fern_id)
    }

    /// Find a window by its string ID.
    pub fn find_window(&self, string_id: &str) -> Option<FernWindowId> {
        self.windows
            .values()
            .find(|w| w.string_id.as_deref() == Some(string_id))
            .map(|w| w.fern_id)
    }

    /// Whether a window is blocked by a modal child.
    pub fn is_blocked(&self, fern_id: FernWindowId) -> bool {
        self.modal_blocked.contains_key(&fern_id)
    }

    pub fn blocking_modal_child(&self, fern_id: FernWindowId) -> Option<FernWindowId> {
        self.modal_blocked.get(&fern_id).copied()
    }

    pub fn refocus_modal_child(&self, blocked_parent: FernWindowId) {
        let Some(child_id) = self.blocking_modal_child(blocked_parent) else {
            return;
        };
        let Some(child_winit) = self.winit_id_for_fern(child_id) else {
            return;
        };
        let Some(child) = self.windows.get(&child_winit) else {
            return;
        };

        child
            .platform_window
            .window()
            .set_window_level(WindowLevel::AlwaysOnTop);
        child.platform_window.window().focus_window();
        child
            .platform_window
            .window()
            .request_user_attention(Some(UserAttentionType::Informational));
        child.platform_window.request_redraw();
    }

    /// Broadcast a theme change to all windows.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme.clone();
        for managed in self.windows.values_mut() {
            managed.tree.set_theme(theme.clone());
        }
    }

    /// Get the current shared theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Get the OS-level accessibility preferences (queried at startup).
    pub fn accessibility_preferences(&self) -> &AccessibilityPreferences {
        &self.a11y_prefs
    }

    /// Get the FernWindowId of the first (primary) window.
    /// Falls back to a synthetic ID when no windows are open yet.
    pub fn primary_window_id(&self) -> FernWindowId {
        self.fern_to_winit
            .keys()
            .copied()
            .min_by_key(|id| id.raw())
            .unwrap_or(FernWindowId::new(0))
    }

    /// Number of active windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Whether no windows remain (app should exit).
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Iterate over all managed windows.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ManagedWindow> {
        self.windows.values()
    }

    /// Get the winit WindowId for a FernWindowId.
    pub fn winit_id_for_fern(&self, fern_id: FernWindowId) -> Option<winit::window::WindowId> {
        self.fern_to_winit.get(&fern_id).copied()
    }

    /// Get the platform title bar host for a window, if the window opted
    /// into custom chrome via `WindowConfig::custom_chrome(true)` and the
    /// platform supports it. Returns `None` for windows that use native
    /// decorations or run on a window system without custom chrome support
    /// (currently X11).
    pub fn title_bar_host(
        &self,
        fern_id: FernWindowId,
    ) -> Option<Rc<dyn PlatformTitleBarHost>> {
        let winit_id = self.fern_to_winit.get(&fern_id).copied()?;
        self.windows
            .get(&winit_id)
            .and_then(|w| w.title_bar_host.clone())
    }

    /// Request redraw on all windows.
    pub fn request_redraw_all(&self) {
        for managed in self.windows.values() {
            managed.platform_window.request_redraw();
        }
    }

    /// Drain pending modal requests from all windows.
    pub fn drain_pending_modal_requests(
        &mut self,
    ) -> Vec<(FernWindowId, Vec<fern_core::QueuedModalRequest>)> {
        let mut all_requests = Vec::new();
        for managed in self.windows.values_mut() {
            let requests = managed.tree.drain_pending_modal_requests();
            if !requests.is_empty() {
                all_requests.push((managed.fern_id, requests));
            }
        }
        all_requests
    }

    /// Drain native modal-window dismiss requests from all windows.
    pub fn drain_pending_modal_dismissals(&mut self) -> Vec<FernWindowId> {
        let mut windows_to_close = Vec::new();
        for managed in self.windows.values_mut() {
            if managed.tree.drain_pending_modal_dismissal() && managed.modal {
                windows_to_close.push(managed.fern_id);
            }
        }
        windows_to_close
    }

    /// Drain pending commands from all windows and route through the handler
    /// with a window-aware `CommandContext`. Returns true if any commands were processed.
    pub fn flush_commands_through(
        &mut self,
        handler: &mut Option<super::app::WindowCommandHandler>,
    ) -> bool {
        // Collect (fern_id, commands) pairs to avoid borrow issues
        let mut all_cmds: Vec<(FernWindowId, Vec<fern_core::app_command::ErasedCommand>)> =
            Vec::new();
        for managed in self.windows.values_mut() {
            let cmds = managed.tree.drain_pending_commands();
            if !cmds.is_empty() {
                all_cmds.push((managed.fern_id, cmds));
            }
        }

        let had_commands = !all_cmds.is_empty();

        for (fern_id, cmds) in all_cmds {
            if let Some(h) = handler.as_mut() {
                let mut ctx =
                    crate::command_context::CommandContext::new(fern_id, self.theme.clone());
                for cmd in &cmds {
                    h(cmd, &mut ctx);
                }
                // Apply deferred operations
                if let Some(new_theme) = ctx.take_theme() {
                    self.set_theme(new_theme);
                }
                for config in ctx.take_creates() {
                    self.queue_create(config);
                }
                for close_id in ctx.take_closes() {
                    self.queue_close(close_id);
                }
            }
        }

        had_commands
    }
}
