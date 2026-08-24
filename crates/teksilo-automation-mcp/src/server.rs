// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The rmcp server: one `#[tool]` handler per automation tool.
//!
//! Each async handler builds an [`AutomationOp`] from its typed parameters,
//! marshals it (with a `oneshot` reply channel) to the tree thread over the
//! [`Job`] channel, awaits the reply, and converts it to a `CallToolResult`.
//! The `!Send` tree never crosses the channel; only `Send` DTOs do.

use base64::Engine;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{ErrorData as McpError, ServerHandler, schemars, tool, tool_handler, tool_router};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use teksilo_automation::dto::{
    Assertion, AutomationOp, AutomationReply, AutomationRequest, PointerAction, PointerButtonDto,
    SettleSpec, WaitCondition,
};

use crate::headless::{HostReply, Job};

/// The MCP server. Cloneable (rmcp requirement); clones share the same job
/// channel and tool router.
#[derive(Clone)]
pub struct AutomationServer {
    job_tx: UnboundedSender<Job>,
    tool_router: ToolRouter<AutomationServer>,
}

// ---------------------------------------------------------------------------
// Tool parameter structs
// ---------------------------------------------------------------------------

/// Optional settle policy, accepted by every mutating tool.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettleArg {
    pub clock_millis: Option<u64>,
    pub max_anim_frames: Option<u32>,
    pub layout_after: Option<bool>,
    pub settle_timeout_ms: Option<u64>,
}

fn settle_spec(arg: &Option<SettleArg>) -> SettleSpec {
    let d = SettleSpec::default();
    match arg {
        None => d,
        Some(s) => SettleSpec {
            clock_millis: s.clock_millis.unwrap_or(d.clock_millis),
            max_anim_frames: s.max_anim_frames.unwrap_or(d.max_anim_frames),
            layout_after: s.layout_after.unwrap_or(d.layout_after),
            settle_timeout_ms: s.settle_timeout_ms.unwrap_or(d.settle_timeout_ms),
        },
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotParams {
    pub window_id: Option<u64>,
    /// Optional depth limit from the root (omit for the whole tree).
    pub max_depth: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadParams {
    pub window_id: Option<u64>,
    pub node: u64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutTreeParams {
    pub window_id: Option<u64>,
    /// Optional depth limit from the roots (omit for the whole tree).
    pub max_depth: Option<usize>,
    /// Include each widget's Debug repr (its parameters). Off by default — it
    /// can be large.
    pub include_debug: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    pub window_id: Option<u64>,
    pub node: u64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeParams {
    pub window_id: Option<u64>,
    pub node: u64,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindParams {
    pub window_id: Option<u64>,
    pub role: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssertParams {
    pub window_id: Option<u64>,
    pub node: u64,
    /// One of: exists, focused, role_equals, label_equals, label_contains,
    /// value_equals, toggled, expanded, selected, disabled.
    pub kind: String,
    /// The expected string, for the *_equals / *_contains kinds.
    pub value: Option<String>,
    /// The expected bool, for toggled / expanded / selected / disabled.
    pub flag: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvokeParams {
    pub window_id: Option<u64>,
    pub node: u64,
    /// AT action name, e.g. click, focus, expand, collapse, set_value,
    /// increment, decrement, show_context_menu. `show_context_menu` opens the
    /// node's context menu (same result as the `right_click` tool, which is the
    /// clearer verb for that).
    pub action: String,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetValueParams {
    pub window_id: Option<u64>,
    pub node: u64,
    pub value: String,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrollParams {
    pub window_id: Option<u64>,
    pub node: u64,
    pub dx: Option<f32>,
    pub dy: Option<f32>,
    /// Modifiers held during the wheel, as for `inject_key`. A modifier-held
    /// wheel is its own gesture — Ctrl+wheel to zoom is why
    /// `WidgetEvent::Scroll` carries modifiers at all — so a probe needs to be
    /// able to send one. All default to false, i.e. a plain wheel.
    pub ctrl: Option<bool>,
    pub shift: Option<bool>,
    pub alt: Option<bool>,
    pub meta: Option<bool>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InjectPointerParams {
    pub window_id: Option<u64>,
    pub x: f32,
    pub y: f32,
    /// click (default), double_click, down, up, or move.
    pub action: Option<String>,
    /// Modifiers held for the press and the release. Ctrl-click to extend a
    /// selection is its own gesture, not a click with decoration.
    pub ctrl: Option<bool>,
    pub shift: Option<bool>,
    pub alt: Option<bool>,
    pub meta: Option<bool>,
    /// primary (default), secondary, middle, back, forward. `secondary` is a
    /// right-click (opens a context menu); prefer the node-based `right_click`
    /// tool for that so you don't have to compute a point.
    pub button: Option<String>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InjectKeyParams {
    pub window_id: Option<u64>,
    /// Key name (Enter, Escape, Tab, F1.., arrows) or a single character.
    pub key: String,
    pub ctrl: Option<bool>,
    pub shift: Option<bool>,
    pub alt: Option<bool>,
    pub meta: Option<bool>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeTextParams {
    pub window_id: Option<u64>,
    pub node: u64,
    pub text: String,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeImeParams {
    pub window_id: Option<u64>,
    pub node: u64,
    pub preedit: Option<String>,
    pub commit: Option<String>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragParams {
    pub window_id: Option<u64>,
    pub node: u64,
    pub to_node: Option<u64>,
    pub to_x: Option<f32>,
    pub to_y: Option<f32>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowOnlyParams {
    pub window_id: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PullParams {
    pub window_id: Option<u64>,
    pub since_seq: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdvanceParams {
    pub window_id: Option<u64>,
    pub millis: u64,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettleParams {
    pub window_id: Option<u64>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitParams {
    pub window_id: Option<u64>,
    /// One of: node_exists, node_value, node_gone, at_version_at_least.
    pub kind: String,
    pub node: Option<u64>,
    pub role: Option<String>,
    pub label: Option<String>,
    pub expected: Option<String>,
    pub version: Option<u64>,
    pub settle: Option<SettleArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotParams {
    pub window_id: Option<u64>,
    /// Crop to this node's bounds (omit to capture the whole window).
    pub node: Option<u64>,
    pub settle: Option<SettleArg>,
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[tool_router]
impl AutomationServer {
    /// Build a server that marshals jobs to the given tree-thread channel.
    pub fn new(job_tx: UnboundedSender<Job>) -> Self {
        Self {
            job_tx,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Snapshot the accessibility tree (roles, labels, values, bounds, actions)."
    )]
    pub(crate) async fn snapshot_tree(
        &self,
        Parameters(p): Parameters<SnapshotParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::SnapshotTree {
                max_depth: p.max_depth,
            },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Read a single semantic node by its id.")]
    pub(crate) async fn read_node(
        &self,
        Parameters(p): Parameters<ReadParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::ReadNode { node: p.node },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(
        description = "Walk the full widget/layout tree (incl. widgets the accessibility tree prunes — layout primitives, dormant branches, presentational widgets) with each widget's type, bounds, flags, and tree position. Returns { roots, nodes } keyed by the same node ids as the AT tools."
    )]
    pub(crate) async fn layout_tree(
        &self,
        Parameters(p): Parameters<LayoutTreeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::LayoutTree {
                max_depth: p.max_depth,
                include_debug: p.include_debug.unwrap_or(false),
            },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(
        description = "Inspect one widget by node id: its type, bounds, flags (active/clips_children), tree position, and its Debug repr (constructor parameters). The inspector's Properties tab for a single node. Works for any widget, AT-visible or not."
    )]
    pub(crate) async fn inspect_node(
        &self,
        Parameters(p): Parameters<InspectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::InspectNode { node: p.node },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Find the first node matching a role and/or label.")]
    pub(crate) async fn find_node(
        &self,
        Parameters(p): Parameters<FindParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::FindNode {
                role: p.role,
                label: p.label,
            },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Assert a property of a node (role/label/value/toggled/expanded/...).")]
    pub(crate) async fn assert_node(
        &self,
        Parameters(p): Parameters<AssertParams>,
    ) -> Result<CallToolResult, McpError> {
        let assertion = build_assertion(&p)?;
        let mut result = self
            .run(
                p.window_id,
                AutomationOp::AssertNode {
                    node: p.node,
                    assertion,
                },
                SettleSpec::default(),
            )
            .await?;
        // A failed assertion (`{"passed": false}`) is a tool error: the toolkit
        // returns it as an Ok payload, but MCP clients rely on `is_error` to
        // detect failure, so surface it here.
        if result.is_error != Some(true) && assertion_failed(&result) {
            result.is_error = Some(true);
        }
        Ok(result)
    }

    #[tool(description = "List the app's managed windows with ids, labels, and titles.")]
    pub(crate) async fn list_windows(
        &self,
        Parameters(p): Parameters<WindowOnlyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::ListWindows,
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Invoke an AccessKit action on a node (click/focus/expand/...).")]
    pub(crate) async fn invoke_action(
        &self,
        Parameters(p): Parameters<InvokeParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::InvokeAction {
                node: p.node,
                action: p.action,
            },
            settle,
        )
        .await
    }

    #[tool(description = "Move focus to a node.")]
    pub(crate) async fn focus_node(
        &self,
        Parameters(p): Parameters<NodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::FocusNode { node: p.node },
            settle,
        )
        .await
    }

    #[tool(description = "Set a node's value via the SetValue AT action.")]
    pub(crate) async fn set_value(
        &self,
        Parameters(p): Parameters<SetValueParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::SetValue {
                node: p.node,
                value: p.value,
            },
            settle,
        )
        .await
    }

    #[tool(description = "Expand a disclosure / tree node.")]
    pub(crate) async fn expand(
        &self,
        Parameters(p): Parameters<NodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(p.window_id, AutomationOp::Expand { node: p.node }, settle)
            .await
    }

    #[tool(description = "Collapse a disclosure / tree node.")]
    pub(crate) async fn collapse(
        &self,
        Parameters(p): Parameters<NodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(p.window_id, AutomationOp::Collapse { node: p.node }, settle)
            .await
    }

    #[tool(
        description = "Scroll the widget under a node by a pixel delta, with optional modifiers \
                       (ctrl/shift/alt/meta) — a modifier-held wheel is its own gesture, e.g. \
                       Ctrl+wheel to zoom."
    )]
    pub(crate) async fn scroll(
        &self,
        Parameters(p): Parameters<ScrollParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::Scroll {
                node: p.node,
                dx: p.dx.unwrap_or(0.0),
                dy: p.dy.unwrap_or(0.0),
                ctrl: p.ctrl.unwrap_or(false),
                shift: p.shift.unwrap_or(false),
                alt: p.alt.unwrap_or(false),
                meta: p.meta.unwrap_or(false),
            },
            settle,
        )
        .await
    }

    #[tool(
        description = "Inject a pointer event at a point: action = click (default), double_click, down, up or move; button = primary (default), secondary, middle, back, forward; with optional ctrl/shift/alt/meta held for the press and release. Unknown names and unknown fields are refused rather than defaulted."
    )]
    pub(crate) async fn inject_pointer(
        &self,
        Parameters(p): Parameters<InjectPointerParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::InjectPointer {
                x: p.x,
                y: p.y,
                action: pointer_action(&p.action)?,
                button: pointer_button(&p.button)?,
                ctrl: p.ctrl.unwrap_or(false),
                shift: p.shift.unwrap_or(false),
                alt: p.alt.unwrap_or(false),
                meta: p.meta.unwrap_or(false),
            },
            settle,
        )
        .await
    }

    #[tool(
        description = "Right-click a node to open its context menu. Injects a secondary (right) \
                       button press+release at the node's centre — the node-based, coordinate-free \
                       way to trigger a widget's context menu. Prefer this over inject_pointer with \
                       button=secondary. After it settles, call get_overlays or snapshot_tree to \
                       read the opened menu (a Menu/MenuItem subtree), then invoke_action(item, \
                       \"click\") to pick an item."
    )]
    pub(crate) async fn right_click(
        &self,
        Parameters(p): Parameters<NodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::RightClick { node: p.node },
            settle,
        )
        .await
    }

    #[tool(description = "Inject a key press (with optional modifiers) to the focused widget.")]
    pub(crate) async fn inject_key(
        &self,
        Parameters(p): Parameters<InjectKeyParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::InjectKey {
                key: p.key,
                ctrl: p.ctrl.unwrap_or(false),
                shift: p.shift.unwrap_or(false),
                alt: p.alt.unwrap_or(false),
                meta: p.meta.unwrap_or(false),
            },
            settle,
        )
        .await
    }

    #[tool(description = "Focus a node and type text into it.")]
    pub(crate) async fn type_text(
        &self,
        Parameters(p): Parameters<TypeTextParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::TypeText {
                node: p.node,
                text: p.text,
            },
            settle,
        )
        .await
    }

    #[tool(description = "Drive IME composition / commit on a node.")]
    pub(crate) async fn type_ime(
        &self,
        Parameters(p): Parameters<TypeImeParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::TypeIme {
                node: p.node,
                preedit: p.preedit,
                commit: p.commit,
            },
            settle,
        )
        .await
    }

    #[tool(description = "Drag from a node to another node or a point.")]
    pub(crate) async fn drag_node(
        &self,
        Parameters(p): Parameters<DragParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::DragNode {
                node: p.node,
                to_node: p.to_node,
                to_x: p.to_x,
                to_y: p.to_y,
            },
            settle,
        )
        .await
    }

    #[tool(description = "List active overlays (popovers, menus, tooltips, dialogs).")]
    pub(crate) async fn get_overlays(
        &self,
        Parameters(p): Parameters<WindowOnlyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::GetOverlays,
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "List effective keyboard shortcuts and their bindings.")]
    pub(crate) async fn get_shortcuts(
        &self,
        Parameters(p): Parameters<WindowOnlyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::GetShortcuts,
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "List nodes that are live regions (polite/assertive).")]
    pub(crate) async fn list_live_regions(
        &self,
        Parameters(p): Parameters<WindowOnlyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::ListLiveRegions,
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Drain captured live-region announcements since a sequence number.")]
    pub(crate) async fn pull_announcements(
        &self,
        Parameters(p): Parameters<PullParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::PullAnnouncements {
                since_seq: p.since_seq.unwrap_or(0),
            },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Advance the simulation clock by N milliseconds.")]
    pub(crate) async fn advance_clock(
        &self,
        Parameters(p): Parameters<AdvanceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(
            p.window_id,
            AutomationOp::AdvanceClock { millis: p.millis },
            SettleSpec::default(),
        )
        .await
    }

    #[tool(description = "Run animations / layout to quiescence, then re-sync the tree.")]
    pub(crate) async fn settle(
        &self,
        Parameters(p): Parameters<SettleParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(p.window_id, AutomationOp::Settle, settle).await
    }

    #[tool(description = "Poll until a condition holds (node exists / value / gone / version).")]
    pub(crate) async fn wait_for_condition(
        &self,
        Parameters(p): Parameters<WaitParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        let condition = build_condition(&p)?;
        self.run(
            p.window_id,
            AutomationOp::WaitForCondition { condition },
            settle,
        )
        .await
    }

    #[tool(description = "Render the window (or a node's bounds) to a PNG image block.")]
    pub(crate) async fn screenshot(
        &self,
        Parameters(p): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let settle = settle_spec(&p.settle);
        self.run(
            p.window_id,
            AutomationOp::Screenshot { node: p.node },
            settle,
        )
        .await
    }
}

impl AutomationServer {
    /// Marshal one op to the tree thread and await the reply.
    pub(crate) async fn run(
        &self,
        window_id: Option<u64>,
        op: AutomationOp,
        settle: SettleSpec,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();
        let req = AutomationRequest {
            window_id,
            op,
            settle,
        };
        self.job_tx
            .send((req, tx))
            .map_err(|_| McpError::internal_error("automation host thread is gone", None))?;
        let reply = rx
            .await
            .map_err(|_| McpError::internal_error("automation host dropped the reply", None))?;
        Ok(to_result(reply))
    }

    /// The tool router, for the conformance test (the macro-generated
    /// `tool_router()` is module-private).
    #[cfg(test)]
    pub(crate) fn router_for_test() -> ToolRouter<Self> {
        Self::tool_router()
    }
}

/// The server's `instructions` — a short "how to drive this app" the MCP
/// client receives at `initialize`, so a fresh agent knows the canonical loop
/// and the argument vocabularies without trial and error.
const SERVER_INSTRUCTIONS: &str = "\
Drive and inspect a Teksilo desktop app through its accessibility tree — no OS \
accessibility layer needed.

Canonical loop:
1. Find a node id. `snapshot_tree {max_depth?}` returns the tree; each node has \
an `id` (an AccessKit id, stable for the widget's lifetime — across relayout / \
theme / locale, but a structural rebuild that recreates the widget yields a \
new id, so re-find after the tree's structure changes), plus role, label, \
value, toggled/expanded/selected, bounds, and the `actions` it supports. \
`find_node {role?, label?}` returns the first match's id directly.
2. Act on it. `invoke_action {node, action}` where action is one of click, \
focus, expand, collapse, set_value, increment, decrement, show_context_menu; \
or the shortcuts `set_value` / `type_text` / `focus_node` / `expand` / \
`collapse` / `scroll`; or raw input `inject_pointer {x,y,action?,button?}` / \
`inject_key {key, ctrl?,shift?,alt?,meta?}` / `type_ime` / `drag_node`.
3. Verify. Re-`snapshot_tree`, `read_node {node}`, or `assert_node {node, kind, \
value?/flag?}` where kind is role_equals, label_equals, label_contains, \
value_equals, toggled, expanded, selected, disabled, exists, or focused. A \
FAILED assert_node comes back as a tool error (isError=true).

Error results carry a stable `code` (in the text body and in structured_content) \
— branch on it, not just on isError: NOT_FOUND / BAD_ARGUMENT / UNKNOWN_NAME are \
real mistakes, while GPU_UNAVAILABLE (no GPU for a screenshot) and SETTLE_TIMEOUT \
(a poll/animation hit its budget) are benign/environmental.

Timing: mutating tools auto-settle (run animations + layout, then re-sync the \
tree). For timed UI (tooltips, debounced reactivity) pass `settle.clock_millis` \
to advance the simulated clock, call `advance_clock {millis}`, or poll with \
`wait_for_condition {kind, ...}` (node_exists / node_value / node_gone / \
at_version_at_least).

Layout debugging: `layout_tree {max_depth?, include_debug?}` walks the FULL \
widget tree — including widgets the accessibility tree prunes (layout \
primitives, dormant branches, presentational widgets) — with each widget's \
type, bounds, and tree position; `inspect_node {node}` gives one widget's full \
record incl. its Debug repr (parameters). Use these for overlap / clipping / \
off-screen / wrong-size questions the semantic tree can't answer. Layout nodes \
share ids with AT nodes, so the same widget correlates across both.

Other tools: `get_overlays` detects open menus/popovers/dialogs; \
`list_live_regions` + `pull_announcements {since_seq}` capture status/toast \
text a screen reader would speak; `get_shortcuts` lists key bindings; \
`list_windows` enumerates windows (pass `window_id` to target one); \
`screenshot {node?}` returns a PNG image block of the whole window or a node's \
bounds (an embedded WebView reads back as a transparent hole, and says so).";

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AutomationServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.instructions = Some(SERVER_INSTRUCTIONS.to_string());
        info
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

/// Convert a host reply to an MCP tool result: JSON text for replies (with
/// `is_error` set on toolkit errors), an image block for screenshots. The
/// payload is also mirrored into `structured_content` so a client can branch
/// on the result (e.g. tell a benign `GPU_UNAVAILABLE` / `SETTLE_TIMEOUT` from
/// a real `NOT_FOUND`) by reading the stable `code` field, without parsing the
/// text block.
pub fn to_result(reply: HostReply) -> CallToolResult {
    match reply {
        HostReply::Reply(AutomationReply::Ok { data }) => {
            let mut result = CallToolResult::success(vec![ContentBlock::text(data.to_string())]);
            result.structured_content = Some(data);
            result
        }
        HostReply::Reply(AutomationReply::Err { code, message }) => {
            let body = serde_json::json!({ "code": code, "message": message });
            let mut result = CallToolResult::error(vec![ContentBlock::text(body.to_string())]);
            result.structured_content = Some(body);
            result
        }
        HostReply::Image { png, warnings } => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            let mut content = vec![ContentBlock::image(b64, "image/png".to_string())];
            if !warnings.is_empty() {
                content.push(ContentBlock::text(
                    serde_json::json!({ "warnings": warnings }).to_string(),
                ));
            }
            CallToolResult::success(content)
        }
    }
}

/// Whether an `assert_node` result's text payload says `"passed": false`.
fn assertion_failed(result: &CallToolResult) -> bool {
    result.content.iter().any(|c| {
        c.as_text()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t.text).ok())
            .and_then(|v| v.get("passed").and_then(|p| p.as_bool()))
            .is_some_and(|passed| !passed)
    })
}

fn build_assertion(p: &AssertParams) -> Result<Assertion, McpError> {
    let value = || {
        p.value
            .clone()
            .ok_or_else(|| McpError::invalid_params("assertion needs 'value'", None))
    };
    let flag = || {
        p.flag
            .ok_or_else(|| McpError::invalid_params("assertion needs 'flag'", None))
    };
    Ok(match p.kind.as_str() {
        "exists" => Assertion::Exists,
        "focused" => Assertion::Focused,
        "role_equals" => Assertion::RoleEquals { value: value()? },
        "label_equals" => Assertion::LabelEquals { value: value()? },
        "label_contains" => Assertion::LabelContains { value: value()? },
        "value_equals" => Assertion::ValueEquals { value: value()? },
        "toggled" => Assertion::Toggled { value: flag()? },
        "expanded" => Assertion::Expanded { value: flag()? },
        "selected" => Assertion::Selected { value: flag()? },
        "disabled" => Assertion::Disabled { value: flag()? },
        other => {
            return Err(McpError::invalid_params(
                format!("unknown assertion kind '{other}'"),
                None,
            ));
        }
    })
}

fn build_condition(p: &WaitParams) -> Result<WaitCondition, McpError> {
    let node = || {
        p.node
            .ok_or_else(|| McpError::invalid_params("condition needs 'node'", None))
    };
    Ok(match p.kind.as_str() {
        "node_exists" => WaitCondition::NodeExists {
            role: p.role.clone(),
            label: p.label.clone(),
        },
        "node_value" => WaitCondition::NodeValue {
            node: node()?,
            expected: p
                .expected
                .clone()
                .ok_or_else(|| McpError::invalid_params("condition needs 'expected'", None))?,
        },
        "node_gone" => WaitCondition::NodeGone { node: node()? },
        "at_version_at_least" => WaitCondition::AtVersionAtLeast {
            version: p
                .version
                .ok_or_else(|| McpError::invalid_params("condition needs 'version'", None))?,
        },
        other => {
            return Err(McpError::invalid_params(
                format!("unknown condition kind '{other}'"),
                None,
            ));
        }
    })
}

/// A name a caller asked for, or an error naming what it could have said.
///
/// Falling back to a default here is the same failure the DTOs now refuse one
/// layer up: an unrecognised name is a caller who meant something, and quietly
/// doing the default instead performs an action nobody asked for while
/// reporting success. `"double_click"` used to arrive here and leave as a
/// single click.
fn pointer_action(s: &Option<String>) -> Result<PointerAction, McpError> {
    Ok(match s.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("click") => PointerAction::Click,
        Some("double_click") => PointerAction::DoubleClick,
        Some("down") => PointerAction::Down,
        Some("up") => PointerAction::Up,
        Some("move") => PointerAction::Move,
        Some(other) => {
            return Err(McpError::invalid_params(
                format!(
                    "unknown pointer action '{other}'                          (click, double_click, down, up, move)"
                ),
                None,
            ));
        }
    })
}

fn pointer_button(s: &Option<String>) -> Result<PointerButtonDto, McpError> {
    Ok(match s.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("primary") => PointerButtonDto::Primary,
        Some("secondary") => PointerButtonDto::Secondary,
        Some("middle") => PointerButtonDto::Middle,
        Some("back") => PointerButtonDto::Back,
        Some("forward") => PointerButtonDto::Forward,
        Some(other) => {
            return Err(McpError::invalid_params(
                format!(
                    "unknown pointer button '{other}'                          (primary, secondary, middle, back, forward)"
                ),
                None,
            ));
        }
    })
}
