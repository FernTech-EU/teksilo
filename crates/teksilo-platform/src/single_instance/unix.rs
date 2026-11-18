// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The election on Unix: a lock file decides, a socket carries the messages.
//!
//! The lock (`flock` through [`File::try_lock`]) is what makes a process the
//! primary, and it is held for as long as the primary lives: the kernel
//! releases it when the process ends, however it ends. That is what closes
//! the race between two starters that both find a stale socket, one
//! removing it after the other has bound its own. Only the lock holder
//! touches the socket's path, so a socket found there by the lock holder is
//! stale by construction. A starter that cannot take the lock is a
//! secondary, and connects.
//!
//! The socket is bound and then made `0600`. In between it carries the
//! umask's mode, which is safe only because the directory around it is the
//! user's and closed to everyone else: [`check_dir`] refuses any other, as
//! the automation bridge relies on its `0700` per-process directory for the
//! same moment.
//!
//! `$XDG_RUNTIME_DIR` may be cleaned of files not used for 6 hours, and the
//! macOS per-user temporary directory of files not used for days, so the
//! serving thread refreshes the times of the socket and the lock file every
//! hour.

use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::protocol::{self, DecodeError, InstanceMessage, Reply};

/// The longest key, in bytes.
const MAX_KEY: usize = 64;

/// Conservative ceiling for a socket path, below the 104-byte `sun_path` of
/// macOS with room for the closing NUL (the automation bridge's limit).
const MAX_SOCKET_PATH: usize = 100;

/// How long a starter waits for an instance that holds the lock to answer
/// on its socket.
const ELECTION_PATIENCE: Duration = Duration::from_secs(5);

/// How long a secondary waits for the primary to take its message.
const FORWARD_DEADLINE: Duration = Duration::from_secs(10);

/// How long the primary gives one connection, in all.
const PEER_DEADLINE: Duration = Duration::from_secs(5);

/// The pause between two attempts, and after an `accept` that failed for a
/// reason that may last (out of file descriptors).
const RETRY: Duration = Duration::from_millis(20);

/// How often the serving thread refreshes the times of its files.
const TOUCH_INTERVAL: Duration = Duration::from_secs(3600);

pub(super) enum Outcome {
    Primary(Primary),
    Secondary(Secondary),
}

pub(super) fn acquire(dir: &Path, key: &str) -> io::Result<Outcome> {
    acquire_within(dir, key, ELECTION_PATIENCE)
}

fn acquire_within(dir: &Path, key: &str, patience: Duration) -> io::Result<Outcome> {
    check_key(key)?;
    check_dir(dir)?;
    let (socket, lock_path) = endpoint_paths(dir, key);
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the single-instance socket path {} is {} bytes, over the \
                 {MAX_SOCKET_PATH}-byte limit of a socket path: use a directory \
                 with a shorter path",
                socket.display(),
                socket.as_os_str().len()
            ),
        ));
    }
    let deadline = Instant::now() + patience;
    loop {
        if let Some(lock) = try_lock(&lock_path)? {
            return Primary::bind(socket, lock_path, lock).map(Outcome::Primary);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        match connect_within(&socket, left) {
            Ok(stream) => {
                check_peer(&stream)?;
                return Ok(Outcome::Secondary(Secondary {
                    stream,
                    deadline: FORWARD_DEADLINE,
                }));
            }
            // The lock holder has not bound its socket yet, or is leaving, or
            // its backlog is full: it is not accepting, and `connect_within`
            // gave up at the deadline.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "an instance holds {} but does not answer on {}",
                    lock_path.display(),
                    socket.display()
                ),
            ));
        }
        std::thread::sleep(RETRY);
    }
}

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

/// The socket and the lock file of `key` in `dir`: `<16 hex digits>.sock`
/// and `.lock`, named from [`fnv1a_64`] of the key rather than from the key.
///
/// A socket path is held to about 100 bytes, and the macOS per-user
/// temporary directory takes half of that, which would leave less than a
/// 64-character key. Two keys with one hash (a chance of 2^-64) share an
/// instance.
fn endpoint_paths(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    let stem = format!("{:016x}", fnv1a_64(key.as_bytes()));
    (
        dir.join(format!("{stem}.sock")),
        dir.join(format!("{stem}.lock")),
    )
}

/// 64-bit FNV-1a. Written here because the name must not change between two
/// builds of an application, and `std`'s hashers are not promised to be
/// stable across Rust releases or processes.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// Connect to `socket`, waiting at most `timeout` for room in its backlog.
///
/// On Linux a blocking connect to a Unix-domain socket whose backlog is full
/// waits for room, without end in `UnixStream::connect`, and a primary that
/// holds the name without accepting keeps its backlog full. Linux bounds that
/// wait by the socket's send timeout and fails with `EAGAIN` when it expires,
/// so the timeout is set before connecting. A non-blocking connect and `poll`
/// would not do: Linux answers a Unix-domain connect with `EAGAIN` at once,
/// never `EINPROGRESS`, and nothing wakes a `poll` when room frees. macOS and
/// the BSDs never wait: they refuse a connection to a full backlog with
/// `ECONNREFUSED`, which the caller retries until its deadline.
fn connect_within(socket: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let path = socket.as_os_str().as_bytes();
    // SAFETY: all-zero bytes are a valid `sockaddr_un`.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if path.len() >= addr.sun_path.len() || path.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} cannot name a socket", socket.display()),
        ));
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (to, &from) in addr.sun_path.iter_mut().zip(path) {
        *to = from as libc::c_char;
    }
    // The path and its NUL. Where the address has a `sun_len`, the kernel
    // sets it from this length.
    let len =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1) as libc::socklen_t;

    // SAFETY: `socket` takes plain integers and returns a new descriptor or -1.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just created and nothing else owns it.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // As `std` does for its own sockets: a program the application starts
    // must not hold the connection open.
    // SAFETY: `fcntl` on a descriptor `stream` owns, for the whole call.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // A zero timeout would mean "wait forever" to the kernel; `std` refuses it.
    stream.set_write_timeout(Some(timeout.max(Duration::from_millis(1))))?;
    // SAFETY: `addr` is a `sockaddr_un` of at least `len` bytes that outlives
    // the call, on a descriptor `stream` owns.
    let rc = unsafe { libc::connect(fd, (&raw const addr).cast(), len) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stream)
}

/// Refuse a directory that is not the user's own and closed to others.
fn check_dir(dir: &Path) -> io::Result<()> {
    // `symlink_metadata`: a link to a directory of the user's is not one, and
    // what it points at could change.
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != euid() || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is not a directory of this user closed to everyone else",
                dir.display()
            ),
        ));
    }
    Ok(())
}

/// Open the lock file and take its lock: `None` when another process holds
/// it, or when the name stopped leading to the file opened.
fn try_lock(path: &Path) -> io::Result<Option<File>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.uid() != euid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not a file of this user", path.display()),
        ));
    }
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Ok(None),
        Err(fs::TryLockError::Error(e)) => return Err(e),
    }
    // A leaving primary removes the file while it still holds the lock. A
    // starter that opened it before that holds, once the lock is released, a
    // lock on a file no one else will open: the name is free, and the next
    // attempt takes it through a file of its own.
    match fs::symlink_metadata(path) {
        Ok(now) if now.dev() == opened.dev() && now.ino() == opened.ino() => Ok(Some(file)),
        _ => Ok(None),
    }
}

fn euid() -> u32 {
    // SAFETY: `geteuid` takes nothing and cannot fail.
    unsafe { libc::geteuid() }
}

/// Drop a peer that runs as another user.
fn check_peer(stream: &UnixStream) -> io::Result<()> {
    let peer = peer_uid(stream)?;
    if peer != euid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the peer runs as uid {peer}, not as this user"),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for writes of the sizes passed, for
    // the duration of the call, on a socket this function borrows.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::other("SO_PEERCRED answered a short credential"));
    }
    Ok(cred.uid)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: `uid` and `gid` are valid for writes for the duration of the
    // call, on a socket this function borrows.
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no way to read a socket peer's user on this platform",
    ))
}

/// Set both times of `path` to now, without following a symbolic link.
fn touch(path: &Path) {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    // SAFETY: a NUL-terminated path that outlives the call; null times mean
    // "now" for both.
    unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path.as_ptr(),
            std::ptr::null(),
            libc::AT_SYMLINK_NOFOLLOW,
        );
    }
}

/// A socket whose reads and writes share one deadline.
///
/// A socket timeout bounds each `read` alone, so a peer sending one byte
/// just before each expires would hold the connection for as long as it
/// likes. Each call here is given only what is left of the deadline.
struct Timed<'a> {
    stream: &'a UnixStream,
    deadline: Instant,
}

impl Timed<'_> {
    fn left(&self) -> io::Result<Duration> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(timed_out());
        }
        Ok(left)
    }
}

impl Read for Timed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        unless_shut(self.stream.set_read_timeout(Some(self.left()?)))?;
        let mut stream = self.stream;
        stream.read(buf).map_err(expired_as_timed_out)
    }
}

impl Write for Timed<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unless_shut(self.stream.set_write_timeout(Some(self.left()?)))?;
        let mut stream = self.stream;
        stream.write(buf).map_err(expired_as_timed_out)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The result of setting a socket timeout, with the refusal of a shut socket
/// taken as no error.
///
/// XNU refuses to set any option on a socket whose connection is shut in
/// both directions, as it is as soon as the peer closes, with `EINVAL`. A
/// primary that answers and closes before the secondary reads its reply
/// leaves the secondary such a socket. It never blocks: a read returns what
/// is still buffered, the reply, then the end of the stream, and a write
/// fails at once, so the call goes ahead without the timeout. The timeout is
/// always a valid one here, so `EINVAL` has no other cause.
fn unless_shut(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(e) if cfg!(target_vendor = "apple") && e.raw_os_error() == Some(libc::EINVAL) => Ok(()),
        result => result,
    }
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "the connection's deadline expired")
}

/// An expired socket timeout reads as `WouldBlock` on Unix; the socket is
/// never non-blocking here, so that is the only way it arises.
fn expired_as_timed_out(e: io::Error) -> io::Error {
    if e.kind() == io::ErrorKind::WouldBlock {
        timed_out()
    } else {
        e
    }
}

#[derive(Debug)]
pub(super) struct Primary {
    socket: PathBuf,
    lock_path: PathBuf,
    lock: Option<File>,
    /// Until [`listen`](Self::listen) hands it to the serving thread.
    listener: Option<UnixListener>,
    /// The serving thread's wake-up: closing this end ends it.
    stop: Option<UnixStream>,
    peer_deadline: Duration,
    /// The serving thread's stack size: an impossible one makes its spawn
    /// fail.
    #[cfg(test)]
    stack_size: Option<usize>,
}

impl Primary {
    fn bind(socket: PathBuf, lock_path: PathBuf, lock: File) -> io::Result<Self> {
        match fs::symlink_metadata(&socket) {
            Ok(found) if found.file_type().is_socket() && found.uid() == euid() => {
                fs::remove_file(&socket)?;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "{} is not a socket of this user; it is left as it is",
                        socket.display()
                    ),
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(&socket)?;
        let primary = Self {
            socket,
            lock_path,
            lock: Some(lock),
            listener: Some(listener),
            stop: None,
            peer_deadline: PEER_DEADLINE,
            #[cfg(test)]
            stack_size: None,
        };
        fs::set_permissions(&primary.socket, fs::Permissions::from_mode(0o600))?;
        Ok(primary)
    }

    pub(super) fn listen(
        &mut self,
        on_message: Box<dyn Fn(InstanceMessage) + Send>,
    ) -> io::Result<()> {
        let Some(listener) = self.listener.as_ref() else {
            return Err(io::Error::other("this instance is already listening"));
        };
        // `poll` says when a connection is waiting; a client that gives up
        // before the `accept` must not leave the thread blocked in it.
        listener.set_nonblocking(true)?;
        let (stop, stopped) = UnixStream::pair()?;
        // The thread is started before it is given the listener, so a thread
        // that cannot start leaves this instance its listener, and the name
        // it holds can still be served by a later call.
        let (hand_over, handed) = std::sync::mpsc::sync_channel::<Serving>(1);
        let builder = std::thread::Builder::new().name("teksilo-single-instance".into());
        #[cfg(test)]
        let builder = match self.stack_size {
            Some(size) => builder.stack_size(size),
            None => builder,
        };
        builder.spawn(move || {
            if let Ok(serving) = handed.recv() {
                serving.run();
            }
        })?;
        let serving = Serving {
            listener: self.listener.take().expect("checked above"),
            stopped,
            socket: self.socket.clone(),
            lock_path: self.lock_path.clone(),
            peer_deadline: self.peer_deadline,
            on_message,
        };
        // The thread holds the receiving end until it has received.
        let _ = hand_over.send(serving);
        self.stop = Some(stop);
        Ok(())
    }
}

impl Drop for Primary {
    fn drop(&mut self) {
        self.stop = None;
        let _ = fs::remove_file(&self.socket);
        // Removed while still locked: see `try_lock`.
        let _ = fs::remove_file(&self.lock_path);
        self.lock = None;
    }
}

/// What the serving thread owns.
struct Serving {
    listener: UnixListener,
    stopped: UnixStream,
    socket: PathBuf,
    lock_path: PathBuf,
    peer_deadline: Duration,
    on_message: Box<dyn Fn(InstanceMessage) + Send>,
}

impl Serving {
    fn run(self) {
        let mut touched = Instant::now();
        loop {
            let mut fds = [
                libc::pollfd {
                    fd: self.listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: self.stopped.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let wait = TOUCH_INTERVAL.saturating_sub(touched.elapsed());
            let wait = wait.as_millis().min(libc::c_int::MAX as u128) as libc::c_int;
            // SAFETY: `fds` holds two valid `pollfd`s for the whole call,
            // whose descriptors this thread owns.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, wait) };
            if ready < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            // Readable or hung up: the `PrimaryInstance` closed its end.
            if fds[1].revents != 0 {
                return;
            }
            if touched.elapsed() >= TOUCH_INTERVAL {
                touch(&self.socket);
                touch(&self.lock_path);
                touched = Instant::now();
            }
            if fds[0].revents == 0 {
                continue;
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let _ = self.answer(&stream);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                            | io::ErrorKind::ConnectionAborted
                    ) => {}
                Err(_) => std::thread::sleep(RETRY),
            }
        }
    }

    /// Serve one connection: read its request within the deadline, hand it
    /// over, answer. A peer of another user, or one that sends anything
    /// malformed, is closed without an answer.
    fn answer(&self, stream: &UnixStream) -> io::Result<()> {
        // macOS and the BSDs give an accepted socket the listener's
        // non-blocking mode; Linux does not.
        stream.set_nonblocking(false)?;
        check_peer(stream)?;
        let mut io = Timed {
            stream,
            deadline: Instant::now() + self.peer_deadline,
        };
        let request = protocol::read_frame(&mut io, protocol::MAX_REQUEST)?;
        let reply = match protocol::decode_request(&request) {
            // A launch that gave up waiting has reported a failure, and its
            // request, still in the backlog, must not be acted on as well.
            Ok(_) if peer_hung_up(stream) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "the second launch stopped waiting",
                ));
            }
            Ok(message) => {
                (self.on_message)(message);
                Reply::Accepted
            }
            Err(DecodeError::Version(_)) => Reply::Rejected {
                version: protocol::VERSION,
            },
            Err(DecodeError::Malformed(why)) => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, why));
            }
        };
        protocol::write_frame(&mut io, &protocol::encode_reply(reply))
    }
}

/// Whether the peer has closed its end. Its request, already read, is all it
/// sent, so a peek that finds the end of the stream means it is gone; one
/// that finds nothing yet means it is still waiting for the answer.
fn peer_hung_up(stream: &UnixStream) -> bool {
    let mut byte = 0u8;
    // SAFETY: `byte` is valid for a write of one byte for the duration of the
    // call, on a socket this function borrows. `MSG_PEEK` leaves the stream as
    // it is and `MSG_DONTWAIT` returns at once.
    let n = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            (&raw mut byte).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    match n {
        0 => true,
        n if n > 0 => false,
        _ => !matches!(
            io::Error::last_os_error().kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
        ),
    }
}

#[derive(Debug)]
pub(super) struct Secondary {
    stream: UnixStream,
    deadline: Duration,
}

impl Secondary {
    pub(super) fn forward(
        self,
        args: &[std::ffi::OsString],
        cwd: &Path,
        activation_token: Option<&str>,
    ) -> io::Result<()> {
        let request = protocol::encode_request(args, cwd, activation_token)?;
        let mut io = Timed {
            stream: &self.stream,
            deadline: Instant::now() + self.deadline,
        };
        protocol::write_frame(&mut io, &request)?;
        let reply = protocol::read_frame(&mut io, protocol::MAX_REPLY)?;
        match protocol::decode_reply(&reply)? {
            Reply::Accepted => Ok(()),
            Reply::Rejected { version } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the running instance speaks version {version} of the single-instance \
                     protocol, this one version {}",
                    protocol::VERSION
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::mpsc;

    /// A directory closed to others, as `check_dir` asks.
    fn private_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn primary(outcome: Outcome) -> Primary {
        match outcome {
            Outcome::Primary(p) => p,
            Outcome::Secondary(_) => panic!("expected to be the primary"),
        }
    }

    fn secondary(outcome: Outcome) -> Secondary {
        match outcome {
            Outcome::Secondary(s) => s,
            Outcome::Primary(_) => panic!("expected to be a secondary"),
        }
    }

    /// A primary that sends what it receives down a channel.
    fn listening(
        dir: &Path,
        key: &str,
        peer_deadline: Duration,
    ) -> (Primary, mpsc::Receiver<InstanceMessage>) {
        let mut p = primary(acquire(dir, key).unwrap());
        p.peer_deadline = peer_deadline;
        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        (p, rx)
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn app_sock(dir: &Path) -> PathBuf {
        endpoint_paths(dir, "app").0
    }

    fn app_lock(dir: &Path) -> PathBuf {
        endpoint_paths(dir, "app").1
    }

    /// The published FNV-1a test vectors: the names must not change between
    /// two builds of an application.
    #[test]
    fn the_file_names_are_a_stable_hash_of_the_key() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
        let (sock, lock) = endpoint_paths(Path::new("/d"), "a");
        assert_eq!(sock, Path::new("/d/af63dc4c8601ec8c.sock"));
        assert_eq!(lock, Path::new("/d/af63dc4c8601ec8c.lock"));
    }

    /// The longest key fits under a directory as long as the macOS per-user
    /// temporary directory with an application's bundle id in it.
    #[test]
    fn the_longest_key_fits_in_a_long_directory() {
        let root = private_dir();
        let base = root.path().as_os_str().len();
        assert!(base < 69, "the temporary directory {base} bytes long");
        let dir = root.path().join("d".repeat(70 - base - 1));
        assert_eq!(dir.as_os_str().len(), 70);
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();

        let (a, b) = ("a".repeat(MAX_KEY), "b".repeat(MAX_KEY));
        let _first = primary(acquire(&dir, &a).unwrap());
        let _other = primary(acquire(&dir, &b).unwrap());
        let _second = secondary(acquire(&dir, &a).unwrap());
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        names.sort();
        let mut expected = vec![
            endpoint_paths(&dir, &a).0,
            endpoint_paths(&dir, &a).1,
            endpoint_paths(&dir, &b).0,
            endpoint_paths(&dir, &b).1,
        ];
        expected.sort();
        assert_eq!(names, expected, "one socket and one lock file per key");
    }

    #[test]
    fn a_path_too_long_for_a_socket_is_refused_with_its_length() {
        let root = private_dir();
        let dir = root.path().join("d".repeat(MAX_SOCKET_PATH));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let err = acquire(&dir, "app").err().expect("refused");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("byte limit"), "{err}");
    }

    /// A primary whose backlog is full and that never accepts does not hold
    /// a starter past its patience. Linux would block the starter's connect
    /// without end; macOS refuses it.
    #[test]
    fn a_full_backlog_does_not_outlast_the_patience() {
        let dir = private_dir();
        let held = File::create(app_lock(dir.path())).unwrap();
        held.try_lock().unwrap();
        let listener = UnixListener::bind(app_sock(dir.path())).unwrap();
        // SAFETY: `listen` on a listening socket this test owns; it only
        // lowers the backlog. Not to 0, which XNU reads as its largest.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        // Linux queues one connection more than the backlog, XNU as many, so
        // the backlog is filled until a connection finds no room.
        let mut queued = Vec::new();
        let full = loop {
            match connect_within(&app_sock(dir.path()), Duration::from_millis(50)) {
                Ok(stream) if queued.len() < 16 => queued.push(stream),
                Ok(_) => panic!("{} connections queued on a backlog of 1", queued.len()),
                Err(e) => break e,
            }
        };
        assert!(
            matches!(
                full.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionRefused
            ),
            "{full:?}"
        );

        let started = Instant::now();
        let err = acquire_within(dir.path(), "app", Duration::from_millis(300))
            .err()
            .expect("no room");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    /// The reviewer's scenario: a launch that timed out before the primary
    /// listened has reported a failure, so its request is not delivered.
    #[test]
    fn a_launch_that_gave_up_is_not_delivered() {
        let dir = private_dir();
        let mut p = primary(acquire(dir.path(), "app").unwrap());
        let mut s = secondary(acquire(dir.path(), "app").unwrap());
        s.deadline = Duration::from_millis(200);
        let err = s
            .forward(&args(&["gave-up"]), Path::new("/"), None)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);

        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        // Served after the one that gave up, which is first in the backlog.
        secondary(acquire(dir.path(), "app").unwrap())
            .forward(&args(&["waited"]), Path::new("/"), None)
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["waited"])
        );
        assert!(rx.try_recv().is_err());
    }

    /// A serving thread that cannot start leaves the listener, and a later
    /// `listen` serves. glibc refuses the impossible stack at
    /// `pthread_create`, which `std` reports as an error.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_listen_that_could_not_start_can_be_retried() {
        let dir = private_dir();
        let mut p = primary(acquire(dir.path(), "app").unwrap());
        p.stack_size = Some(1 << 60);
        assert!(p.listen(Box::new(|_| {})).is_err());
        p.stack_size = None;
        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        secondary(acquire(dir.path(), "app").unwrap())
            .forward(&args(&["retried"]), Path::new("/"), None)
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["retried"])
        );
    }

    #[test]
    fn the_second_election_in_one_process_is_a_secondary() {
        let dir = private_dir();
        let first = primary(acquire(dir.path(), "app").unwrap());
        let _second = secondary(acquire(dir.path(), "app").unwrap());
        // Another key is another instance.
        let _other = primary(acquire(dir.path(), "other").unwrap());
        drop(first);
    }

    #[test]
    fn the_socket_is_owner_only() {
        let dir = private_dir();
        let _p = primary(acquire(dir.path(), "app").unwrap());
        let mode = fs::metadata(app_sock(dir.path())).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let mode = fs::metadata(app_lock(dir.path())).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_message_round_trips_with_and_without_a_token() {
        let dir = private_dir();
        let (_p, rx) = listening(dir.path(), "app", PEER_DEADLINE);
        for token in [None, Some("token-1")] {
            secondary(acquire(dir.path(), "app").unwrap())
                .forward(&args(&["app", "file.txt"]), Path::new("/somewhere"), token)
                .unwrap();
            let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(got.args, args(&["app", "file.txt"]));
            assert_eq!(got.cwd, Path::new("/somewhere"));
            assert_eq!(got.activation_token.as_deref(), token);
        }
    }

    #[test]
    fn a_connection_made_before_listen_is_served_once_it_starts() {
        let dir = private_dir();
        let mut p = primary(acquire(dir.path(), "app").unwrap());
        let s = secondary(acquire(dir.path(), "app").unwrap());
        let sent = std::thread::spawn(move || s.forward(&args(&["early"]), Path::new("/"), None));
        std::thread::sleep(Duration::from_millis(100));
        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        sent.join().unwrap().unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["early"])
        );
    }

    #[test]
    fn a_secondary_gives_up_on_a_primary_that_never_listens() {
        let dir = private_dir();
        let _p = primary(acquire(dir.path(), "app").unwrap());
        let mut s = secondary(acquire(dir.path(), "app").unwrap());
        s.deadline = Duration::from_millis(200);
        let started = Instant::now();
        let err = s.forward(&args(&["x"]), Path::new("/"), None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn listening_twice_is_refused() {
        let dir = private_dir();
        let (mut p, _rx) = listening(dir.path(), "app", PEER_DEADLINE);
        assert!(p.listen(Box::new(|_| {})).is_err());
    }

    #[test]
    fn a_stale_socket_of_this_user_is_removed() {
        let dir = private_dir();
        let path = app_sock(dir.path());
        // A listener dropped without removing its file: what a process that
        // ended without unwinding leaves.
        drop(UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let (_p, rx) = listening(dir.path(), "app", PEER_DEADLINE);
        secondary(acquire(dir.path(), "app").unwrap())
            .forward(&args(&["after"]), Path::new("/"), None)
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["after"])
        );
    }

    #[test]
    fn a_file_or_a_link_at_the_sockets_place_is_left_alone() {
        let dir = private_dir();
        let path = app_sock(dir.path());
        fs::write(&path, b"not a socket").unwrap();
        let err = acquire(dir.path(), "app").err().expect("refused");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"not a socket");

        fs::remove_file(&path).unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"kept").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let err = acquire(dir.path(), "app").err().expect("refused");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), b"kept");

        // The election that failed released the lock: a clear path is won.
        fs::remove_file(&path).unwrap();
        let _p = primary(acquire(dir.path(), "app").unwrap());
    }

    #[test]
    fn a_link_at_the_locks_place_is_refused() {
        let dir = private_dir();
        let target = dir.path().join("target");
        fs::write(&target, b"kept").unwrap();
        std::os::unix::fs::symlink(&target, app_lock(dir.path())).unwrap();
        assert!(acquire(dir.path(), "app").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"kept");
    }

    #[test]
    fn a_directory_open_to_others_is_refused() {
        let dir = private_dir();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let err = acquire(dir.path(), "app").err().expect("refused");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);

        let real = private_dir();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        let err = acquire(&link, "app").err().expect("refused");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn keys_outside_the_alphabet_or_too_long_are_refused() {
        let dir = private_dir();
        for key in ["", "a/b", "a b", "é", "../x", &"k".repeat(MAX_KEY + 1)] {
            let err = acquire(dir.path(), key).err().expect("refused");
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{key:?}");
        }
        let _p = primary(acquire(dir.path(), "eu.ferntech.App-2_x").unwrap());
    }

    #[test]
    fn a_lock_held_with_no_socket_times_out() {
        let dir = private_dir();
        let held = File::create(app_lock(dir.path())).unwrap();
        held.try_lock().unwrap();
        let started = Instant::now();
        let err = acquire_within(dir.path(), "app", Duration::from_millis(200))
            .err()
            .expect("no answer");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn dropping_the_primary_frees_the_name() {
        let dir = private_dir();
        let (p, _rx) = listening(dir.path(), "app", PEER_DEADLINE);
        drop(p);
        assert!(!app_sock(dir.path()).exists());
        assert!(!app_lock(dir.path()).exists());
        let unheard = primary(acquire(dir.path(), "app").unwrap());
        drop(unheard);
        let _p = primary(acquire(dir.path(), "app").unwrap());
    }

    /// Bad peers are each dropped within the deadline, and a good one is
    /// served after them.
    #[test]
    fn bad_peers_are_dropped_and_the_listener_keeps_serving() {
        let deadline = Duration::from_millis(300);
        let dir = private_dir();
        let (_p, rx) = listening(dir.path(), "app", deadline);
        let socket = app_sock(dir.path());

        // Closed without an answer, within `deadline` and well under the
        // 5 seconds the test allows.
        let closed_in_time = |mut peer: UnixStream| {
            peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let started = Instant::now();
            let mut buf = [0u8; 8];
            let n = peer.read(&mut buf).unwrap_or(0);
            assert_eq!(n, 0, "no answer");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{:?}",
                started.elapsed()
            );
        };

        // Says nothing.
        closed_in_time(UnixStream::connect(&socket).unwrap());

        // An oversized length.
        let mut peer = UnixStream::connect(&socket).unwrap();
        peer.write_all(&u32::MAX.to_le_bytes()).unwrap();
        closed_in_time(peer);

        // Garbage of the right size.
        let mut peer = UnixStream::connect(&socket).unwrap();
        protocol::write_frame(&mut peer, &[protocol::VERSION, 0xff, 0xff]).unwrap();
        closed_in_time(peer);

        // One byte every 100 ms: each read is quick, the whole is not.
        let peer = UnixStream::connect(&socket).unwrap();
        let mut writer = peer.try_clone().unwrap();
        let drip = std::thread::spawn(move || {
            let mut frame = Vec::new();
            protocol::write_frame(&mut frame, &[0u8; 1000]).unwrap();
            for byte in frame {
                if writer.write_all(&[byte]).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        closed_in_time(peer);
        drip.join().unwrap();

        // Another version is answered as such.
        let mut peer = UnixStream::connect(&socket).unwrap();
        protocol::write_frame(&mut peer, &[protocol::VERSION + 1]).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let reply = protocol::read_frame(&mut peer, protocol::MAX_REPLY).unwrap();
        assert_eq!(
            protocol::decode_reply(&reply).unwrap(),
            Reply::Rejected {
                version: protocol::VERSION
            }
        );

        assert!(rx.try_recv().is_err(), "nothing bad was handed over");
        secondary(acquire(dir.path(), "app").unwrap())
            .forward(&args(&["good"]), Path::new("/"), None)
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["good"])
        );
    }

    /// A reply the peer sent before closing is read, though XNU refuses the
    /// read timeout on the socket the close shut.
    #[test]
    fn what_a_closed_peer_sent_is_still_read() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        protocol::write_frame(&mut &theirs, b"reply").unwrap();
        drop(theirs);
        let mut io = Timed {
            stream: &ours,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        assert_eq!(protocol::read_frame(&mut io, 64).unwrap(), b"reply");
        assert_eq!(io.read(&mut [0u8; 1]).unwrap(), 0);
    }

    #[test]
    fn the_peer_check_accepts_this_user() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a).unwrap(), euid());
        check_peer(&a).unwrap();
    }
}
