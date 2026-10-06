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
        HostReply::Image { png, meta } => {
            format!(
                "Image({} bytes, {}x{} @ {}x, warnings={:?})",
                png.len(),
                meta.width,
                meta.height,
                meta.scale,
                meta.warnings
            )
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
            // No GPU in this environment — acceptable, non-fatal, unless the
            // run asked for an adapter.
            assert_eq!(code, teksilo_automation::dto::codes::GPU_UNAVAILABLE);
            assert!(
                !teksilo_render::test_support::adapter_required(),
                "an adapter was required, but the screenshot reported none"
            );
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
        // GPU_UNAVAILABLE — fine on a host without a GPU, unless the run
        // asked for an adapter.
        assert!(text_of(&res).contains("GPU_UNAVAILABLE"));
        assert!(
            !teksilo_render::test_support::adapter_required(),
            "an adapter was required, but the screenshot reported none"
        );
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
            assert!(
                !teksilo_render::test_support::adapter_required(),
                "an adapter was required, but the screenshot reported none"
            );
            eprintln!("skipping golden: no GPU");
            return;
        }
        other => panic!("unexpected: {}", debug_reply(&other)),
    };
    let rgba = decode_png(&png);
    compare_or_update("full_window", &rgba);
}

/// A PNG's pixels and size. The headless server writes 8-bit RGBA.
fn decode_png(png: &[u8]) -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().expect("png info");
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .expect("a frame that fits in memory")
    ];
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

// ---------------------------------------------------------------------------
// The live-image fixture (spec I.6 to I.9)
// ---------------------------------------------------------------------------

use teksilo_automation::dto::{
    LiveImageMapReply, LiveImageShot, LiveImageStatsReply, PointerAction, PointerButtonDto,
    PointerKindDto, ScreenshotMeta,
};

/// The node labelled `label` in a fresh snapshot.
async fn labelled(tx: &UnboundedSender<Job>, label: &str) -> serde_json::Value {
    let data = reply_ok(host_call(tx, AutomationOp::SnapshotTree { max_depth: None }).await);
    data["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .find(|n| n["label"] == label)
        .unwrap_or_else(|| panic!("a node labelled {label:?}"))
        .clone()
}

fn id_of(node: &serde_json::Value) -> u64 {
    node["id"].as_u64().expect("a node id")
}

/// The last event the fixture recorded, in its description.
async fn fixture_log(tx: &UnboundedSender<Job>, node: u64) -> String {
    let data = reply_ok(host_call(tx, AutomationOp::ReadNode { node }).await);
    data["description"]
        .as_str()
        .expect("the fixture's description")
        .to_owned()
}

fn at_source(node: u64, pixel: [u32; 2], action: PointerAction) -> AutomationOp {
    AutomationOp::InjectPointer {
        node: Some(node),
        x: None,
        y: None,
        source: Some(pixel),
        action,
        button: PointerButtonDto::Primary,
        kind: PointerKindDto::Mouse,
        pointer_id: None,
        pressure: None,
        tilt: None,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        command: false,
    }
}

async fn stats(tx: &UnboundedSender<Job>, node: u64) -> LiveImageStatsReply {
    let data = reply_ok(host_call(tx, AutomationOp::LiveImageStats { node }).await);
    serde_json::from_value(data).expect("a stats reply")
}

async fn click(tx: &UnboundedSender<Job>, label: &str) {
    let button = labelled(tx, label).await;
    reply_ok(
        host_call(
            tx,
            AutomationOp::InvokeAction {
                node: id_of(&button),
                action: "click".to_owned(),
            },
        )
        .await,
    );
}

/// A screenshot of `node`: its pixels and metadata, or `None` on a host with
/// no adapter, which is acceptable unless the run asked for one.
async fn shot_of(tx: &UnboundedSender<Job>, node: u64) -> Option<(Vec<u8>, u32, ScreenshotMeta)> {
    match host_call(tx, AutomationOp::Screenshot { node: Some(node) }).await {
        HostReply::Image { png, meta } => {
            let (rgba, w, h) = decode_png(&png);
            assert_eq!((w, h), (meta.width, meta.height));
            Some((rgba, w, meta))
        }
        HostReply::Reply(AutomationReply::Err { code, .. })
            if code == teksilo_automation::dto::codes::GPU_UNAVAILABLE =>
        {
            assert!(
                !teksilo_render::test_support::adapter_required(),
                "an adapter was required, but the screenshot reported none"
            );
            None
        }
        other => panic!("unexpected screenshot reply: {}", debug_reply(&other)),
    }
}

/// The fixture's one picture in a screenshot, checked pixel by pixel against
/// the commit the screenshot says it drew: each pixel names its position and
/// its generation (`div 8` undoes the encoding through the sRGB round trip).
fn check_fixture_pixels(rgba: &[u8], width: u32, shot: &LiveImageShot) {
    assert!(!shot.deferred, "{shot:?}");
    let [x0, y0, w, h] = shot.rect;
    assert_eq!((w, h), (96, 64), "drawn one to one: {shot:?}");
    assert!(
        ((y0 + h) * width) as usize * 4 <= rgba.len() && x0 + w <= width,
        "inside the image: {shot:?}"
    );
    let expect_b = (shot.generation % 32) as u8;
    for y in 0..h {
        for x in 0..w {
            let i = (((y0 + y) * width + x0 + x) * 4) as usize;
            let px = &rgba[i..i + 4];
            assert_eq!(
                (px[0] / 8, px[1] / 8, px[2] / 8),
                ((x % 32) as u8, (y % 32) as u8, expect_b),
                "pixel ({x}, {y}) of generation {}: {px:?}",
                shot.generation
            );
        }
    }
}

const FIXTURE_AIMS: [[u32; 2]; 5] = [[0, 0], [95, 0], [0, 63], [95, 63], [48, 32]];

/// I.6. The fixture is in the demo, laid out inside the window at its 96 x 64
/// source size, and every input reaches it where its source pixels are: a
/// press at a source pixel arrives at that pixel's centre, a key reaches it
/// once focused. Its stats show the one commit built in, and no window half:
/// there is no renderer before the first screenshot, and no window.
#[tokio::test]
async fn i6_the_fixture_takes_input_at_its_source_pixels() {
    let tx = setup();
    let picture = labelled(&tx, "live-image-fixture").await;
    let node = id_of(&picture);
    let b = &picture["bounds"];
    let (x, y, w, h) = (
        b["x"].as_f64().unwrap(),
        b["y"].as_f64().unwrap(),
        b["width"].as_f64().unwrap(),
        b["height"].as_f64().unwrap(),
    );
    assert_eq!((w, h), (96.0, 64.0), "{picture}");
    assert!(
        x >= 0.0 && y >= 0.0 && x + w <= 800.0 && y + h <= 600.0,
        "inside the headless window: {picture}"
    );

    let data = reply_ok(
        host_call(
            &tx,
            AutomationOp::LiveImageMap {
                node,
                source: None,
                source_rect: None,
                window: None,
            },
        )
        .await,
    );
    let map: LiveImageMapReply = serde_json::from_value(data).expect("a map");
    assert_eq!(map.source_size, [96, 64]);
    assert_eq!((map.content.width, map.content.height), (96.0, 64.0));

    // AC20: each press reaches the handler at a local point that maps back
    // to the pixel aimed at. The box is not on a whole pixel, and the picture
    // in it is snapped to one, so the point is not the box's (x + ½, y + ½).
    for pixel in FIXTURE_AIMS {
        for (action, phase) in [
            (PointerAction::Down, "pointer_down"),
            (PointerAction::Up, "pointer_up"),
        ] {
            assert!(
                matches!(
                    host_call(&tx, at_source(node, pixel, action)).await,
                    HostReply::Reply(AutomationReply::Ok { .. })
                ),
                "{pixel:?} {phase}"
            );
            let log = fixture_log(&tx, node).await;
            let mut fields = log.split(' ');
            assert_eq!(fields.next(), Some(phase), "{log}");
            let local: Vec<f32> = fields.map(|v| v.parse().expect("a coordinate")).collect();
            let window = [x as f32 + local[0], y as f32 + local[1]];
            let data = reply_ok(
                host_call(
                    &tx,
                    AutomationOp::LiveImageMap {
                        node,
                        source: None,
                        source_rect: None,
                        window: Some(window),
                    },
                )
                .await,
            );
            let back: LiveImageMapReply = serde_json::from_value(data).expect("a map");
            assert_eq!(back.pixel, Some(Some(pixel)), "{phase} at {log}");
            // And at the centre of the pixel, where the map says it is.
            let data = reply_ok(
                host_call(
                    &tx,
                    AutomationOp::LiveImageMap {
                        node,
                        source: Some(pixel),
                        source_rect: None,
                        window: None,
                    },
                )
                .await,
            );
            let there: LiveImageMapReply = serde_json::from_value(data).expect("a map");
            let centre = there.source_point.expect("asked for");
            assert!(
                (centre[0] - window[0]).abs() < 1e-3 && (centre[1] - window[1]).abs() < 1e-3,
                "{phase} {pixel:?}: at {window:?}, its centre at {centre:?}"
            );
        }
    }

    reply_ok(host_call(&tx, AutomationOp::FocusNode { node }).await);
    reply_ok(
        host_call(
            &tx,
            AutomationOp::InjectKey {
                key: "f5".to_owned(),
                text: None,
                phase: Default::default(),
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
                command: false,
            },
        )
        .await,
    );
    assert_eq!(
        fixture_log(&tx, node).await,
        "key_down F5",
        "every key reaches it"
    );

    let stats = stats(&tx, node).await;
    assert_eq!(stats.source.generation, 1);
    assert_eq!((stats.textures, stats.wakes), (None, None));
}

/// I.7. A screenshot of the fixture records the commit it drew and where, and
/// its pixels are that commit's, each at its position.
#[tokio::test]
async fn i7_a_screenshot_of_the_fixture_shows_the_commit_it_records() {
    let tx = setup();
    let node = id_of(&labelled(&tx, "live-image-fixture").await);
    let Some((rgba, width, meta)) = shot_of(&tx, node).await else {
        return;
    };
    assert_eq!(meta.live_images.len(), 1, "{meta:?}");
    let shot = &meta.live_images[0];
    assert_eq!((shot.node, shot.generation), (node, 1));
    // The node's box, rounded outwards for the crop: the picture, snapped to
    // device pixels, lies in it.
    assert!(
        meta.width - shot.rect[2] <= 1 && meta.height - shot.rect[3] <= 1,
        "{meta:?}"
    );
    check_fixture_pixels(&rgba, width, shot);
}

/// I.8. After a screenshot the stats carry the renderer's textures, and a
/// commit stepped on the tree thread is what the next screenshot shows.
#[tokio::test]
async fn i8_a_stepped_commit_reaches_the_stats_and_the_next_screenshot() {
    let tx = setup();
    let node = id_of(&labelled(&tx, "live-image-fixture").await);
    if shot_of(&tx, node).await.is_none() {
        return;
    }
    let before = stats(&tx, node).await;
    let textures = before.textures.expect("the screenshot's renderer");
    assert_eq!(textures.textures, 1);
    assert!(textures.bytes >= 96 * 64 * 4, "{textures:?}");
    assert_eq!(textures.uploads_full, 1);
    assert_eq!(before.attachment.captures, 1);

    click(&tx, "Step live producer").await;
    assert_eq!(stats(&tx, node).await.source.generation, 2);
    let (rgba, width, meta) = shot_of(&tx, node).await.expect("an adapter, as before");
    assert_eq!(meta.live_images[0].generation, 2);
    check_fixture_pixels(&rgba, width, &meta.live_images[0]);
    let after = stats(&tx, node).await;
    assert_eq!(after.attachment.window_generation, 2);
    assert_eq!(after.textures.expect("still").textures, 1, "one texture");
}

/// I.9. The freeze check, headless: with the producer running, two screenshots
/// half a second apart show a later generation, each its own.
#[tokio::test]
async fn i9_two_screenshots_of_a_running_producer_show_a_later_commit() {
    let tx = setup();
    let node = id_of(&labelled(&tx, "live-image-fixture").await);
    if shot_of(&tx, node).await.is_none() {
        return;
    }
    click(&tx, "Start live producer").await;
    // A second press starts no second producer: one writer steps, one runs.
    click(&tx, "Start live producer").await;
    assert_eq!(stats(&tx, node).await.source.writers, 2);
    let (rgba, width, first) = shot_of(&tx, node).await.expect("an adapter");
    check_fixture_pixels(&rgba, width, &first.live_images[0]);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let (rgba, width, second) = shot_of(&tx, node).await.expect("an adapter");
    check_fixture_pixels(&rgba, width, &second.live_images[0]);
    let (a, b) = (
        first.live_images[0].generation,
        second.live_images[0].generation,
    );
    assert!(b > a, "the picture moved on: {a} then {b}");
}
