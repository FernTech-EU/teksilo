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
//!   as logical AT containers (`Scene::add_a11y_group`).
//!
//! All built-ins store their geometry in **local item coordinates**
//! anchored at the origin. Apps construct an item with its size at
//! origin (`RectItem::new(Rect::new(0.0, 0.0, w, h))`) and place it
//! in the scene with `Scene::add_item(item, local_pos)`.

use fern_canvas::{Canvas, Path, Point, Rect, StrokeStyle};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::item::{SceneItem, SceneItemA11yContext, SceneItemPaintContext};

/// Text source for [`TextItem`]: either a static string or a live
/// `Signal<String>`. Signal-bound text refreshes on each paint and
/// dirties the SceneView via `register_bindings`.
#[derive(Debug)]
enum TextSource {
    Static(String),
    Bound(Signal<String>),
}

impl TextSource {
    fn current(&self) -> String {
        match self {
            TextSource::Static(s) => s.clone(),
            TextSource::Bound(signal) => signal.get(),
        }
    }
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
    label: Option<String>,
    description: Option<String>,
    role: Option<accesskit::Role>,
    hidden: bool,
    pub(crate) subtree_mode: AccessSubtreeMode,
}

impl ItemA11yOverrides {
    /// Read access for the AT walker.
    pub fn subtree_mode(&self) -> AccessSubtreeMode {
        self.subtree_mode
    }
}

impl ItemA11yOverrides {
    fn apply(&self, builder: &mut AccessNodeBuilder) {
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
/// `a11y: ItemA11yOverrides` field.
macro_rules! item_a11y_builders {
    () => {
        /// Override the AT name announced for this item. Accepts
        /// anything convertible into [`LocalizedString`] — most
        /// commonly `tr!(...)` for translated labels, or any plain
        /// string (which auto-converts via `From<String>`).
        pub fn access_label(
            mut self,
            label: impl Into<fern_i18n::LocalizedString>,
        ) -> Self {
            let ls: fern_i18n::LocalizedString = label.into();
            self.a11y.label = Some(ls.resolve_now());
            self
        }

        /// Untranslated twin of [`access_label`](Self::access_label).
        /// Wraps a raw string in
        /// [`LocalizedString::literal`](fern_i18n::LocalizedString::literal)
        /// — a grep-marker for call sites that intentionally bypass
        /// the i18n pipeline (debug demos, engine-internal labels).
        #[doc(hidden)]
        pub fn access_label_literal(self, label: impl Into<String>) -> Self {
            self.access_label(fern_i18n::LocalizedString::literal(label))
        }

        /// Long-form context appended to the item's announcement.
        pub fn access_description(
            mut self,
            description: impl Into<fern_i18n::LocalizedString>,
        ) -> Self {
            let ls: fern_i18n::LocalizedString = description.into();
            self.a11y.description = Some(ls.resolve_now());
            self
        }

        /// Untranslated twin of [`access_description`](Self::access_description).
        #[doc(hidden)]
        pub fn access_description_literal(
            self,
            description: impl Into<String>,
        ) -> Self {
            self.access_description(fern_i18n::LocalizedString::literal(description))
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

// ---------------------------------------------------------------------------
// RectItem
// ---------------------------------------------------------------------------

/// A rectangle with optional fill and stroke, in local item coordinates.
///
/// Construct with `RectItem::new(Rect::new(0.0, 0.0, w, h))` and place
/// in the scene via `Scene::add_item(rect, local_pos)`.
#[derive(Debug)]
pub struct RectItem {
    local_bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
    flags: crate::flags::ItemFlags,
    a11y: ItemA11yOverrides,
}

impl RectItem {
    /// A rectangle of the given size in local item coordinates. The
    /// passed `local_bounds` is stored verbatim — typically
    /// `Rect::new(0.0, 0.0, w, h)`. No fill, no stroke — set at least
    /// one or the item is invisible.
    pub fn new(local_bounds: Rect) -> Self {
        Self {
            local_bounds,
            fill: None,
            stroke: None,
            label: None,
            flags: crate::flags::ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (scene-coord pixels — they scale with
    /// the view zoom).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label used for debug and the default AT name.
    /// Takes anything convertible into [`LocalizedString`] — most
    /// commonly `tr!(...)`. Plain strings auto-convert.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label). Wraps the
    /// argument via [`LocalizedString::literal`](fern_i18n::LocalizedString::literal).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }

    /// Opt the rectangle into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(crate::flags::ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    item_a11y_builders!();
}

impl SceneItem for RectItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_rect(self.local_bounds, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_rect(self.local_bounds, color, StrokeStyle::solid(width));
        }
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn initial_flags(&self) -> crate::flags::ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> crate::items::AccessSubtreeMode {
        self.a11y.subtree_mode
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::GraphicsObject);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}

// ---------------------------------------------------------------------------
// PathItem
// ---------------------------------------------------------------------------

/// An arbitrary vector path with optional fill and stroke, in local
/// item coordinates.
///
/// The path's commands are evaluated in local space. Stroke widths
/// scale with view zoom. The caller-provided `local_bounds` AABB is
/// what the spatial index buckets on; it must enclose the path's
/// strokes (including stroke half-width on each side).
#[derive(Debug)]
pub struct PathItem {
    path: Path,
    local_bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
    flags: crate::flags::ItemFlags,
    a11y: ItemA11yOverrides,
}

impl PathItem {
    /// A path with a caller-provided AABB in local coordinates. The
    /// path's points are interpreted as local — `(0, 0)` is the
    /// item's anchor.
    pub fn new(path: Path, local_bounds: Rect) -> Self {
        Self {
            path,
            local_bounds,
            fill: None,
            stroke: None,
            label: None,
            flags: crate::flags::ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (scene-coord pixels).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }

    /// Opt the path into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(crate::flags::ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    item_a11y_builders!();
}

impl SceneItem for PathItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        // The path's geometry is in local coords and stays fixed; only
        // the AABB tracks. Apps that want to *move* a path move the
        // item via `Scene::set_local_pos`. Apps that want to *resize*
        // a path rebuild the item from scratch.
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_path(&self.path, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_path(&self.path, color, StrokeStyle::solid(width));
        }
    }

    fn shape_contains(&self, local_pt: Point) -> bool {
        // Stroke-only paths use per-segment distance to match what
        // users see; filled and mixed fill+stroke paths use AABB
        // (the fill region is the dominant target). Quad/cubic/arc
        // segments fall back to AABB.
        let stroke_width = match self.stroke {
            Some((_, w)) => w,
            None => return self.local_bounds.contains(local_pt),
        };
        if self.fill.is_some() {
            return self.local_bounds.contains(local_pt);
        }
        let tolerance = stroke_width.max(0.0) * 0.5 + 2.0;
        let mut current = Point::ZERO;
        let mut start = Point::ZERO;
        for cmd in &self.path.commands {
            match cmd {
                fern_canvas::PathCommand::MoveTo(p) => {
                    current = *p;
                    start = *p;
                }
                fern_canvas::PathCommand::LineTo(p) => {
                    if point_to_segment_distance(local_pt, current, *p) <= tolerance {
                        return true;
                    }
                    current = *p;
                }
                fern_canvas::PathCommand::Close => {
                    if point_to_segment_distance(local_pt, current, start) <= tolerance {
                        return true;
                    }
                    current = start;
                }
                _ => return self.local_bounds.contains(local_pt),
            }
        }
        false
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn initial_flags(&self) -> crate::flags::ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> crate::items::AccessSubtreeMode {
        self.a11y.subtree_mode
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::GraphicsObject);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}

/// Shortest distance from a point to a line segment.
fn point_to_segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 < 1e-6 {
        let dx = p.x - a.x;
        let dy = p.y - a.y;
        return (dx * dx + dy * dy).sqrt();
    }
    let apx = p.x - a.x;
    let apy = p.y - a.y;
    let t = ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0);
    let cx = a.x + t * abx;
    let cy = a.y + t * aby;
    let dx = p.x - cx;
    let dy = p.y - cy;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// ImageItem
// ---------------------------------------------------------------------------

/// A raster image in a local-coord rectangle. The image is referenced
/// by name (the Canvas image registry).
#[derive(Debug)]
pub struct ImageItem {
    local_bounds: Rect,
    name: String,
    label: Option<String>,
    flags: crate::flags::ItemFlags,
    a11y: ItemA11yOverrides,
}

impl ImageItem {
    /// An image item of the given size in local coordinates,
    /// referencing the image registered under `name`.
    pub fn new(local_bounds: Rect, name: impl Into<String>) -> Self {
        Self {
            local_bounds,
            name: name.into(),
            label: None,
            flags: crate::flags::ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }

    /// Opt the image into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(crate::flags::ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    item_a11y_builders!();
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

    fn initial_flags(&self) -> crate::flags::ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> crate::items::AccessSubtreeMode {
        self.a11y.subtree_mode
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::Image);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}

// ---------------------------------------------------------------------------
// TextItem
// ---------------------------------------------------------------------------

/// Unstyled text in a local-coord rectangle. Text wraps within the
/// rect; size is the caller's responsibility.
#[derive(Debug)]
pub struct TextItem {
    text: TextSource,
    local_bounds: Rect,
    color: Color,
    label: Option<String>,
    flags: crate::flags::ItemFlags,
    a11y: ItemA11yOverrides,
}

impl TextItem {
    /// A static-text item in local coordinates. The `text` is
    /// resolved eagerly via [`LocalizedString::resolve_now`] at
    /// construction; locale changes rebuild the composite parent,
    /// which re-creates this `TextItem` with a fresh translation.
    pub fn new(
        text: impl Into<fern_i18n::LocalizedString>,
        local_bounds: Rect,
    ) -> Self {
        let ls: fern_i18n::LocalizedString = text.into();
        Self {
            text: TextSource::Static(ls.resolve_now()),
            local_bounds,
            color: Color::BLACK,
            label: None,
            flags: crate::flags::ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Untranslated twin of [`new`](Self::new). Wraps the argument
    /// via [`LocalizedString::literal`](fern_i18n::LocalizedString::literal).
    #[doc(hidden)]
    pub fn new_literal(text: impl Into<String>, local_bounds: Rect) -> Self {
        Self::new(fern_i18n::LocalizedString::literal(text), local_bounds)
    }

    /// A text item whose content is driven by a `Signal<String>`.
    /// `register_bindings` ties the signal to the SceneView at
    /// `BindingLevel::RepaintOnly` so changes dirty paint and the
    /// next walk reads the current value.
    pub fn with_signal_text(text: Signal<String>, local_bounds: Rect) -> Self {
        Self {
            text: TextSource::Bound(text),
            local_bounds,
            color: Color::BLACK,
            label: None,
            flags: crate::flags::ItemFlags::default(),
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Opt the text into drag-to-move.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.flags.set(crate::flags::ItemFlags::IS_DRAGGABLE, draggable);
        self
    }

    /// Override the foreground color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Override the AT label (defaults to the current text content).
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }

    item_a11y_builders!();
}

impl SceneItem for TextItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        let text = self.text.current();
        let style = fern_tokens::TextStyle::default();
        if canvas.text_backend().is_some() {
            canvas.draw_paragraph(&text, self.local_bounds, &style, self.color, None);
        } else {
            canvas.draw_text(&text, self.local_bounds, &style, self.color);
        }
    }

    fn label(&self) -> Option<String> {
        self.label.clone().or_else(|| Some(self.text.current()))
    }

    fn initial_flags(&self) -> crate::flags::ItemFlags {
        self.flags
    }

    fn access_subtree_mode(&self) -> crate::items::AccessSubtreeMode {
        self.a11y.subtree_mode
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::Label);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }

    fn register_bindings(&self, ctx: &mut BuildContext, view_id: WidgetId) {
        if let TextSource::Bound(signal) = &self.text {
            signal.bind_to(view_id, ctx.binding_registry(), BindingLevel::RepaintOnly);
        }
    }
}

// ---------------------------------------------------------------------------
// GroupItem
// ---------------------------------------------------------------------------

/// A group container with optional fill / stroke / inline label, in
/// local item coordinates.
///
/// Visually, GroupItem renders a labelled box around its members.
/// Logically, it's the AT-grouping primitive: with no chrome and a
/// label set, it announces itself to AT but draws nothing.
#[derive(Debug)]
pub struct GroupItem {
    local_bounds: Rect,
    label: Option<String>,
    show_label: bool,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    corner_radius: f32,
    label_inset: (f32, f32),
    label_color: Option<Color>,
    a11y: ItemA11yOverrides,
}

impl GroupItem {
    /// A group covering `local_bounds` in local coordinates. No
    /// chrome by default — call `fill` / `stroke` / `show_label` to
    /// give it visible outline / background / inline label.
    pub fn new(local_bounds: Rect) -> Self {
        Self {
            local_bounds,
            label: None,
            show_label: false,
            fill: None,
            stroke: None,
            corner_radius: 0.0,
            label_inset: (8.0, 4.0),
            label_color: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Human-readable label, used as the default AT group name and
    /// (when `show_label` is enabled) rendered inline at top-leading.
    pub fn label(mut self, label: impl Into<fern_i18n::LocalizedString>) -> Self {
        let ls: fern_i18n::LocalizedString = label.into();
        self.label = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`label`](Self::label).
    #[doc(hidden)]
    pub fn label_literal(self, label: impl Into<String>) -> Self {
        self.label(fern_i18n::LocalizedString::literal(label))
    }

    /// Render the label inline at paint time.
    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    /// Override the inset of the inline label from the local origin.
    pub fn label_inset(mut self, dx: f32, dy: f32) -> Self {
        self.label_inset = (dx, dy);
        self
    }

    /// Override the inline label color. Defaults to the stroke
    /// color if set, else `Color::BLACK`.
    pub fn label_color(mut self, color: Color) -> Self {
        self.label_color = Some(color);
        self
    }

    /// Background fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Border stroke (color + scene-coord pixel width).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Rounded corners for fill and stroke. Default `0.0`.
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius.max(0.0);
        self
    }

    /// Whether the group has any visual chrome configured.
    pub fn is_visual(&self) -> bool {
        self.fill.is_some() || self.stroke.is_some() || self.show_label
    }

    item_a11y_builders!();
}

impl SceneItem for GroupItem {
    fn local_bounds(&self) -> Rect {
        self.local_bounds
    }

    fn set_local_bounds(&mut self, bounds: Rect) {
        self.local_bounds = bounds;
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if !self.is_visual() {
            return;
        }
        let lb = self.local_bounds;
        if let Some(fill) = self.fill {
            if self.corner_radius > 0.0 {
                canvas.fill_rounded_rect(
                    lb,
                    fern_tokens::CornerRadius::uniform(self.corner_radius),
                    fill,
                );
            } else {
                canvas.fill_rect(lb, fill);
            }
        }
        if let Some((color, width)) = self.stroke {
            if self.corner_radius > 0.0 {
                canvas.stroke_rounded_rect(
                    lb,
                    fern_tokens::CornerRadius::uniform(self.corner_radius),
                    color,
                    StrokeStyle::solid(width),
                );
            } else {
                canvas.stroke_rect(lb, color, StrokeStyle::solid(width));
            }
        }
        if self.show_label {
            if let Some(label) = &self.label {
                let color = self
                    .label_color
                    .or_else(|| self.stroke.map(|(c, _)| c))
                    .unwrap_or(Color::BLACK);
                let (dx, dy) = self.label_inset;
                let label_bounds = Rect::new(
                    lb.x + dx,
                    lb.y + dy,
                    (lb.width - 2.0 * dx).max(0.0),
                    (lb.height - 2.0 * dy).max(0.0),
                );
                canvas.draw_text(
                    label,
                    label_bounds,
                    &fern_tokens::TextStyle::default(),
                    color,
                );
            }
        }
    }

    /// Non-visual GroupItems pass clicks through to items beneath.
    /// Visual groups (with fill / stroke / inline label) AABB-hit-test
    /// so apps can wire group-level click handlers.
    fn shape_contains(&self, local_pt: Point) -> bool {
        if self.is_visual() {
            self.local_bounds.contains(local_pt)
        } else {
            false
        }
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::Group);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }

    fn access_subtree_mode(&self) -> AccessSubtreeMode {
        self.a11y.subtree_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_twins_match_translated_setters() {
        // Each `_literal` twin must produce the same internal state
        // as its translated counterpart — they're a grep-marker for
        // explicitly-untranslated call sites, not a behavior split.
        // Reach into the private fields directly (test module ⇒
        // same-crate access) since builders shadow the getter name.
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);

        let a = RectItem::new(r)
            .label("Hello")
            .access_label("AT")
            .access_description("desc");
        let b = RectItem::new(r)
            .label_literal("Hello")
            .access_label_literal("AT")
            .access_description_literal("desc");
        assert_eq!(a.label, b.label);
        assert_eq!(a.a11y.label, b.a11y.label);
        assert_eq!(a.a11y.description, b.a11y.description);

        // TextItem ::new vs ::new_literal.
        let t1 = TextItem::new("hi", r);
        let t2 = TextItem::new_literal("hi", r);
        assert_eq!(t1.local_bounds(), t2.local_bounds());

        // SceneItemHandlerSet tooltip vs tooltip_literal.
        let mut h1 = crate::item_handlers::SceneItemHandlerSet::new();
        h1.tooltip("Tip");
        let mut h2 = crate::item_handlers::SceneItemHandlerSet::new();
        h2.tooltip_literal("Tip");
        assert_eq!(h1.tooltip, h2.tooltip);
    }

    #[test]
    fn access_subtree_mode_round_trips() {
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0)).access_merge_subtree();
        assert_eq!(item.access_subtree_mode(), AccessSubtreeMode::Merge);
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .access_subtree(AccessSubtreeMode::Exclude);
        assert_eq!(item.access_subtree_mode(), AccessSubtreeMode::Exclude);
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(item.access_subtree_mode(), AccessSubtreeMode::Inherit);
    }

    #[test]
    fn rect_item_local_bounds_round_trip() {
        let r = Rect::new(0.0, 0.0, 30.0, 40.0);
        let item = RectItem::new(r);
        assert_eq!(item.local_bounds(), r);
    }

    #[test]
    fn rect_item_default_shape_contains() {
        let item = RectItem::new(Rect::new(0.0, 0.0, 50.0, 50.0));
        assert!(item.shape_contains(Point::new(20.0, 20.0)));
        assert!(!item.shape_contains(Point::new(-5.0, 20.0)));
    }

    #[test]
    fn rect_item_paint_emits_fill_and_stroke() {
        let mut canvas = Canvas::new();
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(Color::RED)
            .stroke(Color::BLUE, 2.0);
        let ctx = SceneItemPaintContext::new(fern_canvas::Transform2D::identity(), None);
        item.paint(&mut canvas, &ctx);
        let frame = canvas.into_render_frame();
        assert!(
            !frame.draw_order.is_empty(),
            "paint must emit at least one draw command"
        );
    }

    #[test]
    fn path_item_holds_path_and_local_bounds() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 50.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 50.0)).stroke(Color::BLACK, 1.5);
        assert_eq!(item.local_bounds(), Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn path_item_per_segment_shape_contains_stroke_only() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 100.0));
        let item =
            PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 2.0);

        assert!(item.shape_contains(Point::new(50.0, 50.0)));
        assert!(item.shape_contains(Point::new(52.0, 50.0)));
        assert!(!item.shape_contains(Point::new(80.0, 20.0)));
        assert!(!item.shape_contains(Point::new(200.0, 200.0)));
    }

    #[test]
    fn path_item_filled_uses_aabb_shape_contains() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 100.0))
            .line_to(Point::new(0.0, 100.0))
            .close();
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).fill(Color::RED);
        assert!(item.shape_contains(Point::new(50.0, 50.0)));
        assert!(!item.shape_contains(Point::new(200.0, 50.0)));
    }

    #[test]
    fn path_item_close_segment_hit_tested() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(50.0, 100.0))
            .close();
        let item =
            PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 2.0);
        assert!(item.shape_contains(Point::new(25.0, 50.0)));
    }

    #[test]
    fn path_item_curve_falls_back_to_aabb() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        let item =
            PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 2.0);
        assert!(item.shape_contains(Point::new(50.0, 99.0)));
    }

    #[test]
    fn group_item_does_not_hit_test_through_aabb() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 1000.0, 1000.0));
        assert!(!g.shape_contains(Point::new(500.0, 500.0)));
    }

    #[test]
    fn text_item_label_falls_back_to_text() {
        let item = TextItem::new("Hello", Rect::new(0.0, 0.0, 100.0, 30.0));
        assert_eq!(SceneItem::label(&item).as_deref(), Some("Hello"));
    }

    #[test]
    fn group_item_default_is_not_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(!g.is_visual());
    }

    #[test]
    fn group_item_with_fill_is_visual_and_hit_tests() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).fill(Color::RED);
        assert!(g.is_visual());
        assert!(g.shape_contains(Point::new(50.0, 50.0)));
        assert!(!g.shape_contains(Point::new(150.0, 50.0)));
    }

    #[test]
    fn group_item_with_stroke_only_is_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).stroke(Color::BLACK, 1.0);
        assert!(g.is_visual());
        assert!(g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_with_label_only_is_not_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0)).label("Act 1");
        assert!(!g.is_visual());
        assert!(!g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_with_show_label_is_visual() {
        let g = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            .label("Act 1")
            .show_label(true);
        assert!(g.is_visual());
        assert!(g.shape_contains(Point::new(50.0, 50.0)));
    }

    #[test]
    fn group_item_visual_paint_emits_draws() {
        let invisible = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0));
        let visible = GroupItem::new(Rect::new(0.0, 0.0, 100.0, 100.0))
            .fill(Color::RED)
            .stroke(Color::BLACK, 2.0)
            .corner_radius(8.0);

        let ctx = SceneItemPaintContext::new(fern_canvas::Transform2D::identity(), None);

        let mut c1 = fern_canvas::Canvas::new();
        invisible.paint(&mut c1, &ctx);
        let f1 = c1.into_render_frame();
        assert!(f1.draw_order.is_empty(), "invisible group emitted draws");

        let mut c2 = fern_canvas::Canvas::new();
        visible.paint(&mut c2, &ctx);
        let f2 = c2.into_render_frame();
        assert!(!f2.draw_order.is_empty(), "visible group emitted no draws");
    }
}
