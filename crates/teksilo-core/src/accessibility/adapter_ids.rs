// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The ids a platform adapter is handed: a node that left the tree a reader
//! sees comes back under an id the adapter has never had.
//!
//! A node's AccessKit id is a function of what it is: a widget's id comes from
//! its `WidgetId`, a synthetic node's from its owner and element. So a tab
//! page, a menu or a combo list parked dormant and shown again, a subtree
//! hidden and shown, a row scrolled out of its clipping parent and back, all
//! return under the ids they left with. `accesskit_atspi_common` announces a
//! node that leaves the filtered tree as defunct (`adapter.rs:91-106`,
//! `remove_node`, reached from `adapter.rs:280-349` when the node is gone from
//! the update or stops passing `common_filter`), and nothing unsays it when
//! the id is added again (`add_node`, `adapter.rs:49-80`). libatspi keeps the
//! state for the path, since it rejects the cache signals `accesskit_unix`
//! 0.23 sends (their D-Bus signature is not one it parses), and Orca 46.1
//! drops every event whose source is defunct (`event_manager.py:796-798`).
//! `tools/reader/` measured the result: every control a reader had met went
//! silent once it left and came back.
//!
//! So the tree's own ids stay what they are, for everything that reads them in
//! process (the automation bridge, the announcement ring, tests), and the
//! update handed to a platform adapter is translated on its way out. The
//! translation replays every update it hands out through `accesskit_consumer`
//! with the adapter's own rules, which tells it every id the adapter removed. A
//! node whose id was removed is given a new one the next time it is in an
//! update (while the window is in the background, in the very update that
//! removed it, see [`AdapterIds::deliver`]), and keeps that one while the
//! adapter holds it. A node's first appearance keeps the tree's own id, so a
//! tree nothing ever left is handed out unchanged. Action requests come back
//! through the same map.
//!
//! The UIA and macOS adapters have no defunct state (`accesskit_windows`
//! resolves a node by its id on every call, `node.rs`, `resolve`, and
//! `accesskit_macos` makes a new platform object for a node added again,
//! `context.rs`, `get_or_create_platform_node`), so a new id changes nothing a
//! reader can tell there, and the translation is the same on every platform.

use std::collections::{HashMap, HashSet};

use accesskit::{ActionData, ActionRequest, Node, NodeId, TextSelection, TreeId, TreeUpdate};
use accesskit_consumer::{FilterResult, NodeRef, Tree, TreeChangeHandler, common_filter};

/// Below this many recorded ids, the translation does not look for the ones
/// whose widget is gone.
const PRUNE_FLOOR: usize = 4096;

/// How many times one delivery replays its update. A second pass renames
/// the nodes the first took out of the adapter's view, and takes none out
/// itself; the bound only keeps a consumer surprise from looping.
const MAX_PASSES: usize = 3;

/// The id handed out in place of a node's own, `n` counting from zero.
///
/// Every one has an even, non-zero upper half and the top bit clear. A
/// widget's id has an odd upper half (slotmap keeps an occupied slot's
/// version odd, `KeyData::from_ffi`), a synthetic id the top bit set, and the
/// root and the announcer's ids an upper half of zero, so none of the tree's
/// own ids is ever one of these.
fn reissued_id(n: u64) -> NodeId {
    let high = 2 + 2 * (n >> 32);
    NodeId((high << 32) | (n & 0xFFFF_FFFF))
}

/// Whether `id` is one [`reissued_id`] hands out.
fn is_reissued_id(id: NodeId) -> bool {
    let high = id.0 >> 32;
    high != 0 && high & 1 == 0 && id.0 & crate::accessibility::SYNTHETIC_BIT == 0
}

/// The translation between the tree's ids and the ones a platform adapter
/// holds. See the [module documentation](self).
pub(crate) struct AdapterIds {
    /// The adapter's tree, replayed from what it was handed. `None` before the
    /// first update, and after one the consumer could not apply.
    replay: Option<Tree>,
    /// Whether the adapter was last told its window has focus. A focused node
    /// passes `common_filter` whatever it is, so losing window focus can
    /// remove one.
    window_focused: bool,
    /// The tree's id of each node handed out under another, and that other.
    reissued: HashMap<NodeId, NodeId>,
    /// The same pairs, the other way round.
    own: HashMap<NodeId, NodeId>,
    /// The tree's ids of the nodes whose last id the adapter has removed.
    /// Each is handed a new one the next time it is in an update.
    removed: HashSet<NodeId>,
    /// How many ids [`reissued_id`] has handed out.
    next: u64,
    /// The last update handed out.
    last: Option<TreeUpdate>,
    /// Whether `last` may no longer be what would be handed out now: the
    /// tree's own update changed, or the window's focus took a node out of
    /// the adapter's view since.
    outdated: bool,
    /// How many recorded ids it takes to look for dead ones.
    prune_at: usize,
}

impl AdapterIds {
    pub(crate) fn new() -> Self {
        Self {
            replay: None,
            window_focused: true,
            reissued: HashMap::new(),
            own: HashMap::new(),
            removed: HashSet::new(),
            next: 0,
            last: None,
            outdated: true,
            prune_at: PRUNE_FLOOR,
        }
    }

    /// The tree's own update changed since the last one handed out.
    pub(crate) fn source_changed(&mut self) {
        self.outdated = true;
    }

    /// `source` as the adapter must be handed it.
    ///
    /// `window_focused` is what the adapter was last told about its window's
    /// focus (winit's `Focused`, which `accesskit_winit` forwards as it
    /// arrives). `alive` says whether a node of the tree's own id can still
    /// appear: an id whose widget is gone never comes back, and its record is
    /// dropped once enough have piled up.
    pub(crate) fn deliver(
        &mut self,
        source: &TreeUpdate,
        window_focused: bool,
        alive: impl Fn(NodeId) -> bool,
    ) -> TreeUpdate {
        if window_focused != self.window_focused {
            self.window_focused = window_focused;
            let replay = self.replay.take();
            let removed = self.replayed(replay, |tree, rules| {
                tree.update_host_focus_state_and_process_changes(window_focused, rules);
            });
            // A node that left is still in the update: it is handed its new
            // id below, whether or not anything else changed.
            self.outdated |= self.retire(removed);
        }
        if !self.outdated
            && let Some(last) = &self.last
        {
            return last.clone();
        }

        // A node the adapter removed is back: hand it an id the adapter has
        // never had. One still excluded (hidden, scrolled out) is handed its
        // new id now as well; the adapter adds it only when it passes the
        // filter again, and it is new then.
        self.reissue(source);
        let mut update = if self.reissued.is_empty() {
            source.clone()
        } else {
            self.translated(source)
        };
        // A node this update takes out of the adapter's view and still carries
        // (hidden, scrolled out of its clipping parent) comes back through a
        // later update, and is handed its new id there. Except while the
        // window is in the background: the adapter then adds the focused node
        // back by itself the moment the window has focus again, with no update
        // in between, since `common_filter` passes a focused node whatever it
        // is and the adapter hears of the focus from winit as it happens. So
        // in the background such a node is renamed in this same update.
        let mut pending = false;
        for pass in 1..=MAX_PASSES {
            let removed = self.replay_handed(&update, window_focused);
            if !self.retire(removed) || window_focused {
                break;
            }
            if pass == MAX_PASSES {
                pending = true;
                break;
            }
            if !self.reissue(source) {
                break;
            }
            update = self.translated(source);
        }
        self.last = Some(update.clone());
        self.outdated = pending;
        self.prune(alive);
        update
    }

    /// Give every node of `source` whose id the adapter removed a new one.
    /// Whether there was any.
    fn reissue(&mut self, source: &TreeUpdate) -> bool {
        if self.removed.is_empty() {
            return false;
        }
        let root = source.tree.as_ref().map(|tree| tree.root);
        let mut any = false;
        for (id, _) in &source.nodes {
            if Some(*id) == root || !self.removed.remove(id) {
                continue;
            }
            let fresh = reissued_id(self.next);
            self.next += 1;
            if let Some(previous) = self.reissued.insert(*id, fresh) {
                self.own.remove(&previous);
            }
            self.own.insert(fresh, *id);
            any = true;
        }
        any
    }

    /// Replay `update` as the adapter applies it, and return the ids it
    /// removed.
    fn replay_handed(&mut self, update: &TreeUpdate, window_focused: bool) -> Vec<NodeId> {
        let replay = self.replay.take();
        let handed = update.clone();
        let removed = self.replayed(replay, move |tree, rules| {
            tree.update_and_process_changes(handed, rules);
        });
        if self.replay.is_none() && update.tree.is_some() {
            // The first update, or the first after one the consumer rejected:
            // the adapter's tree is this one from here on.
            let seed = update.clone();
            self.replay = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Tree::new(seed, window_focused)
            }))
            .ok();
        }
        removed
    }

    /// Run `apply` on the replay, if there is one, and return the ids the
    /// adapter removed. The consumer panics on an update it cannot apply,
    /// exactly as it would inside the adapter; the replay is dropped then, and
    /// the update being handed out seeds a new one.
    fn replayed(
        &mut self,
        replay: Option<Tree>,
        apply: impl FnOnce(&mut Tree, &mut Removals),
    ) -> Vec<NodeId> {
        let Some(mut tree) = replay else {
            return Vec::new();
        };
        let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut rules = Removals::default();
            apply(&mut tree, &mut rules);
            (tree, rules.ids)
        }));
        match applied {
            Ok((tree, ids)) => {
                self.replay = Some(tree);
                ids
            }
            Err(_) => Vec::new(),
        }
    }

    /// Record the ids the adapter removed, by the tree's own ids. Whether any
    /// node is newly recorded.
    fn retire(&mut self, handed: Vec<NodeId>) -> bool {
        let mut any = false;
        for id in handed {
            let own = match self.own.get(&id) {
                Some(own) => *own,
                // A new id that has since been replaced: its node already
                // has a later one.
                None if is_reissued_id(id) => continue,
                None => id,
            };
            // The announcer speaks every message from an id it never uses
            // again, so there is nothing to hand out when one comes back.
            if crate::announcer::is_announcer_node(own) {
                continue;
            }
            any |= self.removed.insert(own);
        }
        any
    }

    /// Drop the records of nodes that cannot appear again: a widget that is
    /// gone. Not a synthetic node, whose owner this cannot see, nor any node
    /// the adapter still holds.
    fn prune(&mut self, alive: impl Fn(NodeId) -> bool) {
        let recorded = self.removed.len() + self.reissued.len();
        if recorded < self.prune_at {
            return;
        }
        let held = |handed: NodeId| {
            self.replay.as_ref().is_some_and(|tree| {
                tree.state()
                    .node_by_tree_local_id(handed, TreeId::ROOT)
                    .is_some()
            })
        };
        let reissued = &self.reissued;
        let keep = |own: NodeId| {
            let handed = reissued.get(&own).copied().unwrap_or(own);
            alive(own) || held(handed)
        };
        self.removed.retain(|own| keep(*own));
        let dead: Vec<NodeId> = self
            .reissued
            .keys()
            .copied()
            .filter(|own| !keep(*own))
            .collect();
        for own in dead {
            if let Some(handed) = self.reissued.remove(&own) {
                self.own.remove(&handed);
            }
        }
        self.prune_at = PRUNE_FLOOR.max(2 * (self.removed.len() + self.reissued.len()));
    }

    /// `source` with every node id rewritten to the one the adapter holds.
    fn translated(&self, source: &TreeUpdate) -> TreeUpdate {
        let map = |id: NodeId| self.reissued.get(&id).copied().unwrap_or(id);
        TreeUpdate {
            nodes: source
                .nodes
                .iter()
                .map(|(id, node)| (map(*id), self.translated_node(node)))
                .collect(),
            tree: source.tree.clone().map(|mut tree| {
                tree.root = map(tree.root);
                tree
            }),
            tree_id: source.tree_id,
            focus: map(source.focus),
        }
    }

    /// `node` with every id it refers to rewritten. Every property of a node
    /// that holds a `NodeId` in AccessKit 0.25: eight lists, seven single ids
    /// and the text selection's two ends.
    fn translated_node(&self, node: &Node) -> Node {
        let mut node = node.clone();
        let moved = |ids: &[NodeId]| -> Option<Vec<NodeId>> {
            ids.iter()
                .any(|id| self.reissued.contains_key(id))
                .then(|| {
                    ids.iter()
                        .map(|id| self.reissued.get(id).copied().unwrap_or(*id))
                        .collect()
                })
        };
        let one = |id: Option<NodeId>| id.and_then(|id| self.reissued.get(&id).copied());
        macro_rules! lists {
            ($(($get:ident, $set:ident)),* $(,)?) => {$(
                if let Some(ids) = moved(node.$get()) {
                    node.$set(ids);
                }
            )*};
        }
        macro_rules! singles {
            ($(($get:ident, $set:ident)),* $(,)?) => {$(
                if let Some(id) = one(node.$get()) {
                    node.$set(id);
                }
            )*};
        }
        lists!(
            (children, set_children),
            (controls, set_controls),
            (details, set_details),
            (described_by, set_described_by),
            (flow_to, set_flow_to),
            (labelled_by, set_labelled_by),
            (owns, set_owns),
            (radio_group, set_radio_group),
        );
        singles!(
            (active_descendant, set_active_descendant),
            (error_message, set_error_message),
            (in_page_link_target, set_in_page_link_target),
            (member_of, set_member_of),
            (next_on_line, set_next_on_line),
            (previous_on_line, set_previous_on_line),
            (popup_for, set_popup_for),
        );
        if let Some(selection) = node.text_selection().copied()
            && (self.reissued.contains_key(&selection.anchor.node)
                || self.reissued.contains_key(&selection.focus.node))
        {
            let mut selection: TextSelection = selection;
            selection.anchor.node = self
                .reissued
                .get(&selection.anchor.node)
                .copied()
                .unwrap_or(selection.anchor.node);
            selection.focus.node = self
                .reissued
                .get(&selection.focus.node)
                .copied()
                .unwrap_or(selection.focus.node);
            node.set_text_selection(selection);
        }
        node
    }

    /// The tree's own id for an id the adapter holds. `None` for an id handed
    /// out once and since replaced, which names no node any more.
    pub(crate) fn own_id(&self, handed: NodeId) -> Option<NodeId> {
        match self.own.get(&handed) {
            Some(own) => Some(*own),
            None if is_reissued_id(handed) => None,
            None => Some(handed),
        }
    }

    /// `request` with the ids in it back in the tree's own. `None` when it
    /// targets an id that names no node any more.
    pub(crate) fn own_request(&self, mut request: ActionRequest) -> Option<ActionRequest> {
        request.target_node = self.own_id(request.target_node)?;
        if let Some(ActionData::SetTextSelection(selection)) = &mut request.data {
            selection.anchor.node = self.own_id(selection.anchor.node)?;
            selection.focus.node = self.own_id(selection.focus.node)?;
        }
        Some(request)
    }
}

/// The ids `accesskit_atspi_common` removes, by its own rules
/// (`adapter.rs:280-349`): a node that passed the filter and is gone from the
/// update, or stops passing it; when it stops because it or an ancestor is
/// hidden, its whole filtered subtree with it.
#[derive(Default)]
struct Removals {
    ids: Vec<NodeId>,
}

impl Removals {
    fn remove_subtree(&mut self, node: &NodeRef) {
        for child in node.filtered_children(&common_filter) {
            self.remove_subtree(&child);
        }
        self.ids.push(node.locate().0);
    }
}

impl TreeChangeHandler for Removals {
    fn node_added(&mut self, _node: &NodeRef) {}

    fn node_updated(&mut self, old: &NodeRef, new: &NodeRef) {
        if common_filter(old) != FilterResult::Include {
            return;
        }
        match common_filter(new) {
            FilterResult::Include => {}
            FilterResult::ExcludeSubtree => self.remove_subtree(old),
            FilterResult::ExcludeNode => self.ids.push(old.locate().0),
        }
    }

    fn focus_moved(&mut self, _old: Option<&NodeRef>, _new: Option<&NodeRef>) {}

    fn node_removed(&mut self, node: &NodeRef) {
        if common_filter(node) == FilterResult::Include {
            self.ids.push(node.locate().0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Role, TextPosition, TreeInfo};

    fn update(nodes: Vec<(NodeId, Node)>, focus: NodeId) -> TreeUpdate {
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(NodeId(0))),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    fn window(children: &[NodeId]) -> Node {
        let mut node = Node::new(Role::Window);
        node.set_children(children.to_vec());
        node
    }

    fn button() -> Node {
        Node::new(Role::Button)
    }

    /// A widget's id, as `widget_id_to_node_id` makes one: version 1.
    const BUTTON: NodeId = NodeId((1 << 32) | 1);
    const OTHER: NodeId = NodeId((1 << 32) | 2);

    #[test]
    fn no_id_the_tree_makes_is_one_handed_out_in_its_place() {
        for n in [0, 1, 0xFFFF_FFFF, 1 << 32, (1 << 40) + 7] {
            let id = reissued_id(n);
            assert!(is_reissued_id(id), "{n}");
            assert_eq!(id.0 >> 32 & 1, 0, "an even upper half");
            assert!(!crate::accessibility::is_synthetic(id));
            assert!(!crate::announcer::is_announcer_node(id));
        }
        assert!(!is_reissued_id(BUTTON));
        assert!(!is_reissued_id(NodeId(0)));
        assert!(!is_reissued_id(NodeId(7)), "an announcer id");
        assert!(!is_reissued_id(NodeId(
            crate::accessibility::SYNTHETIC_BIT | (2 << 32)
        )));
        assert_ne!(reissued_id(0), reissued_id(1 << 32));
    }

    /// A node that left and came back is handed a new id, every property that
    /// names it follows, and an action on the new id reaches the node.
    #[test]
    fn a_returning_node_is_named_by_its_new_id_everywhere() {
        let mut ids = AdapterIds::new();
        let alive = |_| true;
        let both = update(
            vec![
                (NodeId(0), window(&[BUTTON, OTHER])),
                (BUTTON, button()),
                (OTHER, button()),
            ],
            BUTTON,
        );
        assert_eq!(ids.deliver(&both, true, alive), both, "a first appearance");

        ids.source_changed();
        let gone = update(
            vec![(NodeId(0), window(&[OTHER])), (OTHER, button())],
            OTHER,
        );
        assert_eq!(ids.deliver(&gone, true, alive), gone);

        let mut other = button();
        other.set_controls(vec![BUTTON]);
        other.set_labelled_by(vec![OTHER, BUTTON]);
        other.set_active_descendant(BUTTON);
        other.set_popup_for(BUTTON);
        let position = TextPosition {
            node: BUTTON,
            character_index: 2,
        };
        other.set_text_selection(TextSelection {
            anchor: position,
            focus: position,
        });
        ids.source_changed();
        let back = update(
            vec![
                (NodeId(0), window(&[BUTTON, OTHER])),
                (BUTTON, button()),
                (OTHER, other),
            ],
            BUTTON,
        );
        let handed = ids.deliver(&back, true, alive);
        let fresh = handed.focus;
        assert!(is_reissued_id(fresh), "the returning button has a new id");
        assert_eq!(handed.nodes[0].1.children(), &[fresh, OTHER]);
        assert_eq!(handed.nodes[1].0, fresh);
        let other = &handed.nodes[2].1;
        assert_eq!(other.controls(), &[fresh]);
        assert_eq!(other.labelled_by(), &[OTHER, fresh]);
        assert_eq!(other.active_descendant(), Some(fresh));
        assert_eq!(other.popup_for(), Some(fresh));
        let selection = other.text_selection().expect("the selection");
        assert_eq!(
            (selection.anchor.node, selection.focus.node),
            (fresh, fresh)
        );

        let request = ids
            .own_request(ActionRequest {
                action: accesskit::Action::SetTextSelection,
                target_tree: TreeId::ROOT,
                target_node: fresh,
                data: Some(ActionData::SetTextSelection(TextSelection {
                    anchor: TextPosition {
                        node: fresh,
                        character_index: 0,
                    },
                    focus: TextPosition {
                        node: OTHER,
                        character_index: 1,
                    },
                })),
            })
            .expect("the id names a node");
        assert_eq!(request.target_node, BUTTON);
        let Some(ActionData::SetTextSelection(selection)) = request.data else {
            panic!("the selection is kept");
        };
        assert_eq!(
            (selection.anchor.node, selection.focus.node),
            (BUTTON, OTHER)
        );
        assert_eq!(ids.own_id(OTHER), Some(OTHER), "an id handed out as itself");
    }

    /// A node that leaves twice gets a new id each time, and the one it
    /// replaced names nothing: an action on it is dropped.
    #[test]
    fn a_replaced_id_names_no_node() {
        let mut ids = AdapterIds::new();
        let alive = |_| true;
        let with = update(
            vec![(NodeId(0), window(&[BUTTON])), (BUTTON, button())],
            NodeId(0),
        );
        let without = update(vec![(NodeId(0), window(&[]))], NodeId(0));
        let mut handed = Vec::new();
        for _ in 0..3 {
            ids.source_changed();
            handed.push(ids.deliver(&with, true, alive).nodes[1].0);
            ids.source_changed();
            ids.deliver(&without, true, alive);
        }
        assert_eq!(handed[0], BUTTON);
        assert!(handed[1] != handed[2] && is_reissued_id(handed[1]) && is_reissued_id(handed[2]));
        assert_eq!(ids.own_id(handed[1]), None);
        assert_eq!(ids.own_id(handed[2]), Some(BUTTON));
    }

    /// The records of widgets that are gone are dropped once enough have piled
    /// up; those of widgets still alive are kept, since those can come back.
    #[test]
    fn records_of_widgets_that_are_gone_are_dropped() {
        let mut ids = AdapterIds::new();
        let count = PRUNE_FLOOR as u64 + 10;
        let widget = |n: u64| NodeId((1 << 32) | (n + 1));
        let all: Vec<NodeId> = (0..count).map(widget).collect();
        let mut nodes = vec![(NodeId(0), window(&all))];
        nodes.extend(all.iter().map(|id| (*id, button())));
        ids.deliver(&update(nodes, NodeId(0)), true, |_| true);
        ids.source_changed();
        ids.deliver(
            &update(vec![(NodeId(0), window(&[]))], NodeId(0)),
            true,
            |id| id == widget(0),
        );
        assert_eq!(
            ids.removed.iter().copied().collect::<Vec<_>>(),
            vec![widget(0)],
            "only the widget still alive can come back"
        );
    }

    /// Nothing changed and the window's focus did not either: the last update
    /// is handed out again without replaying anything.
    #[test]
    fn an_unchanged_tree_is_handed_out_as_it_was() {
        let mut ids = AdapterIds::new();
        let tree = update(
            vec![(NodeId(0), window(&[BUTTON])), (BUTTON, button())],
            BUTTON,
        );
        let first = ids.deliver(&tree, true, |_| true);
        let other = update(vec![(NodeId(0), window(&[]))], NodeId(0));
        assert_eq!(
            ids.deliver(&other, true, |_| true),
            first,
            "without a change of source, the last update stands"
        );
    }
}
