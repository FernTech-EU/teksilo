//! Windows custom title-bar host.
//!
//! Replaces the native frame with the widget-tree-painted title bar
//! by extending the DWM-drawn frame into the client area, then
//! subclassing the HWND to intercept the non-client messages that
//! would otherwise put the OS frame back in charge of hit-testing,
//! painting, and click handling.
//!
//! ## Recipe
//!
//! 1. `DwmExtendFrameIntoClientArea(hwnd, MARGINS{top: 1, ..0})` —
//!    the 1-pixel top inset is what makes Win11 keep its rounded
//!    corners; setting `0` gives square corners. This is the
//!    Microsoft custom-frame article's magic value.
//! 2. `SetWindowSubclass` installs [`fern_titlebar_proc`] on the
//!    HWND. winit's own wndproc was registered through raw
//!    `SetWindowLongPtrW` at class-registration time and runs first;
//!    the comctl32 subclass chain fires after and falls through to
//!    `DefSubclassProc` for messages we don't intercept.
//! 3. The proc handles:
//!    - `WM_NCCALCSIZE` — zero non-client insets so the client area
//!      covers the full window. When maximized, restore the system
//!      `SM_CXFRAME + SM_CXPADDEDBORDER` insets and clamp to the
//!      monitor work area so the maximized window doesn't overflow
//!      the taskbar.
//!    - `WM_NCHITTEST` — return `HTLEFT` / `HTTOP` / corner codes
//!      for the outer N pixels (OS handles the resize loop natively),
//!      `HTCAPTION` for the widget's drag region, and (M5) the
//!      `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE` codes for the
//!      control-button rects. Returning `HTMAXBUTTON` is what makes
//!      Win11 show the snap-layout flyout on hover.
//!    - `WM_DPICHANGED` — re-extend the DWM frame so rounded corners
//!      survive a DPI change, then fall through so winit can resize.
//!    - `WM_NCLBUTTONUP` — for the three button hit codes, post a
//!      [`TitleBarSyntheticEvent`] back through the event loop. The
//!      OS owns the pixels; widget land never saw the click.
//!    - `WM_NCMOUSEMOVE` / `WM_NCMOUSELEAVE` — same pattern for hover
//!      ([`TitleBarHoverEvent`]).
//!    - `WM_NCPAINT` / `WM_NCACTIVATE` — return early so DWM doesn't
//!      paint its legacy caption-button artwork over our pixels.
//!
//! ## Threading
//!
//! The HWND is owned by winit's main thread; Win32 routes every
//! `SendMessage` (including cross-thread) onto that thread before
//! invoking the proc. Both the proc and `WindowsHost::update_hit_regions`
//! therefore run on the UI thread. Re-entry via `SendMessage` from
//! within the proc is possible, so [`SubclassData::hit_regions`]
//! sits behind a `Mutex` with `try_lock` — on contention we fall
//! through to `HTCLIENT` rather than blocking the message pump.
//!
//! ## AccessKit coexistence
//!
//! AccessKit's Windows backend installs its own `SetWindowSubclass`
//! for `WM_GETOBJECT`. The two subclasses use distinct ids and chain
//! through `DefSubclassProc` — verified compatible by message
//! disjointness (we touch `WM_NC*` / `WM_DPICHANGED`, AccessKit
//! touches `WM_GETOBJECT`).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fern_canvas::{Point, Rect, Size};
use fern_core::signal::Signal;
use fern_core::widget_id::WidgetId;
use fern_core::{
    ControlTarget, HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge,
    TitleBarHostCallbacks, TitleBarHoverEvent, TitleBarSyntheticEvent,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    ScreenToClient,
};
use windows::Win32::UI::Controls::{HOVER_DEFAULT, MARGINS};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTCLOSE, HTLEFT,
    HTMAXBUTTON, HTMINBUTTON, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsZoomed, NCCALCSIZE_PARAMS,
    SC_KEYMENU, SM_CXFRAME, SM_CXPADDEDBORDER, SM_CYFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowPos, WM_DPICHANGED, WM_NCACTIVATE,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    WM_NCPAINT, WM_SYSCOMMAND,
};

use super::edge_to_direction;

/// Subclass id counter — must be unique vs other subclasses on the
/// same HWND. Starting at a fern-flavored magic number keeps
/// debugger inspection of `dwRefData` readable.
static NEXT_SUBCLASS_ID: AtomicUsize = AtomicUsize::new(0xFE_111_000);

/// Shared state accessed both by `WindowsHost` (UI thread) and by
/// the subclass proc (also UI thread, but possibly re-entered via
/// `SendMessage`). The `Mutex` is for re-entry safety, not cross-
/// thread sharing — the proc uses `try_lock` and falls through to
/// `HTCLIENT` if the lock is held.
struct SubclassData {
    /// Latest hit-region snapshot in **physical** pixels (already
    /// scaled in `WindowsHost::update_hit_regions`). The proc reads
    /// these directly against `WM_NCHITTEST` coordinates.
    hit_regions: Mutex<HitRegions>,
    /// Posted to the event loop when buttons are tapped or hovered.
    callbacks: TitleBarHostCallbacks,
    /// Last hovered button so we emit enter/leave pairs. UI-thread
    /// only — `Cell` is sufficient.
    last_hover: Cell<Option<ControlTarget>>,
    /// Per-button hover signals registered by `WindowControls` so the
    /// host can write them on synthetic-hover events. Look-up is by
    /// `ControlTarget`. Empty until M5/C.3 wires the registration.
    hover_signals: Mutex<HashMap<ControlTarget, Signal<bool>>>,
}

// SAFETY: `SubclassData` carries a `TitleBarHostCallbacks` that
// holds `Rc<dyn Fn(...)>` — `!Send`. We never touch the Rc from a
// thread other than the UI thread that owns the HWND: Win32 routes
// every `SendMessage` (including cross-thread) through the HWND-
// owning thread before invoking the proc, and `update_hit_regions`
// is invoked from the widget tree's paint pass which also runs on
// the UI thread. The `Send + Sync` impls are an invariant we
// maintain manually so the type can sit behind a raw pointer.
unsafe impl Send for SubclassData {}
unsafe impl Sync for SubclassData {}

pub struct WindowsHost {
    window: Arc<Window>,
    hwnd: HWND,
    subclass_id: usize,
    /// Shared with the subclass proc through a raw pointer in
    /// `dwRefData`. The proc only borrows; the `Rc` here keeps the
    /// allocation alive for the proc's lifetime, which is bounded by
    /// `RemoveWindowSubclass` in `Drop`.
    data: Rc<SubclassData>,
}

impl WindowsHost {
    pub fn new(
        window: Arc<Window>,
        callbacks: TitleBarHostCallbacks,
    ) -> Result<Self, PlatformError> {
        let hwnd = extract_hwnd(&window)?;

        // Step 1: extend the DWM-drawn frame into the client area.
        // The 1-pixel top inset is the Win11 rounded-corner trick;
        // setting `0` (or omitting the call) gives square corners.
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 1,
            cyBottomHeight: 0,
        };
        unsafe {
            DwmExtendFrameIntoClientArea(hwnd, &margins)
                .map_err(|e| PlatformError::Os(format!("DwmExtendFrameIntoClientArea: {e}")))?;
        }

        // Step 2: install the subclass FIRST, before triggering the
        // frame recompute. `SetWindowPos(SWP_FRAMECHANGED)` synchronously
        // sends `WM_NCCALCSIZE` — if the subclass isn't there yet,
        // winit's default proc handles it and the OS leaves the native
        // frame in place until the next resize. Installing the subclass
        // first means our `WM_NCCALCSIZE` handler runs from the very
        // first frame.
        let subclass_id = NEXT_SUBCLASS_ID.fetch_add(1, Ordering::Relaxed);
        let data = Rc::new(SubclassData {
            hit_regions: Mutex::new(HitRegions::default()),
            callbacks,
            last_hover: Cell::new(None),
            hover_signals: Mutex::new(HashMap::new()),
        });
        let raw_ptr = Rc::as_ptr(&data) as usize;

        let installed =
            unsafe { SetWindowSubclass(hwnd, Some(fern_titlebar_proc), subclass_id, raw_ptr) };
        if !installed.as_bool() {
            return Err(PlatformError::Os("SetWindowSubclass returned FALSE".into()));
        }

        // Step 3: now force the frame recompute. The first
        // `WM_NCCALCSIZE` lands on our subclass and zeros the
        // non-client insets so the client area covers the full
        // window from the start.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }

        Ok(Self {
            window,
            hwnd,
            subclass_id,
            data,
        })
    }
}

impl Drop for WindowsHost {
    fn drop(&mut self) {
        // Stop the proc first so no further reads of `data` occur,
        // then drop the Rc. `RemoveWindowSubclass` is synchronous
        // on the same thread, and any subsequent `WM_NC*` for this
        // HWND reaches `DefWindowProc` directly.
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(fern_titlebar_proc), self.subclass_id);
        }
    }
}

impl PlatformTitleBarHost for WindowsHost {
    fn reserved_leading_inset(&self) -> Size {
        // Windows draws no leading-edge artwork inside the client
        // area; the title bar can use the full width.
        Size::ZERO
    }

    fn reserved_trailing_inset(&self) -> Size {
        Size::ZERO
    }

    fn renders_custom_controls(&self) -> bool {
        // The widget renders its own min/max/close cluster.
        true
    }

    fn needs_custom_resize_handles(&self) -> bool {
        // The OS handles edge resize via our `WM_NCHITTEST` returning
        // `HTLEFT` / `HTTOP` / corner codes; the widget-tree
        // `WindowFrame` overlay would receive no clicks (the OS
        // dispatches them as non-client) and is redundant here.
        false
    }

    fn begin_drag(&self) -> Result<(), PlatformError> {
        // winit's `drag_window` is the right entry point on Windows
        // too — it issues `SendMessage(WM_NCLBUTTONDOWN, HTCAPTION)`
        // internally.
        self.window
            .drag_window()
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn begin_resize(&self, edge: ResizeEdge) -> Result<(), PlatformError> {
        self.window
            .drag_resize_window(edge_to_direction(edge))
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
        // Programmatic invocation of the system menu. Right-click on
        // `HTCAPTION` already opens it via `DefSubclassProc`; this
        // path is for code that explicitly wants to show it.
        unsafe {
            let _ = SendMessageW(
                self.hwnd,
                WM_SYSCOMMAND,
                Some(WPARAM(SC_KEYMENU as usize)),
                Some(LPARAM(0)),
            );
        }
        Ok(())
    }

    fn update_hit_regions(&self, regions: &HitRegions) {
        // Convert the widget-tree's logical-pixel rects to physical
        // pixels here, where the live HWND is in scope. The wndproc
        // reads physical coordinates directly out of `WM_NCHITTEST`'s
        // lparam (after `ScreenToClient`), so storing physical here
        // saves the proc from doing per-message DPI math under the
        // re-entrancy-sensitive lock.
        let dpi = unsafe { GetDpiForWindow(self.hwnd) } as f32;
        let scale = if dpi > 0.0 { dpi / 96.0 } else { 1.0 };
        let scaled = scale_hit_regions(regions, scale);
        if let Ok(mut shared) = self.data.hit_regions.lock() {
            *shared = scaled;
        }
    }

    fn title_bar_widget_id(&self, target: ControlTarget) -> Option<WidgetId> {
        let regions = self.data.hit_regions.lock().ok()?;
        match target {
            ControlTarget::Minimize => regions.minimize_id,
            ControlTarget::Maximize => regions.maximize_id,
            ControlTarget::Close => regions.close_id,
        }
    }

    fn set_button_hover(&self, target: ControlTarget, entered: bool) {
        if let Ok(map) = self.data.hover_signals.lock() {
            if let Some(sig) = map.get(&target) {
                sig.set(entered);
            }
        }
    }

    fn register_hover_signal(&self, target: ControlTarget, signal: Signal<bool>) {
        if let Ok(mut map) = self.data.hover_signals.lock() {
            map.insert(target, signal);
        }
    }
}

/// Extract the `HWND` from a winit window via raw-window-handle 0.6.
/// Mirrors the macOS NSWindow extraction in `macos.rs`.
fn extract_hwnd(window: &Arc<Window>) -> Result<HWND, PlatformError> {
    let handle = window
        .window_handle()
        .map_err(|e| PlatformError::Os(format!("window_handle: {e}")))?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Err(PlatformError::Os("expected Win32 window handle".into()));
    };
    Ok(HWND(raw.hwnd.get() as *mut _))
}

/// Multiply every rect in `regions` by `scale`. Used by
/// `WindowsHost::update_hit_regions` to convert from the widget
/// tree's logical pixels to the wndproc's physical pixels.
fn scale_hit_regions(src: &HitRegions, scale: f32) -> HitRegions {
    let scale_rect = |r: Rect| -> Rect {
        Rect::new(r.x * scale, r.y * scale, r.width * scale, r.height * scale)
    };
    HitRegions {
        minimize: src.minimize.map(scale_rect),
        maximize: src.maximize.map(scale_rect),
        close: src.close.map(scale_rect),
        minimize_id: src.minimize_id,
        maximize_id: src.maximize_id,
        close_id: src.close_id,
        drag: src.drag.iter().copied().map(scale_rect).collect(),
        resize_borders: src.resize_borders,
    }
}

/// The subclass procedure. Runs on the UI thread (Win32 marshals
/// cross-thread `SendMessage` onto the HWND-owning thread). The
/// `dwRefData` is a `*const SubclassData` we pass through
/// `SetWindowSubclass`; we re-borrow it as `&SubclassData` for the
/// duration of the call.
///
/// `_uid` is the subclass id we registered with — Win32 echoes it
/// back so a single proc can serve multiple subclass slots if
/// needed. We don't use that capability.
unsafe extern "system" fn fern_titlebar_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid: usize,
    dw_ref_data: usize,
) -> LRESULT {
    if dw_ref_data == 0 {
        return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    }
    let data: &SubclassData = unsafe { &*(dw_ref_data as *const SubclassData) };

    match msg {
        WM_NCCALCSIZE if wparam.0 != 0 => handle_nccalcsize(hwnd, lparam),
        WM_NCHITTEST => handle_nchittest(hwnd, lparam, data),
        WM_NCLBUTTONDOWN => {
            // For our control-button hit codes, swallow the press so
            // `DefSubclassProc` doesn't enter its built-in
            // press-tracking modal loop — that loop consumes the
            // matching `WM_NCLBUTTONUP` and our handler below would
            // never see it (the user-visible symptom: the button
            // appears to need a double-click). The actual action
            // fires on the corresponding `WM_NCLBUTTONUP`.
            let is_button = matches!(
                wparam.0 as u32,
                v if v == HTMINBUTTON || v == HTMAXBUTTON || v == HTCLOSE
            );
            if is_button {
                return LRESULT(0);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCLBUTTONUP => {
            let target = match wparam.0 as u32 {
                v if v == HTMINBUTTON => Some(ControlTarget::Minimize),
                v if v == HTMAXBUTTON => Some(ControlTarget::Maximize),
                v if v == HTCLOSE => Some(ControlTarget::Close),
                _ => None,
            };
            if let Some(target) = target {
                let payload = TitleBarSyntheticEvent {
                    fern_id: data.callbacks.fern_id,
                    target,
                };
                (data.callbacks.post_external)(Box::new(payload));
                return LRESULT(0);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCMOUSEMOVE => {
            let target = match wparam.0 as u32 {
                v if v == HTMINBUTTON => Some(ControlTarget::Minimize),
                v if v == HTMAXBUTTON => Some(ControlTarget::Maximize),
                v if v == HTCLOSE => Some(ControlTarget::Close),
                _ => None,
            };
            let prev = data.last_hover.get();
            if prev != target {
                if let Some(p) = prev {
                    let payload = TitleBarHoverEvent {
                        fern_id: data.callbacks.fern_id,
                        target: p,
                        entered: false,
                    };
                    (data.callbacks.post_external)(Box::new(payload));
                }
                if let Some(t) = target {
                    let payload = TitleBarHoverEvent {
                        fern_id: data.callbacks.fern_id,
                        target: t,
                        entered: true,
                    };
                    (data.callbacks.post_external)(Box::new(payload));

                    // Track non-client mouse leave so we can fire a
                    // final exit event when the cursor leaves the
                    // window's non-client area entirely (the OS
                    // doesn't send `WM_NCMOUSEMOVE` with a
                    // not-on-button hit code in that case).
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_NONCLIENT | TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: HOVER_DEFAULT,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    }
                }
                data.last_hover.set(target);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCMOUSELEAVE => {
            if let Some(prev) = data.last_hover.replace(None) {
                let payload = TitleBarHoverEvent {
                    fern_id: data.callbacks.fern_id,
                    target: prev,
                    entered: false,
                };
                (data.callbacks.post_external)(Box::new(payload));
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_DPICHANGED => {
            // winit handles `WM_DPICHANGED` (resizes the window,
            // emits `ScaleFactorChanged`), but does NOT re-call
            // `DwmExtendFrameIntoClientArea`. Without re-extending,
            // the client area loses its 1-pixel top reserve and the
            // rounded corners go square.
            let margins = MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 1,
                cyBottomHeight: 0,
            };
            unsafe {
                let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCPAINT => {
            // DWM owns the non-client paint via the extended frame.
            // Returning 0 tells the OS we handled it so it doesn't
            // try to paint legacy caption-button artwork over our
            // pixels.
            LRESULT(0)
        }
        WM_NCACTIVATE => {
            // Returning TRUE prevents the OS from repainting the
            // (non-existent) frame on focus changes, which on Win10
            // causes a one-frame flicker if we return 0. We rely on
            // the widget-tree paint pass to update the visual focus
            // state.
            LRESULT(1)
        }
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

fn handle_nccalcsize(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    // SAFETY: When `wparam == TRUE`, `lparam` points to a valid
    // `NCCALCSIZE_PARAMS` whose first rect (`rgrc[0]`) is the
    // proposed client area. We narrow / restore it in place.
    unsafe {
        let p = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
        let dpi = GetDpiForWindow(hwnd);
        if IsZoomed(hwnd).as_bool() {
            // Maximized: the OS computed a frame-inflated rect so
            // the title bar would land outside the visible monitor
            // area. Pull the client rect back in by the OS frame
            // metrics, then clamp to the monitor work area so the
            // window doesn't cover the taskbar.
            let padded = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            let frame = GetSystemMetricsForDpi(SM_CYFRAME, dpi);
            p.rgrc[0].top += padded + frame;
            p.rgrc[0].left += padded + frame;
            p.rgrc[0].right -= padded + frame;
            p.rgrc[0].bottom -= padded + frame;

            let mon: HMONITOR = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(mon, &mut info).as_bool() {
                let work = info.rcWork;
                if p.rgrc[0].top < work.top {
                    p.rgrc[0].top = work.top;
                }
                if p.rgrc[0].left < work.left {
                    p.rgrc[0].left = work.left;
                }
                if p.rgrc[0].right > work.right {
                    p.rgrc[0].right = work.right;
                }
                if p.rgrc[0].bottom > work.bottom {
                    p.rgrc[0].bottom = work.bottom;
                }
            }
        }
        // Floating: leave `rgrc[0]` untouched. Returning 0 below
        // tells Win32 the entire window rect is the client rect — the
        // 1-pixel top reserve from `DwmExtendFrameIntoClientArea`
        // does NOT come back as a non-client inset here; it is
        // handled by DWM internally.
    }
    LRESULT(0)
}

fn handle_nchittest(hwnd: HWND, lparam: LPARAM, data: &SubclassData) -> LRESULT {
    // `lparam` packs the screen-space cursor as
    // (LOWORD = x, HIWORD = y), each as i16 — but we extract as
    // i32 for the rest of the math.
    let raw = lparam.0 as i32;
    let screen_x = (raw & 0xFFFF) as i16 as i32;
    let screen_y = ((raw >> 16) & 0xFFFF) as i16 as i32;
    let mut pt = POINT {
        x: screen_x,
        y: screen_y,
    };
    let _ = unsafe { ScreenToClient(hwnd, &mut pt) };

    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let resize = unsafe {
        GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi) + GetSystemMetricsForDpi(SM_CXFRAME, dpi)
    };

    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };

    // Resize borders win first — these are the 8 outer edges.
    // When maximized, no resize borders (the OS won't allow resize
    // anyway).
    let zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
    if !zoomed {
        let on_top = pt.y < resize;
        let on_bottom = pt.y >= rect.bottom - resize;
        let on_left = pt.x < resize;
        let on_right = pt.x >= rect.right - resize;

        let edge: Option<u32> = match (on_top, on_bottom, on_left, on_right) {
            (true, false, true, false) => Some(HTTOPLEFT),
            (true, false, false, true) => Some(HTTOPRIGHT),
            (false, true, true, false) => Some(HTBOTTOMLEFT),
            (false, true, false, true) => Some(HTBOTTOMRIGHT),
            (true, false, _, _) => Some(HTTOP),
            (false, true, _, _) => Some(HTBOTTOM),
            (_, _, true, false) => Some(HTLEFT),
            (_, _, false, true) => Some(HTRIGHT),
            _ => None,
        };
        if let Some(e) = edge {
            return LRESULT(e as isize);
        }
    }

    // Control-button rects + drag region. `try_lock` so a re-entered
    // proc never deadlocks; on contention we fall through to
    // `HTCLIENT` (the user might miss one frame of NC routing,
    // which is invisible).
    if let Ok(regions) = data.hit_regions.try_lock() {
        let pt_canvas = Point::new(pt.x as f32, pt.y as f32);

        if let Some(r) = regions.minimize {
            if r.contains(pt_canvas) {
                return LRESULT(HTMINBUTTON as isize);
            }
        }
        if let Some(r) = regions.maximize {
            if r.contains(pt_canvas) {
                // `HTMAXBUTTON` is the magic that triggers the
                // Win11 snap-layout flyout when the cursor dwells
                // for ~50 ms. No additional API call needed.
                return LRESULT(HTMAXBUTTON as isize);
            }
        }
        if let Some(r) = regions.close {
            if r.contains(pt_canvas) {
                return LRESULT(HTCLOSE as isize);
            }
        }
        for drag in &regions.drag {
            if drag.contains(pt_canvas) {
                return LRESULT(HTCAPTION as isize);
            }
        }
    }

    LRESULT(HTCLIENT as isize)
}
