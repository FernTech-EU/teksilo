// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Serde data-transfer objects — the entire automation wire protocol.
//!
//! Every value that crosses a thread boundary (the headless tree-thread
//! channel) or a socket (the live in-app bridge) is one of these types.
//! There are **no closures and no `!Send` handles on the wire** — the
//! `!Send` [`WidgetTree`](teksilo_core::WidgetTree) never leaves its owning
//! thread; only these `Send` DTOs are marshaled to it.

use serde::{Deserialize, Serialize};

/// Node identity exposed to an automation client: the raw `accesskit::NodeId`
/// value (which Teksilo derives deterministically from a `WidgetId`). It is
/// stable for the **lifetime of the widget instance** — surviving relayout,
/// repaint, theme, and locale changes (which mutate widgets in place) — but a
/// *structural rebuild* that destroys and recreates the widget (a data-model
/// change, a `Switcher` swap, a `Rebuild`-level binding) allocates a new
/// `WidgetId` and therefore a new id. So caching an id pays off across
/// in-place changes, but re-find (by role/label) after the tree's structure
/// may have changed. Synthetic widget-emitted children (e.g. rich-text runs)
/// have bit 63 set.
pub type NodeRef = u64;

/// One semantic node as the accessibility tree exposes it. Built straight
/// from a node in the freshly-synced `accesskit::TreeUpdate`, so it is a
/// faithful model of what a screen reader sees.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SemanticNode {
    /// The node's [`NodeRef`].
    pub id: NodeRef,
    /// AccessKit role, rendered as its `Debug` name (e.g. `"Button"`).
    pub role: String,
    /// The node's label / name, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The node's value, if any (e.g. a text-field's content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The node's accessible **description** — the supplementary sentence an
    /// assistive technology reads after the name.
    ///
    /// Carried because it is where a whole tier of the tooltip system lives: a
    /// plain tooltip is never auto-shown on focus, so its text reaches a screen
    /// reader only as the described control's description. Without this field a
    /// probe could see a control's name and role and had no way to ask whether
    /// its hint had reached it at all — which is exactly how a bug that put
    /// every plain tooltip's text on an unnamed box beside its control survived
    /// a live probe suite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Toggle state for checkboxes / toggles: `"true"`, `"false"`, or
    /// `"mixed"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggled: Option<String>,
    /// Expanded state for disclosure / tree rows, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    /// Selected state, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
    /// Hierarchy depth for a tree row, 1-based (AccessKit's `level`), when the
    /// node declares one. A client aiming a synthetic pointer at a row's
    /// disclosure chevron cannot compute its x without this, because the
    /// chevron sits one indent step per level in from the row's leading edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<usize>,
    /// Whether the node is disabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Whether this node currently holds focus (tree-level focus).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub focused: bool,
    /// Live-region politeness, when the node is a live region: `"polite"`
    /// or `"assertive"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<String>,
    /// Numeric value (sliders / spin boxes), when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
    /// Screen-projected bounds in logical pixels, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<NodeBounds>,
    /// AT actions the node advertises, as snake_case names (e.g.
    /// `"click"`, `"set_value"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    /// Child node refs, in AT order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NodeRef>,
}

/// A node's bounds in logical pixels.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct NodeBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One widget in the *layout/arena* tree (the inspector's view) — richer than
/// [`SemanticNode`] because it includes widgets the accessibility tree prunes
/// (layout primitives, dormant `Switcher` branches, presentational /
/// `access_exclude` widgets). Keyed by the **same** [`NodeRef`] space as the
/// AT tools (`widget_id_to_node_id`), so a layout node and an AT node for the
/// same widget share an `id`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LayoutNode {
    /// The widget's [`NodeRef`].
    pub id: NodeRef,
    /// The widget's concrete Rust type name (e.g.
    /// `"teksilo_widgets::button::Button"`).
    #[serde(rename = "type")]
    pub type_name: String,
    /// Layout-resolved bounds in logical window-relative pixels.
    pub bounds: NodeBounds,
    /// Whether the widget is currently active (vs dormant — e.g. a hidden
    /// `Switcher` branch, whose `bounds` are its last laid-out values).
    pub active: bool,
    /// Whether the widget clips its children (`ScrollArea`, `MaxSize`, …).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clips_children: bool,
    /// Parent widget ref, or `None` for a root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeRef>,
    /// Child widget refs, in tree order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NodeRef>,
    /// The widget's `Debug` repr — its constructor parameters / fields, the
    /// same "debug repr" the inspector's Properties tab shows. Present only
    /// when requested (`include_debug` / `inspect_node`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<String>,
}

/// One managed window, as reported by `list_windows`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WindowInfo {
    /// The `TeksiloWindowId` raw value, usable as `window_id` in any op.
    pub id: u64,
    /// The stable `string_id` label set via `WindowConfig::id(...)`, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The window's current title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether this window currently has OS focus.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub focused: bool,
}

/// A captured live-region announcement (the DTO mirror of
/// [`teksilo_core::Announcement`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AnnouncementDto {
    pub seq: u64,
    pub text: String,
    /// `"assertive"` or `"polite"`.
    pub politeness: String,
}

impl From<teksilo_core::Announcement> for AnnouncementDto {
    fn from(a: teksilo_core::Announcement) -> Self {
        Self {
            seq: a.seq,
            text: a.text,
            politeness: if a.assertive { "assertive" } else { "polite" }.to_string(),
        }
    }
}

/// One effective keyboard shortcut, as reported by `get_shortcuts`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ShortcutInfo {
    /// The shortcut id (intent name), e.g. `"app.save"`.
    pub id: String,
    /// Display name, if the shortcut declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Primary keystroke, formatted (e.g. `"Ctrl+S"`), if bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    /// Secondary keystroke, formatted, if bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
    /// Whether the shortcut is currently enabled.
    pub enabled: bool,
}

/// Which pointer phase an `inject_pointer` op synthesises.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PointerAction {
    /// Press + release at the point (a full click). The default.
    #[default]
    Click,
    /// Two press+release pairs at the point, with nothing in between, so the
    /// gesture recogniser reads them as one double-click.
    ///
    /// Not the same as sending `Click` twice from a client: the two arrive as
    /// separate ops with a network round trip and a settle between them, which
    /// is exactly the gap a double-click must not have.
    DoubleClick,
    /// A single pointer-down.
    Down,
    /// A single pointer-up.
    Up,
    /// A pointer-move to the point.
    Move,
}

/// Which mouse button an `inject_pointer` op uses.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PointerButtonDto {
    #[default]
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
}

impl PointerButtonDto {
    pub fn to_core(self) -> teksilo_core::PointerButton {
        use teksilo_core::PointerButton as B;
        match self {
            PointerButtonDto::Primary => B::Primary,
            PointerButtonDto::Secondary => B::Secondary,
            PointerButtonDto::Middle => B::Middle,
            PointerButtonDto::Back => B::Back,
            PointerButtonDto::Forward => B::Forward,
        }
    }
}

/// An assertion evaluated against a single node by `assert_node`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assertion {
    /// The node exists in the current tree.
    Exists,
    /// The node holds focus.
    Focused,
    /// The node's role `Debug` name equals the given string.
    RoleEquals { value: String },
    /// The node's label equals the given string.
    LabelEquals { value: String },
    /// The node's label contains the given substring.
    LabelContains { value: String },
    /// The node's value equals the given string.
    ValueEquals { value: String },
    /// The node's toggle state matches.
    Toggled { value: bool },
    /// The node's expanded state matches.
    Expanded { value: bool },
    /// The node's selected state matches.
    Selected { value: bool },
    /// The node's disabled state matches.
    Disabled { value: bool },
}

/// Result of an [`Assertion`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AssertionResult {
    pub passed: bool,
    /// On failure, a short human-readable reason (actual vs expected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AssertionResult {
    pub fn passed() -> Self {
        Self {
            passed: true,
            detail: None,
        }
    }

    pub fn failed(detail: impl Into<String>) -> Self {
        Self {
            passed: false,
            detail: Some(detail.into()),
        }
    }
}

/// A predicate `wait_for_condition` polls until satisfied or it times out.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaitCondition {
    /// A node matching the given role and/or label exists.
    NodeExists {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    /// The node's value equals `expected`.
    NodeValue { node: NodeRef, expected: String },
    /// The node is not present in the tree. NOTE: this is satisfied
    /// **immediately** for any id that is absent — including an id that was
    /// *never* present (a stale or garbage `NodeRef`). It is "not present", not
    /// "was present then removed", so capture the node's id from a prior
    /// snapshot/find before waiting for it to disappear.
    NodeGone { node: NodeRef },
    /// The tree's AT version is at least `version`.
    AtVersionAtLeast { version: u64 },
}

/// How a mutating op should settle the tree after dispatch, before it
/// re-syncs and returns.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SettleSpec {
    /// Advance the simulation clock by this many milliseconds first
    /// (drives tooltip / overlay timers). Default `0`.
    #[serde(default)]
    pub clock_millis: u64,
    /// Cap on the number of 16 ms animation ticks the settle loop runs.
    /// Default `60` (~1 s of animation). A perpetually-looping animation
    /// hits this cap (expected).
    #[serde(default = "default_max_anim_frames")]
    pub max_anim_frames: u32,
    /// Run a layout pass (at the tree's last proposal) after ticking, so
    /// height-for-width / reflow settles before the AT re-walk. Default
    /// `true`.
    #[serde(default = "default_true")]
    pub layout_after: bool,
    /// Hard wall-clock budget for the whole settle, in milliseconds.
    /// Default `500`. Exceeding it ends the settle early (the live bridge
    /// reports `SETTLE_TIMEOUT`).
    #[serde(default = "default_settle_timeout")]
    pub settle_timeout_ms: u64,
}

fn default_max_anim_frames() -> u32 {
    60
}
fn default_true() -> bool {
    true
}
fn default_settle_timeout() -> u64 {
    500
}

impl Default for SettleSpec {
    fn default() -> Self {
        Self {
            clock_millis: 0,
            max_anim_frames: default_max_anim_frames(),
            layout_after: true,
            settle_timeout_ms: default_settle_timeout(),
        }
    }
}

/// One automation operation — exactly one per MCP tool. Externally tagged
/// so the socket JSON is unambiguous; the MCP server constructs these
/// directly from each tool's typed parameters.
///
/// **Unknown fields are refused, here and on every tool's parameters.** An op
/// is a client's *instruction*, and serde's default is to ignore a field it
/// does not recognise and take the `#[serde(default)]` for the one that was
/// meant — so a misspelled argument does not fail, it silently performs a
/// different action. `{"x": .., "y": .., "kind": "move"}` against `InjectPointer`
/// took the default `action`, which is `Click`: a probe that asked to hover
/// clicked every control it pointed at, quietly toggling real settings, and
/// nothing anywhere said so. Replies are deliberately *not* strict, for the
/// opposite reason: a client reading a newer app's richer output should keep
/// working.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum AutomationOp {
    // ---- Query ----
    SnapshotTree {
        #[serde(default)]
        max_depth: Option<usize>,
    },
    ReadNode {
        node: NodeRef,
    },
    /// Walk the full widget/layout (arena) tree — includes widgets the AT tree
    /// prunes (layout primitives, dormant branches, presentational widgets).
    LayoutTree {
        #[serde(default)]
        max_depth: Option<usize>,
        /// Include each widget's `Debug` repr (its parameters). Off by default
        /// (it can be large).
        #[serde(default)]
        include_debug: bool,
    },
    /// One widget's full layout-tree record (type, bounds, flags, tree
    /// position, and its `Debug` repr) — the inspector's Properties tab for a
    /// single node. Works for any widget, AT-visible or not.
    InspectNode {
        node: NodeRef,
    },
    FindNode {
        #[serde(default)]
        role: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    AssertNode {
        node: NodeRef,
        assertion: Assertion,
    },
    ListWindows,
    // ---- AT-action driving ----
    InvokeAction {
        node: NodeRef,
        action: String,
    },
    FocusNode {
        node: NodeRef,
    },
    SetValue {
        node: NodeRef,
        value: String,
    },
    Expand {
        node: NodeRef,
    },
    Collapse {
        node: NodeRef,
    },
    Scroll {
        node: NodeRef,
        #[serde(default)]
        dx: f32,
        #[serde(default)]
        dy: f32,
        // Modifiers held during the wheel, mirroring `InjectKey`'s. They default
        // to none, so every existing caller keeps the plain-wheel behaviour.
        //
        // A modifier is not decoration on a scroll: `WidgetEvent::Scroll` carries
        // them precisely so an app can implement Ctrl-wheel-to-zoom, and until
        // this existed no probe could reach that gesture at all — the one input
        // the bridge could describe but not perform.
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        meta: bool,
    },
    // ---- Synthetic input ----
    InjectPointer {
        x: f32,
        y: f32,
        #[serde(default)]
        action: PointerAction,
        #[serde(default)]
        button: PointerButtonDto,
        // Modifiers held for the press and the release, mirroring `Scroll`'s
        // and `InjectKey`'s. A modifier is not decoration on a click: Ctrl-click
        // to extend a selection is its own gesture, and until this existed no
        // probe could perform it -- the corkboard's multi-select check passed an
        // undeclared `modifiers` field, which serde dropped, so it asserted
        // against a plain click while believing otherwise.
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        meta: bool,
    },
    /// Right-click a node: a synthetic Secondary press+release at the node's
    /// point, which drives the framework's context-menu machinery
    /// (`.context_menu(..)` factory). The node-based, coordinate-free way to
    /// open a context menu — see the `right_click` tool.
    RightClick {
        node: NodeRef,
    },
    InjectKey {
        key: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        meta: bool,
    },
    TypeText {
        node: NodeRef,
        text: String,
    },
    TypeIme {
        node: NodeRef,
        #[serde(default)]
        preedit: Option<String>,
        #[serde(default)]
        commit: Option<String>,
    },
    DragNode {
        node: NodeRef,
        #[serde(default)]
        to_node: Option<NodeRef>,
        #[serde(default)]
        to_x: Option<f32>,
        #[serde(default)]
        to_y: Option<f32>,
    },
    // ---- Introspection ----
    GetOverlays,
    GetShortcuts,
    ListLiveRegions,
    PullAnnouncements {
        #[serde(default)]
        since_seq: u64,
    },
    // ---- Time / settle ----
    AdvanceClock {
        millis: u64,
    },
    Settle,
    WaitForCondition {
        condition: WaitCondition,
    },
    // ---- Visual (host-handled, see `execute`) ----
    Screenshot {
        #[serde(default)]
        node: Option<NodeRef>,
    },
}

/// The reply to one [`AutomationOp`]. `Ok` carries an arbitrary JSON
/// payload (per op); `Err` carries a stable code plus a message.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AutomationReply {
    Ok {
        #[serde(default)]
        data: serde_json::Value,
    },
    Err {
        code: String,
        message: String,
    },
}

impl AutomationReply {
    /// An `Ok` reply carrying `data`.
    pub fn ok(data: serde_json::Value) -> Self {
        AutomationReply::Ok { data }
    }
    /// An `Ok` reply with no payload (`null`).
    pub fn ok_unit() -> Self {
        AutomationReply::Ok {
            data: serde_json::Value::Null,
        }
    }
    /// An `Ok` reply built by serializing `value` (falls back to an
    /// `Err{SERIALIZE_FAILED}` if serialization fails, which never happens
    /// for the toolkit's own DTOs).
    pub fn ok_json<T: Serialize>(value: &T) -> Self {
        match serde_json::to_value(value) {
            Ok(data) => AutomationReply::Ok { data },
            Err(e) => AutomationReply::err("SERIALIZE_FAILED", e.to_string()),
        }
    }
    /// An `Err` reply.
    pub fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        AutomationReply::Err {
            code: code.into(),
            message: message.into(),
        }
    }
    /// Whether this is an `Ok` reply.
    pub fn is_ok(&self) -> bool {
        matches!(self, AutomationReply::Ok { .. })
    }
}

/// The envelope marshaled over the live in-app bridge socket: which window
/// to route to, the op, and how to settle.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct AutomationRequest {
    /// Target window (`TeksiloWindowId` raw). `None` → the focused window,
    /// else the primary. Ignored in headless mode (single tree).
    #[serde(default)]
    pub window_id: Option<u64>,
    /// The operation to perform.
    pub op: AutomationOp,
    /// Settle policy for mutating ops.
    #[serde(default)]
    pub settle: SettleSpec,
}

/// Stable error codes used across the toolkit and both transports.
pub mod codes {
    /// No node / window matched the request.
    pub const NOT_FOUND: &str = "NOT_FOUND";
    /// A required argument was missing or malformed.
    pub const BAD_ARGUMENT: &str = "BAD_ARGUMENT";
    /// The action / role / key name was not recognised.
    pub const UNKNOWN_NAME: &str = "UNKNOWN_NAME";
    /// The node exists but nothing acted on the action — it advertises no such
    /// action, or its handler declined. Distinct from `NOT_FOUND` (no node at
    /// all): the target was real, the UI just did not move.
    pub const UNHANDLED_ACTION: &str = "UNHANDLED_ACTION";
    /// A `wait_for_condition` timed out.
    pub const WAIT_TIMEOUT: &str = "WAIT_TIMEOUT";
    /// A live settle exceeded its wall-clock budget.
    pub const SETTLE_TIMEOUT: &str = "SETTLE_TIMEOUT";
    /// No GPU backend was available for a screenshot.
    pub const GPU_UNAVAILABLE: &str = "GPU_UNAVAILABLE";
    /// `execute` cannot produce pixels / window lists — the host must.
    pub const HOST_REQUIRED: &str = "HOST_REQUIRED";
    /// An `assert_node` assertion evaluated to false against a node that does
    /// exist. Deliberately distinct from [`NOT_FOUND`]: "the button is not
    /// focused" and "there is no such button" are different bugs, and a caller
    /// that cannot tell them apart chases the wrong one.
    pub const ASSERTION_FAILED: &str = "ASSERTION_FAILED";
}
