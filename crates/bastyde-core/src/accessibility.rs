// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use accesskit::{Action, Live, Node, NodeId, Role, TextPosition, TextSelection};

use crate::widget_id::WidgetId;

/// Builder wrapper around accesskit::Node for widget accessibility declarations.
pub struct AccessNodeBuilder {
    inner: Node,
    name: Option<String>,
    value: Option<String>,
    role: Role,
    actions: Vec<Action>,
    toggled: Option<bool>,
    expanded: Option<bool>,
    selected: Option<bool>,
    hidden: bool,
    /// The owning widget's id. Set at construction time by the
    /// tree walker (via `AccessNodeBuilder::for_widget`). Used by
    /// the sub-tree API (`push_paragraph_child` / `push_text_run_child`)
    /// to derive synthetic NodeIds without asking the caller to
    /// pass the WidgetId at every call site.
    owner: Option<WidgetId>,
    /// Pending text selection targeting the widget's own node id.
    /// Resolved at `build(id)` time because the widget doesn't know
    /// its node id during `accessibility(&self, builder)`.
    pending_self_selection: Option<(usize, usize)>,
    /// Deferred text selection targeting synthetic child NodeIds
    /// (TextRuns). Unlike `pending_self_selection`, these
    /// TextPositions reference NodeIds that are already known at
    /// the time the widget calls `set_text_selection_to`, so we
    /// can populate the selection during `build(id)` directly —
    /// the field just holds them until that point.
    pending_explicit_selection: Option<(TextPosition, TextPosition)>,
    /// Synthetic child nodes emitted by the widget via
    /// `push_paragraph_child` / `push_text_run_child`. Drained by
    /// the tree walker after `Widget::accessibility(&self, builder)`
    /// returns and merged into the full AccessKit `TreeUpdate`.
    children_collected: Vec<(NodeId, Node)>,
}

/// Discriminator kind for synthetic-NodeId hashing. Different
/// kinds sharing the same (widget_id, element_id) tuple produce
/// distinct NodeIds so paragraph and run nodes for the same
/// source element don't collide.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum SyntheticKind {
    Paragraph = 1,
    TextRun = 2,
    ImageRun = 3,
    /// An inline link span inside a label-style widget's text
    /// (e.g. `[docs](url)` inside a TextWidget with `.markup(true)`
    /// enabled). Each link span becomes one synthetic child of the
    /// hosting widget's own node, with `Role::Link` and the link label
    /// as its name.
    Link = 4,
    /// A lightweight `SceneItem` rendered by `bastyde_scene::SceneView`.
    /// Items live outside the arena — `SceneView::accessibility`
    /// emits one synthetic child per visible item using
    /// `push_scene_child`.
    SceneItem = 5,
    /// A logical grouping declared via `Scene::add_a11y_group`.
    /// Pure AT structure — no visual counterpart. The parent's
    /// children list orders mixed `SceneItem` and `SceneGroup`
    /// synthetic NodeIds however the app declared the logical tree.
    SceneGroup = 6,
    /// A magnetism anchor ("magnet") attached to a scene item. Emitted
    /// as a synthetic child of the owning item's node by
    /// `SceneView::accessibility` when magnetism is enabled, so the
    /// anchor is screen-reader perceivable and can be the target of the
    /// view's `active_descendant` during the keyboard connect flow.
    SceneMagnet = 7,
    /// A per-datum mark (bar / line point / pie slice) emitted by a
    /// `bastyde-charts` widget's `accessibility()`.
    ChartMark = 8,
    /// An annotation body (a comment thread) attached to a run of text, emitted
    /// by a rich-text widget alongside the `TextRun` that carries it. The run
    /// points at this node through the `details` relation — AccessKit's
    /// `aria-details` — which is what lets a screen reader say "has comment" and
    /// let the user navigate in, rather than reciting the thread inline every
    /// time the caret crosses the span.
    Annotation = 9,
}

/// A captured live-region announcement — the text a screen reader would
/// have spoken when a `Live::{Polite,Assertive}` node's value (or label)
/// changed.
///
/// Bastyde has no OS accessibility layer in headless mode, and even with
/// one there is no in-process way to observe what the platform *spoke*.
/// [`crate::WidgetTree::sync_accessibility`] therefore diffs the live
/// nodes of each freshly-built `TreeUpdate` and records the changes into
/// a ring buffer that an automation / test harness drains via
/// [`crate::WidgetTree::announcements_since`]. This is a faithful,
/// in-process model of the live-region stream, not a replacement for an
/// OS screen-reader smoke test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// Monotonic sequence number, starting at 1. `announcements_since(n)`
    /// returns every announcement whose `seq > n`.
    pub seq: u64,
    /// The announced text — the node's `value`, or its `label` when the
    /// node carries no value.
    pub text: String,
    /// `true` for `Live::Assertive`, `false` for `Live::Polite`.
    pub assertive: bool,
}

/// Top bit of the u64 NodeId encoding. Set for synthetic (widget-
/// emitted child) NodeIds, clear for widget-derived NodeIds.
/// Slotmap-derived WidgetIds never set bit 63 in practice because
/// slotmap's KeyData encoding occupies bits 32-63 with a version
/// counter that starts at 1.
pub(crate) const SYNTHETIC_BIT: u64 = 1u64 << 63;

/// Stable hash of (parent widget, element id, kind) producing a
/// synthetic NodeId that survives edits for as long as the
/// underlying element id is stable. Used by `AccessNodeBuilder`'s
/// sub-tree API to allocate NodeIds for paragraph / text-run
/// children without colliding with widget-derived NodeIds.
pub fn synthetic_node_id(parent: WidgetId, element_id: u64, kind: SyntheticKind) -> NodeId {
    use slotmap::Key;
    let parent_raw = parent.data().as_ffi();
    let h = fnv_mix_u64(parent_raw, element_id, kind as u64);
    NodeId((h & !SYNTHETIC_BIT) | SYNTHETIC_BIT)
}

/// Whether a given `NodeId` is a synthetic child node (emitted by a
/// widget via `push_paragraph_child` / `push_text_run_child`) rather
/// than a widget-derived NodeId.
pub fn is_synthetic(id: NodeId) -> bool {
    id.0 & SYNTHETIC_BIT != 0
}

/// FNV-1a-inspired 64-bit mixer for three u64 inputs. Not a
/// cryptographic hash — just a fast, deterministic, well-distributed
/// mix for collision-free synthetic NodeIds across the
/// (widget, element, kind) space.
fn fnv_mix_u64(a: u64, b: u64, c: u64) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut h = FNV_OFFSET;
    for byte in a
        .to_le_bytes()
        .iter()
        .chain(b.to_le_bytes().iter())
        .chain(c.to_le_bytes().iter())
    {
        h ^= *byte as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Styling attributes surfaced to assistive technology on a synthetic
/// `Role::TextRun` node (WCAG 1.3.1 Info and Relationships / EN 301 549
/// 11.5.2.9 "text attributes"). All fields default to "unset"; a rich-text
/// widget populates them per formatting run so a screen reader can convey
/// bold / italic / underline / strikethrough spans. AccessKit has no
/// dedicated `bold` property, so [`bold`](Self::bold) folds into
/// `set_font_weight(700)` when no explicit [`font_weight`](Self::font_weight)
/// is given.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextRunAttributes {
    /// Explicit numeric font weight (`100..=900`). Takes precedence over
    /// [`bold`](Self::bold).
    pub font_weight: Option<u16>,
    /// Bold flag; folded to weight `700` when `font_weight` is `None`.
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// The `TextDecoration` used for underline / strikethrough on a text run.
/// Screen readers key off the *presence* of a decoration, not its colour,
/// so a solid neutral (black) decoration is sufficient; the run's own
/// foreground colour is not plumbed through this synthetic-node path.
fn default_text_decoration() -> accesskit::TextDecoration {
    accesskit::TextDecoration {
        style: accesskit::TextDecorationStyle::Solid,
        color: accesskit::Color {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 255,
        },
    }
}

impl AccessNodeBuilder {
    pub fn new() -> Self {
        Self {
            inner: Node::new(Role::Unknown),
            name: None,
            value: None,
            role: Role::Unknown,
            actions: Vec::new(),
            toggled: None,
            expanded: None,
            selected: None,
            hidden: false,
            owner: None,
            pending_self_selection: None,
            pending_explicit_selection: None,
            children_collected: Vec::new(),
        }
    }

    /// Construct a builder with a known owner WidgetId. Used by the
    /// tree walker when invoking `Widget::accessibility`; the owner
    /// id drives synthetic NodeId derivation in the sub-tree API.
    pub fn for_widget(owner: WidgetId) -> Self {
        let mut b = Self::new();
        b.owner = Some(owner);
        b
    }

    pub fn set_role(&mut self, role: Role) {
        self.role = role;
        self.inner.set_role(role);
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        let name: String = name.into();
        self.inner.set_label(name.clone());
        self.name = Some(name);
    }

    pub fn set_disabled(&mut self) {
        self.inner.set_disabled();
    }

    /// Clear the disabled flag set by an earlier `set_disabled()` call. Used
    /// by the override layer to un-set state a widget emitted unconditionally
    /// (e.g. a Panel that always calls `set_hidden`/`set_disabled`).
    pub fn clear_disabled(&mut self) {
        self.inner.clear_disabled();
    }

    pub fn add_action(&mut self, action: Action) {
        self.inner.add_action(action);
        self.actions.push(action);
    }

    /// Remove a previously-advertised action. Used by the override layer
    /// (`access_remove_action`) to suppress an action a widget emitted but
    /// that doesn't apply in this composition.
    pub fn remove_action(&mut self, action: Action) {
        self.inner.remove_action(action);
        self.actions.retain(|a| *a != action);
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        let v: String = value.into();
        self.inner.set_value(v.clone());
        self.value = Some(v);
    }

    /// Advertise a color value on this node — typically paired with
    /// [`accesskit::Role::ColorWell`]. Takes a `bastyde_tokens::Color` (f32
    /// channels) and quantizes to AccessKit's 8-bit `Color` representation.
    pub fn set_color_value(&mut self, color: bastyde_tokens::Color) {
        let ak = accesskit::Color {
            red: (color.r() * 255.0).round().clamp(0.0, 255.0) as u8,
            green: (color.g() * 255.0).round().clamp(0.0, 255.0) as u8,
            blue: (color.b() * 255.0).round().clamp(0.0, 255.0) as u8,
            alpha: (color.a() * 255.0).round().clamp(0.0, 255.0) as u8,
        };
        self.inner.set_color_value(ak);
    }

    pub fn set_description(&mut self, description: impl Into<String>) {
        self.inner.set_description(description.into());
    }

    pub fn set_live(&mut self, live: Live) {
        self.inner.set_live(live);
    }

    pub fn set_described_by(&mut self, ids: impl Into<Vec<NodeId>>) {
        self.inner.set_described_by(ids);
    }

    /// Append one node to the `described_by` relationship list. Mirror of
    /// the existing `push_controlled`; used by the override layer's
    /// `access_described_by` builder method and by the framework's
    /// tooltip wiring.
    pub fn push_described_by(&mut self, id: NodeId) {
        self.inner.push_described_by(id);
    }

    /// Append one node to the `labelled_by` relationship list. Used by
    /// `access_labelled_by` to point at an external label widget.
    pub fn push_labelled_by(&mut self, id: NodeId) {
        self.inner.push_labelled_by(id);
    }

    /// Replace the `details` relationship list — AccessKit's analogue of
    /// `aria-details`.
    ///
    /// Distinct from `described_by`, and deliberately so: a *description* is text
    /// a screen reader appends when announcing the element, while *details* points
    /// at a structured node the user can navigate **into**. The W3C annotations
    /// pattern is built on that difference — an annotated run carries
    /// `aria-details` to a `role="comment"` node, so the reader can say "has
    /// comment" and let the user go read it, rather than reciting a whole thread
    /// inline every time the caret crosses the span.
    pub fn set_details(&mut self, ids: impl Into<Vec<NodeId>>) {
        self.inner.set_details(ids);
    }

    /// Append one node to the `details` relationship list.
    ///
    /// It is a list, not a single id, because overlapping annotations are normal:
    /// one run of text can carry several comments, and each gets its own entry.
    pub fn push_detail(&mut self, id: NodeId) {
        self.inner.push_detail(id);
    }

    /// Stable author-supplied identifier (test/debug id, equivalent to
    /// `aria-label`-style `data-testid`). Maps to `accesskit::Node::set_author_id`.
    pub fn set_author_id(&mut self, id: impl Into<String>) {
        self.inner.set_author_id(id.into());
    }

    /// Replace the node's custom-action list with `actions`. Used by the
    /// override layer's `access_custom_action` builder method.
    pub fn set_custom_actions(&mut self, actions: Vec<accesskit::CustomAction>) {
        self.inner.set_custom_actions(actions);
    }

    pub fn set_toggled(&mut self, toggled: bool) {
        self.toggled = Some(toggled);
        self.inner.set_toggled(if toggled {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = Some(expanded);
        self.inner.set_expanded(expanded);
    }

    pub fn set_has_popup(&mut self, kind: accesskit::HasPopup) {
        self.inner.set_has_popup(kind);
    }

    /// Placeholder text displayed when the widget has no user-entered value
    /// yet. Screen readers treat this distinctly from `value` — they'll
    /// announce the placeholder as hint text rather than as the current
    /// value. Used by `ComboBox` when selection is `None`, by `TextInput`
    /// before the user types, etc.
    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.inner.set_placeholder(placeholder.into());
    }

    /// Target URL for link-like widgets. Maps to `aria-url` / platform
    /// link metadata so screen readers can announce the destination
    /// (e.g. "link, `https://example.com`"). Informational only — does
    /// not navigate when activated.
    pub fn set_url(&mut self, url: impl Into<String>) {
        self.inner.set_url(url.into());
    }

    /// Keyboard shortcut announcement (e.g. `"Ctrl+S"`). Maps to
    /// `aria-keyshortcuts`. Used by menu items and buttons whose
    /// chord is shown visually but must also be exposed to assistive
    /// tech so shortcut users discover it.
    pub fn set_keyboard_shortcut(&mut self, shortcut: impl Into<String>) {
        self.inner.set_keyboard_shortcut(shortcut.into());
    }

    /// Autocomplete behavior for combobox / text input widgets. Maps to
    /// ARIA `aria-autocomplete`: `Inline` completes within the field,
    /// `List` shows a popup of matching values, `Both` does both.
    pub fn set_auto_complete(&mut self, kind: accesskit::AutoComplete) {
        self.inner.set_auto_complete(kind);
    }

    /// Selection state — used by `RadioButton`, `Tab`, `ListBoxOption`,
    /// `TreeItem`, menu items in radio/check groups, etc. This is the
    /// correct property for "this option in a mutually exclusive
    /// group is the active one"; don't confuse with `set_toggled`,
    /// which models checkbox/switch on-off state.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = Some(selected);
        self.inner.set_selected(selected);
    }

    pub fn set_orientation(&mut self, orientation: accesskit::Orientation) {
        self.inner.set_orientation(orientation);
    }

    /// 1-based index of this item in its parent set — maps to ARIA
    /// `aria-posinset`. Pair with `set_size_of_set` on every item in
    /// the set so AT can announce "tab 3 of 5", "row 12 of 200", etc.
    /// Use on `Role::Tab`, `Role::ListBoxOption`, `Role::Row`,
    /// `Role::MenuItem`, and similar collection items.
    pub fn set_position_in_set(&mut self, position: usize) {
        self.inner.set_position_in_set(position);
    }

    /// Total number of items in this item's parent set — maps to ARIA
    /// `aria-setsize`. Set on every collection item alongside
    /// `set_position_in_set`; the value should reflect the *logical*
    /// set size, not the visible window (e.g. report 200 for a
    /// virtualized 200-row list even when only 20 rows are realized).
    pub fn set_size_of_set(&mut self, size: usize) {
        self.inner.set_size_of_set(size);
    }

    // ── Grid / table semantics (`aria-rowcount` / `aria-colindex` / …) ──
    //
    // Typed wrappers over the corresponding `accesskit::Node` setters so
    // grid/table widgets don't have to drop to `inner_mut()`. On a
    // `Role::Grid` / `Role::Table` container set the *logical* row/column
    // counts (not the realized window); on each cell set its 1-based
    // row/column index.

    /// Total logical row count on a grid/table container (`aria-rowcount`).
    pub fn set_row_count(&mut self, count: usize) {
        self.inner.set_row_count(count);
    }

    /// Total logical column count on a grid/table container (`aria-colcount`).
    pub fn set_column_count(&mut self, count: usize) {
        self.inner.set_column_count(count);
    }

    /// 1-based row index of a cell / row (`aria-rowindex`).
    pub fn set_row_index(&mut self, index: usize) {
        self.inner.set_row_index(index);
    }

    /// 1-based column index of a cell (`aria-colindex`).
    pub fn set_column_index(&mut self, index: usize) {
        self.inner.set_column_index(index);
    }

    /// Number of rows a cell spans (`aria-rowspan`).
    pub fn set_row_span(&mut self, span: usize) {
        self.inner.set_row_span(span);
    }

    /// Number of columns a cell spans (`aria-colspan`).
    pub fn set_column_span(&mut self, span: usize) {
        self.inner.set_column_span(span);
    }

    /// Whether the container allows multiple selected items
    /// (`aria-multiselectable`). Set on the `Role::Grid` / `Role::ListBox`
    /// container in multi-select mode.
    pub fn set_multiselectable(&mut self, value: bool) {
        if value {
            self.inner.set_multiselectable();
        } else {
            self.inner.clear_multiselectable();
        }
    }

    /// The currently-active descendant (`aria-activedescendant`) — the
    /// roving-focus pattern where focus stays on a composite container and
    /// this points at the focused child (e.g. the focused grid cell).
    pub fn set_active_descendant(&mut self, id: NodeId) {
        self.inner.set_active_descendant(id);
    }

    /// Flag the node as a modal dialog. Use on `Role::Dialog` /
    /// `Role::AlertDialog` when input is blocked outside the dialog.
    pub fn set_modal(&mut self) {
        self.inner.set_modal();
    }

    /// Mark this node as the current item within its container
    /// (e.g. the "current page" crumb inside a `Navigation`, the
    /// current step in a wizard). Maps to ARIA `aria-current`.
    pub fn set_aria_current(&mut self, current: accesskit::AriaCurrent) {
        self.inner.set_aria_current(current);
    }

    /// Single-step delta for `Slider` / `SpinButton` — how much the
    /// value changes per keyboard arrow or Action::Increment tick.
    pub fn set_numeric_value_step(&mut self, step: f64) {
        self.inner.set_numeric_value_step(step);
    }

    /// Page-step delta for `Slider` / `SpinButton` — how much the
    /// value changes per PgUp/PgDown or coarse adjustment.
    pub fn set_numeric_value_jump(&mut self, jump: f64) {
        self.inner.set_numeric_value_jump(jump);
    }

    /// Append a controlled-node relationship — e.g. a `Tab` pointing
    /// at its matching `TabPanel`, a `ComboBox` pointing at its
    /// listbox popup. AccessKit / ARIA equivalent of `aria-controls`.
    pub fn push_controlled(&mut self, id: NodeId) {
        self.inner.push_controlled(id);
    }

    /// Declare this radio button's membership in a radio group.
    /// Each `RadioButton` node should push every sibling in its
    /// group (including itself); screen readers use this to
    /// announce positional info like "2 of 3".
    pub fn push_to_radio_group(&mut self, id: NodeId) {
        self.inner.push_to_radio_group(id);
    }

    pub fn set_numeric_value(&mut self, value: f64) {
        self.inner.set_numeric_value(value);
    }

    pub fn set_min_numeric_value(&mut self, value: f64) {
        self.inner.set_min_numeric_value(value);
    }

    pub fn set_max_numeric_value(&mut self, value: f64) {
        self.inner.set_max_numeric_value(value);
    }

    /// Hide this node from all assistive technologies (equivalent to
    /// `aria-hidden="true"`). The node is still in the widget tree but
    /// is invisible to screen readers and other ATs. Use for purely
    /// decorative elements — e.g. scrollbars (AT scrolls via the
    /// parent `ScrollView`'s scroll actions instead).
    pub fn set_hidden(&mut self) {
        self.hidden = true;
        self.inner.set_hidden();
    }

    /// Clear the hidden flag set by an earlier `set_hidden()` call. Used
    /// by the override layer to re-expose a widget that marked itself
    /// presentational. AccessKit's Node `hidden` is local — un-hiding this
    /// node does not propagate to descendants, but descendants are not
    /// transitively hidden by their ancestor's `hidden` either.
    pub fn clear_hidden(&mut self) {
        self.hidden = false;
        self.inner.clear_hidden();
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    pub fn toggled(&self) -> Option<bool> {
        self.toggled
    }

    pub fn expanded(&self) -> Option<bool> {
        self.expanded
    }

    pub fn selected(&self) -> Option<bool> {
        self.selected
    }

    /// Build the AccessKit Node with the given ID. Resolves any
    /// `pending_self_selection` recorded via `set_caret_position_on_self`
    /// or `set_text_selection_on_self` — at this point we know the
    /// widget's NodeId and can inject it into the text selection.
    /// Returns the primary `(NodeId, Node)` pair plus any synthetic
    /// child nodes emitted by the widget via `push_paragraph_child`
    /// / `push_text_run_child`. The tree walker is responsible for
    /// merging these into the final `TreeUpdate`.
    pub fn build(mut self, id: WidgetId) -> (NodeId, Node, Vec<(NodeId, Node)>) {
        let node_id = widget_id_to_node_id(id);
        // Priority: explicit (child-targeting) selection wins over
        // self-targeting selection — widgets that emit TextRun
        // children use the explicit path.
        if let Some((anchor, focus)) = self.pending_explicit_selection.take() {
            let selection = TextSelection { anchor, focus };
            self.inner.set_text_selection(selection);
        } else if let Some((anchor, focus)) = self.pending_self_selection.take() {
            let selection = TextSelection {
                anchor: TextPosition {
                    node: node_id,
                    character_index: anchor,
                },
                focus: TextPosition {
                    node: node_id,
                    character_index: focus,
                },
            };
            self.inner.set_text_selection(selection);
        }

        // accesskit contract: a `Role::Label` node carries its text in the
        // `value` property, NOT `label`. Every platform adapter reads it that
        // way — Windows UIA derives the node's Name from `value` (its
        // `label_comes_from_value()` returns true for `Role::Label`), macOS
        // maps `Role::Label` to `NSAccessibilityStaticTextRole` whose content
        // is exposed as AXValue, and `accesskit_consumer` reads `value` when
        // another control is `labelled_by` this node. A name left in the
        // `label` property is therefore silently dropped on Windows and stray
        // on macOS. Widgets set the accessible name uniformly via `set_name`
        // (-> the `label` property); re-serialize it to `value` here, the one
        // place every emitted node is finalized — widget nodes (via the tree
        // walker) and scene synthetic children (via `push_scene_child`, which
        // also funnels through `build`). The builder's logical `name()` view
        // is intentionally left untouched, so introspection / `find_by_label`
        // continue to report the accessible name regardless of role. Reading
        // the inner node directly keeps this robust against any label set
        // outside `set_name`, and idempotent (a second pass finds no label).
        if self.inner.role() == Role::Label
            && let Some(label) = self.inner.label().map(|s| s.to_string())
        {
            if self.inner.value().is_none() {
                self.inner.set_value(label);
            }
            self.inner.clear_label();
        }

        (node_id, self.inner, self.children_collected)
    }

    /// Get a reference to the inner node for advanced use.
    pub fn inner_mut(&mut self) -> &mut Node {
        &mut self.inner
    }

    /// The widget id this builder was constructed for, if any. Set
    /// by [`AccessNodeBuilder::for_widget`]; used by the scene-tree
    /// walker to derive synthetic `NodeId`s for items / groups outside
    /// the closure form (`push_scene_child*`).
    pub fn owner_id(&self) -> Option<crate::widget_id::WidgetId> {
        self.owner
    }

    /// Run a mutator over a synthetic child node previously pushed
    /// via `push_scene_child` (or its `_under` variant). Used by
    /// the scene walker to apply cross-tree decorations (relations /
    /// live regions / landmarks) after the initial hierarchy emit.
    /// Returns `true` if the node was found.
    ///
    /// Cannot be used to mutate widget-derived NodeIds — those live
    /// in the global TreeUpdate and are owned by other widgets.
    pub fn with_collected_node<F: FnOnce(&mut Node)>(&mut self, node_id: NodeId, f: F) -> bool {
        for (id, node) in self.children_collected.iter_mut() {
            if *id == node_id {
                f(node);
                return true;
            }
        }
        false
    }

    /// Mark this node as read-only. Used by `RichTextEditor::read_only` so
    /// screen readers announce the widget as a document rather than a form
    /// field.
    pub fn set_read_only(&mut self) {
        self.inner.set_read_only();
    }

    /// Declare the current text selection. `anchor` and `focus` are
    /// character indices into the widget's flat text representation; pass
    /// equal indices for a collapsed caret. Uses the same `NodeId` for
    /// both positions (typical for single-node text widgets that expose
    /// the document as one run, which is what the first milestone of
    /// `RichTextEditor` does).
    pub fn set_text_selection(&mut self, node_id: NodeId, anchor: usize, focus: usize) {
        let selection = TextSelection {
            anchor: TextPosition {
                node: node_id,
                character_index: anchor,
            },
            focus: TextPosition {
                node: node_id,
                character_index: focus,
            },
        };
        self.inner.set_text_selection(selection);
    }

    /// Convenience for exposing a caret position as a collapsed selection.
    pub fn set_caret_position(&mut self, node_id: NodeId, character_index: usize) {
        self.set_text_selection(node_id, character_index, character_index);
    }

    /// Declare a text selection whose anchor and focus live on the
    /// widget's own AccessKit node. The widget doesn't know its own
    /// `NodeId` inside `accessibility(&self, builder)` — it's only
    /// resolved when the tree walker calls `builder.build(widget_id)`.
    /// This method stashes the character indices and defers the
    /// `set_text_selection` call until `build()` knows the ID.
    pub fn set_text_selection_on_self(&mut self, anchor: usize, focus: usize) {
        self.pending_self_selection = Some((anchor, focus));
    }

    /// Convenience wrapper for a collapsed caret on the widget's own node.
    pub fn set_caret_position_on_self(&mut self, character_index: usize) {
        self.set_text_selection_on_self(character_index, character_index);
    }

    // ── Sub-tree API: multi-node widgets (rich text, etc.) ─────────────

    /// Push a `Role::Paragraph` child on the current node and return
    /// its `NodeId`. The NodeId is synthetic (bit 63 set) and
    /// deterministic given the owning widget + `element_id`.
    ///
    /// The owning `WidgetId` comes from the builder's `owner`
    /// field, set by `AccessNodeBuilder::for_widget`. Returns
    /// `NodeId(0)` (a no-op placeholder) if the builder has no
    /// owner, which can only happen when a widget constructs a
    /// builder manually via `new()` instead of going through the
    /// tree walker. That's a programming error worth catching in
    /// debug.
    pub fn push_paragraph_child(&mut self, element_id: u64) -> NodeId {
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_paragraph_child called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        let node_id = synthetic_node_id(owner, element_id, SyntheticKind::Paragraph);
        let node = Node::new(Role::Paragraph);
        self.children_collected.push((node_id, node));
        self.inner.push_child(node_id);
        node_id
    }

    /// Push a `Role::Comment` child carrying an annotation's text, and return its
    /// `NodeId` so the annotated run can point at it via [`push_detail`].
    ///
    /// `group_id` must be the annotation's own durable identity (a comment's uid,
    /// never a store id), so the node keeps the same `NodeId` across rebuilds and
    /// a screen reader's cursor is not thrown out of the thread by an unrelated
    /// edit elsewhere in the document.
    ///
    /// Per the W3C annotations pattern the *body* carries the name; the annotated
    /// span itself must NOT be given an accessible name (`role="mark"` forbids it)
    /// — naming the span would make the reader announce the comment's text in
    /// place of the prose.
    pub fn push_annotation_child(&mut self, group_id: u64, text: impl Into<String>) -> NodeId {
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_annotation_child called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        let node_id = synthetic_node_id(owner, group_id, SyntheticKind::Annotation);
        let mut node = Node::new(Role::Comment);
        node.set_value(text.into());
        self.children_collected.push((node_id, node));
        self.inner.push_child(node_id);
        node_id
    }

    /// Add a `details` target to an already-pushed **child** node.
    ///
    /// The sub-tree API builds children eagerly into `children_collected`, so a
    /// relation between two synthetic siblings (a `TextRun` and its annotation
    /// body) cannot go through the current node's own setters — it has to reach
    /// back into the collected child. A no-op if `child` was never pushed, which
    /// keeps a caller that emitted spans for a run it then skipped from panicking.
    pub fn push_detail_on_child(&mut self, child: NodeId, detail: NodeId) {
        if let Some((_, node)) = self.children_collected.iter_mut().find(|(id, _)| *id == child) {
            node.push_detail(detail);
        }
    }

    /// Push a `Role::Link` child on the current node. Used by label
    /// widgets (e.g. `TextWidget` with `.markup(true)` enabled) to
    /// expose inline `[label](url)` links as individual accessible
    /// nodes alongside the parent's own text.
    ///
    /// `element_id` should be a stable identifier for the link inside
    /// the parent widget (typically the byte offset of the `[` in the
    /// original markup source, so the NodeId survives identical
    /// re-layouts).
    ///
    /// The returned `NodeId` is synthetic (bit 63 set) and deterministic
    /// given `(owner, element_id)`.
    pub fn push_link_child(
        &mut self,
        element_id: u64,
        label: impl Into<String>,
        url: impl Into<String>,
    ) -> NodeId {
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_link_child called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        let node_id = synthetic_node_id(owner, element_id, SyntheticKind::Link);
        let mut node = Node::new(Role::Link);
        let label: String = label.into();
        if !label.is_empty() {
            node.set_label(label);
        }
        // AccessKit exposes the link target through the `Value` property
        // on a `Role::Link` node — same convention the standalone
        // `Link` widget uses via `set_value(...)`.
        node.set_value(url.into());
        self.children_collected.push((node_id, node));
        self.inner.push_child(node_id);
        node_id
    }

    /// Push a synthetic child node representing a lightweight
    /// `SceneItem` (or `SceneGroup`) emitted by `bastyde_scene::SceneView`.
    /// The caller customizes a sub-`AccessNodeBuilder` (mirroring the
    /// `Widget::accessibility` shape) and gets back the
    /// deterministic synthetic `NodeId` allocated for the
    /// `(owner, element_id, kind)` tuple.
    ///
    /// `kind` must be [`SyntheticKind::SceneItem`] or
    /// [`SyntheticKind::SceneGroup`]; passing any other variant
    /// panics in debug.
    ///
    /// Any further synthetic children the closure pushes (a
    /// `SceneGroup` containing nested `SceneItem`s) are forwarded into the parent's
    /// `children_collected` and re-parented under the
    /// just-pushed node via the closure's own `inner.push_child`
    /// calls — same convention as `push_paragraph_child` →
    /// `push_text_run_child`.
    pub fn push_scene_child(
        &mut self,
        element_id: u64,
        kind: SyntheticKind,
        customize: impl FnOnce(&mut AccessNodeBuilder),
    ) -> NodeId {
        debug_assert!(
            matches!(
                kind,
                SyntheticKind::SceneItem
                    | SyntheticKind::SceneGroup
                    | SyntheticKind::SceneMagnet
                    | SyntheticKind::ChartMark
            ),
            "push_scene_child requires SyntheticKind::SceneItem, ::SceneGroup, ::SceneMagnet, or ::ChartMark"
        );
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_scene_child called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        let node_id = synthetic_node_id(owner, element_id, kind);
        // Build the child against a fresh sub-builder so the item
        // sees the same `&mut AccessNodeBuilder` shape as widgets.
        // Owner-id is the SceneView's so further `push_scene_child`
        // calls inside the customize closure (a `SceneGroup`
        // emitting nested items) hash off the same owner.
        let mut child_builder = AccessNodeBuilder::for_widget(owner);
        customize(&mut child_builder);
        // `build(owner)` re-derives a widget-keyed NodeId we throw
        // away — we use our synthetic `node_id` instead. The
        // returned `Node` carries the role / label / bounds / etc
        // the customize closure populated; any *grand*children the
        // closure pushed via further `push_scene_child` calls come
        // back in the third tuple field and we forward them so the
        // main TreeUpdate sees the full subtree.
        let (_unused, node, grand_children) = child_builder.build(owner);
        self.children_collected.push((node_id, node));
        for (gid, gnode) in grand_children {
            self.children_collected.push((gid, gnode));
        }
        self.inner.push_child(node_id);
        node_id
    }

    /// Append an existing synthetic node id as a child of a
    /// previously-pushed `SceneGroup` (or `SceneItem`) child. Used by
    /// the scene logical-tree walker to re-parent items under their
    /// declared logical group rather than as direct children of the
    /// SceneView.
    ///
    /// Returns `true` if the parent was found (and the child was
    /// attached), `false` if the parent isn't in
    /// `children_collected` — the caller misordered the pushes.
    pub fn attach_scene_child_under(&mut self, parent: NodeId, child: NodeId) -> bool {
        for (id, node) in self.children_collected.iter_mut() {
            if *id == parent {
                node.push_child(child);
                return true;
            }
        }
        false
    }

    /// Like `push_scene_child` but lets the caller pick the
    /// parent. `parent = None` attaches to the widget's own node
    /// (same behavior as `push_scene_child`); `parent = Some(...)`
    /// attaches to the previously-pushed scene-child with that id.
    /// The scene logical-tree walker uses this to nest scene items
    /// under declared `A11yGroup` parents.
    ///
    /// Returns the deterministic synthetic `NodeId` for the new
    /// child. If `parent` was `Some` but the parent wasn't found
    /// in `children_collected`, the child still gets created and
    /// recorded but ends up attached to the widget's own node as a
    /// fallback (and a debug-assert fires).
    pub fn push_scene_child_under(
        &mut self,
        parent: Option<NodeId>,
        element_id: u64,
        kind: SyntheticKind,
        customize: impl FnOnce(&mut AccessNodeBuilder),
    ) -> NodeId {
        debug_assert!(
            matches!(
                kind,
                SyntheticKind::SceneItem
                    | SyntheticKind::SceneGroup
                    | SyntheticKind::SceneMagnet
                    | SyntheticKind::ChartMark
            ),
            "push_scene_child_under requires SyntheticKind::SceneItem, ::SceneGroup, ::SceneMagnet, or ::ChartMark"
        );
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_scene_child_under called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        let node_id = synthetic_node_id(owner, element_id, kind);
        let mut child_builder = AccessNodeBuilder::for_widget(owner);
        customize(&mut child_builder);
        let (_unused, node, grand_children) = child_builder.build(owner);
        self.children_collected.push((node_id, node));
        for (gid, gnode) in grand_children {
            self.children_collected.push((gid, gnode));
        }
        match parent {
            Some(parent_id) => {
                let attached = self.attach_scene_child_under(parent_id, node_id);
                if !attached {
                    debug_assert!(
                        false,
                        "push_scene_child_under: parent {:?} not in children_collected — \
                         caller must push the parent before its children",
                        parent_id
                    );
                    self.inner.push_child(node_id);
                }
            }
            None => {
                self.inner.push_child(node_id);
            }
        }
        node_id
    }

    /// Override a previously-pushed paragraph child's role to
    /// `Role::Heading` with the given hierarchical level. Used by
    /// the rich text editor when a block carries a
    /// `BlockFormat::heading_level`. Returns `true` if the node was
    /// found and updated, `false` otherwise (caller misused the
    /// api — the paragraph must have been pushed earlier).
    pub fn set_paragraph_as_heading(&mut self, node_id: NodeId, level: u8) -> bool {
        for (id, node) in self.children_collected.iter_mut() {
            if *id == node_id {
                node.set_role(Role::Heading);
                // AccessKit's `set_level` takes a usize (via the
                // usize_property_methods macro). Clamp to 1..=6 for
                // conventional heading semantics.
                let level: usize = (level as usize).clamp(1, 6);
                node.set_level(level);
                return true;
            }
        }
        false
    }

    /// Set 1-based position-in-set / size-of-set on a previously-pushed
    /// synthetic child (a paragraph, "line 42 of 200").
    ///
    /// AccessKit exposes `position_in_set` / `size_of_set` on every node, but
    /// [`set_position_in_set`](Self::set_position_in_set) /
    /// [`set_size_of_set`](Self::set_size_of_set) only touch the widget's own
    /// node. This reaches a collected child by NodeId, the same way
    /// [`set_paragraph_as_heading`](Self::set_paragraph_as_heading) does.
    /// Returns whether the child was found.
    pub fn set_child_position_in_set(
        &mut self,
        node_id: NodeId,
        position: usize,
        size: usize,
    ) -> bool {
        self.with_collected_node(node_id, |node| {
            node.set_position_in_set(position);
            node.set_size_of_set(size);
        })
    }

    /// Link a run of `Role::TextRun` children as one visual line, so assistive
    /// technology navigating by line treats them as a continuous line rather
    /// than fracturing at each formatting or chunk boundary.
    ///
    /// Sets each run's `next_on_line` to its successor and each successor's
    /// `previous_on_line` to its predecessor (AccessKit's doubly-linked
    /// same-line chain); the first run keeps no `previous_on_line` and the last
    /// no `next_on_line`, which is how the consumer detects the line's ends. A
    /// slice of zero or one is a no-op. Every id must be a run pushed earlier via
    /// [`push_text_run_child`](Self::push_text_run_child).
    pub fn link_runs_on_line(&mut self, run_ids: &[NodeId]) {
        for pair in run_ids.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            self.with_collected_node(a, |node| node.set_next_on_line(b));
            self.with_collected_node(b, |node| node.set_previous_on_line(a));
        }
    }

    /// Push a `Role::TextRun` child under `parent_node` (usually a
    /// paragraph NodeId returned from `push_paragraph_child`, but
    /// may also be the widget's own node for inline editors).
    ///
    /// `element_id` is the stable id of the underlying text-document
    /// inline element; combined with `parent_widget` and a
    /// disambiguator it produces a synthetic NodeId that survives
    /// edits. `fragment_offset` is the block-relative character
    /// offset of this run — used as the disambiguator so two
    /// highlight-split sub-runs sharing one source element don't
    /// collide.
    ///
    /// `character_lengths` must be the UTF-8 byte length of each
    /// character in `value`, per AccessKit's contract. Optional
    /// `word_starts`, `character_positions`, and `character_widths`
    /// populate the corresponding AccessKit properties.
    ///
    /// Returns the allocated synthetic `NodeId` so the caller can
    /// reference it later when attaching a `TextSelection` via
    /// `set_text_selection_to`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_text_run_child(
        &mut self,
        parent_node: NodeId,
        element_id: u64,
        fragment_offset: usize,
        value: String,
        character_lengths: Vec<u8>,
        word_starts: Option<Vec<u8>>,
        character_positions: Option<Vec<f32>>,
        character_widths: Option<Vec<f32>>,
        attrs: TextRunAttributes,
    ) -> NodeId {
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_text_run_child called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        // Give sub-runs of one source element (a highlight split, or a run
        // chunked to stay under the AccessKit word-start cap) distinct NodeIds.
        // A plain `element_id ^ (fragment_offset << 32)` would XOR the offset
        // into the very bits `element_id` already uses to encode the owning
        // block, so a chunk at offset 255 in block A could alias a whole-line run
        // in a block whose id is `A ^ 255`. Hashing the offset across all 64 bits
        // removes that structure; `fragment_offset == 0` (the whole-run common
        // case) stays a no-op, so those NodeIds are unchanged.
        let mixed_element = if fragment_offset == 0 {
            element_id
        } else {
            fnv_mix_u64(element_id, fragment_offset as u64, 0)
        };
        let node_id = synthetic_node_id(owner, mixed_element, SyntheticKind::TextRun);
        let mut node = Node::new(Role::TextRun);
        node.set_value(value);
        node.set_character_lengths(character_lengths);
        if let Some(ws) = word_starts {
            node.set_word_starts(ws);
        }
        if let Some(pos) = character_positions {
            node.set_character_positions(pos);
        }
        if let Some(widths) = character_widths {
            node.set_character_widths(widths);
        }
        // Text attributes (WCAG 1.3.1 / EN 301 549 11.5.2.9). AccessKit has no
        // bold flag, so an explicit weight wins, else bold => 700.
        if let Some(w) = attrs.font_weight {
            node.set_font_weight(w as f32);
        } else if attrs.bold {
            node.set_font_weight(700.0);
        }
        if attrs.italic {
            node.set_italic();
        }
        if attrs.underline {
            node.set_underline(default_text_decoration());
        }
        if attrs.strikethrough {
            node.set_strikethrough(default_text_decoration());
        }
        self.children_collected.push((node_id, node));
        // Attach the text-run to its parent paragraph's child list.
        // The parent must already be in `children_collected`.
        for (id, parent) in self.children_collected.iter_mut() {
            if *id == parent_node {
                parent.push_child(node_id);
                return node_id;
            }
        }
        // Parent not found — push as a direct child of the widget's
        // own node as a fallback. Caller misused the API.
        self.inner.push_child(node_id);
        node_id
    }

    /// Push a single `Role::TextRun` child attached **directly** to the
    /// widget's own node (no intervening `Role::Paragraph`). This is the
    /// single-line text-input shape: `Role::TextInput` → one
    /// `Role::TextRun`.
    ///
    /// Required for screen-reader typing echo. accesskit_consumer's
    /// `supports_text_ranges()` returns `false` for a text input that
    /// only sets `character_lengths` on its *own* node — it needs a
    /// `Role::TextRun` child. Without it the macOS adapter never emits
    /// `AXSelectedTextChanged`, so VoiceOver reads the value once on
    /// focus but never echoes characters/words while typing. Emit this
    /// even when `value` / `character_lengths` are empty so
    /// `supports_text_ranges()` is already true before the first
    /// keystroke (the change-diff's *old* node must also support ranges
    /// for the notification to fire). Target the caret/selection at the
    /// returned `NodeId` via [`set_text_selection_to`](Self::set_text_selection_to).
    pub fn push_text_run_child_on_self(
        &mut self,
        element_id: u64,
        value: String,
        character_lengths: Vec<u8>,
        word_starts: Option<Vec<u8>>,
    ) -> NodeId {
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_text_run_child_on_self called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        let node_id = synthetic_node_id(owner, element_id, SyntheticKind::TextRun);
        let mut node = Node::new(Role::TextRun);
        node.set_value(value);
        node.set_character_lengths(character_lengths);
        if let Some(ws) = word_starts {
            node.set_word_starts(ws);
        }
        self.children_collected.push((node_id, node));
        self.inner.push_child(node_id);
        node_id
    }

    /// Declare a text selection that references TextRun children
    /// previously emitted via `push_text_run_child`. Both the
    /// anchor and the focus are expressed as
    /// `(NodeId, character_index)` pairs where the character index
    /// is an index into the target TextRun's `character_lengths`
    /// (NOT a document-absolute offset — per AccessKit's contract).
    pub fn set_text_selection_to(&mut self, anchor: (NodeId, usize), focus: (NodeId, usize)) {
        self.pending_explicit_selection = Some((
            TextPosition {
                node: anchor.0,
                character_index: anchor.1,
            },
            TextPosition {
                node: focus.0,
                character_index: focus.1,
            },
        ));
    }
}

impl Default for AccessNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a WidgetId to an AccessKit NodeId.
pub fn widget_id_to_node_id(id: WidgetId) -> NodeId {
    use slotmap::Key;
    let key_data = id.data();
    let raw = key_data.as_ffi();
    NodeId(raw)
}

/// Convert an AccessKit NodeId back to a WidgetId. Returns `None`
/// for synthetic NodeIds (widget-emitted child nodes like TextRuns);
/// callers that need to route an `ActionRequest` targeting a
/// synthetic NodeId must consult `WidgetTree::synthetic_parent_map`
/// to find the owning widget.
pub fn node_id_to_widget_id_maybe(node_id: NodeId) -> Option<WidgetId> {
    if is_synthetic(node_id) {
        return None;
    }
    use slotmap::KeyData;
    let key_data = KeyData::from_ffi(node_id.0);
    Some(key_data.into())
}

/// Legacy infallible converter kept for existing call sites that
/// never encounter synthetic NodeIds. New code should prefer
/// [`node_id_to_widget_id_maybe`]. Panics in debug for synthetic
/// ids to catch misrouted calls early.
pub fn node_id_to_widget_id(node_id: NodeId) -> WidgetId {
    debug_assert!(
        !is_synthetic(node_id),
        "node_id_to_widget_id called on synthetic NodeId — use node_id_to_widget_id_maybe"
    );
    use slotmap::KeyData;
    let key_data = KeyData::from_ffi(node_id.0);
    key_data.into()
}

/// The special root node ID for the accessibility tree.
pub fn root_node_id() -> NodeId {
    NodeId(0)
}

/// Query result for accessibility information about a widget.
#[derive(Debug)]
pub struct AccessibilityInfo {
    role: Role,
    name: Option<String>,
    actions: Vec<Action>,
    toggled: Option<bool>,
    expanded: Option<bool>,
    selected: Option<bool>,
    disabled: bool,
    hidden: bool,
}

impl AccessibilityInfo {
    pub fn new(role: Role, name: Option<String>, actions: Vec<Action>) -> Self {
        Self {
            role,
            name,
            actions,
            toggled: None,
            expanded: None,
            selected: None,
            disabled: false,
            hidden: false,
        }
    }

    pub fn with_toggled(mut self, toggled: bool) -> Self {
        self.toggled = Some(toggled);
        self
    }

    pub fn with_expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub fn is_toggled(&self) -> bool {
        self.toggled.unwrap_or(false)
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.unwrap_or(false)
    }

    pub fn is_selected(&self) -> bool {
        self.selected.unwrap_or(false)
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_widget(id: u64) -> WidgetId {
        slotmap::KeyData::from_ffi(id).into()
    }

    #[test]
    fn widget_derived_node_id_has_bit_63_clear() {
        // A freshly-minted slotmap key (version 1, index 0) encodes
        // to a u64 with bit 63 clear. The top-bit namespace split
        // (synthetic NodeIds set bit 63, widget-derived NodeIds clear
        // it) only works if widget-derived NodeIds stay below bit 63.
        let wid = fake_widget(1);
        let nid = widget_id_to_node_id(wid);
        assert_eq!(
            nid.0 & SYNTHETIC_BIT,
            0,
            "widget NodeId must have bit 63 clear"
        );
        assert!(!is_synthetic(nid));
    }

    #[test]
    fn synthetic_node_id_has_bit_63_set() {
        let wid = fake_widget(42);
        let nid = synthetic_node_id(wid, 17, SyntheticKind::TextRun);
        assert_eq!(nid.0 & SYNTHETIC_BIT, SYNTHETIC_BIT);
        assert!(is_synthetic(nid));
    }

    #[test]
    fn link_runs_on_line_chains_runs_both_ways() {
        let mut b = AccessNodeBuilder::for_widget(fake_widget(1));
        let para = b.push_paragraph_child(1);
        let run = |b: &mut AccessNodeBuilder, off: usize| {
            b.push_text_run_child(
                para,
                10,
                off,
                "abc".to_string(),
                vec![1, 1, 1],
                None,
                None,
                None,
                TextRunAttributes::default(),
            )
        };
        let (r0, r1, r2) = (run(&mut b, 0), run(&mut b, 3), run(&mut b, 6));
        b.link_runs_on_line(&[r0, r1, r2]);

        let (_id, _n, children) = b.build(fake_widget(1));
        let node = |id| {
            children
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| n)
                .unwrap()
        };
        // First run: forward only. Middle: both. Last: back only.
        assert_eq!(node(r0).previous_on_line(), None);
        assert_eq!(node(r0).next_on_line(), Some(r1));
        assert_eq!(node(r1).previous_on_line(), Some(r0));
        assert_eq!(node(r1).next_on_line(), Some(r2));
        assert_eq!(node(r2).previous_on_line(), Some(r1));
        assert_eq!(node(r2).next_on_line(), None);
    }

    /// A chunk at a non-zero offset in one element must not collide with a
    /// whole run in another element, even when the two element ids differ by
    /// exactly the low-byte XOR of the chunk offset — the aliasing the old
    /// `element_id ^ (offset << 32)` mix allowed.
    #[test]
    fn a_chunk_offset_does_not_alias_another_elements_run() {
        let mut b = AccessNodeBuilder::for_widget(fake_widget(1));
        let para = b.push_paragraph_child(1);
        // `synth_element_id` encodes the block id in bits 32-61.
        let elem_a: u64 = 0xABCD_u64 << 32;
        let elem_b: u64 = (0xABCD_u64 ^ 255) << 32;
        let run = |b: &mut AccessNodeBuilder, elem: u64, off: usize| {
            b.push_text_run_child(
                para,
                elem,
                off,
                "x".to_string(),
                vec![1],
                None,
                None,
                None,
                TextRunAttributes::default(),
            )
        };
        let a_chunk = run(&mut b, elem_a, 255); // offset-255 chunk in block A
        let b_whole = run(&mut b, elem_b, 0); // whole run in block B = A ^ 255
        assert_ne!(
            a_chunk, b_whole,
            "a chunk offset must not alias another block's run NodeId"
        );
    }

    #[test]
    fn set_child_position_in_set_numbers_a_paragraph() {
        let mut b = AccessNodeBuilder::for_widget(fake_widget(1));
        let para = b.push_paragraph_child(5);
        assert!(b.set_child_position_in_set(para, 42, 200));
        assert!(
            !b.set_child_position_in_set(NodeId(999), 1, 1),
            "an unknown child is not found"
        );

        let (_id, _n, children) = b.build(fake_widget(1));
        let p = children
            .iter()
            .find(|(i, _)| *i == para)
            .map(|(_, n)| n)
            .unwrap();
        assert_eq!(p.position_in_set(), Some(42));
        assert_eq!(p.size_of_set(), Some(200));
    }

    #[test]
    fn synthetic_node_id_stable_across_calls() {
        // Same (widget, element, kind) produces identical NodeIds —
        // this stability is required for screen-reader focus to survive
        // accessibility rebuilds.
        let wid = fake_widget(42);
        let a = synthetic_node_id(wid, 17, SyntheticKind::TextRun);
        let b = synthetic_node_id(wid, 17, SyntheticKind::TextRun);
        assert_eq!(a, b);
    }

    #[test]
    fn synthetic_node_id_differs_by_kind() {
        let wid = fake_widget(42);
        let p = synthetic_node_id(wid, 17, SyntheticKind::Paragraph);
        let r = synthetic_node_id(wid, 17, SyntheticKind::TextRun);
        assert_ne!(
            p, r,
            "paragraph and text-run kinds must produce distinct NodeIds"
        );
    }

    #[test]
    fn synthetic_node_id_differs_by_element() {
        let wid = fake_widget(42);
        let a = synthetic_node_id(wid, 1, SyntheticKind::TextRun);
        let b = synthetic_node_id(wid, 2, SyntheticKind::TextRun);
        assert_ne!(a, b);
    }

    #[test]
    fn node_id_to_widget_id_maybe_returns_none_for_synthetic() {
        let wid = fake_widget(42);
        let syn = synthetic_node_id(wid, 17, SyntheticKind::TextRun);
        assert!(node_id_to_widget_id_maybe(syn).is_none());
    }

    #[test]
    fn node_id_to_widget_id_maybe_round_trips_widget_ids() {
        let wid = fake_widget(99);
        let nid = widget_id_to_node_id(wid);
        let back = node_id_to_widget_id_maybe(nid).unwrap();
        assert_eq!(wid, back);
    }

    #[test]
    fn push_paragraph_child_and_text_run_child_emit_synthetic_nodes() {
        let owner = fake_widget(7);
        let mut builder = AccessNodeBuilder::for_widget(owner);
        builder.set_role(Role::MultilineTextInput);

        let para = builder.push_paragraph_child(100);
        let run = builder.push_text_run_child(
            para,
            200,
            0,
            "hello".to_string(),
            vec![1, 1, 1, 1, 1],
            Some(vec![0]),
            None,
            None,
            TextRunAttributes::default(),
        );
        assert!(is_synthetic(para));
        assert!(is_synthetic(run));

        let (_nid, _node, children) = builder.build(owner);
        // Two emitted children: paragraph + text run.
        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|(id, _)| *id == para));
        assert!(children.iter().any(|(id, _)| *id == run));
    }

    #[test]
    fn text_run_attributes_reach_at_node() {
        // Audit G7 / EN 301 549 11.5.2.9: bold / italic / underline /
        // strikethrough formatting on a run is exposed on its TextRun node.
        let owner = fake_widget(9);
        let mut builder = AccessNodeBuilder::for_widget(owner);
        builder.set_role(Role::MultilineTextInput);
        let para = builder.push_paragraph_child(1);
        let run = builder.push_text_run_child(
            para,
            2,
            0,
            "ab".to_string(),
            vec![1, 1],
            None,
            None,
            None,
            TextRunAttributes {
                bold: true,
                italic: true,
                underline: true,
                strikethrough: true,
                ..Default::default()
            },
        );
        let (_nid, _node, children) = builder.build(owner);
        let (_, run_node) = children
            .iter()
            .find(|(id, _)| *id == run)
            .expect("run node");
        assert_eq!(
            run_node.font_weight(),
            Some(700.0),
            "bold folds to font weight 700"
        );
        assert!(run_node.is_italic(), "italic flag set");
        assert!(run_node.underline().is_some(), "underline decoration set");
        assert!(
            run_node.strikethrough().is_some(),
            "strikethrough decoration set"
        );

        // An explicit numeric weight wins over the bold flag.
        let owner2 = fake_widget(10);
        let mut b2 = AccessNodeBuilder::for_widget(owner2);
        b2.set_role(Role::MultilineTextInput);
        let p2 = b2.push_paragraph_child(1);
        let r2 = b2.push_text_run_child(
            p2,
            2,
            0,
            "x".to_string(),
            vec![1],
            None,
            None,
            None,
            TextRunAttributes {
                bold: true,
                font_weight: Some(300),
                ..Default::default()
            },
        );
        let (_, _, kids2) = b2.build(owner2);
        let (_, r2n) = kids2.iter().find(|(id, _)| *id == r2).expect("run2 node");
        assert_eq!(
            r2n.font_weight(),
            Some(300.0),
            "explicit weight wins over bold"
        );
    }

    #[test]
    fn set_text_selection_to_wins_over_self_selection() {
        let owner = fake_widget(3);
        let mut builder = AccessNodeBuilder::for_widget(owner);
        builder.set_role(Role::MultilineTextInput);
        // Emit a paragraph + run so set_text_selection_to has a
        // real synthetic NodeId to target.
        let para = builder.push_paragraph_child(1);
        let run = builder.push_text_run_child(
            para,
            2,
            0,
            "ab".to_string(),
            vec![1, 1],
            None,
            None,
            None,
            TextRunAttributes::default(),
        );
        // Both a self-targeted AND an explicit selection are
        // staged — the explicit one must win.
        builder.set_text_selection_on_self(0, 0);
        builder.set_text_selection_to((run, 0), (run, 2));
        let (_nid, node, _children) = builder.build(owner);
        let sel = node.text_selection().expect("text selection set");
        assert_eq!(sel.focus.node, run);
        assert_eq!(sel.focus.character_index, 2);
    }
}
