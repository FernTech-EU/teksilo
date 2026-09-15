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
//! # A message is not a packet
//!
//! Windows coalesces the digitizer packets that arrive between two window
//! messages and reports the count in `POINTER_INFO::historyCount`.
//! `GetPointerPenInfo` returns only the newest of them, so a shim built on it
//! alone is capped at the message rate — roughly the display's — no matter how
//! fast the tablet is, and every intermediate packet's pressure and tilt are
//! lost with it. This module calls `GetPointerPenInfoHistory` whenever the
//! count says there is more, and hands the whole run to the translator.
//!
//! The array that call fills is **newest-first**; [`PenSource::poll`] promises
//! oldest-first. [`decode::decode_pen_history`] is the reversal, and it says so
//! at length, because a stroke drawn backwards is a bug that looks like a
//! rendering problem.
//!
//! # Layout, and testing it without Windows
//!
//! The decoding lives in [`decode`], over a `&[u8]`, and is compiled on every
//! target so that recorded layouts can be decoded on a machine that has no
//! Windows. The live path here does the same thing to the bytes the OS just
//! wrote, so the tested code and the shipped code are one.
//!
//! Reference: `docs/touch-and-pen.md`, "Pen and stylus".
//!
//! [`PenSource::poll`]: crate::pen::PenSource::poll

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
        GetPointerPenInfo, GetPointerPenInfoHistory, GetPointerTouchInfo, GetPointerType,
        POINTER_PEN_INFO, POINTER_TOUCH_INFO,
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

    /// Read every pen packet the OS coalesced into the current message for
    /// `pointer_id`, **oldest first**.
    ///
    /// A `WM_POINTERUPDATE` is not one digitizer packet. Windows merges the
    /// packets that arrived since the last message and reports how many in
    /// `POINTER_INFO::historyCount`; `GetPointerPenInfo` — the singular form,
    /// and all this shim used to call — hands back only the newest of them.
    /// That capped a 200-360 Hz stylus at the window message rate, threw away
    /// each intermediate packet's own pressure and tilt, and left the
    /// remaining samples with no spacing to be timed by.
    ///
    /// So: ask `GetPointerPenInfo` once (it is the packet the message is
    /// *about*, and it is the one that answers `historyCount`), and where that
    /// says the message coalesced anything, pull the rest with
    /// `GetPointerPenInfoHistory`. The window's client origin and scale factor
    /// are read once for the whole run: both are properties of the window, and
    /// every entry belongs to the same message.
    fn read_pen(hwnd: HWND, pointer_id: u32, left_window: bool) -> Vec<PenPacket> {
        // `zeroed`, not `default`: see `as_bytes`.
        let mut info: POINTER_PEN_INFO = unsafe { std::mem::zeroed() };
        if unsafe { GetPointerPenInfo(pointer_id, &mut info) }.is_err() {
            return Vec::new();
        }
        let Some(newest) = decode::decode_pen_info(unsafe { as_bytes(&info) }) else {
            return Vec::new();
        };
        let origin = client_origin(hwnd, newest.screen);
        let scale = scale_factor(hwnd);

        if left_window {
            // A tool that has left this window is, as far as this window can
            // tell, out of range: the hover must end here even though the
            // digitizer can still see the pen over the desktop.
            //
            // No history on this path on purpose. A leave is one transition,
            // not a stroke, and replaying its coalesced positions as a run of
            // out-of-range packets would say the tool left several times.
            let mut packet = newest.to_packet(origin, scale);
            packet.in_proximity = false;
            packet.down = false;
            return vec![packet];
        }

        match read_pen_history(pointer_id, newest.history_count) {
            Some(history) => history
                .iter()
                .map(|entry| entry.to_packet(origin, scale))
                .collect(),
            None => vec![newest.to_packet(origin, scale)],
        }
    }

    /// The coalesced packets behind one message, decoded oldest-first.
    ///
    /// `None` when there is nothing to fetch (`history_count` of 0 or 1), when
    /// the call fails, or when it returns nothing decodable — in every case
    /// the caller falls back to the single packet it already has, so a driver
    /// that does not support history costs a failed syscall and nothing else.
    fn read_pen_history(pointer_id: u32, history_count: u32) -> Option<Vec<decode::RawPenInfo>> {
        let wanted = decode::history_entries_to_request(history_count)?;

        // Fully initialised, padding included, before the OS or `as_bytes`
        // ever looks at it — the same reason the singular path uses `zeroed()`
        // rather than `default()`. `write_bytes` over the whole allocation is
        // the only form that promises that for a run of structs: a `vec![v; n]`
        // clones a typed value `n` times, and a typed copy carries no promise
        // about padding bytes.
        let mut buffer: Vec<POINTER_PEN_INFO> = Vec::with_capacity(wanted);
        // SAFETY: `with_capacity(wanted)` allocated room for exactly `wanted`
        // elements, so writing `wanted` zeroed elements stays inside it, and
        // `POINTER_PEN_INFO` is a plain `#[repr(C)]` aggregate of integers,
        // handles and points for which the all-zero bit pattern is valid.
        unsafe {
            std::ptr::write_bytes(buffer.as_mut_ptr(), 0, wanted);
            buffer.set_len(wanted);
        }

        let mut count = wanted as u32;
        unsafe { GetPointerPenInfoHistory(pointer_id, &mut count, Some(buffer.as_mut_ptr())) }
            .ok()?;

        // The OS writes back how many it actually filled; it must never be
        // read as more than was asked for.
        let filled = (count as usize).min(wanted);
        // SAFETY: `buffer` holds `wanted` fully initialised `POINTER_PEN_INFO`
        // values (zeroed above, the first `filled` of them since overwritten
        // by the OS), so `filled * size_of::<POINTER_PEN_INFO>()` bytes from
        // its start are initialised and contiguous.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr() as *const u8,
                filled * std::mem::size_of::<POINTER_PEN_INFO>(),
            )
        };
        // Win32 fills this newest-first; `decode_pen_history` reverses it.
        let decoded = decode::decode_pen_history(bytes, filled);
        (!decoded.is_empty()).then_some(decoded)
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
                let read = read_pen(hwnd, pointer_id, left);
                if !read.is_empty()
                    && let Ok(mut packets) = shared.packets.try_borrow_mut()
                {
                    // Oldest first, appended in order: `PenSource::poll`
                    // hands the whole buffer to the translator as one forward
                    // timeline.
                    packets.extend(read);
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
