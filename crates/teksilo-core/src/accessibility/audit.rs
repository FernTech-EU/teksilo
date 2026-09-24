// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Invariant checks over an emitted accessibility tree.
//!
//! Four classes of defect are easy to introduce, invisible in a
//! screenshot, and only discovered when someone runs a screen reader:
//!
//! 1. **A label repeating its control's name.** A `Button` named "Save"
//!    that also emits a `Role::Label` reading "Save" is announced twice.
//!    The rule is string equality against the ancestor's own name — not
//!    the ancestor's role — because a status region, a tab and a toggle
//!    all leak the same way.
//! 2. **A label with no text ranges.** Without `Role::TextRun` children a
//!    label cannot be reviewed by character or word, cannot be routed to
//!    on a braille display, and cannot answer `AXBoundsForRange`.
//! 3. **Runs that disagree with the node they hang off.** The consumer
//!    derives a node's document text from its runs while every platform
//!    announces the node's own value; a divergence is a place where what
//!    a reader hears and what it reviews are different strings.
//! 4. **A focusable control inside a hidden subtree.** A hidden node hides
//!    everything under it, but the filter still lets the focused node
//!    through, so Tab lands a reader on a control with nothing around it
//!    and no parent that lists it. It is the mark of a wrapper that meant
//!    "I am only chrome" and said it with `set_hidden()`, which is how a
//!    dialog's, a menu's and a toolbar's content were once hidden.
//!
//! These run against a real [`TreeUpdate`] through `accesskit_consumer`,
//! so they measure what an adapter measures rather than what the
//! framework meant.

use accesskit::{Action, NodeId, Role, TreeUpdate};
use accesskit_consumer::{NodeRef, Tree};

/// A label repeating an ancestor's accessible name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelLeak {
    /// The ancestor that owns the name.
    pub owner: NodeId,
    pub owner_role: Role,
    /// The name both nodes announce.
    pub name: String,
    /// The descendant `Role::Label` repeating it.
    pub label: NodeId,
}

/// How a node's text ranges disagree with the node itself.
#[derive(Debug, Clone, PartialEq)]
pub enum DivergenceKind {
    /// The text a reader would review is not the text the node announces.
    TextDiffersFromValue { announced: String, reviewed: String },
    /// The document range reports no bounding boxes at all — the effect of
    /// a single run missing any of `bounds`, `text_direction`,
    /// `character_positions` or `character_widths`.
    NoGeometry,
    /// The boxes fall entirely outside the node's own box, so a magnifier
    /// following the review cursor would leave the label behind.
    GeometryOutsideNode,
    /// Focus landed on a synthetic node. A text run is excluded from
    /// object navigation, but a focused node is included before that
    /// filter runs, so a focused run becomes a visible stop.
    FocusOnSyntheticNode,
}

/// One node whose text ranges disagree with the node.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRangeDivergence {
    pub node: NodeId,
    pub kind: DivergenceKind,
}

fn locate(node: &NodeRef<'_>) -> NodeId {
    node.locate().0
}

/// The name a node announces, by the adapters' own rule: a `Role::Label`
/// carries its text in `value`, everything else in `label`, followed through
/// `labelled_by`. Neither falls back to the other, since no adapter does
/// (see [`crate::accessibility::announced_text`]): a node whose text is only
/// a value has no name to repeat.
fn announced_name(node: &NodeRef<'_>) -> Option<String> {
    let name = if node.label_comes_from_value() {
        node.value()
    } else {
        node.label()
    }?;
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Every `Role::Label` whose text repeats a **strict ancestor's** own
/// accessible name.
///
/// A `labelled_by` target is exempt: a container named after its visible
/// title necessarily repeats it, and that is the relation working, not a
/// leak.
pub fn duplicate_label_leaks(update: &TreeUpdate) -> Vec<LabelLeak> {
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut leaks = Vec::new();
    // Ancestors whose names are still in scope, innermost last.
    let mut stack: Vec<(NodeRef<'_>, Vec<(NodeId, Role, String)>)> =
        vec![(state.root(), Vec::new())];

    while let Some((node, names)) = stack.pop() {
        if node.role() == Role::Label
            && let Some(text) = announced_name(&node)
        {
            let id = locate(&node);
            if let Some((owner, owner_role, name)) = names.iter().rev().find(|(_, _, n)| *n == text)
                && !is_labelled_by_target(&state.root(), *owner, id)
            {
                leaks.push(LabelLeak {
                    owner: *owner,
                    owner_role: *owner_role,
                    name: name.clone(),
                    label: id,
                });
            }
        }

        let mut inherited = names;
        if node.role() != Role::Label
            && let Some(name) = announced_name(&node)
        {
            inherited.push((locate(&node), node.role(), name));
        }
        for child in node.children() {
            stack.push((child, inherited.clone()));
        }
    }
    leaks
}

/// Every node that offers keyboard focus while hidden from assistive
/// technology.
///
/// `NodeRef::is_hidden` is inherited from every ancestor and `common_filter`
/// answers a hidden node with `ExcludeSubtree`, so such a node is outside the
/// tree every adapter walks until it takes focus; then the filter lets it
/// through alone, and a reader hears a control whose parent does not list it
/// and whose neighbours cannot be reached. A disabled node is exempt: it
/// takes no focus. A wrapper that is only chrome around such a control is a
/// bare `Role::GenericContainer`, never hidden.
pub fn focusable_nodes_hidden(update: &TreeUpdate) -> Vec<NodeId> {
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut out = Vec::new();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.data().supports_action(Action::Focus) && node.is_hidden() && !node.is_disabled() {
            out.push(locate(&node));
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    out
}

/// Whether `owner` names itself through a relation pointing at `label`.
fn is_labelled_by_target(root: &NodeRef<'_>, owner: NodeId, label: NodeId) -> bool {
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        if locate(&node) == owner {
            return node.data().labelled_by().contains(&label);
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    false
}

/// Every visible `Role::Label` a reader cannot review by character.
///
/// A label with no text is exempt — it has nothing to review — and so is
/// one that is hidden.
pub fn labels_without_text_ranges(update: &TreeUpdate) -> Vec<NodeId> {
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut out = Vec::new();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if node.role() == Role::Label
            && announced_name(&node).is_some()
            && !node.is_hidden()
            && !node.supports_text_ranges()
        {
            out.push(locate(&node));
        }
        for child in node.children() {
            stack.push(child);
        }
    }
    out
}

/// Every node whose text ranges disagree with the node itself.
pub fn text_range_divergences(update: &TreeUpdate) -> Vec<TextRangeDivergence> {
    let focus = update.focus;
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut out = Vec::new();

    if super::is_synthetic(focus) {
        out.push(TextRangeDivergence {
            node: focus,
            kind: DivergenceKind::FocusOnSyntheticNode,
        });
    }

    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        for child in node.children() {
            stack.push(child);
        }
        if !node.supports_text_ranges() {
            continue;
        }
        // Only where the node owns its text. `supports_text_ranges` walks
        // *through* intermediates, so a composite input (a `DateTimeEdit`
        // over three fields) reports the concatenation of its descendants'
        // runs while its own value is a synthesized summary ("09:30:00"
        // over fields reading "09:30").
        // Those two are different things by design; comparing them is a
        // category error, and the residual inconsistency is recorded in
        // `docs/accessibility-internal-audit.md` rather than flagged here.
        let owns_its_text = node
            .data()
            .children()
            .iter()
            .any(|child| super::is_synthetic(*child));
        if !owns_its_text {
            continue;
        }
        let id = locate(&node);
        let range = node.document_range();
        let reviewed = range.text();
        // Against the node's *content*, not its name. On a label the two are
        // the same string — `AccessNodeBuilder::build` moves a `Role::Label`'s
        // name into `value`, which is where every adapter reads it. On an
        // input they are different things by design: a spin box named "Text
        // size" through a `labelled_by` relation reviews "100", and that is
        // the control working, not a divergence.
        //
        // A node exposing no value at all is a different question, and a
        // legitimate answer for a windowed surface: the log view deliberately
        // does not accumulate a 100 000-line document onto its own node,
        // which is the whole point of walking only the visible window.
        if let Some(announced) = node.value().map(|v| v.to_string())
            && reviewed != announced
        {
            out.push(TextRangeDivergence {
                node: id,
                kind: DivergenceKind::TextDiffersFromValue {
                    announced,
                    reviewed,
                },
            });
        }

        let boxes = range.bounding_boxes();
        if boxes.is_empty() {
            out.push(TextRangeDivergence {
                node: id,
                kind: DivergenceKind::NoGeometry,
            });
            continue;
        }
        // Clipped overflow is legal — a wrapped label in a fixed-height
        // parent paints lines past its own box — so the test is
        // intersection, not containment.
        if let Some(own) = node.bounding_box()
            && !boxes.iter().any(|b| intersects(*b, own))
        {
            out.push(TextRangeDivergence {
                node: id,
                kind: DivergenceKind::GeometryOutsideNode,
            });
        }
    }
    out
}

fn intersects(a: accesskit::Rect, b: accesskit::Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

/// What a screen reader could review on one node.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeTextInfo {
    /// How many text runs the node carries — at least one on any node that
    /// supports ranges.
    pub run_count: usize,
    /// The text a reader would review, assembled from the runs.
    pub document_text: String,
    /// Whether the range reports bounding boxes. `false` means a magnifier
    /// cannot follow the review cursor and braille cannot be routed.
    pub has_geometry: bool,
    /// The node's declared base reading direction, when it has one.
    pub direction: Option<accesskit::TextDirection>,
}

/// Text-range information for every node that carries any.
///
/// Batched deliberately: building a consumer tree is O(nodes), so a probe
/// asking node by node would be quadratic on a snapshot.
pub fn text_infos(update: &TreeUpdate) -> std::collections::HashMap<NodeId, NodeTextInfo> {
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut out = std::collections::HashMap::new();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        for child in node.children() {
            stack.push(child);
        }
        if !node.supports_text_ranges() {
            continue;
        }
        let range = node.document_range();
        out.insert(
            locate(&node),
            NodeTextInfo {
                run_count: node
                    .data()
                    .children()
                    .iter()
                    .filter(|id| {
                        update
                            .nodes
                            .iter()
                            .any(|(n, data)| n == *id && data.role() == Role::TextRun)
                    })
                    .count(),
                document_text: range.text(),
                has_geometry: !range.bounding_boxes().is_empty(),
                direction: node.data().text_direction(),
            },
        );
    }
    out
}

/// Every node's rectangle, resolved the way an adapter resolves it: raw bounds
/// composed with every transform between the node and the window — but **not**
/// the window's own, so the answer stays in logical pixels.
///
/// A node's `bounds` are stated "in the coordinate space of the nearest
/// ancestor with a non-`None` transform", so reading the raw property is
/// reading half an answer. It is the whole answer for most of the tree, where
/// nothing between the node and the window declares a transform — and silently
/// the wrong rectangle inside anything that does, such as the scene subtree
/// under a `SceneView`'s camera.
///
/// The root's transform is the device scale factor and is deliberately left
/// out: AccessKit wants physical pixels and gets them from the adapter, while
/// a caller here — an automation probe aiming a synthetic press, a diagnostic
/// overlay — works in the logical coordinates the rest of the framework uses.
/// A node absent from the map had no bounds to resolve.
pub fn logical_bounds(
    update: &TreeUpdate,
) -> std::collections::HashMap<NodeId, teksilo_canvas::Rect> {
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut out = std::collections::HashMap::new();
    // Depth-first from the root's children, carrying the composition so far.
    // The root itself contributes nothing, which is what drops the device
    // scale; it also has no bounds of its own to report.
    let root = state.root();
    let mut stack: Vec<(_, teksilo_canvas::Transform2D)> = root
        .children()
        .map(|c| (c, teksilo_canvas::Transform2D::IDENTITY))
        .collect();
    while let Some((node, to_window)) = stack.pop() {
        let own = match node.data().transform() {
            Some(t) => from_affine(*t).then(&to_window),
            None => to_window,
        };
        if let Some(r) = node.raw_bounds() {
            let rect = teksilo_canvas::Rect::new(
                r.x0 as f32,
                r.y0 as f32,
                (r.x1 - r.x0) as f32,
                (r.y1 - r.y0) as f32,
            );
            out.insert(locate(&node), own.apply_rect(rect));
        }
        for child in node.children() {
            stack.push((child, own));
        }
    }
    out
}

/// The inverse of [`to_accesskit_affine`](super::to_accesskit_affine): both
/// spell a 3×2 affine `[a, b, c, d, tx, ty]`, so this is a narrowing cast.
fn from_affine(a: accesskit::Affine) -> teksilo_canvas::Transform2D {
    let m = a.as_coeffs();
    teksilo_canvas::Transform2D {
        m: [
            m[0] as f32,
            m[1] as f32,
            m[2] as f32,
            m[3] as f32,
            m[4] as f32,
            m[5] as f32,
        ],
    }
}

/// The name each node announces, resolved the way an adapter resolves it —
/// through `labelled_by` when the node carries no name of its own.
///
/// A container named by pointing at its visible title has no `label` of its
/// own, so a probe reading the raw property sees nothing where a reader
/// hears the title.
pub fn resolved_names(update: &TreeUpdate) -> std::collections::HashMap<NodeId, String> {
    let tree = Tree::new(update.clone(), false);
    let state = tree.state();
    let mut out = std::collections::HashMap::new();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        for child in node.children() {
            stack.push(child);
        }
        if let Some(label) = node.label() {
            out.insert(locate(&node), label);
        }
    }
    out
}

/// Every node a platform adapter walks: the ones `accesskit_consumer`'s
/// `common_filter` includes.
///
/// Left out are a hidden node, everything inside a hidden subtree, a node its
/// clipping parent has scrolled out of view, and the `GenericContainer` and
/// text-run nodes the consumer steps through. The window is taken to have
/// focus, so the focused node is always in, as it is for a user working in
/// the window.
pub fn nodes_in_filtered_tree(update: &TreeUpdate) -> std::collections::HashSet<NodeId> {
    let tree = Tree::new(update.clone(), true);
    let state = tree.state();
    let mut out = std::collections::HashSet::new();
    let mut stack = vec![state.root()];
    while let Some(node) = stack.pop() {
        if accesskit_consumer::common_filter(&node) == accesskit_consumer::FilterResult::Include {
            out.insert(locate(&node));
        }
        stack.extend(node.children());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::{FillWidget, StackWidget};
    use crate::widget_builder::WidgetBuilder;
    use crate::widget_tree::WidgetTree;
    use teksilo_canvas::SizeProposal;

    /// `inner` inside a wrapper that is either hidden or a bare
    /// `GenericContainer`, the two ways a wrapper has said "only chrome".
    fn update_for(inner: FillWidget, hidden: bool) -> TreeUpdate {
        let mut tree = WidgetTree::new();
        let inner = tree.add(inner);
        let wrapper = StackWidget::new().child(inner);
        if hidden {
            tree.add(wrapper.access_hidden(true));
        } else {
            tree.add(wrapper.access_role(Role::GenericContainer));
        }
        tree.layout(SizeProposal::exact(200.0, 100.0));
        tree.sync_accessibility()
    }

    #[test]
    fn a_focusable_control_under_a_hidden_wrapper_is_reported() {
        let update = update_for(FillWidget::new().focusable(), true);
        assert_eq!(focusable_nodes_hidden(&update).len(), 1);
    }

    #[test]
    fn the_same_control_under_a_presentational_wrapper_is_not() {
        let update = update_for(FillWidget::new().focusable(), false);
        assert!(focusable_nodes_hidden(&update).is_empty());
    }

    #[test]
    fn a_hidden_wrapper_around_decoration_is_not() {
        let update = update_for(FillWidget::new().label("Deco"), true);
        assert!(focusable_nodes_hidden(&update).is_empty());
    }

    /// A window holding `outer` and, inside it, a label reading `text`.
    fn label_inside(outer: accesskit::Node, text: &str) -> TreeUpdate {
        let (root, outer_id, label_id) = (NodeId(1), NodeId(2), NodeId(3));
        let mut window = accesskit::Node::new(Role::Window);
        window.set_children(vec![outer_id]);
        let mut outer = outer;
        outer.set_children(vec![label_id]);
        let mut label = accesskit::Node::new(Role::Label);
        label.set_value(text.to_string());
        TreeUpdate {
            nodes: vec![(root, window), (outer_id, outer), (label_id, label)],
            tree: Some(accesskit::TreeInfo::new(root)),
            tree_id: accesskit::TreeId::ROOT,
            focus: root,
        }
    }

    /// A label repeating the name of the node around it is heard twice.
    /// One repeating a value that node carries in place of a name is not:
    /// no adapter reads a value as the name of anything but a label, so
    /// the text is heard once, from the label.
    #[test]
    fn a_label_repeats_a_name_and_not_a_value() {
        let mut named = accesskit::Node::new(Role::Status);
        named.set_label("Enregistré".to_string());
        assert_eq!(
            duplicate_label_leaks(&label_inside(named, "Enregistré")).len(),
            1
        );

        let mut valued = accesskit::Node::new(Role::Status);
        valued.set_value("Enregistré".to_string());
        assert!(duplicate_label_leaks(&label_inside(valued, "Enregistré")).is_empty());
    }
}
