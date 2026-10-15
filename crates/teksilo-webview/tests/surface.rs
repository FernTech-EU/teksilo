// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A page an engine renders offscreen, into a picture the widget draws,
//! driven headlessly against the surface `MemoryWebViewBackend`.
//!
//! That backend commits one opaque frame of the bounds' device-pixel size on
//! every `set_bounds`, as an engine answers a resize, and records every input
//! the widget forwards. So these tests check the widget's half: that it hands
//! the engine a picture, draws it, keeps the engine told where it is and
//! whether it is shown, and forwards input at the page pixel it landed on.
//! What a real engine does with that input is the backend's own business.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::NoopWindowOps;
use teksilo_core::event::{Key, Modifiers, PointerButton, ScrollDelta, WidgetEvent};
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::presets::intui;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::TeksiloWindowId;
use teksilo_widgets::primitives::{FixedSize, MinSize, VStack};

use teksilo_webview::{
    MemoryWebViewRecords, WebView, WebViewEvent, WebViewEventPayload, WebViewId, WebViewInputEvent,
    WebViewOp, WebViewRegistry, WebViewTouchPhase, memory_registry, memory_surface_registry,
};

fn tree_with_registry(registry: WebViewRegistry) -> WidgetTree {
    let mut map: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
    map.insert(TypeId::of::<WebViewRegistry>(), Box::new(registry));
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(map)));
    tree
}

/// Lay out, open the engine, then lay out again: the engine answers its first
/// `set_bounds` with a frame, and the second pass places it.
fn settle(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.run_mount_actions(&mut NoopWindowOps);
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

/// A full-window surface page at `scale`.
fn surface_page(scale: f32) -> (WidgetTree, WebViewId, MemoryWebViewRecords) {
    let (registry, records) = memory_surface_registry();
    let mut tree = tree_with_registry(registry);
    tree.set_device_scale_factor(scale);
    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    tree.add(webview);
    settle(&mut tree);
    (tree, wv_id, records)
}

fn opened_with_surface(records: &MemoryWebViewRecords, id: WebViewId) -> Option<bool> {
    records.ops_for(id).iter().find_map(|op| match op {
        WebViewOp::Open { surface, .. } => Some(*surface),
        _ => None,
    })
}

fn last_bounds(records: &MemoryWebViewRecords, id: WebViewId) -> Option<Rect> {
    records.ops_for(id).iter().rev().find_map(|op| match op {
        WebViewOp::SetBounds { bounds, .. } => Some(*bounds),
        _ => None,
    })
}

fn deliver(tree: &mut WidgetTree, registry: &WebViewRegistry, id: WebViewId, event: WebViewEvent) {
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        registry.deliver(
            WebViewEventPayload {
                window_id_owner: TeksiloWindowId::new(0),
                web_view_id: id,
                event,
            },
            ctx,
        );
    });
}

#[test]
fn only_a_surface_backend_is_handed_a_picture() {
    let (_tree, surface_id, surface_records) = surface_page(1.0);
    assert_eq!(
        opened_with_surface(&surface_records, surface_id),
        Some(true)
    );

    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);
    let webview = WebView::new().url("about:blank");
    let id = webview.id();
    tree.add(webview);
    settle(&mut tree);
    assert_eq!(
        opened_with_surface(&records, id),
        Some(false),
        "a native subview has no picture to draw into"
    );
}

#[test]
fn the_page_is_drawn_one_frame_pixel_per_device_pixel() {
    let (mut tree, _id, _records) = surface_page(2.0);
    let frame = tree.render();
    let quad = frame
        .live_images
        .first()
        .expect("the page's picture is drawn");
    // 800 × 600 logical at 2× is a 1600 × 1200 frame, drawn over the whole box.
    let [x, y, w, h] = quad.screen;
    assert_eq!(
        (x, y, w, h),
        (0.0, 0.0, 800.0, 600.0),
        "the picture covers the box exactly, unscaled"
    );
}

#[test]
fn a_click_reaches_the_engine_at_the_page_pixel_it_landed_on() {
    let (mut tree, id, records) = surface_page(2.0);
    tree.tap_with(teksilo_tokens::PointerKind::Mouse, Point::new(100.0, 50.0));
    let presses: Vec<_> = records
        .input_log(id)
        .into_iter()
        .filter(|e| matches!(e, WebViewInputEvent::PointerButton { .. }))
        .collect();
    assert_eq!(
        presses,
        vec![
            WebViewInputEvent::PointerButton {
                position: Point::new(200.0, 100.0),
                button: PointerButton::Primary,
                pressed: true,
            },
            WebViewInputEvent::PointerButton {
                position: Point::new(200.0, 100.0),
                button: PointerButton::Primary,
                pressed: false,
            },
        ],
        "logical (100, 50) is page pixel (200, 100) at 2x"
    );
}

#[test]
fn a_finger_on_a_surface_page_is_a_touch_from_down_to_up() {
    let (mut tree, id, records) = surface_page(1.0);
    tree.tap_with(teksilo_tokens::PointerKind::Touch, Point::new(40.0, 30.0));
    let phases: Vec<_> = records
        .input_log(id)
        .into_iter()
        .filter_map(|e| match e {
            WebViewInputEvent::Touch {
                phase,
                position,
                pen,
                ..
            } => Some((phase, position, pen)),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases.first(),
        Some(&(WebViewTouchPhase::Down, Point::new(40.0, 30.0), false))
    );
    assert_eq!(
        phases.last().map(|p| p.0),
        Some(WebViewTouchPhase::Up),
        "the contact ends where it began: {phases:?}"
    );
    assert!(
        !records
            .input_log(id)
            .iter()
            .any(|e| matches!(e, WebViewInputEvent::PointerButton { .. })),
        "a finger is not a mouse button"
    );
    tree.assert_no_leaked_pointer_state();
}

#[test]
fn a_wheel_over_a_surface_page_scrolls_the_page() {
    let (mut tree, id, records) = surface_page(1.0);
    tree.dispatch_event(WidgetEvent::scroll_at(
        ScrollDelta::Lines { x: 0.0, y: -3.0 },
        Modifiers::NONE,
        Point::new(300.0, 200.0),
    ));
    assert!(
        records.input_log(id).contains(&WebViewInputEvent::Scroll {
            position: Point::new(300.0, 200.0),
            delta: ScrollDelta::Lines { x: 0.0, y: -3.0 },
            modifiers: Modifiers::NONE,
        }),
        "got {:?}",
        records.input_log(id)
    );
}

#[test]
fn keys_reach_the_page_only_while_it_holds_the_keyboard() {
    let (registry, records) = memory_surface_registry();
    let mut tree = tree_with_registry(registry.clone());
    let webview = WebView::new().url("about:blank");
    let id = webview.id();
    let frame = tree.add(webview);
    settle(&mut tree);
    tree.focus(frame);

    tree.press_key(Key::A, Modifiers::NONE);
    assert!(
        !records
            .input_log(id)
            .iter()
            .any(|e| matches!(e, WebViewInputEvent::Key { .. })),
        "a focused frame whose page was not entered keeps its keys"
    );

    deliver(
        &mut tree,
        &registry,
        id,
        WebViewEvent::EngineFocusChanged(true),
    );
    tree.press_key(Key::A, Modifiers::NONE);
    assert!(
        records.input_log(id).iter().any(|e| matches!(
            e,
            WebViewInputEvent::Key {
                key: Key::A,
                pressed: true,
                ..
            }
        )),
        "a page holding the keyboard receives it: {:?}",
        records.input_log(id)
    );

    let before = records.input_log(id).len();
    tree.press_key(Key::Tab, Modifiers::CTRL);
    assert!(
        !records.input_log(id)[before..]
            .iter()
            .any(|e| matches!(e, WebViewInputEvent::Key { key: Key::Tab, .. })),
        "Ctrl+Tab stays Teksilo's, so the page is never a trap"
    );
}

/// The canvas clips a surface page, so the engine keeps its whole box when a
/// viewport cuts it, and is told it is hidden only when nothing of it shows.
#[test]
fn a_clipped_surface_page_keeps_its_box_and_a_scrolled_out_one_is_hidden() {
    let (registry, records) = memory_surface_registry();
    let mut tree = tree_with_registry(registry);
    let webview = WebView::new().url("about:blank");
    let id = webview.id();
    let scroller = teksilo_widgets::ScrollArea::new().child(
        VStack::new()
            .child(FixedSize::new().width(300.0).height(200.0).child(webview))
            .child(MinSize::new(0.0, 4000.0)),
    );
    let offset = scroller.scroll_y_signal().clone();
    tree.add(scroller);
    settle(&mut tree);
    let full = last_bounds(&records, id).expect("bounds were sent");

    offset.set(100.0);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let clipped = last_bounds(&records, id).expect("bounds were sent");
    assert_eq!(
        (clipped.width, clipped.height),
        (full.width, full.height),
        "a half-scrolled surface page keeps its size"
    );
    assert_eq!(records.visibility_log(id), Vec::<bool>::new());

    offset.set(400.0);
    tree.layout(SizeProposal::exact(800.0, 600.0));
    assert_eq!(
        records.visibility_log(id),
        vec![false],
        "a page nobody can see is told so, so its engine can rest"
    );
}

/// An overlay is drawn over a surface page like over any other content, so
/// the engine has no reason to stand down for one.
#[test]
fn an_overlay_over_a_surface_page_leaves_it_shown() {
    use teksilo_core::overlay::{DismissBehavior, OverlayPlacement, OverlayRequest};

    let (registry, records) = memory_surface_registry();
    let mut tree = tree_with_registry(registry);
    let webview = WebView::new().url("about:blank");
    let id = webview.id();
    let anchor = tree.add(webview);
    let panel = tree.add(MinSize::new(200.0, 120.0));
    tree.set_dormant(panel);
    settle(&mut tree);

    tree.activate(panel);
    tree.show_overlay(OverlayRequest::new(
        panel,
        anchor,
        OverlayPlacement::Centered,
        DismissBehavior::EscapeOrClickOutside,
    ));
    tree.layout(SizeProposal::exact(800.0, 600.0));
    let _ = tree.render();
    assert_eq!(records.visibility_log(id), Vec::<bool>::new());
}
