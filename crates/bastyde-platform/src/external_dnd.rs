// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! External (OS) drag-and-drop service.
//!
//! Lets a window accept drops that originate **outside** the application —
//! files dragged from the file manager, or text / URLs dragged from another
//! app — and feed them into the same drag pipeline used for in-app drags
//! ([`bastyde_core::WidgetTree::begin_external_drag`] et al.).
//!
//! Three concerns are separated, mirroring [`crate::file_dialog`]:
//!
//! - **Trait surface** — [`ExternalDndBackend`] is the swappable platform
//!   abstraction. A backend registers itself as the OS drop target for a
//!   window and, for each phase of a drag, posts an [`ExternalDndEventPayload`]
//!   through [`bastyde_core::AppEventPoster::post_external`].
//! - **Handle** — [`ExternalDndHandle`] is the per-app service registered in
//!   app-state. It owns the backend and the per-window registration guards.
//!   `bastyde-app` calls [`ExternalDndHandle::attach`] when a window is created
//!   and [`ExternalDndHandle::detach`] when it closes.
//! - **Event delivery** — `bastyde-app` picks the payload up in its
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
//! macOS, `wl_data_device` on Wayland), all of which provide position and
//! arbitrary data formats. X11 is out of scope and uses [`NoopExternalDndBackend`].
//!
//! # Threading
//!
//! All four platform drop targets deliver their callbacks on the UI thread,
//! so backends post events synchronously from there. The payload is still
//! routed through [`bastyde_core::AppEventPoster::post_external`] (the same
//! channel as file dialogs) so the borrow of the window's tree happens in one
//! well-defined place in the event loop rather than re-entrantly inside a
//! platform callback.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use bastyde_canvas::Point;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;
use bastyde_core::{DragImageData, DropOutcome, ExternalDropData, OutboundDragData};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;

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
    /// The drag left the window, or the OS cancelled the operation, without
    /// a drop.
    Left,
    /// The user dropped. Carries the extracted payload and the drop position.
    Dropped {
        /// Files / text / URLs / raw MIME bytes extracted from the OS payload.
        data: ExternalDropData,
        /// Drop position in window-logical coordinates.
        position: Point,
    },
    /// An **outbound** (app → OS) drag this window started has finished. Posted
    /// by the platform backend's drag-source callback so the framework can
    /// notify the source widget's `on_drag_ended`.
    DragEnded {
        /// How the OS drag resolved (copy / move into another app, or cancel).
        outcome: DropOutcome,
    },
}

// ============================================================
// ExternalDndEventPayload
// ============================================================

/// Boxed inside `AppEvent::External` when a backend reports a drag phase.
/// `bastyde-app`'s app-event handler downcasts to this type and routes the
/// [`ExternalDragEvent`] to the originating window's `WidgetTree`.
#[derive(Debug)]
pub struct ExternalDndEventPayload {
    /// The window the drag is over. The dispatcher uses this to route the
    /// event to the correct widget tree.
    pub window_id_owner: BastydeWindowId,
    /// The drag phase.
    pub event: ExternalDragEvent,
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
    /// Called when an in-app drag escalates past the window boundary carrying
    /// an OS-exportable payload.
    ///
    /// Returns `true` if a native session actually started. The default is a
    /// no-op returning `false` — outbound is only implemented on macOS and
    /// Wayland; Windows / X11 / the test sink decline, and the framework then
    /// keeps the in-app drag alive (it can come back into the window).
    ///
    /// When the OS drag ends, the backend MUST post an
    /// [`ExternalDragEvent::DragEnded`] through the poster captured at
    /// [`ExternalDndBackend::attach`].
    fn begin_drag(&self, _data: &OutboundDragData, _image: Option<&DragImageData>) -> bool {
        false
    }
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
        window_id: BastydeWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard>;
}

/// Forward through a boxed backend, so `ExternalDndHandle::new(default_backend())`
/// (which returns `Box<dyn ExternalDndBackend>`) type-checks.
impl ExternalDndBackend for Box<dyn ExternalDndBackend> {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: BastydeWindowId,
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
    guards: RefCell<HashMap<BastydeWindowId, Box<dyn ExternalDndGuard>>>,
}

/// Per-app external-drag service. Registered in app-state by
/// `BastydeAppBuilder::install_external_dnd`; `bastyde-app` calls
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
        window_id: BastydeWindowId,
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
        self.inner.guards.borrow_mut().insert(window_id, guard);
    }

    /// Revoke the window's OS drop-target registration (dropping its guard).
    /// Called from `bastyde-app`'s window-close path. No-op if the window was
    /// never attached.
    pub fn detach(&self, window_id: BastydeWindowId) {
        let guard = self.inner.guards.borrow_mut().remove(&window_id);
        drop(guard);
    }

    /// Number of currently-attached windows. Test/diagnostic helper.
    pub fn attached_count(&self) -> usize {
        self.inner.guards.borrow().len()
    }

    /// Start a native OS (outbound) drag for `window_id`, delegating to that
    /// window's guard. Returns `true` if a native session started, `false` if
    /// the window isn't attached or the backend declines (no outbound
    /// support). Called from `bastyde-app`'s `WindowOps::begin_os_drag`.
    pub fn begin_drag(
        &self,
        window_id: BastydeWindowId,
        data: &OutboundDragData,
        image: Option<&DragImageData>,
    ) -> bool {
        self.inner
            .guards
            .borrow()
            .get(&window_id)
            .map(|g| g.begin_drag(data, image))
            .unwrap_or(false)
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
// NoopExternalDndBackend (X11 / unsupported targets)
// ============================================================

/// Backend that registers nothing and never emits events. Used on X11 and any
/// target without a raw drop-target implementation. External OS drops simply
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
        _window_id: BastydeWindowId,
        _poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        Box::new(NoopDndGuard)
    }
}

// ============================================================
// MemoryExternalDndBackend (test backend)
// ============================================================

/// Shared `(window_id, poster)` table held by the test backend and its guards.
type AttachmentList = Arc<std::sync::Mutex<Vec<(BastydeWindowId, Arc<dyn AppEventPoster>)>>>;

/// In-memory backend for headless tests. Records the `(window_id, poster)` of
/// each attached window so a test can synthesize OS drag phases via
/// [`Self::emit`], which posts an [`ExternalDndEventPayload`] exactly as a real
/// backend would. Cloneable — clones share the same recording, so a test can
/// keep a clone after handing one to [`ExternalDndHandle::new`].
#[derive(Clone, Default)]
pub struct MemoryExternalDndBackend {
    attachments: AttachmentList,
    outbound: Arc<std::sync::Mutex<Vec<OutboundDragData>>>,
}

/// Guard that removes the window's attachment record on drop, so
/// [`ExternalDndHandle::detach`] is observable in tests via
/// [`MemoryExternalDndBackend::attached_windows`].
pub struct MemoryDndGuard {
    window_id: BastydeWindowId,
    attachments: AttachmentList,
    outbound: Arc<std::sync::Mutex<Vec<OutboundDragData>>>,
}

impl ExternalDndGuard for MemoryDndGuard {
    fn begin_drag(&self, data: &OutboundDragData, _image: Option<&DragImageData>) -> bool {
        // Record the outbound request and report success so tests can assert
        // escalation reached the backend. Test code drives the matching
        // `DragEnded` via [`MemoryExternalDndBackend::emit`].
        self.outbound.lock().unwrap().push(data.clone());
        let _ = self.window_id;
        true
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
    pub fn emit(&self, window_id: BastydeWindowId, event: ExternalDragEvent) -> bool {
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
    pub fn attached_windows(&self) -> Vec<BastydeWindowId> {
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
}

impl ExternalDndBackend for MemoryExternalDndBackend {
    fn attach(
        &mut self,
        _parent: ParentHandle,
        window_id: BastydeWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        self.attachments.lock().unwrap().push((window_id, poster));
        Box::new(MemoryDndGuard {
            window_id,
            attachments: self.attachments.clone(),
            outbound: self.outbound.clone(),
        })
    }
}

// ============================================================
// Default backend factory
// ============================================================

/// The default external-drag backend for the current target.
///
/// Windows / macOS / Wayland get their raw platform backends; X11 and every
/// other target get [`NoopExternalDndBackend`]. `BastydeAppBuilder::install_external_dnd`
/// uses this.
pub fn default_backend() -> Box<dyn ExternalDndBackend> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacOsExternalDndBackend::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsExternalDndBackend::new())
    }
    // Linux: the Wayland backend self-detects the surface type and is a no-op
    // under X11 (it returns an inert guard when the display isn't Wayland).
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Box::new(wayland::WaylandExternalDndBackend::new())
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
    use bastyde_core::SubscriptionId;
    use std::path::PathBuf;
    use std::sync::Mutex;

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

    fn win(n: u64) -> BastydeWindowId {
        BastydeWindowId::new(n)
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
