// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! External (OS) drag-and-drop service.
//!
//! Lets a window accept drops that originate **outside** the application —
//! files dragged from the file manager, or text / URLs dragged from another
//! app — and feed them into the same drag pipeline used for in-app drags
//! ([`teksilo_core::WidgetTree::begin_external_drag`] et al.).
//!
//! Three concerns are separated, mirroring [`crate::file_dialog`]:
//!
//! - **Trait surface** — [`ExternalDndBackend`] is the swappable platform
//!   abstraction. A backend registers itself as the OS drop target for a
//!   window and, for each phase of a drag, posts an [`ExternalDndEventPayload`]
//!   through [`teksilo_core::AppEventPoster::post_external`].
//! - **Handle** — [`ExternalDndHandle`] is the per-app service registered in
//!   app-state. It owns the backend and the per-window registration guards.
//!   `teksilo-app` calls [`ExternalDndHandle::attach`] when a window is created
//!   and [`ExternalDndHandle::detach`] when it closes.
//! - **Event delivery** — `teksilo-app` picks the payload up in its
//!   `AppEvent::External` arm, routes it to the originating window's
//!   `WidgetTree`, and calls the matching `*_external_drag` method.
//!
//! # Why raw platform backends
//!
//! winit's `DroppedFile` / `HoveredFile` events carry no cursor position,
//! support files only, and are unimplemented on Wayland. A drop-zone widget
//! placed inside a layout needs the drop position to hit-test which zone
//! received the drop, so the real backends sit below winit on the raw
//! platform APIs (OLE `IDropTarget` on Windows, `NSDraggingDestination` on
//! macOS, `wl_data_device` on Wayland, XDND on X11), all of which provide
//! position and arbitrary data formats. [`NoopExternalDndBackend`] is left for
//! targets with no drop-target implementation at all.
//!
//! # Threading
//!
//! The macOS and Windows drop targets deliver their callbacks on the UI
//! thread; the Wayland and X11 backends run a dedicated per-window dispatch
//! thread on their own protocol connection. Either way the payload is routed
//! through [`teksilo_core::AppEventPoster::post_external`] (the same channel as
//! file dialogs) so the borrow of the window's tree happens in one
//! well-defined place in the event loop rather than re-entrantly inside a
//! platform callback — and so a backend thread never touches the tree at all.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use teksilo_canvas::Point;
use teksilo_core::AppEventPoster;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{
    DragImageData, DropOutcome, ExternalDropData, OsDragId, OsDragStart, OutboundDragData,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

// ============================================================
// ExternalDragEvent
// ============================================================

/// One phase of an external (OS) drag over a window. Positions are in the
/// window's logical coordinate space (top-left origin), already converted
/// from the platform's native coordinates.
#[derive(Debug, Clone)]
pub enum ExternalDragEvent {
    /// The drag entered the window. `data` is the best-effort payload the
    /// source offers (fully populated where the platform exposes it during
    /// hover — e.g. macOS; possibly empty until drop on backends that only
    /// transfer bytes at drop time). Lets a drop target validate on hover.
    Entered {
        /// Offered payload (files / text / URLs); may be empty until drop.
        data: ExternalDropData,
        /// Entry position in window-logical coordinates.
        position: Point,
    },
    /// The pointer moved while the drag is over the window.
    Moved {
        /// Current position in window-logical coordinates.
        position: Point,
    },
    /// The drag left the window without a drop, and may come back.
    ///
    /// For an app-originated drag currently re-entered into this window this is
    /// **not** the end: the OS drag is still in flight, so the framework parks
    /// the typed payload for whichever window the drag enters next. Use
    /// [`Cancelled`](Self::Cancelled) for an ending.
    Left,
    /// The drag over this window has been **aborted**: no drop will follow, and
    /// nothing is coming back.
    ///
    /// Distinct from [`Left`](Self::Left) in exactly one way, and it is the
    /// reason both exist: a leave re-parks a re-entered app drag's typed
    /// payload because the OS drag continues, while an abort must not — a parked
    /// payload belonging to a drag that has ended could be misclaimed by the
    /// next genuine drag from another application.
    ///
    /// **Which backends produce it.** No OS tells a *destination* that a
    /// foreign drag was aborted rather than merely leaving: `wl_data_device`
    /// sends `leave`, XDND sends `XdndLeave`, OLE calls `DragLeave`, and AppKit
    /// calls `draggingExited:`, in both cases. What a backend does know is that
    /// its own outbound drag has ended while it was over one of this app's
    /// windows, and the Wayland and X11 backends report that here.
    Cancelled,
    /// The user dropped. Carries the extracted payload and the drop position.
    Dropped {
        /// Files / text / URLs / raw MIME bytes extracted from the OS payload.
        data: ExternalDropData,
        /// Drop position in window-logical coordinates.
        position: Point,
    },
    /// An **outbound** (app → OS) drag this window asked for has started: the
    /// platform has the pointer now.
    ///
    /// Posted by a backend whose [`ExternalDndGuard::begin_drag`] answered
    /// [`OsDragStart::Pending`], once its own loop has started the native drag,
    /// and before anything it posts about that drag (an `Entered` over one of
    /// this app's windows). `teksilo-app` routes it to the window's
    /// `WidgetTree::confirm_os_drag`, which hands the in-app session over. A
    /// backend whose pending start fails posts [`Self::DragEnded`] instead.
    DragStarted {
        /// The id `begin_drag` answered `Pending` with, so a late start of an
        /// earlier drag is not credited to the drag waiting now.
        drag: OsDragId,
    },
    /// An **outbound** (app → OS) drag this window started has finished, or a
    /// pending start of one has failed. Posted by the platform backend so the
    /// framework can notify the source widget's `on_drag_ended`.
    DragEnded {
        /// How the OS drag resolved (copy / move into another app, or cancel).
        outcome: DropOutcome,
        /// The id `begin_drag` answered `Pending` with; `None` for a drag the
        /// backend started at once (`Started`).
        drag: Option<OsDragId>,
    },
}

// ============================================================
// ExternalDndEventPayload
// ============================================================

/// Boxed inside `AppEvent::External` when a backend reports a drag phase.
/// `teksilo-app`'s app-event handler downcasts to this type and routes the
/// [`ExternalDragEvent`] to the originating window's `WidgetTree`.
#[derive(Debug)]
pub struct ExternalDndEventPayload {
    /// The window the drag is over. The dispatcher uses this to route the
    /// event to the correct widget tree.
    pub window_id_owner: TeksiloWindowId,
    /// The drag phase.
    pub event: ExternalDragEvent,
}

/// Posted by a backend whose outbound (app → OS) drag is **blocking** (Windows
/// OLE `DoDragDrop` runs its own modal message loop) so it can be run OUTSIDE
/// the in-app event dispatch that started it. [`ExternalDndGuard::begin_drag`]
/// stashes the payload, posts this, and answers [`OsDragStart::Pending`];
/// `teksilo-app`'s
/// `AppEvent::External` arm downcasts it and calls
/// [`ExternalDndHandle::run_pending_outbound_drag`] on the next loop turn — a
/// point where no window is borrowed out of the manager, mirroring how inbound
/// drag events route through the poster rather than re-entrantly inside a
/// platform callback. Non-blocking backends (macOS / Wayland) never post this.
#[derive(Debug)]
pub struct OutboundOsDragRequest {
    /// The window whose guard stashed the outbound payload to export.
    pub window_id: TeksiloWindowId,
}

// ============================================================
// Outbound payload → MIME, shared by the platform backends
// ============================================================

/// MIME types to advertise for an outbound payload, in a stable order.
///
/// Shared by the Wayland and X11 backends (and available to any future one) so
/// an app exports the same set of types no matter which display server it
/// happens to be running under.
#[cfg_attr(
    not(all(unix, not(target_os = "macos"))),
    allow(dead_code, reason = "only the unix backends export via MIME today")
)]
pub(crate) fn outbound_mimes(data: &OutboundDragData) -> Vec<String> {
    let mut mimes: Vec<String> = data.mime.keys().cloned().collect();
    // Canonical types derived from the structured fields, if not already
    // present in the explicit mime map.
    if (!data.files.is_empty() || !data.uris.is_empty())
        && !mimes.iter().any(|m| m == "text/uri-list")
    {
        mimes.push("text/uri-list".to_string());
    }
    if data.text.is_some() && !mimes.iter().any(|m| m == "text/plain") {
        mimes.push("text/plain".to_string());
    }
    mimes
}

/// Bytes for a given advertised MIME type.
///
/// An explicit entry in [`OutboundDragData::mime`] always wins — the app said
/// exactly what those bytes are. Otherwise the canonical types are rendered
/// from the structured fields.
#[cfg_attr(
    not(all(unix, not(target_os = "macos"))),
    allow(dead_code, reason = "only the unix backends export via MIME today")
)]
pub(crate) fn outbound_bytes(data: &OutboundDragData, mime_type: &str) -> Vec<u8> {
    if let Some(bytes) = data.mime.get(mime_type) {
        return bytes.clone();
    }
    match mime_type {
        // `to_uri_list` percent-encodes, which matters: an un-encoded `#` in a
        // filename starts a comment line and an un-encoded newline splits one
        // path into two. It is the exact inverse of
        // `ExternalDropData::from_uri_list`, so a drag between two Teksilo
        // windows round-trips filenames unchanged.
        "text/uri-list" => data.to_uri_list().into_bytes(),
        "text/plain" | "text/plain;charset=utf-8" => {
            data.text.clone().unwrap_or_default().into_bytes()
        }
        _ => Vec::new(),
    }
}

// ============================================================
// TouchSerialSource — which press opened the implicit grab
// ============================================================

/// The most recent press serial per device class, so an outbound drag can be
/// started with the serial belonging to the device that is actually dragging.
///
/// `wl_data_device::start_drag` must be given the serial of an input event that
/// opened the current implicit grab. A mouse opens one with
/// `wl_pointer::button`, a finger with `wl_touch::down` — different objects,
/// different serials, and handing over the wrong one makes the compositor
/// reject the request **silently**: no drag starts and no terminal event
/// arrives, so the framework's outbound bookkeeping is left waiting for a drag
/// that never existed. A backend that only ever bound `wl_pointer` therefore
/// could not export a finger drag at all.
///
/// Kept here rather than in the Wayland backend so it compiles and is tested on
/// every host, the way [`outbound_mimes`] is.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(all(unix, not(target_os = "macos"))),
    allow(
        dead_code,
        reason = "only the Wayland backend needs a per-device press serial"
    )
)]
pub(crate) struct TouchSerialSource {
    /// Serial of the last `wl_pointer::button` press, or 0 if none.
    pointer: u32,
    /// Serial of the last `wl_touch::down`, or 0 if none.
    touch: u32,
    /// Pointer buttons pressed and not yet released.
    pointer_held: u32,
    /// Touch points down and not yet up.
    touch_held: u32,
}

#[cfg_attr(
    not(all(unix, not(target_os = "macos"))),
    allow(
        dead_code,
        reason = "only the Wayland backend needs a per-device press serial"
    )
)]
impl TouchSerialSource {
    /// Record a pointer-button press.
    pub(crate) fn record_pointer(&mut self, serial: u32) {
        self.pointer = serial;
        self.pointer_held = self.pointer_held.saturating_add(1);
    }

    /// Record a pointer-button release.
    pub(crate) fn record_pointer_release(&mut self) {
        self.pointer_held = self.pointer_held.saturating_sub(1);
    }

    /// Record a touch-down.
    pub(crate) fn record_touch(&mut self, serial: u32) {
        self.touch = serial;
        self.touch_held = self.touch_held.saturating_add(1);
    }

    /// Record a touch-up.
    pub(crate) fn record_touch_up(&mut self) {
        self.touch_held = self.touch_held.saturating_sub(1);
    }

    /// Record a touch cancel: the compositor took every touch point away.
    pub(crate) fn record_touch_cancel(&mut self) {
        self.touch_held = 0;
    }

    /// Record that the pointer left or entered one of our surfaces. A button
    /// held across a leave is released where we cannot see it (a drag grab
    /// took it), so nothing is held any more as far as we know.
    pub(crate) fn record_pointer_focus_change(&mut self) {
        self.pointer_held = 0;
    }

    /// Whether a press of `kind`'s device class has ever been seen, held or
    /// not: a device with none can never start a drag (a seat with no touch,
    /// say), which is known before asking the backend thread.
    pub(crate) fn has_press_for(&self, kind: teksilo_tokens::PointerKind) -> bool {
        if kind.is_coarse() {
            self.touch != 0
        } else {
            self.pointer != 0
        }
    }

    /// The serial to start a drag carried by `kind` with ([`Self::serial_for`]),
    /// marking the press as handed to the drag: once `start_drag` converts the
    /// implicit grab into the compositor's drag grab, that press's release
    /// goes to the drag grab and never reaches us, so counting it as still
    /// held would let a later drag start from a press that is long over. A
    /// refused `start_drag` leaves the release to come, which then finds the
    /// count already at zero.
    pub(crate) fn take_for_drag(&mut self, kind: teksilo_tokens::PointerKind) -> Option<u32> {
        let serial = self.serial_for(kind)?;
        if kind.is_coarse() {
            self.touch_held = 0;
        } else {
            self.pointer_held = 0;
        }
        Some(serial)
    }

    /// The serial to start a drag carried by `kind` with, or `None` when no
    /// usable press has been seen, or when that press has been released.
    ///
    /// A released press names no grab any more: the drag was released before
    /// the native start could happen (the start is asked for on a pointer move
    /// and runs on the backend's own thread a few milliseconds later), and a
    /// `start_drag` now would be refused by the compositor without a word.
    ///
    /// A coarse pointer takes the touch serial and a precise one the pointer
    /// serial. Neither falls back to the other: a serial from the wrong device
    /// does not name a grab that device holds, so offering it would trade a
    /// clean "we cannot start this drag" — which the caller reports as a
    /// cancellation, and which the framework cleans up after — for a silent
    /// compositor refusal that ends nothing.
    pub(crate) fn serial_for(&self, kind: teksilo_tokens::PointerKind) -> Option<u32> {
        let (serial, held) = if kind.is_coarse() {
            (self.touch, self.touch_held)
        } else {
            (self.pointer, self.pointer_held)
        };
        (serial != 0 && held > 0).then_some(serial)
    }
}

// ============================================================
// ExternalDndBackend trait + registration guard
// ============================================================

/// RAII guard for one window's OS drop-target registration. Dropping it
/// revokes the registration (e.g. `RevokeDragDrop` on Windows, unregistering
/// the dragging destination on macOS, destroying the `wl_data_device`
/// listener on Wayland). Backends return a boxed guard from
/// [`ExternalDndBackend::attach`]; [`ExternalDndHandle`] holds it for the
/// lifetime of the window.
pub trait ExternalDndGuard {
    /// Start a native OS drag session (app → OS, "outbound") for this window,
    /// exporting `data` and optionally drawing `image` as the drag cursor.
    /// Called when an in-app drag carrying an OS-exportable payload is handed
    /// to the platform (at the window boundary, or on the first pointer move
    /// for `OsHandoff::Eager`).
    ///
    /// The answer says when the native drag runs:
    ///
    /// - [`OsDragStart::Declined`]: nothing started. The default, for a
    ///   backend with no outbound implementation at all; the framework keeps
    ///   the in-app drag alive (it can come back into the window).
    /// - [`OsDragStart::Started`]: the native drag is running now (macOS).
    /// - [`OsDragStart::Pending`]: the backend starts it on its own loop
    ///   (Wayland, X11, Windows), under a fresh [`OsDragId`]. It MUST then
    ///   either post [`ExternalDragEvent::DragStarted`] with that id once the
    ///   drag runs, before anything else it posts about the drag, or, for a
    ///   blocking drag loop, call the `on_start` of
    ///   [`Self::run_pending_outbound_drag`] with it before entering the loop;
    ///   or post [`ExternalDragEvent::DragEnded`] with it if the start fails.
    ///   A [`Self::cancel_drag`] that arrives first means the start must not
    ///   happen (or, if it already has, that the drag ends).
    ///
    /// A failure the backend can see now (no surface to drag from, nothing to
    /// advertise) answers `Declined`, never `Pending` followed by a failure:
    /// a decline keeps the in-app drag, a failure ends it.
    ///
    /// When a started OS drag ends, the backend MUST post an
    /// [`ExternalDragEvent::DragEnded`] through the poster captured at
    /// [`ExternalDndBackend::attach`].
    /// `pointer` is the device carrying the drag, and a backend cannot infer it:
    /// on Wayland `wl_data_device::start_drag` converts the implicit grab named
    /// by the serial it is given, a mouse opens one with `wl_pointer::button` and
    /// a finger with `wl_touch::down`, and the wrong serial is refused in
    /// silence — no drag, and no terminal event to clean up after. See
    /// `docs/drag-and-drop.md` §11.5.
    fn begin_drag(
        &self,
        _data: &OutboundDragData,
        _image: Option<&DragImageData>,
        _pointer: teksilo_tokens::PointerKind,
    ) -> OsDragStart {
        OsDragStart::Declined
    }

    /// Revise the OS's accept state for an **inbound** drag over this window
    /// from the widget tree's verdict.
    ///
    /// A backend has to answer the drag source on its own thread and at once —
    /// XDND requires an `XdndStatus` per `XdndPosition`, and Wayland wants
    /// `wl_data_offer::accept` plus `set_actions` — which is long before the
    /// widget tree has seen the position. So a backend's first answer can only
    /// say whether the *formats* are readable, and without this the OS went on
    /// showing "will accept" over a target that refuses the payload. Both
    /// protocols allow the answer to be revised for the rest of the drag, which
    /// is what this does.
    ///
    /// The operation follows the bit: Copy when accepted, none when refused.
    /// Copy is the only operation Teksilo advertises in either direction.
    ///
    /// Default: no-op, for a backend with no inbound negotiation to revise.
    fn set_drop_accepted(&self, _accepted: bool) {}

    /// Tell the backend this window's HiDPI scale factor.
    ///
    /// [`ExternalDragEvent`] positions are **window-logical**, but X11 speaks
    /// only physical pixels and — unlike Win32's `GetDpiForWindow` or AppKit's
    /// point space — offers no per-window scale to divide by. So the app layer
    /// pushes winit's own answer down: once at attach, and again on every
    /// `ScaleFactorChanged` (dragging the window to a monitor with a different
    /// scale mid-drag would otherwise start reporting drops at the wrong
    /// place).
    ///
    /// Default no-op: every other backend gets the scale from the OS.
    fn set_scale_factor(&self, _scale: f64) {}

    /// Cancel an outbound OS drag: the user pressed Escape, or released the
    /// pointer before a pending start ([`OsDragStart::Pending`]) was confirmed.
    ///
    /// A start still pending must not happen after this. Once a drag runs,
    /// this is only meaningful for backends that drive it themselves rather
    /// than handing it to a modal OS loop. X11 does (it tracks the pointer on
    /// its own connection), so it has no OS-level Escape handling to inherit,
    /// and without this the drag could only end by releasing the button.
    ///
    /// A backend that ends a running drag MUST still post the terminal
    /// [`ExternalDragEvent::DragEnded`] exactly as it would for any other
    /// ending, so the source widget's `on_drag_ended` fires once either way.
    fn cancel_drag(&self) {}

    /// Run a previously-requested **blocking** outbound drag for this window,
    /// synchronously. Called by [`ExternalDndHandle::run_pending_outbound_drag`]
    /// from a `teksilo-app` event-loop turn AFTER [`Self::begin_drag`] answered
    /// [`OsDragStart::Pending`] and stashed the payload, so a blocking platform
    /// drag loop (Windows OLE `DoDragDrop`) runs outside the dispatch that
    /// started it.
    ///
    /// `on_start` MUST be called right before the loop is entered, and not at
    /// all when nothing is left to run (the start was cancelled): it is the
    /// start's confirmation, and it hands the in-app session over before the
    /// loop, which an event posted from here could not do, since the loop
    /// blocks the event loop that would deliver it. When the loop finishes,
    /// this MUST post [`ExternalDragEvent::DragEnded`] through the captured
    /// poster. Default no-op: non-blocking backends start the session from
    /// their own loop and never stash.
    fn run_pending_outbound_drag(&self, _on_start: &mut dyn FnMut(OsDragId)) {}
}

/// A guard that does nothing on drop. Used by [`NoopExternalDndBackend`] and
/// by backends whose registration needs no explicit teardown.
pub struct NoopDndGuard;
impl ExternalDndGuard for NoopDndGuard {}

/// Swappable external-drag backend. One backend instance serves the whole
/// app; [`Self::attach`] is called once per window.
pub trait ExternalDndBackend {
    /// Register this app as the OS drop target for the window identified by
    /// `parent` (its raw window/display handle). For every phase of a drag
    /// over that window the backend MUST post an [`ExternalDndEventPayload`]
    /// — with `window_id_owner` set to `window_id` — through `poster`.
    ///
    /// Returns a guard whose `Drop` revokes the registration. The guard is
    /// held by [`ExternalDndHandle`] until the window closes.
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: TeksiloWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard>;
}

/// Forward through a boxed backend, so `ExternalDndHandle::new(default_backend())`
/// (which returns `Box<dyn ExternalDndBackend>`) type-checks.
impl ExternalDndBackend for Box<dyn ExternalDndBackend> {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: TeksiloWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        (**self).attach(parent, window_id, poster)
    }
}

// ============================================================
// ExternalDndHandle
// ============================================================

struct ExternalDndState {
    backend: RefCell<Box<dyn ExternalDndBackend>>,
    /// Shared, so a guard can be used without holding this map's borrow: the
    /// blocking drag loop runs app code (its start's confirmation) that may
    /// open or close a window, which attaches or detaches a guard.
    guards: RefCell<HashMap<TeksiloWindowId, Rc<dyn ExternalDndGuard>>>,
}

/// Per-app external-drag service. Registered in app-state by
/// `TeksiloAppBuilder::install_external_dnd`; `teksilo-app` calls
/// [`Self::attach`] / [`Self::detach`] from its window lifecycle hooks.
/// Cloneable; clones share the same backend and guard map.
#[derive(Clone)]
pub struct ExternalDndHandle {
    inner: Rc<ExternalDndState>,
}

impl ExternalDndHandle {
    /// Build a handle wrapping the given backend.
    pub fn new<B: ExternalDndBackend + 'static>(backend: B) -> Self {
        Self {
            inner: Rc::new(ExternalDndState {
                backend: RefCell::new(Box::new(backend)),
                guards: RefCell::new(HashMap::new()),
            }),
        }
    }

    /// Register the window as an OS drop target. Idempotent per window: a
    /// second attach for the same `window_id` replaces (and so revokes) the
    /// previous registration.
    pub fn attach(
        &self,
        window_id: TeksiloWindowId,
        parent: ParentHandle,
        poster: Arc<dyn AppEventPoster>,
    ) {
        // Revoke any prior registration first, so a real backend re-registers
        // from a clean slate (RevokeDragDrop before RegisterDragDrop, etc.).
        // `detach` drops the old guard with no outstanding borrow on `guards`.
        self.detach(window_id);
        let guard = self
            .inner
            .backend
            .borrow_mut()
            .attach(parent, window_id, poster);
        self.inner
            .guards
            .borrow_mut()
            .insert(window_id, Rc::from(guard));
    }

    /// Revoke the window's OS drop-target registration (dropping its guard).
    /// Called from `teksilo-app`'s window-close path. No-op if the window was
    /// never attached.
    pub fn detach(&self, window_id: TeksiloWindowId) {
        let guard = self.inner.guards.borrow_mut().remove(&window_id);
        drop(guard);
    }

    /// Number of currently-attached windows. Test/diagnostic helper.
    pub fn attached_count(&self) -> usize {
        self.inner.guards.borrow().len()
    }

    /// Tell `window_id`'s backend the window's current HiDPI scale factor.
    /// See [`ExternalDndGuard::set_scale_factor`]. No-op if not attached.
    pub fn set_scale_factor(&self, window_id: TeksiloWindowId, scale: f64) {
        if let Some(guard) = self.inner.guards.borrow().get(&window_id) {
            guard.set_scale_factor(scale);
        }
    }

    /// Cancel an in-flight outbound OS drag for `window_id` (the user pressed
    /// Escape). See [`ExternalDndGuard::cancel_drag`]. No-op if not attached.
    pub fn cancel_drag(&self, window_id: TeksiloWindowId) {
        if let Some(guard) = self.inner.guards.borrow().get(&window_id) {
            guard.cancel_drag();
        }
    }

    /// Start a native OS (outbound) drag for `window_id`, delegating to that
    /// window's guard. [`OsDragStart::Declined`] if the window isn't attached
    /// or the backend declines (no outbound support). Called from
    /// `teksilo-app`'s `WindowOps::begin_os_drag`. See
    /// [`ExternalDndGuard::begin_drag`].
    pub fn begin_drag(
        &self,
        window_id: TeksiloWindowId,
        data: &OutboundDragData,
        image: Option<&DragImageData>,
        pointer: teksilo_tokens::PointerKind,
    ) -> OsDragStart {
        self.inner
            .guards
            .borrow()
            .get(&window_id)
            .map_or(OsDragStart::Declined, |g| {
                g.begin_drag(data, image, pointer)
            })
    }

    /// Push the widget tree's accept verdict for an inbound OS drag over
    /// `window_id` to its backend. See [`ExternalDndGuard::set_drop_accepted`].
    /// No-op if the window isn't attached.
    pub fn set_drop_accepted(&self, window_id: TeksiloWindowId, accepted: bool) {
        if let Some(guard) = self.inner.guards.borrow().get(&window_id) {
            guard.set_drop_accepted(accepted);
        }
    }

    /// Run the deferred blocking outbound OS drag for `window_id` (Windows OLE
    /// `DoDragDrop`), if its guard stashed one via `begin_drag`. Called from
    /// `teksilo-app` when it receives an [`OutboundOsDragRequest`]; `on_start`
    /// runs right before the blocking loop (see
    /// [`ExternalDndGuard::run_pending_outbound_drag`]). No-op if the window
    /// isn't attached.
    ///
    /// The (blocking) drag loop runs on a shared handle to the guard, used
    /// without holding the `guards` map's borrow, so `on_start`, which runs app
    /// handlers, may open or close a window (attach or detach a guard) without
    /// a borrow conflict.
    pub fn run_pending_outbound_drag(
        &self,
        window_id: TeksiloWindowId,
        on_start: &mut dyn FnMut(OsDragId),
    ) {
        let guard = self.inner.guards.borrow().get(&window_id).cloned();
        if let Some(guard) = guard {
            guard.run_pending_outbound_drag(on_start);
        }
    }
}

impl std::fmt::Debug for ExternalDndHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalDndHandle")
            .field("attached", &self.inner.guards.borrow().len())
            .finish_non_exhaustive()
    }
}

// ============================================================
// NoopExternalDndBackend (unsupported targets)
// ============================================================

/// Backend that registers nothing and never emits events. Used on any target
/// without a raw drop-target implementation. External OS drops simply
/// don't fire; a `DropZone` widget stays usable via its keyboard "Browse…"
/// fallback button.
#[derive(Default)]
pub struct NoopExternalDndBackend;

impl NoopExternalDndBackend {
    /// Build the no-op backend.
    pub fn new() -> Self {
        Self
    }
}

impl ExternalDndBackend for NoopExternalDndBackend {
    fn attach(
        &mut self,
        _parent: ParentHandle,
        _window_id: TeksiloWindowId,
        _poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        Box::new(NoopDndGuard)
    }
}

// ============================================================
// MemoryExternalDndBackend (test backend)
// ============================================================

/// Shared `(window_id, poster)` table held by the test backend and its guards.
type AttachmentList = Arc<std::sync::Mutex<Vec<(TeksiloWindowId, Arc<dyn AppEventPoster>)>>>;

/// In-memory backend for headless tests. Records the `(window_id, poster)` of
/// each attached window so a test can synthesize OS drag phases via
/// [`Self::emit`], which posts an [`ExternalDndEventPayload`] exactly as a real
/// backend would. Cloneable — clones share the same recording, so a test can
/// keep a clone after handing one to [`ExternalDndHandle::new`].
#[derive(Clone, Default)]
pub struct MemoryExternalDndBackend {
    attachments: AttachmentList,
    outbound: Arc<std::sync::Mutex<Vec<OutboundDragData>>>,
    outbound_pointers: Arc<std::sync::Mutex<Vec<teksilo_tokens::PointerKind>>>,
    accepts: Arc<std::sync::Mutex<Vec<bool>>>,
    /// Whether `begin_drag` answers `Pending` rather than `Started`.
    defer_starts: Arc<std::sync::atomic::AtomicBool>,
    /// How many `cancel_drag` calls the guards received.
    cancels: Arc<std::sync::atomic::AtomicUsize>,
    /// The id of every start answered `Pending`, in order.
    pending: Arc<std::sync::Mutex<Vec<OsDragId>>>,
}

/// Guard that removes the window's attachment record on drop, so
/// [`ExternalDndHandle::detach`] is observable in tests via
/// [`MemoryExternalDndBackend::attached_windows`].
pub struct MemoryDndGuard {
    window_id: TeksiloWindowId,
    attachments: AttachmentList,
    outbound: Arc<std::sync::Mutex<Vec<OutboundDragData>>>,
    outbound_pointers: Arc<std::sync::Mutex<Vec<teksilo_tokens::PointerKind>>>,
    accepts: Arc<std::sync::Mutex<Vec<bool>>>,
    defer_starts: Arc<std::sync::atomic::AtomicBool>,
    cancels: Arc<std::sync::atomic::AtomicUsize>,
    pending: Arc<std::sync::Mutex<Vec<OsDragId>>>,
}

impl ExternalDndGuard for MemoryDndGuard {
    fn begin_drag(
        &self,
        data: &OutboundDragData,
        _image: Option<&DragImageData>,
        pointer: teksilo_tokens::PointerKind,
    ) -> OsDragStart {
        // Record the outbound request and report success so tests can assert
        // escalation reached the backend. Test code drives the matching
        // `DragStarted` (when deferred) and `DragEnded` via
        // [`MemoryExternalDndBackend::emit`].
        self.outbound.lock().unwrap().push(data.clone());
        self.outbound_pointers.lock().unwrap().push(pointer);
        let _ = self.window_id;
        if self.defer_starts.load(std::sync::atomic::Ordering::Relaxed) {
            let drag = OsDragId::next();
            self.pending.lock().unwrap().push(drag);
            OsDragStart::Pending(drag)
        } else {
            OsDragStart::Started
        }
    }

    fn set_drop_accepted(&self, accepted: bool) {
        self.accepts.lock().unwrap().push(accepted);
    }

    fn cancel_drag(&self) {
        self.cancels
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for MemoryDndGuard {
    fn drop(&mut self) {
        if let Ok(mut v) = self.attachments.lock() {
            v.retain(|(id, _)| *id != self.window_id);
        }
    }
}

impl MemoryExternalDndBackend {
    /// Build a new empty test backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Synthesize an OS drag phase for `window_id`, posting it through that
    /// window's recorded poster. Returns `false` if the window isn't attached.
    pub fn emit(&self, window_id: TeksiloWindowId, event: ExternalDragEvent) -> bool {
        let poster = {
            let v = self.attachments.lock().unwrap();
            v.iter()
                .find(|(id, _)| *id == window_id)
                .map(|(_, p)| p.clone())
        };
        match poster {
            Some(p) => {
                p.post_external(Box::new(ExternalDndEventPayload {
                    window_id_owner: window_id,
                    event,
                }) as Box<dyn Any + Send>);
                true
            }
            None => false,
        }
    }

    /// Window ids currently attached. Test helper.
    pub fn attached_windows(&self) -> Vec<TeksiloWindowId> {
        self.attachments
            .lock()
            .unwrap()
            .iter()
            .map(|(id, _)| *id)
            .collect()
    }

    /// Outbound (app → OS) drags requested via `begin_drag`, in order. Test
    /// helper for the escalation path.
    pub fn outbound_drags(&self) -> Vec<OutboundDragData> {
        self.outbound.lock().unwrap().clone()
    }

    /// The pointer kind each outbound drag was started with, in order.
    pub fn outbound_pointers(&self) -> Vec<teksilo_tokens::PointerKind> {
        self.outbound_pointers.lock().unwrap().clone()
    }

    /// Every accept verdict pushed via
    /// [`ExternalDndGuard::set_drop_accepted`], in order.
    pub fn drop_accepts(&self) -> Vec<bool> {
        self.accepts.lock().unwrap().clone()
    }

    /// Make `begin_drag` answer [`OsDragStart::Pending`], as the Wayland, X11
    /// and Windows backends do, instead of [`OsDragStart::Started`]. A test
    /// then confirms the start with [`ExternalDragEvent::DragStarted`] through
    /// [`Self::emit`], naming the drag by its id from [`Self::pending_drags`].
    pub fn defer_starts(&self, defer: bool) {
        self.defer_starts
            .store(defer, std::sync::atomic::Ordering::Relaxed);
    }

    /// The id of every start answered `Pending`, in order.
    pub fn pending_drags(&self) -> Vec<OsDragId> {
        self.pending.lock().unwrap().clone()
    }

    /// How many times a guard was asked to cancel an outbound drag.
    pub fn cancelled_drags(&self) -> usize {
        self.cancels.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl ExternalDndBackend for MemoryExternalDndBackend {
    fn attach(
        &mut self,
        _parent: ParentHandle,
        window_id: TeksiloWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        self.attachments.lock().unwrap().push((window_id, poster));
        Box::new(MemoryDndGuard {
            window_id,
            attachments: self.attachments.clone(),
            outbound: self.outbound.clone(),
            outbound_pointers: self.outbound_pointers.clone(),
            accepts: self.accepts.clone(),
            defer_starts: self.defer_starts.clone(),
            cancels: self.cancels.clone(),
            pending: self.pending.clone(),
        })
    }
}

// ============================================================
// Default backend factory
// ============================================================

/// Routes each window to the Wayland or X11 backend by its live display
/// handle.
///
/// Both are compiled in on Linux/BSD and both are reachable at runtime — an
/// app can be an X11 client in a Wayland session (XWayland), and `DISPLAY` is
/// set in essentially every Wayland session, so the environment cannot decide
/// this. The handle can: it *is* the backend winit created.
#[cfg(all(unix, not(target_os = "macos")))]
struct UnixExternalDndBackend {
    wayland: wayland::WaylandExternalDndBackend,
    x11: x11::X11ExternalDndBackend,
}

#[cfg(all(unix, not(target_os = "macos")))]
impl ExternalDndBackend for UnixExternalDndBackend {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: TeksiloWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        // One shared discriminator, also used by the title-bar host factory, so
        // the two subsystems can never disagree about the same window.
        match crate::window_system::window_system_for_display_handle(&parent.raw_display_handle()) {
            crate::window_system::WindowSystem::Wayland => {
                self.wayland.attach(parent, window_id, poster)
            }
            crate::window_system::WindowSystem::X11 => self.x11.attach(parent, window_id, poster),
            crate::window_system::WindowSystem::Unknown => Box::new(NoopDndGuard),
        }
    }
}

/// The default external-drag backend for the current target.
///
/// Every desktop target now has a real backend: OLE on Windows,
/// `NSDraggingDestination` on macOS, `wl_data_device` on Wayland, XDND on X11.
/// [`NoopExternalDndBackend`] remains for targets with no drop-target
/// implementation at all. `TeksiloAppBuilder::install_external_dnd` uses this.
pub fn default_backend() -> Box<dyn ExternalDndBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsExternalDndBackend::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsExternalDndBackend::new())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Box::new(UnixExternalDndBackend {
            wayland: wayland::WaylandExternalDndBackend::new(),
            x11: x11::X11ExternalDndBackend::new(),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        Box::new(NoopExternalDndBackend::new())
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use teksilo_core::SubscriptionId;

    /// Test poster capturing every posted External payload.
    struct CapturingPoster {
        captured: Mutex<Vec<Box<dyn Any + Send>>>,
    }

    impl CapturingPoster {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                captured: Mutex::new(Vec::new()),
            })
        }
        fn drain(&self) -> Vec<Box<dyn Any + Send>> {
            std::mem::take(&mut *self.captured.lock().unwrap())
        }
    }

    impl AppEventPoster for CapturingPoster {
        fn post_subscription_event(&self, _sub_id: SubscriptionId, _event: Box<dyn Any + Send>) {}
        fn post_external(&self, payload: Box<dyn Any + Send>) {
            self.captured.lock().unwrap().push(payload);
        }
    }

    fn fake_parent() -> ParentHandle {
        // ParentHandle has no public synthetic constructor; tests only need a
        // value to pass through (the Memory/Noop backends ignore it). Build one
        // from a winit-less raw handle via from_window over a dummy that yields
        // an Xlib handle is overkill — instead use the documented escape hatch:
        // ParentHandle implements HasWindowHandle by storing raw handles, but
        // the only constructor is from_window. We therefore exercise attach
        // through a tiny stand-in window.
        DummyWindow::parent()
    }

    /// Minimal `HasWindowHandle + HasDisplayHandle` stand-in so tests can build
    /// a `ParentHandle` without a real window.
    struct DummyWindow;
    impl DummyWindow {
        fn parent() -> ParentHandle {
            ParentHandle::from_window(&DummyWindow).expect("dummy parent handle")
        }
    }
    impl raw_window_handle::HasWindowHandle for DummyWindow {
        fn window_handle(
            &self,
        ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            // A stable, never-dereferenced raw handle. Backends under test
            // (Memory / Noop) never read it.
            use raw_window_handle::{RawWindowHandle, WindowHandle, XlibWindowHandle};
            let raw = RawWindowHandle::Xlib(XlibWindowHandle::new(1));
            // SAFETY: the handle is only stored, never used to touch the OS,
            // and `self` outlives the borrow within this call.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }
    impl raw_window_handle::HasDisplayHandle for DummyWindow {
        fn display_handle(
            &self,
        ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
            use raw_window_handle::{DisplayHandle, RawDisplayHandle, XlibDisplayHandle};
            let raw = RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0));
            // SAFETY: same as window_handle above.
            Ok(unsafe { DisplayHandle::borrow_raw(raw) })
        }
    }

    fn win(n: u64) -> TeksiloWindowId {
        TeksiloWindowId::new(n)
    }

    #[test]
    fn handle_attaches_and_detaches() {
        let backend = MemoryExternalDndBackend::new();
        let handle = ExternalDndHandle::new(backend.clone());
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        handle.attach(win(1), fake_parent(), poster.clone());
        assert_eq!(handle.attached_count(), 1);
        assert_eq!(backend.attached_windows(), vec![win(1)]);

        handle.detach(win(1));
        assert_eq!(handle.attached_count(), 0);
        // The Memory guard's Drop removed the attachment record.
        assert!(backend.attached_windows().is_empty());
    }

    #[test]
    fn reattach_replaces_previous_guard() {
        let backend = MemoryExternalDndBackend::new();
        let handle = ExternalDndHandle::new(backend.clone());
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        handle.attach(win(1), fake_parent(), poster.clone());
        handle.attach(win(1), fake_parent(), poster.clone());
        // One window tracked, even though attach ran twice.
        assert_eq!(handle.attached_count(), 1);
        // The old guard's Drop ran (removing its record); the new attach added
        // one back — net one attachment.
        assert_eq!(backend.attached_windows(), vec![win(1)]);
    }

    #[test]
    fn emit_posts_event_for_attached_window() {
        let backend = MemoryExternalDndBackend::new();
        let handle = ExternalDndHandle::new(backend.clone());
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();
        handle.attach(win(3), fake_parent(), poster);

        let data = ExternalDropData {
            files: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        assert!(backend.emit(
            win(3),
            ExternalDragEvent::Dropped {
                data,
                position: Point::new(12.0, 34.0),
            },
        ));

        let mut posted = cap.drain();
        assert_eq!(posted.len(), 1);
        let payload = *posted
            .pop()
            .unwrap()
            .downcast::<ExternalDndEventPayload>()
            .expect("payload type matches");
        assert_eq!(payload.window_id_owner, win(3));
        match payload.event {
            ExternalDragEvent::Dropped { data, position } => {
                assert_eq!(data.files, vec![PathBuf::from("/tmp/a.png")]);
                assert!((position.x - 12.0).abs() < 0.01 && (position.y - 34.0).abs() < 0.01);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn emit_for_unattached_window_is_noop() {
        let backend = MemoryExternalDndBackend::new();
        let _handle = ExternalDndHandle::new(backend.clone());
        assert!(!backend.emit(win(99), ExternalDragEvent::Left));
    }

    // ------------------------------------------------------------------
    // P31: which press serial starts an outbound drag
    // ------------------------------------------------------------------

    /// A drag carried by a finger takes the **touch-down** serial.
    ///
    /// `wl_data_device::start_drag` converts the implicit grab named by the
    /// serial it is given. A finger's grab was opened by `wl_touch::down`, so a
    /// pointer-button serial names a grab the finger does not hold, and the
    /// compositor refuses the request without a word — no drag, and no terminal
    /// event to clean up after.
    #[test]
    fn a_touch_originated_drag_selects_the_touch_serial() {
        use teksilo_tokens::{PenKind, PointerKind};

        let mut serials = TouchSerialSource::default();
        serials.record_pointer(11);
        serials.record_touch(22);

        assert_eq!(serials.serial_for(PointerKind::Touch), Some(22));
        assert_eq!(serials.serial_for(PointerKind::Mouse), Some(11));
        assert_eq!(serials.serial_for(PointerKind::Pen(PenKind::Pen)), Some(11));
        assert_eq!(serials.serial_for(PointerKind::Unknown), Some(11));
    }

    /// A newer press of the same class replaces the older one; the other class
    /// is untouched. A drag is started by the most recent press of the device
    /// carrying it, and a mouse resting with a button held while a finger taps
    /// must not have its serial overwritten.
    #[test]
    fn each_device_class_keeps_its_own_latest_press() {
        use teksilo_tokens::PointerKind;

        let mut serials = TouchSerialSource::default();
        serials.record_pointer(1);
        serials.record_touch(2);
        serials.record_touch(3);
        assert_eq!(serials.serial_for(PointerKind::Touch), Some(3));
        assert_eq!(serials.serial_for(PointerKind::Mouse), Some(1));
    }

    /// No serial for the device asked about means **no serial**, not the other
    /// device's.
    ///
    /// A seat with no touch capability, or a touch-down not yet dispatched on
    /// the backend thread, has to produce a clean refusal: the caller reports it
    /// as a cancellation and the framework tears the outbound bookkeeping down.
    /// Substituting the pointer's serial would trade that for a silent
    /// compositor refusal, which ends nothing and leaks the parked payload.
    #[test]
    fn a_device_with_no_press_yields_no_serial_rather_than_the_other_ones() {
        use teksilo_tokens::PointerKind;

        let mut serials = TouchSerialSource::default();
        serials.record_pointer(7);
        assert_eq!(serials.serial_for(PointerKind::Touch), None);

        let mut serials = TouchSerialSource::default();
        serials.record_touch(7);
        assert_eq!(serials.serial_for(PointerKind::Mouse), None);

        assert_eq!(
            TouchSerialSource::default().serial_for(PointerKind::Mouse),
            None,
        );
    }

    /// A press released before the backend thread runs the start yields no
    /// serial, so the start is reported as cancelled instead of being refused
    /// by the compositor in silence. An `OsHandoff::Eager` drag asks for the
    /// start on its first move, so a quick release can beat the thread.
    #[test]
    fn a_released_press_yields_no_serial() {
        use teksilo_tokens::PointerKind;

        let mut serials = TouchSerialSource::default();
        serials.record_pointer(5);
        serials.record_pointer_release();
        assert_eq!(serials.serial_for(PointerKind::Mouse), None);
        serials.record_pointer(6);
        assert_eq!(serials.serial_for(PointerKind::Mouse), Some(6));

        // Two fingers down, one lifted: the other still holds a grab.
        serials.record_touch(8);
        serials.record_touch(9);
        serials.record_touch_up();
        assert_eq!(serials.serial_for(PointerKind::Touch), Some(9));
        serials.record_touch_up();
        assert_eq!(serials.serial_for(PointerKind::Touch), None);
        // A stray release never underflows into a held state.
        serials.record_touch_up();
        serials.record_touch(10);
        assert_eq!(serials.serial_for(PointerKind::Touch), Some(10));
    }

    /// Two exports in a row on Wayland: the first `start_drag` succeeds, so
    /// the compositor's drag grab takes its press's release and we never see
    /// it. The second press is then released before the backend thread gets to
    /// its start, and that must still yield no serial (the start is reported
    /// cancelled), not the first press's stale one.
    #[test]
    fn a_press_handed_to_a_drag_is_no_longer_held() {
        use teksilo_tokens::PointerKind;

        let mut serials = TouchSerialSource::default();
        serials.record_pointer(1);
        assert_eq!(serials.take_for_drag(PointerKind::Mouse), Some(1));
        // The first drag's release goes to the drag grab: never recorded.
        serials.record_pointer(2);
        serials.record_pointer_release();
        assert_eq!(
            serials.take_for_drag(PointerKind::Mouse),
            None,
            "the second press is over"
        );

        // The same for a finger.
        serials.record_touch(3);
        assert_eq!(serials.take_for_drag(PointerKind::Touch), Some(3));
        serials.record_touch(4);
        serials.record_touch_up();
        assert_eq!(serials.take_for_drag(PointerKind::Touch), None);

        // A pointer leave also drops a press we will not see released.
        serials.record_pointer(5);
        serials.record_pointer_focus_change();
        assert_eq!(serials.serial_for(PointerKind::Mouse), None);
        // Yet a press was seen, so the guard does not decline up front.
        assert!(serials.has_press_for(PointerKind::Mouse));
        assert!(!TouchSerialSource::default().has_press_for(PointerKind::Touch));
    }

    /// A deferred memory backend answers `Pending` under a fresh id each time
    /// and counts cancels, for tests of the two-phase start.
    #[test]
    fn the_memory_backend_can_defer_its_starts() {
        use teksilo_tokens::PointerKind;

        let backend = MemoryExternalDndBackend::new();
        let handle = ExternalDndHandle::new(backend.clone());
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();
        handle.attach(win(4), fake_parent(), poster);
        let data = OutboundDragData {
            text: Some("hi".to_string()),
            ..Default::default()
        };
        backend.defer_starts(true);
        let first = handle.begin_drag(win(4), &data, None, PointerKind::Mouse);
        let second = handle.begin_drag(win(4), &data, None, PointerKind::Mouse);
        let ids = backend.pending_drags();
        assert_eq!(first, OsDragStart::Pending(ids[0]));
        assert_eq!(second, OsDragStart::Pending(ids[1]));
        assert_ne!(ids[0], ids[1], "each start has its own id");
        handle.cancel_drag(win(4));
        assert_eq!(backend.cancelled_drags(), 1);
    }

    /// The handle forwards the dragging device to the window's guard, and the
    /// widget tree's accept verdict with it.
    #[test]
    fn the_handle_forwards_the_device_and_the_accept_verdict() {
        use teksilo_tokens::PointerKind;

        let backend = MemoryExternalDndBackend::new();
        let handle = ExternalDndHandle::new(backend.clone());
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();
        handle.attach(win(4), fake_parent(), poster);

        let data = OutboundDragData {
            text: Some("hi".to_string()),
            ..Default::default()
        };
        assert_eq!(
            handle.begin_drag(win(4), &data, None, PointerKind::Touch),
            OsDragStart::Started
        );
        assert_eq!(backend.outbound_pointers(), vec![PointerKind::Touch]);

        handle.set_drop_accepted(win(4), false);
        handle.set_drop_accepted(win(4), true);
        assert_eq!(backend.drop_accepts(), vec![false, true]);

        // An unattached window swallows both without panicking.
        handle.set_drop_accepted(win(99), true);
        assert_eq!(
            handle.begin_drag(win(99), &data, None, PointerKind::Mouse),
            OsDragStart::Declined
        );
        assert_eq!(backend.drop_accepts(), vec![false, true]);
    }

    #[test]
    fn noop_backend_attaches_without_emitting() {
        let handle = ExternalDndHandle::new(NoopExternalDndBackend::new());
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();
        handle.attach(win(1), fake_parent(), poster);
        assert_eq!(handle.attached_count(), 1);
        handle.detach(win(1));
        assert_eq!(handle.attached_count(), 0);
        assert!(cap.drain().is_empty());
    }
}
