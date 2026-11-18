// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One running instance per user: a second launch hands its command line to
//! the first and exits.
//!
//! [`acquire`] holds an election on a name private to the user. The first
//! process to hold it is the [`Primary`](Election::Primary) and keeps it for
//! as long as it holds the returned [`PrimaryInstance`]; every later one is a
//! [`Secondary`](Election::Secondary), connected to the primary, and
//! [`forward`](SecondaryInstance::forward)s its arguments, working directory
//! and activation token, then exits. The primary receives each as an
//! [`InstanceMessage`] on a thread of its own once it
//! [`listen`](PrimaryInstance::listen)s.
//!
//! GUI-free: no window, no event loop. `teksilo-app`'s `single_instance`
//! module puts the election in the application's runtime directory (its data
//! directory on Windows) and delivers the messages on the main thread.
//!
//! ## Where it works
//!
//! On Unix (Linux, macOS, the BSDs) the name is a Unix-domain socket beside
//! a lock file, in the directory given to [`acquire`]. Their names are made
//! from the key (16 hexadecimal digits of its 64-bit FNV-1a hash, so a
//! 64-character key fits the length a socket path is held to under the long
//! macOS per-user temporary directory); they are an implementation detail,
//! not something to look for. Two keys with the same hash, a chance of
//! 2^-64, would share an instance.
//!
//! On Windows the name is a named pipe, made from the user's security
//! identifier and the same kind of hash, taken of the directory's path and
//! the key together. Pipe names belong to the whole machine, not to a
//! directory, so the directory is not created, opened or checked: its path
//! only keeps apart two installations (or two data directories) of one
//! application, as the directory does on Unix. Two paths that name one
//! directory differently (`C:\Data` and `c:\data`) are two instances.
//!
//! On any other platform [`acquire`] returns [`io::ErrorKind::Unsupported`].
//!
//! ## Security
//!
//! Same user only. On Unix the directory must be the user's own and closed
//! to others (`0700`), or [`acquire`] refuses it, and the socket is `0600`.
//! On Windows the pipe's access list grants the user alone, and it refuses
//! clients from another machine. On both, each side checks the user of the
//! process at the other end (`SO_PEERCRED` on Linux, `getpeereid` on macOS
//! and the BSDs, the process token's user on Windows, where another user
//! may have taken the pipe's name first) and drops a peer of another user.
//! That does not make a message trustworthy: any process of the same user
//! can connect and send anything, so an [`InstanceMessage`] is untrusted
//! input, to validate like a command line. Nothing here runs or opens
//! anything a message names.
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
//! when it is a socket of this user, before binding its own. On Windows
//! nothing is left behind: the pipe goes with the last handle to it, which
//! the system closes when the process ends.

use std::ffi::OsString;
use std::io;
use std::path::Path;
#[cfg(any(unix, windows))]
use std::time::Duration;

// Elsewhere only `InstanceMessage` is used; the rest serves `unix`,
// `windows` and tests.
#[cfg_attr(not(any(unix, windows)), allow(dead_code))]
mod protocol;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// The longest key, in bytes.
#[cfg(any(unix, windows))]
const MAX_KEY: usize = 64;

/// How long a starter waits for an instance that holds the name to answer.
#[cfg(any(unix, windows))]
const ELECTION_PATIENCE: Duration = Duration::from_secs(5);

/// How long a secondary waits for the primary to take its message.
#[cfg(any(unix, windows))]
const FORWARD_DEADLINE: Duration = Duration::from_secs(10);

/// How long the primary gives one connection, in all.
#[cfg(any(unix, windows))]
const PEER_DEADLINE: Duration = Duration::from_secs(5);

/// The pause between two attempts, and after an `accept` that failed for a
/// reason that may last (out of file descriptors).
#[cfg(any(unix, windows))]
const RETRY: Duration = Duration::from_millis(20);

#[cfg(any(unix, windows))]
fn check_key(key: &str) -> io::Result<()> {
    let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
    if key.is_empty() || key.len() > MAX_KEY || !key.bytes().all(allowed) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "single-instance key {key:?} is not 1 to {MAX_KEY} characters of A-Z, a-z, 0-9, '.', '_' and '-'"
            ),
        ));
    }
    Ok(())
}

/// 64-bit FNV-1a. Written here because the name must not change between two
/// builds of an application, and `std`'s hashers are not promised to be
/// stable across Rust releases or processes.
#[cfg(any(unix, windows))]
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

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
/// `key` names the instance: one to 64 characters from `A`-`Z`, `a`-`z`,
/// `0`-`9`, `.`, `_` and `-`, anything else being refused with
/// [`io::ErrorKind::InvalidInput`]. Two processes with the same directory
/// and key are instances of one application.
///
/// On Unix, `endpoint_dir` must exist, be a directory (not a symbolic link)
/// of the current user, and be closed to everyone else (no group or other
/// permission bits), as `AppPaths::create_runtime_dir` in `teksilo-settings`
/// leaves it; another directory is refused with
/// [`io::ErrorKind::PermissionDenied`]. On Windows `endpoint_dir` is only
/// part of the pipe's name: it need not exist, and nothing is made in it.
///
/// A process that finds an instance starting (it holds the lock and has not yet
/// bound its socket), or one with no room for another connection (it has
/// stopped accepting them), waits for it, for at most 5 seconds in all, and
/// then fails with [`io::ErrorKind::TimedOut`]. On Windows the running
/// instance holds one connection at a time, from the moment it is elected,
/// so a launch that finds it busy (serving another launch, or holding one
/// that waits for it to listen) waits the same way.
///
/// On Unix a file at the socket's place that is not a socket of this user is
/// never removed: the election fails with [`io::ErrorKind::AlreadyExists`].
/// The socket's path must fit the system's limit (about 100 bytes): a
/// directory with a longer path is refused with
/// [`io::ErrorKind::InvalidInput`]. On Windows a pipe of that name made by
/// another user fails the election with [`io::ErrorKind::PermissionDenied`].
///
/// [`io::ErrorKind::Unsupported`] on any platform that is neither Unix nor
/// Windows; see the module documentation.
pub fn acquire(endpoint_dir: &Path, key: &str) -> io::Result<Election> {
    #[cfg(unix)]
    {
        Ok(match unix::acquire(endpoint_dir, key)? {
            unix::Outcome::Primary(inner) => Election::Primary(PrimaryInstance { inner }),
            unix::Outcome::Secondary(inner) => Election::Secondary(SecondaryInstance { inner }),
        })
    }
    #[cfg(windows)]
    {
        Ok(match self::windows::acquire(endpoint_dir, key)? {
            self::windows::Outcome::Primary(inner) => Election::Primary(PrimaryInstance { inner }),
            self::windows::Outcome::Secondary(inner) => {
                Election::Secondary(SecondaryInstance { inner })
            }
        })
    }
    #[cfg(not(any(unix, windows)))]
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
///
/// On Windows the name is the pipe, which lives as long as the serving
/// thread holds it, so dropping waits for that thread to end the connection
/// it is serving, if any: at most 5 seconds, and longer only when it waits
/// on an `on_message` that has not returned. Past 5 seconds the drop
/// returns, and the name is freed once `on_message` returns.
#[derive(Debug)]
pub struct PrimaryInstance {
    #[cfg(unix)]
    inner: unix::Primary,
    #[cfg(windows)]
    inner: self::windows::Primary,
    #[cfg(not(any(unix, windows)))]
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
    /// cannot be started: the instance then keeps its socket (its pipe on
    /// Windows) and the name,
    /// and `listen` may be called again.
    pub fn listen(
        &mut self,
        on_message: impl Fn(InstanceMessage) + Send + 'static,
    ) -> io::Result<()> {
        #[cfg(any(unix, windows))]
        {
            self.inner.listen(Box::new(on_message))
        }
        #[cfg(not(any(unix, windows)))]
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
    #[cfg(windows)]
    inner: self::windows::Secondary,
    #[cfg(not(any(unix, windows)))]
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
        #[cfg(any(unix, windows))]
        {
            let args: Vec<OsString> = args.into_iter().collect();
            self.inner.forward(&args, cwd, activation_token)
        }
        #[cfg(not(any(unix, windows)))]
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
