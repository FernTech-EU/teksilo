//! Opaque platform window/display handle wrapper.
//!
//! `ParentHandle` carries a `(RawWindowHandle, RawDisplayHandle)` pair
//! extracted from a winit window on the main thread. Native dialog
//! libraries (e.g. `rfd::AsyncFileDialog::set_parent`) consume the pair
//! to parent their OS-level UI to the Bastyde window.
//!
//! Lives in `bastyde-core` rather than `bastyde-platform` so the
//! [`WindowOps`](crate::window::WindowOps) trait, which is in core, can
//! mention it without inverting the dependency graph
//! (`core → platform` would be a layering violation; `platform → core`
//! is the established direction).
//!
//! # Thread safety
//!
//! `RawWindowHandle` and `RawDisplayHandle` are enums that include
//! raw pointers (`*mut c_void` in the AppKit/Win32/Wayland variants).
//! Rust marks raw pointers `!Send + !Sync` by default, so the enums
//! inherit that. We need this struct to cross thread boundaries
//! (main → async-std worker driving an `rfd::AsyncFileDialog` future),
//! so we add `unsafe Send + Sync` impls below.
//!
//! Safety contract: the bytes of the handle are moved between threads,
//! but every platform-specific dereference of the inner pointer
//! happens inside backend glue that arranges the correct thread
//! affinity per OS — `dispatch::Queue::main` on macOS, the D-Bus
//! thread on Linux portal, the COM apartment on Windows. Callers
//! must NOT dereference the inner handle off the main thread by hand.

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};

/// Opaque pair of platform handles describing a parent window for a
/// native OS dialog. Construct with [`ParentHandle::from_window`] on
/// the main thread.
///
/// Implements [`HasWindowHandle`] and [`HasDisplayHandle`] so backend
/// code can hand it directly to APIs like `rfd::AsyncFileDialog::set_parent`
/// without an intermediate adapter type.
#[derive(Clone)]
pub struct ParentHandle {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

impl ParentHandle {
    /// Extract the platform handles from a winit window.
    ///
    /// MUST be called on the main thread — winit's handle accessors
    /// are documented as main-thread-only on macOS / Wayland.
    /// Returns `None` if either handle accessor fails (rare; mostly
    /// during teardown when the underlying surface is already gone).
    pub fn from_window<W>(window: &W) -> Option<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + ?Sized,
    {
        let window_handle = window.window_handle().ok()?.as_raw();
        let display_handle = window.display_handle().ok()?.as_raw();
        Some(Self {
            window: window_handle,
            display: display_handle,
        })
    }

    /// Raw window handle bytes. Use when the consuming API wants the
    /// enum directly rather than the borrowed [`WindowHandle<'_>`]
    /// returned by [`HasWindowHandle::window_handle`].
    pub fn raw_window_handle(&self) -> RawWindowHandle {
        self.window
    }

    /// Raw display handle bytes. Paired with
    /// [`Self::raw_window_handle`].
    pub fn raw_display_handle(&self) -> RawDisplayHandle {
        self.display
    }
}

impl HasWindowHandle for ParentHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: `self.window` was extracted on the main thread
        // from a live winit window. The originating window outlives
        // the in-flight dialog because [`FileDialogHandle::purge_window`]
        // drops pending callbacks before the window's tree is torn
        // down. Backends that consume the handle (rfd's `set_parent`)
        // copy the raw enum into their own storage; the borrow tied
        // to `&self` never escapes the `set_parent` call.
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}

impl HasDisplayHandle for ParentHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: see HasWindowHandle::window_handle above.
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

// SAFETY: We only move handle bytes between threads. Every
// platform-specific dereference of the inner pointer happens inside
// backend glue that arranges the correct thread affinity per OS.
// raw-window-handle's own design allows storage of the raw enums
// across threads — only the borrowed `WindowHandle<'_>` /
// `DisplayHandle<'_>` types are `!Send` because of their lifetimes.
unsafe impl Send for ParentHandle {}
unsafe impl Sync for ParentHandle {}

impl std::fmt::Debug for ParentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParentHandle").finish_non_exhaustive()
    }
}
