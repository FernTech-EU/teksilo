//! Windows external drag-and-drop backend (OLE `IDropTarget`).
//!
//! winit registers its own OLE drop target on the window and reports drops
//! without a cursor position. We displace it: `RevokeDragDrop` removes winit's,
//! then `RegisterDragDrop` installs our [`IDropTarget`] COM object, which reads
//! the full payload (files via `CF_HDROP`, text via `CF_UNICODETEXT`, URLs via
//! `CFSTR_INETURLW`) and reports the cursor position (`POINTL`, screen → client
//! → logical) on every callback.
//!
//! Each drag phase posts an [`ExternalDndEventPayload`] through the app's
//! [`AppEventPoster`]; `bastyde-app` routes it to the window's `WidgetTree`.
//! The registration is revoked when the guard drops (window close).
//!
//! **Verification status:** written against the `windows` crate 0.61 API but
//! compiled and exercised only on a Windows host — it is `cfg(target_os =
//! "windows")` and is not built on the macOS development machine. Treat a first
//! Windows build as the verification pass.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;

use bastyde_canvas::Point;
use bastyde_core::AppEventPoster;
use bastyde_core::ExternalDropData;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;
use raw_window_handle::RawWindowHandle;

use windows::Win32::Foundation::{HGLOBAL, HWND, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget,
    IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::{Ref, implement, w};

use super::{
    ExternalDndBackend, ExternalDndEventPayload, ExternalDndGuard, ExternalDragEvent, NoopDndGuard,
};

/// Our OLE drop target COM object, registered per window.
#[implement(IDropTarget)]
struct DropTarget {
    hwnd: HWND,
    window_id: BastydeWindowId,
    poster: Arc<dyn AppEventPoster>,
}

impl DropTarget {
    fn post(&self, event: ExternalDragEvent) {
        self.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: self.window_id,
            event,
        }));
    }

    /// Convert a screen-space `POINTL` to window-logical coordinates.
    fn logical_position(&self, pt: &POINTL) -> Point {
        let mut p = POINT { x: pt.x, y: pt.y };
        // `ScreenToClient` mutates `p` into client (physical) pixels.
        unsafe { ScreenToClient(self.hwnd, &mut p) };
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        Point::new(p.x as f32 / scale, p.y as f32 / scale)
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        pdataobj: Ref<'_, IDataObject>,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let data = pdataobj.ok().map(read_payload).unwrap_or_default();
        self.post(ExternalDragEvent::Entered {
            data,
            position: self.logical_position(pt),
        });
        // Advertise Copy; the widget decides real accept/reject.
        unsafe { *pdweffect = DROPEFFECT_COPY };
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.post(ExternalDragEvent::Moved {
            position: self.logical_position(pt),
        });
        unsafe { *pdweffect = DROPEFFECT_COPY };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.post(ExternalDragEvent::Left);
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: Ref<'_, IDataObject>,
        _grfkeystate: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let data = pdataobj.ok().map(read_payload).unwrap_or_default();
        let accepted = !data.is_empty();
        self.post(ExternalDragEvent::Dropped {
            data,
            position: self.logical_position(pt),
        });
        unsafe { *pdweffect = if accepted { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }
}

/// Read files / text / URLs from the drag's `IDataObject`.
fn read_payload(data: &IDataObject) -> ExternalDropData {
    let mut out = ExternalDropData::default();
    if let Some(files) = read_hdrop(data) {
        out.files = files;
    }
    if let Some(text) = read_unicode_text(data, CF_UNICODETEXT.0) {
        if !text.is_empty() {
            out.text = Some(text);
        }
    }
    // Internet shortcut URL (CFSTR_INETURLW = "UniformResourceLocatorW").
    let inet_url = unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) } as u16;
    if inet_url != 0 {
        if let Some(url) = read_unicode_text(data, inet_url) {
            if !url.is_empty() {
                out.uris.push(url);
            }
        }
    }
    out
}

/// Fetch `STGMEDIUM` (HGLOBAL) for a clipboard format, run `f` on the locked
/// global memory, then unlock + release. `None` if the format is absent.
fn with_hglobal<T>(
    data: &IDataObject,
    cf_format: u16,
    f: impl FnOnce(*const c_void) -> T,
) -> Option<T> {
    let fmt = FORMATETC {
        cfFormat: cf_format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0 as u32,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // SAFETY: FFI; `fmt` is a valid stack FORMATETC, STGMEDIUM released below.
    let mut medium: STGMEDIUM = unsafe { data.GetData(&fmt).ok()? };
    let hglobal: HGLOBAL = unsafe { medium.u.hGlobal };
    let ptr = unsafe { GlobalLock(hglobal) };
    let result = if ptr.is_null() {
        None
    } else {
        let r = f(ptr as *const c_void);
        let _ = unsafe { GlobalUnlock(hglobal) };
        Some(r)
    };
    unsafe { ReleaseStgMedium(&mut medium) };
    result
}

/// Read `CF_HDROP` into a list of paths.
fn read_hdrop(data: &IDataObject) -> Option<Vec<PathBuf>> {
    with_hglobal(data, CF_HDROP.0, |ptr| {
        let hdrop = HDROP(ptr as *mut c_void);
        // `0xFFFF_FFFF` queries the file count.
        let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) };
        let mut files = Vec::with_capacity(count as usize);
        for i in 0..count {
            // First call (None buffer) returns the length sans NUL.
            let len = unsafe { DragQueryFileW(hdrop, i, None) } as usize;
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len + 1];
            let written = unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) } as usize;
            buf.truncate(written);
            files.push(PathBuf::from(String::from_utf16_lossy(&buf)));
        }
        files
    })
}

/// Read a NUL-terminated UTF-16 clipboard format into a `String`.
fn read_unicode_text(data: &IDataObject, cf_format: u16) -> Option<String> {
    with_hglobal(data, cf_format, |ptr| {
        let mut wide = ptr as *const u16;
        let mut units = Vec::new();
        // SAFETY: the OS guarantees a NUL-terminated buffer for text formats.
        unsafe {
            while *wide != 0 {
                units.push(*wide);
                wide = wide.add(1);
            }
        }
        String::from_utf16_lossy(&units)
    })
}

/// Registration guard: revokes the OLE drop target on drop and keeps the COM
/// object alive for the window's lifetime.
pub struct WindowsDndGuard {
    hwnd: HWND,
    // Held only to keep the COM object alive while registered.
    _target: IDropTarget,
}

impl ExternalDndGuard for WindowsDndGuard {}

impl Drop for WindowsDndGuard {
    fn drop(&mut self) {
        // Best-effort: runs on the UI thread at window close.
        let _ = unsafe { RevokeDragDrop(self.hwnd) };
    }
}

/// Windows external-drag backend. See the module docs.
#[derive(Default)]
pub struct WindowsExternalDndBackend {
    // OLE must be initialised once per thread; cheap to repeat (returns S_FALSE).
    _private: RefCell<()>,
}

impl WindowsExternalDndBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ExternalDndBackend for WindowsExternalDndBackend {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: BastydeWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        let RawWindowHandle::Win32(handle) = parent.raw_window_handle() else {
            return Box::new(NoopDndGuard);
        };
        let hwnd = HWND(handle.hwnd.get() as *mut c_void);

        // winit already calls OleInitialize on its thread; repeating returns
        // S_FALSE (harmless). Required before Register/RevokeDragDrop.
        let _ = unsafe { OleInitialize(None) };

        // Drop winit's own drop-target registration first so ours can take over.
        let _ = unsafe { RevokeDragDrop(hwnd) };

        let target: IDropTarget = DropTarget {
            hwnd,
            window_id,
            poster,
        }
        .into();

        if unsafe { RegisterDragDrop(hwnd, &target) }.is_err() {
            return Box::new(NoopDndGuard);
        }

        Box::new(WindowsDndGuard {
            hwnd,
            _target: target,
        })
    }
}
