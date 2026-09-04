// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end smoke test for `--headless` over real MCP stdio.
//!
//! Spawns the actual binary and speaks JSON-RPC to it, so this covers the
//! things a unit test cannot: argument parsing, the rmcp stdio transport, the
//! `!Send` tree thread, and the DTO round-trip through serde.
//!
//! It is an ordinary `cargo test`, needs no display and no GPU, and therefore
//! runs on every OS in the CI matrix — which is the point. The automation
//! surface was portable long before anyone had checked that it was, because
//! nothing ever ran it off Linux.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// Give the whole exchange a bound so a hung server fails the test instead of
/// hanging the job.
const DEADLINE: Duration = Duration::from_secs(120);

struct Server {
    child: Child,
    /// Lines pumped off the child's stdout by a reader thread.
    ///
    /// The read has to happen on *another* thread for `DEADLINE` to mean
    /// anything: a `read_line` straight on the pipe blocks forever when the
    /// server wedges, so checking the clock around it can never fire and the
    /// bound this test advertises would not exist. `recv_timeout` is the bound.
    lines: Receiver<String>,
}

impl Server {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_teksilo-automation-mcp"))
            .arg("--headless")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn teksilo-automation-mcp");
        let stdout = child.stdout.take().expect("stdout");
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break; // the test is done with us
                }
            }
        });
        Self { child, lines }
    }

    fn send(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{line}").expect("write request");
        stdin.flush().expect("flush");
    }

    /// Read lines until one carries `"id":<id>`, then return it parsed.
    ///
    /// Replies are matched by id rather than by arrival order: the server
    /// answers concurrently, and an earlier run of this test saw `id:4` land
    /// before `id:3`.
    fn recv(&mut self, id: u64) -> serde_json::Value {
        let started = Instant::now();
        loop {
            let remaining = DEADLINE
                .checked_sub(started.elapsed())
                .unwrap_or(Duration::ZERO);
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    panic!("no reply to id {id} within {DEADLINE:?}")
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("server closed stdout before answering id {id}")
                }
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue; // not JSON — ignore
            };
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                return v;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Closing stdin is the graceful stop; kill is the backstop.
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn headless_server_speaks_mcp_and_drives_the_tree() {
    let mut s = Server::start();

    s.send(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}"#,
    );
    let init = s.recv(1);
    assert!(
        init["result"]["protocolVersion"].is_string(),
        "initialize did not answer with a protocol version: {init}"
    );

    s.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    // The catalog is the contract: the count must match what the toolkit
    // declares, so adding a tool without updating `TOOL_CATALOG` fails here.
    s.send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#);
    let tools = s.recv(2);
    let listed = tools["result"]["tools"]
        .as_array()
        .expect("tools/list returned no array")
        .len();
    assert_eq!(
        listed,
        teksilo_automation::TOOL_COUNT,
        "tools/list advertised {listed} tools, the catalog declares {}",
        teksilo_automation::TOOL_COUNT
    );

    // A real semantic snapshot of the demo tree.
    s.send(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"snapshot_tree","arguments":{}}}"#);
    let snap = s.recv(3);
    let nodes = snap["result"]["structuredContent"]["nodes"]
        .as_array()
        .expect("snapshot_tree returned no nodes");
    assert!(
        nodes
            .iter()
            .any(|n| n["role"] == "Button" && n["label"] == "Save"),
        "the demo tree should expose a Save button: {snap}"
    );

    // Drive it: click Save, then confirm the tree still answers.
    let save_id = nodes
        .iter()
        .find(|n| n["role"] == "Button" && n["label"] == "Save")
        .and_then(|n| n["id"].as_u64())
        .expect("Save button id");
    s.send(&format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"invoke_action","arguments":{{"node":{save_id},"action":"click"}}}}}}"#
    ));
    let clicked = s.recv(4);
    assert!(
        clicked["result"]["isError"] != serde_json::Value::Bool(true),
        "invoke_action failed: {clicked}"
    );
}

#[test]
fn headless_screenshot_is_a_png_or_a_typed_gpu_error() {
    // The one tool whose result depends on the host. Either it produces a real
    // image, or it says why not with a code a caller can branch on — never a
    // panic, and never a success carrying nothing. A GPU-less runner takes the
    // second branch, which is why this asserts a disjunction rather than an
    // image.
    let mut s = Server::start();
    s.send(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}"#,
    );
    let _ = s.recv(1);
    s.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    s.send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"screenshot","arguments":{}}}"#);
    let shot = s.recv(2);
    let content = shot["result"]["content"]
        .as_array()
        .expect("screenshot returned no content");

    if let Some(img) = content.iter().find(|c| c["type"] == "image") {
        let b64 = img["data"].as_str().expect("image block carries no data");
        assert!(!b64.is_empty(), "image block is empty");
        let meta = &shot["result"]["structuredContent"];
        assert!(
            meta["width"].as_u64().unwrap_or(0) > 0 && meta["height"].as_u64().unwrap_or(0) > 0,
            "screenshot reported a degenerate size: {meta}"
        );
        assert!(
            meta["scale"].as_f64().unwrap_or(0.0) > 0.0,
            "screenshot reported no device scale factor: {meta}"
        );
    } else {
        let code = shot["result"]["structuredContent"]["code"]
            .as_str()
            .unwrap_or_default();
        assert!(
            code == teksilo_automation::codes::GPU_UNAVAILABLE
                || code == teksilo_automation::codes::GPU_READBACK_FAILED,
            "expected an image or a typed GPU error, got: {shot}"
        );
    }
}
