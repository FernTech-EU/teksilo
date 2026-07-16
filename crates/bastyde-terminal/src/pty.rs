//! The pseudo-terminal: a thin wrapper over `portable-pty` that spawns the
//! child shell/process and exposes the master's writer + resize handle + child
//! killer for the UI thread, and hands the caller an independently-owned
//! blocking reader for its background thread.

use std::io::{self, Read, Write};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::engine::{PtyGeom, TerminalCommand, TerminalExit};

/// The UI-thread-owned half of a spawned PTY.
pub(crate) struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Pty {
    /// Write bytes to the child's input.
    pub(crate) fn write(&mut self, bytes: &[u8]) {
        // A dead child / closed PTY makes this fail; drop the error — the
        // reader thread's EOF is the authoritative "child gone" signal.
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
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

    /// Hard-terminate the child. On Unix this is `SIGKILL` (portable-pty's own
    /// `kill` sends only `SIGHUP`, which a `nohup`/daemon/`trap '' HUP` child can
    /// ignore — so it wouldn't honour `KillOnDrop`'s "must not outlive the view"
    /// guarantee); on Windows `Child::kill` is `TerminateProcess`, already hard.
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
    }
}

// NOTE: `Pty` deliberately does NOT kill the child on drop — that would make
// `TerminalClosePolicy::LeaveRunning` a no-op. The owner decides:
// `Terminal`'s `Drop` calls `kill()` for `KillOnDrop`; under `LeaveRunning` it
// leaves the child, and the natural drop of the master here closes it (the
// child receives `SIGHUP`). A direct engine user (no widget) owns the child
// lifecycle and should call `kill()` when done.

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

    let reader = pair.master.try_clone_reader().map_err(to_io)?;
    let writer = pair.master.take_writer().map_err(to_io)?;

    Ok((
        Pty {
            master: pair.master,
            writer,
            child,
        },
        reader,
    ))
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
