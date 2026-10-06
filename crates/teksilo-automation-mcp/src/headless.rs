// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The headless tree-owning thread.
//!
//! `WidgetTree` is `!Send`, so it can never cross a thread boundary. A
//! dedicated `std::thread` owns a [`HeadlessApp`](teksilo::app::HeadlessApp) built from a small demo
//! UI; the async rmcp handlers marshal `Send` DTOs to it over a channel and
//! await a reply. Screenshots run **on this thread** via
//! `pollster::block_on` — correct precisely because this thread has no tokio
//! runtime (calling `pollster::block_on` inside an async rmcp handler would
//! panic).

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use teksilo::core::WidgetTree;
use teksilo::core::accesskit;
use teksilo::core::widget_builder::WidgetBuilder;
use teksilo::core::{LongPressRole, MultiContact, PanClaim, TouchAction};
use teksilo::prelude::*;
use teksilo::widgets::{
    Button, ButtonVariant, Checkbox, FixedSize, HStack, LiveImage, LiveImageSizing, RectWidget,
    ScalingFilter, TextInput, TextWidget, VStack, ZStack,
};
use teksilo_automation::dto::{
    AutomationOp, AutomationReply, AutomationRequest, WindowInfo, codes,
};
use teksilo_automation::recording_ops::RecordingWindowOps;
use teksilo_render::Renderer;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;

/// Logical size the headless window is laid out at.
const HEADLESS_W: f32 = 800.0;
const HEADLESS_H: f32 = 600.0;

/// The reply the host thread produces for one job: either a JSON-bearing
/// [`AutomationReply`] (most ops) or raw PNG bytes (the screenshot op).
pub enum HostReply {
    Reply(AutomationReply),
    Image {
        png: Vec<u8>,
        meta: teksilo_automation::dto::ScreenshotMeta,
    },
}

/// One unit of work sent from an async handler to the tree thread.
pub type Job = (AutomationRequest, oneshot::Sender<HostReply>);

/// Build the headless demo app. Representative of a real Teksilo UI: a
/// heading, two buttons, a text field, and a checkbox — enough surface for
/// agents and golden tests to exercise the toolkit — plus the arbitration
/// fixture below, which the touch ops need.
fn build_app() -> teksilo::app::HeadlessApp {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Teksilo Automation (headless)")
                .id("main")
                .size(HEADLESS_W as u32, HEADLESS_H as u32)
                .root(|tree, _state| {
                    let checked = Signal::new(false);
                    let name = Signal::new(String::new());
                    tree.add(
                        VStack::new()
                            .spacing(12.0)
                            .child(TextWidget::new(lit!("Teksilo Automation Demo")))
                            .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
                            .child(Button::new(lit!("Cancel")))
                            .child(TextInput::new(name).placeholder(lit!("Name")))
                            .child(Checkbox::new(checked).label(lit!("Enabled")))
                            .child(arbitration_fixture())
                            .child(live_image_fixture()),
                    )
                }),
        )
        .build_headless()
}

/// The shape one row of the arbitration matrix is written against — a
/// reorderable list row's own drag inside a vertical scroller's pan claim.
///
/// It is here because the touch ops' CI gate drives it: `a_scripted_touch_/// sequence_reproduces_the_documented_arbitration_row` in
/// `tests/stdio_smoke.rs` presses it, moves twice, and checks the winner
/// against the table generated into `docs/events-and-gestures.md`. Nothing
/// else in this demo has a competitor at all, so without it the gate could
/// only assert that the op returned — which is green whatever the arbitration
/// did.
///
/// The shape is `Scenario::ListRow` in
/// `crates/teksilo-core/tests/arbitration_matrix.rs`, restated with real
/// widgets: a bare leaf inside a node carrying a tap and a drag inside a node
/// declaring a vertical [`PanClaim`]. Both competitors are named and given a
/// role so `find_node` can resolve them; the box is 160 dp tall so the row's
/// tap boundary — a coarse pointer's is the member's own bounds — is nowhere
/// near the ±19 dp the row's movement asks for.
fn arbitration_fixture() -> impl teksilo::core::Widget + 'static {
    ZStack::new()
        .child(
            ZStack::new()
                .child(
                    FixedSize::new()
                        .width(360.0)
                        .height(160.0)
                        .child(RectWidget::new()),
                )
                .on_tap(|_e, _c| {})
                .on_drag(|_p, _c| {})
                .access_role(accesskit::Role::ListItem)
                .access_label(lit!("arbitration-row")),
        )
        .pan_claim(PanClaim::vertical())
        .access_role(accesskit::Role::ScrollView)
        .access_label(lit!("arbitration-scroller"))
}

/// The live picture's size: 96 x 64 source pixels, drawn one to one.
pub(crate) const FIXTURE_W: u32 = 96;
pub(crate) const FIXTURE_H: u32 = 64;

/// One frame of the live-image fixture: every pixel names where it is and
/// which commit it belongs to, `R = 8 (x mod 32) + 4`, `G = 8 (y mod 32) + 4`,
/// `B = 8 (generation mod 32) + 4`, the fourth byte 0 (RGBX). A probe decodes a
/// screenshot's pixel with `div 8`, which survives the sRGB round trip.
fn fixture_frame(generation: u64) -> Vec<u8> {
    let b = (8 * (generation % 32) + 4) as u8;
    let mut px = Vec::with_capacity((FIXTURE_W * FIXTURE_H * 4) as usize);
    for y in 0..FIXTURE_H {
        for x in 0..FIXTURE_W {
            px.extend_from_slice(&[(8 * (x % 32) + 4) as u8, (8 * (y % 32) + 4) as u8, b, 0]);
        }
    }
    px
}

/// Commit the next generation of the fixture's source.
fn commit_fixture(writer: &LiveImageWriter) {
    let next = writer.source().generation() + 1;
    let _ = writer.write_frame(
        FIXTURE_W,
        FIXTURE_H,
        &fixture_frame(next),
        (FIXTURE_W * 4) as usize,
    );
}

/// A `LiveImage` a probe can aim at and read back, with no producer running
/// until it asks for one: the image is generation 1 when built, "Step live
/// producer" commits one more per click on the tree thread, and "Start live
/// producer" starts a thread committing at 60 Hz until the process exits.
/// Until then a full-window screenshot stays the same from run to run.
///
/// The picture takes every input a VM screen takes (focus, every key, every
/// contact, a hold that opens nothing, no pan), and the last event it
/// received, with its local position, is its own accessible description,
/// which `read_node` returns. It is appended after the arbitration fixture,
/// so no node above it moves.
fn live_image_fixture() -> impl teksilo::core::Widget + 'static {
    let source = LiveImageSource::builder(LivePixelFormat::Rgbx8)
        .label("live-image-fixture")
        .build();
    let writer = Rc::new(source.writer());
    commit_fixture(&writer);
    let log = Signal::new("none".to_string());
    let pointer_log = log.clone();
    let key_log = log.clone();
    let image = LiveImage::new(source.clone())
        .sizing(LiveImageSizing::Natural)
        .scaling(ScalingFilter::Nearest)
        .alt(lit!("live-image-fixture"))
        .access_description(log)
        .focusable(true)
        .keyboard_capture(true)
        .multi_contact(MultiContact::All)
        .long_press_role(LongPressRole::None)
        .touch_action(TouchAction::NONE)
        .on_pointer_event(move |event, _ctx| {
            let entry = match event {
                WidgetEvent::PointerDown { position, .. } => {
                    Some(format!("pointer_down {} {}", position.x, position.y))
                }
                WidgetEvent::PointerMove { position, .. } => {
                    Some(format!("pointer_move {} {}", position.x, position.y))
                }
                WidgetEvent::PointerUp { position, .. } => {
                    Some(format!("pointer_up {} {}", position.x, position.y))
                }
                _ => None,
            };
            match entry {
                Some(entry) => {
                    pointer_log.set(entry);
                    EventResponse::Handled
                }
                None => EventResponse::Ignored,
            }
        })
        .on_key(move |event, _ctx| match event {
            WidgetEvent::KeyDown { key, .. } => {
                key_log.set(format!("key_down {key:?}"));
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        });
    let step_writer = writer.clone();
    let started = Arc::new(AtomicBool::new(false));
    HStack::new().spacing(12.0).child(image).child(
        VStack::new()
            .spacing(4.0)
            .child(
                Button::new(lit!("Step live producer"))
                    .on_activate_fn(move |_ctx| commit_fixture(&step_writer)),
            )
            .child(
                Button::new(lit!("Start live producer")).on_activate_fn(move |_ctx| {
                    if started.swap(true, Ordering::AcqRel) {
                        return;
                    }
                    let writer = source.writer();
                    std::thread::Builder::new()
                        .name("live-image-fixture".into())
                        .spawn(move || {
                            loop {
                                commit_fixture(&writer);
                                std::thread::sleep(std::time::Duration::from_micros(16_667));
                            }
                        })
                        .expect("spawn the fixture's producer");
                }),
            ),
    )
}

/// Spawn the tree-owning thread. It builds the app, lays it out, then loops
/// `recv → handle → reply` until the channel closes (server shutdown).
pub fn spawn_tree_thread(mut rx: UnboundedReceiver<Job>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("teksilo-automation-tree".into())
        .spawn(move || {
            let mut app = build_app();
            app.tree.layout(SizeProposal::exact(HEADLESS_W, HEADLESS_H));
            let mut ops = RecordingWindowOps::new();
            let mut cache = RendererCache::new();
            while let Some((req, reply_tx)) = rx.blocking_recv() {
                let reply = handle_job(&mut app, &mut ops, &mut cache, &req);
                // The receiver may have been dropped if the handler future
                // was cancelled; that's fine.
                let _ = reply_tx.send(reply);
            }
        })
        .expect("spawn teksilo-automation tree thread")
}

fn handle_job(
    app: &mut teksilo::app::HeadlessApp,
    ops: &mut RecordingWindowOps,
    cache: &mut RendererCache,
    req: &AutomationRequest,
) -> HostReply {
    match &req.op {
        // Headless is single-tree: report one synthetic window.
        AutomationOp::ListWindows => {
            let windows = vec![WindowInfo {
                id: 0,
                label: Some("main".to_string()),
                title: Some("Teksilo Automation (headless)".to_string()),
                focused: true,
            }];
            HostReply::Reply(AutomationReply::ok_json(&windows))
        }
        AutomationOp::Screenshot { node } => screenshot(app, ops, cache, *node, req),
        // The source and attachment half is the tree's; the textures are the
        // cached renderer's, and there is no window to wake.
        AutomationOp::LiveImageStats { .. } => {
            let reply = teksilo_automation::execute(&mut app.tree, ops, &req.op, &req.settle);
            let textures = cache.renderer().map(Renderer::live_texture_stats);
            HostReply::Reply(teksilo_automation::with_window_stats(reply, textures, None))
        }
        // Everything else is a plain per-tree op.
        other => HostReply::Reply(teksilo_automation::execute(
            &mut app.tree,
            ops,
            other,
            &req.settle,
        )),
    }
}

// ---------------------------------------------------------------------------
// Screenshot
// ---------------------------------------------------------------------------

/// Lazily-initialised offscreen renderer. `inner` is `None` until the first
/// screenshot; thereafter it's `Some(None)` (no GPU on this host) or
/// `Some(Some(..))` (ready).
struct RendererCache {
    inner: Option<Option<(Renderer, wgpu::Device, wgpu::Queue)>>,
    /// Glyph-atlas version the cached renderer last received.
    atlas_version: u64,
}

impl RendererCache {
    fn new() -> Self {
        Self {
            inner: None,
            atlas_version: 0,
        }
    }
    /// The renderer, if a screenshot made it.
    fn renderer(&self) -> Option<&Renderer> {
        self.inner
            .as_ref()?
            .as_ref()
            .map(|(renderer, _, _)| renderer)
    }

    fn get(&mut self) -> Option<(&mut (Renderer, wgpu::Device, wgpu::Queue), &mut u64)> {
        if self.inner.is_none() {
            // No tokio runtime on this thread → `pollster::block_on` is safe.
            self.inner = Some(pollster::block_on(
                teksilo_render::test_support::create_offscreen_renderer("teksilo-automation-mcp"),
            ));
        }
        let atlas_version = &mut self.atlas_version;
        self.inner
            .as_mut()
            .and_then(|o| o.as_mut())
            .map(|r| (r, atlas_version))
    }
}

fn screenshot(
    app: &mut teksilo::app::HeadlessApp,
    ops: &mut RecordingWindowOps,
    cache: &mut RendererCache,
    node: Option<u64>,
    req: &AutomationRequest,
) -> HostReply {
    // Settle so the captured frame reflects any prior action, then render.
    let _ = teksilo_automation::run_settle(&mut app.tree, ops, &req.settle);

    // Compute an optional crop rectangle (logical pixels) from a node.
    let crop = node.and_then(|n| {
        let nid = accesskit::NodeId(n);
        let wid = teksilo::core::accessibility::node_id_to_widget_id_maybe(nid)
            .or_else(|| app.tree.widget_for_synthetic(nid))?;
        Some(app.tree.bounds(wid))
    });

    let warnings = webview_warnings(&mut app.tree);

    let (w, h) = (HEADLESS_W as u32, HEADLESS_H as u32);
    let Some(((renderer, device, queue), atlas_version)) = cache.get() else {
        return HostReply::Reply(AutomationReply::err(
            codes::GPU_UNAVAILABLE,
            "no GPU backend available for offscreen screenshot rendering",
        ));
    };
    let frame = app.render_for_capture(renderer, atlas_version);

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("teksilo-automation-mcp screenshot"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // A capture: every live picture shows its latest commit, whatever its
    // producer is doing, and the render is not counted as a displayed frame.
    renderer.render_capture(&frame, &view, 1.0, w, h, [0.0, 0.0, 0.0, 0.0]);
    // Fallible, not panicking: a lost device must cost the caller one
    // screenshot, not the whole session. This thread owns the `!Send` tree, so
    // a panic here would take every subsequent tool call down with it.
    let rgba =
        match teksilo_render::test_support::try_read_texture_rgba(device, queue, &texture, w, h) {
            Ok(px) => px,
            Err(e) => {
                return HostReply::Reply(AutomationReply::err(
                    codes::GPU_READBACK_FAILED,
                    e.to_string(),
                ));
            }
        };

    let region = match crop {
        Some(rect) => crop_region(rect, w, h),
        None => teksilo_canvas::PixelRect::full(w, h),
    };
    if region.is_empty() {
        return HostReply::Reply(AutomationReply::err(
            codes::BAD_ARGUMENT,
            "crop region is empty / outside the window",
        ));
    }
    let bytes = crop_rgba(&rgba, w, region);
    let live_images = teksilo_automation::live_image_shots(
        &app.tree,
        &frame,
        renderer.live_image_decisions(),
        1.0,
        region,
    );
    HostReply::Image {
        png: encode_png(&bytes, region.width, region.height),
        // Headless lays out at scale 1.0, so physical and logical coincide.
        meta: teksilo_automation::dto::ScreenshotMeta {
            width: region.width,
            height: region.height,
            scale: 1.0,
            warnings,
            live_images,
        },
    }
}

/// If the AT tree contains a `WebView` node, the readback can't see it (a
/// native subview composites on top of wgpu) — warn the caller.
fn webview_warnings(tree: &mut WidgetTree) -> Vec<String> {
    let update = tree.sync_accessibility();
    let has_webview = update
        .nodes
        .iter()
        .any(|(_, n)| n.role() == accesskit::Role::WebView);
    if has_webview {
        vec!["webview_hole_possible".to_string()]
    } else {
        Vec::new()
    }
}

/// The pixels a crop `rect` (logical px == physical px at the headless scale
/// of 1.0) covers: its edges rounded outwards, clamped to the image. Empty
/// when it misses.
fn crop_region(rect: teksilo_canvas::Rect, w: u32, h: u32) -> teksilo_canvas::PixelRect {
    let x0 = (rect.x.floor().max(0.0) as u32).min(w);
    let y0 = (rect.y.floor().max(0.0) as u32).min(h);
    let x1 = ((rect.x + rect.width).ceil().max(0.0) as u32).min(w);
    let y1 = ((rect.y + rect.height).ceil().max(0.0) as u32).min(h);
    if x1 <= x0 || y1 <= y0 {
        return teksilo_canvas::PixelRect::new(x0, y0, 0, 0);
    }
    teksilo_canvas::PixelRect::new(x0, y0, x1 - x0, y1 - y0)
}

/// `region` of a tightly-packed RGBA buffer `w` pixels wide.
fn crop_rgba(src: &[u8], w: u32, region: teksilo_canvas::PixelRect) -> Vec<u8> {
    let row = (region.width * 4) as usize;
    let mut out = Vec::with_capacity(row * region.height as usize);
    for y in region.y..region.y + region.height {
        let start = ((y * w + region.x) * 4) as usize;
        out.extend_from_slice(&src[start..start + row]);
    }
    out
}

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
