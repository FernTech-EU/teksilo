// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `bastyde-automation-mcp` — a Model Context Protocol server that lets an
//! AI agent observe (semantic tree + screenshots) and drive (AT actions +
//! synthetic input) a Bastyde app.
//!
//! Two modes:
//! - `--headless` (default): own a [`HeadlessApp`](bastyde::app::HeadlessApp)
//!   on a dedicated thread and automate it entirely in-process — for
//!   deterministic CI / agent test-authoring with no display, GPU daemon, or
//!   OS accessibility layer.
//! - `--connect <sock> --token <uuid>`: drive a *live* running app through
//!   its debug-only in-app bridge socket (wired by the `automation` feature
//!   of `bastyde-app`).
//!
//! See `docs/automation-mcp.md`.

mod connect;
mod headless;
mod server;

#[cfg(test)]
mod tests;

use anyhow::{Result, bail};
use rmcp::ServiceExt;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(sock) = flag_value(&args, "--connect") {
        let token = flag_value(&args, "--token");
        return run_connect(sock, token).await;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return Ok(());
    }
    run_headless().await
}

/// Headless mode: a dedicated thread owns the `!Send` tree; rmcp tool
/// handlers marshal `Send` ops to it over a channel.
async fn run_headless() -> Result<()> {
    eprintln!(
        "bastyde-automation-mcp: headless mode ({} tools). Speaking MCP over stdio.",
        bastyde_automation::TOOL_COUNT
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let _thread = headless::spawn_tree_thread(rx);
    let service = server::AutomationServer::new(tx)
        .serve(stdio())
        .await
        .inspect_err(|e| eprintln!("bastyde-automation-mcp serve error: {e:?}"))?;
    service.waiting().await?;
    Ok(())
}

/// Live mode: forward ops to a running app's debug bridge socket.
async fn run_connect(sock: String, token: Option<String>) -> Result<()> {
    let token = token
        .or_else(|| std::env::var("BASTYDE_AUTOMATION_TOKEN").ok())
        .ok_or_else(|| {
            anyhow::anyhow!("--connect requires --token <uuid> (or $BASTYDE_AUTOMATION_TOKEN)")
        })?;
    eprintln!("bastyde-automation-mcp: connect mode → {sock}. Speaking MCP over stdio.");
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    connect::spawn_socket_forwarder(sock, token, rx)?;
    let service = server::AutomationServer::new(tx)
        .serve(stdio())
        .await
        .inspect_err(|e| eprintln!("bastyde-automation-mcp serve error: {e:?}"))?;
    service.waiting().await?;
    Ok(())
}

/// Return the value following `flag` in `args`, if present.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn print_usage() {
    eprintln!(
        "bastyde-automation-mcp — MCP server for Bastyde app automation\n\n\
         USAGE:\n\
         \x20 bastyde-automation-mcp [--headless]\n\
         \x20 bastyde-automation-mcp --connect <socket> --token <uuid>\n\n\
         Speaks the Model Context Protocol over stdio."
    );
}

#[allow(dead_code)]
fn _connect_stub_unused(_: String, _: Option<String>) -> Result<()> {
    bail!("unused")
}
