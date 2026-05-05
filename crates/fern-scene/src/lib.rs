//! `fern-scene` — pannable / zoomable scene viewport for FernUI.
//!
//! A sub-toolkit for **scene-based applications** — story corkboards,
//! mind maps, node-graph editors, timeline views — where content is
//! free-positioned at scene coordinates instead of placed by a layout
//! algorithm. Two tiers of content coexist under one view transform:
//!
//! - **Heavyweight tier:** any `Widget` (Button, TextInput, Panel,
//!   composite components) at a parent-relative position, fully
//!   interactive and accessible, with full event / focus / animation /
//!   a11y machinery intact.
//! - **Lightweight tier:** [`SceneItem`]s (paths, rects, images,
//!   custom paint) without arena overhead — for the "background
//!   furniture" of a scene where thousands of items render cheaply.
//!
//! The crate is built on top of fern-core's per-node `set_transform`
//! scope (which composes through hit-test, paint, and a11y) and the
//! platform's pinch / scroll / animated-`Signal<f32>` infrastructure
//! — so OS gestures, reduced-motion snapping, and the four-gate idle
//! scheduler fall out for free.
//!
//! See [`docs/fern-scene.md`](https://github.com/fernui/fern-ui/blob/main/docs/fern-scene.md)
//! for the user-facing reference and
//! [`docs/fern-scene-a11y.md`](https://github.com/fernui/fern-ui/blob/main/docs/fern-scene-a11y.md)
//! for the accessibility-shaping API.
//!
//! ## Quick start
//!
//! ```ignore
//! use fern_scene::{ItemId, RectItem, Scene, SceneView};
//! use fern_canvas::{Point, Rect};
//!
//! let mut scene = Scene::new();
//! let _w: ItemId = scene.add_widget(
//!     my_card_widget(),
//!     Rect::new(0.0, 0.0, 200.0, 120.0),
//! );
//! scene.add_item(
//!     RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(fern_tokens::Color::RED),
//!     Point::new(220.0, 0.0),
//! );
//! let view = SceneView::new(scene);
//! tree.add(view);
//! ```

#![allow(clippy::type_complexity)]

pub mod a11y;
pub mod animation;
pub mod cache;
pub mod flags;
pub mod index;
pub mod item;
pub mod item_handlers;
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
pub use animation::{pulse_once, register_animated_item_signal};
pub use cache::{CacheMode, ItemCoordinateCache};
pub use flags::ItemFlags;
pub use index::{GridHashIndex, SpatialIndex};
pub use item::{ItemId, SceneItem, SceneItemA11yContext, SceneItemPaintContext};
pub use item_handlers::{DragMode, SceneItemHandlerSet};
pub use items::AccessSubtreeMode;
pub use items::{GroupItem, ImageItem, PathItem, RectItem, TextItem};
pub use minimap::SceneMinimap;
pub use scene::Scene;
pub use scene::{ItemChange, PanAxes};
pub use selection::{SceneSelection, SceneSelectionMode};
pub use state::SceneViewState;
pub use view::{DebugOverlay, FocusDirection, SceneView};
