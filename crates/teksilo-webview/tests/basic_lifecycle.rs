// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use teksilo_canvas::SizeProposal;
use teksilo_core::NoopWindowOps;
use teksilo_core::binding::BindingLevel;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::presets::intui;
use teksilo_core::signal::Signal;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_core::window::TeksiloWindowId;
use teksilo_widgets::primitives::Switcher;

use teksilo_webview::{
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
    tree.run_mount_actions(&mut teksilo_core::NoopWindowOps);
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
            .child(teksilo_widgets::primitives::Spacer::new()),
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

/// Two-way `url_signal`: the binding's registration tick is treated as the
/// baseline (no spurious load — the initial page comes from `.url(...)`), but a
/// later external `set()` drives a navigation, and re-setting the same value
/// does not re-navigate (the guard filters the echo).
#[test]
fn inbound_url_binding_navigates() {
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let url = Signal::new(String::from("https://a.com"));
    let webview = WebView::new().url("https://a.com").url_signal(url.clone());
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
        "only the initial source load; the url_signal baseline must not navigate"
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

    // Headless windows register under TeksiloWindowId::new(0) (no real window).
    let win = TeksiloWindowId::new(0);
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
        ops.iter()
            .any(|op| matches!(op, WebViewOp::OpenDevtools { .. })),
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

#[test]
fn the_frame_is_in_the_tab_cycle_and_enter_hands_focus_to_the_engine() {
    // WCAG 2.1.1 / 2.4.3. Before this, `WebView` installed no HandlerSet at
    // all: the widget was never focusable, so Tab skipped straight past it and
    // embedded page content was unreachable without a pointer.
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("https://example.com");
    let wv_id = webview.id();
    let focused_signal = webview.focused_signal();
    let id = tree.add(webview);
    layout(&mut tree);

    // Tab reaches it, and the frame publishes its focus so the style can ring it.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(tree.focused(), Some(id), "Tab must reach the web view");
    assert!(focused_signal.get(), "the frame must publish its focus");

    // Landing on the frame does NOT enter the page — that is the two-step.
    assert!(
        !records
            .ops_for(wv_id)
            .iter()
            .any(|op| matches!(op, WebViewOp::SetFocus { .. })),
        "focusing the frame must not hand the keyboard to the engine"
    );

    // Enter does.
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert!(
        records
            .ops_for(wv_id)
            .iter()
            .any(|op| matches!(op, WebViewOp::SetFocus { .. })),
        "Enter must hand keyboard focus to the engine subview"
    );
}

#[test]
fn the_frame_is_not_a_keyboard_trap() {
    // The other half of 2.1.1: reaching it must not mean being stuck in it.
    // Every key except Enter / Space is declined, so Tab cycles off the frame
    // exactly as it would off any other control.
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

    let (registry, _records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let wv = tree.add(WebView::new().url("https://example.com"));
    let after = tree.add(teksilo_core::widget_builder::WidgetBuilder::focusable(
        teksilo_widgets::primitives::RectWidget::new(),
        true,
    ));
    layout(&mut tree);

    tree.focus(wv);
    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert_eq!(
        tree.focused(),
        Some(after),
        "Tab must move focus off the web view frame"
    );
}

#[test]
fn enter_page_on_focus_is_the_opt_in_one_step() {
    use teksilo_core::event::{Key, Modifiers, WidgetEvent};

    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new()
        .url("https://example.com")
        .enter_page_on_focus(true);
    let wv_id = webview.id();
    tree.add(webview);
    layout(&mut tree);

    tree.dispatch_event(WidgetEvent::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers::NONE,
        text: None,
    });
    assert!(
        records
            .ops_for(wv_id)
            .iter()
            .any(|op| matches!(op, WebViewOp::SetFocus { .. })),
        "with enter_page_on_focus, landing on the frame enters the page"
    );
}

#[test]
fn the_advertised_click_action_actually_enters_the_page() {
    // The §7 lesson from the accessibility audit: an action a widget declares
    // but never executes reports the control to AT as operable when it is not.
    let (registry, records) = memory_registry();
    let mut tree = tree_with_registry(registry);

    let webview = WebView::new().url("https://example.com");
    let wv_id = webview.id();
    let id = tree.add(webview);
    layout(&mut tree);

    let node = tree.accessibility_node(id);
    assert!(
        node.actions()
            .contains(&teksilo_core::accesskit::Action::Click),
        "the web-view frame must advertise Click"
    );
    assert!(
        node.actions()
            .contains(&teksilo_core::accesskit::Action::Focus),
        "the web-view frame must advertise Focus"
    );

    let handled = tree.dispatch_access_action(
        teksilo_core::accessibility::widget_id_to_node_id(id),
        teksilo_core::accesskit::Action::Click,
        None,
        &mut NoopWindowOps,
    );
    assert!(handled, "the advertised Click must be handled, not ignored");
    assert!(
        records
            .ops_for(wv_id)
            .iter()
            .any(|op| matches!(op, WebViewOp::SetFocus { .. })),
        "an AT-invoked Click must enter the page, not be a no-op"
    );
}
