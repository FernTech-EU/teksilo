//! Multi-window management.
//!
//! The `WindowManager` maintains a collection of active windows, each with its
//! own `WidgetTree`, `PlatformWindow`, and translation state. It routes events
//! by winit `WindowId`, broadcasts environment changes to all windows, and
//! handles modal dialog blocking.

use std::collections::HashMap;

use fern_core::WidgetTree;
use fern_platform::AccessibilityPreferences;
use fern_platform::PlatformWindow;
use fern_platform::event_translation::TranslationState;
use fern_tokens::Theme;

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
        }
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

        let window_attrs = winit::window::Window::default_attributes()
            .with_title(&config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(config.width, config.height))
            .with_visible(false); // Must be invisible for AccessKit adapter creation

        let window = target.create_window(window_attrs).unwrap();
        let winit_id = window.id();
        let scale_factor = window.scale_factor();

        let mut translation_state = TranslationState::new();
        translation_state.set_scale_factor(scale_factor);

        // Create with AccessKit adapter (shows window after adapter is ready)
        let pw = pollster::block_on(PlatformWindow::new_with_a11y(window, target));

        let mut tree = WidgetTree::new().with_theme(self.theme.clone());
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
            root_builder(&mut tree);
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

    /// Request redraw on all windows.
    pub fn request_redraw_all(&self) {
        for managed in self.windows.values() {
            managed.platform_window.request_redraw();
        }
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
