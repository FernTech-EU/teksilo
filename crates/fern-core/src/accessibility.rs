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
    /// the existing [`push_controlled`]; used by the override layer's
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
    /// (e.g. "link, https://example.com"). Informational only — does
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
        (node_id, self.inner, self.children_collected)
    }

    /// Get a reference to the inner node for advanced use.
    pub fn inner_mut(&mut self) -> &mut Node {
        &mut self.inner
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

    /// Override a previously-pushed paragraph child's role to
    /// `Role::Heading` with the given hierarchical level. Used by
    /// the rich text editor when a block carries a
    /// `BlockFormat::heading_level`. Returns `true` if the node was
    /// found and updated, `false` otherwise (caller mis-used the
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

    /// Push a `Role::TextRun` child under `parent_node` (usually a
    /// paragraph NodeId returned from [`push_paragraph_child`], but
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
    /// [`set_text_selection_to`].
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
    ) -> NodeId {
        let Some(owner) = self.owner else {
            debug_assert!(
                false,
                "push_text_run_child called on a builder with no owner — \
                 widgets must only call this from Widget::accessibility"
            );
            return NodeId(0);
        };
        // Mix `fragment_offset` into the element_id bits so sub-runs
        // of a highlight-split source element get unique NodeIds.
        let mixed_element = element_id ^ ((fragment_offset as u64) << 32);
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
        // own node as a fallback. Caller mis-used the API.
        self.inner.push_child(node_id);
        node_id
    }

    /// Declare a text selection that references TextRun children
    /// previously emitted via [`push_text_run_child`]. Both the
    /// anchor and the focus are expressed as
    /// `(NodeId, character_index)` pairs where the character index
    /// is an index into the target TextRun's `character_lengths`
    /// (NOT a document-absolute offset — per AccessKit's contract).
    pub fn set_text_selection_to(
        &mut self,
        anchor: (NodeId, usize),
        focus: (NodeId, usize),
    ) {
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
/// ids to catch mis-routed calls early.
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
        // to a u64 with bit 63 clear. The plan's top-bit namespace
        // split only works if widget-derived NodeIds stay below
        // bit 63.
        let wid = fake_widget(1);
        let nid = widget_id_to_node_id(wid);
        assert_eq!(nid.0 & SYNTHETIC_BIT, 0, "widget NodeId must have bit 63 clear");
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
    fn synthetic_node_id_stable_across_calls() {
        // Same (widget, element, kind) produces identical NodeIds —
        // the plan relies on this for screen-reader focus stability
        // across accessibility rebuilds.
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
        assert_ne!(p, r, "paragraph and text-run kinds must produce distinct NodeIds");
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
