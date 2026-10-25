// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `WM_QUERYENDSESSION` and `WM_ENDSESSION`, on a hidden window of our own.
//!
//! Windows sends both to every top-level window when the user logs off,
//! restarts or shuts down. winit's windows hand them to the default
//! procedure, so the session ended under the application with nothing
//! saved, and an application with no window open (a tray application)
//! has no window to hear them on at all. This window is hidden and never
//! shown, top-level (a message-only window does not receive the
//! broadcast), and lives as long as the watch.
//!
//! **The session is not held up.** `WM_QUERYENDSESSION` is answered
//! `TRUE` at once: the user asked to end the session, and Microsoft asks
//! applications not to block it ("Applications should respect the user's
//! intentions and return TRUE"); a block (`ShutdownBlockReasonCreate`)
//! puts every user in front of the full-screen "this app is preventing
//! shutdown" list, where they force the shutdown anyway. The application
//! quits on `WM_ENDSESSION` instead, which says the session really ends:
//! every close guard is asked once with `QuitSource::SessionEnd` and saves
//! without asking, nothing can refuse, and the settings are written.
//!
//! **It quits before the procedure returns.** The process may be ended as
//! soon as `WM_ENDSESSION` is answered, so the procedure posts a
//! [`SessionEndNow`] and runs the thread's messages itself (winit's among
//! them, which deliver the posted request to the event loop's handler)
//! until the application says it has finished, or for at most
//! [`SESSION_END_BUDGET`]. winit hands a message dispatched this way to its
//! handler directly, as it does during the OS's own modal loops (a window
//! being resized); were its handler running already (an application
//! blocked inside a native modal of its own), the request would wait, and
//! `give_up` runs at the end of the budget so the settings at least are
//! written.
//!
//! `WM_CLOSE` (what `taskkill` without `/F` sends every top-level window of
//! the process) is ignored here: the application's own windows hear it and
//! close through their guards, and this window must outlive them.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetWindowLongPtrW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, PostQuitMessage,
    QS_ALLINPUT, RegisterClassW, SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE,
    WM_ENDSESSION, WM_NCCREATE, WM_NCDESTROY, WM_QUERYENDSESSION, WM_QUIT, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};
use windows::core::{PCWSTR, w};

use super::{SESSION_END_BUDGET, SessionEndNow};

/// What the window's procedure reads, owned through `GWLP_USERDATA`.
struct State {
    post: Box<dyn Fn(SessionEndNow)>,
    give_up: Box<dyn Fn()>,
    /// A `WM_ENDSESSION` is being handled: one is enough.
    ending: Cell<bool>,
}

/// Hears the session ending (Windows); see the [module](self). Dropping it
/// destroys its window.
pub struct SessionEndWatch {
    hwnd: HWND,
}

impl SessionEndWatch {
    /// Create the hidden window, on the thread that runs the event loop.
    /// When the session ends, `post` is handed a [`SessionEndNow`] to
    /// deliver to the event loop, and the thread's messages run until it
    /// is [finished](SessionEndNow::finished); `give_up` runs when it is
    /// not within [`SESSION_END_BUDGET`]. `None` when the window cannot be
    /// created.
    pub fn new(
        post: impl Fn(SessionEndNow) + 'static,
        give_up: impl Fn() + 'static,
    ) -> Option<Self> {
        let class = w!("TeksiloSessionEndWatch");
        let state = Box::new(State {
            post: Box::new(post),
            give_up: Box::new(give_up),
            ending: Cell::new(false),
        });
        // SAFETY: plain Win32 calls on this thread; `state` is handed to the
        // window through `lpParam` and taken back on `WM_NCDESTROY`, or
        // here when the window is not created.
        unsafe {
            let instance = GetModuleHandleW(PCWSTR::null()).ok()?;
            let wc = WNDCLASSW {
                lpfnWndProc: Some(session_end_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            // A second registration of the class fails harmlessly: the
            // first one stands.
            RegisterClassW(&wc);
            let state = Box::into_raw(state);
            match CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
                class,
                w!("Teksilo session end"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                Some(state as *const std::ffi::c_void),
            ) {
                Ok(hwnd) => Some(Self { hwnd }),
                Err(_) => {
                    drop(Box::from_raw(state));
                    None
                }
            }
        }
    }
}

impl Drop for SessionEndWatch {
    fn drop(&mut self) {
        // SAFETY: our window, on the thread that created it.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl std::fmt::Debug for SessionEndWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEndWatch").finish()
    }
}

unsafe extern "system" fn session_end_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the window's own procedure, on its thread; the state pointer
    // is the one `new` handed over and lives until `WM_NCDESTROY`.
    unsafe {
        match msg {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_NCDESTROY => {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !state.is_null() {
                    drop(Box::from_raw(state));
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
                if wparam.0 != 0
                    && let Some(state) = state.as_ref()
                    && !state.ending.replace(true)
                {
                    end_session(state);
                }
                LRESULT(0)
            }
            WM_CLOSE => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Post the request and run the thread's messages until it is finished,
/// or until the budget is spent.
fn end_session(state: &State) {
    let done = Arc::new(AtomicBool::new(false));
    (state.post)(SessionEndNow::new(done.clone()));
    let deadline = Instant::now() + SESSION_END_BUDGET;
    let mut msg = MSG::default();
    while !done.load(Ordering::Acquire) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            (state.give_up)();
            return;
        }
        // SAFETY: the standard message loop, on this thread.
        unsafe {
            MsgWaitForMultipleObjects(None, false, left.as_millis() as u32, QS_ALLINPUT);
            while !done.load(Ordering::Acquire)
                && PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool()
            {
                if msg.message == WM_QUIT {
                    // Someone else's: give it back to the loop that waits
                    // for it, and stop here.
                    PostQuitMessage(msg.wParam.0 as i32);
                    if !done.load(Ordering::Acquire) {
                        (state.give_up)();
                    }
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
