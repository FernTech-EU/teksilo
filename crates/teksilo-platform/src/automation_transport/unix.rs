// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Unix-domain-socket transport (Linux, macOS, BSD).
//!
//! The socket lives in a per-process directory created `0700` with `mkdir`'s
//! own mode, so it is never world-reachable — not even during the window
//! between `bind` and a follow-up `chmod`, which is the TOCTOU this ordering
//! exists to close. The socket then gets `0600` as belt and braces.
//!
//! ## Path length
//!
//! `sockaddr_un::sun_path` is **104 bytes on macOS** (108 on Linux), and macOS
//! hands out a per-user `$TMPDIR` under `/var/folders/…` that is already ~52
//! characters. Overflow is not truncation — `bind` fails outright — so the
//! path is measured before use and a short `/tmp` fallback is taken if the
//! preferred one will not fit. The fallback keeps the same `0700` directory
//! discipline, so it is a shorter path, not a weaker one.

use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use teksilo_automation::wire::Endpoint;

use super::{BoundTransport, TransportListener, TransportStream};

/// Conservative ceiling for a socket path, below the 104-byte `sun_path` of
/// the tightest platform (macOS) with room for the terminating NUL.
const MAX_SOCKET_PATH: usize = 100;

/// A `UnixStream` whose reads report a deadline the way the trait promises.
///
/// `SO_RCVTIMEO` expires as `EAGAIN` on Unix but as `WSAETIMEDOUT` on Windows,
/// so `std` surfaces one event as two kinds: [`io::ErrorKind::WouldBlock`]
/// here, [`io::ErrorKind::TimedOut`] there. The trait documents a single kind
/// and the Windows backend already synthesises it, so the normalisation
/// belongs on this side of the seam rather than in every caller — the whole
/// point of the trait is that the two backends are interchangeable.
///
/// The socket is never put in non-blocking mode, so an expired read deadline
/// is the only thing that can produce `WouldBlock` on it.
struct SocketStream(UnixStream);

impl Read for SocketStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf).map_err(timed_out_if_would_block)
    }
}

impl Write for SocketStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl TransportStream for SocketStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }
}

/// Map the Unix spelling of "the read deadline expired" onto the trait's.
fn timed_out_if_would_block(e: io::Error) -> io::Error {
    if e.kind() == io::ErrorKind::WouldBlock {
        io::Error::new(io::ErrorKind::TimedOut, "the socket read deadline expired")
    } else {
        e
    }
}

/// Owns the listener and removes its directory when the bridge thread exits.
struct SocketListener {
    listener: UnixListener,
    dir: PathBuf,
}

impl TransportListener for SocketListener {
    fn accept(&mut self) -> io::Result<Box<dyn TransportStream>> {
        let (stream, _addr) = self.listener.accept()?;
        Ok(Box::new(SocketStream(stream)))
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The preferred per-process socket directory, and a short fallback.
fn candidate_dirs(pid: u32) -> Vec<PathBuf> {
    let mut dirs = vec![teksilo_automation::wire::EndpointFile::dir().join(format!("{pid}.d"))];
    // Short, last-resort location for a `$TMPDIR` deep enough to overflow
    // `sun_path`. Still per-process and still `0700`.
    dirs.push(PathBuf::from(format!("/tmp/tka-{pid}")));
    dirs
}

pub(super) fn bind(pid: u32) -> io::Result<BoundTransport> {
    bind_over(&candidate_dirs(pid))
}

/// The candidate-selection loop `bind` runs, over an explicit candidate list so
/// a test can drive the overflow branch without a 120-character `$TMPDIR`.
fn bind_over(candidates: &[PathBuf]) -> io::Result<BoundTransport> {
    let mut last_err = None;
    for dir in candidates {
        let path = dir.join("s");
        if path.as_os_str().len() > MAX_SOCKET_PATH {
            last_err = Some(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "socket path {} is {} bytes, over the {MAX_SOCKET_PATH}-byte limit",
                    path.display(),
                    path.as_os_str().len()
                ),
            ));
            continue;
        }
        match bind_at(dir, &path) {
            Ok(listener) => {
                let address = path.to_string_lossy().into_owned();
                return Ok(BoundTransport {
                    listener: Box::new(listener),
                    endpoint: Endpoint::unix(address),
                });
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::other("no usable directory for the automation bridge socket")
    }))
}

fn bind_at(dir: &Path, path: &Path) -> io::Result<SocketListener> {
    // A stale directory from a crashed prior run with the same PID.
    let _ = std::fs::remove_dir_all(dir);
    // Only the automation-owned descriptor directory is ours to create and
    // tighten, and only there is `wire`'s helper the right tool rather than a
    // bare `create_dir_all`: that honours the umask (0755 as a rule) and this
    // runs *before* the descriptor is published, so it is what decides the mode
    // the descriptor directory ends up with. Creating it loosely here and
    // letting `EndpointFile::write` find it "already existing" is how the
    // documented `0700` silently became world-readable.
    //
    // The `/tmp` fallback's parent is emphatically *not* ours: it is a
    // pre-existing shared system directory. On macOS `/tmp` is a symlink to
    // `private/tmp`, which the helper rejects by design (a symlink is not what
    // we would be protecting); on Linux it is `1777`, which the helper would
    // "tighten" to `0700` — failing with EPERM for a normal user and, far
    // worse, *succeeding* for one running as root, breaking `/tmp` for every
    // other process on the machine. Neither is a mode we may impose on a
    // directory we did not create, so the fallback relies on `dir`'s own
    // `0700` below: a shorter path, not a weaker one.
    let descriptor_dir = teksilo_automation::wire::EndpointFile::dir();
    if dir.parent() == Some(descriptor_dir.as_path()) {
        teksilo_automation::wire::create_private_dir(&descriptor_dir)?;
    }
    // `mkdir`'s own mode: the directory is never briefly world-reachable.
    std::fs::DirBuilder::new().mode(0o700).create(dir)?;
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(SocketListener {
        listener,
        dir: dir.to_path_buf(),
    })
}

pub(super) fn connect(address: &str) -> io::Result<Box<dyn TransportStream>> {
    Ok(Box::new(SocketStream(UnixStream::connect(address)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_is_owner_only_in_an_owner_only_directory() {
        let pid = std::process::id().wrapping_add(7);
        let bound = match bind(pid) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return,
            Err(e) => panic!("bind failed: {e}"),
        };
        let path = PathBuf::from(&bound.endpoint.address);
        let sock_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(sock_mode, 0o600, "socket must be owner-only");
        assert_eq!(dir_mode, 0o700, "directory must be owner-only");
    }

    #[test]
    fn dropping_the_listener_removes_the_directory() {
        let pid = std::process::id().wrapping_add(8);
        let bound = match bind(pid) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return,
            Err(e) => panic!("bind failed: {e}"),
        };
        let path = PathBuf::from(&bound.endpoint.address);
        let dir = path.parent().unwrap().to_path_buf();
        assert!(dir.exists());
        drop(bound);
        assert!(
            !dir.exists(),
            "the per-process directory must not outlive the bridge"
        );
    }

    #[test]
    fn an_overlong_preferred_path_falls_back_instead_of_failing() {
        // Simulate the macOS hazard — a `$TMPDIR` deep enough that the socket
        // path overruns `sun_path` — and drive the *real* selection loop with
        // it, rather than asserting that a locally-built string is long (which
        // proves nothing about `bind`).
        let deep = PathBuf::from("/tmp/".to_string() + &"d".repeat(120));
        assert!(
            deep.join("s").as_os_str().len() > MAX_SOCKET_PATH,
            "the simulated preferred directory must actually overflow"
        );

        let fallback = candidate_dirs(1234).pop().expect("a fallback candidate");
        assert!(
            fallback.join("s").as_os_str().len() <= MAX_SOCKET_PATH,
            "the fallback must always fit"
        );

        // The guard `bind` applies, over a candidate list whose preferred entry
        // cannot fit: it must skip to the fallback and bind there.
        let pid = std::process::id().wrapping_add(9);
        let candidates = vec![deep.clone(), PathBuf::from(format!("/tmp/tka-{pid}"))];
        let bound = match bind_over(&candidates) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return,
            Err(e) => panic!("bind should have fallen back, but failed: {e}"),
        };
        assert!(
            !bound.endpoint.address.starts_with(deep.to_str().unwrap()),
            "the overlong preferred path must be skipped, got {}",
            bound.endpoint.address
        );
        // Positively: it bound *at the fallback*, rather than the assertion
        // above passing vacuously on some third path. The parent here is a
        // shared system directory (`/tmp`) that `bind_at` must leave alone —
        // tightening it is EPERM for a normal user and, as root, would chmod
        // `/tmp` to `0700` for the whole machine — while the per-process
        // directory it creates underneath is still owner-only.
        let sock = PathBuf::from(&bound.endpoint.address);
        assert_eq!(
            sock.parent(),
            Some(PathBuf::from(format!("/tmp/tka-{pid}")).as_path()),
            "expected the short `/tmp` fallback, got {}",
            bound.endpoint.address
        );
        let dir_mode = std::fs::metadata(sock.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700, "the fallback directory must be owner-only");
    }
}
