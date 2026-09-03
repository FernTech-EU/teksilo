// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The attach client: forward automation ops to a *live* running app's debug
//! bridge over whatever local endpoint that platform uses.
//!
//! Framing and the token handshake come from [`teksilo_automation::wire`], and
//! the endpoint itself from
//! [`teksilo_platform::automation_transport`] — so this module is the same on
//! every platform, and there is exactly one copy of the protocol in the tree.
//!
//! The forwarder owns the connection on its own blocking thread and consumes
//! the same [`Job`] channel the headless tree thread would, so the rmcp server
//! is identical in both modes.

use anyhow::{Context, Result};
use teksilo_automation::dto::{AutomationOp, AutomationReply, AutomationRequest, ScreenshotMeta};
use teksilo_automation::wire::{self, Endpoint};
use teksilo_platform::automation_transport::{self, TransportStream};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::headless::{HostReply, Job};

/// Connect to `endpoint`, authenticate, and pump [`Job`]s over it forever.
pub fn spawn_socket_forwarder(
    endpoint: Endpoint,
    token: String,
    mut rx: UnboundedReceiver<Job>,
) -> Result<()> {
    let mut stream = automation_transport::connect(&endpoint)
        .with_context(|| format!("connecting to the bridge at {endpoint}"))?;
    wire::write_token(&mut stream, &token).context("sending the handshake token")?;

    std::thread::Builder::new()
        .name("teksilo-automation-forward".into())
        .spawn(move || {
            while let Some((req, reply_tx)) = rx.blocking_recv() {
                let hr = exchange(&mut stream, &req).unwrap_or_else(|e| {
                    HostReply::Reply(AutomationReply::err(
                        teksilo_automation::dto::codes::BRIDGE_IO,
                        e.to_string(),
                    ))
                });
                let _ = reply_tx.send(hr);
            }
        })?;
    Ok(())
}

/// One request → one reply, single in flight.
fn exchange(stream: &mut Box<dyn TransportStream>, req: &AutomationRequest) -> Result<HostReply> {
    wire::write_frame(stream, &serde_json::to_vec(req)?, wire::MAX_REQUEST_FRAME)?;
    let resp = wire::read_frame(stream, wire::MAX_REPLY_FRAME)?;
    let reply: AutomationReply = serde_json::from_slice(&resp)?;
    // The live bridge encodes a screenshot's pixels as base64 inside the reply
    // data; rehydrate it to an image block for the MCP client.
    Ok(match (&req.op, &reply) {
        (AutomationOp::Screenshot { .. }, AutomationReply::Ok { data }) => {
            match decode_screenshot(data) {
                Some((png, meta)) => HostReply::Image { png, meta },
                None => HostReply::Reply(reply),
            }
        }
        _ => HostReply::Reply(reply),
    })
}

/// Decode the live bridge's screenshot payload: a [`ScreenshotMeta`] with the
/// PNG bytes base64'd alongside it under `png_base64`.
fn decode_screenshot(data: &serde_json::Value) -> Option<(Vec<u8>, ScreenshotMeta)> {
    use base64::Engine;
    let b64 = data.get("png_base64")?.as_str()?;
    let png = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let meta = serde_json::from_value(data.clone()).ok()?;
    Some((png, meta))
}
