//! Accessibility policies for `SceneView`.
//!
//! Phase 5a ships the **visual-default** path: an off-screen-mode
//! enum + helper that decides which items the AT walker should
//! emit. Phase 5b layers the parallel-structural API on top
//! (logical groups, parents, relations, auto-graft, custom focus
//! callbacks) — see `docs/fern-scene-a11y.md` for the full picture.
//!
//! Defaults are chosen so a quick prototype is accessible out of
//! the box: every visible heavyweight widget participates in the
//! AT walker as a normal direct child of `SceneView`, every visible
//! lightweight item gets a synthetic AT node with role +
//! screen-projected bounds, and Tab cycles in reading order.

use std::sync::atomic::{AtomicU64, Ordering};

use fern_canvas::Rect;
use fern_core::widget_id::WidgetId;

use crate::item::ItemId;

// ---------------------------------------------------------------------------
// Logical AT structure — Phase 5b
// ---------------------------------------------------------------------------

/// Opaque identifier for a logical AT group declared via
/// [`Scene::add_a11y_group`](crate::Scene::add_a11y_group). Stable
/// across the lifetime of the process; safe to hash, compare, store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct A11yGroupId(pub(crate) u64);

impl A11yGroupId {
    pub(crate) fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        A11yGroupId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Raw numeric value. Used by the AT walker to derive a synthetic
    /// `NodeId` via `synthetic_node_id(scene_view_id, id.as_u64(),
    /// SyntheticKind::SceneGroup)`.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Address of a node in the parallel logical AT tree. Lets apps
/// uniformly target lightweight items, virtual groups, and real
/// interactive widgets when declaring relationships, parents, or
/// rotor categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum A11yNode {
    /// A lightweight `SceneItem` in the scene.
    Item(ItemId),
    /// A virtual `A11yGroup` declared via
    /// [`Scene::add_a11y_group`](crate::Scene::add_a11y_group).
    Group(A11yGroupId),
    /// A real interactive widget in the arena. Use this to relocate
    /// a heavyweight `WidgetItem`'s descendant — say, a `ComboBox`
    /// nested visually in a Scene card — to a logical parent
    /// elsewhere in the AT tree.
    Widget(WidgetId),
}

/// AT relationship kind, applied via
/// [`Scene::add_a11y_relation`](crate::Scene::add_a11y_relation).
/// Maps to AccessKit's relationship arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum A11yRelation {
    /// `from` controls `to` (e.g. a button that opens a menu).
    Controls,
    /// `from` is described by `to` (cross-item annotation).
    DescribedBy,
    /// `from` is labelled by `to` (cross-item label).
    LabelledBy,
    /// Logical flow direction — many node-graph editors use this so
    /// VoiceOver / NVDA "next item" follows data-flow order rather
    /// than reading order.
    FlowTo,
}

/// App-defined category tag for AT rotor / quick-nav navigation.
/// Surfaced to AT clients that support categorized navigation
/// (VoiceOver rotor on macOS, NVDA quick-nav). Apps coin their own
/// tag values like `"node"`, `"connector"`, `"comment"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct A11yCategory(pub std::borrow::Cow<'static, str>);

impl A11yCategory {
    pub fn new(name: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self(name.into())
    }
}

/// Builder for an [`A11yGroup`]. Returned by
/// [`A11yGroup::builder`]; consumed by
/// [`Scene::add_a11y_group`](crate::Scene::add_a11y_group).
///
/// ```ignore
/// let act_one = scene.add_a11y_group(
///     A11yGroup::builder()
///         .label("Act 1")
///         .role(accesskit::Role::Group)
/// );
/// scene.set_a11y_parent(A11yNode::Item(scene_card), Some(A11yNode::Group(act_one)));
/// ```
#[derive(Debug)]
pub struct A11yGroupBuilder {
    pub(crate) label: Option<String>,
    pub(crate) role: accesskit::Role,
}

impl A11yGroupBuilder {
    /// Human-readable label for the group, announced when AT clients
    /// land on the group node.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Override the AccessKit role. Default: `Role::Group`. Apps
    /// commonly use `Role::Region` for landmark-style groups.
    pub fn role(mut self, role: accesskit::Role) -> Self {
        self.role = role;
        self
    }
}

/// A logical AT group. Pure structure — no visual counterpart, no
/// hit-test, no paint. Used by Phase 5b to declare AT-shape that
/// diverges from visual scene layout (Acts containing Scene cards,
/// Subgraphs containing Nodes, Layers containing Components).
#[derive(Debug)]
pub struct A11yGroup {
    pub(crate) id: A11yGroupId,
    pub(crate) label: Option<String>,
    pub(crate) role: accesskit::Role,
}

impl A11yGroup {
    /// A fresh builder for a logical group. Default role is
    /// `Role::Group`; override with [`A11yGroupBuilder::role`].
    pub fn builder() -> A11yGroupBuilder {
        A11yGroupBuilder {
            label: None,
            role: accesskit::Role::Group,
        }
    }

    /// The group's id. Stable for the lifetime of the process.
    pub fn id(&self) -> A11yGroupId {
        self.id
    }

    /// The label set on the builder, if any.
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// The role set on the builder. Default `Role::Group`.
    pub fn role(&self) -> accesskit::Role {
        self.role
    }
}

/// Off-screen visibility policy for the AT walker. Decides which
/// scene items get emitted as synthetic AT nodes per AT-rebuild.
///
/// `ViewportPlusN { n: 1 }` is the default: an item appears in the
/// AT tree if its `bounds_in_scene` intersects `viewport ∪ (1×
/// viewport-grown-rect)`. That keeps the tree close to "what the
/// user can interact with right now" while letting screen-reader
/// users discover items just outside the visible region by jumping
/// to the next/prev — at which point `SceneView::focus_item` auto-
/// pans the view to bring the focused item into view (Phase 5+).
#[derive(Debug, Clone, Copy)]
pub enum A11yOffScreenMode {
    /// Emit *every* item in the scene as a synthetic AT node.
    /// Heaviest mode — appropriate for small scenes (< ~500 items)
    /// where AT users want a complete table of contents.
    AllItems,

    /// Emit items inside the viewport plus an `n × viewport`-grown
    /// margin around it. `n = 0` collapses to "viewport only" with
    /// the same allocation pattern as `ViewportOnly`. `n = 1` is
    /// the default — gives screen-reader users a one-screen
    /// "lookahead" to navigate without `focus_item` round-tripping
    /// through pan animation.
    ViewportPlusN { n: u32 },

    /// Strict: only items intersecting the current viewport. Pairs
    /// with apps that have very large scenes where listing
    /// off-screen content would overwhelm AT clients.
    ViewportOnly,
}

impl A11yOffScreenMode {
    /// Compute the scene-coord rectangle a given mode considers
    /// "AT-visible" given the current visible scene region. Used by
    /// `SceneView::accessibility` as the spatial-index query rect.
    /// `AllItems` returns `None` so the caller knows to bypass the
    /// query and emit every item.
    pub fn at_visible_region(&self, visible_scene_region: Rect) -> Option<Rect> {
        match *self {
            A11yOffScreenMode::AllItems => None,
            A11yOffScreenMode::ViewportOnly => Some(visible_scene_region),
            A11yOffScreenMode::ViewportPlusN { n } => {
                if n == 0 {
                    return Some(visible_scene_region);
                }
                let margin_x = visible_scene_region.width * n as f32;
                let margin_y = visible_scene_region.height * n as f32;
                Some(Rect::new(
                    visible_scene_region.x - margin_x,
                    visible_scene_region.y - margin_y,
                    visible_scene_region.width + margin_x * 2.0,
                    visible_scene_region.height + margin_y * 2.0,
                ))
            }
        }
    }
}

impl Default for A11yOffScreenMode {
    fn default() -> Self {
        A11yOffScreenMode::ViewportPlusN { n: 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_viewport_plus_one() {
        assert!(matches!(
            A11yOffScreenMode::default(),
            A11yOffScreenMode::ViewportPlusN { n: 1 }
        ));
    }

    #[test]
    fn all_items_returns_none() {
        assert_eq!(
            A11yOffScreenMode::AllItems
                .at_visible_region(Rect::new(0.0, 0.0, 100.0, 100.0)),
            None
        );
    }

    #[test]
    fn viewport_only_passes_through() {
        let viewport = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(
            A11yOffScreenMode::ViewportOnly.at_visible_region(viewport),
            Some(viewport)
        );
    }

    #[test]
    fn viewport_plus_n_grows_symmetrically() {
        // Viewport at (0,0)-(100,80), n=1 → grow by ±100 in x, ±80
        // in y → final rect (-100,-80)-(200,160) i.e. 300×240.
        let viewport = Rect::new(0.0, 0.0, 100.0, 80.0);
        let grown = A11yOffScreenMode::ViewportPlusN { n: 1 }
            .at_visible_region(viewport)
            .unwrap();
        assert_eq!(grown, Rect::new(-100.0, -80.0, 300.0, 240.0));
    }

    #[test]
    fn viewport_plus_zero_equals_viewport_only() {
        let viewport = Rect::new(50.0, 50.0, 200.0, 100.0);
        assert_eq!(
            A11yOffScreenMode::ViewportPlusN { n: 0 }.at_visible_region(viewport),
            A11yOffScreenMode::ViewportOnly.at_visible_region(viewport),
        );
    }

    #[test]
    fn viewport_plus_two_grows_by_two_viewports_each_side() {
        let viewport = Rect::new(0.0, 0.0, 100.0, 100.0);
        let grown = A11yOffScreenMode::ViewportPlusN { n: 2 }
            .at_visible_region(viewport)
            .unwrap();
        // Margin is 200 on each side → final 500×500.
        assert_eq!(grown, Rect::new(-200.0, -200.0, 500.0, 500.0));
    }
}
