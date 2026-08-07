// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Debug-only in-app automation bridge.
//!
//! When a debug build calls
//! [`install_automation_bridge_in_debug`](TeksiloAppBuilderAutomationExt::install_automation_bridge_in_debug),
//! a background thread binds a private Unix-domain socket and lets
//! `teksilo-automation-mcp --connect <sock> --token <uuid>` drive the *live*
//! running app: it reads framed [`AutomationRequest`](teksilo_automation::dto::AutomationRequest)s, posts an
//! [`AutomationPayload`] (carrying a `Send` reply channel) through the
//! existing `AppEvent::External` path, and the winit main thread runs the
//! op against the real window via
//! [`execute`](teksilo_automation::execute) (or, for screenshots,
//! [`PlatformWindow::capture_offscreen`](teksilo_platform::PlatformWindow::capture_offscreen)).
//!
//! Everything that touches the socket is additionally gated on
//! `debug_assertions`: a *release* build with the `automation` feature on
//! still contains no socket, token, or bridge — the install method is the
//! identity. The framework itself stays runtime-free: this uses only
//! `std::os::unix::net` plus the existing event-proxy plumbing.

use crate::TeksiloAppBuilder;

/// Adds [`install_automation_bridge_in_debug`](Self::install_automation_bridge_in_debug)
/// to [`TeksiloAppBuilder`]. Mirrors `TeksiloAppBuilderInspectorExt`.
pub trait TeksiloAppBuilderAutomationExt {
    /// In a **debug** build: generate a per-process token, bind a private
    /// `0600` Unix socket, print its path + `TEKSILO_AUTOMATION_TOKEN=<uuid>`
    /// to stderr, and spawn the bridge thread on `on_ready`. In a **release**
    /// build (or on a non-Unix target): a no-op returning `self`.
    fn install_automation_bridge_in_debug(self) -> Self;
}

#[cfg(debug_assertions)]
impl TeksiloAppBuilderAutomationExt for TeksiloAppBuilder {
    fn install_automation_bridge_in_debug(self) -> Self {
        install(self)
    }
}

#[cfg(not(debug_assertions))]
impl TeksiloAppBuilderAutomationExt for TeksiloAppBuilder {
    fn install_automation_bridge_in_debug(self) -> Self {
        self
    }
}

// ---------------------------------------------------------------------------
// Debug-only machinery
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
use std::sync::mpsc::SyncSender;

#[cfg(debug_assertions)]
use teksilo_automation::dto::{AutomationReply, SettleSpec};

/// The `Send` payload posted from the bridge thread to the winit main
/// thread. Carries the op to run and a one-shot channel for the reply.
#[cfg(debug_assertions)]
pub struct AutomationPayload {
    /// Target window (`TeksiloWindowId` raw); `None` → focused else primary.
    pub window_id: Option<u64>,
    /// Informational sequence number (single-inflight, so unused for
    /// matching).
    pub request_id: u64,
    /// The operation to perform.
    pub op: teksilo_automation::dto::AutomationOp,
    /// Settle policy.
    pub settle: SettleSpec,
    /// Where the main thread sends the reply.
    pub reply_tx: SyncSender<AutomationReply>,
}

/// Cap on a single inbound request frame (requests are small JSON ops — no
/// images travel inbound). Bounds the `vec![0u8; len]` allocation against a
/// client that sends a bogus 4-byte length (up to ~4 GiB otherwise).
#[cfg(debug_assertions)]
const MAX_REQUEST_FRAME: usize = 16 * 1024 * 1024;

/// The per-process directory holding the bridge socket. Created `0700`, so the
/// socket is unreachable by other local users even during the brief window
/// before its own `0600` is applied (closes the bind→chmod TOCTOU).
#[cfg(debug_assertions)]
fn socket_dir() -> String {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    format!("{dir}/teksilo-automation-{}", std::process::id())
}

#[cfg(debug_assertions)]
fn socket_path() -> String {
    format!("{}/sock", socket_dir())
}

/// Clamp a settle for the LIVE bridge. The settle runs synchronously on the
/// winit main thread, so an unbounded `settle_timeout_ms` / `max_anim_frames`
/// (e.g. a `wait_for_condition` with a 30 s timeout) would freeze the running
/// app's UI for that long. Cap both so the worst-case main-thread hitch is
/// ~2 s; a longer wait should poll from the client or use the headless server
/// (which has no UI to freeze and keeps the caller's values).
#[cfg(debug_assertions)]
pub(crate) fn clamp_live_settle(settle: &SettleSpec) -> SettleSpec {
    const MAX_ANIM_FRAMES: u32 = 120;
    const MAX_TIMEOUT_MS: u64 = 2000;
    SettleSpec {
        max_anim_frames: settle.max_anim_frames.min(MAX_ANIM_FRAMES),
        settle_timeout_ms: settle.settle_timeout_ms.min(MAX_TIMEOUT_MS),
        ..*settle
    }
}

#[cfg(debug_assertions)]
fn install(builder: TeksiloAppBuilder) -> TeksiloAppBuilder {
    // A pinned `TEKSILO_AUTOMATION_TOKEN` lets a test / harness know the token
    // up-front; otherwise generate a fresh per-process one.
    let token = std::env::var("TEKSILO_AUTOMATION_TOKEN")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    let path = socket_path();
    eprintln!("teksilo-automation: bridge socket = {path}");
    eprintln!("TEKSILO_AUTOMATION_TOKEN={token}");
    eprintln!(
        "teksilo-automation: connect with `teksilo-automation-mcp --connect {path} --token {token}`"
    );
    builder.on_ready(move |proxy| {
        if let Err(e) = spawn_bridge_thread(proxy, token) {
            eprintln!("teksilo-automation: bridge failed to start: {e}");
        }
    })
}

/// Bind the socket and spawn the bridge thread. Unix-only; on other targets
/// this is a no-op (`Ok`) with an informational message — the headless MCP
/// mode works everywhere, only the live socket is Unix-gated.
#[cfg(all(debug_assertions, unix))]
pub fn spawn_bridge_thread(proxy: crate::app::AppEventProxy, token: String) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::UnixListener;

    let dir = socket_dir();
    // Stale dir from a crashed prior run (PID reuse). Then create it `0700`
    // atomically (mkdir mode) so the socket is never world-reachable.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    // Belt + suspenders (the 0700 dir already gates access).
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    std::thread::Builder::new()
        .name("teksilo-automation-bridge".into())
        .spawn(move || {
            // Remove the whole per-process dir (socket included) on thread exit.
            struct Cleanup(String);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
            let _cleanup = Cleanup(dir);

            // Single connection at a time.
            for conn in listener.incoming() {
                match conn {
                    Ok(stream) => {
                        // Connection errors just end this connection; the
                        // loop waits for the next client.
                        let _ = handle_connection(stream, &proxy, &token);
                    }
                    Err(_) => break,
                }
            }
        })?;
    Ok(())
}

#[cfg(all(debug_assertions, not(unix)))]
pub fn spawn_bridge_thread(
    _proxy: crate::app::AppEventProxy,
    _token: String,
) -> std::io::Result<()> {
    eprintln!(
        "teksilo-automation: the live bridge needs a Unix-domain socket and is unavailable on \
         this platform; use `teksilo-automation-mcp --headless` instead."
    );
    Ok(())
}

#[cfg(all(debug_assertions, unix))]
fn handle_connection(
    stream: std::os::unix::net::UnixStream,
    proxy: &crate::app::AppEventProxy,
    token: &str,
) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Read};
    use std::time::Duration;

    // Bound the token handshake: a client that connects but never sends the
    // token must not occupy the single connection slot forever.
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    // Token handshake (one line, length-bounded so a stream of bytes without a
    // newline can't exhaust memory).
    let mut token_line = String::new();
    {
        let mut limited = (&mut reader).take(512);
        limited.read_line(&mut token_line)?;
    }
    if token_line.trim() != token {
        return Ok(()); // reject — bad/missing token
    }
    // Requests can arrive sporadically over a long-lived connection, so clear
    // the read deadline now that the client is authenticated.
    reader.get_ref().set_read_timeout(None)?;

    let mut request_id: u64 = 0;
    loop {
        // 4-byte little-endian length prefix.
        let mut len = [0u8; 4];
        if reader.read_exact(&mut len).is_err() {
            break; // clean EOF — client disconnected
        }
        let frame_len = u32::from_le_bytes(len) as usize;
        if frame_len > MAX_REQUEST_FRAME {
            break; // desynced / abusive client — drop the connection
        }
        let mut buf = vec![0u8; frame_len];
        reader.read_exact(&mut buf)?;

        let req: teksilo_automation::dto::AutomationRequest = match serde_json::from_slice(&buf) {
            Ok(r) => r,
            Err(e) => {
                let reply = AutomationReply::err("BAD_REQUEST", e.to_string());
                write_frame(&mut writer, &serde_json::to_vec(&reply).unwrap())?;
                continue;
            }
        };

        request_id += 1;
        // One-slot channel: the main thread's `send` never blocks.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let payload = AutomationPayload {
            window_id: req.window_id,
            request_id,
            op: req.op,
            settle: req.settle,
            reply_tx: tx,
        };
        proxy.send_external(payload);

        let reply = rx.recv().unwrap_or_else(|_| {
            AutomationReply::err("BRIDGE_DROPPED", "the app dropped the automation reply")
        });
        write_frame(&mut writer, &serde_json::to_vec(&reply).unwrap())?;
    }
    Ok(())
}

#[cfg(all(debug_assertions, unix))]
fn write_frame(w: &mut impl std::io::Write, bytes: &[u8]) -> std::io::Result<()> {
    w.write_all(&(bytes.len() as u32).to_le_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

/// Build a screenshot reply: PNG-encode the RGBA pixels, base64 them into a
/// JSON object the `--connect` client rehydrates to an image block. Used by
/// the main-thread screenshot arm in `app.rs`.
#[cfg(debug_assertions)]
pub(crate) fn screenshot_reply(
    rgba: &[u8],
    w: u32,
    h: u32,
    warnings: Vec<String>,
) -> AutomationReply {
    use base64::Engine;
    let png = encode_png(rgba, w, h);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    AutomationReply::ok(serde_json::json!({ "png_base64": b64, "warnings": warnings }))
}

#[cfg(debug_assertions)]
fn encode_png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(rgba).expect("png data");
    }
    buf
}

#[cfg(all(debug_assertions, test))]
mod tests {
    use super::clamp_live_settle;
    use teksilo_automation::dto::SettleSpec;

    #[test]
    fn live_settle_is_clamped() {
        // A long wait/settle must be capped so it can't freeze the main-thread UI.
        let capped = clamp_live_settle(&SettleSpec {
            clock_millis: 25,
            max_anim_frames: 10_000,
            layout_after: true,
            settle_timeout_ms: 30_000,
        });
        assert_eq!(capped.max_anim_frames, 120);
        assert_eq!(capped.settle_timeout_ms, 2000);
        assert_eq!(capped.clock_millis, 25, "non-bound fields pass through");

        // Values already under the cap are untouched.
        let d = SettleSpec::default();
        let small = clamp_live_settle(&d);
        assert_eq!(small.settle_timeout_ms, d.settle_timeout_ms);
        assert_eq!(small.max_anim_frames, d.max_anim_frames);
    }
}
