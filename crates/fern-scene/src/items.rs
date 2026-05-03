//! Built-in [`SceneItem`] implementations.
//!
//! Phase 4 ships five items covering the common decoration cases:
//!
//! - [`RectItem`] — filled / stroked rectangle. Use for backgrounds,
//!   tile patterns, simple decorations.
//! - [`PathItem`] — arbitrary vector path with optional fill and
//!   stroke. The "connector lines between cards" workhorse.
//! - [`ImageItem`] — a raster image at a scene-coord rectangle.
//!   References the image by name (the Canvas image registry).
//! - [`TextItem`] — unstyled text at a scene-coord position. Uses
//!   the canvas's default text rendering path.
//! - [`GroupItem`] — a logical-only container. Phase 4 paints
//!   nothing; Phase 5 uses it to declare AT structure (Acts →
//!   Scenes etc.) without a visual counterpart.
//!
//! Custom items: implement [`SceneItem`] directly. The trait is
//! deliberately small (`bounds_in_scene` + `paint` + optional
//! `hit_test` + optional `label`).

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
/// dirties the SceneView via `register_bindings` so the visual
/// updates when the signal changes — used by chart-style nested
/// scenes where outer axis labels read inner pan/zoom signals.
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

/// Builder-level accessibility overrides shared by every built-in
/// `SceneItem`. Mirrors the widget-level `.access_*` chain in CLAUDE.md
/// (`access_label` / `access_role` / `access_description` /
/// `access_hidden`) — the names match so muscle memory carries from
/// widgets to scene items.
///
/// `AccessNodeBuilder::set_*` semantics are "scalars replace if set"
/// — so a per-item `.access_label("X")` wins over the default
/// derived from `SceneItem::label()`. `apply` runs *after* the
/// default `accessibility` impl populates role + label, so the
/// per-item layer always lands on top.
#[derive(Debug, Default, Clone)]
struct ItemA11yOverrides {
    label: Option<String>,
    description: Option<String>,
    role: Option<accesskit::Role>,
    hidden: bool,
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

/// Macro: emit the per-item `.access_*` builder chain on a struct
/// that holds an `a11y: ItemA11yOverrides` field. Keeps the four
/// methods consistent across `RectItem` / `PathItem` / `ImageItem` /
/// `TextItem` / `GroupItem` without re-typing the bodies.
macro_rules! item_a11y_builders {
    () => {
        /// Override the AT name announced for this item. Default:
        /// `label()` (which falls back to `text` for `TextItem`,
        /// `None` otherwise).
        pub fn access_label(mut self, label: impl Into<String>) -> Self {
            self.a11y.label = Some(label.into());
            self
        }

        /// Long-form context appended to the item's announcement.
        pub fn access_description(mut self, description: impl Into<String>) -> Self {
            self.a11y.description = Some(description.into());
            self
        }

        /// Override the AccessKit role for this item. Default:
        /// item-shape-derived (`GraphicsObject` for `RectItem` /
        /// `PathItem`, `Image` for `ImageItem`, `StaticText` for
        /// `TextItem`, `Group` for `GroupItem`).
        pub fn access_role(mut self, role: accesskit::Role) -> Self {
            self.a11y.role = Some(role);
            self
        }

        /// Hide this item from the AT tree entirely. Equivalent to
        /// `accesskit::Node::set_hidden`. Use sparingly — most
        /// "decorative-only" items should rely on the default
        /// `GraphicsObject` role being interpretable by AT clients.
        pub fn access_hidden(mut self, hidden: bool) -> Self {
            self.a11y.hidden = hidden;
            self
        }
    };
}

// ---------------------------------------------------------------------------
// RectItem
// ---------------------------------------------------------------------------

/// A scene-coord rectangle with optional fill and stroke. Used for
/// backgrounds, tile patterns, simple decorations.
#[derive(Debug)]
pub struct RectItem {
    bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
    a11y: ItemA11yOverrides,
}

impl RectItem {
    /// A rectangle at the given scene-coord bounds. No fill, no
    /// stroke — set at least one or the item will be invisible.
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            fill: None,
            stroke: None,
            label: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (in scene-coord pixels — they scale
    /// with the view zoom).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label used for debug introspection and the
    /// default a11y walker name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    item_a11y_builders!();
}

impl SceneItem for RectItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_rect(self.bounds, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_rect(self.bounds, color, StrokeStyle::solid(width));
        }
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        // Rect items default to GraphicsObject — they're decorations.
        // Apps that want a more specific role (e.g. Role::Image for a
        // colored square that represents a logo) override via
        // `.access_role(...)`.
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

/// An arbitrary vector path (polyline, bezier, etc.) with optional
/// fill and stroke. Use for connector lines between cards, custom
/// shapes, hand-drawn-style decorations.
///
/// The path is evaluated in scene-coord space; fill / stroke widths
/// scale with the view zoom.
#[derive(Debug)]
pub struct PathItem {
    path: Path,
    bounds: Rect,
    fill: Option<Color>,
    stroke: Option<(Color, f32)>,
    label: Option<String>,
    a11y: ItemA11yOverrides,
}

impl PathItem {
    /// A path with a caller-provided AABB. The bounds are *not*
    /// derived from the path automatically — callers know the path's
    /// extent at construction time and pass it in. The bounds are
    /// what the spatial index buckets on, so they need to fully
    /// enclose the path's strokes (including stroke half-width on
    /// each side if you care about partial culling).
    pub fn new(path: Path, bounds: Rect) -> Self {
        Self {
            path,
            bounds,
            fill: None,
            stroke: None,
            label: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Fill color.
    pub fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    /// Stroke color and width (in scene-coord pixels).
    pub fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some((color, width.max(0.0)));
        self
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    item_a11y_builders!();
}

impl SceneItem for PathItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        if let Some(fill) = self.fill {
            canvas.fill_path(&self.path, fill);
        }
        if let Some((color, width)) = self.stroke {
            canvas.stroke_path(&self.path, color, StrokeStyle::solid(width));
        }
    }

    fn hit_test(&self, scene_point: Point) -> bool {
        // Per-segment hit-test for stroked paths: a click "near"
        // any line segment counts as a hit, where "near" is the
        // stroke half-width plus a 2px tolerance for fingertip /
        // mouse imprecision. The default AABB hit-test is too
        // loose for thin connector lines (a long diagonal line
        // has an AABB the size of its bounding rect; clicking
        // anywhere in the rect would falsely hit the line). The
        // per-segment check matches what users see.
        //
        // Filled paths still use AABB (matches the visual).
        // Mixed fill+stroke uses AABB (the fill region is the
        // dominant target). Stroke-only paths use the segment
        // walk.
        let stroke_width = match self.stroke {
            Some((_, w)) => w,
            None => return self.bounds.contains(scene_point),
        };
        if self.fill.is_some() {
            return self.bounds.contains(scene_point);
        }
        // Stroke-only path: walk segments.
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
                    if point_to_segment_distance(scene_point, current, *p) <= tolerance {
                        return true;
                    }
                    current = *p;
                }
                fern_canvas::PathCommand::Close => {
                    if point_to_segment_distance(scene_point, current, start) <= tolerance {
                        return true;
                    }
                    current = start;
                }
                // Quad / cubic / arc segments fall back to AABB
                // hit. Apps building precision paths with curves
                // can override `hit_test` directly.
                _ => {
                    return self.bounds.contains(scene_point);
                }
            }
        }
        false
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::GraphicsObject);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}

/// Shortest distance from a point to a line segment, in scene
/// coordinates. Used by `PathItem::hit_test` to score per-segment
/// proximity for stroke-only paths.
fn point_to_segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let abx = b.x - a.x;
    let aby = b.y - a.y;
    let len2 = abx * abx + aby * aby;
    if len2 < 1e-6 {
        // Degenerate segment (a == b) — distance to point a.
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

/// A raster image at a scene-coord rectangle. The image is
/// referenced by name — callers register images with the Canvas
/// before the first frame; `ImageItem` just records the lookup
/// string and the destination rect.
#[derive(Debug)]
pub struct ImageItem {
    bounds: Rect,
    name: String,
    label: Option<String>,
    a11y: ItemA11yOverrides,
}

impl ImageItem {
    /// Construct an image item at `bounds`, referencing the image
    /// registered under `name`.
    pub fn new(bounds: Rect, name: impl Into<String>) -> Self {
        Self {
            bounds,
            name: name.into(),
            label: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Human-readable label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    item_a11y_builders!();
}

impl SceneItem for ImageItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        canvas.draw_image(self.bounds, self.name.clone());
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        // Default: Role::Image. Apps that want decorative-only
        // semantics should `.access_hidden(true)`.
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

/// Unstyled text at a scene-coord position. Phase 4 ships a minimal
/// implementation: text + position + colour. Full styling (font
/// stack, weight, italic, line height) lands in Phase 7 polish or
/// when an app needs it; for now, anything fancier should use a
/// heavyweight `TextWidget` placed at `Scene::add_widget`.
#[derive(Debug)]
pub struct TextItem {
    text: TextSource,
    bounds: Rect,
    color: Color,
    label: Option<String>,
    a11y: ItemA11yOverrides,
}

impl TextItem {
    /// Construct a text item at `bounds` with static text. The
    /// text wraps within the rectangle; height is callers'
    /// responsibility to set reasonably (Phase 4 doesn't
    /// auto-measure).
    pub fn new(text: impl Into<String>, bounds: Rect) -> Self {
        Self {
            text: TextSource::Static(text.into()),
            bounds,
            color: Color::BLACK,
            label: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Construct a text item whose text is driven by a live
    /// `Signal<String>`. The visual updates whenever the signal
    /// value changes — `register_bindings` ties the signal to the
    /// SceneView at `BindingLevel::RepaintOnly`, so a change
    /// dirties paint and the next walk draws the current value.
    ///
    /// Use this for axis labels in chart-style outer scenes that
    /// derive their text from an inner SceneView's
    /// `pan_x_signal` / `zoom_signal` / `view_transform_signal`.
    pub fn with_signal_text(text: Signal<String>, bounds: Rect) -> Self {
        Self {
            text: TextSource::Bound(text),
            bounds,
            color: Color::BLACK,
            label: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Override the foreground color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Human-readable label. Defaults to the text content if unset.
    /// For signal-bound text, `label()` returns a snapshot of the
    /// current signal value — apps that need a stable AT name
    /// across pan updates should use `.access_label("X")` instead.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    item_a11y_builders!();
}

impl SceneItem for TextItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        let text = self.text.current();
        canvas.draw_text(
            &text,
            self.bounds,
            &fern_tokens::TextStyle::default(),
            self.color,
        );
    }

    fn label(&self) -> Option<String> {
        // Explicit override wins; otherwise fall back to the
        // current text value (a fresh snapshot of the signal for
        // bound items, a clone of the static string otherwise).
        self.label.clone().or_else(|| Some(self.text.current()))
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        builder.set_role(accesskit::Role::Label);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }

    fn register_bindings(&self, ctx: &mut BuildContext, scene_view_id: WidgetId) {
        if let TextSource::Bound(signal) = &self.text {
            signal.bind_to(
                scene_view_id,
                ctx.binding_registry(),
                BindingLevel::RepaintOnly,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// GroupItem
// ---------------------------------------------------------------------------

/// A logical-only group. Paints nothing in Phase 4 — its purpose is
/// to declare AT structure for the Phase 5b a11y-shaping API
/// (`Scene::add_a11y_group`, `set_a11y_parent`, etc.). Bounds are
/// stored so the spatial index can bucket the group for queries
/// like "what's at this scene coord, including grouping context".
///
/// Authors who want a visible group container should pair this
/// with a [`RectItem`] for the chrome.
#[derive(Debug)]
pub struct GroupItem {
    bounds: Rect,
    label: Option<String>,
    a11y: ItemA11yOverrides,
}

impl GroupItem {
    /// A group covering `bounds` in scene coordinates.
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            label: None,
            a11y: ItemA11yOverrides::default(),
        }
    }

    /// Human-readable label. Used by the a11y walker as the default
    /// group name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    item_a11y_builders!();
}

impl SceneItem for GroupItem {
    fn bounds_in_scene(&self) -> Rect {
        self.bounds
    }

    fn paint(&self, _canvas: &mut Canvas, _ctx: &SceneItemPaintContext) {
        // Logical-only — no visual.
    }

    /// Hit-test default would AABB-contain. Override to
    /// `false` so `GroupItem` doesn't intercept pointer events
    /// meant for items behind it. Phase 5/6 will revisit if
    /// groups need pointer interaction.
    fn hit_test(&self, _scene_point: Point) -> bool {
        false
    }

    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder, _ctx: &SceneItemA11yContext) {
        // Group items get Role::Group regardless of visual: they
        // exist to organize AT structure for items beneath them.
        builder.set_role(accesskit::Role::Group);
        if let Some(label) = self.label() {
            builder.set_name(label);
        }
        self.a11y.apply(builder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_item_bounds_round_trip() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        let item = RectItem::new(r);
        assert_eq!(item.bounds_in_scene(), r);
    }

    #[test]
    fn rect_item_default_hit_test() {
        let item = RectItem::new(Rect::new(10.0, 10.0, 50.0, 50.0));
        assert!(item.hit_test(Point::new(20.0, 20.0)));
        assert!(!item.hit_test(Point::new(5.0, 20.0)));
    }

    #[test]
    fn rect_item_paint_emits_fill_and_stroke() {
        // Sanity: paint adds draw commands to the canvas frame.
        let mut canvas = Canvas::new();
        let item = RectItem::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .fill(Color::RED)
            .stroke(Color::BLUE, 2.0);
        let ctx = SceneItemPaintContext {
            view_transform: fern_canvas::Transform2D::IDENTITY,
            dirty_scene_rect: None,
        };
        item.paint(&mut canvas, &ctx);
        let frame = canvas.into_render_frame();
        assert!(
            !frame.draw_order.is_empty(),
            "paint must emit at least one draw command"
        );
    }

    #[test]
    fn path_item_holds_path_and_bounds() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 50.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 50.0))
            .stroke(Color::BLACK, 1.5);
        assert_eq!(item.bounds_in_scene(), Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn path_item_per_segment_hit_test_stroke_only() {
        // A diagonal stroke from (0,0) to (100,100) — AABB is the
        // 100×100 square, but only points near the diagonal line
        // should hit. Per-segment distance check: tolerance =
        // stroke_half_width (1) + 2 = 3.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 100.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0))
            .stroke(Color::BLACK, 2.0);

        // Point on the line at the midpoint.
        assert!(item.hit_test(Point::new(50.0, 50.0)));
        // Point 2px from the line (within tolerance 3).
        assert!(item.hit_test(Point::new(52.0, 50.0)));
        // Point 10px from the line (outside tolerance) but inside
        // AABB — should NOT hit with per-segment.
        assert!(!item.hit_test(Point::new(80.0, 20.0)));
        // Outside AABB.
        assert!(!item.hit_test(Point::new(200.0, 200.0)));
    }

    #[test]
    fn path_item_filled_uses_aabb_hit_test() {
        // A filled path's AABB is the visual target — a click
        // anywhere in the AABB hits.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(100.0, 100.0))
            .line_to(Point::new(0.0, 100.0))
            .close();
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0))
            .fill(Color::RED);
        // Point inside the AABB (and the closed quad).
        assert!(item.hit_test(Point::new(50.0, 50.0)));
        // Point outside AABB.
        assert!(!item.hit_test(Point::new(200.0, 50.0)));
    }

    #[test]
    fn path_item_close_segment_hit_tested() {
        // A triangle: stroke-only. The Close command should
        // produce a hit on the closing segment back to the start.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(100.0, 0.0))
            .line_to(Point::new(50.0, 100.0))
            .close();
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0))
            .stroke(Color::BLACK, 2.0);

        // On the diagonal closing segment from (50,100) to (0,0):
        // midpoint is (25, 50).
        assert!(item.hit_test(Point::new(25.0, 50.0)));
    }

    #[test]
    fn path_item_curve_falls_back_to_aabb() {
        // Quad/cubic/arc segments fall back to AABB hit-test
        // (precise curve-distance is out of scope for the
        // built-in). Verify a stroke-only quad behaves like AABB.
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0))
            .quad_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
        let item = PathItem::new(path, Rect::new(0.0, 0.0, 100.0, 100.0))
            .stroke(Color::BLACK, 2.0);
        // Inside AABB but far from any reasonable curve point.
        assert!(item.hit_test(Point::new(50.0, 99.0)));
    }

    #[test]
    fn group_item_does_not_hit_test_through_aabb() {
        // Default `hit_test` AABB-contains; GroupItem overrides to
        // `false` so it never blocks pointer events on items
        // beneath. This is the contract Phase 5/6 will rely on.
        let g = GroupItem::new(Rect::new(0.0, 0.0, 1000.0, 1000.0));
        assert!(!g.hit_test(Point::new(500.0, 500.0)));
    }

    #[test]
    fn text_item_label_falls_back_to_text() {
        let item = TextItem::new("Hello", Rect::new(0.0, 0.0, 100.0, 30.0));
        assert_eq!(SceneItem::label(&item).as_deref(), Some("Hello"));
    }
}
