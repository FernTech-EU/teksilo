// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Verifies that `TerminalClosePolicy::KillOnDrop`'s kill is a *hard* kill on
//! Unix — portable-pty's own kill sends only `SIGHUP`, which a child trapping
//! HUP survives, so the engine must escalate to `SIGKILL`. Unix + the
//! `alacritty` backend only (it owns the real PTY).
#![cfg(all(unix, feature = "alacritty"))]

use std::time::Duration;

use bastyde_terminal::{AlacrittyEngineFactory, PtyGeom, TerminalCommand, TerminalEngineFactory};

#[test]
fn kill_hard_kills_a_sighup_ignoring_child() {
    let factory = AlacrittyEngineFactory;
    // A shell that ignores SIGHUP and then blocks on stdin (no subprocess to
    // orphan). Under a mere SIGHUP it would survive; only SIGKILL ends it.
    let command =
        TerminalCommand::program("sh", ["-c".to_string(), "trap '' HUP; read _".to_string()]);
    let mut spawned = factory
        .spawn(&command, PtyGeom::new(80, 24, 0, 0), 100)
        .expect("spawn sh");

    // Give the shell a moment to install the trap and block on read.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        spawned.engine.poll_exit().is_none(),
        "child should still be running before kill"
    );

    spawned.engine.kill();

    // Poll for the child to be reaped (SIGKILL can't be trapped).
    let mut exited = false;
    for _ in 0..100 {
        if spawned.engine.poll_exit().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        exited,
        "KillOnDrop's kill() must terminate a SIGHUP-ignoring child (SIGKILL)"
    );
}
