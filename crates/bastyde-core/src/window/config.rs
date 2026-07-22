// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Configuration for creating a new window.
//!
//! Consumed by either the app builder's "initial window" at startup or
//! [`EventContext::open_window`](crate::widget::EventContext) from
//! handler code. The two paths share the same config and produce the
//! same windows — there is no "initial vs runtime" split.

use std::rc::Rc;

use crate::signal::Prop;
use crate::widget::EventContext;
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

/// Verdict returned by a window's [close guard](WindowConfig::on_close_requested)
/// when the user (or the app) asks to close the window.
///
/// The guard runs *before* the window's tree is torn down. Returning
/// [`Veto`](CloseResponse::Veto) cancels that one close attempt and
/// leaves the window open — the idiomatic place to pop a
/// "you have unsaved changes" confirmation, then re-issue the close via
/// [`EventContext::close_window_forced`](crate::widget::EventContext::close_window_forced)
/// once the user confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResponse {
    /// Proceed with closing the window.
    Close,
    /// Cancel this close attempt; the window stays open.
    Veto,
}

/// Signature of a window's close-request guard.
///
/// Invoked with a real [`EventContext`] for the window's own tree, so
/// the guard can show a confirmation dialog, open a modal child, set
/// signals, or fire intents before deciding. It is consulted on every
/// user-initiated close attempt (OS close button / `Alt+F4` / `Cmd+W`,
/// a custom-chrome close button, and
/// [`EventContext::close_window`](crate::widget::EventContext::close_window))
/// and may run many times over a window's lifetime, so it is an `Fn`,
/// not an `FnOnce`.
///
/// It is **not** consulted for a
/// [`close_window_forced`](crate::widget::EventContext::close_window_forced),
/// nor for framework-internal teardown (modal cleanup, the last-window
/// shutdown drain).
pub type CloseGuard = Rc<dyn Fn(&mut EventContext) -> CloseResponse>;

/// Signature of the [`on_close_blocked`](WindowConfig::on_close_blocked)
/// callback — the `Fn`-shaped notification fired when the
/// [`can_close`](WindowConfig::can_close) sugar signal vetoes a close.
/// Runs with the window's [`EventContext`] so it can present the
/// confirmation UI.
pub type CloseBlockedCallback = Rc<dyn Fn(&mut EventContext)>;

/// Snapshot handed to a [`WindowConfig::on_removed`] callback once its
/// window has finished tearing down.
///
/// There is no [`EventContext`] here, unlike [`CloseGuard`] /
/// [`CloseBlockedCallback`]: those run *before* teardown, while the
/// window's own tree is still alive to build a context from; `on_removed`
/// runs *after* — the tree, platform window, and every framework
/// registry entry for this window are already gone (see `on_removed`'s
/// doc comment for exactly where in teardown it fires).
#[derive(Debug, Clone)]
pub struct WindowRemovedEvent {
    /// Identity of the window that was just removed. Redundant with
    /// whatever the closure already captured — a `WindowConfig` callback
    /// is inherently per-window — but useful when one closure is shared
    /// across several windows (e.g. `Rc<dyn Fn>` cloned onto every window
    /// opened for the same document).
    pub id: BastydeWindowId,
    /// The window's `string_id`, if it had one (persistence key / stable
    /// handle apps use to correlate a window with their own bookkeeping).
    pub string_id: Option<String>,
    /// How many windows remain across the whole app, counted AFTER this
    /// one's removal. `0` means this was the last window standing. The
    /// framework has no notion of "this app's Work/document" grouping —
    /// an app that needs a *scoped* last-window answer (e.g. "last window
    /// for this particular Work") combines this fact with its own
    /// window-to-Work bookkeeping; this field only answers "last window,
    /// full stop".
    pub remaining_windows: usize,
}

/// Signature of the [`on_removed`](WindowConfig::on_removed) callback —
/// the framework's window-teardown hook. See [`WindowRemovedEvent`] and
/// [`WindowConfig::on_removed`] for exactly when it runs and what it
/// receives.
pub type WindowRemovedCallback = Rc<dyn Fn(&WindowRemovedEvent)>;

/// Configuration for creating a new window.
pub struct WindowConfig {
    pub title: String,
    pub string_id: Option<String>,
    pub size: (u32, u32),
    pub position: Option<(i32, i32)>,
    pub min_size: Option<(u32, u32)>,
    pub max_size: Option<(u32, u32)>,
    /// Whether this window's geometry is **restored** from the persisted
    /// window state at creation. Default `true`.
    ///
    /// Persisting and restoring are usually the same decision, so
    /// [`string_id`](Self::string_id) normally governs both. They come apart in
    /// one common case: a **multi-window (or multi-process) app where every
    /// window shares one geometry slot.** Restoring the saved geometry into
    /// *every* window would stack them exactly on top of each other; you want
    /// the first window to land where the user left it, and any window opened
    /// alongside it to be placed by the OS (which cascades). But you still want
    /// every window to *save* its geometry, so whichever the user moved or
    /// closed last is what reopens next time — the behaviour of Word, Firefox
    /// and most document apps.
    ///
    /// Set `false` for those later windows: they still persist under their
    /// `string_id`, they simply don't read the saved value back. With
    /// [`position`](Self::position) left `None`, the window manager picks the
    /// spot.
    pub restore_geometry: bool,
    pub initial_placement: WindowPlacement,
    pub decorations: DecorationsMode,
    pub resizable: bool,
    /// Whether the OS window resizes itself to fit its content's intrinsic
    /// size. See [`SizeToContent`]. Default [`SizeToContent::Off`].
    pub size_to_content: SizeToContent,
    pub always_on_top: bool,
    pub skip_taskbar: bool,
    /// When set, this window consumes an `xdg_activation_v1` startup token from
    /// the environment at creation so it comes up focused on Wayland (the
    /// launching process set it via `set_child_activation_env`). No effect off
    /// Wayland/X11.
    pub activate_from_env: bool,
    pub icon: Option<WindowIcon>,
    pub modal: Option<ModalConfig>,
    pub root_builder: Option<RootBuilder>,
    /// Optional post-root wrapper. When set, the framework calls it
    /// after `root_builder` and uses the returned id as the window's
    /// effective root. See [`PostRootBuilder`].
    pub post_root_builder: Option<PostRootBuilder>,
    /// Optional close guard. Consulted before this window closes in
    /// response to a user gesture; returning [`CloseResponse::Veto`]
    /// cancels the close. See [`WindowConfig::on_close_requested`].
    pub on_close_requested: Option<CloseGuard>,
    /// Optional reactive "may this window close?" signal. Sugar over
    /// `on_close_requested`: when present and `false`, a close attempt
    /// is vetoed and [`on_close_blocked`](Self::on_close_blocked) fires
    /// (if set). See [`WindowConfig::can_close`].
    pub can_close: Option<Prop<bool>>,
    /// Optional notification fired when the [`can_close`](Self::can_close)
    /// signal blocks a close — the hook that presents the confirmation
    /// UI. See [`WindowConfig::on_close_blocked`].
    pub on_close_blocked: Option<CloseBlockedCallback>,
    /// Optional teardown hook, fired once this window has been fully
    /// removed from the window manager. See [`WindowConfig::on_removed`].
    pub on_removed: Option<WindowRemovedCallback>,
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
            .field("size_to_content", &self.size_to_content)
            .field("always_on_top", &self.always_on_top)
            .field("skip_taskbar", &self.skip_taskbar)
            .field("activate_from_env", &self.activate_from_env)
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
            .field(
                "on_close_requested",
                &self.on_close_requested.as_ref().map(|_| "<closure>"),
            )
            .field("can_close", &self.can_close.as_ref().map(|_| "<signal>"))
            .field(
                "on_close_blocked",
                &self.on_close_blocked.as_ref().map(|_| "<closure>"),
            )
            .field(
                "on_removed",
                &self.on_removed.as_ref().map(|_| "<closure>"),
            )
            .finish()
    }
}

/// Whether an OS window resizes itself to fit its content's intrinsic height.
///
/// `Off` (default) keeps the window at its configured
/// [`size`](WindowConfig::size). `Height` fixes the width and grows or shrinks
/// the height to the content's natural height — the modal-dialog case, e.g. a
/// `MessageBox` whose "Show details" expander adds text.
///
/// The window never shrinks below its [`min_size`](WindowConfig::min_size)
/// floor, and its width is left untouched. Intended for a window with a single
/// primary content root (a dialog). The content's height must NOT depend on the
/// window's own height (e.g. a signal bound to the window size), or the
/// measure → resize loop may fail to converge. On Wayland only the size
/// round-trips (position is compositor-owned), which is fine — size-to-content
/// changes only size.
///
/// (A width / both-axes mode is intentionally not offered: no consumer needs
/// it, and a half-wired variant would silently behave like `Height`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeToContent {
    /// The window keeps its configured size (the default).
    #[default]
    Off,
    /// Width is fixed; height follows the content's intrinsic height.
    Height,
}

impl SizeToContent {
    /// The window sizes its height to the content.
    pub fn sizes_height(self) -> bool {
        matches!(self, Self::Height)
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
            restore_geometry: true,
            initial_placement: WindowPlacement::Floating,
            decorations: DecorationsMode::Native,
            resizable: true,
            size_to_content: SizeToContent::Off,
            always_on_top: false,
            skip_taskbar: false,
            activate_from_env: false,
            icon: None,
            modal: None,
            root_builder: None,
            post_root_builder: None,
            on_close_requested: None,
            can_close: None,
            on_close_blocked: None,
            on_removed: None,
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

    /// Whether to restore this window's persisted geometry at creation
    /// (default `true`). See [`WindowConfig::restore_geometry`].
    ///
    /// Pass `false` for a window that should still *save* its geometry but be
    /// placed by the OS rather than reopened at the remembered spot — the
    /// second and later windows of an app whose windows share one geometry
    /// slot, which would otherwise all land exactly on top of each other.
    pub fn restore_geometry(mut self, restore: bool) -> Self {
        self.restore_geometry = restore;
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
    /// (on X11, falls back to `Native` when the window manager lacks
    /// `_NET_WM_MOVERESIZE`); `None` is borderless.
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

    /// Make this window resize itself to fit its content's intrinsic size.
    /// See [`SizeToContent`]. The configured [`size`](Self::size) /
    /// [`min_size`](Self::min_size) act as a floor. Used for native modal dialogs
    /// (e.g. `MessageBox`) so the OS window grows when the content does —
    /// matching the in-tree overlay path.
    ///
    /// Do NOT also call `.resizable(false)`: winit encodes non-resizable as
    /// equal min/max size hints (notably on X11), which would clamp away the
    /// programmatic growth this relies on.
    pub fn size_to_content(mut self, mode: SizeToContent) -> Self {
        self.size_to_content = mode;
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

    /// Consume an `xdg_activation_v1` startup token from the environment at
    /// creation so this window comes up focused on Wayland. Set on the initial
    /// window of a process spawned by another instance's "open in new window".
    pub fn activate_from_env(mut self, on: bool) -> Self {
        self.activate_from_env = on;
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

    /// Install a **close guard** consulted before this window closes in
    /// response to a user gesture — the OS close button / `Alt+F4` /
    /// `Cmd+W`, a custom-chrome close button, or
    /// [`EventContext::close_window`](crate::widget::EventContext::close_window).
    ///
    /// The guard runs with a real [`EventContext`] for this window's
    /// tree. Return [`CloseResponse::Close`] to let the close proceed,
    /// or [`CloseResponse::Veto`] to cancel it. The canonical pattern is
    /// veto-then-reissue:
    ///
    /// ```ignore
    /// WindowConfig::new()
    ///     .on_close_requested(move |ctx| {
    ///         if has_unsaved_changes() {
    ///             ctx.show_message_box(/* "Save before closing?" */);
    ///             CloseResponse::Veto
    ///         } else {
    ///             CloseResponse::Close
    ///         }
    ///     });
    ///
    /// // …and from the confirmation dialog's "Discard & Close" button:
    /// ctx.close_window_forced();
    /// ```
    ///
    /// [`close_window_forced`](crate::widget::EventContext::close_window_forced)
    /// bypasses the guard, so the second close actually goes through.
    /// The guard is **not** consulted for framework-internal teardown
    /// (modal cleanup, the final-window shutdown drain).
    pub fn on_close_requested(
        mut self,
        guard: impl Fn(&mut EventContext) -> CloseResponse + 'static,
    ) -> Self {
        self.on_close_requested = Some(Rc::new(guard));
        self
    }

    /// Reactive sugar over [`on_close_requested`](Self::on_close_requested):
    /// bind a `Signal<bool>` that answers "may this window close right
    /// now?". While the signal reads `false`, every user-initiated close
    /// attempt is vetoed and [`on_close_blocked`](Self::on_close_blocked)
    /// (if set) fires so the app can surface a confirmation.
    ///
    /// `can_close` is evaluated *before* the `on_close_requested` guard:
    /// a `false` signal short-circuits to a veto; a `true` signal (or no
    /// signal) falls through to the guard, then to closing.
    pub fn can_close(mut self, may_close: impl Into<Prop<bool>>) -> Self {
        self.can_close = Some(may_close.into());
        self
    }

    /// Notification fired when the [`can_close`](Self::can_close) signal
    /// blocks a close attempt. Runs with this window's [`EventContext`];
    /// use it to open the confirmation dialog / modal that, on confirm,
    /// calls
    /// [`close_window_forced`](crate::widget::EventContext::close_window_forced).
    /// No-op unless a `can_close` signal is also set.
    pub fn on_close_blocked(mut self, on_blocked: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_close_blocked = Some(Rc::new(on_blocked));
        self
    }

    /// Take the close guard out of the config. Consumed by the window
    /// manager once during `create_window`, which stores it on the
    /// managed window for the window's lifetime.
    pub fn take_close_guard(&mut self) -> Option<CloseGuard> {
        self.on_close_requested.take()
    }

    /// Take the `can_close` prop out of the config.
    pub fn take_can_close(&mut self) -> Option<Prop<bool>> {
        self.can_close.take()
    }

    /// Take the `on_close_blocked` callback out of the config.
    pub fn take_close_blocked(&mut self) -> Option<CloseBlockedCallback> {
        self.on_close_blocked.take()
    }

    /// Register a teardown hook for this window: an `Fn`, not `FnOnce` or
    /// `FnMut`, because a shared closure (`Rc`-cloned config, or a
    /// closure built once and attached to several windows opened for the
    /// same document) may run once per window it's attached to.
    ///
    /// Fires exactly once, no matter which of the two ways this window
    /// closes:
    /// - a *guarded* close ([`EventContext::close_window`](crate::widget::EventContext::close_window)
    ///   / the OS close button / `Alt+F4` / `Cmd+W`), once
    ///   [`can_close`](Self::can_close) / [`on_close_requested`](Self::on_close_requested)
    ///   let it through;
    /// - a *forced* close ([`EventContext::close_window_forced`](crate::widget::EventContext::close_window_forced) /
    ///   [`EventContext::close_window_by_id`](crate::widget::EventContext::close_window_by_id)),
    ///   which bypasses the guard entirely;
    ///
    /// because the window manager funnels both through the same, single
    /// teardown routine.
    ///
    /// Runs **after** the window is gone: its tree has been dropped, its
    /// platform window destroyed, and every framework-internal
    /// registration for it (native menu, drag-and-drop target, pending
    /// async completions, …) purged. This is the deliberate choice — it
    /// is what lets [`WindowRemovedEvent::remaining_windows`] already
    /// exclude the window being removed, so a handler that wants to know
    /// "was this the last window [for my Work]" gets an unambiguous
    /// answer rather than having to remember to subtract one. The
    /// trade-off is that the callback cannot reach into the removed
    /// window's own widget tree — by the time it runs, there isn't one.
    /// If a hook needs to read tree state before it's torn down, that has
    /// to happen earlier, in [`on_close_requested`](Self::on_close_requested)
    /// or [`on_close_blocked`](Self::on_close_blocked).
    ///
    /// The intended use is releasing whatever an app keeps keyed by a
    /// window's [`BastydeWindowId`] — a shared-document refcount, an
    /// entry in the app's own "windows open for this Work" map — so that
    /// bookkeeping is decremented exactly when the framework agrees the
    /// window is really gone, instead of a hand-maintained registry that
    /// only ever grows.
    pub fn on_removed(mut self, hook: impl Fn(&WindowRemovedEvent) + 'static) -> Self {
        self.on_removed = Some(Rc::new(hook));
        self
    }

    /// Take the `on_removed` teardown hook out of the config.
    pub fn take_on_removed(&mut self) -> Option<WindowRemovedCallback> {
        self.on_removed.take()
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
    use crate::signal::Signal;

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
        assert!(config.on_close_requested.is_none());
        assert!(config.can_close.is_none());
        assert!(config.on_close_blocked.is_none());
        assert!(config.on_removed.is_none());
        // Geometry is restored unless an app explicitly opts out.
        assert!(config.restore_geometry);
    }

    /// Persisting and restoring geometry are separate decisions.
    ///
    /// An app whose windows share one geometry slot wants the *first* window to
    /// reopen where the user left it and any window opened alongside it to be
    /// placed by the window manager — otherwise they all land on the same pixel.
    /// But those later windows must still *save* their geometry, so whichever
    /// the user moved or closed last is the one that reopens. That is
    /// `id(..)` + `restore_geometry(false)`: persist, don't restore.
    #[test]
    fn restore_geometry_can_be_opted_out_of_without_giving_up_persistence() {
        let config = WindowConfig::new().id("main").restore_geometry(false);

        assert!(!config.restore_geometry, "this window must not be restored");
        assert_eq!(
            config.string_id.as_deref(),
            Some("main"),
            "...but it keeps its id, so it still persists into that slot"
        );
        // And with no explicit position, the window manager picks the spot.
        assert!(config.position.is_none());
    }

    #[test]
    fn close_guard_builders_set_and_take() {
        let may_close = Signal::new(false);
        let mut config = WindowConfig::new()
            .on_close_requested(|_ctx| CloseResponse::Veto)
            .can_close(may_close.clone())
            .on_close_blocked(|_ctx| {});

        assert!(config.on_close_requested.is_some());
        assert!(config.can_close.is_some());
        assert!(config.on_close_blocked.is_some());

        // The window manager drains the guard fields exactly once at
        // create_window time; after that the config no longer carries them.
        let guard = config.take_close_guard();
        let signal = config.take_can_close();
        let blocked = config.take_close_blocked();
        assert!(guard.is_some());
        assert!(signal.is_some());
        assert!(blocked.is_some());
        assert!(config.on_close_requested.is_none());
        assert!(config.can_close.is_none());
        assert!(config.on_close_blocked.is_none());

        // The taken signal is the same handle the caller passed in.
        signal.unwrap().as_signal().set(true);
        assert!(may_close.get());
    }

    #[test]
    fn on_removed_builder_sets_and_takes() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Vec<WindowRemovedEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let mut config = WindowConfig::new().on_removed(move |ev| log.borrow_mut().push(ev.clone()));

        assert!(config.on_removed.is_some());

        // The window manager drains this exactly once at create_window
        // time, same discipline as the close-guard fields above.
        let hook = config.take_on_removed();
        assert!(hook.is_some());
        assert!(config.on_removed.is_none());

        // The taken callback is the same closure the caller passed in,
        // and it receives exactly the event it's handed — no field is
        // dropped or reordered on the way through the `Rc<dyn Fn>`.
        let id = BastydeWindowId::new(7);
        hook.unwrap()(&WindowRemovedEvent {
            id,
            string_id: Some("main".to_string()),
            remaining_windows: 0,
        });
        let logged = seen.borrow();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].id, id);
        assert_eq!(logged[0].string_id.as_deref(), Some("main"));
        assert_eq!(logged[0].remaining_windows, 0);
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
