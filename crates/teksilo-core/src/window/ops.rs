// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! App-level window-operation sink.
//!
//! [`WindowOps`] is the trait implemented by the app-level window
//! manager (`teksilo_app::WindowManager`) and handed to every
//! [`EventContext`](crate::widget::EventContext) during event
//! dispatch. Handlers reach the multi-window API through it.
//!
//! All calls are **synchronous**: `open_window` creates the winit
//! window and registers it before returning; the returned id is
//! immediately usable for `focus_window`, `window_state`,
//! `close_window_by_id`. The trait exists to keep teksilo-core
//! independent of teksilo-app — teksilo-core defines the contract, teksilo-app
//! provides the implementation.

use super::config::WindowConfig;
use super::id::TeksiloWindowId;
use super::state::WindowState;
use crate::raw_handle::ParentHandle;

/// App-level window operations exposed to handlers.
///
/// Implemented by `teksilo_app::WindowManager` (via a short-lived
/// wrapper that also holds `&ActiveEventLoop`). Passed into every
/// dispatch site as `&mut dyn WindowOps` and stored on
/// [`EventContext`](crate::widget::EventContext).
pub trait WindowOps {
    /// Open a new window. Creates the winit-level window
    /// synchronously inside this call and returns its id, which is
    /// immediately valid for any other method on this trait.
    fn open_window(&mut self, config: WindowConfig) -> TeksiloWindowId;

    /// Look up a window by the stable string id it was opened with
    /// (`WindowConfig::id`). Returns `None` if no live window carries
    /// that id.
    fn find_window(&self, string_id: &str) -> Option<TeksiloWindowId>;

    /// Read the reactive state for a specific window.
    fn window_state(&self, id: TeksiloWindowId) -> Option<WindowState>;

    /// Every live window's state, in creation order.
    fn windows(&self) -> Vec<WindowState>;

    /// Raise a window and give it keyboard focus.
    fn focus_window(&mut self, id: TeksiloWindowId);

    /// Request an `xdg_activation_v1` token for `id` — to hand to another window
    /// or a child process so it can raise itself on Wayland. `cb` fires once with
    /// the token string, or with `None` where unsupported (everything but
    /// Wayland/X11). Default implementation: immediate `None`.
    fn request_activation_token(
        &mut self,
        _id: TeksiloWindowId,
        cb: Box<dyn FnOnce(Option<String>)>,
    ) {
        cb(None);
    }

    /// Like [`request_activation_token`](Self::request_activation_token) but for
    /// the **current dispatching** window — the one whose handler is running.
    /// Works even mid-dispatch, when that window is temporarily out of the
    /// manager's map, because it uses the captured window handle instead of an id
    /// lookup. Use this when a focused widget needs a token to hand to another
    /// window or process. Default implementation: immediate `None`.
    fn request_activation_token_self(&mut self, cb: Box<dyn FnOnce(Option<String>)>) {
        cb(None);
    }

    /// Close a specific window by id. The window is fully torn down
    /// before the next event-loop tick.
    fn close_window_by_id(&mut self, id: TeksiloWindowId);

    /// Extract the platform parent handle of the window currently
    /// dispatching the event (the one that owns the in-flight
    /// `EventContext`). Used by native-dialog integrations
    /// (`teksilo_platform::file_dialog`) to parent OS dialogs to the
    /// originating Teksilo window.
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
    fn set_ime_cursor_area(&mut self, _area: teksilo_canvas::Rect) {}

    /// Hand an OS-level drag to the platform when an in-app drag escalates at
    /// the window boundary (the pointer left the window carrying an
    /// OS-exportable payload). The platform backend
    /// (`teksilo_platform::external_dnd`) starts a native drag session
    /// (`NSDraggingSource` / `wl_data_source` / OLE `IDropSource`) using
    /// `data`, optionally drawing `image` as the drag cursor.
    ///
    /// Returns `true` if a native drag session actually started. The default
    /// (standalone / test sink, and platforms without an outbound backend,
    /// a headless build, or a target with no drop-target implementation)
    /// returns `false`, in which case the framework cancels the drag
    /// — the pre-existing "pointer left the window ⇒ drag cancels" behavior.
    fn begin_os_drag(
        &mut self,
        _data: crate::drag_payload::OutboundDragData,
        _image: Option<crate::drag_payload::DragImageData>,
    ) -> bool {
        false
    }

    /// Abandon an OS drag started by [`Self::begin_os_drag`] (the user pressed
    /// Escape).
    ///
    /// Only backends that drive the drag themselves can honour this. macOS and
    /// Windows hand the drag to a modal OS loop that owns Escape already, and
    /// Wayland's compositor does the same; X11 tracks the pointer on its own
    /// connection, so without this its drags could only end by releasing the
    /// button. The backend still reports the terminal `DropOutcome` either way,
    /// so the source widget's `on_drag_ended` fires exactly once regardless.
    ///
    /// Default: no-op.
    fn cancel_os_drag(&mut self) {}

    /// What the host platform can do about an on-screen keyboard.
    ///
    /// Read by a widget that must decide whether a touch-only user can reach a
    /// keyboard at all: where the answer is [`SoftKeyboardSupport::None`] the
    /// framework will never raise one and promises nothing about whether the
    /// platform will, so a text surface that expects a finger has to offer its
    /// own affordance.
    ///
    /// Default: [`SoftKeyboardSupport::None`], which is the truth for a
    /// standalone tree with no window under it.
    fn soft_keyboard_support(&self) -> SoftKeyboardSupport {
        SoftKeyboardSupport::None
    }
}

/// What a platform can do about an on-screen keyboard.
///
/// Three answers, and the difference between them is what a caller may
/// *promise a user*, not how much code stands behind them.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Hash)]
#[non_exhaustive]
pub enum SoftKeyboardSupport {
    /// The framework has no keyboard request to send, and makes no promise
    /// that anything will rise on its own. Either the platform has no software
    /// keyboard at all, or it has one whose appearance is the platform's
    /// business and not reliable enough to promise. A request is dropped, and a
    /// text surface driven by touch needs its own affordance.
    #[default]
    None,
    /// A keyboard exists and is **guaranteed** to rise when a text control
    /// takes focus through the accessibility layer, so a touch-driven text
    /// surface needs no affordance of its own. There is still nothing to ask:
    /// the framework's ordinary IME-allowance reconcile is what summons it, and
    /// an explicit ask would at best duplicate that and at worst re-assert
    /// allowance, which cancels a live composition — so a request resolves to
    /// "already done".
    ///
    /// The guarantee is what separates this from [`None`](Self::None), which
    /// covers every platform where a keyboard may or may not appear.
    ViaAccessibility,
    /// A keyboard exists and can be shown and hidden on demand. Only a backend
    /// that can honour **both** directions may report this: a toggle whose
    /// current state is unknown cannot, because "show" would sometimes hide.
    Explicit,
}

/// No-op implementation used by standalone `WidgetTree`s constructed
/// outside of an app (tests, headless scenarios). Every method
/// returns `None` / does nothing; `open_window` panics because a
/// standalone tree has no winit back-end to create windows in.
pub struct NoopWindowOps;

impl WindowOps for NoopWindowOps {
    fn open_window(&mut self, _config: WindowConfig) -> TeksiloWindowId {
        panic!("open_window called on a standalone WidgetTree (no app context)");
    }

    fn find_window(&self, _string_id: &str) -> Option<TeksiloWindowId> {
        None
    }

    fn window_state(&self, _id: TeksiloWindowId) -> Option<WindowState> {
        None
    }

    fn windows(&self) -> Vec<WindowState> {
        Vec::new()
    }

    fn focus_window(&mut self, _id: TeksiloWindowId) {}

    fn close_window_by_id(&mut self, _id: TeksiloWindowId) {}
}
