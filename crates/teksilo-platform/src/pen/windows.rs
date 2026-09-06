// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Windows Ink through a `SetWindowSubclass` tap on the `WM_POINTER*` family.
//!
//! winit 0.30 already receives every one of these messages — its Windows
//! backend answers `WM_POINTERDOWN` / `WM_POINTERUPDATE` / `WM_POINTERUP`,
//! calls `GetPointerType`, and turns the result into a `Touch` or a mouse
//! event. What it never does is call `GetPointerPenInfo`, so the pressure,
//! tilt, rotation and eraser bits the digitizer is already sending arrive at
//! the window and are dropped on the floor. This module reads them off the same
//! messages, one subclass earlier.
//!
//! # It is a tap, never a filter
//!
//! **Every** message is passed on with `DefSubclassProc`, unconditionally,
//! including the ones we read. Nothing here changes what winit sees, so the
//! mouse and touch streams are byte-for-byte what they were and the shim can
//! be removed without a behavioural diff. `EnableMouseInPointer` is
//! deliberately *not* called: it would route the mouse through the pointer
//! family too and change winit's own input path.
//!
//! # Coexisting with the two other subclasses
//!
//! An HWND in a Teksilo app can carry three subclasses at once — AccessKit's
//! (`WM_GETOBJECT`), Teksilo's custom title bar's (`WM_NC*`, `WM_DPICHANGED`),
//! and this one (`WM_POINTER*`). They coexist because each obeys the same two
//! rules:
//!
//! 1. **A unique subclass id.** Ids come from a process-wide counter in a
//!    private range, as the title-bar host's do — Win32 keys the chain on
//!    `(proc, id)`, and a collision would silently replace another subclass's
//!    entry.
//! 2. **Always chain.** Every path ends in `DefSubclassProc`, so the rest of
//!    the chain and finally winit's own window procedure run exactly as
//!    before.
//!
//! Message disjointness makes that safe rather than merely polite: the three
//! sets do not overlap.
//!
//! # Touch contact geometry comes along for free
//!
//! `WM_TOUCH` — the path winit 0.30 takes for fingers — carries no contact
//! area. `POINTER_TOUCH_INFO::rcContact` does, and the palm heuristic and the
//! finger-avoiding overlay placement both want it. Since the subclass is
//! already reading pointer messages, it records the patch per contact id and
//! answers [`PenSource::touch_contact`](crate::pen::PenSource::touch_contact)
//! from it; the translator folds that into
//! `PointerAxes::contact` for the matching winit `Touch`.
//!
//! # Layout, and testing it without Windows
//!
//! The decoding lives in [`decode`], over a `&[u8]`, and is compiled on every
//! target so that recorded layouts can be decoded on a machine that has no
//! Windows. The live path here does the same thing to the bytes the OS just
//! wrote, so the tested code and the shipped code are one.
//!
//! Reference: `docs/touch-and-pen.md`, "Pen and stylus".

pub mod decode;

#[cfg(target_os = "windows")]
pub use imp::WindowsPenSource;

#[cfg(target_os = "windows")]
mod imp {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use raw_window_handle::RawWindowHandle;
    use teksilo_canvas::Size;
    use teksilo_core::raw_handle::ParentHandle;
    use teksilo_core::trace_input;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::Pointer::{
        GetPointerPenInfo, GetPointerTouchInfo, GetPointerType, POINTER_PEN_INFO,
        POINTER_TOUCH_INFO,
    };
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        POINTER_INPUT_TYPE, WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP,
        WM_POINTERUPDATE,
    };

    use super::decode;
    use crate::pen::{PenCaps, PenPacket, PenSource};

    /// Subclass ids for the pen tap. A private range of its own, distinct from
    /// the title bar's `0xFE_111_000`, so the two can never collide on one
    /// HWND.
    static NEXT_SUBCLASS_ID: AtomicUsize = AtomicUsize::new(0xFE_112_000);

    /// How many contact patches to remember. A hand plus a stylus is seven
    /// contacts; the cap only exists so a driver that never reuses ids cannot
    /// grow the map without bound.
    const MAX_REMEMBERED_CONTACTS: usize = 16;

    /// State shared between the subclass procedure and the source.
    ///
    /// Both run on the UI thread — Win32 marshals every `SendMessage` onto the
    /// HWND-owning thread — so this is an `Rc` with `RefCell`s rather than
    /// anything atomic. The `try_borrow_mut` discipline is for **re-entry**
    /// (a `SendMessage` from inside the proc), the same reason the title-bar
    /// host's `Mutex` uses `try_lock`.
    #[derive(Debug, Default)]
    struct PenShared {
        /// Packets buffered since the last poll, oldest first.
        packets: RefCell<Vec<PenPacket>>,
        /// The most recent contact patch per OS contact id, in logical pixels.
        contacts: RefCell<HashMap<u64, Size>>,
    }

    /// A pen source reading `WM_POINTER*` off one window.
    #[derive(Debug)]
    pub struct WindowsPenSource {
        hwnd: HWND,
        subclass_id: usize,
        /// Kept alive here for the proc's lifetime; the proc borrows it
        /// through the raw pointer in `dwRefData`, and `Drop` removes the
        /// subclass before this `Rc` goes away.
        shared: Rc<PenShared>,
    }

    impl WindowsPenSource {
        /// Install the tap on `parent`'s HWND, or `None` if it is not a Win32
        /// window or the subclass will not install.
        ///
        /// A failure is logged once and degrades to the null source: a missing
        /// pen is a missing capability, never a broken window.
        pub fn attach(parent: &ParentHandle) -> Option<Self> {
            let RawWindowHandle::Win32(handle) = parent.raw_window_handle() else {
                return None;
            };
            let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);

            let shared = Rc::new(PenShared::default());
            let subclass_id = NEXT_SUBCLASS_ID.fetch_add(1, Ordering::Relaxed);
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(teksilo_pen_proc),
                    subclass_id,
                    Rc::as_ptr(&shared) as usize,
                )
            };
            if !installed.as_bool() {
                trace_input!(
                    Samples,
                    "pen: SetWindowSubclass returned FALSE; this window has no pen input"
                );
                return None;
            }
            Some(Self {
                hwnd,
                subclass_id,
                shared,
            })
        }
    }

    impl Drop for WindowsPenSource {
        fn drop(&mut self) {
            // Stop the proc before the `Rc` it borrows through `dwRefData`
            // goes away. `RemoveWindowSubclass` is synchronous on this thread.
            unsafe {
                let _ = RemoveWindowSubclass(self.hwnd, Some(teksilo_pen_proc), self.subclass_id);
            }
        }
    }

    impl PenSource for WindowsPenSource {
        fn poll(&mut self, out: &mut Vec<PenPacket>) {
            if let Ok(mut packets) = self.shared.packets.try_borrow_mut() {
                out.append(&mut packets);
            }
        }

        fn capabilities(&self) -> PenCaps {
            PenCaps {
                touch_contact: true,
                ..PenCaps::FULL_PEN
            }
        }

        fn touch_contact(&self, os_contact_id: u64) -> Option<Size> {
            self.shared
                .contacts
                .try_borrow()
                .ok()?
                .get(&os_contact_id)
                .copied()
        }
    }

    /// The window's DPI scale factor, matching what winit reports.
    fn scale_factor(hwnd: HWND) -> f32 {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
    }

    /// The client area's origin in screen physical pixels, derived from a
    /// point we already have.
    ///
    /// `ScreenToClient` rather than a stored origin: it is exact after a move,
    /// a DPI change or a mirrored (RTL) client area, none of which the shim
    /// would otherwise be told about.
    fn client_origin(hwnd: HWND, screen: (i32, i32)) -> (i32, i32) {
        let mut point = POINT {
            x: screen.0,
            y: screen.1,
        };
        let ok = unsafe { ScreenToClient(hwnd, &mut point) };
        if !ok.as_bool() {
            return (0, 0);
        }
        (screen.0 - point.x, screen.1 - point.y)
    }

    /// A `#[repr(C)]` struct as the bytes the decoder reads.
    ///
    /// # Safety
    ///
    /// `value` must be **fully** initialised, padding included — which is why
    /// both callers below build their struct with `zeroed()` rather than
    /// `default()`. `POINTER_INFO` has four bytes of tail padding, and
    /// `default()` initialises fields, not padding; forming a `&[u8]` over a
    /// range containing uninitialised bytes is not something to do on the
    /// strength of "the OS will have written them".
    ///
    /// Which offsets the decoder then reads is checked against these very
    /// structs at compile time (see `decode::abi_assertions`).
    unsafe fn as_bytes<T>(value: &T) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(value as *const T as *const u8, std::mem::size_of::<T>())
        }
    }

    /// Read one pen packet for `pointer_id`, if the OS still has it.
    fn read_pen(hwnd: HWND, pointer_id: u32, left_window: bool) -> Option<PenPacket> {
        // `zeroed`, not `default`: see `as_bytes`.
        let mut info: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetPointerPenInfo(pointer_id, &mut info) }.ok()?;
        let raw = decode::decode_pen_info(unsafe { as_bytes(&info) })?;
        let mut packet = raw.to_packet(client_origin(hwnd, raw.screen), scale_factor(hwnd));
        if left_window {
            // A tool that has left this window is, as far as this window can
            // tell, out of range: the hover must end here even though the
            // digitizer can still see the pen over the desktop.
            packet.in_proximity = false;
            packet.down = false;
        }
        Some(packet)
    }

    /// Read one contact patch for `pointer_id`, if the OS reports one.
    fn read_touch_contact(hwnd: HWND, pointer_id: u32) -> Option<Size> {
        let mut info: POINTER_TOUCH_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetPointerTouchInfo(pointer_id, &mut info) }.ok()?;
        decode::decode_touch_info(unsafe { as_bytes(&info) })?.contact_size(scale_factor(hwnd))
    }

    /// The subclass procedure. Reads, records, and **always** chains.
    unsafe extern "system" fn teksilo_pen_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid: usize,
        dw_ref_data: usize,
    ) -> LRESULT {
        if dw_ref_data != 0 {
            let shared: &PenShared = unsafe { &*(dw_ref_data as *const PenShared) };
            observe(hwnd, msg, wparam, shared);
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    /// The half of the proc that has nothing unsafe left in it.
    fn observe(hwnd: HWND, msg: u32, wparam: WPARAM, shared: &PenShared) {
        if !matches!(
            msg,
            WM_POINTERUPDATE | WM_POINTERDOWN | WM_POINTERUP | WM_POINTERENTER | WM_POINTERLEAVE
        ) {
            return;
        }
        // GET_POINTERID_WPARAM: the id is the low word.
        let pointer_id = (wparam.0 & 0xFFFF) as u32;

        let mut kind = POINTER_INPUT_TYPE::default();
        if unsafe { GetPointerType(pointer_id, &mut kind) }.is_err() {
            return;
        }
        match kind.0 as u32 {
            decode::PT_PEN => {
                let left = msg == WM_POINTERLEAVE;
                if let Some(packet) = read_pen(hwnd, pointer_id, left)
                    && let Ok(mut packets) = shared.packets.try_borrow_mut()
                {
                    packets.push(packet);
                }
            }
            decode::PT_TOUCH => {
                let Ok(mut contacts) = shared.contacts.try_borrow_mut() else {
                    return;
                };
                if msg == WM_POINTERUP || msg == WM_POINTERLEAVE {
                    contacts.remove(&(pointer_id as u64));
                    return;
                }
                if let Some(size) = read_touch_contact(hwnd, pointer_id) {
                    if contacts.len() >= MAX_REMEMBERED_CONTACTS
                        && !contacts.contains_key(&(pointer_id as u64))
                    {
                        contacts.clear();
                    }
                    contacts.insert(pointer_id as u64, size);
                }
            }
            _ => {}
        }
    }
}
