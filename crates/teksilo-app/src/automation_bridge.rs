// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Debug-only in-app automation bridge.
//!
//! When a debug build calls
//! [`install_automation_bridge_in_debug`](TeksiloAppBuilderAutomationExt::install_automation_bridge_in_debug),
//! a background thread binds this platform's private automation endpoint — a
//! Unix-domain socket, or a named pipe on Windows — and lets
//! `teksilo-automation-mcp --attach` drive the *live* running app: it reads
//! framed [`AutomationRequest`](teksilo_automation::dto::AutomationRequest)s,
//! posts an [`AutomationPayload`] (carrying a `Send` reply channel) through the
//! existing `AppEvent::External` path, and the winit main thread runs the op
//! against the real window via [`execute`](teksilo_automation::execute) (or,
//! for screenshots,
//! [`PlatformWindow::capture_offscreen`](teksilo_platform::PlatformWindow::capture_offscreen)).
//!
//! This module owns *policy* only. The bytes on the wire come from
//! [`teksilo_automation::wire`] and the OS endpoint from
//! [`teksilo_platform::automation_transport`], so nothing here is
//! platform-conditional.
//!
//! Everything is additionally gated on `debug_assertions`: a *release* build
//! with the `automation` feature on still contains no endpoint, token, or
//! bridge — the install method is the identity.

use crate::TeksiloAppBuilder;

/// Adds [`install_automation_bridge_in_debug`](Self::install_automation_bridge_in_debug)
/// to [`TeksiloAppBuilder`]. Mirrors `TeksiloAppBuilderInspectorExt`.
pub trait TeksiloAppBuilderAutomationExt {
    /// In a **debug** build: generate a per-process token, then on `on_ready`
    /// bind a private endpoint, publish an
    /// [`EndpointFile`](teksilo_automation::wire::EndpointFile) describing it,
    /// print how to attach, and spawn the bridge thread. In a **release**
    /// build: a no-op returning `self`.
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
use std::time::Duration;

#[cfg(debug_assertions)]
use teksilo_automation::dto::{AutomationReply, SettleSpec, codes};
#[cfg(debug_assertions)]
use teksilo_automation::wire::{self, EndpointFile};
#[cfg(debug_assertions)]
use teksilo_platform::automation_transport::{self, TransportStream};

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

/// How long the token handshake may take before the connection is dropped, so
/// a peer that connects and says nothing cannot hold the single slot.
#[cfg(debug_assertions)]
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the bridge waits for the UI thread to answer one request.
///
/// This is the bridge's liveness guarantee, and it is not optional. winit's
/// macOS backend queues user events onto the run loop in *default mode only*,
/// so while a native panel is up (`NSOpenPanel`, `NSMenu` tracking) the posted
/// `AppEvent::External` is not delivered at all. Without a deadline the bridge
/// thread would block on `recv` forever, the client would block on its read
/// forever, and — because the bridge serves one connection at a time — the slot
/// would be held for the rest of the process's life with no way to recover
/// short of killing the app. The same shape is possible on Windows inside a
/// modal `WM_ENTERSIZEMOVE` loop.
///
/// Generous relative to the ~2 s settle clamp below, so this only fires when
/// the main thread genuinely is not running our code.
#[cfg(debug_assertions)]
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);

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
    builder.on_ready(move |proxy| {
        if let Err(e) = spawn_bridge_thread(proxy, token) {
            eprintln!("teksilo-automation: bridge failed to start: {e}");
        }
    })
}

/// Bind the endpoint, publish its descriptor, and spawn the bridge thread.
#[cfg(debug_assertions)]
pub fn spawn_bridge_thread(proxy: crate::app::AppEventProxy, token: String) -> std::io::Result<()> {
    let pid = std::process::id();
    let bound = automation_transport::bind(pid)?;
    let endpoint = bound.endpoint.clone();

    // Publish *before* announcing and before accepting: a client acts on the
    // descriptor the moment it can read it, and `--attach` does not retry.
    let descriptor = EndpointFile::new(endpoint.clone(), token.clone());
    let descriptor_path = descriptor.write()?;

    let announce_token = token.clone();
    let mut listener = bound.listener;
    std::thread::Builder::new()
        .name("teksilo-automation-bridge".into())
        .spawn(move || {
            // Remove the descriptor on thread exit; dropping `listener`
            // releases the OS endpoint (and, on Unix, its directory).
            struct Cleanup(u32);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    EndpointFile::remove(self.0);
                }
            }
            let _cleanup = Cleanup(pid);

            // One connection at a time.
            loop {
                match listener.accept() {
                    // Connection errors end that connection only; the loop
                    // waits for the next client.
                    Ok(stream) => {
                        let _ = handle_connection(stream, &proxy, &token);
                    }
                    Err(_) => break,
                }
            }
        })?;

    // Announce only now — after the bind, after the descriptor exists, and
    // after the accept thread exists. A client acts on this the moment it reads
    // it, so every failure has to happen first: announcing at builder time (as
    // this once did) left every client racing the bind, and a failed `spawn`
    // would leave an endpoint that accepts a connection nobody will ever serve.
    eprintln!("teksilo-automation: bridge endpoint = {endpoint}");
    eprintln!(
        "teksilo-automation: descriptor = {}",
        descriptor_path.display()
    );
    eprintln!("TEKSILO_AUTOMATION_TOKEN={announce_token}");
    eprintln!(
        "teksilo-automation: attach with `teksilo-automation-mcp --attach-pid {pid}` \
         (or --connect {endpoint} --token {announce_token})"
    );
    Ok(())
}

/// Serve one connected client until it disconnects.
#[cfg(debug_assertions)]
fn handle_connection(
    mut stream: Box<dyn TransportStream>,
    proxy: &crate::app::AppEventProxy,
    token: &str,
) -> std::io::Result<()> {
    // Bound the handshake: a peer that connects but never sends the token must
    // not occupy the single connection slot forever.
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let offered = wire::read_token(&mut stream)?;
    if !wire::token_matches(token, &offered) {
        return Ok(()); // reject — bad/missing token
    }
    // Requests arrive sporadically over a long-lived connection, so clear the
    // deadline now that the peer is authenticated.
    stream.set_read_timeout(None)?;

    let mut request_id: u64 = 0;
    loop {
        let buf = match wire::read_frame(&mut stream, wire::MAX_REQUEST_FRAME) {
            Ok(b) => b,
            // Clean EOF (client disconnected) or a desynced / abusive peer:
            // either way this conversation is over.
            Err(_) => break,
        };

        let req: teksilo_automation::dto::AutomationRequest = match serde_json::from_slice(&buf) {
            Ok(r) => r,
            Err(e) => {
                let reply = AutomationReply::err(codes::BAD_REQUEST, e.to_string());
                write_reply(&mut stream, &reply)?;
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

        let reply = match rx.recv_timeout(REPLY_TIMEOUT) {
            Ok(reply) => reply,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => AutomationReply::err(
                codes::BRIDGE_TIMEOUT,
                format!(
                    "the app's UI thread did not answer within {}s — it is probably inside a \
                     native modal loop (file dialog, menu, window drag)",
                    REPLY_TIMEOUT.as_secs()
                ),
            ),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => AutomationReply::err(
                codes::BRIDGE_DROPPED,
                "the app dropped the automation reply",
            ),
        };
        write_reply(&mut stream, &reply)?;
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn write_reply(
    stream: &mut Box<dyn TransportStream>,
    reply: &AutomationReply,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(reply)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    match wire::write_frame(stream, &bytes, wire::MAX_REPLY_FRAME) {
        Ok(()) => Ok(()),
        // A reply too large to frame (an enormous screenshot) must not desync
        // the stream: send a typed error of a size that always fits instead.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {
            let fallback = AutomationReply::err(codes::BRIDGE_IO, e.to_string());
            let bytes = serde_json::to_vec(&fallback).unwrap_or_default();
            wire::write_frame(stream, &bytes, wire::MAX_REPLY_FRAME)
        }
        Err(e) => Err(e),
    }
}

/// Build a screenshot reply: PNG-encode the RGBA pixels, base64 them into a
/// JSON object the client rehydrates to an image block. Used by the
/// main-thread screenshot arm in `app.rs`.
///
/// `w`/`h` are **physical** pixels and `scale` is the window's device scale
/// factor, both carried through to the client — see
/// [`ScreenshotMeta`](teksilo_automation::dto::ScreenshotMeta) for why pixels
/// without a scale are not enough to act on.
#[cfg(debug_assertions)]
pub(crate) fn screenshot_reply(
    rgba: &[u8],
    w: u32,
    h: u32,
    scale: f32,
    warnings: Vec<String>,
) -> AutomationReply {
    use base64::Engine;
    let png = encode_png(rgba, w, h);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let meta = teksilo_automation::dto::ScreenshotMeta {
        width: w,
        height: h,
        scale,
        warnings,
    };
    let mut data = serde_json::to_value(&meta).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(obj) = data.as_object_mut() {
        obj.insert("png_base64".to_string(), serde_json::Value::String(b64));
    }
    AutomationReply::ok(data)
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
    use super::*;

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

    #[test]
    fn the_reply_deadline_outlives_the_settle_clamp() {
        // If these ever crossed, a legitimate long settle would be reported as
        // a wedged UI thread. The reply deadline must stay the outer bound.
        let clamped_ms = clamp_live_settle(&SettleSpec {
            settle_timeout_ms: u64::MAX,
            ..SettleSpec::default()
        })
        .settle_timeout_ms;
        assert!(
            REPLY_TIMEOUT.as_millis() as u64 > clamped_ms * 2,
            "REPLY_TIMEOUT ({REPLY_TIMEOUT:?}) must comfortably exceed the {clamped_ms}ms settle clamp"
        );
    }

    #[test]
    fn screenshot_reply_carries_pixels_and_their_scale() {
        // 2×2 opaque red.
        let rgba = [255u8, 0, 0, 255].repeat(4);
        let reply = screenshot_reply(&rgba, 2, 2, 2.0, vec!["webview_hole_possible".into()]);
        let AutomationReply::Ok { data } = reply else {
            panic!("expected ok");
        };
        assert_eq!(data["width"], 2);
        assert_eq!(data["height"], 2);
        assert_eq!(data["scale"], 2.0);
        assert_eq!(data["warnings"][0], "webview_hole_possible");
        let b64 = data["png_base64"].as_str().expect("png");
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("decodes");
        assert_eq!(&png[1..4], b"PNG", "a real PNG signature");
    }
}
