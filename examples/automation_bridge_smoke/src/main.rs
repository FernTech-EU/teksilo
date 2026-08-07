// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live-bridge smoke test (Phase 2).
//!
//! `build_headless` has no event loop / `AppEventProxy`, so a `#[cfg(test)]`
//! unit test can't exercise the bridge's `send_external` path. This example
//! builds a real app with `install_automation_bridge_in_debug()`, then a
//! client thread connects to the printed socket, runs
//! `snapshot_tree → invoke_action → snapshot_tree`, checks the socket is
//! `0600`, and `exit(0)`s the whole process on success (or `exit(1)` on
//! failure).
//!
//! Run (needs a display for the window):
//! `cargo run --example automation_bridge_smoke --features automation`

use teksilo::automation::dto::{AutomationOp, AutomationReply, AutomationRequest, SettleSpec};
use teksilo::prelude::*;
use teksilo::widgets::{Button, VStack};

fn main() {
    // Pin the token so the in-process client knows it without scraping stderr.
    // Respect a token already set in the environment (so `--serve` can be
    // driven by an external `--connect` client with a known token).
    let token = std::env::var("TEKSILO_AUTOMATION_TOKEN").unwrap_or_else(|_| {
        let t = "teksilo-automation-smoke-token".to_string();
        // SAFETY: set before any threads read the environment (single-threaded here).
        unsafe {
            std::env::set_var("TEKSILO_AUTOMATION_TOKEN", &t);
        }
        t
    });
    let token = token.as_str();
    // Matches the bridge's per-process `0700` dir layout (see
    // `teksilo_app::automation_bridge`): `<XDG_RUNTIME_DIR>/teksilo-automation-<pid>/sock`.
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let sock = format!("{dir}/teksilo-automation-{}/sock", std::process::id());

    // `--serve` keeps the app (and its bridge) running so an external
    // `teksilo-automation-mcp --connect <sock>` can drive it interactively.
    // Without it, an in-process client runs the smoke sequence and exits.
    let serve = std::env::args().any(|a| a == "--serve");
    if !serve {
        let sock = sock.clone();
        let token = token.to_string();
        std::thread::spawn(move || match run_smoke(&sock, &token) {
            Ok(()) => {
                eprintln!("automation_bridge_smoke: OK");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("automation_bridge_smoke: FAILED: {e}");
                std::process::exit(1);
            }
        });
    }

    TeksiloAppBuilder::new()
        .theme(intui::light())
        .install_automation_bridge_in_debug()
        .initial_window(
            WindowConfig::new()
                .title("automation bridge smoke")
                .size(400, 300)
                .root(|tree, _state| {
                    tree.add(VStack::new().spacing(8.0).child(Button::new(lit!("Save"))))
                }),
        )
        .run();
}

#[cfg(unix)]
fn run_smoke(sock: &str, token: &str) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    // Wait for the bridge socket to appear (the app is still starting up).
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut stream = loop {
        match UnixStream::connect(sock) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("could not connect to {sock}: {e}")),
        }
    };

    // Socket must be 0600.
    let mode = std::fs::metadata(sock)
        .map_err(|e| e.to_string())?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o600 {
        return Err(format!("socket mode is {mode:o}, expected 600"));
    }

    // Token handshake.
    stream
        .write_all(format!("{token}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    fn exchange(stream: &mut UnixStream, op: AutomationOp) -> Result<AutomationReply, String> {
        let req = AutomationRequest {
            window_id: None,
            op,
            settle: SettleSpec::default(),
        };
        let bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
        stream
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .map_err(|e| e.to_string())?;
        stream.write_all(&bytes).map_err(|e| e.to_string())?;
        stream.flush().map_err(|e| e.to_string())?;
        let mut len = [0u8; 4];
        stream.read_exact(&mut len).map_err(|e| e.to_string())?;
        let frame_len = u32::from_le_bytes(len) as usize;
        if frame_len > 256 * 1024 * 1024 {
            return Err("reply frame exceeds maximum size".to_string());
        }
        let mut buf = vec![0u8; frame_len];
        stream.read_exact(&mut buf).map_err(|e| e.to_string())?;
        serde_json::from_slice(&buf).map_err(|e| e.to_string())
    }

    fn data(reply: AutomationReply) -> Result<serde_json::Value, String> {
        match reply {
            AutomationReply::Ok { data } => Ok(data),
            AutomationReply::Err { code, message } => Err(format!("{code}: {message}")),
        }
    }

    // 1. snapshot_tree → find the Save button.
    let snap = data(exchange(
        &mut stream,
        AutomationOp::SnapshotTree { max_depth: None },
    )?)?;
    let node = snap["nodes"]
        .as_array()
        .ok_or("snapshot has no nodes")?
        .iter()
        .find(|n| n["label"] == "Save" && n["role"] == "Button")
        .and_then(|n| n["id"].as_u64())
        .ok_or("could not find the Save button")?;

    // 2. invoke_action click.
    data(exchange(
        &mut stream,
        AutomationOp::InvokeAction {
            node,
            action: "click".to_string(),
        },
    )?)?;

    // 3. re-snapshot still works (the tree survived the action).
    let snap2 = data(exchange(
        &mut stream,
        AutomationOp::SnapshotTree { max_depth: None },
    )?)?;
    if snap2["nodes"].as_array().map(|a| a.len()).unwrap_or(0) == 0 {
        return Err("re-snapshot returned no nodes".into());
    }

    Ok(())
}

#[cfg(not(unix))]
fn run_smoke(_sock: &str, _token: &str) -> Result<(), String> {
    Err("the live bridge needs a Unix-domain socket; not supported on this platform".into())
}
