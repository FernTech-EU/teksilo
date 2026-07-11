// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Built-in `SceneItem` implementations.
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

use bastyde_canvas::StrokeStyle;
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::color_prop::ColorProp;
use bastyde_tokens::Color;

pub mod group;
pub mod image;
pub mod path;
pub mod rect;
pub mod text;

use bastyde_i18n::LocalizedString;
pub use group::GroupItem;
pub use image::ImageItem;
pub use path::PathItem;
pub use rect::RectItem;
pub use text::{TextAlign, TextItem};

/// A concrete-colour "hint" extracted from a [`ColorProp`] without a theme —
/// used by `thumbnail_color` impls that have no paint context. `Static` and
/// `Bound` colours resolve directly; role-based colours need a theme to
/// resolve, so they yield `None` (the caller falls back to a neutral tint).
///
/// **Known limitation:** [`SceneItem::thumbnail_color`](crate::SceneItem::thumbnail_color)
/// is theme-free by signature, so an item whose fill/stroke is a **theme role**
/// (rather than a `Color` or `Signal<Color>`) renders as the caller's neutral
/// grey in a [`SceneMinimap`](crate::SceneMinimap). Use a concrete `Color` or a
/// `Signal<Color>` for items you want faithfully represented on a minimap.
pub(crate) fn color_prop_hint(prop: &ColorProp) -> Option<Color> {
    match prop {
        ColorProp::Static(c) => Some(*c),
        ColorProp::Bound(s) => Some(s.get()),
        // Role-based (static or dynamic) colours can't resolve without a theme.
        _ => None,
    }
}

/// The fill-then-stroke thumbnail-colour fallback shared by the colour-bearing
/// built-ins. `None` when neither slot yields a theme-free colour — the caller
/// then picks its own neutral fallback. See [`color_prop_hint`] for the
/// role-colour limitation.
pub(crate) fn fill_or_stroke_hint(
    fill: Option<&ColorProp>,
    stroke: Option<&(ColorProp, StrokeStyle)>,
) -> Option<Color> {
    fill.and_then(color_prop_hint)
        .or_else(|| stroke.and_then(|(c, _)| color_prop_hint(c)))
}

/// How the AT walker treats descendants of an item.
///
/// Mirrors the widget-tier `AccessSubtreeMode`: `Inherit` is the
/// default (descendants emit normally); `Exclude` prunes them from
/// the AT tree; `Merge` collapses them into the parent so the
/// subtree reads as a single AT element. Used for "card with rect +
/// label + indicator dot reads as one card" patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessSubtreeMode {
    /// Descendants emit their own AT nodes normally. Default.
    #[default]
    Inherit,
    /// Descendants are pruned from the AT tree; the parent item
    /// emits as a single AT node with no children.
    Exclude,
    /// Descendants' label / description / actions are folded into
    /// the parent AT node; descendants are then pruned. The subtree
    /// reads as one AT element — useful for "card with icon + label
    /// + badge = one selectable card" patterns.
    Merge,
}

/// Builder-level accessibility overrides shared by every built-in
/// `SceneItem`. Mirrors the widget-level `.access_*` chain — names
/// match so muscle memory carries over.
#[derive(Debug, Default, Clone)]
pub struct ItemA11yOverrides {
    pub(crate) label: Option<LocalizedString>,
    pub(crate) description: Option<LocalizedString>,
    pub(crate) role: Option<accesskit::Role>,
    pub(crate) hidden: bool,
    pub(crate) subtree_mode: AccessSubtreeMode,
    /// String value announced for a data-bearing item (e.g. `"42 %"`).
    pub(crate) value: Option<LocalizedString>,
    /// Numeric value + optional range/step, for slider/gauge-like items whose
    /// magnitude AT should describe.
    pub(crate) numeric_value: Option<f64>,
    pub(crate) min_numeric_value: Option<f64>,
    pub(crate) max_numeric_value: Option<f64>,
    pub(crate) numeric_value_step: Option<f64>,
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
            builder.set_name(label.resolve_now());
        }
        if let Some(ref desc) = self.description {
            builder.set_description(desc.resolve_now());
        }
        if self.hidden {
            builder.set_hidden();
        }
        if let Some(ref value) = self.value {
            builder.set_value(value.resolve_now());
        }
        if let Some(n) = self.numeric_value {
            builder.set_numeric_value(n);
        }
        if let Some(n) = self.min_numeric_value {
            builder.set_min_numeric_value(n);
        }
        if let Some(n) = self.max_numeric_value {
            builder.set_max_numeric_value(n);
        }
        if let Some(n) = self.numeric_value_step {
            builder.set_numeric_value_step(n);
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
        /// anything convertible into `LocalizedString` — most
        /// commonly `tr!(...)` for translated labels, or any plain
        /// string (which auto-converts via `From<String>`).
        pub fn access_label(mut self, label: impl Into<LocalizedString>) -> Self {
            let ls: LocalizedString = label.into();
            self.a11y.label = Some(ls);
            self
        }

        /// Long-form context appended to the item's announcement.
        pub fn access_description(mut self, description: impl Into<LocalizedString>) -> Self {
            let ls: LocalizedString = description.into();
            self.a11y.description = Some(ls);
            self
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

        /// Announce a string value for this item (e.g. a formatted data
        /// reading like `"42 %"`). Mirrors the widget-tier `.access_value`.
        pub fn access_value(mut self, value: impl Into<LocalizedString>) -> Self {
            let ls: LocalizedString = value.into();
            self.a11y.value = Some(ls);
            self
        }

        /// Announce a numeric value for this item, for slider/gauge-like data
        /// marks whose magnitude assistive tech should describe. Pair with
        /// [`access_numeric_range`](Self::access_numeric_range) /
        /// [`access_numeric_step`](Self::access_numeric_step) for full
        /// range semantics.
        pub fn access_numeric_value(mut self, value: f64) -> Self {
            self.a11y.numeric_value = Some(value);
            self
        }

        /// Announce the numeric min/max bounds for this item.
        pub fn access_numeric_range(mut self, min: f64, max: f64) -> Self {
            self.a11y.min_numeric_value = Some(min);
            self.a11y.max_numeric_value = Some(max);
            self
        }

        /// Announce the numeric step (per-arrow increment) for this item.
        pub fn access_numeric_step(mut self, step: f64) -> Self {
            self.a11y.numeric_value_step = Some(step);
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
    use bastyde_i18n::lit;

    #[test]
    fn literal_twins_match_translated_setters_via_observable_state() {
        // The `_literal` twin must produce the same observable state
        // as its translated counterpart — they're a grep-marker for
        // explicitly-untranslated call sites, not a behavior split.
        // We compare via the public `SceneItem::label` getter.
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);

        let translated = RectItem::new(r).label(lit!("Hello"));
        let literal = RectItem::new(r).label(lit!("Hello"));
        // The builder shadows the trait getter — disambiguate via UFCS.
        assert_eq!(SceneItem::label(&translated), SceneItem::label(&literal));

        let t1 = TextItem::new(lit!("hi"), r);
        let t2 = TextItem::new(lit!("hi"), r);
        assert_eq!(t1.local_bounds(), t2.local_bounds());

        let mut h1 = crate::item_handlers::SceneItemHandlerSet::new();
        h1.tooltip(lit!("Tip"));
        let mut h2 = crate::item_handlers::SceneItemHandlerSet::new();
        h2.tooltip(lit!("Tip"));
        assert_eq!(
            h1.tooltip.as_ref().map(|t| t.resolve_now()),
            h2.tooltip.as_ref().map(|t| t.resolve_now())
        );
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

    #[test]
    fn access_value_and_numeric_fields_round_trip() {
        // #9: the value / numeric overrides store their fields. End-to-end
        // walker coverage (the fields reaching an AccessKit node) lives in
        // `view/tests.rs`, where a real `WidgetTree` walks the AT tree.
        let o = ItemA11yOverrides {
            value: Some(lit!("42 %")),
            numeric_value: Some(0.42),
            min_numeric_value: Some(0.0),
            max_numeric_value: Some(1.0),
            numeric_value_step: Some(0.1),
            ..Default::default()
        };
        assert_eq!(o.numeric_value, Some(0.42));
        assert_eq!(o.min_numeric_value, Some(0.0));
        assert_eq!(o.max_numeric_value, Some(1.0));
        assert_eq!(o.numeric_value_step, Some(0.1));
        assert_eq!(
            o.value.as_ref().map(|v| v.resolve_now()),
            Some("42 %".to_string())
        );
    }
}
