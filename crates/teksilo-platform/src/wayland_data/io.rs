// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Bounded pipe I/O for data transfers between Wayland clients.
//!
//! Every byte that crosses the clipboard or a drop travels through a pipe the
//! two clients share, and the peer at the other end is another program: it
//! may write slowly, write forever, or never write at all and keep the pipe
//! open. A blocking `read_to_end` on such a pipe freezes whatever thread
//! called it for as long as the peer likes, and a blocking `write_all` does
//! the same once the pipe buffer is full and the reader has stopped reading.
//!
//! So both directions here are bounded twice: by an **idle** limit (no byte
//! moved for that long ends the transfer) and by a **total** limit, plus a
//! size cap on reads so a peer cannot make us allocate without end.

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

/// Limits for one transfer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// Give up when no byte has moved for this long.
    pub(crate) idle: Duration,
    /// Give up when the whole transfer has taken this long.
    pub(crate) total: Duration,
    /// Give up on a read past this many bytes.
    pub(crate) max_bytes: usize,
}

impl Limits {
    /// A paste, read on the UI thread: short, because the user is waiting
    /// and the window cannot draw meanwhile, but long enough for an owner
    /// that builds the format on request (LibreOffice's HTML of a long
    /// selection) to send its first byte.
    #[cfg_attr(
        not(feature = "clipboard"),
        allow(dead_code, reason = "a paste is the clipboard's")
    )]
    pub(crate) const PASTE: Limits = Limits {
        idle: Duration::from_secs(3),
        total: Duration::from_secs(5),
        max_bytes: 256 * 1024 * 1024,
    };

    /// Serving our selection or a drop to another client, on a thread of its
    /// own: longer, since nobody is frozen while it runs.
    pub(crate) const SERVE: Limits = Limits {
        idle: Duration::from_secs(5),
        total: Duration::from_secs(60),
        max_bytes: usize::MAX,
    };
}

/// Why a transfer ended without completing.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TransferError {
    #[error("the other application sent nothing for {0:?}")]
    Idle(Duration),
    #[error("the transfer took longer than {0:?}")]
    Total(Duration),
    #[error("the data is larger than {0} bytes")]
    TooLarge(usize),
    #[error("pipe error: {0}")]
    Io(#[from] io::Error),
}

/// A pipe: `(read end, write end)`, both close-on-exec.
pub(crate) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0i32; 2];
    // SAFETY: `fds` is a valid two-element buffer for `pipe2` to fill.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `pipe2` succeeded, so both descriptors are open and owned by us
    // alone; each is wrapped exactly once.
    let read = unsafe { <OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fds[0]) };
    // SAFETY: as above.
    let write = unsafe { <OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fds[1]) };
    Ok((read, write))
}

fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let raw = fd.as_raw_fd();
    // SAFETY: `raw` is an open descriptor owned by `fd` for this call.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above; only the status flags are changed.
    if unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Wait until `fd` is ready for `events`, for at most `timeout`. `Ok(false)`
/// on timeout.
fn wait(fd: &OwnedFd, events: libc::c_short, timeout: Duration) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events,
        revents: 0,
    };
    let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    loop {
        // SAFETY: `pfd` is one valid pollfd for the duration of the call.
        let n = unsafe { libc::poll(&mut pfd, 1, millis) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        // A hang-up or an error counts as ready: the read or write that
        // follows reports what happened.
        return Ok(n > 0);
    }
}

/// The time left before the next limit, and which limit it is.
fn remaining(
    limits: Limits,
    start: Instant,
    last_progress: Instant,
) -> Result<Duration, TransferError> {
    let now = Instant::now();
    let total_left = limits.total.checked_sub(now.duration_since(start));
    let idle_left = limits.idle.checked_sub(now.duration_since(last_progress));
    match (total_left, idle_left) {
        (None, _) => Err(TransferError::Total(limits.total)),
        (_, None) => Err(TransferError::Idle(limits.idle)),
        (Some(t), Some(i)) => Ok(t.min(i)),
    }
}

/// Read `fd` to its end within `limits`.
pub(crate) fn read_bounded(fd: OwnedFd, limits: Limits) -> Result<Vec<u8>, TransferError> {
    set_nonblocking(&fd)?;
    let start = Instant::now();
    let mut last_progress = start;
    let mut out = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        // SAFETY: `buf` is a valid writable buffer of `buf.len()` bytes.
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            // `n` is positive and at most `buf.len()`.
            let n = n as usize;
            if out.len() + n > limits.max_bytes {
                return Err(TransferError::TooLarge(limits.max_bytes));
            }
            out.extend_from_slice(&buf[..n]);
            last_progress = Instant::now();
            continue;
        }
        if n == 0 {
            return Ok(out);
        }
        let err = io::Error::last_os_error();
        match err.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => {
                let left = remaining(limits, start, last_progress)?;
                wait(&fd, libc::POLLIN, left)?;
            }
            _ => return Err(err.into()),
        }
    }
}

/// Write all of `bytes` to `fd` within `limits`, then close it so the reader
/// sees the end. A reader that went away (`EPIPE`) ends the transfer quietly:
/// it no longer wants the rest.
pub(crate) fn write_bounded(
    fd: OwnedFd,
    bytes: &[u8],
    limits: Limits,
) -> Result<(), TransferError> {
    set_nonblocking(&fd)?;
    let start = Instant::now();
    let mut last_progress = start;
    let mut written = 0;
    while written < bytes.len() {
        let rest = &bytes[written..];
        // SAFETY: `rest` is a valid readable buffer of `rest.len()` bytes.
        // `MSG_NOSIGNAL` is socket-only, so SIGPIPE is avoided by Rust's
        // default of ignoring it in the process; the write then fails with
        // `EPIPE`, handled below.
        let n = unsafe { libc::write(fd.as_raw_fd(), rest.as_ptr().cast(), rest.len()) };
        if n >= 0 {
            // `n` is non-negative and at most `rest.len()`.
            written += n as usize;
            last_progress = Instant::now();
            continue;
        }
        let err = io::Error::last_os_error();
        match err.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::BrokenPipe => return Ok(()),
            io::ErrorKind::WouldBlock => {
                let left = remaining(limits, start, last_progress)?;
                wait(&fd, libc::POLLOUT, left)?;
            }
            _ => return Err(err.into()),
        }
    }
    Ok(())
}

/// Write `bytes` to `fd` on a short-lived thread of its own, so a reader that
/// stops reading holds up nothing but that thread, and only until `limits`.
/// `what` names the transfer in the warning a failure prints.
pub(crate) fn serve_in_background(fd: OwnedFd, bytes: std::sync::Arc<[u8]>, what: String) {
    let label = what.clone();
    let spawned = std::thread::Builder::new()
        .name("teksilo-wayland-send".into())
        .spawn(move || {
            if let Err(err) = write_bounded(fd, &bytes, Limits::SERVE) {
                eprintln!("teksilo-platform: sending {what} to another application failed: {err}");
            }
        });
    if let Err(err) = spawned {
        eprintln!("teksilo-platform: could not start a thread to send {label}: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUICK: Limits = Limits {
        idle: Duration::from_millis(150),
        total: Duration::from_millis(400),
        max_bytes: 1024,
    };

    #[test]
    fn reads_everything_a_writer_sends_before_closing() {
        let (r, w) = pipe().unwrap();
        let writer = std::thread::spawn(move || {
            write_bounded(w, b"hello, clipboard", Limits::SERVE).unwrap();
        });
        assert_eq!(read_bounded(r, QUICK).unwrap(), b"hello, clipboard");
        writer.join().unwrap();
    }

    #[test]
    fn a_writer_that_never_writes_times_out_instead_of_hanging() {
        let (r, w) = pipe().unwrap();
        let started = Instant::now();
        let err = read_bounded(r, QUICK).unwrap_err();
        assert!(matches!(err, TransferError::Idle(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(w);
    }

    #[test]
    fn a_slow_trickle_is_cut_at_the_total_limit() {
        let (r, w) = pipe().unwrap();
        let writer = std::thread::spawn(move || {
            let mut file = std::fs::File::from(w);
            for _ in 0..20 {
                if std::io::Write::write_all(&mut file, b"x").is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let err = read_bounded(r, QUICK).unwrap_err();
        assert!(matches!(err, TransferError::Total(_)), "{err:?}");
        writer.join().unwrap();
    }

    #[test]
    fn a_read_past_the_size_cap_is_refused() {
        let (r, w) = pipe().unwrap();
        let writer = std::thread::spawn(move || {
            let _ = write_bounded(w, &[b'a'; 4096], Limits::SERVE);
        });
        let err = read_bounded(r, QUICK).unwrap_err();
        assert!(matches!(err, TransferError::TooLarge(1024)), "{err:?}");
        writer.join().unwrap();
    }

    #[test]
    fn a_reader_that_stops_reading_does_not_hold_the_writer_forever() {
        let (r, w) = pipe().unwrap();
        // Far more than a pipe buffer holds, and nobody reads it.
        let big = vec![0u8; 4 * 1024 * 1024];
        let started = Instant::now();
        let err = write_bounded(w, &big, QUICK).unwrap_err();
        assert!(
            matches!(err, TransferError::Idle(_) | TransferError::Total(_)),
            "{err:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(r);
    }

    #[test]
    fn a_reader_that_closes_early_ends_the_write_quietly() {
        let (r, w) = pipe().unwrap();
        drop(r);
        write_bounded(w, b"nobody is listening", QUICK).unwrap();
    }
}
