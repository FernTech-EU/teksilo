//! Window configuration and identity types.

use fern_core::{WidgetId, WidgetTree};

/// Opaque identifier for an application window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FernWindowId(u64);

impl FernWindowId {
    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }

    /// The raw numeric ID (for serialization/debugging).
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for FernWindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Window({})", self.0)
    }
}

/// Configuration for creating a new window.
pub struct WindowConfig {
    pub(crate) title: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) string_id: Option<String>,
    pub(crate) modal: bool,
    pub(crate) parent: Option<FernWindowId>,
    pub(crate) root_builder: Option<Box<dyn FnOnce(&mut WidgetTree) -> WidgetId>>,
}

impl WindowConfig {
    pub fn new() -> Self {
        Self {
            title: "FernUI".to_string(),
            width: 800,
            height: 600,
            string_id: None,
            modal: false,
            parent: None,
            root_builder: None,
        }
    }

    /// Set the window title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the window size in logical pixels.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set a string identifier for finding this window later.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.string_id = Some(id.into());
        self
    }

    /// Mark this window as a modal dialog.
    /// A modal window blocks interaction with its parent until dismissed.
    pub fn modal(mut self, is_modal: bool) -> Self {
        self.modal = is_modal;
        self
    }

    /// Set the parent window (required for modal dialogs).
    pub fn parent(mut self, parent: FernWindowId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Set the root widget builder for this window.
    pub fn root(mut self, builder: impl FnOnce(&mut WidgetTree) -> WidgetId + 'static) -> Self {
        self.root_builder = Some(Box::new(builder));
        self
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_id_equality() {
        let a = FernWindowId::new(1);
        let b = FernWindowId::new(1);
        let c = FernWindowId::new(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn window_config_builder() {
        let config = WindowConfig::new()
            .title("Test")
            .size(400, 300)
            .id("test-window")
            .modal(true);

        assert_eq!(config.title, "Test");
        assert_eq!(config.width, 400);
        assert_eq!(config.height, 300);
        assert_eq!(config.string_id, Some("test-window".to_string()));
        assert!(config.modal);
    }

    #[test]
    fn window_config_defaults() {
        let config = WindowConfig::new();
        assert_eq!(config.title, "FernUI");
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
        assert!(!config.modal);
        assert!(config.parent.is_none());
        assert!(config.string_id.is_none());
    }
}
