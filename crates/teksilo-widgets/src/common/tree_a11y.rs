// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a tree view's rows hang in the accessibility tree.
//!
//! A tree view realizes a contiguous window of its flattened rows, and lays
//! them out as siblings. For a screen reader that shape is wrong twice over:
//! NVDA counts a tree item's level from the tree items above it, so a flat
//! list reads every row as level 1, and every adapter reads an item's "of M"
//! from the first node above it that carries a set size, so flat rows share
//! one count whatever their depth.
//!
//! So on Windows and Linux each row hangs from its parent row. When the
//! window starts inside a branch, the rows whose parent is scrolled away hang
//! from a stand-in for it ([`SyntheticKind::TreeAncestor`]), and so do the
//! stand-ins for that parent's own ancestors, up to a root: those rows keep
//! their true ancestor count and their own sibling set size.
//!
//! macOS keeps the flat shape, which is `NSOutlineView`'s: VoiceOver reads
//! the level from neither and AccessKit's macOS adapter publishes no set
//! size. A nested shape there would cost the outline's `AXRows`, since
//! AccessKit's container item queries see only the items directly under the
//! container ([`nest_tree_rows`] says more).
//!
//! [`SyntheticKind::TreeAncestor`]: teksilo_core::accessibility::SyntheticKind::TreeAncestor

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use teksilo_core::accessibility::{
    AccessNodeBuilder, SyntheticKind, synthetic_node_id, widget_id_to_node_id,
};
use teksilo_core::widget_id::WidgetId;

use crate::tree_source::TreeSource;

thread_local! {
    static NEST_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether tree views nest each row under its parent row in the
/// accessibility tree: on Windows and Linux, not on macOS.
///
/// Known limitation where it nests: AccessKit's `NodeRef::items` (behind
/// UIA's `SelectionPattern.GetSelection` and AT-SPI's `Selection`) walks only
/// the items directly under the container, so a selected nested row is not
/// listed until AccessKit descends into nested tree items.
///
/// Tests override it per thread (`NestOverride`, unit tests only), so
/// both shapes are tested on every host; integration tests and the reader
/// harness get the platform's shape.
pub(crate) fn nest_tree_rows() -> bool {
    NEST_OVERRIDE
        .with(Cell::get)
        .unwrap_or(cfg!(not(target_os = "macos")))
}

/// Forces [`nest_tree_rows`] on this thread until dropped.
#[cfg(test)]
pub(crate) struct NestOverride(Option<bool>);

#[cfg(test)]
impl NestOverride {
    pub(crate) fn new(nest: bool) -> Self {
        Self(NEST_OVERRIDE.with(|o| o.replace(Some(nest))))
    }
}

#[cfg(test)]
impl Drop for NestOverride {
    fn drop(&mut self) {
        NEST_OVERRIDE.with(|o| o.set(self.0));
    }
}

/// One realized row, as the planner needs it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealizedRow {
    /// Flat index.
    pub flat: usize,
    /// Whether the row's metadata resolved (it has a key, so a parent).
    pub resolved: bool,
    /// An expanded row with children.
    pub open_branch: bool,
}

/// What a realized row hangs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtParent {
    /// The view's own body node: a root row.
    Pane,
    /// The realized row with this flat index.
    Row(usize),
    /// The stand-in for the unrealized row with this flat index.
    Ancestor(usize),
}

/// Where every realized row hangs, and which ancestors need a stand-in.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct NestPlan {
    /// Per realized row, in flat order.
    pub parents: Vec<(usize, AtParent)>,
    /// The stand-ins, outermost first: an ancestor's flat index and the flat
    /// index of the stand-in it hangs from (`None`: the view's body node).
    pub ancestors: Vec<(usize, Option<usize>)>,
}

/// Plan the nesting of `rows` (in flat order).
///
/// `parent_index` is the source's key-based parent lookup. A row whose
/// metadata has not resolved (a `Loading` placeholder) has no knowable
/// parent: it hangs under the nearest realized row before it when that row
/// is an open branch (its children are what is loading), and beside that row
/// otherwise. Any placement implies an ancestor count, so its level as a
/// screen reader counts it is a best guess; it still publishes no level of
/// its own.
pub(crate) fn plan(
    rows: &[RealizedRow],
    parent_index: impl Fn(usize) -> Option<usize>,
) -> NestPlan {
    let realized: BTreeMap<usize, ()> = rows.iter().map(|r| (r.flat, ())).collect();
    let mut parents: Vec<(usize, AtParent)> = Vec::with_capacity(rows.len());
    let mut needed: BTreeMap<usize, Option<usize>> = BTreeMap::new();
    for (n, row) in rows.iter().enumerate() {
        let parent = if row.resolved {
            match parent_index(row.flat) {
                None => AtParent::Pane,
                Some(p) if realized.contains_key(&p) => AtParent::Row(p),
                Some(p) => {
                    // The window is contiguous and a parent precedes its
                    // children, so an unrealized parent precedes the window
                    // and so do all its ancestors: the whole chain needs
                    // stand-ins.
                    let mut at = Some(p);
                    while let Some(a) = at {
                        if needed.contains_key(&a) {
                            break;
                        }
                        let up = parent_index(a);
                        debug_assert!(
                            up.is_none_or(|u| !realized.contains_key(&u)),
                            "an unrealized row's parent is realized"
                        );
                        needed.insert(a, up);
                        at = up;
                    }
                    AtParent::Ancestor(p)
                }
            }
        } else {
            match n.checked_sub(1).map(|m| (rows[m], parents[m].1)) {
                Some((prev, _)) if prev.open_branch => AtParent::Row(prev.flat),
                Some((_, prev_parent)) => prev_parent,
                None => AtParent::Pane,
            }
        };
        parents.push((row.flat, parent));
    }
    NestPlan {
        parents,
        // A BTreeMap iterates by flat index, and an ancestor's index is
        // smaller than its descendants': outermost first.
        ancestors: needed.into_iter().collect(),
    }
}

/// What one row publishes for the rows below it, filled in by its pane once
/// every realized row exists. Shared by the row widget (`TreeItemWrapper`,
/// `TreeRowA11y`), which reads it in `accessibility` and
/// `accessibility_children`, so a row that rebuilds its own content keeps it.
#[derive(Debug, Default)]
pub(crate) struct RowAt {
    /// The rows that hang from this one, in flat order (nested shape only).
    pub kids: RefCell<Vec<WidgetId>>,
    /// How many children this row has, when it is an open branch. Read by
    /// its child rows as their "of M" (`size_of_set_from_container` reads
    /// a container's value, never an item's own), so every row with child
    /// rows under it must carry one, or they would read an ancestor's.
    pub size_of_set: Cell<Option<usize>>,
}

/// A stand-in for an unrealized ancestor row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StandIn {
    pub flat: usize,
    /// The stand-in it hangs from; `None`: the pane.
    pub parent: Option<usize>,
    pub level: Option<usize>,
    pub position: usize,
    pub size_of_set: usize,
    pub name: Option<String>,
}

/// What a tree view's body pane publishes about its rows, computed at build.
#[derive(Debug, Default)]
pub(crate) struct PaneAt {
    pub nested: bool,
    /// The number of root rows: the "of M" of a root row.
    pub root_count: usize,
    /// The pane's own row children (nested shape), in flat order.
    pub roots: Vec<WidgetId>,
    /// Rows hanging from a stand-in, and the stand-in's flat index.
    pub orphans: HashMap<WidgetId, usize>,
    pub orphan_order: Vec<WidgetId>,
    pub stand_ins: Vec<StandIn>,
}

/// One realized row as the pane hands it over: where it is, its widget (the
/// node carrying the row role), and the cell it publishes through.
pub(crate) struct PaneRow {
    pub row: RealizedRow,
    pub widget: WidgetId,
    pub at: Rc<RowAt>,
}

impl PaneAt {
    /// Plan the rows' placement and fill every row's [`RowAt`].
    pub(crate) fn build<T: 'static>(
        source: &TreeSource<T>,
        rows: &[PaneRow],
        label: Option<&dyn Fn(&T) -> String>,
        nested: bool,
    ) -> Self {
        let mut out = PaneAt {
            nested,
            root_count: source.root_count(),
            ..PaneAt::default()
        };
        let by_flat: HashMap<usize, &PaneRow> = rows.iter().map(|r| (r.row.flat, r)).collect();
        for r in rows {
            r.at.kids.borrow_mut().clear();
            r.at.size_of_set.set(None);
        }
        if nested {
            let realized: Vec<RealizedRow> = rows.iter().map(|r| r.row).collect();
            let plan = plan(&realized, |i| source.parent_index(i));
            for &(flat, parent) in &plan.parents {
                let widget = by_flat[&flat].widget;
                match parent {
                    AtParent::Pane => out.roots.push(widget),
                    AtParent::Row(p) => by_flat[&p].at.kids.borrow_mut().push(widget),
                    AtParent::Ancestor(a) => {
                        out.orphans.insert(widget, a);
                        out.orphan_order.push(widget);
                    }
                }
            }
            for &(flat, parent) in &plan.ancestors {
                let hanging = plan
                    .parents
                    .iter()
                    .filter(|(_, p)| *p == AtParent::Ancestor(flat))
                    .count()
                    + plan
                        .ancestors
                        .iter()
                        .filter(|(_, p)| *p == Some(flat))
                        .count();
                let visible = source.visible_child_count(flat);
                out.stand_ins.push(StandIn {
                    flat,
                    parent,
                    level: source.meta(flat).map(|m| m.depth + 1),
                    position: source
                        .visible_sibling_pos(flat)
                        .map_or(source.sibling_pos(flat).0, |(pos, _)| pos),
                    size_of_set: if visible > 0 { visible } else { hanging },
                    name: label.and_then(|f| source.with_row_str(flat, f)),
                });
            }
        }
        // Every row with child rows under it must carry their count, or they
        // read an ancestor's; so must every open branch, whose children may
        // simply not be realized. The count is of the VISIBLE children, in
        // the view's order: a filter hides some and a sort reorders them.
        for r in rows {
            let realized_kids = r.at.kids.borrow().len();
            if !r.row.open_branch && realized_kids == 0 {
                continue;
            }
            let count = match source.visible_child_count(r.row.flat) {
                0 if realized_kids > 0 => Some(realized_kids),
                0 => None,
                n => Some(n),
            };
            r.at.size_of_set.set(count);
        }
        out
    }

    /// The pane's own AT children: its root rows, then the rows that hang
    /// from a stand-in (listed so the walker builds them; the redirect hook
    /// places them). `None` in the flat shape: the arena order.
    pub(crate) fn accessibility_children(&self) -> Option<Vec<WidgetId>> {
        self.nested.then(|| {
            let mut v = self.roots.clone();
            v.extend(self.orphan_order.iter().copied());
            v
        })
    }

    pub(crate) fn wants_descendant_redirects(&self) -> bool {
        self.nested && !self.orphans.is_empty()
    }

    /// The stand-in a row hangs from, for the walker's redirect hook.
    pub(crate) fn redirect(
        &self,
        pane: WidgetId,
        descendant: WidgetId,
    ) -> Option<teksilo_core::accesskit::NodeId> {
        self.orphans
            .get(&descendant)
            .map(|&a| synthetic_node_id(pane, a as u64, SyntheticKind::TreeAncestor))
    }

    /// Publish the pane's set size and, in the nested shape, the stand-ins
    /// (as `row_role` nodes) with their rows attached. Called from the pane's
    /// `accessibility`, before the walker pushes the realized roots, so a
    /// stand-in reads before the rows below the one it stands for.
    pub(crate) fn publish(
        &self,
        builder: &mut AccessNodeBuilder,
        row_role: teksilo_core::accesskit::Role,
    ) {
        // The root rows' "of M", in both shapes (the set size ships on every
        // platform). In the flat shape (macOS) every row hangs from this
        // node, so a row below the top level reads it too; no macOS adapter
        // reads a set size today, and the root rows' count is right.
        if self.root_count > 0 {
            builder.set_size_of_set(self.root_count);
        }
        if !self.nested {
            return;
        }
        let mut ids: HashMap<usize, teksilo_core::accesskit::NodeId> = HashMap::new();
        for s in &self.stand_ins {
            let parent = s.parent.and_then(|p| ids.get(&p).copied());
            let nid = builder.push_scene_child_under(
                parent,
                s.flat as u64,
                SyntheticKind::TreeAncestor,
                |b| {
                    b.set_role(row_role);
                    if let Some(level) = s.level {
                        b.set_level(level);
                    }
                    b.set_position_in_set(s.position);
                    // Always: the rows below read their "of M" here.
                    b.set_size_of_set(s.size_of_set);
                    if let Some(ref name) = s.name {
                        b.set_name(name.clone());
                    }
                    // Expanded: it stands for an ancestor, open by
                    // construction. AccessKit gives every tree item UIA's
                    // ExpandCollapse pattern whatever its state, so with no
                    // state it would read as a leaf. Collapse is answered
                    // (`stand_in_actions`) by collapsing that ancestor.
                    b.set_expanded(true);
                    b.add_action(teksilo_core::accesskit::Action::Collapse);
                },
            );
            // A zero-size rectangle at the pane's top-leading corner,
            // declared in the pane's space so it follows the pane.
            builder.set_child_bounds_local(nid, teksilo_canvas::Rect::new(0.0, 0.0, 0.0, 0.0));
            ids.insert(s.flat, nid);
        }
        for &w in &self.orphan_order {
            if let Some(nid) = ids.get(&self.orphans[&w]) {
                builder.attach_scene_child_under(*nid, widget_id_to_node_id(w));
            }
        }
    }
}

/// The pane's handler for actions on its stand-ins: `Collapse` collapses
/// the ancestor the stand-in stands for, `Expand` succeeds (it is open).
/// `stand_ins` holds the flat indices of this build's stand-ins.
///
/// The ancestor is resolved through a [`RowAnchor`](crate::data_views::RowAnchor)
/// taken at build, as the rows' own handlers do: a structural change between
/// the build and the action (an insertion above, an expand elsewhere) moves
/// its flat index, and the action must reach the same row, or nothing when
/// it is gone. The stand-in's node id stays the one this build published.
pub(crate) fn stand_in_actions<T: 'static>(
    pane: WidgetId,
    source: Rc<TreeSource<T>>,
    stand_ins: Vec<usize>,
) -> teksilo_core::widget_builder::HandlerSet {
    use teksilo_core::accesskit::Action;
    use teksilo_core::event::EventResponse;
    let anchored: Vec<(
        teksilo_core::accesskit::NodeId,
        crate::data_views::RowAnchor,
    )> = stand_ins
        .into_iter()
        .map(|f| {
            (
                synthetic_node_id(pane, f as u64, SyntheticKind::TreeAncestor),
                source.anchor(f),
            )
        })
        .collect();
    teksilo_core::widget_builder::HandlerSet::new().on_access_action_request(
        move |action, node, _data, _ctx| {
            let Some((_, anchor)) = anchored.iter().find(|(id, _)| *id == node) else {
                return EventResponse::Ignored;
            };
            match action {
                Action::Collapse => {
                    if let Some(row) = anchor.index() {
                        source.set_expanded_at(row, false);
                    }
                    EventResponse::Handled
                }
                Action::Expand => EventResponse::Handled,
                _ => EventResponse::Ignored,
            }
        },
    )
}

/// A row widget's AT children in the nested shape: its own content, then
/// the rows hanging from it. `None` when no row hangs from it (the arena
/// order, its content).
pub(crate) fn row_accessibility_children(
    content: impl IntoIterator<Item = WidgetId>,
    at: &RowAt,
) -> Option<Vec<WidgetId>> {
    let kids = at.kids.borrow();
    if kids.is_empty() {
        return None;
    }
    let mut v: Vec<WidgetId> = content.into_iter().collect();
    v.extend(kids.iter().copied());
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree, flat order, as `(parent flat index)` per row:
    ///
    /// ```text
    /// 0 a
    /// 1   b
    /// 2     c
    /// 3     d
    /// 4   e
    /// 5 f
    /// ```
    fn parent_of(i: usize) -> Option<usize> {
        [None, Some(0), Some(1), Some(1), Some(0), None][i]
    }

    fn rows(range: std::ops::Range<usize>) -> Vec<RealizedRow> {
        range
            .map(|flat| RealizedRow {
                flat,
                resolved: true,
                open_branch: matches!(flat, 0 | 1),
            })
            .collect()
    }

    #[test]
    fn a_whole_tree_nests_by_parent() {
        let plan = plan(&rows(0..6), parent_of);
        assert_eq!(
            plan.parents,
            vec![
                (0, AtParent::Pane),
                (1, AtParent::Row(0)),
                (2, AtParent::Row(1)),
                (3, AtParent::Row(1)),
                (4, AtParent::Row(0)),
                (5, AtParent::Pane),
            ]
        );
        assert!(plan.ancestors.is_empty());
    }

    /// The window starts at a depth-2 row and holds a depth-1 row after it:
    /// each orphan hangs from the stand-in for its OWN parent.
    #[test]
    fn each_orphan_hangs_from_its_own_parent() {
        let plan = plan(&rows(3..6), parent_of);
        assert_eq!(
            plan.parents,
            vec![
                (3, AtParent::Ancestor(1)),
                (4, AtParent::Ancestor(0)),
                (5, AtParent::Pane),
            ]
        );
        assert_eq!(plan.ancestors, vec![(0, None), (1, Some(0))]);
    }

    #[test]
    fn a_loading_row_hangs_under_an_open_branch_before_it_or_beside_its_row() {
        let mut window = rows(0..4);
        // Row 2 has not resolved; the row before it (1) is an open branch.
        window[2].resolved = false;
        // Row 3 has not resolved either; the row before it (2) is no branch.
        window[3].resolved = false;
        let plan = plan(&window, parent_of);
        assert_eq!(plan.parents[2], (2, AtParent::Row(1)));
        assert_eq!(plan.parents[3], (3, AtParent::Row(1)));
    }
}
