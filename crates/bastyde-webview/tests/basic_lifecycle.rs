//! Headless lifecycle tests for `WebView`, driven entirely against the
//! `MemoryWebViewBackend` — no GPU, no window, no real engine.
//!
//! The headline test (`webview_in_switcher_hides_native_subview_on_tab_away`)
//! is the regression guard for the dormancy/visibility bridge: a `WebView`
//! parked in a non-selected `Switcher` branch MUST issue `set_visible(false)`,
//! and `set_visible(true)` when re-selected. This is the one place a native
//! subview's visibility cannot ride the wgpu paint pass.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::SizeProposal;
use bastyde_core::event_source::TreeAppContext;
use bastyde_core::presets::intui;
use bastyde_core::signal::Signal;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_widgets::primitives::Switcher;

use bastyde_webview::{MemoryWebViewRecords, WebView, WebViewOp, WebViewRegistry, memory_registry};

/// Build a tree with the `WebViewRegistry` installed in app-state (mirrors how
/// `install_web_view` registers it at the app level).
fn tree_with_registry(registry: WebViewRegistry) -> WidgetTree {
    let mut map: HashMap<TypeId, Box<dyn Any>> = HashMap::new();
    map.insert(TypeId::of::<WebViewRegistry>(), Box::new(registry));
    let mut tree = WidgetTree::new().with_theme(intui::light());
    tree.set_app_context(Rc::new(TreeAppContext::empty().with_app_state(map)));
    tree
}

fn layout(tree: &mut WidgetTree) {
    tree.layout(SizeProposal::exact(800.0, 600.0));
}

#[test]
fn opens_and_loads_initial_url() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("https://example.com");
    let wv_id = webview.id();
    tree.add(webview);
    layout(&mut tree);

    let ops = records.ops_for(wv_id);
    assert!(
        matches!(ops.first(), Some(WebViewOp::Open { .. })),
        "first op must be Open, got {ops:?}"
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, WebViewOp::LoadUrl { url, .. } if url == "https://example.com")),
        "initial URL must be loaded, got {ops:?}"
    );
}

#[test]
fn tracks_bounds_on_layout() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    tree.add(webview);
    layout(&mut tree);

    assert!(
        records
            .ops_for(wv_id)
            .iter()
            .any(|op| matches!(op, WebViewOp::SetBounds { .. })),
        "a SetBounds must be issued once layout assigns bounds"
    );
}

/// THE headline regression test for the dormancy/visibility bridge.
#[test]
fn webview_in_switcher_hides_native_subview_on_tab_away() {
    let (registry, records): (WebViewRegistry, MemoryWebViewRecords) = memory_registry();
    let mut tree = tree_with_registry(registry);

    // Tab 0 = the WebView ("Browser"); tab 1 = plain content ("Native UI").
    let selected = Signal::new(0_usize);
    let webview = WebView::new().url("app://index.html");
    let wv_id = webview.id();
    let switcher_id = tree.add(
        Switcher::new(selected.clone())
            .child(webview)
            .child(bastyde_widgets::primitives::Spacer::new()),
    );
    layout(&mut tree);

    assert!(
        records
            .ops()
            .iter()
            .any(|op| matches!(op, WebViewOp::Open { web_view_id } if *web_view_id == wv_id)),
        "WebView must open on the initially-selected tab"
    );

    // Active tab: no visibility toggles yet (it opened visible).
    assert_eq!(
        records.visibility_log(wv_id),
        Vec::<bool>::new(),
        "freshly-opened, still-selected WebView issues no set_visible"
    );

    // Tab away to "Native UI" — the WebView's page is parked dormant.
    selected.set(1);
    layout(&mut tree);
    assert_eq!(
        records.visibility_log(wv_id),
        vec![false],
        "tab-away MUST hide the native subview (set_visible(false))"
    );

    // Tab back to "Browser" — the WebView re-activates.
    selected.set(0);
    layout(&mut tree);
    assert_eq!(
        records.visibility_log(wv_id),
        vec![false, true],
        "tab-back MUST re-show the native subview (set_visible(true))"
    );

    // And one more round-trip stays consistent.
    selected.set(1);
    layout(&mut tree);
    assert_eq!(records.visibility_log(wv_id), vec![false, true, false]);

    let _ = switcher_id;
}

/// Dropping the WebView (tree teardown) tears down the engine handle (RAII).
#[test]
fn dropping_widget_tears_down_handle() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);
    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    tree.add(webview);
    layout(&mut tree);

    drop(tree);

    assert!(
        records
            .ops_for(wv_id)
            .iter()
            .any(|op| matches!(op, WebViewOp::Dropped { .. })),
        "engine handle must drop when the widget/tree is torn down"
    );
}
