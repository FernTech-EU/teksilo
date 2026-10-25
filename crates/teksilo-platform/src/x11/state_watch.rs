// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Hearing an X11 window's `_NET_WM_STATE` change, which winit does not
//! report.
//!
//! A window manager says a window is minimised (`_NET_WM_STATE_HIDDEN`) by
//! rewriting the window's `_NET_WM_STATE`, and winit 0.30 drops the
//! `PropertyNotify` that follows: an iconify reaches an application only as
//! `Focused(false)`, and only when the window had the focus, so a minimise of
//! a window without the focus, or the window manager's own confirmation of a
//! minimise the application asked for, wakes nothing. [`WmStateWatch`]
//! selects `PropertyChangeMask` on the windows it is given, over a connection
//! of its own (an X client may select events on any window, and its
//! selection is its own, so winit's is untouched), and calls back from its
//! thread each time one of their `_NET_WM_STATE` changes.

use std::collections::HashSet;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ChangeWindowAttributesAux, ConnectionExt as _, EventMask, Window};

use super::connection::{X11Connection, ignore_errors};
use super::wake_pipe::{Waker, wait_readable};

/// Watches the `_NET_WM_STATE` of a set of X11 windows from a thread of its
/// own, calling `on_change` there each time one changes, and once whenever
/// windows are added to the set (their state may have changed before they
/// were watched). Stops when dropped.
#[doc(hidden)]
pub struct WmStateWatch {
    shutdown: Arc<AtomicBool>,
    commands: Sender<HashSet<Window>>,
    waker: Waker,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl WmStateWatch {
    /// Start watching nothing yet, on the display named by `$DISPLAY`;
    /// `None` when there is no X server to connect to, or no thread or pipe
    /// to be had.
    pub fn new(on_change: impl Fn() + Send + 'static) -> Option<Self> {
        let conn = X11Connection::open().ok()?;
        let waker = Waker::new().ok()?;
        let wake_fd = waker.read_fd();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (commands, received) = channel();
        let thread_shutdown = shutdown.clone();
        let thread = std::thread::Builder::new()
            .name("teksilo-x11-wm-state".into())
            .spawn(move || run(conn, &thread_shutdown, &received, wake_fd, &on_change))
            .ok()?;
        Some(Self {
            shutdown,
            commands,
            waker,
            thread: Mutex::new(Some(thread)),
        })
    }

    /// Watch exactly `windows` from now on: those not watched yet are
    /// added, those no longer named are let go. A window that is not an
    /// X11 one is skipped.
    pub fn watch_only<'a>(&self, windows: impl IntoIterator<Item = &'a winit::window::Window>) {
        let ids = windows
            .into_iter()
            .filter_map(crate::window_system::x11_window_id)
            .collect();
        if self.commands.send(ids).is_ok() {
            self.waker.wake();
        }
    }
}

impl Drop for WmStateWatch {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.waker.wake();
        if let Ok(mut slot) = self.thread.lock()
            && let Some(handle) = slot.take()
        {
            // The wake pipe breaks the thread out of `poll` at once.
            let _ = handle.join();
        }
    }
}

/// The watch's thread: select and deselect as told, call `on_change` for
/// each `_NET_WM_STATE` change of a watched window. A broken connection
/// ends it.
fn run(
    conn: X11Connection,
    shutdown: &AtomicBool,
    commands: &Receiver<HashSet<Window>>,
    wake_fd: std::os::fd::RawFd,
    on_change: &dyn Fn(),
) {
    let x_fd = conn.conn().stream().as_raw_fd();
    let state_atom = conn.atoms().net_wm_state;
    let mut watched: HashSet<Window> = HashSet::new();
    while !shutdown.load(Ordering::SeqCst) {
        let mut changed = false;
        while let Ok(wanted) = commands.try_recv() {
            for &gone in watched.difference(&wanted) {
                // A window destroyed meanwhile answers `BadWindow`, and its
                // selection went with it.
                ignore_errors(conn.conn().change_window_attributes(
                    gone,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT),
                ));
            }
            for &new in wanted.difference(&watched) {
                ignore_errors(conn.conn().change_window_attributes(
                    new,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                ));
                changed = true;
            }
            watched = wanted;
        }
        loop {
            match conn.poll_event() {
                Ok(Some(Event::PropertyNotify(notify)))
                    if notify.atom == state_atom && watched.contains(&notify.window) =>
                {
                    changed = true;
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(err) => {
                    eprintln!("teksilo-platform: the X11 window-state watch stopped: {err}");
                    return;
                }
            }
        }
        if changed {
            on_change();
        }
        let _ = conn.flush();
        wait_readable(x_fd, wake_fd, None);
    }
}
