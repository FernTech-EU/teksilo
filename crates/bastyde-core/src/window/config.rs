//! Configuration for creating a new window.
//!
//! Consumed by either the app builder's "initial window" at startup or
//! [`EventContext::open_window`](crate::widget::EventContext) from
//! handler code. The two paths share the same config and produce the
//! same windows — there is no "initial vs runtime" split.

use crate::widget_id::WidgetId;
use crate::widget_tree::WidgetTree;

use super::decorations::DecorationsMode;
use super::icon::WindowIcon;
use super::id::BastydeWindowId;
use super::placement::WindowPlacement;
use super::state::WindowState;

/// Parent + focus wiring for a modal window.
///
/// Modal is an `Option<ModalConfig>` on [`WindowConfig`]; the type
/// system enforces that a modal always names a parent, something a
/// `modal: bool` + `parent: Option<...>` split could not express.
#[derive(Debug, Clone)]
pub struct ModalConfig {
    /// Window whose input is blocked while this modal is open. Also
    /// the window the modal is transient for (Z-order parent on every
    /// OS).
    pub parent: BastydeWindowId,
    /// Explicit initial-focus target inside the modal's root subtree.
    /// When `None` the framework falls back to the root widget's
    /// `initial_focus_hint`, then `first_focusable_descendant`.
    pub focus_target: Option<WidgetId>,
}

/// Signature of a window's root-builder closure.
///
/// Receives a mutable [`WidgetTree`] and a cloned [`WindowState`] so
/// the builder can register widgets that bind against window-level
/// signals (placement, title, size, …).
pub type RootBuilder = Box<dyn FnOnce(&mut WidgetTree, WindowState) -> WidgetId>;

/// Per-window post-root hook. Runs after the user's `root_builder`
/// returns, with the resulting `WidgetId`. The hook may wrap the user
/// root in another widget and return the wrapper's id, or simply return
/// the original id unchanged. Used by the debug inspector to splice an
/// inspector shell around every window's root in debug builds.
pub type PostRootBuilder = Box<dyn FnOnce(&mut WidgetTree, WidgetId) -> WidgetId>;

/// Configuration for creating a new window.
pub struct WindowConfig {
    pub title: String,
    pub string_id: Option<String>,
    pub size: (u32, u32),
    pub position: Option<(i32, i32)>,
    pub min_size: Option<(u32, u32)>,
    pub max_size: Option<(u32, u32)>,
    pub initial_placement: WindowPlacement,
    pub decorations: DecorationsMode,
    pub resizable: bool,
    pub always_on_top: bool,
    pub skip_taskbar: bool,
    pub icon: Option<WindowIcon>,
    pub modal: Option<ModalConfig>,
    pub root_builder: Option<RootBuilder>,
    /// Optional post-root wrapper. When set, the framework calls it
    /// after `root_builder` and uses the returned id as the window's
    /// effective root. See [`PostRootBuilder`].
    pub post_root_builder: Option<PostRootBuilder>,
}

impl std::fmt::Debug for WindowConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowConfig")
            .field("title", &self.title)
            .field("string_id", &self.string_id)
            .field("size", &self.size)
            .field("position", &self.position)
            .field("min_size", &self.min_size)
            .field("max_size", &self.max_size)
            .field("initial_placement", &self.initial_placement)
            .field("decorations", &self.decorations)
            .field("resizable", &self.resizable)
            .field("always_on_top", &self.always_on_top)
            .field("skip_taskbar", &self.skip_taskbar)
            .field("icon", &self.icon.as_ref().map(|i| (i.width, i.height)))
            .field("modal", &self.modal)
            .field(
                "root_builder",
                &self.root_builder.as_ref().map(|_| "<closure>"),
            )
            .field(
                "post_root_builder",
                &self.post_root_builder.as_ref().map(|_| "<closure>"),
            )
            .finish()
    }
}

impl WindowConfig {
    /// Start a config with sensible defaults.
    ///
    /// Defaults: title `"Bastyde"`, size `800x600`,
    /// `WindowPlacement::Floating`, `DecorationsMode::Native`,
    /// resizable, no parent, no `id`, no root builder.
    pub fn new() -> Self {
        Self {
            title: "Bastyde".to_string(),
            string_id: None,
            size: (800, 600),
            position: None,
            min_size: None,
            max_size: None,
            initial_placement: WindowPlacement::Floating,
            decorations: DecorationsMode::Native,
            resizable: true,
            always_on_top: false,
            skip_taskbar: false,
            icon: None,
            modal: None,
            root_builder: None,
            post_root_builder: None,
        }
    }

    /// User-visible title. Also becomes the initial value of
    /// [`WindowState::title`].
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Restored size in logical pixels. This is the size the window
    /// returns to when leaving `Maximized` or `Fullscreen`, and the
    /// current size when placement is `Floating`.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }

    /// Restored on-screen position in logical pixels. `None` lets the
    /// window manager pick.
    pub fn position(mut self, x: i32, y: i32) -> Self {
        self.position = Some((x, y));
        self
    }

    /// Lower bound on the floating size. The OS prevents the user
    /// from resizing the window below this.
    pub fn min_size(mut self, width: u32, height: u32) -> Self {
        self.min_size = Some((width, height));
        self
    }

    /// Upper bound on the floating size.
    pub fn max_size(mut self, width: u32, height: u32) -> Self {
        self.max_size = Some((width, height));
        self
    }

    /// Stable string identifier for later lookup via
    /// [`EventContext::find_window`](crate::widget::EventContext).
    /// Optional — omit for "open a fresh window every time."
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.string_id = Some(id.into());
        self
    }

    /// Initial placement. Defaults to `Floating`; pass
    /// `WindowPlacement::Fullscreen` / `Maximized` to start in that
    /// state.
    pub fn initial_placement(mut self, placement: WindowPlacement) -> Self {
        self.initial_placement = placement;
        self
    }

    /// Chrome mode. `Native` draws OS decorations; `CustomChrome`
    /// constructs a [`PlatformTitleBarHost`](crate::PlatformTitleBarHost)
    /// (falls back to `Native` on X11); `None` is borderless.
    pub fn decorations(mut self, mode: DecorationsMode) -> Self {
        self.decorations = mode;
        self
    }

    /// Whether the user can resize the window interactively. Also
    /// affects whether maximize gestures are accepted on some
    /// platforms.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Keep this window above all others regardless of focus.
    pub fn always_on_top(mut self, on_top: bool) -> Self {
        self.always_on_top = on_top;
        self
    }

    /// Hide this window from the taskbar / dock. Useful for tool
    /// palettes and secondary overlays.
    pub fn skip_taskbar(mut self, skip: bool) -> Self {
        self.skip_taskbar = skip;
        self
    }

    /// Set the window's icon from a raw RGBA8 buffer. The icon is
    /// used by the taskbar / dock and the window's title bar on
    /// platforms where it applies.
    ///
    /// Invalid buffers (`rgba.len() != width * height * 4`) are
    /// logged and dropped at creation time — the window still opens,
    /// just with the platform default icon.
    pub fn icon(mut self, icon: WindowIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Make this window modal to the given parent, with no explicit
    /// focus target. Prefer this over constructing [`ModalConfig`]
    /// yourself when you already have the parent id handy.
    pub fn modal_to(mut self, parent: BastydeWindowId) -> Self {
        self.modal = Some(ModalConfig {
            parent,
            focus_target: None,
        });
        self
    }

    /// Make this window modal using a caller-built [`ModalConfig`].
    /// Use this form when you need to specify an explicit
    /// `focus_target`.
    pub fn modal(mut self, config: ModalConfig) -> Self {
        self.modal = Some(config);
        self
    }

    /// Root-widget builder. Called once during window creation with
    /// the new window's [`WidgetTree`] and a cloned [`WindowState`]
    /// so widgets can bind against window-level signals.
    pub fn root(
        mut self,
        builder: impl FnOnce(&mut WidgetTree, WindowState) -> WidgetId + 'static,
    ) -> Self {
        self.root_builder = Some(Box::new(builder));
        self
    }

    // ----- Query helpers used by the app-level window manager -------

    /// Take the root builder out of the config, leaving `None` in its
    /// place. Consumed by the window manager exactly once during
    /// `create_window`.
    pub fn take_root_builder(&mut self) -> Option<RootBuilder> {
        self.root_builder.take()
    }

    /// Attach a per-window post-root hook. Runs after the user's
    /// `root_builder` returns; receives the user's root id and may
    /// return either the same id or a wrapper's id. The framework uses
    /// the returned id as the window's effective root.
    ///
    /// Typically used by the debug inspector. Apps that want to
    /// install a default wrapper across all windows should use the
    /// app-level mechanism instead of setting this per-config.
    pub fn post_root(
        mut self,
        builder: impl FnOnce(&mut WidgetTree, WidgetId) -> WidgetId + 'static,
    ) -> Self {
        self.post_root_builder = Some(Box::new(builder));
        self
    }

    /// Take the post-root builder out of the config.
    pub fn take_post_root_builder(&mut self) -> Option<PostRootBuilder> {
        self.post_root_builder.take()
    }

    pub fn is_modal(&self) -> bool {
        self.modal.is_some()
    }

    pub fn modal_parent(&self) -> Option<BastydeWindowId> {
        self.modal.as_ref().map(|m| m.parent)
    }

    pub fn modal_focus_target(&self) -> Option<WidgetId> {
        self.modal.as_ref().and_then(|m| m.focus_target)
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
    fn window_config_defaults() {
        let config = WindowConfig::new();
        assert_eq!(config.title, "Bastyde");
        assert_eq!(config.size, (800, 600));
        assert_eq!(config.initial_placement, WindowPlacement::Floating);
        assert_eq!(config.decorations, DecorationsMode::Native);
        assert!(config.resizable);
        assert!(!config.always_on_top);
        assert!(!config.skip_taskbar);
        assert!(config.modal.is_none());
        assert!(config.string_id.is_none());
        assert!(config.position.is_none());
        assert!(config.min_size.is_none());
        assert!(config.max_size.is_none());
    }

    #[test]
    fn builder_sets_fields() {
        let config = WindowConfig::new()
            .title("Test")
            .size(400, 300)
            .id("test-window")
            .initial_placement(WindowPlacement::Fullscreen)
            .decorations(DecorationsMode::CustomChrome)
            .min_size(200, 150)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .position(100, 50);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size, (400, 300));
        assert_eq!(config.string_id, Some("test-window".to_string()));
        assert_eq!(config.initial_placement, WindowPlacement::Fullscreen);
        assert_eq!(config.decorations, DecorationsMode::CustomChrome);
        assert_eq!(config.min_size, Some((200, 150)));
        assert_eq!(config.position, Some((100, 50)));
        assert!(!config.resizable);
        assert!(config.always_on_top);
        assert!(config.skip_taskbar);
    }

    #[test]
    fn modal_to_sets_parent() {
        let parent = BastydeWindowId::new(3);
        let config = WindowConfig::new().modal_to(parent);
        assert!(config.is_modal());
        assert_eq!(config.modal_parent(), Some(parent));
        assert_eq!(config.modal_focus_target(), None);
    }

    #[test]
    fn modal_with_focus_target() {
        let parent = BastydeWindowId::new(3);
        let target = WidgetId::default();
        let config = WindowConfig::new().modal(ModalConfig {
            parent,
            focus_target: Some(target),
        });
        assert!(config.is_modal());
        assert_eq!(config.modal_parent(), Some(parent));
        assert_eq!(config.modal_focus_target(), Some(target));
    }
}
