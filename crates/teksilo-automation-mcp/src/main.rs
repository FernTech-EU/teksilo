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
use clap::{ArgGroup, Parser};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use teksilo_automation::wire::{Endpoint, EndpointFile};
use teksilo_platform::automation_transport;

/// The flag surface, declared rather than resolved by precedence.
///
/// The five modes go in one [`ArgGroup`] so that asking for two at once is a
/// message instead of a silent win for whichever branch the dispatch happened
/// to test first — the same reasoning that made a value-less `--connect` an
/// error: a caller who believes they are driving their app must not end up
/// talking to the built-in demo.
#[derive(Parser, Debug)]
#[command(
    version,
    about = "MCP server for Teksilo app automation",
    long_about = "Model Context Protocol server for Teksilo app automation, spoken over stdio.\n\n\
                  A live app publishes a bridge when a debug build calls \
                  `install_automation_bridge_in_debug()`. With no mode flag at all the \
                  server owns a built-in demo app in-process instead, which needs no \
                  display and no GPU."
)]
#[command(group(
    ArgGroup::new("mode")
        .args(["headless", "attach", "attach_pid", "connect", "list"])
        .multiple(false)
))]
struct Cli {
    /// Own a demo app in-process (no display, no GPU needed). The default.
    #[arg(long)]
    headless: bool,

    /// Drive the newest live app that published a bridge.
    #[arg(long)]
    attach: bool,

    /// …or the bridge published by one specific process.
    #[arg(long, value_name = "PID")]
    attach_pid: Option<u32>,

    /// …or an endpoint named by hand, when discovery is not an option.
    ///
    /// Requires --token, or $TEKSILO_AUTOMATION_TOKEN.
    #[arg(long, value_name = "ENDPOINT")]
    connect: Option<String>,

    /// Show the live bridges and exit.
    #[arg(long)]
    list: bool,

    /// The shared secret a hand-named endpoint is opened with.
    ///
    /// A bridge's startup banner prints it as a `TEKSILO_AUTOMATION_TOKEN=…`
    /// line, so exporting that line verbatim says the same thing as passing
    /// this flag.
    //
    // Deliberately NOT `requires = "connect"`: the variable is meant to be
    // left exported, and clap counts an env-sourced value as present — so a
    // `requires` would turn every ordinary `--headless` run in that shell into
    // an error.
    #[arg(
        long,
        value_name = "UUID",
        env = "TEKSILO_AUTOMATION_TOKEN",
        hide_env_values = true
    )]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // The group's guarantee, stated once. It is why the dispatch below can be
    // a chain of `if`s whose order carries no meaning, and why `--headless`
    // needs no branch of its own: it and a bare invocation are one request.
    debug_assert!(
        [
            cli.headless,
            cli.attach,
            cli.attach_pid.is_some(),
            cli.connect.is_some(),
            cli.list,
        ]
        .iter()
        .filter(|asked| **asked)
        .count()
            <= 1,
        "the `mode` group should have rejected two modes at once"
    );

    if cli.list {
        return list_bridges();
    }
    if let Some(addr) = cli.connect {
        // `token` already carries the environment fallback (clap reads the
        // variable when the flag is absent), so the absence reported here is
        // the absence of both.
        let token = cli.token.ok_or_else(|| {
            anyhow::anyhow!("--connect requires --token <uuid> (or $TEKSILO_AUTOMATION_TOKEN)")
        })?;
        return run_attached(Endpoint::from_address(&addr), token).await;
    }
    if let Some(pid) = cli.attach_pid {
        let found = EndpointFile::read(&EndpointFile::path_for_pid(pid)).with_context(|| {
            format!("no automation bridge published by process {pid} (is it a debug build with `install_automation_bridge_in_debug()`?)")
        })?;
        return run_attached(found.endpoint, found.token).await;
    }
    if cli.attach {
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
