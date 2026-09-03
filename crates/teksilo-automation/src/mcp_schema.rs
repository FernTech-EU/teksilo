// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The canonical catalog of automation tools — one entry per MCP tool.
//!
//! This is the single source of truth for *which* tools exist, their
//! one-line descriptions, and whether they mutate the UI (and so accept a
//! `SettleSpec`). The MCP server binary registers a handler per entry; its
//! conformance test cross-checks that the registered set matches this
//! catalog exactly. Keeping the catalog in the GUI-free toolkit (which has
//! no `rmcp` dependency) means the tool surface is documented and testable
//! without pulling in the async stack.

/// One automation tool's metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    /// The MCP tool name (snake_case).
    pub name: &'static str,
    /// One-line human description.
    pub description: &'static str,
    /// Whether the tool mutates UI state and therefore accepts an optional
    /// `SettleSpec`.
    pub mutating: bool,
}

/// Every automation tool, in a stable order.
pub const TOOL_CATALOG: &[ToolDescriptor] = &[
    // ---- Query ----
    ToolDescriptor {
        name: "snapshot_tree",
        description: "Snapshot the accessibility tree (roles, labels, values, bounds, actions).",
        mutating: false,
    },
    ToolDescriptor {
        name: "read_node",
        description: "Read a single semantic node by its id.",
        mutating: false,
    },
    ToolDescriptor {
        name: "layout_tree",
        description: "Walk the full widget/layout tree (incl. widgets the AT tree prunes) with bounds + types.",
        mutating: false,
    },
    ToolDescriptor {
        name: "inspect_node",
        description: "One widget's full layout record: type, bounds, flags, tree position, Debug repr.",
        mutating: false,
    },
    ToolDescriptor {
        name: "find_node",
        description: "Find the first node matching a role and/or label.",
        mutating: false,
    },
    ToolDescriptor {
        name: "assert_node",
        description: "Assert a property of a node (role/label/value/toggled/expanded/…).",
        mutating: false,
    },
    ToolDescriptor {
        name: "list_windows",
        description: "List the app's managed windows with ids, labels, and titles.",
        mutating: false,
    },
    // ---- AT-action driving ----
    ToolDescriptor {
        name: "invoke_action",
        description: "Invoke an AccessKit action on a node (click/focus/expand/…).",
        mutating: true,
    },
    ToolDescriptor {
        name: "focus_node",
        description: "Move focus to a node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "set_value",
        description: "Set a node's value via the SetValue AT action.",
        mutating: true,
    },
    ToolDescriptor {
        name: "expand",
        description: "Expand a disclosure / tree node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "collapse",
        description: "Collapse a disclosure / tree node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "scroll",
        description: "Scroll the widget under a node by a pixel delta, with optional modifiers (ctrl/shift/alt/meta/command). A modifier-held wheel is its own gesture, e.g. Ctrl+wheel to zoom. Use `command` for the platform accelerator (Control on Windows/Linux, Command on macOS); `ctrl` is literal Control.",
        mutating: true,
    },
    // ---- Synthetic input ----
    ToolDescriptor {
        name: "inject_pointer",
        description: "Inject a pointer event at a point: action = click (default), double_click, down, up or move; button = primary (default), secondary, middle, back, forward; with optional ctrl/shift/alt/meta/command held for the press and release. Use `command` for the platform accelerator (Control on Windows/Linux, Command on macOS) — accelerator-click to extend a selection is `command`, not `ctrl`. Unknown names and unknown fields are refused rather than defaulted.",
        mutating: true,
    },
    ToolDescriptor {
        name: "right_click",
        description: "Right-click a node (secondary button at its point) to open its context menu.",
        mutating: true,
    },
    ToolDescriptor {
        name: "inject_key",
        description: "Inject a key press (with optional modifiers) to the focused widget. Use `command` for any accelerator chord (Control on Windows/Linux, Command on macOS) — a shortcut declared Ctrl+S resolves to the Command chord on macOS, so `ctrl` there injects a key that matches no binding and still reports success. `ctrl` stays literal Control, for chords that really are Control everywhere (Ctrl+Tab).",
        mutating: true,
    },
    ToolDescriptor {
        name: "type_text",
        description: "Focus a node and type text into it.",
        mutating: true,
    },
    ToolDescriptor {
        name: "type_ime",
        description: "Drive IME composition / commit on a node.",
        mutating: true,
    },
    ToolDescriptor {
        name: "drag_node",
        description: "Drag from a node to another node or a point.",
        mutating: true,
    },
    // ---- Introspection ----
    ToolDescriptor {
        name: "get_overlays",
        description: "List active overlays (popovers, menus, tooltips, dialogs).",
        mutating: false,
    },
    ToolDescriptor {
        name: "get_shortcuts",
        description: "List effective keyboard shortcuts and their bindings.",
        mutating: false,
    },
    ToolDescriptor {
        name: "list_live_regions",
        description: "List nodes that are live regions (polite/assertive).",
        mutating: false,
    },
    ToolDescriptor {
        name: "pull_announcements",
        description: "Drain captured live-region announcements since a sequence number.",
        mutating: false,
    },
    // ---- Time / settle ----
    ToolDescriptor {
        name: "advance_clock",
        description: "Advance the simulation clock by N milliseconds.",
        mutating: true,
    },
    ToolDescriptor {
        name: "settle",
        description: "Run animations / layout to quiescence, then re-sync the tree.",
        mutating: true,
    },
    ToolDescriptor {
        name: "wait_for_condition",
        description: "Poll until a condition holds (node exists / value / gone / version).",
        mutating: true,
    },
    // ---- Visual ----
    ToolDescriptor {
        name: "screenshot",
        description: "Render the window (or a node's bounds) to a PNG image block.",
        mutating: false,
    },
];

/// The number of tools in the catalog (27).
pub const TOOL_COUNT: usize = TOOL_CATALOG.len();
