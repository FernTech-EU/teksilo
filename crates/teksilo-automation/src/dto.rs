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
    ///
    /// `Role::TextRun` children are **omitted**: every visible label now
    /// carries one run per visual line, and an assistive technology never
    /// navigates to them — `accesskit_consumer`'s own filter excludes them
    /// from object navigation. Listing them here would bury every snapshot
    /// under nodes no reader can reach. They remain in the update and stay
    /// addressable by `read_node` / `assert_node` / `invoke_action`; what a
    /// probe usually wants about them is in [`text`](Self::text).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NodeRef>,
    /// What a screen reader could review on this node, when it carries text
    /// ranges at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextRangeInfo>,
    /// The node's own `label` property, before `labelled_by` resolution.
    ///
    /// Present only when it differs from [`label`](Self::label) — i.e. when
    /// the node is named by pointing at another node rather than by a copy
    /// of its string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_label: Option<String>,
}

/// What a screen reader could review on a node that carries text ranges.
///
/// A probe that wants to know whether a label is reviewable should read
/// this rather than counting `Role::TextRun` children, which the snapshot
/// deliberately omits.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TextRangeInfo {
    /// How many text runs the node carries. At least one on any node that
    /// supports ranges — an empty label still emits one, so a change event
    /// has an old node that supports ranges to diff against.
    pub run_count: usize,
    /// The text a reader would review, as the consumer assembles it from
    /// the runs. Must equal the node's announced value; when it does not,
    /// what a reader hears and what it reviews are different strings.
    pub document_text: String,
    /// Whether the range reports bounding boxes — `false` means a magnifier
    /// cannot follow the review cursor and braille cannot be routed.
    pub has_geometry: bool,
    /// The node's base reading direction, when it declares one:
    /// `"left_to_right"` or `"right_to_left"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
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

/// Which device an injected pointer op is pretending to be.
///
/// The three kinds are not interchangeable descriptions of the same event: a
/// mouse is indirect and precise, a finger is direct and coarse, a pen is
/// direct and precise, and the framework tunes slop, hit outset, hover and the
/// whole cross-widget arbitration off exactly that classification. An op that
/// says nothing is a [`Mouse`](Self::Mouse), which is what every automation op
/// was before touch existed.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PointerKindDto {
    /// An indirect precise pointer with a hover state. The default.
    #[default]
    Mouse,
    /// A direct coarse pointer with no hover state — a finger.
    Touch,
    /// A direct precise pointer — a stylus tip.
    Pen,
}

impl PointerKindDto {
    /// The core kind this names. `Pen` maps to the generic
    /// [`PenKind::Pen`](teksilo_tokens::PenKind::Pen) tip; the eraser and the
    /// other tool ids are not addressable from the wire, because nothing in the
    /// framework branches on them yet and a field no reader consults is a
    /// promise this crate cannot keep.
    pub fn to_core(self) -> teksilo_tokens::PointerKind {
        match self {
            PointerKindDto::Mouse => teksilo_tokens::PointerKind::Mouse,
            PointerKindDto::Touch => teksilo_tokens::PointerKind::Touch,
            PointerKindDto::Pen => teksilo_tokens::PointerKind::Pen(teksilo_tokens::PenKind::Pen),
        }
    }

    /// The wire name for a core kind, so a reply and a request speak the same
    /// vocabulary. An [`Unknown`](teksilo_tokens::PointerKind::Unknown) device
    /// reports as `mouse`, which is how the framework tunes it.
    pub fn from_core(kind: teksilo_tokens::PointerKind) -> Self {
        match kind {
            teksilo_tokens::PointerKind::Touch => PointerKindDto::Touch,
            teksilo_tokens::PointerKind::Pen(_) => PointerKindDto::Pen,
            _ => PointerKindDto::Mouse,
        }
    }
}

/// Which [`TargetDensity`](teksilo_tokens::TargetDensity) the app lays out at.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DensityDto {
    /// Desktop mouse-and-keyboard density. The default.
    #[default]
    Compact,
    /// The intermediate ladder for hybrid devices and large-cursor users.
    Comfortable,
    /// Finger-first density.
    Touch,
}

impl DensityDto {
    pub fn to_core(self) -> teksilo_tokens::TargetDensity {
        match self {
            DensityDto::Compact => teksilo_tokens::TargetDensity::Compact,
            DensityDto::Comfortable => teksilo_tokens::TargetDensity::Comfortable,
            DensityDto::Touch => teksilo_tokens::TargetDensity::Touch,
        }
    }

    pub fn from_core(density: teksilo_tokens::TargetDensity) -> Self {
        match density {
            teksilo_tokens::TargetDensity::Compact => DensityDto::Compact,
            teksilo_tokens::TargetDensity::Comfortable => DensityDto::Comfortable,
            teksilo_tokens::TargetDensity::Touch => DensityDto::Touch,
        }
    }
}

/// One phase of a contact in an [`AutomationOp::InjectTouchSequence`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TouchPhaseDto {
    /// The finger lands. Mints the slot's contact identity.
    Down,
    /// The finger moves, still down.
    Move,
    /// The finger lifts. A tap completes here.
    Up,
    /// The system revokes the contact (a `wl_touch.cancel`, a compositor grab).
    /// The position carries no meaning and no tap completes.
    Cancel,
}

/// One step of an [`AutomationOp::InjectTouchSequence`].
///
/// `advance_ms` is applied **before** the sample, on the simulated clock, so
/// the interval between two steps is exactly what the script wrote and not how
/// long the host took to run two lines of code. That is what makes a hold, a
/// flick and a drag distinguishable at all: a drag is decided by distance and a
/// flick by distance over time, and two samples stamped microseconds apart
/// describe a flick at some thousands of dp per second on one machine and
/// something else on the next.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TouchStep {
    /// Which finger. A small slot number, not a pointer id: identities are
    /// minted by the framework's allocator and the op reports back which one
    /// each slot got. Defaults to `0`, the single-finger case.
    #[serde(default)]
    pub contact: u32,
    /// What that finger does.
    pub phase: TouchPhaseDto,
    /// Where, in window-logical coordinates.
    pub x: f32,
    pub y: f32,
    /// Simulated milliseconds to advance **before** this sample. Default `0`.
    #[serde(default)]
    pub advance_ms: u64,
}

/// One competitor in a pointer's cross-widget arbitration, as
/// [`AutomationOp::QueryPointers`] and [`AutomationOp::InjectTouchSequence`]
/// report it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SequenceMemberDto {
    /// The competing node.
    pub node: NodeRef,
    /// What it competes as: `gesture`, `pan`, `raw_drag` or `raw_preview`.
    pub role: String,
    /// Where it stands: `possible`, `held`, `rejected` or `won`.
    pub state: String,
}

/// What one pointer is doing right now.
///
/// The reply shape of [`AutomationOp::QueryPointers`], and the per-step
/// observation [`AutomationOp::InjectTouchSequence`] returns. It carries the
/// whole *observable* arbitration for that pointer — the frozen touch action,
/// every competitor with its role and state, and the winner if one has been
/// decided — because without them a scripted gesture can only assert that the
/// op returned, which is green whatever the arbitration did.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PointerReport {
    /// The pointer's identity, as a number a later op can hand back
    /// (`cancel_pointer`, `inject_pointer { pointer_id }`).
    pub pointer_id: u64,
    /// Which device it is.
    pub kind: PointerKindDto,
    /// The W3C `isPrimary` flag — per kind, so two live pointers of different
    /// kinds can both carry it.
    pub primary: bool,
    /// Whether any button is held (for a contact: whether it is down).
    pub down: bool,
    /// Where it last was, in window-logical coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 2]>,
    /// Reported tip pressure, normalised `0.0..=1.0`, where the device gives
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f32>,
    /// Reported tilt `[tilt_x, tilt_y]` in degrees, where the device gives one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<[f32; 2]>,
    /// The widget holding this pointer's capture, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_by: Option<NodeRef>,
    /// The `TouchAction` frozen at this pointer's press, under the name it is
    /// declared by: `AUTO`, `NONE`, `PAN`, `PAN_X`, `PAN_Y`, `PINCH_ZOOM`,
    /// `MANIPULATION`, or `(composite)` for a combination none of those names.
    pub touch_action: String,
    /// Every competitor for this pointer's press, innermost first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence_members: Vec<SequenceMemberDto>,
    /// The winner of the arbitration, once one has been decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_winner: Option<NodeRef>,
}

/// What one step of an [`AutomationOp::InjectTouchSequence`] left behind.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TouchStepReport {
    /// The slot this step drove.
    pub contact: u32,
    /// The identity that slot held for this step.
    pub pointer_id: u64,
    /// That pointer's whole state after the sample. `None` once the contact has
    /// left the table — after its `Up` or `Cancel` a finger is simply gone, and
    /// reporting a stale copy would let a script assert a winner for a pointer
    /// that no longer exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<PointerReport>,
}

/// The reply to an [`AutomationOp::InjectTouchSequence`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TouchSequenceReport {
    /// One entry per step, in the order the steps were given.
    pub steps: Vec<TouchStepReport>,
    /// Every pointer still live when the sequence ended — the fingers a
    /// sequence that stops short of its `Up` deliberately leaves down.
    pub live: Vec<PointerReport>,
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
    /// The node can be reviewed by character, word and line.
    ///
    /// This is what a screen reader is gated on
    /// (`accesskit_consumer::Node::supports_text_ranges`), and it is not
    /// implied by the node having a name: a label with no text runs is
    /// reachable but unreviewable, and unroutable on a braille display.
    SupportsTextRanges,
    /// The text a reader would review equals the given string.
    DocumentTextEquals { value: String },
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
    /// Budget for the whole settle, in milliseconds. Default `500`.
    ///
    /// For a settle this is a hard **wall-clock** cap: exceeding it ends the
    /// settle early (the live bridge reports `SETTLE_TIMEOUT`), which is what
    /// keeps a long animation from freezing a real app's UI thread.
    ///
    /// For `wait_for_condition` it is instead a **simulated-time** budget, spent
    /// in 16 ms frames, so the same wait resolves identically on every platform
    /// — a wall-clock bound there would make the result depend on the host's
    /// timer granularity (see `wait_for_condition`).
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
/// The default number of intermediate moves each finger of a
/// [`AutomationOp::Pinch`] makes.
fn default_pinch_steps() -> usize {
    8
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
/// different action. A misspelling of `action` against `InjectPointer` took
/// that field's default, which is `Click`: a probe that asked to hover clicked
/// every control it pointed at, quietly toggling real settings, and nothing
/// anywhere said so. Replies are deliberately *not* strict, for the opposite
/// reason: a client reading a newer app's richer output should keep working.
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
        /// Hold the platform's **primary accelerator** — Control on Windows and
        /// Linux, Command (⌘) on macOS.
        ///
        /// Use this, not `ctrl`, whenever the chord means "the accelerator":
        /// save, copy, select-all, accelerator-click to extend a selection. A
        /// Teksilo shortcut *declared* as `Ctrl+S` resolves to ⌘S on macOS, so
        /// `ctrl: true` fires nothing there — and reports success, because the
        /// key really was injected, it just matched no binding. `command: true`
        /// is the same script on all three platforms.
        ///
        /// `ctrl` stays literal Control everywhere, for the chords that really
        /// are Control on macOS too (Ctrl+Tab, the terminal's Ctrl+C).
        #[serde(default)]
        command: bool,
    },
    // ---- Synthetic input ----
    InjectPointer {
        x: f32,
        y: f32,
        #[serde(default)]
        action: PointerAction,
        #[serde(default)]
        button: PointerButtonDto,
        /// Which device is pointing. `mouse` (the default) is the pre-touch
        /// path, byte for byte: a legacy `PointerDown`/`PointerUp` pair whose
        /// pointer is the singular mouse. `touch` and `pen` build a real
        /// [`PointerSample`](teksilo_core::PointerSample) and enter through the
        /// tree's pointer door, so the kind reaches the hit test, the slop, the
        /// hover rules and the arbitration.
        #[serde(default)]
        kind: PointerKindDto,
        /// Which live contact to continue, from a `query_pointers` reply.
        ///
        /// It belongs on a `move` or an `up` — the phases that continue a
        /// contact already on the glass. A `down` mints an identity and a
        /// `click` is a whole contact's life, so both refuse it rather than let
        /// a caller name a finger the op is about to replace. With no id, a
        /// `move` or `up` addresses the sole live pointer of that kind and is
        /// refused when there is more than one: a script driving two fingers
        /// must say which. A mouse refuses it outright — it has one identity.
        #[serde(default)]
        pointer_id: Option<u64>,
        /// Tip pressure, normalised `0.0..=1.0`, as a digitizer reports it.
        /// Read by anything that consults
        /// [`PointerInfo::effective_pressure`](teksilo_core::PointerInfo::effective_pressure).
        #[serde(default)]
        pressure: Option<f32>,
        /// Tilt `[tilt_x, tilt_y]` in degrees, as a digitizer reports it.
        #[serde(default)]
        tilt: Option<[f32; 2]>,
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
        /// Hold the platform's **primary accelerator** — Control on Windows and
        /// Linux, Command (⌘) on macOS.
        ///
        /// Use this, not `ctrl`, whenever the chord means "the accelerator":
        /// save, copy, select-all, accelerator-click to extend a selection. A
        /// Teksilo shortcut *declared* as `Ctrl+S` resolves to ⌘S on macOS, so
        /// `ctrl: true` fires nothing there — and reports success, because the
        /// key really was injected, it just matched no binding. `command: true`
        /// is the same script on all three platforms.
        ///
        /// `ctrl` stays literal Control everywhere, for the chords that really
        /// are Control on macOS too (Ctrl+Tab, the terminal's Ctrl+C).
        #[serde(default)]
        command: bool,
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
        /// Hold the platform's **primary accelerator** — Control on Windows and
        /// Linux, Command (⌘) on macOS.
        ///
        /// Use this, not `ctrl`, whenever the chord means "the accelerator":
        /// save, copy, select-all, accelerator-click to extend a selection. A
        /// Teksilo shortcut *declared* as `Ctrl+S` resolves to ⌘S on macOS, so
        /// `ctrl: true` fires nothing there — and reports success, because the
        /// key really was injected, it just matched no binding. `command: true`
        /// is the same script on all three platforms.
        ///
        /// `ctrl` stays literal Control everywhere, for the chords that really
        /// are Control on macOS too (Ctrl+Tab, the terminal's Ctrl+C).
        #[serde(default)]
        command: bool,
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
    /// A whole multi-touch gesture in one op, and the arbitration it produced
    /// after every step.
    ///
    /// **Self-contained by construction.** Contact identities are minted by the
    /// framework's allocator, not chosen by the client, and `execute` holds no
    /// state between ops — so a gesture split across ops would have no way to
    /// name the same finger twice. Naming fingers by *slot* inside one op does,
    /// and the reply says which identity each slot got, which is what a later
    /// `cancel_pointer` or `inject_pointer { pointer_id }` needs.
    ///
    /// A sequence that stops short of its `Up` leaves the finger down, which is
    /// the point: an arbitration is only observable while the press is live.
    InjectTouchSequence {
        steps: Vec<TouchStep>,
    },
    /// Two fingers moving from one span to another — the pinch a zoomable
    /// surface reads.
    ///
    /// Both contacts land before either moves, because the recognizer's
    /// reference span is the distance between the two landings.
    Pinch {
        /// The first finger's start.
        ax0: f32,
        ay0: f32,
        /// The second finger's start.
        bx0: f32,
        by0: f32,
        /// The first finger's end.
        ax1: f32,
        ay1: f32,
        /// The second finger's end.
        bx1: f32,
        by1: f32,
        /// How many intermediate moves each finger makes. Default `8`; clamped
        /// to at least 1.
        #[serde(default = "default_pinch_steps")]
        steps: usize,
    },
    /// One finger travelling `from` → `to` over `over_ms` of **simulated**
    /// time, released while still moving — the shape a kinetic coast is handed
    /// off from.
    ///
    /// The distinction from a drag is the clock, not the path: a drag latches
    /// on distance, a fling hands a velocity to the scroller. Sampled at one
    /// 60 Hz frame per step so the velocity tracker sees gaps under its stop
    /// threshold and at least its minimum sample count; a flick described by
    /// two far-apart samples yields no velocity and silently never flings.
    Fling {
        from_x: f32,
        from_y: f32,
        to_x: f32,
        to_y: f32,
        /// The flick's duration in simulated milliseconds.
        over_ms: u64,
    },
    /// Press at a point, hold for exactly the device's long-press threshold,
    /// release.
    ///
    /// The hold comes from the active input profile for `kind` rather than a
    /// number the client picks, so the op means "hold long enough" on every
    /// density and every device without the script knowing the threshold.
    LongPress {
        x: f32,
        y: f32,
        /// Which device holds. Default `mouse`.
        #[serde(default)]
        kind: PointerKindDto,
    },
    /// Revoke a live pointer the way the system does — a `wl_touch.cancel`, a
    /// compositor grab, a `PointerCaptureLost`.
    ///
    /// Not an `up`: no tap completes, the end position carries no meaning, and
    /// every widget working on the pointer is told through
    /// [`CancelReason`](teksilo_core::CancelReason). The id comes from a
    /// `query_pointers` reply.
    CancelPointer {
        pointer_id: u64,
    },
    /// Every live pointer, with its identity, kind, position, axes, capture and
    /// whole arbitration state.
    ///
    /// Read-only. It is how a script learns the id of a contact left down by
    /// anything other than an [`InjectTouchSequence`](Self::InjectTouchSequence),
    /// whose reply already names its own. A mouse appears once it has produced
    /// a sample and stays for the life of the tree; a contact appears at its
    /// press and is gone after its up or cancel.
    QueryPointers,
    /// Switch the app's [`TargetDensity`](teksilo_tokens::TargetDensity).
    ///
    /// **Treat every node id a client holds as dead afterwards.** A density
    /// change rebuilds every root, so every widget a `build()` created is
    /// destroyed and recreated with a fresh `WidgetId` — and therefore a fresh
    /// [`NodeRef`]. Re-`snapshot_tree` or re-`find_node` after it. Setting the
    /// density it already has is a no-op and keeps the ids.
    SetDensity {
        density: DensityDto,
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

/// What a screenshot's pixels actually are, sent alongside the image.
///
/// Pixel dimensions are **physical**, and a live window on a HiDPI display is
/// not laid out at that size: a 800×600 logical window captures as 1600×1200
/// at `scale: 2.0`. Every coordinate elsewhere in this toolkit — node
/// `bounds`, `inject_pointer` — is *logical*, so without `scale` a caller
/// cannot relate a pixel it can see to a point it can click, and a script
/// written against one display silently mis-aims on another. Headless always
/// reports `scale: 1.0`, where the two coincide.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ScreenshotMeta {
    /// Image width in physical pixels.
    pub width: u32,
    /// Image height in physical pixels.
    pub height: u32,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// Non-fatal caveats about the capture, e.g. `webview_hole_possible`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
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
    /// A GPU device existed, but reading the rendered pixels back failed —
    /// device loss (driver restart, compositor crash) or memory pressure.
    /// Distinct from [`GPU_UNAVAILABLE`]: retrying this one can succeed.
    pub const GPU_READBACK_FAILED: &str = "GPU_READBACK_FAILED";
    /// `execute` cannot produce pixels / window lists — the host must.
    pub const HOST_REQUIRED: &str = "HOST_REQUIRED";
    /// The app accepted the request but its UI thread did not answer in time —
    /// it is inside a native modal loop (a file dialog, menu tracking, a
    /// window drag) or otherwise wedged. The request may still be applied
    /// later; re-read the tree rather than assuming it was dropped.
    pub const BRIDGE_TIMEOUT: &str = "BRIDGE_TIMEOUT";
    /// The app dropped the request without answering — it is shutting down.
    pub const BRIDGE_DROPPED: &str = "BRIDGE_DROPPED";
    /// A request could not be parsed as an [`AutomationRequest`](super::AutomationRequest).
    pub const BAD_REQUEST: &str = "BAD_REQUEST";
    /// The client could not talk to the bridge at all.
    pub const BRIDGE_IO: &str = "BRIDGE_IO";
    /// An `assert_node` assertion evaluated to false against a node that does
    /// exist. Deliberately distinct from [`NOT_FOUND`]: "the button is not
    /// focused" and "there is no such button" are different bugs, and a caller
    /// that cannot tell them apart chases the wrong one.
    pub const ASSERTION_FAILED: &str = "ASSERTION_FAILED";
}
