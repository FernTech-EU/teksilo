// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! App-level window-operation sink.
//!
//! [`WindowOps`] is the trait implemented by the app-level window
//! manager (`teksilo_app::WindowManager`) and handed to every
//! [`EventContext`](crate::widget::EventContext) during event
//! dispatch. Handlers reach the multi-window API through it.
//!
//! Calls are **synchronous** except the two activation-token requests,
//! which answer through a callback on a later event-loop tick, and a quit
//! request, which the application carries out over the ticks that follow:
//! `open_window` creates the winit window and registers it before
//! returning; the returned id is immediately usable for
//! `focus_window`, `window_state`, `close_window_by_id`. The trait exists to keep teksilo-core
//! independent of teksilo-app — teksilo-core defines the contract, teksilo-app
//! provides the implementation.

use super::config::WindowConfig;
use super::id::TeksiloWindowId;
use super::state::WindowState;
use super::string_id::WindowStringIdError;
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

    /// Give the open window `window` the string id `new_id`, in place of
    /// the one it was opened with (or as its first, for a window opened
    /// without one). See
    /// [`EventContext::set_window_string_id`](crate::widget::EventContext::set_window_string_id)
    /// for what it changes and when it refuses.
    ///
    /// Default implementation: [`WindowStringIdError::Unsupported`], so an
    /// implementation written before this method existed keeps compiling
    /// and says plainly that it cannot rename.
    fn set_window_string_id(
        &mut self,
        _window: TeksiloWindowId,
        _new_id: String,
    ) -> Result<(), WindowStringIdError> {
        Err(WindowStringIdError::Unsupported)
    }

    /// Read the reactive state for a specific window.
    fn window_state(&self, id: TeksiloWindowId) -> Option<WindowState>;

    /// Every live window's state, in creation order.
    fn windows(&self) -> Vec<WindowState>;

    /// Raise a window and give it keyboard focus, restoring it first if it
    /// is minimised.
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

    /// Tell the platform's input method that the focused field's composition
    /// is over (a click elsewhere, a focus change), and return the text the
    /// input method committed for it, if it committed one: the field keeps
    /// that text in place of what the composition showed. On Windows the input
    /// method is asked to complete the composition, as a native edit control
    /// does, and its commit can differ from what it showed (Microsoft Pinyin
    /// commits "nihao" for the "ni'hao" it showed). `None` keeps what the
    /// composition showed: the platform keeps that (macOS, Linux), or the
    /// input method committed nothing.
    ///
    /// The field calls it while it still holds the composition, and keeps the
    /// text right after, in the same handler. Whatever the platform delivers
    /// for the composition arrives after the handler returns (an empty update,
    /// an empty commit), and must find nothing left to change; a copy of the
    /// commit returned here is not delivered.
    ///
    /// `None` on the standalone / test sink.
    fn finish_ime_composition(&mut self) -> Option<String> {
        None
    }

    /// Ask the platform to start a native OS drag for an in-app drag carrying
    /// an OS-exportable payload. The framework asks when the pointer leaves the
    /// window, or, for a payload with
    /// [`OsHandoff::Eager`](crate::OsHandoff::Eager), on the first pointer
    /// move. The platform backend (`teksilo_platform::external_dnd`) starts a
    /// native drag session (`NSDraggingSource` / `wl_data_source` / OLE
    /// `IDropSource` / XDND) using `data`, optionally drawing `image` as the
    /// drag cursor.
    ///
    /// The answer decides what happens to the in-app session:
    ///
    /// - [`OsDragStart::Declined`](crate::OsDragStart::Declined): nothing
    ///   started, and the in-app drag continues; a later pointer move may ask
    ///   again. This is the default, for the standalone / test sink and for a
    ///   platform without an outbound backend.
    /// - [`OsDragStart::Started`](crate::OsDragStart::Started): the native drag
    ///   is running (macOS starts it inside the request). The in-app session is
    ///   handed over at once.
    /// - [`OsDragStart::Pending`](crate::OsDragStart::Pending): the platform
    ///   starts the drag on its own loop (Wayland, X11, Windows), under the
    ///   [`OsDragId`](crate::OsDragId) it carries. The in-app session stays
    ///   alive until the platform confirms that drag's start through
    ///   [`WidgetTree::confirm_os_drag`](crate::WidgetTree::confirm_os_drag). A
    ///   release before that ends the drag as `Cancelled` and calls
    ///   [`Self::cancel_os_drag`]; a start that fails ends it through
    ///   [`WidgetTree::handle_os_drag_ended_for`](crate::WidgetTree::handle_os_drag_ended_for).
    ///   Events naming another drag's id change nothing.
    ///
    /// A failure known now (nothing to drag from, nothing to export) is a
    /// `Declined`, never a `Pending` that fails later: a decline keeps the
    /// in-app drag, a failure ends it.
    ///
    /// `pointer` is the device carrying the drag. It is not decoration: on
    /// Wayland `wl_data_device::start_drag` must be given the serial of the
    /// input event that began the implicit grab, and a finger's grab was opened
    /// by a `wl_touch::down`, not a `wl_pointer::button`; hand the wrong
    /// serial over and the compositor rejects the request silently and sends no
    /// terminal event at all.
    fn begin_os_drag(
        &mut self,
        _data: crate::drag_payload::OutboundDragData,
        _image: Option<crate::drag_payload::DragImageData>,
        _pointer: teksilo_tokens::PointerKind,
    ) -> crate::drag_payload::OsDragStart {
        crate::drag_payload::OsDragStart::Declined
    }

    /// Tell the platform whether the widget under an **inbound** OS drag
    /// accepts it, so the OS shows the right cursor and permits (or refuses)
    /// the drop.
    ///
    /// An inbound backend must answer the drag source synchronously — XDND
    /// requires an `XdndStatus` for every `XdndPosition`, and Wayland wants
    /// `wl_data_offer::accept` + `set_actions` on the offer — which happens on
    /// the backend's own thread, before the widget tree has seen the sample. So
    /// the backend's first answer can only be about *format* compatibility;
    /// this is how the widget's actual verdict gets back to the OS. Called by
    /// the tree only when the answer changes.
    ///
    /// The negotiated *operation* follows from the bit: Copy when accepted,
    /// none when refused. Copy is the only operation Teksilo advertises in
    /// either direction, so there is nothing else for a widget to choose — see
    /// `docs/drag-and-drop.md` §11.5.
    ///
    /// Default: no-op — the standalone sink and any platform without an
    /// inbound backend.
    fn set_drop_accepted(&mut self, _accepted: bool) {}

    /// Abandon an OS drag requested with [`Self::begin_os_drag`]: the user
    /// pressed Escape, or released the pointer before a pending start was
    /// confirmed.
    ///
    /// A start that is still pending must not happen after this: the backend
    /// drops it, or, if the native drag has already started, ends it. Once the
    /// drag runs, macOS and Windows hand it to a modal OS loop that owns Escape
    /// already, and this does nothing there; Wayland ends it by destroying its
    /// data source (an Escape this window still receives, over one of its own
    /// surfaces); X11 tracks the pointer on its own connection, so without this
    /// its drags could only end by releasing the button. A backend that ends a running drag still reports the
    /// terminal `DropOutcome`, so the source widget's `on_drag_ended` fires
    /// exactly once regardless.
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

    /// Ask the application to quit, asking every open window's close guard
    /// in turn. See [`EventContext::quit`](crate::widget::EventContext::quit).
    ///
    /// Default: no-op, for a sink with no application behind it.
    fn quit(&mut self) {}

    /// Quit the application without asking any close guard. See
    /// [`EventContext::quit_forced`](crate::widget::EventContext::quit_forced).
    ///
    /// Default: no-op, for a sink with no application behind it.
    fn quit_forced(&mut self) {}

    /// Whether a quit is under way: requested, and neither finished nor
    /// abandoned. Default: `false`.
    fn quit_in_progress(&self) -> bool {
        false
    }

    /// Where the quit under way came from, or `None` when no quit is under
    /// way. Default: `None`.
    fn quit_source(&self) -> Option<crate::window::QuitSource> {
        None
    }

    /// Abandon the quit under way, if it is a guarded one; see
    /// [`EventContext::cancel_quit`](crate::widget::EventContext::cancel_quit).
    /// Default: no-op.
    fn cancel_quit(&mut self) {}

    /// Take the activation token the event being dispatched carried, while
    /// no window operation has used it: an intent kept for another tree takes
    /// it with it, and nothing else can use it afterwards (a token is good
    /// for one activation). Default: `None`, for a sink with no application
    /// behind it.
    #[doc(hidden)]
    fn take_activation_token(&mut self) -> Option<String> {
        None
    }

    /// Hold `token`, which an intent routed from another tree carried, for
    /// the first window the actions it reaches open or raise. Paired with
    /// [`release_activation_token`](Self::release_activation_token).
    /// Default: no-op.
    #[doc(hidden)]
    fn hold_activation_token(&mut self, _token: String) {}

    /// The routed intent's actions have run: a token they did not use is
    /// left to a `WindowState::focus()` they queued, for this turn only, as
    /// after any event that carried one, unless a token already waits for
    /// such a focus (an earlier handler of the turn left it): that one
    /// stays, whether this one was used or not. Default: no-op.
    #[doc(hidden)]
    fn release_activation_token(&mut self) {}
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
