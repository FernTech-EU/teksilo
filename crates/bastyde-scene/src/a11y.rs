//! Accessibility policies for [`SceneView`](crate::SceneView).
//!
//! Two layers cooperate. The **visual-default** path emits AT nodes
//! for every visible heavyweight widget and every visible lightweight
//! item with role + screen-projected bounds, gated by an
//! [`A11yOffScreenMode`] policy that decides which off-viewport items
//! are still announced. The **logical-structural API** (groups,
//! parents, relations, auto-graft, custom focus callbacks) layers
//! over the top — see [`docs/bastyde-scene-a11y.md`](https://github.com/jacquetc/bastyde/blob/main/docs/bastyde-scene-a11y.md)
//! for the full picture.
//!
//! Defaults are chosen so a quick prototype is accessible out of the
//! box: heavyweight widgets emit normally, lightweight items get
//! synthetic nodes, Tab cycles in reading order. Apps shape the
//! reading experience by declaring [`A11yGroup`]s, reparenting nodes,
//! and installing a focus-order callback.

use std::sync::atomic::{AtomicU64, Ordering};

use bastyde_canvas::Rect;
use bastyde_core::widget_id::WidgetId;

use crate::item::ItemId;

// ---------------------------------------------------------------------------
// Logical AT structure
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
/// uniformly target scene entries, virtual groups, and ad-hoc
/// widgets when declaring relationships, parents, or rotor
/// categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum A11yNode {
    /// Any entry in the scene — lightweight `SceneItem` or
    /// heavyweight `Widget` added via `Scene::add_widget`. The
    /// walker discriminates by entry kind: lightweight items get
    /// a synthetic `SyntheticKind::SceneItem` AT node; heavyweight
    /// items get auto-grafted via the framework redirect hook,
    /// landing the real widget's `NodeId` under the declared
    /// parent.
    Item(ItemId),
    /// A virtual `A11yGroup` declared via
    /// [`Scene::add_a11y_group`](crate::Scene::add_a11y_group).
    Group(A11yGroupId),
    /// A real interactive widget addressed by its arena
    /// [`WidgetId`]. Use this to relocate widgets that aren't
    /// `Scene::add_widget`-managed — typically a *descendant* of
    /// a heavyweight scene item that should logically belong
    /// elsewhere (a global `ComboBox` nested visually inside a
    /// Scene card but logically under a top-level "Tools" group).
    /// For widgets you added via `Scene::add_widget`, prefer
    /// `A11yNode::Item(item_id)` — the walker handles the
    /// heavyweight-item auto-graft for you.
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
    pub(crate) label: Option<bastyde_i18n::LocalizedString>,
    pub(crate) role: accesskit::Role,
}

impl A11yGroupBuilder {
    /// Human-readable label for the group, announced when AT clients
    /// land on the group node. Accepts anything convertible into
    /// [`LocalizedString`](bastyde_i18n::LocalizedString).
    pub fn label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
        let ls: bastyde_i18n::LocalizedString = label.into();
        self.label = Some(ls);
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
/// hit-test, no paint. Declares AT-shape that
/// diverges from visual scene layout (Acts containing Scene cards,
/// Subgraphs containing Nodes, Layers containing Components).
#[derive(Debug)]
pub struct A11yGroup {
    pub(crate) id: A11yGroupId,
    pub(crate) label: Option<bastyde_i18n::LocalizedString>,
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
    pub fn label(&self) -> Option<String> {
        self.label.as_ref().map(|l| l.resolve_now())
    }

    /// The role set on the builder. Default `Role::Group`.
    pub fn role(&self) -> accesskit::Role {
        self.role
    }
}

/// AT-emission strategy for `SceneView`. Decides whether items /
/// widgets that have *not* been placed in the app-declared logical
/// tree appear in the AT tree by default, or are suppressed.
///
/// Pick `Cooperative` when the visual scene layout *is* a sensible
/// AT structure for your app (charts, dashboards, simple maps).
/// Pick `StrictlyParallel` when AT shape diverges meaningfully
/// from visual layout — story corkboards (Acts → Scene cards),
/// node-graph editors (Subgraphs → Nodes → Ports), CAD canvases
/// (Layers → Components). Apps in this category typically declare
/// every AT edge anyway, so the default visual-emission becomes
/// noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum A11yMode {
    /// **Default.** Visual is the AT structure unless overridden.
    /// Items inside the off-screen-mode policy emit as direct AT
    /// children of `SceneView` (or their declared logical parent
    /// if `set_a11y_parent` placed them). Heavyweight widgets
    /// emit through the arena walker as natural descendants of
    /// `SceneView`. The logical-tree machinery layers on
    /// top.
    #[default]
    Cooperative,

    /// AT structure is purely declared. Items are emitted **only**
    /// if the app placed them in the logical tree via
    /// `Scene::set_a11y_parent`. Heavyweight widgets still emit
    /// (they own focus / interaction state the AT layer can't
    /// suppress) but their parent in the AT tree is the declared
    /// logical parent if any, else `SceneView` itself.
    ///
    /// Use this when your app's AT shape is fundamentally
    /// different from its visual layout — declaring every node
    /// once is cheaper than overriding the visual default for
    /// every node.
    StrictlyParallel,
}

/// Coordinate space the AT walker reports `SceneItem` bounds in.
///
/// The framework convention is **screen-projected** bounds — the
/// rectangle a sighted user would see on the physical monitor, after
/// pan/zoom/rotation has been applied. Screen readers consume this
/// for spatial nav (Apple's "explore by touch", touch-screen navi-
/// gation, magnifier follow-focus). 99% of apps want this default.
///
/// **Scene** bounds are the raw scene-coord rectangle stored on the
/// item, with no view-transform applied. Use this only for the
/// rare AT clients that reason about scene topology rather than
/// viewport position — typically when a SceneView's contents have
/// a logical, fixed coordinate system that the user thinks in (a
/// CAD canvas where "the bracket is at (240, 180)" means a fixed
/// physical machine position regardless of zoom level).
///
/// Picking the wrong one makes "go to the next item" navigation
/// either a) ignore the user's current pan (Screen mode in a
/// scene-coord-aware app) or b) report bounds that drift under
/// pan/zoom (Scene mode in a viewport-aware app). Default is
/// `Screen` — change only when you've confirmed your AT users
/// genuinely want the alternative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum A11yBoundsSpace {
    /// Screen-projected bounds — `view_transform * bounds_in_scene`.
    /// The framework default; matches the convention used by every
    /// other widget in the framework.
    #[default]
    Screen,
    /// Raw scene-coordinate bounds, with no view-transform applied.
    /// Apps with a logical fixed coordinate system (CAD canvases,
    /// blueprint editors) may want this so AT users can reason
    /// about "where in the design" an item sits, independent of
    /// the current pan/zoom.
    Scene,
}

/// Off-screen visibility policy for the AT walker. Decides which
/// scene items get emitted as synthetic AT nodes per AT-rebuild.
///
/// `ViewportPlusN { n: 1 }` is the default: an item appears in the
/// AT tree if its `bounds_in_scene` intersects `viewport ∪ (1×
/// viewport-grown-rect)`. That keeps the tree close to "what the
/// user can interact with right now" while letting screen-reader
/// users discover items just outside the visible region by jumping
/// to the next/prev — at which point `SceneView::ensure_visible`
/// pans the view to bring the focused item into view.
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
    /// "lookahead" to navigate without `ensure_visible` round-tripping
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
            A11yOffScreenMode::AllItems.at_visible_region(Rect::new(0.0, 0.0, 100.0, 100.0)),
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
