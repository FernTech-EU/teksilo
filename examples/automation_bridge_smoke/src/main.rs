// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Live-bridge smoke test.
//!
//! `build_headless` has no event loop / `AppEventProxy`, so a `#[cfg(test)]`
//! unit test can't exercise the bridge's `send_external` path. This example
//! builds a real app with `install_automation_bridge_in_debug()`, then a client
//! thread discovers the published endpoint, connects, runs
//! `snapshot_tree → invoke_action → snapshot_tree → screenshot`, and `exit(0)`s
//! the whole process on success (or `exit(1)` on failure).
//!
//! It is platform-agnostic: discovery goes through [`EndpointFile`] and the
//! connection through `teksilo_platform::automation_transport`, so the same
//! code drives a Unix socket and a Windows named pipe.
//!
//! Run (needs a display for the window):
//! `cargo run -p automation_bridge_smoke` (its own manifest already enables the
//! `automation` feature; it is a workspace package, not a cargo example).
//! With `--serve` it keeps the app alive so an external
//! `teksilo-automation-mcp --attach` can drive it interactively.

use std::time::{Duration, Instant};

use teksilo::automation::dto::{AutomationOp, AutomationReply, AutomationRequest, SettleSpec};
use teksilo::automation::wire::{self, EndpointFile};
use teksilo::prelude::*;
use teksilo::widgets::{Button, VStack};
use teksilo_platform::automation_transport::{self, TransportStream};

fn main() {
    // Pin the token so the in-process client knows it without scraping stderr.
    // Respect one already in the environment (so `--serve` can be driven by an
    // external client with a known token).
    if std::env::var("TEKSILO_AUTOMATION_TOKEN").is_err() {
        // SAFETY: set before any threads read the environment (single-threaded here).
        unsafe {
            std::env::set_var("TEKSILO_AUTOMATION_TOKEN", "teksilo-automation-smoke-token");
        }
    }

    // `--serve` keeps the app (and its bridge) running so an external
    // `teksilo-automation-mcp --attach` can drive it interactively. Without it,
    // an in-process client runs the smoke sequence and exits.
    let serve = std::env::args().any(|a| a == "--serve");
    if !serve {
        std::thread::spawn(|| match run_smoke() {
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

/// Wait for this process's own endpoint descriptor, then drive the bridge.
fn run_smoke() -> Result<(), String> {
    let pid = std::process::id();
    let deadline = Instant::now() + Duration::from_secs(20);

    // The app is still starting: poll for the descriptor it publishes.
    let descriptor = loop {
        match EndpointFile::read(&EndpointFile::path_for_pid(pid)) {
            Ok(f) => break f,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("no endpoint descriptor for pid {pid}: {e}")),
        }
    };
    check_descriptor_is_private(&descriptor)?;

    let mut stream = loop {
        match automation_transport::connect(&descriptor.endpoint) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("could not connect to {}: {e}", descriptor.endpoint)),
        }
    };

    wire::write_token(&mut stream, &descriptor.token).map_err(|e| e.to_string())?;

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

    // 4. screenshot: the live-window capture path, including the BGRA swizzle
    //    that only ever executes on a Windows or macOS surface format. A host
    //    with no usable GPU reports that cleanly instead of failing.
    match exchange(&mut stream, AutomationOp::Screenshot { node: None })? {
        AutomationReply::Ok { data } => {
            let b64 = data["png_base64"]
                .as_str()
                .ok_or("screenshot reply carries no png_base64")?;
            if b64.is_empty() {
                return Err("screenshot returned an empty image".into());
            }
            let (w, h) = (data["width"].as_u64(), data["height"].as_u64());
            if w.unwrap_or(0) == 0 || h.unwrap_or(0) == 0 {
                return Err(format!("screenshot reports a degenerate size {w:?}×{h:?}"));
            }
            if data["scale"].as_f64().unwrap_or(0.0) <= 0.0 {
                return Err("screenshot reports no device scale factor".into());
            }
            eprintln!(
                "automation_bridge_smoke: captured {}×{} @ {}×",
                w.unwrap(),
                h.unwrap(),
                data["scale"]
            );
        }
        // Both GPU codes are a skip, not a failure. `GPU_READBACK_FAILED` is
        // just as expected as `GPU_UNAVAILABLE` on the GPU-less runners this
        // smoke is meant to pass on — a software adapter that opens and then
        // cannot map its readback buffer takes that branch — and treating it as
        // an error would fail the `test-automation` job for the exact hosts it
        // exists to cover.
        AutomationReply::Err { code, message }
            if code == teksilo::automation::dto::codes::GPU_UNAVAILABLE
                || code == teksilo::automation::dto::codes::GPU_READBACK_FAILED =>
        {
            eprintln!(
                "automation_bridge_smoke: no usable GPU for the screenshot ({code}: {message}) — skipped"
            );
        }
        AutomationReply::Err { code, message } => {
            return Err(format!("screenshot failed: {code}: {message}"));
        }
    }

    Ok(())
}

/// The descriptor carries the token, so it must not be world-readable.
fn check_descriptor_is_private(descriptor: &EndpointFile) -> Result<(), String> {
    let path = EndpointFile::path_for_pid(descriptor.pid);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o600 {
            return Err(format!("descriptor mode is {mode:o}, expected 600"));
        }
        // The Unix socket itself must be owner-only too.
        let sock_mode = std::fs::metadata(&descriptor.endpoint.address)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode()
            & 0o777;
        if sock_mode != 0o600 {
            return Err(format!("socket mode is {sock_mode:o}, expected 600"));
        }
    }
    #[cfg(not(unix))]
    {
        // Windows: the descriptor lives under %LOCALAPPDATA% (per-user by ACL)
        // and the pipe carries its own owner-only DACL, checked where it is
        // built. Here we only assert the descriptor is actually there.
        if !path.exists() {
            return Err(format!("descriptor {} vanished", path.display()));
        }
    }
    Ok(())
}

fn exchange(
    stream: &mut Box<dyn TransportStream>,
    op: AutomationOp,
) -> Result<AutomationReply, String> {
    let req = AutomationRequest {
        window_id: None,
        op,
        settle: SettleSpec::default(),
    };
    let bytes = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
    wire::write_frame(stream, &bytes, wire::MAX_REQUEST_FRAME).map_err(|e| e.to_string())?;
    let buf = wire::read_frame(stream, wire::MAX_REPLY_FRAME).map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

fn data(reply: AutomationReply) -> Result<serde_json::Value, String> {
    match reply {
        AutomationReply::Ok { data } => Ok(data),
        AutomationReply::Err { code, message } => Err(format!("{code}: {message}")),
    }
}
