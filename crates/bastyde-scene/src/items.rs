//! Built-in [`SceneItem`] implementations.
//!
//! Five lightweight items cover the common decoration cases:
//!
//! - [`RectItem`] — filled / stroked rectangle. Backgrounds, tiles,
//!   simple decorations.
//! - [`PathItem`] — arbitrary vector path with optional fill and
//!   stroke. The "connector lines between cards" workhorse, with
//!   per-segment hit-test for stroke-only paths.
//! - [`ImageItem`] — a raster image at a local-coord rectangle.
//! - [`TextItem`] — unstyled text in a local-coord rectangle, static
//!   string or signal-bound.
//! - [`GroupItem`] — a group container with optional fill / stroke /
//!   inline label. Visually a labelled box; non-visual groups serve
//!   as logical AT containers ([`Scene::add_a11y_group`](crate::Scene::add_a11y_group)).
//!
//! All built-ins store their geometry in **local item coordinates**
//! anchored at the origin. Apps construct an item with its size at
//! origin (`RectItem::new(Rect::new(0.0, 0.0, w, h))`) and place it
//! in the scene with `Scene::add_item(item, local_pos)`.

use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_i18n::lit;

pub mod group;
pub mod image;
pub mod path;
pub mod rect;
pub mod text;

pub use group::GroupItem;
pub use image::ImageItem;
pub use path::PathItem;
pub use rect::RectItem;
pub use text::TextItem;

/// How the AT walker treats descendants of an item.
///
/// Mirrors the widget-tier `AccessSubtreeMode`: `Inherit` is the
/// default (descendants emit normally); `Exclude` prunes them from
/// the AT tree; `Merge` collapses them into the parent so the
/// subtree reads as a single AT element. Used for "card with rect +
/// label + indicator dot reads as one card" patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessSubtreeMode {
    #[default]
    Inherit,
    Exclude,
    Merge,
}

/// Builder-level accessibility overrides shared by every built-in
/// `SceneItem`. Mirrors the widget-level `.access_*` chain — names
/// match so muscle memory carries over.
#[derive(Debug, Default, Clone)]
pub struct ItemA11yOverrides {
    pub(crate) label: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) role: Option<accesskit::Role>,
    pub(crate) hidden: bool,
    pub(crate) subtree_mode: AccessSubtreeMode,
}

impl ItemA11yOverrides {
    /// Read access for the AT walker.
    pub fn subtree_mode(&self) -> AccessSubtreeMode {
        self.subtree_mode
    }

    /// Apply the configured overrides to an [`AccessNodeBuilder`]
    /// after the item's own `accessibility` impl has populated the
    /// default fields. Replaces matching fields rather than merging.
    pub(crate) fn apply(&self, builder: &mut AccessNodeBuilder) {
        if let Some(role) = self.role {
            builder.set_role(role);
        }
        if let Some(ref label) = self.label {
            builder.set_name(label.clone());
        }
        if let Some(ref desc) = self.description {
            builder.set_description(desc.clone());
        }
        if self.hidden {
            builder.set_hidden();
        }
    }
}

/// Emit the `.access_*` builder chain on a struct that holds an
/// `a11y: ItemA11yOverrides` field. Built-in items invoke this inside
/// their inherent impl block so they all share the same translated +
/// `_literal` method names. Custom items can do the same.
#[doc(hidden)]
#[macro_export]
macro_rules! item_a11y_builders {
    () => {
        /// Override the AT name announced for this item. Accepts
        /// anything convertible into [`LocalizedString`] — most
        /// commonly `tr!(...)` for translated labels, or any plain
        /// string (which auto-converts via `From<String>`).
        pub fn access_label(mut self, label: impl Into<bastyde_i18n::LocalizedString>) -> Self {
            let ls: bastyde_i18n::LocalizedString = label.into();
            self.a11y.label = Some(ls.resolve_now());
            self
        }

        /// Untranslated twin of [`access_label`](Self::access_label).
        /// Wraps a raw string in
        /// [`LocalizedString::literal`](bastyde_i18n::LocalizedString::literal)
        /// — a grep-marker for call sites that intentionally bypass
        /// the i18n pipeline (debug demos, engine-internal labels).
        #[doc(hidden)]
        pub fn access_label_literal(self, label: impl Into<String>) -> Self {
            self.access_label(bastyde_i18n::LocalizedString::literal(label))
        }

        /// Long-form context appended to the item's announcement.
        pub fn access_description(
            mut self,
            description: impl Into<bastyde_i18n::LocalizedString>,
        ) -> Self {
            let ls: bastyde_i18n::LocalizedString = description.into();
            self.a11y.description = Some(ls.resolve_now());
            self
        }

        /// Untranslated twin of [`access_description`](Self::access_description).
        #[doc(hidden)]
        pub fn access_description_literal(self, description: impl Into<String>) -> Self {
            self.access_description(bastyde_i18n::LocalizedString::literal(description))
        }

        /// Override the AccessKit role for this item.
        pub fn access_role(mut self, role: accesskit::Role) -> Self {
            self.a11y.role = Some(role);
            self
        }

        /// Hide this item from the AT tree.
        pub fn access_hidden(mut self, hidden: bool) -> Self {
            self.a11y.hidden = hidden;
            self
        }

        /// Set the AT subtree mode. `Merge` collapses descendants
        /// into this item's AT node; `Exclude` prunes them; the
        /// default `Inherit` lets them emit normally.
        pub fn access_subtree(mut self, mode: $crate::items::AccessSubtreeMode) -> Self {
            self.a11y.subtree_mode = mode;
            self
        }

        /// Convenience: collapse all descendants into this item's
        /// AT node so the subtree reads as one element.
        pub fn access_merge_subtree(mut self) -> Self {
            self.a11y.subtree_mode = $crate::items::AccessSubtreeMode::Merge;
            self
        }

        /// Convenience: prune all descendants from the AT tree.
        pub fn access_exclude_subtree(mut self) -> Self {
            self.a11y.subtree_mode = $crate::items::AccessSubtreeMode::Exclude;
            self
        }
    };
}

pub(crate) use item_a11y_builders;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::SceneItem;
    use bastyde_canvas::Rect;

    #[test]
    fn literal_twins_match_translated_setters_via_observable_state() {
        // The `_literal` twin must produce the same observable state
        // as its translated counterpart — they're a grep-marker for
        // explicitly-untranslated call sites, not a behavior split.
        // We compare via the public `SceneItem::label` getter.
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);

        let translated = RectItem::new(r).label("Hello");
        let literal = RectItem::new(r).label(lit!("Hello"));
        // The builder shadows the trait getter — disambiguate via UFCS.
        assert_eq!(SceneItem::label(&translated), SceneItem::label(&literal));

        let t1 = TextItem::new("hi", r);
        let t2 = TextItem::new(lit!("hi"), r);
        assert_eq!(t1.local_bounds(), t2.local_bounds());

        let mut h1 = crate::item_handlers::SceneItemHandlerSet::new();
        h1.tooltip("Tip");
        let mut h2 = crate::item_handlers::SceneItemHandlerSet::new();
        h2.tooltip(lit!("Tip"));
        assert_eq!(h1.tooltip, h2.tooltip);
    }

    #[test]
    fn access_subtree_mode_round_trips() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let item = RectItem::new(r).access_merge_subtree();
        assert_eq!(item.access_subtree_mode(), AccessSubtreeMode::Merge);
        let item = RectItem::new(r).access_subtree(AccessSubtreeMode::Exclude);
        assert_eq!(item.access_subtree_mode(), AccessSubtreeMode::Exclude);
        let item = RectItem::new(r);
        assert_eq!(item.access_subtree_mode(), AccessSubtreeMode::Inherit);
    }
}
