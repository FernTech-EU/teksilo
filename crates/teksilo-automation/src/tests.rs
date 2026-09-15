// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Toolkit unit tests, all driven through [`execute`]. Every test that
//! snapshots the tree validates the produced `TreeUpdate` with the real
//! `accesskit_consumer`, exactly as teksilo-core's own AT tests do.

use std::time::Duration;

use teksilo_canvas::SizeProposal;
use teksilo_core::WidgetTree;
use teksilo_core::accesskit;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::gesture::TapEvent;
use teksilo_core::signal::Signal;
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::{HandlerSet, WidgetBuilder};
use teksilo_core::widget_id::WidgetId;

use crate::dto::*;
use crate::executor::execute;
use crate::recording_ops::RecordingWindowOps;

// ---------------------------------------------------------------------------
// A configurable probe widget — the unit-test fixture. The toolkit depends
// only on teksilo-core, so it can't reach `teksilo-widgets`; this minimal
// widget exercises every path: role/label/value round-trip, reactive
// (AccessibilityOnly) bindings, AT actions (Click / SetValue), live
// regions, typed text, taps, and an optional window-opening action.
// ---------------------------------------------------------------------------

/// Held modifiers as `ctrl+shift`, or `none`. One formatter, so a press and a
/// scroll cannot describe the same modifiers two different ways.
fn mods_tag(modifiers: &teksilo_core::event::Modifiers) -> String {
    let mut out = String::new();
    for (on, tag) in [
        (modifiers.ctrl(), "ctrl"),
        (modifiers.shift(), "shift"),
        (modifiers.alt(), "alt"),
        (modifiers.super_key(), "meta"),
    ] {
        if on {
            if !out.is_empty() {
                out.push('+');
            }
            out.push_str(tag);
        }
    }
    if out.is_empty() {
        out.push_str("none");
    }
    out
}

struct Probe {
    role: accesskit::Role,
    label: Signal<String>,
    value: Option<Signal<String>>,
    live: Option<accesskit::Live>,
    focusable: bool,
    accept_set_value: bool,
    opens_window: bool,
    /// Open a window from `on_long_press` rather than from a click/tap. The
    /// fixture for "a clock jump recognizes a gesture whose handler reaches the
    /// multi-window API".
    opens_window_on_long_press: bool,
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
    /// Each received `Scroll`, as `<dx>,<dy>,<mods>` — so a test can prove the
    /// modifiers a caller asked for actually reached the widget, and not just
    /// the delta.
    scrolls: Signal<Vec<String>>,
    /// The modifiers on each `PointerDown`, same formatting as `scrolls`, for
    /// the same reason: a Ctrl-click is its own gesture and a probe that cannot
    /// prove the Ctrl arrived is asserting against a plain click.
    presses: Signal<Vec<String>>,
}

impl std::fmt::Debug for Probe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Probe").field("role", &self.role).finish()
    }
}

/// The window an `opens_window()` probe asks for, whichever route reached it.
fn probe_child_window() -> teksilo_core::window::WindowConfig {
    teksilo_core::window::WindowConfig::new()
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
            opens_window_on_long_press: false,
            tristate_mixed: false,
            has_context_menu: false,
            handles_show_context_menu: false,
            clicks: Signal::new(0),
            taps: Signal::new(0),
            typed: Signal::new(String::new()),
            received: Signal::new(Vec::new()),
            scrolls: Signal::new(Vec::new()),
            presses: Signal::new(Vec::new()),
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
    /// Open a window when a long press is *recognized* — which happens on a
    /// clock tick, not on an incoming event.
    fn opens_window_on_long_press(mut self) -> Self {
        self.opens_window_on_long_press = true;
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
        let opens_on_long_press = self.opens_window_on_long_press;
        let taps = self.taps.clone();
        let typed = self.typed.clone();
        let received = self.received.clone();
        let scrolls = self.scrolls.clone();
        let presses = self.presses.clone();

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
            })
            .on_pointer_event(move |event, _ctx| {
                if let WidgetEvent::PointerDown { modifiers, .. } = event {
                    let mut log = presses.get();
                    log.push(mods_tag(modifiers));
                    presses.set(log);
                }
                EventResponse::Ignored
            })
            .on_scroll(move |event, _ctx| {
                if let WidgetEvent::Scroll {
                    delta, modifiers, ..
                } = event
                {
                    let (dx, dy) = match delta {
                        teksilo_core::event::ScrollDelta::Pixels { x, y }
                        | teksilo_core::event::ScrollDelta::Lines { x, y } => (*x, *y),
                    };
                    let mods = mods_tag(modifiers);
                    let mut log = scrolls.get();
                    log.push(format!("{dx},{dy},{mods}"));
                    scrolls.set(log);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });
        if opens_on_long_press {
            handlers = handlers.on_long_press(move |_e: &TapEvent, ctx| {
                ctx.open_window(probe_child_window());
            });
        }
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

    fn accessibility(&self, builder: &mut teksilo_core::AccessNodeBuilder) {
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
        bounds: teksilo_canvas::Rect,
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
    teksilo_core::accessibility::widget_id_to_node_id(id).0
}

/// Validate a fresh `TreeUpdate` with the real AccessKit consumer — the
/// same conformance gate teksilo-core's own AT tests use.
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
    // A false assertion is a failure, not a successful report of one. The
    // detail survives into the message, because "role is 'Button', expected
    // 'Slider'" is the whole value of the failure.
    let AutomationReply::Err { code, message } = fail else {
        panic!("a false assertion must be an Err, got {fail:?}");
    };
    assert_eq!(code, codes::ASSERTION_FAILED);
    assert!(
        message.contains("Button") && message.contains("Slider"),
        "the message must carry actual and expected: {message}"
    );
}

/// The distinction the error code exists for. "The button is not focused" and
/// "there is no such button" are different bugs, and a caller that sees one
/// message for both chases the wrong one.
#[test]
fn a_missing_node_is_not_found_rather_than_a_failed_assertion() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "B"));
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::AssertNode {
            node: 12345,
            assertion: Assertion::RoleEquals {
                value: "Button".into(),
            },
        },
        &default_settle(),
    );
    let AutomationReply::Err { code, .. } = reply else {
        panic!("{reply:?}");
    };
    assert_eq!(
        code,
        codes::NOT_FOUND,
        "asserting a property of a node that does not exist is a bad node \
         reference, not a property mismatch"
    );
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
    // Asking whether something exists and being told it does not is an answer,
    // so this is a failed assertion rather than a bad node reference.
    let AutomationReply::Err { code, .. } = reply else {
        panic!("{reply:?}");
    };
    assert_eq!(code, codes::ASSERTION_FAILED);
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

/// **A misspelled argument is refused, not quietly reinterpreted.**
///
/// serde's default is to ignore a field it does not recognise and take the
/// `#[serde(default)]` for the one that was meant. On `InjectPointer` that
/// default is `Click`, so `{"x":.., "y":.., "kind":"move"}` -- `kind` where
/// `action` was meant -- asked to hover and clicked instead, on every control
/// it pointed at. It toggled real settings in a real config while a probe
/// reported nothing wrong, because from serde's side nothing was.
#[test]
fn an_unknown_argument_is_refused_rather_than_defaulted() {
    let good = serde_json::json!({
        "InjectPointer": { "x": 1.0, "y": 2.0, "action": "move" }
    });
    let parsed: AutomationOp = serde_json::from_value(good).expect("a well-formed op parses");
    assert!(matches!(
        parsed,
        AutomationOp::InjectPointer {
            action: PointerAction::Move,
            ..
        }
    ));

    // A field the op does not declare — the shape of the original defect,
    // where a probe meant to hover and clicked instead. This test used to use
    // `kind`, which became a real field when the touch and pen ops landed. Its
    // replacement must be undeclared AND not a near-miss of an English word:
    // the repo's `typos` gate reads test fixtures too, and a plausible
    // misspelling there either fails the gate or has to be excused repo-wide.
    let typo = serde_json::json!({
        "InjectPointer": { "x": 1.0, "y": 2.0, "nosuchfield": "secondary" }
    });
    let err = serde_json::from_value::<AutomationOp>(typo)
        .expect_err("a field nobody declared must not be silently dropped");
    assert!(
        err.to_string().contains("nosuchfield"),
        "and the error must name the offending field, got: {err}"
    );
}

/// A double-click is one op, because two `Click` ops cannot be one: the round
/// trip and the settle between them are longer than the recogniser's window.
#[test]
fn inject_pointer_double_click_is_seen_as_a_double_tap() {
    let probe = Probe::new(accesskit::Role::Button, "Tappable");
    let taps = probe.taps.clone();
    let (mut tree, id) = laid_out(probe);
    let bounds = tree.bounds(id);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InjectPointer {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
            action: PointerAction::DoubleClick,
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
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "double click ok: {reply:?}");
    assert_eq!(
        taps.get(),
        2,
        "both presses must reach the widget, back to back"
    );
}

/// Modifiers reach the synthesised press. Ctrl-click to extend a selection is
/// its own gesture, and the corkboard probe spent its life passing an
/// undeclared `modifiers` field that serde dropped on the floor -- asserting
/// against a plain click while believing it held Ctrl.
#[test]
fn inject_pointer_carries_its_modifiers() {
    let probe = Probe::new(accesskit::Role::Button, "Tappable");
    let presses = probe.presses.clone();
    let (mut tree, id) = laid_out(probe);
    let bounds = tree.bounds(id);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::InjectPointer {
            x: bounds.x + bounds.width * 0.5,
            y: bounds.y + bounds.height * 0.5,
            action: PointerAction::Click,
            button: PointerButtonDto::Primary,
            kind: PointerKindDto::Mouse,
            pointer_id: None,
            pressure: None,
            tilt: None,
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "ctrl-click ok: {reply:?}");
    assert_eq!(
        presses.get(),
        vec!["ctrl".to_string()],
        "the press must carry Ctrl"
    );
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
            kind: PointerKindDto::Mouse,
            pointer_id: None,
            pressure: None,
            tilt: None,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
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
        command: false,
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
        let AutomationReply::Err { code, message } = reply else {
            panic!("Mixed must not satisfy Toggled {{ value: {value} }}: {reply:?}");
        };
        assert_eq!(code, codes::ASSERTION_FAILED);
        assert!(
            message.contains("mixed"),
            "the message must name the tristate state: {message}"
        );
    }
}

/// A modifier-held wheel is a *different gesture* from a plain one, and
/// `WidgetEvent::Scroll` carries modifiers precisely so an app can tell them
/// apart (Ctrl-wheel-to-zoom is the motivating case named in its own doc).
/// The op hardcoded `Modifiers::NONE`, so it was the one input the bridge could
/// describe but not perform: a probe could confirm that a plain wheel scrolls
/// and never that Ctrl+wheel zooms.
#[test]
fn scroll_carries_the_modifiers_it_was_given() {
    let probe = Probe::new(accesskit::Role::GenericContainer, "S");
    let scrolls = probe.scrolls.clone();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let node = node_ref(id);

    let mk = |ctrl, shift, alt, meta| AutomationOp::Scroll {
        node,
        dx: 0.0,
        dy: -48.0,
        ctrl,
        shift,
        alt,
        meta,
        command: false,
    };
    execute(
        &mut tree,
        &mut ops,
        &mk(true, false, false, false),
        &default_settle(),
    );
    execute(
        &mut tree,
        &mut ops,
        &mk(false, true, false, false),
        &default_settle(),
    );
    execute(
        &mut tree,
        &mut ops,
        &mk(false, false, true, true),
        &default_settle(),
    );

    let log = scrolls.get();
    assert_eq!(
        log,
        vec![
            "0,-48,ctrl".to_string(),
            "0,-48,shift".to_string(),
            "0,-48,alt+meta".to_string(),
        ],
        "each modifier must reach the widget as asked, got {log:?}"
    );
}

/// `command` is the platform's *primary accelerator*, `ctrl` is literal Control,
/// and the two are only the same key off macOS.
///
/// This is the difference between a cross-platform agent script and one that
/// silently does nothing: a Teksilo shortcut declared `Ctrl+S` resolves to the
/// Command chord on macOS, so a probe sending literal Control there matches no
/// binding — and reports success, because the key really was injected. Asserting
/// against `Modifiers::COMMAND` rather than a hardcoded `ctrl` keeps this test
/// meaningful on all three platforms: it is the same constant the shortcut
/// registry resolves declarations through.
#[test]
fn command_modifier_is_the_platform_accelerator_and_ctrl_stays_literal() {
    let probe = Probe::new(accesskit::Role::GenericContainer, "S");
    let scrolls = probe.scrolls.clone();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let node = node_ref(id);

    let mk = |ctrl, command| AutomationOp::Scroll {
        node,
        dx: 0.0,
        dy: -1.0,
        ctrl,
        shift: false,
        alt: false,
        meta: false,
        command,
    };
    execute(&mut tree, &mut ops, &mk(false, true), &default_settle());
    execute(&mut tree, &mut ops, &mk(true, false), &default_settle());

    let log = scrolls.get();
    // What the accelerator spells on *this* host, taken from the same constant
    // the rest of the framework resolves `Ctrl`-declared shortcuts through.
    let accel = if teksilo_core::event::Modifiers::COMMAND
        .contains(teksilo_core::event::Modifiers::SUPER)
    {
        "meta"
    } else {
        "ctrl"
    };
    assert_eq!(
        log,
        vec![format!("0,-1,{accel}"), "0,-1,ctrl".to_string()],
        "command must resolve to the platform accelerator and ctrl stay literal, got {log:?}"
    );
}

/// Omitting `command` leaves every existing probe unchanged — it is
/// `#[serde(default)]`, so an op authored before it existed still deserializes.
#[test]
fn command_defaults_to_off_and_is_optional_on_the_wire() {
    // The externally-tagged wire form, with `command` simply absent.
    let json = r#"{"InjectKey":{"key":"s","ctrl":true}}"#;
    let op: AutomationOp = serde_json::from_str(json).expect("legacy op still parses");
    let AutomationOp::InjectKey { ctrl, command, .. } = op else {
        panic!("expected inject_key");
    };
    assert!(ctrl, "ctrl round-trips");
    assert!(!command, "command defaults to false when absent");
}

/// The default stays a bare wheel, so every existing probe keeps working
/// unchanged — the fields are `#[serde(default)]` for exactly this.
#[test]
fn scroll_without_modifiers_is_still_a_plain_wheel() {
    let probe = Probe::new(accesskit::Role::GenericContainer, "S");
    let scrolls = probe.scrolls.clone();
    let (mut tree, id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();
    let node = node_ref(id);

    execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Scroll {
            node,
            dx: 0.0,
            dy: 120.0,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
        &default_settle(),
    );

    assert_eq!(scrolls.get(), vec!["0,120,none".to_string()]);
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
            command: false,
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

// ---------------------------------------------------------------------------
// What the time-moving ops do, now that they all go through one door
// ---------------------------------------------------------------------------
//
// `AdvanceClock`, `Settle` and `WaitForCondition` are the three ops that move
// the clock, and each one changed behaviour when they were folded onto
// `WidgetTree::advance_time_with_ops`. Each change is named and asserted here,
// because none of them is visible from the ops' replies: a `Settle` that
// silently stopped ripening dwells, or a wait whose per-poll step doubled,
// still answers `Ok`.

/// A tooltip whose dwell is `delay`, hovered but not yet ripe.
///
/// The fixture for "something becomes true only after N ms of *simulated*
/// time". A dwell is the cheapest such thing the toolkit can reach: it needs
/// no animation registration, and it is one of the passes the folded
/// `advance_time` is claimed to run.
fn hovered_tooltip(delay: Duration) -> WidgetTree {
    let mut tree = WidgetTree::new();
    let anchor = tree.add(Probe::new(accesskit::Role::Button, "Anchor"));
    let tip = tree.add(Probe::new(accesskit::Role::Tooltip, "the tip"));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.attach_tooltip(anchor, tip, delay);
    tree.pointer_move(tree.bounds(anchor).center());
    assert!(
        tree.active_overlays().is_empty(),
        "the dwell has not run yet"
    );
    tree
}

/// `wait_for_condition` spends its budget as simulated time, one 16 ms frame
/// per poll — so a budget of N ms buys N ms of simulated time, not two N.
///
/// The pair of arms is the assertion. A condition that ripens strictly between
/// the two budgets is red if the per-poll advance moves at all: double it and
/// the short arm stops timing out, halve it and the long arm stops resolving.
/// The existing `wait_for_condition_succeeds_and_times_out` cannot see any of
/// that — its conditions are true immediately or never.
#[test]
fn a_wait_spends_its_budget_as_simulated_time_one_frame_per_poll() {
    // The dwell sits between the two budgets below, so which arm it lands in
    // is decided by how much simulated time a poll buys.
    let dwell = Duration::from_millis(128);
    let condition = WaitCondition::NodeExists {
        role: None,
        label: Some("the tip".into()),
    };

    let mut tree = hovered_tooltip(dwell);
    let mut ops = RecordingWindowOps::new();
    let resolved = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::WaitForCondition {
            condition: condition.clone(),
        },
        &SettleSpec {
            settle_timeout_ms: 160,
            ..Default::default()
        },
    );
    assert!(
        resolved.is_ok(),
        "160 ms of budget must buy 160 ms of simulated time: {resolved:?}"
    );

    let mut tree = hovered_tooltip(dwell);
    let mut ops = RecordingWindowOps::new();
    let timed_out = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::WaitForCondition { condition },
        &SettleSpec {
            settle_timeout_ms: 80,
            ..Default::default()
        },
    );
    assert!(
        matches!(&timed_out, AutomationReply::Err { code, .. } if code == codes::WAIT_TIMEOUT),
        "half the budget must buy half the simulated time: {timed_out:?}"
    );
}

/// A settle ripens what the clock owns — here a tooltip dwell — and moves the
/// simulated clock by exactly `clock_millis` plus one 16 ms step per animation
/// frame it ran.
///
/// Both halves are behaviour the fold introduced. The settle used to move the
/// clock through a door that reached the animation scheduler and nothing else,
/// so a dwell, a delayed overlay, a pointer-leave grace and a long press could
/// all sit unresolved through any number of settles; and the jump and the
/// frames used to be two calls, which advanced the clock twice.
#[test]
fn a_settle_ripens_a_dwell_and_advances_by_exactly_what_it_says() {
    // (a) the clock jump: it ripens the dwell, and it is the *whole* advance
    //     when nothing is animating.
    let jump = Duration::from_millis(200);
    let mut tree = hovered_tooltip(Duration::from_millis(128));
    let mut ops = RecordingWindowOps::new();
    let before = tree.simulated_now();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &SettleSpec {
            clock_millis: jump.as_millis() as u64,
            ..Default::default()
        },
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(
        tree.active_overlays().len(),
        1,
        "the settle's clock jump ran the dwell to term"
    );
    assert_eq!(
        tree.simulated_now().duration_since(before),
        jump,
        "nothing was animating, so the jump is the whole advance"
    );

    // (b) the animation frames: one 16 ms step each, on top of the jump.
    let frame = Duration::from_millis(16);
    let mut tree = WidgetTree::new();
    let owner = tree.add(Probe::new(accesskit::Role::Button, "Animated"));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let animated = Signal::<f32>::new_animated(0.0);
    tree.register_animated_signal(&animated, owner);
    let easing = tree.theme().motion.easing_standard;
    // Deliberately off a frame boundary, so how many frames it needs does not
    // turn on whether the scheduler retires an animation at `elapsed == d` or
    // at `elapsed > d`: 72 ms is done at the 80 ms tick either way. The
    // settle's own jump is the first 16 ms step — that is where a pending
    // `animate_to` is promoted and started — leaving four for the loop.
    let anim_frames = 4;
    animated.animate_to(1.0, frame * 4 + frame / 2, easing);

    let before = tree.simulated_now();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::Settle,
        &SettleSpec {
            clock_millis: frame.as_millis() as u64,
            ..Default::default()
        },
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert!(
        !tree.has_active_animations(),
        "the settle ran the animation out"
    );
    assert_eq!(
        tree.simulated_now().duration_since(before),
        frame + frame * anim_frames,
        "clock_millis plus one 16 ms step per animation frame"
    );
}

/// A clock jump runs over the caller's window sink.
///
/// The hazard the fold created: `AdvanceClock` used to move a scheduler and
/// nothing else, and now it recognizes gestures — so a long press that ripens
/// inside the jump runs its handler, and that handler may call
/// `ctx.open_window`. Dispatched standalone that is a panic on
/// `NoopWindowOps`, which is precisely why `advance_time_with_ops` exists.
/// Nothing else in the workspace exercises `AdvanceClock` at all.
#[test]
fn a_clock_jump_recognizes_a_long_press_over_the_callers_window_sink() {
    let probe = Probe::new(accesskit::Role::Button, "Hold me").opens_window_on_long_press();
    let (mut tree, _id) = laid_out(probe);
    let mut ops = RecordingWindowOps::new();

    // A finger/mouse goes down and stays down: nothing has been recognized,
    // and no further event will arrive to recognize it.
    let hold = tree.theme().input.gestures.mouse.long_press;
    tree.pointer_down_button(
        tree.bounds(_id).center(),
        teksilo_core::event::PointerButton::Primary,
    );
    assert!(ops.opened.is_empty(), "a press alone opens nothing");

    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::AdvanceClock {
            millis: hold.as_millis() as u64,
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(
        ops.opened.len(),
        1,
        "the jump recognized the hold and its handler reached the window sink"
    );
    assert_eq!(ops.opened[0].string_id.as_deref(), Some("probe-child"));

    // …and the op gave the input timeline back. Advancing the clock freezes it
    // — that is what makes the jump deterministic — and on a live attached
    // window a timeline left frozen never measures another gesture.
    assert_wall_clock_resumed(&mut tree);
}

/// The reading of a tree's input timeline moves with the wall clock again.
///
/// A frozen timeline answers the same instant however long you wait, so a real
/// window's every later keystroke, tap and hold is stamped one moment.
fn assert_wall_clock_resumed(tree: &mut WidgetTree) {
    let a = tree.input_now();
    std::thread::sleep(Duration::from_millis(5));
    let b = tree.input_now();
    assert!(
        b > a,
        "the input timeline is still frozen at {a:?} after 5 ms of real time"
    );
}

/// `run_settle` is `pub` and is called directly by the headless tree-thread and
/// the live bridge, not only through [`execute`] — so it hands the input
/// timeline back itself.
#[test]
fn a_settle_called_directly_hands_the_input_timeline_back() {
    let (mut tree, _id) = laid_out(Probe::new(accesskit::Role::Button, "Direct"));
    let mut ops = RecordingWindowOps::new();
    let timed_out = crate::run_settle(
        &mut tree,
        &mut ops,
        &SettleSpec {
            clock_millis: 100,
            ..Default::default()
        },
    );
    assert!(timed_out.is_none(), "{timed_out:?}");
    assert_wall_clock_resumed(&mut tree);
}

/// …and the *animation* clock too: an animation still in flight when the op
/// returns goes on running on the window's own frames.
///
/// The two ways the axis can be wrong are both fatal to a live attached app,
/// and each is what the other's fix looks like from the wrong side. Left on
/// the simulated clock, an attached app's animations only ever move when an
/// op advances them — every transition on screen freezes between operations.
/// Handed back without rebasing what the scheduler stored, the first real
/// frame measures each animation from a start on the abandoned axis and
/// completes it outright.
#[test]
fn an_animation_goes_on_running_after_an_op_returns() {
    let mut tree = WidgetTree::new();
    let owner = tree.add(Probe::new(accesskit::Role::Button, "Animated"));
    tree.layout(SizeProposal::exact(400.0, 300.0));

    // The tree spends real time alive before anything simulates it, and more
    // of it than the op will advance. That is the *live* condition, and it is
    // what separates the two failures: with the wall clock ahead of the
    // simulated one, an un-rebased hand-back hands the animation an elapsed
    // time longer than its whole duration.
    std::thread::sleep(Duration::from_millis(300));

    let animated = Signal::<f32>::new_animated(0.0);
    tree.register_animated_signal(&animated, owner);
    // The theme's own easing, so this asserts nothing about the shape of the
    // curve — only that the animation is somewhere in the middle of it, keeps
    // moving, and is not retired.
    let easing = tree.theme().motion.easing_standard;
    animated.animate_to(1.0, Duration::from_millis(400), easing);

    // `AdvanceClock` runs no settle, so the 100 ms it reports is the whole
    // advance: a quarter of the way in, with the animation still live.
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::AdvanceClock { millis: 100 },
        &default_settle(),
    );
    assert!(reply.is_ok(), "{reply:?}");
    let at_reply = animated.get();
    assert!(
        at_reply > 0.0 && at_reply < 0.9,
        "100 ms of 400 leaves it part-way: {at_reply}"
    );
    assert!(
        tree.has_active_animations(),
        "100 ms of 400 does not retire it"
    );

    // The op has returned and the window is painting its own frames again.
    std::thread::sleep(Duration::from_millis(100));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let after = animated.get();
    assert!(
        after > at_reply + 0.02,
        "frozen: 100 ms of real frames moved it from {at_reply} to {after}"
    );
    assert!(
        tree.has_active_animations(),
        "snapped to its end: 100 ms of a 400 ms animation retired it ({at_reply} -> {after})"
    );
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
            kind: PointerKindDto::Mouse,
            pointer_id: None,
            pressure: None,
            tilt: None,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
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
            command: false,
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
fn tool_catalog_has_34_entries() {
    assert_eq!(crate::mcp_schema::TOOL_COUNT, 34);
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

// ===========================================================================
// P36 — touch, pen, pinch, fling and the pointer queries
// ===========================================================================

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A leaf that fills whatever it is proposed and emits an AT node, so a test
/// can both press it by coordinate and name it by `find_node`.
///
/// Everything it observes goes into one ordered log, because the ops under test
/// are *sequences*: "both fingers landed before either moved" is a statement
/// about order, and a set of counters cannot make it.
#[derive(Debug)]
struct InputProbe {
    label: &'static str,
    log: Signal<Vec<String>>,
    /// Open a window from `on_long_press` — the witness that a touch op keeps
    /// the caller's real `WindowOps` (see `an_injected_touch_reaches_the_
    /// callers_window_ops`).
    opens_window_on_long_press: bool,
}

impl InputProbe {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            log: Signal::new(Vec::new()),
            opens_window_on_long_press: false,
        }
    }
    fn opening_a_window_on_long_press(mut self) -> Self {
        self.opens_window_on_long_press = true;
        self
    }
    fn log(&self) -> Signal<Vec<String>> {
        self.log.clone()
    }
}

/// Push one line onto a probe's log. A free fn so every handler records through
/// one door and the log's shape is decided in one place.
fn record(log: &Signal<Vec<String>>, line: String) {
    let mut v = log.get();
    v.push(line);
    log.set(v);
}

impl Widget for InputProbe {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let pointer_log = self.log.clone();
        let scroll_log = self.log.clone();
        let cancel_log = self.log.clone();
        let press_log = self.log.clone();
        let opens = self.opens_window_on_long_press;
        let handlers = HandlerSet::new()
            .on_pointer_event(move |event, ctx| {
                let phase = match event {
                    WidgetEvent::PointerDown { .. } => "down",
                    WidgetEvent::PointerMove { .. } => "move",
                    WidgetEvent::PointerUp { .. } => "up",
                    _ => return EventResponse::Ignored,
                };
                let p = ctx.pointer();
                let kind = match p.kind {
                    teksilo_tokens::PointerKind::Touch => "touch",
                    teksilo_tokens::PointerKind::Pen(_) => "pen",
                    _ => "mouse",
                };
                // `phase:kind:id:pressure:tilt:stamp`, in that order and with
                // the timestamp last, so a reader that only wants the first
                // three fields can split on ':' and index.
                record(
                    &pointer_log,
                    format!(
                        "{phase}:{kind}:id{}:p{}:t{}:n{}",
                        p.id.get(),
                        p.axes
                            .pressure
                            .map(|v| format!("{v:.2}"))
                            .unwrap_or_else(|| "-".into()),
                        p.axes
                            .tilt
                            .map(|(x, y)| format!("{x:.1}/{y:.1}"))
                            .unwrap_or_else(|| "-".into()),
                        p.time.as_duration().as_nanos(),
                    ),
                );
                EventResponse::Ignored
            })
            .on_scroll(move |_event, ctx| {
                record(&scroll_log, format!("scroll:{:?}", ctx.scroll_source()));
                EventResponse::Handled
            })
            .on_pointer_cancel(move |_pointer, reason, _ctx| {
                record(&cancel_log, format!("cancel:{reason:?}"));
            })
            .on_long_press(move |_e: &TapEvent, ctx| {
                record(&press_log, "long_press".to_string());
                if opens {
                    ctx.open_window(probe_child_window());
                }
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, builder: &mut teksilo_core::AccessNodeBuilder) {
        builder.set_role(accesskit::Role::Button);
        builder.set_name(self.label.to_string());
        builder.add_action(accesskit::Action::Click);
    }
}

/// A leaf that fills its proposal and installs nothing at all — what a press
/// lands on in the arbitration fixture. Restated here (rather than reached for)
/// for the same reason `teksilo-core`'s own `tests/common/mod.rs` restates it:
/// `teksilo_core::test_widgets` is `pub(crate)`, and a seam reachable only by
/// widening visibility is the wrong seam.
#[derive(Debug, Default)]
struct Leaf;

impl Widget for Leaf {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// A container that places every child over its own whole bounds, so a press
/// hits the leaf *and* is on every ancestor's path with no geometry standing
/// between them to explain a result away.
#[derive(Debug, Default)]
struct Stack {
    children: Vec<WidgetId>,
}

impl Stack {
    fn new() -> Self {
        Self::default()
    }

    fn child(mut self, c: impl teksilo_core::IntoTeksiChild) -> Self {
        match teksilo_core::IntoTeksiChild::into_pending(c) {
            teksilo_core::PendingChild::Id(id) => self.children.push(id),
            teksilo_core::PendingChild::Deferred(_) => unreachable!("Stack takes ids"),
        }
        self
    }
}

impl Widget for Stack {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
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
        self.children.clone()
    }
}

fn laid_out_probe(probe: InputProbe) -> (WidgetTree, WidgetId, Signal<Vec<String>>) {
    let log = probe.log();
    let mut tree = WidgetTree::new();
    let id = tree.add(probe);
    tree.layout(SizeProposal::exact(400.0, 300.0));
    (tree, id, log)
}

fn run(tree: &mut WidgetTree, op: &AutomationOp) -> AutomationReply {
    let mut ops = RecordingWindowOps::new();
    execute(tree, &mut ops, op, &default_settle())
}

// ---------------------------------------------------------------------------
// The two no-regression pins A21 names for the pre-existing ops
// ---------------------------------------------------------------------------

#[test]
fn scroll_injects_a_programmatic_sample() {
    // A21: "`Scroll` injects a `ScrollSource::Programmatic` sample". The half a
    // handler can see is the source itself — this is the test that reddens if
    // the op goes back to lowering a bare `WidgetEvent::scroll`, whose snapshot
    // hardcodes `Wheel`.
    let (mut tree, id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::Scroll {
            node: node_ref(id),
            dx: 0.0,
            dy: -40.0,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
    );
    assert!(reply.is_ok(), "scroll ok: {reply:?}");
    assert!(
        log.get().iter().any(|l| l == "scroll:Programmatic"),
        "an automation scroll must reach the handler as Programmatic: {:?}",
        log.get()
    );
}

#[test]
fn a_programmatic_scroll_bubbles_and_no_pan_claimant_competes_for_it() {
    // The other half of A21's clause: "…that no pan claimant competes for".
    //
    // What it can and cannot catch. `ScrollDelivery::for_source` sends only
    // `TouchPan` down the claimant chain, so `Wheel` and `Programmatic` route
    // identically — this test does NOT redden if the source regresses to
    // `Wheel` (`scroll_injects_a_programmatic_sample` above is the test that
    // does). It reddens if the op ever synthesises a *pan*: the probe is not a
    // claimant, so under `ClaimantChain` the walk would skip it entirely and
    // reach only the scroller above it.
    let mut tree = WidgetTree::new();
    let probe = InputProbe::new("probe");
    let log = probe.log();
    let inner = tree.add(probe);
    let scroller = tree.add(
        Stack::new()
            .child(inner)
            .pan_claim(teksilo_core::PanClaim::vertical()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let _ = scroller;

    let reply = run(
        &mut tree,
        &AutomationOp::Scroll {
            node: node_ref(inner),
            dx: 0.0,
            dy: -40.0,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
    );
    assert!(reply.is_ok(), "scroll ok: {reply:?}");
    assert!(
        log.get().iter().any(|l| l.starts_with("scroll:")),
        "a scroll under a pan claimant must still bubble to the widget it was \
         aimed at: {:?}",
        log.get()
    );
}

#[test]
fn drag_node_injects_a_mouse_pointer() {
    // A21: "`DragNode` injects a mouse pointer". A drag is the one op with no
    // `kind` argument, so nothing about the call says which device it is; the
    // guarantee is that it stays the indirect precise one, which is what
    // decides its slop, its hover and whether an ancestor's pan claim is
    // enrolled at all.
    let (mut tree, id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::DragNode {
            node: node_ref(id),
            to_node: None,
            to_x: Some(300.0),
            to_y: Some(250.0),
        },
    );
    assert!(reply.is_ok(), "drag ok: {reply:?}");
    let seen = log.get();
    let kinds: Vec<&str> = seen
        .iter()
        .filter(|l| l.contains(':'))
        .filter_map(|l| l.split(':').nth(1))
        .collect();
    assert!(
        !kinds.is_empty(),
        "the drag reached no pointer handler: {seen:?}"
    );
    assert!(
        kinds.iter().all(|k| *k == "mouse"),
        "every sample a drag_node injects must be a mouse: {seen:?}"
    );
    // And the shape stays three events — down, one intermediate move, up. A
    // latch threshold is evaluated per sample, so the sample count is itself
    // observable behaviour.
    let phases: Vec<&str> = seen
        .iter()
        .filter_map(|l| l.split(':').next())
        .filter(|p| matches!(*p, "down" | "move" | "up"))
        .collect();
    assert_eq!(phases, vec!["down", "move", "up"], "drag shape: {seen:?}");
}

// ---------------------------------------------------------------------------
// The documented arbitration answer, reproduced through the DTO surface
// ---------------------------------------------------------------------------

/// One row of the generated arbitration-matrix table in
/// `docs/events-and-gestures.md`, parsed back out of the documentation.
#[derive(Debug)]
struct DocumentedRow {
    /// The `TouchAction` the press must freeze, by name.
    frozen: String,
    /// Every competitor at the press: fixture name, role, state.
    members: Vec<(String, String, String)>,
    /// The movement, as `(dx, dy, winner-or-None)` after each step.
    steps: Vec<(f32, f32, Option<String>)>,
}

/// Read one row of the matrix out of the documentation.
///
/// **Why the documentation and not the numbers.** The arbitration answer is
/// `teksilo-core`'s to decide and `crates/teksilo-core/tests/arbitration_matrix.rs`
/// is where it is pinned; that file also renders the table below into
/// `docs/events-and-gestures.md` and fails when the two drift. What is *this*
/// crate's to prove is that the automation surface reports the decision
/// faithfully — that a scripted gesture can see the frozen action, the member
/// list and the winner at all, and sees the same ones the framework decided.
/// Restating the numbers here would make a second copy of the proof that could
/// go stale silently; reading them makes the documentation the single source
/// and this test a check on the plumbing, which is the part P36 owns.
fn documented_row(scenario: &str, pointer: &str) -> DocumentedRow {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/events-and-gestures.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let want = format!("| {scenario} | {pointer} | ");
    let line = doc
        .lines()
        .find(|l| l.starts_with(&want))
        .unwrap_or_else(|| {
            panic!(
                "no row '{scenario} · {pointer}' in the generated arbitration matrix in {}",
                path.display()
            )
        });
    let cells: Vec<&str> = line.split('|').map(str::trim).collect();
    // ["", scenario, pointer, frozen, members, movement, losers, note, ""]
    assert!(
        cells.len() > 5,
        "the matrix row has fewer columns than the table declares: {line}"
    );
    let unquote = |s: &str| s.trim().trim_matches('`').to_string();

    let frozen = unquote(cells[3]);

    let members = if cells[4] == "—" {
        Vec::new()
    } else {
        cells[4]
            .split(", ")
            .map(|m| {
                let (name, rest) = m.split_once(' ').unwrap_or_else(|| {
                    panic!("cannot read member {m:?} out of the documented row")
                });
                let (role, state) = rest.split_once('/').unwrap_or_else(|| {
                    panic!("cannot read role/state out of the documented member {m:?}")
                });
                (
                    unquote(name),
                    role.trim().to_string(),
                    state.trim().to_string(),
                )
            })
            .collect()
    };

    let steps = cells[5]
        .split("; ")
        .map(|step| {
            let (offset, winner) = step
                .split_once('→')
                .unwrap_or_else(|| panic!("cannot read a movement step out of {step:?}"));
            let offset = offset.trim().trim_start_matches('(').trim_end_matches(')');
            let (dx, dy) = offset
                .split_once(',')
                .unwrap_or_else(|| panic!("cannot read an offset out of {offset:?}"));
            let num = |s: &str| {
                s.trim()
                    .trim_start_matches('+')
                    .parse::<f32>()
                    .unwrap_or_else(|e| panic!("cannot read {s:?} as a number: {e}"))
            };
            let winner = winner.trim();
            (num(dx), num(dy), (winner != "—").then(|| unquote(winner)))
        })
        .collect();

    DocumentedRow {
        frozen,
        members,
        steps,
    }
}

#[test]
fn a_scripted_touch_sequence_reproduces_the_documented_arbitration_row() {
    let row = documented_row("list row", "touch");

    // A parser that silently read nothing would make every assertion below
    // vacuous, so the row's own shape is checked first: it must bracket a
    // threshold (a step that decides nothing and a later one that decides), and
    // it must have named competitors for the reported member list to be
    // compared against.
    assert!(
        row.steps.len() >= 2,
        "the documented row must bracket a threshold: {row:?}"
    );
    assert!(
        row.steps.iter().any(|(_, _, w)| w.is_none())
            && row.steps.iter().any(|(_, _, w)| w.is_some()),
        "the documented row must have a step before and a step after the latch: {row:?}"
    );
    assert!(
        row.members.len() >= 2,
        "the documented row must name competitors: {row:?}"
    );

    // The fixture the row is written against: `Scenario::ListRow` in
    // `crates/teksilo-core/tests/arbitration_matrix.rs` — a reorderable row's
    // own drag inside a vertical scroller's pan claim, restated here the way
    // that file restates a widget's shape, and named the same way so the
    // documented names resolve.
    let mut tree = WidgetTree::new();
    // Compact is the density the touch programme's no-regression criterion is
    // stated at; the row is generated at it too.
    tree.set_density(teksilo_tokens::TargetDensity::Compact);
    let leaf = tree.add(Leaf);
    let row_id = tree.add(
        Stack::new()
            .child(leaf)
            .on_tap(|_e: &TapEvent, _c| {})
            .on_drag(|_p, _c| {}),
    );
    let scroller = tree.add(
        Stack::new()
            .child(row_id)
            .pan_claim(teksilo_core::PanClaim::vertical()),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let names: Vec<(&str, WidgetId)> =
        vec![("leaf", leaf), ("row", row_id), ("scroller", scroller)];
    let name_of = |node: NodeRef| -> String {
        names
            .iter()
            .find(|(_, id)| node_ref(*id) == node)
            .map(|(n, _)| (*n).to_string())
            .unwrap_or_else(|| format!("<unknown node {node}>"))
    };

    let press = tree.bounds(scroller).center();
    let mut steps = vec![TouchStep {
        contact: 0,
        phase: TouchPhaseDto::Down,
        x: press.x,
        y: press.y,
        advance_ms: 0,
    }];
    for (dx, dy, _) in &row.steps {
        steps.push(TouchStep {
            contact: 0,
            phase: TouchPhaseDto::Move,
            x: press.x + dx,
            y: press.y + dy,
            advance_ms: 0,
        });
    }

    let reply = run(&mut tree, &AutomationOp::InjectTouchSequence { steps });
    let AutomationReply::Ok { data } = reply else {
        panic!("the touch sequence failed: {reply:?}");
    };
    let report: TouchSequenceReport =
        serde_json::from_value(data).expect("the reply is a TouchSequenceReport");

    // Step 0 is the press: it carries the frozen action and the member list.
    let at_press = report.steps[0]
        .pointer
        .as_ref()
        .expect("the contact is live right after its own down");
    assert_eq!(
        at_press.touch_action, row.frozen,
        "the frozen touch action the wire reports must be the documented one"
    );
    let reported: Vec<(String, String, String)> = at_press
        .sequence_members
        .iter()
        .map(|m| {
            (
                name_of(m.node),
                m.role.to_ascii_lowercase(),
                m.state.to_ascii_lowercase(),
            )
        })
        .collect();
    let documented: Vec<(String, String, String)> = row
        .members
        .iter()
        .map(|(n, r, s)| (n.clone(), r.to_ascii_lowercase(), s.to_ascii_lowercase()))
        .collect();
    assert_eq!(
        reported, documented,
        "the member list the wire reports must be the documented one, in order"
    );

    // Then one observation per movement step, in the order the row names them.
    for (i, (dx, dy, winner)) in row.steps.iter().enumerate() {
        let step = &report.steps[i + 1];
        let seen = step
            .pointer
            .as_ref()
            .expect("the contact is still down: the sequence never lifts it")
            .sequence_winner
            .map(name_of);
        assert_eq!(
            seen.as_deref(),
            winner.as_deref(),
            "after moving ({dx:+}, {dy:+}) the documented winner is {winner:?}"
        );
    }

    // The sequence deliberately stops short of the lift, so the contact is
    // still there to be asked about — that is what makes an arbitration
    // observable at all.
    assert_eq!(report.live.len(), 1, "the finger is still down: {report:?}");
}

// ---------------------------------------------------------------------------
// The touch / pen ops
// ---------------------------------------------------------------------------

/// Put `n` fingers down at distinct points and leave them there.
fn fingers_down(tree: &mut WidgetTree, n: u32) -> TouchSequenceReport {
    let steps: Vec<TouchStep> = (0..n)
        .map(|i| TouchStep {
            contact: i,
            phase: TouchPhaseDto::Down,
            x: 60.0 + i as f32 * 40.0,
            y: 80.0,
            advance_ms: 0,
        })
        .collect();
    let reply = run(tree, &AutomationOp::InjectTouchSequence { steps });
    let AutomationReply::Ok { data } = reply else {
        panic!("touch sequence failed: {reply:?}");
    };
    serde_json::from_value(data).expect("a TouchSequenceReport")
}

#[test]
fn query_pointers_reports_two_live_contacts() {
    let (mut tree, _id, _log) = laid_out_probe(InputProbe::new("probe"));
    let report = fingers_down(&mut tree, 2);
    assert_eq!(report.steps.len(), 2);
    assert_eq!(
        report.live.len(),
        2,
        "two fingers were put down and neither was lifted: {report:?}"
    );

    let reply = run(&mut tree, &AutomationOp::QueryPointers);
    let AutomationReply::Ok { data } = reply else {
        panic!("query_pointers failed: {reply:?}");
    };
    let live: Vec<PointerReport> = serde_json::from_value(data).expect("a pointer list");
    assert_eq!(live.len(), 2, "query_pointers must see both: {live:?}");
    assert!(
        live.iter()
            .all(|p| p.kind == PointerKindDto::Touch && p.down),
        "both are fingers and both are down: {live:?}"
    );
    // Two contacts, two identities — a sequence that reused one id would
    // describe one finger teleporting rather than two fingers on the glass.
    assert_ne!(
        live[0].pointer_id, live[1].pointer_id,
        "each contact must have its own identity: {live:?}"
    );
    // And the identities the sequence reported are the ones the table holds,
    // which is what makes the reply usable as the argument to a later op.
    let mut reported: Vec<u64> = report.steps.iter().map(|s| s.pointer_id).collect();
    let mut queried: Vec<u64> = live.iter().map(|p| p.pointer_id).collect();
    reported.sort_unstable();
    queried.sort_unstable();
    assert_eq!(reported, queried);
}

#[test]
fn cancel_pointer_revokes_a_live_contact_rather_than_lifting_it() {
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let report = fingers_down(&mut tree, 1);
    let id = report.live[0].pointer_id;

    let reply = run(&mut tree, &AutomationOp::CancelPointer { pointer_id: id });
    assert!(reply.is_ok(), "cancel_pointer ok: {reply:?}");

    let seen = log.get();
    assert!(
        seen.iter().any(|l| l == "cancel:Platform"),
        "the widget must be told the system revoked the contact: {seen:?}"
    );
    // Not an up: no release reaches the widget, and the contact is gone.
    assert!(
        !seen.iter().any(|l| l.starts_with("up:")),
        "a cancel is not a lift: {seen:?}"
    );
    let AutomationReply::Ok { data } = run(&mut tree, &AutomationOp::QueryPointers) else {
        panic!("query_pointers failed");
    };
    let live: Vec<PointerReport> = serde_json::from_value(data).expect("a pointer list");
    assert!(live.is_empty(), "the revoked contact is gone: {live:?}");
}

#[test]
fn cancelling_a_pointer_that_is_not_live_is_not_found() {
    let (mut tree, _id, _log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::CancelPointer {
            pointer_id: 999_999,
        },
    );
    match reply {
        AutomationReply::Err { code, .. } => assert_eq!(code, codes::NOT_FOUND),
        other => panic!("expected NOT_FOUND, got {other:?}"),
    }
}

#[test]
fn a_long_press_holds_for_exactly_the_profiles_threshold() {
    // The op's promise is "hold long enough for this device", not "hold 500 ms":
    // the number comes off the active input profile, so a script never has to
    // know it. The recognizer fires at `>= long_press`, so a hold of exactly
    // the threshold is what makes the threshold itself assertable — a helper
    // that added a safety margin would pass for a profile with any smaller
    // value.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let hold = tree
        .theme()
        .input
        .profile(teksilo_tokens::PointerKind::Touch)
        .long_press;
    let before = tree.simulated_now();
    let reply = run(
        &mut tree,
        &AutomationOp::LongPress {
            x: 200.0,
            y: 150.0,
            kind: PointerKindDto::Touch,
        },
    );
    assert!(reply.is_ok(), "long_press ok: {reply:?}");
    assert!(
        log.get().iter().any(|l| l == "long_press"),
        "the hold must reach the recognizer: {:?}",
        log.get()
    );
    let spent = tree.simulated_now().duration_since(before);
    assert!(
        spent >= hold,
        "the op must spend the profile's own hold ({hold:?}), spent {spent:?}"
    );
}

#[test]
fn an_injected_touch_reaches_the_callers_window_ops() {
    // The counterpart of `an_injected_click_reaches_the_callers_window_ops` for
    // the direct-pointer ops, and the reason this module builds its own samples
    // instead of calling `WidgetTree`'s A21 helpers: every one of those ends in
    // `dispatch_pointer`, which substitutes a `NoopWindowOps` whose
    // `open_window` panics. A long press recognized inside this op runs a
    // handler, and that handler may open a window — against a live app, the
    // panic took the whole application down.
    let probe = InputProbe::new("New Window").opening_a_window_on_long_press();
    let (mut tree, _id, _log) = laid_out_probe(probe);
    let mut ops = RecordingWindowOps::new();
    let reply = execute(
        &mut tree,
        &mut ops,
        &AutomationOp::LongPress {
            x: 200.0,
            y: 150.0,
            kind: PointerKindDto::Touch,
        },
        &default_settle(),
    );
    assert!(reply.is_ok(), "long_press ok: {reply:?}");
    assert_eq!(
        ops.opened.len(),
        1,
        "the handler's window request must reach the caller's sink, not a \
         panicking no-op one"
    );
}

#[test]
fn a_pinch_lands_both_contacts_before_either_moves() {
    // The recognizer's reference span is the distance between the two
    // landings, so a pinch whose second finger arrives after the first has
    // moved describes a different gesture entirely. That is a statement about
    // order, which is why the probe keeps one ordered log.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::Pinch {
            ax0: 180.0,
            ay0: 150.0,
            bx0: 220.0,
            by0: 150.0,
            ax1: 120.0,
            ay1: 150.0,
            bx1: 280.0,
            by1: 150.0,
            steps: 4,
        },
    );
    assert!(reply.is_ok(), "pinch ok: {reply:?}");
    let seen = log.get();
    let phases: Vec<&str> = seen
        .iter()
        .filter_map(|l| l.split(':').next())
        .filter(|p| matches!(*p, "down" | "move" | "up"))
        .collect();
    assert_eq!(
        &phases[..2],
        &["down", "down"],
        "both fingers must land before either moves: {seen:?}"
    );
    assert_eq!(
        phases.iter().filter(|p| **p == "move").count(),
        8,
        "four steps × two fingers: {seen:?}"
    );
    assert_eq!(
        &phases[phases.len() - 2..],
        &["up", "up"],
        "and both must lift: {seen:?}"
    );
    // Two identities, not one finger sampled twice.
    let ids: std::collections::BTreeSet<&str> =
        seen.iter().filter_map(|l| l.split(':').nth(2)).collect();
    assert_eq!(ids.len(), 2, "a pinch is two contacts: {seen:?}");
    assert!(
        run(&mut tree, &AutomationOp::QueryPointers).is_ok(),
        "and the tree is left with no live contact"
    );
}

#[test]
fn a_fling_spends_its_duration_and_sends_enough_samples_to_have_a_velocity() {
    // The two things that separate a fling from a drag along the same path.
    // Both are load-bearing: a flick described by fewer than the velocity
    // tracker's minimum sample count yields no velocity at all, and one whose
    // samples are stamped at the same instant describes infinite speed —
    // either way the op reports success and nothing coasts.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let before = tree.simulated_now();
    let reply = run(
        &mut tree,
        &AutomationOp::Fling {
            from_x: 200.0,
            from_y: 250.0,
            to_x: 200.0,
            to_y: 50.0,
            over_ms: 100,
        },
    );
    assert!(reply.is_ok(), "fling ok: {reply:?}");
    let moves = log.get().iter().filter(|l| l.starts_with("move:")).count();
    assert!(
        moves >= teksilo_core::kinetic::MIN_SAMPLE_SIZE,
        "a fling must carry at least the tracker's minimum sample count \
         ({}), sent {moves}",
        teksilo_core::kinetic::MIN_SAMPLE_SIZE
    );
    let spent = tree.simulated_now().duration_since(before);
    assert!(
        spent >= Duration::from_millis(90) && spent <= Duration::from_millis(200),
        "the flick must be spread over about its own duration, spent {spent:?}"
    );
}

#[test]
fn inject_pointer_touch_enters_as_a_finger_and_pen_carries_its_axes() {
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::InjectPointer {
            x: 200.0,
            y: 150.0,
            action: PointerAction::Click,
            button: PointerButtonDto::Primary,
            kind: PointerKindDto::Touch,
            pointer_id: None,
            pressure: None,
            tilt: None,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
    );
    assert!(reply.is_ok(), "touch click ok: {reply:?}");
    assert!(
        log.get().iter().any(|l| l.starts_with("down:touch:")),
        "the sample must reach the widget as a finger: {:?}",
        log.get()
    );

    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::InjectPointer {
            x: 200.0,
            y: 150.0,
            action: PointerAction::Down,
            button: PointerButtonDto::Primary,
            kind: PointerKindDto::Pen,
            pointer_id: None,
            pressure: Some(0.75),
            tilt: Some([12.0, -4.0]),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
    );
    assert!(reply.is_ok(), "pen down ok: {reply:?}");
    let seen = log.get();
    assert!(
        seen.iter()
            .any(|l| l.starts_with("down:pen:") && l.contains(":p0.75:t12.0/-4.0:")),
        "the digitizer axes must reach the widget: {seen:?}"
    );
}

#[test]
fn a_mouse_refuses_the_three_fields_it_cannot_carry() {
    // Refusing rather than ignoring: a silently-dropped `pressure` on a mouse
    // is the same defect as a silently-dropped misspelled field, and the caller
    // who wrote it believed they had said something.
    let (mut tree, _id, _log) = laid_out_probe(InputProbe::new("probe"));
    let base = |pointer_id, pressure, tilt| AutomationOp::InjectPointer {
        x: 200.0,
        y: 150.0,
        action: PointerAction::Click,
        button: PointerButtonDto::Primary,
        kind: PointerKindDto::Mouse,
        pointer_id,
        pressure,
        tilt,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        command: false,
    };
    for op in [
        base(Some(7), None, None),
        base(None, Some(0.5), None),
        base(None, None, Some([1.0, 2.0])),
    ] {
        // (a `Click`, so the `pointer_id` refusal here is the mouse's own and
        // not the click-mints-its-own rule tested below)
        match run(&mut tree, &op) {
            AutomationReply::Err { code, .. } => assert_eq!(code, codes::BAD_ARGUMENT),
            other => panic!("a mouse must refuse this, got {other:?} for {op:?}"),
        }
    }
}

#[test]
fn a_touch_step_that_moves_a_finger_that_is_not_down_is_refused() {
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::InjectTouchSequence {
            steps: vec![TouchStep {
                contact: 0,
                phase: TouchPhaseDto::Move,
                x: 10.0,
                y: 10.0,
                advance_ms: 0,
            }],
        },
    );
    match reply {
        AutomationReply::Err { code, .. } => assert_eq!(code, codes::BAD_ARGUMENT),
        other => panic!("expected BAD_ARGUMENT, got {other:?}"),
    }
    assert!(
        log.get().is_empty(),
        "and nothing was dispatched: {:?}",
        log.get()
    );
}

#[test]
fn an_empty_touch_sequence_is_refused() {
    let (mut tree, _id, _log) = laid_out_probe(InputProbe::new("probe"));
    match run(
        &mut tree,
        &AutomationOp::InjectTouchSequence { steps: Vec::new() },
    ) {
        AutomationReply::Err { code, .. } => assert_eq!(code, codes::BAD_ARGUMENT),
        other => panic!("expected BAD_ARGUMENT, got {other:?}"),
    }
}

#[test]
fn a_touch_sequence_advances_the_clock_by_exactly_what_its_steps_asked_for() {
    // A step's interval is what the script wrote, not how long the host took to
    // run two lines of code — which is the whole reason the op freezes the
    // clock before its first sample.
    let (mut tree, _id, _log) = laid_out_probe(InputProbe::new("probe"));
    let before = tree.simulated_now();
    let reply = run(
        &mut tree,
        &AutomationOp::InjectTouchSequence {
            steps: vec![
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Down,
                    x: 200.0,
                    y: 150.0,
                    advance_ms: 0,
                },
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Move,
                    x: 200.0,
                    y: 170.0,
                    advance_ms: 30,
                },
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Up,
                    x: 200.0,
                    y: 170.0,
                    advance_ms: 20,
                },
            ],
        },
    );
    assert!(reply.is_ok(), "sequence ok: {reply:?}");
    let spent = tree.simulated_now().duration_since(before);
    assert_eq!(
        spent,
        Duration::from_millis(50),
        "the clock must move by the steps' own intervals and nothing else"
    );
}

/// A composing widget that builds one child every time it is built — the shape
/// a density switch invalidates, and the one a leaf-only fixture cannot show.
#[derive(Debug, Default)]
struct Rebuilder {
    child: Option<WidgetId>,
}

impl Widget for Rebuilder {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.add(InputProbe::new("child"));
        self.child = Some(id);
        vec![id]
    }
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

#[test]
fn set_density_switches_the_ladder_and_repeating_it_is_a_no_op() {
    let mut tree = WidgetTree::new();
    tree.add(Rebuilder::default());
    tree.layout(SizeProposal::exact(400.0, 300.0));
    assert_eq!(
        tree.input_density(),
        teksilo_tokens::TargetDensity::Compact,
        "a fresh tree is Compact"
    );
    let ids = |tree: &mut WidgetTree| -> Vec<u64> {
        tree.sync_accessibility()
            .nodes
            .iter()
            .map(|(id, _)| id.0)
            .collect()
    };
    let before = ids(&mut tree);

    let reply = run(
        &mut tree,
        &AutomationOp::SetDensity {
            density: DensityDto::Touch,
        },
    );
    assert!(reply.is_ok(), "set_density ok: {reply:?}");
    assert_eq!(tree.input_density(), teksilo_tokens::TargetDensity::Touch);

    // The claim the tool's description and docs/automation-mcp.md both make:
    // a density change rebuilds every root, so every widget a `build()` created
    // comes back with a fresh id and a client's cached ids name nothing.
    let after_first = ids(&mut tree);
    assert_ne!(
        before, after_first,
        "a density change must invalidate the ids a build() created"
    );

    // Setting the density it already has must not rebuild: a repeated set would
    // otherwise invalidate a client's ids for nothing.
    let reply = run(
        &mut tree,
        &AutomationOp::SetDensity {
            density: DensityDto::Touch,
        },
    );
    assert!(reply.is_ok(), "repeat set_density ok: {reply:?}");
    assert_eq!(
        after_first,
        ids(&mut tree),
        "a repeated set_density must keep every node id"
    );
}

#[test]
fn every_new_op_round_trips_through_json() {
    // The wire is the contract: an op that cannot be serialised and read back
    // is unreachable from either transport, whatever the executor does with it.
    let ops = vec![
        AutomationOp::InjectTouchSequence {
            steps: vec![TouchStep {
                contact: 1,
                phase: TouchPhaseDto::Cancel,
                x: 1.0,
                y: 2.0,
                advance_ms: 8,
            }],
        },
        AutomationOp::Pinch {
            ax0: 1.0,
            ay0: 2.0,
            bx0: 3.0,
            by0: 4.0,
            ax1: 5.0,
            ay1: 6.0,
            bx1: 7.0,
            by1: 8.0,
            steps: 3,
        },
        AutomationOp::Fling {
            from_x: 1.0,
            from_y: 2.0,
            to_x: 3.0,
            to_y: 4.0,
            over_ms: 120,
        },
        AutomationOp::LongPress {
            x: 1.0,
            y: 2.0,
            kind: PointerKindDto::Pen,
        },
        AutomationOp::CancelPointer { pointer_id: 42 },
        AutomationOp::QueryPointers,
        AutomationOp::SetDensity {
            density: DensityDto::Comfortable,
        },
    ];
    for op in ops {
        let json = serde_json::to_value(&op).expect("serialise");
        let back: AutomationOp = serde_json::from_value(json.clone()).expect("deserialise");
        assert_eq!(back, op, "round trip changed the op: {json}");
    }

    // Defaults are optional on the wire, so a client need not spell them.
    let terse: AutomationOp = serde_json::from_value(serde_json::json!({
        "InjectTouchSequence": { "steps": [{ "phase": "down", "x": 1.0, "y": 2.0 }] }
    }))
    .expect("a terse sequence parses");
    let AutomationOp::InjectTouchSequence { steps } = terse else {
        panic!("wrong variant");
    };
    assert_eq!(steps[0].contact, 0);
    assert_eq!(steps[0].advance_ms, 0);

    // …and an undeclared one is still refused, on the nested type too.
    serde_json::from_value::<AutomationOp>(serde_json::json!({
        "InjectTouchSequence": {
            "steps": [{ "phase": "down", "x": 1.0, "y": 2.0, "delay_ms": 5 }]
        }
    }))
    .expect_err("an unknown field on a step must be refused");
}

#[test]
fn a_touch_sequence_stamps_every_sample_on_the_simulated_clock() {
    // The op freezes the clock before its first sample, so a step that asked
    // for no interval gets none. On the wall clock two consecutive dispatches
    // are stamped however many nanoseconds apart the host took to run them, and
    // a drag meaning "travel 200 dp, no time passes" would instead describe a
    // flick at some thousands of dp per second — differently on every machine.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let reply = run(
        &mut tree,
        &AutomationOp::InjectTouchSequence {
            steps: vec![
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Down,
                    x: 200.0,
                    y: 250.0,
                    advance_ms: 0,
                },
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Move,
                    x: 200.0,
                    y: 200.0,
                    advance_ms: 0,
                },
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Move,
                    x: 200.0,
                    y: 150.0,
                    advance_ms: 0,
                },
            ],
        },
    );
    assert!(reply.is_ok(), "sequence ok: {reply:?}");
    let stamps: std::collections::BTreeSet<String> = log
        .get()
        .iter()
        .filter_map(|l| l.rsplit(':').next().map(str::to_string))
        .collect();
    assert_eq!(
        stamps.len(),
        1,
        "every sample of an interval-free sequence must carry one stamp: {:?}",
        log.get()
    );
}

#[test]
fn a_move_that_could_mean_either_of_two_fingers_is_refused_and_an_id_resolves_it() {
    // Naming the contact is optional exactly while it is unambiguous. With two
    // fingers down, moving "the finger" would move whichever the table happened
    // to yield first — a silent coin flip in the middle of a gesture.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let report = fingers_down(&mut tree, 2);
    let target = report.live[1].pointer_id;

    let ambiguous = |pointer_id| AutomationOp::InjectPointer {
        x: 300.0,
        y: 200.0,
        action: PointerAction::Move,
        button: PointerButtonDto::Primary,
        kind: PointerKindDto::Touch,
        pointer_id,
        pressure: None,
        tilt: None,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        command: false,
    };
    match run(&mut tree, &ambiguous(None)) {
        AutomationReply::Err { code, message } => {
            assert_eq!(code, codes::BAD_ARGUMENT);
            assert!(
                message.contains('2'),
                "and the refusal must say how many are live: {message}"
            );
        }
        other => panic!("expected BAD_ARGUMENT, got {other:?}"),
    }

    let before = log.get().len();
    assert!(
        run(&mut tree, &ambiguous(Some(target))).is_ok(),
        "naming the contact resolves it"
    );
    let moved: Vec<String> = log.get()[before..].to_vec();
    assert!(
        moved
            .iter()
            .any(|l| l.starts_with("move:touch:") && l.contains(&format!(":id{target}:"))),
        "and it must be the named finger that moved: {moved:?}"
    );
}

#[test]
fn a_click_and_a_down_mint_their_own_contact_and_refuse_to_be_told_one() {
    // `pointer_id` continues a contact that is already on the glass. A down
    // creates an identity and a click is a whole contact's life, so naming one
    // there would let a caller believe they had addressed a finger the op was
    // about to replace.
    let (mut tree, _id, _log) = laid_out_probe(InputProbe::new("probe"));
    let report = fingers_down(&mut tree, 1);
    let live_id = report.live[0].pointer_id;
    let op = |action| AutomationOp::InjectPointer {
        x: 200.0,
        y: 150.0,
        action,
        button: PointerButtonDto::Primary,
        kind: PointerKindDto::Touch,
        pointer_id: Some(live_id),
        pressure: None,
        tilt: None,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        command: false,
    };
    for action in [
        PointerAction::Down,
        PointerAction::Click,
        PointerAction::DoubleClick,
    ] {
        match run(&mut tree, &op(action)) {
            AutomationReply::Err { code, .. } => assert_eq!(
                code,
                codes::BAD_ARGUMENT,
                "{action:?} must refuse a pointer_id"
            ),
            other => panic!("{action:?} must refuse a pointer_id, got {other:?}"),
        }
    }
    // …and a move, which does continue one, accepts it.
    assert!(run(&mut tree, &op(PointerAction::Move)).is_ok());
}

#[test]
fn a_pen_keeps_one_identity_across_a_lift_and_hovers_after_it() {
    // A stylus is singular and it *hovers*, so its table entry outlives a lift
    // the way a mouse's does — which is what lets the pen ops take no id and
    // still address one continuous session. The proof is the identity: four
    // samples spanning a lift must all be the same pointer.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let pen = |action, x: f32| AutomationOp::InjectPointer {
        x,
        y: 150.0,
        action,
        button: PointerButtonDto::Primary,
        kind: PointerKindDto::Pen,
        pointer_id: None,
        pressure: None,
        tilt: None,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        command: false,
    };
    for (action, x) in [
        (PointerAction::Down, 180.0),
        (PointerAction::Move, 200.0),
        (PointerAction::Up, 220.0),
        // In proximity, tip off the glass — the one direct-pointer hover in the
        // framework, and the sample a finger can never produce.
        (PointerAction::Move, 240.0),
    ] {
        assert!(run(&mut tree, &pen(action, x)).is_ok(), "{action:?} ok");
    }
    let seen = log.get();
    let ids: std::collections::BTreeSet<&str> =
        seen.iter().filter_map(|l| l.split(':').nth(2)).collect();
    assert_eq!(ids.len(), 1, "one stylus, one identity: {seen:?}");
    assert!(
        seen.iter().all(|l| l.starts_with("down:pen")
            || l.starts_with("move:pen")
            || l.starts_with("up:pen")),
        "every sample is a pen: {seen:?}"
    );
    // And it is still there afterwards, unlike a finger.
    let AutomationReply::Ok { data } = run(&mut tree, &AutomationOp::QueryPointers) else {
        panic!("query_pointers failed");
    };
    let live: Vec<PointerReport> = serde_json::from_value(data).expect("a pointer list");
    assert_eq!(live.len(), 1, "the stylus survives its lift: {live:?}");
    assert_eq!(live[0].kind, PointerKindDto::Pen);
    assert!(!live[0].down, "and it is no longer pressed: {live:?}");
    let session = live[0].pointer_id;

    // The named-shape ops take no id at all, so a pen one continues the session
    // rather than inventing a second stylus. (`inject_pointer`'s own move / up
    // resolve the same pointer through the sole-live-of-kind rule; this is the
    // path that has only the reuse to go on.)
    let before = log.get().len();
    assert!(
        run(
            &mut tree,
            &AutomationOp::LongPress {
                x: 260.0,
                y: 150.0,
                kind: PointerKindDto::Pen,
            }
        )
        .is_ok(),
        "pen long_press ok"
    );
    let held: Vec<String> = log.get()[before..].to_vec();
    assert!(
        held.iter().any(|l| l.starts_with("down:pen:"))
            && held
                .iter()
                .all(|l| !l.starts_with("down:pen:") || l.contains(&format!(":id{session}:"))),
        "the hold must be the same stylus, not a second one: {held:?}"
    );
}

#[test]
fn a_pen_op_addresses_the_live_stylus_whatever_tool_it_reports() {
    // A digitizer reports which end of the stylus is down — `Pen`, `Eraser`, a
    // brush, a lens — and the wire has one `pen`, so a pen op that matched the
    // live pointer by exact kind would fail to find a stylus a real backend had
    // classified as anything but the generic tip. Unreachable by minting from
    // the wire, so the fixture puts an eraser in the table the way a backend
    // does, through the tree's own pointer door.
    let (mut tree, _id, log) = laid_out_probe(InputProbe::new("probe"));
    let id = tree.new_contact();
    let mut pointer = teksilo_core::PointerInfo::touch(id, tree.input_now());
    pointer.kind = teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::Eraser);
    pointer.buttons = teksilo_core::event::ButtonMask::PRIMARY;
    tree.dispatch_pointer(teksilo_core::PointerSample {
        pointer,
        phase: teksilo_core::PointerPhase::Down,
        position: teksilo_canvas::Point::new(200.0, 150.0),
        button: Some(teksilo_core::PointerButton::Primary),
        modifiers: teksilo_core::event::Modifiers::NONE,
        coalesced: Vec::new(),
    });
    let before = log.get().len();

    let reply = run(
        &mut tree,
        &AutomationOp::InjectPointer {
            x: 220.0,
            y: 150.0,
            action: PointerAction::Move,
            button: PointerButtonDto::Primary,
            kind: PointerKindDto::Pen,
            pointer_id: None,
            pressure: None,
            tilt: None,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            command: false,
        },
    );
    assert!(reply.is_ok(), "the eraser is the live stylus: {reply:?}");
    let moved: Vec<String> = log.get()[before..].to_vec();
    assert!(
        moved
            .iter()
            .any(|l| l.starts_with("move:pen:") && l.contains(&format!(":id{}:", id.get()))),
        "and the move must address it: {moved:?}"
    );
}
