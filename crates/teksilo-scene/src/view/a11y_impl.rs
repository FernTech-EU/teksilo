// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Accessibility implementation for [`SceneView`].
//!
//! Implements `SceneView::accessibility` and the redirect hook
//! `a11y_redirect_descendant_impl`, which together build the two-tier
//! logical AT tree: lightweight `SceneItem`s and virtual `A11yGroup`s
//! become synthetic AccessKit nodes, while heavyweight `Widget` entries
//! are auto-grafted under their declared logical parents via the
//! framework's redirect mechanism. The DFS walker `emit_logical_node`
//! drives the tree and also emits `SceneMagnet` nodes for the
//! keyboard-connect roving-focus pattern.
//!
//! # One coordinate space for the whole scene subtree
//!
//! Every rectangle emitted here is in **scene coordinates**, and the view
//! transform is declared once — as an AccessKit node transform — on each node
//! that sits directly under the `SceneView`'s own node. AccessKit applies a
//! node's transform "to any coordinates within this node and its descendants,
//! including the `bounds` property of this node", and reads `bounds` "in the
//! coordinate space of the nearest ancestor with a non-`None` transform", so
//! one declaration at the boundary carries the camera to everything below it:
//! an item's box, a group's children, a text run's per-character positions, a
//! magnet's anchor.
//!
//! This is the same space the *heavyweight* tier is in. A card's arena bounds
//! are scene coordinates, and the framework walker declares the identical
//! transform on it (see `WidgetTree::build_accessibility_recursive`). So the
//! two tiers do not merely happen to agree about where an item is on screen —
//! they are described in one coordinate system, which is what lets a widget
//! relocated into the logical tree by `set_a11y_parent` land in the right
//! place whichever tier it came from.
//!
//! Projecting instead of declaring would be exact for a rectangle and wrong
//! for everything else: `Rect` is axis-aligned, so a rotated camera loses the
//! shape; per-character geometry would need scaling by hand; and
//! `accesskit_consumer`'s hit test descends by inverting each node's transform,
//! so a pre-projected subtree is unreachable by an explore-by-touch probe.

use super::*;

/// How far, in logical pixels on screen, a magnet's AT box extends from the
/// anchor point. A pointer affordance, so it keeps its screen size as the
/// camera zooms rather than scaling with the drawing.
const MAGNET_AT_HALF_EXTENT: f32 = 8.0;

/// The view transform's scale, for the one case that needs to undo it.
///
/// The geometric mean of the two singular values via `|det|.sqrt()`, so a
/// non-uniform or rotated camera still yields one sensible number. Falls back
/// to `1.0` for a degenerate transform rather than dividing by zero.
fn view_scale(t: &Transform2D) -> f32 {
    let [a, b, c, d, _, _] = t.m;
    let det = (a * d - b * c).abs();
    if det.is_finite() && det > f32::EPSILON {
        det.sqrt()
    } else {
        1.0
    }
}

impl SceneView {
    /// Declare that this synthetic node — and everything under it — states its
    /// rectangles in scene coordinates.
    ///
    /// Only at the boundary: `parent_id` is `None` exactly for a node the
    /// walk is attaching to the `SceneView`'s own node, whose space is the
    /// window's. A node deeper in the logical tree already inherits the
    /// declaration from its parent, and re-stating it there would compose the
    /// camera twice.
    fn declare_scene_space(
        child: &mut teksilo_core::accessibility::AccessNodeBuilder,
        parent_id: Option<accesskit::NodeId>,
        view_transform: Transform2D,
    ) {
        if parent_id.is_none() {
            child
                .inner_mut()
                .set_transform(teksilo_core::accessibility::to_accesskit_affine(
                    view_transform,
                ));
        }
    }

    pub(super) fn a11y_redirect_descendant_impl(
        &self,
        _self_id: WidgetId,
        descendant: WidgetId,
    ) -> Option<accesskit::NodeId> {
        // tell the framework walker to skip
        // its default push for any heavyweight scene entry whose
        // declared logical parent is in our own logical tree.
        // Two paths:
        //   1. The widget was added via `Scene::add_widget` (most
        //      common). Its `ItemId` lives in `widget_to_item`.
        //      Look up the declaration via
        //      `a11y_parent_of(A11yNode::Item(item_id))`.
        //   2. The widget was relocated ad-hoc via
        //      `set_a11y_parent(A11yNode::Widget(widget_id), ...)`.
        //      Look it up directly. Used for descendants of
        //      heavyweight items.
        use crate::a11y::A11yNode;
        use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};
        let owner = self.self_widget_id.get()?;
        let parent = self
            .widget_to_item
            .get(&descendant)
            .and_then(|item_id| self.scene().a11y_parent_of(A11yNode::Item(*item_id)))
            .or_else(|| self.scene().a11y_parent_of(A11yNode::Widget(descendant)))?;
        match parent {
            A11yNode::Item(item_id) => Some(synthetic_node_id(
                owner,
                item_id.as_u64(),
                SyntheticKind::SceneItem,
            )),
            A11yNode::Group(group_id) => Some(synthetic_node_id(
                owner,
                group_id.as_u64(),
                SyntheticKind::SceneGroup,
            )),
            A11yNode::Widget(_) => {
                // Widget→Widget reparenting: the declared parent
                // widget's NodeId isn't ours to attach to (it's
                // owned by another arena widget's accessibility()
                // emission). Fall through.
                None
            }
        }
    }

    pub(super) fn accessibility_impl(
        &self,
        builder: &mut teksilo_core::accessibility::AccessNodeBuilder,
    ) {
        use crate::a11y::A11yNode;
        use crate::scene::SceneEntryKind;
        use std::collections::{HashMap, HashSet};
        use teksilo_core::accessibility::{SyntheticKind, synthetic_node_id};

        // SceneView itself is `Role::Pane` for a top-level scene
        // and `Role::Region` for a logically nested scene (set via
        // `nested_a11y(true)`). Heavyweight children (real widgets
        // in the arena) are emitted by the tree walker as natural
        // descendants; we only need to add the lightweight tier
        // here.
        if self.a11y_nested {
            builder.set_role(accesskit::Role::Region);
        } else {
            builder.set_role(accesskit::Role::Pane);
        }
        if let Some(label) = &self.a11y_label {
            builder.set_name(label.resolve_now());
        }

        // Compute screen-space viewport for the at-visible-region
        // query. `last_viewport` was set by `layout_response`;
        // `bounds_origin_signal` was set by `place_children`.
        let viewport_size = self.last_viewport.get();
        let bounds_origin = self.bounds_origin_signal.get();
        let viewport_screen = Rect::new(
            bounds_origin.x,
            bounds_origin.y,
            viewport_size.width,
            viewport_size.height,
        );
        let view_transform = self.view_transform();
        let visible_scene_region = match view_transform.inverse() {
            Some(inv) => inv.apply_rect(viewport_screen),
            None => Rect::ZERO,
        };
        let at_region = self
            .a11y_off_screen_mode
            .at_visible_region(visible_scene_region);

        // The set of items the off-screen-mode policy says are
        // AT-visible. Used to filter the second pass.
        let visible_item_ids: HashSet<ItemId> = match at_region {
            Some(r) => self.scene().items_in_rect(r).into_iter().collect(),
            None => self.scene().ids().into_iter().collect(),
        };

        // Build a `parent → ordered children` map of the logical
        // tree. Roots (no declared parent) live under the synthetic
        // key `None`. Insertion-order preserves the apps' declared
        // child order: groups in `add_a11y_group` order, items in
        // `add_item` order. The `None` bucket keeps groups before
        // items so screen readers announce structure first.
        let mut logical_children: HashMap<Option<A11yNode>, Vec<A11yNode>> = HashMap::new();

        // Place groups. Groups always emit — they have no
        // visual default to fall back to. A group with no declared
        // parent goes to SceneView root, regardless of mode.
        for group in &self.scene().a11y_groups {
            let node = A11yNode::Group(group.id);
            let parent = self.scene().a11y_parent_of(node);
            logical_children.entry(parent).or_default().push(node);
        }

        // Place all visible scene entries — lightweight
        // items and heavyweight widgets alike. Both kinds use
        // `A11yNode::Item(item_id)` as their logical-tree address.
        // Discrimination by entry kind happens at emit time.
        //
        // Mode dispatch (applies to lightweight items only —
        // heavyweight widgets always emit via the framework walker
        // since they own focus / interaction state; the only
        // question is whether they emit at SceneView root or under
        // a declared logical parent):
        //   - Cooperative: item without a declared parent emits
        //     as a SceneView-root child (visual default).
        //   - StrictlyParallel: lightweight item without a parent
        //     is suppressed; heavyweight without a parent stays
        //     at SceneView root via the framework walker.
        // Heavyweight entries are gated by the set the layout pass just wrote,
        // which is the cards this view *publishes*: those the mode's region
        // reaches, plus any the user is in the middle of, intersected with
        // those the pass kept alive. Two different things could go wrong if
        // this were re-derived here instead of read back, and the recorded set
        // is what rules out both — a card parked by this pass must not be
        // grafted (the framework walker emitted nothing for it, so the graft
        // would name a child that is not in the tree), and a card pinned by
        // focus must be grafted wherever the camera is (its node IS in the
        // tree, and an unnamed node is an orphan the consumer rejects just as
        // hard). A rectangle test here would get the first right and the
        // second wrong.
        //
        // `None` means the view has not been laid out yet, so nothing has been
        // decided and every card is still live.
        //
        // The framework walker is gated by the same decision through
        // `SceneView::accessibility_children`, so a card left out here is left
        // out there too — neither named nor emitted.
        let at_heavy = self.at_heavy.borrow();
        let publishes_heavy =
            |id: ItemId| -> bool { at_heavy.as_ref().is_none_or(|set| set.contains(&id)) };

        // The candidates, gathered from the two sets rather than by walking
        // `entries` and filtering — so the walk costs what it publishes and
        // not what the model holds. Restored to **declaration order** before
        // emitting, because that order is the order a screen reader reads the
        // scene in and neither set carries it.
        let candidates: Vec<(usize, ItemId, bool)> = {
            let scene = self.scene();
            let mut out: Vec<(usize, ItemId, bool)> = Vec::new();
            let push = |id: ItemId, is_widget: bool, out: &mut Vec<_>| {
                if let Some(order) = scene.entry_order(id) {
                    out.push((order, id, is_widget));
                }
            };
            for &id in &visible_item_ids {
                // `items_in_rect` answers for both tiers; the heavyweight ones
                // are decided by the published set instead, just below.
                if scene.item(id).is_some() {
                    push(id, false, &mut out);
                }
            }
            match at_heavy.as_ref() {
                Some(set) => {
                    for &id in set {
                        push(id, true, &mut out);
                    }
                }
                // Not laid out yet: every card is live.
                None => {
                    for entry in &scene.entries {
                        if matches!(&entry.kind, SceneEntryKind::Widget { .. }) {
                            push(entry.id, true, &mut out);
                        }
                    }
                }
            }
            out.sort_unstable_by_key(|(order, _, _)| *order);
            out
        };

        for (_, item_id, is_widget) in candidates {
            let node = A11yNode::Item(item_id);
            let parent = self.scene().a11y_parent_of(node);
            match (parent, is_widget, self.a11y_mode) {
                (Some(p), _, _) => {
                    logical_children.entry(Some(p)).or_default().push(node);
                }
                (None, false, crate::a11y::A11yMode::Cooperative) => {
                    // Lightweight item, root, cooperative → emit at root.
                    logical_children.entry(None).or_default().push(node);
                }
                (None, false, crate::a11y::A11yMode::StrictlyParallel) => {
                    // Lightweight item, root, strict → suppressed.
                }
                (None, true, _) => {
                    // Heavyweight at root — let the framework walker
                    // handle it via natural descendant emission. No
                    // entry in our logical tree.
                }
            }
        }

        // Ad-hoc widget relocations addressed by `WidgetId`
        // (rare — typically a descendant of a heavyweight scene
        // item that should belong elsewhere logically). Widgets
        // referenced via `A11yNode::Item(item_id)` are already
        // handled by the visible-entries pass.
        for (child_node, parent_node) in &self.scene().a11y_parents {
            let A11yNode::Widget(wid) = child_node else {
                continue;
            };
            // A relocation naming a card root directly (rather than through
            // its `ItemId`) is gated by the same published set as the pass
            // above, so the two addresses for one card cannot disagree.
            //
            // A relocation naming a widget *inside* a card cannot be checked
            // here — this method has no arena, and the widget's activation is
            // its ancestor card's. It does not need to be: if that card is
            // parked, or merely left out of `accessibility_children`, the
            // walker never reaches the descendant, nothing is emitted for it,
            // and
            // `WidgetTree::sync_accessibility`'s dangling-child strip drops
            // the reference. That strip is the general guarantee; this check
            // is the specific one, and both are pinned by tests.
            if let Some(item_id) = self.widget_to_item.get(wid)
                && !publishes_heavy(*item_id)
            {
                continue;
            }
            logical_children
                .entry(Some(*parent_node))
                .or_default()
                .push(*child_node);
        }

        // Walk the logical tree DFS, depth-first, emitting synthetic
        // NodeIds. Cycle guard: a node visited twice (the result of
        // a malformed `set_a11y_parent(A, B); set_a11y_parent(B, A)`
        // pairing) is skipped on its second appearance.
        let mut visited: HashSet<A11yNode> = HashSet::new();
        let roots = logical_children.get(&None).cloned().unwrap_or_default();
        for root in roots {
            self.emit_logical_node(builder, root, None, &logical_children, &mut visited);
        }

        // Apply cross-tree decorations (relations / live
        // regions / landmarks). Items / groups must already be in
        // `children_collected` for the writes to land on the right
        // node. Heavyweight widgets are not yet routed through here
        // — the walker can't decorate widget-derived NodeIds from a
        // sibling's accessibility() impl. Apps that need to point
        // a `flow_to`/`controls` at a widget should use the
        // synthetic NodeIds (decorating widgets is part of
        // the deferred auto-graft work).
        let owner = builder.owner_id();
        let resolve = |node: A11yNode| -> Option<accesskit::NodeId> {
            match node {
                A11yNode::Item(id) => {
                    owner.map(|o| synthetic_node_id(o, id.as_u64(), SyntheticKind::SceneItem))
                }
                A11yNode::Group(id) => {
                    owner.map(|o| synthetic_node_id(o, id.as_u64(), SyntheticKind::SceneGroup))
                }
                A11yNode::Widget(id) => Some(teksilo_core::accessibility::widget_id_to_node_id(id)),
            }
        };
        for (from, kind, to) in self.scene().a11y_relations() {
            let (Some(from_id), Some(to_id)) = (resolve(*from), resolve(*to)) else {
                continue;
            };
            self.apply_relation_to_collected(builder, from_id, *kind, to_id);
        }
        for (node, live) in &self.scene().a11y_live {
            let Some(id) = resolve(*node) else { continue };
            self.set_collected_live(builder, id, *live);
        }
        for (node, role) in &self.scene().a11y_landmarks {
            let Some(id) = resolve(*node) else { continue };
            self.set_collected_role(builder, id, *role);
        }

        // Magnetism: in keyboard connect mode, point the SceneView's
        // `active_descendant` at the focused magnet's synthetic node (the
        // roving virtual-focus pattern — the SceneView keeps real arena
        // focus while the screen reader announces the focused magnet).
        // Only when that magnet was actually emitted (its item is a
        // visible lightweight item), so the target node exists this frame.
        if self.magnet_connect_mode.get()
            && let (Some(mid), Some(o)) = (self.magnet_focus.get(), owner)
        {
            let scene = self.scene();
            if let Some(owner_item) = scene.magnet_owner(mid)
                && scene.item(owner_item).is_some()
                && visible_item_ids.contains(&owner_item)
                && scene.magnet_enabled(mid)
            {
                builder.set_active_descendant(synthetic_node_id(
                    o,
                    mid.as_u64(),
                    SyntheticKind::SceneMagnet,
                ));
            }
        }
    }

    /// Recursive DFS step: emit one node of the logical tree under
    /// `parent_id` (`None` = SceneView's own node), then descend.
    /// Cycle-guards via `visited`; the same node visited twice is
    /// skipped on the second appearance, so a malformed parent
    /// declaration doesn't infinite-loop the walker.
    fn emit_logical_node(
        &self,
        builder: &mut teksilo_core::accessibility::AccessNodeBuilder,
        node: crate::a11y::A11yNode,
        parent_id: Option<accesskit::NodeId>,
        logical_children: &std::collections::HashMap<
            Option<crate::a11y::A11yNode>,
            Vec<crate::a11y::A11yNode>,
        >,
        visited: &mut std::collections::HashSet<crate::a11y::A11yNode>,
    ) {
        use crate::a11y::A11yNode;
        use teksilo_core::accessibility::SyntheticKind;

        if !visited.insert(node) {
            return;
        }

        let view_transform = self.view_transform();
        let scene = self.model.0.borrow();
        let synthetic_id = match node {
            A11yNode::Item(item_id) => {
                // Discriminate by entry kind: lightweight items
                // emit a synthetic AT node; heavyweight items
                // attach the framework-emitted widget node under
                // the declared parent (auto-graft).
                if let Some(item) = scene.item(item_id) {
                    let _ = item; // borrowed below for accessibility() call
                    let scene_bounds = scene.scene_rect(item_id).unwrap_or(Rect::ZERO);
                    let ctx = crate::item::SceneItemA11yContext {
                        view_transform,
                        scene_bounds,
                        item_id,
                        local_to_scene: scene.scene_transform(item_id),
                    };
                    builder.push_scene_child_under(
                        parent_id,
                        item_id.as_u64(),
                        SyntheticKind::SceneItem,
                        |child| {
                            item.accessibility(child, &ctx);
                            child.inner_mut().set_bounds(accesskit::Rect {
                                x0: scene_bounds.x as f64,
                                y0: scene_bounds.y as f64,
                                x1: (scene_bounds.x + scene_bounds.width) as f64,
                                y1: (scene_bounds.y + scene_bounds.height) as f64,
                            });
                            Self::declare_scene_space(child, parent_id, view_transform);
                        },
                    )
                } else if let Some(&widget_id) = self.materialized.get(&item_id) {
                    // Heavyweight scene entry — auto-graft.
                    let Some(parent) = parent_id else {
                        debug_assert!(
                            false,
                            "auto-graft requires a declared parent — root \
                             heavyweight items emit through the framework walker"
                        );
                        return;
                    };
                    let widget_node_id =
                        teksilo_core::accessibility::widget_id_to_node_id(widget_id);
                    builder.attach_scene_child_under(parent, widget_node_id);
                    widget_node_id
                } else {
                    // Item id not found — Scene was mutated between
                    // logical-tree population and emit. Skip.
                    return;
                }
            }
            A11yNode::Group(group_id) => {
                let Some(group) = scene.a11y_group(group_id) else {
                    return;
                };
                let role = group.role;
                let label = group.label.clone();
                builder.push_scene_child_under(
                    parent_id,
                    group_id.as_u64(),
                    SyntheticKind::SceneGroup,
                    |child| {
                        child.set_role(role);
                        if let Some(label) = label {
                            child.set_name(label.resolve_now());
                        }
                        // A group carries no box of its own, but it does carry
                        // the space: anything grafted under it — an item, or a
                        // widget relocated by `set_a11y_parent` — states its
                        // rectangle in scene coordinates and inherits the
                        // projection from here.
                        Self::declare_scene_space(child, parent_id, view_transform);
                    },
                )
            }
            A11yNode::Widget(widget_id) => {
                // Auto-graft: the widget's full AT node is emitted
                // by the framework walker as part of the recursive
                // descent. Here we only need to add its NodeId to
                // the declared parent's children list. The
                // redirect hook (`a11y_redirect_descendant`) tells
                // the walker to skip its own push, so the widget
                // appears exactly once — under its declared
                // logical parent.
                //
                // Widgets at the logical-tree root (parent_id =
                // None) should never get here: the population
                // pass only adds widgets when their parent is
                // declared. Bail on that path so we don't
                // double-attach.
                let Some(parent) = parent_id else {
                    debug_assert!(
                        false,
                        "auto-graft requires a declared parent — root widgets emit \
                         through the framework walker as natural descendants"
                    );
                    return;
                };
                let widget_node_id = teksilo_core::accessibility::widget_id_to_node_id(widget_id);
                builder.attach_scene_child_under(parent, widget_node_id);
                widget_node_id
            }
        };

        // Magnetism: emit each enabled magnet of a lightweight item as a
        // synthetic `SceneMagnet` child of the item's node, so the anchor
        // is screen-reader perceivable and can be the `active_descendant`
        // target during the keyboard connect flow. Only for lightweight
        // items (whose synthetic node is in `children_collected` and so
        // can parent a child); heavyweight-item magnets are a follow-up.
        if self.magnetism_active().is_some()
            && let A11yNode::Item(item_id) = node
            && scene.item(item_id).is_some()
        {
            for mid in scene.magnet_ids_of(item_id) {
                if !scene.magnet_enabled(mid) {
                    continue;
                }
                let Some(scene_pos) = scene.magnet_scene_pos(mid) else {
                    continue;
                };
                let name = scene
                    .magnet_label(mid)
                    .map(|l| l.resolve_now())
                    .unwrap_or_else(|| "Connection point".to_string());
                // A small AT box around the anchor so the node has non-zero
                // bounds for hit-test / focus-ring chrome. Scene coordinates,
                // like every other rectangle this walk emits — and sized so
                // that the box stays MAGNET_AT_HALF_EXTENT logical pixels on
                // screen at any zoom, because it is a pointer affordance and
                // not a piece of the drawing.
                let (cx, cy) = (scene_pos.x, scene_pos.y);
                let half = MAGNET_AT_HALF_EXTENT / view_scale(&view_transform);
                builder.push_scene_child_under(
                    Some(synthetic_id),
                    mid.as_u64(),
                    SyntheticKind::SceneMagnet,
                    |child| {
                        child.set_role(accesskit::Role::Button);
                        child.set_name(name);
                        child.inner_mut().set_bounds(accesskit::Rect {
                            x0: (cx - half) as f64,
                            y0: (cy - half) as f64,
                            x1: (cx + half) as f64,
                            y1: (cy + half) as f64,
                        });
                    },
                );
            }
        }

        if let Some(children) = logical_children.get(&Some(node)) {
            for child in children {
                self.emit_logical_node(
                    builder,
                    *child,
                    Some(synthetic_id),
                    logical_children,
                    visited,
                );
            }
        }
    }

    /// Apply an `A11yRelation` to the synthetic node identified by
    /// `from_id` in the builder's collected children. No-op (with
    /// debug-assert) if `from_id` isn't found — the relation source
    /// must have been emitted into the logical tree first.
    fn apply_relation_to_collected(
        &self,
        builder: &mut teksilo_core::accessibility::AccessNodeBuilder,
        from_id: accesskit::NodeId,
        kind: crate::a11y::A11yRelation,
        to_id: accesskit::NodeId,
    ) {
        use crate::a11y::A11yRelation;
        builder.with_collected_node(from_id, |node| match kind {
            A11yRelation::Controls => node.push_controlled(to_id),
            A11yRelation::DescribedBy => node.push_described_by(to_id),
            A11yRelation::LabelledBy => node.push_labelled_by(to_id),
            A11yRelation::FlowTo => node.push_flow_to(to_id),
        });
    }

    fn set_collected_live(
        &self,
        builder: &mut teksilo_core::accessibility::AccessNodeBuilder,
        node_id: accesskit::NodeId,
        live: accesskit::Live,
    ) {
        builder.with_collected_node(node_id, |node| {
            node.set_live(live);
        });
    }

    fn set_collected_role(
        &self,
        builder: &mut teksilo_core::accessibility::AccessNodeBuilder,
        node_id: accesskit::NodeId,
        role: accesskit::Role,
    ) {
        builder.with_collected_node(node_id, |node| {
            node.set_role(role);
        });
    }
}
