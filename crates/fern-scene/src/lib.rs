//! `fern-scene` — pannable/zoomable scene viewport for FernUI.
//!
//! A sub-toolkit for **scene-based applications** — story corkboards,
//! mind maps, node-graph editors, timeline views — where content is
//! free-positioned at scene coordinates instead of placed by a layout
//! algorithm. Two tiers of content coexist under one view transform:
//!
//! - **Heavyweight tier:** any `Widget` (Button, TextInput, Panel,
//!   composite components) at a `scene_rect`, fully interactive and
//!   accessible, with full event/focus/animation/a11y machinery
//!   intact.
//! - **Lightweight tier:** `SceneItem`s (paths, rects, images, custom
//!   paint) without arena overhead — for the "background furniture" of
//!   a scene where thousands of items render cheaply. *(Phase 4+.)*
//!
//! The crate is built on top of fern-core's existing per-node
//! `set_transform` scope (which composes through hit-test, paint, and
//! a11y) and the platform's already-plumbed pinch / scroll-with-
//! modifiers / animated-`Signal<f32>` infrastructure — so OS gestures,
//! reduced-motion snapping, and the four-gate idle scheduler fall out
//! for free.
//!
//! See `docs/plans/scene-plan.md` for the full design and phasing,
//! `docs/fern-scene.md` for usage, and `docs/fern-scene-a11y.md` for
//! the accessibility-shaping API once that lands.
//!
//! ## Phase 1 surface
//!
//! What ships in Phase 1 is the model + a static viewport: build a
//! `Scene`, drop heavyweight widgets at fixed scene coordinates, hand
//! it to a `SceneView`. No view transform yet (identity), no
//! pan/zoom, no spatial index, no lightweight items. Subsequent phases
//! layer those on without API churn.
//!
//! ```ignore
//! use fern_scene::{ItemId, Scene, SceneView};
//! use fern_canvas::Rect;
//!
//! let mut scene = Scene::new();
//! let _id: ItemId = scene.add_widget(my_card_widget(), Rect::new(0.0, 0.0, 200.0, 120.0));
//! scene.add_widget(my_card_widget(), Rect::new(220.0, 0.0, 200.0, 120.0));
//! let view = SceneView::new(scene);
//! tree.add(view);
//! ```

pub mod a11y;
pub mod animation;
pub mod index;
pub mod item;
pub mod items;
pub mod minimap;
pub mod scene;
pub mod selection;
pub mod state;
pub mod transform;
pub mod view;

pub use a11y::{
    A11yBoundsSpace, A11yCategory, A11yGroup, A11yGroupBuilder, A11yGroupId, A11yNode,
    A11yOffScreenMode, A11yRelation,
};
pub use index::{GridHashIndex, SpatialIndex};
pub use item::{ItemId, SceneItem, SceneItemA11yContext, SceneItemPaintContext};
pub use items::{GroupItem, ImageItem, PathItem, RectItem, TextItem};
pub use animation::{pulse_once, register_animated_item_signal};
pub use minimap::SceneMinimap;
pub use state::SceneViewState;
pub use scene::Scene;
pub use selection::{SceneSelection, SceneSelectionMode};
pub use view::{DebugOverlay, FocusDirection, SceneView};
