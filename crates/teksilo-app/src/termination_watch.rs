// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`TerminationWatch`]: `SIGTERM` and `SIGHUP` kept from ending the process
//! for as long as the application holds the watch, before, during and after
//! the event loop.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use teksilo_platform::os_quit::TerminationSignal;

/// Which termination signal arrived; see [`TerminationWatch::signalled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TerminationSignalKind {
    /// `SIGTERM`: a `kill`, a service manager stopping the application, the
    /// session ending.
    Terminate,
    /// `SIGHUP`: the terminal or the login session that started the
    /// application closed.
    Hangup,
}

// Only the listener, which exists on Unix, hears a signal.
#[cfg_attr(not(unix), allow(dead_code))]
impl TerminationSignalKind {
    fn from_signal(signal: i32) -> Self {
        if signal == teksilo_platform::os_quit::SIGHUP {
            Self::Hangup
        } else {
            Self::Terminate
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Terminate => 1,
            Self::Hangup => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Terminate),
            2 => Some(Self::Hangup),
            _ => None,
        }
    }
}

/// Where a signal goes while an event loop runs.
type ToLoop = Box<dyn Fn(TerminationSignal) + Send>;

/// What the listener's thread writes and the application reads. Apart from
/// the watch itself, so the listener's closure holds no cycle.
struct Heard {
    /// The last signal's [`TerminationSignalKind::code`], 0 before any.
    last: AtomicU8,
    count: AtomicU32,
    /// The running loop. Recording a signal and handing it on happen under
    /// this lock, so a signal is either seen as heard by
    /// [`TerminationWatch::attach_loop`] or handed to the loop, never both
    /// nor neither.
    to_loop: Mutex<Option<ToLoop>>,
}

#[cfg_attr(not(unix), allow(dead_code))]
impl Heard {
    fn to_loop(&self) -> MutexGuard<'_, Option<ToLoop>> {
        self.to_loop.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn hear(&self, signal: TerminationSignal) {
        let to_loop = self.to_loop();
        self.last.store(
            TerminationSignalKind::from_signal(signal.signal).code(),
            Ordering::Release,
        );
        self.count.fetch_add(1, Ordering::AcqRel);
        if let Some(post) = to_loop.as_ref() {
            post(signal);
        }
    }
}

struct Shared {
    heard: Arc<Heard>,
    #[cfg(unix)]
    _signals: teksilo_platform::os_quit::SignalWatch,
}

/// The process's watch while one lives: [`TerminationWatch::install`] hands
/// it out again rather than starting a second, which would take the
/// listener from the first.
static CURRENT: Mutex<Weak<Shared>> = Mutex::new(Weak::new());

fn current_slot() -> MutexGuard<'static, Weak<Shared>> {
    CURRENT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps `SIGTERM` and `SIGHUP` from ending the process for as long as a
/// handle to it lives (Unix), so an application that starts a backend
/// before its event loop and shuts it down after can finish both.
///
/// Created at the top of `main`, before anything that a signal should not
/// interrupt, and kept until `main` returns:
///
/// - **Before the event loop**, a signal is remembered:
///   [`signalled`](Self::signalled) tells `main`, which can skip straight
///   to its shutdown. An application that goes on to
///   [`run`](crate::TeksiloAppBuilder::run) anyway quits as its loop
///   starts, before its first window is created, through the quit a signal
///   asks for while the loop runs.
/// - **While the event loop runs**, given to the builder with
///   [`TeksiloAppBuilder::termination_watch`](crate::TeksiloAppBuilder::termination_watch),
///   a signal quits the application as
///   [`handle_termination_signals`](crate::TeksiloAppBuilder::handle_termination_signals)
///   describes: every guard is asked once with
///   [`QuitSource::Signal`](teksilo_core::QuitSource::Signal), nothing can
///   refuse, the settings are written, and the loop ends.
/// - **Once the loop has ended**, a signal is only remembered again, so a
///   bounded shutdown finishes; [`signals_received`](Self::signals_received)
///   tells a signal that came during it from the one that caused the quit.
///
/// ```ignore
/// use teksilo::prelude::*;
///
/// fn main() {
///     // First, so a signal while the backend starts is remembered.
///     let watch = TerminationWatch::install().expect("termination signals");
///     let backend = Backend::start();
///     if watch.signalled().is_none() {
///         TeksiloAppBuilder::new()
///             .termination_watch(&watch)
///             .initial_window(WindowConfig::new().root(|tree, _| tree.add(Root::new())))
///             .run();
///     }
///     // A signal now is counted, and the shutdown runs to its bound.
///     backend.shutdown(std::time::Duration::from_secs(5));
///     drop(watch);
/// }
/// ```
///
/// **macOS.** When AppKit carries out a quit (the Dock's Quit, a logout),
/// `run` does not return: the process ends once the application's state
/// has been dropped. Do the shutdown in the `Drop` of a service registered
/// with [`app_state`](crate::TeksiloAppBuilder::app_state), which runs as
/// the loop ends, and give that service a clone of the watch if it needs to
/// read [`signals_received`](Self::signals_received); a signal during that
/// `Drop` is remembered as it is after `run` returns elsewhere.
///
/// **Windows** has no `SIGTERM` or `SIGHUP`: `install` succeeds and the
/// watch does nothing ([`signalled`](Self::signalled) is always `None`), so
/// one `main` serves every platform. The ways Windows asks an application
/// to quit (the session ending, its console closing, `taskkill`) are heard
/// while the loop runs whether or not a watch exists.
///
/// The rules of the loop's own handling hold here too. A signal the
/// process was started with ignored (`SIGHUP` under `nohup`) stays ignored
/// and is never counted. A disposition the application sets itself once the
/// watch exists (a `signal(SIGTERM, SIG_IGN)`, another library's handler)
/// replaces the watch's for that signal, silently: that is the
/// application's choice.
///
/// Every handle is a clone of one watch, and `install` called again while
/// one lives returns another handle to it. When the last handle is dropped
/// the two signals end the process again, as they do for any program: an
/// application that wants cover until the very end keeps a handle until
/// `main` returns.
#[derive(Clone)]
pub struct TerminationWatch {
    shared: Arc<Shared>,
}

impl TerminationWatch {
    /// Start keeping `SIGTERM` and `SIGHUP` from ending the process, or
    /// return the watch already in place. Fails when the signals cannot be
    /// registered or the listener's thread cannot start; the two signals
    /// then keep their default meaning.
    pub fn install() -> std::io::Result<Self> {
        let mut current = current_slot();
        if let Some(shared) = current.upgrade() {
            return Ok(Self { shared });
        }
        let heard = Arc::new(Heard {
            last: AtomicU8::new(0),
            count: AtomicU32::new(0),
            to_loop: Mutex::new(None),
        });
        #[cfg(unix)]
        let signals = {
            let heard = heard.clone();
            teksilo_platform::os_quit::SignalWatch::new(move |signal| heard.hear(signal))?
        };
        let shared = Arc::new(Shared {
            heard,
            #[cfg(unix)]
            _signals: signals,
        });
        *current = Arc::downgrade(&shared);
        Ok(Self { shared })
    }

    /// The watch in place in the process, if one lives.
    pub(crate) fn current() -> Option<Self> {
        current_slot().upgrade().map(|shared| Self { shared })
    }

    /// The last termination signal that arrived, `None` before any.
    pub fn signalled(&self) -> Option<TerminationSignalKind> {
        TerminationSignalKind::from_code(self.shared.heard.last.load(Ordering::Acquire))
    }

    /// How many termination signals have arrived since the watch was
    /// installed. Read before a shutdown starts, it tells a signal that
    /// came during the shutdown from the one that caused the quit.
    pub fn signals_received(&self) -> u32 {
        self.shared.heard.count.load(Ordering::Acquire)
    }

    /// Hand every signal from now on to `post` (the running loop), and say
    /// whether one arrived before.
    pub(crate) fn attach_loop(&self, post: impl Fn(TerminationSignal) + Send + 'static) -> bool {
        let mut to_loop = self.shared.heard.to_loop();
        *to_loop = Some(Box::new(post));
        self.signals_received() > 0
    }

    /// The loop has ended: a signal is only remembered from now on.
    pub(crate) fn detach_loop(&self) {
        *self.shared.heard.to_loop() = None;
    }
}

impl std::fmt::Debug for TerminationWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminationWatch")
            .field("signalled", &self.signalled())
            .field("signals_received", &self.signals_received())
            .finish()
    }
}

// Each test sends real signals, so each runs its claims in a child process
// of its own (this same test, told so by `CHILD`): the parent's harness and
// its other tests never see a signal, and each child starts with the
// signals as the process was given them.
#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use teksilo_platform::os_quit::{SIGHUP, SIGTERM, TerminationSignal};

    use super::{TerminationSignalKind, TerminationWatch};

    /// Set, the test is the child that runs the claims.
    const CHILD: &str = "TEKSILO_TERMINATION_WATCH_CHILD";

    /// Run `test` again as a child process (under `nohup` when `nohup` is
    /// set) and return how it ended; `None` when this process is the child,
    /// which then runs the claims.
    fn in_child(test: &str, nohup: bool) -> Option<std::process::ExitStatus> {
        if std::env::var_os(CHILD).is_some() {
            return None;
        }
        let name = format!(
            "{}::{test}",
            module_path!()
                .split_once("::")
                .map_or(module_path!(), |(_, rest)| rest)
        );
        let exe = std::env::current_exe().expect("this test");
        let mut command = if nohup {
            let mut command = std::process::Command::new("nohup");
            command.arg(exe);
            command
        } else {
            std::process::Command::new(exe)
        };
        let mut child = command
            .args([name.as_str(), "--exact", "--test-threads=1"])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("the child starts");
        let started = Instant::now();
        Some(loop {
            if let Some(status) = child.try_wait().expect("the child is ours") {
                break status;
            }
            if started.elapsed() > Duration::from_secs(60) {
                let _ = child.kill();
                panic!("the child never ended");
            }
            std::thread::sleep(Duration::from_millis(20));
        })
    }

    /// Send `signal` (as `kill` spells it) to this process.
    fn send(signal: &str) {
        let sent = std::process::Command::new("kill")
            .args([signal, &std::process::id().to_string()])
            .status()
            .expect("kill runs");
        assert!(sent.success());
    }

    /// Wait until `watch` has counted `count` signals.
    fn wait_for(watch: &TerminationWatch, count: u32) {
        let started = Instant::now();
        while watch.signals_received() < count {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the watch heard {} signals, not {count}",
                watch.signals_received()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Before any event loop, a signal is remembered and the process lives:
    /// `signalled` names the last one, `signals_received` counts them, and
    /// a second `install` is the same watch.
    #[test]
    fn a_signal_with_no_loop_is_remembered_and_ends_nothing() {
        if let Some(status) = in_child(
            "a_signal_with_no_loop_is_remembered_and_ends_nothing",
            false,
        ) {
            assert!(
                status.success(),
                "the child lived and its claims held: {status:?}"
            );
            return;
        }
        let watch = TerminationWatch::install().expect("the watch");
        assert_eq!(watch.signalled(), None);
        send("-TERM");
        wait_for(&watch, 1);
        assert_eq!(watch.signalled(), Some(TerminationSignalKind::Terminate));
        let again = TerminationWatch::install().expect("the same watch");
        send("-HUP");
        wait_for(&again, 2);
        assert_eq!(watch.signals_received(), 2);
        assert_eq!(watch.signalled(), Some(TerminationSignalKind::Hangup));
    }

    /// Dropping the last handle gives the signals their default meaning
    /// back: the next `SIGTERM` ends the process, killed by it.
    #[test]
    fn dropping_the_last_handle_lets_the_signal_end_the_process() {
        if let Some(status) = in_child(
            "dropping_the_last_handle_lets_the_signal_end_the_process",
            false,
        ) {
            assert_eq!(
                status.signal(),
                Some(SIGTERM),
                "killed by SIGTERM: {status:?}"
            );
            return;
        }
        let watch = TerminationWatch::install().expect("the watch");
        let other = watch.clone();
        drop(watch);
        send("-TERM");
        wait_for(&other, 1);
        drop(other);
        send("-TERM");
        std::thread::sleep(Duration::from_secs(10));
        panic!("SIGTERM did not end the process once the watch was gone");
    }

    /// A signal the process was started with ignored (`nohup`) stays
    /// ignored: not counted, and the process lives on, with the watch and
    /// once it is gone.
    #[test]
    fn a_signal_ignored_from_the_start_stays_ignored() {
        if let Some(status) = in_child("a_signal_ignored_from_the_start_stays_ignored", true) {
            assert!(
                status.success(),
                "the child lived and its claims held: {status:?}"
            );
            return;
        }
        let watch = TerminationWatch::install().expect("the watch");
        send("-HUP");
        send("-TERM");
        wait_for(&watch, 1);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(watch.signals_received(), 1, "only SIGTERM was counted");
        assert_eq!(watch.signalled(), Some(TerminationSignalKind::Terminate));
        drop(watch);
        send("-HUP");
        std::thread::sleep(Duration::from_millis(300));
    }

    /// The three phases: a signal before the loop is reported to the loop
    /// as it attaches, not handed to it; one while it is attached is handed
    /// to it; one after it has detached is only remembered.
    #[test]
    fn a_signal_reaches_the_loop_only_while_it_is_attached() {
        if let Some(status) = in_child("a_signal_reaches_the_loop_only_while_it_is_attached", false)
        {
            assert!(
                status.success(),
                "the child lived and its claims held: {status:?}"
            );
            return;
        }
        let watch = TerminationWatch::install().expect("the watch");
        send("-TERM");
        wait_for(&watch, 1);
        let (sender, posted) = mpsc::channel();
        let sender = std::sync::Mutex::new(sender);
        assert!(
            watch.attach_loop(move |signal| {
                let _ = sender.lock().unwrap().send(signal);
            }),
            "the loop hears that a signal came before it"
        );
        assert!(posted.try_recv().is_err(), "and is not handed that one");
        send("-HUP");
        assert_eq!(
            posted.recv_timeout(Duration::from_secs(10)),
            Ok(TerminationSignal { signal: SIGHUP })
        );
        watch.detach_loop();
        send("-TERM");
        wait_for(&watch, 3);
        assert!(
            posted.try_recv().is_err(),
            "nothing reaches a loop that ended"
        );
        assert_eq!(watch.signalled(), Some(TerminationSignalKind::Terminate));
    }
}
