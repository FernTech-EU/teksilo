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
        description: "Inject a pointer event at a point: action = click (default), double_click, down, up or move; button = primary (default), secondary, middle, back, forward; kind = mouse (default), touch or pen; with optional ctrl/shift/alt/meta/command held for the press and release. A touch or pen enters through the tree's pointer door, so the kind reaches the hit test, the slop, the hover rules and the arbitration; pen carries `pressure` (0..1) and `tilt` ([x, y] degrees). Continue a contact a previous call left down with `pointer_id` from query_pointers (on a move or an up; a down and a click mint their own), or drive a whole gesture with inject_touch_sequence. Use `command` for the platform accelerator (Control on Windows/Linux, Command on macOS) — accelerator-click to extend a selection is `command`, not `ctrl`. Unknown names and unknown fields are refused rather than defaulted, and so are pressure/tilt/pointer_id on a mouse.",
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
    ToolDescriptor {
        name: "inject_touch_sequence",
        description: "Drive a whole multi-touch gesture in one call — a list of steps, each naming a finger slot, a phase (down/move/up/cancel), a point, and how many simulated milliseconds to advance first — and report the arbitration after every step: the frozen touch_action, every competitor with its role and state, and the winner. Fingers are named by slot, not by id: identities are minted by the framework and the reply says which one each slot got. A sequence that stops short of its `up` leaves the finger down, which is how a live arbitration stays observable.",
        mutating: true,
    },
    ToolDescriptor {
        name: "pinch",
        description: "Two fingers moving from one span to another — the pinch a zoomable surface reads. Both land before either moves, because the recogniser's reference span is the distance between the landings.",
        mutating: true,
    },
    ToolDescriptor {
        name: "fling",
        description: "One finger travelling from one point to another over N simulated milliseconds and released while still moving — the shape a kinetic coast is handed off from. A drag latches on distance; a fling hands a velocity to the scroller, so the duration is the whole difference.",
        mutating: true,
    },
    ToolDescriptor {
        name: "long_press",
        description: "Press at a point, hold for exactly the device's long-press threshold, release. The hold is read off the active input profile for the pointer kind, so the call means 'hold long enough' without the script knowing the number.",
        mutating: true,
    },
    ToolDescriptor {
        name: "cancel_pointer",
        description: "Revoke a live pointer the way the system does (a compositor grab, a lost capture). Not an up: no tap completes and every widget working on the pointer is told. Takes an id from query_pointers.",
        mutating: true,
    },
    ToolDescriptor {
        name: "query_pointers",
        description: "Every live pointer with its id, kind, position, pressure/tilt, capture, frozen touch_action, competitors and arbitration winner. How to learn the id of a contact left down by anything but inject_touch_sequence, whose reply names its own.",
        mutating: false,
    },
    ToolDescriptor {
        name: "set_density",
        description: "Switch the app's target density (compact / comfortable / touch). WARNING: a density change rebuilds every widget, so every node id captured before it is dead — re-snapshot or re-find afterwards. Setting the density it already has is a no-op and keeps the ids.",
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

/// The number of tools in the catalog (34).
pub const TOOL_COUNT: usize = TOOL_CATALOG.len();
