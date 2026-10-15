// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A real Servo renders a page into a `WebView`'s picture, with no window.
//!
//! Ignored by default: it starts the whole engine, which takes seconds and an
//! OpenGL driver (a software one does: `LIBGL_ALWAYS_SOFTWARE=1` runs it on
//! llvmpipe). Run it with
//!
//! ```text
//! cargo test -p teksilo-webview --features servo-backend --test servo_smoke -- --ignored
//! ```
//!
//! Servo allows one engine per process, so everything is one test.

#![cfg(feature = "servo-backend")]

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use teksilo_canvas::SizeProposal;
use teksilo_canvas::live_image::testing::LiveImageMirror;
use teksilo_core::NoopWindowOps;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::presets::intui;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_webview::{ServoBackend, WebView, WebViewRegistry};

/// Run the engine and the window until `done` holds for the frame the window
/// drew, or `timeout` passes. The test has no app event loop, so it answers
/// the engine's pump requests itself, as `teksilo-app` does.
fn run_until(
    tree: &mut WidgetTree,
    registry: &WebViewRegistry,
    mirror: &mut LiveImageMirror,
    size: (f32, f32),
    timeout: Duration,
    mut done: impl FnMut(u32, u32, &[u8]) -> bool,
) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        registry.pump();
        tree.layout(SizeProposal::exact(size.0, size.1));
        let frame = tree.render();
        mirror.consume(&frame);
        if let Some(quad) = frame.live_images.first()
            && let Some((w, h, pixels)) = mirror.pixels(quad.consumer.source().id())
            && done(w, h, pixels)
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The pixel at `(x, y)` counted from the TOP, in a texture whose rows are
/// stored bottom-up, as a GL readback stores them.
fn px(w: u32, h: u32, pixels: &[u8], x: u32, y_from_top: u32) -> [u8; 3] {
    let row = (h - 1 - y_from_top) as usize;
    let i = (row * w as usize + x as usize) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

#[test]
#[ignore = "starts a real Servo engine; needs OpenGL"]
fn servo_renders_a_page_into_the_web_views_picture() {
    let registry = WebViewRegistry::new(ServoBackend::new());
    let mut map: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
    map.insert(TypeId::of::<WebViewRegistry>(), Box::new(registry.clone()));
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(map)));

    tree.add(WebView::new().html(concat!(
        "<html><body style='margin:0'>",
        "<div style='position:fixed;left:0;top:0;width:100%;height:50%;background:rgb(255,0,0)'></div>",
        "<div style='position:fixed;left:0;top:50%;width:100%;height:50%;background:rgb(0,0,255)'></div>",
        "</body></html>"
    )));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    let mut mirror = LiveImageMirror::new();

    let rendered = run_until(
        &mut tree,
        &registry,
        &mut mirror,
        (400.0, 300.0),
        Duration::from_secs(60),
        |w, h, pixels| {
            (w, h) == (400, 300)
                && px(w, h, pixels, 200, 50) == [255, 0, 0]
                && px(w, h, pixels, 200, 250) == [0, 0, 255]
        },
    );
    assert!(
        rendered,
        "the page's top half red and bottom half blue, at the widget's size"
    );

    let frame = tree.render();
    let quad = frame.live_images.first().expect("the picture is drawn");
    // Drawn turned upright: the top of the screen samples the texture's last
    // row, which holds the page's first.
    assert!(
        quad.uv[0][1] > quad.uv[3][1],
        "the bottom-up rows are drawn flipped, uv {:?}",
        quad.uv
    );

    // A resize: the engine renders the page laid out at the new size.
    let resized = run_until(
        &mut tree,
        &registry,
        &mut mirror,
        (200.0, 100.0),
        Duration::from_secs(30),
        |w, h, pixels| {
            (w, h) == (200, 100)
                && px(w, h, pixels, 100, 10) == [255, 0, 0]
                && px(w, h, pixels, 100, 90) == [0, 0, 255]
        },
    );
    assert!(resized, "the page follows the widget to its new size");
}
