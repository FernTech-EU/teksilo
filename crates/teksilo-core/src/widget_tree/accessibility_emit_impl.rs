// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! AccessKit node emission: the walker that turns the widget arena into
//! `accesskit::Node` values — bounds, roles, properties, synthetic children,
//! and the subtree-merge helpers it leans on.

use super::*;

use super::accessibility_impl::to_accesskit_rect;
use crate::accessibility::AccessNodeBuilder;

impl WidgetTree {
    #[allow(clippy::type_complexity)]
    pub(super) fn build_accessibility_tree(
        &self,
    ) -> (
        accesskit::TreeUpdate,
        std::collections::HashMap<accesskit::NodeId, WidgetId>,
        std::collections::HashMap<accesskit::NodeId, teksilo_canvas::Rect>,
    ) {
        use crate::accessibility::{root_node_id, widget_id_to_node_id};

        let roots = self.arena.roots();
        let mut nodes: Vec<(accesskit::NodeId, accesskit::Node)> = Vec::new();
        let mut synthetic_parents: std::collections::HashMap<accesskit::NodeId, WidgetId> =
            std::collections::HashMap::new();
        // Global deduplication: AccessKit's consumer panics if the same child
        // NodeId appears in more than one node's children list across a TreeUpdate.
        // Track which widget first claimed each child so we can skip duplicates
        // and emit a diagnostic pointing at the two conflicting parents.
        let mut seen_children: std::collections::HashMap<accesskit::NodeId, WidgetId> =
            std::collections::HashMap::new();
        // Where each synthetic child sits inside its owner, for the ones
        // that declared local bounds. A pure translation re-places them
        // from here without walking again.
        let mut local_bounds: std::collections::HashMap<accesskit::NodeId, teksilo_canvas::Rect> =
            std::collections::HashMap::new();

        let mut root = accesskit::Node::new(accesskit::Role::Window);
        // Every rectangle below is logical; AccessKit wants physical. One
        // transform here converts the whole tree, synthetic children included.
        // See [`super::accessibility_impl::emit_root_transform`].
        super::accessibility_impl::emit_root_transform(&mut root, self.device_scale_factor());
        // Tag the root with the app's current locale (BCP-47, e.g. "fr-FR").
        // AccessKit nodes inherit `language` from their ancestors, so setting
        // it once on the Window node propagates to the whole tree. Without it,
        // VoiceOver/Narrator have no language hint and fall back to a default
        // (often English) TTS voice instead of the user's system voice. The
        // locale is fed in by the app layer via `WidgetTree::set_locale`.
        if let Some(locale) = self.locale_signal.get() {
            root.set_language(locale);
        }
        for &root_id in &roots {
            if self.arena.is_active(root_id) {
                let child_nid = widget_id_to_node_id(root_id);
                if seen_children.insert(child_nid, root_id).is_none() {
                    root.push_child(child_nid);
                } else {
                    eprintln!(
                        "Teksilo bug: duplicate accessibility child {:?} in Window root — \
                         already claimed by another parent. Please file a bug report.",
                        root_id
                    );
                }
            }
        }
        // The framework's own live regions, last in the root's child list so
        // they sit after the application's content in reading order. Both are
        // always present and almost always hidden; see [`crate::announcer`] for
        // why an announcement is delivered by putting a node back into the
        // filtered tree rather than by editing a label in place.
        let announcer_nodes = [
            self.announcer_polite.node(),
            self.announcer_assertive.node(),
        ];
        for (id, _) in &announcer_nodes {
            root.push_child(*id);
        }

        nodes.push((root_node_id(), root));
        nodes.extend(announcer_nodes);

        // One pass, one set: a tooltip is placed once, on the first node that
        // may have it, and the walk order makes that the owner rather than the
        // anchor.
        let mut handled_tooltips: std::collections::HashSet<WidgetId> =
            std::collections::HashSet::new();
        for &root_id in &roots {
            self.build_accessibility_recursive(
                root_id,
                &mut nodes,
                &mut synthetic_parents,
                &mut local_bounds,
                &mut seen_children,
                &mut handled_tooltips,
            );
        }

        let focus = self
            .focused
            .filter(|id| self.arena.is_active(*id))
            .map(widget_id_to_node_id)
            .unwrap_or_else(root_node_id);

        // ── Name-from-content for row nodes ───────────────────────────
        // A virtualized row is two nodes: a thin structural wrapper
        // (`ListItemWrapper` / `TreeItemWrapper`) carrying the role, level
        // and selected/expanded state, and the app's delegate widget below
        // it carrying the visible label. ARIA computes an `option`'s /
        // `treeitem`'s name from its contents; AccessKit does not. Without
        // this pass the platform adapters announce a nameless "tree item",
        // and any client that matches a role AND a label — a screen-reader
        // search, the automation bridge's `find_node` — can never match a
        // row, because the two live on different nodes.
        //
        // Fill each nameless row node's name in from its first named
        // descendant, in the order they read. The descendant keeps its own
        // name (as in a browser's accessibility tree, where the text that
        // contributes to a computed name stays in the tree).
        {
            use accesskit::Role;
            use std::collections::HashMap;

            // Row roles that take their name from content. A row inside a
            // row (never emitted today, but cheap to be correct about) owns
            // its own name and terminates the search.
            fn names_from_content(role: Role) -> bool {
                matches!(role, Role::TreeItem | Role::ListBoxOption)
            }

            let index: HashMap<accesskit::NodeId, usize> = nodes
                .iter()
                .enumerate()
                .map(|(i, (nid, _))| (*nid, i))
                .collect();

            let mut hoisted: Vec<(usize, String)> = Vec::new();
            for (i, (_, node)) in nodes.iter().enumerate() {
                if !names_from_content(node.role()) || node.label().is_some() {
                    continue;
                }
                // Depth-first, children in document order.
                let mut stack: Vec<accesskit::NodeId> =
                    node.children().iter().rev().copied().collect();
                while let Some(nid) = stack.pop() {
                    let Some(&j) = index.get(&nid) else { continue };
                    let descendant = &nodes[j].1;
                    if names_from_content(descendant.role()) {
                        continue;
                    }
                    match descendant.label() {
                        Some(label) if !label.trim().is_empty() => {
                            hoisted.push((i, label.to_string()));
                            break;
                        }
                        _ => stack.extend(descendant.children().iter().rev().copied()),
                    }
                }
            }
            for (i, label) in hoisted {
                nodes[i].1.set_label(label);
            }
        }

        // ── Presentational-node pruning ───────────────────────────────
        // Layout primitives (HStack/VStack/ZStack/Center/Padding/Expand/…)
        // emit empty `GenericContainer` / `Unknown` AT nodes purely to
        // carry visual structure. VoiceOver announces a `GenericContainer`
        // as "group", so a Button whose chrome is composed from these
        // primitives reads as "<label>, button, group". Browsers collapse
        // such semantically-empty nodes out of the platform tree
        // ("ignored" / "presentational" nodes); do the same — drop each
        // empty container and PROMOTE its children to its parent (bounds
        // are absolute, so promotion is structural only).
        //
        // A node is prunable only if it is a visible, content-free
        // `GenericContainer`/`Unknown`: no name, value, live region,
        // popup, relation, or focus/click action. The Window root, the
        // focused node, and any relationship target are always kept.
        {
            use std::collections::{HashMap, HashSet};

            // Nodes referenced by another node's relations must survive so
            // the reference can't dangle.
            let mut relation_targets: HashSet<accesskit::NodeId> = HashSet::new();
            for (_, node) in &nodes {
                relation_targets.extend(node.controls());
                relation_targets.extend(node.described_by());
                relation_targets.extend(node.labelled_by());
            }

            let prunable: HashSet<accesskit::NodeId> = nodes
                .iter()
                .filter(|(nid, node)| {
                    *nid != root_node_id()
                        && *nid != focus
                        && !relation_targets.contains(nid)
                        && is_presentational_container(node)
                })
                .map(|(nid, _)| *nid)
                .collect();

            if !prunable.is_empty() {
                // Pre-pruning children lists, for chain resolution.
                let children_map: HashMap<accesskit::NodeId, Vec<accesskit::NodeId>> = nodes
                    .iter()
                    .map(|(nid, node)| (*nid, node.children().to_vec()))
                    .collect();

                // A kept node's effective children: each prunable child is
                // replaced by its own (recursively resolved) kept children,
                // so chains of empty containers collapse in one pass. The
                // AT tree is acyclic, so the memo is the only guard needed.
                fn resolve(
                    nid: accesskit::NodeId,
                    children_map: &HashMap<accesskit::NodeId, Vec<accesskit::NodeId>>,
                    prunable: &HashSet<accesskit::NodeId>,
                    memo: &mut HashMap<accesskit::NodeId, Vec<accesskit::NodeId>>,
                ) -> Vec<accesskit::NodeId> {
                    if let Some(cached) = memo.get(&nid) {
                        return cached.clone();
                    }
                    let mut out = Vec::new();
                    if let Some(kids) = children_map.get(&nid) {
                        for &c in kids {
                            if prunable.contains(&c) {
                                out.extend(resolve(c, children_map, prunable, memo));
                            } else {
                                out.push(c);
                            }
                        }
                    }
                    memo.insert(nid, out.clone());
                    out
                }

                let mut memo: HashMap<accesskit::NodeId, Vec<accesskit::NodeId>> = HashMap::new();
                for (nid, node) in &mut nodes {
                    if prunable.contains(nid) {
                        continue;
                    }
                    let resolved = resolve(*nid, &children_map, &prunable, &mut memo);
                    if children_map.get(nid) != Some(&resolved) {
                        node.set_children(resolved);
                    }
                }
                nodes.retain(|(nid, _)| !prunable.contains(nid));
            }
        }

        // Strip relationship targets (controls, described_by, labelled_by) that
        // reference NodeIds absent from the emitted tree. Dormant widgets (e.g.
        // inactive tab panels) are excluded from the TreeUpdate; if a node still
        // holds a `push_controlled` / `push_described_by` / `push_labelled_by`
        // reference to one of them, accesskit_macos will unwrap() it and panic
        // when VoiceOver follows the linked_ui_elements attribute.
        //
        // `labelled_by` is the one that costs a name rather than a link: the
        // consumer concatenates its targets' values to build the node's name
        // (`accesskit_consumer::node::write_label`), and a dangling target
        // panics the iterator before it gets there.
        let emitted: std::collections::HashSet<accesskit::NodeId> =
            nodes.iter().map(|(id, _)| *id).collect();
        for (_, node) in &mut nodes {
            let controlled: Vec<_> = node
                .controls()
                .iter()
                .filter(|id| emitted.contains(*id))
                .copied()
                .collect();
            if controlled.len() != node.controls().len() {
                node.set_controls(controlled);
            }
            let described: Vec<_> = node
                .described_by()
                .iter()
                .filter(|id| emitted.contains(*id))
                .copied()
                .collect();
            if described.len() != node.described_by().len() {
                node.set_described_by(described);
            }
            let labelled: Vec<_> = node
                .labelled_by()
                .iter()
                .filter(|id| emitted.contains(*id))
                .copied()
                .collect();
            if labelled.len() != node.labelled_by().len() {
                node.set_labelled_by(labelled);
            }
        }

        (
            accesskit::TreeUpdate {
                nodes,
                tree: Some(accesskit::TreeInfo::new(root_node_id())),
                tree_id: accesskit::TreeId::ROOT,
                focus,
            },
            synthetic_parents,
            local_bounds,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_accessibility_recursive(
        &self,
        id: WidgetId,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        synthetic_parents: &mut std::collections::HashMap<accesskit::NodeId, WidgetId>,
        local_bounds: &mut std::collections::HashMap<accesskit::NodeId, teksilo_canvas::Rect>,
        seen_children: &mut std::collections::HashMap<accesskit::NodeId, WidgetId>,
        // Tooltips whose text has already been placed on a node this pass.
        // One tooltip describes one control, so once an owner has taken it the
        // anchor further down must not take it again -- a description announced
        // twice on the way into a control is worse than one announced in the
        // wrong place.
        handled_tooltips: &mut std::collections::HashSet<WidgetId>,
    ) {
        use crate::accessibility::widget_id_to_node_id;
        use crate::widget_builder::AccessSubtreeMode;

        if !self.arena.is_active(id) {
            return;
        }

        let node = self.arena.get(id).expect("widget id is active in arena");
        let mut builder = AccessNodeBuilder::for_widget(id);
        node.widget.accessibility(&mut builder);
        // Same call, same place, as `build_overridden_builder` — the two builder
        // paths are parallel by construction and a derivation added to one of them
        // only is a node that reads differently to a test than to a screen reader.
        self.announce_context_menu(id, &mut builder);
        self.announce_focusable(id, &mut builder);

        // Apply builder-level overrides AFTER the inner widget has
        // emitted its defaults, so the overrides win for scalar fields
        // and append on relationship lists.
        if let Some(ov) = node.access_overrides.as_deref() {
            ov.apply(&mut builder);
            // `access_shortcut_id` resolution happens here in the
            // walker (not in `apply()`) because the override struct
            // can't reach the tree's `ShortcutRegistry`. Same
            // mechanism as `MenuItem::for_shortcut(...)` — look up
            // the effective primary keystroke and announce it via
            // `KeyStroke`'s `Display` impl. Falls back silently if
            // the id has no registered default.
            if let Some(ref id) = ov.shortcut_id
                && let Some(eff) = self.shortcut_registry.effective(id)
                && let Some(ks) = eff.primary
            {
                builder.set_keyboard_shortcut(ks.to_string());
            }
        }

        let subtree_mode = node.access_subtree;
        // G17: a widget may present a different child ORDER to assistive tech
        // than its paint / z-order child order — e.g. TableView / TreeTableView
        // build body rows before the header for correct z-stacking, but the
        // header must read first in the AT (and Tab) linear order. Honour the
        // widget's `accessibility_children()` override when it provides one;
        // otherwise use the arena's paint-order child list. Geometry and paint
        // are untouched (they keep reading `arena.children`).
        let at_children_owned;
        let children: &[WidgetId] = match node.widget.accessibility_children() {
            Some(v) => {
                at_children_owned = v;
                &at_children_owned
            }
            None => self.arena.children(id),
        };

        // Subtree dispatch:
        //   Inherit  — push child NodeIds onto the parent and recurse normally
        //   Exclude  — neither push nor recurse: descendants vanish from AT
        //   Merge    — collect descendants' label/description/value/actions
        //              into THIS node, then prune (no push, no recurse)
        match subtree_mode {
            AccessSubtreeMode::Inherit => {
                for &child_id in children {
                    if self.arena.is_active(child_id) {
                        let child_nid = widget_id_to_node_id(child_id);
                        // AT-redirect hook (scene logical-tree auto-graft):
                        // walk up the arena from `id` asking every
                        // opted-in ancestor whether it claims this
                        // descendant. First `Some(_)` wins, scanned
                        // bottom-up so closest ancestor takes
                        // priority. The immediate parent is queried
                        // first if it opts in — direct-child
                        // relocation is the special case of an
                        // ancestor walk of length zero.
                        //
                        // Performance: most widgets default
                        // `wants_descendant_redirects = false` and
                        // are skipped without calling the hook, so
                        // the walk is O(opted-in ancestors) per
                        // child push, typically 0 or 1 for a
                        // SceneView-rooted subtree.
                        if self.ancestor_chain_redirects(id, child_id) {
                            // Still record so a sibling can't
                            // double-claim the same descendant.
                            seen_children.insert(child_nid, id);
                            continue;
                        }
                        if let Some(&prior_parent) = seen_children.get(&child_nid) {
                            eprintln!(
                                "Teksilo bug: duplicate accessibility child {:?}: \
                                 first claimed by parent {:?}, now also claimed by {:?}. \
                                 Please file a bug report.",
                                child_id, prior_parent, id
                            );
                            continue;
                        }
                        seen_children.insert(child_nid, id);
                        builder.inner_mut().push_child(child_nid);
                    }
                }
            }
            AccessSubtreeMode::Exclude => {
                // No children pushed, no descendants recursed-into.
            }
            AccessSubtreeMode::Merge => {
                merge_descendants_into(&mut builder, id, &self.arena);
            }
        }

        let bounds = self.arena.bounds(id);
        builder.inner_mut().set_bounds(to_accesskit_rect(bounds));

        // Framework-driven disabled gate. Respects an
        // `access_disabled(false)` override that wants to clear
        // arena-driven disabled state too — without this short-circuit,
        // `clear_disabled()` in the override layer would be re-set here.
        let force_clear_disabled =
            node.access_overrides.as_deref().and_then(|ov| ov.disabled) == Some(false);
        if !self.arena.is_enabled(id) && !force_clear_disabled {
            builder.set_disabled();
        }

        // ── the tooltip's text, onto the node that will be read ───────────
        //
        // Not necessarily the node the overlay hangs off. A composing control
        // anchors the tooltip on an inner chrome node -- the thing with the
        // right bounds to open against -- and keeps its role, its name and its
        // focusability on its own outer node. Emitting on the anchor put the
        // description on an unnamed box beside the control: present in the
        // tree, attached to nothing anyone reads. Since a plain tooltip is
        // never auto-shown on focus (see `docs/tooltips.md`), that description
        // IS the whole non-pointer path for the tier, and it reached nobody.
        //
        // So a tooltip names an owner, and the owner is honoured here -- but
        // only where exactly ONE tooltip claims it. `BuildContext` records the
        // widget that was building, and one build can attach many tooltips: a
        // list body pane attaches one per visible row, all of them naming the
        // pane. Granting that would put one row's text on the pane and lose
        // every other row's entirely. A contested claim is no claim, and each
        // of those tooltips falls back to its own anchor, which is where they
        // already were.
        if let Some(content_id) =
            self.tooltip_description_target(id, handled_tooltips, &mut builder)
        {
            if self
                .tooltips
                .iter()
                .any(|t| t.content_id == content_id && t.overlay_id.is_some())
            {
                // Shown: the content node is live in the AT tree, so the
                // richer `described_by` relation can point straight at it.
                builder
                    .inner_mut()
                    .push_described_by(widget_id_to_node_id(self.tooltip_content_node(content_id)));
                handled_tooltips.insert(content_id);
            } else if let Some(text) = self.tooltip_access_description(content_id) {
                // Not shown. `described_by` cannot be used — the content is a
                // dormant node and absent from the AT tree — and a *plain*
                // tooltip is never auto-shown on focus, so gating the relation
                // on `overlay_id` left the whole tier (the majority of call
                // sites) with no screen-reader path at all. Copy the text onto
                // the control as a static description instead, which is what a
                // keyboard-only or screen-reader user actually reaches.
                builder.set_description(text);
                handled_tooltips.insert(content_id);
            }
        }

        // The fourth element — the widget-local rects of any synthetic
        // children — is what lets a pure translation re-place the runs
        // without a full walk. Recorded by `sync_accessibility`.
        let (node_id, ak_node, synthetic_children, synthetic_local_bounds) = builder.build(id);
        nodes.push((node_id, ak_node));
        local_bounds.extend(synthetic_local_bounds);
        // Merge the widget's emitted synthetic children into the
        // tree update and record their parent-widget mapping so
        // `handle_accessibility_actions` can route incoming
        // `ActionRequest`s targeting these child NodeIds back to
        // the owning widget.
        for (syn_id, syn_node) in synthetic_children {
            nodes.push((syn_id, syn_node));
            synthetic_parents.insert(syn_id, id);
        }

        // Recurse only for `Inherit` — `Exclude` and `Merge` prune
        // descendants from the AT tree.
        if matches!(subtree_mode, AccessSubtreeMode::Inherit) {
            for &child_id in children {
                self.build_accessibility_recursive(
                    child_id,
                    nodes,
                    synthetic_parents,
                    local_bounds,
                    seen_children,
                    handled_tooltips,
                );
            }
        }
    }

    /// Walk the arena from `parent_id` up through ancestors, asking
    /// each opted-in widget whether it claims `descendant` via
    /// [`Widget::a11y_redirect_descendant`]. First widget that
    /// returns `Some(_)` wins (closest-ancestor-first scan). Returns
    /// `true` if any ancestor claimed the descendant — the caller
    /// then skips the default child-list push.
    ///
    /// Cost: bounded by arena depth, but most widgets default
    /// `wants_descendant_redirects = false` and short-circuit
    /// without invoking the redirect hook itself. Trees with no
    /// opted-in ancestors pay one `is_active` + one `Widget::
    /// wants_descendant_redirects` call per ancestor — both
    /// trivial — and walk to root.
    ///
    /// `parent_id` is queried *first* — direct-child relocation is
    /// just the special case of an ancestor walk of length one.
    fn ancestor_chain_redirects(&self, parent_id: WidgetId, descendant: WidgetId) -> bool {
        let mut current = Some(parent_id);
        while let Some(curr) = current {
            let Some(curr_node) = self.arena.get(curr) else {
                break;
            };
            if curr_node.widget.wants_descendant_redirects()
                && curr_node
                    .widget
                    .a11y_redirect_descendant(curr, descendant)
                    .is_some()
            {
                return true;
            }
            current = self.arena.parent(curr);
        }
        false
    }

    /// Build a builder representing the widget's full a11y state at this
    /// instant — the inner widget's `accessibility(builder)` plus any
    /// builder-level overrides (`access_label`, `access_role`, …) and,
    /// when the widget has `access_subtree(Merge)`, the merged
    /// descendant state. Centralized so `accessibility_node`,
    /// `text_content`, and the recursive walker stay in sync.
    /// Which tooltip's text this node should carry, if any.
    ///
    /// Two ways a node can come to own a tooltip's description, tried in that
    /// order:
    ///
    ///   * **as the claimed owner** -- the widget that was building when the
    ///     tooltip was attached. This is the case the whole mechanism exists
    ///     for: `Button` and the two dozen controls shaped like it hang the
    ///     overlay off an inner chrome node while their role and name live
    ///     out here. Granted only when this node is *unambiguously* the
    ///     owner: exactly one tooltip may claim it, and the node must be
    ///     something an assistive technology would actually stop on rather
    ///     than an anonymous box.
    ///
    ///   * **as the anchor** -- the historic behaviour, and still correct for
    ///     a widget that anchors its tooltip on itself. Also the fallback for
    ///     everything the first case declines, so declining is always safe:
    ///     the worst it can do is leave the description exactly where it was
    ///     before any of this existed.
    ///
    /// The order matters and only works because of the walk's order. An
    /// anchor is always a descendant of the widget that built it, and parents
    /// are visited first, so an owner has already taken its tooltip (and said
    /// so in `handled`) by the time the anchor is reached. Reverse the walk
    /// and both would take it.
    fn tooltip_description_target(
        &self,
        id: WidgetId,
        handled: &std::collections::HashSet<WidgetId>,
        builder: &mut AccessNodeBuilder,
    ) -> Option<WidgetId> {
        let mut claims = self
            .tooltips
            .iter()
            .filter(|t| t.description_owner_id == id && !handled.contains(&t.content_id));
        // `next()` twice rather than `count()`: one claim is the answer, two is
        // a refusal, and there is nothing to learn from a third.
        if let (Some(only), None) = (claims.next(), claims.next())
            && only.anchor_id != id
            // An anonymous container is not a place a description can be read
            // from, and a widget whose own `accessibility()` leaves it one
            // (`TextInput` says so out loud, keeping the real role on an inner
            // field) is not the control being described either. Nothing is
            // lost by declining: the anchor still takes it below.
            && !is_presentational_container(builder.inner_mut())
            // A description the widget wrote itself is the widget's own words
            // about itself; a tooltip's is supplementary. Both land in the one
            // scalar field, so the specific one wins. `MenuItem::trailing_hint`
            // is the case that made this reachable.
            && builder.inner_mut().description().is_none()
        {
            return Some(only.content_id);
        }
        self.tooltips
            .iter()
            .find(|t| t.anchor_id == id && !handled.contains(&t.content_id))
            .map(|t| t.content_id)
    }

    /// Announce the context menu a widget owns, before any override runs.
    ///
    /// The dispatcher has always *serviced* `Action::ShowContextMenu` by falling
    /// through to the node's `.context_menu(..)` factory, so an assistive
    /// technology that tried the action got a menu. Nothing ever told it the
    /// action was there, and an AT offers what a node advertises: the menu was
    /// reachable and undiscoverable at the same time.
    ///
    /// That is not a cosmetic gap wherever a menu is the accessible route to
    /// something else. A row that puts its actions on hover buttons has to hide
    /// those buttons from the AT -- a control that exists only under a pointer
    /// does not exist for a keyboard -- and offer the same actions on its context
    /// menu instead. Unannounced, that leaves the actions with no route at all.
    ///
    /// Only where the node owns the factory ITSELF, not wherever the ancestor walk
    /// would eventually find one: every descendant of a widget with a menu would
    /// otherwise advertise it, and an AT would offer the same menu on a dozen
    /// nested boxes. Applied BEFORE the overrides so `access_remove_action` still
    /// takes it away, and so a widget wiring its own `on_access_action` handler is
    /// not given a second one.
    /// Announce that a focusable node can be focused, before any override runs.
    ///
    /// The sibling of [`Self::announce_context_menu`], for the same reason. The
    /// dispatcher has always *serviced* `Action::Focus` for any node -- the
    /// `AccessAction` arm calls `focus_with_origin_ops` itself rather than
    /// handing the action to the widget -- so an assistive technology that tried
    /// it got focus. Nothing told it the action was there, and an AT offers what
    /// a node advertises: focus was reachable and undiscoverable at the same
    /// time.
    ///
    /// Leaving it to each widget made it a rule that had to be remembered ~40
    /// times and was not: every stock button remembered, while `ListView`,
    /// `TreeView`, `TableView`, `TreeTableView`, `GridView`, `MenuList` and
    /// `OverlayTrigger` did not -- so a screen reader could not put focus in a
    /// list, a tree, a table, a grid or an open menu. Deriving it from the
    /// arena's own focusable flag makes the advertisement true by construction
    /// and keeps it true for widgets not yet written.
    ///
    /// Gated on the *arena* flag rather than on the widget's opinion, because
    /// that flag is what `focus_with_origin_ops` will actually honour -- a node
    /// that is not focusable would advertise an action that then declines to
    /// land. Applied BEFORE the overrides, so `access_remove_action` can still
    /// take it away.
    fn announce_focusable(&self, id: WidgetId, builder: &mut AccessNodeBuilder) {
        if self
            .arena
            .get(id)
            .is_some_and(|node| self.is_node_focusable(node))
            && !builder.actions().contains(&accesskit::Action::Focus)
        {
            builder.add_action(accesskit::Action::Focus);
        }
    }

    fn announce_context_menu(&self, id: WidgetId, builder: &mut AccessNodeBuilder) {
        if self
            .arena
            .get(id)
            .is_some_and(|node| node.context_menu_factory.is_some())
            && !builder
                .actions()
                .contains(&accesskit::Action::ShowContextMenu)
        {
            builder.add_action(accesskit::Action::ShowContextMenu);
        }
    }

    /// Whether the node `id` publishes to assistive technology offers
    /// `Action::Focus`.
    ///
    /// Answered off the same builder the AT tree is emitted from — widget
    /// emission, then `announce_focusable`, then the app's overrides (which can
    /// both add an action and `access_remove_action` one) — so it is exactly
    /// what the assistive technology was told.
    ///
    /// This is the gate on the `Action::Focus` walk into a subtree. A focusable
    /// leaf gets the action from `announce_focusable` and resolves to itself; a
    /// composite that keeps focus on an inner leaf — `SpinBox`, `ComboBox`,
    /// `DateEdit` and its siblings — adds the action in its own
    /// `accessibility()` and means the walk. Everything else — a `Panel`, a
    /// `GroupBox`, a landmark, a label — offers no `Focus`, and moving focus
    /// into it would land the keyboard on a descendant the assistive technology
    /// could have named itself and did not.
    pub(crate) fn advertises_focus_action(&self, id: WidgetId) -> bool {
        self.arena.get(id).is_some()
            && self
                .build_overridden_builder(id)
                .actions()
                .contains(&accesskit::Action::Focus)
    }

    pub(super) fn build_overridden_builder(&self, id: WidgetId) -> AccessNodeBuilder {
        use crate::widget_builder::AccessSubtreeMode;
        let node = self.arena.get(id).expect("widget id is active in arena");
        // Every consumer of this builder reads scalars off it — role, name,
        // actions, the tri-state flags — and discards it. A name probe so a
        // text-bearing widget does not shape and emit runs nobody collects.
        let mut builder = AccessNodeBuilder::for_name_probe(id);
        node.widget.accessibility(&mut builder);
        self.announce_context_menu(id, &mut builder);
        self.announce_focusable(id, &mut builder);
        if let Some(ov) = node.access_overrides.as_deref() {
            ov.apply(&mut builder);
            // Resolve `access_shortcut_id` against the live registry —
            // see the matching block in `build_accessibility_recursive`.
            if let Some(ref sid) = ov.shortcut_id
                && let Some(eff) = self.shortcut_registry.effective(sid)
                && let Some(ks) = eff.primary
            {
                builder.set_keyboard_shortcut(ks.to_string());
            }
        }
        if node.access_subtree == AccessSubtreeMode::Merge {
            merge_descendants_into(&mut builder, id, &self.arena);
        }
        builder
    }
}

/// Whether an AT node is a purely-structural container that should be
/// collapsed out of the tree (its children promoted to its parent).
///
/// True only for a content-free `GenericContainer` / `Unknown` node — the
/// empty boxes layout primitives (HStack/VStack/Padding/Expand/…) emit.
/// The check is exhaustive by construction: set aside the framework-applied
/// children, bounds, and disabled flag, then compare against a fresh
/// default node of the same role. Any author- or widget-set property —
/// name, value, description, orientation, aria-current, identifier, live
/// region, popup, action, relation, hidden flag, … — makes the node differ
/// from the default and keeps it, so no semantic property can be missed.
/// Callers additionally exempt the Window root, the focused node, and
/// relationship targets.
fn is_presentational_container(node: &accesskit::Node) -> bool {
    use accesskit::Role;
    if !matches!(node.role(), Role::GenericContainer | Role::Unknown) {
        return false;
    }
    // Set aside the framework-applied structural bits (children, bounds,
    // arena-driven disabled flag), then check the node carries no semantic
    // content by comparing against a bare node of the same role.
    //
    // We compare the *Debug* form, not `==`: AccessKit's `clear_*` leaves
    // residue in the private property-value vec (the index is unset but the
    // value stays), so `PartialEq` never matches a fresh node. `Node`'s
    // Debug renders only logically-set properties, so it reflects true
    // content and is exhaustive — any author/widget property keeps the node.
    let mut probe = node.clone();
    probe.clear_children();
    probe.clear_bounds();
    probe.clear_disabled();
    format!("{probe:?}") == format!("{:?}", accesskit::Node::new(node.role()))
}

// ─────────────────────────────────────────────────────────────────────
// Subtree-merge helpers
// ─────────────────────────────────────────────────────────────────────
//
// These run when a widget's `access_subtree` is `Merge`. The walker
// recurses through the descendants, applies each descendant's own
// `accessibility() + override apply()` into a temp builder, and absorbs
// the resulting label / description / value / actions / relationships
// into a `MergeAccumulator`. After the walk finishes the accumulator
// flushes its accumulated state onto the parent's builder.
//
// The accumulator deliberately discards descendant role and numeric
// fields (parent's role wins for the merged element) and discards
// hidden / disabled (parent's state governs the whole merged subtree).
// Action lists union with deduplication so two child Buttons each
// emitting `Click` don't pollute the merged parent with two copies.

/// Walk the descendants of `parent_id` and absorb their label /
/// description / value / actions / relationships into `parent_builder`.
/// Per-descendant subtree-mode handling lives in
/// [`merge_collect_recursive`].
fn merge_descendants_into(
    parent_builder: &mut AccessNodeBuilder,
    parent_id: WidgetId,
    arena: &crate::arena::WidgetArena,
) {
    let mut acc = MergeAccumulator::default();
    for &child in arena.children(parent_id) {
        merge_collect_recursive(child, arena, &mut acc);
    }
    acc.flush_into(parent_builder);
}

fn merge_collect_recursive(
    id: WidgetId,
    arena: &crate::arena::WidgetArena,
    acc: &mut MergeAccumulator,
) {
    use crate::widget_builder::AccessSubtreeMode;

    if !arena.is_active(id) {
        return;
    }
    let Some(node) = arena.get(id) else {
        return;
    };

    // Build a temp builder for this descendant the same way the walker
    // would: widget.accessibility() then override apply(). This means
    // a descendant's `.access_label(...)` contributes its resolved
    // override string, not its raw widget label.
    // A name probe: `absorb` takes the descendant's name, value, actions and
    // relations, never its synthetic children — and the runs a text widget
    // would emit here would be discarded with `tmp` while the widget's own
    // node keeps its copies.
    let mut tmp = AccessNodeBuilder::for_name_probe(id);
    node.widget.accessibility(&mut tmp);
    if let Some(ov) = node.access_overrides.as_deref() {
        ov.apply(&mut tmp);
    }
    // Nested-merge: the descendant itself has access_subtree=Merge.
    // Run its own merge into `tmp` BEFORE absorbing — otherwise we'd
    // absorb the descendant's empty container state and lose its
    // subtree-merged label.
    if matches!(node.access_subtree, AccessSubtreeMode::Merge) {
        merge_descendants_into(&mut tmp, id, arena);
    }
    // Skip nodes that opted out of AT entirely: a child marked
    // `access_hidden(true)` (or whose widget called `set_hidden()`)
    // contributes nothing to the merge.
    if !tmp.is_hidden() {
        acc.absorb(&tmp);
    }

    match node.access_subtree {
        AccessSubtreeMode::Exclude => {
            // Prune — don't recurse into descendants of an excluded subtree.
        }
        AccessSubtreeMode::Merge => {
            // Nested Merge: descendant's own subtree was absorbed into
            // `tmp` above; don't re-walk its children at this level
            // (would double-count). The descendant reads as one
            // AT element from the outer merge's perspective.
        }
        AccessSubtreeMode::Inherit => {
            for &grandchild in arena.children(id) {
                merge_collect_recursive(grandchild, arena, acc);
            }
        }
    }
}

/// Per-merge-walk accumulator. Collects descendant state across the
/// recursive walk, then `flush_into` writes the unioned values onto
/// the parent's builder.
///
/// Fields that the absorb path can read from `AccessNodeBuilder`
/// (label, value, advertised actions) participate in the merge.
/// Description and relationship lists (`controls` / `described_by` /
/// `labelled_by`) live inside `accesskit::Node` and have no public
/// getter on `AccessNodeBuilder`; merging them would require
/// reflecting builder mutations into a parallel field, which we
/// haven't found a real-world use case for. App authors who need
/// description / relationship merge can use the `access_customize`
/// escape hatch on the parent.
#[derive(Default)]
struct MergeAccumulator {
    label_parts: Vec<String>,
    /// First non-empty value wins.
    value: Option<String>,
    actions: Vec<accesskit::Action>,
}

impl MergeAccumulator {
    fn absorb(&mut self, src: &AccessNodeBuilder) {
        if let Some(name) = src.name()
            && !name.is_empty()
        {
            self.label_parts.push(name.to_string());
        }
        if let Some(value) = src.value()
            && self.value.is_none()
            && !value.is_empty()
        {
            self.value = Some(value.to_string());
        }
        for &action in src.actions() {
            if !self.actions.contains(&action) {
                self.actions.push(action);
            }
        }
    }

    fn flush_into(self, dst: &mut AccessNodeBuilder) {
        // Concatenate new label parts onto whatever the parent's
        // builder already carried. Existing parent name kept first.
        if !self.label_parts.is_empty() {
            let existing = dst.name().map(|s| s.to_string());
            let merged = match existing {
                Some(e) if !e.is_empty() => {
                    let mut s = e;
                    for part in self.label_parts {
                        s.push(' ');
                        s.push_str(&part);
                    }
                    s
                }
                _ => self.label_parts.join(" "),
            };
            dst.set_name(merged);
        }
        if let Some(v) = self.value {
            // Only overwrite parent value if it's currently None — the
            // parent's own value (from the inner widget or override
            // `access_value`) takes precedence.
            if dst.value().is_none() {
                dst.set_value(v);
            }
        }
        for action in self.actions {
            // Union: skip actions already advertised on the parent
            // (avoid duplicate Click/Focus when parent is itself a Button).
            if !dst.actions().contains(&action) {
                dst.add_action(action);
            }
        }
    }
}
