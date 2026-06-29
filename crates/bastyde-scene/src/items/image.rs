// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`ImageItem`] — a raster image at a local-coord rectangle.
//!
//! `ImageItem` renders a raster image registered in the Canvas image registry
//! at a caller-specified rectangle in local item coordinates. The image
//! reference is a string key into that registry, not a path — apps pre-load
//! images and then name them here.
//!
//! ## When to use
//!
//! Use `ImageItem` when you need a static or swappable raster graphic in
//! the lightweight tier (no arena overhead). For interactive images that need
//! focus, drag-and-drop, or rich accessibility, embed a full `ImageWidget`
//! as a heavyweight scene widget instead.
//!
//! ## Example
//!
//! ```ignore
//! use bastyde_scene::{SceneModel, ImageItem};
//! use bastyde_canvas::Rect;
//! use bastyde_i18n::lit;
//!
//! let model = SceneModel::new();
//! let item = ImageItem::new(Rect::new(0.0, 0.0, 64.0, 64.0), "avatar")
//!     .label(lit!("User avatar"))
//!     .draggable(true);
//! model.add_item(item, bastyde_canvas::Point::new(100.0, 50.0));
//! ```

use accesskit::Role;
use bastyde_canvas::{Canvas, Rect};
use bastyde_core::accessibility::AccessNodeBuilder;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use bastyde_i18n::LocalizedString;

/// A raster image in a local-coord rectangle.
///
/// The image is referenced by a string key into the Canvas image registry.
/// Place the item in the scene via `Scene::add_item`; the key must resolve
/// to a registered image at paint time.
#[derive(Debug)]
pub struct ImageItem {
    local_bounds: Rect,
    name: String,
    label: Option<String>,
    flags: ItemFlags,
    a11y: ItemA11yOverrides,
}

impl ImageItem {
    /// An image item of the given size in local coordinates,
    /// referencing the image registered under `name`. The `name` is
    /// the Canvas-image-registry identifier — not a user-visible
    /// string, so it is not localized.
    pub fn new(local_bounds: Rect, name: impl Into<String>) -> Self {
        Self {
            local_bounds,
            name: name.into(),
            label: None,
            flags: ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Opt the image into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    crate::items::item_a11y_builders!();
}

impl SceneItem for ImageItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        canvas.draw_image(self.local_bounds, self.name.clone());
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn initial_flags(&self) -> ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> AccessSubtreeMode {
        self.a11y.subtree_mode()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(Role::Image);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}
