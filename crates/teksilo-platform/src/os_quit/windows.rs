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
//! `TRUE`: the user asked to end the session, and Microsoft asks
//! applications not to block it ("Applications should respect the user's
//! intentions and return TRUE"); a block (`ShutdownBlockReasonCreate`)
//! puts every user in front of the full-screen "this app is preventing
//! shutdown" list, where they force the shutdown anyway.
//!
//! **It quits on `WM_QUERYENDSESSION`, before answering.** A real sign-out
//! on Windows 11 ended the application about five seconds after that
//! message without ever sending it `WM_ENDSESSION` (measured: both messages
//! traced on the main thread and on a second thread with a window of its
//! own; only the first arrived, the main thread answering throughout). So
//! the procedure quits there: every close guard is asked once with
//! `QuitSource::SessionEnd` and saves without asking, nothing can refuse,
//! and the settings are written, then it answers `TRUE`. Should another
//! application cancel the sign-out after that, this one has quit, its work
//! saved. `WM_ENDSESSION`, when it comes first or alone, does the same.
//!
//! **It quits before the procedure returns.** The procedure posts a
//! [`SessionEndNow`] and runs the thread's messages itself (winit's among
//! them, which deliver the posted request to the event loop's handler)
//! until the application says it has finished, or for at most
//! [`SESSION_END_BUDGET`], under the five seconds Windows waits for an
//! answer. winit hands a message dispatched this way to its handler
//! directly, as it does during the OS's own modal loops (a window being
//! resized); were its handler running already (an application blocked
//! inside a native modal of its own), the request would wait, and
//! `give_up` runs at the end of the budget so the settings at least are
//! written.
//!
//! `WM_CLOSE` (what `taskkill` without `/F` sends a top-level window of the
//! process) is ignored here: the application's own windows hear it and
//! close through their guards, and this window must outlive them.
//!
//! **winit's own hidden window is kept too.** winit's event loop runs on a
//! window of its own ("Winit Thread Event Target"), visible to Windows
//! though never drawn, whose procedure hands `WM_CLOSE` to the default one,
//! which destroys it. With no window of the application open, `taskkill`
//! closed that one, and the application, alive, heard nothing from then on:
//! not its own event proxy, not the end of the session. The watch
//! subclasses that window: a `WM_CLOSE` reaching it asks the application to
//! quit (`on_close`), as `SIGTERM` does on Unix, and never destroys it.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::{
    CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    EnumThreadWindows, GWLP_USERDATA, GetClassNameW, GetWindowLongPtrW, IsWindow, MSG,
    MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, PostQuitMessage, QS_ALLINPUT,
    RegisterClassW, SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE, WM_CLOSE, WM_ENDSESSION,
    WM_NCCREATE, WM_NCDESTROY, WM_QUERYENDSESSION, WM_QUIT, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_OVERLAPPED,
};
use windows::core::BOOL;
use windows::core::{PCWSTR, w};

use super::{ConsoleClosing, SESSION_END_BUDGET, SessionEndNow};

/// What the window's procedure reads, through `GWLP_USERDATA`; owned by
/// the [`SessionEndWatch`].
struct State {
    post: Box<dyn Fn(SessionEndNow)>,
    give_up: Box<dyn Fn()>,
    /// The session's end is being handled (`WM_QUERYENDSESSION` or
    /// `WM_ENDSESSION`): once is enough.
    ending: Cell<bool>,
}

/// Hears the session ending (Windows), and keeps winit's hidden window from
/// a `WM_CLOSE`; see the [module](self). Dropping it destroys its window and
/// takes its subclass off winit's.
pub struct SessionEndWatch {
    hwnd: HWND,
    /// What the window's procedure reads, owned here: the window only
    /// borrows it through `GWLP_USERDATA`, which `WM_NCDESTROY` clears, so
    /// it is freed exactly once whichever way the window goes.
    state: *mut State,
    /// winit's hidden window, subclassed, and what its `WM_CLOSE` calls,
    /// owned here and borrowed by the subclass through `dwRefData`.
    target: Option<(HWND, *mut OnClose)>,
}

/// What a `WM_CLOSE` on winit's hidden window calls.
type OnClose = Box<dyn Fn()>;

/// The class of winit's hidden event-loop window (winit 0.30).
const WINIT_TARGET_CLASS: &str = "Winit Thread Event Target";

/// The subclass id on winit's window: a private value, distinct from the
/// title bar's and the pen tap's ranges.
const TARGET_SUBCLASS_ID: usize = 0xFE_113_000;

impl SessionEndWatch {
    /// Create the hidden window, on the thread that runs the event loop.
    /// When the session ends, `post` is handed a [`SessionEndNow`] to
    /// deliver to the event loop, and the thread's messages run until it
    /// is [finished](SessionEndNow::finished); `give_up` runs when it is
    /// not within [`SESSION_END_BUDGET`]. `on_close` runs for a `WM_CLOSE`
    /// sent to winit's hidden window, which it no longer destroys. `None`
    /// when the window cannot be created.
    pub fn new(
        post: impl Fn(SessionEndNow) + 'static,
        give_up: impl Fn() + 'static,
        on_close: impl Fn() + 'static,
    ) -> Option<Self> {
        let class = w!("TeksiloSessionEndWatch");
        let state = Box::new(State {
            post: Box::new(post),
            give_up: Box::new(give_up),
            ending: Cell::new(false),
        });
        // SAFETY: plain Win32 calls on this thread; `state` is lent to the
        // window through `lpParam`, owned by the watch, and freed here when
        // the window is not created (a window that failed after
        // `WM_NCCREATE` was destroyed, and its `WM_NCDESTROY` let go of it).
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
                Ok(hwnd) => Some(Self {
                    hwnd,
                    state,
                    target: keep_winit_target(Box::new(on_close)),
                }),
                Err(_) => {
                    drop(Box::from_raw(state));
                    None
                }
            }
        }
    }
}

impl SessionEndWatch {
    /// Whether winit's hidden window was found and subclassed, so a
    /// `WM_CLOSE` on it quits the application rather than destroying it.
    /// `false` with another winit, whose window has another class name.
    pub fn keeps_winit_target(&self) -> bool {
        self.target.is_some()
    }
}

impl Drop for SessionEndWatch {
    fn drop(&mut self) {
        // SAFETY: our window, on the thread that created it; the subclass
        // comes off winit's window before what it borrows is dropped.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.state));
            if let Some((target, on_close)) = self.target.take() {
                // Freed only when nothing can call it any more: the subclass
                // removed, or winit's window gone with it. Otherwise it is
                // kept for the process's life.
                if RemoveWindowSubclass(target, Some(target_proc), TARGET_SUBCLASS_ID).as_bool()
                    || !IsWindow(Some(target)).as_bool()
                {
                    drop(Box::from_raw(on_close));
                }
            }
        }
    }
}

/// Subclass winit's hidden window on this thread so a `WM_CLOSE` calls
/// `on_close` instead of destroying it. `None` when there is no such window
/// (another winit version, another class name) or the subclass will not
/// install.
fn keep_winit_target(on_close: OnClose) -> Option<(HWND, *mut OnClose)> {
    unsafe extern "system" fn find(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut class = [0u16; 64];
        // SAFETY: `found` is the `Option<HWND>` `keep_winit_target` passes,
        // alive for the enumeration.
        unsafe {
            let len = GetClassNameW(hwnd, &mut class) as usize;
            if String::from_utf16_lossy(&class[..len]) == WINIT_TARGET_CLASS {
                *(found.0 as *mut Option<HWND>) = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut found: Option<HWND> = None;
    // SAFETY: plain Win32 calls on the event loop's thread; the box handed
    // to the subclass is taken back if it does not install, or in `Drop`.
    unsafe {
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(find),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        );
        let target = found?;
        let on_close = Box::into_raw(Box::new(on_close));
        if SetWindowSubclass(
            target,
            Some(target_proc),
            TARGET_SUBCLASS_ID,
            on_close as usize,
        )
        .as_bool()
        {
            Some((target, on_close))
        } else {
            drop(Box::from_raw(on_close));
            None
        }
    }
}

/// The subclass on winit's hidden window: `WM_CLOSE` asks to quit and goes
/// no further; everything else reaches winit.
unsafe extern "system" fn target_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    on_close: usize,
) -> LRESULT {
    // SAFETY: `on_close` is the box `keep_winit_target` handed over, alive
    // until `Drop` removes this subclass.
    unsafe {
        if msg == WM_CLOSE {
            (*(on_close as *const OnClose))();
            return LRESULT(0);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
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
                // The watch owns the state: the window only lets go of it.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_QUERYENDSESSION => {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
                if let Some(state) = state.as_ref()
                    && !state.ending.replace(true)
                {
                    // Not carried out in time: the next session end
                    // tries again rather than meeting the latch.
                    if !end_session(state) {
                        state.ending.set(false);
                    }
                }
                LRESULT(1)
            }
            WM_ENDSESSION => {
                let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
                if wparam.0 != 0
                    && let Some(state) = state.as_ref()
                    && !state.ending.replace(true)
                {
                    // Not carried out in time: the next session end
                    // tries again rather than meeting the latch.
                    if !end_session(state) {
                        state.ending.set(false);
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => LRESULT(0),
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Post the request and run the thread's messages until it is finished,
/// or until the budget is spent; whether it was finished. One not finished
/// is expired, so the event loop asks nothing when it gets to it later.
fn end_session(state: &State) -> bool {
    let done = Arc::new(AtomicBool::new(false));
    let request = SessionEndNow::new(done.clone());
    (state.post)(request.clone());
    let deadline = Instant::now() + SESSION_END_BUDGET;
    let mut msg = MSG::default();
    while !done.load(Ordering::Acquire) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            request.expire();
            (state.give_up)();
            return false;
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
                        request.expire();
                        (state.give_up)();
                        return false;
                    }
                    return true;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    true
}

/// What the console handler calls: see [`ConsoleWatch::new`].
struct ConsoleCalls {
    post: Box<dyn Fn(ConsoleClosing) + Send + Sync>,
    give_up: Box<dyn Fn() + Send + Sync>,
}

/// The calls of the one [`ConsoleWatch`] alive, read by its handler, which
/// Windows runs on a thread of its own.
static CONSOLE: std::sync::Mutex<Option<ConsoleCalls>> = std::sync::Mutex::new(None);

/// The handler is registered: once per process, and never removed.
/// Removing it waits for a handler that is running, and the running one
/// waits for the process to end, which waited in turn on the removal: the
/// process ended only when the handler's budget ran out, 4 s late.
static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Hears the console the application runs in closing, and the session
/// ending while it runs in one (Windows). An application built as a
/// console program (Rust's default, and what `cargo run` starts) and run
/// from a console was ended by Windows' default handler when that console
/// closed, which a sign-out does too: no guard was asked and nothing was
/// written.
///
/// Windows sends a process that has windows the logoff and shutdown
/// events as `WM_ENDSESSION` instead ([`SessionEndWatch`]); the console
/// closing still arrives here, and is answered as a termination signal is.
/// Ctrl+C and Ctrl+Break keep their default, as `SIGINT` does on Unix.
///
/// Dropped, it leaves the handler registered with nothing to call, so the
/// handler passes every event on to Windows' default.
pub struct ConsoleWatch {
    _private: (),
}

impl ConsoleWatch {
    /// Install the handler. When the console closes or the session ends,
    /// `post` is handed a [`ConsoleClosing`] to deliver to the event loop,
    /// and the handler waits, at most [`SESSION_END_BUDGET`], for it to be
    /// [finished](ConsoleClosing::finished) and for the process to end;
    /// `give_up` runs when it is not finished in time. `None` when the
    /// handler cannot be installed.
    pub fn new(
        post: impl Fn(ConsoleClosing) + Send + Sync + 'static,
        give_up: impl Fn() + Send + Sync + 'static,
    ) -> Option<Self> {
        if !REGISTERED.load(Ordering::Acquire) {
            // SAFETY: registers a plain function, for the process's life.
            unsafe { SetConsoleCtrlHandler(Some(console_handler), true) }.ok()?;
            REGISTERED.store(true, Ordering::Release);
        }
        *CONSOLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(ConsoleCalls {
            post: Box::new(post),
            give_up: Box::new(give_up),
        });
        Some(Self { _private: () })
    }
}

impl Drop for ConsoleWatch {
    fn drop(&mut self) {
        *CONSOLE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl std::fmt::Debug for ConsoleWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleWatch").finish()
    }
}

/// The console handler, on a thread Windows creates for the event. For a
/// close, a logoff or a shutdown, Windows ends the process as soon as this
/// returns: it posts the request, then waits for the application to quit
/// and for the process to end by itself, which ends this thread with it.
unsafe extern "system" fn console_handler(event: u32) -> BOOL {
    let session_end = match event {
        CTRL_CLOSE_EVENT => false,
        CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => true,
        // Ctrl+C, Ctrl+Break: the next handler, by default the end of the
        // process.
        _ => return BOOL(0),
    };
    let done = Arc::new(AtomicBool::new(false));
    {
        let calls = CONSOLE.lock().unwrap_or_else(|e| e.into_inner());
        let Some(calls) = calls.as_ref() else {
            return BOOL(0);
        };
        (calls.post)(ConsoleClosing::new(session_end, done.clone()));
    }
    let deadline = Instant::now() + SESSION_END_BUDGET;
    while !done.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !done.load(Ordering::Acquire) {
        if let Some(calls) = CONSOLE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            (calls.give_up)();
        }
        return BOOL(1);
    }
    // Quit: `run` returns and the process ends on its own, after what
    // follows `run` in `main`. Returning now would end it before that; it
    // has a second for it, or what is left of the budget when the quit was
    // slow.
    let grace = (Instant::now() + std::time::Duration::from_secs(1)).min(deadline);
    while Instant::now() < grace {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    BOOL(1)
}
