//! Native file-dialog service.
//!
//! This module provides an async, parent-aware, testable file-dialog
//! API. Three concerns are separated:
//!
//! - **Trait surface** — [`FileDialogBackend`] is the swappable
//!   abstraction (rfd, mock, custom). Mirrors the `ClipboardBackend`
//!   pattern.
//! - **Handle** — [`FileDialogHandle`] is the per-app service
//!   registered in app-state. Holds an `Rc<RefCell<dyn FileDialogBackend>>`
//!   plus a pending-callbacks map keyed by [`RequestId`]. Cloneable.
//! - **Result delivery** — backend posts a
//!   [`FileDialogEventPayload`] via [`fern_core::AppEventPoster::post_external`];
//!   `fern-app` picks the payload up in its `AppEvent::External` arm,
//!   routes it to the originating window's `WidgetTree`, and invokes
//!   [`FileDialogHandle::deliver`] which pops the callback and calls
//!   it with a fully built `EventContext`.
//!
//! # Threading
//!
//! The OS dialog runs on its native UI thread (e.g. macOS dispatches
//! to the AppKit main run loop internally; Linux uses an XDG portal
//! D-Bus call; Windows uses COM). The `rfd::AsyncFileDialog` future
//! is `Send` across all rfd-supported platforms, so it is polled by
//! an `async-std` worker thread spawned in [`RfdAsyncBackend`]. The
//! result is sent back to the UI thread as an
//! [`AppEvent::External`](fern_core::AppEvent::External) and the
//! callback runs on the main thread inside an `EventContext` —
//! handlers can `ctx.send_intent(...)`, mutate signals, open windows,
//! exactly as they would from a normal pointer event.
//!
//! # Window safety
//!
//! Each pending callback is tagged with the originating window's
//! `FernWindowId`. When that window closes, [`FileDialogHandle::purge_window`]
//! (called by `fern-app`'s window-close hook) drops the callback box
//! before the widget tree is torn down. A worker-thread future that
//! resolves after window close still arrives at the dispatcher, but
//! `deliver` finds no pending entry and silently drops the result —
//! no panic, no use-after-free.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use fern_core::raw_handle::ParentHandle;
use fern_core::widget::EventContext;
use fern_core::window::FernWindowId;

// ============================================================
// RequestId
// ============================================================

/// Unique id for one in-flight file-dialog request, allocated by
/// [`FileDialogHandle`] at submit time.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct RequestId(u64);

// ============================================================
// FileDialogResult
// ============================================================

/// Outcome of a file-dialog request, delivered to the result callback.
#[derive(Debug, Clone)]
pub enum FileDialogResult {
    /// Open-single-file: `Some(path)` if the user picked a file,
    /// `None` if they cancelled.
    File(Option<PathBuf>),

    /// Open-multiple-files: empty `Vec` if cancelled or no selection.
    Files(Vec<PathBuf>),

    /// Pick-folder: `Some(path)` on selection, `None` on cancel.
    Folder(Option<PathBuf>),

    /// Save-file: `Some(path)` on confirm, `None` on cancel.
    Saved(Option<PathBuf>),

    /// Backend or OS error. Rare; expected paths return Cancelled
    /// rather than `Error`.
    Error(String),
}

// ============================================================
// FileDialogRequest
// ============================================================

/// Kind of dialog to open. Picked by the constructor used:
/// [`FileDialogRequest::pick_file`], [`FileDialogRequest::pick_files`],
/// [`FileDialogRequest::pick_folder`], or [`FileDialogRequest::save_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    PickFile,
    PickFiles,
    PickFolder,
    SaveFile,
}

/// One file-extension filter row in the dialog's filter dropdown.
#[derive(Debug, Clone)]
pub struct FileFilter {
    /// Human-readable label shown in the dropdown (e.g. `"Images"`).
    pub label: String,
    /// Extension list without leading dots (e.g. `["png", "jpg"]`).
    pub extensions: Vec<String>,
}

/// Builder describing one file-dialog request.
///
/// Construct via [`Self::pick_file`] / [`Self::pick_files`] /
/// [`Self::pick_folder`] / [`Self::save_file`]; chain options;
/// hand to [`FileDialogHandle::submit`] (or call one of the
/// `EventContext::pick_*` convenience methods).
#[derive(Debug, Clone)]
pub struct FileDialogRequest {
    kind: DialogKind,
    title: Option<String>,
    starting_dir: Option<PathBuf>,
    default_file_name: Option<String>,
    filters: Vec<FileFilter>,
    parent: Option<ParentHandle>,
}

impl FileDialogRequest {
    fn new(kind: DialogKind) -> Self {
        Self {
            kind,
            title: None,
            starting_dir: None,
            default_file_name: None,
            filters: Vec::new(),
            parent: None,
        }
    }

    /// Build an open-single-file dialog request.
    pub fn pick_file() -> Self {
        Self::new(DialogKind::PickFile)
    }

    /// Build an open-multiple-files dialog request.
    pub fn pick_files() -> Self {
        Self::new(DialogKind::PickFiles)
    }

    /// Build a pick-folder dialog request.
    pub fn pick_folder() -> Self {
        Self::new(DialogKind::PickFolder)
    }

    /// Build a save-file dialog request.
    pub fn save_file() -> Self {
        Self::new(DialogKind::SaveFile)
    }

    /// Set the dialog's title (window caption on most platforms).
    #[must_use]
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = Some(t.into());
        self
    }

    /// Set the directory the dialog opens in.
    #[must_use]
    pub fn starting_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.starting_dir = Some(p.into());
        self
    }

    /// Set the default file name pre-filled in the save dialog.
    /// No-op for open / pick-folder kinds (kept on the request so
    /// callers can prepare a single builder regardless of kind).
    #[must_use]
    pub fn default_file_name(mut self, n: impl Into<String>) -> Self {
        self.default_file_name = Some(n.into());
        self
    }

    /// Add an extension filter row (e.g. `"Images"`, `&["png", "jpg"]`).
    /// Extensions are case-insensitive on platforms that natively
    /// support filtering; do not include leading dots.
    #[must_use]
    pub fn add_filter(mut self, label: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push(FileFilter {
            label: label.into(),
            extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
        });
        self
    }

    /// Stamp the parent window handle. Called by the
    /// `EventContext::pick_*` convenience methods — apps that submit
    /// a request directly via [`FileDialogHandle::submit`] are
    /// responsible for providing the parent handle themselves.
    #[must_use]
    pub fn with_parent(mut self, p: ParentHandle) -> Self {
        self.parent = Some(p);
        self
    }

    /// Validate filter extensions. Called by [`FileDialogHandle::submit`]
    /// before dispatch. Returns the first problem found:
    ///
    /// - empty extensions list on a filter,
    /// - extension containing a leading dot, slash, or whitespace.
    pub fn validate(&self) -> Result<(), String> {
        for f in &self.filters {
            if f.extensions.is_empty() {
                return Err(format!("filter {:?} has no extensions", f.label));
            }
            for ext in &f.extensions {
                if ext.is_empty() {
                    return Err(format!("filter {:?} has an empty extension", f.label));
                }
                if ext.starts_with('.') {
                    return Err(format!(
                        "filter {:?} extension {ext:?} must not start with a dot",
                        f.label
                    ));
                }
                if ext.chars().any(|c| c.is_whitespace() || c == '/' || c == '\\') {
                    return Err(format!(
                        "filter {:?} extension {ext:?} contains whitespace or path separator",
                        f.label
                    ));
                }
            }
        }
        Ok(())
    }

    fn kind(&self) -> DialogKind {
        self.kind
    }
}

// ============================================================
// FileDialogEventPayload
// ============================================================

/// Boxed inside `AppEvent::External` when a backend completes a
/// dialog. `fern-app`'s app-event handler downcasts to this type and
/// routes to [`FileDialogHandle::deliver`].
pub struct FileDialogEventPayload {
    /// Identifies which pending callback to invoke.
    pub request_id: RequestId,
    /// The window the request was submitted from. The dispatcher
    /// uses this to route delivery to the correct widget tree.
    pub window_id_owner: FernWindowId,
    /// The OS dialog's outcome.
    pub result: FileDialogResult,
}

// ============================================================
// FileDialogBackend trait
// ============================================================

/// Swappable file-dialog backend. Mirrors the `ClipboardBackend`
/// pattern.
///
/// The real backend ([`RfdAsyncBackend`] behind the `rfd-backend`
/// feature) drives an `rfd::AsyncFileDialog` future on a worker
/// thread; the test backend ([`MemoryFileDialog`]) returns scripted
/// results synchronously.
pub trait FileDialogBackend {
    /// Spawn an async pick/save/folder request. The backend MUST
    /// eventually deliver the result by calling
    /// [`AppEventPoster::post_external`](fern_core::AppEventPoster::post_external)
    /// on the supplied poster, with a boxed [`FileDialogEventPayload`]
    /// whose `request_id` matches the argument and whose
    /// `window_id_owner` is set to `window_id`.
    fn dispatch(
        &mut self,
        request_id: RequestId,
        window_id: FernWindowId,
        request: FileDialogRequest,
        poster: Arc<dyn fern_core::AppEventPoster>,
    );
}

// ============================================================
// FileDialogHandle
// ============================================================

/// Boxed callback waiting for an in-flight dialog to resolve.
type ResultCallback = Box<dyn FnOnce(FileDialogResult, &mut EventContext)>;

struct PendingCallback {
    window_id: FernWindowId,
    callback: ResultCallback,
}

struct FileDialogState {
    backend: RefCell<Box<dyn FileDialogBackend>>,
    pending: RefCell<HashMap<RequestId, PendingCallback>>,
    next_id: Cell<u64>,
}

/// Per-app file-dialog service. Registered in app-state by
/// [`FernAppBuilder::install_file_dialog`](https://docs.rs/fern-app);
/// reachable from any handler via
/// `ctx.app_state::<FileDialogHandle>()`. Cloneable; clones share the
/// same backend and pending-callbacks map.
#[derive(Clone)]
pub struct FileDialogHandle {
    inner: Rc<FileDialogState>,
}

impl FileDialogHandle {
    /// Build a handle wrapping the given backend.
    pub fn new<B: FileDialogBackend + 'static>(backend: B) -> Self {
        Self {
            inner: Rc::new(FileDialogState {
                backend: RefCell::new(Box::new(backend)),
                pending: RefCell::new(HashMap::new()),
                next_id: Cell::new(1),
            }),
        }
    }

    /// Submit a request. Validates the request, registers the
    /// callback, and asks the backend to dispatch.
    ///
    /// `on_result` runs on the main thread when the OS dialog
    /// completes, or is dropped if `window_id`'s window closes
    /// first ([`Self::purge_window`]).
    ///
    /// Returns the [`RequestId`] for diagnostics; the caller does
    /// not need to track it for the result to be delivered.
    pub fn submit(
        &self,
        window_id: FernWindowId,
        request: FileDialogRequest,
        poster: Arc<dyn fern_core::AppEventPoster>,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String> {
        request.validate()?;
        let id = self.alloc_id();
        self.inner.pending.borrow_mut().insert(
            id,
            PendingCallback {
                window_id,
                callback: Box::new(on_result),
            },
        );
        self.inner
            .backend
            .borrow_mut()
            .dispatch(id, window_id, request, poster);
        Ok(id)
    }

    /// Deliver a backend-completed payload to its pending callback.
    /// Called by `fern-app` from the `AppEvent::External` arm. If
    /// the callback was already purged (window closed), the payload
    /// is silently dropped.
    pub fn deliver(&self, payload: FileDialogEventPayload, ctx: &mut EventContext) {
        let entry = self.inner.pending.borrow_mut().remove(&payload.request_id);
        let Some(pending) = entry else {
            return;
        };
        if pending.window_id != payload.window_id_owner {
            // Window changed since submit (re-use of an id slot is
            // impossible because ids are monotonic Cell<u64> bumps,
            // but this is a defensive guard).
            return;
        }
        (pending.callback)(payload.result, ctx);
    }

    /// Drop every pending callback whose owning window matches
    /// `window_id`. Called by `fern-app`'s window-close path so
    /// callbacks capturing widget state cannot fire into a
    /// torn-down tree.
    pub fn purge_window(&self, window_id: FernWindowId) {
        self.inner
            .pending
            .borrow_mut()
            .retain(|_, p| p.window_id != window_id);
    }

    /// Number of pending callbacks. Test helper.
    pub fn pending_count(&self) -> usize {
        self.inner.pending.borrow().len()
    }

    fn alloc_id(&self) -> RequestId {
        let n = self.inner.next_id.get();
        self.inner.next_id.set(n.wrapping_add(1));
        RequestId(n)
    }
}

impl std::fmt::Debug for FileDialogHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileDialogHandle")
            .field("pending", &self.inner.pending.borrow().len())
            .finish_non_exhaustive()
    }
}

// ============================================================
// EventContext extension trait
// ============================================================

/// Convenience methods on [`EventContext`] for opening native file
/// dialogs. Brings the four shapes (open file, open files, pick
/// folder, save file) into scope as `ctx.pick_file(req, |result| ...)`
/// without forcing every caller to look up the handle and poster
/// from app-state by hand.
///
/// Apps `use fern_platform::file_dialog::EventContextFileDialogExt`
/// (or `use fern_ui::prelude::*` once the umbrella re-exports it).
///
/// All four methods perform the same internal sequence:
///
///   1. (macOS only) focus the current window so the panel comes to
///      front for non-bundled binaries.
///   2. Stamp the parent handle into the request.
///   3. Look up [`FileDialogHandle`] and the
///      [`fern_core::AppEventPoster`] via app-state.
///   4. Forward to [`FileDialogHandle::submit`].
///
/// Returns `Err` only when the request fails validation
/// ([`FileDialogRequest::validate`]) or when the framework was not
/// initialised with a [`FileDialogHandle`] (i.e. the application
/// did not call `FernAppBuilder::install_file_dialog`).
///
/// **Convenience method overrides the request kind.** The method you
/// call dictates the operation: `ctx.pick_file(req, …)` always opens
/// a single-file picker even if `req` was built with
/// [`FileDialogRequest::save_file`]. Each method overwrites
/// `request.kind` so the [`FileDialogResult`] variant returned to the
/// callback always matches the method name. To control the kind
/// explicitly, call [`FileDialogHandle::submit`] directly with a
/// pre-built request.
pub trait EventContextFileDialogExt {
    /// Open a single-file dialog parented to the current window.
    /// `on_result` runs on the main thread on dialog completion or is
    /// dropped if the originating window closes first.
    ///
    /// The request's kind is forced to `PickFile` regardless of how
    /// it was constructed — see the trait-level docs.
    fn pick_file(
        &mut self,
        request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String>;

    /// Open a multi-file selection dialog. The request's kind is
    /// forced to `PickFiles`. See [`Self::pick_file`].
    fn pick_files(
        &mut self,
        request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String>;

    /// Open a folder picker. The request's kind is forced to
    /// `PickFolder`. See [`Self::pick_file`].
    fn pick_folder(
        &mut self,
        request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String>;

    /// Open a save dialog. The request's kind is forced to
    /// `SaveFile`. See [`Self::pick_file`].
    fn save_file(
        &mut self,
        request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String>;
}

impl EventContextFileDialogExt for EventContext<'_> {
    fn pick_file(
        &mut self,
        mut request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String> {
        request.kind = DialogKind::PickFile;
        submit_via_ctx(self, request, on_result)
    }

    fn pick_files(
        &mut self,
        mut request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String> {
        request.kind = DialogKind::PickFiles;
        submit_via_ctx(self, request, on_result)
    }

    fn pick_folder(
        &mut self,
        mut request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String> {
        request.kind = DialogKind::PickFolder;
        submit_via_ctx(self, request, on_result)
    }

    fn save_file(
        &mut self,
        mut request: FileDialogRequest,
        on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
    ) -> Result<RequestId, String> {
        request.kind = DialogKind::SaveFile;
        submit_via_ctx(self, request, on_result)
    }
}

fn submit_via_ctx(
    ctx: &mut EventContext,
    mut request: FileDialogRequest,
    on_result: impl FnOnce(FileDialogResult, &mut EventContext) + 'static,
) -> Result<RequestId, String> {
    let window_id = ctx
        .window()
        .map(|w| w.id())
        .ok_or_else(|| "EventContext has no window — file dialog needs a parent window".to_string())?;

    // macOS focus-to-front: a non-bundled binary may launch the
    // panel behind another app. Focusing the parent first reliably
    // brings it forward via NSApp.activateIgnoringOtherApps.
    #[cfg(target_os = "macos")]
    ctx.focus_window(window_id);

    if request.parent.is_none() {
        if let Some(parent) = ctx.parent_window_handle() {
            request = request.with_parent(parent);
        }
    }

    let handle = ctx
        .app_state::<FileDialogHandle>()
        .ok_or_else(|| {
            "FileDialogHandle not installed in app-state — call \
             FernAppBuilder::install_file_dialog (or app_state(...)) at startup"
                .to_string()
        })?
        .clone();
    let poster = ctx
        .poster()
        .ok_or_else(|| {
            "AppEventPoster not installed — file dialog needs a way to post \
             results back to the UI loop"
                .to_string()
        })?
        .clone();

    handle.submit(window_id, request, poster, on_result)
}

// ============================================================
// MemoryFileDialog (test backend)
// ============================================================

/// In-memory deterministic backend for headless tests. Holds a
/// scripted queue of pre-canned [`FileDialogResult`]s that pop in
/// submission order. Each `dispatch` call pops one result and
/// immediately posts it through the supplied poster — handy for
/// tests that drive the event loop one tick at a time.
pub struct MemoryFileDialog {
    scripted: VecDeque<FileDialogResult>,
}

impl MemoryFileDialog {
    /// Build a new empty mock backend. Use [`Self::enqueue`] to
    /// script per-call results.
    pub fn new() -> Self {
        Self {
            scripted: VecDeque::new(),
        }
    }

    /// Push a result onto the FIFO queue. Each `dispatch` call pops
    /// the front of the queue.
    pub fn enqueue(&mut self, r: FileDialogResult) {
        self.scripted.push_back(r);
    }
}

impl Default for MemoryFileDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDialogBackend for MemoryFileDialog {
    fn dispatch(
        &mut self,
        request_id: RequestId,
        window_id: FernWindowId,
        _request: FileDialogRequest,
        poster: Arc<dyn fern_core::AppEventPoster>,
    ) {
        let result = self.scripted.pop_front().unwrap_or_else(|| {
            FileDialogResult::Error("MemoryFileDialog: no scripted result enqueued".into())
        });
        let payload = FileDialogEventPayload {
            request_id,
            window_id_owner: window_id,
            result,
        };
        poster.post_external(Box::new(payload) as Box<dyn Any + Send>);
    }
}

// ============================================================
// RfdAsyncBackend (real backend, gated behind rfd-backend feature)
// ============================================================

#[cfg(feature = "rfd-backend")]
mod rfd_backend {
    use super::*;

    /// Native file-dialog backend backed by the `rfd` crate.
    ///
    /// Each [`Self::dispatch`] call builds an `rfd::AsyncFileDialog`,
    /// attaches the parent window handle, then spawns the future on
    /// `async-std`'s global thread pool. The future's resolution
    /// posts a [`FileDialogEventPayload`] back through the supplied
    /// [`fern_core::AppEventPoster`].
    ///
    /// On macOS, rfd dispatches the actual `NSOpenPanel` /
    /// `NSSavePanel` to the AppKit main run loop internally — the
    /// future drives the wakeup machinery, but the panel UI runs on
    /// the main thread that winit is already pumping.
    pub struct RfdAsyncBackend;

    impl RfdAsyncBackend {
        pub fn new() -> Self {
            Self
        }
    }

    impl Default for RfdAsyncBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl FileDialogBackend for RfdAsyncBackend {
        fn dispatch(
            &mut self,
            request_id: RequestId,
            window_id: FernWindowId,
            request: FileDialogRequest,
            poster: Arc<dyn fern_core::AppEventPoster>,
        ) {
            let mut dialog = rfd::AsyncFileDialog::new();
            if let Some(t) = request.title.as_ref() {
                dialog = dialog.set_title(t);
            }
            if let Some(d) = request.starting_dir.as_ref() {
                dialog = dialog.set_directory(d);
            }
            if let Some(n) = request.default_file_name.as_ref() {
                dialog = dialog.set_file_name(n);
            }
            for f in &request.filters {
                let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
                dialog = dialog.add_filter(&f.label, &exts);
            }
            if let Some(parent) = request.parent.as_ref() {
                // `ParentHandle` itself implements `HasWindowHandle +
                // HasDisplayHandle`, so rfd can extract the raw bytes
                // eagerly into its own storage.
                dialog = dialog.set_parent(parent);
            }

            let kind = request.kind();
            spawn_dialog_task(async move {
                let result = match kind {
                    DialogKind::PickFile => FileDialogResult::File(
                        dialog.pick_file().await.map(|h| h.path().to_path_buf()),
                    ),
                    DialogKind::PickFiles => FileDialogResult::Files(
                        dialog
                            .pick_files()
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|h| h.path().to_path_buf())
                            .collect(),
                    ),
                    DialogKind::PickFolder => FileDialogResult::Folder(
                        dialog.pick_folder().await.map(|h| h.path().to_path_buf()),
                    ),
                    DialogKind::SaveFile => FileDialogResult::Saved(
                        dialog.save_file().await.map(|h| h.path().to_path_buf()),
                    ),
                };
                let payload = FileDialogEventPayload {
                    request_id,
                    window_id_owner: window_id,
                    result,
                };
                poster.post_external(Box::new(payload) as Box<dyn Any + Send>);
            });
        }
    }

    fn spawn_dialog_task<F>(f: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        // Wrapped in a private function so swapping executors (tokio,
        // smol, ...) is a one-line change without touching the public
        // backend.
        async_std::task::spawn(f);
    }
}

#[cfg(feature = "rfd-backend")]
pub use rfd_backend::RfdAsyncBackend;

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::AppEventPoster;
    use std::any::Any;
    use std::sync::Mutex;

    /// Test poster that captures every posted External payload into
    /// a shared queue so tests can pull them out and feed them back
    /// to `deliver`.
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
        fn post_subscription_event(
            &self,
            _sub_id: fern_core::SubscriptionId,
            _event: Box<dyn Any + Send>,
        ) {
        }

        fn post_external(&self, payload: Box<dyn Any + Send>) {
            self.captured.lock().unwrap().push(payload);
        }
    }

    fn fern_id(n: u64) -> FernWindowId {
        FernWindowId::new(n)
    }

    #[test]
    fn validate_rejects_empty_extension_list() {
        let req = FileDialogRequest::pick_file().add_filter("Images", &[]);
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_leading_dot() {
        let req = FileDialogRequest::pick_file().add_filter("Images", &[".png"]);
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_rejects_whitespace_extension() {
        let req = FileDialogRequest::pick_file().add_filter("Images", &["png ", "jpg"]);
        assert!(req.validate().is_err());
    }

    #[test]
    fn validate_accepts_clean_filters() {
        let req = FileDialogRequest::pick_file()
            .title("Open")
            .add_filter("Images", &["png", "jpg", "JPG"]);
        assert!(req.validate().is_ok());
    }

    #[test]
    fn memory_backend_pops_scripted_in_order() {
        let mut mock = MemoryFileDialog::new();
        mock.enqueue(FileDialogResult::File(Some(PathBuf::from("/tmp/a.txt"))));
        mock.enqueue(FileDialogResult::File(Some(PathBuf::from("/tmp/b.txt"))));
        let handle = FileDialogHandle::new(mock);
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        let _ = handle
            .submit(fern_id(1), FileDialogRequest::pick_file(), poster.clone(), |_, _| {})
            .unwrap();
        let _ = handle
            .submit(fern_id(1), FileDialogRequest::pick_file(), poster.clone(), |_, _| {})
            .unwrap();

        // Two callbacks pending; two payloads posted.
        assert_eq!(handle.pending_count(), 2);
        let posted = cap.drain();
        assert_eq!(posted.len(), 2);
        for p in posted {
            let typed = p.downcast::<FileDialogEventPayload>().unwrap();
            match typed.result {
                FileDialogResult::File(Some(_)) => {}
                _ => panic!("expected File(Some)"),
            }
        }
    }

    #[test]
    fn purge_drops_callbacks_for_matching_window() {
        let mut mock = MemoryFileDialog::new();
        mock.enqueue(FileDialogResult::File(None));
        mock.enqueue(FileDialogResult::File(None));
        let handle = FileDialogHandle::new(mock);
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        let _ = handle
            .submit(fern_id(7), FileDialogRequest::pick_file(), poster.clone(), |_, _| {})
            .unwrap();
        let _ = handle
            .submit(fern_id(8), FileDialogRequest::pick_file(), poster.clone(), |_, _| {})
            .unwrap();
        assert_eq!(handle.pending_count(), 2);

        handle.purge_window(fern_id(7));
        assert_eq!(handle.pending_count(), 1);
        handle.purge_window(fern_id(8));
        assert_eq!(handle.pending_count(), 0);
    }

    #[test]
    fn submit_validates_before_dispatch() {
        let mock = MemoryFileDialog::new();
        let handle = FileDialogHandle::new(mock);
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();
        // Bad filter — should never reach the backend.
        let bad = FileDialogRequest::pick_file().add_filter("Images", &[".png"]);
        assert!(handle.submit(fern_id(1), bad, poster, |_, _| {}).is_err());
        assert_eq!(handle.pending_count(), 0);
        // Backend was not asked to dispatch — nothing was posted.
        assert_eq!(cap.drain().len(), 0);
    }

    #[test]
    fn payload_round_trips_through_capturing_poster() {
        let mut mock = MemoryFileDialog::new();
        mock.enqueue(FileDialogResult::Folder(Some(PathBuf::from("/home/u"))));
        let handle = FileDialogHandle::new(mock);
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        let req_id = handle
            .submit(
                fern_id(42),
                FileDialogRequest::pick_folder(),
                poster,
                |_, _| {},
            )
            .unwrap();

        let mut posted = cap.drain();
        assert_eq!(posted.len(), 1);
        let payload = posted
            .pop()
            .unwrap()
            .downcast::<FileDialogEventPayload>()
            .expect("payload type matches");
        assert_eq!(payload.request_id, req_id);
        assert_eq!(payload.window_id_owner, fern_id(42));
        match &payload.result {
            FileDialogResult::Folder(Some(p)) => assert_eq!(p, &PathBuf::from("/home/u")),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn deliver_after_purge_is_silent() {
        // Build a tiny tree so we can synthesize an EventContext to
        // hand into deliver. The callback should NOT fire after the
        // window's pending entries were purged.
        use fern_core::WidgetTree;
        use std::cell::Cell as StdCell;

        let mut mock = MemoryFileDialog::new();
        mock.enqueue(FileDialogResult::File(Some(PathBuf::from("/tmp/x"))));
        let handle = FileDialogHandle::new(mock);
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        let fired = Rc::new(StdCell::new(false));
        let fired_clone = fired.clone();

        let req_id = handle
            .submit(
                fern_id(5),
                FileDialogRequest::pick_file(),
                poster,
                move |_, _| fired_clone.set(true),
            )
            .unwrap();

        // Window closes before delivery.
        handle.purge_window(fern_id(5));

        // Pull the posted payload (still queued in `cap`) and
        // attempt delivery.
        let mut posted = cap.drain();
        let payload = *posted
            .pop()
            .unwrap()
            .downcast::<FileDialogEventPayload>()
            .unwrap();
        assert_eq!(payload.request_id, req_id);

        let mut tree = WidgetTree::new();
        let mut noop = fern_core::NoopWindowOps;
        tree.run_with_event_context(&mut noop, |ctx| {
            handle.deliver(payload, ctx);
        });

        assert!(!fired.get(), "callback must not fire after purge");
    }

    #[test]
    fn deliver_invokes_callback_with_result() {
        use fern_core::WidgetTree;
        use std::cell::Cell as StdCell;

        let mut mock = MemoryFileDialog::new();
        mock.enqueue(FileDialogResult::File(Some(PathBuf::from("/tmp/y.txt"))));
        let handle = FileDialogHandle::new(mock);
        let cap = CapturingPoster::new();
        let poster: Arc<dyn AppEventPoster> = cap.clone();

        let captured: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let captured_clone = captured.clone();
        // Discard the unused Cell import warning by referencing it.
        let _ = StdCell::new(0);

        let _ = handle
            .submit(
                fern_id(11),
                FileDialogRequest::pick_file(),
                poster,
                move |result, _| {
                    if let FileDialogResult::File(Some(p)) = result {
                        *captured_clone.borrow_mut() = Some(p);
                    }
                },
            )
            .unwrap();

        let payload = *cap
            .drain()
            .pop()
            .unwrap()
            .downcast::<FileDialogEventPayload>()
            .unwrap();
        let mut tree = WidgetTree::new();
        let mut noop = fern_core::NoopWindowOps;
        tree.run_with_event_context(&mut noop, |ctx| handle.deliver(payload, ctx));

        assert_eq!(*captured.borrow(), Some(PathBuf::from("/tmp/y.txt")));
        // After delivery, no callback remains.
        assert_eq!(handle.pending_count(), 0);
    }
}
