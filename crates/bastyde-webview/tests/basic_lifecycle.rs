//! Headless lifecycle tests for `WebView`, driven entirely against the
//! `MemoryWebViewBackend` — no GPU, no window, no real engine.
//!
//! The headline test (`webview_in_switcher_hides_native_subview_on_tab_away`)
//! is the regression guard for the dormancy/visibility bridge: a `WebView`
//! parked in a non-selected `Switcher` branch MUST issue `set_visible(false)`,
//! and `set_visible(true)` when re-selected. This is the one place a native
//! subview's visibility cannot ride the wgpu paint pass.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bastyde_canvas::SizeProposal;
use bastyde_core::NoopWindowOps;
use bastyde_core::binding::BindingLevel;
use bastyde_core::event_source::TreeAppContext;
use bastyde_core::presets::intui;
use bastyde_core::signal::Signal;
use bastyde_core::widget_tree::WidgetTree;
use bastyde_core::window::BastydeWindowId;
use bastyde_widgets::primitives::Switcher;

use bastyde_webview::{
    MemoryWebViewRecords, WebView, WebViewEvent, WebViewEventPayload, WebViewOp, WebViewRegistry,
    memory_registry,
};

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
    // The app loop drains post-mount actions each iteration with a real
    // WindowOps; headless tests pump them with a Noop sink (parent handle is
    // None, app_state present). This is where the WebView opens its engine.
    tree.run_mount_actions(&mut bastyde_core::NoopWindowOps);
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

/// A WebView mounted while already parked dormant must open *hidden* — no
/// visible-then-hidden flash. Exercises the post-mount open reading the current
/// activation state.
#[test]
fn webview_opened_while_parked_starts_hidden() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    let node = tree.add(webview);

    // Park it dormant before the post-mount open runs.
    let visible = Signal::new(false);
    tree.visible_when(node, visible.clone());
    layout(&mut tree); // build → dormant → pump mount actions → open hidden

    assert_eq!(
        records.visibility_log(wv_id),
        vec![false],
        "a WebView opened while parked dormant must start hidden"
    );

    // Reveal it → set_visible(true).
    visible.set(true);
    layout(&mut tree);
    assert_eq!(records.visibility_log(wv_id), vec![false, true]);
}

/// Two-way `bind_url`: the binding's registration tick is treated as the
/// baseline (no spurious load — the initial page comes from `.url(...)`), but a
/// later external `set()` drives a navigation, and re-setting the same value
/// does not re-navigate (the guard filters the echo).
#[test]
fn inbound_url_binding_navigates() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let url = Signal::new(String::from("https://a.com"));
    let webview = WebView::new().url("https://a.com").bind_url(url.clone());
    let wv_id = webview.id();
    tree.add(webview);
    layout(&mut tree);

    // Only the initial source URL loaded — the binding's first tick did NOT add
    // a second navigation to the same baseline value.
    let loads = |records: &MemoryWebViewRecords| -> Vec<String> {
        records
            .ops_for(wv_id)
            .into_iter()
            .filter_map(|op| match op {
                WebViewOp::LoadUrl { url, .. } => Some(url),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        loads(&records),
        vec!["https://a.com".to_string()],
        "only the initial source load; the bind_url baseline must not navigate"
    );

    // External programmatic navigation.
    url.set("https://b.com".into());
    assert_eq!(
        loads(&records),
        vec!["https://a.com".to_string(), "https://b.com".to_string()],
        "an external url.set() must drive a load_url"
    );

    // Re-setting the same value must not re-navigate (guard filters it).
    url.set("https://b.com".into());
    assert_eq!(
        loads(&records),
        vec!["https://a.com".to_string(), "https://b.com".to_string()],
        "setting the already-current URL must not re-navigate"
    );
}

/// Download start/finish events delivered by a backend reach the
/// `on_download_started` / `on_download_finished` callbacks.
#[test]
fn download_events_reach_callbacks() {
    let (registry, _records) = memory_registry();
    let mut tree = tree_with_registry(registry.clone());

    let started: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let finished: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let started_cb = started.clone();
    let finished_cb = finished.clone();

    let webview = WebView::new()
        .url("about:blank")
        .on_download_started(move |d, _ctx| started_cb.borrow_mut().push(d.url))
        .on_download_finished(move |o, _ctx| finished_cb.borrow_mut().push(o.success));
    let wv_id = webview.id();
    tree.add(webview);
    layout(&mut tree);

    // Headless windows register under BastydeWindowId::new(0) (no real window).
    let win = BastydeWindowId::new(0);
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        registry.deliver(
            WebViewEventPayload {
                window_id_owner: win,
                web_view_id: wv_id,
                event: WebViewEvent::DownloadStarted {
                    url: "https://x/file.zip".into(),
                    suggested_path: "/tmp/file.zip".into(),
                },
            },
            ctx,
        );
    });
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        registry.deliver(
            WebViewEventPayload {
                window_id_owner: win,
                web_view_id: wv_id,
                event: WebViewEvent::DownloadFinished {
                    path: "/tmp/file.zip".into(),
                    success: true,
                },
            },
            ctx,
        );
    });

    assert_eq!(&*started.borrow(), &["https://x/file.zip".to_string()]);
    assert_eq!(&*finished.borrow(), &[true]);
}

/// The imperative devtools toggle reaches the engine handle (recorded by the
/// memory backend), invoked the way an app would: `ctx.with_widget_mut`.
#[test]
fn devtools_toggle_reaches_handle() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("about:blank");
    let wv_id = webview.id();
    let node = tree.add(webview);
    layout(&mut tree);

    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.with_widget_mut::<WebView>(node, BindingLevel::RepaintOnly, |w| w.open_devtools());
    });
    tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
        ctx.with_widget_mut::<WebView>(node, BindingLevel::RepaintOnly, |w| w.close_devtools());
    });

    let ops = records.ops_for(wv_id);
    assert!(
        ops.iter().any(|op| matches!(op, WebViewOp::OpenDevtools { .. })),
        "open_devtools must reach the handle, got {ops:?}"
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, WebViewOp::CloseDevtools { .. })),
        "close_devtools must reach the handle, got {ops:?}"
    );
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
