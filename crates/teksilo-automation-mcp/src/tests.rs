// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Conformance tests: drive the rmcp tool surface and the headless
//! tree-thread marshaling in-process.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use teksilo_automation::dto::{AutomationOp, AutomationReply, AutomationRequest, SettleSpec};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::headless::{HostReply, Job, spawn_tree_thread};
use crate::server::{AssertParams, AutomationServer, FindParams, InvokeParams, SnapshotParams};

/// Spawn a fresh headless tree thread and return its job sender (each test
/// gets its own tree so mutations don't bleed across parallel tests).
fn setup() -> UnboundedSender<Job> {
    let (tx, rx) = unbounded_channel::<Job>();
    let _thread = spawn_tree_thread(rx);
    tx
}

/// Send one op straight to the tree thread (bypassing rmcp) and await the
/// host reply.
async fn host_call(tx: &UnboundedSender<Job>, op: AutomationOp) -> HostReply {
    let (rtx, rrx) = oneshot::channel();
    tx.send((
        AutomationRequest {
            window_id: None,
            op,
            settle: SettleSpec::default(),
        },
        rtx,
    ))
    .expect("tree thread alive");
    rrx.await.expect("reply")
}

fn reply_ok(hr: HostReply) -> serde_json::Value {
    match hr {
        HostReply::Reply(AutomationReply::Ok { data }) => data,
        other => panic!("expected ok reply, got {:?}", debug_reply(&other)),
    }
}

fn debug_reply(hr: &HostReply) -> String {
    match hr {
        HostReply::Reply(r) => format!("{r:?}"),
        HostReply::Image { png, warnings } => {
            format!("Image({} bytes, warnings={warnings:?})", png.len())
        }
    }
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .find_map(|c| c.as_text().map(|t| t.text.clone()))
        .expect("a text content block")
}

fn image_of(result: &CallToolResult) -> Option<String> {
    result
        .content
        .iter()
        .find_map(|c| c.as_image().map(|i| i.data.clone()))
}

// ---------------------------------------------------------------------------
// rmcp tool surface
// ---------------------------------------------------------------------------

#[test]
fn tool_router_matches_catalog() {
    let router = AutomationServer::router_for_test();
    let tools = router.list_all();
    assert_eq!(
        tools.len(),
        teksilo_automation::TOOL_COUNT,
        "router must expose every catalog tool"
    );
    for entry in teksilo_automation::TOOL_CATALOG {
        assert!(
            router.has_route(entry.name),
            "missing tool route: {}",
            entry.name
        );
    }
    // Every tool advertises an input schema.
    for t in &tools {
        assert!(
            !t.input_schema.is_empty(),
            "tool {} has an empty input schema",
            t.name
        );
    }
}

// ---------------------------------------------------------------------------
// Headless tree-thread marshaling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_finds_demo_button() {
    let tx = setup();
    let data = reply_ok(host_call(&tx, AutomationOp::SnapshotTree { max_depth: None }).await);
    let nodes = data["nodes"].as_array().expect("nodes array");
    let save = nodes
        .iter()
        .find(|n| n["label"] == "Save")
        .expect("the demo 'Save' button");
    assert_eq!(save["role"], "Button");
}

#[tokio::test]
async fn list_windows_reports_single_headless_window() {
    let tx = setup();
    let data = reply_ok(host_call(&tx, AutomationOp::ListWindows).await);
    let windows = data.as_array().expect("windows array");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["label"], "main");
}

#[tokio::test]
async fn server_handler_snapshot_find_invoke_round_trip() {
    let tx = setup();
    let server = AutomationServer::new(tx);

    // snapshot_tree → a text block of JSON nodes.
    let snap = server
        .snapshot_tree(Parameters(SnapshotParams {
            window_id: None,
            max_depth: None,
        }))
        .await
        .expect("snapshot ok");
    assert_ne!(snap.is_error, Some(true));
    let json: serde_json::Value = serde_json::from_str(&text_of(&snap)).expect("valid json");
    assert!(json["nodes"].as_array().unwrap().len() >= 3);

    // find_node the Save button.
    let found = server
        .find_node(Parameters(FindParams {
            window_id: None,
            role: Some("Button".into()),
            label: Some("Save".into()),
        }))
        .await
        .expect("find ok");
    let found_json: serde_json::Value = serde_json::from_str(&text_of(&found)).unwrap();
    let node = found_json["node"].as_u64().expect("a node id");

    // invoke_action click → not an error.
    let invoked = server
        .invoke_action(Parameters(InvokeParams {
            window_id: None,
            node,
            action: "click".into(),
            settle: None,
        }))
        .await
        .expect("invoke ok");
    assert_ne!(invoked.is_error, Some(true), "{}", text_of(&invoked));
}

#[tokio::test]
async fn assert_node_failure_is_tool_error() {
    // Regression: a failed assertion must surface as an MCP tool error
    // (is_error = true), not a success with a {passed:false} body.
    //
    // This is now decided in the toolkit rather than here, so the socket
    // bridge and every direct `execute` caller inherit it too — the MCP server
    // used to re-read its own JSON payload to set the flag, and was the only
    // transport that did.
    let tx = setup();
    let server = AutomationServer::new(tx);
    let found = server
        .find_node(Parameters(FindParams {
            window_id: None,
            role: Some("Button".into()),
            label: Some("Save".into()),
        }))
        .await
        .unwrap();
    let node = serde_json::from_str::<serde_json::Value>(&text_of(&found)).unwrap()["node"]
        .as_u64()
        .expect("the Save button id");

    let fail = server
        .assert_node(Parameters(AssertParams {
            window_id: None,
            node,
            kind: "role_equals".into(),
            value: Some("Slider".into()),
            flag: None,
        }))
        .await
        .unwrap();
    assert_eq!(
        fail.is_error,
        Some(true),
        "failed assertion must be a tool error: {}",
        text_of(&fail)
    );
    // Machine-readable via structured_content, and now says *which* kind of
    // failure it was: ASSERTION_FAILED is a real node whose property did not
    // match, NOT_FOUND is a node reference that names nothing. Those are
    // different bugs.
    let sc = fail
        .structured_content
        .as_ref()
        .expect("structured_content present");
    assert_eq!(
        sc["code"],
        serde_json::json!(teksilo_automation::dto::codes::ASSERTION_FAILED)
    );
    let message = sc["message"].as_str().expect("a message");
    assert!(
        message.contains("Button") && message.contains("Slider"),
        "the message must carry actual and expected: {message}"
    );

    let pass = server
        .assert_node(Parameters(AssertParams {
            window_id: None,
            node,
            kind: "role_equals".into(),
            value: Some("Button".into()),
            flag: None,
        }))
        .await
        .unwrap();
    assert_ne!(
        pass.is_error,
        Some(true),
        "passing assertion is not an error"
    );
}

#[tokio::test]
async fn snapshot_max_depth_has_no_dangling_children() {
    // Regression: with a depth cap, no emitted node may reference a child id
    // that is absent from the `nodes` array.
    let tx = setup();
    let data = reply_ok(host_call(&tx, AutomationOp::SnapshotTree { max_depth: Some(1) }).await);
    let nodes = data["nodes"].as_array().unwrap();
    let ids: std::collections::HashSet<u64> =
        nodes.iter().filter_map(|n| n["id"].as_u64()).collect();
    for n in nodes {
        if let Some(children) = n["children"].as_array() {
            for c in children {
                let cid = c.as_u64().unwrap();
                assert!(
                    ids.contains(&cid),
                    "dangling child {cid} under node {:?}",
                    n["id"]
                );
            }
        }
    }
}

#[tokio::test]
async fn invoke_unknown_action_is_tool_error() {
    let tx = setup();
    let server = AutomationServer::new(tx);
    let res = server
        .invoke_action(Parameters(InvokeParams {
            window_id: None,
            node: 1,
            action: "frobnicate".into(),
            settle: None,
        }))
        .await
        .expect("call returns");
    assert_eq!(res.is_error, Some(true));
    assert!(text_of(&res).contains("UNKNOWN_NAME"));
}

#[tokio::test]
async fn screenshot_decodes_to_png_or_reports_no_gpu() {
    let tx = setup();
    let hr = host_call(&tx, AutomationOp::Screenshot { node: None }).await;
    match hr {
        HostReply::Image { png, .. } => {
            assert!(png.len() > 8, "non-trivial PNG");
            assert_eq!(&png[0..4], &[0x89, b'P', b'N', b'G'], "PNG magic bytes");
        }
        HostReply::Reply(AutomationReply::Err { code, .. }) => {
            // No GPU in this environment — acceptable, non-fatal.
            assert_eq!(code, teksilo_automation::dto::codes::GPU_UNAVAILABLE);
        }
        other => panic!("unexpected screenshot reply: {}", debug_reply(&other)),
    }
}

#[tokio::test]
async fn screenshot_tool_emits_image_block_when_gpu_present() {
    let tx = setup();
    let server = AutomationServer::new(tx);
    let res = server
        .screenshot(Parameters(crate::server::ScreenshotParams {
            window_id: None,
            node: None,
            settle: None,
        }))
        .await
        .expect("screenshot call");
    if res.is_error == Some(true) {
        // GPU_UNAVAILABLE — fine in headless CI.
        assert!(text_of(&res).contains("GPU_UNAVAILABLE"));
    } else {
        let b64 = image_of(&res).expect("an image content block");
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("valid base64");
        assert_eq!(&png[0..4], &[0x89, b'P', b'N', b'G']);
    }
}

// ---------------------------------------------------------------------------
// Golden screenshots (opt-in: need a real GPU)
// ---------------------------------------------------------------------------

#[cfg(feature = "golden-tests")]
#[tokio::test]
async fn golden_full_window() {
    let tx = setup();
    let hr = host_call(&tx, AutomationOp::Screenshot { node: None }).await;
    let png = match hr {
        HostReply::Image { png, .. } => png,
        HostReply::Reply(AutomationReply::Err { code, .. })
            if code == teksilo_automation::dto::codes::GPU_UNAVAILABLE =>
        {
            eprintln!("skipping golden: no GPU");
            return;
        }
        other => panic!("unexpected: {}", debug_reply(&other)),
    };
    let rgba = decode_png(&png);
    compare_or_update("full_window", &rgba);
}

#[cfg(feature = "golden-tests")]
fn decode_png(png: &[u8]) -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().expect("png info");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png frame");
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}

/// Inline per-channel pixel compare (tolerance ≤ 2). `UPDATE_GOLDENS=1`
/// (re)writes the golden instead of comparing. Goldens live under
/// `tests/goldens/` and are GPU-dependent, so they're generated per-box.
#[cfg(feature = "golden-tests")]
fn compare_or_update(name: &str, actual: &(Vec<u8>, u32, u32)) {
    use std::path::PathBuf;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.raw"));
    let header = format!("{}x{}\n", actual.1, actual.2);
    let mut payload = header.into_bytes();
    payload.extend_from_slice(&actual.0);

    if std::env::var("UPDATE_GOLDENS").is_ok() || !path.exists() {
        std::fs::write(&path, &payload).unwrap();
        eprintln!("wrote golden {name}");
        return;
    }
    let expected = std::fs::read(&path).unwrap();
    assert_eq!(
        expected.len(),
        payload.len(),
        "golden {name} size mismatch (dimensions changed?)"
    );
    let split = expected.iter().position(|&b| b == b'\n').unwrap() + 1;
    let (exp_px, act_px) = (&expected[split..], &payload[split..]);
    let diffs = exp_px
        .iter()
        .zip(act_px)
        .filter(|(a, b)| a.abs_diff(**b) > 2)
        .count();
    assert!(
        diffs == 0,
        "golden {name}: {diffs} channels differ beyond tolerance"
    );
}
