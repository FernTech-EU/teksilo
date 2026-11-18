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
use std::os::fd::AsRawFd;
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
    let socket = dir.join(format!("{key}.sock"));
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "socket path {} is {} bytes, over the {MAX_SOCKET_PATH}-byte limit",
                socket.display(),
                socket.as_os_str().len()
            ),
        ));
    }
    let lock_path = dir.join(format!("{key}.lock"));
    let deadline = Instant::now() + patience;
    loop {
        if let Some(lock) = try_lock(&lock_path)? {
            return Primary::bind(socket, lock_path, lock).map(Outcome::Primary);
        }
        match UnixStream::connect(&socket) {
            Ok(stream) => {
                check_peer(&stream)?;
                return Ok(Outcome::Secondary(Secondary {
                    stream,
                    deadline: FORWARD_DEADLINE,
                }));
            }
            // The lock holder has not bound its socket yet, or is leaving.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
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
        self.stream.set_read_timeout(Some(self.left()?))?;
        let mut stream = self.stream;
        stream.read(buf).map_err(expired_as_timed_out)
    }
}

impl Write for Timed<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.left()?))?;
        let mut stream = self.stream;
        stream.write(buf).map_err(expired_as_timed_out)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
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
        let serving = Serving {
            listener: self.listener.take().expect("checked above"),
            stopped,
            socket: self.socket.clone(),
            lock_path: self.lock_path.clone(),
            peer_deadline: self.peer_deadline,
            on_message,
        };
        std::thread::Builder::new()
            .name("teksilo-single-instance".into())
            .spawn(move || serving.run())?;
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
        let mode = fs::metadata(dir.path().join("app.sock")).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let mode = fs::metadata(dir.path().join("app.lock")).unwrap().mode() & 0o777;
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
        let path = dir.path().join("app.sock");
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
        let path = dir.path().join("app.sock");
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
        std::os::unix::fs::symlink(&target, dir.path().join("app.lock")).unwrap();
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
        let held = File::create(dir.path().join("app.lock")).unwrap();
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
        assert!(!dir.path().join("app.sock").exists());
        assert!(!dir.path().join("app.lock").exists());
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
        let socket = dir.path().join("app.sock");

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

    #[test]
    fn the_peer_check_accepts_this_user() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a).unwrap(), euid());
        check_peer(&a).unwrap();
    }
}
