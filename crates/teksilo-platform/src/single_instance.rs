// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One running instance per user: a second launch hands its command line to
//! the first and exits.
//!
//! [`acquire`] holds an election on a name in a directory private to the
//! user. The first process to hold it is the [`Primary`](Election::Primary)
//! and keeps it for as long as it holds the returned [`PrimaryInstance`];
//! every later one is a [`Secondary`](Election::Secondary), connected to the
//! primary, and [`forward`](SecondaryInstance::forward)s its arguments,
//! working directory and activation token, then exits. The primary receives
//! each as an [`InstanceMessage`] on a thread of its own once it
//! [`listen`](PrimaryInstance::listen)s.
//!
//! GUI-free: no window, no event loop. `teksilo-app`'s `single_instance`
//! module puts the election in the application's runtime directory and
//! delivers the messages on the main thread.
//!
//! ## Where it works
//!
//! On Unix (Linux, macOS, the BSDs) the name is a Unix-domain socket beside
//! a lock file, in the directory given to [`acquire`]. Their names are made
//! from the key (16 hexadecimal digits of its 64-bit FNV-1a hash, so a
//! 64-character key fits the length a socket path is held to under the long
//! macOS per-user temporary directory); they are an implementation detail,
//! not something to look for. Two keys with the same hash, a chance of
//! 2^-64, would share an instance. On Windows [`acquire`] returns
//! [`io::ErrorKind::Unsupported`]: a named pipe would serve, but the check
//! that the pipe found was made by the same user (a pipe name is global, so
//! another user may have taken it first) is not written yet.
//!
//! ## Security
//!
//! Same user only. The directory must be the user's own and closed to others
//! (`0700`), or [`acquire`] refuses it; the socket is `0600`; and each side
//! checks the user id of the process at the other end (`SO_PEERCRED` on
//! Linux, `getpeereid` on macOS and the BSDs) and drops a peer of another
//! user. That does not make a message trustworthy: any process of the same
//! user can connect and send anything, so an [`InstanceMessage`] is
//! untrusted input, to validate like a command line. Nothing here runs or
//! opens anything a message names.
//!
//! Every exchange is bounded: at most 65536 arguments, 2 MiB in all (Linux's
//! `ARG_MAX`) and an activation token of 1 KiB; the primary gives each
//! connection 5 seconds in all, so a peer that sends nothing, or one byte at a
//! time, is dropped and the next connection served.
//!
//! ## A process that ends without unwinding
//!
//! Dropping the [`PrimaryInstance`] removes the socket and the lock file.
//! A process that ends without unwinding (`std::process::exit`, an abort, a
//! crash, a kill) leaves them behind. The lock goes with the process, so the
//! next [`acquire`] wins the election, finds the socket stale and removes it,
//! when it is a socket of this user, before binding its own.

use std::ffi::OsString;
use std::io;
use std::path::Path;

// Off Unix only `InstanceMessage` is used; the rest serves `unix` and tests.
#[cfg_attr(not(unix), allow(dead_code))]
mod protocol;
#[cfg(unix)]
mod unix;

pub use protocol::InstanceMessage;

/// The outcome of [`acquire`].
#[derive(Debug)]
pub enum Election {
    /// No instance was running: this process is the one, for as long as it
    /// holds the [`PrimaryInstance`].
    Primary(PrimaryInstance),
    /// An instance is running, and this is a connection to it.
    Secondary(SecondaryInstance),
}

/// Hold the single-instance election for `key` in `endpoint_dir`.
///
/// `endpoint_dir` must exist, be a directory (not a symbolic link) of the
/// current user, and be closed to everyone else (no group or other
/// permission bits), as `AppPaths::create_runtime_dir` in `teksilo-settings`
/// leaves it; another directory is refused with
/// [`io::ErrorKind::PermissionDenied`]. `key` names the instance: one to
/// 64 characters from `A`-`Z`, `a`-`z`, `0`-`9`, `.`, `_` and `-`, anything
/// else being refused with [`io::ErrorKind::InvalidInput`]. Two processes
/// with the same directory and key are instances of one application.
///
/// A process that finds an instance starting (it holds the lock and has not yet
/// bound its socket), or one with no room for another connection (it has
/// stopped accepting them), waits for it, for at most 5 seconds in all, and
/// then fails with [`io::ErrorKind::TimedOut`]. A file at the socket's place
/// that is not a socket of this user is never removed: the election fails with
/// [`io::ErrorKind::AlreadyExists`].
///
/// The socket's path must fit the system's limit (about 100 bytes): a
/// directory with a longer path is refused with
/// [`io::ErrorKind::InvalidInput`].
///
/// [`io::ErrorKind::Unsupported`] on Windows and on any platform that is not
/// Unix; see the module documentation.
pub fn acquire(endpoint_dir: &Path, key: &str) -> io::Result<Election> {
    #[cfg(unix)]
    {
        Ok(match unix::acquire(endpoint_dir, key)? {
            unix::Outcome::Primary(inner) => Election::Primary(PrimaryInstance { inner }),
            unix::Outcome::Secondary(inner) => Election::Secondary(SecondaryInstance { inner }),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (endpoint_dir, key);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "single-instance election is not implemented on this platform",
        ))
    }
}

/// The running instance: holds the election until it is dropped.
///
/// Connections that arrive before [`listen`](Self::listen) wait, unanswered,
/// and are served once it is called: a second launch made while the first
/// is still starting is not lost, as long as the first starts listening
/// within the 10 seconds the second waits for its answer. One that stopped
/// waiting has reported a failure, and its message is dropped.
///
/// Dropping it stops serving, removes the socket and the lock file, and
/// frees the name for the next [`acquire`]. A message being served as it is
/// dropped is still handed to `on_message` (at most 5 seconds later).
#[derive(Debug)]
pub struct PrimaryInstance {
    #[cfg(unix)]
    inner: unix::Primary,
    #[cfg(not(unix))]
    inner: std::convert::Infallible,
}

impl PrimaryInstance {
    /// Serve second launches on a thread of this instance's own, one
    /// connection at a time, handing each message to `on_message` there.
    ///
    /// `on_message` runs on that thread, not on the main thread: post what
    /// it receives to wherever it is acted on (`teksilo-app` posts it to the
    /// event loop). A second launch learns that its message was handed over
    /// once `on_message` returns, so keep it short.
    ///
    /// Fails when called again after it succeeded, and when the thread
    /// cannot be started: the instance then keeps its socket and the name,
    /// and `listen` may be called again.
    pub fn listen(
        &mut self,
        on_message: impl Fn(InstanceMessage) + Send + 'static,
    ) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.inner.listen(Box::new(on_message))
        }
        #[cfg(not(unix))]
        {
            let _ = on_message;
            match self.inner {}
        }
    }
}

/// A connection to the running instance.
#[derive(Debug)]
pub struct SecondaryInstance {
    #[cfg(unix)]
    inner: unix::Secondary,
    #[cfg(not(unix))]
    inner: std::convert::Infallible,
}

impl SecondaryInstance {
    /// Hand `args`, `cwd` and `activation_token` to the running instance and
    /// wait for it to take them, for at most 10 seconds.
    ///
    /// `Ok` means the running instance took the message (`teksilo-app` posts
    /// it to its event queue), not that it acted on it: an instance that is
    /// shutting down as it takes it may end before handling it. A launch that
    /// stopped waiting is not delivered, unless it stopped in the moment
    /// between the running instance checking that it still waits and handing
    /// the message over.
    ///
    /// Fails with [`io::ErrorKind::InvalidInput`] before sending anything
    /// when the message is over a bound (see the module documentation), with
    /// [`io::ErrorKind::TimedOut`] when the running instance does not answer
    /// in time (it may not be listening yet), and with
    /// [`io::ErrorKind::InvalidData`] when it speaks another version of the
    /// protocol (an update installed while it was running).
    pub fn forward(
        self,
        args: impl IntoIterator<Item = OsString>,
        cwd: &Path,
        activation_token: Option<&str>,
    ) -> io::Result<()> {
        #[cfg(unix)]
        {
            let args: Vec<OsString> = args.into_iter().collect();
            self.inner.forward(&args, cwd, activation_token)
        }
        #[cfg(not(unix))]
        {
            let _ = (args, cwd, activation_token);
            match self.inner {}
        }
    }

    /// [`forward`](Self::forward) this process's command line: every
    /// argument of [`std::env::args_os`] (`argv[0]` included), the working
    /// directory (empty when it cannot be read), and the activation token
    /// its launcher gave it, taken out of the environment by
    /// [`activation_token_from_env`](crate::window_activation::activation_token_from_env).
    ///
    /// Call it from `main`, before any other thread is started: taking the
    /// token removes environment variables.
    pub fn forward_command_line(self) -> io::Result<()> {
        let token = crate::window_activation::activation_token_from_env();
        let cwd = std::env::current_dir().unwrap_or_default();
        self.forward(std::env::args_os(), &cwd, token.as_deref())
    }
}
