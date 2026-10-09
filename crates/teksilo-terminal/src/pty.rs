// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pseudo-terminal: a thin wrapper over `portable-pty` that spawns the
//! child shell/process and exposes its input, resize handle and child killer
//! for the UI thread, and hands the caller an independently-owned blocking
//! reader for its background thread.
//!
//! Neither direction can hold the UI thread. What the view writes (keys,
//! paste, the terminal's replies to the child's queries) goes to a queue a
//! writer thread drains, so a child that does not read its input stalls
//! nothing; the queue holds [`INPUT_QUEUE_CAP`] at most, and what a child
//! that never reads would push past it is dropped.
//!
//! The engine going ends the reads and writes in flight. On Unix the reader
//! and the writer each use a descriptor of their own on the PTY's master,
//! polled together with a stop pipe that [`Pty`] closes when it is killed or
//! dropped: the read returns the end of the output at once and the writer
//! abandons what is left, so the master closes with the engine and the
//! child's session is hung up, whatever still holds the child's side (a
//! grandchild started with `&`) and whether or not output is on its way. On
//! Windows the pseudoconsole closing with the engine ends both.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::{Arc, Condvar, Mutex};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::engine::{PtyGeom, TerminalCommand, TerminalExit};

/// The most input queued for the child before more is dropped.
pub(crate) const INPUT_QUEUE_CAP: usize = 4 << 20;

/// The UI-thread-owned half of a spawned PTY.
pub(crate) struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    input: Arc<InputQueue>,
    /// Ends the reads and writes in flight when dropped (Unix: closes the
    /// write end of the stop pipe both poll).
    stop: Option<Stop>,
}

impl Pty {
    /// Queue bytes for the child's input. Never waits: the writer thread
    /// writes them as the child takes them.
    pub(crate) fn write(&mut self, bytes: &[u8]) {
        self.input.push(bytes);
    }

    /// Resize the PTY window (sends `SIGWINCH` to the child).
    pub(crate) fn resize(&mut self, geom: PtyGeom) {
        let _ = self.master.resize(to_pty_size(geom));
    }

    /// Poll the child's exit status without blocking.
    pub(crate) fn poll_exit(&mut self) -> Option<TerminalExit> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(TerminalExit {
                success: status.success(),
                code: Some(status.exit_code()),
            }),
            _ => None,
        }
    }

    /// Hard-terminate the child, and end the reads and writes in flight.
    ///
    /// On Unix this is `SIGKILL` (portable-pty's own `kill` sends only
    /// `SIGHUP`, which a `nohup`/daemon/`trap '' HUP` child can ignore — so it
    /// wouldn't honour `KillOnDrop`'s "must not outlive the view" guarantee);
    /// on Windows `Child::kill` is `TerminateProcess`, already hard.
    pub(crate) fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.process_id() {
            // SAFETY: `kill(2)` with our own child's pid. SIGKILL can't be
            // caught, so a hung/SIGHUP-ignoring child still dies.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
        self.stop_io();
    }

    fn stop_io(&mut self) {
        self.input.close();
        self.stop = None;
    }
}

impl Drop for Pty {
    /// Ends the reads and writes in flight, so the master closes with the
    /// engine. `Pty` deliberately does NOT kill the child on drop — that would
    /// make `TerminalClosePolicy::LeaveRunning` a no-op. The owner decides:
    /// `Terminal`'s `Drop` calls `kill()` for `KillOnDrop`; under
    /// `LeaveRunning` the child is left, and the master closing here hangs up
    /// its session (it receives `SIGHUP`). A direct engine user (no widget)
    /// owns the child lifecycle and should call `kill()` when done.
    fn drop(&mut self) {
        self.stop_io();
    }
}

/// The input on its way to the child, between the UI thread and the writer
/// thread.
struct InputQueue {
    state: Mutex<InputState>,
    ready: Condvar,
}

#[derive(Default)]
struct InputState {
    bytes: VecDeque<u8>,
    /// The engine went, or the child cannot take input any more.
    closed: bool,
    /// The writer is waiting for input, so a test can tell it is.
    #[cfg(test)]
    waiting: bool,
}

impl InputQueue {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(InputState::default()),
            ready: Condvar::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, InputState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Queue what fits under [`INPUT_QUEUE_CAP`]; nothing once closed.
    fn push(&self, bytes: &[u8]) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        let room = INPUT_QUEUE_CAP.saturating_sub(state.bytes.len());
        state.bytes.extend(&bytes[..bytes.len().min(room)]);
        drop(state);
        self.ready.notify_one();
    }

    /// Writer thread: wait for input, then take up to `max` bytes of it.
    /// `None` once closed: what is left is abandoned.
    fn wait_take(&self, max: usize) -> Option<Vec<u8>> {
        let mut state = self.lock();
        while !state.closed && state.bytes.is_empty() {
            #[cfg(test)]
            {
                state.waiting = true;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(test)]
            {
                state.waiting = false;
            }
        }
        if state.closed {
            return None;
        }
        let n = max.min(state.bytes.len());
        Some(state.bytes.drain(..n).collect())
    }

    fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.bytes.clear();
        drop(state);
        self.ready.notify_all();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().bytes.len()
    }
}

fn to_pty_size(geom: PtyGeom) -> PtySize {
    PtySize {
        rows: geom.rows,
        cols: geom.cols,
        pixel_width: geom.pixel_width,
        pixel_height: geom.pixel_height,
    }
}

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

/// Spawn the child over a fresh PTY. Returns the UI-thread [`Pty`] handle plus
/// the child-output reader for the caller's background thread.
pub(crate) fn spawn(
    command: &TerminalCommand,
    geom: PtyGeom,
) -> io::Result<(Pty, Box<dyn Read + Send>)> {
    let system = native_pty_system();
    let pair = system.openpty(to_pty_size(geom)).map_err(to_io)?;

    let cmd = build_command(command);
    let child = pair.slave.spawn_command(cmd).map_err(to_io)?;
    // The slave is no longer needed once the child owns it; dropping it here is
    // correct (`PtyPair` drops slave-first anyway).
    drop(pair.slave);

    let input = InputQueue::new();
    let (reader, stop) = io_threads(pair.master.as_ref(), &input)?;
    Ok((
        Pty {
            master: pair.master,
            child,
            input,
            stop: Some(stop),
        },
        reader,
    ))
}

#[cfg(unix)]
use unix::{Stop, io_threads};

#[cfg(unix)]
mod unix {
    //! The master's reader and writer, on descriptors of their own, each
    //! polled with the stop pipe.

    use std::io::{self, Read};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::sync::Arc;

    use portable_pty::MasterPty;

    use super::InputQueue;

    /// The write end of the stop pipe: dropping it makes the read end
    /// readable (end of file), which every poll below watches.
    pub(crate) struct Stop {
        _write_end: OwnedFd,
    }

    /// Start the writer thread and return the reader, with the stop that
    /// ends both.
    pub(crate) fn io_threads(
        master: &dyn MasterPty,
        input: &Arc<InputQueue>,
    ) -> io::Result<(Box<dyn Read + Send>, Stop)> {
        let fd = master
            .as_raw_fd()
            .ok_or_else(|| io::Error::other("the PTY master has no descriptor"))?;
        let (stop_read, stop_write) = pipe()?;
        let read_fd = dup(fd)?;
        let write_fd = dup(fd)?;
        // The descriptors share the master's open file description, flags
        // included: non-blocking for both, so a poll that saw it ready and a
        // read or write that finds it no longer so loop instead of blocking.
        set_nonblocking(read_fd.as_raw_fd())?;
        let reader = MasterReader {
            fd: read_fd,
            stop: dup(stop_read.as_raw_fd())?,
        };
        let writer = MasterWriter {
            fd: write_fd,
            stop: stop_read,
        };
        let input = Arc::clone(input);
        std::thread::Builder::new()
            .name("teksilo-terminal-pty-input".into())
            .spawn(move || writer.run(&input))?;
        Ok((
            Box::new(reader),
            Stop {
                _write_end: stop_write,
            },
        ))
    }

    /// Reads the child's output until it ends or the stop pipe closes.
    struct MasterReader {
        fd: OwnedFd,
        stop: OwnedFd,
    }

    impl Read for MasterReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            loop {
                if !wait(self.fd.as_raw_fd(), libc::POLLIN, self.stop.as_raw_fd())? {
                    // Stopped: the engine went, or the child was killed.
                    return Ok(0);
                }
                // SAFETY: `buf` is valid for `buf.len()` bytes of writes.
                let n =
                    unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n >= 0 {
                    return Ok(n as usize);
                }
                let error = io::Error::last_os_error();
                match error.raw_os_error() {
                    Some(libc::EINTR) | Some(libc::EAGAIN) => continue,
                    // Every holder of the child's side closed it: the end of
                    // the output (Linux reports it as an error).
                    Some(libc::EIO) => return Ok(0),
                    _ => return Err(error),
                }
            }
        }
    }

    /// Writes the queued input as the child takes it, until the queue closes,
    /// the stop pipe closes, or the child can take no more.
    struct MasterWriter {
        fd: OwnedFd,
        stop: OwnedFd,
    }

    impl MasterWriter {
        fn run(self, input: &InputQueue) {
            while let Some(chunk) = input.wait_take(8192) {
                if !self.write_all(&chunk) {
                    break;
                }
            }
            // The child takes no more input, or the engine went: nothing
            // queued from now on reaches it.
            input.close();
        }

        /// Whether all of `bytes` was written.
        fn write_all(&self, mut bytes: &[u8]) -> bool {
            while !bytes.is_empty() {
                match wait(self.fd.as_raw_fd(), libc::POLLOUT, self.stop.as_raw_fd()) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => return false,
                }
                // SAFETY: `bytes` is valid for `bytes.len()` bytes of reads.
                let n =
                    unsafe { libc::write(self.fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
                if n >= 0 {
                    bytes = &bytes[n as usize..];
                    continue;
                }
                match io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) | Some(libc::EAGAIN) => continue,
                    _ => return false,
                }
            }
            true
        }
    }

    /// Wait until `fd` has `events` (or an error or hang-up, which the
    /// following read or write reports) or the stop pipe closes. `false`
    /// when stopped.
    fn wait(fd: RawFd, events: libc::c_short, stop: RawFd) -> io::Result<bool> {
        loop {
            let mut fds = [
                libc::pollfd {
                    fd,
                    events,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: `fds` is a valid array of two `pollfd`s.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(error);
            }
            if fds[1].revents != 0 {
                return Ok(false);
            }
            if fds[0].revents != 0 {
                return Ok(true);
            }
        }
    }

    fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` has room for the two descriptors `pipe` writes.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both were just returned by `pipe` and are owned by nobody
        // else.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        set_cloexec(read.as_raw_fd())?;
        set_cloexec(write.as_raw_fd())?;
        Ok((read, write))
    }

    /// A close-on-exec duplicate, so a child spawned later inherits none.
    fn dup(fd: RawFd) -> io::Result<OwnedFd> {
        // SAFETY: `F_DUPFD_CLOEXEC` on a descriptor we hold open.
        let new = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if new < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `new` was just returned by `fcntl` and is owned by nobody
        // else.
        Ok(unsafe { OwnedFd::from_raw_fd(new) })
    }

    fn set_cloexec(fd: RawFd) -> io::Result<()> {
        // SAFETY: `F_GETFD`/`F_SETFD` on a descriptor we hold open.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn set_nonblocking(fd: RawFd) -> io::Result<()> {
        // SAFETY: `F_GETFL`/`F_SETFL` on a descriptor we hold open.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[cfg(not(unix))]
use other::{Stop, io_threads};

#[cfg(not(unix))]
mod other {
    //! portable-pty's own reader and writer: the pseudoconsole closing with
    //! the engine ends a read or write in flight.

    use std::io::{self, Read, Write};
    use std::sync::Arc;

    use portable_pty::MasterPty;

    use super::InputQueue;

    /// Nothing to close: the queue closing stops the writer thread, and the
    /// master's drop ends the rest.
    pub(crate) struct Stop;

    pub(crate) fn io_threads(
        master: &dyn MasterPty,
        input: &Arc<InputQueue>,
    ) -> io::Result<(Box<dyn Read + Send>, Stop)> {
        let reader = master.try_clone_reader().map_err(super::to_io)?;
        let mut writer = master.take_writer().map_err(super::to_io)?;
        let input = Arc::clone(input);
        std::thread::Builder::new()
            .name("teksilo-terminal-pty-input".into())
            .spawn(move || {
                while let Some(chunk) = input.wait_take(8192) {
                    if writer
                        .write_all(&chunk)
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
                input.close();
            })?;
        Ok((reader, Stop))
    }
}

fn build_command(command: &TerminalCommand) -> CommandBuilder {
    let mut builder = match &command.program {
        Some(program) => {
            // An explicit program: pass its args normally.
            let mut b = CommandBuilder::new(program);
            for arg in &command.args {
                b.arg(arg);
            }
            b
        }
        // The platform default login shell. `new_default_prog()` must NOT have
        // args added to it (portable-pty panics otherwise), so we never touch
        // `command.args` on this path.
        None => CommandBuilder::new_default_prog(),
    };

    for (key, value) in &command.env {
        builder.env(key, value);
    }
    if let Some(cwd) = &command.cwd {
        builder.cwd(cwd);
    }
    // Advertise a capable terminal so programs enable colour + full features.
    // Only set it when the caller didn't override TERM themselves.
    if !command.env.iter().any(|(k, _)| k == "TERM") {
        builder.env("TERM", "xterm-256color");
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_queue_holds_its_cap_at_most_and_nothing_once_closed() {
        let queue = InputQueue::new();
        queue.push(&vec![b'x'; INPUT_QUEUE_CAP + 100]);
        assert_eq!(
            queue.len(),
            INPUT_QUEUE_CAP,
            "what is past the cap is dropped"
        );
        assert_eq!(queue.wait_take(10).map(|b| b.len()), Some(10));
        queue.close();
        assert_eq!(queue.len(), 0, "closing abandons what was left");
        queue.push(b"late");
        assert_eq!(queue.len(), 0);
        assert_eq!(queue.wait_take(10), None);
    }

    #[test]
    fn closing_the_input_queue_releases_a_waiting_writer() {
        let queue = InputQueue::new();
        let remote = Arc::clone(&queue);
        let writer = std::thread::spawn(move || remote.wait_take(10));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !queue.lock().waiting {
            assert!(
                std::time::Instant::now() < deadline,
                "the writer never waited"
            );
            std::thread::yield_now();
        }
        queue.close();
        assert_eq!(writer.join().unwrap(), None);
    }
}
