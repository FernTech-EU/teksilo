// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `--connect` client: forward automation ops to a *live* running app's
//! debug bridge over a Unix-domain socket.
//!
//! Framing matches the in-app bridge (`bastyde_app`'s automation bridge): a
//! token line first, then for each request a **4-byte little-endian length
//! prefix + UTF-8 JSON** frame, single connection / single in-flight. The
//! forwarder owns the socket on its own blocking thread and consumes the
//! same [`Job`] channel the headless tree thread would — so the rmcp server
//! is identical in both modes.

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;

#[cfg(not(unix))]
use crate::headless::HostReply;
use crate::headless::Job;

#[cfg(unix)]
pub fn spawn_socket_forwarder(
    sock: String,
    token: String,
    mut rx: UnboundedReceiver<Job>,
) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    use anyhow::Context;
    use bastyde_automation::dto::{AutomationOp, AutomationReply, AutomationRequest};

    use crate::headless::HostReply;

    let mut stream = UnixStream::connect(&sock)
        .with_context(|| format!("connecting to bridge socket {sock}"))?;
    // Token handshake: one line.
    stream.write_all(token.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    fn write_frame(stream: &mut UnixStream, bytes: &[u8]) -> std::io::Result<()> {
        stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
        stream.write_all(bytes)?;
        stream.flush()
    }
    // Replies can carry a base64 screenshot, so allow more than a request
    // frame — but still bound the allocation against a corrupt length prefix.
    const MAX_REPLY_FRAME: usize = 256 * 1024 * 1024;
    fn read_frame(stream: &mut UnixStream) -> std::io::Result<Vec<u8>> {
        let mut len = [0u8; 4];
        stream.read_exact(&mut len)?;
        let frame_len = u32::from_le_bytes(len) as usize;
        if frame_len > MAX_REPLY_FRAME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "reply frame exceeds maximum size",
            ));
        }
        let mut buf = vec![0u8; frame_len];
        stream.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn exchange(stream: &mut UnixStream, req: &AutomationRequest) -> Result<HostReply> {
        write_frame(stream, &serde_json::to_vec(req)?)?;
        let resp = read_frame(stream)?;
        let reply: AutomationReply = serde_json::from_slice(&resp)?;
        // The live bridge encodes a screenshot's pixels as base64 inside the
        // reply data; rehydrate it to an image block for the MCP client.
        let hr = match (&req.op, &reply) {
            (AutomationOp::Screenshot { .. }, AutomationReply::Ok { data }) => {
                match decode_screenshot(data) {
                    Some((png, warnings)) => HostReply::Image { png, warnings },
                    None => HostReply::Reply(reply),
                }
            }
            _ => HostReply::Reply(reply),
        };
        Ok(hr)
    }

    std::thread::Builder::new()
        .name("bastyde-automation-forward".into())
        .spawn(move || {
            while let Some((req, reply_tx)) = rx.blocking_recv() {
                let hr = exchange(&mut stream, &req).unwrap_or_else(|e| {
                    HostReply::Reply(AutomationReply::err("BRIDGE_IO", e.to_string()))
                });
                let _ = reply_tx.send(hr);
            }
        })?;
    Ok(())
}

#[cfg(not(unix))]
pub fn spawn_socket_forwarder(
    _sock: String,
    _token: String,
    _rx: UnboundedReceiver<Job>,
) -> Result<()> {
    let _ = std::marker::PhantomData::<HostReply>;
    anyhow::bail!(
        "--connect uses a Unix-domain socket and is not supported on this platform; use --headless"
    )
}

/// Decode the live bridge's screenshot payload (`{ png_base64, warnings }`).
#[cfg(unix)]
fn decode_screenshot(data: &serde_json::Value) -> Option<(Vec<u8>, Vec<String>)> {
    use base64::Engine;
    let b64 = data.get("png_base64")?.as_str()?;
    let png = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let warnings = data
        .get("warnings")
        .and_then(|w| w.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Some((png, warnings))
}
