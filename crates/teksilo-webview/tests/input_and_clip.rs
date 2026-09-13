// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Who owns the pointer over a page, and where the engine subview is allowed
//! to be — driven headlessly against the `MemoryWebViewBackend`.
//!
//! Every test here runs on the crate's **default** features, i.e. with no
//! engine at all. That is deliberate and it is also the limit: what these
//! assert is the *framework* half of each mechanism — the declarations on the
//! node, the pointer teardown, and the exact ops the widget issues to whatever
//! handle it was given. Whether a real engine honours
//! `set_input_passthrough` is a property of that engine (wry does not, and
//! says so), and no headless test can reach it.
//!
//! The two input-mode tests are written against a fixture with a **tappable
//! ancestor** over the page rather than a bare page in a stack, because that
//! is the only shape in which the two modes give different answers: without an
//! eligible handler above it, a declined press has nowhere to go and both
//! modes look alike.

use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::NoopWindowOps;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::pointer::touch_action::TouchAction;
use teksilo_core::presets::intui;
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::TeksiloWindowId;
use teksilo_widgets::primitives::{FixedSize, MinSize, VStack, ZStack};

use teksilo_webview::{
    MemoryWebViewRecords, WebView, WebViewEvent, WebViewEventPayload, WebViewId, WebViewInput,
    WebViewOp, WebViewRegistry, memory_registry,
};

fn tree_with_registry(registry: WebViewRegistry) -> WidgetTree {
    let mut map: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
    map.insert(TypeId::of::<WebViewRegistry>(), Box::new(registry));
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(map)));
    tree
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(800.0, 600.0));
    tree.run_mount_actions(&mut NoopWindowOps);
}

/// A page under a tappable ancestor — the discriminating fixture. Returns the
/// tap counter, the page's node id, its routing id and the op recorder.
fn page_under_a_tappable_ancestor(
    input: WebViewInput,
) -> (
    WidgetTree,
    Rc<Cell<u32>>,
    WidgetId,
    WebViewId,
    MemoryWebViewRecords,
) {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank").input_mode(input);
    let wv_id = webview.id();
    let taps = Rc::new(Cell::new(0_u32));
    let counter = taps.clone();
    let root = ZStack::new()
        .child(webview)
        .on_tap(move |_event, _ctx| counter.set(counter.get() + 1));
    tree.add(root);
    layout(&mut tree);
    let node = tree
        .roots()
        .first()
        .copied()
        .and_then(|root| find_web_view(&tree, root))
        .expect("the web view is in the tree");
    (tree, taps, node, wv_id, records)
}

/// The `WebView`'s own node, found by type name (the widget is nested inside
/// the ancestor's handler wrapper and the ZStack).
fn find_web_view(tree: &WidgetTree, from: WidgetId) -> Option<WidgetId> {
    if tree
        .widget_type_name(from)
        .is_some_and(|n| n.ends_with("WebView"))
    {
        return Some(from);
    }
    tree.children(from)
        .into_iter()
        .find_map(|child| find_web_view(tree, child))
}

// ---------------------------------------------------------------------------
// who owns the pointer
// ---------------------------------------------------------------------------

/// `Native`: the engine owns the rectangle, so a press over the page is not
/// the app's to act on — and the ancestor that would have taken it does not.
#[test]
fn a_press_on_a_native_page_is_never_offered_to_the_app() {
    let (mut tree, taps, _node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Native);
    tree.tap_with(teksilo_tokens::PointerKind::Mouse, Point::new(400.0, 300.0));
    assert_eq!(
        taps.get(),
        0,
        "a press the engine owns must not also activate a Teksilo ancestor"
    );
}

/// `Transparent`: Teksilo owns the rectangle, so the same press bubbles and the
/// ancestor's tap fires. This is what makes app UI — a toolbar row, a card, an
/// overlay — operable over a page that is only being displayed.
#[test]
fn a_press_on_a_transparent_page_bubbles_to_the_app() {
    let (mut tree, taps, _node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Transparent);
    tree.tap_with(teksilo_tokens::PointerKind::Mouse, Point::new(400.0, 300.0));
    assert_eq!(taps.get(), 1, "the app must receive a press it owns");
}

/// The same, by finger: a coarse pointer takes the same two answers.
#[test]
fn a_finger_reaches_the_app_over_a_transparent_page_and_not_over_a_native_one() {
    let (mut tree, taps, _node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Transparent);
    tree.tap_with(teksilo_tokens::PointerKind::Touch, Point::new(400.0, 300.0));
    assert_eq!(taps.get(), 1, "a finger must reach the app it belongs to");

    let (mut tree, taps, _node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Native);
    tree.tap_with(teksilo_tokens::PointerKind::Touch, Point::new(400.0, 300.0));
    assert_eq!(taps.get(), 0, "…and must not, over a page that owns it");
}

/// The declaration half. `Native` forbids every default touch behaviour on the
/// hit path (the page scrolls, pinches and holds itself); `Transparent`
/// forbids none, because Teksilo is the one handling the contact.
#[test]
fn a_native_page_forbids_every_touch_default_and_a_transparent_one_forbids_none() {
    let (tree, _taps, node, _wv, _records) = page_under_a_tappable_ancestor(WebViewInput::Native);
    assert_eq!(
        tree.touch_action_for(node),
        TouchAction::NONE,
        "a native page must leave no default touch behaviour on its hit path"
    );

    let (tree, _taps, node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Transparent);
    assert_eq!(
        tree.touch_action_for(node),
        TouchAction::AUTO,
        "a page Teksilo drives must not declare a restriction it does not need"
    );
}

/// The teardown. A contact that lands on a native page is a contact the OS
/// will hand to the engine, so Teksilo will see no move and no lift for it:
/// leaving the interaction alive strands an arbitration waiting for movement
/// that never comes and a press record waiting for a release that never comes.
///
/// Asserted after the **down alone**, deliberately — the lift is the thing
/// that will not arrive.
#[test]
fn a_finger_on_a_native_page_leaves_no_pointer_state_behind() {
    let (mut tree, _taps, _node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Native);
    let finger = tree.new_contact();
    tree.touch_down(finger, Point::new(400.0, 300.0));

    tree.assert_no_leaked_pointer_state();
    assert_eq!(
        tree.live_pointers().count(),
        0,
        "the revoked contact must not survive the funnel"
    );
}

/// …and the same press on a page Teksilo owns is left completely alone: the
/// contact is live, which is what lets it become a tap, a pan or a hold.
#[test]
fn a_finger_on_a_transparent_page_keeps_its_press_alive() {
    let (mut tree, _taps, _node, _wv, _records) =
        page_under_a_tappable_ancestor(WebViewInput::Transparent);
    let finger = tree.new_contact();
    tree.touch_down(finger, Point::new(400.0, 300.0));

    assert_eq!(
        tree.live_pointers().count(),
        1,
        "Teksilo's own contact must not be revoked"
    );
    tree.touch_up(finger, Point::new(400.0, 300.0));
    tree.assert_no_leaked_pointer_state();
}

/// The engine half is only ever *asked for* in `Transparent`. A native page
/// never asks, so a backend that cannot pass input through is never made to
/// report an operation nobody wanted.
#[test]
fn only_a_transparent_page_asks_the_engine_to_pass_input_through() {
    let (_tree, _taps, _node, wv, records) =
        page_under_a_tappable_ancestor(WebViewInput::Transparent);
    assert!(
        records.ops_for(wv).iter().any(|op| matches!(
            op,
            WebViewOp::SetInputPassthrough {
                passthrough: true,
                ..
            }
        )),
        "a transparent page must ask its engine to stop taking input, got {:?}",
        records.ops_for(wv)
    );

    let (_tree, _taps, _node, wv, records) = page_under_a_tappable_ancestor(WebViewInput::Native);
    assert!(
        !records
            .ops_for(wv)
            .iter()
            .any(|op| matches!(op, WebViewOp::SetInputPassthrough { .. })),
        "a native page must not ask, got {:?}",
        records.ops_for(wv)
    );
}

/// The other half of "the painted rectangle is the contract": a press that
/// *missed* the page is never re-attributed to it.
///
/// The geometry is deliberate. A10's miss-only top-up is
/// `(target_size − min(w, h)) / 2`, so it is exactly zero for any page bigger
/// than the density's target size — which is nearly every page, and there
/// `no_hit_slop` changes nothing observable. It bites on a *small* one: a 16 dp
/// page has a 4 dp top-up at Compact, and this test presses 2 dp outside it in
/// dead space, where no eligible handler competes.
#[test]
fn a_small_native_page_is_never_handed_a_press_that_missed_it() {
    let (registry, _records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank");
    let stack = VStack::new()
        .child(FixedSize::new().width(16.0).height(16.0).child(webview))
        .child(MinSize::new(0.0, 200.0));
    let root = tree.add(stack);
    layout(&mut tree);
    let node = find_web_view(&tree, root).expect("the web view is in the tree");
    let bounds = tree.bounds(node);
    assert!(
        (bounds.height - 16.0).abs() < 0.01,
        "the fixture needs a page small enough for a top-up: {bounds:?}"
    );

    let just_below = Point::new(bounds.center().x, bounds.bottom() + 2.0);
    let finger = teksilo_core::pointer::PointerInfo::touch(
        tree.new_contact(),
        teksilo_core::pointer::EventTime::ZERO,
    );
    assert_ne!(
        tree.hit_test_for(just_below, &finger),
        Some(node),
        "a press that missed the page must not be re-attributed to it"
    );
}

// ---------------------------------------------------------------------------
// where the subview may be
// ---------------------------------------------------------------------------

/// A subview is parented to the top-level window, so no clipping ancestor
/// clips it for us. The widget mirrors what survives its clip chain: the
/// visible strip while some of the page is in view, and nothing at all —
/// `set_visible(false)` — once the viewport has scrolled past it.
#[test]
fn a_page_scrolled_out_of_its_viewport_is_hidden_and_a_partly_clipped_one_is_mirrored_clipped() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    let scroller = teksilo_widgets::ScrollArea::new().child(
        VStack::new()
            .child(FixedSize::new().width(300.0).height(200.0).child(webview))
            .child(MinSize::new(0.0, 4000.0)),
    );
    let offset = scroller.scroll_y_signal().clone();
    tree.add(scroller);
    layout(&mut tree);

    let full = last_bounds(&records, wv_id).expect("the page's bounds were mirrored");
    assert!(
        full.height > 0.0,
        "an unclipped page is mirrored at its own height, got {full:?}"
    );

    // Half of the page above the viewport's top edge: the mirrored rectangle
    // is the strip that is still inside it.
    offset.set(100.0);
    layout(&mut tree);
    let clipped = last_bounds(&records, wv_id).expect("a clipped page is still mirrored");
    assert!(
        clipped.height < full.height,
        "a half-scrolled page must be mirrored at the visible strip, got {clipped:?} of {full:?}"
    );
    assert_eq!(
        records.visibility_log(wv_id),
        Vec::<bool>::new(),
        "a partly visible page stays visible"
    );

    // Past its own height: nothing of it is in view.
    offset.set(400.0);
    layout(&mut tree);
    assert_eq!(
        records.visibility_log(wv_id),
        vec![false],
        "a page scrolled out of view must hide its engine subview"
    );

    // …and it comes back.
    offset.set(0.0);
    layout(&mut tree);
    assert_eq!(
        records.visibility_log(wv_id),
        vec![false, true],
        "scrolling back must show it again"
    );
}

fn last_bounds(records: &MemoryWebViewRecords, id: WebViewId) -> Option<Rect> {
    records.ops_for(id).iter().rev().find_map(|op| match op {
        WebViewOp::SetBounds { bounds, .. } => Some(*bounds),
        _ => None,
    })
}

/// An overlay renders in the wgpu pass, i.e. *under* the engine subview, and
/// the OS routes a press over that region to the engine. So while one stands
/// over the page the subview stands down — which is the only way a menu, a
/// popover or a modal dialog over a web view is both visible and operable.
#[test]
fn an_overlay_standing_over_the_page_hides_the_engine_subview() {
    use teksilo_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};

    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    let anchor = tree.add(webview);
    let panel = tree.add(MinSize::new(200.0, 120.0));
    tree.set_dormant(panel);
    layout(&mut tree);
    let _ = tree.render();
    assert_eq!(
        records.visibility_log(wv_id),
        Vec::<bool>::new(),
        "nothing covers the page yet"
    );

    tree.activate(panel);
    let overlay = tree.show_overlay(OverlayRequest {
        content_id: panel,
        anchor,
        placement: OverlayPlacement::Centered,
        dismiss: DismissBehavior::EscapeOrClickOutside,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: None,
        fade_duration: None,
    });
    layout(&mut tree);
    let _ = tree.render();
    assert_eq!(
        records.visibility_log(wv_id),
        vec![false],
        "an overlay over the page must stand the subview down"
    );

    tree.dismiss_overlay(overlay);
    layout(&mut tree);
    let _ = tree.render();
    assert_eq!(
        records.visibility_log(wv_id),
        vec![false, true],
        "and dismissing it must bring the page back"
    );
}

// ---------------------------------------------------------------------------
// the two focus rings
// ---------------------------------------------------------------------------

/// A web view has two disjoint focus rings and the engine's is the one Teksilo
/// cannot see. When the page takes the keyboard, the toolkit's focus follows —
/// otherwise whatever held it goes on believing it still does, caret blinking.
#[test]
fn the_engine_taking_the_keyboard_moves_the_toolkits_focus_onto_the_frame() {
    let (registry, _records) = memory_registry();
    let mut tree = tree_with_registry(registry.clone());

    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    let page_focused = webview.page_focused_signal();
    let frame = tree.add(webview);
    let elsewhere = tree.add(WidgetBuilder::focusable(
        teksilo_widgets::primitives::RectWidget::new(),
        true,
    ));
    layout(&mut tree);

    tree.focus(elsewhere);
    assert_eq!(tree.focused(), Some(elsewhere));

    let win = TeksiloWindowId::new(0);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        registry.deliver(
            WebViewEventPayload {
                window_id_owner: win,
                web_view_id: wv_id,
                event: WebViewEvent::EngineFocusChanged(true),
            },
            ctx,
        );
    });

    assert!(page_focused.get(), "the page's own focus must be published");
    assert_eq!(
        tree.focused(),
        Some(frame),
        "the toolkit's focus must follow the OS onto the frame"
    );

    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        registry.deliver(
            WebViewEventPayload {
                window_id_owner: win,
                web_view_id: wv_id,
                event: WebViewEvent::EngineFocusChanged(false),
            },
            ctx,
        );
    });
    assert!(!page_focused.get(), "…and be published when it is given up");
    assert_eq!(
        tree.focused(),
        Some(frame),
        "a blur says nothing about where focus went, so it moves nothing"
    );
}
