// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Arc;

use bastyde_canvas::Point;
use bastyde_core::AppEventPoster;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;
use bastyde_core::{DragImageData, DropOutcome, ExternalDropData, OutboundDragData};
use raw_window_handle::RawWindowHandle;

use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOTIMPL,
    HGLOBAL, HWND, OLE_E_ADVISENOTSUPPORTED, POINT, POINTL, S_OK,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE,
    DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, OleInitialize,
    RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MK_RBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP, SHCreateStdEnumFmtEtc};
use windows::core::{Ref, implement, w};

use super::{
    ExternalDndBackend, ExternalDndEventPayload, ExternalDndGuard, ExternalDragEvent, NoopDndGuard,
    OutboundOsDragRequest,
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
        // `ScreenToClient` mutates `p` into client (physical) pixels; its BOOL
        // return (success flag) is not actionable here.
        let _ = unsafe { ScreenToClient(self.hwnd, &mut p) };
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
        unsafe {
            *pdweffect = if accepted {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            }
        };
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
/// object alive for the window's lifetime. Also drives the **outbound**
/// (app → OS) drag: [`ExternalDndGuard::begin_drag`] stashes the payload and
/// [`ExternalDndGuard::run_pending_outbound_drag`] runs the blocking
/// `DoDragDrop` deferred, off the in-app dispatch that armed it.
pub struct WindowsDndGuard {
    hwnd: HWND,
    window_id: BastydeWindowId,
    poster: Arc<dyn AppEventPoster>,
    /// Outbound payload parked by `begin_drag`, consumed once by
    /// `run_pending_outbound_drag`.
    pending: RefCell<Option<OutboundDragData>>,
    // Held only to keep the inbound drop-target COM object alive while registered.
    _target: IDropTarget,
}

impl WindowsDndGuard {
    fn post(&self, event: ExternalDragEvent) {
        self.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: self.window_id,
            event,
        }));
    }
}

impl ExternalDndGuard for WindowsDndGuard {
    fn begin_drag(&self, data: &OutboundDragData, _image: Option<&DragImageData>) -> bool {
        // `DoDragDrop` runs its own modal message loop, so it must NOT run
        // re-entrantly inside the in-app dispatch that armed the escalation.
        // Stash the payload and post a request; `bastyde-app` calls
        // `run_pending_outbound_drag` on the next loop turn (no window borrowed).
        // The drag image is ignored for v1 (as on macOS).
        *self.pending.borrow_mut() = Some(data.clone());
        self.poster.post_external(Box::new(OutboundOsDragRequest {
            window_id: self.window_id,
        }));
        true
    }

    fn run_pending_outbound_drag(&self) {
        let data = match self.pending.borrow_mut().take() {
            Some(d) if !d.is_empty() => d,
            _ => return,
        };

        // Source-owned COM objects: a data object carrying the exportable
        // formats, and a drop source driving the button/escape policy.
        let data_object: IDataObject = DataObject::from_data(&data).into();
        let drop_source: IDropSource = DropSource.into();

        // We advertise Copy only (mirrors macOS): allowing Move would let the
        // destination physically relocate a dragged file — too dangerous as a
        // default. The OS runs its modal drag loop here on the UI thread.
        let mut effect = DROPEFFECT_NONE;
        // SAFETY: STA / UI thread (OleInitialize ran in `attach`); both COM
        // objects outlive the call; `effect` is a valid out pointer.
        let hr = unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) };

        let outcome = if hr == DRAGDROP_S_DROP {
            if effect == DROPEFFECT_MOVE {
                DropOutcome::OsMove
            } else if effect == DROPEFFECT_COPY {
                DropOutcome::OsCopy
            } else {
                // effect == DROPEFFECT_NONE (or anything we didn't advertise).
                DropOutcome::Cancelled
            }
        } else {
            // DRAGDROP_S_CANCEL or an error HRESULT.
            DropOutcome::Cancelled
        };

        self.post(ExternalDragEvent::DragEnded { outcome });
    }
}

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
            // The inbound target and the outbound guard each keep a poster.
            poster: poster.clone(),
        }
        .into();

        if unsafe { RegisterDragDrop(hwnd, &target) }.is_err() {
            return Box::new(NoopDndGuard);
        }

        Box::new(WindowsDndGuard {
            hwnd,
            window_id,
            poster,
            pending: RefCell::new(None),
            _target: target,
        })
    }
}

// ============================================================
// Outbound (app → OS) drag source
// ============================================================

/// One clipboard format we advertise on the outbound drag: the format id and
/// the precomputed HGLOBAL payload bytes. `GetData` copies these into a *fresh*
/// HGLOBAL on every call (the OS takes ownership of each returned STGMEDIUM and
/// frees it via `ReleaseStgMedium`, so the same handle must never be reused).
struct FormatBlob {
    cf: u16,
    bytes: Vec<u8>,
}

/// Source-side `IDataObject`: hands the OS the exportable representations of an
/// [`OutboundDragData`] (files → `CF_HDROP`, text → `CF_UNICODETEXT`, the first
/// URL → `CFSTR_INETURLW` + a `CF_UNICODETEXT` fallback). Source-only: the
/// `SetData` / advise family are not implemented.
#[implement(IDataObject)]
struct DataObject {
    blobs: Vec<FormatBlob>,
    /// One `FORMATETC` per blob, in the same order — handed to
    /// `SHCreateStdEnumFmtEtc` for `EnumFormatEtc`.
    formats: Vec<FORMATETC>,
}

impl DataObject {
    /// Precompute the format list once. `GetData` then only copies bytes.
    fn from_data(data: &OutboundDragData) -> Self {
        let mut blobs: Vec<FormatBlob> = Vec::new();

        if !data.files.is_empty() {
            blobs.push(FormatBlob {
                cf: CF_HDROP.0,
                bytes: build_hdrop(&data.files),
            });
        }

        let has_text = data.text.as_deref().is_some_and(|t| !t.is_empty());
        if let Some(text) = &data.text
            && !text.is_empty()
        {
            blobs.push(FormatBlob {
                cf: CF_UNICODETEXT.0,
                bytes: build_unicode_text(text),
            });
        }

        if let Some(first_url) = data.uris.first() {
            // CFSTR_INETURLW = "UniformResourceLocatorW" — the exact string the
            // inbound reader registers, so a round-trip stays symmetric.
            let inet_url =
                unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) } as u16;
            if inet_url != 0 {
                blobs.push(FormatBlob {
                    cf: inet_url,
                    bytes: build_unicode_text(first_url),
                });
            }
            // Give plain-text-only consumers something too, unless real text
            // already occupies CF_UNICODETEXT.
            if !has_text {
                blobs.push(FormatBlob {
                    cf: CF_UNICODETEXT.0,
                    bytes: build_unicode_text(first_url),
                });
            }
        }

        let formats = blobs.iter().map(|b| formatetc(b.cf)).collect();
        Self { blobs, formats }
    }
}

impl IDataObject_Impl for DataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows_core::Result<STGMEDIUM> {
        // SAFETY: the OLE runtime passes a valid FORMATETC pointer.
        let fmt = unsafe { &*pformatetcin };
        let wants_hglobal = fmt.tymed & TYMED_HGLOBAL.0 as u32 != 0;
        // SAFETY (inner): `alloc_hglobal` builds a fresh, OS-owned HGLOBAL.
        if wants_hglobal
            && let Some(blob) = self.blobs.iter().find(|b| b.cf == fmt.cfFormat)
            && let Some(hglobal) = unsafe { alloc_hglobal(&blob.bytes) }
        {
            return Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: hglobal },
                pUnkForRelease: ManuallyDrop::new(None),
            });
        }
        Err(windows_core::Error::from_hresult(DV_E_FORMATETC))
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> windows_core::Result<()> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> windows_core::HRESULT {
        // SAFETY: the OLE runtime passes a valid FORMATETC pointer.
        let fmt = unsafe { &*pformatetc };
        let wants_hglobal = fmt.tymed & TYMED_HGLOBAL.0 as u32 != 0;
        if wants_hglobal && self.blobs.iter().any(|b| b.cf == fmt.cfFormat) {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> windows_core::HRESULT {
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: windows_core::BOOL,
    ) -> windows_core::Result<()> {
        // Source-only object; the OS never pushes data back into us.
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows_core::Result<IEnumFORMATETC> {
        if dwdirection == DATADIR_GET.0 as u32 {
            // SAFETY: `formats` is a valid, non-dangling FORMATETC slice; the
            // shell copies it into the returned enumerator.
            unsafe { SHCreateStdEnumFmtEtc(&self.formats) }
        } else {
            // We never accept SetData, so there is nothing to enumerate for SET.
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<IAdviseSink>,
    ) -> windows_core::Result<u32> {
        Err(windows_core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows_core::Result<()> {
        Err(windows_core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }

    fn EnumDAdvise(&self) -> windows_core::Result<IEnumSTATDATA> {
        Err(windows_core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }
}

/// Source-side `IDropSource`: drives the drag with the button/escape policy.
#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(
        &self,
        fescapepressed: windows_core::BOOL,
        grfkeystate: MODIFIERKEYS_FLAGS,
    ) -> windows_core::HRESULT {
        let keys = grfkeystate.0;
        if fescapepressed.as_bool() || (keys & MK_RBUTTON.0) != 0 {
            // Escape or right-button chord aborts the drag.
            DRAGDROP_S_CANCEL
        } else if (keys & MK_LBUTTON.0) == 0 {
            // Primary button released → the user dropped.
            DRAGDROP_S_DROP
        } else {
            // Button still held; keep dragging.
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> windows_core::HRESULT {
        // Let the OS draw the standard copy/move/no-drop cursors.
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

// ============================================================
// Outbound format builders
// ============================================================

/// A `FORMATETC` requesting `cf` as a single `TYMED_HGLOBAL` content aspect.
fn formatetc(cf: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Copy `bytes` into a fresh `GMEM_MOVEABLE` HGLOBAL. The handle is handed to
/// the OS inside a `TYMED_HGLOBAL` STGMEDIUM; the OS frees it via
/// `ReleaseStgMedium`, so a new one is allocated on every `GetData` call.
unsafe fn alloc_hglobal(bytes: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: FFI. On success we own the HGLOBAL until the STGMEDIUM consumer
    // (the OS) releases it.
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.ok()?;
    let ptr = unsafe { GlobalLock(hglobal) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `GlobalLock` returns a pointer to at least `bytes.len()` writable
    // bytes; the ranges do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(hglobal);
    }
    Some(hglobal)
}

/// Build a `CF_HDROP` blob: a `DROPFILES` header (`fWide = TRUE`, `pFiles` =
/// header size) followed by each path as UTF-16, the whole list
/// double-NUL-terminated.
fn build_hdrop(files: &[PathBuf]) -> Vec<u8> {
    let header = DROPFILES {
        pFiles: std::mem::size_of::<DROPFILES>() as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: windows_core::BOOL::from(false),
        fWide: windows_core::BOOL::from(true),
    };
    let mut buf = Vec::new();
    // SAFETY: DROPFILES is plain-old-data; read its representation as bytes.
    let header_bytes = unsafe {
        std::slice::from_raw_parts(
            (&header as *const DROPFILES) as *const u8,
            std::mem::size_of::<DROPFILES>(),
        )
    };
    buf.extend_from_slice(header_bytes);
    for path in files {
        for unit in path.as_os_str().encode_wide() {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        // Terminate each path.
        buf.extend_from_slice(&0u16.to_le_bytes());
    }
    // Final NUL terminates the whole double-NUL list.
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf
}

/// Build a `CF_UNICODETEXT` blob: `s` as NUL-terminated little-endian UTF-16.
fn build_unicode_text(s: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    for unit in s.encode_utf16() {
        buf.extend_from_slice(&unit.to_le_bytes());
    }
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf
}
