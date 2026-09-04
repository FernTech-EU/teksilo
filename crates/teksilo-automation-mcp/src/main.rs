// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `teksilo-automation-mcp` — a Model Context Protocol server that lets an
//! AI agent observe (semantic tree + screenshots) and drive (AT actions +
//! synthetic input) a Teksilo app.
//!
//! Two modes:
//! - `--headless` (default): own a [`HeadlessApp`](teksilo::app::HeadlessApp)
//!   on a dedicated thread and automate it entirely in-process — for
//!   deterministic CI / agent test-authoring with no display, GPU daemon, or
//!   OS accessibility layer.
//! - `--attach` / `--attach-pid <pid>`: drive a *live* running app through its
//!   debug-only in-app bridge (wired by the `automation` feature of
//!   `teksilo-app`), discovered from the endpoint descriptor it publishes.
//!   `--connect <endpoint> --token <uuid>` names one explicitly.
//!
//! See `docs/automation-mcp.md`.

mod connect;
mod headless;
mod server;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result, bail};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use teksilo_automation::wire::{Endpoint, EndpointFile};
use teksilo_platform::automation_transport;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return Ok(());
    }
    if args.iter().any(|a| a == "--list") {
        return list_bridges();
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("teksilo-automation-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // A value-taking flag with its value missing must be an error, not a
    // shrug: `flag_value` returns `None` either way, and falling through to
    // the default would quietly start the *demo* server while the caller
    // believes it is driving their app.
    for flag in ["--connect", "--attach-pid", "--token"] {
        if args.iter().any(|a| a == flag) && flag_value(&args, flag).is_none() {
            bail!("{flag} needs a value (see --help)");
        }
    }
    if let Some(addr) = flag_value(&args, "--connect") {
        let token = flag_value(&args, "--token")
            .or_else(|| std::env::var("TEKSILO_AUTOMATION_TOKEN").ok())
            .ok_or_else(|| {
                anyhow::anyhow!("--connect requires --token <uuid> (or $TEKSILO_AUTOMATION_TOKEN)")
            })?;
        return run_attached(Endpoint::from_address(&addr), token).await;
    }
    if let Some(pid) = flag_value(&args, "--attach-pid") {
        let pid: u32 = pid.parse().context("--attach-pid expects a process id")?;
        let found = EndpointFile::read(&EndpointFile::path_for_pid(pid)).with_context(|| {
            format!("no automation bridge published by process {pid} (is it a debug build with `install_automation_bridge_in_debug()`?)")
        })?;
        return run_attached(found.endpoint, found.token).await;
    }
    if args.iter().any(|a| a == "--attach") {
        let mut live = live_bridges();
        if live.is_empty() {
            bail!(
                "no live Teksilo automation bridge found in {}. Start a debug build that calls \
                 `install_automation_bridge_in_debug()`, or pass --connect <endpoint> --token <uuid>.",
                EndpointFile::dir().display()
            );
        }
        // Newest first, so `--attach` means "the app I just started".
        let chosen = live.remove(0);
        if !live.is_empty() {
            eprintln!(
                "teksilo-automation-mcp: {} bridges live; attaching to the newest (pid {}, {}). \
                 Use --attach-pid to pick another, or --list to see them.",
                live.len() + 1,
                chosen.pid,
                chosen.app.as_deref().unwrap_or("?")
            );
        }
        return run_attached(chosen.endpoint, chosen.token).await;
    }
    run_headless().await
}

/// Every published bridge that still answers, newest first.
///
/// A descriptor outlives its process whenever the app exits without unwinding,
/// so the listing is filtered by an actual probe and dead entries are removed
/// as they are found — otherwise `--attach` would keep picking the newest
/// corpse and every run would need a manual cleanup.
fn live_bridges() -> Vec<EndpointFile> {
    use automation_transport::Liveness;
    EndpointFile::list()
        .into_iter()
        .filter(|f| match automation_transport::probe(&f.endpoint) {
            Liveness::Live => true,
            // Listening, but not free right now: another client holds the
            // single slot, or the server is between accepts. Keep it — pruning
            // here would unregister a perfectly healthy app because somebody
            // else got there first, and `--attach-pid` would then never find it
            // again for the life of the process.
            Liveness::Busy => true,
            Liveness::Dead => {
                EndpointFile::remove(f.pid);
                false
            }
        })
        .collect()
}

/// Print every bridge this user currently has live.
fn list_bridges() -> Result<()> {
    let found = live_bridges();
    if found.is_empty() {
        println!(
            "no live Teksilo automation bridges in {}",
            EndpointFile::dir().display()
        );
        return Ok(());
    }
    for f in found {
        println!(
            "pid {:<8} {:<24} {}",
            f.pid,
            f.app.as_deref().unwrap_or("?"),
            f.endpoint
        );
    }
    Ok(())
}

/// Headless mode: a dedicated thread owns the `!Send` tree; rmcp tool
/// handlers marshal `Send` ops to it over a channel.
async fn run_headless() -> Result<()> {
    eprintln!(
        "teksilo-automation-mcp: headless mode ({} tools). Speaking MCP over stdio.",
        teksilo_automation::TOOL_COUNT
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let _thread = headless::spawn_tree_thread(rx);
    let service = server::AutomationServer::new(tx)
        .serve(stdio())
        .await
        .inspect_err(|e| eprintln!("teksilo-automation-mcp serve error: {e:?}"))?;
    service.waiting().await?;
    Ok(())
}

/// Live mode: forward ops to a running app's debug bridge.
async fn run_attached(endpoint: Endpoint, token: String) -> Result<()> {
    eprintln!("teksilo-automation-mcp: attached → {endpoint}. Speaking MCP over stdio.");
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    connect::spawn_socket_forwarder(endpoint, token, rx)?;
    let service = server::AutomationServer::new(tx)
        .serve(stdio())
        .await
        .inspect_err(|e| eprintln!("teksilo-automation-mcp serve error: {e:?}"))?;
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
        "teksilo-automation-mcp — MCP server for Teksilo app automation\n\n\
         USAGE:\n\
         \x20 teksilo-automation-mcp [--headless]      own a demo app in-process (no display, no GPU needed)\n\
         \x20 teksilo-automation-mcp --attach          drive the newest live app that published a bridge\n\
         \x20 teksilo-automation-mcp --attach-pid <pid>   …or one specific process\n\
         \x20 teksilo-automation-mcp --connect <endpoint> --token <uuid>   …or an endpoint named by hand\n\
         \x20 teksilo-automation-mcp --list            show the live bridges and exit\n\n\
         A live app publishes a bridge when a debug build calls\n\
         `install_automation_bridge_in_debug()`. Speaks the Model Context Protocol over stdio."
    );
}
