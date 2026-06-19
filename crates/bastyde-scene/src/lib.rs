// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `bastyde-scene` — pannable / zoomable scene viewport for Bastyde.
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
//! The crate is built on top of bastyde-core's per-node `set_transform`
//! scope (which composes through hit-test, paint, and a11y) and the
//! platform's pinch / scroll / animated-`Signal<f32>` infrastructure
//! — so OS gestures, reduced-motion snapping, and the four-gate idle
//! scheduler fall out for free.
//!
//! See [`docs/bastyde-scene.md`](https://github.com/ferntech-eu/bastyde/blob/main/docs/bastyde-scene.md)
//! for the user-facing reference and
//! [`docs/bastyde-scene-a11y.md`](https://github.com/ferntech-eu/bastyde/blob/main/docs/bastyde-scene-a11y.md)
//! for the accessibility-shaping API.
//!
//! ## Quick start
//!
//! ```ignore
//! use bastyde_scene::{ItemId, RectItem, Scene, SceneView};
//! use bastyde_canvas::{Point, Rect};
//!
//! let mut scene = Scene::new();
//! let _w: ItemId = scene.add_widget(
//!     my_card_widget(),
//!     Rect::new(0.0, 0.0, 200.0, 120.0),
//! );
//! scene.add_item(
//!     RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0)).fill(bastyde_tokens::Color::RED),
//!     Point::new(220.0, 0.0),
//! );
//! let view = SceneView::new(scene);
//! tree.add(view);
//! ```

#![allow(clippy::type_complexity)]

// Implementation modules are intentionally `pub(crate)`. The crate's
// public surface is the re-export block below — every type or trait
// an external consumer is meant to use is reachable as
// `bastyde_scene::Foo` (never `bastyde_scene::flags::Foo` etc). Narrowing
// here lets internal refactors land without ripple effects in
// downstream apps. Adding a new external-facing type? Re-export it
// rather than widening a module to `pub`.
pub(crate) mod a11y;
pub(crate) mod animation;
pub(crate) mod cache;
pub(crate) mod flags;
pub(crate) mod index;
pub(crate) mod item;
pub(crate) mod item_handlers;
pub(crate) mod items;
pub(crate) mod magnet;
pub(crate) mod minimap;
pub(crate) mod scene;
pub(crate) mod scene_model;
pub(crate) mod scroll_view;
pub(crate) mod selection;
pub(crate) mod state;
pub(crate) mod transform;
pub(crate) mod view;

pub use a11y::{
    A11yBoundsSpace, A11yCategory, A11yGroup, A11yGroupBuilder, A11yGroupId, A11yMode, A11yNode,
    A11yOffScreenMode, A11yRelation,
};
pub use animation::{pulse_once, register_animated_item_signal};
pub use cache::CacheMode;
pub use flags::ItemFlags;
pub use index::{GridHashIndex, SpatialIndex};
pub use item::{ItemId, SceneItem, SceneItemA11yContext, SceneItemPaintContext};
pub use item_handlers::{DragMode, SceneItemHandlerSet, SceneTapEvent};
pub use items::AccessSubtreeMode;
pub use items::{GroupItem, ImageItem, PathItem, RectItem, TextItem};
pub use magnet::{
    Magnet, MagnetConnection, MagnetFeedback, MagnetId, MagnetMarker, MagnetRef, MagnetRole,
    MagnetSnap, MagnetVerdict, MagnetVisualState, MagnetismConfig, MarkerVisibility,
};
pub use minimap::SceneMinimap;
pub use scene::Scene;
pub use scene::{ItemChange, PanAxes, SceneConstraints, SceneLayer};
pub use scene_model::SceneModel;
pub use scroll_view::{SceneScrollView, ScrollBarMode, ScrollBarPolicy};
pub use selection::{SceneSelection, SceneSelectionMode};
pub use state::SceneViewState;
pub use view::{DebugOverlay, FocusDirection, SceneView};
