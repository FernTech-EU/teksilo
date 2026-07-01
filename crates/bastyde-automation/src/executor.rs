// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The single core function that performs every automation operation
//! against a live [`WidgetTree`].
//!
//! ```text
//! execute(tree: &mut WidgetTree, ops: &mut dyn WindowOps,
//!         op: &AutomationOp, settle: &SettleSpec) -> AutomationReply
//! ```
//!
//! `WidgetTree` is `Rc/RefCell`-based and therefore `!Send`, so it lives on
//! exactly one thread; the async / socket layers marshal `Send` DTOs to it
//! and call this. Two ops can't be served here — `ListWindows` (needs the
//! window manager) and `Screenshot` (needs a GPU / platform window). Both
//! return [`codes::HOST_REQUIRED`]; the headless tree-thread and the live
//! bridge intercept them with the extra context they alone hold.

use std::time::{Duration, Instant};

use bastyde_canvas::{Point, Rect};
use bastyde_core::WidgetTree;
use bastyde_core::accesskit;
use bastyde_core::event::{Key, Modifiers, ScrollDelta, WidgetEvent};
use bastyde_core::widget_id::WidgetId;
use bastyde_core::window::WindowOps;

use crate::dto::{
    AnnouncementDto, Assertion, AssertionResult, AutomationOp, AutomationReply, NodeBounds,
    NodeRef, SemanticNode, SettleSpec, ShortcutInfo, WaitCondition, codes,
};

/// Perform one automation operation. See the module docs.
pub fn execute(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    op: &AutomationOp,
    settle: &SettleSpec,
) -> AutomationReply {
    match op {
        // ---- Query ----
        AutomationOp::SnapshotTree { max_depth } => {
            let update = tree.sync_accessibility();
            AutomationReply::ok(snapshot_json(&update, *max_depth))
        }
        AutomationOp::ReadNode { node } => {
            let update = tree.sync_accessibility();
            match find_node(&update, *node) {
                Some(sn) => AutomationReply::ok_json(&sn),
                None => AutomationReply::err(codes::NOT_FOUND, format!("no node {node}")),
            }
        }
        AutomationOp::LayoutTree {
            max_depth,
            include_debug,
        } => AutomationReply::ok(layout_tree_json(tree, *max_depth, *include_debug)),
        AutomationOp::InspectNode { node } => {
            let nid = accesskit::NodeId(*node);
            if bastyde_core::accessibility::is_synthetic(nid) {
                return AutomationReply::err(
                    codes::NOT_FOUND,
                    "synthetic node has no backing widget — use read_node for its AT detail",
                );
            }
            let widget = bastyde_core::accessibility::node_id_to_widget_id_maybe(nid)
                .filter(|w| tree.widget_type_name(*w).is_some());
            match widget {
                Some(w) => AutomationReply::ok_json(&layout_node(tree, w, true)),
                None => {
                    AutomationReply::err(codes::NOT_FOUND, format!("no widget for node {node}"))
                }
            }
        }
        AutomationOp::FindNode { role, label } => {
            let update = tree.sync_accessibility();
            let found = find_node_ref(&update, role.as_deref(), label.as_deref());
            AutomationReply::ok(serde_json::json!({ "node": found }))
        }
        AutomationOp::AssertNode { node, assertion } => {
            let update = tree.sync_accessibility();
            AutomationReply::ok_json(&evaluate_assertion(&update, *node, assertion))
        }
        AutomationOp::ListWindows => AutomationReply::err(
            codes::HOST_REQUIRED,
            "list_windows is served by the host (window manager / headless shim)",
        ),

        // ---- AT-action driving ----
        AutomationOp::InvokeAction { node, action } => {
            let Some(act) = action_from_str(action) else {
                return AutomationReply::err(
                    codes::UNKNOWN_NAME,
                    format!("unknown action '{action}'"),
                );
            };
            dispatch_action_and_settle(tree, ops, settle, *node, act, None)
        }
        AutomationOp::FocusNode { node } => {
            dispatch_action_and_settle(tree, ops, settle, *node, accesskit::Action::Focus, None)
        }
        AutomationOp::SetValue { node, value } => dispatch_action_and_settle(
            tree,
            ops,
            settle,
            *node,
            accesskit::Action::SetValue,
            Some(accesskit::ActionData::Value(value.clone().into_boxed_str())),
        ),
        AutomationOp::Expand { node } => {
            dispatch_action_and_settle(tree, ops, settle, *node, accesskit::Action::Expand, None)
        }
        AutomationOp::Collapse { node } => {
            dispatch_action_and_settle(tree, ops, settle, *node, accesskit::Action::Collapse, None)
        }
        AutomationOp::Scroll { node, dx, dy } => {
            let update = tree.sync_accessibility();
            let Some(widget) = resolve_widget(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            let c = center(tree.bounds(widget));
            // Route the wheel: hover the target first (scroll dispatches to
            // the hovered/focused widget), then deliver the delta.
            tree.pointer_move(c);
            tree.dispatch_event_with_ops(
                WidgetEvent::Scroll {
                    delta: ScrollDelta::Pixels { x: *dx, y: *dy },
                    modifiers: Modifiers::NONE,
                },
                ops,
            );
            finish_settle(tree, ops, settle)
        }

        // ---- Synthetic input ----
        AutomationOp::InjectPointer {
            x,
            y,
            action,
            button,
        } => {
            use crate::dto::PointerAction as PA;
            let p = Point::new(*x, *y);
            let btn = button.to_core();
            match action {
                PA::Move => tree.pointer_move(p),
                PA::Down => tree.pointer_down_button(p, btn),
                PA::Up => tree.pointer_up_button(p, btn),
                PA::Click => {
                    tree.pointer_down_button(p, btn);
                    tree.pointer_up_button(p, btn);
                }
            }
            finish_settle(tree, ops, settle)
        }
        AutomationOp::InjectKey {
            key,
            ctrl,
            shift,
            alt,
            meta,
        } => {
            let Some(k) = key_from_str(key) else {
                return AutomationReply::err(codes::UNKNOWN_NAME, format!("unknown key '{key}'"));
            };
            tree.press_key(k, modifiers(*ctrl, *shift, *alt, *meta));
            finish_settle(tree, ops, settle)
        }
        AutomationOp::TypeText { node, text } => {
            let update = tree.sync_accessibility();
            let Some(widget) = resolve_widget(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            // `type_text` routes to the *focused* widget, so focus first.
            tree.focus_ops(widget, ops);
            tree.type_text(widget, text);
            finish_settle(tree, ops, settle)
        }
        AutomationOp::TypeIme {
            node,
            preedit,
            commit,
        } => {
            let update = tree.sync_accessibility();
            let Some(widget) = resolve_widget(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            tree.focus_ops(widget, ops);
            if let Some(text) = preedit {
                tree.dispatch_event_with_ops(
                    WidgetEvent::ImeComposition {
                        text: text.clone(),
                        cursor: None,
                    },
                    ops,
                );
            }
            if let Some(text) = commit {
                tree.dispatch_event_with_ops(WidgetEvent::ImeCommit { text: text.clone() }, ops);
            }
            finish_settle(tree, ops, settle)
        }
        AutomationOp::DragNode {
            node,
            to_node,
            to_x,
            to_y,
        } => {
            let update = tree.sync_accessibility();
            let Some(from) = node_point(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            let to = if let Some(tn) = to_node {
                match node_point(tree, &update, *tn) {
                    Some(p) => p,
                    None => {
                        return AutomationReply::err(codes::NOT_FOUND, format!("no node {tn}"));
                    }
                }
            } else if let (Some(x), Some(y)) = (to_x, to_y) {
                Point::new(*x, *y)
            } else {
                return AutomationReply::err(
                    codes::BAD_ARGUMENT,
                    "drag_node needs to_node or (to_x, to_y)",
                );
            };
            tree.drag(from, to);
            finish_settle(tree, ops, settle)
        }

        // ---- Introspection ----
        AutomationOp::GetOverlays => {
            let overlays = tree.active_overlays();
            let ids: Vec<String> = overlays.iter().map(|o| format!("{o:?}")).collect();
            AutomationReply::ok(serde_json::json!({ "count": overlays.len(), "ids": ids }))
        }
        AutomationOp::GetShortcuts => {
            let list: Vec<ShortcutInfo> = tree
                .shortcut_registry()
                .iter_effective()
                .map(|eff| ShortcutInfo {
                    id: eff.shortcut.id.to_string(),
                    name: Some(eff.shortcut.name.get()).filter(|n| !n.is_empty()),
                    primary: eff.primary.map(format_keystroke),
                    secondary: eff.secondary.map(format_keystroke),
                    enabled: eff.enabled,
                })
                .collect();
            AutomationReply::ok_json(&list)
        }
        AutomationOp::ListLiveRegions => {
            let update = tree.sync_accessibility();
            let focus = update.focus;
            let regions: Vec<SemanticNode> = update
                .nodes
                .iter()
                .filter(|(_, n)| {
                    matches!(
                        n.live(),
                        Some(accesskit::Live::Polite) | Some(accesskit::Live::Assertive)
                    )
                })
                .map(|(id, n)| semantic_node(*id, n, focus))
                .collect();
            AutomationReply::ok_json(&regions)
        }
        AutomationOp::PullAnnouncements { since_seq } => {
            // Re-sync so the latest rebuild's announcements are captured.
            tree.sync_accessibility();
            let list: Vec<AnnouncementDto> = tree
                .announcements_since(*since_seq)
                .into_iter()
                .map(AnnouncementDto::from)
                .collect();
            AutomationReply::ok_json(&list)
        }

        // ---- Time / settle ----
        AutomationOp::AdvanceClock { millis } => {
            tree.advance_time(Duration::from_millis(*millis));
            tree.sync_accessibility();
            AutomationReply::ok_unit()
        }
        AutomationOp::Settle => finish_settle(tree, ops, settle),
        AutomationOp::WaitForCondition { condition } => {
            wait_for_condition(tree, ops, settle, condition)
        }

        // ---- Visual (host-handled) ----
        AutomationOp::Screenshot { .. } => AutomationReply::err(
            codes::HOST_REQUIRED,
            "screenshot pixels are produced by the host (offscreen renderer / platform window)",
        ),
    }
}

// ---------------------------------------------------------------------------
// Settle
// ---------------------------------------------------------------------------

/// Run the settle described by `settle` and then re-sync the AT tree.
/// Returns `Some(code)` if the wall-clock budget was exceeded (the loop is
/// sim-clock-driven, so it can only overrun on a pathological animation),
/// else `None`.
pub fn run_settle(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
) -> Option<&'static str> {
    let deadline = Instant::now() + Duration::from_millis(settle.settle_timeout_ms.max(1));
    if settle.clock_millis > 0 {
        tree.advance_time(Duration::from_millis(settle.clock_millis));
    }
    let mut frames = 0u32;
    let mut timed_out = false;
    while tree.has_active_animations() && frames < settle.max_anim_frames {
        tree.tick_animations(Duration::from_millis(16));
        frames += 1;
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
    }
    if settle.layout_after {
        let proposal = tree.last_proposal();
        tree.layout_with_ops(proposal, ops);
    }
    tree.sync_accessibility();
    timed_out.then_some(codes::SETTLE_TIMEOUT)
}

fn finish_settle(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
) -> AutomationReply {
    match run_settle(tree, ops, settle) {
        Some(code) => AutomationReply::err(code, "settle exceeded its time budget"),
        None => AutomationReply::ok_unit(),
    }
}

fn dispatch_action_and_settle(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
    node: NodeRef,
    action: accesskit::Action,
    data: Option<accesskit::ActionData>,
) -> AutomationReply {
    // Sync first so the synthetic-parent map is fresh AND so we can confirm
    // the node is actually live: `node_id_to_widget_id_maybe` happily decodes
    // any non-synthetic u64 into a `WidgetId`, so presence in the AT tree —
    // not a non-`None` resolution — is the real liveness check.
    let update = tree.sync_accessibility();
    if !node_present(&update, node) {
        return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
    }
    tree.dispatch_access_action(accesskit::NodeId(node), action, data, ops);
    finish_settle(tree, ops, settle)
}

fn wait_for_condition(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
    condition: &WaitCondition,
) -> AutomationReply {
    let deadline = Instant::now() + Duration::from_millis(settle.settle_timeout_ms.max(1));
    loop {
        let update = tree.sync_accessibility();
        if condition_met(tree, &update, condition) {
            return AutomationReply::ok_unit();
        }
        if Instant::now() >= deadline {
            return AutomationReply::err(
                codes::WAIT_TIMEOUT,
                "wait_for_condition timed out before the predicate held",
            );
        }
        // Drive timed / animated state forward one frame, then re-layout so
        // reactive (AccessibilityOnly) bindings flush before the next sync.
        tree.advance_time(Duration::from_millis(16));
        tree.tick_animations(Duration::from_millis(16));
        let proposal = tree.last_proposal();
        tree.layout_with_ops(proposal, ops);
        // Yield the CPU between polls. The loop body is pure in-memory work
        // (no VSync / OS wait), so without this it would pin a core at 100% and
        // — on the single tree-owning thread — starve other queued tool calls
        // for the whole timeout. The sim clock still advances above, so this
        // doesn't change settle semantics.
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn condition_met(
    tree: &WidgetTree,
    update: &accesskit::TreeUpdate,
    condition: &WaitCondition,
) -> bool {
    match condition {
        WaitCondition::NodeExists { role, label } => {
            find_node_ref(update, role.as_deref(), label.as_deref()).is_some()
        }
        WaitCondition::NodeValue { node, expected } => update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == *node)
            .map(|(_, n)| n.value() == Some(expected.as_str()))
            .unwrap_or(false),
        WaitCondition::NodeGone { node } => !update.nodes.iter().any(|(id, _)| id.0 == *node),
        WaitCondition::AtVersionAtLeast { version } => tree.at_version().get() >= *version,
    }
}

// ---------------------------------------------------------------------------
// Node / tree helpers
// ---------------------------------------------------------------------------

/// Whether `node` is present in the freshly-synced AT tree (the reliable
/// liveness check — see [`dispatch_action_and_settle`]).
fn node_present(update: &accesskit::TreeUpdate, node: NodeRef) -> bool {
    update.nodes.iter().any(|(id, _)| id.0 == node)
}

/// Resolve a *present* [`NodeRef`] to its owning [`WidgetId`] — directly for
/// a widget node, or via the synthetic-parent map for a widget-emitted
/// child. Returns `None` when the node isn't in the live tree.
fn resolve_widget(
    tree: &WidgetTree,
    update: &accesskit::TreeUpdate,
    node: NodeRef,
) -> Option<WidgetId> {
    if !node_present(update, node) {
        return None;
    }
    let nid = accesskit::NodeId(node);
    bastyde_core::accessibility::node_id_to_widget_id_maybe(nid)
        .or_else(|| tree.widget_for_synthetic(nid))
}

fn center(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

/// The pointer point to use when driving a gesture at `node`. Prefers the
/// node's own AT bounds — correct for *synthetic* children (scene items,
/// rich-text runs) whose owning widget may span far more area than the child
/// — and falls back to the owning widget's arena bounds. `None` only when the
/// node is absent from the live tree.
fn node_point(tree: &WidgetTree, update: &accesskit::TreeUpdate, node: NodeRef) -> Option<Point> {
    if let Some((_, n)) = update.nodes.iter().find(|(id, _)| id.0 == node)
        && let Some(r) = n.bounds()
    {
        return Some(Point::new(
            ((r.x0 + r.x1) * 0.5) as f32,
            ((r.y0 + r.y1) * 0.5) as f32,
        ));
    }
    let widget = resolve_widget(tree, update, node)?;
    Some(center(tree.bounds(widget)))
}

/// Build a [`SemanticNode`] from a raw AccessKit node.
fn semantic_node(
    id: accesskit::NodeId,
    node: &accesskit::Node,
    focus: accesskit::NodeId,
) -> SemanticNode {
    let toggled = node.toggled().map(|t| {
        match t {
            accesskit::Toggled::True => "true",
            accesskit::Toggled::False => "false",
            accesskit::Toggled::Mixed => "mixed",
        }
        .to_string()
    });
    let live = match node.live() {
        Some(accesskit::Live::Polite) => Some("polite".to_string()),
        Some(accesskit::Live::Assertive) => Some("assertive".to_string()),
        _ => None,
    };
    let bounds = node.bounds().map(|r| NodeBounds {
        x: r.x0,
        y: r.y0,
        width: r.x1 - r.x0,
        height: r.y1 - r.y0,
    });
    let actions = ADVERTISABLE_ACTIONS
        .iter()
        .filter(|(a, _)| node.supports_action(*a))
        .map(|(_, name)| name.to_string())
        .collect();
    SemanticNode {
        id: id.0,
        role: format!("{:?}", node.role()),
        label: node.label().map(|s| s.to_string()),
        value: node.value().map(|s| s.to_string()),
        toggled,
        expanded: node.is_expanded(),
        selected: node.is_selected(),
        disabled: node.is_disabled(),
        focused: id == focus,
        live,
        numeric_value: node.numeric_value(),
        bounds,
        actions,
        children: node.children().iter().map(|c| c.0).collect(),
    }
}

/// Build a [`LayoutNode`](crate::dto::LayoutNode) for one arena widget.
fn layout_node(tree: &WidgetTree, id: WidgetId, include_debug: bool) -> crate::dto::LayoutNode {
    let to_ref = |w: WidgetId| bastyde_core::accessibility::widget_id_to_node_id(w).0;
    let b = tree.bounds(id);
    crate::dto::LayoutNode {
        id: to_ref(id),
        type_name: tree
            .widget_type_name(id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string()),
        bounds: NodeBounds {
            x: b.x as f64,
            y: b.y as f64,
            width: b.width as f64,
            height: b.height as f64,
        },
        active: tree.is_active(id),
        clips_children: tree.widget_clips_children(id),
        parent: tree.parent(id).map(to_ref),
        children: tree.children(id).into_iter().map(to_ref).collect(),
        debug: if include_debug {
            tree.widget_debug_string(id)
        } else {
            None
        },
    }
}

/// Walk the arena widget tree from the roots (BFS, depth-capped), keying every
/// widget by the same `NodeRef` space as the AT tools.
fn layout_tree_json(
    tree: &WidgetTree,
    max_depth: Option<usize>,
    include_debug: bool,
) -> serde_json::Value {
    use std::collections::{HashSet, VecDeque};
    let to_ref = |w: WidgetId| bastyde_core::accessibility::widget_id_to_node_id(w).0;
    let roots = tree.roots();
    let mut out: Vec<crate::dto::LayoutNode> = Vec::new();
    let mut seen: HashSet<WidgetId> = HashSet::new();
    let mut queue: VecDeque<(WidgetId, usize)> = roots.iter().map(|r| (*r, 0usize)).collect();
    while let Some((id, depth)) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let descend = max_depth.map(|d| depth < d).unwrap_or(true);
        let mut node = layout_node(tree, id, include_debug);
        if !descend {
            // At the cap: drop child refs so there are no dangling ids.
            node.children.clear();
        }
        out.push(node);
        if descend {
            for c in tree.children(id) {
                queue.push_back((c, depth + 1));
            }
        }
    }
    serde_json::json!({
        "roots": roots.into_iter().map(to_ref).collect::<Vec<_>>(),
        "nodes": out,
    })
}

fn find_node(update: &accesskit::TreeUpdate, node: NodeRef) -> Option<SemanticNode> {
    let focus = update.focus;
    update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == node)
        .map(|(id, n)| semantic_node(*id, n, focus))
}

/// First node (in AT/build order) whose role and/or label match. A `None`
/// filter matches anything; role compares against the role's `Debug` name
/// case-insensitively; label compares for exact equality.
fn find_node_ref(
    update: &accesskit::TreeUpdate,
    role: Option<&str>,
    label: Option<&str>,
) -> Option<NodeRef> {
    update
        .nodes
        .iter()
        .find(|(_, n)| {
            let role_ok = role
                .map(|r| format!("{:?}", n.role()).eq_ignore_ascii_case(r))
                .unwrap_or(true);
            let label_ok = label.map(|l| n.label() == Some(l)).unwrap_or(true);
            role_ok && label_ok
        })
        .map(|(id, _)| id.0)
}

fn snapshot_json(update: &accesskit::TreeUpdate, max_depth: Option<usize>) -> serde_json::Value {
    use std::collections::{HashMap, HashSet, VecDeque};
    let focus = update.focus;
    let map: HashMap<accesskit::NodeId, &accesskit::Node> =
        update.nodes.iter().map(|(id, n)| (*id, n)).collect();
    let root = update.tree.as_ref().map(|t| t.root);
    let mut out: Vec<SemanticNode> = Vec::new();
    match root {
        Some(root) => {
            let mut seen: HashSet<accesskit::NodeId> = HashSet::new();
            let mut queue: VecDeque<(accesskit::NodeId, usize)> = VecDeque::new();
            queue.push_back((root, 0));
            while let Some((nid, depth)) = queue.pop_front() {
                if !seen.insert(nid) {
                    continue;
                }
                if let Some(node) = map.get(&nid) {
                    let descend = max_depth.map(|d| depth < d).unwrap_or(true);
                    let mut sn = semantic_node(nid, node, focus);
                    // At the depth cap the children aren't emitted, so drop the
                    // child refs rather than leave dangling ids pointing at
                    // nodes absent from `nodes`.
                    if !descend {
                        sn.children.clear();
                    }
                    out.push(sn);
                    if descend {
                        for c in node.children() {
                            queue.push_back((*c, depth + 1));
                        }
                    }
                }
            }
        }
        None => {
            for (id, n) in &update.nodes {
                out.push(semantic_node(*id, n, focus));
            }
        }
    }
    serde_json::json!({
        "root": root.map(|r| r.0),
        "focus": focus.0,
        "nodes": out,
    })
}

fn evaluate_assertion(
    update: &accesskit::TreeUpdate,
    node: NodeRef,
    assertion: &Assertion,
) -> AssertionResult {
    let found = update.nodes.iter().find(|(id, _)| id.0 == node);
    let pass = |passed: bool, detail: Option<String>| AssertionResult { passed, detail };
    let Some((id, n)) = found else {
        // Only `Exists` can pass on a missing node (as `false`).
        return pass(false, Some(format!("node {node} not present")));
    };
    match assertion {
        Assertion::Exists => pass(true, None),
        Assertion::Focused => {
            let ok = id.0 == update.focus.0;
            pass(ok, (!ok).then(|| "node is not focused".to_string()))
        }
        Assertion::RoleEquals { value } => {
            let actual = format!("{:?}", n.role());
            let ok = actual.eq_ignore_ascii_case(value);
            pass(
                ok,
                (!ok).then(|| format!("role is '{actual}', expected '{value}'")),
            )
        }
        Assertion::LabelEquals { value } => {
            let actual = n.label();
            let ok = actual == Some(value.as_str());
            pass(
                ok,
                (!ok).then(|| format!("label is {actual:?}, expected '{value}'")),
            )
        }
        Assertion::LabelContains { value } => {
            let actual = n.label().unwrap_or("");
            let ok = actual.contains(value.as_str());
            pass(
                ok,
                (!ok).then(|| format!("label '{actual}' does not contain '{value}'")),
            )
        }
        Assertion::ValueEquals { value } => {
            let actual = n.value();
            let ok = actual == Some(value.as_str());
            pass(
                ok,
                (!ok).then(|| format!("value is {actual:?}, expected '{value}'")),
            )
        }
        Assertion::Toggled { value } => {
            // A bool assertion must NOT collapse `Mixed` (tristate /
            // indeterminate) into `false`: `Toggled { value: false }` on a
            // partially-checked parent checkbox should FAIL, not silently pass.
            let state = n.toggled();
            let ok = matches!(
                (value, state),
                (true, Some(accesskit::Toggled::True)) | (false, Some(accesskit::Toggled::False))
            );
            let actual = match state {
                Some(accesskit::Toggled::True) => "true",
                Some(accesskit::Toggled::False) => "false",
                Some(accesskit::Toggled::Mixed) => "mixed",
                None => "none",
            };
            pass(
                ok,
                (!ok).then(|| format!("toggled is {actual}, expected {value}")),
            )
        }
        Assertion::Expanded { value } => {
            let actual = n.is_expanded().unwrap_or(false);
            let ok = actual == *value;
            pass(
                ok,
                (!ok).then(|| format!("expanded is {actual}, expected {value}")),
            )
        }
        Assertion::Selected { value } => {
            let actual = n.is_selected().unwrap_or(false);
            let ok = actual == *value;
            pass(
                ok,
                (!ok).then(|| format!("selected is {actual}, expected {value}")),
            )
        }
        Assertion::Disabled { value } => {
            let actual = n.is_disabled();
            let ok = actual == *value;
            pass(
                ok,
                (!ok).then(|| format!("disabled is {actual}, expected {value}")),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Name <-> enum mapping
// ---------------------------------------------------------------------------

/// Actions surfaced in a `SemanticNode.actions` list, paired with the
/// snake_case name an automation client uses in `invoke_action`.
const ADVERTISABLE_ACTIONS: &[(accesskit::Action, &str)] = &[
    (accesskit::Action::Click, "click"),
    (accesskit::Action::Focus, "focus"),
    (accesskit::Action::Increment, "increment"),
    (accesskit::Action::Decrement, "decrement"),
    (accesskit::Action::Expand, "expand"),
    (accesskit::Action::Collapse, "collapse"),
    (accesskit::Action::SetValue, "set_value"),
    (accesskit::Action::ShowContextMenu, "show_context_menu"),
    (accesskit::Action::ScrollIntoView, "scroll_into_view"),
    (accesskit::Action::ScrollUp, "scroll_up"),
    (accesskit::Action::ScrollDown, "scroll_down"),
    (accesskit::Action::ScrollLeft, "scroll_left"),
    (accesskit::Action::ScrollRight, "scroll_right"),
];

/// Map an automation action name to an `accesskit::Action`. Accepts the
/// snake_case names plus a few intuitive aliases.
fn action_from_str(s: &str) -> Option<accesskit::Action> {
    use accesskit::Action as A;
    let lower = s.to_ascii_lowercase();
    Some(match lower.as_str() {
        "click" | "default" | "press" | "activate" => A::Click,
        "focus" => A::Focus,
        "blur" => A::Blur,
        "increment" => A::Increment,
        "decrement" => A::Decrement,
        "expand" => A::Expand,
        "collapse" => A::Collapse,
        "set_value" => A::SetValue,
        "show_context_menu" | "context_menu" => A::ShowContextMenu,
        "scroll_into_view" => A::ScrollIntoView,
        "scroll_up" => A::ScrollUp,
        "scroll_down" => A::ScrollDown,
        "scroll_left" => A::ScrollLeft,
        "scroll_right" => A::ScrollRight,
        "show_tooltip" => A::ShowTooltip,
        "hide_tooltip" => A::HideTooltip,
        _ => return None,
    })
}

fn modifiers(ctrl: bool, shift: bool, alt: bool, meta: bool) -> Modifiers {
    let mut m = Modifiers::NONE;
    if ctrl {
        m = m | Modifiers::CTRL;
    }
    if shift {
        m = m | Modifiers::SHIFT;
    }
    if alt {
        m = m | Modifiers::ALT;
    }
    if meta {
        m = m | Modifiers::SUPER;
    }
    m
}

fn format_keystroke(ks: bastyde_core::shortcut::KeyStroke) -> String {
    // `Modifiers` Display emits a trailing "+", e.g. "Ctrl+"; `Key` Display
    // emits the key name. Together: "Ctrl+S".
    format!("{}{}", ks.modifiers, ks.key)
}

/// Map an automation key name to a [`Key`]. Accepts named keys
/// (case-insensitive), single characters, and ASCII letters.
fn key_from_str(s: &str) -> Option<Key> {
    let lower = s.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" | " " => Some(Key::Space),
        "enter" | "return" => Some(Key::Enter),
        "escape" | "esc" => Some(Key::Escape),
        "tab" => Some(Key::Tab),
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "up" | "arrowup" => Some(Key::ArrowUp),
        "down" | "arrowdown" => Some(Key::ArrowDown),
        "left" | "arrowleft" => Some(Key::ArrowLeft),
        "right" | "arrowright" => Some(Key::ArrowRight),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "capslock" => Some(Key::CapsLock),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    // A single character.
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => {
            // ASCII letters MUST map to the named `Key::A..Key::Z` variants,
            // not `Key::Character`: shortcuts register with `Key::S` etc., and
            // `KeyStroke` equality is by variant, so `inject_key {key:"s",
            // ctrl:true}` would otherwise never fire a `Ctrl+S` shortcut.
            if ch.is_ascii_alphabetic() {
                Some(letter_key(ch.to_ascii_uppercase()))
            } else {
                Some(Key::Character(ch))
            }
        }
        _ => None,
    }
}

/// Map an uppercase ASCII letter to its `Key::A..Key::Z` variant.
fn letter_key(upper: char) -> Key {
    match upper {
        'A' => Key::A,
        'B' => Key::B,
        'C' => Key::C,
        'D' => Key::D,
        'E' => Key::E,
        'F' => Key::F,
        'G' => Key::G,
        'H' => Key::H,
        'I' => Key::I,
        'J' => Key::J,
        'K' => Key::K,
        'L' => Key::L,
        'M' => Key::M,
        'N' => Key::N,
        'O' => Key::O,
        'P' => Key::P,
        'Q' => Key::Q,
        'R' => Key::R,
        'S' => Key::S,
        'T' => Key::T,
        'U' => Key::U,
        'V' => Key::V,
        'W' => Key::W,
        'X' => Key::X,
        'Y' => Key::Y,
        // Only reached for ASCII alphabetic chars, so 'Z' is the last case.
        _ => Key::Z,
    }
}
