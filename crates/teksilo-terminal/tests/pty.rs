// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The default engine's PTY against real children: the engine going ends the
//! read in flight and hangs the child's session up, whatever holds the
//! child's side, and writing never waits for a child that does not read.
//! Unix + the `alacritty` backend only (it owns the real PTY).
#![cfg(all(unix, feature = "alacritty"))]

use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use teksilo_terminal::{AlacrittyEngineFactory, PtyGeom, TerminalCommand, TerminalEngineFactory};

const WAIT: Duration = Duration::from_secs(10);

fn sh(script: &str) -> TerminalCommand {
    TerminalCommand::program("sh", ["-c".to_string(), script.to_string()])
}

/// What the child printed, read on a thread of its own as the terminal's
/// reader thread does: each read's text, then `None` at the end.
fn read_on_a_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(None);
                    return;
                }
                Ok(n) => {
                    let _ = tx.send(Some(String::from_utf8_lossy(&buf[..n]).into_owned()));
                }
            }
        }
    });
    rx
}

/// Read until the output contains `needle`; what was read so far.
fn read_until(rx: &Receiver<Option<String>>, needle: &str) -> String {
    let deadline = Instant::now() + WAIT;
    let mut seen = String::new();
    while !seen.contains(needle) {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Some(text)) => seen.push_str(&text),
            Ok(None) => panic!("the output ended before {needle:?}: {seen:?}"),
            Err(_) => panic!("no {needle:?} in time: {seen:?}"),
        }
    }
    seen
}

/// Whether the reader reaches the end of the output within `timeout`.
fn ends_within(rx: &Receiver<Option<String>>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Some(_)) => continue,
            Ok(None) => return true,
            Err(_) => return false,
        }
    }
}

fn marker() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("teksilo-pty-{}-{nanos}", std::process::id()))
}

/// A child left running (no kill) is hung up when its engine goes, while the
/// terminal's reader is blocked reading it: the read no longer holds the PTY
/// open.
#[test]
fn a_child_left_running_is_hung_up_when_the_engine_goes() {
    let mark = marker();
    let script = format!(
        "trap 'echo hup > {}; exit 0' HUP; echo ready; while :; do sleep 0.05; done",
        mark.display()
    );
    let spawned = AlacrittyEngineFactory
        .spawn(&sh(&script), PtyGeom::new(80, 24, 0, 0), 100)
        .expect("spawn sh");
    let rx = read_on_a_thread(spawned.reader);
    read_until(&rx, "ready");

    drop(spawned.engine);
    assert!(ends_within(&rx, WAIT), "the read ends with the engine");
    let deadline = Instant::now() + WAIT;
    while !mark.exists() {
        assert!(Instant::now() < deadline, "the child was never hung up");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&mark);
}

/// A kill ends the read at once, even while a grandchild that ignores the
/// hangup still holds the child's side of the PTY.
#[test]
fn a_kill_ends_the_read_while_a_grandchild_holds_the_pty() {
    let script = "trap '' HUP; sleep 20 & echo \"pid:$!:\"; read _";
    let mut spawned = AlacrittyEngineFactory
        .spawn(&sh(script), PtyGeom::new(80, 24, 0, 0), 100)
        .expect("spawn sh");
    let rx = read_on_a_thread(spawned.reader);
    let seen = read_until(&rx, ":\r\n");
    let pid: libc::pid_t = seen
        .split("pid:")
        .nth(1)
        .and_then(|rest| rest.split(':').next())
        .and_then(|pid| pid.trim().parse().ok())
        .expect("the grandchild's pid");

    spawned.engine.kill();
    let ended = ends_within(&rx, WAIT);
    // SAFETY: `kill(2)` on the grandchild this test started.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    assert!(ended, "the read ends with the kill");
}

/// Writing to a child that never reads its input returns at once, however
/// much is written.
#[test]
fn writing_to_a_child_that_never_reads_never_waits() {
    let mut spawned = AlacrittyEngineFactory
        .spawn(&sh("echo ready; sleep 20"), PtyGeom::new(80, 24, 0, 0), 100)
        .expect("spawn sh");
    let rx = read_on_a_thread(spawned.reader);
    read_until(&rx, "ready");
    // A write that blocked would hang this thread, so a watchdog fails the
    // run instead (this file is a test binary of its own).
    let (done_tx, done_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if done_rx.recv_timeout(WAIT).is_err() {
            eprintln!("a write to the child blocked");
            std::process::abort();
        }
    });
    let started = Instant::now();
    spawned.engine.write(&vec![b'x'; 8 << 20]);
    spawned.engine.write(b"more\n");
    let took = started.elapsed();
    done_tx.send(()).unwrap();
    assert!(took < Duration::from_secs(1), "writing waited {took:?}");
    spawned.engine.kill();
    assert!(ends_within(&rx, WAIT));
}

/// What the view writes reaches the child, and what the child prints in reply
/// comes back.
#[test]
fn input_reaches_the_child_and_its_reply_comes_back() {
    let mut spawned = AlacrittyEngineFactory
        .spawn(
            &sh("echo ready; read line; echo \"got:$line\""),
            PtyGeom::new(80, 24, 0, 0),
            100,
        )
        .expect("spawn sh");
    let rx = read_on_a_thread(spawned.reader);
    read_until(&rx, "ready");
    spawned.engine.write(b"hello\n");
    read_until(&rx, "got:hello");
    assert!(
        ends_within(&rx, WAIT),
        "the child exits, and its output ends"
    );
}

/// A synchronized update the child begins and never ends is held back, with
/// a deadline, until the view ends it.
#[test]
fn a_synchronized_update_is_held_until_it_ends() {
    let mut spawned = AlacrittyEngineFactory
        .spawn(&sh("sleep 20"), PtyGeom::new(80, 24, 0, 0), 100)
        .expect("spawn sh");
    let row = |engine: &dyn teksilo_terminal::TerminalEngine| -> String {
        engine.snapshot().cells[..20]
            .iter()
            .map(|cell| cell.ch)
            .collect()
    };
    spawned.engine.advance(b"\x1b[?2026hheld back");
    assert!(!row(spawned.engine.as_ref()).contains("held back"), "held");
    let deadline = spawned
        .engine
        .synchronized_update_deadline()
        .expect("a deadline while held");
    assert!(deadline > Instant::now());
    spawned.engine.end_synchronized_update();
    assert!(row(spawned.engine.as_ref()).contains("held back"), "shown");
    assert_eq!(spawned.engine.synchronized_update_deadline(), None);
    spawned.engine.kill();
}
