// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Toolkit unit tests, all driven through [`execute`]. Every test that
//! snapshots the tree validates the produced `TreeUpdate` with the real
//! `accesskit_consumer`, exactly as bastyde-core's own AT tests do.

use bastyde_canvas::SizeProposal;
use bastyde_core::WidgetTree;
use bastyde_core::accesskit;
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, WidgetEvent};
use bastyde_core::gesture::TapEvent;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;

use crate::dto::*;
use crate::executor::execute;
use crate::recording_ops::RecordingWindowOps;

// ---------------------------------------------------------------------------
// A configurable probe widget — the unit-test fixture. The toolkit depends
// only on bastyde-core, so it can't reach `bastyde-widgets`; this minimal
// widget exercises every path: role/label/value round-trip, reactive
// (AccessibilityOnly) bindings, AT actions (Click / SetValue), live
// regions, typed text, taps, and an optional window-opening action.
// ---------------------------------------------------------------------------

struct Probe {
    role: accesskit::Role,
    label: Signal<String>,
    value: Option<Signal<String>>,
    live: Option<accesskit::Live>,
    focusable: bool,
    accept_set_value: bool,
    opens_window: bool,
    tristate_mixed: bool,
    /// Attach a `.context_menu(..)` factory that mounts a `Role::Menu` child
    /// labelled "context-menu" — the fixture for right-click / ShowContextMenu.
    has_context_menu: bool,
    /// When true, also advertise + explicitly handle `Action::ShowContextMenu`
    /// (bumping `clicks`) so a test can prove the widget's own handler wins over
    /// the factory fallback.
    handles_show_context_menu: bool,
    clicks: Signal<u64>,
    taps: Signal<u64>,
    typed: Signal<String>,
    /// Each received KeyDown, tagged `named:<Display>` or `char:<c>` so a test
    /// can tell `Key::S` (a named variant) from `Key::Character('s')`.
    received: Signal<Vec<String>>,
}

impl std::fmt::Debug for Probe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Probe").field("role", &self.role).finish()
    }
}

/// The window an `opens_window()` probe asks for, whichever route reached it.
fn probe_child_window() -> bastyde_core::window::WindowConfig {
    bastyde_core::window::WindowConfig::new()
        .title("probe child")
        .id("probe-child")
        .size(200, 100)
}

impl Probe {
    fn new(role: accesskit::Role, label: &str) -> Self {
        Self {
            role,
            label: Signal::new(label.to_string()),
            value: None,
            live: None,
            focusable: true,
            accept_set_value: false,
            opens_window: false,
            tristate_mixed: false,
            has_context_menu: false,
            handles_show_context_menu: false,
            clicks: Signal::new(0),
            taps: Signal::new(0),
            typed: Signal::new(String::new()),
            received: Signal::new(Vec::new()),
        }
    }
    fn value(mut self, v: Signal<String>) -> Self {
        self.value = Some(v);
        self.accept_set_value = true;
        self
    }
    fn live(mut self, l: accesskit::Live) -> Self {
        self.live = Some(l);
        self
    }
    fn opens_window(mut self) -> Self {
        self.opens_window = true;
        self
    }
    /// Emit `Toggled::Mixed` (tristate / indeterminate).
    fn mixed(mut self) -> Self {
        self.tristate_mixed = true;
        self
    }
    /// Attach a context-menu factory (see [`Probe::has_context_menu`]).
    fn with_context_menu(mut self) -> Self {
        self.has_context_menu = true;
        self
    }
    /// Advertise + explicitly handle `Action::ShowContextMenu` in the widget's
    /// own handler (see [`Probe::handles_show_context_menu`]).
    fn handling_show_context_menu(mut self) -> Self {
        self.handles_show_context_menu = true;
        self
    }
}

impl Widget for Probe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        {
            let registry = ctx.binding_registry();
            self.label
                .bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
            if let Some(v) = &self.value {
                v.bind_to(self_id, registry, BindingLevel::AccessibilityOnly);
            }
        }

        let clicks = self.clicks.clone();
        let value_sig = self.value.clone();
        let accept_set = self.accept_set_value;
        let opens_window = self.opens_window;
        // The same "this command opens a window" behaviour, reachable through
        // the two *synthetic input* routes as well as the AT action — that is
        // what proves the executor keeps its real `WindowOps` on every path and
        // not just the AT one (see `executor`'s "Synthetic input" section).
        let opens_on_tap = self.opens_window;
        let opens_on_key = self.opens_window;
        let handles_show = self.handles_show_context_menu;
        let taps = self.taps.clone();
        let typed = self.typed.clone();
        let received = self.received.clone();

        let mut handlers = HandlerSet::new();
        if self.focusable {
            handlers = handlers.focusable(true);
        }
        handlers = handlers
            .on_access_action_request(move |action, _node, data, ctx| match action {
                accesskit::Action::Click => {
                    clicks.set(clicks.get() + 1);
                    if opens_window {
                        ctx.open_window(probe_child_window());
                    }
                    EventResponse::Handled
                }
                accesskit::Action::SetValue if accept_set => {
                    if let Some(accesskit::ActionData::Value(s)) = data
                        && let Some(v) = &value_sig
                    {
                        v.set(s.to_string());
                    }
                    EventResponse::Handled
                }
                accesskit::Action::ShowContextMenu if handles_show => {
                    // Prove the widget's own handler wins over the factory
                    // fallback: bump `clicks` and claim the action.
                    clicks.set(clicks.get() + 1);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            })
            .on_tap(move |_e: &TapEvent, ctx| {
                taps.set(taps.get() + 1);
                if opens_on_tap {
                    ctx.open_window(probe_child_window());
                }
            })
            .on_key(move |event, ctx| {
                if let WidgetEvent::KeyDown { key, .. } = event {
                    if opens_on_key {
                        ctx.open_window(probe_child_window());
                    }
                    let mut log = received.get();
                    log.push(match key {
                        Key::Character(c) => format!("char:{c}"),
                        other => format!("named:{other}"),
                    });
                    received.set(log);
                    if let Key::Character(ch) = key {
                        let mut s = typed.get();
                        s.push(*ch);
                        typed.set(s);
                    }
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        if self.has_context_menu {
            handlers = handlers.context_menu(|_pos, _ctx| {
                Some(Box::new(Probe::new(accesskit::Role::Menu, "context-menu")) as Box<dyn Widget>)
            });
        }
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(120.0, 30.0).into()
    }

    fn accessibility(&self, builder: &mut bastyde_core::AccessNodeBuilder) {
        builder.set_role(self.role);
        builder.set_name(self.label.get());
        if let Some(v) = &self.value {
            builder.set_value(v.get());
        }
        if let Some(live) = self.live {
            builder.set_live(live);
        }
        if self.tristate_mixed {
            builder.inner_mut().set_toggled(accesskit::Toggled::Mixed);
        }
        builder.add_action(accesskit::Action::Focus);
        builder.add_action(accesskit::Action::Click);
        if self.accept_set_value {
            builder.add_action(accesskit::Action::SetValue);
        }
        if self.handles_show_context_menu {
            builder.add_action(accesskit::Action::ShowContextMenu);
        }
    }
}

/// A minimal presentational container: holds one child, emits NO AT role (so
/// the accessibility walk prunes it), but is a real arena widget with bounds.
/// Used to prove `layout_tree` sees what the AT tree doesn't.
#[derive(Debug)]
struct Container {
    pending: Option<Box<dyn Widget>>,
    child_id: Option<WidgetId>,
}

impl Container {
    fn new(child: impl Widget + 'static) -> Self {
        Self {
            pending: Some(Box::new(child)),
            child_id: None,
        }
    }
}

impl Widget for Container {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(child) = self.pending.take() {
            self.child_id = Some(ctx.add_boxed(child));
        }
        self.child_id.into_iter().collect()
    }
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(200.0, 100.0).into()
    }
    fn place_children(
        &self,
        bounds: bastyde_canvas::Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }
    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
    // No `accessibility` override → bare presentational node, pruned from the AT tree.
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn node_ref(id: WidgetId) -> NodeRef {
    bastyde_core::accessibility::widget_id_to_node_id(id).0
}

/// Validate a fresh `TreeUpdate` with the real AccessKit consumer — the
/// same conformance gate bastyde-core's own AT tests use.
fn assert_valid(tree: &mut WidgetTree) {
    let update = tree.sync_accessibility();
    let _consumer = accesskit_consumer::Tree::new(update, false);
}

fn laid_out(probe: Probe) -> (WidgetTree, WidgetId) {
    let mut tree = WidgetTree::new();
    let id = tree.add(probe);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, id)
}

fn default_settle() -> SettleSpec {
    SettleSpec::default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn snapshot_round_trips_role_and_label() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Save"));
    assert_valid(&mut tree);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::SnapshotTree { max_depth: None },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("expected ok, got {reply:?}");
    };
    let nodes = data["nodes"].as_array().unwrap();
    let probe = nodes
        .iter()
        .find(|n| n["id"].as_u64() == Some(node_ref(id)))
        .expect("probe node present");
    assert_eq!(probe["role"], "Button");
    assert_eq!(probe["label"], "Save");
    assert!(
        probe["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "click")
    );
}

#[test]
fn read_node_returns_semantic_node() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Open"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::ReadNode { node: node_ref(id) },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    let sn: SemanticNode = serde_json::from_value(data).unwrap();
    assert_eq!(sn.role, "Button");
    assert_eq!(sn.label.as_deref(), Some("Open"));
}

#[test]
fn read_missing_node_is_not_found() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "X"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::ReadNode { node: 999_999 },
        &default_settle(),
    );
    assert!(matches!(reply, AutomationReply::Err { code, .. } if code == codes::NOT_FOUND));
}

#[test]
fn layout_tree_includes_widgets_the_at_tree_prunes() {
    // A presentational Container wrapping a Button: the Container has no AT
    // role (pruned from the AT tree) but is a real arena widget.
    let mut tree = WidgetTree::new();
    let _root = tree.add(Container::new(Probe::new(accesskit::Role::Button, "Inner")));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let mut ops = RecordingWindowOps::new();

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::LayoutTree {
            max_depth: None,
            include_debug: false,
        },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    let nodes: Vec<LayoutNode> = serde_json::from_value(data["nodes"].clone()).unwrap();
    let types: Vec<&str> = nodes.iter().map(|n| n.type_name.as_str()).collect();
    assert!(
        types.iter().any(|t| t.contains("Container")),
        "the presentational Container is in the layout tree: {types:?}"
    );
    assert!(
        types.iter().any(|t| t.contains("Probe")),
        "the Button is too: {types:?}"
    );
    // Every node carries real bounds (the Container is 400x300 — the root fills
    // the proposal — and is active).
    let container = nodes
        .iter()
        .find(|n| n.type_name.contains("Container"))
        .unwrap();
    assert!(container.active);
    assert!(container.bounds.width > 0.0 && container.bounds.height > 0.0);
    assert!(
        !container.children.is_empty(),
        "Container has the Button as a child"
    );

    // Contrast: the Container is absent from the AT snapshot (no role).
    let at = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::SnapshotTree { max_depth: None },
        &default_settle(),
    );
    let AutomationReply::Ok { data: at } = at else {
        panic!();
    };
    let at_roles: Vec<&str> = at["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["role"].as_str())
        .collect();
    assert!(at_roles.contains(&"Button"), "Button is in the AT tree");
}

#[test]
fn inspect_node_returns_type_bounds_and_debug() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Inspect"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InspectNode { node: node_ref(id) },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    let ln: LayoutNode = serde_json::from_value(data).unwrap();
    assert_eq!(ln.id, node_ref(id));
    assert!(ln.type_name.contains("Probe"), "type: {}", ln.type_name);
    assert!(ln.active);
    // The Debug repr (the inspector's Properties data) is present.
    assert!(ln.debug.as_deref().unwrap_or("").contains("Probe"));

    // A made-up id resolves to nothing.
    let missing = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InspectNode { node: 7 },
        &default_settle(),
    );
    assert!(matches!(missing, AutomationReply::Err { code, .. } if code == codes::NOT_FOUND));
}

#[test]
fn find_node_by_role_and_label() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Find Me"));
    let mut ops = RecordingWindowOps::new();
    for op in [
        AutomationOp::FindNode {
            role: Some("Button".into()),
            label: None,
        },
        AutomationOp::FindNode {
            role: None,
            label: Some("Find Me".into()),
        },
        AutomationOp::FindNode {
            role: Some("button".into()), // case-insensitive
            label: Some("Find Me".into()),
        },
    ] {
        let reply = execute(&mut tree, &mut ops, &op, &default_settle());
        let AutomationReply::Ok { data } = reply else {
            panic!("{op:?} -> err");
        };
        assert_eq!(data["node"].as_u64(), Some(node_ref(id)), "for {op:?}");
    }
}

#[test]
fn find_node_no_match_is_null() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "X"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::FindNode {
            role: Some("Slider".into()),
            label: None,
        },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    assert!(data["node"].is_null());
}

#[test]
fn assert_node_role_pass_and_fail() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let pass = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::AssertNode {
            node: node_ref(id),
            assertion: Assertion::RoleEquals {
                value: "Button".into(),
            },
        },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = pass else {
        panic!();
    };
    let res: AssertionResult = serde_json::from_value(data).unwrap();
    assert!(res.passed);

    let fail = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::AssertNode {
            node: node_ref(id),
            assertion: Assertion::RoleEquals {
                value: "Slider".into(),
            },
        },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = fail else {
        panic!();
    };
    let res: AssertionResult = serde_json::from_value(data).unwrap();
    assert!(!res.passed);
    assert!(res.detail.is_some());
}

#[test]
fn assert_exists_on_missing_node_fails_gracefully() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::AssertNode {
            node: 12345,
            assertion: Assertion::Exists,
        },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    let res: AssertionResult = serde_json::from_value(data).unwrap();
    assert!(!res.passed);
}

#[test]
fn invoke_click_fires_handler() {
    let probe = Probe::new(accesskit::Role::Button, "Click");
    let clicks = probe.clicks.clone();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InvokeAction {
            node: node_ref(id),
            action: "click".into(),
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(clicks.get(), 1);
}

#[test]
fn invoke_unknown_action_is_unknown_name() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InvokeAction {
            node: node_ref(id),
            action: "frobnicate".into(),
        },
        &default_settle(),
    );
    assert!(matches!(reply, AutomationReply::Err { code, .. } if code == codes::UNKNOWN_NAME));
}

#[test]
fn invoke_on_missing_node_is_not_found() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InvokeAction {
            node: 424242,
            action: "click".into(),
        },
        &default_settle(),
    );
    assert!(matches!(reply, AutomationReply::Err { code, .. } if code == codes::NOT_FOUND));
}

#[test]
fn set_value_updates_and_resnapshots() {
    let value = Signal::new("before".to_string());
    let probe = Probe::new(accesskit::Role::TextInput, "Field").value(value.clone());
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::SetValue {
            node: node_ref(id),
            value: "after".into(),
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(value.get(), "after");

    // Re-snapshot reflects the new value.
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::ReadNode { node: node_ref(id) },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!();
    };
    let sn: SemanticNode = serde_json::from_value(data).unwrap();
    assert_eq!(sn.value.as_deref(), Some("after"));
}

#[test]
fn focus_then_type_text_routes_to_target() {
    let probe = Probe::new(accesskit::Role::TextInput, "Field");
    let typed = probe.typed.clone();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::TypeText {
            node: node_ref(id),
            text: "hi".into(),
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(typed.get(), "hi");
}

#[test]
fn inject_pointer_click_taps_widget() {
    let probe = Probe::new(accesskit::Role::Button, "Tappable");
    let taps = probe.taps.clone();
    let (mut tree, id) = laid_out(probe);
    let bounds = tree.bounds(id);
    let (cx, cy) = (
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    );
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InjectPointer {
            x: cx,
            y: cy,
            action: PointerAction::Click,
            button: PointerButtonDto::Primary,
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(taps.get(), 1);
}

#[test]
fn inject_key_letter_maps_to_named_variant() {
    // Regression: a single ASCII letter must become `Key::S` (named), not
    // `Key::Character('s')`, or it would never fire a letter shortcut.
    let probe = Probe::new(accesskit::Role::Button, "B");
    let received = probe.received.clone();
    let (mut tree, id) = laid_out(probe);
    tree.focus(id);
    let mut ops = RecordingWindowOps::new();

    let mk = |key: &str| AutomationOp::InjectKey {
        key: key.into(),
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    execute(&mut tree, &mut ops, &mk("s"), &default_settle());
    execute(&mut tree, &mut ops, &mk("/"), &default_settle());

    let log = received.get();
    assert!(
        log.contains(&"named:S".to_string()),
        "letter → Key::S, got {log:?}"
    );
    assert!(
        log.contains(&"char:/".to_string()),
        "non-letter → Character, got {log:?}"
    );
}

#[test]
fn assert_toggled_false_fails_on_mixed() {
    // Regression: `Toggled { value: false }` must FAIL on a tristate/Mixed
    // node, not silently pass by collapsing Mixed into false.
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::CheckBox, "cb").mixed());
    let mut ops = RecordingWindowOps::new();
    for value in [false, true] {
        let reply = execute(
            &mut tree,
            &mut ops,
            &AutomationOp::AssertNode {
                node: node_ref(id),
                assertion: Assertion::Toggled { value },
            },
            &default_settle(),
        );
        let AutomationReply::Ok { data } = reply else {
            panic!("{reply:?}");
        };
        let res: AssertionResult = serde_json::from_value(data).unwrap();
        assert!(
            !res.passed,
            "Mixed must not satisfy Toggled {{ value: {value} }}"
        );
        assert!(res.detail.unwrap().contains("mixed"));
    }
}

#[test]
fn inject_key_unknown_is_unknown_name() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InjectKey {
            key: "NopeKey".into(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        },
        &default_settle(),
    );
    assert!(matches!(reply, AutomationReply::Err { code, .. } if code == codes::UNKNOWN_NAME));
}

#[test]
fn settle_terminates_on_static_tree() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &default_settle(),
    );
    assert!(reply.is_ok(), "settle should not time out on a static tree");
}

#[test]
fn pull_announcements_captures_changes_and_dedups() {
    let label = Signal::new("Ready".to_string());
    let probe = Probe::new(accesskit::Role::Label, "Ready").live(accesskit::Live::Polite);
    // Reuse the probe's own label signal so we can mutate the announced text.
    let probe = Probe {
        label: label.clone(),
        ..probe
    };
    let (mut tree, _id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();

    // Prime: drain whatever the first sync produced.
    let _ = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::PullAnnouncements { since_seq: 0 },
        &default_settle(),
    );
    let baseline = pull(&mut tree, &mut ops, 0)
        .last()
        .map(|a| a.seq)
        .unwrap_or(0);

    // Change the announced text → one new announcement.
    label.set("Saved".to_string());
    let _ = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &default_settle(),
    );
    let after_first = pull(&mut tree, &mut ops, baseline);
    assert_eq!(after_first.len(), 1, "one announcement after a change");
    assert_eq!(after_first[0].text, "Saved");
    let seq1 = after_first[0].seq;

    // No change → no new announcement (dedup).
    label.set("Saved".to_string());
    let _ = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &default_settle(),
    );
    assert!(
        pull(&mut tree, &mut ops, seq1).is_empty(),
        "identical text must not re-announce"
    );

    // New change → another announcement.
    label.set("Closed".to_string());
    let _ = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &default_settle(),
    );
    let after_second = pull(&mut tree, &mut ops, seq1);
    assert_eq!(after_second.len(), 1);
    assert_eq!(after_second[0].text, "Closed");
    let seq2 = after_second[0].seq;

    // Regression: clearing the live region then re-setting the SAME text must
    // re-announce (the cleared state must not leave a stale dedup entry).
    label.set(String::new());
    let _ = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &default_settle(),
    );
    assert!(
        pull(&mut tree, &mut ops, seq2).is_empty(),
        "empty text does not announce"
    );
    label.set("Closed".to_string());
    let _ = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &default_settle(),
    );
    let after_reappear = pull(&mut tree, &mut ops, seq2);
    assert_eq!(
        after_reappear.len(),
        1,
        "same text after a clear re-announces"
    );
    assert_eq!(after_reappear[0].text, "Closed");
}

fn pull(tree: &mut WidgetTree, ops: &mut RecordingWindowOps, since: u64) -> Vec<AnnouncementDto> {
    let reply = execute(
        tree,
        ops,
        &AutomationOp::PullAnnouncements { since_seq: since },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    serde_json::from_value(data).unwrap()
}

#[test]
fn wait_for_condition_succeeds_and_times_out() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Wait"));
    let mut ops = RecordingWindowOps::new();

    // Already-true condition resolves immediately.
    let ok = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::WaitForCondition {
            condition: WaitCondition::NodeExists {
                role: Some("Button".into()),
                label: None,
            },
        },
        &SettleSpec {
            settle_timeout_ms: 200,
            ..Default::default()
        },
    );
    assert!(ok.is_ok(), "{ok:?}");

    // Impossible condition times out.
    let timeout = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::WaitForCondition {
            condition: WaitCondition::NodeValue {
                node: node_ref(id),
                expected: "never".into(),
            },
        },
        &SettleSpec {
            settle_timeout_ms: 80,
            ..Default::default()
        },
    );
    assert!(matches!(timeout, AutomationReply::Err { code, .. } if code == codes::WAIT_TIMEOUT));
}

#[test]
fn recording_window_ops_captures_open_without_panic() {
    let probe = Probe::new(accesskit::Role::Button, "New Window").opens_window();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InvokeAction {
            node: node_ref(id),
            action: "click".into(),
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(
        ops.opened.len(),
        1,
        "open_window must be recorded, not panic"
    );
    assert_eq!(ops.opened[0].title, "probe child");
    assert_eq!(ops.opened[0].string_id.as_deref(), Some("probe-child"));
}

// ---------------------------------------------------------------------------
// Synthetic input must carry the caller's `WindowOps`
// ---------------------------------------------------------------------------
//
// The regression: `InjectPointer`/`InjectKey`/`TypeText`/`DragNode` went through
// `WidgetTree`'s test API, which dispatches with a `NoopWindowOps` — whose
// `open_window` *panics*. Against a live app the executor is handed the real
// ops and threw them away, so an injected click or keystroke on any command
// that opens a window killed the whole application. The AT-action path above
// never had the bug; these two are its synthetic-input counterparts.
//
// `RecordingWindowOps` is what makes the failure legible here: with the bug, the
// panic came from `NoopWindowOps` deep inside the tree; without it, the request
// lands in `ops.opened` where it can be asserted.

#[test]
fn an_injected_click_reaches_the_callers_window_ops() {
    let probe = Probe::new(accesskit::Role::Button, "New Window").opens_window();
    let (mut tree, id) = laid_out(probe);
    let bounds = tree.bounds(id);
    let mut ops = RecordingWindowOps::new();

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InjectPointer {
            x: bounds.center().x,
            y: bounds.center().y,
            action: PointerAction::Click,
            button: Default::default(),
        },
        &default_settle(),
    );

    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(
        ops.opened.len(),
        1,
        "a synthetic click must reach the caller's WindowOps, not a NoopWindowOps"
    );
    assert_eq!(ops.opened[0].string_id.as_deref(), Some("probe-child"));
}

#[test]
fn an_injected_key_reaches_the_callers_window_ops() {
    // `Probe` is focusable by default, which key routing needs.
    let probe = Probe::new(accesskit::Role::Button, "New Window").opens_window();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    // Key events route by focus, so focus the probe first — through the
    // executor, so this exercises the real op sequence a probe script uses.
    let focused = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::FocusNode { node: node_ref(id) },
        &default_settle(),
    );
    assert!(focused.is_ok(), "{focused:?}");

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InjectKey {
            key: "n".into(),
            ctrl: true,
            shift: true,
            alt: false,
            meta: false,
        },
        &default_settle(),
    );

    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(
        ops.opened.len(),
        1,
        "an injected keystroke must reach the caller's WindowOps, not a NoopWindowOps"
    );
    assert_eq!(ops.opened[0].string_id.as_deref(), Some("probe-child"));
}

#[test]
fn list_live_regions_finds_polite_node() {
    let probe = Probe::new(accesskit::Role::Label, "Status").live(accesskit::Live::Polite);
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::ListLiveRegions,
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    let regions: Vec<SemanticNode> = serde_json::from_value(data).unwrap();
    assert!(regions.iter().any(|n| n.id == node_ref(id)));
    assert_eq!(regions[0].live.as_deref(), Some("polite"));
}

#[test]
fn get_overlays_empty_on_plain_tree() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::GetOverlays,
        &default_settle(),
    );
    let AutomationReply::Ok { data } = reply else {
        panic!("{reply:?}");
    };
    assert_eq!(data["count"].as_u64(), Some(0));
}

#[test]
fn list_windows_and_screenshot_defer_to_host() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    for op in [
        AutomationOp::ListWindows,
        AutomationOp::Screenshot { node: None },
    ] {
        let reply = execute(&mut tree, &mut ops, &op, &default_settle());
        assert!(
            matches!(reply, AutomationReply::Err { ref code, .. } if code == codes::HOST_REQUIRED),
            "{op:?} -> {reply:?}"
        );
    }
}

#[test]
fn dto_round_trips_through_json() {
    // The socket protocol depends on every op + reply round-tripping.
    let req = AutomationRequest {
        window_id: Some(7),
        op: AutomationOp::SetValue {
            node: 42,
            value: "x".into(),
        },
        settle: SettleSpec::default(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: AutomationRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.window_id, Some(7));
    assert_eq!(back.op, req.op);

    let reply = AutomationReply::ok(serde_json::json!({"k": 1}));
    let s = serde_json::to_string(&reply).unwrap();
    let back: AutomationReply = serde_json::from_str(&s).unwrap();
    assert_eq!(back, reply);
}

// ---------------------------------------------------------------------------
// Right-click / context menus
// ---------------------------------------------------------------------------

/// Run `GetOverlays` and return the active-overlay count.
fn overlay_count(tree: &mut WidgetTree, ops: &mut RecordingWindowOps) -> u64 {
    let reply = execute(tree, ops, &AutomationOp::GetOverlays, &default_settle());
    let AutomationReply::Ok { data } = reply else {
        panic!("get_overlays: {reply:?}");
    };
    data["count"].as_u64().expect("count field")
}

#[test]
fn right_click_opens_the_context_menu_factory() {
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Row").with_context_menu());
    let mut ops = RecordingWindowOps::new();

    assert_eq!(overlay_count(&mut tree, &mut ops), 0, "no overlay before");

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::RightClick { node: node_ref(id) },
        &default_settle(),
    );
    assert!(reply.is_ok(), "right_click ok: {reply:?}");

    assert_eq!(
        overlay_count(&mut tree, &mut ops),
        1,
        "context menu overlay opened by right_click"
    );
    assert_valid(&mut tree);

    // The mounted menu is visible in the AT snapshot.
    let found = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::FindNode {
            role: Some("Menu".into()),
            label: None,
        },
        &default_settle(),
    );
    let AutomationReply::Ok { data } = found else {
        panic!("{found:?}");
    };
    assert!(data["node"].as_u64().is_some(), "menu node found: {data}");
}

#[test]
fn right_click_on_missing_node_is_not_found() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "Row").with_context_menu());
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::RightClick { node: 999_999 },
        &default_settle(),
    );
    assert!(
        matches!(reply, AutomationReply::Err { ref code, .. } if code == codes::NOT_FOUND),
        "expected NOT_FOUND, got {reply:?}"
    );
}

#[test]
fn show_context_menu_action_opens_the_factory_menu() {
    // The framework a11y route: `invoke_action(node, "show_context_menu")` on a
    // widget that wires its menu via `.context_menu(..)` (and does NOT handle the
    // AT action itself) now opens the menu — it used to be a silent no-op.
    let (mut tree, id) = laid_out(Probe::new(accesskit::Role::Button, "Row").with_context_menu());
    let mut ops = RecordingWindowOps::new();

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InvokeAction {
            node: node_ref(id),
            action: "show_context_menu".into(),
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "invoke show_context_menu ok: {reply:?}");
    assert_eq!(
        overlay_count(&mut tree, &mut ops),
        1,
        "show_context_menu AT action opened the factory menu"
    );
}

#[test]
fn show_context_menu_prefers_the_widgets_own_handler() {
    // A widget that explicitly handles `Action::ShowContextMenu` wins over the
    // factory fallback: its handler fires and NO factory overlay is mounted.
    let probe = Probe::new(accesskit::Role::Button, "Row")
        .with_context_menu()
        .handling_show_context_menu();
    let clicks = probe.clicks.clone();
    let mut tree = WidgetTree::new();
    let id = tree.add(probe);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let mut ops = RecordingWindowOps::new();

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InvokeAction {
            node: node_ref(id),
            action: "show_context_menu".into(),
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(clicks.get(), 1, "the widget's own handler fired");
    assert_eq!(
        overlay_count(&mut tree, &mut ops),
        0,
        "handler consumed the action — factory fallback skipped"
    );
}

#[test]
fn tool_catalog_has_27_entries() {
    assert_eq!(crate::mcp_schema::TOOL_COUNT, 27);
    // Names are unique.
    let mut names: Vec<&str> = crate::mcp_schema::TOOL_CATALOG
        .iter()
        .map(|t| t.name)
        .collect();
    names.sort_unstable();
    let unique = {
        let mut n = names.clone();
        n.dedup();
        n.len()
    };
    assert_eq!(unique, names.len(), "tool names must be unique");
}
