// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

use crate::accessibility::{AccessNodeBuilder, AccessibilityInfo};

impl WidgetTree {
    /// Build an AccessKit `TreeUpdate` from the current state of all active
    /// widgets. Call this once per frame, between layout and paint, and push
    /// the result to the `accesskit_winit::Adapter`.
    /// Caches the result and only rebuilds when layout has changed.
    pub fn sync_accessibility(&mut self) -> accesskit::TreeUpdate {
        // Explicit re-walk request (e.g. `SceneView` materialised / destroyed a
        // scene widget, or an a11y-only scene mutation). A relayout no longer
        // sets `a11y_dirty` on its own, so this is the lever; drain it before
        // the cache check below.
        if self.a11y_update_requested.replace(false) {
            self.a11y_dirty = true;
        }
        // Shortcut registry rebinds bump
        // `ShortcutRegistry::version()`. The `access_shortcut_id`
        // resolution in the walker reads the live registry, so any
        // rebind must invalidate the AT-tree cache too — otherwise
        // a rebind would not surface in the announced shortcut
        // until something else dirties the tree (a layout, a focus
        // change, …). Cheap: one u64 compare per `sync_accessibility`.
        let current_shortcut_version = self.shortcut_registry.version().get();
        if current_shortcut_version != self.last_synced_shortcut_version {
            self.a11y_dirty = true;
            self.last_synced_shortcut_version = current_shortcut_version;
        }

        // Locale switches make `access_label(tr!(...))` (stored as a
        // locale-bound `Prop<String>`) resolve to a new value. The
        // override props are read in `apply()` during the walk, so the
        // tree must re-walk for the new announcement to surface —
        // otherwise the screen reader keeps the old-locale string until
        // something else dirties the tree. Mirror the shortcut-version
        // guard above. (Same-direction switches don't rebuild the
        // composite, so this is the only thing that refreshes AT labels.)
        let current_locale = self.locale_signal.get();
        if current_locale != self.last_synced_locale {
            self.a11y_dirty = true;
            self.last_synced_locale = current_locale;
        }

        // The framework's own live regions. Advancing them here, before the
        // cache check, is what makes a queued announcement able to wake a tree
        // that is otherwise clean: `announce` sets `a11y_update_requested`,
        // which the drain above turned into `a11y_dirty`, and the step below
        // decides what this update will say. Each message costs two updates —
        // one that exposes its node, one that retracts it — so an announcer
        // that is still busy asks for another sync and another frame.
        let announcers_busy = self.step_announcers();

        if !self.a11y_dirty
            && let Some(cached) = &self.cached_a11y
        {
            return cached.clone();
        }

        let (update, parents) = self.build_accessibility_tree();
        // A `&mut self` post-pass: diff the freshly-built live nodes and
        // record any changed text into the announcement ring buffer. Must
        // run here (not in the `&self` `build_accessibility_tree`) and
        // before the cache store, so it sees exactly the update that is
        // about to become canonical.
        self.collect_announcements(&update);
        // Bump the AT version only when the tree's *content* actually changed.
        // A rebuild can be triggered by a shortcut-rebind / locale switch that
        // produces a byte-for-byte identical `TreeUpdate`; bumping then would
        // make `WaitCondition::AtVersionAtLeast` wake on no real change.
        // `TreeUpdate: PartialEq`, so this is a precise (if O(n)) comparison —
        // acceptable since it only runs on a real rebuild, not a cache hit.
        let content_changed = self.cached_a11y.as_ref() != Some(&update);
        self.cached_a11y = Some(update.clone());
        self.synthetic_parent_map = parents;
        self.a11y_dirty = false;
        if content_changed {
            // Mirror of the shortcut-registry version signal; saturating so the
            // documented "monotonic" contract holds even past `u64::MAX`.
            self.at_version.set(self.at_version.get().saturating_add(1));
        }
        if announcers_busy {
            self.request_accessibility_update();
            self.request_frame();
        }
        update
    }

    /// Build a `TreeUpdate` describing the tree right now, without touching any
    /// state.
    ///
    /// For a caller that wants to *look* at the accessibility tree rather than
    /// deliver it: an automation query, a screenshot's blind-spot check, a test
    /// assertion. Unlike [`sync_accessibility`](Self::sync_accessibility) this
    /// neither caches, nor bumps the AT version, nor records announcements, nor
    /// advances the framework's live regions — which matters, because a caller
    /// that consumed an announcer step and then dropped the update would have
    /// silently eaten a message the user was meant to hear.
    ///
    /// It is a full walk every time; `sync_accessibility` is the one with the
    /// cache.
    pub fn accessibility_tree_snapshot(&self) -> accesskit::TreeUpdate {
        self.build_accessibility_tree().0
    }

    /// Advance both announcers one step and report whether either still has
    /// work. See [`crate::announcer`].
    fn step_announcers(&mut self) -> bool {
        // Both are stepped, not just the busy one: an announcer that is Idle
        // and empty returns false and emits the same hidden node it emitted
        // last time, so this costs nothing when nobody is announcing.
        let polite = self.announcer_polite.step();
        let assertive = self.announcer_assertive.step();
        if polite || assertive {
            self.a11y_dirty = true;
        }
        polite || assertive
    }

    /// Diff the live-region nodes of a freshly-built `TreeUpdate` against
    /// the per-node last-announced-text map and push any changes into the
    /// capped announcement ring buffer. See
    /// [`crate::accessibility::Announcement`] and
    /// [`Self::announcements_since`].
    fn collect_announcements(&mut self, update: &accesskit::TreeUpdate) {
        use accesskit::Live;
        let mut seen: std::collections::HashSet<accesskit::NodeId> =
            std::collections::HashSet::with_capacity(self.automation_last_text.len());
        for (node_id, node) in &update.nodes {
            let assertive = match node.live() {
                Some(Live::Polite) => false,
                Some(Live::Assertive) => true,
                // `Live::Off` or unset: not a live region.
                _ => continue,
            };
            // A live region announces its `value` if it has one, else its
            // `label` (matching how the AT layer voices it).
            let text = node
                .value()
                .or_else(|| node.label())
                .unwrap_or("")
                .trim()
                .to_string();
            if text.is_empty() {
                // A cleared live region: forget its last text (and leave it out
                // of `seen`, so `retain` drops it too) so the SAME text
                // reappearing on a later sync is announced as novel rather than
                // deduped against the stale entry.
                self.automation_last_text.remove(node_id);
                continue;
            }
            seen.insert(*node_id);
            let changed = self
                .automation_last_text
                .get(node_id)
                .map(|prev| prev != &text)
                .unwrap_or(true);
            if !changed {
                continue;
            }
            self.automation_last_text.insert(*node_id, text.clone());
            self.automation_announce_seq = self.automation_announce_seq.saturating_add(1);
            if self.automation_announcements.len() >= super::AUTOMATION_ANNOUNCE_CAP {
                self.automation_announcements.pop_front();
            }
            self.automation_announcements
                .push_back(crate::accessibility::Announcement {
                    seq: self.automation_announce_seq,
                    text,
                    assertive,
                });
        }
        // Forget nodes that are no longer present (or no longer live), so a
        // node that reappears with identical text re-announces.
        self.automation_last_text.retain(|k, _| seen.contains(k));
    }

    /// Dispatch a synthetic AccessKit action to the node identified by
    /// `node_id` (which may be a *synthetic*, widget-emitted child node —
    /// e.g. a rich-text `TextRun`). Resolves the owning widget exactly the
    /// way the platform AT adapter does
    /// ([`node_id_to_widget_id_maybe`](crate::accessibility::node_id_to_widget_id_maybe)
    /// then [`widget_for_synthetic`](Self::widget_for_synthetic)), builds a
    /// [`crate::event::WidgetEvent::AccessAction`],
    /// and dispatches it through `ops` so actions that open windows /
    /// dialogs work.
    ///
    /// Returns `true` when the action was **consumed** — a handler claimed it,
    /// focus moved, or the context-menu fallback opened a menu — and `false`
    /// when nothing acted on it, including when the target node resolves to no
    /// live widget. Callers are expected to surface that: an action a node
    /// never handles is a caller error, and reporting it as success (which is
    /// what "a widget existed at the target" amounted to) leaves an automation
    /// client chasing timing and coordinates for a UI that was never going to
    /// move.
    ///
    /// This is the in-process equivalent of an OS screen reader invoking an
    /// action — the channel an [automation](crate::WidgetTree) harness uses
    /// to *drive* the UI without the OS AT layer.
    pub fn dispatch_access_action(
        &mut self,
        node_id: accesskit::NodeId,
        action: accesskit::Action,
        data: Option<accesskit::ActionData>,
        ops: &mut dyn crate::window::WindowOps,
    ) -> bool {
        let target = crate::accessibility::node_id_to_widget_id_maybe(node_id)
            .or_else(|| self.widget_for_synthetic(node_id));
        let event = crate::event::WidgetEvent::AccessAction {
            action,
            target,
            target_node: node_id,
            data,
        };
        self.access_action_handled = false;
        self.dispatch_event_with_ops(event, ops);
        self.access_action_handled
    }

    fn build_accessibility_tree(
        &self,
    ) -> (
        accesskit::TreeUpdate,
        std::collections::HashMap<accesskit::NodeId, WidgetId>,
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

        let mut root = accesskit::Node::new(accesskit::Role::Window);
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

        // Strip relationship targets (controls, described_by) that reference
        // NodeIds absent from the emitted tree. Dormant widgets (e.g. inactive
        // tab panels) are excluded from the TreeUpdate; if a node still holds a
        // `push_controlled` or `push_described_by` reference to one of them,
        // accesskit_macos will unwrap() it and panic when VoiceOver follows the
        // linked_ui_elements attribute.
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
        }

        (
            accesskit::TreeUpdate {
                nodes,
                tree: Some(accesskit::TreeInfo::new(root_node_id())),
                tree_id: accesskit::TreeId::ROOT,
                focus,
            },
            synthetic_parents,
        )
    }

    // (helper `is_presentational_container` is a module-level free fn below)

    /// Look up the owning widget for a synthetic AccessKit `NodeId`
    /// emitted by `push_text_run_child` / `push_paragraph_child`.
    /// Used by `handle_accessibility_actions` to route an
    /// `ActionRequest` targeting a TextRun child back to the
    /// editor that owns it.
    pub fn widget_for_synthetic(&self, node_id: accesskit::NodeId) -> Option<WidgetId> {
        self.synthetic_parent_map.get(&node_id).copied()
    }

    fn build_accessibility_recursive(
        &self,
        id: WidgetId,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        synthetic_parents: &mut std::collections::HashMap<accesskit::NodeId, WidgetId>,
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
        builder.inner_mut().set_bounds(accesskit::Rect {
            x0: bounds.x as f64,
            y0: bounds.y as f64,
            x1: (bounds.x + bounds.width) as f64,
            y1: (bounds.y + bounds.height) as f64,
        });

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

        let (node_id, ak_node, synthetic_children) = builder.build(id);
        nodes.push((node_id, ak_node));
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

    fn build_overridden_builder(&self, id: WidgetId) -> AccessNodeBuilder {
        use crate::widget_builder::AccessSubtreeMode;
        let node = self.arena.get(id).expect("widget id is active in arena");
        let mut builder = AccessNodeBuilder::for_widget(id);
        node.widget.accessibility(&mut builder);
        self.announce_context_menu(id, &mut builder);
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

    /// The text a tooltip would announce, harvested from its (dormant)
    /// content widget so an anchor can carry it as a static AT description
    /// while the tooltip is not shown.
    ///
    /// All three tiers publish their body through `accessibility()` as the
    /// node *name* (`TooltipWidget::accessibility` → `set_name(text)`), so
    /// probing the widget is enough — no parallel copy of the text has to be
    /// threaded through `attach_tooltip*` and kept in sync. Reads at walk
    /// time, so a locale change or a `Signal<String>` swap is picked up by the
    /// same AT re-walk that already tracks them.
    /// Resolve a tooltip's content node past a deferred host.
    ///
    /// A deferred tooltip body answers for itself while un-built (see
    /// `DeferredSubtree::accessibility`), but once the user has hovered it the
    /// host has handed its widget value to a real child and has nothing left to
    /// say. Follow it, or a tooltip's description would be readable exactly
    /// until the first time it was shown.
    pub(crate) fn tooltip_content_node(&self, content_id: WidgetId) -> WidgetId {
        self.arena
            .get(content_id)
            .and_then(|node| node.widget.as_any())
            .and_then(|any| any.downcast_ref::<crate::deferred_subtree::DeferredSubtree>())
            .and_then(|d| d.materialized_child())
            .unwrap_or(content_id)
    }

    pub(crate) fn tooltip_access_description(&self, content_id: WidgetId) -> Option<String> {
        let content_id = self.tooltip_content_node(content_id);
        let node = self.arena.get(content_id)?;
        let mut probe = AccessNodeBuilder::for_widget(content_id);
        node.widget.accessibility(&mut probe);
        if let Some(ov) = node.access_overrides.as_deref() {
            ov.apply(&mut probe);
        }
        probe
            .name()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }

    pub fn accessibility_node(&self, id: WidgetId) -> AccessibilityInfo {
        let node = self.arena.get(id).expect("widget id is active in arena");
        let builder = self.build_overridden_builder(id);
        let role = builder.role();
        let name = builder.name().map(|s| s.to_string());
        let actions = builder.actions().to_vec();
        let mut info = AccessibilityInfo::new(role, name, actions);
        if let Some(toggled) = builder.toggled() {
            info = info.with_toggled(toggled);
        }
        if let Some(expanded) = builder.expanded() {
            info = info.with_expanded(expanded);
        }
        if let Some(selected) = builder.selected() {
            info = info.with_selected(selected);
        }
        // Mirror the framework gate at `build_accessibility_recursive`:
        // arena-driven disabled wins unless the override explicitly
        // asks for `access_disabled(false)`.
        let force_clear_disabled =
            node.access_overrides.as_deref().and_then(|ov| ov.disabled) == Some(false);
        let disabled_arena = !self.arena.is_enabled(id) && !force_clear_disabled;
        // Override `Some(true)` already called `set_disabled()` inside
        // the override apply; we only need to surface it here as a
        // separate signal because `AccessNodeBuilder` doesn't expose a
        // `is_disabled()` getter on the builder side.
        let disabled_override =
            node.access_overrides.as_deref().and_then(|ov| ov.disabled) == Some(true);
        if disabled_arena || disabled_override {
            info = info.with_disabled(true);
        }
        if builder.is_hidden() {
            info = info.with_hidden(true);
        }
        info
    }

    pub fn find_by_role(&self, role: accesskit::Role) -> Option<WidgetId> {
        self.arena
            .active_ids_iter()
            .find(|&id| self.build_overridden_builder(id).role() == role)
    }

    pub fn find_by_label(&self, label: &str) -> Option<WidgetId> {
        self.arena
            .active_ids_iter()
            .find(|&id| self.build_overridden_builder(id).name() == Some(label))
    }

    pub fn find_by_action(&self, action: accesskit::Action) -> Option<WidgetId> {
        self.arena.active_ids_iter().find(|&id| {
            self.build_overridden_builder(id)
                .actions()
                .contains(&action)
        })
    }

    /// Get the text content of a widget from its accessibility name.
    /// Equivalent to the label set via `AccessNodeBuilder::set_name`,
    /// after override application.
    pub fn text_content(&self, id: WidgetId) -> Option<String> {
        self.arena.get(id)?;
        self.build_overridden_builder(id)
            .name()
            .map(|s| s.to_string())
    }

    /// Get the text value of a widget from its accessibility value.
    /// Equivalent to the value set via `AccessNodeBuilder::set_value`,
    /// after override application.
    pub fn text_value(&self, id: WidgetId) -> Option<String> {
        self.arena.get(id)?;
        self.build_overridden_builder(id)
            .value()
            .map(|s| s.to_string())
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
    let mut tmp = AccessNodeBuilder::for_widget(id);
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

#[cfg(test)]
pub(crate) mod test_helpers {
    /// Feed a `TreeUpdate` into `accesskit_consumer::Tree`, which runs the
    /// same validation that every platform AT (VoiceOver, NVDA, …) runs on
    /// activation. Panics on duplicate children, dangling relationship
    /// targets, orphaned nodes, and invalid focus — turning those runtime
    /// crashes into CI failures.
    pub(crate) fn assert_a11y_tree_valid(update: &accesskit::TreeUpdate) {
        accesskit_consumer::Tree::new(update.clone(), false);
    }

    /// Assert that every NodeId referenced in `controls()` or
    /// `described_by()` of any node is present in the tree. This is the
    /// invariant our post-processing pass enforces; having a test here means
    /// a future refactor can't silently drop the pass and regress it.
    pub(crate) fn assert_no_dangling_relationships(update: &accesskit::TreeUpdate) {
        let emitted: std::collections::HashSet<accesskit::NodeId> =
            update.nodes.iter().map(|(id, _)| *id).collect();
        for (parent_id, node) in &update.nodes {
            for &target in node.controls() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has controls() → {target:?} which is absent from the tree"
                );
            }
            for &target in node.described_by() {
                assert!(
                    emitted.contains(&target),
                    "node {parent_id:?} has described_by() → {target:?} which is absent from the tree"
                );
            }
        }
    }

    /// Return all NodeIds whose role matches `role`.
    #[allow(dead_code)]
    pub(crate) fn nodes_with_role(
        update: &accesskit::TreeUpdate,
        role: accesskit::Role,
    ) -> Vec<accesskit::NodeId> {
        update
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == role)
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use crate::test_widgets::{FillWidget, StackWidget};

    /// The two framework-owned live-region nodes every `TreeUpdate` carries.
    /// Named rather than inlined so a node-count assertion says what it is
    /// counting. See [`crate::announcer`].
    const ANNOUNCER_NODES: usize = 2;

    // The framework's announcer.
    //
    // These assert through `accesskit_consumer`, not only through the tree's
    // own announcement ring. The ring reads `value().or(label())` while every
    // platform adapter reads `label()` alone, so a live region can pass the
    // in-process check and be silent on all three platforms — which is exactly
    // how two of this framework's own live regions shipped mute. Whether the
    // node is *in the filtered tree* is what the adapters key on, so that is
    // what these check.

    /// Is the announcer node one an adapter would see, and what does it say?
    ///
    /// `accesskit_consumer::Tree` runs the same filter each platform adapter
    /// runs. A hidden node is `FilterResult::ExcludeSubtree`, so it is absent
    /// from the filtered walk and no adapter will announce it.
    fn announced_by_consumer(update: &accesskit::TreeUpdate) -> Vec<(String, accesskit::Live)> {
        let consumer = accesskit_consumer::Tree::new(update.clone(), false);
        let state = consumer.state();
        let mut found = Vec::new();
        let mut stack = vec![state.root()];
        while let Some(node) = stack.pop() {
            if node.live() != accesskit::Live::Off
                && let Some(label) = node.label()
            {
                found.push((label, node.live()));
            }
            for child in node.filtered_children(&accesskit_consumer::common_filter) {
                stack.push(child);
            }
        }
        found
    }

    /// Every live node an adapter can reach in the filtered walk, by id.
    ///
    /// Distinct from [`announced_by_consumer`] on purpose: a node that stays in
    /// the tree with its label cleared looks "silent" to a label-based check
    /// while remaining present. Present-but-unlabelled is exactly what does NOT
    /// work — on Linux the announcement is emitted from `add_node` alone, so
    /// only a node that genuinely leaves and re-enters the filtered tree
    /// announces twice.
    fn live_nodes_in_filtered_tree(update: &accesskit::TreeUpdate) -> Vec<String> {
        let consumer = accesskit_consumer::Tree::new(update.clone(), false);
        let state = consumer.state();
        let mut found = Vec::new();
        let mut stack = vec![state.root()];
        while let Some(node) = stack.pop() {
            if node.live() != accesskit::Live::Off {
                found.push(format!("{:?}", node.id()));
            }
            for child in node.filtered_children(&accesskit_consumer::common_filter) {
                stack.push(child);
            }
        }
        found.sort();
        found
    }

    fn tree_with_one_widget() -> WidgetTree {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("content"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree
    }

    /// The headline behaviour: `announce` reaches the filtered tree exactly
    /// once, then leaves it again.
    #[test]
    fn an_announcement_enters_the_filtered_tree_then_leaves_it() {
        let mut tree = tree_with_one_widget();
        // Nothing announced yet: both live regions are hidden, so a platform
        // adapter sees neither.
        let update = tree.sync_accessibility();
        assert_eq!(announced_by_consumer(&update), Vec::new());
        assert_eq!(
            live_nodes_in_filtered_tree(&update),
            Vec::<String>::new(),
            "an idle announcer must be outside the filtered tree"
        );
        assert_a11y_tree_valid(&update);

        tree.announce("Event added");

        let exposed = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&exposed),
            vec![("Event added".to_string(), accesskit::Live::Polite)],
            "the message must be a label on a live node inside the filtered tree"
        );
        assert_eq!(
            live_nodes_in_filtered_tree(&exposed).len(),
            1,
            "exactly the polite announcer must have entered the filtered tree"
        );
        assert_a11y_tree_valid(&exposed);

        let retracted = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&retracted),
            Vec::new(),
            "the node must stop carrying the message"
        );
        // The load-bearing half. Clearing the label would satisfy the check
        // above while leaving the node in the tree, and a node that never
        // leaves the tree never announces again on Linux: the AT-SPI adapter
        // emits `ObjectEvent::Announcement` from `add_node` and from nowhere
        // else. So assert the node is genuinely gone from the filtered walk.
        assert_eq!(
            live_nodes_in_filtered_tree(&retracted),
            Vec::<String>::new(),
            "the live node must leave the filtered tree entirely, not merely \
             lose its label"
        );
        assert_a11y_tree_valid(&retracted);
    }

    /// The case the retract exists for. Both the Windows and the macOS adapter
    /// only announce an update whose label *changed*, so the same string twice
    /// in a row would be spoken once. Leaving and re-entering the tree is what
    /// makes the second one a fresh arrival.
    #[test]
    fn the_same_message_announced_twice_is_exposed_twice() {
        let mut tree = tree_with_one_widget();
        tree.announce("Saved");
        tree.announce("Saved");

        let mut exposures = 0;
        for _ in 0..6 {
            let update = tree.sync_accessibility();
            if announced_by_consumer(&update)
                .iter()
                .any(|(text, _)| text == "Saved")
            {
                exposures += 1;
            }
        }
        assert_eq!(exposures, 2, "each announcement needs its own exposure");
    }

    #[test]
    fn an_assertive_announcement_uses_the_assertive_live_setting() {
        let mut tree = tree_with_one_widget();
        tree.announce_with("Could not save", crate::announcer::Politeness::Assertive);
        let update = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&update),
            vec![("Could not save".to_string(), accesskit::Live::Assertive)]
        );
    }

    /// The two levels are independent nodes, so an urgent message does not have
    /// to wait behind a polite one.
    #[test]
    fn the_two_politeness_levels_announce_independently() {
        let mut tree = tree_with_one_widget();
        tree.announce("Event added");
        tree.announce_with("Could not save", crate::announcer::Politeness::Assertive);

        let mut spoken = announced_by_consumer(&tree.sync_accessibility());
        spoken.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            spoken,
            vec![
                ("Could not save".to_string(), accesskit::Live::Assertive),
                ("Event added".to_string(), accesskit::Live::Polite),
            ],
            "both levels must be exposed on the same update"
        );
    }

    /// Announcing must wake a tree that has nothing else to redraw, or a
    /// message raised by a handler that changed nothing visible would sit
    /// unspoken until something unrelated happened.
    #[test]
    fn announcing_requests_the_syncs_it_needs() {
        let mut tree = tree_with_one_widget();
        let _ = tree.sync_accessibility();
        tree.a11y_update_requested.set(false);
        tree.frame_tick_requested.set(false);

        tree.announce("Event added");
        assert!(
            tree.a11y_update_requested.get(),
            "an announcement must dirty the accessibility tree"
        );
        assert!(
            tree.frame_requested(),
            "an announcement must ask for the frame that carries it"
        );

        // Exposing leaves a retract still to do, so it asks again.
        tree.a11y_update_requested.set(false);
        tree.frame_tick_requested.set(false);
        let _ = tree.sync_accessibility();
        assert!(tree.a11y_update_requested.get());
        assert!(tree.frame_requested());

        // After the retract there is nothing more to schedule.
        tree.a11y_update_requested.set(false);
        tree.frame_tick_requested.set(false);
        let _ = tree.sync_accessibility();
        assert!(!tree.a11y_update_requested.get());
        assert!(!tree.frame_requested());
    }

    /// The in-process ring is what the automation bridge reports, so it has to
    /// see the same thing the adapters do.
    #[test]
    fn an_announcement_reaches_the_automation_ring() {
        let mut tree = tree_with_one_widget();
        let before = tree.announcements_since(0).len() as u64;
        tree.announce("Event added");
        let _ = tree.sync_accessibility();
        let got = tree.announcements_since(before);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "Event added");
        assert!(!got[0].assertive);
    }

    /// A screenshot inspects the tree and throws the update away. If it went
    /// through `sync_accessibility` it would consume the exposure step and the
    /// user would never hear the message.
    #[test]
    fn a_tree_snapshot_does_not_consume_a_pending_announcement() {
        let mut tree = tree_with_one_widget();
        tree.announce("Event added");

        let snapshot = tree.accessibility_tree_snapshot();
        assert_eq!(
            announced_by_consumer(&snapshot),
            Vec::new(),
            "a snapshot must not advance the announcer"
        );

        let delivered = tree.sync_accessibility();
        assert_eq!(
            announced_by_consumer(&delivered),
            vec![("Event added".to_string(), accesskit::Live::Polite)],
            "the message must still be there to deliver"
        );
    }

    /// An empty message is dropped rather than costing an exposure that says
    /// nothing.
    #[test]
    fn an_empty_announcement_is_not_exposed() {
        let mut tree = tree_with_one_widget();
        let _ = tree.sync_accessibility();
        tree.announce("   ");
        let update = tree.sync_accessibility();
        assert_eq!(announced_by_consumer(&update), Vec::new());
    }

    #[derive(Debug)]
    struct ActionWidget;

    impl Widget for ActionWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name("Save");
            builder.add_action(accesskit::Action::Click);
            builder.add_action(accesskit::Action::Focus);
        }
    }

    #[derive(Debug)]
    struct ClickableWidget;

    impl Widget for ClickableWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
            builder.set_role(accesskit::Role::Button);
            builder.set_name("Click Me");
            builder.add_action(accesskit::Action::Click);
        }
    }

    #[test]
    fn labeled_widget_has_accessibility() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Hello"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        let info = tree.accessibility_node(widget);
        assert_eq!(info.role(), accesskit::Role::Label);
        assert_eq!(info.name(), Some("Hello"));
    }

    #[test]
    fn find_by_label_works() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Save"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert_eq!(tree.find_by_label("Save"), Some(widget));
    }

    #[test]
    fn label_role_emits_text_as_value_not_label() {
        // accesskit contract: a `Role::Label` node carries its text in the
        // `value` property, not `label`. Widgets set the accessible name via
        // `set_name` (-> `label`); the builder re-serializes it to `value`
        // when the role is `Role::Label`, otherwise the Name is empty under
        // Windows UIA and stray under macOS AXStaticText. The logical
        // introspection name (used by `find_by_label` etc.) is unchanged.
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Section")); // Role::Label
        tree.layout(SizeProposal::exact(100.0, 20.0));

        // Logical view still reports the accessible name.
        assert_eq!(tree.accessibility_node(widget).name(), Some("Section"));

        // The *emitted* node carries the text in `value`, with `label` cleared.
        let update = tree.sync_accessibility();
        let node = &update
            .nodes
            .iter()
            .find(|(nid, _)| *nid == crate::accessibility::widget_id_to_node_id(widget))
            .expect("label node must be present in the tree update")
            .1;
        assert_eq!(node.role(), accesskit::Role::Label);
        assert_eq!(
            node.value(),
            Some("Section"),
            "Role::Label text must be exposed via `value`"
        );
        assert_eq!(
            node.label(),
            None,
            "Role::Label must not leave text in the `label` property"
        );
    }

    #[test]
    fn find_by_role_works() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Text"));
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert!(tree.find_by_role(accesskit::Role::Label).is_some());
    }

    #[test]
    fn access_hidden_bound_to_signal_toggles_reactively() {
        use crate::signal::Signal;
        use crate::widget_builder::WidgetBuilder;
        let hidden = Signal::new(false);
        let mut tree = WidgetTree::new();
        let w = tree.add(ClickableWidget.access_hidden(hidden.clone()));
        tree.layout(SizeProposal::exact(100.0, 30.0));
        assert!(
            !tree.accessibility_node(w).is_hidden(),
            "node should be visible to AT while the signal is false"
        );
        hidden.set(true);
        // `apply()` reads the prop fresh, so the pulled node reflects the flip.
        assert!(
            tree.accessibility_node(w).is_hidden(),
            "node should hide from AT when the bound signal flips to true"
        );
        hidden.set(false);
        assert!(!tree.accessibility_node(w).is_hidden());
    }

    /// **A widget with a context menu says so.** The dispatcher already opened
    /// the menu for `Action::ShowContextMenu`; nothing advertised the action, and
    /// an assistive technology offers what a node advertises.
    #[test]
    fn a_widget_with_a_context_menu_advertises_the_action() {
        let mut tree = WidgetTree::new();
        let plain = tree.add(FillWidget::new().label("no menu"));
        let with_menu = tree.add(
            FillWidget::new()
                .label("has menu")
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>)),
        );
        tree.layout(SizeProposal::exact(100.0, 20.0));

        assert!(
            tree.accessibility_node(with_menu)
                .actions()
                .contains(&Action::ShowContextMenu),
            "a node owning a context-menu factory must announce it"
        );
        assert!(
            !tree
                .accessibility_node(plain)
                .actions()
                .contains(&Action::ShowContextMenu),
            "and a node without one must not"
        );
    }

    /// The action is derived BEFORE the overrides, so an author who explicitly
    /// takes it away still has it taken away.
    #[test]
    fn a_removed_context_menu_action_stays_removed() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .label("suppressed")
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>))
                .access_remove_action(Action::ShowContextMenu),
        );
        tree.layout(SizeProposal::exact(100.0, 20.0));
        assert!(
            !tree
                .accessibility_node(id)
                .actions()
                .contains(&Action::ShowContextMenu)
        );
    }

    /// A descendant of a widget with a menu does **not** advertise it, even
    /// though the ancestor walk would open one there. Otherwise every nested box
    /// under a row with a menu would offer that menu, and an AT would read a
    /// dozen of them.
    #[test]
    fn a_child_does_not_advertise_its_parents_context_menu() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("inner"));
        let parent = tree.add(
            StackWidget::new()
                .add_child(child)
                .context_menu(|_pos, _ctx| Some(Box::new(FillWidget::new()) as Box<dyn Widget>)),
        );
        tree.layout(SizeProposal::exact(100.0, 20.0));

        assert!(
            tree.accessibility_node(parent)
                .actions()
                .contains(&Action::ShowContextMenu)
        );
        assert!(
            !tree
                .accessibility_node(child)
                .actions()
                .contains(&Action::ShowContextMenu),
            "the menu belongs to the widget that declared it"
        );
    }

    #[test]
    fn accessibility_node_collects_actions() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(ActionWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        let info = tree.accessibility_node(widget);
        assert_eq!(info.role(), accesskit::Role::Button);
        assert_eq!(info.name(), Some("Save"));
        assert_eq!(info.actions().len(), 2);
        assert!(info.actions().contains(&accesskit::Action::Click));
        assert!(info.actions().contains(&accesskit::Action::Focus));
    }

    #[test]
    fn sync_accessibility_produces_tree_update() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("First"));
        tree.add(FillWidget::new().label("Second"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();
        // Root, the two widgets, and the two framework live regions, which are
        // always present (and, here, hidden). See `crate::announcer`.
        assert_eq!(update.nodes.len(), 3 + ANNOUNCER_NODES);
        assert_eq!(update.nodes[0].0, accesskit::NodeId(0));
        assert!(update.tree.is_some());
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn root_node_carries_locale_as_language() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Bonjour"));

        // No locale set yet → no language hint (VoiceOver uses its default).
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let update = tree.sync_accessibility();
        assert_eq!(update.nodes[0].0, accesskit::NodeId(0));
        assert_eq!(
            update.nodes[0].1.language(),
            None,
            "no language before a locale is set"
        );

        // Once the app sets the locale, the root Window node carries it as a
        // BCP-47 language tag, which AccessKit propagates to the whole subtree.
        tree.set_locale("fr-FR".to_string());
        let update = tree.sync_accessibility();
        assert_eq!(
            update.nodes[0].1.language(),
            Some("fr-FR"),
            "root node must advertise the active locale as its language"
        );
    }

    #[test]
    fn sync_accessibility_excludes_dormant_widgets() {
        let mut tree = WidgetTree::new();
        tree.add(FillWidget::new().label("Active"));
        let dormant = tree.add(FillWidget::new().label("Dormant"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        tree.set_dormant(dormant);

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 2 + ANNOUNCER_NODES);
        assert_a11y_tree_valid(&update);
    }

    /// Regression test: a Relayout-only signal flip (no activation
    /// change, no role/label/value change, no focus change, no overlay
    /// activation) must NOT dirty the AccessKit cache. Previously
    /// `layout()` set `a11y_dirty = true` unconditionally on every
    /// layout pass, which fired ~60 Hz on any scene with a Pulse /
    /// Cycle animation.
    #[test]
    fn relayout_without_activation_change_does_not_dirty_a11y() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("Static"));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        // First sync clears `a11y_dirty` and populates the cache.
        let _ = tree.sync_accessibility();
        assert!(
            !tree.a11y_dirty,
            "sync_accessibility must clear the dirty flag"
        );

        // Simulate a Relayout-binding flip: the widget needs
        // re-layout, but its accessibility shape (active set, focus,
        // role, label, value) is unchanged.
        tree.arena.mark_needs_layout(id);
        tree.layout(SizeProposal::exact(200.0, 100.0));

        assert!(
            !tree.a11y_dirty,
            "pure Relayout (no activation / focus / overlay / a11y-binding change) must not dirty the AT cache"
        );
    }

    /// Companion to the regression above: the dormant→active path
    /// MUST still dirty the AT cache, because the accessibility walk
    /// skips dormant nodes.
    #[test]
    fn activation_transition_does_dirty_a11y() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("Toggle"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let _ = tree.sync_accessibility();
        assert!(!tree.a11y_dirty);

        tree.set_dormant(id);
        tree.layout(SizeProposal::exact(200.0, 100.0));
        assert!(
            tree.a11y_dirty,
            "active→dormant transition must dirty the AT cache so the dormant node is removed"
        );
    }

    #[test]
    fn sync_accessibility_includes_focus() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().focusable().label("Focused"));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        tree.focus(widget);

        let update = tree.sync_accessibility();
        let expected_focus = crate::accessibility::widget_id_to_node_id(widget);
        assert_eq!(update.focus, expected_focus);
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn sync_accessibility_parent_child_relationship() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        // A label keeps the parent from being collapsed as a presentational
        // container, so this exercises the parent→child push (not pruning).
        let parent = tree.add(
            StackWidget::new()
                .add_child(child)
                .access_label_literal("Parent"),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();
        assert_eq!(update.nodes.len(), 3 + ANNOUNCER_NODES);

        let parent_node_id = crate::accessibility::widget_id_to_node_id(parent);
        let parent_node = update
            .nodes
            .iter()
            .find(|(id, _)| *id == parent_node_id)
            .map(|(_, node)| node)
            .unwrap();

        let child_node_id = crate::accessibility::widget_id_to_node_id(child);
        assert!(parent_node.children().contains(&child_node_id));
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn presentational_containers_collapse_and_promote_children() {
        // A chain of bare presentational containers (StackWidget → empty
        // `Role::Unknown`) wrapping a labeled leaf collapses entirely: the
        // leaf is promoted to its nearest semantic ancestor, and no empty
        // grouping node remains (VoiceOver would announce one as "group").
        // A *labeled* container is semantic and survives.
        let mut tree = WidgetTree::new();
        let leaf = tree.add(FillWidget::new().label("Leaf")); // Role::Label
        let inner = tree.add(StackWidget::new().add_child(leaf)); // bare → pruned
        let outer = tree.add(StackWidget::new().add_child(inner)); // bare → pruned
        let labeled = tree.add(
            StackWidget::new()
                .add_child(outer)
                .access_label_literal("Group"),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();

        assert!(find_node(&update, inner).is_none(), "bare inner pruned");
        assert!(find_node(&update, outer).is_none(), "bare outer pruned");

        let labeled_node = find_node(&update, labeled).expect("labeled group survives");
        let leaf_nid = crate::accessibility::widget_id_to_node_id(leaf);
        assert!(
            labeled_node.children().contains(&leaf_nid),
            "leaf promoted past both bare containers to the labeled ancestor"
        );
        assert!(find_node(&update, leaf).is_some(), "labeled leaf kept");
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn find_by_action_finds_clickable() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(ClickableWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.find_by_action(accesskit::Action::Click), Some(widget));
        assert_eq!(tree.find_by_action(accesskit::Action::Focus), None);
    }

    #[test]
    fn text_content_returns_accessibility_name() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new().label("Hello World"));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(widget), Some("Hello World".to_string()));
    }

    #[test]
    fn text_content_returns_none_without_label() {
        let mut tree = WidgetTree::new();
        let widget = tree.add(FillWidget::new());
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert_eq!(tree.text_content(widget), None);
    }

    #[test]
    fn descendant_of_disabled_ancestor_reports_disabled() {
        use crate::signal::Signal;

        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let parent = tree.add(StackWidget::new().add_child(child));
        tree.enabled_when(parent, Signal::new(false));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        assert!(
            tree.accessibility_node(child).is_disabled(),
            "descendant should report disabled when ancestor is disabled"
        );
    }

    #[test]
    fn text_value_returns_accessibility_value() {
        #[derive(Debug)]
        struct ValueWidget;

        impl Widget for ValueWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }

            fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
                builder.set_role(accesskit::Role::Slider);
                builder.set_name("Volume");
                builder.set_value("75%");
            }
        }

        let mut tree = WidgetTree::new();
        let widget = tree.add(ValueWidget);
        tree.layout(SizeProposal::exact(100.0, 40.0));

        assert_eq!(tree.text_value(widget), Some("75%".to_string()));
        assert_eq!(tree.text_content(widget), Some("Volume".to_string()));
    }

    #[test]
    fn sync_accessibility_has_no_duplicate_children() {
        // Regression test for the AccessKit "duplicate child" crash (VoiceOver/NVDA).
        // assert_a11y_tree_valid already catches this via the consumer, but the
        // manual check here provides a more actionable failure message.
        let mut tree = WidgetTree::new();
        let grandchild = tree.add(FillWidget::new().label("Grandchild"));
        let child_a = tree.add(StackWidget::new().add_child(grandchild));
        let child_b = tree.add(FillWidget::new().label("Sibling"));
        let _root = tree.add(StackWidget::new().add_child(child_a).add_child(child_b));
        tree.layout(SizeProposal::exact(200.0, 100.0));

        let update = tree.sync_accessibility();

        let mut all_children: std::collections::HashMap<accesskit::NodeId, accesskit::NodeId> =
            std::collections::HashMap::new();
        for (parent_id, node) in &update.nodes {
            for &child_id in node.children() {
                let prev = all_children.insert(child_id, *parent_id);
                assert!(
                    prev.is_none(),
                    "duplicate child NodeId {child_id:?}: claimed by both {prev:?} and {parent_id:?}"
                );
            }
        }
        assert_a11y_tree_valid(&update);
    }

    #[test]
    fn no_dangling_relationships_in_basic_tree() {
        let mut tree = WidgetTree::new();
        let child = tree.add(FillWidget::new().label("Child"));
        let _parent = tree.add(StackWidget::new().add_child(child));
        tree.layout(SizeProposal::exact(100.0, 50.0));

        let update = tree.sync_accessibility();
        assert_no_dangling_relationships(&update);
        assert_a11y_tree_valid(&update);
    }

    // ── Builder-level accessibility override tests ───────────────────
    //
    // Tests for `WidgetBuilder::access_*` methods.

    use crate::widget_builder::WidgetBuilder;
    use accesskit::{Action, AriaCurrent, HasPopup, Live, Orientation, Role};

    /// Find a node in a TreeUpdate by WidgetId.
    fn find_node(update: &accesskit::TreeUpdate, id: WidgetId) -> Option<&accesskit::Node> {
        let nid = crate::accessibility::widget_id_to_node_id(id);
        update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == nid)
            .map(|(_, n)| n)
    }

    /// A widget that calls set_hidden() unconditionally — used to test
    /// `access_hidden(false)` clears widget-emitted hidden state.
    #[derive(Debug)]
    struct AlwaysHiddenWidget;
    impl Widget for AlwaysHiddenWidget {
        fn layout_response(
            &self,
            proposal: SizeProposal,
            _ctx: &LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
        fn accessibility(&self, builder: &mut AccessNodeBuilder) {
            builder.set_role(Role::GenericContainer);
            builder.set_hidden();
        }
    }

    // Test 1
    #[test]
    fn access_label_replaces_widget_label() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_label_literal("Publish"));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Publish"));
    }

    // Test 2
    #[test]
    fn access_description_appears_on_bare_widget() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_description_literal("Decorative"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).expect("node present");
        assert_eq!(node.description(), Some("Decorative"));
    }

    // Test 3
    #[test]
    fn access_value_replaces_widget_value() {
        #[derive(Debug)]
        struct SliderWidget;
        impl Widget for SliderWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                builder.set_role(Role::Slider);
                builder.set_value("50");
            }
        }
        let mut tree = WidgetTree::new();
        let id = tree.add(SliderWidget.access_value_literal("Custom"));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert_eq!(tree.text_value(id), Some("Custom".to_string()));
    }

    // Test 4
    #[test]
    fn access_role_overrides_widget_role() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("H").access_role(Role::Heading));
        tree.layout(SizeProposal::exact(100.0, 40.0));
        assert_eq!(tree.accessibility_node(id).role(), Role::Heading);
    }

    #[test]
    fn an_unshown_tooltip_still_describes_its_anchor_for_screen_readers() {
        // The pass used to wire `described_by` only while the tooltip's
        // overlay was live. A plain tooltip is never auto-shown on focus, and
        // a dormant content node is excluded from the emitted tree (so it
        // cannot be a relation target anyway) — which left the whole plain
        // tier with no screen-reader path at all. With no hover, the anchor
        // must still carry the text as a static description.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().label("Save"));
        let tip = tree.add(FillWidget::new().label("Save the file"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(500));

        assert!(tree.active_overlays().is_empty(), "not hovered, not shown");
        let update = tree.sync_accessibility();
        let node = find_node(&update, anchor).expect("anchor node present");
        assert_eq!(
            node.description(),
            Some("Save the file"),
            "the anchor must describe itself with its tooltip text while unshown"
        );
    }

    #[test]
    fn a_shown_tooltip_switches_the_anchor_to_the_described_by_relation() {
        // Once the content is genuinely in the tree, the richer relation is
        // used instead of the copied string.
        let mut tree = WidgetTree::new();
        let anchor = tree.add(FillWidget::new().label("Save"));
        let tip = tree.add(FillWidget::new().label("Save the file"));
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.attach_tooltip(anchor, tip, std::time::Duration::from_millis(100));

        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(std::time::Duration::from_millis(150));
        assert_eq!(tree.active_overlays().len(), 1, "tooltip shown");

        let update = tree.sync_accessibility();
        let node = find_node(&update, anchor).expect("anchor node present");
        assert!(
            !node.described_by().is_empty(),
            "a shown tooltip is wired as a described_by relation"
        );
    }

    // Test 5
    #[test]
    fn access_hint_alias_writes_description_field() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_hint_literal("Tip"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.description(), Some("Tip"));
    }

    // Test 6
    #[test]
    fn access_identifier_writes_author_id() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_identifier("save-button"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.author_id(), Some("save-button"));
    }

    // Test 7a — literal shortcut variant
    #[test]
    fn access_shortcut_literal_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_shortcut_literal("Ctrl+S"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.keyboard_shortcut(), Some("Ctrl+S"));
    }

    // Test 7b — id-based shortcut resolves through ShortcutRegistry,
    // including auto-refresh on rebind.
    #[test]
    fn access_shortcut_id_resolves_and_tracks_rebinds() {
        use crate::event::Key;
        use crate::shortcut::{KeyStroke, Shortcut};

        let mut tree = WidgetTree::new();
        tree.shortcut_registry_mut().register(
            Shortcut::new("app.save")
                .name("Save")
                .primary(KeyStroke::command(Key::S))
                .build(),
        );
        let id = tree.add(ClickableWidget.access_shortcut_id("app.save"));
        tree.layout(SizeProposal::exact(50.0, 50.0));

        // The chord is declared on the primary accelerator, so the announced
        // name is the key the user is actually looking at: ⌘ on macOS, Ctrl
        // everywhere else (see `Modifiers`' `Display`).
        let accel = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };

        // Initial registration: AT announces the default keystroke.
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(
            node.keyboard_shortcut(),
            Some(format!("{accel}+S").as_str())
        );

        // Simulate a user rebind: the AT announcement should track it.
        tree.shortcut_registry_mut()
            .rebind_primary("app.save", Some(KeyStroke::command(Key::Q)));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(
            node.keyboard_shortcut(),
            Some(format!("{accel}+Q").as_str())
        );
    }

    // Test 7c — silently omits the announcement when the id has no
    // registered default. Same fallback behavior as `MenuItem::for_shortcut`.
    #[test]
    fn access_shortcut_id_unknown_id_omits_announcement() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_shortcut_id("never.registered"));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.keyboard_shortcut(), None);
    }

    // Test 8
    #[test]
    fn access_hidden_true_hides_widget() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_hidden(true));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(tree.accessibility_node(id).is_hidden());
    }

    // Test 9
    #[test]
    fn access_hidden_false_clears_widget_set_hidden() {
        let mut tree = WidgetTree::new();
        let id = tree.add(AlwaysHiddenWidget.access_hidden(false));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(
            !tree.accessibility_node(id).is_hidden(),
            "access_hidden(false) should clear widget-emitted hidden"
        );
    }

    // Test 10
    #[test]
    fn access_disabled_true_marks_disabled() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_disabled(true));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(tree.accessibility_node(id).is_disabled());
    }

    // Test 11
    #[test]
    fn access_disabled_false_clears_arena_driven_disabled() {
        use crate::signal::Signal;
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().label("X").access_disabled(false));
        tree.enabled_when(id, Signal::new(false));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert!(
            !tree.accessibility_node(id).is_disabled(),
            "access_disabled(false) should clear even arena-driven disabled"
        );
    }

    // Test 12
    #[test]
    fn access_controls_appends() {
        let mut tree = WidgetTree::new();
        let target = tree.add(FillWidget::new().label("Target"));
        let controller = tree.add(
            FillWidget::new()
                .label("Controller")
                .access_controls(target),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, controller).unwrap();
        let target_nid = crate::accessibility::widget_id_to_node_id(target);
        assert!(
            node.controls().contains(&target_nid),
            "controls list should contain the target NodeId"
        );
    }

    // Test 13
    #[test]
    fn access_described_by_appends() {
        let mut tree = WidgetTree::new();
        let other = tree.add(FillWidget::new().label("Desc"));
        let id = tree.add(FillWidget::new().label("Main").access_described_by(other));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        let other_nid = crate::accessibility::widget_id_to_node_id(other);
        assert!(node.described_by().contains(&other_nid));
    }

    // Test 14
    #[test]
    fn access_labelled_by_appends() {
        let mut tree = WidgetTree::new();
        let other = tree.add(FillWidget::new().label("Lbl"));
        let id = tree.add(FillWidget::new().label("Main").access_labelled_by(other));
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        let other_nid = crate::accessibility::widget_id_to_node_id(other);
        assert!(node.labelled_by().contains(&other_nid));
    }

    // Test 15
    #[test]
    fn access_live_assertive_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_live(Live::Assertive));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.live(), Some(Live::Assertive));
    }

    // Test 16
    #[test]
    fn access_aria_current_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_current(AriaCurrent::Page));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.aria_current(), Some(AriaCurrent::Page));
    }

    // Test 17
    #[test]
    fn access_has_popup_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ClickableWidget.access_has_popup(HasPopup::Menu));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.has_popup(), Some(HasPopup::Menu));
    }

    // Test 18
    #[test]
    fn access_orientation_set() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_orientation(Orientation::Vertical));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.orientation(), Some(Orientation::Vertical));
    }

    // Test 19
    #[test]
    fn access_numeric_value_and_range() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_role(Role::Slider)
                .access_numeric_value(50.0)
                .access_numeric_range(0.0, 100.0)
                .access_numeric_step(5.0),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.numeric_value(), Some(50.0));
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        assert_eq!(node.numeric_value_step(), Some(5.0));
    }

    // Test 20
    #[test]
    fn access_action_advertises_and_routes() {
        use crate::signal::Signal;
        let flag = Signal::new(false);
        let flag_for_cb = flag.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_action(Action::ShowContextMenu, move |_ctx| flag_for_cb.set(true)),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let info = tree.accessibility_node(id);
        assert!(info.actions().contains(&Action::ShowContextMenu));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::ShowContextMenu,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(flag.get(), "callback should have been invoked");
    }

    // Test 21
    #[test]
    fn access_two_actions_both_route() {
        use crate::signal::Signal;
        let click = Signal::new(false);
        let increment = Signal::new(false);
        let click_cb = click.clone();
        let inc_cb = increment.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_action(Action::ShowContextMenu, move |_| click_cb.set(true))
                .access_action(Action::Increment, move |_| inc_cb.set(true)),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::ShowContextMenu,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(click.get() && !increment.get(), "only first action fired");
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::Increment,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(
            click.get() && increment.get(),
            "both actions fired exactly once each"
        );
    }

    // Test 22
    #[test]
    fn access_remove_action_suppresses_widget_action() {
        let mut tree = WidgetTree::new();
        let id = tree.add(ActionWidget.access_remove_action(Action::Click));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let info = tree.accessibility_node(id);
        assert!(!info.actions().contains(&Action::Click));
        assert!(info.actions().contains(&Action::Focus));
    }

    // Test 23
    #[test]
    fn access_custom_action_uses_localized_label() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_custom_action_literal("Reply", |_ctx| {}));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        let actions = node.custom_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, 0);
        assert_eq!(actions[0].description.as_str(), "Reply");
    }

    // Test 24
    #[test]
    fn access_custom_action_routes_by_index() {
        use crate::signal::Signal;
        let first = Signal::new(false);
        let second = Signal::new(false);
        let f = first.clone();
        let s = second.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_custom_action_literal("First", move |_| f.set(true))
                .access_custom_action_literal("Second", move |_| s.set(true)),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::CustomAction,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: Some(accesskit::ActionData::CustomAction(1)),
        });
        assert!(!first.get(), "first should not fire");
        assert!(second.get(), "second should fire (index 1)");
    }

    // Test 25
    #[test]
    fn access_action_layered_with_on_access_action() {
        use crate::signal::Signal;
        let from_override = Signal::new(false);
        let from_user = Signal::new(false);
        let ov_cb = from_override.clone();
        let user_cb = from_user.clone();
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_action(Action::ShowContextMenu, move |_| ov_cb.set(true))
                .on_access_action(move |_action, _ctx| {
                    user_cb.set(true);
                    crate::event::EventResponse::Handled
                }),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        tree.dispatch_event(crate::event::WidgetEvent::AccessAction {
            action: Action::ShowContextMenu,
            target: Some(id),
            target_node: crate::accessibility::widget_id_to_node_id(id),
            data: None,
        });
        assert!(from_override.get(), "override callback fired");
        assert!(from_user.get(), "user catch-all fired");
    }

    // Test 26 — i18n integration via teksilo_i18n's
    // `From<LocalizedString> for Prop<String>` impl. We don't import
    // teksilo-i18n here (it depends on teksilo-core), but the conversion
    // works the same way for any `Into<Prop<String>>`. This stand-in
    // covers the same code path the FTL-bundle case takes.
    #[test]
    fn access_label_accepts_resolved_string_via_into() {
        // Simulate a `LocalizedString`-like wrapper: any type that
        // `impl Into<Prop<String>>`. The override surface stores the
        // prop and the walker reads its current value.
        struct ResolvedAtCall(String);
        impl From<ResolvedAtCall> for crate::signal::Prop<String> {
            fn from(v: ResolvedAtCall) -> crate::signal::Prop<String> {
                crate::signal::Prop::Static(v.0)
            }
        }
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_label(ResolvedAtCall("Save".to_string())));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("Save"));
    }

    // Test 27
    #[test]
    fn access_exclude_subtree_prunes_children_from_at_tree() {
        let mut tree = WidgetTree::new();
        let inner1 = tree.add(FillWidget::new().label("A"));
        let inner2 = tree.add(FillWidget::new().label("B"));
        let outer = tree.add(
            StackWidget::new()
                .add_child(inner1)
                .add_child(inner2)
                // A label keeps `outer` from being collapsed as a
                // presentational container, so the test exercises Exclude
                // (not the new presentational-pruning pass).
                .access_label_literal("Section")
                .access_exclude_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let update = tree.sync_accessibility();
        // Outer is present; inner1 and inner2 are pruned.
        assert!(find_node(&update, outer).is_some());
        assert!(find_node(&update, inner1).is_none(), "inner1 pruned");
        assert!(find_node(&update, inner2).is_none(), "inner2 pruned");
        let outer_node = find_node(&update, outer).unwrap();
        assert!(
            outer_node.children().is_empty(),
            "outer should have no AT children when excluded"
        );
    }

    // Test 28
    #[test]
    fn access_merge_subtree_concatenates_descendant_labels() {
        let mut tree = WidgetTree::new();
        let title = tree.add(FillWidget::new().label("Title"));
        let subtitle = tree.add(FillWidget::new().label("Subtitle"));
        let card = tree.add(
            StackWidget::new()
                .add_child(title)
                .add_child(subtitle)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        // After merge: card's name is "Title Subtitle", children pruned.
        assert_eq!(tree.text_content(card), Some("Title Subtitle".to_string()));
        let update = tree.sync_accessibility();
        assert!(find_node(&update, title).is_none());
        assert!(find_node(&update, subtitle).is_none());
    }

    // Test 29
    #[test]
    fn access_merge_subtree_unions_actions() {
        let mut tree = WidgetTree::new();
        let click_a = tree.add(ClickableWidget);
        let click_b = tree.add(ClickableWidget);
        let card = tree.add(
            StackWidget::new()
                .add_child(click_a)
                .add_child(click_b)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let actions = tree.accessibility_node(card).actions().to_vec();
        let click_count = actions.iter().filter(|a| **a == Action::Click).count();
        assert_eq!(
            click_count, 1,
            "Click should be present exactly once after merge (deduplicated)"
        );
    }

    // Test 30 — covered by the i18n integration mechanism documented in
    // `From<LocalizedString> for String`. Since teksilo-core can't reference
    // LocalizedString, the merged-localized-label case is exercised by
    // tests 26 + 28 in combination: each child's resolved-at-call-time
    // String contributes to the merged label. The full FTL-bundle
    // round-trip is tested in the teksilo-i18n / teksilo-widgets integration
    // tests, not here.

    // Test 31
    #[test]
    fn access_merge_subtree_first_nonempty_value_wins() {
        #[derive(Debug)]
        struct ValueWidget(&'static str);
        impl Widget for ValueWidget {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                builder.set_role(Role::Slider);
                builder.set_value(self.0);
            }
        }
        let mut tree = WidgetTree::new();
        let v1 = tree.add(ValueWidget("first"));
        let v2 = tree.add(ValueWidget("second"));
        let card = tree.add(
            StackWidget::new()
                .add_child(v1)
                .add_child(v2)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        assert_eq!(tree.text_value(card), Some("first".to_string()));
    }

    // Test 32
    #[test]
    fn access_exclude_inside_merge() {
        let mut tree = WidgetTree::new();
        let visible = tree.add(FillWidget::new().label("VISIBLE"));
        let pruned = tree.add(FillWidget::new().label("PRUNED"));
        let inner_excluded = tree.add(
            StackWidget::new()
                .add_child(pruned)
                .access_exclude_subtree(),
        );
        let card = tree.add(
            StackWidget::new()
                .add_child(visible)
                .add_child(inner_excluded)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(100.0, 50.0));
        let merged = tree.text_content(card).unwrap_or_default();
        // VISIBLE present, PRUNED absent (because inner_excluded
        // pruned its own subtree before the merge could absorb it).
        assert!(merged.contains("VISIBLE"));
        assert!(
            !merged.contains("PRUNED"),
            "excluded subtree should not contribute to merge"
        );
    }

    // Test 33
    #[test]
    fn access_merge_inside_merge() {
        let mut tree = WidgetTree::new();
        let inner_a = tree.add(FillWidget::new().label("a"));
        let inner_b = tree.add(FillWidget::new().label("b"));
        let inner_card = tree.add(
            StackWidget::new()
                .add_child(inner_a)
                .add_child(inner_b)
                .access_merge_subtree(),
        );
        let outer_extra = tree.add(FillWidget::new().label("X"));
        let outer = tree.add(
            StackWidget::new()
                .add_child(inner_card)
                .add_child(outer_extra)
                .access_merge_subtree(),
        );
        tree.layout(SizeProposal::exact(200.0, 100.0));
        // Outer absorbs inner_card's already-merged label ("a b") AND
        // outer_extra ("X"), giving something like "a b X" or "X a b".
        // Order is descendant-walk order; we just verify all parts
        // appear and inner children are NOT double-counted.
        let merged = tree.text_content(outer).unwrap_or_default();
        assert!(merged.contains("a"));
        assert!(merged.contains("b"));
        assert!(merged.contains("X"));
        // Inner children should not appear AS THEIR OWN nodes:
        let update = tree.sync_accessibility();
        assert!(find_node(&update, inner_a).is_none());
        assert!(find_node(&update, inner_b).is_none());
        assert!(find_node(&update, inner_card).is_none());
        assert!(find_node(&update, outer_extra).is_none());
    }

    // Test 34
    #[test]
    fn access_customize_runs_last() {
        let mut tree = WidgetTree::new();
        let id = tree.add(
            FillWidget::new()
                .access_label_literal("A")
                .access_customize(|b| b.set_name("B")),
        );
        tree.layout(SizeProposal::exact(50.0, 50.0));
        assert_eq!(tree.accessibility_node(id).name(), Some("B"));
    }

    // Test 35
    #[test]
    fn access_customize_can_reach_inner_mut() {
        let mut tree = WidgetTree::new();
        let id = tree.add(FillWidget::new().access_customize(|b| {
            b.inner_mut().set_author_id("from-customize");
        }));
        tree.layout(SizeProposal::exact(50.0, 50.0));
        let update = tree.sync_accessibility();
        let node = find_node(&update, id).unwrap();
        assert_eq!(node.author_id(), Some("from-customize"));
    }

    // Test 36 — sanity guard. WidgetNode size delta when no overrides.
    #[test]
    fn access_overrides_zero_cost_when_unused() {
        // The override fields add only `Option<Box<...>>` (8 bytes for
        // the pointer-sized null) + `AccessSubtreeMode` (1 byte enum,
        // padded). Sanity: at most 16 bytes added.
        // We don't assert a specific size because struct layout shifts
        // with rustc versions; we just confirm both fields are
        // pointer/byte sized.
        use std::mem::size_of;
        assert!(size_of::<Option<Box<crate::widget_builder::AccessibilityOverrides>>>() <= 16);
        assert!(size_of::<crate::widget_builder::AccessSubtreeMode>() <= 4);
    }

    #[test]
    fn accessibility_children_overrides_at_reading_order() {
        // Audit G17: a widget can present a different child ORDER to assistive
        // tech than its layout/paint child order via `accessibility_children()`
        // — the mechanism TableView/TreeTableView use to read the header before
        // the body even though they build the body first (z-order).
        #[derive(Debug)]
        struct NamedLeaf(&'static str);
        impl Widget for NamedLeaf {
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(10.0, 10.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                // Button keeps `set_name` as the node's label (Role::Label
                // routes text to `value` instead), so the test can find the
                // leaves by name.
                builder.set_role(accesskit::Role::Button);
                builder.set_name(self.0);
            }
        }

        #[derive(Debug, Default)]
        struct ReorderContainer {
            a: Option<WidgetId>,
            b: Option<WidgetId>,
        }
        impl Widget for ReorderContainer {
            fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
                let a = ctx.add(NamedLeaf("A"));
                let b = ctx.add(NamedLeaf("B"));
                self.a = Some(a);
                self.b = Some(b);
                vec![a, b]
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> crate::widget::LayoutResponse {
                proposal.resolve(100.0, 100.0).into()
            }
            fn accessibility(&self, builder: &mut AccessNodeBuilder) {
                // A real role so the container itself isn't dropped as a
                // presentational node — we need to inspect its child order.
                builder.set_role(accesskit::Role::Group);
                builder.set_name("Container");
            }
            fn children(&self) -> Vec<WidgetId> {
                [self.a, self.b].into_iter().flatten().collect()
            }
            fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
                // Reverse of build/layout order.
                Some([self.b, self.a].into_iter().flatten().collect())
            }
        }

        let mut tree = WidgetTree::new();
        let container = tree.add(ReorderContainer::default());
        tree.layout(SizeProposal::exact(100.0, 100.0));
        let update = tree.sync_accessibility();

        let a_nid = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("A"))
            .map(|(id, _)| *id)
            .expect("A node present");
        let b_nid = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some("B"))
            .map(|(id, _)| *id)
            .expect("B node present");
        let cnid = crate::accessibility::widget_id_to_node_id(container);
        let cnode = update
            .nodes
            .iter()
            .find(|(id, _)| *id == cnid)
            .map(|(_, n)| n)
            .expect("container node present");
        let kids = cnode.children();
        let pa = kids.iter().position(|k| *k == a_nid).expect("A is a child");
        let pb = kids.iter().position(|k| *k == b_nid).expect("B is a child");
        assert!(
            pb < pa,
            "accessibility_children() must set AT order B-before-A; got {kids:?}"
        );
    }
}
