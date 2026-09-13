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

// ---------------------------------------------------------------------------
// The touch gate
// ---------------------------------------------------------------------------

/// One row of the generated arbitration-matrix table in
/// `docs/events-and-gestures.md`.
#[derive(Debug)]
struct DocumentedRow {
    frozen: String,
    members: Vec<(String, String, String)>,
    steps: Vec<(f32, f32, Option<String>)>,
}

/// Read one row of the matrix out of the documentation.
///
/// The arbitration answer belongs to `teksilo-core`, and
/// `crates/teksilo-core/tests/arbitration_matrix.rs` is where it is pinned;
/// that file also generates the table read here and fails when the two drift.
/// What this gate proves is the other half: that a script driving the app over
/// MCP can *see* the decision — the frozen action, the member list, the winner
/// — and sees the one the framework made. Restating the numbers here would make
/// a second copy of the proof that could go stale on its own.
fn documented_row(scenario: &str, pointer: &str) -> DocumentedRow {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/events-and-gestures.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let want = format!("| {scenario} | {pointer} | ");
    let line = doc
        .lines()
        .find(|l| l.starts_with(&want))
        .unwrap_or_else(|| panic!("no row '{scenario} · {pointer}' in {}", path.display()));
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    assert!(cells.len() > 5, "row has too few columns: {line}");
    let unquote = |s: &str| s.trim().trim_matches('`').to_string();

    let members = if cells[4] == "—" {
        Vec::new()
    } else {
        cells[4]
            .split(", ")
            .map(|m| {
                let (name, rest) = m.split_once(' ').expect("member name");
                let (role, state) = rest.split_once('/').expect("role/state");
                (
                    unquote(name),
                    role.trim().to_ascii_lowercase(),
                    state.trim().to_ascii_lowercase(),
                )
            })
            .collect()
    };
    let steps = cells[5]
        .split("; ")
        .map(|step| {
            let (offset, winner) = step.split_once('→').expect("an arrow");
            let offset = offset.trim().trim_start_matches('(').trim_end_matches(')');
            let (dx, dy) = offset.split_once(',').expect("an offset pair");
            let num = |s: &str| {
                s.trim()
                    .trim_start_matches('+')
                    .parse::<f32>()
                    .expect("a number")
            };
            let winner = winner.trim();
            (num(dx), num(dy), (winner != "—").then(|| unquote(winner)))
        })
        .collect();
    DocumentedRow {
        frozen: unquote(cells[3]),
        members,
        steps,
    }
}

impl Server {
    /// `tools/call` with `arguments`, returning the reply's JSON payload.
    ///
    /// Read from the text block rather than from `structuredContent`, because
    /// a tool whose payload is a JSON *array* (`query_pointers`,
    /// `get_shortcuts`) has no object to put there. The text block carries the
    /// same value for every tool.
    fn call(&mut self, id: u64, tool: &str, args: serde_json::Value) -> serde_json::Value {
        self.send(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#
        ));
        let v = self.recv(id);
        assert!(
            v["result"]["isError"] != serde_json::Value::Bool(true),
            "{tool} failed: {v}"
        );
        let text = v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} returned no text block: {v}"));
        serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("{tool} returned unparseable JSON ({e}): {text}"))
    }

    fn handshake(&mut self) {
        self.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}"#,
        );
        let _ = self.recv(1);
        self.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    }

    fn find(&mut self, id: u64, label: &str) -> u64 {
        let found = self.call(id, "find_node", serde_json::json!({ "label": label }));
        found["node"]
            .as_u64()
            .unwrap_or_else(|| panic!("no node labelled {label:?}: {found}"))
    }
}

/// The touch CI gate: a scripted multi-touch gesture, over real MCP stdio,
/// reproducing a documented arbitration answer.
///
/// Deterministic on every OS: the server's only clock is the simulated one, the
/// sequence carries its own intervals, and nothing here polls wall time. It is
/// an ordinary `cargo test`, so it rides the workspace run on all three
/// platforms with no workflow entry and no display.
#[test]
fn a_scripted_touch_sequence_reproduces_the_documented_arbitration_row() {
    let row = documented_row("list row", "touch");
    // A parser that read nothing would make every assertion below vacuous.
    assert!(
        row.steps.len() >= 2,
        "the row must bracket a threshold: {row:?}"
    );
    assert!(
        row.steps.iter().any(|(_, _, w)| w.is_none())
            && row.steps.iter().any(|(_, _, w)| w.is_some()),
        "the row must have a step before and a step after the latch: {row:?}"
    );
    assert!(
        row.members.len() >= 2,
        "the row must name competitors: {row:?}"
    );

    let mut s = Server::start();
    s.handshake();
    let row_node = s.find(2, "arbitration-row");
    let scroller_node = s.find(3, "arbitration-scroller");
    let name_of = move |node: u64| -> String {
        if node == row_node {
            "row".to_string()
        } else if node == scroller_node {
            "scroller".to_string()
        } else {
            format!("<unknown node {node}>")
        }
    };

    // Press the row's own centre, read off the tree rather than guessed.
    // `inspect_node`, not `read_node`: the layout record's bounds are always
    // there, where an AT node's are optional.
    let node = s.call(4, "inspect_node", serde_json::json!({ "node": row_node }));
    let b = &node["bounds"];
    let (px, py) = (
        b["x"].as_f64().expect("bounds.x") + b["width"].as_f64().expect("bounds.width") / 2.0,
        b["y"].as_f64().expect("bounds.y") + b["height"].as_f64().expect("bounds.height") / 2.0,
    );

    let mut steps = vec![serde_json::json!({ "phase": "down", "x": px, "y": py })];
    for (dx, dy, _) in &row.steps {
        steps.push(serde_json::json!({
            "phase": "move",
            "x": px + *dx as f64,
            "y": py + *dy as f64
        }));
    }
    let report = s.call(
        5,
        "inject_touch_sequence",
        serde_json::json!({ "steps": steps }),
    );
    let reported_steps = report["steps"].as_array().expect("steps");
    assert_eq!(reported_steps.len(), row.steps.len() + 1);

    // The press froze an action and enrolled a member list.
    let at_press = &reported_steps[0]["pointer"];
    assert_eq!(
        at_press["touch_action"].as_str(),
        Some(row.frozen.as_str()),
        "the frozen touch action must be the documented one: {report}"
    );
    let seen_members: Vec<(String, String, String)> = at_press["sequence_members"]
        .as_array()
        .expect("sequence_members")
        .iter()
        .map(|m| {
            (
                name_of(m["node"].as_u64().expect("member node")),
                m["role"].as_str().expect("role").to_ascii_lowercase(),
                m["state"].as_str().expect("state").to_ascii_lowercase(),
            )
        })
        .collect();
    assert_eq!(
        seen_members, row.members,
        "the member list must be the documented one, in order: {report}"
    );

    // Then one winner per movement step.
    for (i, (dx, dy, winner)) in row.steps.iter().enumerate() {
        let seen = reported_steps[i + 1]["pointer"]["sequence_winner"]
            .as_u64()
            .map(&name_of);
        assert_eq!(
            seen.as_deref(),
            winner.as_deref(),
            "after moving ({dx:+}, {dy:+}) the documented winner is {winner:?}: {report}"
        );
    }

    // The gesture never lifted, so the contact is still there — and
    // `query_pointers` is the tool that says so.
    let live = s.call(6, "query_pointers", serde_json::json!({}));
    let live = live
        .as_array()
        .unwrap_or_else(|| panic!("query_pointers returned no list: {live}"));
    assert_eq!(live.len(), 1, "one finger is still down: {live:?}");
    assert_eq!(live[0]["kind"].as_str(), Some("touch"));

    // And revoking it is not a lift: the contact goes away.
    let id = live[0]["pointer_id"].as_u64().expect("pointer_id");
    s.call(7, "cancel_pointer", serde_json::json!({ "pointer_id": id }));
    let after = s.call(8, "query_pointers", serde_json::json!({}));
    let after = after.as_array().expect("a list");
    assert!(after.is_empty(), "the revoked contact is gone: {after:?}");
}
