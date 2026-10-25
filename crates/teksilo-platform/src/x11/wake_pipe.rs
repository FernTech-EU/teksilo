// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The self-pipe and the `poll(2)` wait shared by the threads that read our
//! own X connection: the XDND backend's and the window-state watch's.

use std::os::fd::RawFd;
use std::time::Duration;

/// A self-pipe used to break a thread watching our X connection (the DnD
/// thread, the window-state watch) out of `poll(2)`.
///
/// The alternative, sending ourselves an X message, would need a second
/// connection just for teardown; a pipe is two file descriptors and cannot
/// fail because the X connection is already broken.
pub(crate) struct Waker {
    read: RawFd,
    write: RawFd,
}

impl Waker {
    pub(crate) fn new() -> Result<Self, std::io::Error> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` is a valid two-element array for `pipe2` to fill.
        // O_CLOEXEC keeps the descriptors out of any child process.
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            read: fds[0],
            write: fds[1],
        })
    }

    pub(crate) fn read_fd(&self) -> RawFd {
        self.read
    }

    pub(crate) fn wake(&self) {
        let byte = 1u8;
        // SAFETY: `self.write` is an open pipe descriptor owned by `self` and
        // `byte` is a valid one-byte buffer. A full pipe (EAGAIN) is fine: a
        // wakeup is already pending, which is all the reader needs.
        unsafe {
            libc::write(self.write, std::ptr::from_ref(&byte).cast(), 1);
        }
    }
}

impl Drop for Waker {
    fn drop(&mut self) {
        // SAFETY: both descriptors are owned by `self` and closed exactly once.
        unsafe {
            libc::close(self.read);
            libc::close(self.write);
        }
    }
}

/// Block until the X socket or the wake pipe is readable, or `timeout` passes.
/// `None` waits indefinitely.
pub(crate) fn wait_readable(x_fd: RawFd, wake_fd: RawFd, timeout: Option<Duration>) {
    let mut fds = [
        libc::pollfd {
            fd: x_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let millis = timeout.map_or(-1, |d| d.as_millis().min(i32::MAX as u128) as libc::c_int);
    // SAFETY: `fds` is a valid array of two initialised pollfds.
    unsafe {
        libc::poll(fds.as_mut_ptr(), 2, millis);
    }
    if fds[1].revents & libc::POLLIN != 0 {
        // Drain the pipe so the next poll blocks again.
        let mut buf = [0u8; 64];
        // SAFETY: `buf` is a valid 64-byte buffer and the fd is non-blocking,
        // so this returns immediately once drained.
        while unsafe { libc::read(wake_fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }
}
