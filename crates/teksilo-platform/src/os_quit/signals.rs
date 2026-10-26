// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SIGTERM` and `SIGHUP`, heard on a thread of their own.
//!
//! One thread and one registration for the life of the process, whatever
//! the number of watches: the signal-hook registration cannot give the
//! default disposition back once made (removing the last action leaves the
//! signal ignored), so the thread stays, and with no watch in place it does
//! what the default would, ending the process. A watch therefore changes
//! the meaning of the two signals only while it lives, which is while the
//! event loop runs.
//!
//! A signal ignored when the listener starts (as `nohup` leaves `SIGHUP`
//! for the program it runs, and a daemon's parent may leave `SIGTERM`) is
//! left alone: it is not registered, so it stays ignored, during the loop
//! and after it.
//!
//! A disposition the application sets itself once the listener runs (a
//! `signal(SIGTERM, SIG_IGN)` of its own, or a handler of another library)
//! replaces the registration, and is the application's choice: the watch
//! hears that signal no more, and nothing says so. An application that
//! handles the two signals itself turns the watch off instead
//! (`TeksiloAppBuilder::termination_signals(false)`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use signal_hook::consts::{SIGHUP, SIGTERM};
use signal_hook::iterator::Signals;

use super::TerminationSignal;

/// Where a signal goes: the watch in place, if any.
type Post = Box<dyn Fn(TerminationSignal) + Send>;

/// The watch in place, with the number it was given.
static POST: Mutex<Option<(u64, Post)>> = Mutex::new(None);

/// Start the process's one listener, the first time.
fn start_listener() -> std::io::Result<()> {
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    STARTED
        .get_or_init(|| {
            let wanted: Vec<i32> = [SIGTERM, SIGHUP]
                .into_iter()
                .filter(|&signal| !ignored(signal))
                .collect();
            if wanted.is_empty() {
                return Ok(());
            }
            let mut signals = Signals::new(wanted).map_err(|e| e.to_string())?;
            std::thread::Builder::new()
                .name("teksilo-quit-signals".into())
                .spawn(move || {
                    for signal in signals.forever() {
                        deliver(signal);
                    }
                })
                .map(drop)
                .map_err(|e| e.to_string())
        })
        .clone()
        .map_err(std::io::Error::other)
}

/// Whether `signal` is ignored now, which before the listener starts is
/// what the process inherited or set for itself.
fn ignored(signal: i32) -> bool {
    // SAFETY: with a null new action `sigaction` only reads the current
    // disposition into `old`, a plain struct it fills.
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        libc::sigaction(signal, std::ptr::null(), &mut old) == 0
            && old.sa_sigaction == libc::SIG_IGN
    }
}

fn post_slot() -> std::sync::MutexGuard<'static, Option<(u64, Post)>> {
    POST.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Hand `signal` to the watch in place, or end the process as the default
/// disposition would with none.
fn deliver(signal: i32) {
    let post = post_slot();
    match post.as_ref() {
        Some((_, post)) => post(TerminationSignal { signal }),
        None => {
            drop(post);
            let _ = signal_hook::low_level::emulate_default_handler(signal);
        }
    }
}

/// Turns `SIGTERM` and `SIGHUP` into a [`TerminationSignal`] handed to
/// `post`, on the listener's thread, while it lives. Dropping it gives
/// both signals their default meaning back: the process ends. A signal the
/// process ignored when the first watch started stays ignored throughout.
///
/// One watch at a time: a second replaces the first, whose drop then
/// changes nothing.
pub struct SignalWatch {
    number: u64,
}

impl SignalWatch {
    /// Start listening, or take the listener over. Fails when the signals
    /// cannot be registered or the thread cannot start.
    pub fn new(post: impl Fn(TerminationSignal) + Send + 'static) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        // In place before the listener starts: a signal it read before
        // finding a watch would end the process as the default does.
        *post_slot() = Some((number, Box::new(post)));
        let watch = Self { number };
        start_listener()?;
        Ok(watch)
    }
}

impl Drop for SignalWatch {
    fn drop(&mut self) {
        let mut slot = post_slot();
        if slot
            .as_ref()
            .is_some_and(|(number, _)| *number == self.number)
        {
            *slot = None;
        }
    }
}

impl std::fmt::Debug for SignalWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalWatch").finish()
    }
}
