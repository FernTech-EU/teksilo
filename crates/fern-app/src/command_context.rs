//! Window-aware command context for application command handlers.
//!
//! When a widget emits a command, the application's command handler receives
//! it along with a `CommandContext` that identifies which window the command
//! came from and allows the handler to create/close windows and switch themes.

use fern_tokens::Theme;

use crate::window_config::{FernWindowId, WindowConfig};

/// Context available to application command handlers.
///
/// Provides window identity, lifecycle operations, and environment changes.
/// All mutating operations are queued and applied after the handler returns.
pub struct CommandContext {
    source: FernWindowId,
    current_theme: Theme,
    pending_creates: Vec<WindowConfig>,
    pending_closes: Vec<FernWindowId>,
    pending_theme: Option<Theme>,
}

impl CommandContext {
    pub(crate) fn new(source: FernWindowId, theme: Theme) -> Self {
        Self {
            source,
            current_theme: theme,
            pending_creates: Vec::new(),
            pending_closes: Vec::new(),
            pending_theme: None,
        }
    }

    /// The window from which the current command was emitted.
    pub fn source_window(&self) -> FernWindowId {
        self.source
    }

    /// Read the current theme (before any queued changes).
    pub fn theme(&self) -> &Theme {
        &self.current_theme
    }

    /// Queue a theme change. Applied after the command handler returns.
    /// Triggers a full composite rebuild across all windows.
    pub fn set_theme(&mut self, theme: Theme) {
        self.pending_theme = Some(theme);
    }

    /// Close a window by its ID.
    pub fn close_window(&mut self, id: FernWindowId) {
        self.pending_closes.push(id);
    }

    /// Create a new window with the given configuration.
    pub fn create_window(&mut self, config: WindowConfig) {
        self.pending_creates.push(config);
    }

    /// Drain pending window creates.
    pub(crate) fn take_creates(&mut self) -> Vec<WindowConfig> {
        std::mem::take(&mut self.pending_creates)
    }

    /// Drain pending window closes.
    pub(crate) fn take_closes(&mut self) -> Vec<FernWindowId> {
        std::mem::take(&mut self.pending_closes)
    }

    /// Drain pending theme change.
    pub(crate) fn take_theme(&mut self) -> Option<Theme> {
        self.pending_theme.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_window_round_trips() {
        let id = FernWindowId::new(42);
        let ctx = CommandContext::new(id, Theme::light_default());
        assert_eq!(ctx.source_window(), id);
    }

    #[test]
    fn create_and_close_are_deferred() {
        let id = FernWindowId::new(1);
        let mut ctx = CommandContext::new(id, Theme::light_default());

        ctx.close_window(FernWindowId::new(2));
        ctx.create_window(WindowConfig::new().title("New"));

        let creates = ctx.take_creates();
        let closes = ctx.take_closes();

        assert_eq!(creates.len(), 1);
        assert_eq!(creates[0].title, "New");
        assert_eq!(closes.len(), 1);
        assert_eq!(closes[0], FernWindowId::new(2));
    }

    #[test]
    fn set_theme_is_deferred() {
        let id = FernWindowId::new(1);
        let mut ctx = CommandContext::new(id, Theme::light_default());

        assert!(ctx.take_theme().is_none());
        ctx.set_theme(Theme::dark_default());
        let theme = ctx.take_theme();
        assert!(theme.is_some());
    }

    #[test]
    fn theme_returns_current() {
        let id = FernWindowId::new(1);
        let ctx = CommandContext::new(id, Theme::dark_default());
        // Should return the current theme, not any pending one
        let _theme = ctx.theme();
    }
}
