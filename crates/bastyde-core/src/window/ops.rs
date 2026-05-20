//! App-level window-operation sink.
//!
//! [`WindowOps`] is the trait implemented by the app-level window
//! manager (`bastyde_app::WindowManager`) and handed to every
//! [`EventContext`](crate::widget::EventContext) during event
//! dispatch. Handlers reach the multi-window API through it.
//!
//! All calls are **synchronous**: `open_window` creates the winit
//! window and registers it before returning; the returned id is
//! immediately usable for `focus_window`, `window_state`,
//! `close_window_by_id`. The trait exists to keep bastyde-core
//! independent of bastyde-app — bastyde-core defines the contract, bastyde-app
//! provides the implementation.

use super::config::WindowConfig;
use super::id::BastydeWindowId;
use super::state::WindowState;
use crate::raw_handle::ParentHandle;

/// App-level window operations exposed to handlers.
///
/// Implemented by `bastyde_app::WindowManager` (via a short-lived
/// wrapper that also holds `&ActiveEventLoop`). Passed into every
/// dispatch site as `&mut dyn WindowOps` and stored on
/// [`EventContext`](crate::widget::EventContext).
pub trait WindowOps {
    /// Open a new window. Creates the winit-level window
    /// synchronously inside this call and returns its id, which is
    /// immediately valid for any other method on this trait.
    fn open_window(&mut self, config: WindowConfig) -> BastydeWindowId;

    /// Look up a window by the stable string id it was opened with
    /// (`WindowConfig::id`). Returns `None` if no live window carries
    /// that id.
    fn find_window(&self, string_id: &str) -> Option<BastydeWindowId>;

    /// Read the reactive state for a specific window.
    fn window_state(&self, id: BastydeWindowId) -> Option<WindowState>;

    /// Every live window's state, in creation order.
    fn windows(&self) -> Vec<WindowState>;

    /// Raise a window and give it keyboard focus.
    fn focus_window(&mut self, id: BastydeWindowId);

    /// Close a specific window by id. The window is fully torn down
    /// before the next event-loop tick.
    fn close_window_by_id(&mut self, id: BastydeWindowId);

    /// Extract the platform parent handle of the window currently
    /// dispatching the event (the one that owns the in-flight
    /// `EventContext`). Used by native-dialog integrations
    /// (`bastyde_platform::file_dialog`) to parent OS dialogs to the
    /// originating Bastyde window.
    ///
    /// Returns `None` for the standalone / test sink and on rare
    /// platform paths where the underlying surface refuses a handle
    /// (e.g. during shutdown).
    fn current_parent_handle(&self) -> Option<ParentHandle> {
        None
    }

    /// Report the focused text widget's caret rectangle (in window-logical
    /// pixels) so the platform can position the OS IME candidate window
    /// next to the insertion point. Called from text-editing widgets
    /// whenever the caret moves. The app applies it to the in-flight
    /// window's `set_ime_cursor_area`, deduped against the last value.
    ///
    /// No-op on the standalone / test sink.
    fn set_ime_cursor_area(&mut self, _area: bastyde_canvas::Rect) {}
}

/// No-op implementation used by standalone `WidgetTree`s constructed
/// outside of an app (tests, headless scenarios). Every method
/// returns `None` / does nothing; `open_window` panics because a
/// standalone tree has no winit back-end to create windows in.
pub struct NoopWindowOps;

impl WindowOps for NoopWindowOps {
    fn open_window(&mut self, _config: WindowConfig) -> BastydeWindowId {
        panic!("open_window called on a standalone WidgetTree (no app context)");
    }

    fn find_window(&self, _string_id: &str) -> Option<BastydeWindowId> {
        None
    }

    fn window_state(&self, _id: BastydeWindowId) -> Option<WindowState> {
        None
    }

    fn windows(&self) -> Vec<WindowState> {
        Vec::new()
    }

    fn focus_window(&mut self, _id: BastydeWindowId) {}

    fn close_window_by_id(&mut self, _id: BastydeWindowId) {}
}
