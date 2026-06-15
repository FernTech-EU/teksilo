// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! [`ImageItem`] — a raster image at a local-coord rectangle.

use accesskit::Role;
use bastyde_canvas::{Canvas, Rect};
use bastyde_core::accessibility::AccessNodeBuilder;

use crate::flags::ItemFlags;
use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};
use crate::items::{AccessSubtreeMode, ItemA11yOverrides};
use bastyde_i18n::LocalizedString;

/// A raster image in a local-coord rectangle. The image is referenced
/// by name (the Canvas image registry).
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
