// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `taskkill` without `/F` with no window of the application open (Windows
//! only, hence `harness = false`: the event loop runs on the main thread).
//!
//! `taskkill` sends `WM_CLOSE` to a top-level window of the process. With
//! none of the application's open, the only one is winit's hidden
//! event-loop window, whose procedure handed the message to the default
//! one, which destroyed it: the application kept running and heard nothing
//! from then on, not even the end of the session. The message now quits the
//! application as `SIGTERM` does on Unix: the application's guard is asked
//! once, reading `QuitSource::Signal`, and `run` returns.
//!
//! The test posts that `WM_CLOSE` itself, from a thread, to the hidden
//! window of this process; a watchdog fails it when `run` does not return.

#[cfg(target_os = "windows")]
fn main() {
    use std::sync::Mutex;
    use std::time::Duration;

    use teksilo_app::{QuitPolicy, TeksiloAppBuilder};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };
    use windows::core::BOOL;

    /// What the application's guard read, once the loop has ended.
    static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    unsafe extern "system" fn find(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the `Option<HWND>` `winit_target` passes, alive
        // for the enumeration.
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != std::process::id() {
                return BOOL(1);
            }
            let mut class = [0u16; 64];
            let len = GetClassNameW(hwnd, &mut class) as usize;
            if String::from_utf16_lossy(&class[..len]) == "Winit Thread Event Target" {
                *(found.0 as *mut Option<HWND>) = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    fn winit_target() -> Option<isize> {
        let mut found: Option<HWND> = None;
        // SAFETY: a plain enumeration of the desktop's top-level windows.
        unsafe {
            let _ = EnumWindows(Some(find), LPARAM(&mut found as *mut Option<HWND> as isize));
        }
        found.map(|hwnd| hwnd.0 as isize)
    }

    std::thread::spawn(|| {
        // Let the loop start and the watch subclass the window.
        std::thread::sleep(Duration::from_secs(2));
        // A panic here would end this thread only, and the loop would run
        // on: every failure exits the process.
        let Some(target) = winit_target() else {
            eprintln!("os_taskkill: FAIL: no winit hidden window in this process");
            std::process::exit(1);
        };
        // SAFETY: posting a message to a window; the handle is only passed on.
        let posted = unsafe {
            PostMessageW(
                Some(HWND(target as *mut std::ffi::c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        };
        if let Err(e) = posted {
            eprintln!("os_taskkill: FAIL: WM_CLOSE not posted: {e}");
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("os_taskkill: FAIL: run() did not return 30 s after WM_CLOSE");
        std::process::exit(1);
    });

    TeksiloAppBuilder::new()
        .theme(teksilo_core::presets::intui::light())
        .quit_policy(QuitPolicy::Explicit)
        .on_quit_requested(|ctx| {
            ASKED
                .lock()
                .expect("not poisoned")
                .push(format!("{:?}", ctx.quit_source()));
            teksilo_core::CloseResponse::Veto
        })
        .run();

    let asked = ASKED.lock().expect("not poisoned").clone();
    assert_eq!(
        asked,
        ["Some(Signal)"],
        "the application's guard was asked once, as for SIGTERM, and could not refuse"
    );
    println!("os_taskkill: ok");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("os_taskkill: Windows only");
}
