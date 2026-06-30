// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The headless tree-owning thread.
//!
//! `WidgetTree` is `!Send`, so it can never cross a thread boundary. A
//! dedicated `std::thread` owns a [`HeadlessApp`](bastyde::app::HeadlessApp) built from a small demo
//! UI; the async rmcp handlers marshal `Send` DTOs to it over a channel and
//! await a reply. Screenshots run **on this thread** via
//! `pollster::block_on` — correct precisely because this thread has no tokio
//! runtime (calling `pollster::block_on` inside an async rmcp handler would
//! panic).

use bastyde::core::WidgetTree;
use bastyde::core::accesskit;
use bastyde::prelude::*;
use bastyde::widgets::{Button, ButtonVariant, Checkbox, TextInput, TextWidget, VStack};
use bastyde_automation::dto::{
    AutomationOp, AutomationReply, AutomationRequest, WindowInfo, codes,
};
use bastyde_automation::recording_ops::RecordingWindowOps;
use bastyde_render::Renderer;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::oneshot;

/// Logical size the headless window is laid out at.
const HEADLESS_W: f32 = 800.0;
const HEADLESS_H: f32 = 600.0;

/// The reply the host thread produces for one job: either a JSON-bearing
/// [`AutomationReply`] (most ops) or raw PNG bytes (the screenshot op).
pub enum HostReply {
    Reply(AutomationReply),
    Image { png: Vec<u8>, warnings: Vec<String> },
}

/// One unit of work sent from an async handler to the tree thread.
pub type Job = (AutomationRequest, oneshot::Sender<HostReply>);

/// Build the headless demo app. Representative of a real Bastyde UI: a
/// heading, two buttons, a text field, and a checkbox — enough surface for
/// agents and golden tests to exercise the toolkit.
fn build_app() -> bastyde::app::HeadlessApp {
    BastydeAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde Automation (headless)")
                .id("main")
                .size(HEADLESS_W as u32, HEADLESS_H as u32)
                .root(|tree, _state| {
                    let checked = Signal::new(false);
                    let name = Signal::new(String::new());
                    tree.add(
                        VStack::new()
                            .spacing(12.0)
                            .child(TextWidget::new(lit!("Bastyde Automation Demo")))
                            .child(Button::new(lit!("Save")).variant(ButtonVariant::Filled))
                            .child(Button::new(lit!("Cancel")))
                            .child(TextInput::new(name).placeholder(lit!("Name")))
                            .child(Checkbox::new(checked).label(lit!("Enabled"))),
                    )
                }),
        )
        .build_headless()
}

/// Spawn the tree-owning thread. It builds the app, lays it out, then loops
/// `recv → handle → reply` until the channel closes (server shutdown).
pub fn spawn_tree_thread(mut rx: UnboundedReceiver<Job>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("bastyde-automation-tree".into())
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
        .expect("spawn bastyde-automation tree thread")
}

fn handle_job(
    app: &mut bastyde::app::HeadlessApp,
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
                title: Some("Bastyde Automation (headless)".to_string()),
                focused: true,
            }];
            HostReply::Reply(AutomationReply::ok_json(&windows))
        }
        AutomationOp::Screenshot { node } => screenshot(app, ops, cache, *node, req),
        // Everything else is a plain per-tree op.
        other => HostReply::Reply(bastyde_automation::execute(
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
}

impl RendererCache {
    fn new() -> Self {
        Self { inner: None }
    }
    fn get(&mut self) -> Option<&mut (Renderer, wgpu::Device, wgpu::Queue)> {
        if self.inner.is_none() {
            // No tokio runtime on this thread → `pollster::block_on` is safe.
            self.inner = Some(pollster::block_on(
                bastyde_render::test_support::create_test_renderer("bastyde-automation-mcp"),
            ));
        }
        self.inner.as_mut().and_then(|o| o.as_mut())
    }
}

fn screenshot(
    app: &mut bastyde::app::HeadlessApp,
    ops: &mut RecordingWindowOps,
    cache: &mut RendererCache,
    node: Option<u64>,
    req: &AutomationRequest,
) -> HostReply {
    // Settle so the captured frame reflects any prior action, then render.
    let _ = bastyde_automation::run_settle(&mut app.tree, ops, &req.settle);

    // Compute an optional crop rectangle (logical pixels) from a node.
    let crop = node.and_then(|n| {
        let nid = accesskit::NodeId(n);
        let wid = bastyde::core::accessibility::node_id_to_widget_id_maybe(nid)
            .or_else(|| app.tree.widget_for_synthetic(nid))?;
        Some(app.tree.bounds(wid))
    });

    let warnings = webview_warnings(&mut app.tree);
    let frame = app.tree.render();

    let (w, h) = (HEADLESS_W as u32, HEADLESS_H as u32);
    let Some((renderer, device, queue)) = cache.get() else {
        return HostReply::Reply(AutomationReply::err(
            codes::GPU_UNAVAILABLE,
            "no GPU backend available for offscreen screenshot rendering",
        ));
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bastyde-automation-mcp screenshot"),
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
    renderer.render(&frame, &view, 1.0, w, h, [0.0, 0.0, 0.0, 0.0]);
    let rgba = bastyde_render::test_support::read_texture_rgba(device, queue, &texture, w, h);

    let (bytes, ow, oh) = match crop {
        Some(rect) => crop_rgba(&rgba, w, h, rect),
        None => (rgba, w, h),
    };
    if ow == 0 || oh == 0 {
        return HostReply::Reply(AutomationReply::err(
            codes::BAD_ARGUMENT,
            "crop region is empty / outside the window",
        ));
    }
    HostReply::Image {
        png: encode_png(&bytes, ow, oh),
        warnings,
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

/// Crop a tightly-packed RGBA buffer to `rect` (logical px == physical px at
/// the headless scale of 1.0), clamped to the image.
fn crop_rgba(src: &[u8], w: u32, h: u32, rect: bastyde_canvas::Rect) -> (Vec<u8>, u32, u32) {
    let x0 = (rect.x.floor().max(0.0) as u32).min(w);
    let y0 = (rect.y.floor().max(0.0) as u32).min(h);
    let x1 = ((rect.x + rect.width).ceil().max(0.0) as u32).min(w);
    let y1 = ((rect.y + rect.height).ceil().max(0.0) as u32).min(h);
    if x1 <= x0 || y1 <= y0 {
        return (Vec::new(), 0, 0);
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y0..y1 {
        let start = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&src[start..start + (cw * 4) as usize]);
    }
    (out, cw, ch)
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
